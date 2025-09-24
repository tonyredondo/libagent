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

# Set up function signature
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
    # Make the request using callbacks - no manual memory management!
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

if __name__ == '__main__':
    # Example: os.environ['LIBAGENT_TRACE_AGENT_UDS'] = '/var/run/datadog/apm.socket'
    main()
