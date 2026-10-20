//! Disjoint current-data implementation and exact policy withdrawal authority.
use super::*;
use crate::product_store::scope::{OwnedSourceRequest, PreparedOwnedTransitionV2, ProofBudget};
use std::sync::{atomic::AtomicBool, Arc};

/// Preserve an actual gate/runtime first cause at only the new P5 callbacks.
/// Decision-specific verification failures keep their existing diagnostic form.
pub(super) fn into_store_error(error: DecisionError) -> StoreError {
    match error {
        DecisionError::Store(error) => error,
        DecisionError::Runtime(error) => StoreError::Runtime(error),
        error => StoreError::Invalid(error.to_string()),
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) struct OwnedTransitionDecisionContext {
    pub prepared: PreparedOwnedTransitionV2,
    pub next: Option<DecisionGraph>,
    pub current_scene: bool,
}
pub struct OwnedWithdrawalSummary {
    pub retire_decisions: Vec<Id>,
}
struct Restore<'a, R: RuntimeAdapter> {
    engine: &'a DecisionEngine<R>,
    previous: Option<OwnedTransitionDecisionContext>,
}
impl<R: RuntimeAdapter> Drop for Restore<'_, R> {
    fn drop(&mut self) {
        self.engine
            .pending_owned_transition
            .replace(self.previous.take());
        self.engine.clear_admission_cache();
    }
}
impl<R: RuntimeAdapter> DecisionEngine<R> {
    fn with_owned_transition<T>(
        &self,
        context: OwnedTransitionDecisionContext,
        f: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        if self.pending_scope.borrow().is_some()
            || self.pending_owned.borrow().is_some()
            || self.pending_rehearsal.borrow().is_some()
            || !self.pending_correspondences.borrow().is_empty()
        {
            return Err(invalid(
                "owned transition cannot coexist with another change authority",
            ));
        }
        self.clear_admission_cache();
        let guard = Restore {
            engine: self,
            previous: self.pending_owned_transition.replace(Some(context)),
        };
        let result = f();
        drop(guard);
        result
    }
    pub fn prepare_owned_evolution(
        &self,
        store: &ProductStore,
        before: &CapturedProgram,
        after: &CapturedProgram,
        sources: &OwnedSourceRequest,
        layer: &Digest,
        operation: &str,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(PreparedOwnedTransitionV2, CheckReport)> {
        let current = store.load()?;
        if self.archive.snapshot()? != current {
            return Err(invalid(
                "owned transition archive differs from current store",
            ));
        }
        let prepared = crate::product_store::scope::prepare_owned_evolution(
            &current,
            before,
            after,
            sources,
            layer,
            operation,
            false,
            budget,
            cancelled,
            self.runtime.cancellation_token(),
            |_| Ok(()),
        )?;
        let report = self.with_owned_transition(
            OwnedTransitionDecisionContext {
                prepared: prepared.clone(),
                next: None,
                current_scene: false,
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
            return Err(invalid(
                "current changed during owned evolution preparation",
            ));
        }
        Ok((prepared, report))
    }
    fn capture_owned_transition(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedTransitionV2,
        scene: &ScenarioSpec,
        disclosure: Disclosure,
        alternative: bool,
    ) -> Result<AcceptedScene> {
        let current = store.load()?;
        prepared.original_frame(&current, scene)?;
        let target = if alternative {
            prepared.target()
        } else {
            current.program()?
        };
        let next = if prepared.is_withdrawal() {
            let ids = owned_policy::retirement_ids(&current, prepared.original_layer())?;
            Some(owned_policy::withdrawal_graph(
                &current,
                &ids,
                prepared.operation_id(),
            )?)
        } else {
            None
        };
        self.with_owned_transition(
            OwnedTransitionDecisionContext {
                prepared: prepared.clone(),
                next,
                current_scene: !alternative,
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
                    return Err(invalid("owned transition scene must execute completely"));
                }
                if store.load()? != current {
                    return Err(invalid(
                        "current changed during owned transition experience",
                    ));
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
    pub fn accept_owned_transition_current_scene(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedTransitionV2,
        scene: &ScenarioSpec,
        disclosure: Disclosure,
    ) -> Result<AcceptedScene> {
        self.capture_owned_transition(store, prepared, scene, disclosure, false)
    }
    pub fn accept_owned_transition_scene(
        &self,
        store: &ProductStore,
        prepared: &PreparedOwnedTransitionV2,
        scene: &ScenarioSpec,
        disclosure: Disclosure,
    ) -> Result<AcceptedScene> {
        self.capture_owned_transition(store, prepared, scene, disclosure, true)
    }
    pub fn prepare_owned_implementation_evolution(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedTransitionV2,
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        prepared.check_basis(&current)?;
        if prepared.operation_id() != id || prepared.is_withdrawal() {
            return Err(invalid("implementation evolution requires its exact operation and implementation preparation"));
        }
        let target = prepared.target().clone();
        self.with_owned_transition(
            OwnedTransitionDecisionContext {
                prepared,
                next: Some(current.decisions.clone()),
                current_scene: false,
            },
            || {
                self.prepare(
                    store,
                    &current,
                    &target,
                    current.decisions.clone(),
                    self.current_mappings(&current)?,
                    vec![],
                    id,
                )
            },
        )
    }
    pub fn preview_owned_withdrawal(
        &self,
        store: &ProductStore,
        layer: &Digest,
        id: &str,
        budget: ProofBudget,
        cancelled: Arc<AtomicBool>,
    ) -> Result<(
        PreparedOwnedTransitionV2,
        OwnedWithdrawalSummary,
        CheckReport,
    )> {
        let current = store.load()?;
        if self.archive.snapshot()? != current {
            return Err(invalid(
                "owned withdrawal archive differs from current store",
            ));
        }
        let healthy = self.check_current(&current)?;
        if healthy.disposition != CheckDisposition::Ready {
            return Err(DecisionError::Unverified(
                "owned withdrawal requires healthy current intentions".into(),
            ));
        }
        let mut retire = vec![];
        let prepared = crate::product_store::scope::prepare_owned_withdrawal(
            &current,
            layer,
            id,
            budget,
            cancelled,
            self.runtime.cancellation_token(),
            |check| {
                retire = owned_policy::check_withdrawal(&self.archive, check)
                    .map_err(into_store_error)?;
                Ok(())
            },
        )?;
        let next = owned_policy::withdrawal_graph(&current, &retire, id)?;
        let report = self.with_owned_transition(
            OwnedTransitionDecisionContext {
                prepared: prepared.clone(),
                next: Some(next.clone()),
                current_scene: false,
            },
            || self.check_bound(&next, prepared.target(), &self.current_mappings(&current)?),
        )?;
        if store.load()? != current {
            return Err(invalid("current changed during owned withdrawal preview"));
        }
        Ok((
            prepared,
            OwnedWithdrawalSummary {
                retire_decisions: retire,
            },
            report,
        ))
    }
    pub fn prepare_owned_withdrawal(
        &self,
        store: &ProductStore,
        prepared: PreparedOwnedTransitionV2,
        withdraw: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        prepared.check_basis(&current)?;
        if !prepared.is_withdrawal() || prepared.operation_id() != id {
            return Err(invalid(
                "withdrawal requires exact withdrawal preparation and operation",
            ));
        }
        let (checked, summary, _) = self.preview_owned_withdrawal(
            store,
            prepared.original_layer(),
            id,
            prepared.proof_budget(),
            prepared.cancellation(),
        )?;
        if checked != prepared || withdraw != summary.retire_decisions.as_slice() {
            return Err(invalid(
                "owned withdrawal differs from complete displayed retirement union",
            ));
        }
        let next = owned_policy::withdrawal_graph(&current, withdraw, id)?;
        let target = prepared.target().clone();
        self.with_owned_transition(
            OwnedTransitionDecisionContext {
                prepared,
                next: Some(next.clone()),
                current_scene: false,
            },
            || {
                self.prepare(
                    store,
                    &current,
                    &target,
                    next,
                    self.current_mappings(&current)?,
                    withdraw.to_vec(),
                    id,
                )
            },
        )
    }
    pub(super) fn adopt_owned_transition(
        &self,
        store: &ProductStore,
        change: &VerifiedChange,
        context: &OwnedTransitionDecisionContext,
        #[cfg(test)] fault: Option<crate::product_store::FaultPoint>,
    ) -> Result<ProjectSnapshot> {
        self.with_owned_transition(context.clone(), || {
            Ok(store.adopt_owned_transition_verified(
                change.revision,
                &context.prepared,
                &change.prepared,
                &change.decisions,
                self.runtime.cancellation_token(),
                |current, fresh, next, plan, candidate, check, archive_policy| {
                    #[cfg(test)]
                    crate::product_store::scope::adoption_diagnostic::phase(
                        crate::product_store::scope::adoption_diagnostic::Phase::CallbackBinding,
                    );
                    if canonical_digest(IdentityDomain::Data, current)? != change.expected_snapshot
                        || self.runtime.capabilities() != change.runtime
                        || self.limits != change.limits
                        || next != &change.decisions
                        || fresh.target() != &change.target
                        || plan != change.prepared.plan()
                    {
                        return Err(StoreError::Conflict(
                            "owned transition rehearsal changed".into(),
                        ));
                    }
                    let mut verify = || -> Result<()> {
                        #[cfg(test)]
                        crate::product_store::scope::adoption_diagnostic::phase(
                            crate::product_store::scope::adoption_diagnostic::Phase::WithdrawalGraph,
                        );
                        validate_withdrawal_history(current)?;
                        validate_withdrawal_delta(current, next, plan)?;
                        if fresh.is_withdrawal() {
                            #[cfg(test)]
                            crate::product_store::scope::adoption_diagnostic::phase(
                                crate::product_store::scope::adoption_diagnostic::Phase::Policy,
                            );
                            check.charge(6)?;
                            let ids = owned_policy::check_withdrawal(&self.archive, check)
                                .map_err(into_store_error)?;
                            if ids != plan.retire_decisions {
                                return Err(invalid("locked owned retirement union changed"));
                            }
                        } else if next != &current.decisions || !plan.retire_decisions.is_empty() {
                            return Err(invalid("implementation transition changed intentions"));
                        }
                        #[cfg(test)]
                        crate::product_store::scope::adoption_diagnostic::phase(
                            crate::product_store::scope::adoption_diagnostic::Phase::Packages,
                        );
                        self.verify_destination_packages(store, current, next, plan)?;
                        #[cfg(test)]
                        crate::product_store::scope::adoption_diagnostic::phase(
                            crate::product_store::scope::adoption_diagnostic::Phase::RemainingReplay,
                        );
                        let report = self.check_bound(next, fresh.target(), &change.mappings)?;
                        if report.disposition != CheckDisposition::Ready {
                            return Err(DecisionError::Unverified(
                                "owned transition fails independent remaining-intention replay"
                                    .into(),
                            ));
                        }
                        #[cfg(test)]
                        crate::product_store::scope::adoption_diagnostic::phase(
                            crate::product_store::scope::adoption_diagnostic::Phase::Bundle,
                        );
                        if let Some(capsule) = archive_policy {
                            check.charge(super::bundle::LOCKED_ARCHIVE_EXPORT_PAYMENT)?;
                            IntentArchive::new(store.clone())
                                .export_for_locked_withdrawal(candidate, capsule)?;
                        } else {
                            IntentArchive::new(store.clone()).export_for(candidate)?;
                        }
                        Ok(())
                    };
                    verify().map_err(into_store_error)
                },
                |current| {
                    IntentArchive::new(store.clone())
                        .export_for(current)
                        .map(|_| ())
                        .map_err(into_store_error)
                },
                #[cfg(test)]
                fault,
            )?)
        })
    }
    #[cfg(test)]
    pub(crate) fn adopt_owned_transition_with_fault(
        &self,
        store: &ProductStore,
        change: &VerifiedChange,
        fault: crate::product_store::FaultPoint,
    ) -> Result<ProjectSnapshot> {
        let ChangeAuthority::OwnedTransition(context) = &change.authority else {
            return Err(invalid(
                "transition fault helper requires verified transition",
            ));
        };
        self.adopt_owned_transition(store, change, context, Some(fault))
    }

    #[cfg(test)]
    pub(crate) fn assert_owned_retirement_callback_cancelled_for_test(
        &self,
        current: &ProjectSnapshot,
    ) {
        use std::sync::atomic::Ordering;
        let state = current.owned_scope().expect("genuine adopted fixture");
        assert_eq!(state.layers.len(), 1);
        let layer = state.layers.keys().next().unwrap();
        let primary = Arc::new(AtomicBool::new(false));
        let mut reached = false;
        let result = crate::product_store::scope::prepare_owned_withdrawal(
            current,
            layer,
            "eligibility-callback-cancel",
            ProofBudget::default(),
            primary.clone(),
            self.runtime.cancellation_token(),
            |check| {
                reached = true;
                primary.store(true, Ordering::Release);
                let refusal = owned_policy::check_withdrawal(&self.archive, check);
                assert!(matches!(&refusal, Err(DecisionError::Store(StoreError::Invalid(reason)))
                    if reason == "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
                refusal.map(|_| ()).map_err(into_store_error)
            },
        );
        assert!(reached, "the real eligibility callback must be reached before cancellation");
        assert!(primary.load(Ordering::Acquire));
        assert!(matches!(result, Err(StoreError::Invalid(ref reason))
            if reason == "Common factual admission: budget: cancelled or cumulative elapsed budget exhausted"));
    }
}
