//! The shadow mode of the incremental step passes: when on, every pass that reuses an earlier result also runs its full computation
//! and fails the step on any difference, with an error that starts with `PREFIX`. Off by
//! default; a test or a checking harness turns it on for
//! the whole process.

use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

/// how a shadow difference's error message starts
pub const PREFIX: &str = "shadow:";

pub fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// the error for a difference found by the shadow check of `pass`
pub(crate) fn differs(pass: &str, what: &str) -> String {
    format!("{PREFIX} {pass}: {what}")
}
