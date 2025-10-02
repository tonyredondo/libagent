/* Functional test for Alpine libagent builds
 * 
 * This is a simplified version of functional-test.c specifically for
 * Alpine/musl builds during Docker build process.
 * 
 * This test verifies that the optimized Alpine libraries:
 * 1. Can be loaded dynamically
 * 2. Export all required FFI functions
 * 3. Execute without crashing
 * 4. Return expected data types and values
 */

#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <unistd.h>

// Inline the MetricsData struct definition (from libagent.h)
typedef struct MetricsData {
  uint64_t agent_spawns;
  uint64_t trace_agent_spawns;
  uint64_t agent_failures;
  uint64_t trace_agent_failures;
  double uptime_seconds;
  uint64_t proxy_get_requests;
  uint64_t proxy_post_requests;
  uint64_t proxy_put_requests;
  uint64_t proxy_delete_requests;
  uint64_t proxy_patch_requests;
  uint64_t proxy_head_requests;
  uint64_t proxy_options_requests;
  uint64_t proxy_other_requests;
  uint64_t proxy_2xx_responses;
  uint64_t proxy_3xx_responses;
  uint64_t proxy_4xx_responses;
  uint64_t proxy_5xx_responses;
  double response_time_ema_all;
  double response_time_ema_2xx;
  double response_time_ema_4xx;
  double response_time_ema_5xx;
  uint64_t response_time_sample_count;
  uint64_t dogstatsd_requests;
  uint64_t dogstatsd_successes;
  uint64_t dogstatsd_errors;
} MetricsData;

// Declare FFI functions
extern void Initialize(void);
extern void Stop(void);
extern struct MetricsData GetMetrics(void);
extern int32_t SendDogStatsDMetric(const uint8_t *payload_ptr, size_t payload_len);

int main(void) {
    int test_failures = 0;

    printf("═══════════════════════════════════════════════════════════\n");
    printf("        ALPINE LIBAGENT FUNCTIONAL TEST\n");
    printf("═══════════════════════════════════════════════════════════\n\n");

    // Test 1: Initialize
    printf("Test 1: Initialize() function\n");
    printf("  Calling Initialize()...\n");
    Initialize();
    printf("  ✓ Initialize() completed without crash\n\n");

    // Test 2: GetMetrics
    printf("Test 2: GetMetrics() function\n");
    printf("  Calling GetMetrics()...\n");
    struct MetricsData metrics = GetMetrics();
    printf("  ✓ GetMetrics() completed without crash\n");
    printf("  Metrics returned:\n");
    printf("    agent_spawns: %llu\n", (unsigned long long)metrics.agent_spawns);
    printf("    trace_agent_spawns: %llu\n", (unsigned long long)metrics.trace_agent_spawns);
    printf("    agent_failures: %llu\n", (unsigned long long)metrics.agent_failures);
    printf("    trace_agent_failures: %llu\n", (unsigned long long)metrics.trace_agent_failures);
    printf("    uptime_seconds: %.2f\n", metrics.uptime_seconds);
    
    // Verify uptime is reasonable (should be > 0 and < 10 seconds)
    if (metrics.uptime_seconds < 0.0 || metrics.uptime_seconds > 10.0) {
        printf("  ✗ FAIL: Uptime is unreasonable: %.2f seconds\n", metrics.uptime_seconds);
        test_failures++;
    } else {
        printf("  ✓ Uptime is reasonable: %.2f seconds\n", metrics.uptime_seconds);
    }
    printf("\n");

    // Test 3: SendDogStatsDMetric
    printf("Test 3: SendDogStatsDMetric() function\n");
    const char *metric = "test.metric:1|c";
    printf("  Calling SendDogStatsDMetric() with: '%s'\n", metric);
    int32_t result = SendDogStatsDMetric((const uint8_t*)metric, strlen(metric));
    printf("  ✓ SendDogStatsDMetric() completed without crash\n");
    printf("  Return code: %d\n", result);
    // Note: return code might be -2 (send error) if no DogStatsD is running, that's okay
    if (result == 0) {
        printf("  ✓ Metric sent successfully\n");
    } else if (result == -2) {
        printf("  ℹ Metric send failed (expected - no DogStatsD running)\n");
    } else {
        printf("  ✗ FAIL: Unexpected return code: %d\n", result);
        test_failures++;
    }
    printf("\n");

    // Test 4: GetMetrics again (verify uptime increased)
    printf("Test 4: Verify metrics update over time\n");
    printf("  Sleeping for 0.1 seconds...\n");
    usleep(100000); // 100ms
    struct MetricsData metrics2 = GetMetrics();
    printf("  Second GetMetrics() call:\n");
    printf("    uptime_seconds: %.2f\n", metrics2.uptime_seconds);
    
    if (metrics2.uptime_seconds > metrics.uptime_seconds) {
        printf("  ✓ Uptime increased (%.2f -> %.2f seconds)\n", 
               metrics.uptime_seconds, metrics2.uptime_seconds);
    } else {
        printf("  ✗ FAIL: Uptime did not increase\n");
        test_failures++;
    }
    printf("\n");

    // Test 5: Stop
    printf("Test 5: Stop() function\n");
    printf("  Calling Stop()...\n");
    Stop();
    printf("  ✓ Stop() completed without crash\n\n");

    // Final summary
    printf("═══════════════════════════════════════════════════════════\n");
    if (test_failures == 0) {
        printf("        ✅ ALL FUNCTIONAL TESTS PASSED\n");
    } else {
        printf("        ❌ %d TEST(S) FAILED\n", test_failures);
    }
    printf("═══════════════════════════════════════════════════════════\n");

    return test_failures > 0 ? 1 : 0;
}

