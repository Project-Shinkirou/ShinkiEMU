use super::*;
use crate::eden_save::{EdenCheckpoint, EdenHistoryLine, EdenHistoryMessage, EdenStand};

fn name<'a>(uri: &'a str, prefix: &str) -> Result<&'a str, MusicaRuntimeError> {
    uri.strip_prefix(prefix).ok_or(MusicaRuntimeError::State)
}

// Fixed-width native cursors; the drawing font never changes slot geometry.
fn visual_lines(
    text: &str,
    layout: &crate::eden_save::NovelLayout,
) -> Result<usize, MusicaRuntimeError> {
    let visible = crate::parse_musica_message_markup(text)?.visible_text;
    if visible.is_empty() {
        return Ok(0);
    }
    let columns_per_line = layout.body_width / (layout.font / 2);
    Ok(visible
        .split('\n')
        .map(|line| {
            let mut lines = 1;
            let mut columns = 0;
            for character in line.chars() {
                let width = if character.is_ascii() { 1 } else { 2 };
                if columns + width > columns_per_line {
                    lines += 1;
                    columns = 0;
                }
                columns += width;
            }
            lines
        })
        .sum())
}

/// CLog captures *authored* text, never a translation or a replacement hook's text.
pub(super) fn record_message(
    message_id: i64,
    voice: String,
    speaker: String,
    text: String,
    state: &mut MusicaRuntimeState,
) -> Result<(), MusicaRuntimeError> {
    let script = name(&state.script_uri, "musica:/scr/")?.to_owned();
    let (background, background_position, foreground_position, stands) =
        if let Some(stage) = &state.stage {
            let background = stage
                .background
                .as_ref()
                .map(|layer| name(&layer.resource_uri, "musica:/bg/").map(str::to_owned))
                .transpose()?
                .unwrap_or_default();
            let background_position = stage
                .background
                .as_ref()
                .map_or([0, 0], |layer| [layer.x, layer.y]);
            let stands = stage
                .stands
                .iter()
                .enumerate()
                .map(|(slot, layer)| {
                    Ok(EdenStand {
                        slot: u32::try_from(slot).map_err(|_| MusicaRuntimeError::State)?,
                        resource: name(&layer.resource_uri, "musica:/st/")?.to_owned(),
                        position: layer.position,
                        offset: layer.resource_parameter,
                    })
                })
                .collect::<Result<Vec<_>, MusicaRuntimeError>>()?;
            (
                background,
                background_position,
                stage.reference_position.unwrap_or([0, 0]),
                stands,
            )
        } else {
            (String::new(), [0, 0], [0, 0], Vec::new())
        };
    let panel_mode = state.panel.as_ref().map_or(0, |panel| panel.mode);
    let panel_resource = state.panel.as_ref().map_or(Ok(String::new()), |panel| {
        let resource = name(&panel.resource_uri, "musica:/sys/")?;
        Ok::<_, MusicaRuntimeError>(
            if (panel.mode == 1 && resource == "msgPanel.png")
                || (matches!(panel.mode, 2 | 3) && resource == "fullPanel.png")
            {
                String::new()
            } else {
                resource.to_owned()
            },
        )
    })?;
    let (bgm, bgm_volume) = match state.audio.get(&0) {
        Some(audio) if audio.playing => (
            if audio.pan_milli == 0 {
                name(&audio.resource_uri, "musica:/bgm/")?.to_owned()
            } else {
                format!(
                    "{}[100,{}]",
                    name(&audio.resource_uri, "musica:/bgm/")?,
                    audio.pan_milli / 10
                )
            },
            audio.volume_milli / 10,
        ),
        _ => (String::new(), 100),
    };
    let page = if panel_mode == 2 {
        if state
            .native_history
            .last()
            .is_none_or(|last| last.panel_mode != 2 || last.message_id != message_id)
        {
            state.native_novel_page.clear();
        }
        if !state.native_novel_page.is_empty()
            && state
                .native_history
                .last()
                .is_some_and(|last| last.panel_mode == 2)
        {
            state.native_history.pop();
        }
        let layout = state.native_edition.novel_layout();
        let top_y = state
            .native_novel_page
            .last()
            .map_or(Ok(layout.top), |last| {
                let lines = visual_lines(&last.text, &layout)?;
                let advance = i32::try_from(lines)
                    .map_err(|_| MusicaRuntimeError::Overflow)?
                    .checked_mul(layout.line_height as i32)
                    .ok_or(MusicaRuntimeError::Overflow)?;
                last.top_y
                    .checked_add(advance)
                    .ok_or(MusicaRuntimeError::Overflow)
            })?;
        state.native_novel_page.push(EdenHistoryLine {
            voice: voice.clone(),
            speaker: speaker.clone(),
            text: text.clone(),
            top_y,
        });
        state.native_novel_page.clone()
    } else {
        Vec::new()
    };
    let (fire_effect, effect_resource, effect_count, effect_speed) = match &state.firefly {
        Some(firefly) if !firefly.ending => (
            "Firefly".into(),
            name(&firefly.resources[0], "musica:/sys/")?
                .strip_suffix("S.png")
                .ok_or(MusicaRuntimeError::State)?
                .to_owned(),
            i32::try_from(firefly.target_count).map_err(|_| MusicaRuntimeError::Overflow)?,
            i32::try_from(firefly.duration_ms).map_err(|_| MusicaRuntimeError::Overflow)?,
        ),
        Some(_) => ("end".into(), String::new(), -1, -1),
        None => (String::new(), String::new(), -1, -1),
    };
    state.native_history.push(EdenHistoryMessage {
        script,
        next_line: state.pc_line,
        message_id,
        state_message_id: None,
        voice: if panel_mode == 2 {
            String::new()
        } else {
            voice
        },
        speaker: if panel_mode == 2 {
            String::new()
        } else {
            speaker
        },
        text: if panel_mode == 2 { String::new() } else { text },
        page,
        background,
        background_position,
        foreground: state.native_foreground.clone(),
        foreground_position,
        stands,
        panel_mode,
        panel_resource,
        bgm,
        bgm_volume,
        sound_effects: [sound_effect_name(state, 1)?, sound_effect_name(state, 2)?],
        stand_count: state.native_stand_slots,
        transition_ticks: state.transition.param,
        fire_effect,
        effect_resource,
        effect_count,
        effect_speed,
        effect_param: -1,
        transition_method: state.transition.mode,
        transition_resource: state.transition.resource.clone().unwrap_or_default(),
    });
    state.native_message_variables.clone_from(&state.variables);
    state.native_message_saveable = representable(state);
    Ok(())
}

