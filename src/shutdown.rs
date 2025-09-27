//! Process shutdown utilities.
//!
//! This module provides platform-specific process termination logic
//! with graceful escalation strategies to ensure clean process cleanup.

#[cfg(unix)]
use crate::config::GRACEFUL_SHUTDOWN_TIMEOUT_SECS;
#[cfg(not(unix))]
use crate::logging::log_info;
#[cfg(unix)]
use crate::logging::{log_debug, log_info, log_warn};
use std::process::Child;
#[cfg(unix)]
use std::time::Duration;

/// Waits for a child process to exit with a timeout.
///
/// Returns true if the process exited within the timeout, false otherwise.
/// This prevents indefinite blocking during shutdown operations.
#[cfg(unix)]
pub fn wait_with_timeout(child: &mut Child, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

/// Send a signal to a Unix process group and wait for termination.
///
/// Returns true if the process group was successfully terminated within the timeout.
#[cfg(unix)]
pub fn signal_process_group(
    child: &mut Child,
    name: &str,
    pid: i32,
    pgid: i32,
    signal: i32,
    signal_name: &str,
    timeout: Duration,
) -> bool {
    if unsafe { libc::kill(pgid, signal) } == 0 {
        log_debug(&format!(
            "Sent {} to {} group (pid={}).",
            signal_name, name, pid
        ));
        wait_with_timeout(child, timeout)
    } else {
        log_warn(&format!(
            "Failed to send {} to {} group (pid={}).",
            signal_name, name, pid
        ));
        false
    }
}

/// Send a signal to a Unix process PID and wait for termination.
///
/// Returns true if the process was successfully terminated within the timeout.
#[cfg(unix)]
pub fn signal_process_pid(
    child: &mut Child,
    name: &str,
    pid: i32,
    signal: i32,
    signal_name: &str,
    timeout: Duration,
) -> bool {
    if unsafe { libc::kill(pid, signal) } == 0 {
        log_debug(&format!("Sent {} to {} (pid={}).", signal_name, name, pid));
        wait_with_timeout(child, timeout)
    } else {
        log_warn(&format!(
            "Failed to send {} to {} (pid={}).",
            signal_name, name, pid
        ));
        false
    }
}

/// Gracefully terminates a child process using platform-specific mechanisms.
///
/// This method implements a multi-stage shutdown strategy to ensure clean process termination
/// while avoiding orphaned child processes and providing reliable cleanup.
///
/// # Unix Shutdown Strategy (Linux/macOS/BSD)
///
/// The shutdown follows a hierarchical escalation approach:
///
/// 1. **Process Group Termination (SIGTERM)**: First attempts to terminate the entire
///    process group using negative PID (-pid). This ensures all child processes
///    and their descendants are signaled together.
///
/// 2. **Process Group Kill (SIGKILL)**: If SIGTERM doesn't work within timeout,
///    escalates to SIGKILL on the process group for immediate termination.
///
/// 3. **Individual Process Signaling**: Falls back to per-PID signaling if process
///    group operations fail (e.g., when `setsid()` failed during spawn).
///
/// 4. **Force Kill**: As a last resort, uses Rust's `child.kill()` with timeout
///    to avoid indefinite blocking.
///
/// # Windows Shutdown Strategy
///
/// Uses Job Object termination, which automatically kills all processes in the job
/// when the job handle is closed (configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`).
///
/// # Parameters
/// - `name`: Human-readable process name for logging
/// - `child_opt`: Mutable reference to Option<Child> to take ownership
///
/// # Timeout Behavior
/// - Unix: Uses `GRACEFUL_SHUTDOWN_TIMEOUT_SECS` (default 5 seconds) between escalation stages
/// - Windows: Immediate termination via Job Object
///
/// # Safety
/// - Unix: Uses libc signal functions with proper error handling
/// - Windows: Safe Job Object operations
pub fn graceful_kill(name: &str, child_opt: &mut Option<Child>) {
    if let Some(mut child) = child_opt.take() {
        #[cfg(unix)]
        {
            let pid = child.id() as i32;
            let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
            // Try process group first (negative pid targets group). If that fails,
            // fall back to signaling the specific PID to avoid hangs when setsid() fails.
            let pgid = -pid;
            let mut terminated;

            // Stage 1: TERM the entire process group for graceful shutdown
            terminated = signal_process_group(
                &mut child,
                name,
                pid,
                pgid,
                libc::SIGTERM,
                "SIGTERM",
                timeout,
            );

            // Stage 2: KILL the process group if SIGTERM didn't work
            if !terminated {
                terminated = signal_process_group(
                    &mut child,
                    name,
                    pid,
                    pgid,
                    libc::SIGKILL,
                    "SIGKILL",
                    timeout,
                );
            }

            // Stage 3: Fall back to individual process signaling if group operations failed
            if !terminated {
                terminated =
                    signal_process_pid(&mut child, name, pid, libc::SIGTERM, "SIGTERM", timeout);
                if !terminated {
                    terminated = signal_process_pid(
                        &mut child,
                        name,
                        pid,
                        libc::SIGKILL,
                        "SIGKILL",
                        timeout,
                    );
                }
            }

            // Stage 4: Last resort - use Rust's kill() with timeout to avoid hanging
            if !terminated {
                let _ = child.kill();
                if wait_with_timeout(&mut child, timeout) {
                    terminated = true;
                }
            }

            if terminated {
                log_info(&format!("{} terminated.", name));
            } else {
                log_warn(&format!(
                    "{} may still be running after shutdown attempts (pid={}).",
                    name, pid
                ));
            }
        }

        #[cfg(not(unix))]
        {
            let _ = child.kill();
            let _ = child.wait();
            log_info(&format!("{} terminated.", name));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    #[test]
    fn test_wait_with_timeout_process_exits() {
        // Start a process that exits quickly
        let mut child = Command::new("true").spawn().unwrap();

        // Should return true since the process exits quickly
        let result = wait_with_timeout(&mut child, Duration::from_secs(1));
        assert!(result);

        // Clean up
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_with_timeout_timeout() {
        // Start a long-running process
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();

        // Wait with a very short timeout - should return false (timeout)
        let result = wait_with_timeout(&mut child, Duration::from_millis(10));
        assert!(!result);

        // Clean up
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn test_graceful_kill_none_child() {
        // Test graceful_kill with None child - should not panic
        graceful_kill("test", &mut None);
    }

    #[cfg(unix)]
    #[test]
    fn test_graceful_kill_quick_exit() {
        // Start a process that exits quickly
        let child = Command::new("true").spawn().unwrap();
        let mut child_opt = Some(child);

        // Should terminate successfully
        graceful_kill("test", &mut child_opt);
        assert!(child_opt.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_graceful_kill_long_running() {
        // Start a long-running process
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let mut child_opt = Some(child);

        // This should eventually terminate the process
        graceful_kill("test", &mut child_opt);
        assert!(child_opt.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn test_graceful_kill_windows() {
        // Start a process that exits quickly
        let child = Command::new("cmd").arg("/c").arg("exit").spawn().unwrap();
        let mut child_opt = Some(child);

        // Should terminate successfully on Windows
        graceful_kill("test", &mut child_opt);
        assert!(child_opt.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_signal_process_group_invalid_pid() {
        // Start a process
        let mut child = Command::new("sleep").arg("1").spawn().unwrap();
        let pid = child.id() as i32;

        // Try to signal an invalid process group
        let result = signal_process_group(
            &mut child,
            "test",
            pid,
            -99999, // Invalid PGID
            libc::SIGTERM,
            "SIGTERM",
            Duration::from_millis(100),
        );

        // Should return false due to invalid PGID
        assert!(!result);

        // Clean up
        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn test_signal_process_pid_invalid_pid() {
        // Start a process
        let mut child = Command::new("sleep").arg("1").spawn().unwrap();

        // Try to signal an invalid PID
        let result = signal_process_pid(
            &mut child,
            "test",
            999999, // Invalid PID
            libc::SIGTERM,
            "SIGTERM",
            Duration::from_millis(100),
        );

        // Should return false due to invalid PID
        assert!(!result);

        // Clean up
        let _ = child.kill();
        let _ = child.wait();
    }
}
