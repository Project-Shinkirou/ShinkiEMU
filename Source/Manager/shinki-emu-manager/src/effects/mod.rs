//! GPU filter product module owned by ShinkiEMU Manager.
//!
//! The host owns the device, queue, ordered targets and presentation. Backends
//! record into the host encoder and cache only their own scratch resources.
//! The chain is replaced only after a candidate has been prepared successfully.

mod anime4k_original;
mod chain;
mod classic;
mod compiler;
mod engine;
pub mod format4;
mod format4_gpu;
mod generator;
mod layout;

pub use engine::{FilterEngine, FilterError, FilterTargets, PreparedFilter, gpu_checked};
