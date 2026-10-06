//! Hand-authored component checks, not controller/native-user acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
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
use std::fs;

fn backup_source(root: &Path) -> ProductStore {
    let store = create(&root.join("source"));
    let first = store
        .apply(0, "first", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let mut source = organizer();
    source["entities"][0]["fields"].as_array_mut().unwrap().push(serde_json::json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    source["actions"][0]["steps"][0]["values"]["note"] = serde_json::json!({"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"text","value":"Later fact"}});
    let program = capture(source);
    let plan = store.prepare_switch(&program, "switch").unwrap();
    let adopted = store
        .adopt(first.revision, &plan, &program, &first.decisions)
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
    store
        .apply(
            selected.revision,
            "export",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    store
}

#[test]
fn verified_backups_recover_later_work_and_outputs_in_a_fresh_process() {
    use product_backup::VerifiedBackup;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = backup_source(&root);
    let saved = store.load().unwrap();
    let backup = VerifiedBackup::capture(&store).unwrap();
    assert_eq!(backup.snapshot(), &saved);
    let file = root.join("my-tool.gmbak");
    backup.export_new(&file).unwrap();
    let bytes = fs::read(&file).unwrap();
    assert!(backup.export_new(&file).is_err());
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert!(backup.recover_new(&root.join("source")).is_err());
    assert_eq!(store.load().unwrap(), saved);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "backup_recovery_child_probe"])
        .env("GITMANAGER_BACKUP_PROBE", &file)
        .status()
        .unwrap();
    assert!(child.success());
    let recovered = product_backup::open_verified(&root.join("fresh-recovery"), None).unwrap();
    assert_eq!(
        recovered.snapshot.data.records.len(),
        saved.data.records.len() + 1
    );
    assert_eq!(recovered.snapshot.artifacts, saved.artifacts);
    assert_eq!(
        &recovered.snapshot.data.events[..saved.data.events.len()],
        saved.data.events
    );
    assert_eq!(
        recovered.snapshot.data.records[1].values["note"],
        string("Later fact")
    );
    assert_eq!(fs::read(&file).unwrap(), bytes);
    assert_eq!(store.load().unwrap(), saved);
}

#[test]
fn backup_recovery_child_probe() {
    let Some(file) = std::env::var_os("GITMANAGER_BACKUP_PROBE") else {
        return;
    };
    let file = std::path::PathBuf::from(file);
    let backup = product_backup::VerifiedBackup::read(&file).unwrap();
    let recent = product_locations::RecentTools::open(file.parent().unwrap()).unwrap();
    let created = backup
        .recover_tool(&file.parent().unwrap().join("fresh-recovery"), &recent, 1)
        .unwrap();
    let id = created.registration.unwrap();
    assert_eq!(recent.list().unwrap()[0].tool.id, id);
    let recovered = created.store;
    let saved = recovered.load().unwrap();
    assert_eq!(&saved, backup.snapshot());
    recovered
        .apply(
            saved.revision,
            "continued",
            &add("Fresh process"),
            RuntimeLimits::default(),
        )
        .unwrap();
}

#[test]
fn recovered_intention_history_is_reachable_and_rechecked_by_a_fresh_engine() {
    use product_backup::VerifiedBackup;
    use product_decisions::{
        accept_scene, Choice, DecisionEngine, IntentArchive, IntentionBinding,
    };
    use product_runtime::LocalRuntime;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let source = root.join("source");
    let program = capture(organizer());
    let store = ProductStore::create(&source, &program, 20000).unwrap();
    let runtime = LocalRuntime::default();
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    let choice = |id: &str, operation: &str, outcome| Choice {
        id: id.into(),
        request: format!("Keep the observed {operation} work"),
        rationale: None,
        scope: DecisionScope {
            operations: [operation.into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let accepted = scenario(
        &program,
        vec![
            add("Accepted scene"),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
    );
    let scene = accept_scene(
        &runtime,
        &program,
        &accepted,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let change = engine
        .prepare_choice(
            &store,
            &program,
            choice("keep-create", "add_person", DecisionOutcome::KeepCurrent),
            vec![scene],
            "accept-create",
        )
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let exported = scenario(
        &program,
        vec![
            add("Accepted export"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "exported".into(),
            },
        ],
    );
    let scene = accept_scene(
        &runtime,
        &program,
        &exported,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let deferred = engine
        .prepare_choice(
            &store,
            &program,
            choice("later-export", "export_people", DecisionOutcome::Deferred),
            vec![scene],
            "defer-export",
        )
        .unwrap();
    let current = engine.adopt(&store, &deferred).unwrap();
    let current = store
        .apply(
            current.revision,
            "daily-record",
            &add("Later real work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let selected = store
        .apply(
            current.revision,
            "daily-select",
            &invoke("collect", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let saved = store
        .apply(
            selected.revision,
            "daily-output",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(saved.decisions.decisions.len(), 2);
    let backup = VerifiedBackup::capture(&store).unwrap();
    assert!(backup.summary().unwrap().intention_objects >= 2);
    let file = root.join("with-intentions.gmbak");
    backup.export_new(&file).unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "backup_recovery_child_probe"])
        .env("GITMANAGER_BACKUP_PROBE", &file)
        .status()
        .unwrap();
    assert!(child.success());
    let target = root.join("fresh-recovery");
    let reopened = ProductStore::open(&target).unwrap();
    let resumed = reopened.load().unwrap();
    assert_eq!(resumed.decisions, saved.decisions);
    assert_eq!(resumed.adoptions, saved.adoptions);
    assert_eq!(resumed.data.records.len(), saved.data.records.len() + 1);
    assert_eq!(resumed.artifacts, saved.artifacts);
    let recovered_bundle = IntentArchive::new(reopened.clone())
        .export_for(&resumed)
        .unwrap();
    assert_eq!(
        recovered_bundle.snapshot_digest(),
        &canonical_digest(IdentityDomain::Data, &resumed).unwrap()
    );
    assert_eq!(
        recovered_bundle.object_count(),
        backup.summary().unwrap().intention_objects
    );
    let fresh = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(reopened.clone()),
    );
    assert_eq!(
        fresh
            .intention_binding(&saved.decisions.decisions[0])
            .unwrap(),
        IntentionBinding::ObservedOutcome
    );
    // A later adoption uses the recovered packages through the real engine,
    // rerunning earlier obligations rather than accepting imported pass flags.
    let scene = accept_scene(
        &runtime,
        &program,
        &accepted,
        Disclosure::Synthetic,
        RuntimeLimits::default(),
    )
    .unwrap();
    let followup = fresh
        .prepare_choice(
            &reopened,
            &program,
            choice("after-recovery", "add_person", DecisionOutcome::KeepCurrent),
            vec![scene],
            "rechecked",
        )
        .unwrap();
    let checked = fresh.adopt(&reopened, &followup).unwrap();
    assert_eq!(checked.data, resumed.data);
    assert_eq!(checked.artifacts, resumed.artifacts);
    assert_eq!(checked.decisions.decisions.len(), 3);
    assert_eq!(store.load().unwrap(), saved);
    // Missing or damaged retained packages block opening and further backup.
    let witness = &saved.decisions.decisions[0].witness;
    let object = target.join(format!("extension-{}.json", witness.as_str()));
    fs::write(&object, b"damaged intention").unwrap();
    assert!(product_backup::open_verified(&target, None).is_err());
    assert!(VerifiedBackup::capture(&reopened).is_err());
    assert_eq!(fs::read(&object).unwrap(), b"damaged intention");
}

#[test]
fn backup_intake_rejects_corruption_future_versions_and_oversized_files() {
    use product_backup::{VerifiedBackup, MAX_BACKUP_BYTES};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = create(&root.join("source"));
    let before = store.load().unwrap();
    let backup = VerifiedBackup::capture(&store).unwrap();
    let bytes = backup.to_bytes().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut future = value.clone();
    future["version"] = 99.into();
    let mut corrupt = value.clone();
    corrupt["payload"]["snapshot"]["revision"] = 200.into();
    let mut extra = value;
    extra["../../escape"] = "untrusted".into();
    for bad in [
        canonical_bytes(&future).unwrap(),
        canonical_bytes(&corrupt).unwrap(),
        canonical_bytes(&extra).unwrap(),
        b"{unfinished".to_vec(),
        [bytes.clone(), b" ".to_vec()].concat(),
    ] {
        assert!(VerifiedBackup::from_bytes(&bad).is_err());
    }
    let huge = root.join("huge.gmbak");
    fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_BACKUP_BYTES as u64 + 1)
        .unwrap();
    assert!(VerifiedBackup::read(&huge).is_err());
    assert_eq!(store.load().unwrap(), before);
    assert!(!root.join("escape").exists());
}

#[test]
fn backup_checksums_do_not_authorize_bad_bundle_references_or_runtime_data() {
    use product_backup::VerifiedBackup;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = create(&root.join("source"));
    let bytes = VerifiedBackup::capture(&store).unwrap().to_bytes().unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut cases = Vec::new();
    let mut wrong = original.clone();
    wrong["payload"]["intentions"]["snapshot"] = "0".repeat(64).into();
    cases.push(wrong);
    let mut future = original.clone();
    future["payload"]["intentions"]["version"] = 99.into();
    cases.push(future);
    let mut extra = original.clone();
    extra["payload"]["intentions"]["objects"] =
        serde_json::json!([{ "digest": "0".repeat(64), "bytes": [123, 125] }]);
    cases.push(extra);
    let mut huge = original.clone();
    huge["payload"]["intentions"]["objects"] = serde_json::json!(vec![
        serde_json::json!({ "digest": "0".repeat(64), "bytes": [123, 125] });
        513
    ]);
    cases.push(huge);
    let mut invalid = original;
    invalid["payload"]["snapshot"]["session"]["view"] = "absent-view".into();
    cases.push(invalid);
    for mut value in cases {
        value["digest"] = serde_json::to_value(
            canonical_digest(IdentityDomain::Evidence, &value["payload"]).unwrap(),
        )
        .unwrap();
        assert!(VerifiedBackup::from_bytes(&canonical_bytes(&value).unwrap()).is_err());
    }
    let duplicate = [b"{\"version\":1,".as_slice(), &bytes[1..]].concat();
    assert!(VerifiedBackup::from_bytes(&duplicate).is_err());
    assert!(store.load().is_ok());
}

#[test]
fn checkpoints_capture_only_consistent_committed_snapshots_during_writes() {
    use product_backup::VerifiedBackup;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = create(&root.join("source"));
    let initial = store.load().unwrap();
    let writer = store.clone();
    let thread = std::thread::spawn(move || {
        let mut committed = vec![initial];
        for i in 0..24 {
            let saved = writer
                .apply(
                    i,
                    &format!("write-{i}"),
                    &add(&format!("Record {i}")),
                    RuntimeLimits::default(),
                )
                .unwrap();
            committed.push(saved);
        }
        committed
    });
    let mut captures = Vec::new();
    for _ in 0..12 {
        captures.push(VerifiedBackup::capture(&store).unwrap());
    }
    let committed = thread.join().unwrap();
    for backup in captures {
        assert!(
            committed.contains(backup.snapshot()),
            "backup mixed data from separate commits"
        );
        VerifiedBackup::from_bytes(&backup.to_bytes().unwrap()).unwrap();
    }
}

#[test]
fn backup_capture_ignores_an_interrupted_uncommitted_snapshot() {
    use product_backup::VerifiedBackup;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = create(&root.join("source"));
    let committed = store
        .apply(0, "saved", &add("Saved"), RuntimeLimits::default())
        .unwrap();
    let before = VerifiedBackup::capture(&store).unwrap();
    assert!(store
        .apply_with_fault(
            committed.revision,
            "interrupted",
            &add("Uncommitted"),
            RuntimeLimits::default(),
            FaultPoint::AfterObject
        )
        .is_err());
    let after = VerifiedBackup::capture(&store).unwrap();
    assert_eq!(after.snapshot(), &committed);
    assert_eq!(after.digest(), before.digest());
    assert_eq!(after.to_bytes().unwrap(), before.to_bytes().unwrap());
}

#[test]
fn doctor_offers_only_verified_instance_checkpoints_and_preserves_corruption() {
    use product_backup::{doctor, CheckpointShelf};
    use product_locations::{RecentTools, ToolIdentity, ToolLocations};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let locations = ToolLocations::chosen(&root).unwrap();
    let store = create(&root.join("source"));
    let saved = store
        .apply(0, "save", &add("Saved"), RuntimeLimits::default())
        .unwrap();
    let identity = ToolIdentity::from_snapshot(&saved).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let instance = recent.remember(&root.join("source"), 1).unwrap();
    let shelf = CheckpointShelf::for_tool(&locations, &identity, &instance).unwrap();
    let first = shelf.capture(&store).unwrap();
    assert_eq!(first.digest, shelf.capture(&store).unwrap().digest);
    let later = store
        .apply(
            saved.revision,
            "later",
            &add("Later"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let last = shelf.capture(&store).unwrap();
    let current_pointer = fs::read(root.join("source/CURRENT")).unwrap();
    fs::write(&last.path, b"interrupted final backup").unwrap();
    fs::write(
        shelf.path().join(".pending-unfinished"),
        b"not a checkpoint",
    )
    .unwrap();
    // Untrusted directory names must not reach byte-indexing panics.
    fs::write(
        shelf
            .path()
            .join(format!("checkpoint-{}x.gmbak", "é".repeat(42))),
        b"untrusted",
    )
    .unwrap();
    fs::write(root.join("source/CURRENT"), b"broken current").unwrap();
    let report = doctor(&root.join("source"), &identity, &shelf);
    assert!(report.current.is_none());
    assert!(report.issue.is_some());
    let recovered = report.recovery.unwrap();
    assert_eq!(recovered.snapshot(), &saved);
    assert!(!report.checkpoint_issues.is_empty());
    recovered.recover_new(&root.join("recovered")).unwrap();
    assert_eq!(
        fs::read(root.join("source/CURRENT")).unwrap(),
        b"broken current"
    );
    assert_eq!(fs::read(&last.path).unwrap(), b"interrupted final backup");
    assert_eq!(later.data.records.len(), 2);
    fs::write(root.join("source/CURRENT"), &current_pointer).unwrap();
    let pointer: serde_json::Value = serde_json::from_slice(&current_pointer).unwrap();
    let object = root.join("source").join(format!(
        "object-{}.json",
        pointer["object"].as_str().unwrap()
    ));
    fs::write(&object, b"broken snapshot object").unwrap();
    assert!(doctor(&root.join("source"), &identity, &shelf)
        .recovery
        .is_some());
    assert_eq!(fs::read(&object).unwrap(), b"broken snapshot object");
    fs::write(&first.path, b"also corrupt").unwrap();
    assert!(doctor(&root.join("source"), &identity, &shelf)
        .recovery
        .is_none());
}

#[cfg(unix)]
#[test]
fn backup_files_refuse_link_redirection_and_leave_targets_unchanged() {
    use product_backup::VerifiedBackup;
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = create(&root.join("source"));
    let backup = VerifiedBackup::capture(&store).unwrap();
    let original = root.join("original.gmbak");
    backup.export_new(&original).unwrap();
    let bytes = fs::read(&original).unwrap();
    let link = root.join("linked.gmbak");
    symlink(&original, &link).unwrap();
    assert!(VerifiedBackup::read(&link).is_err());
    assert!(backup.export_new(&link).is_err());
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.join("redirected-parent")).unwrap();
    assert!(backup
        .recover_new(&root.join("redirected-parent/recovery"))
        .is_err());
    assert!(!outside.path().join("recovery").exists());
    fs::hard_link(&original, root.join("hard.gmbak")).unwrap();
    assert!(VerifiedBackup::read(&original).is_err());
    assert_eq!(fs::read(&original).unwrap(), bytes);
}

#[test]
fn locations_keep_explicit_choices_and_refuse_unsafe_or_unavailable_folders() {
    use product_locations::ToolLocations;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    assert!(ToolLocations::chosen(root.join("missing")).is_err());
    let chosen = ToolLocations::chosen(&root).unwrap();
    let first = chosen.new_tool_path().unwrap();
    let second = chosen.new_tool_path().unwrap();
    assert_eq!(first.parent(), Some(root.as_path()));
    assert_ne!(first, second);
    create(&first);
    assert!(ProductStore::create(&first, &capture(organizer()), 20000).is_err());
    fs::write(root.join("file"), b"preserve").unwrap();
    assert!(ToolLocations::chosen(root.join("file")).is_err());
    assert!(ToolLocations::chosen(root.join("../escape")).is_err());
    for name in [
        "CON",
        "aux.txt",
        "COM¹.log",
        "stream:secret",
        "trailing.",
        "wild*card",
    ] {
        assert_eq!(
            product_locations::SelectedFile::new(&root.join(name))
                .err()
                .unwrap()
                .kind,
            product_locations::IssueKind::UnsafePath
        );
    }
    #[cfg(unix)]
    assert_eq!(
        product_locations::SelectedFile::new(&root.join("back\\slash"))
            .err()
            .unwrap()
            .kind,
        product_locations::IssueKind::UnsafePath
    );
    let default = ToolLocations::create_default_at(&root.join("private/tools")).unwrap();
    assert!(default
        .new_tool_path()
        .unwrap()
        .starts_with(root.join("private/tools")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root.join("private/tools"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn recent_tools_survive_restart_and_require_deliberate_identity_checked_relocation() {
    use product_locations::{RecentAvailability, RecentTools};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let settings = root.join("settings");
    fs::create_dir(&settings).unwrap();
    let recent = RecentTools::open(&settings).unwrap();
    let path = root.join("tool");
    let store = create(&path);
    let id = recent.remember(&path, 10).unwrap();
    drop(store);
    let moved = root.join("moved");
    fs::rename(&path, &moved).unwrap();
    let reopened = RecentTools::open(&settings).unwrap();
    assert_eq!(
        reopened.list().unwrap()[0].availability,
        RecentAvailability::Missing
    );
    reopened.relocate(&id, &moved, 20).unwrap();
    let entry = reopened.list().unwrap().remove(0);
    assert_eq!(entry.availability, RecentAvailability::Located);
    assert_eq!(entry.tool.path, moved);
    let foreign = root.join("foreign");
    let mut program = capture(organizer());
    program.binding.project_id = "other-project".into();
    ProductStore::create(&foreign, &program, 20000).unwrap();
    assert!(reopened.relocate(&id, &foreign, 30).is_err());
    assert_eq!(reopened.list().unwrap()[0].tool.path, moved);
    fs::write(moved.join("CURRENT"), b"broken").unwrap();
    assert_eq!(
        reopened.list().unwrap()[0].availability,
        RecentAvailability::Unverified
    );
    assert_eq!(fs::read(moved.join("CURRENT")).unwrap(), b"broken");
}

#[test]
fn a_recovered_copy_at_a_relocated_tools_old_path_gets_a_distinct_instance() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let original = root.join("original");
    let store = create(&original);
    let snapshot = store.load().unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let old_id = recent.remember(&original, 1).unwrap();
    drop(store);
    let moved = root.join("moved");
    fs::rename(&original, &moved).unwrap();
    recent.relocate(&old_id, &moved, 2).unwrap();
    ProductStore::create_recovered(&original, &snapshot).unwrap();
    let new_id = recent.remember(&original, 3).unwrap();
    assert_ne!(
        old_id, new_id,
        "separate recovered instances must not share a checkpoint shelf"
    );
    let entries = RecentTools::open(&root).unwrap().list().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(recent.remember(&moved, 4).unwrap(), old_id);
    assert_eq!(recent.remember(&original, 5).unwrap(), new_id);
}

#[test]
fn recovery_registration_cannot_inherit_an_unrelocated_instances_checkpoints() {
    use product_backup::{CheckpointShelf, VerifiedBackup};
    use product_locations::{RecentTools, ToolIdentity, ToolLocations};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let original = root.join("original");
    let store = create(&original);
    let older = VerifiedBackup::capture(&store).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let old_id = recent.remember(&original, 1).unwrap();
    let current = store
        .apply(
            0,
            "newer",
            &add("Newer original work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let identity = ToolIdentity::from_snapshot(&current).unwrap();
    let locations = ToolLocations::chosen(&root).unwrap();
    let old_shelf = CheckpointShelf::for_tool(&locations, &identity, &old_id).unwrap();
    old_shelf.capture(&store).unwrap();
    drop(store);
    let moved = root.join("moved-without-updating-recents");
    fs::rename(&original, &moved).unwrap();
    let before = fs::read(root.join("recent-tools.json")).unwrap();
    let issue = older.recover_tool(&original, &recent, 2).err().unwrap();
    assert_eq!(issue.kind, product_locations::IssueKind::Collision);
    assert!(
        !original.exists(),
        "refusal must precede creating the recovered target"
    );
    assert_eq!(fs::read(root.join("recent-tools.json")).unwrap(), before);
    assert_eq!(ProductStore::open(&moved).unwrap().load().unwrap(), current);
    let destination = root.join("separate-recovery");
    let created = older.recover_tool(&destination, &recent, 3).unwrap();
    let recovered_id = created.registration.unwrap();
    assert_ne!(recovered_id, old_id);
    assert_eq!(created.store.load().unwrap(), *older.snapshot());
    let recovered_shelf = CheckpointShelf::for_tool(&locations, &identity, &recovered_id).unwrap();
    assert!(recovered_shelf
        .newest_verified()
        .unwrap()
        .recovery
        .is_none());
    recovered_shelf.capture(&created.store).unwrap();
    fs::write(destination.join("CURRENT"), b"damaged").unwrap();
    assert_eq!(
        product_backup::doctor(&destination, &identity, &recovered_shelf)
            .recovery
            .unwrap()
            .snapshot(),
        older.snapshot()
    );
    assert_eq!(
        old_shelf
            .newest_verified()
            .unwrap()
            .recovery
            .unwrap()
            .snapshot(),
        &current
    );
    let entries = recent.list().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.tool.id == old_id)
            .unwrap()
            .availability,
        product_locations::RecentAvailability::Missing
    );
}

#[test]
fn recovery_refuses_a_listed_case_alias_before_activation() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let original = root.join("original");
    let store = create(&original);
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let original_id = recent.remember(&original, 1).unwrap();
    // Exercise whichever filesystem semantics the actual platform provides.
    let alias = root.join("ORIGINAL");
    let aliases = alias.exists();
    drop(store);
    let moved = root.join("moved");
    fs::rename(&original, &moved).unwrap();
    let before = fs::read(root.join("recent-tools.json")).unwrap();
    let result = backup.recover_tool(&alias, &recent, 2);
    if aliases {
        assert!(
            result.is_err(),
            "case alias cannot inherit an old recent identity"
        );
        assert!(
            !alias.join("CURRENT").exists(),
            "alias refusal must happen before activation"
        );
        assert_eq!(fs::read(root.join("recent-tools.json")).unwrap(), before);
    } else {
        let created = result.unwrap();
        assert_ne!(created.registration.unwrap(), original_id);
        assert!(!original.exists());
        assert_eq!(created.store.load().unwrap(), *backup.snapshot());
    }
    assert_eq!(
        ProductStore::open(&moved).unwrap().load().unwrap(),
        *backup.snapshot()
    );
}

#[test]
fn recent_relocation_cannot_merge_a_recovered_instance_through_an_alias() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let original = root.join("original");
    let store = create(&original);
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let original_id = recent.remember(&original, 1).unwrap();
    drop(store);
    let moved = root.join("moved-original");
    fs::rename(&original, &moved).unwrap();
    let recovered = root.join("recovered");
    let result = backup.recover_tool(&recovered, &recent, 2).unwrap();
    let recovered_id = result.registration.unwrap();
    let alias = root.join("RECOVERED");
    // On a case-sensitive volume, verify the exact-directory collision; on
    // case-insensitive volumes, exercise the distinct-spelling alias directly.
    let selected = if alias.exists() { &alias } else { &recovered };
    assert_eq!(recent.remember(selected, 3).unwrap(), recovered_id);
    assert_eq!(recent.list().unwrap().len(), 2);
    let before = fs::read(root.join("recent-tools.json")).unwrap();
    assert!(recent.relocate(&original_id, selected, 4).is_err());
    assert_eq!(fs::read(root.join("recent-tools.json")).unwrap(), before);
    recent.relocate(&original_id, &moved, 5).unwrap();
    assert_eq!(recent.remember(&moved, 6).unwrap(), original_id);
    assert_eq!(result.store.load().unwrap(), *backup.snapshot());
}

#[cfg(unix)]
#[test]
fn recovery_does_not_activate_when_recent_folder_is_replaced() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let source = create(&root.join("source"));
    let saved = source.load().unwrap();
    let settings = root.join("settings");
    fs::create_dir(&settings).unwrap();
    let recent = RecentTools::open(&settings).unwrap();
    recent.remember(&root.join("source"), 1).unwrap();
    let previous = fs::read(settings.join("recent-tools.json")).unwrap();
    let destination = root.join("fresh");
    let moved = root.join("moved-settings");
    let result = recent.create_and_remember(&destination, 2, |verify_instance| {
        ProductStore::create_recovered_with(&destination, &saved, |_| {
            fs::rename(&settings, &moved).unwrap();
            fs::create_dir(&settings).unwrap();
            verify_instance().map_err(|e| StoreError::Conflict(e.to_string()))
        })
        .map_err(Into::into)
    });
    assert!(
        result.is_err(),
        "unverified recent-folder identity must prevent activation"
    );
    assert!(!destination.join("CURRENT").exists());
    assert_eq!(fs::read(moved.join("recent-tools.json")).unwrap(), previous);
    assert!(!settings.join("recent-tools.json").exists());
    assert_eq!(source.load().unwrap(), saved);
}

#[test]
fn recovered_data_survives_recent_registration_failure_and_retry() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let source = root.join("source");
    let store = create(&source);
    let backup = product_backup::VerifiedBackup::capture(&store).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    recent.remember(&source, 1).unwrap();
    let metadata = root.join("recent-tools.json");
    let previous = fs::read(&metadata).unwrap();
    let destination = root.join("fresh");
    let result = recent
        .create_and_remember(&destination, 2, |verify_instance| {
            let recovered = backup.recover_checked(&destination, verify_instance)?;
            // Model an index-write failure after successful fresh activation.
            fs::hard_link(&metadata, root.join("metadata-alias")).unwrap();
            Ok(recovered)
        })
        .unwrap();
    let issue = result.registration.unwrap_err();
    assert!(issue.message.contains("was saved"));
    assert!(issue.next_step.contains("Do not repeat recovery"));
    assert_eq!(result.store.load().unwrap(), *backup.snapshot());
    assert_eq!(
        product_backup::open_verified(&destination, None)
            .unwrap()
            .snapshot,
        *backup.snapshot()
    );
    assert_eq!(fs::read(&metadata).unwrap(), previous);
    assert_eq!(store.load().unwrap(), *backup.snapshot());
    // A corrupt/inaccessible index must stop a later recovery before creation.
    let another = root.join("another");
    assert!(backup.recover_tool(&another, &recent, 3).is_err());
    assert!(!another.exists());
    assert_eq!(fs::read(&metadata).unwrap(), previous);
}

#[test]
fn recent_metadata_corruption_and_future_versions_are_preserved() {
    use product_locations::RecentTools;
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let recent = RecentTools::open(&root).unwrap();
    let project = root.join("tool");
    create(&project);
    recent.remember(&project, 1).unwrap();
    let path = root.join("recent-tools.json");
    let original = fs::read(&path).unwrap();
    let mut future: serde_json::Value = serde_json::from_slice(&original).unwrap();
    future["version"] = 99.into();
    for invalid in [
        b"{partial".to_vec(),
        canonical_bytes(&future).unwrap(),
        vec![b'x'; 1024 * 1024 + 1],
    ] {
        fs::write(&path, &invalid).unwrap();
        assert!(recent.list().is_err());
        assert!(recent.remember(&project, 2).is_err());
        assert_eq!(fs::read(&path).unwrap(), invalid);
    }
    fs::write(&path, &original).unwrap();
    assert_eq!(recent.list().unwrap().len(), 1);
    let mut full: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let template = full["entries"][0].clone();
    full["entries"] = serde_json::Value::Array(
        (0..512)
            .map(|i| {
                let mut entry = template.clone();
                entry["id"] =
                    serde_json::to_value(canonical_digest(IdentityDomain::Evidence, &i).unwrap())
                        .unwrap();
                entry["path"] = serde_json::to_value(root.join(format!("remembered-{i}"))).unwrap();
                entry
            })
            .collect(),
    );
    let full = canonical_bytes(&full).unwrap();
    fs::write(&path, &full).unwrap();
    let issue = recent.remember(&project, 3).unwrap_err();
    assert_eq!(issue.kind, product_locations::IssueKind::Limit);
    assert!(issue.next_step.contains("folder picker"));
    assert_eq!(fs::read(&path).unwrap(), full);
    assert!(ProductStore::open(&project).unwrap().load().is_ok());
}

#[cfg(unix)]
#[test]
fn location_and_recent_links_cannot_redirect_reads_or_writes() {
    use product_locations::{RecentTools, ToolLocations};
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    symlink(outside.path(), root.join("redirect")).unwrap();
    assert!(ToolLocations::chosen(root.join("redirect")).is_err());
    let target = outside.path().join("private");
    fs::write(&target, b"preserved").unwrap();
    fs::hard_link(&target, root.join("recent-tools.json")).unwrap();
    assert!(RecentTools::open(&root).unwrap().list().is_err());
    assert_eq!(fs::read(&target).unwrap(), b"preserved");
    let locations = ToolLocations::chosen(&root).unwrap();
    let parked = root.with_file_name(format!(
        "{}-parked",
        root.file_name().unwrap().to_string_lossy()
    ));
    fs::rename(&root, &parked).unwrap();
    symlink(outside.path(), &root).unwrap();
    assert!(locations.new_tool_path().is_err());
    fs::remove_file(&root).unwrap();
    fs::rename(&parked, &root).unwrap();
}

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
    assert!(product_backup::VerifiedBackup::capture(&store).is_err());
    assert!(product_backup::open_verified(&dir.path().join("source"), None).is_err());
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
