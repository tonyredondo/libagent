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
use crate::manager::log_debug;
use crate::uds;
#[cfg(windows)]
use crate::winpipe;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Type alias for validated proxy arguments to reduce type complexity.
type ValidatedProxyArgs = (String, String, Vec<(String, String)>, Vec<u8>);

/// Global request ID counter for tracking concurrent requests in debug logs
static REQUEST_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Initialize the library: start the Agent and Trace Agent and begin monitoring.
///
/// This function is safe to call multiple times; subsequent calls are no-ops
/// once the library has already been initialized.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn Initialize() {
    log_debug("FFI call: Initialize()");
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
    log_debug("FFI call: Stop()");
    let _ = catch_unwind(AssertUnwindSafe(|| {
        manager::stop();
    }));
}

/// Callback function type for successful HTTP responses.
/// Parameters:
/// - status: HTTP status code
/// - headers_data: header data (CRLF-separated lines)
/// - headers_len: length of headers data
/// - body_data: response body data
/// - body_len: length of body data
/// - user_data: user-provided context pointer
pub type ResponseCallback = extern "C" fn(
    status: u16,
    headers_data: *const u8,
    headers_len: usize,
    body_data: *const u8,
    body_len: usize,
    user_data: *mut ::std::os::raw::c_void,
);

/// Callback function type for errors.
/// Parameters:
/// - error_message: null-terminated error message
/// - user_data: user-provided context pointer
pub type ErrorCallback = extern "C" fn(
    error_message: *const ::std::os::raw::c_char,
    user_data: *mut ::std::os::raw::c_void,
);

unsafe fn cstr_arg<'a>(p: *const c_char, name: &str) -> Result<&'a str, String> {
    if p.is_null() {
        return Err(format!("{} is null", name));
    }
    match unsafe { CStr::from_ptr(p) }.to_str() {
        Ok(s) => Ok(s),
        Err(_) => Err(format!("{} is not valid UTF-8", name)),
    }
}

