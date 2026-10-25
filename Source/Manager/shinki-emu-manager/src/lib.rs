pub mod effects;
mod gamepad;
mod host;
pub mod modal_service;
pub mod platform;
pub mod startup;
pub mod window_service;

extern crate self as shinki_emu_manager;

pub use host::{
    CloseDecision, ConfigPathPick, FilterApply, HostError, HostExit, HostWake, KeyCaptureState,
    ManagerController, ShinkiUnderlayRenderer, SnackbarOutcome, StageTextureUpdate,
    WgpuFrameContext, close_requested, run_manager, run_manager_with_initial_state,
};

// Android loads the Manager as a cdylib through NativeActivity; the
// application modules and `android_main` come from the shared binary root.
#[cfg(target_os = "android")]
include!("main.rs");
