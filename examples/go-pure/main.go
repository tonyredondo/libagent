package main

import (
	"errors"
	"fmt"
	"os"
	"runtime"
	"unsafe"

	purego "github.com/cloudflare/purego"
)

// Callback function types
type ResponseCallback func(status uint16, headersData *byte, headersLen uintptr, bodyData *byte, bodyLen uintptr, userData unsafe.Pointer)
type ErrorCallback func(errorMessage *byte, userData unsafe.Pointer)

// Function pointers resolved at runtime via purego
var (
	proxyTraceAgent func(method, path, headers *byte, bodyPtr unsafe.Pointer, bodyLen uintptr, onResponse, onError, userData unsafe.Pointer) int32
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

// Callback implementations
func onResponse(status uint16, headersData *byte, headersLen uintptr, bodyData *byte, bodyLen uintptr, userData unsafe.Pointer) {
	fmt.Println("Status:", status)
	if headersData != nil && headersLen > 0 {
		headers := unsafe.Slice(headersData, headersLen)
		fmt.Println("Headers:\n" + string(headers))
	}
	if bodyData != nil && bodyLen > 0 {
		body := unsafe.Slice(bodyData, bodyLen)
		fmt.Println("Body:\n" + string(body))
	}
}

func onError(errorMessage *byte, userData unsafe.Pointer) {
	if errorMessage != nil {
		// Find NUL terminator
		var n uintptr
		for {
			if *(*byte)(unsafe.Pointer(uintptr(unsafe.Pointer(errorMessage)) + n)) == 0 {
				break
			}
			n++
		}
		// Build slice
		msg := unsafe.Slice(errorMessage, n)
		fmt.Println("error:", string(msg))
	} else {
		fmt.Println("error: unknown error")
	}
}

func main() {
	handle, err := dlopenLibagent()
	if err != nil {
		fmt.Println("failed to load libagent:", err)
		fmt.Println("Hint: set DYLD_LIBRARY_PATH/LD_LIBRARY_PATH or LIBAGENT_LIB to the full path of the library.")
		os.Exit(1)
	}

	// Resolve function
	purego.RegisterFunc(&proxyTraceAgent, handle, "ProxyTraceAgent")

	// Build inputs
	method := cstr("GET")
	path := cstr("/info")
	headers := cstr("Accept: application/json\n")

	// Create callback function pointers
	responseCallback := purego.NewCallback(onResponse)
	errorCallback := purego.NewCallback(onError)
	defer purego.DeleteCallback(responseCallback)
	defer purego.DeleteCallback(errorCallback)

	// Make the request using callbacks - no manual memory management!
	rc := proxyTraceAgent(method, path, headers, nil, 0, responseCallback, errorCallback, nil)

	if rc != 0 {
		fmt.Println("ProxyTraceAgent returned error code:", rc)
		os.Exit(2)
	}
}
