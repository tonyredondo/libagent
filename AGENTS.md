# Repository Guidelines

## Project Structure & Module Organization
- For a deeper architectural description (lifecycle, backoff, platform specifics), see ARCHITECTURE.md.
- `src/` — core library code:
  - `lib.rs` (public API: `initialize`, `stop`), `ffi.rs` (C FFI: `Initialize`, `Stop`), `manager.rs` (process lifecycle), `config.rs` (constants/env overrides).
- `tests/` — integration tests (`respawn.rs`, `idempotent.rs`, `start_stop_unix.rs`, `windows_sanity.rs`) plus helpers in `tests/common/`.
- `.github/workflows/ci.yml` — GitHub Actions (Linux, macOS, Windows; nightly toolchain).
- `target/` — build artifacts.

## Build, Test, and Development Commands
- Install nightly: `rustup toolchain install nightly`.
- Build (debug/release): `cargo +nightly build` / `cargo +nightly build --release`.
- Test (show logs): `LIBAGENT_LOG=debug cargo +nightly test -- --nocapture`.
- Format: `cargo fmt --all`.
- Lint (recommended): `cargo +nightly clippy --all-targets -- -D warnings`.

Outputs include a `cdylib` for embedding and an `rlib` for Rust linking.

## Coding Style & Naming Conventions
- Rust style with `rustfmt` defaults (4-space indent, imports grouped).
- Naming: `snake_case` for fns/vars; `CamelCase` for types; modules in `snake_case`.
- FFI uses PascalCase (`Initialize`, `Stop`) intentionally; keep Rust wrappers idiomatic.
- Keep modules focused; prefer small helpers in `manager.rs` over sprawling functions.

## Testing Guidelines
- Integration tests live in `tests/` and use `serial_test` to isolate global state. Mark new stateful tests with `#[serial]`.
- Prefer integration-style tests that exercise `initialize`/`stop` and respawn/backoff behavior. Use `tempfile` and helpers in `tests/common` to avoid touching the real system.
- Cross‑platform: Unix tests rely on `/bin/sh`; Windows tests use PowerShell.
- Run locally with `cargo +nightly test`; CI runs on all three OSes.
- Rust 2024/`nightly`: environment mutations are `unsafe`. Wrap `env::set_var`/`env::remove_var` in `unsafe { ... }` in tests, and keep tests `#[serial]` to avoid races.

## Commit & Pull Request Guidelines
- Use Conventional Commits (e.g., `feat:`, `fix:`, `test:`, `chore:`). Keep subject ≤72 chars; add a focused body when helpful.
- In PRs: describe motivation and approach, link issues, include test updates/new tests, and note platform implications (Unix/Windows).
- Ensure `cargo fmt`, `cargo +nightly clippy`, and `cargo +nightly test` pass before requesting review.

## Security & Configuration Tips
- Default programs/args live in `src/config.rs`. Runtime overrides (for dev/tests): `LIBAGENT_AGENT_PROGRAM`, `LIBAGENT_AGENT_ARGS`, `LIBAGENT_TRACE_AGENT_PROGRAM`, `LIBAGENT_TRACE_AGENT_ARGS`, `LIBAGENT_MONITOR_INTERVAL_SECS`, `LIBAGENT_LOG`, `LIBAGENT_DEBUG`.
- `*_ARGS` values are parsed using shell-words. Quote arguments as you would in a shell (e.g., `LIBAGENT_AGENT_ARGS='-c "my arg"'`).
- Example: `LIBAGENT_LOG=debug LIBAGENT_MONITOR_INTERVAL_SECS=1 cargo +nightly test -- --nocapture`.

## FFI & Headers
- Nightly/edition-2024 requires `#[unsafe(no_mangle)]` on FFI exports. We keep this as-is to match current toolchain; do not change to `#[no_mangle]`.
- Generate the C header with cbindgen: `cbindgen --config cbindgen.toml --crate libagent --output include/libagent.h` (CI regenerates it and validates that it matches the committed header).
- If you change the FFI surface, regenerate and commit the updated `include/libagent.h`.
- Optional feature: enable `--features log` to route logs through the `log` crate instead of stderr. You may run tests with logging enabled: `cargo +nightly test --features log -- --nocapture`.

## Platform Notes
- Windows process management uses Job Objects for reliable termination of the child tree. For compatibility across `windows-sys` versions, `CreateJobObjectW` is declared via an `unsafe extern "system"` block; do not change its import path without verifying CI across OS/toolchains.
