//! Native streaming objects (manager slots5..15), distinct from static buffers.
use super::*;
pub(super) mod native_pcm;
pub(super) mod progress;

pub(in crate::session) const FIXED_STREAM_CHANNEL: u8 = 39;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session) enum NativeStreamControl {
    Pause,
    Start { looping: bool },
    Restart { looping: bool },
    Volume(u32),
    Pan(i32),
    Dispose,
    FadeOut { duration_ms: u32 },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::session) struct NativeStreamSnapshot {
    pub exists: bool,
    pub playing: bool,
    pub position_ms: Option<u32>,
    pub decoded_bytes: u32,
    pub levels: [u32; 2],
    pub marker: NativeStreamMarker,
}

/// Accepted PCM queue coordinates, not a device-consumed or audible clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::session) struct NativeStreamMarker {
    pub generation: u64,
    pub accepted_bus_frames: u64,
    pub accepted_start_frame: Option<u64>,
    pub accepted_end_frame: Option<u64>,
    pub accepted_generation_frames: u64,
    pub generation_output_frame_base: u64,
    pub accepted_output_frames: u64,
    pub generation_decoder_source_frame_bits: u64,
    /// Decoder read-ahead position; deliberately separate from buffered input.
    pub decoder_source_frame_bits: u64,
    pub input_buffer_frame_bits: u64,
    pub input_sample_rate: u32,
    pub original_source_rate: u32,
    pub looping: bool,
}

pub(super) struct NativeFade {
    duration_ms: u32,
    elapsed_frames: u64,
    pub(super) fade_in: bool,
}

impl NativeFade {
    pub(super) fn new(duration_ms: u32, fade_in: bool) -> Self {
        Self {
            duration_ms,
            elapsed_frames: 0,
            fade_in,
        }
    }

    fn elapsed_ms(&self) -> u32 {
        (self.elapsed_frames.wrapping_mul(1000) / u64::from(OUTPUT_RATE)) as u32
    }

    pub(super) fn advance_frame(&mut self) {
        self.elapsed_frames = self.elapsed_frames.wrapping_add(1);
    }

    pub(super) fn finished(&self) -> bool {
        self.elapsed_ms() >= self.duration_ms
    }

    fn disposes_after_frames(&self, frames: u64) -> bool {
        !self.fade_in
            && (self.elapsed_frames.wrapping_add(frames).wrapping_mul(1000)
                / u64::from(OUTPUT_RATE)) as u32
                >= self.duration_ms
    }

    pub(super) fn envelope(&self) -> u32 {
        if self.finished() {
            return if self.fade_in { 256 } else { 0 };
        }
        //48D600 compares unsigned elapsed before division, including duration0.
        let fraction = self.elapsed_ms().wrapping_shl(8) / self.duration_ms;
        if self.fade_in {
            fraction
        } else {
            256u32.wrapping_sub(fraction)
        }
    }
}

fn check_channel(channel: u8) -> SessionResult<()> {
    if (5..15).contains(&channel) || channel == FIXED_STREAM_CHANNEL {
        Ok(())
    } else {
        Err(error(
            "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
            "native stream channel must be5..14 or fixed-stream bus39",
        ))
    }
}

