#[path = "fixtures/tool_project_fixture.rs"]
mod fixture;
#[path = "../src/tool_project.rs"]
mod tool_project;
#[path = "../src/tool_runtime.rs"]
mod tool_runtime;
#[path = "../src/tool_store.rs"]
mod tool_store;

use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::thread;
use tool_store::{CommitDisposition, ProjectStore, ReadOnlyReason, StoreError, StoreLoad};

fn writable(load: StoreLoad) -> tool_project::ProjectSnapshot {
    match load {
        StoreLoad::Writable(snapshot) => snapshot,
        StoreLoad::ReadOnly(read_only) => {
            panic!("unexpected read-only store: {:?}", read_only.reason)
        }
    }
}

fn create_store(path: &Path) -> (ProjectStore, tool_project::ProjectSnapshot) {
    let snapshot = fixture::empty_project(tool_project::studio_order_template(), "store-project");
    let store = ProjectStore::create(path, &snapshot, "create-project-1").unwrap();
    let base = writable(store.load().unwrap());
    (store, base)
}

fn edited(snapshot: &tool_project::ProjectSnapshot, name: &str) -> tool_project::ProjectSnapshot {
    let mut next = snapshot.clone();
    next.project_name = name.to_owned();
    next
}

fn decision(
    decision_id: &str,
    status: tool_project::DecisionStatus,
    supersedes: Vec<String>,
) -> tool_project::DecisionRecord {
    tool_project::DecisionRecord {
        decision_id: decision_id.to_owned(),
        intent_revision: 1,
        choice: tool_project::DecisionChoice::Adopt,
        expected_outcomes: Vec::new(),
        scope: None,
        rationale: "Keep the original promised date".to_owned(),
        unresolved_questions: Vec::new(),
        scenario_ids: Vec::new(),
        evidence_ids: Vec::new(),
        supersedes,
        status,
        created_at: fixture::at(2026, 10, 5, 12),
        extensions: Default::default(),
    }
}

