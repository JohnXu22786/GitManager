#[path = "../build_support/git_metadata.rs"]
mod git_metadata;
#[path = "../build_support/version_info.rs"]
mod version_info;

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn watches_head_and_shared_refs_for_worktree_git_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("worktree");
    let common_git_dir = temp_dir.path().join("repository/.git");
    let worktree_git_dir = common_git_dir.join("worktrees/worktree");

    fs::create_dir_all(&package_root).unwrap();
    fs::create_dir_all(common_git_dir.join("refs/tags")).unwrap();
    fs::create_dir_all(common_git_dir.join("reftable")).unwrap();
    fs::create_dir_all(&worktree_git_dir).unwrap();
    fs::write(
        package_root.join(".git"),
        "gitdir: ../repository/.git/worktrees/worktree\n",
    )
    .unwrap();
    fs::write(worktree_git_dir.join("commondir"), "../..\n").unwrap();
    fs::write(worktree_git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(worktree_git_dir.join("index"), "test-index\n").unwrap();
    fs::write(common_git_dir.join("refs/tags/v1"), "test-tag\n").unwrap();
    fs::write(common_git_dir.join("packed-refs"), "# packed refs\n").unwrap();
    fs::write(
        common_git_dir.join("reftable/tables.list"),
        "table-00000001\n",
    )
    .unwrap();

    let actual: Vec<_> = git_metadata::watch_paths(&package_root)
        .iter()
        .map(|path| fs::canonicalize(path).unwrap())
        .collect();
    let expected = [
        package_root.join(".git").canonicalize().unwrap(),
        worktree_git_dir.join("commondir").canonicalize().unwrap(),
        common_git_dir
            .join("worktrees/worktree/HEAD")
            .canonicalize()
            .unwrap(),
        worktree_git_dir.join("index").canonicalize().unwrap(),
        common_git_dir.join("refs").canonicalize().unwrap(),
        common_git_dir.join("packed-refs").canonicalize().unwrap(),
        common_git_dir.join("reftable").canonicalize().unwrap(),
    ];

    assert!(actual.starts_with(&expected));
}

#[test]
fn watches_metadata_in_a_regular_git_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("repository");
    let git_dir = package_root.join(".git");
    fs::create_dir_all(git_dir.join("refs")).unwrap();
    fs::create_dir_all(git_dir.join("reftable")).unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(git_dir.join("packed-refs"), "# packed refs\n").unwrap();
    fs::write(git_dir.join("reftable/tables.list"), "table-00000001\n").unwrap();

    let actual = git_metadata::watch_paths(Path::new(&package_root));

    assert!(actual.starts_with(&[
        git_dir.join("HEAD"),
        git_dir.join("index"),
        git_dir.join("refs"),
        git_dir.join("packed-refs"),
        git_dir.join("reftable"),
    ]));
}

#[test]
fn watches_the_index_and_every_tracked_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("repository");
    fs::create_dir_all(package_root.join("src")).unwrap();
    run_git(&package_root, &["init", "--quiet"]);
    run_git(&package_root, &["config", "user.name", "Test User"]);
    run_git(&package_root, &["config", "user.email", "test@example.com"]);
    run_git(&package_root, &["config", "commit.gpgsign", "false"]);
    fs::write(package_root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(package_root.join("README.md"), "tracked\n").unwrap();
    run_git(&package_root, &["add", "src/main.rs", "README.md"]);
    run_git(&package_root, &["commit", "--quiet", "-m", "initial"]);

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&package_root.join(".git/index")));
    assert!(actual.contains(&package_root.join("src/main.rs")));
    assert!(actual.contains(&package_root.join("README.md")));
}

#[cfg(unix)]
#[test]
fn watches_the_parent_of_a_non_utf8_tracked_file() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("repository");
    let source_dir = package_root.join("src");
    fs::create_dir_all(&source_dir).unwrap();
    run_git(&package_root, &["init", "--quiet"]);
    run_git(&package_root, &["config", "user.name", "Test User"]);
    run_git(&package_root, &["config", "user.email", "test@example.com"]);
    run_git(&package_root, &["config", "commit.gpgsign", "false"]);
    fs::write(
        source_dir.join(OsString::from_vec(b"non-utf8-\xff.rs".to_vec())),
        "tracked\n",
    )
    .unwrap();
    run_git(&package_root, &["add", "--all"]);
    run_git(&package_root, &["commit", "--quiet", "-m", "initial"]);

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&source_dir));
}

#[cfg(unix)]
#[test]
fn watches_the_parent_of_a_newline_tracked_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("repository");
    let source_dir = package_root.join("src");
    fs::create_dir_all(&source_dir).unwrap();
    run_git(&package_root, &["init", "--quiet"]);
    run_git(&package_root, &["config", "user.name", "Test User"]);
    run_git(&package_root, &["config", "user.email", "test@example.com"]);
    run_git(&package_root, &["config", "commit.gpgsign", "false"]);
    fs::write(source_dir.join("line\nbreak.rs"), "tracked\n").unwrap();
    run_git(&package_root, &["add", "--all"]);
    run_git(&package_root, &["commit", "--quiet", "-m", "initial"]);

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&source_dir));
}

#[test]
fn cargo_watch_path_uses_a_safe_ancestor() {
    let package_root = Path::new("/worktree");
    let tracked_path = package_root.join("src/line\nbreak.rs");
    let external_git_path = package_root.join("../repo\nname/.git/index");

    assert_eq!(
        git_metadata::cargo_watch_path(package_root, package_root),
        Path::new(".")
    );
    assert_eq!(
        git_metadata::cargo_watch_path(package_root, &tracked_path),
        Path::new("src")
    );
    assert_eq!(
        git_metadata::cargo_watch_path(package_root, &external_git_path),
        Path::new("..")
    );
}

