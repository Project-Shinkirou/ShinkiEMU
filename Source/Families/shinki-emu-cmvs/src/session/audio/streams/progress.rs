use super::*;
use shinki_emu_sdk::{NativeAudioProgress, NativeAudioProgressReader};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(in crate::session::audio) struct PendingDrain {
    pub generation: u64,
    pub end_frame: u64,
}

pub(in crate::session::audio) type PendingDrains =
    Arc<Mutex<[Option<PendingDrain>; CHANNEL_SOUNDS]>>;

/// Output timeline in 48 kHz bus frames, readable while the worker blocks.
pub(in crate::session::audio) trait PlaybackSource: Send + Sync {
    fn progress(&self) -> NativeAudioProgress;
}

impl PlaybackSource for NativeAudioProgressReader {
    fn progress(&self) -> NativeAudioProgress {
        NativeAudioProgressReader::progress(self)
    }
}

pub(in crate::session::audio) type PlaybackClock = Arc<dyn PlaybackSource>;

pub(in crate::session::audio) fn retire_drained(
    clock: &dyn PlaybackSource,
    pending: &PendingDrains,
    generations: &[AtomicU64; CHANNEL_SOUNDS],
) -> SessionResult<()> {
    let mut pending = pending.lock().map_err(|_| {
        error(
            "SHINKI_EMU_CMVS_AUDIO_WORKER",
            "audio drain table is poisoned",
        )
    })?;
    for (channel, drain) in pending.iter_mut().enumerate() {
        if drain
            .is_some_and(|drain| generations[channel].load(Ordering::Acquire) != drain.generation)
        {
            *drain = None;
        }
    }
    if pending.iter().all(Option::is_none) {
        return Ok(());
    }
    let played = clock.progress().played_frames;
    for (channel, drain) in pending.iter_mut().enumerate() {
        if let Some(current) = *drain
            && played >= current.end_frame
        {
            retire_channel(generations, channel as u8, current.generation);
            *drain = None;
        }
    }
    Ok(())
}

#[derive(Default)]
pub(in crate::session::audio) struct PlaybackTimeline {
    ranges: VecDeque<(u64, u64)>,
    consumed_frames: u64,
}

impl PlaybackTimeline {
    pub(in crate::session::audio) fn accept(&mut self, start: u64, frames: u32) {
        if frames == 0 {
            return;
        }
        let end = start + u64::from(frames);
        if let Some(previous) = self.ranges.back_mut().filter(|range| range.1 == start) {
            previous.1 = end;
        } else {
            self.ranges.push_back((start, end));
        }
    }

    /// Counts only this stream's accepted intervals, excluding pause gaps and
    /// other channels. A source underrun never advances the host's played clock.
    pub(in crate::session::audio) fn consumed(&mut self, played_bus_frames: u64) -> u64 {
        while self
            .ranges
            .front()
            .is_some_and(|range| played_bus_frames >= range.1)
        {
            let (start, end) = self.ranges.pop_front().expect("checked playback range");
            self.consumed_frames += end - start;
        }
        self.consumed_frames
            + self.ranges.front().map_or(0, |&(start, end)| {
                played_bus_frames.saturating_sub(start).min(end - start)
            })
    }
}
