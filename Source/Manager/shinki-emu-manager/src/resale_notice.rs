//! Binary-owned anti-resale text and acknowledgement policy.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shinki_emu_manager_ui_slint::ResaleNoticeViewModel;
use std::time::{Duration, Instant};

#[path = "resale_notice_raster.rs"]
mod raster;
pub use raster::{NoticeImage, NoticeRasterizer, NoticeTheme};

pub const TEXT: &str = "ShinkiEMU 是免费的开源软件，官方发布渠道只有 project-shinkirou.github.io 与 GitHub Releases。任何付费出售、捆绑或「激活」本软件的行为都与本项目无关；如果你为它付了钱，请申请退款并举报卖家。\n\nShinkiEMU is free, open-source software. Its only official distribution channels are project-shinkirou.github.io and GitHub Releases. This project has no connection with anyone charging for, bundling, or “activating” this software. If you paid for it, request a refund and report the seller.";
pub const ACK_KEY: &str = "onboarding.resale_notice";
pub const REQUIRED_DWELL: Duration = Duration::from_secs(8);

pub fn digest() -> String {
    format!("{:x}", Sha256::digest(TEXT.as_bytes()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgement {
    pub digest: String,
    pub visible_millis: u64,
    pub acknowledged_unix_millis: i64,
}
impl Acknowledgement {
    pub fn is_current(&self) -> bool {
        self.digest == digest() && self.visible_millis >= REQUIRED_DWELL.as_millis() as u64
    }
}

/// A full, readable raster must fit inside both the slot and its clipped window area.
/// Partial visibility cannot unlock onboarding, even if Slint reports `visible`.
pub fn slot_is_readable(
    reported_visible: bool,
    slot: shinki_emu_manager_ui_slint::NoticeSlotRect,
    minimum: (f32, f32),
    window: (f32, f32),
) -> bool {
    let Some(clip) = slot.clip else { return false };
    let values = [
        slot.x, slot.y, slot.width, slot.height,
        clip.x, clip.y, clip.width, clip.height,
        minimum.0, minimum.1, window.0, window.1,
    ];
    if !reported_visible || values.iter().any(|value| !value.is_finite())
        || minimum.0 < 96.0 || minimum.1 <= 0.0
        || slot.width < minimum.0 || slot.height < minimum.1
        || clip.width <= 0.0 || clip.height <= 0.0
    {
        return false;
    }
    let left = slot.x.max(clip.x).max(0.0);
    let top = slot.y.max(clip.y).max(0.0);
    let right = (slot.x + slot.width).min(clip.x + clip.width).min(window.0);
    let bottom = (slot.y + slot.height).min(clip.y + clip.height).min(window.1);
    // Subpixel layout rounding must not reject an otherwise exact fit.
    right - left + 0.5 >= minimum.0 && bottom - top + 0.5 >= minimum.1
}

#[derive(Default)]
pub struct Reading {
    checked: bool,
    visible: Duration,
    last_frame: Option<Instant>,
    acknowledged: bool,
}
impl Reading {
    pub fn restored(record: Option<&Acknowledgement>) -> Self {
        let acknowledged = record.is_some_and(Acknowledgement::is_current);
        Self {
            checked: acknowledged,
            visible: if acknowledged {
                REQUIRED_DWELL
            } else {
                Duration::ZERO
            },
            acknowledged,
            last_frame: None,
        }
    }

    /// Only consecutive, presented foreground frames count toward reading.
    /// A suspended/minimized host cannot accumulate wall-clock dwell.
    pub fn presented(&mut self, now: Instant, visible: bool) {
        if visible {
            if let Some(previous) = self.last_frame {
                let delta = now.saturating_duration_since(previous);
                if delta <= Duration::from_millis(500) {
                    self.visible = (self.visible + delta).min(REQUIRED_DWELL);
                }
            }
            self.last_frame = Some(now);
        } else {
            self.last_frame = None;
        }
    }

    pub fn set_checked(&mut self, checked: bool) {
        self.checked = checked;
        if !checked {
            self.acknowledged = false;
        }
    }

    pub fn pending_record(&self, unix_millis: i64) -> Option<Acknowledgement> {
        (self.checked && self.visible >= REQUIRED_DWELL && !self.acknowledged).then(|| {
            Acknowledgement {
                digest: digest(),
                visible_millis: self.visible.as_millis() as u64,
                acknowledged_unix_millis: unix_millis,
            }
        })
    }
    pub fn recorded(&mut self) {
        self.acknowledged = true;
    }
    pub fn view(&self) -> ResaleNoticeViewModel {
        ResaleNoticeViewModel {
            checked: self.checked,
            remaining_seconds: REQUIRED_DWELL
                .saturating_sub(self.visible)
                .as_secs_f32()
                .ceil() as i32,
            acknowledged: self.acknowledged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suspended_and_hidden_time_does_not_count() {
        let mut reading = Reading::default();
        let start = Instant::now();
        reading.presented(start, true);
        reading.presented(start + Duration::from_secs(9), true);
        assert_eq!(reading.view().remaining_seconds, 8);
        reading.presented(start + Duration::from_secs(10), false);
        reading.presented(start + Duration::from_secs(20), true);
        assert_eq!(reading.view().remaining_seconds, 8);
    }

    #[test]
    fn hidden_small_and_clipped_slots_pause_dwell_until_full_raster_is_visible() {
        use shinki_emu_manager_ui_slint::{NoticeClipRect, NoticeSlotRect};
        let full = NoticeSlotRect {
            x: 20.0, y: 20.0, width: 320.0, height: 300.0,
            clip: Some(NoticeClipRect { x: 0.0, y: 0.0, width: 360.0, height: 700.0 }),
        };
        let mut reading = Reading::default();
        reading.set_checked(true);
        let start = Instant::now();
        let cases = [
            (false, full),
            (true, NoticeSlotRect { width: 200.0, ..full }),
            (true, NoticeSlotRect { height: 100.0, ..full }),
            (true, NoticeSlotRect { y: -50.0, ..full }),
            (true, NoticeSlotRect { clip: Some(NoticeClipRect { height: 200.0, ..full.clip.unwrap() }), ..full }),
            (true, NoticeSlotRect::default()),
        ];
        for (index, (visible, slot)) in cases.into_iter().enumerate() {
            for frame in 0..=100 {
                reading.presented(start + Duration::from_millis((index as u64 * 101 + frame) * 100),
                    slot_is_readable(visible, slot, (320.0, 300.0), (360.0, 700.0)));
            }
            assert_eq!(reading.view().remaining_seconds, 8);
            assert!(reading.pending_record(1).is_none());
        }
        let resumed = start + Duration::from_secs(100);
        for frame in 0..=80 {
            reading.presented(resumed + Duration::from_millis(frame * 100),
                slot_is_readable(true, full, (320.0, 300.0), (360.0, 700.0)));
        }
        assert!(reading.pending_record(1).is_some());
        assert!(!slot_is_readable(true, full, (320.0, 300.0), (200.0, 700.0)));
        assert!(!slot_is_readable(true, full, (f32::NAN, 300.0), (360.0, 700.0)));
    }
}
