use crate::{Error, Result, crx::Crx, graphics::Clip, mes::Script, save::PlayerNames, text::Text};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

const CHARACTER_FRAME: Duration = Duration::from_nanos(16_666_667);

/// A message frame can contain button and glyph art above its main full-width
/// panel. The last wide alpha band is the region used by the message layer.
pub(crate) fn panel(image: &Crx) -> Option<Clip> {
    let (width, height) = image.image.dimensions();
    let mut bottom = height;
    let wide =
        |y| {
            image.image.rows().nth(y as usize).is_some_and(|row| {
                row.filter(|pixel| pixel[3] > 8).count() * 4 >= width as usize * 3
            })
        };
    while bottom > 0 && !wide(bottom - 1) {
        bottom -= 1;
    }
    let mut top = bottom;
    while top > 0 && wide(top - 1) {
        top -= 1;
    }
    (bottom > top).then_some(Clip {
        x: 0,
        y: top as i32,
        width,
        height: bottom - top,
    })
}

pub(crate) struct Message {
    speaker: String,
    original: String,
    color_index: Option<u16>,
    line: String,
    next: usize,
    accumulator: Duration,
    elapsed: Duration,
    effects: VecDeque<Effect>,
    all_revealed: bool,
}

struct Effect {
    offset: usize,
    at: Option<Duration>,
    command: Command,
}

#[derive(Clone, Copy)]
pub(crate) enum Command {
    Script(usize),
    Music(u16),
    FadeMusic(u16),
    Sound(u16, bool),
    SwapTexture { from: u8, to: u8 },
    Shake { actor: Option<u8>, vertical: bool },
}

impl Message {
    pub(crate) fn new(
        body: &[u8],
        speaker: String,
        names: &PlayerNames,
        encoding: &'static encoding_rs::Encoding,
    ) -> Result<Self> {
        Self::parse(Script::decode_text(body, encoding)?, speaker, names)
    }

    fn parse(decoded: String, speaker: String, names: &PlayerNames) -> Result<Self> {
        let mut source = decoded.as_str();
        let mut line = String::with_capacity(decoded.len());
        let mut effects = VecDeque::new();
        let mut next_timestamp = None;
        let mut target = 4;
        let mut color_index = None;
        while let Some(first) = source.chars().next() {
            if let Some(rest) = source.strip_prefix("&f") {
                line.push_str(&names.family);
                source = rest;
                continue;
            }
            if let Some(rest) = source.strip_prefix("&n") {
                line.push_str(&names.given);
                source = rest;
                continue;
            }
            if first != '@' {
                line.push(first);
                source = &source[first.len_utf8()..];
                continue;
            }
            if let Some(rest) = source.strip_prefix("@n") {
                line.push('\n');
                source = rest;
                continue;
            }
            if let Some(rest) = source.strip_prefix("@c")
                && rest.as_bytes().first().is_some_and(u8::is_ascii_digit)
            {
                let (color, after) = number(rest)?;
                color_index = Some(color);
                source = after;
                continue;
            }
            if let Some(rest) = source.strip_prefix("@t") {
                let (value, after) = number(rest)?;
                next_timestamp = Some(Duration::from_millis(100 * u64::from(value)));
                source = after;
                continue;
            }
            if let Some((slot, rest)) = indexed(source, "@b") {
                target = 4 + slot;
                source = rest;
                continue;
            }
            if let Some(rest) = source
                .strip_prefix("@bc")
                .or_else(|| source.strip_prefix("@bf"))
            {
                target = if source.starts_with("@bc") { 25 } else { 31 };
                source = rest;
                continue;
            }
            let (command, rest) = if let Some(rest) = source
                .strip_prefix("@cmd")
                .or_else(|| source.strip_prefix("@d"))
            {
                let (value, rest) = number(rest)?;
                (Command::Script(usize::from(value.saturating_sub(1))), rest)
            } else if let Some(rest) = source.strip_prefix("@muf") {
                let (duration, rest) = number(rest)?;
                (Command::FadeMusic(duration), rest)
            } else if let Some(rest) = source.strip_prefix("@mu") {
                let (value, rest) = number(rest)?;
                (Command::Music(value), rest)
            } else if let Some(rest) = source.strip_prefix("@lse") {
                let (value, rest) = number(rest)?;
                (Command::Sound(value, true), rest)
            } else if let Some(rest) = source.strip_prefix("@se") {
                let (value, rest) = number(rest)?;
                (Command::Sound(value, false), rest)
            } else if let Some((slot, rest)) = indexed(source, "@wb") {
                (
                    Command::SwapTexture {
                        from: 19 + slot,
                        to: 1,
                    },
                    rest,
                )
            } else if let Some((slot, rest)) = indexed(source, "@w") {
                (
                    Command::SwapTexture {
                        from: 19 + slot,
                        to: target,
                    },
                    rest,
                )
            } else if let Some((actor, rest)) =
                indexed(source, "@bt").or_else(|| indexed(source, "@by"))
            {
                (
                    Command::Shake {
                        actor: Some(actor),
                        vertical: source.starts_with("@bt"),
                    },
                    rest,
                )
            } else if let Some(rest) = source
                .strip_prefix("@ht")
                .or_else(|| source.strip_prefix("@hy"))
            {
                (
                    Command::Shake {
                        actor: None,
                        vertical: source.starts_with("@ht"),
                    },
                    rest,
                )
            } else {
                line.push('@');
                source = &source[1..];
                continue;
            };
            effects.push_back(Effect {
                offset: line.len(),
                at: next_timestamp.take(),
                command,
            });
            source = rest;
        }
        Ok(Self {
            speaker: speaker
                .replace("&f", &names.family)
                .replace("&n", &names.given),
            original: decoded,
            color_index,
            line,
            next: 0,
            accumulator: Duration::ZERO,
            elapsed: Duration::ZERO,
            effects,
            all_revealed: false,
        })
    }

