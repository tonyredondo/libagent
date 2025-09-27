import ctypes as ct
import os
import sys

# Adjust library name/path if needed
if sys.platform == 'darwin':
    libname = 'libagent.dylib'
elif sys.platform.startswith('linux'):
    libname = 'libagent.so'
else:
    libname = 'libagent'

lib = ct.CDLL(libname)

# Define callback function types
RESPONSE_CALLBACK = ct.CFUNCTYPE(None, ct.c_uint16, ct.c_void_p, ct.c_size_t, ct.c_void_p, ct.c_size_t, ct.c_void_p)
ERROR_CALLBACK = ct.CFUNCTYPE(None, ct.c_char_p, ct.c_void_p)

# Define MetricsData structure
class MetricsData(ct.Structure):
    _fields_ = [
        ("agent_spawns", ct.c_uint64),
        ("trace_agent_spawns", ct.c_uint64),
        ("agent_failures", ct.c_uint64),
        ("trace_agent_failures", ct.c_uint64),
        ("uptime_seconds", ct.c_double),
        ("proxy_get_requests", ct.c_uint64),
        ("proxy_post_requests", ct.c_uint64),
        ("proxy_put_requests", ct.c_uint64),
        ("proxy_delete_requests", ct.c_uint64),
        ("proxy_patch_requests", ct.c_uint64),
        ("proxy_head_requests", ct.c_uint64),
        ("proxy_options_requests", ct.c_uint64),
        ("proxy_other_requests", ct.c_uint64),
        ("proxy_2xx_responses", ct.c_uint64),
        ("proxy_3xx_responses", ct.c_uint64),
        ("proxy_4xx_responses", ct.c_uint64),
        ("proxy_5xx_responses", ct.c_uint64),
        ("response_time_ema_all", ct.c_double),
        ("response_time_ema_2xx", ct.c_double),
        ("response_time_ema_4xx", ct.c_double),
        ("response_time_ema_5xx", ct.c_double),
        ("response_time_sample_count", ct.c_uint64),
    ]

# Set up function signatures
lib.Initialize.argtypes = []
lib.Initialize.restype = None

lib.Stop.argtypes = []
lib.Stop.restype = None

lib.GetMetrics.argtypes = []
lib.GetMetrics.restype = MetricsData

lib.ProxyTraceAgent.argtypes = [
    ct.c_char_p, ct.c_char_p, ct.c_char_p,
    ct.c_void_p, ct.c_size_t,
    ct.c_void_p, ct.c_void_p, ct.c_void_p,  # callbacks and user_data
]
lib.ProxyTraceAgent.restype = ct.c_int32

# Global variables to store response data
response_data = {}

# Callback function for successful responses
@RESPONSE_CALLBACK
def on_response(status, headers_data, headers_len, body_data, body_len, user_data):
    print("Status:", int(status))
    if headers_data and headers_len > 0:
        headers = ct.string_at(headers_data, headers_len)
        print("Headers:\n" + headers.decode('utf-8', 'replace'))
    if body_data and body_len > 0:
        body = ct.string_at(body_data, body_len)
        print("Body:\n" + body.decode('utf-8', 'replace'))

# Callback function for errors
@ERROR_CALLBACK
def on_error(error_message, user_data):
    if error_message:
        print("error:", ct.string_at(error_message).decode('utf-8'), file=sys.stderr)
    else:
        print("error: unknown error", file=sys.stderr)

def main():
    print("Initializing libagent...")
    lib.Initialize()

    # Get and display initial metrics
    print("\n=== Initial Metrics ===")
    metrics = lib.GetMetrics()
    print(f"Agent spawns: {metrics.agent_spawns}")
    print(f"Trace agent spawns: {metrics.trace_agent_spawns}")
    print(f"Uptime: {metrics.uptime_seconds:.2f} seconds")

    # Make the request using callbacks - no manual memory management!
    print("\nMaking proxy request...")
    rc = lib.ProxyTraceAgent(
        b"GET",                           # method
        b"/info",                         # path
        b"Accept: application/json\n",    # headers
        None, 0,                          # body (none)
        on_response,                      # success callback
        on_error,                         # error callback
        None                              # user data (not used)
    )

    if rc != 0:
        print("ProxyTraceAgent returned error code:", rc, file=sys.stderr)
        lib.Stop()
        return

    # Get and display final metrics
    print("\n=== Final Metrics ===")
    metrics = lib.GetMetrics()
    print(f"Agent spawns: {metrics.agent_spawns}")
    print(f"Trace agent spawns: {metrics.trace_agent_spawns}")
    print(f"GET requests: {metrics.proxy_get_requests}")
    print(f"2xx responses: {metrics.proxy_2xx_responses}")
    print(f"4xx responses: {metrics.proxy_4xx_responses}")
    print(f"5xx responses: {metrics.proxy_5xx_responses}")
    print(f"Avg response time: {metrics.response_time_ema_all:.2f} ms")
    print(f"Uptime: {metrics.uptime_seconds:.2f} seconds")

    print("\nStopping libagent...")
    lib.Stop()

if __name__ == '__main__':
    # Example: os.environ['LIBAGENT_TRACE_AGENT_UDS'] = '/var/run/datadog/apm.socket'
    main()
