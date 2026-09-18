use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_MAX_ENTRIES: usize = 20;

/// A single entry in the recent open history.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RecentEntry {
    pub path: String,
    pub name: String,
    pub last_opened: String,
}

/// Manages the list of recently opened repository paths.
/// Persists to a JSON file on disk.
pub struct RecentRepos {
    entries: Vec<RecentEntry>,
    max_entries: usize,
    file_path: PathBuf,
}

/// Return the final path component for either Unix or Windows-style paths.
///
/// Repository paths can be restored on a different platform than the one that
/// created them, so `std::path::Path` alone cannot recognize every separator.
pub(crate) fn path_name(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }

    let trimmed = path.trim_end_matches(|separator| {
        separator == '/' || separator == '\\'
    });
    if trimmed.is_empty() {
        return path.to_string();
    }

    trimmed
        .rsplit(|separator| separator == '/' || separator == '\\')
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

impl RecentRepos {
    /// Loads recent repos from the config file, or returns an empty list.
    pub fn load() -> Self {
        let file_path = get_config_path();
        let entries = load_entries(&file_path, DEFAULT_MAX_ENTRIES);
        RecentRepos {
            entries,
            max_entries: DEFAULT_MAX_ENTRIES,
            file_path,
        }
    }

    /// Loads from a specific path (for testing).
    #[allow(dead_code)]
    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_entries(&path, DEFAULT_MAX_ENTRIES);
        RecentRepos {
            entries,
            max_entries: DEFAULT_MAX_ENTRIES,
            file_path: path,
        }
    }

    /// Adds a path to the recent list. Moves to front if already exists.
    /// Automatically saves to disk and returns any persistence error.
    pub fn add(&mut self, path: &str) -> std::io::Result<()> {
        // Remove existing entry with same path (deduplicate)
        self.entries.retain(|e| e.path != path);

        let name = path_name(path);

        let last_opened = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.entries.insert(
            0,
            RecentEntry {
                path: path.to_string(),
                name,
                last_opened,
            },
        );

        self.entries.truncate(self.max_entries);
        self.save()
    }

    /// Removes an entry at the given index. Automatically saves to disk and
    /// returns any persistence error.
    pub fn remove(&mut self, index: usize) -> std::io::Result<()> {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.save()?
        }
        Ok(())
    }

    /// Returns a reference to all entries (most recent first).
    pub fn entries(&self) -> &[RecentEntry] {
        &self.entries
    }

    /// Returns the number of entries.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Persists entries to the JSON file on disk, returning any I/O error.
    pub fn save(&self) -> std::io::Result<()> {
        let content = serde_json::to_string_pretty(&self.entries).map_err(|error| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
        })?;

        if let Some(parent) = self.file_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        std::fs::write(&self.file_path, content)
    }
}

fn load_entries(path: &PathBuf, max_entries: usize) -> Vec<RecentEntry> {
    let mut entries: Vec<RecentEntry> = std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default();
    entries.truncate(max_entries);
    entries
}

