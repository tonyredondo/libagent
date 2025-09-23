# libagent

[![CI](https://github.com/tonyredondo/libagent/actions/workflows/ci.yml/badge.svg)](https://github.com/tonyredondo/libagent/actions/workflows/ci.yml)
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
- Configuration (`config.rs`): compile-time defaults with env overrides parsed via `shell-words`; tunables include programs, args, and monitor interval.
- FFI (`ffi.rs`): exports `Initialize`, `Stop`, and a transport-agnostic trace-agent proxy (`ProxyTraceAgentUds`) with `catch_unwind`.
  - Unix: connects over UDS.
  - Windows: connects over Windows Named Pipes.
- Logging: `LIBAGENT_LOG` level and `LIBAGENT_DEBUG` to inherit child stdout/stderr.

For a deeper dive, see ARCHITECTURE.md.

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
// Provided by libagent shared library
void Initialize(void);
void Stop(void);
// Proxy an HTTP request over Unix Domain Socket (UDS) to the trace-agent
// Returns 0 on success; negative on error. Allocates output which must be freed.
typedef struct {
  unsigned char *data;
  size_t len;
} LibagentHttpBuffer;

typedef struct {
  uint16_t status;
  LibagentHttpBuffer headers; // CRLF-separated header lines
  LibagentHttpBuffer body;
} LibagentHttpResponse;

int32_t ProxyTraceAgent(const char *method,
                           const char *path,
                           const char *headers,
                           const unsigned char *body_ptr,
                           size_t body_len,
                           LibagentHttpResponse **out_resp,
                           char **out_err);

void FreeHttpBuffer(LibagentHttpBuffer buf);
void FreeHttpResponse(LibagentHttpResponse *resp);
void FreeCString(char *s);

int main(void) {
    Initialize();
    // ... your program ...
    Stop();
    return 0;
}
```

UDS proxy notes:
- Socket path resolution (Unix): env `LIBAGENT_TRACE_AGENT_UDS` or default `/var/run/datadog/apm.socket`.
- `headers`: string with lines `Name: Value` separated by `\n` or `\r\n`.
- `out_resp->headers`: same format, concatenated CRLF lines; free with `FreeHttpBuffer` via `FreeHttpResponse`.
- Free all allocated outputs with the provided free functions.

Notes: The FFI functions return `void`. Operational errors are logged; panics in Rust are caught with `catch_unwind` to avoid unwinding across the FFI boundary.

## Configuration
Defaults live in `src/config.rs`. Override at runtime via environment variables:
- `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_AGENT_ARGS`
- `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_ARGS`
- `LIBAGENT_MONITOR_INTERVAL_SECS`
- Logging: `LIBAGENT_LOG` (error|warn|info|debug), `LIBAGENT_DEBUG` (1/true)

Notes:
- `*_ARGS` values are parsed using shell-words. Quote arguments as you would in a shell, e.g. `LIBAGENT_AGENT_ARGS='-c "my arg"'`.

Example:
```sh
LIBAGENT_AGENT_PROGRAM=/usr/bin/datadog-agent \
LIBAGENT_TRACE_AGENT_PROGRAM=/usr/bin/trace-agent \
LIBAGENT_LOG=info LIBAGENT_MONITOR_INTERVAL_SECS=1 \
cargo +nightly test -- --nocapture
```

## Logging
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
- Function: `ProxyTraceAgentUds(method, path, headers, body_ptr, body_len, out_resp, out_err)`.
- Transport:
  - Unix: UDS. Path from env `LIBAGENT_TRACE_AGENT_UDS` or default `/var/run/datadog/apm.socket`.
  - Windows: Named Pipe. Pipe name from env `LIBAGENT_TRACE_AGENT_PIPE` or default `trace-agent`. Full path: `\\.\\pipe\\trace-agent`.
- Headers format: one string with lines `Name: Value` separated by `\n` or `\r\n`.
- Response: status (u16), headers (CRLF-joined lines), body (bytes). Free with `FreeHttpResponse`.
- Protocols: supports `Content-Length` and `Transfer-Encoding: chunked` responses.

## Testing
Run the cross‑platform integration suite:
- `cargo +nightly test -- --nocapture`

Examples in multiple languages are under `examples/` (C, Go, Java/JNA, .NET, Node.js, Python, Ruby). See `examples/README.md` for build/run tips.

Note: On Rust 2024 nightly, environment mutations in tests (e.g., `std::env::set_var`) are `unsafe`; wrap them in `unsafe { ... }` or use helpers. See AGENTS.md for guidance.

## Contributing
See AGENTS.md for project structure, style, test guidance, and PR expectations.
