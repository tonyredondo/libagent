//! Process spawning and management utilities.
//!
//! This module handles the platform-specific logic for spawning child processes,
//! configuring them for IPC-only operation, and managing process lifecycle.

#[cfg(target_os = "linux")]
fn set_parent_death_signal(signal: libc::c_int) -> std::io::Result<()> {
    // SAFETY: `prctl` is invoked with the expected arguments for PR_SET_PDEATHSIG.
    let result = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, signal as libc::c_ulong, 0, 0, 0) };

    if result == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(windows)]
use crate::config::get_trace_agent_pipe_name;
#[cfg(unix)]
use crate::config::get_trace_agent_uds_path;
use crate::logging::{is_debug_enabled, log_debug};
use std::process::{Child, Command, Stdio};

/// Specification of a subprocess to manage.
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    /// Human-readable name used in logs.
    pub name: &'static str,
    /// Executable path or binary name.
    pub program: String,
    /// Command-line arguments.
    pub args: Vec<String>,
}

impl ProcessSpec {
    pub fn new(name: &'static str, program: String, args: Vec<String>) -> Self {
        Self {
            name,
            program,
            args,
        }
    }
}

/// Returns true if child processes should inherit stdout/stderr for debugging.
///
/// In debug mode, inherit stdout/stderr so the child processes' output is visible.
/// Otherwise, silence both streams to avoid chatty output in host applications.
pub fn child_stdio_inherit() -> bool {
    // Inherit when explicit debug env is on or our internal level is Debug.
    if is_debug_enabled() || crate::logging::current_log_level() >= crate::logging::LogLevel::Debug
    {
        return true;
    }
    // If the optional `log` facade is enabled and the host logger is at debug for our target,
    // also inherit to surface child output alongside our logs.
    #[cfg(feature = "log")]
    {
        if log::log_enabled!(target: "libagent", log::Level::Debug) {
            return true;
        }
    }
    false
}

/// Configures a Command for trace-agent IPC-only operation.
///
/// This sets up the trace-agent to use local IPC transport only,
/// preventing conflicts with system Datadog installations.
pub fn configure_trace_agent_command(cmd: &mut Command) {
    // Disable TCP receiver to force IPC-only operation
    cmd.env("DD_APM_RECEIVER_PORT", "0");

    #[cfg(unix)]
    {
        // Create directory for the configured Unix Domain Socket if it doesn't exist
        // Uses the configured path (defaulting to a libagent-specific temp socket)
        let socket_path = std::path::PathBuf::from(get_trace_agent_uds_path());
        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        cmd.env("DD_APM_RECEIVER_SOCKET", socket_path.as_os_str());
    }

    #[cfg(windows)]
    {
        // Use configured pipe name (default: datadog-libagent) for IPC-only operation
        let pipe_name = get_trace_agent_pipe_name();
        let pipe_env = pipe_name
            .strip_prefix("\\\\.\\pipe\\")
            .unwrap_or(&pipe_name);
        cmd.env("DD_APM_WINDOWS_PIPE_NAME", pipe_env);
    }
}

/// Configures Unix-specific process spawning behavior.
///
/// On Unix systems, this creates a new session/process group via `setsid()`
/// and optionally sets up parent death signals on Linux.
#[cfg(unix)]
pub fn configure_unix_process(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            // Create new session/process group for isolation and clean shutdown
            // SAFETY: calling setsid in child just before exec
            if libc::setsid() == -1 {
                // If setsid fails, continue anyway; we just lose group control
            }

            // On Linux, set up parent death signal so child dies if parent dies
            // This ensures no orphaned processes even when parent is killed forcefully
            // SAFETY: prctl syscall is called correctly here
            #[cfg(target_os = "linux")]
            if let Err(err) = set_parent_death_signal(libc::SIGTERM) {
                // If setting PDEATHSIG fails, log but continue. Process groups still help cleanup.
                eprintln!(
                    "Warning: Failed to set parent death signal for child process: {}",
                    err
                );
            }

            Ok(())
        });
    }
}

