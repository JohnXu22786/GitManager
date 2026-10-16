//! Closed typed whole-pair proof. Authority is attached to exact sites; no old
//! analyzer flag, field-taint erasure, or caller-supplied inventory is consumed.
use super::common_fact_admission::{refuse, AdmissionResult, ProofMeter};
use super::*;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
enum Kind {
    Root,
    Aux {
        owner: Id,
    },
    Link {
        owner: Id,
    },
    Registry {
        key: Id,
    },
    History {
        owner: Option<Id>,
        sequence: Id,
        archived: bool,
    },
}
impl Kind {
    fn owner(&self) -> Option<&str> {
        match self {
            Self::Aux { owner } | Self::Link { owner } => Some(owner),
            Self::History { owner, .. } => owner.as_deref(),
            _ => None,
        }
    }
    fn auxiliary(&self) -> bool {
        matches!(self, Self::Aux { .. })
    }
}
#[derive(Clone)]
struct Value {
    ty: Type,
    root: Option<String>,
    active_root: bool,
    policy: BTreeSet<String>,
    facets: BTreeSet<String>,
    private_identity: bool,
    frame_dependent: bool,
    session_dependent: bool,
}
impl Value {
    fn plain(ty: Type) -> Self {
        Self {
            ty,
            root: None,
            active_root: false,
            policy: BTreeSet::new(),
            facets: BTreeSet::new(),
            private_identity: false,
            frame_dependent: false,
            session_dependent: false,
        }
    }
    fn merge(&mut self, other: &Self) {
        self.policy.extend(other.policy.iter().cloned());
        self.facets.extend(other.facets.iter().cloned());
        self.private_identity |= other.private_identity;
        self.frame_dependent |= other.frame_dependent;
        self.session_dependent |= other.session_dependent
    }
    fn item(&self) -> AdmissionResult<Self> {
        let Type::List { item } = &self.ty else {
            return Err(refuse("expression", "expected list"));
        };
        let mut v = self.clone();
        v.ty = *item.clone();
        v.policy.clear();
        v.facets.clear();
        Ok(v)
    }
}
type Env = BTreeMap<Id, Value>;
struct Rules<'a> {
    app: &'a AppDefinition,
    inventory: &'a CommonFactInventory,
    kinds: &'a BTreeMap<Id, Kind>,
    policy: &'a BTreeSet<(Id, Id)>,
    lifecycle: (&'a str, &'a DataValue),
    side: OwnedSourceSide,
    meter: &'a mut ProofMeter,
    certs: Vec<CommonFactCertificate>,
    discovered: BTreeSet<(Id, Id)>,
    all_roots_active: bool,
}
fn literal(expr: &Expr) -> Option<&DataValue> {
    if let Expr::Literal { value, .. } = expr {
        Some(value)
    } else {
        None
    }
}
fn truth(expr: &Expr) -> bool {
    literal(expr) == Some(&DataValue::Boolean { value: true })
}
fn variable(expr: &Expr, name: &str) -> bool {
    matches!(expr,Expr::Variable{name:n} if n==name)
}
fn lifecycle(predicate: &Expr) -> AdmissionResult<(&str, &DataValue)> {
    let Expr::Equal { left, right } = predicate else {
        return Err(refuse(
            "lifecycle",
            "only typed own-row field equals literal is supported",
        ));
    };
    let Expr::Field { record, field } = left.as_ref() else {
        return Err(refuse("lifecycle", "completion receiver is not own row"));
    };
    if !variable(record, "record") {
        return Err(refuse(
            "lifecycle",
            "completion receiver is not exact record binder",
        ));
    }
    let Some(value) = literal(right) else {
        return Err(refuse("lifecycle", "completion value must be a literal"));
    };
    if !matches!(value, DataValue::Text { .. } | DataValue::Boolean { .. }) {
        return Err(refuse("lifecycle", "unsupported lifecycle type"));
    }
    Ok((field, value))
}
fn bookkeeping(expr: &Expr) -> Option<(&str, &str, bool)> {
    let Expr::Add { left, right } = expr else {
        return None;
    };
    if literal(right) != Some(&DataValue::Integer { value: 1 }) {
        return None;
    }
    let Expr::Count { items } = left.as_ref() else {
        return None;
    };
    let Expr::Query {
        entity,
        binding,
        predicate,
        sort,
        limit,
        include_archived,
    } = items.as_ref()
    else {
        return None;
    };
    if !truth(predicate) || !sort.is_empty() || *limit != MAX_COLLECTION {
        return None;
    }
    Some((entity, binding, *include_archived))
}
fn all_statements<'a>(steps: &'a [Statement], out: &mut Vec<&'a Statement>) {
    for step in steps {
        out.push(step);
        if let Statement::ForEach { steps, .. } = step {
            all_statements(steps, out)
        }
    }
}
fn schemas(
    snapshot: &ProjectSnapshot,
    after: &AppDefinition,
    request: &OwnedPreviewRequest,
    inventory: &CommonFactInventory,
) -> AdmissionResult<BTreeMap<Id, Kind>> {
    let before = &snapshot.program()?.program;
    let root = &request.inventory.sources.root_entity;
    for e in &snapshot.data.schema {
        if before.entities.iter().find(|x| x.id == e.id) != Some(e)
            || after.entities.iter().find(|x| x.id == e.id) != Some(e)
        {
            return Err(refuse(
                &format!("entity {}", e.id),
                "retained schema differs from both exact sources",
            ));
        }
    }
    let mut result = BTreeMap::new();
    let mut declared = BTreeSet::new();
    for aux in &request.auxiliary {
        if !declared.insert(aux.entity.clone()) {
            return Err(refuse(&aux.entity, "duplicate auxiliary declaration"));
        }
    }
    for entity in &after.entities {
        if !entity.constraints.is_empty() {
            return Err(refuse(
                &format!("entity {}", entity.id),
                "schema constraints are outside the total admission grammar",
            ));
        }
        let refs: Vec<_> = entity
            .fields
            .iter()
            .filter(|f| matches!(f.value_type, Type::Reference { .. }))
            .collect();
        if entity.fields.iter().any(|f| {
            super::common_fact_inventory::relation(&f.value_type).is_some()
                && !matches!(f.value_type, Type::Reference { .. })
        }) {
            return Err(refuse(
                &entity.id,
                "optional/list references have unsupported ownership provenance",
            ));
        }
        let owner = refs
            .iter()
            .find(|f| f.value_type == Type::reference(root))
            .map(|f| f.id.clone());
        let kind = if &entity.id == root {
            if !entity.unique.is_empty() || !refs.is_empty() {
                return Err(refuse(
                    &entity.id,
                    "root uniqueness or reference ownership is unsupported",
                ));
            }
            Kind::Root
        } else if let Some(aux) = request.auxiliary.iter().find(|a| a.entity == entity.id) {
            if before.entities.iter().any(|e| e.id == entity.id)
                || snapshot.data.schema.iter().any(|e| e.id == entity.id)
                || owner.as_ref() != Some(&aux.owner_field)
                || refs.len() != 1
                || entity.unique != vec![vec![aux.owner_field.clone()]]
            {
                return Err(refuse(&entity.id,"auxiliary must be fresh with one immutable required owner and owner-only uniqueness"));
            }
            Kind::Aux {
                owner: aux.owner_field.clone(),
            }
        } else if let Some(owner) = owner.clone().filter(|_| !entity.unique.is_empty()) {
            let fields: BTreeSet<_> = refs.iter().map(|f| f.id.clone()).collect();
            if refs
                .iter()
                .filter(|f| f.value_type == Type::reference(root))
                .count()
                != 1
                || entity.unique.len() != 1
                || entity.unique[0].iter().cloned().collect::<BTreeSet<_>>() != fields
                || entity.unique[0].len() != fields.len()
            {
                return Err(refuse(
                    &entity.id,
                    "owner-partitioned uniqueness must contain every exact required reference",
                ));
            }
            if entity
                .fields
                .iter()
                .any(|f| !fields.contains(&f.id) && f.value_type != Type::Boolean)
            {
                return Err(refuse(
                    &entity.id,
                    "only Boolean non-owner link state is supported",
                ));
            }
            Kind::Link { owner }
        } else if !entity.unique.is_empty() {
            if !refs.is_empty() || entity.unique.len() != 1 || entity.unique[0].len() != 1 {
                return Err(refuse(&entity.id, "unsupported global uniqueness"));
            }
            let key = &entity.unique[0][0];
            if !entity
                .fields
                .iter()
                .any(|f| f.id == *key && f.value_type == Type::Text)
            {
                return Err(refuse(&entity.id, "registry key must be required text"));
            }
            Kind::Registry { key: key.clone() }
        } else {
            let mut sequence = None;
            let mut mode = None;
            let mut creates = 0;
            for app in [before, after] {
                for action in &app.actions {
                    let mut statements = vec![];
                    all_statements(&action.steps, &mut statements);
                    for step in statements {
                        if let Statement::Create {
                            entity: e, values, ..
                        } = step
                        {
                            if e == &entity.id {
                                creates += 1;
                                let matches: Vec<_> = values
                                    .iter()
                                    .filter_map(|(f, v)| {
                                        bookkeeping(v)
                                            .filter(|(r, _, _)| *r == entity.id)
                                            .map(|(_, _, a)| (f, a))
                                    })
                                    .collect();
                                if matches.len() != 1 {
                                    return Err(refuse(&entity.id,"terminal history needs one bounded count-plus-one producer field"));
                                }
                                let (field, archived) = matches[0];
                                if sequence.as_ref().is_some_and(|f| f != field)
                                    || mode.is_some_and(|m| m != archived)
                                {
                                    return Err(refuse(
                                        &entity.id,
                                        "history producers disagree on sequence/count population",
                                    ));
                                }
                                sequence = Some(field.clone());
                                mode = Some(archived);
                            }
                        }
                    }
                }
            }
            if creates == 0 || refs.len() != 1 {
                return Err(refuse(&entity.id, "unknown relation/producer grammar"));
            }
            Kind::History {
                owner,
                sequence: sequence.unwrap(),
                archived: mode.unwrap(),
            }
        };
        result.insert(entity.id.clone(), kind);
    }
    for aux in &request.auxiliary {
        if !result.get(&aux.entity).is_some_and(Kind::auxiliary) {
            return Err(refuse(&aux.entity, "undeclared auxiliary addition"));
        }
    }
    for e in &after.entities {
        if !before.entities.iter().any(|x| x.id == e.id) && !declared.contains(&e.id) {
            return Err(refuse(
                &e.id,
                "every added relation needs exact auxiliary ownership",
            ));
        }
    }
    for reference in &inventory.references {
        if !reference.required || !reference.mutations.is_empty() {
            return Err(refuse(
                &format!("{}.{}", reference.entity, reference.field),
                "all reference assignment/archive/collection routes must be immutable",
            ));
        }
        if let Some(kind) = result.get(&reference.entity) {
            if matches!(
                result.get(&reference.target_entity),
                Some(Kind::History { .. } | Kind::Aux { .. })
            ) {
                return Err(refuse(
                    &reference.entity,
                    "references to terminal history/private auxiliary relations are unsupported",
                ));
            }
            if kind.owner() == Some(&reference.field) && reference.target_entity != *root {
                return Err(refuse(
                    &reference.entity,
                    "owner reference is not the exact root",
                ));
            }
        }
    }
    for (entity, kind) in &result {
        if let Kind::History {
            sequence, archived, ..
        } = kind
        {
            if !archived
                && snapshot
                    .data
                    .records
                    .iter()
                    .any(|r| r.entity == *entity && r.archived)
            {
                return Err(refuse(
                    entity,
                    "active-only sequence producer has retained archived identity/order ambiguity",
                ));
            }
            let rows: Vec<_> = snapshot
                .data
                .records
                .iter()
                .filter(|r| r.entity == *entity && (*archived || !r.archived))
                .collect();
            let mut numbers = BTreeSet::new();
            for row in &rows {
                let Some(DataValue::Integer { value }) = row.values.get(sequence) else {
                    return Err(refuse(entity, "retained sequence missing or mistyped"));
                };
                numbers.insert(*value);
            }
            if numbers.len() != rows.len() || !numbers.iter().copied().eq(1..=rows.len() as i64) {
                return Err(refuse(entity,"retained sequence must be exactly distinct 1..count; fresh/retained ties are unsupported"));
            }
        }
    }
    Ok(result)
}
impl Rules<'_> {
    fn certify<T: Serialize>(&mut self, site: &str, rule: &str, value: &T) -> AdmissionResult<()> {
        self.meter.charge(1)?;
        self.certs.push(CommonFactCertificate {
            side: self.side,
            site: site.into(),
            rule: rule.into(),
            identity: canonical_digest(IdentityDomain::Evidence, value)?,
        });
        Ok(())
    }
    fn env(&self, params: &BTreeMap<Id, Type>, prefix: &str) -> AdmissionResult<Env> {
        let mut env = Env::new();
        for (n, t) in params {
            let mut v = Value::plain(t.clone());
            if let Type::Reference { entity } = t {
                match self.kinds.get(entity) {
                    Some(Kind::Root) => v.root = Some(format!("{prefix}/{n}")),
                    Some(Kind::Link { owner }) => v.root = Some(format!("{prefix}/{n}.{owner}")),
                    Some(Kind::Registry { .. }) => {}
                    _ => {
                        return Err(refuse(
                            prefix,
                            "direct reference to private/terminal relation is unsupported",
                        ))
                    }
                }
            }
            if v.root.is_some() {
                v.active_root = self.all_roots_active;
            }
            env.insert(n.clone(), v);
        }
        Ok(env)
    }
    fn ty(&self, e: &Expr, env: &Env) -> AdmissionResult<Type> {
        Ok(self.app.expression_type_in(
            e,
            &env.iter().map(|(n, v)| (n.clone(), v.ty.clone())).collect(),
        )?)
    }
    fn receiver(&self, e: &Expr, env: &Env) -> Option<String> {
        match e {
            Expr::Variable { name } => env.get(name)?.root.clone(),
            Expr::Field { record, field } => {
                let Type::Reference { entity } = self.ty(record, env).ok()? else {
                    return None;
                };
                if self.kinds.get(&entity)?.owner() == Some(field) {
                    self.receiver(record, env)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
    fn facet(&self, e: &Expr, env: &Env) -> Option<String> {
        let Expr::Equal { left, right } = e else {
            return None;
        };
        if literal(right) != Some(self.lifecycle.1) {
            return None;
        }
        let Expr::Field { record, field } = left.as_ref() else {
            return None;
        };
        if field != self.lifecycle.0
            || self.ty(record, env).ok()? != Type::reference(&self.inventory.structure.root_entity)
        {
            return None;
        }
        self.receiver(record, env)
    }
    fn unfinished(&self, e: &Expr, env: &Env) -> BTreeSet<String> {
        match e {
            Expr::And { values } => values
                .iter()
                .flat_map(|v| self.unfinished(v, env))
                .collect(),
            Expr::Not { value } => self.facet(value, env).into_iter().collect(),
            Expr::Equal { left, right } => {
                if let (Expr::Field { record, field }, Some(value)) =
                    (left.as_ref(), literal(right))
                {
                    if field == self.lifecycle.0
                        && value != self.lifecycle.1
                        && self.ty(record, env).ok()
                            == Some(Type::reference(&self.inventory.structure.root_entity))
                    {
                        return self.receiver(record, env).into_iter().collect();
                    }
                }
                BTreeSet::new()
            }
            _ => BTreeSet::new(),
        }
    }
    fn correlation(&self, p: &Expr, entity: &str, binding: &str, env: &Env) -> Option<String> {
        match p {
            Expr::And { values } => values
                .iter()
                .find_map(|p| self.correlation(p, entity, binding, env)),
            Expr::Equal { left, right } => {
                for (a, b) in [
                    (left.as_ref(), right.as_ref()),
                    (right.as_ref(), left.as_ref()),
                ] {
                    let matches = if entity == self.inventory.structure.root_entity {
                        variable(a, binding)
                    } else {
                        matches!(a,Expr::Field{record,field} if variable(record,binding)&&self.kinds.get(entity).and_then(Kind::owner)==Some(field))
                    };
                    if matches
                        && self.ty(b, env).ok()
                            == Some(Type::reference(&self.inventory.structure.root_entity))
                    {
                        if let Some(root) = self.receiver(b, env) {
                            return Some(root);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
    fn expr(
        &mut self,
        e: &Expr,
        env: &Env,
        facets: bool,
        display: bool,
        at: &str,
    ) -> AdmissionResult<Value> {
        self.meter.charge(1)?;
        let ty = self.ty(e, env)?;
        let mut out = Value::plain(ty);
        if facets {
            if let Some(root) = self.facet(e, env) {
                out.facets.insert(root);
                self.certify(at, "exact accepted lifecycle facet occurrence", e)?;
                return Ok(out);
            }
        }
        match e {
            Expr::Literal { value_type, .. } => {
                if super::common_fact_inventory::relation(value_type).is_some() {
                    return Err(refuse(
                        at,
                        "literal references have no authenticated provenance",
                    ));
                }
            }
            Expr::Variable { name } => {
                return Ok(env
                    .get(name)
                    .ok_or_else(|| refuse(at, "missing typed binding"))?
                    .clone())
            }
            Expr::Today => out.frame_dependent = true,
            Expr::State { .. } => {
                out.frame_dependent = true;
                out.session_dependent = true;
            }
            Expr::Field { record, field } => {
                let receiver = self.expr(record, env, facets, display, &format!("{at}.record"))?;
                let Type::Reference { entity } = &receiver.ty else {
                    return Err(refuse(at, "unproved field reference"));
                };
                out.merge(&receiver);
                let kind = self
                    .kinds
                    .get(entity)
                    .ok_or_else(|| refuse(at, "unknown entity"))?;
                if self.policy.contains(&(entity.clone(), field.clone()))
                    || (kind.auxiliary() && kind.owner() != Some(field))
                {
                    out.policy
                        .insert(receiver.root.clone().unwrap_or_else(|| "global".into()));
                }
                if matches!(out.ty, Type::Reference { .. }) && kind.owner() == Some(field) {
                    out.root = receiver.root.clone();
                    out.active_root = receiver.active_root;
                }
                if matches!(kind, Kind::History { .. }) && !display {
                    return Err(refuse(at, "terminal history field has a business consumer"));
                }
            }
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                include_archived,
            } => {
                if matches!(self.kinds.get(entity), Some(Kind::History { .. })) {
                    return Err(refuse(at,"terminal history query is not an exact bookkeeping or inert history surface"));
                }
                let root = self
                    .correlation(predicate, entity, binding, env)
                    .or_else(|| {
                        (entity == &self.inventory.structure.root_entity)
                            .then(|| format!("{at}/{binding}"))
                    });
                let mut local = env.clone();
                let mut row = Value::plain(Type::reference(entity));
                row.root = root.clone();
                row.active_root = self.all_roots_active
                    || (entity == &self.inventory.structure.root_entity && !include_archived)
                    || env
                        .values()
                        .any(|v| v.active_root && v.root.is_some() && v.root == root);
                out.active_root = row.active_root;
                local.insert(binding.clone(), row);
                let p = self.expr(
                    predicate,
                    &local,
                    facets,
                    display,
                    &format!("{at}.predicate"),
                )?;
                out.merge(&p);
                out.root = root.clone();
                if self.kinds.get(entity).is_some_and(Kind::auxiliary) {
                    out.policy.insert(root.clone().ok_or_else(|| {
                        refuse(at, "auxiliary membership lacks exact owner correlation")
                    })?);
                }
                if !out.policy.is_empty()
                    && (!out.policy.iter().all(|r| Some(r) == root.as_ref())
                        || *limit != MAX_COLLECTION)
                {
                    return Err(refuse(at, "policy selection is cross-root or truncated"));
                }
                for key in sort {
                    let v = self.expr(&key.value, &local, false, display, &format!("{at}.sort"))?;
                    if !v.policy.is_empty() || !out.policy.is_empty() {
                        return Err(refuse(at, "policy-dependent query order is unsupported"));
                    }
                    out.merge(&v)
                }
            }
            Expr::All {
                items,
                binding,
                predicate,
            }
            | Expr::Any {
                items,
                binding,
                predicate,
            }
            | Expr::Filter {
                items,
                binding,
                predicate,
            } => {
                let items = self.expr(items, env, facets, display, &format!("{at}.items"))?;
                let mut local = env.clone();
                local.insert(binding.clone(), items.item()?);
                let p = self.expr(
                    predicate,
                    &local,
                    facets,
                    display,
                    &format!("{at}.predicate"),
                )?;
                out.merge(&items);
                out.merge(&p);
                if matches!(e, Expr::Filter { .. }) {
                    out.root = items.root;
                    out.active_root = items.active_root;
                }
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                if !display {
                    return Err(refuse(at, "business map/order fold is unsupported"));
                }
                let items = self.expr(items, env, facets, true, &format!("{at}.items"))?;
                let mut local = env.clone();
                local.insert(binding.clone(), items.item()?);
                let v = self.expr(value, &local, facets, true, &format!("{at}.value"))?;
                out.merge(&items);
                out.merge(&v);
            }
            Expr::Count { items } => {
                let items = self.expr(items, env, facets, display, &format!("{at}.items"))?;
                out.merge(&items);
            }
            Expr::Add { left, right } => {
                if let Some((entity, _, archived)) = bookkeeping(e) {
                    let permitted = self.inventory.structure.writers.iter().any(|w|
                        w.side == self.side && w.entity == entity && w.kind == OwnedStatementKind::Create
                        && w.field.as_ref().is_some_and(|f| format!("{} {:?}.{}", w.action, w.path, f) == at)
                        && matches!(self.kinds.get(entity), Some(Kind::History{sequence,..}) if w.field.as_ref() == Some(sequence)));
                    if !permitted {
                        return Err(refuse(
                            at,
                            "terminal count-plus-one used outside exact append bookkeeping site",
                        ));
                    }

                    if !matches!(self.kinds.get(entity),Some(Kind::History{archived:a,..}) if *a==archived)
                    {
                        return Err(refuse(
                            at,
                            "count-plus-one is not verified terminal bookkeeping",
                        ));
                    }
                    self.certify(at, "bounded count-plus-one terminal bookkeeping", e)?;
                } else if let (
                    Some(DataValue::Integer { value: a }),
                    Some(DataValue::Integer { value: b }),
                ) = (literal(left), literal(right))
                {
                    if a.checked_add(*b).is_none() {
                        return Err(refuse(at, "integer addition is not total"));
                    }
                } else {
                    return Err(refuse(at, "arithmetic range/totality is unproved"));
                }
            }
            Expr::Subtract { left, right } => {
                if !matches!((literal(left),literal(right)),(Some(DataValue::Integer{value:a}),Some(DataValue::Integer{value:b})) if a.checked_sub(*b).is_some())
                {
                    return Err(refuse(at, "subtraction range/totality is unproved"));
                }
            }
            Expr::DateAdd { .. }
            | Expr::DateDifference { .. }
            | Expr::Sum { .. }
            | Expr::Concat { .. } => {
                return Err(refuse(
                    at,
                    "partial date/arithmetic/text operation is outside total grammar",
                ))
            }
            Expr::Equal { left, right } | Expr::Less { left, right } => {
                out.merge(&self.expr(left, env, facets, display, &format!("{at}.left"))?);
                out.merge(&self.expr(right, env, facets, display, &format!("{at}.right"))?);
            }
            Expr::TextContains { text, search } => {
                out.merge(&self.expr(text, env, facets, display, at)?);
                out.merge(&self.expr(search, env, facets, display, at)?);
            }
            Expr::Contains { items, value } => {
                out.merge(&self.expr(items, env, facets, display, at)?);
                out.merge(&self.expr(value, env, facets, display, at)?);
            }
            Expr::Lower { value } => {
                let Some(DataValue::Text { value: text }) = literal(value) else {
                    return Err(refuse(at, "lowercase output byte range is unproved"));
                };
                self.meter.charge(text.len() as u64)?;
                if text.to_lowercase().len() > MAX_TEXT_BYTES {
                    return Err(refuse(
                        at,
                        "lowercase expansion exceeds the text byte bound",
                    ));
                }
                out.merge(&self.expr(value, env, facets, display, &format!("{at}.value"))?);
                self.certify(at, "literal lowercase output fits the text byte bound", e)?;
            }
            Expr::Not { value } => {
                out.merge(&self.expr(value, env, facets, display, &format!("{at}.value"))?)
            }
            Expr::And { values } | Expr::Or { values } => {
                for (i, v) in values.iter().enumerate() {
                    out.merge(&self.expr(v, env, facets, display, &format!("{at}[{i}]"))?)
                }
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                for (label, v) in [
                    ("condition", condition),
                    ("then", then_value),
                    ("else", else_value),
                ] {
                    out.merge(&self.expr(v, env, facets, display, &format!("{at}.{label}"))?)
                }
            }
            Expr::Coalesce { value, fallback } => {
                out.merge(&self.expr(value, env, facets, display, at)?);
                out.merge(&self.expr(fallback, env, facets, display, at)?);
            }
        }
        Ok(out)
    }
    fn variant(&self, action: &str, path: &[usize]) -> Option<&OwnedSpanEndpoint> {
        self.inventory
            .structure
            .edits
            .iter()
            .map(|e| {
                if self.side == OwnedSourceSide::Before {
                    &e.before
                } else {
                    &e.after
                }
            })
            .find(|e| {
                e.action == action
                    && path.len() > e.list_path.len()
                    && path.starts_with(&e.list_path)
                    && (e.start..e.start + e.len).contains(&path[e.list_path.len()])
            })
    }
    fn finite_unfinished(&self, e: &Expr, root: &str, env: &Env) -> bool {
        if let Some(value) = literal(e) {
            return value != self.lifecycle.1;
        }
        if let Expr::Field { record, field } = e {
            // Owner association is not record identity: an auxiliary may have
            // a same-named field containing an unrelated lifecycle value.
            return field == self.lifecycle.0
                && self.ty(record, env).ok()
                    == Some(Type::reference(&self.inventory.structure.root_entity))
                && self.receiver(record, env).as_deref() == Some(root);
        }
        if let Expr::If {
            condition,
            then_value,
            else_value,
        } = e
        {
            if self.facet(condition, env).as_deref() == Some(root) {
                return self.finite_unfinished(else_value, root, env);
            }
            return self.finite_unfinished(then_value, root, env)
                && self.finite_unfinished(else_value, root, env);
        }
        false
    }
    fn mixed(&self, action: &ActionDefinition) -> bool {
        // Use independently typed writers, not syntax such as a Variable target:
        // a directly addressed registry Update is still a common factual effect.
        self.inventory.structure.writers.iter().any(|w| {
            w.side == self.side
                && w.action == action.id
                && match self.kinds.get(&w.entity) {
                    Some(Kind::Root) => w.kind == OwnedStatementKind::Create,
                    Some(Kind::Aux { .. }) => false,
                    Some(_) => true,
                    None => false,
                }
        })
    }
    fn shared_update(&self, action: &str) -> bool {
        self.inventory.structure.writers.iter().any(|w| {
            w.side == self.side
                && w.action == action
                && w.kind == OwnedStatementKind::Update
                && matches!(self.kinds.get(&w.entity), Some(Kind::Registry { .. }))
        })
    }
    fn auxiliary_absence(
        &self,
        action: &ActionDefinition,
        path: &[usize],
        entity: &str,
        root: &str,
        env: &Env,
    ) -> bool {
        if path.len() < 2 {
            return false;
        }
        let mut steps = action.steps.as_slice();
        let mut parent = None;
        for index in &path[..path.len() - 1] {
            let Statement::ForEach {
                items,
                steps: inner,
                ..
            } = &steps[*index]
            else {
                return false;
            };
            parent = Some(items);
            steps = inner;
        }
        let Some(Expr::Query {
            entity: gate_entity,
            binding,
            predicate,
            limit,
            include_archived: false,
            ..
        }) = parent
        else {
            return false;
        };
        if gate_entity != &self.inventory.structure.root_entity
            || *limit != MAX_COLLECTION
            || self
                .correlation(predicate, gate_entity, binding, env)
                .as_deref()
                != Some(root)
        {
            return false;
        }
        fn absent(rules: &Rules<'_>, expr: &Expr, entity: &str, root: &str, env: &Env) -> bool {
            if let Expr::And { values } = expr {
                return values.iter().any(|v| absent(rules, v, entity, root, env));
            }
            let Expr::Equal { left, right } = expr else {
                return false;
            };
            if literal(right) != Some(&DataValue::Integer { value: 0 }) {
                return false;
            }
            let Expr::Count { items } = left.as_ref() else {
                return false;
            };
            let Expr::Query {
                entity: e,
                binding,
                predicate,
                limit,
                sort,
                include_archived: false,
            } = items.as_ref()
            else {
                return false;
            };
            e == entity
                && *limit == MAX_COLLECTION
                && sort.is_empty()
                && matches!(predicate.as_ref(), Expr::Equal { .. })
                && rules.correlation(predicate, e, binding, env).as_deref() == Some(root)
        }
        // The selected absence witness must still hold at the Create. A
        // nested producer can consume it before a later direct Create, even
        // when each gate has only one direct producer. Keep this first rule
        // atomic in source structure: the sole body statement is this Create.
        matches!(steps, [Statement::Create { entity: e, .. }] if e == entity)
            && absent(self, predicate, entity, root, env)
    }
    fn statements(
        &mut self,
        action: &ActionDefinition,
        steps: &[Statement],
        path: &[usize],
        env: &mut Env,
        control: &Value,
        unfinished: &BTreeSet<String>,
        owner: Option<&str>,
        mixed: bool,
    ) -> AdmissionResult<()> {
        for (i, step) in steps.iter().enumerate() {
            self.meter.charge(1)?;
            let mut path = path.to_vec();
            path.push(i);
            let at = format!("{} {path:?}", action.id);
            let endpoint = self.variant(&action.id, &path).cloned();
            let variant = endpoint.is_some();
            // Structural resolution authenticates the binder's type, not its
            // ownership. Derive its actual root identity independently, then
            // enforce it for every descendant and target in the entire span.
            let mut verified_owner = owner.map(str::to_owned);
            if let Some(root) = endpoint.as_ref().and_then(|e| e.root.as_ref()) {
                let derived = env
                    .get(&root.name)
                    .filter(|v| v.ty == Type::reference(&self.inventory.structure.root_entity))
                    .and_then(|v| v.root.clone())
                    .ok_or_else(|| {
                        refuse(&at, "declared span root has no derived owner identity")
                    })?;
                if owner.is_some_and(|outer| outer != derived) {
                    return Err(refuse(
                        &at,
                        "declared span root differs from its enclosing frontier",
                    ));
                }
                self.certify(
                    &at,
                    "endpoint root authenticated against exact derived owner",
                    &(root, &derived),
                )?;
                verified_owner = Some(derived);
            }
            let owner = verified_owner.as_deref();
            match step {
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let selection = self.expr(items, env, true, false, &format!("{at}.items"))?;
                    if variant && owner.is_none() {
                        if !matches!(items, Expr::Query { entity, limit, include_archived: false, .. }
                            if entity == &self.inventory.structure.root_entity && *limit == MAX_COLLECTION)
                        {
                            return Err(refuse(
                                &at,
                                "unbound insertion lacks an exact active root traversal",
                            ));
                        }
                        self.certify(
                            &at,
                            "one-sided insertion establishes an exact per-row root frontier",
                            &(binding, items, &selection.root),
                        )?;
                    }
                    if variant && owner.is_some() && selection.root.as_deref() != owner {
                        return Err(refuse(
                            &at,
                            "nested variant traversal escapes its exact root frontier",
                        ));
                    }
                    let mut local = env.clone();
                    local.insert(binding.clone(), selection.item()?);
                    let mut c = control.clone();
                    c.merge(&selection);
                    let mut u = unfinished.clone();
                    if let Expr::Query {
                        entity,
                        binding: query_binding,
                        predicate,
                        ..
                    } = items
                    {
                        let mut query_env = env.clone();
                        let mut row = Value::plain(Type::reference(entity));
                        row.root = selection.root.clone();
                        query_env.insert(query_binding.clone(), row);
                        u.extend(self.unfinished(predicate, &query_env));
                    }
                    let root = selection.root.as_deref().or(owner);
                    if !c.policy.is_empty() && !c.policy.iter().all(|r| Some(r.as_str()) == root) {
                        return Err(refuse(&at, "loop policy control crosses root ownership"));
                    }
                    self.statements(action, steps, &path, &mut local, &c, &u, root, mixed)?;
                }
                Statement::Update { record, values } => {
                    let target = self.expr(record, env, false, false, &format!("{at}.target"))?;
                    let Type::Reference { entity } = &target.ty else {
                        return Err(refuse(&at, "update target is not a required reference"));
                    };
                    let kind = self.kinds.get(entity).unwrap();
                    if matches!(kind, Kind::History { .. }) {
                        return Err(refuse(&at, "terminal history update is forbidden"));
                    }
                    if variant && !matches!(kind, Kind::Root | Kind::Aux { .. }) {
                        return Err(refuse(&at, "variant writes a common relation"));
                    }
                    if matches!(kind, Kind::Root)
                        && !target.root.as_ref().is_some_and(|r| unfinished.contains(r))
                    {
                        return Err(refuse(
                            &at,
                            "business update can touch previously completed history",
                        ));
                    }
                    if variant && (owner.is_none() || target.root.as_deref() != owner) {
                        return Err(refuse(&at, "owned write targets a different root"));
                    }
                    for (field, value) in values {
                        if kind.owner() == Some(field)
                            || matches!(kind,Kind::Registry{key} if key==field)
                        {
                            return Err(refuse(&at, "immutable owner/registry key reassignment"));
                        }
                        let v = self.expr(value, env, true, false, &format!("{at}.{field}"))?;
                        let mut deps = control.clone();
                        deps.merge(&target);
                        deps.merge(&v);
                        if matches!(kind, Kind::Root)
                            && (variant || (mixed && !deps.policy.is_empty()))
                            && !target.active_root
                        {
                            return Err(refuse(
                                &at,
                                "policy update lacks unarchived root-target provenance",
                            ));
                        }
                        match kind {
                            Kind::Root | Kind::Aux { .. } => {
                                if variant || !deps.policy.is_empty() {
                                    self.discovered.insert((entity.clone(), field.clone()));
                                }
                                if !deps.policy.iter().all(|r| Some(r) == target.root.as_ref()) {
                                    return Err(refuse(
                                        &at,
                                        "owned value depends on another root's policy",
                                    ));
                                }
                                if matches!(kind, Kind::Root) && field == self.lifecycle.0 {
                                    let root = target.root.as_deref().unwrap();
                                    if !self.finite_unfinished(value, root, env) {
                                        if mixed
                                            || variant
                                            || steps.len() != 1
                                            || path.len() != 1
                                            || path[0] != 0
                                            || literal(value) != Some(self.lifecycle.1)
                                        {
                                            return Err(refuse(
                                                &at,
                                                "mixed or unproved lifecycle transition",
                                            ));
                                        }
                                        self.certify(&at,"isolated explicit completion transition; capture then seal required",step)?;
                                    }
                                }
                            }
                            Kind::Link { .. } => {
                                if !deps.policy.is_empty()
                                    || !matches!(literal(value), Some(DataValue::Boolean { .. }))
                                {
                                    return Err(refuse(
                                        &at,
                                        "link non-owner update must be common literal Boolean",
                                    ));
                                }
                            }
                            Kind::Registry { .. } => {
                                let explicit = matches!(record,Expr::Variable{name} if action.parameters.get(name)==Some(&Type::reference(entity)));
                                if !explicit || !deps.policy.is_empty() || !deps.facets.is_empty() {
                                    return Err(refuse(&at,"shared existing fact update requires explicit shared parameter and policy-independent admission"));
                                }
                                if !matches!(value, Expr::Literal { .. } | Expr::Today)
                                    && !matches!(value,Expr::Variable{name} if action.parameters.get(name).is_some_and(Type::is_scalar))
                                {
                                    return Err(refuse(
                                        &at,
                                        "shared fact value is not explicit input/literal/clock",
                                    ));
                                }
                            }
                            Kind::History { .. } => unreachable!(),
                        }
                    }
                    self.certify(
                        &at,
                        if variant {
                            "exact owner policy write with total expressions"
                        } else {
                            "exact common or isolated owner writer"
                        },
                        step,
                    )?;
                }
                Statement::Create {
                    entity,
                    bind,
                    values,
                } => {
                    let kind = self.kinds.get(entity).unwrap();
                    if variant && !kind.auxiliary() {
                        return Err(refuse(
                            &at,
                            "variant may create only a declared owned auxiliary",
                        ));
                    }
                    let mut deps = control.clone();
                    let mut evaluated = BTreeMap::new();
                    for (field, value) in values {
                        let v = self.expr(value, env, true, false, &format!("{at}.{field}"))?;
                        deps.merge(&v);
                        evaluated.insert(field, v);
                    }
                    let root = kind
                        .owner()
                        .and_then(|field| evaluated.get(&field.to_owned()))
                        .and_then(|v| v.root.clone());
                    match kind {
                        Kind::Aux { .. } => {
                            if !root.as_ref().is_some_and(|r| {
                                self.auxiliary_absence(action, &path, entity, r, env)
                            }) {
                                return Err(refuse(&at,"auxiliary uniqueness can reject common transaction: exact current owner-absence gate required"));
                            }
                            if root.is_none()
                                || owner != root.as_deref()
                                || !deps.policy.iter().all(|r| Some(r) == root.as_ref())
                            {
                                return Err(refuse(
                                    &at,
                                    "auxiliary create lacks exact local owner",
                                ));
                            }
                        }
                        Kind::Root => {
                            if !deps.policy.is_empty()
                                || !values
                                    .get(self.lifecycle.0)
                                    .is_some_and(|v| self.finite_unfinished(v, "new-root", env))
                            {
                                return Err(refuse(&at,"root creation must be policy-independent and initially unfinished"));
                            }
                        }
                        Kind::Registry { key } => {
                            if !deps.policy.is_empty() || deps.private_identity {
                                return Err(refuse(&at, "policy-derived shared registry create"));
                            }
                            for (field, value) in values {
                                if field == key {
                                    if let Expr::Field { record, field } = value {
                                        let Type::Reference { entity } = self.ty(record, env)?
                                        else {
                                            return Err(refuse(&at, "registry key origin"));
                                        };
                                        if self.inventory.structure.writers.iter().any(|w| {
                                            w.entity == entity
                                                && w.field.as_ref() == Some(field)
                                                && w.kind != OwnedStatementKind::Create
                                        }) {
                                            return Err(refuse(&at,"registry name must come from immutable legacy fact or explicit scalar input"));
                                        }
                                    }
                                } else if !matches!(
                                    literal(value),
                                    Some(DataValue::Boolean { value: false } | DataValue::Null)
                                ) && !matches!(literal(value),Some(DataValue::Text{value}) if value.is_empty())
                                {
                                    return Err(refuse(&at,"registry initial receipt fields must be empty/false/null literals"));
                                }
                            }
                        }
                        Kind::Link { .. } => {
                            if root.is_none() || !deps.policy.is_empty() || deps.private_identity {
                                return Err(refuse(
                                    &at,
                                    "common link create has unproved owner or private dependency",
                                ));
                            }
                            for (field, value) in values {
                                if self
                                    .app
                                    .entities
                                    .iter()
                                    .find(|x| x.id == *entity)
                                    .unwrap()
                                    .fields
                                    .iter()
                                    .find(|f| f.id == *field)
                                    .unwrap()
                                    .value_type
                                    == Type::Boolean
                                    && !matches!(literal(value), Some(DataValue::Boolean { .. }))
                                {
                                    return Err(refuse(&at, "new link flags must be literals"));
                                }
                            }
                        }
                        Kind::History {
                            owner: history_owner,
                            sequence,
                            ..
                        } => {
                            if !deps.policy.is_empty()
                                || deps.private_identity
                                || (!deps.facets.is_empty() && history_owner.is_none())
                            {
                                return Err(refuse(&at,"terminal append has policy/lifecycle-selected common admission"));
                            }
                            if !values
                                .get(sequence)
                                .and_then(bookkeeping)
                                .is_some_and(|(e, _, _)| e == entity)
                            {
                                return Err(refuse(&at, "terminal producer bookkeeping differs"));
                            }
                        }
                    }
                    let mut created = Value::plain(Type::reference(entity));
                    created.root = if matches!(kind, Kind::Root) {
                        Some(format!("{at}/{bind}"))
                    } else {
                        root
                    };
                    created.active_root = matches!(kind, Kind::Root)
                        || self.all_roots_active
                        || env
                            .values()
                            .any(|v| v.active_root && v.root.is_some() && v.root == created.root);
                    created.private_identity = variant;
                    env.insert(bind.clone(), created);
                    self.certify(
                        &at,
                        if variant {
                            "exact owned auxiliary create"
                        } else {
                            "exact common creation with complete producer/consumer closure"
                        },
                        step,
                    )?;
                }
                Statement::SetState { state, value } => {
                    if variant
                        || !control.policy.is_empty()
                        || path.len() != 1
                        || i + 1 != steps.len()
                        || !matches!(literal(value),Some(DataValue::Text{value}) if value.is_empty())
                    {
                        return Err(refuse(
                            &at,
                            "input reset must be literal empty, common, top-level and final",
                        ));
                    }
                    self.certify(&at, &format!("closed input reset {state}"), step)?;
                }
                Statement::Assert { condition, .. } => {
                    let v = self.expr(condition, env, false, false, &at)?;
                    if variant || !v.policy.is_empty() || !control.policy.is_empty() {
                        return Err(refuse(&at, "dependent assertion is unsupported"));
                    }
                }
                Statement::Archive { .. }
                | Statement::Collection { .. }
                | Statement::Emit { .. } => {
                    return Err(refuse(
                        &at,
                        "archive/collection/output effect has no closed preview rule",
                    ))
                }
            }
        }
        Ok(())
    }
    fn actions(&mut self) -> AdmissionResult<()> {
        for action in &self.app.actions {
            let mut env = self.env(&action.parameters, &action.id)?;
            let mut controls = Value::plain(Type::Boolean);
            let mut unfinished = BTreeSet::new();
            let mixed = self.mixed(action);
            for (i, guard) in action.guards.iter().enumerate() {
                let at = format!("{} guard[{i}]", action.id);
                let v = self.expr(guard, &env, true, false, &at)?;
                controls.merge(&v);
                unfinished.extend(self.unfinished(guard, &env));
            }
            if mixed && !controls.policy.is_empty() {
                return Err(refuse(&action.id,"common factual admission depends on private policy rather than exact lifecycle facet"));
            }
            if !action.ensures.is_empty() {
                return Err(refuse(
                    &action.id,
                    "action postconditions are outside closed total admission grammar",
                ));
            }
            self.statements(
                action,
                &action.steps,
                &[],
                &mut env,
                &controls,
                &unfinished,
                None,
                mixed,
            )?;
        }
        Ok(())
    }
}
impl Rules<'_> {
    fn history_view(
        &mut self,
        view: &ViewDefinition,
        entity: &str,
        rows: &Expr,
        columns: &[Column],
    ) -> AdmissionResult<()> {
        let Kind::History { sequence, .. } = &self.kinds[entity] else {
            unreachable!()
        };
        let Expr::Query {
            entity: e,
            binding,
            predicate,
            sort,
            limit,
            include_archived: _,
        } = rows
        else {
            return Err(refuse(&view.id, "history must be an inert direct query"));
        };
        if e != entity
            || !truth(predicate)
            || *limit != MAX_COLLECTION
            || sort.len() != 1
            || !matches!(&sort[0].value,Expr::Field{record,field} if variable(record,binding)&&field==sequence)
            || !view.actions.is_empty()
            || !view.keys.is_empty()
        {
            return Err(refuse(
                &view.id,
                "fresh-identity history consumer has unsupported order, selection or action",
            ));
        }
        if let ViewKind::List {
            controls,
            selection,
            ..
        } = &view.kind
        {
            if !controls.is_empty() || selection.is_some() {
                return Err(refuse(
                    &view.id,
                    "history controls/selection would expose shifted row identities",
                ));
            }
        }
        for column in columns {
            if !matches!(&column.value,Expr::Field{record,..} if variable(record,"row")) {
                return Err(refuse(
                    &view.id,
                    "history columns must be direct stored scalar fields",
                ));
            }
            let env = [("row".into(), Value::plain(Type::reference(entity)))].into();
            let v = self.expr(&column.value, &env, false, true, &view.id)?;
            if !v.ty.is_scalar() && !matches!(v.ty, Type::Optional { .. }) {
                return Err(refuse(
                    &view.id,
                    "history cannot expose references or list identity consumers",
                ));
            }
        }
        self.certify(&format!("view {}",view.id),"inert stored-field history, distinct retained/inductive sequence order; actual row IDs retained",view)
    }
    /// Completed and archived root fields cannot be updated by any admitted
    /// writer. Only this finite, own-row expression grammar therefore remains
    /// stable after capture; unchanged source syntax alone proves nothing about
    /// a selector that follows mutable common facts.
    fn stable_own_row(e: &Expr, binding: &str) -> bool {
        match e {
            Expr::Literal { .. } => true,
            Expr::Field { record, .. } => variable(record, binding),
            Expr::Not { value } => Self::stable_own_row(value, binding),
            Expr::Equal { left, right } | Expr::Less { left, right } => {
                Self::stable_own_row(left, binding) && Self::stable_own_row(right, binding)
            }
            Expr::And { values } | Expr::Or { values } => {
                values.iter().all(|v| Self::stable_own_row(v, binding))
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => [condition, then_value, else_value]
                .iter()
                .all(|v| Self::stable_own_row(v, binding)),
            Expr::Coalesce { value, fallback } => {
                Self::stable_own_row(value, binding) && Self::stable_own_row(fallback, binding)
            }
            _ => false,
        }
    }
    fn historical_rows(&mut self, view: &ViewDefinition, rows: &Expr) -> AdmissionResult<()> {
        let Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            limit,
            include_archived,
        } = rows
        else {
            return Err(refuse(
                &view.id,
                "historical root rows require an exact root query",
            ));
        };
        let mut row = Value::plain(Type::reference(entity));
        row.root = Some("historical-view-root".into());
        let env = [(binding.clone(), row)].into();
        let first = match predicate.as_ref() {
            Expr::And { values } => values.first(),
            other => Some(other),
        };
        let excludes_history = !include_archived
            && matches!(first, Some(Expr::Not { value }) if self.facet(value, &env).as_deref() == Some("historical-view-root"));
        if entity != &self.inventory.structure.root_entity
            || *limit != MAX_COLLECTION
            || (!Self::stable_own_row(predicate, binding) && !excludes_history)
            || !sort
                .iter()
                .all(|key| Self::stable_own_row(&key.value, binding))
        {
            return Err(refuse(
                &view.id,
                "historical root membership/order lacks stable own-row or completed-exclusion rule",
            ));
        }
        self.certify(
            &format!("view {} rows", view.id),
            "untruncated stable historical root membership/order or leading completed exclusion",
            rows,
        )
    }
    fn observations(&mut self) -> AdmissionResult<()> {
        for view in &self.app.views {
            let mut row_env = Env::new();
            match &view.kind {
                ViewKind::List {
                    entity,
                    rows,
                    columns,
                    controls,
                    selection,
                } => {
                    if matches!(self.kinds.get(entity), Some(Kind::History { .. })) {
                        self.history_view(view, entity, rows, columns)?;
                        continue;
                    }
                    if entity == &self.inventory.structure.root_entity {
                        self.historical_rows(view, rows)?;
                    }
                    let rows_value = self.expr(
                        rows,
                        &Env::new(),
                        true,
                        true,
                        &format!("view {} rows", view.id),
                    )?;
                    if !rows_value.policy.is_empty() {
                        return Err(refuse(
                            &view.id,
                            "row identity/selection depends on private policy",
                        ));
                    }
                    let mut row = rows_value.item()?;
                    if entity == &self.inventory.structure.root_entity
                        || matches!(
                            self.kinds.get(entity),
                            Some(Kind::Link { .. } | Kind::Aux { .. })
                        )
                    {
                        // The displayed link has one schema-proved immutable root owner.
                        // Its row-local owner identity is distinct from a global shared fact.
                        row.root = Some(format!("view/{}/row", view.id))
                    }
                    row_env.insert("row".into(), row.clone());
                    for column in columns {
                        let v = self.expr(
                            &column.value,
                            &row_env,
                            true,
                            true,
                            &format!("view {} column {}", view.id, column.id),
                        )?;
                        if entity == &self.inventory.structure.root_entity && v.frame_dependent {
                            return Err(refuse(
                                &format!("view {} column {}", view.id, column.id),
                                "historical time/session-dependent column is unsupported",
                            ));
                        }
                        if !v.policy.iter().all(|r| Some(r) == row.root.as_ref()) {
                            return Err(refuse(
                                &view.id,
                                "display reads another root's private policy",
                            ));
                        }
                    }
                    if controls.iter().any(|c| c.on_change.is_some()) || selection.is_some() {
                        return Err(refuse(
                            &view.id,
                            "callback/selection state ingress lacks a closed input rule",
                        ));
                    }
                }
                ViewKind::Detail { .. } => {
                    return Err(refuse(
                        &view.id,
                        "detail identity/history selection is unsupported",
                    ))
                }
                ViewKind::Form { defaults, .. } => {
                    if defaults
                        .values()
                        .any(|v| matches!(v, DataValue::Reference { .. } | DataValue::List { .. }))
                    {
                        return Err(refuse(
                            &view.id,
                            "form reference defaults lack independent provenance",
                        ));
                    }
                }
            }
            for action in &view.actions {
                let empty = Env::new();
                let env = if action.placement == ActionPlacement::Row {
                    &row_env
                } else {
                    &empty
                };
                let enabled = self.expr(
                    &action.enabled,
                    env,
                    true,
                    true,
                    &format!("view {} binding {} enabled", view.id, action.id),
                )?;
                let target_action = self
                    .app
                    .actions
                    .iter()
                    .find(|a| a.id == action.action)
                    .ok_or_else(|| refuse(&view.id, "binding target action missing"))?;
                if (self.mixed(target_action) && !enabled.policy.is_empty())
                    || (self.shared_update(&action.action) && !enabled.facets.is_empty())
                {
                    return Err(refuse(&format!("view {} binding {}", view.id, action.id),
                        "common factual UI admission depends on private policy or unrelated lifecycle"));
                }
                if !enabled
                    .policy
                    .iter()
                    .all(|r| Some(r) == env.get("row").and_then(|r| r.root.as_ref()))
                {
                    return Err(refuse(&view.id, "nonlocal policy enablement"));
                }
                for (name, arg) in &action.arguments {
                    let value = self.expr(
                        arg,
                        env,
                        false,
                        false,
                        &format!("view {} binding {} argument {name}", view.id, action.id),
                    )?;
                    if !value.policy.is_empty() || value.private_identity {
                        return Err(refuse(
                            &view.id,
                            "policy-derived scalar/reference binding laundering",
                        ));
                    }
                }
            }
        }
        for observable in &self.app.observables {
            // An inert relation count is a real common fact. Terminal histories remain
            // closed to their producer bookkeeping and one stored-field history view.
            let value = self.expr(
                &observable.value,
                &Env::new(),
                true,
                true,
                &format!("observable {}", observable.id),
            )?;
            if value.session_dependent {
                return Err(refuse(
                    &observable.id,
                    "historical session-dependent observable is unsupported",
                ));
            }
            if value.frame_dependent {
                let guarded = if let Expr::Count { items } = &observable.value {
                    if let Expr::Query {
                        entity,
                        binding,
                        predicate,
                        ..
                    } = items.as_ref()
                    {
                        let first = match predicate.as_ref() {
                            Expr::And { values } => values.first(),
                            other => Some(other),
                        };
                        let mut row = Value::plain(Type::reference(entity));
                        row.root = Some("historical-count-root".into());
                        let env = [(binding.clone(), row)].into();
                        entity == &self.inventory.structure.root_entity
                            && matches!(first, Some(Expr::Not { value }) if self.facet(value, &env).as_deref() == Some("historical-count-root"))
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !guarded {
                    return Err(refuse(
                        &observable.id,
                        "time/session historical count lacks leading completed-false short-circuit",
                    ));
                }
            }
            if !value.policy.is_empty() {
                let Expr::Count { items } = &observable.value else {
                    return Err(refuse(
                        &observable.id,
                        "private aggregate is not Count(Query(root))",
                    ));
                };
                let Expr::Query {
                    entity,
                    limit,
                    sort,
                    ..
                } = items.as_ref()
                else {
                    return Err(refuse(
                        &observable.id,
                        "private count has no exact root query",
                    ));
                };
                if entity != &self.inventory.structure.root_entity
                    || *limit != MAX_COLLECTION
                    || !sort.is_empty()
                {
                    return Err(refuse(
                        &observable.id,
                        "private count is cross-root, sorted or truncated",
                    ));
                }
            }
        }
        Ok(())
    }
}
fn projections(
    before: &AppDefinition,
    after: &AppDefinition,
    root: &str,
) -> AdmissionResult<Vec<OwnedProjectionRecipe>> {
    let mut result = vec![];
    for view in &after.views {
        if let ViewKind::List {
            entity,
            rows,
            columns,
            ..
        } = &view.kind
        {
            if entity != root {
                continue;
            }
            let original = before
                .views
                .iter()
                .find(|v| v.id == view.id)
                .ok_or_else(|| refuse(&view.id, "new root views unsupported"))?;
            let ViewKind::List {
                rows: old_rows,
                columns: old,
                ..
            } = &original.kind
            else {
                return Err(refuse(&view.id, "view kind changed"));
            };
            if rows != old_rows {
                return Err(refuse(&view.id, "row selection/identity changed"));
            }
            let env = [("row".into(), Type::reference(root))].into();
            for column in columns {
                let prior = old.iter().find(|c| c.id == column.id);
                let t = after.expression_type_in(&column.value, &env)?;
                if prior.is_none() && t != Type::Boolean {
                    return Err(refuse(
                        &format!("{}.{}", view.id, column.id),
                        "added column has no nullable Boolean availability rule",
                    ));
                }
                if let Some(prior) = prior {
                    if before.expression_type_in(&prior.value, &env)? != t {
                        return Err(refuse(&view.id, "changed projection type is unsupported"));
                    }
                }
                // Include unchanged transitive common-data displays and lists: freezing only
                // changed status fields would let completed history drift after receipt edits.
                result.push(OwnedProjectionRecipe {
                    location: format!("view/{}/{}", view.id, column.id),
                    destination: OwnedProjectionDestination::ViewColumn {
                        view: view.id.clone(),
                        column: column.id.clone(),
                    },
                    binding: "row".into(),
                    host_value_type: if prior.is_none() {
                        Type::Optional {
                            item: Box::new(t.clone()),
                        }
                    } else {
                        t.clone()
                    },
                    absent_value: prior.is_none().then_some(DataValue::Null),
                    value_type: t,
                    baseline: prior.map(|p| p.value.clone()),
                    candidate: column.value.clone(),
                    capture_required: true,
                });
            }
        }
    }
    for observable in &after.observables {
        let Expr::Count { items } = &observable.value else {
            continue;
        };
        let Expr::Query {
            entity,
            binding,
            predicate,
            sort,
            limit,
            include_archived,
        } = items.as_ref()
        else {
            continue;
        };
        if entity != root {
            continue;
        }
        if *limit != MAX_COLLECTION || !sort.is_empty() {
            return Err(refuse(
                &observable.id,
                "root count must be untruncated and unordered before mixed per-root selection",
            ));
        }
        let prior = before.observables.iter().find(|o| o.id == observable.id);
        let baseline = if let Some(prior) = prior {
            let Expr::Count { items } = &prior.value else {
                return Err(refuse(&observable.id, "count changed shape"));
            };
            let Expr::Query {
                entity: e,
                binding: b,
                predicate: p,
                sort: s,
                limit: l,
                include_archived: a,
            } = items.as_ref()
            else {
                return Err(refuse(&observable.id, "count changed query"));
            };
            if (e, b, s, l, a) != (entity, binding, sort, limit, include_archived) {
                return Err(refuse(&observable.id, "count header changed"));
            }
            Some(*p.clone())
        } else {
            None
        };
        result.push(OwnedProjectionRecipe {
            location: format!("count/{}", observable.id),
            destination: OwnedProjectionDestination::RootCount {
                observable: observable.id.clone(),
                include_archived: *include_archived,
            },
            binding: binding.clone(),
            value_type: Type::Boolean,
            host_value_type: Type::Boolean,
            absent_value: baseline
                .is_none()
                .then_some(DataValue::Boolean { value: false }),
            baseline,
            candidate: *predicate.clone(),
            capture_required: true,
        });
    }
    Ok(result)
}
pub(super) fn verify(
    snapshot: &ProjectSnapshot,
    candidate: &CapturedProgram,
    request: &OwnedPreviewRequest,
    inventory: &CommonFactInventory,
    meter: &mut ProofMeter,
) -> AdmissionResult<(
    Vec<CommonFactCertificate>,
    Vec<OwnedProjectionRecipe>,
    Vec<String>,
)> {
    let before = &snapshot.program()?.program;
    let after = &candidate.program;
    let first = lifecycle(&request.inventory.lifecycle.before)?;
    let second = lifecycle(&request.inventory.lifecycle.after)?;
    if first != second {
        return Err(refuse(
            "lifecycle",
            "before/after completion meaning differs",
        ));
    }
    for edit in &inventory.structure.edits {
        if edit.before.root.is_none() && edit.before.len > 0 && edit.after.len > 0 {
            return Err(refuse(&edit.id,
                "paired variant lacks an exact single-root correspondence; split at a proved root frontier"));
        }
    }
    let kinds = schemas(snapshot, after, request, inventory)?;
    // Every allowed LocalRuntime run has <= 10,000,000 cumulative fuel. Each
    // business write consumes fuel, so this exact retained-data margin rules out
    // policy-selected revision overflow throughout that entire isolated run.
    if snapshot
        .data
        .records
        .iter()
        .filter(|r| !r.archived)
        .any(|r| r.revision.checked_add(10_000_000).is_none())
    {
        return Err(refuse(
            "retained revisions",
            "insufficient revision range for the bounded runtime run",
        ));
    }
    let all_roots_active = !snapshot
        .data
        .records
        .iter()
        .any(|r| r.entity == inventory.structure.root_entity && r.archived);
    let mut policy = BTreeSet::new();
    for writer in &inventory.structure.writers {
        if let Some(field) = &writer.field {
            let edited = inventory
                .structure
                .edits
                .iter()
                .map(|e| {
                    if writer.side == OwnedSourceSide::Before {
                        &e.before
                    } else {
                        &e.after
                    }
                })
                .any(|e| {
                    e.action == writer.action
                        && writer.path.len() > e.list_path.len()
                        && writer.path.starts_with(&e.list_path)
                        && (e.start..e.start + e.len).contains(&writer.path[e.list_path.len()])
                });
            if edited
                && (writer.entity == inventory.structure.root_entity
                    || kinds
                        .get(&writer.entity)
                        .is_some_and(|k| k.auxiliary() && k.owner() != Some(field.as_str())))
            {
                policy.insert((writer.entity.clone(), field.clone()));
            }
        }
    }
    // The predicate's raw field remains policy-tainted everywhere except exact
    // accepted equality occurrences. This never changes the old analyzer's sets.
    policy.insert((inventory.structure.root_entity.clone(), first.0.into()));
    let certs = loop {
        let mut certs = vec![];
        let mut discovered = BTreeSet::new();
        for (app, side) in [
            (before, OwnedSourceSide::Before),
            (after, OwnedSourceSide::After),
        ] {
            let mut rules = Rules {
                app,
                inventory,
                kinds: &kinds,
                policy: &policy,
                lifecycle: first,
                side,
                meter,
                certs: vec![],
                discovered: BTreeSet::new(),
                all_roots_active,
            };
            rules.actions()?;
            rules.observations()?;
            certs.extend(rules.certs);
            discovered.extend(rules.discovered);
        }
        let previous = policy.len();
        policy.extend(discovered);
        if previous == policy.len() {
            break certs;
        }
        meter.charge(1)?;
    };
    for hazard in &inventory.identity_hazards {
        if !matches!(kinds.get(&hazard.relation), Some(Kind::History { .. })) {
            return Err(refuse(
                &format!("{} {:?}", hazard.action, hazard.common_create),
                "fresh identity shift reaches a business-consumed relation",
            ));
        }
    }
    if snapshot
        .decisions
        .decisions
        .iter()
        .any(|d| d.status == DecisionStatus::Active)
    {
        return Err(refuse("accepted decisions","active retained outcome/property comparison is unsupported by this preview proof; no obligation is silently passed"));
    }
    let projections = projections(before, after, &inventory.structure.root_entity)?;
    // Exhaustive correspondence coverage is independent of policy taint. Equal
    // common consumers still need closure, and changed passive surfaces cannot
    // disappear merely because their new expressions are literals/common facts.
    for change in &inventory.structure.changes {
        let covered = match change.kind {
            OwnedStructuralChangeKind::ColumnAdded | OwnedStructuralChangeKind::ColumnChanged =>
                projections.iter().any(|p| matches!(&p.destination,
                    OwnedProjectionDestination::ViewColumn { view, column }
                        if Some(view) == change.container.as_ref() && column == &change.id)),
            OwnedStructuralChangeKind::ObservableAdded | OwnedStructuralChangeKind::ObservableChanged =>
                projections.iter().any(|p| matches!(&p.destination,
                    OwnedProjectionDestination::RootCount { observable, .. } if observable == &change.id)),
            OwnedStructuralChangeKind::ApplicationLabelChanged | OwnedStructuralChangeKind::EntityAdded => true,
        };
        if !covered {
            return Err(refuse(
                &change.id,
                "changed observation has no supported exact projection recipe",
            ));
        }
    }
    let mut comparison = vec![];
    if !inventory.identity_hazards.is_empty() {
        comparison.push("unsupported: fresh history row IDs can differ; inert-use/order proof does not establish exact accepted observation equality".into())
    }
    if projections.iter().any(|p| {
        p.baseline.is_none()
            && matches!(p.destination, OwnedProjectionDestination::ViewColumn { .. })
    }) {
        comparison.push("unsupported: added Boolean uses Optional<Boolean> with explicit Null absence; raw Boolean/schema equality is not claimed".into())
    }
    if projections.iter().any(|p| {
        p.baseline.is_none()
            && matches!(p.destination, OwnedProjectionDestination::RootCount { .. })
    }) {
        comparison.push("unsupported: added count has no baseline observation; excluded/pre-existing sealed roots contribute zero by the disclosed eligible-member rule".into());
    }
    if comparison.is_empty() {
        comparison.push("not performed: runtime outcome comparison belongs to isolated execution, not static admission".into())
    }
    meter
        .charge((inventory.uses.len() + inventory.structure.writers.len() + certs.len()) as u64)?;
    Ok((certs, projections, comparison))
}
