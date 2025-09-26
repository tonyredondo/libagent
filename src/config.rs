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

/// Configuration validation constants.
const MIN_MONITOR_INTERVAL_SECS: u64 = 1;
const MAX_MONITOR_INTERVAL_SECS: u64 = 300; // 5 minutes
#[allow(dead_code)] // Reserved for future backoff validation
const MIN_BACKOFF_SECS: u64 = 1;
#[allow(dead_code)] // Reserved for future backoff validation
const MAX_BACKOFF_SECS: u64 = 3600; // 1 hour
#[cfg(unix)]
#[allow(dead_code)] // Reserved for future shutdown timeout validation
const MIN_SHUTDOWN_TIMEOUT_SECS: u64 = 1;
#[cfg(unix)]
#[allow(dead_code)] // Reserved for future shutdown timeout validation
const MAX_SHUTDOWN_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Environment variable names used for runtime overrides.
const ENV_AGENT_PROGRAM: &str = "LIBAGENT_AGENT_PROGRAM";
const ENV_AGENT_ARGS: &str = "LIBAGENT_AGENT_ARGS";
const ENV_AGENT_ENABLED: &str = "LIBAGENT_AGENT_ENABLED";
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

/// Validates and clamps a numeric configuration value within specified bounds.
fn validate_numeric_config(value: u64, min: u64, max: u64, env_var: &str, _default: u64) -> u64 {
    if value < min {
        eprintln!(
            "[libagent] WARN: {} value {} is below minimum {}. Using minimum value {}",
            env_var, value, min, min
        );
        min
    } else if value > max {
        eprintln!(
            "[libagent] WARN: {} value {} exceeds maximum {}. Using maximum value {}",
            env_var, value, max, max
        );
        max
    } else {
        value
    }
}

