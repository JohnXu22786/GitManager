#[path = "fixtures/product_scope/mod.rs"]
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
