Examples: Calling the Trace Agent Proxy FFI

These examples show how to call `ProxyTraceAgent` from various languages to proxy an HTTP request over a local IPC transport to the Datadog trace-agent.

Prerequisites
- Build the library: `cargo +nightly build` (or `--release`).
- Ensure the dynamic library can be found by your loader:
  - Linux: export `LD_LIBRARY_PATH=target/debug` (or `target/release`).
  - macOS: export `DYLD_LIBRARY_PATH=target/debug` (or `target/release`).
- Socket path (Unix): set `LIBAGENT_TRACE_AGENT_UDS` to your trace-agent UDS path if not using the default `/var/run/datadog/apm.socket`.
- Windows Named Pipe: set `LIBAGENT_TRACE_AGENT_PIPE` to the pipe name (default `trace-agent`). Timeout is enforced using a separate thread with cancellation (default: 50 seconds).

Notes
- The examples call `GET /info` with `Accept: application/json` to avoid sending large payloads.
- Cross-platform: works on Unix (UDS) and Windows (Named Pipes) platforms.

---

C (examples/c/uds_proxy.c)

Compile (Linux/macOS):

clang -I include -L target/debug -llibagent examples/c/uds_proxy.c -o examples/c/uds_proxy

Run (Unix UDS):

DYLD_LIBRARY_PATH=target/debug \
LIBAGENT_TRACE_AGENT_UDS=/var/run/datadog/apm.socket \
examples/c/uds_proxy

---

Go (cgo) (examples/go/main.go)

Build and run (module-less):

go run examples/go/main.go

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

---

.NET (examples/dotnet/Program.cs)

Build as a console app (create a project or compile directly):

dotnet new console -n UdsProxy && mv examples/dotnet/Program.cs UdsProxy/Program.cs && \
dotnet run --project UdsProxy

Ensure the loader can locate the native library (see prerequisites).

---

Node.js (examples/js/index.js)

Install deps and run:

npm install ffi-napi ref-napi ref-struct-napi && \
DYLD_LIBRARY_PATH=target/debug node examples/js/index.js

---

Python (examples/python/uds_proxy.py)

Run:

DYLD_LIBRARY_PATH=target/debug python3 examples/python/uds_proxy.py

---

Ruby (examples/ruby/uds_proxy.rb)

Install ffi gem and run:

gem install ffi && \
DYLD_LIBRARY_PATH=target/debug ruby examples/ruby/uds_proxy.rb
