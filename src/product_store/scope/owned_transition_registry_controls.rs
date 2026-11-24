//! Assertion-only frozen old preparation. No context constructor or authority output.
use super::*;

// This is the exact old prepare_owned_withdrawal + prepare_owned_evolution
// sequence, including original constructor, secondary watch, charge(0), old
// reconstructing transition and finish. Only its prepared output is discarded.
pub(super) fn old_preparation(
    current: &ProjectSnapshot,
    layer: &Digest,
    operation: &str,
    budget: ProofBudget,
    primary: Arc<AtomicBool>,
    secondary: Option<Arc<AtomicBool>>,
    check: impl FnOnce(&VerifiedOwnedTransitionExecution, &mut ProofMeter) -> Result<()>,
) -> Result<()> {
    let (before, after, sources) =
        owned_persist::effective_pair(current, &current.active_revision)?;
    let mut meter = ProofMeter::new(budget, primary.clone())?;
    if let Some(token) = &secondary {
        meter.watch_cancellation(token);
    }
    meter.charge(0)?;
    let execution = owned_persist::transition(
        current, before, after, sources, layer, operation, true, &mut meter,
    )?;
    check(&execution, &mut meter)?;
    let prepared = finish(execution, budget, primary, &mut meter)?;
    drop(prepared);
    Ok(())
}

impl ProjectSnapshot {
    pub(crate) fn run_old_registry_withdrawal_for_test(
        &self,
        layer: &Digest,
        operation: &str,
        check: impl FnOnce(&VerifiedOwnedTransitionExecution, &mut ProofMeter) -> Result<()>,
    ) -> Result<()> {
        old_preparation(
            self,
            layer,
            operation,
            ProofBudget::default(),
            Arc::new(AtomicBool::new(false)),
            None,
            check,
        )
    }
}