fn observe_scenario(
    snapshot: &tool_project::ProjectSnapshot,
    scenario_id: &str,
) -> tool_project::Scenario {
    tool_project::Scenario {
        scenario_id: scenario_id.to_owned(),
        name: "本地证据演练".to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings,
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: fixture::standard_behavior(),
        as_of_date: fixture::date(2026, 10, 5),
        fixed_now: fixture::at(2026, 10, 5, 12),
        steps: vec![tool_project::ScenarioStep::Observe {
            record_id: "record-1".to_owned(),
        }],
        expected: Vec::new(),
        extensions: Default::default(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use ring::digest::{digest, SHA256};
    digest(&SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rewrite_snapshot(path: &Path, transform: impl FnOnce(&mut Value)) -> Vec<u8> {
    let pointer_path = path.join("CURRENT");
    let pointer: Value = serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
    let snapshot_name = pointer["snapshot_file"].as_str().unwrap();
    let snapshot_path = path.join("snapshots").join(snapshot_name);
    let mut snapshot: Value = serde_json::from_slice(&fs::read(&snapshot_path).unwrap()).unwrap();
    transform(&mut snapshot);
    let snapshot_bytes = serde_json::to_vec(&snapshot).unwrap();
    fs::write(&snapshot_path, &snapshot_bytes).unwrap();

    let mut next_pointer: Value =
        serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
    next_pointer["snapshot_sha256"] = json!(sha256_hex(&snapshot_bytes));
    fs::write(&pointer_path, serde_json::to_vec(&next_pointer).unwrap()).unwrap();
    snapshot_bytes
}

#[test]
fn create_commit_reopen_and_optional_extensions_preserve_project_state() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("studio-project");
    let mut snapshot =
        fixture::empty_project(tool_project::studio_order_template(), "store-project");
    snapshot.extensions.insert(
        "future_optional_metadata".to_owned(),
        json!({"color": "amber", "revision": 3}),
    );
    let store = ProjectStore::create(&root, &snapshot, "create-project-1").unwrap();
    let base = writable(store.load().unwrap());
    assert_eq!(base.generation, 0);
    assert_eq!(
        base.extensions.get("future_optional_metadata"),
        Some(&json!({"color": "amber", "revision": 3}))
    );

    let next = edited(&base, "工作室订单");
    let committed = store.commit(0, "rename-project-1", &next).unwrap();
    assert_eq!(committed.disposition, CommitDisposition::Committed);
    assert_eq!(committed.snapshot.generation, 1);

    let reopened = ProjectStore::open(&root).unwrap();
    let saved = writable(reopened.load().unwrap());
    assert_eq!(saved.project_name, "工作室订单");
    assert_eq!(saved.generation, 1);
    assert_eq!(saved.extensions, base.extensions);
}

#[test]
fn commit_rejects_forged_native_execution_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("native-evidence-project");
    let mut snapshot = fixture::studio_order_project("store-project");
    let store = ProjectStore::create(&root, &snapshot, "create-native-project").unwrap();
    snapshot = writable(store.load().unwrap());

    let scenario = observe_scenario(&snapshot, "native-scenario");
    let evidence = tool_runtime::run_scenario(&scenario).unwrap();
    snapshot.scenarios.push(scenario);
    snapshot.evidence.push(evidence);
    snapshot.evidence[0].status = tool_project::EvidenceStatus::Passed;

    assert!(matches!(
        store.commit(0, "forged-native-evidence", &snapshot),
        Err(StoreError::InvalidArgument(_)) | Err(StoreError::InvalidSnapshot(_))
    ));
    assert!(writable(store.load().unwrap()).evidence.is_empty());
}

#[test]
fn all_existing_scope_freezes_every_record_at_its_confirmed_generation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("all-existing-scope-project");
    let snapshot = fixture::studio_order_project("store-project");
    let store = ProjectStore::create(&root, &snapshot, "create-scope-project").unwrap();
    let base = writable(store.load().unwrap());

    let mut request = base.clone();
    request.rule_bindings.push(tool_project::RuleBinding {
        binding_id: "incomplete-all-scope".to_owned(),
        rule_key: tool_project::RuleKey::Timer,
        record_id: None,
        behavior_revision_id: "behavior-1".to_owned(),
        previous_binding_id: None,
        scope: tool_project::ScopeSelection {
            kind: tool_project::ScopeKind::AllExistingAndFuture,
            confirmed_generation: base.generation,
            frozen_record_ids: Vec::new(),
            applies_to_future_records: true,
            effective_sequence: 1,
        },
        effective_sequence: 1,
    });

    assert!(matches!(
        store.commit(0, "incomplete-all-scope", &request),
        Err(StoreError::InvalidArgument(_))
    ));
    assert!(writable(store.load().unwrap()).rule_bindings.is_empty());

    let mut complete_request = base.clone();
    complete_request
        .rule_bindings
        .push(tool_project::RuleBinding {
            binding_id: "complete-all-scope".to_owned(),
            rule_key: tool_project::RuleKey::Timer,
            record_id: None,
            behavior_revision_id: "behavior-1".to_owned(),
            previous_binding_id: None,
            scope: tool_project::ScopeSelection {
                kind: tool_project::ScopeKind::AllExistingAndFuture,
                confirmed_generation: base.generation,
                frozen_record_ids: vec!["record-1".to_owned()],
                applies_to_future_records: true,
                effective_sequence: 1,
            },
            effective_sequence: 1,
        });
    let complete = store
        .commit(0, "complete-all-scope", &complete_request)
        .unwrap();

    let mut incomplete_request = complete.snapshot.clone();
    incomplete_request
        .rule_bindings
        .push(tool_project::RuleBinding {
            binding_id: "incomplete-only-scope".to_owned(),
            rule_key: tool_project::RuleKey::Timer,
            record_id: None,
            behavior_revision_id: "behavior-1".to_owned(),
            previous_binding_id: None,
            scope: tool_project::ScopeSelection {
                kind: tool_project::ScopeKind::IncompleteAndFuture,
                confirmed_generation: complete.snapshot.generation,
                frozen_record_ids: Vec::new(),
                applies_to_future_records: true,
                effective_sequence: 2,
            },
            effective_sequence: 2,
        });
    assert!(matches!(
        store.commit(1, "incomplete-only-scope", &incomplete_request),
        Err(StoreError::InvalidArgument(_))
    ));
}

#[test]
fn a_binding_single_record_scope_must_match_its_target_record() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("single-record-scope-project");
    let initial = fixture::studio_order_project("scope-project");
    let store = ProjectStore::create(&root, &initial, "create-scope-project").unwrap();
    let base = writable(store.load().unwrap());

    let mut values = initial.records[0].typed_values.clone();
    values.insert(
        "order_number".to_owned(),
        tool_project::FieldValue::Text("第二笔订单".to_owned()),
    );
    let occurred_at = fixture::at(2026, 10, 2, 10);
    let with_second_record = tool_runtime::apply_command(
        &base,
        &tool_project::ToolCommand::CreateRecord {
            operation_id: "create-second-record".to_owned(),
            expected_generation: base.generation,
            record_id: "record-2".to_owned(),
            initial_stage_id: "queued".to_owned(),
            values,
            occurred_at: occurred_at.clone(),
        },
        occurred_at,
    )
    .unwrap();
    let with_two_records = store
        .commit(0, "create-second-record", &with_second_record)
        .unwrap()
        .snapshot;
    assert_eq!(with_two_records.records.len(), 2);

    let mut mismatched = with_two_records.clone();
    mismatched.rule_bindings.push(tool_project::RuleBinding {
        binding_id: "mismatched-single-record-binding".to_owned(),
        rule_key: tool_project::RuleKey::Timer,
        record_id: Some("record-1".to_owned()),
        behavior_revision_id: "behavior-1".to_owned(),
        previous_binding_id: None,
        scope: tool_project::ScopeSelection {
            kind: tool_project::ScopeKind::SingleRecord,
            confirmed_generation: with_two_records.generation,
            frozen_record_ids: vec!["record-2".to_owned()],
            applies_to_future_records: false,
            effective_sequence: 1,
        },
        effective_sequence: 1,
    });

    assert!(matches!(
        store.commit(1, "mismatched-single-record-binding", &mismatched),
        Err(StoreError::InvalidArgument(_))
    ));
    assert!(writable(store.load().unwrap()).rule_bindings.is_empty());
}

