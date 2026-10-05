//! Read-only probes never generate text, install, log in, or read credentials.
use super::{input, process, protocol::*};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};

#[derive(Debug, Clone)]
pub(crate) struct Endpoint {
    pub kind: ProviderKind,
    pub executable: PathBuf,
    pub home: PathBuf,
    pub fixture: bool,
}
impl Endpoint {
    pub(crate) fn environment(&self) -> BTreeMap<OsString, OsString> {
        let mut env = BTreeMap::new();
        env.insert("HOME".into(), self.home.as_os_str().into());
        let mut paths = vec![];
        if let Some(parent) = self.executable.parent() {
            paths.push(parent.to_owned());
        }
        paths.extend([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
        env.insert(
            "PATH".into(),
            std::env::join_paths(paths).unwrap_or_default(),
        );
        env.insert("LANG".into(), "C.UTF-8".into());
        env.insert("LC_ALL".into(), "C.UTF-8".into());
        env
    }
    pub(crate) fn environment_digest(&self) -> String {
        let values: Vec<_> = self
            .environment()
            .into_iter()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect();
        encoded_digest(&values)
    }
    pub(crate) fn command_digest(&self, directory: &Path, schema: &[u8], nonce: &str) -> String {
        let arguments: Vec<String> = super::adapter::arguments(self.kind, directory, schema, nonce)
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        encoded_digest(&(
            self.executable.to_string_lossy(),
            directory.to_string_lossy(),
            arguments,
            self.environment_digest(),
        ))
    }
    pub(crate) fn command(&self, directory: &Path) -> Command {
        let mut cmd = Command::new(&self.executable);
        cmd.env_clear()
            .envs(self.environment())
            .current_dir(directory);
        cmd
    }
    pub(crate) fn probe(&self, directory: &Path) -> ProviderCapabilities {
        let mut cap = ProviderCapabilities {
            provider: self.kind,
            readiness: Readiness::ProbeFailed,
            executable_digest: String::new(),
            version: String::new(),
            probe_digest: String::new(),
            data_only: false,
            trusted_harness: false,
            detail: String::new(),
            provenance: if self.fixture {
                InvocationProvenance::TransportFixture
            } else {
                InvocationProvenance::LiveCli
            },
        };
        if !self.executable.exists() {
            cap.readiness = Readiness::Missing;
            cap.detail = "selected CLI is not installed".into();
            return cap;
        }
        cap.executable_digest = match executable_digest(&self.executable) {
            Ok(v) => v,
            Err(e) => {
                cap.detail = e;
                return cap;
            }
        };
        if !cfg!(any(target_os = "linux", target_os = "macos")) {
            cap.readiness = Readiness::UnsupportedPlatform;
            cap.detail = "bounded process-tree transport currently requires Unix".into();
            return cap;
        }
        let limits = JobLimits {
            timeout_ms: 5_000,
            stdout_bytes: 64 * 1024,
            stderr_bytes: 64 * 1024,
            result_bytes: MAX_RESULT_BYTES,
        };
        let mut evidence = vec![];
        let mut call = |args: &[&str]| -> Result<process::Output, String> {
            let mut cmd = self.command(directory);
            cmd.args(args);
            let out = process::run(cmd, b"", &limits, &AtomicBool::new(false), None, None);
            evidence.push(json_evidence(args, &out));
            if out.failure.is_some() {
                Err(format!(
                    "read-only CLI probe {} failed ({:?}): {}",
                    args.join(" "),
                    out.failure,
                    out.detail
                ))
            } else {
                Ok(out)
            }
        };
        let result = (|| -> Result<(), String> {
            let version = call(&["--version"])?;
            if version.exit_code != Some(0) {
                return Err("version probe exited unsuccessfully".into());
            }
            cap.version = String::from_utf8(version.stdout)
                .map_err(|_| "version output is not UTF-8")?
                .trim()
                .to_owned();
            let supported = match self.kind {
                ProviderKind::Codex => cap.version == "codex-cli 0.159.2",
                ProviderKind::Claude => cap.version == "2.1.286 (Claude Code)",
            };
            if !supported {
                cap.readiness = Readiness::UnsupportedVersion;
                return Err("CLI version is outside the reviewed adapter versions".into());
            }
            let help = call(&["--help"])?;
            if help.exit_code != Some(0) {
                return Err("help probe exited unsuccessfully".into());
            }
            let mut help = String::from_utf8(help.stdout).map_err(|_| "help is not UTF-8")?;
            if self.kind == ProviderKind::Codex {
                let exec = call(&["exec", "--help"])?;
                if exec.exit_code != Some(0) {
                    return Err("exec help probe failed".into());
                }
                help.push_str(
                    &String::from_utf8(exec.stdout).map_err(|_| "exec help is not UTF-8")?,
                );
            }
            let required: &[&str] = match self.kind {
                ProviderKind::Codex => &[
                    "--json",
                    "--output-schema",
                    "--output-last-message",
                    "--ephemeral",
                    "--ignore-user-config",
                    "--ignore-rules",
                    "--no-daemon",
                    "--config",
                    "--disable",
                    "--cd",
                ],
                ProviderKind::Claude => &[
                    "--print",
                    "--output-format",
                    "--json-schema",
                    "--tools",
                    "--disallowedTools",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "--setting-sources",
                    "--settings",
                    "--restricted",
                    "--safe-mode",
                    "--no-chrome",
                    "--no-session-persistence",
                    "--permission-prompts",
                    "--session-id",
                    "--max-turns",
                ],
            };
            if required.iter().any(|flag| {
                !help
                    .split_whitespace()
                    .any(|word| word == *flag || word.trim_end_matches(',') == *flag)
            }) {
                cap.readiness = Readiness::UnsupportedCapability;
                return Err("installed help lacks a required adapter flag".into());
            }
            let auth = call(match self.kind {
                ProviderKind::Codex => &["login", "status"],
                ProviderKind::Claude => &["auth", "status"],
            })?;
            if auth.exit_code == Some(1) {
                cap.readiness = Readiness::AuthRequired;
                return Err("existing subscription authentication is required".into());
            }
            if auth.exit_code != Some(0) {
                return Err("authentication readiness probe failed".into());
            }
            let authenticated = match self.kind {
                ProviderKind::Codex => [auth.stdout.as_slice(), auth.stderr.as_slice()]
                    .concat()
                    .split(|b| *b == b'\n')
                    .any(|line| line == b"Logged in using ChatGPT"),
                ProviderKind::Claude => {
                    let v = input::parse_json_bytes(&auth.stdout)
                        .map_err(|_| "invalid auth status response")?;
                    v.get("loggedIn") == Some(&serde_json::json!(true))
                        && v.get("authMethod") == Some(&serde_json::json!("claude.ai"))
                }
            };
            if !authenticated {
                cap.readiness = Readiness::AuthRequired;
                return Err("subscription auth was not established; API and third-party auth are not enabled".into());
            }
            cap.readiness = Readiness::ReadyUntested;
            // Fake subprocesses are available ONLY through the cfg(test) factory.
            // Help/auth alone cannot attest to a live CLI's effective managed
            // hooks, tool surface, host discovery, or filesystem boundary.
            // The optional TrustedHarness profile explicitly retains that
            // uncertainty; it does NOT masquerade as DataOnly.
            cap.data_only = self.fixture;
            cap.trusted_harness = true;
            cap.detail=if self.fixture{"synthetic transport fixture; no model or sandbox evidence"}else{"CLI and subscription auth ready but live generation untested. Strict data-only is unavailable. Trusted-Harness requires separate consent for installed/admin configuration, hooks, plugins/MCP and host context; effective isolation is unverified."}.into();
            Ok(())
        })();
        if let Err(e) = result {
            cap.detail = e;
        }
        cap.probe_digest = encoded_digest(&evidence);
        cap
    }
}
fn json_evidence(args: &[&str], output: &process::Output) -> serde_json::Value {
    serde_json::json!({"argv":args,"exit_code":output.exit_code,"stdout_digest":digest(&output.stdout),"stderr_digest":digest(&output.stderr),"failure":output.failure})
}
pub(crate) fn executable_digest(path: &Path) -> Result<String, String> {
    // Reject devices/FIFOs before open; retain nonblocking/no-follow flags
    // and descriptor validation so a replacement between stat and open cannot
    // turn a readiness probe into an unbounded pipe/device read.
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("CLI must be a regular executable file".into());
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "linux")]
        options.custom_flags(0x20000 | 0x800);
        #[cfg(target_os = "macos")]
        options.custom_flags(0x100 | 0x4);
    }
    let mut file = options.open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err("CLI is not a bounded regular executable".into());
    }
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        total += n;
        if total > 512 * 1024 * 1024 {
            return Err("CLI changed while hashing".into());
        }
        hash.update(&buffer[..n]);
    }
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
