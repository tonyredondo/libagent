//! Internal metrics and observability for libagent.
//!
//! This module provides internal metrics tracking for monitoring the health
//! and performance of the agent management system.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global metrics instance.
static METRICS: std::sync::OnceLock<Metrics> = std::sync::OnceLock::new();

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
    initialization_time: Mutex<Option<Instant>>,

    /// HTTP proxy request metrics by method
    proxy_get_requests: AtomicU64,
    proxy_post_requests: AtomicU64,
    proxy_delete_requests: AtomicU64,
    proxy_patch_requests: AtomicU64,
    proxy_put_requests: AtomicU64,
    proxy_head_requests: AtomicU64,
    proxy_options_requests: AtomicU64,
    proxy_other_requests: AtomicU64,

    /// HTTP response status code metrics
    proxy_2xx_responses: AtomicU64,
    proxy_3xx_responses: AtomicU64,
    proxy_4xx_responses: AtomicU64,
    proxy_5xx_responses: AtomicU64,

    /// Response time tracking (moving averages in milliseconds)
    response_times: Mutex<ResponseTimeStats>,
}

/// Response time statistics with moving averages
#[derive(Debug)]
struct ResponseTimeStats {
    /// Exponential moving average of all response times (milliseconds)
    ema_all: f64,
    /// Exponential moving average for 2xx responses (milliseconds)
    ema_2xx: f64,
    /// Exponential moving average for 4xx responses (milliseconds)
    ema_4xx: f64,
    /// Exponential moving average for 5xx responses (milliseconds)
    ema_5xx: f64,
    /// Sample count for calculating averages
    sample_count: u64,
    /// Alpha for exponential moving average (0.1 = fast adaptation)
    alpha: f64,
}

impl ResponseTimeStats {
    fn new() -> Self {
        Self {
            ema_all: 0.0,
            ema_2xx: 0.0,
            ema_4xx: 0.0,
            ema_5xx: 0.0,
            sample_count: 0,
            alpha: 0.1, // Adapt quickly to changes
        }
    }

    fn update(&mut self, duration_ms: f64, status_code: u16) {
        // Update overall EMA
        if self.sample_count == 0 {
            self.ema_all = duration_ms;
        } else {
            self.ema_all = self.alpha * duration_ms + (1.0 - self.alpha) * self.ema_all;
        }

        // Update status-specific EMA
        if (200..300).contains(&status_code) {
            if self.ema_2xx == 0.0 {
                self.ema_2xx = duration_ms;
            } else {
                self.ema_2xx = self.alpha * duration_ms + (1.0 - self.alpha) * self.ema_2xx;
            }
        } else if (400..500).contains(&status_code) {
            if self.ema_4xx == 0.0 {
                self.ema_4xx = duration_ms;
            } else {
                self.ema_4xx = self.alpha * duration_ms + (1.0 - self.alpha) * self.ema_4xx;
            }
        } else if (500..600).contains(&status_code) {
            if self.ema_5xx == 0.0 {
                self.ema_5xx = duration_ms;
            } else {
                self.ema_5xx = self.alpha * duration_ms + (1.0 - self.alpha) * self.ema_5xx;
            }
        }

        self.sample_count += 1;
    }
}

