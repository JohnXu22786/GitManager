//! Synthetic egui and dialog fixtures; not native OS dialogs, packaged GUI or live AI evidence.
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
#[path = "../src/product_export.rs"]
mod product_export;
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
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "../src/product_studio.rs"]
mod product_studio;
use egui_harness::EguiHarness;
use product_runtime_view::WidgetTrace;
use product_studio::{ProductStudio, TestHooks};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};

fn frame(h: &mut EguiHarness, s: &mut ProductStudio) -> WidgetTrace {
    s.poll();
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| s.show(ui))
            .inner
    })
}
fn click(h: &mut EguiHarness, s: &mut ProductStudio, key: &str) {
    let trace = frame(h, s);
    let control = trace
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing control {key}; {:?}", trace.text));
    assert!(control.enabled, "disabled {key}");
    let p = control.rect.center();
    h.press_at(p);
    frame(h, s);
    h.release_at(p);
    frame(h, s);
}
fn fill(h: &mut EguiHarness, s: &mut ProductStudio, key: &str, text: &str) {
    click(h, s, key);
    h.text(text);
    frame(h, s);
}
fn settle(h: &mut EguiHarness, s: &mut ProductStudio) {
    let start = Instant::now();
    while s.is_busy() {
        frame(h, s);
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{}",
            s.test_notice()
        );
        // The actual busy host requests a repaint every 30 ms. Match that
        // cadence instead of making the renderer compete with its worker in
        // a 2 ms redraw loop; the same 120-second completion gate still applies.
        std::thread::sleep(Duration::from_millis(30));
    }
    frame(h, s);
}

use product_contract::*;
use product_store::ProductStore;

#[test]
fn controls_export_exact_occurrence_without_rerunning_or_overwriting() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let mut source = fixture::organizer();
    let step = source["actions"][2]["steps"][0].clone();
    source["actions"][2]["steps"]
        .as_array_mut()
        .unwrap()
        .push(step);
    let store = ProductStore::create(root.join("tool"), &fixture::capture(source), 20000).unwrap();
    for (id, input) in [
        ("add", fixture::add("Ada")),
        ("collect", fixture::invoke("collect", Values::new())),
        ("output", fixture::invoke("export_people", Values::new())),
    ] {
        store
            .apply(
                store.load().unwrap().revision,
                id,
                &input,
                RuntimeLimits::default(),
            )
            .unwrap();
    }
    let before = store.load().unwrap();
    let path = root.join("selected.csv");
    let mut s = ProductStudio::testing(
        root.clone(),
        None,
        TestHooks {
            folder_choice: Some(path.clone()),
            ..Default::default()
        },
    );
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut s);
    s.test_open(root.join("tool"));
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "files");
    click(&mut h, &mut s, "studio.export.1");
    settle(&mut h, &mut s);
    assert_eq!(fs::read(&path).unwrap(), before.artifacts[1].bytes);
    assert!(
        s.test_notice().contains("exact original output"),
        "{}",
        s.test_notice()
    );
    assert!(s.test_notice().contains("output 2"), "{}", s.test_notice());
    assert_eq!(store.load().unwrap(), before);
    click(&mut h, &mut s, "studio.files-back");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.export.1");
    settle(&mut h, &mut s);
    assert_eq!(fs::read(&path).unwrap(), before.artifacts[1].bytes);
    assert!(!s.test_notice().contains("exact original output"));
    assert_eq!(store.load().unwrap(), before);
}

