#![feature(test)]

extern crate test;
use test::Bencher;

/// Benchmark basic string operations that would be used in HTTP parsing
#[bench]
fn bench_string_operations_status_line_split(b: &mut Bencher) {
    let status_line = "HTTP/1.1 200 OK";
    b.iter(|| {
        test::black_box(status_line.split_whitespace().collect::<Vec<&str>>());
    });
}

/// Benchmark header parsing simulation
#[bench]
fn bench_string_operations_header_parsing(b: &mut Bencher) {
    let headers_str = "Content-Type: application/json\r\nContent-Length: 42\r\nUser-Agent: libagent/1.0\r\nAccept: */*\r\n";
    b.iter(|| {
        let mut result = Vec::new();
        for line in headers_str.split("\r\n") {
            if line.is_empty() {
                break;
            }
            if let Some(idx) = line.find(':') {
                let (name, value) = line.split_at(idx);
                let value = value[1..].trim_start();
                result.push((name.to_string(), value.to_string()));
            }
        }
        test::black_box(result);
    });
}

/// Benchmark byte searching (similar to HTTP parsing)
#[bench]
fn bench_memory_operations_find_crlf_crlf(b: &mut Bencher) {
    let buffer = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 42\r\n\r\n{\"status\": \"ok\"}";
    b.iter(|| {
        test::black_box(if buffer.len() < 4 {
            None
        } else {
            for i in 0..=buffer.len() - 4 {
                if &buffer[i..i + 4] == b"\r\n\r\n" {
                    return Some(i + 4);
                }
            }
            None
        })
    });
}

/// Benchmark library initialization and basic operations
#[bench]
fn bench_library_operations_get_metrics(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_metrics());
    });
}

/// Benchmark metrics formatting
#[bench]
fn bench_library_operations_format_metrics(b: &mut Bencher) {
    b.iter(|| {
        let metrics = libagent::get_metrics();
        test::black_box(metrics.format_metrics());
    });
}

/// Benchmark metrics operations
#[bench]
fn bench_metrics_read_agent_spawns(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        test::black_box(metrics.agent_spawns());
    });
}

/// Benchmark reading all metrics
#[bench]
fn bench_metrics_read_all_metrics(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        test::black_box(metrics.agent_spawns());
        test::black_box(metrics.trace_agent_spawns());
        test::black_box(metrics.agent_failures());
        test::black_box(metrics.trace_agent_failures());
        test::black_box(metrics.uptime());
    });
}

/// Benchmark metrics formatting
#[bench]
fn bench_metrics_format_metrics(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        test::black_box(metrics.format_metrics());
    });
}

/// Benchmark HTTP status line parsing
#[bench]
fn bench_http_parse_status_line(b: &mut Bencher) {
    let status_line = "HTTP/1.1 200 OK";
    b.iter(|| {
        let _ = test::black_box(libagent::parse_status_line(status_line));
    });
}

/// Benchmark HTTP status line parsing with different codes
#[bench]
fn bench_http_parse_status_line_various(b: &mut Bencher) {
    let status_lines = [
        "HTTP/1.1 200 OK",
        "HTTP/1.1 404 Not Found",
        "HTTP/1.1 500 Internal Server Error",
        "HTTP/1.0 302 Found",
        "HTTP/2.0 201 Created",
    ];
    let mut i = 0;
    b.iter(|| {
        let line = status_lines[i % status_lines.len()];
        i += 1;
        let _ = test::black_box(libagent::parse_status_line(line));
    });
}

/// Benchmark HTTP header parsing
#[bench]
fn bench_http_parse_headers(b: &mut Bencher) {
    let headers_str = "Content-Type: application/json\r\nContent-Length: 42\r\nUser-Agent: libagent/1.0\r\nAccept: */*\r\nX-Custom-Header: value\r\n";
    b.iter(|| {
        test::black_box(libagent::parse_headers(headers_str));
    });
}

/// Benchmark HTTP header parsing with many headers
#[bench]
fn bench_http_parse_headers_many(b: &mut Bencher) {
    let headers_str = (0..50)
        .map(|i| format!("X-Header-{}: value-{}\r\n", i, i))
        .collect::<String>();
    b.iter(|| {
        test::black_box(libagent::parse_headers(&headers_str));
    });
}

/// Benchmark header lookup in parsed headers
#[bench]
fn bench_http_header_lookup(b: &mut Bencher) {
    let headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Content-Length".to_string(), "42".to_string()),
        ("User-Agent".to_string(), "libagent/1.0".to_string()),
        ("Accept".to_string(), "*/*".to_string()),
        ("X-Custom".to_string(), "custom-value".to_string()),
    ];
    b.iter(|| {
        test::black_box(libagent::header_lookup(&headers, "Content-Type"));
        test::black_box(libagent::header_lookup(&headers, "User-Agent"));
        test::black_box(libagent::header_lookup(&headers, "X-Custom"));
        test::black_box(libagent::header_lookup(&headers, "Non-Existent"));
    });
}

