use crate::tasks::TaskRecord;
use ring::rand::{SecureRandom, SystemRandom};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

pub const CODEX_PROVIDER_REF: &str = "codex";
pub const CLAUDE_PROVIDER_REF: &str = "claude";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeCapability {
    Unsupported,
    UserSelectsSession,
    SessionReference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessCapabilities {
    pub can_start: bool,
    pub tracks_sessions: bool,
    pub resume: ResumeCapability,
    pub continue_latest: bool,
}

pub struct HarnessAvailability {
    pub available: bool,
    pub message: String,
}

/// Narrow task-facing contract for providers that can open a task in a harness.
pub trait TaskHarness {
    fn provider_ref(&self) -> &'static str;
    fn capabilities(&self) -> HarnessCapabilities;
    fn availability(&self) -> HarnessAvailability;
    fn start(&self, task: &TaskRecord) -> Result<Option<String>, String>;
    fn resume(&self, task: &TaskRecord) -> Result<(), String>;
}

pub struct CodexHarness;

impl TaskHarness for CodexHarness {
    fn provider_ref(&self) -> &'static str {
        CODEX_PROVIDER_REF
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            can_start: true,
            tracks_sessions: false,
            resume: ResumeCapability::UserSelectsSession,
            continue_latest: false,
        }
    }

    fn availability(&self) -> HarnessAvailability {
        if find_program("codex").is_none() {
            return HarnessAvailability {
                available: false,
                message: "Codex CLI was not found in PATH".into(),
            };
        }

        match terminal_availability("Codex") {
            Ok(()) => HarnessAvailability {
                available: true,
                message: "Codex CLI is available; session IDs are not tracked".into(),
            },
            Err(message) => HarnessAvailability {
                available: false,
                message,
            },
        }
    }

    fn start(&self, task: &TaskRecord) -> Result<Option<String>, String> {
        let codex = find_program("codex")
            .ok_or_else(|| "Codex CLI was not found in PATH".to_string())?;
        let worktree = Path::new(&task.worktree_path);
        if !worktree.is_dir() {
            return Err("The task worktree is unavailable".into());
        }

        launch_harness(
            &codex,
            worktree,
            &[
                OsString::from("--cd"),
                OsString::from(task.worktree_path.as_str()),
                OsString::from("--"),
                OsString::from(task.title.as_str()),
            ],
            "Codex",
        )
        .map(|()| None)
    }

    fn resume(&self, task: &TaskRecord) -> Result<(), String> {
        let codex = find_program("codex")
            .ok_or_else(|| "Codex CLI was not found in PATH".to_string())?;
        let worktree = Path::new(&task.worktree_path);
        if !worktree.is_dir() {
            return Err("The task worktree is unavailable".into());
        }

        // Codex's picker is filtered to the current working directory unless --all is used.
        // This app does not receive an interactive CLI thread ID, so the user selects a session.
        let mut args = vec![OsString::from("resume")];
        if task.provider_ref.as_deref() == Some(CODEX_PROVIDER_REF) {
            if let Some(session_ref) = task.session_ref.as_deref() {
                args.push(OsString::from(session_ref));
            }
        }
        launch_harness(&codex, worktree, &args, "Codex")
    }
}

pub struct ClaudeHarness;

