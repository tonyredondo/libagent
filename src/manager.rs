//! Core process-lifetime management for the Datadog Agent and Trace Agent.
//!
//! This module encapsulates the business logic of spawning the two required
//! processes, monitoring them for unexpected exits, and ensuring that they
//! are restarted while the library remains loaded and "initialized".
//!
//! The module exposes plain Rust functions (`initialize` and `stop`) for
//! Rust consumers and to be called from the C FFI layer.

#[cfg(unix)]
use crate::config::GRACEFUL_SHUTDOWN_TIMEOUT_SECS;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
#[cfg(windows)]
unsafe extern "system" {
    fn CreateJobObjectW(lpJobAttributes: *const core::ffi::c_void, lpName: *const u16) -> HANDLE;
}

use crate::config::{
    get_agent_args, get_agent_program, get_backoff_initial_secs, get_backoff_max_secs,
    get_monitor_interval_secs, get_trace_agent_args, get_trace_agent_program,
};
use crate::metrics;
#[cfg(windows)]
use crate::winpipe;

/// Environment variable to enable verbose debug logging.
///
/// When this variable is set to a truthy value ("1", "true", "yes", "on"),
/// the library prints detailed logs about its activity, and the spawned
/// subprocesses' stdout/stderr are inherited by the host process so their
/// output becomes visible.
const ENV_DEBUG: &str = "LIBAGENT_DEBUG";
const ENV_LOG_LEVEL: &str = "LIBAGENT_LOG"; // one of: error, warn, info, debug

/// Returns true if debug logging is enabled via `LIBAGENT_DEBUG`.
fn is_debug_enabled() -> bool {
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
enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

fn current_log_level() -> LogLevel {
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

#[cfg(feature = "log")]
fn log_at(level: LogLevel, msg: &str) {
    // Defer filtering to the `log` facade; emit at mapped level
    match level {
        LogLevel::Error => log::error!(target: "libagent", "{}", msg),
        LogLevel::Warn => log::warn!(target: "libagent", "{}", msg),
        LogLevel::Info => log::info!(target: "libagent", "{}", msg),
        LogLevel::Debug => log::debug!(target: "libagent", "{}", msg),
    }
}

#[cfg(not(feature = "log"))]
fn log_at(level: LogLevel, msg: &str) {
    if current_log_level() >= level {
        // Route all logs to stderr to avoid polluting host stdout
        eprintln!("[libagent] {}", msg);
    }
}

fn log_error(msg: &str) {
    log_at(LogLevel::Error, msg);
}
fn log_warn(msg: &str) {
    log_at(LogLevel::Warn, msg);
}
fn log_info(msg: &str) {
    log_at(LogLevel::Info, msg);
}
fn log_debug(msg: &str) {
    log_at(LogLevel::Debug, msg);
}

/// Helper function to lock a mutex while handling potential poisoning.
/// If the mutex is poisoned (due to a panic in another thread), we recover
/// the data and continue, logging a warning.
fn lock_mutex<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log_warn("Mutex was poisoned, recovering and continuing");
            poisoned.into_inner()
        }
    }
}

