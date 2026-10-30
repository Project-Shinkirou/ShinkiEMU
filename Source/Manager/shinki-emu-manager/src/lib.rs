pub mod effects;
mod gamepad;
mod host;
pub mod modal_service;
pub mod platform;
pub mod resale_notice;
pub mod startup;
pub mod window_service;

extern crate self as shinki_emu_manager;

pub use host::{
    CloseDecision, ConfigPathPick, FilterApply, HostError, HostExit, HostWake, KeyCaptureState,
    ManagerController, ShinkiUnderlayRenderer, SnackbarOutcome, StageTextureUpdate,
    WgpuFrameContext, close_requested, run_manager, run_manager_with_initial_state,
};

// Mobile native hosts load the Manager as a cdylib; platform entrypoints are
// registered by their respective activity/NAPI modules.
#[cfg(any(target_os = "android", target_env = "ohos"))]
include!("main.rs");
