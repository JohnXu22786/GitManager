#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_alloc.rs"]
mod product_alloc;
#[path = "../src/product_json.rs"]
mod product_json;
#[path = "../src/product_app.rs"]
mod product_app;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_store/mod.rs"]
mod product_store;
use fixture::*;
use product_contract::*;
use product_runtime::*;
use product_store::{scope::*, ProductStore};
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::num::NonZeroUsize;

fn start(p: &CapturedProgram) -> ProductRun {
    LocalRuntime::default()
        .start(
            p,
            &seed(p),
            &SessionState::initial(&p.program).unwrap(),
            20000,
            42,
            RuntimeLimits::default(),
        )
        .unwrap()
}

#[test]
fn map_binding_uses_managed_env_and_propagates_each_growth_refusal() {
    let mut definition = organizer();
    definition["observables"][0]["value"] = json!({
        "kind": "map",
        "items": query("person"),
        "binding": "row",
        "value": {
            "kind": "map",
            "items": {
                "kind": "literal",
                "value_type": {"kind": "list", "item": {"kind": "integer"}},
                "value": {"kind": "list", "item_type": {"kind": "integer"}, "items": [
                    {"kind": "integer", "value": 2},
                    {"kind": "integer", "value": 3}
                ]}
            },
            "binding": "n",
            "value": field(var("row"), "name")
        },
    });
    let program = capture(definition);
    let mut run = start(&program);
    apply(&mut run, add("Ada"), "add-map-binding");

    let runtime = LocalRuntime::default();
    let before_data = runtime.data(&run).clone();
    let before_session = runtime.session(&run).clone();
    let (observed, growths) = runtime.observe_with_allocations(
        &run,
        "managed-map",
        product_alloc::AllocationContext::new(),
    );
    assert_eq!(observed.unwrap().values["selected_count"], DataValue::List {
        item_type: Type::list(Type::Text),
        items: vec![DataValue::List {
            item_type: Type::Text,
            items: vec![string("Ada"), string("Ada")],
        }],
    });
    assert!(growths > 0);

    for ordinal in 1..=growths {
        let context = product_alloc::AllocationContext::fail_nth_growth(
            NonZeroUsize::new(ordinal).unwrap(),
            false,
        );
        let (result, observed_growths) = runtime.observe_with_allocations(
            &run,
            "managed-map",
            context,
        );
        assert_eq!(observed_growths, ordinal);
        assert_eq!(
            result.unwrap_err(),
            AdapterError::Failed("expression binding allocation failed".into()),
            "allocation refusal at growth {ordinal}",
        );
        assert_eq!(
            runtime.observe(&run, "managed-map").unwrap().values["selected_count"],
            DataValue::List {
                item_type: Type::list(Type::Text),
                items: vec![DataValue::List {
                    item_type: Type::Text,
                    items: vec![string("Ada"), string("Ada")],
                }],
            },
            "runtime remains usable after refusal at growth {ordinal}",
        );
        assert_eq!(runtime.data(&run), &before_data);
        assert_eq!(runtime.session(&run), &before_session);
    }

    let cancelled = Arc::new(AtomicBool::new(true));
    let cancelled_runtime = LocalRuntime::with_cancellation(cancelled);
    let (cancelled_result, cancelled_growths) = cancelled_runtime.observe_with_allocations(
        &run,
        "managed-map",
        product_alloc::AllocationContext::fail_nth_growth(
            NonZeroUsize::new(1).unwrap(),
            false,
        ),
    );
    assert_eq!(cancelled_result.unwrap_err(), AdapterError::Cancelled);
    assert_eq!(cancelled_growths, 0, "cancellation precedes Shared allocation");
}
fn apply(r: &mut ProductRun, input: SemanticInput, id: &str) {
    LocalRuntime::default().apply(r, &input, id).unwrap();
}

struct DecodedFixture {
    program: FallibleDecodedCapturedProgram,
    data: FallibleDecodedDataSnapshot,
    session: FallibleDecodedSessionState,
}
impl DecodedFixture {
    fn new(source: serde_json::Value, generation: Option<u64>) -> Self {
        let capture = fixture::capture(source);
        let mut data = seed(&capture);
        if let Some(generation) = generation { data.generation = generation; }
        let session = SessionState::initial(&capture.program).unwrap();
        let mut allocations = product_alloc::AllocationContext::new();
        let program = decode_captured_program_fallible(
            product_json::parse_json_bytes(&serde_json::to_vec(&capture).unwrap(), &mut allocations).unwrap(),
            &mut allocations,
        ).unwrap();
        let data = decode_data_snapshot_fallible(
            product_json::parse_json_bytes(&serde_json::to_vec(&data).unwrap(), &mut allocations).unwrap(),
            &mut allocations,
        ).unwrap();
        let session = decode_session_state_fallible(
            product_json::parse_json_bytes(&serde_json::to_vec(&session).unwrap(), &mut allocations).unwrap(),
            &mut allocations,
        ).unwrap();
        Self { program, data, session }
    }
    fn start<'a>(&'a self, runtime: &LocalRuntime) -> DecodedProductRun<'a> {
        runtime.start_decoded(&self.program, &self.data, &self.session, 20_000, 42,
            RuntimeLimits::default(), &mut product_alloc::AllocationContext::new()).unwrap()
    }
}
fn decoded_ready_run<'a>(owners: &'a DecodedFixture, runtime: &LocalRuntime) -> DecodedProductRun<'a> {
    let mut run = owners.start(runtime);
    let mut allocations = product_alloc::AllocationContext::new();
    runtime.apply_decoded(&mut run, &add("Ada"), "decoded-add", &mut allocations).unwrap();
    runtime.apply_decoded(&mut run, &invoke("collect", Values::new()), "decoded-collect", &mut allocations).unwrap();
    run
}

