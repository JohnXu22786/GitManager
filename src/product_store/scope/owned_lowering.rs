//! Deterministic compilation. The opaque reconstructed proof is the sole input;
//! no serialized phase inventory or caller-authored target can enter this path.
use super::common_fact_admission::{refuse, AdmissionResult, ProofMeter};
use super::*;
use serde::Serialize;

const LOWERING_VERSION: u32 = 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct Metadata {
    pub identity: Digest,
    pub root: Id,
    pub member: Id,
    pub sealed: Id,
    pub day: Id,
    pub origin: Id,
    pub saved: Vec<Id>,
    /// Owned-v2 authenticates exact root revisions despite event coalescing.
    pub revision: Option<Id>,
}
pub(super) fn literal(value: DataValue, value_type: Type) -> Expr {
    Expr::Literal { value, value_type }
}
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
/// A non-capture placeholder with the exact scalar/list type. Sealing and map-key
/// presence authenticate capture; placeholders are never read as history.
pub(super) fn empty(typ: &Type) -> AdmissionResult<DataValue> {
    Ok(match typ {
        Type::Boolean => DataValue::Boolean { value: false },
        Type::Text => DataValue::Text {
            value: String::new(),
        },
        Type::Integer => DataValue::Integer { value: 0 },
        Type::Date => DataValue::Date { days: 0 },
        Type::Optional { .. } => DataValue::Null,
        Type::List { item } => DataValue::List {
            item_type: *item.clone(),
            items: vec![],
        },
        _ => {
            return Err(refuse(
                "projection",
                "required reference has no non-capture placeholder",
            ))
        }
    })
}
/// Capture-avoiding free-variable substitution. Query/fold binders scope only
/// their body, never their input. New host names are fresh across both sources.
pub(super) fn rebind(expr: &Expr, from: &str, to: &str) -> Expr {
    let mut result = expr.clone();
    fn visit(e: &mut Expr, from: &str, to: &str) {
        match e {
            Expr::Variable { name } if name == from => *name = to.into(),
            Expr::Literal { .. } | Expr::Variable { .. } | Expr::State { .. } | Expr::Today => {}
            Expr::Field { record, .. } => visit(record, from, to),
            Expr::Query {
                binding,
                predicate,
                sort,
                ..
            } => {
                if binding != from {
                    visit(predicate, from, to);
                    for key in sort {
                        visit(&mut key.value, from, to);
                    }
                }
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            }
            | Expr::Any {
                items,
                binding,
                predicate,
            }
            | Expr::All {
                items,
                binding,
                predicate,
            } => {
                visit(items, from, to);
                if binding != from {
                    visit(predicate, from, to);
                }
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                visit(items, from, to);
                if binding != from {
                    visit(value, from, to);
                }
            }
            Expr::Count { items } | Expr::Sum { items } => visit(items, from, to),
            Expr::Contains { items, value } => {
                visit(items, from, to);
                visit(value, from, to);
            }
            Expr::Equal { left, right }
            | Expr::Less { left, right }
            | Expr::Add { left, right }
            | Expr::Subtract { left, right }
            | Expr::Concat { left, right } => {
                visit(left, from, to);
                visit(right, from, to);
            }
            Expr::TextContains { text, search } => {
                visit(text, from, to);
                visit(search, from, to);
            }
            Expr::Lower { value } | Expr::Not { value } => visit(value, from, to),
            Expr::And { values } | Expr::Or { values } => {
                for value in values {
                    visit(value, from, to);
                }
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                visit(condition, from, to);
                visit(then_value, from, to);
                visit(else_value, from, to);
            }
            Expr::Coalesce { value, fallback } => {
                visit(value, from, to);
                visit(fallback, from, to);
            }
            Expr::DateAdd { date, days } => {
                visit(date, from, to);
                visit(days, from, to);
            }
            Expr::DateDifference { later, earlier } => {
                visit(later, from, to);
                visit(earlier, from, to);
            }
        }
    }
    visit(&mut result, from, to);
    result
}
impl Metadata {
    fn new(proof: &VerifiedOwnedPreviewPlan, owned: Option<&Digest>) -> AdmissionResult<Self> {
        let preview_identity = canonical_digest(
            IdentityDomain::Evidence,
            &(
                "owned-preview-compiler/1",
                LOWERING_VERSION,
                RUNTIME_VERSION,
                DRIVER_VERSION,
                &proof.snapshot,
                &proof.candidate,
                &proof.request,
                &proof.report,
                &proof.captured,
            ),
        )?;
        let identity = match owned {
            Some(layer) => canonical_digest(
                IdentityDomain::Evidence,
                &(
                    "owned-scope-compiler/2",
                    OWNED_LOWERING_VERSION,
                    layer,
                    &preview_identity,
                ),
            )?,
            None => preview_identity,
        };
        let prefix = format!("gm_scope_op_{}", identity.as_str());
        // Identical typed expressions at the same capture boundary share one
        // physical cell. This is compiler common-expression elimination, never
        // a cached outcome: every root and every completion still evaluates it.
        let mut slots = BTreeMap::new();
        let mut saved = Vec::new();
        for (i, recipe) in proof.report.projections.iter().enumerate() {
            let key = canonical_bytes(&(
                &recipe.binding,
                &recipe.value_type,
                &recipe.host_value_type,
                &recipe.baseline,
                &recipe.candidate,
                &recipe.absent_value,
            ))?;
            let index = *slots.entry(key).or_insert(i);
            saved.push(format!("{prefix}_p{index}"));
        }
        Ok(Self {
            identity,
            root: proof.request.inventory.sources.root_entity.clone(),
            member: format!("{prefix}_member"),
            sealed: format!("{prefix}_sealed"),
            day: format!("{prefix}_day"),
            origin: format!("{prefix}_origin"),
            saved,
            revision: owned.map(|_| format!("{prefix}_revision")),
        })
    }
    pub(super) fn fields(&self, proof: &VerifiedOwnedPreviewPlan) -> Vec<FieldDefinition> {
        let mut fields = vec![
            (self.member.clone(), Type::Boolean),
            (self.sealed.clone(), Type::Boolean),
            (
                self.day.clone(),
                Type::Optional {
                    item: Box::new(Type::Date),
                },
            ),
            (
                self.origin.clone(),
                Type::Optional {
                    item: Box::new(Type::Text),
                },
            ),
        ];
        if let Some(revision) = &self.revision {
            fields.push((revision.clone(), Type::Integer));
        }
        fields.extend(
            self.saved
                .iter()
                .zip(&proof.report.projections)
                .map(|(id, p)| (id.clone(), p.host_value_type.clone())),
        );
        let mut seen = BTreeSet::new();
        fields
            .into_iter()
            .filter(|(id, _)| seen.insert(id.clone()))
            .map(|(id, value_type)| FieldDefinition {
                id,
                value_type,
                label: "Protected preview evidence".into(),
            })
            .collect()
    }
    pub(super) fn defaults(
        &self,
        proof: &VerifiedOwnedPreviewPlan,
        member: bool,
    ) -> AdmissionResult<Values> {
        let mut values = Values::from([
            (self.member.clone(), DataValue::Boolean { value: member }),
            (self.sealed.clone(), DataValue::Boolean { value: false }),
            (self.day.clone(), DataValue::Null),
            (self.origin.clone(), DataValue::Null),
        ]);
        if let Some(revision) = &self.revision {
            values.insert(revision.clone(), DataValue::Integer { value: 1 });
        }
        for (key, p) in self.saved.iter().zip(&proof.report.projections) {
            values.insert(key.clone(), empty(&p.host_value_type)?);
        }
        Ok(values)
    }
    pub(super) fn live(
        &self,
        recipe: &OwnedProjectionRecipe,
        binding: &str,
    ) -> AdmissionResult<Expr> {
        let mut after = rebind(&recipe.candidate, &recipe.binding, binding);
        if recipe.host_value_type != recipe.value_type {
            if recipe.value_type != Type::Boolean
                || recipe.host_value_type
                    != (Type::Optional {
                        item: Box::new(Type::Boolean),
                    })
            {
                return Err(refuse(&recipe.location, "unsupported nullable lift"));
            }
            after = condition(
                after,
                literal(
                    DataValue::Boolean { value: true },
                    recipe.host_value_type.clone(),
                ),
                literal(
                    DataValue::Boolean { value: false },
                    recipe.host_value_type.clone(),
                ),
            );
        }
        let before = match &recipe.baseline {
            Some(e) => rebind(e, &recipe.binding, binding),
            None => literal(
                recipe
                    .absent_value
                    .clone()
                    .ok_or_else(|| refuse(&recipe.location, "missing explicit absence"))?,
                recipe.host_value_type.clone(),
            ),
        };
        Ok(condition(
            field(variable(binding), &self.member),
            after,
            before,
        ))
    }
    fn next_revision(&self, record: Expr, key: &str) -> Expr {
        Expr::Add {
            left: Box::new(field(record, key)),
            right: Box::new(literal(DataValue::Integer { value: 1 }, Type::Integer)),
        }
    }
    fn projection(&self, recipe: &OwnedProjectionRecipe, index: usize) -> AdmissionResult<Expr> {
        Ok(condition(
            field(variable(&recipe.binding), &self.sealed),
            field(variable(&recipe.binding), &self.saved[index]),
            self.live(recipe, &recipe.binding)?,
        ))
    }
}
struct Compiler<'a> {
    proof: &'a VerifiedOwnedPreviewPlan,
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
                if member {
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
                .request
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
                    &self.proof.request.inventory.lifecycle.after,
                    "record",
                    &binding,
                ),
            ],
        };
        let mut values = BTreeMap::new();
        for (i, p) in self.proof.report.projections.iter().enumerate() {
            values.insert(
                self.metadata.saved[i].clone(),
                self.metadata.live(p, &binding)?,
            );
        }
        if let Some(revision) = &self.metadata.revision {
            values.insert(
                revision.clone(),
                self.metadata.next_revision(variable(&binding), revision),
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
            match &mut step {
                Statement::Create {
                    entity,
                    bind,
                    values,
                } => {
                    if entity == &self.metadata.root {
                        let defaults = self.metadata.defaults(
                            self.proof,
                            matches!(
                                self.proof.request.population,
                                ScopePopulation::All | ScopePopulation::FutureWork
                            ),
                        )?;
                        for definition in self.metadata.fields(self.proof) {
                            values.insert(
                                definition.id.clone(),
                                literal(defaults[&definition.id].clone(), definition.value_type),
                            );
                        }
                    }
                    if self
                        .proof
                        .request
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
                    if let Some(revision) = &self.metadata.revision {
                        values.insert(
                            revision.clone(),
                            self.metadata.next_revision(record.clone(), revision),
                        );
                    }
                    // Only the separately certified isolated transition needs a
                    // capture; preserving writers cannot complete a root.
                    let (field, completed) =
                        lifecycle_parts(&self.proof.request.inventory.lifecycle.after)?;
                    if matches!(values.get(field), Some(Expr::Literal{value,..}) if value==completed)
                    {
                        seal = Some(record.clone());
                    }
                }
                Statement::Update { record, values } => {
                    let typ = app.expression_type_in(record, &env)?;
                    if self
                        .proof
                        .request
                        .auxiliary
                        .iter()
                        .any(|a| typ == Type::reference(&a.entity))
                    {
                        if let Some(revision) = &self.metadata.revision {
                            values.insert(
                                revision.clone(),
                                self.metadata.next_revision(record.clone(), revision),
                            );
                        }
                    }
                }
                _ => {}
            }
            out.push(step);
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
pub(super) fn completed(proof: &VerifiedOwnedPreviewPlan, row: &Record) -> AdmissionResult<bool> {
    let (field, value) = lifecycle_parts(&proof.request.inventory.lifecycle.before)?;
    Ok(row.values.get(field) == Some(value))
}
pub(super) fn compile(
    proof: &VerifiedOwnedPreviewPlan,
    meter: &mut ProofMeter,
) -> AdmissionResult<(CapturedProgram, Metadata)> {
    compile_inner(proof, meter, None)
}
pub(super) fn compile_owned(
    proof: &VerifiedOwnedPreviewPlan,
    layer: &Digest,
    meter: &mut ProofMeter,
) -> AdmissionResult<(CapturedProgram, Metadata)> {
    if proof
        .snapshot
        .data
        .records
        .iter()
        .filter(|r| r.entity == proof.request.inventory.sources.root_entity)
        .any(|r| r.revision > (i64::MAX as u64).saturating_sub(10_000_000))
    {
        return Err(refuse(
            "owned root revision",
            "insufficient exact-counter range for bounded execution",
        ));
    }
    compile_inner(proof, meter, Some(layer))
}
fn compile_inner(
    proof: &VerifiedOwnedPreviewPlan,
    meter: &mut ProofMeter,
    owned: Option<&Digest>,
) -> AdmissionResult<(CapturedProgram, Metadata)> {
    meter.charge(1)?;
    let metadata = Metadata::new(proof, owned)?;
    // String collection is conservative: it includes every lexical name plus
    // all other authored strings, so an internal binder cannot capture one.
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
        serde_json::to_value((&proof.snapshot.program()?.program, &proof.candidate.program))?,
        &mut used,
    );
    let mut compiler = Compiler {
        proof,
        metadata,
        names: used,
        next: 0,
        used_edits: BTreeSet::new(),
        meter,
    };
    let mut app = proof.candidate.program.clone();
    let root = app
        .entities
        .iter_mut()
        .find(|e| e.id == compiler.metadata.root)
        .ok_or_else(|| refuse("schema", "root missing"))?;
    root.fields.extend(compiler.metadata.fields(proof));
    if let Some(revision) = &compiler.metadata.revision {
        for auxiliary in &proof.request.auxiliary {
            app.entities
                .iter_mut()
                .find(|e| e.id == auxiliary.entity)
                .ok_or_else(|| refuse("schema", "auxiliary entity missing"))?
                .fields
                .push(FieldDefinition {
                    id: revision.clone(),
                    value_type: Type::Integer,
                    label: "Protected owned revision".into(),
                });
        }
    }
    for (index, recipe) in proof.report.projections.iter().enumerate() {
        let expr = compiler.metadata.projection(recipe, index)?;
        match &recipe.destination {
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
                    .value = expr;
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
                *predicate = Box::new(expr);
            }
        }
    }
    let typed = app.clone();
    for action in &mut app.actions {
        let before = proof
            .snapshot
            .program()?
            .program
            .actions
            .iter()
            .find(|a| a.id == action.id)
            .ok_or_else(|| refuse(&action.id, "missing baseline action"))?;
        let steps = compiler.compose(&action.id, &before.steps, &action.steps, &[], &[])?;
        action.steps = compiler.instrument(&steps, &typed, &action.parameters)?;
    }
    if compiler.used_edits.len() != proof.request.inventory.sources.edits.len() {
        return Err(refuse("lowering", "not every source endpoint was consumed"));
    }
    compiler.meter.charge(canonical_bytes(&app)?.len() as u64)?;
    app.validate()?;
    let target = CapturedProgram::capture(
        &canonical_bytes(&app)?,
        &proof.snapshot.data.project_id,
        Producer::ExternalAuthor {
            description: match owned {
                Some(_) => format!(
                    "Verified host owned scope composition v{OWNED_LOWERING_VERSION}: {}",
                    compiler.metadata.identity.as_str()
                ),
                None => format!(
                    "Verified host owned preview compiler v{LOWERING_VERSION}: {}",
                    compiler.metadata.identity.as_str()
                ),
            },
        },
        None,
    )?;
    Ok((target, compiler.metadata))
}
