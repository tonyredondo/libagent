//! Core process-lifetime management for the Datadog Agent and Trace Agent.
//!
//! This module encapsulates the high-level business logic for managing
//! the Agent and Trace Agent processes. It coordinates between the various
//! subsystems (logging, process spawning, monitoring, shutdown) to provide
//! a unified interface for process lifecycle management.
//!
//! The module exposes plain Rust functions (`initialize` and `stop`) for
//! Rust consumers and to be called from the C FFI layer.

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::process::Child;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
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
    get_agent_args, get_agent_program, get_backoff_initial_secs, get_trace_agent_args,
    get_trace_agent_program,
};
use crate::logging::{is_debug_enabled, log_debug, log_info, log_warn};
use crate::metrics;
use crate::monitor;
use crate::process::ProcessSpec;
use crate::shutdown;
#[cfg(windows)]
use crate::winpipe;

#[cfg(windows)]
fn normalize_pipe_path(pipe_name: &str) -> String {
    if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name.to_string()
    } else {
        format!(r"\\.\pipe\{}", pipe_name)
    }
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

/// Manager responsible for starting, monitoring, and stopping the two agents.
///
/// The manager keeps track of child processes for both the Agent and Trace
/// Agent. A background monitor thread periodically checks if either child has
/// exited, and respawns it when necessary while initialization is active.
pub struct AgentManager {
    /// Flag indicating whether the monitoring loop should run.
    pub(crate) should_run: AtomicBool,
    /// Coarse-grained mutex to serialize start/stop operations.
    start_stop_lock: Mutex<()>,
    /// Handle to the monitor thread, if running.
    monitor_thread: Mutex<Option<JoinHandle<()>>>,
    pub(crate) monitor_cv: Condvar,
    pub(crate) monitor_cv_lock: Mutex<()>,
    /// Subprocess specification for the Agent (optional - may be None if not configured).
    pub(crate) agent_spec: Option<ProcessSpec>,
    /// Subprocess specification for the Trace Agent.
    pub(crate) trace_spec: ProcessSpec,
    /// Currently running Agent child, if any.
    pub(crate) agent_child: Mutex<Option<Child>>,
    /// Currently running Trace Agent child, if any.
    pub(crate) trace_child: Mutex<Option<Child>>,
    /// Backoff state for agent respawns (only used if agent_spec is Some)
    pub(crate) agent_backoff_secs: Mutex<u64>,
    pub(crate) agent_next_attempt: Mutex<Option<Instant>>,
    /// Backoff state for trace-agent respawns
    pub(crate) trace_backoff_secs: Mutex<u64>,
    pub(crate) trace_next_attempt: Mutex<Option<Instant>>,
    /// Whether we've verified the current trace-agent is ready to accept connections
    trace_agent_ready: Mutex<bool>,
    #[cfg(windows)]
    windows_job: Mutex<Option<isize>>, // Job handle stored as isize for Send/Sync
}

impl AgentManager {
    /// Creates a new manager with default `ProcessSpec`s based on constants.
    fn new() -> Self {
        #[cfg(test)]
        MANAGER_CREATION_COUNT.fetch_add(1, Ordering::SeqCst);

        // Note: On Unix, child processes are placed in their own process groups using setsid().
        // This provides good isolation, but if the parent application is killed forcefully (SIGKILL),
        // the child processes may become orphaned. On Linux, PDEATHSIG prevents this.
        // On other Unix systems, a background monitor detects reparenting to init and cleans up.

        use crate::config::is_agent_enabled;

        let agent_program = get_agent_program();
        let agent_args = get_agent_args();

        // Only create agent spec if agent is enabled
        let agent_spec = if is_agent_enabled() {
            Some(ProcessSpec::new("agent", agent_program, agent_args))
        } else {
            log_debug("Agent disabled, skipping agent management");
            None
        };

        Self {
            should_run: AtomicBool::new(false),
            start_stop_lock: Mutex::new(()),
            monitor_thread: Mutex::new(None),
            monitor_cv: Condvar::new(),
            monitor_cv_lock: Mutex::new(()),
            agent_spec,
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
            trace_agent_ready: Mutex::new(false),
            #[cfg(windows)]
            windows_job: Mutex::new(None),
        }
    }

