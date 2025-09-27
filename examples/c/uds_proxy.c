#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include "../../include/libagent.h"

// Callback function for successful responses
void on_response(uint16_t status,
                 const uint8_t* headers_data, size_t headers_len,
                 const uint8_t* body_data, size_t body_len,
                 void* user_data) {
    printf("Status: %u\n", (unsigned)status);
    if (headers_data && headers_len > 0) {
        printf("Headers:\n%.*s\n", (int)headers_len, (const char*)headers_data);
    }
    if (body_data && body_len > 0) {
        printf("Body:\n%.*s\n", (int)body_len, (const char*)body_data);
    }
}

// Callback function for errors
void on_error(const char* error_message, void* user_data) {
    fprintf(stderr, "ProxyTraceAgent failed: %s\n", error_message);
}

int main(void) {
    printf("Initializing libagent...\n");
    Initialize();

    // Get and display initial metrics
    struct MetricsData metrics = GetMetrics();
    printf("Initial metrics:\n");
    printf("  Agent spawns: %llu\n", (unsigned long long)metrics.agent_spawns);
    printf("  Trace agent spawns: %llu\n", (unsigned long long)metrics.trace_agent_spawns);
    printf("  GET requests: %llu\n", (unsigned long long)metrics.proxy_get_requests);
    printf("  Uptime: %.2f seconds\n\n", metrics.uptime_seconds);

    const char *method = "GET";
    const char *path = "/info";
    const char *headers = "Accept: application/json\n";

    printf("Making proxy request...\n");
    // Make the request using callbacks - no manual memory management!
    int32_t rc = ProxyTraceAgent(method, path, headers, NULL, 0,
                                on_response, on_error, NULL);

    if (rc != 0) {
        fprintf(stderr, "ProxyTraceAgent returned error code: %d\n", rc);
        Stop();
        return 1;
    }

    printf("\nGetting final metrics...\n");
    metrics = GetMetrics();
    printf("Final metrics:\n");
    printf("  Agent spawns: %llu\n", (unsigned long long)metrics.agent_spawns);
    printf("  Trace agent spawns: %llu\n", (unsigned long long)metrics.trace_agent_spawns);
    printf("  GET requests: %llu\n", (unsigned long long)metrics.proxy_get_requests);
    printf("  2xx responses: %llu\n", (unsigned long long)metrics.proxy_2xx_responses);
    printf("  4xx responses: %llu\n", (unsigned long long)metrics.proxy_4xx_responses);
    printf("  5xx responses: %llu\n", (unsigned long long)metrics.proxy_5xx_responses);
    printf("  Avg response time: %.2f ms\n", metrics.response_time_ema_all);
    printf("  Uptime: %.2f seconds\n", metrics.uptime_seconds);

    printf("Stopping libagent...\n");
    Stop();

    return 0;
}
