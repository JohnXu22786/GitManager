//! Synthetic parity and validation-work regressions, not acceptance evidence.
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
use product_runtime::*;
use product_scenarios::*;
use serde_json::json;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

fn contrast() -> (CapturedProgram, CapturedProgram, ScenarioSpec) {
    let a = capture(filtered());
    let mut value = filtered();
    value["actions"][1]["steps"][0] = json!({"kind":"set_state","state":"selected","value":value["actions"][1]["steps"][0]["items"].clone()});
    let b = capture(value);
    let mut s = scenario(
        &a,
        vec![
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
    for name in ["Ada", "Zoe", "Extra", "Four", "Five", "Six", "Seven"] {
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
fn parity(
    runtime: &LocalRuntime,
    p: &CapturedProgram,
    prepared: &PreparedProgram,
    s: &ScenarioSpec,
    limits: RuntimeLimits,
    admission: Option<&dyn ReplayAdmission>,
) {
    let ordinary = runtime.replay_admitted(p, s, &decisions(), limits.clone(), "run", admission);
    let reused = runtime.replay_prepared(prepared, s, &decisions(), limits, "run", admission);
    assert_eq!(format!("{ordinary:?}"), format!("{reused:?}"));
}

#[test]
fn prepared_minimization_preserves_complete_evidence() {
    let (a, b, s) = contrast();
    struct ExecutionCount {
        seeds: AtomicUsize,
        states: AtomicUsize,
    }
    impl ReplayAdmission for ExecutionCount {
        fn validate_seed(
            &self,
            _: &CapturedProgram,
            _: &DataSnapshot,
            _: i32,
        ) -> Result<(), AdapterError> {
            self.seeds.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn validate_state(
            &self,
            _: &CapturedProgram,
            _: &DataSnapshot,
            _: i32,
        ) -> Result<(), AdapterError> {
            self.states.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let executions = Arc::new(ExecutionCount {
        seeds: AtomicUsize::new(0),
        states: AtomicUsize::new(0),
    });
    let engine =
        ComparisonEngine::new(Arc::new(AtomicBool::new(false))).with_admission(executions.clone());
    let budget = SearchBudget::default();
    let ordinary = engine
        .test_minimize_unprepared(&a, &b, &s, &decisions(), target(), budget.clone())
        .unwrap();
    let ordinary_checks = validation_counts();
    let ordinary_executions = (
        executions.seeds.swap(0, Ordering::SeqCst),
        executions.states.swap(0, Ordering::SeqCst),
    );
    reset_validation_counts();
    let _prepared_a = PreparedProgram::new(&a).unwrap();
    let _prepared_b = PreparedProgram::new(&b).unwrap();
    let once = validation_counts();
    assert_eq!(once.captures, 2);
    assert_eq!(once.observables, 2);
    assert_eq!(once.views, a.program.views.len() + b.program.views.len());
    reset_validation_counts();
    let reused = engine
        .minimize(&a, &b, &s, &decisions(), target(), budget)
        .unwrap();
    assert_eq!(
        validation_counts(),
        once,
        "all source/static metadata work is bounded by two captures, never trial count"
    );
    assert_eq!(format!("{ordinary:?}"), format!("{reused:?}"));
    let verified = reused.witness.unwrap();
    let certificate = verified.witness().minimization.as_ref().unwrap();
    assert!(certificate.complete);
    eprintln!(
        "source validation work: ordinary={ordinary_checks:?}, prepared={once:?}, trials={}",
        certificate.trials.len()
    );
    assert!(certificate.trials.len() > 10);
    assert_eq!(certificate.trials.len(), verified.reduction_audit().len());
    assert_eq!(
        executions.seeds.load(Ordering::SeqCst),
        2 * (certificate.trials.len() + 1)
    );
    assert_eq!(
        (
            executions.seeds.load(Ordering::SeqCst),
            executions.states.load(Ordering::SeqCst)
        ),
        ordinary_executions
    );
    assert!(
        verified
            .reduction_audit()
            .iter()
            .flat_map(|a| &a.runs)
            .count()
            > 10
    );
}

#[test]
fn prepared_source_rejects_mutations_and_owns_snapshot() {
    let (a, b, s) = contrast();
    let prepared = PreparedProgram::new(&a).unwrap();
    let mut mutations = vec![];
    let mut p = a.clone();
    p.source_bytes.push(b' ');
    mutations.push(p);
    let mut p = a.clone();
    p.program.label.push('x');
    mutations.push(p);
    let mut p = a.clone();
    p.artifact.raw_digest = b.artifact.raw_digest.clone();
    mutations.push(p);
    let mut p = a.clone();
    p.artifact.program_digest = b.artifact.program_digest.clone();
    mutations.push(p);
    let mut p = a.clone();
    p.artifact.semantic_digest = b.artifact.semantic_digest.clone();
    mutations.push(p);
    let mut p = a.clone();
    p.binding.source_digest = b.binding.source_digest.clone();
    mutations.push(p);
    let mut p = a.clone();
    p.binding.project_id = String::new();
    mutations.push(p);
    for p in mutations {
        assert!(PreparedProgram::new(&p).is_err());
        let engine = ComparisonEngine::new(Arc::new(AtomicBool::new(false)));
        assert!(engine
            .compare(&p, &b, &s, &decisions(), target(), RuntimeLimits::default())
            .is_err());
        assert!(engine
            .minimize(&p, &b, &s, &decisions(), target(), SearchBudget::default())
            .is_err());
        assert!(engine.single_reductions(&p, &b, &s).is_err());
        assert!(LocalRuntime::default()
            .replay(&p, &s, &decisions(), RuntimeLimits::default(), "run")
            .is_err());
    }
    let mut invalid_app = a.program.clone();
    invalid_app.initial_view = "absent".into();
    assert!(invalid_app.observable_types().is_err());
    assert!(invalid_app.view_schema(&a.program.initial_view).is_err());
    assert!(SessionState::initial(&invalid_app).is_err());
    assert!(s.session.validate(&invalid_app).is_err());
    assert!(s.validate(&invalid_app).is_err());
    let mut changed = a.clone();
    changed.program.label = "changed".into();
    assert_eq!(prepared.capture(), &a);
    parity(
        &LocalRuntime::default(),
        &a,
        &prepared,
        &s,
        RuntimeLimits::default(),
        None,
    );
    let witness = ComparisonEngine::new(Arc::new(AtomicBool::new(false)))
        .minimize(
            &a,
            &b,
            &s,
            &decisions(),
            target(),
            SearchBudget {
                max_trials: 0,
                ..SearchBudget::default()
            },
        )
        .unwrap()
        .witness
        .unwrap();
    assert!(!witness.matches_sources(&changed, &b));
    let mut rebound = a.clone();
    rebound.binding.project_id = "another-project".into();
    assert!(PreparedProgram::new(&rebound).is_ok());
    assert!(LocalRuntime::default()
        .replay_prepared(
            &PreparedProgram::new(&rebound).unwrap(),
            &s,
            &decisions(),
            RuntimeLimits::default(),
            "run",
            None
        )
        .is_err());
    assert!(!witness.matches_sources(&rebound, &b));
}

#[test]
fn prepared_replay_checks_dynamic_inputs_and_budgets() {
    let mut value = filtered();
    value["entities"][0]["constraints"] = json!([{"kind":"not","value":{"kind":"equal","left":field(var("record"),"name"),"right":text("")}}]);
    value["actions"][0]["guards"] =
        json!([{"kind":"not","value":{"kind":"equal","left":var("name"),"right":text("blocked")}}]);
    let p = capture(value);
    let prepared = PreparedProgram::new(&p).unwrap();
    let runtime = LocalRuntime::default();
    let good = scenario(
        &p,
        vec![
            add("Ada"),
            invoke("collect", Values::new()),
            invoke("export_people", Values::new()),
            SemanticInput::Observe {
                point: "done".into(),
            },
        ],
    );
    let mut cases = vec![good.clone()];
    let mut s = good.clone();
    s.inputs[0] = add("blocked");
    cases.push(s);
    let mut s = good.clone();
    s.inputs[0] = add("");
    cases.push(s);
    let mut s = good.clone();
    s.session
        .values
        .insert("search".into(), DataValue::Integer { value: 4 });
    cases.push(s);
    let mut s = good.clone();
    s.session.focused_record = Some(RecordRef {
        entity: "person".into(),
        record: "absent".into(),
    });
    cases.push(s);
    let mut s = good.clone();
    s.session.values.insert(
        "selected".into(),
        DataValue::List {
            item_type: Type::reference("person"),
            items: vec![reference("person", "missing")],
        },
    );
    cases.push(s);
    let mut s = good.clone();
    s.seed.records.push(Record {
        entity: "person".into(),
        id: "bad".into(),
        revision: 1,
        created_program: p.artifact.program_digest.clone(),
        archived: false,
        values: args(&[("name", string("")), ("area", string("north"))]),
    });
    cases.push(s);
    let mut s = good.clone();
    s.inputs[0] = invoke(
        "add_person",
        args(&[
            ("name", DataValue::Integer { value: 3 }),
            ("area", string("north")),
        ]),
    );
    cases.push(s);
    for s in cases {
        parity(&runtime, &p, &prepared, &s, RuntimeLimits::default(), None);
    }
    for fuel in [1, 2, 10, 50, 100, 500, 2000] {
        parity(
            &runtime,
            &p,
            &prepared,
            &good,
            RuntimeLimits {
                fuel,
                ..RuntimeLimits::default()
            },
            None,
        );
    }
    parity(
        &runtime,
        &p,
        &prepared,
        &good,
        RuntimeLimits {
            output_bytes: 1,
            ..RuntimeLimits::default()
        },
        None,
    );
    parity(
        &runtime,
        &p,
        &prepared,
        &good,
        RuntimeLimits {
            transaction_writes: 1,
            ..RuntimeLimits::default()
        },
        None,
    );
    // A new successful replay must not inherit a prior exhausted/rejected run.
    assert_eq!(
        runtime
            .replay_prepared(
                &prepared,
                &good,
                &decisions(),
                RuntimeLimits::default(),
                "run",
                None
            )
            .unwrap()
            .state,
        EvidenceState::Observed
    );
}

#[test]
fn prepared_replay_checks_admission_and_cancellation_each_time() {
    struct Admission {
        rejected: AtomicBool,
        seeds: AtomicUsize,
        states: AtomicUsize,
    }
    impl ReplayAdmission for Admission {
        fn validate_seed(
            &self,
            _: &CapturedProgram,
            _: &DataSnapshot,
            _: i32,
        ) -> Result<(), AdapterError> {
            self.seeds.fetch_add(1, Ordering::SeqCst);
            if self.rejected.load(Ordering::SeqCst) {
                Err(AdapterError::Stale("changed admission".into()))
            } else {
                Ok(())
            }
        }
        fn validate_state(
            &self,
            _: &CapturedProgram,
            _: &DataSnapshot,
            _: i32,
        ) -> Result<(), AdapterError> {
            self.states.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }
    let (p, _, s) = contrast();
    let prepared = PreparedProgram::new(&p).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let runtime = LocalRuntime::with_cancellation(cancelled.clone());
    let admission = Admission {
        rejected: AtomicBool::new(false),
        seeds: AtomicUsize::new(0),
        states: AtomicUsize::new(0),
    };
    for rejected in [false, true, false] {
        admission.rejected.store(rejected, Ordering::SeqCst);
        parity(
            &runtime,
            &p,
            &prepared,
            &s,
            RuntimeLimits::default(),
            Some(&admission),
        );
    }
    assert_eq!(admission.seeds.load(Ordering::SeqCst), 6);
    assert_eq!(admission.states.load(Ordering::SeqCst), 4 * s.inputs.len());
    for stopped in [true, false, true] {
        cancelled.store(stopped, Ordering::SeqCst);
        parity(&runtime, &p, &prepared, &s, RuntimeLimits::default(), None);
    }
}
