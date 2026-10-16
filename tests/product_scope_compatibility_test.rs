//! Literal pre-migration scalar histories; expectations are frozen at c527ef89.
#[path = "support/scalar_compatibility.rs"]
mod compatibility;
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

use compatibility::*;
use fixture::*;
use product_backup::*;
use product_contract::*;
use product_decisions::*;
use product_runtime::LocalRuntime;
use product_store::scope::*;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};

fn fixture_dir() -> PathBuf {
    PathBuf::from(file!())
        .parent()
        .unwrap()
        .join("fixtures/scalar_compatibility")
}

#[test]
#[ignore = "Explicit reproduction only: set SCALAR_FIXTURE_OUTPUT to a new directory"]
fn reproduce_scalar_baseline() {
    let out =
        PathBuf::from(std::env::var_os("SCALAR_FIXTURE_OUTPUT").expect("explicit output path"));
    assert!(out.is_absolute());
    fs::create_dir(&out).expect("output directory must not already exist");
    generate(&out);
}

#[test]
fn frozen_scalar_bytes_and_identities_are_exact() {
    let manifest_bytes = fs::read(fixture_dir().join("manifest.json")).unwrap();
    assert_eq!(
        sha256(&manifest_bytes),
        "8dcbeed21d003aa164c0fee934cd68ae4ba58f3daa365b01dfad5c6564cdc02f"
    );
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(
        manifest["baseline"],
        "c527ef892ddec9af6ed37431898133ce36054c62"
    );
    let names: std::collections::BTreeSet<_> = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        CURRENT_CASES.into_iter().chain(["legacy-v1"]).collect()
    );
    assert_eq!(manifest["cases"].as_array().unwrap().len(), names.len());
    let mut total_bytes = 0;
    for case in manifest["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let bytes = fs::read(fixture_dir().join(format!("{name}.gmbak"))).unwrap();
        assert!(
            bytes.len() <= 4 * 1024 * 1024,
            "{name}: keep the baseline bounded"
        );
        total_bytes += bytes.len();
        assert_eq!(inventory(name, &bytes), *case, "{name}");
    }
    assert!(total_bytes <= 8 * 1024 * 1024);
}

const CURRENT_CASES: [&str; 5] = ["active", "evolved", "paired", "withdrawn", "prospective"];
fn frozen_bytes(name: &str) -> Vec<u8> {
    fs::read(fixture_dir().join(format!("{name}.gmbak"))).unwrap()
}
fn converted(bytes: &[u8]) -> VerifiedBackup {
    let BackupIntake::UpgradeRequired(old) = inspect_backup(bytes).unwrap() else {
        panic!("expected frozen old backup")
    };
    // Expectations remain literal pre-change bytes. Use only the verified
    // conversion's immutable target; fresh recovery is exercised below.
    let mut envelope: Value = serde_json::from_slice(bytes).unwrap();
    envelope["payload"]["snapshot"] = serde_json::to_value(old.snapshot()).unwrap();
    VerifiedBackup::from_bytes(&rebind_backup(&mut envelope)).unwrap()
}

fn frozen(name: &str) -> VerifiedBackup {
    converted(&frozen_bytes(name))
}

fn bundle_from(bytes: &[u8]) -> IntentionBundle {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    serde_json::from_value(envelope["payload"]["intentions"].clone()).unwrap()
}

