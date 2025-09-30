/* Example: Send DogStatsD metrics from C using libagent
 * 
 * Compile (macOS):
 *   clang -I../../include -L../../target/debug -lagent -o dogstatsd dogstatsd.c
 *   DYLD_LIBRARY_PATH=../../target/debug ./dogstatsd
 *
 * Compile (Linux):
 *   gcc -I../../include -L../../target/debug -lagent -o dogstatsd dogstatsd.c
 *   LD_LIBRARY_PATH=../../target/debug ./dogstatsd
 */

#include "libagent.h"
#include <stdio.h>
#include <string.h>
#include <unistd.h>

int main() {
    printf("DogStatsD Example - Sending Custom Metrics\n");
    printf("==========================================\n\n");

    // Initialize libagent (starts the trace-agent/agent if needed)
    Initialize();
    printf("Initialized libagent\n\n");

    // Give the agent a moment to start
    sleep(1);

    // Example 1: Counter - Count page views
    printf("Sending counter metric (page views)...\n");
    const char* counter = "example.page.views:1|c|#env:prod,service:web";
    int32_t result = SendDogStatsDMetric((const uint8_t*)counter, strlen(counter));
    if (result == 0) {
        printf("✓ Counter sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send counter: error code %d\n\n", result);
    }

    // Example 2: Gauge - Report current temperature
    printf("Sending gauge metric (temperature)...\n");
    const char* gauge = "example.temperature:72.5|g|#location:office,sensor:A1";
    result = SendDogStatsDMetric((const uint8_t*)gauge, strlen(gauge));
    if (result == 0) {
        printf("✓ Gauge sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send gauge: error code %d\n\n", result);
    }

    // Example 3: Histogram - Response time with sampling
    printf("Sending histogram metric (response time)...\n");
    const char* histogram = "example.request.duration:250|h|@0.5|#endpoint:/api/users";
    result = SendDogStatsDMetric((const uint8_t*)histogram, strlen(histogram));
    if (result == 0) {
        printf("✓ Histogram sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send histogram: error code %d\n\n", result);
    }

    // Example 4: Distribution - Response size
    printf("Sending distribution metric (response size)...\n");
    const char* distribution = "example.response.size:1024|d|#status:200";
    result = SendDogStatsDMetric((const uint8_t*)distribution, strlen(distribution));
    if (result == 0) {
        printf("✓ Distribution sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send distribution: error code %d\n\n", result);
    }

    // Example 5: Timing - Query execution time
    printf("Sending timing metric (database query)...\n");
    const char* timing = "example.db.query.time:45|ms|#db:postgres,table:users";
    result = SendDogStatsDMetric((const uint8_t*)timing, strlen(timing));
    if (result == 0) {
        printf("✓ Timing sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send timing: error code %d\n\n", result);
    }

    // Example 6: Set - Unique visitors
    printf("Sending set metric (unique visitors)...\n");
    const char* set = "example.unique.visitors:user_12345|s";
    result = SendDogStatsDMetric((const uint8_t*)set, strlen(set));
    if (result == 0) {
        printf("✓ Set sent successfully\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send set: error code %d\n\n", result);
    }

    // Example 7: Batched metrics - Send multiple metrics at once
    printf("Sending batched metrics...\n");
    const char* batch = 
        "example.requests.total:1|c|#env:prod\n"
        "example.cache.hits:42|c|#cache:redis\n"
        "example.active.connections:100|g\n"
        "example.cpu.usage:75.5|g|#host:web01";
    result = SendDogStatsDMetric((const uint8_t*)batch, strlen(batch));
    if (result == 0) {
        printf("✓ Batched metrics sent successfully (4 metrics)\n\n");
    } else {
        fprintf(stderr, "✗ Failed to send batched metrics: error code %d\n\n", result);
    }

    // Get and display metrics
    printf("Retrieving libagent metrics...\n");
    MetricsData metrics = GetMetrics();
    printf("DogStatsD Metrics:\n");
    printf("  Requests: %llu\n", metrics.dogstatsd_requests);
    printf("  Successes: %llu\n", metrics.dogstatsd_successes);
    printf("  Errors: %llu\n", metrics.dogstatsd_errors);
    printf("\n");

    // Clean shutdown
    printf("Stopping libagent...\n");
    Stop();
    printf("Done!\n");

    return 0;
}