#[test]
fn decoded_product_run_invoke_uses_shared_kernel_and_commits_event_outputs() {
    let capture = fixture::capture(organizer());
    let mut allocations = product_alloc::AllocationContext::new();
    let decoded_program = decode_captured_program_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&capture).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let data = seed(&capture);
    let decoded_data = decode_data_snapshot_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&data).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let session = SessionState::initial(&capture.program).unwrap();
    let decoded_session = decode_session_state_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&session).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let runtime = LocalRuntime::default();
    let initial_identity = decoded_data.identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let mut run = runtime.start_decoded(
        &decoded_program, &decoded_data, &decoded_session, 20_000, 42,
        RuntimeLimits::default(), &mut allocations,
    ).unwrap();

    let step = runtime.apply_decoded(&mut run, &add("Ada"), "decoded-add", &mut allocations).unwrap();
    assert_eq!(step.outcome, StepOutcome::Applied);
    assert_eq!(step.before_data, initial_identity, "the first action must retain the caller's original data identity");
    assert_eq!(run.data().generation(), 1);
    assert_eq!(run.data().records().len(), 1);
    assert_eq!(run.data().records()[0].entity(), "person");
    assert_eq!(run.data().events().len(), 1);
    assert_eq!(run.data().events()[0].operation_id(), "decoded-add");

    runtime.apply_decoded(&mut run, &invoke("collect", Values::new()), "decoded-collect", &mut allocations).unwrap();
    let exported = runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export", &mut allocations).unwrap();
    assert_eq!(exported.outcome, StepOutcome::Applied);
    assert_eq!(run.artifacts().len(), 1);
    assert_eq!(run.artifacts()[0].bytes(), b"name\r\nAda\r\n");
    assert_eq!(run.data().events().last().unwrap().outputs(), &[run.artifacts()[0].digest().clone()]);
    runtime.apply_decoded(&mut run, &SemanticInput::Observe { point: "selected_count".into() },
        "decoded-observe-after-output", &mut allocations).unwrap();
    assert_eq!(run.observations().last().unwrap().outputs().len(), 1);
    assert_eq!(run.observations().last().unwrap().outputs()[0].bytes(), b"name\r\nAda\r\n");
    let prior_generation = run.data().generation();
    let replay_refusal = runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export",
        &mut product_alloc::AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), false));
    assert!(matches!(replay_refusal, Err(AdapterError::Allocation(_))));
    assert_eq!(run.data().generation(), prior_generation);
    let replay = runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export", &mut allocations).unwrap();
    assert_eq!(replay.index, exported.index);
    assert_eq!(run.data().generation(), prior_generation, "receipt replay must not append a second event");
    let stale = runtime.apply_decoded(&mut run, &add("Grace"), "decoded-export", &mut allocations).unwrap_err();
    assert!(matches!(stale, AdapterError::Stale(_)));
}

#[test]
fn decoded_product_run_start_rejects_incompatible_data_and_preserves_original_snapshot() {
    let capture = fixture::capture(organizer());
    let mut data = seed(&capture);
    data.schema[0].fields.push(FieldDefinition {
        id: "retired_required".into(),
        label: "Retired required field".into(),
        value_type: Type::Text,
    });
    let mut allocations = product_alloc::AllocationContext::new();
    let program = decode_captured_program_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&capture).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let data = decode_data_snapshot_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&data).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let session = SessionState::initial(&capture.program).unwrap();
    let session = decode_session_state_fallible(
        product_json::parse_json_bytes(&serde_json::to_vec(&session).unwrap(), &mut allocations).unwrap(),
        &mut allocations,
    ).unwrap();
    let error = LocalRuntime::default().start_decoded(
        &program, &data, &session, 20_000, 42, RuntimeLimits::default(), &mut allocations,
    ).err().unwrap();
    assert!(matches!(error, AdapterError::Unsupported(_)));
}

