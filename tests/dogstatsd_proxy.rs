//! Integration tests for DogStatsD proxy functionality.
//!
//! These tests verify that the DogStatsD proxy can send metrics over Unix Domain Sockets.
//! The tests create a mock DogStatsD server (datagram socket) and verify that metrics
//! are correctly sent and received.

#![cfg(unix)]

use serial_test::serial;
use std::os::unix::net::UnixDatagram;
use tempfile::TempDir;

/// Helper to create a temporary DogStatsD mock server.
/// Returns (socket, socket_path, temp_dir)
fn create_mock_dogstatsd_server() -> (UnixDatagram, String, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let socket_path = temp_dir.path().join("dogstatsd.socket");
    let socket_path_str = socket_path.to_string_lossy().to_string();

    // Create a datagram socket (DogStatsD uses SOCK_DGRAM)
    let socket = UnixDatagram::bind(&socket_path).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .unwrap();

    (socket, socket_path_str, temp_dir)
}

#[test]
#[serial]
fn test_dogstatsd_send_counter() {
    // Set up mock DogStatsD server
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    // Override the DogStatsD socket path
    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send a counter metric
    let metric = b"test.counter:1|c|#env:test";
    let result =
        libagent::send_metric_over_uds(1, &socket_path, metric, std::time::Duration::from_secs(5));
    assert!(result.is_ok(), "Failed to send metric: {:?}", result);

    // Receive and verify the metric
    let mut buf = [0u8; 1024];
    let (size, _addr) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(received, "test.counter:1|c|#env:test");

    // Clean up
    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_send_gauge() {
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send a gauge metric
    let metric = b"test.temperature:72.5|g|#location:office";
    let result =
        libagent::send_metric_over_uds(2, &socket_path, metric, std::time::Duration::from_secs(5));
    assert!(result.is_ok());

    let mut buf = [0u8; 1024];
    let (size, _) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(received, "test.temperature:72.5|g|#location:office");

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_send_histogram() {
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send a histogram metric
    let metric = b"test.response_time:250|h|@0.5|#endpoint:/api";
    let result =
        libagent::send_metric_over_uds(3, &socket_path, metric, std::time::Duration::from_secs(5));
    assert!(result.is_ok());

    let mut buf = [0u8; 1024];
    let (size, _) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(received, "test.response_time:250|h|@0.5|#endpoint:/api");

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_send_batched_metrics() {
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send batched metrics (multiple metrics in one payload)
    let metrics = b"test.counter:1|c\ntest.gauge:42|g\ntest.timer:100|ms";
    let result =
        libagent::send_metric_over_uds(4, &socket_path, metrics, std::time::Duration::from_secs(5));
    assert!(result.is_ok());

    let mut buf = [0u8; 1024];
    let (size, _) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(
        received,
        "test.counter:1|c\ntest.gauge:42|g\ntest.timer:100|ms"
    );

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_send_distribution() {
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send a distribution metric
    let metric = b"test.response_size:512|d|#status:200";
    let result =
        libagent::send_metric_over_uds(5, &socket_path, metric, std::time::Duration::from_secs(5));
    assert!(result.is_ok());

    let mut buf = [0u8; 1024];
    let (size, _) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(received, "test.response_size:512|d|#status:200");

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_nonexistent_socket() {
    // Test with a non-existent socket path
    let result = libagent::send_metric_over_uds(
        6,
        "/tmp/nonexistent_dogstatsd_socket_12345.socket",
        b"test.metric:1|c",
        std::time::Duration::from_secs(1),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("No such file or directory"));
}

#[test]
#[serial]
fn test_dogstatsd_with_multiple_tags() {
    let (server_socket, socket_path, _temp_dir) = create_mock_dogstatsd_server();

    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", &socket_path);
    }

    // Send metric with multiple tags
    let metric = b"test.api.calls:1|c|#env:prod,region:us-east-1,service:api,version:2.0";
    let result =
        libagent::send_metric_over_uds(7, &socket_path, metric, std::time::Duration::from_secs(5));
    assert!(result.is_ok());

    let mut buf = [0u8; 1024];
    let (size, _) = server_socket.recv_from(&mut buf).unwrap();
    let received = std::str::from_utf8(&buf[..size]).unwrap();
    assert_eq!(
        received,
        "test.api.calls:1|c|#env:prod,region:us-east-1,service:api,version:2.0"
    );

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}

#[test]
#[serial]
fn test_dogstatsd_config_default_path() {
    // Test that default path is /tmp/datadog_dogstatsd.socket
    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }

    let path = libagent::get_dogstatsd_uds_path();
    assert_eq!(path, "/tmp/datadog_dogstatsd.socket");
}

#[test]
#[serial]
fn test_dogstatsd_config_override_path() {
    // Test that env var overrides default path
    let custom_path = "/tmp/custom_dogstatsd.socket";
    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_UDS", custom_path);
    }

    let path = libagent::get_dogstatsd_uds_path();
    assert_eq!(path, custom_path);

    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_UDS");
    }
}