    pub(crate) fn finished(&self) -> bool {
        self.next == self.line.len() && self.effects.is_empty()
    }

    pub(crate) fn resumed(line: String, speaker: String, names: &PlayerNames) -> Result<Self> {
        let speaker = speaker.strip_prefix("@s").unwrap_or(&speaker).to_owned();
        let mut message = Self::parse(line, speaker, names)?;
        message.effects.clear();
        message.reveal();
        Ok(message)
    }

    pub(crate) fn parts(&self) -> (&str, &str) {
        (&self.line, &self.speaker)
    }

    pub(crate) fn native_parts(&self) -> (&str, &str) {
        (&self.original, &self.speaker)
    }

    pub(crate) fn reveal(&mut self) {
        self.next = self.line.len();
        self.all_revealed = true;
    }

    pub(crate) fn advance(&mut self, elapsed: Duration, character_frames: u32) -> bool {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.next == self.line.len() {
            return false;
        }
        if character_frames == 0 {
            self.next = self.line.len();
            return true;
        }
        let interval = CHARACTER_FRAME * character_frames;
        self.accumulator = self.accumulator.saturating_add(elapsed);
        let before = self.next;
        while self.accumulator >= interval {
            self.accumulator -= interval;
            let Some(character) = self.line[self.next..].chars().next() else {
                break;
            };
            self.next += character.len_utf8();
        }
        self.next != before
    }

    pub(crate) fn due(&mut self) -> Vec<Command> {
        let mut commands = Vec::new();
        self.effects.retain(|effect| {
            let due = self.all_revealed
                || effect
                    .at
                    .map_or(self.next >= effect.offset, |at| self.elapsed >= at);
            if due {
                commands.push(effect.command);
            }
            !due
        });
        commands
    }

    pub(crate) fn text(&self, top: i32, width: u32, palette: &BTreeMap<u16, [u8; 3]>) -> [Text; 2] {
        let margin = width / 16;
        let x = margin as i32;
        let width = Some(width - 2 * margin);
        let color = self
            .color_index
            .and_then(|index| palette.get(&index).copied())
            .unwrap_or([255; 3]);
        [
            Text {
                content: self.speaker.clone(),
                x,
                y: top + 13,
                size: 22,
                color: [255; 3],
                width,
            },
            Text {
                content: self.line[..self.next].to_owned(),
                x,
                y: top + 44,
                size: 26,
                color,
                width,
            },
        ]
    }
}

fn indexed<'a>(source: &'a str, prefix: &str) -> Option<(u8, &'a str)> {
    let rest = source.strip_prefix(prefix)?;
    let digit = *rest.as_bytes().first()?;
    (b'1'..=b'5')
        .contains(&digit)
        .then(|| (digit - b'1', &rest[1..]))
}

fn number(text: &str) -> Result<(u16, &str)> {
    let digits = text
        .as_bytes()
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits == 0 {
        return Ok((0, text));
    }
    let value = text[..digits]
        .parse()
        .map_err(|_| Error::Invalid("ADV32 message control number"))?;
    Ok((value, &text[digits..]))
}
