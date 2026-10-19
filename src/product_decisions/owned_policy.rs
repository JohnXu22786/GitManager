//! Closed whole-obligation ownership. These interval calculations never mint
//! source, replay, graph, or publication authority by themselves.
use super::*;
type Intervals = Vec<(i64, i64)>;

fn push_range(
    out: &mut Intervals,
    lo: i64,
    hi: i64,
    charge: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<()> {
    if lo <= hi {
        charge(1)?;
        out.push((lo, hi));
    }
    Ok(())
}
fn union(
    a: Intervals,
    b: Intervals,
    charge: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<Intervals> {
    let mut result: Intervals = vec![];
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        charge(1)?;
        let next = if j == b.len() || (i < a.len() && a[i].0 <= b[j].0) {
            let value = a[i];
            i += 1;
            value
        } else {
            let value = b[j];
            j += 1;
            value
        };
        if let Some(last) = result.last_mut() {
            // Endpoints are in [0, MAX_COLLECTION], so adjacency cannot overflow.
            if next.0 <= last.1 + 1 {
                last.1 = last.1.max(next.1);
                continue;
            }
        }
        push_range(&mut result, next.0, next.1, charge)?;
    }
    Ok(result)
}
fn intersect(
    a: Intervals,
    b: Intervals,
    charge: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<Intervals> {
    let mut result = vec![];
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        charge(1)?;
        push_range(&mut result, a[i].0.max(b[j].0), a[i].1.min(b[j].1), charge)?;
        if a[i].1 < b[j].1 {
            i += 1;
        } else {
            j += 1;
        }
    }
    Ok(result)
}
fn complement(
    values: Intervals,
    cap: i64,
    charge: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<Intervals> {
    let mut result = vec![];
    let mut start = 0;
    for (lo, hi) in values {
        charge(1)?;
        push_range(&mut result, start, lo - 1, charge)?;
        start = hi + 1;
    }
    push_range(&mut result, start, cap, charge)?;
    Ok(result)
}

/// The caller's qualifier is a read of an already source-proved surface, never
/// an allowlist of projection names. Every atom is qualified, including leaves
/// that ordinary Boolean evaluation could short-circuit.
fn predicate_intervals(
    predicate: &PropertyPredicate,
    qualify: &mut dyn FnMut(&str, &str) -> Result<i64>,
    charge: &mut dyn FnMut(u64) -> Result<()>,
) -> Result<(Id, Id, Intervals)> {
    fn walk(
        predicate: &PropertyPredicate,
        key: &mut Option<(Id, Id, i64)>,
        qualify: &mut dyn FnMut(&str, &str) -> Result<i64>,
        charge: &mut dyn FnMut(u64) -> Result<()>,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<Intervals> {
        charge(1)?;
        *nodes = nodes
            .checked_add(1)
            .ok_or_else(|| invalid("policy predicate node overflow"))?;
        if depth > MAX_EXPR_DEPTH || *nodes > MAX_AST_NODES {
            return Err(invalid("policy predicate exceeds bounded grammar"));
        }
        match predicate {
            PropertyPredicate::Equal { left, right } | PropertyPredicate::Less { left, right } => {
                charge(2)?;
                let (point, observable, literal, reversed) = match (left, right) {
                    (
                        PropertyTerm::Observed {
                            point,
                            observable,
                            value_type: Type::Integer,
                        },
                        PropertyTerm::Literal {
                            value_type: Type::Integer,
                            value: DataValue::Integer { value },
                        },
                    ) => (point, observable, *value, false),
                    (
                        PropertyTerm::Literal {
                            value_type: Type::Integer,
                            value: DataValue::Integer { value },
                        },
                        PropertyTerm::Observed {
                            point,
                            observable,
                            value_type: Type::Integer,
                        },
                    ) => (point, observable, *value, true),
                    _ => return Err(invalid(
                        "owned retirement needs a direct integer policy count and integer literal",
                    )),
                };
                let cap = qualify(point, observable)?;
                if cap <= 0 || cap > MAX_COLLECTION as i64 {
                    return Err(invalid(
                        "owned policy count has no proved nonempty finite domain",
                    ));
                }
                if let Some((prior_point, prior_observable, prior_cap)) = key {
                    if prior_point != point || prior_observable != observable || *prior_cap != cap {
                        return Err(invalid(
                            "one policy property mixes source, point or counter",
                        ));
                    }
                } else {
                    charge((point.len() + observable.len()) as u64)?;
                    *key = Some((point.clone(), observable.clone(), cap));
                }
                let mut result = vec![];
                if matches!(predicate, PropertyPredicate::Equal { .. }) {
                    if 0 <= literal && literal <= cap {
                        push_range(&mut result, literal, literal, charge)?;
                    }
                } else if reversed {
                    if literal < 0 {
                        push_range(&mut result, 0, cap, charge)?;
                    } else if literal < cap {
                        push_range(&mut result, literal + 1, cap, charge)?;
                    }
                } else if literal > cap {
                    push_range(&mut result, 0, cap, charge)?;
                } else if literal > 0 {
                    push_range(&mut result, 0, literal - 1, charge)?;
                }
                Ok(result)
            }
            PropertyPredicate::And { values } | PropertyPredicate::Or { values } => {
                if values.is_empty() {
                    return Err(invalid("empty policy Boolean combination"));
                }
                let mut result = None;
                for value in values {
                    charge(1)?;
                    let next = walk(value, key, qualify, charge, depth + 1, nodes)?;
                    result = Some(match result {
                        None => next,
                        Some(prior) if matches!(predicate, PropertyPredicate::And { .. }) => {
                            intersect(prior, next, charge)?
                        }
                        Some(prior) => union(prior, next, charge)?,
                    });
                }
                Ok(result.expect("nonempty combination checked"))
            }
            PropertyPredicate::Not { value } => {
                let inner = walk(value, key, qualify, charge, depth + 1, nodes)?;
                let cap = key
                    .as_ref()
                    .ok_or_else(|| invalid("policy predicate has no counter"))?
                    .2;
                complement(inner, cap, charge)
            }
        }
    }
    let mut key = None;
    let intervals = walk(predicate, &mut key, qualify, charge, 0, &mut 0)?;
    let (point, observable, cap) =
        key.ok_or_else(|| invalid("policy predicate has no owned counter"))?;
    if intervals.is_empty() || intervals == [(0, cap)] {
        return Err(invalid(
            "owned policy predicate is empty or tautological over its proved domain",
        ));
    }
    Ok((point, observable, intervals))
}

#[cfg(test)]
#[path = "owned_policy_predicate_tests.rs"]
mod predicate_tests;

pub(super) fn retirement_ids(current: &ProjectSnapshot, layer: &Digest) -> Result<Vec<Id>> {
    let state = current.owned_scope()?;
    let original = state
        .layers
        .get(layer)
        .ok_or_else(|| invalid("unknown original owned layer"))?;
    let adoption = current
        .adoptions
        .iter()
        .find(|a| a.plan.id == original.operation)
        .ok_or_else(|| invalid("original owned adoption missing"))?;
    let receipt = state
        .adoptions
        .iter()
        .find(|a| a.revision == adoption.revision)
        .ok_or_else(|| invalid("original owned decision receipt missing"))?;
    let mut ids = vec![];
    for id in &receipt.decisions {
        let decision = current
            .decisions
            .decisions
            .iter()
            .find(|d| &d.id == id)
            .ok_or_else(|| invalid("receipt-linked owned decision missing"))?;
        if decision.status == DecisionStatus::Active {
            ids.push(id.clone());
        }
    }
    ids.sort();
    if ids.windows(2).any(|p| p[0] == p[1]) {
        return Err(invalid("duplicate owned retirement"));
    }
    Ok(ids)
}
pub(super) fn withdrawal_graph(
    current: &ProjectSnapshot,
    ids: &[Id],
    operation: &str,
) -> Result<DecisionGraph> {
    let mut next = current.decisions.clone();
    for id in ids {
        let decision = next
            .decisions
            .iter_mut()
            .find(|d| &d.id == id && d.status == DecisionStatus::Active)
            .ok_or_else(|| invalid("owned withdrawal requires exact active IDs"))?;
        decision.status = DecisionStatus::Withdrawn {
            adoption: operation.into(),
        };
        decision.revision = decision
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision revision overflow"))?;
    }
    if !ids.is_empty() {
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision graph revision overflow"))?;
    }
    next.validate()?;
    Ok(next)
}
pub(super) fn check_decision(
    execution: &crate::product_store::scope::VerifiedOwnedTransitionExecution,
    decision: &ScopedDecision,
    scenes: &[AcceptedScene],
    meter: &mut crate::product_store::scope::ProofMeter,
) -> Result<()> {
    use crate::product_store::scope::{
        transition_layer, transition_policy_scope, transition_policy_surface,
    };
    meter.charge(1)?;
    decision.validate()?;
    if validate_scene_bindings(decision, scenes)? != IntentionBinding::PropertiesOnly
        || decision.scope != transition_policy_scope(execution)?
        || decision.obligations.is_empty()
    {
        return Err(DecisionError::Unverified(
            "owned retirement requires the complete original PropertiesOnly obligation".into(),
        ));
    }
    let ids = scenes
        .iter()
        .map(|s| s.scenario.identity())
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if ids != decision.scenarios {
        return Err(invalid("owned retirement scene identities changed"));
    }
    let mut surfaces: Vec<(
        &CapturedProgram,
        crate::product_store::scope::VerifiedOwnedPolicySurface,
    )> = vec![];
    for scene in scenes {
        meter.charge(1)?;
        scene.validate()?;
        let mut found = None;
        for (index, (source, _)) in surfaces.iter().enumerate() {
            meter.charge(1)?;
            if *source == &scene.program {
                found = Some(index);
                break;
            }
        }
        let index = if let Some(index) = found {
            index
        } else {
            let surface = transition_policy_surface(execution, &scene.program, meter)?;
            meter.charge(1)?;
            surfaces.push((&scene.program, surface));
            surfaces.len() - 1
        };
        let surface = &surfaces[index].1;
        surface.validate_frame(&scene.scenario.seed)?;
        if surface.original_layer() != transition_layer(execution)
            || surface.source() != &canonical_digest(IdentityDomain::Source, &scene.program)?
        {
            return Err(invalid(
                "owned policy surface belongs to another producer or layer",
            ));
        }
        for property in &decision.obligations {
            meter.charge(1)?;
            property.validate()?;
            let mut qualify = |point: &str, observable: &str| -> Result<i64> {
                let cap = surface.qualify_observable(observable).ok_or_else(|| {
                    DecisionError::Unverified(
                        "whole obligation contains an unowned observed surface".into(),
                    )
                })?;
                let observation = scene
                    .evidence
                    .observations
                    .iter()
                    .find(|o| o.point == point)
                    .ok_or_else(|| invalid("owned policy point is absent from its actual scene"))?;
                if observation.value_types.get(observable) != Some(&Type::Integer)
                    || !matches!(observation.values.get(observable),Some(DataValue::Integer { value }) if *value>=0 && *value<=cap)
                {
                    return Err(invalid(
                        "owned policy actual observation type or finite domain differs",
                    ));
                }
                Ok(cap)
            };
            predicate_intervals(&property.predicate, &mut qualify, &mut |units| {
                meter.charge(units).map_err(DecisionError::from)
            })?;
            if property.evaluate(&scene.evidence.observations) != Some(true) {
                return Err(DecisionError::Unverified(
                    "owned policy predicate is not satisfied by its actual accepted scene".into(),
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn check_withdrawal(
    archive: &IntentArchive,
    execution: &crate::product_store::scope::VerifiedOwnedTransitionExecution,
    meter: &mut crate::product_store::scope::ProofMeter,
) -> Result<Vec<Id>> {
    let current = execution.current();
    let ids = retirement_ids(
        current,
        crate::product_store::scope::transition_layer(execution),
    )?;
    for id in &ids {
        meter.charge(1)?;
        let decision = current
            .decisions
            .decisions
            .iter()
            .find(|d| &d.id == id)
            .ok_or_else(|| invalid("owned retirement decision missing"))?;
        let scenes = archive.load(&decision.witness)?;
        check_decision(execution, decision, &scenes, meter)?;
    }
    Ok(ids)
}
