package examples.java;

import com.sun.jna.*;

public class JNAExample {
    // Callback interfaces
    public interface ResponseCallback extends Callback {
        void invoke(short status,
                   Pointer headersData, NativeLong headersLen,
                   Pointer bodyData, NativeLong bodyLen,
                   Pointer userData);
    }

    public interface ErrorCallback extends Callback {
        void invoke(Pointer errorMessage, Pointer userData);
    }

    // Metrics data structure matching the C API
    public static class MetricsData extends Structure {
        public long agent_spawns;
        public long trace_agent_spawns;
        public long agent_failures;
        public long trace_agent_failures;
        public double uptime_seconds;

        public long proxy_get_requests;
        public long proxy_post_requests;
        public long proxy_put_requests;
        public long proxy_delete_requests;
        public long proxy_patch_requests;
        public long proxy_head_requests;
        public long proxy_options_requests;
        public long proxy_other_requests;

        public long proxy_2xx_responses;
        public long proxy_3xx_responses;
        public long proxy_4xx_responses;
        public long proxy_5xx_responses;

        public double response_time_ema_all;
        public double response_time_ema_2xx;
        public double response_time_ema_4xx;
        public double response_time_ema_5xx;
        public long response_time_sample_count;

        @Override
        protected java.util.List<String> getFieldOrder() {
            return java.util.Arrays.asList(
                "agent_spawns", "trace_agent_spawns", "agent_failures", "trace_agent_failures", "uptime_seconds",
                "proxy_get_requests", "proxy_post_requests", "proxy_put_requests", "proxy_delete_requests",
                "proxy_patch_requests", "proxy_head_requests", "proxy_options_requests", "proxy_other_requests",
                "proxy_2xx_responses", "proxy_3xx_responses", "proxy_4xx_responses", "proxy_5xx_responses",
                "response_time_ema_all", "response_time_ema_2xx", "response_time_ema_4xx", "response_time_ema_5xx",
                "response_time_sample_count"
            );
        }
    }

    public interface LibAgent extends Library {
        LibAgent INSTANCE = Native.load("agent", LibAgent.class);

        void Initialize();
        void Stop();
        MetricsData GetMetrics();

        int ProxyTraceAgent(String method,
                           String path,
                           String headers,
                           Pointer bodyPtr,
                           NativeLong bodyLen,
                           ResponseCallback onResponse,
                           ErrorCallback onError,
                           Pointer userData);
    }

    // Callback implementations
    public static class ResponseCallbackImpl implements ResponseCallback {
        @Override
        public void invoke(short status,
                          Pointer headersData, NativeLong headersLen,
                          Pointer bodyData, NativeLong bodyLen,
                          Pointer userData) {
            System.out.println("Status: " + (status & 0xFFFF));
            if (headersData != null && headersLen.intValue() > 0) {
                String headers = headersData.getString(0);
                System.out.println("Headers:\n" + headers);
            }
            if (bodyData != null && bodyLen.intValue() > 0) {
                byte[] body = bodyData.getByteArray(0, bodyLen.intValue());
                System.out.println("Body:\n" + new String(body));
            }
        }
    }

    public static class ErrorCallbackImpl implements ErrorCallback {
        @Override
        public void invoke(Pointer errorMessage, Pointer userData) {
            if (errorMessage != null) {
                System.err.println("error: " + errorMessage.getString(0));
            } else {
                System.err.println("error: unknown error");
            }
        }
    }

    public static void main(String[] args) {
        System.out.println("Initializing libagent...");
        LibAgent.INSTANCE.Initialize();

        // Get and display initial metrics
        System.out.println("\n=== Initial Metrics ===");
        MetricsData metrics = LibAgent.INSTANCE.GetMetrics();
        System.out.println("Agent spawns: " + metrics.agent_spawns);
        System.out.println("Trace agent spawns: " + metrics.trace_agent_spawns);
        System.out.println("Uptime: " + String.format("%.2f", metrics.uptime_seconds) + " seconds");

        // Create callback instances
        ResponseCallbackImpl responseCallback = new ResponseCallbackImpl();
        ErrorCallbackImpl errorCallback = new ErrorCallbackImpl();

        // Make the request using callbacks - no manual memory management!
        System.out.println("\nMaking proxy request...");
        int rc = LibAgent.INSTANCE.ProxyTraceAgent(
                "GET",                           // method
                "/info",                         // path
                "Accept: application/json\n",    // headers
                Pointer.NULL,                    // body (none)
                new NativeLong(0),               // body length
                responseCallback,                // success callback
                errorCallback,                   // error callback
                Pointer.NULL                     // user data (not used)
        );

        if (rc != 0) {
            System.err.println("ProxyTraceAgent returned error code: " + rc);
            LibAgent.INSTANCE.Stop();
            return;
        }

        // Get and display final metrics
        System.out.println("\n=== Final Metrics ===");
        metrics = LibAgent.INSTANCE.GetMetrics();
        System.out.println("Agent spawns: " + metrics.agent_spawns);
        System.out.println("Trace agent spawns: " + metrics.trace_agent_spawns);
        System.out.println("GET requests: " + metrics.proxy_get_requests);
        System.out.println("2xx responses: " + metrics.proxy_2xx_responses);
        System.out.println("4xx responses: " + metrics.proxy_4xx_responses);
        System.out.println("5xx responses: " + metrics.proxy_5xx_responses);
        System.out.println("Avg response time: " + String.format("%.2f", metrics.response_time_ema_all) + " ms");
        System.out.println("Uptime: " + String.format("%.2f", metrics.uptime_seconds) + " seconds");

        System.out.println("\nStopping libagent...");
        LibAgent.INSTANCE.Stop();
    }
}
