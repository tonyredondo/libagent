//! Minimal HTTP-over-Windows-Named-Pipe client for proxying requests to the trace agent (Windows).
//!
//! Connects to a Windows Named Pipe (\\.\\pipe\\<name>), writes an HTTP/1.1 request,
//! and parses the HTTP response. Mirrors the behavior of the Unix UDS client.
//!
//! Timeout is enforced using a separate thread with cancellation support (default: 50 seconds).
//! The request thread can be interrupted at key points during the HTTP transaction.

use crate::manager::log_debug;
use std::io::Write;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub use crate::http::Response;

/// Work item for the worker pool
struct WorkItem {
    request_id: u64,
    pipe_name: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    result_tx: mpsc::Sender<Result<Response, String>>,
}

/// Worker pool for handling named pipe requests
struct WorkerPool {
    work_tx: Option<mpsc::Sender<WorkItem>>,
    _workers: Vec<thread::JoinHandle<()>>,
}

impl WorkerPool {
    fn new(num_workers: usize) -> Self {
        let (work_tx, work_rx) = mpsc::channel::<WorkItem>();
        let work_rx = Arc::new(Mutex::new(work_rx));

        let mut workers = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            let work_rx = Arc::clone(&work_rx);
            let worker = thread::spawn(move || {
                Self::worker_loop(work_rx);
            });
            workers.push(worker);
        }

        WorkerPool {
            work_tx: Some(work_tx),
            _workers: workers,
        }
    }

    fn worker_loop(work_rx: Arc<Mutex<mpsc::Receiver<WorkItem>>>) {
        loop {
            let work_item = {
                let rx = work_rx.lock().unwrap();
                match rx.recv() {
                    Ok(item) => item,
                    Err(_) => break, // Channel closed, shutdown
                }
            };

            let result = perform_request(
                work_item.request_id,
                &work_item.pipe_name,
                &work_item.method,
                &work_item.path,
                work_item.headers,
                &work_item.body,
            );

            // Send result back, ignore send errors (receiver may have timed out)
            let _ = work_item.result_tx.send(result);
        }
    }

    fn submit_work(&self, item: WorkItem) -> Result<(), String> {
        if let Some(ref tx) = self.work_tx {
            tx.send(item).map_err(|_| "channel closed".to_string())
        } else {
            Err("worker pool shut down".to_string())
        }
    }

    fn shutdown(&mut self) {
        // Drop the sender to close the channel and signal workers to shut down
        self.work_tx = None;

        // Wait for all workers to finish
        for worker in self._workers.drain(..) {
            let _ = worker.join();
        }
    }
}

/// Global worker pool singleton
static WORKER_POOL: OnceLock<Mutex<Option<WorkerPool>>> = OnceLock::new();

fn get_worker_pool() -> Result<(), String> {
    let pool_mutex = WORKER_POOL.get_or_init(|| Mutex::new(None));

    let mut pool_guard = pool_mutex
        .lock()
        .map_err(|_| "failed to lock worker pool".to_string())?;
    if pool_guard.is_none() {
        *pool_guard = Some(WorkerPool::new(4));
    }
    Ok(())
}

fn submit_work_to_pool(item: WorkItem) -> Result<(), String> {
    let pool_mutex = WORKER_POOL.get_or_init(|| Mutex::new(None));
    let mut pool_guard = pool_mutex
        .lock()
        .map_err(|_| "failed to lock worker pool".to_string())?;
    if let Some(ref mut pool) = *pool_guard {
        pool.submit_work(item)
    } else {
        Err("worker pool not initialized".to_string())
    }
}

pub fn shutdown_worker_pool() {
    if let Some(pool_mutex) = WORKER_POOL.get()
        && let Ok(mut pool_guard) = pool_mutex.lock()
    {
        if let Some(ref mut pool) = *pool_guard {
            pool.shutdown();
        }
        *pool_guard = None;
    }
}

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
    request_id: u64,
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

    // Create result channel for this request
    let (result_tx, result_rx) = mpsc::channel();

    // Create work item
    let work_item = WorkItem {
        request_id,
        pipe_name: full_path,
        method: method.to_string(),
        path: path.to_string(),
        headers,
        body: body.to_vec(),
        result_tx,
    };

    // Ensure worker pool is initialized
    get_worker_pool()?;

    // Submit work to the worker pool
    log_debug(&format!(
        "PIPE [req:{}]: submitting {} {} request to worker pool",
        request_id, method, path
    ));
    submit_work_to_pool(work_item)?;

    // Wait for the result with timeout
    match result_rx.recv_timeout(timeout) {
        Ok(result) => {
            log_debug(&format!(
                "PIPE [req:{}]: received result from worker pool",
                request_id
            ));
            result
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let err = format!("request timed out after {:?}", timeout);
            log_debug(&format!("PIPE [req:{}]: {}", request_id, err));
            Err(err)
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            let err = "worker thread disconnected unexpectedly".to_string();
            log_debug(&format!("PIPE [req:{}]: {}", request_id, err));
            Err(err)
        }
    }
}

