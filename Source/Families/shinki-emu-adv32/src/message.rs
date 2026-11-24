use crate::{
    Error, Result,
    crx::Crx,
    graphics::Clip,
    mes::Dialect,
    save::PlayerNames,
    text::{Text, cell_width},
};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

const CHARACTER_FRAME: Duration = Duration::from_nanos(16_666_667);

/// Locate the dialogue panel in the MESWIN atlas.
pub(crate) fn panel(image: &Crx, dialect: Dialect) -> Option<Clip> {
    let (width, height) = image.image.dimensions();
    if dialect.classic() && (width, height) == (1280, 544) {
        // The classic 544-row atlas stores the dialogue window above the
        // two small panels. Neither of those panels is the dialogue window.
        return Some(Clip {
            x: 0,
            y: 314,
            width,
            height: 152,
        });
    }
    // The full-stage atlas stores tool sprites above the bottom message band.
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

fn resolve_name_variants(mut text: &str, custom_names: bool) -> std::borrow::Cow<'_, str> {
    if !text.contains("@re1") {
        return text.into();
    }
    let mut resolved = String::with_capacity(text.len());
    let mut replacements = Vec::new();
    while let Some((prefix, variant)) = text.split_once("@re1") {
        resolved.push_str(prefix);
        let Some((original, alternate)) = variant.split_once("@re2") else {
            return resolved.into();
        };
        let (alternate, suffix) = alternate.split_once("@n").unwrap_or((alternate, ""));
        replacements.push((original, alternate));
        text = suffix;
    }
    resolved.push_str(text);
    if custom_names {
        for (original, alternate) in replacements {
            resolved = if original.is_empty() {
                alternate.to_owned()
            } else {
                resolved.replace(original, alternate)
            };
        }
    }
    resolved.into()
}

struct TextReveal {
    rows: Vec<RevealRow>,
    row: usize,
    x: u32,
}

struct RevealRow {
    end: usize,
    width: u32,
    glyphs: Vec<(u32, usize)>,
}

impl TextReveal {
    fn new(text: &str, size: u16, encoding: &'static encoding_rs::Encoding) -> Self {
        let mut offset = 0;
        let rows = text
            .split('\n')
            .map(|line| {
                let mut x = 2;
                let glyphs = line
                    .char_indices()
                    .map(|(start, character)| {
                        x += u32::from(cell_width(character, size, encoding));
                        (x, offset + start + character.len_utf8())
                    })
                    .collect();
                let end = offset + line.len();
                offset = end + 1;
                RevealRow {
                    end,
                    width: x + 2,
                    glyphs,
                }
            })
            .collect();
        Self { rows, row: 0, x: 0 }
    }
}

struct Ruby {
    base: std::ops::Range<usize>,
    reading: String,
}

pub(crate) struct Message {
    speaker: String,
    original: String,
    ruby: Vec<Ruby>,
    color_index: Option<u16>,
    line: String,
    next: usize,
    accumulator: Duration,
    elapsed: Duration,
    effects: VecDeque<Effect>,
    all_revealed: bool,
    scan: Option<TextReveal>,
}

struct Effect {
    offset: usize,
    at: Option<Duration>,
    command: Command,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Command {
    Script(usize),
    Music(u16),
    FadeMusic(u16),
    Sound(u16, bool),
    SwapTexture { from: u8, to: u8 },
    Shake { actor: Option<u8>, vertical: bool },
    ActorFade { actor: u8, hide: bool },
}

impl Message {
    pub(crate) fn new(
        body: String,
        speaker: String,
        names: &PlayerNames,
        custom_names: bool,
        classic_effects: bool,
    ) -> Result<Self> {
        Self::parse(body, speaker, names, custom_names, classic_effects)
    }

