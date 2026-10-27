use shinki_emu_sdk::CoreError;

pub const MGV1_MAGIC: &[u8; 4] = b"MGV1";
const MGV1_HEADER_BYTES: usize = 32;
const MGV1_INDEX_ENTRY_BYTES: usize = 4;
const MAX_MGV1_INDEX_BYTES: usize = 64 * 1024 * 1024;

/// Returns the embedded Ogg stream after validating the MGV1 framing.
///
/// The original `mog3x` input path copies its fixed 32-byte header, then an
/// index of `header[4] * 4` bytes, then the embedded Ogg byte range. The
/// remaining video payload is not interpreted here.
pub fn mgv1_embedded_ogg(source: &[u8]) -> Result<&[u8], CoreError> {
    let header = source
        .get(..MGV1_HEADER_BYTES)
        .ok_or_else(|| invalid("SHINKI_EMU_CMVS_MGV_HEADER", "MGV1 header is truncated"))?;
    if &header[..4] != MGV1_MAGIC {
        return Err(invalid(
            "SHINKI_EMU_CMVS_MGV_MAGIC",
            "MGV container magic is unsupported",
        ));
    }
    if le_u32(header, 4) as usize != MGV1_HEADER_BYTES {
        return Err(invalid(
            "SHINKI_EMU_CMVS_MGV_HEADER",
            "MGV1 header length does not match the fixed input contract",
        ));
    }
    let index_bytes = (le_u32(header, 16) as usize)
        .checked_mul(MGV1_INDEX_ENTRY_BYTES)
        .filter(|bytes| *bytes <= MAX_MGV1_INDEX_BYTES)
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_MGV_INDEX",
                "MGV1 index exceeds the bounded media policy",
            )
        })?;
    let start = MGV1_HEADER_BYTES + index_bytes;
    let ogg = start
        .checked_add(le_u32(header, 12) as usize)
        .and_then(|end| source.get(start..end))
        .ok_or_else(|| {
            invalid(
                "SHINKI_EMU_CMVS_MGV_RANGE",
                "MGV1 index or embedded Ogg range exceeds the source",
            )
        })?;
    if !ogg.starts_with(b"OggS") {
        return Err(invalid(
            "SHINKI_EMU_CMVS_MGV_OGG",
            "MGV1 embedded audio is not an Ogg stream",
        ));
    }
    Ok(ogg)
}

fn le_u32(header: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        header[offset..offset + 4]
            .try_into()
            .expect("offset lies inside the 32-byte header"),
    )
}

fn invalid(code: &'static str, message: &'static str) -> CoreError {
    CoreError::invalid(code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0_u8; 32 + 12 + 5 + 9];
        bytes[..4].copy_from_slice(MGV1_MAGIC);
        bytes[4..8].copy_from_slice(&32_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&5_u32.to_le_bytes());
        bytes[16..20].copy_from_slice(&3_u32.to_le_bytes());
        bytes[44..48].copy_from_slice(b"OggS");
        bytes
    }

    #[test]
    fn returns_only_the_bounded_embedded_ogg_stream() {
        assert_eq!(mgv1_embedded_ogg(&fixture()).unwrap(), b"OggS\0");
    }

    #[test]
    fn rejects_a_truncated_embedded_ogg_range() {
        let mut bytes = fixture();
        bytes.truncate(48);
        assert_eq!(
            mgv1_embedded_ogg(&bytes).unwrap_err().code(),
            "SHINKI_EMU_CMVS_MGV_RANGE"
        );
    }

    #[test]
    fn rejects_a_non_ogg_embedded_audio_range() {
        let mut bytes = fixture();
        bytes[44..48].copy_from_slice(b"RIFF");
        assert_eq!(
            mgv1_embedded_ogg(&bytes).unwrap_err().code(),
            "SHINKI_EMU_CMVS_MGV_OGG"
        );
    }
}