impl Metrics {
    /// Creates a new metrics instance with zeroed counters.
    fn new() -> Self {
        Self {
            agent_spawns: AtomicU64::new(0),
            trace_agent_spawns: AtomicU64::new(0),
            agent_failures: AtomicU64::new(0),
            trace_agent_failures: AtomicU64::new(0),
            initialization_time: Mutex::new(None),
            proxy_get_requests: AtomicU64::new(0),
            proxy_post_requests: AtomicU64::new(0),
            proxy_delete_requests: AtomicU64::new(0),
            proxy_patch_requests: AtomicU64::new(0),
            proxy_put_requests: AtomicU64::new(0),
            proxy_head_requests: AtomicU64::new(0),
            proxy_options_requests: AtomicU64::new(0),
            proxy_other_requests: AtomicU64::new(0),
            proxy_2xx_responses: AtomicU64::new(0),
            proxy_3xx_responses: AtomicU64::new(0),
            proxy_4xx_responses: AtomicU64::new(0),
            proxy_5xx_responses: AtomicU64::new(0),
            response_times: Mutex::new(ResponseTimeStats::new()),
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
        if let Ok(mut guard) = self.initialization_time.lock() {
            *guard = Some(Instant::now());
        }
    }

    /// Clears the initialization time, used when the manager fully stops.
    pub fn clear_initialization(&self) {
        if let Ok(mut guard) = self.initialization_time.lock() {
            *guard = None;
        }
    }

    /// Records an HTTP proxy request by method.
    pub fn record_proxy_request(&self, method: &str) {
        match method.to_uppercase().as_str() {
            "GET" => self.proxy_get_requests.fetch_add(1, Ordering::Relaxed),
            "POST" => self.proxy_post_requests.fetch_add(1, Ordering::Relaxed),
            "DELETE" => self.proxy_delete_requests.fetch_add(1, Ordering::Relaxed),
            "PATCH" => self.proxy_patch_requests.fetch_add(1, Ordering::Relaxed),
            "PUT" => self.proxy_put_requests.fetch_add(1, Ordering::Relaxed),
            "HEAD" => self.proxy_head_requests.fetch_add(1, Ordering::Relaxed),
            "OPTIONS" => self.proxy_options_requests.fetch_add(1, Ordering::Relaxed),
            _ => self.proxy_other_requests.fetch_add(1, Ordering::Relaxed),
        };
    }

    /// Records an HTTP proxy response with status code and response time.
    pub fn record_proxy_response(&self, status_code: u16, response_time: Duration) {
        // Record status code ranges
        match status_code {
            200..=299 => {
                self.proxy_2xx_responses.fetch_add(1, Ordering::Relaxed);
            }
            300..=399 => {
                self.proxy_3xx_responses.fetch_add(1, Ordering::Relaxed);
            }
            400..=499 => {
                self.proxy_4xx_responses.fetch_add(1, Ordering::Relaxed);
            }
            500..=599 => {
                self.proxy_5xx_responses.fetch_add(1, Ordering::Relaxed);
            }
            _ => {} // Ignore other status codes
        };

        // Record response time moving average
        let duration_ms = response_time.as_secs_f64() * 1000.0;
        if let Ok(mut stats) = self.response_times.lock() {
            stats.update(duration_ms, status_code);
        }
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
        self.initialization_time
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|start| start.elapsed()))
    }

    /// Gets the number of proxy requests by HTTP method
    pub fn proxy_get_requests(&self) -> u64 {
        self.proxy_get_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_post_requests(&self) -> u64 {
        self.proxy_post_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_delete_requests(&self) -> u64 {
        self.proxy_delete_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_patch_requests(&self) -> u64 {
        self.proxy_patch_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_put_requests(&self) -> u64 {
        self.proxy_put_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_head_requests(&self) -> u64 {
        self.proxy_head_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_options_requests(&self) -> u64 {
        self.proxy_options_requests.load(Ordering::Relaxed)
    }

    pub fn proxy_other_requests(&self) -> u64 {
        self.proxy_other_requests.load(Ordering::Relaxed)
    }

    /// Gets the number of proxy responses by status code range
    pub fn proxy_2xx_responses(&self) -> u64 {
        self.proxy_2xx_responses.load(Ordering::Relaxed)
    }

    pub fn proxy_3xx_responses(&self) -> u64 {
        self.proxy_3xx_responses.load(Ordering::Relaxed)
    }

    pub fn proxy_4xx_responses(&self) -> u64 {
        self.proxy_4xx_responses.load(Ordering::Relaxed)
    }

    pub fn proxy_5xx_responses(&self) -> u64 {
        self.proxy_5xx_responses.load(Ordering::Relaxed)
    }

    /// Gets response time moving averages (in milliseconds)
    pub fn response_time_ema_all(&self) -> f64 {
        self.response_times
            .lock()
            .map(|stats| stats.ema_all)
            .unwrap_or(0.0)
    }

    pub fn response_time_ema_2xx(&self) -> f64 {
        self.response_times
            .lock()
            .map(|stats| stats.ema_2xx)
            .unwrap_or(0.0)
    }

    pub fn response_time_ema_4xx(&self) -> f64 {
        self.response_times
            .lock()
            .map(|stats| stats.ema_4xx)
            .unwrap_or(0.0)
    }

    pub fn response_time_ema_5xx(&self) -> f64 {
        self.response_times
            .lock()
            .map(|stats| stats.ema_5xx)
            .unwrap_or(0.0)
    }

    pub fn response_time_sample_count(&self) -> u64 {
        self.response_times
            .lock()
            .map(|stats| stats.sample_count)
            .unwrap_or(0)
    }

    /// Gets all metrics as a human-readable string.
    pub fn format_metrics(&self) -> String {
        let mut output = String::new();
        output.push_str("libagent metrics:\n");

        // Process metrics
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

        // HTTP proxy request metrics
        output.push_str("\n  HTTP Proxy Requests:\n");
        output.push_str(&format!("    GET: {}\n", self.proxy_get_requests()));
        output.push_str(&format!("    POST: {}\n", self.proxy_post_requests()));
        output.push_str(&format!("    PUT: {}\n", self.proxy_put_requests()));
        output.push_str(&format!("    DELETE: {}\n", self.proxy_delete_requests()));
        output.push_str(&format!("    PATCH: {}\n", self.proxy_patch_requests()));
        output.push_str(&format!("    HEAD: {}\n", self.proxy_head_requests()));
        output.push_str(&format!("    OPTIONS: {}\n", self.proxy_options_requests()));
        output.push_str(&format!("    OTHER: {}\n", self.proxy_other_requests()));

        // HTTP proxy response metrics
        output.push_str("\n  HTTP Proxy Responses:\n");
        output.push_str(&format!("    2xx: {}\n", self.proxy_2xx_responses()));
        output.push_str(&format!("    3xx: {}\n", self.proxy_3xx_responses()));
        output.push_str(&format!("    4xx: {}\n", self.proxy_4xx_responses()));
        output.push_str(&format!("    5xx: {}\n", self.proxy_5xx_responses()));

        // Response time moving averages
        output.push_str("\n  Response Time Moving Averages (ms):\n");
        let sample_count = self.response_time_sample_count();
        if sample_count > 0 {
            output.push_str(&format!(
                "    all_responses: {:.2}ms ({} samples)\n",
                self.response_time_ema_all(),
                sample_count
            ));
            if self.response_time_ema_2xx() > 0.0 {
                output.push_str(&format!(
                    "    2xx_responses: {:.2}ms\n",
                    self.response_time_ema_2xx()
                ));
            }
            if self.response_time_ema_4xx() > 0.0 {
                output.push_str(&format!(
                    "    4xx_responses: {:.2}ms\n",
                    self.response_time_ema_4xx()
                ));
            }
            if self.response_time_ema_5xx() > 0.0 {
                output.push_str(&format!(
                    "    5xx_responses: {:.2}ms\n",
                    self.response_time_ema_5xx()
                ));
            }
        } else {
            output.push_str("    no_response_times_recorded\n");
        }

        output
    }
}

/// Gets the global metrics instance.
pub fn get_metrics() -> &'static Metrics {
    METRICS.get_or_init(Metrics::new)
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

        // Record proxy requests
        metrics.record_proxy_request("GET");
        metrics.record_proxy_request("POST");
        metrics.record_proxy_request("GET");
        metrics.record_proxy_request("UNKNOWN");

        // Record proxy responses with different status codes and response times
        use std::time::Duration;
        metrics.record_proxy_response(200, Duration::from_millis(100));
        metrics.record_proxy_response(404, Duration::from_millis(50));
        metrics.record_proxy_response(500, Duration::from_millis(2000));
        metrics.record_proxy_response(201, Duration::from_millis(150));

        // Test proxy request counts
        assert_eq!(metrics.proxy_get_requests(), 2);
        assert_eq!(metrics.proxy_post_requests(), 1);
        assert_eq!(metrics.proxy_other_requests(), 1);

        // Test proxy response counts
        assert_eq!(metrics.proxy_2xx_responses(), 2); // 200, 201
        assert_eq!(metrics.proxy_4xx_responses(), 1); // 404
        assert_eq!(metrics.proxy_5xx_responses(), 1); // 500

        // Test response time EMAs (should be approximate due to moving average)
        assert!(metrics.response_time_ema_all() > 0.0);
        assert!(metrics.response_time_ema_2xx() > 0.0);
        assert!(metrics.response_time_ema_4xx() > 0.0);
        assert!(metrics.response_time_ema_5xx() > 0.0);
        assert_eq!(metrics.response_time_sample_count(), 4);

        // Test formatting
        let formatted = metrics.format_metrics();
        assert!(formatted.contains("agent_spawns: 2"));
        assert!(formatted.contains("trace_agent_spawns: 1"));
        assert!(formatted.contains("agent_failures: 1"));
        assert!(formatted.contains("trace_agent_failures: 2"));
        assert!(formatted.contains("uptime_seconds"));
        assert!(formatted.contains("GET: 2"));
        assert!(formatted.contains("POST: 1"));
        assert!(formatted.contains("2xx: 2"));
        assert!(formatted.contains("4xx: 1"));
        assert!(formatted.contains("5xx: 1"));
        assert!(formatted.contains("Response Time Moving Averages"));
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

    #[test]
    fn test_metrics_all_http_methods() {
        let metrics = Metrics::new();

        // Test all HTTP methods are recorded correctly
        metrics.record_proxy_request("GET");
        metrics.record_proxy_request("POST");
        metrics.record_proxy_request("PUT");
        metrics.record_proxy_request("DELETE");
        metrics.record_proxy_request("PATCH");
        metrics.record_proxy_request("HEAD");
        metrics.record_proxy_request("OPTIONS");
        metrics.record_proxy_request("CUSTOM");

        assert_eq!(metrics.proxy_get_requests(), 1);
        assert_eq!(metrics.proxy_post_requests(), 1);
        assert_eq!(metrics.proxy_put_requests(), 1);
        assert_eq!(metrics.proxy_delete_requests(), 1);
        assert_eq!(metrics.proxy_patch_requests(), 1);
        assert_eq!(metrics.proxy_head_requests(), 1);
        assert_eq!(metrics.proxy_options_requests(), 1);
        assert_eq!(metrics.proxy_other_requests(), 1);
    }

    #[test]
    fn test_metrics_all_status_code_ranges() {
        let metrics = Metrics::new();
        use std::time::Duration;

        // Test all status code ranges
        metrics.record_proxy_response(200, Duration::from_millis(100)); // 2xx
        metrics.record_proxy_response(301, Duration::from_millis(150)); // 3xx
        metrics.record_proxy_response(404, Duration::from_millis(50)); // 4xx
        metrics.record_proxy_response(500, Duration::from_millis(200)); // 5xx
        metrics.record_proxy_response(100, Duration::from_millis(75)); // Other (should not count)

        assert_eq!(metrics.proxy_2xx_responses(), 1);
        assert_eq!(metrics.proxy_3xx_responses(), 1);
        assert_eq!(metrics.proxy_4xx_responses(), 1);
        assert_eq!(metrics.proxy_5xx_responses(), 1);
    }

    #[test]
    fn test_metrics_response_time_ema_calculation() {
        let metrics = Metrics::new();
        use std::time::Duration;

        // Test EMA calculation with multiple samples
        metrics.record_proxy_response(200, Duration::from_millis(100));
        metrics.record_proxy_response(200, Duration::from_millis(200));
        metrics.record_proxy_response(200, Duration::from_millis(150));

        // EMA should be calculated (exact value depends on alpha, but should be reasonable)
        let ema_2xx = metrics.response_time_ema_2xx();
        assert!(ema_2xx > 0.0);
        assert!(ema_2xx < 200.0); // Should be within reasonable bounds

        assert_eq!(metrics.response_time_sample_count(), 3);
    }

    #[test]
    fn test_metrics_response_time_different_status_codes() {
        let metrics = Metrics::new();
        use std::time::Duration;

        // Record responses with different status codes
        metrics.record_proxy_response(200, Duration::from_millis(100));
        metrics.record_proxy_response(404, Duration::from_millis(50));
        metrics.record_proxy_response(500, Duration::from_millis(300));

        // Each status code range should have its own EMA
        assert!(metrics.response_time_ema_2xx() > 0.0);
        assert!(metrics.response_time_ema_4xx() > 0.0);
        assert!(metrics.response_time_ema_5xx() > 0.0);

        // Overall EMA should also be calculated
        assert!(metrics.response_time_ema_all() > 0.0);
    }

    #[test]
    fn test_metrics_format_metrics_comprehensive() {
        let metrics = Metrics::new();
        use std::time::Duration;

        // Record various metrics
        metrics.record_initialization();
        metrics.record_agent_spawn();
        metrics.record_trace_agent_spawn();
        metrics.record_agent_failure();
        metrics.record_proxy_request("GET");
        metrics.record_proxy_request("POST");
        metrics.record_proxy_response(200, Duration::from_millis(100));
        metrics.record_proxy_response(404, Duration::from_millis(50));

        let formatted = metrics.format_metrics();

        // Check that all sections are present
        assert!(formatted.contains("libagent metrics:"));
        assert!(formatted.contains("agent_spawns: 1"));
        assert!(formatted.contains("trace_agent_spawns: 1"));
        assert!(formatted.contains("agent_failures: 1"));
        assert!(formatted.contains("GET: 1"));
        assert!(formatted.contains("POST: 1"));
        assert!(formatted.contains("2xx: 1"));
        assert!(formatted.contains("4xx: 1"));
        assert!(formatted.contains("uptime_seconds"));
    }

    #[test]
    fn test_metrics_zero_values() {
        let metrics = Metrics::new();

        // Test that all getters return 0 for uninitialized metrics
        assert_eq!(metrics.agent_spawns(), 0);
        assert_eq!(metrics.trace_agent_spawns(), 0);
        assert_eq!(metrics.agent_failures(), 0);
        assert_eq!(metrics.trace_agent_failures(), 0);
        assert_eq!(metrics.proxy_get_requests(), 0);
        assert_eq!(metrics.proxy_post_requests(), 0);
        assert_eq!(metrics.proxy_2xx_responses(), 0);
        assert_eq!(metrics.proxy_4xx_responses(), 0);
        assert_eq!(metrics.response_time_sample_count(), 0);
        assert_eq!(metrics.response_time_ema_all(), 0.0);
        assert_eq!(metrics.response_time_ema_2xx(), 0.0);
        assert_eq!(metrics.response_time_ema_4xx(), 0.0);
        assert_eq!(metrics.response_time_ema_5xx(), 0.0);
    }

    #[test]
    fn test_metrics_uptime_calculation() {
        let metrics = Metrics::new();

        // Initially no uptime
        assert!(metrics.uptime().is_none());

        // After recording initialization, uptime should be available
        metrics.record_initialization();
        assert!(metrics.uptime().is_some());

        // Uptime should be very small (just recorded)
        let uptime = metrics.uptime().unwrap();
        assert!(uptime.as_secs() < 1); // Should be less than 1 second
    }

    #[test]
    fn test_global_metrics_singleton() {
        let metrics1 = get_metrics();
        let metrics2 = get_metrics();

        // Should return the same instance
        assert_eq!(metrics1.agent_spawns(), metrics2.agent_spawns());

        // Modifying one should affect the other (singleton behavior)
        metrics1.record_agent_spawn();
        assert_eq!(metrics2.agent_spawns(), 1);
    }
}
