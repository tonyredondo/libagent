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
#[cfg(unix)]
const ENV_TRACE_AGENT_UDS: &str = "LIBAGENT_TRACE_AGENT_UDS";
#[cfg(windows)]
const ENV_TRACE_AGENT_PIPE: &str = "LIBAGENT_TRACE_AGENT_PIPE";

/// Returns agent program, allowing env override via `LIBAGENT_AGENT_PROGRAM`.
pub fn get_agent_program() -> String {
    std::env::var(ENV_AGENT_PROGRAM).unwrap_or_else(|_| AGENT_PROGRAM.to_string())
}

/// Returns agent args, allowing env override via `LIBAGENT_AGENT_ARGS`.
/// The override is parsed using shell-style splitting.
pub fn get_agent_args() -> Vec<String> {
    match std::env::var(ENV_AGENT_ARGS) {
        Ok(val) => match shell_words::split(val.trim()) {
            Ok(args) => args,
            Err(e) => {
                eprintln!(
                    "[libagent] WARN: Failed to parse {} '{}': {}. Using default args",
                    ENV_AGENT_ARGS, val, e
                );
                AGENT_ARGS.iter().map(|s| s.to_string()).collect()
            }
        },
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
        Ok(val) => match shell_words::split(val.trim()) {
            Ok(args) => args,
            Err(e) => {
                eprintln!(
                    "[libagent] WARN: Failed to parse {} '{}': {}. Using default args",
                    ENV_TRACE_AGENT_ARGS, val, e
                );
                TRACE_AGENT_ARGS.iter().map(|s| s.to_string()).collect()
            }
        },
        Err(_) => TRACE_AGENT_ARGS.iter().map(|s| s.to_string()).collect(),
    }
}

/// Returns monitor interval in seconds, allowing env override via `LIBAGENT_MONITOR_INTERVAL_SECS`.
pub fn get_monitor_interval_secs() -> u64 {
    match std::env::var(ENV_MONITOR_INTERVAL_SECS) {
        Ok(val) => match val.parse::<u64>() {
            Ok(parsed) => parsed,
            Err(e) => {
                eprintln!(
                    "[libagent] WARN: Invalid {} '{}': {}. Using default value {}s",
                    ENV_MONITOR_INTERVAL_SECS, val, e, MONITOR_INTERVAL_SECS
                );
                MONITOR_INTERVAL_SECS
            }
        },
        Err(_) => MONITOR_INTERVAL_SECS,
    }
}

/// Default Unix Domain Socket path for the trace agent.
#[cfg(unix)]
pub(crate) const TRACE_AGENT_UDS_DEFAULT: &str = "/var/run/datadog/apm.socket";

/// Returns trace agent UDS path, allowing env override via `LIBAGENT_TRACE_AGENT_UDS`.
#[cfg(unix)]
pub fn get_trace_agent_uds_path() -> String {
    std::env::var(ENV_TRACE_AGENT_UDS).unwrap_or_else(|_| TRACE_AGENT_UDS_DEFAULT.to_string())
}

/// Default Windows Named Pipe name for the trace agent (used as \\.\\pipe\\<name>).
#[cfg(windows)]
pub(crate) const TRACE_AGENT_PIPE_DEFAULT: &str = "trace-agent";

