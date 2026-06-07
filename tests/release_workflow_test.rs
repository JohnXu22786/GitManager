use std::fs;
use std::path::Path;

/// Helper: read the release workflow file content
fn read_release_workflow() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(".github")
        .join("workflows")
        .join("release.yml");
    assert!(
        path.exists(),
        "release.yml must exist at {:?}",
        path
    );
    fs::read_to_string(&path).expect("Failed to read release.yml")
}

// ──────────────────────────────────────────────
// Stroom Pattern: Structural checks
// ──────────────────────────────────────────────

/// The workflow MUST have a `prepare-version` job that extracts the version
/// once, so all platform jobs can reuse it (Stroom pattern).
#[test]
fn test_has_prepare_version_job() {
    let content = read_release_workflow();
    assert!(
        content.contains("prepare-version"),
        "Workflow must have a 'prepare-version' job (Stroom pattern)"
    );
}

/// The workflow MUST NOT have a separate upload-artifacts phase.
/// Stroom pattern: each platform uploads directly — no aggregator job.
#[test]
fn test_no_separate_upload_job() {
    let content = read_release_workflow();
    assert!(
        !content.contains("upload-artifacts"),
        "Workflow must NOT have a separate 'upload-artifacts' job \
         (Stroom pattern: each platform uploads directly)"
    );
}

/// The workflow MUST NOT set `body`, `body_path`, or `name` on the release action.
/// Setting any of these overwrites the user's manually written release notes
/// or renames the release. Stroom pattern: only `tag_name` and `files` are set.
#[test]
fn test_no_body_or_name_field_in_upload_step() {
    let content = read_release_workflow();
    // Scan the `with:` block of softprops/action-gh-release for forbidden keys
    let lines: Vec<&str> = content.lines().collect();
    let mut inside_gh_release = false;
    let mut found_forbidden_key = false;
    let mut key_name = "";
    let mut indent_level: Option<usize> = None;

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("uses: softprops/action-gh-release") {
            inside_gh_release = true;
            indent_level = None;
            continue;
        }
        if inside_gh_release {
            // Determine the indent level of the first `with:` line after the action
            if trimmed == "with:" {
                indent_level = Some(line.len() - trimmed.len());
                continue;
            }
            // Check if we've moved past the `with:` block (less indentation)
            if let Some(base_indent) = indent_level {
                let current_indent = line.len() - trimmed.len();
                if current_indent <= base_indent && !trimmed.is_empty() {
                    // We've left the `with:` block
                    inside_gh_release = false;
                    continue;
                }
                // Check for forbidden keys in the with block
                if trimmed.starts_with("body:")
                    || trimmed.starts_with("body_path:")
                    || trimmed.starts_with("name:")
                {
                    found_forbidden_key = true;
                    key_name = trimmed.split(':').next().unwrap_or("unknown");
                    break;
                }
            }
        }
    }

    assert!(
        !found_forbidden_key,
        "Workflow MUST NOT set '{}' in softprops/action-gh-release \
         (Stroom pattern: preserve user-written release notes)",
        key_name
    );
}

/// Each build job MUST directly call softprops/action-gh-release to upload
/// its artifact (not a separate upload job).
/// The action is defined once in the matrix job; the matrix expands it at
/// runtime to run for each platform target.
#[test]
fn test_each_build_job_uploads_directly() {
    let content = read_release_workflow();
    // The action appears once in the YAML (inside the build matrix job).
    // The matrix expansion means it runs for all 5 platforms at runtime.
    let gh_release_count = content
        .matches("uses: softprops/action-gh-release")
        .count();
    assert!(
        gh_release_count >= 1,
        "The build matrix job must use softprops/action-gh-release, \
         but found {} occurrences",
        gh_release_count
    );
}

/// The workflow MUST NOT use intermediate workflow artifact upload/download.
/// Stroom pattern: each platform uploads directly to the release.
#[test]
fn test_no_intermediate_artifacts() {
    let content = read_release_workflow();
    assert!(
        !content.contains("actions/upload-artifact"),
        "Workflow must NOT use 'actions/upload-artifact' \
         (Stroom pattern: each platform uploads directly to release)"
    );
    assert!(
        !content.contains("actions/download-artifact"),
        "Workflow must NOT use 'actions/download-artifact' \
         (Stroom pattern: each platform uploads directly to release)"
    );
}

// ──────────────────────────────────────────────
// Build matrix preservation checks
// ──────────────────────────────────────────────

/// All 5 original platform targets must still be present.
#[test]
fn test_all_platform_targets_present() {
    let content = read_release_workflow();
    let required_targets = [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];
    for target in &required_targets {
        assert!(
            content.contains(target),
            "Build matrix must include target '{}'",
            target
        );
    }
}

/// All 5 original artifact suffixes must still be present.
#[test]
fn test_all_artifact_suffixes_present() {
    let content = read_release_workflow();
    let required_suffixes = [
        "linux-x86_64",
        "linux-aarch64",
        "windows-x86_64",
        "macos-x86_64",
        "macos-aarch64",
    ];
    for suffix in &required_suffixes {
        assert!(
            content.contains(suffix),
            "Build matrix must include artifact suffix '{}'",
            suffix
        );
    }
}

/// The workflow must still trigger on `release: [published]`.
#[test]
fn test_triggers_on_release_published() {
    let content = read_release_workflow();
    assert!(
        content.contains("release:")
            && content.contains("published"),
        "Workflow must trigger on 'release: [published]'"
    );
}

/// The workflow must have `contents: write` permission.
#[test]
fn test_has_contents_write_permission() {
    let content = read_release_workflow();
    assert!(
        content.contains("contents: write"),
        "Workflow must have 'contents: write' permission"
    );
}

/// The build matrix must still have `fail-fast: false` for all platforms.
#[test]
fn test_fail_fast_false() {
    let content = read_release_workflow();
    assert!(
        content.contains("fail-fast: false"),
        "Build matrix must have 'fail-fast: false' so all platforms build"
    );
}

// ──────────────────────────────────────────────
// Version extraction checks
// ──────────────────────────────────────────────

/// The `prepare-version` job should set an `app_version` output that all build
/// jobs can reference via `needs.prepare-version.outputs.app_version`.
#[test]
fn test_prepare_version_sets_app_version_output() {
    let content = read_release_workflow();
    assert!(
        content.contains("app_version"),
        "The prepare-version job should set an 'app_version' output"
    );
}

/// Each build job should `need: [prepare-version]` (Stroom pattern).
#[test]
fn test_build_jobs_depend_on_prepare_version() {
    let content = read_release_workflow();
    // The build matrix job definition (before the `runs-on` line) should
    // declare `needs: [prepare-version]`
    let lines: Vec<&str> = content.lines().collect();
    let mut found_needs_for_build = false;

    for (i, line) in lines.iter().enumerate() {
        if line.trim() == "strategy:" {
            // Look backwards from this point for the nearest `needs:` declaration
            // in the `build:` job (before `strategy:` but after a job name)
            for j in (0..i).rev() {
                let t = lines[j].trim();
                if t == "needs:" || t.starts_with("needs:") {
                    // Continue with next line if it's a list
                    if t == "needs:" {
                        if j + 1 < i && lines[j + 1].trim().contains("prepare-version") {
                            found_needs_for_build = true;
                        }
                    } else if t.contains("prepare-version") {
                        found_needs_for_build = true;
                    }
                    break;
                }
                if t.starts_with("runs-on:") || t.starts_with("name:") {
                    break;
                }
            }
        }
    }

    assert!(
        found_needs_for_build,
        "Build matrix job should have 'needs: [prepare-version]'"
    );
}
