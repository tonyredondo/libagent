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
