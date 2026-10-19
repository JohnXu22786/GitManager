//! Lowering-3 is separate from the byte-reconstructible lowering-2 compiler.
//! Physical storage comes from the original initializer; producer identity is new.
use super::common_fact_admission::{refuse, AdmissionResult, ProofMeter};
use super::owned_continuation::VerifiedOwnedContinuation;
use super::owned_lowering::{empty, literal, rebind, Metadata};
use super::*;

pub(super) const OWNED_TRANSITION_LOWERING_VERSION: u32 = 3;
fn boolean(value: bool) -> Expr {
    literal(DataValue::Boolean { value }, Type::Boolean)
}
fn variable(name: &str) -> Expr {
    Expr::Variable { name: name.into() }
}
fn field(record: Expr, name: &str) -> Expr {
    Expr::Field {
        record: Box::new(record),
        field: name.into(),
    }
}
fn not(value: Expr) -> Expr {
    Expr::Not {
        value: Box::new(value),
    }
}
fn condition(test: Expr, yes: Expr, no: Expr) -> Expr {
    Expr::If {
        condition: Box::new(test),
        then_value: Box::new(yes),
        else_value: Box::new(no),
    }
}
fn next_revision(record: Expr, key: &str) -> Expr {
    Expr::Add {
        left: Box::new(field(record, key)),
        right: Box::new(literal(DataValue::Integer { value: 1 }, Type::Integer)),
    }
}
fn defaults(proof: &VerifiedOwnedContinuation, member: bool) -> AdmissionResult<Values> {
    let m = proof.layout();
    let mut values = Values::from([
        (m.member.clone(), DataValue::Boolean { value: member }),
        (m.sealed.clone(), DataValue::Boolean { value: false }),
        (m.day.clone(), DataValue::Null),
        (m.origin.clone(), DataValue::Null),
    ]);
    if let Some(revision) = &m.revision {
        values.insert(revision.clone(), DataValue::Integer { value: 1 });
    }
    for (key, p) in m.saved.iter().zip(proof.projections()) {
        values.insert(key.clone(), empty(&p.host_value_type)?);
    }
    Ok(values)
}
struct Compiler<'a> {
    proof: &'a VerifiedOwnedContinuation,
    active: bool,
    metadata: Metadata,
    names: BTreeSet<String>,
    next: usize,
    used_edits: BTreeSet<Id>,
    meter: &'a mut ProofMeter,
}
impl Compiler<'_> {
    fn fresh(&mut self) -> Id {
        loop {
            let name = format!("gm_scope_op_bind_{}", self.next);
            self.next += 1;
            if self.names.insert(name.clone()) {
                return name;
            }
        }
    }
    fn gate(&mut self, root: Expr, member: bool, steps: Vec<Statement>) -> Statement {
        let binding = self.fresh();
        let predicate = Expr::And {
            values: vec![
                Expr::Equal {
                    left: Box::new(variable(&binding)),
                    right: Box::new(root),
                },
                not(field(variable(&binding), &self.metadata.sealed)),
                if !self.active {
                    boolean(!member)
                } else if member {
                    field(variable(&binding), &self.metadata.member)
                } else {
                    not(field(variable(&binding), &self.metadata.member))
                },
            ],
        };
        Statement::ForEach {
            items: Expr::Query {
                entity: self.metadata.root.clone(),
                binding: binding.clone(),
                predicate: Box::new(predicate),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            },
            binding,
            steps,
        }
    }
    fn protect(&mut self, root: Expr, steps: Vec<Statement>) -> Statement {
        let binding = self.fresh();
        Statement::ForEach {
            items: Expr::Query {
                entity: self.metadata.root.clone(),
                binding: binding.clone(),
                predicate: Box::new(Expr::And {
                    values: vec![
                        Expr::Equal {
                            left: Box::new(variable(&binding)),
                            right: Box::new(root),
                        },
                        not(field(variable(&binding), &self.metadata.sealed)),
                    ],
                }),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            },
            binding,
            steps,
        }
    }
    fn compose(
        &mut self,
        action: &str,
        before: &[Statement],
        after: &[Statement],
        bp: &[usize],
        ap: &[usize],
    ) -> AdmissionResult<Vec<Statement>> {
        let mut out = vec![];
        let (mut i, mut j) = (0, 0);
        while i < before.len() || j < after.len() {
            self.meter.charge(1)?;
            let edit = self
                .proof
                .request()
                .inventory
                .sources
                .edits
                .iter()
                .find(|e| {
                    e.before.action == action
                        && e.before.list_path == bp
                        && e.after.list_path == ap
                        && e.before.start == i
                        && e.after.start == j
                })
                .cloned();
            if let Some(edit) = edit {
                if !self.used_edits.insert(edit.id.clone()) {
                    return Err(refuse(action, "repeated lowering endpoint"));
                }
                let old = before
                    .get(i..i + edit.before.len)
                    .ok_or_else(|| refuse(action, "baseline endpoint outside list"))?;
                let new = after
                    .get(j..j + edit.after.len)
                    .ok_or_else(|| refuse(action, "candidate endpoint outside list"))?;
                match (&edit.before.root, &edit.after.root) {
                    (Some(a), Some(b))
                        if a == b && a.value_type == Type::reference(&self.metadata.root) =>
                    {
                        if !old.is_empty() {
                            out.push(self.gate(variable(&a.name), false, old.to_vec()));
                        }
                        if !new.is_empty() {
                            out.push(self.gate(variable(&b.name), true, new.to_vec()));
                        }
                    }
                    (None, None) if old.is_empty() || new.is_empty() => {
                        let (steps, member) = if old.is_empty() {
                            (new, true)
                        } else {
                            (old, false)
                        };
                        for step in steps {
                            let Statement::ForEach {
                                items,
                                binding,
                                steps,
                            } = step
                            else {
                                return Err(refuse(
                                    &edit.id,
                                    "unbound variant is not a root frontier",
                                ));
                            };
                            if !matches!(items,Expr::Query{entity,limit,include_archived:false,..} if entity==&self.metadata.root&&*limit==MAX_COLLECTION)
                            {
                                return Err(refuse(
                                    &edit.id,
                                    "unbound variant lacks exact root query",
                                ));
                            }
                            let gate = self.gate(variable(binding), member, steps.clone());
                            out.push(Statement::ForEach {
                                items: items.clone(),
                                binding: binding.clone(),
                                steps: vec![gate],
                            });
                        }
                    }
                    _ => return Err(refuse(&edit.id, "unsupported root correspondence")),
                }
                i += edit.before.len;
                j += edit.after.len;
                continue;
            }
            let (a, b) = before
                .get(i)
                .zip(after.get(j))
                .ok_or_else(|| refuse(action, "unaccounted lowering endpoint"))?;
            match (a, b) {
                (
                    Statement::ForEach {
                        items: x,
                        binding: xb,
                        steps: xs,
                    },
                    Statement::ForEach {
                        items: y,
                        binding: yb,
                        steps: ys,
                    },
                ) if x == y && xb == yb => {
                    let mut nx = bp.to_vec();
                    nx.push(i);
                    let mut ny = ap.to_vec();
                    ny.push(j);
                    let steps = self.compose(action, xs, ys, &nx, &ny)?;
                    out.push(Statement::ForEach {
                        items: x.clone(),
                        binding: xb.clone(),
                        steps,
                    });
                }
                _ if a == b => out.push(a.clone()),
                _ => {
                    return Err(refuse(
                        action,
                        "unclassified changed statement during lowering",
                    ))
                }
            }
            i += 1;
            j += 1;
        }
        Ok(out)
    }
    fn seal(&mut self, record: Expr) -> AdmissionResult<Statement> {
        let binding = self.fresh();
        let predicate = Expr::And {
            values: vec![
                Expr::Equal {
                    left: Box::new(variable(&binding)),
                    right: Box::new(record),
                },
                not(field(variable(&binding), &self.metadata.sealed)),
                rebind(
                    &self.proof.request().inventory.lifecycle.after,
                    "record",
                    &binding,
                ),
            ],
        };
        let mut values = BTreeMap::new();
        for (i, p) in self.proof.report().projections.iter().enumerate() {
            values.insert(
                self.metadata.saved[i].clone(),
                self.proof.capture_expression(p, &binding, self.active)?,
            );
        }
        if let Some(revision) = &self.metadata.revision {
            values.insert(
                revision.clone(),
                next_revision(variable(&binding), revision),
            );
        }
        values.insert(self.metadata.sealed.clone(), boolean(true));
        values.insert(self.metadata.day.clone(), Expr::Today);
        values.insert(
            self.metadata.origin.clone(),
            literal(
                DataValue::Text {
                    value: self.metadata.identity.as_str().into(),
                },
                Type::Text,
            ),
        );
        Ok(Statement::ForEach {
            items: Expr::Query {
                entity: self.metadata.root.clone(),
                binding: binding.clone(),
                predicate: Box::new(predicate),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            },
            binding: binding.clone(),
            steps: vec![Statement::Update {
                record: variable(&binding),
                values,
            }],
        })
    }
    fn instrument(
        &mut self,
        steps: &[Statement],
        app: &AppDefinition,
        env: &BTreeMap<Id, Type>,
    ) -> AdmissionResult<Vec<Statement>> {
        let mut env = env.clone();
        let mut out = vec![];
        for original in steps {
            self.meter.charge(1)?;
            let mut step = original.clone();
            let mut seal = None;
            let mut root_update = None;
            match &mut step {
                Statement::Create {
                    entity,
                    bind,
                    values,
                } => {
                    if entity == &self.metadata.root {
                        let defaults = defaults(
                            self.proof,
                            self.active
                                && matches!(
                                    self.proof.request().population,
                                    ScopePopulation::All | ScopePopulation::FutureWork
                                ),
                        )?;
                        for definition in self.proof.storage_fields() {
                            values.insert(
                                definition.id.clone(),
                                literal(
                                    defaults[&definition.id].clone(),
                                    definition.value_type.clone(),
                                ),
                            );
                        }
                    }
                    if self
                        .proof
                        .request()
                        .auxiliary
                        .iter()
                        .any(|a| &a.entity == entity)
                    {
                        if let Some(revision) = &self.metadata.revision {
                            values.insert(
                                revision.clone(),
                                literal(DataValue::Integer { value: 1 }, Type::Integer),
                            );
                        }
                    }
                    env.insert(bind.clone(), Type::reference(entity.clone()));
                }
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let Type::List { item } = app.expression_type_in(items, &env)? else {
                        return Err(refuse("lowering", "non-list iterator"));
                    };
                    let mut nested = env.clone();
                    nested.insert(binding.clone(), *item);
                    *steps = self.instrument(steps, app, &nested)?;
                }
                Statement::Update { record, values }
                    if app.expression_type_in(record, &env)?
                        == Type::reference(&self.metadata.root) =>
                {
                    root_update = Some(record.clone());
                    if let Some(revision) = &self.metadata.revision {
                        values.insert(revision.clone(), next_revision(record.clone(), revision));
                    }
                    // Only the separately certified isolated transition needs a
                    // capture; preserving writers cannot complete a root.
                    let (field, completed) =
                        lifecycle_parts(&self.proof.request().inventory.lifecycle.after)?;
                    if matches!(values.get(field), Some(Expr::Literal{value,..}) if value==completed)
                    {
                        seal = Some(record.clone());
                    }
                }
                Statement::Update { record, values } => {
                    let typ = app.expression_type_in(record, &env)?;
                    if self
                        .proof
                        .request()
                        .auxiliary
                        .iter()
                        .any(|a| typ == Type::reference(&a.entity))
                    {
                        if let Some(revision) = &self.metadata.revision {
                            values
                                .insert(revision.clone(), next_revision(record.clone(), revision));
                        }
                    }
                }
                _ => {}
            }
            if let Some(record) = root_update {
                out.push(self.protect(record, vec![step]));
            } else {
                out.push(step);
            }
            if let Some(root) = seal {
                out.push(self.seal(root)?);
            }
        }
        Ok(out)
    }
}

