//! C ABI validation and cancellation shared by native core owners.
use crate::runtime;
use shinki_emu_abi as abi;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
pub struct ServiceContext {
    pub session: u64,
    pub epoch: AtomicU64,
    pub cancelled: AtomicBool,
    pub waiting: AtomicBool,
    pub text: Option<abi::TextService>,
    pub window: Option<abi::WindowService>,
    pub host_cancel: Option<abi::CancellationService>,
}
// Host service contexts explicitly support the native worker and independent
// concurrent invalidation. Their storage remains alive until close joins it.
unsafe impl Send for ServiceContext {}
unsafe impl Sync for ServiceContext {}
impl ServiceContext {
    pub fn is_cancelled(&self) -> bool {
        if let Some(service) = self.host_cancel
            && service
                .is_cancelled
                .is_some_and(|check| unsafe { check(service.context, self.session) } != 0)
            && !self.cancelled.swap(true, Ordering::AcqRel)
        {
            self.invalidate();
        }
        self.cancelled.load(Ordering::Acquire)
    }
    pub fn invalidate(&self) {
        let next = match self
            .epoch
            .try_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            }) {
            Ok(previous) => previous + 1,
            Err(epoch) => {
                self.cancelled.store(true, Ordering::Release);
                epoch
            }
        };
        if let Some(service) = self.text
            && let Some(invalidate) = service.invalidate
        {
            unsafe { invalidate(service.context, self.session, next) };
        }
        if let Some(service) = self.window
            && let Some(invalidate) = service.invalidate
        {
            unsafe { invalidate(service.context, self.session, next) };
        }
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.invalidate();
    }
}
/// Validate and own explicit C configuration entries before handing them to a worker.
/// # Safety
/// Descriptor fields and request arrays must be readable for their declared lengths.
pub unsafe fn config_entries(
    descriptor: abi::Descriptor,
    request: &abi::OpenRequest,
) -> Result<Vec<runtime::ConfigEntry>, abi::Status> {
    let fields = unsafe {
        abi::borrowed_array(
            descriptor.configuration,
            descriptor.configuration_count,
            abi::MAX_CONFIG_FIELDS,
        )
    }?;
    let entries = unsafe {
        abi::borrowed_array(
            request.configuration,
            request.configuration_count,
            abi::MAX_CONFIG_FIELDS,
        )
    }?;
    unsafe { abi::validate_config(fields, entries) }.map_err(|_| abi::INVALID_ARGUMENT)?;
    entries
        .iter()
        .map(|entry| {
            let id = unsafe { entry.id.text(128) }?;
            let value = match entry.value.kind {
                abi::VALUE_BOOL => runtime::ConfigValue::Bool(entry.value.integer != 0),
                abi::VALUE_INTEGER => runtime::ConfigValue::Integer(entry.value.integer),
                abi::VALUE_NUMBER => runtime::ConfigValue::Number(entry.value.number),
                abi::VALUE_STRING => runtime::ConfigValue::String(
                    unsafe { entry.value.text.text(abi::MAX_CONFIG_TEXT) }?.into(),
                ),
                abi::VALUE_ENUM => {
                    runtime::ConfigValue::Enum(unsafe { entry.value.text.text(128) }?.into())
                }
                _ => return Err(abi::INVALID_ARGUMENT),
            };
            Ok(runtime::ConfigEntry {
                id: id.into(),
                value,
            })
        })
        .collect()
}

struct CpuFrameOwner<T> {
    pixels: T,
    count: Arc<AtomicUsize>,
}
unsafe extern "C" fn release_cpu<T>(owner: *mut std::ffi::c_void) {
    if !owner.is_null() {
        let owner = unsafe { Box::from_raw(owner.cast::<CpuFrameOwner<T>>()) };
        owner.count.fetch_sub(1, Ordering::AcqRel);
    }
}
/// Leases own only immutable CPU pixels and their counter; returning one never touches a session.
pub fn lease_cpu<T: Clone + Send + Sync + 'static>(
    pixels: &T,
    count: &Arc<AtomicUsize>,
    bytes: fn(&T) -> &[u8],
) -> Option<abi::CpuLease> {
    count
        .try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < 3).then_some(n + 1)
        })
        .ok()?;
    let owner = Box::new(CpuFrameOwner {
        pixels: pixels.clone(),
        count: count.clone(),
    });
    Some(abi::CpuLease {
        pixels: abi::Bytes::new(bytes(&owner.pixels)),
        owner: Box::into_raw(owner).cast(),
        release: Some(release_cpu::<T>),
    })
}

