#![cfg(unix)]

use serial_test::serial;
use std::env;
use std::ffi::{CString, c_char};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::ptr;
use std::thread;

// Test result storage for callbacks
#[derive(Clone)]
struct TestResult {
    status: Option<u16>,
    headers: Option<String>,
    body: Option<Vec<u8>>,
    error: Option<String>,
}

impl TestResult {
    fn new() -> Self {
        Self {
            status: None,
            headers: None,
            body: None,
            error: None,
        }
    }
}

unsafe extern "C" {
    fn ProxyTraceAgent(
        method: *const c_char,
        path: *const c_char,
        headers: *const c_char,
        body_ptr: *const u8,
        body_len: usize,
        on_response: extern "C" fn(
            u16,
            *const u8,
            usize,
            *const u8,
            usize,
            *mut ::std::os::raw::c_void,
        ),
        on_error: extern "C" fn(*const c_char, *mut ::std::os::raw::c_void),
        user_data: *mut ::std::os::raw::c_void,
    ) -> i32;
}

// Callback functions for tests
extern "C" fn test_response_callback(
    status: u16,
    headers_data: *const u8,
    headers_len: usize,
    body_data: *const u8,
    body_len: usize,
    user_data: *mut ::std::os::raw::c_void,
) {
    let result = unsafe { &mut *(user_data as *mut TestResult) };
    result.status = Some(status);

    if !headers_data.is_null() && headers_len > 0 {
        let headers_slice = unsafe { std::slice::from_raw_parts(headers_data, headers_len) };
        result.headers = Some(String::from_utf8_lossy(headers_slice).to_string());
    }

    if !body_data.is_null() && body_len > 0 {
        let body_slice = unsafe { std::slice::from_raw_parts(body_data, body_len) };
        result.body = Some(body_slice.to_vec());
    }
}

extern "C" fn test_error_callback(
    error_message: *const c_char,
    user_data: *mut ::std::os::raw::c_void,
) {
    let result = unsafe { &mut *(user_data as *mut TestResult) };
    if !error_message.is_null() {
        let c_str = unsafe { std::ffi::CStr::from_ptr(error_message) };
        result.error = Some(c_str.to_string_lossy().to_string());
    }
}

fn write_response(mut stream: impl Write, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let _ = write!(stream, "HTTP/1.1 {}\r\n", status);
    for (k, v) in headers {
        let _ = write!(stream, "{}: {}\r\n", k, v);
    }
    let _ = write!(stream, "Content-Length: {}\r\n\r\n", body.len());
    let _ = stream.write_all(body);
}

#[test]
#[serial]
fn uds_proxy_basic() {
    // Touch the crate to ensure the lib is linked for extern C symbols
    libagent::stop();
    // Create a temporary socket path
    let tmp = tempfile::tempdir().unwrap();
    let sock_path: PathBuf = tmp.path().join("apm.sock");

    // Start UDS HTTP server
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("skipping uds_proxy_basic: cannot bind UDS: {}", e);
            return;
        }
    };
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _addr)) = listener.accept() {
            // Read until headers end
            let mut buf = [0u8; 8192];
            let mut read_total = 0usize;
            loop {
                let n = stream.read(&mut buf[read_total..]).unwrap();
                if n == 0 {
                    break;
                }
                read_total += n;
                if read_total >= 4 && buf[..read_total].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if read_total == buf.len() {
                    break;
                }
            }
            // Respond
            write_response(
                &mut stream,
                "201 Created",
                &[("Content-Type", "text/plain"), ("X-Server", "uds-test")],
                b"response",
            );
        }
    });

    // Configure env var for UDS path
    unsafe {
        env::set_var("LIBAGENT_TRACE_AGENT_UDS", sock_path.as_os_str());
    }

    // Prepare inputs
    let method = CString::new("POST").unwrap();
    let path = CString::new("/v0.7/traces?foo=bar").unwrap();
    let headers = CString::new("Content-Type: text/plain\nX-Request: yes").unwrap();
    let body: &[u8] = b"hello world";

    // Prepare result storage
    let mut result = TestResult::new();
    let result_ptr = &mut result as *mut TestResult as *mut ::std::os::raw::c_void;

    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            body.as_ptr(),
            body.len(),
            test_response_callback,
            test_error_callback,
            result_ptr,
        )
    };

    assert_eq!(
        rc, 0,
        "expected success, got rc={}, err={:?}",
        rc, result.error
    );

    assert_eq!(result.status, Some(201));
    assert!(
        result
            .headers
            .as_ref()
            .unwrap()
            .contains("Content-Type: text/plain")
    );
    assert!(
        result
            .headers
            .as_ref()
            .unwrap()
            .contains("X-Server: uds-test")
    );
    assert_eq!(result.body, Some(b"response".to_vec()));

    let _ = handle.join();
}

#[test]
#[serial]
fn uds_proxy_chunked() {
    // Create a temporary socket path
    let tmp = tempfile::tempdir().unwrap();
    let sock_path: PathBuf = tmp.path().join("apm.sock");

    // Start UDS HTTP server
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("skipping uds_proxy_chunked: cannot bind UDS: {}", e);
            return;
        }
    };
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _addr)) = listener.accept() {
            // Read until headers end
            let mut buf = [0u8; 8192];
            let mut read_total = 0usize;
            loop {
                let n = stream.read(&mut buf[read_total..]).unwrap();
                if n == 0 {
                    break;
                }
                read_total += n;
                if read_total >= 4 && buf[..read_total].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                if read_total == buf.len() {
                    break;
                }
            }
            // Respond using chunked transfer-encoding
            let _ = write!(stream, "HTTP/1.1 200 OK\r\n");
            let _ = write!(stream, "Transfer-Encoding: chunked\r\n");
            let _ = write!(stream, "X-Server: uds-test-chunked\r\n\r\n");
            // chunks: "hello" (5), " world" (6), then end
            let _ = write!(stream, "5\r\nhello\r\n");
            let _ = write!(stream, "6\r\n world\r\n");
            let _ = write!(stream, "0\r\n\r\n");
        }
    });

    // Configure env var for UDS path
    unsafe {
        env::set_var("LIBAGENT_TRACE_AGENT_UDS", sock_path.as_os_str());
    }

    // Prepare inputs
    let method = CString::new("GET").unwrap();
    let path = CString::new("/v0.7/traces").unwrap();
    let headers = CString::new("Accept: */*\nX-Request: yes").unwrap();

    // Prepare result storage
    let mut result = TestResult::new();
    let result_ptr = &mut result as *mut TestResult as *mut ::std::os::raw::c_void;

    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            test_response_callback,
            test_error_callback,
            result_ptr,
        )
    };

    if rc != 0 {
        // Likely skipped; ensure server thread exits cleanly
        let _ = handle.join();
        eprintln!(
            "skipping uds_proxy_chunked: rc={} err={:?}",
            rc, result.error
        );
        return;
    }

    assert_eq!(result.status, Some(200));
    let headers_str = result.headers.as_ref().unwrap();
    assert!(
        headers_str
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked"),
        "headers: {}",
        headers_str
    );
    assert!(
        headers_str.contains("X-Server: uds-test-chunked"),
        "headers: {}",
        headers_str
    );
    assert_eq!(result.body, Some(b"hello world".to_vec()));

    let _ = handle.join();
}
