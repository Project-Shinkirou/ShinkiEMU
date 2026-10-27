use super::*;
use crate::{
    CmvsNativeAudioAction as Action, CmvsNativeAudioControl as Control,
    CmvsNativeAudioQuery as Query,
};

pub(super) fn execute(
    archive: &CmvsArchive,
    vm: &mut CmvsPs2aVmState,
    bridge: &mut audio::AudioBridge,
    native_format: [u32; 2],
    action: CmvsPs2aVmAction,
) -> SessionResult<()> {
    let action = match action {
        CmvsPs2aVmAction::ConfigureResourceAudio { slot, fade_in_ms } => {
            let loaded = native_save_audio::configure_resource(
                archive,
                vm,
                bridge,
                slot,
                fade_in_ms,
                native_format,
            )?;
            vm.write_interpreter_word(81220, u32::from(loaded));
            return Ok(());
        }
        CmvsPs2aVmAction::NativeAudio(action) => action,
        _ => {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CONTRACT",
                "action is not a native audio request",
            ));
        }
    };
    match action {
        Action::LoadFixedStream {
            name,
            volume,
            fade_in_ms,
            looping,
            start,
        } => {
            let channel = audio::FIXED_STREAM_CHANNEL;
            bridge.control_channel(channel, audio::NativeStreamControl::Dispose)?;
            let clip = match native_save_audio::decode_with_extent(
                archive,
                vm,
                &name,
                CmvsResourceSearchBank::Native0d0c,
            ) {
                Ok((Ok(clip), _)) => clip,
                Ok((Err(cause), _)) => {
                    //48DB70 ignores decoder callback status;576 has no extent out-pointer.
                    vm.write_interpreter_word(81220, 1);
                    return Err(core(cause));
                }
                Err(cause) => {
                    bridge.check_failure()?;
                    vm.write_interpreter_word(81220, 0);
                    tracing::warn!(event = "cmvs.session.fixed_sound_load_failed", error = ?cause);
                    return Ok(());
                }
            };
            let master = vm.interpreter_words.get(&1560).copied().unwrap_or(256);
            bridge.configure_fixed_stream(
                clip.with_native_format(native_format[0], native_format[1]),
                looping,
                start,
                volume.wrapping_mul(master) >> 8,
                fade_in_ms,
            )?;
            vm.write_interpreter_word(81220, 1);
            Ok(())
        }
        Action::LoadStatic { slot, name } => {
            let (encoded, _, malformed) = encoding_rs::SHIFT_JIS.encode(&name);
            if malformed {
                return Err(error(
                    "SHINKI_EMU_CMVS_STATIC_AUDIO_SOURCE",
                    "static sound name is not validCP932",
                ));
            }
            let loaded = native_save_audio::configure_static(archive, vm, bridge, slot, &encoded)?;
            vm.write_interpreter_word(81220, u32::from(loaded));
            Ok(())
        }
        Action::FreeStatic { slot } => bridge.clear_static_sound(slot),
        Action::PlayStatic {
            slot,
            volume,
            looping,
        } => {
            let gain = native_save_audio::channel_gain(vm, 9);
            let gain = if (volume as i32) > 0 {
                gain.wrapping_mul(volume) >> 8
            } else {
                gain
            };
            bridge.play_static_sound(slot, gain, Some(looping))
        }
        Action::StopStatic { slot } => bridge.stop_static_sound(slot),
        Action::LoadVoice {
            channel,
            name,
            volume,
            looping,
            rate_bits,
        } => {
            //48DB70 disposes the previous decoder before validating/loading its replacement.
            bridge.control_channel(channel, audio::NativeStreamControl::Dispose)?;
            let (mut clip, extent) = match native_save_audio::decode_with_extent(
                archive,
                vm,
                &name,
                CmvsResourceSearchBank::Native0d14,
            ) {
                Ok((Ok(clip), extent)) => (clip, extent),
                Ok((Err(cause), extent)) => {
                    //48DB70 ignores the load callback's status. A size query
                    //may already have defined its DWORD before PCM use fails.
                    vm.write_interpreter_word(81220, 1);
                    if let Some(extent) = extent {
                        vm.write_interpreter_word(81236, extent);
                    } else {
                        vm.interpreter_words.remove(&81236);
                        vm.native_audio_extent_indeterminate = true;
                    }
                    return Err(core(cause));
                }
                Err(cause) => {
                    bridge.check_failure()?;
                    vm.write_interpreter_word(81220, 0);
                    //489710 still copies its unwritten stack local v12 to81236.
                    vm.interpreter_words.remove(&81236);
                    vm.native_audio_extent_indeterminate = true;
                    tracing::warn!(event = "cmvs.session.voice_sound_load_failed", channel, error = ?cause);
                    return Ok(());
                }
            };
            let mut rate = f32::from_bits(rate_bits);
            if rate < 1.0 || rate > 3.0 {
                rate = 1.0;
            }
            if clip.native_channels() == 1 && f64::from(rate) >= 1.1 && f64::from(rate) <= 3.0 {
                let stretch = crate::audio::native_stretch::NativeMono175::new(
                    clip.source_sample_rate(),
                    rate,
                    looping,
                    native_format[0],
                    native_format[1],
                )
                .map_err(core)?;
                clip = clip.with_native_stretch(stretch);
            } else {
                clip = clip.with_native_format(native_format[0], native_format[1]);
            }
            let gain = native_save_audio::channel_gain(vm, channel);
            let gain = if (volume as i32) > 0 {
                gain.wrapping_mul(volume) >> 8
            } else {
                gain
            };
            bridge.configure_voice(channel, clip, looping, gain)?;
            vm.write_interpreter_word(81220, 1);
            if let Some(extent) = extent {
                vm.write_interpreter_word(81236, extent);
            } else {
                // No native x86 ACM query established an extent.
                vm.interpreter_words.remove(&81236);
                vm.native_audio_extent_indeterminate = true;
            }
            Ok(())
        }
        Action::Control { channel, control } => {
            let control = match control {
                Control::Start { looping } => audio::NativeStreamControl::Start { looping },
                Control::Restart { looping } => audio::NativeStreamControl::Restart { looping },
                Control::Pause => audio::NativeStreamControl::Pause,
                Control::Dispose => audio::NativeStreamControl::Dispose,
                Control::Volume(value) => audio::NativeStreamControl::Volume(value),
                Control::Pan(value) => audio::NativeStreamControl::Pan(value),
                Control::FadeOut { duration_ms } => {
                    audio::NativeStreamControl::FadeOut { duration_ms }
                }
            };
            bridge.control_channel(channel, control)
        }
        Action::Query { channel, query } => {
            let stream = bridge.snapshot(channel, matches!(query, Query::Position))?;
            match query {
                Query::Position => {
                    let position = if stream.playing {
                        stream.position_ms.ok_or_else(|| {
                            error(
                                "SHINKI_EMU_CMVS_NATIVE_AUDIO_POSITION",
                                "playing stream has no observed playback position",
                            )
                        })?
                    } else {
                        u32::MAX
                    };
                    vm.write_interpreter_word(81220, position);
                }
                Query::DecodedBytes => {
                    vm.write_interpreter_word(
                        81220,
                        if stream.playing {
                            stream.decoded_bytes
                        } else {
                            u32::MAX
                        },
                    );
                }
                Query::Levels => {
                    let levels = if stream.playing {
                        stream.levels
                    } else {
                        [0; 2]
                    };
                    vm.write_interpreter_word(81236, levels[0]);
                    vm.write_interpreter_word(81240, levels[1]);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
