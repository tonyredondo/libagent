//! Minimal HTTP-over-Windows-Named-Pipe client for proxying requests to the trace agent (Windows).
//!
//! Connects to a Windows Named Pipe (\\.\\pipe\\<name>), writes an HTTP/1.1 request,
//! and parses the HTTP response. Mirrors the behavior of the Unix UDS client.

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::time::Duration;

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

// no need for wide conversions; std::fs::OpenOptions handles wide path internally

fn build_request(
    method: &str,
    path: &str,
    mut headers: Vec<(String, String)>,
    body: &[u8],
) -> Vec<u8> {
    let mut has_content_length = false;
    let mut has_host = false;
    let mut has_connection = false;
    for (name, _) in &headers {
        let n = name.to_ascii_lowercase();
        if n == "content-length" {
            has_content_length = true;
        } else if n == "host" {
            has_host = true;
        } else if n == "connection" {
            has_connection = true;
        }
    }
    if !has_content_length {
        headers.push(("Content-Length".to_string(), body.len().to_string()));
    }
    if !has_host {
        headers.push(("Host".to_string(), "pipe".to_string()));
    }
    if !has_connection {
        headers.push(("Connection".to_string(), "close".to_string()));
    }

    let mut req = Vec::with_capacity(256 + headers.len() * 32 + body.len());
    req.extend_from_slice(method.as_bytes());
    req.extend_from_slice(b" ");
    req.extend_from_slice(path.as_bytes());
    req.extend_from_slice(b" HTTP/1.1\r\n");
    for (name, value) in headers {
        req.extend_from_slice(name.as_bytes());
        req.extend_from_slice(b": ");
        req.extend_from_slice(value.as_bytes());
        req.extend_from_slice(b"\r\n");
    }
    req.extend_from_slice(b"\r\n");
    req.extend_from_slice(body);
    req
}

fn parse_status_line(line: &str) -> Result<u16, String> {
    let mut parts = line.split_whitespace();
    let proto = parts
        .next()
        .ok_or_else(|| "malformed status line".to_string())?;
    if !proto.starts_with("HTTP/") {
        return Err("malformed status line".to_string());
    }
    let code_str = parts
        .next()
        .ok_or_else(|| "missing status code".to_string())?;
    let code: u16 = code_str
        .parse()
        .map_err(|_| "invalid status code".to_string())?;
    Ok(code)
}

fn parse_headers(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in raw.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(':') {
            let (name, value) = line.split_at(idx);
            let value = value[1..].trim_start();
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}

fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let lname = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(n, _)| n.to_ascii_lowercase() == lname)
        .map(|(_, v)| v.as_str())
}

fn memchr_crlf_crlf(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    for i in 0..=buf.len() - 4 {
        if &buf[i..i + 4] == b"\r\n\r\n" {
            return Some(i + 4);
        }
    }
    None
}

fn read_until_double_crlf(stream: &mut dyn Read, buf: &mut Vec<u8>) -> Result<usize, String> {
    let mut tmp = [0u8; 1024];
    loop {
        if let Some(pos) = memchr_crlf_crlf(buf) {
            return Ok(pos);
        }
        let n = stream
            .read(&mut tmp)
            .map_err(|e| format!("read error: {}", e))?;
        if n == 0 {
            return Err("unexpected EOF while reading headers".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 1024 * 1024 {
            return Err("headers too large".to_string());
        }
    }
}

fn read_exact_len(stream: &mut dyn Read, len: usize) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; len];
    let mut read_total = 0;
    while read_total < len {
        let n = stream
            .read(&mut out[read_total..])
            .map_err(|e| format!("read error: {}", e))?;
        if n == 0 {
            return Err("unexpected EOF while reading body".to_string());
        }
        read_total += n;
    }
    Ok(out)
}

