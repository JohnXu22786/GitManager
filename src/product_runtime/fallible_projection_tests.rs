use super::*;
use crate::product_alloc::AllocationContext;
use serde_json::json;
use std::num::NonZeroUsize;

#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;

fn owners() -> (CapturedProgram, DataSnapshot, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot) {
    let mut app: AppDefinition = serde_json::from_value(fixture::minimal()).unwrap();
    app.entities[0].constraints.push(Expr::Equal {
        left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "name".into() }),
        right: Box::new(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "Ada".into() } }),
    });
    app.state.push(StateDefinition { id: "count".into(), label: "Count".into(),
        value_type: Type::Integer, initial: DataValue::Integer { value: 3 } });
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&app).unwrap(), "project", Producer::UserAuthored, None,
    ).unwrap();
    let mut data = DataSnapshot::empty("project", &capture.program).unwrap();
    data.records.push(Record {
        entity: "row".into(), id: "r1".into(), revision: 1,
        created_program: capture.artifact.program_digest.clone(), archived: false,
        values: BTreeMap::from([("name".into(), DataValue::Text { value: "Ada".into() })]),
    });
    let mut cx = AllocationContext::new();
    let capture_value = crate::product_json::parse_json_bytes(
        &serde_json::to_vec(&capture).unwrap(), &mut cx,
    ).unwrap();
    let decoded_capture = decode_captured_program_fallible(capture_value, &mut cx).unwrap();
    let data_value = crate::product_json::parse_json_bytes(
        &serde_json::to_vec(&data).unwrap(), &mut cx,
    ).unwrap();
    let decoded_data = decode_data_snapshot_fallible(data_value, &mut cx).unwrap();
    (capture, data, decoded_capture, decoded_data)
}

fn observation_owners(view_kind: serde_json::Value) -> (
    CapturedProgram,
    DataSnapshot,
    FallibleDecodedCapturedProgram,
    FallibleDecodedDataSnapshot,
    SessionState,
    FallibleDecodedSessionState,
) {
    let mut wire = fixture::minimal();
    wire["state"] = json!([
        {"id":"control","label":"Control","value_type":{"kind":"text"},"initial":{"kind":"text","value":"on"}},
        {"id":"selection","label":"Selection","value_type":{"kind":"list","item":{"kind":"reference","entity":"row"}},"initial":{"kind":"list","item_type":{"kind":"reference","entity":"row"},"items":[{"kind":"reference","entity":"row","record":"r1"}]}}
    ]);
    wire["actions"][0]["parameters"] = json!({"name":{"kind":"text"}});
    let toolbar = json!({
        "id":"toolbar_check","label":"Toolbar check","placement":"toolbar","action":"check",
        "arguments":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}},
        "enabled":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}
    });
    let row = json!({
        "id":"row_check","label":"Row check","placement":"row","action":"check",
        "arguments":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"Ada"}}},
        "enabled":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}}
    });
    let bindings = if view_kind["kind"] == "form" {
        json!([toolbar])
    } else {
        json!([toolbar, row])
    };
    wire["views"][0]["kind"] = view_kind;
    wire["views"][0]["actions"] = bindings;
    let app: AppDefinition = serde_json::from_value(wire).unwrap();
    let capture = CapturedProgram::capture(
        &serde_json::to_vec(&app).unwrap(), "project", Producer::UserAuthored, None,
    ).unwrap();
    let mut data = DataSnapshot::empty("project", &capture.program).unwrap();
    data.records.push(Record {
        entity: "row".into(), id: "r1".into(), revision: 1,
        created_program: capture.artifact.program_digest.clone(), archived: false,
        values: BTreeMap::from([("name".into(), DataValue::Text { value: "Ada".into() })]),
    });
    let session = SessionState::initial(&capture.program).unwrap();

    let mut cx = AllocationContext::new();
    let capture_wire = serde_json::to_vec(&capture).unwrap();
    let capture_value = crate::product_json::parse_json_bytes(&capture_wire, &mut cx).unwrap();
    let decoded_capture = decode_captured_program_fallible(capture_value, &mut cx).unwrap();
    let data_wire = serde_json::to_vec(&data).unwrap();
    let data_value = crate::product_json::parse_json_bytes(&data_wire, &mut cx).unwrap();
    let decoded_data = decode_data_snapshot_fallible(data_value, &mut cx).unwrap();
    let decoded_session = decoded_capture.initial_session_with(&mut cx).unwrap();
    (capture, data, decoded_capture, decoded_data, session, decoded_session)
}

