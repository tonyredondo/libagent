//! C FFI surface for libagent.
//!
//! This module exposes a minimal set of functions intended to be consumed
//! from non-Rust languages. The business logic is provided by
//! `crate::manager`, and these functions simply forward to those safe
//! Rust APIs.

#[cfg(windows)]
use crate::config::get_dogstatsd_pipe_name;
#[cfg(unix)]
use crate::config::get_dogstatsd_uds_path;
#[cfg(windows)]
use crate::config::get_trace_agent_pipe_name;
#[cfg(unix)]
use crate::config::get_trace_agent_uds_path;
use crate::dogstatsd;
use crate::logging::log_debug;
use crate::manager;
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

/// Metrics data structure for FFI
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MetricsData {
    /// Process lifecycle metrics
    pub agent_spawns: u64,
    pub trace_agent_spawns: u64,
    pub agent_failures: u64,
    pub trace_agent_failures: u64,
    pub uptime_seconds: f64,

    /// HTTP proxy request metrics by method
    pub proxy_get_requests: u64,
    pub proxy_post_requests: u64,
    pub proxy_put_requests: u64,
    pub proxy_delete_requests: u64,
    pub proxy_patch_requests: u64,
    pub proxy_head_requests: u64,
    pub proxy_options_requests: u64,
    pub proxy_other_requests: u64,

    /// HTTP proxy response metrics by status code range
    pub proxy_2xx_responses: u64,
    pub proxy_3xx_responses: u64,
    pub proxy_4xx_responses: u64,
    pub proxy_5xx_responses: u64,

    /// Response time moving averages (milliseconds)
    pub response_time_ema_all: f64,
    pub response_time_ema_2xx: f64,
    pub response_time_ema_4xx: f64,
    pub response_time_ema_5xx: f64,
    pub response_time_sample_count: u64,

    /// DogStatsD metrics
    pub dogstatsd_requests: u64,
    pub dogstatsd_successes: u64,
    pub dogstatsd_errors: u64,
}

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

/// Get metrics data structure.
///
/// Returns a MetricsData structure containing all current metrics.
/// This provides direct access to individual metric values without string parsing.
///
/// The returned structure is a copy - it can be used immediately without copying.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn GetMetrics() -> MetricsData {
    log_debug("FFI call: GetMetrics()");
    let result = catch_unwind(AssertUnwindSafe(|| {
        let metrics = crate::metrics::get_metrics();

        MetricsData {
            agent_spawns: metrics.agent_spawns(),
            trace_agent_spawns: metrics.trace_agent_spawns(),
            agent_failures: metrics.agent_failures(),
            trace_agent_failures: metrics.trace_agent_failures(),
            uptime_seconds: metrics.uptime().map(|d| d.as_secs_f64()).unwrap_or(0.0),

            proxy_get_requests: metrics.proxy_get_requests(),
            proxy_post_requests: metrics.proxy_post_requests(),
            proxy_put_requests: metrics.proxy_put_requests(),
            proxy_delete_requests: metrics.proxy_delete_requests(),
            proxy_patch_requests: metrics.proxy_patch_requests(),
            proxy_head_requests: metrics.proxy_head_requests(),
            proxy_options_requests: metrics.proxy_options_requests(),
            proxy_other_requests: metrics.proxy_other_requests(),

            proxy_2xx_responses: metrics.proxy_2xx_responses(),
            proxy_3xx_responses: metrics.proxy_3xx_responses(),
            proxy_4xx_responses: metrics.proxy_4xx_responses(),
            proxy_5xx_responses: metrics.proxy_5xx_responses(),

            response_time_ema_all: metrics.response_time_ema_all(),
            response_time_ema_2xx: metrics.response_time_ema_2xx(),
            response_time_ema_4xx: metrics.response_time_ema_4xx(),
            response_time_ema_5xx: metrics.response_time_ema_5xx(),
            response_time_sample_count: metrics.response_time_sample_count(),

            dogstatsd_requests: metrics.dogstatsd_requests(),
            dogstatsd_successes: metrics.dogstatsd_successes(),
            dogstatsd_errors: metrics.dogstatsd_errors(),
        }
    }));

    match result {
        Ok(metrics_data) => metrics_data,
        Err(_) => {
            log_debug("FFI GetMetrics: panic occurred, returning zeroed struct");
            MetricsData {
                agent_spawns: 0,
                trace_agent_spawns: 0,
                agent_failures: 0,
                trace_agent_failures: 0,
                uptime_seconds: 0.0,
                proxy_get_requests: 0,
                proxy_post_requests: 0,
                proxy_put_requests: 0,
                proxy_delete_requests: 0,
                proxy_patch_requests: 0,
                proxy_head_requests: 0,
                proxy_options_requests: 0,
                proxy_other_requests: 0,
                proxy_2xx_responses: 0,
                proxy_3xx_responses: 0,
                proxy_4xx_responses: 0,
                proxy_5xx_responses: 0,
                response_time_ema_all: 0.0,
                response_time_ema_2xx: 0.0,
                response_time_ema_4xx: 0.0,
                response_time_ema_5xx: 0.0,
                response_time_sample_count: 0,
                dogstatsd_requests: 0,
                dogstatsd_successes: 0,
                dogstatsd_errors: 0,
            }
        }
    }
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