use crate::runtime::{SessionError, WindowCommand};
use std::borrow::Cow;
#[derive(Clone, Debug)]
pub struct NativeFailure {
    pub status: abi::Status,
    code: Cow<'static, str>,
    message: Cow<'static, str>,
}
impl NativeFailure {
    pub fn new(
        status: abi::Status,
        code: impl Into<Cow<'static, str>>,
        message: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn diagnostic(&self) -> abi::Error {
        let mut diagnostic = abi::Error::default();
        diagnostic.set(&self.code, &self.message);
        diagnostic
    }
}
impl From<SessionError> for NativeFailure {
    fn from(cause: SessionError) -> Self {
        let status = if cause.code.ends_with("_CANCELLED") {
            abi::CANCELLED
        } else {
            abi::CORE_ERROR
        };
        Self::new(status, cause.code, cause.message)
    }
}

pub fn window_request(
    services: &ServiceContext,
    id: u64,
    command: &WindowCommand,
) -> Result<(), NativeFailure> {
    let service = services.window.ok_or_else(|| {
        NativeFailure::new(
            abi::UNSUPPORTED,
            "SHINKI_EMU_WINDOW_SERVICE",
            "window operation requires a native host service",
        )
    })?;
    let epoch = services.epoch.load(Ordering::Acquire);
    let mut request = abi::WindowRequest {
        session_epoch: services.session,
        context_epoch: epoch,
        request_id: id,
        kind: 0,
        flags: 0,
        text: abi::Bytes::EMPTY,
        pixels: abi::Bytes::EMPTY,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        autohide_ms: 0,
        buttons: std::ptr::null(),
        button_count: 0,
        caption: abi::Bytes::EMPTY,
        initial_text: abi::Bytes::EMPTY,
    };
    match command {
        WindowCommand::SetFullscreen(fullscreen) => {
            request.kind = abi::WINDOW_FULLSCREEN;
            request.flags = if *fullscreen {
                abi::FULLSCREEN_BORDERLESS
            } else {
                abi::FULLSCREEN_WINDOWED
            };
        }
        WindowCommand::SetTitle(title) => {
            request.kind = abi::WINDOW_TITLE;
            request.text = abi::Bytes::new(title.as_bytes());
        }
        WindowCommand::SetIcon(icon) => {
            request.kind = abi::WINDOW_ICON;
            request.width = icon.width;
            request.height = icon.height;
            request.pixels = abi::Bytes::new(icon.rgba.as_slice());
        }
        WindowCommand::SetPointer(pointer) => {
            request.kind = abi::WINDOW_POINTER;
            if let Some(x) = pointer.logical_x {
                request.flags |= abi::POINTER_SET_X;
                request.x = x;
            }
            if let Some(y) = pointer.logical_y {
                request.flags |= abi::POINTER_SET_Y;
                request.y = y;
            }
            if let Some(visible) = pointer.visible {
                request.flags |= abi::POINTER_SET_VISIBILITY;
                if visible {
                    request.flags |= abi::POINTER_VISIBLE;
                }
            }
            if let Some(ms) = pointer.autohide_ms {
                request.flags |= abi::POINTER_SET_AUTOHIDE;
                request.autohide_ms = ms;
            }
        }
    }
    let mut result = abi::WindowResult {
        selected_button: 0,
        reserved: 0,
        valid_fields: 0,
        fullscreen: 0,
        width: 0,
        height: 0,
        x: 0,
        y: 0,
        input: abi::TextBuffer {
            data: std::ptr::null_mut(),
            capacity: 0,
            len: 0,
        },
        visible: 0,
        minimized: 0,
    };
    let mut diagnostic = abi::Error::default();
    services.waiting.store(true, Ordering::Release);
    let status = unsafe {
        service.request.unwrap()(service.context, &request, &mut result, &mut diagnostic)
    };
    services.waiting.store(false, Ordering::Release);
    if services.epoch.load(Ordering::Acquire) != epoch || services.is_cancelled() {
        return Err(NativeFailure::new(
            abi::CANCELLED,
            "SHINKI_EMU_CANCELLED",
            "window request was cancelled",
        ));
    }
    if status != abi::OK {
        let code = diagnostic
            .code
            .get(..diagnostic.code_len as usize)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| {
                NativeFailure::new(
                    abi::INVALID_ARGUMENT,
                    "SHINKI_EMU_WINDOW_RESULT",
                    "invalid window diagnostic",
                )
            })?;
        let message = diagnostic
            .message
            .get(..diagnostic.message_len as usize)
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .ok_or_else(|| {
                NativeFailure::new(
                    abi::INVALID_ARGUMENT,
                    "SHINKI_EMU_WINDOW_RESULT",
                    "invalid window diagnostic",
                )
            })?;
        return Err(NativeFailure::new(
            status,
            code.to_owned(),
            message.to_owned(),
        ));
    }
    if request.kind == abi::WINDOW_FULLSCREEN
        && (result.valid_fields & abi::WINDOW_RESULT_MODE == 0
            || result.fullscreen != request.flags)
    {
        return Err(NativeFailure::new(
            abi::CORE_ERROR,
            "SHINKI_EMU_WINDOW_RESULT",
            "host did not apply the requested window mode",
        ));
    }
    Ok(())
}
