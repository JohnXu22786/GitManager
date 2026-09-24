use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn watch_paths(package_root: &Path) -> Vec<PathBuf> {
    let dot_git = package_root.join(".git");
    let Some(git_dir) = resolve_git_dir(package_root) else {
        return vec![package_root.to_path_buf()];
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
        git_dir.join("index"),
        common_dir.join("refs"),
        common_dir.join("packed-refs"),
        common_dir.join("reftable"),
    ]);
    paths.extend(tracked_paths(package_root));
    paths
}

pub fn cargo_watch_path(package_root: &Path, path: &Path) -> PathBuf {
    let path = path.strip_prefix(package_root).unwrap_or(path);
    representable_ancestor(path).unwrap_or_else(|| PathBuf::from("."))
}

fn tracked_paths(package_root: &Path) -> Vec<PathBuf> {
    let Some(repository_root) = git_command_path(package_root, &["rev-parse", "--show-toplevel"])
    else {
        return vec![package_root.to_path_buf()];
    };
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(&repository_root)
        .args(["ls-files", "--cached", "--full-name", "-z"])
        .output()
    else {
        return vec![package_root.to_path_buf()];
    };
    if !output.status.success() {
        return vec![package_root.to_path_buf()];
    }

    let mut paths = Vec::new();
    for path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let Some(path) = tracked_watch_path(&repository_root, path) else {
            continue;
        };
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

fn git_command_path(package_root: &Path, args: &[&str]) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(package_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = path_from_git_bytes(trim_line_ending(&output.stdout))?;
    (!path.as_os_str().is_empty()).then_some(path)
}

fn tracked_watch_path(package_root: &Path, path: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        return representable_ancestor(Path::new(std::ffi::OsStr::from_bytes(path)))
            .map(|path| package_root.join(path));
    }

    #[cfg(not(unix))]
    {
        representable_ancestor(Path::new(std::str::from_utf8(path).ok()?))
            .map(|path| package_root.join(path))
    }
}

fn representable_ancestor(path: &Path) -> Option<PathBuf> {
    let mut candidate = Some(path);
    while let Some(path) = candidate {
        if let Some(path_text) = path.to_str() {
            if !path_text.contains('\n') && !path_text.contains('\r') {
                if !path.as_os_str().is_empty() {
                    return Some(path.to_path_buf());
                }
                return Some(PathBuf::from("."));
            }
        }
        candidate = path.parent();
    }
    None
}

fn resolve_git_dir(package_root: &Path) -> Option<PathBuf> {
    if let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(package_root)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
    {
        if output.status.success() {
            if let Some(git_dir) = path_from_git_bytes(trim_line_ending(&output.stdout)) {
                if !git_dir.as_os_str().is_empty() {
                    return Some(resolve_path(package_root, &git_dir));
                }
            }
        }
    }

    resolve_local_git_dir(package_root)
}

fn resolve_local_git_dir(package_root: &Path) -> Option<PathBuf> {
    let dot_git = package_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }

    let git_file = fs::read(dot_git).ok()?;
    let git_dir = git_file.strip_prefix(b"gitdir:")?;
    let git_dir = git_dir
        .strip_prefix(b" ")
        .or_else(|| git_dir.strip_prefix(b"\t"))
        .unwrap_or(git_dir);
    let git_dir = trim_line_ending(git_dir);
    if git_dir.is_empty() {
        return None;
    }

    let git_dir = path_from_git_bytes(git_dir)?;
    Some(resolve_path(package_root, &git_dir))
}

fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let Ok(common_dir) = fs::read(git_dir.join("commondir")) else {
        return git_dir.to_path_buf();
    };
    let Some(common_dir) = path_from_git_bytes(trim_line_ending(&common_dir)) else {
        return git_dir.to_path_buf();
    };
    if common_dir.as_os_str().is_empty() {
        return git_dir.to_path_buf();
    }
    resolve_path(git_dir, &common_dir)
}

fn path_from_git_bytes(path: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        Some(PathBuf::from(OsString::from_vec(path.to_vec())))
    }

    #[cfg(not(unix))]
    {
        Some(PathBuf::from(std::str::from_utf8(path).ok()?))
    }
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    bytes.strip_suffix(b"\n").unwrap_or(bytes)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}
