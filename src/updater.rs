/// Auto-update module for Git Manager.
///
/// Checks GitHub Releases API for newer versions and notifies the user.
/// Also provides automatic download of update assets.

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A release asset from GitHub Releases API.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub content_type: String,
}

/// GitHub release response (only fields we need).
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// Parsed version number for comparison.
#[derive(Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

/// State of the update checker.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    /// No check has been performed yet.
    Idle,
    /// A check is currently in progress.
    Checking,
    /// Check completed, no update available.
    UpToDate,
    /// Check completed, an update is available.
    UpdateAvailable { latest_version: String, download_url: String, assets: Vec<ReleaseAsset> },
    /// Download is in progress with progress percentage (0.0 to 1.0).
    Downloading { progress: f32, file_name: String },
    /// Download completed successfully.
    Downloaded { file_path: String },
    /// Check failed with an error.
    Error(String),
}

/// Parse a semver version string like "0.1.0" or "v0.1.0" into a Version.
pub fn parse_version(s: &str) -> Option<Version> {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Version {
        major: parts[0].parse().ok()?,
        minor: parts[1].parse().ok()?,
        patch: parts[2].parse().ok()?,
    })
}

/// Compare two versions. Returns true if `current` is older than `latest`.
pub fn is_update_available(current: &Version, latest: &Version) -> bool {
    current.major < latest.major
        || (current.major == latest.major && current.minor < latest.minor)
        || (current.major == latest.major && current.minor == latest.minor && current.patch < latest.patch)
}

/// The GitHub API URL for checking the latest release.
const GITHUB_API_URL: &str = "https://api.github.com/repos/JohnXu22786/GitManager/releases/latest";

/// Check for updates by fetching the latest release from GitHub.
/// Returns the UpdateState.
pub fn check_for_update(current_version: &str) -> UpdateState {
    let current = match parse_version(current_version) {
        Some(v) => v,
        None => return UpdateState::Error(format!("Invalid current version: {}", current_version)),
    };

    let response = match ureq::get(GITHUB_API_URL)
        .set("User-Agent", "GitManager")
        .set("Accept", "application/json")
        .call()
    {
        Ok(r) => r,
        Err(e) => return UpdateState::Error(format!("Failed to check for updates: {}", e)),
    };

    let release: GitHubRelease = match response.into_json() {
        Ok(r) => r,
        Err(e) => return UpdateState::Error(format!("Failed to parse release info: {}", e)),
    };

    let latest = match parse_version(&release.tag_name) {
        Some(v) => v,
        None => return UpdateState::Error(format!("Invalid latest version tag: {}", release.tag_name)),
    };

    if is_update_available(&current, &latest) {
        UpdateState::UpdateAvailable {
            latest_version: release.tag_name,
            download_url: release.html_url,
            assets: release.assets,
        }
    } else {
        UpdateState::UpToDate
    }
}

/// Detect the current platform suffix used in release asset names.
/// Matches the naming convention from `.github/workflows/release.yml`.
fn get_platform_suffix() -> &'static str {
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "linux-x86_64"
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        "linux-aarch64"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "macos-x86_64"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "macos-aarch64"
    }
}

/// Find the download asset that matches the current platform.
/// Returns (download_url, file_name) if found.
pub fn find_asset_for_current_platform(assets: &[ReleaseAsset]) -> Option<(String, String)> {
    let suffix = get_platform_suffix();
    find_asset_by_suffix(assets, suffix)
}

/// Find a release asset whose name contains the given suffix.
/// Returns (download_url, file_name) if found.
fn find_asset_by_suffix(assets: &[ReleaseAsset], suffix: &str) -> Option<(String, String)> {
    for asset in assets {
        if asset.name.contains(suffix) {
            return Some((asset.browser_download_url.clone(), asset.name.clone()));
        }
    }
    None
}

/// The name of the binary inside the archive (platform-dependent).
fn binary_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "git_manager.exe"
    }
    #[cfg(not(target_os = "windows"))]
    {
        "git_manager"
    }
}

/// Extract the binary (`git_manager` or `git_manager.exe`) from a downloaded
/// release archive (.zip on Windows, .tar.gz on Unix).  The CI stores the
/// binary at an arbitrary depth inside the archive, so we search by file name.
/// Returns the path to the extracted binary (in a temp directory).
pub fn extract_binary_from_archive(archive_path: &Path) -> Result<PathBuf, String> {
    let name = archive_path.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        extract_binary_from_zip(archive_path)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_binary_from_tar_gz(archive_path)
    } else {
        Err(format!("Unsupported archive format: {}", archive_path.display()))
    }
}

fn extract_binary_from_zip(zip_path: &Path) -> Result<PathBuf, String> {
    extract_binary_from_zip_in(zip_path, &std::env::temp_dir())
}

fn zip_unix_mode_is_regular_file(mode: Option<u32>) -> bool {
    mode.map_or(true, |mode| matches!(mode & 0o170000, 0 | 0o100000))
}

fn extract_binary_from_zip_in(zip_path: &Path, temp_dir: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read zip: {}", e))?;
    let target_name = binary_name();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;
        let entry_path = entry.name().to_string();
        // Match the exact archive file name across both ZIP separators.
        if entry.is_file()
            && zip_unix_mode_is_regular_file(entry.unix_mode())
            && entry_path
                .rsplit(|separator| separator == '/' || separator == '\\')
                .next()
                == Some(target_name)
        {
            let mut dest_file = create_temp_file_in(temp_dir, "git_manager-", ".tmp")
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            std::io::copy(&mut entry, dest_file.as_file_mut())
                .map_err(|e| format!("Failed to extract binary: {}", e))?;
            let (_, dest_path) = dest_file
                .keep()
                .map_err(|e| format!("Failed to keep extracted binary: {}", e.error))?;
            return Ok(dest_path);
        }
    }

    Err(format!("Binary '{}' not found in zip archive", target_name))
}

fn extract_binary_from_tar_gz(tar_gz_path: &Path) -> Result<PathBuf, String> {
    extract_binary_from_tar_gz_in(tar_gz_path, &std::env::temp_dir())
}

