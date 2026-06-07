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
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("Failed to open zip: {}", e))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read zip: {}", e))?;
    let target_name = binary_name();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("Failed to read zip entry {}: {}", i, e))?;
        let entry_path = entry.name().to_string();
        // Match by file name (ignore directory nesting)
        if entry_path.ends_with(target_name) {
            // Create a temp file for the extracted binary
            let temp_dir = std::env::temp_dir();
            let dest_path = temp_dir.join(target_name);
            let mut dest_file = std::fs::File::create(&dest_path)
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            std::io::copy(&mut entry, &mut dest_file)
                .map_err(|e| format!("Failed to extract binary: {}", e))?;
            return Ok(dest_path);
        }
    }

    Err(format!("Binary '{}' not found in zip archive", target_name))
}

fn extract_binary_from_tar_gz(tar_gz_path: &Path) -> Result<PathBuf, String> {
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
        // Match by file name (ignore directory nesting)
        if entry_path.ends_with(target_name) {
            let temp_dir = std::env::temp_dir();
            let dest_path = temp_dir.join(target_name);
            let mut dest_file = std::fs::File::create(&dest_path)
                .map_err(|e| format!("Failed to create temp file: {}", e))?;
            std::io::copy(&mut entry, &mut dest_file)
                .map_err(|e| format!("Failed to extract binary: {}", e))?;
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
    #[cfg(target_os = "windows")]
    {
        let script_path = temp_dir.join("update_git_manager.bat");
        let mut script = std::fs::File::create(&script_path)
            .map_err(|e| format!("Failed to create update script: {}", e))?;
        write!(script, r#"@echo off
ping 127.0.0.1 -n 3 > nul
copy /Y "{}" "{}"
del /F /Q "{}"
start "" "{}"
del "%~f0"
"#,
            new_binary.display(),
            current_binary.display(),
            new_binary.display(),
            current_binary.display(),
        ).map_err(|e| format!("Failed to write update script: {}", e))?;
        Ok(script_path)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let script_path = temp_dir.join("update_git_manager.sh");
        let mut script = std::fs::File::create(&script_path)
            .map_err(|e| format!("Failed to create update script: {}", e))?;
        write!(script, r#"#!/bin/sh
sleep 2
cp -f "{}" "{}"
chmod +x "{}"
rm -f "{}"
"{}" &
rm -- "$0"
"#,
            new_binary.display(),
            current_binary.display(),
            current_binary.display(),
            new_binary.display(),
            current_binary.display(),
        ).map_err(|e| format!("Failed to write update script: {}", e))?;
        // Mark script as executable on Unix
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to make script executable: {}", e))?;
        Ok(script_path)
    }
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

    // Read response body with progress tracking
    let mut reader = response.into_reader();
    let mut buffer = Vec::new();
    let mut downloaded: u64 = 0;
    let mut chunk = [0u8; 8192];

    loop {
        let bytes_read = reader
            .read(&mut chunk)
            .map_err(|e| format!("Download read error: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);
        downloaded += bytes_read as u64;

        // Update progress
        if total_size > 0 {
            let p = downloaded as f32 / total_size as f32;
            if let Ok(mut prog) = progress.lock() {
                *prog = p.min(1.0);
            }
        }
    }

    // Write downloaded data to file
    std::fs::write(dest_path, &buffer)
        .map_err(|e| format!("Failed to write download to file: {}", e))?;

    // Mark as complete
    if let Ok(mut prog) = progress.lock() {
        *prog = 1.0;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
