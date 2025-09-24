package main

/*
#cgo CFLAGS: -I../../include
#cgo darwin LDFLAGS: -L../../target/debug -llibagent
#cgo linux LDFLAGS: -L../../target/debug -llibagent
#include "../../include/libagent.h"

// Internal callback functions for the high-level API
void internal_response_callback(uint16_t status,
                               const uint8_t* headers_data, size_t headers_len,
                               const uint8_t* body_data, size_t body_len,
                               void* user_data);

// Internal error callback
void internal_error_callback(const char* error_message, void* user_data);
*/
import "C"
import (
	"errors"
	"fmt"
	"sync"
	"time"
	"unsafe"
)

// Response represents an HTTP response
type Response struct {
	Status  uint16
	Headers string
	Body    []byte
}

// Client provides a high-level API for making requests
type Client struct{}

// RequestAsync makes an HTTP request asynchronously and returns a response or error
func (c *Client) RequestAsync(method, path, headers string, body []byte) (*Response, error) {
	// Channel to receive the result
	resultChan := make(chan *Response, 1)
	errorChan := make(chan error, 1)

	// Context to pass to callbacks
	type requestContext struct {
		resultChan chan *Response
		errorChan  chan error
		completed  bool
		mu         sync.Mutex
	}

	ctx := &requestContext{
		resultChan: resultChan,
		errorChan:  errorChan,
		completed:  false,
	}

	// Run the request in a goroutine for true async behavior
	go func() {
		// Set up context pointer for callbacks
		ctxPtr := unsafe.Pointer(ctx)

		// Callback implementations
		responseCallback := (*C.uintptr_t)(unsafe.Pointer(C.internal_response_callback))
		errorCallback := (*C.uintptr_t)(unsafe.Pointer(C.internal_error_callback))

		// Convert strings to C strings
		cMethod := C.CString(method)
		cPath := C.CString(path)
		cHeaders := C.CString(headers)
		defer C.free(unsafe.Pointer(cMethod))
		defer C.free(unsafe.Pointer(cPath))
		defer C.free(unsafe.Pointer(cHeaders))

		// Prepare body
		var bodyPtr unsafe.Pointer
		var bodyLen C.size_t
		if len(body) > 0 {
			bodyPtr = C.CBytes(body)
			bodyLen = C.size_t(len(body))
			defer C.free(bodyPtr)
		}

		// Make the request (this will block this goroutine but not the caller)
		rc := C.ProxyTraceAgent(cMethod, cPath, cHeaders, bodyPtr, bodyLen,
			responseCallback, errorCallback, ctxPtr)

		// Handle immediate errors (though callbacks should handle most cases)
		ctx.mu.Lock()
		if !ctx.completed && rc != 0 {
			ctx.completed = true
			ctx.errorChan <- fmt.Errorf("ProxyTraceAgent returned error code: %d", int(rc))
		}
		ctx.mu.Unlock()
	}()

	// Wait for either result or error
	select {
	case response := <-resultChan:
		return response, nil
	case err := <-errorChan:
		return nil, err
	case <-time.After(60 * time.Second): // Timeout after 60 seconds
		return nil, errors.New("request timed out")
	}
}

// Request makes an HTTP request synchronously (for compatibility)
func (c *Client) Request(method, path, headers string, body []byte) (*Response, error) {
	return c.RequestAsync(method, path, headers, body)
}

// Convenience methods
func (c *Client) Get(path, headers string) (*Response, error) {
	return c.Request("GET", path, headers, nil)
}

func (c *Client) Post(path, headers string, body []byte) (*Response, error) {
	return c.Request("POST", path, headers, body)
}

func (c *Client) Put(path, headers string, body []byte) (*Response, error) {
	return c.Request("PUT", path, headers, body)
}

func (c *Client) Delete(path, headers string) (*Response, error) {
	return c.Request("DELETE", path, headers, body)
}

// Global client instance
var DefaultClient = &Client{}

// Package-level convenience functions
func Request(method, path, headers string, body []byte) (*Response, error) {
	return DefaultClient.Request(method, path, headers, body)
}

func Get(path, headers string) (*Response, error) {
	return DefaultClient.Get(path, headers)
}

func Post(path, headers string, body []byte) (*Response, error) {
	return DefaultClient.Post(path, headers, body)
}

func Put(path, headers string, body []byte) (*Response, error) {
	return DefaultClient.Put(path, headers, body)
}

func Delete(path, headers string) (*Response, error) {
	return DefaultClient.Delete(path, headers)
}

//export internal_response_callback
func internal_response_callback(status C.uint16_t,
	headersData *C.uint8_t, headersLen C.size_t,
	bodyData *C.uint8_t, bodyLen C.size_t,
	userData unsafe.Pointer) {
	ctx := (*requestContext)(userData)

	ctx.mu.Lock()
	defer ctx.mu.Unlock()

	if ctx.completed {
		return // Already completed
	}
	ctx.completed = true

	// Extract headers
	headers := ""
	if headersLen > 0 {
		headersBytes := C.GoBytes(unsafe.Pointer(headersData), C.int(headersLen))
		headers = string(headersBytes)
	}

	// Extract body
	var body []byte
	if bodyLen > 0 {
		body = C.GoBytes(unsafe.Pointer(bodyData), C.int(bodyLen))
	}

	response := &Response{
		Status:  uint16(status),
		Headers: headers,
		Body:    body,
	}

	// Send result (non-blocking)
	select {
	case ctx.resultChan <- response:
	default:
		// Channel already has a value or is closed
	}
}

//export internal_error_callback
func internal_error_callback(errorMessage *C.char, userData unsafe.Pointer) {
	ctx := (*requestContext)(userData)

	ctx.mu.Lock()
	defer ctx.mu.Unlock()

	if ctx.completed {
		return // Already completed
	}
	ctx.completed = true

	errorStr := "Unknown error"
	if errorMessage != nil {
		errorStr = C.GoString(errorMessage)
	}

	// Send error (non-blocking)
	select {
	case ctx.errorChan <- errors.New(errorStr):
	default:
		// Channel already has a value or is closed
	}
}

func main() {
	fmt.Println("=== Example 1: Simple GET ===")
	response, err := Get("/info", "Accept: application/json\n")
	if err != nil {
		fmt.Printf("Error: %v\n", err)
		return
	}
	fmt.Printf("Status: %d\n", response.Status)
	fmt.Printf("Headers: %s", response.Headers)
	fmt.Printf("Body: %s\n", string(response.Body))

	fmt.Println("\n=== Example 2: POST with body ===")
	postBody := []byte(`{"key": "value"}`)
	postResponse, err := Post("/api/test", "Content-Type: application/json\nAccept: application/json\n", postBody)
	if err != nil {
		fmt.Printf("POST Error: %v\n", err)
		return
	}
	fmt.Printf("POST Status: %d\n", postResponse.Status)

	fmt.Println("\n=== Example 3: Using generic Request ===")
	customResponse, err := Request("PUT", "/api/config", "Content-Type: application/json\n",
		[]byte(`{"setting": true}`))
	if err != nil {
		fmt.Printf("PUT Error: %v\n", err)
		return
	}
	fmt.Printf("PUT Status: %d\n", customResponse.Status)
}
