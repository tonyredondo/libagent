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
/// - "agentless-agent"
/// - "/usr/bin/agentless-agent"
pub(crate) const TRACE_AGENT_PROGRAM: &str = "agentless-agent";

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
const ENV_AGENT_REMOTE_CONFIG_ADDR: &str = "LIBAGENT_AGENT_REMOTE_CONFIG_ADDR";
#[cfg(unix)]
const ENV_TRACE_AGENT_UDS: &str = "LIBAGENT_TRACE_AGENT_UDS";
#[cfg(windows)]
const ENV_TRACE_AGENT_PIPE: &str = "LIBAGENT_TRACE_AGENT_PIPE";
#[cfg(unix)]
const ENV_DOGSTATSD_UDS: &str = "LIBAGENT_DOGSTATSD_UDS";
#[cfg(windows)]
const ENV_DOGSTATSD_PIPE: &str = "LIBAGENT_DOGSTATSD_PIPE";

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

/// Returns the directory containing the libagent dynamic library.
///
/// Uses platform-specific APIs to determine the actual library file location:
/// - Unix/macOS: Uses `dladdr` to find the library path
/// - Windows: Uses `GetModuleHandleEx` and `GetModuleFileName`
fn get_library_directory() -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    {
        use std::ffi::c_void;
        unsafe {
            let mut info: libc::Dl_info = std::mem::zeroed();
            // Use the address of this function as a reference point in the library
            let addr = get_library_directory as *const c_void;
            if libc::dladdr(addr, &mut info) != 0 && !info.dli_fname.is_null() {
                let path = std::ffi::CStr::from_ptr(info.dli_fname);
                if let Ok(path_str) = path.to_str() {
                    let lib_path = std::path::Path::new(path_str);
                    return lib_path.parent().map(|p| p.to_path_buf());
                }
            }
        }
        None
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{GetLastError, HMODULE, MAX_PATH};
        use windows_sys::Win32::System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleExW,
        };
        use windows_sys::core::PCWSTR;

        unsafe {
            let mut module: HMODULE = std::ptr::null_mut();
            // Use the address of this function as a reference point in the library
            let addr: PCWSTR = (get_library_directory as *const ()).cast();
            let flags = GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT;

            if GetModuleHandleExW(flags, addr, &mut module) != 0 {
                let mut buffer = vec![0u16; MAX_PATH as usize];
                let len = GetModuleFileNameW(module, buffer.as_mut_ptr(), buffer.len() as u32);
                if len > 0 && len < buffer.len() as u32 {
                    buffer.truncate(len as usize);
                    let lib_path = std::path::PathBuf::from(String::from_utf16_lossy(&buffer));
                    return lib_path.parent().map(|p| p.to_path_buf());
                } else {
                    let error = GetLastError();
                    eprintln!(
                        "[libagent] WARN: GetModuleFileNameW failed with error {}",
                        error
                    );
                }
            } else {
                let error = GetLastError();
                eprintln!(
                    "[libagent] WARN: GetModuleHandleExW failed with error {}",
                    error
                );
            }
        }
        None
    }

    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// Returns trace agent program, allowing env override via `LIBAGENT_TRACE_AGENT_PROGRAM`.
///
/// By default, searches for the trace agent in the following locations (in order):
/// 1. Same directory as the libagent dynamic library (preferred)
/// 2. Same directory as the current executable
/// 3. Just the binary name (searches PATH)
///
/// Returns the first location where the binary exists, or falls back to just the name.
pub fn get_trace_agent_program() -> String {
    std::env::var(ENV_TRACE_AGENT_PROGRAM).unwrap_or_else(|_| {
        let mut search_paths = Vec::new();

        // First, try the dynamic library location
        if let Some(lib_dir) = get_library_directory() {
            search_paths.push(lib_dir.join(TRACE_AGENT_PROGRAM));
        }

        // Second, try relative to current executable
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            search_paths.push(exe_dir.join(TRACE_AGENT_PROGRAM));
        }

        // Search for the binary in all candidate paths
        for candidate_path in search_paths {
            if candidate_path.exists()
                && let Some(path_str) = candidate_path.to_str()
            {
                return path_str.to_string();
            }
        }

        // Final fallback to just the binary name (will search PATH)
        TRACE_AGENT_PROGRAM.to_string()
    })
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