#[test]
fn decoded_product_run_observe_and_refusal_do_not_publish_action_history() {
    let owners = DecodedFixture::new(organizer(), None);
    let runtime = LocalRuntime::default();
    let mut run = owners.start(&runtime);
    let before_data = run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let before_session = run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let observe = runtime.apply_decoded(&mut run, &SemanticInput::Observe { point: "selected_count".into() },
        "decoded-observe", &mut product_alloc::AllocationContext::new()).unwrap();
    assert_eq!(observe.outcome, StepOutcome::Applied);
    assert_eq!(run.observations().len(), 1);
    assert_eq!(run.data().generation(), 0);
    assert!(run.data().events().is_empty());
    assert_eq!(run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_data);
    assert_eq!(run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_session);

    let mut rejected = organizer();
    rejected["actions"][0]["guards"] = json!([{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}]);
    let rejected_owners = DecodedFixture::new(rejected, None);
    let mut rejected_run = rejected_owners.start(&runtime);
    let error = runtime.apply_decoded(&mut rejected_run, &add("Ada"), "decoded-guard-reject",
        &mut product_alloc::AllocationContext::new()).unwrap_err();
    assert!(matches!(error, AdapterError::Invalid(_)));
    assert_eq!(rejected_run.trace().last().unwrap().outcome, StepOutcome::Rejected);
    assert_eq!(rejected_run.data().generation(), 0);
    assert!(rejected_run.data().events().is_empty());

    let mut ensure_rejected = organizer();
    ensure_rejected["actions"][0]["ensures"] = json!([{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}]);
    let ensure_owners = DecodedFixture::new(ensure_rejected, None);
    let mut ensure_run = ensure_owners.start(&runtime);
    let error = runtime.apply_decoded(&mut ensure_run, &add("Ada"), "decoded-ensure-reject",
        &mut product_alloc::AllocationContext::new()).unwrap_err();
    assert!(matches!(error, AdapterError::Invalid(_)));
    assert_eq!(ensure_run.trace().last().unwrap().outcome, StepOutcome::Rejected);
    assert_eq!(ensure_run.data().generation(), 0);
    assert!(ensure_run.data().records().is_empty());
    assert!(ensure_run.data().events().is_empty());
}

#[test]
fn decoded_product_run_rejects_unsupported_input_without_publication() {
    let owners = DecodedFixture::new(organizer(), None);
    let runtime = LocalRuntime::default();
    let mut run = owners.start(&runtime);
    let before_data = run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let before_session = run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let before_trace = run.trace().len();
    let error = runtime.apply_decoded(
        &mut run,
        &SemanticInput::Navigate { view: "people".into() },
        "decoded-unsupported-navigate",
        &mut product_alloc::AllocationContext::new(),
    ).unwrap_err();
    assert!(matches!(error, AdapterError::Unsupported(_)));
    assert_eq!(run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_data);
    assert_eq!(run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_session);
    assert_eq!(run.trace().len(), before_trace);
    assert!(run.data().events().is_empty());
    assert!(run.observations().is_empty());
}

#[test]
fn decoded_product_run_finalization_allocation_refusal_preserves_owners_and_retry_commits() {
    let runtime = LocalRuntime::default();
    let owners = DecodedFixture::new(organizer(), None);
    let mut probe = decoded_ready_run(&owners, &runtime);
    let mut probe_allocations = product_alloc::AllocationContext::new();
    runtime.apply_decoded(&mut probe, &invoke("export_people", Values::new()), "decoded-export",
        &mut probe_allocations).unwrap();
    let growths = probe_allocations.growth_requests();
    assert!(growths > 0);

    for ordinal in 1..=growths {
        let mut run = decoded_ready_run(&owners, &runtime);
        let before_data = run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
        let before_session = run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
        let before_trace = run.trace().len();
        let mut refused_allocations = product_alloc::AllocationContext::fail_nth_growth(
            NonZeroUsize::new(ordinal).unwrap(), false,
        );
        let refusal = runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export",
            &mut refused_allocations);
        assert!(matches!(refusal, Err(AdapterError::Allocation(_))), "growth {ordinal}");
        assert_eq!(run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_data);
        assert_eq!(run.session().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before_session);
        assert_eq!(run.trace().len(), before_trace);
        assert!(run.artifacts().is_empty());

        runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export",
            &mut product_alloc::AllocationContext::new()).unwrap();
        assert_eq!(run.data().generation(), 3);
        assert_eq!(run.data().events().last().unwrap().operation_id(), "decoded-export");
        assert_eq!(run.artifacts().len(), 1);
    }
}

#[test]
fn decoded_product_run_output_byte_limit_is_cumulative_across_invokes() {
    let owners = DecodedFixture::new(organizer(), None);
    let runtime = LocalRuntime::default();
    let mut limits = RuntimeLimits::default();
    limits.output_bytes = b"name\r\nAda\r\n".len();
    let mut allocations = product_alloc::AllocationContext::new();
    let mut run = runtime.start_decoded(&owners.program, &owners.data, &owners.session,
        20_000, 42, limits, &mut allocations).unwrap();
    runtime.apply_decoded(&mut run, &add("Ada"), "decoded-add", &mut allocations).unwrap();
    runtime.apply_decoded(&mut run, &invoke("collect", Values::new()), "decoded-collect", &mut allocations).unwrap();
    runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export-1", &mut allocations).unwrap();
    let generation = run.data().generation();
    let event_count = run.data().events().len();
    let refusal = runtime.apply_decoded(&mut run, &invoke("export_people", Values::new()), "decoded-export-2", &mut allocations);
    assert!(matches!(refusal, Err(AdapterError::BudgetExhausted(message)) if message == "cumulative output limit"));
    assert_eq!(run.artifacts().len(), 1);
    assert_eq!(run.data().generation(), generation);
    assert_eq!(run.data().events().len(), event_count);
}

