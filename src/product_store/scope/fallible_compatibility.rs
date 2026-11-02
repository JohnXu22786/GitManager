//! Allocation-aware compatibility report reconstruction for decoded owners.
use super::{DecodedCompatibilityReport, DecodedSemanticKey, StoreError};
use crate::product_alloc::{self, AllocationContext, AllocationFailure, AllocationSite, FormatFailure};
use crate::product_contract::{
    AdapterError, AppValidationError, CompatibilityState, DataRecordView, DataSnapshotView,
    Digest, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot,
    MAX_ITEMS, Type,
};
use crate::product_runtime::LocalRuntime;

#[derive(Debug)]
pub(crate) enum CompatibilityFailure {
    Store(StoreError),
    Allocation(AllocationFailure),
}
impl From<AllocationFailure> for CompatibilityFailure {
    fn from(value: AllocationFailure) -> Self { Self::Allocation(value) }
}
impl From<StoreError> for CompatibilityFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}

fn app_failure(error: AppValidationError) -> CompatibilityFailure {
    match error {
        AppValidationError::Contract(error) => CompatibilityFailure::Store(StoreError::Runtime(AdapterError::Invalid(error))),
        AppValidationError::Allocation(failure) => CompatibilityFailure::Allocation(failure),
    }
}
fn runtime_failure(error: AdapterError) -> CompatibilityFailure {
    match error {
        AdapterError::Allocation(failure) => CompatibilityFailure::Allocation(failure),
        error => CompatibilityFailure::Store(StoreError::Runtime(error)),
    }
}
fn format_issue(
    cx: &mut AllocationContext,
    args: std::fmt::Arguments<'_>,
) -> Result<String, CompatibilityFailure> {
    let mut output = String::new();
    product_alloc::try_append_fmt(&mut output, args, cx, AllocationSite::String).map_err(|error| match error {
        FormatFailure::Allocation(failure) => CompatibilityFailure::Allocation(failure),
        FormatFailure::Formatter => CompatibilityFailure::Store(StoreError::Runtime(AdapterError::Invalid(
            crate::product_contract::ContractError("compatibility diagnostic formatting failed".into()),
        ))),
    })?;
    Ok(output)
}
fn push_issue(
    issues: &mut Vec<String>,
    issue: String,
    cx: &mut AllocationContext,
) -> Result<(), CompatibilityFailure> {
    if issues.len() == MAX_ITEMS { return Ok(()); }
    product_alloc::try_push(issues, issue, cx, AllocationSite::Vector)?;
    Ok(())
}
fn copy_digest(value: &Digest, cx: &mut AllocationContext) -> Result<Digest, CompatibilityFailure> {
    let value = product_alloc::try_copy_str(value.as_str(), cx, AllocationSite::String)?;
    Ok(Digest::from_owned_lower_hex(value).expect("validated digest remains valid"))
}
fn copy_record(
    row: DataRecordView<'_>,
    cx: &mut AllocationContext,
) -> Result<crate::product_contract::RecordRef, CompatibilityFailure> {
    Ok(crate::product_contract::RecordRef {
        entity: product_alloc::try_copy_str(row.entity(), cx, AllocationSite::String)?,
        record: product_alloc::try_copy_str(row.id(), cx, AllocationSite::String)?,
    })
}

/// Recompute the persisted compatibility report directly over decoded owners.
/// Semantic ordering follows `product_runtime::compatibility::check`; every
/// owned report element and diagnostic uses the caller's bounded allocator.
pub(crate) fn report(
    program: &FallibleDecodedCapturedProgram,
    current: &FallibleDecodedDataSnapshot,
    day: i32,
    cx: &mut AllocationContext,
) -> Result<DecodedCompatibilityReport, CompatibilityFailure> {
    crate::product_contract::validate_day(day)
        .map_err(|error| StoreError::Runtime(AdapterError::Invalid(error)))?;
    program.validate_with(cx).map_err(app_failure)?;
    current.validate_with(cx).map_err(app_failure)?;
    report_after_input_validation(program, current, day, cx)
}

