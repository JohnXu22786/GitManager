//! Focused fixture assembly for the private decoded OwnedV2 capture tests.
#[path = "fixtures/common_fact/mod.rs"]
mod common;
#[path = "fixtures/product_runtime/mod.rs"]
mod runtime_fixture;
#[path = "fixtures/product_scope/mod.rs"]
mod scope_fixture;

mod fixture {
    use crate::product_contract::{AppDefinition, CapturedProgram};

    pub trait CaptureInput { fn capture(self) -> CapturedProgram; }
    impl CaptureInput for AppDefinition {
        fn capture(self) -> CapturedProgram { crate::common::capture(&self) }
    }
    impl CaptureInput for serde_json::Value {
        fn capture(self) -> CapturedProgram { crate::runtime_fixture::capture(self) }
    }
    pub fn capture<T: CaptureInput>(value: T) -> CapturedProgram { value.capture() }
    pub fn organizer() -> serde_json::Value { crate::runtime_fixture::organizer() }
    pub fn program(pause: bool) -> CapturedProgram { crate::scope_fixture::program(pause) }
    pub fn query(entity: &str) -> serde_json::Value { crate::runtime_fixture::query(entity) }
    pub fn seed(program: &CapturedProgram) -> crate::product_contract::DataSnapshot { crate::runtime_fixture::seed(program) }
    pub fn decisions() -> crate::product_contract::DecisionGraph { crate::runtime_fixture::decisions() }
    pub fn add(name: &str) -> crate::product_contract::SemanticInput { crate::runtime_fixture::add(name) }
}

#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_json.rs"]
mod product_json;
#[path = "../src/product_app.rs"]
mod product_app;
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

use product_contract::*;
use product_store::{ProductStore, scope::*};
use std::sync::{Arc, atomic::AtomicBool};

fn cancel() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(false)) }
fn witnesses(store: &ProductStore) -> Vec<LifecycleWitness> {
    let snapshot = store.load().unwrap();
    let row = |done: bool| {
        let record = snapshot.data.records.iter().find(|record| {
            record.entity == "job" && (record.values.get("status") == Some(&DataValue::Text { value: "done".into() })) == done
        }).unwrap();
        RecordRef { entity: record.entity.clone(), record: record.id.clone() }
    };
    [OwnedSourceSide::Before, OwnedSourceSide::After].into_iter().map(|side| LifecycleWitness {
        side,
        scenario: ScenarioSpec {
            version: 1, id: "capture-fixture".into(), label: "Capture fixture".into(),
            seed: snapshot.data.clone(), session: snapshot.session.clone(), clock_day: snapshot.clock_day,
            random_seed: 1, inputs: vec![SemanticInput::Observe { point: "capture".into() }], validity: vec![],
        },
        completed: row(true), unfinished: row(false),
    }).collect()
}
