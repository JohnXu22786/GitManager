//! Synthetic component regressions; these are not live/held-out acceptance.
#[path = "fixtures/product_runtime/mod.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
use fixture::*;
use product_contract::*;
use product_scenarios::*;
use serde_json::json;
use std::sync::{atomic::AtomicBool, Arc};

fn contrast() -> (CapturedProgram, CapturedProgram, ScenarioSpec) {
    let a = capture(filtered());
    let mut value = filtered();
    value["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":value["actions"][1]["steps"][0]["items"].clone()});
    let b = capture(value);
    let mut s = scenario(
        &a,
        vec![
            SemanticInput::Navigate {
                view: "people".into(),
            },
            invoke("collect", Values::new()),
            SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: string("Ada"),
            },
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    for name in ["Ada", "Zoe", "Redundant"] {
        s.seed.records.push(Record {
            entity: "person".into(),
            id: name.to_lowercase(),
            revision: 1,
            created_program: a.artifact.program_digest.clone(),
            archived: false,
            values: args(&[("name", string(name)), ("area", string("north"))]),
        });
    }
    (a, b, s)
}
fn target() -> ObservationTarget {
    ObservationTarget::OutputCount {
        point: "done".into(),
        output: "roster".into(),
    }
}
fn engine() -> ComparisonEngine {
    ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
}

#[test]
fn actual_exports_use_identical_inputs_and_dynamic_reductions() {
    let (a, b, s) = contrast();
    let report = engine()
        .minimize(&a, &b, &s, &decisions(), target(), SearchBudget::default())
        .unwrap();
    assert_eq!(report.state, EvidenceState::Observed);
    let verified = report.witness.unwrap();
    let w = verified.witness();
    w.validate().unwrap();
    assert_eq!(w.before.origin, ExecutionOrigin::ProductionRuntime);
    assert_eq!(w.before.binding.input_digest, w.after.binding.input_digest);
    assert_eq!(
        w.before.binding.input_digest,
        w.scenario.input_identity().unwrap()
    );
    assert!(w.scenario.seed.records.len() < s.seed.records.len());
    assert!(w.scenario.inputs.len() < s.inputs.len());
    assert_ne!(
        w.before.observations[0].outputs[0].rows,
        w.after.observations[0].outputs[0].rows
    );
    let c = w.minimization.as_ref().unwrap();
    assert!(c.complete);
    let deletions = engine().single_reductions(&a, &b, &w.scenario).unwrap();
    assert_eq!(deletions.len(), c.final_single_deletions.len());
    for ((operation, reduced), trial) in deletions.iter().zip(&c.final_single_deletions) {
        assert_eq!(&trial.operation, operation);
        assert_eq!(trial.from, w.scenario.identity().unwrap());
        assert_eq!(trial.reduced, reduced.identity().unwrap());
        assert!(matches!(
            trial.outcome,
            ReductionOutcome::DifferenceLost | ReductionOutcome::InvalidScenario
        ));
        let rerun = engine()
            .compare(
                &a,
                &b,
                reduced,
                &decisions(),
                target(),
                RuntimeLimits::default(),
            )
            .unwrap();
        assert_ne!(rerun.state, EvidenceState::Observed);
    }
    assert_eq!(s.seed.records.len(), 3, "live seed was not mutated");
    assert!(verified.matches_sources(&a, &b));
    assert!(!verified.matches_sources(&b, &a));
}

#[test]
fn exhausted_reduction_budget_never_claims_minimality() {
    let (a, b, s) = contrast();
    let report = engine()
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            SearchBudget {
                max_trials: 1,
                ..SearchBudget::default()
            },
        )
        .unwrap();
    let w = report.witness.unwrap();
    assert!(!w.witness().minimization.as_ref().unwrap().complete);
    assert!(report.diagnostics.iter().any(|s| s.contains("incomplete")));
}

#[test]
fn invalid_and_inconclusive_workflows_never_create_proof() {
    let (a, b, mut s) = contrast();
    s.validity.push(AcceptedProperty {
        id: "legitimate".into(),
        description: "Export must contain one hundred rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::OutputCount {
                point: "done".into(),
                output: "roster".into(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 100 },
            },
        },
    });
    let r = engine()
        .compare(&a, &b, &s, &decisions(), target(), RuntimeLimits::default())
        .unwrap();
    assert_eq!(r.state, EvidenceState::Failed);
    assert!(r.witness.is_none());
    assert_eq!(r.runs.len(), 2);
    s.validity.clear();
    let r = engine()
        .compare(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            RuntimeLimits {
                fuel: 1,
                ..RuntimeLimits::default()
            },
        )
        .unwrap();
    assert_eq!(r.state, EvidenceState::Inconclusive);
    assert!(r.witness.is_none());
    assert!(r.runs.iter().any(|r| r.trace.len() < s.inputs.len()));
}

