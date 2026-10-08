//! Offline controller regressions. No live AI, native GUI, or user acceptance proof.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/ui/product_runtime_view.rs"]
pub(crate) mod product_runtime_view;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "../src/product_studio.rs"]
mod product_studio;
use fixture::*;
use product_contract::*;
use product_store::ProductStore;
use product_studio::{ProductStudio, TestHooks, TestPause};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

fn until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "controller did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn settle(studio: &mut ProductStudio) {
    until(|| {
        studio.poll();
        !studio.is_busy()
    });
}
fn root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    (temp, root)
}
fn saved(root: &std::path::Path) -> PathBuf {
    let path = root.join("existing");
    ProductStore::create(&path, &capture(organizer()), 20000).unwrap();
    path
}
fn open(studio: &mut ProductStudio, path: &std::path::Path) {
    studio.test_open(path.into());
    settle(studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
}
fn stop(studio: ProductStudio, stopped: &Arc<AtomicBool>) {
    drop(studio);
    until(|| stopped.load(Ordering::Acquire));
}

#[test]
fn offline_daily_reopen_retains_exact_work_and_does_not_touch_legacy_files() {
    let (_temp, root) = root();
    let path = saved(&root);
    let legacy = root.join("legacy.json");
    fs::write(&legacy, b"legacy bytes left alone").unwrap();
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut studio);
    open(&mut studio, &path);
    studio.test_daily(add("Mira"));
    settle(&mut studio);
    assert_eq!(
        ProductStore::open(&path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        1
    );
    assert_eq!(studio.test_runtime().unwrap().retained_records.len(), 1);
    stop(studio, &stopped);
    let mut restarted = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "daily");
    restarted.test_daily(add("Noor"));
    settle(&mut restarted);
    assert_eq!(
        ProductStore::open(&path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        2
    );
    assert_eq!(fs::read(legacy).unwrap(), b"legacy bytes left alone");
}

#[test]
fn accepted_precommit_cancel_and_close_prevent_a_write_and_late_reopen() {
    let (_temp, root) = root();
    let path = saved(&root);
    let pause = Arc::new(TestPause::default());
    let hooks = TestHooks {
        before_commit: Some(pause.clone()),
        ..TestHooks::default()
    };
    let mut studio = ProductStudio::testing(root, None, hooks);
    settle(&mut studio);
    open(&mut studio, &path);
    studio.test_daily(add("Cancelled"));
    until(|| pause.reached.load(Ordering::Acquire));
    assert!(studio.test_cancel());
    studio.test_close();
    pause.release.store(true, Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    assert!(ProductStore::open(path)
        .unwrap()
        .load()
        .unwrap()
        .data
        .records
        .is_empty());
}

#[test]
fn commit_cannot_be_cancelled_and_close_reports_the_actual_saved_result() {
    let (_temp, root) = root();
    let path = saved(&root);
    let pause = Arc::new(TestPause::default());
    let hooks = TestHooks {
        after_commit: Some(pause.clone()),
        ..TestHooks::default()
    };
    let mut studio = ProductStudio::testing(root, None, hooks);
    settle(&mut studio);
    open(&mut studio, &path);
    studio.test_daily(add("Kept"));
    until(|| pause.reached.load(Ordering::Acquire));
    assert!(!studio.test_cancel());
    studio.test_close();
    pause.release.store(true, Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    assert!(
        studio.test_notice().contains("saved"),
        "{}",
        studio.test_notice()
    );
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        1
    );
}

#[test]
fn lost_daily_ack_is_reconciled_once_and_double_click_does_not_duplicate() {
    let (_temp, root) = root();
    let path = saved(&root);
    let hooks = TestHooks {
        lose_ack: Arc::new(AtomicBool::new(true)),
        duplicate_completion: true,
        ..TestHooks::default()
    };
    let mut studio = ProductStudio::testing(root, None, hooks);
    settle(&mut studio);
    open(&mut studio, &path);
    studio.test_daily(add("Exactly once"));
    studio.test_daily(add("Unaccepted second click"));
    settle(&mut studio);
    let current = ProductStore::open(path).unwrap().load().unwrap();
    assert_eq!(current.data.records.len(), 1);
    assert_eq!(current.operations.len(), 1);
    assert_eq!(studio.test_runtime().unwrap().retained_records.len(), 1);
}

#[test]
fn stale_external_source_data_and_session_reject_the_original_operation() {
    for change in ["source", "data", "session"] {
        let (_temp, root) = root();
        let path = saved(&root);
        let mut studio = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut studio);
        open(&mut studio, &path);
        let store = ProductStore::open(&path).unwrap();
        match change {
            "source" => {
                let mut source = organizer();
                source["label"] = serde_json::json!("Externally changed source");
                let source = capture(source);
                let plan = store.prepare_switch(&source, "external-source").unwrap();
                let snapshot = store.load().unwrap();
                store
                    .adopt(snapshot.revision, &plan, &source, &snapshot.decisions)
                    .unwrap();
            }
            "data" => {
                store
                    .apply(
                        0,
                        "external-data",
                        &add("External fact"),
                        RuntimeLimits::default(),
                    )
                    .unwrap();
            }
            _ => {
                store
                    .apply(
                        0,
                        "external-session",
                        &SemanticInput::Navigate {
                            view: "new_person".into(),
                        },
                        RuntimeLimits::default(),
                    )
                    .unwrap();
            }
        }
        let before = store.load().unwrap();
        studio.test_daily(add("Stale"));
        settle(&mut studio);
        assert_eq!(
            store.load().unwrap(),
            before,
            "stale {change} must not write"
        );
        assert!(
            studio.test_notice().contains("changed"),
            "{}",
            studio.test_notice()
        );
    }
}

#[test]
fn corrupt_or_future_project_is_left_intact_and_unavailable() {
    for bytes in [
        br#"{"version":999,"snapshot":"future"}"#.as_slice(),
        b"broken".as_slice(),
    ] {
        let (_temp, root) = root();
        let path = saved(&root);
        fs::write(path.join("CURRENT"), bytes).unwrap();
        let mut studio = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut studio);
        studio.test_open(path.clone());
        settle(&mut studio);
        assert_ne!(studio.test_page(), "daily");
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), bytes);
    }
}

