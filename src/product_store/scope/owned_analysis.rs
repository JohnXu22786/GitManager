//! Typed, conservative partition/dependency analysis. No source substitution.
use super::owned_effects::{OwnedAuxiliary, OwnedRead};
use super::{OwnedSourceSide, OwnedStructuralReport};
use crate::product_contract::*;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, ContractError>;
type Fields = BTreeSet<(Id, Id)>;
type Env = BTreeMap<Id, Value>;
fn error(at: &str, reason: &str) -> ContractError {
    ContractError(format!("Owned effect safety: {at}: {reason}"))
}
#[derive(Clone, Default)]
struct Dependencies {
    roots: BTreeSet<Id>,
    global: bool,
}
impl Dependencies {
    fn merge(&mut self, other: &Self) {
        self.roots.extend(other.roots.iter().cloned());
        self.global |= other.global;
    }
    fn any(&self) -> bool {
        self.global || !self.roots.is_empty()
    }
    fn local(&self, root: Option<&str>) -> bool {
        !self.global && self.roots.iter().all(|r| Some(r.as_str()) == root)
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Origin {
    Root(Id),
    Related { root: Id, owner: Id, writable: bool },
    Shared,
}
impl Origin {
    fn root(&self) -> Option<&str> {
        match self {
            Self::Root(root) | Self::Related { root, .. } => Some(root),
            _ => None,
        }
    }
    fn writable(&self) -> bool {
        matches!(self, Self::Root(_) | Self::Related { writable: true, .. })
    }
}
#[derive(Clone)]
struct Value {
    ty: Type,
    origin: Origin,
    deps: Dependencies,
    /// A many-root collection's per-item identity, not one concrete root.
    partition: Option<Id>,
}
impl Value {
    fn plain(ty: Type) -> Self {
        Self {
            ty,
            origin: Origin::Shared,
            deps: Dependencies::default(),
            partition: None,
        }
    }
    fn item(&self) -> Result<Self> {
        let Type::List { item } = &self.ty else {
            return Err(error("collection", "not a typed list"));
        };
        Ok(Self {
            ty: *item.clone(),
            origin: self.origin.clone(),
            deps: Dependencies::default(),
            partition: None,
        })
    }
    fn scalar_dependencies(&self) -> Dependencies {
        let mut deps = self.deps.clone();
        if self
            .partition
            .as_ref()
            .is_some_and(|p| deps.roots.remove(p))
        {
            deps.global = true;
        }
        deps
    }
}
struct Analyzer<'a> {
    app: &'a AppDefinition,
    structure: &'a OwnedStructuralReport,
    auxiliary: &'a BTreeMap<Id, Id>,
    side: OwnedSourceSide,
    protected: &'a Fields,
    discovered: Fields,
    reads: Vec<OwnedRead>,
}
impl Analyzer<'_> {
    fn ty(&self, expr: &Expr, env: &Env) -> Result<Type> {
        self.app.expression_type_in(
            expr,
            &env.iter().map(|(n, v)| (n.clone(), v.ty.clone())).collect(),
        )
    }
    fn endpoint(&self, action: &str, path: &[usize]) -> Option<&super::OwnedSpanEndpoint> {
        self.structure
            .edits
            .iter()
            .map(|e| match self.side {
                OwnedSourceSide::Before => &e.before,
                OwnedSourceSide::After => &e.after,
            })
            .find(|e| {
                e.action == action
                    && path.len() > e.list_path.len()
                    && path.starts_with(&e.list_path)
                    && (e.start..e.start + e.len).contains(&path[e.list_path.len()])
            })
    }
    fn ancestor(&self, action: &str, path: &[usize]) -> bool {
        self.structure
            .edits
            .iter()
            .map(|e| match self.side {
                OwnedSourceSide::Before => &e.before,
                OwnedSourceSide::After => &e.after,
            })
            .any(|e| e.action == action && e.len > 0 && e.list_path.starts_with(path))
    }
    // Only positive, mandatory conjuncts count. No disjunction/negation inference.
    fn correlation(&self, expr: &Expr, entity: &str, binding: &str, env: &Env) -> Option<Origin> {
        match expr {
            Expr::And { values } => values
                .iter()
                .find_map(|v| self.correlation(v, entity, binding, env)),
            Expr::Equal { left, right } => {
                for (candidate, root) in [
                    (left.as_ref(), right.as_ref()),
                    (right.as_ref(), left.as_ref()),
                ] {
                    let Expr::Variable { name } = root else {
                        continue;
                    };
                    let Some(Value {
                        origin: Origin::Root(root),
                        ty,
                        ..
                    }) = env.get(name)
                    else {
                        continue;
                    };
                    if *ty != Type::reference(&self.structure.root_entity) {
                        continue;
                    }
                    if entity == self.structure.root_entity
                        && matches!(candidate,Expr::Variable { name } if name == binding)
                    {
                        return Some(Origin::Root(root.clone()));
                    }
                    if let Expr::Field { record, field } = candidate {
                        // Root records never become owned children of other roots.
                        // An auxiliary's other references are not its owner key.
                        if entity == self.structure.root_entity
                            || self
                                .auxiliary
                                .get(entity)
                                .is_some_and(|owner| owner != field)
                        {
                            continue;
                        }
                        if matches!(record.as_ref(),Expr::Variable { name } if name == binding)
                            && self
                                .app
                                .entities
                                .iter()
                                .find(|e| e.id == entity)?
                                .fields
                                .iter()
                                .any(|f| f.id == *field && f.value_type == *ty)
                        {
                            return Some(Origin::Related {
                                root: root.clone(),
                                owner: field.clone(),
                                writable: self.auxiliary.get(entity) == Some(field),
                            });
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
    fn expression(
        &mut self,
        expr: &Expr,
        env: &Env,
        owner: Option<&str>,
        strict: bool,
        partition: bool,
        at: &str,
    ) -> Result<Value> {
        let ty = self.ty(expr, env)?;
        let mut result = Value::plain(ty);
        match expr {
            Expr::Variable { name } => {
                return Ok(env.get(name).expect("validated binding").clone())
            }
            Expr::Literal { value_type, .. } => {
                if strict && matches!(value_type, Type::Reference { .. } | Type::List { .. }) {
                    return Err(error(
                        at,
                        "literal reference/collection has no ownership provenance",
                    ));
                }
            }
            Expr::Today => {}
            Expr::State { .. } => {
                if strict {
                    return Err(error(
                        at,
                        "session-dependent variant expression is unsupported",
                    ));
                }
            }
            Expr::Field { record, field } => {
                let record =
                    self.expression(record, env, owner, strict, false, &format!("{at}.record"))?;
                let Type::Reference { entity } = &record.ty else {
                    return Err(error(at, "field receiver is not a typed reference"));
                };
                result.deps = record.scalar_dependencies();
                self.reads.push(OwnedRead {
                    location: format!("{:?} {at}", self.side),
                    entity: entity.clone(),
                    field: field.clone(),
                });
                if self.protected.contains(&(entity.clone(), field.clone())) {
                    if let Some(root) = record.origin.root() {
                        result.deps.roots.insert(root.into());
                    } else {
                        result.deps.global = true;
                    }
                }
                if strict && entity == &self.structure.root_entity && record.origin.root() != owner
                {
                    return Err(error(at, "foreign root read is outside this partition"));
                }
                if let Type::Reference { .. } = result.ty {
                    if let Origin::Related {
                        root,
                        owner: owner_field,
                        ..
                    } = &record.origin
                    {
                        if field == owner_field {
                            result.origin = Origin::Root(root.clone());
                        }
                    }
                }
            }
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                ..
            } => {
                let correlation = self.correlation(predicate, entity, binding, env);
                if let Some(Origin::Related { owner: field, .. }) = &correlation {
                    // An alias can mutate the key after selection. The initial
                    // grammar requires stable correlation keys source-wide.
                    if let Some(writer) = self.structure.writers.iter().find(|w| {
                        w.side == self.side
                            && w.entity == *entity
                            && w.field.as_ref() == Some(field)
                            && w.kind != super::OwnedStatementKind::Create
                    }) {
                        return Err(error(
                            &format!("{} {:?}", writer.action, writer.path),
                            "owner transfer mutates an ownership correlation key",
                        ));
                    }
                }
                let origin = correlation.clone().unwrap_or_else(|| {
                    if entity == &self.structure.root_entity {
                        Origin::Root(format!("{at}/{binding}"))
                    } else {
                        Origin::Shared
                    }
                });
                let many_roots = entity == &self.structure.root_entity && correlation.is_none();
                if strict && many_roots && (!partition || owner.is_some()) {
                    return Err(error(
                        at,
                        "global or unrelated root query is unsupported here",
                    ));
                }
                if strict && entity != &self.structure.root_entity && origin.root().is_none() {
                    return Err(error(
                        at,
                        "query lacks mandatory exact-reference owner correlation",
                    ));
                }
                if strict && owner.is_some() && origin.root() != owner {
                    return Err(error(at, "query is correlated to a different root"));
                }
                let mut local = env.clone();
                let mut row = Value::plain(Type::reference(entity));
                row.origin = origin.clone();
                local.insert(binding.clone(), row);
                let local_owner = if many_roots {
                    origin.root()
                } else {
                    owner.or(origin.root())
                };
                let predicate = self.expression(
                    predicate,
                    &local,
                    local_owner,
                    strict,
                    false,
                    &format!("{at}.predicate"),
                )?;
                result.deps = predicate.scalar_dependencies();
                if self.auxiliary.contains_key(entity) {
                    if let Some(root) = origin.root() {
                        result.deps.roots.insert(root.into());
                    } else {
                        result.deps.global = true;
                    }
                }
                let affected = strict || result.deps.any();
                if affected && *limit != MAX_COLLECTION {
                    return Err(error(
                        at,
                        "affected query must be untruncated (MAX_COLLECTION)",
                    ));
                }
                for (index, key) in sort.iter().enumerate() {
                    let value = self.expression(
                        &key.value,
                        &local,
                        local_owner,
                        strict,
                        false,
                        &format!("{at}.sort[{index}]"),
                    )?;
                    if affected || value.deps.any() {
                        return Err(error(at, "affected query ordering is unsupported"));
                    }
                }
                if affected && !result.deps.local(origin.root()) {
                    return Err(error(
                        at,
                        "query selection depends on a foreign/global root",
                    ));
                }
                result.origin = origin;
                result.partition = many_roots.then(|| result.origin.root().unwrap().to_owned());
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
                let source_items = items;
                let items =
                    self.expression(items, env, owner, strict, partition, &format!("{at}.items"))?;
                let mut local = env.clone();
                local.insert(binding.clone(), items.item()?);
                let predicate = self.expression(
                    predicate,
                    &local,
                    owner.or(items.origin.root()),
                    strict,
                    false,
                    &format!("{at}.predicate"),
                )?;
                result.deps = items.deps.clone();
                result.deps.merge(&predicate.scalar_dependencies());
                // A later filter cannot make an earlier truncated query safe.
                if !strict && result.deps.any() {
                    self.expression(
                        source_items,
                        env,
                        owner,
                        true,
                        partition,
                        &format!("{at}.items"),
                    )?;
                }
                result.origin = items.origin;
                result.partition = items.partition;
                if !matches!(expr, Expr::Filter { .. }) {
                    result.deps = result.scalar_dependencies();
                    result.partition = None;
                    result.origin = Origin::Shared;
                }
            }
            Expr::Count { items } => {
                let items =
                    self.expression(items, env, owner, strict, partition, &format!("{at}.items"))?;
                result.deps = items.scalar_dependencies();
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let items =
                    self.expression(items, env, owner, strict, false, &format!("{at}.items"))?;
                let mut local = env.clone();
                local.insert(binding.clone(), items.item()?);
                let value =
                    self.expression(value, &local, owner, strict, false, &format!("{at}.value"))?;
                result.deps = items.scalar_dependencies();
                result.deps.merge(&value.scalar_dependencies());
                if strict || result.deps.any() {
                    return Err(error(
                        at,
                        "dependent map/binder transformation is unsupported",
                    ));
                }
            }
            _ => {
                let children: Vec<(&str, &Expr)> = match expr {
                    Expr::Sum { items } => vec![("items", items)],
                    Expr::Contains { items, value } => vec![("items", items), ("value", value)],
                    Expr::Equal { left, right }
                    | Expr::Less { left, right }
                    | Expr::Add { left, right }
                    | Expr::Subtract { left, right }
                    | Expr::Concat { left, right } => vec![("left", left), ("right", right)],
                    Expr::TextContains { text, search } => vec![("text", text), ("search", search)],
                    Expr::Lower { value } | Expr::Not { value } => vec![("value", value)],
                    Expr::And { values } | Expr::Or { values } => {
                        values.iter().map(|v| ("operand", v)).collect()
                    }
                    Expr::If {
                        condition,
                        then_value,
                        else_value,
                    } => vec![
                        ("condition", condition),
                        ("then", then_value),
                        ("else", else_value),
                    ],
                    Expr::Coalesce { value, fallback } => {
                        vec![("value", value), ("fallback", fallback)]
                    }
                    Expr::DateAdd { date, days } => vec![("date", date), ("days", days)],
                    Expr::DateDifference { later, earlier } => {
                        vec![("later", later), ("earlier", earlier)]
                    }
                    _ => unreachable!("handled expression"),
                };
                for (index, (name, child)) in children.into_iter().enumerate() {
                    result.deps.merge(
                        &self
                            .expression(
                                child,
                                env,
                                owner,
                                strict,
                                false,
                                &format!("{at}.{name}[{index}]"),
                            )?
                            .scalar_dependencies(),
                    );
                }
                if matches!(expr, Expr::Sum { .. }) && (strict || result.deps.any()) {
                    return Err(error(at, "dependent aggregate is unsupported"));
                }
            }
        }
        Ok(result)
    }
    fn write(
        &mut self,
        entity: &str,
        field: &str,
        target: &Origin,
        deps: &Dependencies,
        owner: Option<&str>,
        variant: bool,
        at: &str,
    ) -> Result<()> {
        if variant || deps.any() {
            if !target.writable() {
                return Err(error(at, "shared or unproved write target"));
            }
            if variant && target.root() != owner {
                return Err(error(at, "write targets a different root"));
            }
            if !deps.local(target.root()) {
                return Err(error(at, "foreign/global business dependency"));
            }
            self.discovered.insert((entity.into(), field.into()));
        }
        if self.auxiliary.get(entity).is_some_and(|o| o == field) {
            return Err(error(at, "auxiliary owner transfer is unsupported"));
        }
        Ok(())
    }
    fn statements(
        &mut self,
        action: &str,
        statements: &[Statement],
        path: &[usize],
        env: &mut Env,
        owner: Option<&str>,
        controls: &Dependencies,
    ) -> Result<()> {
        for (index, statement) in statements.iter().enumerate() {
            let mut path = path.to_vec();
            path.push(index);
            let at = format!("{action} {path:?}");
            let endpoint = self.endpoint(action, &path);
            let variant = endpoint.is_some();
            let explicit = endpoint
                .and_then(|e| e.root.as_ref())
                .map(|r| r.name.clone());
            let explicit_owner = explicit
                .as_ref()
                .and_then(|name| env.get(name))
                .and_then(|v| v.origin.root())
                .map(str::to_owned);
            if explicit.is_some() && explicit_owner.is_none() {
                return Err(error(&at, "explicit root lacks identity provenance"));
            }
            let owner = explicit_owner.as_deref().or(owner);
            match statement {
                Statement::ForEach {
                    items,
                    binding,
                    steps,
                } => {
                    let affected = variant || self.ancestor(action, &path);
                    let items =
                        self.expression(items, env, owner, affected, true, &format!("{at}.items"))?;
                    let mut local = env.clone();
                    local.insert(binding.clone(), items.item()?);
                    let mut control = controls.clone();
                    control.merge(&items.deps);
                    let root = items.origin.root().or(owner);
                    if (affected || items.deps.any()) && !control.local(root) {
                        return Err(error(&at, "foreign/global iteration control dependency"));
                    }
                    self.statements(action, steps, &path, &mut local, root, &control)?;
                }
                Statement::Update { record, values } => {
                    let target = self.expression(
                        record,
                        env,
                        owner,
                        variant,
                        false,
                        &format!("{at}.target"),
                    )?;
                    let Type::Reference { entity } = &target.ty else {
                        unreachable!()
                    };
                    for (field, value) in values {
                        let mut deps = controls.clone();
                        deps.merge(&target.scalar_dependencies());
                        deps.merge(
                            &self
                                .expression(
                                    value,
                                    env,
                                    owner.or(target.origin.root()),
                                    variant,
                                    false,
                                    &format!("{at}.{field}"),
                                )?
                                .scalar_dependencies(),
                        );
                        self.write(entity, field, &target.origin, &deps, owner, variant, &at)?;
                    }
                }
                Statement::Create {
                    entity,
                    values,
                    bind,
                } => {
                    let mut deps = controls.clone();
                    let mut evaluated = BTreeMap::new();
                    for (field, value) in values {
                        let value = self.expression(
                            value,
                            env,
                            owner,
                            variant,
                            false,
                            &format!("{at}.{field}"),
                        )?;
                        deps.merge(&value.scalar_dependencies());
                        evaluated.insert(field, value);
                    }
                    let origin = if let Some(field) = self.auxiliary.get(entity) {
                        let Some(Value {
                            origin: Origin::Root(root),
                            ..
                        }) = evaluated.get(field)
                        else {
                            return Err(error(&at, "auxiliary create lacks exact root owner"));
                        };
                        if Some(root.as_str()) != owner {
                            return Err(error(&at, "auxiliary create targets a different root"));
                        }
                        Origin::Related {
                            root: root.clone(),
                            owner: field.clone(),
                            writable: true,
                        }
                    } else if entity == &self.structure.root_entity {
                        if variant {
                            return Err(error(&at, "variant root creation is unsupported"));
                        }
                        Origin::Root(format!("{at}/{bind}"))
                    } else {
                        Origin::Shared
                    };
                    if variant || deps.any() {
                        if !origin.writable() || !deps.local(origin.root()) {
                            return Err(error(
                                &at,
                                "shared/global dependent create is unsupported",
                            ));
                        }
                        for field in values.keys() {
                            self.discovered.insert((entity.clone(), field.clone()));
                        }
                    }
                    let mut value = Value::plain(Type::reference(entity));
                    value.origin = origin;
                    env.insert(bind.clone(), value);
                }
                Statement::Archive { record } => {
                    let target =
                        self.expression(record, env, owner, false, false, &format!("{at}.target"))?;
                    let Type::Reference { entity } = &target.ty else {
                        unreachable!()
                    };
                    if variant
                        || self.auxiliary.contains_key(entity)
                        || controls.any()
                        || target.deps.any()
                    {
                        return Err(error(&at, "dependent/owned archival is unsupported"));
                    }
                }
                Statement::Collection { target, items, .. } => {
                    let value =
                        self.expression(items, env, owner, false, false, &format!("{at}.items"))?;
                    let mut deps = controls.clone();
                    deps.merge(&value.scalar_dependencies());
                    if let CollectionTarget::Field { record, field } = target {
                        let Type::Reference { entity } = self.ty(record, env)? else {
                            unreachable!()
                        };
                        // Insert/remove reads the old list before writing it.
                        // Track that implicit field read, not just its receiver.
                        let target = self.expression(
                            &Expr::Field {
                                record: Box::new(record.clone()),
                                field: field.clone(),
                            },
                            env,
                            owner,
                            false,
                            false,
                            &format!("{at}.target"),
                        )?;
                        deps.merge(&target.scalar_dependencies());
                        if self.auxiliary.get(&entity) == Some(field) {
                            return Err(error(&at, "owner collection mutation is unsupported"));
                        }
                    }
                    if variant || deps.any() {
                        return Err(error(&at, "dependent collection mutation is unsupported"));
                    }
                }
                Statement::SetState { value, .. } => {
                    let value =
                        self.expression(value, env, owner, false, false, &format!("{at}.value"))?;
                    if variant || controls.any() || value.deps.any() {
                        return Err(error(&at, "dependent session write is unsupported"));
                    }
                }
                Statement::Assert { condition, .. } => {
                    let value = self.expression(
                        condition,
                        env,
                        owner,
                        variant,
                        false,
                        &format!("{at}.condition"),
                    )?;
                    if variant || value.deps.any() || controls.any() {
                        return Err(error(&at, "dependent assertion is unsupported"));
                    }
                }
                Statement::Emit {
                    items,
                    binding,
                    columns,
                    ..
                } => {
                    let items =
                        self.expression(items, env, owner, false, true, &format!("{at}.items"))?;
                    let mut local = env.clone();
                    local.insert(binding.clone(), items.item()?);
                    let mut deps = controls.clone();
                    deps.merge(&items.deps);
                    for (id, value) in columns {
                        deps.merge(
                            &self
                                .expression(
                                    value,
                                    &local,
                                    items.origin.root(),
                                    false,
                                    false,
                                    &format!("{at}.{id}"),
                                )?
                                .scalar_dependencies(),
                        );
                    }
                    if variant || deps.any() {
                        return Err(error(
                            &at,
                            "dependent output requires separate ordering/history proof",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
    fn actions(&mut self) -> Result<()> {
        for action in &self.app.actions {
            let mut env: Env = action
                .parameters
                .iter()
                .map(|(name, ty)| {
                    let mut value = Value::plain(ty.clone());
                    if *ty == Type::reference(&self.structure.root_entity) {
                        value.origin = Origin::Root(format!("parameter/{name}"));
                    }
                    (name.clone(), value)
                })
                .collect();
            let mut controls = Dependencies::default();
            for (index, guard) in action.guards.iter().enumerate() {
                controls.merge(
                    &self
                        .expression(
                            guard,
                            &env,
                            None,
                            false,
                            false,
                            &format!("{} guard[{index}]", action.id),
                        )?
                        .scalar_dependencies(),
                );
            }
            self.statements(&action.id, &action.steps, &[], &mut env, None, &controls)?;
            for (index, ensure) in action.ensures.iter().enumerate() {
                if self
                    .expression(
                        ensure,
                        &env,
                        None,
                        false,
                        false,
                        &format!("{} ensure[{index}]", action.id),
                    )?
                    .deps
                    .any()
                {
                    return Err(error(&action.id, "dependent postcondition is unsupported"));
                }
            }
        }
        Ok(())
    }
    fn observations(&mut self) -> Result<()> {
        for entity in &self.app.entities {
            if entity
                .unique
                .iter()
                .flatten()
                .any(|f| self.protected.contains(&(entity.id.clone(), f.clone())))
                && !self.auxiliary.contains_key(&entity.id)
            {
                return Err(error(
                    &format!("entity {}", entity.id),
                    "protected uniqueness couples root partitions",
                ));
            }
            let mut row = Value::plain(Type::reference(&entity.id));
            if entity.id == self.structure.root_entity {
                row.origin = Origin::Root("constraint/record".into());
            }
            let env = [("record".into(), row)].into();
            for (index, expr) in entity.constraints.iter().enumerate() {
                if self
                    .expression(
                        expr,
                        &env,
                        None,
                        false,
                        false,
                        &format!("entity {} constraint[{index}]", entity.id),
                    )?
                    .deps
                    .any()
                {
                    return Err(error(
                        &entity.id,
                        "dependent schema constraint is unsupported",
                    ));
                }
            }
        }
        for view in &self.app.views {
            let mut row_env = Env::new();
            match &view.kind {
                ViewKind::List {
                    entity,
                    rows,
                    columns,
                    ..
                } => {
                    let items = self.expression(
                        rows,
                        &Env::new(),
                        None,
                        false,
                        true,
                        &format!("view {} rows", view.id),
                    )?;
                    if items.deps.any() {
                        self.expression(
                            rows,
                            &Env::new(),
                            None,
                            true,
                            true,
                            &format!("view {} rows", view.id),
                        )?;
                    }
                    let mut row = items.item()?;
                    if entity == &self.structure.root_entity {
                        row.origin = Origin::Root(format!("view {}/row", view.id));
                    }
                    row_env.insert("row".into(), row.clone());
                    for column in columns {
                        let changed =
                            self.structure.changes.iter().any(|c| {
                                c.container.as_ref() == Some(&view.id) && c.id == column.id
                            });
                        let value = self.expression(
                            &column.value,
                            &row_env,
                            row.origin.root(),
                            changed,
                            false,
                            &format!("view {} column {}", view.id, column.id),
                        )?;
                        if (changed || value.deps.any()) && !value.ty.is_scalar() {
                            return Err(error(
                                &format!("view {} column {}", view.id, column.id),
                                "affected projection must be scalar",
                            ));
                        }
                        if (changed || value.deps.any())
                            && (entity != &self.structure.root_entity
                                || !value.deps.local(row.origin.root()))
                        {
                            return Err(error(&view.id, "nonlocal changed projection"));
                        }
                        if changed || value.deps.any() {
                            self.expression(
                                rows,
                                &Env::new(),
                                None,
                                true,
                                true,
                                &format!("view {} rows", view.id),
                            )?;
                        }
                    }
                }
                ViewKind::Detail {
                    entity,
                    record,
                    columns,
                } => {
                    let value = self.expression(
                        record,
                        &Env::new(),
                        None,
                        false,
                        false,
                        &format!("view {} record", view.id),
                    )?;
                    if value.deps.any() {
                        return Err(error(
                            &format!("view {} record", view.id),
                            "dependent detail selection is unsupported",
                        ));
                    }
                    row_env.insert("row".into(), value);
                    for column in columns {
                        let value = self.expression(
                            &column.value,
                            &row_env,
                            None,
                            false,
                            false,
                            &format!("view {} column {}", view.id, column.id),
                        )?;
                        if value.deps.any()
                            || self.structure.changes.iter().any(|c| {
                                c.container.as_ref() == Some(&view.id) && c.id == column.id
                            })
                        {
                            return Err(error(
                                entity,
                                "dependent detail projection is unsupported",
                            ));
                        }
                    }
                }
                // Form defaults are literal DataValue cells, not expressions.
                ViewKind::Form { .. } => {}
            }
            for action in &view.actions {
                let env = if action.placement == ActionPlacement::Row {
                    &row_env
                } else {
                    &Env::new()
                };
                let value = self.expression(
                    &action.enabled,
                    env,
                    env.get("row").and_then(|v| v.origin.root()),
                    false,
                    false,
                    &format!("view {} action {} enabled", view.id, action.id),
                )?;
                if !value
                    .deps
                    .local(env.get("row").and_then(|v| v.origin.root()))
                {
                    return Err(error(&view.id, "nonlocal action enablement"));
                }
                for (name, expr) in &action.arguments {
                    if self
                        .expression(
                            expr,
                            env,
                            None,
                            false,
                            false,
                            &format!("view {} action {} argument {name}", view.id, action.id),
                        )?
                        .deps
                        .any()
                    {
                        return Err(error(&view.id, "dependent action argument is unsupported"));
                    }
                }
            }
        }
        for observable in &self.app.observables {
            let changed = self.structure.changes.iter().any(|c| {
                c.container.is_none()
                    && c.id == observable.id
                    && matches!(
                        c.kind,
                        super::OwnedStructuralChangeKind::ObservableAdded
                            | super::OwnedStructuralChangeKind::ObservableChanged
                    )
            });
            let at = format!("observable {}", observable.id);
            let value = self.expression(&observable.value, &Env::new(), None, false, true, &at)?;
            if changed || value.deps.any() {
                let Expr::Count { items } = &observable.value else {
                    return Err(error(
                        &at,
                        "only per-root Count(Query) observations are supported",
                    ));
                };
                let Expr::Query { entity, .. } = items.as_ref() else {
                    return Err(error(&at, "only direct Query counts are supported"));
                };
                if entity != &self.structure.root_entity {
                    return Err(error(&at, "count is not partitioned by the root"));
                }
                self.expression(items, &Env::new(), None, true, true, &at)?;
            }
        }
        Ok(())
    }
}

pub(super) fn analyze(
    before: &AppDefinition,
    after: &AppDefinition,
    structure: &OwnedStructuralReport,
    auxiliary: &[OwnedAuxiliary],
) -> Result<Vec<OwnedRead>> {
    let owners: BTreeMap<_, _> = auxiliary
        .iter()
        .map(|a| (a.entity.clone(), a.owner_field.clone()))
        .collect();
    let mut protected = Fields::new();
    for writer in &structure.writers {
        if let Some(field) = &writer.field {
            let edited = structure
                .edits
                .iter()
                .map(|e| match writer.side {
                    OwnedSourceSide::Before => &e.before,
                    OwnedSourceSide::After => &e.after,
                })
                .any(|e| {
                    e.action == writer.action
                        && writer.path.len() > e.list_path.len()
                        && writer.path.starts_with(&e.list_path)
                        && (e.start..e.start + e.len).contains(&writer.path[e.list_path.len()])
                });
            if edited {
                protected.insert((writer.entity.clone(), field.clone()));
            }
        }
    }
    for entity in &after.entities {
        if owners.contains_key(&entity.id) {
            for field in &entity.fields {
                protected.insert((entity.id.clone(), field.id.clone()));
            }
        }
    }
    // Monotone closure is bounded by the finite typed field inventory.
    loop {
        let mut discovered = Fields::new();
        let mut reads = vec![];
        for (app, side) in [
            (before, OwnedSourceSide::Before),
            (after, OwnedSourceSide::After),
        ] {
            let mut analyzer = Analyzer {
                app,
                structure,
                auxiliary: &owners,
                side,
                protected: &protected,
                discovered: Fields::new(),
                reads: vec![],
            };
            analyzer
                .actions()
                .map_err(|e| error(&format!("{side:?}"), &e.0))?;
            analyzer
                .observations()
                .map_err(|e| error(&format!("{side:?}"), &e.0))?;
            discovered.extend(analyzer.discovered);
            reads.extend(analyzer.reads);
        }
        let old = protected.len();
        protected.extend(discovered);
        if protected.len() == old {
            return Ok(reads);
        }
    }
}