/// Benchmark header lookup in large header list
#[bench]
fn bench_http_header_lookup_large(b: &mut Bencher) {
    let headers: Vec<(String, String)> = (0..100)
        .map(|i| (format!("X-Header-{}", i), format!("value-{}", i)))
        .collect();
    b.iter(|| {
        test::black_box(libagent::header_lookup(&headers, "X-Header-50"));
        test::black_box(libagent::header_lookup(&headers, "X-Header-99"));
        test::black_box(libagent::header_lookup(&headers, "Non-Existent"));
    });
}

/// Benchmark memory CRLF CRLF search
#[bench]
fn bench_http_memchr_crlf_crlf(b: &mut Bencher) {
    let buffer = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 42\r\n\r\n{\"status\": \"ok\"}";
    b.iter(|| {
        test::black_box(libagent::memchr_crlf_crlf(buffer));
    });
}

/// Benchmark memory CRLF CRLF search in larger buffer
#[bench]
fn bench_http_memchr_crlf_crlf_large(b: &mut Bencher) {
    let mut buffer = Vec::new();
    for _ in 0..100 {
        buffer.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
    }
    buffer.extend_from_slice(b"\r\n{\"status\": \"ok\"}");
    b.iter(|| {
        test::black_box(libagent::memchr_crlf_crlf(&buffer));
    });
}

/// Benchmark header serialization
#[bench]
fn bench_http_serialize_headers(b: &mut Bencher) {
    let headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Content-Length".to_string(), "42".to_string()),
        ("User-Agent".to_string(), "libagent/1.0".to_string()),
    ];
    b.iter(|| {
        test::black_box(libagent::serialize_headers(&headers));
    });
}

/// Benchmark header serialization with many headers
#[bench]
fn bench_http_serialize_headers_many(b: &mut Bencher) {
    let headers: Vec<(String, String)> = (0..50)
        .map(|i| (format!("X-Header-{}", i), format!("value-{}", i)))
        .collect();
    b.iter(|| {
        test::black_box(libagent::serialize_headers(&headers));
    });
}

/// Benchmark configuration getters
#[bench]
fn bench_config_getters(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_agent_program());
        test::black_box(libagent::get_agent_args());
        test::black_box(libagent::get_trace_agent_program());
        test::black_box(libagent::get_monitor_interval_secs());
        test::black_box(libagent::get_backoff_initial_secs());
    });
}

/// Benchmark individual config getters
#[bench]
fn bench_config_get_agent_program(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_agent_program());
    });
}

#[bench]
fn bench_config_get_agent_args(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_agent_args());
    });
}

#[bench]
fn bench_config_get_trace_agent_program(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_trace_agent_program());
    });
}

#[bench]
fn bench_config_get_monitor_interval(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_monitor_interval_secs());
    });
}

#[bench]
fn bench_config_get_backoff_values(b: &mut Bencher) {
    b.iter(|| {
        test::black_box(libagent::get_backoff_initial_secs());
        test::black_box(libagent::get_backoff_max_secs());
    });
}

/// Benchmark metrics counter access
#[bench]
fn bench_metrics_counters(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        test::black_box(metrics.agent_spawns());
        test::black_box(metrics.trace_agent_spawns());
        test::black_box(metrics.agent_failures());
        test::black_box(metrics.trace_agent_failures());
    });
}

/// Benchmark metrics recording operations
#[bench]
fn bench_metrics_recording(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        // Note: These don't actually modify global state in benchmarks
        // but we can still measure the function call overhead
        metrics.record_agent_spawn();
        metrics.record_trace_agent_spawn();
        metrics.record_agent_failure();
        metrics.record_trace_agent_failure();
        test::black_box(());
    });
}

/// Benchmark metrics uptime calculation
#[bench]
fn bench_metrics_uptime(b: &mut Bencher) {
    let metrics = libagent::get_metrics();
    b.iter(|| {
        test::black_box(metrics.uptime());
    });
}

/// Benchmark build_request function
#[bench]
fn bench_http_build_request(b: &mut Bencher) {
    let headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("User-Agent".to_string(), "libagent/1.0".to_string()),
    ];
    let body = b"{\"test\": \"data\"}";
    b.iter(|| {
        let headers_copy = headers.clone();
        test::black_box(libagent::build_request(
            "POST",
            "/v1/test",
            headers_copy,
            body,
        ));
    });
}

/// Benchmark build_request with many headers
#[bench]
fn bench_http_build_request_many_headers(b: &mut Bencher) {
    let headers: Vec<(String, String)> = (0..20)
        .map(|i| (format!("X-Header-{}", i), format!("value-{}", i)))
        .collect();
    let body = b"{\"test\": \"data\"}";
    b.iter(|| {
        let headers_copy = headers.clone();
        test::black_box(libagent::build_request(
            "POST",
            "/v1/test",
            headers_copy,
            body,
        ));
    });
}

/// Benchmark add_default_headers function
#[bench]
fn bench_http_add_default_headers(b: &mut Bencher) {
    let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    let body = b"{\"test\": \"data\"}";
    b.iter(|| {
        let mut test_headers = headers.clone();
        libagent::add_default_headers(&mut test_headers, body, "localhost:8126");
        test::black_box(test_headers);
    });
}
