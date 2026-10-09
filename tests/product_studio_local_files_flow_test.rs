//! Synthetic local-file workflow tests. Native dialogs and shared control routing
//! require the later serial host integration; these fixtures are not user trials.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_studio/local_files_flow.rs"]
mod local_files_flow;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_export.rs"]
mod product_export;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_store/mod.rs"]
mod product_store;

use fixture::*;
use local_files_flow::*;
use product_backup::*;
use product_contract::*;
use product_export::*;
use product_locations::*;
use product_store::*;
use serde_json::json;
use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    store: ProductStore,
    locations: ToolLocations,
    recent: RecentTools,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let mut source = organizer();
        let step = source["actions"][2]["steps"][0].clone();
        source["actions"][2]["steps"]
            .as_array_mut()
            .unwrap()
            .push(step);
        let store = ProductStore::create(root.join("original"), &capture(source), 20000).unwrap();
        fs::create_dir(root.join("settings")).unwrap();
        let locations = ToolLocations::chosen(root.join("settings")).unwrap();
        let recent = RecentTools::open(locations.path()).unwrap();
        recent.remember(&root.join("original"), 1).unwrap();
        let fixture = Self {
            _temp: temp,
            root,
            store,
            locations,
            recent,
        };
        fixture.apply(
            "add",
            invoke(
                "add_person",
                args(&[("name", string("Ada")), ("area", string("West"))]),
            ),
        );
        fixture.apply("select", invoke("collect", Values::new()));
        fixture.apply("first-output", invoke("export_people", Values::new()));
        fixture
    }
    fn snapshot(&self) -> ProjectSnapshot {
        self.store.load().unwrap()
    }
    fn apply(&self, id: &str, input: SemanticInput) -> ProjectSnapshot {
        self.store
            .apply(
                self.snapshot().revision,
                id,
                &input,
                RuntimeLimits::default(),
            )
            .unwrap()
    }
    fn reply(&self, operation: &str, name: &str) -> DestinationChoice {
        DestinationChoice {
            operation: operation.into(),
            destination: Some(self.root.join(name)),
        }
    }
    fn shelf(&self) -> CheckpointShelf {
        let instance = self
            .recent
            .remember(&self.root.join("original"), 1)
            .unwrap();
        CheckpointShelf::for_tool(
            &self.locations,
            &ToolIdentity::from_snapshot(&self.snapshot()).unwrap(),
            &instance,
        )
        .unwrap()
    }
}
fn basis(snapshot: &ProjectSnapshot) -> Digest {
    canonical_digest(IdentityDomain::Evidence, snapshot).unwrap()
}
fn selection(snapshot: &ProjectSnapshot, index: usize) -> ArtifactSelection {
    ArtifactSelection {
        inventory_index: index,
        expected_digest: snapshot.artifacts[index].digest.clone(),
    }
}
fn legacy_bytes(store: &ProductStore) -> Vec<u8> {
    let mut value: serde_json::Value =
        serde_json::from_slice(&VerifiedBackup::capture(store).unwrap().to_bytes().unwrap())
            .unwrap();
    let snapshot = &mut value["payload"]["snapshot"];
    snapshot.as_object_mut().unwrap().remove("scope");
    snapshot["version"] = json!(1);
    let old = canonical_digest(IdentityDomain::Data, snapshot).unwrap();
    value["payload"]["intentions"]["snapshot"] = json!(old);
    value["digest"] = json!(canonical_digest(IdentityDomain::Evidence, &value["payload"]).unwrap());
    canonical_bytes(&value).unwrap()
}

