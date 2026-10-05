//! Hand-authored language capability fixtures, not live generation evidence.
#[path = "../../support/product_contract_fixture.rs"]
mod contract_fixture;
use crate::product_contract::*;
pub use contract_fixture::*;
use serde_json::{json, Value};

pub fn capture(value: Value) -> CapturedProgram {
    CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "runtime-project",
        Producer::Fixture {
            name: "runtime capability".into(),
        },
        None,
    )
    .unwrap()
}
pub fn string(value: &str) -> DataValue {
    DataValue::Text {
        value: value.into(),
    }
}
pub fn reference(entity: &str, record: &str) -> DataValue {
    DataValue::Reference {
        entity: entity.into(),
        record: record.into(),
    }
}
pub fn args(items: &[(&str, DataValue)]) -> Values {
    items
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect()
}
pub fn invoke(action: &str, arguments: Values) -> SemanticInput {
    SemanticInput::Invoke {
        action: action.into(),
        arguments,
    }
}
pub fn add(name: &str) -> SemanticInput {
    invoke(
        "add_person",
        args(&[("name", string(name)), ("area", string("north"))]),
    )
}
pub fn decisions() -> DecisionGraph {
    DecisionGraph {
        version: 1,
        revision: 0,
        decisions: vec![],
    }
}
pub fn seed(program: &CapturedProgram) -> DataSnapshot {
    DataSnapshot::empty("runtime-project", &program.program).unwrap()
}
pub fn equipment_seed(program: &CapturedProgram) -> DataSnapshot {
    let mut data = seed(program);
    data.records.push(Record {
        entity: "asset".into(),
        id: "drill".into(),
        revision: 1,
        created_program: program.artifact.program_digest.clone(),
        archived: false,
        values: args(&[("name", string("Drill"))]),
    });
    data
}
pub fn filtered() -> Value {
    let mut p = organizer();
    let predicate = json!({"kind":"text_contains","text":field(var("item"),"name"),"search":{"kind":"state","state":"search"}});
    p["views"][0]["kind"]["rows"]["predicate"] = predicate.clone();
    p["actions"][1]["steps"][0]["items"]["predicate"] = predicate;
    p["views"][0]["kind"]["rows"]["sort"] =
        json!([{"value":field(var("item"),"name"),"descending":false}]);
    p
}
pub fn scenario(program: &CapturedProgram, inputs: Vec<SemanticInput>) -> ScenarioSpec {
    ScenarioSpec {
        version: 1,
        id: "scene".into(),
        label: "Execution capability".into(),
        seed: seed(program),
        session: SessionState::initial(&program.program).unwrap(),
        clock_day: 20000,
        random_seed: 42,
        inputs,
        validity: vec![],
    }
}