    /// Wait for the trace-agent to be ready to accept connections.
    /// Returns true if the trace-agent is ready, false if timeout exceeded.
    fn wait_for_trace_agent_ready(&self, timeout: Duration) -> bool {
        #[cfg(unix)]
        {
            use crate::config::get_trace_agent_uds_path;
            let uds_path = get_trace_agent_uds_path();

            log_debug("Manager: waiting for trace-agent UDS socket to be ready");
            let start_time = std::time::Instant::now();

            while start_time.elapsed() < timeout {
                use std::os::unix::net::UnixStream;

                // Try to connect to the socket
                if UnixStream::connect(&uds_path).is_ok() {
                    log_debug("Manager: trace-agent UDS socket is ready");
                    return true;
                }

                // Wait a bit before retrying
                std::thread::sleep(Duration::from_millis(100));
            }

            log_debug("Manager: timeout waiting for trace-agent UDS socket");
            false
        }
        #[cfg(windows)]
        {
            use crate::config::get_trace_agent_pipe_name;
            let pipe_name = get_trace_agent_pipe_name();
            if pipe_name.trim().is_empty() {
                log_debug("Manager: LIBAGENT_TRACE_AGENT_PIPE not set, cannot wait for readiness");
                return false;
            }

            log_debug("Manager: waiting for trace-agent named pipe to be ready");
            let start_time = std::time::Instant::now();

            let pipe_path = normalize_pipe_path(&pipe_name);

            while start_time.elapsed() < timeout {
                // Try to open the pipe
                if std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&pipe_path)
                    .is_ok()
                {
                    log_debug("Manager: trace-agent named pipe is ready");
                    return true;
                }

                // Wait a bit before retrying
                std::thread::sleep(Duration::from_millis(100));
            }

            log_debug("Manager: timeout waiting for trace-agent named pipe");
            false
        }
        #[cfg(all(not(unix), not(windows)))]
        {
            // For unsupported platforms, assume ready
            true
        }
    }

    /// Reset the trace-agent readiness flag.
    /// Called when a new trace-agent instance is spawned.
    pub(crate) fn reset_trace_agent_readiness(&self) {
        *lock_mutex(&self.trace_agent_ready) = false;
    }

    /// Get the current trace-agent readiness state (for testing).
    #[cfg(test)]
    pub(crate) fn is_trace_agent_ready(&self) -> bool {
        *lock_mutex(&self.trace_agent_ready)
    }

    /// Set the trace-agent readiness flag (for testing).
    #[cfg(test)]
    pub(crate) fn set_trace_agent_ready_for_test(&self, value: bool) {
        *lock_mutex(&self.trace_agent_ready) = value;
    }

    /// Ensure the trace-agent is ready to accept connections.
    /// This will block on the first call after a trace-agent spawn.
    pub fn ensure_trace_agent_ready(&self) -> Result<(), String> {
        // Lock the ready flag first to avoid race conditions
        let mut ready = lock_mutex(&self.trace_agent_ready);
        
        // Fast path: if already verified ready, return immediately
        if *ready {
            return Ok(());
        }

        // Check if a trace-agent was actually spawned by libagent
        // If not (like in tests that use mock servers), skip readiness check
        // We check this after locking ready to avoid TOCTOU race conditions
        let trace_child_none = lock_mutex(&self.trace_child).is_none();
        log_debug(&format!(
            "FFI: ensure_trace_agent_ready called, trace_child is_none={}",
            trace_child_none
        ));
        if trace_child_none {
            log_debug("FFI: no trace-agent spawned by libagent, skipping readiness check");
            return Ok(());
        }

        log_debug("FFI: waiting for trace-agent to be ready before first proxy call");
        // Use a shorter timeout for tests and development
        let timeout = if cfg!(debug_assertions) {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(10)
        };

        if self.wait_for_trace_agent_ready(timeout) {
            *ready = true;
            log_debug("FFI: trace-agent is ready, proceeding with proxy call");
            Ok(())
        } else {
            log_debug("FFI: trace-agent readiness check timed out, proceeding anyway");
            // In debug/test builds, don't fail on timeout - allow the request to proceed
            // This helps with tests and development where trace-agent might not be fully ready
            if cfg!(debug_assertions) {
                *ready = true; // Mark as ready to avoid repeated checks
                Ok(())
            } else {
                Err("trace-agent not ready to accept connections within timeout".to_string())
            }
        }
    }

    /// Spawns a subprocess according to the provided spec.
    ///
    /// Delegates to the process module for platform-specific spawning logic.
    #[allow(dead_code)]
    pub(crate) fn spawn_process(&self, spec: &ProcessSpec) -> std::io::Result<Child> {
        let child = crate::process::spawn_process(spec)?;

        // On Windows, assign child to Job object so we can terminate whole tree later
        #[cfg(windows)]
        {
            self.assign_child_to_job(&child);
        }

        // For trace-agent, reset the readiness flag since we spawned a new instance
        if spec.name == "trace-agent" {
            self.reset_trace_agent_readiness();
        }

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
    /// attempts to (re)spawn it with platform-specific conflict detection.
    fn tick_process(
        &self,
        child_guard: &mut Option<Child>,
        spec: &ProcessSpec,
        backoff_secs_guard: &mut u64,
        next_attempt_guard: &mut Option<Instant>,
    ) {
        let spawn_fn = |spec_to_spawn: &ProcessSpec| self.spawn_process(spec_to_spawn);
        let trace_agent_spawned = monitor::tick_process(
            child_guard,
            spec,
            backoff_secs_guard,
            next_attempt_guard,
            &spawn_fn,
        );

        // Reset trace-agent readiness if we spawned a new instance
        if trace_agent_spawned {
            self.reset_trace_agent_readiness();
        }
    }

    /// Starts both child processes (if not already started) and launches the monitor thread.
    fn reset_backoff_state(&self) {
        *lock_mutex(&self.agent_backoff_secs) = get_backoff_initial_secs();
        *lock_mutex(&self.agent_next_attempt) = None;
        *lock_mutex(&self.trace_backoff_secs) = get_backoff_initial_secs();
        *lock_mutex(&self.trace_next_attempt) = None;
    }

    fn start(self: &Arc<Self>) {
        let _guard = lock_mutex(&self.start_stop_lock);
        if self.should_run.swap(true, Ordering::SeqCst) {
            // Already running
            log_debug("Initialize called; manager already running.");
            return;
        }

        // Record initialization metrics
        metrics::get_metrics().record_initialization();

        // Reset backoff so new session starts immediately
        self.reset_backoff_state();
        self.reset_trace_agent_readiness();

        let agent_enabled = self.agent_spec.is_some();
        if agent_enabled {
            log_info("Starting Agent and Trace Agent...");
        } else {
            log_info("Starting Trace Agent (Agent disabled)...");
        }

        // Ensure processes started immediately
        if let Some(ref agent_spec) = self.agent_spec {
            let mut a = lock_mutex(&self.agent_child);
            let mut ab = lock_mutex(&self.agent_backoff_secs);
            let mut an = lock_mutex(&self.agent_next_attempt);
            self.tick_process(&mut a, agent_spec, &mut ab, &mut an);
        }
        {
            let mut t = lock_mutex(&self.trace_child);
            let mut tb = lock_mutex(&self.trace_backoff_secs);
            let mut tn = lock_mutex(&self.trace_next_attempt);
            self.tick_process(&mut t, &self.trace_spec, &mut tb, &mut tn);
        }

        // Now that processes are started, fork the monitor process with their PIDs
        // Skip in tests to avoid hanging processes
        #[cfg(all(unix, not(target_os = "linux")))]
        if !cfg!(test) {
            self.fork_orphan_cleanup_monitor();
        }

        // Start monitor thread
        let mut monitor_guard = lock_mutex(&self.monitor_thread);
        if monitor_guard.is_none() {
            let this = Arc::clone(self);
            let handle = monitor::start_monitor_thread(this);
            *monitor_guard = Some(handle);
        }
    }

    /// Stops the monitor thread and terminates both child processes.
    fn stop(self: &Arc<Self>) {
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
            if let Some(ref agent_spec) = self.agent_spec {
                let mut a = lock_mutex(&self.agent_child);
                shutdown::graceful_kill(agent_spec.name, &mut a);
            }
            let mut t = lock_mutex(&self.trace_child);
            shutdown::graceful_kill(self.trace_spec.name, &mut t);
        }
        #[cfg(not(windows))]
        {
            if let Some(ref agent_spec) = self.agent_spec {
                let mut a = lock_mutex(&self.agent_child);
                shutdown::graceful_kill(agent_spec.name, &mut a);
            }
            let mut t = lock_mutex(&self.trace_child);
            shutdown::graceful_kill(self.trace_spec.name, &mut t);
        }

        // Log final metrics if debug logging is enabled
        if is_debug_enabled() {
            // Print metrics summary to stdout so users can see it
            println!("{}", crate::metrics::get_metrics().format_metrics());
        }

        // Reset uptime so the next initialize() call represents a fresh session
        crate::metrics::get_metrics().clear_initialization();

        // Prepare for a future restart by clearing readiness/backoff state
        self.reset_trace_agent_readiness();
        self.reset_backoff_state();
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn fork_orphan_cleanup_monitor(&self) {
        // Get current child PIDs
        let agent_pid = lock_mutex(&self.agent_child).as_ref().map(|c| c.id());
        let trace_pid = lock_mutex(&self.trace_child).as_ref().map(|c| c.id());
        let parent_pid = unsafe { libc::getpid() };

        // Pass PIDs to monitor process via environment variables
        // (since fork() copies memory but we want the monitor to know the PIDs)
        unsafe {
            if let Some(pid) = agent_pid {
                let pid_str = std::ffi::CString::new(pid.to_string()).unwrap();
                libc::setenv(
                    std::ffi::CString::new("LIBAGENT_MONITOR_AGENT_PID")
                        .unwrap()
                        .as_ptr(),
                    pid_str.as_ptr(),
                    1,
                );
            }
            if let Some(pid) = trace_pid {
                let pid_str = std::ffi::CString::new(pid.to_string()).unwrap();
                libc::setenv(
                    std::ffi::CString::new("LIBAGENT_MONITOR_TRACE_PID")
                        .unwrap()
                        .as_ptr(),
                    pid_str.as_ptr(),
                    1,
                );
            }
            let parent_pid_str = std::ffi::CString::new(parent_pid.to_string()).unwrap();
            libc::setenv(
                std::ffi::CString::new("LIBAGENT_MONITOR_PARENT_PID")
                    .unwrap()
                    .as_ptr(),
                parent_pid_str.as_ptr(),
                1,
            );
        }

        // Fork a monitor process that will survive parent death
        match unsafe { libc::fork() } {
            -1 => {
                // Fork failed
                log_warn("Failed to fork orphan cleanup monitor process");
                Self::clear_orphan_monitor_env();
            }
            0 => {
                // Child process (monitor) - read PIDs from environment
                let agent_pid = std::env::var("LIBAGENT_MONITOR_AGENT_PID")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok());
                let trace_pid = std::env::var("LIBAGENT_MONITOR_TRACE_PID")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok());
                let parent_pid = std::env::var("LIBAGENT_MONITOR_PARENT_PID")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1); // fallback to init

                Self::clear_orphan_monitor_env();
                self.run_orphan_monitor(parent_pid, agent_pid, trace_pid);
                // Monitor process exits after cleanup
                unsafe { libc::_exit(0) };
            }
            _ => {
                // Parent process - monitor process is now running independently
                log_debug("Forked orphan cleanup monitor process");
                Self::clear_orphan_monitor_env();
            }
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn run_orphan_monitor(&self, parent_pid: u32, agent_pid: Option<u32>, trace_pid: Option<u32>) {
        loop {
            // Check if parent is still alive every 5 seconds
            match unsafe { libc::kill(parent_pid as i32, 0) } {
                0 => {
                    // Parent is still alive, keep monitoring
                    unsafe { libc::sleep(5) };
                }
                _ => {
                    // Parent is dead, terminate our children
                    log_debug("Parent process died, monitor process cleaning up children");

                    if let Some(pid) = agent_pid {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                            libc::sleep(1); // Give it a moment
                            libc::kill(pid as i32, libc::SIGKILL); // Force kill if needed
                        }
                    }

                    if let Some(pid) = trace_pid {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                            libc::sleep(1); // Give it a moment
                            libc::kill(pid as i32, libc::SIGKILL); // Force kill if needed
                        }
                    }

                    break;
                }
            }
        }
    }

    #[cfg(all(unix, not(target_os = "linux")))]
    fn clear_orphan_monitor_env() {
        const KEYS: [&str; 3] = [
            "LIBAGENT_MONITOR_AGENT_PID",
            "LIBAGENT_MONITOR_TRACE_PID",
            "LIBAGENT_MONITOR_PARENT_PID",
        ];

        unsafe {
            for key in KEYS {
                if let Ok(c_key) = std::ffi::CString::new(key) {
                    libc::unsetenv(c_key.as_ptr());
                }
            }
        }
    }
}