#[test]
fn runtime_failure_keeps_real_partial_trace_and_cancellation_is_bounded() {
    let p = capture(equipment());
    let mut s = scenario(
        &p,
        vec![
            invoke("borrow", args(&[("asset", reference("asset", "drill"))])),
            SemanticInput::Observe {
                point: "first".into(),
            },
            invoke("borrow", args(&[("asset", reference("asset", "drill"))])),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    s.seed = equipment_seed(&p);
    let t = ObservationTarget::Observable {
        point: "done".into(),
        observable: "loan_count".into(),
    };
    let r = engine()
        .compare(
            &p,
            &p,
            &s,
            &decisions(),
            t.clone(),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert_eq!(r.state, EvidenceState::Failed);
    assert!(r.witness.is_none());
    assert_eq!(r.runs[0].observations.len(), 1);
    assert_eq!(r.runs[0].trace.len(), 3);
    let e = ComparisonEngine::new(Arc::new(AtomicBool::new(true)));
    let r = e
        .compare(&p, &p, &s, &decisions(), t, RuntimeLimits::default())
        .unwrap();
    assert_eq!(r.state, EvidenceState::Inconclusive);
    assert!(r.witness.is_none());
}

#[test]
fn structurally_new_transaction_composition_is_executed() {
    let mut value = equipment();
    value["actions"][0]["steps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"kind":"update","record":var("loan"),"values":{"returned":yes()}}));
    value["observables"][0]["value"] = json!({"kind":"count","items":{"kind":"query","entity":"loan","binding":"item","predicate":{"kind":"not","value":field(var("item"),"returned")},"sort":[],"limit":100,"include_archived":false}});
    let mut av = equipment();
    av["observables"] = value["observables"].clone();
    let a = capture(av);
    let b = capture(value);
    let mut s = scenario(
        &a,
        vec![
            invoke("borrow", args(&[("asset", reference("asset", "drill"))])),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    s.seed = equipment_seed(&a);
    let r = engine()
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            ObservationTarget::Observable {
                point: "done".into(),
                observable: "loan_count".into(),
            },
            SearchBudget::default(),
        )
        .unwrap();
    let w = r.witness.unwrap();
    assert_eq!(w.witness().state, EvidenceState::Observed);
    assert_ne!(
        w.witness().before.observations[0].values,
        w.witness().after.observations[0].values
    );
    assert_ne!(
        w.witness().before.trace[0].before_data,
        w.witness().before.trace[0].after_data
    );
}

#[test]
fn optional_fields_and_initial_condition_overrides_are_reduced() {
    let (a, b, mut s) = contrast();
    let mut av = serde_json::to_value(&a.program).unwrap();
    let mut bv = serde_json::to_value(&b.program).unwrap();
    for v in [&mut av, &mut bv] {
        v["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    }
    let a = capture(av);
    let b = capture(bv);
    s.seed.schema = a.program.entities.clone();
    for r in &mut s.seed.records {
        r.values.insert("note".into(), string("irrelevant"));
    }
    // Setting a filter before the first collect can be dropped when it names all rows.
    s.session.values.insert("search".into(), string("e"));
    let r = engine()
        .minimize(&a, &b, &s, &decisions(), target(), SearchBudget::default())
        .unwrap();
    let w = r.witness.unwrap();
    assert!(w
        .witness()
        .scenario
        .seed
        .records
        .iter()
        .all(|r| !r.values.contains_key("note")));
    assert!(w.witness().minimization.as_ref().unwrap().complete);
}

#[test]
fn equal_execution_is_finite_no_difference_not_universal_equivalence() {
    let (a, _, s) = contrast();
    let r = engine()
        .compare(&a, &a, &s, &decisions(), target(), RuntimeLimits::default())
        .unwrap();
    assert_eq!(r.state, EvidenceState::NoDifferenceFound);
    assert!(r.witness.is_none());
    assert!(r.diagnostics.iter().any(|s| s.contains("scenario")));
}

#[test]
fn real_view_rows_and_columns_have_typed_empty_and_null_provenance() {
    let a = capture(filtered());
    let mut v = filtered();
    v["views"][0]["kind"]["rows"]["sort"][0]["descending"] = json!(true);
    let b = capture(v);
    let s = scenario(
        &a,
        vec![
            add("Ada"),
            add("Zoe"),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let target = ObservationTarget::ViewColumn {
        point: "done".into(),
        column: "name".into(),
    };
    let r = engine()
        .minimize(&a, &b, &s, &decisions(), target, SearchBudget::default())
        .unwrap();
    let w = r.witness.unwrap();
    assert_ne!(
        w.witness().before.observations[0].view.rows,
        w.witness().after.observations[0].view.rows
    );
    let p = AcceptedProperty {
        id: "empty-rows".into(),
        description: "Empty typed rows".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewRows {
                point: "empty".into(),
                entity: "person".into(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(Type::reference("person")),
                value: DataValue::List {
                    item_type: Type::reference("person"),
                    items: vec![],
                },
            },
        },
    };
    let empty = scenario(
        &a,
        vec![SemanticInput::Observe {
            point: "empty".into(),
        }],
    );
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &a,
            &empty,
            &decisions(),
            RuntimeLimits::default(),
            "empty-run",
        )
        .unwrap();
    assert_eq!(p.evaluate(&run.observations), Some(true));
    let mut old = run.observations.clone();
    old[0].view_schema = None;
    assert_eq!(p.evaluate(&old), None);
    let mut wrong = p.clone();
    if let PropertyPredicate::Equal { left, .. } = &mut wrong.predicate {
        *left = PropertyTerm::ViewRows {
            point: "empty".into(),
            entity: "loan".into(),
        };
    }
    assert_eq!(wrong.evaluate(&run.observations), None);
    let mut optional = filtered();
    optional["entities"][0]["fields"].as_array_mut().unwrap().push(json!({"id":"note","label":"Note","value_type":{"kind":"optional","item":{"kind":"text"}}}));
    optional["views"][0]["kind"]["columns"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"note","label":"Note","value":field(var("row"),"note")}));
    let optional = capture(optional);
    let s = scenario(
        &optional,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &optional,
            &s,
            &decisions(),
            RuntimeLimits::default(),
            "null-run",
        )
        .unwrap();
    let typ = Type::Optional {
        item: Box::new(Type::Text),
    };
    let p = AcceptedProperty {
        id: "null-cell".into(),
        description: "Nullable view column".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::ViewColumn {
                point: "done".into(),
                column: "note".into(),
                value_type: typ.clone(),
            },
            right: PropertyTerm::Literal {
                value_type: Type::list(typ.clone()),
                value: DataValue::List {
                    item_type: typ,
                    items: vec![DataValue::Null],
                },
            },
        },
    };
    assert_eq!(p.evaluate(&run.observations), Some(true));
    let mut corrupt = run.observations.clone();
    corrupt[0]
        .view_schema
        .as_mut()
        .unwrap()
        .columns
        .insert("note".into(), Type::Integer);
    assert!(corrupt[0].validate().is_err());
    assert_eq!(p.evaluate(&corrupt), None);
}

#[test]
fn reduction_audit_is_reconstructible_and_runtime_version_invalidates_old_proof() {
    let (a, b, s) = contrast();
    let r = engine()
        .minimize(&a, &b, &s, &decisions(), target(), SearchBudget::default())
        .unwrap();
    let verified = r.witness.unwrap();
    assert_eq!(verified.initial_scenario(), &s);
    for run in [verified.initial_runs().0, verified.initial_runs().1] {
        run.validate().unwrap();
        assert_eq!(run.binding.scenario_digest, s.identity().unwrap());
        assert_eq!(run.trace.len(), s.inputs.len());
    }
    let certificate = verified.witness().minimization.as_ref().unwrap();
    assert_eq!(certificate.trials.len(), verified.reduction_audit().len());
    for (trial, audit) in certificate.trials.iter().zip(verified.reduction_audit()) {
        assert_eq!(trial, &audit.trial);
        for run in &audit.runs {
            run.validate().unwrap();
            assert!(
                Some(run.identity().unwrap()) == trial.before_run
                    || Some(run.identity().unwrap()) == trial.after_run
            );
        }
    }
    let current = &verified.witness().after.binding;
    assert_eq!(current.runtime_version, "local-interpreter/3");
    let mut old = verified.witness().after.clone();
    old.binding.runtime_version = "local-interpreter/1".into();
    assert!(!old.is_current(current));
    assert_eq!(current.driver_version, "semantic-input/3");
    old = verified.witness().after.clone();
    old.binding.driver_version = "semantic-input/1".into();
    assert!(!old.is_current(current));
    let r = engine()
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            SearchBudget {
                max_evidence_bytes: 1,
                ..SearchBudget::default()
            },
        )
        .unwrap();
    assert!(
        !r.witness
            .unwrap()
            .witness()
            .minimization
            .as_ref()
            .unwrap()
            .complete
    );
}

#[test]
fn reduction_does_not_substitute_order_for_membership_difference() {
    let (a, _, mut s) = contrast();
    let mut av = serde_json::to_value(&a.program).unwrap();
    av["actions"][1]["steps"][0]["items"]["sort"] =
        json!([{"value":field(var("item"),"name"),"descending":false}]);
    av["observables"].as_array_mut().unwrap().push(
        json!({"id":"members","label":"Members","value":{"kind":"state","state":"selected"}}),
    );
    av["observables"].as_array_mut().unwrap().push(
        json!({"id":"total","label":"Total","value":{"kind":"count","items":query("person")}}),
    );
    s.validity.push(AcceptedProperty {
        id: "multiple-record-workflow".into(),
        description: "This workflow needs multiple records".into(),
        predicate: PropertyPredicate::Less {
            left: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 1 },
            },
            right: PropertyTerm::Observed {
                point: "done".into(),
                observable: "total".into(),
                value_type: Type::Integer,
            },
        },
    });
    let mut bv = av.clone();
    let mut items = bv["actions"][1]["steps"][0]["items"].clone();
    items["sort"][0]["descending"] = json!(true);
    bv["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":items});
    let a = capture(av);
    let b = capture(bv);
    s.seed.schema = a.program.entities.clone();
    let report = engine()
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            ObservationTarget::Observable {
                point: "done".into(),
                observable: "members".into(),
            },
            SearchBudget::default(),
        )
        .unwrap();
    let w = report.witness.unwrap();
    let left = &w.witness().before.observations[0].values["members"];
    let right = &w.witness().after.observations[0].values["members"];
    match (left, right) {
        (DataValue::List { items: a, .. }, DataValue::List { items: b, .. }) => {
            assert!(a.len() > b.len())
        }
        _ => panic!("expected actual member lists"),
    }
}

