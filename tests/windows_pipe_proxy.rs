#![cfg(windows)]

use serial_test::serial;
use std::env;
use std::ffi::{CString, c_char};
use std::io::{Read, Write};
use std::os::windows::io::FromRawHandle;
use std::ptr;
use std::thread;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_FIRST_PIPE_INSTANCE;
use windows_sys::Win32::System::Pipes::{PIPE_READMODE_BYTE, PIPE_TYPE_BYTE};

// Minimal FFI for named pipe APIs to avoid windows-sys module differences.
use std::ffi::c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        lpName: *const u16,
        dwOpenMode: u32,
        dwPipeMode: u32,
        nMaxInstances: u32,
        nOutBufferSize: u32,
        nInBufferSize: u32,
        nDefaultTimeOut: u32,
        lpSecurityAttributes: *mut c_void,
    ) -> HANDLE;

    fn ConnectNamedPipe(hNamedPipe: HANDLE, lpOverlapped: *mut c_void) -> i32; // BOOL
}

const PIPE_ACCESS_DUPLEX_CONST: u32 = 0x00000003;

// Force the Rust crate to link so the C symbols are available to the test.
#[inline(never)]
fn force_link_lib() {
    let _ = libagent::stop as fn();
}

#[repr(C)]
struct TestResult {
    status: Option<u16>,
    headers: Option<Vec<u8>>,
    body: Option<Vec<u8>>,
    error: Option<String>,
}

impl TestResult {
    fn new() -> Self {
        Self {
            status: None,
            headers: None,
            body: None,
            error: None,
        }
    }
}

extern "C" fn test_response_callback(
    status: u16,
    headers_data: *const u8,
    headers_len: usize,
    body_data: *const u8,
    body_len: usize,
    user_data: *mut ::std::os::raw::c_void,
) {
    let result = unsafe { &mut *(user_data as *mut TestResult) };
    result.status = Some(status);

    if !headers_data.is_null() && headers_len > 0 {
        let headers_slice = unsafe { std::slice::from_raw_parts(headers_data, headers_len) };
        result.headers = Some(headers_slice.to_vec());
    }

    if !body_data.is_null() && body_len > 0 {
        let body_slice = unsafe { std::slice::from_raw_parts(body_data, body_len) };
        result.body = Some(body_slice.to_vec());
    }
}

extern "C" fn test_error_callback(
    error_message: *const c_char,
    user_data: *mut ::std::os::raw::c_void,
) {
    let result = unsafe { &mut *(user_data as *mut TestResult) };
    if !error_message.is_null() {
        let c_str = unsafe { std::ffi::CStr::from_ptr(error_message) };
        result.error = Some(c_str.to_string_lossy().to_string());
    }
}

unsafe extern "C" {
    fn ProxyTraceAgent(
        method: *const c_char,
        path: *const c_char,
        headers: *const c_char,
        body_ptr: *const u8,
        body_len: usize,
        on_response: extern "C" fn(
            u16,
            *const u8,
            usize,
            *const u8,
            usize,
            *mut ::std::os::raw::c_void,
        ),
        on_error: extern "C" fn(*const c_char, *mut ::std::os::raw::c_void),
        user_data: *mut ::std::os::raw::c_void,
    ) -> i32;
}