#[test]
fn inventory_and_export_keep_identical_occurrences_and_historical_labels() {
    let f = Fixture::new();
    f.apply("tomorrow", SemanticInput::AdvanceClock { days: 1 });
    let old = f.apply("second-output", invoke("export_people", Values::new()));
    let mut changed = organizer();
    changed["label"] = json!("New tool name");
    changed["outputs"][0]["label"] = json!("New output name");
    changed["actions"][2]["label"] = json!("New action name");
    let program = capture(changed);
    let plan = f.store.prepare_switch(&program, "rename").unwrap();
    f.store
        .adopt(old.revision, &plan, &program, &old.decisions)
        .unwrap();
    let snapshot = f.snapshot();
    let inventory = inventory(&f.store, &snapshot).unwrap();
    assert_eq!(inventory.rows.len(), 4);
    assert_eq!(inventory.basis, basis(&snapshot));
    assert_eq!(inventory.rows[3].selection, selection(&snapshot, 3));
    assert_eq!(inventory.rows[3].produced_day, 20001);
    assert_eq!(inventory.rows[3].output_ordinal, 1);
    assert_eq!(inventory.rows[3].operation_id, "second-output");
    assert_eq!(
        inventory.rows[3].program_label,
        snapshot.programs[0].program.label
    );
    assert!(snapshot
        .artifacts
        .windows(2)
        .all(|rows| rows[0].digest == rows[1].digest));
    assert_eq!(
        inventory.rows[3].output_label,
        snapshot.programs[0].program.outputs[0].label
    );
    assert_eq!(
        inventory.rows[3].action_label,
        snapshot.programs[0].program.actions[2].label
    );
    let draft = ExportDraft::prepare(
        &f.store,
        &snapshot,
        inventory.rows[3].selection.clone(),
        "export-3",
    )
    .unwrap();
    let result = draft.publish(
        f.reply("export-3", "chosen.csv"),
        "export-3",
        &AtomicBool::new(false),
    );
    let ExportOutcome::Verified(receipt) = result else {
        panic!("export not verified")
    };
    assert_eq!(receipt.output.inventory_index, 3);
    assert_eq!(receipt.output.event_output_ordinal, 1);
    assert_eq!(
        fs::read(receipt.destination).unwrap(),
        snapshot.artifacts[3].bytes
    );
    assert_eq!(f.snapshot(), snapshot);
}

#[test]
fn dialog_cancel_navigation_staleness_and_collision_never_replace_a_file() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    for (choice, active, cancelled) in [
        (
            DestinationChoice {
                operation: "export".into(),
                destination: None,
            },
            "export",
            false,
        ),
        (f.reply("export", "navigation.csv"), "new-operation", false),
        (f.reply("wrong-dialog", "wrong.csv"), "export", false),
        (f.reply("export", "cancelled.csv"), "export", true),
    ] {
        let draft =
            ExportDraft::prepare(&f.store, &snapshot, selection(&snapshot, 0), "export").unwrap();
        assert!(matches!(
            draft.publish(choice.clone(), active, &AtomicBool::new(cancelled)),
            ExportOutcome::NotAttempted(_)
        ));
        if let Some(path) = choice.destination {
            assert!(!path.exists());
        }
    }
    let stale =
        ExportDraft::prepare(&f.store, &snapshot, selection(&snapshot, 0), "stale").unwrap();
    f.apply("later", SemanticInput::AdvanceClock { days: 1 });
    assert!(matches!(
        stale.publish(
            f.reply("stale", "stale.csv"),
            "stale",
            &AtomicBool::new(false)
        ),
        ExportOutcome::NotAttempted(ExportError::StaleBasis)
    ));
    assert!(inventory(&f.store, &snapshot).is_err());
    assert!(!f.root.join("stale.csv").exists());
    let current = f.snapshot();
    let existing = f.root.join("existing.csv");
    fs::write(&existing, "keep me").unwrap();
    let result = ExportDraft::prepare(&f.store, &current, selection(&current, 0), "collision")
        .unwrap()
        .publish(
            f.reply("collision", "existing.csv"),
            "collision",
            &AtomicBool::new(false),
        );
    assert!(matches!(result, ExportOutcome::PublicationUncertain { .. }));
    assert_eq!(fs::read(existing).unwrap(), b"keep me");
}

#[test]
fn publication_views_preserve_all_four_outcomes_without_retry_advice() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    let summary = PreparedExport::capture(&f.store, &basis(&snapshot), selection(&snapshot, 0))
        .unwrap()
        .summary()
        .clone();
    let receipt = ExportReceipt {
        destination: f.root.join("saved.csv"),
        output: summary,
    };
    let issue = OperationIssue::new(IssueKind::Unavailable, "simulated readback/write failure");
    let outcomes = [
        ExportOutcome::NotAttempted(ExportError::Cancelled),
        ExportOutcome::PublicationUncertain {
            receipt: receipt.clone(),
            issue: issue.clone(),
        },
        ExportOutcome::PublishedUnverified {
            receipt: receipt.clone(),
            issue,
        },
        ExportOutcome::Verified(receipt),
    ];
    let views: Vec<_> = outcomes.iter().map(export_report).collect();
    assert_eq!(
        views.iter().map(|v| v.status).collect::<Vec<_>>(),
        vec![
            PublicationStatus::NotAttempted,
            PublicationStatus::Uncertain,
            PublicationStatus::PublishedUnverified,
            PublicationStatus::Verified
        ]
    );
    assert!(views[1].message.contains("may have been saved"));
    assert!(views[2].message.contains("saved"));
    for view in &views[1..3] {
        assert!(view.message.contains("Do not repeat"));
    }
}

