use std::fs;
use std::path::{Path, PathBuf};

pub fn watch_paths(package_root: &Path) -> Vec<PathBuf> {
    let dot_git = package_root.join(".git");
    let Some(git_dir) = resolve_git_dir(package_root) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    if !dot_git.is_dir() {
        paths.push(dot_git);
    }
    let common_dir_file = git_dir.join("commondir");
    if common_dir_file.is_file() {
        paths.push(common_dir_file);
    }

    let common_dir = resolve_common_dir(&git_dir);

    paths.extend([
        git_dir.join("HEAD"),
        common_dir.join("refs"),
        common_dir.join("packed-refs"),
        common_dir.join("reftable"),
    ]);
    paths
}

fn resolve_git_dir(package_root: &Path) -> Option<PathBuf> {
    let dot_git = package_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let git_file = fs::read_to_string(dot_git).ok()?;
    let git_dir = git_file.trim().strip_prefix("gitdir:")?.trim();
    if git_dir.is_empty() {
        return None;
    }

    let git_dir = PathBuf::from(git_dir);
    Some(resolve_path(package_root, &git_dir))
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(common_dir) = fs::read_to_string(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let common_dir = PathBuf::from(common_dir.trim());
    resolve_path(git_dir, &common_dir)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
