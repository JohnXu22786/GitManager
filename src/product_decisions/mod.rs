//! Host-owned intention verification. Provider suggestions and archived proof
//! flags never authorize adoption; every active obligation is replayed afresh.
mod archive;
mod bundle;
mod context;
mod evolution;
mod mapping;
mod recovery;
mod replay;
use crate::product_contract::*;
use crate::product_runtime::LocalRuntime;
use crate::product_store::{PreparedAdoption, ProductStore, ProjectSnapshot, StoreError};
pub use archive::IntentArchive;
pub use bundle::{validate_bundle, IntentionBundle};
pub use evolution::{EvolutionDraft, ReconciliationRequest};
pub use mapping::ImplementationMapping;
use mapping::Mapping;
use recovery::{validate_withdrawal_delta, validate_withdrawal_history};
use replay::*;
pub use replay::{accept_scene, scope_match, AcceptedScene, ScopeAssessment};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
type Result<T> = std::result::Result<T, DecisionError>;
#[derive(Debug)]
pub enum DecisionError {
    Invalid(String),
    Unverified(String),
    Archive(String),
    Runtime(AdapterError),
    Store(StoreError),
}
impl fmt::Display for DecisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for DecisionError {}
impl From<ContractError> for DecisionError {
    fn from(e: ContractError) -> Self {
        Self::Invalid(e.to_string())
    }
}
impl From<AdapterError> for DecisionError {
    fn from(e: AdapterError) -> Self {
        Self::Runtime(e)
    }
}
impl From<StoreError> for DecisionError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
fn invalid(s: &str) -> DecisionError {
    DecisionError::Invalid(s.into())
}
/// A concrete choice stays concrete even when additional predicates are supplied.
/// PropertiesOnly is a separate, explicit host/user choice, never inferred by a model.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentionBinding {
    #[default]
    ObservedOutcome,
    PropertiesOnly,
}
#[derive(Clone, Debug)]
pub struct Choice {
    pub id: Id,
    pub request: String,
    pub rationale: Option<String>,
    pub scope: DecisionScope,
    pub outcome: DecisionOutcome,
    pub obligations: Vec<AcceptedProperty>,
    pub binding: IntentionBinding,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckDisposition {
    Ready,
    RepairRequired,
    Unverified,
}
#[derive(Clone, Debug)]
pub struct CheckReport {
    pub checks: Vec<DecisionCheck>,
    pub runs: Vec<RunEvidence>,
    pub disposition: CheckDisposition,
    pub business_questions: usize,
}
impl CheckReport {
    fn empty() -> Self {
        Self {
            checks: vec![],
            runs: vec![],
            disposition: CheckDisposition::Ready,
            business_questions: 0,
        }
    }
    fn add(&mut self, check: DecisionCheck) {
        if matches!(check.state, CheckState::Violated | CheckState::Failed) {
            self.disposition = CheckDisposition::RepairRequired;
        } else if !matches!(check.state, CheckState::Satisfied | CheckState::Outside)
            && self.disposition != CheckDisposition::RepairRequired
        {
            self.disposition = CheckDisposition::Unverified;
        }
        self.checks.push(check);
    }
}
/// Kept opaque: only this host can prepare it, and commit repeats the checks.
pub struct VerifiedChange {
    prepared: PreparedAdoption,
    revision: u64,
    target: CapturedProgram,
    decisions: DecisionGraph,
    mappings: Vec<ImplementationMapping>,
    report: CheckReport,
    expected_snapshot: Digest,
    runtime: String,
}
impl VerifiedChange {
    pub fn report(&self) -> &CheckReport {
        &self.report
    }
    pub fn plan(&self) -> &AdoptionPlan {
        self.prepared.plan()
    }
    pub fn candidate(&self) -> &CapturedProgram {
        &self.target
    }
}
pub struct DecisionEngine<R: RuntimeAdapter> {
    runtime: R,
    archive: IntentArchive,
    limits: RuntimeLimits,
}
impl<R: RuntimeAdapter> DecisionEngine<R> {
    pub fn new(runtime: R, archive: IntentArchive) -> Self {
        Self {
            runtime,
            archive,
            limits: RuntimeLimits::default(),
        }
    }
    /// Read the recorded promise kind for presentation; this is not execution proof.
    pub fn intention_binding(&self, decision: &ScopedDecision) -> Result<IntentionBinding> {
        decision.validate()?;
        validate_scene_bindings(decision, &self.archive.load(&decision.witness)?)
    }
    pub fn current_mappings(
        &self,
        current: &ProjectSnapshot,
    ) -> Result<Vec<ImplementationMapping>> {
        self.mappings_for(current, current.program()?)
    }
    fn mappings_for(
        &self,
        current: &ProjectSnapshot,
        target: &CapturedProgram,
    ) -> Result<Vec<ImplementationMapping>> {
        match current
            .adoptions
            .iter()
            .rev()
            .find(|a| a.plan.target == target.artifact)
            .and_then(|a| a.plan.evidence.first())
        {
            Some(digest) => self.archive.load_mappings(digest, &target.artifact),
            None => Ok(vec![]),
        }
    }
    pub fn check_current(&self, current: &ProjectSnapshot) -> Result<CheckReport> {
        validate_withdrawal_history(current)?;
        self.check_bound(
            &current.decisions,
            current.program()?,
            &self.current_mappings(current)?,
        )
    }
    pub fn check_all(
        &self,
        graph: &DecisionGraph,
        target: &CapturedProgram,
        mappings: &[SemanticMapping],
    ) -> Result<CheckReport> {
        let mappings = self.bind_mappings(graph, target, mappings)?;
        self.check_bound(graph, target, &mappings)
    }
    fn bind_mappings(
        &self,
        graph: &DecisionGraph,
        target: &CapturedProgram,
        proposed: &[SemanticMapping],
    ) -> Result<Vec<ImplementationMapping>> {
        let sources = self.accepted_sources(graph)?;
        let mut seen = BTreeSet::new();
        for m in proposed {
            if m.from.kind != m.to.kind
                || !m.to.exists_in(&target.program)
                || !seen.insert(canonical_bytes(&m.from)?)
                || !sources.iter().any(|p| m.from.exists_in(&p.program))
            {
                return Err(invalid(
                    "mapping is ambiguous or not bound to accepted source and target semantics",
                ));
            }
        }
        Ok(sources
            .iter()
            .filter(|p| p.artifact.program_digest != target.artifact.program_digest)
            .filter_map(|p| {
                let mappings: Vec<_> = proposed
                    .iter()
                    .filter(|m| m.from.exists_in(&p.program))
                    .cloned()
                    .collect();
                (!mappings.is_empty()).then(|| ImplementationMapping {
                    source_program: p.artifact.program_digest.clone(),
                    mappings,
                    scenarios: vec![],
                })
            })
            .collect())
    }
    fn accepted_sources(&self, graph: &DecisionGraph) -> Result<Vec<CapturedProgram>> {
        let mut sources = vec![];
        for d in &graph.decisions {
            let scenes = self.archive.load(&d.witness)?;
            validate_scene_bindings(d, &scenes)?;
            for scene in scenes {
                if !sources.iter().any(|p: &CapturedProgram| {
                    p.artifact.program_digest == scene.program.artifact.program_digest
                }) {
                    sources.push(scene.program);
                }
            }
        }
        Ok(sources)
    }
    fn check_bound(
        &self,
        graph: &DecisionGraph,
        target: &CapturedProgram,
        mappings: &[ImplementationMapping],
    ) -> Result<CheckReport> {
        graph.validate()?;
        target.validate()?;
        self.runtime.validate(target)?;
        let sources = self.accepted_sources(graph)?;
        let mut seen = BTreeSet::new();
        for bound in mappings {
            if !seen.insert(&bound.source_program) {
                return Err(invalid("ambiguous accepted source mapping"));
            }
            let source = sources
                .iter()
                .find(|p| p.artifact.program_digest == bound.source_program)
                .ok_or_else(|| invalid("mapping has no accepted source program"))?;
            Mapping::new(&source.program, &target.program, &bound.mappings)?;
            let mut seen_scenes = BTreeSet::new();
            for replacement in &bound.scenarios {
                if replacement
                    .source_program
                    .as_ref()
                    .is_some_and(|p| p != &bound.source_program)
                {
                    return Err(invalid(
                        "stored scenario mapping disagrees with its accepted source program",
                    ));
                }
                if !seen_scenes.insert(&replacement.original) {
                    return Err(invalid("ambiguous scenario replacement"));
                }
                let mut original = None;
                for d in &graph.decisions {
                    for scene in self.archive.load(&d.witness)? {
                        if scene.program.artifact.program_digest == bound.source_program
                            && scene.scenario.identity()? == replacement.original
                        {
                            original = Some(scene.scenario);
                        }
                    }
                }
                original.ok_or_else(|| invalid("replacement has no accepted source scenario"))?;
                // Target inputs are checked when the scoped obligation applies.
                // An obsolete bridge cannot impose requirements on retired/outside scenes.
            }
        }
        let mut report = CheckReport::empty();
        for decision in graph
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
        {
            let scenes = self.archive.load(&decision.witness)?;
            if scenes
                .iter()
                .map(|s| s.scenario.identity())
                .collect::<std::result::Result<Vec<_>, _>>()?
                != decision.scenarios
            {
                return Err(invalid(
                    "accepted scene identities differ from the decision",
                ));
            }
            let start = report.checks.len();
            let mut exercised = BTreeSet::new();
            for (index, scene) in scenes.iter().enumerate() {
                let relevant = mappings.iter().find(|m| {
                    m.source_program == scene.program.artifact.program_digest
                        && m.source_program != target.artifact.program_digest
                });
                exercised.extend(self.check_scene(
                    graph,
                    decision,
                    scene,
                    target,
                    relevant,
                    index,
                    &mut report,
                )?);
            }
            let missing: Vec<_> = decision
                .scope
                .operations
                .difference(&exercised)
                .cloned()
                .collect();
            if !missing.is_empty() {
                let mut uncovered = report.checks[start].clone();
                uncovered.scope = ScopeMatch::Unknown;
                uncovered.state = CheckState::Unknown;
                uncovered.evidence = None;
                uncovered.property_digest = canonical_digest(
                    IdentityDomain::Decision,
                    &(&decision.scope, "operation-coverage"),
                )?;
                uncovered.explanation = format!(
                    "No applicable independently replayed accepted scene covers operation(s): {}",
                    missing.join(", ")
                );
                report.add(uncovered);
            }
        }
        Ok(report)
    }
    fn check_scene(
        &self,
        graph: &DecisionGraph,
        decision: &ScopedDecision,
        scene: &AcceptedScene,
        target: &CapturedProgram,
        bound: Option<&ImplementationMapping>,
        index: usize,
        report: &mut CheckReport,
    ) -> Result<BTreeSet<Id>> {
        let relevant: Vec<_> = bound
            .map(|m| m.mappings.as_slice())
            .unwrap_or(&[])
            .iter()
            .filter(|m| m.from.exists_in(&scene.program.program))
            .cloned()
            .collect();
        let mapping = Mapping::new(&scene.program.program, &target.program, &relevant)?;
        let (prior, contexts) = execute(
            &self.runtime,
            &scene.program,
            &scene.scenario,
            graph,
            self.limits.clone(),
            &format!(
                "old-{}",
                &canonical_digest(IdentityDomain::Decision, &(&decision.id, index))?.as_str()[..32]
            ),
        )?;
        let scope = assess_scope(&decision.scope, &contexts);
        let mut check = DecisionCheck {
            decision: decision.id.clone(),
            decision_digest: decision.identity()?,
            property_digest: canonical_digest(
                IdentityDomain::Decision,
                &(&decision.obligations, scene.observations()),
            )?,
            scope,
            state: CheckState::Unknown,
            binding: RunBinding {
                source: target.binding.clone(),
                artifact: target.artifact.clone(),
                runtime_version: self.runtime.capabilities().version,
                ..prior.binding.clone()
            },
            evidence: None,
            explanation: String::new(),
        };
        if prior.state != EvidenceState::Observed
            || scene.observations() != prior.observations.as_slice()
        {
            check.state = CheckState::Stale;
            check.explanation="Accepted scene no longer reproduces in the current runtime; review its execution, not the settled preference".into();
            report.add(check);
            return Ok(BTreeSet::new());
        }
        let exercised = contexts
            .iter()
            .filter(|c| scope_match(&decision.scope, c).state == ScopeMatch::Applies)
            .map(|c| c.operation.clone())
            .collect();
        if scope != ScopeMatch::Applies {
            check.state = if scope == ScopeMatch::Outside {
                CheckState::Outside
            } else {
                CheckState::Unknown
            };
            check.explanation = if scope == ScopeMatch::Outside {
                "Accepted scene is outside this explicit scope".into()
            } else {
                contexts
                    .iter()
                    .map(|c| scope_match(&decision.scope, c))
                    .filter(|a| a.state == ScopeMatch::Unknown)
                    .map(|a| a.reason)
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            report.add(check);
            return Ok(exercised);
        }
        let explicit = bound.and_then(|b| {
            b.scenarios
                .iter()
                .find(|s| s.original == scene.scenario.identity().expect("validated scene"))
        });
        let mapped = match explicit.map_or_else(
            || mapping.scenario(&scene.scenario, &target.program),
            |replacement| {
                mapping.replacement(&scene.scenario, &target.program, &replacement.replacement)
            },
        ) {
            Ok(s) => s,
            Err(e) => {
                check.state = CheckState::Unsupported;
                check.explanation =
                    format!("Accepted scene cannot be mapped to this implementation: {e}");
                report.add(check);
                return Ok(exercised);
            }
        };
        let (run, target_contexts) = match execute(
            &self.runtime,
            target,
            &mapped,
            graph,
            self.limits.clone(),
            &format!(
                "check-{}",
                &canonical_digest(IdentityDomain::Decision, &(&decision.id, index))?.as_str()[..32]
            ),
        ) {
            Ok(r) => r,
            Err(e) => {
                check.state = CheckState::Unsupported;
                check.explanation =
                    format!("Actual implementation cannot execute the mapped requirement: {e}");
                report.add(check);
                return Ok(exercised);
            }
        };
        if run.state == EvidenceState::Observed {
            let scope_check = evolution::mapped_operations(
                &exercised,
                &scene.scenario,
                &mapped,
                &mapping,
                &scene.program.program,
                &target.program,
            )
            .and_then(|operations| {
                verify_target_scope(&decision.scope, &operations, &mapping, &target_contexts)
            });
            if let Err(error) = scope_check {
                check.binding = run.binding.clone();
                check.evidence = Some(run.identity()?);
                check.state = CheckState::Unknown;
                check.explanation =
                    format!("Mapped target operation or scope is unverified: {error}");
                report.runs.push(run);
                report.add(check);
                return Ok(exercised);
            }
        }
        check.binding = run.binding.clone();
        check.evidence = Some(run.identity()?);
        if run.state != EvidenceState::Observed {
            check.state = match run.state {
                EvidenceState::Failed => CheckState::Failed,
                EvidenceState::Stale => CheckState::Stale,
                EvidenceState::Unsupported => CheckState::Unsupported,
                _ => CheckState::Unknown,
            };
            check.explanation = format!("Replacement execution is {:?}; only a completed independent check can establish this intention", run.state);
        } else {
            let mut result = Some(true);
            // Choosing a concrete outcome still binds its observed consequences when no
            // separately authored predicate list exists. Empty is never vacuous proof.
            if decision.obligations.is_empty() || scene.bind_outcome {
                result = same_outcome(scene.observations(), &run.observations, &mapping)?;
            }
            if result != Some(false) {
                for p in &decision.obligations {
                    match mapping.property(p).evaluate(&run.observations) {
                        Some(false) => {
                            result = Some(false);
                            break;
                        }
                        None => result = None,
                        Some(true) => {}
                    }
                }
            }
            check.state = match result {
                Some(true) => CheckState::Satisfied,
                Some(false) => CheckState::Violated,
                None => CheckState::Unknown,
            };
            check.explanation=match result{Some(true)=>"Independently replayed the accepted business outcome on this implementation",Some(false)=>"Replacement violates an accepted intention; repair it without re-asking the settled business choice",None=>"Required observable is unmapped or unsupported; this is not a pass"}.into();
        }
        report.runs.push(run);
        report.add(check);
        Ok(exercised)
    }
    fn reproduce_scene(
        &self,
        scene: &AcceptedScene,
        graph: &DecisionGraph,
    ) -> Result<(RunEvidence, Vec<ScopeContext>)> {
        scene.validate()?;
        let result = execute(
            &self.runtime,
            &scene.program,
            &scene.scenario,
            graph,
            self.limits.clone(),
            "reproduce-history",
        )?;
        if result.0.state != EvidenceState::Observed
            || scene.observations() != result.0.observations.as_slice()
        {
            return Err(DecisionError::Unverified(
                "Supplied accepted history does not reproduce in the production runtime".into(),
            ));
        }
        Ok(result)
    }
    pub fn prepare_choice(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        id: &str,
    ) -> Result<VerifiedChange> {
        self.prepare_choice_resolving(store, target, choice, scenes, &[], id)
    }
    /// Resolve only these exact pending choices after the person chooses. Active
    /// promises require an explicit evolution or withdrawal, never this path.
    pub fn prepare_resolution(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        choice: Choice,
        scenes: Vec<AcceptedScene>,
        resolves: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        if resolves.is_empty() {
            return Err(invalid("resolution needs exact pending decision IDs"));
        }
        self.prepare_choice_resolving(store, target, choice, scenes, resolves, id)
    }
    fn prepare_choice_resolving(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        choice: Choice,
        mut scenes: Vec<AcceptedScene>,
        resolves: &[Id],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        if choice.binding == IntentionBinding::PropertiesOnly && choice.obligations.is_empty() {
            return Err(invalid(
                "explicit property-only intentions require nonempty verified predicates",
            ));
        }
        let mut verified_properties = false;

        let active = matches!(
            choice.outcome,
            DecisionOutcome::Accept { .. } | DecisionOutcome::KeepCurrent
        );
        if let DecisionOutcome::Accept { artifact } = &choice.outcome {
            if artifact != &target.artifact.program_digest {
                return Err(invalid("chosen artifact differs from adoption target"));
            }
        }
        if (!active || matches!(choice.outcome, DecisionOutcome::KeepCurrent))
            && target != current.program()?
        {
            return Err(invalid(
                "this choice must retain the current executable behavior",
            ));
        }
        if active && scenes.iter().any(|s| s.program.artifact != target.artifact) {
            return Err(invalid(
                "chosen scenes belong to another executable outcome",
            ));
        }
        for scene in &scenes {
            if scene.program.binding.project_id != current.data.project_id {
                return Err(invalid("chosen scene belongs to another project"));
            }
            let (actual, contexts) = self.reproduce_scene(scene, &current.decisions)?;
            if assess_scope(&choice.scope, &contexts) != ScopeMatch::Applies {
                continue;
            }
            verified_properties = true;
            for p in &choice.obligations {
                if p.evaluate(&actual.observations) != Some(true) {
                    return Err(invalid(
                        "accepted property is not demonstrated by the chosen scene",
                    ));
                }
            }
        }
        if choice.binding == IntentionBinding::PropertiesOnly && !verified_properties {
            return Err(DecisionError::Unverified(
                "No applicable scene verifies the property-only intention".into(),
            ));
        }
        for scene in &mut scenes {
            scene.bind_outcome = choice.binding == IntentionBinding::ObservedOutcome;
        }
        let mut next = current.decisions.clone();
        if resolves.iter().collect::<BTreeSet<_>>().len() != resolves.len() {
            return Err(invalid("duplicate pending resolution"));
        }
        for prior in resolves {
            let old = next
                .decisions
                .iter_mut()
                .find(|d| &d.id == prior && d.status == DecisionStatus::Pending)
                .ok_or_else(|| invalid("resolution must name a current pending decision"))?;
            old.status = DecisionStatus::Superseded {
                by: choice.id.clone(),
            };
            old.revision = old
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("decision revision overflow"))?;
        }
        if next.decisions.iter().any(|d| d.id == choice.id) {
            return Err(invalid("decision ID already exists; preserve its history"));
        }
        let decision = ScopedDecision {
            id: choice.id,
            revision: 1,
            request: choice.request,
            rationale: choice.rationale,
            scope: choice.scope,
            outcome: choice.outcome,
            status: if active {
                DecisionStatus::Active
            } else {
                DecisionStatus::Pending
            },
            obligations: choice.obligations,
            scenarios: scenes
                .iter()
                .map(|s| s.scenario.identity())
                .collect::<std::result::Result<_, _>>()?,
            witness: self.archive.stage(&scenes)?,
            supersedes: resolves.to_vec(),
        };
        decision.validate()?;
        next.decisions.push(decision);
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("decision revision overflow"))?;
        self.prepare(
            store,
            &current,
            target,
            next,
            self.current_mappings(&current)?,
            resolves.to_vec(),
            id,
        )
    }
    pub fn prepare_change(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        mappings: &[SemanticMapping],
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        let mappings = self.compose_for_current(&current, target, mappings)?;
        self.prepare(
            store,
            &current,
            target,
            current.decisions.clone(),
            mappings,
            vec![],
            id,
        )
    }
    fn compose_for_current(
        &self,
        current: &ProjectSnapshot,
        target: &CapturedProgram,
        added: &[SemanticMapping],
    ) -> Result<Vec<ImplementationMapping>> {
        self.compose_sources(
            &current.decisions,
            current.program()?,
            target,
            &self.current_mappings(current)?,
            added,
            &[],
        )
    }
    fn compose_sources(
        &self,
        graph: &DecisionGraph,
        current: &CapturedProgram,
        target: &CapturedProgram,
        prior: &[ImplementationMapping],
        added: &[SemanticMapping],
        requested_needs: &[Id],
    ) -> Result<Vec<ImplementationMapping>> {
        let origins = self.accepted_sources(graph)?;
        // Historical receipts remain reachable, but their unused recipes must
        // not constrain the next executable's session/schema. Determine demand
        // from actual source applicability, before attempting target composition.
        let mut required_scenes = BTreeSet::new();
        for decision in graph.decisions.iter().filter(|decision| {
            decision.status == DecisionStatus::Active || requested_needs.contains(&decision.id)
        }) {
            for scene in self.archive.load(&decision.witness)? {
                let (_, contexts) = self.reproduce_scene(&scene, graph)?;
                if assess_scope(&decision.scope, &contexts) == ScopeMatch::Applies {
                    required_scenes.insert((
                        scene.program.artifact.program_digest,
                        scene.scenario.identity()?,
                    ));
                }
            }
        }
        let mut seen = BTreeSet::new();
        for m in added {
            if m.from.kind != m.to.kind
                || !m.to.exists_in(&target.program)
                || !seen.insert(canonical_bytes(&m.from)?)
                || (!m.from.exists_in(&current.program)
                    && !origins.iter().any(|p| m.from.exists_in(&p.program)))
            {
                return Err(invalid(
                    "new mapping is ambiguous or not bound to actual source and target semantics",
                ));
            }
        }
        let mut result = vec![];
        for source in origins {
            if source.artifact.program_digest == target.artifact.program_digest {
                continue;
            }
            let old = prior
                .iter()
                .find(|m| m.source_program == source.artifact.program_digest)
                .map(|m| m.mappings.clone())
                .unwrap_or_default();
            let mut mappings = old.clone();
            for entry in &mut mappings {
                if let Some(new) = added.iter().find(|m| m.from == entry.to) {
                    entry.to = new.to.clone();
                }
            }
            for new in added {
                if new.from.exists_in(&source.program)
                    && (source.artifact.program_digest == current.artifact.program_digest
                        || !new.from.exists_in(&current.program)
                        || !old.iter().any(|m| m.from == new.from))
                {
                    if let Some(entry) = mappings.iter_mut().find(|m| m.from == new.from) {
                        *entry = new.clone();
                    } else {
                        mappings.push(new.clone());
                    }
                }
            }
            mappings.retain(|m| m.from != m.to && m.to.exists_in(&target.program));
            let mut scenarios = prior
                .iter()
                .find(|m| m.source_program == source.artifact.program_digest)
                .map(|m| {
                    m.scenarios
                        .iter()
                        .filter(|scenario| {
                            required_scenes.contains(&(
                                source.artifact.program_digest.clone(),
                                scenario.original.clone(),
                            ))
                        })
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            if !scenarios.is_empty() {
                let current_maps: Vec<_> = added
                    .iter()
                    .filter(|m| m.from.exists_in(&current.program))
                    .cloned()
                    .collect();
                let bridge = Mapping::new(&current.program, &target.program, &current_maps)?;
                for scenario in &mut scenarios {
                    let mut replacement = bridge.context(&scenario.replacement, &target.program)?;
                    replacement.inputs = replacement
                        .inputs
                        .iter()
                        .map(|input| bridge.input(input))
                        .collect();
                    // Explicit reconciliation inputs can override obsolete arguments.
                    // Final selected scenes are validated before independent execution.
                    scenario.replacement = replacement;
                }
            }
            if !mappings.is_empty() || !scenarios.is_empty() {
                result.push(ImplementationMapping {
                    source_program: source.artifact.program_digest,
                    mappings,
                    scenarios,
                });
            }
        }
        Ok(result)
    }
    pub fn prepare_recovery(
        &self,
        store: &ProductStore,
        target: &CapturedProgram,
        id: &str,
    ) -> Result<VerifiedChange> {
        let current = store.load()?;
        if !current.programs.contains(target) {
            return Err(invalid(
                "recovery target must be an actual retained program revision",
            ));
        }
        let result = self.prepare(
            store,
            &current,
            target,
            current.decisions.clone(),
            self.mappings_for(&current, target)?,
            vec![],
            id,
        )?;
        let recovery = RecoveryPlan {
            adoption: result.prepared.plan().clone(),
            withdraw_decisions: vec![],
            preserve_current_data: current.data.identity()?,
            preserve_current_events: canonical_digest(IdentityDomain::Data, &current.data.events)?,
        };
        store.prepare_recovery(&recovery, target)?;
        Ok(result)
    }
    fn verify_destination_packages(
        &self,
        store: &ProductStore,
        current: &ProjectSnapshot,
        next: &DecisionGraph,
        plan: &AdoptionPlan,
    ) -> Result<()> {
        let destination = IntentArchive::new(store.clone());
        for decision in &next.decisions {
            let scenes = destination.load(&decision.witness)?;
            validate_scene_bindings(decision, &scenes)?;
            if scenes
                .iter()
                .map(|s| s.scenario.identity())
                .collect::<std::result::Result<Vec<_>, _>>()?
                != decision.scenarios
                || scenes
                    .iter()
                    .any(|s| s.program.binding.project_id != current.data.project_id)
            {
                return Err(invalid(
                    "destination intention package differs from its project/decision references",
                ));
            }
        }
        for receipt in &current.adoptions {
            if let Some(digest) = receipt.plan.evidence.first() {
                destination.load_mappings(digest, &receipt.plan.target)?;
            }
        }
        if let Some(digest) = plan.evidence.first() {
            destination.load_mappings(digest, &plan.target)?;
        }
        Ok(())
    }
    fn prepare(
        &self,
        store: &ProductStore,
        current: &ProjectSnapshot,
        target: &CapturedProgram,
        next: DecisionGraph,
        mut mappings: Vec<ImplementationMapping>,
        retire: Vec<Id>,
        id: &str,
    ) -> Result<VerifiedChange> {
        validate_withdrawal_history(current)?;
        let report = self.check_bound(&next, target, &mappings)?;
        if report.disposition != CheckDisposition::Ready {
            return Err(DecisionError::Unverified(format!(
                "Adoption blocked: {:?}: {:?}",
                report.disposition, report.checks
            )));
        }
        // Only after every active scoped scene passes may obsolete, unused
        // recipes be omitted from this target's receipt. Their historical
        // receipts and decision witnesses remain immutable and reachable.
        for bound in &mut mappings {
            let mut kept = vec![];
            for replacement in &bound.scenarios {
                let mut valid = false;
                for decision in &next.decisions {
                    for scene in self.archive.load(&decision.witness)? {
                        if scene.program.artifact.program_digest == bound.source_program
                            && scene.scenario.identity()? == replacement.original
                        {
                            let mapping = Mapping::new(
                                &scene.program.program,
                                &target.program,
                                &bound.mappings,
                            )?;
                            valid = mapping
                                .replacement(
                                    &scene.scenario,
                                    &target.program,
                                    &replacement.replacement,
                                )
                                .is_ok();
                        }
                    }
                }
                if valid {
                    kept.push(replacement.clone());
                }
            }
            bound.scenarios = kept;
        }
        mappings.retain(|m| !m.mappings.is_empty() || !m.scenarios.is_empty());
        let compatibility =
            LocalRuntime::default().compatibility_at(target, &current.data, current.clock_day)?;
        let plan = AdoptionPlan {
            version: CONTRACT_VERSION,
            id: id.into(),
            project_id: current.data.project_id.clone(),
            expected_generation: current.data.generation,
            expected_data: current.data.identity()?,
            expected_decisions: current.decisions.identity()?,
            expected_session: current.session.identity()?,
            current_source: current.program()?.binding.clone(),
            target: target.artifact.clone(),
            scope: DecisionScope {
                operations: target
                    .program
                    .actions
                    .iter()
                    .map(|a| a.id.clone())
                    .collect(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            compatibility,
            required_decisions: next
                .decisions
                .iter()
                .filter(|d| d.status == DecisionStatus::Active)
                .map(|d| d.id.clone())
                .collect(),
            checks: report.checks.clone(),
            evidence: std::iter::once(Ok(self
                .archive
                .stage_mappings(&target.artifact, &mappings)?))
            .chain(report.runs.iter().map(RunEvidence::identity))
            .collect::<std::result::Result<_, _>>()?,
            retire_decisions: retire,
        };
        validate_withdrawal_delta(current, &next, &plan)?;
        self.verify_destination_packages(store, current, &next, &plan)?;
        let prepared = store.prepare_adoption(plan, target)?;
        Ok(VerifiedChange {
            prepared,
            revision: current.revision,
            target: target.clone(),
            decisions: next,
            mappings,
            report,
            expected_snapshot: canonical_digest(IdentityDomain::Data, current)?,
            runtime: self.runtime.capabilities().version,
        })
    }
    pub fn adopt(&self, store: &ProductStore, change: &VerifiedChange) -> Result<ProjectSnapshot> {
        Ok(store.adopt_verified(
            change.revision,
            &change.prepared,
            &change.target,
            &change.decisions,
            |current, target, next, plan| {
                if canonical_digest(IdentityDomain::Data, current)? != change.expected_snapshot
                    || self.runtime.capabilities().version != change.runtime
                    || next != &change.decisions
                    || target != &change.target
                    || plan != change.prepared.plan()
                {
                    return Err(StoreError::Conflict(
                        "Intention rehearsal changed; recheck current source, data and runtime"
                            .into(),
                    ));
                }
                validate_withdrawal_history(current)
                    .and_then(|_| validate_withdrawal_delta(current, next, plan))
                    .map_err(|e| StoreError::Invalid(e.to_string()))?;
                self.verify_destination_packages(store, current, next, plan)
                    .map_err(|e| StoreError::Invalid(e.to_string()))?;
                for decision in &next.decisions {
                    if !current
                        .decisions
                        .decisions
                        .iter()
                        .any(|old| old.id == decision.id && old.witness == decision.witness)
                    {
                        for scene in self
                            .archive
                            .load(&decision.witness)
                            .map_err(|e| StoreError::Invalid(e.to_string()))?
                        {
                            self.reproduce_scene(&scene, next)
                                .map_err(|e| StoreError::Invalid(e.to_string()))?;
                        }
                    }
                }
                let report = self
                    .check_bound(next, target, &change.mappings)
                    .map_err(|e| StoreError::Invalid(e.to_string()))?;
                if report.disposition != CheckDisposition::Ready {
                    return Err(StoreError::Invalid(
                        "Current implementation fails independent intention replay".into(),
                    ));
                }
                Ok(())
            },
        )?)
    }
}