fn extract_binary_from_tar_gz_in(tar_gz_path: &Path, temp_dir: &Path) -> Result<PathBuf, String> {
    let file = std::fs::File::open(tar_gz_path)
        .map_err(|e| format!("Failed to open tar.gz: {}", e))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let target_name = binary_name();

    for entry in archive.entries()
        .map_err(|e| format!("Failed to read tar entries: {}", e))?
    {
        let mut entry = entry
            .map_err(|e| format!("Failed to read tar entry: {}", e))?;
        let entry_path = entry.path()
            .map_err(|e| format!("Failed to get entry path: {}", e))?
            .to_string_lossy()
            .to_string();
        // Match the exact archive file name while ignoring directory nesting.
        if entry.header().entry_type().is_file()
            && entry_path.rsplit('/').next() == Some(target_name)
        {
            let mut dest_file = create_temp_file_in(temp_dir, "git_manager-", ".tmp")
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            std::io::copy(&mut entry, dest_file.as_file_mut())
                .map_err(|e| format!("Failed to extract binary: {}", e))?;
            let (_, dest_path) = dest_file
                .keep()
                .map_err(|e| format!("Failed to keep extracted binary: {}", e.error))?;
            return Ok(dest_path);
        }
    }

    Err(format!("Binary '{}' not found in tar.gz archive", target_name))
}

/// Create a self-update script that replaces the running binary, cleans up
/// temp files, and restarts.  Returns the path to the created script.
///
/// - **Windows**: writes a batch file that waits for the process to exit,
///   copies the new exe over the current one, deletes temp files, then starts.
/// - **Unix** (Linux/macOS): writes a shell script with the same logic.
pub fn create_self_update_script(new_binary: &Path, current_binary: &Path) -> Result<PathBuf, String> {
    let temp_dir = std::env::temp_dir();
    create_self_update_script_in(new_binary, current_binary, &temp_dir)
}

fn create_self_update_script_in(
    new_binary: &Path,
    current_binary: &Path,
    temp_dir: &Path,
) -> Result<PathBuf, String> {
    #[cfg(target_os = "windows")]
    {
        let mut script = create_temp_file_in(temp_dir, "update_git_manager-", ".bat")
            .map_err(|e| format!("Failed to create update script: {}", e))?;
        script
            .write_all(windows_self_update_script(new_binary, current_binary).as_bytes())
            .map_err(|e| format!("Failed to write update script: {}", e))?;
        let (_, script_path) = script
            .keep()
            .map_err(|e| format!("Failed to keep update script: {}", e.error))?;
        Ok(script_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        use std::os::unix::fs::PermissionsExt;

        let script_contents = unix_self_update_script(new_binary, current_binary)?;
        let mut script = create_temp_file_in(temp_dir, "update_git_manager-", ".sh")
            .map_err(|e| format!("Failed to create update script: {}", e))?;
        script
            .write_all(script_contents.as_bytes())
            .map_err(|e| format!("Failed to write update script: {}", e))?;
        script
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to make script executable: {}", e))?;
        let (_, script_path) = script
            .keep()
            .map_err(|e| format!("Failed to keep update script: {}", e.error))?;
        Ok(script_path)
    }
}

fn create_temp_file_in(
    temp_dir: &Path,
    prefix: &str,
    suffix: &str,
) -> std::io::Result<tempfile::NamedTempFile> {
    tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(temp_dir)
}

#[cfg(any(windows, test))]
fn windows_update_temp_path(current_binary: &Path) -> PathBuf {
    let mut temp_path = current_binary.as_os_str().to_os_string();
    temp_path.push(format!(".update-{}.tmp", std::process::id()));
    PathBuf::from(temp_path)
}

#[cfg(any(windows, test))]
fn windows_self_update_script(new_binary: &Path, current_binary: &Path) -> String {
    let temp_path = windows_update_temp_path(current_binary);

    format!(
        r#"@echo off
ping 127.0.0.1 -n 3 > nul
copy /Y {} {}
if errorlevel 1 goto update_failed
move /Y {} {}
if errorlevel 1 goto update_failed
del /F /Q {}
start "" {}
del "%~f0"
exit /b 0

:update_failed
del /F /Q {} >nul 2>&1
echo Git Manager update failed: could not replace executable; downloaded update retained. 1>&2
exit /b 1
"#,
        windows_batch_quote_path(new_binary),
        windows_batch_quote_path(&temp_path),
        windows_batch_quote_path(&temp_path),
        windows_batch_quote_path(current_binary),
        windows_batch_quote_path(new_binary),
        windows_batch_quote_path(current_binary),
        windows_batch_quote_path(&temp_path),
    )
}

#[cfg(not(target_os = "windows"))]
fn unix_executable_mode(path: &Path) -> Result<u32, String> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o777)
        .map_err(|e| format!("Failed to read current executable permissions: {}", e))
}

#[cfg(not(target_os = "windows"))]
fn unix_self_update_script(new_binary: &Path, current_binary: &Path) -> Result<String, String> {
    let current_mode = unix_executable_mode(current_binary)?;
    let mut script = String::from("#!/bin/sh\n");
    script.push_str(&shell_path_assignment("current_binary", current_binary));
    script.push_str(&shell_path_assignment("new_binary", new_binary));
    script.push_str(&format!(
        r#"sleep 2
update_temp=$(mktemp "${{current_binary}}.update.XXXXXX") || {{
    printf '%s\n' 'Git Manager update failed: could not stage executable; downloaded update retained.' >&2
    exit 1
}}
if ! cp -f "$new_binary" "$update_temp"; then
    printf '%s\n' 'Git Manager update failed: could not copy executable; downloaded update retained.' >&2
    rm -f "$update_temp"
    exit 1
fi
if ! chmod {:o} "$update_temp"; then
    printf '%s\n' 'Git Manager update failed: could not set executable permissions; downloaded update retained.' >&2
    rm -f "$update_temp"
    exit 1
fi
if ! mv -f "$update_temp" "$current_binary"; then
    printf '%s\n' 'Git Manager update failed: could not replace executable; downloaded update retained.' >&2
    rm -f "$update_temp"
    exit 1
fi
rm -f "$new_binary"
"$current_binary" &
rm -- "$0"
"#,
        current_mode,
    ));
    Ok(script)
}

