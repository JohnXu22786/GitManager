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
fn policy_context_fixed_debits_keep_real_execution_borrows() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_exact_costs_for_test();
}
#[test]
fn policy_context_constructors_refuse_before_callback_at_real_fuel_boundary() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_constructor_fuel_for_test();
}
#[test]
fn policy_context_methods_preserve_both_tokens_and_real_fuel_refusal() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_policy_context_actual_refusals_for_test();
}

#[test]
fn policy_context_locked_adoption_keeps_fresh_registry_and_exact_dispatch() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_adoption_policy_context_exact_dispatch_for_test();
}
#[test]
fn policy_context_locked_adoption_preserves_actual_fuel_and_both_tokens() {
    let (_dir, store, a, b) = setup();
    adopt(&store, &a, &b);
    store.assert_owned_adoption_policy_context_actual_refusals_for_test();
}