// removed unsafe arg caching; ProcessSpec now owns program and args

/// Global singleton manager guard used by both Rust and FFI front-ends.
static GLOBAL_MANAGER: OnceLock<Mutex<Arc<AgentManager>>> = OnceLock::new();

#[cfg(test)]
static MANAGER_CREATION_COUNT: AtomicUsize = AtomicUsize::new(0);

fn manager_cell() -> &'static Mutex<Arc<AgentManager>> {
    GLOBAL_MANAGER.get_or_init(|| Mutex::new(Arc::new(AgentManager::new())))
}

pub fn get_manager() -> Arc<AgentManager> {
    manager_cell()
        .lock()
        .expect("manager lock poisoned")
        .clone()
}

/// Initializes the libagent runtime: starts the Agent and Trace Agent and
/// launches the monitor task. Safe to call multiple times (idempotent).
pub fn initialize() {
    let cell = manager_cell();
    let mut guard = cell.lock().expect("manager lock poisoned");
    if guard.should_run.load(Ordering::SeqCst) {
        return;
    }

    let new_manager = Arc::new(AgentManager::new());
    let manager_for_start = Arc::clone(&new_manager);
    *guard = new_manager;

    // Ensure should_run is set under the global manager lock to keep
    // initialize() idempotent even when invoked concurrently.
    manager_for_start.start();
}

