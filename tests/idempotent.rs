use serial_test::serial;
use std::env;
use std::time::Duration;

mod common;

#[test]
#[serial]
fn initialize_is_idempotent_and_stop_is_idempotent() {
    // Use a sleeper to ensure processes stay alive
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    #[cfg(unix)]
    let stub = common::unix::create_stub_sleep(dir);
    #[cfg(unix)]
    {
        unsafe {
            env::set_var("LIBAGENT_AGENT_PROGRAM", &stub.script);
            env::set_var("LIBAGENT_TRACE_AGENT_PROGRAM", &stub.script);
            env::set_var("LIBAGENT_AGENT_ARGS", "");
            env::set_var("LIBAGENT_TRACE_AGENT_ARGS", "");
            env::set_var("LIBAGENT_MONITOR_INTERVAL_SECS", "1");
        }
    }

    // Call initialize multiple times
    libagent::initialize();
    libagent::initialize();
    libagent::initialize();

    // Short wait then stop multiple times
    std::thread::sleep(Duration::from_millis(200));
    libagent::stop();
    libagent::stop();
    libagent::stop();
}


