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
const SCOPED_DECISION_FIELDS: &[&str] = &[
    "id", "revision", "request", "rationale", "scope", "outcome", "status", "obligations", "scenarios", "witness", "supersedes",
];
const DECISION_SCOPE_FIELDS: &[&str] = &["operations", "population", "conditions", "excluded_records", "unknowns"];
const UNKNOWN_BOUNDARY_FIELDS: &[&str] = &["id", "operations", "description"];
const POPULATION_VARIANTS: &[&str] = &["all", "new_work", "created_after", "records", "entity", "where"];
const OUTCOME_VARIANTS: &[&str] = &["accept", "keep_current", "either_acceptable", "both_needed", "neither_fits", "deferred"];
const STATUS_VARIANTS: &[&str] = &["active", "pending", "superseded", "withdrawn"];
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
pub(super) struct DecodedUnknownBoundary {
    pub(super) id: String,
    pub(super) operations: FallibleStringSet,
    pub(super) description: String,
}

pub(super) struct FallibleStringSet(crate::product_alloc::ordered::OrderedSet<String>);

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
    let fields = object(value, "a struct DecisionGraph")?;
    let mut version = None;
    let mut revision = None;
    let mut decoded_decisions = None;
    for (key, value) in fields.into_entries() {
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
    let fields = object(value, "a struct ScopedDecision")?;
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
    for (key, value) in fields.into_entries() {
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
    let fields = object(value, "a struct DecisionScope")?;
    let mut operations = None;
    let mut population = None;
    let mut conditions = None;
    let mut excluded_records = None;
    let mut unknowns = None;
    for (key, value) in fields.into_entries() {
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
    let fields = object(value, "a struct UnknownBoundary")?;
    let mut id = None;
    let mut operations = None;
    let mut description = None;
    for (key, value) in fields.into_entries() {
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
    let fields = object(value, "a struct AcceptedProperty")?;
    let mut id = None;
    let mut description = None;
    let mut predicate = None;
    let mut budget = PropertyBudget { nodes: 0 };
    for (key, value) in fields.into_entries() {
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
    for value in values { decoded.push(decode_record_ref(value)?); }
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
        assert!(matches!(failure.site, AllocationSite::Vector | AllocationSite::OrderedMap | AllocationSite::OrderedSet | AllocationSite::Box));
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