#[test]
fn watches_package_root_when_git_metadata_is_unavailable() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("source-archive");
    fs::create_dir_all(&package_root).unwrap();
    fs::write(package_root.join("README.md"), "source\n").unwrap();

    assert_eq!(git_metadata::watch_paths(&package_root), vec![package_root]);
}

#[test]
fn watches_package_root_when_git_file_cannot_be_resolved() {
    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("broken-worktree");
    fs::create_dir_all(&package_root).unwrap();
    fs::write(package_root.join(".git"), "gitdir: missing-git-dir\n").unwrap();

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&package_root));
}

#[test]
fn watches_parent_repository_metadata_for_a_nested_package() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repository = temp_dir.path().join("repository");
    let package_root = repository.join("nested/package");
    fs::create_dir_all(&package_root).unwrap();
    fs::write(repository.join("sibling.txt"), "tracked outside package\n").unwrap();
    run_git(&repository, &["init", "--quiet"]);
    run_git(&repository, &["config", "user.name", "Test User"]);
    run_git(&repository, &["config", "user.email", "test@example.com"]);
    run_git(&repository, &["config", "commit.gpgsign", "false"]);
    fs::write(package_root.join("Cargo.toml"), "[package]\n").unwrap();
    run_git(
        &repository,
        &["add", "nested/package/Cargo.toml", "sibling.txt"],
    );
    run_git(&repository, &["commit", "--quiet", "-m", "initial"]);

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&repository.join(".git/index")));
    assert!(actual.contains(&package_root.join("Cargo.toml")));
    assert!(actual.contains(&repository.join("sibling.txt")));
}

#[test]
fn watches_tracked_files_in_a_real_linked_worktree() {
    let temp_dir = tempfile::tempdir().unwrap();
    let repository = temp_dir.path().join("repository");
    let worktree = temp_dir.path().join("linked-worktree");
    fs::create_dir_all(&repository).unwrap();
    run_git(&repository, &["init", "--quiet"]);
    run_git(&repository, &["config", "user.name", "Test User"]);
    run_git(&repository, &["config", "user.email", "test@example.com"]);
    run_git(&repository, &["config", "commit.gpgsign", "false"]);
    fs::write(repository.join("README.md"), "tracked\n").unwrap();
    run_git(&repository, &["add", "README.md"]);
    run_git(&repository, &["commit", "--quiet", "-m", "initial"]);
    run_git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    let git_file = fs::read_to_string(worktree.join(".git")).unwrap();
    let git_dir = Path::new(git_file.trim().strip_prefix("gitdir:").unwrap().trim());
    let git_dir = if git_dir.is_absolute() {
        git_dir.to_path_buf()
    } else {
        worktree.join(git_dir)
    };
    let actual = git_metadata::watch_paths(&worktree);

    assert!(actual.contains(&git_dir.join("index")));
    assert!(actual.contains(&worktree.join("README.md")));
}

#[cfg(unix)]
#[test]
fn watches_linked_metadata_under_a_non_utf8_repository_path() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let repository = temp_dir
        .path()
        .join(OsString::from_vec(b"repo-\xff-name".to_vec()));
    let worktree = temp_dir.path().join("linked-worktree\r\n");
    fs::create_dir_all(&repository).unwrap();
    run_git(&repository, &["init", "--quiet"]);
    run_git(&repository, &["config", "user.name", "Test User"]);
    run_git(&repository, &["config", "user.email", "test@example.com"]);
    run_git(&repository, &["config", "commit.gpgsign", "false"]);
    fs::write(repository.join("README.md"), "tracked\n").unwrap();
    run_git(&repository, &["add", "README.md"]);
    run_git(&repository, &["commit", "--quiet", "-m", "initial"]);
    run_git(
        &repository,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree.to_str().unwrap(),
        ],
    );

    let actual = git_metadata::watch_paths(&worktree);
    let index = actual
        .iter()
        .find(|path| path.file_name().is_some_and(|name| name == "index"))
        .expect("linked worktree index should be watched");
    let cargo_path = git_metadata::cargo_watch_path(&worktree, index);

    assert!(!index.starts_with(&worktree));
    assert!(actual.contains(&worktree.join("README.md")));
    assert!(cargo_path.to_str().is_some_and(|path| !path.contains('\n')));
}

#[cfg(unix)]
#[test]
fn resolves_a_common_directory_path_ending_in_newline() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp_dir = tempfile::tempdir().unwrap();
    let package_root = temp_dir.path().join("worktree");
    let git_dir = temp_dir.path();
    let common_dir = git_dir.join(OsString::from_vec(b"common-git\r\n".to_vec()));
    fs::create_dir_all(&package_root).unwrap();
    fs::create_dir_all(common_dir.join("refs")).unwrap();
    fs::write(
        package_root.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )
    .unwrap();
    fs::write(git_dir.join("commondir"), "common-git\r\n\n").unwrap();
    fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(git_dir.join("index"), "test-index\n").unwrap();

    let actual = git_metadata::watch_paths(&package_root);

    assert!(actual.contains(&common_dir.join("refs")));
}

fn run_git(package_root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(package_root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generated_version_info_escapes_quotes_in_valid_tag_names() {
    let unusual_tag = r#"v1.0.0"quoted"#;
    let source = version_info::generate("0.1.0", "abc123", unusual_tag, "2025-01-01T00:00:00Z");

    assert!(source.contains(r#"pub const GIT_DESCRIBE: &str = "v1.0.0\"quoted";"#));
}
