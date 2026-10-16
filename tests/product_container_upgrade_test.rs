//! Atomic format-3 cutover exercised against literal pre-change containers.
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
use product_backup::*;
use product_contract::*;
use product_decisions::*;
use product_store::*;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
const CASES: [&str; 6] = [
    "legacy-v1",
    "active",
    "evolved",
    "paired",
    "withdrawn",
    "prospective",
];
fn golden(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(file!())
            .parent()
            .unwrap()
            .join(format!("fixtures/scalar_compatibility/{name}.gmbak")),
    )
    .unwrap()
}
fn install(path: &Path, name: &str, pretty: bool) -> (ProductStore, Value, Vec<u8>) {
    let envelope: Value = serde_json::from_slice(&golden(name)).unwrap();
    let snapshot = envelope["payload"]["snapshot"].clone();
    let digest = canonical_digest(IdentityDomain::Data, &snapshot).unwrap();
    fs::create_dir(path).unwrap();
    let bytes = if pretty {
        serde_json::to_vec_pretty(&snapshot).unwrap()
    } else {
        canonical_bytes(&snapshot).unwrap()
    };
    fs::write(
        path.join(format!("object-{}.json", digest.as_str())),
        &bytes,
    )
    .unwrap();
    fs::write(path.join("CURRENT"), canonical_bytes(&json!({"magic":"gitmanager.generated-project","version":snapshot["version"],"revision":snapshot["revision"],"project_id":snapshot["data"]["project_id"],"object":digest})).unwrap()).unwrap();
    for object in envelope["payload"]["intentions"]["objects"]
        .as_array()
        .unwrap()
    {
        let bytes: Vec<u8> = serde_json::from_value(object["bytes"].clone()).unwrap();
        fs::write(
            path.join(format!(
                "extension-{}.json",
                object["digest"].as_str().unwrap()
            )),
            bytes,
        )
        .unwrap();
    }
    (ProductStore::open(path).unwrap(), snapshot, bytes)
}
fn tree(path: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    fs::read_dir(path)
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            let name = e.file_name().to_string_lossy().into_owned();
            if name == ".write.lock" || !e.file_type().unwrap().is_file() {
                None
            } else {
                Some((name, fs::read(e.path()).unwrap()))
            }
        })
        .collect()
}
fn upgrade(store: &ProductStore) -> Result<UpgradeSummary, StoreError> {
    let archive = IntentArchive::new(store.clone());
    store.upgrade_generated_project_verified(
        |_| {},
        |s| {
            archive
                .export_for(s)
                .map(|_| ())
                .map_err(|e| StoreError::Corrupt(e.to_string()))
        },
    )
}
#[test]
fn current_writes_tagged_scalar_and_normal_intake_rejects_both_old_formats() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("new"), &program(false), 20000).unwrap();
    let value = serde_json::to_value(store.load().unwrap()).unwrap();
    assert_eq!(value["version"], 3);
    assert_eq!(value["scope"]["family"], "scalar_v1");
    assert_eq!(value["scope"]["state"]["version"], 1);
    for name in CASES {
        assert!(
            matches!(VerifiedBackup::from_bytes(&golden(name)), Err(e) if e.kind==product_locations::IssueKind::UpgradeRequired),
            "{name}"
        );
        let path = dir.path().join(name);
        let (store, _, _) = install(&path, name, false);
        let before = tree(&path);
        assert!(
            matches!(store.load(), Err(StoreError::UpgradeRequired)),
            "{name}"
        );
        assert!(store
            .apply(
                0,
                "no-lazy-upgrade",
                &invoke("export", &[]),
                RuntimeLimits::default()
            )
            .is_err());
        assert_eq!(tree(&path), before);
    }
}
#[test]
fn every_frozen_history_upgrades_with_exact_inverse_projection_and_retained_bytes() {
    let dir = tempdir();
    for name in CASES {
        let path = dir.path().join(name);
        let (store, old, raw) = install(&path, name, true);
        let before = tree(&path);
        let summary = upgrade(&store).unwrap();
        assert_eq!(
            summary.from_version,
            old["version"].as_u64().unwrap() as u32
        );
        assert_eq!(summary.to_version, 3);
        assert!(matches!(store.load(), Err(StoreError::RestartRequired)));
        let reopened = open_verified(&path, None).unwrap();
        let target = serde_json::to_value(&reopened.snapshot).unwrap();
        let mut projected = target.clone();
        projected["version"] = old["version"].clone();
        if old["version"] == 1 {
            projected.as_object_mut().unwrap().remove("scope");
        } else {
            projected["scope"] = target["scope"]["state"].clone();
        }
        assert_eq!(
            canonical_bytes(&projected).unwrap(),
            canonical_bytes(&old).unwrap(),
            "{name}"
        );
        for (file, bytes) in &before {
            if file != "CURRENT" {
                assert_eq!(fs::read(path.join(file)).unwrap(), *bytes, "{name}/{file}");
            }
        }
        let digest = canonical_digest(IdentityDomain::Data, &old).unwrap();
        assert_eq!(
            fs::read(path.join(format!("object-{}.json", digest.as_str()))).unwrap(),
            raw
        );
        let journal: Value = serde_json::from_slice(
            &fs::read(path.join(format!(
                "upgrade-{}-3-{}.json",
                summary.from_version,
                digest.as_str()
            )))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(journal["version"], 2);
        assert_eq!(journal["migration"], "generated-project-container/3");
        let expected = canonical_digest(
            IdentityDomain::Evidence,
            &("container-preservation/1", canonical_bytes(&old).unwrap()),
        )
        .unwrap();
        assert_eq!(
            journal["preserved_payload"],
            serde_json::to_value(expected).unwrap()
        );
        let pointer = fs::read(path.join("CURRENT")).unwrap();
        upgrade(&reopened.store).unwrap();
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
        let backup = VerifiedBackup::capture(&reopened.store).unwrap();
        let recovered = backup
            .recover_new(&dir.path().join(format!("{name}-copy")))
            .unwrap();
        assert_eq!(recovered.load().unwrap(), reopened.snapshot);
        apply(&reopened.store, "continued-export", invoke("export", &[]));
    }
}
#[test]
fn old_backup_intake_rebinds_only_outer_link_and_resumes_exact_target() {
    let dir = tempdir();
    for name in CASES {
        let bytes = golden(name);
        let BackupIntake::UpgradeRequired(backup) = inspect_backup(&bytes).unwrap() else {
            panic!("{name}")
        };
        assert_eq!(backup.original_bytes(), bytes);
        let path = dir.path().join(name);
        backup.upgrade_recover_new(&path, |_| {}).unwrap();
        let current = VerifiedBackup::capture(&ProductStore::open(&path).unwrap()).unwrap();
        let old: Value = serde_json::from_slice(&bytes).unwrap();
        let new: Value = serde_json::from_slice(&current.to_bytes().unwrap()).unwrap();
        assert_eq!(
            old["payload"]["intentions"]["objects"],
            new["payload"]["intentions"]["objects"]
        );
        assert_eq!(
            old["payload"]["snapshot"]["programs"],
            new["payload"]["snapshot"]["programs"]
        );
        assert_eq!(new["payload"]["snapshot"]["version"], 3);
        let marker: Value =
            serde_json::from_slice(&fs::read(path.join("UPGRADE-INBOX")).unwrap()).unwrap();
        assert_eq!(marker["version"], 2);
        assert_eq!(marker["backup"], old["digest"]);
        let before = tree(&path);
        backup.upgrade_recover_new(&path, |_| {}).unwrap();
        assert_eq!(tree(&path), before);
        apply(
            &ProductStore::open(&path).unwrap(),
            "later-export",
            invoke("export", &[]),
        );
        let later = tree(&path);
        assert!(backup.upgrade_recover_new(&path, |_| {}).is_err());
        assert_eq!(tree(&path), later);
    }
}
#[test]
fn unknown_family_nested_version_and_current_untagged_scope_fail_closed() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("new"), &program(false), 20000).unwrap();
    let base = serde_json::to_value(store.load().unwrap()).unwrap();
    for family in ["owned_v2", "scalar_v2", "unknown"] {
        let mut v = base.clone();
        v["scope"] = json!({"family":family,"state":{}});
        assert!(serde_json::from_value::<ProjectSnapshot>(v).is_err());
    }
    let mut v = base.clone();
    v["scope"]["state"]["version"] = json!(2);
    assert!(serde_json::from_value::<ProjectSnapshot>(v)
        .and_then(|s| s.validate().map_err(serde::de::Error::custom))
        .is_err());
    let mut v = base;
    v["scope"] = serde_json::to_value(product_store::scope::ScopeState::default()).unwrap();
    assert!(serde_json::from_value::<ProjectSnapshot>(v).is_err());
}
#[test]
fn already_current_verified_upgrade_checks_packages_and_runtime() {
    let dir = tempdir();
    let path = dir.path().join("active");
    let (store, _, _) = install(&path, "active", false);
    upgrade(&store).unwrap();
    let current = ProductStore::open(&path).unwrap();
    let mut called = false;
    assert!(current
        .upgrade_generated_project_verified(
            |_| {},
            |_| {
                called = true;
                Err(StoreError::Corrupt("test verifier refused".into()))
            }
        )
        .is_err());
    assert!(called);
    let extension = fs::read_dir(&path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("extension-")
        })
        .unwrap();
    fs::write(extension, b"{}").unwrap();
    assert!(upgrade(&ProductStore::open(&path).unwrap()).is_err());
}
#[test]
fn old_inbox_is_preserved_and_activated_old_destination_uses_in_place_gate() {
    let dir = tempdir();
    let bytes = golden("legacy-v1");
    let BackupIntake::UpgradeRequired(backup) = inspect_backup(&bytes).unwrap() else {
        panic!()
    };
    let path = dir.path().join("old-inbox");
    fs::create_dir(&path).unwrap();
    fs::write(
        path.join("UPGRADE-INBOX"),
        b"[\"old-source\",\"old-target\"]",
    )
    .unwrap();
    fs::write(path.join("retained"), b"old staged bytes").unwrap();
    let before = tree(&path);
    assert!(backup.upgrade_recover_new(&path, |_| {}).is_err());
    assert_eq!(tree(&path), before);
    let path = dir.path().join("old-activated");
    let (store, _, _) = install(&path, "active", false);
    fs::write(path.join("UPGRADE-INBOX"), b"old marker").unwrap();
    upgrade(&store).unwrap();
    assert_eq!(fs::read(path.join("UPGRADE-INBOX")).unwrap(), b"old marker");
}
#[test]
fn activated_recovery_retry_requires_original_and_journal() {
    for remove in ["original", "journal"] {
        let dir = tempdir();
        let bytes = golden("legacy-v1");
        let envelope: Value = serde_json::from_slice(&bytes).unwrap();
        let old = canonical_digest(IdentityDomain::Data, &envelope["payload"]["snapshot"]).unwrap();
        let BackupIntake::UpgradeRequired(backup) = inspect_backup(&bytes).unwrap() else {
            panic!()
        };
        let path = dir.path().join("copy");
        backup.upgrade_recover_new(&path, |_| {}).unwrap();
        let name = if remove == "original" {
            format!("object-{}.json", old.as_str())
        } else {
            format!("upgrade-1-3-{}.json", old.as_str())
        };
        fs::write(path.join(name), b"{}").unwrap();
        let before = tree(&path);
        assert!(backup.upgrade_recover_new(&path, |_| {}).is_err());
        assert_eq!(tree(&path), before);
    }
}
#[test]
fn activated_retry_rechecks_retained_inputs_after_verified_progress() {
    for kind in ["original", "journal", "marker", "extension"] {
        for remove in [false, true] {
            let dir = tempdir();
            let path = dir.path().join("copy");
            let BackupIntake::UpgradeRequired(backup) =
                inspect_backup(&golden("legacy-v1")).unwrap()
            else {
                panic!()
            };
            backup.upgrade_recover_new(&path, |_| {}).unwrap();
            let marker: Value =
                serde_json::from_slice(&fs::read(path.join("UPGRADE-INBOX")).unwrap()).unwrap();
            let name = match kind {
                "original" => format!("object-{}.json", marker["original"].as_str().unwrap()),
                "journal" => format!("upgrade-1-3-{}.json", marker["original"].as_str().unwrap()),
                "marker" => "UPGRADE-INBOX".into(),
                "extension" => tree(&path)
                    .keys()
                    .find(|name| name.starts_with("extension-"))
                    .unwrap()
                    .clone(),
                _ => unreachable!(),
            };
            let before = tree(&path);
            let mut expected = before.clone();
            if remove {
                expected.remove(&name);
            } else {
                expected.insert(name.clone(), b"{}".to_vec());
            }
            let result = backup.upgrade_recover_new(&path, |stage| {
                if stage == UpgradeProgress::Verified {
                    if remove {
                        fs::remove_file(path.join(&name)).unwrap();
                    } else {
                        fs::write(path.join(&name), b"{}").unwrap();
                    }
                }
            });
            assert!(
                result.is_err(),
                "{kind} removal={remove}: changed verified input accepted"
            );
            assert!(
                tree(&path) == expected,
                "retry altered other retained evidence"
            );
            fs::write(path.join(&name), &before[&name]).unwrap();
            backup.upgrade_recover_new(&path, |_| {}).unwrap();
            assert!(tree(&path) == before);
            open_verified(&path, None).unwrap();
        }
    }
}
#[test]
fn newest_valid_old_checkpoint_remains_discoverable_without_rewriting_shelf() {
    let dir = tempdir();
    let bytes = golden("withdrawn");
    let envelope: Value = serde_json::from_slice(&bytes).unwrap();
    let old = &envelope["payload"]["snapshot"];
    let identity = product_locations::ToolIdentity {
        project_id: serde_json::from_value(old["data"]["project_id"].clone()).unwrap(),
        first_program: canonical_digest(IdentityDomain::Source, &old["programs"][0]).unwrap(),
    };
    fs::create_dir(dir.path().join("locations")).unwrap();
    let locations = product_locations::ToolLocations::chosen(dir.path().join("locations")).unwrap();
    let shelf = CheckpointShelf::for_tool(
        &locations,
        &identity,
        &canonical_digest(IdentityDomain::Evidence, &"instance").unwrap(),
    )
    .unwrap();
    let name = format!(
        "checkpoint-{:020}-{}.gmbak",
        old["revision"].as_u64().unwrap(),
        envelope["digest"].as_str().unwrap()
    );
    // A newer valid old checkpoint must outrank an older current checkpoint,
    // while an even newer corrupt candidate remains a reported issue.
    let older_path = dir.path().join("older-current");
    let (older, _, _) = install(&older_path, "active", false);
    upgrade(&older).unwrap();
    let receipt = shelf
        .capture(&ProductStore::open(&older_path).unwrap())
        .unwrap();
    assert!(receipt.summary.revision < old["revision"].as_u64().unwrap());
    fs::write(shelf.path().join(&name), &bytes).unwrap();
    let corrupt_name = format!(
        "checkpoint-{:020}-{}.gmbak",
        old["revision"].as_u64().unwrap() + 1,
        envelope["digest"].as_str().unwrap()
    );
    fs::write(shelf.path().join(corrupt_name), b"corrupt newer checkpoint").unwrap();
    let before = tree(shelf.path());
    let search = shelf.newest_verified().unwrap();
    assert_eq!(search.issues.len(), 1);
    let recovery = search.recovery.unwrap();
    assert!(recovery.requires_upgrade());
    assert_eq!(
        recovery.digest().as_str(),
        envelope["digest"].as_str().unwrap()
    );
    assert_eq!(recovery.to_bytes().unwrap(), bytes);
    assert!(tree(shelf.path()) == before);
    let report = doctor(&dir.path().join("missing"), &identity, &shelf);
    assert_eq!(report.checkpoint_issues.len(), 1);
    let recovery = report.recovery.unwrap();
    assert!(recovery.requires_upgrade());
    assert_eq!(recovery.to_bytes().unwrap(), bytes);
    assert!(tree(shelf.path()) == before);
}
#[test]
fn direct_upgrades_and_backup_retries_preserve_each_fault_boundary() {
    for name in ["legacy-v1", "prospective"] {
        for fault in [
            FaultPoint::AfterObject,
            FaultPoint::AfterUpgradeObject,
            FaultPoint::AfterUpgradeJournal,
            FaultPoint::BeforePointer,
            FaultPoint::AfterPointer,
        ] {
            let dir = tempdir();
            let path = dir.path().join("store");
            let (store, old, _) = install(&path, name, false);
            let before = tree(&path);
            let archive = IntentArchive::new(store.clone());
            assert!(store
                .upgrade_verified_with_fault(
                    |_| {},
                    |s| archive
                        .export_for(s)
                        .map(|_| ())
                        .map_err(|e| StoreError::Corrupt(e.to_string())),
                    fault
                )
                .is_err());
            if fault != FaultPoint::AfterPointer {
                assert_eq!(fs::read(path.join("CURRENT")).unwrap(), before["CURRENT"]);
            }
            upgrade(&ProductStore::open(&path).unwrap()).unwrap();
            let current = ProductStore::open(&path).unwrap().load().unwrap();
            assert_eq!(serde_json::to_value(&current.data).unwrap(), old["data"]);
            assert_eq!(current.revision, old["revision"].as_u64().unwrap());
            let BackupIntake::UpgradeRequired(backup) = inspect_backup(&golden(name)).unwrap()
            else {
                panic!()
            };
            let copy = dir.path().join("copy");
            assert!(backup
                .upgrade_recover_with_fault(&copy, |_| {}, fault)
                .is_err());
            backup.upgrade_recover_new(&copy, |_| {}).unwrap();
            assert_eq!(ProductStore::open(&copy).unwrap().load().unwrap(), current);
        }
    }
}
#[test]
fn extension_staging_rejects_old_and_unsupported_current_pointers() {
    let dir = tempdir();
    let bytes = canonical_bytes(&json!({"staged": "new"})).unwrap();
    for name in ["legacy-v1", "prospective", "future", "malformed"] {
        let path = dir.path().join(name);
        let (store, _, _) = install(
            &path,
            if name == "prospective" {
                name
            } else {
                "legacy-v1"
            },
            false,
        );
        if name == "future" {
            let mut pointer: Value =
                serde_json::from_slice(&fs::read(path.join("CURRENT")).unwrap()).unwrap();
            pointer["version"] = json!(99);
            fs::write(path.join("CURRENT"), canonical_bytes(&pointer).unwrap()).unwrap();
        } else if name == "malformed" {
            fs::write(path.join("CURRENT"), b"not json").unwrap();
        }
        let before = tree(&path);
        let result = store.stage_extension(&bytes);
        match name {
            "legacy-v1" | "prospective" => assert!(
                matches!(result, Err(StoreError::UpgradeRequired)),
                "{name}: {result:?}"
            ),
            "future" => assert!(
                matches!(result, Err(StoreError::UnsupportedFormat(99))),
                "{result:?}"
            ),
            _ => assert!(result.is_err()),
        }
        assert_eq!(tree(&path), before, "{name}");
    }
    let fresh = dir.path().join("fresh");
    fs::create_dir(&fresh).unwrap();
    let store = ProductStore::open(&fresh).unwrap();
    let digest = store.stage_extension(&bytes).unwrap();
    assert_eq!(store.read_extension(&digest).unwrap(), bytes);
    assert!(!fresh.join("CURRENT").exists());
    let store = ProductStore::create(dir.path().join("current"), &program(false), 20000).unwrap();
    let before = store.load().unwrap();
    let digest = store.stage_extension(&bytes).unwrap();
    assert_eq!(store.read_extension(&digest).unwrap(), bytes);
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn invalidated_upgrade_handles_cannot_stage_or_sync_extensions() {
    for fault in [
        None,
        Some(FaultPoint::AfterObject),
        Some(FaultPoint::BeforePointer),
        Some(FaultPoint::AfterPointer),
    ] {
        let dir = tempdir();
        let path = dir.path().join("old");
        let (store, _, _) = install(&path, "legacy-v1", false);
        let reused = canonical_bytes(&json!({"staged": "retained"})).unwrap();
        let digest =
            canonical_digest(IdentityDomain::Evidence, &json!({"staged": "retained"})).unwrap();
        let reused_path = path.join(format!("extension-{}.json", digest.as_str()));
        fs::write(&reused_path, &reused).unwrap();
        if let Some(fault) = fault {
            let archive = IntentArchive::new(store.clone());
            assert!(store
                .upgrade_verified_with_fault(
                    |_| {},
                    |snapshot| archive
                        .export_for(snapshot)
                        .map(|_| ())
                        .map_err(|e| StoreError::Corrupt(e.to_string())),
                    fault
                )
                .is_err());
        } else {
            upgrade(&store).unwrap();
        }
        assert!(matches!(store.load(), Err(StoreError::RestartRequired)));
        let new = canonical_bytes(&json!({"staged": "new"})).unwrap();
        let before = tree(&path);
        let sync_fault = product_store::file_sync_testing::fail(reused_path);
        for handle in [&store, &store.clone()] {
            for bytes in [&new, &reused] {
                assert!(
                    matches!(
                        handle.stage_extension(bytes),
                        Err(StoreError::RestartRequired)
                    ),
                    "{fault:?}"
                );
                assert_eq!(
                    sync_fault.attempts(),
                    0,
                    "invalidated handle attempted content sync"
                );
                assert_eq!(tree(&path), before, "invalidated handle changed files");
            }
        }
        drop(sync_fault);
        upgrade(&ProductStore::open(&path).unwrap()).unwrap();
        let reopened = ProductStore::open(&path).unwrap();
        let snapshot = reopened.load().unwrap();
        reopened.stage_extension(&new).unwrap();
        reopened.stage_extension(&reused).unwrap();
        assert_eq!(reopened.load().unwrap(), snapshot);
    }
}

#[test]
fn attempted_activation_requires_reopen_before_any_same_handle_retry() {
    for fault in [
        FaultPoint::AfterObject,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let dir = tempdir();
        let path = dir.path().join("old");
        let (store, _, _) = install(&path, "legacy-v1", false);
        let archive = IntentArchive::new(store.clone());
        assert!(store
            .upgrade_verified_with_fault(
                |_| {},
                |snapshot| archive
                    .export_for(snapshot)
                    .map(|_| ())
                    .map_err(|e| StoreError::Corrupt(e.to_string())),
                fault,
            )
            .is_err());
        assert!(matches!(store.load(), Err(StoreError::RestartRequired)));
        let before = tree(&path);
        for handle in [&store, &store.clone()] {
            let progressed = std::cell::Cell::new(false);
            let verified = std::cell::Cell::new(false);
            let result = handle.upgrade_generated_project_verified(
                |_| progressed.set(true),
                |_| {
                    verified.set(true);
                    Ok(())
                },
            );
            assert!(
                matches!(result, Err(StoreError::RestartRequired)),
                "{fault:?}"
            );
            assert!(
                !progressed.get() && !verified.get(),
                "poisoned handle invoked callbacks"
            );
            assert!(
                tree(&path) == before,
                "poisoned handle changed retained bytes"
            );
        }
        upgrade(&ProductStore::open(&path).unwrap()).unwrap();
        open_verified(&path, None).unwrap();
    }
}
#[test]
fn stale_packages_and_replaced_project_path_block_activation() {
    let dir = tempdir();
    let path = dir.path().join("store");
    let (store, _, _) = install(&path, "prospective", false);
    let pointer = fs::read(path.join("CURRENT")).unwrap();
    let archive = IntentArchive::new(store.clone());
    let result = store.upgrade_generated_project_verified(
        |stage| {
            if stage == UpgradeProgress::Staged {
                let ext = fs::read_dir(&path)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("extension-")
                    })
                    .unwrap();
                fs::write(ext, b"{}").unwrap();
            }
        },
        |s| {
            archive
                .export_for(s)
                .map(|_| ())
                .map_err(|e| StoreError::Corrupt(e.to_string()))
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    #[cfg(unix)]
    {
        let path = dir.path().join("replaced");
        let (store, _, _) = install(&path, "legacy-v1", false);
        let original = dir.path().join("original");
        fs::rename(&path, &original).unwrap();
        fs::create_dir(&path).unwrap();
        let original_before = tree(&original);
        assert!(upgrade(&store).is_err());
        assert_eq!(tree(&original), original_before);
        assert!(tree(&path).is_empty());
    }
}
#[test]
fn malformed_old_inputs_and_pointer_mismatches_never_stage() {
    for name in ["legacy-v1", "prospective"] {
        for kind in [
            "duplicate",
            "trailing",
            "deep",
            "revision",
            "project",
            "digest",
            "version",
            "proof",
        ] {
            let dir = tempdir();
            let path = dir.path().join("store");
            let (store, mut old, _) = install(&path, name, false);
            let mut pointer: Value =
                serde_json::from_slice(&fs::read(path.join("CURRENT")).unwrap()).unwrap();
            let file = path.join(format!(
                "object-{}.json",
                pointer["object"].as_str().unwrap()
            ));
            match kind {
                "duplicate" => {
                    let bytes = canonical_bytes(&old).unwrap();
                    let mut changed = b"{\"version\":1,".to_vec();
                    changed.extend_from_slice(&bytes[1..]);
                    fs::write(file, changed).unwrap();
                }
                "trailing" => {
                    let mut bytes = canonical_bytes(&old).unwrap();
                    bytes.extend_from_slice(b" {}");
                    fs::write(file, bytes).unwrap();
                }
                "deep" => {
                    fs::write(file, format!("{}0{}", "[".repeat(140), "]".repeat(140))).unwrap();
                }
                "revision" => {
                    pointer["revision"] = json!(999);
                }
                "project" => {
                    pointer["project_id"] = json!("foreign-project");
                }
                "digest" => {
                    pointer["object"] = json!("0".repeat(64));
                }
                "version" => {
                    pointer["version"] = json!(99);
                }
                "proof" => {
                    if name == "legacy-v1" {
                        old["unknown"] = json!(true);
                    } else {
                        old["scope"]["version"] = json!(99);
                    }
                    let digest = canonical_digest(IdentityDomain::Data, &old).unwrap();
                    pointer["object"] = serde_json::to_value(&digest).unwrap();
                    fs::write(
                        path.join(format!("object-{}.json", digest.as_str())),
                        canonical_bytes(&old).unwrap(),
                    )
                    .unwrap();
                }
                _ => unreachable!(),
            }
            fs::write(path.join("CURRENT"), canonical_bytes(&pointer).unwrap()).unwrap();
            let before = tree(&path);
            assert!(upgrade(&store).is_err(), "{name}/{kind}");
            assert_eq!(tree(&path), before, "{name}/{kind}");
        }
    }
}
#[test]
fn old_backups_reject_mismatched_missing_duplicate_and_wrong_kind_packages() {
    for kind in [
        "link",
        "missing",
        "duplicate",
        "wrong-kind",
        "version",
        "duplicate-json",
        "trailing",
    ] {
        let bytes = golden("active");
        let mut envelope: Value = serde_json::from_slice(&bytes).unwrap();
        let bundle = &mut envelope["payload"]["intentions"];
        match kind {
            "link" => bundle["snapshot"] = json!("0".repeat(64)),
            "missing" => {
                bundle["objects"].as_array_mut().unwrap().pop();
            }
            "duplicate" => {
                let first = bundle["objects"][0].clone();
                bundle["objects"].as_array_mut().unwrap().push(first);
            }
            "wrong-kind" => {
                bundle["objects"][0]["bytes"] = json!([123, 125]);
            }
            "version" => bundle["version"] = json!(99),
            _ => {}
        }
        envelope["digest"] = serde_json::to_value(
            canonical_digest(IdentityDomain::Evidence, &envelope["payload"]).unwrap(),
        )
        .unwrap();
        let mut bytes = canonical_bytes(&envelope).unwrap();
        if kind == "duplicate-json" {
            let mut dup = b"{\"version\":1,".to_vec();
            dup.extend_from_slice(&bytes[1..]);
            bytes = dup;
        }
        if kind == "trailing" {
            bytes.extend_from_slice(b" {}");
        }
        assert!(inspect_backup(&bytes).is_err(), "{kind}");
    }
}
#[test]
fn reused_upgrade_files_require_content_sync_before_activation_or_success() {
    for kind in [
        "target",
        "journal",
        "marker",
        "extension",
        "original",
        "current",
    ] {
        let dir = tempdir();
        let path = dir.path().join("copy");
        let bytes = golden("prospective");
        let BackupIntake::UpgradeRequired(backup) = inspect_backup(&bytes).unwrap() else {
            panic!()
        };
        assert!(backup
            .upgrade_recover_with_fault(&path, |_| {}, FaultPoint::BeforePointer)
            .is_err());
        let marker: Value =
            serde_json::from_slice(&fs::read(path.join("UPGRADE-INBOX")).unwrap()).unwrap();
        let name = match kind {
            "target" => format!("object-{}.json", marker["target"].as_str().unwrap()),
            "journal" => format!("upgrade-2-3-{}.json", marker["original"].as_str().unwrap()),
            "original" => format!("object-{}.json", marker["original"].as_str().unwrap()),
            "marker" => "UPGRADE-INBOX".into(),
            "extension" => fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .find(|n| n.starts_with("extension-"))
                .unwrap(),
            "current" => {
                backup.upgrade_recover_new(&path, |_| {}).unwrap();
                "CURRENT".into()
            }
            _ => unreachable!(),
        };
        let before = tree(&path);
        let fault = product_store::file_sync_testing::fail(path.join(name));
        assert!(backup.upgrade_recover_new(&path, |_| {}).is_err(), "{kind}");
        assert!(fault.attempts() > 0, "{kind}");
        assert_eq!(tree(&path), before);
        drop(fault);
        backup.upgrade_recover_new(&path, |_| {}).unwrap();
        open_verified(&path, None).unwrap();
    }
}
#[test]
#[cfg(unix)]
fn post_pointer_directory_barrier_failure_requires_reopen_and_explicit_completion() {
    let dir = tempdir();
    let path = dir.path().join("old");
    let (store, _, _) = install(&path, "legacy-v1", false);
    let mut fault = None;
    let archive = IntentArchive::new(store.clone());
    let result = store.upgrade_generated_project_verified(
        |stage| {
            if stage == UpgradeProgress::Activating {
                fault = Some(product_store::file_sync_testing::fail_after(
                    path.clone(),
                    1,
                ));
            }
        },
        |snapshot| {
            archive
                .export_for(snapshot)
                .map(|_| ())
                .map_err(|e| StoreError::Corrupt(e.to_string()))
        },
    );
    assert!(result.is_err());
    assert!(matches!(store.load(), Err(StoreError::RestartRequired)));
    let pointer: Value = serde_json::from_slice(&fs::read(path.join("CURRENT")).unwrap()).unwrap();
    assert_eq!(pointer["version"], 3);
    let reopened = ProductStore::open(&path).unwrap();
    assert!(reopened.load().is_ok());
    assert!(upgrade(&reopened).is_err());
    drop(fault);
    let before = tree(&path);
    upgrade(&reopened).unwrap();
    assert_eq!(tree(&path), before);
}
#[test]
fn conflicting_upgrade_target_journal_or_marker_never_overwrites_evidence() {
    for kind in ["target", "journal", "marker"] {
        let dir = tempdir();
        let path = dir.path().join("copy");
        let BackupIntake::UpgradeRequired(backup) = inspect_backup(&golden("legacy-v1")).unwrap()
        else {
            panic!()
        };
        assert!(backup
            .upgrade_recover_with_fault(&path, |_| {}, FaultPoint::AfterUpgradeJournal)
            .is_err());
        let marker: Value =
            serde_json::from_slice(&fs::read(path.join("UPGRADE-INBOX")).unwrap()).unwrap();
        let name = match kind {
            "target" => format!("object-{}.json", marker["target"].as_str().unwrap()),
            "journal" => format!("upgrade-1-3-{}.json", marker["original"].as_str().unwrap()),
            _ => "UPGRADE-INBOX".into(),
        };
        fs::write(path.join(name), b"{\"conflict\":true}").unwrap();
        let before = tree(&path);
        assert!(backup.upgrade_recover_new(&path, |_| {}).is_err());
        assert_eq!(tree(&path), before);
        assert!(!path.join("CURRENT").exists());
    }
}
#[test]
fn wrapper_budget_crossing_refuses_without_dropping_history() {
    let dir = tempdir();
    let path = dir.path().join("budget");
    let store = ProductStore::create(&path, &program(false), 20000).unwrap();
    let mut snapshot = store.load().unwrap();
    let source = snapshot.programs[0].source_bytes.clone();
    let old_value = |snapshot: &ProjectSnapshot| {
        let mut value = serde_json::to_value(snapshot).unwrap();
        value["version"] = json!(2);
        value["scope"] = serde_json::to_value(snapshot.scalar_scope().unwrap()).unwrap();
        value
    };
    const LIMIT: usize = 16 * 1024 * 1024;
    // Bounded, ordinary captures may differ only in original JSON whitespace.
    // No compiler/proof identities are changed to manufacture a valid history.
    let mut ordinal = 0;
    while canonical_bytes(&old_value(&snapshot)).unwrap().len() + 3 * MAX_WIRE_BYTES + 32768 < LIMIT
    {
        let mut bytes = source.clone();
        bytes.resize(MAX_WIRE_BYTES - ordinal, b' ');
        snapshot.programs.push(
            CapturedProgram::capture(
                &bytes,
                "scope-project",
                Producer::Fixture {
                    name: format!("budget-{ordinal}"),
                },
                None,
            )
            .unwrap(),
        );
        ordinal += 1;
    }
    snapshot.programs.push(
        CapturedProgram::capture(
            &source,
            "scope-project",
            Producer::Fixture {
                name: "budget-tail".into(),
            },
            None,
        )
        .unwrap(),
    );
    let base = canonical_bytes(&old_value(&snapshot)).unwrap().len();
    let mut bytes = source;
    bytes.resize(bytes.len() + (LIMIT - base - 1) / 3, b' ');
    assert!(bytes.len() <= MAX_WIRE_BYTES);
    *snapshot.programs.last_mut().unwrap() = CapturedProgram::capture(
        &bytes,
        "scope-project",
        Producer::Fixture {
            name: "budget-tail".into(),
        },
        None,
    )
    .unwrap();
    snapshot.validate().unwrap();
    let old = old_value(&snapshot);
    let raw = canonical_bytes(&old).unwrap();
    assert!(raw.len() <= LIMIT);
    assert!(canonical_bytes(&snapshot).unwrap().len() > LIMIT);
    let digest = canonical_digest(IdentityDomain::Data, &old).unwrap();
    fs::write(path.join(format!("object-{}.json", digest.as_str())), raw).unwrap();
    fs::write(path.join("CURRENT"),canonical_bytes(&json!({"magic":"gitmanager.generated-project","version":2,"revision":snapshot.revision,"project_id":snapshot.data.project_id,"object":digest})).unwrap()).unwrap();
    let before = tree(&path);
    assert!(
        matches!(upgrade(&store),Err(StoreError::Invalid(message)) if message.contains("budget"))
    );
    assert_eq!(tree(&path), before);
}
