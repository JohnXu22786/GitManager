//! Host-only bridge from immutable V07 scene packages, not differential hashes.
use super::*;
use crate::product_decisions::{
    CheckDisposition, CheckReport, DecisionEngine, IntentArchive, IntentionBinding,
    VerifiedDiscoveryScene,
};
use crate::product_runtime::ReplayAdmission;
use crate::product_store::scope::{PreparedScopedChange, ScopedExecutionContext};
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
    mapped_scenes: Vec<VerifiedDiscoveryScene>,
    replay_context: ScopedExecutionContext,
    /// Host-proposed mappings are validated and exercised by V07, never trusted
    /// as evidence. Each proposal is bound to one exact captured target.
    target_mappings: BTreeMap<Digest, Vec<SemanticMapping>>,
    prepared_targets: BTreeMap<Digest, PreparedScopedChange>,
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
        let mapped_scenes = engine.discovery_scenes(&current).map_err(unavailable)?;
        let replay_context = engine
            .retained_replay_context(&current)
            .map_err(unavailable)?;
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
        let projection_replays = projection_replays
            + current
                .decisions
                .decisions
                .iter()
                .filter(|d| d.status == DecisionStatus::Pending)
                .map(|d| d.scenarios.len() * 2)
                .sum::<usize>();
        Ok(Self {
            store: store.clone(),
            current,
            context,
            bindings,
            projection_replays,
            mapped_scenes,
            replay_context,
            target_mappings: BTreeMap::new(),
            prepared_targets: BTreeMap::new(),
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
    pub fn map_prepared_target(
        &mut self,
        prepared: PreparedScopedChange,
        mappings: Vec<SemanticMapping>,
    ) -> Result<(), AdapterError> {
        if self.store.load().map_err(unavailable)? != self.current {
            return Err(unavailable("prepared-target basis changed"));
        }
        ScopedExecutionContext::prepared(&self.current, &prepared)
            .map_err(unavailable)?
            .admit_target(prepared.target())
            .map_err(unavailable)?;
        self.map_target(prepared.target(), mappings)?;
        self.prepared_targets.insert(
            canonical_digest(IdentityDomain::Source, prepared.target())?,
            prepared,
        );
        Ok(())
    }
    pub(super) fn replay_admission(&self) -> Result<Arc<dyn ReplayAdmission>, AdapterError> {
        let mut contexts = vec![self.replay_context.clone()];
        for prepared in self.prepared_targets.values() {
            contexts.push(
                ScopedExecutionContext::prepared(&self.current, prepared)
                    .map_err(unavailable)?
                    .with_correspondences(&self.replay_context.correspondence_proofs())
                    .map_err(unavailable)?,
            );
            for scene in &self.mapped_scenes {
                if let Ok((_, Some(context))) = self.projected_scene_context(
                    self.current.program().map_err(unavailable)?,
                    prepared.target(),
                    scene.mapped(),
                ) {
                    contexts.push(context);
                }
            }
        }
        Ok(Arc::new(HistoryAdmission { contexts }))
    }
    /// Host-authenticated preparation only: both compared executables receive
    /// this same actual scenario. Unknown correspondence remains unavailable.
    pub(super) fn project_comparison_scene(
        &self,
        before: &CapturedProgram,
        target: &CapturedProgram,
        scene: &ScenarioSpec,
    ) -> Result<ScenarioSpec, AdapterError> {
        Ok(self.projected_scene_context(before, target, scene)?.0)
    }
    fn projected_scene_context(
        &self,
        before: &CapturedProgram,
        target: &CapturedProgram,
        scene: &ScenarioSpec,
    ) -> Result<(ScenarioSpec, Option<ScopedExecutionContext>), AdapterError> {
        let target_id = canonical_digest(IdentityDomain::Source, target)?;
        let Some(prepared) = self.prepared_targets.get(&target_id) else {
            return Ok((scene.clone(), None));
        };
        if before != self.current.program().map_err(unavailable)? || prepared.target() != target {
            return Err(unavailable(
                "scene projection does not name the exact current/prepared pair",
            ));
        }
        let context = ScopedExecutionContext::prepared(&self.current, prepared)
            .map_err(unavailable)?
            .with_correspondences(&self.replay_context.correspondence_proofs())
            .map_err(unavailable)?;
        let mut mapped = scene.clone();
        mapped.seed = crate::product_runtime::merged_data(target, &scene.seed)?;
        let (context, actual) = context
            .project_scenario(before, scene, target, &mapped)
            .map_err(unavailable)?;
        context
            .verify_seed(before, &actual.seed, actual.clock_day)
            .map_err(unavailable)?;
        Ok((actual, Some(context)))
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
            || request
                .context
                .data_digest
                .as_ref()
                .is_some_and(|digest| self.current.data.identity().ok().as_ref() != Some(digest))
            || request
                .context
                .session_digest
                .as_ref()
                .is_some_and(|digest| self.current.session.identity().ok().as_ref() != Some(digest))
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
        if fresh != self.context
            || Self::bindings(&engine, &self.current)? != self.bindings
            || engine
                .discovery_scenes(&self.current)
                .map_err(unavailable)?
                != self.mapped_scenes
        {
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
        policy: &DiscoveryPolicy,
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
        let target_id = canonical_digest(IdentityDomain::Source, target)?;
        let mappings = self
            .target_mappings
            .get(&target_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut checked = if let Some(prepared) = self.prepared_targets.get(&target_id) {
            engine.check_prepared_discovery_candidate(
                &self.current,
                target,
                prepared,
                mappings,
                policy.search.runtime.clone(),
            )
        } else {
            engine.check_discovery_candidate(
                &self.current,
                target,
                mappings,
                policy.search.runtime.clone(),
            )
        }
        .map_err(unavailable)?;
        // These are the actual mapped target executions returned by V07.
        // Workflow validity is a pure post-execution predicate, so evaluate it
        // here without fabricating another run or weakening the saved binding.
        let mut workflow_checks = vec![];
        for run in &checked.runs {
            let evidence = run.identity()?;
            let Some(origin) = checked
                .checks
                .iter()
                .find(|check| check.evidence.as_ref() == Some(&evidence))
            else {
                continue;
            };
            let operations =
                input_operations(run.trace.iter().map(|step| &step.input), &target.program);
            let mut seen = BTreeSet::new();
            for property in operations
                .iter()
                .filter_map(|operation| policy.workflow_validity.get(operation))
                .flatten()
            {
                let property_digest = property.identity()?;
                if !seen.insert(property_digest.clone()) {
                    continue;
                }
                let state = if run.state != EvidenceState::Observed {
                    CheckState::Unknown
                } else {
                    match property.evaluate(&run.observations) {
                        Some(true) => continue,
                        Some(false) => CheckState::Failed,
                        None => CheckState::Unknown,
                    }
                };
                let mut check = origin.clone();
                check.property_digest = property_digest;
                check.state = state;
                check.explanation = format!("Host-approved workflow validity {} was not satisfied on the mapped accepted scene", property.id);
                workflow_checks.push(check);
                if state == CheckState::Failed {
                    checked.disposition = CheckDisposition::RepairRequired;
                } else if checked.disposition == CheckDisposition::Ready {
                    checked.disposition = CheckDisposition::Unverified;
                }
            }
        }
        checked.checks.extend(workflow_checks);
        Ok(checked)
    }
    pub(super) fn pending_scenes<'a>(
        &'a self,
        decision: &'a ScopedDecision,
    ) -> impl Iterator<Item = &'a VerifiedDiscoveryScene> {
        self.mapped_scenes
            .iter()
            .filter(move |s| s.decision() == decision.id && s.package() == &decision.witness)
    }
    pub(super) fn pending_operations(&self, decision: &ScopedDecision) -> BTreeSet<Id> {
        self.pending_scenes(decision)
            .flat_map(|s| s.operations().iter().cloned())
            .collect()
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
    ) -> Result<(ScenarioSpec, RetainedRun, RetainedRun), AdapterError> {
        if !self.contains(decision) {
            return Err(unavailable("decision/package identity is not verified"));
        }
        let contexts: Vec<_> = self.pending_scenes(decision).collect();
        // Labels/scene IDs are annotations, while accepted validity remains
        // binding. Distinct historical sources can name the same input scene.
        let key = |scene: &ScenarioSpec| -> Result<Digest, AdapterError> {
            Ok(canonical_digest(
                IdentityDomain::Scenario,
                &(scene_equivalence_key(scene)?, &scene.validity),
            )?)
        };
        let source = request
            .sources
            .first()
            .ok_or_else(|| unavailable("current source missing"))?;
        let target = request
            .sources
            .get(1)
            .ok_or_else(|| unavailable("candidate source missing"))?;
        let projected: Vec<_> = contexts
            .iter()
            .map(|context| self.project_comparison_scene(source, target, context.mapped()))
            .collect::<Result<_, _>>()?;
        let mut scenes = BTreeMap::new();
        for scene in &projected {
            scenes.insert(key(scene)?, scene);
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
        let digest = key(selected)?;
        let sides: Vec<_> = contexts
            .into_iter()
            .zip(projected.iter())
            .filter(|(_, scene)| key(scene).ok().as_ref() == Some(&digest))
            .collect();
        if sides.len() != 2
            || sides[0].0.original().binding.artifact == sides[1].0.original().binding.artifact
        {
            return Err(unavailable(
                "nonbinary history needs both real source-qualified outcomes",
            ));
        }
        let mut outcomes = vec![];
        for (index, (side, projected)) in sides.iter().enumerate() {
            let original = replay_accepted_outcome(
                request,
                policy,
                runtime,
                budget,
                decision,
                side.scenario(),
                &side.original().binding.artifact,
                if index == 0 {
                    "retained-package-first"
                } else {
                    "retained-package-second"
                },
                runs,
            )?;
            // Receipt initialization changes the actual compared seed, never
            // the historical accepted outcome. Re-execute correspondence on
            // that actual input instead of relabeling an earlier run binding.
            let correspondence = if *projected != side.mapped() {
                let run = budget.replay(
                    runtime,
                    source,
                    projected,
                    &request.decisions,
                    policy.search.runtime.clone(),
                    if index == 0 {
                        "retained-projected-first"
                    } else {
                        "retained-projected-second"
                    },
                )?;
                if run.state != EvidenceState::Observed {
                    return Err(unavailable(
                        "initialized historical correspondence did not execute",
                    ));
                }
                run
            } else {
                side.replay().clone()
            };
            runs.push(correspondence.clone());
            outcomes.push(RetainedRun {
                original,
                compared_input: selected.input_identity()?,
                correspondence,
                mappings: side.mappings().to_vec(),
            });
        }
        let after = outcomes.pop().unwrap();
        let before = outcomes.pop().unwrap();
        Ok((selected.clone(), before, after))
    }
}

/// Original execution evidence plus a separately executed target trace used
/// only for correspondence. Semantic names are projected during sampling; no
/// historical RunEvidence, output receipt or immutable identity is rewritten.
pub(super) struct RetainedRun {
    pub original: RunEvidence,
    pub correspondence: RunEvidence,
    compared_input: Digest,
    pub mappings: Vec<SemanticMapping>,
}
impl RetainedRun {
    pub fn direct(run: RunEvidence) -> Self {
        Self {
            original: run.clone(),
            compared_input: run.binding.input_digest.clone(),
            correspondence: run,
            mappings: vec![],
        }
    }
    pub fn forward(&self, kind: SemanticKind, id: &str) -> Id {
        self.mappings
            .iter()
            .find(|m| m.from.kind == kind && m.from.id == id)
            .map(|m| m.to.id.clone())
            .unwrap_or_else(|| id.into())
    }
    pub fn target(
        &self,
        target: &ObservationTarget,
        current: &RunEvidence,
    ) -> Option<ObservationTarget> {
        let mut old = retained_target(target, current, &self.correspondence)?;
        let reverse = |kind, id: &mut Id| {
            if let Some(mapping) = self
                .mappings
                .iter()
                .find(|m| m.to.kind == kind && m.to.id == *id)
            {
                *id = mapping.from.id.clone();
            }
        };
        match &mut old {
            ObservationTarget::Observable { observable, .. } => {
                reverse(SemanticKind::Observable, observable)
            }
            ObservationTarget::OutputCount { output, .. }
            | ObservationTarget::OutputColumn { output, .. } => {
                reverse(SemanticKind::Output, output)
            }
            _ => {}
        }
        Some(old)
    }
    pub fn material(&self, observation: &Observation) -> Option<BTreeMap<String, Digest>> {
        let mut profile = unrepresented_material(observation)?;
        profile.insert(
            "view".into(),
            canonical_digest(
                IdentityDomain::Observation,
                &self.forward(SemanticKind::View, &observation.view.view),
            )
            .ok()?,
        );
        for (index, artifact) in observation.outputs.iter().enumerate() {
            profile.insert(
                format!("output/{index}/id"),
                canonical_digest(
                    IdentityDomain::Observation,
                    &self.forward(SemanticKind::Output, &artifact.output),
                )
                .ok()?,
            );
        }
        Some(profile)
    }
    pub fn comparable(&self, run: &RunEvidence) -> bool {
        run.state == EvidenceState::Observed
            && self.original.state == EvidenceState::Observed
            && self.correspondence.state == EvidenceState::Observed
            && run.binding.runtime_version == self.original.binding.runtime_version
            && run.binding.driver_version == self.original.binding.driver_version
            && run.binding.input_digest == self.compared_input
            && run.observations.len() == self.original.observations.len()
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

struct HistoryAdmission {
    contexts: Vec<ScopedExecutionContext>,
}
impl ReplayAdmission for HistoryAdmission {
    fn replay_operation_ids(
        &self,
        source: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<Id>, AdapterError> {
        let mut result = None;
        let explicit = self.contexts.iter().any(|context| {
            context.has_scenario_correspondence(scenario)
                && context
                    .validate_seed(source, &scenario.seed, scenario.clock_day)
                    .is_ok()
        });
        for context in &self.contexts {
            if explicit && !context.has_scenario_correspondence(scenario) {
                continue;
            }
            if context
                .validate_seed(source, &scenario.seed, scenario.clock_day)
                .is_err()
            {
                continue;
            }
            let ids = context.replay_operation_ids(source, scenario)?;
            if result.as_ref().is_some_and(|prior| prior != &ids) {
                return Err(unavailable(
                    "ambiguous historical replay operation identity",
                ));
            }
            result = Some(ids);
        }
        if let Some(ids) = result {
            return Ok(ids);
        }
        if crate::product_runtime::has_protected_fields(source) {
            return Err(unavailable(
                "scoped replay operation identity is unverified",
            ));
        }
        Ok(scenario.replay_operation_ids()?)
    }

    fn validate_seed(
        &self,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<(), AdapterError> {
        if !crate::product_runtime::has_protected_fields(source) {
            return Ok(());
        }
        // Several independently verified contexts can contain the same
        // baseline source. Admission is the exact source-and-data pair, not
        // the first source match (a newer preparation may initialize a seed).
        self.contexts
            .iter()
            .filter(|context| context.contains_managed_source(source))
            .find_map(|context| context.validate_seed(source, data, day).ok())
            .ok_or_else(|| {
                unavailable("scoped source and data have no matching verified host context")
            })
    }
    fn validate_state(
        &self,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<(), AdapterError> {
        if !crate::product_runtime::has_protected_fields(source) {
            return Ok(());
        }
        // Several independently verified contexts can contain the same
        // baseline source. Admission is the exact source-and-data pair, not
        // the first source match (a newer preparation may initialize a seed).
        self.contexts
            .iter()
            .filter(|context| context.contains_managed_source(source))
            .find_map(|context| context.validate_state(source, data, day).ok())
            .ok_or_else(|| {
                unavailable("scoped source and data have no matching verified host context")
            })
    }
}
