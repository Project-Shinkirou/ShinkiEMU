use super::{NativeDecodedAudio, append_pcm_s16};
use kira::{
    Frame,
    sound::static_sound::{StaticSoundData, StaticSoundSettings},
};
use shinki_emu_sdk::CoreError;
use std::sync::atomic::AtomicBool;

fn boundary(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_ACM_BOUNDARY", message)
}
fn environment(message: &str) -> CoreError {
    CoreError::invalid("SHINKI_EMU_CMVS_NATIVE_ACM_ENVIRONMENT", message)
}

struct Request<'a> {
    format: [u8; 30],
    input: u32,
    payload: &'a [u8],
}

/// 48F850/48FC10/48FE70: one raw MPEG-1/2 Layer III header, no scanning,
/// RIFF unwrapping, footer search, Xing duration, or LAME delay adjustment.
fn request(source: &[u8]) -> Result<Request<'_>, CoreError> {
    let invalid = || {
        CoreError::invalid(
            "SHINKI_EMU_CMVS_NATIVE_MP3_HEADER",
            "native MP3 header fields are not fully defined",
        )
    };
    let mut end = source.len();
    if end >= 128 && source.get(end - 128..end - 125) == Some(b"TAG") {
        end -= 128;
    }
    let mut start = 0usize;
    if source.starts_with(b"ID3") {
        let tag = source.get(..10).ok_or_else(invalid)?;
        start = 10
            + ((usize::from(tag[6]) & 127) << 21)
            + ((usize::from(tag[7]) & 127) << 14)
            + ((usize::from(tag[8]) & 127) << 7)
            + (usize::from(tag[9]) & 127);
    }
    let header = u32::from_be_bytes(
        source
            .get(start..start + 4)
            .ok_or_else(invalid)?
            .try_into()
            .unwrap(),
    );
    if header >> 20 != 0xfff || (header >> 17) & 3 != 1 {
        return Err(invalid());
    }
    let index = ((header >> 12) & 15) as usize;
    let rate_index = ((header >> 10) & 3) as usize;
    // Indices0/15 and reserved rate3 read adjacent/uninitialized native
    // table state; they are not evidence of defined free-format support.
    if index == 0 || index == 15 || rate_index == 3 {
        return Err(invalid());
    }
    let mpeg1 = header & (1 << 19) != 0;
    let rates = if mpeg1 {
        [44100u32, 48000, 32000]
    } else {
        [22050u32, 24000, 16000]
    };
    let bitrates = if mpeg1 {
        [
            0u32, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ]
    } else {
        [
            0u32, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160,
        ]
    };
    let rate = rates[rate_index];
    let kbps = bitrates[index];
    let channels = if (header >> 6) & 3 == 3 { 1u16 } else { 2u16 };
    let mut format = [0u8; 30];
    format[..2].copy_from_slice(&0x55u16.to_le_bytes());
    format[2..4].copy_from_slice(&channels.to_le_bytes());
    format[4..8].copy_from_slice(&rate.to_le_bytes());
    format[8..12].copy_from_slice(&(kbps * 125).to_le_bytes());
    format[12..14].copy_from_slice(&1u16.to_le_bytes());
    format[16..18].copy_from_slice(&12u16.to_le_bytes());
    format[18..20].copy_from_slice(&1u16.to_le_bytes());
    format[20..24].copy_from_slice(&2u32.to_le_bytes());
    format[24..26].copy_from_slice(&((144000 * kbps / rate) as u16).to_le_bytes());
    format[26..28].copy_from_slice(&1u16.to_le_bytes());
    format[28..30].copy_from_slice(&1393u16.to_le_bytes());
    Ok(Request {
        format,
        input: (end as u32).wrapping_sub(start as u32),
        payload: source.get(start..end).unwrap_or(&[]),
    })
}

