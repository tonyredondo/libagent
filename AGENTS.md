# Repository Guidelines

## Project Structure & Module Organization
- For a deeper architectural description (lifecycle, backoff, platform specifics), see ARCHITECTURE.md.
- `src/` — core library code:
  - `lib.rs` (public API: `initialize`, `stop`), `ffi.rs` (C FFI: `Initialize`, `Stop`, `ProxyTraceAgent`, `GetMetrics`), `manager.rs` (process lifecycle orchestration), `config.rs` (constants/env overrides), `http.rs` (shared HTTP parsing utilities), `uds.rs` (HTTP-over-UDS client), `winpipe.rs` (HTTP-over-Windows-Named-Pipe client).
  - `logging.rs` (centralized logging functionality), `process.rs` (platform-specific process spawning), `shutdown.rs` (platform-specific process termination), `monitor.rs` (background monitoring thread), `metrics.rs` (observability metrics tracking).
- `tests/` — integration tests (`respawn.rs`, `idempotent.rs`, `start_stop_unix.rs`, `uds_proxy.rs`, `windows_pipe_proxy.rs`, `windows_sanity.rs`) plus helpers in `tests/common/`.
- `.github/workflows/ci.yml` — GitHub Actions (Linux, macOS, Windows; nightly toolchain).
- `target/` — build artifacts.

## Build, Test, and Development Commands
- Install nightly: `rustup toolchain install nightly`.
- Build (debug/release): `cargo +nightly build` / `cargo +nightly build --release`.
- Test (show logs): `LIBAGENT_LOG=debug cargo +nightly test -- --nocapture`.
- Format: `cargo +nightly fmt --all` (use nightly to match CI formatting rules).
- Lint (recommended): `cargo +nightly clippy --all-targets -- -D warnings`.

Outputs include a `cdylib` for embedding and an `rlib` for Rust linking.

## Coding Style & Naming Conventions
- Rust style with `rustfmt` defaults (4-space indent, imports grouped).
- Naming: `snake_case` for fns/vars; `CamelCase` for types; modules in `snake_case`.
- FFI uses PascalCase (`Initialize`, `Stop`) intentionally; keep Rust wrappers idiomatic.
- Keep modules focused; prefer small helpers in `manager.rs` over sprawling functions. `initialize()` is thread-safe; feel free to call it from multiple threads so long as you avoid re-entrant env mutation races in your own code.

## Testing Guidelines
- Integration tests live in `tests/` and use `serial_test` to isolate global state. Mark new stateful tests with `#[serial]`.
- Prefer integration-style tests that exercise `initialize`/`stop` and respawn/backoff behavior. Use `tempfile` and helpers in `tests/common` to avoid touching the real system.
- Cross‑platform: Unix tests rely on `/bin/sh`; Windows tests use PowerShell.
- Run locally with `cargo +nightly test`; CI runs on all three OSes.
- Rust 2024/`nightly`: environment mutations are `unsafe`. Wrap `env::set_var`/`env::remove_var` in `unsafe { ... }` in tests, and keep tests `#[serial]` to avoid races.
 - UDS proxy tests: see `tests/uds_proxy.rs` (basic and chunked). Tests skip gracefully if UDS bind is denied by the environment.
 - Windows Named Pipe proxy tests: see `tests/windows_pipe_proxy.rs` (basic and chunked).

## Commit & Pull Request Guidelines
- Use Conventional Commits (e.g., `feat:`, `fix:`, `test:`, `chore:`). Keep subject ≤72 chars; add a focused body when helpful.
- In PRs: describe motivation and approach, link issues, include test updates/new tests, and note platform implications (Unix/Windows).
- **Quality Enforcement**: All changes must pass the following checks before committing or requesting review:
  - `cargo +nightly fmt --all -- --check` (code formatting - uses nightly to match CI)
  - `cargo +nightly clippy --all-targets -- -D warnings` (linting)
  - `cargo +nightly test -- --nocapture` (full test suite)
- Never commit code that fails these checks. Use pre-commit hooks or CI to enforce compliance.

