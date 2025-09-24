//! Internal metrics and observability for libagent.
//!
//! This module provides internal metrics tracking for monitoring the health
//! and performance of the agent management system.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global metrics instance.
static METRICS: Metrics = Metrics::new();

/// Internal metrics for observability.
#[derive(Debug)]
pub struct Metrics {
    /// Total number of agent process spawns (including restarts)
    agent_spawns: AtomicU64,
    /// Total number of trace-agent process spawns (including restarts)
    trace_agent_spawns: AtomicU64,
    /// Total number of agent process failures
    agent_failures: AtomicU64,
    /// Total number of trace-agent process failures
    trace_agent_failures: AtomicU64,
    /// Time when the manager was first initialized
    initialization_time: std::sync::OnceLock<Instant>,
}

impl Metrics {
    /// Creates a new metrics instance with zeroed counters.
    const fn new() -> Self {
        Self {
            agent_spawns: AtomicU64::new(0),
            trace_agent_spawns: AtomicU64::new(0),
            agent_failures: AtomicU64::new(0),
            trace_agent_failures: AtomicU64::new(0),
            initialization_time: std::sync::OnceLock::new(),
        }
    }

    /// Records that the agent process was spawned.
    pub fn record_agent_spawn(&self) {
        self.agent_spawns.fetch_add(1, Ordering::Relaxed);
    }

    /// Records that the trace-agent process was spawned.
    pub fn record_trace_agent_spawn(&self) {
        self.trace_agent_spawns.fetch_add(1, Ordering::Relaxed);
    }

    /// Records that the agent process failed.
    pub fn record_agent_failure(&self) {
        self.agent_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Records that the trace-agent process failed.
    pub fn record_trace_agent_failure(&self) {
        self.trace_agent_failures.fetch_add(1, Ordering::Relaxed);
    }

    /// Records the initialization time.
    pub fn record_initialization(&self) {
        let _ = self.initialization_time.set(Instant::now());
    }

    /// Gets the total number of agent spawns.
    pub fn agent_spawns(&self) -> u64 {
        self.agent_spawns.load(Ordering::Relaxed)
    }

    /// Gets the total number of trace-agent spawns.
    pub fn trace_agent_spawns(&self) -> u64 {
        self.trace_agent_spawns.load(Ordering::Relaxed)
    }

    /// Gets the total number of agent failures.
    pub fn agent_failures(&self) -> u64 {
        self.agent_failures.load(Ordering::Relaxed)
    }

    /// Gets the total number of trace-agent failures.
    pub fn trace_agent_failures(&self) -> u64 {
        self.trace_agent_failures.load(Ordering::Relaxed)
    }

    /// Gets the uptime since initialization, if initialized.
    pub fn uptime(&self) -> Option<Duration> {
        self.initialization_time.get().map(|start| start.elapsed())
    }

    /// Gets all metrics as a human-readable string.
    pub fn format_metrics(&self) -> String {
        let mut output = String::new();
        output.push_str("libagent metrics:\n");
        output.push_str(&format!("  agent_spawns: {}\n", self.agent_spawns()));
        output.push_str(&format!(
            "  trace_agent_spawns: {}\n",
            self.trace_agent_spawns()
        ));
        output.push_str(&format!("  agent_failures: {}\n", self.agent_failures()));
        output.push_str(&format!(
            "  trace_agent_failures: {}\n",
            self.trace_agent_failures()
        ));

        if let Some(uptime) = self.uptime() {
            output.push_str(&format!("  uptime_seconds: {:.2}\n", uptime.as_secs_f64()));
        } else {
            output.push_str("  uptime_seconds: not_initialized\n");
        }

        output
    }
}

/// Gets the global metrics instance.
pub fn get_metrics() -> &'static Metrics {
    &METRICS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_recording() {
        let metrics = Metrics::new();

        // Test initial state
        assert_eq!(metrics.agent_spawns(), 0);
        assert_eq!(metrics.trace_agent_spawns(), 0);
        assert_eq!(metrics.agent_failures(), 0);
        assert_eq!(metrics.trace_agent_failures(), 0);
        assert!(metrics.uptime().is_none());

        // Record initialization
        metrics.record_initialization();
        assert!(metrics.uptime().is_some());

        // Record spawns
        metrics.record_agent_spawn();
        metrics.record_agent_spawn();
        metrics.record_trace_agent_spawn();

        assert_eq!(metrics.agent_spawns(), 2);
        assert_eq!(metrics.trace_agent_spawns(), 1);

        // Record failures
        metrics.record_agent_failure();
        metrics.record_trace_agent_failure();
        metrics.record_trace_agent_failure();

        assert_eq!(metrics.agent_failures(), 1);
        assert_eq!(metrics.trace_agent_failures(), 2);

        // Test formatting
        let formatted = metrics.format_metrics();
        assert!(formatted.contains("agent_spawns: 2"));
        assert!(formatted.contains("trace_agent_spawns: 1"));
        assert!(formatted.contains("agent_failures: 1"));
        assert!(formatted.contains("trace_agent_failures: 2"));
        assert!(formatted.contains("uptime_seconds"));
    }

    #[test]
    fn test_global_metrics() {
        let metrics = get_metrics();

        // Test that we can call methods without panicking
        let _ = metrics.agent_spawns();
        let _ = metrics.trace_agent_spawns();
        let _ = metrics.agent_failures();
        let _ = metrics.trace_agent_failures();
        let _ = metrics.uptime();
        let _ = metrics.format_metrics();
    }
}
