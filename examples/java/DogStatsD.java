import com.sun.jna.*;
import com.sun.jna.ptr.PointerByReference;
import java.nio.charset.StandardCharsets;

/**
 * Example: Send DogStatsD metrics from Java using libagent
 * 
 * Compile and run:
 *   javac -cp jna-5.13.0.jar DogStatsD.java
 *   java -cp .:jna-5.13.0.jar -Djna.library.path=../../target/debug DogStatsD
 */
public class DogStatsD {

    public interface LibAgent extends Library {
        LibAgent INSTANCE = Native.load("libagent", LibAgent.class);

        void Initialize();
        void Stop();
        int SendDogStatsDMetric(byte[] payload, NativeLong payloadLen);
        MetricsData GetMetrics();
    }

    @Structure.FieldOrder({
        "agent_spawns", "trace_agent_spawns", "agent_failures", "trace_agent_failures",
        "uptime_seconds", "proxy_get_requests", "proxy_post_requests", "proxy_put_requests",
        "proxy_delete_requests", "proxy_patch_requests", "proxy_head_requests",
        "proxy_options_requests", "proxy_other_requests", "proxy_2xx_responses",
        "proxy_3xx_responses", "proxy_4xx_responses", "proxy_5xx_responses",
        "response_time_ema_all", "response_time_ema_2xx", "response_time_ema_4xx",
        "response_time_ema_5xx", "response_time_sample_count",
        "dogstatsd_requests", "dogstatsd_successes", "dogstatsd_errors"
    })
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
        public long dogstatsd_requests;
        public long dogstatsd_successes;
        public long dogstatsd_errors;
    }

    private static int sendMetric(String metric) {
        byte[] metricBytes = metric.getBytes(StandardCharsets.UTF_8);
        return LibAgent.INSTANCE.SendDogStatsDMetric(metricBytes, new NativeLong(metricBytes.length));
    }

    public static void main(String[] args) throws InterruptedException {
        System.out.println("DogStatsD Example - Sending Custom Metrics");
        System.out.println("==========================================\n");

        // Initialize libagent
        LibAgent.INSTANCE.Initialize();
        System.out.println("Initialized libagent\n");

        // Give the agent a moment to start
        Thread.sleep(1000);

        // Example 1: Counter - Page views
        System.out.println("Sending counter metric (page views)...");
        int result = sendMetric("example.page.views:1|c|#env:prod,service:web");
        System.out.println("✓ Counter sent: " + (result == 0) + "\n");

        // Example 2: Gauge - Temperature
        System.out.println("Sending gauge metric (temperature)...");
        result = sendMetric("example.temperature:72.5|g|#location:office,sensor:A1");
        System.out.println("✓ Gauge sent: " + (result == 0) + "\n");

        // Example 3: Histogram - Response time
        System.out.println("Sending histogram metric (response time)...");
        result = sendMetric("example.request.duration:250|h|@0.5|#endpoint:/api/users");
        System.out.println("✓ Histogram sent: " + (result == 0) + "\n");

        // Example 4: Distribution - Response size
        System.out.println("Sending distribution metric (response size)...");
        result = sendMetric("example.response.size:1024|d|#status:200");
        System.out.println("✓ Distribution sent: " + (result == 0) + "\n");

        // Example 5: Timing - Database query
        System.out.println("Sending timing metric (database query)...");
        result = sendMetric("example.db.query.time:45|ms|#db:postgres,table:users");
        System.out.println("✓ Timing sent: " + (result == 0) + "\n");

        // Example 6: Set - Unique visitors
        System.out.println("Sending set metric (unique visitors)...");
        result = sendMetric("example.unique.visitors:user_12345|s");
        System.out.println("✓ Set sent: " + (result == 0) + "\n");

        // Example 7: Batched metrics
        System.out.println("Sending batched metrics...");
        String batch = "example.requests.total:1|c|#env:prod\n" +
                      "example.cache.hits:42|c|#cache:redis\n" +
                      "example.active.connections:100|g\n" +
                      "example.cpu.usage:75.5|g|#host:web01";
        result = sendMetric(batch);
        System.out.println("✓ Batched metrics sent (4 metrics): " + (result == 0) + "\n");

        // Get metrics
        System.out.println("Retrieving libagent metrics...");
        MetricsData metrics = LibAgent.INSTANCE.GetMetrics();
        System.out.println("DogStatsD Metrics:");
        System.out.println("  Requests: " + metrics.dogstatsd_requests);
        System.out.println("  Successes: " + metrics.dogstatsd_successes);
        System.out.println("  Errors: " + metrics.dogstatsd_errors + "\n");

        // Clean shutdown
        System.out.println("Stopping libagent...");
        LibAgent.INSTANCE.Stop();
        System.out.println("Done!");
    }
}