fn report_after_input_validation(
    program: &FallibleDecodedCapturedProgram,
    current: &FallibleDecodedDataSnapshot,
    day: i32,
    cx: &mut AllocationContext,
) -> Result<DecodedCompatibilityReport, CompatibilityFailure> {
    if program.project_id() != current.project_id() {
        return Err(StoreError::Runtime(AdapterError::Invalid(
            crate::product_contract::ContractError("compatibility project mismatch".into()),
        )).into());
    }
    let current_data = current.identity_with(cx).map_err(app_failure)?;
    let target_program = copy_digest(program.program_digest(), cx)?;
    let view = DataSnapshotView::from(current);
    let mut retained_records = Vec::new();
    product_alloc::try_reserve(&mut retained_records, view.record_count(), cx, AllocationSite::Vector)?;
    for row in view.records() {
        product_alloc::try_push(&mut retained_records, copy_record(row, cx)?, cx, AllocationSite::Vector)?;
    }
    let mut retained_events = Vec::new();
    product_alloc::try_reserve(&mut retained_events, current.events().len(), cx, AllocationSite::Vector)?;
    for event in current.events() {
        let id = product_alloc::try_copy_str(event.id(), cx, AllocationSite::String)?;
        product_alloc::try_push(&mut retained_events, id, cx, AllocationSite::Vector)?;
    }
    let field_count = current.schema().iter().map(|entity| entity.fields.len()).sum();
    let mut retained_fields = Vec::new();
    product_alloc::try_reserve(&mut retained_fields, field_count, cx, AllocationSite::Vector)?;
    for entity in current.schema() {
        for field in &entity.fields {
            product_alloc::try_push(
                &mut retained_fields,
                DecodedSemanticKey::field(&entity.id, &field.id, cx)?,
                cx,
                AllocationSite::Vector,
            )?;
        }
    }

    let app = crate::product_contract::AppRef::from(program.app());
    let mut state = CompatibilityState::Compatible;
    let mut issues = Vec::new();
    for stored in current.schema() {
        if let Some(target) = app.entities().iter().find(|entity| entity.id == stored.id) {
            for field in &stored.fields {
                if !target.fields.iter().any(|candidate| candidate.id == field.id)
                    && !matches!(&field.value_type, Type::Optional { .. })
                {
                    let issue = format_issue(cx, format_args!(
                        "Required stored field {}.{} is missing from the target; keep it in a compatible forward repair",
                        stored.id, field.id,
                    ))?;
                    push_issue(&mut issues, issue, cx)?;
                }
            }
        } else if current.records().iter().any(|row| row.entity() == stored.id) {
            state = CompatibilityState::RetainedButUnavailable;
            let issue = format_issue(cx, format_args!(
                "Entity {} remains inspectable but the target has no workflow for its current records",
                stored.id,
            ))?;
            push_issue(&mut issues, issue, cx)?;
        }
    }

    match current.merge_app_with(program.app(), cx) {
        Ok(merged) => {
            // The day is explicit here, so clock-sensitive constraints are
            // evaluated at that day instead of producing an Unknown report.
            match LocalRuntime::default().check_decoded_compatibility_constraints(program, &merged, day, cx)
                .map_err(runtime_failure)?
            {
                Ok(()) => {}
                Err(error) => {
                    let issue = format_issue(cx, format_args!("Current records violate target constraints: {error:?}"))?;
                    push_issue(&mut issues, issue, cx)?;
                    state = CompatibilityState::Incompatible;
                }
            }
        }
        Err(AppValidationError::Allocation(failure)) => return Err(failure.into()),
        Err(AppValidationError::Contract(error)) => {
            let error = AdapterError::Invalid(error);
            let issue = format_issue(cx, format_args!("Current schema/data cannot be represented safely: {error:?}"))?;
            push_issue(&mut issues, issue, cx)?;
            state = CompatibilityState::Incompatible;
        }
    }
    if state == CompatibilityState::Compatible && !issues.is_empty() {
        state = CompatibilityState::Incompatible;
    }
    let report = DecodedCompatibilityReport::new(
        state, current_data, target_program, retained_records, retained_events, retained_fields, issues,
    );
    report.validate_with(cx).map_err(|error| match error {
        crate::product_contract::DecisionValidationError::Contract(error) => {
            CompatibilityFailure::Store(StoreError::Runtime(AdapterError::Invalid(error)))
        }
        crate::product_contract::DecisionValidationError::Allocation(failure) => {
            CompatibilityFailure::Allocation(failure)
        }
    })?;
    Ok(report)
}