fn read_chunked(stream: &mut dyn Read) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    let mut line_buf = Vec::new();
    let mut tmp = [0u8; 1];
    loop {
        line_buf.clear();
        let mut last = 0u8;
        loop {
            let n = stream
                .read(&mut tmp)
                .map_err(|e| format!("read error: {}", e))?;
            if n == 0 {
                return Err("unexpected EOF in chunked encoding".to_string());
            }
            let b = tmp[0];
            line_buf.push(b);
            if last == b'\r' && b == b'\n' {
                break;
            }
            last = b;
            if line_buf.len() > 1024 {
                return Err("chunk size line too long".to_string());
            }
        }
        if line_buf.len() < 2 {
            return Err("invalid chunk size line".to_string());
        }
        let line = std::str::from_utf8(&line_buf[..line_buf.len() - 2])
            .map_err(|_| "invalid utf-8 in chunk size".to_string())?;
        let size_str = line.split(';').next().unwrap_or("");
        let size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|_| "invalid chunk size".to_string())?;
        if size == 0 {
            let _ = stream.read(&mut tmp);
            let _ = stream.read(&mut tmp);
            break;
        }
        let mut chunk = read_exact_len(stream, size)?;
        body.append(&mut chunk);
        let mut crlf = [0u8; 2];
        stream
            .read_exact(&mut crlf)
            .map_err(|e| format!("read error: {}", e))?;
        if &crlf != b"\r\n" {
            return Err("invalid chunk terminator".to_string());
        }
    }
    Ok(body)
}

pub fn request_over_named_pipe(
    pipe_name: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
    _timeout: Duration,
) -> Result<Response, String> {
    // Compose full pipe path if only name is given
    let full_path = if pipe_name.starts_with(r"\\.\pipe\") {
        pipe_name.to_string()
    } else {
        format!(r"\\.\pipe\{}", pipe_name)
    };

    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&full_path)
        .map_err(|e| format!("failed to open pipe {}: {}", full_path, e))?;

    // Write request
    let req = build_request(method, path, headers, body);
    file.write_all(&req)
        .map_err(|e| format!("write error: {}", e))?;

    // Read response headers
    let mut buf = Vec::with_capacity(8192);
    let header_end = read_until_double_crlf(&mut file, &mut buf)?;
    let (head, rest) = buf.split_at(header_end);
    let head_str = std::str::from_utf8(head).map_err(|_| "invalid utf-8 in headers".to_string())?;
    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| "empty response".to_string())?;
    let status = parse_status_line(status_line)?;
    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = parse_headers(&header_str);

    // Read body
    let body = if let Some(len_str) = header_lookup(&headers_vec, "Content-Length") {
        let len: usize = len_str
            .parse()
            .map_err(|_| "invalid Content-Length".to_string())?;
        let mut body = Vec::with_capacity(len);
        if !rest.is_empty() {
            body.extend_from_slice(rest);
        }
        if body.len() < len {
            let mut remaining = len - body.len();
            let mut chunk = vec![0u8; remaining.min(16 * 1024)];
            while remaining > 0 {
                let n = file
                    .read(&mut chunk)
                    .map_err(|e| format!("read error: {}", e))?;
                if n == 0 {
                    return Err("unexpected EOF while reading body".to_string());
                }
                body.extend_from_slice(&chunk[..n]);
                remaining -= n;
                if chunk.len() > remaining {
                    chunk.resize(remaining, 0);
                }
            }
        }
        body
    } else if matches!(header_lookup(&headers_vec, "Transfer-Encoding"), Some(v) if v.to_ascii_lowercase().contains("chunked"))
    {
        // Similar to UDS, create a reader draining `rest` first then the file
        struct RestThen<'a, R: Read> {
            rest: &'a [u8],
            inner: R,
        }
        impl<R: Read> Read for RestThen<'_, R> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if !self.rest.is_empty() {
                    let n = self.rest.read(buf)?;
                    self.rest = &self.rest[n..];
                    Ok(n)
                } else {
                    self.inner.read(buf)
                }
            }
        }
        let mut reader = RestThen {
            rest,
            inner: &mut file,
        };
        read_chunked(&mut reader)?
    } else {
        // read to EOF
        let mut body = rest.to_vec();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = file
                .read(&mut chunk)
                .map_err(|e| format!("read error: {}", e))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body
    };

    Ok(Response {
        status,
        headers: headers_vec,
        body,
    })
}
