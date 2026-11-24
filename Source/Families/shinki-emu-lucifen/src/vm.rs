use shinki_emu_lucifen_formats::{ModuleKind, Relocation, Sob, UserVariables, Variable};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

const MAX_STACK_BYTES: usize = 256 * 1024;
const MAX_MEMORY_OPERATION: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Address {
    Stack(usize),
    Image { module: usize, offset: u32 },
    Engine(u32),
    User(u32),
    UserData(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Word {
    Integer(i32),
    Address(Address),
    Native(u32),
}

impl Default for Word {
    fn default() -> Self {
        Self::Integer(0)
    }
}

impl Word {
    pub fn integer(self) -> io::Result<i32> {
        match self {
            Self::Integer(value) => Ok(value),
            _ => Err(invalid("expected a Lucifen integer")),
        }
    }

    pub fn address(self) -> io::Result<Address> {
        match self {
            Self::Address(value) => Ok(value),
            _ => Err(invalid("expected a Lucifen address")),
        }
    }

    fn truthy(self) -> bool {
        self != Self::Integer(0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct NativeCall {
    pub function: u32,
    pub arguments: Word,
    destination: Option<Address>,
}

#[derive(Clone, Copy, Debug)]
pub enum Step {
    Yield,
    Finished,
    Calling(NativeCall),
    BudgetExhausted,
}

struct Module {
    image: Vec<u8>,
    code_len: u32,
    relocations: BTreeMap<u32, Relocation>,
    writes: BTreeMap<u32, Word>,
}

impl Module {
    fn from_sob(sob: Sob<'_>) -> io::Result<Self> {
        Ok(Self {
            image: sob.code_and_data.to_vec(),
            code_len: sob.code.len() as u32,
            relocations: sob.relocations()?,
            writes: BTreeMap::new(),
        })
    }
}

struct Caller {
    module: usize,
    cursor: (usize, u32),
    sp: usize,
}

#[derive(Clone)]
struct MemoryImage {
    bytes: Vec<u8>,
    references: Vec<(usize, Word)>,
}

pub struct Vm {
    modules: BTreeMap<usize, Module>,
    next_module: usize,
    library: Option<usize>,
    cursor: (usize, u32),
    active_module: usize,
    memory: BTreeMap<Address, Word>,
    user_data: Vec<u8>,
    user_defaults: Vec<u8>,
    user_pointers: BTreeMap<u32, u32>,
    variable_checkpoint: BTreeMap<usize, MemoryImage>,
    variables: Vec<Variable>,
    live_slot_variables: BTreeSet<usize>,
    stack: Vec<u8>,
    sp: usize,
    return_value: Word,
    pending: Option<NativeCall>,
    finished: bool,
    callers: Vec<Caller>,
}

impl Vm {
    pub fn open(script: &[u8], library: Option<&[u8]>) -> io::Result<Self> {
        let script = Sob::parse(script)?;
        if script.kind != ModuleKind::Script {
            return Err(invalid("expected a SOB0 program"));
        }
        let mut modules = BTreeMap::from([(0, Module::from_sob(script)?)]);
        if let Some(library) = library {
            let library = Sob::parse(library)?;
            if library.kind != ModuleKind::Library {
                return Err(invalid("expected a SOB2 library"));
            }
            modules.insert(1, Module::from_sob(library)?);
        }
        Ok(Self {
            library: (modules.len() > 1).then_some(1),
            next_module: modules.len(),
            modules,
            cursor: (0, 0),
            active_module: 0,
            memory: BTreeMap::new(),
            user_data: Vec::new(),
            user_defaults: Vec::new(),
            user_pointers: BTreeMap::new(),
            variable_checkpoint: BTreeMap::new(),
            variables: Vec::new(),
            live_slot_variables: BTreeSet::new(),
            stack: vec![0; MAX_STACK_BYTES],
            sp: 4,
            return_value: Word::default(),
            pending: None,
            finished: false,
            callers: Vec::new(),
        })
    }

    pub(crate) fn set_user_variables(&mut self, bytes: &[u8]) -> io::Result<()> {
        let UserVariables {
            defaults,
            variables,
            pointers,
        } = UserVariables::parse(bytes)?;
        self.user_data = defaults.clone();
        self.user_defaults = defaults;
        self.variables = variables;
        self.user_pointers = pointers.into_iter().collect();
        for (&address, &target) in &self.user_pointers {
            self.memory.insert(
                Address::UserData(address),
                Word::Address(Address::UserData(target)),
            );
        }
        for (id, variable) in self.variables.iter().enumerate() {
            if variable.storage == 1 {
                self.memory.insert(
                    Address::Engine(id as u32),
                    Word::Address(Address::UserData(variable.offset)),
                );
            }
        }
        Ok(())
    }

    fn variable_destination(&self, id: usize, variable: Variable) -> io::Result<Address> {
        if variable.storage == 1 {
            self.read(Address::Engine(id as u32))?.address()
        } else {
            Ok(Address::UserData(variable.offset))
        }
    }

    fn variable_matches(variable: Variable, persistence: u8, storage: u8) -> bool {
        (storage == 0 || variable.storage == storage - 1)
            && (!(1..=3).contains(&persistence) || variable.persistence == persistence - 1)
    }

    pub(crate) fn change_variables(
        &mut self,
        operation: u32,
        persistence: u8,
        storage: u8,
    ) -> io::Result<()> {
        if storage > 3 {
            return Ok(());
        }
        for id in 0..self.variables.len() {
            let variable = self.variables[id];
            if !Self::variable_matches(variable, persistence, storage) {
                continue;
            }
            let destination = self.variable_destination(id, variable)?;
            match operation {
                27 => {
                    self.fill_memory(destination, Word::Integer(0), variable.bytes, false)?;
                    continue;
                }
                28 => {
                    let snapshot = self.read_memory(destination, variable.bytes)?;
                    self.variable_checkpoint.insert(id, snapshot);
                    continue;
                }
                29 => {
                    if let Some(snapshot) = self.variable_checkpoint.get(&id).cloned() {
                        self.write_memory(destination, snapshot)?;
                        continue;
                    }
                }
                _ => {}
            }
            let end = variable.offset as usize + variable.bytes as usize;
            let bytes = self.user_defaults[variable.offset as usize..end].to_vec();
            self.write_data(destination, &bytes)?;
            let pointers: Vec<_> = self
                .user_pointers
                .range(variable.offset..end as u32)
                .filter(|&(&position, _)| u64::from(position) + 4 <= end as u64)
                .map(|(&position, &target)| (position - variable.offset, target))
                .collect();
            for (offset, target) in pointers {
                self.write(
                    displace(destination, i64::from(offset))?,
                    Word::Address(Address::UserData(target)),
                )?;
            }
        }
        Ok(())
    }

    fn persisted_ranges(&self, group: u8) -> io::Result<Vec<(usize, u8, std::ops::Range<usize>)>> {
        self.variables
            .iter()
            .enumerate()
            .filter(|(_, variable)| variable.persistence == group && variable.storage <= 1)
            .map(|(id, variable)| {
                let offset = if variable.storage == 1 {
                    match self.read(Address::Engine(id as u32))? {
                        Word::Address(Address::UserData(offset)) => offset,
                        _ => {
                            return Err(invalid(
                                "Lucifen saved variable points outside its variable bank",
                            ));
                        }
                    }
                } else {
                    variable.offset
                } as usize;
                let end = offset
                    .checked_add(variable.bytes as usize)
                    .filter(|end| *end <= self.user_data.len())
                    .ok_or_else(|| invalid("Lucifen saved variable exceeds its variable bank"))?;
                Ok((id, variable.storage, offset..end))
            })
            .collect()
    }

    pub(crate) fn persistent_variables(&self, group: u8) -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        for (_, storage, range) in self.persisted_ranges(group)? {
            output.push(storage);
            output.extend_from_slice(&self.user_data[range]);
        }
        Ok(output)
    }

    pub(crate) fn register_slot_variable(&mut self, address: Address) {
        let matching = self.variables.iter().enumerate().find(|&(id, variable)| {
            if variable.storage == 1 {
                address == Address::Engine(id as u32)
            } else {
                address == Address::UserData(variable.offset)
            }
        });
        if let Some((id, _)) = matching {
            self.live_slot_variables.insert(id);
        }
    }

    pub(crate) fn clear_slot_variables(&mut self) {
        self.live_slot_variables.clear();
    }

    pub(crate) fn decode_slot(
        &mut self,
        bytes: &[u8],
    ) -> io::Result<(crate::state::SlotHistory, Option<Vec<u8>>)> {
        let (kind, data) = bytes
            .split_first()
            .ok_or_else(|| invalid("missing Lucifen slot state"))?;
        let group = kind
            .checked_add(1)
            .filter(|&group| group <= 2)
            .ok_or_else(|| invalid("invalid Lucifen slot variable group"))?;
        let size = self
            .persisted_ranges(group)?
            .iter()
            .map(|(_, _, range)| range.len() + 1)
            .sum();
        let (variables, tail) = data
            .split_at_checked(size)
            .ok_or_else(|| invalid("truncated Lucifen slot state"))?;
        let history = crate::state::SlotHistory::decode(tail)?;
        self.restore_variables(variables, group)?;
        Ok(history)
    }

    pub(crate) fn encode_slot(
        &mut self,
        history: &crate::state::SlotHistory,
        include_history: bool,
        checkpoint: Option<&[u8]>,
    ) -> io::Result<Vec<u8>> {
        for &id in &self.live_slot_variables {
            let variable = self.variables[id];
            let address = self.variable_destination(id, variable)?;
            let snapshot = self.read_memory(address, variable.bytes)?;
            self.variable_checkpoint.insert(id, snapshot);
        }
        let mut bytes = vec![0];
        for (id, storage, range) in self.persisted_ranges(1)? {
            bytes.push(storage);
            if let Some(snapshot) = self.variable_checkpoint.get(&id) {
                bytes.extend_from_slice(&snapshot.bytes);
            } else {
                bytes.extend_from_slice(&self.user_data[range]);
            }
        }
        history.encode(&mut bytes, include_history, checkpoint)?;
        Ok(bytes)
    }

    pub(crate) fn restore_variables(&mut self, mut input: &[u8], group: u8) -> io::Result<()> {
        let ranges = self.persisted_ranges(group)?;
        let mut values = Vec::with_capacity(ranges.len());
        for (_, storage, range) in ranges {
            let (kind, tail) = input
                .split_first()
                .ok_or_else(|| invalid("missing Lucifen saved variable"))?;
            if *kind != storage {
                return Err(invalid("Lucifen saved variable storage differs"));
            }
            let (bytes, tail) = tail
                .split_at_checked(range.len())
                .ok_or_else(|| invalid("truncated Lucifen saved variable"))?;
            values.push((range, bytes));
            input = tail;
        }
        if !input.is_empty() {
            return Err(invalid("unrecognized trailing Lucifen saved variables"));
        }
        for (range, bytes) in values {
            self.memory.retain(|address, _| {
                !matches!(address, Address::UserData(offset) if (*offset as usize) < range.end && (*offset as usize) + 4 > range.start)
            });
            self.user_data[range].copy_from_slice(bytes);
        }
        Ok(())
    }

    pub fn enter_script(&mut self, script: &[u8], resume_caller: bool) -> io::Result<()> {
        let script = Sob::parse(script)?;
        if script.kind != ModuleKind::Script {
            return Err(invalid("expected a SOB0 program"));
        }
        if self.pending.is_some() {
            return Err(invalid("cannot switch scripts during a native call"));
        }
        if self.callers.len() >= 256 || self.sp >= MAX_STACK_BYTES {
            return Err(invalid("Lucifen call or stack limit exceeded"));
        }
        let module = Module::from_sob(script)?;
        let id = self.next_module;
        self.next_module = id
            .checked_add(1)
            .ok_or_else(|| invalid("Lucifen module counter overflow"))?;
        if resume_caller {
            self.callers.push(Caller {
                module: self.active_module,
                cursor: self.cursor,
                sp: self.sp,
            });
        } else {
            self.modules.remove(&self.active_module);
        }
        self.active_module = id;
        self.cursor = (id, 0);
        self.modules.insert(id, module);
        self.push(Word::default())?;
        self.finished = false;
        Ok(())
    }

    pub(crate) fn resume_caller(&mut self) -> bool {
        if let Some(caller) = self.callers.pop() {
            self.modules.remove(&self.active_module);
            self.active_module = caller.module;
            self.cursor = caller.cursor;
            self.sp = caller.sp;
            self.finished = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn finish(&mut self) {
        self.callers.clear();
        self.finished = true;
    }

    pub fn read(&self, address: Address) -> io::Result<Word> {
        match address {
            Address::Stack(offset) => {
                if offset > MAX_STACK_BYTES - 4 {
                    return Err(invalid("Lucifen stack pointer outside its range"));
                }
                if let Some(&value) = self.memory.get(&address) {
                    return Ok(value);
                }
                let value = self.stack[offset..][..4].try_into().unwrap();
                Ok(Word::Integer(i32::from_le_bytes(value)))
            }
            Address::Image { module, offset } => {
                let image = self
                    .modules
                    .get(&module)
                    .ok_or_else(|| invalid("unknown SOB module"))?;
                if let Some(value) = image.writes.get(&offset).copied() {
                    return Ok(value);
                }
                if let Some(relocation) = image.relocations.get(&offset).copied() {
                    return self.relocate(module, relocation);
                }
                let bytes = image
                    .image
                    .get(offset as usize..)
                    .and_then(|bytes| bytes.first_chunk())
                    .ok_or_else(|| invalid("Lucifen word outside its SOB image"))?;
                Ok(Word::Integer(i32::from_le_bytes(*bytes)))
            }
            Address::UserData(offset) => {
                if let Some(value) = self.memory.get(&address).copied() {
                    return Ok(value);
                }
                let word = self
                    .user_data
                    .get(offset as usize..)
                    .and_then(|bytes| bytes.first_chunk())
                    .ok_or_else(|| invalid("Lucifen word outside its variable data"))?;
                Ok(Word::Integer(i32::from_le_bytes(*word)))
            }
            _ => Ok(self.memory.get(&address).copied().unwrap_or_default()),
        }
    }

    pub fn write(&mut self, address: Address, value: Word) -> io::Result<()> {
        if let Address::Stack(offset) = address {
            if let Word::Integer(value) = value {
                self.write_stack(offset, &value.to_le_bytes())?;
            } else {
                self.write_stack(offset, &[0; 4])?;
                self.memory.insert(address, value);
            }
        } else if let Address::Image { module, offset } = address {
            let image = self
                .modules
                .get_mut(&module)
                .filter(|image| u64::from(offset) + 4 <= image.image.len() as u64)
                .ok_or_else(|| invalid("Lucifen write outside its SOB image"))?;
            if let Word::Integer(value) = value {
                image.image[offset as usize..][..4].copy_from_slice(&value.to_le_bytes());
                image.relocations.remove(&offset);
                image.writes.remove(&offset);
            } else {
                image.writes.insert(offset, value);
            }
        } else if let Address::UserData(offset) = address {
            let bytes = self
                .user_data
                .get_mut(offset as usize..)
                .and_then(|bytes| bytes.first_chunk_mut())
                .ok_or_else(|| invalid("Lucifen word outside its variable data"))?;
            if let Word::Integer(value) = value {
                *bytes = value.to_le_bytes();
                self.memory.remove(&address);
            } else {
                self.memory.insert(address, value);
            }
        } else {
            self.memory.insert(address, value);
        }
        Ok(())
    }

    fn write_stack(&mut self, offset: usize, bytes: &[u8]) -> io::Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .filter(|&end| end <= MAX_STACK_BYTES)
            .ok_or_else(|| invalid("Lucifen stack write outside its range"))?;
        let replaced: Vec<_> = self
            .memory
            .range(Address::Stack(offset.saturating_sub(3))..Address::Stack(end))
            .map(|(&key, _)| key)
            .collect();
        for key in replaced {
            self.memory.remove(&key);
        }
        self.stack[offset..end].copy_from_slice(bytes);
        Ok(())
    }

    pub(crate) fn byte_tail(&self, address: Address) -> io::Result<Option<&[u8]>> {
        Ok(match address {
            Address::Stack(offset) => Some(
                self.stack
                    .get(offset..)
                    .ok_or_else(|| invalid("Lucifen string outside its stack"))?,
            ),
            Address::Image { module, offset } => Some(
                self.modules
                    .get(&module)
                    .and_then(|module| module.image.get(offset as usize..))
                    .ok_or_else(|| invalid("Lucifen string outside its SOB image"))?,
            ),
            Address::UserData(offset) => Some(
                self.user_data
                    .get(offset as usize..)
                    .ok_or_else(|| invalid("Lucifen string outside its variable data"))?,
            ),
            _ => None,
        })
    }

    pub(crate) fn host_image(
        &mut self,
        current: &mut Option<usize>,
        bytes: Vec<u8>,
    ) -> io::Result<usize> {
        let id = if let Some(id) = *current {
            id
        } else {
            let id = self.next_module;
            self.next_module = id
                .checked_add(1)
                .ok_or_else(|| invalid("Lucifen module counter overflow"))?;
            *current = Some(id);
            id
        };
        self.modules.insert(
            id,
            Module {
                image: bytes,
                code_len: 0,
                relocations: BTreeMap::new(),
                writes: BTreeMap::new(),
            },
        );
        Ok(id)
    }

    pub fn text(&self, address: Address) -> io::Result<Vec<u8>> {
        const MAX_TEXT_BYTES: usize = 1024 * 1024;
        if let Some(remaining) = self.byte_tail(address)? {
            let end = remaining
                .iter()
                .take(MAX_TEXT_BYTES)
                .position(|&byte| byte == 0)
                .ok_or_else(|| invalid("unterminated Lucifen string"))?;
            return Ok(remaining[..end].to_vec());
        }
        let mut result = Vec::new();
        for index in 0..MAX_TEXT_BYTES / 4 {
            let offset = displace(address, (index * 4) as i64)?;
            let word = self.read(offset)?.integer()?.to_le_bytes();
            if let Some(end) = word.iter().position(|&byte| byte == 0) {
                result.extend_from_slice(&word[..end]);
                return Ok(result);
            }
            result.extend_from_slice(&word);
        }
        Err(invalid("unterminated Lucifen string"))
    }

    pub fn write_text(&mut self, address: Address, bytes: &[u8]) -> io::Result<()> {
        let mut data = bytes.to_vec();
        data.push(0);
        self.write_data(address, &data)
    }

    pub(crate) fn fill_memory(
        &mut self,
        address: Address,
        value: Word,
        count: u32,
        words: bool,
    ) -> io::Result<()> {
        let size = memory_size(count, if words { 4 } else { 1 })?;
        match (words, value) {
            (true, Word::Integer(value)) => {
                let pattern = value.to_le_bytes();
                let mut bytes = vec![0; size];
                for word in bytes.chunks_exact_mut(pattern.len()) {
                    word.copy_from_slice(&pattern);
                }
                self.write_data(address, &bytes)
            }
            (true, _) => {
                for offset in (0..size).step_by(4) {
                    self.write(displace(address, offset as i64)?, value)?;
                }
                Ok(())
            }
            (false, value) => self.write_data(address, &vec![value.integer()? as u8; size]),
        }
    }

    pub(crate) fn copy_memory(
        &mut self,
        destination: Address,
        source: Address,
        count: u32,
    ) -> io::Result<()> {
        let image = self.read_memory(source, count)?;
        self.write_memory(destination, image)
    }

    fn read_memory(&self, source: Address, count: u32) -> io::Result<MemoryImage> {
        let size = memory_size(count, 1)?;
        let mut bytes = Vec::with_capacity(size);
        let mut references = Vec::new();
        for offset in (0..size).step_by(4) {
            let address = displace(source, offset as i64)?;
            let length = (size - offset).min(4);
            if length < 4 {
                let tail = match address {
                    Address::Stack(offset) => self.stack.get(offset..),
                    Address::Image { module, offset } => self
                        .modules
                        .get(&module)
                        .and_then(|module| module.image.get(offset as usize..)),
                    Address::UserData(offset) => self.user_data.get(offset as usize..),
                    _ => None,
                };
                if let Some(tail) = tail {
                    bytes.extend_from_slice(
                        tail.get(..length)
                            .ok_or_else(|| invalid("Lucifen memory read out of bounds"))?,
                    );
                    continue;
                }
            }
            match self.read(address)? {
                Word::Integer(value) => bytes.extend_from_slice(&value.to_le_bytes()[..length]),
                word if length == 4 => {
                    references.push((offset, word));
                    bytes.extend_from_slice(&[0; 4]);
                }
                _ => return Err(invalid("Lucifen memory copy cuts through a reference")),
            }
        }
        Ok(MemoryImage { bytes, references })
    }

    fn write_memory(&mut self, destination: Address, image: MemoryImage) -> io::Result<()> {
        self.write_data(destination, &image.bytes)?;
        for (offset, value) in image.references {
            self.write(displace(destination, offset as i64)?, value)?;
        }
        Ok(())
    }

    pub(crate) fn write_data(&mut self, address: Address, bytes: &[u8]) -> io::Result<()> {
        let count = bytes.len();
        if count == 0 {
            return Ok(());
        }
        if let Address::Stack(offset) = address {
            return self.write_stack(offset, bytes);
        }
        if let Address::Image { module, offset } = address {
            let image = self
                .modules
                .get_mut(&module)
                .ok_or_else(|| invalid("unknown SOB module"))?;
            let destination = image
                .image
                .get_mut(offset as usize..)
                .and_then(|buffer| buffer.get_mut(..count))
                .ok_or_else(|| invalid("Lucifen byte write outside its SOB image"))?;
            destination.copy_from_slice(bytes);
            let end = u64::from(offset) + count as u64;
            let overlaps = |position: u32| {
                u64::from(position) < end && u64::from(position) + 4 > u64::from(offset)
            };
            image.relocations.retain(|position, _| !overlaps(*position));
            image.writes.retain(|position, _| !overlaps(*position));
            return Ok(());
        }
        if let Address::UserData(offset) = address {
            let destination = self
                .user_data
                .get_mut(offset as usize..)
                .and_then(|buffer| buffer.get_mut(..count))
                .ok_or_else(|| invalid("Lucifen byte write outside its variable data"))?;
            destination.copy_from_slice(bytes);
            let end = u64::from(offset) + count as u64;
            self.memory.retain(|key, _| {
                !matches!(key, Address::UserData(position) if u64::from(*position) < end && u64::from(*position) + 4 > u64::from(offset))
            });
            return Ok(());
        }
        displace(address, ((count - 1) & !3) as i64)?;
        for (index, chunk) in bytes.chunks(4).enumerate() {
            let target = displace(address, (index * 4) as i64)?;
            let mut word = if chunk.len() < 4 {
                self.read(target)?.integer()?.to_le_bytes()
            } else {
                [0; 4]
            };
            word[..chunk.len()].copy_from_slice(chunk);
            self.write(target, Word::Integer(i32::from_le_bytes(word)))?;
        }
        Ok(())
    }

    pub(crate) fn host_text(&mut self, bytes: &[u8]) -> Word {
        // The application configuration exposes a stable C string pointer.
        // This module is data-only and cannot be used as a jump target.
        const HOST_TEXT: usize = usize::MAX;
        let mut image = bytes.to_vec();
        image.push(0);
        self.modules.insert(
            HOST_TEXT,
            Module {
                image,
                code_len: 0,
                relocations: BTreeMap::new(),
                writes: BTreeMap::new(),
            },
        );
        Word::Address(Address::Image {
            module: HOST_TEXT,
            offset: 0,
        })
    }

    pub fn argument(&self, call: NativeCall, index: usize) -> io::Result<Word> {
        let offset = index
            .checked_mul(4)
            .and_then(|bytes| i64::try_from(bytes).ok())
            .ok_or_else(|| invalid("Lucifen native argument offset overflow"))?;
        let address = displace(call.arguments.address()?, offset)?;
        self.read(address)
    }

    pub fn complete_call(&mut self, result: Word) -> io::Result<()> {
        let call = self
            .pending
            .take()
            .ok_or_else(|| invalid("no pending Lucifen native call"))?;
        if let Some(destination) = call.destination {
            self.write(destination, result)?;
        }
        Ok(())
    }

    pub fn advance(&mut self, instruction_budget: usize) -> io::Result<Step> {
        if let Some(call) = self.pending {
            return Ok(Step::Calling(call));
        }
        if self.finished {
            return Ok(Step::Finished);
        }
        for _ in 0..instruction_budget {
            let pc = self.cursor.1;
            if let Some(step) = self.execute().map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("SOB instruction at 0x{pc:X}: {error}"),
                )
            })? {
                return Ok(step);
            }
        }
        Ok(Step::BudgetExhausted)
    }

    fn execute(&mut self) -> io::Result<Option<Step>> {
        let opcode = self.fetch()?.integer()?;
        match opcode {
            0 => return Ok(Some(Step::Yield)),
            1 | 2 => {
                if opcode == 2 {
                    self.return_value = self.fetch()?;
                }
                let target = self.pop()?;
                if target == Word::Integer(0) {
                    self.finished = true;
                    return Ok(Some(Step::Finished));
                }
                self.jump(target)?;
            }
            3 => {
                let destination = self.fetch()?.address()?;
                self.write(destination, self.return_value)?;
            }
            4 | 6 => {
                let target = self.fetch()?;
                if opcode == 4 {
                    let (module, offset) = self.cursor;
                    self.push(Word::Address(Address::Image { module, offset }))?;
                }
                self.jump(target)?;
            }
            5 => {
                let Word::Native(function) = self.fetch()? else {
                    return Err(invalid("Lucifen native call lacks a function relocation"));
                };
                let arguments = self.fetch()?;
                let destination = match self.fetch()? {
                    Word::Integer(0) => None,
                    word => Some(word.address()?),
                };
                let call = NativeCall {
                    function,
                    arguments,
                    destination,
                };
                self.pending = Some(call);
                return Ok(Some(Step::Calling(call)));
            }
            7 => {
                let index = self.fetch()?.integer()?;
                let bytes = u32::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_mul(4))
                    .ok_or_else(|| invalid("invalid Lucifen jump-table index"))?;
                self.cursor.1 = self
                    .cursor
                    .1
                    .checked_add(bytes)
                    .ok_or_else(|| invalid("Lucifen jump-table offset overflow"))?;
                let target = self.fetch()?;
                self.jump(target)?;
            }
            8 | 9 => {
                let word = self.fetch()?;
                let word = if opcode == 9 {
                    self.read(word.address()?)?
                } else {
                    word
                };
                self.push(word)?;
            }
            10 => {
                let value = self.pop()?;
                let address = self.fetch()?.address()?;
                self.write(address, value)?;
            }
            11 | 12 => {
                let bytes = self.fetch()?;
                let slot = self.stack_address(bytes, opcode == 11)?;
                self.sp = slot;
            }
            13..=21 | 24 | 25 | 41..=46 => self.binary(opcode)?,
            22 | 23 => {
                let word = self.fetch()?;
                let result = if opcode == 22 {
                    !word.integer()?
                } else {
                    i32::from(!word.truthy())
                };
                let address = self.fetch()?.address()?;
                self.write(address, Word::Integer(result))?;
            }
            26..=29 => {
                let operand = self.fetch()?;
                let address = if opcode < 28 {
                    operand.address()?
                } else {
                    Address::Stack(self.stack_address(operand, false)?)
                };
                let current = self.read(address)?.integer()?;
                let changed = current.wrapping_add(if opcode & 1 == 0 { 1 } else { -1 });
                self.write(address, Word::Integer(changed))?;
            }
            30 => {
                let condition = self.fetch()?;
                let target = self.fetch()?;
                if !condition.truthy() {
                    self.jump(target)?;
                }
            }
            31..=40 => self.move_word(opcode)?,
            unknown => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("unsupported Lucifen opcode {unknown}"),
                ));
            }
        }
        Ok(None)
    }

    fn fetch(&mut self) -> io::Result<Word> {
        let (module, offset) = self.cursor;
        if u64::from(offset) + 4 > u64::from(self.modules[&module].code_len) {
            return Err(invalid("instruction outside the SOB code section"));
        }
        let value = self.read(Address::Image { module, offset })?;
        self.cursor.1 += 4;
        Ok(value)
    }

    fn jump(&mut self, value: Word) -> io::Result<()> {
        let Address::Image { module, offset } = value.address()? else {
            return Err(invalid("Lucifen branch target is not executable"));
        };
        if self
            .modules
            .get(&module)
            .is_none_or(|image| offset >= image.code_len)
            || offset % 4 != 0
        {
            return Err(invalid(
                "Lucifen branch target outside the SOB code section",
            ));
        }
        self.cursor = (module, offset);
        Ok(())
    }

    fn relocate(&self, module: usize, reference: Relocation) -> io::Result<Word> {
        Ok(match reference {
            Relocation::NativeFunction(id) => Word::Native(id),
            Relocation::EngineVariable(id) => {
                let address = self
                    .variables
                    .get(id as usize)
                    .filter(|variable| variable.storage != 1)
                    .map_or(Address::Engine(id), |variable| {
                        Address::UserData(variable.offset)
                    });
                Word::Address(address)
            }
            Relocation::UserVariable(id) => Word::Address(Address::User(id)),
            Relocation::LibraryOffset(offset) => Word::Address(Address::Image {
                module: self
                    .library
                    .ok_or_else(|| invalid("SOB function library is required"))?,
                offset,
            }),
            Relocation::ImageOffset(offset) => Word::Address(Address::Image { module, offset }),
        })
    }

    fn push(&mut self, value: Word) -> io::Result<()> {
        self.write(Address::Stack(self.sp), value)?;
        self.sp += 4;
        Ok(())
    }

    fn pop(&mut self) -> io::Result<Word> {
        self.sp = self
            .sp
            .checked_sub(4)
            .ok_or_else(|| invalid("Lucifen stack underflow"))?;
        self.read(Address::Stack(self.sp))
    }

    fn stack_address(&self, operand: Word, add: bool) -> io::Result<usize> {
        let amount = i64::from(operand.integer()?);
        let bytes = if add { amount } else { -amount };
        let bytes = isize::try_from(bytes).map_err(|_| invalid("Lucifen stack offset overflow"))?;
        self.sp
            .checked_add_signed(bytes)
            .filter(|&index| index <= MAX_STACK_BYTES)
            .ok_or_else(|| invalid("Lucifen stack pointer outside its range"))
    }

    fn binary(&mut self, opcode: i32) -> io::Result<()> {
        let left = self.fetch()?;
        let right = self.fetch()?;
        let value = match (opcode, left, right) {
            (45, _, _) => Word::Integer(i32::from(left == right)),
            (46, _, _) => Word::Integer(i32::from(left != right)),
            (13, Word::Address(address), Word::Integer(bytes))
            | (13, Word::Integer(bytes), Word::Address(address)) => {
                Word::Address(displace(address, i64::from(bytes))?)
            }
            (14, Word::Address(address), Word::Integer(bytes)) => {
                Word::Address(displace(address, -i64::from(bytes))?)
            }
            (_, Word::Integer(left), Word::Integer(right)) => {
                Word::Integer(arithmetic(opcode, left, right)?)
            }
            _ => return Err(invalid("unsupported Lucifen pointer arithmetic")),
        };
        let destination = self.fetch()?.address()?;
        self.write(destination, value)
    }

    fn move_word(&mut self, opcode: i32) -> io::Result<()> {
        let first = self.fetch()?;
        let second = self.fetch()?;
        let value = match opcode {
            31 | 33 | 34 => first,
            32 | 36 => self.read(first.address()?)?,
            35 | 37 | 38 => {
                let value = self.read(Address::Stack(self.stack_address(first, false)?))?;
                if opcode == 38 {
                    self.read(value.address()?)?
                } else {
                    value
                }
            }
            39 => self.read(self.read(first.address()?)?.address()?)?,
            40 => Word::Address(Address::Stack(self.stack_address(first, false)?)),
            _ => unreachable!("checked Lucifen move opcode"),
        };
        let destination = match opcode {
            33 => self.read(second.address()?)?.address()?,
            34 | 36 | 37 => Address::Stack(self.stack_address(second, false)?),
            _ => second.address()?,
        };
        self.write(destination, value)
    }
}