    fn parse(
        decoded: String,
        speaker: String,
        names: &PlayerNames,
        custom_names: bool,
        classic_effects: bool,
    ) -> Result<Self> {
        let resolved = resolve_name_variants(&decoded, custom_names);
        let mut source = resolved.as_ref();
        let mut line = String::with_capacity(decoded.len());
        let mut ruby = Vec::new();
        let mut effects = VecDeque::new();
        let mut next_timestamp = None;
        let (actor_start, swap_start) = if classic_effects { (2, 14) } else { (4, 19) };
        let mut target = actor_start;
        let mut color_index = None;
        while let Some(first) = source.chars().next() {
            if let Some(rest) = source
                .strip_prefix("&f")
                .or_else(|| source.strip_prefix("$f"))
            {
                line.push_str(&names.family);
                source = rest;
                continue;
            }
            if let Some(rest) = source
                .strip_prefix("&n")
                .or_else(|| source.strip_prefix("$n"))
            {
                line.push_str(&names.given);
                source = rest;
                continue;
            }
            if let Some((reading, base, rest)) = ruby_markup(source) {
                let start = line.len();
                line.push_str(base);
                ruby.push(Ruby {
                    base: start..line.len(),
                    reading: reading.to_owned(),
                });
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
                target = actor_start + slot;
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
            } else if let Some(rest) = source.strip_prefix("@mu").or_else(|| {
                classic_effects
                    .then(|| numeric_suffix(source, "@m"))
                    .flatten()
            }) {
                let (value, rest) = number(rest)?;
                (Command::Music(value), rest)
            } else if let Some(rest) = source.strip_prefix("@lse") {
                let (value, rest) = number(rest)?;
                (Command::Sound(value, true), rest)
            } else if let Some(rest) = source.strip_prefix("@se").or_else(|| {
                classic_effects
                    .then(|| numeric_suffix(source, "@s"))
                    .flatten()
            }) {
                let (value, rest) = number(rest)?;
                (Command::Sound(value, false), rest)
            } else if let Some((slot, rest)) = indexed(source, "@wb") {
                (
                    Command::SwapTexture {
                        from: swap_start + slot,
                        to: 1,
                    },
                    rest,
                )
            } else if let Some((slot, rest)) = indexed(source, "@w") {
                (
                    Command::SwapTexture {
                        from: swap_start + slot,
                        to: target,
                    },
                    rest,
                )
            } else if let Some((actor, rest)) = classic_effects
                .then(|| indexed(source, "@bi").or_else(|| indexed(source, "@bo")))
                .flatten()
                .filter(|(actor, _)| *actor < 4)
            {
                (
                    Command::ActorFade {
                        actor,
                        hide: source.starts_with("@bo"),
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
            speaker: resolve_name_variants(&speaker, custom_names)
                .replace("&f", &names.family)
                .replace("&n", &names.given)
                .replace("$f", &names.family)
                .replace("$n", &names.given),
            original: decoded,
            ruby,
            color_index,
            line,
            next: 0,
            accumulator: Duration::ZERO,
            elapsed: Duration::ZERO,
            effects,
            all_revealed: false,
            scan: None,
        })
    }

    pub(crate) fn finished(&self) -> bool {
        self.visible_complete() && self.effects.is_empty()
    }

    pub(crate) fn resumed(
        line: String,
        speaker: String,
        names: &PlayerNames,
        custom_names: bool,
        classic_effects: bool,
    ) -> Result<Self> {
        let speaker = speaker.strip_prefix("@s").unwrap_or(&speaker).to_owned();
        let mut message = Self::parse(line, speaker, names, custom_names, classic_effects)?;
        message.effects.clear();
        message.reveal();
        Ok(message)
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.elapsed
    }

    pub(crate) fn parts(&self) -> (&str, &str) {
        (&self.line, &self.speaker)
    }

    pub(crate) fn native_parts(&self) -> (&str, &str) {
        (&self.original, &self.speaker)
    }

    fn visible_complete(&self) -> bool {
        self.next == self.line.len()
            && self
                .scan
                .as_ref()
                .is_none_or(|scan| scan.row == scan.rows.len())
    }

    pub(crate) fn reveal(&mut self) {
        self.next = self.line.len();
        self.scan = None;
        self.all_revealed = true;
    }

    pub(crate) fn advance(
        &mut self,
        elapsed: Duration,
        speed: u32,
        dialect: Dialect,
        encoding: &'static encoding_rs::Encoding,
    ) -> bool {
        self.elapsed = self.elapsed.saturating_add(elapsed);
        if self.visible_complete() {
            return false;
        }
        if speed >= 11 {
            self.next = self.line.len();
            self.scan = None;
            return true;
        }
        let size = if dialect.classic() { 24 } else { 22 };
        let scan = self
            .scan
            .get_or_insert_with(|| TextReveal::new(&self.line, size, encoding));
        self.accumulator = self.accumulator.saturating_add(elapsed);
        let before = (scan.row, scan.x);
        // The original clips each rendered line at a moving pixel boundary.
        // Each subsequent line starts at zero on the following video tick.
        let stride = 2 * speed + 8;
        while self.accumulator >= CHARACTER_FRAME {
            let Some(row) = scan.rows.get(scan.row) else {
                break;
            };
            self.accumulator -= CHARACTER_FRAME;
            scan.x += stride;
            if scan.x >= row.width {
                self.next = (row.end + 1).min(self.line.len());
                scan.row += 1;
                scan.x = 0;
            } else {
                let visible = row.glyphs.partition_point(|(x, _)| *x <= scan.x);
                if visible > 0 {
                    self.next = row.glyphs[visible - 1].1;
                }
            }
        }
        (scan.row, scan.x) != before
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

    pub(crate) fn text(
        &self,
        top: i32,
        width: u32,
        dialect: Dialect,
        palette: &BTreeMap<u16, [u8; 3]>,
        templates: [Option<&Text>; 2],
        encoding: &'static encoding_rs::Encoding,
    ) -> Vec<Text> {
        let margin = width / 16;
        let body_color = self
            .color_index
            .and_then(|index| palette.get(&index).copied());
        let body_size = if dialect.classic() { 24 } else { 22 };
        let speaker_size = if dialect.classic() { body_size } else { 20 };
        let make = |template: Option<&Text>, y, content, size, color: Option<[u8; 3]>| {
            let mut text = template.cloned().unwrap_or_else(|| Text {
                fixed_pitch: true,
                line_height: None,
                outline: false,
                clip_width: None,
                content: String::new(),
                x: margin as i32,
                y,
                size,
                color: [255; 3],
                width: Some(width - 2 * margin),
            });
            text.content = content;
            text.size = size;
            if let Some(color) = color {
                text.color = color;
            }
            text
        };
        let mut text = vec![make(
            templates[0],
            top + 13,
            self.speaker.clone(),
            speaker_size,
            None,
        )];
        let body = make(templates[1], top + 44, String::new(), body_size, body_color);
        let line_height = i32::from(body.line_height.unwrap_or(body.size + 12));
        let complete = self.visible_complete();
        let mut row_start = 0;
        for (row, content) in self.line.split('\n').enumerate() {
            let clip = if complete || self.scan.as_ref().is_some_and(|scan| row < scan.row) {
                None
            } else if let Some(scan) = &self.scan
                && row == scan.row
                && scan.x > 0
            {
                Some(scan.x)
            } else {
                break;
            };
            if !content.is_empty() {
                let mut line = body.clone();
                line.content = content.to_owned();
                line.y += row as i32 * line_height;
                line.clip_width = clip;
                let annotations = self.ruby.iter().filter(|ruby| {
                    ruby.base.start >= row_start && ruby.base.end <= row_start + content.len()
                });
                for ruby in annotations {
                    let width = |value: &str, size| {
                        value
                            .chars()
                            .map(|c| i32::from(cell_width(c, size, encoding)))
                            .sum::<i32>()
                    };
                    let base = (ruby.base.start - row_start)..(ruby.base.end - row_start);
                    let left = width(&content[..base.start], line.size);
                    let base_width = width(&content[base], line.size);
                    let reading_width = width(&ruby.reading, 12);
                    let offset = left + (base_width - reading_width) / 2;
                    let visible = clip.map(|x| (i64::from(x) - i64::from(offset)).max(0) as u32);
                    if visible != Some(0) {
                        let mut annotation = line.clone();
                        annotation.content.clone_from(&ruby.reading);
                        annotation.x += offset;
                        annotation.y -= 9;
                        annotation.size = 12;
                        annotation.line_height = None;
                        annotation.clip_width = visible;
                        text.push(annotation);
                    }
                }
                text.push(line);
            }
            row_start += content.len() + 1;
        }
        text
    }
}

fn ruby_markup(source: &str) -> Option<(&str, &str, &str)> {
    let (span, rest) = source.strip_prefix('｛')?.split_once('｝')?;
    let (reading, base) = span.split_once('／')?;
    (!reading.is_empty() && !base.is_empty() && !span.contains(['\n', '｛']))
        .then_some((reading, base, rest))
}

fn indexed<'a>(source: &'a str, prefix: &str) -> Option<(u8, &'a str)> {
    let rest = source.strip_prefix(prefix)?;
    let digit = *rest.as_bytes().first()?;
    (b'1'..=b'5')
        .contains(&digit)
        .then(|| (digit - b'1', &rest[1..]))
}

fn numeric_suffix<'a>(source: &'a str, prefix: &str) -> Option<&'a str> {
    source
        .strip_prefix(prefix)
        .filter(|suffix| suffix.as_bytes().first().is_some_and(u8::is_ascii_digit))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_message_macros_use_classic_slots_and_keep_commands_out_of_dialogue() {
        let mut message = Message::new(
            "@m001@b2@w1@wb5@bi2@bo2@s081Dialogue".into(),
            String::new(),
            &PlayerNames::default(),
            false,
            true,
        )
        .unwrap();
        message.reveal();
        assert_eq!(message.parts(), ("Dialogue", ""));
        assert_eq!(
            message.due(),
            vec![
                Command::Music(1),
                Command::SwapTexture { from: 14, to: 3 },
                Command::SwapTexture { from: 18, to: 1 },
                Command::ActorFade {
                    actor: 1,
                    hide: false
                },
                Command::ActorFade {
                    actor: 1,
                    hide: true
                },
                Command::Sound(81, false),
            ]
        );
    }
}
