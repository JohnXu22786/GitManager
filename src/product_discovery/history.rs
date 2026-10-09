//! Host-only bridge from immutable V07 scene packages, not differential hashes.
use super::*;
use crate::product_decisions::{
    CheckDisposition, CheckReport, DecisionEngine, IntentArchive, IntentionBinding,
    VerifiedDiscoveryScene,
};
use crate::product_runtime::ReplayAdmission;
use crate::product_store::scope::{
    PreparedScopedChange, ProvenanceColumns, ScopedExecutionContext,
};
use crate::product_store::{ProductStore, ProjectSnapshot};

type ProjectionRegistry = Arc<std::sync::RwLock<BTreeMap<Digest, ScopedExecutionContext>>>;

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
    projected_contexts: ProjectionRegistry,
    /// Host-proposed mappings are validated and exercised by V07, never trusted
    /// as evidence. Each proposal is bound to one exact captured target.
    target_mappings: BTreeMap<Digest, Vec<SemanticMapping>>,
    prepared_targets: BTreeMap<Digest, PreparedScopedChange>,
    prepared_results: Vec<PreparedDiscoveryCandidate>,
}
/// Exact checked replay authority for one current-versus-prepared witness.
/// It is not serializable and cannot authorize adoption or change its sources,
/// population, starting frame or the preparation from which it was minted.
#[derive(Clone)]
pub struct CheckedWitnessReplay {
    basis: ProjectSnapshot,
    prepared: PreparedScopedChange,
    witness: VerifiedWitness,
    admission: Arc<dyn ReplayAdmission>,
}
impl CheckedWitnessReplay {
    pub fn witness(&self) -> &VerifiedWitness {
        &self.witness
    }
    pub fn check(
        &self,
        store: &ProductStore,
        prepared: &PreparedScopedChange,
        cancelled: &AtomicBool,
    ) -> Result<(), AdapterError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AdapterError::Cancelled);
        }
        if store.load().map_err(unavailable)? != self.basis || prepared != &self.prepared {
            return Err(AdapterError::Stale(
                "witness replay basis or exact preparation changed".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn admission(&self) -> Arc<dyn ReplayAdmission> {
        self.admission.clone()
    }
}

/// A replay handle can extend inputs in its authenticated starting frame, but
/// cannot borrow another source, day, seed, session or scope projection.
struct WitnessAdmission {
    sources: [CapturedProgram; 2],
    frames: [ScenarioSpec; 2],
    history: HistoryAdmission,
}
impl ReplayAdmission for WitnessAdmission {
    fn replay_operation_ids(
        &self,
        source: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<Id>, AdapterError> {
        if !self.sources.contains(source)
            || !self.frames.iter().any(|frame| {
                let mut frame = frame.clone();
                frame.inputs = scenario.inputs.clone();
                frame.label = scenario.label.clone();
                frame == *scenario
            })
        {
            return Err(unavailable(
                "witness replay changed its authenticated source or input frame",
            ));
        }
        self.history.replay_operation_ids(source, scenario)
    }
    fn validate_seed(
        &self,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<(), AdapterError> {
        if !self.sources.contains(source)
            || !self
                .frames
                .iter()
                .any(|s| &s.seed == data && s.clock_day == day)
        {
            return Err(unavailable(
                "witness replay changed its source, seed or day",
            ));
        }
        self.history.validate_seed(source, data, day)
    }
    fn validate_state(
        &self,
        source: &CapturedProgram,
        data: &DataSnapshot,
        day: i32,
    ) -> Result<(), AdapterError> {
        if !self.sources.contains(source) {
            return Err(unavailable("witness replay changed its source"));
        }
        self.history.validate_state(source, data, day)
    }
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
/// Only the exact host-regenerated source may identify compiler-added columns.
/// An ordinary source gets no exemption for names resembling system metadata.
pub(super) fn comparison_columns(
    policy: &DiscoveryPolicy,
    source: &CapturedProgram,
) -> Result<ProvenanceColumns, AdapterError> {
    source.validate()?;
    if let Some(history) = &policy.retained_history {
        let id = canonical_digest(IdentityDomain::Source, source)?;
        if let Some(prepared) = history.prepared_targets.get(&id) {
            return ScopedExecutionContext::prepared(&history.current, prepared)
                .and_then(|context| context.provenance_columns(source))
                .map_err(unavailable);
        }
        return history
            .replay_context
            .provenance_columns(source)
            .map_err(unavailable);
    }
    if crate::product_runtime::has_protected_fields(source) {
        return Err(unavailable(
            "comparison source has no verified compiler manifest",
        ));
    }
    Ok(ProvenanceColumns::default())
}

/// A temporary material-comparison projection, never a RunEvidence or a saved
/// artifact. The original runs and their complete byte receipts stay intact.
pub(super) fn comparison_observations(
    policy: &DiscoveryPolicy,
    source: &CapturedProgram,
    run: &RunEvidence,
) -> Result<Vec<Observation>, AdapterError> {
    if run.binding.source != source.binding || run.binding.artifact != source.artifact {
        return Err(unavailable(
            "comparison projection source differs from execution",
        ));
    }
    let columns = comparison_columns(policy, source)?;
    let mut observations = run.observations.clone();
    for observation in &mut observations {
        if let Some(schema) = &mut observation.view_schema {
            schema
                .columns
                .retain(|column, _| !columns.view(&observation.view.view, column));
        }
        for row in &mut observation.view.rows {
            row.cells
                .retain(|column, _| !columns.view(&observation.view.view, column));
        }
        for artifact in &mut observation.outputs {
            artifact.validate()?;
            if artifact
                .columns
                .iter()
                .any(|column| columns.output(&artifact.output, &column.id))
            {
                let fields = artifact
                    .columns
                    .iter()
                    .filter(|column| !columns.output(&artifact.output, &column.id))
                    .cloned()
                    .collect();
                let rows = artifact
                    .rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .filter(|(column, _)| !columns.output(&artifact.output, column))
                            .map(|(column, value)| (column.clone(), value.clone()))
                            .collect()
                    })
                    .collect();
                *artifact =
                    LocalArtifact::from_rows(&artifact.output, artifact.format, fields, rows)?;
            }
        }
    }
    Ok(observations)
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
            projected_contexts: Arc::new(std::sync::RwLock::new(BTreeMap::new())),
            target_mappings: BTreeMap::new(),
            prepared_targets: BTreeMap::new(),
            prepared_results: vec![],
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
    /// Register only a complete checked response-to-executable link, never a
    /// candidate label or provider-authored claim of host compilation.
    pub fn map_prepared_result(
        &mut self,
        candidate: PreparedDiscoveryCandidate,
    ) -> Result<(), AdapterError> {
        if self.store.load().map_err(unavailable)? != self.current {
            return Err(unavailable("prepared result basis changed"));
        }
        candidate.verify(&self.current, candidate.request(), candidate.result())?;
        if self.prepared_results.len() >= 8
            || self
                .prepared_results
                .iter()
                .any(|prior| prior.candidate_id() == candidate.candidate_id())
        {
            return Err(invalid(
                "prepared result registration is duplicate or exceeds response bounds",
            ));
        }
        self.map_prepared_target(
            candidate.preparation().clone(),
            candidate.mappings().to_vec(),
        )?;
        self.prepared_results.push(candidate);
        Ok(())
    }
    pub(super) fn result_lowerings(
        &self,
        request: &DevelopmentRequest,
        result: &DevelopmentResult,
    ) -> Result<Vec<PreparedDiscoveryCandidate>, AdapterError> {
        if !self.prepared_results.is_empty()
            && self.store.load().map_err(unavailable)? != self.current
        {
            return Err(AdapterError::Stale(
                "prepared discovery basis changed".into(),
            ));
        }
        for candidate in &self.prepared_results {
            candidate.verify(&self.current, request, result)?;
        }
        Ok(self.prepared_results.clone())
    }
    fn replay_contexts(&self) -> Result<Vec<ScopedExecutionContext>, AdapterError> {
        let mut contexts = vec![self.replay_context.clone()];
        for prepared in self.prepared_targets.values() {
            contexts.push(
                ScopedExecutionContext::prepared(&self.current, prepared)
                    .map_err(unavailable)?
                    .with_correspondences(&self.replay_context.correspondence_proofs())
                    .map_err(unavailable)?,
            );
            for scene in &self.mapped_scenes {
                // Populate the same bounded registry used by projection and
                // replay. A returned projected scene must not lose the proof
                // independently regenerated from its retained original input.
                let _ = self.project_comparison_scene(
                    self.current.program().map_err(unavailable)?,
                    prepared.target(),
                    scene.mapped(),
                );
            }
        }
        Ok(contexts)
    }
    pub(super) fn replay_admission(&self) -> Result<Arc<dyn ReplayAdmission>, AdapterError> {
        Ok(Arc::new(HistoryAdmission {
            contexts: self.replay_contexts()?,
            projected_contexts: self.projected_contexts.clone(),
        }))
    }
    /// Preserve the checked projection registry that actually produced this
    /// witness. A preparation alone loses synthetic creation identities after
    /// metadata initialization. Reproduce both original and reduced evidence
    /// before handing any authority to the host.
    pub fn checked_witness_replay(
        &self,
        prepared: &PreparedScopedChange,
        witness: &VerifiedWitness,
        cancelled: Arc<AtomicBool>,
    ) -> Result<CheckedWitnessReplay, AdapterError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AdapterError::Cancelled);
        }
        let current = self.current.program().map_err(unavailable)?;
        let id = canonical_digest(IdentityDomain::Source, prepared.target())?;
        if self.store.load().map_err(unavailable)? != self.current
            || self.prepared_targets.get(&id) != Some(prepared)
            || !(witness.matches_sources(current, prepared.target())
                || witness.matches_sources(prepared.target(), current))
        {
            return Err(unavailable(
                "witness is not the exact registered current/prepared rule pair",
            ));
        }
        let contexts = self.replay_contexts()?;
        // Freeze the existing registry. Later discovery on another clone must
        // not expand or change this handle's admitted replay frame.
        let projected = self
            .projected_contexts
            .read()
            .map_err(|_| unavailable("projected scene admission lock is poisoned"))?
            .clone();
        let admission: Arc<dyn ReplayAdmission> = Arc::new(WitnessAdmission {
            sources: [
                witness.before_program().clone(),
                witness.after_program().clone(),
            ],
            frames: [
                witness.initial_scenario().clone(),
                witness.witness().scenario.clone(),
            ],
            history: HistoryAdmission {
                contexts,
                projected_contexts: Arc::new(std::sync::RwLock::new(projected)),
            },
        });
        let runtime =
            LocalRuntime::with_cancellation(cancelled.clone()).with_admission(admission.clone());
        for (scenario, expected) in [
            (witness.initial_scenario(), witness.initial_runs()),
            (
                &witness.witness().scenario,
                (&witness.witness().before, &witness.witness().after),
            ),
        ] {
            for (source, expected) in [
                (witness.before_program(), expected.0),
                (witness.after_program(), expected.1),
            ] {
                if cancelled.load(Ordering::Acquire) {
                    return Err(AdapterError::Cancelled);
                }
                let actual = runtime.replay_admitted(
                    source,
                    scenario,
                    &self.current.decisions,
                    RuntimeLimits::default(),
                    "checked-witness-replay",
                    Some(admission.as_ref()),
                )?;
                if actual.state != EvidenceState::Observed
                    || actual.binding != expected.binding
                    || actual.observations != expected.observations
                    || actual.trace != expected.trace
                {
                    return Err(unavailable(
                        "witness replay did not reproduce its exact original evidence",
                    ));
                }
            }
        }
        let checked = CheckedWitnessReplay {
            basis: self.current.clone(),
            prepared: prepared.clone(),
            witness: witness.clone(),
            admission,
        };
        checked.check(&self.store, prepared, &cancelled)?;
        Ok(checked)
    }
    /// Host-authenticated preparation only: both compared executables receive
    /// this same actual scenario. Unknown correspondence remains unavailable.
    pub(super) fn project_comparison_scene(
        &self,
        before: &CapturedProgram,
        target: &CapturedProgram,
        scene: &ScenarioSpec,
    ) -> Result<ScenarioSpec, AdapterError> {
        let (scene, context) = self.projected_scene_context(before, target, scene)?;
        if let Some(context) = context {
            let key = scene.identity()?;
            let mut registry = self
                .projected_contexts
                .write()
                .map_err(|_| unavailable("projected scene admission lock is poisoned"))?;
            if let Some(existing) = registry.get(&key) {
                if existing.correspondence_proofs() != context.correspondence_proofs() {
                    return Err(unavailable(
                        "projected scene has ambiguous admission context",
                    ));
                }
            } else {
                if registry.len() >= MAX_ITEMS {
                    return Err(unavailable("projected scene admission exceeds bounds"));
                }
                registry.insert(key, context);
            }
        }
        Ok(scene)
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
        {
            let registry = self
                .projected_contexts
                .read()
                .map_err(|_| unavailable("projected scene admission lock is poisoned"))?;
            let mut matched: Option<&ScopedExecutionContext> = None;
            for context in registry.values().filter(|context| {
                context.has_scenario_correspondence(scene) && context.admit_target(target).is_ok()
            }) {
                // A prepared context also carries older retained scenario
                // correspondences. Matching one of those frames does not mean
                // its seed already contains this new target's initialization.
                // Reuse only an admitted common input; otherwise regenerate it
                // below through the same checked projection path.
                if context
                    .verify_seed(before, &scene.seed, scene.clock_day)
                    .is_err()
                    || context
                        .verify_seed(target, &scene.seed, scene.clock_day)
                        .is_err()
                {
                    continue;
                }
                if matched.is_some_and(|prior| {
                    prior.correspondence_proofs() != context.correspondence_proofs()
                }) {
                    return Err(unavailable(
                        "projected scene has ambiguous regenerated authority",
                    ));
                }
                matched = Some(context);
            }
            if let Some(context) = matched {
                return Ok((scene.clone(), Some(context.clone())));
            }
        }
        let context = ScopedExecutionContext::prepared(&self.current, prepared)
            .map_err(unavailable)?
            .with_correspondences(&self.replay_context.correspondence_proofs())
            .map_err(unavailable)?;
        // A new action may be absent from current. Authenticate its common
        // input independently of current's ability to execute that action;
        // discovery still requires the independent feature gate before A/B.
        if context
            .verify_seed(before, &scene.seed, scene.clock_day)
            .is_ok()
            && context
                .verify_seed(target, &scene.seed, scene.clock_day)
                .is_ok()
        {
            scene.validate(&target.program)?;
            return Ok((scene.clone(), Some(context)));
        }
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
    /// Input-frame equivalence only, never settlement or adoption authority.
    /// Callers must first reproduce and compare every material outcome. No
    /// business cell, schema, event, input or execution evidence is rewritten.
    pub(crate) fn equivalent_prepared_frames(
        &self,
        before: &CapturedProgram,
        after: &CapturedProgram,
        left: &ScenarioSpec,
        right: &ScenarioSpec,
        cancelled: &AtomicBool,
    ) -> Result<bool, AdapterError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(AdapterError::Cancelled);
        }
        if self.store.load().map_err(unavailable)? != self.current {
            return Err(unavailable("retained frame basis changed"));
        }
        let current = self.current.program().map_err(unavailable)?;
        let target = if before == current {
            after
        } else if after == current {
            before
        } else {
            return Ok(false);
        };
        let Some(prepared) = self
            .prepared_targets
            .get(&canonical_digest(IdentityDomain::Source, target)?)
        else {
            return Ok(false);
        };
        if prepared.initialization().is_none() {
            return Ok(false);
        }
        if left.version != right.version
            || left.session != right.session
            || left.clock_day != right.clock_day
            || left.random_seed != right.random_seed
            || left.validity != right.validity
            || left.inputs.len() != right.inputs.len()
            || !left.inputs.iter().zip(&right.inputs).all(|(a, b)| {
                a == b
                    || matches!(
                        (a, b),
                        (SemanticInput::Observe { .. }, SemanticInput::Observe { .. })
                    )
            })
        {
            return Ok(false);
        }
        let mut contexts = self.replay_contexts()?;
        contexts.extend(
            self.projected_contexts
                .read()
                .map_err(|_| unavailable("projected scene admission lock is poisoned"))?
                .values()
                .cloned(),
        );
        let admission = HistoryAdmission {
            contexts: contexts.clone(),
            projected_contexts: self.projected_contexts.clone(),
        };
        let mut namespaces = vec![];
        for source in [before, after] {
            for scene in [left, right] {
                // Both real sources must admit the exact frame, including its
                // original creation namespace, before origins can be compared.
                namespaces.push(admission.replay_operation_ids(source, scene)?);
            }
        }
        if namespaces.iter().any(|ids| ids != &namespaces[0]) {
            // Valid but different business inputs are not equivalent. An
            // unavailable or ambiguous namespace already failed admission.
            return Ok(false);
        }
        let mut proofs = BTreeMap::new();
        for context in &contexts {
            for (id, proof) in context.correspondence_proofs() {
                if proofs.get(&id).is_some_and(|prior| prior != &proof) {
                    return Err(unavailable("retained origin proof is ambiguous"));
                }
                proofs.insert(id, proof);
                if proofs.len() > MAX_ITEMS {
                    return Err(unavailable("retained origin inventory exceeds bounds"));
                }
            }
        }
        let mut sources: BTreeMap<_, _> = self
            .current
            .programs
            .iter()
            .chain(&self.context.sources)
            .map(|source| {
                Ok((
                    canonical_digest(IdentityDomain::Source, source)?,
                    source.clone(),
                ))
            })
            .collect::<Result<_, AdapterError>>()?;
        for prepared in self.prepared_targets.values() {
            for source in [prepared.candidate(), prepared.target()] {
                sources.insert(
                    canonical_digest(IdentityDomain::Source, source)?,
                    source.clone(),
                );
            }
        }
        let schema = &prepared.candidate().program.entities;
        let mut checked_schemas = BTreeSet::new();
        let mut origin = |scene: &ScenarioSpec| -> Result<DataSnapshot, AdapterError> {
            let mut pending = vec![(scene.clone(), None::<Digest>, BTreeSet::new())];
            let mut root: Option<DataSnapshot> = None;
            let mut visited = 0usize;
            while let Some((frame, expected, mut path)) = pending.pop() {
                if cancelled.load(Ordering::Acquire) {
                    return Err(AdapterError::Cancelled);
                }
                visited += 1;
                if visited > MAX_ITEMS || !path.insert(frame.identity()?) {
                    return Err(unavailable("retained origin is cyclic or exceeds bounds"));
                }
                let seed = frame.seed.identity()?;
                if expected.as_ref() == Some(&seed)
                    || (expected.is_none() && frame.seed == self.current.data)
                {
                    if root.as_ref().is_some_and(|prior| prior != &frame.seed) {
                        return Err(unavailable(
                            "retained frame has ambiguous original business input",
                        ));
                    }
                    root = Some(frame.seed);
                    continue;
                }
                let matching: Vec<_> = proofs
                    .values()
                    .filter(|proof| {
                        proof
                            .scenario
                            .as_ref()
                            .is_some_and(|pair| same_input_frame(&pair.projected, &frame))
                    })
                    .collect();
                if matching.is_empty() {
                    return Err(unavailable("retained frame lacks an exact checked origin"));
                }
                for proof in matching {
                    if expected
                        .as_ref()
                        .is_some_and(|id| id != &proof.operation_seed)
                        || proof.projected != seed
                    {
                        return Err(unavailable("retained origin namespace is ambiguous"));
                    }
                    let pair = proof.scenario.as_ref().expect("matched scenario proof");
                    // This route concerns compiler initialization only. A
                    // business-schema or semantic-input migration needs its
                    // own checked correspondence, not this equivalence gate.
                    if pair.original.session != pair.projected.session
                        || pair.original.inputs != pair.projected.inputs
                        || pair.original.clock_day != pair.projected.clock_day
                        || pair.original.random_seed != pair.projected.random_seed
                        || pair.original.validity != pair.projected.validity
                    {
                        return Err(unavailable(
                            "retained origin changes the execution environment or inputs",
                        ));
                    }
                    let target = sources
                        .get(&proof.target)
                        .ok_or_else(|| unavailable("retained origin target is missing"))?;
                    for source in [&proof.source, target] {
                        let id = canonical_digest(IdentityDomain::Source, source)?;
                        if checked_schemas.insert(id)
                            && self.checked_business_schema(source, &contexts)? != *schema
                        {
                            return Err(unavailable(
                                "retained origin changes ordinary business schema",
                            ));
                        }
                    }
                    let mut previous = pair.original.clone();
                    // A structural reduction keeps its actual inputs. Only the
                    // independently checked ancestor frame is followed back.
                    previous.inputs = frame.inputs.clone();
                    pending.push((previous, Some(proof.operation_seed.clone()), path.clone()));
                }
            }
            root.ok_or_else(|| unavailable("retained original business input is unavailable"))
        };
        Ok(origin(left)? == origin(right)?)
    }
    fn checked_business_schema(
        &self,
        source: &CapturedProgram,
        contexts: &[ScopedExecutionContext],
    ) -> Result<Vec<EntityDefinition>, AdapterError> {
        if !contexts
            .iter()
            .any(|context| context.provenance_columns(source).is_ok())
        {
            return Err(unavailable(
                "retained source has no checked compiler manifest",
            ));
        }
        let id = canonical_digest(IdentityDomain::Source, source)?;
        if let Some(prepared) = self.prepared_targets.get(&id) {
            return Ok(prepared.candidate().program.entities.clone());
        }
        let manifest = self.current.scope.compositions.get(&id).or_else(|| {
            self.current
                .scope
                .rehearsals
                .get(&id)
                .map(|proof| &proof.manifest)
        });
        if let Some(manifest) = manifest {
            let business = self
                .current
                .programs
                .iter()
                .chain(&self.context.sources)
                .find(|p| {
                    canonical_digest(IdentityDomain::Source, *p).ok().as_ref()
                        == Some(&manifest.business)
                })
                .ok_or_else(|| unavailable("retained ordinary schema source is missing"))?;
            return Ok(business.program.entities.clone());
        }
        if crate::product_runtime::has_protected_fields(source) {
            return Err(unavailable("retained schema lacks its ordinary source"));
        }
        Ok(source.program.entities.clone())
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
            if !request.examples.contains(original) {
                return Err(unavailable("exact historical typed input is missing"));
            }
            if !request.sources.contains(source) {
                // The transport contract deduplicates artifacts. A freshly
                // regenerated host capture may execute identical bytes under
                // a different preparation/producer identity. Verify that exact
                // link locally; retain and replay the original capture below.
                let represented = request.sources.iter().any(|captured| {
                    captured.artifact == source.artifact
                        && captured.source_bytes == source.source_bytes
                        && self.replay_context.contains_managed_source(source)
                        && canonical_digest(IdentityDomain::Source, captured)
                            .ok()
                            .and_then(|id| self.prepared_targets.get(&id))
                            .is_some_and(|prepared| {
                                prepared.target() == captured
                                    && ScopedExecutionContext::prepared(&self.current, prepared)
                                        .and_then(|context| context.admit_target(captured))
                                        .is_ok()
                            })
                });
                if !represented {
                    return Err(unavailable("exact historical capture or independently prepared byte correspondence is missing"));
                }
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
            let original_source = self
                .current
                .programs
                .iter()
                .chain(self.context.sources.iter())
                .find(|source| {
                    source.binding == side.original().binding.source
                        && source.artifact == side.original().binding.artifact
                })
                .ok_or_else(|| unavailable("exact retained comparison capture is unavailable"))?;
            let original = budget.replay(
                runtime,
                original_source,
                &checked_scene(side.scenario(), true, &[original_source], policy),
                &request.decisions,
                policy.search.runtime.clone(),
                if index == 0 {
                    "retained-package-first"
                } else {
                    "retained-package-second"
                },
            )?;
            if original.state != EvidenceState::Observed
                || original.observations != side.original().observations
            {
                return Err(unavailable(
                    "exact historical capture did not reproduce its accepted observations",
                ));
            }
            runs.push(original.clone());
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
            let comparison = comparison_observations(policy, original_source, &original)?;
            outcomes.push(RetainedRun {
                original,
                comparison,
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

fn same_input_frame(left: &ScenarioSpec, right: &ScenarioSpec) -> bool {
    left.version == right.version
        && left.id == right.id
        && left.seed == right.seed
        && left.session == right.session
        && left.clock_day == right.clock_day
        && left.random_seed == right.random_seed
        && left.validity == right.validity
}

/// Original execution evidence plus a separately executed target trace used
/// only for correspondence. Semantic names are projected during sampling; no
/// historical RunEvidence, output receipt or immutable identity is rewritten.
pub(super) struct RetainedRun {
    pub original: RunEvidence,
    pub comparison: Vec<Observation>,
    pub correspondence: RunEvidence,
    compared_input: Digest,
    pub mappings: Vec<SemanticMapping>,
}
impl RetainedRun {
    pub fn direct(run: RunEvidence) -> Self {
        Self {
            original: run.clone(),
            comparison: run.observations.clone(),
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
    projected_contexts: ProjectionRegistry,
}
impl ReplayAdmission for HistoryAdmission {
    fn replay_operation_ids(
        &self,
        source: &CapturedProgram,
        scenario: &ScenarioSpec,
    ) -> Result<Vec<Id>, AdapterError> {
        let projected = self
            .projected_contexts
            .read()
            .map_err(|_| unavailable("projected scene admission lock is poisoned"))?;
        let mut result = None;
        let explicit = self
            .contexts
            .iter()
            .chain(projected.values())
            .any(|context| {
                context.has_scenario_correspondence(scenario)
                    && context
                        .validate_seed(source, &scenario.seed, scenario.clock_day)
                        .is_ok()
            });
        for context in self.contexts.iter().chain(projected.values()) {
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
        let projected = self
            .projected_contexts
            .read()
            .map_err(|_| unavailable("projected scene admission lock is poisoned"))?;
        self.contexts
            .iter()
            .chain(projected.values())
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
        let projected = self
            .projected_contexts
            .read()
            .map_err(|_| unavailable("projected scene admission lock is poisoned"))?;
        self.contexts
            .iter()
            .chain(projected.values())
            .filter(|context| context.contains_managed_source(source))
            .find_map(|context| context.validate_state(source, data, day).ok())
            .ok_or_else(|| {
                unavailable("scoped source and data have no matching verified host context")
            })
    }
}
