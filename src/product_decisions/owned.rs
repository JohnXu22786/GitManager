//! Decision-owned preparation, experience and first-layer choice. Lifecycle
//! acceptance is separate from the business choice and its locked verification.
use super::*;
use crate::product_store::scope::{AcceptedPreviewLifecycle, OwnedPreviewRequest, ProofBudget};
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Clone, PartialEq, Eq)]
pub(super) struct OwnedDecisionContext {
    pub prepared: PreparedOwnedScopedChangeV2,
    pub mode: OwnedChoiceMode,
    pub next: Option<DecisionGraph>,
}
pub(super) enum ChangeAuthority {
    Ordinary,
    ScalarActivation(PreparedScopedChange),
    ScalarRecording(Vec<PreparedScopedChange>, Vec<Id>),
    Owned(OwnedDecisionContext),
}
impl<R: RuntimeAdapter> DecisionEngine<R> {
    pub(super) fn with_owned<T>(
        &self,
        owned: OwnedDecisionContext,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if self.pending_scope.borrow().is_some()
            || self.pending_rehearsal.borrow().is_some()
            || !self.pending_correspondences.borrow().is_empty()
        {
            return Err(invalid("owned and scalar decision modes cannot coexist"));
        }
        self.clear_admission_cache();
        let old = self.pending_owned.replace(Some(owned));
        let result = f();
        self.pending_owned.replace(old);
        self.clear_admission_cache();
        result
    }
    /// Structural preparation and fresh candidate diagnostics. A non-Ready
    /// report never authorizes activation, but does not invent satisfaction or
    /// forbid recording an honest nonbinary comparison against healthy current.
    pub fn prepare_owned_change(
        &self,
        store: &ProductStore,
        candidate: &CapturedProgram,
        request: &OwnedPreviewRequest,
        lifecycle: &AcceptedPreviewLifecycle,
        operation: &str,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(PreparedOwnedScopedChangeV2, CheckReport)> {
        let current = store.load()?;
        if self.archive.snapshot()? != current {
            return Err(invalid(
                "owned preparation archive differs from current store",
            ));
        }
        let prepared = crate::product_store::scope::prepare_owned(
            &current, candidate, request, lifecycle, operation, budget, cancelled,
        )?;
        let report = self.with_owned(
            OwnedDecisionContext {
                prepared: prepared.clone(),
                mode: OwnedChoiceMode::Activate,
                next: None,
            },
            || {
                self.check_bound(
                    &current.decisions,
                    prepared.target(),
                    &self.current_mappings(&current)?,
                )
            },
        )?;
        if store.load()? != current {
            return Err(invalid("current changed during owned preparation"));
        }
        Ok((prepared, report))
    }
    fn capture_owned(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedScopedChangeV2,
        original: &ScenarioSpec,
        disclosure: Disclosure,
        alternative: bool,
    ) -> Result<AcceptedScene> {
        let current = store.load()?;
        let projected = prepared.original_frame(&current, original)?;
        let (target, scene) = if alternative {
            (prepared.target(), &projected)
        } else {
            (current.program()?, original)
        };
        self.with_owned(
            OwnedDecisionContext {
                prepared: prepared.clone(),
                mode: OwnedChoiceMode::Activate,
                next: None,
            },
            || {
                let (evidence, _) = self.execute_scene(
                    target,
                    scene,
                    &current.decisions,
                    self.limits.clone(),
                    "accepted",
                )?;
                if evidence.state != EvidenceState::Observed {
                    return Err(invalid("owned scene must execute completely"));
                }
                if store.load()? != current {
                    return Err(invalid("current changed during owned experience"));
                }
                Ok(AcceptedScene {
                    program: target.clone(),
                    scenario: scene.clone(),
                    evidence,
                    disclosure,
                    bind_outcome: true,
                })
            },
        )
    }
    pub fn accept_owned_current_scene(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedScopedChangeV2,
        original: &ScenarioSpec,
        disclosure: Disclosure,
    ) -> Result<AcceptedScene> {
        self.capture_owned(store, prepared, original, disclosure, false)
    }
    pub fn accept_owned_scene(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedScopedChangeV2,
        original: &ScenarioSpec,
        disclosure: Disclosure,
    ) -> Result<AcceptedScene> {
        self.capture_owned(store, prepared, original, disclosure, true)
    }
    pub fn prepare_owned_choice(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedScopedChangeV2,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        id: &str,
    ) -> Result<VerifiedChange> {
        self.prepare_owned_choice_resolving(store, prepared, choice, scenes, &[], id)
    }
    pub fn prepare_owned_resolution(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedScopedChangeV2,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        resolves: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        if resolves.is_empty() {
            return Err(invalid("resolution needs exact pending IDs"));
        }
        self.prepare_owned_choice_resolving(store, prepared, choice, scenes, resolves, id)
    }
    fn prepare_owned_choice_resolving(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedScopedChangeV2,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        resolves: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        prepared.check_basis(&store.load()?)?;
        if prepared.operation_id() != id
            || choice.scope != prepared.scope()
            || !matches!(&choice.outcome, DecisionOutcome::Accept { artifact } if artifact == &prepared.target().artifact.program_digest)
            || scenes.iter().any(|s| s.program() != prepared.target())
        {
            return Err(invalid("owned activation needs exact durable target, scope, operation and Accept; owned KeepCurrent is unsupported"));
        }
        let target = prepared.target().clone();
        self.with_owned(
            OwnedDecisionContext {
                prepared,
                mode: OwnedChoiceMode::Activate,
                next: None,
            },
            || self.prepare_choice_resolving(store, &target, choice, scenes, resolves, id),
        )
    }
    pub fn prepare_owned_rehearsed_choice(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedScopedChangeV2,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        resolves: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        prepared.check_basis(&current)?;
        if choice.scope != prepared.scope()
            || !matches!(
                choice.outcome,
                DecisionOutcome::EitherAcceptable
                    | DecisionOutcome::BothNeeded
                    | DecisionOutcome::NeitherFits
                    | DecisionOutcome::Deferred
            )
            || !scenes
                .iter()
                .any(|s| s.program() == current.program().expect("validated source"))
            || !scenes.iter().any(|s| s.program() == prepared.target())
            || scenes.iter().any(|s| {
                s.program() != current.program().expect("validated source")
                    && s.program() != prepared.target()
            })
        {
            return Err(invalid(
                "owned recording needs exact paired sources/scope and a nonbinary outcome",
            ));
        }
        // Reject malformed pair before any recording can be prepared, in
        // addition to the candidate archive's independent final closure check.
        for scene in &scenes {
            let mut original = scene.scenario().clone();
            let on_current = scene.program() == current.program()?;
            if !on_current {
                original.seed = current.data.clone();
            }
            let projected = prepared.original_frame(&current, &original)?;
            if !on_current && scene.scenario() != &projected
                || !scenes.iter().any(|other| {
                    other.program()
                        == if on_current {
                            prepared.target()
                        } else {
                            current.program().expect("validated source")
                        }
                        && other.scenario() == if on_current { &projected } else { &original }
                })
            {
                return Err(invalid("owned recording scene has no exact counterpart"));
            }
        }
        let ids = vec![choice.id.clone()];
        self.with_owned(
            OwnedDecisionContext {
                prepared,
                mode: OwnedChoiceMode::Record(ids),
                next: None,
            },
            || {
                self.prepare_choice_resolving(
                    store,
                    current.program()?,
                    choice,
                    scenes,
                    resolves,
                    id,
                )
            },
        )
    }
    pub(super) fn adopt_owned(
        &self,
        store: &ProductStore,
        change: &VerifiedChange,
        owned: &OwnedDecisionContext,
        #[cfg(test)] fault: Option<crate::product_store::FaultPoint>,
    ) -> Result<ProjectSnapshot> {
        self.with_owned(owned.clone(), || {
            Ok(store.adopt_owned_verified(
                change.revision,
                &owned.prepared,
                &owned.mode,
                &change.prepared,
                &change.decisions,
                |current, target, next, plan, candidate| {
                    if canonical_digest(IdentityDomain::Data, current)? != change.expected_snapshot
                        || self.runtime.capabilities() != change.runtime
                        || self.limits != change.limits
                        || next != &change.decisions
                        || target != &change.target
                        || plan != change.prepared.plan()
                    {
                        return Err(StoreError::Conflict(
                            "owned intention rehearsal changed".into(),
                        ));
                    }
                    let verify = || -> Result<()> {
                        validate_withdrawal_history(current)?;
                        validate_withdrawal_delta(current, next, plan)?;
                        self.verify_destination_packages(store, current, next, plan)?;
                        for decision in &next.decisions {
                            if !current
                                .decisions
                                .decisions
                                .iter()
                                .any(|d| d.id == decision.id && d.witness == decision.witness)
                            {
                                for scene in self.archive.load(&decision.witness)? {
                                    self.reproduce_scene(&scene, next)?;
                                }
                            }
                        }
                        let report = self.check_bound(next, target, &change.mappings)?;
                        if report.disposition != CheckDisposition::Ready {
                            return Err(DecisionError::Unverified(
                                "owned save fails independent intention replay".into(),
                            ));
                        }
                        IntentArchive::new(store.clone()).export_for(candidate)?;
                        Ok(())
                    };
                    verify().map_err(|e| StoreError::Invalid(e.to_string()))
                },
                |current| {
                    IntentArchive::new(store.clone())
                        .export_for(current)
                        .map(|_| ())
                        .map_err(|e| StoreError::Invalid(e.to_string()))
                },
                #[cfg(test)]
                fault,
            )?)
        })
    }
    #[cfg(test)]
    pub(crate) fn adopt_owned_with_fault(
        &self,
        store: &ProductStore,
        change: &VerifiedChange,
        fault: crate::product_store::FaultPoint,
    ) -> Result<ProjectSnapshot> {
        let ChangeAuthority::Owned(owned) = &change.authority else {
            return Err(invalid("owned fault helper requires verified owned choice"));
        };
        self.adopt_owned(store, change, owned, Some(fault))
    }
}
#[cfg(test)]
impl<R: RuntimeAdapter> DecisionEngine<R> {
    pub(crate) fn owned_context_for_test(
        &self,
        change: &VerifiedChange,
    ) -> Result<(ScopedExecutionContext, DecisionGraph)> {
        let ChangeAuthority::Owned(owned) = &change.authority else {
            return Err(invalid("owned context required"));
        };
        self.with_owned(owned.clone(), || {
            Ok((self.scope_context()?, change.decisions.clone()))
        })
    }
}
