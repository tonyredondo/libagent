#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include "../../include/libagent.h"

int main(void) {
    const char *method = "GET";
    const char *path = "/info";
    const char *headers = "Accept: application/json\n";

    struct LibagentHttpResponse *resp = NULL;
    char *err = NULL;

    int32_t rc = ProxyTraceAgentUds(method, path, headers, NULL, 0, &resp, &err);
    if (rc != 0) {
        fprintf(stderr, "ProxyTraceAgentUds failed (rc=%d)%s%s\n", rc, err ? ": " : "", err ? err : "");
        if (err) FreeCString(err);
        return 1;
    }

    printf("Status: %u\n", (unsigned)resp->status);
    if (resp->headers.data && resp->headers.len > 0) {
        printf("Headers:\n%.*s\n", (int)resp->headers.len, (const char*)resp->headers.data);
    }
    if (resp->body.data && resp->body.len > 0) {
        printf("Body:\n%.*s\n", (int)resp->body.len, (const char*)resp->body.data);
    }

    FreeHttpResponse(resp);
    return 0;
}

