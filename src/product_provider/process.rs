//! Lifecycle bounds for trusted installed CLIs. A process group is not a sandbox.
use super::protocol::*;
use std::{
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};

// Only test fixtures publish temporary executable bytes. Coordinate that
// publication with test forks so a child cannot inherit a transient writable
// script descriptor and manufacture ETXTBSY. Production behavior is unchanged.
#[cfg(test)]
pub(crate) static FIXTURE_SPAWN_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
    pub failure: Option<JobState>,
    pub detail: String,
}
impl Output {
    fn error(detail: String) -> Self {
        Self {
            stdout: vec![],
            stderr: vec![],
            exit_code: None,
            failure: Some(JobState::ProviderError),
            detail,
        }
    }
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub(crate) fn run(
    _: Command,
    _: &[u8],
    _: &JobLimits,
    _: &AtomicBool,
    _: Option<&std::path::Path>,
    _: Option<u64>,
) -> Output {
    Output::error("process-tree transport has not been implemented on this platform".into())
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) fn run(
    mut command: Command,
    input: &[u8],
    limits: &JobLimits,
    cancel: &AtomicBool,
    output_file: Option<&std::path::Path>,
    authorization_expires: Option<u64>,
) -> Output {
    use std::{
        io::{Read, Write},
        os::{fd::AsRawFd, unix::process::CommandExt},
        process::{Child, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
        fn kill(pid: i32, signal: i32) -> i32;
        fn waitid(kind: i32, id: u32, info: *mut std::ffi::c_void, options: i32) -> i32;
    }
    fn nonblocking(fd: i32) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        let flag = 0x800;
        #[cfg(not(target_os = "linux"))]
        let flag = 0x4;
        // F_GETFL/F_SETFL operate only on our newly created pipe descriptors.
        let old = unsafe { fcntl(fd, 3) };
        if old < 0 || unsafe { fcntl(fd, 4, old | flag) } < 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }
    fn exited_without_reaping(pid: u32) -> Result<bool, String> {
        // Linux and Darwin siginfo_t begin with si_signo and fit in 128 bytes.
        // Overallocate aligned opaque storage: no union layout is interpreted.
        // waitid(P_PID, WEXITED | WNOHANG | WNOWAIT) retains the leader as a
        // waitable zombie, so its numeric PGID cannot be reused before cleanup.
        #[cfg(target_os = "linux")]
        const WNOWAIT: i32 = 0x0100_0000;
        #[cfg(target_os = "macos")]
        const WNOWAIT: i32 = 0x20;
        let mut info = [0u64; 32];
        if unsafe { waitid(1, pid, info.as_mut_ptr().cast(), 4 | 1 | WNOWAIT) } < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(format!(
                "child ownership/status unavailable; cleanup unconfirmed: {error}"
            ));
        }
        Ok(unsafe { *info.as_ptr().cast::<i32>() } != 0)
    }
    struct Guard {
        child: Child,
        status: Option<ExitStatus>,
    }
    impl Guard {
        fn stop(&mut self) -> Result<ExitStatus, String> {
            if let Some(status) = self.status {
                return Ok(status);
            }
            // Verify this is still our unreaped child; never signal after
            // ECHILD, and never use a PID recovered from persistent storage.
            exited_without_reaping(self.child.id())?;
            let group_error = if unsafe { kill(-(self.child.id() as i32), 9) } < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(3) {
                    None
                } else {
                    Some(error.to_string())
                }
            } else {
                None
            };
            let _ = self.child.kill();
            let status = self.child.wait().map_err(|e| e.to_string())?;
            self.status = Some(status);
            if let Some(error) = group_error {
                Err(format!("process group cleanup failed: {error}"))
            } else {
                Ok(status)
            }
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }
    fn drain(reader: &mut impl Read, buffer: &mut Vec<u8>, limit: usize) -> Result<bool, String> {
        let mut chunk = [0u8; 8192];
        // Bounded work per poll keeps cancellation responsive under floods.
        for _ in 0..32 {
            match reader.read(&mut chunk) {
                Ok(0) => return Ok(true),
                Ok(n) => {
                    if n > limit.saturating_sub(buffer.len()) {
                        return Err("byte limit".into());
                    }
                    buffer.extend_from_slice(&chunk[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(false)
    }
    if cancel.load(Ordering::Acquire) {
        let mut out = Output::error("cancelled before launch".into());
        out.failure = Some(JobState::Cancelled);
        return out;
    }
    command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if authorization_expires.is_some_and(|expires| expires <= unix_ms()) {
        let mut out = Output::error("authorization expired before process launch".into());
        out.failure = Some(JobState::AuthorizationExpired);
        return out;
    }
    let spawned = {
        #[cfg(test)]
        let _fixture_guard = match FIXTURE_SPAWN_GATE.lock() {
            Ok(guard) => guard,
            Err(_) => return Output::error("fixture spawn coordination poisoned".into()),
        };
        // The test-only mutex can wait; honor cancellation/expiry at spawn too.
        if cancel.load(Ordering::Acquire) {
            let mut out = Output::error("cancelled before launch".into());
            out.failure = Some(JobState::Cancelled);
            return out;
        }
        if authorization_expires.is_some_and(|expires| expires <= unix_ms()) {
            let mut out = Output::error("authorization expired before process launch".into());
            out.failure = Some(JobState::AuthorizationExpired);
            return out;
        }
        command.spawn()
    };
    let child = match spawned {
        Ok(child) => child,
        Err(e) => return Output::error(format!("could not start CLI: {e}")),
    };
    let mut child = Guard {
        child,
        status: None,
    };
    let mut stdin = child.child.stdin.take();
    let mut stdout = child.child.stdout.take().expect("piped stdout");
    let mut stderr = child.child.stderr.take().expect("piped stderr");
    for fd in [
        stdin.as_ref().unwrap().as_raw_fd(),
        stdout.as_raw_fd(),
        stderr.as_raw_fd(),
    ] {
        if let Err(e) = nonblocking(fd) {
            return Output::error(e);
        }
    }
    let start = Instant::now();
    let mut ended = None;
    let mut written = 0;
    let mut out = Output {
        stdout: vec![],
        stderr: vec![],
        exit_code: None,
        failure: None,
        detail: String::new(),
    };
    loop {
        if cancel.load(Ordering::Acquire) {
            out.failure = Some(JobState::Cancelled);
            out.detail = "cancellation requested".into();
            break;
        }
        if start.elapsed() >= Duration::from_millis(limits.timeout_ms) {
            out.failure = Some(JobState::TimedOut);
            out.detail = "job deadline exceeded".into();
            break;
        }
        if let Some(writer) = stdin.as_mut() {
            match writer.write(&input[written..]) {
                Ok(0) if written < input.len() => {
                    out.failure = Some(JobState::ProviderError);
                    out.detail = "CLI closed stdin before complete request".into();
                    break;
                }
                Ok(n) => written += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => {
                    out.failure = Some(JobState::ProviderError);
                    out.detail = format!("request stdin failed: {e}");
                    break;
                }
            }
            if written == input.len() {
                stdin.take();
            }
        }
        let a = drain(&mut stdout, &mut out.stdout, limits.stdout_bytes);
        let b = drain(&mut stderr, &mut out.stderr, limits.stderr_bytes);
        if let Some(error) = a.as_ref().err().or(b.as_ref().err()) {
            out.failure = Some(if error == "byte limit" {
                JobState::OutputLimit
            } else {
                JobState::ProviderError
            });
            out.detail = format!("CLI output capture failed: {error}");
            break;
        }
        if let Some(path) = output_file {
            match std::fs::symlink_metadata(path) {
                Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
                    out.failure = Some(JobState::InvalidOutput);
                    out.detail = "designated output is not a regular file".into();
                    break;
                }
                Ok(m) if m.len() > limits.result_bytes as u64 => {
                    out.failure = Some(JobState::OutputLimit);
                    out.detail = "designated output exceeds byte limit".into();
                    break;
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    out.failure = Some(JobState::ProviderError);
                    out.detail = e.to_string();
                    break;
                }
            }
        }
        if ended.is_none() {
            match exited_without_reaping(child.child.id()) {
                Ok(true) => {
                    match child.stop() {
                        Ok(status) => out.exit_code = status.code(),
                        Err(error) => {
                            out.failure = Some(JobState::ProviderError);
                            out.detail = error;
                            break;
                        }
                    }
                    ended = Some(Instant::now());
                    if written != input.len() {
                        out.failure = Some(JobState::ProviderError);
                        out.detail =
                            "CLI exited before the complete request was delivered on stdin".into();
                        break;
                    }
                }
                Ok(false) => (),
                Err(error) => {
                    out.failure = Some(JobState::ProviderError);
                    out.detail = error;
                    break;
                }
            }
        }
        if let Some(ended) = ended {
            if a == Ok(true) && b == Ok(true) {
                break;
            }
            if ended.elapsed() > Duration::from_millis(500) {
                out.failure = Some(JobState::ProviderError);
                out.detail = "pipes remained open after process-tree cleanup".into();
                break;
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    if let Err(error) = child.stop() {
        out.failure = Some(JobState::ProviderError);
        out.detail = error;
    }
    out
}
