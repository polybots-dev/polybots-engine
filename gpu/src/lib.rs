//! GPU backend for engine-fast.
//!
//! The rules live in `kernel/` as a C subset built twice: as a host library (the *twin*,
//! always built) and as CUDA device code (with the `cuda` feature). Both are checked bit for
//! bit against the Rust engine; this crate is the Rust side of those checks and, later, the
//! backend behind the batch interface.

pub mod action;
#[cfg(feature = "cuda")]
pub mod batch;
pub mod check;
#[cfg(feature = "cuda")]
pub mod device;
pub mod dump;
pub mod floats;
pub mod menu;
pub mod outputs;
pub mod state;
pub mod step;
pub mod tables;
pub mod view;
