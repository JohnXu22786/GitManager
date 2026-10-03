use crate::tasks::TaskRecord;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CODEX_PROVIDER_REF: &str = "codex";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeCapability {
    Unsupported,
    UserSelectsSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HarnessCapabilities {
    pub can_start: bool,
    pub tracks_sessions: bool,
    pub resume: ResumeCapability,
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
    fn start(&self, task: &TaskRecord) -> Result<(), String>;
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
        }
    }

    fn availability(&self) -> HarnessAvailability {
        if find_program("codex").is_none() {
            return HarnessAvailability {
                available: false,
                message: "Codex CLI was not found in PATH".into(),
            };
        }

        match terminal_availability() {
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

    fn start(&self, task: &TaskRecord) -> Result<(), String> {
        let codex = find_program("codex")
            .ok_or_else(|| "Codex CLI was not found in PATH".to_string())?;
        let worktree = Path::new(&task.worktree_path);
        if !worktree.is_dir() {
            return Err("The task worktree is unavailable".into());
        }

        launch_codex(
            &codex,
            worktree,
            &[
                OsString::from("--cd"),
                OsString::from(task.worktree_path.as_str()),
                OsString::from("--"),
                OsString::from(task.title.as_str()),
            ],
        )
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
        launch_codex(&codex, worktree, &args)
    }
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
fn terminal_availability() -> Result<(), String> {
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
    .ok_or_else(|| "Codex CLI was found, but no supported terminal was found".into())
}

#[cfg(target_os = "macos")]
fn terminal_availability() -> Result<(), String> {
    if Path::new("/usr/bin/open").is_file() {
        Ok(())
    } else {
        Err("Codex CLI was found, but macOS Terminal could not be opened".into())
    }
}

#[cfg(windows)]
fn terminal_availability() -> Result<(), String> {
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn terminal_availability() -> Result<(), String> {
    Err("Codex handoff is unavailable on this platform".into())
}

#[cfg(target_os = "linux")]
fn launch_codex(program: &Path, cwd: &Path, codex_args: &[OsString]) -> Result<(), String> {
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
    .ok_or_else(|| "Codex CLI was found, but no supported terminal was found".to_string())?;

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
    command.arg(program).args(codex_args).spawn().map(|_| ()).map_err(|error| {
        format!("Could not open Codex in a terminal: {error}")
    })
}

#[cfg(target_os = "macos")]
fn launch_codex(program: &Path, cwd: &Path, codex_args: &[OsString]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let mut script = String::from("#!/bin/sh\ncd -- ");
    script.push_str(&shell_quote(&cwd.to_string_lossy()));
    script.push_str(" || exit 1\n");
    script.push_str(&shell_quote(&program.to_string_lossy()));
    for argument in codex_args {
        script.push(' ');
        script.push_str(&shell_quote(&argument.to_string_lossy()));
    }
    script.push_str("\nresult=$?\nrm -- \"$0\"\nexit \"$result\"\n");

    let mut launcher = tempfile::Builder::new()
        .prefix("gitmanager-codex-")
        .suffix(".command")
        .tempfile()
        .map_err(|error| format!("Could not create Codex terminal launcher: {error}"))?;
    launcher
        .write_all(script.as_bytes())
        .map_err(|error| format!("Could not prepare Codex terminal launcher: {error}"))?;
    launcher
        .as_file()
        .set_permissions(std::fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("Could not prepare Codex terminal launcher: {error}"))?;
    let (_, launcher_path) = launcher
        .keep()
        .map_err(|error| format!("Could not prepare Codex terminal launcher: {}", error.error))?;

    let result = Command::new("/usr/bin/open")
        .args(["-a", "Terminal"])
        .arg(&launcher_path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open Codex in Terminal: {error}"));
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
fn launch_codex(program: &Path, cwd: &Path, codex_args: &[OsString]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    if program
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
    {
        return launch_codex_script(program, cwd, codex_args);
    }

    Command::new(program)
        .current_dir(cwd)
        .args(codex_args)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open Codex in a console: {error}"))
}

#[cfg(windows)]
fn launch_codex_script(program: &Path, cwd: &Path, codex_args: &[OsString]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_CONSOLE: u32 = 0x00000010;
    const POWERSHELL_LAUNCH: &str = "Set-Location -LiteralPath $env:GITMANAGER_CODEX_CWD; $codexArgs = @(); for ($i = 0; $i -lt [int]$env:GITMANAGER_CODEX_ARG_COUNT; $i++) { $codexArgs += [System.Environment]::GetEnvironmentVariable(('GITMANAGER_CODEX_ARG_' + $i)) }; & $env:GITMANAGER_CODEX_PROGRAM @codexArgs";

    let mut command = Command::new("powershell.exe");
    command
        .args(["-NoLogo", "-NoProfile", "-NoExit", "-Command", POWERSHELL_LAUNCH])
        .env("GITMANAGER_CODEX_PROGRAM", program)
        .env("GITMANAGER_CODEX_CWD", cwd)
        .env("GITMANAGER_CODEX_ARG_COUNT", codex_args.len().to_string());
    for (index, argument) in codex_args.iter().enumerate() {
        command.env(format!("GITMANAGER_CODEX_ARG_{index}"), argument);
    }
    command
        .current_dir(cwd)
        .creation_flags(CREATE_NEW_CONSOLE)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Could not open the Codex command shim in a console: {error}"))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn launch_codex(_program: &Path, _cwd: &Path, _codex_args: &[OsString]) -> Result<(), String> {
    Err("Codex handoff is unavailable on this platform".into())
}
