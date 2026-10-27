//! Native this+3096 message history (467D10..46832A), saved as tag1344.
use crate::system_save::slot_io::{Reader, invalid};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use shinki_emu_sdk::CoreError;
use std::collections::VecDeque;

const BYTE_LIMIT: usize = 3 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MessageEntry {
    // Native node +4, +36, owned +100, +68, in serialization order.
    pub strings: [Vec<u8>; 4],
    pub values: [u32; 6],
}

impl MessageEntry {
    fn validate(&self) -> Result<(), CoreError> {
        for (index, value) in self.strings.iter().enumerate() {
            let capacity = if index == 2 { BYTE_LIMIT } else { 32 };
            if value.len() >= capacity
                || value.contains(&0)
                || encoding_rs::SHIFT_JIS.decode_without_bom_handling(value).1
            {
                return Err(invalid(
                    "native message history string exceeds its CP932 buffer",
                ));
            }
        }
        Ok(())
    }
    fn byte_length(&self) -> usize {
        28 + self.strings.iter().map(Vec::len).sum::<usize>()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MessageHistory {
    pub limit: u32,
    pub entries: VecDeque<MessageEntry>,
}

impl Default for MessageHistory {
    fn default() -> Self {
        Self {
            limit: 128,
            entries: VecDeque::new(),
        }
    }
}

impl MessageHistory {
    pub fn validate(&self) -> Result<(), CoreError> {
        let mut size = 12usize;
        for entry in &self.entries {
            entry.validate()?;
            size = size
                .checked_add(entry.byte_length())
                .ok_or_else(|| invalid("native message history size overflowed"))?;
            if size > BYTE_LIMIT {
                return Err(invalid("native message history exceeds its byte budget"));
            }
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, CoreError> {
        let mut reader = Reader::new(bytes);
        let first = reader.u32()?;
        // 468260 also accepts the older two-dword header.
        let limit = if first >= 0xffff_0000 {
            reader.u32()?
        } else {
            first
        };
        let count = reader.u32()? as usize;
        if count > reader.remaining() / 28 {
            return Err(invalid("native message history count exceeds its payload"));
        }
        let mut entries = VecDeque::with_capacity(count);
        for _ in 0..count {
            let mut strings: [Vec<u8>; 4] = std::array::from_fn(|_| Vec::new());
            for (index, value) in strings.iter_mut().enumerate() {
                *value = reader
                    .c_string(if index == 2 { BYTE_LIMIT } else { 32 })?
                    .to_vec();
            }
            let mut values = [0; 6];
            for value in &mut values {
                *value = reader.u32()?;
            }
            entries.push_back(MessageEntry { strings, values });
        }
        reader.finish()?;
        let value = Self { limit, entries };
        value.validate()?;
        Ok(value)
    }

    pub fn encode(&self) -> Result<Vec<u8>, CoreError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(
            12 + self
                .entries
                .iter()
                .map(MessageEntry::byte_length)
                .sum::<usize>(),
        );
        for word in [0xffff_0000, self.limit, self.entries.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for entry in &self.entries {
            for string in &entry.strings {
                bytes.extend_from_slice(string);
                bytes.push(0);
            }
            for word in entry.values {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        Ok(bytes)
    }

    pub fn capture(&self, maximum: u32) -> Self {
        Self {
            limit: self.limit,
            entries: self
                .entries
                .iter()
                .take(maximum as usize)
                .cloned()
                .collect(),
        }
    }

    pub fn append(&mut self, entry: MessageEntry) -> Result<(), CoreError> {
        entry.validate()?;
        if self.limit == 0 {
            return Err(invalid("native message history capacity is zero"));
        }
        let retained = self.entries.len().min(self.limit as usize - 1);
        let bytes = 12
            + entry.byte_length()
            + self
                .entries
                .iter()
                .take(retained)
                .map(MessageEntry::byte_length)
                .sum::<usize>();
        if bytes > BYTE_LIMIT {
            return Err(invalid("native message history exceeds its byte budget"));
        }
        self.entries.truncate(retained);
        self.entries.push_front(entry);
        Ok(())
    }

    pub fn entry(&self, index: u32) -> Option<&MessageEntry> {
        // 467E90/Fxx use a signed walk counter; negative indices select the head.
        self.entries.get((index as i32).max(0) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(text: &[u8], value: u32) -> MessageEntry {
        MessageEntry {
            strings: [
                b"script".to_vec(),
                b"voice".to_vec(),
                text.to_vec(),
                b"name".to_vec(),
            ],
            values: [value, 1, 2, 3, 4, 5],
        }
    }
    #[test]
    fn native_history_restores_query_order_and_trims_oldest_on_new_append() {
        let mut history = MessageHistory {
            limit: 2,
            ..Default::default()
        };
        history.append(entry(b"old", 10)).unwrap();
        history.append(entry(b"new", 20)).unwrap();
        let bytes = history.capture(1).encode().unwrap();
        let mut loaded = MessageHistory::decode(&bytes).unwrap();
        assert_eq!(loaded.entry(0).unwrap().strings[2], b"new");
        assert_eq!(loaded.entry(u32::MAX).unwrap().values[0], 20);
        loaded.append(entry(b"next", 30)).unwrap();
        loaded.append(entry(b"last", 40)).unwrap();
        assert_eq!(
            loaded
                .entries
                .iter()
                .map(|entry| entry.values[0])
                .collect::<Vec<_>>(),
            [40, 30]
        );
        assert!(loaded.entry(2).is_none());
        let mut legacy = bytes[4..].to_vec();
        assert_eq!(
            MessageHistory::decode(&legacy)
                .unwrap()
                .entry(0)
                .unwrap()
                .values[0],
            20
        );
        legacy.push(0);
        assert!(MessageHistory::decode(&legacy).is_err());
    }
    #[test]
    fn malformed_history_rejects_count_and_owned_string_before_mutation() {
        assert!(MessageHistory::decode(&[0; 7]).is_err());
        let mut bytes = MessageHistory::default().encode().unwrap();
        bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(MessageHistory::decode(&bytes).is_err());
        let mut history = MessageHistory::default();
        let before = history.clone();
        let mut oversized = entry(b"valid", 0);
        oversized.strings[0] = vec![b'x'; 32];
        assert!(history.append(oversized).is_err());
        assert_eq!(history, before);
    }
    #[test]
    fn script_history_queries_use_owned_restored_nodes_and_clear_keeps_capacity() {
        use crate::scene::parent_load_tests::command;
        let mut vm = crate::CmvsPs2aVmState::new(0);
        for (index, value) in [b"script".as_slice(), b"voice", b"dialogue", b"name"]
            .into_iter()
            .enumerate()
        {
            vm.process_string_slots.insert(index as u32, value.to_vec());
        }
        command(
            &mut vm,
            310,
            &[
                0x8000_0000,
                0x8000_0001,
                0x8000_0002,
                0x8000_0003,
                10,
                11,
                12,
                13,
                14,
                15,
            ],
        );
        vm.process_string_slots.insert(2, b"replaced".to_vec());
        vm.message_history = MessageHistory::decode(&vm.message_history.encode().unwrap()).unwrap();
        command(&mut vm, 304, &[]);
        assert_eq!(vm.interpreter_words[&81220], 1);
        for (id, expected) in [
            (305, b"voice".as_slice()),
            (306, b"dialogue"),
            (307, b"script"),
            (308, b"name"),
        ] {
            command(&mut vm, id, &[0, 0x8000_0004]);
            assert_eq!(vm.process_string_slots[&4], expected);
        }
        command(&mut vm, 309, &[0, 8]);
        assert_eq!(vm.interpreter_words[&81220], 12);
        command(&mut vm, 309, &[100, 0]);
        assert_eq!(vm.interpreter_words[&81220], u32::MAX);
        command(&mut vm, 303, &[]);
        command(&mut vm, 304, &[]);
        assert_eq!(vm.interpreter_words[&81220], 0);
        command(&mut vm, 302, &[]);
        assert_eq!(vm.interpreter_words[&81220], 128);
    }
}
