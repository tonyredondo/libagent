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
    const char *method = "GET";
    const char *path = "/info";
    const char *headers = "Accept: application/json\n";

    // Make the request using callbacks - no manual memory management!
    int32_t rc = ProxyTraceAgent(method, path, headers, NULL, 0,
                                on_response, on_error, NULL);

    if (rc != 0) {
        fprintf(stderr, "ProxyTraceAgent returned error code: %d\n", rc);
        return 1;
    }

    return 0;
}