fn assert_decoded_observation_matches_legacy(
    view_kind: serde_json::Value,
    point: &str,
) {
    let (capture, data, decoded_capture, decoded_data, session, decoded_session) =
        observation_owners(view_kind);
    let legacy_state = State {
        data,
        session,
        day: 7,
        outputs: Vec::new(),
        output_participants: Vec::new(),
        output_participant_count: 0,
    };
    let mut limits = RuntimeLimits::default();
    limits.fuel = 2_000_000;
    let mut legacy_meter = Meter::new(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)));
    let legacy = view::observe(
        &ProgramSource::ordinary(&capture), &legacy_state, point, &mut legacy_meter,
    ).unwrap();
    let decoded = view::observe_decoded(
        &decoded_capture, &decoded_data, &decoded_session, 7, point, limits,
        Arc::new(AtomicBool::new(false)), &mut AllocationContext::new(),
    ).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), serde_json::to_value(legacy).unwrap());
}

#[test]
fn decoded_observer_matches_list_detail_and_form_legacy_payloads() {
    let list = json!({
        "kind":"list","entity":"row",
        "rows":{"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":10,"include_archived":false},
        "columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],
        "controls":[{"id":"control_value","label":"Control","state":"control","on_change":null}],
        "selection":{"id":"selected","state":"selection","on_change":null}
    });
    assert_decoded_observation_matches_legacy(list, "list_point");

    let detail = json!({
        "kind":"detail","entity":"row",
        "record":{"kind":"literal","value_type":{"kind":"reference","entity":"row"},"value":{"kind":"reference","entity":"row","record":"r1"}},
        "columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}]
    });
    assert_decoded_observation_matches_legacy(detail, "detail_point");

    let form = json!({
        "kind":"form","action":"check","fields":[],
        "defaults":{"name":{"kind":"text","value":"Ada"}}
    });
    assert_decoded_observation_matches_legacy(form, "form_point");
}

#[test]
fn decoded_observer_rejects_duplicate_rows_with_legacy_error_order() {
    let repeated_reference = json!({
        "kind":"literal",
        "value_type":{"kind":"list","item":{"kind":"reference","entity":"row"}},
        "value":{"kind":"list","item_type":{"kind":"reference","entity":"row"},"items":[
            {"kind":"reference","entity":"row","record":"r1"},
            {"kind":"reference","entity":"row","record":"r1"}
        ]}
    });
    let kind = json!({
        "kind":"list","entity":"row","rows":repeated_reference,
        "columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],
        "controls":[],"selection":null
    });
    let (capture, data, decoded_capture, decoded_data, session, decoded_session) = observation_owners(kind);
    let legacy_state = State { data, session, day: 7, outputs: Vec::new(), output_participants: Vec::new(), output_participant_count: 0 };
    let mut limits = RuntimeLimits::default();
    limits.fuel = 2_000_000;
    let mut legacy_meter = Meter::new(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)));
    let legacy = view::observe(&ProgramSource::ordinary(&capture), &legacy_state, "point", &mut legacy_meter).unwrap_err();
    let decoded = view::observe_decoded(
        &decoded_capture, &decoded_data, &decoded_session, 7, "point", limits,
        Arc::new(AtomicBool::new(false)), &mut AllocationContext::new(),
    ).unwrap_err();
    assert_eq!(format!("{legacy:?}"), format!("{decoded:?}"));
}

#[test]
fn decoded_observer_checks_point_before_other_preflight_errors() {
    let kind = json!({
        "kind":"list","entity":"row",
        "rows":{"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":10,"include_archived":false},
        "columns":[],"controls":[],"selection":null
    });
    let (capture, data, decoded_capture, decoded_data, session, decoded_session) = observation_owners(kind);
    let legacy_state = State { data, session, day: -1, outputs: Vec::new(), output_participants: Vec::new(), output_participant_count: 0 };
    let mut legacy_meter = Meter::new(RuntimeLimits::default(), RuntimeLimits::default().fuel, Arc::new(AtomicBool::new(false)));
    let legacy = view::observe(&ProgramSource::ordinary(&capture), &legacy_state, "INVALID", &mut legacy_meter).unwrap_err();
    let decoded = view::observe_decoded(
        &decoded_capture, &decoded_data, &decoded_session, -1, "INVALID", RuntimeLimits::default(),
        Arc::new(AtomicBool::new(false)), &mut AllocationContext::new(),
    ).unwrap_err();
    assert_eq!(format!("{legacy:?}"), format!("{decoded:?}"));
}

