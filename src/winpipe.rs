//! Minimal HTTP-over-Windows-Named-Pipe client for proxying requests to the trace agent (Windows).
//!
//! Connects to a Windows Named Pipe (\\.\\pipe\\<name>), writes an HTTP/1.1 request,
//! and parses the HTTP response. Mirrors the behavior of the Unix UDS client.
//!
//! Timeout is enforced using a separate thread with cancellation support (default: 50 seconds).
//! The request thread can be interrupted at key points during the HTTP transaction.

use crate::logging::log_debug;
use std::io::{self, Read};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, WAIT_ABANDONED_0, WAIT_FAILED,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    PIPE_READMODE_BYTE, SetNamedPipeHandleState, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

const ERROR_BROKEN_PIPE: u32 = windows_sys::Win32::Foundation::ERROR_BROKEN_PIPE;
const ERROR_FILE_NOT_FOUND: u32 = windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
const ERROR_IO_PENDING: u32 = windows_sys::Win32::Foundation::ERROR_IO_PENDING;
const ERROR_PIPE_BUSY: u32 = windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;
const ERROR_SEM_TIMEOUT: u32 = windows_sys::Win32::Foundation::ERROR_SEM_TIMEOUT;

pub use crate::http::Response;

/// Work item for the worker pool
struct WorkItem {
    request_id: u64,
    pipe_name: String,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    timeout: Duration,
    result_tx: mpsc::Sender<Result<Response, String>>,
}

/// Worker pool for handling named pipe requests
struct WorkerPool {
    senders: Vec<mpsc::Sender<WorkItem>>,
    next_idx: usize,
    _workers: Vec<thread::JoinHandle<()>>,
}

impl WorkerPool {
    fn new(num_workers: usize) -> Self {
        let mut senders = Vec::with_capacity(num_workers);
        let mut workers = Vec::with_capacity(num_workers);
        for _ in 0..num_workers {
            let (tx, rx) = mpsc::channel::<WorkItem>();
            let worker = thread::spawn(move || {
                Self::worker_loop(rx);
            });
            senders.push(tx);
            workers.push(worker);
        }

        WorkerPool {
            senders,
            next_idx: 0,
            _workers: workers,
        }
    }

    fn worker_loop(work_rx: mpsc::Receiver<WorkItem>) {
        while let Ok(work_item) = work_rx.recv() {
            let result = perform_request(
                work_item.request_id,
                &work_item.pipe_name,
                &work_item.method,
                &work_item.path,
                work_item.headers,
                &work_item.body,
                work_item.timeout,
            );

            // Send result back, ignore send errors (receiver may have timed out)
            let _ = work_item.result_tx.send(result);
        }
    }

    fn submit_work(&mut self, item: WorkItem) -> Result<(), String> {
        if self.senders.is_empty() {
            return Err("worker pool shut down".to_string());
        }
        let idx = self.next_idx % self.senders.len();
        self.next_idx = self.next_idx.wrapping_add(1);
        self.senders[idx]
            .send(item)
            .map_err(|_| "worker channel closed".to_string())
    }

    fn shutdown(&mut self) {
        // Drop the senders to close the channels and signal workers to shut down
        self.senders.clear();
        self.next_idx = 0;

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
        timeout,
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
    timeout: Duration,
) -> Result<Response, String> {
    log_debug(&format!(
        "PIPE [req:{}]: opening pipe: {}",
        request_id, full_path
    ));

    let deadline = Instant::now() + timeout;
    let wide_path = to_wide(full_path);

    let handle = map_io_err(
        request_id,
        &format!("failed to open pipe {}", full_path),
        open_pipe_handle(&wide_path, deadline),
    )?;

    let connection = PipeConnection::new(handle, deadline);

    let req = build_request(method, path, headers, body);
    log_debug(&format!(
        "PIPE [req:{}]: sending {} {} request ({} bytes total, body: {} bytes)",
        request_id,
        method,
        path,
        req.len(),
        body.len()
    ));

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

    map_io_err(
        request_id,
        "request write failed",
        connection.write_all(&req),
    )?;
    log_debug(&format!(
        "PIPE [req:{}]: request write completed successfully",
        request_id
    ));

    log_debug(&format!(
        "PIPE [req:{}]: starting to read response headers",
        request_id
    ));

    let mut stream = connection.into_stream();
    let mut buf = Vec::with_capacity(8192);
    let header_end = crate::http::read_until_double_crlf(&mut stream, &mut buf).map_err(|e| {
        log_debug(&format!(
            "PIPE [req:{}]: failed to read headers after successful write: {}",
            request_id, e
        ));
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

    log_debug(&format!(
        "PIPE [req:{}]: starting to read response body",
        request_id
    ));
    let body_vec = crate::http::read_http_body(&mut stream, rest, &headers_vec).map_err(|e| {
        log_debug(&format!(
            "PIPE [req:{}]: failed to read response body: {}",
            request_id, e
        ));
        e
    })?;
    log_debug(&format!(
        "PIPE [req:{}]: read response body of {} bytes",
        request_id,
        body_vec.len()
    ));

    if let Ok(body_str) = std::str::from_utf8(&body_vec) {
        log_debug(&format!(
            "PIPE [req:{}]: response body content:\n{}",
            request_id, body_str
        ));
    } else {
        log_debug(&format!(
            "PIPE [req:{}]: response body contains binary data ({} bytes)",
            request_id,
            body_vec.len()
        ));
    }

    log_debug(&format!(
        "PIPE [req:{}]: response complete, body size: {} bytes",
        request_id,
        body_vec.len()
    ));

    Ok(Response {
        status,
        headers: headers_vec,
        body: body_vec,
    })
}

fn map_io_err<T>(request_id: u64, context: &str, result: io::Result<T>) -> Result<T, String> {
    result.map_err(|err| {
        let msg = format!("{}: {}", context, err);
        log_debug(&format!("PIPE [req:{}]: {}", request_id, msg));
        msg
    })
}

fn to_wide(input: &str) -> Vec<u16> {
    std::ffi::OsStr::new(input)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn open_pipe_handle(path: &[u16], deadline: Instant) -> io::Result<OwnedHandle> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "timed out waiting for pipe"))?;

        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                ptr::null_mut::<SECURITY_ATTRIBUTES>(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                0 as HANDLE,
            )
        };

