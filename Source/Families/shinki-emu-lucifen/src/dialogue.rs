use crate::{Address, Vm, vm::displace};
use encoding_rs::{Encoding, GBK};
use std::{borrow::Cow, collections::BTreeSet, io};

#[derive(Clone, Copy)]
pub(crate) struct Destination {
    pub surface: i32,
    pub font: i32,
    pub x: i32,
    pub y: i32,
    pub spacing: i32,
}

pub(crate) struct Dialogue {
    pub destination: Option<Destination>,
    pub packet: Option<Packet>,
    pub seen: BTreeSet<Address>,
    pub annotations_enabled: bool,
    pub annotation_capacity: usize,
    pub annotations: Vec<Vec<u8>>,
    pub annotation_image: Option<usize>,
    pub annotation_key: Option<(u8, u16)>,
    pub annotation_selection: u8,
    pub choices_read: BTreeSet<(u8, u16)>,
}

impl Default for Dialogue {
    fn default() -> Self {
        Self {
            destination: None,
            packet: None,
            seen: BTreeSet::new(),
            annotations_enabled: true,
            annotation_capacity: 8,
            annotations: Vec::new(),
            annotation_image: None,
            annotation_key: None,
            annotation_selection: 0,
            choices_read: BTreeSet::new(),
        }
    }
}

impl Dialogue {
    pub(crate) fn clear_message(&mut self) {
        self.packet = None;
        self.annotations.clear();
        self.annotation_key = None;
        self.annotation_selection = 0;
    }

    pub(crate) fn annotation_label(&self, index: usize) -> Option<&[u8]> {
        let label = self.annotations.get(index)?.get(2..)?;
        let end = label
            .iter()
            .position(|&byte| byte == 0)
            .unwrap_or(label.len());
        Some(&label[..end])
    }

    pub(crate) fn choice_key(&self, index: u8) -> Option<(u8, u16)> {
        let (group, id) = self.annotation_key?;
        (group != 2 && usize::from(index) < self.annotations.len())
            .then_some((group, id.wrapping_add(u16::from(index))))
    }
}

pub(crate) struct Packet {
    pub address: Option<Address>,
    pub previously_seen: bool,
    pub header: [u8; 3],
    pub speaker: Option<Vec<u8>>,
    pub body: Vec<u8>,
    pub command: Vec<u8>,
    pub parameter: i32,
    scanned: usize,
    drawn: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TextKind {
    Speaker,
    Body,
    Annotation,
}

impl Packet {
    pub(crate) fn read(vm: &Vm, address: Address, encoding: &'static Encoding) -> io::Result<Self> {
        let data = vm.byte_tail(address)?.ok_or_else(invalid)?;
        let (&header, rest) = data.split_first_chunk::<3>().ok_or_else(invalid)?;
        let (speaker, body_start) = if rest.first() == Some(&b'#') {
            let (speaker, mut end) = expand(vm, address, data, 4, TextKind::Speaker, encoding)?;
            while let Some((_, bytes)) = speaker_marker(&data[end..], encoding) {
                end += bytes;
            }
            (Some(speaker), end)
        } else {
            (None, 3)
        };
        let (body, _) = expand(vm, address, data, body_start, TextKind::Body, encoding)?;
        Ok(Self {
            address: Some(address),
            previously_seen: false,
            header,
            speaker,
            body,
            command: Vec::new(),
            parameter: 0,
            scanned: 0,
            drawn: 0,
        })
    }

    pub(crate) fn from_history(header: [u8; 3], speaker: Option<Vec<u8>>, body: Vec<u8>) -> Self {
        Self {
            address: None,
            previously_seen: true,
            header,
            speaker,
            body,
            command: Vec::new(),
            parameter: 0,
            scanned: 0,
            drawn: 0,
        }
    }

    pub(crate) fn speaker_part(&self, part: usize, encoding: &'static Encoding) -> Option<&[u8]> {
        let name = self.speaker.as_deref()?;
        if part == 0 {
            return Some(name);
        }
        let mut start = 0;
        let mut index = 1;
        let mut cursor = 0;
        while cursor < name.len() {
            if name[cursor] == b'\\' {
                if index == part {
                    break;
                }
                index += 1;
                start = cursor + 1;
            }
            cursor += character_bytes(&name[cursor..], encoding);
        }
        (index == part && start < cursor).then(|| &name[start..cursor])
    }