#[test]
fn folder_dialog_cancellation_is_explicit_and_never_uses_a_fallback() {
    let (_temp, root) = root();
    let mut studio = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut studio);
    studio.test_choose_open(None);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    assert!(studio.test_notice().contains("cancelled"));
    assert!(product_locations::RecentTools::open(&root)
        .unwrap()
        .list()
        .unwrap()
        .is_empty());
}

#[test]
fn postcommit_checkpoint_failure_keeps_saved_work_and_never_repeats_input() {
    let (_temp, root) = root();
    let path = saved(&root);
    fs::write(root.join("checkpoints"), b"blocked backup folder").unwrap();
    let mut studio = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut studio);
    open(&mut studio, &path);
    studio.test_daily(add("Saved despite backup warning"));
    settle(&mut studio);
    assert_eq!(studio.test_runtime().unwrap().retained_records.len(), 1);
    assert!(studio.test_notice().contains("backup"));
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .operations
            .len(),
        1
    );
}

#[test]
fn corrupted_restart_association_does_not_overwrite_or_autosubmit() {
    let (_temp, root) = root();
    fs::create_dir(root.join("studio")).unwrap();
    let journal = root.join("studio/session.json");
    let original = br#"{"version":999,"need":"preserve me"}"#;
    fs::write(&journal, original).unwrap();
    let mut studio = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut studio);
    assert!(studio.test_notice().contains("restart"));
    assert_eq!(fs::read(journal).unwrap(), original);
}

#[test]
fn restart_reconciles_creation_and_committed_input_without_duplicates() {
    let (_temp, root) = root();
    let path = saved(&root);
    product_studio::test_stage_creation(&root, &path);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut s = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut s);
    assert_eq!(s.test_page(), "daily");
    stop(s, &stopped);
    product_studio::test_stage_daily(&root, &path, add("Survived lost acknowledgement"), true);
    let mut s = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut s);
    assert_eq!(s.test_runtime().unwrap().retained_records.len(), 1);
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .operations
            .len(),
        1
    );
}
#[test]
fn restart_never_autoreplays_an_uncommitted_input_but_explicit_retry_uses_same_id() {
    let (_temp, root) = root();
    let path = saved(&root);
    product_studio::test_stage_daily(&root, &path, add("Explicit retry"), false);
    let mut s = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut s);
    assert!(ProductStore::open(&path)
        .unwrap()
        .load()
        .unwrap()
        .data
        .records
        .is_empty());
    s.test_reconcile();
    settle(&mut s);
    let snapshot = ProductStore::open(path).unwrap().load().unwrap();
    assert_eq!(snapshot.data.records.len(), 1);
    assert!(snapshot.operations.contains_key("interrupted-input"));
    s.test_reconcile();
    settle(&mut s);
    assert_eq!(s.test_runtime().unwrap().retained_records.len(), 1);
}