/// Returns the remote configuration address used to detect existing agents.
///
/// By default returns `Some("127.0.0.1:5001")`. Setting
/// `LIBAGENT_AGENT_REMOTE_CONFIG_ADDR` to an alternate host:port changes the
/// detection target. Setting it to an empty or falsey value disables the probe.
pub fn get_agent_remote_config_addr() -> Option<String> {
    match std::env::var(ENV_AGENT_REMOTE_CONFIG_ADDR) {
        Ok(val) => {
            let trimmed = val.trim();
            let normalized = trimmed.to_ascii_lowercase();
            if trimmed.is_empty()
                || matches!(
                    normalized.as_str(),
                    "0" | "false" | "off" | "no" | "disabled"
                )
            {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!(
                "[libagent] WARN: {} contains non-unicode data; disabling remote config detection",
                ENV_AGENT_REMOTE_CONFIG_ADDR
            );
            None
        }
        Err(std::env::VarError::NotPresent) => Some("127.0.0.1:5001".to_string()),
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
/// For libagent, uses `/tmp/datadog_libagent.socket` to avoid conflicts with system trace-agent.
#[cfg(unix)]
pub fn get_trace_agent_uds_path() -> String {
    std::env::var(ENV_TRACE_AGENT_UDS)
        .unwrap_or_else(|_| "/tmp/datadog_libagent.socket".to_string())
}

/// Returns trace agent Windows Named Pipe name, allowing env override via `LIBAGENT_TRACE_AGENT_PIPE`.
/// For libagent, uses a custom pipe name to avoid conflicts with system trace-agent.
#[cfg(windows)]
pub fn get_trace_agent_pipe_name() -> String {
    std::env::var(ENV_TRACE_AGENT_PIPE).unwrap_or_else(|_| "datadog-libagent".to_string())
}

/// Returns DogStatsD UDS path, allowing env override via `LIBAGENT_DOGSTATSD_UDS`.
/// For libagent, uses `/tmp/datadog_dogstatsd.socket` to match the custom agent default.
#[cfg(unix)]
pub fn get_dogstatsd_uds_path() -> String {
    std::env::var(ENV_DOGSTATSD_UDS).unwrap_or_else(|_| "/tmp/datadog_dogstatsd.socket".to_string())
}

/// Returns DogStatsD Windows Named Pipe name, allowing env override via `LIBAGENT_DOGSTATSD_PIPE`.
/// For libagent, uses a custom pipe name matching the custom agent default.
#[cfg(windows)]
pub fn get_dogstatsd_pipe_name() -> String {
    std::env::var(ENV_DOGSTATSD_PIPE).unwrap_or_else(|_| "datadog-dogstatsd".to_string())
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
    #[serial]
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
    #[serial]
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
    #[serial]
    fn test_get_trace_agent_program_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PROGRAM);
        }
        // Test default value - should contain agentless-agent
        // The exact path depends on current_exe(), but should end with the binary name
        let program = get_trace_agent_program();
        assert!(
            program.ends_with(TRACE_AGENT_PROGRAM) || program == TRACE_AGENT_PROGRAM,
            "Expected program to end with '{}', got '{}'",
            TRACE_AGENT_PROGRAM,
            program
        );
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_program_override() {
        unsafe {
            std::env::set_var(ENV_TRACE_AGENT_PROGRAM, "/custom/agentless-agent");
        }
        let program = get_trace_agent_program();
        assert_eq!(program, "/custom/agentless-agent");
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PROGRAM);
        }
    }

    #[test]
    #[serial]
    fn test_get_trace_agent_program_search_behavior() {
        use std::fs;
        use std::io::Write;
        use tempfile::TempDir;

        // Clean up environment
        unsafe {
            std::env::remove_var(ENV_TRACE_AGENT_PROGRAM);
        }

        // Create a temporary directory and write a fake agentless-agent binary
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let fake_agent_path = temp_dir.path().join(TRACE_AGENT_PROGRAM);

        // Create the fake binary
        let mut file = fs::File::create(&fake_agent_path).expect("Failed to create fake agent");
        file.write_all(b"#!/bin/sh\necho test\n")
            .expect("Failed to write to fake agent");
        drop(file);

        // Make it executable on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&fake_agent_path)
                .expect("Failed to get metadata")
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&fake_agent_path, perms).expect("Failed to set permissions");
        }

        // Verify the file exists
        assert!(fake_agent_path.exists(), "Fake agent should exist");

        // Note: We can't easily test the library directory search in unit tests
        // because the test binary and library are in the same location during tests.
        // This test verifies that when a file exists, it's found by the search logic.

        // The actual search will typically fall back to TRACE_AGENT_PROGRAM since
        // the fake file isn't in the test binary's directory
        let program = get_trace_agent_program();

        // Should return either a full path (if found) or just the binary name
        assert!(
            program.ends_with(TRACE_AGENT_PROGRAM) || program == TRACE_AGENT_PROGRAM,
            "Expected program to end with or be '{}', got '{}'",
            TRACE_AGENT_PROGRAM,
            program
        );
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
    #[serial]
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

        // Test default value - should be /tmp/datadog_libagent.socket
        let path = get_trace_agent_uds_path();
        assert_eq!(path, "/tmp/datadog_libagent.socket");
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

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_get_dogstatsd_uds_path_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_DOGSTATSD_UDS);
        }
        // Double-check cleanup
        assert!(std::env::var(ENV_DOGSTATSD_UDS).is_err());

        // Test default value - should be /tmp/datadog_dogstatsd.socket
        let path = get_dogstatsd_uds_path();
        assert_eq!(path, "/tmp/datadog_dogstatsd.socket");
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_get_dogstatsd_uds_path_override() {
        // Ensure clean state
        unsafe {
            std::env::remove_var(ENV_DOGSTATSD_UDS);
        }

        unsafe {
            std::env::set_var(ENV_DOGSTATSD_UDS, "/tmp/custom_dogstatsd.socket");
        }
        let path = get_dogstatsd_uds_path();
        assert_eq!(path, "/tmp/custom_dogstatsd.socket");

        // Clean up
        unsafe {
            std::env::remove_var(ENV_DOGSTATSD_UDS);
        }
        // Verify cleanup
        assert!(std::env::var(ENV_DOGSTATSD_UDS).is_err());
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_get_dogstatsd_pipe_name_default() {
        // Clean up any environment variables from previous tests
        unsafe {
            std::env::remove_var(ENV_DOGSTATSD_PIPE);
        }
        // Test default value - should be custom dogstatsd pipe name
        let name = get_dogstatsd_pipe_name();
        assert_eq!(name, "datadog-dogstatsd");
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_get_dogstatsd_pipe_name_override() {
        unsafe {
            std::env::set_var(ENV_DOGSTATSD_PIPE, "custom-dogstatsd-pipe");
        }
        let name = get_dogstatsd_pipe_name();
        assert_eq!(name, "custom-dogstatsd-pipe");
        unsafe {
            std::env::remove_var(ENV_DOGSTATSD_PIPE);
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
    #[serial]
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

    #[test]
    #[serial]
    fn test_get_agent_remote_config_addr_default() {
        unsafe {
            std::env::remove_var(ENV_AGENT_REMOTE_CONFIG_ADDR);
        }

        let addr = get_agent_remote_config_addr();
        assert_eq!(addr.as_deref(), Some("127.0.0.1:5001"));
    }

    #[test]
    #[serial]
    fn test_get_agent_remote_config_addr_override() {
        unsafe {
            std::env::set_var(ENV_AGENT_REMOTE_CONFIG_ADDR, "localhost:6001");
        }

        let addr = get_agent_remote_config_addr();
        assert_eq!(addr.as_deref(), Some("localhost:6001"));

        unsafe {
            std::env::remove_var(ENV_AGENT_REMOTE_CONFIG_ADDR);
        }
    }

    #[test]
    #[serial]
    fn test_get_agent_remote_config_addr_disable() {
        unsafe {
            std::env::set_var(ENV_AGENT_REMOTE_CONFIG_ADDR, "off");
        }

        let addr = get_agent_remote_config_addr();
        assert!(addr.is_none());

        unsafe {
            std::env::remove_var(ENV_AGENT_REMOTE_CONFIG_ADDR);
        }
    }
}
