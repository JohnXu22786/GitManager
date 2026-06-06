/// Auto-update module for Git Manager.
///
/// Checks GitHub Releases API for newer versions and notifies the user.

use serde::Deserialize;

/// GitHub release response (only fields we need).
#[derive(Debug, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub html_url: String,
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
    UpdateAvailable { latest_version: String, download_url: String },
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
        }
    } else {
        UpdateState::UpToDate
    }
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
}
