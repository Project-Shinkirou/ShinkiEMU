use std::io;

pub(crate) struct VoiceGroup {
    pub enabled: bool,
    pub gain: i32,
    pub option: u8,
}

pub(crate) struct Voices {
    entries: Vec<(Vec<u8>, usize)>,
    pub extension: Vec<u8>,
    groups: Vec<VoiceGroup>,
}

impl Voices {
    pub(crate) fn parse(mut data: &[u8]) -> io::Result<Self> {
        let extension = text(&mut data)?.to_vec();
        let entries = length(&mut data)?;
        let groups = length(&mut data)?;
        if entries > data.len() || groups > data.len() {
            return Err(invalid());
        }
        let mut table = Self {
            entries: Vec::with_capacity(entries),
            extension,
            groups: Vec::with_capacity(groups),
        };
        for id in 0..groups {
            text(&mut data)?; // Group label for the original preferences menu.
            text(&mut data)?; // Optional label for the enabled-state control.
            table.groups.push(VoiceGroup {
                enabled: true,
                gain: 256,
                option: 0,
            });
            let count = length(&mut data)?;
            if count > entries - table.entries.len() {
                return Err(invalid());
            }
            for _ in 0..count {
                let prefix = text(&mut data)?.to_vec();
                table.entries.push((prefix, id));
            }
        }
        if table.entries.len() != entries {
            return Err(invalid());
        }
        Ok(table)
    }

    pub(crate) fn lookup(&self, name: &[u8]) -> Option<(usize, usize)> {
        self.entries
            .iter()
            .enumerate()
            .find(|(_, (prefix, _))| name.starts_with(prefix))
            .map(|(index, (_, group))| (index, *group))
    }

    pub(crate) fn group(&self, index: usize) -> Option<&VoiceGroup> {
        self.groups.get(index)
    }

    pub(crate) fn set_group(&mut self, index: usize, setting: i32, value: i32) -> i32 {
        let Some(group) = self.groups.get_mut(index) else {
            return -1;
        };
        match setting {
            0 => group.gain = value,
            1 => group.enabled = value as u8 != 0,
            2 => group.option = value as u8,
            _ => return -1,
        }
        0
    }
}

fn text<'a>(data: &mut &'a [u8]) -> io::Result<&'a [u8]> {
    let (size, rest) = data.split_first().ok_or_else(invalid)?;
    let (bytes, rest) = rest
        .split_at_checked(usize::from(*size))
        .ok_or_else(invalid)?;
    *data = rest;
    if bytes.is_empty() {
        return Ok(bytes);
    }
    bytes.strip_suffix(&[0]).ok_or_else(invalid)
}

fn length(data: &mut &[u8]) -> io::Result<usize> {
    let (head, tail) = data.split_at_checked(4).ok_or_else(invalid)?;
    *data = tail;
    Ok(u32::from_le_bytes(head.try_into().unwrap()) as usize)
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Lucifen voice metadata")
}
