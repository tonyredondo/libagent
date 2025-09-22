#![allow(dead_code)]
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

pub fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut f = File::create(path).expect("create file");
    f.write_all(contents.as_bytes()).expect("write file");
}

pub fn read_to_string(path: &Path) -> String {
    let mut s = String::new();
    if let Ok(mut f) = File::open(path) {
        let _ = f.read_to_string(&mut s);
    }
    s
}

pub fn wait_for_lines(path: &Path, min_lines: usize, timeout: Duration) -> Vec<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let content = read_to_string(path);
        let lines: Vec<String> = content
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if lines.len() >= min_lines {
            return lines;
        }
        if Instant::now() >= deadline {
            return lines;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
pub mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    pub struct StubPaths {
        pub script: PathBuf,
        pub starts: PathBuf,
        pub events: PathBuf,
    }

    pub fn create_stub_sleep(dir: &Path) -> StubPaths {
        let script = dir.join("stub_agent.sh");
        let starts = dir.join("starts.log");
        let events = dir.join("events.log");
        let script_contents = format!(
            "#!/bin/sh\nset -eu\nSTARTS=\"{}\"\nEVENTS=\"{}\"\necho $$ >> \"$STARTS\"\ntrap 'echo \"TERM $$\" >> \"$EVENTS\"; exit 0' TERM\nwhile :; do sleep 1; done\n",
            starts.display(),
            events.display()
        );
        write_file(&script, &script_contents);
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
        StubPaths { script, starts, events }
    }

    pub fn create_stub_exit(dir: &Path) -> (PathBuf, PathBuf) {
        let script = dir.join("stub_exit.sh");
        let starts = dir.join("starts.log");
        let script_contents = format!(
            "#!/bin/sh\nset -eu\nSTARTS=\"{}\"\necho $$ >> \"$STARTS\"\nexit 0\n",
            starts.display()
        );
        write_file(&script, &script_contents);
        let mut perm = fs::metadata(&script).unwrap().permissions();
        perm.set_mode(0o755);
        fs::set_permissions(&script, perm).unwrap();
        (script, starts)
    }
}

#[cfg(windows)]
pub mod windows {
    pub fn powershell_sleep_args(seconds: u64) -> (&'static str, String) {
        (
            "powershell.exe",
            format!("-NoProfile -Command Start-Sleep -Seconds {}", seconds),
        )
    }
}


