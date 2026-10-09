//! Portable context for a fresh development invocation. No prior chat is used.
use super::*;
/// Independently executed, source-qualified mapping of one pending scene.
/// Original observations remain original evidence; the target replay establishes
/// input/point correspondence, not that this target preserves either preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedDiscoveryScene {
    decision: Id,
    package: Digest,
    original: RunEvidence,
    scenario: ScenarioSpec,
    mapped: ScenarioSpec,
    replay: RunEvidence,
    mappings: Vec<SemanticMapping>,
    operations: BTreeSet<Id>,
}
impl VerifiedDiscoveryScene {
    pub fn decision(&self) -> &str {
        &self.decision
    }
    pub fn package(&self) -> &Digest {
        &self.package
    }
    pub fn original(&self) -> &RunEvidence {
        &self.original
    }
    pub fn scenario(&self) -> &ScenarioSpec {
        &self.scenario
    }
    pub fn mapped(&self) -> &ScenarioSpec {
        &self.mapped
    }
    pub fn replay(&self) -> &RunEvidence {
        &self.replay
    }
    pub fn mappings(&self) -> &[SemanticMapping] {
        &self.mappings
    }
    pub fn operations(&self) -> &BTreeSet<Id> {
        &self.operations
    }
}
impl<R: RuntimeAdapter> DecisionEngine<R> {
    fn pending_scene_package(
        &self,
        current: &ProjectSnapshot,
        decision: &ScopedDecision,
    ) -> Result<Vec<AcceptedScene>> {
        if self.archive.snapshot()? != *current {
            return Err(DecisionError::Unverified(
                "Saved work changed before reading the pending comparison".into(),
            ));
        }
        if decision.status != DecisionStatus::Pending
            || !current.decisions.decisions.contains(decision)
        {
            return Err(invalid(
                "the exact pending decision is not in this saved tool",
            ));
        }
        self.archive.accepted_scenes(decision)
    }
    /// Reproduce the original scenes of one exact pending decision without
    /// claiming that Current implements their prospective actions or state.
    /// Returned originals are checked history, not fresh-copy adoption authority.
    pub fn pending_original_scenes(
        &self,
        current: &ProjectSnapshot,
        decision: &ScopedDecision,
    ) -> Result<Vec<AcceptedScene>> {
        current.validate()?;
        validate_withdrawal_history(current)?;
        // The shared package gate verifies the full pending decision, project,
        // source and ordered scene identities against the exact current store.
        let scenes = self.pending_scene_package(current, decision)?;
        for scene in &scenes {
            let (_, contexts) = self.reproduce_scene(scene, &current.decisions)?;
            if assess_scope(&decision.scope, &contexts) != ScopeMatch::Applies {
                return Err(DecisionError::Unverified(
                    "The original scene does not establish the pending decision's applicability"
                        .into(),
                ));
            }
        }
        if self.archive.snapshot()? != *current {
            return Err(DecisionError::Unverified(
                "Saved work changed while checking the pending comparison".into(),
            ));
        }
        Ok(scenes)
    }