/// Exact decoded counterpart of the scope's ordinary initial-runtime check.
/// It preserves the runtime's validation and compatibility error order while
/// keeping allocation refusal distinct from semantic incompatibility.
pub(crate) fn validate_initial_runtime(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    session: &crate::product_contract::FallibleDecodedSessionState,
    day: i32,
    limits: crate::product_contract::RuntimeLimits,
    cx: &mut AllocationContext,
) -> Result<(), CompatibilityFailure> {
    // This is the order in LocalRuntime::start_source before compatibility.
    program.validate_with(cx).map_err(app_failure)?;
    data.validate_with(cx).map_err(app_failure)?;
    program.validate_session_with(session, cx).map_err(app_failure)?;
    crate::product_contract::validate_day(day).map_err(|error| StoreError::Runtime(AdapterError::Invalid(error)))?;
    limits.validate().map_err(|error| StoreError::Runtime(AdapterError::Invalid(error)))?;
    if data.project_id() != program.project_id() {
        return Err(StoreError::Runtime(AdapterError::Invalid(crate::product_contract::ContractError(
            "program and data belong to different projects".into(),
        ))).into());
    }
    // `LocalRuntime::start_source` runs program/data validation once directly,
    // then `compatibility::check` repeats those checks before building the report.
    program.validate_with(cx).map_err(app_failure)?;
    data.validate_with(cx).map_err(app_failure)?;
    let report = report_after_input_validation(program, data, day, cx)?;
    LocalRuntime::default()
        .validate_decoded_initial_runtime_after_report(
            program, data, session, day, limits, report.state(), report.issues(), cx,
        )
        .map_err(runtime_failure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::{
        canonical_bytes, decode_captured_program_fallible, decode_data_snapshot_fallible,
        AppDefinition, CapturedProgram, DataSnapshot, DataValue, EntityDefinition,
        Expr, FieldDefinition, Producer, Record, RuntimeLimits, SessionState,
    };
    use std::num::NonZeroUsize;

    fn minimal_app() -> serde_json::Value {
        let yes = serde_json::json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}});
        serde_json::json!({
            "version":1,"id":"source_fixture","label":"Source fixture",
            "entities":[{"id":"row","label":"Rows","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}],
            "state":[],"actions":[{"id":"check","label":"Check","parameters":{},"guards":[],"steps":[{"kind":"assert","condition":yes,"message":"yes"}],"ensures":[]}],
            "outputs":[],"observables":[{"id":"yes","label":"Yes","value":yes}],
            "views":[{"id":"rows","label":"Rows","kind":{"kind":"list","entity":"row","rows":{"kind":"query","entity":"row","binding":"item","predicate":yes,"sort":[],"limit":1,"include_archived":false},"columns":[{"id":"name","label":"Name","value":{"kind":"field","record":{"kind":"variable","name":"row"},"field":"name"}}],"controls":[],"selection":null},"actions":[],"keys":[]}],
            "initial_view":"rows"
        })
    }

    fn owners() -> (CapturedProgram, DataSnapshot, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot) {
        let app: AppDefinition = serde_json::from_value(minimal_app()).unwrap();
        let capture = CapturedProgram::capture(
            &serde_json::to_vec(&app).unwrap(), "project", Producer::UserAuthored, None,
        ).unwrap();
        let mut data = DataSnapshot::empty("project", &capture.program).unwrap();
        data.records.push(Record {
            entity: "row".into(), id: "r1".into(), revision: 1,
            created_program: capture.artifact.program_digest.clone(), archived: false,
            values: [ ("name".into(), DataValue::Text { value: "Ada".into() }) ].into(),
        });
        decode(&capture, &data)
    }

    fn decode(
        capture: &CapturedProgram,
        data: &DataSnapshot,
    ) -> (CapturedProgram, DataSnapshot, FallibleDecodedCapturedProgram, FallibleDecodedDataSnapshot) {
        let mut cx = AllocationContext::new();
        let capture_value = crate::product_json::parse_json_bytes(&serde_json::to_vec(capture).unwrap(), &mut cx).unwrap();
        let decoded_capture = decode_captured_program_fallible(capture_value, &mut cx).unwrap();
        let data_value = crate::product_json::parse_json_bytes(&serde_json::to_vec(data).unwrap(), &mut cx).unwrap();
        let decoded_data = decode_data_snapshot_fallible(data_value, &mut cx).unwrap();
        (capture.clone(), data.clone(), decoded_capture, decoded_data)
    }

    fn assert_report_parity(capture: &CapturedProgram, data: &DataSnapshot, day: i32) {
        let (_, _, decoded_capture, decoded_data) = decode(capture, data);
        let expected = crate::product_runtime::LocalRuntime::default().compatibility_at(capture, data, day).unwrap();
        let actual = report(&decoded_capture, &decoded_data, day, &mut AllocationContext::new()).unwrap();
        assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
        assert_eq!(actual.state(), expected.state);
    }

    #[test]
    fn decoded_compatibility_report_matches_full_legacy_report_variants() {
        let (capture, data, _, _) = owners();
        assert_report_parity(&capture, &data, 0);

        // A current required field absent from the captured app produces the
        // legacy incompatibility issue while remaining valid persisted data.
        let mut missing = data.clone();
        missing.schema[0].fields.push(FieldDefinition {
            id: "required".into(), label: "Required".into(), value_type: Type::Text,
        });
        missing.records[0].values.insert("required".into(), DataValue::Text { value: "x".into() });
        assert_report_parity(&capture, &missing, 0);

        // An entity retained with records but absent from the target remains
        // inspectable and is reported in the exact legacy order.
        let mut retained = data.clone();
        retained.schema.push(EntityDefinition {
            id: "legacy".into(), label: "Legacy".into(),
            fields: vec![FieldDefinition { id: "flag".into(), label: "Flag".into(), value_type: Type::Boolean }],
            unique: vec![], constraints: vec![],
        });
        retained.records.push(Record {
            entity: "legacy".into(), id: "old".into(), revision: 1,
            created_program: capture.artifact.program_digest.clone(), archived: false,
            values: [ ("flag".into(), DataValue::Boolean { value: true }) ].into(),
        });
        assert_report_parity(&capture, &retained, 0);

        // Schema type incompatibility follows the legacy merge-error branch.
        let mut changed_app: AppDefinition = serde_json::from_value(minimal_app()).unwrap();
        changed_app.entities[0].fields[0].value_type = Type::Integer;
        let changed = CapturedProgram::capture(
            &serde_json::to_vec(&changed_app).unwrap(), "project", Producer::UserAuthored, None,
        ).unwrap();
        assert_report_parity(&changed, &data, 0);

        // Constraint failures and explicit clock-sensitive constraints are
        // evaluated by the shared decoded evaluator at the requested day.
        let mut constrained: AppDefinition = serde_json::from_value(minimal_app()).unwrap();
        constrained.entities[0].constraints.push(Expr::Equal {
            left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "name".into() }),
            right: Box::new(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "Ada".into() } }),
        });
        let constrained = CapturedProgram::capture(
            &serde_json::to_vec(&constrained).unwrap(), "project", Producer::UserAuthored, None,
        ).unwrap();
        assert_report_parity(&constrained, &data, 0);
        let mut wrong_data = data.clone();
        wrong_data.records[0].values.insert("name".into(), DataValue::Text { value: "Grace".into() });
        assert_report_parity(&constrained, &wrong_data, 0);

        let mut clocked: AppDefinition = serde_json::from_value(minimal_app()).unwrap();
        clocked.entities[0].constraints.push(Expr::Equal {
            left: Box::new(Expr::Today),
            right: Box::new(Expr::Literal { value_type: Type::Date, value: DataValue::Date { days: 9 } }),
        });
        let clocked = CapturedProgram::capture(
            &serde_json::to_vec(&clocked).unwrap(), "project", Producer::UserAuthored, None,
        ).unwrap();
        assert_report_parity(&clocked, &data, 9);
        assert_report_parity(&clocked, &data, 8);
    }

    #[test]
    fn decoded_compatibility_report_refuses_allocation_and_retries() {
        let (capture, data, decoded_capture, decoded_data) = owners();
        let expected = crate::product_runtime::LocalRuntime::default().compatibility_at(&capture, &data, 0).unwrap();
        let before_data = canonical_bytes(&data).unwrap();
        let mut refusals = 0;
        let mut completed = false;
        for n in 1..4096 {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
            match report(&decoded_capture, &decoded_data, 0, &mut cx) {
                Ok(actual) => {
                    assert_eq!(canonical_bytes(&expected).unwrap(), canonical_bytes(&actual).unwrap());
                    completed = true;
                    break;
                }
                Err(CompatibilityFailure::Allocation(_)) => refusals += 1,
                other => panic!("unexpected compatibility result at growth {n}: {other:?}"),
            }
            assert_eq!(canonical_bytes(&data).unwrap(), before_data);
        }
        assert!(completed, "bounded compatibility refusal sweep did not complete");
        assert!(refusals > 0);
        assert_eq!(canonical_bytes(&data).unwrap(), before_data);
    }

    #[test]
    fn decoded_compatibility_report_preserves_invalid_day_and_inventory_errors() {
        let (capture, data, decoded_capture, decoded_data) = owners();
        let runtime = crate::product_runtime::LocalRuntime::default();
        let legacy_day = runtime.compatibility_at(&capture, &data, i32::MAX).unwrap_err();
        let decoded_day = report(&decoded_capture, &decoded_data, i32::MAX, &mut AllocationContext::new()).unwrap_err();
        match decoded_day {
            CompatibilityFailure::Store(StoreError::Runtime(error)) => {
                assert_eq!(format!("{error:?}"), format!("{legacy_day:?}"));
            }
            other => panic!("unexpected invalid-day classification: {other:?}"),
        }

        // Each entity is individually valid, but the aggregate retained-field
        // inventory exceeds the report's shared MAX_COLLECTION limit.
        let mut many_fields = data.clone();
        for entity_index in 0..40 {
            let mut fields = Vec::new();
            for field_index in 0..256 {
                fields.push(FieldDefinition {
                    id: format!("f{field_index}"),
                    label: format!("Field {field_index}"),
                    value_type: Type::Text,
                });
            }
            many_fields.schema.push(EntityDefinition {
                id: format!("entity{entity_index}"),
                label: format!("Entity {entity_index}"),
                fields,
                unique: Vec::new(),
                constraints: Vec::new(),
            });
        }
        many_fields.validate().unwrap();
        let legacy = runtime.compatibility_at(&capture, &many_fields, 0).unwrap_err();
        let (_, _, decoded_capture, decoded_data) = decode(&capture, &many_fields);
        let decoded = report(&decoded_capture, &decoded_data, 0, &mut AllocationContext::new()).unwrap_err();
        match decoded {
            CompatibilityFailure::Store(StoreError::Runtime(error)) => {
                assert_eq!(format!("{error:?}"), format!("{legacy:?}"));
            }
            other => panic!("unexpected aggregate-inventory classification: {other:?}"),
        }
    }

    #[test]
    fn decoded_initial_runtime_matches_legacy_start_and_propagates_refusal() {
        let (capture, data, decoded_capture, decoded_data) = owners();
        let mut cx = AllocationContext::new();
        let session = decoded_capture.initial_session_with(&mut cx).unwrap();
        let legacy_session = SessionState::initial(&capture.program).unwrap();
        assert!(crate::product_contract::RuntimeAdapter::start(
            &crate::product_runtime::LocalRuntime::default(), &capture, &data, &legacy_session, 0, 7, RuntimeLimits::default(),
        ).is_ok());
        validate_initial_runtime(&decoded_capture, &decoded_data, &session, 0, RuntimeLimits::default(), &mut AllocationContext::new()).unwrap();

        let before_data = canonical_bytes(&data).unwrap();
        let mut refused = 0;
        let mut completed = false;
        for n in 1..4096 {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(n).unwrap(), false);
            match validate_initial_runtime(&decoded_capture, &decoded_data, &session, 0, RuntimeLimits::default(), &mut cx) {
                Ok(()) => { completed = true; break; }
                Err(CompatibilityFailure::Allocation(_)) => refused += 1,
                other => panic!("unexpected initial runtime result at growth {n}: {other:?}"),
            }
            assert_eq!(canonical_bytes(&data).unwrap(), before_data);
        }
        assert!(completed, "bounded initial runtime refusal sweep did not complete");
        assert!(refused > 0);
        assert_eq!(canonical_bytes(&data).unwrap(), before_data);

        // A malformed focus must preserve the ordinary start failure class.
        let mut bad_session_value = serde_json::to_value(&legacy_session).unwrap();
        bad_session_value["focused_record"] = serde_json::json!({"entity":"row","record":"missing"});
        let bytes = serde_json::to_vec(&bad_session_value).unwrap();
        let mut parse_cx = AllocationContext::new();
        let session_value = crate::product_json::parse_json_bytes(&bytes, &mut parse_cx).unwrap();
        let bad_session = crate::product_contract::decode_session_state_fallible(session_value, &mut parse_cx).unwrap();
        let legacy_bad: SessionState = serde_json::from_value(bad_session_value).unwrap();
        let legacy = crate::product_contract::RuntimeAdapter::start(
            &crate::product_runtime::LocalRuntime::default(), &capture, &data, &legacy_bad, 0, 7, RuntimeLimits::default(),
        ).err().unwrap();
        let actual = validate_initial_runtime(&decoded_capture, &decoded_data, &bad_session, 0, RuntimeLimits::default(), &mut AllocationContext::new()).unwrap_err();
        match actual {
            CompatibilityFailure::Store(StoreError::Runtime(actual)) => assert_eq!(format!("{actual:?}"), format!("{legacy:?}")),
            other => panic!("unexpected decoded start error: {other:?}"),
        }

        let legacy_bad_day = crate::product_contract::RuntimeAdapter::start(
            &crate::product_runtime::LocalRuntime::default(), &capture, &data, &legacy_session,
            i32::MAX, 7, RuntimeLimits::default(),
        ).err().unwrap();
        let actual_bad_day = validate_initial_runtime(
            &decoded_capture, &decoded_data, &session, i32::MAX, RuntimeLimits::default(),
            &mut AllocationContext::new(),
        ).unwrap_err();
        match actual_bad_day {
            CompatibilityFailure::Store(StoreError::Runtime(actual)) => {
                assert_eq!(format!("{actual:?}"), format!("{legacy_bad_day:?}"));
            }
            other => panic!("unexpected decoded day error: {other:?}"),
        }
    }
}
