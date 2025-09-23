//! Minimal HTTP-over-Windows-Named-Pipe client for proxying requests to the trace agent (Windows).
//!
//! Connects to a Windows Named Pipe (\\.\\pipe\\<name>), writes an HTTP/1.1 request,
//! and parses the HTTP response. Mirrors the behavior of the Unix UDS client.
//!
//! Timeout is enforced using a separate thread with cancellation support (default: 50 seconds).
//! The request thread can be interrupted at key points during the HTTP transaction.

use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub use crate::http::Response;

// no need for wide conversions; std::fs::OpenOptions handles wide path internally

fn build_request(
    method: &str,
    path: &str,
    mut headers: Vec<(String, String)>,
    body: &[u8],
) -> Vec<u8> {
    crate::http::add_default_headers(&mut headers, body, "pipe");
    crate::http::build_request(method, path, headers, body)
}

pub fn request_over_named_pipe(
    pipe_name: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    timeout: Duration,
) -> Result<Response, String> {
    // Compose full pipe path if only name is given
    let full_path = if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name.to_string()
    } else {
        format!(r"\\.\pipe\{}", pipe_name)
    };

    // Use a channel to communicate the result and cancellation signal
    let (tx, rx) = mpsc::channel();
    let (cancel_tx, cancel_rx) = mpsc::channel::<()>();

    // Spawn a thread to perform the request
    let _handle = {
        let pipe_name = full_path.clone();
        let method = method.to_string();
        let path = path.to_string();
        let headers = headers.clone();
        let body = body.to_vec();

        thread::spawn(move || {
            // Check for cancellation signal periodically
            let check_cancel = || {
                match cancel_rx.try_recv() {
                    Ok(_) => Err("cancelled".to_string()),
                    Err(mpsc::TryRecvError::Empty) => Ok(()),
                    Err(mpsc::TryRecvError::Disconnected) => Ok(()), // sender dropped, continue
                }
            };

            let result = perform_request_cancellable(
                &pipe_name,
                &method,
                &path,
                headers,
                &body,
                check_cancel,
            );
            let _ = tx.send(result);
        })
    };

    // Wait for either the result or timeout
    match rx.recv_timeout(timeout) {
        Ok(result) => {
            // Signal cancellation in case the thread is still running
            let _ = cancel_tx.send(());
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // Signal cancellation to stop the thread
            let _ = cancel_tx.send(());
            // Give a brief moment for the thread to clean up
            thread::sleep(Duration::from_millis(10));
            Err(format!("request timed out after {:?}", timeout))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("request thread disconnected unexpectedly".to_string())
        }
    }
}

fn perform_request_cancellable<F>(
    full_path: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    check_cancel: F,
) -> Result<Response, String>
where
    F: Fn() -> Result<(), String>,
{
    // Check for cancellation before starting
    check_cancel()?;

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(full_path)
        .map_err(|e| format!("failed to open pipe {}: {}", full_path, e))?;

    // Check for cancellation after opening pipe
    check_cancel()?;

    // Write request
    let req = build_request(method, path, headers, body);
    file.write_all(&req)
        .map_err(|e| format!("write error: {}", e))?;

    // Check for cancellation after writing
    check_cancel()?;

    // Read response headers
    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut file, &mut buf)?;
    let (head, rest) = buf.split_at(header_end);
    let head_str = std::str::from_utf8(head).map_err(|_| "invalid utf-8 in headers".to_string())?;
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| "empty response".to_string())?;
    let status = crate::http::parse_status_line(status_line)?;
    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = crate::http::parse_headers(&header_str);

    // Check for cancellation after reading headers
    check_cancel()?;

    // Read the response body
    let body = crate::http::read_http_body(&mut file, rest, &headers_vec)?;

    Ok(Response {
        status,
        headers: headers_vec,
        body,
    })
}

#[allow(dead_code)]
fn perform_request(
    full_path: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
) -> Result<Response, String> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(full_path)
        .map_err(|e| format!("failed to open pipe {}: {}", full_path, e))?;

    // Write request
    let req = build_request(method, path, headers, body);
    file.write_all(&req)
        .map_err(|e| format!("write error: {}", e))?;

    // Read response headers
    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut file, &mut buf)?;
    let (head, rest) = buf.split_at(header_end);
    let head_str = std::str::from_utf8(head).map_err(|_| "invalid utf-8 in headers".to_string())?;
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| "empty response".to_string())?;
    let status = crate::http::parse_status_line(status_line)?;
    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = crate::http::parse_headers(&header_str);

    // Read the response body
    let body = crate::http::read_http_body(&mut file, rest, &headers_vec)?;

    Ok(Response {
        status,
        headers: headers_vec,
        body,
    })
}
