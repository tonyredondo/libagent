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
mod http;
mod logging;
mod manager;
mod metrics;
mod monitor;
mod process;
mod shutdown;
mod uds;
#[cfg(windows)]
mod winpipe;

pub use config::{
    get_agent_args, get_agent_program, get_agent_remote_config_addr, get_backoff_initial_secs,
    get_backoff_max_secs, get_monitor_interval_secs, get_trace_agent_args, get_trace_agent_program,
};
#[cfg(unix)]
pub use config::{get_graceful_shutdown_timeout_secs, get_trace_agent_uds_path};
pub use http::{
    add_default_headers, build_request, header_lookup, memchr_crlf_crlf, parse_headers,
    parse_status_line, serialize_headers,
};
pub use manager::{initialize, stop};
pub use metrics::{Metrics, get_metrics};

#[cfg(windows)]
pub use config::get_trace_agent_pipe_name;

/// Destructor that runs on library unload. This attempts to cleanly stop the
/// monitor thread and terminate any child processes that were started.
#[ctor::dtor]
fn lib_dtor() {
    manager::stop();
}
