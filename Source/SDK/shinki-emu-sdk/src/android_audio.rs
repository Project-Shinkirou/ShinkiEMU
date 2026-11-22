//! Process-lifetime JNI binding for CPAL in a downloaded Android core.
use crate::native::NativeFailure;
use shinki_emu_abi as abi;
use std::{ffi::c_void, sync::OnceLock};

const CODE: &str = "SHINKI_EMU_ANDROID_AUDIO_CONTEXT";
static BINDING: OnceLock<(usize, usize)> = OnceLock::new();

/// Bind the module's CPAL context once, before audio workers can start.
///
/// # Safety
/// Call only from a dlopened core's explicit activation, not an Activity or
/// static host that already initializes `ndk_context`. `java_vm` and `application`
/// must be the host's process-lifetime JavaVM and Application global reference.
pub unsafe fn bind_context(
    java_vm: *mut c_void,
    application: *mut c_void,
) -> Result<(), NativeFailure> {
    if java_vm.is_null() || application.is_null() {
        return Err(NativeFailure::new(
            abi::INVALID_ARGUMENT,
            CODE,
            "JavaVM and Application global reference are required",
        ));
    }
    let requested = (java_vm.addr(), application.addr());
    if *BINDING.get_or_init(|| {
        unsafe { ndk_context::initialize_android_context(java_vm, application) };
        requested
    }) != requested
    {
        return Err(NativeFailure::new(
            abi::INVALID_STATE,
            CODE,
            "Android audio is already bound to a different context",
        ));
    }
    Ok(())
}

/// Require explicit host activation before starting a native session.
pub fn require_context() -> Result<(), NativeFailure> {
    if BINDING.get().is_none() {
        return Err(NativeFailure::new(
            abi::INVALID_STATE,
            CODE,
            "activate the Android context before opening a session",
        ));
    }
    Ok(())
}