impl TaskHarness for ClaudeHarness {
    fn provider_ref(&self) -> &'static str {
        CLAUDE_PROVIDER_REF
    }

    fn capabilities(&self) -> HarnessCapabilities {
        let support = claude_cli_support();
        HarnessCapabilities {
            can_start: true,
            tracks_sessions: support.session_id,
            resume: if support.resume {
                ResumeCapability::SessionReference
            } else {
                ResumeCapability::Unsupported
            },
            continue_latest: support.continue_session,
        }
    }

    fn availability(&self) -> HarnessAvailability {
        let Some(claude) = find_program("claude") else {
            return HarnessAvailability {
                available: false,
                message: "Claude Code CLI was not found in PATH".into(),
            };
        };

        if let Err(message) = terminal_availability("Claude Code") {
            return HarnessAvailability {
                available: false,
                message,
            };
        }

        let support = claude_cli_support_for(&claude);
        let message = if !support.inspected {
            "Claude Code CLI is available; session support could not be checked, so exact tracking and resume may be unavailable. Session creation and run state are not confirmed or tracked.".into()
        } else if support.session_id && support.resume {
            "Claude Code CLI can assign a task session ID for best-effort resume. Session creation and run state are not confirmed or tracked.".into()
        } else if support.session_id && support.continue_session {
            "Claude Code CLI can assign a task session ID, but continuation opens only the most recent conversation in this worktree. Session creation and run state are not confirmed or tracked.".into()
        } else if support.session_id {
            "Claude Code CLI can assign a task session ID, but this version does not support session resume. Session creation and run state are not confirmed or tracked.".into()
        } else if support.continue_session {
            "Claude Code CLI is available; resume opens the most recent conversation in this worktree. Session IDs and run state are not tracked.".into()
        } else {
            "Claude Code CLI is available; session tracking and resume are unavailable".into()
        };
        HarnessAvailability {
            available: true,
            message,
        }
    }

    fn start(&self, task: &TaskRecord) -> Result<Option<String>, String> {
        let claude = find_program("claude")
            .ok_or_else(|| "Claude Code CLI was not found in PATH".to_string())?;
        let worktree = Path::new(&task.worktree_path);
        if !worktree.is_dir() {
            return Err("The task worktree is unavailable".into());
        }

        let support = claude_cli_support_for(&claude);
        let session_ref = if support.session_id {
            generate_session_id()
        } else {
            None
        };
        let mut args = Vec::new();
        if let Some(session_ref) = &session_ref {
            args.extend([OsString::from("--session-id"), OsString::from(session_ref)]);
        }
        args.push(OsString::from("--"));
        args.push(OsString::from(task.title.as_str()));
        launch_harness(&claude, worktree, &args, "Claude Code")?;
        Ok(session_ref)
    }

    fn resume(&self, task: &TaskRecord) -> Result<(), String> {
        let claude = find_program("claude")
            .ok_or_else(|| "Claude Code CLI was not found in PATH".to_string())?;
        let worktree = Path::new(&task.worktree_path);
        if !worktree.is_dir() {
            return Err("The task worktree is unavailable".into());
        }

        let support = claude_cli_support_for(&claude);
        let mut args = Vec::new();
        if support.resume && task.provider_ref.as_deref() == Some(CLAUDE_PROVIDER_REF) {
            if let Some(session_ref) = task.session_ref.as_deref() {
                args.extend([OsString::from("--resume"), OsString::from(session_ref)]);
            }
        }
        if args.is_empty() {
            if support.continue_session {
                args.push(OsString::from("--continue"));
            } else {
                return Err(
                    "This Claude Code CLI does not support resuming or continuing a session".into(),
                );
            }
        }
        launch_harness(&claude, worktree, &args, "Claude Code")
    }
}

#[derive(Clone, Copy, Default)]
struct ClaudeCliSupport {
    inspected: bool,
    session_id: bool,
    resume: bool,
    continue_session: bool,
}

static CLAUDE_CLI_SUPPORT: OnceLock<ClaudeCliSupport> = OnceLock::new();

fn claude_cli_support() -> ClaudeCliSupport {
    if let Some(support) = CLAUDE_CLI_SUPPORT.get() {
        return *support;
    }
    let Some(claude) = find_program("claude") else {
        return ClaudeCliSupport::default();
    };
    claude_cli_support_for(&claude)
}

fn claude_cli_support_for(program: &Path) -> ClaudeCliSupport {
    if let Some(support) = CLAUDE_CLI_SUPPORT.get() {
        return *support;
    }
    // Inspect the installed CLI instead of assuming it supports current documented flags.
    let support = probe_claude_cli(program).unwrap_or_default();
    let _ = CLAUDE_CLI_SUPPORT.set(support);
    support
}

fn probe_claude_cli(program: &Path) -> Option<ClaudeCliSupport> {
    let output = claude_help_output(program).ok()?;
    let help = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let inspected = output.status.success() && !help.trim().is_empty();
    Some(ClaudeCliSupport {
        inspected,
        session_id: inspected && help.contains("--session-id"),
        resume: inspected && help.contains("--resume"),
        continue_session: inspected && help.contains("--continue"),
    })
}

fn claude_help_output(program: &Path) -> std::io::Result<Output> {
    #[cfg(windows)]
    if program
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
        .unwrap_or(false)
    {
        return Command::new("powershell.exe")
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "& $env:GITMANAGER_CLAUDE_PROGRAM --help",
            ])
            .env("GITMANAGER_CLAUDE_PROGRAM", program)
            .output();
    }

    Command::new(program).arg("--help").output()
}

fn generate_session_id() -> Option<String> {
    use std::fmt::Write as _;

    let mut bytes = [0_u8; 16];
    SystemRandom::new().fill(&mut bytes).ok()?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let mut session_id = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            session_id.push('-');
        }
        let _ = write!(&mut session_id, "{byte:02x}");
    }
    Some(session_id)
}