#[test]
fn initialization_cannot_be_cancelled_into_an_unusable_session() {
    let (_temp, root) = root();
    let mut s = ProductStudio::testing(root, None, TestHooks::default());
    assert!(!s.test_cancel());
    settle(&mut s);
    assert!(!s.test_generation_blocked());
    assert_eq!(s.test_page(), "home");
}
#[test]
fn rollover_is_persisted_before_today_dependent_work_and_survives_restart() {
    use serde_json::json;
    let (_temp, root) = root();
    let path = root.join("dated-tool");
    let mut source = organizer();
    source["entities"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"entered","label":"Entered","value_type":{"kind":"date"}}));
    source["actions"][0]["steps"][0]["values"]["entered"] = json!({"kind":"today"});
    ProductStore::create(&path, &capture(source), 20000).unwrap();
    let hooks = TestHooks::default();
    let clock = hooks.today.clone();
    let stopped = hooks.stopped.clone();
    let mut s = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut s);
    open(&mut s, &path);
    clock.store(20003, Ordering::Release);
    settle(&mut s);
    s.test_daily(add("Later work"));
    settle(&mut s);
    let snapshot = ProductStore::open(&path).unwrap().load().unwrap();
    assert_eq!(snapshot.clock_day, 20003);
    assert_eq!(
        snapshot.data.records[0].values["entered"],
        DataValue::Date { days: 20003 }
    );
    stop(s, &stopped);
    let hooks = TestHooks::default();
    hooks.today.store(20004, Ordering::Release);
    let mut s = ProductStudio::testing(root, None, hooks);
    settle(&mut s);
    assert_eq!(
        ProductStore::open(path).unwrap().load().unwrap().clock_day,
        20004
    );
    assert_eq!(s.test_runtime().unwrap().retained_records.len(), 1);
}
#[test]
fn unactivated_creation_can_be_explicitly_set_aside_without_deleting_partial_files() {
    for partial in [false, true] {
        let (_temp, root) = root();
        let path = root.join("unactivated");
        if partial {
            fs::create_dir(&path).unwrap();
            fs::write(path.join("orphan"), b"preserve exact partial bytes").unwrap();
        }
        product_studio::test_stage_unstarted_creation(&root, &path, &capture(organizer()));
        let mut s = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut s);
        assert!(s.test_generation_blocked());
        s.test_abandon();
        settle(&mut s);
        assert!(!s.test_generation_blocked(), "{}", s.test_notice());
        if partial {
            assert_eq!(
                fs::read(path.join("orphan")).unwrap(),
                b"preserve exact partial bytes"
            );
        } else {
            assert!(!path.exists());
        }
    }
}
#[test]
fn corrupt_activation_cannot_be_abandoned_as_an_uncommitted_creation() {
    let (_temp, root) = root();
    let path = saved(&root);
    fs::write(path.join("CURRENT"), b"corrupt activation").unwrap();
    product_studio::test_stage_unstarted_creation(&root, &path, &capture(organizer()));
    let mut s = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut s);
    s.test_abandon();
    settle(&mut s);
    assert!(s.test_generation_blocked());
    assert_eq!(
        fs::read(path.join("CURRENT")).unwrap(),
        b"corrupt activation"
    );
}