impl AudioBridge {
    pub(super) fn publish_generation(&mut self, channel: u8) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = self.next_generation;
        // Publish before enqueueing so an older, output-blocked EOS cannot clear it.
        if let Some(slot) = self.channel_generations.get(usize::from(channel)) {
            slot.store(generation, Ordering::Release);
        }
        generation
    }

    pub(in crate::session) fn configure_voice(
        &mut self,
        channel: u8,
        clip: DecodedClip,
        looping: bool,
        native_gain: u32,
    ) -> SessionResult<()> {
        if !(5..9).contains(&channel) {
            return Err(error(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CHANNEL",
                "voice stream channel must be5..8",
            ));
        }
        // The command175 codec-specific extent is computed by the archive adapter;
        // it is not the expanded stereo clip's PCM byte length.
        self.play_channel(channel, clip, looping, native_gain)
    }

    pub(in crate::session) fn configure_fixed_stream(
        &mut self,
        clip: DecodedClip,
        looping: bool,
        playing: bool,
        native_gain: u32,
        fade_in_ms: u32,
    ) -> SessionResult<()> {
        let mut stream = Worker::start_stream(clip, looping, 0, false, native_gain);
        stream.playing = playing;
        if playing {
            stream.begin_generation(self.publish_generation(FIXED_STREAM_CHANNEL));
            if fade_in_ms != 0 {
                stream.native_fade = Some(NativeFade::new(fade_in_ms, true));
            }
        } else {
            self.channel_generations[usize::from(FIXED_STREAM_CHANNEL)].store(0, Ordering::Release);
        }
        let generation = stream.generation;
        if let Err(cause) = self.submit(Command::ConfigureNativeChannel {
            channel: usize::from(FIXED_STREAM_CHANNEL),
            stream: Box::new(stream),
        }) {
            retire_channel(&self.channel_generations, FIXED_STREAM_CHANNEL, generation);
            return Err(cause);
        }
        Ok(())
    }

    pub(in crate::session) fn snapshot(
        &self,
        channel: u8,
        observe_position: bool,
    ) -> SessionResult<NativeStreamSnapshot> {
        check_channel(channel)?;
        self.check_failure()?;
        if self.cancelled.load(Ordering::Acquire) {
            return Err(error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "stream query requested after shutdown",
            ));
        }
        let (reply, result) = std::sync::mpsc::channel();
        self.submit(Command::StreamSnapshot {
            channel,
            observe_position,
            reply,
        })?;
        // The worker cannot consume this request until prior output writes complete.
        let Ok(snapshot) = result.recv() else {
            self.check_failure()?;
            record_failure(&self.failure, "CMVS audio worker dropped a stream query");
            return Err(error(
                "SHINKI_EMU_CMVS_AUDIO_WORKER",
                "CMVS audio worker dropped a stream query",
            ));
        };
        self.check_failure()?;
        Ok(snapshot)
    }

    pub(in crate::session) fn control_channel(
        &mut self,
        channel: u8,
        control: NativeStreamControl,
    ) -> SessionResult<()> {
        check_channel(channel)?;
        self.check_failure()?;
        let generation = match control {
            NativeStreamControl::Start { .. } | NativeStreamControl::Restart { .. } => {
                self.publish_generation(channel)
            }
            NativeStreamControl::Pause | NativeStreamControl::Dispose => {
                self.channel_generations[usize::from(channel)].store(0, Ordering::Release);
                0
            }
            _ => 0,
        };
        let result = self.submit(Command::StreamControl {
            channel,
            generation,
            control,
        });
        if result.is_err() && generation != 0 {
            retire_channel(&self.channel_generations, channel, generation);
        }
        result
    }
}

impl Worker {
    pub(super) fn stream_snapshot(
        &mut self,
        channel: u8,
        accepted_bus_frames: u64,
        observe_position: bool,
        clock: &dyn progress::PlaybackSource,
    ) -> NativeStreamSnapshot {
        let channel = usize::from(channel);
        if let Some(stream) = self.channels[channel].as_mut() {
            let position_ms = (observe_position && stream.playing).then(|| {
                let played = clock.progress().played_frames;
                (stream.playback.consumed(played) * 1000 / u64::from(OUTPUT_RATE)) as u32
            });
            stream.snapshot(accepted_bus_frames, position_ms)
        } else {
            let mut marker = self.retired_stream_markers[channel];
            marker.accepted_bus_frames = accepted_bus_frames;
            NativeStreamSnapshot {
                marker,
                ..Default::default()
            }
        }
    }