impl MusicaVm {
    pub(crate) fn save_load_prohibited(&self) -> bool {
        self.state
            .global_variables
            .get("prohibitSaveLoad")
            .is_some_and(|value| *value != 0)
    }
    /// Native message boundary; transient animation, movie and unsupported
    /// secondary layers cannot be serialized into the original format.
    pub fn native_checkpoint(&self) -> Result<EdenCheckpoint, MusicaRuntimeError> {
        checkpoint(&self.state, true)
    }
    pub(crate) fn set_native_edition(&mut self, edition: crate::eden_save::EdenEdition) {
        self.state.native_edition = edition;
    }
    pub(crate) fn set_native_auto_cycle(&mut self, cycle: u64) {
        self.native_auto_cycle = cycle;
    }
    pub(crate) fn take_native_autosave(&mut self) -> Option<EdenCheckpoint> {
        self.native_auto_pending.take()
    }
}

pub(super) fn autosave_prohibited(state: &MusicaRuntimeState) -> bool {
    ["exitFromEvent", "prohibitAutoSave"]
        .iter()
        .any(|key| state.variables.get(*key).is_some_and(|value| *value != 0))
        || state
            .global_variables
            .get("prohibitSaveLoad")
            .is_some_and(|value| *value != 0)
}

pub(super) fn checkpoint(
    state: &MusicaRuntimeState,
    strict: bool,
) -> Result<EdenCheckpoint, MusicaRuntimeError> {
    if !state.native_message_saveable {
        return Err(MusicaRuntimeError::State);
    }
    let last = state
        .native_history
        .last()
        .ok_or(MusicaRuntimeError::State)?;
    if strict
        && (last.next_line != state.pc_line
            || state
                .message
                .as_ref()
                .is_none_or(|message| message.message_id != last.message_id))
    {
        return Err(MusicaRuntimeError::State);
    }
    Ok(EdenCheckpoint {
        script: last.script.clone(),
        next_line: last.next_line,
        message_id: last.message_id,
        background: last.background.clone(),
        background_position: last.background_position,
        foreground: last.foreground.clone(),
        foreground_position: last.foreground_position,
        stands: last.stands.clone(),
        script_variables: state.native_message_variables.clone(),
        panel_mode: last.panel_mode,
        panel_resource: last.panel_resource.clone(),
        bgm: last.bgm.clone(),
        bgm_volume: last.bgm_volume,
        transition_ticks: last.transition_ticks,
        history: state.native_history.clone(),
    })
}