fn fixture_root() -> (tempfile::TempDir, std::path::PathBuf, ProductStore) {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let store = ProductStore::create(
        root.join("tool"),
        &fixture::capture(fixture::organizer()),
        20000,
    )
    .unwrap();
    for (id, input) in [
        ("add", fixture::add("Ada")),
        ("collect", fixture::invoke("collect", Values::new())),
        ("output", fixture::invoke("export_people", Values::new())),
    ] {
        store
            .apply(
                store.load().unwrap().revision,
                id,
                &input,
                RuntimeLimits::default(),
            )
            .unwrap();
    }
    (temp, root, store)
}
fn opened(root: &Path, hooks: TestHooks) -> (ProductStudio, EguiHarness) {
    let mut s = ProductStudio::testing(root.into(), None, hooks);
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut s);
    s.test_open(root.join("tool"));
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "daily", "{}", s.test_notice());
    (s, h)
}
fn pause_until(pause: &product_studio::TestPause) {
    let start = Instant::now();
    while !pause.reached.load(std::sync::atomic::Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn stopped(s: ProductStudio, flag: &std::sync::atomic::AtomicBool) {
    drop(s);
    let start = Instant::now();
    while !flag.load(std::sync::atomic::Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn dialog_outcomes_are_distinct_and_do_not_replace_saved_work() {
    use product_studio::TestFileDialog;
    for (outcome, message) in [
        (TestFileDialog::Cancelled, "cancelled"),
        (TestFileDialog::Unavailable, "unavailable"),
        (
            TestFileDialog::Error("fixture failure".into()),
            "fixture failure",
        ),
        (TestFileDialog::Indeterminate, "could not confirm"),
    ] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let (mut s, mut h) = opened(
            &root,
            TestHooks {
                file_dialog: Some(outcome),
                ..Default::default()
            },
        );
        click(&mut h, &mut s, "studio.files");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.export.0");
        settle(&mut h, &mut s);
        assert!(s.test_notice().contains(message), "{}", s.test_notice());
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(s.test_page(), "files");
    }
}
#[test]
fn backup_import_requires_explicit_fresh_target_and_separate_open() {
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let backup = root.join("chosen.gmbak");
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            folder_choice: Some(backup.clone()),
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.backup");
    settle(&mut h, &mut s);
    assert_eq!(
        product_backup::VerifiedBackup::from_bytes(&fs::read(&backup).unwrap())
            .unwrap()
            .snapshot(),
        &before
    );
    click(&mut h, &mut s, "studio.files-back");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.import");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "import");
    let target = s.test_recovery_target().unwrap().to_path_buf();
    assert!(!target.exists());
    assert_ne!(target, root.join("tool"));
    click(&mut h, &mut s, "studio.recover");
    settle(&mut h, &mut s);
    assert_eq!(store.load().unwrap(), before);
    assert_eq!(ProductStore::open(&target).unwrap().load().unwrap(), before);
    assert_eq!(s.test_active_tool(), Some(root.join("tool").as_path()));
    click(&mut h, &mut s, "studio.open-recovered");
    settle(&mut h, &mut s);
    assert_eq!(s.test_location(), Some(target.as_path()));
    s.test_daily(fixture::add("Later"));
    settle(&mut h, &mut s);
    assert_eq!(
        ProductStore::open(&target)
            .unwrap()
            .load()
            .unwrap()
            .data
            .records
            .len(),
        2
    );
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn stale_inventory_and_cancelled_write_preserve_files_and_work() {
    use std::sync::{atomic::Ordering, Arc};
    for cancel in [false, true] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let path = root.join("chosen.csv");
        let pause = Arc::new(product_studio::TestPause::default());
        let (mut s, mut h) = opened(
            &root,
            TestHooks {
                before_file_effect: Some(pause.clone()),
                folder_choice: Some(path.clone()),
                ..Default::default()
            },
        );
        click(&mut h, &mut s, "studio.files");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.export.0");
        pause_until(&pause);
        if cancel {
            assert!(s.test_cancel());
        } else {
            store
                .apply(
                    before.revision,
                    "later",
                    &fixture::add("Grace"),
                    RuntimeLimits::default(),
                )
                .unwrap();
        }
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        assert!(!path.exists());
        assert!(!s.test_notice().contains("exact original output"));
        assert_eq!(
            store.load().unwrap().data.records.len(),
            if cancel { 1 } else { 2 }
        );
    }
}
#[test]
fn files_navigation_respects_pending_input_and_duplicate_clicks() {
    use std::sync::{atomic::Ordering, Arc};
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    let path = root.join("once.csv");
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            before_file_effect: Some(pause.clone()),
            folder_choice: Some(path.clone()),
            duplicate_completion: true,
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "daily.navigate.new_person");
    settle(&mut h, &mut s);
    let before = store.load().unwrap();
    fill(&mut h, &mut s, "daily.field.name", "unfinished");
    assert!(!frame(&mut h, &mut s).controls["studio.files"].enabled);
    s.test_files();
    assert!(!s.is_busy());
    assert_eq!(s.test_page(), "daily");
    // Explicit close discards unsent input, then reopen the same committed work.
    s.test_close();
    settle(&mut h, &mut s);
    s.test_open(root.join("tool"));
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.export.0");
    pause_until(&pause);
    let operation = s.test_pending_operation().unwrap().to_string();
    s.test_export(0);
    assert_eq!(s.test_pending_operation(), Some(operation.as_str()));
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut s);
    assert_eq!(fs::read(path).unwrap(), before.artifacts[0].bytes);
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn corrupt_current_is_diagnosed_from_its_own_recent_checkpoint() {
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let (mut s, mut h) = opened(&root, TestHooks::default());
    s.test_close();
    settle(&mut h, &mut s);
    fs::write(root.join("tool/CURRENT"), b"broken current").unwrap();
    let recent = s.test_recent_ids();
    assert_eq!(recent.len(), 1);
    click(&mut h, &mut s, &format!("studio.diagnose.{}", recent[0]));
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "diagnosis");
    assert!(frame(&mut h, &mut s)
        .text
        .iter()
        .any(|x| x.contains("could not be verified")));
    click(&mut h, &mut s, "studio.review-recovery");
    settle(&mut h, &mut s);
    let target = s.test_recovery_target().unwrap().to_path_buf();
    click(&mut h, &mut s, "studio.recover");
    settle(&mut h, &mut s);
    assert_eq!(
        fs::read(root.join("tool/CURRENT")).unwrap(),
        b"broken current"
    );
    assert_eq!(ProductStore::open(target).unwrap().load().unwrap(), before);
}
#[test]
fn legacy_backup_upgrade_is_explicit_and_unknown_backup_is_rejected() {
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let path = root.join("legacy.gmbak");
    let mut value: serde_json::Value = serde_json::from_slice(
        &product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .to_bytes()
            .unwrap(),
    )
    .unwrap();
    value["payload"]["snapshot"]
        .as_object_mut()
        .unwrap()
        .remove("scope");
    value["payload"]["snapshot"]["version"] = serde_json::json!(1);
    value["payload"]["intentions"]["snapshot"] =
        serde_json::json!(
            canonical_digest(IdentityDomain::Data, &value["payload"]["snapshot"]).unwrap()
        );
    value["digest"] =
        serde_json::json!(canonical_digest(IdentityDomain::Evidence, &value["payload"]).unwrap());
    let bytes = canonical_bytes(&value).unwrap();
    fs::write(&path, &bytes).unwrap();
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            folder_choice: Some(path.clone()),
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.import");
    settle(&mut h, &mut s);
    assert!(frame(&mut h, &mut s)
        .text
        .iter()
        .any(|x| x.contains("older backup") && x.contains("upgrade")));
    let target = s.test_recovery_target().unwrap().to_path_buf();
    click(&mut h, &mut s, "studio.recover");
    settle(&mut h, &mut s);
    let restored = ProductStore::open(target).unwrap().load().unwrap();
    assert_eq!(restored.data, before.data);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    click(&mut h, &mut s, "studio.files-back");
    settle(&mut h, &mut s);
    value["version"] = serde_json::json!(999);
    fs::write(&path, canonical_bytes(&value).unwrap()).unwrap();
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.import");
    settle(&mut h, &mut s);
    assert_ne!(s.test_page(), "import");
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn interrupted_external_effect_is_inspect_only_across_restarts() {
    use std::sync::{atomic::Ordering, Arc};
    for after_effect in [false, true] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let path = root.join("one.csv");
        let stopped_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (mut s, mut h) = opened(
            &root,
            TestHooks {
                interrupt_file: Some(after_effect),
                stopped: stopped_flag.clone(),
                folder_choice: Some(path.clone()),
                ..Default::default()
            },
        );
        click(&mut h, &mut s, "studio.files");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.export.0");
        settle(&mut h, &mut s);
        assert_eq!(path.exists(), after_effect);
        assert!(!s.test_notice().contains("exact original output"));
        stopped(s, &stopped_flag);
        for pass in 0..2 {
            let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (mut s, mut h) = opened(
                &root,
                TestHooks {
                    stopped: flag.clone(),
                    ..Default::default()
                },
            );
            assert!(frame(&mut h, &mut s)
                .text
                .iter()
                .any(|x| x.contains("interrupted file") && x.contains("not repeated")));
            assert_eq!(path.exists(), after_effect);
            assert!(!s.test_notice().contains("exact original output"));
            if pass == 1 {
                s.test_daily(fixture::add("Continued"));
                settle(&mut h, &mut s);
                assert_eq!(store.load().unwrap().data.records.len(), 2);
            }
            stopped(s, &flag);
            assert!(flag.load(Ordering::Acquire));
        }
        if after_effect {
            assert_eq!(fs::read(path).unwrap(), before.artifacts[0].bytes);
        }
    }
}
#[cfg(unix)]
#[test]
fn destination_parent_swap_is_refused_without_following_links() {
    use std::sync::{atomic::Ordering, Arc};
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let folder = root.join("chosen");
    fs::create_dir(&folder).unwrap();
    fs::create_dir(root.join("elsewhere")).unwrap();
    let pause = Arc::new(product_studio::TestPause::default());
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            before_file_effect: Some(pause.clone()),
            folder_choice: Some(folder.join("out.csv")),
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.export.0");
    pause_until(&pause);
    fs::rename(&folder, root.join("kept")).unwrap();
    std::os::unix::fs::symlink(root.join("elsewhere"), &folder).unwrap();
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut s);
    assert!(!root.join("elsewhere/out.csv").exists());
    assert!(!root.join("kept/out.csv").exists());
    assert_eq!(store.load().unwrap(), before);
}

