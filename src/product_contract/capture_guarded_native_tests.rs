//! Guarded native source admission controls. No original setup or route runner.
use super::app_length_tests::{
    capture_minimal, complete_fixture, deepest_valid_fixture, old, old_app, source_result, Work,
};
use super::*;
use serde_json::{json, Value};

fn native_capture(value: Value) -> CapturedProgram {
    let mut c = capture_minimal();
    c.program = serde_json::from_value(value).unwrap();
    c.source_bytes = serde_json::to_vec(&c.program).unwrap();
    rehash(&mut c);
    c
}
fn rehash(c: &mut CapturedProgram) {
    c.artifact.raw_digest = bytes_digest(IdentityDomain::Source, &c.source_bytes);
    c.binding.source_digest = c.artifact.raw_digest.clone();
    c.artifact.program_digest = canonical_digest(IdentityDomain::Program, &c.program).unwrap();
    let mut semantic = c.program.clone();
    semantic
        .clear_display_labels_audited(Audit::none())
        .unwrap();
    c.artifact.semantic_digest =
        canonical_digest(IdentityDomain::SemanticProgram, &semantic).unwrap();
}
fn expected(c: &CapturedProgram) -> std::result::Result<(), String> {
    let old: old::CapturedProgram =
        serde_json::from_slice(&serde_json::to_vec(c).unwrap()).unwrap();
    old.validate().map_err(|e| e.0)
}
fn facts(c: &CapturedProgram, w: &mut Work) -> ContractWorkResult<[usize; 4]> {
    let state = WorkState::new(w);
    state.finish(c.validate_audited(Audit(Some(&state))).map(|f| {
        [
            f.total_nodes,
            f.action_prefix_nodes,
            f.canonical_len,
            f.action_count,
        ]
    }))
}
#[test]
fn guarded_native_hit_removes_intake_preserves_facts_and_bytes() {
    for value in [
        app_length_tests::fixture::minimal(),
        complete_fixture(),
        deepest_valid_fixture(),
    ] {
        let c = native_capture(value);
        let mut w = Work::default();
        closed_value::native_outputs::begin();
        let f = facts(&c, &mut w).unwrap();
        let outputs = closed_value::native_outputs::take();
        assert_eq!(outputs.len(), 2);
        assert_eq!(
            outputs[0].0,
            old::canonical_bytes(&old_app(&c.program)).unwrap()
        );
        let mut semantic = c.program.clone();
        semantic
            .clear_display_labels_audited(Audit::none())
            .unwrap();
        assert_eq!(
            outputs[1].0,
            old::canonical_bytes(&old_app(&semantic)).unwrap()
        );
        assert_ne!(
            outputs[0].1, outputs[1].1,
            "Program Vec stays alive through Semantic"
        );
        let original = c.program.validate_audited(Audit::none()).unwrap();
        assert_eq!(
            f,
            [
                original.total_nodes,
                original.action_prefix_nodes,
                outputs[0].0.len(),
                original.action_count
            ]
        );
        // Old actual-Value admission eagerly starts format(107), then parses.
        let mut intake = Work::default();
        let state = WorkState::new(&mut intake);
        Audit(Some(&state)).format(107).unwrap();
        assert!(
            !w.calls
                .windows(intake.calls.len())
                .any(|p| p == intake.calls),
            "exact native source hit must not run the old tokenizer/intake"
        );
        assert!(!w.calls.windows(6).any(|p| p == [0, 96, 170496, 358, 1, 1]));
    }
}
#[test]
fn guarded_native_spelling_misses_keep_full_fallback_and_error_order() {
    let base = native_capture(app_length_tests::fixture::minimal());
    let value = serde_json::to_value(&base.program).unwrap();
    let mut cases = vec![
        serde_json::to_vec(&value).unwrap(),
        serde_json::to_vec_pretty(&value).unwrap(),
    ];
    let mut omitted = value.clone();
    omitted["views"][0]["kind"]
        .as_object_mut()
        .unwrap()
        .remove("selection");
    cases.push(serde_json::to_vec(&omitted).unwrap());
    cases.extend(
        app_length_tests::fixture::malformed_sources(MAX_WIRE_BYTES)
            .into_iter()
            .filter(|(name, _)| !name.contains("size") && !name.contains("oversize"))
            .map(|(_, b)| b),
    );
    for raw in cases {
        let mut c = base.clone();
        c.source_bytes = raw;
        rehash(&mut c);
        let want = expected(&c);
        let mut w = Work::default();
        let got = source_result(&c, &mut w).map_err(|e| match e {
            ContractWorkError::Contract(e) => e.0,
            e => panic!("{e:?}"),
        });
        assert_eq!(got, want);
        c.program.version = 2;
        let want = expected(&c);
        let mut w = Work::default();
        let got = source_result(&c, &mut w).map_err(|e| match e {
            ContractWorkError::Contract(e) => e.0,
            e => panic!("{e:?}"),
        });
        assert_eq!(got, want, "retained speculative failure must stay private");
    }
}

