use crate::render::Canvas;
use crate::{
    Error, Result,
    crx::Crx,
    effects::rand15,
    graphics,
    mes::Script,
    text::{Text, TextPainter},
};
use std::{collections::VecDeque, sync::Arc, time::Duration};

pub(crate) struct Comments {
    pub(crate) visible: bool,
    background: Option<Arc<Crx>>,
    entries: Vec<(Vec<u8>, String)>,
    rows: VecDeque<String>,
    index: usize,
    interval: Duration,
    period: Option<u16>,
    position: (i32, i32),
    line_height: i32,
    capacity: usize,
    repeat: bool,
    random_order: bool,
}

#[derive(Default)]
pub(crate) struct SavedComments {
    pub(crate) entries: Vec<Vec<u8>>,
    pub(crate) period: Option<u16>,
    pub(crate) random_order: bool,
    pub(crate) repeat: bool,
    pub(crate) visible: bool,
}

impl Comments {
    pub(crate) fn new(kind: u16, background: Option<Arc<Crx>>) -> Result<Self> {
        let (position, line_height, capacity) = match kind {
            1 => ((10, 374), 12, 6),
            2 => ((468, 80), 16, 5),
            _ => return Err(Error::Invalid("ADV32 comment overlay type")),
        };
        Ok(Self {
            visible: false,
            background,
            entries: Vec::new(),
            rows: VecDeque::new(),
            index: 0,
            interval: Duration::ZERO,
            period: None,
            position,
            line_height,
            capacity,
            repeat: true,
            random_order: false,
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
        self.entries.push((encoded.to_vec(), line));
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> SavedComments {
        SavedComments {
            entries: self
                .entries
                .iter()
                .map(|(encoded, _)| encoded.clone())
                .collect(),
            period: self.period,
            random_order: self.random_order,
            repeat: self.repeat,
            visible: self.visible,
        }
    }

    pub(crate) fn restore(
        &mut self,
        snapshot: SavedComments,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<()> {
        self.reset(true);
        self.period = snapshot.period;
        self.random_order = snapshot.random_order;
        self.repeat = snapshot.repeat;
        self.visible = snapshot.visible;
        for entry in snapshot.entries {
            self.append(&entry, encoding)?;
        }
        Ok(())
    }

    pub(crate) fn reset(&mut self, clear_entries: bool) {
        self.visible = false;
        self.rows.clear();
        self.index = 0;
        self.interval = Duration::ZERO;
        if clear_entries {
            self.entries.clear();
        }
    }

    pub(crate) fn set_period(&mut self, value: u16) {
        self.period = (value != u16::MAX).then_some(value);
    }

    pub(crate) fn set_repeat(&mut self, value: bool) {
        self.repeat = value;
    }

    pub(crate) fn set_random_order(&mut self, value: bool) {
        self.random_order = value;
    }

    pub(crate) fn advance(&mut self, elapsed: Duration, random: &mut u32) -> bool {
        if !self.visible || self.entries.is_empty() {
            return false;
        }
        self.interval = self.interval.saturating_sub(elapsed);
        if !self.interval.is_zero() || (!self.repeat && self.index >= self.entries.len()) {
            return false;
        }
        let index = if self.random_order {
            rand15(random) as usize % self.entries.len()
        } else {
            self.index % self.entries.len()
        };
        self.index = self.index.saturating_add(1);
        self.interval = match self.period {
            Some(period) => {
                Duration::from_millis(100 * u64::from(period) + u64::from(rand15(random) % 200))
            }
            None => Duration::from_millis(500 + u64::from(rand15(random) % 2500)),
        };
        if self.rows.len() == self.capacity {
            self.rows.pop_front();
        }
        self.rows.push_back(self.entries[index].1.clone());
        true
    }

    pub(crate) fn draw(&self, frame: &mut Canvas, painter: &mut TextPainter) {
        if !self.visible || self.rows.is_empty() {
            return;
        }
        let (x, y) = self.position;
        if let Some(background) = &self.background {
            graphics::blit(frame, background, 8, 390, None);
        }
        for (row, content) in self.rows.iter().enumerate() {
            painter.draw(
                frame,
                &Text {
                    content: content.clone(),
                    x,
                    y: y + row as i32 * self.line_height,
                    size: self.line_height as u16,
                    color: [255; 3],
                    width: Some(500),
                },
            );
        }
    }
}
