//! GPU backend for engine-fast: the batch interface with every game in GPU memory.
//!
//! The rules live in `kernel/` as a C subset built twice: as a host library (the *twin*,
//! always built, for tests without a GPU) and as CUDA device code (feature `cuda`). Both
//! are checked bit for bit against the Rust engine. With `cuda`, `batch::GpuBatch` is the
//! batch on the device: one thread per game, views, legal moves and moves kept in device
//! memory, and finished games restarted from games built ahead of time on CPU threads
//! (`restarts`).

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
pub mod restarts;
pub mod state;
pub mod step;
pub mod tables;
pub mod view;