// New-stage observations/faults exist only in tests; the recorded counter is
// driven by the real work callback, not synthesized production payments.
use super::closed_value::guarded::controls;
struct TracedWork {
    inner: Work,
}
impl ContractWork for TracedWork {
    fn charge(&mut self, n: u64) -> std::result::Result<(), WorkFailure> {
        controls::tick();
        self.inner.charge(n)
    }
}
fn traced(
    c: &CapturedProgram,
    refuse: Option<usize>,
    cause: WorkFailure,
    fault: Option<&'static str>,
) -> (
    ContractWorkResult<[usize; 4]>,
    Vec<u64>,
    Vec<(&'static str, usize)>,
) {
    controls::begin(fault);
    let mut w = TracedWork {
        inner: Work {
            refuse,
            cause: Some(cause),
            ..Work::default()
        },
    };
    let state = WorkState::new(&mut w);
    let result = state.finish(c.validate_audited(Audit(Some(&state))).map(|f| {
        [
            f.total_nodes,
            f.action_prefix_nodes,
            f.canonical_len,
            f.action_count,
        ]
    }));
    (result, w.inner.calls, controls::take())
}
#[test]
fn guarded_native_length_miss_precedes_allocation_and_fallback_is_fresh() {
    let base = native_capture(app_length_tests::fixture::minimal());
    for raw in [
        b"{".to_vec(),
        [base.source_bytes.clone(), vec![b' ']].concat(),
        vec![b' '; base.source_bytes.len() - 1],
    ] {
        let mut c = base.clone();
        c.source_bytes = raw;
        let (result, calls, events) = traced(&c, None, WorkFailure::Cancelled, None);
        assert!(events.iter().any(|e| e.0 == "length-miss"));
        assert!(!events.iter().any(|e| e.0 == "raw-native"));
        let at = events.iter().find(|e| e.0 == "fallback").unwrap().1;
        let mut old_work = Work::default();
        let state = WorkState::new(&mut old_work);
        let actual = capture_typed::parse_app(&c.source_bytes, Audit(Some(&state)));
        let parsed = state.finish(actual);
        assert_eq!(&calls[at..at + old_work.calls.len()], old_work.calls);
        if let Err(ContractWorkError::Contract(error)) = parsed {
            assert_eq!(result.unwrap_err(), ContractWorkError::Contract(error));
        }
    }
    let mut c = base.clone();
    let at = c.source_bytes.iter().position(|&b| b == b'S').unwrap();
    c.source_bytes[at] = b'T';
    let (_, _, events) = traced(&c, None, WorkFailure::Cancelled, None);
    assert!(events.iter().any(|e| e.0 == "raw-native"));
    assert!(events.iter().any(|e| e.0 == "raw-drop"));
    assert!(events.iter().any(|e| e.0 == "fallback"));
}
#[test]
fn guarded_native_new_refusals_preserve_first_cause_without_fallback() {
    let c = native_capture(app_length_tests::fixture::minimal());
    let (ok, calls, events) = traced(&c, None, WorkFailure::Cancelled, None);
    ok.unwrap();
    let mut indices = vec![45, 46];
    for &(name, at) in &events {
        if matches!(
            name,
            "preflight-head"
                | "entity-next"
                | "field-next"
                | "type-next"
                | "semantic"
                | "raw-shape"
                | "match-length"
                | "raw-reserve"
                | "raw-native-ok"
                | "wire-tail"
                | "equal-bytes"
                | "material-root"
                | "material-shape"
                | "material-errors"
                | "material-native-reserve"
                | "material-native-ok"
                | "pending"
        ) {
            indices.push(at + 1);
        }
    }
    for &(name, at) in &events {
        if matches!(name, "raw-native" | "material-native") {
            indices.push(at);
        }
        if matches!(name, "raw-shape" | "material-shape" | "material-errors") {
            indices.extend(at + 1..=at + 6);
        }
    }
    indices.sort_unstable();
    indices.dedup();
    for at in indices {
        assert!(at <= calls.len());
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("guarded test refusal"),
        ] {
            let (result, observed, events) = traced(&c, Some(at), cause.clone(), None);
            assert_eq!(result.unwrap_err(), ContractWorkError::Resource(cause));
            assert_eq!(observed, &calls[..at]);
            assert!(!events.iter().any(|e| e.0 == "fallback"));
        }
    }
}
#[test]
fn guarded_native_native_error_has_no_success_poll_and_drops_before_fallback() {
    let c = native_capture(app_length_tests::fixture::minimal());
    let (result, calls, events) = traced(&c, None, WorkFailure::Cancelled, Some("raw-error"));
    result.unwrap();
    let error = events.iter().find(|e| e.0 == "raw-native-error").unwrap().1;
    let fallback = events.iter().find(|e| e.0 == "fallback").unwrap().1;
    assert_eq!(
        error, fallback,
        "no payment or success poll after native error"
    );
    assert_ne!(calls[fallback], 0);
    let (result, calls, events) = traced(&c, None, WorkFailure::Cancelled, Some("value-error"));
    assert!(matches!(result, Err(ContractWorkError::Contract(_))));
    let error = events
        .iter()
        .find(|e| e.0 == "material-native-error")
        .unwrap()
        .1;
    assert_eq!(
        error,
        calls.len(),
        "no success poll after materialization error"
    );
    assert!(!events.iter().any(|e| e.0 == "fallback"));
}
#[test]
fn guarded_native_identity_short_circuits_and_no_retained_mutation() {
    let c = native_capture(complete_fixture());
    for fault in 0..3 {
        let mut bad = c.clone();
        match fault {
            0 => bad.artifact.raw_digest = bytes_digest(IdentityDomain::Source, b"wrong"),
            1 => bad.artifact.program_digest = bytes_digest(IdentityDomain::Program, b"wrong"),
            _ => {
                bad.artifact.semantic_digest =
                    bytes_digest(IdentityDomain::SemanticProgram, b"wrong")
            }
        };
        let before = serde_json::to_vec(&bad).unwrap();
        closed_value::native_outputs::begin();
        let (result, _, events) = traced(&bad, None, WorkFailure::Cancelled, None);
        let outputs = closed_value::native_outputs::take();
        assert_eq!(
            result.unwrap_err(),
            ContractWorkError::Contract(ContractError(
                if fault == 0 {
                    "raw source digest mismatch"
                } else {
                    "program digest mismatch"
                }
                .into()
            ))
        );
        assert_eq!(outputs.len(), [0, 1, 2][fault]);
        assert_eq!(events.iter().any(|e| e.0 == "material-native"), fault != 0);
        assert_eq!(serde_json::to_vec(&bad).unwrap(), before);
    }
}
#[test]
fn guarded_native_overflows_are_sticky_resources_not_misses() {
    let c = native_capture(app_length_tests::fixture::minimal());
    for fault in [
        "raw-length-overflow",
        "raw-native-overflow",
        "material-native-overflow",
    ] {
        let (result, _, events) = traced(&c, None, WorkFailure::Cancelled, Some(fault));
        assert_eq!(
            result.unwrap_err(),
            ContractWorkError::Resource(WorkFailure::BudgetExhausted(
                "capture work arithmetic overflow"
            ))
        );
        assert!(!events.iter().any(|e| e.0 == "fallback"));
    }
}

