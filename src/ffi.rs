//! C FFI surface for libagent.
//!
//! This module exposes a minimal set of functions intended to be consumed
//! from non-Rust languages. The business logic is provided by
//! `crate::manager`, and these functions simply forward to those safe
//! Rust APIs.

#[cfg(windows)]
use crate::config::get_trace_agent_pipe_name;
#[cfg(unix)]
use crate::config::get_trace_agent_uds_path;
use crate::manager;
use crate::uds;
#[cfg(windows)]
use crate::winpipe;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::time::Duration;

/// Type alias for validated proxy arguments to reduce type complexity.
type ValidatedProxyArgs = (String, String, Vec<(String, String)>, Vec<u8>);

/// Initialize the library: start the Agent and Trace Agent and begin monitoring.
///
/// This function is safe to call multiple times; subsequent calls are no-ops
/// once the library has already been initialized.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn Initialize() {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        manager::initialize();
    }));
}

/// Stop the library: terminate the Agent and Trace Agent and stop monitoring.
///
/// This function is safe to call multiple times.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn Stop() {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        manager::stop();
    }));
}

/// A simple owned buffer passed across FFI boundaries.
#[repr(C)]
pub struct LibagentHttpBuffer {
    pub data: *mut u8,
    pub len: usize,
}

/// HTTP response container returned by ProxyTraceAgent.
#[repr(C)]
pub struct LibagentHttpResponse {
    pub status: u16,
    pub headers: LibagentHttpBuffer, // CRLF-separated header lines
    pub body: LibagentHttpBuffer,
}

fn to_c_error(out_err: *mut *mut c_char, msg: String) {
    unsafe {
        if !out_err.is_null() {
            // Sanitize the message by removing null bytes, as C strings cannot contain them
            let sanitized_msg = msg.replace('\0', "");
            let c = CString::new(sanitized_msg).unwrap_or_else(|_| {
                // Fallback to a safe error message if sanitization still fails
                CString::new("error").unwrap()
            });
            *out_err = c.into_raw();
        }
    }
}

unsafe fn cstr_arg<'a>(
    p: *const c_char,
    name: &str,
    out_err: *mut *mut c_char,
) -> Result<&'a str, i32> {
    if p.is_null() {
        to_c_error(out_err, format!("{} is null", name));
        return Err(-1);
    }
    match unsafe { CStr::from_ptr(p) }.to_str() {
        Ok(s) => Ok(s),
        Err(_) => {
            to_c_error(out_err, format!("{} is not valid UTF-8", name));
            Err(-1)
        }
    }
}

fn make_buf(mut v: Vec<u8>) -> LibagentHttpBuffer {
    v.shrink_to_fit();
    let len = v.len();
    let ptr = v.as_mut_ptr();
    std::mem::forget(v);
    LibagentHttpBuffer { data: ptr, len }
}

/// Free a buffer previously allocated by libagent.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn FreeHttpBuffer(buf: LibagentHttpBuffer) {
    if !buf.data.is_null() && buf.len > 0 {
        unsafe {
            let _ = Vec::from_raw_parts(buf.data, buf.len, buf.len);
        }
    }
}

/// Free a CString previously returned by libagent.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn FreeCString(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Free a response previously allocated by libagent, including its buffers.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn FreeHttpResponse(resp: *mut LibagentHttpResponse) {
    if resp.is_null() {
        return;
    }
    unsafe {
        let r = &*resp;
        let headers = LibagentHttpBuffer {
            data: r.headers.data,
            len: r.headers.len,
        };
        let body = LibagentHttpBuffer {
            data: r.body.data,
            len: r.body.len,
        };
        FreeHttpBuffer(headers);
        FreeHttpBuffer(body);
        drop(Box::from_raw(resp));
    }
}

