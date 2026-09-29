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
    fs::read_to_string(&path)
        .expect("Failed to read release.yml")
        .replace("\r\n", "\n")
}

fn workflow_job_section(content: &str, name: &str) -> String {
    let jobs = content
        .split_once("jobs:\n")
        .expect("workflow must define jobs")
        .1;
    let marker = format!("  {}:", name);
    let mut in_job = false;
    let mut section = String::new();

    for line in jobs.lines() {
        if line.starts_with("  ") && !line.starts_with("    ") {
            if in_job {
                break;
            }
            in_job = line == marker;
        }
        if in_job {
            section.push_str(line);
            section.push('\n');
        }
    }

    assert!(!section.is_empty(), "Workflow must define job '{}'", name);
    section
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

/// The release upload job must be separate from dependency builds.
#[test]
fn test_release_upload_has_a_separate_job() {
    let content = read_release_workflow();
    assert!(
        workflow_job_section(&content, "release").contains("softprops/action-gh-release"),
        "The release job must upload assets after the builds finish"
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

/// Build jobs must create artifacts but never call the release API.
#[test]
fn test_build_job_only_uploads_workflow_artifacts() {
    let content = read_release_workflow();
    let build = workflow_job_section(&content, "build");
    assert!(
        build.contains("actions/upload-artifact@v4"),
        "Each platform build must publish its package as a workflow artifact"
    );
    assert!(
        build.contains("name: release-${{ matrix.artifact_suffix }}"),
        "Each platform artifact name must match the release download pattern"
    );
    assert!(
        build.contains("path: ${{ steps.package.outputs.archive }}"),
        "Each platform artifact must contain the package produced by its build"
    );
    assert!(
        !build.contains("softprops/action-gh-release"),
        "Build jobs must not upload to the release"
    );
}

/// The final upload job downloads build outputs after all platforms complete.
#[test]
fn test_release_job_collects_build_artifacts() {
    let content = read_release_workflow();
    let release = workflow_job_section(&content, "release");
    assert!(
        release.contains("needs: [prepare-version, build]"),
        "The release job must wait for every platform build"
    );
    assert!(
        release.contains("actions/download-artifact@v4"),
        "The release job must download the platform build artifacts"
    );
    assert!(
        release.contains("pattern: release-*") && release.contains("merge-multiple: true"),
        "The release job must flatten every platform artifact into the shared upload directory"
    );
    assert!(
        release.contains("softprops/action-gh-release"),
        "The release job must upload the downloaded artifacts"
    );
    assert!(
        release.contains("files: release-assets/git-manager-*"),
        "The release job must attach the downloaded GitManager packages"
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
        content.contains("\npermissions:\n  contents: read\n"),
        "Workflow defaults must be read-only"
    );
    assert!(
        workflow_job_section(&content, "release").contains("contents: write"),
        "Only the release upload job must have 'contents: write' permission"
    );
    assert!(
        !workflow_job_section(&content, "build").contains("contents: write"),
        "Build jobs must not have 'contents: write' permission"
    );
    assert!(
        !workflow_job_section(&content, "prepare-version").contains("contents: write"),
        "The version preparation job must not have 'contents: write' permission"
    );
    assert_eq!(
        content.matches("contents: write").count(),
        1,
        "The release upload job must be the only write-enabled permission scope"
    );
}

#[test]
fn test_checkout_does_not_persist_credentials() {
    let content = read_release_workflow();
    assert!(
        workflow_job_section(&content, "prepare-version").contains("persist-credentials: false"),
        "Version preparation does not need persisted checkout credentials"
    );
    assert!(
        workflow_job_section(&content, "build").contains("persist-credentials: false"),
        "Dependency build scripts must not read checkout credentials"
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

/// Release tags must remain data across the workflow's command interpreters.
#[test]
fn test_release_tag_is_not_interpolated_into_shell_source() {
    let content = read_release_workflow();
    let lines: Vec<&str> = content.lines().collect();
    let run_line = lines
        .iter()
        .position(|line| line.trim() == "run: |")
        .expect("The release tag extraction step must have a shell script");
    let run_script = lines[run_line + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || line.starts_with("          "))
        .copied()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        run_script.contains("TAG=\"$GITHUB_REF_NAME\""),
        "The release tag must be read from the GitHub-provided environment variable"
    );
    assert!(
        !run_script.contains("${{"),
        "GitHub expressions must not be interpolated into the release shell script"
    );
    assert!(
        !content.contains("VERSION=\"${{ needs.prepare-version.outputs.app_version }}\""),
        "The tag-derived version must not be interpolated into downstream shell source"
    );
    assert_eq!(
        content
            .matches("VERSION: ${{ needs.prepare-version.outputs.app_version }}")
            .count(),
        2,
        "Both build steps must receive the tag-derived version through the environment"
    );
    assert!(
        !content
            .lines()
            .any(|line| line.contains("sed") && line.contains("$VERSION")),
        "The tag-derived version must not be interpolated into a sed program"
    );
    assert!(
        content.contains(
            r#"perl -i -pe 's/^version = ".*"/version = "$ENV{VERSION}"/' Cargo.toml"#
        ),
        "Unix version replacement must use a fixed Perl program and read VERSION from the environment"
    );
    let powershell_commands: Vec<&str> = content
        .lines()
        .filter(|line| line.contains("powershell -Command"))
        .map(str::trim)
        .collect();
    assert_eq!(
        powershell_commands,
        [r##"powershell -Command "(Get-Content Cargo.toml) -replace '^version = \".*\"', ('version = \"' + \$env:VERSION + '\"') | Set-Content Cargo.toml""##],
        "The Windows command must use the fixed PowerShell program and read VERSION at runtime"
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