#[test]
fn close_after_external_effect_rejects_late_page_and_restart_never_exports_twice() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (_temp, root, store) = fixture_root();
    let before = store.load().unwrap();
    let path = root.join("late.csv");
    let pause = Arc::new(product_studio::TestPause::default());
    let flag = Arc::new(AtomicBool::new(false));
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            after_commit: Some(pause.clone()),
            stopped: flag.clone(),
            folder_choice: Some(path.clone()),
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.export.0");
    pause_until(&pause);
    assert!(path.exists());
    s.test_close();
    assert!(!s.test_cancel());
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "home");
    assert_eq!(fs::read(&path).unwrap(), before.artifacts[0].bytes);
    assert_eq!(store.load().unwrap(), before);
    stopped(s, &flag);
    let (s, _h) = opened(&root, TestHooks::default());
    assert_eq!(s.test_location(), Some(root.join("tool").as_path()));
    assert_eq!(store.load().unwrap(), before);
}
#[test]
fn old_host_journal_opens_without_rewrite_and_bad_file_association_fails_closed() {
    use std::sync::{atomic::AtomicBool, Arc};
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    fs::create_dir(root.join("studio")).unwrap();
    let old=br#"{"magic":"gitmanager.generated-tool-host","version":1,"need":"Remember my original need","last":null,"pending":null,"provider":null,"abandoned_creation":null,"last_unsaved":null}"#;
    let path = root.join("studio/session.json");
    fs::write(&path, old).unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    let mut s = ProductStudio::testing(
        root.clone(),
        None,
        TestHooks {
            stopped: flag.clone(),
            ..Default::default()
        },
    );
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut s);
    assert_eq!(s.test_need(), "Remember my original need");
    assert_eq!(fs::read(&path).unwrap(), old);
    stopped(s, &flag);
    let mut bad: serde_json::Value = serde_json::from_slice(old).unwrap();
    bad["local_file"] = serde_json::json!({"operation":"bad","tool":null,"basis":null,"destination":"relative","target":{"kind":"backup","digest":"0".repeat(64)}});
    let bytes = canonical_bytes(&bad).unwrap();
    fs::write(&path, &bytes).unwrap();
    let mut s = ProductStudio::testing(root, None, TestHooks::default());
    settle(&mut h, &mut s);
    assert!(s.test_generation_blocked());
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(s.test_notice().contains("Invalid interrupted"));
}

