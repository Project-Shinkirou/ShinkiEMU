use crate::{Error, Result};

pub(crate) fn range(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    data.get(
        offset
            ..offset
                .checked_add(len)
                .ok_or(Error::Invalid("file range"))?,
    )
    .ok_or(Error::Invalid("file range"))
}

pub(crate) fn word(data: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        range(data, offset, 2)?.try_into().expect("word"),
    ))
}

pub(crate) fn dword(data: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        range(data, offset, 4)?.try_into().expect("dword"),
    ))
}

pub(crate) fn c_string(bytes: &[u8]) -> Result<&[u8]> {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .ok_or(Error::Invalid("unterminated field"))?;
    Ok(&bytes[..end])
}

pub(crate) fn decode_japanese(bytes: &[u8]) -> Result<String> {
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling_and_without_replacement(bytes)
        .map(std::borrow::Cow::into_owned)
        .ok_or(Error::Invalid("CP932 string"))
}
