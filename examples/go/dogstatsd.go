package main

/*
#cgo CFLAGS: -I../../include
#cgo LDFLAGS: -L../../target/debug -lagent
#include "libagent.h"
#include <stdlib.h>
*/
import "C"
import (
	"fmt"
	"time"
	"unsafe"
)

// SendMetric sends a DogStatsD metric to the agent
func SendMetric(metric string) int {
	cMetric := []byte(metric)
	result := C.SendDogStatsDMetric(
		(*C.uint8_t)(unsafe.Pointer(&cMetric[0])),
		C.size_t(len(cMetric)),
	)
	return int(result)
}

func main() {
	fmt.Println("DogStatsD Example - Sending Custom Metrics")
	fmt.Println("==========================================\n")

	// Initialize libagent
	C.Initialize()
	fmt.Println("Initialized libagent\n")

	// Give the agent a moment to start
	time.Sleep(1 * time.Second)

	// Example 1: Counter - Page views
	fmt.Println("Sending counter metric (page views)...")
	result := SendMetric("example.page.views:1|c|#env:prod,service:web")
	if result == 0 {
		fmt.Println("✓ Counter sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send counter: error code %d\n\n", result)
	}

	// Example 2: Gauge - Temperature
	fmt.Println("Sending gauge metric (temperature)...")
	result = SendMetric("example.temperature:72.5|g|#location:office,sensor:A1")
	if result == 0 {
		fmt.Println("✓ Gauge sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send gauge: error code %d\n\n", result)
	}

	// Example 3: Histogram - Response time
	fmt.Println("Sending histogram metric (response time)...")
	result = SendMetric("example.request.duration:250|h|@0.5|#endpoint:/api/users")
	if result == 0 {
		fmt.Println("✓ Histogram sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send histogram: error code %d\n\n", result)
	}

	// Example 4: Distribution - Response size
	fmt.Println("Sending distribution metric (response size)...")
	result = SendMetric("example.response.size:1024|d|#status:200")
	if result == 0 {
		fmt.Println("✓ Distribution sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send distribution: error code %d\n\n", result)
	}

	// Example 5: Timing - Database query
	fmt.Println("Sending timing metric (database query)...")
	result = SendMetric("example.db.query.time:45|ms|#db:postgres,table:users")
	if result == 0 {
		fmt.Println("✓ Timing sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send timing: error code %d\n\n", result)
	}

	// Example 6: Set - Unique visitors
	fmt.Println("Sending set metric (unique visitors)...")
	result = SendMetric("example.unique.visitors:user_12345|s")
	if result == 0 {
		fmt.Println("✓ Set sent successfully\n")
	} else {
		fmt.Printf("✗ Failed to send set: error code %d\n\n", result)
	}

	// Example 7: Batched metrics
	fmt.Println("Sending batched metrics...")
	batch := `example.requests.total:1|c|#env:prod
example.cache.hits:42|c|#cache:redis
example.active.connections:100|g
example.cpu.usage:75.5|g|#host:web01`
	result = SendMetric(batch)
	if result == 0 {
		fmt.Println("✓ Batched metrics sent successfully (4 metrics)\n")
	} else {
		fmt.Printf("✗ Failed to send batched metrics: error code %d\n\n", result)
	}

	// Get metrics
	fmt.Println("Retrieving libagent metrics...")
	metrics := C.GetMetrics()
	fmt.Println("DogStatsD Metrics:")
	fmt.Printf("  Requests: %d\n", metrics.dogstatsd_requests)
	fmt.Printf("  Successes: %d\n", metrics.dogstatsd_successes)
	fmt.Printf("  Errors: %d\n\n", metrics.dogstatsd_errors)

	// Clean shutdown
	fmt.Println("Stopping libagent...")
	C.Stop()
	fmt.Println("Done!")
}