#[test]
fn frozen_scalar_histories_reconstruct_reopen_replay_and_recover() {
    let dir = tempdir();
    let mut snapshots = std::collections::BTreeMap::new();
    for name in CURRENT_CASES {
        let bytes = frozen_bytes(name);
        let backup = converted(&bytes);
        assert_eq!(inspect_backup(&bytes).unwrap().to_bytes().unwrap(), bytes);
        let snapshot = backup.snapshot();
        assert_eq!(snapshot.version, 3);
        assert_eq!(snapshot.scalar_scope().unwrap().version, 1);
        let bundle = bundle_from(&backup.to_bytes().unwrap());
        validate_bundle(snapshot, &bundle).unwrap();
        for manifest in snapshot.scalar_scope().unwrap().compositions.values() {
            let reconstructed = compile_test_manifest(snapshot, manifest).unwrap();
            let retained = snapshot
                .programs
                .iter()
                .find(|p| canonical_digest(IdentityDomain::Source, p).unwrap() == manifest.output)
                .unwrap();
            assert_eq!(
                &reconstructed, retained,
                "{name}: scalar compiler/source/provenance drift"
            );
            assert_eq!(
                canonical_bytes(&reconstructed).unwrap(),
                canonical_bytes(retained).unwrap()
            );
        }
        let path = dir.path().join(name);
        let recovered = backup.recover_new(&path).unwrap();
        drop(recovered);
        let reopened = open_verified(&path, Some(&backup.summary().unwrap().identity)).unwrap();
        assert_eq!(&reopened.snapshot, snapshot);
        assert_eq!(
            fs::read(path.join(format!(
                    "object-{}.json",
                    canonical_digest(IdentityDomain::Data, snapshot)
                        .unwrap()
                        .as_str()
                )))
            .unwrap(),
            canonical_bytes(snapshot).unwrap()
        );
        let archive = IntentArchive::new(reopened.store.clone());
        assert_eq!(
            canonical_bytes(&archive.export_for(snapshot).unwrap()).unwrap(),
            canonical_bytes(&bundle).unwrap()
        );
        let engine = DecisionEngine::new(LocalRuntime::default(), archive.clone());
        assert_eq!(
            engine.check_current(snapshot).unwrap().disposition,
            CheckDisposition::Ready,
            "{name}"
        );
        for decision in &snapshot.decisions.decisions {
            let scenes = archive.accepted_scenes(decision).unwrap();
            assert!(!scenes.is_empty());
        }
        if snapshot
            .decisions
            .decisions
            .iter()
            .any(|d| d.status == DecisionStatus::Pending)
        {
            assert!(
                !engine.discovery_scenes(snapshot).unwrap().is_empty(),
                "{name}: pending scenes must replay"
            );
        }
        let recaptured = VerifiedBackup::capture(&reopened.store).unwrap();
        assert_eq!(recaptured.to_bytes().unwrap(), backup.to_bytes().unwrap());
        // A normal operation remains available after verified reopen. The old
        // immutable object and original fixture must remain byte-identical.
        apply(&reopened.store, "continued-export", invoke("export", &[]));
        assert_eq!(
            fs::read(path.join(format!("object-{}.json", bundle.snapshot_digest().as_str())))
                .unwrap(),
            canonical_bytes(snapshot).unwrap()
        );
        assert_eq!(frozen_bytes(name), bytes);
        snapshots.insert(name, snapshot.clone());
    }
    let active = &snapshots["active"];
    let evolved = &snapshots["evolved"];
    let paired = &snapshots["paired"];
    let withdrawn = &snapshots["withdrawn"];
    let prospective = &snapshots["prospective"];
    assert_eq!(active.scalar_scope().unwrap().layers.len(), 1);
    assert_eq!(active.scalar_scope().unwrap().initializations.len(), 1);
    assert!(active
        .data
        .records
        .iter()
        .any(|row| row.values.get("done") == Some(&DataValue::Boolean { value: true })));
    assert!(active
        .data
        .records
        .iter()
        .any(|row| row.values.keys().any(|key| key.starts_with("gm_scope_"))));
    assert!(!active.data.events.is_empty());
    assert!(!active.artifacts.is_empty());
    assert!(!active.scalar_scope().unwrap().correspondences.is_empty());
    assert!(active
        .decisions
        .decisions
        .iter()
        .any(|d| d.id == "ordinary-promise" && d.status == DecisionStatus::Active));
    assert_eq!(
        active.scalar_scope().unwrap().layers,
        evolved.scalar_scope().unwrap().layers
    );
    assert_eq!(active.data, evolved.data);
    assert_eq!(active.data, withdrawn.data);
    assert_eq!(active.artifacts, withdrawn.artifacts);
    assert_eq!(paired.scalar_scope().unwrap().rehearsals.len(), 2);
    assert_eq!(
        active.scalar_scope().unwrap().layers,
        withdrawn.scalar_scope().unwrap().layers
    );
    assert!(paired
        .scalar_scope()
        .unwrap()
        .rehearsals
        .values()
        .all(|r| r.layer.is_none()));
    assert!(
        withdrawn.scalar_scope().unwrap().compositions[&withdrawn.active_revision]
            .active
            .is_empty()
    );
    assert_eq!(withdrawn.decisions.decisions, evolved.decisions.decisions);
    assert!(prospective.scalar_scope().unwrap().layers.is_empty());
    assert!(prospective.scalar_scope().unwrap().compositions.is_empty());
    assert!(prospective
        .scalar_scope()
        .unwrap()
        .initializations
        .is_empty());
    assert_eq!(prospective.scalar_scope().unwrap().rehearsals.len(), 1);
    assert_eq!(
        prospective.decisions.decisions[0].status,
        DecisionStatus::Pending
    );
    assert_eq!(
        prospective.active_revision,
        canonical_digest(IdentityDomain::Source, &prospective.programs[0]).unwrap()
    );
}

