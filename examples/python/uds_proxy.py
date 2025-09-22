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

class LibagentHttpBuffer(ct.Structure):
    _fields_ = [
        ("data", ct.c_void_p),
        ("len", ct.c_size_t),
    ]

class LibagentHttpResponse(ct.Structure):
    _fields_ = [
        ("status", ct.c_uint16),
        ("headers", LibagentHttpBuffer),
        ("body", LibagentHttpBuffer),
    ]

lib.ProxyTraceAgentUds.argtypes = [
    ct.c_char_p, ct.c_char_p, ct.c_char_p,
    ct.c_void_p, ct.c_size_t,
    ct.POINTER(ct.POINTER(LibagentHttpResponse)),
    ct.POINTER(ct.c_char_p),
]
lib.ProxyTraceAgentUds.restype = ct.c_int32

lib.FreeHttpResponse.argtypes = [ct.POINTER(LibagentHttpResponse)]
lib.FreeHttpResponse.restype = None

lib.FreeCString.argtypes = [ct.c_char_p]
lib.FreeCString.restype = None

def main():
    out_resp = ct.POINTER(LibagentHttpResponse)()
    out_err = ct.c_char_p()
    rc = lib.ProxyTraceAgentUds(b"GET", b"/info", b"Accept: application/json\n", None, 0, ct.byref(out_resp), ct.byref(out_err))
    if rc != 0:
        if out_err:
            print("error:", out_err.decode('utf-8'), file=sys.stderr)
            lib.FreeCString(out_err)
        else:
            print("error rc=", rc, file=sys.stderr)
        return

    resp = out_resp.contents
    print("Status:", int(resp.status))
    if resp.headers.data and resp.headers.len:
        headers = ct.string_at(resp.headers.data, resp.headers.len)
        print("Headers:\n" + headers.decode('utf-8', 'replace'))
    if resp.body.data and resp.body.len:
        body = ct.string_at(resp.body.data, resp.body.len)
        print("Body:\n" + body.decode('utf-8', 'replace'))
    lib.FreeHttpResponse(out_resp)

if __name__ == '__main__':
    # Example: os.environ['LIBAGENT_TRACE_AGENT_UDS'] = '/var/run/datadog/apm.socket'
    main()

