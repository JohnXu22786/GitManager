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
    let (_temp, root) = root();
    let path = saved(&root);
    let mut studio = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut studio);
    open(&mut studio, &path);
    ProductStore::open(&path)
        .unwrap()
        .apply(
            0,
            "external",
            &SemanticInput::Navigate {
                view: "new_person".into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    studio.test_daily(add("Stale"));
    settle(&mut studio);
    assert!(ProductStore::open(&path)
        .unwrap()
        .load()
        .unwrap()
        .data
        .records
        .is_empty());
    assert!(
        studio.test_notice().contains("changed"),
        "{}",
        studio.test_notice()
    );
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