#[test]
fn corrupt_original_cannot_trap_file_report_navigation() {
    use std::sync::{atomic::Ordering, Arc};
    let (_temp, root, _store) = fixture_root();
    let pause = Arc::new(product_studio::TestPause::default());
    let (mut s, mut h) = opened(
        &root,
        TestHooks {
            folder_choice: Some(root.join("kept.csv")),
            after_commit: Some(pause.clone()),
            ..Default::default()
        },
    );
    click(&mut h, &mut s, "studio.files");
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.export.0");
    pause_until(&pause);
    fs::write(root.join("tool/CURRENT"), b"damaged after publication").unwrap();
    pause.release.store(true, Ordering::Release);
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "file-report");
    click(&mut h, &mut s, "studio.files-back");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "file-report");
    click(&mut h, &mut s, "studio.files-home");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "home");
    assert!(root.join("kept.csv").exists());
    assert_eq!(
        fs::read(root.join("tool/CURRENT")).unwrap(),
        b"damaged after publication"
    );
    let recent = s.test_recent_ids();
    click(&mut h, &mut s, &format!("studio.diagnose.{}", recent[0]));
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "diagnosis");
}

#[test]
fn original_parent_selection_survives_until_external_publication_boundary() {
    use product_studio::TestFileDialog;
    use std::collections::VecDeque;
    use std::sync::{atomic::Ordering, Arc, Mutex};
    for mode in ["output", "backup", "current", "legacy"] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let parent = root.join("destination");
        let kept = root.join("selected-parent-kept");
        fs::create_dir(&parent).unwrap();
        let source = root.join("source.gmbak");
        let mut bytes = product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .to_bytes()
            .unwrap();
        if mode == "legacy" {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value["payload"]["snapshot"]
                .as_object_mut()
                .unwrap()
                .remove("scope");
            value["payload"]["snapshot"]["version"] = serde_json::json!(1);
            value["payload"]["intentions"]["snapshot"] = serde_json::json!(canonical_digest(
                IdentityDomain::Data,
                &value["payload"]["snapshot"]
            )
            .unwrap());
            value["digest"] =
                serde_json::json!(
                    canonical_digest(IdentityDomain::Evidence, &value["payload"]).unwrap()
                );
            bytes = canonical_bytes(&value).unwrap();
        }
        fs::write(&source, &bytes).unwrap();
        let replies = if mode == "output" || mode == "backup" {
            vec![TestFileDialog::Selected(parent.join("result"))]
        } else {
            vec![
                TestFileDialog::Selected(source.clone()),
                TestFileDialog::Selected(parent.clone()),
            ]
        };
        let pause = Arc::new(product_studio::TestPause::default());
        let (mut s, mut h) = opened(
            &root,
            TestHooks {
                file_dialog_queue: Arc::new(Mutex::new(VecDeque::from(replies))),
                after_file_staged: Some(pause.clone()),
                ..Default::default()
            },
        );
        click(&mut h, &mut s, "studio.files");
        settle(&mut h, &mut s);
        if mode == "output" {
            click(&mut h, &mut s, "studio.export.0");
        } else if mode == "backup" {
            click(&mut h, &mut s, "studio.backup");
        } else {
            click(&mut h, &mut s, "studio.import");
            settle(&mut h, &mut s);
            click(&mut h, &mut s, "studio.recovery-location");
            settle(&mut h, &mut s);
            assert_eq!(
                s.test_recovery_target().unwrap().parent(),
                Some(parent.as_path())
            );
            click(&mut h, &mut s, "studio.recover");
        }
        pause_until(&pause);
        let moved = fs::rename(&parent, &kept);
        #[cfg(unix)]
        assert!(
            moved.is_ok(),
            "original descriptor must not redirect after a Unix rename"
        );
        if moved.is_ok() {
            fs::create_dir(&parent).unwrap();
        } else {
            assert!(cfg!(windows), "unexpected rename failure: {moved:?}");
            assert!(parent.is_dir());
        }
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        if moved.is_ok() {
            assert_eq!(
                fs::read_dir(&parent).unwrap().count(),
                0,
                "redirected writes for {mode}"
            );
            assert_eq!(
                fs::read_dir(&kept).unwrap().count(),
                0,
                "late selected-parent rejection for {mode}"
            );
            assert!(!s.test_notice().contains("exact original output"));
            assert!(!s
                .test_notice()
                .contains("separate recovered tool was reopened"));
        } else {
            // Windows' retained directory/ancestor guards can reject rename.
            assert!(!kept.exists());
            assert!(fs::read_dir(&parent).unwrap().count() > 0);
        }
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}