    pub(crate) fn voice_name(&self, encoding: &'static Encoding) -> Option<Cow<'_, [u8]>> {
        let name = self.speaker_part(2, encoding)?;
        if encoding != GBK || name.is_ascii() {
            return Some(Cow::Borrowed(name));
        }
        let Some(decoded) = encoding.decode_without_bom_handling_and_without_replacement(name)
        else {
            return Some(Cow::Borrowed(name));
        };
        let ascii = decoded
            .chars()
            .map(|ch| match ch {
                '！'..='～' => Some((ch as u32 - 0xfee0) as u8),
                ch if ch.is_ascii() => Some(ch as u8),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        Some(ascii.map_or(Cow::Borrowed(name), Cow::Owned))
    }

    pub(crate) fn scan(&mut self, skip: i32, encoding: &'static Encoding) -> io::Result<i32> {
        self.scanned = 0;
        self.drawn = 0;
        let mut skip = skip;
        while let Some(&byte) = self.body.get(self.scanned) {
            let tail = &self.body[self.scanned..];
            match byte {
                b'&' => {
                    self.scanned += 1;
                    return Ok(1);
                }
                b'<' if tail.get(1) == Some(&b'<') => self.scanned += 2,
                b'<' => {
                    let end = command_end(tail, encoding)?;
                    self.scanned += end;
                    if skip == 0 {
                        let parameter = &tail[1..end - 1];
                        if self.body.get(self.scanned) == Some(&b'\\') {
                            self.scanned += 1;
                        }
                        if let Some(command) = parameter
                            .strip_prefix(b"\"")
                            .and_then(|p| p.strip_suffix(b"\""))
                        {
                            self.command = command.to_vec();
                            return Ok(3);
                        }
                        self.parameter = std::str::from_utf8(parameter)
                            .ok()
                            .and_then(|parameter| parameter.trim().parse().ok())
                            .ok_or_else(invalid)?;
                        return Ok(4);
                    }
                    skip = skip.saturating_sub(1);
                }
                b'[' if tail.get(1) == Some(&0) => {
                    self.scanned += binary_tag_size(tail)?;
                    if skip == 0 {
                        if self.body.get(self.scanned) == Some(&b'\\') {
                            self.scanned += 1;
                        }
                        return Ok(8);
                    }
                    skip = skip.saturating_sub(1);
                }
                _ => self.scanned += character_bytes(tail, encoding),
            }
        }
        Ok(2)
    }

    pub(crate) fn take_text(&mut self, encoding: &'static Encoding) -> io::Result<Option<String>> {
        if self.drawn >= self.scanned {
            return Ok(None);
        }
        let body = &self.body[self.drawn..self.scanned];
        self.drawn = self.scanned;
        let output = plain_text(body, encoding, false)?;
        Ok(Some(
            encoding.decode_without_bom_handling(&output).0.into_owned(),
        ))
    }

    pub(crate) fn summary(&self, encoding: &'static Encoding) -> io::Result<Vec<u8>> {
        plain_text(&self.body, encoding, true)
    }

    pub(crate) fn message_text(&self, encoding: &'static Encoding) -> io::Result<Vec<u8>> {
        plain_text(&self.body, encoding, false)
    }
}

pub(crate) fn read_annotations(
    vm: &Vm,
    address: Address,
    capacity: usize,
    encoding: &'static Encoding,
) -> io::Result<Vec<Vec<u8>>> {
    let data = vm.byte_tail(address)?.ok_or_else(invalid)?;
    let count = usize::from(*data.get(3).ok_or_else(invalid)?).min(capacity);
    let expand_text = *data.get(4).ok_or_else(invalid)? != 0;
    let mut cursor = 5;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let tail = data.get(cursor..).ok_or_else(invalid)?;
        let size = usize::from(
            *tail
                .first()
                .filter(|&&size| size >= 2)
                .ok_or_else(invalid)?,
        );
        let record = tail.get(..size).ok_or_else(invalid)?;
        if expand_text && record[1] != 0 {
            let (text, _) = expand(
                vm,
                address,
                &data[..cursor + size],
                cursor + 2,
                TextKind::Annotation,
                encoding,
            )?;
            records.push(annotation(&text, record[1])?);
        } else {
            records.push(record.to_vec());
        }
        cursor += size;
    }
    Ok(records)
}

pub(crate) fn annotation(label: &[u8], flags: u8) -> io::Result<Vec<u8>> {
    let length = u8::try_from(label.len() + 3).map_err(|_| invalid())?;
    let mut record = vec![length, flags];
    record.extend_from_slice(label);
    record.push(0);
    Ok(record)
}

fn plain_text(mut body: &[u8], encoding: &'static Encoding, summary: bool) -> io::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(body.len());
    while let Some(&byte) = body.first() {
        let size = match byte {
            b'$' | b'&' if summary => break,
            b'$' | b'&' => 1,
            b'\\' => {
                output.extend_from_slice(if summary { b"\r\n" } else { b"\n" });
                1
            }
            b'<' if body.get(1) == Some(&b'<') => {
                output.push(b'<');
                2
            }
            b'<' => {
                let end = command_end(body, encoding)?;
                end + usize::from(body.get(end) == Some(&b'\\'))
            }
            b'[' if body.get(1) == Some(&0) => {
                let end = binary_tag_size(body)?;
                end + usize::from(body.get(end) == Some(&b'\\'))
            }
            b'[' => 1,
            b'@' if body.get(1) == Some(&b'@') => {
                output.push(b'@');
                2
            }
            b'@' => {
                1 + body[1..]
                    .iter()
                    .take(5)
                    .take_while(|byte| byte.is_ascii_digit())
                    .count()
            }
            _ => {
                let size = character_bytes(body, encoding);
                output.extend_from_slice(&body[..size]);
                size
            }
        };
        body = &body[size..];
    }
    Ok(output)
}

fn expand(
    vm: &Vm,
    base: Address,
    input: &[u8],
    mut cursor: usize,
    kind: TextKind,
    encoding: &'static Encoding,
) -> io::Result<(Vec<u8>, usize)> {
    const MAX_BYTES: usize = 1024 * 1024;
    let mut output = Vec::new();
    while output.len() < MAX_BYTES && cursor < input.len() {
        let tail = &input[cursor..];
        if kind == TextKind::Speaker && matches!(speaker_marker(tail, encoding), Some((b'$', _))) {
            return Ok((output, cursor));
        }
        match tail[0] {
            0 => return Ok((output, cursor)),
            b'%' => {
                let mode = *tail.get(1).ok_or_else(invalid)?;
                let mut address = vm.read(displace(base, (cursor + 2) as i64)?)?.address()?;
                if mode != 0 {
                    address = vm.read(address)?.address()?;
                }
                output.extend(vm.text(address)?);
                cursor += 6;
            }
            byte => {
                let size = match byte {
                    b'<' if kind == TextKind::Body && tail.get(1) == Some(&b'<') => 2,
                    b'<' if kind == TextKind::Body => command_end(tail, encoding)?,
                    b'[' if kind == TextKind::Body && tail.get(1) == Some(&0) => {
                        binary_tag_size(tail)?
                    }
                    _ => character_bytes(tail, encoding),
                };
                output.extend_from_slice(&tail[..size]);
                cursor += size;
            }
        }
    }
    Err(invalid())
}

fn speaker_marker(tail: &[u8], encoding: &'static Encoding) -> Option<(u8, usize)> {
    match tail {
        [marker @ (b'$' | b'&'), ..] => Some((*marker, 1)),
        // GBK dialogue can use fullwidth markers in the speaker header.
        [0xa1, 0xe7, ..] if encoding == GBK => Some((b'$', 2)),
        [0xa3, 0xa6, ..] if encoding == GBK => Some((b'&', 2)),
        _ => None,
    }
}

fn command_end(tag: &[u8], encoding: &'static Encoding) -> io::Result<usize> {
    let mut cursor = 1;
    let mut quoted = false;
    while let Some(&byte) = tag.get(cursor) {
        match byte {
            0 => break,
            b'>' if !quoted => return Ok(cursor + 1),
            b'"' => quoted = !quoted,
            _ => {}
        }
        cursor += character_bytes(&tag[cursor..], encoding);
    }
    Err(invalid())
}

fn binary_tag_size(tag: &[u8]) -> io::Result<usize> {
    let length = tag
        .get(6..8)
        .and_then(|length| length.first_chunk())
        .ok_or_else(invalid)?;
    let total = 8 + usize::from(u16::from_le_bytes(*length));
    (tag.len() >= total).then_some(total).ok_or_else(invalid)
}

fn character_bytes(tail: &[u8], encoding: &'static Encoding) -> usize {
    let lead = if encoding == GBK {
        matches!(tail[0], 0x81..=0xfe)
    } else {
        matches!(tail[0], 0x81..=0x9f | 0xe0..=0xfc)
    };
    if lead && tail.get(1).is_some_and(|&byte| byte != 0) {
        2
    } else {
        1
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid Lucifen dialogue packet",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_controls_preserve_multibyte_trails_and_strip_markup() {
        let encoding = encoding_rs::SHIFT_JIS;
        let body = b"\x95\x5c<<@@<\"tag\">\\abc\\def$";
        let message = plain_text(body, encoding, false).unwrap();
        let summary = plain_text(body, encoding, true).unwrap();
        assert_eq!(message, b"\x95\x5c<@abc\ndef");
        assert_eq!(summary, b"\x95\x5c<@abc\r\ndef");
    }

    #[test]
    fn gbk_speaker_markers_keep_dialogue_out_of_the_voice_name() {
        let mut program = b"SOB0".to_vec();
        program.extend(24u32.to_le_bytes());
        program.extend([0; 24]);
        program.extend([4u32, 4, 1].into_iter().flat_map(u32::to_le_bytes));
        let mut vm = Vm::open(&program, None).unwrap();
        let address = Address::Stack(16);
        let (encoded, _, _) = GBK.encode("#测试&名字\\Ａ１２＄＆价格是＄３$");
        let mut data = vec![0; 3];
        data.extend_from_slice(&encoded);
        data.push(0);
        vm.write_data(address, &data).unwrap();

        let packet = Packet::read(&vm, address, GBK).unwrap();
        assert_eq!(packet.voice_name(GBK).as_deref(), Some(b"A12".as_slice()));
        let name = packet.speaker_part(1, GBK).unwrap();
        assert_eq!(GBK.decode(name).0, "测试&名字");
        assert_eq!(
            GBK.decode(&packet.message_text(GBK).unwrap()).0,
            "价格是＄３"
        );
    }
}
