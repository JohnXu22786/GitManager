//! Bounded read-only analysis, deliberately separate from executable preparation.
use super::owned_contract::*;
use crate::product_contract::*;
use crate::product_store::{ProductStore, ProjectSnapshot, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Untrusted suggestions. The store supplies and authenticates the data basis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedEffectRequest {
    pub snapshot: Digest,
    pub sources: OwnedSourceRequest,
    pub auxiliary: Vec<OwnedAuxiliary>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnedAuxiliary {
    pub entity: Id,
    pub owner_field: Id,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedRead {
    pub location: String,
    pub entity: Id,
    pub field: Id,
}
/// Diagnostic evidence only. It cannot be passed to adoption or preparation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OwnedEffectReport {
    pub snapshot: Digest,
    pub structure: OwnedStructuralReport,
    pub auxiliary: Vec<OwnedAuxiliary>,
    pub reads: Vec<OwnedRead>,
    /// Closed-use diagnostics; no lifecycle or executable proof is conveyed.
    pub common_operations: Vec<super::OwnedCommonOperation>,
}
// Span resolution has already authenticated every path before this helper runs.
// Preserve existing iterator headers even when an entire loop lies in a span.
// New loops may follow the original lexical prefix; no filter is pushed down.
fn preserve_selections(
    before: &AppDefinition,
    after: &AppDefinition,
    report: &OwnedStructuralReport,
) -> Result<(), StoreError> {
    fn span<'a>(app: &'a AppDefinition, endpoint: &OwnedSpanEndpoint) -> &'a [Statement] {
        let mut list = app
            .actions
            .iter()
            .find(|a| a.id == endpoint.action)
            .expect("resolved action")
            .steps
            .as_slice();
        for index in &endpoint.list_path {
            let Statement::ForEach { steps, .. } = &list[*index] else {
                unreachable!("resolved list path");
            };
            list = steps;
        }
        &list[endpoint.start..endpoint.start + endpoint.len]
    }
    fn headers<'a>(steps: &'a [Statement], result: &mut Vec<(&'a Id, &'a Expr)>) {
        for step in steps {
            if let Statement::ForEach {
                items,
                binding,
                steps,
            } = step
            {
                result.push((binding, items));
                headers(steps, result);
            }
        }
    }
    for edit in &report.edits {
        let mut old = vec![];
        let mut new = vec![];
        headers(span(before, &edit.before), &mut old);
        headers(span(after, &edit.after), &mut new);
        if !new.starts_with(&old) {
            return Err(StoreError::Invalid(format!("Owned effect safety: {} {:?} at {}: existing outer selection/header changed in span {}", edit.before.action, edit.before.list_path, edit.before.start, edit.id)));
        }
    }
    Ok(())
}

// This value is created only after exact source and authenticated basis checks.
// It deliberately implements neither Serialize nor Deserialize. The public
// report is diagnostic data, never a preparation or future proof input.
struct VerifiedOwnedAnalysis {
    report: OwnedEffectReport,
}
impl VerifiedOwnedAnalysis {
    fn derive(
        snapshot: &ProjectSnapshot,
        candidate: &CapturedProgram,
        request: &OwnedEffectRequest,
    ) -> Result<Self, StoreError> {
        let basis = canonical_digest(IdentityDomain::Data, snapshot)?;
        if basis != request.snapshot || snapshot.active_revision != request.sources.before {
            return Err(StoreError::Invalid(
                "Owned effect safety: stale authenticated snapshot/source basis".into(),
            ));
        }
        let structure =
            super::analyze_owned_sources(snapshot.program()?, candidate, &request.sources)?;
        preserve_selections(&snapshot.program()?.program, &candidate.program, &structure)?;
        if snapshot.scope != super::ScopeState::default() {
            return Err(StoreError::Invalid(
                "Owned effect safety: existing scope lineage is unsupported".into(),
            ));
        }
        let added: BTreeSet<_> = structure
            .changes
            .iter()
            .filter(|c| c.kind == OwnedStructuralChangeKind::EntityAdded)
            .map(|c| c.id.clone())
            .collect();
        let mut declared = BTreeSet::new();
        for auxiliary in &request.auxiliary {
            let fail = |reason: &str| {
                StoreError::Invalid(format!(
                    "Owned effect safety: entity {}: {reason}",
                    auxiliary.entity
                ))
            };
            if !declared.insert(auxiliary.entity.clone()) || !added.contains(&auxiliary.entity) {
                return Err(fail("duplicate or non-additive auxiliary declaration"));
            }
            if snapshot
                .data
                .schema
                .iter()
                .any(|e| e.id == auxiliary.entity)
                || snapshot
                    .data
                    .records
                    .iter()
                    .any(|r| r.entity == auxiliary.entity)
            {
                return Err(fail(
                    "auxiliary entity is already present in retained schema/storage",
                ));
            }
            let entity = candidate
                .program
                .entities
                .iter()
                .find(|e| e.id == auxiliary.entity)
                .expect("structurally resolved addition");
            if !entity.fields.iter().any(|f| {
                f.id == auxiliary.owner_field
                    && f.value_type == Type::reference(&structure.root_entity)
            }) {
                return Err(fail("owner must be one required typed root reference"));
            }
            if entity.unique != vec![vec![auxiliary.owner_field.clone()]]
                || !entity.constraints.is_empty()
            {
                return Err(fail(
                    "only owner-only uniqueness without constraints is supported",
                ));
            }
        }
        if added != declared {
            return Err(StoreError::Invalid(
                "Owned effect safety: every additive entity needs an auxiliary owner declaration"
                    .into(),
            ));
        }
        let common = super::owned_common::VerifiedCommonOperations::derive(
            snapshot,
            &candidate.program,
            &structure,
            &basis,
        )?;
        let reads = super::owned_analysis::analyze(
            &snapshot.program()?.program,
            &candidate.program,
            &structure,
            &request.auxiliary,
            &common,
        )?;
        Ok(Self {
            report: OwnedEffectReport {
                snapshot: basis,
                structure,
                auxiliary: request.auxiliary.clone(),
                reads,
                common_operations: common.reports(),
            },
        })
    }
}
impl ProductStore {
    /// Analyze an exact source pair against the currently loaded, validated store.
    /// No source, schema, session, proof object or activation pointer is written.
    /// Completed-history and executable-lowering obligations remain separate.
    pub fn analyze_owned_effects(
        &self,
        candidate: &CapturedProgram,
        request: &OwnedEffectRequest,
    ) -> Result<OwnedEffectReport, StoreError> {
        Ok(VerifiedOwnedAnalysis::derive(&self.load()?, candidate, request)?.report)
    }
}