#[allow(dead_code)]
fn perform_request(
    request_id: u64,
    full_path: &str,
    method: &str,
    path: &str,
    headers: Vec<(String, String)>,
    body: &[u8],
) -> Result<Response, String> {
    log_debug(&format!(
        "PIPE [req:{}]: opening pipe: {}",
        request_id, full_path
    ));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(full_path)
        .map_err(|e| {
            let err = format!("failed to open pipe {}: {}", full_path, e);
            log_debug(&format!("PIPE [req:{}]: open failed: {}", request_id, err));
            err
        })?;

    // Write request
    let req = build_request(method, path, headers, body);
    log_debug(&format!(
        "PIPE [req:{}]: sending {} {} request ({} bytes total, body: {} bytes)",
        request_id,
        method,
        path,
        req.len(),
        body.len()
    ));

    // Log request details for debugging (text only, avoid binary formats like MessagePack)
    if let Ok(req_str) = std::str::from_utf8(&req) {
        log_debug(&format!(
            "PIPE [req:{}]: request content:\n{}",
            request_id, req_str
        ));
    } else {
        log_debug(&format!(
            "PIPE [req:{}]: request contains binary data ({} bytes)",
            request_id,
            req.len()
        ));
    }

    // Write the request and track progress
    log_debug(&format!(
        "PIPE [req:{}]: starting request write",
        request_id
    ));
    let write_result = file.write_all(&req);
    match &write_result {
        Ok(_) => log_debug(&format!(
            "PIPE [req:{}]: request write completed successfully",
            request_id
        )),
        Err(e) => log_debug(&format!(
            "PIPE [req:{}]: request write failed: {}",
            request_id, e
        )),
    }
    write_result.map_err(|e| {
        let err = format!("write error: {}", e);
        log_debug(&format!(
            "PIPE [req:{}]: write operation failed: {}",
            request_id, err
        ));
        err
    })?;

    // Read response headers
    log_debug(&format!(
        "PIPE [req:{}]: starting to read response headers",
        request_id
    ));

    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut file, &mut buf).map_err(|e| {
        log_debug(&format!(
            "PIPE [req:{}]: failed to read headers after successful write: {}",
            request_id, e
        ));

        // Check if this indicates the server closed the connection
        if e.contains("The pipe has been ended") || e.contains("Broken pipe") || e.contains("broken pipe") {
            log_debug(&format!(
                "PIPE [req:{}]: server closed pipe immediately after receiving request (no response sent)",
                request_id
            ));
            log_debug(&format!(
                "PIPE [req:{}]: this typically means the endpoint '{}' is not supported by the trace agent",
                request_id, path
            ));
        }

        e
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: read {} bytes of headers, header_end at {}",
        request_id,
        buf.len(),
        header_end
    ));

    let (head, rest) = buf.split_at(header_end);
    log_debug(&format!(
        "PIPE [req:{}]: header section is {} bytes, remaining data: {} bytes",
        request_id,
        head.len(),
        rest.len()
    ));

    let head_str = std::str::from_utf8(head).map_err(|_| {
        let err = "invalid utf-8 in headers".to_string();
        log_debug(&format!("PIPE [req:{}]: {}", request_id, err));
        err
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: header content:\n{}",
        request_id, head_str
    ));

    let mut lines = head_str.split("\r\n");
    let status_line = lines.next().ok_or_else(|| {
        let err = "empty response".to_string();
        log_debug(&format!("PIPE [req:{}]: {}", request_id, err));
        err
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: status line: '{}'",
        request_id, status_line
    ));

    let status = crate::http::parse_status_line(status_line).map_err(|e| {
        log_debug(&format!(
            "PIPE [req:{}]: failed to parse status line '{}': {}",
            request_id, status_line, e
        ));
        e
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: parsed status: {}",
        request_id, status
    ));

    let header_str = lines.collect::<Vec<_>>().join("\r\n");
    let headers_vec = crate::http::parse_headers(&header_str);
    log_debug(&format!(
        "PIPE [req:{}]: parsed {} headers",
        request_id,
        headers_vec.len()
    ));

    // Read the response body
    log_debug(&format!(
        "PIPE [req:{}]: starting to read response body",
        request_id
    ));
    let body = crate::http::read_http_body(&mut file, rest, &headers_vec).map_err(|e| {
        log_debug(&format!(
            "PIPE [req:{}]: failed to read response body: {}",
            request_id, e
        ));
        e
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: read response body of {} bytes",
        request_id,
        body.len()
    ));

    // Log response details for debugging (text only, avoid binary formats like MessagePack)
    if let Ok(body_str) = std::str::from_utf8(&body) {
        log_debug(&format!(
            "PIPE [req:{}]: response body content:\n{}",
            request_id, body_str
        ));
    } else {
        log_debug(&format!(
            "PIPE [req:{}]: response body contains binary data ({} bytes)",
            request_id,
            body.len()
        ));
    }

    log_debug(&format!(
        "PIPE [req:{}]: response complete, body size: {} bytes",
        request_id,
        body.len()
    ));

    Ok(Response {
        status,
        headers: headers_vec,
        body,
    })
}