#[test]
fn conclusively_failed_recovery_keeps_input_without_permanently_blocking_work() {
    let (_temp, root) = root();
    let path = root.join("unique-tool");
    let mut source = organizer();
    source["entities"][0]["unique"] = serde_json::json!([["name"]]);
    let store = ProductStore::create(&path, &capture(source), 20000).unwrap();
    store
        .apply(0, "first", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let duplicate = add("Ada");
    product_studio::test_stage_daily(&root, &path, duplicate.clone(), false);
    let mut s = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut s);
    s.test_reconcile();
    settle(&mut s);
    assert!(!s.test_generation_blocked(), "{}", s.test_notice());
    assert!(s.test_notice().contains("did not commit"));
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    assert!(journal["pending"].is_null());
    assert_eq!(
        journal["last_unsaved"]["input"],
        serde_json::to_value(&duplicate).unwrap()
    );
    s.test_daily(add("Noor"));
    settle(&mut s);
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        2
    );
}
#[test]
fn stale_uncommitted_input_can_be_explicitly_set_aside_while_preserving_current_work() {
    let (_temp, root) = root();
    let path = saved(&root);
    let input = add("Uncommitted older input");
    product_studio::test_stage_daily(&root, &path, input.clone(), false);
    ProductStore::open(&path)
        .unwrap()
        .apply(
            0,
            "external-new-work",
            &add("Later real work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let mut s = ProductStudio::testing(root.clone(), None, TestHooks::default());
    settle(&mut s);
    s.test_reconcile();
    settle(&mut s);
    assert!(s.test_generation_blocked());
    s.test_abandon_daily();
    settle(&mut s);
    assert!(!s.test_generation_blocked());
    s.test_daily(add("Continued"));
    settle(&mut s);
    let snapshot = ProductStore::open(path).unwrap().load().unwrap();
    assert_eq!(snapshot.data.records.len(), 2);
    assert!(snapshot
        .data
        .records
        .iter()
        .all(|r| r.values["name"] != string("Uncommitted older input")));
    let journal: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    assert_eq!(
        journal["last_unsaved"]["input"],
        serde_json::to_value(input).unwrap()
    );
}

#[test]
fn cancelled_unsaved_input_recovery_preserves_the_pending_association() {
    let (_temp, root) = root();
    let path = saved(&root);
    product_studio::test_stage_daily(&root, &path, add("Pending entry"), false);
    let pending: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    let pause = Arc::new(TestPause::default());
    let hooks = TestHooks {
        before_abandon: Some(pause.clone()),
        ..TestHooks::default()
    };
    let mut s = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut s);
    s.test_abandon_daily();
    until(|| pause.reached.load(Ordering::Acquire));
    assert!(s.test_cancel());
    settle(&mut s);
    assert_eq!(s.test_page(), "daily");
    assert!(s.test_generation_blocked());
    let after: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("studio/session.json")).unwrap()).unwrap();
    assert_eq!(after["pending"], pending["pending"]);
    assert!(ProductStore::open(path)
        .unwrap()
        .load()
        .unwrap()
        .data
        .records
        .is_empty());
}

// Build the exact legacy wire shape from production-created state, including
// real operation receipts, rather than inventing a second legacy interpreter.
fn legacy_copy(
    path: &std::path::Path,
) -> (product_store::ProjectSnapshot, Vec<u8>, PathBuf, Vec<u8>) {
    let snapshot = ProductStore::open(path).unwrap().load().unwrap();
    assert!(snapshot.scope.layers.is_empty());
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    wire.as_object_mut().unwrap().remove("scope");
    wire["version"] = serde_json::json!(1);
    let bytes = canonical_bytes(&wire).unwrap();
    let digest = canonical_digest(IdentityDomain::Data, &wire).unwrap();
    let object = path.join(format!("object-{}.json", digest.as_str()));
    fs::write(&object, &bytes).unwrap();
    let pointer = canonical_bytes(&serde_json::json!({
        "magic":"gitmanager.generated-project", "version":1,
        "project_id":snapshot.data.project_id, "revision":snapshot.revision,
        "object":digest
    }))
    .unwrap();
    fs::write(path.join("CURRENT"), &pointer).unwrap();
    (snapshot, pointer, object, bytes)
}

