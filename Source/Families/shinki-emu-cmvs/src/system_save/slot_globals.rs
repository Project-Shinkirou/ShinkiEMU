//! Native game[828] timeline and game[792/793] sound owners.
//! 424120 zeroes the 1556-byte timeline; 424B30 zeroes all six sound records.
//! These tags do not serialize the settings object, search paths, or fonts.
use super::slot_io::{Reader, invalid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use shinki_emu_sdk::CoreError;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeGlobals {
    pub timeline: NativeTimeline,
    pub bgm: NativeBgmSources,
    pub resource_channels: NativeResourceChannels,
    pub static_sounds: NativeStaticSounds,
}

impl NativeGlobals {
    pub(crate) fn decode_record(&mut self, tag: u16, payload: &[u8]) -> Result<(), CoreError> {
        match tag {
            1760 => self.timeline = NativeTimeline::decode(payload)?,
            784 => self.resource_channels = NativeResourceChannels::decode(payload)?,
            792 => self.static_sounds = NativeStaticSounds::decode(payload)?,
            _ => return Err(invalid("unknown native globals record")),
        }
        Ok(())
    }

    pub(crate) fn encode_record(&self, tag: u16) -> Result<Vec<u8>, CoreError> {
        match tag {
            1760 => self.timeline.encode(),
            784 => self.resource_channels.encode(),
            792 => self.static_sounds.encode(),
            _ => Err(invalid("unknown native globals record")),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        self.timeline.validate()?;
        self.bgm.validate()?;
        self.resource_channels.validate()?;
        self.static_sounds.validate()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct NativeTimeline {
    pub active: u32,
    pub count: u32,
    pub cursor: u32,
    pub start_millis: u32,
    pub elapsed_millis: u32,
    pub records: [NativeTimelineEntry; 16],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct NativeTimelineEntry {
    pub kind: u32,
    pub mode: u32,
    pub target: u32,
    pub name: String,
    pub duration: u32,
    pub values: [u32; 3],
    pub curve: u32,
}

impl NativeTimeline {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.count > 16 || self.cursor > self.count {
            return Err(invalid("native timeline count or cursor exceeds its table"));
        }
        for record in &self.records {
            encode_string(&record.name, 64)?;
        }
        Ok(())
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 1558 {
            return Err(invalid("native timeline length is not 1558"));
        }
        let mut reader = Reader::new(bytes);
        version(&mut reader)?;
        let mut value = Self {
            active: reader.u32()?,
            count: reader.u32()?,
            cursor: reader.u32()?,
            start_millis: reader.u32()?,
            elapsed_millis: reader.u32()?,
            ..Self::default()
        };
        for record in &mut value.records {
            record.kind = reader.u32()?;
            record.mode = reader.u32()?;
            record.target = reader.u32()?;
            record.name = fixed_string(reader.bytes(64)?)?;
            record.duration = reader.u32()?;
            record.values = [reader.u32()?, reader.u32()?, reader.u32()?];
            record.curve = reader.u32()?;
        }
        reader.finish()?;
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(1558);
        bytes.extend_from_slice(&1u16.to_le_bytes());
        for word in [
            self.active,
            self.count,
            self.cursor,
            self.start_millis,
            self.elapsed_millis,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for record in &self.records {
            for word in [record.kind, record.mode, record.target] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            append_fixed(&mut bytes, &record.name, 64)?;
            for word in [
                record.duration,
                record.values[0],
                record.values[1],
                record.values[2],
                record.curve,
            ] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeBgmSources {
    pub primary: String,
    pub secondary: String,
}

impl NativeBgmSources {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        encode_string(&self.primary, 64)?;
        encode_string(&self.secondary, 64)?;
        Ok(())
    }

    pub(crate) fn decode_fields(reader: &mut Reader<'_>) -> Result<Self, CoreError> {
        let value = Self {
            primary: counted_string(reader, 64)?,
            secondary: counted_string(reader, 64)?,
        };
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn encode_fields(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::new();
        for name in [&self.primary, &self.secondary] {
            let encoded = encode_string(name, 64)?;
            bytes.extend_from_slice(&((encoded.len() + 1) as u16).to_le_bytes());
            bytes.extend_from_slice(&encoded);
            bytes.push(0);
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeResourceChannels {
    pub slots: [NativeResourceChannel; 6],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeResourceChannel {
    pub name: String,
    pub loop_playback: u32,
    pub volume: u32,
    pub channel_volume: u32,
    pub stereo_pan: u32,
    pub restart_mode: u32,
}

impl NativeResourceChannels {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        for slot in &self.slots {
            encode_string(&slot.name, 32)?;
        }
        Ok(())
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        if bytes.len() != 312 {
            return Err(invalid("native resource sound table length is not 312"));
        }
        let mut reader = Reader::new(bytes);
        let mut value = Self::default();
        for slot in &mut value.slots {
            slot.name = fixed_string(reader.bytes(32)?)?;
            slot.loop_playback = reader.u32()?;
            slot.volume = reader.u32()?;
            slot.channel_volume = reader.u32()?;
            slot.stereo_pan = reader.u32()?;
            slot.restart_mode = reader.u32()?;
        }
        reader.finish()?;
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(312);
        for slot in &self.slots {
            append_fixed(&mut bytes, &slot.name, 32)?;
            for word in [
                slot.loop_playback,
                slot.volume,
                slot.channel_volume,
                slot.stereo_pan,
                slot.restart_mode,
            ] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        Ok(bytes)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct NativeStaticSounds {
    pub entries: Vec<NativeStaticSound>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NativeStaticSound {
    pub owner: u8,
    pub name: String,
    pub playing: u32,
    pub volume: u32,
}

impl NativeStaticSounds {
    pub(crate) fn validate(&self) -> Result<(), CoreError> {
        if self.entries.len() > 24 {
            return Err(invalid("native static sound count exceeds 24"));
        }
        let mut owners = 0u32;
        for entry in &self.entries {
            if entry.owner >= 24 || owners & (1u32 << entry.owner) != 0 {
                return Err(invalid(
                    "native static sound owner is invalid or duplicated",
                ));
            }
            owners |= 1u32 << entry.owner;
            encode_string(&entry.name, 32)?;
        }
        Ok(())
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let mut reader = Reader::new(bytes);
        version(&mut reader)?;
        let count = reader.u32()? as usize;
        if count > 24 {
            return Err(invalid("native static sound count exceeds 24"));
        }
        let mut value = Self {
            entries: Vec::with_capacity(count),
        };
        for _ in 0..count {
            let owner = reader.u32()?;
            if owner >= 24 {
                return Err(invalid("native static sound owner exceeds 23"));
            }
            value.entries.push(NativeStaticSound {
                owner: owner as u8,
                name: decode_string(reader.c_string(32)?)?,
                playing: reader.u32()?,
                volume: reader.u32()?,
            });
        }
        reader.finish()?;
        value.validate()?;
        Ok(value)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        // 48C310 visits owner slots in ascending order, independently of allocation order.
        let mut entries = [None; 24];
        for entry in &self.entries {
            entries[usize::from(entry.owner)] = Some(entry);
        }
        for entry in entries.into_iter().flatten() {
            bytes.extend_from_slice(&u32::from(entry.owner).to_le_bytes());
            bytes.extend_from_slice(&encode_string(&entry.name, 32)?);
            bytes.push(0);
            bytes.extend_from_slice(&entry.playing.to_le_bytes());
            bytes.extend_from_slice(&entry.volume.to_le_bytes());
        }
        Ok(bytes)
    }
}

fn version(reader: &mut Reader<'_>) -> Result<(), CoreError> {
    if reader.u16()? != 1 {
        return Err(invalid("native globals version is not one"));
    }
    Ok(())
}

fn counted_string(reader: &mut Reader<'_>, maximum: usize) -> Result<String, CoreError> {
    let length = usize::from(reader.u16()?);
    if length == 0 || length > maximum {
        return Err(invalid("native counted string length is invalid"));
    }
    let bytes = reader.bytes(length)?;
    if bytes.last() != Some(&0) || bytes[..length - 1].contains(&0) {
        return Err(invalid("native counted string terminator is invalid"));
    }
    decode_string(&bytes[..length - 1])
}

fn fixed_string(bytes: &[u8]) -> Result<String, CoreError> {
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| invalid("native fixed string is unterminated"))?;
    decode_string(&bytes[..length])
}

fn decode_string(bytes: &[u8]) -> Result<String, CoreError> {
    let (value, _, malformed) = encoding_rs::SHIFT_JIS.decode(bytes);
    if malformed {
        return Err(invalid("native globals string is not valid CP932"));
    }
    let (encoded, _, invalid_encoding) = encoding_rs::SHIFT_JIS.encode(&value);
    if invalid_encoding || encoded.as_ref() != bytes {
        return Err(invalid("native globals string cannot round-trip CP932"));
    }
    Ok(value.into_owned())
}

fn encode_string(value: &str, maximum: usize) -> Result<std::borrow::Cow<'_, [u8]>, CoreError> {
    let (encoded, _, malformed) = encoding_rs::SHIFT_JIS.encode(value);
    if malformed || encoded.len() >= maximum || value.contains('\0') {
        return Err(invalid("native globals string exceeds its CP932 buffer"));
    }
    Ok(encoded)
}

fn append_fixed(bytes: &mut Vec<u8>, value: &str, size: usize) -> Result<(), CoreError> {
    let encoded = encode_string(value, size)?;
    let start = bytes.len();
    bytes.extend_from_slice(&encoded);
    bytes.resize(start + size, 0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_timeline_fixture_restores_header_and_semantic_fields() {
        let mut fixture = vec![0; 1558];
        fixture[..2].copy_from_slice(&1u16.to_le_bytes());
        for (offset, word) in [
            (2, 1u32),
            (6, 1),
            (14, 900),
            (18, 120),
            (22, 3),
            (30, 7),
            (98, 250),
        ] {
            fixture[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        }
        fixture[34..43].copy_from_slice(b"scene.ps3");
        let value = NativeTimeline::decode(&fixture).unwrap();
        assert_eq!(
            (
                value.active,
                value.count,
                value.start_millis,
                value.elapsed_millis
            ),
            (1, 1, 900, 120)
        );
        assert_eq!(
            (
                value.records[0].kind,
                value.records[0].target,
                value.records[0].duration
            ),
            (3, 7, 250)
        );
        assert_eq!(value.records[0].name, "scene.ps3");
        assert_eq!(value.encode().unwrap(), fixture);
    }

    #[test]
    fn static_sound_fixture_retains_sparse_owner_and_playback_mode() {
        let fixture = [
            1, 0, 1, 0, 0, 0, 23, 0, 0, 0, b'a', 0, 1, 0, 0, 0, 128, 0, 0, 0,
        ];
        let value = NativeStaticSounds::decode(&fixture).unwrap();
        assert_eq!(
            value.entries[0],
            NativeStaticSound {
                owner: 23,
                name: "a".into(),
                playing: 1,
                volume: 128
            }
        );
        assert_eq!(value.encode().unwrap(), fixture);
        let mut bad = fixture;
        bad[6] = 24;
        assert!(NativeStaticSounds::decode(&bad).is_err());
    }
}
