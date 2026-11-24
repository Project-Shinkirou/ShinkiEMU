use crate::render::Canvas;
use crate::{
    Error, Result,
    crx::Crx,
    effects::rand15,
    graphics::{self, Clip},
    mes::Script,
    text::{Text, TextPainter},
};
use image::{Pixel, Rgba};
use roukaku_media_core::SceneCommand;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    time::Duration,
};

pub(crate) struct Comments {
    visible: bool,
    pub(crate) suppress_dialogue_wait: bool,
    background: Option<Arc<Crx>>,
    stamps: Option<Arc<Crx>>,
    senders: BTreeMap<String, u8>,
    chat_panel: bool,
    entries: Vec<Arc<CommentEntry>>,
    rows: VecDeque<Arc<CommentEntry>>,
    index: usize,
    interval: Duration,
    period: Option<u16>,
    period_scale: [i32; 3],
    period_elapsed: Duration,
    position: (i32, i32),
    line_height: i32,
    row_pitch: i32,
    capacity: usize,
    clear_every: u16,
    random_order: bool,
}

struct CommentEntry {
    encoded: Vec<u8>,
    content: String,
    portrait: Option<u8>,
    stamp: Option<u16>,
    delay: Option<Duration>,
}

#[derive(Clone, Default)]
pub(crate) struct SavedComments {
    pub(crate) entries: Vec<Vec<u8>>,
    pub(crate) period: Option<u16>,
    pub(crate) random_order: bool,
    pub(crate) clear_every: u16,
    pub(crate) period_scale: [i32; 3],
    pub(crate) visible: bool,
}

impl Comments {
    pub(crate) fn new(
        kind: u16,
        background: Option<Arc<Crx>>,
        stamps: Option<Arc<Crx>>,
        senders: BTreeMap<String, u8>,
    ) -> Result<Self> {
        let (position, line_height, capacity) = match kind {
            1 => ((10, 374), 12, 6),
            2 => ((540, 94), 16, 5),
            _ => return Err(Error::Invalid("ADV32 comment overlay type")),
        };
        Ok(Self {
            visible: false,
            suppress_dialogue_wait: false,
            background,
            stamps,
            senders,
            chat_panel: kind == 2,
            entries: Vec::new(),
            rows: VecDeque::new(),
            index: 0,
            interval: Duration::ZERO,
            period: None,
            period_scale: [0; 3],
            period_elapsed: Duration::ZERO,
            position,
            line_height,
            row_pitch: if kind == 2 { 81 } else { line_height },
            capacity,
            clear_every: 0,
            random_order: kind == 1,
        })
    }

