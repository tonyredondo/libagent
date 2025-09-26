#![cfg(unix)]
use serial_test::serial;
use std::env;
use std::time::Duration;

mod common;

#[test]
#[serial]
fn initialize_and_stop_kills_children() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let stub = common::unix::create_stub_sleep(dir);

    // Configure both agent and trace agent to use the same stub
    unsafe {
        env::set_var("LIBAGENT_AGENT_ENABLED", "true");
        env::set_var("LIBAGENT_AGENT_PROGRAM", &stub.script);
        env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", &stub.script);
        env::set_var("LIBAGENT_AGENT_ARGS", "");
        env::set_var("LIBAGENT_TRACE_AGENT_ARGS", "");
        env::set_var("LIBAGENT_MONITOR_INTERVAL_SECS", "1");
        env::set_var("LIBAGENT_LOG", "debug");
    }

    // Start
    libagent::initialize();

    // Wait until both have started at least once
    let lines = common::wait_for_lines(&stub.starts, 2, Duration::from_secs(5));
    assert!(
        lines.len() >= 2,
        "expected at least two starts, got {:?}",
        lines
    );

    // Stop
    libagent::stop();

    // After stop, send signals should not be necessary; verify that TERM was handled
    let events = common::read_to_string(&stub.events);
    assert!(
        events.contains("TERM"),
        "expected TERM events, got: {}",
        events
    );
}
