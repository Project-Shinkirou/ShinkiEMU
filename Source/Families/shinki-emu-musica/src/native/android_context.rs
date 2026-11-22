use super::{Failure, abi, boundary};
use jni::{
    JavaVM, jni_sig, jni_str,
    objects::{JObject, JString},
};
use shinki_emu_sdk::android_audio;
use std::{
    ffi::c_void,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) static API: abi::AndroidContextV1 = abi::AndroidContextV1 {
    header: abi::Header::of::<abi::AndroidContextV1>(),
    initialize: Some(initialize),
};
static CACHE_READY: AtomicBool = AtomicBool::new(false);
const CACHE_CODE: &str = "SHINKI_EMU_MUSICA_ANDROID_CACHE";

pub(super) fn require_initialized() -> Result<(), Failure> {
    android_audio::require_context()?;
    if !CACHE_READY.load(Ordering::Acquire) {
        return Err(Failure::new(
            abi::INVALID_STATE,
            CACHE_CODE,
            "activate the Android movie cache before opening a session",
        ));
    }
    Ok(())
}

unsafe extern "C" fn initialize(
    java_vm: *mut c_void,
    application: *mut c_void,
    error: *mut abi::Error,
) -> abi::Status {
    boundary(error, || {
        unsafe { android_audio::bind_context(java_vm, application) }?;
        if CACHE_READY.load(Ordering::Acquire) {
            return Ok(());
        }
        // A downloaded core has its own tempfile state, separate from Manager.
        let vm = unsafe { JavaVM::from_raw(java_vm.cast()) };
        let cache = vm
            .attach_current_thread_for_scope(|env| -> jni::errors::Result<_> {
                let application = unsafe { JObject::from_raw(env, application.cast()) };
                let directory = env
                    .call_method(
                        &application,
                        jni_str!("getCacheDir"),
                        jni_sig!("()Ljava/io/File;"),
                        &[],
                    )?
                    .l()?;
                let path = env
                    .call_method(
                        &directory,
                        jni_str!("getAbsolutePath"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                env.cast_local::<JString>(path)?.try_to_string(env)
            })
            .map(PathBuf::from)
            .map_err(|error| Failure::new(abi::CORE_ERROR, CACHE_CODE, error.to_string()))?;
        std::fs::create_dir_all(&cache)
            .map_err(|error| Failure::new(abi::CORE_ERROR, CACHE_CODE, error.to_string()))?;
        if let Err(previous) = tempfile::env::override_temp_dir(&cache)
            && previous != cache
        {
            return Err(Failure::new(
                abi::INVALID_STATE,
                CACHE_CODE,
                "Android movie cache is already set to a different directory",
            ));
        }
        CACHE_READY.store(true, Ordering::Release);
        Ok(())
    })
}
