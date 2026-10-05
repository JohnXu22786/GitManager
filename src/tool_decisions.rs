//! Local, scoped rule changes over the shared project/runtime contracts.
//!
//! This module never writes files. A prepared change retains the source generation
//! and is submitted unchanged to `ProjectStore::commit`; the store owns locking,
//! receipts and generation advancement. Retain that exact candidate after a lost
//! reply and retry it with the same operation ID. Do not rebuild it from new data.
//! Daily views must use `evaluate_bound_record`, not the project's fallback policy.

use crate::tool_project::*;
use crate::tool_runtime::{evaluate_record, run_scenario, RecordEvaluation, RuntimeError};
use chrono::{DateTime, FixedOffset, NaiveDate};
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const INTENT_KEY: &str = "tool_decisions_intent";
const TARGET_KEY: &str = "tool_decisions_target";
const RULE_KEYS: [RuleKey; 3] = [RuleKey::Timer, RuleKey::DeliveryTarget, RuleKey::Reminder];

/// A future-record case explicitly uses one copied record as an example of the
/// future default. It never makes that example an affected existing record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RehearsalTarget {
    Record(String),
    FutureRecord(String),
}

impl RehearsalTarget {
    fn record_id(&self) -> &str {
        match self {
            Self::Record(id) | Self::FutureRecord(id) => id,
        }
    }
}