pub(super) fn decode(
    source: &[u8],
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    let request = request(source)?;
    #[cfg(windows)]
    {
        match run(&request, max_frames, cancelled) {
            Ok(decoded) => Ok(decoded),
            Err(cause) => Ok(NativeDecodedAudio {
                channels: u16::from_le_bytes(request.format[2..4].try_into().unwrap()),
                extent: None,
                data: Err(cause),
                repeat_data: None,
                restart_rewinds: false,
            }),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (max_frames, cancelled);
        Ok(NativeDecodedAudio {
            channels: u16::from_le_bytes(request.format[2..4].try_into().unwrap()),
            extent: None,
            data: Err(environment(
                "native MP3 requires the original x86 Windows ACM driver environment; no portable size/PCM estimate is substituted",
            )),
            repeat_data: None,
            restart_rewinds: false,
        })
    }
}

#[cfg(windows)]
struct Output<'a> {
    header: Option<[u8; 88]>,
    frames: Vec<Frame>,
    max_frames: usize,
    cancelled: &'a AtomicBool,
    error: Option<CoreError>,
}
#[cfg(windows)]
impl Output<'_> {
    fn push(&mut self, bytes: &[u8]) -> bool {
        if self.header.is_none() {
            if bytes.len() != 88 || &bytes[..4] != b"CAC1" {
                self.error = Some(boundary("x86 ACM helper returned an invalid query header"));
                return false;
            }
            self.header = Some(bytes.try_into().unwrap());
            return true;
        }
        if self.error.is_some() {
            return false;
        }
        let channels =
            u16::from_le_bytes(self.header.as_ref().unwrap()[10..12].try_into().unwrap());
        if let Err(cause) = append_pcm_s16(
            bytes,
            channels,
            &mut self.frames,
            self.max_frames,
            self.cancelled,
        ) {
            self.error = Some(cause);
            return false;
        }
        true
    }
    fn finish(self, status: i32) -> Result<NativeDecodedAudio, CoreError> {
        let header = self
            .header
            .ok_or_else(|| environment("x86 ACM helper did not reach a size query"))?;
        let extent = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let channels = u16::from_le_bytes(header[10..12].try_into().unwrap());
        let rate = u32::from_le_bytes(header[12..16].try_into().unwrap());
        let field =
            |offset: usize| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
        let name_end = header[56..88]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(32);
        tracing::debug!(event = "cmvs.audio.native_acm_query", architecture = "x86", extent,
            suggest_status = field(28), open_status = field(32), size_status = field(36),
            driver_status = field(40), driver_version = field(44), manufacturer = field(48), product = field(52),
            driver = %String::from_utf8_lossy(&header[56..56 + name_end]));
        let data = if let Some(cause) = self.error {
            Err(cause)
        } else if status != 0 || rate == 0 || self.frames.is_empty() {
            Err(boundary(&format!(
                "native x86 ACM PCM consumer stopped at boundary {status}"
            )))
        } else {
            Ok(StaticSoundData {
                sample_rate: rate,
                frames: self.frames.into(),
                settings: StaticSoundSettings::default(),
                slice: None,
            })
        };
        Ok(NativeDecodedAudio {
            data,
            channels,
            extent: Some(extent),
            repeat_data: None,
            restart_rewinds: false,
        })
    }
}

