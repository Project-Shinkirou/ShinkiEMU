//! Native 772/784/792 audio reload. Projection tags 2176..2179 contain no audio.
//! 475E40 restarts named sources; it never restores the decoder cursor or fade.
use super::*;
use crate::system_save::slot_globals::{
    NativeBgmSources, NativeGlobals, NativeResourceChannel, NativeResourceChannels,
    NativeStaticSounds,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct NativeAudioCapture {
    pub bgm: NativeBgmSources,
    pub resource_channels: NativeResourceChannels,
    pub static_sounds: NativeStaticSounds,
}

impl NativeAudioCapture {
    pub(super) fn write_globals(&self, globals: &mut NativeGlobals) {
        globals.bgm = self.bgm.clone();
        globals.resource_channels = self.resource_channels.clone();
        globals.static_sounds = self.static_sounds.clone();
    }
}

pub(super) fn capture(
    bridge: Option<&audio::AudioBridge>,
    vm: &CmvsPs2aVmState,
    globals: &mut NativeGlobals,
) -> SessionResult<()> {
    if let Some(bridge) = bridge {
        bridge.capture_native_audio(globals)?;
    }
    for (&slot, record) in &vm.resource_channel_slots {
        let target = globals
            .resource_channels
            .slots
            .get_mut(slot as usize)
            .ok_or_else(|| {
                error(
                    "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
                    "native resource audio slot exceeds six entries",
                )
            })?;
        *target = resource_record(vm, slot, record);
    }
    globals.bgm.validate().map_err(core)?;
    globals.resource_channels.validate().map_err(core)?;
    globals.static_sounds.validate().map_err(core)
}

fn resource_record(
    vm: &CmvsPs2aVmState,
    slot: u32,
    record: &CmvsResourceChannelSlotRecord,
) -> NativeResourceChannel {
    let word = |offset, initial| {
        vm.interpreter_words
            .get(&(1608 + slot * 52 + offset))
            .copied()
            .unwrap_or(initial)
    };
    NativeResourceChannel {
        name: record.source.clone(),
        loop_playback: word(32, u32::from(record.loop_playback)),
        volume: word(36, record.volume),
        channel_volume: word(40, 0),
        stereo_pan: word(44, 0),
        restart_mode: word(48, u32::from(record.start_playback)),
    }
}

///424B30 constructor defaults remain current settings, never slot state.
pub(super) fn bgm_gain(vm: &CmvsPs2aVmState) -> u32 {
    setting_product(vm, 1564)
}

pub(super) fn channel_gain(vm: &CmvsPs2aVmState, channel: u8) -> u32 {
    match channel {
        2..=4 => setting_product(vm, 1576),
        5..=8 => setting_product(vm, 1572),
        9..=14 => setting_product(vm, 1568),
        _ => 256,
    }
}

fn setting_product(vm: &CmvsPs2aVmState, category: u32) -> u32 {
    let word = |offset| vm.interpreter_words.get(&offset).copied().unwrap_or(256);
    word(1560).wrapping_mul(word(category)) >> 8
}

///428280 synchronously updates only the native streaming groups. Static
///buffers retain their gain until a later play request computes a new word.
pub(super) fn apply_gain_setting(
    vm: &CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    command_id: u16,
) -> SessionResult<()> {
    let mut channels = [None; 15];
    let mut bgm = None;
    match command_id {
        729 => {
            bgm = Some(bgm_gain(vm));
            for (slot, gain) in channels.iter_mut().enumerate().skip(2) {
                *gain = Some(channel_gain(vm, slot as u8));
            }
        }
        730 => bgm = Some(bgm_gain(vm)),
        731 => {
            for (slot, gain) in channels.iter_mut().enumerate().skip(9) {
                let resource = (slot - 9) as u32;
                let volume = vm
                    .interpreter_words
                    .get(&(1644 + resource * 52))
                    .copied()
                    .or_else(|| {
                        vm.resource_channel_slots
                            .get(&resource)
                            .map(|record| record.volume)
                    })
                    .unwrap_or_else(|| bridge.native_resource_volume(resource as usize));
                let sound = channel_gain(vm, slot as u8);
                *gain = Some(if (volume as i32) > 0 {
                    sound.wrapping_mul(volume) >> 8
                } else {
                    sound
                });
            }
        }
        732 => channels[5..=8].fill(Some(channel_gain(vm, 5))),
        733 => channels[2..=4].fill(Some(channel_gain(vm, 2))),
        _ => {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_GAIN",
                "unknown native audio setting command",
            ));
        }
    }
    bridge.apply_native_gains(bgm, channels)
}

