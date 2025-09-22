package main

import (
    "errors"
    "fmt"
    "os"
    "runtime"
    "unsafe"

    purego "github.com/cloudflare/purego"
)

// Go representations of the C structs
type libagentHttpBuffer struct {
    data *byte
    len  uintptr // size_t
}

type libagentHttpResponse struct {
    status  uint16
    headers libagentHttpBuffer
    body    libagentHttpBuffer
}

// Function pointers resolved at runtime via purego
var (
    proxyTraceAgentUds func(method, path, headers *byte, bodyPtr unsafe.Pointer, bodyLen uintptr, outResp **libagentHttpResponse, outErr **byte) int32
    freeHttpResponse   func(resp *libagentHttpResponse)
    freeCString        func(s *byte)
)

func dlopenLibagent() (uintptr, error) {
    // Allow override via env
    if name := os.Getenv("LIBAGENT_LIB"); name != "" {
        if h, err := purego.Dlopen(name, purego.RTLD_NOW|purego.RTLD_LOCAL); err == nil {
            return h, nil
        } else {
            return 0, err
        }
    }
    // Otherwise try platform defaults
    var candidates []string
    switch runtime.GOOS {
    case "darwin":
        candidates = []string{"libagent.dylib", "./libagent.dylib"}
    case "linux":
        candidates = []string{"libagent.so", "./libagent.so"}
    default:
        candidates = []string{"libagent"}
    }
    var lastErr error
    for _, c := range candidates {
        h, err := purego.Dlopen(c, purego.RTLD_NOW|purego.RTLD_LOCAL)
        if err == nil {
            return h, nil
        }
        lastErr = err
    }
    if lastErr == nil {
        lastErr = errors.New("could not load libagent")
    }
    return 0, lastErr
}

func cstr(s string) *byte {
    b := append([]byte(s), 0)
    return &b[0]
}

func ccharpToString(p *byte) string {
    if p == nil {
        return ""
    }
    // find NUL terminator
    var n uintptr
    for {
        if *(*byte)(unsafe.Pointer(uintptr(unsafe.Pointer(p)) + n)) == 0 {
            break
        }
        n++
    }
    // build slice
    hdr := unsafe.Slice(p, n)
    return string(hdr)
}

func main() {
    handle, err := dlopenLibagent()
    if err != nil {
        fmt.Println("failed to load libagent:", err)
        fmt.Println("Hint: set DYLD_LIBRARY_PATH/LD_LIBRARY_PATH or LIBAGENT_LIB to the full path of the library.")
        os.Exit(1)
    }
    // Resolve functions
    purego.RegisterFunc(&proxyTraceAgentUds, handle, "ProxyTraceAgentUds")
    purego.RegisterFunc(&freeHttpResponse, handle, "FreeHttpResponse")
    purego.RegisterFunc(&freeCString, handle, "FreeCString")

    // Build inputs
    method := cstr("GET")
    path := cstr("/info")
    headers := cstr("Accept: application/json\n")

    var resp *libagentHttpResponse
    var errStr *byte
    rc := proxyTraceAgentUds(method, path, headers, nil, 0, &resp, &errStr)
    if rc != 0 {
        if errStr != nil {
            fmt.Println("error:", ccharpToString(errStr))
            freeCString(errStr)
        } else {
            fmt.Println("error rc=", rc)
        }
        os.Exit(2)
    }
    defer freeHttpResponse(resp)

    fmt.Println("Status:", resp.status)
    if resp.headers.data != nil && resp.headers.len > 0 {
        hdr := unsafe.Slice(resp.headers.data, resp.headers.len)
        fmt.Println("Headers:\n" + string(hdr))
    }
    if resp.body.data != nil && resp.body.len > 0 {
        body := unsafe.Slice(resp.body.data, resp.body.len)
        fmt.Println("Body:\n" + string(body))
    }
}

