#!/usr/bin/env python3
"""
Example: Send DogStatsD metrics from Python using libagent

Usage:
    export DYLD_LIBRARY_PATH=../../target/debug  # macOS
    export LD_LIBRARY_PATH=../../target/debug    # Linux
    python3 dogstatsd.py
"""

import ctypes
import os
import time
from pathlib import Path

# Find the library
lib_path = Path(__file__).parent.parent.parent / "target" / "debug"
if os.uname().sysname == "Darwin":
    lib_file = lib_path / "liblibagent.dylib"
elif os.uname().sysname == "Linux":
    lib_file = lib_path / "liblibagent.so"
else:
    lib_file = lib_path / "libagent.dll"

# Load libagent
libagent = ctypes.CDLL(str(lib_file))

# Define function signatures
libagent.Initialize.argtypes = []
libagent.Initialize.restype = None

libagent.Stop.argtypes = []
libagent.Stop.restype = None

libagent.SendDogStatsDMetric.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t]
libagent.SendDogStatsDMetric.restype = ctypes.c_int32

# Define MetricsData structure (simplified for this example)
class MetricsData(ctypes.Structure):
    _fields_ = [
        ("agent_spawns", ctypes.c_uint64),
        ("trace_agent_spawns", ctypes.c_uint64),
        ("agent_failures", ctypes.c_uint64),
        ("trace_agent_failures", ctypes.c_uint64),
        ("uptime_seconds", ctypes.c_double),
        ("proxy_get_requests", ctypes.c_uint64),
        ("proxy_post_requests", ctypes.c_uint64),
        ("proxy_put_requests", ctypes.c_uint64),
        ("proxy_delete_requests", ctypes.c_uint64),
        ("proxy_patch_requests", ctypes.c_uint64),
        ("proxy_head_requests", ctypes.c_uint64),
        ("proxy_options_requests", ctypes.c_uint64),
        ("proxy_other_requests", ctypes.c_uint64),
        ("proxy_2xx_responses", ctypes.c_uint64),
        ("proxy_3xx_responses", ctypes.c_uint64),
        ("proxy_4xx_responses", ctypes.c_uint64),
        ("proxy_5xx_responses", ctypes.c_uint64),
        ("response_time_ema_all", ctypes.c_double),
        ("response_time_ema_2xx", ctypes.c_double),
        ("response_time_ema_4xx", ctypes.c_double),
        ("response_time_ema_5xx", ctypes.c_double),
        ("response_time_sample_count", ctypes.c_uint64),
        ("dogstatsd_requests", ctypes.c_uint64),
        ("dogstatsd_successes", ctypes.c_uint64),
        ("dogstatsd_errors", ctypes.c_uint64),
    ]

libagent.GetMetrics.argtypes = []
libagent.GetMetrics.restype = MetricsData


def send_metric(metric_str):
    """Send a DogStatsD metric"""
    metric_bytes = metric_str.encode('utf-8')
    metric_array = (ctypes.c_uint8 * len(metric_bytes))(*metric_bytes)
    result = libagent.SendDogStatsDMetric(metric_array, len(metric_bytes))
    return result


def main():
    print("DogStatsD Example - Sending Custom Metrics")
    print("==========================================\n")

    # Initialize libagent
    libagent.Initialize()
    print("Initialized libagent\n")

    # Give the agent a moment to start
    time.sleep(1)

    # Example 1: Counter
    print("Sending counter metric (page views)...")
    result = send_metric("example.page.views:1|c|#env:prod,service:web")
    print(f"✓ Counter sent: {result == 0}\n")

    # Example 2: Gauge
    print("Sending gauge metric (temperature)...")
    result = send_metric("example.temperature:72.5|g|#location:office,sensor:A1")
    print(f"✓ Gauge sent: {result == 0}\n")

    # Example 3: Histogram
    print("Sending histogram metric (response time)...")
    result = send_metric("example.request.duration:250|h|@0.5|#endpoint:/api/users")
    print(f"✓ Histogram sent: {result == 0}\n")

    # Example 4: Distribution
    print("Sending distribution metric (response size)...")
    result = send_metric("example.response.size:1024|d|#status:200")
    print(f"✓ Distribution sent: {result == 0}\n")

    # Example 5: Timing
    print("Sending timing metric (database query)...")
    result = send_metric("example.db.query.time:45|ms|#db:postgres,table:users")
    print(f"✓ Timing sent: {result == 0}\n")

    # Example 6: Set
    print("Sending set metric (unique visitors)...")
    result = send_metric("example.unique.visitors:user_12345|s")
    print(f"✓ Set sent: {result == 0}\n")

    # Example 7: Batched metrics
    print("Sending batched metrics...")
    batch = """example.requests.total:1|c|#env:prod
example.cache.hits:42|c|#cache:redis
example.active.connections:100|g
example.cpu.usage:75.5|g|#host:web01"""
    result = send_metric(batch)
    print(f"✓ Batched metrics sent (4 metrics): {result == 0}\n")

    # Get metrics
    print("Retrieving libagent metrics...")
    metrics = libagent.GetMetrics()
    print("DogStatsD Metrics:")
    print(f"  Requests: {metrics.dogstatsd_requests}")
    print(f"  Successes: {metrics.dogstatsd_successes}")
    print(f"  Errors: {metrics.dogstatsd_errors}\n")

    # Clean shutdown
    print("Stopping libagent...")
    libagent.Stop()
    print("Done!")


if __name__ == "__main__":
    main()