#[test]
fn format_one_open_requires_confirmation_then_reopens_with_records_receipts_and_checkpoint() {
    let (_temp, root) = root();
    let path = saved(&root);
    ProductStore::open(&path)
        .unwrap()
        .apply(
            0,
            "old-work",
            &add("Earlier work"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let (before, pointer, object, bytes) = legacy_copy(&path);
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut studio);
    studio.test_open(path.clone());
    settle(&mut studio);
    assert_eq!(studio.test_page(), "upgrade", "{}", studio.test_notice());
    assert!(studio.test_runtime().is_none());
    studio.test_daily(add("Must not migrate as a side effect"));
    settle(&mut studio);
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    studio.test_upgrade();
    studio.test_upgrade(); // Repeated click cannot submit a second upgrade.
    settle(&mut studio);
    assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
    let upgraded = ProductStore::open(&path).unwrap().load().unwrap();
    assert_eq!(upgraded.version, 2);
    assert_eq!(upgraded.data, before.data);
    assert_eq!(upgraded.operations, before.operations);
    assert_eq!(upgraded.session, before.session);
    assert_eq!(upgraded.artifacts, before.artifacts);
    assert_eq!(upgraded.programs, before.programs);
    assert_eq!(fs::read(object).unwrap(), bytes);
    let recent = product_locations::RecentTools::open(&root)
        .unwrap()
        .list()
        .unwrap();
    let entry = recent.iter().find(|e| e.tool.path == path).unwrap();
    let locations = product_locations::ToolLocations::chosen(&root).unwrap();
    let shelf =
        product_backup::CheckpointShelf::for_tool(&locations, &entry.tool.identity, &entry.tool.id)
            .unwrap();
    assert_eq!(
        shelf
            .newest_verified()
            .unwrap()
            .recovery
            .unwrap()
            .snapshot(),
        &upgraded
    );
    assert_eq!(product_runtime::RUNTIME_VERSION, "local-interpreter/3");
    assert_eq!(product_runtime::DRIVER_VERSION, "semantic-input/3");
    stop(studio, &stopped);
    let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "daily");
    restarted.test_daily(add("Later work"));
    settle(&mut restarted);
    assert_eq!(
        ProductStore::open(path)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        2
    );
}

#[test]
fn upgrade_precommit_cancel_keeps_legacy_bytes_and_reprompts_after_restart() {
    let (_temp, root) = root();
    let path = saved(&root);
    let (_, pointer, _, _) = legacy_copy(&path);
    let pause = Arc::new(TestPause::default());
    let hooks = TestHooks {
        before_commit: Some(pause.clone()),
        ..TestHooks::default()
    };
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut studio);
    studio.test_open(path.clone());
    settle(&mut studio);
    studio.test_upgrade();
    until(|| pause.reached.load(Ordering::Acquire));
    assert!(studio.test_cancel());
    studio.test_close();
    pause.release.store(true, Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    stop(studio, &stopped);
    let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "upgrade");
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
    restarted.test_upgrade();
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "daily");
}

#[test]
fn committed_upgrade_lost_ack_and_close_reconcile_without_repeating_creation() {
    let (_temp, root) = root();
    let path = saved(&root);
    let (before, _, object, bytes) = legacy_copy(&path);
    let pause = Arc::new(TestPause::default());
    let hooks = TestHooks {
        after_commit: Some(pause.clone()),
        lose_ack: Arc::new(AtomicBool::new(true)),
        ..TestHooks::default()
    };
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut studio);
    studio.test_open(path.clone());
    settle(&mut studio);
    studio.test_upgrade();
    until(|| pause.reached.load(Ordering::Acquire));
    assert!(!studio.test_cancel());
    studio.test_close();
    pause.release.store(true, Ordering::Release);
    settle(&mut studio);
    assert_eq!(studio.test_page(), "home");
    let current = ProductStore::open(&path).unwrap().load().unwrap();
    assert_eq!(current.version, 2);
    assert_eq!(current.data, before.data);
    assert_eq!(fs::read(object).unwrap(), bytes);
    let pointer = fs::read(path.join("CURRENT")).unwrap();
    stop(studio, &stopped);
    let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "daily");
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
}

#[test]
fn interrupted_upgrade_uses_only_verified_activation_and_never_auto_migrates() {
    use product_store::FaultPoint;
    for fault in [
        FaultPoint::AfterUpgradeObject,
        FaultPoint::AfterUpgradeJournal,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let (_temp, root) = root();
        let path = saved(&root);
        ProductStore::open(&path)
            .unwrap()
            .apply(0, "old-work", &add("Kept"), RuntimeLimits::default())
            .unwrap();
        product_studio::test_stage_creation(&root, &path);
        let (before, _, object, bytes) = legacy_copy(&path);
        assert!(ProductStore::open(&path)
            .unwrap()
            .upgrade_with_fault(|_| {}, fault)
            .is_err());
        let pointer = fs::read(path.join("CURRENT")).unwrap();
        let version = serde_json::from_slice::<serde_json::Value>(&pointer).unwrap()["version"]
            .as_u64()
            .unwrap();
        let mut studio = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut studio);
        assert_eq!(fs::read(path.join("CURRENT")).unwrap(), pointer);
        if version == 1 {
            assert_eq!(studio.test_page(), "upgrade", "{}", studio.test_notice());
            studio.test_upgrade();
            settle(&mut studio);
        }
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        assert_eq!(
            ProductStore::open(&path).unwrap().load().unwrap().data,
            before.data
        );
        assert_eq!(fs::read(object).unwrap(), bytes);
        studio.test_daily(add("Continued"));
        settle(&mut studio);
        assert_eq!(
            ProductStore::open(&path)
                .unwrap()
                .load()
                .unwrap()
                .data
                .records
                .len(),
            2
        );
    }
}

