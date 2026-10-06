//! Host-only bridge from immutable V07 scene packages, not differential hashes.
use super::*;
use crate::product_decisions::{
    CheckDisposition, CheckReport, DecisionEngine, IntentArchive, IntentionBinding,
};
use crate::product_store::{ProductStore, ProjectSnapshot};

/// Loaded only through V07's independently replayed projection. No provider or
/// deserialized public flags can construct this authority. The saved package
/// identities remain unchanged and reachable through the ordinary intent bundle.
#[derive(Clone)]
pub struct VerifiedRetainedHistory {
    store: ProductStore,
    current: ProjectSnapshot,
    context: DevelopmentRequest,
    bindings: BTreeMap<Id, IntentionBinding>,
    projection_replays: usize,
    /// Host-proposed mappings are validated and exercised by V07, never trusted
    /// as evidence. Each proposal is bound to one exact captured target.
    target_mappings: BTreeMap<Digest, Vec<SemanticMapping>>,
}
impl std::fmt::Debug for VerifiedRetainedHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedRetainedHistory")
            .field("revision", &self.current.revision)
            .field("decisions", &self.bindings.keys())
            .finish_non_exhaustive()
    }
}
fn unavailable(error: impl std::fmt::Display) -> AdapterError {
    AdapterError::Unsupported(format!("Retained intention history is unverified: {error}"))
}
impl VerifiedRetainedHistory {
    pub fn load(store: &ProductStore) -> Result<Self, AdapterError> {
        let current = store.load().map_err(unavailable)?;
        let engine =
            DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
        let context = Self::projection(&engine, &current)?;
        let bindings = Self::bindings(&engine, &current)?;
        let projection_replays = current
            .decisions
            .decisions
            .iter()
            .filter(|d| matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending))
            .map(|d| d.scenarios.len())
            .sum::<usize>()
            + engine
                .current_mappings(&current)
                .map_err(unavailable)?
                .iter()
                .map(|mapping| mapping.scenarios.len())
                .sum::<usize>();
        Ok(Self {
            store: store.clone(),
            current,
            context,
            bindings,
            projection_replays,
            target_mappings: BTreeMap::new(),
        })
    }
    /// A rename suggestion is not a passing check. Discovery independently
    /// composes it with retained source-qualified mappings and checks the target.
    pub fn map_target(
        &mut self,
        target: &CapturedProgram,
        mappings: Vec<SemanticMapping>,
    ) -> Result<(), AdapterError> {
        target.validate()?;
        if mappings.len() > MAX_ITEMS || target.binding.project_id != self.current.data.project_id {
            return Err(invalid(
                "target mappings exceed limits or name another project",
            ));
        }
        self.target_mappings
            .insert(canonical_digest(IdentityDomain::Source, target)?, mappings);
        Ok(())
    }
    fn projection(
        engine: &DecisionEngine<LocalRuntime>,
        current: &ProjectSnapshot,
    ) -> Result<DevelopmentRequest, AdapterError> {
        engine
            .development_request(
                current,
                "retained-history",
                DevelopmentOperation::Modify,
                "Verify retained intention history",
                DevelopmentContext {
                    view: None,
                    selected: vec![],
                    recent_inputs: vec![],
                    data_digest: Some(current.data.identity()?),
                    session_digest: Some(current.session.identity()?),
                },
            )
            .map_err(unavailable)
    }
    fn bindings(
        engine: &DecisionEngine<LocalRuntime>,
        current: &ProjectSnapshot,
    ) -> Result<BTreeMap<Id, IntentionBinding>, AdapterError> {
        current
            .decisions
            .decisions
            .iter()
            .filter(|d| matches!(d.status, DecisionStatus::Active | DecisionStatus::Pending))
            .map(|d| {
                engine
                    .intention_binding(d)
                    .map(|binding| (d.id.clone(), binding))
                    .map_err(unavailable)
            })
            .collect()
    }
    pub(super) fn verify(
        &self,
        request: &DevelopmentRequest,
        cancelled: Arc<AtomicBool>,
        budget: &ReplayBudget,
    ) -> Result<(), AdapterError> {
        if self.store.load().map_err(unavailable)? != self.current
            || request.decisions != self.current.decisions
            || request.sources.first() != Some(self.current.program().map_err(unavailable)?)
        {
            return Err(unavailable(
                "the request or stored revision differs from the captured history",
            ));
        }
        budget.reserve(self.projection_replays)?;
        let engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(cancelled),
            IntentArchive::new(self.store.clone()),
        );
        let fresh = Self::projection(&engine, &self.current)?;
        if fresh != self.context || Self::bindings(&engine, &self.current)? != self.bindings {
            return Err(unavailable("the independently reproduced history changed"));
        }
        if request.accepted_scenes != self.context.accepted_scenes {
            return Err(unavailable(
                "the request omitted or changed source-qualified accepted outcomes",
            ));
        }
        for accepted in &self.context.accepted_scenes {
            let original = self
                .context
                .examples
                .iter()
                .find(|e| e.scenario.identity().ok().as_ref() == Some(&accepted.scenario))
                .ok_or_else(|| unavailable("historical input is missing"))?;
            let source = self
                .context
                .sources
                .iter()
                .find(|p| p.artifact == accepted.source)
                .ok_or_else(|| unavailable("historical source is missing"))?;
            if !request.examples.contains(original) || !request.sources.contains(source) {
                return Err(unavailable(
                    "exact historical source or typed input is missing",
                ));
            }
        }
        Ok(())
    }
    pub(super) fn contains(&self, decision: &ScopedDecision) -> bool {
        self.current
            .decisions
            .decisions
            .iter()
            .any(|d| d == decision)
            && self.bindings.contains_key(&decision.id)
    }
    pub(super) fn check(
        &self,
        target: &CapturedProgram,
        cancelled: Arc<AtomicBool>,
        limits: RuntimeLimits,
        budget: &ReplayBudget,
    ) -> Result<CheckReport, AdapterError> {
        // Composition replays applicable originals, then V07 replays each old
        // and mapped new scene. Reserve all three before starting the check.
        let scenes: usize = self
            .current
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.scenarios.len())
            .sum();
        budget.reserve(scenes.saturating_mul(3))?;
        let mut engine = DecisionEngine::new(
            LocalRuntime::with_cancellation(cancelled),
            IntentArchive::new(self.store.clone()),
        );
        engine
            .check_discovery_candidate(
                &self.current,
                target,
                self.target_mappings
                    .get(&canonical_digest(IdentityDomain::Source, target)?)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                limits,
            )
            .map_err(unavailable)
    }
    pub(super) fn pair(
        &self,
        decision: &ScopedDecision,
        current_scene: &ScenarioSpec,
        request: &DevelopmentRequest,
        policy: &DiscoveryPolicy,
        runtime: &LocalRuntime,
        budget: &ReplayBudget,
        runs: &mut Vec<RunEvidence>,
    ) -> Result<(ScenarioSpec, RunEvidence, RunEvidence), AdapterError> {
        if !self.contains(decision) {
            return Err(unavailable("decision/package identity is not verified"));
        }
        let contexts: Vec<_> = self
            .context
            .accepted_scenes
            .iter()
            .filter(|s| s.decision == decision.id)
            .collect();
        let mut scenes = BTreeMap::new();
        for context in &contexts {
            let selected = self
                .context
                .examples
                .iter()
                .find(|e| e.scenario.identity().ok().as_ref() == Some(&context.scenario))
                .ok_or_else(|| unavailable("accepted input is missing"))?;
            scenes.insert(context.scenario.clone(), &selected.scenario);
        }
        let exact: Vec<_> = scenes
            .values()
            .filter(|s| s.input_identity().ok() == current_scene.input_identity().ok())
            .copied()
            .collect();
        let matching: Vec<_> = scenes
            .values()
            .filter(|s| history_execution_key(s).ok() == history_execution_key(current_scene).ok())
            .copied()
            .collect();
        let selected = if exact.len() == 1 {
            exact[0]
        } else if matching.len() == 1 {
            matching[0]
        } else if scenes.len() == 1 {
            scenes.values().next().copied().unwrap()
        } else {
            return Err(unavailable(
                "accepted history correspondence is ambiguous or missing",
            ));
        };
        let digest = selected.identity()?;
        let sides: Vec<_> = contexts
            .into_iter()
            .filter(|c| c.scenario == digest)
            .collect();
        if sides.len() != 2 || sides[0].source == sides[1].source {
            return Err(unavailable(
                "nonbinary history needs both real source-qualified outcomes",
            ));
        }
        let before = replay_accepted_outcome(
            request,
            policy,
            runtime,
            budget,
            decision,
            selected,
            &sides[0].source,
            "retained-package-first",
            runs,
        )?;
        let after = replay_accepted_outcome(
            request,
            policy,
            runtime,
            budget,
            decision,
            selected,
            &sides[1].source,
            "retained-package-second",
            runs,
        )?;
        Ok((selected.clone(), before, after))
    }
}

pub(super) fn append_check(
    report: &mut DiscoveryReport,
    checked: CheckReport,
    candidate: bool,
) -> CheckDisposition {
    let disposition = checked.disposition;
    for check in &checked.checks {
        match check.state {
            CheckState::Violated | CheckState::Failed if candidate => report.defects.push(format!(
                "Active decision {} requires repair: {}",
                check.decision, check.explanation
            )),
            CheckState::Satisfied
            | CheckState::Outside
            | CheckState::Violated
            | CheckState::Failed => {}
            _ => report.unverified.push(format!(
                "Decision {} is unverified: {}",
                check.decision, check.explanation
            )),
        }
    }
    report.checks.extend(checked.checks);
    report.runs.extend(checked.runs);
    disposition
}