#[test]
fn decoded_observer_preserves_fuel_collection_and_cancellation_checks() {
    let repeated_reference = json!({
        "kind":"literal",
        "value_type":{"kind":"list","item":{"kind":"reference","entity":"row"}},
        "value":{"kind":"list","item_type":{"kind":"reference","entity":"row"},"items":[
            {"kind":"reference","entity":"row","record":"r1"},
            {"kind":"reference","entity":"row","record":"r1"}
        ]}
    });
    let kind = json!({
        "kind":"list","entity":"row","rows":repeated_reference,
        "columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],
        "controls":[],"selection":null
    });
    let (capture, data, decoded_capture, decoded_data, session, decoded_session) = observation_owners(kind);
    let legacy_state = State { data, session, day: 7, outputs: Vec::new(), output_participants: Vec::new(), output_participant_count: 0 };

    for (mut limits, cancellation) in [
        (RuntimeLimits { fuel: 1, ..RuntimeLimits::default() }, false),
        (RuntimeLimits { collection_items: 1, ..RuntimeLimits::default() }, false),
        (RuntimeLimits::default(), true),
    ] {
        limits.fuel = limits.fuel.max(1);
        let cancelled = Arc::new(AtomicBool::new(cancellation));
        let mut legacy_meter = Meter::new(limits.clone(), limits.fuel, cancelled.clone());
        let legacy = view::observe(&ProgramSource::ordinary(&capture), &legacy_state, "point", &mut legacy_meter).unwrap_err();
        let decoded = view::observe_decoded(
            &decoded_capture, &decoded_data, &decoded_session, 7, "point", limits,
            cancelled, &mut AllocationContext::new(),
        ).unwrap_err();
        assert_eq!(format!("{legacy:?}"), format!("{decoded:?}"));
    }
}

#[test]
fn decoded_observer_allocation_refusal_is_atomic_and_retryable() {
    let kind = json!({
        "kind":"list","entity":"row",
        "rows":{"kind":"query","entity":"row","binding":"item","predicate":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}},"sort":[],"limit":10,"include_archived":false},
        "columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],
        "controls":[{"id":"control_value","label":"Control","state":"control","on_change":null}],
        "selection":{"id":"selected","state":"selection","on_change":null}
    });
    let (capture, data, decoded_capture, decoded_data, session, decoded_session) = observation_owners(kind);
    let before_capture = serde_json::to_vec(&decoded_capture).unwrap();
    let before_data = serde_json::to_vec(&decoded_data).unwrap();
    let before_session = serde_json::to_vec(&decoded_session).unwrap();
    let legacy_state = State { data, session, day: 7, outputs: Vec::new(), output_participants: Vec::new(), output_participant_count: 0 };
    let mut limits = RuntimeLimits::default();
    limits.fuel = 2_000_000;
    let mut legacy_meter = Meter::new(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)));
    let expected = serde_json::to_value(
        view::observe(&ProgramSource::ordinary(&capture), &legacy_state, "point", &mut legacy_meter).unwrap(),
    ).unwrap();
    let mut successful_allocations = AllocationContext::new();
    let successful = view::observe_decoded(
        &decoded_capture, &decoded_data, &decoded_session, 7, "point", limits.clone(),
        Arc::new(AtomicBool::new(false)), &mut successful_allocations,
    ).unwrap();
    assert_eq!(serde_json::to_value(successful).unwrap(), expected);
    let growth_count = successful_allocations.growth_requests();

    let mut refusals = 0;
    for n in 1..=growth_count {
        let mut allocations = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
        match view::observe_decoded(
            &decoded_capture, &decoded_data, &decoded_session, 7, "point", limits.clone(),
            Arc::new(AtomicBool::new(false)), &mut allocations,
        ) {
            Err(AdapterError::Allocation(_)) => refusals += 1,
            Ok(_) => panic!("allocation refusal at growth {n} was not reached"),
            other => panic!("unexpected decoded observation result at growth {n}: {other:?}"),
        }
        assert_eq!(serde_json::to_vec(&decoded_capture).unwrap(), before_capture);
        assert_eq!(serde_json::to_vec(&decoded_data).unwrap(), before_data);
        assert_eq!(serde_json::to_vec(&decoded_session).unwrap(), before_session);
    }
    assert!(refusals > 0);
    let retry = view::observe_decoded(
        &decoded_capture, &decoded_data, &decoded_session, 7, "point", limits,
        Arc::new(AtomicBool::new(false)), &mut AllocationContext::new(),
    ).unwrap();
    assert_eq!(serde_json::to_value(retry).unwrap(), expected);
    assert_eq!(serde_json::to_vec(&decoded_capture).unwrap(), before_capture);
    assert_eq!(serde_json::to_vec(&decoded_data).unwrap(), before_data);
    assert_eq!(serde_json::to_vec(&decoded_session).unwrap(), before_session);
}

