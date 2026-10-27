use shinki_emu_sdk::CoreError;

pub(crate) fn invalid(message: &'static str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_SLOT", message)
}

/// A bounded view of one native record; sub-records retain their own limits.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    pub(crate) fn bytes(&mut self, count: usize) -> Result<&'a [u8], CoreError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| invalid("native record length overflowed"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| invalid("native record is truncated"))?;
        self.position = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, CoreError> {
        Ok(self.bytes(1)?[0])
    }

    pub(crate) fn u16(&mut self) -> Result<u16, CoreError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CoreError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    pub(crate) fn f32(&mut self) -> Result<f32, CoreError> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub(crate) fn c_string(&mut self, maximum: usize) -> Result<&'a [u8], CoreError> {
        let available = self.remaining().min(maximum);
        let count = self.bytes[self.position..self.position + available]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid("native string is unterminated or exceeds its limit"))?;
        let value = self.bytes(count + 1)?;
        Ok(&value[..count])
    }

    pub(crate) fn finish(self) -> Result<(), CoreError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(invalid("native record has trailing bytes"))
        }
    }
}
