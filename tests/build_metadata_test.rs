#[path = "../build_support/git_metadata.rs"]
mod git_metadata;

use std::fs;
use std::path::Path;

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
        common_git_dir.join("refs").canonicalize().unwrap(),
        common_git_dir.join("packed-refs").canonicalize().unwrap(),
        common_git_dir.join("reftable").canonicalize().unwrap(),
    ];

    assert_eq!(actual, expected);
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

    assert_eq!(actual[0], git_dir.join("HEAD"));
    assert_eq!(actual[1], git_dir.join("refs"));
    assert_eq!(actual[2], git_dir.join("packed-refs"));
    assert_eq!(actual[3], git_dir.join("reftable"));
}