#[test]
fn legacy_pending_input_keeps_exact_receipt_or_unsaved_input_across_upgrade() {
    for committed in [false, true] {
        let (_temp, root) = root();
        let path = saved(&root);
        product_studio::test_stage_daily(&root, &path, add("Interrupted work"), committed);
        let (_, _, _, _) = legacy_copy(&path);
        let session_path = root.join("studio/session.json");
        let mut session: serde_json::Value =
            serde_json::from_slice(&fs::read(&session_path).unwrap()).unwrap();
        session["pending"]["basis"]["runtime"] = serde_json::json!("local-interpreter/2");
        session["pending"]["basis"]["driver"] = serde_json::json!("semantic-input/2");
        fs::write(&session_path, canonical_bytes(&session).unwrap()).unwrap();
        let input = session["pending"]["input"].clone();
        let mut studio = ProductStudio::testing(root, None, TestHooks::default());
        settle(&mut studio);
        assert_eq!(studio.test_page(), "upgrade", "{}", studio.test_notice());
        studio.test_upgrade();
        settle(&mut studio);
        assert_eq!(studio.test_page(), "daily", "{}", studio.test_notice());
        let current = ProductStore::open(&path).unwrap().load().unwrap();
        assert_eq!(current.data.records.len(), usize::from(committed));
        if !committed {
            studio.test_reconcile();
            settle(&mut studio);
            assert!(studio.test_notice().contains("cannot be repeated safely"));
            studio.test_abandon_daily();
            settle(&mut studio);
            let session: serde_json::Value =
                serde_json::from_slice(&fs::read(&session_path).unwrap()).unwrap();
            assert_eq!(session["last_unsaved"]["input"], input);
        }
        studio.test_daily(add("New work"));
        settle(&mut studio);
        assert_eq!(
            ProductStore::open(path)
                .unwrap()
                .load()
                .unwrap()
                .data
                .records
                .len(),
            usize::from(committed) + 1
        );
    }
}

#[path = "fixtures/product_scope/mod.rs"]
mod scope_fixture;
#[test]
fn daily_store_view_preserves_completed_history_after_reopen_and_further_work() {
    use scope_fixture as s;
    let (_temp, root) = root();
    let path = root.join("scoped");
    let store = ProductStore::create(&path, &s::program(false), 20000).unwrap();
    let row = s::add(&store, "old", "Completed result");
    s::action(&store, "complete", "complete", &row);
    let before = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &s::program(true),
            &s::request(&before, product_store::scope::ScopePopulation::FutureWork),
            "scope",
        )
        .unwrap();
    store.adopt_scoped(before.revision, &prepared).unwrap();
    let expected = store.runtime_view().unwrap();
    assert!(!expected.history.results.is_empty());
    let hooks = TestHooks::default();
    let stopped = hooks.stopped.clone();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks);
    settle(&mut studio);
    open(&mut studio, &path);
    assert_eq!(studio.test_runtime().unwrap().history, expected.history);
    studio.test_daily(s::invoke(
        "add",
        &[
            ("name", s::text("Later work")),
            ("promised", DataValue::Date { days: 20020 }),
        ],
    ));
    settle(&mut studio);
    assert_eq!(
        studio.test_runtime().unwrap().history,
        store.runtime_view().unwrap().history
    );
    assert_eq!(
        studio.test_runtime().unwrap().history.results,
        expected.history.results
    );
    stop(studio, &stopped);
    let mut restarted = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut restarted);
    assert_eq!(restarted.test_page(), "daily");
    assert_eq!(
        restarted.test_runtime().unwrap().history,
        store.runtime_view().unwrap().history
    );
}