#[test]
fn fallible_data_and_session_initialization_match_legacy() {
    let (capture, _, decoded_capture, _) = owners();
    let mut cx = AllocationContext::new();
    let data = FallibleDecodedDataSnapshot::empty_with("project", decoded_capture.app(), &mut cx).unwrap();
    let legacy_data = DataSnapshot::empty("project", &capture.program).unwrap();
    assert_eq!(data.canonical_bytes_with(&mut cx).unwrap(), canonical_bytes(&legacy_data).unwrap());
    let session = decoded_capture.initial_session_with(&mut cx).unwrap();
    let legacy_session = SessionState::initial(&capture.program).unwrap();
    assert_eq!(canonical_bytes(&session).unwrap(), canonical_bytes(&legacy_session).unwrap());
}

#[test]
fn empty_and_merge_reject_structurally_decoded_invalid_app_before_copy() {
    let (capture, _, _, data) = owners();
    let mut wire = serde_json::to_value(&capture).unwrap();
    let entities = wire["program"]["entities"].as_array_mut().unwrap();
    entities.push(entities[0].clone());
    let bytes = serde_json::to_vec(&wire).unwrap();
    let mut parse_allocations = AllocationContext::new();
    let value = crate::product_json::parse_json_bytes(&bytes, &mut parse_allocations).unwrap();
    let invalid_capture = decode_captured_program_fallible(value, &mut parse_allocations).unwrap();

    let mut empty_allocations = AllocationContext::new();
    assert!(matches!(
        FallibleDecodedDataSnapshot::empty_with("project", invalid_capture.app(), &mut empty_allocations),
        Err(AppValidationError::Contract(_)),
    ));

    let before = data.canonical_bytes_with(&mut AllocationContext::new()).unwrap();
    let mut merge_allocations = AllocationContext::new();
    assert!(matches!(
        data.merge_app_with(invalid_capture.app(), &mut merge_allocations),
        Err(AppValidationError::Contract(_)),
    ));
    assert_eq!(data.canonical_bytes_with(&mut AllocationContext::new()).unwrap(), before);
}

#[test]
fn decoded_constraint_replay_shares_eval_and_propagates_allocations() {
    let (capture, data, decoded_capture, decoded_data) = owners();
    let mut cx = AllocationContext::new();
    let session = decoded_capture.initial_session_with(&mut cx).unwrap();
    let before = canonical_bytes(&data).unwrap();
    let mut successful = false;
    let mut refusals = 0;
    for n in 1..512 {
        let limits = RuntimeLimits { fuel: 10_000_000, ..RuntimeLimits::default() };
        let meter = Meter::with_allocations(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)),
            AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false));
        let mut meter = meter;
        match execute::constraints_decoded(&decoded_capture, &decoded_data, &session, 0, &mut meter) {
            Ok(()) => { successful = true; break; }
            Err(AdapterError::Allocation(_)) => refusals += 1,
            other => panic!("unexpected decoded constraint replay result at growth {n}: {other:?}"),
        }
        assert_eq!(canonical_bytes(&data).unwrap(), before);
    }
    assert!(successful);
    assert!(refusals > 0);
    let limits = RuntimeLimits { fuel: 10_000_000, ..RuntimeLimits::default() };
    let mut meter = Meter::with_allocations(limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)), AllocationContext::new());
    execute::constraints_decoded(&decoded_capture, &decoded_data, &session, 0, &mut meter).unwrap();
    assert_eq!(canonical_bytes(&data).unwrap(), before);
    let _ = capture;
}

