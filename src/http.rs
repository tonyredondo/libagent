//! Common HTTP parsing and utilities shared between Unix UDS and Windows Named Pipe clients.
//!
//! This module contains HTTP/1.1 parsing logic that's identical between the two transport
//! implementations, extracted to reduce code duplication.

use std::io::Read;

/// HTTP response container.
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Parse an HTTP status line (e.g., "HTTP/1.1 200 OK") and return the status code.
pub fn parse_status_line(line: &str) -> Result<u16, String> {
    // Example: HTTP/1.1 200 OK
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

/// Parse raw header lines into a vector of (name, value) pairs.
pub fn parse_headers(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in raw.split("\r\n") {
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(':') {
            let (name, value) = line.split_at(idx);
            let value = value[1..].trim_start(); // skip ':' then trim leading spaces
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}

/// Look up a header value by name (case-insensitive).
pub fn header_lookup<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let lname = name.to_ascii_lowercase();
    headers
        .iter()
        .find(|(n, _)| n.to_ascii_lowercase() == lname)
        .map(|(_, v)| v.as_str())
}

/// Add default headers to a header list if they are missing.
///
/// This function ensures that Content-Length, Host, and Connection headers
/// are present with sensible defaults.
pub fn add_default_headers(headers: &mut Vec<(String, String)>, body: &[u8], host: &str) {
    let mut has_content_length = false;
    let mut has_host = false;
    let mut has_connection = false;
    for (name, _) in &*headers {
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
        headers.push(("Host".to_string(), host.to_string()));
    }
    if !has_connection {
        headers.push(("Connection".to_string(), "close".to_string()));
    }
}

/// Build an HTTP request from method, path, headers, and body.
pub fn build_request(
    method: &str,
    path: &str,
    mut headers: Vec<(String, String)>,
    body: &[u8],
) -> Vec<u8> {
    add_default_headers(&mut headers, body, "localhost");

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

/// Find the position of "\r\n\r\n" in a buffer.
pub fn memchr_crlf_crlf(buf: &[u8]) -> Option<usize> {
    // Return index of end of header (position just after \r\n\r\n)
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

/// Read from stream until we see \r\n\r\n, returning the position after the headers.
pub fn read_until_double_crlf(stream: &mut dyn Read, buf: &mut Vec<u8>) -> Result<usize, String> {
    // Read until we see \r\n\r\n. Return index after the header end.
    let mut tmp = [0u8; 1024];
    loop {
        if let Some(pos) = memchr_crlf_crlf(buf) {
            return Ok(pos);
        }
        let n = stream
            .read(&mut tmp)
            .map_err(|e| format!("read error: {}", e))?;
        if n == 0 {
            // EOF before headers complete
            return Err("unexpected EOF while reading headers".to_string());
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.len() > 1024 * 1024 {
            // 1MB header safety cap
            return Err("headers too large".to_string());
        }
    }
}

/// Read exactly `len` bytes from the stream.
pub fn read_exact_len(stream: &mut dyn Read, len: usize) -> Result<Vec<u8>, String> {
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

/// Read a chunked-encoded body from the stream.
pub fn read_chunked(stream: &mut dyn Read) -> Result<Vec<u8>, String> {
    // Very small chunked decoder sufficient for agent responses
    let mut body = Vec::new();
    let mut line_buf = Vec::new();
    let mut tmp = [0u8; 1];
    loop {
        // read a line with \r\n
        line_buf.clear();
        // read until CRLF
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
                // guard
                return Err("chunk size line too long".to_string());
            }
        }
        // line_buf ends with CRLF
        if line_buf.len() < 2 {
            return Err("invalid chunk size line".to_string());
        }
        let line = std::str::from_utf8(&line_buf[..line_buf.len() - 2])
            .map_err(|_| "invalid utf-8 in chunk size".to_string())?;
        let size_str = line.split(';').next().unwrap_or("");
        let size = usize::from_str_radix(size_str.trim(), 16)
            .map_err(|_| "invalid chunk size".to_string())?;
        if size == 0 {
            // read trailing CRLF after 0-size chunk and possible trailers (skip)
            // For simplicity, read and discard a final CRLF
            // Attempt to read two bytes CRLF; ignore errors
            let _ = stream.read(&mut tmp);
            let _ = stream.read(&mut tmp);
            break;
        }
        let mut chunk = read_exact_len(stream, size)?;
        body.append(&mut chunk);
        // read the trailing CRLF after the chunk
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

/// Parse a raw header-lines string into vector of (name,value) pairs.
/// Used by the FFI layer.
pub fn parse_header_lines(input: &str) -> Vec<(String, String)> {
    let mut headers = Vec::new();
    for raw_line in input.split('\n') {
        let line = raw_line.trim_end_matches('\r').trim();
        if line.is_empty() {
            continue;
        }
        if let Some(idx) = line.find(':') {
            let (name, value) = line.split_at(idx);
            let value = value[1..].trim_start();
            if !name.is_empty() {
                headers.push((name.to_string(), value.to_string()));
            }
        }
    }
    headers
}

/// Serialize headers back to a single string with CRLF line endings.
pub fn serialize_headers(headers: &[(String, String)]) -> String {
    let mut s = String::new();
    for (k, v) in headers {
        s.push_str(k);
        s.push_str(": ");
        s.push_str(v);
        s.push_str("\r\n");
    }
    s
}

/// A reader that first reads from remaining bytes, then from an inner reader.
pub struct RestThen<'a, R: Read> {
    rest: &'a [u8],
    pos: usize,
    inner: R,
}

impl<'a, R: Read> RestThen<'a, R> {
    pub fn new(rest: &'a [u8], inner: R) -> Self {
        Self {
            rest,
            pos: 0,
            inner,
        }
    }
}

impl<R: Read> Read for RestThen<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos < self.rest.len() {
            let remaining = &self.rest[self.pos..];
            let n = std::cmp::min(buf.len(), remaining.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.pos += n;
            Ok(n)
        } else {
            self.inner.read(buf)
        }
    }
}

/// Read an HTTP response body from a stream, handling Content-Length, Transfer-Encoding, and EOF.
pub fn read_http_body<R: Read>(
    stream: &mut R,
    rest: &[u8],
    headers: &[(String, String)],
) -> Result<Vec<u8>, String> {
    if let Some(len_str) = header_lookup(headers, "Content-Length") {
        // parse len
        let len: usize = len_str
            .parse()
            .map_err(|_| "invalid Content-Length".to_string())?;
        let mut body = Vec::with_capacity(len);
        // copy any already-read bytes from `rest`
        if !rest.is_empty() {
            body.extend_from_slice(rest);
        }
        if body.len() < len {
            let remaining = len - body.len();
            let mut chunk = read_exact_len(stream, remaining)?;
            body.append(&mut chunk);
        }
        Ok(body)
    } else if matches!(header_lookup(headers, "Transfer-Encoding"), Some(v) if v.to_ascii_lowercase().contains("chunked"))
    {
        let mut reader = RestThen::new(rest, stream);
        read_chunked(&mut reader)
    } else {
        // no length; read to EOF
        let mut body = rest.to_vec();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = stream
                .read(&mut chunk)
                .map_err(|e| format!("read error: {}", e))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        Ok(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_line() {
        assert_eq!(parse_status_line("HTTP/1.1 200 OK"), Ok(200));
        assert_eq!(parse_status_line("HTTP/1.0 404 Not Found"), Ok(404));
        assert!(parse_status_line("INVALID").is_err());
        assert!(parse_status_line("HTTP/1.1 invalid").is_err());
    }

    #[test]
    fn test_parse_headers() {
        let input = "Content-Type: application/json\r\nContent-Length: 42\r\n";
        let headers = parse_headers(input);
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(headers[1], ("Content-Length".to_string(), "42".to_string()));
    }

    #[test]
    fn test_header_lookup() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), "42".to_string()),
        ];

        assert_eq!(
            header_lookup(&headers, "Content-Type"),
            Some("application/json")
        );
        assert_eq!(
            header_lookup(&headers, "content-type"),
            Some("application/json")
        ); // case insensitive
        assert_eq!(header_lookup(&headers, "Non-Existent"), None);
    }

    #[test]
    fn test_memchr_crlf_crlf() {
        assert_eq!(memchr_crlf_crlf(b""), None);
        assert_eq!(memchr_crlf_crlf(b"hello"), None);
        assert_eq!(memchr_crlf_crlf(b"hello\r\n\r\n"), Some(9));
        assert_eq!(memchr_crlf_crlf(b"hello\r\n\r\nworld"), Some(9));
    }

    #[test]
    fn test_parse_header_lines() {
        let input = "Content-Type: application/json\nContent-Length: 42\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(headers[1], ("Content-Length".to_string(), "42".to_string()));
    }

    #[test]
    fn test_serialize_headers() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Length".to_string(), "42".to_string()),
        ];
        let serialized = serialize_headers(&headers);
        assert_eq!(
            serialized,
            "Content-Type: application/json\r\nContent-Length: 42\r\n"
        );
    }

    #[test]
    fn test_add_default_headers() {
        let mut headers = vec![("Custom".to_string(), "value".to_string())];
        let body = b"Hello World";

        add_default_headers(&mut headers, body, "test-host");

        // Should have added the default headers
        assert_eq!(headers.len(), 4);
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Content-Length" && v == "11")
        );
        assert!(headers.iter().any(|(k, v)| k == "Host" && v == "test-host"));
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Connection" && v == "close")
        );
        assert!(headers.iter().any(|(k, v)| k == "Custom" && v == "value"));
    }

    #[test]
    fn test_add_default_headers_preserves_existing() {
        let mut headers = vec![
            ("Content-Length".to_string(), "999".to_string()),
            ("Host".to_string(), "existing-host".to_string()),
            ("Connection".to_string(), "keep-alive".to_string()),
        ];
        let body = b"Hello World";

        add_default_headers(&mut headers, body, "test-host");

        // Should not have added defaults since they already exist
        assert_eq!(headers.len(), 3);
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Content-Length" && v == "999")
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Host" && v == "existing-host")
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Connection" && v == "keep-alive")
        );
    }

    #[test]
    fn test_read_http_body_content_length() {
        use std::io::Cursor;
        let headers = vec![("Content-Length".to_string(), "5".to_string())];
        let mut stream = Cursor::new(b"hello");
        let rest = b"";

        let body = read_http_body(&mut stream, rest, &headers).unwrap();
        assert_eq!(body, b"hello");
    }

    #[test]
    fn test_read_http_body_with_rest() {
        use std::io::Cursor;
        let headers = vec![("Content-Length".to_string(), "10".to_string())];
        let mut stream = Cursor::new(b"world");
        let rest = b"hello";

        let body = read_http_body(&mut stream, rest, &headers).unwrap();
        assert_eq!(body, b"helloworld");
    }

    #[test]
    fn test_read_http_body_chunked() {
        use std::io::Cursor;
        let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        // Simple chunked response: "5\r\nhello\r\n0\r\n\r\n"
        let mut stream = Cursor::new(b"5\r\nhello\r\n0\r\n\r\n");
        let rest = b"";

        let body = read_http_body(&mut stream, rest, &headers).unwrap();
        assert_eq!(body, b"hello");
    }

    #[test]
    fn test_read_http_body_eof() {
        use std::io::Cursor;
        let headers = vec![]; // No Content-Length or Transfer-Encoding
        let mut stream = Cursor::new(b"hello world");
        let rest = b"";

        let body = read_http_body(&mut stream, rest, &headers).unwrap();
        assert_eq!(body, b"hello world");
    }

    #[test]
    fn test_rest_then_reader() {
        use std::io::Cursor;
        let rest = b"hello";
        let inner = Cursor::new(b" world");
        let mut reader = RestThen::new(rest, inner);

        let mut buf = [0u8; 20];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..n], b"hello");

        // Read from inner stream now
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 6);
        assert_eq!(&buf[..n], b" world");
    }

    #[test]
    fn test_build_request() {
        let headers = vec![("Custom".to_string(), "value".to_string())];
        let body = b"Hello World";

        let request = build_request("POST", "/test", headers, body);

        // Check that it starts with the request line
        assert!(request.starts_with(b"POST /test HTTP/1.1\r\n"));

        // Check that it ends with the body
        assert!(request.ends_with(b"\r\nHello World"));

        // Check that it contains the headers
        let request_str = String::from_utf8(request).unwrap();
        assert!(request_str.contains("Custom: value"));
        assert!(request_str.contains("Content-Length: 11"));
        assert!(request_str.contains("Host: localhost"));
        assert!(request_str.contains("Connection: close"));
    }

    #[test]
    fn test_parse_header_lines_empty() {
        let input = "";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 0);
    }

    #[test]
    fn test_parse_header_lines_malformed() {
        let input = "Content-Type application/json\nContent-Length: 42\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 1); // Only the valid header
        assert_eq!(headers[0], ("Content-Length".to_string(), "42".to_string()));
    }

    #[test]
    fn test_parse_header_lines_no_colon() {
        let input = "Content-Type application/json\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 0); // No valid headers
    }

    #[test]
    fn test_parse_header_lines_multiple_colons() {
        let input = "Content-Type: application/json: extra\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 1);
        assert_eq!(
            headers[0],
            (
                "Content-Type".to_string(),
                "application/json: extra".to_string()
            )
        );
    }

    #[test]
    fn test_parse_header_lines_lf_only() {
        let input = "Content-Type: application/json\nContent-Length: 42\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(headers[1], ("Content-Length".to_string(), "42".to_string()));
    }

    #[test]
    fn test_parse_header_lines_with_spaces() {
        let input = "Content-Type:   application/json   \nContent-Length: 42\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 2);
        assert_eq!(
            headers[0],
            ("Content-Type".to_string(), "application/json".to_string())
        );
        assert_eq!(headers[1], ("Content-Length".to_string(), "42".to_string()));
    }

    #[test]
    fn test_parse_header_lines_case_insensitive_lookup() {
        let input = "CONTENT-TYPE: application/json\ncontent-length: 42\n\n";
        let headers = parse_header_lines(input);
        assert_eq!(headers.len(), 2);

        // Test case insensitive lookup
        assert_eq!(
            header_lookup(&headers, "Content-Type"),
            Some("application/json")
        );
        assert_eq!(
            header_lookup(&headers, "CONTENT-TYPE"),
            Some("application/json")
        );
        assert_eq!(
            header_lookup(&headers, "content-type"),
            Some("application/json")
        );
        assert_eq!(header_lookup(&headers, "Content-Length"), Some("42"));
        assert_eq!(header_lookup(&headers, "content-length"), Some("42"));
    }

    #[test]
    fn test_read_exact_len_too_short() {
        use std::io::Cursor;
        let mut reader = Cursor::new(b"short");
        let result = read_exact_len(&mut reader, 10);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unexpected EOF"));
    }

    #[test]
    fn test_read_chunked_invalid_size() {
        use std::io::Cursor;
        let data = b"invalid\r\nchunk\r\n0\r\n\r\n";
        let mut reader = Cursor::new(data);
        let result = read_chunked(&mut reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_read_chunked_zero_size() {
        use std::io::Cursor;
        let data = b"0\r\n\r\n";
        let mut reader = Cursor::new(data);
        let result = read_chunked(&mut reader);
        assert_eq!(result, Ok(vec![]));
    }

    #[test]
    fn test_read_http_body_content_length_invalid() {
        let headers = vec![("Content-Length".to_string(), "invalid".to_string())];
        let rest = b"";
        let mut reader = std::io::empty();
        let result = read_http_body(&mut reader, rest, &headers);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid Content-Length"));
    }

    #[test]
    fn test_read_http_body_chunked_with_rest() {
        let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
        let rest = b"5\r\nhello\r\n0\r\n\r\n";
        let mut reader = std::io::Cursor::new(rest);
        let result = read_http_body(&mut reader, &[], &headers);
        assert_eq!(result, Ok(b"hello".to_vec()));
    }

    #[test]
    fn test_read_http_body_chunked_mixed_case() {
        let headers = vec![("Transfer-Encoding".to_string(), "CHUNKED".to_string())];
        let rest = b"5\r\nhello\r\n0\r\n\r\n";
        let mut reader = std::io::Cursor::new(rest);
        let result = read_http_body(&mut reader, &[], &headers);
        assert_eq!(result, Ok(b"hello".to_vec()));
    }

    #[test]
    fn test_add_default_headers_empty_body_preserves_length() {
        let mut headers = vec![];
        let body = b"";

        add_default_headers(&mut headers, body, "test-host");

        // Should add default headers with Content-Length: 0 for empty body
        assert_eq!(headers.len(), 3);
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Content-Length" && v == "0")
        );
        assert!(headers.iter().any(|(k, v)| k == "Host" && v == "test-host"));
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Connection" && v == "close")
        );
    }

    #[test]
    fn test_add_default_headers_empty_body() {
        let mut headers = vec![];
        let body = b"";

        add_default_headers(&mut headers, body, "test-host");

        // Should add default headers
        assert_eq!(headers.len(), 3);
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Content-Length" && v == "0")
        );
        assert!(headers.iter().any(|(k, v)| k == "Host" && v == "test-host"));
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "Connection" && v == "close")
        );
    }
}