#[cfg(unix)]
#[test]
fn selected_recovery_parent_replaced_by_symlink_never_creates_a_copy() {
    use product_studio::TestFileDialog;
    use std::collections::VecDeque;
    use std::sync::{atomic::Ordering, Arc, Mutex};
    for legacy in [false, true] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let parent = root.join("chosen");
        let outside = root.join("outside");
        let kept = root.join("kept");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&outside).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(
            &product_backup::VerifiedBackup::capture(&store)
                .unwrap()
                .to_bytes()
                .unwrap(),
        )
        .unwrap();
        if legacy {
            value["payload"]["snapshot"]
                .as_object_mut()
                .unwrap()
                .remove("scope");
            value["payload"]["snapshot"]["version"] = serde_json::json!(1);
            value["payload"]["intentions"]["snapshot"] = serde_json::json!(canonical_digest(
                IdentityDomain::Data,
                &value["payload"]["snapshot"]
            )
            .unwrap());
            value["digest"] =
                serde_json::json!(
                    canonical_digest(IdentityDomain::Evidence, &value["payload"]).unwrap()
                );
        }
        let source = root.join("source.gmbak");
        let bytes = canonical_bytes(&value).unwrap();
        fs::write(&source, &bytes).unwrap();
        let pause = Arc::new(product_studio::TestPause::default());
        let (mut s, mut h) = opened(
            &root,
            TestHooks {
                file_dialog_queue: Arc::new(Mutex::new(VecDeque::from(vec![
                    TestFileDialog::Selected(source.clone()),
                    TestFileDialog::Selected(parent.clone()),
                ]))),
                after_file_staged: Some(pause.clone()),
                ..Default::default()
            },
        );
        click(&mut h, &mut s, "studio.files");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.import");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.recovery-location");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.recover");
        pause_until(&pause);
        fs::rename(&parent, &kept).unwrap();
        std::os::unix::fs::symlink(&outside, &parent).unwrap();
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        assert_eq!(fs::read_dir(&kept).unwrap().count(), 0);
        assert_eq!(store.load().unwrap(), before);
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}

