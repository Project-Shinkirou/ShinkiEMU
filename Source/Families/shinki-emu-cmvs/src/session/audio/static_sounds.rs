//! Native48C920 duplicates a retained static buffer;48C9E0 plays its main
//! buffer and48DB50 stops only that main buffer. Streaming stop48CAB0 is not
//! part of this bank. One-shot duplicates are not persisted by native792.
use super::*;
use crate::system_save::slot_globals::NativeStaticSound;

pub(super) const STATIC_COUNT: usize = 24;
const STATIC_BASE: usize = 15;
const DUPLICATE_LIMIT: usize = 32;

pub(super) enum StaticCommand {
    Load {
        slot: u8,
        clip: DecodedClip,
    },
    Clear {
        slot: u8,
    },
    Duplicate {
        slot: u8,
        amplitude: f32,
    },
    Main {
        slot: u8,
        amplitude: f32,
        looping: bool,
    },
    Stop {
        slot: u8,
    },
}

fn check_slot(slot: u8) -> SessionResult<()> {
    if usize::from(slot) < STATIC_COUNT {
        Ok(())
    } else {
        Err(error(
            "SHINKI_EMU_CMVS_STATIC_AUDIO_SLOT",
            "native static sound slot exceeds23",
        ))
    }
}

impl AudioBridge {
    ///48BE90 destroys the previous object before attempting to load the name.
    pub(in crate::session) fn clear_static_sound(&mut self, slot: u8) -> SessionResult<()> {
        check_slot(slot)?;
        self.submit(Command::Static(StaticCommand::Clear { slot }))?;
        self.native
            .static_sounds
            .entries
            .retain(|record| record.owner != slot);
        Ok(())
    }

    pub(in crate::session) fn load_static_sound(
        &mut self,
        slot: u8,
        name: String,
        clip: DecodedClip,
    ) -> SessionResult<()> {
        check_slot(slot)?;
        self.submit(Command::Static(StaticCommand::Load { slot, clip }))?;
        let entries = &mut self.native.static_sounds.entries;
        let record = NativeStaticSound {
            owner: slot,
            name,
            playing: 0,
            volume: 256,
        };
        if let Some(index) = entries.iter().position(|record| record.owner == slot) {
            entries[index] = record;
        } else {
            entries.push(record);
        }
        entries.sort_by_key(|record| record.owner);
        Ok(())
    }

    pub(in crate::session) fn play_static_sound(
        &mut self,
        slot: u8,
        volume: u32,
        main_loop: Option<bool>,
    ) -> SessionResult<()> {
        check_slot(slot)?;
        self.check_failure()?;
        let Some(record) = self
            .native
            .static_sounds
            .entries
            .iter_mut()
            .find(|record| record.owner == slot)
        else {
            // Genuine null buffer after a failed load:48C920/48C9E0 skip it.
            return Ok(());
        };
        let gain_word = volume.wrapping_mul(record.volume) >> 8;
        //48B6C0/48B7B0 use4000*log10(gain): the PCM amplitude is gain squared,
        //matching the existing native792 restore adapter.
        let gain = gain_word as f32 / 256.0;
        let amplitude = gain * gain;
        let command = match main_loop {
            None => StaticCommand::Duplicate { slot, amplitude },
            Some(looping) => {
                //48B7B0 only sets this flag on the looping branch; once-shot
                //main playback does not clear a previously set native flag.
                if looping {
                    record.playing = 1;
                }
                StaticCommand::Main {
                    slot,
                    amplitude,
                    looping,
                }
            }
        };
        self.submit(Command::Static(command))
    }

    pub(in crate::session) fn stop_static_sound(&mut self, slot: u8) -> SessionResult<()> {
        check_slot(slot)?;
        self.submit(Command::Static(StaticCommand::Stop { slot }))?;
        if let Some(record) = self
            .native
            .static_sounds
            .entries
            .iter_mut()
            .find(|record| record.owner == slot)
        {
            record.playing = 0;
        }
        Ok(())
    }
}

impl Worker {
    pub(super) fn static_command(&mut self, command: StaticCommand) {
        let slot = match &command {
            StaticCommand::Load { slot, .. }
            | StaticCommand::Clear { slot }
            | StaticCommand::Duplicate { slot, .. }
            | StaticCommand::Main { slot, .. }
            | StaticCommand::Stop { slot } => usize::from(*slot),
        };
        let channel = STATIC_BASE + slot;
        match command {
            StaticCommand::Load { clip, .. } => {
                let voices = &mut self.static_voices[slot];
                voices.clear();
                if voices.capacity() < DUPLICATE_LIMIT {
                    voices.reserve_exact(DUPLICATE_LIMIT);
                }
                let mut stream = Self::start_stream(clip, false, 0, false, 256);
                stream.playing = false;
                stream.native_mixer = false;
                self.channels[channel] = Some(stream);
            }
            StaticCommand::Clear { .. } => {
                self.channels[channel] = None;
                self.static_voices[slot].clear();
            }
            StaticCommand::Duplicate { amplitude, .. } => {
                let voices = &mut self.static_voices[slot];
                if voices.len() >= DUPLICATE_LIMIT {
                    return;
                }
                let Some(main) = self.channels[channel].as_ref() else {
                    return;
                };
                let clip = DecodedClip {
                    sample_rate: main.clip.sample_rate,
                    frames: Arc::clone(&main.clip.frames),
                    source: None,
                    native_channels: main.clip.native_channels,
                    indeterminate_tail: false,
                    native_restart_rewinds: true,
                    native_repeat: None,
                    native_pcm: None,
                };
                let mut stream = Self::start_stream(clip, false, 0, false, 256);
                stream.native_mixer = false;
                stream.stereo_gain = [amplitude; 2];
                voices.push(stream);
            }
            StaticCommand::Main {
                amplitude, looping, ..
            } => {
                if let Some(stream) = self.channels[channel].as_mut() {
                    stream.playing = true;
                    stream.looping = looping;
                    stream.volume = 1.0;
                    stream.stereo_gain = [amplitude; 2];
                    stream.fade = None;
                }
            }
            StaticCommand::Stop { .. } => {
                if let Some(stream) = self.channels[channel].as_mut() {
                    //48B860 calls Stop but never SetCurrentPosition.
                    stream.playing = false;
                }
            }
        }
    }

    pub(super) fn mix_static_duplicates(&mut self, mix: &mut [f32]) {
        for voices in &mut self.static_voices {
            for voice in voices.iter_mut() {
                mix_stream(voice, mix);
            }
            voices.retain(|voice| !voice.finished());
        }
    }
}

#[cfg(test)]
mod tests;