    pub(super) fn control_stream(
        &mut self,
        channel: u8,
        generation: u64,
        control: NativeStreamControl,
        generations: &[AtomicU64; CHANNEL_SOUNDS],
        accepted_bus_frames: u64,
    ) {
        let slot = &mut self.channels[usize::from(channel)];
        let Some(stream) = slot.as_mut() else {
            retire_channel(generations, channel, generation);
            return;
        };
        match control {
            NativeStreamControl::Pause => {
                stream.playing = false;
                retire_channel(generations, channel, stream.generation);
            }
            NativeStreamControl::Start { looping } => {
                if !stream.playing {
                    stream.playing = true;
                    stream.looping = looping;
                    if let Some(pcm) = stream.clip.native_pcm.as_mut()
                        && pcm.start(looping)
                    {
                        Arc::get_mut(&mut stream.clip.frames)
                            .expect("exclusive native PCM buffer")
                            .clear();
                        stream.position = 0.0;
                        stream.clip.indeterminate_tail = false;
                    }
                }
                stream.begin_generation(generation);
            }
            NativeStreamControl::Restart { looping } => {
                if stream.clip.native_restart_rewinds {
                    stream.position = 0.0;
                    if let Some(pcm) = stream.clip.native_pcm.as_mut() {
                        pcm.restart(looping);
                        Arc::get_mut(&mut stream.clip.frames)
                            .expect("exclusive native PCM buffer")
                            .clear();
                        stream.clip.indeterminate_tail = false;
                    }
                } else if let Some(pcm) = stream.clip.native_pcm.as_mut()
                    && pcm.start(looping)
                {
                    Arc::get_mut(&mut stream.clip.frames)
                        .expect("exclusive native PCM buffer")
                        .clear();
                    stream.position = 0.0;
                    stream.clip.indeterminate_tail = false;
                }
                stream.playing = true;
                stream.looping = looping;
                stream.begin_generation(generation);
                //48CA10 resets the decoder, not accumulated decoded bytes148.
            }
            NativeStreamControl::Volume(volume) => stream.native_volume = volume,
            NativeStreamControl::Pan(pan) => stream.native_pan = pan,
            NativeStreamControl::Dispose => {
                retire_channel(generations, channel, stream.generation);
                self.retired_stream_markers[usize::from(channel)] =
                    stream.terminal_marker(accepted_bus_frames);
                *slot = None;
            }
            NativeStreamControl::FadeOut { duration_ms } => {
                if stream
                    .native_fade
                    .as_ref()
                    .is_some_and(|fade| !fade.fade_in)
                {
                    return;
                }
                if duration_ms == 0 {
                    retire_channel(generations, channel, stream.generation);
                    self.retired_stream_markers[usize::from(channel)] =
                        stream.terminal_marker(accepted_bus_frames);
                    *slot = None;
                } else {
                    stream.fade = None;
                    stream.native_fade = Some(NativeFade::new(duration_ms, false));
                }
            }
        }
    }
}

impl Stream {
    fn decoder_source_frame(&self) -> f64 {
        self.clip
            .native_pcm
            .as_ref()
            .map_or(self.position, |pcm| pcm.decoder_source_frame())
    }

    pub(super) fn begin_generation(&mut self, generation: u64) {
        self.generation = generation;
        self.marker = NativeStreamMarker {
            generation_output_frame_base: self.output_frames,
            generation_decoder_source_frame_bits: self.decoder_source_frame().to_bits(),
            ..Default::default()
        };
        self.pending_output_frames = 0;
    }

    fn marker_snapshot(&self, accepted_bus_frames: u64) -> NativeStreamMarker {
        NativeStreamMarker {
            generation: self.generation,
            accepted_bus_frames,
            accepted_output_frames: self.output_frames,
            decoder_source_frame_bits: self.decoder_source_frame().to_bits(),
            input_buffer_frame_bits: self.position.to_bits(),
            input_sample_rate: self.clip.sample_rate,
            original_source_rate: self.clip.source_sample_rate(),
            looping: self.looping,
            ..self.marker
        }
    }

    pub(super) fn terminal_marker(&self, accepted_bus_frames: u64) -> NativeStreamMarker {
        let mut marker = self.marker_snapshot(accepted_bus_frames);
        if marker.accepted_end_frame.is_none() {
            marker.accepted_end_frame = Some(
                marker
                    .accepted_start_frame
                    .map_or(accepted_bus_frames, |start| {
                        start.wrapping_add(marker.accepted_generation_frames)
                    }),
            );
        }
        marker
    }

