//! DogStatsD client for sending metrics over Unix Domain Sockets or Named Pipes.
//!
//! This module provides a simple client for sending metrics to DogStatsD using the
//! standard DogStatsD protocol over IPC (Unix Domain Sockets on Unix, Named Pipes on Windows).
//!
//! The DogStatsD protocol is a simple text-based format:
//! ```text
//! <metric_name>:<value>|<type>|@<sample_rate>|#<tags>|T<timestamp>
//! ```
//!
//! Examples:
//! - Counter: `page.views:1|c`
//! - Gauge: `temperature:72.5|g|#env:prod`
//! - Histogram: `request.duration:250|h|@0.5|#endpoint:/api`
//!
//! Multiple metrics can be batched in a single payload, separated by newlines.

#[cfg(unix)]
use crate::logging::log_debug;
#[cfg(unix)]
use std::time::Duration;

/// Sends a DogStatsD metric payload over Unix Domain Socket.
///
/// This function sends a raw DogStatsD protocol payload to the specified socket path.
/// The payload should be in the standard DogStatsD format (text-based).
///
/// # Arguments
/// * `request_id` - Unique ID for logging/debugging
/// * `uds_path` - Path to the DogStatsD Unix Domain Socket
/// * `payload` - Raw DogStatsD metric payload (e.g., "page.views:1|c|#env:prod")
/// * `timeout` - Write timeout
///
/// # Returns
/// `Ok(())` on success, or an error message on failure.
#[cfg(unix)]
pub fn send_metric_over_uds(
    request_id: u64,
    uds_path: &str,
    payload: &[u8],
    timeout: Duration,
) -> Result<(), String> {
    use std::os::unix::net::UnixDatagram;

    log_debug(&format!(
        "DogStatsD [req:{}]: connecting to socket: {}",
        request_id, uds_path
    ));

    // DogStatsD uses SOCK_DGRAM (datagram sockets) for metrics
    let socket = UnixDatagram::unbound().map_err(|e| {
        let err = format!("failed to create datagram socket: {}", e);
        log_debug(&format!("DogStatsD [req:{}]: {}", request_id, err));
        err
    })?;

    let _ = socket.set_write_timeout(Some(timeout));

    log_debug(&format!(
        "DogStatsD [req:{}]: sending metric ({} bytes)",
        request_id,
        payload.len()
    ));

    // Log payload for debugging (it's text-based)
    if let Ok(payload_str) = std::str::from_utf8(payload) {
        log_debug(&format!(
            "DogStatsD [req:{}]: payload: {}",
            request_id, payload_str
        ));
    }

    // Send the metric as a datagram to the DogStatsD socket
    socket.send_to(payload, uds_path).map_err(|e| {
        let err = format!("send_to error ({}): {}", uds_path, e);
        log_debug(&format!("DogStatsD [req:{}]: {}", request_id, err));
        err
    })?;

    log_debug(&format!(
        "DogStatsD [req:{}]: metric sent successfully",
        request_id
    ));

    Ok(())
}

/// Sends a DogStatsD metric payload over Windows Named Pipe.
///
/// This function sends a raw DogStatsD protocol payload to the specified pipe.
/// The payload should be in the standard DogStatsD format (text-based).
///
/// # Arguments
/// * `request_id` - Unique ID for logging/debugging
/// * `pipe_name` - Name of the DogStatsD Named Pipe (without `\\.\pipe\` prefix)
/// * `payload` - Raw DogStatsD metric payload
/// * `_timeout` - Write timeout (unused for Named Pipe writes)
///
/// # Returns
/// `Ok(())` on success, or an error message on failure.
#[cfg(windows)]
pub fn send_metric_over_pipe(
    request_id: u64,
    pipe_name: &str,
    payload: &[u8],
    _timeout: std::time::Duration,
) -> Result<(), String> {
    use crate::logging::log_debug;
    use std::io::Write;

    // Construct full pipe path
    let pipe_path = if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name.to_string()
    } else {
        format!(r"\\.\pipe\{}", pipe_name)
    };

    log_debug(&format!(
        "DogStatsD [req:{}]: connecting to pipe: {}",
        request_id, pipe_path
    ));

    // Open the named pipe
    let mut pipe = std::fs::OpenOptions::new()
        .write(true)
        .open(&pipe_path)
        .map_err(|e| {
            let err = format!("failed to open pipe ({}): {}", pipe_path, e);
            log_debug(&format!("DogStatsD [req:{}]: {}", request_id, err));
            err
        })?;

    log_debug(&format!(
        "DogStatsD [req:{}]: sending metric ({} bytes)",
        request_id,
        payload.len()
    ));

    // Log payload for debugging
    if let Ok(payload_str) = std::str::from_utf8(payload) {
        log_debug(&format!(
            "DogStatsD [req:{}]: payload: {}",
            request_id, payload_str
        ));
    }

    // Write the metric to the pipe
    pipe.write_all(payload).map_err(|e| {
        let err = format!("write error: {}", e);
        log_debug(&format!("DogStatsD [req:{}]: {}", request_id, err));
        err
    })?;

    // Note: DogStatsD is fire-and-forget, no response expected
    log_debug(&format!(
        "DogStatsD [req:{}]: metric sent successfully",
        request_id
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_send_metric_over_uds_socket_not_found() {
        // Test with non-existent socket
        let result = send_metric_over_uds(
            1,
            "/tmp/nonexistent_dogstatsd.socket",
            b"test.metric:1|c",
            Duration::from_secs(1),
        );
        assert!(result.is_err());
        // Check that the error contains expected message
        if let Err(err) = result {
            assert!(err.contains("No such file or directory") || err.contains("send_to error"));
        }
    }

    #[cfg(windows)]
    #[test]
    fn test_send_metric_over_pipe_not_found() {
        // Test with non-existent pipe
        let result = send_metric_over_pipe(
            1,
            "nonexistent-dogstatsd-pipe",
            b"test.metric:1|c",
            std::time::Duration::from_secs(1),
        );
        assert!(result.is_err());
    }
}