pub(super) fn configure_resource(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    slot: u8,
    fade_in_ms: u32,
    native_format: [u32; 2],
) -> SessionResult<bool> {
    let record = vm
        .resource_channel_slots
        .get(&u32::from(slot))
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
                "resource audio action has no configured descriptor",
            )
        })?;
    let record = resource_record(vm, u32::from(slot), record);
    bridge.control_channel(slot + 9, audio::NativeStreamControl::Dispose)?;
    let playback = match prepare_resource(
        archive,
        vm,
        usize::from(slot),
        &record,
        record.restart_mode != 0,
        native_format,
    ) {
        Ok(playback) => playback,
        Err(cause) => {
            bridge.check_failure()?;
            tracing::warn!(event = "cmvs.session.resource_sound_load_failed", slot, error = ?cause);
            return Ok(false);
        }
    };
    bridge.configure_native_resource(record, playback, fade_in_ms)?;
    Ok(true)
}

/// Native48BE90 deletes the old static object even when the new asset fails.
/// The scene constructor ignores that asset-load result, not mixer failures.
pub(super) fn configure_static(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    slot: u8,
    raw_name: &[u8],
) -> SessionResult<bool> {
    bridge.clear_static_sound(slot)?;
    let name = encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(raw_name)
        .ok_or_else(|| {
            error(
                "SHINKI_EMU_CMVS_STATIC_AUDIO_SOURCE",
                "static sound name is not validCP932",
            )
        })?;
    let clip = match decode(archive, vm, &name, CmvsResourceSearchBank::Native0d0c) {
        Ok(clip) => clip,
        Err(cause) => {
            tracing::warn!(event = "cmvs.session.scene_sound_load_failed", slot, error = ?cause);
            return Ok(false);
        }
    };
    bridge.load_static_sound(slot, name.into_owned(), clip)?;
    Ok(true)
}

fn prepare_resource(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    slot: usize,
    record: &NativeResourceChannel,
    playing: bool,
    native_format: [u32; 2],
) -> SessionResult<audio::NativeChannelPlayback> {
    if slot >= 6 {
        return Err(error(
            "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
            "native resource audio slot exceeds six entries",
        ));
    }
    let clip = decode(
        archive,
        vm,
        &record.name,
        CmvsResourceSearchBank::Native0d0c,
    )?;
    let sound_gain = channel_gain(vm, 9);
    let volume = if (record.volume as i32) > 0 {
        sound_gain.wrapping_mul(record.volume) >> 8
    } else {
        sound_gain
    };
    let pan = record.stereo_pan as i32;
    Ok(audio::NativeChannelPlayback {
        slot: slot + 9,
        clip: clip.with_native_format(native_format[0], native_format[1]),
        looping: record.loop_playback != 0,
        playing,
        native_gain: volume,
        native_volume: if (record.channel_volume as i32) < 256 {
            record.channel_volume
        } else {
            256
        },
        native_pan: pan,
        volume: 1.0,
        stereo_gain: [1.0; 2],
    })
}

pub(super) struct PreparedNativeAudio {
    playback: audio::PreparedNativePlayback,
    capture: NativeAudioCapture,
}

impl PreparedNativeAudio {
    /// Resolves and decodes every named object, including loaded-but-stopped
    /// channels. Preparation neither submits commands nor touches a PCM sink.
    pub(super) fn prepare(
        archive: &CmvsArchive,
        vm: &CmvsPs2aVmState,
        globals: &NativeGlobals,
        native_format: [u32; 2],
    ) -> SessionResult<Self> {
        globals.bgm.validate().map_err(core)?;
        globals.resource_channels.validate().map_err(core)?;
        globals.static_sounds.validate().map_err(core)?;
        let capture = NativeAudioCapture {
            bgm: globals.bgm.clone(),
            resource_channels: globals.resource_channels.clone(),
            static_sounds: globals.static_sounds.clone(),
        };
        // 424B30 initializes game[390..394] to 256. These gains are current
        // settings, not slot data; absent interpreter overrides use that ctor.
        let bgm_gain = bgm_gain(vm);
        let sound_gain = channel_gain(vm, 9);
        let mut bgm = [None, None];
        if !capture.bgm.primary.is_empty() {
            bgm[0] = Some(
                decode(
                    archive,
                    vm,
                    &capture.bgm.primary,
                    CmvsResourceSearchBank::CrossfadeAudio,
                )?
                .with_native_format(native_format[0], native_format[1]),
            );
            if !capture.bgm.secondary.is_empty() {
                bgm[1] = Some(
                    decode(
                        archive,
                        vm,
                        &capture.bgm.secondary,
                        CmvsResourceSearchBank::CrossfadeAudio,
                    )?
                    .with_native_format(native_format[0], native_format[1]),
                );
            }
        } else if !capture.bgm.secondary.is_empty() {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_SOURCE",
                "paired BGM has no primary source",
            ));
        }
        let mut channels = Vec::with_capacity(6 + capture.static_sounds.entries.len());
        for (slot, record) in capture.resource_channels.slots.iter().enumerate() {
            if record.name.is_empty() {
                continue;
            }
            match prepare_resource(
                archive,
                vm,
                slot,
                record,
                record.loop_playback != 0 && record.restart_mode != 0,
                native_format,
            ) {
                Ok(channel) => channels.push(channel),
                Err(cause) => tracing::warn!(
                    event = "cmvs.session.resource_sound_restore_failed", slot, error = ?cause,
                ),
            }
        }
        for record in &capture.static_sounds.entries {
            let clip = decode(
                archive,
                vm,
                &record.name,
                CmvsResourceSearchBank::Native0d0c,
            )?;
            let volume = sound_gain.wrapping_mul(record.volume) >> 8;
            let gain = volume as f32 / 256.0;
            channels.push(audio::NativeChannelPlayback {
                slot: usize::from(record.owner) + 15,
                clip,
                looping: true,
                playing: record.playing != 0,
                native_gain: 256,
                native_volume: 256,
                native_pan: 0,
                volume: 1.0,
                //48B7B0's static attenuation is already a PCM amplitude.
                stereo_gain: [gain * gain; 2],
            });
        }
        Ok(Self {
            playback: audio::PreparedNativePlayback::new(bgm, bgm_gain, channels),
            capture,
        })
    }

    /// The parent calls this only after all candidate subsystems validate.
    pub(super) fn commit(self, bridge: &mut audio::AudioBridge) -> SessionResult<()> {
        bridge.commit_native_audio(self.playback, self.capture)
    }
}