#[test]
fn decoded_product_run_generation_overflow_and_cancellation_publish_no_history() {
    let owners = DecodedFixture::new(organizer(), Some(u64::MAX));
    let runtime = LocalRuntime::default();
    let mut run = owners.start(&runtime);
    let before = run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap();
    let overflow = runtime.apply_decoded(&mut run, &add("Ada"), "decoded-overflow",
        &mut product_alloc::AllocationContext::new()).unwrap_err();
    assert!(matches!(overflow, AdapterError::Failed(message) if message == "data generation overflow"));
    assert_eq!(run.data().identity_with(&mut product_alloc::AllocationContext::new()).unwrap(), before);
    assert!(run.data().events().is_empty());

    let normal = DecodedFixture::new(organizer(), None);
    let mut cancelled_run = normal.start(&runtime);
    let cancelled = LocalRuntime::with_cancellation(Arc::new(AtomicBool::new(true)));
    let cancelled_error = cancelled.apply_decoded(&mut cancelled_run, &add("Ada"), "decoded-cancel",
        &mut product_alloc::AllocationContext::new()).unwrap_err();
    assert_eq!(cancelled_error, AdapterError::Cancelled);
    assert_eq!(cancelled_run.data().generation(), 0);
    assert!(cancelled_run.data().events().is_empty());
    runtime.apply_decoded(&mut cancelled_run, &add("Ada"), "decoded-cancel",
        &mut product_alloc::AllocationContext::new()).unwrap();
    assert_eq!(cancelled_run.data().generation(), 1);
}

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn witnesses(store: &ProductStore) -> Vec<LifecycleWitness> {
    let snapshot = store.load().unwrap();
    let row = |done: bool| {
        let record = snapshot.data.records.iter().find(|record| {
            record.entity == "job"
                && (record.values.get("done") == Some(&DataValue::Boolean { value: true }))
                    == done
        }).unwrap();
        RecordRef { entity: record.entity.clone(), record: record.id.clone() }
    };
    [OwnedSourceSide::Before, OwnedSourceSide::After]
        .into_iter()
        .map(|side| LifecycleWitness {
            side,
            scenario: ScenarioSpec {
                version: 1,
                id: "test".into(),
                label: "test".into(),
                seed: snapshot.data.clone(),
                session: snapshot.session.clone(),
                clock_day: snapshot.clock_day,
                random_seed: 1,
                inputs: vec![SemanticInput::Observe { point: "test".into() }],
                validity: vec![],
            },
            completed: row(true),
            unfinished: row(false),
        })
        .collect()
}

#[test]
fn filtered_selection_exports_actual_rows_and_independent_counts() {
    let p = capture(filtered());
    let mut r = start(&p);
    apply(&mut r, add("Zoe"), "add-z");
    apply(&mut r, add("Ada"), "add-a");
    let before = LocalRuntime::default().observe(&r, "before").unwrap();
    assert_eq!(
        before
            .view
            .rows
            .iter()
            .map(|r| &r.cells["name"])
            .collect::<Vec<_>>(),
        vec![&string("Ada"), &string("Zoe")]
    );
    apply(
        &mut r,
        SemanticInput::Control {
            view: "people".into(),
            control: "search_input".into(),
            value: string("Ada"),
        },
        "filter",
    );
    apply(&mut r, invoke("collect", Values::new()), "collect");
    apply(
        &mut r,
        SemanticInput::Control {
            view: "people".into(),
            control: "search_input".into(),
            value: string("Zoe"),
        },
        "filter-z",
    );
    apply(
        &mut r,
        SemanticInput::Activate {
            view: "people".into(),
            binding: "export_button".into(),
            row: None,
        },
        "export",
    );
    let observation = LocalRuntime::default().observe(&r, "done").unwrap();
    assert_eq!(observation.view.rows[0].cells["name"], string("Zoe"));
    assert_eq!(
        observation.outputs[0].rows,
        vec![args(&[("name", string("Ada"))])]
    );
    assert_eq!(observation.outputs[0].bytes, b"name\r\nAda\r\n");
    assert_eq!(
        observation.values["selected_count"],
        DataValue::Integer { value: 1 }
    );
    let property = AcceptedProperty {
        id: "count-matches".into(),
        description: "Preview equals actual output".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "selected_count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::OutputCount {
                point: "done".into(),
                output: "roster".into(),
            },
        },
    };
    assert_eq!(property.evaluate(&[observation]), Some(true));
    assert_eq!(LocalRuntime::default().data(&r).records.len(), 2);
}

#[test]
fn unicode_lowercase_runs_through_the_real_observation_evaluator() {
    let mut source = organizer();
    source["views"][0]["kind"]["columns"][0]["value"] = json!({
        "kind": "lower",
        "value": {"kind": "field", "record": {"kind": "variable", "name": "row"}, "field": "name"}
    });
    let program = capture(source);
    let runtime = LocalRuntime::default();
    let mut run = start(&program);
    apply(&mut run, add("ΟΣ İ"), "add-unicode");
    let observation = runtime.observe(&run, "unicode-lowercase").unwrap();
    assert_eq!(observation.view.rows[0].cells["name"], string("ος i\u{307}"));
}

