#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
use fixture::*;
use product_contract::*;
use product_runtime::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

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
fn apply(r: &mut ProductRun, input: SemanticInput, id: &str) {
    LocalRuntime::default().apply(r, &input, id).unwrap();
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