#[test]
fn decoded_projection_uses_shared_eval_and_matches_legacy() {
    let (capture, data, decoded_capture, decoded_data) = owners();
    let expression: Expr = serde_json::from_value(json!({
        "kind":"field", "record":{"kind":"variable","name":"item"}, "field":"name"
    })).unwrap();
    let row = RecordRef { entity: "row".into(), record: "r1".into() };
    let expected = Type::Text;
    let runtime = LocalRuntime::default();
    let legacy = runtime.evaluate_record_projection(
        &capture, &data, &row, "item", &expression, &expected, 0,
    ).unwrap();
    let decoded = runtime.evaluate_decoded_record_projection_with(
        &decoded_capture, &decoded_data, &row, "item", &expression, &expected, 0,
        RuntimeLimits::default(), AllocationContext::new(),
    ).unwrap();
    assert_eq!(decoded, legacy);
    assert_eq!(decoded, DataValue::Text { value: "Ada".into() });
}

#[test]
fn decoded_lowercase_refusal_preserves_typed_allocation_error_and_retry() {
    let (capture, _, decoded_capture, decoded_data) = owners();
    let expression: Expr = serde_json::from_value(json!({
        "kind":"lower",
        "value":{"kind":"literal", "value_type":{"kind":"text"}, "value":{"kind":"text","value":"Ada"}}
    })).unwrap();
    let legacy_session = SessionState::initial(&capture.program).unwrap();
    let mut legacy_meter = Meter::new(RuntimeLimits::default(), RuntimeLimits::default().fuel,
        Arc::new(AtomicBool::new(false)));
    let legacy = Eval::legacy(&capture.program, &DataSnapshot::empty("project", &capture.program).unwrap(),
        &legacy_session, 0, &mut legacy_meter).eval(&expression, &Env::new()).unwrap();
    assert_eq!(legacy, DataValue::Text { value: "ada".into() });

    let mut setup = AllocationContext::new();
    let session = decoded_capture.initial_session_with(&mut setup).unwrap();
    let limits = RuntimeLimits::default();
    let mut refused_meter = Meter::with_allocations(
        limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)),
        AllocationContext::fail_nth_growth(NonZeroUsize::new(2).unwrap(), true),
    );
    let mut evaluator = Eval::fallible(decoded_capture.app(), &decoded_data, &session, 0, &mut refused_meter);
    assert!(matches!(evaluator.eval(&expression, &Env::new()), Err(AdapterError::Allocation(_))));

    let mut retry_meter = Meter::with_allocations(
        limits.clone(), limits.fuel, Arc::new(AtomicBool::new(false)), AllocationContext::new(),
    );
    let mut evaluator = Eval::fallible(decoded_capture.app(), &decoded_data, &session, 0, &mut retry_meter);
    assert_eq!(evaluator.eval(&expression, &Env::new()).unwrap(), legacy);
}

#[test]
fn decoded_historical_projection_uses_borrowed_override_and_matches_legacy() {
    let (capture, data, decoded_capture, decoded_data) = owners();
    let expression: Expr = serde_json::from_value(json!({
        "kind":"field", "record":{"kind":"variable","name":"item"}, "field":"name"
    })).unwrap();
    let row = RecordRef { entity: "row".into(), record: "r1".into() };
    let expected = Type::Text;
    let legacy_values = BTreeMap::from([("name".into(), DataValue::Text { value: "Grace".into() })]);
    let runtime = LocalRuntime::default();
    let legacy = runtime.evaluate_historical_record_projection(
        &capture, &data, &row, &legacy_values, true, "item", &expression, &expected, 0,
    ).unwrap();
    let mut cx = AllocationContext::new();
    let values = FallibleValues::try_from_iter(
        [Ok(("name".to_owned(), DataValue::Text { value: "Grace".into() }))], &mut cx,
    ).unwrap();
    let decoded = runtime.evaluate_decoded_historical_record_projection_with(
        &decoded_capture, &decoded_data, &row, &values, true, "item", &expression, &expected, 0,
        RuntimeLimits::default(), AllocationContext::new(),
    ).unwrap();
    assert_eq!(decoded, legacy);
    assert_eq!(decoded, DataValue::Text { value: "Grace".into() });
}

