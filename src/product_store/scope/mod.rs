//! Host-owned scoped executable composition and protected completion history.
mod compiler;
mod contract;
mod correspondence;
mod history;
mod rehearsal;
mod replay;
pub(super) use correspondence::install as retain_correspondences;
pub(super) use rehearsal::{rehearsal_request, retain_rehearsal};
pub(crate) use replay::ProvenanceColumns;
pub use replay::ScopedExecutionContext;
mod verify;
use super::*;
use crate::product_runtime::{DRIVER_VERSION, RUNTIME_VERSION};
pub use contract::*;
use std::collections::BTreeSet;

fn program<'a>(snapshot: &'a ProjectSnapshot, id: &Digest) -> Result<&'a CapturedProgram> {
    snapshot
        .programs
        .iter()
        .find(|p| revision(p).ok().as_ref() == Some(id))
        .ok_or_else(|| compiler::error("retained source revision missing"))
}
fn basis(snapshot: &ProjectSnapshot) -> Result<FrozenBasis> {
    Ok(FrozenBasis {
        snapshot: canonical_digest(IdentityDomain::Data, snapshot)?,
        revision: snapshot.revision,
        active: snapshot.active_revision.clone(),
        data: snapshot.data.clone(),
        session: snapshot.session.clone(),
        day: snapshot.clock_day,
        decisions: snapshot.decisions.identity()?,
    })
}
fn retain(snapshot: &mut ProjectSnapshot, target: &CapturedProgram) -> Result<Digest> {
    let id = revision(target)?;
    if !snapshot
        .programs
        .iter()
        .any(|p| revision(p).ok().as_ref() == Some(&id))
    {
        snapshot.programs.push(target.clone());
    }
    Ok(id)
}
fn request_valid(snapshot: &ProjectSnapshot, request: &ScopeRequest) -> Result<()> {
    if request.operations.is_empty()
        || request.operations.len() > MAX_ITEMS
        || request.excluded_records.len() > MAX_COLLECTION
    {
        return Err(compiler::error("invalid scope bounds"));
    }
    for operation in &request.operations {
        if !snapshot
            .program()?
            .program
            .actions
            .iter()
            .any(|a| &a.id == operation)
        {
            return Err(compiler::error("unknown scope operation"));
        }
    }
    let mut entities = BTreeSet::new();
    for lifecycle in &request.lifecycles {
        if lifecycle.source != snapshot.active_revision || !entities.insert(&lifecycle.entity) {
            return Err(compiler::error("stale or duplicate completion binding"));
        }
        if snapshot
            .scope
            .layers
            .values()
            .flat_map(|layer| &layer.request.lifecycles)
            .any(|prior| prior.entity == lifecycle.entity && prior.completed != lifecycle.completed)
        {
            return Err(compiler::error("completion meaning differs from protected entity history; lifecycle redefinition requires an explicit preservation design"));
        }
        compiler::own_row(
            &lifecycle.completed,
            "record",
            &lifecycle.entity,
            &snapshot.program()?.program,
            &BTreeMap::new(),
            false,
        )?;
    }
    let check_refs = |refs: &[RecordRef]| -> Result<()> {
        let mut seen = BTreeSet::new();
        for r in refs {
            if !seen.insert((&r.entity, &r.record))
                || !snapshot
                    .data
                    .records
                    .iter()
                    .any(|row| history::row_ref(row) == *r)
            {
                return Err(compiler::error("missing or duplicate frozen record"));
            }
        }
        Ok(())
    };
    check_refs(&request.excluded_records)?;
    if let ScopePopulation::SelectedUnfinished { records } = &request.population {
        if records.is_empty() || records.len() > MAX_COLLECTION {
            return Err(compiler::error(
                "selected unfinished population is empty or too large",
            ));
        }
        check_refs(records)?;
        for reference in records {
            let row = snapshot
                .data
                .records
                .iter()
                .find(|r| history::row_ref(r) == *reference)
                .unwrap();
            let lifecycle = request
                .lifecycles
                .iter()
                .find(|l| l.entity == row.entity)
                .ok_or_else(|| compiler::error("selected record has no completed-work meaning"))?;
            if row.archived
                || history::completed(
                    snapshot.program()?,
                    &snapshot.data,
                    reference,
                    lifecycle,
                    snapshot.clock_day,
                )?
            {
                return Err(compiler::error(
                    "selected work is already completed or archived",
                ));
            }
            if request.excluded_records.contains(reference) {
                return Err(compiler::error("selected work is also excluded"));
            }
        }
    }
    Ok(())
}
fn validate_initial_runtime(target: &CapturedProgram, data: &DataSnapshot, day: i32) -> Result<()> {
    LocalRuntime::default().start(
        target,
        data,
        &SessionState::initial(&target.program)?,
        day,
        0,
        RuntimeLimits::default(),
    )?;
    Ok(())
}
fn prepare(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &ScopeRequest,
    id: &str,
) -> Result<PreparedScopedChange> {
    snapshot.validate()?;
    candidate.validate()?;
    compiler::reject_reserved(&candidate.program)?;
    if !valid_id(id)
        || candidate.binding.project_id != snapshot.data.project_id
        || snapshot.scope.layers.len() >= MAX_LAYERS
        || snapshot.operations.contains_key(id)
    {
        return Err(compiler::error(
            "invalid, reused or over-budget scoped operation",
        ));
    }
    request_valid(snapshot, request)?;
    let business = compiler::business_program(snapshot)?;
    let patches = compiler::derive_patches(&business, &candidate.program, request)?;
    let layer = ScopeLayer {
        version: COMPILER_VERSION,
        operation: id.into(),
        basis: basis(snapshot)?,
        candidate: revision(candidate)?,
        request: request.clone(),
        patches,
    };
    let layer_id = layer.identity()?;
    let mut working = snapshot.clone();
    retain(&mut working, candidate)?;
    working.scope.layers.insert(layer_id.clone(), layer.clone());
    let prior = snapshot.scope.compositions.get(&snapshot.active_revision);
    let mut layers = prior.map(|m| m.layers.clone()).unwrap_or_default();
    layers.push(layer_id.clone());
    let mut active = prior.map(|m| m.active.clone()).unwrap_or_default();
    active.insert(layer_id);
    let mut manifest = CompositionManifest {
        version: COMPILER_VERSION,
        basis: basis(snapshot)?,
        transition: ScopeTransition::Adoption,
        rewrites: prior.map(|m| m.rewrites.clone()).unwrap_or_default(),
        business: revision(candidate)?,
        layers,
        active,
        previous: prior.map(|_| snapshot.active_revision.clone()),
        operation: id.into(),
        runtime: RUNTIME_VERSION.into(),
        driver: DRIVER_VERSION.into(),
        output: snapshot.active_revision.clone(),
    };
    let target = compiler::compile(&working, &manifest)?;
    manifest.output = revision(&target)?;
    let (initialized, receipt) = history::initialize(&working, &layer, &target)?;
    let compatibility =
        LocalRuntime::default().compatibility_at(&target, &initialized, snapshot.clock_day)?;
    if compatibility.state != CompatibilityState::Compatible {
        return Err(StoreError::Incompatible(compatibility));
    }
    validate_initial_runtime(&target, &initialized, snapshot.clock_day)?;
    let plan = make_plan(snapshot, &target, layer.scope(), id)?;
    Ok(PreparedScopedChange {
        correspondences: BTreeMap::new(),
        adoption: plan,
        basis: layer.basis.snapshot.clone(),
        target,
        candidate: candidate.clone(),
        scope: layer.scope(),
        layer: Some(layer),
        evolution: None,
        manifest,
        initialized,
        receipt: Some(receipt),
        compatibility,
    })
}
fn prepare_withdrawal(
    snapshot: &ProjectSnapshot,
    layers: &[Digest],
    id: &str,
) -> Result<PreparedScopedChange> {
    snapshot.validate()?;
    let previous = snapshot
        .scope
        .compositions
        .get(&snapshot.active_revision)
        .ok_or_else(|| compiler::error("there is no managed scope to withdraw"))?;
    let unique: BTreeSet<_> = layers.iter().cloned().collect();
    if !valid_id(id)
        || layers.is_empty()
        || unique.len() != layers.len()
        || unique.iter().any(|layer| !previous.active.contains(layer))
        || snapshot.operations.contains_key(id)
    {
        return Err(compiler::error(
            "withdrawal must name exact distinct active layers",
        ));
    }
    let mut manifest = previous.clone();
    manifest.basis = basis(snapshot)?;
    manifest.transition = ScopeTransition::Withdrawal;
    manifest.previous = Some(snapshot.active_revision.clone());
    manifest.operation = id.into();
    manifest.active.retain(|layer| !unique.contains(layer));
    let target = compiler::compile(snapshot, &manifest)?;
    manifest.output = revision(&target)?;
    let initialized = merged_data(&target, &snapshot.data)?;
    if initialized.records != snapshot.data.records || initialized.events != snapshot.data.events {
        return Err(compiler::error("withdrawal changed current facts"));
    }
    let compatibility =
        LocalRuntime::default().compatibility_at(&target, &initialized, snapshot.clock_day)?;
    if compatibility.state != CompatibilityState::Compatible {
        return Err(StoreError::Incompatible(compatibility));
    }
    validate_initial_runtime(&target, &initialized, snapshot.clock_day)?;
    let scope = DecisionScope {
        operations: layers
            .iter()
            .flat_map(|id| snapshot.scope.layers[id].request.operations.iter().cloned())
            .collect(),
        population: Population::All,
        conditions: Values::new(),
        excluded_records: vec![],
        unknowns: vec![],
    };
    let adoption = make_plan(snapshot, &target, scope.clone(), id)?;
    Ok(PreparedScopedChange {
        correspondences: BTreeMap::new(),
        basis: manifest.basis.snapshot.clone(),
        adoption,
        target,
        candidate: program(snapshot, &manifest.business)?.clone(),
        layer: None,
        evolution: None,
        manifest,
        initialized,
        receipt: None,
        compatibility,
        scope,
    })
}

