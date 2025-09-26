//! Minimal HTTP-over-UDS client for proxying requests to the trace agent.
//!
//! This module implements a small HTTP/1.1 client which connects to a Unix
//! Domain Socket, writes an HTTP request and parses the HTTP response.
//! It is used by the FFI function `ProxyTraceAgent`.

#[cfg(unix)]
use crate::manager::log_debug;
#[cfg(unix)]
use std::io::Write;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
pub use crate::http::Response;

#[cfg(unix)]
fn build_request(
    method: &str,
    path: &str,
    mut headers: Vec<(String, String)>,
    body: &[u8],
) -> Vec<u8> {
    crate::http::add_default_headers(&mut headers, body, "unix");
    crate::http::build_request(method, path, headers, body)
}

#[cfg(unix)]
pub fn request_over_uds(
    uds_path: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    timeout: Duration,
) -> Result<Response, String> {
    use std::os::unix::net::UnixStream;

    log_debug(&format!("UDS: connecting to socket: {}", uds_path));
    let mut stream = UnixStream::connect(uds_path).map_err(|e| {
        let err = format!("connect error ({}): {}", uds_path, e);
        log_debug(&format!("UDS: connection failed: {}", err));
        err
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let req = build_request(method, path, headers, body);
    log_debug(&format!(
        "UDS: sending {} {} request ({} bytes)",
        method,
        path,
        req.len()
    ));
    stream.write_all(&req).map_err(|e| {
        let err = format!("write error: {}", e);
        log_debug(&format!("UDS: write failed: {}", err));
        err
    })?;

    // Read headers
    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut stream, &mut buf).map_err(|e| {
        log_debug(&format!("UDS: failed to read headers: {}", e));
        e
    })?;
    let (head, rest) = buf.split_at(header_end);
    let head_str = std::str::from_utf8(head).map_err(|_| {
        let err = "invalid utf-8 in headers".to_string();
        log_debug(&format!("UDS: {}", err));
        err
    })?;
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| {
        let err = "empty response".to_string();
        log_debug(&format!("UDS: {}", err));
        err
    })?;
    let status = crate::http::parse_status_line(status_line).map_err(|e| {
        log_debug(&format!(
            "UDS: failed to parse status line '{}': {}",
            status_line, e
        ));
        e
    })?;
    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = crate::http::parse_headers(&header_str);

    log_debug(&format!("UDS: received status {}", status));

    // Read the response body
    let body = crate::http::read_http_body(&mut stream, rest, &headers_vec).map_err(|e| {
        log_debug(&format!("UDS: failed to read response body: {}", e));
        e
    })?;

    log_debug(&format!(
        "UDS: response complete, body size: {} bytes",
        body.len()
    ));

    Ok(Response {
        status,
        headers: headers_vec,
        body,
    })
}

// no non-Unix variant is provided; callers gate by #[cfg(unix)]

/// Parse a raw header-lines string into vector of (name,value) pairs.
pub fn parse_header_lines(input: &str) -> Vec<(String, String)> {
    crate::http::parse_header_lines(input)
}

/// Serialize headers back to a single string with CRLF line endings.
pub fn serialize_headers(headers: &[(String, String)]) -> String {
    crate::http::serialize_headers(headers)
}
