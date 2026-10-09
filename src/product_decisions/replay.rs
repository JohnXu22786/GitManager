use super::*;

pub struct ScopeAssessment {
    pub state: ScopeMatch,
    pub reason: String,
}
pub fn scope_match(scope: &DecisionScope, context: &ScopeContext) -> ScopeAssessment {
    if let Err(error) = scope.validate() {
        return ScopeAssessment {
            state: ScopeMatch::Unknown,
            reason: format!("Invalid scope: {error}"),
        };
    }
    let state = scope.matches(context);
    let reason = match state {
        ScopeMatch::Applies => "Action, population, conditions and exclusions apply".into(),
        ScopeMatch::Outside if !scope.operations.contains(&context.operation) => format!("Action {} is outside the accepted operations", context.operation),
        ScopeMatch::Outside => "Record population, a condition or an explicit exclusion places this work outside the intention".into(),
        ScopeMatch::Unknown => {
            let mut missing = vec![];
            if !scope.excluded_records.is_empty() && context.record.is_none() { missing.push("record identity needed to check exclusions".to_string()); }
            for key in scope.conditions.keys().filter(|key| !context.attributes.contains_key(*key)) { missing.push(format!("condition value {key}")); }
            for unknown in scope.unknowns.iter().filter(|u| u.operations.contains(&context.operation)) { missing.push(format!("unresolved boundary {}: {}", unknown.id, unknown.description)); }
            match &scope.population {
                Population::NewWork if context.is_new.is_none() => missing.push("whether this action creates new work".into()),
                Population::CreatedAfter { .. } if context.created_generation.is_none() => missing.push("record creation generation".into()),
                Population::Records { .. } | Population::Entity { .. } if context.record.is_none() => missing.push("affected record identity".into()),
                Population::Where { .. } => missing.push("independent evaluation of the population predicate".into()),
                _ => {}
            }
            format!("Applicability is unverified: {}", missing.join("; "))
        }
    };
    ScopeAssessment { state, reason }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptedScene {
    pub(super) program: CapturedProgram,
    pub(super) scenario: ScenarioSpec,
    pub(super) evidence: RunEvidence,
    pub(super) disclosure: Disclosure,
    #[serde(default = "observed_binding_default")]
    pub(super) bind_outcome: bool,
}
fn observed_binding_default() -> bool {
    true
}
pub(super) fn validate_scene_bindings(
    decision: &ScopedDecision,
    scenes: &[AcceptedScene],
) -> Result<IntentionBinding> {
    let observed = scenes
        .first()
        .ok_or_else(|| invalid("intention has no accepted scenes"))?
        .bind_outcome;
    if scenes.iter().any(|s| s.bind_outcome != observed)
        || (!observed && decision.obligations.is_empty())
    {
        return Err(invalid("mixed or empty property-only intention binding"));
    }
    Ok(if observed {
        IntentionBinding::ObservedOutcome
    } else {
        IntentionBinding::PropertiesOnly
    })
}
pub(super) fn verify_target_scope(
    scope: &DecisionScope,
    operations: &BTreeSet<Id>,
    mapping: &Mapping,
    contexts: &[ScopeContext],
) -> Result<()> {
    if operations.is_empty() {
        return Err(DecisionError::Unverified(
            "No mapped scoped operation is exercised".into(),
        ));
    }
    let mut mapped = scope.clone();
    mapped.operations = operations.clone();
    for unknown in &mut mapped.unknowns {
        unknown.operations = unknown
            .operations
            .iter()
            .map(|op| mapping.id(SemanticKind::Action, op))
            .filter(|op| operations.contains(op))
            .collect();
    }
    mapped.unknowns.retain(|u| !u.operations.is_empty());
    let mut covered = BTreeSet::new();
    for context in contexts {
        let assessment = scope_match(&mapped, context);
        match assessment.state {
            ScopeMatch::Unknown => return Err(DecisionError::Unverified(assessment.reason)),
            ScopeMatch::Applies => {
                covered.insert(context.operation.clone());
            }
            ScopeMatch::Outside => {}
        }
    }
    if covered != *operations {
        return Err(DecisionError::Unverified(format!(
            "Target does not exercise every mapped operation in scope: {}",
            operations
                .difference(&covered)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(())
}
impl AcceptedScene {
    pub fn scenario(&self) -> &ScenarioSpec {
        &self.scenario
    }
    pub fn observations(&self) -> &[Observation] {
        &self.evidence.observations
    }
    pub fn evidence(&self) -> &RunEvidence {
        &self.evidence
    }
    pub fn program(&self) -> &CapturedProgram {
        &self.program
    }
    pub(super) fn validate(&self) -> Result<()> {
        self.program.validate()?;
        self.scenario.validate(&self.program.program)?;
        self.evidence.validate()?;
        let binding = &self.evidence.binding;
        if self.evidence.state != EvidenceState::Observed
            || binding.source != self.program.binding
            || binding.artifact != self.program.artifact
            || binding.data_digest != self.scenario.seed.identity()?
            || binding.session_digest != self.scenario.session.identity()?
            || binding.input_digest != self.scenario.input_identity()?
            || binding.scenario_digest != self.scenario.identity()?
            || self.evidence.trace.len() != self.scenario.inputs.len()
            || !self
                .evidence
                .trace
                .iter()
                .map(|s| &s.input)
                .eq(self.scenario.inputs.iter())
        {
            return Err(invalid(
                "accepted scene evidence does not bind its exact program and complete input",
            ));
        }
        Ok(())
    }
}
pub fn accept_scene<R: RuntimeAdapter>(
    runtime: &R,
    program: &CapturedProgram,
    scenario: &ScenarioSpec,
    disclosure: Disclosure,
    limits: RuntimeLimits,
) -> Result<AcceptedScene> {
    Ok(capture_scene(runtime, program, scenario, disclosure, limits)?.0)
}
pub(super) fn capture_scene<R: RuntimeAdapter>(
    runtime: &R,
    program: &CapturedProgram,
    scenario: &ScenarioSpec,
    disclosure: Disclosure,
    limits: RuntimeLimits,
) -> Result<(AcceptedScene, Vec<ScopeContext>)> {
    let (run, contexts) = execute(
        runtime,
        program,
        scenario,
        &empty_graph(),
        limits,
        "accepted",
    )?;
    if run.state != EvidenceState::Observed {
        return Err(invalid(
            "accepted scene must execute completely with actual observations",
        ));
    }
    Ok((
        AcceptedScene {
            program: program.clone(),
            scenario: scenario.clone(),
            evidence: run,
            disclosure,
            bind_outcome: true,
        },
        contexts,
    ))
}
pub(super) fn empty_graph() -> DecisionGraph {
    DecisionGraph {
        version: CONTRACT_VERSION,
        revision: 0,
        decisions: vec![],
    }
}
pub(super) fn execute<R: RuntimeAdapter>(
    runtime: &R,
    program: &CapturedProgram,
    scenario: &ScenarioSpec,
    graph: &DecisionGraph,
    limits: RuntimeLimits,
    id: &str,
) -> Result<(RunEvidence, Vec<ScopeContext>)> {
    execute_admitted(runtime, program, scenario, graph, limits, id, None)
}
pub(super) fn execute_admitted<R: RuntimeAdapter>(
    runtime: &R,
    program: &CapturedProgram,
    scenario: &ScenarioSpec,
    graph: &DecisionGraph,
    limits: RuntimeLimits,
    id: &str,
    admission: Option<&dyn crate::product_runtime::ReplayAdmission>,
) -> Result<(RunEvidence, Vec<ScopeContext>)> {
    if crate::product_runtime::has_protected_fields(program) && admission.is_none() {
        return Err(DecisionError::Unverified(
            "Scoped scenes require verified source, cohort and completed-result provenance".into(),
        ));
    }
    if let Some(admission) = admission {
        admission.validate_seed(program, &scenario.seed, scenario.clock_day)?;
    }
    runtime.validate(program)?;
    scenario.validate(&program.program)?;
    graph.validate()?;
    let binding = RunBinding {
        source: program.binding.clone(),
        artifact: program.artifact.clone(),
        data_digest: scenario.seed.identity()?,
        input_digest: scenario.input_identity()?,
        scenario_digest: scenario.identity()?,
        decision_digest: graph.identity()?,
        session_digest: scenario.session.identity()?,
        runtime_version: runtime.capabilities().version,
        driver_version: crate::product_runtime::DRIVER_VERSION.into(),
    };
    let mut run = runtime.start(
        program,
        &scenario.seed,
        &scenario.session,
        scenario.clock_day,
        scenario.random_seed,
        limits.clone(),
    )?;
    let mut evidence = RunEvidence {
        version: CONTRACT_VERSION,
        id: id.into(),
        origin: ExecutionOrigin::ProductionRuntime,
        binding,
        source_after: program.binding.clone(),
        state: EvidenceState::Observed,
        trace: vec![],
        observations: vec![],
        limits,
        errors: vec![],
        uncovered: vec![],
    };
    let mut contexts = vec![];
    let mut clock_day = scenario.clock_day;
    let operation_ids = if let Some(admission) = admission {
        admission.replay_operation_ids(program, scenario)?
    } else {
        scenario.replay_operation_ids()?
    };
    for (index, input) in scenario.inputs.iter().enumerate() {
        let before = runtime.data(&run).clone();
        let op = operation_ids[index].clone();
        match runtime.apply(&mut run, input, &op) {
            Ok(step) => evidence.trace.push(step),
            Err(e) => {
                evidence.state = if matches!(
                    e,
                    AdapterError::Cancelled | AdapterError::BudgetExhausted(_)
                ) {
                    EvidenceState::Inconclusive
                } else {
                    EvidenceState::Failed
                };
                evidence.errors.push(format!("{e:?}"));
                break;
            }
        }
        let after = runtime.data(&run);
        if let SemanticInput::AdvanceClock { days } = input {
            clock_day = clock_day
                .checked_add(i32::try_from(*days).map_err(|_| invalid("clock overflow"))?)
                .ok_or_else(|| invalid("clock overflow"))?;
        }
        if let Some(admission) = admission {
            admission.validate_state(program, after, clock_day)?;
        }
        // Actual event provenance resolves UI bindings and collection changes. An
        // action with no identifiable record stays unknown for record-scoped rules.
        for event in after.events.iter().filter(|e| e.operation_id == op) {
            let mut refs: Vec<_> = event
                .changes
                .iter()
                .map(|c| RecordRef {
                    entity: c.entity.clone(),
                    record: c.record.clone(),
                })
                .collect();
            match input {
                SemanticInput::Invoke { arguments, .. }
                | SemanticInput::Submit { arguments, .. } => {
                    for value in arguments.values() {
                        input_references(value, &mut refs);
                    }
                }
                SemanticInput::Activate { row: Some(row), .. } => refs.push(row.clone()),
                _ => {}
            }
            let mut unknown_emission = false;
            if admission.is_some() && !event.outputs.is_empty() {
                if let Some(receipts) = runtime.emitted_record_participants(&run)? {
                    let artifacts = runtime.emitted_artifacts(&run)?;
                    let step = evidence
                        .trace
                        .last()
                        .ok_or_else(|| invalid("emission input step is absent"))?;
                    if receipts.len() != artifacts.len()
                        || event.outputs != step.outputs
                        || event.program != program.artifact.program_digest
                    {
                        return Err(DecisionError::Unverified(
                            "emission participant inventory differs from the actual input event"
                                .into(),
                        ));
                    }
                    let start = artifacts
                        .len()
                        .checked_sub(event.outputs.len())
                        .ok_or_else(|| invalid("emission receipt ordinal is absent"))?;
                    let records: BTreeMap<_, _> = after
                        .records
                        .iter()
                        .map(|row| ((row.entity.as_str(), row.id.as_str()), row))
                        .collect();
                    let mut births = BTreeMap::new();
                    for birth in &after.events {
                        for change in birth
                            .changes
                            .iter()
                            .filter(|change| change.before.is_none())
                        {
                            births
                                .entry((change.entity.as_str(), change.record.as_str()))
                                .and_modify(|entry| *entry = None)
                                .or_insert(Some(&birth.program));
                        }
                    }
                    for (offset, digest) in event.outputs.iter().enumerate() {
                        let ordinal = start + offset;
                        let output = &artifacts[ordinal];
                        if &output.digest != digest {
                            return Err(DecisionError::Unverified(
                                "emission receipt does not name the actual event output".into(),
                            ));
                        }
                        let receipt = &receipts[ordinal];
                        receipt.validate_for(program, &op, ordinal, output, step, clock_day)?;
                        let Some(participants) = receipt.records() else {
                            unknown_emission = true;
                            continue;
                        };
                        let mut emission_refs = Vec::with_capacity(participants.len());
                        let mut authenticated = true;
                        for (participant, output_row) in participants.iter().zip(&output.rows) {
                            let reference = participant.record();
                            let key = (reference.entity.as_str(), reference.record.as_str());
                            let born = births.get(&key).copied().flatten();
                            if !records.get(&key).is_some_and(|row| {
                                &row.created_program == participant.created_program()
                            }) || born != Some(participant.created_program())
                            {
                                authenticated = false;
                            }
                            for (field, value) in output_row {
                                if field.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX)
                                    && field.ends_with("_record")
                                    && value
                                        != &(DataValue::Reference {
                                            entity: reference.entity.clone(),
                                            record: reference.record.clone(),
                                        })
                                {
                                    return Err(DecisionError::Unverified("compiler output provenance disagrees with actual emitted participants".into()));
                                }
                            }
                            emission_refs.push(reference.clone());
                        }
                        if authenticated {
                            refs.extend(emission_refs);
                        } else {
                            // One missing/ambiguous birth invalidates the whole
                            // emission, never just the inconvenient participants.
                            unknown_emission = true;
                        }
                    }
                } else if crate::product_runtime::has_protected_fields(program) {
                    let artifacts = runtime.emitted_artifacts(&run)?;
                    for digest in &event.outputs {
                        let output = artifacts
                            .iter()
                            .find(|output| &output.digest == digest)
                            .ok_or_else(|| {
                                DecisionError::Unverified(
                                    "Emitted scoped output receipt is unavailable".into(),
                                )
                            })?;
                        output.validate()?;
                        for row in &output.rows {
                            for (field, value) in row {
                                if field.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX)
                                    && field.ends_with("_record")
                                {
                                    input_references(value, &mut refs);
                                }
                            }
                        }
                    }
                }
            }
            refs.sort_by(|a, b| (&a.entity, &a.record).cmp(&(&b.entity, &b.record)));
            refs.dedup();
            if refs.is_empty() || unknown_emission {
                contexts.push(ScopeContext {
                    operation: event.action.clone(),
                    record: None,
                    is_new: None,
                    created_generation: None,
                    attributes: Values::new(),
                    predicate_result: None,
                });
            }
            for record in refs {
                let old = before
                    .records
                    .iter()
                    .find(|r| r.entity == record.entity && r.id == record.record);
                let row = old.or_else(|| {
                    after
                        .records
                        .iter()
                        .find(|r| r.entity == record.entity && r.id == record.record)
                });
                let created = after
                    .events
                    .iter()
                    .find(|e| {
                        e.changes.iter().any(|c| {
                            c.entity == record.entity
                                && c.record == record.record
                                && c.before.is_none()
                        })
                    })
                    .map(|e| e.sequence);
                contexts.push(ScopeContext {
                    operation: event.action.clone(),
                    record: Some(record),
                    is_new: Some(old.is_none()),
                    created_generation: created,
                    attributes: row.map(|r| r.values.clone()).unwrap_or_default(),
                    predicate_result: None,
                });
            }
        }
        if let SemanticInput::Observe { point } = input {
            match runtime.observe(&run, point) {
                Ok(o) => evidence.observations.push(o),
                Err(e) => {
                    evidence.state = EvidenceState::Inconclusive;
                    evidence.errors.push(format!("{e:?}"));
                    break;
                }
            }
        }
    }
    if evidence.state == EvidenceState::Observed {
        if evidence.observations.is_empty() {
            evidence.state = EvidenceState::Inconclusive;
            evidence.uncovered.push("No observation point".into());
        }
        for p in &scenario.validity {
            match p.evaluate(&evidence.observations) {
                Some(true) => {}
                Some(false) => {
                    evidence.state = EvidenceState::Failed;
                    evidence
                        .errors
                        .push(format!("Workflow validity failed: {}", p.id));
                }
                None => {
                    evidence.state = EvidenceState::Inconclusive;
                    evidence
                        .uncovered
                        .push(format!("Workflow validity unverified: {}", p.id));
                }
            }
        }
    }
    // Failed adapters need not return a trace step. Completed prefixes still carry
    // truthful evidence; no synthesized output or success fills the missing step.
    evidence.validate()?;
    Ok((evidence, contexts))
}
pub(super) fn assess_scope(scope: &DecisionScope, contexts: &[ScopeContext]) -> ScopeMatch {
    let mut applies = false;
    for c in contexts {
        match scope_match(scope, c).state {
            ScopeMatch::Unknown => return ScopeMatch::Unknown,
            ScopeMatch::Applies => applies = true,
            ScopeMatch::Outside => {}
        }
    }
    if applies {
        ScopeMatch::Applies
    } else {
        ScopeMatch::Outside
    }
}
pub(super) fn same_outcome(
    expected: &[Observation],
    actual: &[Observation],
    mapping: &Mapping,
    before_provenance: &crate::product_store::scope::ProvenanceColumns,
    after_provenance: &crate::product_store::scope::ProvenanceColumns,
) -> Result<Option<bool>> {
    // Missing information does not erase a definite violation elsewhere.
    let mut unknown = expected.len() != actual.len();
    for old in expected {
        let Some(new) = actual.iter().find(|o| o.point == old.point) else {
            unknown = true;
            continue;
        };
        for (key, value) in &old.values {
            let id = mapping.id(SemanticKind::Observable, key);
            match new.values.get(&id) {
                Some(found) if new.value_types.get(&id) == old.value_types.get(key) => {
                    if found != value {
                        return Ok(Some(false));
                    }
                }
                _ => unknown = true,
            }
        }
        if mapping.id(SemanticKind::View, &old.view.view) != new.view.view
            || old.view.rows.iter().map(|r| &r.record).collect::<Vec<_>>()
                != new.view.rows.iter().map(|r| &r.record).collect::<Vec<_>>()
            || old.view.selected != new.view.selected
        {
            return Ok(Some(false));
        }
        match (&old.view_schema, &new.view_schema) {
            (Some(before), Some(after)) => {
                if mapping.id(SemanticKind::Entity, &before.entity) != after.entity {
                    return Ok(Some(false));
                }
                for (column, value_type) in &before.columns {
                    if before_provenance.view(&old.view.view, column) {
                        continue;
                    }
                    match after.columns.get(column) {
                        Some(found) if found != value_type => return Ok(Some(false)),
                        None => unknown = true,
                        _ => {}
                    }
                }
            }
            _ if mapping.requires_view_schema(&old.view.view) => unknown = true,
            _ => {}
        }
        match mapping.availability(
            &old.view.view,
            ActionPlacement::Toolbar,
            &old.view.enabled_actions,
            &new.view.enabled_actions,
        ) {
            Some(false) => return Ok(Some(false)),
            None => unknown = true,
            _ => {}
        }
        for (a, b) in old.view.rows.iter().zip(&new.view.rows) {
            match mapping.availability(
                &old.view.view,
                ActionPlacement::Row,
                &a.enabled_actions,
                &b.enabled_actions,
            ) {
                Some(false) => return Ok(Some(false)),
                None => unknown = true,
                _ => {}
            }
            for (column, value) in &a.cells {
                if before_provenance.view(&old.view.view, column) {
                    continue;
                }
                match b.cells.get(column) {
                    Some(actual) if value != actual => return Ok(Some(false)),
                    None => unknown = true,
                    _ => {}
                }
            }
        }
        for (before, after) in [
            (&old.view.controls, &new.view.controls),
            (&old.view.form_values, &new.view.form_values),
        ] {
            for (key, value) in before {
                match after.get(key) {
                    Some(actual) if value != actual => return Ok(Some(false)),
                    None => unknown = true,
                    _ => {}
                }
            }
        }
        let old_order: Vec<_> = old
            .outputs
            .iter()
            .map(|o| mapping.id(SemanticKind::Output, &o.output))
            .filter(|id| new.outputs.iter().any(|o| &o.output == id))
            .collect();
        let new_order: Vec<_> = new
            .outputs
            .iter()
            .map(|o| o.output.clone())
            .filter(|id| {
                old.outputs
                    .iter()
                    .any(|o| mapping.id(SemanticKind::Output, &o.output) == *id)
            })
            .collect();
        if old_order != new_order {
            return Ok(Some(false));
        }
        for output in &old.outputs {
            let id = mapping.id(SemanticKind::Output, &output.output);
            let matches: Vec<_> = new.outputs.iter().filter(|o| o.output == id).collect();
            let expected: Vec<_> = old
                .outputs
                .iter()
                .filter(|o| o.output == output.output)
                .collect();
            if matches.is_empty() {
                unknown = true;
                continue;
            }
            if matches.len() != expected.len() {
                return Ok(Some(false));
            }
            for (a, b) in expected.into_iter().zip(matches) {
                if a.format != b.format
                    || a.rows
                        .iter()
                        .map(|row| {
                            row.iter()
                                .filter(|(column, _)| !before_provenance.output(&a.output, column))
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                        != b.rows
                            .iter()
                            .map(|row| {
                                row.iter()
                                    .filter(|(column, _)| {
                                        !after_provenance.output(&b.output, column)
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>()
                    || a.columns
                        .iter()
                        .filter(|c| !before_provenance.output(&a.output, &c.id))
                        .map(|c| (&c.id, &c.value_type))
                        .collect::<Vec<_>>()
                        != b.columns
                            .iter()
                            .filter(|c| !after_provenance.output(&b.output, &c.id))
                            .map(|c| (&c.id, &c.value_type))
                            .collect::<Vec<_>>()
                {
                    return Ok(Some(false));
                }
            }
        }
        if new.outputs.iter().any(|o| {
            !old.outputs
                .iter()
                .any(|old| mapping.id(SemanticKind::Output, &old.output) == o.output)
        }) {
            return Ok(Some(false));
        }
    }
    Ok(if unknown { None } else { Some(true) })
}

fn input_references(value: &DataValue, refs: &mut Vec<RecordRef>) {
    match value {
        DataValue::Reference { entity, record } => refs.push(RecordRef {
            entity: entity.clone(),
            record: record.clone(),
        }),
        DataValue::List { items, .. } => {
            for item in items {
                input_references(item, refs);
            }
        }
        _ => {}
    }
}
