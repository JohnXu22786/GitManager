#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_store::*;
use serde_json::json;
use std::fs;

fn make_legacy(path: &std::path::Path) -> (ProductStore, Vec<u8>, Vec<u8>) {
    let store = ProductStore::create(path, &program(false), 20000).unwrap();
    add(&store, "first", "Original work");
    let mut value = serde_json::to_value(store.load().unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("scope");
    value["version"] = json!(1);
    let bytes = canonical_bytes(&value).unwrap();
    let digest = canonical_digest(IdentityDomain::Data, &value).unwrap();
    fs::write(
        path.join(format!("object-{}.json", digest.as_str())),
        &bytes,
    )
    .unwrap();
    let pointer=canonical_bytes(&json!({"magic":"gitmanager.generated-project","version":1,"project_id":"scope-project","revision":1,"object":digest})).unwrap();
    fs::write(path.join("CURRENT"), &pointer).unwrap();
    (store, pointer, bytes)
}
#[test]
fn explicit_upgrade_preserves_format_one_bytes_and_requires_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old");
    let (store, pointer, old) = make_legacy(&path);
    assert!(matches!(store.load(), Err(StoreError::UpgradeRequired)));
    assert!(store
        .apply(
            1,
            "must-not-migrate",
            &invoke("export", &[]),
            RuntimeLimits::default()
        )
        .is_err());
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    let mut stages = vec![];
    let summary = store.upgrade_generated_project(|p| stages.push(p)).unwrap();
    assert!(summary.restart_required);
    assert!(stages.contains(&UpgradeProgress::Verified));
    assert!(stages.contains(&UpgradeProgress::RestartRequired));
    let original: serde_json::Value = serde_json::from_slice(&pointer).unwrap();
    let id = original["object"].as_str().unwrap();
    assert_eq!(
        fs::read(path.join(format!("object-{id}.json"))).unwrap(),
        old
    );
    let reopened = ProductStore::open(&path).unwrap();
    let current = reopened.load().unwrap();
    assert_eq!(current.version, 2);
    assert!(current.scope.rehearsals.is_empty());
    assert!(current.scope.correspondences.is_empty());
    assert_eq!(current.data.records.len(), 1);
    let current_pointer = fs::read(path.join("CURRENT")).unwrap();
    reopened.upgrade_generated_project(|_| {}).unwrap();
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), current_pointer);
    apply(&reopened, "after-upgrade", invoke("export", &[]));
}
#[test]
fn upgrade_faults_are_idempotent_without_ordinary_edit_migration() {
    for fault in [
        FaultPoint::AfterUpgradeObject,
        FaultPoint::AfterUpgradeJournal,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old");
        let (store, _, _) = make_legacy(&path);
        assert!(store.upgrade_with_fault(|_| {}, fault).is_err());
        let reopened = ProductStore::open(&path).unwrap();
        reopened.upgrade_generated_project(|_| {}).unwrap();
        let reloaded = ProductStore::open(&path).unwrap();
        assert_eq!(reloaded.load().unwrap().data.records.len(), 1);
        add(&reloaded, "later", "Continued work");
        assert_eq!(reloaded.load().unwrap().data.records.len(), 2);
    }
}

#[test]
fn tool_open_reports_upgrade_and_verifies_identity_before_activation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old");
    let (_, pointer, _) = make_legacy(&path);
    assert!(matches!(
        product_backup::inspect_open(&path, None).unwrap(),
        product_backup::OpenGate::UpgradeRequired(_)
    ));
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    let mut progress = vec![];
    let upgraded =
        product_backup::upgrade_open_verified(&path, None, |stage| progress.push(stage)).unwrap();
    assert!(upgraded.restart_required);
    assert!(progress.contains(&UpgradeProgress::Verified));
    assert!(progress.contains(&UpgradeProgress::RestartRequired));
    let product_backup::OpenGate::Ready(opened) =
        product_backup::inspect_open(&path, None).unwrap()
    else {
        panic!("upgrade did not open")
    };
    assert_eq!(opened.snapshot.data.records.len(), 1);
    add(&opened.store, "continued", "After upgrade");
}