#[test]
fn operation_retry_is_idempotent_and_reuse_with_new_payload_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("idempotent-project");
    let (store, base) = create_store(&root);
    let request = edited(&base, "第一次修改");
    let first = store.commit(0, "operation-1", &request).unwrap();
    assert_eq!(first.snapshot.generation, 1);

    let retry = store.commit(0, "operation-1", &request).unwrap();
    assert_eq!(retry.disposition, CommitDisposition::AlreadyApplied);
    assert_eq!(retry.snapshot.generation, 1);

    let conflicting_payload = edited(&base, "不同请求");
    assert!(matches!(
        store.commit(0, "operation-1", &conflicting_payload),
        Err(StoreError::OperationIdentityConflict { .. })
    ));
    assert!(matches!(
        store.commit(0, "operation-2", &conflicting_payload),
        Err(StoreError::GenerationConflict {
            expected: 0,
            actual: 1
        })
    ));
    assert_eq!(writable(store.load().unwrap()).generation, 1);
}

#[test]
fn operation_receipt_does_not_bypass_candidate_generation_validation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("receipt-generation-project");
    let (store, base) = create_store(&root);
    let request = edited(&base, "First generation");
    store
        .commit(0, "generation-checked-operation", &request)
        .unwrap();

    let mismatched = store.commit(1, "generation-checked-operation", &request);
    assert!(matches!(
        mismatched,
        Err(StoreError::GenerationConflict {
            expected: 1,
            actual: 0
        })
    ));
    assert_eq!(writable(store.load().unwrap()).generation, 1);
}