#[test]
fn guarded_native_overwire_retained_graph_misses_before_raw_allocation() {
    let base = native_capture(app_length_tests::fixture::minimal());
    for target in [MAX_WIRE_BYTES - 1, MAX_WIRE_BYTES, MAX_WIRE_BYTES + 1] {
        let mut c = base.clone();
        c.program = app_length_tests::wire_app(target);
        for raw in [b"{".to_vec(), base.source_bytes.clone()] {
            c.source_bytes = raw;
            let (result, _, events) = traced(&c, None, WorkFailure::Cancelled, None);
            assert!(matches!(result, Err(ContractWorkError::Contract(_))));
            assert!(events.iter().any(|e| e.0 == "length-miss"));
            assert!(!events
                .iter()
                .any(|e| e.0 == "raw-reserve" || e.0 == "raw-native"));
            assert!(events.iter().any(|e| e.0 == "fallback"));
        }
    }
}
#[test]
fn guarded_native_adversarial_stored_types_keep_bounded_semantic_prefix() {
    fn chain(n: usize) -> Type {
        let mut t = Type::Boolean;
        for _ in 0..n {
            t = Type::List { item: Box::new(t) };
        }
        t
    }
    fn dispose(mut t: Type) {
        loop {
            match t {
                Type::List { item } | Type::Optional { item } => t = *item,
                _ => break,
            }
        }
    }
    for site in 0..4 {
        let mut c = native_capture(app_length_tests::fixture::minimal());
        let expected_source = capture_typed::parse_app(b"{", Audit::none()).unwrap_err();
        c.source_bytes = b"{".to_vec();
        match site {
            0 => c.program.state.push(StateDefinition {
                id: "late".into(),
                label: "Late".into(),
                value_type: chain(50000),
                initial: DataValue::Boolean { value: true },
            }),
            1 => c.program.outputs.push(OutputDefinition {
                format: OutputFormat::Json,
                id: "late".into(),
                label: "Late".into(),
                columns: vec![FieldDefinition {
                    id: "late".into(),
                    label: "Late".into(),
                    value_type: chain(50000),
                }],
            }),
            2 => {
                c.program.actions[0]
                    .parameters
                    .insert("late".into(), chain(50000));
            }
            _ => c.program.state.push(StateDefinition {
                id: "late".into(),
                label: "Late".into(),
                value_type: Type::List {
                    item: Box::new(Type::Boolean),
                },
                initial: DataValue::List {
                    item_type: chain(50000),
                    items: vec![],
                },
            }),
        }
        let (result, _, events) = traced(&c, None, WorkFailure::Cancelled, None);
        let owned = match site {
            0 => std::mem::replace(&mut c.program.state[0].value_type, Type::Boolean),
            1 => std::mem::replace(
                &mut c.program.outputs[0].columns[0].value_type,
                Type::Boolean,
            ),
            2 => c.program.actions[0].parameters.remove("late").unwrap(),
            _ => {
                let DataValue::List { item_type, .. } = &mut c.program.state[0].initial else {
                    panic!()
                };
                std::mem::replace(item_type, Type::Boolean)
            }
        };
        dispose(owned);
        assert_eq!(
            result.unwrap_err(),
            ContractWorkError::Contract(expected_source)
        );
        assert!(events.iter().any(|e| e.0 == "semantic"));
        assert!(!events.iter().any(|e| e.0 == "raw-shape"));
    }
}
