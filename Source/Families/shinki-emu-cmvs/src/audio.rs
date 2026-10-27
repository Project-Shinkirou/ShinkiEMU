use crate::{CmvsArchive, MGV1_MAGIC, mgv1_embedded_ogg};
use kira::sound::static_sound::StaticSoundData;
use shinki_emu_sdk::{CoreError, decode_audio};
use std::sync::atomic::{AtomicBool, Ordering};
mod native_mp3;
mod native_mv2;
mod native_mv2_tables;
mod native_pcm;
pub(crate) mod native_stretch;

pub(crate) struct NativeDecodedAudio {
    // A filename/header may define extent before the PCM consumer reaches
    // an unrecovered/invalid boundary. Keep those two results independent.
    pub data: Result<StaticSoundData, CoreError>,
    /// MV2's40C300 forces shift4 on repeat; absent when first/repeat agree.
    pub repeat_data: Option<StaticSoundData>,
    pub channels: u16,
    pub extent: Option<u32>,
    pub restart_rewinds: bool,
}

const MAX_ENCODED_AUDIO_BYTES: u64 = 64 * 1024 * 1024;

impl CmvsArchive {
    /// Decode ordinary audio or a validated MGV1 embedded Ogg stream.
    /// This does not interpret MGV video timing or create a playback worker.
    pub fn load_audio(
        &self,
        uri: &str,
        max_frames: usize,
        cancelled: &AtomicBool,
    ) -> Result<StaticSoundData, CoreError> {
        let source = self.read_audio_source(uri, cancelled)?;
        decode_source(source.as_slice(), max_frames, cancelled)
    }

    pub(crate) fn load_native_audio(
        &self,
        uri: &str,
        native_name: &str,
        max_frames: usize,
        cancelled: &AtomicBool,
    ) -> Result<NativeDecodedAudio, CoreError> {
        let source = self.read_audio_source(uri, cancelled)?;
        let extension = native_name
            .rsplit_once('.')
            .map(|(_, extension)| extension)
            .unwrap_or("");
        let ogg = extension.eq_ignore_ascii_case("ogg") || extension.eq_ignore_ascii_case("mog");
        if !ogg {
            let mp3 = extension.eq_ignore_ascii_case("mp3");
            let mv2 = extension.eq_ignore_ascii_case("mv2");
            let decoded = if mp3 {
                native_mp3::decode(&source, max_frames, cancelled)
            } else if mv2 {
                native_mv2::decode(&source, max_frames, cancelled)
            } else {
                // .wav/.wma/.mv/unknown all select CMusicPCM.
                native_pcm::decode(&source, max_frames, cancelled)
            };
            //48DB70 ignores these decoder load callbacks' return values.
            //Only resource/name failures above are early loader failures.
            return Ok(decoded.unwrap_or_else(|cause| NativeDecodedAudio {
                //490EA0 sets physical length84 before attempting RIFF parse.
                data: Err(cause),
                repeat_data: None,
                channels: 0,
                extent: if mp3 || mv2 {
                    None
                } else {
                    Some(source.len() as u32)
                },
                restart_rewinds: false,
            }));
        }
        use symphonia::core::{codecs::CodecParameters, formats::TrackType, io::MediaSourceStream};
        let encoded = if source.starts_with(MGV1_MAGIC) {
            mgv1_embedded_ogg(&source)?
        } else {
            &source
        };
        let stream =
            MediaSourceStream::new(Box::new(std::io::Cursor::new(encoded)), Default::default());
        let reader = symphonia::default::get_probe()
            .probe(
                &Default::default(),
                stream,
                Default::default(),
                Default::default(),
            )
            .map_err(|_| {
                CoreError::invalid(
                    "SHINKI_EMU_CMVS_AUDIO_METADATA",
                    "native MOG source format is unavailable",
                )
            })?;
        let params = reader
            .default_track(TrackType::Audio)
            .and_then(|track| track.codec_params.as_ref())
            .and_then(|params| match params {
                CodecParameters::Audio(audio) => Some(audio),
                _ => None,
            })
            .ok_or_else(|| {
                CoreError::invalid(
                    "SHINKI_EMU_CMVS_AUDIO_METADATA",
                    "native MOG decoder has no audio track",
                )
            })?;
        if params.codec != symphonia::core::codecs::audio::well_known::CODEC_ID_VORBIS {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_NATIVE_AUDIO_CODEC",
                "filename selects the native MOG/Vorbis decoder",
            ));
        }
        let channels = params
            .channels
            .as_ref()
            .map(|channels| channels.count() as u16)
            .filter(|channels| matches!(channels, 1 | 2))
            .ok_or_else(|| {
                CoreError::invalid(
                    "SHINKI_EMU_CMVS_AUDIO_METADATA",
                    "native MOG source is not mono or stereo",
                )
            })?;
        let extent = if encoded.starts_with(b"OggS") {
            Some(ogg_pcm_extent(encoded)?)
        } else {
            None
        };
        Ok(NativeDecodedAudio {
            data: decode_source(&source, max_frames, cancelled),
            channels,
            extent,
            repeat_data: None,
            restart_rewinds: true,
        })
    }

    fn read_audio_source(
        &self,
        uri: &str,
        cancelled: &AtomicBool,
    ) -> Result<roukaku_byte_source::OwnedByteBuffer, CoreError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(CoreError::invalid(
                "SHINKI_EMU_AUDIO_CANCELLED",
                "audio decode cancelled",
            ));
        }
        let size = self.stat(uri)?.size;
        if size == 0 || size > MAX_ENCODED_AUDIO_BYTES {
            return Err(CoreError::invalid(
                "SHINKI_EMU_CMVS_AUDIO_BOUND",
                "encoded audio exceeds the byte limit",
            ));
        }
        Ok(self.read_range(uri, 0, size)?.bytes)
    }
}