/// Safely converts a void pointer to a ResponseCallback function pointer.
///
/// # Safety
/// - The caller must ensure `ptr` is either null or a valid pointer to a function
///   matching the `ResponseCallback` signature
/// - If `ptr` is non-null, it must be a valid function pointer that can be called
///   with the expected signature
/// - Calling this with an invalid function pointer will result in undefined behavior
fn ptr_to_response_callback(ptr: *const ::std::os::raw::c_void) -> Option<ResponseCallback> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: We verify the pointer is non-null above.
        // The caller is responsible for ensuring this pointer points to a valid
        // function matching the ResponseCallback signature. This is part of the
        // FFI contract - callers must pass correct function pointer types.
        // We use `as` instead of `transmute` because it's more restricted (only
        // works for compatible pointer types) and slightly safer.
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

/// Safely converts a void pointer to an ErrorCallback function pointer.
///
/// # Safety
/// - The caller must ensure `ptr` is either null or a valid pointer to a function
///   matching the `ErrorCallback` signature
/// - If `ptr` is non-null, it must be a valid function pointer that can be called
///   with the expected signature
/// - Calling this with an invalid function pointer will result in undefined behavior
fn ptr_to_error_callback(ptr: *const ::std::os::raw::c_void) -> Option<ErrorCallback> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: We verify the pointer is non-null above.
        // The caller is responsible for ensuring this pointer points to a valid
        // function matching the ErrorCallback signature. This is part of the
        // FFI contract - callers must pass correct function pointer types.
        // We use `transmute` here because function pointers require explicit
        // conversion when coming from void* in C FFI.
        Some(unsafe { std::mem::transmute(ptr) })
    }
}

unsafe fn cstr_arg<'a>(p: *const c_char, name: &str) -> Result<&'a str, String> {
    if p.is_null() {
        return Err(format!("{} is null", name));
    }
    // Note: In Rust 2024, explicit unsafe blocks are required even within unsafe fn
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
                    if let Some(on_error_fn) = ptr_to_error_callback(on_error) {
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
                if let Some(on_error_fn) = ptr_to_error_callback(on_error) {
                    let c_err =
                        CString::new(err_msg).unwrap_or_else(|_| CString::new("error").unwrap());
                    on_error_fn(c_err.as_ptr(), user_data);
                }
                return -2;
            }
        };

        // call the response callback
        if let Some(on_response_fn) = ptr_to_response_callback(on_response) {
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
        if let Some(on_error_fn) = ptr_to_error_callback(on_error) {
            let c_err = CString::new("panic in ProxyTraceAgent").unwrap();
            on_error_fn(c_err.as_ptr(), user_data);
        }
        -100
    })
}