#[test]
fn backup_export_rechecks_basis_and_reports_ambiguous_write_errors() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    let draft = BackupDraft::prepare(&f.store, &snapshot, "copy").unwrap();
    let result = draft.publish(
        f.reply("copy", "saved.gmbak"),
        "copy",
        &AtomicBool::new(false),
    );
    assert!(matches!(result, BackupOutcome::Verified { .. }));
    let bytes = fs::read(f.root.join("saved.gmbak")).unwrap();
    let duplicate = BackupDraft::prepare(&f.store, &snapshot, "copy-again")
        .unwrap()
        .publish(
            f.reply("copy-again", "saved.gmbak"),
            "copy-again",
            &AtomicBool::new(false),
        );
    assert!(matches!(duplicate, BackupOutcome::Uncertain { .. }));
    assert!(backup_report(&duplicate)
        .message
        .contains("may have been saved"));
    assert_eq!(fs::read(f.root.join("saved.gmbak")).unwrap(), bytes);
    let stale = BackupDraft::prepare(&f.store, &snapshot, "stale").unwrap();
    f.apply("later", SemanticInput::AdvanceClock { days: 1 });
    assert!(matches!(
        stale.publish(
            f.reply("stale", "stale.gmbak"),
            "stale",
            &AtomicBool::new(false)
        ),
        BackupOutcome::NotAttempted(_)
    ));
    assert!(!f.root.join("stale.gmbak").exists());
}

#[test]
fn current_and_legacy_import_reopen_fresh_instances_and_keep_originals() {
    for legacy in [false, true] {
        let f = Fixture::new();
        let original = f.snapshot();
        let bytes = if legacy {
            legacy_bytes(&f.store)
        } else {
            VerifiedBackup::capture(&f.store)
                .unwrap()
                .to_bytes()
                .unwrap()
        };
        let file = f.root.join("import.gmbak");
        fs::write(&file, &bytes).unwrap();
        let draft = RecoveryDraft::read(&file, "recover").unwrap();
        assert_eq!(draft.preview().requires_upgrade, legacy);
        let mut progress = vec![];
        let recovered = draft.recover(
            f.reply("recover", "recovered"),
            "recover",
            &AtomicBool::new(false),
            &f.recent,
            &f.locations,
            2,
            |p| progress.push(p),
        );
        let RecoveryOutcome::Recovered(mut result) = recovered else {
            panic!("recovery failed")
        };
        assert_eq!(result.opened.snapshot.data, original.data);
        assert_eq!(result.opened.snapshot.artifacts, original.artifacts);
        assert!(result.registration.is_ok());
        assert!(matches!(result.checkpoint, CheckpointOutcome::Verified(_)));
        if legacy {
            assert!(progress.contains(&UpgradeProgress::RestartRequired));
        }
        let original_id = f
            .recent
            .list()
            .unwrap()
            .into_iter()
            .find(|e| e.tool.path == f.root.join("original"))
            .unwrap()
            .tool
            .id;
        assert_ne!(result.registration.as_ref().unwrap(), &original_id);
        result
            .opened
            .store
            .apply(
                result.opened.snapshot.revision,
                "recovered-add",
                &invoke(
                    "add_person",
                    args(&[("name", string("Later")), ("area", string("East"))]),
                ),
                RuntimeLimits::default(),
            )
            .unwrap();
        let reopened = open_verified(
            &f.root.join("recovered"),
            Some(&result.opened.summary.identity),
        )
        .unwrap();
        assert_eq!(
            reopened.snapshot.data.records.len(),
            original.data.records.len() + 1
        );
        assert_eq!(f.snapshot(), original);
        assert_eq!(fs::read(file).unwrap(), bytes);
        result.opened = reopened;
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "fresh_process_daily_use_probe"])
            .env("GITMANAGER_LOCAL_FILES_RESTART", f.root.join("recovered"))
            .status()
            .unwrap();
        assert!(child.success());
        let after_restart = open_verified(&f.root.join("recovered"), None).unwrap();
        assert_eq!(
            after_restart.snapshot.data.records.len(),
            original.data.records.len() + 2
        );
        assert_eq!(
            after_restart.snapshot.artifacts.len(),
            original.artifacts.len() + 2
        );
        assert_eq!(f.snapshot(), original);
    }
}

