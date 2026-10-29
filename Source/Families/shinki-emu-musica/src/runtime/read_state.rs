use super::*;
use crate::minori_data::READ_FLAG_BYTES;
const MAX_READ_MESSAGES: usize = 131_072;

/// One script's original `<script>.tbr` bitmap: bit `0x80 >> (id & 7)` of
/// byte `id / 8` is set once message `id` has been read.
pub(crate) type ReadBitmap = [u8; READ_FLAG_BYTES];

pub(super) fn identity(
    script: Hash256,
    source: crate::SourceSpan,
    message_id: i64,
    text_hash: Hash256,
) -> Hash256 {
    let mut material = Vec::with_capacity(160);
    material.extend_from_slice(b"shinki.emu.musica.message.read.v1\0");
    material.extend_from_slice(script.as_bytes());
    material.extend_from_slice(&source.offset.to_le_bytes());
    material.extend_from_slice(&source.length.to_le_bytes());
    material.extend_from_slice(&message_id.to_le_bytes());
    material.extend_from_slice(text_hash.as_bytes());
    Hash256::from_sha256(&material)
}

/// `.tbr` stem of a script URI: the file name up to its last `.`, as the
/// original builds `<name>.tbr` from the script file name.
pub(crate) fn script_stem(uri: &str) -> &str {
    let name = uri.rsplit_once('/').map_or(uri, |(_, name)| name);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

/// Message ids the original can record; others are never read.
fn bit_id(message_id: i64) -> Option<u16> {
    u16::try_from(message_id).ok()
}

struct ScriptReadFlags {
    bits: Box<ReadBitmap>,
    dirty: bool,
}

/// Session read state, kept outside the saved VM state like the original's
/// per-script objects. `pending` is the shown message id the original marks
/// when the next message with a different id starts, or on `.chain`/`.end`.
pub(super) struct ReadFlags {
    scripts: BTreeMap<String, ScriptReadFlags>,
    current: String,
    pending: Option<u16>,
}

impl ReadFlags {
    pub(super) fn new(script_uri: &str) -> Self {
        Self {
            scripts: BTreeMap::new(),
            current: script_stem(script_uri).to_owned(),
            pending: None,
        }
    }

    pub(super) fn is_read(&self, state: &MusicaRuntimeState) -> bool {
        let Some(id) = state
            .message
            .as_ref()
            .and_then(|message| bit_id(message.message_id))
        else {
            return false;
        };
        self.scripts.get(&self.current).is_some_and(|flags| {
            flags.bits[usize::from(id / 8)] & (0x80 >> (id & 7)) != 0
        })
    }

    fn mark(&mut self, id: u16) {
        let flags = self
            .scripts
            .entry(self.current.clone())
            .or_insert_with(|| ScriptReadFlags {
                bits: Box::new([0; READ_FLAG_BYTES]),
                dirty: false,
            });
        let byte = &mut flags.bits[usize::from(id / 8)];
        let mask = 0x80 >> (id & 7);
        if *byte & mask == 0 {
            *byte |= mask;
            flags.dirty = true;
        }
    }

    /// perseus `sub_457C40`: a new message marks the previous one read.
    pub(super) fn message_started(&mut self, message_id: i64) {
        let next = bit_id(message_id);
        if let Some(previous) = self.pending
            && Some(previous) != next
        {
            self.mark(previous);
        }
        self.pending = next;
    }

    /// perseus `sub_45A770` (`.chain`/`.end`): marks the shown message read.
    pub(super) fn finish_message(&mut self) {
        if let Some(previous) = self.pending.take() {
            self.mark(previous);
        }
    }

    pub(super) fn switch_script(&mut self, script_uri: &str) {
        self.finish_message();
        self.current = script_stem(script_uri).to_owned();
    }

    /// A restored message is shown again and becomes the pending mark.
    pub(super) fn resume(&mut self, state: &MusicaRuntimeState) {
        self.current = script_stem(&state.script_uri).to_owned();
        self.pending = state
            .message
            .as_ref()
            .and_then(|message| bit_id(message.message_id));
    }
}

pub(super) fn validate(state: &MusicaRuntimeState) -> Result<(), MusicaRuntimeError> {
    if state.read_message_identities.len() > MAX_READ_MESSAGES
        || state
            .read_message_identities
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
    {
        return Err(MusicaRuntimeError::State);
    }
    if let Some(message) = &state.message {
        let entry = state.backlog.last().ok_or(MusicaRuntimeError::State)?;
        if entry.source != message.source
            || entry.message_id != message.message_id
            || identity(
                state.script_hash,
                message.source,
                message.message_id,
                entry.text_hash,
            ) != message.read_identity
        {
            return Err(MusicaRuntimeError::State);
        }
    }
    Ok(())
}
impl MusicaVm {
    pub fn active_message_is_read(&self) -> Result<bool, MusicaRuntimeError> {
        if self.state.message.is_none() {
            return Err(MusicaRuntimeError::State);
        }
        Ok(self.read_flags.is_read(&self.state))
    }

    /// `.tbr` stem of the running script.
    pub(crate) fn read_flags_script(&self) -> &str {
        &self.read_flags.current
    }

    pub(crate) fn has_read_flags(&self, stem: &str) -> bool {
        self.read_flags.scripts.contains_key(stem)
    }

    /// Adds stored bits for `stem`; bits already marked in memory stay set.
    pub(crate) fn install_read_flags(&mut self, stem: &str, bits: &ReadBitmap) {
        match self.read_flags.scripts.get_mut(stem) {
            Some(flags) => {
                for (byte, stored) in flags.bits.iter_mut().zip(bits) {
                    *byte |= stored;
                }
            }
            None => {
                self.read_flags.scripts.insert(
                    stem.to_owned(),
                    ScriptReadFlags {
                        bits: Box::new(*bits),
                        dirty: false,
                    },
                );
            }
        }
    }

    /// Writes every bitmap changed since its last successful write.
    pub(crate) fn flush_read_flags<E>(
        &mut self,
        mut write: impl FnMut(&str, &ReadBitmap) -> Result<(), E>,
    ) -> Result<(), E> {
        for (stem, flags) in &mut self.read_flags.scripts {
            if flags.dirty {
                write(stem, &flags.bits)?;
                flags.dirty = false;
            }
        }
        Ok(())
    }

    /// Carries the read bitmaps across a VM rebuilt for a loaded slot; the
    /// in-memory bitmaps replace freshly read copies of the same scripts.
    pub(crate) fn take_read_flags_from(&mut self, previous: &mut MusicaVm) {
        self.read_flags
            .scripts
            .append(&mut previous.read_flags.scripts);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn vm(source: &[u8]) -> MusicaVm {
        MusicaVm::new(
            "musica:/scr/k01.sc".into(),
            Hash256::from_sha256(source),
            crate::parse_sc(source, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            1,
        )
        .unwrap()
    }
    fn finish(vm: &mut MusicaVm) {
        let MusicaWaitState::Input { token_id } = vm.state().wait.clone().unwrap() else {
            panic!("expected message");
        };
        vm.resolve_wait(&token_id).unwrap();
    }
    fn flushed(vm: &mut MusicaVm) -> Vec<(String, ReadBitmap)> {
        let mut written = Vec::new();
        vm.flush_read_flags(|stem, bits| {
            written.push((stem.to_owned(), *bits));
            Ok::<_, ()>(())
        })
        .unwrap();
        written
    }

    #[test]
    fn next_message_marks_previous_id_and_skip_follows_the_bitmap() {
        let source = b".label start\r\n.message 9  speaker A\r\n.message 9  speaker B\r\n.message 70000  speaker C\r\n.message 10  speaker D\r\n.goto start\r\n";
        let mut vm = vm(source);
        vm.toggle_play_mode(MusicaPlayMode::Skip).unwrap();
        vm.step(1).unwrap();
        finish(&mut vm);
        // Finishing a message does not mark it; the next message does.
        assert!(!vm.active_message_is_read().unwrap());
        vm.step(2).unwrap();
        assert!(!vm.active_message_is_read().unwrap());
        finish(&mut vm);
        vm.step(3).unwrap();
        finish(&mut vm);
        vm.step(4).unwrap();
        finish(&mut vm);
        let [(stem, bits)] = flushed(&mut vm).try_into().unwrap();
        assert_eq!(stem, "k01");
        assert_eq!(bits[1], 0x40);
        assert_eq!(bits.iter().filter(|byte| **byte != 0).count(), 1);
        assert!(flushed(&mut vm).is_empty());
        vm.step(5).unwrap();
        assert!(vm.active_message_is_read().unwrap());
        assert!(vm.fast_forward_active());
        vm.state.system_ui.skip_enabled = false;
        assert!(!vm.fast_forward_active());
        vm.state.system_ui.skip_enabled = true;
        finish(&mut vm);
        vm.step(6).unwrap();
        assert!(vm.active_message_is_read().unwrap());
        finish(&mut vm);
        vm.step(7).unwrap();
        // 70000 is outside the original bitmap and is never read.
        assert!(!vm.active_message_is_read().unwrap());
        finish(&mut vm);
        vm.step(8).unwrap();
        assert!(vm.active_message_is_read().unwrap());
        assert!(vm.state().read_message_identities.is_empty());
    }

    #[test]
    fn chain_and_end_mark_the_shown_message_in_its_own_script() {
        let mut vm = vm(b".message 3  speaker A\r\n.chain k02.sc\r\n");
        vm.step(1).unwrap();
        finish(&mut vm);
        assert!(matches!(
            vm.step(2).unwrap(),
            Some(MusicaVmEvent::Chain { .. })
        ));
        let next = b".message 3  speaker B\r\n.end\r\n";
        vm.replace_script(
            "musica:/scr/k02.sc".into(),
            Hash256::from_sha256(next),
            crate::parse_sc(next, &crate::ScOpcodeCatalog::observed_musica()).unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(vm.read_flags_script(), "k02");
        vm.step(3).unwrap();
        assert!(!vm.active_message_is_read().unwrap());
        finish(&mut vm);
        assert_eq!(vm.step(4).unwrap(), Some(MusicaVmEvent::Terminal));
        let written = flushed(&mut vm);
        assert_eq!(
            written
                .iter()
                .map(|(stem, bits)| (stem.as_str(), bits[0]))
                .collect::<Vec<_>>(),
            [("k01", 0x10), ("k02", 0x10)]
        );
    }

    #[test]
    fn stored_bits_merge_and_survive_vm_replacement() {
        let source = b".message 0  speaker A\r\n.message 1  speaker B\r\n.end\r\n";
        let mut first = vm(source);
        let mut stored = [0u8; READ_FLAG_BYTES];
        stored[0] = 0x80;
        first.install_read_flags("k01", &stored);
        first.step(1).unwrap();
        assert!(first.active_message_is_read().unwrap());
        assert!(flushed(&mut first).is_empty());
        finish(&mut first);
        first.step(2).unwrap();
        let mut second = vm(source);
        second.install_read_flags("k01", &[0; READ_FLAG_BYTES]);
        second.take_read_flags_from(&mut first);
        assert!(!first.has_read_flags("k01"));
        second.step(1).unwrap();
        assert!(second.active_message_is_read().unwrap());
        // `first` marked nothing new: message 0 was already read.
        assert!(flushed(&mut second).is_empty());
    }
}
