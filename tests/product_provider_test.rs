//! All automatic subprocess tests use an explicitly synthetic Python CLI.
//! They exercise transport safety, not AI generation or product acceptance.
#[path = "../src/product_provider/mod.rs"]
mod product_provider;

use product_provider::*;
use serde_json::{json, Value};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

fn request(id: &str, kind: ProviderKind) -> ProviderRequest {
    ProviderRequest {
        request_id: id.into(), provider: kind, source_digest: digest(b"synthetic source"),
        prompt: "Fictional café \"quote\" ; $(touch NEVER) 中文".as_bytes().to_vec(),
        schema: br#"{"type":"object","properties":{"text":{"type":"string"}},"additionalProperties":true}"#.to_vec(),
        purpose: "synthetic transport regression".into(), data_categories: vec!["fictional values".into()],
        profile: CapabilityProfile::DataOnly, limits: JobLimits::default(),
    }
}
fn approval(prepared: &PreparedJob) -> ConsentReceipt {
    ConsentReceipt {
        disclosure_digest: prepared.disclosure.digest(),
        approval_reference: "fixture-only-not-live-approval".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
struct Fixture {
    _temp: tempfile::TempDir,
    transport: ProviderTransport,
    root: std::path::PathBuf,
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Fixture {
    fn new(kind: ProviderKind, mode: &str) -> Self {
        let _publish = fixture_executable_write_guard();
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let bin = root.join("fake CLI 中文's.py");
        fs::write(
            &bin,
            include_bytes!("fixtures/provider_transport/fake_cli.py"),
        )
        .unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            bin.with_extension("json"),
            serde_json::to_vec(&json!({"provider":kind,"mode":mode})).unwrap(),
        )
        .unwrap();
        fs::write(bin.with_extension("outside"), b"outside-must-not-be-read").unwrap();
        let home = root.join("fixture-home");
        fs::create_dir(&home).unwrap();
        let transport = ProviderTransport::new_fixture(root.join("jobs"), kind, bin, home).unwrap();
        Self {
            _temp: temp,
            transport,
            root,
        }
    }
    fn prepare(&self, id: &str) -> PreparedJob {
        self.transport
            .prepare(request(id, self.transport.provider()))
            .unwrap()
    }
    fn run(&self, id: &str) -> JobReceipt {
        let p = self.prepare(id);
        let a = approval(&p);
        let mut job = self.transport.submit(p, &a).unwrap();
        wait(&mut job)
    }
    fn dir(&self, id: &str) -> std::path::PathBuf {
        self.root.join("jobs").join(id)
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait(job: &mut ProviderJob) -> JobReceipt {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(r) = job.poll().unwrap() {
            return r;
        }
        assert!(Instant::now() < deadline, "fixture timeout");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn request_identity_covers_every_disclosed_dimension() {
    let original = request("identity", ProviderKind::Codex);
    let d = original.digest().unwrap();
    let mut changes = vec![];
    let mut r = original.clone();
    r.provider = ProviderKind::Claude;
    changes.push(r);
    let mut r = original.clone();
    r.source_digest = digest(b"changed");
    changes.push(r);
    let mut r = original.clone();
    r.prompt.push(b' ');
    changes.push(r);
    let mut r = original.clone();
    r.schema.push(b' ');
    changes.push(r);
    let mut r = original.clone();
    r.profile = CapabilityProfile::TrustedHarness;
    changes.push(r);
    let mut r = original.clone();
    r.data_categories.push("other".into());
    changes.push(r);
    let mut r = original.clone();
    r.limits.timeout_ms -= 1;
    changes.push(r);
    for r in changes {
        assert_ne!(d, r.digest().unwrap());
    }
}
#[test]
fn malformed_request_never_becomes_a_job() {
    for id in ["", "../escape", "a/b", "..", "é", "a\\b"] {
        assert!(request(id, ProviderKind::Codex).validate().is_err());
    }
    for schema in [
        br#"{"x":1,"x":2}"#.as_slice(),
        b"{",
        br#"{"$ref":"https://untrusted.invalid/schema"}"#,
    ] {
        let mut r = request("schema", ProviderKind::Codex);
        r.schema = schema.to_vec();
        assert!(r.validate().is_err());
    }
    let mut r = request("huge", ProviderKind::Codex);
    r.prompt = vec![b'x'; MAX_PROMPT_BYTES + 1];
    assert!(r.validate().is_err());
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn fixtures_preserve_unicode_and_argv_without_creating_proof() {
    for kind in [ProviderKind::Codex, ProviderKind::Claude] {
        let f = Fixture::new(kind, "good");
        let r = f.run("good");
        assert_eq!(r.state, JobState::TransportValidated);
        assert_eq!(r.provenance, InvocationProvenance::TransportFixture);
        let value = f
            .transport
            .ingest("good", &r.request_digest, &r.source_digest)
            .unwrap()
            .unwrap();
        let parsed: Value = serde_json::from_slice(&value.final_bytes).unwrap();
        assert_eq!(parsed["passed"], true);
        assert_eq!(
            parsed["text"],
            String::from_utf8(request("good", kind).prompt).unwrap()
        );
        assert!(!f.dir("good").join("NEVER").exists());
        let invocation: Value = serde_json::from_slice(
            &fs::read(f.dir("good").join("fixture-invocation.json")).unwrap(),
        )
        .unwrap();
        let args = invocation["argv"].as_array().unwrap();
        assert!(!args
            .iter()
            .any(|a| a.as_str().unwrap().contains("Fictional")));
        let keys = invocation["environment_keys"].as_array().unwrap();
        assert!(!keys.iter().any(|k| k.as_str().unwrap().contains("API_KEY")
            || k == "LD_PRELOAD"
            || k == "CODEX_HOME"));
        assert_eq!(
            f.transport.reconcile("good").unwrap().state,
            JobState::TransportValidated
        );
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn readiness_distinguishes_missing_unsupported_auth_and_failed_probes() {
    for (mode, expected) in [
        ("auth_required", Readiness::AuthRequired),
        ("failed_probe", Readiness::ProbeFailed),
        ("failed_auth_probe", Readiness::ProbeFailed),
        ("missing_flag", Readiness::UnsupportedCapability),
        ("bounded_probe", Readiness::ProbeFailed),
    ] {
        let f = Fixture::new(ProviderKind::Codex, mode);
        assert_eq!(f.transport.probe().readiness, expected, "{mode}");
    }
    let f = Fixture::new(ProviderKind::Codex, "good");
    fs::remove_file(f.transport.executable()).unwrap();
    assert_eq!(f.transport.probe().readiness, Readiness::Missing);
    let f = Fixture::new(ProviderKind::Codex, "good");
    fs::write(
        f.transport.executable().with_extension("json"),
        br#"{"version":"codex-cli 99.0.0"}"#,
    )
    .unwrap();
    assert_eq!(f.transport.probe().readiness, Readiness::UnsupportedVersion);
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn consent_and_capability_changes_are_rejected_before_invocation() {
    for change in ["missing", "expired", "provider", "schema", "executable"] {
        let f = Fixture::new(ProviderKind::Codex, "good");
        let mut p = f.prepare("consent");
        let mut a = approval(&p);
        match change {
            "missing" => a.approval_reference.clear(),
            "expired" => a.expires_at_unix_ms = 0,
            "provider" => p.disclosure.provider = ProviderKind::Claude,
            "schema" => fs::write(f.dir("consent").join("schema.json"), b"{}").unwrap(),
            _ => {
                let _publish = fixture_executable_write_guard();
                use std::io::Write;
                fs::OpenOptions::new()
                    .append(true)
                    .open(f.transport.executable())
                    .unwrap()
                    .write_all(b"\n# changed")
                    .unwrap();
            }
        }
        assert!(f.transport.submit(p, &a).is_err(), "{change}");
        assert!(!f.dir("consent").join("fixture-invocation.json").exists());
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn duplicate_requests_and_stale_source_cannot_reuse_results() {
    let f = Fixture::new(ProviderKind::Codex, "good");
    let r = f.run("once");
    assert!(f
        .transport
        .prepare(request("once", ProviderKind::Codex))
        .is_err());
    assert!(f
        .transport
        .ingest("once", &digest(b"other"), &r.source_digest)
        .is_err());
    assert!(f
        .transport
        .ingest("once", &r.request_digest, &digest(b"other source"))
        .is_err());
    fs::write(f.dir("once").join("result.json"), b"{}").unwrap();
    assert!(f
        .transport
        .ingest("once", &r.request_digest, &r.source_digest)
        .is_err());
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn malformed_out_of_order_duplicate_and_partial_codex_results_fail_closed() {
    for mode in [
        "wrong_digest",
        "wrong_provider",
        "wrong_nonce",
        "wrong_source",
        "duplicate_json",
        "out_of_order",
        "malformed",
        "duplicate_id",
        "after_terminal",
        "missing_terminal",
        "truncated_line",
        "partial",
        "wrong_route",
        "tool_call",
    ] {
        let f = Fixture::new(ProviderKind::Codex, mode);
        let r = f.run("bad");
        assert_eq!(r.state, JobState::InvalidOutput, "{mode}: {}", r.detail);
        assert!(f
            .transport
            .ingest("bad", &r.request_digest, &r.source_digest)
            .unwrap()
            .is_none());
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn claude_requires_structured_output_success_and_its_own_session() {
    for mode in [
        "text_only",
        "wrong_session",
        "partial",
        "wrong_digest",
        "wrong_provider",
    ] {
        let f = Fixture::new(ProviderKind::Claude, mode);
        assert_eq!(f.run("claude").state, JobState::InvalidOutput, "{mode}");
    }
    let f = Fixture::new(ProviderKind::Claude, "claude_error");
    assert_eq!(f.run("error").state, JobState::ProviderError);
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn nonzero_quota_and_network_are_not_success() {
    for (mode, expected) in [
        ("nonzero", JobState::ProviderError),
        ("quota", JobState::Quota),
        ("network", JobState::NetworkError),
    ] {
        let f = Fixture::new(ProviderKind::Codex, mode);
        let r = f.run("error");
        assert_eq!(r.state, expected, "{mode}");
        assert_ne!(r.exit_code, Some(0));
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn stdout_stderr_and_result_files_are_bounded() {
    for mode in ["stdout_overflow", "stderr_overflow", "oversize_file"] {
        let f = Fixture::new(ProviderKind::Codex, mode);
        let receipt = f.run("bound");
        assert_eq!(
            receipt.state,
            JobState::OutputLimit,
            "{mode}: detail={:?}, exit_code={:?}",
            receipt.detail,
            receipt.exit_code
        );
    }
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn symlinks_and_parent_escape_are_not_read_or_overwritten() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new(ProviderKind::Codex, "symlink_output");
    let r = f.run("link");
    assert_eq!(r.state, JobState::InvalidOutput);
    assert_eq!(
        fs::read(f.transport.executable().with_extension("outside")).unwrap(),
        b"outside-must-not-be-read"
    );
    assert!(f.transport.inspect("../escape").is_err());
    symlink(f.root.join("fixture-home"), f.root.join("alias")).unwrap();
    assert!(ProviderTransport::new(
        f.root.join("alias/jobs"),
        ProviderKind::Codex,
        f.transport.executable().to_owned(),
        f.root.join("fixture-home")
    )
    .is_err());
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn timeout_cancellation_and_drop_stop_inherited_child_processes() {
    for mode in ["slow", "child", "blocked_stdin"] {
        let f = Fixture::new(ProviderKind::Codex, mode);
        let mut r = request("timeout", ProviderKind::Codex);
        r.limits.timeout_ms = 150;
        if mode == "blocked_stdin" {
            r.prompt = vec![b'x'; MAX_PROMPT_BYTES];
        }
        let p = f.transport.prepare(r).unwrap();
        let a = approval(&p);
        let mut j = f.transport.submit(p, &a).unwrap();
        assert_eq!(wait(&mut j).state, JobState::TimedOut);
        thread::sleep(Duration::from_millis(100));
        assert!(!f.dir("timeout").join("child-survived").exists());
    }
    let f = Fixture::new(ProviderKind::Codex, "child");
    let p = f.prepare("cancel");
    let a = approval(&p);
    let mut j = f.transport.submit(p, &a).unwrap();
    thread::sleep(Duration::from_millis(100));
    assert!(j.cancel().unwrap());
    assert_eq!(wait(&mut j).state, JobState::Cancelled);
    assert!(!j.cancel().unwrap());
    let p = f.prepare("drop");
    let a = approval(&p);
    let j = f.transport.submit(p, &a).unwrap();
    drop(j);
    assert_eq!(
        f.transport.reconcile("drop").unwrap().state,
        JobState::Cancelled
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn successful_parent_cannot_leave_delayed_children_or_partial_publication() {
    let f = Fixture::new(ProviderKind::Codex, "orphan_child");
    let r = f.run("parent");
    assert_eq!(r.state, JobState::TransportValidated);
    thread::sleep(Duration::from_millis(2200));
    assert!(!f.dir("parent").join("child-survived").exists());
    let p = f.prepare("restart");
    let mut receipt = f.transport.inspect("restart").unwrap();
    receipt.state = JobState::Running;
    fs::write(
        f.dir("restart").join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    fs::write(f.dir("restart").join("final.json"), b"{\"passed\":true}").unwrap();
    assert_eq!(
        f.transport.reconcile("restart").unwrap().state,
        JobState::Interrupted
    );
    assert!(f
        .transport
        .ingest(
            "restart",
            &p.disclosure.request_digest,
            &p.disclosure.source_digest
        )
        .unwrap()
        .is_none());
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn active_reconciliation_and_cancellation_races_do_not_invent_completion() {
    let f = Fixture::new(ProviderKind::Codex, "slow");
    let p = f.prepare("active");
    let a = approval(&p);
    let mut j = f.transport.submit(p, &a).unwrap();
    assert!(f.transport.reconcile("active").is_err());
    j.cancel().unwrap();
    assert_eq!(wait(&mut j).state, JobState::Cancelled);
    for n in 0..8 {
        let f = Fixture::new(ProviderKind::Codex, "good");
        let p = f.prepare(&format!("race{n}"));
        let a = approval(&p);
        let mut j = f.transport.submit(p, &a).unwrap();
        let accepted = j.cancel().unwrap();
        let r = wait(&mut j);
        if accepted {
            assert_eq!(r.state, JobState::Cancelled);
        } else {
            assert!(r.state.is_terminal());
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn real_endpoint_never_infers_data_only_from_help_or_authentication() {
    let f = Fixture::new(ProviderKind::Codex, "good");
    // Still a synthetic executable: production factory exercises the ordinary
    // probe policy without any model call or generation authority.
    let transport = ProviderTransport::new(
        f.root.join("policy"),
        ProviderKind::Codex,
        f.transport.executable().to_owned(),
        f.root.join("fixture-home"),
    )
    .unwrap();
    let cap = transport.probe();
    assert_eq!(cap.readiness, Readiness::ReadyUntested);
    assert!(!cap.supports(CapabilityProfile::DataOnly));
    assert!(cap.supports(CapabilityProfile::TrustedHarness));
    assert!(transport
        .prepare(request("data-only", ProviderKind::Codex))
        .is_err());
    let mut r = request("trusted", ProviderKind::Codex);
    r.profile = CapabilityProfile::TrustedHarness;
    let p = transport.prepare(r).unwrap();
    assert!(p.disclosure.capability_notice.contains("outside this job"));
    let mut consent = approval(&p);
    consent.disclosure_digest = digest(b"generic synthetic-data approval");
    assert!(transport.submit(p, &consent).is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn trusted_harness_discloses_limits_of_unchanged_probe_fingerprints() {
    for kind in [ProviderKind::Codex, ProviderKind::Claude] {
        let f = Fixture::new(kind, "good");
        // The production policy still runs only the synthetic fixture's probes.
        let transport = ProviderTransport::new(
            f.root.join("policy"),
            kind,
            f.transport.executable().to_owned(),
            f.root.join("fixture-home"),
        )
        .unwrap();
        let mut r = request("diagnostics", kind);
        r.profile = CapabilityProfile::TrustedHarness;
        let p = transport.prepare(r).unwrap();
        assert!(!p.capabilities.supports(CapabilityProfile::DataOnly));
        let notice = &p.disclosure.capability_notice;
        for limitation in [
            "application-controlled input",
            "Requested controls",
            "version/help/auth diagnostic outputs",
            "not isolation or effective-configuration attestations",
            "Configuration changes with unchanged diagnostic outputs can go undetected",
            "effective tool catalog and injected context remain unknown",
        ] {
            assert!(notice.contains(limitation), "{kind:?}: {limitation}");
        }

        // Fixture behavior changes without changing its executable or readiness
        // output. Equality is a diagnostic recheck, not a configuration check.
        fs::write(
            f.transport.executable().with_extension("json"),
            serde_json::to_vec(&json!({"provider": kind, "mode": "quota"})).unwrap(),
        )
        .unwrap();
        assert_eq!(p.capabilities, transport.probe());
        let mut incomplete_disclosure = p.disclosure.clone();
        incomplete_disclosure.capability_notice.clear();
        let mut consent = approval(&p);
        consent.disclosure_digest = incomplete_disclosure.digest();
        assert!(transport.submit(p, &consent).is_err());
        assert!(!f
            .root
            .join("policy/diagnostics/fixture-invocation.json")
            .exists());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn corrupted_receipts_and_unfinished_publication_do_not_panic_or_pass() {
    let f = Fixture::new(ProviderKind::Claude, "good");
    let p = f.prepare("corrupt");
    let a = approval(&p);
    let mut receipt = f.transport.inspect("corrupt").unwrap();
    receipt.disclosure.nonce = "short".into();
    fs::write(
        f.dir("corrupt").join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(f.transport.submit(p, &a).is_err());
    let r = f.run("done");
    fs::rename(
        f.dir("done").join("result.json"),
        f.dir("done").join("result.json.partial"),
    )
    .unwrap();
    assert!(f
        .transport
        .ingest("done", &r.request_digest, &r.source_digest)
        .is_err());
    let mut receipt = f.transport.inspect("done").unwrap();
    receipt.version = 999;
    fs::write(
        f.dir("done").join("receipt.json"),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    assert!(f.transport.reconcile("done").is_err());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn enveloping_preserves_root_local_schema_references_and_rejects_anchors() {
    let f = Fixture::new(ProviderKind::Codex, "good");
    let mut r = request("refs", ProviderKind::Codex);
    r.schema=br##"{"type":"object","$defs":{"Text":{"type":"string"}},"properties":{"text":{"$ref":"#/$defs/Text"}}}"##.to_vec();
    f.transport.prepare(r).unwrap();
    let schema: Value =
        serde_json::from_slice(&fs::read(f.dir("refs").join("schema.json")).unwrap()).unwrap();
    assert_eq!(
        schema
            .pointer("/properties/payload/properties/text/$ref")
            .unwrap(),
        "#/properties/payload/$defs/Text"
    );
    for schema in [
        br##"{"$ref":"#named"}"##.as_slice(),
        br##"{"$id":"https://example.invalid/schema"}"##,
    ] {
        let mut r = request("anchor", ProviderKind::Codex);
        r.schema = schema.to_vec();
        assert!(r.validate().is_err());
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn schema_literals_and_properties_named_like_keywords_are_not_rewritten() {
    let f = Fixture::new(ProviderKind::Codex, "good");
    let mut r = request("literal", ProviderKind::Codex);
    r.schema=br##"{"type":"object","const":{"$ref":"#"},"properties":{"$ref":{"type":"string"}},"examples":[{"$id":"literal data"}],"default":{"$anchor":"literal"}}"##.to_vec();
    f.transport.prepare(r).unwrap();
    let schema: Value =
        serde_json::from_slice(&fs::read(f.dir("literal").join("schema.json")).unwrap()).unwrap();
    assert_eq!(
        schema.pointer("/properties/payload/const/$ref").unwrap(),
        "#"
    );
    assert_eq!(
        schema
            .pointer("/properties/payload/examples/0/$id")
            .unwrap(),
        "literal data"
    );
    assert_eq!(
        schema
            .pointer("/properties/payload/properties/$ref/type")
            .unwrap(),
        "string"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn consent_expiring_during_probes_never_launches_a_provider() {
    let f = Fixture::new(ProviderKind::Codex, "slow_probe");
    let p = f.prepare("expires");
    let mut a = approval(&p);
    a.expires_at_unix_ms = unix_ms() + 50;
    assert!(f.transport.submit(p, &a).is_err());
    assert!(!f.dir("expires").join("fixture-invocation.json").exists());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_correlated_response_cannot_hide_incomplete_stdin_delivery() {
    let f = Fixture::new(ProviderKind::Codex, "early_exit");
    let mut request = request("early", ProviderKind::Codex);
    request.prompt = vec![b'x'; MAX_PROMPT_BYTES];
    let p = f.transport.prepare(request).unwrap();
    let a = approval(&p);
    let mut job = f.transport.submit(p, &a).unwrap();
    assert_eq!(wait(&mut job).state, JobState::ProviderError);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn identical_binary_on_another_route_needs_new_disclosure() {
    let f = Fixture::new(ProviderKind::Codex, "good");
    let p = f.prepare("route");
    let a = approval(&p);
    let other = f.root.join("other-fixture.py");
    let publish = fixture_executable_write_guard();
    fs::copy(f.transport.executable(), &other).unwrap();
    fs::copy(
        f.transport.executable().with_extension("json"),
        other.with_extension("json"),
    )
    .unwrap();
    drop(publish);
    let transport = ProviderTransport::new_fixture(
        f.root.join("jobs"),
        ProviderKind::Codex,
        other,
        f.root.join("fixture-home"),
    )
    .unwrap();
    assert_eq!(p.capabilities, transport.probe());
    assert!(transport.submit(p, &a).is_err());
    assert!(!f.dir("route").join("fixture-invocation.json").exists());
}

#[test]
fn rejects_recursive_schema_keywords_but_preserves_literal_instances() {
    for schema in [
        br##"{"type":"object","properties":{"child":{"$recursiveRef":"#"}}}"##.as_slice(),
        br##"{"$recursiveAnchor":true}"##,
    ] {
        let mut r = request("recursive", ProviderKind::Codex);
        r.schema = schema.to_vec();
        assert!(r.validate().is_err());
    }
    let mut r = request("literal-recursive", ProviderKind::Codex);
    r.schema=br##"{"const":{"$recursiveRef":"#","$recursiveAnchor":true},"properties":{"$recursiveRef":{"type":"string"}}}"##.to_vec();
    assert!(r.validate().is_ok());
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn readiness_rejects_fifo_without_waiting_for_a_writer() {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, fs::OpenOptionsExt},
        sync::mpsc,
    };
    #[cfg(target_os = "linux")]
    type Mode = u32;
    #[cfg(target_os = "macos")]
    type Mode = u16;
    unsafe extern "C" {
        fn mkfifo(path: *const std::ffi::c_char, mode: Mode) -> i32;
    }
    let f = Fixture::new(ProviderKind::Codex, "good");
    let fifo = f.root.join("not-an-executable");
    let name = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { mkfifo(name.as_ptr(), 0o600) }, 0);
    let transport = ProviderTransport::new(
        f.root.join("fifo-jobs"),
        ProviderKind::Codex,
        fifo.clone(),
        f.root.join("fixture-home"),
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        tx.send(transport.probe()).unwrap();
    });
    let response = rx.recv_timeout(Duration::from_secs(1));
    if response.is_err() {
        // Unblock the old implementation's read-open without letting a failing
        // regression leak a thread or making the test's own writer block.
        #[cfg(target_os = "linux")]
        let nonblocking = 0x800;
        #[cfg(target_os = "macos")]
        let nonblocking = 0x4;
        let _ = fs::OpenOptions::new()
            .write(true)
            .custom_flags(nonblocking)
            .open(&fifo);
    }
    worker.join().unwrap();
    assert!(response.is_ok(), "probe waited for a FIFO writer");
    assert_eq!(response.unwrap().readiness, Readiness::ProbeFailed);
}