    pub(super) fn accept_chunk(&mut self, bus_start_frame: u64) {
        self.playback
            .accept(bus_start_frame, self.pending_output_frames);
        if self.pending_output_frames != 0 {
            self.marker
                .accepted_start_frame
                .get_or_insert(bus_start_frame);
            self.marker.accepted_generation_frames = self
                .marker
                .accepted_generation_frames
                .wrapping_add(u64::from(self.pending_output_frames));
        }
        if self.finished() {
            self.marker.accepted_end_frame = Some(if self.pending_output_frames != 0 {
                bus_start_frame.wrapping_add(u64::from(self.pending_output_frames))
            } else {
                self.marker
                    .accepted_start_frame
                    .map_or(bus_start_frame, |start| {
                        start.wrapping_add(self.marker.accepted_generation_frames)
                    })
            });
        }
    }

    pub(super) fn set_continuation(&mut self, mut continuation: Option<DecodedClip>) {
        if let (Some(pcm), Some(next)) = (self.clip.native_pcm.as_mut(), continuation.as_mut())
            && let Some(next_pcm) = next.native_pcm.take()
        {
            pcm.set_continuation(*next_pcm);
            return;
        }
        self.continuation = continuation;
    }

    /// Limit the whole bus before consuming any stream, so a partial known
    /// prefix never advances another channel past PCM actually sent to the output.
    pub(super) fn prepare_for_mix(&mut self, frames: usize) -> Option<usize> {
        self.pending_output_frames = 0;
        if !self.playing {
            return None;
        }
        if let Some(pcm) = self.clip.native_pcm.as_mut() {
            let frames = Arc::get_mut(&mut self.clip.frames).expect("exclusive native PCM buffer");
            self.clip.indeterminate_tail = pcm.refill(frames, &mut self.position);
        }
        if !self.clip.indeterminate_tail {
            return None;
        }
        let source_frames = self.clip.frames.len() / 2;
        let step = f64::from(self.clip.sample_rate) / f64::from(OUTPUT_RATE);
        let mut position = self.position;
        for frame in 0..frames {
            if self
                .native_fade
                .as_ref()
                .is_some_and(|fade| fade.disposes_after_frames(frame as u64 + 1))
            {
                return None;
            }
            let index = position.floor() as usize;
            let fraction = (position - index as f64) as f32;
            if index >= source_frames || fraction != 0.0 && index + 1 >= source_frames {
                return Some(frame);
            }
            position += step;
        }
        None
    }

    pub(super) fn native_channel_gains(&self) -> [u32; 2] {
        //48CE30 uses wrapping u32 products and logical shifts; gains>=256
        //bypass sample scaling. Negative raw cap/pan is not a signed clamp.
        let base = self
            .native_mix_word()
            .wrapping_mul(self.native_volume)
            .wrapping_mul(256)
            >> 16;
        let attenuation = 256i32.wrapping_sub(self.native_pan.wrapping_abs()) as u32;
        let pan = if self.native_pan > 0 {
            [attenuation, 256]
        } else if self.native_pan < 0 {
            [256, attenuation]
        } else {
            [256, 256]
        };
        pan.map(|gain| base.wrapping_mul(gain) >> 8)
    }

    pub(super) fn fade_disposes(&self) -> bool {
        self.native_fade
            .as_ref()
            .is_some_and(|fade| !fade.fade_in && fade.finished())
            || (self.volume <= 0.0
                && self
                    .fade
                    .as_ref()
                    .is_some_and(|fade| fade.removes_at_target))
    }

    pub(super) fn snapshot(
        &self,
        accepted_bus_frames: u64,
        position_ms: Option<u32>,
    ) -> NativeStreamSnapshot {
        NativeStreamSnapshot {
            exists: true,
            playing: self.playing,
            position_ms,
            decoded_bytes: self.clip.native_pcm.as_ref().map_or_else(
                || {
                    (self.decoded_frames.floor() as u64)
                        .wrapping_mul(u64::from(self.clip.native_channels) * 2)
                        as u32
                },
                |pcm| pcm.decoded_bytes,
            ),
            //48CE30 writes old index then advances;48C2A0 reads the new index.
            levels: if self.playing {
                self.clip
                    .native_pcm
                    .as_ref()
                    .map_or(self.level_ring[self.level_index], |pcm| pcm.levels())
            } else {
                [0; 2]
            },
            marker: self.marker_snapshot(accepted_bus_frames),
        }
    }
}

#[cfg(test)]
mod tests;
