use super::*;
pub(crate) fn is_gallery(page: &MusicaSystemPage) -> bool {
    matches!(
        page,
        MusicaSystemPage::Memories
            | MusicaSystemPage::GalleryCg
            | MusicaSystemPage::GalleryBgm
            | MusicaSystemPage::GalleryReplay
            | MusicaSystemPage::GalleryMovie
    )
}
#[cfg(test)]
pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if is_gallery(&state.system_ui.page)
        && (state.launch_mode != MusicaLaunchMode::Title
            || state.wait.is_some()
            || state.terminal
            || state.system_ui.focus_index >= 4096)
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(())
}
impl MusicaVm {
    pub(crate) fn gallery_pages(&self) -> impl Iterator<Item = MusicaSystemPage> + '_ {
        [
            MusicaSystemPage::GalleryCg,
            MusicaSystemPage::GalleryReplay,
            MusicaSystemPage::GalleryBgm,
            MusicaSystemPage::GalleryMovie,
        ]
        .into_iter()
        .filter(|page| self.gallery_entries(page).next().is_some())
        .chain(std::iter::once(MusicaSystemPage::Title))
    }
    pub(crate) fn gallery_count(&self, page: &MusicaSystemPage) -> Option<u32> {
        if *page == MusicaSystemPage::Memories {
            Some(self.gallery_pages().count() as u32)
        } else if is_gallery(page) {
            Some(self.gallery_entries(page).count() as u32)
        } else {
            None
        }
    }
    pub(crate) fn set_gallery_page(
        &mut self,
        page: MusicaSystemPage,
        focus: u32,
    ) -> Result<(), MusicaRuntimeError> {
        if self.state.launch_mode != MusicaLaunchMode::Title
            || self.state.wait.is_some()
            || self.state.terminal
            || !(self.state.system_ui.page == MusicaSystemPage::Title
                || is_gallery(&self.state.system_ui.page))
        {
            return Err(MusicaRuntimeError::State);
        }
        let count = if page == MusicaSystemPage::Title {
            self.title()?.buttons.len() as u32
        } else {
            self.gallery_count(&page).ok_or(MusicaRuntimeError::State)?
        };
        if focus >= count {
            return Err(MusicaRuntimeError::State);
        }
        self.state.system_ui.page = page;
        self.state.system_ui.focus_index = focus;
        Ok(())
    }
    pub(crate) fn move_gallery_focus(&mut self, delta: i32) -> Result<(), MusicaRuntimeError> {
        let count = self
            .gallery_count(&self.state.system_ui.page)
            .filter(|count| *count > 0)
            .ok_or(MusicaRuntimeError::State)?;
        let focus = (i64::from(self.state.system_ui.focus_index) + i64::from(delta))
            .rem_euclid(i64::from(count)) as u32;
        self.set_gallery_page(self.state.system_ui.page.clone(), focus)
    }
    pub(crate) fn gallery_bgm_play(
        &mut self,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        if self.state.system_ui.page != MusicaSystemPage::GalleryBgm {
            return Err(MusicaRuntimeError::State);
        }
        let uri = self
            .gallery_entries(&MusicaSystemPage::GalleryBgm)
            .nth(self.state.system_ui.focus_index as usize)
            .ok_or(MusicaRuntimeError::State)?
            .resource
            .clone();
        let mut commands = Vec::new();
        if self.state.audio.get(&0).is_some_and(|a| a.playing) {
            commands.push(MusicaAudioCommand::Stop {
                sequence: next_effect_sequence(&mut self.state)?,
                stream_id: 0,
                fade_ms: 0,
            });
        }
        audio_commands::append_audio_load_and_play(
            &mut self.state,
            &mut commands,
            0,
            &uri,
            1000,
            0,
            true,
            0,
        )?;
        self.state.audio.insert(
            0,
            MusicaAudioState {
                bus: "bgm".into(),
                resource_uri: uri,
                looped: true,
                volume_milli: 1000,
                pan_milli: 0,
                playing: true,
                continuation_pts: 0,
            },
        );
        Ok(commands)
    }
    pub(crate) fn gallery_bgm_stop(
        &mut self,
    ) -> Result<Vec<MusicaAudioCommand>, MusicaRuntimeError> {
        if !matches!(
            self.state.system_ui.page,
            MusicaSystemPage::GalleryBgm | MusicaSystemPage::Memories
        ) {
            return Err(MusicaRuntimeError::State);
        }
        match audio_commands::stop_audio_stream(&mut self.state, 0, 0)? {
            Some(MusicaVmEvent::Audio { commands }) => Ok(commands),
            None => Ok(Vec::new()),
            _ => Err(MusicaRuntimeError::State),
        }
    }
}