#[test]
fn related_loans_guard_duplicate_borrow_and_return_atomically() {
    let p = capture(equipment());
    let data = equipment_seed(&p);
    let runtime = LocalRuntime::default();
    let mut r = runtime
        .start(
            &p,
            &data,
            &SessionState::initial(&p.program).unwrap(),
            20000,
            42,
            RuntimeLimits::default(),
        )
        .unwrap();
    let borrow = invoke("borrow", args(&[("asset", reference("asset", "drill"))]));
    apply(&mut r, borrow.clone(), "borrow-1");
    let before = runtime.data(&r).clone();
    assert!(runtime.apply(&mut r, &borrow, "borrow-2").is_err());
    assert_eq!(runtime.data(&r), &before);
    let loan = before
        .records
        .iter()
        .find(|r| r.entity == "loan")
        .unwrap()
        .id
        .clone();
    apply(
        &mut r,
        invoke("return_loan", args(&[("loan", reference("loan", &loan))])),
        "return",
    );
    apply(&mut r, borrow, "borrow-3");
    assert_eq!(runtime.data(&r).events.len(), 3);
    assert_eq!(
        runtime
            .data(&r)
            .records
            .iter()
            .filter(|r| r.entity == "loan")
            .count(),
        2
    );
    assert_eq!(
        runtime.data(&r).events[0].changes[0].after["returned"],
        DataValue::Boolean { value: false }
    );
}

#[test]
fn composed_persistent_collection_keeps_members_after_session_reset() {
    let p = capture(new_composition());
    let mut r = start(&p);
    apply(&mut r, add("Ada"), "add");
    apply(&mut r, invoke("collect", Values::new()), "collect");
    apply(
        &mut r,
        invoke("save_collection", args(&[("name", string("Trip"))])),
        "save",
    );
    let data = LocalRuntime::default().data(&r);
    let collection = data
        .records
        .iter()
        .find(|r| r.entity == "collection")
        .unwrap();
    assert!(matches!(&collection.values["members"],DataValue::List {items,..} if items.len()==1));
    let reopened = LocalRuntime::default()
        .start(
            &p,
            data,
            &SessionState::initial(&p.program).unwrap(),
            20001,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(LocalRuntime::default().data(&reopened), data);
    assert_eq!(
        LocalRuntime::default()
            .observe(&reopened, "reopened")
            .unwrap()
            .values["selected_count"],
        DataValue::Integer { value: 0 }
    );
}

#[test]
fn invalid_postconditions_roll_back_records_state_and_outputs() {
    let mut source = organizer();
    source["actions"][0]["steps"].as_array_mut().unwrap().extend(vec![json!({"kind":"set_state","state":"search","value":text("changed")}),json!({"kind":"emit","output":"roster","items":query("person"),"binding":"row","columns":{"name":field(var("row"),"name")}})]);
    source["actions"][0]["ensures"] = json!([{"kind":"not","value":yes()}]);
    let p = capture(source);
    let mut r = start(&p);
    let before = LocalRuntime::default().data(&r).clone();
    assert!(LocalRuntime::default()
        .apply(&mut r, &add("Ada"), "bad")
        .is_err());
    assert_eq!(LocalRuntime::default().data(&r), &before);
    let o = LocalRuntime::default().observe(&r, "after").unwrap();
    assert_eq!(o.view.controls["search_input"], string(""));
    assert!(o.outputs.is_empty());
    assert_eq!(r.trace().last().unwrap().outcome, StepOutcome::Rejected);
}

#[test]
fn dangling_parameters_and_hidden_row_activations_are_rejected() {
    let p = capture(equipment());
    let mut r = start(&p);
    assert!(LocalRuntime::default()
        .apply(
            &mut r,
            &invoke("borrow", args(&[("asset", reference("asset", "missing"))])),
            "missing"
        )
        .is_err());
    assert!(LocalRuntime::default()
        .apply(
            &mut r,
            &SemanticInput::Activate {
                view: "assets".into(),
                binding: "borrow_asset".into(),
                row: Some(RecordRef {
                    entity: "asset".into(),
                    record: "missing".into()
                })
            },
            "hidden"
        )
        .is_err());
    assert!(LocalRuntime::default().data(&r).events.is_empty());
}

#[test]
fn replay_is_deterministic_and_duplicate_operations_are_not_reexecuted() {
    let p = capture(organizer());
    let mut a = start(&p);
    let mut b = start(&p);
    apply(&mut a, add("Ada"), "same");
    apply(&mut b, add("Ada"), "same");
    let first = LocalRuntime::default().data(&a).clone();
    apply(&mut a, add("Ada"), "same");
    assert_eq!(LocalRuntime::default().data(&a), &first);
    assert!(LocalRuntime::default()
        .apply(&mut a, &add("Changed"), "same")
        .is_err());
    assert_eq!(
        LocalRuntime::default().data(&a),
        LocalRuntime::default().data(&b)
    );
}

#[test]
fn cancellation_and_nested_fuel_exhaustion_are_truthful_partial_runs() {
    let p = capture(organizer());
    let s = scenario(
        &p,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "after".into(),
            },
        ],
    );
    let cancelled = Arc::new(AtomicBool::new(true));
    let runtime = LocalRuntime::with_cancellation(cancelled.clone());
    let e = runtime
        .replay(&p, &s, &decisions(), RuntimeLimits::default(), "cancelled")
        .unwrap();
    assert_eq!(e.state, EvidenceState::Inconclusive);
    assert!(e.observations.is_empty());
    e.validate().unwrap();
    cancelled.store(false, Ordering::SeqCst);
    let mut limits = RuntimeLimits::default();
    limits.fuel = 1;
    let e = runtime
        .replay(&p, &s, &decisions(), limits, "exhausted")
        .unwrap();
    assert_eq!(e.state, EvidenceState::Inconclusive);
    assert!(e.trace.iter().all(|s| s.before_data == s.after_data));
    e.validate().unwrap();
}