/// Cases use the shared scenario steps on validated copied inputs, which may be
/// edited or desensitized without changing live facts. Each case observes one
/// record (possibly at several points); use separate cases for other records.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RehearsalCase {
    pub scenario: Scenario,
    pub target: RehearsalTarget,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRequest {
    pub request_id: String,
    pub operation_id: String,
    pub decision_id: String,
    pub revision_id: String,
    pub rule_keys: Vec<RuleKey>,
    pub candidate_policy: BehaviorPolicy,
    pub scope: ScopeSelection,
    pub original_request: String,
    pub rationale: String,
    pub unresolved_questions: Vec<String>,
    pub supersedes: Vec<String>,
    pub cases: Vec<RehearsalCase>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordImpact {
    pub record_id: String,
    pub before: EvidenceObservation,
    pub after: EvidenceObservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionConflict {
    pub decision_id: String,
    pub scenario_id: Option<String>,
    pub message: String,
    pub expected: Vec<EvidenceObservation>,
    pub actual: Vec<EvidenceObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangePreview {
    pub request_id: String,
    pub project_id: String,
    pub generation: u64,
    pub scope: ScopeSelection,
    pub impacts: Vec<RecordImpact>,
    pub future_policy: BehaviorPolicy,
    pub scenarios: Vec<Scenario>,
    pub evidence: Vec<Evidence>,
    pub checked_decision_ids: Vec<String>,
    pub conflicts: Vec<DecisionConflict>,
    fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreservedBinding {
    pub record_id: Option<String>,
    pub rule_key: RuleKey,
    pub behavior_revision_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WithdrawalPreview {
    pub request_id: String,
    pub project_id: String,
    pub generation: u64,
    pub revision_id: String,
    pub operation_id: String,
    pub preserved_record_count: usize,
    pub preserved_event_count: usize,
    pub impacts: Vec<RecordImpact>,
    pub future_policy: BehaviorPolicy,
    pub preserved_later_bindings: Vec<PreservedBinding>,
    pub checked_decision_ids: Vec<String>,
    pub conflicts: Vec<DecisionConflict>,
    fingerprint: String,
    candidate: ProjectSnapshot,
}

/// Immutable candidate and commit identity. These accessors are the complete
/// persistence handoff; neither generation nor receipts are changed here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedChange {
    operation_id: String,
    snapshot: ProjectSnapshot,
}

impl PreparedChange {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn expected_generation(&self) -> u64 {
        self.snapshot.generation
    }
    pub fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecisionError {
    InvalidRequest(String),
    StaleEvidence,
    Conflicts(Vec<DecisionConflict>),
    Runtime(RuntimeError),
    Validation(ProjectValidationError),
}

impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "{message}"),
            Self::StaleEvidence => write!(f, "the context changed; rehearse again before adopting"),
            Self::Conflicts(conflicts) => {
                write!(f, "{} confirmed decisions conflict", conflicts.len())
            }
            Self::Runtime(error) => write!(f, "{error}"),
            Self::Validation(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for DecisionError {}
impl From<RuntimeError> for DecisionError {
    fn from(e: RuntimeError) -> Self {
        Self::Runtime(e)
    }
}
impl From<ProjectValidationError> for DecisionError {
    fn from(e: ProjectValidationError) -> Self {
        Self::Validation(e)
    }
}

// Versioned metadata lives in the already-supported, losslessly preserved
// extension fields. No legacy project/task format or core contract is changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionIntent {
    version: u32,
    original_request: String,
    rule_keys: Vec<RuleKey>,
    behavior_revision_id: Option<String>,
    baseline_binding_count: usize,
    fallback_revision_id: String,
    targets: BTreeMap<String, RehearsalTarget>,
    withdraws_revision_id: Option<String>,
}

pub fn decision_original_request(decision: &DecisionRecord) -> Result<String, DecisionError> {
    Ok(read_intent(decision)?.original_request)
}

/// Resolve undo from the recorded intent, never by inventing a revision ID from
/// a UI label. Pending choices and withdrawal receipts have no adopted revision.
pub fn decision_behavior_revision_id(
    decision: &DecisionRecord,
) -> Result<Option<String>, DecisionError> {
    Ok(read_intent(decision)?.behavior_revision_id)
}

pub fn freeze_scope(
    snapshot: &ProjectSnapshot,
    kind: ScopeKind,
    selected_record_id: Option<&str>,
) -> Result<ScopeSelection, DecisionError> {
    snapshot.validate()?;
    let mut ids = match kind {
        ScopeKind::SingleRecord => {
            let id = selected_record_id.ok_or_else(|| invalid("select a record for this scope"))?;
            if !snapshot.records.iter().any(|record| record.record_id == id) {
                return Err(invalid("the selected record does not exist"));
            }
            vec![id.to_owned()]
        }
        ScopeKind::FutureRecords => Vec::new(),
        ScopeKind::IncompleteAndFuture => snapshot
            .records
            .iter()
            .filter(|record| {
                snapshot
                    .spec_revisions
                    .iter()
                    .find(|spec| SpecReference::from(*spec) == record.spec)
                    .and_then(|spec| spec.stage(&record.current_stage_id))
                    .is_some_and(|stage| !stage.terminal)
            })
            .map(|record| record.record_id.clone())
            .collect(),
        ScopeKind::AllExistingAndFuture => snapshot
            .records
            .iter()
            .map(|record| record.record_id.clone())
            .collect(),
    };
    ids.sort();
    let scope = ScopeSelection {
        kind,
        confirmed_generation: snapshot.generation,
        frozen_record_ids: ids,
        applies_to_future_records: kind != ScopeKind::SingleRecord,
        effective_sequence: snapshot
            .event_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("event sequence overflow"))?,
    };
    scope.validate()?;
    Ok(scope)
}

/// The fallback revision is left unchanged by scoped operations. Bindings are an
/// append-only overlay; the latest applicable binding wins separately per rule.
pub fn policy_for_record(
    snapshot: &ProjectSnapshot,
    record_id: &str,
) -> Result<BehaviorPolicy, DecisionError> {
    snapshot.validate()?;
    if !snapshot
        .records
        .iter()
        .any(|record| record.record_id == record_id)
    {
        return Err(invalid("record does not exist"));
    }
    resolved_policy(snapshot, Some(record_id))
}

pub fn policy_for_future(snapshot: &ProjectSnapshot) -> Result<BehaviorPolicy, DecisionError> {
    snapshot.validate()?;
    resolved_policy(snapshot, None)
}

pub fn evaluate_bound_record(
    snapshot: &ProjectSnapshot,
    record_id: &str,
    as_of_date: NaiveDate,
) -> Result<RecordEvaluation, DecisionError> {
    Ok(evaluate_record(
        snapshot,
        record_id,
        &policy_for_record(snapshot, record_id)?,
        as_of_date,
    )?)
}

pub fn scenario_from_snapshot(
    snapshot: &ProjectSnapshot,
    scenario_id: &str,
    name: &str,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<Scenario, DecisionError> {
    snapshot.validate()?;
    check_clock(snapshot, as_of_date, now)?;
    let scenario = Scenario {
        scenario_id: scenario_id.to_owned(),
        name: name.to_owned(),
        project_id: snapshot.project_id.clone(),
        base_generation: snapshot.generation,
        spec_revisions: snapshot.spec_revisions.clone(),
        active_spec: snapshot.active_spec.clone(),
        date_settings: snapshot.date_settings,
        project_created_at: snapshot.created_at,
        event_sequence: snapshot.event_sequence,
        records: snapshot.records.clone(),
        event_history: snapshot.event_history.clone(),
        candidate_policy: policy_for_future(snapshot)?,
        as_of_date,
        fixed_now: now,
        steps: Vec::new(),
        expected: Vec::new(),
        extensions: BTreeMap::new(),
    };
    scenario.validate()?;
    Ok(scenario)
}

pub fn rehearse_change(
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<ChangePreview, DecisionError> {
    validate_request(snapshot, request, now)?;
    check_clock(snapshot, as_of_date, now)?;
    if request.cases.is_empty() || request.cases.len() > MAX_SCENARIOS {
        return Err(invalid(
            "provide a bounded, nonempty set of rehearsal cases",
        ));
    }
    let candidate = bind_change(snapshot, request, now)?;
    let mut scenario_ids = BTreeSet::new();
    let mut scenarios = Vec::new();
    let mut evidence = Vec::new();
    for case in &request.cases {
        if !scenario_ids.insert(case.scenario.scenario_id.clone())
            || snapshot
                .scenarios
                .iter()
                .any(|old| old.scenario_id == case.scenario.scenario_id)
        {
            return Err(invalid("rehearsal scenario IDs must be new and unique"));
        }
        let mut scenario =
            prepare_case(snapshot, &candidate, case, &request.scope, as_of_date, now)?;
        scenario.candidate_policy = target_policy(&candidate, &case.target)?;
        let result = run_scenario(&scenario)?;
        if result.observations.is_empty()
            || result
                .observations
                .iter()
                .any(|observation| observation.record_id != case.target.record_id())
        {
            return Err(invalid(
                "each rehearsal case must observe its selected example",
            ));
        }
        scenarios.push(scenario);
        evidence.push(result);
    }
    let (checked_decision_ids, conflicts) = check_decisions(&candidate, &request.supersedes)?;
    Ok(ChangePreview {
        request_id: request.request_id.clone(),
        project_id: snapshot.project_id.clone(),
        generation: snapshot.generation,
        scope: request.scope.clone(),
        impacts: impacts(
            snapshot,
            &candidate,
            &request.scope.frozen_record_ids,
            as_of_date,
        )?,
        future_policy: resolved_policy(&candidate, None)?,
        scenarios,
        evidence,
        checked_decision_ids,
        conflicts,
        fingerprint: fingerprint(&(
            snapshot,
            request,
            as_of_date.to_string(),
            now.to_rfc3339(),
            RUNTIME_SEMANTICS_VERSION,
        ))?,
    })
}

/// Recompute the exact context and native observations immediately before handing
/// a candidate to the store. A late callback, imported success flag or edited
/// preview cannot restore adoption eligibility.
pub fn prepare_adoption(
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
    preview: &ChangePreview,
    current_request_id: &str,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<PreparedChange, DecisionError> {
    if current_request_id != request.request_id || preview.request_id != current_request_id {
        return Err(DecisionError::StaleEvidence);
    }
    let fresh = rehearse_change(snapshot, request, as_of_date, now)?;
    if &fresh != preview {
        return Err(DecisionError::StaleEvidence);
    }
    if !fresh.conflicts.is_empty() {
        return Err(DecisionError::Conflicts(fresh.conflicts));
    }
    if fresh.evidence.iter().any(|e| {
        e.source != EvidenceSource::NativeExecution
            || e.status == EvidenceStatus::Failed
            || !e.errors.is_empty()
    }) {
        return Err(invalid(
            "all selected cases must execute locally without failed expectations",
        ));
    }
    let mut candidate = bind_change(snapshot, request, now)?;
    let targets = request
        .cases
        .iter()
        .map(|case| (case.scenario.scenario_id.clone(), case.target.clone()))
        .collect();
    let outcomes = fresh
        .evidence
        .iter()
        .flat_map(|e| e.observations.clone())
        .collect();
    let scenario_ids = fresh
        .scenarios
        .iter()
        .map(|s| s.scenario_id.clone())
        .collect();
    let evidence_ids = fresh.evidence.iter().map(|e| e.run_id.clone()).collect();
    let mut intent_revision = 1;
    for decision in &mut candidate.decisions {
        if request.supersedes.contains(&decision.decision_id) {
            intent_revision = intent_revision.max(
                decision
                    .intent_revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("decision revision overflow"))?,
            );
            decision.status = DecisionStatus::Superseded;
        }
    }
    candidate.scenarios.extend(fresh.scenarios);
    candidate.evidence.extend(fresh.evidence);
    candidate.decisions.push(DecisionRecord {
        decision_id: request.decision_id.clone(),
        intent_revision,
        choice: DecisionChoice::Adopt,
        expected_outcomes: outcomes,
        scope: Some(request.scope.clone()),
        rationale: request.rationale.clone(),
        unresolved_questions: request.unresolved_questions.clone(),
        scenario_ids,
        evidence_ids,
        supersedes: request.supersedes.clone(),
        status: DecisionStatus::Active,
        created_at: now,
        extensions: intent_extensions(DecisionIntent {
            version: 1,
            original_request: request.original_request.clone(),
            rule_keys: request.rule_keys.clone(),
            behavior_revision_id: Some(request.revision_id.clone()),
            baseline_binding_count: candidate.rule_bindings.len(),
            fallback_revision_id: candidate.active_behavior_revision_id.clone(),
            targets,
            withdraws_revision_id: None,
        })?,
    });
    candidate.validate()?;
    Ok(PreparedChange {
        operation_id: request.operation_id.clone(),
        snapshot: candidate,
    })
}

/// Both/neither/defer are durable unresolved outcomes. Resolving one later creates
/// an explicitly superseding decision rather than changing its original words.
pub fn prepare_unresolved(
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
    choice: DecisionChoice,
    now: DateTime<FixedOffset>,
) -> Result<PreparedChange, DecisionError> {
    validate_request(snapshot, request, now)?;
    if choice == DecisionChoice::Adopt || !request.supersedes.is_empty() {
        return Err(invalid(
            "an unresolved choice cannot adopt or replace an existing decision",
        ));
    }
    let mut candidate = snapshot.clone();
    candidate.updated_at = now;
    candidate.decisions.push(DecisionRecord {
        decision_id: request.decision_id.clone(),
        intent_revision: 1,
        choice,
        expected_outcomes: Vec::new(),
        scope: Some(request.scope.clone()),
        rationale: request.rationale.clone(),
        unresolved_questions: request.unresolved_questions.clone(),
        scenario_ids: Vec::new(),
        evidence_ids: Vec::new(),
        supersedes: Vec::new(),
        status: DecisionStatus::Pending,
        created_at: now,
        extensions: intent_extensions(DecisionIntent {
            version: 1,
            original_request: request.original_request.clone(),
            rule_keys: request.rule_keys.clone(),
            behavior_revision_id: None,
            baseline_binding_count: candidate.rule_bindings.len(),
            fallback_revision_id: candidate.active_behavior_revision_id.clone(),
            targets: BTreeMap::new(),
            withdraws_revision_id: None,
        })?,
    });
    candidate.validate()?;
    Ok(PreparedChange {
        operation_id: request.operation_id.clone(),
        snapshot: candidate,
    })
}

pub fn rehearse_withdrawal(
    snapshot: &ProjectSnapshot,
    revision_id: &str,
    operation_id: &str,
    request_id: &str,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<WithdrawalPreview, DecisionError> {
    snapshot.validate()?;
    check_clock(snapshot, as_of_date, now)?;
    check_operation(snapshot, operation_id)?;
    check_id(request_id)?;
    let mut withdrawn = BTreeSet::new();
    let mut withdrawing_decisions = Vec::new();
    for decision in &snapshot.decisions {
        if let Ok(intent) = read_intent(decision) {
            if let Some(id) = intent.withdraws_revision_id {
                withdrawn.insert(id);
            }
            if intent.behavior_revision_id.as_deref() == Some(revision_id) {
                withdrawing_decisions.push(decision.decision_id.clone());
            }
        }
    }
    if withdrawn.contains(revision_id) {
        return Err(invalid("this rule revision has already been withdrawn"));
    }
    if withdrawing_decisions.is_empty()
        || revision_id == snapshot.active_behavior_revision_id
        || snapshot.behavior_revision(revision_id).is_none()
    {
        return Err(invalid(
            "only an adopted scoped rule revision can be withdrawn",
        ));
    }
    let keys: Vec<RuleKey> = RULE_KEYS
        .into_iter()
        .filter(|key| {
            snapshot.rule_bindings.iter().any(|binding| {
                binding.rule_key == *key && binding.behavior_revision_id == revision_id
            })
        })
        .collect();
    if keys.is_empty() {
        return Err(invalid("the revision has no explainable scoped bindings"));
    }
    withdrawn.insert(revision_id.to_owned());
    let mut candidate = snapshot.clone();
    candidate.updated_at = now;
    let mut preserved_later_bindings = Vec::new();
    let mut affected_ids = Vec::new();
    for target in snapshot
        .records
        .iter()
        .map(|record| Some(record.record_id.as_str()))
        .chain(std::iter::once(None))
    {
        for key in &keys {
            let affected_before = snapshot.rule_bindings.iter().any(|binding| {
                binding.rule_key == *key
                    && binding.behavior_revision_id == revision_id
                    && binding_applies(snapshot, binding, target)
            });
            if !affected_before {
                continue;
            }
            let Some((index, current)) = latest_binding(snapshot, *key, target) else {
                continue;
            };
            if current.behavior_revision_id != revision_id {
                preserved_later_bindings.push(PreservedBinding {
                    record_id: target.map(str::to_owned),
                    rule_key: *key,
                    behavior_revision_id: current.behavior_revision_id.clone(),
                });
                continue;
            }
            // Follow the saved predecessor history on today's data. Skip revisions
            // already withdrawn, so an older undo cannot resurrect them later.
            let restored_revision = snapshot.rule_bindings[..index]
                .iter()
                .rev()
                .find(|binding| {
                    binding.rule_key == *key
                        && binding_applies(snapshot, binding, target)
                        && !withdrawn.contains(&binding.behavior_revision_id)
                })
                .map(|binding| binding.behavior_revision_id.clone())
                .unwrap_or_else(|| snapshot.active_behavior_revision_id.clone());
            let scope = freeze_scope(
                snapshot,
                if target.is_some() {
                    ScopeKind::SingleRecord
                } else {
                    ScopeKind::FutureRecords
                },
                target,
            )?;
            append_binding(
                &mut candidate,
                operation_id,
                *key,
                target.map(str::to_owned),
                restored_revision,
                scope,
            )?;
            if let Some(id) = target {
                if !affected_ids.iter().any(|existing| existing == id) {
                    affected_ids.push(id.to_owned());
                }
            }
        }
    }
    for decision in &mut candidate.decisions {
        if withdrawing_decisions.contains(&decision.decision_id)
            && decision.status == DecisionStatus::Active
        {
            decision.status = DecisionStatus::Withdrawn;
        }
    }
    let future_policy = resolved_policy(&candidate, None)?;
    // The compensating revision records the withdrawal operation; the new
    // bindings point to the actual predecessor revisions to retain attribution.
    let undo_revision_id = stable_id("withdrawal", &(operation_id, revision_id))?;
    candidate.behavior_revisions.push(BehaviorRevision {
        revision_id: undo_revision_id,
        operation_id: Some(operation_id.to_owned()),
        parent_revision_id: candidate
            .behavior_revisions
            .last()
            .map(|revision| revision.revision_id.clone()),
        policy: future_policy.clone(),
        reason: format!("Withdraw scoped behavior {revision_id}; preserve current business facts"),
        created_at: now,
    });
    let (checked_decision_ids, conflicts) = check_decisions(&candidate, &withdrawing_decisions)?;
    candidate.decisions.push(DecisionRecord {
        decision_id: stable_id("withdrawal-decision", &(operation_id, revision_id))?,
        intent_revision: 1,
        choice: DecisionChoice::Adopt,
        expected_outcomes: Vec::new(),
        scope: None,
        rationale: format!("Withdraw rule revision {revision_id}; preserve later data and independently bound rules"),
        unresolved_questions: Vec::new(),
        scenario_ids: Vec::new(),
        evidence_ids: Vec::new(),
        supersedes: Vec::new(),
        status: DecisionStatus::Withdrawn,
        created_at: now,
        extensions: intent_extensions(DecisionIntent {
            version: 1,
            original_request: format!("Withdraw {revision_id}"),
            rule_keys: keys,
            behavior_revision_id: None,
            baseline_binding_count: candidate.rule_bindings.len(),
            fallback_revision_id: candidate.active_behavior_revision_id.clone(),
            targets: BTreeMap::new(),
            withdraws_revision_id: Some(revision_id.to_owned()),
        })?,
    });
    candidate.validate()?;
    Ok(WithdrawalPreview {
        request_id: request_id.to_owned(),
        project_id: snapshot.project_id.clone(),
        generation: snapshot.generation,
        revision_id: revision_id.to_owned(),
        operation_id: operation_id.to_owned(),
        preserved_record_count: snapshot.records.len(),
        preserved_event_count: snapshot.event_history.len(),
        impacts: impacts(snapshot, &candidate, &affected_ids, as_of_date)?,
        future_policy,
        preserved_later_bindings,
        checked_decision_ids,
        conflicts,
        fingerprint: fingerprint(&(
            snapshot,
            revision_id,
            operation_id,
            request_id,
            as_of_date.to_string(),
            now.to_rfc3339(),
            RUNTIME_SEMANTICS_VERSION,
        ))?,
        candidate,
    })
}

pub fn prepare_withdrawal(
    snapshot: &ProjectSnapshot,
    preview: &WithdrawalPreview,
    current_request_id: &str,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<PreparedChange, DecisionError> {
    if preview.request_id != current_request_id {
        return Err(DecisionError::StaleEvidence);
    }
    let fresh = rehearse_withdrawal(
        snapshot,
        &preview.revision_id,
        &preview.operation_id,
        current_request_id,
        as_of_date,
        now,
    )?;
    if &fresh != preview {
        return Err(DecisionError::StaleEvidence);
    }
    if !fresh.conflicts.is_empty() {
        return Err(DecisionError::Conflicts(fresh.conflicts));
    }
    Ok(PreparedChange {
        operation_id: fresh.operation_id,
        snapshot: fresh.candidate,
    })
}

fn validate_request(
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
    now: DateTime<FixedOffset>,
) -> Result<(), DecisionError> {
    snapshot.validate()?;
    for id in [
        &request.request_id,
        &request.decision_id,
        &request.revision_id,
    ] {
        check_id(id)?;
    }
    check_operation(snapshot, &request.operation_id)?;
    if now < snapshot.updated_at {
        return Err(invalid("the current clock precedes the saved project"));
    }
    if snapshot
        .decisions
        .iter()
        .any(|decision| decision.decision_id == request.decision_id)
        || snapshot.behavior_revision(&request.revision_id).is_some()
    {
        return Err(invalid("decision and behavior revision IDs must be new"));
    }
    if request.original_request.trim().is_empty()
        || request.original_request.len() > 16_384
        || request.rationale.trim().is_empty()
        || request.rationale.len() > 16_384
    {
        return Err(invalid(
            "preserve a nonempty, bounded request and rationale",
        ));
    }
    if request.rule_keys.is_empty()
        || request.rule_keys.len() > 3
        || request
            .rule_keys
            .iter()
            .enumerate()
            .any(|(index, key)| request.rule_keys[..index].contains(key))
    {
        return Err(invalid("select distinct supported rule dimensions"));
    }
    request.candidate_policy.validate()?;
    request.scope.validate()?;
    let expected = freeze_scope(
        snapshot,
        request.scope.kind,
        request.scope.frozen_record_ids.first().map(String::as_str),
    )?;
    let mut supplied = request.scope.clone();
    supplied.frozen_record_ids.sort();
    if supplied != expected {
        return Err(DecisionError::StaleEvidence);
    }
    let mut superseded = BTreeSet::new();
    for id in &request.supersedes {
        if !superseded.insert(id)
            || !snapshot.decisions.iter().any(|decision| {
                decision.decision_id == *id
                    && matches!(
                        decision.status,
                        DecisionStatus::Active | DecisionStatus::Pending
                    )
            })
        {
            return Err(invalid(
                "only existing active or pending decisions can be explicitly superseded",
            ));
        }
    }
    Ok(())
}

fn bind_change(
    snapshot: &ProjectSnapshot,
    request: &ChangeRequest,
    now: DateTime<FixedOffset>,
) -> Result<ProjectSnapshot, DecisionError> {
    let mut candidate = snapshot.clone();
    candidate.updated_at = now;
    candidate.behavior_revisions.push(BehaviorRevision {
        revision_id: request.revision_id.clone(),
        operation_id: Some(request.operation_id.clone()),
        parent_revision_id: candidate
            .behavior_revisions
            .last()
            .map(|revision| revision.revision_id.clone()),
        policy: request.candidate_policy.clone(),
        reason: request.rationale.clone(),
        created_at: now,
    });
    let target = if request.scope.kind == ScopeKind::SingleRecord {
        request.scope.frozen_record_ids.first().cloned()
    } else {
        None
    };
    for key in &request.rule_keys {
        append_binding(
            &mut candidate,
            &request.operation_id,
            *key,
            target.clone(),
            request.revision_id.clone(),
            request.scope.clone(),
        )?;
    }
    candidate.validate()?;
    Ok(candidate)
}

fn append_binding(
    snapshot: &mut ProjectSnapshot,
    operation_id: &str,
    key: RuleKey,
    target: Option<String>,
    revision_id: String,
    scope: ScopeSelection,
) -> Result<(), DecisionError> {
    let previous_binding_id = snapshot
        .rule_bindings
        .iter()
        .rev()
        .find(|binding| binding.rule_key == key && binding.record_id == target)
        .map(|binding| binding.binding_id.clone());
    let binding_id = stable_id("binding", &(operation_id, key, &target))?;
    if snapshot
        .rule_bindings
        .iter()
        .any(|binding| binding.binding_id == binding_id)
    {
        return Err(invalid("binding operation identity is already used"));
    }
    snapshot.rule_bindings.push(RuleBinding {
        binding_id,
        rule_key: key,
        record_id: target,
        behavior_revision_id: revision_id,
        previous_binding_id,
        effective_sequence: scope.effective_sequence,
        scope,
    });
    Ok(())
}

fn binding_applies(
    snapshot: &ProjectSnapshot,
    binding: &RuleBinding,
    record_id: Option<&str>,
) -> bool {
    match record_id {
        None => binding.record_id.is_none() && binding.scope.applies_to_future_records,
        Some(id) => {
            if binding
                .record_id
                .as_deref()
                .is_some_and(|target| target != id)
            {
                return false;
            }
            binding
                .scope
                .frozen_record_ids
                .iter()
                .any(|target| target == id)
                || (binding.scope.applies_to_future_records
                    && snapshot.records.iter().any(|record| {
                        record.record_id == id
                            && record.created_sequence >= binding.scope.effective_sequence
                    }))
        }
    }
}

fn latest_binding<'a>(
    snapshot: &'a ProjectSnapshot,
    key: RuleKey,
    record_id: Option<&str>,
) -> Option<(usize, &'a RuleBinding)> {
    snapshot
        .rule_bindings
        .iter()
        .enumerate()
        .rev()
        .find(|(_, binding)| {
            binding.rule_key == key && binding_applies(snapshot, binding, record_id)
        })
}

fn resolved_policy(
    snapshot: &ProjectSnapshot,
    record_id: Option<&str>,
) -> Result<BehaviorPolicy, DecisionError> {
    resolved_policy_from(
        snapshot,
        record_id,
        &snapshot.rule_bindings,
        &snapshot.active_behavior_revision_id,
    )
}

fn resolved_policy_from(
    snapshot: &ProjectSnapshot,
    record_id: Option<&str>,
    bindings: &[RuleBinding],
    fallback_revision_id: &str,
) -> Result<BehaviorPolicy, DecisionError> {
    let mut policy = snapshot
        .behavior_revision(fallback_revision_id)
        .ok_or_else(|| invalid("missing fallback behavior"))?
        .policy
        .clone();
    for key in RULE_KEYS {
        if let Some(binding) = bindings.iter().rev().find(|binding| {
            binding.rule_key == key && binding_applies(snapshot, binding, record_id)
        }) {
            let bound = snapshot
                .behavior_revision(&binding.behavior_revision_id)
                .ok_or_else(|| invalid("binding has no behavior revision"))?;
            match key {
                RuleKey::Timer => policy.timer = bound.policy.timer,
                RuleKey::DeliveryTarget => policy.due_date = bound.policy.due_date,
                RuleKey::Reminder => policy.reminder = bound.policy.reminder,
            }
        }
    }
    Ok(policy)
}

fn target_policy(
    snapshot: &ProjectSnapshot,
    target: &RehearsalTarget,
) -> Result<BehaviorPolicy, DecisionError> {
    match target {
        RehearsalTarget::Record(id) => {
            if !snapshot
                .records
                .iter()
                .any(|record| record.record_id == *id)
            {
                return Err(invalid("the rehearsal target no longer exists"));
            }
            resolved_policy(snapshot, Some(id))
        }
        RehearsalTarget::FutureRecord(_) => resolved_policy(snapshot, None),
    }
}

fn prepare_case(
    snapshot: &ProjectSnapshot,
    candidate: &ProjectSnapshot,
    case: &RehearsalCase,
    scope: &ScopeSelection,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<Scenario, DecisionError> {
    let input = &case.scenario;
    if input.project_id != snapshot.project_id
        || input.base_generation != snapshot.generation
        || input.spec_revisions != snapshot.spec_revisions
        || input.active_spec != snapshot.active_spec
        || input.date_settings != snapshot.date_settings
        || input.project_created_at != snapshot.created_at
        || input.as_of_date != as_of_date
        || input.fixed_now != now
    {
        return Err(DecisionError::StaleEvidence);
    }
    let targeted = match &case.target {
        RehearsalTarget::Record(id) => scope.frozen_record_ids.contains(id),
        RehearsalTarget::FutureRecord(_) => scope.applies_to_future_records,
    };
    if !targeted {
        return Err(invalid(
            "the selected case must demonstrate a record or future default in the frozen scope",
        ));
    }
    let mut scenario = input.clone();
    scenario.candidate_policy = target_policy(candidate, &case.target)?;
    let target_json = serde_json::to_value(&case.target).map_err(|e| invalid(e.to_string()))?;
    if scenario
        .extensions
        .get(TARGET_KEY)
        .is_some_and(|old| old != &target_json)
    {
        return Err(invalid(
            "scenario target metadata disagrees with the selected case",
        ));
    }
    scenario
        .extensions
        .insert(TARGET_KEY.to_owned(), target_json);
    if scenario.steps.iter().any(|step| matches!(step, ScenarioStep::Observe { record_id } if record_id != case.target.record_id())) { return Err(invalid("a case may only observe its selected record")); }
    if !scenario
        .steps
        .iter()
        .any(|step| matches!(step, ScenarioStep::Observe { .. }))
    {
        scenario.steps.push(ScenarioStep::Observe {
            record_id: case.target.record_id().to_owned(),
        });
    }
    scenario.validate()?;
    Ok(scenario)
}

fn impacts(
    before: &ProjectSnapshot,
    after: &ProjectSnapshot,
    record_ids: &[String],
    as_of_date: NaiveDate,
) -> Result<Vec<RecordImpact>, DecisionError> {
    record_ids
        .iter()
        .map(|id| {
            Ok(RecordImpact {
                record_id: id.clone(),
                before: EvidenceObservation::from(&evaluate_record(
                    before,
                    id,
                    &resolved_policy(before, Some(id))?,
                    as_of_date,
                )?),
                after: EvidenceObservation::from(&evaluate_record(
                    after,
                    id,
                    &resolved_policy(after, Some(id))?,
                    as_of_date,
                )?),
            })
        })
        .collect()
}

fn check_decisions(
    snapshot: &ProjectSnapshot,
    excluded: &[String],
) -> Result<(Vec<String>, Vec<DecisionConflict>), DecisionError> {
    let mut checked = Vec::new();
    let mut conflicts = Vec::new();
    for decision in snapshot.decisions.iter().filter(|decision| {
        decision.status == DecisionStatus::Active && !excluded.contains(&decision.decision_id)
    }) {
        checked.push(decision.decision_id.clone());
        let mut check = || -> Result<(), DecisionError> {
            let intent = read_intent(decision)?;
            if decision.choice != DecisionChoice::Adopt
                || intent.rule_keys.is_empty()
                || intent.targets.len() != decision.scenario_ids.len()
                || decision.scenario_ids.is_empty()
                || decision.evidence_ids.len() != decision.scenario_ids.len()
            {
                return Err(invalid(
                    "active decision lacks interpretable local regression cases",
                ));
            }
            let baseline_bindings = snapshot
                .rule_bindings
                .get(..intent.baseline_binding_count)
                .ok_or_else(|| invalid("decision binding baseline is missing"))?;
            let mut confirmed = Vec::new();
            for (scenario_id, evidence_id) in
                decision.scenario_ids.iter().zip(&decision.evidence_ids)
            {
                let source = snapshot
                    .scenarios
                    .iter()
                    .find(|scenario| scenario.scenario_id == *scenario_id)
                    .ok_or_else(|| invalid("decision scenario is missing"))?;
                let evidence = snapshot
                    .evidence
                    .iter()
                    .find(|evidence| evidence.run_id == *evidence_id)
                    .ok_or_else(|| invalid("decision evidence is missing"))?;
                let target = intent
                    .targets
                    .get(scenario_id)
                    .ok_or_else(|| invalid("decision target is missing"))?;
                if evidence.scenario_id != *scenario_id
                    || evidence.source != EvidenceSource::NativeExecution
                    || evidence.runtime_semantics_version != RUNTIME_SEMANTICS_VERSION
                    || evidence.status == EvidenceStatus::Failed
                {
                    return Err(invalid("decision evidence requires a current local replay"));
                }
                let source_target = match target {
                    RehearsalTarget::Record(id) => Some(id.as_str()),
                    RehearsalTarget::FutureRecord(_) => None,
                };
                let original_policy = resolved_policy_from(
                    snapshot,
                    source_target,
                    baseline_bindings,
                    &intent.fallback_revision_id,
                )?;
                if source.candidate_policy != original_policy {
                    return Err(invalid(
                        "decision example does not match its adoption-time bindings",
                    ));
                }
                let native = run_scenario(source)?;
                let mut saved = evidence.clone();
                saved.extensions.clear();
                if native != saved {
                    return Err(invalid(
                        "saved decision evidence disagrees with native execution",
                    ));
                }
                confirmed.extend(native.observations.clone());
                let scope = decision
                    .scope
                    .as_ref()
                    .ok_or_else(|| invalid("active decision has no frozen scope"))?;
                if source.extensions.get(TARGET_KEY)
                    != Some(&serde_json::to_value(target).map_err(|e| invalid(e.to_string()))?)
                {
                    return Err(invalid("decision scenario target metadata is inconsistent"));
                }
                // An example confirms the selected dimensions for its whole
                // frozen scope, including the future default. The append-only
                // adoption-time binding prefix preserves each target's preexisting
                // dependencies, so heterogeneous date/reminder policies are not
                // mistaken for a regression introduced by this candidate.
                let mut checked_policies = BTreeSet::new();
                let targets = snapshot
                    .records
                    .iter()
                    .filter(|record| {
                        scope.frozen_record_ids.contains(&record.record_id)
                            || (scope.applies_to_future_records
                                && record.created_sequence >= scope.effective_sequence)
                    })
                    .map(|record| Some(record.record_id.as_str()))
                    .chain(scope.applies_to_future_records.then_some(None));
                for affected in targets {
                    let policy = resolved_policy(snapshot, affected)?;
                    let baseline = resolved_policy_from(
                        snapshot,
                        affected,
                        baseline_bindings,
                        &intent.fallback_revision_id,
                    )?;
                    if !checked_policies.insert(fingerprint(&(&baseline, &policy))?) {
                        continue;
                    }
                    let mut replay = source.clone();
                    replay.candidate_policy = baseline;
                    let expected = run_scenario(&replay)?.observations;
                    replay.candidate_policy = policy;
                    let actual = run_scenario(&replay)?.observations;
                    if !same_outcomes(&expected, &actual, &intent.rule_keys) {
                        conflicts.push(DecisionConflict {
                            decision_id: decision.decision_id.clone(),
                            scenario_id: Some(scenario_id.clone()),
                            message: format!(
                                "the proposed behavior changes a confirmed outcome for {}",
                                affected.unwrap_or("future records")
                            ),
                            expected,
                            actual,
                        });
                    }
                }
            }
            if confirmed != decision.expected_outcomes {
                return Err(invalid(
                    "decision expectations no longer match its saved evidence",
                ));
            }
            Ok(())
        };
        if let Err(error) = check() {
            conflicts.push(DecisionConflict {
                decision_id: decision.decision_id.clone(),
                scenario_id: None,
                message: error.to_string(),
                expected: decision.expected_outcomes.clone(),
                actual: Vec::new(),
            });
        }
    }
    Ok((checked, conflicts))
}

fn same_outcomes(
    expected: &[EvidenceObservation],
    actual: &[EvidenceObservation],
    keys: &[RuleKey],
) -> bool {
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(before, after)| {
            before.record_id == after.record_id
                && before.current_stage_id == after.current_stage_id
                && keys.iter().all(|key| match key {
                    RuleKey::Timer => before.elapsed_work_days == after.elapsed_work_days,
                    RuleKey::DeliveryTarget => {
                        before.original_due_date == after.original_due_date
                            && before.display_due_date == after.display_due_date
                    }
                    RuleKey::Reminder => before.reminder == after.reminder,
                })
        })
}

fn read_intent(decision: &DecisionRecord) -> Result<DecisionIntent, DecisionError> {
    let value = decision
        .extensions
        .get(INTENT_KEY)
        .ok_or_else(|| invalid("decision has no supported scoped intent metadata"))?;
    let intent: DecisionIntent = serde_json::from_value(value.clone())
        .map_err(|e| invalid(format!("cannot interpret decision: {e}")))?;
    if intent.version != 1
        || intent.rule_keys.len() > 3
        || intent
            .rule_keys
            .iter()
            .enumerate()
            .any(|(i, key)| intent.rule_keys[..i].contains(key))
    {
        return Err(invalid(
            "unsupported decision intent version or rule dimensions",
        ));
    }
    Ok(intent)
}

fn intent_extensions(intent: DecisionIntent) -> Result<ExtensionFields, DecisionError> {
    Ok(BTreeMap::from([(
        INTENT_KEY.to_owned(),
        serde_json::to_value(intent).map_err(|e| invalid(e.to_string()))?,
    )]))
}

fn check_clock(
    snapshot: &ProjectSnapshot,
    as_of_date: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<(), DecisionError> {
    let offset = FixedOffset::east_opt(snapshot.date_settings.calendar_utc_offset_seconds)
        .ok_or_else(|| invalid("invalid project calendar offset"))?;
    if now < snapshot.updated_at || as_of_date > now.with_timezone(&offset).date_naive() {
        return Err(invalid(
            "rehearsal clock must not precede the project or project-local as-of date",
        ));
    }
    Ok(())
}

fn check_operation(snapshot: &ProjectSnapshot, operation_id: &str) -> Result<(), DecisionError> {
    check_id(operation_id)?;
    if snapshot.operation_receipts.contains_key(operation_id)
        || snapshot
            .event_history
            .iter()
            .any(|event| event.event_id == operation_id)
        || snapshot
            .behavior_revisions
            .iter()
            .any(|revision| revision.operation_id.as_deref() == Some(operation_id))
    {
        return Err(invalid(
            "operation was already used; retry the original prepared candidate unchanged",
        ));
    }
    Ok(())
}
fn check_id(value: &str) -> Result<(), DecisionError> {
    if value.is_empty()
        || value.len() > MAX_ID_LENGTH
        || !value.as_bytes()[0].is_ascii_lowercase()
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_.".contains(&byte)
        })
    {
        return Err(invalid("identities must be stable lowercase IDs"));
    }
    Ok(())
}
fn fingerprint(value: &impl Serialize) -> Result<String, DecisionError> {
    let canonical = serde_json::to_value(value).map_err(|e| invalid(e.to_string()))?;
    let bytes = serde_json::to_vec(&canonical).map_err(|e| invalid(e.to_string()))?;
    Ok(digest(&SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn stable_id(prefix: &str, value: &impl Serialize) -> Result<String, DecisionError> {
    Ok(format!("{prefix}-{}", fingerprint(value)?))
}
fn invalid(message: impl Into<String>) -> DecisionError {
    DecisionError::InvalidRequest(message.into())
}