    /// Project applicable pending history onto the current executable through
    /// its retained source-qualified mappings. Exact prospective-pair scenes
    /// whose new operations are absent from current keep their original replay
    /// source until a fresh target can execute them. No archive decoding escapes
    /// this boundary and no pending preference becomes an active obligation.
    pub fn discovery_scenes(
        &self,
        current: &ProjectSnapshot,
    ) -> Result<Vec<VerifiedDiscoveryScene>> {
        current.validate()?;
        validate_withdrawal_history(current)?;
        if self.archive.snapshot()? != *current {
            return Err(DecisionError::Unverified(
                "Saved work changed before checking pending history".into(),
            ));
        }
        let target = current.program()?;
        let pending: Vec<_> = current
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Pending)
            .collect();
        let mappings = self.current_mappings(current)?;
        let mut result = vec![];
        for decision in pending {
            let scenes = self.pending_scene_package(current, decision)?;
            // Ordinary pairs have no compiler rehearsal receipts. Preserve
            // only two distinct authenticated prospective sources on the same
            // complete input, with no managed source or Current-side claim.
            let ordinary_pair = current.editable_scope_context()?.is_none()
                && scenes.len() == 2
                && scenes[0].program() != scenes[1].program()
                && scenes[0].scenario() == scenes[1].scenario()
                && scenes.iter().all(|scene| {
                    scene.program() != target
                        && !crate::product_runtime::has_protected_fields(scene.program())
                });
            for scene in scenes {
                let (original, contexts) = self.reproduce_scene(&scene, &current.decisions)?;
                match assess_scope(&decision.scope, &contexts) {
                    ScopeMatch::Outside => continue,
                    ScopeMatch::Unknown => {
                        return Err(DecisionError::Unverified(
                            "Pending scene applicability is unknown".into(),
                        ))
                    }
                    ScopeMatch::Applies => {}
                }
                // A retained two-prospective design may introduce an action
                // absent from live current. Preserve its actual replay source;
                // it is not an observation of current or an equivalence claim.
                // Managed pairs retain their exact compiler proofs. Ordinary
                // pairs passed the bounded source/input checks above. Both
                // routes still independently replay originals and assess scope.
                let paired: Vec<_> = current
                    .scope
                    .rehearsals
                    .values()
                    .filter(|proof| proof.witnesses.get(&decision.id) == Some(&decision.witness))
                    .collect();
                let managed_pair = paired.len() == 2
                    && paired[0].recorded_by == paired[1].recorded_by
                    && paired[0].manifest.basis == paired[1].manifest.basis;
                if (managed_pair || ordinary_pair)
                    && scene.scenario.validate(&target.program).is_err()
                {
                    let operations = contexts
                        .iter()
                        .filter(|context| {
                            scope_match(&decision.scope, context).state == ScopeMatch::Applies
                        })
                        .map(|context| context.operation.clone())
                        .collect();
                    result.push(VerifiedDiscoveryScene {
                        decision: decision.id.clone(),
                        package: decision.witness.clone(),
                        original: original.clone(),
                        scenario: scene.scenario.clone(),
                        mapped: scene.scenario,
                        replay: original,
                        mappings: vec![],
                        operations,
                    });
                    continue;
                }
                let candidates: Vec<_> = mappings
                    .iter()
                    .filter(|m| {
                        m.source_program == scene.program.artifact.program_digest
                            && m.source_program != target.artifact.program_digest
                    })
                    .collect();
                if candidates.len() > 1 {
                    return Err(invalid("ambiguous pending source mapping"));
                }
                let bound = candidates.first().copied();
                if let Some(bound) = bound {
                    let mut originals = BTreeSet::new();
                    if bound.scenarios.iter().any(|replacement| {
                        !originals.insert(&replacement.original)
                            || replacement
                                .source_program
                                .as_ref()
                                .is_some_and(|source| source != &bound.source_program)
                    }) {
                        return Err(invalid("ambiguous or mismatched pending scenario mapping"));
                    }
                }
                let renames = bound.map(|m| m.mappings.clone()).unwrap_or_default();
                let mapping = Mapping::new(&scene.program.program, &target.program, &renames)?;

                let mapped = match bound.and_then(|m| {
                    m.scenarios
                        .iter()
                        .find(|s| s.original == scene.scenario.identity().expect("validated scene"))
                }) {
                    Some(replacement) => mapping.replacement(
                        &scene.scenario,
                        &target.program,
                        &replacement.replacement,
                    )?,
                    None => mapping.scenario(&scene.scenario, &target.program)?,
                };
                let (replay, target_contexts, mapped) = self.execute_mapped_scene(
                    &scene.program,
                    &scene.scenario,
                    target,
                    &mapped,
                    &current.decisions,
                    self.limits.clone(),
                    "pending-mapped-history",
                )?;
                if replay.state != EvidenceState::Observed {
                    return Err(DecisionError::Unverified(
                        "Mapped pending scene cannot execute completely".into(),
                    ));
                }
                let exercised = contexts
                    .iter()
                    .filter(|c| scope_match(&decision.scope, c).state == ScopeMatch::Applies)
                    .map(|c| c.operation.clone())
                    .collect();
                let operations = evolution::mapped_operations(
                    &exercised,
                    &scene.scenario,
                    &mapped,
                    &mapping,
                    &scene.program.program,
                    &target.program,
                )?;
                verify_target_scope(&decision.scope, &operations, &mapping, &target_contexts)?;
                result.push(VerifiedDiscoveryScene {
                    decision: decision.id.clone(),
                    package: decision.witness.clone(),
                    original,
                    scenario: scene.scenario,
                    mapped,
                    replay,
                    mappings: renames,
                    operations,
                });
            }
        }
        if self.archive.snapshot()? != *current {
            return Err(DecisionError::Unverified(
                "Saved work changed while checking pending history".into(),
            ));
        }
        Ok(result)
    }
    /// Read-only authoritative discovery gate. Compose the same retained,
    /// source-qualified mappings used by adoption, then independently verify
    /// concrete outcomes AND extra predicates before offering this executable.
    /// This creates no adoption authority and writes no archive objects.
    pub fn check_prepared_discovery_candidate(
        &mut self,
        current: &ProjectSnapshot,
        target: &CapturedProgram,
        prepared: &PreparedScopedChange,
        mappings: &[SemanticMapping],
        limits: RuntimeLimits,
    ) -> Result<CheckReport> {
        self.clear_admission_cache();
        if prepared.target() != target {
            return Err(invalid(
                "prepared discovery target differs from the exact captured candidate",
            ));
        }
        ScopedExecutionContext::prepared(current, prepared)?;
        let previous = self.pending_scope.replace(Some(prepared.clone()));
        let previous_correspondences = self
            .pending_correspondences
            .replace(prepared.correspondences.clone());
        let result = self.check_discovery_candidate(current, target, mappings, limits);
        self.pending_scope.replace(previous);
        self.pending_correspondences
            .replace(previous_correspondences);
        self.clear_admission_cache();
        result
    }
    pub fn check_discovery_candidate(
        &mut self,
        current: &ProjectSnapshot,
        target: &CapturedProgram,
        mappings: &[SemanticMapping],
        limits: RuntimeLimits,
    ) -> Result<CheckReport> {
        current.validate()?;
        limits.validate()?;
        validate_withdrawal_history(current)?;
        let previous = std::mem::replace(&mut self.limits, limits);
        let result = self
            .compose_for_current(current, target, mappings)
            .and_then(|mappings| self.check_bound(&current.decisions, target, &mappings));
        self.limits = previous;
        result
    }
    /// Build a current-program modification/reconciliation request. Discovery
    /// callers use `inherit_request` to preserve their exact baseline/candidate.
    pub fn development_request(
        &self,
        current: &ProjectSnapshot,
        id: &str,
        operation: DevelopmentOperation,
        request: &str,
        context: DevelopmentContext,
    ) -> Result<DevelopmentRequest> {
        if !matches!(
            operation,
            DevelopmentOperation::Modify | DevelopmentOperation::Reconcile
        ) {
            return Err(invalid("this builder needs a current-program modification; enrich discovery with its exact source pair"));
        }
        self.inherit_request(
            current,
            DevelopmentRequest {
                version: CONTRACT_VERSION,
                id: id.into(),
                project_id: current.data.project_id.clone(),
                operation,
                request: request.into(),
                sources: vec![current.program()?.clone()],
                context,
                examples: vec![],
                accepted_scenes: vec![],
                decisions: current.decisions.clone(),
                unknowns: vec![],
                required_capabilities: BTreeSet::new(),
            },
        )
    }
    /// Supply retained intentions to an already source-bound host request. This
    /// never reads chat history or truncates an accepted scene to fit a budget.
    pub fn inherit_request(
        &self,
        current: &ProjectSnapshot,
        mut request: DevelopmentRequest,
    ) -> Result<DevelopmentRequest> {
        current.validate()?;
        validate_withdrawal_history(current)?;
        if request.project_id != current.data.project_id
            || request
                .context
                .data_digest
                .as_ref()
                .is_some_and(|d| current.data.identity().ok().as_ref() != Some(d))
            || request
                .context
                .session_digest
                .as_ref()
                .is_some_and(|d| current.session.identity().ok().as_ref() != Some(d))
        {
            return Err(invalid(
                "development context is not bound to the current project state",
            ));
        }
        let primary =
            match request.operation {
                DevelopmentOperation::Discover => {
                    if request.sources.len() < 2 {
                        return Err(invalid("discovery needs its exact baseline and candidate"));
                    }
                    request.sources.first()
                }
                DevelopmentOperation::Modify | DevelopmentOperation::Reconcile => {
                    request.sources.last()
                }
                DevelopmentOperation::Generate => return Err(invalid(
                    "an existing project's intentions require a source-bound development request",
                )),
            };
        if primary != Some(current.program()?) {
            return Err(invalid(
                "development request lost the exact current source binding",
            ));
        }
        request.decisions = current.decisions.clone();
        request.accepted_scenes.clear();
        let mut accepted_keys = BTreeSet::new();
        let mut relevant_scenes = vec![];
        let mut binding_index = vec![];
        for decision in
            current.decisions.decisions.iter().filter(|d| {
                d.status == DecisionStatus::Active || d.status == DecisionStatus::Pending
            })
        {
            let scenes = self.archive.load(&decision.witness)?;
            if scenes
                .iter()
                .map(|scene| scene.scenario.identity())
                .collect::<std::result::Result<Vec<_>, _>>()?
                != decision.scenarios
            {
                return Err(invalid(
                    "accepted scene identities differ from the decision",
                ));
            }
            let binding = validate_scene_bindings(decision, &scenes)?;
            binding_index.push(serde_json::json!({"decision":decision.id,"binding":binding}));
            for scene in scenes {
                relevant_scenes.push(scene.clone());
                let (run, _) = self.execute_scene(
                    &scene.program,
                    &scene.scenario,
                    &current.decisions,
                    self.limits.clone(),
                    "portable-context",
                )?;
                if run.state != EvidenceState::Observed
                    || scene.observations() != run.observations.as_slice()
                {
                    return Err(DecisionError::Unverified("Accepted scene cannot be independently reproduced for the next development request".into()));
                }
                if accepted_keys.insert((
                    decision.id.clone(),
                    scene.program.artifact.program_digest.clone(),
                    scene.scenario.identity()?,
                )) {
                    request.accepted_scenes.push(AcceptedSceneContext {
                        decision: decision.id.clone(),
                        source: scene.program.artifact.clone(),
                        scenario: scene.scenario.identity()?,
                        observations: run.observations,
                        disclosure: scene.disclosure,
                    });
                }
                if !request
                    .sources
                    .iter()
                    .any(|p| p.artifact == scene.program.artifact)
                {
                    request.sources.push(scene.program.clone());
                }
                if let Some(prior) = request
                    .examples
                    .iter()
                    .find(|e| e.scenario.identity().ok() == scene.scenario.identity().ok())
                {
                    if prior.disclosure != scene.disclosure {
                        return Err(invalid(
                            "identical accepted input has conflicting disclosure origins",
                        ));
                    }
                } else {
                    request.examples.push(SelectedScenario {
                        disclosure: scene.disclosure,
                        scenario: scene.scenario,
                    });
                }
            }
            request.unknowns.extend(decision.scope.unknowns.clone());
            if decision.status == DecisionStatus::Pending
                && decision.outcome != DecisionOutcome::EitherAcceptable
            {
                request.unknowns.push(UnknownBoundary {
                    id: format!("pending-{}", decision.id),
                    operations: decision.scope.operations.clone(),
                    description: format!("Unresolved {:?}: {}", decision.outcome, decision.request),
                });
            }
        }
        if let Some(editable) = current.editable_scope_context()? {
            let editing = request.operation != DevelopmentOperation::Discover;
            // A whole-population composition can already be ordinary source.
            // Keep the artifact-deduplicated current capture and its real local
            // producer; the editing guide must name that supplied capture.
            let editable_source = if editable.editable.program == current.program()?.program
                && editable.editable.source_bytes == current.program()?.source_bytes
            {
                current.program()?
            } else {
                &editable.editable
            };
            if editing && !request.sources.iter().any(|p| p == editable_source) {
                request.sources.push(editable_source.clone());
            }
            // Discover's extra sources must remain accepted-scene artifacts.
            // It receives the slot index, while modification/reconciliation
            // also carries the honest ordinary capture as an actual source.
            let index = serde_json::json!({
                "compiled_source": editable.compiled_source,
                "editable_source": if editing { Some(canonical_digest(IdentityDomain::Source, editable_source)?) } else { None },
                "slots": editable.slots,
            });
            request.request.push_str(&format!(
                "\nHost-verified editing guide: slots use pre-instrumentation logical coordinates bound to compiled_source, not compiled JSON pointers. Edit ordinary rules; never author protected metadata. Map every retained slot to the exact new ordinary source, including inactive history slots. The host must prepare and recheck the complete result. {}",
                String::from_utf8(canonical_bytes(&index)?).map_err(|_| invalid("scope editing index encoding failed"))?
            ));
        }
        // Discover preserves its primary pair; other transports bind the last source.
        if request.operation != DevelopmentOperation::Discover {
            request.sources.retain(|p| {
                p.artifact
                    != current
                        .program()
                        .expect("validated current program")
                        .artifact
            });
            request.sources.push(current.program()?.clone());
        }
        request.unknowns.sort_by(|a, b| a.id.cmp(&b.id));
        request.unknowns.dedup_by(|a, b| a == b);
        let mut index = vec![];
        for bound in self.current_mappings(current)? {
            let Some(source) = relevant_scenes
                .iter()
                .find(|s| s.program.artifact.program_digest == bound.source_program)
            else {
                continue;
            };
            let mapping = Mapping::new(
                &source.program.program,
                &current.program()?.program,
                &bound.mappings,
            )?;
            let mut scenarios = vec![];
            for replacement in &bound.scenarios {
                let Some(original) = relevant_scenes.iter().find(|s| {
                    s.program.artifact.program_digest == bound.source_program
                        && s.scenario.identity().ok().as_ref() == Some(&replacement.original)
                }) else {
                    continue;
                };
                let mapped = mapping.replacement(
                    &original.scenario,
                    &current.program()?.program,
                    &replacement.replacement,
                )?;
                let (actual, _, mapped) = self.execute_mapped_scene(
                    &original.program,
                    &original.scenario,
                    current.program()?,
                    &mapped,
                    &current.decisions,
                    self.limits.clone(),
                    "portable-mapping",
                )?;
                if actual.state != EvidenceState::Observed {
                    return Err(DecisionError::Unverified(
                        "Current replay mapping cannot execute for the next development request"
                            .into(),
                    ));
                }
                let mapped_id = mapped.identity()?;
                if let Some(prior) = request
                    .examples
                    .iter()
                    .find(|e| e.scenario.identity().ok().as_ref() == Some(&mapped_id))
                {
                    if prior.disclosure != original.disclosure {
                        return Err(invalid("mapped input has conflicting disclosure origins"));
                    }
                } else {
                    request.examples.push(SelectedScenario {
                        disclosure: original.disclosure,
                        scenario: mapped,
                    });
                }
                scenarios.push(
                    serde_json::json!({"original":replacement.original,"replacement":mapped_id}),
                );
            }
            index.push(serde_json::json!({"source_program":bound.source_program,"target_program":current.program()?.artifact.program_digest,"mappings":bound.mappings,"scenarios":scenarios}));
        }
        if !binding_index.is_empty() {
            request.request.push_str(&format!("\nHost-verified intention bindings (observed_outcome preserves the concrete outcome AND all predicates; properties_only is an explicit independent promise): {}", String::from_utf8(canonical_bytes(&binding_index)?).map_err(|_| invalid("binding index encoding failed"))?));
        }
        if !index.is_empty() {
            request.request.push_str(&format!("\nRetained source-qualified replay mappings; replacement digests identify complete typed examples. Recheck all outcomes on your executable result: {}", String::from_utf8(canonical_bytes(&index)?).map_err(|_| invalid("mapping index encoding failed"))?));
            if request.request.len() > MAX_TEXT_BYTES {
                return Err(DecisionError::Unverified(
                    "Complete intention mapping index exceeds the development request limit".into(),
                ));
            }
        }
        if request.request.len() > MAX_TEXT_BYTES {
            return Err(DecisionError::Unverified(
                "Complete intention context exceeds the development request limit".into(),
            ));
        }
        request.validate()?;
        Ok(request)
    }
}