#[test]
fn completed_evidence_contains_only_real_observation_points_and_fresh_binding() {
    let p = capture(organizer());
    let s = scenario(
        &p,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "after".into(),
            },
        ],
    );
    let e = LocalRuntime::default()
        .replay(&p, &s, &decisions(), RuntimeLimits::default(), "run")
        .unwrap();
    assert_eq!(e.state, EvidenceState::Observed);
    assert_eq!(e.origin, ExecutionOrigin::ProductionRuntime);
    assert_eq!(e.trace.len(), 2);
    assert_eq!(e.observations.len(), 1);
    e.validate().unwrap();
    let mut binding = e.binding.clone();
    binding.runtime_version.push_str("-changed");
    assert!(!e.is_current(&binding));
    let no_observe = scenario(&p, vec![add("Ada")]);
    assert_eq!(
        LocalRuntime::default()
            .replay(
                &p,
                &no_observe,
                &decisions(),
                RuntimeLimits::default(),
                "unobserved"
            )
            .unwrap()
            .state,
        EvidenceState::Inconclusive
    );
}

#[test]
fn collection_write_and_output_limits_roll_back_whole_inputs() {
    let p = capture(organizer());
    let mut limits = RuntimeLimits::default();
    limits.collection_items = 1;
    let runtime = LocalRuntime::default();
    let mut r = runtime
        .start(
            &p,
            &seed(&p),
            &SessionState::initial(&p.program).unwrap(),
            20000,
            42,
            limits,
        )
        .unwrap();
    apply(&mut r, add("Ada"), "one");
    let before = runtime.data(&r).clone();
    assert!(matches!(
        runtime.apply(&mut r, &add("Zoe"), "two"),
        Err(AdapterError::BudgetExhausted(_))
    ));
    assert_eq!(runtime.data(&r), &before);
    let mut limits = RuntimeLimits::default();
    limits.output_bytes = 5;
    let mut r = runtime
        .start(
            &p,
            &seed(&p),
            &SessionState::initial(&p.program).unwrap(),
            20000,
            42,
            limits,
        )
        .unwrap();
    apply(&mut r, add("Ada"), "one");
    apply(&mut r, invoke("collect", Values::new()), "collect");
    let before = runtime.data(&r).clone();
    assert!(matches!(
        runtime.apply(&mut r, &invoke("export_people", Values::new()), "export"),
        Err(AdapterError::BudgetExhausted(_))
    ));
    assert_eq!(runtime.data(&r), &before);
}

#[test]
fn compatibility_retains_optional_fields_but_refuses_lossy_types() {
    let original = capture(organizer());
    let mut new = organizer();
    new["entities"][0]["fields"].as_array_mut().unwrap().push(
        json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}),
    );
    let added = capture(new);
    let mut r = start(&added);
    apply(&mut r, add("Ada"), "add");
    let mut data = LocalRuntime::default().data(&r).clone();
    data.records[0]
        .values
        .insert("note".into(), string("Later fact"));
    let report = LocalRuntime::default()
        .compatibility(&original, &data)
        .unwrap();
    assert_eq!(report.state, CompatibilityState::Compatible);
    assert!(report.retained_fields.iter().any(|f| f.id == "note"));
    let reopened = LocalRuntime::default()
        .start(
            &original,
            &data,
            &SessionState::initial(&original.program).unwrap(),
            20000,
            0,
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(
        LocalRuntime::default()
            .view_model(&reopened)
            .unwrap()
            .retained_records[0]
            .values["note"],
        string("Later fact")
    );
    let mut changed = organizer();
    changed["entities"][0]["fields"][0]["value_type"] = json!({"kind":"integer"});
    changed["actions"][0]["parameters"]["name"] = json!({"kind":"integer"});
    changed["outputs"][0]["columns"][0]["value_type"] = json!({"kind":"integer"});
    assert_eq!(
        LocalRuntime::default()
            .compatibility(&capture(changed), &data)
            .unwrap()
            .state,
        CompatibilityState::Incompatible
    );
}

#[test]
fn control_handlers_and_loop_write_budgets_roll_back_together() {
    let mut source = organizer();
    source["actions"].as_array_mut().unwrap().push(json!({"id":"handle_search","label":"Search handler","parameters":{"value":{"kind":"text"}},"guards":[],"steps":[{"kind":"for_each","items":query("person"),"binding":"person","steps":[{"kind":"update","record":var("person"),"values":{"area":var("value")}}]},{"kind":"assert","condition":{"kind":"not","value":yes()},"message":"Rejected handler"}],"ensures":[]}));
    source["views"][0]["kind"]["controls"][0]["on_change"] = json!("handle_search");
    let p = capture(source.clone());
    let mut r = start(&p);
    apply(&mut r, add("Ada"), "add");
    let data = LocalRuntime::default().data(&r).clone();
    assert!(LocalRuntime::default()
        .apply(
            &mut r,
            &SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("changed")
            },
            "control"
        )
        .is_err());
    assert_eq!(LocalRuntime::default().data(&r), &data);
    assert_eq!(
        LocalRuntime::default().session(&r).values["search"],
        string("")
    );
    source["actions"][3]["steps"].as_array_mut().unwrap().pop();
    let p = capture(source);
    let mut limits = RuntimeLimits::default();
    limits.transaction_writes = 1;
    let runtime = LocalRuntime::default();
    let mut r = runtime
        .start(
            &p,
            &data,
            &SessionState::initial(&p.program).unwrap(),
            20000,
            0,
            limits,
        )
        .unwrap();
    assert!(matches!(
        runtime.apply(
            &mut r,
            &SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("changed")
            },
            "control"
        ),
        Err(AdapterError::BudgetExhausted(_))
    ));
    assert_eq!(runtime.data(&r), &data);
}

