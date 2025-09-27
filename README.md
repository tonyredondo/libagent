# libagent

[![CI](https://github.com/tonyredondo/libagent/actions/workflows/ci.yml/badge.svg)](https://github.com/tonyredondo/libagent/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/tonyredondo/libagent/branch/main/graph/badge.svg)](https://codecov.io/gh/tonyredondo/libagent)
![Rust nightly](https://img.shields.io/badge/rust-nightly-blue)
![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-informational)

Minimal-runtime Rust library that ensures the Datadog Agent and Trace Agent are running while the library is loaded. Exposes both a Rust API and a stable C FFI for embedding in non-Rust hosts.

## Features
- Starts and monitors Agent + Trace Agent with exponential backoff
- Graceful shutdown (SIGTERM then SIGKILL on Unix; Windows Job kill)
- Idempotent `initialize`/`stop`; destructor stops on unload
- Cross‑platform: Linux, macOS, Windows (tested in CI)

## Build
- Requires Rust nightly.
- Debug: `cargo +nightly build`
- Release: `cargo +nightly build --release`

Outputs include a Rust `rlib` and a shared library (`.so/.dylib/.dll`) per the crate-type settings.

## Architecture Overview
- Public API: `lib.rs` re-exports `initialize`/`stop`; a destructor (`#[ctor::dtor]`) calls `stop` on unload.
- Process Manager (`manager.rs`): spawns Agent and Trace Agent; monitors with periodic ticks and exponential backoff; restarts on exit.
  - Unix: children run in their own session (`setsid`); sends `SIGTERM`, then `SIGKILL` to the process group on shutdown.
  - Windows: assigns children to a Job; terminating the Job kills the tree.
  - **Trace Agent Configuration**: automatically configured for IPC-only operation (TCP port disabled, custom UDS/Named Pipe paths).
  - **Smart Spawning**: Trace-agent only spawns if IPC socket/pipe is available; Agent only spawns if no existing remote config service exists.
- Configuration (`config.rs`): compile-time defaults with env overrides parsed via `shell-words`; tunables include programs, args, and monitor interval.
- FFI (`ffi.rs`): exports `Initialize`, `Stop`, and a transport-agnostic trace-agent proxy (`ProxyTraceAgent`) with `catch_unwind`.
  - Unix: connects over UDS.
  - Windows: connects over Windows Named Pipes.
- Logging: `LIBAGENT_LOG` level and `LIBAGENT_DEBUG` to inherit child stdout/stderr.

For a deeper dive, see ARCHITECTURE.md.

## Process Spawning Behavior

libagent implements smart process spawning to prevent conflicts and ensure cooperation:

### Trace-Agent Spawning
- **Spawns when**: IPC socket/pipe (`/tmp/datadog_libagent.socket` or `\\.\pipe\datadog-libagent`) is available
- **Skips when**: Another process is already using the IPC endpoint
- **Configuration**: IPC-only mode (TCP disabled, custom socket/pipe path)

### Agent Spawning
- **Spawns when**: `LIBAGENT_AGENT_ENABLED=true` AND no existing agent provides remote configuration on `localhost:5001`
- **Skips when**: `LIBAGENT_AGENT_ENABLED=false` (default) OR (agent is enabled AND existing agent provides remote config service)
- **Purpose**: Support custom trace-agents by default; allow traditional Datadog agent cooperation when explicitly enabled
- **Configuration**: Set `LIBAGENT_AGENT_ENABLED=true` to enable the main Datadog agent (disabled by default for custom trace-agents)

### Monitoring & Recovery
- Both processes are continuously monitored and automatically restarted on failure
- Only processes spawned by libagent are managed (external processes are respected)
- Exponential backoff prevents resource exhaustion during failure scenarios

### Process Lifecycle & Cleanup
- **Normal Shutdown**: When `stop()` is called or the library unloads, all child processes are terminated gracefully (SIGTERM then SIGKILL on Unix, Job termination on Windows)
- **Parent Process Killed**: If the parent application is killed normally, libagent's destructor ensures proper cleanup
- **Forceful Termination**: If the parent is killed with SIGKILL (or crashes):
  - **Linux**: Parent death signals ensure child processes are terminated immediately when the parent dies
  - **macOS/BSD**: Dedicated monitor process provides immediate cleanup when parent dies
  - **Windows**: Job Objects ensure all processes in the job are terminated immediately when the job handle closes
- **Best Practice**: Ensure your application calls `libagent::stop()` during shutdown for reliable cleanup

## Usage (Rust)
```rust
fn main() {
    // Start agents and monitoring
    libagent::initialize();

    // ... your app logic ...

    // Clean stop (idempotent); also runs automatically at unload/exit
    libagent::stop();
}
```

## Usage (C / FFI)
Link your application to the produced shared library and call the exported symbols:
```c
#include "libagent.h"

// Callback function for successful responses
void on_response(uint16_t status,
                 const uint8_t* headers_data, size_t headers_len,
                 const uint8_t* body_data, size_t body_len,
                 void* user_data) {
    printf("Status: %u\n", status);
    printf("Headers: %.*s\n", (int)headers_len, (const char*)headers_data);
    printf("Body: %.*s\n", (int)body_len, (const char*)body_data);
}

// Callback function for errors
void on_error(const char* error_message, void* user_data) {
    fprintf(stderr, "Error: %s\n", error_message);
}

int main(void) {
    Initialize();

    // Get current metrics
    struct MetricsData metrics = GetMetrics();
    printf("Agent spawns: %llu\n", metrics.agent_spawns);
    printf("GET requests: %llu\n", metrics.proxy_get_requests);
    printf("Average response time: %.2f ms\n", metrics.response_time_ema_all);

    // Make a request using callbacks - no manual memory management!
    int32_t result = ProxyTraceAgent(
        "GET",                           // method
        "/info",                         // path
        "Accept: application/json\n",    // headers
        NULL, 0,                         // body (none)
        on_response,                     // success callback
        on_error,                        // error callback
        NULL                             // user data (not used here)
    );

    // ... your program continues ...

    Stop();
    return 0;
}
```

Callback API notes:
- Socket path resolution (Unix): env `LIBAGENT_TRACE_AGENT_UDS` or default `/tmp/datadog_libagent.socket` (temp directory).
- Pipe name resolution (Windows): env `LIBAGENT_TRACE_AGENT_PIPE` or default `datadog-libagent` (full path: `\\.\\pipe\\datadog-libagent`).
- Timeout: 50 seconds for both Unix UDS and Windows Named Pipe connections.
- `headers`: string with lines `Name: Value` separated by `\n` or `\r\n`.
- Callbacks receive data directly - no memory management required!
- `on_error` callback may be `NULL` if error handling is not needed.

Metrics API:
- `GetMetrics()` returns a `MetricsData` struct with all current metrics values.
- Provides direct access to process lifecycle metrics, HTTP proxy request/response counts, and response time moving averages.
- Thread-safe and can be called at any time.

Notes: The `Initialize` and `Stop` FFI functions return `void`. The `ProxyTraceAgent` function returns an `int32_t` error code (0 for success, negative for errors). Operational errors are logged; panics in Rust are caught with `catch_unwind` to avoid unwinding across the FFI boundary.

## Configuration
Defaults live in `src/config.rs`. Override at runtime via environment variables:
- `LIBAGENT_AGENT_ENABLED` (enable main agent; disabled by default for custom trace-agents)
- `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_AGENT_ARGS`
- `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_ARGS`
- `LIBAGENT_MONITOR_INTERVAL_SECS`
- Logging: `LIBAGENT_LOG` (error|warn|info|debug), `LIBAGENT_DEBUG` (1/true)

Notes:
- `*_ARGS` values are parsed using shell-words. Quote arguments as you would in a shell, e.g. `LIBAGENT_AGENT_ARGS='-c "my arg"'`.
- Agent is **disabled by default** to support custom trace-agent implementations.
- Set `LIBAGENT_AGENT_ENABLED=true` to enable the main Datadog agent.
- When agent is enabled, remote config cooperation is automatically enabled.

Example with main agent enabled:
```sh
LIBAGENT_AGENT_ENABLED=true \
LIBAGENT_AGENT_PROGRAM=/usr/bin/datadog-agent \
LIBAGENT_TRACE_AGENT_PROGRAM=/usr/bin/trace-agent \
LIBAGENT_LOG=info LIBAGENT_MONITOR_INTERVAL_SECS=1 \
cargo +nightly test -- --nocapture
```

Trace-agent only (default behavior):
```sh
LIBAGENT_TRACE_AGENT_PROGRAM=/path/to/custom/trace-agent \
LIBAGENT_LOG=info \
cargo +nightly test -- --nocapture
```

Traditional Datadog agent cooperation:
```sh
LIBAGENT_AGENT_ENABLED=true \
LIBAGENT_AGENT_PROGRAM=/usr/bin/datadog-agent \
LIBAGENT_TRACE_AGENT_PROGRAM=/usr/bin/trace-agent \
LIBAGENT_LOG=info \
cargo +nightly test -- --nocapture
```

## Logging
- **Format**: `2025-09-26T14:44:51.408Z [libagent] [LEVEL] message`
- **Timestamps**: ISO 8601 format with millisecond precision (UTC)
- **Levels**: `[ERROR]`, `[WARN]`, `[INFO]`, `[DEBUG]`
- Default: the library writes its own logs to stderr. Child process stdout/stderr are inherited when `LIBAGENT_DEBUG=1` or when `LIBAGENT_LOG=debug`.
- `LIBAGENT_DEBUG=1` also sets the internal log level to `debug`.
- Optional log facade: enable the `log` feature to route logs through the Rust `log` crate.

Cargo example (path dependency):
```toml
[dependencies]
libagent = { path = "../libagent", features = ["log"] }
```

Init a logger in your host (e.g., env_logger):
```rust
fn main() {
    env_logger::init();
    libagent::initialize();
    // ...
    libagent::stop();
}
```

## C Header
- Use `include/libagent.h` for the C API. Regenerate with cbindgen:
```sh
cbindgen --config cbindgen.toml --crate libagent --output include/libagent.h
```

## Trace Agent Proxy (UDS/Named Pipe)
- Purpose: allow embedding hosts to call the trace-agent HTTP API without bundling an HTTP client.
- Function: `ProxyTraceAgent(method, path, headers, body_ptr, body_len, on_response, on_error, user_data)`.
- Transport:
  - Unix: UDS. Path from env `LIBAGENT_TRACE_AGENT_UDS` or default `/tmp/datadog_libagent.socket`.
  - Windows: Named Pipe. Pipe name from env `LIBAGENT_TRACE_AGENT_PIPE` or default `datadog-libagent`. Full path: `\\.\\pipe\\datadog-libagent`.
- Timeout: 50 seconds for both Unix UDS and Windows Named Pipe connections.
- Headers format: one string with lines `Name: Value` separated by `\n` or `\r\n`.
- Response: delivered via callback with status (u16), headers (bytes), body (bytes) - no manual memory management!
- Callbacks: Either success or error callback is guaranteed to be called before function returns.
- Protocols: supports `Content-Length` and `Transfer-Encoding: chunked` responses.

## Testing
Run the cross‑platform integration suite:
- `cargo +nightly test -- --nocapture`

Code coverage reports are automatically generated in CI and uploaded to [Codecov](https://codecov.io/gh/tonyredondo/libagent).

Examples in multiple languages are under `examples/` (C, Go, Java/JNA, .NET, Node.js, Python, Ruby). See `examples/README.md` for build/run tips.

Note: On Rust 2024 nightly, environment mutations in tests (e.g., `std::env::set_var`) are `unsafe`; wrap them in `unsafe { ... }` or use helpers. See AGENTS.md for guidance.

## Releases
Automated builds are created for the following platforms on every commit:
- **Linux x64/arm64 (glibc)**: Standard Linux distributions
- **macOS arm64**: Apple Silicon
- **Windows x64**: MSVC

Release artifacts are automatically attached when you create a new GitHub release.

## Contributing
See AGENTS.md for project structure, style, test guidance, and PR expectations.
