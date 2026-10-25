#![allow(non_local_definitions)]

use std::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use super::{
    descriptor::{SessionError, SessionResult},
    validate_dimensions,
};

static NEXT_SESSION_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Allocate an epoch for in-process runtime callers and synthetic inputs.
pub fn allocate_session_epoch() -> SessionResult<u64> {
    NEXT_SESSION_EPOCH
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| invalid("SHINKI_EMU_FAMILY_SESSION_EPOCH", "session epoch exhausted"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameAlpha {
    Opaque,
    /// Native RGB is already composited; ignore render-target alpha bytes.
    Ignored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameFormat {
    Rgba8Srgb { alpha: FrameAlpha },
    Bgra8Srgb { alpha: FrameAlpha },
    Rgba8Linear { alpha: FrameAlpha },
    Bgra8Linear { alpha: FrameAlpha },
}

impl FrameFormat {
    pub fn alpha(self) -> FrameAlpha {
        match self {
            Self::Rgba8Srgb { alpha }
            | Self::Bgra8Srgb { alpha }
            | Self::Rgba8Linear { alpha }
            | Self::Bgra8Linear { alpha } => alpha,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRowOrder {
    TopToBottom,
    BottomToTop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub logical_width: u32,
    pub logical_height: u32,
    pub stride: u32,
    pub format: FrameFormat,
    pub rows: FrameRowOrder,
}

impl FrameInfo {
    pub fn validate(&self) -> SessionResult<()> {
        validate_dimensions(self.width, self.height)?;
        validate_dimensions(self.logical_width, self.logical_height)?;
        let row = self
            .width
            .checked_mul(4)
            .ok_or_else(|| invalid("SHINKI_EMU_FAMILY_FRAME_SIZE", "frame row size overflows"))?;
        if self.stride < row {
            return Err(invalid(
                "SHINKI_EMU_FAMILY_FRAME_STRIDE",
                "frame stride is smaller than the RGBA row",
            ));
        }
        self.required_bytes()
            .map(|_| ())
            .ok_or_else(|| invalid("SHINKI_EMU_FAMILY_FRAME_SIZE", "frame size overflows"))
    }

    pub fn required_bytes(&self) -> Option<usize> {
        u64::from(self.stride)
            .checked_mul(u64::from(self.height))
            .and_then(|bytes| usize::try_from(bytes).ok())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameIdentity {
    pub session_epoch: u64,
    pub generation: u64,
}

impl FrameIdentity {
    pub fn validate(&self, completed: bool) -> SessionResult<()> {
        if self.session_epoch == 0 || (completed && self.generation == 0) {
            return Err(invalid(
                "SHINKI_EMU_FAMILY_FRAME_GENERATION",
                "frame epoch or completed generation is zero",
            ));
        }
        Ok(())
    }
}

trait FramePixels: Send + Sync {
    fn pixels(&self) -> &[u8];
}
struct PixelOwner<T> {
    value: T,
    bytes: fn(&T) -> &[u8],
}
impl<T: Send + Sync> FramePixels for PixelOwner<T> {
    fn pixels(&self) -> &[u8] {
        (self.bytes)(&self.value)
    }
}
#[derive(Clone)]
pub struct FrameLease {
    pub info: FrameInfo,
    pub identity: FrameIdentity,
    pixels: std::sync::Arc<dyn FramePixels>,
}

impl fmt::Debug for FrameLease {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FrameLease")
            .field("info", &self.info)
            .field("identity", &self.identity)
            .field("pixel_bytes", &self.pixels().len())
            .finish()
    }
}

impl FrameLease {
    /// Moves the existing CPU owner without reallocating its pixel payload.
    pub fn from_owner<T: Send + Sync + 'static>(
        value: T,
        bytes: fn(&T) -> &[u8],
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> SessionResult<Self> {
        info.validate()?;
        identity.validate(true)?;
        validate_pixels(bytes(&value), info)?;
        Ok(Self {
            info,
            identity,
            pixels: std::sync::Arc::new(PixelOwner { value, bytes }),
        })
    }

    pub fn from_vec(
        value: Vec<u8>,
        info: FrameInfo,
        identity: FrameIdentity,
    ) -> SessionResult<Self> {
        Self::from_owner(value, Vec::as_slice, info, identity)
    }

    pub fn pixels(&self) -> &[u8] {
        self.pixels.pixels()
    }

    /// Validation does not re-scan certified immutable alpha on every pull.
    pub fn validate(&self) -> SessionResult<()> {
        self.info.validate()?;
        self.identity.validate(true)?;
        validate_length(self.pixels(), self.info)
    }
}

#[derive(Debug, Clone)]
pub enum FramePoll {
    Ready(FrameLease),
    Unchanged(FrameIdentity),
    Pending(FrameIdentity),
}

impl FramePoll {
    pub fn identity(&self) -> FrameIdentity {
        match self {
            Self::Ready(frame) => frame.identity,
            Self::Unchanged(identity) | Self::Pending(identity) => *identity,
        }
    }

    pub fn validate(&self, session_epoch: u64) -> SessionResult<()> {
        if self.identity().session_epoch != session_epoch {
            return Err(invalid(
                "SHINKI_EMU_FAMILY_FRAME_EPOCH",
                "frame belongs to a different session",
            ));
        }
        match self {
            Self::Ready(frame) => frame.validate(),
            Self::Unchanged(identity) => identity.validate(true),
            Self::Pending(identity) => identity.validate(false),
        }
    }
}

/// Owner-local publication state. Logical clocks never reset this generation.
#[derive(Debug)]
pub struct FramePublisher {
    identity: FrameIdentity,
    current: Option<FrameLease>,
}

impl FramePublisher {
    pub fn new(session_epoch: u64) -> SessionResult<Self> {
        let identity = FrameIdentity {
            session_epoch,
            generation: 0,
        };
        identity.validate(false)?;
        Ok(Self {
            identity,
            current: None,
        })
    }

    pub fn publish<T: Send + Sync + 'static>(
        &mut self,
        value: T,
        bytes: fn(&T) -> &[u8],
        info: FrameInfo,
    ) -> SessionResult<()> {
        let identity = FrameIdentity {
            session_epoch: self.identity.session_epoch,
            generation: self.identity.generation.checked_add(1).ok_or_else(|| {
                invalid(
                    "SHINKI_EMU_FAMILY_FRAME_GENERATION",
                    "frame generation exhausted",
                )
            })?,
        };
        let frame = FrameLease::from_owner(value, bytes, info, identity)?;
        self.current = Some(frame);
        self.identity = identity;
        Ok(())
    }

    pub fn current(&self) -> Option<&FrameLease> {
        self.current.as_ref()
    }

    pub fn poll(&self, after: Option<FrameIdentity>) -> FramePoll {
        match &self.current {
            None => self.pending(),
            Some(_) if after == Some(self.identity) => FramePoll::Unchanged(self.identity),
            Some(frame) => FramePoll::Ready(frame.clone()),
        }
    }

    pub fn pending(&self) -> FramePoll {
        FramePoll::Pending(self.identity)
    }
}

fn validate_length(pixels: &[u8], info: FrameInfo) -> SessionResult<()> {
    let required = info.required_bytes().ok_or_else(|| {
        invalid(
            "SHINKI_EMU_FAMILY_FRAME_SIZE",
            "frame size does not fit host usize",
        )
    })?;
    if pixels.len() < required {
        return Err(invalid(
            "SHINKI_EMU_FAMILY_FRAME_BYTES",
            "frame pixels are shorter than stride times height",
        ));
    }
    Ok(())
}

fn validate_pixels(pixels: &[u8], info: FrameInfo) -> SessionResult<()> {
    validate_length(pixels, info)?;
    if info.format.alpha() == FrameAlpha::Opaque {
        for row in
            pixels[..info.required_bytes().expect("validated size")].chunks(info.stride as usize)
        {
            if row[..info.width as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel[3] != 255)
            {
                return Err(invalid(
                    "SHINKI_EMU_FAMILY_FRAME_ALPHA",
                    "opaque frame contains non-opaque pixels",
                ));
            }
        }
    }
    Ok(())
}

fn invalid(code: &'static str, message: &'static str) -> SessionError {
    SessionError::invalid(code, message)
}

#[cfg(test)]
#[path = "frame_tests.rs"]
mod tests;