fn get_config_path() -> PathBuf {
    #[cfg(windows)]
    {
        let config_dir = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("USERPROFILE")
                    .map(PathBuf::from)
                    .map(|path| path.join("AppData").join("Roaming"))
                    .filter(|path| path.is_absolute())
            })
            .unwrap_or_else(std::env::temp_dir);
        config_dir.join("GitManager").join("recent_repos.json")
    }

    #[cfg(not(windows))]
    {
        let config_dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".config"))
                    .filter(|path| path.is_absolute())
            })
            .unwrap_or_else(std::env::temp_dir);
        config_dir.join("GitManager").join("recent_repos.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(windows))]
    use std::ffi::{OsStr, OsString};
    use std::fs;
    #[cfg(not(windows))]
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);
    #[cfg(not(windows))]
    static CONFIG_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(not(windows))]
    struct EnvVarGuard {
        name: &'static str,
        previous: Option<OsString>,
    }

    #[cfg(not(windows))]
    impl EnvVarGuard {
        fn set(name: &'static str, value: Option<&OsStr>) -> Self {
            let previous = std::env::var_os(name);
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
            Self { name, previous }
        }
    }

    #[cfg(not(windows))]
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn temp_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        path.push(format!("gitmanager_test_recent_{}.json", id));
        path
    }

    #[test]
    fn test_add_deduplicates() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("/path/to/repo1").unwrap();
        repos.add("/path/to/repo2").unwrap();
        repos.add("/path/to/repo1").unwrap(); // duplicate, should move to front

        assert_eq!(repos.len(), 2);
        assert_eq!(repos.entries()[0].path, "/path/to/repo1");
        assert_eq!(repos.entries()[1].path, "/path/to/repo2");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_add_extracts_name() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("/home/user/projects/my-repo").unwrap();
        repos.add("C:\\Users\\test\\my-project\\").unwrap();

        assert_eq!(repos.entries()[0].name, "my-project");
        assert_eq!(repos.entries()[1].name, "my-repo");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_remove_by_index() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("/path/repo_a").unwrap();
        repos.add("/path/repo_b").unwrap();
        repos.add("/path/repo_c").unwrap();

        repos.remove(1).unwrap(); // remove repo_b

        assert_eq!(repos.len(), 2);
        assert_eq!(repos.entries()[0].path, "/path/repo_c");
        assert_eq!(repos.entries()[1].path, "/path/repo_a");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_remove_persists_to_disk() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        {
            let mut repos = RecentRepos::load_from(p.clone());
            repos.add("/path/repo_x").unwrap();
            repos.add("/path/repo_y").unwrap();
            repos.add("/path/repo_z").unwrap();
            repos.remove(1).unwrap(); // remove repo_y
        } // save() was called inside remove(), drop scope

        {
            let repos = RecentRepos::load_from(p.clone());
            assert_eq!(repos.len(), 2);
            assert_eq!(repos.entries()[0].path, "/path/repo_z");
            assert_eq!(repos.entries()[1].path, "/path/repo_x");
        }

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_remove_out_of_bounds() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("/path/repo").unwrap();
        repos.remove(5).unwrap(); // should be no-op

        assert_eq!(repos.len(), 1);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_load_empty_when_no_file() {
        let p = temp_path();
        let _ = fs::remove_file(&p); // ensure file doesn't exist

        let repos = RecentRepos::load_from(p.clone());
        assert!(repos.is_empty());

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_load_limits_entries_to_default_maximum() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let entries: Vec<RecentEntry> = (0..25)
            .map(|index| RecentEntry {
                path: format!("/repo/{}", index),
                name: format!("repo-{}", index),
                last_opened: format!("2025-01-01 00:00:{:02}", index),
            })
            .collect();
        fs::write(&p, serde_json::to_string(&entries).unwrap()).unwrap();

        let repos = RecentRepos::load_from(p.clone());

        assert_eq!(repos.len(), 20);
        assert_eq!(repos.entries()[0].path, "/repo/0");
        assert_eq!(repos.entries()[19].path, "/repo/19");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_save_reports_persistence_errors() {
        let parent = temp_path();
        let _ = fs::remove_file(&parent);
        fs::write(&parent, "not a directory").unwrap();
        let p = parent.join("recent_repos.json");
        let mut repos = RecentRepos::load_from(p);

        let error = repos
            .add("/path/to/repo")
            .expect_err("a failed save must be reported to the caller");

        assert!(!error.to_string().is_empty());
        let _ = fs::remove_file(&parent);
    }

    #[cfg(not(windows))]
    #[test]
    fn test_unix_config_path_uses_xdg_config_home() {
        let _lock = CONFIG_ENV_LOCK.lock().unwrap();
        let config_home = tempfile::tempdir().unwrap();
        let _appdata = EnvVarGuard::set("APPDATA", None);
        let _xdg_config_home =
            EnvVarGuard::set("XDG_CONFIG_HOME", Some(config_home.path().as_os_str()));

        assert_eq!(
            get_config_path(),
            config_home
                .path()
                .join("GitManager")
                .join("recent_repos.json")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_unix_config_path_falls_back_to_home_config() {
        let _lock = CONFIG_ENV_LOCK.lock().unwrap();
        let home = tempfile::tempdir().unwrap();
        let _appdata = EnvVarGuard::set("APPDATA", None);
        let _xdg_config_home = EnvVarGuard::set("XDG_CONFIG_HOME", None);
        let _home = EnvVarGuard::set("HOME", Some(home.path().as_os_str()));

        assert_eq!(
            get_config_path(),
            home.path()
                .join(".config")
                .join("GitManager")
                .join("recent_repos.json")
        );
    }

    #[test]
    fn test_persist_and_load() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        {
            let mut repos = RecentRepos::load_from(p.clone());
            repos.add("/path/to/persisted-repo").unwrap();
        } // repos dropped, but file stayed

        {
            let repos = RecentRepos::load_from(p.clone());
            assert_eq!(repos.len(), 1);
            assert_eq!(repos.entries()[0].path, "/path/to/persisted-repo");
        }

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_save_writes_valid_json() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        {
            let mut repos = RecentRepos::load_from(p.clone());
            repos.add("/valid/json/repo").unwrap();
        }

        let content = fs::read_to_string(&p).expect("File should exist");
        let parsed: Vec<RecentEntry> =
            serde_json::from_str(&content).expect("Should be valid JSON");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "/valid/json/repo");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_truncates_to_max() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());
        repos.max_entries = 3;

        repos.add("/repo/1").unwrap();
        repos.add("/repo/2").unwrap();
        repos.add("/repo/3").unwrap();
        repos.add("/repo/4").unwrap(); // should evict /repo/1

        assert_eq!(repos.len(), 3);
        assert_eq!(repos.entries()[0].path, "/repo/4");
        assert_eq!(repos.entries()[2].path, "/repo/2");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_add_empty_path() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("").unwrap(); // edge case: empty path

        assert_eq!(repos.len(), 1);
        assert_eq!(repos.entries()[0].name, "");

        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_save_and_reload_is_idempotent() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let paths = vec![
            "/alpha",
            "/beta",
            "/gamma",
        ];

        {
            let mut repos = RecentRepos::load_from(p.clone());
            for path in &paths {
                repos.add(path).unwrap();
            }
        }

        {
            let repos = RecentRepos::load_from(p.clone());
            assert_eq!(repos.len(), 3);
            // Most recently added last, so it's first
            assert_eq!(repos.entries()[0].path, "/gamma");
            assert_eq!(repos.entries()[1].path, "/beta");
            assert_eq!(repos.entries()[2].path, "/alpha");
        }

        {
            // Load again - should be exactly the same
            let repos = RecentRepos::load_from(p.clone());
            assert_eq!(repos.len(), 3);
        }

        let _ = fs::remove_file(&p);
    }
}
