//! Frozen 2f87f870 old reconstructing qualifier; cfg(test) parent only.
use super::*;
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