fn legacy_backup_bytes(store: &ProductStore) -> Vec<u8> {
    let backup = product_backup::VerifiedBackup::capture(store).unwrap();
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&backup.to_bytes().unwrap()).unwrap();
    let snapshot = &mut envelope["payload"]["snapshot"];
    snapshot.as_object_mut().unwrap().remove("scope");
    snapshot["version"] = json!(1);
    let old_identity = canonical_digest(IdentityDomain::Data, snapshot).unwrap();
    envelope["payload"]["intentions"]["snapshot"] = serde_json::to_value(old_identity).unwrap();
    envelope["digest"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Evidence, &envelope["payload"]).unwrap(),
    )
    .unwrap();
    canonical_bytes(&envelope).unwrap()
}
#[test]
fn old_backup_upgrades_to_fresh_usable_recovery_and_preserves_original_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("source"), &program(false), 20000).unwrap();
    let record = add(&store, "old", "Original");
    let current = store.load().unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "retained".into(),
        label: "Retained calculation".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 0,
        inputs: vec![
            invoke("calculate", &[("row", reference(&record))]),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
        validity: vec![],
    };
    let accepted = product_decisions::accept_scene(
        &product_runtime::LocalRuntime::default(),
        current.program().unwrap(),
        &scene,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let engine = product_decisions::DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        product_decisions::IntentArchive::new(store.clone()),
    );
    let change = engine
        .prepare_choice(
            &store,
            current.program().unwrap(),
            product_decisions::Choice {
                id: "retained-calculation".into(),
                request: "Keep the observed calculation".into(),
                rationale: None,
                scope: DecisionScope {
                    operations: ["calculate".into()].into_iter().collect(),
                    population: Population::All,
                    conditions: Values::new(),
                    excluded_records: vec![],
                    unknowns: vec![],
                },
                outcome: DecisionOutcome::KeepCurrent,
                obligations: vec![],
                binding: product_decisions::IntentionBinding::ObservedOutcome,
            },
            vec![accepted],
            "save-intention",
        )
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let before = store.load().unwrap();
    let bytes = legacy_backup_bytes(&store);
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&bytes)
            .unwrap_err()
            .kind,
        product_locations::IssueKind::UpgradeRequired
    );
    let product_backup::BackupIntake::UpgradeRequired(legacy) =
        product_backup::inspect_backup(&bytes).unwrap()
    else {
        panic!("missing legacy gate")
    };
    let path = dir.path().join("recovered");
    let summary = legacy.upgrade_recover_new(&path, |_| {}).unwrap();
    assert!(summary.restart_required);
    assert_eq!(legacy.original_bytes(), bytes.as_slice());
    let recovered = product_backup::open_verified(&path, None).unwrap();
    assert_eq!(recovered.snapshot.data, before.data);
    assert_eq!(recovered.snapshot.decisions, before.decisions);
    let restored_engine = product_decisions::DecisionEngine::new(
        product_runtime::LocalRuntime::default(),
        product_decisions::IntentArchive::new(recovered.store.clone()),
    );
    assert_eq!(
        restored_engine
            .check_current(&recovered.snapshot)
            .unwrap()
            .disposition,
        product_decisions::CheckDisposition::Ready
    );
    let original_bundle: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let new_backup = product_backup::VerifiedBackup::capture(&recovered.store).unwrap();
    let new_bundle: serde_json::Value =
        serde_json::from_slice(&new_backup.to_bytes().unwrap()).unwrap();
    assert_eq!(
        original_bundle["payload"]["intentions"]["objects"],
        new_bundle["payload"]["intentions"]["objects"]
    );

    action(&recovered.store, "continued", "wait", &record);
    let current = recovered.store.load().unwrap();
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), current);
    assert_eq!(store.load().unwrap(), before);
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let old_snapshot = canonical_bytes(&original["payload"]["snapshot"]).unwrap();
    let digest = canonical_digest(IdentityDomain::Data, &original["payload"]["snapshot"]).unwrap();
    assert_eq!(
        fs::read(path.join(format!("object-{}.json", digest.as_str()))).unwrap(),
        old_snapshot
    );
}
#[test]
fn interrupted_old_backup_upgrade_retries_same_fresh_target_without_duplicate_work() {
    for fault in [
        FaultPoint::AfterUpgradeObject,
        FaultPoint::AfterUpgradeJournal,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let source =
            ProductStore::create(dir.path().join("source"), &program(false), 20000).unwrap();
        add(&source, "old", "Original");
        let bytes = legacy_backup_bytes(&source);
        let legacy = product_backup::LegacyBackup::from_bytes(&bytes).unwrap();
        let path = dir.path().join("upgrade");
        assert!(legacy
            .upgrade_recover_with_fault(&path, |_| {}, fault)
            .is_err());
        legacy.upgrade_recover_new(&path, |_| {}).unwrap();
        let store = product_backup::open_verified(&path, None).unwrap().store;
        assert_eq!(store.load().unwrap().data.records.len(), 1);
        add(&store, "later", "Later");
        assert_eq!(store.load().unwrap().data.records.len(), 2);
        assert_eq!(legacy.original_bytes(), bytes.as_slice());
    }
}

#[test]
fn legacy_backup_refuses_unknown_versions_and_corrupted_intention_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let bytes = legacy_backup_bytes(&store);
    let mut broken: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    broken["payload"]["intentions"]["snapshot"] = json!("0".repeat(64));
    broken["digest"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Evidence, &broken["payload"]).unwrap(),
    )
    .unwrap();
    assert!(product_backup::LegacyBackup::from_bytes(&canonical_bytes(&broken).unwrap()).is_err());
    let mut future: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    future["payload"]["snapshot"]["version"] = json!(99);
    future["digest"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Evidence, &future["payload"]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&canonical_bytes(&future).unwrap())
            .unwrap_err()
            .kind,
        product_locations::IssueKind::Unsupported
    );
}
