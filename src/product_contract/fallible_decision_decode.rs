//! Fallible structural decoder for the persisted Product Contract decision graph.
//!
//! Decision DTOs replace BTreeSet/BTreeMap value owners with ordered fallible
//! owners while preserving their sorted Serde sequence/object representation.
//! Semantic decision validation remains a separate caller step.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

use super::fallible_expr_decode::decode_expr;
use super::fallible_persisted_decode::decode_record_ref;
use super::fallible_value_decode::{decode_data_value, decode_type, decode_values, JsonKind, NumericValue, ValueDecodeError};
use super::{
    AcceptedProperty, DecisionOutcome, DecisionStatus, Digest, Expr, FallibleValues, Population,
    PropertyPredicate, PropertyTerm, RecordRef, MAX_AST_NODES, MAX_COLLECTION, MAX_EXPR_DEPTH,
    MAX_ITEMS,
};

const GRAPH_FIELDS: &[&str] = &["version", "revision", "decisions"];
const ADOPTION_PLAN_FIELDS: &[&str] = &["version", "id", "project_id", "expected_generation", "expected_data", "expected_decisions", "expected_session", "current_source", "target", "scope", "compatibility", "required_decisions", "checks", "evidence", "retire_decisions"];
const DECISION_CHECK_FIELDS: &[&str] = &["decision", "decision_digest", "property_digest", "scope", "state", "binding", "evidence", "explanation"];
const SCOPED_DECISION_FIELDS: &[&str] = &[
    "id", "revision", "request", "rationale", "scope", "outcome", "status", "obligations", "scenarios", "witness", "supersedes",
];
const DECISION_SCOPE_FIELDS: &[&str] = &["operations", "population", "conditions", "excluded_records", "unknowns"];
const UNKNOWN_BOUNDARY_FIELDS: &[&str] = &["id", "operations", "description"];
const POPULATION_VARIANTS: &[&str] = &["all", "new_work", "created_after", "records", "entity", "where"];
const OUTCOME_VARIANTS: &[&str] = &["accept", "keep_current", "either_acceptable", "both_needed", "neither_fits", "deferred"];
const STATUS_VARIANTS: &[&str] = &["active", "pending", "superseded", "withdrawn"];
const SCOPE_MATCH_VARIANTS: &[&str] = &["applies", "outside", "unknown"];
const CHECK_STATE_VARIANTS: &[&str] = &["satisfied", "violated", "unknown", "outside", "unsupported", "stale", "failed"];
const ACCEPTED_PROPERTY_FIELDS: &[&str] = &["id", "description", "predicate"];
const PROPERTY_TERM_VARIANTS: &[&str] = &[
    "literal", "observed", "view_rows", "row_action_rows", "view_column", "output_count", "output_column", "count",
];
const PROPERTY_PREDICATE_VARIANTS: &[&str] = &["equal", "less", "and", "or", "not"];
const NO_FIELDS: &[&str] = &[];
const GENERATION_FIELDS: &[&str] = &["generation"];
const RECORDS_FIELDS: &[&str] = &["records"];
const ENTITY_FIELDS: &[&str] = &["entity"];
const WHERE_FIELDS: &[&str] = &["entity", "predicate"];
const ARTIFACT_FIELDS: &[&str] = &["artifact"];
const BY_FIELDS: &[&str] = &["by"];
const ADOPTION_FIELDS: &[&str] = &["adoption"];
const LITERAL_FIELDS: &[&str] = &["value_type", "value"];
const OBSERVED_FIELDS: &[&str] = &["point", "observable", "value_type"];
const VIEW_ROWS_FIELDS: &[&str] = &["point", "entity"];
const ROW_ACTION_ROWS_FIELDS: &[&str] = &["point", "view", "entity", "binding", "action"];
const VIEW_COLUMN_FIELDS: &[&str] = &["point", "column", "value_type"];
const OUTPUT_COUNT_FIELDS: &[&str] = &["point", "output"];
const OUTPUT_COLUMN_FIELDS: &[&str] = &["point", "output", "column", "value_type"];
const COUNT_FIELDS: &[&str] = &["value"];
const BINARY_PROPERTY_FIELDS: &[&str] = &["left", "right"];
const PROPERTY_VALUES_FIELDS: &[&str] = &["values"];
const PROPERTY_VALUE_FIELDS: &[&str] = &["value"];