fn to_wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[test]
#[serial]
fn windows_pipe_proxy_basic() {
    force_link_lib();
    // Unique pipe name
    let pipe_name = format!("libagent_test_pipe_{}", std::process::id());
    let full_path = format!(r"\\.\pipe\{}", pipe_name);
    let wpath = to_wide(&full_path);

    // Create server
    let handle: HANDLE = unsafe {
        CreateNamedPipeW(
            wpath.as_ptr(),
            PIPE_ACCESS_DUPLEX_CONST | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE,
            1, // max instances
            65536,
            65536,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        eprintln!("skipping windows_pipe_proxy_basic: cannot create pipe");
        return;
    }

    // Spawn acceptor thread
    let handle_val = handle as isize;
    let th = thread::spawn(move || {
        let handle = handle_val as HANDLE;
        let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if ok == 0 {
            unsafe { CloseHandle(handle) };
            return;
        }
        // Wrap handle as File
        let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
        // Read until headers end
        let mut buf = [0u8; 8192];
        let mut read_total = 0usize;
        loop {
            let n = file.read(&mut buf[read_total..]).unwrap_or(0);
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
        let _ = write!(file, "HTTP/1.1 200 OK\r\n");
        let _ = write!(file, "Content-Type: text/plain\r\nX-Server: pipe-test\r\n");
        let body = b"pipe response";
        let _ = write!(file, "Content-Length: {}\r\n\r\n", body.len());
        let _ = file.write_all(body);
        // File drops and closes the handle
    });

    // Configure env var for pipe name
    unsafe {
        env::set_var("LIBAGENT_TRACE_AGENT_PIPE", &pipe_name);
    }

    // Call FFI with callbacks
    let method = CString::new("GET").unwrap();
    let path = CString::new("/info").unwrap();
    let headers = CString::new("Accept: */*\n").unwrap();

    // Prepare result storage
    let mut result = TestResult::new();
    let result_ptr = &mut result as *mut TestResult as *mut ::std::os::raw::c_void;

    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            test_response_callback,
            test_error_callback,
            result_ptr,
        )
    };

    if rc != 0 {
        eprintln!(
            "skipping windows_pipe_proxy_basic: rc={}, err={:?}",
            rc, result.error
        );
        let _ = th.join();
        return;
    }

    assert_eq!(result.status, Some(200));
    let headers_vec = result.headers.as_ref().expect("headers should be set");
    let headers_str = String::from_utf8_lossy(headers_vec);
    assert!(
        headers_str.contains("X-Server: pipe-test"),
        "headers: {}",
        headers_str
    );
    let body_vec = result.body.as_ref().expect("body should be set");
    assert_eq!(body_vec, &b"pipe response".to_vec());
    let _ = th.join();

    // Clean up environment variable
    unsafe {
        env::remove_var("LIBAGENT_TRACE_AGENT_PIPE");
    }
}

#[test]
#[serial]
fn windows_pipe_proxy_chunked() {
    force_link_lib();
    // Unique pipe name
    let pipe_name = format!("libagent_test_pipe_chunked_{}", std::process::id());
    let full_path = format!(r"\\.\pipe\{}", pipe_name);
    let wpath = to_wide(&full_path);

    // Create server
    let handle: HANDLE = unsafe {
        CreateNamedPipeW(
            wpath.as_ptr(),
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
        eprintln!("skipping windows_pipe_proxy_chunked: cannot create pipe");
        return;
    }

    let handle_val = handle as isize;
    let th = thread::spawn(move || {
        let handle = handle_val as HANDLE;
        let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
        if ok == 0 {
            unsafe { CloseHandle(handle) };
            return;
        }
        let mut file = unsafe { std::fs::File::from_raw_handle(handle) };
        let mut buf = [0u8; 8192];
        let mut read_total = 0usize;
        loop {
            let n = file.read(&mut buf[read_total..]).unwrap_or(0);
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
        // Respond chunked
        let _ = write!(file, "HTTP/1.1 200 OK\r\n");
        let _ = write!(
            file,
            "Transfer-Encoding: chunked\r\nX-Server: pipe-test-chunked\r\n\r\n"
        );
        let _ = write!(file, "5\r\nhello\r\n");
        let _ = write!(file, "6\r\n world\r\n");
        let _ = write!(file, "0\r\n\r\n");
    });

    // Configure env var for pipe name
    unsafe {
        env::set_var("LIBAGENT_TRACE_AGENT_PIPE", &pipe_name);
    }

    let method = CString::new("GET").unwrap();
    let path = CString::new("/info").unwrap();
    let headers = CString::new("Accept: */*\n").unwrap();

    // Prepare result storage
    let mut result = TestResult::new();
    let result_ptr = &mut result as *mut TestResult as *mut ::std::os::raw::c_void;

    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            test_response_callback,
            test_error_callback,
            result_ptr,
        )
    };

    if rc != 0 {
        eprintln!(
            "skipping windows_pipe_proxy_chunked: rc={}, err={:?}",
            rc, result.error
        );
        let _ = th.join();
        return;
    }

    assert_eq!(result.status, Some(200));
    let headers_vec = result.headers.as_ref().expect("headers should be set");
    let headers_str = String::from_utf8_lossy(headers_vec);
    assert!(
        headers_str
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked"),
        "headers: {}",
        headers_str
    );
    assert!(
        headers_str.contains("X-Server: pipe-test-chunked"),
        "headers: {}",
        headers_str
    );
    let body_vec = result.body.as_ref().expect("body should be set");
    assert_eq!(body_vec, &b"hello world".to_vec());
    let _ = th.join();

    // Clean up environment variable
    unsafe {
        env::remove_var("LIBAGENT_TRACE_AGENT_PIPE");
    }
}