#[test]
fn frozen_checkpoint_keeps_its_original_name_bytes_and_discovery_identity() {
    let dir = tempdir();
    let bytes = frozen_bytes("withdrawn");
    let backup = converted(&bytes);
    let store = backup.recover_new(&dir.path().join("tool")).unwrap();
    fs::create_dir(dir.path().join("shelf")).unwrap();
    let locations = product_locations::ToolLocations::chosen(dir.path().join("shelf")).unwrap();
    let identity = backup.summary().unwrap().identity;
    let instance = canonical_digest(IdentityDomain::Evidence, &"scalar-frozen-instance").unwrap();
    let shelf = CheckpointShelf::for_tool(&locations, &identity, &instance).unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(fixture_dir().join("manifest.json")).unwrap()).unwrap();
    let case = manifest["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "withdrawn")
        .unwrap();
    let name = case["checkpoint_name"].as_str().unwrap();
    // Discovery starts with an actual frozen checkpoint, not today's writer.
    fs::write(shelf.path().join(name), &bytes).unwrap();
    let found = shelf.newest_verified().unwrap();
    assert!(found.issues.is_empty());
    assert_eq!(found.recovery.unwrap().to_bytes().unwrap(), bytes);
    let receipt = shelf.capture(&store).unwrap();
    assert_ne!(receipt.path.file_name().unwrap().to_str().unwrap(), name);
    assert_eq!(fs::read(shelf.path().join(name)).unwrap(), bytes);
    assert_eq!(receipt.digest, *backup.digest());
    assert_eq!(fs::read(receipt.path).unwrap(), backup.to_bytes().unwrap());
}

#[test]
fn frozen_format_one_requires_explicit_preserving_upgrade() {
    let dir = tempdir();
    let bytes = frozen_bytes("legacy-v1");
    let raw: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(raw["payload"]["snapshot"]["version"], 1);
    assert!(raw["payload"]["snapshot"].get("scope").is_none());
    assert_eq!(
        VerifiedBackup::from_bytes(&bytes).unwrap_err().kind,
        product_locations::IssueKind::UpgradeRequired
    );
    let BackupIntake::UpgradeRequired(legacy) = inspect_backup(&bytes).unwrap() else {
        panic!("expected explicit V1 intake")
    };
    let path = dir.path().join("legacy");
    let summary = legacy.upgrade_recover_new(&path, |_| {}).unwrap();
    assert!(summary.restart_required);
    assert_eq!(legacy.original_bytes(), bytes);
    let opened = open_verified(&path, None).unwrap();
    let mut projected = serde_json::to_value(&opened.snapshot).unwrap();
    assert_eq!(
        opened.snapshot.scalar_scope().unwrap(),
        &ScopeState::default()
    );
    projected.as_object_mut().unwrap().remove("scope");
    projected["version"] = json!(1);
    assert_eq!(projected, raw["payload"]["snapshot"]);
    let digest = canonical_digest(IdentityDomain::Data, &projected).unwrap();
    assert_eq!(
        fs::read(path.join(format!("object-{}.json", digest.as_str()))).unwrap(),
        canonical_bytes(&projected).unwrap()
    );
    let bundle = serde_json::to_value(
        IntentArchive::new(opened.store.clone())
            .export_for(&opened.snapshot)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(bundle["objects"], raw["payload"]["intentions"]["objects"]);
    let engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(opened.store.clone()),
    );
    assert_eq!(
        engine.check_current(&opened.snapshot).unwrap().disposition,
        CheckDisposition::Ready
    );
    apply(&opened.store, "continued-legacy", invoke("export", &[]));
    assert_eq!(frozen_bytes("legacy-v1"), bytes);
}

fn rebind_backup(raw: &mut Value) -> Vec<u8> {
    raw["payload"]["intentions"]["snapshot"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Data, &raw["payload"]["snapshot"]).unwrap(),
    )
    .unwrap();
    raw["digest"] =
        serde_json::to_value(canonical_digest(IdentityDomain::Evidence, &raw["payload"]).unwrap())
            .unwrap();
    canonical_bytes(raw).unwrap()
}

