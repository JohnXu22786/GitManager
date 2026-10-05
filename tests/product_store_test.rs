#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
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

fn create(path: &std::path::Path) -> ProductStore {
    ProductStore::create(path, &capture(organizer()), 20000).unwrap()
}
#[test]
fn restart_and_exact_retry_preserve_current_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let first = store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let reopened = ProductStore::open(&path).unwrap();
    assert_eq!(reopened.load().unwrap(), first);
    assert_eq!(
        reopened
            .apply(0, "add", &add("Ada"), RuntimeLimits::default())
            .unwrap(),
        first
    );
    assert!(reopened
        .apply(0, "add", &add("Changed"), RuntimeLimits::default())
        .is_err());
    assert!(reopened
        .apply(0, "another", &add("Zoe"), RuntimeLimits::default())
        .is_err());
    assert_eq!(reopened.load().unwrap(), first);
}
#[test]
fn failed_save_leaves_old_pointer_and_recovers_without_reexecution() {
    for point in [
        FaultPoint::AfterObject,
        FaultPoint::BeforePointer,
        FaultPoint::AfterPointer,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let store = create(&dir.path().join("tool"));
        let before = store.load().unwrap();
        assert!(store
            .apply_with_fault(0, "add", &add("Ada"), RuntimeLimits::default(), point)
            .is_err());
        let reopened = ProductStore::open(dir.path().join("tool")).unwrap();
        let current = reopened.load().unwrap();
        if point == FaultPoint::AfterPointer {
            assert_eq!(current.data.records.len(), 1);
        } else {
            assert_eq!(current, before);
        }
        let next = reopened
            .apply(0, "add", &add("Ada"), RuntimeLimits::default())
            .unwrap();
        assert_eq!(next.data.records.len(), 1);
        assert_eq!(next.data.events.len(), 1);
    }
}
#[test]
fn corrupted_and_future_format_stores_are_never_writable() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let current = fs::read(path.join("CURRENT")).unwrap();
    fs::write(path.join("CURRENT"), b"broken").unwrap();
    assert!(store.load().is_err());
    assert!(store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .is_err());
    assert_eq!(fs::read(path.join("CURRENT")).unwrap(), b"broken");
    let mut pointer: serde_json::Value = serde_json::from_slice(&current).unwrap();
    pointer["version"] = json!(99);
    fs::write(path.join("CURRENT"), serde_json::to_vec(&pointer).unwrap()).unwrap();
    assert!(matches!(
        store.load(),
        Err(StoreError::UnsupportedFormat(_))
    ));
}
#[test]
fn adoption_and_recovery_keep_later_records_values_and_event_facts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let first = store
        .apply(0, "first", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let mut source = organizer();
    source["label"] = json!("Changed behavior");
    source["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    source["actions"][0]["steps"][0]["values"]["note"] = json!({"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"text","value":"Later fact"}});
    let target = capture(source);
    let plan = store.prepare_switch(&target, "switch").unwrap();
    let adopted = store
        .adopt(first.revision, &plan, &target, &first.decisions)
        .unwrap();
    let later = store
        .apply(
            adopted.revision,
            "later",
            &add("Zoe"),
            RuntimeLimits::default(),
        )
        .unwrap();
    let original = capture(organizer());
    let recovery = store.prepare_switch(&original, "restore").unwrap();
    let restored = store
        .adopt(later.revision, &recovery, &original, &later.decisions)
        .unwrap();
    assert_eq!(restored.data.records, later.data.records);
    assert_eq!(restored.data.events, later.data.events);
    assert_eq!(restored.programs.len(), 2);
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), restored);
    let continued = store
        .apply(
            restored.revision,
            "continued",
            &add("Ian"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(continued.data.records.len(), 3);
    assert_eq!(
        continued.data.records[1].values["note"],
        string("Later fact")
    );
}
#[test]
fn incompatible_and_stale_adoptions_preserve_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = create(&dir.path().join("tool"));
    let mut source = organizer();
    source["label"] = json!("Candidate");
    let target = capture(source);
    let plan = store.prepare_switch(&target, "switch").unwrap();
    let later = store
        .apply(0, "later", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    assert!(store
        .adopt(later.revision, &plan, &target, &later.decisions)
        .is_err());
    assert_eq!(store.load().unwrap(), later);
    let mut source = organizer();
    source["entities"][0]["fields"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"required","label":"Required","value_type":{"kind":"text"}}));
    source["actions"][0]["steps"][0]["values"]["required"] = text("new");
    assert!(store
        .prepare_switch(&capture(source), "incompatible")
        .is_err());
    assert_eq!(store.load().unwrap(), later);
}
#[test]
fn two_writers_cannot_both_commit_the_same_generation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    create(&path);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|i| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let store = ProductStore::open(path).unwrap();
                barrier.wait();
                store.apply(
                    0,
                    &format!("writer-{i}"),
                    &add("Ada"),
                    RuntimeLimits::default(),
                )
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .filter(|h| h.thread().id() != std::thread::current().id())
            .map(|h| h.join().unwrap())
            .filter(Result::is_ok)
            .count(),
        1
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
fn generated_store_does_not_open_or_modify_legacy_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("CURRENT"), b"legacy order bytes").unwrap();
    assert!(ProductStore::create(&path, &capture(organizer()), 20000).is_err());
    assert!(ProductStore::open(&path).unwrap().load().is_err());
    assert_eq!(
        fs::read(path.join("CURRENT")).unwrap(),
        b"legacy order bytes"
    );
}
#[cfg(unix)]
#[test]
fn symlink_pointer_and_root_are_rejected_without_touching_target() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let outside = dir.path().join("outside");
    fs::write(&outside, b"private").unwrap();
    fs::remove_file(path.join("CURRENT")).unwrap();
    symlink(&outside, path.join("CURRENT")).unwrap();
    assert!(store.load().is_err());
    assert!(store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .is_err());
    assert_eq!(fs::read(&outside).unwrap(), b"private");
    symlink(&path, dir.path().join("linked")).unwrap();
    assert!(ProductStore::open(dir.path().join("linked")).is_err());
}