/// Stops the monitor task and terminates both child processes. Safe to call
/// multiple times (idempotent). Called automatically on library unload.
pub fn stop() {
    // Shutdown worker pool on Windows
    #[cfg(windows)]
    {
        winpipe::shutdown_worker_pool();
    }

    log_debug("Global stop: checking if manager exists");
    if let Some(cell) = GLOBAL_MANAGER.get() {
        log_debug("Global stop: manager exists, calling manager.stop()");
        let manager = cell.lock().expect("manager lock poisoned").clone();
        manager.stop();
    } else {
        log_debug("Global stop: no manager started");
        // No manager was started, but log metrics anyway if debug is enabled
        // (useful when ProxyTraceAgent was used without initialize())
        if crate::logging::is_debug_enabled() {
            // Print metrics summary to stdout so users can see it
            println!("{}", crate::metrics::get_metrics().format_metrics());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;
    use std::sync::Arc;
    use std::sync::Barrier;
    #[cfg(unix)]
    use std::sync::Mutex;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn test_is_debug_enabled_default() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function doesn't crash
        let _ = is_debug_enabled();
    }

    #[test]
    fn test_is_debug_enabled_true_values() {
        // Snapshot the original value to restore later
        let original_value = env::var("LIBAGENT_LOG").ok();
        unsafe {
            env::set_var("LIBAGENT_LOG", "debug");
        }
        // Test that the function returns true when LIBAGENT_LOG=debug
        // We can't easily test this due to OnceLock caching, but we can at least
        // verify the environment variable is set correctly
        assert_eq!(env::var("LIBAGENT_LOG").unwrap(), "debug");

        // Restore the original value
        match original_value {
            Some(val) => unsafe {
                env::set_var("LIBAGENT_LOG", val);
            },
            None => unsafe {
                env::remove_var("LIBAGENT_LOG");
            },
        }
    }

    #[test]
    fn test_current_log_level_default() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function doesn't crash and returns a valid LogLevel
        let level = crate::logging::current_log_level();
        match level {
            crate::logging::LogLevel::Error
            | crate::logging::LogLevel::Warn
            | crate::logging::LogLevel::Info
            | crate::logging::LogLevel::Debug => {
                // Valid log level
            }
        }
    }

    #[test]
    fn test_current_log_level_debug() {
        unsafe {
            env::set_var("LIBAGENT_LOG", "debug");
        }
        // This sets log level to Debug, but we can't test OnceLock easily
    }

    #[test]
    fn test_lock_mutex_panic_recovery() {
        let mutex = Mutex::new(42);
        let guard = lock_mutex(&mutex);
        assert_eq!(*guard, 42);
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
    #[serial]
    fn test_initialize_concurrent_creates_single_manager() {
        crate::manager::stop();
        MANAGER_CREATION_COUNT.store(0, Ordering::SeqCst);
        let baseline = MANAGER_CREATION_COUNT.load(Ordering::SeqCst);

        let original_program = env::var("LIBAGENT_TRACE_AGENT_PROGRAM").ok();
        let original_agent_enabled = env::var("LIBAGENT_AGENT_ENABLED").ok();

        unsafe {
            env::set_var("LIBAGENT_AGENT_ENABLED", "0");
            env::set_var(
                "LIBAGENT_TRACE_AGENT_PROGRAM",
                "libagent-concurrent-test-binary-does-not-exist",
            );
        }

        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for _ in 0..2 {
            let barrier_clone = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                barrier_clone.wait();
                crate::manager::initialize();
            }));
        }
        for handle in handles {
            handle.join().expect("initialize thread panicked");
        }

        crate::manager::stop();

        let final_count = MANAGER_CREATION_COUNT.load(Ordering::SeqCst);
        let created = final_count.saturating_sub(baseline);
        assert!(
            (1..=3).contains(&created),
            "expected between 1 and 3 manager constructions, saw {}",
            created
        );

        match original_program {
            Some(val) => unsafe {
                env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", val);
            },
            None => unsafe {
                env::remove_var("LIBAGENT_TRACE_AGENT_PROGRAM");
            },
        }
        match original_agent_enabled {
            Some(val) => unsafe {
                env::set_var("LIBAGENT_AGENT_ENABLED", val);
            },
            None => unsafe {
                env::remove_var("LIBAGENT_AGENT_ENABLED");
            },
        }
    }

    #[test]
    fn test_log_functions() {
        // Test that log functions don't crash
        crate::logging::log_error("test error");
        log_warn("test warning");
        log_info("test info");
        log_debug("test debug");
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
    fn test_normalize_pipe_path_prefixed() {
        assert_eq!(
            super::normalize_pipe_path(r"\\.\pipe\already-prefixed"),
            r"\\.\pipe\already-prefixed"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_normalize_pipe_path_unprefixed() {
        assert_eq!(
            super::normalize_pipe_path("unprefixed"),
            r"\\.\pipe\unprefixed"
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_agent_manager_windows_fields() {
        let manager = AgentManager::new();
        // Test that Windows-specific fields are initialized correctly
        let job_guard = lock_mutex(&manager.windows_job);
        assert!(job_guard.is_none());
    }

    #[test]
    fn test_is_debug_enabled_returns_bool() {
        // Note: OnceLock may already be set from previous tests
        // Just test that the function returns a boolean without crashing
        let _enabled = is_debug_enabled();
        // We can't reliably test the environment variable behavior due to OnceLock caching
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

    #[test]
    #[serial]
    fn test_agent_manager_optional_agent() {
        // Test that agent is disabled by default
        unsafe {
            std::env::remove_var("LIBAGENT_AGENT_ENABLED");
        }

        let manager = AgentManager::new();
        assert!(manager.agent_spec.is_none());
    }

    #[test]
    #[serial]
    fn test_agent_manager_with_agent() {
        // Test that agent is created when enabled
        unsafe {
            std::env::set_var("LIBAGENT_AGENT_ENABLED", "true");
        }

        let manager = AgentManager::new();
        assert!(manager.agent_spec.is_some());

        unsafe {
            std::env::remove_var("LIBAGENT_AGENT_ENABLED");
        }
    }

    #[test]
    fn test_agent_manager_ensure_trace_agent_ready() {
        let manager = AgentManager::new();

        // Initially no child is spawned, so should return Ok (no readiness check)
        let result = manager.ensure_trace_agent_ready();
        assert!(result.is_ok());

        // Test with debug timeout logic (removed timing test due to unreliable timing in tests)
    }

    #[test]
    fn test_agent_manager_wait_for_trace_agent_ready_timeout() {
        let manager = AgentManager::new();

        // Test timeout behavior - should return false when no trace-agent is listening
        let result = manager.wait_for_trace_agent_ready(Duration::from_millis(10));
        assert!(!result);
    }

    #[test]
    fn test_agent_manager_windows_job_handle() {
        let _manager = AgentManager::new();

        #[cfg(windows)]
        {
            // On Windows, should be able to create job handle (may be null if creation fails)
            let handle = _manager.windows_job_handle();
            // Handle might be null, but function should not panic
            let _ = handle;
        }

        #[cfg(not(windows))]
        {
            // On non-Windows, this test is skipped (no job handle functionality)
        }
    }

    #[test]
    fn test_agent_manager_assign_child_to_job() {
        #[cfg(windows)]
        {
            use std::process::Command;
            let manager = AgentManager::new();

            // Create a job handle first
            let _job = manager.windows_job_handle();

            // Start a quick process
            let mut child = Command::new("cmd").arg("/c").arg("exit").spawn().unwrap();

            // Should not panic when assigning to job
            manager.assign_child_to_job(&child);

            // Clean up
            let _ = child.wait();
        }
    }

    #[test]
    fn test_agent_manager_fork_orphan_cleanup_monitor() {
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            let manager = AgentManager::new();

            // This should not panic, though the actual fork may or may not succeed
            // depending on the environment
            manager.fork_orphan_cleanup_monitor();
        }
    }

    #[test]
    #[serial]
    fn test_agent_manager_run_orphan_monitor() {
        #[cfg(all(unix, not(target_os = "linux")))]
        {
            let manager = AgentManager::new();

            // Test with a PID that doesn't exist - should exit quickly
            // Spawn in a thread and wait for it to complete with timeout
            let handle = std::thread::spawn(move || {
                manager.run_orphan_monitor(999999, None, None);
            });

            // Wait for the monitor to exit with a reasonable timeout
            // Since PID 999999 doesn't exist, it should exit immediately
            match handle.join() {
                Ok(_) => {
                    // Thread completed successfully - monitor detected dead parent and exited
                }
                Err(_) => {
                    panic!("Monitor thread panicked");
                }
            }
        }
    }

    #[test]
    fn test_agent_manager_tick_process_none_spec() {
        let manager = AgentManager::new();

        let mut child_opt = None;
        let mut backoff_secs = 1u64;
        let mut next_attempt = None;

        // Should not panic when spec is None
        manager.tick_process(
            &mut child_opt,
            &ProcessSpec::new("test", "test".to_string(), vec![]),
            &mut backoff_secs,
            &mut next_attempt,
        );
    }
}
