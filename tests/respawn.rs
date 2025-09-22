#![cfg(unix)]
use serial_test::serial;
use std::env;
use std::time::Duration;

mod common;

#[test]
#[serial]
fn respawns_on_exit_with_backoff() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    let (script, starts) = common::unix::create_stub_exit(dir);

    // Configure agent to exit immediately; trace agent to a sleeper so the thread keeps running
    let sleeper = common::unix::create_stub_sleep(dir);

    unsafe {
        env::set_var("LIBAGENT_AGENT_PROGRAM", &script);
        env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", &sleeper.script);
        env::set_var("LIBAGENT_AGENT_ARGS", "");
        env::set_var("LIBAGENT_TRACE_AGENT_ARGS", "");
        env::set_var("LIBAGENT_MONITOR_INTERVAL_SECS", "1");
        env::set_var("LIBAGENT_LOG", "error");
    }

    libagent::initialize();

    // Wait a bit to allow multiple respawn attempts; we expect at least 3 starts after ~5s with backoff
    std::thread::sleep(Duration::from_secs(6));
    let content = common::read_to_string(&starts);
    let count = content.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(count >= 3, "expected >=3 restarts, got {} (content: {})", count, content);

    libagent::stop();
}


