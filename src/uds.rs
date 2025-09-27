//! Minimal HTTP-over-UDS client for proxying requests to the trace agent.
//!
//! This module implements a small HTTP/1.1 client which connects to a Unix
//! Domain Socket, writes an HTTP request and parses the HTTP response.
//! It is used by the FFI function `ProxyTraceAgent`.

#[cfg(unix)]
use crate::logging::log_debug;
#[cfg(unix)]
use std::io::{Read, Write};
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
    request_id: u64,
    uds_path: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    timeout: Duration,
) -> Result<Response, String> {
    use std::os::unix::net::UnixStream;

    log_debug(&format!(
        "UDS [req:{}]: connecting to socket: {}",
        request_id, uds_path
    ));
    let mut stream = UnixStream::connect(uds_path).map_err(|e| {
        let err = format!("connect error ({}): {}", uds_path, e);
        log_debug(&format!(
            "UDS [req:{}]: connection failed: {}",
            request_id, err
        ));
        err
    })?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let req = build_request(method, path, headers, body);
    log_debug(&format!(
        "UDS [req:{}]: sending {} {} request ({} bytes total, body: {} bytes)",
        request_id,
        method,
        path,
        req.len(),
        body.len()
    ));

    // Log request details for debugging (text only, avoid binary formats like MessagePack)
    if let Ok(req_str) = std::str::from_utf8(&req) {
        log_debug(&format!(
            "UDS [req:{}]: request content:\n{}",
            request_id, req_str
        ));
    } else {
        log_debug(&format!(
            "UDS [req:{}]: request contains binary data ({} bytes)",
            request_id,
            req.len()
        ));
    }

    // Write the request and track progress
    log_debug(&format!("UDS [req:{}]: starting request write", request_id));
    let write_result = stream.write_all(&req);
    match &write_result {
        Ok(_) => log_debug(&format!(
            "UDS [req:{}]: request write completed successfully",
            request_id
        )),
        Err(e) => log_debug(&format!(
            "UDS [req:{}]: request write failed: {}",
            request_id, e
        )),
    }
    write_result.map_err(|e| {
        let err = format!("write error: {}", e);
        log_debug(&format!(
            "UDS [req:{}]: write operation failed: {}",
            request_id, err
        ));
        err
    })?;

    // Read headers
    log_debug(&format!(
        "UDS [req:{}]: starting to read response headers",
        request_id
    ));

    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut stream, &mut buf).map_err(|e| {
        log_debug(&format!(
            "UDS [req:{}]: failed to read headers after successful write: {}",
            request_id, e
        ));

        // Check if this is a broken pipe error, which indicates the server closed the connection
        if e.contains("Broken pipe") || e.contains("broken pipe") {
            log_debug(&format!(
                "UDS [req:{}]: server closed connection immediately after receiving request (no HTTP response sent)",
                request_id
            ));
            log_debug(&format!(
                "UDS [req:{}]: this typically means the endpoint '{}' is not supported by the trace agent",
                request_id, path
            ));
        }

        // Try to read any available data to see what (if anything) the server sent before closing
        let mut temp_buf = Vec::new();
        match stream.read_to_end(&mut temp_buf) {
            Ok(0) => log_debug(&format!(
                "UDS [req:{}]: no additional data available from closed connection",
                request_id
            )),
            Ok(n) => {
                log_debug(&format!(
                    "UDS [req:{}]: server sent {} bytes before closing: {:?}",
                    request_id, n, &temp_buf[..std::cmp::min(n, 100)] // Limit to first 100 bytes
                ));
                if let Ok(partial_response) = std::str::from_utf8(&temp_buf[..std::cmp::min(n, 200)]) {
                    log_debug(&format!(
                        "UDS [req:{}]: partial response content: {}",
                        request_id, partial_response
                    ));
                }
            }
            Err(read_err) => log_debug(&format!(
                "UDS [req:{}]: failed to read remaining data: {}",
                request_id, read_err
            )),
        }

        e
    })?;
    log_debug(&format!(
        "UDS [req:{}]: read {} bytes of headers, header_end at {}",
        request_id,
        buf.len(),
        header_end
    ));

    let (head, rest) = buf.split_at(header_end);
    log_debug(&format!(
        "UDS [req:{}]: header section is {} bytes, remaining data: {} bytes",
        request_id,
        head.len(),
        rest.len()
    ));

    let head_str = std::str::from_utf8(head).map_err(|_| {
        let err = "invalid utf-8 in headers".to_string();
        log_debug(&format!("UDS [req:{}]: {}", request_id, err));
        err
    })?;
    log_debug(&format!(
        "UDS [req:{}]: header content:\n{}",
        request_id, head_str
    ));

    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| {
        let err = "empty response".to_string();
        log_debug(&format!("UDS [req:{}]: {}", request_id, err));
        err
    })?;
    log_debug(&format!(
        "UDS [req:{}]: status line: '{}'",
        request_id, status_line
    ));

    let status = crate::http::parse_status_line(status_line).map_err(|e| {
        log_debug(&format!(
            "UDS [req:{}]: failed to parse status line '{}': {}",
            request_id, status_line, e
        ));
        e
    })?;
    log_debug(&format!(
        "UDS [req:{}]: parsed status: {}",
        request_id, status
    ));

    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = crate::http::parse_headers(&header_str);
    log_debug(&format!(
        "UDS [req:{}]: parsed {} headers",
        request_id,
        headers_vec.len()
    ));

    // Read the response body
    log_debug(&format!(
        "UDS [req:{}]: starting to read response body",
        request_id
    ));
    let body = crate::http::read_http_body(&mut stream, rest, &headers_vec).map_err(|e| {
        log_debug(&format!(
            "UDS [req:{}]: failed to read response body: {}",
            request_id, e
        ));
        e
    })?;
    log_debug(&format!(
        "UDS [req:{}]: read response body of {} bytes",
        request_id,
        body.len()
    ));

    // Log response details for debugging (text only, avoid binary formats like MessagePack)
    if let Ok(body_str) = std::str::from_utf8(&body) {
        log_debug(&format!(
            "UDS [req:{}]: response body content:\n{}",
            request_id, body_str
        ));
    } else {
        log_debug(&format!(
            "UDS [req:{}]: response body contains binary data ({} bytes)",
            request_id,
            body.len()
        ));
    }

    log_debug(&format!(
        "UDS [req:{}]: response complete, body size: {} bytes",
        request_id,
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