fn find_program(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    #[cfg(windows)]
    let extensions: Vec<OsString> = std::env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(OsString::from)
                .collect()
        })
        .unwrap_or_else(|| vec![OsString::from(".EXE"), OsString::from(".CMD")]);
    #[cfg(not(windows))]
    let extensions = vec![OsString::new()];

    for directory in std::env::split_paths(&path) {
        for extension in &extensions {
            let mut candidate_name = OsString::from(name);
            candidate_name.push(extension);
            let candidate = directory.join(candidate_name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        true
    }
    #[cfg(not(any(unix, windows)))]
    {
        true
    }
}

#[cfg(target_os = "linux")]
fn terminal_availability(provider: &str) -> Result<(), String> {
    [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "kitty",
        "alacritty",
        "xterm",
    ]
    .iter()
    .any(|name| find_program(name).is_some())
    .then_some(())
    .ok_or_else(|| format!("{provider} CLI was found, but no supported terminal was found"))
}

#[cfg(target_os = "macos")]
fn terminal_availability(provider: &str) -> Result<(), String> {
    if Path::new("/usr/bin/open").is_file() {
        Ok(())
    } else {
        Err(format!("{provider} CLI was found, but macOS Terminal could not be opened"))
    }
}

#[cfg(windows)]
fn terminal_availability(_provider: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminal_availability(provider: &str) -> Result<(), String> {
    Err(format!("{provider} handoff is unavailable on this platform"))
}

#[cfg(target_os = "linux")]
fn launch_harness(program: &Path, cwd: &Path, args: &[OsString], provider: &str) -> Result<(), String> {
    let terminal = [
        "x-terminal-emulator",
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "kitty",
        "alacritty",
        "xterm",
    ]
    .iter()
    .find_map(|name| find_program(name).map(|path| ((*name).to_string(), path)))
    .ok_or_else(|| format!("{provider} CLI was found, but no supported terminal was found"))?;

    let (terminal_name, terminal_path) = terminal;
    let mut command = Command::new(terminal_path);
    command.current_dir(cwd);
    match terminal_name.as_str() {
        "gnome-terminal" => {
            command.arg("--");
        }
        "konsole" => {
            command.arg("--workdir").arg(cwd).arg("-e");
        }
        "xfce4-terminal" => {
            command.arg("--working-directory").arg(cwd).arg("-x");
        }
        "kitty" => {
            command.arg("--directory").arg(cwd);
        }
        "alacritty" => {
            command.arg("--working-directory").arg(cwd).arg("-e");
        }
        "x-terminal-emulator" | "xterm" => {
            command.arg("-e");
        }
        _ => unreachable!(),
    }
    command.arg(program).args(args).spawn().map(|_| ()).map_err(|error| {
        format!("Could not open {provider} in a terminal: {error}")
    })
}

#[cfg(target_os = "macos")]
fn launch_harness(program: &Path, cwd: &Path, args: &[OsString], provider: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let mut script = String::from("#!/bin/sh\ncd -- ");
    script.push_str(&shell_quote(&cwd.to_string_lossy()));
    script.push_str(" || exit 1\n");
    script.push_str(&shell_quote(&program.to_string_lossy()));
    for argument in args {
        script.push(' ');
        script.push_str(&shell_quote(&argument.to_string_lossy()));
    }
    script.push_str("\nresult=$?\nrm -- \"$0\"\nexit \"$result\"\n");

    let launcher_prefix = format!("gitmanager-{}-", provider.to_ascii_lowercase().replace(' ', "-"));
    let mut launcher = tempfile::Builder::new()
        .prefix(&launcher_prefix)
        .suffix(".command")
        .tempfile()
        .map_err(|error| format!("Could not create {provider} terminal launcher: {error}"))?;
    launcher
        .write_all(script.as_bytes())
        .map_err(|error| format!("Could not prepare {provider} terminal launcher: {error}"))?;
    launcher
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not prepare {provider} terminal launcher: {error}"))?;
    let (_, launcher_path) = launcher
        .keep()
        .map_err(|error| format!("Could not prepare {provider} terminal launcher: {}", error.error))?;

    let result = Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&launcher_path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {provider} in Terminal: {error}"));
    if result.is_err() {
        let _ = std::fs::remove_file(launcher_path);
    }
    result
}

#[cfg(target_os = "macos")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(windows)]
fn launch_harness(program: &Path, cwd: &Path, args: &[OsString], provider: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    if program
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
    {
        return launch_harness_script(program, cwd, args, provider);
    }

    Command::new(program)
        .current_dir(cwd)
        .args(args)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open {provider} in a console: {error}"))
}

#[cfg(windows)]
fn launch_harness_script(program: &Path, cwd: &Path, args: &[OsString], provider: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    const POWERSHELL_LAUNCH: &str = "Set-Location -LiteralPath $env:GITMANAGER_HARNESS_CWD; $harnessArgs = @(); for ($i = 0; $i -lt [int]$env:GITMANAGER_HARNESS_ARG_COUNT; $i++) { $harnessArgs += [System.Environment]::GetEnvironmentVariable(('GITMANAGER_HARNESS_ARG_' + $i)) }; & $env:GITMANAGER_HARNESS_PROGRAM @harnessArgs";

    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoLogo", "-NoProfile", "-NoExit", "-Command", POWERSHELL_LAUNCH])
        .env("GITMANAGER_HARNESS_PROGRAM", program)
        .env("GITMANAGER_HARNESS_CWD", cwd)
        .env("GITMANAGER_HARNESS_ARG_COUNT", args.len().to_string());
    for (index, argument) in args.iter().enumerate() {
        command.env(format!("GITMANAGER_HARNESS_ARG_{index}"), argument);
    }
    command
        .current_dir(cwd)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the {provider} command shim in a console: {error}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn launch_harness(_program: &Path, _cwd: &Path, _args: &[OsString], provider: &str) -> Result<(), String> {
    Err(format!("{provider} handoff is unavailable on this platform"))
}
