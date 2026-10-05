use crate::tool_project::{
    BehaviorPolicy, DateDuePolicy, Evidence, EvidenceObservation, EvidenceSource, EvidenceStatus,
    FieldRole, FieldValue, ProjectSnapshot, RecordEvent, RecordEventKind, ReminderObservation,
    ReminderPolicy, Scenario, ScenarioStep, StageClock, TimerPolicy, ToolCommand, WorkRecord,
    MAX_EVENTS, MAX_RECORDS, RUNTIME_SEMANTICS_VERSION,
};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use ring::digest::{digest, SHA256};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordEvaluation {
    pub record_id: String,
    pub current_stage_id: String,
    pub elapsed_work_days: Option<i64>,
    pub paused_days: i64,
    pub original_due_date: Option<NaiveDate>,
    pub display_due_date: Option<NaiveDate>,
    pub reminder: Option<NextStepReminder>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextStepReminder {
    FollowUpWhileWaiting {
        waiting_days: i64,
        days_until_due: Option<i64>,
    },
}

impl From<&RecordEvaluation> for EvidenceObservation {
    fn from(result: &RecordEvaluation) -> Self {
        Self {
            record_id: result.record_id.clone(),
            current_stage_id: result.current_stage_id.clone(),
            elapsed_work_days: result.elapsed_work_days,
            paused_days: result.paused_days,
            original_due_date: result.original_due_date,
            display_due_date: result.display_due_date,
            reminder: result.reminder.as_ref().map(|reminder| match reminder {
                NextStepReminder::FollowUpWhileWaiting {
                    waiting_days,
                    days_until_due,
                } => ReminderObservation::FollowUpWhileWaiting {
                    waiting_days: *waiting_days,
                    days_until_due: *days_until_due,
                },
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    ProjectValidation(crate::tool_project::ProjectValidationError),
    GenerationConflict { expected: u64, actual: u64 },
    RecordNotFound(String),
    DuplicateRecord(String),
    InvalidTransition { from: String, to: String },
    FutureEvent { event_id: String },
    InvalidClock(String),
    RecordRevisionConflict { expected: u64, actual: u64 },
    InvalidOperation(String),
    NoChange,
    DateOverflow,
    Serialization(String),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProjectValidation(error) => write!(f, "{error}"),
            Self::GenerationConflict { expected, actual } => write!(
                f,
                "project changed: expected generation {expected}, found {actual}"
            ),
            Self::RecordNotFound(record_id) => write!(f, "record {record_id} was not found"),
            Self::DuplicateRecord(record_id) => write!(f, "record {record_id} already exists"),
            Self::InvalidTransition { from, to } => {
                write!(f, "transition from {from} to {to} is not allowed")
            }
            Self::FutureEvent { event_id } => {
                write!(f, "event {event_id} is later than the fixed clock")
            }
            Self::InvalidClock(message) => write!(f, "invalid fixed clock: {message}"),
            Self::RecordRevisionConflict { expected, actual } => write!(
                f,
                "record changed: expected revision {expected}, found {actual}"
            ),
            Self::InvalidOperation(message) => write!(f, "invalid operation: {message}"),
            Self::NoChange => write!(f, "operation does not change the current project"),
            Self::DateOverflow => {
                write!(f, "calendar date calculation exceeds the supported range")
            }
            Self::Serialization(message) => write!(f, "cannot fingerprint scenario: {message}"),
        }
    }
}

impl std::error::Error for RuntimeError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEvidenceCheck {
    Current,
    StaleRuntime,
}

impl From<crate::tool_project::ProjectValidationError> for RuntimeError {
    fn from(value: crate::tool_project::ProjectValidationError) -> Self {
        Self::ProjectValidation(value)
    }
}

pub fn apply_command(
    snapshot: &ProjectSnapshot,
    command: &ToolCommand,
    now: DateTime<FixedOffset>,
) -> Result<ProjectSnapshot, RuntimeError> {
    snapshot.validate()?;
    let (operation_id, expected_generation, occurred_at) = match command {
        ToolCommand::CreateRecord {
            operation_id,
            expected_generation,
            occurred_at,
            ..
        }
        | ToolCommand::EditRecord {
            operation_id,
            expected_generation,
            occurred_at,
            ..
        }
        | ToolCommand::TransitionStage {
            operation_id,
            expected_generation,
            occurred_at,
            ..
        }
        | ToolCommand::SetBehavior {
            operation_id,
            expected_generation,
            occurred_at,
            ..
        } => (operation_id, expected_generation, occurred_at),
    };
    if !valid_stable_id(operation_id) {
        return Err(RuntimeError::InvalidOperation(
            "operation ID must be a stable lowercase ID".to_owned(),
        ));
    }
    if *expected_generation != snapshot.generation {
        return Err(RuntimeError::GenerationConflict {
            expected: *expected_generation,
            actual: snapshot.generation,
        });
    }
    if *occurred_at > now {
        return Err(RuntimeError::FutureEvent {
            event_id: operation_id.clone(),
        });
    }
    if now < snapshot.updated_at || *occurred_at < snapshot.created_at {
        return Err(RuntimeError::InvalidClock(
            "operation time must not move the project backwards or precede project creation"
                .to_owned(),
        ));
    }

    let mut next = snapshot.clone();
    match command {
        ToolCommand::CreateRecord {
            operation_id,
            record_id,
            initial_stage_id,
            values,
            occurred_at,
            ..
        } => {
            if !valid_stable_id(record_id) {
                return Err(RuntimeError::InvalidOperation(
                    "record ID must be a stable lowercase ID".to_owned(),
                ));
            }
            if next
                .records
                .iter()
                .any(|record| record.record_id == *record_id)
            {
                return Err(RuntimeError::DuplicateRecord(record_id.clone()));
            }
            if next.records.len() >= MAX_RECORDS || next.event_history.len() >= MAX_EVENTS {
                return Err(RuntimeError::InvalidOperation(
                    "project reached the supported record or event limit".to_owned(),
                ));
            }
            let spec = next
                .active_spec()
                .ok_or_else(|| RuntimeError::InvalidOperation("active spec is missing".to_owned()))?
                .clone();
            if spec.stage(initial_stage_id).is_none() {
                return Err(RuntimeError::InvalidOperation(format!(
                    "initial stage {initial_stage_id} is not defined"
                )));
            }
            ProjectSnapshot::validate_values(&spec, values)?;
            let sequence = next.event_sequence + 1;
            next.records.push(WorkRecord {
                record_id: record_id.clone(),
                spec: (&spec).into(),
                record_revision: 1,
                created_sequence: sequence,
                created_at: occurred_at.clone(),
                typed_values: values.clone(),
                current_stage_id: initial_stage_id.clone(),
                extensions: BTreeMap::new(),
            });
            next.event_history.push(RecordEvent {
                event_id: operation_id.clone(),
                record_id: record_id.clone(),
                sequence,
                record_revision: 1,
                occurred_at: occurred_at.clone(),
                kind: RecordEventKind::Created {
                    initial_stage_id: initial_stage_id.clone(),
                    initial_values: values.clone(),
                },
                extensions: BTreeMap::new(),
            });
            next.event_sequence = sequence;
        }
        ToolCommand::EditRecord {
            operation_id,
            record_id,
            expected_record_revision,
            changes,
            occurred_at,
            ..
        } => {
            if changes.is_empty() {
                return Err(RuntimeError::NoChange);
            }
            let record_index = find_record_index(&next, record_id)?;
            let record = &next.records[record_index];
            check_record_revision(record.record_revision, *expected_record_revision)?;
            validate_event_time(&next, record, operation_id, occurred_at)?;
            let spec = spec_for_record(&next, record)?.clone();
            let mut values = record.typed_values.clone();
            let mut event_changes = Vec::with_capacity(changes.len());
            for (field_id, after) in changes {
                let field = spec.field(field_id).ok_or_else(|| {
                    RuntimeError::InvalidOperation(format!("field {field_id} is not defined"))
                })?;
                if after
                    .as_ref()
                    .is_some_and(|value| !field_accepts(field, value))
                {
                    return Err(RuntimeError::ProjectValidation(
                        crate::tool_project::ProjectValidationError {
                            code: crate::tool_project::ValidationCode::InvalidFieldValue,
                            path: format!("values.{field_id}"),
                            message: "field value does not match its declared type".to_owned(),
                        },
                    ));
                }
                let before = values.get(field_id).cloned();
                if before == *after {
                    continue;
                }
                match after {
                    Some(value) => {
                        values.insert(field_id.clone(), value.clone());
                    }
                    None => {
                        values.remove(field_id);
                    }
                }
                event_changes.push(crate::tool_project::FieldValueChange {
                    field_id: field_id.clone(),
                    before,
                    after: after.clone(),
                });
            }
            if event_changes.is_empty() {
                return Err(RuntimeError::NoChange);
            }
            ProjectSnapshot::validate_values(&spec, &values)?;
            let sequence = next.event_sequence + 1;
            let record = &mut next.records[record_index];
            record.record_revision += 1;
            record.typed_values = values;
            next.event_history.push(RecordEvent {
                event_id: operation_id.clone(),
                record_id: record_id.clone(),
                sequence,
                record_revision: record.record_revision,
                occurred_at: occurred_at.clone(),
                kind: RecordEventKind::FieldsChanged {
                    changes: event_changes,
                },
                extensions: BTreeMap::new(),
            });
            next.event_sequence = sequence;
        }
        ToolCommand::TransitionStage {
            operation_id,
            record_id,
            expected_record_revision,
            to_stage_id,
            occurred_at,
            ..
        } => {
            let record_index = find_record_index(&next, record_id)?;
            let record = &next.records[record_index];
            check_record_revision(record.record_revision, *expected_record_revision)?;
            validate_event_time(&next, record, operation_id, occurred_at)?;
            let spec = spec_for_record(&next, record)?;
            let from = spec
                .stage(&record.current_stage_id)
                .expect("validated record stage exists");
            if from.terminal
                || from.id == *to_stage_id
                || !from
                    .allowed_next_stage_ids
                    .iter()
                    .any(|stage_id| stage_id == to_stage_id)
            {
                return Err(RuntimeError::InvalidTransition {
                    from: from.id.clone(),
                    to: to_stage_id.clone(),
                });
            }
            let sequence = next.event_sequence + 1;
            let record = &mut next.records[record_index];
            let from_stage_id = record.current_stage_id.clone();
            record.current_stage_id = to_stage_id.clone();
            record.record_revision += 1;
            next.event_history.push(RecordEvent {
                event_id: operation_id.clone(),
                record_id: record_id.clone(),
                sequence,
                record_revision: record.record_revision,
                occurred_at: occurred_at.clone(),
                kind: RecordEventKind::StageChanged {
                    from_stage_id,
                    to_stage_id: to_stage_id.clone(),
                },
                extensions: BTreeMap::new(),
            });
            next.event_sequence = sequence;
        }
        ToolCommand::SetBehavior {
            operation_id,
            revision_id,
            parent_revision_id,
            policy,
            reason,
            occurred_at,
            ..
        } => {
            if next
                .event_history
                .iter()
                .any(|event| event.event_id == *operation_id)
                || next
                    .behavior_revisions
                    .iter()
                    .any(|revision| revision.operation_id.as_deref() == Some(operation_id.as_str()))
            {
                return Err(RuntimeError::InvalidOperation(
                    "behavior command operation ID has already been used".to_owned(),
                ));
            }
            if !valid_stable_id(revision_id)
                || next
                    .behavior_revisions
                    .iter()
                    .any(|revision| revision.revision_id == *revision_id)
                || parent_revision_id.as_deref() != Some(&next.active_behavior_revision_id)
                || reason.trim().is_empty()
                || reason.len() > 16_384
            {
                return Err(RuntimeError::InvalidOperation(
                    "behavior revision must have a unique ID, a current parent, and a reason"
                        .to_owned(),
                ));
            }
            policy.validate()?;
            let parent = next
                .behavior_revision(&next.active_behavior_revision_id)
                .expect("active behavior revision was validated");
            if occurred_at < &parent.created_at {
                return Err(RuntimeError::InvalidClock(
                    "behavior revisions cannot precede their parent revision".to_owned(),
                ));
            }
            next.behavior_revisions
                .push(crate::tool_project::BehaviorRevision {
                    revision_id: revision_id.clone(),
                    operation_id: Some(operation_id.clone()),
                    parent_revision_id: parent_revision_id.clone(),
                    policy: policy.clone(),
                    reason: reason.clone(),
                    created_at: occurred_at.clone(),
                });
            next.active_behavior_revision_id = revision_id.clone();
        }
    }
    next.updated_at = now;
    next.validate()?;
    Ok(next)
}

pub fn evaluate_record(
    snapshot: &ProjectSnapshot,
    record_id: &str,
    policy: &BehaviorPolicy,
    as_of_date: NaiveDate,
) -> Result<RecordEvaluation, RuntimeError> {
    snapshot.validate()?;
    policy.validate()?;
    let record = snapshot
        .records
        .iter()
        .find(|record| record.record_id == record_id)
        .ok_or_else(|| RuntimeError::RecordNotFound(record_id.to_owned()))?;
    let spec = spec_for_record(snapshot, record)?;
    let created_date = to_calendar_date(
        &record.created_at,
        snapshot.date_settings.calendar_utc_offset_seconds,
    )?;
    if as_of_date < created_date {
        return Err(RuntimeError::InvalidClock(
            "as-of date cannot precede record creation".to_owned(),
        ));
    }
    let work_start = spec
        .field_for_role(FieldRole::WorkStartedOn)
        .and_then(|field| record.typed_values.get(&field.id))
        .and_then(|value| match value {
            FieldValue::Date(date) => Some(*date),
            _ => None,
        });
    if work_start.is_some_and(|date| date > as_of_date) {
        return Err(RuntimeError::InvalidClock(
            "work start date cannot be after the fixed as-of date".to_owned(),
        ));
    }
    let range_start = work_start.unwrap_or(created_date);
    let record_events: Vec<_> = snapshot
        .event_history
        .iter()
        .filter(|event| event.record_id == record_id)
        .collect();
    let first = record_events
        .first()
        .expect("snapshot validation ensures a creation event");
    let mut current_stage_id = match &first.kind {
        RecordEventKind::Created {
            initial_stage_id, ..
        } => initial_stage_id.clone(),
        _ => unreachable!("snapshot validation requires the first event to create the record"),
    };
    let mut work_cursor = range_start;
    let mut paused_cursor = created_date;
    let mut active_days = 0_i64;
    let mut paused_days = 0_i64;
    let mut current_pause_start = spec
        .stage(&current_stage_id)
        .is_some_and(|stage| stage.clock == StageClock::Paused)
        .then_some(created_date);

    for event in record_events.iter().skip(1) {
        let event_date = to_calendar_date(
            &event.occurred_at,
            snapshot.date_settings.calendar_utc_offset_seconds,
        )?;
        if event_date > as_of_date {
            return Err(RuntimeError::FutureEvent {
                event_id: event.event_id.clone(),
            });
        }
        let RecordEventKind::StageChanged {
            from_stage_id,
            to_stage_id,
        } = &event.kind
        else {
            continue;
        };
        if event_date >= range_start {
            accumulate_work_interval(
                spec.stage(&current_stage_id)
                    .expect("snapshot validation requires a known stage"),
                work_cursor,
                event_date,
                policy.timer,
                &mut active_days,
            );
            work_cursor = work_cursor.max(event_date);
        }
        accumulate_paused_interval(
            spec.stage(&current_stage_id)
                .expect("snapshot validation requires a known stage"),
            paused_cursor,
            event_date,
            &mut paused_days,
        );
        paused_cursor = paused_cursor.max(event_date);
        if spec.stage(from_stage_id).is_none() || spec.stage(to_stage_id).is_none() {
            return Err(RuntimeError::InvalidTransition {
                from: from_stage_id.clone(),
                to: to_stage_id.clone(),
            });
        }
        current_stage_id = to_stage_id.clone();
        if spec
            .stage(&current_stage_id)
            .is_some_and(|stage| stage.clock == StageClock::Paused)
        {
            current_pause_start = Some(event_date);
        } else {
            current_pause_start = None;
        }
    }
    accumulate_work_interval(
        spec.stage(&current_stage_id)
            .expect("snapshot validation requires a known stage"),
        work_cursor,
        as_of_date,
        policy.timer,
        &mut active_days,
    );
    accumulate_paused_interval(
        spec.stage(&current_stage_id)
            .expect("snapshot validation requires a known stage"),
        paused_cursor,
        as_of_date,
        &mut paused_days,
    );
    if current_stage_id != record.current_stage_id {
        return Err(RuntimeError::InvalidOperation(
            "record state and event history disagree".to_owned(),
        ));
    }
    let current_waiting_days = if spec
        .stage(&current_stage_id)
        .is_some_and(|stage| stage.clock == StageClock::Paused)
    {
        current_pause_start
            .unwrap_or(created_date)
            .signed_duration_since(as_of_date)
            .num_days()
            .checked_neg()
            .ok_or(RuntimeError::DateOverflow)?
            .max(0)
    } else {
        0
    };

    let original_due_date = spec
        .field_for_role(FieldRole::PromisedDate)
        .and_then(|field| record.typed_values.get(&field.id))
        .and_then(|value| match value {
            FieldValue::Date(date) => Some(*date),
            _ => None,
        });
    let display_due_date = match (original_due_date, policy.due_date) {
        (Some(date), DateDuePolicy::KeepOriginal) => Some(date),
        (Some(date), DateDuePolicy::ExtendByPausedDays) => Some(
            date.checked_add_signed(Duration::days(paused_days))
                .ok_or(RuntimeError::DateOverflow)?,
        ),
        (None, _) => None,
    };
    let stage_is_paused = spec
        .stage(&current_stage_id)
        .is_some_and(|stage| stage.clock == StageClock::Paused);
    let days_until_due =
        display_due_date.map(|date| date.signed_duration_since(as_of_date).num_days());
    let reminder = if stage_is_paused {
        match policy.reminder {
            ReminderPolicy::Never => None,
            ReminderPolicy::WaitingBeforeDue { days_before_due } => days_until_due
                .filter(|days| *days >= 0 && *days <= i64::from(days_before_due))
                .map(|days| NextStepReminder::FollowUpWhileWaiting {
                    waiting_days: current_waiting_days,
                    days_until_due: Some(days),
                }),
            ReminderPolicy::WaitingAfterDays { days_waiting }
                if current_waiting_days >= i64::from(days_waiting) =>
            {
                Some(NextStepReminder::FollowUpWhileWaiting {
                    waiting_days: current_waiting_days,
                    days_until_due,
                })
            }
            ReminderPolicy::WaitingAfterDays { .. } => None,
        }
    } else {
        None
    };
    Ok(RecordEvaluation {
        record_id: record.record_id.clone(),
        current_stage_id,
        elapsed_work_days: work_start.map(|_| active_days),
        paused_days,
        original_due_date,
        display_due_date,
        reminder,
    })
}

pub fn run_scenario(scenario: &Scenario) -> Result<Evidence, RuntimeError> {
    scenario.validate()?;
    if scenario.records.len() > MAX_RECORDS || scenario.event_history.len() > MAX_EVENTS {
        return Err(RuntimeError::InvalidOperation(
            "scenario exceeds the supported record or event limit".to_owned(),
        ));
    }
    let mut snapshot = scenario.base_snapshot()?;
    let fixed_clock_date = to_calendar_date(
        &scenario.fixed_now,
        scenario.date_settings.calendar_utc_offset_seconds,
    )?;
    if scenario.as_of_date > fixed_clock_date {
        return Err(RuntimeError::InvalidClock(
            "scenario as-of date cannot exceed the project-local fixed clock date".to_owned(),
        ));
    }

    let mut as_of_date = scenario.as_of_date;
    let mut observations = Vec::new();
    for (index, step) in scenario.steps.iter().enumerate() {
        match step {
            ScenarioStep::CreateRecord {
                operation_id,
                record_id,
                initial_stage_id,
                values,
                occurred_at,
            } => {
                let command = ToolCommand::CreateRecord {
                    operation_id: operation_id.clone(),
                    expected_generation: snapshot.generation,
                    record_id: record_id.clone(),
                    initial_stage_id: initial_stage_id.clone(),
                    values: values.clone(),
                    occurred_at: occurred_at.clone(),
                };
                snapshot = apply_command(&snapshot, &command, scenario.fixed_now)?;
            }
            ScenarioStep::EditRecord {
                operation_id,
                record_id,
                expected_record_revision,
                changes,
                occurred_at,
            } => {
                let command = ToolCommand::EditRecord {
                    operation_id: operation_id.clone(),
                    expected_generation: snapshot.generation,
                    record_id: record_id.clone(),
                    expected_record_revision: *expected_record_revision,
                    changes: changes.clone(),
                    occurred_at: occurred_at.clone(),
                };
                snapshot = apply_command(&snapshot, &command, scenario.fixed_now)?;
            }
            ScenarioStep::TransitionStage {
                operation_id,
                record_id,
                expected_record_revision,
                to_stage_id,
                occurred_at,
            } => {
                let command = ToolCommand::TransitionStage {
                    operation_id: operation_id.clone(),
                    expected_generation: snapshot.generation,
                    record_id: record_id.clone(),
                    expected_record_revision: *expected_record_revision,
                    to_stage_id: to_stage_id.clone(),
                    occurred_at: occurred_at.clone(),
                };
                snapshot = apply_command(&snapshot, &command, scenario.fixed_now)?;
            }
            ScenarioStep::AdvanceDate {
                as_of_date: next_date,
            } => {
                if *next_date < as_of_date || *next_date > fixed_clock_date {
                    return Err(RuntimeError::InvalidClock(
                        "scenario dates must move forward and cannot exceed its fixed clock date"
                            .to_owned(),
                    ));
                }
                as_of_date = *next_date;
            }
            ScenarioStep::Observe { record_id } => {
                observations.push(EvidenceObservation::from(&evaluate_record(
                    &snapshot,
                    record_id,
                    &scenario.candidate_policy,
                    as_of_date,
                )?));
            }
        }
        if index >= crate::tool_project::MAX_SCENARIO_STEPS {
            return Err(RuntimeError::InvalidOperation(
                "scenario exceeded its supported step limit".to_owned(),
            ));
        }
    }
    if !scenario
        .steps
        .iter()
        .any(|step| matches!(step, ScenarioStep::Observe { .. }))
    {
        for record in &snapshot.records {
            observations.push(EvidenceObservation::from(&evaluate_record(
                &snapshot,
                &record.record_id,
                &scenario.candidate_policy,
                as_of_date,
            )?));
        }
    }
    let status = if scenario.expected.is_empty() {
        EvidenceStatus::Executed
    } else if scenario.expected == observations {
        EvidenceStatus::Passed
    } else {
        EvidenceStatus::Failed
    };
    let errors = if status == EvidenceStatus::Failed {
        vec!["actual observations did not match the scenario expectations".to_owned()]
    } else {
        Vec::new()
    };
    let mut input = serde_json::to_value(scenario)
        .map_err(|error| RuntimeError::Serialization(error.to_string()))?;
    if let serde_json::Value::Object(fields) = &mut input {
        fields.remove("candidate_policy");
    }
    let input_fingerprint = fingerprint(&input)?;
    let candidate_fingerprint = fingerprint(&scenario.candidate_policy)?;
    let run_identity = (
        scenario.scenario_id.as_str(),
        RUNTIME_SEMANTICS_VERSION,
        &input_fingerprint,
        &candidate_fingerprint,
    );
    Ok(Evidence {
        run_id: format!("run-{}", fingerprint(&run_identity)?),
        scenario_id: scenario.scenario_id.clone(),
        status,
        observations,
        expected: scenario.expected.clone(),
        input_fingerprint,
        candidate_fingerprint,
        runtime_semantics_version: RUNTIME_SEMANTICS_VERSION,
        source: EvidenceSource::NativeExecution,
        errors,
        completed_at: scenario.fixed_now,
        extensions: BTreeMap::new(),
    })
}

/// Replays evidence produced by this runtime version. Older or newer evidence
/// remains readable as stale history, but callers must not treat it as current.
pub fn validate_native_evidence(
    snapshot: &ProjectSnapshot,
) -> Result<NativeEvidenceCheck, RuntimeError> {
    let mut stale_runtime = false;
    for evidence in &snapshot.evidence {
        if evidence.source != EvidenceSource::NativeExecution {
            continue;
        }
        if evidence.runtime_semantics_version != RUNTIME_SEMANTICS_VERSION {
            stale_runtime = true;
            continue;
        }
        let scenario = snapshot
            .scenarios
            .iter()
            .find(|scenario| scenario.scenario_id == evidence.scenario_id)
            .ok_or_else(|| {
                RuntimeError::InvalidOperation(
                    "native evidence references a missing scenario".to_owned(),
                )
            })?;
        let expected = run_scenario(scenario)?;
        let mut stored = evidence.clone();
        stored.extensions.clear();
        if stored != expected {
            return Err(RuntimeError::InvalidOperation(
                "native evidence differs from the local scenario replay".to_owned(),
            ));
        }
    }
    Ok(if stale_runtime {
        NativeEvidenceCheck::StaleRuntime
    } else {
        NativeEvidenceCheck::Current
    })
}

fn accumulate_work_interval(
    stage: &crate::tool_project::StageDefinition,
    start: NaiveDate,
    end: NaiveDate,
    timer: TimerPolicy,
    active_days: &mut i64,
) {
    let days = end.signed_duration_since(start).num_days().max(0);
    match stage.clock {
        StageClock::Active => *active_days += days,
        StageClock::Paused if timer == TimerPolicy::CountPausedStages => {
            *active_days += days;
        }
        StageClock::Paused | StageClock::Stopped => {}
    }
}

fn accumulate_paused_interval(
    stage: &crate::tool_project::StageDefinition,
    start: NaiveDate,
    end: NaiveDate,
    paused_days: &mut i64,
) {
    if stage.clock == StageClock::Paused {
        *paused_days += end.signed_duration_since(start).num_days().max(0);
    }
}

fn find_record_index(snapshot: &ProjectSnapshot, record_id: &str) -> Result<usize, RuntimeError> {
    snapshot
        .records
        .iter()
        .position(|record| record.record_id == record_id)
        .ok_or_else(|| RuntimeError::RecordNotFound(record_id.to_owned()))
}

fn spec_for_record<'a>(
    snapshot: &'a ProjectSnapshot,
    record: &WorkRecord,
) -> Result<&'a crate::tool_project::ToolSpec, RuntimeError> {
    snapshot
        .spec_revisions
        .iter()
        .find(|spec| spec.spec_id == record.spec.spec_id && spec.revision == record.spec.revision)
        .ok_or_else(|| RuntimeError::InvalidOperation("record spec revision is missing".to_owned()))
}

fn check_record_revision(actual: u64, expected: u64) -> Result<(), RuntimeError> {
    if actual != expected {
        return Err(RuntimeError::RecordRevisionConflict { expected, actual });
    }
    Ok(())
}

fn validate_event_time(
    snapshot: &ProjectSnapshot,
    record: &WorkRecord,
    operation_id: &str,
    occurred_at: &DateTime<FixedOffset>,
) -> Result<(), RuntimeError> {
    if *occurred_at < record.created_at
        || snapshot
            .event_history
            .iter()
            .filter(|event| event.record_id == record.record_id)
            .any(|event| event.occurred_at > *occurred_at)
    {
        return Err(RuntimeError::InvalidClock(
            "record event timestamps must follow the record's existing history".to_owned(),
        ));
    }
    if snapshot.event_history.len() >= MAX_EVENTS || !valid_stable_id(operation_id) {
        return Err(RuntimeError::InvalidOperation(
            "event limit or stable operation ID constraint was violated".to_owned(),
        ));
    }
    Ok(())
}

fn to_calendar_date(
    at: &DateTime<FixedOffset>,
    offset_seconds: i32,
) -> Result<NaiveDate, RuntimeError> {
    let offset = FixedOffset::east_opt(offset_seconds).ok_or_else(|| {
        RuntimeError::InvalidClock("project calendar offset is invalid".to_owned())
    })?;
    Ok(at.with_timezone(&offset).date_naive())
}

fn field_accepts(field: &crate::tool_project::FieldDefinition, value: &FieldValue) -> bool {
    match (&field.kind, value) {
        (crate::tool_project::FieldKind::Text, FieldValue::Text(_))
        | (crate::tool_project::FieldKind::Date, FieldValue::Date(_))
        | (crate::tool_project::FieldKind::Integer, FieldValue::Integer(_))
        | (crate::tool_project::FieldKind::Boolean, FieldValue::Boolean(_)) => true,
        (crate::tool_project::FieldKind::Enum { options }, FieldValue::Enum(value)) => {
            options.contains(value)
        }
        _ => false,
    }
}

fn valid_stable_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= crate::tool_project::MAX_ID_LENGTH
        && value.as_bytes()[0].is_ascii_lowercase()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
}

fn fingerprint(value: &impl Serialize) -> Result<String, RuntimeError> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| RuntimeError::Serialization(error.to_string()))?;
    Ok(digest(&SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