#[test]
fn corrupt_original_pending_saves_survive_explicit_recovery_handoff_and_restart() {
    use std::sync::{atomic::AtomicBool, Arc};
    for mode in ["create", "daily", "change"] {
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let source = root.join("intact.gmbak");
        product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .export_new(&source)
            .unwrap();
        match mode {
            "create" => product_studio::test_stage_creation(&root, &root.join("tool")),
            "daily" => product_studio::test_stage_daily(
                &root,
                &root.join("tool"),
                fixture::add("Original unconfirmed input"),
                false,
            ),
            _ => {
                let mut value = fixture::organizer();
                value["label"] = serde_json::json!("Unconfirmed changed tool");
                let plan = store
                    .prepare_switch(&fixture::capture(value), "unconfirmed-change")
                    .unwrap();
                product_studio::test_stage_change(
                    &root,
                    &root.join("tool"),
                    &before,
                    plan.plan().clone(),
                );
            }
        }
        let journal = root.join("studio/session.json");
        let pending: serde_json::Value =
            serde_json::from_slice::<serde_json::Value>(&fs::read(&journal).unwrap()).unwrap()
                ["pending"]
                .clone();
        fs::write(root.join("tool/CURRENT"), b"corrupt original pointer").unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let mut s = ProductStudio::testing(
            root.clone(),
            None,
            TestHooks {
                folder_choice: Some(source.clone()),
                stopped: flag.clone(),
                ..Default::default()
            },
        );
        let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.import");
        settle(&mut h, &mut s);
        let target = s.test_recovery_target().unwrap().to_path_buf();
        click(&mut h, &mut s, "studio.recover");
        settle(&mut h, &mut s);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&journal).unwrap()).unwrap()
                ["pending"],
            pending,
            "recovery alone must not clear original"
        );
        assert!(frame(&mut h, &mut s)
            .text
            .iter()
            .any(|x| x.contains("original interrupted attempt")));
        click(&mut h, &mut s, "studio.open-recovered");
        settle(&mut h, &mut s);
        assert_eq!(s.test_location(), Some(target.as_path()));
        s.test_daily(fixture::add("New saved work"));
        settle(&mut h, &mut s);
        assert_eq!(
            ProductStore::open(&target)
                .unwrap()
                .load()
                .unwrap()
                .data
                .records
                .len(),
            2
        );
        stopped(s, &flag);
        for _ in 0..2 {
            let flag = Arc::new(AtomicBool::new(false));
            let mut s = ProductStudio::testing(
                root.clone(),
                None,
                TestHooks {
                    stopped: flag.clone(),
                    ..Default::default()
                },
            );
            settle(&mut h, &mut s);
            assert_eq!(s.test_location(), Some(target.as_path()));
            assert!(frame(&mut h, &mut s)
                .text
                .iter()
                .any(|x| x.contains("original interrupted attempt") && x.contains("not repeated")));
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
            assert!(value["pending"].is_null());
            assert_eq!(value["recovery_handoffs"][0]["original"], pending);
            assert_eq!(value["recovery_handoffs"].as_array().unwrap().len(), 1);
            stopped(s, &flag);
        }
        assert_eq!(
            fs::read(root.join("tool/CURRENT")).unwrap(),
            b"corrupt original pointer"
        );
    }
}

