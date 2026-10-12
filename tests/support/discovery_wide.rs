//! Shared fictional managed-work construction. This is the unchanged wide
//! four-slot fixture, not compact_program or estimated storage arithmetic.
use crate::change_adapter;
use crate::product_contract::*;
use crate::product_decisions::*;
use crate::product_runtime::LocalRuntime;
use crate::product_store::{scope::*, ProductStore};
use crate::scope_fixture as fixture;
use fixture::*;
use std::sync::Arc;
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
pub fn created_scene(current: &crate::product_store::ProjectSnapshot) -> (ScenarioSpec, Record) {
    let mut scene = ScenarioSpec {
        version: 1,
        id: "new-waiting-work".into(),
        label: "New work and its customer commitment".into(),
        seed: current.data.clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![invoke(
            "add",
            &[
                ("name", text("Example new work")),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        )],
        validity: vec![],
    };
    let context = ScopedExecutionContext::committed(current).unwrap();
    let runtime = LocalRuntime::default().with_admission(Arc::new(context));
    let mut run = runtime
        .start(
            current.program().unwrap(),
            &scene.seed,
            &scene.session,
            scene.clock_day,
            scene.random_seed,
            RuntimeLimits::default(),
        )
        .unwrap();
    runtime
        .apply(
            &mut run,
            &scene.inputs[0],
            &scene.replay_operation_ids().unwrap()[0],
        )
        .unwrap();
    let created = runtime
        .data(&run)
        .records
        .iter()
        .find(|r| r.values["name"] == text("Example new work"))
        .unwrap()
        .clone();
    scene.inputs.extend([
        SemanticInput::Observe {
            point: "unrelated".into(),
        },
        invoke("wait", &[("row", reference(&created))]),
        SemanticInput::AdvanceClock { days: 3 },
        invoke("calculate", &[("row", reference(&created))]),
        invoke("complete", &[("row", reference(&created))]),
        invoke("export", &[]),
        SemanticInput::Observe {
            point: "result".into(),
        },
    ]);
    (scene, created)
}
pub fn compact_program(pause: bool) -> CapturedProgram {
    // A stored production result is the sole changed semantic slot. Completion
    // preserves that result; both the view and export expose it unchanged.
    let mut raw = serde_json::to_value(program(pause).program).unwrap();
    raw["actions"][4]["steps"][0]["values"]
        .as_object_mut()
        .unwrap()
        .remove("production");
    raw["actions"][6]["steps"][0]["columns"]["production"] =
        serde_json::to_value(field("row", "production")).unwrap();
    raw["views"][0]["kind"]["columns"][1]["value"] =
        serde_json::to_value(field("row", "production")).unwrap();
    capture(raw)
}
pub fn managed(store: &ProductStore, compact: bool) -> (Record, Record, CapturedProgram) {
    let selected = add(store, "selected", "Selected commitment");
    let archived = add(store, "archived", "Completed history");
    action(store, "finish-old", "complete", &archived);
    action(store, "archive-old", "archive", &archived);
    action(store, "wait-selected", "wait", &selected);
    tick(store, "day", 20003);
    let current = store.load().unwrap();
    let authored = if compact {
        compact_program(true)
    } else {
        program(true)
    };
    let population = ScopePopulation::SelectedUnfinished {
        records: vec![RecordRef {
            entity: selected.entity.clone(),
            record: selected.id.clone(),
        }],
    };
    let scope = change_adapter::request(
        &current,
        &authored,
        population,
        fixture::request(&current, ScopePopulation::All).lifecycles,
        Default::default(),
    )
    .unwrap();
    let first = store
        .prepare_scoped_change(&authored, &scope, "selected-rule")
        .unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "selected-promise".into(),
        label: "Keep selected timing and commitment".into(),
        seed: first.seed().clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&selected))]),
            invoke("complete", &[("row", reference(&selected))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let e = engine(store);
    let accepted = e
        .accept_scoped_scene(store, &first, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "selected-promise".into(),
        request: "Keep selected timing, customer commitment and completed history".into(),
        rationale: None,
        scope: first.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: first.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = e
        .prepare_scoped_choice(store, first, choice, vec![accepted], "selected-rule")
        .unwrap();
    e.adopt(store, &change).unwrap();
    let mut raw = serde_json::to_value(authored.program).unwrap();
    let pointers: &[&str] = if compact {
        &["/actions/3/steps/0/values/production"]
    } else {
        &[
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ]
    };
    for pointer in pointers {
        let old = raw.pointer(pointer).unwrap().clone();
        *raw.pointer_mut(pointer).unwrap() =
            serde_json::json!({"kind":"add","left":old,"right":int(1)});
    }
    (selected, archived, capture(raw))
}
