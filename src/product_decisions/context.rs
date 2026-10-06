//! Portable context for a fresh development invocation. No prior chat is used.
use super::*;
impl<R: RuntimeAdapter> DecisionEngine<R> {
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
            let binding = validate_scene_bindings(decision, &scenes)?;
            binding_index.push(serde_json::json!({"decision":decision.id,"binding":binding}));
            for scene in scenes {
                relevant_scenes.push(scene.clone());
                let (run, _) = execute(
                    &self.runtime,
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
                let (actual, _) = execute(
                    &self.runtime,
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
