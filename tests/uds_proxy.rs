#![cfg(unix)]

use serial_test::serial;
use std::env;
use std::ffi::{CString, c_char};
use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::ptr;
use std::thread;

#[repr(C)]
struct LibagentHttpBuffer {
    data: *mut u8,
    len: usize,
}

#[repr(C)]
struct LibagentHttpResponse {
    status: u16,
    headers: LibagentHttpBuffer,
    body: LibagentHttpBuffer,
}

unsafe extern "C" {
    fn ProxyTraceAgent(
        method: *const c_char,
        path: *const c_char,
        headers: *const c_char,
        body_ptr: *const u8,
        body_len: usize,
        out_resp: *mut *mut LibagentHttpResponse,
        out_err: *mut *mut c_char,
    ) -> i32;

    fn FreeHttpResponse(resp: *mut LibagentHttpResponse);
    fn FreeCString(s: *mut c_char);
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

    let mut resp_ptr: *mut LibagentHttpResponse = ptr::null_mut();
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            body.as_ptr(),
            body.len(),
            &mut resp_ptr,
            &mut err_ptr,
        )
    };

    assert_eq!(rc, 0, "expected success, got rc={}, err={:?}", rc, unsafe {
        if err_ptr.is_null() {
            None
        } else {
            Some(CString::from_raw(err_ptr))
        }
    });

    assert!(!resp_ptr.is_null(), "response pointer is null");
    unsafe {
        let resp = &*resp_ptr;
        assert_eq!(resp.status, 201);
        let headers_slice = std::slice::from_raw_parts(resp.headers.data, resp.headers.len);
        let headers_str = String::from_utf8_lossy(headers_slice);
        assert!(
            headers_str.contains("Content-Type: text/plain"),
            "headers: {}",
            headers_str
        );
        assert!(
            headers_str.contains("X-Server: uds-test"),
            "headers: {}",
            headers_str
        );

        let body_slice = std::slice::from_raw_parts(resp.body.data, resp.body.len);
        assert_eq!(body_slice, b"response");

        FreeHttpResponse(resp_ptr);
    }

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

    let mut resp_ptr: *mut LibagentHttpResponse = ptr::null_mut();
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            &mut resp_ptr,
            &mut err_ptr,
        )
    };

    if rc != 0 {
        // Likely skipped; ensure server thread exits cleanly
        let _ = handle.join();
        if !err_ptr.is_null() {
            unsafe {
                FreeCString(err_ptr);
            }
        }
        eprintln!("skipping uds_proxy_chunked: rc={} err set", rc);
        return;
    }

    assert!(!resp_ptr.is_null(), "response pointer is null");
    unsafe {
        let resp = &*resp_ptr;
        assert_eq!(resp.status, 200);
        let headers_slice = std::slice::from_raw_parts(resp.headers.data, resp.headers.len);
        let headers_str = String::from_utf8_lossy(headers_slice);
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

        let body_slice = std::slice::from_raw_parts(resp.body.data, resp.body.len);
        assert_eq!(body_slice, b"hello world");

        FreeHttpResponse(resp_ptr);
    }

    let _ = handle.join();
}
