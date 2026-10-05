//! Hand-authored component checks, not controller/native-user acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_store::*;
use std::fs;

fn create(path: &std::path::Path) -> ProductStore {
    ProductStore::create(path, &capture(organizer()), 20000).unwrap()
}
#[test]
fn recovery_restores_exact_current_work_to_a_fresh_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("source");
    let store = create(&source_path);
    let first = store
        .apply(0, "first", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let mut source = organizer();
    source["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    source["actions"][0]["steps"][0]["values"]["note"] = serde_json::json!({"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"text","value":"Later fact"}});
    let target = capture(source);
    let plan = store.prepare_switch(&target, "switch").unwrap();
    let adopted = store
        .adopt(first.revision, &plan, &target, &first.decisions)
        .unwrap();
    let later = store
        .apply(
            adopted.revision,
            "later",
            &add("Ian"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let selected = store
        .apply(
            later.revision,
            "select",
            &invoke("collect", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let saved = store
        .apply(
            selected.revision,
            "export",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(saved.data.records[1].values["note"], string("Later fact"));
    assert_eq!(saved.artifacts.len(), 1);
    let recovered = ProductStore::create_recovered(dir.path().join("recovered"), &saved).unwrap();
    assert_eq!(recovered.load().unwrap(), saved);
    drop(recovered);
    let reopened = ProductStore::open(dir.path().join("recovered")).unwrap();
    assert_eq!(reopened.load().unwrap(), saved);
    assert_eq!(
        reopened
            .apply(
                saved.revision,
                "next",
                &add("Zoe"),
                RuntimeLimits::default()
            )
            .unwrap()
            .data
            .records
            .len(),
        3
    );
    assert_eq!(store.load().unwrap(), saved);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "recovered_tool_process_probe"])
        .env("GITMANAGER_RECOVERY_TEST", dir.path().join("recovered"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(ProductStore::create_recovered(&source_path, &saved).is_err());
    assert_eq!(store.load().unwrap(), saved);
}
#[test]
fn invalid_recovery_and_interrupted_activation_never_publish_current() {
    let dir = tempfile::tempdir().unwrap();
    let store = create(&dir.path().join("source"));
    let before = store.load().unwrap();
    let mut invalid = before.clone();
    invalid.version = 99;
    let target = dir.path().join("invalid");
    assert!(ProductStore::create_recovered(&target, &invalid).is_err());
    assert!(!target.exists());
    let target = dir.path().join("interrupted");
    assert!(
        ProductStore::create_recovered_with(&target, &before, |_| Err(StoreError::Invalid(
            "interrupted attachment restore".into()
        )))
        .is_err()
    );
    assert!(!target.join("CURRENT").exists());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn recovery_activation_cannot_replace_a_competing_current_pointer() {
    let dir = tempfile::tempdir().unwrap();
    let original_path = dir.path().join("original");
    let original = create(&original_path);
    let existing = original
        .apply(
            0,
            "original",
            &add("Existing work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let recovery = original
        .apply(
            existing.revision,
            "later",
            &add("Recovery work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let other_path = dir.path().join("other");
    ProductStore::create_recovered(&other_path, &existing).unwrap();
    let pointer = fs::read(other_path.join("CURRENT")).unwrap();
    assert!(ProductStore::create(&other_path, &capture(organizer()), 20000).is_err());
    assert_eq!(fs::read(other_path.join("CURRENT")).unwrap(), pointer);
    let target = dir.path().join("recovered");
    // Deterministically install an already-committed project after the fresh
    // target is pinned, reproducing the activation side of a creation race.
    let result = ProductStore::create_recovered_with(&target, &recovery, |_| {
        for entry in fs::read_dir(&other_path)? {
            let entry = entry?;
            if entry.file_name() != ".write.lock" {
                fs::copy(entry.path(), target.join(entry.file_name()))?;
            }
        }
        Ok(())
    });
    assert!(result.is_err(), "recovery replaced a competing project");
    assert_eq!(fs::read(target.join("CURRENT")).unwrap(), pointer);
    assert_eq!(
        fs::read_dir(&target).unwrap().count(),
        fs::read_dir(&other_path).unwrap().count(),
        "a rejected fresh activation must not stage an extra snapshot"
    );
    assert_eq!(
        ProductStore::open(&target).unwrap().load().unwrap(),
        existing
    );
    assert_eq!(original.load().unwrap(), recovery);
    assert_eq!(fs::read(other_path.join("CURRENT")).unwrap(), pointer);
    let invalid_target = dir.path().join("invalid-current");
    assert!(
        ProductStore::create_recovered_with(&invalid_target, &recovery, |_| {
            fs::write(
                invalid_target.join("CURRENT"),
                b"interrupted competing project",
            )?;
            Ok(())
        })
        .is_err()
    );
    assert_eq!(
        fs::read(invalid_target.join("CURRENT")).unwrap(),
        b"interrupted competing project"
    );
}
#[test]
fn extension_objects_are_canonical_bounded_and_content_addressed() {
    let dir = tempfile::tempdir().unwrap();
    let store = create(&dir.path().join("source"));
    let before = store.load().unwrap();
    #[derive(serde::Serialize)]
    struct TypedObject {
        version: u32,
        fact: String,
    }
    let typed = TypedObject {
        version: 1,
        fact: "accepted".into(),
    };
    let bytes = canonical_bytes(&typed).unwrap();
    let digest = store.stage_extension(&bytes).unwrap();
    assert_eq!(
        digest,
        canonical_digest(
            IdentityDomain::Evidence,
            &serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        digest,
        canonical_digest(IdentityDomain::Evidence, &typed).unwrap()
    );
    assert_eq!(store.read_extension(&digest).unwrap(), bytes);
    assert_eq!(store.stage_extension(&bytes).unwrap(), digest);
    for invalid in [
        b"{\"x\":1,\"x\":2}".as_slice(),
        b"{ \"x\": 1 }",
        b"not JSON",
        b"{\"float\":1.5}",
    ] {
        assert!(store.stage_extension(invalid).is_err());
    }
    assert!(store
        .stage_extension(&vec![b' '; MAX_WIRE_BYTES + 1])
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    let path = dir
        .path()
        .join("source")
        .join(format!("extension-{}.json", digest.as_str()));
    fs::write(&path, b"{\"fact\":\"corrupted\"}").unwrap();
    assert!(store.read_extension(&digest).is_err());
    assert!(store.stage_extension(&bytes).is_err());
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn recovery_stages_verified_extensions_before_activation() {
    let dir = tempfile::tempdir().unwrap();
    let original = create(&dir.path().join("source"));
    let snapshot = original.load().unwrap();
    let bytes = canonical_bytes(&serde_json::json!({"scene":"saved"})).unwrap();
    let digest = original.stage_extension(&bytes).unwrap();
    let target = dir.path().join("recovered");
    let recovered = ProductStore::create_recovered_with(&target, &snapshot, |fresh| {
        assert!(!target.join("CURRENT").exists());
        assert_eq!(fresh.stage_extension(&bytes)?, digest);
        assert_eq!(fresh.read_extension(&digest)?, bytes);
        Ok(())
    })
    .unwrap();
    assert_eq!(recovered.load().unwrap(), snapshot);
    drop(recovered);
    let reopened = ProductStore::open(&target).unwrap();
    assert_eq!(reopened.read_extension(&digest).unwrap(), bytes);
    assert_eq!(reopened.load().unwrap(), snapshot);
}
#[test]
fn recovery_without_extension_verification_refuses_references() {
    let dir = tempfile::tempdir().unwrap();
    let store = create(&dir.path().join("source"));
    let initial = store.load().unwrap();
    let mut candidate = organizer();
    candidate["label"] = serde_json::json!("Changed");
    let target = capture(candidate);
    let mut plan = store
        .prepare_switch(&target, "switch")
        .unwrap()
        .plan()
        .clone();
    plan.evidence
        .push(canonical_digest(IdentityDomain::Evidence, &"missing package").unwrap());
    let prepared = store.prepare_adoption(plan, &target).unwrap();
    let current = store
        .adopt(initial.revision, &prepared, &target, &initial.decisions)
        .unwrap();
    let destination = dir.path().join("recovery");
    assert!(ProductStore::create_recovered(&destination, &current).is_err());
    assert!(!destination.exists());
    assert_eq!(store.load().unwrap(), current);
}
#[cfg(unix)]
#[test]
fn recovery_path_swap_and_linked_objects_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let original_path = dir.path().join("source");
    let store = create(&original_path);
    let snapshot = store.load().unwrap();
    let target = dir.path().join("recovery");
    let parked = dir.path().join("parked");
    let foreign = tempfile::tempdir().unwrap();
    assert!(
        ProductStore::create_recovered_with(&target, &snapshot, |_| {
            fs::rename(&target, &parked)?;
            symlink(foreign.path(), &target)?;
            Ok(())
        })
        .is_err()
    );
    assert!(!foreign.path().join("CURRENT").exists());
    assert!(!parked.join("CURRENT").exists());
    let bytes = canonical_bytes(&serde_json::json!({"scene":"saved"})).unwrap();
    let digest = store.stage_extension(&bytes).unwrap();
    let object = original_path.join(format!("extension-{}.json", digest.as_str()));
    fs::hard_link(&object, foreign.path().join("alias")).unwrap();
    assert!(store.read_extension(&digest).is_err());
    assert!(store.stage_extension(&bytes).is_err());
    assert_eq!(fs::read(foreign.path().join("alias")).unwrap(), bytes);
    assert_eq!(store.load().unwrap(), snapshot);
    fs::hard_link(
        original_path.join("CURRENT"),
        foreign.path().join("pointer-alias"),
    )
    .unwrap();
    assert!(store.load().is_err());
}
#[test]
fn recovered_tool_process_probe() {
    let Some(path) = std::env::var_os("GITMANAGER_RECOVERY_TEST") else {
        return;
    };
    let store = ProductStore::open(path).unwrap();
    let saved = store.load().unwrap();
    assert_eq!(saved.data.records.len(), 3);
    assert_eq!(saved.data.records[1].values["note"], string("Later fact"));
    assert_eq!(saved.artifacts.len(), 1);
    let continued = store
        .apply(
            saved.revision,
            "child-process",
            &add("Fresh process"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(continued.data.records.len(), 4);
    assert_eq!(
        &continued.data.events[..saved.data.events.len()],
        saved.data.events
    );
    assert_eq!(continued.artifacts, saved.artifacts);
}

// Reuse the same private implementation to exercise its replacement race at
// the file boundary, without making test hooks part of production store APIs.
#[path = "../src/product_store/files.rs"]
mod product_files;
use std::io;
use std::path::Path;

#[test]
fn concurrent_pointer_replacement_keeps_open_readers_valid() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    };
    use std::time::{Duration, Instant};
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    let files = product_files::Directory::open(&path).unwrap();
    files.publish("CURRENT", b"first", false).unwrap();
    let fault = std::env::var("GITMANAGER_POINTER_WRITER_FAULT").ok();
    let timeout = if fault.as_deref() == Some("timeout") {
        Duration::from_millis(20)
    } else {
        Duration::from_secs(30)
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let (start, ready) = mpsc::sync_channel(1);
    let writer = std::thread::spawn(move || {
        ready.recv().unwrap();
        match fault.as_deref() {
            Some("panic") => panic!("injected pointer writer failure"),
            Some("timeout") => std::thread::sleep(Duration::from_secs(1)),
            _ => {}
        }
        if worker_cancelled.load(Ordering::Acquire) {
            return;
        }
        let writer = product_files::Directory::open(&path).unwrap();
        for i in 0..1000 {
            if worker_cancelled.load(Ordering::Acquire) {
                return;
            }
            writer
                .publish(
                    "CURRENT",
                    if i % 2 == 0 { b"first" } else { b"other" },
                    true,
                )
                .unwrap();
        }
    });
    let deadline = Instant::now() + timeout;
    start.send(()).unwrap();
    let mut failure_count = 0usize;
    let mut first_failure = None;
    // is_finished covers both successful completion and an unwinding writer.
    // Keep diagnostics bounded even if every read fails.
    while !writer.is_finished() && Instant::now() < deadline {
        match files.read("CURRENT", 16) {
            Ok(bytes) => assert!(bytes == b"first" || bytes == b"other"),
            Err(error) => {
                failure_count += 1;
                first_failure.get_or_insert(error);
            }
        }
    }
    if !writer.is_finished() {
        cancelled.store(true, Ordering::Release);
        // Joining an unfinished I/O operation could itself hang the test.
        panic!("pointer writer exceeded its deadline of {timeout:?}");
    }
    if let Err(panic) = writer.join() {
        std::panic::resume_unwind(panic);
    }
    assert_eq!(
        failure_count, 0,
        "an atomically replaced pointer must remain readable; first failure: {first_failure:?}"
    );
}

#[test]
fn interrupted_immutable_save_remains_singly_linked_and_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    let store = create(&path);
    let before = store.load().unwrap();
    assert!(store
        .apply_with_fault(
            0,
            "interrupted",
            &add("Ada"),
            RuntimeLimits::default(),
            FaultPoint::AfterObject
        )
        .is_err());
    assert_eq!(store.load().unwrap(), before);
    for entry in fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        assert!(!entry.file_name().to_string_lossy().starts_with(".pending-"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if entry.file_type().unwrap().is_file() {
                assert_eq!(entry.metadata().unwrap().nlink(), 1);
            }
        }
    }
    let retry = ProductStore::open(&path)
        .unwrap()
        .apply(0, "interrupted", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    assert_eq!(retry.data.records.len(), 1);
    assert_eq!(retry.data.events.len(), 1);
}

#[test]
fn same_program_choices_preserve_session_but_real_switches_reset_it() {
    for outcome in [
        DecisionOutcome::Deferred,
        DecisionOutcome::BothNeeded,
        DecisionOutcome::NeitherFits,
        DecisionOutcome::EitherAcceptable,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tool");
        let store = create(&path);
        let first = store
            .apply(0, "first", &add("Ada"), RuntimeLimits::default())
            .unwrap();
        let selected = store
            .apply(
                first.revision,
                "selected",
                &invoke("collect", Values::new()),
                RuntimeLimits::default(),
            )
            .unwrap();
        let filtered = store
            .apply(
                selected.revision,
                "filtered",
                &SemanticInput::Control {
                    view: "people".into(),
                    control: "search_input".into(),
                    value: string("Ada"),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
        let before = store
            .apply(
                filtered.revision,
                "navigate",
                &SemanticInput::Navigate {
                    view: "new_person".into(),
                },
                RuntimeLimits::default(),
            )
            .unwrap();
        let target = before.program().unwrap().clone();
        assert_ne!(
            before.session,
            SessionState::initial(&target.program).unwrap()
        );
        let prepared = store.prepare_switch(&target, "save-choice").unwrap();
        // A structural pending-choice fixture. V07 separately verifies the
        // actual accepted scene/package; this check isolates store atomicity.
        let scene = canonical_digest(IdentityDomain::Evidence, &"accepted-scene-fixture").unwrap();
        let mut graph = before.decisions.clone();
        graph.revision += 1;
        graph.decisions.push(ScopedDecision {
            id: "pending-choice".into(),
            revision: 1,
            request: "Keep this choice for later".into(),
            rationale: None,
            scope: prepared.plan().scope.clone(),
            outcome,
            status: DecisionStatus::Pending,
            obligations: vec![],
            scenarios: vec![scene.clone()],
            witness: scene,
            supersedes: vec![],
        });
        let saved = store
            .adopt_verified(
                before.revision,
                &prepared,
                &target,
                &graph,
                |current, _, next, _| {
                    assert_eq!(current, &before);
                    assert_eq!(next, &graph);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(saved.session, before.session);
        assert_eq!(saved.data, before.data);
        assert_eq!(saved.decisions, graph);
        assert_eq!(saved.artifacts, before.artifacts);
        assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), saved);
        assert_eq!(
            store
                .adopt_verified(
                    before.revision,
                    &prepared,
                    &target,
                    &graph,
                    |_, _, _, _| panic!("an exact retry must use its committed receipt")
                )
                .unwrap(),
            saved
        );
        let mut source = organizer();
        source["label"] = serde_json::json!("A genuinely different program revision");
        let changed = capture(source);
        let switch = store.prepare_switch(&changed, "actual-switch").unwrap();
        let switched = store
            .adopt_verified(saved.revision, &switch, &changed, &graph, |_, _, _, _| {
                Ok(())
            })
            .unwrap();
        assert_eq!(
            switched.session,
            SessionState::initial(&changed.program).unwrap()
        );
        assert_eq!(switched.data.records, saved.data.records);
        assert_eq!(switched.data.events, saved.data.events);
        assert_eq!(switched.decisions, graph);
    }
}

#[test]
fn pointer_writer_failure_and_timeout_are_bounded() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    for (fault, expected) in [
        ("panic", "injected pointer writer failure"),
        ("timeout", "pointer writer exceeded its deadline"),
    ] {
        let log = tempfile::NamedTempFile::new().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "concurrent_pointer_replacement_keeps_open_readers_valid",
                "--nocapture",
            ])
            .env("GITMANAGER_POINTER_WRITER_FAULT", fault)
            .stdout(Stdio::null())
            .stderr(Stdio::from(log.reopen().unwrap()))
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("pointer regression child hung after {fault}");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(
            !status.success(),
            "the injected {fault} must remain a failing test"
        );
        let mut diagnostic = String::new();
        log.reopen()
            .unwrap()
            .take(32 * 1024)
            .read_to_string(&mut diagnostic)
            .unwrap();
        assert!(
            diagnostic.contains(expected),
            "missing actual {fault} diagnosis: {diagnostic}"
        );
    }
}

#[cfg(windows)]
#[test]
fn windows_pointer_replacement_preserves_an_open_reader() {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    let files = product_files::Directory::open(&path).unwrap();
    files
        .publish("CURRENT", b"old committed pointer", false)
        .unwrap();
    let mut old_reader = fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path.join("CURRENT"))
        .unwrap();
    files
        .publish("CURRENT", b"new committed pointer", true)
        .unwrap();
    let mut old_bytes = Vec::new();
    old_reader.read_to_end(&mut old_bytes).unwrap();
    assert_eq!(old_bytes, b"old committed pointer");
    assert_eq!(files.read("CURRENT", 64).unwrap(), b"new committed pointer");
    assert!(files.publish("CURRENT", b"collision", false).is_err());
    assert_eq!(files.read("CURRENT", 64).unwrap(), b"new committed pointer");
}

#[cfg(windows)]
#[test]
fn windows_replacement_does_not_override_readonly_or_hardlinks() {
    let dir = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(dir.path()).unwrap();
    let files = product_files::Directory::open(&path).unwrap();
    files.publish("CURRENT", b"preserved", false).unwrap();
    let target = path.join("CURRENT");
    let original = fs::metadata(&target).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&target, readonly).unwrap();
    let rejected = files.publish("CURRENT", b"must not replace", true);
    let bytes = fs::read(&target).unwrap();
    fs::set_permissions(&target, original).unwrap();
    assert!(rejected.is_err(), "readonly refusal must remain visible");
    assert_eq!(bytes, b"preserved");
    let alias = path.join("external-alias");
    fs::hard_link(&target, &alias).unwrap();
    assert!(files
        .publish("CURRENT", b"must not redirect", true)
        .is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserved");
    assert_eq!(fs::read(&alias).unwrap(), b"preserved");
}
