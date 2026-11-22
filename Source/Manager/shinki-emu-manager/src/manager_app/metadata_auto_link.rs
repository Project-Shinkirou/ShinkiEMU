//! When network access is granted, unlinked games are searched by title one at
//! a time. Link the sole valid candidate or the only exact title match.

use super::*;
use shinki_emu_manager_ui_slint::{describe_error, tr};

/// Game ids whose automatic search has run or whose link was removed by hand.
/// Pruned to library games on every write.
pub(super) const AUTO_LINK_ATTEMPTED_KEY: &str = "metadata.auto_link_attempted";
pub(super) const AUTO_LINK_PREFIX: &str = "auto";
const AUTO_LINK_RESULTS: u8 = 10;

enum AutoLinkStage {
    Search { query: String },
    Fetch { remote_id: String },
}

pub(super) struct AutoLinkRequest {
    pub(super) request_id: String,
    pub(super) game_id: String,
    stage: AutoLinkStage,
}

fn valid_vndb_candidate(record: &MetadataRecord) -> bool {
    record.provider == MetadataProviderId::Vndb
        && record.remote_id.len() <= 256
        && record.remote_id.strip_prefix('v').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
        && std::iter::once(&record.title)
            .chain(&record.alternate_titles)
            .any(|title| !title.trim().is_empty())
}

impl ShinkiEmuManagerController {
    /// Queue every unlinked, not yet attempted game and start the batch.
    /// Does nothing while network access is not allowed.
    pub(super) fn enqueue_auto_link(&mut self) -> Result<(), String> {
        if !self.network_allowed {
            return Ok(());
        }
        let snapshot = self
            .library
            .library_snapshot()
            .map_err(|error| error.to_string())?;
        for game in snapshot.games {
            let linked = snapshot
                .identities
                .get(&game.game_id)
                .is_some_and(|identities| {
                    identities
                        .iter()
                        .any(|identity| identity.provider == MetadataProviderId::Vndb.as_str())
                });
            if linked
                || game.display_title().trim().is_empty()
                || self.auto_link_attempted.contains(&game.game_id)
                || self.auto_link_queue.contains(&game.game_id)
                || self
                    .auto_link
                    .as_ref()
                    .is_some_and(|request| request.game_id == game.game_id)
            {
                continue;
            }
            self.auto_link_queue.push_back(game.game_id);
        }
        self.pump_auto_link()
    }