/// Returns trace agent Windows Named Pipe name, allowing env override via `LIBAGENT_TRACE_AGENT_PIPE`.
#[cfg(windows)]
pub fn get_trace_agent_pipe_name() -> String {
    std::env::var(ENV_TRACE_AGENT_PIPE).unwrap_or_else(|_| TRACE_AGENT_PIPE_DEFAULT.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_get_agent_program_default() {
        // Test default value
        let program = get_agent_program();
        assert_eq!(program, AGENT_PROGRAM);
    }

    #[test]
    #[serial]
    fn test_get_agent_program_override() {
        unsafe {
            std::env::set_var(ENV_AGENT_PROGRAM, "/custom/path/agent");
        }
        let program = get_agent_program();
        assert_eq!(program, "/custom/path/agent");
        unsafe {
            std::env::remove_var(ENV_AGENT_PROGRAM);
        }
    }

    #[test]
    fn test_get_agent_args_default() {
        // Test default value
        let args = get_agent_args();
        assert_eq!(
            args,
            AGENT_ARGS.iter().map(|s| s.to_string()).collect::<Vec<_>>()
        );
    }

    #[test]
    #[serial]
    fn test_get_agent_args_override() {
        unsafe {
            std::env::set_var(ENV_AGENT_ARGS, "--config /path/to/config --verbose");
        }
        let args = get_agent_args();
        assert_eq!(
            args,
            vec![
                "--config".to_string(),
                "/path/to/config".to_string(),
                "--verbose".to_string()
            ]
        );
        unsafe {
            std::env::remove_var(ENV_AGENT_ARGS);
        }
    }

    #[test]
    #[serial]
    fn test_get_agent_args_invalid_shell_words() {
        unsafe {
            std::env::set_var(ENV_AGENT_ARGS, "\"unclosed quote");
        }
        let args = get_agent_args();
        // Should return empty vec on parse error
        assert_eq!(args, Vec::<String>::new());
        unsafe {
            std::env::remove_var(ENV_AGENT_ARGS);
        }
    }

    #[test]
    fn test_get_trace_agent_program_default() {
        // Test default value
        let program = get_trace_agent_program();
        assert_eq!(program, TRACE_AGENT_PROGRAM);
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_program_override() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_PROGRAM, "/custom/trace-agent");
        }
        let program = get_trace_agent_program();
        assert_eq!(program, "/custom/trace-agent");
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PROGRAM);
        }
    }

    #[test]
    fn test_get_trace_agent_args_default() {
        // Test default value
        let args = get_trace_agent_args();
        assert_eq!(
            args,
            TRACE_AGENT_ARGS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_args_override() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_ARGS, "--port 8126 --debug");
        }
        let args = get_trace_agent_args();
        assert_eq!(
            args,
            vec![
                "--port".to_string(),
                "8126".to_string(),
                "--debug".to_string()
            ]
        );
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_ARGS);
        }
    }

    #[test]
    fn test_get_monitor_interval_secs_default() {
        // Test default value
        let interval = get_monitor_interval_secs();
        assert_eq!(interval, MONITOR_INTERVAL_SECS);
    }

    #[test]
    #[serial]
    fn test_get_monitor_interval_secs_override() {
        unsafe {
            std::env::set_var(ENV_MONITOR_INTERVAL_SECS, "5");
        }
        let interval = get_monitor_interval_secs();
        assert_eq!(interval, 5);
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
    }

    #[test]
    #[serial]
    fn test_get_monitor_interval_secs_invalid() {
        unsafe {
            std::env::set_var(ENV_MONITOR_INTERVAL_SECS, "not-a-number");
        }
        let interval = get_monitor_interval_secs();
        // Should fall back to default on parse error
        assert_eq!(interval, MONITOR_INTERVAL_SECS);
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_get_trace_agent_uds_path_default() {
        // Test default value
        let path = get_trace_agent_uds_path();
        assert_eq!(path, TRACE_AGENT_UDS_DEFAULT);
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_get_trace_agent_uds_path_override() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_UDS, "/tmp/custom.socket");
        }
        let path = get_trace_agent_uds_path();
        assert_eq!(path, "/tmp/custom.socket");
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_UDS);
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_get_trace_agent_pipe_name_default() {
        // Test default value
        let name = get_trace_agent_pipe_name();
        assert_eq!(name, TRACE_AGENT_PIPE_DEFAULT);
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_get_trace_agent_pipe_name_override() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_PIPE, "custom-pipe");
        }
        let name = get_trace_agent_pipe_name();
        assert_eq!(name, "custom-pipe");
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PIPE);
        }
    }
}
