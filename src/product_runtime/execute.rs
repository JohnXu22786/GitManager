use super::*;
use eval::{boolean, list, record, value_ref};

fn evaluate(
    app: AppRef<'_>,
    state: &State,
    expr: &Expr,
    env: &Env,
    meter: &mut Meter,
) -> Result<DataValue> {
    Eval::view(app, DataSnapshotView::from(&state.data), SessionStateView::from(&state.session), state.day, meter)
    .eval(expr, env)
}
fn evaluate_owner(
    app: AppRef<'_>,
    state: &KernelState<'_>,
    expr: &Expr,
    env: &Env,
    meter: &mut Meter,
) -> Result<DataValue> {
    let (data, session, day) = state.views();
    Eval::view(app, data, session, day, meter).eval(expr, env)
}
fn assignments(
    app: AppRef<'_>,
    state: &State,
    values: StringMapRef<'_, Expr>,
    env: &Env,
    meter: &mut Meter,
) -> Result<Values> {
    values
        .iter()
        .map(|(id, expr)| Ok((id.to_owned(), evaluate(app, state, expr, env, meter)?)))
        .collect()
}
fn update_assignments(
    app: AppRef<'_>,
    state: &KernelState<'_>,
    values: StringMapRef<'_, Expr>,
    env: &Env,
    meter: &mut Meter,
) -> Result<UpdateValues> {
    match state {
        KernelState::Legacy(_) => values.iter().map(|(id, expr)| {
            Ok((id.to_owned(), evaluate_owner(app, state, expr, env, meter)?))
        }).collect::<Result<Values>>().map(UpdateValues::Legacy),
        KernelState::Decoded { .. } => {
            let mut output = FallibleValues::new();
            for (id, expr) in values.iter() {
                let value = evaluate_owner(app, state, expr, env, meter)?;
                let id = product_alloc::try_copy_str(
                    id,
                    &mut meter.allocations,
                    product_alloc::AllocationSite::String,
                ).map_err(AdapterError::Allocation)?;
                let value = try_clone_data_value_with(&value, &mut meter.allocations)
                    .map_err(AdapterError::Allocation)?;
                output.try_insert(id, value, &mut meter.allocations)
                    .map_err(AdapterError::Allocation)?;
            }
            Ok(UpdateValues::Fallible(output))
        }
    }
}
fn update(
    state: &mut State,
    key: &DataValue,
    values: Values,
    archive: bool,
    meter: &mut Meter,
) -> Result<()> {
    update_owner(
        DataOwner::Legacy(&mut state.data),
        key,
        UpdateValues::Legacy(values),
        archive,
        meter,
    )
}