    /// Submit the next queued search unless an automatic request is running.
    pub(super) fn pump_auto_link(&mut self) -> Result<(), String> {
        if self.auto_link.is_some() {
            return Ok(());
        }
        if !self.network_allowed {
            self.auto_link_queue.clear();
            return Ok(());
        }
        while let Some(game_id) = self.auto_link_queue.pop_front() {
            let Some(game) = self
                .library
                .game(&game_id)
                .map_err(|error| error.to_string())?
            else {
                continue;
            };
            let linked = self
                .library
                .external_identities(&game_id)
                .map_err(|error| error.to_string())?
                .iter()
                .any(|identity| identity.provider == MetadataProviderId::Vndb.as_str());
            let busy = self
                .pending_metadata
                .values()
                .any(|pending| pending.game_id.as_deref() == Some(game_id.as_str()));
            let query = MetadataSearchQuery {
                title: game.display_title().trim().into(),
                aliases: Vec::new(),
                developer: None,
                release_date: None,
                limit: AUTO_LINK_RESULTS,
            };
            if linked
                || busy
                || self.auto_link_attempted.contains(&game_id)
                || query.validate().is_err()
            {
                continue;
            }
            let title = query.title.clone();
            match self.queue_metadata(
                &game,
                MetadataProviderId::Vndb,
                MetadataCommandKind::Search(query),
                AUTO_LINK_PREFIX,
            ) {
                Ok(request_id) => {
                    self.metadata_status
                        .insert(game_id.clone(), MetadataStatus::Message(tr!("正在 VNDB 自动匹配…").into()));
                    self.auto_link = Some(AutoLinkRequest {
                        request_id,
                        game_id,
                        stage: AutoLinkStage::Search { query: title },
                    });
                    return Ok(());
                }
                Err(error) => {
                    self.stop_auto_link(game_id, error);
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Apply a completed automatic request and continue with the next game.
    pub(super) fn finish_auto_link(
        &mut self,
        request: AutoLinkRequest,
        result: Result<MetadataPayload, String>,
    ) -> Result<(), String> {
        let AutoLinkRequest { game_id, stage, .. } = request;
        match (stage, result) {
            (_, Err(error)) => {
                self.stop_auto_link(game_id, error);
                return Ok(());
            }
            (AutoLinkStage::Search { query }, Ok(MetadataPayload::Search(mut records))) => {
                let returned = records.len();
                records.retain(valid_vndb_candidate);
                if returned != 0 && records.is_empty() {
                    self.stop_auto_link(game_id, "SHINKI_EMU_METADATA_RESPONSE_IDENTITY_MISMATCH".into());
                    return Ok(());
                }
                // Ten results at most. Collapse repeated works in place while
                // keeping titles that can disambiguate an exact match.
                let mut index = 1;
                while index < records.len() {
                    if let Some(first) = records[..index]
                        .iter()
                        .position(|record| record.remote_id == records[index].remote_id)
                    {
                        let duplicate = records.remove(index);
                        let original = &mut records[first];
                        original.alternate_titles.push(duplicate.title);
                        original.alternate_titles.extend(duplicate.alternate_titles);
                    } else {
                        index += 1;
                    }
                }
                let count = records.len();
                let matched = match records.as_slice() {
                    [record] => Some(record.remote_id.clone()),
                    _ => unique_exact_match(&query, &records)
                        .map(|index| records[index].remote_id.clone()),
                };
                self.match_candidates
                    .retain(|_, (candidate_game, _)| *candidate_game != game_id);
                self.store_match_candidates(&game_id, MetadataProviderId::Vndb, records);
                if let Some(remote_id) = matched.filter(|_| self.network_allowed) {
                    let game = self.game(&game_id)?;
                    match self.queue_metadata(
                        &game,
                        MetadataProviderId::Vndb,
                        MetadataCommandKind::Fetch(remote_id.clone()),
                        AUTO_LINK_PREFIX,
                    ) {
                        Ok(request_id) => {
                            self.metadata_status
                                .insert(game_id.clone(), MetadataStatus::Message(tr!("正在自动关联…").into()));
                            self.auto_link = Some(AutoLinkRequest {
                                request_id,
                                game_id,
                                stage: AutoLinkStage::Fetch { remote_id },
                            });
                        }
                        Err(error) => self.stop_auto_link(game_id, error),
                    }
                    return Ok(());
                }
                self.metadata_status.insert(
                    game_id.clone(),
                    MetadataStatus::Message(if count == 0 {
                        tr!("未在 VNDB 找到匹配作品").into()
                    } else {
                        tr!("未能自动关联，找到 {count} 个候选，请手动选择", count = count)
                    }),
                );
                if self.network_allowed {
                    self.mark_auto_link_attempted(&game_id)?;
                }
            }
            (AutoLinkStage::Fetch { remote_id }, Ok(MetadataPayload::Fetch { record, cover })) => {
                if !self.network_allowed {
                    self.metadata_status.remove(&game_id);
                    self.auto_link_queue.clear();
                    return Ok(());
                }
                if !valid_vndb_candidate(&record) || record.remote_id != remote_id {
                    self.stop_auto_link(game_id, "SHINKI_EMU_METADATA_RESPONSE_IDENTITY_MISMATCH".into());
                    return Ok(());
                }
                let identities = self
                    .library
                    .external_identities(&game_id)
                    .map_err(|error| error.to_string())?;
                if Self::vndb_id(&identities).is_some() {
                    self.metadata_status.remove(&game_id);
                    return self.pump_auto_link();
                }
                let title = match self.link_metadata(&game_id, MetadataProviderId::Vndb, *record, cover) {
                    Ok(title) => title,
                    Err(error) => {
                        self.stop_auto_link(game_id, error);
                        return Ok(());
                    }
                };
                self.metadata_status
                    .insert(game_id.clone(), MetadataStatus::Message(tr!("已自动关联：{title}", title = title)));
                self.mark_auto_link_attempted(&game_id)?;
            }
            (_, Ok(_)) => return Err("SHINKI_EMU_METADATA_RESPONSE_UNEXPECTED".into()),
        }
        self.pump_auto_link()
    }

    /// End the batch after a failed request. Keep its error with the affected
    /// game and let the remaining games retry at the next trigger.
    fn stop_auto_link(&mut self, game_id: String, error: String) {
        tracing::warn!(
            event = "shinki.emu.metadata.auto_link_stopped",
            remaining = self.auto_link_queue.len(),
            diagnostic_code = %error
        );
        self.auto_link_queue.clear();
        let reason = describe_error(&error);
        let message = if reason.detail.is_empty() {
            reason.title
        } else {
            reason.detail
        };
        self.metadata_status.insert(game_id, MetadataStatus::Error(message));
    }

    pub(super) fn mark_auto_link_attempted(&mut self, game_id: &str) -> Result<(), String> {
        let present = self
            .library
            .list_games()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|game| game.game_id)
            .collect::<BTreeSet<_>>();
        self.auto_link_attempted.insert(game_id.to_owned());
        self.auto_link_attempted.retain(|id| present.contains(id));
        self.library
            .set_app_state(AUTO_LINK_ATTEMPTED_KEY, &self.auto_link_attempted)
            .map_err(|error| error.to_string())
    }
}