#[test]
fn recovery_handoff_refuses_changed_copy_and_preserves_original_pending() {
    let (_temp, root, store) = fixture_root();
    let source = root.join("checked.gmbak");
    product_backup::VerifiedBackup::capture(&store)
        .unwrap()
        .export_new(&source)
        .unwrap();
    product_studio::test_stage_daily(&root, &root.join("tool"), fixture::add("Keep me"), false);
    let journal = root.join("studio/session.json");
    let pending = serde_json::from_slice::<serde_json::Value>(&fs::read(&journal).unwrap())
        .unwrap()["pending"]
        .clone();
    fs::write(root.join("tool/CURRENT"), b"damaged").unwrap();
    let mut s = ProductStudio::testing(
        root.clone(),
        None,
        TestHooks {
            folder_choice: Some(source),
            ..Default::default()
        },
    );
    let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
    settle(&mut h, &mut s);
    click(&mut h, &mut s, "studio.import");
    settle(&mut h, &mut s);
    let target = s.test_recovery_target().unwrap().to_path_buf();
    click(&mut h, &mut s, "studio.recover");
    settle(&mut h, &mut s);
    let copy = ProductStore::open(&target).unwrap();
    copy.apply(
        copy.load().unwrap().revision,
        "other-work",
        &fixture::add("Other window"),
        RuntimeLimits::default(),
    )
    .unwrap();
    click(&mut h, &mut s, "studio.open-recovered");
    settle(&mut h, &mut s);
    assert_eq!(s.test_page(), "file-report");
    assert!(s.test_notice().contains("changed"));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(journal).unwrap()).unwrap();
    assert_eq!(value["pending"], pending);
    assert!(value.get("recovery_handoffs").is_none());
    assert_eq!(copy.load().unwrap().data.records.len(), 2);
}

#[test]
fn bounded_recovery_history_and_journal_size_refuse_without_dropping_attempts() {
    for near_limit in [false, true] {
        let (_temp, root, store) = fixture_root();
        let source = root.join("backup.gmbak");
        product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .export_new(&source)
            .unwrap();
        product_studio::test_stage_daily(
            &root,
            &root.join("tool"),
            fixture::add("Unconfirmed"),
            false,
        );
        product_studio::test_seed_recovery_history(
            &root,
            if near_limit { 0 } else { 16 },
            near_limit,
        );
        let journal = root.join("studio/session.json");
        let before = fs::read(&journal).unwrap();
        fs::write(root.join("tool/CURRENT"), b"damaged original").unwrap();
        let mut s = ProductStudio::testing(
            root.clone(),
            None,
            TestHooks {
                folder_choice: Some(source),
                ..Default::default()
            },
        );
        let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.import");
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "import");
        let target = s.test_recovery_target().unwrap().to_path_buf();
        click(&mut h, &mut s, "studio.recover");
        settle(&mut h, &mut s);
        if near_limit {
            assert!(!target.exists());
            assert!(s.test_notice().contains("bounded restart record"));
        } else {
            assert!(target.exists());
            click(&mut h, &mut s, "studio.open-recovered");
            settle(&mut h, &mut s);
            assert!(s.test_notice().contains("history is full"));
            assert_eq!(s.test_page(), "file-report");
        }
        assert_eq!(fs::read(&journal).unwrap(), before);
        click(&mut h, &mut s, "studio.files-home");
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "home");
        assert_eq!(
            fs::read(root.join("tool/CURRENT")).unwrap(),
            b"damaged original"
        );
    }
}