/// Returns monitor interval in seconds, allowing env override via `LIBAGENT_MONITOR_INTERVAL_SECS`.
pub fn get_monitor_interval_secs() -> u64 {
    match std::env::var(ENV_MONITOR_INTERVAL_SECS) {
        Ok(val) => match val.parse::<u64>() {
            Ok(parsed) => validate_numeric_config(
                parsed,
                MIN_MONITOR_INTERVAL_SECS,
                MAX_MONITOR_INTERVAL_SECS,
                ENV_MONITOR_INTERVAL_SECS,
                MONITOR_INTERVAL_SECS,
            ),
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

/// Returns trace agent UDS path, allowing env override via `LIBAGENT_TRACE_AGENT_UDS`.
/// For libagent, uses a custom temp path to avoid conflicts with system trace-agent.
#[cfg(unix)]
pub fn get_trace_agent_uds_path() -> String {
    std::env::var(ENV_TRACE_AGENT_UDS).unwrap_or_else(|_| {
        let temp_dir = std::env::temp_dir();
        temp_dir
            .join("datadog_libagent.socket")
            .to_string_lossy()
            .to_string()
    })
}

/// Returns trace agent Windows Named Pipe name, allowing env override via `LIBAGENT_TRACE_AGENT_PIPE`.
/// For libagent, uses a custom pipe name to avoid conflicts with system trace-agent.
#[cfg(windows)]
pub fn get_trace_agent_pipe_name() -> String {
    std::env::var(ENV_TRACE_AGENT_PIPE).unwrap_or_else(|_| "datadog-libagent".to_string())
}

/// Returns initial backoff seconds for respawn, with validation.
pub fn get_backoff_initial_secs() -> u64 {
    BACKOFF_INITIAL_SECS // This is a compile-time constant, no runtime override needed
}

/// Returns maximum backoff seconds for respawn, with validation.
pub fn get_backoff_max_secs() -> u64 {
    BACKOFF_MAX_SECS // This is a compile-time constant, no runtime override needed
}

/// Returns graceful shutdown timeout in seconds.
#[cfg(unix)]
#[allow(dead_code)] // Reserved for future shutdown timeout validation
pub fn get_graceful_shutdown_timeout_secs() -> u64 {
    GRACEFUL_SHUTDOWN_TIMEOUT_SECS // This is a compile-time constant, no runtime override needed
}

/// Returns true if the main Datadog agent should be enabled.
/// When enabled, the agent will be spawned if configured.
/// When disabled (default), only the trace-agent will run, suitable for custom trace-agent implementations.
pub fn is_agent_enabled() -> bool {
    std::env::var(ENV_AGENT_ENABLED)
        .map(|val| {
            let normalized = val.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn test_get_agent_program_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_AGENT_PROGRAM);
        }
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
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_AGENT_ARGS);
        }
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
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PROGRAM);
        }
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
    #[serial]
    fn test_get_trace_agent_args_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_ARGS);
        }
        // Test default value - should be empty since TRACE_AGENT_ARGS is empty
        let args = get_trace_agent_args();
        assert_eq!(args, Vec::<String>::new());
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
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
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
    #[serial]
    fn test_get_trace_agent_uds_path_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_UDS);
        }
        // Double-check cleanup
        assert!(std::env::var(ENV_TRACE_AGENT_UDS).is_err());

        // Test default value - should be temp directory + datadog_libagent.socket
        let path = get_trace_agent_uds_path();
        let temp_dir = std::env::temp_dir();
        let expected = temp_dir
            .join("datadog_libagent.socket")
            .to_string_lossy()
            .to_string();
        assert_eq!(path, expected);
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_get_trace_agent_uds_path_override() {
        // Ensure clean state
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_UDS);
        }

        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_UDS, "/tmp/custom.socket");
        }
        let path = get_trace_agent_uds_path();
        assert_eq!(path, "/tmp/custom.socket");

        // Clean up
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_UDS);
        }
        // Verify cleanup
        assert!(std::env::var(ENV_TRACE_AGENT_UDS).is_err());
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_get_trace_agent_pipe_name_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PIPE);
        }
        // Test default value - should be custom libagent pipe name
        let name = get_trace_agent_pipe_name();
        assert_eq!(name, "datadog-libagent");
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

    #[test]
    #[serial]
    fn test_get_monitor_interval_secs_too_low() {
        unsafe {
            std::env::set_var(ENV_MONITOR_INTERVAL_SECS, "0");
        }
        let interval = get_monitor_interval_secs();
        // Should clamp to minimum
        assert_eq!(interval, MIN_MONITOR_INTERVAL_SECS);
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
    }

    #[test]
    #[serial]
    fn test_get_monitor_interval_secs_too_high() {
        unsafe {
            std::env::set_var(ENV_MONITOR_INTERVAL_SECS, "1000");
        }
        let interval = get_monitor_interval_secs();
        // Should clamp to maximum
        assert_eq!(interval, MAX_MONITOR_INTERVAL_SECS);
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
    }

    #[test]
    fn test_get_backoff_initial_secs() {
        let initial = get_backoff_initial_secs();
        assert_eq!(initial, BACKOFF_INITIAL_SECS);
    }

    #[test]
    fn test_get_backoff_max_secs() {
        let max = get_backoff_max_secs();
        assert_eq!(max, BACKOFF_MAX_SECS);
    }

    #[cfg(unix)]
    #[test]
    fn test_get_graceful_shutdown_timeout_secs() {
        let timeout = get_graceful_shutdown_timeout_secs();
        assert_eq!(timeout, GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
    }

    #[test]
    #[serial]
    fn test_get_monitor_interval_secs_empty_string() {
        unsafe {
            std::env::set_var(ENV_MONITOR_INTERVAL_SECS, "");
        }
        let interval = get_monitor_interval_secs();
        // Should fall back to default on parse error
        assert_eq!(interval, MONITOR_INTERVAL_SECS);
        unsafe {
            std::env::remove_var(ENV_MONITOR_INTERVAL_SECS);
        }
    }

    #[test]
    #[serial]
    fn test_get_agent_args_empty() {
        unsafe {
            std::env::set_var(ENV_AGENT_ARGS, "");
        }
        let args = get_agent_args();
        assert_eq!(args, Vec::<String>::new());
        unsafe {
            std::env::remove_var(ENV_AGENT_ARGS);
        }
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_args_empty() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_ARGS, "");
        }
        let args = get_trace_agent_args();
        assert_eq!(args, Vec::<String>::new());
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_ARGS);
        }
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_args_invalid_shell_words() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_ARGS, "\"unclosed quote");
        }
        let args = get_trace_agent_args();
        // Should return default args on parse error (existing test shows this behavior)
        assert_eq!(
            args,
            TRACE_AGENT_ARGS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_ARGS);
        }
    }

    #[test]
    fn test_is_agent_enabled_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_AGENT_ENABLED);
        }
        // Test default value - should be false (disabled by default)
        assert!(!is_agent_enabled());
    }

    #[test]
    #[serial]
    fn test_is_agent_enabled_true() {
        unsafe {
            std::env::set_var(ENV_AGENT_ENABLED, "true");
        }
        assert!(is_agent_enabled());
        unsafe {
            std::env::remove_var(ENV_AGENT_ENABLED);
        }
    }

    #[test]
    #[serial]
    fn test_is_agent_enabled_1() {
        unsafe {
            std::env::set_var(ENV_AGENT_ENABLED, "1");
        }
        assert!(is_agent_enabled());
        unsafe {
            std::env::remove_var(ENV_AGENT_ENABLED);
        }
    }

    #[test]
    #[serial]
    fn test_is_agent_enabled_yes() {
        unsafe {
            std::env::set_var(ENV_AGENT_ENABLED, "yes");
        }
        assert!(is_agent_enabled());
        unsafe {
            std::env::remove_var(ENV_AGENT_ENABLED);
        }
    }

    #[test]
    #[serial]
    fn test_is_agent_enabled_false() {
        unsafe {
            std::env::set_var(ENV_AGENT_ENABLED, "false");
        }
        assert!(!is_agent_enabled());
        unsafe {
            std::env::remove_var(ENV_AGENT_ENABLED);
        }
    }
}