/// Validate and extract arguments for ProxyTraceAgent.
fn validate_proxy_args(
    method: *const c_char,
    path: *const c_char,
    headers: *const c_char,
    body_ptr: *const u8,
    body_len: usize,
    out_err: *mut *mut c_char,
) -> Result<ValidatedProxyArgs, i32> {
    let method = unsafe {
        match cstr_arg(method, "method", out_err) {
            Ok(s) => s.to_string(),
            Err(e) => return Err(e),
        }
    };
    let path = unsafe {
        match cstr_arg(path, "path", out_err) {
            Ok(s) => s.to_string(),
            Err(e) => return Err(e),
        }
    };
    let headers_str = unsafe { cstr_arg(headers, "headers", out_err)? };

    let hdrs = uds::parse_header_lines(headers_str);
    let body = unsafe {
        if body_ptr.is_null() || body_len == 0 {
            Vec::new()
        } else {
            std::slice::from_raw_parts(body_ptr, body_len).to_vec()
        }
    };

    Ok((method, path, hdrs, body))
}

/// Construct a LibagentHttpResponse from an HTTP response.
fn make_http_response(resp: crate::http::Response) -> LibagentHttpResponse {
    let headers_txt = uds::serialize_headers(&resp.headers);
    let headers_buf = make_buf(headers_txt.into_bytes());
    let body_buf = make_buf(resp.body);
    LibagentHttpResponse {
        status: resp.status,
        headers: headers_buf,
        body: body_buf,
    }
}

/// Perform the actual proxy request based on platform.
fn perform_proxy_request(
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    timeout: Duration,
    out_err: *mut *mut c_char,
) -> Result<crate::http::Response, i32> {
    #[cfg(unix)]
    {
        let uds_path = get_trace_agent_uds_path();
        uds::request_over_uds(&uds_path, method, path, headers, body, timeout).map_err(|err| {
            to_c_error(out_err, err);
            -2
        })
    }
    #[cfg(windows)]
    {
        let pipe_name = get_trace_agent_pipe_name();
        if pipe_name.trim().is_empty() {
            to_c_error(out_err, "LIBAGENT_TRACE_AGENT_PIPE not set".to_string());
            return Err(-4);
        }
        winpipe::request_over_named_pipe(&pipe_name, method, path, headers, body, timeout).map_err(
            |err| {
                to_c_error(out_err, err);
                -2
            },
        )
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        to_c_error(out_err, "platform not supported".to_string());
        Err(-3)
    }
}