#[test]
fn stale_handles_and_simultaneous_writers_cannot_drop_an_update() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("concurrent-project");
    let (store, base) = create_store(&root);
    let stale_handle = ProjectStore::open(&root).unwrap();
    let first = edited(&base, "winner A");
    let second = edited(&base, "winner B");
    store.commit(0, "sequential-winner", &first).unwrap();
    assert!(matches!(
        stale_handle.commit(0, "sequential-stale", &second),
        Err(StoreError::GenerationConflict {
            expected: 0,
            actual: 1
        })
    ));

    let racing_root = temp.path().join("race-project");
    let (seed_store, race_base) = create_store(&racing_root);
    drop(seed_store);
    let left = ProjectStore::open(&racing_root).unwrap();
    let right = ProjectStore::open(&racing_root).unwrap();
    let left_request = edited(&race_base, "race left");
    let right_request = edited(&race_base, "race right");
    let barrier = Arc::new(Barrier::new(3));
    let left_barrier = Arc::clone(&barrier);
    let right_barrier = Arc::clone(&barrier);
    let left_thread = thread::spawn(move || {
        left_barrier.wait();
        left.commit(0, "race-left", &left_request)
    });
    let right_thread = thread::spawn(move || {
        right_barrier.wait();
        right.commit(0, "race-right", &right_request)
    });
    barrier.wait();
    let left_result = left_thread.join().unwrap();
    let right_result = right_thread.join().unwrap();
    assert_ne!(left_result.is_ok(), right_result.is_ok());
    let final_snapshot = writable(ProjectStore::open(&racing_root).unwrap().load().unwrap());
    assert_eq!(final_snapshot.generation, 1);
    assert!(
        final_snapshot.project_name == "race left" || final_snapshot.project_name == "race right"
    );
}

#[test]
fn decision_lifecycle_changes_only_status_and_appends_superseding_intent() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("decision-project");
    let (store, base) = create_store(&root);

    let mut first_request = base.clone();
    first_request.updated_at = fixture::at(2026, 10, 5, 12);
    first_request.decisions.push(decision(
        "decision-1",
        tool_project::DecisionStatus::Active,
        Vec::new(),
    ));
    let first = store
        .commit(0, "activate-decision-1", &first_request)
        .unwrap();

    let mut replacement_request = first.snapshot.clone();
    replacement_request.decisions[0].status = tool_project::DecisionStatus::Superseded;
    replacement_request.decisions.push(decision(
        "decision-2",
        tool_project::DecisionStatus::Active,
        vec!["decision-1".to_owned()],
    ));
    let replacement = store
        .commit(1, "replace-decision-1", &replacement_request)
        .unwrap();
    assert_eq!(
        replacement.snapshot.decisions[0].status,
        tool_project::DecisionStatus::Superseded
    );
    assert_eq!(
        replacement.snapshot.decisions[1].supersedes,
        vec!["decision-1".to_owned()]
    );

    let mut resurrected = replacement.snapshot.clone();
    resurrected.decisions[0].status = tool_project::DecisionStatus::Active;
    assert!(matches!(
        store.commit(2, "reactivate-superseded-decision", &resurrected),
        Err(StoreError::InvalidArgument(_))
    ));

    let mut invalid_edit = replacement.snapshot.clone();
    invalid_edit.decisions[0].rationale = "Rewrite the historical reason".to_owned();
    assert!(matches!(
        store.commit(2, "rewrite-decision-history", &invalid_edit),
        Err(StoreError::InvalidArgument(_))
    ));
    assert_eq!(writable(store.load().unwrap()).generation, 2);
}

#[test]
fn appended_decisions_cannot_supersede_another_appended_active_decision() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("same-commit-decision-project");
    let (store, base) = create_store(&root);

    let mut request = base.clone();
    request.updated_at = fixture::at(2026, 10, 5, 12);
    request.decisions.push(decision(
        "decision-1",
        tool_project::DecisionStatus::Active,
        Vec::new(),
    ));
    request.decisions.push(decision(
        "decision-2",
        tool_project::DecisionStatus::Active,
        vec!["decision-1".to_owned()],
    ));

    assert!(matches!(
        store.commit(0, "append-conflicting-decisions", &request),
        Err(StoreError::InvalidArgument(_))
    ));
    assert!(writable(store.load().unwrap()).decisions.is_empty());
}

#[test]
fn every_interruption_point_exposes_only_the_old_or_complete_new_snapshot() {
    for point in [
        tool_store::StoreFaultPoint::AfterSnapshotTempWrite,
        tool_store::StoreFaultPoint::AfterSnapshotSync,
        tool_store::StoreFaultPoint::AfterSnapshotPublish,
        tool_store::StoreFaultPoint::AfterPointerSync,
        tool_store::StoreFaultPoint::AfterPointerSwitch,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("fault-project");
        let (store, base) = create_store(&root);
        let request = edited(&base, "fully committed");
        assert!(matches!(
            store.commit_with_fault(0, "fault-operation", &request, point),
            Err(StoreError::InjectedInterruption(_))
        ));

        let reopened = ProjectStore::open(&root).unwrap();
        let saved = writable(reopened.load().unwrap());
        if point == tool_store::StoreFaultPoint::AfterPointerSwitch {
            assert_eq!(saved.generation, 1);
            assert_eq!(saved.project_name, "fully committed");
            let retry = reopened.commit(0, "fault-operation", &request).unwrap();
            assert_eq!(retry.disposition, CommitDisposition::AlreadyApplied);
        } else {
            assert_eq!(saved.generation, 0);
            assert_eq!(saved.project_name, "U01 fixture");
        }
    }
}

