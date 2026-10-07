use std::fs;
use std::path::Path;

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const TARGET: &str = "x86_64-unknown-linux-gnu";
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "gitmanager candidate {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let fixture = Self(path);
            fixture.reset_inputs();
            fixture
        }

        fn reset_inputs(&self) {
            fs::write(
                self.0.join("binary"),
                b"#!/bin/sh\nprintf 'candidate fixture\\n'\n",
            )
            .unwrap();
            fs::set_permissions(self.0.join("binary"), fs::Permissions::from_mode(0o755)).unwrap();
            fs::write(
                self.0.join("rustc.txt"),
                format!("rustc 1.99.0 (synthetic fixture)\nhost: {TARGET}\nrelease: 1.99.0\n"),
            )
            .unwrap();
            fs::write(
                self.0.join("cargo.txt"),
                "cargo 1.99.0 (synthetic fixture)\n",
            )
            .unwrap();
        }

        fn package(&self, output: &str, overrides: &[(&str, &str)]) -> Output {
            let mut args = vec![
                (
                    "--binary",
                    self.0.join("binary").to_str().unwrap().to_owned(),
                ),
                ("--source-sha", SHA.to_owned()),
                ("--target", TARGET.to_owned()),
                (
                    "--rustc-info",
                    self.0.join("rustc.txt").to_str().unwrap().to_owned(),
                ),
                (
                    "--cargo-info",
                    self.0.join("cargo.txt").to_str().unwrap().to_owned(),
                ),
                (
                    "--output-dir",
                    self.0.join(output).to_str().unwrap().to_owned(),
                ),
            ];
            for (key, value) in overrides {
                args.iter_mut().find(|arg| arg.0 == *key).unwrap().1 = value.to_string();
            }
            Command::new("python3")
                .arg("-B")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package_candidate.py"))
                .args(args.iter().flat_map(|(key, value)| [*key, value.as_str()]))
                .output()
                .expect("Linux packaging tests require Python 3")
        }

        fn successful_package(&self, output: &str) {
            let result = self.package(output, &[]);
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }

        fn verify(&self, script: &str) {
            let result = Command::new("python3")
                .args(["-B", "-c", script])
                .arg(&self.0)
                .arg(SHA)
                .arg(TARGET)
                .arg(env!("CARGO_MANIFEST_DIR"))
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }

        fn refused(&self, overrides: &[(&str, &str)]) {
            let result = self.package("rejected", overrides);
            assert!(
                !result.status.success(),
                "invalid input unexpectedly packaged"
            );
            assert!(
                !result.stderr.is_empty(),
                "failure must explain the blocker"
            );
            assert!(
                !self.0.join("rejected").exists(),
                "failure left a package behind"
            );
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn package_preserves_executable_and_verifiable_identity() {
        let fixture = Fixture::new();
        fixture.successful_package("output");
        fixture.verify(r#"
import hashlib, json, pathlib, stat, subprocess, sys, tarfile
root, sha, target, repo = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3], pathlib.Path(sys.argv[4])
stem = f'git_manager-{sha}-{target}'
output = root / 'output'
archive = output / (stem + '.tar.gz')
assert sorted(p.name for p in output.iterdir()) == sorted([archive.name, archive.name + '.sha256', stem + '.metadata.json'])
assert (output / (archive.name + '.sha256')).read_text() == hashlib.sha256(archive.read_bytes()).hexdigest() + '  ' + archive.name + '\n'
metadata_bytes = (output / (stem + '.metadata.json')).read_bytes()
metadata = json.loads(metadata_bytes)
assert metadata['schema_version'] == 1
assert metadata['distribution'] == 'unsigned-ci-test'
assert metadata['source_sha'] == sha
assert metadata['source_url'] == f'https://github.com/JohnXu22786/GitManager/tree/{sha}'
assert metadata['target'] == target
assert metadata['toolchain'] == {'rustc_verbose': (root / 'rustc.txt').read_text().strip(), 'cargo': (root / 'cargo.txt').read_text().strip()}
binary = (root / 'binary').read_bytes()
assert metadata['binary'] == {'path': 'git_manager', 'sha256': hashlib.sha256(binary).hexdigest(), 'size_bytes': len(binary)}
with tarfile.open(archive, 'r:gz') as tar:
    members = tar.getmembers()
    assert [m.name for m in members] == [stem + '/git_manager', stem + '/metadata.json', stem + '/LICENSE']
    assert [m.mode for m in members] == [0o755, 0o644, 0o644]
    assert all(m.isfile() and m.uid == 0 and m.gid == 0 and m.mtime == 0 and m.uname == '' and m.gname == '' for m in members)
    assert tar.extractfile(members[0]).read() == binary
    assert tar.extractfile(members[1]).read() == metadata_bytes
    assert tar.extractfile(members[2]).read() == (repo / 'LICENSE').read_bytes()
    tar.extractall(root / 'fresh extraction')
executable = root / 'fresh extraction' / stem / 'git_manager'
assert stat.S_IMODE(executable.stat().st_mode) == 0o755
assert subprocess.check_output([str(executable)], cwd=root / 'fresh extraction') == b'candidate fixture\n'
"#);
    }

    #[test]
    fn repackaging_identical_inputs_is_deterministic() {
        let fixture = Fixture::new();
        fixture.successful_package("first");
        fs::set_permissions(fixture.0.join("binary"), fs::Permissions::from_mode(0o711)).unwrap();
        fixture.verify("import os, sys; os.utime(sys.argv[1] + '/binary', (123456789, 123456789))");
        fixture.successful_package("second");
        fixture.verify(
            r#"
import pathlib, sys
root = pathlib.Path(sys.argv[1])
for file in (root / 'first').iterdir():
    assert file.read_bytes() == (root / 'second' / file.name).read_bytes(), file.name
"#,
        );
        fs::write(
            fixture.0.join("binary"),
            b"#!/bin/sh\nprintf 'different fixture\\n'\n",
        )
        .unwrap();
        fixture.successful_package("changed");
        fixture.verify(
            r#"
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
for file in (root / 'first').iterdir():
    assert file.read_bytes() != (root / 'changed' / file.name).read_bytes(), file.name
"#,
        );
    }

    #[test]
    fn refuses_invalid_inputs_and_existing_output() {
        let fixture = Fixture::new();
        fixture.successful_package("valid-control");
        for sha in [
            "",
            "abc123",
            "../escape",
            "A123456789abcdef0123456789abcdef01234567",
        ] {
            fixture.refused(&[("--source-sha", sha)]);
        }
        fixture.refused(&[("--target", "aarch64-unknown-linux-gnu")]);
        fixture.refused(&[("--binary", fixture.0.join("missing").to_str().unwrap())]);
        fixture.refused(&[("--binary", fixture.0.to_str().unwrap())]);
        symlink(fixture.0.join("binary"), fixture.0.join("link")).unwrap();
        fixture.refused(&[("--binary", fixture.0.join("link").to_str().unwrap())]);
        fs::write(fixture.0.join("binary"), []).unwrap();
        fixture.refused(&[]);
        fixture.reset_inputs();
        fs::set_permissions(fixture.0.join("binary"), fs::Permissions::from_mode(0o644)).unwrap();
        fixture.refused(&[]);
        fixture.reset_inputs();
        for rustc in [
            "",
            "not a compiler",
            "rustc 1.99.0\nhost: aarch64-unknown-linux-gnu\n",
        ] {
            fs::write(fixture.0.join("rustc.txt"), rustc).unwrap();
            fixture.refused(&[]);
        }
        fixture.reset_inputs();
        for cargo in ["", "not cargo", "cargo 1.99.0\nextra line"] {
            fs::write(fixture.0.join("cargo.txt"), cargo).unwrap();
            fixture.refused(&[]);
        }
        fixture.reset_inputs();
        fixture.refused(&[("--rustc-info", fixture.0.join("missing").to_str().unwrap())]);
        fixture.refused(&[("--cargo-info", fixture.0.join("missing").to_str().unwrap())]);
        fs::create_dir(fixture.0.join("existing")).unwrap();
        fs::write(fixture.0.join("existing/keep"), "keep").unwrap();
        assert!(!fixture.package("existing", &[]).status.success());
        assert_eq!(
            fs::read_to_string(fixture.0.join("existing/keep")).unwrap(),
            "keep"
        );
        assert_eq!(fs::read_dir(fixture.0.join("existing")).unwrap().count(), 1);
        symlink(fixture.0.join("existing"), fixture.0.join("output-link")).unwrap();
        assert!(!fixture.package("output-link", &[]).status.success());
        fixture.refused(&[(
            "--output-dir",
            fixture.0.join("binary/child").to_str().unwrap(),
        )]);
    }
}

