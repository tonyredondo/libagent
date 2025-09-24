#![cfg(windows)]
use serial_test::serial;
use std::env;

mod common;

#[test]
#[serial]
fn start_and_stop_windows_job() {
    // Use PowerShell to sleep for a while
    let (prog, arg) = common::windows::powershell_sleep_args(3);

    unsafe {
        env::set_var("LIBAGENT_AGENT_PROGRAM", prog);
        env::set_var("LIBAGENT_AGENT_ARGS", arg.clone());
        env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", prog);
        env::set_var("LIBAGENT_TRACE_AGENT_ARGS", arg);
        env::set_var("LIBAGENT_MONITOR_INTERVAL_SECS", "1");
    }

    libagent::initialize();
    // Immediately stop; job should terminate children reliably
    libagent::stop();
}

#[test]
fn test_windows_config_functions() {
    // Test Windows-specific config functions
    let pipe_name = libagent::get_trace_agent_pipe_name();
    assert!(!pipe_name.is_empty());

    // Test with environment override
    unsafe {
        env::set_var("LIBAGENT_TRACE_AGENT_PIPE", "custom-pipe");
    }
    let custom_pipe = libagent::get_trace_agent_pipe_name();
    assert_eq!(custom_pipe, "custom-pipe");
    unsafe {
        env::remove_var("LIBAGENT_TRACE_AGENT_PIPE");
    }
}

#[test]
#[serial]
fn test_windows_job_object_integration() {
    // Test that Windows job objects work by starting and stopping
    // This exercises the Windows-specific job object code paths
    let (prog, arg) = common::windows::powershell_sleep_args(1);

    unsafe {
        env::set_var("LIBAGENT_AGENT_PROGRAM", prog);
        env::set_var("LIBAGENT_AGENT_ARGS", arg.clone());
        env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", prog);
        env::set_var("LIBAGENT_TRACE_AGENT_ARGS", arg);
        env::set_var("LIBAGENT_MONITOR_INTERVAL_SECS", "1");
    }

    // Initialize should create job objects
    libagent::initialize();
    std::thread::sleep(std::time::Duration::from_millis(500));
    // Stop should terminate job objects and all child processes
    libagent::stop();
}