fn prepare_evolution(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    mappings: &[ScopeSlotMapping],
    id: &str,
) -> Result<PreparedScopedChange> {
    snapshot.validate()?;
    candidate.validate()?;
    compiler::reject_reserved(&candidate.program)?;
    if !valid_id(id)
        || candidate.binding.project_id != snapshot.data.project_id
        || snapshot.operations.contains_key(id)
    {
        return Err(compiler::error("invalid managed evolution identity"));
    }
    let previous = snapshot
        .scope
        .compositions
        .get(&snapshot.active_revision)
        .ok_or_else(|| compiler::error("unmanaged projects use ordinary adoption"))?;
    compiler::validate_mappings(snapshot, previous, candidate, mappings)?;
    let mut working = snapshot.clone();
    retain(&mut working, candidate)?;
    let mut manifest = previous.clone();
    manifest.basis = basis(snapshot)?;
    manifest.transition = ScopeTransition::Evolution;
    manifest.business = revision(candidate)?;
    manifest.rewrites = mappings.to_vec();
    manifest.previous = Some(snapshot.active_revision.clone());
    manifest.operation = id.into();
    let target = compiler::compile(&working, &manifest)?;
    manifest.output = revision(&target)?;
    let initialized = merged_data(&target, &snapshot.data)?;
    if initialized.records != snapshot.data.records || initialized.events != snapshot.data.events {
        return Err(compiler::error(
            "managed evolution changed current business facts",
        ));
    }
    let compatibility =
        LocalRuntime::default().compatibility_at(&target, &initialized, snapshot.clock_day)?;
    if compatibility.state != CompatibilityState::Compatible {
        return Err(StoreError::Incompatible(compatibility));
    }
    validate_initial_runtime(&target, &initialized, snapshot.clock_day)?;
    let scope = DecisionScope {
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
    };
    let adoption = make_plan(snapshot, &target, scope.clone(), id)?;
    Ok(PreparedScopedChange {
        correspondences: BTreeMap::new(),
        basis: manifest.basis.snapshot.clone(),
        adoption,
        target,
        candidate: candidate.clone(),
        layer: None,
        evolution: Some(mappings.to_vec()),
        manifest,
        initialized,
        receipt: None,
        compatibility,
        scope,
    })
}