fn decode(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    name: &str,
    bank: CmvsResourceSearchBank,
) -> SessionResult<audio::DecodedClip> {
    decode_with_extent(archive, vm, name, bank).and_then(|(clip, _)| clip.map_err(core))
}

pub(super) fn decode_with_extent(
    archive: &CmvsArchive,
    vm: &CmvsPs2aVmState,
    name: &str,
    bank: CmvsResourceSearchBank,
) -> SessionResult<(
    Result<audio::DecodedClip, shinki_emu_sdk::CoreError>,
    Option<u32>,
)> {
    let (encoded, _, malformed) = encoding_rs::SHIFT_JIS.encode(name);
    if malformed || encoded.is_empty() || encoded.len() >= 32 || encoded.contains(&0) {
        return Err(error(
            "SHINKI_EMU_CMVS_NATIVE_AUDIO_SOURCE",
            "native audio loader requires a nonempty CP932 source shorter than 32 bytes",
        ));
    }
    let uri = archive
        .resolve_native_resource(vm, bank, name)
        .map_err(core)?;
    let decoded = archive
        .load_native_audio(
            &uri,
            name,
            audio::MAX_CLIP_FRAMES,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .map_err(core)?;
    let clip = decoded.data.and_then(|data| {
        if data.sample_rate == 0 || data.frames.is_empty() {
            return Err(shinki_emu_sdk::CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_SOURCE",
                "native audio source decoded no playable PCM",
            ));
        }
        Ok(audio::DecodedClip::from_kira(data)
            .with_source(name.to_owned())
            .with_native_channels(decoded.channels)
            .with_native_repeat(decoded.repeat_data)
            .with_native_restart_rewinds(decoded.restart_rewinds))
    });
    Ok((clip, decoded.extent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_empty_audio_uses_constructor_records() {
        let mut globals = NativeGlobals::default();
        capture(None, &CmvsPs2aVmState::new(0), &mut globals).unwrap();
        // 48E910/424B30/48B880: empty counted BGM names, zeroed six-record
        // resource table, and version-one static manager with no owners.
        assert_eq!(globals.bgm.encode_fields().unwrap(), [1, 0, 0, 1, 0, 0]);
        assert_eq!(globals.resource_channels.encode().unwrap(), [0; 312]);
        assert_eq!(globals.static_sounds.encode().unwrap(), [1, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn configured_resource_capture_owns_name_and_consumes_live_control_words() {
        let mut vm = CmvsPs2aVmState::new(0);
        vm.resource_channel_slots.insert(
            5,
            CmvsResourceChannelSlotRecord {
                source: "wind.ogg".into(),
                start_playback: true,
                volume: 192,
                loop_playback: true,
            },
        );
        let base = 1608 + 5 * 52;
        vm.write_interpreter_word(base + 40, 64);
        vm.write_interpreter_word(base + 44, (-128i32) as u32);
        vm.write_interpreter_word(base + 48, 0);
        let mut globals = NativeGlobals::default();
        capture(None, &vm, &mut globals).unwrap();
        let record = &globals.resource_channels.slots[5];
        assert_eq!(record.name, "wind.ogg");
        assert_eq!(record.volume, 192);
        assert_eq!(record.channel_volume, 64);
        assert_eq!(record.stereo_pan as i32, -128);
        assert_eq!(record.restart_mode, 0);
        let bytes = globals.resource_channels.encode().unwrap();
        assert_eq!(
            NativeResourceChannels::decode(&bytes).unwrap(),
            globals.resource_channels
        );
    }
}