#[test]
fn rehashed_frozen_proofs_and_packages_reject_tampering() {
    for name in CURRENT_CASES {
        let original = frozen_bytes(name);
        let snapshot = frozen(name).snapshot().clone();
        let mut altered = snapshot.clone();
        if name == "prospective" {
            altered
                .scalar_scope_mut()
                .unwrap()
                .rehearsals
                .values_mut()
                .next()
                .unwrap()
                .recorded_revision += 1;
        } else if name == "paired" {
            altered
                .scalar_scope_mut()
                .unwrap()
                .rehearsals
                .values_mut()
                .next()
                .unwrap()
                .witnesses
                .clear();
        } else if name == "withdrawn" {
            // Inactive scalar proof is still validated.
            altered
                .scalar_scope_mut()
                .unwrap()
                .layers
                .values_mut()
                .next()
                .unwrap()
                .version = 99;
        } else if name == "evolved" {
            let key = altered
                .scalar_scope()
                .unwrap()
                .correspondences
                .keys()
                .next()
                .unwrap()
                .clone();
            let mut receipt = altered
                .scalar_scope_mut()
                .unwrap()
                .correspondences
                .remove(&key)
                .unwrap();
            receipt.proof.original.records[0]
                .values
                .insert("name".into(), text("Altered historical work"));
            altered
                .scalar_scope_mut()
                .unwrap()
                .correspondences
                .insert(receipt.proof.identity().unwrap(), receipt);
        } else {
            altered.scalar_scope_mut().unwrap().initializations[0].capture_day += 1;
            // Rehash the nested initialization link as well as the outer backup;
            // the retained basis must still prove this claimed historical day.
            altered.scalar_scope_mut().unwrap().adoptions[0].initialization = Some(
                canonical_digest(
                    IdentityDomain::Adoption,
                    &altered.scalar_scope().unwrap().initializations[0],
                )
                .unwrap(),
            );
        }
        assert!(altered.validate().is_err(), "{name}");
        let mut raw: Value = serde_json::from_slice(&original).unwrap();
        raw["payload"]["snapshot"] = serde_json::to_value(altered).unwrap();
        assert!(
            inspect_backup(&rebind_backup(&mut raw)).is_err(),
            "{name}: resealed proof forgery"
        );
        let mut raw: Value = serde_json::from_slice(&original).unwrap();
        let objects = raw["payload"]["intentions"]["objects"]
            .as_array_mut()
            .unwrap();
        assert!(!objects.is_empty());
        objects.pop();
        assert!(
            inspect_backup(&rebind_backup(&mut raw)).is_err(),
            "{name}: missing reachable package"
        );
        let mut raw: Value = serde_json::from_slice(&original).unwrap();
        raw["payload"]["intentions"]["objects"][0]["bytes"][0] = json!(b' ');
        assert!(
            inspect_backup(&rebind_backup(&mut raw)).is_err(),
            "{name}: altered package bytes"
        );
        assert_eq!(frozen_bytes(name), original);
    }
}

#[test]
fn scalar_adapters_reject_unsupported_versions_without_mutating_history() {
    for name in ["active", "withdrawn", "prospective"] {
        // Decode literal pre-refactor bytes, not a new family serializer.
        let raw: Value = serde_json::from_slice(&frozen_bytes(name)).unwrap();
        let original = frozen(name).snapshot().clone();
        for empty in [false, true] {
            let mut snapshot = original.clone();
            if empty {
                snapshot.scope = ScopeEnvelope::ScalarV1(ScopeState::default());
            }
            let bytes = canonical_bytes(&snapshot).unwrap();
            assert_eq!(
                serde_json::to_value(snapshot.scalar_scope().unwrap()).unwrap(),
                if empty {
                    serde_json::to_value(ScopeState::default()).unwrap()
                } else {
                    raw["payload"]["snapshot"]["scope"].clone()
                }
            );
            let expected = snapshot.scalar_scope().unwrap().clone();
            assert_eq!(snapshot.scalar_scope_mut().unwrap(), &expected);
            assert_eq!(canonical_bytes(&snapshot).unwrap(), bytes);
            for version in [0, 2, 99] {
                let mut unsupported = snapshot.clone();
                unsupported.scalar_scope_mut().unwrap().version = version;
                let bytes = canonical_bytes(&unsupported).unwrap();
                assert!(unsupported.scalar_scope().is_err(), "{name}: {version}");
                assert!(unsupported.scalar_scope_mut().is_err(), "{name}: {version}");
                assert_eq!(canonical_bytes(&unsupported).unwrap(), bytes);
            }
            for version in [1, 2, 99] {
                let mut unsupported = snapshot.clone();
                unsupported.version = version;
                let bytes = canonical_bytes(&unsupported).unwrap();
                assert!(
                    unsupported.scalar_scope().is_err(),
                    "{name}: format {version}"
                );
                assert!(
                    unsupported.scalar_scope_mut().is_err(),
                    "{name}: format {version}"
                );
                assert_eq!(canonical_bytes(&unsupported).unwrap(), bytes);
            }
        }
    }
}