fn make_plan(
    snapshot: &ProjectSnapshot,
    target: &CapturedProgram,
    scope: DecisionScope,
    id: &str,
) -> Result<PreparedAdoption> {
    let compatibility =
        LocalRuntime::default().compatibility_at(target, &snapshot.data, snapshot.clock_day)?;
    if compatibility.state != CompatibilityState::Compatible {
        return Err(StoreError::Incompatible(compatibility));
    }
    let plan = AdoptionPlan {
        version: CONTRACT_VERSION,
        id: id.into(),
        project_id: snapshot.data.project_id.clone(),
        expected_generation: snapshot.data.generation,
        expected_data: snapshot.data.identity()?,
        expected_decisions: snapshot.decisions.identity()?,
        expected_session: snapshot.session.identity()?,
        current_source: snapshot.program()?.binding.clone(),
        target: target.artifact.clone(),
        scope,
        compatibility,
        required_decisions: snapshot
            .decisions
            .decisions
            .iter()
            .filter(|d| d.status == DecisionStatus::Active)
            .map(|d| d.id.clone())
            .collect(),
        checks: vec![],
        evidence: vec![],
        retire_decisions: vec![],
    };
    plan.validate()?;
    Ok(PreparedAdoption {
        plan,
        expected_revision: snapshot.revision,
        target: revision(target)?,
    })
}

