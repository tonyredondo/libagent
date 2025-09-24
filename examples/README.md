Examples: Calling the Trace Agent Proxy FFI

These examples show how to call `ProxyTraceAgent` from various languages to proxy an HTTP request over a local IPC transport to the Datadog trace-agent.

Prerequisites
- Build the library: `cargo +nightly build` (or `--release`).
- Ensure the dynamic library can be found by your loader:
  - Linux: export `LD_LIBRARY_PATH=target/debug` (or `target/release`).
  - macOS: export `DYLD_LIBRARY_PATH=target/debug` (or `target/release`).
- Socket path (Unix): set `LIBAGENT_TRACE_AGENT_UDS` to your trace-agent UDS path if not using the default `/var/run/datadog/apm.socket`.
- Windows Named Pipe: set `LIBAGENT_TRACE_AGENT_PIPE` to the pipe name (default `trace-agent`). Uses a reusable worker pool (4 workers by default) to handle concurrent requests efficiently under high load, with per-request timeout support (default: 50 seconds).

Notes
- The examples call `GET /info` with `Accept: application/json` to avoid sending large payloads.
- **High-level APIs available**: Most languages now provide idiomatic async APIs (Tasks, Promises, etc.) that wrap the callback-based FFI
- **Low-level callback API**: Still available for advanced use cases requiring direct callback control
- Uses callback-based API internally - no manual memory management required!
- Cross-platform: works on Unix (UDS) and Windows (Named Pipes) platforms.

---

C (examples/c/uds_proxy.c)

Compile (Linux/macOS):

clang -I include -L target/debug -llibagent examples/c/uds_proxy.c -o examples/c/uds_proxy

**API**: Uses callback-based API directly with function pointers.

Run (Unix UDS):

DYLD_LIBRARY_PATH=target/debug \
LIBAGENT_TRACE_AGENT_UDS=/var/run/datadog/apm.socket \
examples/c/uds_proxy

---

Go (cgo) (examples/go/main.go)

Build and run (module-less):

go run examples/go/main.go

**High-level API**: Package-level functions like `Get()`, `Post()`, etc. that return `(*Response, error)` and hide callback complexity.

Adjust the `#cgo` LDFLAGS in the source if using `--release` output or a different path.

---

Go (PureGo, no cgo) (examples/go-pure/main.go)

Uses github.com/cloudflare/purego to dynamically load the shared library and call functions without cgo.

Run:

cd examples/go-pure && \
go run .

Adjust `LIBAGENT_LIB` to point to the full library path if the loader cannot find it, e.g. `export LIBAGENT_LIB=../../target/debug/libagent.dylib`.

---

Java + JNA (examples/java/JNAExample.java)

Run (add JNA to classpath):

javac -cp jna-5.13.0.jar examples/java/JNAExample.java && \
DYLD_LIBRARY_PATH=target/debug java -cp .:jna-5.13.0.jar examples.java.JNAExample

**API**: Uses callback-based API with JNA Callback interfaces.

---

.NET (examples/dotnet/Program.cs)

Build as a console app (create a project or compile directly):

dotnet new console -n UdsProxy && mv examples/dotnet/Program.cs UdsProxy/Program.cs && \
dotnet run --project UdsProxy

**High-level API**: `LibAgentClient` class provides async methods like `GetAsync()`, `PostAsync()`, etc. that return `Task<Response>`.

Ensure the loader can locate the native library (see prerequisites).

---

Node.js (examples/js/index.js & examples/js/async-worker.js)

Install deps and run:

npm install ffi-napi ref-napi && \
DYLD_LIBRARY_PATH=target/debug node examples/js/index.js

**Standard API**: `LibAgentClient` class provides promise-based methods that return Promises (synchronous FFI calls wrapped in promises).

**Truly Async API**: `AsyncLibAgentClient` in `async-worker.js` uses worker threads for non-blocking FFI calls - run with:

DYLD_LIBRARY_PATH=target/debug node examples/js/async-worker.js

---

Python (examples/python/uds_proxy.py)

Run:

DYLD_LIBRARY_PATH=target/debug python3 examples/python/uds_proxy.py

**API**: Uses callback-based API with ctypes function pointers.

---

Ruby (examples/ruby/uds_proxy.rb)

Install ffi gem and run:

gem install ffi && \
DYLD_LIBRARY_PATH=target/debug ruby examples/ruby/uds_proxy.rb

**API**: Uses callback-based API with FFI callback definitions.
