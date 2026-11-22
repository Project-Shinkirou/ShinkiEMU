use crate::{Error, Header, Status};
use std::ffi::c_void;

pub const ANDROID_CONTEXT_EXTENSION: &[u8] = b"shinki.emu.android_context";
pub const ANDROID_CONTEXT_VERSION: u32 = 1;

/// Optional Android `CoreApi.extension` table for a dynamically loaded core.
/// Negotiate `shinki.emu.android_context`, version 1; the header uses ABI v3.
/// On explicit activation, before `CoreApi.initialize`, call `initialize` with
/// a `JavaVM*` and a JNI global reference to the application context. The host
/// owns both for the life of the process. Never pass an Activity or `JNIEnv*`.
/// Repeating the call with the same pointers must succeed without reinitializing.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AndroidContextV1 {
    pub header: Header,
    pub initialize: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut Error) -> Status>,
}