impl ProductStore {
    pub fn prepare_managed_evolution(
        &self,
        candidate: &CapturedProgram,
        mappings: &[ScopeSlotMapping],
        id: &str,
    ) -> Result<PreparedScopedChange> {
        prepare_evolution(&self.load()?, candidate, mappings, id)
    }
    pub fn prepare_scoped_withdrawal(
        &self,
        layers: &[Digest],
        id: &str,
    ) -> Result<PreparedScopedChange> {
        prepare_withdrawal(&self.load()?, layers, id)
    }
    pub fn prepare_scoped_change(
        &self,
        candidate: &CapturedProgram,
        request: &ScopeRequest,
        id: &str,
    ) -> Result<PreparedScopedChange> {
        prepare(&self.load()?, candidate, request, id)
    }
    /// Empty-intention projects may use this direct path. Projects with accepted
    /// intentions must use the decision engine's independent replay callback.
    pub fn adopt_scoped(
        &self,
        expected_revision: u64,
        prepared: &PreparedScopedChange,
    ) -> Result<ProjectSnapshot> {
        let snapshot = self.load()?;
        let plan = self.scoped_plan(prepared)?;
        self.adopt_scoped_verified(
            expected_revision,
            prepared,
            &plan,
            &snapshot.decisions,
            |current, _, next, _| {
                if current.decisions != *next
                    || next
                        .decisions
                        .iter()
                        .any(|d| d.status == DecisionStatus::Active)
                {
                    return Err(compiler::error(
                        "active intentions require independent current-target replay",
                    ));
                }
                Ok(())
            },
        )
    }
    pub fn scoped_plan(&self, prepared: &PreparedScopedChange) -> Result<PreparedAdoption> {
        Ok(prepared.adoption.clone())
    }
    pub fn adopt_scoped_verified<F>(
        &self,
        expected_revision: u64,
        prepared: &PreparedScopedChange,
        adoption: &PreparedAdoption,
        decisions: &DecisionGraph,
        verify: F,
    ) -> Result<ProjectSnapshot>
    where
        F: FnOnce(&ProjectSnapshot, &CapturedProgram, &DecisionGraph, &AdoptionPlan) -> Result<()>,
    {
        self.adopt_scoped_inner(
            expected_revision,
            prepared,
            adoption,
            decisions,
            verify,
            #[cfg(test)]
            None,
        )
    }
    #[cfg(test)]
    pub fn adopt_scoped_with_fault(
        &self,
        expected_revision: u64,
        prepared: &PreparedScopedChange,
        fault: FaultPoint,
    ) -> Result<ProjectSnapshot> {
        let snapshot = self.load()?;
        let plan = self.scoped_plan(prepared)?;
        self.adopt_scoped_inner(
            expected_revision,
            prepared,
            &plan,
            &snapshot.decisions,
            |s, _, _, _| {
                if s.decisions.decisions.is_empty() {
                    Ok(())
                } else {
                    Err(compiler::error("fault helper does not bypass intentions"))
                }
            },
            Some(fault),
        )
    }
    fn adopt_scoped_inner<F>(
        &self,
        expected_revision: u64,
        prepared: &PreparedScopedChange,
        adoption: &PreparedAdoption,
        decisions: &DecisionGraph,
        verify: F,
        #[cfg(test)] fault: Option<FaultPoint>,
    ) -> Result<ProjectSnapshot>
    where
        F: FnOnce(&ProjectSnapshot, &CapturedProgram, &DecisionGraph, &AdoptionPlan) -> Result<()>,
    {
        let _lock = self.root.lock()?;
        let mut current = self.load()?;
        let request = canonical_digest(
            IdentityDomain::Adoption,
            &(
                &adoption.plan,
                &prepared.manifest,
                decisions,
                &prepared.correspondences,
            ),
        )?;
        if let Some(receipt) = current.operations.get(&adoption.plan.id) {
            return if receipt.request == request {
                Ok(current)
            } else {
                Err(StoreError::Conflict(
                    "scoped adoption ID already belongs to another request".into(),
                ))
            };
        }
        if current.revision != expected_revision
            || current.revision != adoption.expected_revision
            || canonical_digest(IdentityDomain::Data, &current)? != prepared.basis
        {
            return Err(StoreError::Conflict(
                "scoped rehearsal is stale; source, data, session, day or intentions changed"
                    .into(),
            ));
        }
        let regenerated = if let Some(layer) = &prepared.layer {
            prepare(
                &current,
                &prepared.candidate,
                &layer.request,
                &layer.operation,
            )?
        } else if let Some(mappings) = &prepared.evolution {
            prepare_evolution(
                &current,
                &prepared.candidate,
                mappings,
                &prepared.manifest.operation,
            )?
        } else {
            let prior = current
                .scope
                .compositions
                .get(&current.active_revision)
                .ok_or_else(|| compiler::error("withdrawal source is not managed"))?;
            let withdrawn: Vec<_> = prior
                .active
                .difference(&prepared.manifest.active)
                .cloned()
                .collect();
            prepare_withdrawal(&current, &withdrawn, &prepared.manifest.operation)?
        };
        if regenerated.manifest != prepared.manifest
            || regenerated.target != prepared.target
            || regenerated.initialized != prepared.initialized
            || regenerated.receipt != prepared.receipt
            || regenerated.compatibility != prepared.compatibility
            || adoption.plan.scope != prepared.scope
        {
            return Err(compiler::error(
                "prepared composition or initialization does not regenerate",
            ));
        }
        self.check_plan(&current, &adoption.plan, &prepared.target)?;
        decisions.validate()?;
        verify(&current, &prepared.target, decisions, &adoption.plan)?;
        let previous = current.active_revision.clone();
        retain(&mut current, &prepared.candidate)?;
        retain(&mut current, &prepared.target)?;
        if let Some(layer) = &prepared.layer {
            current
                .scope
                .layers
                .insert(layer.identity()?, layer.clone());
        }
        current
            .scope
            .compositions
            .insert(prepared.manifest.output.clone(), prepared.manifest.clone());
        if let Some(receipt) = &prepared.receipt {
            current.scope.initializations.push(receipt.clone());
        }
        current.data = prepared.initialized.clone();
        current.active_revision = prepared.manifest.output.clone();
        current.session = SessionState::initial(&prepared.target.program)?;
        let activated_decisions: Vec<_> = decisions
            .decisions
            .iter()
            .filter(|d| {
                d.status == DecisionStatus::Active
                    && d.scope == prepared.scope
                    && !current.decisions.decisions.iter().any(|old| old.id == d.id)
            })
            .map(|d| d.id.clone())
            .collect();
        current.decisions = decisions.clone();
        current.revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| compiler::error("revision exhausted"))?;
        current.adoptions.push(AdoptionReceipt {
            plan: adoption.plan.clone(),
            previous,
            active: current.active_revision.clone(),
            revision: current.revision,
        });
        current.scope.adoptions.push(ScopedAdoptionReceipt {
            decisions: activated_decisions,
            plan: adoption.plan.identity()?,
            composition: canonical_digest(IdentityDomain::Adoption, &prepared.manifest)?,
            initialization: prepared
                .receipt
                .as_ref()
                .map(|r| canonical_digest(IdentityDomain::Adoption, r))
                .transpose()?,
            initialized_compatibility: prepared.compatibility.clone(),
            revision: current.revision,
        });
        current.operations.insert(
            adoption.plan.id.clone(),
            OperationReceipt {
                operation: adoption.plan.id.clone(),
                request,
                revision: current.revision,
            },
        );
        let revision = current.revision;
        correspondence::install(
            &mut current,
            &prepared.correspondences,
            &adoption.plan.id,
            revision,
        )?;
        LocalRuntime::default().start(
            &prepared.target,
            &current.data,
            &current.session,
            current.clock_day,
            0,
            RuntimeLimits::default(),
        )?;
        self.save(
            &current,
            true,
            #[cfg(test)]
            fault,
        )?;
        Ok(current)
    }
}