/// MOG_InputMemory→ov_pcm_total(-1): sum final granules per logical stream.
/// Native175 publishes the low DWORD, without a channel/sample-width factor.
fn ogg_pcm_extent(source: &[u8]) -> Result<u32, CoreError> {
    let invalid = || {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_AUDIO_METADATA",
            "Ogg PCM extent is malformed",
        )
    };
    let mut totals = std::collections::BTreeMap::new();
    let mut completed = 0u64;
    let mut streams = 0usize;
    let mut cursor = 0usize;
    while cursor < source.len() {
        let header = source.get(cursor..cursor + 27).ok_or_else(invalid)?;
        if &header[..4] != b"OggS" || header[4] != 0 {
            return Err(invalid());
        }
        let segments = source
            .get(cursor + 27..cursor + 27 + usize::from(header[26]))
            .ok_or_else(invalid)?;
        let body = segments
            .iter()
            .map(|size| usize::from(*size))
            .sum::<usize>();
        let end = cursor + 27 + segments.len() + body;
        if end > source.len() {
            return Err(invalid());
        }
        let serial = u32::from_le_bytes(header[14..18].try_into().unwrap());
        let granule = u64::from_le_bytes(header[6..14].try_into().unwrap());
        if granule != u64::MAX {
            totals.insert(serial, granule);
        }
        if header[5] & 4 != 0
            && let Some(total) = totals.remove(&serial)
        {
            completed = completed.wrapping_add(total);
            streams += 1;
        }
        cursor = end;
    }
    if totals.is_empty() && streams == 0 {
        return Err(invalid());
    }
    Ok(totals
        .values()
        .fold(completed, |total, value| total.wrapping_add(*value)) as u32)
}

/// Native PCM/ACM/MV2 all expose signed little-endian16. Do not normalize a
/// WAV according to its claimed tag/bit depth before the actual consumer.
fn append_pcm_s16(
    source: &[u8],
    channels: u16,
    frames: &mut Vec<kira::Frame>,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<(), CoreError> {
    let invalid = || {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_PCM_BOUNDARY",
            "native signed16 output does not contain initialized whole mono/stereo frames",
        )
    };
    if !matches!(channels, 1 | 2) || !source.len().is_multiple_of(2 * usize::from(channels)) {
        return Err(invalid());
    }
    let stride = 2 * usize::from(channels);
    if frames
        .len()
        .checked_add(source.len() / stride)
        .is_none_or(|len| len > max_frames)
    {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_AUDIO_BOUND",
            "native decoded audio exceeds the frame limit",
        ));
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(CoreError::invalid(
            "SHINKI_EMU_AUDIO_CANCELLED",
            "audio decode cancelled",
        ));
    }
    frames.reserve(source.len() / stride);
    for sample in source.chunks_exact(stride) {
        let left = f32::from(i16::from_le_bytes(sample[..2].try_into().unwrap())) * (1.0 / 32768.0);
        let right = f32::from(i16::from_le_bytes(sample[stride - 2..].try_into().unwrap()))
            * (1.0 / 32768.0);
        frames.push(kira::Frame::new(left, right));
    }
    Ok(())
}

fn pcm_s16(
    source: &[u8],
    channels: u16,
    rate: u32,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    if rate == 0 || source.is_empty() {
        return Err(CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_PCM_BOUNDARY",
            "native decoder produced no playable PCM",
        ));
    }
    let mut frames = Vec::new();
    append_pcm_s16(source, channels, &mut frames, max_frames, cancelled)?;
    Ok(StaticSoundData {
        sample_rate: rate,
        frames: frames.into(),
        settings: Default::default(),
        slice: None,
    })
}

fn decode_source(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    let audio = if source.starts_with(MGV1_MAGIC) {
        mgv1_embedded_ogg(source)?
    } else {
        source
    };
    decode_audio(audio, max_frames, cancelled)
}

/// Decodes one raw encoded stream for session-side media (movie audio).
pub(crate) fn decode_audio_public(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<StaticSoundData, CoreError> {
    decode_audio(source, max_frames, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_mgv_is_not_retried_as_another_audio_format() {
        assert_eq!(
            decode_source(b"MGV1invalid", 32, &AtomicBool::new(false))
                .err()
                .unwrap()
                .code(),
            "SHINKI_EMU_CMVS_MGV_HEADER"
        );
    }
}