#[derive(Debug, Serialize)]
pub(super) struct DecodedDecisionGraph {
    pub(super) version: u32,
    pub(super) revision: u64,
    pub(super) decisions: Vec<DecodedScopedDecision>,
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedScopedDecision {
    pub(super) id: String,
    pub(super) revision: u64,
    pub(super) request: String,
    pub(super) rationale: Option<String>,
    pub(super) scope: DecodedDecisionScope,
    pub(super) outcome: DecisionOutcome,
    pub(super) status: DecisionStatus,
    pub(super) obligations: Vec<AcceptedProperty>,
    pub(super) scenarios: Vec<Digest>,
    pub(super) witness: Digest,
    pub(super) supersedes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedDecisionScope {
    pub(super) operations: FallibleStringSet,
    pub(super) population: Population,
    pub(super) conditions: FallibleValues,
    pub(super) excluded_records: Vec<RecordRef>,
    pub(super) unknowns: Vec<DecodedUnknownBoundary>,
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedAdoptionPlan {
    version: u32,
    id: String,
    project_id: String,
    expected_generation: u64,
    expected_data: Digest,
    expected_decisions: Digest,
    expected_session: Digest,
    current_source: super::SourceBinding,
    target: super::ArtifactRef,
    scope: DecodedDecisionScope,
    compatibility: crate::product_store::scope::DecodedCompatibilityReport,
    required_decisions: Vec<String>,
    checks: Vec<DecodedDecisionCheck>,
    evidence: Vec<Digest>,
    retire_decisions: Vec<String>,
}
impl DecodedAdoptionPlan {
    pub(crate) fn validate_with(
        &self,
        cx: &mut AllocationContext,
    ) -> Result<(), super::DecisionValidationError> {
        super::version(self.version)?;
        super::check_id(&self.id)?;
        super::check_id(&self.project_id)?;
        self.current_source.validate()?;
        self.target.validate()?;
        super::validate_decoded_decision_scope(&self.scope, cx)?;
        super::require(
            self.project_id == self.current_source.project_id,
            "plan project/source mismatch",
        )?;
        super::unique_with(self.required_decisions.iter().map(String::as_str), cx)?;
        super::unique_with(self.retire_decisions.iter().map(String::as_str), cx)?;
        super::bounded(self.checks.len())?;
        super::bounded(self.evidence.len())?;
        super::require(
            self.compatibility.identity_matches(&self.expected_data, &self.target.program_digest),
            "compatibility refers to different data/program",
        )?;
        self.compatibility.validate_with(cx)?;
        for check in &self.checks {
            super::check_id(&check.decision)?;
            check.binding.validate()?;
            super::text(&check.explanation)?;
            if check.state == super::CheckState::Satisfied {
                super::require(
                    check.scope == super::ScopeMatch::Applies && check.evidence.is_some(),
                    "satisfied decision has no applicable run evidence",
                )?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct DecodedDecisionCheck {
    decision: String,
    decision_digest: Digest,
    property_digest: Digest,
    scope: super::ScopeMatch,
    state: super::CheckState,
    binding: super::RunBinding,
    evidence: Option<Digest>,
    explanation: String,
}

#[derive(Debug, Serialize)]
pub(super) struct DecodedUnknownBoundary {
    pub(super) id: String,
    pub(super) operations: FallibleStringSet,
    pub(super) description: String,
}

pub(super) struct FallibleStringSet(crate::product_alloc::ordered::OrderedSet<String>);
impl FallibleStringSet {
    pub(super) fn is_empty(&self) -> bool { self.0.is_empty() }
    pub(super) fn iter(&self) -> impl Iterator<Item = &String> { self.0.iter() }
}

impl std::fmt::Debug for FallibleStringSet {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_set().entries(self.0.iter()).finish()
    }
}

impl Serialize for FallibleStringSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0.iter() {
            sequence.serialize_element(value)?;
        }
        sequence.end()
    }
}

pub(super) fn decode_decision_graph(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedDecisionGraph, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct DecisionGraph", GRAPH_FIELDS)?;
    let mut version = None;
    let mut revision = None;
    let mut decoded_decisions = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(integer_u32(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            "decisions" => decoded_decisions = Some(decisions(value, cx)?),
            _ => return Err(unknown_field(key, GRAPH_FIELDS)),
        }
    }
    Ok(DecodedDecisionGraph {
        version: required(version, "version")?,
        revision: required(revision, "revision")?,
        decisions: required(decoded_decisions, "decisions")?,
    })
}

pub(super) fn decode_adoption_plan(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedAdoptionPlan, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct AdoptionPlan", ADOPTION_PLAN_FIELDS)?;
    let mut version = None;
    let mut id = None;
    let mut project_id = None;
    let mut expected_generation = None;
    let mut expected_data = None;
    let mut expected_decisions = None;
    let mut expected_session = None;
    let mut current_source = None;
    let mut target = None;
    let mut scope = None;
    let mut compatibility = None;
    let mut required_decisions = None;
    let mut checks = None;
    let mut evidence = None;
    let mut retire_decisions = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "version" => version = Some(integer_u32(value)?),
            "id" => id = Some(string(value)?),
            "project_id" => project_id = Some(string(value)?),
            "expected_generation" => expected_generation = Some(integer_u64(value)?),
            "expected_data" => expected_data = Some(digest(value)?),
            "expected_decisions" => expected_decisions = Some(digest(value)?),
            "expected_session" => expected_session = Some(digest(value)?),
            "current_source" => current_source = Some(super::decode_source_binding_fallible(value, cx)?),
            "target" => target = Some(super::decode_artifact_ref_fallible(value, cx)?),
            "scope" => scope = Some(decode_decision_scope(value, cx)?),
            "compatibility" => compatibility = Some(crate::product_store::scope::decode_compatibility_report_fallible(value, cx)?),
            "required_decisions" => required_decisions = Some(strings(value, cx, "adoption required decision count")?),
            "checks" => checks = Some(adoption_checks(value, cx)?),
            "evidence" => evidence = Some(digests(value, cx, "adoption evidence count")?),
            "retire_decisions" => retire_decisions = Some(strings(value, cx, "adoption retired decision count")?),
            _ => return Err(unknown_field(key, ADOPTION_PLAN_FIELDS)),
        }
    }
    Ok(DecodedAdoptionPlan {
        version: required(version, "version")?,
        id: required(id, "id")?,
        project_id: required(project_id, "project_id")?,
        expected_generation: required(expected_generation, "expected_generation")?,
        expected_data: required(expected_data, "expected_data")?,
        expected_decisions: required(expected_decisions, "expected_decisions")?,
        expected_session: required(expected_session, "expected_session")?,
        current_source: required(current_source, "current_source")?,
        target: required(target, "target")?,
        scope: required(scope, "scope")?,
        compatibility: required(compatibility, "compatibility")?,
        required_decisions: required(required_decisions, "required_decisions")?,
        checks: required(checks, "checks")?,
        evidence: required(evidence, "evidence")?,
        retire_decisions: required(retire_decisions, "retire_decisions")?,
    })
}

fn adoption_checks(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedDecisionCheck>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, "adoption decision check count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_adoption_check(value, cx)?);
    }
    Ok(decoded)
}

fn decode_adoption_check(value: Value, cx: &mut AllocationContext) -> Result<DecodedDecisionCheck, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct DecisionCheck", DECISION_CHECK_FIELDS)?;
    let mut decision = None;
    let mut decision_digest = None;
    let mut property_digest = None;
    let mut scope = None;
    let mut state = None;
    let mut binding = None;
    // Serde treats missing Option fields in object maps as None.
    let mut evidence = None;
    let mut explanation = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "decision" => decision = Some(string(value)?),
            "decision_digest" => decision_digest = Some(digest(value)?),
            "property_digest" => property_digest = Some(digest(value)?),
            "scope" => scope = Some(decode_scope_match(value)?),
            "state" => state = Some(decode_check_state(value)?),
            "binding" => binding = Some(super::decode_run_binding_fallible(value, cx)?),
            "evidence" => evidence = Some(optional_digest(value)?),
            "explanation" => explanation = Some(string(value)?),
            _ => return Err(unknown_field(key, DECISION_CHECK_FIELDS)),
        }
    }
    Ok(DecodedDecisionCheck {
        decision: required(decision, "decision")?,
        decision_digest: required(decision_digest, "decision_digest")?,
        property_digest: required(property_digest, "property_digest")?,
        scope: required(scope, "scope")?,
        state: required(state, "state")?,
        binding: required(binding, "binding")?,
        evidence: evidence.unwrap_or(None),
        explanation: required(explanation, "explanation")?,
    })
}

fn optional_digest(value: Value) -> Result<Option<Digest>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => digest(value).map(Some),
    }
}

fn decode_scope_match(value: Value) -> Result<super::ScopeMatch, ValueDecodeError> {
    let tag = unit_variant(value, SCOPE_MATCH_VARIANTS)?;
    match tag.as_str() {
        "applies" => Ok(super::ScopeMatch::Applies),
        "outside" => Ok(super::ScopeMatch::Outside),
        "unknown" => Ok(super::ScopeMatch::Unknown),
        _ => unreachable!("variant helper checked the closed list"),
    }
}

fn decode_check_state(value: Value) -> Result<super::CheckState, ValueDecodeError> {
    let tag = unit_variant(value, CHECK_STATE_VARIANTS)?;
    match tag.as_str() {
        "satisfied" => Ok(super::CheckState::Satisfied),
        "violated" => Ok(super::CheckState::Violated),
        "unknown" => Ok(super::CheckState::Unknown),
        "outside" => Ok(super::CheckState::Outside),
        "unsupported" => Ok(super::CheckState::Unsupported),
        "stale" => Ok(super::CheckState::Stale),
        "failed" => Ok(super::CheckState::Failed),
        _ => unreachable!("variant helper checked the closed list"),
    }
}

