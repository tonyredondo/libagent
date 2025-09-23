require 'ffi'

module LibAgent
  extend FFI::Library
  ffi_lib 'libagent'

  class LibagentHttpBuffer < FFI::Struct
    layout :data, :pointer,
           :len,  :size_t
  end

  class LibagentHttpResponse < FFI::Struct
    layout :status,  :uint16,
           :headers, LibagentHttpBuffer,
           :body,    LibagentHttpBuffer
  end

  attach_function :ProxyTraceAgent, [:string, :string, :string, :pointer, :size_t, :pointer, :pointer], :int32
  attach_function :FreeHttpResponse, [LibagentHttpResponse.by_ref], :void
  attach_function :FreeCString, [:pointer], :void
end

def main
  out_resp_ptr = FFI::MemoryPointer.new(:pointer)
  out_err_ptr  = FFI::MemoryPointer.new(:pointer)

  rc = LibAgent.ProxyTraceAgent('GET', '/info', "Accept: application/json\n", FFI::Pointer::NULL, 0, out_resp_ptr, out_err_ptr)
  if rc != 0
    err_p = out_err_ptr.read_pointer
    if !err_p.null?
      puts "error: #{err_p.read_string}"
      LibAgent.FreeCString(err_p)
    else
      puts "error rc=#{rc}"
    end
    return
  end

  resp_p = out_resp_ptr.read_pointer
  resp = LibAgent::LibagentHttpResponse.new(resp_p)
  puts "Status: #{resp[:status]}"
  unless resp[:headers][:data].null? || resp[:headers][:len] == 0
    headers = resp[:headers][:data].read_string_length(resp[:headers][:len])
    puts "Headers:\n#{headers}"
  end
  unless resp[:body][:data].null? || resp[:body][:len] == 0
    body = resp[:body][:data].read_string_length(resp[:body][:len])
    puts "Body:\n#{body}"
  end
  LibAgent.FreeHttpResponse(resp_p)
end

main