#[test]
fn corrupted_immutable_object_and_duplicate_pointer_keys_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let pointer = fs::read(path.join("CURRENT")).unwrap();
    let p: serde_json::Value = serde_json::from_slice(&pointer).unwrap();
    let object = path.join(format!("object-{}.json", p["object"].as_str().unwrap()));
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&object).unwrap()).unwrap();
    value["clock_day"] = json!(20001);
    fs::write(&object, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(matches!(store.load(), Err(StoreError::Corrupt(_))));
    let pointer = String::from_utf8(pointer).unwrap().replacen(
        "\"version\":1",
        "\"version\":1,\"version\":1",
        1,
    );
    fs::write(path.join("CURRENT"), pointer).unwrap();
    assert!(store.load().is_err());
}

#[cfg(unix)]
#[test]
fn replaced_store_directory_cannot_redirect_a_live_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    fs::rename(&path, dir.path().join("moved")).unwrap();
    let replacement = create(&path);
    let untouched = replacement.load().unwrap();
    assert!(store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .is_err());
    assert_eq!(replacement.load().unwrap(), untouched);
}

#[test]
fn restart_in_a_fresh_process_reads_committed_current_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "store_subprocess_entry", "--nocapture"])
        .env("PRODUCT_STORE_CHILD", &path)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(store.load().unwrap().data.records.len(), 2);
}
#[test]
fn store_subprocess_entry() {
    let Some(path) = std::env::var_os("PRODUCT_STORE_CHILD") else {
        return;
    };
    let store = ProductStore::open(path).unwrap();
    let current = store.load().unwrap();
    assert_eq!(current.data.records.len(), 1);
    let next = store
        .apply(
            current.revision,
            "child",
            &add("Zoe"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(next.data.events.len(), 2);
}

#[test]
fn exports_survive_non_emitting_inputs_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = create(&path);
    let first = store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    let selected = store
        .apply(
            first.revision,
            "collect",
            &invoke("collect", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let exported = store
        .apply(
            selected.revision,
            "export",
            &invoke("export_people", Values::new()),
            RuntimeLimits::default(),
        )
        .unwrap();
    let observed = store
        .apply(
            exported.revision,
            "observe",
            &SemanticInput::Observe {
                point: "after".into(),
            },
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(observed.artifacts, exported.artifacts);
    assert_eq!(observed.artifacts[0].bytes, b"name\r\nAda\r\n");
    let reopened = ProductStore::open(&path).unwrap();
    let next = reopened
        .apply(
            observed.revision,
            "more",
            &add("Zoe"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(next.artifacts, exported.artifacts);
    let mut limits = RuntimeLimits::default();
    limits.output_bytes = exported.artifacts[0].bytes.len();
    assert!(reopened
        .apply(
            next.revision,
            "overflow",
            &invoke("export_people", Values::new()),
            limits
        )
        .is_err());
    assert_eq!(reopened.load().unwrap(), next);
}

#[test]
fn invalid_clock_changes_cannot_publish_an_unopenable_project() {
    let mut source = organizer();
    source["entities"][0]["constraints"] = json!([{"kind":"less","left":{"kind":"today"},"right":{"kind":"literal","value_type":{"kind":"date"},"value":{"kind":"date","days":20001}}}]);
    let p = capture(source);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    let store = ProductStore::create(&path, &p, 20000).unwrap();
    let first = store
        .apply(0, "add", &add("Ada"), RuntimeLimits::default())
        .unwrap();
    assert!(store
        .apply(
            first.revision,
            "clock",
            &SemanticInput::AdvanceClock { days: 1 },
            RuntimeLimits::default()
        )
        .is_err());
    assert_eq!(ProductStore::open(&path).unwrap().load().unwrap(), first);
    let runtime = product_runtime::LocalRuntime::default();
    let mut run = runtime
        .start(
            &p,
            &first.data,
            &first.session,
            20000,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(runtime
        .apply(&mut run, &SemanticInput::AdvanceClock { days: 1 }, "clock")
        .is_err());
    assert_eq!(run.clock_day(), 20000);
}

#[test]
fn invalid_initial_runtime_is_rejected_before_creating_a_directory() {
    let mut source = organizer();
    source["state"][1]["initial"]["items"] =
        json!([{"kind":"reference","entity":"person","record":"missing"}]);
    let p = capture(source);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tool");
    assert!(ProductStore::create(&path, &p, 20000).is_err());
    assert!(!path.exists());
    create(&path).load().unwrap();
}

#[cfg(unix)]
#[test]
fn created_store_files_keep_private_usable_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tool");
    let store = create(&path);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    for entry in fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        let metadata = entry.metadata().unwrap();
        assert!(metadata.is_file());
        assert_eq!(
            metadata.permissions().mode() & 0o7777,
            0o600,
            "incorrect creation mode for {:?}",
            entry.file_name()
        );
    }
    let reopened = ProductStore::open(&path).unwrap();
    let before = reopened.load().unwrap();
    let saved = store
        .apply(
            before.revision,
            "write",
            &add("Ada"),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(reopened.load().unwrap(), saved);
    for entry in fs::read_dir(&path).unwrap() {
        let entry = entry.unwrap();
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o7777,
            0o600,
            "incorrect replacement mode for {:?}",
            entry.file_name()
        );
    }
}
