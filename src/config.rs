//! Configuration constants for libagent.
//!
//! These values define the programs and arguments used to launch the
//! Datadog Agent and Trace Agent subprocesses. They are intentionally
//! compile-time constants so that host applications do not rely on any
//! runtime environment to configure process execution.
//!
//! If you need to change the programs or arguments, update the constants
//! in this module and rebuild the library.

/// Path or binary name for the main Datadog Agent.
///
/// Examples:
/// - "datadog-agent"
/// - "/usr/bin/datadog-agent"
pub(crate) const AGENT_PROGRAM: &str = "datadog-agent";

/// Arguments to pass to the Datadog Agent.
pub(crate) const AGENT_ARGS: &[&str] = &[];

/// Path or binary name for the Datadog Trace Agent.
///
/// Examples:
/// - "trace-agent"
/// - "/usr/bin/trace-agent"
pub(crate) const TRACE_AGENT_PROGRAM: &str = "trace-agent";

/// Arguments to pass to the Trace Agent.
pub(crate) const TRACE_AGENT_ARGS: &[&str] = &[];

/// Monitor loop interval in seconds.
pub(crate) const MONITOR_INTERVAL_SECS: u64 = 1;

/// Graceful shutdown timeout before forcing kill (seconds).
#[cfg(unix)]
pub(crate) const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;

/// Initial and maximum backoff for respawn on failure (seconds).
pub(crate) const BACKOFF_INITIAL_SECS: u64 = 1;
pub(crate) const BACKOFF_MAX_SECS: u64 = 30;

/// Environment variable names used for runtime overrides.
const ENV_AGENT_PROGRAM: &str = "LIBAGENT_AGENT_PROGRAM";
const ENV_AGENT_ARGS: &str = "LIBAGENT_AGENT_ARGS";
const ENV_TRACE_AGENT_PROGRAM: &str = "LIBAGENT_TRACE_AGENT_PROGRAM";
const ENV_TRACE_AGENT_ARGS: &str = "LIBAGENT_TRACE_AGENT_ARGS";
const ENV_MONITOR_INTERVAL_SECS: &str = "LIBAGENT_MONITOR_INTERVAL_SECS";

/// Returns agent program, allowing env override via `LIBAGENT_AGENT_PROGRAM`.
pub fn get_agent_program() -> String {
    std::env::var(ENV_AGENT_PROGRAM).unwrap_or_else(|_| AGENT_PROGRAM.to_string())
}

/// Returns agent args, allowing env override via `LIBAGENT_AGENT_ARGS`.
/// The override is parsed using shell-style splitting.
pub fn get_agent_args() -> Vec<String> {
    match std::env::var(ENV_AGENT_ARGS) {
        Ok(val) => shell_words::split(val.trim()).unwrap_or_default(),
        Err(_) => AGENT_ARGS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Returns trace agent program, allowing env override via `LIBAGENT_TRACE_AGENT_PROGRAM`.
pub fn get_trace_agent_program() -> String {
    std::env::var(ENV_TRACE_AGENT_PROGRAM).unwrap_or_else(|_| TRACE_AGENT_PROGRAM.to_string())
}

/// Returns trace agent args, allowing env override via `LIBAGENT_TRACE_AGENT_ARGS`.
pub fn get_trace_agent_args() -> Vec<String> {
    match std::env::var(ENV_TRACE_AGENT_ARGS) {
        Ok(val) => shell_words::split(val.trim()).unwrap_or_default(),
        Err(_) => TRACE_AGENT_ARGS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Returns monitor interval in seconds, allowing env override via `LIBAGENT_MONITOR_INTERVAL_SECS`.
pub fn get_monitor_interval_secs() -> u64 {
    match std::env::var(ENV_MONITOR_INTERVAL_SECS) {
        Ok(val) => val.parse::<u64>().unwrap_or(MONITOR_INTERVAL_SECS),
        Err(_) => MONITOR_INTERVAL_SECS,
    }
}