#[test]
fn future_formats_and_required_features_are_read_only_and_byte_preserving() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("future-pointer");
    let (store, base) = create_store(&root);
    let pointer_path = root.join("CURRENT");
    let mut pointer: Value = serde_json::from_slice(&fs::read(&pointer_path).unwrap()).unwrap();
    pointer["format_version"] = json!(999);
    let future_pointer = serde_json::to_vec(&pointer).unwrap();
    fs::write(&pointer_path, &future_pointer).unwrap();
    match store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert!(matches!(
                read_only.reason,
                ReadOnlyReason::UnsupportedFormat { found: 999, .. }
            ));
            assert_eq!(
                read_only.raw_pointer_bytes.as_deref(),
                Some(future_pointer.as_slice())
            );
        }
        StoreLoad::Writable(_) => panic!("future pointer must remain read-only"),
    }
    assert!(matches!(
        store.commit(0, "must-not-write", &edited(&base, "replacement")),
        Err(StoreError::ReadOnly(_))
    ));
    assert_eq!(fs::read(&pointer_path).unwrap(), future_pointer);
    assert!(!root.join(".write.lock").exists());

    let snapshot_root = temp.path().join("future-snapshot");
    let (snapshot_store, _) = create_store(&snapshot_root);
    let bytes = rewrite_snapshot(&snapshot_root, |value| value["format_version"] = json!(999));
    match snapshot_store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert!(matches!(
                read_only.reason,
                ReadOnlyReason::UnsupportedFormat { found: 999, .. }
            ));
            assert_eq!(
                read_only.raw_snapshot_bytes.as_deref(),
                Some(bytes.as_slice())
            );
        }
        StoreLoad::Writable(_) => panic!("future snapshot must remain read-only"),
    }
    assert!(matches!(
        snapshot_store.commit(
            0,
            "must-not-write",
            &edited(
                &fixture::empty_project(tool_project::studio_order_template(), "store-project"),
                "replacement"
            )
        ),
        Err(StoreError::ReadOnly(_))
    ));
    let snapshot_pointer: Value =
        serde_json::from_slice(&fs::read(snapshot_root.join("CURRENT")).unwrap()).unwrap();
    let future_snapshot_path = snapshot_root
        .join("snapshots")
        .join(snapshot_pointer["snapshot_file"].as_str().unwrap());
    assert_eq!(fs::read(future_snapshot_path).unwrap(), bytes);
    assert!(!snapshot_root.join(".write.lock").exists());

    let feature_root = temp.path().join("future-feature");
    let (feature_store, _) = create_store(&feature_root);
    rewrite_snapshot(&feature_root, |value| {
        value["required_features"] = json!(["unrecognized_rule_engine"])
    });
    match feature_store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert!(matches!(
                read_only.reason,
                ReadOnlyReason::UnsupportedRequiredFeature(_)
            ));
            assert!(read_only.raw_snapshot_bytes.is_some());
        }
        StoreLoad::Writable(_) => panic!("unknown required feature must remain read-only"),
    }
}

