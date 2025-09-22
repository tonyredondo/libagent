//! C FFI surface for libagent.
//!
//! This module exposes a minimal set of functions intended to be consumed
//! from non-Rust languages. The business logic is provided by
//! `crate::manager`, and these functions simply forward to those safe
//! Rust APIs.

#[cfg(unix)]
use crate::config::get_trace_agent_uds_path;
use crate::manager;
use crate::uds;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::time::Duration;

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

/// HTTP response container returned by ProxyTraceAgentUds.
#[repr(C)]
pub struct LibagentHttpResponse {
    pub status: u16,
    pub headers: LibagentHttpBuffer, // CRLF-separated header lines
    pub body: LibagentHttpBuffer,
}

fn to_c_error(out_err: *mut *mut c_char, msg: String) {
    unsafe {
        if !out_err.is_null() {
            let c = CString::new(msg).unwrap_or_else(|_| CString::new("error").unwrap());
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

/// Proxy an HTTP request over a Unix Domain Socket to the trace-agent and return the response.
///
/// Parameters:
/// - uds_path: path to the UDS (e.g., "/var/run/datadog/apm.socket")
/// - method: HTTP method (e.g., "GET", "POST")
/// - path: HTTP request path + optional query (e.g., "/v0.7/traces")
/// - headers: CRLF or LF separated lines in the form "Name: Value"
/// - body_ptr/body_len: optional body bytes (pass null/0 for none)
/// - out_resp: on success, set to an allocated response pointer to be freed with FreeHttpResponse
/// - out_err: on failure, set to an allocated error string to be freed with FreeCString
///   Returns 0 on success, negative on error.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub extern "C" fn ProxyTraceAgentUds(
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

        // validate args
        let method = unsafe {
            match cstr_arg(method, "method", out_err) {
                Ok(s) => s,
                Err(e) => return e,
            }
        };
        let path = unsafe {
            match cstr_arg(path, "path", out_err) {
                Ok(s) => s,
                Err(e) => return e,
            }
        };
        let headers_str = unsafe {
            match cstr_arg(headers, "headers", out_err) {
                Ok(s) => s,
                Err(e) => return e,
            }
        };

        let hdrs = uds::parse_header_lines(headers_str);
        let body = unsafe {
            if body_ptr.is_null() || body_len == 0 {
                &[][..]
            } else {
                std::slice::from_raw_parts(body_ptr, body_len)
            }
        };

        // Reasonable default timeout
        let timeout = Duration::from_secs(10);
        // Resolve UDS path (Unix only); on non-Unix request_over_uds will error.
        #[cfg(unix)]
        let uds_path = get_trace_agent_uds_path();
        #[cfg(not(unix))]
        let uds_path = String::new();

        match uds::request_over_uds(&uds_path, method, path, hdrs, body, timeout) {
            Ok(resp) => {
                let headers_txt = uds::serialize_headers(&resp.headers);
                let headers_buf = make_buf(headers_txt.into_bytes());
                let body_buf = make_buf(resp.body);
                let boxed = Box::new(LibagentHttpResponse {
                    status: resp.status,
                    headers: headers_buf,
                    body: body_buf,
                });
                unsafe {
                    if !out_resp.is_null() {
                        *out_resp = Box::into_raw(boxed);
                    }
                }
                0
            }
            Err(err) => {
                to_c_error(out_err, err);
                -2
            }
        }
    }))
    .unwrap_or_else(|_| {
        to_c_error(out_err, "panic in ProxyTraceAgentUds".to_string());
        -100
    })
}
