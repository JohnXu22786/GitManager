use super::*;

/// Validate history linkage, not imported execution success flags.
pub(super) fn validate_withdrawal_history(snapshot: &ProjectSnapshot) -> Result<()> {
    let mut groups = std::collections::BTreeMap::<&str, BTreeSet<&str>>::new();
    for decision in &snapshot.decisions.decisions {
        if let DecisionStatus::Withdrawn { adoption } = &decision.status {
            groups.entry(adoption).or_default().insert(&decision.id);
        }
    }
    for (id, decisions) in groups {
        let receipts: Vec<_> = snapshot
            .adoptions
            .iter()
            .filter(|r| r.plan.id == id)
            .collect();
        if receipts.len() != 1 {
            return Err(invalid("withdrawn history has no unique adoption receipt"));
        }
        let receipt = receipts[0];
        if receipt
            .plan
            .retire_decisions
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != decisions
            || receipt
                .plan
                .required_decisions
                .iter()
                .any(|id| decisions.contains(id.as_str()))
        {
            return Err(invalid(
                "withdrawn decisions do not match the exact adoption retirement set",
            ));
        }
        let target = snapshot.programs.iter().find(|p| {
            canonical_digest(IdentityDomain::Source, *p).ok().as_ref() == Some(&receipt.active)
        });
        let previous = snapshot.programs.iter().find(|p| {
            canonical_digest(IdentityDomain::Source, *p).ok().as_ref() == Some(&receipt.previous)
        });
        if target.map(|p| &p.artifact) != Some(&receipt.plan.target)
            || previous.map(|p| &p.binding) != Some(&receipt.plan.current_source)
        {
            return Err(invalid(
                "withdrawal receipt is not bound to its actual program transition",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_withdrawal_delta(
    current: &ProjectSnapshot,
    next: &DecisionGraph,
    plan: &AdoptionPlan,
) -> Result<()> {
    let mut withdrawn = BTreeSet::new();
    for decision in &next.decisions {
        let Some(old) = current
            .decisions
            .decisions
            .iter()
            .find(|d| d.id == decision.id)
        else {
            if matches!(decision.status, DecisionStatus::Withdrawn { .. }) {
                return Err(invalid("new decisions cannot arrive already withdrawn"));
            }
            continue;
        };
        if matches!(old.status, DecisionStatus::Withdrawn { .. }) && old != decision {
            return Err(invalid(
                "terminal withdrawn history cannot be rewritten or revived",
            ));
        }
        if let DecisionStatus::Withdrawn { adoption } = &decision.status {
            if old.status == decision.status {
                continue;
            }
            if old.status != DecisionStatus::Active || adoption != &plan.id {
                return Err(invalid(
                    "withdrawal must name an active decision and this exact adoption",
                ));
            }
            let mut expected = old.clone();
            expected.status = decision.status.clone();
            expected.revision = expected
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("decision revision overflow"))?;
            if expected != *decision {
                return Err(invalid("withdrawal altered the historical intention"));
            }
            withdrawn.insert(decision.id.as_str());
        }
    }
    if !withdrawn.is_empty()
        && withdrawn != plan.retire_decisions.iter().map(String::as_str).collect()
    {
        return Err(invalid(
            "withdrawal changed the explicitly named retirement set",
        ));
    }
    Ok(())
}

impl<R: RuntimeAdapter> DecisionEngine<R> {
    /// Call only after an explicit choice to withdraw these exact decisions.
    /// History stays terminal; no successor preference or old database is invented.
    pub fn prepare_withdrawal(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        withdraw: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        validate_withdrawal_history(&current)?;
        if !current.programs.contains(target)
            || withdraw.is_empty()
            || withdraw.iter().collect::<BTreeSet<_>>().len() != withdraw.len()
        {
            return Err(invalid(
                "withdrawal needs a retained program and exact distinct decision IDs",
            ));
        }
        let mut next = current.decisions.clone();
        for prior in withdraw {
            let decision = next
                .decisions
                .iter_mut()
                .find(|d| &d.id == prior && d.status == DecisionStatus::Active)
                .ok_or_else(|| invalid("only a named active decision can be withdrawn"))?;
            decision.status = DecisionStatus::Withdrawn {
                adoption: id.into(),
            };
            decision.revision = decision
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("decision revision overflow"))?;
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision revision overflow"))?;
        next.validate()?;
        let result = self.prepare(
            store,
            &current,
            target,
            next,
            self.mappings_for(&current, target)?,
            withdraw.to_vec(),
            id,
        )?;
        let recovery = RecoveryPlan {
            adoption: result.prepared.plan().clone(),
            withdraw_decisions: withdraw.to_vec(),
            preserve_current_data: current.data.identity()?,
            preserve_current_events: canonical_digest(IdentityDomain::Data, &current.data.events)?,
        };
        store.prepare_recovery(&recovery, target)?;
        Ok(result)
    }
}

impl<R: RuntimeAdapter> DecisionEngine<R> {
    /// Withdraw exact behavior layers on current data. Their own accepted
    /// decisions retire; unrelated active promises are independently replayed.
    pub fn prepare_scoped_withdrawal(
        &self,
        store: &ProductStore,
        layers: &[Digest],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        let prepared = store.prepare_scoped_withdrawal(layers, id)?;
        let mut retire = BTreeSet::new();
        for layer in layers {
            let manifest = current
                .scope
                .layers
                .get(layer)
                .ok_or_else(|| invalid("unknown scope layer"))?;
            let adoption = current
                .adoptions
                .iter()
                .find(|a| a.plan.id == manifest.operation)
                .ok_or_else(|| invalid("scope layer adoption missing"))?;
            let receipt = current
                .scope
                .adoptions
                .iter()
                .find(|r| r.revision == adoption.revision)
                .ok_or_else(|| invalid("scoped decision receipt missing"))?;
            retire.extend(receipt.decisions.iter().cloned());
        }
        // A promise can have been explicitly retired before its executable
        // layer. Preserve that terminal decision and its original receipt.
        retire.retain(|id| {
            current
                .decisions
                .decisions
                .iter()
                .any(|d| &d.id == id && d.status == DecisionStatus::Active)
        });
        let mut next = current.decisions.clone();
        for decision in &mut next.decisions {
            if retire.contains(&decision.id) {
                decision.status = DecisionStatus::Withdrawn {
                    adoption: id.into(),
                };
                decision.revision = decision
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("decision revision overflow"))?;
            }
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision revision overflow"))?;
        let target = prepared.target().clone();
        let mappings = self.compose_for_current(&current, &target, &[])?;
        self.with_scoped(Some(prepared), || {
            self.prepare(
                store,
                &current,
                &target,
                next,
                mappings,
                retire.into_iter().collect(),
                id,
            )
        })
    }
}