        if handle != INVALID_HANDLE_VALUE {
            unsafe {
                let mut mode = PIPE_READMODE_BYTE;
                let mode_ptr = &mut mode as *mut _;
                if SetNamedPipeHandleState(handle, mode_ptr, ptr::null_mut(), ptr::null_mut()) == 0
                {
                    let err_code = GetLastError();
                    let err = io::Error::from_raw_os_error(err_code as i32);
                    CloseHandle(handle);
                    return Err(err);
                }
            }

            let owned = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
            return Ok(owned);
        }

        let err = unsafe { GetLastError() };
        if err == ERROR_PIPE_BUSY {
            let wait_ms = duration_to_millis(remaining);
            let waited = unsafe { WaitNamedPipeW(path.as_ptr(), wait_ms) };
            if waited == 0 {
                let wait_err = unsafe { GetLastError() };
                if wait_err == ERROR_SEM_TIMEOUT {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "timed out waiting for pipe",
                    ));
                } else {
                    return Err(io::Error::from_raw_os_error(wait_err as i32));
                }
            }
            continue;
        } else if err == ERROR_FILE_NOT_FOUND {
            if remaining <= Duration::from_millis(10) {
                return Err(io::Error::new(io::ErrorKind::NotFound, "pipe not found"));
            }
            thread::sleep(Duration::from_millis(10));
            continue;
        } else {
            return Err(io::Error::from_raw_os_error(err as i32));
        }
    }
}

fn duration_to_millis(duration: Duration) -> u32 {
    let millis = duration.as_millis();
    if millis == 0 {
        1
    } else if millis >= u32::MAX as u128 {
        u32::MAX - 1
    } else {
        millis as u32
    }
}

struct PipeConnection {
    handle: OwnedHandle,
    deadline: Instant,
}

impl PipeConnection {
    fn new(handle: OwnedHandle, deadline: Instant) -> Self {
        Self { handle, deadline }
    }

    fn write_all(&self, buf: &[u8]) -> io::Result<()> {
        if buf.is_empty() {
            return Ok(());
        }

        let mut written = 0;
        while written < buf.len() {
            let remaining = self.remaining()?;
            let bytes = write_some(self.raw_handle(), &buf[written..], remaining)?;
            if bytes == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "failed to write to named pipe",
                ));
            }
            written += bytes;
        }
        Ok(())
    }

    fn into_stream(self) -> PipeStream {
        PipeStream {
            handle: self.handle,
            deadline: self.deadline,
        }
    }

    fn raw_handle(&self) -> HANDLE {
        handle_raw(&self.handle)
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request timed out"))
    }
}

struct PipeStream {
    handle: OwnedHandle,
    deadline: Instant,
}

impl PipeStream {
    fn raw_handle(&self) -> HANDLE {
        handle_raw(&self.handle)
    }

    fn remaining(&self) -> io::Result<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "request timed out"))
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let remaining = self.remaining()?;
        read_some(self.raw_handle(), buf, remaining)
    }
}

fn handle_raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

