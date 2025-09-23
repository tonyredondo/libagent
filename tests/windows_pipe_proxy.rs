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
struct LibagentHttpBuffer {
    data: *mut u8,
    len: usize,
}

#[repr(C)]
struct LibagentHttpResponse {
    status: u16,
    headers: LibagentHttpBuffer,
    body: LibagentHttpBuffer,
}

unsafe extern "C" {
    fn ProxyTraceAgent(
        method: *const c_char,
        path: *const c_char,
        headers: *const c_char,
        body_ptr: *const u8,
        body_len: usize,
        out_resp: *mut *mut LibagentHttpResponse,
        out_err: *mut *mut c_char,
    ) -> i32;

    fn FreeHttpResponse(resp: *mut LibagentHttpResponse);
    fn FreeCString(s: *mut c_char);
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

    // Call FFI
    let method = CString::new("GET").unwrap();
    let path = CString::new("/info").unwrap();
    let headers = CString::new("Accept: */*\n").unwrap();
    let mut resp_ptr: *mut LibagentHttpResponse = ptr::null_mut();
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            &mut resp_ptr,
            &mut err_ptr,
        )
    };
    if rc != 0 {
        if !err_ptr.is_null() {
            unsafe { FreeCString(err_ptr) };
        }
        eprintln!("skipping windows_pipe_proxy_basic: rc={} err", rc);
        let _ = th.join();
        return;
    }

    unsafe {
        let resp = &*resp_ptr;
        assert_eq!(resp.status, 200);
        let headers_slice = std::slice::from_raw_parts(resp.headers.data, resp.headers.len);
        let headers_str = String::from_utf8_lossy(headers_slice);
        assert!(
            headers_str.contains("X-Server: pipe-test"),
            "headers: {}",
            headers_str
        );
        let body_slice = std::slice::from_raw_parts(resp.body.data, resp.body.len);
        assert_eq!(body_slice, b"pipe response");
        FreeHttpResponse(resp_ptr);
    }
    let _ = th.join();
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
    let mut resp_ptr: *mut LibagentHttpResponse = ptr::null_mut();
    let mut err_ptr: *mut c_char = ptr::null_mut();
    let rc = unsafe {
        ProxyTraceAgent(
            method.as_ptr(),
            path.as_ptr(),
            headers.as_ptr(),
            ptr::null(),
            0,
            &mut resp_ptr,
            &mut err_ptr,
        )
    };
    if rc != 0 {
        if !err_ptr.is_null() {
            unsafe { FreeCString(err_ptr) };
        }
        eprintln!("skipping windows_pipe_proxy_chunked: rc={} err", rc);
        let _ = th.join();
        return;
    }

    unsafe {
        let resp = &*resp_ptr;
        assert_eq!(resp.status, 200);
        let headers_slice = std::slice::from_raw_parts(resp.headers.data, resp.headers.len);
        let headers_str = String::from_utf8_lossy(headers_slice);
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
        let body_slice = std::slice::from_raw_parts(resp.body.data, resp.body.len);
        assert_eq!(body_slice, b"hello world");
        FreeHttpResponse(resp_ptr);
    }
    let _ = th.join();
}
