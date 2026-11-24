use std::io;

pub(crate) fn decode_global(bytes: &[u8]) -> io::Result<(u8, Vec<u8>)> {
    let stored = word(bytes, 0)?;
    let expanded = word(bytes, 4)?;
    let extra = word(bytes, 8)?;
    let after_extra = extra.checked_add(12).ok_or_else(invalid)?;
    let metadata = word(bytes, after_extra)?;
    let header = after_extra
        .checked_add(4)
        .and_then(|start| start.checked_add(metadata))
        .ok_or_else(invalid)?;
    let encoded = bytes.get(header..).ok_or_else(invalid)?;
    let [kind, checksum, data @ ..] = encoded else {
        return Err(invalid());
    };
    if *kind > 1
        || data.len() != stored
        || data.iter().copied().fold(0u8, u8::wrapping_add) != *checksum
    {
        return Err(invalid());
    }
    let state = if expanded == 0 {
        data.to_vec()
    } else {
        shinki_emu_lucifen_formats::decode_lzss(data, expanded, 64 * 1024 * 1024)?
    };
    Ok((kind + 1, state))
}

pub(crate) fn encode_global(data: &[u8], group: u8) -> io::Result<Vec<u8>> {
    let kind = group
        .checked_sub(1)
        .filter(|kind| *kind <= 1)
        .ok_or_else(invalid)?;
    let length = u32::try_from(data.len()).map_err(|_| invalid())?;
    let mut output = Vec::with_capacity(data.len() + 18);
    output.extend_from_slice(&length.to_le_bytes());
    // Uncompressed state and empty optional metadata sections are supported
    // by the native loader when saving the default global state.
    output.extend_from_slice(&[0; 12]);
    output.push(kind);
    output.push(data.iter().copied().fold(0u8, u8::wrapping_add));
    output.extend_from_slice(data);
    Ok(output)
}

/// Preserve the native text log, including the per-script read state and
/// variant-specific flags. Validate its framing before retaining the payload.
pub(crate) struct TextLog(Vec<u8>);