fn lifecycle_parts(expr: &Expr) -> AdmissionResult<(&str, &DataValue)> {
    match expr {
        Expr::Equal { left, right } => match (left.as_ref(), right.as_ref()) {
            (Expr::Field { field, .. }, Expr::Literal { value, .. })
            | (Expr::Literal { value, .. }, Expr::Field { field, .. }) => Ok((field, value)),
            _ => Err(refuse("lifecycle", "unsupported predicate")),
        },
        _ => Err(refuse("lifecycle", "unsupported predicate")),
    }
}

/// The output field is intentionally absent from this identity. Callers cannot
/// influence the compiler by filling it with a prospective or self-referential ID.
pub(super) fn compile(
    proof: &VerifiedOwnedContinuation,
    manifest: &OwnedCompositionV2,
    active: bool,
    meter: &mut ProofMeter,
) -> AdmissionResult<(CapturedProgram, Metadata)> {
    meter.charge(1)?;
    let current = proof.current();
    if current
        .data
        .records
        .iter()
        .filter(|r| {
            r.entity == proof.layout().root
                || proof
                    .request()
                    .auxiliary
                    .iter()
                    .any(|a| a.entity == r.entity)
        })
        .any(|r| r.revision > (i64::MAX as u64).saturating_sub(10_000_000))
    {
        return Err(refuse(
            "owned revision",
            "insufficient exact counter range for bounded execution",
        ));
    }
    let expected_active = if active {
        BTreeSet::from([proof.layer().clone()])
    } else {
        BTreeSet::new()
    };
    if manifest.version != OWNED_SCOPE_VERSION
        || manifest.lowering != OWNED_TRANSITION_LOWERING_VERSION
        || !matches!(
            manifest.transition,
            ScopeTransition::Evolution | ScopeTransition::Withdrawal
        )
        || manifest.layers != vec![proof.layer().clone()]
        || manifest.active != expected_active
        || manifest.previous.as_ref() != Some(&current.active_revision)
        || manifest.business != revision(proof.after())?
        || manifest.runtime != RUNTIME_VERSION
        || manifest.driver != DRIVER_VERSION
        || manifest.basis.container_format != super::super::FORMAT
        || manifest.basis.family != OwnedBasisFamilyV2::OwnedV2
        || manifest.basis.frozen != super::basis(current)?
        || manifest.basis.graph != current.decisions
        || manifest.basis.artifacts != current.artifacts
        || manifest.basis.sources
            != current
                .programs
                .iter()
                .map(revision)
                .collect::<std::result::Result<Vec<_>, _>>()?
    {
        return Err(refuse(
            "transition compiler",
            "manifest is not the exact verified continuation basis",
        ));
    }
    let state = current.owned_scope()?;
    let predecessor = state
        .adoptions
        .last()
        .and_then(|r| state.compositions.get(&r.composition))
        .ok_or_else(|| refuse("transition compiler", "committed predecessor missing"))?;
    match manifest.transition {
        ScopeTransition::Evolution => {
            let correspondence = OwnedImplementationCorrespondenceV2 {
                version: OWNED_SCOPE_VERSION,
                source: current.active_revision.clone(),
                target: revision(proof.after())?,
                sources: proof.request().inventory.sources.clone(),
            };
            if manifest.active != predecessor.active
                || manifest.correspondences != vec![correspondence]
            {
                return Err(refuse(
                    "transition compiler",
                    "evolution changed activity or exact pair correspondence",
                ));
            }
        }
        ScopeTransition::Withdrawal => {
            let effective = predecessor
                .correspondences
                .first()
                .map(|c| &c.sources)
                .or_else(|| {
                    state
                        .layers
                        .get(proof.layer())
                        .map(|l| &l.request.inventory.sources)
                })
                .ok_or_else(|| refuse("transition compiler", "effective ordinary pair missing"))?;
            if active
                || predecessor.active != BTreeSet::from([proof.layer().clone()])
                || manifest.correspondences != predecessor.correspondences
                || &proof.request().inventory.sources != effective
            {
                return Err(refuse(
                    "transition compiler",
                    "withdrawal changed pair, correspondence or prior activity",
                ));
            }
        }
        ScopeTransition::Adoption => {
            return Err(refuse(
                "transition compiler",
                "second initialization forbidden",
            ))
        }
    }
    let mut metadata = proof.layout().clone();
    metadata.identity = canonical_digest(
        IdentityDomain::Evidence,
        &(
            "owned-transition-compiler/3",
            OWNED_TRANSITION_LOWERING_VERSION,
            (
                &manifest.version,
                &manifest.lowering,
                &manifest.basis,
                &manifest.transition,
                &manifest.business,
                &manifest.layers,
                &manifest.active,
                &manifest.previous,
                &manifest.operation,
                &manifest.runtime,
                &manifest.driver,
                &manifest.correspondences,
            ),
            proof.layer(),
            proof.initialization(),
            proof.before(),
            proof.after(),
            proof.layout(),
            proof.request(),
            proof.report(),
            proof.continuation_identity()?,
        ),
    )?;
    fn names(value: serde_json::Value, set: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::String(s) => {
                set.insert(s);
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    names(v, set)
                }
            }
            serde_json::Value::Object(o) => {
                for (k, v) in o {
                    set.insert(k);
                    names(v, set)
                }
            }
            _ => {}
        }
    }
    let mut used = BTreeSet::new();
    names(
        serde_json::to_value((&proof.before().program, &proof.after().program))?,
        &mut used,
    );
    let mut compiler = Compiler {
        proof,
        active,
        metadata,
        names: used,
        next: 0,
        used_edits: BTreeSet::new(),
        meter,
    };
    let mut app = if active {
        proof.after().program.clone()
    } else {
        proof.before().program.clone()
    };
    // Inactive source has no auxiliary behavior, but its schema must still resume
    // every genuine retained row, with its original owner and revision layout.
    if !active {
        for aux in &proof.request().auxiliary {
            app.entities.push(
                proof
                    .after()
                    .program
                    .entities
                    .iter()
                    .find(|e| e.id == aux.entity)
                    .ok_or_else(|| refuse(&aux.entity, "original auxiliary schema missing"))?
                    .clone(),
            );
        }
    }
    app.entities
        .iter_mut()
        .find(|e| e.id == compiler.metadata.root)
        .ok_or_else(|| refuse("schema", "root missing"))?
        .fields
        .extend(proof.storage_fields().iter().cloned());
    if let Some(revision) = &compiler.metadata.revision {
        for aux in &proof.request().auxiliary {
            app.entities
                .iter_mut()
                .find(|e| e.id == aux.entity)
                .ok_or_else(|| refuse(&aux.entity, "auxiliary missing"))?
                .fields
                .push(FieldDefinition {
                    id: revision.clone(),
                    value_type: Type::Integer,
                    label: "Protected owned revision".into(),
                });
        }
    }
    for (index, p) in proof.projections().iter().enumerate() {
        if !proof.projection_present(p, active) {
            continue;
        }
        let expression = condition(
            field(variable(&p.binding), &compiler.metadata.sealed),
            field(variable(&p.binding), &compiler.metadata.saved[index]),
            proof.capture_expression(p, &p.binding, active)?,
        );
        match &p.destination {
            OwnedProjectionDestination::ViewColumn { view, column } => {
                let ViewKind::List { columns, .. } = &mut app
                    .views
                    .iter_mut()
                    .find(|v| &v.id == view)
                    .ok_or_else(|| refuse(view, "projection view absent"))?
                    .kind
                else {
                    return Err(refuse(view, "projection view kind"));
                };
                columns
                    .iter_mut()
                    .find(|c| &c.id == column)
                    .ok_or_else(|| refuse(column, "projection column absent"))?
                    .value = expression;
            }
            OwnedProjectionDestination::RootCount { observable, .. } => {
                let Expr::Count { items } = &mut app
                    .observables
                    .iter_mut()
                    .find(|o| &o.id == observable)
                    .ok_or_else(|| refuse(observable, "count absent"))?
                    .value
                else {
                    return Err(refuse(observable, "count shape"));
                };
                let Expr::Query { predicate, .. } = items.as_mut() else {
                    return Err(refuse(observable, "count query"));
                };
                *predicate = Box::new(expression);
            }
        }
    }
    // Keep the full query app immutable while deriving all replacements. This
    // avoids a per-action deep clone without skipping any semantic/type query.
    let mut replacements = Vec::new();
    for action in &app.actions {
        let before = proof
            .before()
            .program
            .actions
            .iter()
            .find(|a| a.id == action.id)
            .ok_or_else(|| refuse(&action.id, "baseline action missing"))?;
        let steps = if active {
            compiler.compose(&action.id, &before.steps, &action.steps, &[], &[])?
        } else {
            before.steps.clone()
        };
        replacements.push(compiler.instrument(&steps, &app, &action.parameters)?);
    }
    for (action, steps) in app.actions.iter_mut().zip(replacements) {
        action.steps = steps;
    }
    if active && compiler.used_edits.len() != proof.request().inventory.sources.edits.len() {
        return Err(refuse("lowering", "not every logical endpoint consumed"));
    }
    compiler.meter.charge(canonical_bytes(&app)?.len() as u64)?;
    app.validate()?;
    let bytes = canonical_bytes(&app)?;
    // Capture performs the second complete parsed-App semantic validation.
    let target = CapturedProgram::capture(
        &bytes,
        &current.data.project_id,
        Producer::ExternalAuthor {
            description: format!(
                "Verified host owned scope composition v3: {}",
                compiler.metadata.identity.as_str()
            ),
        },
        None,
    )?;
    if target.program.entities.len() != current.data.schema.len()
        || target
            .program
            .entities
            .iter()
            .any(|e| current.data.schema.iter().find(|s| s.id == e.id) != Some(e))
    {
        return Err(refuse(
            "transition compiler",
            "identity current-data schema does not exactly resume",
        ));
    }
    Ok((target, compiler.metadata))
}
