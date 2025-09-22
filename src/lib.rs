//! libagent: minimal-runtime library managing Datadog Agent processes.
//!
//! This crate provides a small runtime that ensures the Datadog Agent and
//! Trace Agent are running while the library is loaded. It exposes both a
//! Rust API (`initialize`, `stop`) and a C FFI surface (`Initialize`, `Stop`).

mod config;
mod manager;
mod ffi;

pub use manager::{initialize, stop};

/// Destructor that runs on library unload. This attempts to cleanly stop the
/// monitor thread and terminate any child processes that were started.
#[ctor::dtor]
fn lib_dtor() {
    manager::stop();
}
