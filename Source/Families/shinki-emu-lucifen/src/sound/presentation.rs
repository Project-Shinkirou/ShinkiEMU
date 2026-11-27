use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Presentation {
    queued: VecDeque<(PlaybackSpan, u64)>,
    pub(super) time_ms: i32,
}

impl Presentation {
    pub(super) fn reset(&mut self) {
        self.queued.clear();
        self.time_ms = 0;
    }

    pub(super) fn push(&mut self, span: PlaybackSpan, end: u64) {
        self.queued.push_back((span, end));
    }

    pub(super) fn pending(&self) -> bool {
        !self.queued.is_empty()
    }

    pub(super) fn present(&mut self, played: u64) {
        while let Some(&(span, end)) = self.queued.front() {
            if played <= span.begin {
                break;
            }
            self.time_ms = span.time_at(played.min(end));
            if played < end {
                break;
            }
            self.queued.pop_front();
        }
    }
}

/// A continuous run of one clip at a fixed rate, in the SDK's submitted bus frames.
/// Delay/silence before a channel starts does not belong to its timeline.
#[derive(Clone, Copy)]
pub(super) struct PlaybackSpan {
    pub(super) begin: u64,
    pub(super) position: f64,
    pub(super) step: f64,
    pub(super) sample_rate: u32,
    pub(super) length: usize,
    pub(super) loops: bool,
}

impl PlaybackSpan {
    fn time_at(self, presented: u64) -> i32 {
        let frames = self.position + (presented - self.begin) as f64 * self.step;
        let length = self.length as f64;
        let frames = if self.loops {
            frames % length
        } else {
            frames.min(length)
        };
        (frames * 1000.0 / f64::from(self.sample_rate)) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delayed_clip_finishes_only_after_its_last_frame_is_presented() {
        let mut clock = Presentation::default();
        clock.push(
            PlaybackSpan {
                begin: 100,
                position: 0.0,
                step: 0.125,
                sample_rate: 6_000,
                length: 6,
                loops: false,
            },
            148,
        );
        clock.present(100);
        assert_eq!(clock.time_ms, 0);
        assert!(clock.pending());
        clock.present(147);
        assert_eq!(clock.time_ms, 0);
        assert!(clock.pending());
        clock.present(148);
        assert_eq!(clock.time_ms, 1);
        assert!(!clock.pending());
        clock.present(200);
        assert_eq!(clock.time_ms, 1);
    }

    #[test]
    fn presented_position_switches_from_intro_to_loop_without_counting_queued_audio() {
        let mut clock = Presentation::default();
        let intro = PlaybackSpan {
            begin: 0,
            position: 0.0,
            step: 1.0,
            sample_rate: 48_000,
            length: 480,
            loops: false,
        };
        clock.push(intro, 480);
        clock.push(
            PlaybackSpan {
                begin: 480,
                length: 144,
                loops: true,
                ..intro
            },
            900,
        );
        clock.present(240);
        assert_eq!(clock.time_ms, 5);
        clock.present(480);
        assert_eq!(clock.time_ms, 10);
        clock.present(480 + 144 * 2 + 96);
        assert_eq!(clock.time_ms, 2);
        assert!(clock.pending());
        clock.reset();
        clock.present(900);
        assert_eq!(clock.time_ms, 0);
        assert!(!clock.pending());
    }
}