#[test]
fn ci_candidate_steps_are_main_push_only_and_read_only() {
    let workflow =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/ci.yml"))
            .unwrap()
            .replace("\r\n", "\n");
    assert!(workflow.contains("permissions:\n  contents: read\n"));
    assert_eq!(workflow.matches("permissions:").count(), 1);
    assert!(!workflow.contains("contents: write"));
    assert!(!workflow.contains("action-gh-release"));
    assert!(workflow.contains("run: cargo test --all-targets"));
    assert!(workflow.contains("    env:\n      CARGO_PROFILE_TEST_OPT_LEVEL: \"1\"\n      CARGO_PROFILE_TEST_DEBUG_ASSERTIONS: \"true\"\n      CARGO_PROFILE_TEST_OVERFLOW_CHECKS: \"true\"\n"));
    for key in [
        "CARGO_PROFILE_TEST_OPT_LEVEL",
        "CARGO_PROFILE_TEST_DEBUG_ASSERTIONS",
        "CARGO_PROFILE_TEST_OVERFLOW_CHECKS",
    ] {
        assert_eq!(workflow.matches(key).count(), 1);
    }
    for os in ["ubuntu-latest", "windows-latest", "macos-latest"] {
        assert!(workflow.contains(&format!("          - {os}")));
    }
    let steps: Vec<_> = workflow.split("      - ").collect();
    let test_index = steps
        .iter()
        .position(|step| step.starts_with("name: Run tests\n"))
        .unwrap();
    assert!(steps[test_index].contains("        id: full_suite\n"));
    assert!(steps[test_index].contains("        if: ${{ !cancelled() }}\n"));
    assert!(!workflow.contains("continue-on-error:"));
    let mut harness_steps = Vec::new();
    for name in [
        "Compile Linux scope test harness",
        "Package Linux scope test harness",
        "Upload Linux scope test harness",
    ] {
        let index = steps
            .iter()
            .position(|step| step.starts_with(&format!("name: {name}\n")))
            .unwrap();
        assert!(index < test_index);
        assert!(steps[index].contains("        if: ${{ success() && runner.os == 'Linux' && github.event_name == 'push' && github.ref == 'refs/heads/feat/enforced-adoption-scope' }}\n"));
        harness_steps.push((index, steps[index]));
    }
    assert!(harness_steps.windows(2).all(|pair| pair[0].0 < pair[1].0));
    assert!(harness_steps[0].1.contains("cargo test --locked --no-run --test product_scope_runtime_view_test --message-format=json-render-diagnostics"));
    assert!(!harness_steps[0].1.contains("--release"));
    assert!(!harness_steps[0].1.contains("--features"));
    assert!(harness_steps[1]
        .1
        .contains("python3 scripts/package_scope_test.py"));
    assert!(harness_steps[1].1.contains("--source-sha \"$GITHUB_SHA\""));
    assert!(harness_steps[2]
        .1
        .contains("uses: actions/upload-artifact@v4"));
    assert!(harness_steps[2].1.contains("scope-tests-${{ github.sha }}-${{ github.run_id }}-${{ github.run_attempt }}-product_scope_runtime_view_test-x86_64-unknown-linux-gnu"));
    assert!(harness_steps[2].1.contains("retention-days: 1\n"));
    assert!(harness_steps[2].1.contains("if-no-files-found: error\n"));
    let diagnostic_index = steps
        .iter()
        .position(|step| step.starts_with("name: Diagnose macOS bridge fixture in isolation\n"))
        .expect("missing failure-only macOS diagnostic");
    assert!(diagnostic_index > test_index);
    let diagnostic = steps[diagnostic_index];
    assert!(diagnostic.contains("        if: ${{ failure() && runner.os == 'macOS' && steps.full_suite.outcome == 'failure' }}\n"));
    assert!(diagnostic.contains("        timeout-minutes: 5\n"));
    assert!(diagnostic.contains("        run: cargo test --test product_provider_test bridge_rejects_correlated_and_domain_mismatches_and_cancels_running_fixture -- --exact --nocapture --test-threads=1\n"));
    let consented_index = steps
        .iter()
        .position(|step| {
            step.starts_with("name: Diagnose macOS consented discovery fixture in isolation\n")
        })
        .expect("missing failure-only consented discovery diagnostic");
    assert!(consented_index > diagnostic_index);
    let consented = steps[consented_index];
    assert!(consented.contains("        if: ${{ failure() && runner.os == 'macOS' && steps.full_suite.outcome == 'failure' }}\n"));
    assert!(consented.contains("        timeout-minutes: 5\n"));
    assert!(consented.contains("        run: cargo test --test product_provider_test consented_fake_cli_bridge_preserves_origin_and_rejects_mismatches -- --exact --nocapture --test-threads=1\n"));
    let mut candidate_steps = Vec::new();
    for name in [
        "Build Linux candidate",
        "Package Linux candidate",
        "Upload Linux candidate",
    ] {
        let index = steps
            .iter()
            .position(|step| step.starts_with(&format!("name: {name}\n")))
            .expect("missing candidate step");
        assert!(index > test_index);
        assert!(index > diagnostic_index);
        assert!(steps[index].contains("        if: ${{ success() && runner.os == 'Linux' && github.event_name == 'push' && github.ref == 'refs/heads/main' }}\n"));
        candidate_steps.push((index, steps[index]));
    }
    assert!(candidate_steps.windows(2).all(|pair| pair[0].0 < pair[1].0));
    let build = candidate_steps[0].1;
    assert!(build.contains(
        "cargo build --locked --release --bin git_manager --target x86_64-unknown-linux-gnu"
    ));
    assert!(build.contains("test \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\""));
    let package = candidate_steps[1].1;
    assert!(package.contains("git diff --exit-code HEAD"));
    assert!(package.contains("--source-sha \"$GITHUB_SHA\""));
    assert!(package.contains("--target x86_64-unknown-linux-gnu"));
    assert!(package.contains("rustc --version --verbose"));
    assert!(package.contains("cargo --version"));
    let upload = candidate_steps[2].1;
    assert!(upload.contains("uses: actions/upload-artifact@v4"));
    assert!(
        upload.contains("name: git_manager-${{ github.sha }}-x86_64-unknown-linux-gnu-unsigned")
    );
    assert!(upload.contains("if-no-files-found: error"));
}
