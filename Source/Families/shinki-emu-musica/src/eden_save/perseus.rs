//! perseus.exe slot files `perseus_%04d.sav`. Header and body share the eden
//! container (`EdenEdition::Perseus`); this module adds the slot file name and
//! the SerialCode header binding of the original reader and writer.
use super::*;
use crate::minori_data::FilePrefix;

/// `sub_4866C0`, `sub_4867A0` and `sub_483520` refuse slots above 9999 and
/// append `.sav` to `perseus_%04d`.
pub fn slot_file_name(slot: u32) -> Result<String, CoreError> {
    if slot > 9999 {
        return Err(invalid("BOUND", "perseus slot number exceeds 9999"));
    }
    Ok(format!("perseus_{slot:04}.sav"))
}

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