#[test]
fn damaged_pointer_or_checksum_is_diagnostic_and_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("corrupt-pointer");
    let (store, base) = create_store(&root);
    let pointer_path = root.join("CURRENT");
    let broken_pointer = b"{\"format_version\":1".to_vec();
    fs::write(&pointer_path, &broken_pointer).unwrap();
    assert!(matches!(
        store.load().unwrap(),
        StoreLoad::ReadOnly(ref read_only) if matches!(read_only.reason, ReadOnlyReason::MalformedPointer(_))
    ));
    assert!(matches!(
        store.commit(0, "must-not-replace", &edited(&base, "replacement")),
        Err(StoreError::ReadOnly(_))
    ));
    assert_eq!(fs::read(&pointer_path).unwrap(), broken_pointer);

    let checksum_root = temp.path().join("corrupt-snapshot");
    let (checksum_store, _) = create_store(&checksum_root);
    let pointer: Value =
        serde_json::from_slice(&fs::read(checksum_root.join("CURRENT")).unwrap()).unwrap();
    let snapshot_path = checksum_root
        .join("snapshots")
        .join(pointer["snapshot_file"].as_str().unwrap());
    let mut bytes = fs::read(&snapshot_path).unwrap();
    bytes[0] ^= 0x01;
    fs::write(&snapshot_path, &bytes).unwrap();
    match checksum_store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert_eq!(read_only.reason, ReadOnlyReason::ChecksumMismatch);
            assert_eq!(
                read_only.raw_snapshot_bytes.as_deref(),
                Some(bytes.as_slice())
            );
        }
        StoreLoad::Writable(_) => panic!("checksum mismatch must be read-only"),
    }
    assert_eq!(fs::read(&snapshot_path).unwrap(), bytes);
}

#[test]
fn orphan_snapshots_are_ignored_and_invalid_candidates_do_not_advance_current() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("orphan-project");
    let (store, base) = create_store(&root);
    fs::write(
        root.join("snapshots/999-orphan.json"),
        b"not a committed snapshot",
    )
    .unwrap();
    assert_eq!(writable(store.load().unwrap()).generation, 0);

    let mut invalid = edited(&base, "invalid candidate");
    invalid.active_spec.spec_id = "missing-spec".to_owned();
    assert!(matches!(
        store.commit(0, "invalid-snapshot", &invalid),
        Err(StoreError::InvalidSnapshot(_))
    ));
    assert_eq!(writable(store.load().unwrap()).generation, 0);
}

#[cfg(unix)]
#[test]
fn symlinked_project_or_current_pointer_is_not_followed_or_written() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real-project");
    let (store, base) = create_store(&real);
    let alias = temp.path().join("project-link");
    symlink(&real, &alias).unwrap();
    assert!(matches!(
        ProjectStore::open(&alias),
        Err(StoreError::UnsafePath(_))
    ));

    let pointer_path = real.join("CURRENT");
    let saved_pointer = temp.path().join("saved-pointer");
    fs::rename(&pointer_path, &saved_pointer).unwrap();
    let external = temp.path().join("external-pointer-bytes");
    fs::write(&external, b"external content stays untouched").unwrap();
    symlink(&external, &pointer_path).unwrap();
    match store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => assert_eq!(read_only.reason, ReadOnlyReason::UnsafePath),
        StoreLoad::Writable(_) => panic!("symlinked pointer must be read-only"),
    }
    assert!(matches!(
        store.commit(0, "symlink-write", &edited(&base, "replacement")),
        Err(StoreError::ReadOnly(_))
    ));
    assert_eq!(
        fs::read(external).unwrap(),
        b"external content stays untouched"
    );
    fs::remove_file(pointer_path).unwrap();
    fs::rename(saved_pointer, real.join("CURRENT")).unwrap();

    let external_root = temp.path().join("external-project");
    let (_, external_base) = create_store(&external_root);
    let moved_real = temp.path().join("moved-real-project");
    fs::rename(&real, &moved_real).unwrap();
    symlink(&external_root, &real).unwrap();
    match store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert_eq!(read_only.reason, ReadOnlyReason::UnsafePath)
        }
        StoreLoad::Writable(_) => panic!("replaced project directory must be read-only"),
    }
    assert!(matches!(
        store.commit(0, "symlink-root-write", &edited(&base, "must not escape")),
        Err(StoreError::ReadOnly(_))
    ));
    assert_eq!(
        writable(ProjectStore::open(&external_root).unwrap().load().unwrap()),
        external_base
    );
}