#[test]
fn failed_or_cancelled_handoff_keeps_exact_original_journal_and_copy() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    for cancel in [false, true] {
        let (_temp, root, store) = fixture_root();
        let source = root.join("backup.gmbak");
        product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .export_new(&source)
            .unwrap();
        product_studio::test_stage_daily(
            &root,
            &root.join("tool"),
            fixture::add("Original"),
            false,
        );
        fs::write(root.join("tool/CURRENT"), b"damaged original").unwrap();
        let pause = Arc::new(product_studio::TestPause::default());
        let flag = Arc::new(AtomicBool::new(false));
        let mut s = ProductStudio::testing(
            root.clone(),
            None,
            TestHooks {
                folder_choice: Some(source),
                before_recovery_handoff: Some(pause.clone()),
                stopped: flag.clone(),
                ..Default::default()
            },
        );
        let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.import");
        settle(&mut h, &mut s);
        let target = s.test_recovery_target().unwrap().to_path_buf();
        click(&mut h, &mut s, "studio.recover");
        settle(&mut h, &mut s);
        let journal = root.join("studio/session.json");
        let before = fs::read(&journal).unwrap();
        let kept = root.join("studio/original-session-kept.json");
        click(&mut h, &mut s, "studio.open-recovered");
        pause_until(&pause);
        if cancel {
            assert!(s.test_cancel());
        } else {
            fs::rename(&journal, &kept).unwrap();
            fs::create_dir(&journal).unwrap();
        }
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        assert_eq!(s.test_page(), "file-report");
        assert!(ProductStore::open(&target).unwrap().load().is_ok());
        if !cancel {
            fs::remove_dir(&journal).unwrap();
            fs::rename(&kept, &journal).unwrap();
        }
        assert_eq!(fs::read(&journal).unwrap(), before);
        stopped(s, &flag);
        let mut again = ProductStudio::testing(root.clone(), None, TestHooks::default());
        settle(&mut h, &mut again);
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&journal).unwrap()).unwrap();
        assert!(!value["pending"].is_null());
        assert!(value.get("recovery_handoffs").is_none());
        assert_eq!(
            fs::read(root.join("tool/CURRENT")).unwrap(),
            b"damaged original"
        );
    }
}

#[test]
fn recovery_handoff_keeps_pending_when_selected_parent_is_replaced_after_open() {
    use product_studio::TestFileDialog;
    use std::collections::VecDeque;
    use std::sync::{atomic::Ordering, Arc, Mutex};
    for symlink in [false, true] {
        if symlink && !cfg!(unix) {
            continue;
        }
        let (_temp, root, store) = fixture_root();
        let before = store.load().unwrap();
        let source = root.join("backup.gmbak");
        product_backup::VerifiedBackup::capture(&store)
            .unwrap()
            .export_new(&source)
            .unwrap();
        product_studio::test_stage_daily(
            &root,
            &root.join("tool"),
            fixture::add("Keep original intent"),
            false,
        );
        fs::write(root.join("tool/CURRENT"), b"damaged original").unwrap();
        let parent = root.join("selected");
        let kept = root.join("retained-original-parent");
        let elsewhere = root.join("elsewhere");
        fs::create_dir(&parent).unwrap();
        fs::create_dir(&elsewhere).unwrap();
        let pause = Arc::new(product_studio::TestPause::default());
        let mut s = ProductStudio::testing(
            root.clone(),
            None,
            TestHooks {
                file_dialog_queue: Arc::new(Mutex::new(VecDeque::from(vec![
                    TestFileDialog::Selected(source),
                    TestFileDialog::Selected(parent.clone()),
                ]))),
                before_recovery_handoff: Some(pause.clone()),
                ..Default::default()
            },
        );
        let mut h = EguiHarness::new(egui::vec2(1500.0, 2400.0));
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.import");
        settle(&mut h, &mut s);
        click(&mut h, &mut s, "studio.recovery-location");
        settle(&mut h, &mut s);
        let target = s.test_recovery_target().unwrap().to_path_buf();
        click(&mut h, &mut s, "studio.recover");
        settle(&mut h, &mut s);
        let journal = root.join("studio/session.json");
        let pending_bytes = fs::read(&journal).unwrap();
        click(&mut h, &mut s, "studio.open-recovered");
        pause_until(&pause);
        let renamed = fs::rename(&parent, &kept);
        #[cfg(unix)]
        assert!(renamed.is_ok());
        if renamed.is_ok() {
            #[cfg(unix)]
            if symlink {
                std::os::unix::fs::symlink(&elsewhere, &parent).unwrap();
            }
            if !symlink {
                fs::create_dir(&parent).unwrap();
            }
        } else {
            assert!(cfg!(windows), "{renamed:?}");
        }
        pause.release.store(true, Ordering::Release);
        settle(&mut h, &mut s);
        if renamed.is_ok() {
            assert_eq!(s.test_page(), "file-report");
            assert_eq!(fs::read(&journal).unwrap(), pending_bytes);
            assert_eq!(
                ProductStore::open(kept.join(target.file_name().unwrap()))
                    .unwrap()
                    .load()
                    .unwrap(),
                before
            );
            assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
            assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
        } else {
            assert_eq!(s.test_location(), Some(target.as_path()));
            assert!(!kept.exists());
        }
        assert_eq!(
            fs::read(root.join("tool/CURRENT")).unwrap(),
            b"damaged original"
        );
    }
}