fn unit_variant(value: Value, variants: &'static [&'static str]) -> Result<String, ValueDecodeError> {
    let tag = match value {
        Value::String(tag) => tag,
        Value::Object(fields) => {
            if fields.len() != 1 {
                return Err(ValueDecodeError::InvalidLength { actual: fields.len(), expected: 1 });
            }
            let (tag, payload) = fields.into_entries().next().expect("one enum map entry checked");
            if !variants.iter().any(|variant| *variant == tag) {
                return Err(ValueDecodeError::UnknownVariant { tag, expected: variants });
            }
            if !matches!(payload, Value::Null) {
                return Err(invalid_type(payload, "unit"));
            }
            return Ok(tag);
        }
        value => return Err(invalid_type(value, "a string variant identifier or map")),
    };
    if variants.iter().any(|variant| *variant == tag) {
        Ok(tag)
    } else {
        Err(ValueDecodeError::UnknownVariant { tag, expected: variants })
    }
}

fn decisions(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedScopedDecision>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, "decision count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_scoped_decision(value, cx)?);
    }
    Ok(decoded)
}

fn decode_scoped_decision(value: Value, cx: &mut AllocationContext) -> Result<DecodedScopedDecision, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct ScopedDecision", SCOPED_DECISION_FIELDS)?;
    let mut id = None;
    let mut revision = None;
    let mut request = None;
    let mut rationale = None;
    let mut scope = None;
    let mut outcome = None;
    let mut status = None;
    let mut obligations = None;
    let mut scenarios = None;
    let mut witness = None;
    let mut supersedes = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "id" => id = Some(string(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            "request" => request = Some(string(value)?),
            "rationale" => rationale = Some(optional_string(value)?),
            "scope" => scope = Some(decode_decision_scope(value, cx)?),
            "outcome" => outcome = Some(decode_outcome(value)?),
            "status" => status = Some(decode_status(value)?),
            "obligations" => obligations = Some(accepted_properties(value, cx)?),
            "scenarios" => scenarios = Some(digests(value, cx, "decision scenario count")?),
            "witness" => witness = Some(digest(value)?),
            "supersedes" => supersedes = Some(strings(value, cx, "superseded decision count")?),
            _ => return Err(unknown_field(key, SCOPED_DECISION_FIELDS)),
        }
    }
    Ok(DecodedScopedDecision {
        id: required(id, "id")?,
        revision: required(revision, "revision")?,
        request: required(request, "request")?,
        rationale: rationale.unwrap_or(None),
        scope: required(scope, "scope")?,
        outcome: required(outcome, "outcome")?,
        status: required(status, "status")?,
        obligations: required(obligations, "obligations")?,
        scenarios: required(scenarios, "scenarios")?,
        witness: required(witness, "witness")?,
        supersedes: required(supersedes, "supersedes")?,
    })
}

fn decode_decision_scope(value: Value, cx: &mut AllocationContext) -> Result<DecodedDecisionScope, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct DecisionScope", DECISION_SCOPE_FIELDS)?;
    let mut operations = None;
    let mut population = None;
    let mut conditions = None;
    let mut excluded_records = None;
    let mut unknowns = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "operations" => operations = Some(string_set(value, cx, "decision operation count")?),
            "population" => population = Some(decode_population(value, cx)?),
            "conditions" => conditions = Some(values(value, cx)?),
            "excluded_records" => excluded_records = Some(record_refs(value, cx, "excluded record count", MAX_COLLECTION)?),
            "unknowns" => unknowns = Some(unknown_boundaries(value, cx)?),
            _ => return Err(unknown_field(key, DECISION_SCOPE_FIELDS)),
        }
    }
    Ok(DecodedDecisionScope {
        operations: required(operations, "operations")?,
        population: required(population, "population")?,
        conditions: required(conditions, "conditions")?,
        excluded_records: required(excluded_records, "excluded_records")?,
        unknowns: required(unknowns, "unknowns")?,
    })
}

fn decode_unknown_boundary(value: Value, cx: &mut AllocationContext) -> Result<DecodedUnknownBoundary, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct UnknownBoundary", UNKNOWN_BOUNDARY_FIELDS)?;
    let mut id = None;
    let mut operations = None;
    let mut description = None;
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "id" => id = Some(string(value)?),
            "operations" => operations = Some(string_set(value, cx, "unknown boundary operation count")?),
            "description" => description = Some(string(value)?),
            _ => return Err(unknown_field(key, UNKNOWN_BOUNDARY_FIELDS)),
        }
    }
    Ok(DecodedUnknownBoundary {
        id: required(id, "id")?,
        operations: required(operations, "operations")?,
        description: required(description, "description")?,
    })
}

fn unknown_boundaries(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedUnknownBoundary>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, "unknown boundary count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_unknown_boundary(value, cx)?);
    }
    Ok(decoded)
}

fn decode_population(value: Value, cx: &mut AllocationContext) -> Result<Population, ValueDecodeError> {
    let (tag, fields) = tagged(value, POPULATION_VARIANTS, "internally tagged enum Population")?;
    match tag.as_str() {
        "all" => unit(fields, NO_FIELDS).map(|()| Population::All),
        "new_work" => unit(fields, NO_FIELDS).map(|()| Population::NewWork),
        "created_after" => {
            let generation = u64_field(fields, "generation", GENERATION_FIELDS)?;
            Ok(Population::CreatedAfter { generation })
        }
        "records" => {
            let records = record_refs_field(fields, "records", RECORDS_FIELDS, cx, MAX_COLLECTION)?;
            Ok(Population::Records { records })
        }
        "entity" => {
            let entity = string_field(fields, "entity", ENTITY_FIELDS)?;
            Ok(Population::Entity { entity })
        }
        "where" => {
            let mut entity = None;
            let mut predicate = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "entity" => entity = Some(string(value)?),
                    "predicate" => predicate = Some(decode_expr(value, cx)?),
                    _ => return Err(unknown_field(key, WHERE_FIELDS)),
                }
            }
            Ok(Population::Where {
                entity: required(entity, "entity")?,
                predicate: required(predicate, "predicate")?,
            })
        }
        _ => unreachable!("tag comes from the closed Population variant table"),
    }
}

fn decode_outcome(value: Value) -> Result<DecisionOutcome, ValueDecodeError> {
    let (tag, fields) = tagged(value, OUTCOME_VARIANTS, "internally tagged enum DecisionOutcome")?;
    match tag.as_str() {
        "accept" => {
            let artifact = digest_field(fields, "artifact", ARTIFACT_FIELDS)?;
            Ok(DecisionOutcome::Accept { artifact })
        }
        "keep_current" => unit(fields, NO_FIELDS).map(|()| DecisionOutcome::KeepCurrent),
        "either_acceptable" => unit(fields, NO_FIELDS).map(|()| DecisionOutcome::EitherAcceptable),
        "both_needed" => unit(fields, NO_FIELDS).map(|()| DecisionOutcome::BothNeeded),
        "neither_fits" => unit(fields, NO_FIELDS).map(|()| DecisionOutcome::NeitherFits),
        "deferred" => unit(fields, NO_FIELDS).map(|()| DecisionOutcome::Deferred),
        _ => unreachable!("tag comes from the closed DecisionOutcome variant table"),
    }
}