#[cfg(unix)]
#[test]
fn project_parent_replaced_by_symlink_cannot_redirect_an_open_store() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("project-parent");
    fs::create_dir(&parent).unwrap();
    let root = parent.join("project");
    let (store, original) = create_store(&root);

    let external_parent = temp.path().join("external-parent");
    fs::create_dir(&external_parent).unwrap();
    let external_root = external_parent.join("project");
    let mut external_initial =
        fixture::empty_project(tool_project::studio_order_template(), "store-project");
    external_initial.project_name = "External project".to_owned();
    let external_store =
        ProjectStore::create(&external_root, &external_initial, "create-external-project").unwrap();
    let external_before = writable(external_store.load().unwrap());

    let moved_parent = temp.path().join("moved-project-parent");
    fs::rename(&parent, &moved_parent).unwrap();
    symlink(&external_parent, &parent).unwrap();

    match store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert_eq!(read_only.reason, ReadOnlyReason::UnsafePath)
        }
        StoreLoad::Writable(snapshot) => assert_eq!(
            snapshot.project_name, original.project_name,
            "a replaced parent path must not redirect the store into another project"
        ),
    }
    assert_eq!(writable(external_store.load().unwrap()), external_before);
}

#[cfg(unix)]
#[test]
fn opened_project_root_stays_pinned_when_its_path_is_replaced() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let moved_root = temp.path().join("moved-project");
    let (store, original) = create_store(&root);

    fs::rename(&root, &moved_root).unwrap();
    let mut replacement =
        fixture::empty_project(tool_project::studio_order_template(), "store-project");
    replacement.project_name = "Replacement directory".to_owned();
    let replacement_store =
        ProjectStore::create(&root, &replacement, "create-replacement-project").unwrap();

    match store.load().unwrap() {
        StoreLoad::ReadOnly(read_only) => {
            assert_eq!(read_only.reason, ReadOnlyReason::UnsafePath)
        }
        StoreLoad::Writable(snapshot) => assert_eq!(
            snapshot, original,
            "a replaced project path must never load data from the replacement directory"
        ),
    }
    assert!(matches!(
        store.commit(
            0,
            "commit-after-project-move",
            &edited(&original, "must not redirect")
        ),
        Err(StoreError::ReadOnly(_))
    ));

    assert_eq!(
        writable(ProjectStore::open(&moved_root).unwrap().load().unwrap()),
        original
    );
    assert_eq!(
        writable(replacement_store.load().unwrap()).project_name,
        "Replacement directory"
    );
}

#[cfg(windows)]
#[test]
fn windows_store_handle_does_not_follow_a_replaced_ancestor_path() {
    let temp = tempfile::tempdir().unwrap();
    let original_ancestor = temp.path().join("original-container");
    let original_parent = original_ancestor.join("store-parent");
    fs::create_dir_all(&original_parent).unwrap();
    let original_root = original_parent.join("project");
    let (store, original) = create_store(&original_root);

    let external_ancestor = temp.path().join("external-container");
    let external_parent = external_ancestor.join("store-parent");
    fs::create_dir_all(&external_parent).unwrap();
    let external_root = external_parent.join("project");
    let mut external_initial =
        fixture::empty_project(tool_project::studio_order_template(), "external-project");
    external_initial.project_name = "External project".to_owned();
    drop(
        ProjectStore::create(&external_root, &external_initial, "create-external-project").unwrap(),
    );

    let moved_ancestor = temp.path().join("moved-container");
    if fs::rename(&original_ancestor, &moved_ancestor).is_err() {
        assert_eq!(
            writable(store.load().unwrap()).project_id,
            original.project_id
        );
        return;
    }
    fs::rename(&external_ancestor, &original_ancestor).unwrap();

    match store.load().unwrap() {
        StoreLoad::Writable(snapshot) => assert_eq!(
            snapshot.project_id, original.project_id,
            "a moved ancestor path must not redirect the store into a different project"
        ),
        StoreLoad::ReadOnly(read_only) => assert!(matches!(
            read_only.reason,
            ReadOnlyReason::UnsafePath | ReadOnlyReason::UnsupportedPlatform
        )),
    }
}

#[test]
fn project_creation_never_overwrites_an_existing_directory() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("owned-data");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("keep.txt"), b"keep original bytes").unwrap();
    let snapshot = fixture::empty_project(tool_project::studio_order_template(), "store-project");
    assert!(matches!(
        ProjectStore::create(&root, &snapshot, "create-project-1"),
        Err(StoreError::AlreadyExists)
    ));
    assert_eq!(
        fs::read(root.join("keep.txt")).unwrap(),
        b"keep original bytes"
    );
}