#[cfg(all(windows, target_arch = "x86"))]
fn run(
    request: &Request<'_>,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    unsafe extern "C" {
        fn cmvs_acm_decode(
            format: *const u8,
            source: *const u8,
            length: u32,
            available: u32,
            max_pcm: u32,
            sink: unsafe extern "C" fn(*mut std::ffi::c_void, *const u8, u32) -> i32,
            user: *mut std::ffi::c_void,
        ) -> i32;
    }
    unsafe extern "C" fn sink(user: *mut std::ffi::c_void, bytes: *const u8, len: u32) -> i32 {
        let output = unsafe { &mut *(user as *mut Output<'_>) };
        i32::from(output.push(unsafe { std::slice::from_raw_parts(bytes, len as usize) }))
    }
    let mut output = Output {
        header: None,
        frames: Vec::new(),
        max_frames,
        cancelled,
        error: None,
    };
    let max_pcm = max_frames.saturating_mul(4).min(256 * 1024 * 1024) as u32;
    let status = unsafe {
        cmvs_acm_decode(
            request.format.as_ptr(),
            request.payload.as_ptr(),
            request.input,
            request.payload.len() as u32,
            max_pcm,
            sink,
            (&mut output as *mut Output<'_>).cast(),
        )
    };
    output.finish(status)
}

#[cfg(all(windows, not(target_arch = "x86")))]
fn run(
    request: &Request<'_>,
    max_frames: usize,
    cancelled: &AtomicBool,
) -> Result<NativeDecodedAudio, CoreError> {
    use std::{
        io::{Read, Write},
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
    };
    struct OwnedChild(Child);
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut directory = std::env::current_exe()
        .map_err(|_| environment("cannot locate the x86 ACM helper beside the host executable"))?;
    directory.pop();
    // Cargo test executables live in profile/deps, while the staged helper
    // and installed Manager executable live directly in profile.
    if directory.file_name().is_some_and(|name| name == "deps") {
        directory.pop();
    }
    let mut child = OwnedChild(Command::new(directory.join("cmvs-acm32.exe")).creation_flags(0x08000000)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .map_err(|_| environment("native MP3 requires packaged cmvs-acm32.exe and installed x86 Windows ACM codecs"))?);
    let max_pcm = max_frames.saturating_mul(4).min(256 * 1024 * 1024) as u32;
    {
        let mut stdin = child.0.stdin.take().unwrap();
        stdin
            .write_all(&request.format)
            .and_then(|_| stdin.write_all(&request.input.to_le_bytes()))
            .and_then(|_| stdin.write_all(&(request.payload.len() as u32).to_le_bytes()))
            .and_then(|_| stdin.write_all(&max_pcm.to_le_bytes()))
            .and_then(|_| stdin.write_all(request.payload))
            .map_err(|_| {
                environment("x86 ACM helper could not receive the bounded encoded input")
            })?;
    }
    let mut output = Output {
        header: None,
        frames: Vec::new(),
        max_frames,
        cancelled,
        error: None,
    };
    let mut stdout = child.0.stdout.take().unwrap();
    let mut header = [0u8; 88];
    stdout
        .read_exact(&mut header)
        .map_err(|_| environment("x86 ACM helper exited before returning its size query"))?;
    if !output.push(&header) {
        return output.finish(-1);
    }
    let mut buffer = [0u8; 0x8000];
    let mut count = [0u8; 4];
    loop {
        if stdout.read_exact(&mut count).is_err() {
            output.error = Some(boundary(
                "x86 ACM helper stopped after its defined size query",
            ));
            return output.finish(-1);
        }
        let size = u32::from_le_bytes(count) as usize;
        if size == 0 {
            break;
        }
        if size > buffer.len() || stdout.read_exact(&mut buffer[..size]).is_err() {
            output.error = Some(boundary("x86 ACM helper returned an invalid PCM chunk"));
            return output.finish(-1);
        }
        if !output.push(&buffer[..size]) {
            return output.finish(-1);
        }
    }
    if stdout.read_exact(&mut count).is_err() {
        output.error = Some(boundary("x86 ACM helper omitted its PCM result"));
        return output.finish(-1);
    }
    let status = u32::from_le_bytes(count) as i32;
    if !child.0.wait().is_ok_and(|status| status.success()) {
        output.error = Some(boundary(
            "x86 ACM helper failed after publishing its size query",
        ));
    }
    output.finish(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp3_format_is_the_exact_native_thirty_byte_wire() {
        let source = [0xff, 0xfb, 0x90, 0xc0, 1, 2, 3, 4];
        let request = request(&source).unwrap();
        let expected = [
            0x55, 0, 1, 0, 0x44, 0xac, 0, 0, 0x80, 0x3e, 0, 0, 1, 0, 0, 0, 12, 0, 1, 0, 2, 0, 0, 0,
            0xa1, 1, 1, 0, 0x71, 5,
        ];
        assert_eq!(request.format, expected);
        assert_eq!(request.input, 8);
        assert_eq!(request.payload, source);
    }

    #[test]
    fn tag_trim_masks_synchsafe_bytes_and_does_not_scan_or_add_a_footer() {
        let mut source = vec![0; 14];
        source[..3].copy_from_slice(b"ID3");
        source[5] = 0x10; // Native still does not add a ten-byte footer.
        source[9] = 0x84;
        source.extend_from_slice(&[0xff, 0xfb, 0x90, 0xc0, 5, 6, 7, 8]);
        source.extend_from_slice(b"TAG");
        source.extend_from_slice(&[0; 125]);
        let decoded = request(&source).unwrap();
        assert_eq!(decoded.input, 8);
        assert_eq!(decoded.payload, &source[14..22]);
        assert!(request(b"RIFF\xff\xfb\x90\xc0WAVE").is_err());
        assert!(request(b"junk\xff\xfb\x90\xc0").is_err());
        assert!(request(b"ID3").is_err());
        assert!(request(&[0xff, 0xe3, 0x90, 0]).is_err()); // MPEG2.5 does not pass twelve ones.
    }

    #[test]
    fn encoded_extent_subtraction_wraps_without_exposing_inaccessible_pcm() {
        let mut source = vec![0; 160];
        source[..3].copy_from_slice(b"ID3");
        source[9] = 54; // Header starts64, while TAG trimming sets end32.
        source[32..35].copy_from_slice(b"TAG");
        source[64..68].copy_from_slice(&[0xff, 0xfb, 0x90, 0xc0]);
        let decoded = request(&source).unwrap();
        assert_eq!(decoded.input, 0xffff_ffe0);
        assert!(decoded.payload.is_empty());
    }

    #[test]
    fn mpeg_two_uses_native_bitrate_rate_and_unhalved_blocksize_formula() {
        let decoded = request(&[0xff, 0xf3, 0x80, 0]).unwrap();
        assert_eq!(
            u32::from_le_bytes(decoded.format[4..8].try_into().unwrap()),
            22050
        );
        assert_eq!(
            u32::from_le_bytes(decoded.format[8..12].try_into().unwrap()),
            8000
        );
        assert_eq!(
            u16::from_le_bytes(decoded.format[24..26].try_into().unwrap()),
            417
        );
    }
}

#[cfg(all(test, windows, cmvs_acm32_available))]
mod windows_tests {
    use super::*;

    #[test]
    fn actual_x86_acm_size_error_keeps_zero_defined_when_pcm_is_unavailable() {
        let source = [0xff, 0xfb, 0x90, 0xc0];
        let mut query = request(&source).unwrap();
        // Exercise the documented API error independently of MP3 trim gates.
        query.input = 0;
        let decoded = run(&query, 8, &AtomicBool::new(false)).unwrap();
        assert_eq!(decoded.extent, Some(0));
        assert!(decoded.data.is_err());
    }
}
