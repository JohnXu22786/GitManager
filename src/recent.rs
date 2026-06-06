use chrono::Local;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

impl RecentRepos {
    /// Loads recent repos from the config file, or returns an empty list.
    pub fn load() -> Self {
        let file_path = get_config_path();
        let entries = load_entries(&file_path);
        RecentRepos {
            entries,
            max_entries: 20,
            file_path,
        }
    }

    /// Loads from a specific path (for testing).
    #[allow(dead_code)]
    pub fn load_from(path: PathBuf) -> Self {
        let entries = load_entries(&path);
        RecentRepos {
            entries,
            max_entries: 20,
            file_path: path,
        }
    }

    /// Adds a path to the recent list. Moves to front if already exists.
    /// Automatically saves to disk.
    pub fn add(&mut self, path: &str) {
        // Remove existing entry with same path (deduplicate)
        self.entries.retain(|e| e.path != path);

        let name = PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());

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
        self.save();
    }

    /// Removes an entry at the given index. Automatically saves to disk.
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
            self.save();
        }
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

    /// Persists entries to the JSON file on disk.
    pub fn save(&self) {
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(&self.entries) {
            let _ = std::fs::write(&self.file_path, content);
        }
    }
}

fn load_entries(path: &PathBuf) -> Vec<RecentEntry> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn get_config_path() -> PathBuf {
    // Use %APPDATA% on Windows
    if let Ok(appdata) = std::env::var("APPDATA") {
        let mut path = PathBuf::from(appdata);
        path.push("GitManager");
        path.push("recent_repos.json");
        path
    } else {
        // Fallback to a file in the working directory
        PathBuf::from("recent_repos.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

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

        repos.add("/path/to/repo1");
        repos.add("/path/to/repo2");
        repos.add("/path/to/repo1"); // duplicate, should move to front

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

        repos.add("/home/user/projects/my-repo");

        assert_eq!(repos.entries()[0].name, "my-repo");
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn test_remove_by_index() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        let mut repos = RecentRepos::load_from(p.clone());

        repos.add("/path/repo_a");
        repos.add("/path/repo_b");
        repos.add("/path/repo_c");

        repos.remove(1); // remove repo_b

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
            repos.add("/path/repo_x");
            repos.add("/path/repo_y");
            repos.add("/path/repo_z");
            repos.remove(1); // remove repo_y
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

        repos.add("/path/repo");
        repos.remove(5); // should be no-op

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
    fn test_persist_and_load() {
        let p = temp_path();
        let _ = fs::remove_file(&p);
        {
            let mut repos = RecentRepos::load_from(p.clone());
            repos.add("/path/to/persisted-repo");
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
            repos.add("/valid/json/repo");
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

        repos.add("/repo/1");
        repos.add("/repo/2");
        repos.add("/repo/3");
        repos.add("/repo/4"); // should evict /repo/1

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

        repos.add(""); // edge case: empty path

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
                repos.add(path);
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
