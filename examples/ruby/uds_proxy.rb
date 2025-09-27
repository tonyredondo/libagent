require 'ffi'

module LibAgent
  extend FFI::Library
  ffi_lib 'libagent'

  # Callback types
  callback :response_callback, [:uint16, :pointer, :size_t, :pointer, :size_t, :pointer], :void
  callback :error_callback, [:pointer, :pointer], :void

  # Metrics data structure
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
           :response_time_sample_count, :uint64
  end

  attach_function :Initialize, [], :void
  attach_function :Stop, [], :void
  attach_function :GetMetrics, [], MetricsData
  attach_function :ProxyTraceAgent, [:string, :string, :string, :pointer, :size_t, :response_callback, :error_callback, :pointer], :int32
end

# Callback implementations
def on_response(status, headers_data, headers_len, body_data, body_len, user_data)
  puts "Status: #{status}"
  unless headers_data.null? || headers_len == 0
    headers = headers_data.read_string_length(headers_len)
    puts "Headers:\n#{headers}"
  end
  unless body_data.null? || body_len == 0
    body = body_data.read_string_length(body_len)
    puts "Body:\n#{body}"
  end
end

def on_error(error_message, user_data)
  if !error_message.null?
    puts "error: #{error_message.read_string}"
  else
    puts "error: unknown error"
  end
end

def main
  puts "Initializing libagent..."
  LibAgent.Initialize

  # Get and display initial metrics
  puts "\n=== Initial Metrics ==="
  metrics = LibAgent.GetMetrics
  puts "Agent spawns: #{metrics[:agent_spawns]}"
  puts "Trace agent spawns: #{metrics[:trace_agent_spawns]}"
  puts "Uptime: #{metrics[:uptime_seconds].round(2)} seconds"

  # Make the request using callbacks - no manual memory management!
  puts "\nMaking proxy request..."
  rc = LibAgent.ProxyTraceAgent(
    'GET',                           # method
    '/info',                         # path
    "Accept: application/json\n",    # headers
    FFI::Pointer::NULL,              # body (none)
    0,                               # body length
    method(:on_response),            # success callback
    method(:on_error),               # error callback
    FFI::Pointer::NULL               # user data (not used)
  )

  if rc != 0
    puts "ProxyTraceAgent returned error code: #{rc}"
    LibAgent.Stop
    return
  end

  # Get and display final metrics
  puts "\n=== Final Metrics ==="
  metrics = LibAgent.GetMetrics
  puts "Agent spawns: #{metrics[:agent_spawns]}"
  puts "Trace agent spawns: #{metrics[:trace_agent_spawns]}"
  puts "GET requests: #{metrics[:proxy_get_requests]}"
  puts "2xx responses: #{metrics[:proxy_2xx_responses]}"
  puts "4xx responses: #{metrics[:proxy_4xx_responses]}"
  puts "5xx responses: #{metrics[:proxy_5xx_responses]}"
  puts "Avg response time: #{metrics[:response_time_ema_all].round(2)} ms"
  puts "Uptime: #{metrics[:uptime_seconds].round(2)} seconds"

  puts "\nStopping libagent..."
  LibAgent.Stop
end

main