impl TextLog {
    pub(crate) fn decode(data: Vec<u8>) -> io::Result<Self> {
        if !data.starts_with(b"TLOG") {
            return Err(invalid());
        }
        let entries = word(&data, 4)?;
        let offset = entries
            .checked_mul(6)
            .and_then(|length| length.checked_add(8))
            .ok_or_else(invalid)?;
        let stored = word(&data, offset)?;
        let tail = data.get(offset + 4..).ok_or_else(invalid)?;
        if stored == 0 && tail.is_empty() {
            return Ok(Self(data));
        }
        let expanded = word(tail, 0)?;
        let count = word(tail, 4)?;
        let body = tail
            .get(8..)
            .filter(|bytes| bytes.len() == stored)
            .ok_or_else(invalid)?;
        let decoded;
        let mut records = if expanded == 0 {
            body
        } else {
            decoded = shinki_emu_lucifen_formats::decode_lzss(body, expanded, 64 * 1024 * 1024)?;
            &decoded
        };
        for _ in 0..count {
            let (length, item) = records.split_first().ok_or_else(invalid)?;
            let (name, tail) = item
                .split_at_checked(usize::from(*length))
                .ok_or_else(invalid)?;
            if name.last() != Some(&0) || name[..name.len() - 1].contains(&0) {
                return Err(invalid());
            }
            records = tail;
            for _ in 0..2 {
                let length = word(records, 4)?;
                records = records
                    .get(8..)
                    .and_then(|tail| tail.get(length..))
                    .ok_or_else(invalid)?;
            }
        }
        if !records.is_empty() {
            return Err(invalid());
        }
        Ok(Self(data))
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

/// The part of a native slot after its typed persistent variables. Store
/// history records byte-for-byte so both text encodings and original voice
/// information survive a load/save cycle.
#[derive(Default)]
pub(crate) struct SlotHistory {
    bytes: Option<Vec<u8>>,
    pub(crate) position: i32,
}

impl SlotHistory {
    pub(crate) fn entry_count(&self) -> i32 {
        self.bytes
            .as_deref()
            .and_then(|bytes| bytes.first_chunk::<4>())
            .map_or(0, |count| i32::from_le_bytes(*count))
    }

    pub(crate) fn decode(tail: &[u8]) -> io::Result<(Self, Option<Vec<u8>>)> {
        match tail.first() {
            Some(0) if tail.len() == 1 => return Ok((Self::default(), None)),
            Some(1) => {}
            _ => return Err(invalid()),
        }
        let data = &tail[1..];
        let count = word(data, 0)?;
        let mut next = &data[4..];
        for _ in 0..count {
            next = history_entry_tail(next)?;
        }
        let history_length = data.len() - next.len();
        let extension_length = word(next, 0)?;
        let extension = next
            .get(4..)
            .filter(|bytes| bytes.len() == extension_length)
            .ok_or_else(invalid)?;
        Ok((
            Self {
                bytes: Some(data[..history_length].to_vec()),
                position: count.saturating_sub(1) as i32,
            },
            (!extension.is_empty()).then(|| extension.to_vec()),
        ))
    }

    pub(crate) fn push(
        &mut self,
        packet: &crate::dialogue::Packet,
        limit: usize,
    ) -> io::Result<()> {
        if limit == 0 {
            return Ok(());
        }
        let history = self.bytes.get_or_insert_with(|| vec![0; 4]);
        let count = word(history, 0)?;
        if count >= limit {
            let mut next = &history[4..];
            for _ in limit..=count {
                next = history_entry_tail(next)?;
            }
            let offset = history.len() - next.len();
            history.drain(4..offset);
        }
        let count = u32::try_from(count.saturating_add(1).min(limit)).map_err(|_| invalid())?;
        history[..4].copy_from_slice(&count.to_le_bytes());
        for line in [packet.speaker.as_deref(), Some(packet.body.as_slice())] {
            if let Some(line) = line {
                let length = u32::try_from(line.len() + 1).map_err(|_| invalid())?;
                history.extend_from_slice(&length.to_le_bytes());
                history.extend_from_slice(line);
                history.push(0);
            } else {
                history.extend_from_slice(&[0; 4]);
            }
        }
        history.extend_from_slice(&packet.header);
        history.extend_from_slice(&[0; 4]);
        self.position = count.saturating_sub(1) as i32;
        Ok(())
    }

    pub(crate) fn select(&mut self, index: i32) -> io::Result<Option<crate::dialogue::Packet>> {
        let Some(history) = self.bytes.as_deref() else {
            return Ok(None);
        };
        if !(0..self.entry_count()).contains(&index) {
            return Ok(None);
        }
        let mut entry = &history[4..];
        for _ in 0..index {
            entry = history_entry_tail(entry)?;
        }
        let (speaker, entry) = history_text(entry)?;
        let (body, entry) = history_text(entry)?;
        let header = *entry.first_chunk::<3>().ok_or_else(invalid)?;
        self.position = index;
        Ok(Some(crate::dialogue::Packet::from_history(
            header,
            speaker,
            body.unwrap_or_default(),
        )))
    }

    pub(crate) fn encode(
        &self,
        bytes: &mut Vec<u8>,
        enabled: bool,
        checkpoint: Option<&[u8]>,
    ) -> io::Result<()> {
        if enabled {
            bytes.push(1);
            bytes.extend_from_slice(self.bytes.as_deref().unwrap_or(&[0; 4]));
            let checkpoint = checkpoint.unwrap_or_default();
            let length = u32::try_from(checkpoint.len()).map_err(|_| invalid())?;
            bytes.extend_from_slice(&length.to_le_bytes());
            bytes.extend_from_slice(checkpoint);
        } else {
            bytes.push(0);
        }
        Ok(())
    }
}

fn history_entry_tail(mut next: &[u8]) -> io::Result<&[u8]> {
    for _ in 0..2 {
        let length = word(next, 0)?;
        next = next
            .get(4..)
            .and_then(|bytes| bytes.get(length..))
            .ok_or_else(invalid)?;
    }
    next = next.get(3..).ok_or_else(invalid)?;
    let length = word(next, 0)?;
    next = next.get(4..).ok_or_else(invalid)?;
    if length != 0 {
        next = next
            .get(length.checked_add(8).ok_or_else(invalid)?..)
            .ok_or_else(invalid)?;
    }
    Ok(next)
}

fn history_text(input: &[u8]) -> io::Result<(Option<Vec<u8>>, &[u8])> {
    let length = word(input, 0)?;
    let (line, remaining) = input[4..].split_at_checked(length).ok_or_else(invalid)?;
    let text = if line.is_empty() {
        None
    } else {
        Some(line.strip_suffix(&[0]).ok_or_else(invalid)?.to_vec())
    };
    Ok((text, remaining))
}

fn word(bytes: &[u8], offset: usize) -> io::Result<usize> {
    bytes
        .get(offset..)
        .and_then(|tail| tail.first_chunk())
        .map(|word| u32::from_le_bytes(*word) as usize)
        .ok_or_else(invalid)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen global save")
}