fn decode_status(value: Value) -> Result<DecisionStatus, ValueDecodeError> {
    let (tag, fields) = tagged(value, STATUS_VARIANTS, "internally tagged enum DecisionStatus")?;
    match tag.as_str() {
        "active" => unit(fields, NO_FIELDS).map(|()| DecisionStatus::Active),
        "pending" => unit(fields, NO_FIELDS).map(|()| DecisionStatus::Pending),
        "superseded" => {
            let by = string_field(fields, "by", BY_FIELDS)?;
            Ok(DecisionStatus::Superseded { by })
        }
        "withdrawn" => {
            let adoption = string_field(fields, "adoption", ADOPTION_FIELDS)?;
            Ok(DecisionStatus::Withdrawn { adoption })
        }
        _ => unreachable!("tag comes from the closed DecisionStatus variant table"),
    }
}

fn accepted_properties(value: Value, cx: &mut AllocationContext) -> Result<Vec<AcceptedProperty>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, "decision obligation count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_accepted_property(value, cx)?);
    }
    Ok(decoded)
}

pub(super) fn decode_accepted_property(value: Value, cx: &mut AllocationContext) -> Result<AcceptedProperty, ValueDecodeError> {
    let mut fields = struct_fields(value, "a struct AcceptedProperty", ACCEPTED_PROPERTY_FIELDS)?;
    let mut id = None;
    let mut description = None;
    let mut predicate = None;
    let mut budget = PropertyBudget { nodes: 0 };
    while let Some((key, value)) = fields.next(cx)? {
        match key.as_str() {
            "id" => id = Some(string(value)?),
            "description" => description = Some(string(value)?),
            "predicate" => predicate = Some(decode_property_predicate(value, 0, &mut budget, cx)?),
            _ => return Err(unknown_field(key, ACCEPTED_PROPERTY_FIELDS)),
        }
    }
    Ok(AcceptedProperty {
        id: required(id, "id")?,
        description: required(description, "description")?,
        predicate: required(predicate, "predicate")?,
    })
}

#[derive(Default)]
struct PropertyBudget { nodes: usize }

fn enter_property_node(depth: usize, budget: &mut PropertyBudget) -> Result<(), ValueDecodeError> {
    if depth > MAX_EXPR_DEPTH {
        return Err(ValueDecodeError::LimitExceeded { resource: "property expression depth", limit: MAX_EXPR_DEPTH });
    }
    if budget.nodes >= MAX_AST_NODES {
        return Err(ValueDecodeError::LimitExceeded { resource: "property expression node count", limit: MAX_AST_NODES });
    }
    budget.nodes += 1;
    Ok(())
}

fn decode_property_predicate(
    value: Value,
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyPredicate, ValueDecodeError> {
    enter_property_node(depth, budget)?;
    let (tag, fields) = tagged(value, PROPERTY_PREDICATE_VARIANTS, "internally tagged enum PropertyPredicate")?;
    match tag.as_str() {
        "equal" => binary_predicate(fields, false, depth, budget, cx),
        "less" => binary_predicate(fields, true, depth, budget, cx),
        "and" => predicate_values(fields, true, depth, budget, cx),
        "or" => predicate_values(fields, false, depth, budget, cx),
        "not" => {
            let value = predicate_field(fields, "value", PROPERTY_VALUE_FIELDS, depth + 1, budget, cx)?;
            Ok(PropertyPredicate::Not { value: product_alloc::try_box(value, cx)? })
        }
        _ => unreachable!("tag comes from the closed PropertyPredicate variant table"),
    }
}

fn binary_predicate(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    less: bool,
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyPredicate, ValueDecodeError> {
    let mut left = None;
    let mut right = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "left" => left = Some(decode_property_term(value, depth + 1, budget, cx)?),
            "right" => right = Some(decode_property_term(value, depth + 1, budget, cx)?),
            _ => return Err(unknown_field(key, BINARY_PROPERTY_FIELDS)),
        }
    }
    let left = required(left, "left")?;
    let right = required(right, "right")?;
    if less { Ok(PropertyPredicate::Less { left, right }) } else { Ok(PropertyPredicate::Equal { left, right }) }
}

fn predicate_values(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    and: bool,
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyPredicate, ValueDecodeError> {
    let mut values = None;
    for (key, value) in fields.into_entries() {
        if key == "values" {
            let items = array(value)?;
            bounded_length(items.len(), MAX_ITEMS, "property predicate item count")?;
            let mut decoded = Vec::new();
            product_alloc::try_reserve(&mut decoded, items.len(), cx, AllocationSite::Vector)?;
            for item in items {
                decoded.push(decode_property_predicate(item, depth + 1, budget, cx)?);
            }
            values = Some(decoded);
        } else {
            return Err(unknown_field(key, PROPERTY_VALUES_FIELDS));
        }
    }
    let values = required(values, "values")?;
    if and { Ok(PropertyPredicate::And { values }) } else { Ok(PropertyPredicate::Or { values }) }
}

fn decode_property_term(
    value: Value,
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyTerm, ValueDecodeError> {
    enter_property_node(depth, budget)?;
    let (tag, fields) = tagged(value, PROPERTY_TERM_VARIANTS, "internally tagged enum PropertyTerm")?;
    match tag.as_str() {
        "literal" => {
            let mut value_type = None;
            let mut literal = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "value_type" => value_type = Some(decode_type(value, cx)?),
                    "value" => literal = Some(decode_data_value(value, cx)?),
                    _ => return Err(unknown_field(key, LITERAL_FIELDS)),
                }
            }
            Ok(PropertyTerm::Literal { value_type: required(value_type, "value_type")?, value: required(literal, "value")? })
        }
        "observed" => {
            let mut point = None;
            let mut observable = None;
            let mut value_type = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "observable" => observable = Some(string(value)?),
                    "value_type" => value_type = Some(decode_type(value, cx)?),
                    _ => return Err(unknown_field(key, OBSERVED_FIELDS)),
                }
            }
            Ok(PropertyTerm::Observed { point: required(point, "point")?, observable: required(observable, "observable")?, value_type: required(value_type, "value_type")? })
        }
        "view_rows" => {
            let mut point = None;
            let mut entity = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "entity" => entity = Some(string(value)?),
                    _ => return Err(unknown_field(key, VIEW_ROWS_FIELDS)),
                }
            }
            Ok(PropertyTerm::ViewRows { point: required(point, "point")?, entity: required(entity, "entity")? })
        }
        "row_action_rows" => {
            let mut point = None;
            let mut view = None;
            let mut entity = None;
            let mut binding = None;
            let mut action = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "view" => view = Some(string(value)?),
                    "entity" => entity = Some(string(value)?),
                    "binding" => binding = Some(string(value)?),
                    "action" => action = Some(string(value)?),
                    _ => return Err(unknown_field(key, ROW_ACTION_ROWS_FIELDS)),
                }
            }
            Ok(PropertyTerm::RowActionRows {
                point: required(point, "point")?, view: required(view, "view")?, entity: required(entity, "entity")?,
                binding: required(binding, "binding")?, action: required(action, "action")?,
            })
        }
        "view_column" => {
            let mut point = None;
            let mut column = None;
            let mut value_type = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "column" => column = Some(string(value)?),
                    "value_type" => value_type = Some(decode_type(value, cx)?),
                    _ => return Err(unknown_field(key, VIEW_COLUMN_FIELDS)),
                }
            }
            Ok(PropertyTerm::ViewColumn { point: required(point, "point")?, column: required(column, "column")?, value_type: required(value_type, "value_type")? })
        }
        "output_count" => {
            let mut point = None;
            let mut output = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "output" => output = Some(string(value)?),
                    _ => return Err(unknown_field(key, OUTPUT_COUNT_FIELDS)),
                }
            }
            Ok(PropertyTerm::OutputCount { point: required(point, "point")?, output: required(output, "output")? })
        }
        "output_column" => {
            let mut point = None;
            let mut output = None;
            let mut column = None;
            let mut value_type = None;
            for (key, value) in fields.into_entries() {
                match key.as_str() {
                    "point" => point = Some(string(value)?),
                    "output" => output = Some(string(value)?),
                    "column" => column = Some(string(value)?),
                    "value_type" => value_type = Some(decode_type(value, cx)?),
                    _ => return Err(unknown_field(key, OUTPUT_COLUMN_FIELDS)),
                }
            }
            Ok(PropertyTerm::OutputColumn {
                point: required(point, "point")?, output: required(output, "output")?, column: required(column, "column")?,
                value_type: required(value_type, "value_type")?,
            })
        }
        "count" => {
            let value = term_field(fields, "value", COUNT_FIELDS, depth + 1, budget, cx)?;
            Ok(PropertyTerm::Count { value: product_alloc::try_box(value, cx)? })
        }
        _ => unreachable!("tag comes from the closed PropertyTerm variant table"),
    }
}

