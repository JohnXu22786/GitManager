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
pub(super) fn inherited_state_only_append(proof: &VerifiedOwnedPreviewPlan) -> bool {
    if !matches!(&proof.snapshot.scope, ScopeEnvelope::OwnedV2(state) if !state.layers.is_empty()) {
        return false;
    }
    let before = &proof.baseline.program;
    let after = &proof.candidate.program;
    if before.version != after.version
        || before.id != after.id
        || before.label != after.label
        || before.entities != after.entities
        || before.state != after.state
        || before.outputs != after.outputs
        || before.observables != after.observables
        || before.views != after.views
        || before.initial_view != after.initial_view
        || before.actions.len() != after.actions.len()
    {
        return false;
    }
    let mut found = false;
    for (old, new) in before.actions.iter().zip(&after.actions) {
        if old.id != new.id || old.label != new.label || old.parameters != new.parameters
            || old.guards != new.guards || old.ensures != new.ensures
        {
            return false;
        }
        if old.steps == new.steps { continue; }
        if new.steps.len() <= old.steps.len() || !new.steps.starts_with(&old.steps)
            || new.steps[old.steps.len()..].iter().any(|step| !matches!(step, Statement::SetState { .. })) { return false; }
        found = true;
    }
    found && inherited_active_metadata_prefix(proof).is_some()
}
fn inherited_completion_semantics_match<'a>(
    mut inherited: impl Iterator<Item = &'a Expr>,
    current: &Expr,
) -> bool {
    inherited.all(|semantics| semantics == current)
}
fn retained_projection_saved_field(expr: &Expr, binding: &str) -> Option<Id> {
    let Expr::If { condition, then_value, .. } = expr else { return None; };
    let Expr::Field { record: condition_record, field: sealed } = condition.as_ref() else { return None; };
    let Expr::Variable { name: condition_binding } = condition_record.as_ref() else { return None; };
    let Expr::Field { record: saved_record, field: saved } = then_value.as_ref() else { return None; };
    let Expr::Variable { name: saved_binding } = saved_record.as_ref() else { return None; };
    let prefix = sealed.strip_suffix("_sealed")?;
    let (saved_prefix, slot) = saved.rsplit_once("_p")?;
    if condition_binding != binding
        || saved_binding != binding
        || !prefix.starts_with(RESERVED_PREFIX)
        || prefix != saved_prefix
        || slot.is_empty()
        || !slot.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(saved.clone())
}
fn retained_projection_revision_field(proof: &VerifiedOwnedPreviewPlan) -> Option<Id> {
    if !matches!(&proof.snapshot.scope, ScopeEnvelope::OwnedV2(state) if !state.layers.is_empty()) {
        return None;
    }
    for projection in &proof.report.projections {
        let Some(expr) = projection.baseline.as_ref() else { continue; };
        let Some(saved) = retained_projection_saved_field(expr, &projection.binding) else { continue; };
        let (prefix, _) = saved.rsplit_once("_p")?;
        let revision = format!("{prefix}_revision");
        let root = proof.baseline.program.entities.iter().find(|e| e.id == proof.request.inventory.sources.root_entity)?;
        if root.fields.iter().any(|field| field.id == revision && field.value_type == Type::Integer) {
            return Some(revision);
        }
    }
    None
}
fn inherited_active_metadata_prefix(proof: &VerifiedOwnedPreviewPlan) -> Option<String> {
    if proof.report.projections.is_empty() { return None; }
    let mut prefix: Option<String> = None;
    for projection in &proof.report.projections {
        if projection.baseline.as_ref() != Some(&projection.candidate) { return None; }
        let expr = projection.baseline.as_ref()?;
        let saved = retained_projection_saved_field(expr, &projection.binding)?;
        let current = saved.rsplit_once("_p")?.0.to_owned();
        if prefix.as_ref().is_some_and(|expected| expected != &current) { return None; }
        prefix = Some(current);
    }
    let prefix = prefix?;
    let root = proof.baseline.program.entities.iter()
        .find(|entity| entity.id == proof.request.inventory.sources.root_entity)?;
    let fields: BTreeMap<_, _> = root.fields.iter().map(|field| (field.id.as_str(), &field.value_type)).collect();
    let day_type = Type::Optional { item: Box::new(Type::Date) };
    let origin_type = Type::Optional { item: Box::new(Type::Text) };
    [
        (format!("{prefix}_member"), Type::Boolean),
        (format!("{prefix}_sealed"), Type::Boolean),
        (format!("{prefix}_day"), day_type),
        (format!("{prefix}_origin"), origin_type),
        (format!("{prefix}_revision"), Type::Integer),
    ].iter().all(|(id, ty)| fields.get(id.as_str()).is_some_and(|actual| **actual == *ty))
        .then_some(prefix)
}
/// A state-only continuation reuses the already authenticated capture cells.
/// Its protected origin must therefore remain the identity that produced
/// those cells. Recover it from the exact generated field prefix and require
/// every row's seal/origin pair to agree; the caller has already authenticated
/// this snapshot against the committed OwnedV2 registry.
fn inherited_active_metadata_identity(proof: &VerifiedOwnedPreviewPlan) -> AdmissionResult<Digest> {
    let prefix = inherited_active_metadata_prefix(proof)
        .ok_or_else(|| refuse("composition", "inherited OwnedV2 capture prefix is not exact"))?;
    let operation_prefix = format!("{RESERVED_PREFIX}op_");
    let raw = prefix.strip_prefix(&operation_prefix)
        .ok_or_else(|| refuse("composition", "inherited OwnedV2 identity prefix is invalid"))?;
    let identity = Digest::from_owned_lower_hex(raw.to_owned())
        .ok_or_else(|| refuse("composition", "inherited OwnedV2 identity is invalid"))?;
    for row in proof.snapshot.data.records.iter()
        .filter(|row| row.entity == proof.request.inventory.sources.root_entity)
    {
        let sealed = match row.values.get(&format!("{prefix}_sealed")) {
            Some(DataValue::Boolean { value }) => *value,
            _ => return Err(refuse("composition", "inherited seal is missing or invalid")),
        };
        match (sealed, row.values.get(&format!("{prefix}_origin"))) {
            (true, Some(DataValue::Text { value })) if value == identity.as_str() => {}
            (false, Some(DataValue::Null)) => {}
            _ => return Err(refuse("composition", "inherited seal/origin provenance differs")),
        }
    }
    Ok(identity)
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
                &proof.baseline,
                &proof.candidate,
                &proof.request,
                &proof.report,
                &proof.captured,
            ),
        )?;
        let generated_identity = match owned {
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
        let inherited_noop = inherited_state_only_append(proof);
        let identity = if inherited_noop {
            inherited_active_metadata_identity(proof)?
        } else {
            generated_identity
        };
        let prefix = if inherited_noop {
            inherited_active_metadata_prefix(proof)
                .ok_or_else(|| refuse("composition", "inherited OwnedV2 capture prefix is not exact"))?
        } else {
            format!("gm_scope_op_{}", identity.as_str())
        };
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
            let retained = if matches!(&proof.snapshot.scope, ScopeEnvelope::OwnedV2(state) if !state.layers.is_empty())
                && recipe.baseline.as_ref().is_some_and(|baseline| baseline == &recipe.candidate)
            {
                recipe.baseline.as_ref()
                    .and_then(|baseline| retained_projection_saved_field(baseline, &recipe.binding))
            } else {
                None
            };
            saved.push(retained.unwrap_or_else(|| format!("{prefix}_p{index}")));
        }
        Ok(Self {
            identity,
            root: proof.request.inventory.sources.root_entity.clone(),
            member: format!("{prefix}_member"),
            sealed: format!("{prefix}_sealed"),
            day: format!("{prefix}_day"),
            origin: format!("{prefix}_origin"),
            saved,
            revision: owned.map(|_| retained_projection_revision_field(proof)
                .unwrap_or_else(|| format!("{prefix}_revision"))),
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
    fn reuses_saved(&self, proof: &VerifiedOwnedPreviewPlan, index: usize) -> bool {
        let Some(recipe) = proof.report.projections.get(index) else { return false; };
        recipe.baseline.as_ref().is_some_and(|baseline| baseline == &recipe.candidate)
            && recipe.baseline.as_ref()
                .and_then(|baseline| retained_projection_saved_field(baseline, &recipe.binding))
                .as_ref() == self.saved.get(index)
    }
    fn reuses_revision(&self, proof: &VerifiedOwnedPreviewPlan) -> bool {
        self.revision.as_ref().is_some_and(|revision|
            retained_projection_revision_field(proof).as_ref() == Some(revision))
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
                    (None, None) if old.is_empty() && new.len() == 1
                        && matches!(new[0], Statement::SetState { .. }) =>
                    {
                        out.push(new[0].clone());
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
            if self.metadata.reuses_saved(self.proof, i) {
                continue;
            }
            values.insert(
                self.metadata.saved[i].clone(),
                self.metadata.live(p, &binding)?,
            );
        }
        if !self.metadata.reuses_revision(self.proof) {
            if let Some(revision) = &self.metadata.revision {
            values.insert(
                revision.clone(),
                self.metadata.next_revision(variable(&binding), revision),
            );
            }
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
    fn seal_inherited_completed(&mut self) -> AdmissionResult<Statement> {
        let binding = self.fresh();
        let predicate = Expr::And {
            values: vec![
                not(field(variable(&binding), &self.metadata.sealed)),
                field(variable(&binding), &self.metadata.member),
                rebind(
                    &self.proof.request.inventory.lifecycle.after,
                    "record",
                    &binding,
                ),
            ],
        };
        let mut values = BTreeMap::new();
        for (index, projection) in self.proof.report.projections.iter().enumerate() {
            if self.metadata.reuses_saved(self.proof, index) {
                continue;
            }
            values.insert(
                self.metadata.saved[index].clone(),
                self.metadata.live(projection, &binding)?,
            );
        }
        if !self.metadata.reuses_revision(self.proof) {
            if let Some(revision) = &self.metadata.revision {
            values.insert(
                revision.clone(),
                self.metadata.next_revision(variable(&binding), revision),
            );
            }
        }
        values.insert(self.metadata.sealed.clone(), boolean(true));
        values.insert(self.metadata.day.clone(), Expr::Today);
        values.insert(
            self.metadata.origin.clone(),
            literal(
                DataValue::Text { value: self.metadata.identity.as_str().into() },
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
            steps: vec![Statement::Update { record: variable(&binding), values }],
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

    /// Extend a previously authenticated OwnedV2 action without regenerating
    /// its existing instrumentation. The earlier layer remains the authority
    /// for its exact statements; this pass adds only this layer's fields,
    /// revision tracking, and matching completion capture.
    fn instrument_inherited(
        &mut self,
        steps: &[Statement],
        app: &AppDefinition,
        env: &BTreeMap<Id, Type>,
    ) -> AdmissionResult<Vec<Statement>> {
        let mut env = env.clone();
        let mut out = Vec::with_capacity(steps.len());
        for original in steps {
            self.meter.charge(1)?;
            let mut step = original.clone();
            match &mut step {
                Statement::Create { entity, bind, values } => {
                    if entity == &self.metadata.root {
                        let defaults = self.metadata.defaults(
                            self.proof,
                            matches!(self.proof.request.population,
                                ScopePopulation::All | ScopePopulation::FutureWork),
                        )?;
                        for definition in self.metadata.fields(self.proof) {
                            values.insert(definition.id.clone(), literal(
                                defaults[&definition.id].clone(), definition.value_type,
                            ));
                        }
                    }
                    if self.proof.request.auxiliary.iter().any(|a| &a.entity == entity) {
                        if let Some(revision) = &self.metadata.revision {
                            values.insert(revision.clone(), literal(DataValue::Integer { value: 1 }, Type::Integer));
                        }
                    }
                    env.insert(bind.clone(), Type::reference(entity.clone()));
                }
                Statement::ForEach { items, binding, steps } => {
                    let Type::List { item } = app.expression_type_in(items, &env)? else {
                        return Err(refuse("lowering", "non-list inherited iterator"));
                    };
                    let mut nested = env.clone();
                    nested.insert(binding.clone(), *item);
                    *steps = self.instrument_inherited(steps, app, &nested)?;
                }
                Statement::Update { record, values }
                    if app.expression_type_in(record, &env)?
                        == Type::reference(&self.metadata.root) =>
                {
                    if let Some(revision) = &self.metadata.revision {
                        values.insert(revision.clone(), self.metadata.next_revision(record.clone(), revision));
                    }
                }
                Statement::Update { record, values } => {
                    let typ = app.expression_type_in(record, &env)?;
                    if self.proof.request.auxiliary.iter().any(|a| typ == Type::reference(&a.entity)) {
                        if let Some(revision) = &self.metadata.revision {
                            values.insert(revision.clone(), self.metadata.next_revision(record.clone(), revision));
                        }
                    }
                }
                _ => {}
            }
            out.push(step);
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
    let inherited_owned = owned.is_some()
        && proof.baseline.program.entities.iter().any(|entity|
            entity.fields.iter().any(|field| field.id.starts_with(RESERVED_PREFIX)));
    if inherited_owned {
        if let ScopeEnvelope::OwnedV2(state) = &proof.snapshot.scope {
            if !inherited_completion_semantics_match(
                state.layers.values().map(|layer| &layer.request.inventory.lifecycle.after),
                &proof.request.inventory.lifecycle.after,
            ) {
                return Err(refuse("composition", "inherited OwnedV2 layers have conflicting completion semantics"));
            }
        } else {
            return Err(refuse("composition", "generated OwnedV2 source lacks an OwnedV2 registry"));
        }
    }
    if inherited_state_only_append(proof) {
        let bytes = canonical_bytes(&proof.candidate.program)?;
        meter.charge(bytes.len() as u64)?;
        let target = CapturedProgram::capture(
            &bytes,
            &proof.snapshot.data.project_id,
            Producer::ExternalAuthor {
                description: format!(
                    "Verified host owned scope composition v{OWNED_LOWERING_VERSION}: {}",
                    metadata.identity.as_str()
                ),
            },
            None,
        )?;
        return Ok((target, metadata));
    }
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
        serde_json::to_value((&proof.baseline.program, &proof.candidate.program))?,
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
    for field in compiler.metadata.fields(proof) {
        if !root.fields.iter().any(|existing| existing.id == field.id) {
            root.fields.push(field);
        }
    }
    if let Some(revision) = &compiler.metadata.revision {
        for auxiliary in &proof.request.auxiliary {
            let entity = app.entities
                .iter_mut()
                .find(|e| e.id == auxiliary.entity)
                .ok_or_else(|| refuse("schema", "auxiliary entity missing"))?;
            if !entity.fields.iter().any(|field| field.id == *revision) {
                entity.fields.push(FieldDefinition {
                    id: revision.clone(),
                    value_type: Type::Integer,
                    label: "Protected owned revision".into(),
                });
            }
        }
    }
    for (index, recipe) in proof.report.projections.iter().enumerate() {
        if inherited_owned
            && recipe.baseline.as_ref().is_some_and(|baseline| baseline == &recipe.candidate)
            && recipe.baseline.as_ref()
                .and_then(|baseline| retained_projection_saved_field(baseline, &recipe.binding))
                .as_ref() == Some(&compiler.metadata.saved[index])
        {
            continue;
        }
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
            .baseline
            .program
            .actions
            .iter()
            .find(|a| a.id == action.id)
            .ok_or_else(|| refuse(&action.id, "missing baseline action"))?;
        let steps = compiler.compose(&action.id, &before.steps, &action.steps, &[], &[])?;
        if inherited_owned {
            let inherited_len = before.steps.len();
            if steps.len() < inherited_len {
                return Err(refuse(&action.id, "inherited action prefix was truncated"));
            }
            let mut layered = compiler.instrument_inherited(&steps[..inherited_len], &typed, &action.parameters)?;
            layered.extend(compiler.instrument(&steps[inherited_len..], &typed, &action.parameters)?);
            layered.push(compiler.seal_inherited_completed()?);
            action.steps = layered;
        } else {
            action.steps = compiler.instrument(&steps, &typed, &action.parameters)?;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn lifecycle(value: &str) -> Expr {
        Expr::Equal {
            left: Box::new(Expr::Field {
                record: Box::new(Expr::Variable { name: "record".into() }),
                field: "status".into(),
            }),
            right: Box::new(Expr::Literal {
                value_type: Type::Text,
                value: DataValue::Text { value: value.into() },
            }),
        }
    }

    #[test]
    fn inherited_completion_semantics_match_before_state_only_append() {
        let original = lifecycle("done");
        let conflicting = lifecycle("closed");
        assert!(inherited_completion_semantics_match([&original].into_iter(), &original));
        assert!(!inherited_completion_semantics_match([&original].into_iter(), &conflicting));
        assert!(inherited_completion_semantics_match(std::iter::empty(), &conflicting));
    }
}
