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

// Metrics data structure matching the C API
type MetricsData struct {
	AgentSpawns             uint64
	TraceAgentSpawns        uint64
	AgentFailures           uint64
	TraceAgentFailures      uint64
	UptimeSeconds           float64
	ProxyGetRequests        uint64
	ProxyPostRequests       uint64
	ProxyPutRequests        uint64
	ProxyDeleteRequests     uint64
	ProxyPatchRequests      uint64
	ProxyHeadRequests       uint64
	ProxyOptionsRequests    uint64
	ProxyOtherRequests      uint64
	Proxy2xxResponses       uint64
	Proxy3xxResponses       uint64
	Proxy4xxResponses       uint64
	Proxy5xxResponses       uint64
	ResponseTimeEmaAll      float64
	ResponseTimeEma2xx      float64
	ResponseTimeEma4xx      float64
	ResponseTimeEma5xx      float64
	ResponseTimeSampleCount uint64
}

// Function pointers resolved at runtime via purego
var (
	initialize      func()
	stop            func()
	getMetrics      func() MetricsData
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

	// Resolve functions
	purego.RegisterFunc(&initialize, handle, "Initialize")
	purego.RegisterFunc(&stop, handle, "Stop")
	purego.RegisterFunc(&getMetrics, handle, "GetMetrics")
	purego.RegisterFunc(&proxyTraceAgent, handle, "ProxyTraceAgent")

	fmt.Println("Initializing libagent...")
	initialize()

	// Get and display initial metrics
	fmt.Println("\n=== Initial Metrics ===")
	metrics := getMetrics()
	fmt.Printf("Agent spawns: %d\n", metrics.AgentSpawns)
	fmt.Printf("Trace agent spawns: %d\n", metrics.TraceAgentSpawns)
	fmt.Printf("Uptime: %.2f seconds\n", metrics.UptimeSeconds)

	// Build inputs
	method := cstr("GET")
	path := cstr("/info")
	headers := cstr("Accept: application/json\n")

	// Create callback function pointers
	responseCallback := purego.NewCallback(onResponse)
	errorCallback := purego.NewCallback(onError)
	defer purego.DeleteCallback(responseCallback)
	defer purego.DeleteCallback(errorCallback)

	fmt.Println("\nMaking proxy request...")
	// Make the request using callbacks - no manual memory management!
	rc := proxyTraceAgent(method, path, headers, nil, 0, responseCallback, errorCallback, nil)

	if rc != 0 {
		fmt.Println("ProxyTraceAgent returned error code:", rc)
		stop()
		os.Exit(2)
	}

	// Get and display final metrics
	fmt.Println("\n=== Final Metrics ===")
	metrics = getMetrics()
	fmt.Printf("Agent spawns: %d\n", metrics.AgentSpawns)
	fmt.Printf("Trace agent spawns: %d\n", metrics.TraceAgentSpawns)
	fmt.Printf("GET requests: %d\n", metrics.ProxyGetRequests)
	fmt.Printf("2xx responses: %d\n", metrics.Proxy2xxResponses)
	fmt.Printf("4xx responses: %d\n", metrics.Proxy4xxResponses)
	fmt.Printf("5xx responses: %d\n", metrics.Proxy5xxResponses)
	fmt.Printf("Avg response time: %.2f ms\n", metrics.ResponseTimeEmaAll)
	fmt.Printf("Uptime: %.2f seconds\n", metrics.UptimeSeconds)

	fmt.Println("\nStopping libagent...")
	stop()
}
