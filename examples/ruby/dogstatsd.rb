#!/usr/bin/env ruby

# Example: Send DogStatsD metrics from Ruby using libagent
#
# Install dependencies:
#   gem install ffi
#
# Run:
#   DYLD_LIBRARY_PATH=../../target/debug ruby dogstatsd.rb

require 'ffi'

module LibAgent
  extend FFI::Library
  
  # Determine library extension
  lib_ext = case RbConfig::CONFIG['host_os']
  when /darwin/
    'dylib'
  when /linux/
    'so'
  when /mswin|mingw/
    'dll'
  else
    'so'
  end
  
  lib_path = File.join(__dir__, '../../target/debug', "liblibagent.#{lib_ext}")
  ffi_lib lib_path

  # FFI function definitions
  attach_function :Initialize, [], :void
  attach_function :Stop, [], :void
  attach_function :SendDogStatsDMetric, [:pointer, :size_t], :int32

  class MetricsData < FFI::Struct
    layout :agent_spawns, :uint64,
           :trace_agent_spawns, :uint64,
           :agent_failures, :uint64,
           :trace_agent_failures, :uint64,
           :uptime_seconds, :double,
           :proxy_get_requests, :uint64,
           :proxy_post_requests, :uint64,
           :proxy_put_requests, :uint64,
           :proxy_delete_requests, :uint64,
           :proxy_patch_requests, :uint64,
           :proxy_head_requests, :uint64,
           :proxy_options_requests, :uint64,
           :proxy_other_requests, :uint64,
           :proxy_2xx_responses, :uint64,
           :proxy_3xx_responses, :uint64,
           :proxy_4xx_responses, :uint64,
           :proxy_5xx_responses, :uint64,
           :response_time_ema_all, :double,
           :response_time_ema_2xx, :double,
           :response_time_ema_4xx, :double,
           :response_time_ema_5xx, :double,
           :response_time_sample_count, :uint64,
           :dogstatsd_requests, :uint64,
           :dogstatsd_successes, :uint64,
           :dogstatsd_errors, :uint64
  end

  attach_function :GetMetrics, [], MetricsData.by_value
end

def send_metric(metric)
  metric_bytes = metric.encode('UTF-8')
  ptr = FFI::MemoryPointer.from_string(metric_bytes)
  LibAgent.SendDogStatsDMetric(ptr, metric_bytes.bytesize)
end

def main
  puts "DogStatsD Example - Sending Custom Metrics"
  puts "=========================================="
  puts

  # Initialize libagent
  LibAgent.Initialize
  puts "Initialized libagent"
  puts

  # Give the agent a moment to start
  sleep 1

  # Example 1: Counter - Page views
  puts "Sending counter metric (page views)..."
  result = send_metric('example.page.views:1|c|#env:prod,service:web')
  puts "✓ Counter sent: #{result == 0}"
  puts

  # Example 2: Gauge - Temperature
  puts "Sending gauge metric (temperature)..."
  result = send_metric('example.temperature:72.5|g|#location:office,sensor:A1')
  puts "✓ Gauge sent: #{result == 0}"
  puts

  # Example 3: Histogram - Response time
  puts "Sending histogram metric (response time)..."
  result = send_metric('example.request.duration:250|h|@0.5|#endpoint:/api/users')
  puts "✓ Histogram sent: #{result == 0}"
  puts

  # Example 4: Distribution - Response size
  puts "Sending distribution metric (response size)..."
  result = send_metric('example.response.size:1024|d|#status:200')
  puts "✓ Distribution sent: #{result == 0}"
  puts

  # Example 5: Timing - Database query
  puts "Sending timing metric (database query)..."
  result = send_metric('example.db.query.time:45|ms|#db:postgres,table:users')
  puts "✓ Timing sent: #{result == 0}"
  puts

  # Example 6: Set - Unique visitors
  puts "Sending set metric (unique visitors)..."
  result = send_metric('example.unique.visitors:user_12345|s')
  puts "✓ Set sent: #{result == 0}"
  puts

  # Example 7: Batched metrics
  puts "Sending batched metrics..."
  batch = <<~METRICS
    example.requests.total:1|c|#env:prod
    example.cache.hits:42|c|#cache:redis
    example.active.connections:100|g
    example.cpu.usage:75.5|g|#host:web01
  METRICS
  result = send_metric(batch.strip)
  puts "✓ Batched metrics sent (4 metrics): #{result == 0}"
  puts

  # Get metrics
  puts "Retrieving libagent metrics..."
  metrics = LibAgent.GetMetrics
  puts "DogStatsD Metrics:"
  puts "  Requests: #{metrics[:dogstatsd_requests]}"
  puts "  Successes: #{metrics[:dogstatsd_successes]}"
  puts "  Errors: #{metrics[:dogstatsd_errors]}"
  puts

  # Clean shutdown
  puts "Stopping libagent..."
  LibAgent.Stop
  puts "Done!"
end

main if __FILE__ == $PROGRAM_NAME