#[test]
fn corrupt_import_and_cancelled_recovery_keep_existing_work() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    let file = f.root.join("backup.gmbak");
    let bytes = VerifiedBackup::capture(&f.store)
        .unwrap()
        .to_bytes()
        .unwrap();
    fs::write(&file, b"corrupt").unwrap();
    assert!(RecoveryDraft::read(&file, "recover").is_err());
    assert_eq!(fs::read(&file).unwrap(), b"corrupt");
    fs::write(&file, bytes).unwrap();
    for active in ["recover", "navigated-away"] {
        let draft = RecoveryDraft::read(&file, "recover").unwrap();
        let result = draft.recover(
            f.reply("recover", "unused"),
            active,
            &AtomicBool::new(active == "recover"),
            &f.recent,
            &f.locations,
            2,
            |_| {},
        );
        assert!(matches!(result, RecoveryOutcome::NotAttempted(_)));
        assert!(!f.root.join("unused").exists());
    }
    assert_eq!(f.snapshot(), snapshot);
}

#[test]
fn doctor_keeps_corruption_and_recovers_only_a_verified_checkpoint() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    let shelf = f.shelf();
    let receipt = checkpoint_before(&f.store, &snapshot, &shelf).unwrap();
    let identity = ToolIdentity::from_snapshot(&snapshot).unwrap();
    fs::write(f.root.join("original/CURRENT"), b"corrupted pointer").unwrap();
    let mut diagnosis = diagnose(&f.root.join("original"), &identity, &shelf, "repair");
    assert!(diagnosis.view.current_issue.is_some());
    assert!(diagnosis.view.recovery.is_some());
    assert_eq!(
        diagnosis
            .view
            .recovery
            .as_ref()
            .unwrap()
            .summary
            .as_ref()
            .unwrap()
            .digest,
        receipt.digest
    );
    let draft = diagnosis.recovery.take().unwrap();
    let result = draft.recover(
        f.reply("repair", "repaired"),
        "repair",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        3,
        |_| {},
    );
    let RecoveryOutcome::Recovered(result) = result else {
        panic!("recovery failed")
    };
    assert_eq!(result.opened.snapshot, snapshot);
    assert_eq!(
        fs::read(f.root.join("original/CURRENT")).unwrap(),
        b"corrupted pointer"
    );
    let store = ProductStore::open(f.root.join("repaired")).unwrap();
    store
        .apply(
            snapshot.revision,
            "continued",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        store.load().unwrap().artifacts.len(),
        snapshot.artifacts.len() + 2
    );
}

#[test]
fn checkpoint_failure_before_action_is_distinct_from_saved_work_warning() {
    let f = Fixture::new();
    let snapshot = f.snapshot();
    let shelf = f.shelf();
    let receipt = checkpoint_before(&f.store, &snapshot, &shelf).unwrap();
    fs::write(&receipt.path, "corrupt checkpoint").unwrap();
    assert!(checkpoint_before(&f.store, &snapshot, &shelf).is_err());
    assert_eq!(f.snapshot(), snapshot);
    let mut opened = open_verified(&f.root.join("original"), None).unwrap();
    let registration = f.recent.remember(&f.root.join("original"), 2);
    assert!(matches!(
        checkpoint_after(&mut opened, &f.locations, &registration),
        CheckpointOutcome::Warning(_)
    ));
    let issue = OperationIssue::new(IssueKind::Unavailable, "recent write unavailable");
    assert!(matches!(
        checkpoint_after(&mut opened, &f.locations, &Err(issue)),
        CheckpointOutcome::RegistrationRequired
    ));
    assert_eq!(opened.store.load().unwrap(), snapshot);
}