fn sound_effect_name(state: &MusicaRuntimeState, id: u32) -> Result<String, MusicaRuntimeError> {
    let Some(audio) = state.audio.get(&id).filter(|audio| audio.playing) else {
        return Ok(String::new());
    };
    let resource = name(&audio.resource_uri, "musica:/se/")?;
    Ok(if audio.volume_milli == 1000 && audio.pan_milli == 0 {
        resource.to_owned()
    } else {
        format!(
            "{resource}[{},{}]",
            audio.volume_milli / 10,
            audio.pan_milli / 10
        )
    })
}

fn representable(state: &MusicaRuntimeState) -> bool {
    state.effect.is_none()
        && state.movie.is_none()
        && state.firefly.is_none()
        && state.secondary_effect.is_none()
        && state.wscroll2.is_none()
        && state.scroll_xf.is_none()
        && state.characters.is_empty()
        && state.message_loads.is_empty()
        && state.transition.mode == 0
        && state.transition.resource.is_none()
        && !state
            .audio
            .iter()
            .any(|(id, audio)| *id != 0 && *id != 4 && audio.playing)
        && !state.audio.get(&0).is_some_and(|audio| {
            audio.playing
                && (!audio.looped || audio.pan_milli % 10 != 0 || audio.volume_milli % 10 != 0)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScOpcodeCatalog;
    fn machine(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/test.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &ScOpcodeCatalog::observed_musica()).unwrap(),
            0,
        )
        .unwrap()
    }
    fn advance(vm: &mut MusicaVm) {
        let token = match vm.state().wait.as_ref().unwrap() {
            MusicaWaitState::Input { token_id } => token_id.clone(),
            _ => panic!("not an input boundary"),
        };
        vm.resolve_wait(&token).unwrap();
        vm.step(vm.state().fixed_tick + 1).unwrap();
    }
    #[test]
    fn periodic_autosave_counts_commands_and_keeps_variables_at_saved_pc() {
        let mut vm = machine(b".set counter = 1\n.message 1   First\n.set counter = counter + 1\n.message 2   Second\n.end\n");
        vm.set_native_auto_cycle(3);
        vm.step(1).unwrap();
        assert!(vm.take_native_autosave().is_none());
        advance(&mut vm);
        let checkpoint = vm.take_native_autosave().unwrap();
        assert_eq!(checkpoint.message_id, 1);
        assert_eq!(checkpoint.script_variables.get("counter"), Some(&1));
        assert_eq!(vm.state().variables.get("counter"), Some(&2));
    }
    #[test]
    fn selection_movie_and_all_three_prohibitions_control_autosave() {
        for suffix in [
            ".select One:done Two:done\n.label done\n.end\n",
            ".movie 1 clip.avi 1280 720 t\n.end\n",
        ] {
            for gate in [
                "",
                ".set exitFromEvent = 1\n",
                ".set prohibitAutoSave = 1\n",
                ".setglobal prohibitSaveLoad = 1\n",
            ] {
                let source = format!(".message 1   First\n{gate}{suffix}");
                let mut vm = machine(source.as_bytes());
                vm.step(1).unwrap();
                advance(&mut vm);
                assert_eq!(vm.take_native_autosave().is_some(), gate.is_empty());
            }
        }
    }
    #[test]
    fn novel_line_advance_uses_native_wrap_and_empty_message_rules() {
        let layout = crate::eden_save::EdenEdition::English.novel_layout();
        assert_eq!(visual_lines("", &layout).unwrap(), 0);
        assert_eq!(visual_lines(&"a".repeat(68), &layout).unwrap(), 1);
        assert_eq!(visual_lines(&"a".repeat(69), &layout).unwrap(), 2);
        assert_eq!(visual_lines("a\\nb", &layout).unwrap(), 2);
        assert_eq!(visual_lines(&"文".repeat(35), &layout).unwrap(), 2);
    }
}
