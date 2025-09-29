//! Monitor thread implementation.
//!
//! This module handles the background monitoring logic for child processes,
//! including respawn logic with exponential backoff and IPC resource conflict detection.

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::config::get_trace_agent_pipe_name;
#[cfg(unix)]
use crate::config::get_trace_agent_uds_path;
use crate::config::{
    get_agent_remote_config_addr, get_backoff_initial_secs, get_backoff_max_secs,
    get_monitor_interval_secs, is_agent_enabled,
};
use crate::logging::{log_debug, log_error, log_warn};
use crate::metrics;
use crate::process::ProcessSpec;
use std::net::{TcpStream, ToSocketAddrs};

fn record_process_spawn(spec: &ProcessSpec) {
    match spec.name {
        "agent" => metrics::get_metrics().record_agent_spawn(),
        "trace-agent" => metrics::get_metrics().record_trace_agent_spawn(),
        _ => {}
    }
}

fn record_process_failure(spec: &ProcessSpec) {
    match spec.name {
        "agent" => metrics::get_metrics().record_agent_failure(),
        "trace-agent" => metrics::get_metrics().record_trace_agent_failure(),
        _ => {}
    }
}

/// Checks if a Unix Domain Socket is already in use by trying to connect to it.
/// Returns true if the socket exists and accepts connections (another process is listening).
#[cfg(unix)]
fn is_socket_in_use(socket_path: &Path) -> bool {
    use crate::logging::log_debug;

    // Try to connect to the socket - if successful, another process is listening
    match UnixStream::connect(socket_path) {
        Ok(_) => {
            log_debug(&format!(
                "Socket {} is already in use by another process",
                socket_path.display()
            ));
            true
        }
        Err(_) => {
            // Socket doesn't exist or isn't accepting connections
            false
        }
    }
}

/// Checks if a Windows Named Pipe is already in use.
/// For Windows, we check if the pipe exists by attempting to connect.
/// This is a best-effort check since Windows named pipes don't have a simple existence check.
#[cfg(windows)]
fn is_pipe_in_use(pipe_name: &str) -> bool {
    let pipe_path = if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name.to_string()
    } else {
        format!(r"\\.\pipe\{}", pipe_name)
    };

    // Try to open the pipe for reading/writing - if successful, another process has it open
    match OpenOptions::new().read(true).write(true).open(&pipe_path) {
        Ok(_) => {
            log_debug(&format!(
                "Pipe {} is already in use by another process",
                pipe_name
            ));
            true
        }
        Err(_) => {
            // Pipe doesn't exist or isn't accepting connections
            false
        }
    }
}

/// Checks if the remote configuration gRPC service is available on the main agent.
/// Returns true if there's already an agent running that provides remote config.
/// If true, we should skip spawning our own agent to avoid conflicts.
/// Remote config is only checked when agent is enabled.
fn is_remote_config_available() -> bool {
    // Only check remote config if agent is enabled
    if !is_agent_enabled() {
        log_debug("Agent disabled, skipping remote configuration check");
        return false;
    }

    use std::time::Duration;

    let Some(addr) = get_agent_remote_config_addr() else {
        log_debug("Remote configuration detection disabled via LIBAGENT_AGENT_REMOTE_CONFIG_ADDR");
        return false;
    };

    let addrs = match addr.to_socket_addrs() {
        Ok(addrs) => addrs.collect::<Vec<_>>(),
        Err(err) => {
            log_warn(&format!(
                "Failed to resolve remote configuration address '{}': {}",
                addr, err
            ));
            return false;
        }
    };

    if addrs.is_empty() {
        log_warn(&format!(
            "Remote configuration address '{}' resolved to no endpoints",
            addr
        ));
        return false;
    }

    for socket_addr in addrs {
        match TcpStream::connect_timeout(&socket_addr, Duration::from_millis(100)) {
            Ok(_) => {
                log_debug(&format!(
                    "Remote configuration service detected at {}",
                    socket_addr
                ));
                return true;
            }
            Err(err) => {
                log_debug(&format!(
                    "Remote configuration probe failed for {}: {}",
                    socket_addr, err
                ));
            }
        }
    }

    log_debug(&format!(
        "Remote configuration service not reachable at {}; will spawn our own agent",
        addr
    ));
    false
}

