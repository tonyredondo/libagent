//! libagent: minimal-runtime library managing Datadog Agent processes.
//!
//! This crate provides a small runtime that ensures the Datadog Agent and
//! Trace Agent are running while the library is loaded. It exposes both a
//! Rust API (`initialize`, `stop`) and a C FFI surface (`Initialize`, `Stop`).
//!
//! Example (Rust)
//! ```no_run
//! libagent::initialize();
//! // ... your application ...
//! libagent::stop();
//! ```
//!
//! See `src/config.rs` for environment variable overrides (e.g.,
//! `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_LOG`).

mod config;
mod ffi;
mod manager;
mod uds;

pub use manager::{initialize, stop};

/// Destructor that runs on library unload. This attempts to cleanly stop the
/// monitor thread and terminate any child processes that were started.
#[ctor::dtor]
fn lib_dtor() {
    manager::stop();
}