    pub(crate) fn append(
        &mut self,
        encoded: &[u8],
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<()> {
        if self.entries.len() >= 20 {
            return Ok(());
        }
        let mut line = Script::decode_text(encoded, encoding)?;
        if line.starts_with('@')
            && line
                .as_bytes()
                .get(1)
                .is_some_and(|byte| b"mlsiryz".contains(byte))
        {
            line.drain(..2);
        }
        let mut entry = CommentEntry {
            encoded: encoded.to_vec(),
            content: line,
            portrait: None,
            stamp: None,
            delay: None,
        };
        if self.chat_panel {
            if let Some((sender, body)) = entry
                .content
                .strip_prefix('「')
                .and_then(|value| value.split_once('」'))
            {
                entry.portrait = Some(self.senders.get(sender).copied().unwrap_or(1));
                entry.content = body.to_owned();
            }
            if let Some(at) = entry.content.find("@d")
                && let Some((stamp, _)) = comment_number(&entry.content[at + 2..])
                && stamp > 0
            {
                entry.stamp = Some(stamp - 1);
                entry.content.clear();
            }
            while let Some(at) = entry.content.find("@t")
                && let Some((timing, digits)) = comment_number(&entry.content[at + 2..])
            {
                entry.delay = Some(Duration::from_millis(10 * u64::from(timing)));
                entry.content.replace_range(at..at + 2 + digits, "");
            }
            let mut bytes = 0;
            for (index, character) in entry.content.char_indices() {
                let mut utf8 = [0; 4];
                bytes += encoding.encode(character.encode_utf8(&mut utf8)).0.len();
                if bytes > 28 {
                    entry.content.insert(index, '\n');
                    break;
                }
            }
        }
        self.entries.push(Arc::new(entry));
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> SavedComments {
        SavedComments {
            entries: self
                .entries
                .iter()
                .map(|entry| entry.encoded.clone())
                .collect(),
            period: self.period,
            random_order: self.random_order,
            clear_every: self.clear_every,
            period_scale: self.period_scale,
            visible: self.visible,
        }
    }

    pub(crate) fn restore(
        &mut self,
        snapshot: SavedComments,
        encoding: &'static encoding_rs::Encoding,
        random: &mut u32,
    ) -> Result<()> {
        self.clear();
        self.rows.clear();
        self.period = if self.chat_panel {
            None
        } else {
            snapshot.period
        };
        self.random_order = !self.chat_panel && snapshot.random_order;
        self.clear_every = snapshot.clear_every;
        self.period_scale = snapshot.period_scale;
        self.period_elapsed = Duration::ZERO;
        self.visible = snapshot.visible;
        for entry in snapshot.entries {
            self.append(&entry, encoding)?;
        }
        if self.chat_panel {
            self.finish_batch();
        } else {
            self.prefill(random);
        }
        Ok(())
    }

    pub(crate) fn clear(&mut self) {
        self.suppress_dialogue_wait = false;
        self.index = 0;
        self.interval = Duration::ZERO;
        self.entries.clear();
        if self.chat_panel {
            self.period = None;
        } else {
            self.finish_batch();
            self.rows.clear();
        }
    }

    pub(crate) fn show(&mut self, refresh: bool) {
        if refresh {
            if !self.chat_panel {
                self.clear();
            }
            self.rows.clear();
        }
        self.visible = true;
        self.suppress_dialogue_wait = true;
    }

    pub(crate) fn hide(&mut self) {
        self.visible = false;
        self.suppress_dialogue_wait = false;
    }

    pub(crate) fn finish_batch(&mut self) {
        self.suppress_dialogue_wait = false;
        if !self.chat_panel {
            self.index = 0;
            self.interval = Duration::ZERO;
            self.period = None;
            self.random_order = true;
            self.clear_every = 0;
            self.period_scale = [0; 3];
            self.period_elapsed = Duration::ZERO;
            return;
        }
        if self.period.is_none_or(|period| period == 0) {
            let first = self.entries.len().saturating_sub(self.capacity);
            self.rows.clear();
            self.rows.extend(self.entries[first..].iter().cloned());
            self.index = self.entries.len();
        }
    }

    pub(crate) fn set_period(&mut self, value: u16) {
        self.period = (value != u16::MAX).then_some(value);
    }

    pub(crate) fn set_clear_every(&mut self, value: u16) {
        if !self.chat_panel {
            self.clear_every = value;
        }
    }

    pub(crate) fn scale_period(&mut self, start: i16, end: i16, steps: u16) {
        if !self.chat_panel && steps != 0 {
            self.period_scale = [i32::from(start), i32::from(end), i32::from(steps)];
            self.period_elapsed = Duration::ZERO;
        }
    }

    pub(crate) fn prefill(&mut self, random: &mut u32) {
        if !self.chat_panel && !self.entries.is_empty() {
            let rows = self.rows.len();
            for _ in rows..self.capacity {
                self.append_row(random);
            }
        }
    }

    pub(crate) fn set_random_order(&mut self, value: bool) {
        self.random_order = !self.chat_panel && value;
    }

    pub(crate) fn advance(&mut self, elapsed: Duration, random: &mut u32) -> bool {
        self.period_elapsed = self.period_elapsed.saturating_add(elapsed);
        if !self.visible || self.entries.is_empty() {
            return false;
        }
        self.interval = self.interval.saturating_sub(elapsed);
        if !self.interval.is_zero() || (self.chat_panel && self.index >= self.entries.len()) {
            return false;
        }
        self.append_row(random);
        true
    }

    fn append_row(&mut self, random: &mut u32) {
        if self.clear_every != 0 {
            let cycle = if self.random_order {
                usize::from(self.clear_every)
            } else {
                self.entries.len()
            };
            if self.index >= cycle {
                self.rows.clear();
                self.index = 0;
            }
        }
        let index = if self.random_order {
            rand15(random) as usize % self.entries.len()
        } else {
            self.index % self.entries.len()
        };
        self.index = self.index.saturating_add(1);
        self.interval = if self.chat_panel {
            self.entries[index]
                .delay
                .or_else(|| {
                    self.period
                        .map(|period| Duration::from_millis(10 * u64::from(period)))
                })
                .unwrap_or_default()
        } else if self.period_scale[2] > 0 {
            let [start, end, steps] = self.period_scale;
            let total = Duration::from_millis(100 * steps as u64);
            let fraction = self.period_elapsed.div_duration_f64(total).min(1.0);
            let percent =
                (f64::from(start) + (f64::from(end) - f64::from(start)) * fraction) as i64;
            let base = i64::from(self.period.unwrap_or(2500));
            let delay = base * percent / 100 * 100;
            Duration::from_millis(delay.max(0) as u64 + u64::from(rand15(random) % 50))
        } else {
            match self.period {
                Some(period) => {
                    Duration::from_millis(100 * u64::from(period) + u64::from(rand15(random) % 200))
                }
                None => Duration::from_millis(500 + u64::from(rand15(random) % 2500)),
            }
        };
        if self.rows.len() == self.capacity {
            self.rows.pop_front();
        }
        self.rows.push_back(self.entries[index].clone());
    }

    pub(crate) fn draw(&self, frame: &mut Canvas, painter: &mut TextPainter) -> crate::Result<()> {
        if !self.visible || self.rows.is_empty() {
            return Ok(());
        }
        let (x, y) = self.position;
        if self.chat_panel {
            let (width, height) = frame.dimensions();
            if !frame.queue(SceneCommand::rect(
                "adv32-comment-dim",
                0,
                0,
                width,
                height,
                [0, 0, 0, 128],
            )) {
                for pixel in frame.raster_mut().pixels_mut() {
                    pixel.blend(&Rgba([0, 0, 0, 128]));
                }
            }
            if let Some(background) = &self.background {
                graphics::blit(
                    frame,
                    background,
                    440,
                    16,
                    Some(Clip {
                        x: 0,
                        y: 0,
                        width: 372,
                        height: 518,
                    }),
                );
            }
        } else if let Some(background) = &self.background {
            graphics::blit(frame, background, 8, 390, None);
        }
        for (row, entry) in self.rows.iter().enumerate() {
            let offset = row as i32 * self.row_pitch;
            if let Some(portrait) = entry.portrait
                && let Some(atlas) = &self.background
            {
                graphics::blit(
                    frame,
                    atlas,
                    468,
                    80 + offset,
                    Some(Clip {
                        x: 420,
                        y: 81 * i32::from(portrait),
                        width: 320,
                        height: 81,
                    }),
                );
            }
            if let Some(stamp) = entry.stamp
                && let Some(atlas) = &self.stamps
            {
                graphics::blit(
                    frame,
                    atlas,
                    480,
                    96 + offset,
                    Some(Clip {
                        x: 80 * i32::from(stamp / 8),
                        y: 64 * i32::from(stamp % 8),
                        width: 79,
                        height: 63,
                    }),
                );
            }
            painter.draw(
                frame,
                &Text {
                    fixed_pitch: self.chat_panel,
                    line_height: None,
                    outline: self.chat_panel,
                    clip_width: None,
                    content: entry.content.clone(),
                    x,
                    y: y + offset,
                    size: self.line_height as u16,
                    color: [255; 3],
                    width: Some(if self.chat_panel { 224 } else { 500 }),
                },
            )?;
        }
        Ok(())
    }
}

fn comment_number(text: &str) -> Option<(u16, usize)> {
    let length = text.bytes().take_while(u8::is_ascii_digit).count();
    Some((text[..length].parse().ok()?, length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_metadata_selects_portraits_and_stamps_and_keeps_markup_out_of_the_text() {
        let encoding = encoding_rs::SHIFT_JIS;
        let mut comments =
            Comments::new(2, None, None, BTreeMap::from([("Sender".into(), 3)])).unwrap();
        let mut append = |message: &str| {
            let (data, _, invalid) = encoding.encode(message);
            assert!(!invalid);
            let shifted = data
                .iter()
                .map(|byte| byte.wrapping_sub(32))
                .collect::<Vec<_>>();
            comments.append(&shifted, encoding).unwrap();
        };
        append("「Sender」1234567890123456789012345678XY@t35");
        append("@d09");
        assert_eq!(comments.entries[0].portrait, Some(3));
        assert_eq!(
            comments.entries[0].content,
            "1234567890123456789012345678\nXY"
        );
        assert_eq!(comments.entries[0].delay, Some(Duration::from_millis(350)));
        assert_eq!(comments.entries[1].stamp, Some(8));
        assert!(comments.entries[1].content.is_empty());
    }

    #[test]
    fn publishing_a_chat_batch_keeps_the_panel_visible_and_restores_dialogue_waits() {
        let mut comments = Comments::new(2, None, None, BTreeMap::new()).unwrap();
        comments.show(false);
        let encoded = b"Example".map(|byte| byte - 32);
        comments.append(&encoded, encoding_rs::SHIFT_JIS).unwrap();
        assert!(comments.suppress_dialogue_wait);

        comments.finish_batch();
        assert!(comments.visible);
        assert!(!comments.suppress_dialogue_wait);
        assert_eq!(comments.rows[0].content, "Example");
        comments.clear();
        assert!(comments.entries.is_empty());
        assert_eq!(comments.rows[0].content, "Example");
        comments.finish_batch();
        assert!(comments.rows.is_empty());
    }
}
