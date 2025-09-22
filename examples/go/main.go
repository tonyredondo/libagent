package main

/*
#cgo CFLAGS: -I../../include
#cgo darwin LDFLAGS: -L../../target/debug -lagent
#cgo linux LDFLAGS: -L../../target/debug -lagent
#include "../../include/libagent.h"
*/
import "C"
import (
    "fmt"
    "unsafe"
)

func main() {
    method := C.CString("GET")
    path := C.CString("/info")
    headers := C.CString("Accept: application/json\n")
    defer C.free(unsafe.Pointer(method))
    defer C.free(unsafe.Pointer(path))
    defer C.free(unsafe.Pointer(headers))

    var resp *C.LibagentHttpResponse
    var errStr *C.char
    rc := C.ProxyTraceAgentUds(method, path, headers, nil, 0, &resp, &errStr)
    if rc != 0 {
        if errStr != nil {
            fmt.Printf("error: %s\n", C.GoString(errStr))
            C.FreeCString(errStr)
        }
        return
    }
    defer C.FreeHttpResponse(resp)

    fmt.Printf("Status: %d\n", int(resp.status))
    hdr := C.GoBytes(unsafe.Pointer(resp.headers.data), C.int(resp.headers.len))
    body := C.GoBytes(unsafe.Pointer(resp.body.data), C.int(resp.body.len))
    fmt.Printf("Headers:\n%s\n", string(hdr))
    fmt.Printf("Body:\n%s\n", string(body))
}