#[test]
fn recovered_registration_and_checkpoint_warnings_do_not_erase_success() {
    let f = Fixture::new();
    let file = f.root.join("legacy.gmbak");
    fs::write(&file, legacy_bytes(&f.store)).unwrap();
    let draft = RecoveryDraft::read(&file, "registration-warning").unwrap();
    let result = draft.recover(
        f.reply("registration-warning", "registered-warning"),
        "registration-warning",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        3,
        |progress| {
            if progress == UpgradeProgress::RestartRequired {
                fs::hard_link(
                    f.locations.path().join("recent-tools.json"),
                    f.root.join("recent-alias"),
                )
                .unwrap();
            }
        },
    );
    let RecoveryOutcome::Recovered(result) = result else {
        panic!("saved recovery lost to registration warning")
    };
    assert!(result.registration.is_err());
    assert!(matches!(
        result.checkpoint,
        CheckpointOutcome::RegistrationRequired
    ));
    assert_eq!(result.opened.snapshot.data, f.snapshot().data);
    assert!(recovery_report(&result).contains("recent"));
    let g = Fixture::new();
    fs::write(g.locations.path().join("checkpoints"), "blocked").unwrap();
    let file = g.root.join("current.gmbak");
    VerifiedBackup::capture(&g.store)
        .unwrap()
        .export_new(&file)
        .unwrap();
    let draft = RecoveryDraft::read(&file, "checkpoint-warning").unwrap();
    let RecoveryOutcome::Recovered(result) = draft.recover(
        g.reply("checkpoint-warning", "checkpoint-warning"),
        "checkpoint-warning",
        &AtomicBool::new(false),
        &g.recent,
        &g.locations,
        2,
        |_| {},
    ) else {
        panic!("saved recovery lost to checkpoint warning")
    };
    assert!(result.registration.is_ok());
    assert!(matches!(result.checkpoint, CheckpointOutcome::Warning(_)));
    assert!(recovery_report(&result).contains("backup"));
    assert_eq!(
        open_verified(&g.root.join("checkpoint-warning"), None)
            .unwrap()
            .snapshot
            .data,
        g.snapshot().data
    );
}

