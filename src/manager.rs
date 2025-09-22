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
    BACKOFF_INITIAL_SECS, BACKOFF_MAX_SECS, get_agent_args, get_agent_program,
    get_monitor_interval_secs, get_trace_agent_args, get_trace_agent_program,
};

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

fn child_stdio_inherit() -> bool {
    is_debug_enabled() || current_log_level() >= LogLevel::Debug
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
            agent_backoff_secs: Mutex::new(BACKOFF_INITIAL_SECS),
            agent_next_attempt: Mutex::new(None),
            trace_backoff_secs: Mutex::new(BACKOFF_INITIAL_SECS),
            trace_next_attempt: Mutex::new(None),
            #[cfg(windows)]
            windows_job: Mutex::new(None),
        }
    }

    /// Spawns a subprocess according to the provided spec.
    fn spawn_process(&self, spec: &ProcessSpec) -> std::io::Result<Child> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args).stdin(Stdio::null());

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
        let mut guard = self.windows_job.lock().unwrap();
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
                *backoff_secs_guard = BACKOFF_INITIAL_SECS;
                *next_attempt_guard = None;
            }
            Err(err) => {
                log_error(&format!(
                    "Failed to spawn {} (program='{}', args={:?}): {}. Backing off {}s.",
                    spec.name, spec.program, spec.args, err, *backoff_secs_guard
                ));
                let wait = Duration::from_secs(*backoff_secs_guard);
                *next_attempt_guard = Some(now + wait);
                *backoff_secs_guard = (*backoff_secs_guard)
                    .saturating_mul(2)
                    .min(BACKOFF_MAX_SECS);
            }
        }
    }

    /// Starts both child processes (if not already started) and launches the monitor thread.
    fn start(&self) {
        let _guard = self.start_stop_lock.lock().unwrap();
        if self.should_run.swap(true, Ordering::SeqCst) {
            // Already running
            log_debug("Initialize called; manager already running.");
            return;
        }

        log_info("Starting Agent and Trace Agent...");

        // Ensure processes started immediately
        {
            let mut a = self.agent_child.lock().unwrap();
            let mut ab = self.agent_backoff_secs.lock().unwrap();
            let mut an = self.agent_next_attempt.lock().unwrap();
            self.tick_process(&mut a, &self.agent_spec, &mut ab, &mut an);
        }
        {
            let mut t = self.trace_child.lock().unwrap();
            let mut tb = self.trace_backoff_secs.lock().unwrap();
            let mut tn = self.trace_next_attempt.lock().unwrap();
            self.tick_process(&mut t, &self.trace_spec, &mut tb, &mut tn);
        }

        // Start monitor thread
        let mut monitor_guard = self.monitor_thread.lock().unwrap();
        if monitor_guard.is_none() {
            let this = Arc::clone(
                GLOBAL_MANAGER
                    .get()
                    .expect("GLOBAL_MANAGER must be initialized before start"),
            );
            let handle = thread::spawn(move || {
                this.monitor_loop();
            });
            *monitor_guard = Some(handle);
        }
    }

    /// Periodically checks the child processes and respawns any that have exited.
    fn monitor_loop(&self) {
        log_debug("Monitor thread started.");
        let interval = Duration::from_secs(get_monitor_interval_secs());
        while self.should_run.load(Ordering::SeqCst) {
            // Tick and (re)spawn agent if needed
            {
                let mut a = self.agent_child.lock().unwrap();
                let mut ab = self.agent_backoff_secs.lock().unwrap();
                let mut an = self.agent_next_attempt.lock().unwrap();
                self.tick_process(&mut a, &self.agent_spec, &mut ab, &mut an);
            }

            // Tick and (re)spawn trace-agent if needed
            {
                let mut t = self.trace_child.lock().unwrap();
                let mut tb = self.trace_backoff_secs.lock().unwrap();
                let mut tn = self.trace_next_attempt.lock().unwrap();
                self.tick_process(&mut t, &self.trace_spec, &mut tb, &mut tn);
            }

            // Compute dynamic sleep until next try based on backoff timers
            let now = Instant::now();
            let next_agent = *self.agent_next_attempt.lock().unwrap();
            let next_trace = *self.trace_next_attempt.lock().unwrap();
            let mut sleep_dur = interval;
            match next_agent {
                Some(na) if na > now => {
                    sleep_dur = sleep_dur.min(na - now);
                }
                _ => {}
            }
            match next_trace {
                Some(nt) if nt > now => {
                    sleep_dur = sleep_dur.min(nt - now);
                }
                _ => {}
            }

            // Wait with condvar so stop() can wake immediately
            let lock = self.monitor_cv_lock.lock().unwrap();
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

    fn graceful_kill(name: &str, child_opt: &mut Option<Child>) {
        if let Some(mut child) = child_opt.take() {
            #[cfg(unix)]
            unsafe {
                let pid = child.id() as i32;
                let timeout = Duration::from_secs(GRACEFUL_SHUTDOWN_TIMEOUT_SECS);
                // Send SIGTERM to the process group (negative pid targets group)
                let pgid = -pid;
                if libc::kill(pgid, libc::SIGTERM) == -1 {
                    log_warn(&format!(
                        "Failed to send SIGTERM to {} group (pid={}).",
                        name, pid
                    ));
                } else {
                    log_debug(&format!("Sent SIGTERM to {} group (pid={}).", name, pid));
                }
                let terminated_cleanly = Self::wait_with_timeout(&mut child, timeout);
                if !terminated_cleanly {
                    // escalate
                    if libc::kill(pgid, libc::SIGKILL) == -1 {
                        log_warn(&format!(
                            "Failed to send SIGKILL to {} group (pid={}).",
                            name, pid
                        ));
                    } else {
                        log_debug(&format!("Sent SIGKILL to {} group (pid={}).", name, pid));
                    }
                    let _ = child.wait();
                }
            }

            #[cfg(not(unix))]
            {
                let _ = child.kill();
                let _ = child.wait();
            }

            log_info(&format!("{} terminated.", name));
        }
    }

    /// Stops the monitor thread and terminates both child processes.
    fn stop(&self) {
        let _guard = self.start_stop_lock.lock().unwrap();
        if !self.should_run.swap(false, Ordering::SeqCst) {
            // Already stopped
            log_debug("Stop called; manager already stopped.");
            return;
        }

        // Stop monitor thread
        if let Some(handle) = self.monitor_thread.lock().unwrap().take() {
            // Wake the monitor to exit promptly
            self.monitor_cv.notify_all();
            let _ = handle.join();
        }

        // On Windows, terminate the Job (kills all assigned processes), then close it
        // Record whether we've terminated via Job to avoid redundant per-child kill.
        #[cfg(windows)]
        let terminated_via_job: bool = unsafe {
            if let Some(job_raw) = self.windows_job.lock().unwrap().take() {
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
            let mut a = self.agent_child.lock().unwrap();
            Self::graceful_kill(self.agent_spec.name, &mut a);
            let mut t = self.trace_child.lock().unwrap();
            Self::graceful_kill(self.trace_spec.name, &mut t);
        }
        #[cfg(not(windows))]
        {
            let mut a = self.agent_child.lock().unwrap();
            Self::graceful_kill(self.agent_spec.name, &mut a);
            let mut t = self.trace_child.lock().unwrap();
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
    if let Some(mgr) = GLOBAL_MANAGER.get() {
        mgr.stop();
    }
}