#[test]
fn decoded_historical_lookup_charges_full_identifier_comparison_bytes() {
    let (_, mut data, decoded_capture, _) = owners();
    let long_record = "r".repeat(128);
    data.records[0].id = long_record.clone();
    let mut parse_allocations = AllocationContext::new();
    let data_value = crate::product_json::parse_json_bytes(
        &serde_json::to_vec(&data).unwrap(), &mut parse_allocations,
    ).unwrap();
    let decoded_data = decode_data_snapshot_fallible(data_value, &mut parse_allocations).unwrap();
    let row = RecordRef { entity: "row".into(), record: long_record };
    let expression: Expr = serde_json::from_value(json!({
        "kind":"field", "record":{"kind":"variable","name":"item"}, "field":"name"
    })).unwrap();
    let mut value_allocations = AllocationContext::new();
    let values = FallibleValues::try_from_iter(
        [Ok(("name".to_owned(), DataValue::Text { value: "Grace".into() }))],
        &mut value_allocations,
    ).unwrap();
    let runtime = LocalRuntime::default();
    let too_small = RuntimeLimits { fuel: 261, ..RuntimeLimits::default() };
    assert!(matches!(runtime.evaluate_decoded_historical_record_projection_with(
        &decoded_capture, &decoded_data, &row, &values, false, "item", &expression,
        &Type::Text, 0, too_small, AllocationContext::new(),
    ), Err(AdapterError::BudgetExhausted(_))));

    let enough = RuntimeLimits { fuel: 1000, ..RuntimeLimits::default() };
    assert_eq!(runtime.evaluate_decoded_historical_record_projection_with(
        &decoded_capture, &decoded_data, &row, &values, false, "item", &expression,
        &Type::Text, 0, enough, AllocationContext::new(),
    ).unwrap(), DataValue::Text { value: "Grace".into() });
}

#[test]
fn fallible_merge_matches_legacy_and_refusal_keeps_sources_retryable() {
    let old_app: AppDefinition = serde_json::from_value(fixture::minimal()).unwrap();
    let old_capture = CapturedProgram::capture(
        &serde_json::to_vec(&old_app).unwrap(), "project", Producer::UserAuthored, None,
    ).unwrap();
    let mut source_data = DataSnapshot::empty("project", &old_capture.program).unwrap();
    source_data.records.push(Record {
        entity: "row".into(), id: "r1".into(), revision: 1,
        created_program: old_capture.artifact.program_digest.clone(), archived: false,
        values: BTreeMap::from([("name".into(), DataValue::Text { value: "Ada".into() })]),
    });
    let mut candidate_app = old_app.clone();
    candidate_app.entities[0].label = "People".into();
    candidate_app.entities[0].fields.push(FieldDefinition {
        id: "nickname".into(), label: "Nickname".into(),
        value_type: Type::Optional { item: Box::new(Type::Text) },
    });
    let candidate = CapturedProgram::capture(
        &serde_json::to_vec(&candidate_app).unwrap(), "project", Producer::UserAuthored, None,
    ).unwrap();
    let legacy = compatibility::merge(&candidate.program, &source_data).unwrap();
    let mut cx = AllocationContext::new();
    let decode = |bytes: &[u8], cx: &mut AllocationContext| {
        let value = crate::product_json::parse_json_bytes(bytes, cx).unwrap();
        decode_data_snapshot_fallible(value, cx).unwrap()
    };
    let decoded_data = decode(&serde_json::to_vec(&source_data).unwrap(), &mut cx);
    let candidate_value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&candidate).unwrap(), &mut cx).unwrap();
    let decoded_candidate = decode_captured_program_fallible(candidate_value, &mut cx).unwrap();
    let before_data = canonical_bytes(&source_data).unwrap();
    let before_candidate = serde_json::to_vec(&candidate).unwrap();
    let merged = decoded_data.merge_app_with(decoded_candidate.app(), &mut cx).unwrap();
    let merged_bytes = merged.canonical_bytes_with(&mut cx).unwrap();
    assert_eq!(merged_bytes, canonical_bytes(&legacy).unwrap());

    let mut refusals = 0;
    let mut complete = false;
    for n in 1..4096 {
        let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
        match decoded_data.merge_app_with(decoded_candidate.app(), &mut cx) {
            Err(AppValidationError::Allocation(_)) => refusals += 1,
            Ok(value) => {
                assert_eq!(value.canonical_bytes_with(&mut AllocationContext::new()).unwrap(), merged_bytes);
                complete = true;
                break;
            }
            other => panic!("unexpected fallible merge result at growth {n}: {other:?}"),
        }
        assert_eq!(canonical_bytes(&source_data).unwrap(), before_data);
        assert_eq!(serde_json::to_vec(&candidate).unwrap(), before_candidate);
    }
    assert!(complete, "bounded refusal sweep did not pass all merge growth sites");
    assert!(refusals > 0);
    assert_eq!(canonical_bytes(&source_data).unwrap(), before_data);
    assert_eq!(serde_json::to_vec(&candidate).unwrap(), before_candidate);
}