enum DataOwner<'a> {
    Legacy(&'a mut DataSnapshot),
    Fallible(&'a mut FallibleDecodedDataSnapshot),
}
enum UpdateValues {
    Legacy(Values),
    Fallible(FallibleValues),
}
enum SessionOwner<'a> {
    Legacy(&'a mut SessionState),
    Fallible(&'a mut FallibleDecodedSessionState),
}

/// Persistent owners used by the one statement interpreter. Legacy execution
/// borrows its existing runtime state; decoded execution supplies the same data
/// and session owners without materializing either through legacy maps.
enum KernelState<'a> {
    Legacy(&'a mut State),
    Decoded {
        data: &'a mut FallibleDecodedDataSnapshot,
        session: &'a mut FallibleDecodedSessionState,
        day: i32,
    },
}
impl KernelState<'_> {
    fn views(&self) -> (DataSnapshotView<'_>, SessionStateView<'_>, i32) {
        match self {
            Self::Legacy(state) => (
                DataSnapshotView::from(&state.data),
                SessionStateView::from(&state.session),
                state.day,
            ),
            Self::Decoded { data, session, day } => (
                DataSnapshotView::from(&**data),
                SessionStateView::from(&**session),
                *day,
            ),
        }
    }
    fn legacy(&mut self) -> Result<&mut State> {
        match self {
            Self::Legacy(state) => Ok(state),
            Self::Decoded { .. } => Err(failed("statement is outside decoded update/state slice")),
        }
    }
    fn update(&mut self, key: &DataValue, values: UpdateValues, meter: &mut Meter) -> Result<()> {
        match self {
            Self::Legacy(state) => {
                let UpdateValues::Legacy(values) = values else {
                    return Err(failed("transaction values owner mismatch"));
                };
                update(state, key, values, false, meter)
            }
            Self::Decoded { data, .. } => {
                let UpdateValues::Fallible(values) = values else {
                    return Err(failed("transaction values owner mismatch"));
                };
                update_owner(
                    DataOwner::Fallible(data),
                    key,
                    UpdateValues::Fallible(values),
                    false,
                    meter,
                )
            }
        }
    }
    fn set_state(&mut self, id: &str, value: DataValue, meter: &mut Meter) -> Result<()> {
        match self {
            Self::Legacy(state) => set_state_value(SessionOwner::Legacy(&mut state.session), id, value, meter),
            Self::Decoded { session, .. } => set_state_value(SessionOwner::Fallible(session), id, value, meter),
        }
    }
}

fn set_state_value(
    owner: SessionOwner<'_>,
    id: &str,
    value: DataValue,
    meter: &mut Meter,
) -> Result<()> {
    meter.write()?;
    match owner {
        SessionOwner::Legacy(session) => {
            session.values.insert(id.to_owned(), value);
            Ok(())
        }
        SessionOwner::Fallible(session) => session.values_mut()
            .try_set(id, value, &mut meter.allocations)
            .map(|_| ())
            .map_err(AdapterError::Allocation),
    }
}

fn advance_generation(data: &mut DataSnapshot) -> Result<u64> {
    let next = data.generation.checked_add(1)
        .ok_or_else(|| failed("data generation overflow"))?;
    data.generation = next;
    Ok(next)
}

/// Shared record update/archive primitive for both persisted owner families.
/// The caller owns a transaction-local snapshot, so partial fallible map growth
/// is discarded with that snapshot if any later reservation fails.
fn update_owner(
    owner: DataOwner<'_>,
    key: &DataValue,
    values: UpdateValues,
    archive: bool,
    meter: &mut Meter,
) -> Result<()> {
    meter.write()?;
    let record_count = match &owner {
        DataOwner::Legacy(data) => data.records.len(),
        DataOwner::Fallible(data) => data.record_count(),
    };
    meter.tick(record_count as u64)?;
    let (entity, record) = match key {
        DataValue::Reference { entity, record } => (entity.as_str(), record.as_str()),
        _ => return Err(failed("expected record reference")),
    };
    match owner {
        DataOwner::Legacy(data) => {
            let row = data.records.iter_mut()
                .find(|row| row.entity == entity && row.id == record)
                .ok_or_else(|| invalid("record does not exist"))?;
            if row.archived {
                return Err(invalid("archived historical records cannot be modified"));
            }
            let revision = row.revision.checked_add(1)
                .ok_or_else(|| failed("record revision overflow"))?;
            let UpdateValues::Legacy(values) = values else {
                return Err(failed("transaction values owner mismatch"));
            };
            row.values.extend(values);
            row.archived = archive;
            row.revision = revision;
            Ok(())
        }
        DataOwner::Fallible(data) => {
            let UpdateValues::Fallible(values) = values else {
                return Err(failed("transaction values owner mismatch"));
            };
            data.try_update_record_by(entity, record, values, archive, &mut meter.allocations)
                .map_err(|error| match error {
                    AppValidationError::Contract(error) if error.0 == "record revision overflow" => {
                        failed("record revision overflow")
                    }
                    error => app_validation_error(error),
                })
        }
    }
}

#[cfg(test)]
pub(crate) fn decoded_shared_transaction_kernel_parity_and_atomicity(
) -> std::result::Result<(), String> {
    use std::num::NonZeroUsize;
    let program = Digest::try_from("0".repeat(64)).map_err(|error| error.to_string())?;
    let legacy = DataSnapshot {
        version: CONTRACT_VERSION,
        project_id: "project".into(),
        generation: 0,
        schema: vec![],
        records: vec![Record {
            entity: "job".into(),
            id: "row-1".into(),
            revision: 1,
            created_program: program.clone(),
            archived: false,
            values: Values::from([("name".into(), DataValue::Text { value: "old".into() })]),
        }],
        events: vec![],
    };
    let encoded = serde_json::to_vec(&legacy).map_err(|error| error.to_string())?;
    let mut decode_allocations = product_alloc::AllocationContext::new();
    let decoded = decode_data_snapshot_fallible(
        crate::product_json::parse_json_bytes(&encoded, &mut decode_allocations)
            .map_err(|error| format!("{error:?}"))?,
        &mut decode_allocations,
    ).map_err(|error| format!("{error:?}"))?;

    // Exercise the exact statement interpreter used by LocalRuntime::apply,
    // with a decoded data/session owner. The update and state write are kept in
    // one StatementSlice so evaluation order and meter behavior are shared.
    let reference = DataValue::Reference { entity: "job".into(), record: "row-1".into() };
    let steps = vec![
        Statement::Update {
            record: Expr::Literal { value_type: Type::reference("job"), value: reference.clone() },
            values: [("extra".into(), Expr::Literal {
                value_type: Type::Text,
                value: DataValue::Text { value: "through-kernel".into() },
            })].into(),
        },
        Statement::SetState {
            state: "selected".into(),
            value: Expr::Literal {
                value_type: Type::Text,
                value: DataValue::Text { value: "through-kernel".into() },
            },
        },
    ];
    let only_state = vec![steps[1].clone()];
    let app = AppDefinition {
        version: CONTRACT_VERSION,
        id: "kernel-test".into(),
        label: "Kernel test".into(),
        entities: vec![],
        state: vec![],
        actions: vec![
            ActionDefinition { id: "kernel".into(), label: "Kernel".into(), parameters: BTreeMap::new(), guards: vec![], steps: steps.clone(), ensures: vec![] },
            ActionDefinition { id: "update_only".into(), label: "Update only".into(), parameters: BTreeMap::new(), guards: vec![], steps: steps[..1].to_vec(), ensures: vec![] },
            ActionDefinition { id: "state_only".into(), label: "State only".into(), parameters: BTreeMap::new(), guards: vec![], steps: only_state.clone(), ensures: vec![] },
            ActionDefinition { id: "create_only".into(), label: "Create only".into(), parameters: BTreeMap::new(), guards: vec![], steps: vec![Statement::Create {
                entity: "job".into(),
                values: [("name".into(), Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "created".into() } })].into(),
                bind: "created".into(),
            }], ensures: vec![] },
            ActionDefinition { id: "create_update".into(), label: "Create then update".into(), parameters: BTreeMap::new(), guards: vec![], steps: vec![
                Statement::Create {
                    entity: "job".into(),
                    values: [("name".into(), Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "created".into() } })].into(),
                    bind: "created".into(),
                },
                Statement::Update {
                    record: Expr::Variable { name: "created".into() },
                    values: [("extra".into(), Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "same-transaction".into() } })].into(),
                },
            ], ensures: vec![] },
        ],
        outputs: vec![],
        observables: vec![],
        views: vec![],
        initial_view: "work".into(),
    };
    let mut app_allocations = product_alloc::AllocationContext::new();
    let app_value = crate::product_json::parse_json_bytes(
        &serde_json::to_vec(&app).map_err(|error| error.to_string())?,
        &mut app_allocations,
    ).map_err(|error| format!("decoded app parse: {error:?}"))?;
    let decoded_app = fallible_app_decode::decode_app_definition(app_value, &mut app_allocations)
        .map_err(|error| format!("decoded app: {error:?}"))?;
    let legacy_steps = AppRef::from(&app).action("kernel").ok_or("legacy test action absent")?.steps();
    let decoded_steps = AppRef::from(&decoded_app).action("kernel").ok_or("decoded test action absent")?.steps();
    let decoded_update_only = AppRef::from(&decoded_app).action("update_only").ok_or("decoded update action absent")?.steps();
    let decoded_state_only = AppRef::from(&decoded_app).action("state_only").ok_or("decoded state action absent")?.steps();
    let legacy_create = AppRef::from(&app).action("create_only").ok_or("legacy create action absent")?.steps();
    let decoded_create = AppRef::from(&decoded_app).action("create_only").ok_or("decoded create action absent")?.steps();
    let legacy_create_update = AppRef::from(&app).action("create_update").ok_or("legacy create-update action absent")?.steps();
    let decoded_create_update = AppRef::from(&decoded_app).action("create_update").ok_or("decoded create-update action absent")?.steps();
    let mut legacy_kernel_state = State {
        data: legacy.clone(),
        session: SessionState { view: "work".into(), values: Values::new(), focused_record: None },
        day: 0,
        outputs: vec![],
        output_participants: vec![],
        output_participant_count: 0,
    };
    let mut legacy_kernel_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut legacy_owners = KernelState::Legacy(&mut legacy_kernel_state);
    Transaction {
        program: None,
        app: AppRef::from(&app),
        program_digest: &program,
        operation: "kernel-op",
        seed: 7,
        created: 0,
        cancel_after_statement: None,
    }.statements(&mut legacy_owners, legacy_steps, &mut Env::new(), &mut legacy_kernel_meter)
        .map_err(|error| format!("legacy statement kernel: {error:?}"))?;

    let mut decoded_kernel_data = decoded.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded kernel data copy: {error:?}"))?;
    let initial_session = SessionState { view: "work".into(), values: Values::new(), focused_record: None };
    let mut session_allocations = product_alloc::AllocationContext::new();
    let mut decoded_kernel_session = decode_session_state_fallible(
        crate::product_json::parse_json_bytes(
            &serde_json::to_vec(&initial_session).map_err(|error| error.to_string())?,
            &mut session_allocations,
        ).map_err(|error| format!("decoded kernel session parse: {error:?}"))?,
        &mut session_allocations,
    ).map_err(|error| format!("decoded kernel session: {error:?}"))?;
    let before_kernel_refusal = decoded_kernel_data.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded kernel refusal copy: {error:?}"))?;
    let mut refuse_update = Meter::with_allocations(
        RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)),
        product_alloc::AllocationContext::fail_nth_growth(NonZeroUsize::new(7).unwrap(), false),
    );
    let mut decoded_owners = KernelState::Decoded { data: &mut decoded_kernel_data, session: &mut decoded_kernel_session, day: 0 };
    let refused_update = Transaction {
        program: None, app: AppRef::from(&decoded_app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None,
    }.statements(&mut decoded_owners, decoded_update_only, &mut Env::new(), &mut refuse_update);
    if !matches!(
        refused_update,
        Err(AdapterError::Allocation(failure)) if failure.site == product_alloc::AllocationSite::OrderedMap
    ) || refuse_update.allocations.growth_requests() != 7
        || decoded_kernel_data != before_kernel_refusal
    {
        return Err(format!("decoded statement update refusal was not atomic: {refused_update:?}; growth requests {}", refuse_update.allocations.growth_requests()));
    }

    let before_state_refusal = decoded_kernel_session.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded state refusal copy: {error:?}"))?;
    let mut refuse_state = Meter::with_allocations(
        RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)),
        product_alloc::AllocationContext::fail_nth_growth(NonZeroUsize::new(3).unwrap(), false),
    );
    let mut decoded_owners = KernelState::Decoded { data: &mut decoded_kernel_data, session: &mut decoded_kernel_session, day: 0 };
    let refused_state = Transaction {
        program: None, app: AppRef::from(&decoded_app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None,
    }.statements(&mut decoded_owners, decoded_state_only, &mut Env::new(), &mut refuse_state);
    if !matches!(
        refused_state,
        Err(AdapterError::Allocation(failure)) if failure.site == product_alloc::AllocationSite::OrderedMap
    ) || refuse_state.allocations.growth_requests() != 3
        || decoded_kernel_session != before_state_refusal
    {
        return Err(format!("decoded statement state refusal was not atomic: {refused_state:?}"));
    }

    let mut retry_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut decoded_owners = KernelState::Decoded { data: &mut decoded_kernel_data, session: &mut decoded_kernel_session, day: 0 };
    Transaction {
        program: None, app: AppRef::from(&decoded_app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None,
    }.statements(&mut decoded_owners, decoded_steps, &mut Env::new(), &mut retry_meter)
        .map_err(|error| format!("decoded statement kernel retry: {error:?}"))?;
    if serde_json::to_value(&legacy_kernel_state.data.records[0]).map_err(|error| error.to_string())?
        != serde_json::to_value(&decoded_kernel_data.records()[0]).map_err(|error| error.to_string())?
        || serde_json::to_value(&legacy_kernel_state.session).map_err(|error| error.to_string())?
        != serde_json::to_value(&decoded_kernel_session).map_err(|error| error.to_string())?
    {
        return Err("legacy and decoded statement kernel results diverged".into());
    }

    // Create runs through the same statement interpreter for both owner types.
    let mut create_legacy = legacy_kernel_state.clone();
    let mut create_legacy_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut create_legacy_owner = KernelState::Legacy(&mut create_legacy);
    Transaction { program: None, app: AppRef::from(&app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut create_legacy_owner, legacy_create, &mut Env::new(), &mut create_legacy_meter)
        .map_err(|error| format!("legacy create: {error:?}"))?;
    let create_base = decoded_kernel_data.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded create base copy: {error:?}"))?;
    let mut create_probe = create_base.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded create probe copy: {error:?}"))?;
    let mut create_probe_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut create_probe_owner = KernelState::Decoded { data: &mut create_probe, session: &mut decoded_kernel_session, day: 0 };
    Transaction { program: None, app: AppRef::from(&decoded_app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut create_probe_owner, decoded_create, &mut Env::new(), &mut create_probe_meter)
        .map_err(|error| format!("decoded create probe: {error:?}"))?;
    let create_growths = create_probe_meter.allocations.growth_requests();
    if create_growths == 0 || create_legacy.data.records.last().map(|row| serde_json::to_value(row).unwrap())
        != create_probe.records().last().map(|row| serde_json::to_value(row).unwrap()) {
        return Err("legacy and decoded Create results diverged".into());
    }
    for nth in 1..=create_growths {
        let mut refused_data = create_base.try_clone_with(&mut product_alloc::AllocationContext::new())
            .map_err(|error| format!("decoded create refusal copy: {error:?}"))?;
        let before_refusal = refused_data.try_clone_with(&mut product_alloc::AllocationContext::new())
            .map_err(|error| format!("decoded create refusal snapshot: {error:?}"))?;
        let mut refuse_create = Meter::with_allocations(
            RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)),
            product_alloc::AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false),
        );
        let mut refused_owner = KernelState::Decoded { data: &mut refused_data, session: &mut decoded_kernel_session, day: 0 };
        let refused = Transaction { program: None, app: AppRef::from(&decoded_app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
            .statements(&mut refused_owner, decoded_create, &mut Env::new(), &mut refuse_create);
        if !matches!(refused, Err(AdapterError::Allocation(_))) || refused_data != before_refusal {
            return Err(format!("decoded Create refusal {nth} changed the owner or lost allocation error: {refused:?}"));
        }
    }
    let mut retry_create = create_base.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded create retry copy: {error:?}"))?;
    let mut retry_create_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut retry_create_owner = KernelState::Decoded { data: &mut retry_create, session: &mut decoded_kernel_session, day: 0 };
    Transaction { program: None, app: AppRef::from(&decoded_app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut retry_create_owner, decoded_create, &mut Env::new(), &mut retry_create_meter)
        .map_err(|error| format!("decoded Create retry: {error:?}"))?;
    if retry_create != create_probe { return Err("decoded Create retry differs from successful Create".into()); }

    let colliding_id = format!("record-{}", &canonical_digest(
        IdentityDomain::Input, &(7u64, "kernel-op", "job", 1usize),
    ).map_err(|error| format!("collision identity digest: {error}"))?.as_str()[..32]);
    let mut collision_legacy = legacy_kernel_state.clone();
    collision_legacy.data.records.push(Record {
        entity: "job".into(), id: colliding_id.clone(), revision: 1,
        created_program: program.clone(), archived: false, values: Values::new(),
    });
    let mut collision_decoded = create_base.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded collision copy: {error:?}"))?;
    collision_decoded.try_append_record(
        "job", &colliding_id, 1, &program, FallibleValues::new(),
        &mut product_alloc::AllocationContext::new(),
    ).map_err(|error| format!("decoded collision fixture: {error:?}"))?;
    let collision_before = collision_decoded.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded collision snapshot: {error:?}"))?;
    let mut collision_legacy_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut collision_legacy_owner = KernelState::Legacy(&mut collision_legacy);
    let collision_legacy_result = Transaction { program: None, app: AppRef::from(&app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut collision_legacy_owner, legacy_create, &mut Env::new(), &mut collision_legacy_meter);
    let mut collision_decoded_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut collision_decoded_owner = KernelState::Decoded { data: &mut collision_decoded, session: &mut decoded_kernel_session, day: 0 };
    let collision_decoded_result = Transaction { program: None, app: AppRef::from(&decoded_app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut collision_decoded_owner, decoded_create, &mut Env::new(), &mut collision_decoded_meter);
    if !matches!(&collision_legacy_result, Err(AdapterError::Invalid(message)) if message.0 == "generated record identity collision")
        || !matches!(&collision_decoded_result, Err(AdapterError::Invalid(message)) if message.0 == "generated record identity collision")
        || collision_decoded != collision_before
        || collision_legacy.data.records.len() != collision_decoded.record_count() {
        return Err(format!("legacy and decoded generated-identity collision behavior diverged: {collision_legacy_result:?} / {collision_decoded_result:?}"));
    }

    let mut create_update_legacy = legacy_kernel_state.clone();
    let mut create_update_legacy_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut create_update_legacy_owner = KernelState::Legacy(&mut create_update_legacy);
    Transaction { program: None, app: AppRef::from(&app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut create_update_legacy_owner, legacy_create_update, &mut Env::new(), &mut create_update_legacy_meter)
        .map_err(|error| format!("legacy Create then Update: {error:?}"))?;
    let mut create_update_decoded = create_base.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded Create then Update copy: {error:?}"))?;
    let mut create_update_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut create_update_owner = KernelState::Decoded { data: &mut create_update_decoded, session: &mut decoded_kernel_session, day: 0 };
    Transaction { program: None, app: AppRef::from(&decoded_app), program_digest: &program, operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None }
        .statements(&mut create_update_owner, decoded_create_update, &mut Env::new(), &mut create_update_meter)
        .map_err(|error| format!("decoded Create then Update: {error:?}"))?;
    if create_update_legacy.data.records.last().map(|row| (row.revision, serde_json::to_value(&row.values).unwrap()))
        != create_update_decoded.records().last().map(|row| (row.revision(), serde_json::to_value(row.values()).unwrap())) {
        return Err("legacy and decoded Create then Update results diverged".into());
    }

    // Decoded overflow keeps the same refusal class as the legacy update.
    let mut overflow_legacy = legacy.clone();
    overflow_legacy.records[0].revision = u64::MAX;
    let mut overflow_allocations = product_alloc::AllocationContext::new();
    let overflow_bytes = serde_json::to_vec(&overflow_legacy).map_err(|error| error.to_string())?;
    let overflow_value = crate::product_json::parse_json_bytes(&overflow_bytes, &mut overflow_allocations)
        .map_err(|error| format!("overflow snapshot parse: {error:?}"))?;
    let mut overflow_decoded = decode_data_snapshot_fallible(overflow_value, &mut overflow_allocations)
        .map_err(|error| format!("overflow snapshot decode: {error:?}"))?;
    let before_overflow_decoded = overflow_decoded.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("overflow snapshot copy: {error:?}"))?;
    let mut overflow_legacy_state = State {
        data: overflow_legacy.clone(),
        session: initial_session.clone(),
        day: 0,
        outputs: vec![],
        output_participants: vec![],
        output_participant_count: 0,
    };
    let mut overflow_legacy_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut overflow_legacy_owner = KernelState::Legacy(&mut overflow_legacy_state);
    let overflow_legacy_result = Transaction {
        program: None, app: AppRef::from(&app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None,
    }.statements(&mut overflow_legacy_owner, legacy_steps, &mut Env::new(), &mut overflow_legacy_meter);
    let mut overflow_session = decode_session_state_fallible(
        crate::product_json::parse_json_bytes(
            &serde_json::to_vec(&initial_session).map_err(|error| error.to_string())?,
            &mut product_alloc::AllocationContext::new(),
        ).map_err(|error| format!("overflow session parse: {error:?}"))?,
        &mut product_alloc::AllocationContext::new(),
    ).map_err(|error| format!("overflow session decode: {error:?}"))?;
    let mut overflow_decoded_meter = Meter::new(RuntimeLimits::default(), 1_000_000, Arc::new(AtomicBool::new(false)));
    let mut overflow_decoded_owner = KernelState::Decoded { data: &mut overflow_decoded, session: &mut overflow_session, day: 0 };
    let overflow_decoded_result = Transaction {
        program: None, app: AppRef::from(&decoded_app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: None,
    }.statements(&mut overflow_decoded_owner, decoded_update_only, &mut Env::new(), &mut overflow_decoded_meter);
    if !matches!(&overflow_legacy_result, Err(AdapterError::Failed(message)) if message == "record revision overflow")
        || !matches!(&overflow_decoded_result, Err(AdapterError::Failed(message)) if message == "record revision overflow")
        || overflow_decoded != before_overflow_decoded
        || overflow_legacy_state.data != overflow_legacy
    {
        return Err(format!("legacy and decoded revision overflow diverged: {overflow_legacy_result:?} vs {overflow_decoded_result:?}"));
    }

    let before_cancel = decoded_kernel_data.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded cancel copy: {error:?}"))?;
    let before_cancel_session = decoded_kernel_session.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("decoded cancel session copy: {error:?}"))?;
    let mut cancelled_private_data = before_cancel.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("cancel private data copy: {error:?}"))?;
    let mut cancelled_private_session = before_cancel_session.try_clone_with(&mut product_alloc::AllocationContext::new())
        .map_err(|error| format!("cancel private session copy: {error:?}"))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut cancel_meter = Meter::new(RuntimeLimits::default(), 1_000_000, cancel.clone());
    let mut decoded_owners = KernelState::Decoded { data: &mut cancelled_private_data, session: &mut cancelled_private_session, day: 0 };
    let cancelled = Transaction {
        program: None, app: AppRef::from(&decoded_app), program_digest: &program,
        operation: "kernel-op", seed: 7, created: 0, cancel_after_statement: Some(cancel.clone()),
    }.statements(&mut decoded_owners, decoded_steps, &mut Env::new(), &mut cancel_meter);
    if !matches!(cancelled, Err(AdapterError::Cancelled))
        || cancelled_private_data == before_cancel
        || decoded_kernel_data != before_cancel
        || decoded_kernel_session != before_cancel_session
    {
        return Err(format!("decoded statement cancellation did not roll back after an update: {cancelled:?}"));
    }
    Ok(())
}
struct Transaction<'a> {
    program: Option<&'a CapturedProgram>,
    app: AppRef<'a>,
    program_digest: &'a Digest,
    operation: &'a str,
    seed: u64,
    created: usize,
    #[cfg(test)]
    cancel_after_statement: Option<Arc<AtomicBool>>,
}
impl Transaction<'_> {
    fn statements(
        &mut self,
        state: &mut KernelState<'_>,
        steps: StatementSlice<'_>,
        env: &mut Env,
        meter: &mut Meter,
    ) -> Result<()> {
        let app = self.app;
        for index in 0..steps.len() {
            meter.step()?;
            match steps.get(index).expect("bounded statement index") {
                StatementView::Create {
                    entity,
                    values,
                    bind,
                } => match state {
                    KernelState::Legacy(state) => {
                        let values = assignments(app, state, values, env, meter)?;
                        meter.write()?;
                        meter.collection(state.data.records.len() + 1)?;
                        self.created += 1;
                        let digest = canonical_digest(
                            IdentityDomain::Input,
                            &(self.seed, self.operation, entity, self.created),
                        )?;
                        let id = format!("record-{}", &digest.as_str()[..32]);
                        if state
                            .data
                            .records
                            .iter()
                            .any(|r| r.entity == *entity && r.id == id)
                        {
                            return Err(invalid("generated record identity collision"));
                        }
                        let row = Record {
                            entity: entity.to_owned(),
                            id,
                            revision: 1,
                            created_program: self.program_digest.clone(),
                            archived: false,
                            values,
                        };
                        env.insert(bind.to_owned(), (Type::reference(entity), value_ref(&row)));
                        state.data.records.push(row);
                    }
                    KernelState::Decoded { .. } => {
                        let values = update_assignments(app, state, values, env, meter)?;
                        let UpdateValues::Fallible(values) = values else {
                            return Err(failed("transaction values owner mismatch"));
                        };
                        let KernelState::Decoded { data, .. } = state else { unreachable!() };
                        meter.write()?;
                        meter.collection(data.record_count() + 1)?;
                        self.created += 1;
                        let digest = canonical_digest(
                            IdentityDomain::Input,
                            &(self.seed, self.operation, entity, self.created),
                        )?;
                        let id = format!("record-{}", &digest.as_str()[..32]);
                        if data.records().iter().any(|row| row.entity() == entity && row.id() == id) {
                            return Err(invalid("generated record identity collision"));
                        }
                        data.try_append_record(entity, &id, 1, self.program_digest, values, &mut meter.allocations)
                            .map_err(app_validation_error)?;
                        env.insert(bind.to_owned(), (Type::reference(entity), DataValue::Reference {
                            entity: entity.to_owned(), record: id,
                        }));
                    }
                },
                StatementView::Update { record, values } => {
                    let key = evaluate_owner(app, state, record, env, meter)?;
                    let values = update_assignments(app, state, values, env, meter)?;
                    state.update(&key, values, meter)?;
                }
                StatementView::Archive { record } => {
                    let state = state.legacy()?;
                    let key = evaluate(app, state, record, env, meter)?;
                    update(state, &key, Values::new(), true, meter)?;
                }
                StatementView::SetState { state: id, value } => {
                    let value = evaluate_owner(app, state, value, env, meter)?;
                    state.set_state(id, value, meter)?;
                }
                StatementView::Collection {
                    target,
                    operation,
                    items,
                } => {
                    let state = state.legacy()?;
                    let (_, items) = list(evaluate(app, state, items, env, meter)?)?;
                    let (key, field, old) = match target {
                        CollectionTarget::State { state: id } => (
                            None,
                            id.to_owned(),
                            state
                                .session
                                .values
                                .get(id)
                                .cloned()
                                .ok_or_else(|| failed("state absent"))?,
                        ),
                        CollectionTarget::Field {
                            record: expr,
                            field,
                        } => {
                            let key = evaluate(app, state, expr, env, meter)?;
                            let value = record(&state.data, &key)?
                                .values
                                .get(field)
                                .cloned()
                                .ok_or_else(|| invalid("collection field absent"))?;
                            (Some(key), field.to_owned(), value)
                        }
                    };
                    let (item_type, mut previous) = list(old)?;
                    match operation {
                        CollectionOperation::Insert => {
                            for item in items {
                                meter.tick(previous.len() as u64 + 1)?;
                                if !previous.contains(&item) {
                                    meter.collection(previous.len() + 1)?;
                                    previous.push(item);
                                }
                            }
                        }
                        CollectionOperation::Remove => {
                            let mut retained = Vec::new();
                            for item in previous {
                                meter.tick(items.len() as u64 + 1)?;
                                if !items.contains(&item) {
                                    retained.push(item);
                                }
                            }
                            previous = retained;
                        }
                    }
                    let value = DataValue::List {
                        item_type,
                        items: previous,
                    };
                    if let Some(key) = key {
                        update(state, &key, BTreeMap::from([(field, value)]), false, meter)?;
                    } else {
                        set_state_value(SessionOwner::Legacy(&mut state.session), &field, value, meter)?;
                    }
                }
                StatementView::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let (typ, items) = list(evaluate_owner(app, state, items, env, meter)?)?;
                    for item in items {
                        meter.tick(1)?;
                        let mut local = env.clone();
                        local.insert(binding.to_owned(), (typ.clone(), item));
                        self.statements(state, steps, &mut local, meter)?;
                    }
                }
                StatementView::Assert { condition, message } => {
                    if !boolean(evaluate_owner(app, state, condition, env, meter)?)? {
                        return Err(invalid(message));
                    }
                }
                StatementView::Emit {
                    output,
                    items,
                    binding,
                    columns,
                } => {
                    let state = state.legacy()?;
                    let program = self.program.ok_or_else(|| failed("output requires captured legacy program"))?;
                    let definition = app
                        .outputs()
                        .iter()
                        .find(|o| o.id == output)
                        .ok_or_else(|| failed("output absent"))?;
                    let (typ, items) = list(evaluate(app, state, items, env, meter)?)?;
                    // Auxiliary identity evidence must never change Emit's
                    // business evaluation or partially certify a collection.
                    let eligible = matches!(typ, Type::Reference { .. })
                        && !items.is_empty()
                        && items.len()
                            <= MAX_COLLECTION.saturating_sub(state.output_participant_count);
                    let index = eligible.then(|| {
                        state
                            .data
                            .records
                            .iter()
                            .map(|row| ((row.entity.as_str(), row.id.as_str()), row))
                            .collect::<BTreeMap<_, _>>()
                    });
                    let mut participants = eligible.then(|| Vec::with_capacity(items.len()));
                    let mut rows = Vec::new();
                    let header = LocalArtifact::from_rows(
                        output,
                        definition.format,
                        definition.columns.clone(),
                        vec![],
                    )?;
                    let prior_bytes = state.outputs.iter().map(|a| a.bytes.len()).sum::<usize>();
                    let mut encoded_bytes = header.bytes.len();
                    if prior_bytes.saturating_add(encoded_bytes) > meter.limits.output_bytes {
                        return Err(exhausted("cumulative output limit"));
                    }
                    for item in items {
                        meter.tick(1)?;
                        if participants.is_some() {
                            let resolved = match &item {
                                DataValue::Reference { entity, record } => {
                                    index.as_ref().and_then(|index| {
                                        index.get(&(entity.as_str(), record.as_str()))
                                    })
                                }
                                _ => None,
                            };
                            if let Some(row) = resolved {
                                participants
                                    .as_mut()
                                    .unwrap()
                            .push(EmissionRecord::from_record(row));
                            } else {
                                participants = None;
                            }
                        }
                        let mut local = env.clone();
                        local.insert(binding.to_owned(), (typ.clone(), item));
                        let row = assignments(app, state, columns, &local, meter)?;
                        for value in row.values() {
                            meter.value(value)?;
                        }
                        // Measure one bounded row through the authoritative
                        // encoder before accumulating it. This prevents a wide
                        // repeated projection allocating an oversized artifact.
                        let single = LocalArtifact::from_rows(
                            output,
                            definition.format,
                            definition.columns.clone(),
                            vec![row.clone()],
                        )
                        .map_err(|_| exhausted("output row exceeds encoder limits"))?;
                        meter.tick(single.bytes.len() as u64)?;
                        encoded_bytes =
                            encoded_bytes.saturating_add(single.bytes.len() - header.bytes.len());
                        if definition.format == OutputFormat::Json && !rows.is_empty() {
                            encoded_bytes = encoded_bytes.saturating_add(1);
                        }
                        if prior_bytes.saturating_add(encoded_bytes) > meter.limits.output_bytes {
                            return Err(exhausted("cumulative output limit"));
                        }
                        rows.push(row);
                    }
                    let artifact = LocalArtifact::from_rows(
                        output,
                        definition.format,
                        definition.columns.clone(),
                        rows,
                    )?;
                    meter.tick(artifact.bytes.len() as u64)?;
                    let total = state
                        .outputs
                        .iter()
                        .map(|a| a.bytes.len())
                        .sum::<usize>()
                        .saturating_add(artifact.bytes.len());
                    if total > meter.limits.output_bytes || state.outputs.len() >= MAX_ITEMS {
                        return Err(exhausted("cumulative output limit"));
                    }
                    state.output_participant_count += participants.as_ref().map_or(0, Vec::len);
                    state
                        .output_participants
                        .push(EmittedRecordParticipants::from_execution(
                            program,
                            self.operation,
                            state.outputs.len(),
                            &artifact,
                            participants,
                        ));
                    state.outputs.push(artifact);
                }
            }
            #[cfg(test)]
            if let Some(cancel) = &self.cancel_after_statement {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        Ok(())
    }
}
pub(super) fn constraints(program: &ProgramSource, state: &State, meter: &mut Meter) -> Result<()> {
    let app = AppRef::from(&program.program);
    let data = DataSnapshotView::from(&state.data);
    let session = SessionStateView::from(&state.session);
    constraints_view(app, data, session, state.day, meter,
        |_| state.data.validate().map_err(AdapterError::Invalid),
        |_, _| program.validate_session(&state.session))
}

pub(super) fn constraints_decoded(
    program: &FallibleDecodedCapturedProgram,
    data: &FallibleDecodedDataSnapshot,
    session: &FallibleDecodedSessionState,
    day: i32,
    meter: &mut Meter,
) -> Result<()> {
    let app = AppRef::from(program.app());
    let data_view = DataSnapshotView::from(data);
    let session_view = SessionStateView::from(session);
    constraints_view(app, data_view, session_view, day, meter,
        |allocations| data.validate_with(allocations).map_err(app_validation_error),
        |_, allocations| program.validate_session_with(session, allocations).map_err(app_validation_error))
}

fn app_validation_error(error: AppValidationError) -> AdapterError {
    match error {
        AppValidationError::Contract(error) => AdapterError::Invalid(error),
        AppValidationError::Allocation(failure) => AdapterError::Allocation(failure),
    }
}

fn constraints_view<D, S>(
    app: AppRef<'_>,
    data: DataSnapshotView<'_>,
    session: SessionStateView<'_>,
    day: i32,
    meter: &mut Meter,
    validate_data: D,
    validate_session: S,
) -> Result<()>
where
    D: FnOnce(&mut product_alloc::AllocationContext) -> Result<()>,
    S: FnOnce(&SessionStateView<'_>, &mut product_alloc::AllocationContext) -> Result<()>,
{
    meter.collection(data.record_count())?;
    for row in data.records() {
        meter.tick(1)?;
        for (_, value) in row.values().iter() { meter.value(value)?; }
        if row.archived() { continue; }
        let Some(entity) = app.entities().iter().find(|entity| entity.id == row.entity()) else { continue; };
        let mut legacy_env = Env::new();
        let mut managed_env = ManagedEnv::new();
        let env: &dyn EnvAccess = if app.is_fallible() {
            let reference = DataValue::Reference {
                entity: product_alloc::try_copy_str(row.entity(), &mut meter.allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
                record: product_alloc::try_copy_str(row.id(), &mut meter.allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)?,
            };
            let typ = Type::Reference { entity: product_alloc::try_copy_str(row.entity(), &mut meter.allocations, product_alloc::AllocationSite::String).map_err(AdapterError::Allocation)? };
            managed_env.try_bind("record", &typ, &reference, &mut meter.allocations).map_err(AdapterError::Allocation)?;
            &managed_env
        } else {
            legacy_env.insert("record".into(), (Type::reference(row.entity()), DataValue::Reference {
                entity: row.entity().into(), record: row.id().into(),
            }));
            &legacy_env
        };
        for constraint in &entity.constraints {
            let result = Eval::view(app, data, session, day, meter).eval(constraint, env)?;
            if !boolean(result)? { return Err(invalid(&format!("Entity constraint failed: {}", entity.id))); }
        }
    }
    validate_data(&mut meter.allocations)?;
    validate_session(&session, &mut meter.allocations)?;
    for (_, value) in session.values().iter() {
        meter.value(value)?;
        check_references_view(value, &data)?;
    }
    meter.tick(1)?;
    Ok(())
}

fn check_references_view(value: &DataValue, data: &DataSnapshotView<'_>) -> Result<()> {
    match value {
        DataValue::Reference { entity, record } => {
            if data.record(entity, record).is_none() { return Err(invalid("record reference does not exist")); }
        }
        DataValue::List { items, .. } => for item in items { check_references_view(item, data)?; },
        _ => {}
    }
    Ok(())
}
fn action(
    program: &ProgramSource,
    state: &mut State,
    id: &str,
    arguments: &Values,
    operation: &str,
    seed: u64,
    meter: &mut Meter,
) -> Result<()> {
    let app = AppRef::from(&program.program);
    let definition = app
        .action(id)
        .ok_or_else(|| invalid("unknown action"))?;
    validate_input(
        &SemanticInput::Invoke {
            action: id.into(),
            arguments: arguments.clone(),
        },
        &program.program,
    )?;
    let mut env = Env::new();
    for (id, value) in arguments {
        meter.value(value)?;
        check_references(value, &state.data)?;
        env.insert(
            id.clone(),
            (definition.parameters().get(id).expect("validated action parameter").clone(), value.clone()),
        );
    }
    for guard in definition.guards() {
        if !boolean(evaluate(app, state, guard, &env, meter)?)? {
            return Err(invalid(&format!("Action guard rejected: {id}")));
        }
    }
    let previous = state.data.clone();
    let output_start = state.outputs.len();
    state.data = merged_data(program, &state.data)?;
    let mut owners = KernelState::Legacy(state);
    Transaction {
        program: Some(program),
        app,
        program_digest: &program.artifact.program_digest,
        operation,
        seed,
        created: 0,
        #[cfg(test)]
        cancel_after_statement: None,
    }
    .statements(&mut owners, definition.steps(), &mut env, meter)?;
    let KernelState::Legacy(state) = owners else { unreachable!() };
    for ensure in definition.ensures() {
        if !boolean(evaluate(app, state, ensure, &env, meter)?)? {
            return Err(invalid(&format!("Action postcondition rejected: {id}")));
        }
    }
    advance_generation(&mut state.data)?;
    let mut changes = Vec::new();
    for row in &state.data.records {
        meter.tick(previous.records.len() as u64 + 1)?;
        let before = previous
            .records
            .iter()
            .find(|r| r.entity == row.entity && r.id == row.id);
        if before != Some(row) {
            changes.push(RecordChange {
                entity: row.entity.clone(),
                record: row.id.clone(),
                before: before.map(|r| r.values.clone()),
                after: row.values.clone(),
                archived: row.archived,
            });
        }
    }
    let event_id = canonical_digest(
        IdentityDomain::Input,
        &(operation, &program.artifact.program_digest),
    )?;
    state.data.events.push(BusinessEvent {
        id: format!("event-{}", &event_id.as_str()[..32]),
        sequence: state.data.generation,
        operation_id: operation.into(),
        action: id.into(),
        day: state.day,
        program: program.artifact.program_digest.clone(),
        changes,
        outputs: state.outputs[output_start..]
            .iter()
            .map(|a| a.digest.clone())
            .collect(),
    });
    constraints(program, state, meter)
}
pub(super) fn input(
    program: &ProgramSource,
    state: &mut State,
    input: &SemanticInput,
    operation: &str,
    seed: u64,
    meter: &mut Meter,
) -> Result<()> {
    let app = &program.program;
    match input {
        SemanticInput::Invoke {
            action: id,
            arguments,
        } => action(program, state, id, arguments, operation, seed, meter),
        SemanticInput::Navigate { view } => {
            state.session.view = view.clone();
            state.session.focused_record = None;
            Ok(())
        }
        SemanticInput::AdvanceClock { days } => {
            let next = i64::from(state.day) + i64::from(*days);
            let next = i32::try_from(next).map_err(|_| failed("date overflow"))?;
            validate_day(next)?;
            state.day = next;
            // A saved clock must satisfy the same constraints as reopening it.
            // The caller discards this entire tentative state on rejection.
            constraints(program, state, meter)
        }
        SemanticInput::Observe { .. } => Ok(()),
        SemanticInput::Submit { view, arguments } => {
            if &state.session.view != view {
                return Err(invalid("form is not the active view"));
            }
            let view = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let ViewKind::Form { action: id, .. } = &view.kind else {
                return Err(invalid("not a form"));
            };
            action(program, state, id, arguments, operation, seed, meter)
        }
        SemanticInput::Control {
            view,
            control,
            value,
        } => {
            if &state.session.view != view {
                return Err(invalid("control is not in the active view"));
            }
            meter.value(value)?;
            check_references(value, &state.data)?;
            let definition = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let ViewKind::List {
                controls,
                selection,
                ..
            } = &definition.kind
            else {
                return Err(invalid("not a list control"));
            };
            let (id, on_change) = if let Some(control) = controls.iter().find(|c| &c.id == control)
            {
                (&control.state, &control.on_change)
            } else {
                let selection = selection
                    .as_ref()
                    .filter(|s| &s.id == control)
                    .ok_or_else(|| invalid("unknown control"))?;
                let (_, items) = list(value.clone())?;
                let mut unique = BTreeSet::new();
                for item in items {
                    if !unique.insert(canonical_bytes(&item)?) {
                        return Err(invalid("duplicate selected record"));
                    }
                }
                (&selection.state, &selection.on_change)
            };
            meter.write()?;
            state.session.values.insert(id.clone(), value.clone());
            if let Some(id) = on_change {
                action(
                    program,
                    state,
                    id,
                    &BTreeMap::from([("value".into(), value.clone())]),
                    operation,
                    seed,
                    meter,
                )?;
            }
            program.validate_session(&state.session)?;
            Ok(())
        }
        SemanticInput::Activate { view, binding, row } => {
            if &state.session.view != view {
                return Err(invalid("binding is not in the active view"));
            }
            let presented = super::view::render(app, state, meter)?;
            let definition = app
                .views
                .iter()
                .find(|v| &v.id == view)
                .ok_or_else(|| invalid("unknown view"))?;
            let binding = definition
                .actions
                .iter()
                .find(|b| &b.id == binding)
                .ok_or_else(|| invalid("unknown binding"))?;
            let mut env = Env::new();
            if let Some(row) = row {
                let presented = presented
                    .rows
                    .iter()
                    .find(|r| &r.record == row)
                    .ok_or_else(|| invalid("row is not visible"))?;
                if !presented.enabled_actions.contains(&binding.id) {
                    return Err(invalid("row action is disabled"));
                }
                env.insert(
                    "row".into(),
                    (
                        Type::reference(&row.entity),
                        DataValue::Reference {
                            entity: row.entity.clone(),
                            record: row.record.clone(),
                        },
                    ),
                );
            } else if !presented.enabled_actions.contains(&binding.id) {
                return Err(invalid("action is disabled"));
            }
            let arguments = assignments(
                AppRef::from(app),
                state,
                StringMapRef::Legacy(&binding.arguments),
                &env,
                meter,
            )?;
            action(
                program,
                state,
                &binding.action,
                &arguments,
                operation,
                seed,
                meter,
            )
        }
    }
}