fn memory_size(count: u32, element_size: usize) -> io::Result<usize> {
    (count as usize)
        .checked_mul(element_size)
        .filter(|&size| size <= MAX_MEMORY_OPERATION)
        .ok_or_else(|| invalid("Lucifen memory operation exceeds its limit"))
}

pub(crate) fn displace(address: Address, bytes: i64) -> io::Result<Address> {
    let error = || invalid("Lucifen pointer displacement overflow");
    match address {
        Address::Image { module, offset } => {
            let offset = u32::try_from(i64::from(offset).checked_add(bytes).ok_or_else(error)?)
                .map_err(|_| error())?;
            Ok(Address::Image { module, offset })
        }
        Address::UserData(offset) => {
            let offset = u32::try_from(i64::from(offset).checked_add(bytes).ok_or_else(error)?)
                .map_err(|_| error())?;
            Ok(Address::UserData(offset))
        }
        Address::Stack(offset) => {
            let bytes = isize::try_from(bytes).map_err(|_| error())?;
            let offset = offset.checked_add_signed(bytes).ok_or_else(error)?;
            Ok(Address::Stack(offset))
        }
        _ if bytes == 0 => Ok(address),
        _ => Err(invalid("unsupported Lucifen pointer displacement")),
    }
}

fn arithmetic(opcode: i32, left: i32, right: i32) -> io::Result<i32> {
    let division = || invalid("invalid Lucifen integer division");
    Ok(match opcode {
        13 => left.wrapping_add(right),
        14 => left.wrapping_sub(right),
        15 => left.wrapping_mul(right),
        16 => left.checked_div(right).ok_or_else(division)?,
        17 => left.checked_rem(right).ok_or_else(division)?,
        18 => {
            let result = left.checked_rem(right).ok_or_else(division)?;
            if result < 0 {
                result.wrapping_add(right)
            } else {
                result
            }
        }
        19 => left & right,
        20 => left | right,
        21 => left ^ right,
        24 => left.wrapping_shr(right as u32),
        25 => left.wrapping_shl(right as u32),
        41 => i32::from(left < right),
        42 => i32::from(left <= right),
        43 => i32::from(left > right),
        44 => i32::from(left >= right),
        _ => unreachable!("checked Lucifen arithmetic opcode"),
    })
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_supports_packed_native_strings_and_unaligned_references() {
        let mut program = b"SOB0".to_vec();
        program.extend(24u32.to_le_bytes());
        program.extend([0; 24]);
        program.extend([4u32, 4, 1].into_iter().flat_map(u32::to_le_bytes));
        let mut vm = Vm::open(&program, None).unwrap();
        let slot = Address::Stack(5);
        let previous = Word::Address(Address::Stack(22));
        vm.write(slot, previous).unwrap();
        assert_eq!(vm.read(slot).unwrap(), previous);
        let text = displace(slot, 1).unwrap();
        vm.write_text(text, b"AB").unwrap();
        assert_eq!(vm.text(text).unwrap(), b"AB");
        assert_eq!(vm.read(slot).unwrap(), Word::Integer(0x00424100));
    }

    #[test]
    fn variable_checkpoint_preserves_relocated_references() {
        let mut program = b"SOB0".to_vec();
        program.extend(24u32.to_le_bytes());
        program.extend([0; 24]);
        program.extend([4u32, 4, 1].into_iter().flat_map(u32::to_le_bytes));
        let mut vm = Vm::open(&program, None).unwrap();

        let mut user = b"SOB1".to_vec();
        user.extend([8u32, 3, 0, 1].into_iter().flat_map(u32::to_le_bytes));
        user.extend([0, 1]);
        user.extend([0u32, 8, 1, 4, 0, 0].into_iter().flat_map(u32::to_le_bytes));
        vm.set_user_variables(&user).unwrap();
        vm.write(Address::UserData(0), Word::Integer(17)).unwrap();
        vm.change_variables(28, 2, 0).unwrap();
        vm.change_variables(27, 2, 0).unwrap();
        assert_eq!(vm.read(Address::UserData(4)).unwrap(), Word::Integer(0));
        vm.change_variables(29, 2, 0).unwrap();
        assert_eq!(vm.read(Address::UserData(0)).unwrap(), Word::Integer(17));
        assert_eq!(
            vm.read(Address::UserData(4)).unwrap(),
            Word::Address(Address::UserData(0))
        );
        vm.change_variables(26, 2, 0).unwrap();
        assert_eq!(vm.read(Address::UserData(0)).unwrap(), Word::Integer(3));
        assert_eq!(
            vm.read(Address::UserData(4)).unwrap(),
            Word::Address(Address::UserData(0))
        );
    }

    #[test]
    fn slot_persists_current_state_and_history_alongside_the_script_checkpoint() {
        let mut program = b"SOB0".to_vec();
        program.extend(24u32.to_le_bytes());
        program.extend([0; 24]);
        program.extend([4u32, 4, 1].into_iter().flat_map(u32::to_le_bytes));
        let mut vm = Vm::open(&program, None).unwrap();
        let mut user = b"SOB1".to_vec();
        user.extend([8u32, 3, 0, 1].into_iter().flat_map(u32::to_le_bytes));
        user.extend([0, 1]);
        user.extend([0u32, 8, 1, 4, 0, 0].into_iter().flat_map(u32::to_le_bytes));
        vm.set_user_variables(&user).unwrap();
        let position = Address::UserData(0);
        vm.write(position, Word::Integer(17)).unwrap();
        vm.change_variables(28, 2, 0).unwrap();
        let mut history = crate::state::SlotHistory::default();
        history
            .push(
                &crate::dialogue::Packet::from_history([0; 3], None, b"first".to_vec()),
                4,
            )
            .unwrap();
        let checkpoint = vm.encode_slot(&history, true, None).unwrap();
        vm.write(position, Word::Integer(42)).unwrap();
        vm.change_variables(28, 2, 0).unwrap();
        history
            .push(
                &crate::dialogue::Packet::from_history([0; 3], None, b"later".to_vec()),
                4,
            )
            .unwrap();
        let slot = vm.encode_slot(&history, true, Some(&checkpoint)).unwrap();
        vm.write(position, Word::Integer(0)).unwrap();

        let (mut loaded, earlier) = vm.decode_slot(&slot).unwrap();
        assert_eq!(vm.read(position).unwrap(), Word::Integer(42));
        assert_eq!(loaded.select(1).unwrap().unwrap().body, b"later");
        assert_eq!(earlier.as_deref(), Some(checkpoint.as_slice()));
        let (mut loaded, extension) = vm.decode_slot(&earlier.unwrap()).unwrap();
        assert_eq!(vm.read(position).unwrap(), Word::Integer(17));
        assert_eq!(loaded.select(0).unwrap().unwrap().body, b"first");
        assert!(extension.is_none());

        vm.register_slot_variable(position);
        vm.write(position, Word::Integer(64)).unwrap();
        vm.encode_slot(&history, false, None).unwrap();
        vm.clear_slot_variables();
        vm.write(position, Word::Integer(81)).unwrap();
        let slot = vm.encode_slot(&history, false, None).unwrap();
        vm.decode_slot(&slot).unwrap();
        assert_eq!(vm.read(position).unwrap(), Word::Integer(64));
    }

    #[test]
    fn native_call_relocates_result_into_the_next_instruction() {
        let mut bytes = b"SOB0".to_vec();
        bytes.extend(48u32.to_le_bytes());
        // Header relocation groups contain one native function and two
        // positions inside this module. The native result is added to three.
        let reloc = [0u32, 1, 4, 55, 0, 0, 2, 12, 20, 28, 36, 0];
        bytes.extend(reloc.into_iter().flat_map(u32::to_le_bytes));
        bytes.extend(36u32.to_le_bytes());
        bytes.extend(40u32.to_le_bytes());
        let program = [5u32, 0, 0, 0, 13, 0, 3, 0, 1, 0];
        bytes.extend(program.into_iter().flat_map(u32::to_le_bytes));
        let mut vm = Vm::open(&bytes, None).unwrap();
        let Step::Calling(call) = vm.advance(8).unwrap() else {
            panic!("expected a native call")
        };
        assert_eq!(call.function, 55);
        vm.complete_call(Word::Integer(17)).unwrap();
        assert!(matches!(vm.advance(8).unwrap(), Step::Finished));
        assert_eq!(
            vm.read(Address::Image {
                module: 0,
                offset: 36
            })
            .unwrap(),
            Word::Integer(20)
        );
    }
}
