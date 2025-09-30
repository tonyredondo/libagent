using System;
using System.Runtime.InteropServices;
using System.Text;

/// <summary>
/// Example: Send DogStatsD metrics from .NET using libagent
/// 
/// Usage:
///   dotnet run DogStatsD.cs
/// </summary>
class DogStatsDExample
{
    // Platform-specific library names
    private const string LibraryName = "libagent";

    [DllImport(LibraryName)]
    private static extern void Initialize();

    [DllImport(LibraryName)]
    private static extern void Stop();

    [DllImport(LibraryName)]
    private static extern int SendDogStatsDMetric(byte[] payload, UIntPtr payloadLen);

    [StructLayout(LayoutKind.Sequential)]
    private struct MetricsData
    {
        public ulong agent_spawns;
        public ulong trace_agent_spawns;
        public ulong agent_failures;
        public ulong trace_agent_failures;
        public double uptime_seconds;
        public ulong proxy_get_requests;
        public ulong proxy_post_requests;
        public ulong proxy_put_requests;
        public ulong proxy_delete_requests;
        public ulong proxy_patch_requests;
        public ulong proxy_head_requests;
        public ulong proxy_options_requests;
        public ulong proxy_other_requests;
        public ulong proxy_2xx_responses;
        public ulong proxy_3xx_responses;
        public ulong proxy_4xx_responses;
        public ulong proxy_5xx_responses;
        public double response_time_ema_all;
        public double response_time_ema_2xx;
        public double response_time_ema_4xx;
        public double response_time_ema_5xx;
        public ulong response_time_sample_count;
        public ulong dogstatsd_requests;
        public ulong dogstatsd_successes;
        public ulong dogstatsd_errors;
    }

    [DllImport(LibraryName)]
    private static extern MetricsData GetMetrics();

    private static int SendMetric(string metric)
    {
        byte[] metricBytes = Encoding.UTF8.GetBytes(metric);
        return SendDogStatsDMetric(metricBytes, (UIntPtr)metricBytes.Length);
    }

    static void Main()
    {
        Console.WriteLine("DogStatsD Example - Sending Custom Metrics");
        Console.WriteLine("==========================================\n");

        // Initialize libagent
        Initialize();
        Console.WriteLine("Initialized libagent\n");

        // Give the agent a moment to start
        System.Threading.Thread.Sleep(1000);

        // Example 1: Counter - Page views
        Console.WriteLine("Sending counter metric (page views)...");
        int result = SendMetric("example.page.views:1|c|#env:prod,service:web");
        Console.WriteLine($"✓ Counter sent: {result == 0}\n");

        // Example 2: Gauge - Temperature
        Console.WriteLine("Sending gauge metric (temperature)...");
        result = SendMetric("example.temperature:72.5|g|#location:office,sensor:A1");
        Console.WriteLine($"✓ Gauge sent: {result == 0}\n");

        // Example 3: Histogram - Response time
        Console.WriteLine("Sending histogram metric (response time)...");
        result = SendMetric("example.request.duration:250|h|@0.5|#endpoint:/api/users");
        Console.WriteLine($"✓ Histogram sent: {result == 0}\n");

        // Example 4: Distribution - Response size
        Console.WriteLine("Sending distribution metric (response size)...");
        result = SendMetric("example.response.size:1024|d|#status:200");
        Console.WriteLine($"✓ Distribution sent: {result == 0}\n");

        // Example 5: Timing - Database query
        Console.WriteLine("Sending timing metric (database query)...");
        result = SendMetric("example.db.query.time:45|ms|#db:postgres,table:users");
        Console.WriteLine($"✓ Timing sent: {result == 0}\n");

        // Example 6: Set - Unique visitors
        Console.WriteLine("Sending set metric (unique visitors)...");
        result = SendMetric("example.unique.visitors:user_12345|s");
        Console.WriteLine($"✓ Set sent: {result == 0}\n");

        // Example 7: Batched metrics
        Console.WriteLine("Sending batched metrics...");
        string batch = @"example.requests.total:1|c|#env:prod
example.cache.hits:42|c|#cache:redis
example.active.connections:100|g
example.cpu.usage:75.5|g|#host:web01";
        result = SendMetric(batch);
        Console.WriteLine($"✓ Batched metrics sent (4 metrics): {result == 0}\n");

        // Get metrics
        Console.WriteLine("Retrieving libagent metrics...");
        MetricsData metrics = GetMetrics();
        Console.WriteLine("DogStatsD Metrics:");
        Console.WriteLine($"  Requests: {metrics.dogstatsd_requests}");
        Console.WriteLine($"  Successes: {metrics.dogstatsd_successes}");
        Console.WriteLine($"  Errors: {metrics.dogstatsd_errors}\n");

        // Clean shutdown
        Console.WriteLine("Stopping libagent...");
        Stop();
        Console.WriteLine("Done!");
    }
}
