//! Integration tests for DogStatsD proxy functionality on Windows.
//!
//! These tests verify basic DogStatsD functionality on Windows using Named Pipes.

#![cfg(windows)]

use serial_test::serial;

#[test]
#[serial]
fn test_dogstatsd_nonexistent_pipe() {
    // Test sending a metric to a non-existent pipe
    let result = libagent::send_metric_over_pipe(
        1,
        "nonexistent-dogstatsd-pipe-test",
        b"test.metric:1|c",
        std::time::Duration::from_secs(1),
    );

    // Should fail since the pipe doesn't exist
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("failed to open pipe")
            || err.contains("The system cannot find the file specified")
    );
}

#[test]
#[serial]
fn test_dogstatsd_config_default_pipe_name() {
    let pipe_name = libagent::get_dogstatsd_pipe_name();
    assert_eq!(pipe_name, "datadog-dogstatsd");
}

#[test]
#[serial]
fn test_dogstatsd_config_override_pipe_name() {
    unsafe {
        std::env::set_var("LIBAGENT_DOGSTATSD_PIPE", "custom-dogstatsd-pipe");
    }
    let pipe_name = libagent::get_dogstatsd_pipe_name();
    assert_eq!(pipe_name, "custom-dogstatsd-pipe");
    unsafe {
        std::env::remove_var("LIBAGENT_DOGSTATSD_PIPE");
    }
}