#[test]
fn fresh_process_daily_use_probe() {
    let Some(path) = std::env::var_os("GITMANAGER_LOCAL_FILES_RESTART") else {
        return;
    };
    let opened = open_verified(&PathBuf::from(path), None).unwrap();
    opened
        .store
        .apply(
            opened.snapshot.revision,
            "fresh-process-add",
            &invoke(
                "add_person",
                args(&[("name", string("Restart work")), ("area", string("South"))]),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let snapshot = opened.store.load().unwrap();
    opened
        .store
        .apply(
            snapshot.revision,
            "fresh-process-output",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
}

#[test]
fn legacy_recovery_refuses_existing_staged_destinations_and_listed_aliases() {
    let f = Fixture::new();
    let file = f.root.join("legacy.gmbak");
    let bytes = legacy_bytes(&f.store);
    fs::write(&file, &bytes).unwrap();
    let staged = f.root.join("staged");
    let legacy = LegacyBackup::from_bytes(&bytes).unwrap();
    assert!(legacy
        .upgrade_recover_with_fault(&staged, |_| {}, FaultPoint::BeforePointer)
        .is_err());
    assert!(!staged.join("CURRENT").exists());
    let marker = fs::read(staged.join("UPGRADE-INBOX")).unwrap();
    let draft = RecoveryDraft::read(&file, "existing-stage").unwrap();
    let result = draft.recover(
        f.reply("existing-stage", "staged"),
        "existing-stage",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |_| {},
    );
    assert!(
        matches!(result, RecoveryOutcome::AttemptFailed { .. }),
        "ordinary recovery must not silently resume an earlier ambiguous attempt"
    );
    assert!(!staged.join("CURRENT").exists());
    assert_eq!(fs::read(staged.join("UPGRADE-INBOX")).unwrap(), marker);
    let activated = f.root.join("already-recovered");
    legacy.upgrade_recover_new(&activated, |_| {}).unwrap();
    let current = fs::read(activated.join("CURRENT")).unwrap();
    let result = RecoveryDraft::read(&file, "activated").unwrap().recover(
        f.reply("activated", "already-recovered"),
        "activated",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |_| {},
    );
    assert!(matches!(result, RecoveryOutcome::AttemptFailed { .. }));
    assert_eq!(fs::read(activated.join("CURRENT")).unwrap(), current);
    let snapshot = f.snapshot();
    let original_id = f.recent.list().unwrap()[0].tool.id.clone();
    let alias = f.root.join("ORIGINAL");
    let aliases = alias.exists();
    fs::rename(f.root.join("original"), f.root.join("moved-original")).unwrap();
    let recent_before = fs::read(f.locations.path().join("recent-tools.json")).unwrap();
    let same = RecoveryDraft::read(&file, "same").unwrap().recover(
        f.reply("same", "original"),
        "same",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |_| {},
    );
    assert!(matches!(same, RecoveryOutcome::AttemptFailed { .. }));
    assert!(!f.root.join("original/CURRENT").exists());
    let result = RecoveryDraft::read(&file, "alias").unwrap().recover(
        f.reply("alias", "ORIGINAL"),
        "alias",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |_| {},
    );
    if aliases {
        assert!(matches!(result, RecoveryOutcome::AttemptFailed { .. }));
        assert!(
            !alias.join("CURRENT").exists(),
            "instance alias must be refused before CURRENT activation"
        );
        assert_eq!(
            fs::read(f.locations.path().join("recent-tools.json")).unwrap(),
            recent_before
        );
        println!("ALIAS-COVERAGE legacy_recovery: case-insensitive assertions passed");
    } else {
        let RecoveryOutcome::Recovered(result) = result else {
            panic!("distinct case-sensitive fresh path refused")
        };
        assert_ne!(result.registration.unwrap(), original_id);
        println!("ALIAS-COVERAGE legacy_recovery: case-sensitive assertions passed; case-insensitive branch unrun");
    }
    assert_eq!(
        ProductStore::open(f.root.join("moved-original"))
            .unwrap()
            .load()
            .unwrap(),
        snapshot
    );
    assert_eq!(fs::read(file).unwrap(), bytes);
}

#[test]
fn legacy_recovery_rejects_replaced_parent_before_activation() {
    let f = Fixture::new();
    let file = f.root.join("legacy.gmbak");
    fs::write(&file, legacy_bytes(&f.store)).unwrap();
    let parent = f.root.join("destinations");
    fs::create_dir(&parent).unwrap();
    let moved = f.root.join("moved-destinations");
    let draft = RecoveryDraft::read(&file, "replaced").unwrap();
    let result = draft.recover(
        DestinationChoice {
            operation: "replaced".into(),
            destination: Some(parent.join("fresh")),
        },
        "replaced",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |stage| {
            if stage == UpgradeProgress::Staged {
                fs::rename(&parent, &moved).unwrap();
                fs::create_dir(&parent).unwrap();
            }
        },
    );
    assert!(matches!(result, RecoveryOutcome::AttemptFailed { .. }));
    assert!(!moved.join("fresh/CURRENT").exists());
    assert!(!parent.join("fresh/CURRENT").exists());
    assert_eq!(f.recent.list().unwrap().len(), 1);
}

#[test]
fn recovery_reports_saved_but_changed_copy_without_installing_it() {
    let f = Fixture::new();
    let file = f.root.join("legacy.gmbak");
    fs::write(&file, legacy_bytes(&f.store)).unwrap();
    let path = f.root.join("changed-copy");
    let legacy = LegacyBackup::from_bytes(&fs::read(&file).unwrap()).unwrap();
    let created = legacy
        .upgrade_recover_tool(&path, &f.recent, 2, |_| {})
        .unwrap();
    let expected = created.store.load().unwrap();
    // The real helper has now released its lock. Inject work in the actual
    // post-creation/pre-reopen window, without weakening the production lock.
    created
        .store
        .apply(
            expected.revision,
            "concurrent-work",
            &invoke(
                "add_person",
                args(&[("name", string("Concurrent")), ("area", string("East"))]),
            ),
            RuntimeLimits::default(),
        )
        .unwrap();
    let result = test_finish_created_recovery(path.clone(), created, &expected, &f.locations);
    assert!(matches!(
        result,
        RecoveryOutcome::SavedButUnavailable { .. }
    ));
    let view = recovery_outcome_report(&result);
    assert_eq!(view.status, RecoveryStatus::SavedUnavailable);
    assert!(view.message.contains("Do not repeat recovery"));
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        f.snapshot().data.records.len() + 1
    );
}

#[test]
fn legacy_fresh_recovery_refuses_a_target_claimed_during_validation() {
    let f = Fixture::new();
    let file = f.root.join("legacy.gmbak");
    fs::write(&file, legacy_bytes(&f.store)).unwrap();
    let path = f.root.join("claimed");
    let mut competing = None;
    let result = RecoveryDraft::read(&file, "claim").unwrap().recover(
        f.reply("claim", "claimed"),
        "claim",
        &AtomicBool::new(false),
        &f.recent,
        &f.locations,
        2,
        |stage| {
            if stage == UpgradeProgress::Validating {
                let other = ProductStore::create(&path, &capture(organizer()), 20000).unwrap();
                competing = Some(other.load().unwrap());
            }
        },
    );
    assert!(matches!(result, RecoveryOutcome::AttemptFailed { .. }));
    assert_eq!(
        ProductStore::open(path).unwrap().load().unwrap(),
        competing.unwrap()
    );
    assert_eq!(f.recent.list().unwrap().len(), 1);
}