/// Sends DogStatsD metrics over Unix Domain Socket or Named Pipe.
///
/// This is a fire-and-forget operation - metrics are sent to the DogStatsD service
/// as datagrams without waiting for a response. The DogStatsD protocol is text-based:
///
/// Format: `<metric_name>:<value>|<type>|@<sample_rate>|#<tags>`
///
/// Examples:
/// - Counter: `page.views:1|c`
/// - Gauge: `temperature:72.5|g|#env:prod`
/// - Histogram: `request.duration:250|h|@0.5|#endpoint:/api`
/// - Distribution: `response.size:512|d|#status:200`
/// - Set: `unique.visitors:user123|s`
///
/// Multiple metrics can be batched by separating them with newlines.
///
/// # Arguments
/// * `payload_ptr` - Pointer to the DogStatsD metric payload (text format)
/// * `payload_len` - Length of the payload in bytes
///
/// # Returns
/// * `0` on success (metric sent)
/// * `-1` on validation error (null/empty payload)
/// * `-2` on send error (socket/pipe unavailable)
/// * `-100` on panic (should never happen)
///
/// # Platform Support
/// - **Unix**: Uses Unix Domain Socket (default: `/tmp/datadog_dogstatsd.socket`)
/// - **Windows**: Uses Named Pipe (default: `datadog-dogstatsd`)
/// - Override with `LIBAGENT_DOGSTATSD_UDS` (Unix) or `LIBAGENT_DOGSTATSD_PIPE` (Windows)
///
/// # Example
/// ```c
/// const char* metric = "page.views:1|c|#env:prod";
/// int result = SendDogStatsDMetric(
///     (const uint8_t*)metric,
///     strlen(metric)
/// );
/// if (result != 0) {
///     fprintf(stderr, "Failed to send metric: %d\n", result);
/// }
/// ```
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn SendDogStatsDMetric(payload_ptr: *const u8, payload_len: usize) -> i32 {
    // Generate unique request ID for tracking concurrent requests
    let request_id = REQUEST_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    log_debug(&format!(
        "FFI call: SendDogStatsDMetric() [req:{}]",
        request_id
    ));

    catch_unwind(AssertUnwindSafe(|| {
        // Validate payload
        let payload = unsafe {
            if payload_ptr.is_null() || payload_len == 0 {
                log_debug(&format!(
                    "FFI SendDogStatsDMetric [req:{}]: validation failed: payload is null or empty",
                    request_id
                ));
                return -1;
            }
            std::slice::from_raw_parts(payload_ptr, payload_len)
        };

        // Log payload if it's valid UTF-8
        if let Ok(payload_str) = std::str::from_utf8(payload) {
            log_debug(&format!(
                "FFI SendDogStatsDMetric [req:{}]: sending {} bytes: {}",
                request_id,
                payload_len,
                payload_str.lines().next().unwrap_or(payload_str)
            ));
        } else {
            log_debug(&format!(
                "FFI SendDogStatsDMetric [req:{}]: sending {} bytes (binary)",
                request_id, payload_len
            ));
        }

        // Record DogStatsD request metrics
        crate::metrics::get_metrics().record_dogstatsd_request();

        // Reasonable timeout for datagram send
        let timeout = Duration::from_secs(5);

        // Send the metric
        #[cfg(unix)]
        let result = {
            let uds_path = get_dogstatsd_uds_path();
            dogstatsd::send_metric_over_uds(request_id, &uds_path, payload, timeout)
        };

        #[cfg(windows)]
        let result = {
            let pipe_name = get_dogstatsd_pipe_name();
            dogstatsd::send_metric_over_pipe(request_id, &pipe_name, payload, timeout)
        };

        #[cfg(all(not(unix), not(windows)))]
        let result = Err("platform not supported".to_string());

        match result {
            Ok(_) => {
                log_debug(&format!(
                    "FFI SendDogStatsDMetric [req:{}]: metric sent successfully",
                    request_id
                ));
                // Record successful send
                crate::metrics::get_metrics().record_dogstatsd_success();
                0
            }
            Err(err_msg) => {
                log_debug(&format!(
                    "FFI SendDogStatsDMetric [req:{}]: send failed: {}",
                    request_id, err_msg
                ));
                // Record error
                crate::metrics::get_metrics().record_dogstatsd_error();
                -2
            }
        }
    }))
    .unwrap_or_else(|_| {
        log_debug(&format!(
            "FFI SendDogStatsDMetric [req:{}]: panic occurred",
            request_id
        ));
        -100
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use serial_test::serial;

    #[cfg(unix)]
    use std::io::{Read, Write};
    #[cfg(unix)]
    use std::os::unix::net::{UnixDatagram, UnixListener};
    #[cfg(unix)]
    use std::sync::{Arc, Mutex};
    #[cfg(unix)]
    use tempfile::tempdir;

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
    fn test_get_metrics_function() {
        // Test that GetMetrics() returns a valid MetricsData struct
        let metrics_data = GetMetrics();

        // Test that we can access individual fields
        // These should be accessible (may be 0 if no activity)
        let _agent_spawns = metrics_data.agent_spawns;
        let _trace_spawns = metrics_data.trace_agent_spawns;
        let _get_requests = metrics_data.proxy_get_requests;
        let _post_requests = metrics_data.proxy_post_requests;
        let _status_2xx = metrics_data.proxy_2xx_responses;
        let _status_4xx = metrics_data.proxy_4xx_responses;
        let _response_time_all = metrics_data.response_time_ema_all;

        // Test that calling again still works
        let metrics_data2 = GetMetrics();
        assert_eq!(metrics_data2.agent_spawns, metrics_data.agent_spawns);
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

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_proxy_trace_agent_success() {
        use std::env;
        use std::ffi::CString;
        use std::thread;

        #[derive(Default)]
        struct CallbackResult {
            status: Option<u16>,
            headers: Option<String>,
            body: Option<Vec<u8>>,
            error: Option<String>,
        }

        extern "C" fn response_callback(
            status: u16,
            headers_data: *const u8,
            headers_len: usize,
            body_data: *const u8,
            body_len: usize,
            user_data: *mut ::std::os::raw::c_void,
        ) {
            let result = unsafe { &mut *(user_data as *mut CallbackResult) };
            result.status = Some(status);
            if !headers_data.is_null() && headers_len > 0 {
                let slice = unsafe { std::slice::from_raw_parts(headers_data, headers_len) };
                result.headers = Some(String::from_utf8_lossy(slice).to_string());
            }
            if !body_data.is_null() && body_len > 0 {
                let slice = unsafe { std::slice::from_raw_parts(body_data, body_len) };
                result.body = Some(slice.to_vec());
            }
        }

        extern "C" fn error_callback(
            error_message: *const ::std::os::raw::c_char,
            user_data: *mut ::std::os::raw::c_void,
        ) {
            let result = unsafe { &mut *(user_data as *mut CallbackResult) };
            if !error_message.is_null() {
                let msg = unsafe { CStr::from_ptr(error_message) }
                    .to_string_lossy()
                    .to_string();
                result.error = Some(msg);
            } else {
                result.error = Some(String::new());
            }
        }

        let tmp = tempdir().unwrap();
        let sock_path = tmp.path().join("ffi_proxy.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let mut collected = Vec::new();
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            collected.extend_from_slice(&buf[..n]);
                            if collected.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let response =
                    b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nX-Test: yes\r\n\r\nsuccess";
                let _ = stream.write_all(response);
            }
        });

        manager::stop();
        unsafe {
            env::set_var("LIBAGENT_TRACE_AGENT_UDS", &sock_path);
        }

        let method = CString::new("POST").unwrap();
        let path = CString::new("/ok").unwrap();
        let headers = CString::new("Content-Type: text/plain").unwrap();
        let body = b"payload";

        let mut result_box = Box::new(CallbackResult::default());
        let ctx_ptr = (&mut *result_box) as *mut CallbackResult as *mut _;
        let response_callback_ptr: *const ::std::os::raw::c_void = response_callback as *const _;
        let error_callback_ptr: *const ::std::os::raw::c_void = error_callback as *const _;

        let rc = ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            body.as_ptr(),
            body.len(),
            response_callback_ptr,
            error_callback_ptr,
            ctx_ptr,
        );

        let _ = server.join();
        unsafe {
            env::remove_var("LIBAGENT_TRACE_AGENT_UDS");
        }

        assert_eq!(rc, 0);
        assert!(result_box.error.is_none());
        assert_eq!(result_box.status, Some(200));
        assert!(
            result_box
                .headers
                .as_ref()
                .is_some_and(|h| h.contains("X-Test: yes"))
        );
        assert_eq!(result_box.body, Some(b"success".to_vec()));

        manager::stop();
    }

    #[test]
    #[serial]
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
    #[serial]
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
    #[serial]
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

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_send_dogstatsd_metric_success() {
        use std::env;
        use std::thread;

        let tmp = tempdir().unwrap();
        let sock_path = tmp.path().join("dogstatsd.sock");
        let server = UnixDatagram::bind(&sock_path).unwrap();

        let received = Arc::new(Mutex::new(None::<Vec<u8>>));
        let recv_clone = Arc::clone(&received);
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 256];
            if let Ok((n, _addr)) = server.recv_from(&mut buf) {
                *recv_clone.lock().unwrap() = Some(buf[..n].to_vec());
            }
        });

        unsafe {
            env::set_var("LIBAGENT_DOGSTATSD_UDS", &sock_path);
        }

        let payload = b"test.metric:1|c";
        let rc = SendDogStatsDMetric(payload.as_ptr(), payload.len());
        assert_eq!(rc, 0);

        let _ = handle.join();
        let data = received.lock().unwrap().clone().unwrap();
        assert_eq!(data, payload);

        unsafe {
            env::remove_var("LIBAGENT_DOGSTATSD_UDS");
        }
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn test_send_dogstatsd_metric_send_error() {
        use std::env;

        unsafe {
            env::set_var("LIBAGENT_DOGSTATSD_UDS", "/tmp/nonexistent_dogstatsd.sock");
        }

        let payload = b"test.metric:1|c";
        let rc = SendDogStatsDMetric(payload.as_ptr(), payload.len());
        assert_eq!(rc, -2);

        unsafe {
            env::remove_var("LIBAGENT_DOGSTATSD_UDS");
        }
    }

    #[test]
    fn test_send_dogstatsd_metric_validation_error() {
        let rc = SendDogStatsDMetric(std::ptr::null(), 0);
        assert_eq!(rc, -1);
    }

    #[test]
    fn test_trace_agent_readiness_reset_on_respawn() {
        // This test verifies that trace-agent readiness is properly reset when a new instance is spawned.
        // This is a regression test for the bug where readiness wasn't reset on respawn.

        let manager = crate::manager::get_manager();

        // Initially, no trace-agent is spawned, so readiness check should return Ok (skip check)
        let result = manager.ensure_trace_agent_ready();
        assert!(result.is_ok());

        // Initially, readiness should be false
        assert!(
            !manager.is_trace_agent_ready(),
            "Initial readiness should be false"
        );

        // Mark as ready to simulate successful readiness check
        manager.set_trace_agent_ready_for_test(true);

        assert!(
            manager.is_trace_agent_ready(),
            "Readiness should be true after manual set"
        );

        // Simulate respawn by calling reset_trace_agent_readiness (this is what spawn_process does)
        manager.reset_trace_agent_readiness();

        // Readiness should be reset to false
        assert!(
            !manager.is_trace_agent_ready(),
            "Trace-agent readiness should be reset to false after reset call"
        );

        // Now test that ensure_trace_agent_ready works correctly with the reset flag
        // Since no real trace-agent is running, it should return Ok (skip check when trace_child is None)
        let result = manager.ensure_trace_agent_ready();
        assert!(result.is_ok());
    }
}
