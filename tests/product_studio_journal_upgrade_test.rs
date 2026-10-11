//! Offline controller regressions. No live AI, native GUI, or user acceptance proof.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/harness.rs"]
mod harness;
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
#[path = "../src/product_sources/mod.rs"]
mod product_sources;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/task_delivery.rs"]
mod task_delivery;
#[path = "../src/task_verification.rs"]
mod task_verification;
#[path = "../src/tasks.rs"]
mod tasks;
#[path = "../src/tool_proposal_input.rs"]
mod tool_proposal_input;
mod ui {
    pub(crate) use super::product_runtime_view;
}
#[path = "../src/product_studio.rs"]
mod product_studio;
use product_studio::{ProductStudio, TestHooks};
use std::{
    fs,
    path::Path,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

fn settle(studio: &mut ProductStudio) {
    let start = Instant::now();
    loop {
        studio.poll();
        if !studio.is_busy() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "controller did not finish: {}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn legacy(root: &Path) -> Vec<u8> {
    // Frozen minimal v1 shape, before optional file/recovery/task fields existed.
    let bytes = br#"{ "magic":"gitmanager.generated-tool-host", "version":1,
      "need":"Keep my original need", "last":null, "pending":null,
      "provider":null, "abandoned_creation":null, "last_unsaved":null }"#
        .to_vec();
    fs::create_dir_all(root.join("studio")).unwrap();
    fs::write(root.join("studio/session.json"), &bytes).unwrap();
    bytes
}
fn stop(studio: ProductStudio, hooks: &TestHooks) {
    drop(studio);
    let start = Instant::now();
    while !hooks.stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn startup_upgrades_frozen_v1_and_reopens_current_host_session() {
    let temp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let original = legacy(&root);
    let hooks = TestHooks::default();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks.clone());
    settle(&mut studio);
    let activated = fs::read(root.join("studio/session.json")).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&activated).unwrap();
    assert_eq!(
        value["version"], 2,
        "startup must activate the current journal before opening ordinary work"
    );
    assert_eq!(value["need"], "Keep my original need");
    assert!(
        !studio.test_generation_blocked(),
        "{}",
        studio.test_notice()
    );
    assert!(
        studio.test_notice().contains("fresh host session"),
        "{}",
        studio.test_notice()
    );
    let backup = root.join("studio").join(format!(
        "session-v1-{}.json",
        product_provider::digest(&original)
    ));
    assert_eq!(fs::read(&backup).unwrap(), original);
    stop(studio, &hooks);
    let hooks = TestHooks::default();
    let mut restarted = ProductStudio::testing(root.clone(), None, hooks.clone());
    settle(&mut restarted);
    assert_eq!(
        fs::read(root.join("studio/session.json")).unwrap(),
        activated
    );
    assert_eq!(fs::read(backup).unwrap(), original);
    assert!(!restarted.test_generation_blocked());
    stop(restarted, &hooks);
}