pub(super) fn verify_snapshot(snapshot: &ProjectSnapshot) -> Result<()> {
    verify::validate(snapshot)
}
pub(super) fn verify_transition(before: &ProjectSnapshot, after: &ProjectSnapshot) -> Result<()> {
    verify::preserve(before, after)
}
pub(super) fn reject_unmanaged_target(
    snapshot: &ProjectSnapshot,
    target: &CapturedProgram,
) -> Result<()> {
    if snapshot.scope.compositions.contains_key(&revision(target)?) {
        Ok(())
    } else {
        compiler::reject_reserved(&target.program)
    }
}

impl ProductStore {
    /// The daily renderer gets preserved results only after full snapshot and
    /// history validation. Raw isolated interpreter views carry no such claim.
    pub fn runtime_view(&self) -> Result<crate::product_protocol::RuntimeView> {
        let snapshot = self.load()?;
        let runtime = LocalRuntime::default();
        let run = runtime.resume(
            snapshot.program()?,
            &snapshot.data,
            &snapshot.session,
            snapshot.clock_day,
            0,
            RuntimeLimits::default(),
            &snapshot.artifacts,
        )?;
        let mut view = runtime.view_model(&run)?;
        view.history = history::presentation(&snapshot)?;
        Ok(view)
    }
}