/// Validate and extract arguments for ProxyTraceAgent (callback version).
fn validate_proxy_args(
    method: *const c_char,
    path: *const c_char,
    headers: *const c_char,
    body_ptr: *const u8,
    body_len: usize,
) -> Result<ValidatedProxyArgs, String> {
    let method = unsafe {
        match cstr_arg(method, "method") {
            Ok(s) => s.to_string(),
            Err(e) => return Err(e),
        }
    };
    let mut path = unsafe {
        match cstr_arg(path, "path") {
            Ok(s) => s.to_string(),
            Err(e) => return Err(e),
        }
    };

    // Ensure path starts with '/' for valid HTTP request
    if !path.starts_with('/') {
        path.insert(0, '/');
        log_debug(&format!(
            "FFI ProxyTraceAgent: prepended '/' to path: '{}'",
            path
        ));
    }
    let headers_str = unsafe { cstr_arg(headers, "headers")? };

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

/// Perform the actual proxy request based on platform (callback version).
fn perform_proxy_request_new(
    request_id: u64,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    timeout: Duration,
) -> Result<crate::http::Response, String> {
    // Ensure trace-agent is ready before making the first proxy call
    if let Err(e) = manager::get_manager().ensure_trace_agent_ready() {
        log_debug(&format!("FFI ProxyTraceAgent: {}", e));
        return Err(e);
    }
    #[cfg(unix)]
    {
        let uds_path = get_trace_agent_uds_path();
        uds::request_over_uds(request_id, &uds_path, method, path, headers, body, timeout)
    }
    #[cfg(windows)]
    {
        let pipe_name = get_trace_agent_pipe_name();
        if pipe_name.trim().is_empty() {
            return Err("LIBAGENT_TRACE_AGENT_PIPE not set".to_string());
        }
        winpipe::request_over_named_pipe(
            request_id, &pipe_name, method, path, headers, body, timeout,
        )
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        Err("platform not supported".to_string())
    }
}

/// Proxy an HTTP request to the trace-agent over the local IPC transport.
///
/// This function uses callbacks to deliver results instead of returning allocated buffers.
/// Memory management is handled automatically - no manual freeing required.
///
/// Parameters:
/// - method: HTTP method (e.g., "GET", "POST")
/// - path: HTTP request path + optional query (e.g., "/v0.7/traces")
/// - headers: CRLF or LF separated lines in the form "Name: Value"
/// - body_ptr/body_len: optional body bytes (pass null/0 for none)
/// - on_response: callback invoked on successful response
/// - on_error: callback invoked on error (may be null)
/// - user_data: context pointer passed to callbacks
///
/// Returns 0 on success, negative on error.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn ProxyTraceAgent(
    method: *const c_char,
    path: *const c_char,
    headers: *const c_char,
    body_ptr: *const u8,
    body_len: usize,
    on_response: *const ::std::os::raw::c_void, // Actually ResponseCallback, but made void* for C compatibility
    on_error: *const ::std::os::raw::c_void, // Actually ErrorCallback, but made void* for C compatibility
    user_data: *mut ::std::os::raw::c_void,
) -> i32 {
    // Generate unique request ID for tracking concurrent requests
    let request_id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    log_debug(&format!("FFI call: ProxyTraceAgent() [req:{}]", request_id));
    catch_unwind(AssertUnwindSafe(|| {
        // validate and extract arguments
        let (method, path, headers, body) =
            match validate_proxy_args(method, path, headers, body_ptr, body_len) {
                Ok(args) => {
                    log_debug(&format!(
                        "FFI ProxyTraceAgent: validation successful, method={}, path={}",
                        args.0, args.1
                    ));
                    // Record proxy request metrics
                    crate::metrics::get_metrics().record_proxy_request(&args.0);
                    args
                }
                Err(err_msg) => {
                    log_debug(&format!(
                        "FFI ProxyTraceAgent: validation failed: {}",
                        err_msg
                    ));
                    if !on_error.is_null() {
                        let on_error_fn: ErrorCallback = unsafe { std::mem::transmute(on_error) };
                        let c_err = CString::new(err_msg)
                            .unwrap_or_else(|_| CString::new("error").unwrap());
                        on_error_fn(c_err.as_ptr(), user_data);
                    }
                    return -1;
                }
            };

        // Reasonable default timeout
        let timeout = Duration::from_secs(50);

        // perform the proxy request
        log_debug(&format!(
            "FFI ProxyTraceAgent: sending {} {} request, body_len={}",
            method,
            path,
            body.len()
        ));

        // Start timing the request
        let request_start = std::time::Instant::now();

        let resp = match perform_proxy_request_new(
            request_id, &method, &path, headers, &body, timeout,
        ) {
            Ok(resp) => {
                let response_time = request_start.elapsed();
                log_debug(&format!(
                    "FFI ProxyTraceAgent: received response with status={}, response_time={:.2}ms",
                    resp.status,
                    response_time.as_secs_f64() * 1000.0
                ));
                // Record proxy response metrics
                crate::metrics::get_metrics().record_proxy_response(resp.status, response_time);
                resp
            }
            Err(err_msg) => {
                log_debug(&format!("FFI ProxyTraceAgent: request failed: {}", err_msg));
                if !on_error.is_null() {
                    let on_error_fn: ErrorCallback = unsafe { std::mem::transmute(on_error) };
                    let c_err =
                        CString::new(err_msg).unwrap_or_else(|_| CString::new("error").unwrap());
                    on_error_fn(c_err.as_ptr(), user_data);
                }
                return -2;
            }
        };

        // call the response callback
        if !on_response.is_null() {
            let on_response_fn: ResponseCallback = unsafe { std::mem::transmute(on_response) };
            let headers_data = uds::serialize_headers(&resp.headers);
            let headers_bytes = headers_data.as_bytes();
            on_response_fn(
                resp.status,
                headers_bytes.as_ptr(),
                headers_bytes.len(),
                resp.body.as_ptr(),
                resp.body.len(),
                user_data,
            );
        }
        0
    }))
    .unwrap_or_else(|_| {
        if !on_error.is_null() {
            let on_error_fn: ErrorCallback = unsafe { std::mem::transmute(on_error) };
            let c_err = CString::new("panic in ProxyTraceAgent").unwrap();
            on_error_fn(c_err.as_ptr(), user_data);
        }
        -100
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cstr_arg_null_pointer() {
        let result = unsafe { cstr_arg(std::ptr::null(), "test_arg") };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test_arg is null"));
    }

    #[test]
    fn test_cstr_arg_invalid_utf8() {
        use std::ffi::CString;

        // Create invalid UTF-8 bytes
        let invalid_utf8 = vec![0x80, 0x81, 0x00]; // Invalid UTF-8 followed by null terminator
        let c_string = CString::from_vec_with_nul(invalid_utf8).unwrap();

        let result = unsafe { cstr_arg(c_string.as_ptr(), "test_arg") };
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("test_arg is not valid UTF-8"));
    }

    #[test]
    fn test_cstr_arg_valid_input() {
        use std::ffi::CString;

        let c_string = CString::new("valid string").unwrap();
        let result = unsafe { cstr_arg(c_string.as_ptr(), "test_arg") };
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "valid string");
    }

    #[test]
    fn test_initialize_function() {
        // Test that Initialize() can be called without panicking
        // This should be safe to call as it's idempotent
        Initialize();
    }

    #[test]
    fn test_stop_function() {
        // Test that Stop() can be called without panicking
        // This should be safe to call as it's idempotent
        Stop();
    }

    #[test]
    fn test_validate_proxy_args_null_method() {
        use std::ffi::CString;

        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        let result = validate_proxy_args(
            std::ptr::null(), // null method
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("method is null"));
    }

    #[test]
    fn test_validate_proxy_args_null_path() {
        use std::ffi::CString;

        let method = CString::new("GET").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        let result = validate_proxy_args(
            method.as_ptr(),
            std::ptr::null(), // null path
            headers.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path is null"));
    }

    #[test]
    fn test_validate_proxy_args_invalid_method_utf8() {
        use std::ffi::CString;

        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        // Create invalid UTF-8 for method
        let invalid_method = vec![0x80, 0x81, 0x00]; // Invalid UTF-8 followed by null terminator
        let method_cstr = unsafe { CString::from_vec_with_nul_unchecked(invalid_method) };

        let result = validate_proxy_args(
            method_cstr.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("method is not valid UTF-8"));
    }

    #[test]
    fn test_validate_proxy_args_invalid_path_utf8() {
        use std::ffi::CString;

        let method = CString::new("GET").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        // Create invalid UTF-8 for path
        let invalid_path = vec![0x80, 0x81, 0x00]; // Invalid UTF-8 followed by null terminator
        let path_cstr = unsafe { CString::from_vec_with_nul_unchecked(invalid_path) };

        let result = validate_proxy_args(
            method.as_ptr(),
            path_cstr.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path is not valid UTF-8"));
    }

    #[test]
    fn test_validate_proxy_args_null_headers() {
        use std::ffi::CString;

        let method = CString::new("GET").unwrap();
        let path = CString::new("/test").unwrap();

        let result = validate_proxy_args(
            method.as_ptr(),
            path.as_ptr(),
            std::ptr::null(), // null headers
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("headers is null"));
    }

    #[test]
    fn test_validate_proxy_args_invalid_headers_utf8() {
        use std::ffi::CString;

        let method = CString::new("GET").unwrap();
        let path = CString::new("/test").unwrap();

        // Create invalid UTF-8 for headers
        let invalid_headers = vec![0x80, 0x81, 0x00]; // Invalid UTF-8 followed by null terminator
        let headers_cstr = unsafe { CString::from_vec_with_nul_unchecked(invalid_headers) };

        let result = validate_proxy_args(
            method.as_ptr(),
            path.as_ptr(),
            headers_cstr.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("headers is not valid UTF-8"));
    }

    #[test]
    fn test_validate_proxy_args_with_body() {
        use std::ffi::CString;

        let method = CString::new("POST").unwrap();
        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();
        let body_data = b"{\"test\": \"data\"}";
        let body_len = body_data.len();

        let result = validate_proxy_args(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            body_data.as_ptr(),
            body_len,
        );

        assert!(result.is_ok());
        let (method_str, path_str, _headers_vec, body_vec) = result.unwrap();
        assert_eq!(method_str, "POST");
        assert_eq!(path_str, "/test");
        assert_eq!(body_vec, body_data);
    }

    #[test]
    fn test_validate_proxy_args_empty_body() {
        use std::ffi::CString;

        let method = CString::new("GET").unwrap();
        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        let result = validate_proxy_args(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(), // null body
            0,                // zero length
        );

        assert!(result.is_ok());
        let (method_str, path_str, _headers_vec, body_vec) = result.unwrap();
        assert_eq!(method_str, "GET");
        assert_eq!(path_str, "/test");
        assert!(body_vec.is_empty());
    }

    #[test]
    fn test_validate_proxy_args_path_normalization() {
        use std::ffi::CString;

        let method = CString::new("POST").unwrap();
        let path = CString::new("v0.7/traces").unwrap(); // path without leading slash
        let headers = CString::new("Content-Type: application/json").unwrap();

        let result = validate_proxy_args(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
        );

        assert!(result.is_ok());
        let (method_str, path_str, _headers_vec, _body_vec) = result.unwrap();
        assert_eq!(method_str, "POST");
        assert_eq!(path_str, "/v0.7/traces"); // should have leading slash prepended
    }

    #[test]
    fn test_proxy_trace_agent_validation_error() {
        use std::ffi::CString;
        use std::sync::Mutex;

        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        static ERROR_CALLED: Mutex<bool> = Mutex::new(false);

        extern "C" fn error_callback(
            _msg: *const ::std::os::raw::c_char,
            _user_data: *mut ::std::os::raw::c_void,
        ) {
            *ERROR_CALLED.lock().unwrap() = true;
        }

        let response_callback: *const ::std::os::raw::c_void = std::ptr::null();
        let error_callback_ptr: *const ::std::os::raw::c_void = error_callback as *const _;

        // Test with null method (should trigger validation error)
        let result = ProxyTraceAgent(
            std::ptr::null(), // null method
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
            response_callback,
            error_callback_ptr,
            std::ptr::null_mut(),
        );

        assert_eq!(result, -1);
        assert!(*ERROR_CALLED.lock().unwrap());
    }

    #[test]
    fn test_proxy_trace_agent_request_error() {
        use std::ffi::CString;
        use std::sync::Mutex;

        // Use a path that will likely cause a connection error
        let method = CString::new("GET").unwrap();
        let path = CString::new("/nonexistent").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        static ERROR_CALLED: Mutex<bool> = Mutex::new(false);

        extern "C" fn error_callback(
            _msg: *const ::std::os::raw::c_char,
            _user_data: *mut ::std::os::raw::c_void,
        ) {
            *ERROR_CALLED.lock().unwrap() = true;
        }

        let response_callback: *const ::std::os::raw::c_void = std::ptr::null();
        let error_callback_ptr: *const ::std::os::raw::c_void = error_callback as *const _;

        // This should fail because there's no actual trace agent running
        let result = ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
            response_callback,
            error_callback_ptr,
            std::ptr::null_mut(),
        );

        // Should return an error code (likely -2 for request error)
        assert_eq!(result, -2);
        assert!(*ERROR_CALLED.lock().unwrap());
    }

    #[test]
    fn test_proxy_trace_agent_panic_handling() {
        use std::ffi::CString;
        use std::sync::Mutex;

        let method = CString::new("GET").unwrap();
        let path = CString::new("/test").unwrap();
        let headers = CString::new("Content-Type: application/json").unwrap();

        static ERROR_CALLED: Mutex<bool> = Mutex::new(false);

        extern "C" fn error_callback(
            _msg: *const ::std::os::raw::c_char,
            _user_data: *mut ::std::os::raw::c_void,
        ) {
            *ERROR_CALLED.lock().unwrap() = true;
        }

        let response_callback: *const ::std::os::raw::c_void = std::ptr::null();
        let error_callback_ptr: *const ::std::os::raw::c_void = error_callback as *const _;

        // Test panic handling - this should catch panics in the unwind closure
        // We'll use a method that might trigger some internal panic scenario
        let result = ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            std::ptr::null(),
            0,
            response_callback,
            error_callback_ptr,
            std::ptr::null_mut(),
        );

        // Even if it doesn't panic, it should handle potential panics gracefully
        // The return value could be -2 (request error) or -100 (panic), both are acceptable
        assert!(result == -2 || result == -100);
    }
}