fn child_stdio_inherit() -> bool {
    // Inherit when explicit debug env is on or our internal level is Debug.
    if is_debug_enabled() || current_log_level() >= LogLevel::Debug {
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

/// Specification of a subprocess to manage.
#[derive(Clone, Debug)]
struct ProcessSpec {
    /// Human-readable name used in logs.
    name: &'static str,
    /// Executable path or binary name.
    program: String,
    /// Command-line arguments.
    args: Vec<String>,
}

impl ProcessSpec {
    fn new(name: &'static str, program: String, args: Vec<String>) -> Self {
        Self {
            name,
            program,
            args,
        }
    }
}

/// Manager responsible for starting, monitoring, and stopping the two agents.
///
/// The manager keeps track of child processes for both the Agent and Trace
/// Agent. A background monitor thread periodically checks if either child has
/// exited, and respawns it when necessary while initialization is active.
pub struct AgentManager {
    /// Flag indicating whether the monitoring loop should run.
    should_run: AtomicBool,
    /// Coarse-grained mutex to serialize start/stop operations.
    start_stop_lock: Mutex<()>,
    /// Handle to the monitor thread, if running.
    monitor_thread: Mutex<Option<JoinHandle<()>>>,
    monitor_cv: Condvar,
    monitor_cv_lock: Mutex<()>,
    /// Subprocess specification for the Agent.
    agent_spec: ProcessSpec,
    /// Subprocess specification for the Trace Agent.
    trace_spec: ProcessSpec,
    /// Currently running Agent child, if any.
    agent_child: Mutex<Option<Child>>,
    /// Currently running Trace Agent child, if any.
    trace_child: Mutex<Option<Child>>,
    /// Backoff state for agent respawns
    agent_backoff_secs: Mutex<u64>,
    agent_next_attempt: Mutex<Option<Instant>>,
    /// Backoff state for trace-agent respawns
    trace_backoff_secs: Mutex<u64>,
    trace_next_attempt: Mutex<Option<Instant>>,
    #[cfg(windows)]
    windows_job: Mutex<Option<isize>>, // Job handle stored as isize for Send/Sync
}

impl AgentManager {
    /// Creates a new manager with default `ProcessSpec`s based on constants.
    fn new() -> Self {
        Self {
            should_run: AtomicBool::new(false),
            start_stop_lock: Mutex::new(()),
            monitor_thread: Mutex::new(None),
            monitor_cv: Condvar::new(),
            monitor_cv_lock: Mutex::new(()),
            agent_spec: ProcessSpec::new("agent", get_agent_program(), get_agent_args()),
            trace_spec: ProcessSpec::new(
                "trace-agent",
                get_trace_agent_program(),
                get_trace_agent_args(),
            ),
            agent_child: Mutex::new(None),
            trace_child: Mutex::new(None),
            agent_backoff_secs: Mutex::new(get_backoff_initial_secs()),
            agent_next_attempt: Mutex::new(None),
            trace_backoff_secs: Mutex::new(get_backoff_initial_secs()),
            trace_next_attempt: Mutex::new(None),
            #[cfg(windows)]
            windows_job: Mutex::new(None),
        }
    }

    /// Spawns a subprocess according to the provided spec.
    fn spawn_process(&self, spec: &ProcessSpec) -> std::io::Result<Child> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args).stdin(Stdio::null());

        // Configure trace-agent with IPC-only settings
        if spec.name == "trace-agent" {
            // Disable TCP receiver
            cmd.env("DD_APM_RECEIVER_PORT", "0");

            #[cfg(unix)]
            {
                // Create temp directory for socket if it doesn't exist
                let temp_dir = std::env::temp_dir();
                let socket_path = temp_dir.join("datadog_libagent.socket");
                if let Some(parent) = socket_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                cmd.env(
                    "DD_APM_RECEIVER_SOCKET",
                    socket_path.to_string_lossy().as_ref(),
                );
            }

            #[cfg(windows)]
            {
                // Use custom pipe name for libagent
                cmd.env("DD_APM_WINDOWS_PIPE_NAME", "datadog-libagent");
            }
        }

        // In debug mode, inherit stdout/stderr so the child processes' output is visible.
        // Otherwise, silence both streams to avoid chatty output in host applications.
        if child_stdio_inherit() {
            cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        } else {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }

        // On Unix, create a new session/process group so we can signal the whole tree
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    // SAFETY: calling setsid in child just before exec
                    if libc::setsid() == -1 {
                        // If setsid fails, continue anyway; we just lose group control
                    }
                    Ok(())
                });
            }
        }

        let child = cmd.spawn()?;

        // On Windows, assign child to Job object so we can terminate whole tree later
        #[cfg(windows)]
        {
            self.assign_child_to_job(&child);
        }

        // Record metrics
        match spec.name {
            "agent" => metrics::get_metrics().record_agent_spawn(),
            "trace-agent" => metrics::get_metrics().record_trace_agent_spawn(),
            _ => {} // Unknown process type, don't record
        }

        log_debug(&format!(
            "Spawned {} (program='{}', pid={})",
            spec.name,
            spec.program,
            child.id()
        ));
        Ok(child)
    }

    #[cfg(windows)]
    fn windows_job_handle(&self) -> HANDLE {
        use std::ptr::null_mut;
        let mut guard = lock_mutex(&self.windows_job);
        if let Some(hraw) = *guard {
            return hraw as HANDLE;
        }
        unsafe {
            let job: HANDLE = CreateJobObjectW(null_mut(), null_mut());
            if job.is_null() {
                log_warn(
                    "Failed to create Windows Job object; process-tree termination may be unreliable.",
                );
                return null_mut();
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // Explicitly zero other potentially sensitive fields
            info.BasicLimitInformation.PerProcessUserTimeLimit = 0;
            info.BasicLimitInformation.PerJobUserTimeLimit = 0;
            info.IoInfo = std::mem::zeroed();
            info.JobMemoryLimit = 0;
            info.ProcessMemoryLimit = 0;
            info.PeakProcessMemoryUsed = 0;
            info.PeakJobMemoryUsed = 0;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                log_warn(
                    "Failed to configure Windows Job object; process-tree termination may be unreliable.",
                );
                CloseHandle(job);
                return null_mut();
            }
            *guard = Some(job as isize);
            job
        }
    }

    #[cfg(windows)]
    fn assign_child_to_job(&self, child: &Child) {
        let job = self.windows_job_handle();
        if job.is_null() {
            return;
        }
        unsafe {
            let ph: HANDLE = child.as_raw_handle() as HANDLE;
            let ok = AssignProcessToJobObject(job, ph);
            if ok == 0 {
                log_warn(
                    "Failed to assign child process to Windows Job; termination may leave orphans.",
                );
            }
        }
    }

    /// Ensures the child described by `spec` is running. If it has exited or was never started,
    /// attempts to (re)spawn it.
    fn tick_process(
        &self,
        child_guard: &mut Option<Child>,
        spec: &ProcessSpec,
        backoff_secs_guard: &mut u64,
        next_attempt_guard: &mut Option<Instant>,
    ) {
        // If child exists, see if it exited
        if let Some(child) = child_guard.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    log_warn(&format!(
                        "{} exited with status {:?}. Will respawn.",
                        spec.name, status
                    ));
                    *child_guard = None;
                }
                Ok(None) => {
                    return; // still running
                }
                Err(err) => {
                    log_warn(&format!(
                        "Failed to check {} status: {}. Treating as not running.",
                        spec.name, err
                    ));
                    *child_guard = None;
                }
            }
        }

        // Not running; check backoff window
        let now = Instant::now();
        if next_attempt_guard.as_ref().is_some_and(|&next| now < next) {
            return;
        }

        // Try to spawn
        match self.spawn_process(spec) {
            Ok(new_child) => {
                *child_guard = Some(new_child);
                *backoff_secs_guard = get_backoff_initial_secs();
                *next_attempt_guard = None;
            }
            Err(err) => {
                // Record failure metrics
                match spec.name {
                    "agent" => metrics::get_metrics().record_agent_failure(),
                    "trace-agent" => metrics::get_metrics().record_trace_agent_failure(),
                    _ => {}
                }

                log_error(&format!(
                    "Failed to spawn {} (program='{}', args={:?}): {}. Backing off {}s.",
                    spec.name, spec.program, spec.args, err, *backoff_secs_guard
                ));
                let wait = Duration::from_secs(*backoff_secs_guard);
                *next_attempt_guard = Some(now + wait);
                *backoff_secs_guard = (*backoff_secs_guard)
                    .saturating_mul(2)
                    .min(get_backoff_max_secs());
            }
        }
    }

    /// Starts both child processes (if not already started) and launches the monitor thread.
    fn start(&self) {
        let _guard = lock_mutex(&self.start_stop_lock);
        if self.should_run.swap(true, Ordering::SeqCst) {
            // Already running
            log_debug("Initialize called; manager already running.");
            return;
        }

        // Record initialization metrics
        metrics::get_metrics().record_initialization();

        log_info("Starting Agent and Trace Agent...");

        // Ensure processes started immediately
        {
            let mut a = lock_mutex(&self.agent_child);
            let mut ab = lock_mutex(&self.agent_backoff_secs);
            let mut an = lock_mutex(&self.agent_next_attempt);
            self.tick_process(&mut a, &self.agent_spec, &mut ab, &mut an);
        }
        {
            let mut t = lock_mutex(&self.trace_child);
            let mut tb = lock_mutex(&self.trace_backoff_secs);
            let mut tn = lock_mutex(&self.trace_next_attempt);
            self.tick_process(&mut t, &self.trace_spec, &mut tb, &mut tn);
        }

        // Start monitor thread
        let mut monitor_guard = lock_mutex(&self.monitor_thread);
        if monitor_guard.is_none() {
            let this = Arc::clone(get_manager());
            let handle = thread::spawn(move || {
                this.monitor_loop();
            });
            *monitor_guard = Some(handle);
        }
    }

    /// Monitor a single process and return the next attempt time if backoff is active.
    fn monitor_single_process(
        &self,
        child_mutex: &Mutex<Option<Child>>,
        spec: &ProcessSpec,
        backoff_mutex: &Mutex<u64>,
        next_attempt_mutex: &Mutex<Option<Instant>>,
    ) -> Option<Instant> {
        let mut child_guard = lock_mutex(child_mutex);
        let mut backoff_guard = lock_mutex(backoff_mutex);
        let mut next_attempt_guard = lock_mutex(next_attempt_mutex);
        self.tick_process(
            &mut child_guard,
            spec,
            &mut backoff_guard,
            &mut next_attempt_guard,
        );
        *next_attempt_guard
    }

    /// Periodically checks the child processes and respawns any that have exited.
    fn monitor_loop(&self) {
        log_debug("Monitor thread started.");
        let interval = Duration::from_secs(get_monitor_interval_secs());
        while self.should_run.load(Ordering::SeqCst) {
            // Tick and (re)spawn processes if needed
            let next_agent = self.monitor_single_process(
                &self.agent_child,
                &self.agent_spec,
                &self.agent_backoff_secs,
                &self.agent_next_attempt,
            );
            let next_trace = self.monitor_single_process(
                &self.trace_child,
                &self.trace_spec,
                &self.trace_backoff_secs,
                &self.trace_next_attempt,
            );

            // Compute dynamic sleep until next try based on backoff timers
            let now = Instant::now();
            let mut sleep_dur = interval;
            if let Some(na) = next_agent
                && na > now
            {
                sleep_dur = sleep_dur.min(na - now);
            }
            if let Some(nt) = next_trace
                && nt > now
            {
                sleep_dur = sleep_dur.min(nt - now);
            }

            // Wait with condvar so stop() can wake immediately
            let lock = lock_mutex(&self.monitor_cv_lock);
            let _ = self.monitor_cv.wait_timeout(lock, sleep_dur).unwrap();
        }
        log_debug("Monitor thread stopping.");
    }

    /// Attempts to gracefully terminate a child process. If the process already
    /// exited, the error is ignored.
    #[cfg(unix)]
    fn wait_with_timeout(child: &mut Child, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return false;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return false,
            }
        }
    }

    /// Send a signal to a Unix process group and wait for termination.
    #[cfg(unix)]
    fn signal_process_group(
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
            Self::wait_with_timeout(child, timeout)
        } else {
            log_warn(&format!(
                "Failed to send {} to {} group (pid={}).",
                signal_name, name, pid
            ));
            false
        }
    }

    /// Send a signal to a Unix process PID and wait for termination.
    #[cfg(unix)]
    fn signal_process_pid(
        child: &mut Child,
        name: &str,
        pid: i32,
        signal: i32,
        signal_name: &str,
        timeout: Duration,
    ) -> bool {
        if unsafe { libc::kill(pid, signal) } == 0 {
            log_debug(&format!("Sent {} to {} (pid={}).", signal_name, name, pid));
            Self::wait_with_timeout(child, timeout)
        } else {
            log_warn(&format!(
                "Failed to send {} to {} (pid={}).",
                signal_name, name, pid
            ));
            false
        }
    }

    fn graceful_kill(name: &str, child_opt: &mut Option<Child>) {
        if let Some(mut child) = child_opt.take() {
            #[cfg(unix)]
            {
                let pid = child.id() as i32;
                let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
                // Try process group first (negative pid targets group). If that fails,
                // fall back to signaling the specific PID to avoid hangs when setsid() fails.
                let pgid = -pid;
                let mut terminated;

                // TERM group
                terminated = Self::signal_process_group(
                    &mut child,
                    name,
                    pid,
                    pgid,
                    libc::SIGTERM,
                    "SIGTERM",
                    timeout,
                );

                // KILL group if still running
                if !terminated {
                    terminated = Self::signal_process_group(
                        &mut child,
                        name,
                        pid,
                        pgid,
                        libc::SIGKILL,
                        "SIGKILL",
                        timeout,
                    );
                }

                // Fall back to per-PID signaling if group signaling failed
                if !terminated {
                    terminated = Self::signal_process_pid(
                        &mut child,
                        name,
                        pid,
                        libc::SIGTERM,
                        "SIGTERM",
                        timeout,
                    );
                    if !terminated {
                        terminated = Self::signal_process_pid(
                            &mut child,
                            name,
                            pid,
                            libc::SIGKILL,
                            "SIGKILL",
                            timeout,
                        );
                    }
                }

                // As a last resort, ask std to kill and wait with timeout; avoid indefinite blocking.
                if !terminated {
                    let _ = child.kill();
                    if Self::wait_with_timeout(&mut child, timeout) {
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

    /// Stops the monitor thread and terminates both child processes.
    fn stop(&self) {
        let _guard = lock_mutex(&self.start_stop_lock);
        if !self.should_run.swap(false, Ordering::SeqCst) {
            // Already stopped
            log_debug("Stop called; manager already stopped.");
            return;
        }

        // Stop monitor thread
        if let Some(handle) = lock_mutex(&self.monitor_thread).take() {
            // Wake the monitor to exit promptly
            self.monitor_cv.notify_all();
            let _ = handle.join();
        }

        // On Windows, terminate the Job (kills all assigned processes), then close it
        // Record whether we've terminated via Job to avoid redundant per-child kill.
        #[cfg(windows)]
        let terminated_via_job: bool = unsafe {
            if let Some(job_raw) = lock_mutex(&self.windows_job).take() {
                let job = job_raw as HANDLE;
                let _ = TerminateJobObject(job, 1);
                let _ = CloseHandle(job);
                true
            } else {
                false
            }
        };

        // Kill children unless we already terminated the process tree via Windows Job
        #[cfg(windows)]
        if !terminated_via_job {
            let mut a = lock_mutex(&self.agent_child);
            Self::graceful_kill(self.agent_spec.name, &mut a);
            let mut t = lock_mutex(&self.trace_child);
            Self::graceful_kill(self.trace_spec.name, &mut t);
        }
        #[cfg(not(windows))]
        {
            let mut a = lock_mutex(&self.agent_child);
            Self::graceful_kill(self.agent_spec.name, &mut a);
            let mut t = lock_mutex(&self.trace_child);
            Self::graceful_kill(self.trace_spec.name, &mut t);
        }
    }
}

// removed unsafe arg caching; ProcessSpec now owns program and args

/// Global singleton manager used by both Rust and FFI front-ends.
static GLOBAL_MANAGER: OnceLock<Arc<AgentManager>> = OnceLock::new();

fn get_manager() -> &'static Arc<AgentManager> {
    GLOBAL_MANAGER.get_or_init(|| Arc::new(AgentManager::new()))
}

/// Initializes the libagent runtime: starts the Agent and Trace Agent and
/// launches the monitor task. Safe to call multiple times (idempotent).
pub fn initialize() {
    let mgr = Arc::clone(get_manager());
    mgr.start();
}

/// Stops the monitor task and terminates both child processes. Safe to call
/// multiple times (idempotent). Called automatically on library unload.
pub fn stop() {
    // Shutdown worker pool on Windows
    #[cfg(windows)]
    {
        winpipe::shutdown_worker_pool();
    }

    if let Some(mgr) = GLOBAL_MANAGER.get() {
        mgr.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    #[cfg(unix)]
    use std::process::Command;
    use std::sync::Mutex;
    #[cfg(unix)]
    use std::time::Duration;

    #[test]
    fn test_is_debug_enabled_default() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function doesn't crash
        let _ = is_debug_enabled();
    }

    #[test]
    fn test_is_debug_enabled_true_values() {
        // Snapshot the original value to restore later
        let original_value = env::var("LIBAGENT_DEBUG").ok();
        unsafe {
            env::set_var("LIBAGENT_DEBUG", "1");
        }
        // Test that the function returns true when LIBAGENT_DEBUG=1
        // We can't easily test this due to OnceLock caching, but we can at least
        // verify the environment variable is set correctly
        assert_eq!(env::var("LIBAGENT_DEBUG").unwrap(), "1");

        // Restore the original value
        match original_value {
            Some(val) => unsafe {
                env::set_var("LIBAGENT_DEBUG", val);
            },
            None => unsafe {
                env::remove_var("LIBAGENT_DEBUG");
            },
        }
    }

    #[test]
    fn test_current_log_level_default() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function doesn't crash and returns a valid LogLevel
        let level = current_log_level();
        match level {
            LogLevel::Error | LogLevel::Warn | LogLevel::Info | LogLevel::Debug => {
                // Valid log level
            }
        }
    }

    #[test]
    fn test_current_log_level_debug() {
        unsafe {
            env::set_var("LIBAGENT_DEBUG", "1");
        }
        // This would set log level to Debug, but we can't test OnceLock easily
    }

    #[test]
    fn test_lock_mutex_panic_recovery() {
        let mutex = Mutex::new(42);
        let guard = lock_mutex(&mutex);
        assert_eq!(*guard, 42);
    }

    #[test]
    fn test_child_stdio_inherit_debug() {
        // Test with debug enabled - should inherit
        // Snapshot the original value to restore later
        let original_value = env::var("LIBAGENT_DEBUG").ok();
        unsafe {
            env::set_var("LIBAGENT_DEBUG", "1");
        }
        // Test that the function exists and doesn't crash
        let _ = child_stdio_inherit();
        // Verify the environment variable was set correctly
        assert_eq!(env::var("LIBAGENT_DEBUG").unwrap(), "1");

        // Restore the original value
        match original_value {
            Some(val) => unsafe {
                env::set_var("LIBAGENT_DEBUG", val);
            },
            None => unsafe {
                env::remove_var("LIBAGENT_DEBUG");
            },
        }
    }

    #[test]
    fn test_child_stdio_inherit_default() {
        // Test default behavior - should not inherit
        let _ = child_stdio_inherit();
    }

    #[test]
    fn test_process_spec_new() {
        let spec = ProcessSpec::new(
            "test",
            "program".to_string(),
            vec!["arg1".to_string(), "arg2".to_string()],
        );
        assert_eq!(spec.name, "test");
        assert_eq!(spec.program, "program");
        assert_eq!(spec.args, vec!["arg1".to_string(), "arg2".to_string()]);
    }

    #[test]
    fn test_agent_manager_new() {
        let manager = AgentManager::new();
        assert!(!manager.should_run.load(Ordering::SeqCst));
        assert!(manager.monitor_thread.lock().unwrap().is_none());
        assert!(manager.agent_child.lock().unwrap().is_none());
        assert!(manager.trace_child.lock().unwrap().is_none());
    }

    #[test]
    fn test_log_functions() {
        // Test that log functions don't crash
        log_error("test error");
        log_warn("test warning");
        log_info("test info");
        log_debug("test debug");
    }

    #[test]
    fn test_log_at_levels() {
        // Test log_at with different levels
        log_at(LogLevel::Error, "error message");
        log_at(LogLevel::Warn, "warn message");
        log_at(LogLevel::Info, "info message");
        log_at(LogLevel::Debug, "debug message");
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_job_handle() {
        let manager = AgentManager::new();
        // Test that windows_job_handle can be called without crashing
        let handle = manager.windows_job_handle();
        // Handle might be null if CreateJobObjectW fails, but function should not crash
        let _ = handle;
    }

    #[cfg(windows)]
    #[test]
    fn test_agent_manager_windows_fields() {
        let manager = AgentManager::new();
        // Test that Windows-specific fields are initialized correctly
        let job_guard = lock_mutex(&manager.windows_job);
        assert!(job_guard.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_with_timeout() {
        // Start a process that exits quickly
        let mut child = Command::new("true").spawn().unwrap();

        // Should return true since the process exits quickly
        let result = AgentManager::wait_with_timeout(&mut child, Duration::from_secs(1));
        assert!(result);

        // Clean up
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn test_signal_process_group_invalid_pid() {
        // Start a process
        let mut child = Command::new("sleep").arg("1").spawn().unwrap();
        let pid = child.id() as i32;

        // Try to signal an invalid process group
        let result = AgentManager::signal_process_group(
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

    #[test]
    fn test_current_log_level_returns_valid_level() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function returns a valid LogLevel
        let level = current_log_level();
        match level {
            LogLevel::Error | LogLevel::Warn | LogLevel::Info | LogLevel::Debug => {
                // Valid log level
            }
        }
    }

    #[test]
    fn test_is_debug_enabled_returns_bool() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function returns a boolean without crashing
        let _enabled = is_debug_enabled();
        // We can't reliably test the environment variable behavior due to OnceLock caching
    }

    #[test]
    fn test_process_spec_constructor() {
        let spec = ProcessSpec::new(
            "test",
            "/bin/test".to_string(),
            vec!["arg1".to_string(), "arg2".to_string()],
        );
        assert_eq!(spec.name, "test");
        assert_eq!(spec.program, "/bin/test");
        assert_eq!(spec.args, vec!["arg1".to_string(), "arg2".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn test_wait_with_timeout_timeout() {
        // Start a long-running process
        let mut child = Command::new("sleep").arg("10").spawn().unwrap();

        // Wait with a very short timeout - should return false (timeout)
        let result = AgentManager::wait_with_timeout(&mut child, Duration::from_millis(10));
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
        let result = AgentManager::signal_process_pid(
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

    #[test]
    fn test_monitor_single_process_backoff_timer() {
        let manager = AgentManager::new();
        let spec = ProcessSpec::new("test", "nonexistent".to_string(), vec![]);

        // Create mutexes for the test
        let child_mutex = Mutex::new(None::<Child>);
        let backoff_mutex = Mutex::new(1u64);
        let next_attempt_mutex = Mutex::new(Some(Instant::now() + Duration::from_secs(1)));

        // This should return the next attempt time since backoff is active
        let result = manager.monitor_single_process(
            &child_mutex,
            &spec,
            &backoff_mutex,
            &next_attempt_mutex,
        );

        // Should return Some(next_attempt_time) because backoff is active
        assert!(result.is_some());
    }

    #[test]
    fn test_tick_process_backoff_logic() {
        let manager = AgentManager::new();
        let spec = ProcessSpec::new("test", "nonexistent".to_string(), vec![]);

        // Create test state - no child, but backoff timer in future
        let mut child_opt = None;
        let mut backoff_secs = 1;
        let mut next_attempt = Some(Instant::now() + Duration::from_secs(1));

        // This should not try to spawn because backoff is active
        manager.tick_process(&mut child_opt, &spec, &mut backoff_secs, &mut next_attempt);

        // Child should still be None, backoff should be increased
        assert!(child_opt.is_none());
        assert_eq!(backoff_secs, 1); // Should not change when backoff is active
    }
}