/// Ensures the child described by `spec` is running. If it has exited or was never started,
/// attempts to (re)spawn it with platform-specific conflict detection.
///
/// # Process Spawning Logic
///
/// The method implements smart spawning to prevent conflicts:
///
/// ## Trace Agent
/// - Checks if IPC socket/pipe is already in use before spawning
/// - Skips spawn if another process owns the IPC endpoint
/// - Uses exponential backoff for respawn attempts on failure
///
/// ## Agent
/// - Only spawns if agent is enabled AND no existing remote config service is detected
/// - Prevents multiple agent instances competing for resources
///
/// # Backoff Strategy
/// - Initial delay: `BACKOFF_INITIAL_SECS` (1 second)
/// - Exponential growth: doubles each failure up to `BACKOFF_MAX_SECS` (30 seconds)
/// - Resets to initial delay on successful spawn
///
/// # Returns
/// Returns `true` if a trace-agent was successfully spawned, `false` otherwise.
/// This allows the caller to reset readiness state when a new trace-agent instance starts.
pub fn tick_process(
    child_guard: &mut Option<std::process::Child>,
    spec: &ProcessSpec,
    backoff_secs_guard: &mut u64,
    next_attempt_guard: &mut Option<Instant>,
    spawn_fn: &dyn Fn(&ProcessSpec) -> std::io::Result<std::process::Child>,
) -> bool {
    // If child exists, see if it exited
    if let Some(child) = child_guard.as_mut() {
        match child.try_wait() {
            Ok(Some(status)) => {
                log_warn(&format!(
                    "{} exited with status {:?}. Will respawn.",
                    spec.name, status
                ));
                record_process_failure(spec);
                *child_guard = None;
            }
            Ok(None) => {
                return false; // still running
            }
            Err(err) => {
                log_warn(&format!(
                    "Failed to check {} status: {}. Treating as not running.",
                    spec.name, err
                ));
                record_process_failure(spec);
                *child_guard = None;
            }
        }
    }

    // Not running; check backoff window
    let now = Instant::now();
    if next_attempt_guard.as_ref().is_some_and(|&next| now < next) {
        return false;
    }

    // Check if we should spawn based on resource availability
    match spec.name {
        "trace-agent" => {
            #[cfg(unix)]
            {
                let socket_path = std::path::PathBuf::from(get_trace_agent_uds_path());
                if !socket_path.as_os_str().is_empty() && is_socket_in_use(&socket_path) {
                    log_debug(
                        "Skipping trace-agent spawn - socket already in use by another process",
                    );
                    return false;
                }
            }
            #[cfg(windows)]
            {
                let pipe_name = get_trace_agent_pipe_name();
                if !pipe_name.trim().is_empty() && is_pipe_in_use(&pipe_name) {
                    log_debug(
                        "Skipping trace-agent spawn - pipe already in use by another process",
                    );
                    return false;
                }
            }
        }
        "agent" => {
            if is_remote_config_available() {
                log_debug(
                    "Skipping agent spawn - remote configuration service already available from existing agent",
                );
                return false;
            }
        }
        _ => {}
    }

    // Try to spawn
    match spawn_fn(spec) {
        Ok(new_child) => {
            *child_guard = Some(new_child);
            *backoff_secs_guard = get_backoff_initial_secs();
            *next_attempt_guard = None;
            record_process_spawn(spec);

            // Return true if this is a trace-agent spawn
            spec.name == "trace-agent"
        }
        Err(err) => {
            log_error(&format!(
                "Failed to spawn {} (program='{}', args={:?}): {}. Backing off {}s.",
                spec.name, spec.program, spec.args, err, *backoff_secs_guard
            ));
            record_process_failure(spec);
            let wait = Duration::from_secs(*backoff_secs_guard);
            *next_attempt_guard = Some(now + wait);
            *backoff_secs_guard = (*backoff_secs_guard)
                .saturating_mul(2)
                .min(get_backoff_max_secs());
            false
        }
    }
}

/// Starts the monitor thread for process lifecycle management.
///
/// The monitor thread runs in the background and periodically checks child processes,
/// respawning them as needed with exponential backoff. It uses a condition variable
/// for efficient wake-up during shutdown.
///
/// # Monitor Loop Behavior
/// - Checks each process every `MONITOR_INTERVAL_SECS` (default 1 second)
/// - Uses dynamic sleep timing based on backoff deadlines to avoid delays
/// - Exits promptly when signaled via condition variable during shutdown
///
/// # Thread Safety
/// - Shares mutable state via Arc<Mutex<>> for thread-safe access
/// - Uses atomic bool for run state to avoid locks in hot path
pub fn start_monitor_thread(manager: Arc<crate::manager::AgentManager>) -> JoinHandle<()> {
    thread::spawn(move || {
        monitor_loop(&manager);
    })
}

