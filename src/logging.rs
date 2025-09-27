//! Logging utilities for libagent.
//!
//! This module provides centralized logging functionality with support for
//! different log levels, timestamps, and both stderr and optional log facade routing.

use std::sync::OnceLock;

/// Environment variable to enable verbose debug logging.
///
/// When this variable is set to a truthy value ("1", "true", "yes", "on"),
/// the library prints detailed logs about its activity, and the spawned
/// subprocesses' stdout/stderr are inherited by the host process so their
/// output becomes visible.
const ENV_DEBUG: &str = "LIBAGENT_DEBUG";
const ENV_LOG_LEVEL: &str = "LIBAGENT_LOG"; // one of: error, warn, info, debug

/// Returns true if debug logging is enabled via `LIBAGENT_DEBUG`.
pub fn is_debug_enabled() -> bool {
    static DEBUG: OnceLock<bool> = OnceLock::new();
    *DEBUG.get_or_init(|| match std::env::var(ENV_DEBUG) {
        Ok(val) => {
            let normalized = val.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        }
        Err(_) => false,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

pub fn current_log_level() -> LogLevel {
    static LEVEL: OnceLock<LogLevel> = OnceLock::new();
    *LEVEL.get_or_init(|| {
        if is_debug_enabled() {
            return LogLevel::Debug;
        }
        match std::env::var(ENV_LOG_LEVEL) {
            Ok(val) => match val.trim().to_ascii_lowercase().as_str() {
                "debug" => LogLevel::Debug,
                "info" => LogLevel::Info,
                "warn" | "warning" => LogLevel::Warn,
                "error" => LogLevel::Error,
                _ => LogLevel::Error,
            },
            Err(_) => LogLevel::Error,
        }
    })
}

fn format_timestamp() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn format_log_level(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN",
        LogLevel::Info => "INFO",
        LogLevel::Debug => "DEBUG",
    }
}

#[cfg(feature = "log")]
fn log_at(level: LogLevel, msg: &str) {
    // Defer filtering to the `log` facade; emit at mapped level
    let timestamped_msg = format!(
        "{} [libagent] [{}] {}",
        format_timestamp(),
        format_log_level(level),
        msg
    );
    match level {
        LogLevel::Error => log::error!(target: "libagent", "{}", timestamped_msg),
        LogLevel::Warn => log::warn!(target: "libagent", "{}", timestamped_msg),
        LogLevel::Info => log::info!(target: "libagent", "{}", timestamped_msg),
        LogLevel::Debug => log::debug!(target: "libagent", "{}", timestamped_msg),
    }
}

#[cfg(not(feature = "log"))]
fn log_at(level: LogLevel, msg: &str) {
    if current_log_level() >= level {
        // Route all logs to stderr to avoid polluting host stdout
        eprintln!(
            "{} [libagent] [{}] {}",
            format_timestamp(),
            format_log_level(level),
            msg
        );
    }
}

pub fn log_error(msg: &str) {
    log_at(LogLevel::Error, msg);
}
pub fn log_warn(msg: &str) {
    log_at(LogLevel::Warn, msg);
}
pub fn log_info(msg: &str) {
    log_at(LogLevel::Info, msg);
}
pub fn log_debug(msg: &str) {
    log_at(LogLevel::Debug, msg);
}