impl ProjectSnapshot {
    /// Supply a fresh author with ordinary editable rules and complete logical
    /// slot coordinates, without exposing frozen data or authoring authority.
    pub fn editable_scope_context(&self) -> Result<Option<ScopeEditableContext>> {
        self.validate()?;
        let Some(manifest) = self.scope.compositions.get(&self.active_revision) else {
            return Ok(None);
        };
        let app = compiler::business_program(self)?;
        compiler::reject_reserved(&app)?;
        let editable = CapturedProgram::capture(
            &canonical_bytes(&app)?,
            &self.data.project_id,
            Producer::ExternalAuthor {
                description: format!(
                    "Host-authored editable live-rule projection of {}",
                    self.active_revision.as_str()
                ),
            },
            None,
        )?;
        let mut slots = vec![];
        for id in &manifest.layers {
            for index in 0..self.scope.layers[id].patches.len() {
                let patch = compiler::effective_patch(self, manifest, id, index)?;
                slots.push(ScopeEditableSlot {
                    layer: id.clone(),
                    patch: index,
                    active: manifest.active.contains(id),
                    destination: patch.request.destination,
                    entity: patch.request.entity,
                    subject: patch.request.subject,
                    value_type: patch.request.value_type,
                });
            }
        }
        Ok(Some(ScopeEditableContext {
            compiled_source: self.active_revision.clone(),
            editable,
            slots,
        }))
    }
}
