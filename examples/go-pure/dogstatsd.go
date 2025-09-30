package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"time"

	"github.com/ebitengine/purego"
)

var (
	libagent            uintptr
	initialize          func()
	stop                func()
	sendDogStatsDMetric func(payload *byte, payloadLen uintptr) int32
	getMetrics          func() MetricsData
)

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
	DogstatsdRequests       uint64
	DogstatsdSuccesses      uint64
	DogstatsdErrors         uint64
}

func init() {
	// Determine library path
	var libPath string
	if envPath := os.Getenv("LIBAGENT_LIB"); envPath != "" {
		libPath = envPath
	} else {
		// Default to ../../target/debug/liblibagent.[ext]
		var libExt string
		switch runtime.GOOS {
		case "darwin":
			libExt = "dylib"
		case "windows":
			libExt = "dll"
		default:
			libExt = "so"
		}
		libPath = filepath.Join("..", "..", "target", "debug", "liblibagent."+libExt)
	}

	var err error
	libagent, err = purego.Dlopen(libPath, purego.RTLD_NOW|purego.RTLD_GLOBAL)
	if err != nil {
		panic(fmt.Sprintf("Failed to load libagent: %v", err))
	}

	// Register functions
	purego.RegisterLibFunc(&initialize, libagent, "Initialize")
	purego.RegisterLibFunc(&stop, libagent, "Stop")
	purego.RegisterLibFunc(&sendDogStatsDMetric, libagent, "SendDogStatsDMetric")
	purego.RegisterLibFunc(&getMetrics, libagent, "GetMetrics")
}

func SendMetric(metric string) int32 {
	metricBytes := []byte(metric)
	return sendDogStatsDMetric(&metricBytes[0], uintptr(len(metricBytes)))
}

func main() {
	fmt.Println("DogStatsD Example - Sending Custom Metrics (PureGo)")
	fmt.Println("====================================================\n")

	// Initialize libagent
	initialize()
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
	metrics := getMetrics()
	fmt.Println("DogStatsD Metrics:")
	fmt.Printf("  Requests: %d\n", metrics.DogstatsdRequests)
	fmt.Printf("  Successes: %d\n", metrics.DogstatsdSuccesses)
	fmt.Printf("  Errors: %d\n\n", metrics.DogstatsdErrors)

	// Clean shutdown
	fmt.Println("Stopping libagent...")
	stop()
	fmt.Println("Done!")
}
