# Architecture Overview

This document explains how libagent is structured, how it manages the Agent and Trace Agent processes, and the key behaviors across platforms.

## Goals
- Start and keep Datadog Agent + Trace Agent running while the library is loaded.
- Recover from child exits with exponential backoff.
- Stop cleanly and reliably terminate process trees on unload or explicit stop.
- Provide both Rust API and stable C FFI.

## Components
- Public API: `src/lib.rs` re-exports `initialize`/`stop` and registers a destructor that calls `stop` on unload.
- Process Manager: `src/manager.rs` implements spawn, monitoring/respawn, and shutdown.
- Configuration: `src/config.rs` contains defaults and environment variable overrides (parsed with shell-words).
- FFI: `src/ffi.rs` exposes `Initialize` and `Stop` for C consumers; see `include/libagent.h`.

## Lifecycle
1. Initialize: `initialize()` sets up a singleton `AgentManager` and starts both processes immediately, then launches a monitor thread.
2. Monitor: A background loop ticks at `LIBAGENT_MONITOR_INTERVAL_SECS` (default 1s), checking each child:
   - If running: no-op.
   - If exited or failed to spawn: log and schedule respawn with exponential backoff (`BACKOFF_INITIAL_SECS=1`, doubling up to `BACKOFF_MAX_SECS=30`).
   - Sleep duration adapts to the next backoff deadline so respawns aren’t delayed unnecessarily.
3. Stop: `stop()` signals the monitor thread to exit and terminates both child trees (platform-specific mechanics below). Multiple calls are safe.
4. Destructor: On library unload, the destructor calls `stop()` to ensure cleanup.

## Diagrams

Flow overview

```mermaid
flowchart TD
    init["initialize"] --> spawn_agent["spawn agent"]
    init --> spawn_trace["spawn trace agent"]
    spawn_agent --> monitor["monitor loop"]
    spawn_trace --> monitor

    monitor --> check_agent["check agent status"]
    monitor --> check_trace["check trace agent status"]

    check_agent -->|running| noop1["no-op"]
    check_agent -->|exited or error| respawn_agent["spawn agent"]
    respawn_agent -->|ok| reset1["reset backoff"]
    respawn_agent -->|fail| backoff1["set next_attempt; increase backoff"]

    check_trace -->|running| noop2["no-op"]
    check_trace -->|exited or error| respawn_trace["spawn trace agent"]
    respawn_trace -->|ok| reset2["reset backoff"]
    respawn_trace -->|fail| backoff2["set next_attempt; increase backoff"]

    monitor --> sleep["sleep until min(next_attempts, monitor interval)"]
    sleep --> monitor

    stop["stop"] --> signal["signal monitor to exit"]
    signal --> shutdown{"platform shutdown"}
    shutdown -->|Unix| unix["SIGTERM group; wait; SIGKILL if needed"]
    shutdown -->|Windows| win["Terminate Job; Close handle"]
```

Backoff timing (example)

```
Attempt 1: start -> fail -> next_attempt = now + 1s; backoff = 2s
Attempt 2: (≥1s) start -> fail -> next_attempt = now + 2s; backoff = 4s
Attempt 3: (≥2s) start -> ok   -> backoff reset to 1s
```

## Unix Details
- Children are placed into a new session/process group via `setsid()` in `pre_exec` so we can signal the whole tree.
- Shutdown sends `SIGTERM` to the negative PGID (process group), waits up to `GRACEFUL_SHUTDOWN_TIMEOUT_SECS` (default 5s), then escalates to `SIGKILL` if needed.
- Child stdio: inherited when debug-level logging is enabled; otherwise set to null to avoid noisy hosts.

## Windows Details
- A Job Object is created and configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- Each child process is assigned to the Job; on stop, the Job is terminated to kill the entire tree, and the handle is closed.
- `CreateJobObjectW` is declared via an `unsafe extern "system"` block to maintain compatibility across `windows-sys` versions.

## Configuration
- Defaults live in `src/config.rs`:
  - Agent program/args: `datadog-agent`, empty args.
  - Trace Agent program/args: `trace-agent`, empty args.
  - Monitor interval: 1s.
- Runtime overrides via environment variables (parsed with shell-words):
  - `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_AGENT_ARGS`
  - `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_ARGS`
  - `LIBAGENT_MONITOR_INTERVAL_SECS`
- Example: `LIBAGENT_AGENT_ARGS='-c "quoted arg"'`

## Logging
- Library log level: `LIBAGENT_LOG` set to `error|warn|info|debug`. `LIBAGENT_DEBUG=1` also forces debug level and inherits child stdout/stderr.
- By default, logs go to stderr; with the `log` feature enabled, logs route through the Rust `log` facade instead.
- Note: log level and debug checks are cached with `OnceLock`, so changes to env vars after first read do not take effect within the same process.

## Idempotency and Safety
- `initialize()` and `stop()` are idempotent.
- FFI functions catch panics with `catch_unwind` to prevent unwinding across the FFI boundary.

## FFI Surface
- C API: `Initialize(void)` and `Stop(void)` (see `include/libagent.h`).
- Rust nightly 2024 uses `#[unsafe(no_mangle)]` on FFI exports to match the current toolchain.

## Testing Strategy
- Integration tests under `tests/` use temporary stub scripts to simulate child behavior.
- `serial_test` isolates global state; environment mutations are marked `unsafe` due to Rust 2024 nightly constraints.
- Cross‑platform coverage:
  - Unix: start/stop lifecycle and respawn/backoff behavior.
  - Windows: sanity check that Job-based shutdown works.

## When to Update This Doc
- Changing process lifecycle or shutdown semantics in `src/manager.rs`.
- Adding/removing environment variables or defaults in `src/config.rs`.
- Modifying the FFI surface or header generation.