## Security & Configuration Tips
- Default programs/args live in `src/config.rs`. Runtime overrides (for dev/tests): `LIBAGENT_AGENT_ENABLED`, `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_AGENT_ARGS`, `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_ARGS`, `LIBAGENT_MONITOR_INTERVAL_SECS`, `LIBAGENT_LOG`.
- **Trace Agent IPC-Only Operation**: The trace-agent is automatically configured for IPC-only operation (TCP port disabled) to ensure secure, local-only communication. Custom UDS/Named Pipe paths prevent conflicts with system installations.
- **Smart Process Spawning**: Trace-agent only spawns if IPC resources are available; Agent only spawns if enabled AND no existing remote configuration service is detected. Agent is disabled by default to support custom trace-agents. When agent is enabled, remote config cooperation is automatically enabled. This prevents conflicts between multiple libagent instances and respects existing Datadog installations.
- **Process Ownership Safety**: libagent only terminates processes it spawned. External processes using the same IPC resources are left untouched.
- UDS proxy: `LIBAGENT_TRACE_AGENT_UDS` overrides the Unix socket path used by both the trace-agent spawner and the proxy (default: `/tmp/datadog_libagent.socket`).
- Windows Named Pipe proxy: `LIBAGENT_TRACE_AGENT_PIPE` overrides the pipe name used by both the trace-agent spawner and the proxy on Windows (default: `datadog-libagent`, full path: `\\.\\pipe\\datadog-libagent`).
- Monitor conflict detection and readiness checks consume the same overrides so libagent never mixes endpoints across components.
- `*_ARGS` values are parsed using shell-words. Quote arguments as you would in a shell (e.g., `LIBAGENT_AGENT_ARGS='-c "my arg"'`).
- `LIBAGENT_AGENT_ENABLED` controls whether the main Datadog agent should be spawned (disabled by default for custom trace-agents).
- Example: `LIBAGENT_LOG=debug LIBAGENT_MONITOR_INTERVAL_SECS=1 cargo +nightly test -- --nocapture`.

## FFI & Headers
- Nightly/edition-2024 requires `#[unsafe(no_mangle)]` on FFI exports. We keep this as-is to match current toolchain; do not change to `#[no_mangle]`.
- Generate the C header with cbindgen: `cbindgen --config cbindgen.toml --crate libagent --output include/libagent.h` (CI regenerates it and validates that it matches the committed header).
- If you change the FFI surface, regenerate and commit the updated `include/libagent.h`.
- Optional feature: enable `--features log` to route logs through the `log` crate instead of stderr. You may run tests with logging enabled: `cargo +nightly test --features log -- --nocapture`.

### Proxy FFI
- New export: `ProxyTraceAgent(...)` proxies an HTTP request over an IPC transport to the trace-agent using callback-based API. On Unix this uses UDS; on Windows it uses Named Pipes.
- Windows note: the named-pipe client uses a reusable worker pool (4 workers by default) and overlapped I/O with cancellation to enforce the per-request timeout (default: 50 seconds) even when the server stalls.
- Callback-based API: provides `ResponseCallback` and `ErrorCallback` function pointers for handling responses/errors without manual memory management.
- Either success or error callback is guaranteed to be called before the function returns.
- See `include/libagent.h` for the callback function type definitions.
- On unsupported platforms, the function returns an error.

### Metrics FFI
- New export: `GetMetrics()` returns a `MetricsData` struct containing comprehensive metrics.
- `MetricsData` struct provides direct access to:
  - Process lifecycle metrics (agent spawns, failures, uptime)
  - HTTP proxy request counts by method (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS, OTHER)
  - HTTP proxy response counts by status code range (2xx, 3xx, 4xx, 5xx)
  - Response time moving averages (all, 2xx, 4xx, 5xx responses in milliseconds)
- Thread-safe and returns a struct copy - no memory management required.
- See `include/libagent.h` for the `MetricsData` struct definition.

## Platform Notes
- Windows process management uses Job Objects for reliable termination of the child tree. For compatibility across `windows-sys` versions, `CreateJobObjectW` is declared via an `unsafe extern "system"` block; do not change its import path without verifying CI across OS/toolchains.