/// Proxy an HTTP request to the trace-agent over the local IPC transport and return the response.
///
/// Parameters:
/// - method: HTTP method (e.g., "GET", "POST")
/// - path: HTTP request path + optional query (e.g., "/v0.7/traces")
/// - headers: CRLF or LF separated lines in the form "Name: Value"
/// - body_ptr/body_len: optional body bytes (pass null/0 for none)
/// - out_resp: on success, set to an allocated response pointer to be freed with FreeHttpResponse
/// - out_err: on failure, set to an allocated error string to be freed with FreeCString
///   Returns 0 on success, negative on error.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn ProxyTraceAgent(
    method: *const c_char,
    path: *const c_char,
    headers: *const c_char,
    body_ptr: *const u8,
    body_len: usize,
    out_resp: *mut *mut LibagentHttpResponse,
    out_err: *mut *mut c_char,
) -> i32 {
    catch_unwind(AssertUnwindSafe(|| {
        // clear outputs
        unsafe {
            if !out_resp.is_null() {
                *out_resp = ptr::null_mut();
            }
            if !out_err.is_null() {
                *out_err = ptr::null_mut();
            }
        }

        // validate and extract arguments
        let (method, path, headers, body) =
            match validate_proxy_args(method, path, headers, body_ptr, body_len, out_err) {
                Ok(args) => args,
                Err(code) => return code,
            };

        // Reasonable default timeout
        let timeout = Duration::from_secs(50);

        // perform the proxy request
        let resp = match perform_proxy_request(&method, &path, headers, &body, timeout, out_err) {
            Ok(resp) => resp,
            Err(code) => return code,
        };

        // construct and return the response
        let c_response = make_http_response(resp);
        let boxed = Box::new(c_response);
        unsafe {
            if !out_resp.is_null() {
                *out_resp = Box::into_raw(boxed);
            }
        }
        0
    }))
    .unwrap_or_else(|_| {
        to_c_error(out_err, "panic in ProxyTraceAgent".to_string());
        -100
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http;

    #[test]
    fn test_make_http_response() {
        let response = http::Response {
            status: 200,
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Content-Length".to_string(), "25".to_string()),
            ],
            body: b"{\"message\": \"success\"}".to_vec(),
        };

        let c_response = make_http_response(response);

        assert_eq!(c_response.status, 200);
        assert_eq!(c_response.body.len, 22); // Length of "{\"message\": \"success\"}"
        // Headers should contain: "Content-Type: application/json\r\nContent-Length: 25\r\n"
        assert!(c_response.headers.len > 40); // Should be around 52 bytes

        // Verify the data was copied correctly (without accessing raw pointers)
        // The actual data verification is done in integration tests
    }

    #[test]
    fn test_to_c_error_with_null_pointer() {
        // Test that to_c_error handles null output pointers gracefully
        to_c_error(std::ptr::null_mut(), "test error".to_string());
        // Should not crash
    }

    #[test]
    fn test_to_c_error_with_valid_pointer() {
        use std::ffi::CStr;

        let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        to_c_error(&mut err_ptr, "test error message".to_string());

        assert!(!err_ptr.is_null());
        unsafe {
            let c_str = CStr::from_ptr(err_ptr);
            let str_slice = c_str.to_str().unwrap();
            assert_eq!(str_slice, "test error message");

            // Clean up
            FreeCString(err_ptr);
        }
    }

    #[test]
    fn test_to_c_error_with_null_bytes() {
        let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        to_c_error(&mut err_ptr, "test\0error\0message".to_string());

        assert!(!err_ptr.is_null());
        unsafe {
            let c_str = CStr::from_ptr(err_ptr);
            let str_slice = c_str.to_str().unwrap();
            // Null bytes should be removed
            assert_eq!(str_slice, "testerrormessage");

            // Clean up
            FreeCString(err_ptr);
        }
    }

    #[test]
    fn test_cstr_arg_null_pointer() {
        let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        let result = unsafe { cstr_arg(std::ptr::null(), "test_arg", &mut err_ptr) };

        assert!(result.is_err());
        assert!(!err_ptr.is_null());

        unsafe {
            let c_str = CStr::from_ptr(err_ptr);
            let str_slice = c_str.to_str().unwrap();
            assert!(str_slice.contains("test_arg is null"));

            // Clean up
            FreeCString(err_ptr);
        }
    }

    #[test]
    fn test_cstr_arg_invalid_utf8() {
        use std::ffi::CString;

        // Create invalid UTF-8 bytes
        let invalid_utf8 = vec![0x80, 0x81, 0x00]; // Invalid UTF-8 followed by null terminator
        let c_string = CString::from_vec_with_nul(invalid_utf8).unwrap();

        let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        let result = unsafe { cstr_arg(c_string.as_ptr(), "test_arg", &mut err_ptr) };

        assert!(result.is_err());
        assert!(!err_ptr.is_null());

        unsafe {
            let c_str = CStr::from_ptr(err_ptr);
            let str_slice = c_str.to_str().unwrap();
            assert!(str_slice.contains("test_arg is not valid UTF-8"));

            // Clean up
            FreeCString(err_ptr);
        }
    }

    #[test]
    fn test_cstr_arg_valid_input() {
        use std::ffi::CString;

        let c_string = CString::new("valid string").unwrap();
        let mut err_ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        let result = unsafe { cstr_arg(c_string.as_ptr(), "test_arg", &mut err_ptr) };

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid string");
        assert!(err_ptr.is_null()); // No error should be set
    }

    #[test]
    fn test_make_buf() {
        let data = vec![1, 2, 3, 4, 5];
        let buf = make_buf(data);

        assert_eq!(buf.len, 5);
        assert!(!buf.data.is_null());

        // Clean up
        FreeHttpBuffer(buf);
    }

    #[test]
    fn test_free_http_buffer_null() {
        // Should handle null data gracefully
        let buf = LibagentHttpBuffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
        FreeHttpBuffer(buf);
        // Should not crash
    }

    #[test]
    fn test_free_cstring_null() {
        // Should handle null pointers gracefully
        FreeCString(std::ptr::null_mut());
        // Should not crash
    }

    #[test]
    fn test_free_http_response_null() {
        // Should handle null response pointers gracefully
        FreeHttpResponse(std::ptr::null_mut());
        // Should not crash
    }
}
