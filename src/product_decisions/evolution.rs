use super::*;
/// Frozen host context. The provider cannot enlarge the requested needs or
/// decide which unrelated promises to retire.
#[derive(Clone)]
pub struct ReconciliationRequest {
    request: DevelopmentRequest,
    needs: Vec<Id>,
    current: Digest,
    mappings: Vec<ImplementationMapping>,
}
impl std::ops::Deref for ReconciliationRequest {
    type Target = DevelopmentRequest;
    fn deref(&self) -> &DevelopmentRequest {
        &self.request
    }
}
struct ResolvedNeed {
    prior: Id,
    scope: DecisionScope,
    obligations: Vec<AcceptedProperty>,
    scenes: Vec<AcceptedScene>,
}
pub struct EvolutionDraft {
    request: ReconciliationRequest,
    candidate: CapturedProgram,
    suggestion: EvolutionSuggestion,
    resolved: Vec<ResolvedNeed>,
    runtime: String,
    mappings: Vec<ImplementationMapping>,
}
impl EvolutionDraft {
    pub fn candidate(&self) -> &CapturedProgram {
        &self.candidate
    }
    pub fn suggestion(&self) -> &EvolutionSuggestion {
        &self.suggestion
    }
}
impl<R: RuntimeAdapter> DecisionEngine<R> {
    pub fn reconciliation_request(
        &self,
        current: &ProjectSnapshot,
        id: &str,
        request: &str,
        needs: &[Id],
    ) -> Result<ReconciliationRequest> {
        if needs.is_empty() || needs.iter().collect::<BTreeSet<_>>().len() != needs.len() {
            return Err(invalid("reconciliation needs exact distinct decision IDs"));
        }
        for need in needs {
            if !current.decisions.decisions.iter().any(|d| {
                &d.id == need
                    && (d.status == DecisionStatus::Active || d.status == DecisionStatus::Pending)
            }) {
                return Err(invalid("reconciliation names a missing or retired need"));
            }
        }
        let mut scene_count = 0usize;
        for need in needs {
            let d = current
                .decisions
                .decisions
                .iter()
                .find(|d| &d.id == need)
                .expect("validated need");
            scene_count += self.archive.load(&d.witness)?.len();
        }
        if scene_count < 2 {
            return Err(invalid(
                "reconciliation needs at least two actual accepted work scenes",
            ));
        }
        let request=self.development_request(current,id,DevelopmentOperation::Reconcile,&format!("{request}\nPreserve both accepted needs: {}. Return an executable design, explicit input mappings and exact proposed retirements. Preserve every independent obligation.",needs.join(", ")),DevelopmentContext{view:Some(current.session.view.clone()),selected:current.session.focused_record.clone().into_iter().collect(),recent_inputs:vec![],data_digest:Some(current.data.identity()?),session_digest:Some(current.session.identity()?)} )?;
        Ok(ReconciliationRequest {
            request,
            needs: needs.to_vec(),
            current: canonical_digest(IdentityDomain::Data, current)?,
            mappings: self.current_mappings(current)?,
        })
    }
    pub fn develop_evolution<P: DevelopmentProvider>(
        &self,
        provider: &P,
        request: &ReconciliationRequest,
        id: &str,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<EvolutionDraft> {
        if cancelled() {
            return Err(AdapterError::Cancelled.into());
        }
        let result = provider.develop(&request.request, cancelled)?;
        if cancelled() {
            return Err(AdapterError::Cancelled.into());
        }
        result.response.validate_for(&request.request)?;
        result.producer.validate()?;
        if let Producer::LiveAgent { request_digest, .. } = &result.producer {
            if request_digest != &request.identity()? {
                return Err(invalid("provider provenance belongs to another request"));
            }
        }
        let suggestion = result
            .response
            .evolutions
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| invalid("provider returned no requested executable evolution"))?
            .clone();
        if suggestion.needs.iter().collect::<BTreeSet<_>>()
            != request.needs.iter().collect::<BTreeSet<_>>()
            || suggestion
                .proposed_retirement
                .iter()
                .any(|id| !request.needs.contains(id))
        {
            return Err(invalid(
                "provider changed the requested needs or retired an independent intention",
            ));
        }
        let candidate = result
            .response
            .candidates
            .iter()
            .find(|c| c.id == suggestion.candidate)
            .ok_or_else(|| invalid("evolution executable is missing"))?;
        let candidate = CapturedProgram::capture(
            candidate.source_json.as_bytes(),
            &request.project_id,
            result.producer,
            None,
        )?;
        self.runtime.validate(&candidate)?;
        if let Some(current) = request.sources.last() {
            let mut executable = candidate.program.clone();
            // A new application identifier alone is not a new executable design.
            executable.id = current.program.id.clone();
            if executable.semantic_identity()? == current.artifact.semantic_digest {
                return Err(invalid("provider returned no executable design change; keep the current design explicitly instead"));
            }
        }
        let mut mappings = self.compose_sources(
            &request.decisions,
            request
                .sources
                .last()
                .ok_or_else(|| invalid("missing current source"))?,
            &candidate,
            &request.mappings,
            &suggestion.mappings,
            &request.needs,
        )?;
        let preserved: BTreeSet<_> = request
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .flat_map(|d| d.obligations.iter())
            .map(AcceptedProperty::identity)
            .collect::<std::result::Result<_, _>>()?;
        if preserved != suggestion.preserved_obligations.iter().cloned().collect() {
            return Err(invalid(
                "evolution must explicitly retain all active independent properties",
            ));
        }
        // Explicit replacements can preserve unrelated active intentions too.
        // They never expand the set of decisions the caller authorized retiring.
        let mut accepted = vec![];
        for decision in
            request.decisions.decisions.iter().filter(|d| {
                d.status == DecisionStatus::Active || d.status == DecisionStatus::Pending
            })
        {
            for scene in self.archive.load(&decision.witness)? {
                accepted.push((decision, scene));
            }
        }
        let mut supplied = BTreeSet::new();
        for proposed in &suggestion.scenarios {
            let mut decoded = proposed.decode()?;
            let matching: Vec<_> = accepted
                .iter()
                .filter(|(_, s)| s.scenario.identity().ok().as_ref() == Some(&decoded.original))
                .collect();
            let programs: BTreeSet<_> = matching
                .iter()
                .map(|(_, s)| s.program.artifact.program_digest.clone())
                .collect();
            let selected_program = match &decoded.source_program {
                Some(source) if programs.contains(source) => source.clone(),
                Some(_) => return Err(invalid("scenario replacement names the wrong accepted source program")),
                None if programs.len() == 1 => programs.iter().next().unwrap().clone(),
                None => return Err(DecisionError::Unverified("Scenario replacement needs an unambiguous accepted source-program discriminator".into())),
            };
            let (_, original) = matching
                .iter()
                .copied()
                .find(|(d, s)| {
                    s.program.artifact.program_digest == selected_program
                        && (d.status == DecisionStatus::Active || request.needs.contains(&d.id))
                })
                .ok_or_else(|| {
                    invalid("replacement is unrelated to an active intention or requested need")
                })?;
            decoded.source_program = Some(selected_program);
            if !supplied.insert((
                original.program.artifact.program_digest.clone(),
                decoded.original.clone(),
            )) {
                return Err(invalid("duplicate source/scenario replacement"));
            }
            let relevant = mappings
                .iter()
                .find(|m| m.source_program == original.program.artifact.program_digest)
                .map(|m| m.mappings.as_slice())
                .unwrap_or(&[]);
            let mapping = Mapping::new(&original.program.program, &candidate.program, relevant)?;
            self.reproduce_scene(original, &request.decisions)?;
            decoded.replacement = mapping.replacement(
                &original.scenario,
                &candidate.program,
                &decoded.replacement,
            )?;
            accept_scene(
                &self.runtime,
                &candidate,
                &decoded.replacement,
                original.disclosure,
                self.limits.clone(),
            )?;
            if !mappings
                .iter()
                .any(|m| m.source_program == original.program.artifact.program_digest)
            {
                mappings.push(ImplementationMapping {
                    source_program: original.program.artifact.program_digest.clone(),
                    mappings: vec![],
                    scenarios: vec![],
                });
            }
            let bound = mappings
                .iter_mut()
                .find(|m| m.source_program == original.program.artifact.program_digest)
                .unwrap();
            bound.scenarios.retain(|s| s.original != decoded.original);
            bound.scenarios.push(decoded);
        }
        let mut resolved = vec![];
        for need in &request.needs {
            let decision = request
                .decisions
                .decisions
                .iter()
                .find(|d| &d.id == need)
                .unwrap();
            let mut scenes = vec![];
            let mut scope = decision.scope.clone();
            scope.operations.clear();
            let mut obligations = vec![];
            let mut exercised = BTreeSet::new();
            for original in self.archive.load(&decision.witness)? {
                let (prior, contexts) = execute(
                    &self.runtime,
                    &original.program,
                    &original.scenario,
                    &request.decisions,
                    self.limits.clone(),
                    "recheck-accepted",
                )?;
                if prior.state != EvidenceState::Observed
                    || original.observations() != prior.observations.as_slice()
                {
                    return Err(DecisionError::Unverified(
                        "Accepted need no longer reproduces or its scope is unverified".into(),
                    ));
                }
                match assess_scope(&decision.scope, &contexts) {
                    ScopeMatch::Outside => continue,
                    ScopeMatch::Unknown => {
                        return Err(DecisionError::Unverified(
                            "Accepted need applicability remains unknown".into(),
                        ))
                    }
                    ScopeMatch::Applies => {}
                }
                let relevant = mappings
                    .iter()
                    .find(|m| m.source_program == original.program.artifact.program_digest)
                    .map(|m| m.mappings.clone())
                    .unwrap_or_default();
                let mapping =
                    Mapping::new(&original.program.program, &candidate.program, &relevant)?;
                let original_identity = original.scenario.identity()?;
                let replacement = if let Some(old) = mappings
                    .iter()
                    .find(|m| m.source_program == original.program.artifact.program_digest)
                    .and_then(|m| m.scenarios.iter().find(|s| s.original == original_identity))
                {
                    mapping.replacement(&original.scenario, &candidate.program, &old.replacement)?
                } else {
                    mapping.scenario(&original.scenario, &candidate.program)?
                };
                let (mut checked, target_contexts) = capture_scene(
                    &self.runtime,
                    &candidate,
                    &replacement,
                    original.disclosure,
                    self.limits.clone(),
                )?;
                checked.bind_outcome = original.bind_outcome;
                if (decision.obligations.is_empty() || original.bind_outcome)
                    && same_outcome(original.observations(), checked.observations(), &mapping)?
                        != Some(true)
                {
                    return Err(DecisionError::Unverified(
                        "New design does not reproduce both accepted outcomes".into(),
                    ));
                }
                for p in &decision.obligations {
                    if mapping.property(p).evaluate(checked.observations()) != Some(true) {
                        return Err(DecisionError::Unverified(
                            "New design violates or cannot check an accepted need".into(),
                        ));
                    }
                }
                let applicable: BTreeSet<_> = contexts
                    .iter()
                    .filter(|context| {
                        scope_match(&decision.scope, context).state == ScopeMatch::Applies
                    })
                    .map(|context| context.operation.clone())
                    .collect();
                exercised.extend(applicable.iter().cloned());
                let operations = mapped_operations(
                    &applicable,
                    &original.scenario,
                    &replacement,
                    &mapping,
                    &original.program.program,
                    &candidate.program,
                )?;
                verify_target_scope(&decision.scope, &operations, &mapping, &target_contexts)?;
                scope.operations.extend(operations);
                for p in &decision.obligations {
                    let mapped = mapping.property(p);
                    if !obligations.contains(&mapped) {
                        obligations.push(mapped);
                    }
                }
                scenes.push(checked);
            }
            if exercised != decision.scope.operations {
                return Err(DecisionError::Unverified(
                    "Accepted scenes do not collectively exercise every scoped operation".into(),
                ));
            }
            resolved.push(ResolvedNeed {
                prior: need.clone(),
                scope,
                obligations,
                scenes,
            });
        }
        if resolved.iter().map(|need| need.scenes.len()).sum::<usize>() < 2 {
            return Err(DecisionError::Unverified(
                "Reconciliation needs at least two applicable accepted scenes".into(),
            ));
        }
        // All independent intentions are checked too. Retired defaults are represented
        // by the re-executed, unchanged accepted needs above, never simply skipped.
        // Supersession references require the full graph. Suppress only exact need IDs
        // in a temporary replay selection, retaining every other active obligation.
        let mut remaining = request.decisions.clone();
        for d in &mut remaining.decisions {
            if request.needs.contains(&d.id) {
                d.status = DecisionStatus::Pending;
            }
        }
        let report = self.check_bound(&remaining, &candidate, &mappings)?;
        if report.disposition != CheckDisposition::Ready {
            return Err(DecisionError::Unverified(
                "New design fails an unrelated active intention".into(),
            ));
        }
        Ok(EvolutionDraft {
            request: request.clone(),
            candidate,
            suggestion,
            resolved,
            runtime: self.runtime.capabilities().version,
            mappings,
        })
    }
    pub fn prepare_evolution(
        &self,
        store: &ProductStore,
        draft: &EvolutionDraft,
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        if canonical_digest(IdentityDomain::Data, &current)? != draft.request.current
            || self.runtime.capabilities().version != draft.runtime
        {
            return Err(DecisionError::Unverified(
                "Evolution was prepared for older source, decisions or live data".into(),
            ));
        }
        let mut next = current.decisions.clone();
        let suggestion = &draft.suggestion;
        if next.decisions.iter().any(|d| d.id == suggestion.id) {
            return Err(invalid("evolution decision ID already exists"));
        }
        for (index, resolved) in draft.resolved.iter().enumerate() {
            let successor = if index == 0 {
                suggestion.id.clone()
            } else {
                format!("{}-{}", suggestion.id, resolved.prior)
            };
            if next.decisions.iter().any(|d| d.id == successor) {
                return Err(invalid("evolution successor already exists"));
            }
            let retired = suggestion.proposed_retirement.contains(&resolved.prior);
            if retired {
                let old = next
                    .decisions
                    .iter_mut()
                    .find(|d| d.id == resolved.prior)
                    .ok_or_else(|| invalid("retired need disappeared"))?;
                old.status = DecisionStatus::Superseded {
                    by: successor.clone(),
                };
                old.revision = old
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| invalid("decision revision overflow"))?;
            }
            next.decisions.push(ScopedDecision {
                id: successor,
                revision: 1,
                request: draft.request.request.request.clone(),
                rationale: None,
                scope: resolved.scope.clone(),
                outcome: DecisionOutcome::Accept {
                    artifact: draft.candidate.artifact.program_digest.clone(),
                },
                status: DecisionStatus::Active,
                obligations: resolved.obligations.clone(),
                scenarios: resolved
                    .scenes
                    .iter()
                    .map(|s| s.scenario.identity())
                    .collect::<std::result::Result<_, _>>()?,
                witness: self.archive.stage(&resolved.scenes)?,
                supersedes: if retired {
                    vec![resolved.prior.clone()]
                } else {
                    vec![]
                },
            });
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision revision overflow"))?;
        next.validate()?;
        self.prepare(
            store,
            &current,
            &draft.candidate,
            next,
            draft.mappings.clone(),
            suggestion.proposed_retirement.clone(),
            id,
        )
    }
}
fn observation_points(s: &ScenarioSpec) -> Vec<&str> {
    s.inputs
        .iter()
        .filter_map(|i| {
            if let SemanticInput::Observe { point } = i {
                Some(point.as_str())
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn mapped_operations(
    applicable: &BTreeSet<Id>,
    original: &ScenarioSpec,
    replacement: &ScenarioSpec,
    mapping: &Mapping,
    source: &AppDefinition,
    target: &AppDefinition,
) -> Result<BTreeSet<Id>> {
    let mut result = BTreeSet::new();
    // The caller supplies independently applicable operations for this scene.
    // An action that only ran outside its population is not a local obligation.
    for operation in applicable {
        if !original
            .inputs
            .iter()
            .any(|i| input_action(source, i).as_ref() == Some(operation))
        {
            continue;
        }
        let named = mapping.id(SemanticKind::Action, operation);
        // Stable action identity or an explicit checked rename is stronger than
        // array position, including when unrelated inputs are inserted.
        if replacement
            .inputs
            .iter()
            .any(|i| input_action(target, i).as_ref() == Some(&named))
        {
            result.insert(named);
            continue;
        }
        if original.inputs.len() != replacement.inputs.len() {
            return Err(DecisionError::Unverified(
                "Changed workflow needs an explicit action-scope mapping".into(),
            ));
        }
        let mut found = false;
        for (before, after) in original.inputs.iter().zip(&replacement.inputs) {
            if input_action(source, before).as_ref() == Some(operation) {
                let action = input_action(target, after).ok_or_else(|| {
                    DecisionError::Unverified("Accepted operation has no replacement action".into())
                })?;
                result.insert(action);
                found = true;
            } else if mapping.input(before) != *after {
                return Err(DecisionError::Unverified(
                    "Reordered or changed inputs do not establish action-scope correspondence"
                        .into(),
                ));
            }
        }
        if !found {
            return Err(DecisionError::Unverified(
                "Accepted scope operation is not exercised in the replacement".into(),
            ));
        }
    }
    Ok(result)
}
fn input_action(app: &AppDefinition, input: &SemanticInput) -> Option<Id> {
    match input {
        SemanticInput::Invoke { action, .. } => Some(action.clone()),
        SemanticInput::Activate { view, binding, .. } => app
            .views
            .iter()
            .find(|v| &v.id == view)?
            .actions
            .iter()
            .find(|a| &a.id == binding)
            .map(|a| a.action.clone()),
        SemanticInput::Submit { view, .. } => match &app.views.iter().find(|v| &v.id == view)?.kind
        {
            ViewKind::Form { action, .. } => Some(action.clone()),
            _ => None,
        },
        SemanticInput::Control { view, control, .. } => {
            match &app.views.iter().find(|v| &v.id == view)?.kind {
                ViewKind::List {
                    controls,
                    selection,
                    ..
                } => controls
                    .iter()
                    .find(|c| &c.id == control)
                    .and_then(|c| c.on_change.clone())
                    .or_else(|| {
                        selection
                            .as_ref()
                            .filter(|s| &s.id == control)
                            .and_then(|s| s.on_change.clone())
                    }),
                _ => None,
            }
        }
        _ => None,
    }
}