fn term_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyTerm, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(decode_property_term(value, depth, budget, cx)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

fn predicate_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    depth: usize,
    budget: &mut PropertyBudget,
    cx: &mut AllocationContext,
) -> Result<PropertyPredicate, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(decode_property_predicate(value, depth, budget, cx)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

fn values(value: Value, cx: &mut AllocationContext) -> Result<FallibleValues, ValueDecodeError> {
    let entries = match value {
        Value::Object(entries) => entries,
        value => return Err(invalid_type(value, "a map")),
    };
    bounded_length(entries.len(), MAX_ITEMS, "decision condition count")?;
    decode_values(Value::Object(entries), cx)
}

fn record_refs_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
    cx: &mut AllocationContext,
    limit: usize,
) -> Result<Vec<RecordRef>, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(record_refs(value, cx, "population record count", limit)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

fn record_refs(
    value: Value,
    cx: &mut AllocationContext,
    resource: &'static str,
    limit: usize,
) -> Result<Vec<RecordRef>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), limit, resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(decode_record_ref(value, cx)?); }
    Ok(decoded)
}

fn digests(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<Digest>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(digest(value)?); }
    Ok(decoded)
}

fn strings(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<Vec<String>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), MAX_ITEMS, resource)?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values { decoded.push(string(value)?); }
    Ok(decoded)
}

fn string_set(value: Value, cx: &mut AllocationContext, resource: &'static str) -> Result<FallibleStringSet, ValueDecodeError> {
    let values = array(value)?;
    let mut decoded = crate::product_alloc::ordered::OrderedSet::new();
    for value in values {
        let value = string(value)?;
        if !decoded.contains(&value) && decoded.len() == MAX_ITEMS {
            return Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS });
        }
        decoded.try_insert(value, cx)?;
    }
    Ok(FallibleStringSet(decoded))
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value)?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}

fn tagged(
    value: Value,
    variants: &'static [&'static str],
    expected: &'static str,
) -> Result<(String, crate::product_alloc::ordered::OrderedMap<String, Value>), ValueDecodeError> {
    let mut fields = object(value, expected)?;
    let tag = fields.remove("kind").ok_or(ValueDecodeError::MissingField("kind"))?;
    let tag = string(tag)?;
    if variants.iter().any(|variant| *variant == tag) { Ok((tag, fields)) }
    else { Err(ValueDecodeError::UnknownVariant { tag, expected: variants }) }
}

fn unit(fields: crate::product_alloc::ordered::OrderedMap<String, Value>, expected: &'static [&'static str]) -> Result<(), ValueDecodeError> {
    if let Some((key, _)) = fields.into_entries().next() { Err(unknown_field(key, expected)) } else { Ok(()) }
}

fn string_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<String, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(string(value)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

fn u64_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<u64, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(integer_u64(value)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

fn digest_field(
    fields: crate::product_alloc::ordered::OrderedMap<String, Value>,
    field: &'static str,
    expected: &'static [&'static str],
) -> Result<Digest, ValueDecodeError> {
    let mut decoded = None;
    for (key, value) in fields.into_entries() {
        if key == field { decoded = Some(digest(value)?); }
        else { return Err(unknown_field(key, expected)); }
    }
    required(decoded, field)
}

enum StructFields {
    Object(std::vec::IntoIter<(String, Value)>),
    Sequence {
        names: &'static [&'static str],
        values: std::vec::IntoIter<Value>,
        index: usize,
        actual: usize,
    },
}

impl StructFields {
    fn next(&mut self, cx: &mut AllocationContext) -> Result<Option<(String, Value)>, ValueDecodeError> {
        match self {
            Self::Object(fields) => Ok(fields.next()),
            Self::Sequence { names, values, index, actual } => {
                if let Some(name) = names.get(*index) {
                    let Some(value) = values.next() else {
                        return Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() });
                    };
                    *index += 1;
                    let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                    Ok(Some((name, value)))
                } else if values.next().is_some() {
                    Err(ValueDecodeError::InvalidLength { actual: *actual, expected: names.len() })
                } else {
                    Ok(None)
                }
            }
        }
    }
}

fn struct_fields(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
) -> Result<StructFields, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(StructFields::Object(fields.into_entries())),
        Value::Array(values) => {
            let actual = values.len();
            Ok(StructFields::Sequence { names, values: values.into_iter(), index: 0, actual })
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn object(
    value: Value,
    expected: &'static str,
) -> Result<crate::product_alloc::ordered::OrderedMap<String, Value>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields),
        value => Err(invalid_type(value, expected)),
    }
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn string(value: Value) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}

fn optional_string(value: Value) -> Result<Option<String>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => string(value).map(Some),
    }
}

fn integer_u32(value: Value) -> Result<u32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "u32" }),
        Value::Number(Number::I64(value)) => u32::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u32" }),
        value => Err(invalid_type(value, "u32")),
    }
}

fn integer_u64(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }),
        value => Err(invalid_type(value, "u64")),
    }
}

fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn bounded_length(len: usize, limit: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > limit { Err(ValueDecodeError::LimitExceeded { resource, limit }) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::AllocationFailureKind;
    use crate::product_json::{self, Cause};
    use std::num::NonZeroUsize;

    const D: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
        serde_json::from_str(source).unwrap()
    }

    fn allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
        assert!(matches!(failure.site, AllocationSite::Vector | AllocationSite::Bytes | AllocationSite::String | AllocationSite::OrderedMap | AllocationSite::OrderedSet | AllocationSite::Box));
    }

    fn graph_source() -> String {
        format!(r#"{{"version":1,"revision":5,"decisions":[{{"id":"d0","revision":1,"request":"Keep existing behavior","rationale":"Measured evidence","scope":{{"operations":["edit","edit","inspect"],"population":{{"kind":"where","entity":"person","predicate":{{"kind":"equal","left":{{"kind":"field","record":{{"kind":"variable","name":"row"}},"field":"name"}},"right":{{"kind":"literal","value_type":{{"kind":"text"}},"value":{{"kind":"text","value":"Ada"}}}}}}}},"conditions":{{"enabled":{{"kind":"boolean","value":true}}}},"excluded_records":[{{"entity":"person","record":"old"}}],"unknowns":[{{"id":"missing-source","operations":["edit","edit"],"description":"Source mapping unavailable"}}]}},"outcome":{{"kind":"accept","artifact":"{D}"}},"status":{{"kind":"active"}},"obligations":[{{"id":"name-stable","description":"Name remains Ada","predicate":{{"kind":"and","values":[{{"kind":"equal","left":{{"kind":"observed","point":"after","observable":"name","value_type":{{"kind":"text"}}}},"right":{{"kind":"literal","value_type":{{"kind":"text"}},"value":{{"kind":"text","value":"Ada"}}}}}},{{"kind":"not","value":{{"kind":"less","left":{{"kind":"output_count","point":"after","output":"report"}},"right":{{"kind":"output_count","point":"before","output":"report"}}}}}}]}}}}],"scenarios":["{D}"],"witness":"{D}","supersedes":[]}}]}}"#)
    }

    #[test]
    fn persisted_decision_graph_matches_canonical_serde_bytes() {
        let source = graph_source();
        let expected: super::super::DecisionGraph = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_decision_graph(syntax(&source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    fn supersession_source() -> String {
        format!(r#"{{"version":1,"revision":5,"decisions":[
          {{"id":"d0","revision":1,"request":"Prior","scope":{{"operations":["edit"],"population":{{"kind":"records","records":[{{"entity":"person","record":"included"}}]}},"conditions":{{}},"excluded_records":[{{"entity":"person","record":"ignored"}},{{"entity":"person","record":"other"}}],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"superseded","by":"d1"}},"obligations":[],"scenarios":["{D}"],"witness":"{D}","supersedes":[]}},
          {{"id":"d1","revision":2,"request":"Current","scope":{{"operations":["edit"],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"active"}},"obligations":[],"scenarios":["{D}"],"witness":"{D}","supersedes":["d0"]}}
        ]}}"#)
    }

    #[test]
    fn fallible_graph_validation_and_identity_match_legacy_semantics() {
        let source = supersession_source();
        let expected: super::super::DecisionGraph = typed(&source);
        expected.validate().unwrap();
        let expected_identity = expected.identity().unwrap();
        let mut decode_cx = AllocationContext::new();
        let decoded = super::super::decode_decision_graph_fallible(syntax(&source), &mut decode_cx).unwrap();
        let mut validate_cx = AllocationContext::new();
        decoded.validate_with(&mut validate_cx).unwrap();
        let mut identity_cx = AllocationContext::new();
        assert_eq!(decoded.identity_with(&mut identity_cx).unwrap(), expected_identity);
        let identity_growths = identity_cx.growth_requests();
        for nth in 1..=identity_growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = decoded.identity_with(&mut cx).unwrap_err();
            let super::super::DecisionValidationError::Allocation(failure) = error else { panic!("expected identity allocation refusal") };
            assert_eq!(Some(failure), cx.first_failure());
            let mut retry = AllocationContext::new();
            assert_eq!(decoded.identity_with(&mut retry).unwrap(), expected_identity);
        }

        let mut invalids = Vec::new();
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["version"] = serde_json::json!(2);
        graph["decisions"][0]["id"] = serde_json::json!("INVALID ID");
        invalids.push((graph, "unsupported contract version"));
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][0]["status"] = serde_json::json!({"kind":"active"});
        invalids.push((graph, "supersession graph is not reciprocal"));
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][0]["status"] = serde_json::json!({"kind":"active"});
        graph["decisions"][1]["supersedes"] = serde_json::json!(["absent"]);
        invalids.push((graph, "unknown superseded decision"));
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][1]["id"] = serde_json::json!("d0");
        invalids.push((graph, "duplicate ID: d0"));
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][0]["scope"]["excluded_records"][1] = graph["decisions"][0]["scope"]["excluded_records"][0].clone();
        invalids.push((graph, "duplicate selected record"));
        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][0]["status"] = serde_json::json!({"kind":"superseded","by":"ghost"});
        invalids.push((graph, "missing superseding decision"));
        for (value, expected_error) in invalids {
            let source = serde_json::to_string(&value).unwrap();
            let legacy: super::super::DecisionGraph = typed(&source);
            let legacy_error = legacy.validate().unwrap_err();
            assert_eq!(legacy_error.0, expected_error);
            let decoded = super::super::decode_decision_graph_fallible(syntax(&source), &mut AllocationContext::new()).unwrap();
            let error = decoded.validate_with(&mut AllocationContext::new()).unwrap_err();
            assert!(matches!(error, super::super::DecisionValidationError::Contract(ref error) if error.0 == expected_error));
        }

        let mut graph: serde_json::Value = serde_json::from_str(&source).unwrap();
        graph["decisions"][1]["status"] = serde_json::json!({"kind":"superseded","by":"d0"});
        graph["decisions"][0]["status"] = serde_json::json!({"kind":"superseded","by":"d1"});
        graph["decisions"][1]["supersedes"] = serde_json::json!(["d0"]);
        graph["decisions"][0]["supersedes"] = serde_json::json!(["d1"]);
        let cycle_source = serde_json::to_string(&graph).unwrap();
        let legacy: super::super::DecisionGraph = typed(&cycle_source);
        assert_eq!(legacy.validate().unwrap_err().0, "cyclic decision supersession");
        let decoded = super::super::decode_decision_graph_fallible(syntax(&cycle_source), &mut AllocationContext::new()).unwrap();
        assert!(matches!(decoded.validate_with(&mut AllocationContext::new()), Err(super::super::DecisionValidationError::Contract(ref error)) if error.0 == "cyclic decision supersession"));
    }

    #[test]
    fn decision_index_and_supersession_set_refusal_is_preserved_and_retryable() {
        let source = supersession_source();
        let decoded = super::super::decode_decision_graph_fallible(syntax(&source), &mut AllocationContext::new()).unwrap();
        let mut baseline = AllocationContext::new();
        decoded.validate_with(&mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths >= 3, "index and traversal set allocations are both charged");
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let error = decoded.validate_with(&mut cx).unwrap_err();
            let super::super::DecisionValidationError::Allocation(failure) = error else { panic!("expected allocation refusal") };
            assert_eq!(Some(failure), cx.first_failure());
            let mut retry = AllocationContext::new();
            decoded.validate_with(&mut retry).unwrap();
        }
    }

    #[test]
    fn fallible_record_reference_uniqueness_charges_and_retries() {
        let refs = [
            RecordRef { entity: "person".into(), record: "one".into() },
            RecordRef { entity: "person".into(), record: "two".into() },
        ];
        let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        let error = super::super::validate_record_refs_with(&refs, &mut cx).unwrap_err();
        let super::super::DecisionValidationError::Allocation(failure) = error else { panic!("expected record reference allocation refusal") };
        assert_eq!(failure.site, AllocationSite::OrderedSet);
        assert_eq!(Some(failure), cx.first_failure());
        let mut retry = AllocationContext::new();
        super::super::validate_record_refs_with(&refs, &mut retry).unwrap();
    }

    fn struct_sequence(value: &mut serde_json::Value, fields: &[&str]) {
        let object = value.as_object().expect("fixture struct object");
        let sequence = fields.iter().map(|field| object[*field].clone()).collect();
        *value = serde_json::Value::Array(sequence);
    }

    #[test]
    fn nested_struct_sequences_match_serde_and_canonical_bytes() {
        let mut value: serde_json::Value = serde_json::from_str(&graph_source()).unwrap();
        let decision = &mut value["decisions"][0];
        let scope = &mut decision["scope"];
        struct_sequence(&mut scope["excluded_records"][0], &["entity", "record"]);
        struct_sequence(&mut scope["unknowns"][0], &["id", "operations", "description"]);
        struct_sequence(scope, &["operations", "population", "conditions", "excluded_records", "unknowns"]);
        struct_sequence(&mut decision["obligations"][0], &["id", "description", "predicate"]);
        struct_sequence(decision, &["id", "revision", "request", "rationale", "scope", "outcome", "status", "obligations", "scenarios", "witness", "supersedes"]);
        struct_sequence(&mut value, &["version", "revision", "decisions"]);

        let expected: super::super::DecisionGraph = serde_json::from_value(value.clone()).unwrap();
        let source = serde_json::to_vec(&value).unwrap();
        let decoded = decode_decision_graph(
            product_json::parse_json_bytes(&source, &mut AllocationContext::new()).unwrap(),
            &mut AllocationContext::new(),
        ).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());

        let source = String::from_utf8(source).unwrap();
        let mut baseline = AllocationContext::new();
        decode_decision_graph(syntax(&source), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for ordinal in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(ordinal).unwrap(), true);
            allocation(decode_decision_graph(syntax(&source), &mut cx).unwrap_err(), &cx);
            let retry = decode_decision_graph(syntax(&source), &mut AllocationContext::new()).unwrap();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&retry).unwrap());
        }
    }

    #[test]
    fn decision_graph_sequence_errors_follow_declared_field_order() {
        let value = Value::Array(vec![Value::String("bad version".into()), Value::Number(Number::I64(-1)), Value::Array(vec![]), Value::Null]);
        assert!(matches!(decode_decision_graph(value, &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { expected: "u32", .. })));
    }

    #[test]
    fn all_persisted_enum_variants_and_optional_rationale_match_serde() {
        let populations = [
            r#"{"kind":"all"}"#.to_owned(), r#"{"kind":"new_work"}"#.to_owned(),
            r#"{"kind":"created_after","generation":9}"#.to_owned(),
            r#"{"kind":"records","records":[{"entity":"e","record":"r"}]}"#.to_owned(),
            r#"{"kind":"entity","entity":"e"}"#.to_owned(), r#"{"kind":"where","entity":"e","predicate":{"kind":"today"}}"#.to_owned(),
        ];
        let outcomes = [r#"{"kind":"accept","artifact":"D"}"#.replace('D', D), r#"{"kind":"keep_current"}"#.into(), r#"{"kind":"either_acceptable"}"#.into(), r#"{"kind":"both_needed"}"#.into(), r#"{"kind":"neither_fits"}"#.into(), r#"{"kind":"deferred"}"#.into()];
        let statuses = [r#"{"kind":"active"}"#.to_owned(), r#"{"kind":"pending"}"#.to_owned(), r#"{"kind":"superseded","by":"d1"}"#.to_owned(), r#"{"kind":"withdrawn","adoption":"a1"}"#.to_owned()];
        let terms = [
            r#"{"kind":"literal","value_type":{"kind":"integer"},"value":{"kind":"integer","value":1}}"#,
            r#"{"kind":"observed","point":"p","observable":"o","value_type":{"kind":"text"}}"#,
            r#"{"kind":"view_rows","point":"p","entity":"e"}"#,
            r#"{"kind":"row_action_rows","point":"p","view":"v","entity":"e","binding":"b","action":"a"}"#,
            r#"{"kind":"view_column","point":"p","column":"c","value_type":{"kind":"boolean"}}"#,
            r#"{"kind":"output_count","point":"p","output":"o"}"#,
            r#"{"kind":"output_column","point":"p","output":"o","column":"c","value_type":{"kind":"date"}}"#,
            r#"{"kind":"count","value":{"kind":"view_rows","point":"p","entity":"e"}}"#,
        ];
        let properties = terms.iter().enumerate().map(|(index, term)| {
            let predicate = match index % 5 {
                0 => format!(r#"{{"kind":"equal","left":{term},"right":{term}}}"#),
                1 => format!(r#"{{"kind":"less","left":{term},"right":{term}}}"#),
                2 => format!(r#"{{"kind":"and","values":[{{"kind":"not","value":{{"kind":"equal","left":{term},"right":{term}}}}}]}}"#),
                3 => format!(r#"{{"kind":"or","values":[{{"kind":"equal","left":{term},"right":{term}}}]}}"#),
                _ => format!(r#"{{"kind":"not","value":{{"kind":"equal","left":{term},"right":{term}}}}}"#),
            };
            format!(r#"{{"id":"p{index}","description":"Property {index}","predicate":{predicate}}}"#)
        }).collect::<Vec<_>>().join(",");
        let decisions = (0..6).map(|index| {
            let rationale = if index == 0 { String::new() } else { r#", "rationale":"Context""#.to_owned() };
            format!(r#"{{"id":"d{index}","revision":1,"request":"Request {index}"{rationale},"scope":{{"operations":["op"],"population":{},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{},"status":{},"obligations":[{}],"scenarios":["{D}"],"witness":"{D}","supersedes":[]}}"#,
                populations[index], outcomes[index], statuses[index % statuses.len()], if index == 0 { properties.as_str() } else { r#"{"id":"simple","description":"Simple","predicate":{"kind":"equal","left":{"kind":"output_count","point":"p","output":"o"},"right":{"kind":"output_count","point":"p","output":"o"}}}"# })
        }).collect::<Vec<_>>().join(",");
        let source = format!(r#"{{"version":1,"revision":9,"decisions":[{decisions}]}}"#);
        let expected: super::super::DecisionGraph = typed(&source);
        let decoded = decode_decision_graph(syntax(&source), &mut AllocationContext::new()).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
    }

    #[test]
    fn allocation_refusal_at_every_graph_owner_growth_cleans_and_retries() {
        let source = graph_source();
        let expected: super::super::DecisionGraph = typed(&source);
        let mut cx = AllocationContext::new();
        let decoded = decode_decision_graph(syntax(&source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        let growths = cx.growth_requests();
        assert!(growths >= 12);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_decision_graph(syntax(&source), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decode_decision_graph(syntax(&source), &mut retry).unwrap()).unwrap());
        }
    }

    #[test]
    fn malformed_unknown_range_depth_duplicate_and_set_semantics_are_preserved() {
        assert!(matches!(decode_decision_graph(syntax(r#"{"version":1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("revision"))));
        assert!(matches!(decode_decision_graph(syntax(r#"{"version":4294967296,"revision":0,"decisions":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u32", .. })));
        assert!(matches!(decode_decision_graph(syntax(r#"{"version":1,"revision":-1,"decisions":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u64", .. })));
        assert!(matches!(decode_decision_graph(syntax(r#"{"version":"one","revision":0,"decisions":[]}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { actual: JsonKind::String, expected: "u32" })));
        assert!(matches!(decode_population(syntax(r#"{"kind":"created_after","generation":-1}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u64", .. })));
        assert!(matches!(decode_outcome(syntax(r#"{"kind":"unknown"}"#)), Err(ValueDecodeError::UnknownVariant { .. })));
        assert!(matches!(decode_status(syntax(r#"{"kind":"active","extra":true}"#)), Err(ValueDecodeError::UnknownField { .. })));
        let unsupported = decode_decision_graph(syntax(r#"{"version":2,"revision":1,"decisions":[]}"#), &mut AllocationContext::new()).unwrap();
        assert_eq!(unsupported.version, 2, "structural decoding preserves the version for later semantic validation");

        let mut source = graph_source();
        source = source.replace(r#""operations":["edit","edit","inspect"]"#, r#""operations":["op","op","op"]"#);
        let expected: super::super::DecisionGraph = typed(&source);
        let decoded = decode_decision_graph(syntax(&source), &mut AllocationContext::new()).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());

        let mut cx = AllocationContext::new();
        let duplicate_error = match product_json::parse_json_bytes(br#"{"version":1,"version":1}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate graph member was accepted"),
        };
        assert_eq!(duplicate_error.cause, Cause::DuplicateObjectMember);

        let nested = |depth: usize| {
            let mut term = r#"{"kind":"output_count","point":"p","output":"o"}"#.to_owned();
            for _ in 0..depth { term = format!(r#"{{"kind":"count","value":{term}}}"#); }
            let pred = format!(r#"{{"kind":"equal","left":{term},"right":{term}}}"#);
            format!(r#"{{"id":"p","description":"P","predicate":{pred}}}"#)
        };
        let property = nested(MAX_EXPR_DEPTH - 1);
        let source = format!(r#"{{"id":"d","revision":1,"request":"R","scope":{{"operations":["o"],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"pending"}},"obligations":[{property}],"scenarios":["{D}"],"witness":"{D}","supersedes":[]}}"#);
        let decision = format!(r#"{{"version":1,"revision":1,"decisions":[{source}]}}"#);
        assert!(decode_decision_graph(syntax(&decision), &mut AllocationContext::new()).is_ok());
        let property = nested(MAX_EXPR_DEPTH);
        let source = format!(r#"{{"id":"d","revision":1,"request":"R","scope":{{"operations":["o"],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"pending"}},"obligations":[{property}],"scenarios":["{D}"],"witness":"{D}","supersedes":[]}}"#);
        let decision = format!(r#"{{"version":1,"revision":1,"decisions":[{source}]}}"#);
        assert!(matches!(decode_decision_graph(syntax(&decision), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "property expression depth", .. })));

        let nested_type = |depth: usize| {
            let mut value_type = r#"{"kind":"text"}"#.to_owned();
            for _ in 0..depth { value_type = format!(r#"{{"kind":"optional","item":{value_type}}}"#); }
            format!(r#"{{"kind":"observed","point":"p","observable":"o","value_type":{value_type}}}"#)
        };
        let property_term = nested_type(super::super::MAX_TYPE_DEPTH);
        assert!(decode_property_term(syntax(&property_term), 0, &mut PropertyBudget { nodes: 0 }, &mut AllocationContext::new()).is_ok());
        let property_term = nested_type(super::super::MAX_TYPE_DEPTH + 1);
        assert!(matches!(decode_property_term(syntax(&property_term), 0, &mut PropertyBudget { nodes: 0 }, &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "type nesting depth", limit: super::super::MAX_TYPE_DEPTH })));

        let nested_child = r#"{"kind":"not","value":{"kind":"and","values":[]}}"#;
        let child_group = format!(r#"{{"kind":"and","values":[{}]}}"#, vec![nested_child; 16].join(","));
        let node_predicate = format!(r#"{{"kind":"and","values":[{}]}}"#, vec![child_group; 256].join(","));
        let property = format!(r#"{{"id":"p","description":"P","predicate":{node_predicate}}}"#);
        let decision = format!(r#"{{"id":"d","revision":1,"request":"R","scope":{{"operations":[],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"pending"}},"obligations":[{property}],"scenarios":[],"witness":"{D}","supersedes":[]}}"#);
        let graph = format!(r#"{{"version":1,"revision":1,"decisions":[{decision}]}}"#);
        assert!(matches!(decode_decision_graph(syntax(&graph), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "property expression node count", limit: MAX_AST_NODES })));
    }

    #[test]
    fn decision_collection_bounds_preserve_duplicate_set_deserialization() {
        let duplicate_operations = vec![r#""op""#; MAX_ITEMS + 1].join(",");
        let source = format!(r#"{{"version":1,"revision":1,"decisions":[{{"id":"d","revision":1,"request":"R","scope":{{"operations":[{duplicate_operations}],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"pending"}},"obligations":[],"scenarios":[],"witness":"{D}","supersedes":[]}}]}}"#);
        assert!(decode_decision_graph(syntax(&source), &mut AllocationContext::new()).is_ok());

        let operations = (0..=MAX_ITEMS).map(|index| format!(r#""op{index:03}""#)).collect::<Vec<_>>().join(",");
        let source = format!(r#"{{"version":1,"revision":1,"decisions":[{{"id":"d","revision":1,"request":"R","scope":{{"operations":[{operations}],"population":{{"kind":"all"}},"conditions":{{}},"excluded_records":[],"unknowns":[]}},"outcome":{{"kind":"keep_current"}},"status":{{"kind":"pending"}},"obligations":[],"scenarios":[],"witness":"{D}","supersedes":[]}}]}}"#);
        assert!(matches!(decode_decision_graph(syntax(&source), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "decision operation count", limit: MAX_ITEMS })));

        let over_decisions = format!(r#"{{"version":1,"revision":1,"decisions":[{}]}}"#, vec!["null"; MAX_ITEMS + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_decision_graph(syntax(&over_decisions), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "decision count", limit: MAX_ITEMS })));
        assert_eq!(cx.growth_requests(), 0);

        let over_predicate = format!(r#"{{"kind":"and","values":[{}]}}"#, vec!["null"; MAX_ITEMS + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_property_predicate(syntax(&over_predicate), 0, &mut PropertyBudget { nodes: 0 }, &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "property predicate item count", limit: MAX_ITEMS })));
        assert_eq!(cx.growth_requests(), 0);

        let over_population_records = format!(r#"{{"kind":"records","records":[{}]}}"#, vec!["null"; MAX_COLLECTION + 1].join(","));
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_population(syntax(&over_population_records), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "population record count", limit: MAX_COLLECTION })));
        assert_eq!(cx.growth_requests(), 0);
    }

    #[test]
    fn decision_strings_move_from_the_json_tree_without_new_allocation() {
        let source = r#"{"id":"雪","revision":1,"request":"Keep this decision","rationale":"Résumé","scope":{"operations":[],"population":{"kind":"all"},"conditions":{},"excluded_records":[],"unknowns":[]},"outcome":{"kind":"keep_current"},"status":{"kind":"pending"},"obligations":[],"scenarios":[],"witness":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","supersedes":[]}"#;
        let mut cx = AllocationContext::new();
        let decoded = decode_scoped_decision(syntax(source), &mut cx).unwrap();
        assert_eq!(decoded.id, "雪");
        assert_eq!(decoded.rationale.as_deref(), Some("Résumé"));
        assert_eq!(cx.growth_requests(), 0);
    }
}
