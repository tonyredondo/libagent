require 'ffi'

module LibAgent
  extend FFI::Library
  ffi_lib 'libagent'

  # Callback types
  callback :response_callback, [:uint16, :pointer, :size_t, :pointer, :size_t, :pointer], :void
  callback :error_callback, [:pointer, :pointer], :void

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
  # Make the request using callbacks - no manual memory management!
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
  end
end

main