fn write_some(handle: HANDLE, data: &[u8], timeout: Duration) -> io::Result<usize> {
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let event = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null()) };
    if event.is_null() {
        let err = unsafe { GetLastError() };
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    overlapped.hEvent = event;

    let mut bytes_written = 0u32;
    let bytes_to_write = data.len().min(u32::MAX as usize) as u32;
    let result = unsafe {
        WriteFile(
            handle,
            data.as_ptr() as *const _,
            bytes_to_write,
            &mut bytes_written,
            &mut overlapped,
        )
    };

    if result != 0 {
        unsafe { CloseHandle(event) };
        return Ok(bytes_written as usize);
    }

    let err = unsafe { GetLastError() };
    if err != ERROR_IO_PENDING {
        unsafe { CloseHandle(event) };
        return Err(io::Error::from_raw_os_error(err as i32));
    }

    let transferred = wait_overlapped(handle, &mut overlapped, event, timeout, false)?;
    unsafe { CloseHandle(event) };
    Ok(transferred as usize)
}

fn read_some(handle: HANDLE, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let event = unsafe { CreateEventW(ptr::null_mut(), 0, 0, ptr::null()) };
    if event.is_null() {
        let err = unsafe { GetLastError() };
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    overlapped.hEvent = event;

    let mut bytes_read = 0u32;
    let bytes_to_read = buf.len().min(u32::MAX as usize) as u32;
    let result = unsafe {
        ReadFile(
            handle,
            buf.as_mut_ptr() as *mut _,
            bytes_to_read,
            &mut bytes_read,
            &mut overlapped,
        )
    };

    if result != 0 {
        unsafe { CloseHandle(event) };
        return Ok(bytes_read as usize);
    }

    let err = unsafe { GetLastError() };
    if err == ERROR_BROKEN_PIPE {
        unsafe { CloseHandle(event) };
        return Ok(0);
    }
    if err != ERROR_IO_PENDING {
        unsafe { CloseHandle(event) };
        return Err(io::Error::from_raw_os_error(err as i32));
    }

    let transferred = wait_overlapped(handle, &mut overlapped, event, timeout, true)?;
    unsafe { CloseHandle(event) };
    Ok(transferred as usize)
}

fn wait_overlapped(
    handle: HANDLE,
    overlapped: &mut OVERLAPPED,
    event: HANDLE,
    timeout: Duration,
    eof_ok: bool,
) -> io::Result<u32> {
    let wait = unsafe { WaitForSingleObject(event, duration_to_millis(timeout)) };
    match wait {
        WAIT_OBJECT_0 => {
            let mut transferred = 0u32;
            let ok = unsafe { GetOverlappedResult(handle, overlapped, &mut transferred, 0) };
            if ok != 0 {
                return Ok(transferred);
            }
            let err = unsafe { GetLastError() };
            if eof_ok && err == ERROR_BROKEN_PIPE {
                return Ok(0);
            }
            Err(io::Error::from_raw_os_error(err as i32))
        }
        WAIT_TIMEOUT => {
            unsafe {
                CancelIoEx(handle, overlapped as *mut OVERLAPPED);
            }
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "operation timed out",
            ))
        }
        WAIT_FAILED => {
            let err = unsafe { GetLastError() };
            Err(io::Error::from_raw_os_error(err as i32))
        }
        WAIT_ABANDONED_0 => Err(io::Error::other("wait abandoned")),
        other => Err(io::Error::other(format!("unexpected wait result: {other}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::thread;
    use std::time::Duration;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_FIRST_PIPE_INSTANCE;
    use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE};

    const PIPE_ACCESS_DUPLEX_CONST: u32 = 0x0000_0003;

    #[test]
    #[serial]
    fn request_times_out_when_server_hangs() {
        let pipe_name = format!("libagent_timeout_test_{}", std::process::id());
        let full_path = format!(r"\\.\pipe\{}", pipe_name);
        let wide = to_wide(&full_path);

        let handle: HANDLE = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX_CONST | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE,
                1,
                65536,
                65536,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            panic!("failed to create test named pipe");
        }

        let handle_val = handle as isize;
        let server = thread::spawn(move || {
            let handle = handle_val as HANDLE;
            let ok = unsafe { ConnectNamedPipe(handle, ptr::null_mut()) };
            if ok == 0 {
                unsafe { CloseHandle(handle) };
                return;
            }
            // Hold the pipe open without responding to trigger a timeout on the client.
            thread::sleep(Duration::from_millis(400));
            unsafe { CloseHandle(handle) };
        });

        let result = request_over_named_pipe(
            42,
            &pipe_name,
            "GET",
            "/hang",
            Vec::new(),
            &[],
            Duration::from_millis(150),
        );

        assert!(result.is_err(), "expected timeout error");
        let err = result.unwrap_err();
        assert!(
            err.contains("timed out"),
            "error message should mention timeout: {}",
            err
        );

        let _ = server.join();
        shutdown_worker_pool();
    }
}