#[test]
fn decoded_compatibility_state_matches_legacy_and_refusal_is_typed() {
    let (capture, data, decoded_capture, decoded_data) = owners();
    let runtime = LocalRuntime::default();
    let expected = runtime.compatibility_at(&capture, &data, 0).unwrap().state;
    assert_eq!(runtime.compatibility_decoded_state_at(
        &decoded_capture, &decoded_data, 0, AllocationContext::new(),
    ).unwrap(), expected);
    let before = canonical_bytes(&data).unwrap();
    let mut refusals = 0;
    let mut complete = false;
    for n in 1..4096 {
        let cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
        match runtime.compatibility_decoded_state_at(&decoded_capture, &decoded_data, 0, cx) {
            Err(AdapterError::Allocation(_)) => refusals += 1,
            Ok(state) => { assert_eq!(state, expected); complete = true; break; }
            other => panic!("unexpected fallible compatibility result at growth {n}: {other:?}"),
        }
        assert_eq!(canonical_bytes(&data).unwrap(), before);
    }
    assert!(complete, "bounded refusal sweep did not pass all compatibility growth sites");
    assert!(refusals > 0);
    assert_eq!(canonical_bytes(&data).unwrap(), before);
}

#[test]
fn decoded_projection_allocation_refusal_is_typed_and_retryable() {
    let (capture, data, decoded_capture, decoded_data) = owners();
    let expression: Expr = serde_json::from_value(json!({
        "kind":"field", "record":{"kind":"variable","name":"item"}, "field":"name"
    })).unwrap();
    let row = RecordRef { entity: "row".into(), record: "r1".into() };
    let expected = Type::Text;
    let before_capture = serde_json::to_vec(&capture).unwrap();
    let before_data = serde_json::to_vec(&data).unwrap();
    let runtime = LocalRuntime::default();
    let mut refusals = 0;
    let mut reached_success = false;
    for n in 1..4096 {
        let cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
        match runtime.evaluate_decoded_record_projection_with(
            &decoded_capture, &decoded_data, &row, "item", &expression, &expected, 0,
            RuntimeLimits::default(), cx,
        ) {
            Err(AdapterError::Allocation(_)) => refusals += 1,
            Ok(value) => {
                assert_eq!(value, DataValue::Text { value: "Ada".into() });
                reached_success = true;
                break;
            }
            other => panic!("unexpected decoded projection result at growth {n}: {other:?}"),
        }
        assert_eq!(serde_json::to_vec(&capture).unwrap(), before_capture);
        assert_eq!(serde_json::to_vec(&data).unwrap(), before_data);
        let retry = runtime.evaluate_decoded_record_projection_with(
            &decoded_capture, &decoded_data, &row, "item", &expression, &expected, 0,
            RuntimeLimits::default(), AllocationContext::new(),
        ).unwrap();
        assert_eq!(retry, DataValue::Text { value: "Ada".into() });
    }
    assert!(reached_success, "bounded refusal sweep did not pass all growth sites");
    assert!(refusals > 0);
    assert_eq!(serde_json::to_vec(&capture).unwrap(), before_capture);
    assert_eq!(serde_json::to_vec(&data).unwrap(), before_data);
}
