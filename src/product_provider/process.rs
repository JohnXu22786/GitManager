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
// Darwin kill(-pgid, signal) skips zombies and can return EPERM when the
// retained leader is the only member. Never suppress EPERM by errno alone.
// Apple XNU: bsd/kern/kern_sig.c, killpg1() and its SZOMB filter.
#[cfg(any(test, target_os = "linux", target_os = "macos"))]
fn group_signal_result(
    error: std::io::Error,
    darwin: bool,
    only_owned_zombie: impl FnOnce() -> Result<bool, String>,
) -> Result<(), String> {
    if error.raw_os_error() == Some(3) {
        return Ok(());
    } // ESRCH
    if darwin && error.raw_os_error() == Some(1) {
        // EPERM
        match only_owned_zombie() {
            Ok(true) => return Ok(()),
            Ok(false) => (),
            Err(detail) => {
                return Err(format!(
                    "{error}; owned group verification failed: {detail}"
                ))
            }
        }
    }
    Err(error.to_string())
}

#[cfg(any(test, target_os = "macos"))]
fn group_is_only_owner(owner: u32, bytes: i32, members: &[i32]) -> Result<bool, String> {
    let width = std::mem::size_of::<i32>();
    if owner <= 1
        || owner > i32::MAX as u32
        || bytes <= 0
        || bytes as usize % width != 0
        || bytes as usize >= std::mem::size_of_val(members)
    {
        return Err("empty, invalid or possibly truncated group membership".into());
    }
    let count = bytes as usize / width;
    let pids = &members[..count];
    if pids.iter().any(|pid| *pid <= 1) {
        return Err("invalid group member PID".into());
    }
    // This deliberately does not exempt additional members, even zombies:
    // only our still-unreaped leader has independently established identity.
    Ok(pids == [owner as i32])
}

#[cfg(target_os = "macos")]
fn darwin_group_is_only_owner(owner: u32) -> Result<bool, String> {
    #[link(name = "proc")]
    unsafe extern "C" {
        fn proc_listpids(kind: u32, group: u32, buffer: *mut std::ffi::c_void, bytes: i32) -> i32;
    }
    // PROC_PGRP_ONLY=2 restricts this kernel snapshot to our held group. It
    // includes zombies. No command lines, environment, paths or credentials
    // are requested. A full buffer, error or any extra PID fails closed.
    // Apple XNU: sys/proc_info.h; kern/proc_info.c; wrappers/libproc/libproc.c.
    let mut members = [0i32; 1024];
    let bytes = unsafe {
        proc_listpids(
            2,
            owner,
            members.as_mut_ptr().cast(),
            std::mem::size_of_val(&members) as i32,
        )
    };
    if bytes <= 0 {
        return Err(format!(
            "group snapshot unavailable: {}",
            std::io::Error::last_os_error()
        ));
    }
    group_is_only_owner(owner, bytes, &members)
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
                group_signal_result(error, cfg!(target_os = "macos"), || {
                    #[cfg(target_os = "macos")]
                    {
                        // Recheck terminal ownership without reaping. The
                        // leader therefore still reserves this exact PGID
                        // throughout the group snapshot and signal decision.
                        if !exited_without_reaping(self.child.id())? {
                            return Ok(false);
                        }
                        darwin_group_is_only_owner(self.child.id())
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        Ok(false)
                    }
                })
                .err()
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

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[test]
    fn darwin_eperm_requires_proof_of_only_the_owned_zombie() {
        let denied = || std::io::Error::from_raw_os_error(1);
        assert!(group_signal_result(denied(), true, || Ok(true)).is_ok());
        assert!(group_signal_result(denied(), true, || Ok(false)).is_err());
        assert!(
            group_signal_result(denied(), true, || Err("membership unavailable".into())).is_err()
        );
        assert!(
            group_signal_result(denied(), false, || panic!("Linux must not suppress EPERM"))
                .is_err()
        );
        assert!(
            group_signal_result(std::io::Error::from_raw_os_error(22), true, || panic!(
                "unrelated errors must not inspect groups"
            ))
            .is_err()
        );
        assert!(
            group_signal_result(std::io::Error::from_raw_os_error(3), true, || panic!(
                "missing group needs no exception"
            ))
            .is_ok()
        );
    }
    #[test]
    fn group_evidence_rejects_truncation_foreign_and_additional_members() {
        assert!(group_is_only_owner(42, 4, &[42, 0, 0]).unwrap());
        assert!(!group_is_only_owner(42, 4, &[43, 0, 0]).unwrap());
        assert!(!group_is_only_owner(42, 8, &[42, 43, 0]).unwrap());
        for bytes in [-1, 0, 1, 3, 12, 16] {
            assert!(
                group_is_only_owner(42, bytes, &[42, 0, 0]).is_err(),
                "{bytes}"
            );
        }
        assert!(group_is_only_owner(0, 4, &[0, 0, 0]).is_err());
        assert!(group_is_only_owner(42, 4, &[-1, 0, 0]).is_err());
    }
}
