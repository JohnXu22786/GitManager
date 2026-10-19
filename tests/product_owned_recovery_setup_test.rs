//! Fresh synthetic acceptance. No native GUI/operator evidence is claimed.
#[path = "fixtures/common_fact/mod.rs"] mod common;
#[path = "fixtures/owned_recovery/mod.rs"] mod fixture;
#[path = "../src/product_backup.rs"] mod product_backup;
#[path = "../src/product_contract.rs"] mod product_contract;
#[path = "../src/product_decisions/mod.rs"] mod product_decisions;
#[path = "../src/product_locations.rs"] mod product_locations;
#[path = "../src/product_protocol.rs"] mod product_protocol;
#[path = "../src/product_runtime/mod.rs"] mod product_runtime;
#[path = "../src/product_store/mod.rs"] mod product_store;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_store::{scope::*, ProductStore};

#[test]
fn fresh_policy_count_setup_uses_real_p4_choice_and_work() {
    let (dir, store, a, b) = setup();
    let initial = adopt(&store, &a, &b);
    let key = member_key(&initial);
    for title in ["live", "reserve", "complete-original"] { assert_member(&initial, title, &key, true); }
    assert_member(&initial, "finished", &key, true); // All retains membership; sealing protects it.
    job_action(&store, "later-begin", "begin", "live");
    apply(&store, "later-withdraw-material", material_input(&store, "withdraw", "genuine later memory"));
    let later = store.load().unwrap();
    assert!(flag(&later, "live"));
    assert!(!flag(&later, "reserve"));
    assert!(later.data.records.iter().any(|r| r.entity == "pause"));
    assert!(later.data.events.len() > initial.data.events.len());
    assert_eq!(later.owned_scope().unwrap().layers, initial.owned_scope().unwrap().layers);
    assert_eq!(later.owned_scope().unwrap().initializations, initial.owned_scope().unwrap().initializations);
    let reopened = product_backup::open_verified(&dir.path().join("store"), None).unwrap().store;
    assert_eq!(reopened.load().unwrap(), later);
    assert_eq!(engine(&reopened).check_current(&later).unwrap().disposition, CheckDisposition::Ready);
}