/// Spawns a subprocess according to the provided spec.
///
/// This method handles platform-specific process spawning configuration:
///
/// # Unix (Linux/macOS/BSD)
/// - Creates new session/process group via `setsid()` for clean process tree management
/// - On Linux: Sets up parent death signal (`PR_SET_PDEATHSIG`) to ensure child processes
///   are automatically terminated if the parent dies (preventing orphaned processes)
/// - Trace-agent is configured for IPC-only operation using Unix Domain Sockets
///
/// # Windows
/// - Trace-agent is configured for IPC-only operation using Named Pipes
/// - Job Object assignment happens after spawn
///
/// # Process Configuration
/// - Stdin is always null to prevent blocking on input
/// - Stdout/stderr handling depends on debug mode (inherited vs null)
/// - Trace-agent gets special IPC-only configuration to avoid conflicts with system installations
///
/// # Safety
/// - Unix: Uses `pre_exec` with unsafe `setsid()` and `prctl()` calls
pub fn spawn_process(spec: &ProcessSpec) -> std::io::Result<Child> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).stdin(Stdio::null());

    // Configure trace-agent with IPC-only settings to prevent conflicts
    // with system Datadog installations and ensure local-only communication
    if spec.name == "trace-agent" {
        configure_trace_agent_command(&mut cmd);
    }

    // Configure stdio based on debug settings
    if child_stdio_inherit() {
        cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
    }

    // On Unix, create a new session/process group so we can signal the whole tree
    #[cfg(unix)]
    configure_unix_process(&mut cmd);

    let child = cmd.spawn()?;

    log_debug(&format!(
        "Spawned {} (program='{}', pid={})",
        spec.name,
        spec.program,
        child.id()
    ));

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::ffi::OsStr;

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_configure_trace_agent_command_uses_uds_override() {
        let temp_dir = tempfile::tempdir().unwrap();
        let socket_path = temp_dir.path().join("custom_libagent.socket");

        // Ensure clean state by removing any existing value
        let mut cmd = Command::new("true");
        let socket_override = socket_path.to_string_lossy().to_string();
        unsafe {
            std::env::set_var("LIBAGENT_TRACE_AGENT_UDS", &socket_override);
        }
        configure_trace_agent_command(&mut cmd);

        let socket_env = cmd
            .get_envs()
            .find(|(key, _)| key == &OsStr::new("DD_APM_RECEIVER_SOCKET"))
            .and_then(|(_, value)| value.map(|v| v.to_os_string()));

        assert_eq!(
            socket_env.as_deref(),
            Some(socket_path.as_os_str()),
            "trace-agent socket env should match override"
        );

        unsafe {
            std::env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
        }
        drop(socket_path);
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_configure_trace_agent_command_uses_pipe_override() {
        unsafe {
            std::env::set_var("LIBAGENT_TRACE_AGENT_PIPE", "custom-pipe-name");
        }

        let mut cmd = Command::new("cmd");
        configure_trace_agent_command(&mut cmd);

        let pipe_env = cmd
            .get_envs()
            .find(|(key, _)| key == &OsStr::new("DD_APM_WINDOWS_PIPE_NAME"))
            .and_then(|(_, value)| value.map(|v| v.to_os_string()));

        assert_eq!(
            pipe_env.as_deref(),
            Some(OsStr::new("custom-pipe-name")),
            "trace-agent pipe env should match override"
        );

        unsafe {
            std::env::remove_var("LIBAGENT_TRACE_AGENT_PIPE");
        }
    }

    #[cfg(windows)]
    #[test]
    #[serial]
    fn test_configure_trace_agent_command_strips_pipe_prefix() {
        let prefixed = r"\\.\pipe\prefixed-pipe";
        unsafe {
            std::env::set_var("LIBAGENT_TRACE_AGENT_PIPE", prefixed);
        }

        let mut cmd = Command::new("cmd");
        configure_trace_agent_command(&mut cmd);

        let pipe_env = cmd
            .get_envs()
            .find(|(key, _)| key == &OsStr::new("DD_APM_WINDOWS_PIPE_NAME"))
            .and_then(|(_, value)| value.map(|v| v.to_os_string()));

        assert_eq!(
            pipe_env.as_deref(),
            Some(OsStr::new("prefixed-pipe")),
            "trace-agent pipe env should strip \\.\\pipe\\ prefix"
        );

        unsafe {
            std::env::remove_var("LIBAGENT_TRACE_AGENT_PIPE");
        }
    }
}