/// The main monitor loop that runs in the background thread.
///
/// This function implements the core monitoring logic with dynamic timing
/// to balance responsiveness with CPU efficiency.
fn monitor_loop(manager: &crate::manager::AgentManager) {
    log_debug("Monitor thread started.");
    let interval = Duration::from_secs(get_monitor_interval_secs());
    let spawn_fn = |spec: &ProcessSpec| manager.spawn_process(spec);
    while manager.should_run.load(Ordering::SeqCst) {
        // Check and (re)spawn processes if needed
        {
            let mut agent_child = manager.agent_child.lock().unwrap();
            let mut agent_backoff = manager.agent_backoff_secs.lock().unwrap();
            let mut agent_next = manager.agent_next_attempt.lock().unwrap();

            if let Some(ref agent_spec) = manager.agent_spec {
                let _trace_agent_spawned = tick_process(
                    &mut agent_child,
                    agent_spec,
                    &mut agent_backoff,
                    &mut agent_next,
                    &spawn_fn,
                );
            }
        }

        {
            let mut trace_child = manager.trace_child.lock().unwrap();
            let mut trace_backoff = manager.trace_backoff_secs.lock().unwrap();
            let mut trace_next = manager.trace_next_attempt.lock().unwrap();

            let trace_agent_spawned = tick_process(
                &mut trace_child,
                &manager.trace_spec,
                &mut trace_backoff,
                &mut trace_next,
                &spawn_fn,
            );

            // Reset trace-agent readiness if we spawned a new instance
            if trace_agent_spawned {
                manager.reset_trace_agent_readiness();
            }
        }

        // Compute dynamic sleep until next try based on backoff timers
        let now = Instant::now();
        let mut sleep_dur = interval;

        // Check agent backoff timer
        if let Ok(agent_next_guard) = manager.agent_next_attempt.lock()
            && let Some(na) = *agent_next_guard
            && na > now
        {
            sleep_dur = sleep_dur.min(na - now);
        }

        // Check trace-agent backoff timer
        if let Ok(trace_next_guard) = manager.trace_next_attempt.lock()
            && let Some(nt) = *trace_next_guard
            && nt > now
        {
            sleep_dur = sleep_dur.min(nt - now);
        }

        // Wait with condvar so stop() can wake immediately
        let lock = manager.monitor_cv_lock.lock().unwrap();
        let _ = manager.monitor_cv.wait_timeout(lock, sleep_dur).unwrap();
    }
    log_debug("Monitor thread stopping.");
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn test_tick_process_respects_uds_override_conflict() {
        use std::os::unix::net::UnixListener;

        let socket_path = std::env::current_dir()
            .unwrap()
            .join(format!("libagent_conflict_{}.socket", std::process::id()));

        // Ensure clean state by removing any existing value
        unsafe {
            std::env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
        }

        unsafe {
            std::env::set_var("LIBAGENT_TRACE_AGENT_UDS", &socket_path);
        }

        // Verify the environment variable is set correctly
        assert_eq!(
            std::env::var("LIBAGENT_TRACE_AGENT_UDS").as_deref(),
            Ok(socket_path.to_string_lossy().as_ref())
        );

        let _ = std::fs::remove_file(&socket_path);
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!(
                    "Skipping test_tick_process_respects_uds_override_conflict: {}",
                    err
                );
                unsafe {
                    std::env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
                }
                return;
            }
            Err(err) => panic!("should bind override socket: {err}"),
        };
        let _listener = listener;

        if !super::is_socket_in_use(&socket_path) {
            eprintln!(
                "Skipping test_tick_process_respects_uds_override_conflict: cannot connect to {}",
                socket_path.display()
            );
            unsafe {
                std::env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
            }
            let _ = std::fs::remove_file(&socket_path);
            return;
        }

        let spec = crate::process::ProcessSpec::new(
            "trace-agent",
            "nonexistent-binary".to_string(),
            Vec::new(),
        );

        let mut child_guard = None;
        let mut backoff = super::get_backoff_initial_secs();
        let mut next_attempt = None;

        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let spawn_called = Arc::new(AtomicBool::new(false));
        let spawn_fn = {
            let spawn_called = Arc::clone(&spawn_called);
            move |_: &crate::process::ProcessSpec| -> std::io::Result<std::process::Child> {
                spawn_called.store(true, Ordering::SeqCst);
                Err(std::io::Error::other(
                    "spawn should not occur when socket is already in use",
                ))
            }
        };
        let result = super::tick_process(
            &mut child_guard,
            &spec,
            &mut backoff,
            &mut next_attempt,
            &spawn_fn,
        );

        assert!(
            !spawn_called.load(Ordering::SeqCst),
            "spawn should not occur when socket is already in use"
        );
        assert!(!result, "tick_process should indicate no spawn occurred");

        assert!(
            child_guard.is_none(),
            "should skip spawning when socket in use"
        );
        assert!(
            next_attempt.is_none(),
            "backoff timer should remain unchanged when spawn is skipped"
        );
        assert_eq!(
            backoff,
            super::get_backoff_initial_secs(),
            "backoff should not advance when spawn skipped"
        );

        unsafe {
            std::env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
        }

        let _ = std::fs::remove_file(&socket_path);
    }
}