#[test]
fn optional_empty_maps_numeric_overflow_and_clock_are_typed() {
    let mut source = organizer();
    source["observables"] = json!([
        {"id":"lengths","label":"Mapped empty values","value":{"kind":"map","items":query("person"),"binding":"person","value":field(var("person"),"name")}},
        {"id":"today","label":"Today","value":{"kind":"today"}},
        {"id":"shifted","label":"Shifted day","value":{"kind":"date_add","date":{"kind":"today"},"days":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":3}}}},
        {"id":"optional","label":"Optional","value":{"kind":"literal","value_type":{"kind":"optional","item":{"kind":"text"}},"value":{"kind":"null"}}}
    ]);
    let p = capture(source);
    let mut r = start(&p);
    apply(&mut r, SemanticInput::AdvanceClock { days: 2 }, "clock");
    let o = LocalRuntime::default().observe(&r, "date").unwrap();
    assert_eq!(o.values["today"], DataValue::Date { days: 20002 });
    assert_eq!(o.values["shifted"], DataValue::Date { days: 20005 });
    assert_eq!(
        o.values["lengths"],
        DataValue::List {
            item_type: Type::Text,
            items: vec![]
        }
    );
    assert_eq!(
        o.value_types["optional"],
        Type::Optional {
            item: Box::new(Type::Text)
        }
    );
    let mut source = organizer();
    source["actions"][0]["steps"].as_array_mut().unwrap().push(json!({"kind":"assert","condition":{"kind":"less","left":{"kind":"add","left":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":i64::MAX}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}},"message":"Check arithmetic"}));
    let p = capture(source);
    let mut r = start(&p);
    assert!(matches!(
        LocalRuntime::default().apply(&mut r, &add("Ada"), "overflow"),
        Err(AdapterError::Failed(_))
    ));
    assert!(LocalRuntime::default().data(&r).records.is_empty());
}

#[test]
fn uniqueness_constraints_and_archival_preserve_historical_facts() {
    let mut source = organizer();
    source["entities"][0]["unique"] = json!([["name"]]);
    source["entities"][0]["constraints"] = json!([{"kind":"not","value":{"kind":"equal","left":field(var("record"),"name"),"right":text("")}}]);
    source["actions"].as_array_mut().unwrap().push(json!({"id":"archive_person","label":"Archive","parameters":{"person":{"kind":"reference","entity":"person"}},"guards":[],"steps":[{"kind":"archive","record":var("person")}],"ensures":[]}));
    let p = capture(source);
    let mut r = start(&p);
    let runtime = LocalRuntime::default();
    assert!(runtime.apply(&mut r, &add(""), "empty").is_err());
    apply(&mut r, add("Ada"), "first");
    let first = runtime.data(&r).clone();
    assert!(runtime.apply(&mut r, &add("Ada"), "duplicate").is_err());
    assert_eq!(runtime.data(&r), &first);
    apply(
        &mut r,
        invoke(
            "archive_person",
            args(&[("person", reference("person", &first.records[0].id))]),
        ),
        "archive",
    );
    apply(&mut r, add("Ada"), "second");
    assert_eq!(runtime.data(&r).records.len(), 2);
    assert!(runtime.data(&r).records[0].archived);
    assert_eq!(runtime.data(&r).events[0], first.events[0]);
    assert_eq!(runtime.observe(&r, "visible").unwrap().view.rows.len(), 1);
}

#[test]
fn elapsed_budget_and_mid_transaction_cancellation_leave_no_effects() {
    let mut source = organizer();
    source["actions"].as_array_mut().unwrap().push(json!({"id":"expensive","label":"Evaluate relationships","parameters":{},"guards":[],"steps":[{"kind":"set_state","state":"search","value":text("tentative")},{"kind":"assert","condition":{"kind":"all","items":query("person"),"binding":"outer","predicate":{"kind":"all","items":query("person"),"binding":"inner","predicate":{"kind":"equal","left":field(var("inner"),"area"),"right":field(var("outer"),"area")}}},"message":"Areas agree"}],"ensures":[]}));
    source["actions"][3]["steps"][1]["condition"]["items"]["limit"] = json!(1000);
    source["actions"][3]["steps"][1]["condition"]["predicate"]["items"]["limit"] = json!(1000);
    let p = capture(source);
    let mut data = seed(&p);
    for i in 0..600 {
        data.records.push(Record {
            entity: "person".into(),
            id: format!("person-{i}"),
            revision: 1,
            created_program: p.artifact.program_digest.clone(),
            archived: false,
            values: args(&[("name", string("Ada")), ("area", string("north"))]),
        });
    }
    let session = SessionState::initial(&p.program).unwrap();
    let mut limits = RuntimeLimits::default();
    limits.fuel = 10_000_000;
    limits.elapsed_millis = 1;
    let runtime = LocalRuntime::default();
    let mut r = runtime
        .start(&p, &data, &session, 20000, 0, limits.clone())
        .unwrap();
    assert!(matches!(
        runtime.apply(&mut r, &invoke("expensive", Values::new()), "time"),
        Err(AdapterError::BudgetExhausted(_))
    ));
    assert_eq!(runtime.data(&r), &data);
    assert_eq!(runtime.session(&r), &session);
    limits.elapsed_millis = 60_000;
    let token = Arc::new(AtomicBool::new(false));
    let runtime = LocalRuntime::with_cancellation(token.clone());
    let mut r = runtime
        .start(&p, &data, &session, 20000, 0, limits)
        .unwrap();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2));
        token.store(true, Ordering::SeqCst);
    });
    let result = runtime.apply(&mut r, &invoke("expensive", Values::new()), "cancel");
    cancel.join().unwrap();
    assert_eq!(result, Err(AdapterError::Cancelled));
    assert_eq!(runtime.data(&r), &data);
    assert_eq!(runtime.session(&r), &session);
}

#[test]
fn evaluated_literal_references_cannot_fabricate_observed_records() {
    for value in [
        json!({"kind":"reference","entity":"person","record":"missing"}),
        json!({"kind":"list","item_type":{"kind":"reference","entity":"person"},"items":[{"kind":"reference","entity":"person","record":"missing"}]}),
    ] {
        let mut source = organizer();
        let value_type = if value["kind"] == "list" {
            json!({"kind":"list","item":{"kind":"reference","entity":"person"}})
        } else {
            json!({"kind":"reference","entity":"person"})
        };
        source["observables"] = json!([{"id":"phantom","label":"Phantom","value":{"kind":"literal","value_type":value_type,"value":value}}]);
        let p = capture(source);
        let scene = scenario(
            &p,
            vec![SemanticInput::Observe {
                point: "after".into(),
            }],
        );
        let result = LocalRuntime::default()
            .replay(&p, &scene, &decisions(), RuntimeLimits::default(), "run")
            .unwrap();
        assert_eq!(result.state, EvidenceState::Failed);
        assert!(result.observations.is_empty());
        result.validate().unwrap();
    }
}

#[test]
fn oversized_debug_diagnostics_preserve_the_executed_failure_prefix() {
    for message in [
        "x".repeat(MAX_TEXT_BYTES),
        format!("x{}", "\n".repeat(MAX_TEXT_BYTES - 1)),
        "界".repeat(MAX_TEXT_BYTES / 3),
    ] {
        let mut source = organizer();
        source["actions"].as_array_mut().unwrap().push(json!({"id":"reject","label":"Reject","parameters":{},"guards":[],"steps":[{"kind":"assert","condition":{"kind":"not","value":yes()},"message":message}],"ensures":[]}));
        let p = capture(source);
        let scene = scenario(
            &p,
            vec![
                add("Ada"),
                invoke("reject", Values::new()),
                SemanticInput::Observe {
                    point: "never".into(),
                },
            ],
        );
        let evidence = LocalRuntime::default()
            .replay(&p, &scene, &decisions(), RuntimeLimits::default(), "run")
            .unwrap();
        assert_eq!(evidence.state, EvidenceState::Failed);
        assert_eq!(evidence.trace.len(), 2);
        assert_eq!(evidence.trace[0].outcome, StepOutcome::Applied);
        assert_eq!(evidence.trace[1].outcome, StepOutcome::Rejected);
        assert_eq!(evidence.trace[1].before_data, evidence.trace[1].after_data);
        assert!(evidence.observations.is_empty());
        assert!(evidence.errors[0].len() <= MAX_TEXT_BYTES);
        assert!(evidence.trace[1].diagnostic.as_ref().unwrap().len() <= MAX_TEXT_BYTES);
        evidence.validate().unwrap();
    }
}

#[test]
fn query_failures_follow_stable_ids_before_predicates_execute() {
    let mut source = organizer();
    let mut rows = query("person");
    rows["predicate"] = json!({"kind":"if","condition":{"kind":"equal","left":field(var("item"),"name"),"right":text("a")},"then_value":{"kind":"less","left":{"kind":"add","left":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":i64::MAX}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}},"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":0}}},"else_value":{"kind":"text_contains","text":text(&"x".repeat(MAX_TEXT_BYTES)),"search":text("x")}});
    source["observables"] =
        json!([{"id":"count","label":"Count","value":{"kind":"count","items":rows}}]);
    let p = capture(source);
    for ids in [["z", "a"], ["a", "z"]] {
        let mut scene = scenario(
            &p,
            vec![SemanticInput::Observe {
                point: "after".into(),
            }],
        );
        for id in ids {
            scene.seed.records.push(Record {
                entity: "person".into(),
                id: id.into(),
                revision: 1,
                created_program: p.artifact.program_digest.clone(),
                archived: false,
                values: args(&[("name", string(id)), ("area", string("north"))]),
            });
        }
        let mut limits = RuntimeLimits::default();
        limits.fuel = 200;
        let evidence = LocalRuntime::default()
            .replay(&p, &scene, &decisions(), limits, "run")
            .unwrap();
        assert_eq!(evidence.state, EvidenceState::Failed);
        assert!(evidence.errors[0].contains("integer overflow"));
        evidence.validate().unwrap();
    }
}