#[cfg(not(target_os = "windows"))]
fn shell_path_assignment(variable: &str, path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    // Octal escapes keep arbitrary Unix path bytes out of the UTF-8 script.
    let escaped_path = path
        .as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| format!("\\0{:03o}", byte))
        .collect::<String>();
    // The sentinel prevents command substitution from stripping path newlines.
    format!("{variable}=$(printf '%bX' '{escaped_path}')\n{variable}=${{{variable}%X}}\n")
}

#[cfg(test)]
fn shell_quote_path(path: &Path) -> String {
    format!(
        "'{}'",
        path.to_str()
            .expect("test shell paths should be valid UTF-8")
            .replace('\'', "'\\''")
    )
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn windows_batch_quote_path(path: &Path) -> String {
    let mut quoted = String::from('"');
    for character in path.to_string_lossy().chars() {
        match character {
            '%' => quoted.push_str("%%"),
            '^' => quoted.push_str("^^"),
            '&' => quoted.push_str("^&"),
            '|' => quoted.push_str("^|"),
            '<' => quoted.push_str("^<"),
            '>' => quoted.push_str("^>"),
            '(' => quoted.push_str("^("),
            ')' => quoted.push_str("^)"),
            '!' => quoted.push_str("^!"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// Launch the generated self-update script and return any process-start error.
pub fn launch_self_update_script(script_path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let result = {
        let command = format!("call {}", windows_batch_quote_path(script_path));
        std::process::Command::new("cmd")
            .args(["/C", &command])
            .spawn()
    };

    #[cfg(not(target_os = "windows"))]
    let result = std::process::Command::new(script_path).spawn();

    result
        .map(|_| ())
        .map_err(|e| format!("Failed to launch update script: {}", e))
}

/// Get the default download directory path.
/// On Windows, uses %USERPROFILE%\Downloads. On Unix, uses ~/Downloads.
/// Falls back to current directory.
pub fn get_default_download_dir() -> String {
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let path = Path::new(&profile).join("Downloads");
            if path.exists() || std::fs::create_dir_all(&path).is_ok() {
                return path.to_string_lossy().to_string();
            }
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = Path::new(&home).join("Downloads");
            if path.exists() || std::fs::create_dir_all(&path).is_ok() {
                return path.to_string_lossy().to_string();
            }
        }
    }
    std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string())
}

/// Download a file from a URL to the specified path with progress tracking.
/// `progress` is updated from 0.0 to 1.0 as the download progresses.
/// Returns Ok(()) on success.
pub fn download_file_with_progress(
    url: &str,
    dest_path: &Path,
    progress: Arc<Mutex<f32>>,
) -> Result<(), String> {
    let response = ureq::get(url)
        .set("User-Agent", "GitManager")
        .call()
        .map_err(|e| format!("Download request failed: {}", e))?;

    // Get total content length for progress calculation
    let total_size: u64 = response
        .header("Content-Length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let mut reader = response.into_reader();
    stream_response_to_path(&mut reader, dest_path, total_size, &progress)?;

    if let Ok(mut prog) = progress.lock() {
        *prog = 1.0;
    }

    Ok(())
}

const DOWNLOAD_CHUNK_SIZE: usize = 8192;

fn stream_response_to_writer<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    total_size: u64,
    progress: &Arc<Mutex<f32>>,
) -> Result<(), String> {
    let mut downloaded: u64 = 0;
    let mut chunk = [0u8; DOWNLOAD_CHUNK_SIZE];

    loop {
        let bytes_read = reader
            .read(&mut chunk)
            .map_err(|e| format!("Download read error: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        writer
            .write_all(&chunk[..bytes_read])
            .map_err(|e| format!("Download write error: {}", e))?;
        downloaded += bytes_read as u64;

        if total_size > 0 {
            let p = downloaded as f32 / total_size as f32;
            if let Ok(mut prog) = progress.lock() {
                *prog = p.min(1.0);
            }
        }
    }

    Ok(())
}

fn stream_response_to_path<R: Read>(
    reader: &mut R,
    dest_path: &Path,
    total_size: u64,
    progress: &Arc<Mutex<f32>>,
) -> Result<(), String> {
    let mut dest_file = std::fs::File::create(dest_path)
        .map_err(|e| format!("Failed to write download to file: {}", e))?;
    stream_response_to_writer(reader, &mut dest_file, total_size, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RepeatingReader {
        remaining: usize,
    }

    impl Read for RepeatingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let bytes_read = buffer.len().min(self.remaining);
            buffer[..bytes_read].fill(b'x');
            self.remaining -= bytes_read;
            Ok(bytes_read)
        }
    }

    #[derive(Default)]
    struct CountingWriter {
        bytes_written: usize,
        largest_write: usize,
    }

    impl Write for CountingWriter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.bytes_written += buffer.len();
            self.largest_write = self.largest_write.max(buffer.len());
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn set_zip_member_unix_mode(archive_path: &Path, member_name: &str, mode: u32) {
        use std::io::{Seek, SeekFrom};

        let file = std::fs::File::open(archive_path).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let central_header_start = (0..archive.len())
            .find_map(|index| {
                let entry = archive.by_index(index).ok()?;
                (entry.name() == member_name).then(|| entry.central_header_start())
            })
            .expect("ZIP member should exist");
        drop(archive);

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(archive_path)
            .unwrap();
        // The central-directory external-attributes field starts 38 bytes into its header.
        file.seek(SeekFrom::Start(central_header_start + 38))
            .unwrap();
        file.write_all(&(mode << 16).to_le_bytes()).unwrap();
    }

    #[test]
    fn test_stream_response_to_writer_uses_bounded_chunks() {
        let body_size = 16 * 1024 * 1024;
        let mut reader = RepeatingReader {
            remaining: body_size,
        };
        let mut writer = CountingWriter::default();
        let progress = Arc::new(Mutex::new(0.0));

        stream_response_to_writer(&mut reader, &mut writer, body_size as u64, &progress).unwrap();

        assert_eq!(writer.bytes_written, body_size);
        assert!(writer.largest_write <= DOWNLOAD_CHUNK_SIZE);
        assert_eq!(*progress.lock().unwrap(), 1.0);
    }

    #[cfg(unix)]
    #[test]
    fn test_stream_response_to_path_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let dest_path = temp_dir.path().join("download.zip");
        std::fs::write(&dest_path, b"old download").unwrap();
        std::fs::set_permissions(&dest_path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let mut reader = std::io::Cursor::new(b"new download");
        let progress = Arc::new(Mutex::new(0.0));

        stream_response_to_path(
            &mut reader,
            &dest_path,
            b"new download".len() as u64,
            &progress,
        )
        .unwrap();

        assert_eq!(std::fs::read(&dest_path).unwrap(), b"new download");
        assert_eq!(
            std::fs::metadata(&dest_path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    // --- parse_version tests ---

    #[test]
    fn test_parse_version_standard() {
        let v = parse_version("0.1.0").unwrap();
        assert_eq!(v, Version { major: 0, minor: 1, patch: 0 });
    }

    #[test]
    fn test_parse_version_with_v_prefix() {
        let v = parse_version("v1.2.3").unwrap();
        assert_eq!(v, Version { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn test_parse_version_large_numbers() {
        let v = parse_version("10.20.30").unwrap();
        assert_eq!(v, Version { major: 10, minor: 20, patch: 30 });
    }

    #[test]
    fn test_parse_version_invalid_empty() {
        assert!(parse_version("").is_none());
    }

    #[test]
    fn test_parse_version_invalid_not_enough_parts() {
        assert!(parse_version("1.2").is_none());
    }

    #[test]
    fn test_parse_version_invalid_too_many_parts() {
        assert!(parse_version("1.2.3.4").is_none());
    }

    #[test]
    fn test_parse_version_invalid_non_numeric() {
        assert!(parse_version("1.a.3").is_none());
    }

    // --- is_update_available tests ---

    #[test]
    fn test_update_available_major() {
        let current = Version { major: 0, minor: 1, patch: 0 };
        let latest = Version { major: 1, minor: 0, patch: 0 };
        assert!(is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_available_minor() {
        let current = Version { major: 0, minor: 1, patch: 0 };
        let latest = Version { major: 0, minor: 2, patch: 0 };
        assert!(is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_available_patch() {
        let current = Version { major: 0, minor: 1, patch: 0 };
        let latest = Version { major: 0, minor: 1, patch: 1 };
        assert!(is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_not_available_same_version() {
        let current = Version { major: 0, minor: 1, patch: 0 };
        let latest = Version { major: 0, minor: 1, patch: 0 };
        assert!(!is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_not_available_current_newer_major() {
        let current = Version { major: 2, minor: 0, patch: 0 };
        let latest = Version { major: 1, minor: 0, patch: 0 };
        assert!(!is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_not_available_current_newer_minor() {
        let current = Version { major: 0, minor: 3, patch: 0 };
        let latest = Version { major: 0, minor: 2, patch: 0 };
        assert!(!is_update_available(&current, &latest));
    }

    #[test]
    fn test_update_not_available_current_newer_patch() {
        let current = Version { major: 0, minor: 1, patch: 5 };
        let latest = Version { major: 0, minor: 1, patch: 3 };
        assert!(!is_update_available(&current, &latest));
    }

    // --- GitHubRelease deserialization test ---

    #[test]
    fn test_github_release_deserialize() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/JohnXu22786/GitManager/releases/tag/v0.2.0"
        }"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.html_url, "https://github.com/JohnXu22786/GitManager/releases/tag/v0.2.0");
    }

    // --- Integration-style test for check_for_update with a mock via struct ---

    #[test]
    fn test_parse_version_very_high_is_valid() {
        // Verify that a very high version number can be parsed correctly.
        // This also validates that parse_version handles large numbers.
        let v = parse_version("99.99.99").unwrap();
        assert_eq!(v, Version { major: 99, minor: 99, patch: 99 });
    }

    #[test]
    fn test_parse_version_edge_cases() {
        // Test that "v" prefix with all zeros works
        let v = parse_version("v0.0.0").unwrap();
        assert_eq!(v, Version { major: 0, minor: 0, patch: 0 });

        // Test single digit versions
        let v = parse_version("1.0.0").unwrap();
        assert_eq!(v, Version { major: 1, minor: 0, patch: 0 });
    }

    #[test]
    fn test_check_for_update_invalid_version() {
        let state = check_for_update("not.a.version");
        match state {
            UpdateState::Error(msg) => assert!(msg.contains("Invalid current version")),
            _ => panic!("Expected Error state for invalid version"),
        }
    }

    // --- ReleaseAsset deserialization tests ---

    #[test]
    fn test_release_asset_deserialize() {
        let json = r#"{
            "name": "git-manager-0.2.0-windows-x86_64.zip",
            "browser_download_url": "https://github.com/JohnXu22786/GitManager/releases/download/v0.2.0/git-manager-0.2.0-windows-x86_64.zip",
            "content_type": "application/zip",
            "size": 1234567
        }"#;
        let asset: ReleaseAsset = serde_json::from_str(json).unwrap();
        assert_eq!(asset.name, "git-manager-0.2.0-windows-x86_64.zip");
        assert_eq!(asset.browser_download_url, "https://github.com/JohnXu22786/GitManager/releases/download/v0.2.0/git-manager-0.2.0-windows-x86_64.zip");
        assert_eq!(asset.content_type, "application/zip");
    }

    #[test]
    fn test_release_asset_deserialize_with_optional_size() {
        let json = r#"{
            "name": "git-manager-0.2.0-linux-x86_64.tar.gz",
            "browser_download_url": "https://github.com/JohnXu22786/GitManager/releases/download/v0.2.0/git-manager-0.2.0-linux-x86_64.tar.gz",
            "content_type": "application/gzip"
        }"#;
        let asset: ReleaseAsset = serde_json::from_str(json).unwrap();
        assert_eq!(asset.name, "git-manager-0.2.0-linux-x86_64.tar.gz");
        assert_eq!(asset.content_type, "application/gzip");
    }

    // --- find_asset_by_suffix tests ---

    #[test]
    fn test_find_asset_by_suffix_found() {
        let assets = vec![
            ReleaseAsset {
                name: "git-manager-0.2.0-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
            ReleaseAsset {
                name: "git-manager-0.2.0-windows-x86_64.zip".to_string(),
                browser_download_url: "https://example.com/windows.zip".to_string(),
                content_type: "application/zip".to_string(),
            },
            ReleaseAsset {
                name: "git-manager-0.2.0-macos-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/macos.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
        ];
        let result = find_asset_by_suffix(&assets, "windows-x86_64");
        assert!(result.is_some());
        let (url, name) = result.unwrap();
        assert_eq!(url, "https://example.com/windows.zip");
        assert_eq!(name, "git-manager-0.2.0-windows-x86_64.zip");
    }

    #[test]
    fn test_find_asset_by_suffix_macos() {
        let assets = vec![
            ReleaseAsset {
                name: "git-manager-0.2.0-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
            ReleaseAsset {
                name: "git-manager-0.2.0-macos-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/macos.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
        ];
        let result = find_asset_by_suffix(&assets, "macos-x86_64");
        assert!(result.is_some());
        let (url, name) = result.unwrap();
        assert_eq!(url, "https://example.com/macos.tar.gz");
        assert_eq!(name, "git-manager-0.2.0-macos-x86_64.tar.gz");
    }

    #[test]
    fn test_find_asset_by_suffix_not_found() {
        let assets = vec![
            ReleaseAsset {
                name: "git-manager-0.2.0-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
        ];
        let result = find_asset_by_suffix(&assets, "windows-x86_64");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_asset_by_suffix_empty() {
        let assets = vec![];
        let result = find_asset_by_suffix(&assets, "windows-x86_64");
        assert!(result.is_none());
    }

    #[test]
    fn test_find_asset_by_suffix_linux_aarch64() {
        let assets = vec![
            ReleaseAsset {
                name: "git-manager-0.2.0-linux-aarch64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux-arm64.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
            ReleaseAsset {
                name: "git-manager-0.2.0-linux-x86_64.tar.gz".to_string(),
                browser_download_url: "https://example.com/linux-x64.tar.gz".to_string(),
                content_type: "application/gzip".to_string(),
            },
        ];
        let result = find_asset_by_suffix(&assets, "linux-aarch64");
        assert!(result.is_some());
        let (url, _) = result.unwrap();
        assert_eq!(url, "https://example.com/linux-arm64.tar.gz");
    }

    // --- UpdateState Downloading / Downloaded tests ---

    #[test]
    fn test_update_state_downloading() {
        let state = UpdateState::Downloading { progress: 0.5, file_name: "test.zip".to_string() };
        match state {
            UpdateState::Downloading { progress, file_name } => {
                assert!((progress - 0.5).abs() < f32::EPSILON);
                assert_eq!(file_name, "test.zip");
            }
            _ => panic!("Expected Downloading variant"),
        }
    }

    #[test]
    fn test_update_state_downloaded() {
        let state = UpdateState::Downloaded { file_path: "C:\\Downloads\\test.zip".to_string() };
        match state {
            UpdateState::Downloaded { file_path } => {
                assert_eq!(file_path, "C:\\Downloads\\test.zip");
            }
            _ => panic!("Expected Downloaded variant"),
        }
    }

    #[test]
    fn test_update_state_downloading_full_progress() {
        let state = UpdateState::Downloading { progress: 1.0, file_name: "update.zip".to_string() };
        match state {
            UpdateState::Downloading { progress, .. } => {
                assert!((progress - 1.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected Downloading variant"),
        }
    }

    // --- GitHubRelease with assets deserialization test ---

    #[test]
    fn test_github_release_deserialize_with_assets() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "html_url": "https://github.com/JohnXu22786/GitManager/releases/tag/v0.2.0",
            "assets": [
                {
                    "name": "git-manager-0.2.0-windows-x86_64.zip",
                    "browser_download_url": "https://github.com/JohnXu22786/GitManager/releases/download/v0.2.0/git-manager-0.2.0-windows-x86_64.zip",
                    "content_type": "application/zip",
                    "size": 1234567
                }
            ]
        }"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.2.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, "git-manager-0.2.0-windows-x86_64.zip");
        assert!(release.assets[0].browser_download_url.contains("windows-x86_64"));
    }

    #[test]
    fn test_github_release_deserialize_without_assets() {
        // old-style response with no assets field should still work
        let json = r#"{
            "tag_name": "v0.1.0",
            "html_url": "https://github.com/JohnXu22786/GitManager/releases/tag/v0.1.0"
        }"#;
        let release: GitHubRelease = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v0.1.0");
        assert!(release.assets.is_empty(), "Should have empty assets when field is missing");
    }

    // --- get_default_download_dir tests ---

    #[test]
    fn test_get_default_download_dir_format() {
        let dir = get_default_download_dir();
        assert!(!dir.is_empty(), "Download dir should not be empty");
        // On any platform, should end with a meaningful name
        let path = std::path::Path::new(&dir);
        assert!(path.components().count() > 0, "Should be a valid path");
    }

    #[test]
    fn test_shell_path_escaping_preserves_special_characters() {
        let path = Path::new("/tmp/Update folder/it's $HOME/$(do-not-run).bin");

        assert_eq!(
            shell_quote_path(path),
            "'/tmp/Update folder/it'\\''s $HOME/$(do-not-run).bin'"
        );
    }

    #[test]
    fn test_windows_batch_path_escaping_preserves_metacharacters() {
        let path = Path::new(r"C:\Users\A & B\100% ready!\update.exe");

        assert_eq!(
            windows_batch_quote_path(path),
            r#""C:\Users\A ^& B\100%% ready^!\update.exe""#
        );
    }

    #[test]
    fn test_windows_update_script_stops_when_copy_fails() {
        let script = windows_self_update_script(
            Path::new(r"C:\Temp\update.exe"),
            Path::new(r"C:\Program Files\Git Manager\git_manager.exe"),
        );
        let copy_failure_check = script.find("if errorlevel 1 goto update_failed").unwrap();
        let move_pos = script.find("move /Y").unwrap();
        let move_failure_check = script[move_pos..]
            .find("if errorlevel 1 goto update_failed")
            .map(|index| index + move_pos)
            .unwrap();
        let cleanup = script.find("del /F /Q").unwrap();
        let relaunch = script.find("start \"\"").unwrap();
        let failure_handler = script.find(":update_failed").unwrap();

        assert!(copy_failure_check < move_pos);
        assert!(move_pos < move_failure_check);
        assert!(move_failure_check < cleanup);
        assert!(move_failure_check < relaunch);
        assert!(failure_handler > relaunch);
        assert!(script.contains(
            "could not replace executable; downloaded update retained."
        ));
        assert!(script.contains("exit /b 1"));
    }

    #[cfg(unix)]
    #[test]
    fn test_extract_zip_does_not_follow_predictable_temp_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().unwrap();
        let archive_path = temp_dir.path().join("update.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
        archive
            .start_file(binary_name(), zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"archive binary").unwrap();
        archive.finish().unwrap();

        let protected_path = temp_dir.path().join("protected-file");
        let predictable_path = temp_dir.path().join(binary_name());
        std::fs::write(&protected_path, b"leave this file alone").unwrap();
        symlink(&protected_path, &predictable_path).unwrap();

        let extracted_path = extract_binary_from_zip_in(&archive_path, temp_dir.path()).unwrap();

        assert_ne!(extracted_path, predictable_path);
        assert_eq!(
            std::fs::read(&protected_path).unwrap(),
            b"leave this file alone"
        );
        assert_eq!(std::fs::read(extracted_path).unwrap(), b"archive binary");
    }

    #[test]
    fn test_extract_zip_selects_exact_binary_file_across_separators() {
        for separator in ['/', '\\'] {
            let temp_dir = tempfile::tempdir().unwrap();
            let archive_path = temp_dir.path().join("update.zip");
            let mut archive = zip::ZipWriter::new(std::fs::File::create(&archive_path).unwrap());
            archive
                .add_directory(
                    format!("release/{}/", binary_name()),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            let special_member = format!("special/{}", binary_name());
            archive
                .start_file(&special_member, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(b"special file").unwrap();
            archive
                .start_file(
                    format!("bin/decoy_{}", binary_name()),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(b"decoy binary").unwrap();
            archive
                .add_symlink(
                    format!("links/{}", binary_name()),
                    "symlink target",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive
                .start_file(
                    format!("release{separator}{}", binary_name()),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            archive.write_all(b"exact binary").unwrap();
            archive.finish().unwrap();
            let fifo_mode = 0o010644;
            set_zip_member_unix_mode(&archive_path, &special_member, fifo_mode);

            let mut archive =
                zip::ZipArchive::new(std::fs::File::open(&archive_path).unwrap()).unwrap();
            let entry = archive.by_name(&special_member).unwrap();
            assert!(entry.is_file());
            assert_eq!(entry.unix_mode(), Some(fifo_mode));
            assert!(!zip_unix_mode_is_regular_file(entry.unix_mode()));
            drop(entry);
            drop(archive);

            let extracted_path =
                extract_binary_from_zip_in(&archive_path, temp_dir.path()).unwrap();

            assert_eq!(std::fs::read(extracted_path).unwrap(), b"exact binary");
        }
    }

    #[test]
    fn test_extract_tar_gz_selects_exact_binary_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let archive_path = temp_dir.path().join("update.tar.gz");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);

        let mut directory_header = tar::Header::new_gnu();
        directory_header.set_entry_type(tar::EntryType::Directory);
        directory_header.set_size(0);
        directory_header.set_mode(0o755);
        directory_header.set_cksum();
        archive
            .append_data(
                &mut directory_header,
                format!("release/{}", binary_name()),
                std::io::empty(),
            )
            .unwrap();

        let decoy = b"decoy binary";
        let mut header = tar::Header::new_gnu();
        header.set_size(decoy.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                format!("bin/decoy_{}", binary_name()),
                &decoy[..],
            )
            .unwrap();

        let mut symlink_header = tar::Header::new_gnu();
        symlink_header.set_entry_type(tar::EntryType::Symlink);
        symlink_header.set_size(0);
        symlink_header.set_mode(0o777);
        symlink_header.set_link_name("target").unwrap();
        symlink_header.set_cksum();
        archive
            .append_data(
                &mut symlink_header,
                format!("links/{}", binary_name()),
                std::io::empty(),
            )
            .unwrap();

        let exact = b"exact binary";
        let mut header = tar::Header::new_gnu();
        header.set_size(exact.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                format!("release/{}", binary_name()),
                &exact[..],
            )
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();

        let extracted_path = extract_binary_from_tar_gz_in(&archive_path, temp_dir.path()).unwrap();

        assert_eq!(std::fs::read(extracted_path).unwrap(), exact);
    }

    #[cfg(unix)]
    #[test]
    fn test_extract_tar_gz_does_not_follow_predictable_temp_symlink() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().unwrap();
        let archive_path = temp_dir.path().join("update.tar.gz");
        let archive_file = std::fs::File::create(&archive_path).unwrap();
        let encoder = flate2::write::GzEncoder::new(archive_file, flate2::Compression::default());
        let mut archive = tar::Builder::new(encoder);
        let contents = b"archive binary";
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive
            .append_data(&mut header, binary_name(), &contents[..])
            .unwrap();
        archive.into_inner().unwrap().finish().unwrap();

        let protected_path = temp_dir.path().join("protected-file");
        let predictable_path = temp_dir.path().join(binary_name());
        std::fs::write(&protected_path, b"leave this file alone").unwrap();
        symlink(&protected_path, &predictable_path).unwrap();

        let extracted_path = extract_binary_from_tar_gz_in(&archive_path, temp_dir.path()).unwrap();

        assert_ne!(extracted_path, predictable_path);
        assert_eq!(
            std::fs::read(&protected_path).unwrap(),
            b"leave this file alone"
        );
        assert_eq!(std::fs::read(extracted_path).unwrap(), contents);
    }

    #[cfg(unix)]
    #[test]
    fn test_create_update_script_does_not_follow_predictable_temp_symlink() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let temp_dir = tempfile::tempdir().unwrap();
        let new_binary = temp_dir.path().join("downloaded-update");
        let current_binary = temp_dir.path().join("current-binary");
        let protected_path = temp_dir.path().join("protected-file");
        let predictable_path = temp_dir.path().join("update_git_manager.sh");
        std::fs::write(&new_binary, b"updated binary").unwrap();
        std::fs::write(&current_binary, b"current binary").unwrap();
        std::fs::set_permissions(&current_binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&protected_path, b"leave this file alone").unwrap();
        std::fs::set_permissions(&protected_path, std::fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&protected_path, &predictable_path).unwrap();

        let script_path =
            create_self_update_script_in(&new_binary, &current_binary, temp_dir.path()).unwrap();

        assert_ne!(script_path, predictable_path);
        assert_eq!(
            std::fs::read(&protected_path).unwrap(),
            b"leave this file alone"
        );
        assert_eq!(
            std::fs::metadata(&protected_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
        assert_eq!(
            std::fs::metadata(&script_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert!(std::fs::read_to_string(script_path)
            .unwrap()
            .contains("#!/bin/sh"));
    }

    #[cfg(windows)]
    #[test]
    fn test_create_update_script_does_not_overwrite_predictable_temp_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let new_binary = temp_dir.path().join("downloaded-update.exe");
        let current_binary = temp_dir.path().join("current-binary.exe");
        let predictable_path = temp_dir.path().join("update_git_manager.bat");
        std::fs::write(&predictable_path, b"preserve this file").unwrap();

        let script_path =
            create_self_update_script_in(&new_binary, &current_binary, temp_dir.path()).unwrap();

        assert_ne!(script_path, predictable_path);
        assert!(script_path.extension().is_some_and(|extension| extension == "bat"));
        assert_eq!(
            std::fs::read(&predictable_path).unwrap(),
            b"preserve this file"
        );
        assert!(std::fs::read_to_string(script_path)
            .unwrap()
            .contains("@echo off"));
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_update_script_preserves_download_when_move_fails() {
        let temp_dir = tempfile::tempdir().unwrap();
        let new_binary = temp_dir.path().join("downloaded-update.exe");
        std::fs::write(&new_binary, b"downloaded update").unwrap();

        let current_binary = temp_dir.path().join("current-binary.bat");
        let launch_marker = temp_dir.path().join("old-binary-launched");
        let original_current = format!(
            "@echo off\necho launched > \"{}\"\n",
            launch_marker.display()
        );
        std::fs::write(&current_binary, &original_current).unwrap();
        let mut current_permissions = std::fs::metadata(&current_binary).unwrap().permissions();
        current_permissions.set_readonly(true);
        std::fs::set_permissions(&current_binary, current_permissions).unwrap();

        let script_path = temp_dir.path().join("update.bat");
        std::fs::write(
            &script_path,
            windows_self_update_script(&new_binary, &current_binary),
        )
        .unwrap();
        let output = std::process::Command::new("cmd")
            .args(["/C"])
            .arg(&script_path)
            .output()
            .unwrap();
        let mut current_permissions = std::fs::metadata(&current_binary).unwrap().permissions();
        current_permissions.set_readonly(false);
        std::fs::set_permissions(&current_binary, current_permissions).unwrap();

        assert!(
            !output.status.success(),
            "failed replacement must return failure"
        );
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("could not replace executable; downloaded update retained."));
        assert_eq!(std::fs::read(&new_binary).unwrap(), b"downloaded update");
        assert_eq!(
            std::fs::read(&current_binary).unwrap(),
            original_current.as_bytes()
        );
        for _ in 0..100 {
            if launch_marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!launch_marker.exists(), "old executable must not be relaunched");
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_update_script_preserves_files_after_partial_copy_failure() {
        let temp_dir = tempfile::tempdir().unwrap();
        let new_binary = temp_dir.path().join("downloaded-update.exe");
        let current_binary = temp_dir.path().join("current-binary.bat");
        let launch_marker = temp_dir.path().join("old-binary-launched");
        let failing_copy = temp_dir.path().join("partial-copy.bat");
        let copy_marker = temp_dir.path().join("partial-copy-ran");
        let script_path = temp_dir.path().join("update.bat");
        let staged_binary = windows_update_temp_path(&current_binary);
        let original_current = format!(
            "@echo off\necho launched > \"{}\"\n",
            launch_marker.display()
        );

        std::fs::write(&new_binary, b"downloaded update").unwrap();
        std::fs::write(&current_binary, &original_current).unwrap();
        assert!(!staged_binary.exists());
        std::fs::write(
            &failing_copy,
            format!(
                "@echo off\r\n> \"%~2\" echo partial\r\nif errorlevel 1 exit /b 2\r\nif not exist \"%~2\" exit /b 2\r\nfor %%A in (\"%~2\") do if %%~zA LEQ 0 exit /b 2\r\n> {} echo partial-written\r\nexit /b 1\r\n",
                windows_batch_quote_path(&copy_marker)
            ),
        )
        .unwrap();

        let generated_copy = format!(
            "copy /Y {} {}",
            windows_batch_quote_path(&new_binary),
            windows_batch_quote_path(&staged_binary),
        );
        let simulated_copy = format!(
            "call {} {} {}",
            windows_batch_quote_path(&failing_copy),
            windows_batch_quote_path(&new_binary),
            windows_batch_quote_path(&staged_binary),
        );
        let script = windows_self_update_script(&new_binary, &current_binary);
        assert!(script.contains(generated_copy.as_str()));
        std::fs::write(
            &script_path,
            script.replace(generated_copy.as_str(), simulated_copy.as_str()),
        )
        .unwrap();

        let output = std::process::Command::new("cmd")
            .args(["/C"])
            .arg(&script_path)
            .output()
            .unwrap();

        assert!(
            !output.status.success(),
            "failed replacement must return failure"
        );
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("could not replace executable; downloaded update retained."));
        assert_eq!(std::fs::read(&new_binary).unwrap(), b"downloaded update");
        assert_eq!(std::fs::read_to_string(&copy_marker).unwrap().trim(), "partial-written");
        assert_eq!(
            std::fs::read(&current_binary).unwrap(),
            original_current.as_bytes()
        );
        assert!(!staged_binary.exists(), "partial staging file must be removed");
        for _ in 0..100 {
            if launch_marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!launch_marker.exists(), "old executable must not be relaunched");
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_update_script_preserves_download_and_stops_when_copy_fails() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir(&bin_dir).unwrap();

        let new_binary = temp_dir.path().join("downloaded-update");
        std::fs::write(&new_binary, b"downloaded update").unwrap();

        let current_binary = temp_dir.path().join("current-binary");
        let launch_marker = temp_dir.path().join("old-binary-launched");
        let copy_marker = temp_dir.path().join("partial-copy-ran");
        let original_current = format!(
            "#!/bin/sh\nprintf launched > {}\n",
            shell_quote_path(&launch_marker)
        );
        std::fs::write(&current_binary, &original_current).unwrap();
        std::fs::set_permissions(&current_binary, std::fs::Permissions::from_mode(0o755)).unwrap();

        let failing_cp = bin_dir.join("cp");
        std::fs::write(
            &failing_cp,
            format!(
                "#!/bin/sh\nprintf partial > \"$3\"\ntest -s \"$3\" || exit 2\nprintf partial-written > {}\nexit 1\n",
                shell_quote_path(&copy_marker)
            ),
        )
        .unwrap();
        std::fs::set_permissions(&failing_cp, std::fs::Permissions::from_mode(0o755)).unwrap();

        let script_path = temp_dir.path().join("update.sh");
        let script = unix_self_update_script(&new_binary, &current_binary).unwrap();
        assert!(script.contains("chmod 755 \"$update_temp\""));
        std::fs::write(&script_path, script).unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let inherited_path = std::env::var_os("PATH").unwrap_or_default();
        let test_path = format!("{}:{}", bin_dir.display(), inherited_path.to_string_lossy());
        let output = std::process::Command::new(&script_path)
            .env("PATH", test_path)
            .output()
            .unwrap();

        assert!(
            !output.status.success(),
            "failed replacement must return failure"
        );
        assert!(String::from_utf8_lossy(&output.stderr)
            .contains("could not copy executable; downloaded update retained."));
        assert_eq!(std::fs::read(&new_binary).unwrap(), b"downloaded update");
        assert_eq!(std::fs::read_to_string(&copy_marker).unwrap(), "partial-written");
        assert_eq!(
            std::fs::read(&current_binary).unwrap(),
            original_current.as_bytes()
        );
        assert!(!std::fs::read_dir(temp_dir.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("current-binary.update.")
        }));
        assert!(!launch_marker.exists(), "old executable must not be relaunched");
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_update_script_replaces_binary_and_preserves_mode() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let new_binary = temp_dir.path().join("downloaded-update");
        let current_binary = temp_dir.path().join("current-binary");
        let launch_marker = temp_dir.path().join("updated-binary-launched");
        let new_contents = format!(
            "#!/bin/sh\nprintf updated > {}\n",
            shell_quote_path(&launch_marker)
        );
        std::fs::write(&new_binary, &new_contents).unwrap();
        std::fs::write(&current_binary, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&current_binary, std::fs::Permissions::from_mode(0o711)).unwrap();

        let script_path = temp_dir.path().join("update.sh");
        let script = unix_self_update_script(&new_binary, &current_binary).unwrap();
        assert!(script.contains("chmod 711 \"$update_temp\""));
        std::fs::write(&script_path, script).unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let output = std::process::Command::new(&script_path).output().unwrap();
        assert!(output.status.success());
        assert_eq!(std::fs::read(&current_binary).unwrap(), new_contents.as_bytes());
        assert_eq!(
            std::fs::metadata(&current_binary).unwrap().permissions().mode() & 0o777,
            0o711
        );
        assert!(!new_binary.exists(), "downloaded file is removed after success");
        assert!(launch_marker.exists(), "updated executable must be relaunched");
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_update_script_preserves_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let binary_dir = temp_dir
            .path()
            .join(OsString::from_vec(b"bin-\xff".to_vec()));
        std::fs::create_dir(&binary_dir).unwrap();

        let new_binary = binary_dir.join(OsString::from_vec(b"download-\xfe.bin".to_vec()));
        let current_binary = binary_dir.join(OsString::from_vec(b"current-\xfd\n".to_vec()));
        let launch_marker = temp_dir.path().join("updated-binary-launched");
        let new_contents = format!(
            "#!/bin/sh\nprintf updated > {}\n",
            shell_quote_path(&launch_marker)
        );
        std::fs::write(&new_binary, &new_contents).unwrap();
        std::fs::write(&current_binary, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&current_binary, std::fs::Permissions::from_mode(0o711)).unwrap();

        let script_path = temp_dir.path().join("update-non-utf8.sh");
        let script = unix_self_update_script(&new_binary, &current_binary).unwrap();
        assert!(!script.contains('\u{fffd}'));
        assert!(script.contains(r"\0377"));
        assert!(script.contains(r"\0376"));
        assert!(script.contains(r"\0375"));
        std::fs::write(&script_path, script).unwrap();
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let output = std::process::Command::new(&script_path).output().unwrap();
        assert!(
            output.status.success(),
            "update script failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            std::fs::read(&current_binary).unwrap(),
            new_contents.as_bytes()
        );
        assert_eq!(
            std::fs::metadata(&current_binary)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o711
        );
        assert!(
            !new_binary.exists(),
            "downloaded file is removed after success"
        );
        for _ in 0..100 {
            if launch_marker.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            launch_marker.exists(),
            "updated executable must be relaunched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_launch_self_update_script_reports_spawn_failure() {
        let result = launch_self_update_script(Path::new(
            "/path/that/does/not/exist/update_git_manager.sh",
        ));

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Failed to launch update script"));
    }
}