#[test]
fn reduction_audit_retains_side_when_only_after_can_start() {
    let (a, b, mut s) = contrast();
    let mut av = serde_json::to_value(&a.program).unwrap();
    av["entities"][0]["constraints"] = json!([{"kind":"less","left":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}},"right":{"kind":"count","items":query("person")}}]);
    let a = capture(av);
    s.seed.schema = a.program.entities.clone();
    let report = engine()
        .minimize(&a, &b, &s, &decisions(), target(), SearchBudget::default())
        .unwrap();
    let witness = report.witness.unwrap();
    let audits: Vec<_> = witness
        .reduction_audit()
        .iter()
        .filter(|a| a.runs.len() == 1)
        .collect();
    assert!(!audits.is_empty());
    for audit in audits {
        assert_eq!(audit.runs[0].id, "after-run");
        assert!(audit.trial.before_run.is_none());
        assert_eq!(
            audit.trial.after_run,
            Some(audit.runs[0].identity().unwrap())
        );
    }
}

#[test]
fn observation_instrumentation_cannot_change_created_records_or_ordered_exports() {
    let source = capture(filtered());
    let initial = scenario(
        &source,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = product_runtime::LocalRuntime::default();
    let original = runtime
        .replay(
            &source,
            &initial,
            &decisions(),
            RuntimeLimits::default(),
            "original",
        )
        .unwrap();
    assert_eq!(original.state, EvidenceState::Observed);
    assert_eq!(original.observations[0].view.rows.len(), 2);
    assert_ne!(
        original.observations[0].view.rows[0].record,
        original.observations[0].view.rows[1].record
    );
    let mut instrumented = initial.clone();
    instrumented.inputs.insert(
        1,
        SemanticInput::Observe {
            point: "midway".into(),
        },
    );
    instrumented.inputs.push(SemanticInput::Observe {
        point: "again".into(),
    });
    for renamed in [false, true] {
        let mut scene = instrumented.clone();
        if renamed {
            for input in &mut scene.inputs {
                if let SemanticInput::Observe { point } = input {
                    *point = format!("renamed-{point}");
                }
            }
        }
        let actual = runtime
            .replay(
                &source,
                &scene,
                &decisions(),
                RuntimeLimits::default(),
                "instrumented",
            )
            .unwrap();
        assert_eq!(actual.state, EvidenceState::Observed);
        let last = actual.observations.last().unwrap();
        let expected = &original.observations[0];
        assert_eq!(last.data_digest, expected.data_digest);
        assert_eq!(last.session_digest, expected.session_digest);
        assert_eq!(last.view, expected.view);
        assert_eq!(last.outputs, expected.outputs);
        assert_ne!(actual.binding.input_digest, original.binding.input_digest);
    }
}

#[test]
fn mapped_action_names_and_additive_schema_preserve_replay_record_identity() {
    let source = capture(filtered());
    let initial = scenario(
        &source,
        vec![
            add("Ada"),
            add("Zoe"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let runtime = product_runtime::LocalRuntime::default();
    let original = runtime
        .replay(
            &source,
            &initial,
            &decisions(),
            RuntimeLimits::default(),
            "original",
        )
        .unwrap();
    let mut value = filtered();
    let old = value["actions"][0]["id"].as_str().unwrap().to_string();
    value["actions"][0]["id"] = json!("renamed_creation");
    value["views"][1]["kind"]["action"] = json!("renamed_creation");
    let mut extra = value["entities"][0].clone();
    extra["id"] = json!("extra");
    value["entities"].as_array_mut().unwrap().push(extra);
    let mapped = capture(value);
    let mut changed = initial.clone();
    changed.id = "another-scene-name".into();
    changed.label = "Another display label".into();
    changed.seed.schema = mapped.program.entities.clone();
    for input in &mut changed.inputs {
        if let SemanticInput::Invoke { action, .. } = input {
            if action == &old {
                *action = "renamed_creation".into();
            }
        }
    }
    let actual = runtime
        .replay(
            &mapped,
            &changed,
            &decisions(),
            RuntimeLimits::default(),
            "mapped",
        )
        .unwrap();
    assert_eq!(actual.state, EvidenceState::Observed);
    assert_eq!(actual.observations[0].view, original.observations[0].view);
    assert_eq!(
        actual.observations[0].outputs,
        original.observations[0].outputs
    );
    assert_ne!(actual.binding.input_digest, original.binding.input_digest);
    assert_ne!(actual.binding.source, original.binding.source);
}

#[test]
fn replay_ids_are_unique_for_repeated_mutations_and_interleaved_steps() {
    let source = capture(filtered());
    let scene = scenario(
        &source,
        vec![
            add("Ada"),
            SemanticInput::Observe {
                point: "first".into(),
            },
            SemanticInput::Navigate {
                view: "people".into(),
            },
            SemanticInput::AdvanceClock { days: 1 },
            add("Ada"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let ids = scene.replay_operation_ids().unwrap();
    assert_eq!(ids.len(), scene.inputs.len());
    assert_eq!(
        ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
        ids.len()
    );
    assert!(ids.iter().all(|id| id.len() <= 128));
    let run = product_runtime::LocalRuntime::default()
        .replay(
            &source,
            &scene,
            &decisions(),
            RuntimeLimits::default(),
            "repeated",
        )
        .unwrap();
    assert_eq!(run.state, EvidenceState::Observed);
    assert_eq!(run.observations[1].view.rows.len(), 2);
    assert_ne!(
        run.observations[1].view.rows[0].record,
        run.observations[1].view.rows[1].record
    );
    assert_eq!(run.observations[1].outputs[0].rows.len(), 2);
    let mut changed = scene.clone();
    changed.random_seed += 1;
    assert_ne!(
        changed.operation_namespace().unwrap(),
        scene.operation_namespace().unwrap()
    );
}

#[test]
fn evidence_overflow_retains_an_executed_failed_reduction_receipt() {
    let (before, after, mut scene) = contrast();
    scene.session.values.insert(
        "selected".into(),
        DataValue::List {
            item_type: Type::reference("person"),
            items: vec![reference("person", "ada")],
        },
    );
    let first = engine()
        .single_reductions(&before, &after, &scene)
        .unwrap()
        .remove(0);
    let failed = engine()
        .compare(
            &before,
            &after,
            &first.1,
            &decisions(),
            target(),
            RuntimeLimits::default(),
        )
        .unwrap();
    assert!(failed.witness.is_none());
    assert_eq!(failed.state, EvidenceState::Failed);
    let report = engine()
        .minimize(
            &before,
            &after,
            &scene,
            &decisions(),
            target(),
            SearchBudget {
                max_evidence_bytes: 1,
                ..SearchBudget::default()
            },
        )
        .unwrap();
    let witness = report.witness.unwrap();
    assert_eq!(witness.reduction_audit().len(), 1);
    let receipt = &witness.reduction_audit()[0];
    assert!(receipt.evidence_omitted);
    assert_eq!(receipt.comparison_state, EvidenceState::Failed);
    assert_eq!(receipt.trial.reduced, first.1.identity().unwrap());
    assert_eq!(receipt.trial.outcome, ReductionOutcome::Inconclusive);
    assert!(receipt.diagnostics.iter().any(|s| s.contains("Failed")));
    assert!(receipt.diagnostics.iter().any(|s| s.contains("before-run")));
    assert!(receipt.diagnostics.iter().any(|s| s.contains("after-run")));
    assert!(receipt.diagnostics.iter().any(|s| s.contains("omitted")));
    assert!(!witness.witness().minimization.as_ref().unwrap().complete);
}

#[test]
fn unrepresentable_valid_reductions_cannot_certify_one_minimality() {
    let mut before = filtered();
    before["state"].as_array_mut().unwrap().push(json!({
        "id":"export_values", "label":"Export values", "value_type":{"kind":"list","item":{"kind":"integer"}},
        "initial":{"kind":"list","item_type":{"kind":"integer"},"items":(0..6000).map(|value|json!({"kind":"integer","value":value})).collect::<Vec<_>>()}
    }));
    before["actions"].as_array_mut().unwrap().push(json!({
        "id":"narrow", "label":"Narrow rows", "parameters":{},"guards":[],"ensures":[],
        "steps":[{"kind":"set_state","state":"export_values","value":{"kind":"filter","items":{"kind":"state","state":"export_values"},"binding":"n","predicate":{"kind":"less","left":var("n"),"right":{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":4000}}}}}]
    }));
    before["actions"][2]["steps"][0]["items"] = json!({"kind":"state","state":"export_values"});
    before["actions"][2]["steps"][0]["columns"]["name"] = text("before");
    let mut after = before.clone();
    after["actions"][2]["steps"][0]["columns"]["name"] = text("after");
    let (before, after) = (capture(before), capture(after));
    let mut scene = scenario(
        &before,
        vec![
            invoke("narrow", Values::new()),
            invoke("export_people", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    scene.validity.push(AcceptedProperty {
        id: "enough-rows".into(),
        description: "Retain at least 8000 actually emitted rows".into(),
        predicate: PropertyPredicate::Not {
            value: Box::new(PropertyPredicate::Less {
                left: PropertyTerm::OutputCount {
                    point: "done".into(),
                    output: "roster".into(),
                },
                right: PropertyTerm::Literal {
                    value_type: Type::Integer,
                    value: DataValue::Integer { value: 8000 },
                },
            }),
        },
    });
    let report = engine()
        .minimize(
            &before,
            &after,
            &scene,
            &decisions(),
            ObservationTarget::OutputColumn {
                point: "done".into(),
                output: "roster".into(),
                column: "name".into(),
            },
            SearchBudget {
                runtime: RuntimeLimits {
                    fuel: 10_000_000,
                    elapsed_millis: 60_000,
                    ..RuntimeLimits::default()
                },
                ..SearchBudget::default()
            },
        )
        .unwrap();
    let witness = report.witness.unwrap();
    let trial = witness
        .reduction_audit()
        .iter()
        .find(|a| a.trial.operation == "delete input 0")
        .unwrap();
    assert_eq!(trial.runs.len(), 2);
    for run in &trial.runs {
        assert_eq!(run.state, EvidenceState::Observed, "{:?}", run.errors);
        assert_eq!(scene.validity[0].evaluate(&run.observations), Some(true));
        assert_eq!(
            run.observations[0]
                .outputs
                .iter()
                .map(|a| a.rows.len())
                .sum::<usize>(),
            12_000
        );
    }
    assert_ne!(
        trial.runs[0].observations[0].outputs[0].rows[0],
        trial.runs[1].observations[0].outputs[0].rows[0]
    );
    assert_eq!(trial.trial.outcome, ReductionOutcome::Inconclusive);
    assert!(!witness.witness().minimization.as_ref().unwrap().complete);
    assert!(report.diagnostics.iter().any(|s| s.contains("incomplete")));
}
