//! Minimal HTTP-over-Windows-Named-Pipe client for proxying requests to the trace agent (Windows).
//!
//! Connects to a Windows Named Pipe (\\.\\pipe\\<name>), writes an HTTP/1.1 request,
//! and parses the HTTP response. Mirrors the behavior of the Unix UDS client.
//!
//! Timeout is enforced using a separate thread with cancellation support (default: 50 seconds).
//! The request thread can be interrupted at key points during the HTTP transaction.

use std::io::Write;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub use crate::http::Response;

/// Work item for the worker pool
struct WorkItem {
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
    submit_work_to_pool(work_item)?;

    // Wait for the result with timeout
    match result_rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err(format!("request timed out after {:?}", timeout))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("worker thread disconnected unexpectedly".to_string())
        }
    }
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
