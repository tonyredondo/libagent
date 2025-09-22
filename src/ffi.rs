//! C FFI surface for libagent.
//!
//! This module exposes a minimal set of functions intended to be consumed
//! from non-Rust languages. The business logic is provided by
//! `crate::manager`, and these functions simply forward to those safe
//! Rust APIs.

use crate::manager;
use std::panic::{catch_unwind, AssertUnwindSafe};

/// Initialize the library: start the Agent and Trace Agent and begin monitoring.
///
/// This function is safe to call multiple times; subsequent calls are no-ops
/// once the library has already been initialized.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn Initialize() {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        manager::initialize();
    }));
}

/// Stop the library: terminate the Agent and Trace Agent and stop monitoring.
///
/// This function is safe to call multiple times.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn Stop() {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        manager::stop();
    }));
}

