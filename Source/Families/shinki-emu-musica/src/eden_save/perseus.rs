//! Historical extended-layout codec with SerialCode header validation.
//! Runtime paths and naming come from SaveIdentity, not this codec.
use super::*;
use crate::minori_data::FilePrefix;

/// `sub_46A860` reads the u32 header first and fails the load when it differs
/// from the SerialCode CRC32 (`sub_47EFB0`). A non-strict prefix does not come
/// from the input the original reads, so it does not reject.
pub fn decode_slot(
    bytes: &[u8],
    prefix: FilePrefix,
    encoding: EdenSaveEncoding,
) -> Result<EdenSave, CoreError> {
    if prefix.strict && bytes.get(..4) != Some(&prefix.value.to_le_bytes()[..]) {
        return Err(invalid(
            "SIGNATURE",
            "perseus slot header does not match the SerialCode prefix",
        ));
    }
    EdenSave::decode(bytes, EdenEdition::Perseus, encoding)
}

/// `sub_470C50` writes the resolved prefix as header; a slot carrying another
/// prefix would be rejected by the original on load.
pub fn encode_slot(save: &EdenSave, prefix: FilePrefix) -> Result<Vec<u8>, CoreError> {
    if save.edition != EdenEdition::Perseus || save.prefix != prefix.value {
        return Err(invalid(
            "SIGNATURE",
            "perseus slot header does not match the SerialCode prefix",
        ));
    }
    save.encode()
}
