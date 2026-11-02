//! Deterministic lowering into the existing typed language. No second runtime.
use super::*;
use crate::product_contract::StringValueMap;

pub(super) fn error(message: &str) -> StoreError {
    StoreError::Invalid(format!("Scoped change unsupported: {message}. Keep current work and prepare a compatible forward repair"))
}
pub(super) fn literal(value: DataValue, value_type: Type) -> Expr {
    Expr::Literal { value_type, value }
}
pub(super) fn boolean(value: bool) -> Expr {
    literal(DataValue::Boolean { value }, Type::Boolean)
}
pub(super) fn variable(name: &str) -> Expr {
    Expr::Variable { name: name.into() }
}
pub(super) fn field(subject: &str, id: &str) -> Expr {
    Expr::Field {
        record: Box::new(variable(subject)),
        field: id.into(),
    }
}
pub(super) fn flag(subject: &str, id: &str) -> Expr {
    Expr::Coalesce {
        value: Box::new(field(subject, id)),
        fallback: Box::new(boolean(false)),
    }
}
pub(super) fn conditional(condition: Expr, then_value: Expr, else_value: Expr) -> Expr {
    Expr::If {
        condition: Box::new(condition),
        then_value: Box::new(then_value),
        else_value: Box::new(else_value),
    }
}
pub(super) fn key(layer: &Digest, entity: &str, suffix: &str) -> String {
    let digest = canonical_digest(IdentityDomain::Adoption, &(layer, entity, suffix))
        .expect("bounded serializable key");
    format!("{RESERVED_PREFIX}{}_{suffix}", &digest.as_str()[..24])
}
pub(super) fn saved_key(layer: &Digest, patch: &EffectPatch) -> Result<Id> {
    let hash = canonical_digest(IdentityDomain::Adoption, &patch.request.destination)?;
    Ok(key(
        layer,
        &patch.request.entity,
        &format!("value_{}", &hash.as_str()[..16]),
    ))
}
pub(super) fn optional(value: &Type) -> Type {
    match value {
        Type::Optional { .. } => value.clone(),
        _ => Type::Optional {
            item: Box::new(value.clone()),
        },
    }
}
fn inert(typ: &Type) -> Result<Expr> {
    Ok(match typ {
        Type::Boolean => boolean(false),
        Type::Integer => literal(DataValue::Integer { value: 0 }, Type::Integer),
        Type::Text => literal(
            DataValue::Text {
                value: String::new(),
            },
            Type::Text,
        ),
        Type::Date => literal(DataValue::Date { days: 0 }, Type::Date),
        _ => {
            return Err(error(
                "protected result must be a scalar value, optionally nullable",
            ))
        }
    })
}
pub(super) fn projection(destination: &EffectDestination) -> bool {
    matches!(
        destination,
        EffectDestination::ViewColumn { .. } | EffectDestination::EmitColumn { .. }
    )
}
pub(super) fn action(destination: &EffectDestination) -> Option<&str> {
    match destination {
        EffectDestination::Update { action, .. }
        | EffectDestination::CreateValue { action, .. }
        | EffectDestination::EmitColumn { action, .. }
        | EffectDestination::EmitItems { action, .. } => Some(action),
        _ => None,
    }
}
fn step<'a>(steps: &'a mut [Statement], path: &[usize]) -> Result<&'a mut Statement> {
    if path.is_empty() || path.len() > MAX_EXPR_DEPTH {
        return Err(error("invalid statement path"));
    }
    let value = steps
        .get_mut(path[0])
        .ok_or_else(|| error("statement path is absent"))?;
    if path.len() == 1 {
        return Ok(value);
    }
    if let Statement::ForEach { steps, .. } = value {
        step(steps, &path[1..])
    } else {
        Err(error("statement path does not traverse a loop"))
    }
}
pub(super) fn slot<'a>(
    app: &'a mut AppDefinition,
    destination: &EffectDestination,
) -> Result<&'a mut Expr> {
    match destination {
        EffectDestination::ViewColumn { view, column } => {
            let v = app
                .views
                .iter_mut()
                .find(|v| &v.id == view)
                .ok_or_else(|| error("view is absent"))?;
            let columns = match &mut v.kind {
                ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => columns,
                _ => return Err(error("form is not a row projection")),
            };
            Ok(&mut columns
                .iter_mut()
                .find(|c| &c.id == column)
                .ok_or_else(|| error("view column is absent"))?
                .value)
        }
        EffectDestination::Observable { observable } => Ok(&mut app
            .observables
            .iter_mut()
            .find(|o| &o.id == observable)
            .ok_or_else(|| error("observable is absent"))?
            .value),
        _ => {
            let (id, path) = match destination {
                EffectDestination::Update { action, path, .. }
                | EffectDestination::CreateValue { action, path, .. }
                | EffectDestination::EmitColumn { action, path, .. }
                | EffectDestination::EmitItems { action, path } => (action, path),
                _ => unreachable!(),
            };
            let a = app
                .actions
                .iter_mut()
                .find(|a| &a.id == id)
                .ok_or_else(|| error("action is absent"))?;
            match (step(&mut a.steps, path)?, destination) {
                (Statement::Update { values, .. }, EffectDestination::Update { field, .. })
                | (
                    Statement::Create { values, .. },
                    EffectDestination::CreateValue { field, .. },
                ) => values
                    .get_mut(field)
                    .ok_or_else(|| error("assignment is absent")),
                (Statement::Emit { columns, .. }, EffectDestination::EmitColumn { column, .. }) => {
                    columns
                        .get_mut(column)
                        .ok_or_else(|| error("output column is absent"))
                }
                (Statement::Emit { items, .. }, EffectDestination::EmitItems { .. }) => Ok(items),
                _ => Err(error("effect destination has the wrong statement kind")),
            }
        }
    }
}

pub(super) fn slot_fallible<'a>(
    app: &'a mut crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    destination: &super::fallible_scalar_decode::EffectDestinationDto,
) -> Result<&'a mut Expr> {
    use super::fallible_scalar_decode::EffectDestinationDto as D;
    use crate::product_contract::fallible_app_decode::{DecodedStatement as S, DecodedViewKind as V};
    match destination {
        D::ViewColumn { view, column } => {
            let view = app.views.iter_mut().find(|v| v.id == *view)
                .ok_or_else(|| error("view is absent"))?;
            let columns = match &mut view.kind {
                V::List { columns, .. } | V::Detail { columns, .. } => columns,
                V::Form { .. } => return Err(error("form is not a row projection")),
            };
            Ok(&mut columns.iter_mut().find(|v| v.id == *column)
                .ok_or_else(|| error("view column is absent"))?.value)
        }
        D::Observable { observable } => Ok(&mut app.observables.iter_mut()
            .find(|o| o.id == *observable)
            .ok_or_else(|| error("observable is absent"))?.value),
        _ => {
            let (id, path) = match destination {
                D::Update { action, path, .. } | D::CreateValue { action, path, .. }
                | D::EmitColumn { action, path, .. } | D::EmitItems { action, path } => (action, path),
                _ => unreachable!(),
            };
            if path.is_empty() || path.len() > MAX_EXPR_DEPTH {
                return Err(error("invalid statement path"));
            }
            let action = app.actions.iter_mut().find(|a| a.id == *id)
                .ok_or_else(|| error("action is absent"))?;
            let statement = decoded_step_mut(&mut action.steps, path)?;
            match (statement, destination) {
                (S::Update { values, .. }, D::Update { field, .. })
                | (S::Create { values, .. }, D::CreateValue { field, .. }) => values.get_mut(field)
                    .ok_or_else(|| error("assignment is absent")),
                (S::Emit { columns, .. }, D::EmitColumn { column, .. }) => columns.get_mut(column)
                    .ok_or_else(|| error("output column is absent")),
                (S::Emit { items, .. }, D::EmitItems { .. }) => Ok(items),
                _ => Err(error("effect destination has the wrong statement kind")),
            }
        }
    }
}

pub(super) fn slot_fallible_ref<'a>(
    app: &'a crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    destination: &super::fallible_scalar_decode::EffectDestinationDto,
) -> Result<&'a Expr> {
    use super::fallible_scalar_decode::EffectDestinationDto as D;
    use crate::product_contract::fallible_app_decode::{DecodedStatement as S, DecodedViewKind as V};
    match destination {
        D::ViewColumn { view, column } => {
            let view = app.views.iter().find(|v| v.id == *view)
                .ok_or_else(|| error("view is absent"))?;
            let columns = match &view.kind {
                V::List { columns, .. } | V::Detail { columns, .. } => columns,
                V::Form { .. } => return Err(error("form is not a row projection")),
            };
            Ok(&columns.iter().find(|v| v.id == *column)
                .ok_or_else(|| error("view column is absent"))?.value)
        }
        D::Observable { observable } => Ok(&app.observables.iter()
            .find(|o| o.id == *observable)
            .ok_or_else(|| error("observable is absent"))?.value),
        _ => {
            let (id, path) = match destination {
                D::Update { action, path, .. } | D::CreateValue { action, path, .. }
                | D::EmitColumn { action, path, .. } | D::EmitItems { action, path } => (action, path),
                _ => unreachable!(),
            };
            if path.is_empty() || path.len() > MAX_EXPR_DEPTH {
                return Err(error("invalid statement path"));
            }
            let action = app.actions.iter().find(|a| a.id == *id)
                .ok_or_else(|| error("action is absent"))?;
            let statement = decoded_step_ref(&action.steps, path)?;
            match (statement, destination) {
                (S::Update { values, .. }, D::Update { field, .. })
                | (S::Create { values, .. }, D::CreateValue { field, .. }) => values.get(field)
                    .ok_or_else(|| error("assignment is absent")),
                (S::Emit { columns, .. }, D::EmitColumn { column, .. }) => columns.get(column)
                    .ok_or_else(|| error("output column is absent")),
                (S::Emit { items, .. }, D::EmitItems { .. }) => Ok(items),
                _ => Err(error("effect destination has the wrong statement kind")),
            }
        }
    }
}

fn decoded_step_ref<'a>(
    steps: &'a [crate::product_contract::fallible_app_decode::DecodedStatement],
    path: &[usize],
) -> Result<&'a crate::product_contract::fallible_app_decode::DecodedStatement> {
    use crate::product_contract::fallible_app_decode::DecodedStatement::ForEach;
    let (&index, rest) = path.split_first().ok_or_else(|| error("invalid statement path"))?;
    let step = steps.get(index).ok_or_else(|| error("statement path is absent"))?;
    if rest.is_empty() { return Ok(step); }
    match step {
        ForEach { steps, .. } => decoded_step_ref(steps, rest),
        _ => Err(error("statement path does not traverse a loop")),
    }
}

fn decoded_step_mut<'a>(
    steps: &'a mut [crate::product_contract::fallible_app_decode::DecodedStatement],
    path: &[usize],
) -> Result<&'a mut crate::product_contract::fallible_app_decode::DecodedStatement> {
    use crate::product_contract::fallible_app_decode::DecodedStatement::ForEach;
    let (&index, rest) = path.split_first().ok_or_else(|| error("invalid statement path"))?;
    let step = steps.get_mut(index).ok_or_else(|| error("statement path is absent"))?;
    if rest.is_empty() { return Ok(step); }
    match step {
        ForEach { steps, .. } => decoded_step_mut(steps, rest),
        _ => Err(error("statement path does not traverse a loop")),
    }
}
pub(super) fn list_entity<'a, A, M>(
    expr: &'a Expr,
    app: A,
    env: &'a M,
) -> Option<&'a str>
where
    A: Into<crate::product_contract::AppRef<'a>> + Copy,
    M: StringValueMap<Type>,
{
    let app: crate::product_contract::AppRef<'a> = app.into();
    match expr {
        Expr::Query { entity, .. } => Some(entity),
        Expr::Filter { items, .. } => list_entity(items, app, env),
        Expr::State { state } => app.state_type(state).and_then(type_entity),
        Expr::Variable { name } => env.get(name).and_then(type_entity),
        Expr::Literal { value_type, .. } => type_entity(value_type),
        _ => None,
    }
}
fn type_entity(typ: &Type) -> Option<&str> {
    if let Type::List { item } = typ {
        if let Type::Reference { entity } = item.as_ref() {
            return Some(entity);
        }
    }
    None
}
#[derive(Debug)]
pub(super) enum CompilerFailure {
    Store(StoreError),
    Allocation(crate::product_alloc::AllocationFailure),
}
impl From<StoreError> for CompilerFailure {
    fn from(value: StoreError) -> Self { Self::Store(value) }
}
pub(super) enum CompilerEnvironment {
    Legacy(BTreeMap<Id, Type>),
    Fallible(crate::product_alloc::ordered::OrderedMap<Id, Type>),
}
pub(super) enum CompilerEnvironmentIter<'a> {
    Legacy(std::collections::btree_map::Iter<'a, Id, Type>),
    Fallible(std::slice::Iter<'a, (Id, Type)>),
}
impl<'a> Iterator for CompilerEnvironmentIter<'a> {
    type Item = (&'a Id, &'a Type);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Legacy(iter) => iter.next(),
            Self::Fallible(iter) => iter.next().map(|(key, value)| (key, value)),
        }
    }
}
impl ExactSizeIterator for CompilerEnvironmentIter<'_> {}
impl StringValueMap<Type> for CompilerEnvironment {
    type Iter<'a> = CompilerEnvironmentIter<'a> where Self: 'a, Type: 'a;
    fn len(&self) -> usize { match self { Self::Legacy(map) => map.len(), Self::Fallible(map) => map.len() } }
    fn get(&self, key: &str) -> Option<&Type> { match self { Self::Legacy(map) => map.get(key), Self::Fallible(map) => map.get(key) } }
    fn contains_key(&self, key: &str) -> bool { match self { Self::Legacy(map) => map.contains_key(key), Self::Fallible(map) => map.contains_key(key) } }
    fn iter(&self) -> Self::Iter<'_> {
        match self { Self::Legacy(map) => CompilerEnvironmentIter::Legacy(map.iter()), Self::Fallible(map) => CompilerEnvironmentIter::Fallible(map.iter_entries()) }
    }
}
impl CompilerEnvironment {
    fn new(fallible: bool) -> Self {
        if fallible { Self::Fallible(crate::product_alloc::ordered::OrderedMap::new()) }
        else { Self::Legacy(BTreeMap::new()) }
    }
    fn from_parameters(
        parameters: crate::product_contract::StringMapRef<'_, Type>,
        fallible: bool,
        mut cx: Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<Self, CompilerFailure> {
        let mut result = Self::new(fallible);
        for (key, value) in parameters.iter() {
            if fallible {
                let cx = cx.as_deref_mut().expect("fallible environment has allocation context");
                let key = crate::product_alloc::try_copy_str(key, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(CompilerFailure::Allocation)?;
                let value = crate::product_contract::try_clone_type_with(value, cx)
                    .map_err(CompilerFailure::Allocation)?;
                if let Self::Fallible(map) = &mut result {
                    map.try_insert(key, value, cx).map_err(CompilerFailure::Allocation)?;
                }
            } else if let Self::Legacy(map) = &mut result {
                map.insert(key.clone(), value.clone());
            }
        }
        Ok(result)
    }
    fn insert_reference(
        &mut self,
        key: &str,
        entity: &str,
        cx: Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<(), CompilerFailure> {
        match self {
            Self::Legacy(map) => { map.insert(key.to_owned(), Type::reference(entity)); }
            Self::Fallible(map) => {
                let cx = cx.expect("fallible environment has allocation context");
                let key = crate::product_alloc::try_copy_str(key, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(CompilerFailure::Allocation)?;
                let entity = crate::product_alloc::try_copy_str(entity, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(CompilerFailure::Allocation)?;
                map.try_insert(key, Type::Reference { entity }, cx).map_err(CompilerFailure::Allocation)?;
            }
        }
        Ok(())
    }
    fn insert_reference_owned(
        &mut self,
        key: &str,
        entity: String,
        cx: Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<(), CompilerFailure> {
        match self {
            Self::Legacy(map) => { map.insert(key.to_owned(), Type::Reference { entity }); }
            Self::Fallible(map) => {
                let cx = cx.expect("fallible environment has allocation context");
                let key = crate::product_alloc::try_copy_str(key, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(CompilerFailure::Allocation)?;
                map.try_insert(key, Type::Reference { entity }, cx).map_err(CompilerFailure::Allocation)?;
            }
        }
        Ok(())
    }
    fn try_clone_with(
        &self,
        cx: Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<Self, CompilerFailure> {
        match self {
            Self::Legacy(map) => Ok(Self::Legacy(map.clone())),
            Self::Fallible(map) => {
                let cx = cx.expect("fallible environment has allocation context");
                let copied = map.try_clone_with(
                    cx,
                    |key, cx| crate::product_alloc::try_copy_str(key, cx, crate::product_alloc::AllocationSite::String),
                    |value, cx| crate::product_contract::try_clone_type_with(value, cx),
                ).map_err(CompilerFailure::Allocation)?;
                Ok(Self::Fallible(copied))
            }
        }
    }
    fn remove(&mut self, key: &str) {
        match self { Self::Legacy(map) => { map.remove(key); }, Self::Fallible(map) => { map.remove(key); } }
    }
    fn into_legacy(self) -> BTreeMap<Id, Type> {
        match self { Self::Legacy(map) => map, Self::Fallible(_) => unreachable!("legacy compiler returned fallible environment") }
    }
    fn keys(&self) -> impl Iterator<Item = &String> { self.iter().map(|(key, _)| key) }
}
trait DestinationAccess {
    fn action_path(&self) -> Option<(&str, &[usize])>;
    fn view_id(&self) -> Option<&str>;
    fn is_emit_items(&self) -> bool;
    fn update_field(&self) -> Option<&str>;
    fn is_update(&self) -> bool;
    fn emit_column(&self) -> Option<(&str, &[usize], &str)>;
    fn is_projection(&self) -> bool;
}
impl DestinationAccess for EffectDestination {
    fn action_path(&self) -> Option<(&str, &[usize])> {
        match self {
            Self::Update { action, path, .. } | Self::CreateValue { action, path, .. }
            | Self::EmitColumn { action, path, .. } | Self::EmitItems { action, path } => Some((action, path)),
            _ => None,
        }
    }
    fn view_id(&self) -> Option<&str> { if let Self::ViewColumn { view, .. } = self { Some(view) } else { None } }
    fn is_emit_items(&self) -> bool { matches!(self, Self::EmitItems { .. }) }
    fn update_field(&self) -> Option<&str> {
        match self { Self::Update { field, .. } | Self::CreateValue { field, .. } => Some(field), _ => None }
    }
    fn is_update(&self) -> bool { matches!(self, Self::Update { .. }) }
    fn emit_column(&self) -> Option<(&str, &[usize], &str)> {
        if let Self::EmitColumn { action, path, column } = self { Some((action, path, column)) } else { None }
    }
    fn is_projection(&self) -> bool { matches!(self, Self::ViewColumn { .. } | Self::EmitColumn { .. }) }
}
impl DestinationAccess for super::fallible_scalar_decode::EffectDestinationDto {
    fn action_path(&self) -> Option<(&str, &[usize])> {
        use super::fallible_scalar_decode::EffectDestinationDto as D;
        match self {
            D::Update { action, path, .. } | D::CreateValue { action, path, .. }
            | D::EmitColumn { action, path, .. } | D::EmitItems { action, path } => Some((action, path)),
            _ => None,
        }
    }
    fn view_id(&self) -> Option<&str> {
        if let Self::ViewColumn { view, .. } = self { Some(view) } else { None }
    }
    fn is_emit_items(&self) -> bool { matches!(self, Self::EmitItems { .. }) }
    fn update_field(&self) -> Option<&str> {
        match self { Self::Update { field, .. } | Self::CreateValue { field, .. } => Some(field), _ => None }
    }
    fn is_update(&self) -> bool { matches!(self, Self::Update { .. }) }
    fn emit_column(&self) -> Option<(&str, &[usize], &str)> {
        if let Self::EmitColumn { action, path, column } = self { Some((action, path, column)) } else { None }
    }
    fn is_projection(&self) -> bool { matches!(self, Self::ViewColumn { .. } | Self::EmitColumn { .. }) }
}
trait PatchView {
    type Destination: DestinationAccess;
    fn entity(&self) -> &str;
    fn destination(&self) -> &Self::Destination;
}
impl PatchView for EffectPatchRequest {
    type Destination = EffectDestination;
    fn entity(&self) -> &str { &self.entity }
    fn destination(&self) -> &Self::Destination { &self.destination }
}
impl PatchView for super::EffectPatchRequestDto {
    type Destination = super::EffectDestinationDto;
    fn entity(&self) -> &str { &self.entity }
    fn destination(&self) -> &Self::Destination { &self.destination }
}
fn locate_context(
    app: &AppDefinition,
    request: &EffectPatchRequest,
) -> Result<BTreeMap<Id, Type>> {
    let app = crate::product_contract::AppRef::from(app);
    locate_context_core(
        app,
        request.destination.action_path(),
        request.destination.is_emit_items(),
        request.entity.as_str(),
        request.subject.as_str(),
        request.destination.view_id(),
        None,
    ).map(CompilerEnvironment::into_legacy)
        .map_err(|failure| match failure { CompilerFailure::Store(error) => error, CompilerFailure::Allocation(_) => unreachable!("legacy environment cannot allocate fallibly") })
}

pub(super) fn locate_context_fallible(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    request: &super::fallible_scalar_decode::EffectPatchRequestDto,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<CompilerEnvironment, CompilerFailure> {
    let app = crate::product_contract::AppRef::from(app);
    locate_context_core(
        app,
        request.destination.action_path(),
        request.destination.is_emit_items(),
        request.entity.as_str(),
        request.subject.as_str(),
        request.destination.view_id(),
        Some(cx),
    )
}

pub(super) fn rebind_fallible(
    expr: &Expr,
    from: &str,
    to: &str,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Expr, CompilerFailure> {
    clone_expr_with_rebind(expr, Some((from, to)), cx)
}

fn clone_expr_fallible(
    value: &Expr,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Expr, CompilerFailure> {
    clone_expr_with_rebind(value, None, cx)
}

fn clone_expr_with_rebind(
    expr: &Expr,
    rename: Option<(&str, &str)>,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Expr, CompilerFailure> {
    fn copy(
        expr: &Expr,
        rename: Option<(&str, &str)>,
        depth: usize,
        nodes: &mut usize,
        cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Expr, CompilerFailure> {
        use crate::product_alloc::{try_box, try_copy_str, try_push, try_reserve, AllocationSite};
        *nodes = nodes.checked_add(1).ok_or_else(|| error("expression copy invariant"))?;
        if depth > crate::product_contract::MAX_EXPR_DEPTH || *nodes > crate::product_contract::MAX_AST_NODES {
            return Err(error("expression copy invariant").into());
        }
        let string = |value: &str, cx: &mut crate::product_alloc::AllocationContext| {
            try_copy_str(value, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)
        };
        let child = |value: &Expr, nodes: &mut usize, cx: &mut crate::product_alloc::AllocationContext| {
            let value = copy(value, rename, depth + 1, nodes, cx)?;
            try_box(value, cx).map_err(CompilerFailure::Allocation)
        };
        use Expr::*;
        Ok(match expr {
            Literal { value_type, value } => Literal {
                value_type: crate::product_contract::try_clone_type_with(value_type, cx)
                    .map_err(CompilerFailure::Allocation)?,
                value: crate::product_contract::try_clone_data_value_with(value, cx)
                    .map_err(CompilerFailure::Allocation)?,
            },
            Variable { name } => {
                let name = match rename {
                    Some((from, to)) if name == from => string(to, cx)?,
                    _ => string(name, cx)?,
                };
                Variable { name }
            }
            Field { record, field } => Field {
                record: child(record, nodes, cx)?,
                field: string(field, cx)?,
            },
            State { state } => State { state: string(state, cx)? },
            Today => Today,
            Query { entity, binding, predicate, sort, limit, include_archived } => {
                let predicate = child(predicate, nodes, cx)?;
                let mut copied = Vec::new();
                try_reserve(&mut copied, sort.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                for key in sort {
                    let value = copy(&key.value, rename, depth + 1, nodes, cx)?;
                    let key = crate::product_contract::SortKey { value, descending: key.descending };
                    try_push(&mut copied, key, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                }
                Query { entity: string(entity, cx)?, binding: string(binding, cx)?, predicate, sort: copied, limit: *limit, include_archived: *include_archived }
            }
            Filter { items, binding, predicate } => Filter {
                items: child(items, nodes, cx)?, binding: string(binding, cx)?, predicate: child(predicate, nodes, cx)?,
            },
            Map { items, binding, value } => Map {
                items: child(items, nodes, cx)?, binding: string(binding, cx)?, value: child(value, nodes, cx)?,
            },
            Count { items } => Count { items: child(items, nodes, cx)? },
            Any { items, binding, predicate } => Any {
                items: child(items, nodes, cx)?, binding: string(binding, cx)?, predicate: child(predicate, nodes, cx)?,
            },
            All { items, binding, predicate } => All {
                items: child(items, nodes, cx)?, binding: string(binding, cx)?, predicate: child(predicate, nodes, cx)?,
            },
            Sum { items } => Sum { items: child(items, nodes, cx)? },
            Contains { items, value } => Contains { items: child(items, nodes, cx)?, value: child(value, nodes, cx)? },
            Equal { left, right } => Equal { left: child(left, nodes, cx)?, right: child(right, nodes, cx)? },
            Less { left, right } => Less { left: child(left, nodes, cx)?, right: child(right, nodes, cx)? },
            Add { left, right } => Add { left: child(left, nodes, cx)?, right: child(right, nodes, cx)? },
            Subtract { left, right } => Subtract { left: child(left, nodes, cx)?, right: child(right, nodes, cx)? },
            Concat { left, right } => Concat { left: child(left, nodes, cx)?, right: child(right, nodes, cx)? },
            TextContains { text, search } => TextContains { text: child(text, nodes, cx)?, search: child(search, nodes, cx)? },
            Lower { value } => Lower { value: child(value, nodes, cx)? },
            And { values } | Or { values } => {
                let mut copied = Vec::new();
                try_reserve(&mut copied, values.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                for value in values {
                    try_push(&mut copied, copy(value, rename, depth + 1, nodes, cx)?, cx, AllocationSite::Vector)
                        .map_err(CompilerFailure::Allocation)?;
                }
                if matches!(expr, And { .. }) { And { values: copied } } else { Or { values: copied } }
            }
            Not { value } => Not { value: child(value, nodes, cx)? },
            If { condition, then_value, else_value } => If {
                condition: child(condition, nodes, cx)?, then_value: child(then_value, nodes, cx)?, else_value: child(else_value, nodes, cx)?,
            },
            Coalesce { value, fallback } => Coalesce { value: child(value, nodes, cx)?, fallback: child(fallback, nodes, cx)? },
            DateAdd { date, days } => DateAdd { date: child(date, nodes, cx)?, days: child(days, nodes, cx)? },
            DateDifference { later, earlier } => DateDifference { later: child(later, nodes, cx)?, earlier: child(earlier, nodes, cx)? },
        })
    }
    let mut nodes = 0;
    copy(expr, rename, 0, &mut nodes, cx)
}

fn clone_request_fallible(
    value: &super::EffectPatchRequestDto,
    cx: &mut crate::product_alloc::AllocationContext,
 ) -> std::result::Result<super::EffectPatchRequestDto, CompilerFailure> {
    Ok(super::EffectPatchRequestDto {
        destination: clone_destination_fallible(&value.destination, cx)?,
        entity: crate::product_alloc::try_copy_str(&value.entity, cx, crate::product_alloc::AllocationSite::String)
            .map_err(CompilerFailure::Allocation)?,
        subject: crate::product_alloc::try_copy_str(&value.subject, cx, crate::product_alloc::AllocationSite::String)
            .map_err(CompilerFailure::Allocation)?,
        value_type: crate::product_contract::try_clone_type_with(&value.value_type, cx)
            .map_err(CompilerFailure::Allocation)?,
    })
}

fn clone_destination_fallible(
    value: &super::EffectDestinationDto,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<super::EffectDestinationDto, CompilerFailure> {
    use super::EffectDestinationDto as D;
    use crate::product_alloc::{try_copy_str, try_push, try_reserve, AllocationSite};
    let string = |value: &str, cx: &mut crate::product_alloc::AllocationContext| {
        try_copy_str(value, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)
    };
    let path = |value: &[usize], cx: &mut crate::product_alloc::AllocationContext|
        -> std::result::Result<Vec<usize>, CompilerFailure> {
        let mut copied = Vec::new();
        try_reserve(&mut copied, value.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
        for index in value {
            try_push(&mut copied, *index, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
        }
        Ok(copied)
    };
    Ok(match value {
        D::Update { action, path: indices, field } => D::Update { action: string(action, cx)?, path: path(indices, cx)?, field: string(field, cx)? },
        D::CreateValue { action, path: indices, field } => D::CreateValue { action: string(action, cx)?, path: path(indices, cx)?, field: string(field, cx)? },
        D::EmitColumn { action, path: indices, column } => D::EmitColumn { action: string(action, cx)?, path: path(indices, cx)?, column: string(column, cx)? },
        D::ViewColumn { view, column } => D::ViewColumn { view: string(view, cx)?, column: string(column, cx)? },
        D::EmitItems { action, path: indices } => D::EmitItems { action: string(action, cx)?, path: path(indices, cx)? },
        D::Observable { observable } => D::Observable { observable: string(observable, cx)? },
    })
}

fn clone_effect_patch_fallible(
    value: &super::DecodedEffectPatch,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<super::DecodedEffectPatch, CompilerFailure> {
    Ok(super::DecodedEffectPatch {
        request: clone_request_fallible(&value.request, cx)?,
        before: clone_expr_fallible(&value.before, cx)?,
        after: clone_expr_fallible(&value.after, cx)?,
    })
}

pub(super) fn effective_patch_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    id: &crate::product_contract::Digest,
    index: usize,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<super::DecodedEffectPatch, CompilerFailure> {
    use super::DecodedEffectPatch;
    let scope = snapshot.scalar_scope().ok_or_else(|| error("owned family cannot enter scalar scope authority"))?;
    let original = scope.layer(id).and_then(|layer| layer.patches().get(index))
        .ok_or_else(|| error("mapped layer patch absent"))?;
    let mut patch = clone_effect_patch_fallible(original, cx)?;
    if let Some(mapping) = manifest.rewrite_for(id, index) {
        let source = snapshot.captured_program_by_revision_with(mapping.to_source(), cx)
            .map_err(|validation_error| match validation_error {
                crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
                _ => CompilerFailure::Store(error("mapped source program is invalid")),
            })?
            .ok_or_else(|| error("mapped source program absent"))?;
        if std::mem::discriminant(mapping.to()) != std::mem::discriminant(&original.request().destination) {
            return Err(error("mapping changes the kind of a protected result").into());
        }
        let from = patch.request.subject.as_str();
        let before = rebind_fallible(patch.before(), &from, mapping.subject(), cx)?;
        let destination = clone_destination_fallible(mapping.to(), cx)?;
        let subject = crate::product_alloc::try_copy_str(mapping.subject(), cx, crate::product_alloc::AllocationSite::String)
            .map_err(CompilerFailure::Allocation)?;
        patch.request.destination = destination;
        patch.request.subject = subject;
        let after = clone_expr_fallible(slot_fallible_ref(source.app(), &patch.request.destination)?, cx)?;
        patch.before = before;
        patch.after = after;
        let env = locate_context_fallible(source.app(), &patch.request, cx)?;
        if !matches!(patch.request.destination, super::EffectDestinationDto::EmitItems { .. } | super::EffectDestinationDto::Observable { .. }) {
            own_row(&patch.after, &patch.request.subject, &patch.request.entity, crate::product_contract::AppRef::from(source.app()), &env, true)?;
        }
    }
    Ok(patch)
}

pub(super) fn business_program_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<crate::product_contract::fallible_app_decode::DecodedAppDefinition, CompilerFailure> {
    let scope = snapshot.scalar_scope().ok_or_else(|| error("owned family cannot enter scalar scope authority"))?;
    if let Some(manifest) = scope.composition(snapshot.active_revision()) {
        let business = snapshot.captured_program_by_revision_with(manifest.business(), cx)
            .map_err(|validation_error| match validation_error {
                crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
                _ => CompilerFailure::Store(error("business source program is invalid")),
            })?
            .ok_or_else(|| error("business source program absent"))?;
        let mut app = business.app().try_clone_with(cx).map_err(|validation_error| match validation_error {
            crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
            _ => CompilerFailure::Store(error("business app copy invariant")),
        })?;
        let mut seen = crate::product_alloc::ordered::OrderedSet::new();
        for id in manifest.layers() {
            let layer = scope.layer(id).ok_or_else(|| error("missing layer"))?;
            for index in 0..layer.patches().len() {
                let patch = effective_patch_fallible(snapshot, manifest, id, index, cx)?;
                let destination_key = crate::product_backup::fallible_codec::canonical_bytes(&patch.request.destination, cx)
                    .map_err(|codec_error| match codec_error {
                        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
                        _ => CompilerFailure::Store(error("destination identity invariant")),
                    })?;
                let first = seen.try_insert(destination_key, cx).map_err(CompilerFailure::Allocation)?;
                if first {
                    let before = clone_expr_fallible(&patch.before, cx)?;
                    *slot_fallible(&mut app, &patch.request.destination)? = before;
                }
                if manifest.active_contains(id) {
                    let after = clone_expr_fallible(&patch.after, cx)?;
                    *slot_fallible(&mut app, &patch.request.destination)? = after;
                }
            }
        }
        Ok(app)
    } else {
        let active = snapshot.captured_program_by_revision_with(snapshot.active_revision(), cx)
            .map_err(|validation_error| match validation_error {
                crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
                _ => CompilerFailure::Store(error("active source program is invalid")),
            })?
            .ok_or_else(|| error("active program revision missing"))?;
        active.app().try_clone_with(cx).map_err(|validation_error| match validation_error {
            crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
            _ => CompilerFailure::Store(error("active app copy invariant")),
        })
    }
}

fn locate_context_core(
    app: crate::product_contract::AppRef<'_>,
    action_path: Option<(&str, &[usize])>,
    is_emit_items: bool,
    requested_entity: &str,
    subject: &str,
    view_id: Option<&str>,
    mut cx: Option<&mut crate::product_alloc::AllocationContext>,
) -> std::result::Result<CompilerEnvironment, CompilerFailure> {
    if let Some(view) = view_id {
        let view_entity = app.view_entity(view).ok_or_else(|| error("view missing"))?
            .ok_or_else(|| error("form projection"))?;
        if view_entity != requested_entity || subject != "row" {
            return Err(error("view subject is not its own row").into());
        }
        let mut env = CompilerEnvironment::new(app.is_fallible());
        env.insert_reference("row", view_entity, cx.as_deref_mut())?;
        return Ok(env);
    }
    let Some((id, path)) = action_path else {
        return Ok(CompilerEnvironment::new(app.is_fallible()));
    };
    let a = app.action(id).ok_or_else(|| error("action missing"))?;
    let mut env = CompilerEnvironment::from_parameters(a.parameters(), app.is_fallible(), cx.as_deref_mut())?;
    locate_context_path(app, &mut env, a.steps(), path, is_emit_items, requested_entity, subject, cx.as_deref_mut())?;
    Ok(env)
}

fn locate_context_path(
    app: crate::product_contract::AppRef<'_>,
    env: &mut CompilerEnvironment,
    steps: crate::product_contract::StatementSlice<'_>,
    path: &[usize],
    is_emit_items: bool,
    requested_entity: &str,
    subject: &str,
    mut cx: Option<&mut crate::product_alloc::AllocationContext>,
) -> std::result::Result<(), CompilerFailure> {
    let Some((&index, rest)) = path.split_first() else {
        return Err(error("invalid statement path").into());
    };
    // Earlier Create bindings are genuine local references, never guessed.
    for previous_index in 0..index.min(steps.len()) {
        if let Some(crate::product_contract::StatementView::Create { entity, bind, .. }) = steps.get(previous_index) {
            env.insert_reference(bind, entity, cx.as_deref_mut())?;
        }
    }
    let statement = steps.get(index).ok_or_else(|| error("statement missing"))?;
    if !rest.is_empty() {
        if let crate::product_contract::StatementView::ForEach { items, binding, steps: inner } = statement {
            let loop_entity = list_entity(items, app, env).ok_or_else(|| error("loop subject cannot be proven"))?;
            let loop_entity = if let Some(cx) = cx.as_deref_mut() {
                crate::product_alloc::try_copy_str(loop_entity, cx, crate::product_alloc::AllocationSite::String)
                    .map_err(CompilerFailure::Allocation)?
            } else { loop_entity.to_owned() };
            env.insert_reference_owned(binding, loop_entity, cx.as_deref_mut())?;
            return locate_context_path(app, env, inner, rest, is_emit_items, requested_entity, subject, cx.as_deref_mut());
        }
        return Err(error("invalid nested effect path").into());
    }
    match statement {
        crate::product_contract::StatementView::Update { record, .. } => {
            if record != &variable(subject)
                || !env.get(subject).is_some_and(|typ| {
                    matches!(typ, Type::Reference { entity } if entity == requested_entity)
                })
            {
                return Err(error("update writes a different subject").into());
            }
        }
        crate::product_contract::StatementView::Create { entity, .. } => {
            if entity != requested_entity {
                return Err(error("create entity mismatch").into());
            }
        }
        crate::product_contract::StatementView::Emit { items, binding, .. } => {
            if !is_emit_items
                && (binding != subject || list_entity(items, app, env) != Some(requested_entity))
            {
                return Err(error("output subject cannot be proven").into());
            }
            env.insert_reference(binding, requested_entity, cx.as_deref_mut())?;
        }
        _ => return Err(error("unsupported effect statement").into()),
    }
    Ok(())
}
/// Check reads structurally, before any host instrumentation exists.
pub(super) fn own_row<'a, A, M>(
    expr: &Expr,
    subject: &str,
    entity: &str,
    app: A,
    env: &M,
    allow_day: bool,
) -> Result<()>
where
    A: Into<crate::product_contract::AppRef<'a>> + Copy,
    M: StringValueMap<Type>,
{
    let app: crate::product_contract::AppRef<'a> = app.into();
    match expr {
        Expr::Literal { value_type, .. } => {
            if !matches!(
                value_type,
                Type::Boolean | Type::Integer | Type::Text | Type::Date | Type::Optional { .. }
            ) {
                return Err(error("non-scalar literal in record-local effect"));
            }
        }
        Expr::Variable { name } => {
            if name == subject
                || !env.get(name).is_some_and(|t| {
                    matches!(t, Type::Boolean | Type::Integer | Type::Text | Type::Date)
                })
            {
                return Err(error("unproved scalar parameter"));
            }
        }
        Expr::Field { record, field } => {
            if record.as_ref() != &variable(subject) || field.starts_with(RESERVED_PREFIX) {
                return Err(error("foreign or protected field read"));
            }
            let typ = app
                .entity_field_type(entity, field)
                .ok_or_else(|| error("unknown subject field"))?;
            if matches!(typ, Type::Reference { .. } | Type::List { .. }) {
                return Err(error("cross-record or collection field read"));
            }
        }
        Expr::Today => {
            if !allow_day {
                return Err(error("clock-sensitive completion predicate"));
            }
        }
        Expr::Equal { left, right }
        | Expr::Less { left, right }
        | Expr::Add { left, right }
        | Expr::Subtract { left, right }
        | Expr::Concat { left, right } => {
            own_row(left, subject, entity, app, env, allow_day)?;
            own_row(right, subject, entity, app, env, allow_day)?;
        }
        Expr::TextContains { text, search } => {
            own_row(text, subject, entity, app, env, allow_day)?;
            own_row(search, subject, entity, app, env, allow_day)?;
        }
        Expr::DateAdd { date, days } => {
            own_row(date, subject, entity, app, env, allow_day)?;
            own_row(days, subject, entity, app, env, allow_day)?;
        }
        Expr::DateDifference { later, earlier } => {
            own_row(later, subject, entity, app, env, allow_day)?;
            own_row(earlier, subject, entity, app, env, allow_day)?;
        }
        Expr::Not { value } | Expr::Lower { value } => {
            own_row(value, subject, entity, app, env, allow_day)?
        }
        Expr::And { values } | Expr::Or { values } => {
            for value in values {
                own_row(value, subject, entity, app, env, allow_day)?;
            }
        }
        Expr::If {
            condition,
            then_value,
            else_value,
        } => {
            own_row(condition, subject, entity, app, env, allow_day)?;
            own_row(then_value, subject, entity, app, env, allow_day)?;
            own_row(else_value, subject, entity, app, env, allow_day)?;
        }
        Expr::Coalesce { value, fallback } => {
            own_row(value, subject, entity, app, env, allow_day)?;
            own_row(fallback, subject, entity, app, env, allow_day)?;
        }
        _ => {
            return Err(error(
                "global query, aggregation, session or cross-record effect",
            ))
        }
    }
    Ok(())
}
pub(super) fn reject_reserved(app: &AppDefinition) -> Result<()> {
    fn contains(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::String(s) => s.starts_with(RESERVED_PREFIX),
            serde_json::Value::Array(a) => a.iter().any(contains),
            serde_json::Value::Object(m) => m
                .iter()
                .any(|(k, v)| k.starts_with(RESERVED_PREFIX) || contains(v)),
            _ => false,
        }
    }
    if contains(&serde_json::to_value(app)?) {
        Err(error(
            "authored source uses the reserved metadata namespace",
        ))
    } else {
        Ok(())
    }
}
fn normalize_labels(app: &mut AppDefinition) {
    app.label.clear();
    for entity in &mut app.entities {
        entity.label.clear();
        for field in &mut entity.fields {
            field.label.clear();
        }
    }
    for state in &mut app.state {
        state.label.clear();
    }
    for action in &mut app.actions {
        action.label.clear();
    }
    for output in &mut app.outputs {
        output.label.clear();
        for field in &mut output.columns {
            field.label.clear();
        }
    }
    for observable in &mut app.observables {
        observable.label.clear();
    }
    for view in &mut app.views {
        view.label.clear();
        for action in &mut view.actions {
            action.label.clear();
        }
        match &mut view.kind {
            ViewKind::List {
                columns, controls, ..
            } => {
                for column in columns {
                    column.label.clear();
                }
                for control in controls {
                    control.label.clear();
                }
            }
            ViewKind::Detail { columns, .. } => {
                for column in columns {
                    column.label.clear();
                }
            }
            ViewKind::Form { fields, .. } => {
                for field in fields {
                    field.label.clear();
                }
            }
        }
    }
}

fn check_dependencies(app: &AppDefinition, entity_id: &str, changed: &str) -> Result<()> {
    check_dependencies_core(crate::product_contract::AppRef::from(app), entity_id, changed)
        .map_err(|failure| match failure {
            CompilerFailure::Store(error) => error,
            CompilerFailure::Allocation(_) => unreachable!("borrowed dependency inspection does not allocate"),
        })
}
fn expr_mentions_field(expr: &Expr, field: &str) -> bool {
    use Expr::*;
    match expr {
        Field { record, field: current } => current == field || expr_mentions_field(record, field),
        Query { predicate, sort, .. } => expr_mentions_field(predicate, field)
            || sort.iter().any(|key| expr_mentions_field(&key.value, field)),
        Filter { items, predicate, .. } | Any { items, predicate, .. } | All { items, predicate, .. } => {
            expr_mentions_field(items, field) || expr_mentions_field(predicate, field)
        }
        Map { items, value, .. } => expr_mentions_field(items, field) || expr_mentions_field(value, field),
        Count { items } | Sum { items } | Lower { value: items } | Not { value: items } => expr_mentions_field(items, field),
        Contains { items, value } | Equal { left: items, right: value } | Less { left: items, right: value }
        | Add { left: items, right: value } | Subtract { left: items, right: value } | Concat { left: items, right: value }
        | TextContains { text: items, search: value } | Coalesce { value: items, fallback: value } => {
            expr_mentions_field(items, field) || expr_mentions_field(value, field)
        }
        And { values } | Or { values } => values.iter().any(|value| expr_mentions_field(value, field)),
        If { condition, then_value, else_value } => expr_mentions_field(condition, field)
            || expr_mentions_field(then_value, field) || expr_mentions_field(else_value, field),
        DateAdd { date, days } => expr_mentions_field(date, field) || expr_mentions_field(days, field),
        DateDifference { later, earlier } => expr_mentions_field(later, field) || expr_mentions_field(earlier, field),
        Literal { .. } | Variable { .. } | State { .. } | Today => false,
    }
}

fn expr_foreign_read(expr: &Expr, changed: &str, subject: Option<&str>) -> bool {
    use Expr::*;
    match expr {
        Field { record, field } => {
            (field == changed && !matches!(&**record, Variable { name } if Some(name.as_str()) == subject))
                || expr_foreign_read(record, changed, subject)
        }
        Query { predicate, sort, .. } => expr_foreign_read(predicate, changed, subject)
            || sort.iter().any(|key| expr_foreign_read(&key.value, changed, subject)),
        Filter { items, predicate, .. } | Any { items, predicate, .. } | All { items, predicate, .. } => {
            expr_foreign_read(items, changed, subject) || expr_foreign_read(predicate, changed, subject)
        }
        Map { items, value, .. } => expr_foreign_read(items, changed, subject) || expr_foreign_read(value, changed, subject),
        Count { items } | Sum { items } | Lower { value: items } | Not { value: items } => expr_foreign_read(items, changed, subject),
        Contains { items, value } | Equal { left: items, right: value } | Less { left: items, right: value }
        | Add { left: items, right: value } | Subtract { left: items, right: value } | Concat { left: items, right: value }
        | TextContains { text: items, search: value } | Coalesce { value: items, fallback: value } => {
            expr_foreign_read(items, changed, subject) || expr_foreign_read(value, changed, subject)
        }
        And { values } | Or { values } => values.iter().any(|value| expr_foreign_read(value, changed, subject)),
        If { condition, then_value, else_value } => expr_foreign_read(condition, changed, subject)
            || expr_foreign_read(then_value, changed, subject) || expr_foreign_read(else_value, changed, subject),
        DateAdd { date, days } => expr_foreign_read(date, changed, subject) || expr_foreign_read(days, changed, subject),
        DateDifference { later, earlier } => expr_foreign_read(later, changed, subject) || expr_foreign_read(earlier, changed, subject),
        Literal { .. } | Variable { .. } | State { .. } | Today => false,
    }
}

fn expr_dangerous_dependency(expr: &Expr, field: &str) -> bool {
    use Expr::*;
    match expr {
        Field { record, field: current } => {
            (current == field && !matches!(&**record, Variable { .. })) || expr_dangerous_dependency(record, field)
        }
        Query { .. } | Filter { .. } | Map { .. } | Any { .. } | All { .. } | Contains { .. }
            if expr_mentions_field(expr, field) => true,
        Query { predicate, sort, .. } => expr_dangerous_dependency(predicate, field)
            || sort.iter().any(|key| expr_dangerous_dependency(&key.value, field)),
        Filter { items, predicate, .. } | Any { items, predicate, .. } | All { items, predicate, .. } => {
            expr_dangerous_dependency(items, field) || expr_dangerous_dependency(predicate, field)
        }
        Map { items, value, .. } => expr_dangerous_dependency(items, field) || expr_dangerous_dependency(value, field),
        Count { items } | Sum { items } | Lower { value: items } | Not { value: items } => expr_dangerous_dependency(items, field),
        Contains { items, value } | Equal { left: items, right: value } | Less { left: items, right: value }
        | Add { left: items, right: value } | Subtract { left: items, right: value } | Concat { left: items, right: value }
        | TextContains { text: items, search: value } | Coalesce { value: items, fallback: value } => {
            expr_dangerous_dependency(items, field) || expr_dangerous_dependency(value, field)
        }
        And { values } | Or { values } => values.iter().any(|value| expr_dangerous_dependency(value, field)),
        If { condition, then_value, else_value } => expr_dangerous_dependency(condition, field)
            || expr_dangerous_dependency(then_value, field) || expr_dangerous_dependency(else_value, field),
        DateAdd { date, days } => expr_dangerous_dependency(date, field) || expr_dangerous_dependency(days, field),
        DateDifference { later, earlier } => expr_dangerous_dependency(later, field) || expr_dangerous_dependency(earlier, field),
        Literal { .. } | Variable { .. } | State { .. } | Today => false,
    }
}

fn statement_dangerous_dependency(steps: crate::product_contract::StatementSlice<'_>, field: &str) -> bool {
    use crate::product_contract::StatementView as S;
    for index in 0..steps.len() {
        match steps.get(index).expect("bounded statement index") {
            S::SetState { value, .. } if expr_mentions_field(value, field) => return true,
            S::Collection { target, items, .. } => {
                let target_dep = match target {
                    CollectionTarget::Field { record, field: target_field } => target_field == field || expr_mentions_field(record, field),
                    CollectionTarget::State { .. } => false,
                };
                if target_dep || expr_mentions_field(items, field) { return true; }
            }
            S::ForEach { steps, .. } if statement_dangerous_dependency(steps, field) => return true,
            _ => {}
        }
    }
    false
}

fn sink_dependencies(steps: crate::product_contract::StatementSlice<'_>, changed: &str) -> bool {
    use crate::product_contract::StatementView as S;
    for index in 0..steps.len() {
        match steps.get(index).expect("bounded statement index") {
            S::Update { record, values } => {
                if expr_foreign_read(record, changed, None) { return true; }
                let subject = match record { Expr::Variable { name } => Some(name.as_str()), _ => None };
                if values.iter().any(|(field, value)| expr_foreign_read(value, changed, subject)
                    || (field != changed && expr_mentions_field(value, changed))) { return true; }
            }
            S::Create { values, .. } if values.iter().any(|(_, value)| expr_foreign_read(value, changed, None)) => return true,
            S::SetState { value, .. } | S::Assert { condition: value, .. } | S::Archive { record: value }
                if expr_foreign_read(value, changed, None) => return true,
            S::ForEach { items, steps, .. } if expr_foreign_read(items, changed, None) || sink_dependencies(steps, changed) => return true,
            S::Emit { items, binding, columns, .. } if expr_foreign_read(items, changed, None)
                || columns.iter().any(|(_, value)| expr_foreign_read(value, changed, Some(binding))) => return true,
            _ => {}
        }
    }
    false
}

fn check_dependencies_fallible(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    entity_id: &str,
    changed: &str,
) -> std::result::Result<(), CompilerFailure> {
    check_dependencies_core(crate::product_contract::AppRef::from(app), entity_id, changed)
}
fn check_dependencies_core(
    app: crate::product_contract::AppRef<'_>,
    entity_id: &str,
    changed: &str,
) -> std::result::Result<(), CompilerFailure> {
    let entity = app.entities().iter().find(|entity| entity.id == entity_id)
        .ok_or_else(|| error("effect entity absent"))?;
    let unique_dep = entity.unique.iter().flatten().any(|field| field == changed);
    let constraint_dep = entity.constraints.iter().any(|expr| expr_mentions_field(expr, changed));
    let mut dangerous_dep = false;
    app.visit_expressions(&mut |expr| dangerous_dep |= expr_dangerous_dependency(expr, changed));
    for action in app.actions().iter() {
        dangerous_dep |= statement_dangerous_dependency(action.steps(), changed);
    }
    let mut view_dep = false;
    app.visit_view_action_expressions(&mut |expr| view_dep |= expr_foreign_read(expr, changed, None));
    let action_dep = app.actions().iter().any(|action| {
        sink_dependencies(action.steps(), changed)
            || action.guards().iter().chain(action.ensures()).any(|expr| expr_foreign_read(expr, changed, None))
    });
    if unique_dep || constraint_dep || dangerous_dep || view_dep || action_dep {
        return Err(error("changed durable value influences another stored field, a foreign-row write, shared selection, state, constraint or uniqueness rule").into());
    }
    Ok(())
}
fn check_durable_writers(app: &AppDefinition, patches: &[EffectPatchRequest]) -> Result<()> {
    check_durable_writers_core(crate::product_contract::AppRef::from(app), patches, None)
        .map_err(|failure| match failure {
            CompilerFailure::Store(error) => error,
            CompilerFailure::Allocation(_) => unreachable!("legacy projection does not allocate fallibly"),
        })
}
fn check_durable_writers_fallible(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    patches: &[super::EffectPatchRequestDto],
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), CompilerFailure> {
    check_durable_writers_core(crate::product_contract::AppRef::from(app), patches, Some(cx))
}
fn check_durable_writers_core<P: PatchView>(
    app: crate::product_contract::AppRef<'_>,
    patches: &[P],
    mut cx: Option<&mut crate::product_alloc::AllocationContext>,
) -> std::result::Result<(), CompilerFailure> {
    use crate::product_contract::StatementView as S;
    fn visit<P: PatchView>(
        app: crate::product_contract::AppRef<'_>,
        action: &str,
        steps: crate::product_contract::StatementSlice<'_>,
        path: &mut Vec<usize>,
        mut env: CompilerEnvironment,
        patches: &[P],
        mut cx: Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<(), CompilerFailure> {
        for index in 0..steps.len() {
            if let Some(cx) = cx.as_deref_mut() {
                crate::product_alloc::try_push(path, index, cx, crate::product_alloc::AllocationSite::Vector)
                    .map_err(CompilerFailure::Allocation)?;
            } else { path.push(index); }
            let statement = steps.get(index).expect("bounded statement index");
            match statement {
                S::Update { record, values } => {
                    let entity = match record {
                        Expr::Variable { name } => match env.get(name) { Some(Type::Reference { entity }) => Some(entity.as_str()), _ => None },
                        _ => None,
                    };
                    for protected in patches {
                        let Some(field) = protected.destination().update_field() else { continue; };
                        if values.contains_key(field)
                            && entity.is_none_or(|entity| entity == protected.entity())
                            && !patches.iter().any(|candidate| {
                                candidate.entity() == protected.entity()
                                    && candidate.destination().action_path().is_some_and(|(candidate_action, candidate_path)| candidate_action == action && candidate_path == path)
                                    && candidate.destination().is_update()
                                    && candidate.destination().update_field() == Some(field)
                            })
                        {
                            return Err(error("a durable result has an uncovered writer; explicitly reconcile every operation that can overwrite completed work").into());
                        }
                    }
                }
                S::ForEach { items, binding, steps } => {
                    let mut inner = env.try_clone_with(cx.as_deref_mut())?;
                    if let Some(entity) = list_entity(items, app, &env) {
                        inner.insert_reference(binding, entity, cx.as_deref_mut())?;
                    } else {
                        inner.remove(binding);
                    }
                    visit(app, action, steps, path, inner, patches, cx.as_deref_mut())?;
                }
                S::Create { entity, bind, .. } => env.insert_reference(bind, entity, cx.as_deref_mut())?,
                _ => {}
            }
            path.pop();
        }
        Ok(())
    }
    let app_ref = crate::product_contract::AppRef::from(app);
    let actions = app_ref.actions();
    for action in actions.iter() {
        let env = CompilerEnvironment::from_parameters(action.parameters(), app_ref.is_fallible(), cx.as_deref_mut())?;
        let mut path = Vec::new();
        visit(app_ref, action.id(), action.steps(), &mut path, env, patches, cx.as_deref_mut())?;
    }
    Ok(())
}
fn check_completion_order(app: &AppDefinition) -> Result<()> {
    check_completion_order_ref(crate::product_contract::AppRef::from(app))
}
fn check_completion_order_ref(app: crate::product_contract::AppRef<'_>) -> Result<()> {
    use crate::product_contract::StatementView as S;
    fn visit(steps: crate::product_contract::StatementSlice<'_>, mut emitted: bool) -> Result<bool> {
        for index in 0..steps.len() {
            match steps.get(index).expect("bounded statement index") {
                S::Emit { .. } => emitted = true,
                S::ForEach { steps, .. } => {
                    let nested = visit(steps, emitted)?;
                    if nested && !emitted { visit(steps, true)?; }
                    emitted |= nested;
                }
                S::Create { .. } | S::Update { .. } | S::Archive { .. } if emitted => {
                    return Err(error("a business write follows output; completion capture requires a compatible final-output action"));
                }
                S::Collection { target, .. } if emitted && matches!(target, CollectionTarget::Field { .. }) => {
                    return Err(error("a business write follows output; completion capture requires a compatible final-output action"));
                }
                _ => {}
            }
        }
        Ok(emitted)
    }
    for action in app.actions().iter() { visit(action.steps(), false)?; }
    Ok(())
}
fn check_output_coverage(app: &AppDefinition, patches: &[EffectPatchRequest]) -> Result<()> {
    check_output_coverage_core(crate::product_contract::AppRef::from(app), patches, None)
        .map_err(|failure| match failure {
            CompilerFailure::Store(error) => error,
            CompilerFailure::Allocation(_) => unreachable!("legacy projection does not allocate fallibly"),
        })
}

struct EmitSite<'a> { action: &'a str, path: Vec<usize>, output: &'a str }
fn check_output_coverage_fallible(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    patches: &[super::EffectPatchRequestDto],
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), CompilerFailure> {
    check_output_coverage_core(crate::product_contract::AppRef::from(app), patches, Some(cx))
}
fn check_output_coverage_core<P: PatchView>(
    app: crate::product_contract::AppRef<'_>,
    patches: &[P],
    mut cx: Option<&mut crate::product_alloc::AllocationContext>,
) -> std::result::Result<(), CompilerFailure> {
    use crate::product_contract::StatementView as S;
    fn sites<'a>(
        action: &'a str,
        steps: crate::product_contract::StatementSlice<'a>,
        path: &mut Vec<usize>,
        out: &mut Vec<EmitSite<'a>>,
        cx: &mut Option<&mut crate::product_alloc::AllocationContext>,
    ) -> std::result::Result<(), CompilerFailure> {
        for index in 0..steps.len() {
            let statement = steps.get(index).expect("bounded statement index");
            if let Some(cx) = cx.as_deref_mut() {
                crate::product_alloc::try_push(path, index, cx, crate::product_alloc::AllocationSite::Vector)
                    .map_err(CompilerFailure::Allocation)?;
            } else { path.push(index); }
            match statement {
                S::Emit { output, .. } => {
                    let mut saved_path = Vec::new();
                    if let Some(cx) = cx.as_deref_mut() {
                        crate::product_alloc::try_reserve(&mut saved_path, path.len(), cx, crate::product_alloc::AllocationSite::Vector)
                            .map_err(CompilerFailure::Allocation)?;
                        saved_path.extend_from_slice(path);
                        crate::product_alloc::try_reserve(out, 1, cx, crate::product_alloc::AllocationSite::Vector)
                            .map_err(CompilerFailure::Allocation)?;
                    } else {
                        saved_path.extend_from_slice(path);
                        out.try_reserve(1).map_err(|_| CompilerFailure::Store(error("compiler output-site allocation failed")))?;
                    }
                    out.push(EmitSite { action, path: saved_path, output });
                }
                S::ForEach { steps, .. } => sites(action, steps, path, out, cx)?,
                _ => {}
            }
            path.pop();
        }
        Ok(())
    }
    let mut emitted = Vec::new();
    let mut path = Vec::new();
    let actions = app.actions();
    for action in actions.iter() {
        sites(action.id(), action.steps(), &mut path, &mut emitted, &mut cx)?;
    }
    for patch in patches {
        let Some((action, wanted_path, column)) = patch.destination().emit_column() else { continue; };
        let (_, output) = emitted.iter().find(|site| site.action == action && site.path == wanted_path)
            .map(|site| (site, site.output))
            .ok_or_else(|| error("protected output site absent"))?;
        for other in &emitted {
            if other.output == output && !patches.iter().any(|candidate| {
                candidate.entity() == patch.entity()
                    && candidate.destination().emit_column().is_some_and(|(a, p, c)| a == other.action && p == other.path && c == column)
            }) {
                return Err(error("a shared output has an unprotected emission; separate its output definition or explicitly cover every affected result").into());
            }
        }
    }
    Ok(())
}
pub(super) fn derive_patches(
    baseline: &AppDefinition,
    candidate: &AppDefinition,
    request: &ScopeRequest,
) -> Result<Vec<EffectPatch>> {
    reject_reserved(candidate)?;
    if request.patches.is_empty()
        || request.patches.len() > MAX_ITEMS
        || request.lifecycles.len() > MAX_ITEMS
    {
        return Err(error("empty or oversized effect specification"));
    }
    if !request.lifecycles.is_empty() {
        check_completion_order(baseline)?;
        check_completion_order(candidate)?;
    }
    check_output_coverage(baseline, &request.patches)?;
    check_output_coverage(candidate, &request.patches)?;
    check_durable_writers(baseline, &request.patches)?;
    check_durable_writers(candidate, &request.patches)?;
    let mut masked = baseline.clone();
    let mut seen = BTreeSet::new();
    let mut patches = vec![];
    for p in &request.patches {
        if !seen.insert(canonical_bytes(&p.destination)?) {
            return Err(error("duplicate effect destination"));
        }
        if let Some(action) = action(&p.destination) {
            if !request.operations.contains(action) {
                return Err(error("effect falls outside the named operations"));
            }
        }
        let env = locate_context(baseline, p)?;
        locate_context(candidate, p)?;
        let before = slot(&mut baseline.clone(), &p.destination)?.clone();
        let after = slot(&mut candidate.clone(), &p.destination)?.clone();
        let global = matches!(
            p.destination,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        );
        if global {
            if request.population != ScopePopulation::All || !request.excluded_records.is_empty() {
                return Err(error(
                    "global operation expression needs the whole population",
                ));
            }
        } else {
            own_row(&before, &p.subject, &p.entity, baseline, &env, true)?;
            own_row(&after, &p.subject, &p.entity, candidate, &env, true)?;
            if !request.lifecycles.iter().any(|l| l.entity == p.entity) {
                return Err(error("row effect needs an explicit completed-work meaning"));
            }
            let typ = match &p.value_type {
                Type::Optional { item } => item.as_ref(),
                typ => typ,
            };
            inert(typ)?;
            if projection(&p.destination)
                && env.iter().any(|(id, _)| {
                    id != &p.subject
                        && (expression_variable(&before, id) || expression_variable(&after, id))
                })
            {
                return Err(error("saved projection depends on an action parameter"));
            }
        }
        if let EffectDestination::Update { field, .. }
        | EffectDestination::CreateValue { field, .. } = &p.destination
        {
            for app in [baseline, candidate] {
                check_dependencies(app, &p.entity, field)?;
            }
        }
        *slot(&mut masked, &p.destination)? = after.clone();
        patches.push(EffectPatch {
            request: p.clone(),
            before,
            after,
        });
    }
    let mut target = candidate.clone();
    normalize_labels(&mut masked);
    normalize_labels(&mut target);
    if masked != target {
        return Err(error(
            "partial adoption changes the action/view shell outside enumerated effect expressions",
        ));
    }
    Ok(patches)
}

/// Derive protected result patches over the bounded fallible graph while
/// sharing every semantic check with the legacy compiler. All temporary app,
/// request, patch and destination owners are charged to the caller's context;
/// no fallible owner is materialized as a BTreeMap-backed model.
pub(super) fn derive_patches_fallible(
    baseline: &crate::product_contract::FallibleDecodedCapturedProgram,
    candidate: &crate::product_contract::FallibleDecodedCapturedProgram,
    request: &super::fallible_scalar_decode::DecodedScopeRequest,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Vec<super::DecodedEffectPatch>, CompilerFailure> {
    use crate::product_contract::fallible_app_decode::DecodedAppDefinition;
    use crate::product_contract::AppValidationError;
    use crate::product_store::scope::fallible_scalar_decode::ScopePopulationDto;
    use crate::product_alloc::{try_push, AllocationSite};

    fn app_error(failure: AppValidationError) -> CompilerFailure {
        match failure {
            AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
            AppValidationError::Contract(error) => CompilerFailure::Store(StoreError::Invalid(error.0)),
        }
    }
    fn codec_error(failure: crate::product_backup::fallible_codec::CodecError) -> CompilerFailure {
        match failure {
            crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
            _ => CompilerFailure::Store(error("compiler codec invariant")),
        }
    }
    fn values_contain_reserved(value: &crate::product_json::Value) -> bool {
        use crate::product_json::Value;
        match value {
            Value::String(value) => value.starts_with(RESERVED_PREFIX),
            Value::Array(values) => values.iter().any(values_contain_reserved),
            Value::Object(values) => values.iter().any(|(key, value)| {
                key.starts_with(RESERVED_PREFIX) || values_contain_reserved(value)
            }),
            _ => false,
        }
    }

    // Preserve the legacy reserved-namespace walk over the exact Serde graph,
    // but build that temporary tree through the bounded private serializer.
    let value = crate::product_backup::fallible_codec::try_serialize_value(candidate.app(), cx)
        .map_err(codec_error)?;
    if values_contain_reserved(&value) {
        return Err(error("authored source uses the reserved metadata namespace").into());
    }

    if request.patches.is_empty()
        || request.patches.len() > MAX_ITEMS
        || request.lifecycles.len() > MAX_ITEMS
    {
        return Err(error("empty or oversized effect specification").into());
    }
    if !request.lifecycles.is_empty() {
        check_completion_order_ref(crate::product_contract::AppRef::from(baseline.app()))?;
        check_completion_order_ref(crate::product_contract::AppRef::from(candidate.app()))?;
    }
    check_output_coverage_fallible(baseline.app(), request.patches(), cx)?;
    check_output_coverage_fallible(candidate.app(), request.patches(), cx)?;
    check_durable_writers_fallible(baseline.app(), request.patches(), cx)?;
    check_durable_writers_fallible(candidate.app(), request.patches(), cx)?;

    let mut masked: DecodedAppDefinition = baseline.app().try_clone_with(cx).map_err(app_error)?;
    let mut seen = crate::product_alloc::ordered::OrderedSet::new();
    let mut patches = Vec::new();
    crate::product_alloc::try_reserve(&mut patches, request.patches().len(), cx, AllocationSite::Vector)
        .map_err(CompilerFailure::Allocation)?;
    for p in request.patches() {
        let destination = crate::product_backup::fallible_codec::canonical_bytes(&p.destination, cx)
            .map_err(codec_error)?;
        if !seen.try_insert(destination, cx).map_err(CompilerFailure::Allocation)? {
            return Err(error("duplicate effect destination").into());
        }
        if let Some((action, _)) = p.destination.action_path() {
            if !request.operations.iter().any(|operation| operation == action) {
                return Err(error("effect falls outside the named operations").into());
            }
        }
        let baseline_env = locate_context_fallible(baseline.app(), p, cx)?;
        let _candidate_env = locate_context_fallible(candidate.app(), p, cx)?;
        let before = clone_expr_fallible(slot_fallible_ref(baseline.app(), &p.destination)?, cx)?;
        let after = clone_expr_fallible(slot_fallible_ref(candidate.app(), &p.destination)?, cx)?;
        let global = matches!(
            &p.destination,
            super::fallible_scalar_decode::EffectDestinationDto::EmitItems { .. }
                | super::fallible_scalar_decode::EffectDestinationDto::Observable { .. }
        );
        if global {
            if !matches!(&request.population, ScopePopulationDto::All)
                || !request.excluded_records.is_empty()
            {
                return Err(error("global operation expression needs the whole population").into());
            }
        } else {
            own_row(&before, &p.subject, &p.entity, crate::product_contract::AppRef::from(baseline.app()), &baseline_env, true)?;
            own_row(&after, &p.subject, &p.entity, crate::product_contract::AppRef::from(candidate.app()), &_candidate_env, true)?;
            if !request.lifecycles().iter().any(|lifecycle| lifecycle.entity() == p.entity) {
                return Err(error("row effect needs an explicit completed-work meaning").into());
            }
            let typ = match &p.value_type { Type::Optional { item } => item.as_ref(), typ => typ };
            inert(typ)?;
            if p.destination.is_projection()
                && baseline_env.iter().any(|(id, _)| {
                    id != &p.subject && (expression_variable(&before, id) || expression_variable(&after, id))
                })
            {
                return Err(error("saved projection depends on an action parameter").into());
            }
        }
        if let Some(field) = p.destination.update_field() {
            check_dependencies_fallible(baseline.app(), &p.entity, field)?;
            check_dependencies_fallible(candidate.app(), &p.entity, field)?;
        }
        *slot_fallible(&mut masked, &p.destination)? = clone_expr_fallible(&after, cx)?;
        let cloned = clone_request_fallible(p, cx)?;
        try_push(&mut patches, super::DecodedEffectPatch { request: cloned, before, after }, cx, AllocationSite::Vector)
            .map_err(CompilerFailure::Allocation)?;
    }

    masked.clear_display_labels();
    let mut target = candidate.app().try_clone_with(cx).map_err(app_error)?;
    target.clear_display_labels();
    let masked = crate::product_backup::fallible_codec::canonical_bytes(&masked, cx).map_err(codec_error)?;
    let target = crate::product_backup::fallible_codec::canonical_bytes(&target, cx).map_err(codec_error)?;
    if masked != target {
        return Err(error("partial adoption changes the action/view shell outside enumerated effect expressions").into());
    }
    Ok(patches)
}
fn expression_variable(expr: &Expr, id: &str) -> bool {
    use Expr::*;
    match expr {
        Variable { name } => name == id,
        Field { record, .. } | Count { items: record } | Sum { items: record }
        | Lower { value: record } | Not { value: record } => expression_variable(record, id),
        Query { predicate, sort, .. } => expression_variable(predicate, id) || sort.iter().any(|key| expression_variable(&key.value, id)),
        Filter { items, predicate, .. } | Any { items, predicate, .. } | All { items, predicate, .. } => {
            expression_variable(items, id) || expression_variable(predicate, id)
        }
        Map { items, value, .. } => expression_variable(items, id) || expression_variable(value, id),
        Contains { items, value } | Equal { left: items, right: value }
        | Less { left: items, right: value } | Add { left: items, right: value }
        | Subtract { left: items, right: value } | Concat { left: items, right: value }
        | TextContains { text: items, search: value } | Coalesce { value: items, fallback: value } => {
            expression_variable(items, id) || expression_variable(value, id)
        }
        And { values } | Or { values } => values.iter().any(|value| expression_variable(value, id)),
        If { condition, then_value, else_value } => expression_variable(condition, id)
            || expression_variable(then_value, id) || expression_variable(else_value, id),
        DateAdd { date, days } => expression_variable(date, id) || expression_variable(days, id),
        DateDifference { later, earlier } => expression_variable(later, id) || expression_variable(earlier, id),
        Literal { .. } | State { .. } | Today => false,
    }
}
pub(super) fn rebind(expr: &Expr, from: &str, to: &str) -> Result<Expr> {
    // Accepted record-local expressions contain no binders; there is no capture.
    fn visit(v: &mut serde_json::Value, from: &str, to: &str) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("kind").and_then(|v| v.as_str()) == Some("variable")
                    && m.get("name").and_then(|v| v.as_str()) == Some(from)
                {
                    m.insert("name".into(), to.into());
                }
                for child in m.values_mut() {
                    visit(child, from, to)
                }
            }
            serde_json::Value::Array(a) => {
                for child in a {
                    visit(child, from, to)
                }
            }
            _ => {}
        }
    }
    let mut v = serde_json::to_value(expr)?;
    visit(&mut v, from, to);
    Ok(serde_json::from_value(v)?)
}
pub(super) fn effective_patch(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    id: &Digest,
    index: usize,
) -> Result<EffectPatch> {
    let original = snapshot
        .scalar_scope()?
        .layers
        .get(id)
        .and_then(|l| l.patches.get(index))
        .ok_or_else(|| error("mapped layer patch absent"))?;
    let mut patch = original.clone();
    if let Some(mapping) = manifest
        .rewrites
        .iter()
        .find(|m| &m.layer == id && m.patch == index)
    {
        let source = program(snapshot, &mapping.to_source)?;
        if std::mem::discriminant(&mapping.to)
            != std::mem::discriminant(&original.request.destination)
        {
            return Err(error("mapping changes the kind of a protected result"));
        }
        patch.before = rebind(&patch.before, &patch.request.subject, &mapping.subject)?;
        patch.request.destination = mapping.to.clone();
        patch.request.subject = mapping.subject.clone();
        patch.after = slot(&mut source.program.clone(), &mapping.to)?.clone();
        let env = locate_context(&source.program, &patch.request)?;
        if !matches!(
            mapping.to,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        ) {
            own_row(
                &patch.after,
                &patch.request.subject,
                &patch.request.entity,
                &source.program,
                &env,
                true,
            )?;
        }
    }
    Ok(patch)
}
pub(super) fn validate_mappings(
    snapshot: &ProjectSnapshot,
    previous: &CompositionManifest,
    candidate: &CapturedProgram,
    mappings: &[ScopeSlotMapping],
) -> Result<()> {
    let state = snapshot.scalar_scope()?;
    let mut seen = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for mapping in mappings {
        if mapping.from_source != previous.output
            || mapping.to_source != revision(candidate)?
            || !previous.layers.contains(&mapping.layer)
            || !seen.insert((&mapping.layer, mapping.patch))
        {
            return Err(error("ambiguous, stale or foreign protected-slot mapping"));
        }
        let current = effective_patch(snapshot, previous, &mapping.layer, mapping.patch)?;
        if mapping.from != current.request.destination {
            return Err(error(
                "protected-slot mapping does not name the current source slot",
            ));
        }
        match (&current.request.destination, &mapping.to) {
            (
                EffectDestination::Update { field: from, .. },
                EffectDestination::Update { field: to, .. },
            )
            | (
                EffectDestination::CreateValue { field: from, .. },
                EffectDestination::CreateValue { field: to, .. },
            ) if from != to => {
                return Err(error(
                    "durable field retargeting needs an explicit preservation migration",
                ));
            }
            _ => {}
        }
        if !destinations.insert(canonical_bytes(&mapping.to)?) {
            return Err(error(
                "overlapping protected slots need an explicit compatible reconciliation",
            ));
        }
        let mut p = current.request.clone();
        p.destination = mapping.to.clone();
        p.subject = mapping.subject.clone();
        let env = locate_context(&candidate.program, &p)?;
        let expression = slot(&mut candidate.program.clone(), &mapping.to)?.clone();
        if !matches!(
            mapping.to,
            EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
        ) {
            own_row(
                &expression,
                &p.subject,
                &p.entity,
                &candidate.program,
                &env,
                true,
            )?;
            if projection(&p.destination)
                && env
                    .keys()
                    .any(|id| id != &p.subject && expression_variable(&expression, id))
            {
                return Err(error(
                    "mapped saved projection depends on an action parameter",
                ));
            }
        }
    }
    if previous
        .layers
        .iter()
        .any(|id| !state.layers[id].request.lifecycles.is_empty())
    {
        check_completion_order(&candidate.program)?;
    }
    for id in &previous.layers {
        let mapped: Vec<_> = mappings
            .iter()
            .filter(|m| &m.layer == id)
            .map(|m| {
                let mut p = state.layers[id].patches[m.patch].request.clone();
                p.destination = m.to.clone();
                p.subject = m.subject.clone();
                p
            })
            .collect();
        check_output_coverage(&candidate.program, &mapped)?;
        check_durable_writers(&candidate.program, &mapped)?;
        for p in &mapped {
            if let EffectDestination::Update { field, .. }
            | EffectDestination::CreateValue { field, .. } = &p.destination
            {
                check_dependencies(&candidate.program, &p.entity, field)?;
            }
        }
    }
    let expected: usize = previous
        .layers
        .iter()
        .map(|id| state.layers[id].patches.len())
        .sum();
    if mappings.len() != expected {
        return Err(error(
            "every protected history and derived-write slot needs a source-qualified mapping",
        ));
    }
    Ok(())
}

pub(super) fn validate_mappings_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    previous: &super::DecodedCompositionManifest,
    candidate: &crate::product_contract::FallibleDecodedCapturedProgram,
    mappings: &[super::fallible_scalar_decode::DecodedScopeSlotMapping],
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), CompilerFailure> {
    let state = snapshot.scalar_scope().ok_or_else(|| error("owned family cannot enter scalar scope authority"))?;
    let mut seen = crate::product_alloc::ordered::OrderedSet::new();
    let mut destinations = crate::product_alloc::ordered::OrderedSet::new();
    let mut candidate_revision = None;
    for mapping in mappings {
        if mapping.from_source() != previous.output() {
            return Err(error("ambiguous, stale or foreign protected-slot mapping").into());
        }
        if candidate_revision.is_none() {
            candidate_revision = Some(candidate.revision_with(cx).map_err(|validation_error| match validation_error {
                crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
                _ => CompilerFailure::Store(error("mapped candidate program is invalid")),
            })?);
        }
        let revision = candidate_revision.as_ref().expect("revision initialized after source binding check");
        if mapping.to_source() != revision
            || !previous.layers().contains(mapping.layer())
            || !seen.try_insert((mapping.layer(), mapping.patch()), cx).map_err(CompilerFailure::Allocation)?
        {
            return Err(error("ambiguous, stale or foreign protected-slot mapping").into());
        }
        let current = effective_patch_fallible(snapshot, previous, mapping.layer(), mapping.patch(), cx)?;
        if mapping.from() != &current.request.destination {
            return Err(error("protected-slot mapping does not name the current source slot").into());
        }
        match (&current.request.destination, mapping.to()) {
            (super::EffectDestinationDto::Update { field: from, .. }, super::EffectDestinationDto::Update { field: to, .. })
            | (super::EffectDestinationDto::CreateValue { field: from, .. }, super::EffectDestinationDto::CreateValue { field: to, .. }) if from != to => {
                return Err(error("durable field retargeting needs an explicit preservation migration").into());
            }
            _ => {}
        }
        let key = crate::product_backup::fallible_codec::canonical_bytes(mapping.to(), cx)
            .map_err(|codec_error| match codec_error {
                crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
                _ => CompilerFailure::Store(error("protected destination identity invariant")),
            })?;
        if !destinations.try_insert(key, cx).map_err(CompilerFailure::Allocation)? {
            return Err(error("overlapping protected slots need an explicit compatible reconciliation").into());
        }
        let mut request = clone_request_fallible(current.request(), cx)?;
        request.destination = clone_destination_fallible(mapping.to(), cx)?;
        request.subject = crate::product_alloc::try_copy_str(mapping.subject(), cx, crate::product_alloc::AllocationSite::String)
            .map_err(CompilerFailure::Allocation)?;
        let environment = locate_context_fallible(candidate.app(), &request, cx)?;
        let expression = clone_expr_fallible(slot_fallible_ref(candidate.app(), &request.destination)?, cx)?;
        if !matches!(request.destination, super::EffectDestinationDto::EmitItems { .. } | super::EffectDestinationDto::Observable { .. }) {
            own_row(&expression, &request.subject, &request.entity, crate::product_contract::AppRef::from(candidate.app()), &environment, true)?;
            if request.destination.is_projection() && environment.iter().any(|(id, _)| id != &request.subject && expression_variable(&expression, id)) {
                return Err(error("mapped saved projection depends on an action parameter").into());
            }
        }
    }
    if previous.layers().iter().any(|id| state.layer(id).is_some_and(|layer| !layer.request().lifecycles().is_empty())) {
        check_completion_order_ref(crate::product_contract::AppRef::from(candidate.app()))?;
    }
    for id in previous.layers() {
        let layer = state.layer(id).ok_or_else(|| error("missing layer"))?;
        let mut mapped = Vec::new();
        for mapping in mappings.iter().filter(|mapping| mapping.layer() == id) {
            let mut request = clone_request_fallible(&layer.patches()[mapping.patch()].request, cx)?;
            request.destination = clone_destination_fallible(mapping.to(), cx)?;
            request.subject = crate::product_alloc::try_copy_str(mapping.subject(), cx, crate::product_alloc::AllocationSite::String)
                .map_err(CompilerFailure::Allocation)?;
            crate::product_alloc::try_push(&mut mapped, request, cx, crate::product_alloc::AllocationSite::Vector)
                .map_err(CompilerFailure::Allocation)?;
        }
        check_output_coverage_fallible(candidate.app(), &mapped, cx)?;
        check_durable_writers_fallible(candidate.app(), &mapped, cx)?;
        for request in &mapped {
            if let Some(field) = request.destination.update_field() {
                check_dependencies_fallible(candidate.app(), &request.entity, field)?;
            }
        }
    }
    let mut expected = 0usize;
    for id in previous.layers() {
        expected = expected.checked_add(state.layer(id).ok_or_else(|| error("missing layer"))?.patches().len())
            .ok_or_else(|| error("protected mapping count overflow"))?;
    }
    if mappings.len() != expected {
        return Err(error("every protected history and derived-write slot needs a source-qualified mapping").into());
    }
    Ok(())
}

pub(super) fn business_program(snapshot: &ProjectSnapshot) -> Result<AppDefinition> {
    if let Some(manifest) = snapshot
        .scalar_scope()?
        .compositions
        .get(&snapshot.active_revision)
    {
        let mut app = program(snapshot, &manifest.business)?.program.clone();
        let mut seen = BTreeSet::new();
        for id in &manifest.layers {
            let layer = &snapshot.scalar_scope()?.layers[id];
            for index in 0..layer.patches.len() {
                let patch = effective_patch(snapshot, manifest, id, index)?;
                let key = canonical_bytes(&patch.request.destination)?;
                if seen.insert(key) {
                    *slot(&mut app, &patch.request.destination)? = patch.before.clone();
                }
                if manifest.active.contains(id) {
                    *slot(&mut app, &patch.request.destination)? = patch.after.clone();
                }
            }
        }
        Ok(app)
    } else {
        Ok(snapshot.program()?.program.clone())
    }
}

pub(super) fn envelope_app(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<AppDefinition> {
    let business = program(snapshot, &manifest.business)?;
    reject_reserved(&business.program)?;
    let mut app = business.program.clone();
    let mut seen = BTreeSet::new();
    // Reset a shared destination exactly once to its earliest retained meaning.
    for id in &manifest.layers {
        let layer = snapshot
            .scalar_scope()?
            .layers
            .get(id)
            .ok_or_else(|| error("missing layer"))?;
        for index in 0..layer.patches.len() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            if seen.insert(canonical_bytes(&patch.request.destination)?) {
                *slot(&mut app, &patch.request.destination)? = patch.before.clone();
            }
        }
    }
    for id in &manifest.layers {
        let layer = &snapshot.scalar_scope()?.layers[id];
        let active = manifest.active.contains(id);
        for lifecycle in &layer.request.lifecycles {
            let entity = app
                .entities
                .iter_mut()
                .find(|e| e.id == lifecycle.entity)
                .ok_or_else(|| error("protected entity removed"))?;
            entity.constraints.push(Expr::Or {
                values: vec![
                    Expr::Not {
                        value: Box::new(flag("record", &key(id, &lifecycle.entity, "sealed"))),
                    },
                    lifecycle.completed.clone(),
                ],
            });
            for (suffix, typ, label) in [
                ("member", Type::Boolean, "Scope membership"),
                ("sealed", Type::Boolean, "Preserved result"),
                ("day", Type::Date, "Result capture day"),
                ("origin", Type::Text, "Result provenance"),
            ] {
                entity.fields.push(FieldDefinition {
                    id: key(id, &lifecycle.entity, suffix),
                    label: label.into(),
                    value_type: optional(&typ),
                });
            }
        }
        for (index, original) in layer.patches.iter().enumerate() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            let p = &patch.request;
            let old = slot(&mut app, &p.destination)?.clone();
            let global = matches!(
                p.destination,
                EffectDestination::EmitItems { .. } | EffectDestination::Observable { .. }
            );
            let mut live = if !active {
                old.clone()
            } else if global {
                patch.after.clone()
            } else if matches!(p.destination, EffectDestination::CreateValue { .. }) {
                if matches!(
                    layer.request.population,
                    ScopePopulation::FutureWork | ScopePopulation::All
                ) {
                    patch.after.clone()
                } else {
                    old.clone()
                }
            } else {
                conditional(
                    flag(&p.subject, &key(id, &p.entity, "member")),
                    patch.after.clone(),
                    old.clone(),
                )
            };
            if projection(&p.destination) {
                let saved = saved_key(id, original)?;
                let sealed = key(id, &p.entity, "sealed");
                let entity = app
                    .entities
                    .iter_mut()
                    .find(|e| e.id == p.entity)
                    .ok_or_else(|| error("protected entity absent"))?;
                entity.fields.push(FieldDefinition {
                    id: saved.clone(),
                    label: "Preserved completed result".into(),
                    value_type: optional(&p.value_type),
                });
                let read = if matches!(p.value_type, Type::Optional { .. }) {
                    field(&p.subject, &saved)
                } else {
                    entity.constraints.push(Expr::Or {
                        values: vec![
                            Expr::Not {
                                value: Box::new(flag("record", &sealed)),
                            },
                            Expr::Not {
                                value: Box::new(Expr::Equal {
                                    left: Box::new(field("record", &saved)),
                                    right: Box::new(literal(
                                        DataValue::Null,
                                        optional(&p.value_type),
                                    )),
                                }),
                            },
                        ],
                    });
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &saved)),
                        fallback: Box::new(inert(&p.value_type)?),
                    }
                };
                live = conditional(flag(&p.subject, &sealed), read, live);
            } else if let EffectDestination::Update { field: target, .. } = &p.destination {
                live = conditional(
                    flag(&p.subject, &key(id, &p.entity, "sealed")),
                    field(&p.subject, target),
                    live,
                );
            }
            *slot(&mut app, &p.destination)? = live;
        }
    }
    Ok(app)
}
pub(super) fn compile(
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<CapturedProgram> {
    let mut app = envelope_app(snapshot, manifest)?;
    let projected = app.clone();
    for action in &mut app.actions {
        action.steps = instrument(
            &action.steps,
            &projected,
            snapshot,
            manifest,
            &action.parameters,
        )?;
        action
            .steps
            .extend(sealing_passes(&projected, snapshot, manifest, None)?);
    }
    add_provenance(&mut app, snapshot, manifest)?;
    app.validate()?;
    let input = canonical_digest(
        IdentityDomain::Adoption,
        &(
            &manifest.business,
            &manifest.layers,
            &manifest.active,
            &manifest.rewrites,
            &manifest.previous,
            &manifest.operation,
            COMPILER_VERSION,
        ),
    )?;
    Ok(CapturedProgram::capture(
        &canonical_bytes(&app)?,
        &snapshot.data.project_id,
        Producer::ExternalAuthor {
            description: format!(
                "Verified host scope composition v{COMPILER_VERSION}: {}",
                input.as_str()
            ),
        },
        None,
    )?)
}

enum FallibleCompositionBuild {
    Envelope(crate::product_contract::fallible_app_decode::DecodedAppDefinition),
    Capture(crate::product_contract::FallibleDecodedCapturedProgram),
}

/// Build the pre-instrumentation app envelope used for historical projection
/// evaluation. This is the decoded counterpart of `envelope_app`.
pub(super) fn envelope_app_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<crate::product_contract::fallible_app_decode::DecodedAppDefinition, CompilerFailure> {
    match build_fallible(snapshot, manifest, cx, false)? {
        FallibleCompositionBuild::Envelope(app) => Ok(app),
        FallibleCompositionBuild::Capture(_) => unreachable!("envelope build returned a capture"),
    }
}

/// Fallible scalar composition compiler for the promoted decoded owner graph.
/// This mirrors `compile` without materializing the legacy BTreeMap model.
pub(super) fn compile_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<crate::product_contract::FallibleDecodedCapturedProgram, CompilerFailure> {
    match build_fallible(snapshot, manifest, cx, true)? {
        FallibleCompositionBuild::Capture(program) => Ok(program),
        FallibleCompositionBuild::Envelope(_) => unreachable!("capture build returned an envelope"),
    }
}

fn build_fallible(
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    cx: &mut crate::product_alloc::AllocationContext,
    capture: bool,
) -> std::result::Result<FallibleCompositionBuild, CompilerFailure> {
    use crate::product_alloc::{try_append_str, try_reserve, try_push, AllocationSite};
    use crate::product_contract::{AppValidationError, Producer};

    fn app_error(error: AppValidationError) -> CompilerFailure {
        match error {
            AppValidationError::Contract(error) => CompilerFailure::Store(StoreError::Invalid(error.0)),
            AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
        }
    }
    fn codec_error(failure: crate::product_backup::fallible_codec::CodecError) -> CompilerFailure {
        match failure {
            crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
            _ => CompilerFailure::Store(error("compiler codec invariant")),
        }
    }
    fn string(value: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<String, CompilerFailure> {
        crate::product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)
    }
    fn boxed(value: Expr, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Box<Expr>, CompilerFailure> {
        crate::product_alloc::try_box(value, cx).map_err(CompilerFailure::Allocation)
    }
    fn copied_type(value: &Type, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Type, CompilerFailure> {
        crate::product_contract::try_clone_type_with(value, cx).map_err(CompilerFailure::Allocation)
    }
    fn optional_type(value: &Type, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Type, CompilerFailure> {
        if matches!(value, Type::Optional { .. }) { copied_type(value, cx) }
        else { Ok(Type::Optional { item: crate::product_alloc::try_box(copied_type(value, cx)?, cx).map_err(CompilerFailure::Allocation)? }) }
    }
    fn boolean(value: bool) -> Expr {
        Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value } }
    }
    fn text(value: String) -> Expr {
        Expr::Literal { value_type: Type::Text, value: DataValue::Text { value } }
    }
    fn variable(name: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
        Ok(Expr::Variable { name: string(name, cx)? })
    }
    fn field(subject: &str, id: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
        Ok(Expr::Field { record: boxed(variable(subject, cx)?, cx)?, field: string(id, cx)? })
    }
    fn flag(subject: &str, id: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
        Ok(Expr::Coalesce { value: boxed(field(subject, id, cx)?, cx)?, fallback: boxed(boolean(false), cx)? })
    }
    fn conditional(
        condition: Expr, yes: Expr, no: Expr, cx: &mut crate::product_alloc::AllocationContext,
    ) -> std::result::Result<Expr, CompilerFailure> {
        Ok(Expr::If { condition: boxed(condition, cx)?, then_value: boxed(yes, cx)?, else_value: boxed(no, cx)? })
    }
    fn inert(value: &Type) -> std::result::Result<Expr, CompilerFailure> {
        Ok(match value {
            Type::Boolean => boolean(false),
            Type::Integer => Expr::Literal { value_type: Type::Integer, value: DataValue::Integer { value: 0 } },
            Type::Text => Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: String::new() } },
            Type::Date => Expr::Literal { value_type: Type::Date, value: DataValue::Date { days: 0 } },
            _ => return Err(error("protected result must be a scalar value, optionally nullable").into()),
        })
    }
    fn reserved_strings(value: &crate::product_json::Value) -> bool {
        use crate::product_json::Value;
        match value {
            Value::String(value) => value.starts_with(RESERVED_PREFIX),
            Value::Array(values) => values.iter().any(reserved_strings),
            Value::Object(values) => values.iter().any(|(key, value)| key.starts_with(RESERVED_PREFIX) || reserved_strings(value)),
            _ => false,
        }
    }

    let scope = snapshot.scalar_scope().ok_or_else(|| error("owned family cannot enter scalar scope authority"))?;
    let business = snapshot.captured_program_by_revision_with(manifest.business(), cx).map_err(app_error)?
        .ok_or_else(|| error("business source program absent"))?;
    let source_value = crate::product_backup::fallible_codec::try_serialize_value(business.app(), cx).map_err(codec_error)?;
    if reserved_strings(&source_value) { return Err(error("authored source uses the reserved metadata namespace").into()); }
    let mut app = business.app().try_clone_with(cx).map_err(app_error)?;

    // Restore every shared destination to the earliest retained meaning before
    // replaying the layer chain, matching the legacy two-pass lowering.
    let mut seen = crate::product_alloc::ordered::OrderedSet::new();
    for id in manifest.layers() {
        let layer = scope.layer(id).ok_or_else(|| error("missing layer"))?;
        for index in 0..layer.patches().len() {
            let patch = effective_patch_fallible(snapshot, manifest, id, index, cx)?;
            let key = crate::product_backup::fallible_codec::canonical_bytes(&patch.request.destination, cx).map_err(codec_error)?;
            if seen.try_insert(key, cx).map_err(CompilerFailure::Allocation)? {
                let before = clone_expr_fallible(&patch.before, cx)?;
                *slot_fallible(&mut app, &patch.request.destination)? = before;
            }
        }
    }
    for id in manifest.layers() {
        let layer = scope.layer(id).ok_or_else(|| error("missing layer"))?;
        let active = manifest.active_contains(id);
        for lifecycle in layer.request().lifecycles() {
            let sealed = key_fallible(id, lifecycle.entity(), "sealed", cx)?;
            let mut negated = Vec::new();
            try_push(&mut negated, Expr::Not { value: boxed(flag("record", &sealed, cx)?, cx)? }, cx, AllocationSite::Vector)
                .map_err(CompilerFailure::Allocation)?;
            try_push(&mut negated, clone_expr_fallible(lifecycle.completed(), cx)?, cx, AllocationSite::Vector)
                .map_err(CompilerFailure::Allocation)?;
            let entity = app.entities.iter_mut().find(|entity| entity.id == lifecycle.entity())
                .ok_or_else(|| error("protected entity removed"))?;
            try_push(&mut entity.constraints, Expr::Or { values: negated }, cx, AllocationSite::Vector)
                .map_err(CompilerFailure::Allocation)?;
            for (suffix, typ, label) in [
                ("member", Type::Boolean, "Scope membership"),
                ("sealed", Type::Boolean, "Preserved result"),
                ("day", Type::Date, "Result capture day"),
                ("origin", Type::Text, "Result provenance"),
            ] {
                let id = key_fallible(id, lifecycle.entity(), suffix, cx)?;
                let label = string(label, cx)?;
                let value_type = optional_type(&typ, cx)?;
                try_push(&mut entity.fields, crate::product_contract::FieldDefinition { id, label, value_type }, cx, AllocationSite::Vector)
                    .map_err(CompilerFailure::Allocation)?;
            }
        }
        for index in 0..layer.patches().len() {
            let original = &layer.patches()[index];
            let patch = effective_patch_fallible(snapshot, manifest, id, index, cx)?;
            let request = patch.request();
            let old = clone_expr_fallible(slot_fallible_ref(&app, &request.destination)?, cx)?;
            let global = matches!(request.destination, super::EffectDestinationDto::EmitItems { .. } | super::EffectDestinationDto::Observable { .. });
            let mut live = if !active { old }
            else if global { clone_expr_fallible(patch.after(), cx)? }
            else if matches!(request.destination, super::EffectDestinationDto::CreateValue { .. })
                && matches!(layer.request().population, super::ScopePopulationDto::FutureWork | super::ScopePopulationDto::All) {
                clone_expr_fallible(patch.after(), cx)?
            } else if matches!(request.destination, super::EffectDestinationDto::CreateValue { .. }) {
                old
            } else {
                let member = key_fallible(id, &request.entity, "member", cx)?;
                conditional(flag(&request.subject, &member, cx)?, clone_expr_fallible(patch.after(), cx)?, old, cx)?
            };
            if matches!(request.destination, super::EffectDestinationDto::ViewColumn { .. } | super::EffectDestinationDto::EmitColumn { .. }) {
                let saved = saved_key_fallible(id, original, cx)?;
                let sealed = key_fallible(id, &request.entity, "sealed", cx)?;
                let entity = app.entities.iter_mut().find(|entity| entity.id == request.entity)
                    .ok_or_else(|| error("protected entity absent"))?;
                let saved_id = string(&saved, cx)?;
                let saved_label = string("Preserved completed result", cx)?;
                let saved_type = optional_type(&request.value_type, cx)?;
                try_push(&mut entity.fields, crate::product_contract::FieldDefinition { id: saved_id, label: saved_label, value_type: saved_type }, cx, AllocationSite::Vector)
                    .map_err(CompilerFailure::Allocation)?;
                let read = if matches!(request.value_type, Type::Optional { .. }) {
                    field(&request.subject, &saved, cx)?
                } else {
                    let mut conditions = Vec::new();
                    try_push(&mut conditions, Expr::Not { value: boxed(flag("record", &sealed, cx)?, cx)? }, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                    let null = Expr::Literal { value_type: optional_type(&request.value_type, cx)?, value: DataValue::Null };
                    let equal = Expr::Equal { left: boxed(field("record", &saved, cx)?, cx)?, right: boxed(null, cx)? };
                    try_push(&mut conditions, Expr::Not { value: boxed(equal, cx)? }, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                    try_push(&mut entity.constraints, Expr::Or { values: conditions }, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                    let inert_type = match &request.value_type { Type::Optional { item } => item.as_ref(), typ => typ };
                    Expr::Coalesce { value: boxed(field(&request.subject, &saved, cx)?, cx)?, fallback: boxed(inert(inert_type)?, cx)? }
                };
                live = conditional(flag(&request.subject, &sealed, cx)?, read, live, cx)?;
            } else if let super::EffectDestinationDto::Update { field: target, .. } = &request.destination {
                let sealed = key_fallible(id, &request.entity, "sealed", cx)?;
                live = conditional(flag(&request.subject, &sealed, cx)?, field(&request.subject, target, cx)?, live, cx)?;
            }
            *slot_fallible(&mut app, &request.destination)? = live;
        }
    }

    if !capture {
        return Ok(FallibleCompositionBuild::Envelope(app));
    }

    // Instrument original action steps from the projected graph, then append
    // seal passes and provenance in the same order as the legacy compiler.
    let projected = app.try_clone_with(cx).map_err(app_error)?;
    for action_index in 0..app.actions.len() {
        let steps = instrument_fallible(&app.actions[action_index].steps, &projected, snapshot, manifest, &app.actions[action_index].parameters, cx)?;
        let seals = sealing_passes_fallible(&projected, snapshot, manifest, None, cx)?;
        let action = &mut app.actions[action_index];
        action.steps = steps;
        try_reserve(&mut action.steps, seals.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
        action.steps.extend(seals);
    }
    add_provenance_fallible(&mut app, snapshot, manifest, cx)?;
    app.validate_with(cx).map_err(app_error)?;

    let mut description = String::new();
    let input = manifest.compiler_input_identity_with(cx).map_err(codec_error)?;
    let suffix = input.as_str();
    let mut version = [0u8; 10];
    let mut version_len = 0;
    let mut remaining = COMPILER_VERSION;
    loop {
        version[version_len] = b'0' + (remaining % 10) as u8;
        version_len += 1;
        remaining /= 10;
        if remaining == 0 { break; }
    }
    version[..version_len].reverse();
    let version = std::str::from_utf8(&version[..version_len]).expect("decimal digits are UTF-8");
    let description_len = "Verified host scope composition v".len() + version.len() + ": ".len() + suffix.len();
    crate::product_alloc::try_reserve_string(&mut description, description_len, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    try_append_str(&mut description, "Verified host scope composition v", cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    description.push_str(version);
    try_append_str(&mut description, ": ", cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    try_append_str(&mut description, suffix, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    let producer = Producer::ExternalAuthor { description };
    crate::product_contract::capture_generated_app_fallible(app, snapshot.data().project_id(), producer, cx)
        .map(FallibleCompositionBuild::Capture)
        .map_err(app_error)
}

fn clone_statement_fallible(
    statement: &crate::product_contract::fallible_app_decode::DecodedStatement,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<crate::product_contract::fallible_app_decode::DecodedStatement, CompilerFailure> {
    let value = crate::product_backup::fallible_codec::try_serialize_value(statement, cx).map_err(|failure| match failure {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
        _ => CompilerFailure::Store(error("statement copy invariant")),
    })?;
    crate::product_contract::fallible_app_decode::decode_statement_value(value, cx).map_err(|failure| match failure {
        crate::product_contract::fallible_value_decode::ValueDecodeError::Allocation(failure) => CompilerFailure::Allocation(failure),
        _ => CompilerFailure::Store(error("statement copy invariant")),
    })
}

pub(super) fn key_fallible(
    layer: &Digest,
    entity: &str,
    suffix: &str,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<String, CompilerFailure> {
    use crate::product_alloc::{try_append_str, try_reserve_string, AllocationSite};
    let digest = crate::product_backup::fallible_codec::canonical_digest(
        IdentityDomain::Adoption,
        &super::DecodedCompilerKeyIdentity { layer, entity, suffix },
        cx,
    ).map_err(|failure| match failure {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
        _ => CompilerFailure::Store(error("compiler key invariant")),
    })?;
    let mut result = String::new();
    let length = RESERVED_PREFIX.len() + 24 + 1 + suffix.len();
    try_reserve_string(&mut result, length, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    try_append_str(&mut result, RESERVED_PREFIX, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    result.push_str(&digest.as_str()[..24]);
    result.push('_');
    result.push_str(suffix);
    Ok(result)
}

pub(super) fn saved_key_fallible(
    layer: &Digest,
    patch: &super::DecodedEffectPatch,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<String, CompilerFailure> {
    use crate::product_alloc::{try_append_str, try_reserve_string, AllocationSite};
    let digest = crate::product_backup::fallible_codec::canonical_digest(
        IdentityDomain::Adoption, &patch.request.destination, cx,
    ).map_err(|failure| match failure {
        crate::product_backup::fallible_codec::CodecError::Allocation(failure) => CompilerFailure::Allocation(failure),
        _ => CompilerFailure::Store(error("compiler key invariant")),
    })?;
    let mut suffix = String::new();
    try_reserve_string(&mut suffix, 22, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    try_append_str(&mut suffix, "value_", cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
    suffix.push_str(&digest.as_str()[..16]);
    key_fallible(layer, &patch.request.entity, &suffix, cx)
}

fn decoded_boolean(value: bool) -> Expr {
    Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value } }
}
fn decoded_text(value: String) -> Expr {
    Expr::Literal { value_type: Type::Text, value: DataValue::Text { value } }
}
fn decoded_variable(name: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
    Ok(Expr::Variable { name: crate::product_alloc::try_copy_str(name, cx, crate::product_alloc::AllocationSite::String).map_err(CompilerFailure::Allocation)? })
}
fn decoded_field(subject: &str, id: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
    Ok(Expr::Field {
        record: crate::product_alloc::try_box(decoded_variable(subject, cx)?, cx).map_err(CompilerFailure::Allocation)?,
        field: crate::product_alloc::try_copy_str(id, cx, crate::product_alloc::AllocationSite::String).map_err(CompilerFailure::Allocation)?,
    })
}
fn decoded_flag(subject: &str, id: &str, cx: &mut crate::product_alloc::AllocationContext) -> std::result::Result<Expr, CompilerFailure> {
    Ok(Expr::Coalesce {
        value: crate::product_alloc::try_box(decoded_field(subject, id, cx)?, cx).map_err(CompilerFailure::Allocation)?,
        fallback: crate::product_alloc::try_box(decoded_boolean(false), cx).map_err(CompilerFailure::Allocation)?,
    })
}

fn instrument_fallible(
    steps: &[crate::product_contract::fallible_app_decode::DecodedStatement],
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    parameters: &crate::product_contract::fallible_app_decode::FallibleMap<Type>,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Vec<crate::product_contract::fallible_app_decode::DecodedStatement>, CompilerFailure> {
    let env = CompilerEnvironment::from_parameters(
        crate::product_contract::StringMapRef::Fallible(parameters), true, Some(cx),
    )?;
    instrument_scope_steps(steps, app, snapshot, manifest, env, cx)
}

fn instrument_scope_steps(
    steps: &[crate::product_contract::fallible_app_decode::DecodedStatement],
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    mut environment: CompilerEnvironment,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Vec<crate::product_contract::fallible_app_decode::DecodedStatement>, CompilerFailure> {
    use crate::product_alloc::{try_push, try_reserve, AllocationSite};
    use crate::product_contract::fallible_app_decode::DecodedStatement as S;
    let mut result = Vec::new();
    try_reserve(&mut result, steps.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
    for original in steps {
        let mut statement = clone_statement_fallible(original, cx)?;
        match &mut statement {
            S::Create { entity, values, .. } => {
                if let Some(scope) = snapshot.scalar_scope() {
                    for id in manifest.layers() {
                        let layer = scope.layer(id).ok_or_else(|| error("missing layer"))?;
                        if layer.request().lifecycles().iter().any(|life| life.entity() == entity) {
                            let member = key_fallible(id, entity, "member", cx)?;
                            let sealed = key_fallible(id, entity, "sealed", cx)?;
                            let is_member = manifest.active_contains(id)
                                && matches!(layer.request().population, super::ScopePopulationDto::FutureWork | super::ScopePopulationDto::All);
                            values.try_insert(member, decoded_boolean(is_member), cx).map_err(CompilerFailure::Allocation)?;
                            values.try_insert(sealed, decoded_boolean(false), cx).map_err(CompilerFailure::Allocation)?;
                        }
                    }
                }
            }
            S::Archive { record } => {
                let entity = match record {
                    Expr::Variable { name } => match environment.get(name) {
                        Some(Type::Reference { entity }) => entity.as_str(),
                        _ => return Err(error("archive subject cannot be proven").into()),
                    },
                    _ => return Err(error("archive subject cannot be proven").into()),
                };
                let mut seals = sealing_passes_fallible(app, snapshot, manifest, Some((record, entity)), cx)?;
                try_reserve(&mut result, seals.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                result.append(&mut seals);
            }
            S::Emit { .. } => {
                let mut seals = sealing_passes_fallible(app, snapshot, manifest, None, cx)?;
                try_reserve(&mut result, seals.len(), cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
                result.append(&mut seals);
            }
            S::ForEach { steps: nested_steps, items, binding } => {
                let entity = list_entity(items, crate::product_contract::AppRef::from(app), &environment)
                    .ok_or_else(|| error("loop subject cannot be proven for completion capture"))?;
                let mut nested = environment.try_clone_with(Some(cx))?;
                nested.insert_reference(binding, entity, Some(cx))?;
                *nested_steps = instrument_scope_steps(nested_steps, app, snapshot, manifest, nested, cx)?;
            }
            _ => {}
        }
        if let S::Create { entity, bind, .. } = &statement {
            environment.insert_reference(bind, entity, Some(cx))?;
        }
        try_push(&mut result, statement, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
    }
    Ok(result)
}

fn sealing_passes_fallible(
    app: &crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    archive: Option<(&Expr, &str)>,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<Vec<crate::product_contract::fallible_app_decode::DecodedStatement>, CompilerFailure> {
    use crate::product_alloc::{try_box, try_push, try_reserve, AllocationSite};
    use crate::product_contract::fallible_app_decode::{DecodedStatement as S, FallibleMap};
    let mut result = Vec::new();
    let Some(scope) = snapshot.scalar_scope() else { return Err(error("owned family cannot enter scalar scope authority").into()); };
    for id in manifest.layers() {
        let layer = scope.layer(id).ok_or_else(|| error("missing layer"))?;
        for lifecycle in layer.request().lifecycles() {
            let binding = key_fallible(id, lifecycle.entity(), "seal_row", cx)?;
            let condition = if let Some((record, entity)) = archive {
                if entity != lifecycle.entity() { continue; }
                Expr::Equal { left: try_box(decoded_variable(&binding, cx)?, cx).map_err(CompilerFailure::Allocation)?, right: try_box(clone_expr_fallible(record, cx)?, cx).map_err(CompilerFailure::Allocation)? }
            } else {
                rebind_fallible(lifecycle.completed(), "record", &binding, cx)?
            };
            let sealed = key_fallible(id, lifecycle.entity(), "sealed", cx)?;
            let not_sealed = Expr::Not { value: try_box(decoded_flag(&binding, &sealed, cx)?, cx).map_err(CompilerFailure::Allocation)? };
            let mut predicates = Vec::new();
            try_reserve(&mut predicates, 2, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
            try_push(&mut predicates, not_sealed, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
            try_push(&mut predicates, condition, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
            let predicate = Expr::And { values: predicates };
            let query = Expr::Query {
                entity: crate::product_alloc::try_copy_str(lifecycle.entity(), cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?,
                binding: crate::product_alloc::try_copy_str(&binding, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?,
                predicate: try_box(predicate, cx).map_err(CompilerFailure::Allocation)?,
                sort: Vec::new(), limit: crate::product_contract::MAX_COLLECTION, include_archived: false,
            };
            let mut assignments = FallibleMap::new();
            for (index, original) in layer.patches().iter().enumerate() {
                let patch = effective_patch_fallible(snapshot, manifest, id, index, cx)?;
                if patch.request.entity == lifecycle.entity() && matches!(patch.request.destination, super::EffectDestinationDto::ViewColumn { .. } | super::EffectDestinationDto::EmitColumn { .. }) {
                    let expression = clone_expr_fallible(slot_fallible_ref(app, &patch.request.destination)?, cx)?;
                    let expression = rebind_fallible(&expression, &patch.request.subject, &binding, cx)?;
                    let saved = saved_key_fallible(id, original, cx)?;
                    assignments.try_insert(saved, expression, cx).map_err(CompilerFailure::Allocation)?;
                }
            }
            assignments.try_insert(key_fallible(id, lifecycle.entity(), "sealed", cx)?, decoded_boolean(true), cx).map_err(CompilerFailure::Allocation)?;
            assignments.try_insert(key_fallible(id, lifecycle.entity(), "day", cx)?, Expr::Today, cx).map_err(CompilerFailure::Allocation)?;
            let origin = if archive.is_some() { "ObservedAtArchive" } else { "ObservedAtCompletion" };
            let origin = crate::product_alloc::try_copy_str(origin, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
            assignments.try_insert(key_fallible(id, lifecycle.entity(), "origin", cx)?, decoded_text(origin), cx).map_err(CompilerFailure::Allocation)?;
            let record = decoded_variable(&binding, cx)?;
            let mut body = Vec::new();
            try_reserve(&mut body, 1, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
            try_push(&mut body, S::Update { record, values: assignments }, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
            try_push(&mut result, S::ForEach { items: query, binding, steps: body }, cx, AllocationSite::Vector).map_err(CompilerFailure::Allocation)?;
        }
    }
    Ok(result)
}

fn add_provenance_fallible(
    app: &mut crate::product_contract::fallible_app_decode::DecodedAppDefinition,
    snapshot: &crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot,
    manifest: &super::DecodedCompositionManifest,
    cx: &mut crate::product_alloc::AllocationContext,
) -> std::result::Result<(), CompilerFailure> {
    use crate::product_alloc::{try_push, AllocationSite};
    use crate::product_contract::fallible_app_decode::{DecodedStatement as S, DecodedViewKind as V};
    for layer_id in manifest.layers() {
        let scope = snapshot.scalar_scope().ok_or_else(|| error("owned family cannot enter scalar scope authority"))?;
        let layer = scope.layer(layer_id).ok_or_else(|| error("missing layer"))?;
        for index in 0..layer.patches().len() {
            let patch = effective_patch_fallible(snapshot, manifest, layer_id, index, cx)?;
            let request = patch.request();
            if !matches!(request.destination, super::EffectDestinationDto::ViewColumn { .. } | super::EffectDestinationDto::EmitColumn { .. }) {
                continue;
            }
            let origin = key_fallible(layer_id, &request.entity, "origin", cx)?;
            let day = key_fallible(layer_id, &request.entity, "day", cx)?;
            let record = key_fallible(layer_id, &request.entity, "record", cx)?;
            let origin_expr = Expr::Coalesce {
                value: crate::product_alloc::try_box(decoded_field(&request.subject, &origin, cx)?, cx).map_err(CompilerFailure::Allocation)?,
                fallback: crate::product_alloc::try_box(decoded_text(crate::product_alloc::try_copy_str("CurrentCalculation", cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?), cx).map_err(CompilerFailure::Allocation)?,
            };
            let day_expr = Expr::Coalesce {
                value: crate::product_alloc::try_box(decoded_field(&request.subject, &day, cx)?, cx).map_err(CompilerFailure::Allocation)?,
                fallback: crate::product_alloc::try_box(Expr::Today, cx).map_err(CompilerFailure::Allocation)?,
            };
            let record_expr = decoded_variable(&request.subject, cx)?;
            let record_type = Type::Reference {
                entity: crate::product_alloc::try_copy_str(&request.entity, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?,
            };
            let origin_column = crate::product_alloc::try_copy_str(&origin, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
            let day_column = crate::product_alloc::try_copy_str(&day, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
            let values: [(String, &'static str, Type, Expr); 3] = [
                (origin_column, "Result provenance", Type::Text, origin_expr),
                (day_column, "Result day", Type::Date, day_expr),
                (record, "Source record", record_type, record_expr),
            ];
            match &request.destination {
                super::EffectDestinationDto::ViewColumn { view, .. } => {
                    let view = app.views.iter_mut().find(|view_def| &view_def.id == view)
                        .ok_or_else(|| error("view removed"))?;
                    let columns = match &mut view.kind {
                        V::List { columns, .. } | V::Detail { columns, .. } => columns,
                        V::Form { .. } => return Err(error("form is not a row projection").into()),
                    };
                    for (id, label, _, value) in values {
                        if !columns.iter().any(|column| column.id == id) {
                            let label = crate::product_alloc::try_copy_str(label, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
                            try_push(columns, crate::product_contract::fallible_app_decode::DecodedColumn { id, label, value }, cx, AllocationSite::Vector)
                                .map_err(CompilerFailure::Allocation)?;
                        }
                    }
                }
                super::EffectDestinationDto::EmitColumn { action, path, .. } => {
                    let source = snapshot.captured_program_by_revision_with(manifest.business(), cx)
                        .map_err(|failure| match failure {
                            crate::product_contract::AppValidationError::Allocation(failure) => CompilerFailure::Allocation(failure),
                            _ => CompilerFailure::Store(error("business source program is invalid")),
                        })?.ok_or_else(|| error("business source program absent"))?;
                    let source_action = source.app().actions.iter().find(|action_def| &action_def.id == action)
                        .ok_or_else(|| error("output action absent"))?;
                    let source_statement = decoded_step_ref(&source_action.steps, path)?;
                    let output = match source_statement { S::Emit { output, .. } => output.as_str(), _ => return Err(error("output slot absent").into()) };
                    let definition = app.outputs.iter_mut().find(|definition| definition.id == output)
                        .ok_or_else(|| error("output definition absent"))?;
                    for (id, label, typ, _) in &values {
                        if !definition.columns.iter().any(|column| &column.id == id) {
                            let id = crate::product_alloc::try_copy_str(id, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
                            let label = crate::product_alloc::try_copy_str(label, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
                            let value_type = crate::product_contract::try_clone_type_with(typ, cx).map_err(CompilerFailure::Allocation)?;
                            try_push(&mut definition.columns, crate::product_contract::FieldDefinition { id, label, value_type }, cx, AllocationSite::Vector)
                                .map_err(CompilerFailure::Allocation)?;
                        }
                    }
                    fn append(
                        steps: &mut [S], output: &str, subject: &str, values: &[(String, &'static str, Type, Expr); 3],
                        cx: &mut crate::product_alloc::AllocationContext,
                    ) -> std::result::Result<(), CompilerFailure> {
                        for step in steps {
                            match step {
                                S::Emit { output: id, binding, columns, .. } if id == output => {
                                    for (id, _, _, expr) in values {
                                        let key = crate::product_alloc::try_copy_str(id, cx, AllocationSite::String).map_err(CompilerFailure::Allocation)?;
                                        let expr = rebind_fallible(expr, subject, binding, cx)?;
                                        columns.try_insert(key, expr, cx).map_err(CompilerFailure::Allocation)?;
                                    }
                                }
                                S::ForEach { steps, .. } => append(steps, output, subject, values, cx)?,
                                _ => {}
                            }
                        }
                        Ok(())
                    }
                    for action_def in &mut app.actions {
                        append(&mut action_def.steps, output, &request.subject, &values, cx)?;
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

fn instrument(
    steps: &[Statement],
    app: &AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    environment: &BTreeMap<Id, Type>,
) -> Result<Vec<Statement>> {
    let mut environment = environment.clone();
    let mut result = vec![];
    for original in steps {
        let mut s = original.clone();
        match &mut s {
            Statement::Create { entity, values, .. } => {
                for id in &manifest.layers {
                    let layer = &snapshot.scalar_scope()?.layers[id];
                    if layer.request.lifecycles.iter().any(|l| &l.entity == entity) {
                        values.insert(
                            key(id, entity, "member"),
                            boolean(
                                manifest.active.contains(id)
                                    && matches!(
                                        layer.request.population,
                                        ScopePopulation::FutureWork | ScopePopulation::All
                                    ),
                            ),
                        );
                        values.insert(key(id, entity, "sealed"), boolean(false));
                    }
                }
            }
            Statement::Archive { record } => {
                let entity = match record {
                    Expr::Variable { name } => match environment.get(name) {
                        Some(Type::Reference { entity }) => entity.clone(),
                        _ => return Err(error("archive subject cannot be proven")),
                    },
                    _ => return Err(error("archive subject cannot be proven")),
                };
                result.extend(sealing_passes(
                    app,
                    snapshot,
                    manifest,
                    Some((record, &entity)),
                )?)
            }
            Statement::Emit { .. } => result.extend(sealing_passes(app, snapshot, manifest, None)?),
            Statement::ForEach {
                steps,
                items,
                binding,
            } => {
                let entity = list_entity(items, app, &environment)
                    .ok_or_else(|| error("loop subject cannot be proven for completion capture"))?;
                let mut nested = environment.clone();
                nested.insert(binding.clone(), Type::reference(entity));
                *steps = instrument(steps, app, snapshot, manifest, &nested)?
            }
            _ => {}
        }
        if let Statement::Create { entity, bind, .. } = &s {
            environment.insert(bind.clone(), Type::reference(entity));
        }
        result.push(s);
    }
    Ok(result)
}
fn sealing_passes(
    app: &AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
    archive: Option<(&Expr, &str)>,
) -> Result<Vec<Statement>> {
    let mut result = vec![];
    for id in &manifest.layers {
        let layer = &snapshot.scalar_scope()?.layers[id];
        for lifecycle in &layer.request.lifecycles {
            let binding = key(id, &lifecycle.entity, "seal_row");
            // Archive supports the same explicit own-row reference grammar. Cross-
            // entity archive programs are rejected rather than inventing a cast.
            let condition = if let Some((record, entity)) = archive {
                if entity != lifecycle.entity {
                    continue;
                }
                Expr::Equal {
                    left: Box::new(variable(&binding)),
                    right: Box::new(record.clone()),
                }
            } else {
                rebind(&lifecycle.completed, "record", &binding)?
            };
            let query = Expr::Query {
                entity: lifecycle.entity.clone(),
                binding: binding.clone(),
                predicate: Box::new(Expr::And {
                    values: vec![
                        Expr::Not {
                            value: Box::new(flag(&binding, &key(id, &lifecycle.entity, "sealed"))),
                        },
                        condition,
                    ],
                }),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            };
            // Expr assignments (not data values) all read the pre-seal row together.
            let mut assignments = BTreeMap::new();
            for (index, original) in layer.patches.iter().enumerate() {
                let patch = effective_patch(snapshot, manifest, id, index)?;
                if patch.request.entity == lifecycle.entity
                    && projection(&patch.request.destination)
                {
                    let expression = slot(&mut app.clone(), &patch.request.destination)?.clone();
                    assignments.insert(
                        saved_key(id, original)?,
                        rebind(&expression, &patch.request.subject, &binding)?,
                    );
                }
            }
            assignments.insert(key(id, &lifecycle.entity, "sealed"), boolean(true));
            assignments.insert(key(id, &lifecycle.entity, "day"), Expr::Today);
            assignments.insert(
                key(id, &lifecycle.entity, "origin"),
                literal(
                    DataValue::Text {
                        value: if archive.is_some() {
                            "ObservedAtArchive"
                        } else {
                            "ObservedAtCompletion"
                        }
                        .into(),
                    },
                    Type::Text,
                ),
            );
            // Keep the pass an ordinary bounded atomic language transaction.
            result.push(Statement::ForEach {
                items: query,
                binding: binding.clone(),
                steps: vec![Statement::Update {
                    record: variable(&binding),
                    values: assignments,
                }],
            });
        }
    }
    Ok(result)
}
fn add_provenance(
    app: &mut AppDefinition,
    snapshot: &ProjectSnapshot,
    manifest: &CompositionManifest,
) -> Result<()> {
    // Schema-carried provenance survives exported bytes, including mixed live
    // and saved rows; it is not merely a transient host caption.
    for id in &manifest.layers {
        let layer = &snapshot.scalar_scope()?.layers[id];
        for index in 0..layer.patches.len() {
            let patch = effective_patch(snapshot, manifest, id, index)?;
            let p = &patch.request;
            if !projection(&p.destination) {
                continue;
            }
            let origin = key(id, &p.entity, "origin");
            let day = key(id, &p.entity, "day");
            let record = key(id, &p.entity, "record");
            let values = [
                (
                    origin.clone(),
                    "Result provenance",
                    Type::Text,
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &origin)),
                        fallback: Box::new(literal(
                            DataValue::Text {
                                value: "CurrentCalculation".into(),
                            },
                            Type::Text,
                        )),
                    },
                ),
                (
                    day.clone(),
                    "Result day",
                    Type::Date,
                    Expr::Coalesce {
                        value: Box::new(field(&p.subject, &day)),
                        fallback: Box::new(Expr::Today),
                    },
                ),
                (
                    record,
                    "Source record",
                    Type::reference(&p.entity),
                    variable(&p.subject),
                ),
            ];
            match &p.destination {
                EffectDestination::ViewColumn { view, .. } => {
                    let v = app
                        .views
                        .iter_mut()
                        .find(|v| &v.id == view)
                        .ok_or_else(|| error("view removed"))?;
                    let columns = match &mut v.kind {
                        ViewKind::List { columns, .. } | ViewKind::Detail { columns, .. } => {
                            columns
                        }
                        _ => unreachable!(),
                    };
                    for (id, label, _, value) in values {
                        if !columns.iter().any(|c| c.id == id) {
                            columns.push(Column {
                                id,
                                label: label.into(),
                                value,
                            });
                        }
                    }
                }
                EffectDestination::EmitColumn { action, path, .. } => {
                    // Instrumentation changed statement positions, so locate every
                    // original matching output/binding rather than reusing indices.
                    let mut source = program(snapshot, &manifest.business)?.program.clone();
                    let a = source
                        .actions
                        .iter_mut()
                        .find(|a| &a.id == action)
                        .ok_or_else(|| error("output action absent"))?;
                    let output = if let Statement::Emit { output, .. } = step(&mut a.steps, path)? {
                        output.clone()
                    } else {
                        return Err(error("output slot absent"));
                    };
                    let definition = app
                        .outputs
                        .iter_mut()
                        .find(|o| o.id == output)
                        .ok_or_else(|| error("output definition absent"))?;
                    for (id, label, typ, _) in &values {
                        if !definition.columns.iter().any(|c| &c.id == id) {
                            definition.columns.push(FieldDefinition {
                                id: id.clone(),
                                label: label.to_string(),
                                value_type: typ.clone(),
                            });
                        }
                    }
                    fn append(
                        steps: &mut [Statement],
                        output: &str,
                        subject: &str,
                        values: &[(Id, &str, Type, Expr); 3],
                    ) -> Result<()> {
                        for step in steps {
                            match step {
                                Statement::Emit {
                                    output: id,
                                    binding,
                                    columns,
                                    ..
                                } if id == output => {
                                    for (id, _, _, expr) in values {
                                        columns.insert(id.clone(), rebind(expr, subject, binding)?);
                                    }
                                }
                                Statement::ForEach { steps, .. } => {
                                    append(steps, output, subject, values)?
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    }
                    for action in &mut app.actions {
                        append(&mut action.steps, &output, &p.subject, &values)?;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod borrowed_view_tests {
    use super::*;
    use crate::product_alloc::AllocationContext;
    use crate::product_contract::fallible_app_decode::{decode_app_definition, DecodedAppDefinition};
    use crate::product_contract::{AppRef, FieldDefinition, StateDefinition, Statement, EntityDefinition, MAX_ITEMS};
    use crate::product_store::scope::EffectPatchRequest;
    use crate::product_json;
    use std::num::NonZeroUsize;

    fn syntax(bytes: &[u8]) -> crate::product_json::Value {
        product_json::parse_json_bytes(bytes, &mut AllocationContext::new()).unwrap()
    }

    fn fixture() -> (AppDefinition, EffectPatchRequest) {
        let app = AppDefinition {
            version: 1,
            id: "workflow".into(),
            label: "Workflow".into(),
            entities: vec![EntityDefinition {
                id: "item".into(),
                label: "Item".into(),
                fields: vec![FieldDefinition {
                    id: "name".into(),
                    label: "Name".into(),
                    value_type: Type::Text,
                }],
                unique: vec![],
                constraints: vec![],
            }],
            state: vec![StateDefinition {
                id: "items".into(),
                label: "Items".into(),
                value_type: Type::list(Type::reference("item")),
                initial: DataValue::List {
                    item_type: Type::reference("item"),
                    items: vec![],
                },
            }],
            actions: vec![ActionDefinition {
                id: "edit".into(),
                label: "Edit".into(),
                parameters: BTreeMap::from([(
                    "items".into(),
                    Type::list(Type::reference("item")),
                )]),
                guards: vec![],
                steps: vec![Statement::ForEach {
                    items: Expr::Variable { name: "items".into() },
                    binding: "record".into(),
                    steps: vec![Statement::Update {
                        record: Expr::Variable { name: "record".into() },
                        values: BTreeMap::from([(
                            "name".into(),
                            Expr::Literal {
                                value: DataValue::Text { value: "old".into() },
                                value_type: Type::Text,
                            },
                        )]),
                    }],
                }],
                ensures: vec![],
            }],
            outputs: vec![],
            observables: vec![],
            views: vec![crate::product_contract::ViewDefinition {
                id: "main".into(),
                label: "Main".into(),
                kind: crate::product_contract::ViewKind::Form {
                    action: "edit".into(),
                    fields: vec![],
                    defaults: BTreeMap::from([("items".into(), DataValue::List {
                        item_type: Type::reference("item"), items: vec![],
                    })]),
                },
                actions: vec![],
                keys: vec![],
            }],
            initial_view: "main".into(),
        };
        let request = EffectPatchRequest {
            destination: EffectDestination::Update {
                action: "edit".into(),
                path: vec![0, 0],
                field: "name".into(),
            },
            entity: "item".into(),
            subject: "record".into(),
            value_type: Type::Text,
        };
        (app, request)
    }

    fn decoded_app(app: &AppDefinition) -> DecodedAppDefinition {
        let bytes = serde_json::to_vec(app).unwrap();
        decode_app_definition(syntax(&bytes), &mut AllocationContext::new()).unwrap()
    }

    fn decoded_request(request: &EffectPatchRequest) -> super::super::fallible_scalar_decode::EffectPatchRequestDto {
        let bytes = serde_json::to_vec(request).unwrap();
        super::super::fallible_scalar_decode::decode_effect_patch_request(
            syntax(&bytes),
            &mut AllocationContext::new(),
        ).unwrap()
    }

    fn decoded_capture(app: &AppDefinition) -> crate::product_contract::FallibleDecodedCapturedProgram {
        let bytes = serde_json::to_vec(app).unwrap();
        let captured = crate::product_contract::CapturedProgram::capture(
            &bytes,
            "project",
            crate::product_contract::Producer::Fixture { name: "compiler-fallible-test".into() },
            None,
        ).unwrap();
        let wire = serde_json::to_vec(&captured).unwrap();
        crate::product_contract::decode_captured_program_fallible(
            syntax(&wire),
            &mut AllocationContext::new(),
        ).unwrap()
    }

    fn minimal_snapshot() -> ProjectSnapshot {
        let source = r#"{"version":1,"id":"app","label":"App","entities":[{"id":"item","label":"Item","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[],"constraints":[]}],"state":[],"actions":[{"id":"noop","label":"No-op","parameters":{},"guards":[],"steps":[{"kind":"create","entity":"item","values":{"name":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"x"}}},"bind":"created"}],"ensures":[]}],"outputs":[],"observables":[{"id":"status","label":"Status","value":{"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":"before"}}}],"views":[{"id":"main","label":"Main","kind":{"kind":"form","action":"noop","fields":[],"defaults":{}},"actions":[],"keys":[] }],"initial_view":"main"}"#;
        let captured = crate::product_contract::CapturedProgram::capture(
            source.as_bytes(),
            "project",
            crate::product_contract::Producer::Fixture { name: "compiler-test".into() },
            None,
        ).unwrap();
        let data = crate::product_contract::DataSnapshot::empty("project", &captured.program).unwrap();
        let session = crate::product_contract::SessionState::initial(&captured.program).unwrap();
        let active_revision = crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Source, &captured).unwrap();
        ProjectSnapshot {
            magic: crate::product_store::MAGIC.into(),
            version: crate::product_store::GENERATED_PROJECT_FORMAT,
            revision: 1,
            programs: vec![captured],
            active_revision,
            data,
            session,
            clock_day: 0,
            decisions: crate::product_contract::DecisionGraph { version: crate::product_contract::CONTRACT_VERSION, revision: 0, decisions: vec![] },
            adoptions: vec![],
            operations: BTreeMap::new(),
            artifacts: vec![],
            scope: super::super::ScopeEnvelope::ScalarV1(super::super::ScopeState::default()),
        }
    }

    fn valid_scalar_adoption_snapshot() -> ProjectSnapshot {
        let mut snapshot = initialization_snapshot();
        let mut layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap().clone();
        layer.request.lifecycles[0].source = snapshot.active_revision.clone();
        let source = snapshot.programs[0].clone();
        let before = compiler::business_program(&snapshot).unwrap();
        layer.patches = compiler::derive_patches(&before, &source.program, &layer.request).unwrap();
        let corrected_layer_id = layer.identity().unwrap();
        let scope = snapshot.scalar_scope_mut().unwrap();
        scope.layers.clear();
        scope.layers.insert(corrected_layer_id, layer.clone());
        let source_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &source).unwrap();
        let layer_id = layer.identity().unwrap();
        let mut manifest = super::super::CompositionManifest {
            version: super::super::COMPILER_VERSION,
            basis: layer.basis.clone(),
            transition: super::super::ScopeTransition::Adoption,
            rewrites: vec![],
            business: source_revision.clone(),
            layers: vec![layer_id.clone()],
            active: BTreeSet::from([layer_id.clone()]),
            previous: None,
            operation: layer.operation.clone(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
            output: source_revision.clone(),
        };
        let mut target = None;
        for _ in 0..8 {
            let compiled = compiler::compile(&snapshot, &manifest).unwrap();
            let revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &compiled).unwrap();
            if revision == manifest.output { target = Some(compiled); break; }
            manifest.output = revision;
        }
        let target = target.expect("composition output revision should stabilize");
        let target_revision = manifest.output.clone();
        let (data, initialization) = super::super::history::initialize(&snapshot, &layer, &target).unwrap();
        let input_compatibility = crate::product_runtime::LocalRuntime::default()
            .compatibility_at(&target, &layer.basis.data, layer.basis.day).unwrap();
        let output_compatibility = crate::product_runtime::LocalRuntime::default()
            .compatibility_at(&target, &data, layer.basis.day).unwrap();
        let plan = crate::product_contract::AdoptionPlan {
            version: crate::product_contract::CONTRACT_VERSION,
            id: layer.operation.clone(),
            project_id: snapshot.data.project_id.clone(),
            expected_generation: layer.basis.data.generation,
            expected_data: layer.basis.data.identity().unwrap(),
            expected_decisions: layer.basis.decisions.clone(),
            expected_session: layer.basis.session.identity().unwrap(),
            current_source: source.binding.clone(),
            target: target.artifact.clone(),
            scope: layer.scope(),
            compatibility: input_compatibility,
            required_decisions: vec![],
            checks: vec![],
            evidence: vec![],
            retire_decisions: vec![],
        };
        let plan_id = crate::product_contract::canonical_digest(IdentityDomain::Adoption, &plan).unwrap();
        let composition_id = crate::product_contract::canonical_digest(IdentityDomain::Adoption, &manifest).unwrap();
        let initialization_id = crate::product_contract::canonical_digest(IdentityDomain::Adoption, &initialization).unwrap();
        let revision = snapshot.revision + 1;
        snapshot.revision = revision;
        snapshot.programs.push(target.clone());
        snapshot.active_revision = target_revision.clone();
        snapshot.data = data;
        snapshot.session = crate::product_contract::SessionState::initial(&target.program).unwrap();
        snapshot.adoptions.push(crate::product_store::AdoptionReceipt {
            plan: plan.clone(), previous: source_revision, active: target_revision.clone(), revision,
        });
        snapshot.operations.insert(plan.id.clone(), crate::product_store::OperationReceipt {
            operation: plan.id.clone(),
            request: crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"adoption-request").unwrap(),
            revision,
        });
        let scope = snapshot.scalar_scope_mut().unwrap();
        scope.initializations.push(initialization);
        scope.compositions.insert(target_revision.clone(), manifest);
        scope.adoptions.push(super::super::ScopedAdoptionReceipt {
            decisions: vec![], plan: plan_id, composition: composition_id,
            initialization: Some(initialization_id), initialized_compatibility: output_compatibility, revision,
        });
        snapshot
    }

    fn valid_scalar_decision_birth_snapshot() -> ProjectSnapshot {
        let mut snapshot = valid_scalar_adoption_snapshot();
        let decision_id = "accepted-on-adoption".to_owned();
        let target = snapshot.programs[1].clone();
        let witness = crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"accepted-witness").unwrap();
        let scenario = crate::product_contract::canonical_digest(IdentityDomain::Scenario, &"accepted-scenario").unwrap();
        let scope = {
            let plan = &mut snapshot.adoptions[0].plan;
            plan.required_decisions = vec![decision_id.clone()];
            plan.scope.clone()
        };
        snapshot.decisions.revision = 1;
        snapshot.decisions.decisions.push(crate::product_contract::ScopedDecision {
            id: decision_id.clone(),
            revision: 1,
            request: "Choose the accepted target".into(),
            rationale: None,
            scope,
            outcome: crate::product_contract::DecisionOutcome::Accept { artifact: target.artifact.program_digest.clone() },
            status: crate::product_contract::DecisionStatus::Active,
            obligations: vec![],
            scenarios: vec![scenario],
            witness,
            supersedes: vec![],
        });
        snapshot.scalar_scope_mut().unwrap().adoptions[0].decisions = vec![decision_id];
        let plan_id = crate::product_contract::canonical_digest(IdentityDomain::Adoption, &snapshot.adoptions[0].plan).unwrap();
        snapshot.scalar_scope_mut().unwrap().adoptions[0].plan = plan_id;
        snapshot
    }

    fn valid_scalar_rehearsal_snapshot() -> ProjectSnapshot {
        let mut snapshot = valid_scalar_adoption_snapshot();
        let active_revision = snapshot.active_revision.clone();
        let active = snapshot.program().unwrap().clone();
        let prior = snapshot.scalar_scope().unwrap().compositions[&active_revision].clone();
        let layer_id = prior.layers[0].clone();
        let mut candidate_app = snapshot.programs[0].program.clone();
        candidate_app.entities[0].fields.push(FieldDefinition {
            id: "evolution_note".into(),
            label: "Evolution note".into(),
            value_type: Type::Optional { item: Box::new(Type::Text) },
        });
        candidate_app.entities.push(EntityDefinition {
            id: "audit".into(), label: "Audit".into(),
            fields: vec![FieldDefinition { id: "note".into(), label: "Note".into(), value_type: Type::Text }],
            unique: vec![], constraints: vec![],
        });
        let candidate = crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&candidate_app).unwrap(),
            "project", crate::product_contract::Producer::Fixture { name: "scalar-rehearsal-candidate".into() }, None,
        ).unwrap();
        let candidate_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &candidate).unwrap();
        let decisions_before = snapshot.decisions.identity().unwrap();
        let basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            revision: snapshot.revision,
            active: active_revision.clone(),
            data: snapshot.data.clone(),
            session: snapshot.session.clone(),
            day: snapshot.clock_day,
            decisions: decisions_before.clone(),
        };
        let mut manifest = prior;
        manifest.basis = basis.clone();
        manifest.transition = super::super::ScopeTransition::Evolution;
        manifest.business = candidate_revision.clone();
        manifest.previous = Some(active_revision.clone());
        manifest.operation = "record-evolution".into();
        let layer = snapshot.scalar_scope().unwrap().layers[&layer_id].clone();
        let destination = layer.patches[0].request.destination.clone();
        manifest.rewrites = vec![super::super::ScopeSlotMapping {
            layer: layer_id,
            patch: 0,
            from_source: active_revision.clone(),
            from: destination.clone(),
            to_source: candidate_revision,
            to: destination,
            subject: "row".into(),
        }];
        snapshot.programs.push(candidate.clone());
        let target = compiler::compile(&snapshot, &manifest).unwrap();
        manifest.output = crate::product_contract::canonical_digest(IdentityDomain::Source, &target).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&target).unwrap(),
            crate::product_contract::canonical_bytes(&compiler::compile(&snapshot, &manifest).unwrap()).unwrap(),
        );
        let seed = crate::product_runtime::merged_data(&target, &basis.data).unwrap();
        let compatibility = crate::product_runtime::LocalRuntime::default()
            .compatibility_at(&target, &seed, basis.day).unwrap();
        let recorded_by = manifest.operation.clone();
        let recorded_revision = snapshot.revision + 1;
        let witness = crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"rehearsal-choice").unwrap();
        let rehearsal = super::super::ManagedRehearsal {
            version: super::super::COMPILER_VERSION,
            manifest,
            seed: seed.identity().unwrap(),
            layer: None,
            initialization: None,
            compatibility,
            recorded_by: recorded_by.clone(),
            recorded_revision,
            witnesses: BTreeMap::from([("retained-choice".into(), witness.clone())]),
        };
        snapshot.programs.push(target.clone());
        let scope = layer.scope();
        snapshot.decisions.revision += 1;
        snapshot.decisions.decisions.push(crate::product_contract::ScopedDecision {
            id: "retained-choice".into(),
            revision: 1,
            request: "Keep the alternative available".into(),
            rationale: None,
            scope,
            outcome: crate::product_contract::DecisionOutcome::EitherAcceptable,
            status: crate::product_contract::DecisionStatus::Pending,
            obligations: vec![],
            scenarios: vec![crate::product_contract::canonical_digest(IdentityDomain::Scenario, &"rehearsal-scenario").unwrap()],
            witness,
            supersedes: vec![],
        });
        let plan = crate::product_contract::AdoptionPlan {
            version: crate::product_contract::CONTRACT_VERSION,
            id: recorded_by.clone(),
            project_id: snapshot.data.project_id.clone(),
            expected_generation: snapshot.data.generation,
            expected_data: snapshot.data.identity().unwrap(),
            expected_decisions: decisions_before,
            expected_session: snapshot.session.identity().unwrap(),
            current_source: active.binding.clone(),
            target: active.artifact.clone(),
            scope: layer.scope(),
            compatibility: crate::product_runtime::LocalRuntime::default()
                .compatibility_at(&active, &snapshot.data, snapshot.clock_day).unwrap(),
            required_decisions: vec![],
            checks: vec![], evidence: vec![], retire_decisions: vec![],
        };
        snapshot.adoptions.push(crate::product_store::AdoptionReceipt {
            plan,
            previous: active_revision.clone(),
            active: active_revision,
            revision: recorded_revision,
        });
        snapshot.operations.insert(recorded_by.clone(), crate::product_store::OperationReceipt {
            operation: recorded_by.clone(),
            request: crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"record-rehearsal").unwrap(),
            revision: recorded_revision,
        });
        snapshot.revision = recorded_revision;
        snapshot.scalar_scope_mut().unwrap().rehearsals.insert(rehearsal.manifest.output.clone(), rehearsal);
        add_paired_evolution_rehearsal(&mut snapshot);
        snapshot
    }

    fn add_paired_evolution_rehearsal(snapshot: &mut ProjectSnapshot) {
        let active_revision = snapshot.active_revision.clone();
        let active = snapshot.program().unwrap().clone();
        let prior = snapshot.scalar_scope().unwrap().compositions[&active_revision].clone();
        let layer_id = prior.layers[0].clone();
        let original_basis = snapshot.scalar_scope().unwrap().rehearsals.values().next().unwrap().manifest.basis.clone();
        let original_proof = snapshot.scalar_scope().unwrap().rehearsals.values().next().unwrap().clone();
        let mut candidate_app = snapshot.programs[0].program.clone();
        candidate_app.entities[0].fields.push(FieldDefinition {
            id: "other_evolution_note".into(),
            label: "Other evolution note".into(),
            value_type: Type::Optional { item: Box::new(Type::Text) },
        });
        candidate_app.entities.push(EntityDefinition {
            id: "audit_alt".into(), label: "Alternate audit".into(),
            fields: vec![FieldDefinition { id: "note".into(), label: "Note".into(), value_type: Type::Text }],
            unique: vec![], constraints: vec![],
        });
        let candidate = crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&candidate_app).unwrap(),
            "project", crate::product_contract::Producer::Fixture { name: "scalar-rehearsal-alternative".into() }, None,
        ).unwrap();
        let candidate_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &candidate).unwrap();
        let layer = snapshot.scalar_scope().unwrap().layers[&layer_id].clone();
        let destination = layer.patches[0].request.destination.clone();
        let mut manifest = prior;
        manifest.basis = original_basis;
        manifest.transition = super::super::ScopeTransition::Evolution;
        manifest.business = candidate_revision.clone();
        manifest.previous = Some(active_revision.clone());
        manifest.operation = "record-evolution-alternative".into();
        manifest.rewrites = vec![super::super::ScopeSlotMapping {
            layer: layer_id,
            patch: 0,
            from_source: active_revision.clone(),
            from: destination.clone(),
            to_source: candidate_revision,
            to: destination,
            subject: "row".into(),
        }];
        snapshot.programs.push(candidate.clone());
        let target = compiler::compile(snapshot, &manifest).unwrap();
        manifest.output = crate::product_contract::canonical_digest(IdentityDomain::Source, &target).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&target).unwrap(),
            crate::product_contract::canonical_bytes(&compiler::compile(snapshot, &manifest).unwrap()).unwrap(),
        );
        let seed = crate::product_runtime::merged_data(&target, &manifest.basis.data).unwrap();
        let compatibility = crate::product_runtime::LocalRuntime::default()
            .compatibility_at(&target, &seed, manifest.basis.day).unwrap();
        let rehearsal = super::super::ManagedRehearsal {
            version: super::super::COMPILER_VERSION,
            seed: seed.identity().unwrap(),
            manifest,
            layer: None,
            initialization: None,
            compatibility,
            recorded_by: original_proof.recorded_by,
            recorded_revision: original_proof.recorded_revision,
            witnesses: original_proof.witnesses,
        };
        snapshot.programs.push(target.clone());
        snapshot.scalar_scope_mut().unwrap().rehearsals.insert(rehearsal.manifest.output.clone(), rehearsal);

        let pair_tag = crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"managed-evolution-pair/1").unwrap();
        let mut requests = Vec::new();
        let decisions = vec!["retained-choice".to_owned()];
        for (_, proof) in &snapshot.scalar_scope().unwrap().rehearsals {
            let business = snapshot.programs.iter().find(|program| {
                crate::product_contract::canonical_digest(IdentityDomain::Source, program).unwrap() == proof.manifest.business
            }).unwrap();
            let output = snapshot.programs.iter().find(|program| {
                crate::product_contract::canonical_digest(IdentityDomain::Source, program).unwrap() == proof.manifest.output
            }).unwrap();
            requests.push(crate::product_contract::canonical_digest(
                IdentityDomain::Adoption,
                &("managed-rehearsal/1", &proof.manifest.basis.snapshot, &proof.manifest, business, output, &proof.seed, &proof.compatibility, &decisions),
            ).unwrap());
        }
        requests.sort();
        let pair_request = crate::product_contract::canonical_digest(
            IdentityDomain::Adoption, &("managed-evolution-pair/1", requests),
        ).unwrap();
        let plan = &mut snapshot.adoptions.last_mut().unwrap().plan;
        plan.evidence.push(crate::product_contract::canonical_digest(IdentityDomain::Evidence, &"raw-evidence").unwrap());
        plan.evidence.push(pair_tag);
        plan.evidence.push(pair_request.clone());
        let recording_request = crate::product_contract::canonical_digest(
            IdentityDomain::Adoption,
            &("record-managed-rehearsal/1", &*plan, &active, &snapshot.decisions, &pair_request),
        ).unwrap();
        snapshot.operations.get_mut(&plan.id).unwrap().request = recording_request;
    }

    #[test]
    fn decoded_scalar_history_empty_checkpoint_matches_legacy_and_refuses_atomically() {
        use std::num::NonZeroUsize;
        let snapshot = minimal_snapshot();
        super::super::verify::validate(&snapshot).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        let mut baseline = AllocationContext::new();
        decoded.validate_scalar_scope_with(&mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            assert!(matches!(decoded.validate_scalar_scope_with(&mut refused), Err(crate::product_store::scope::ScalarValidationFailure::Allocation(_))), "refusal {nth}/{growths}");
            assert_eq!(before, crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap());
            let mut retry = AllocationContext::new();
            decoded.validate_scalar_scope_with(&mut retry).unwrap();
        }
    }

    #[test]
    fn decoded_scalar_full_adoption_matches_legacy_and_refuses_atomically() {
        use std::num::NonZeroUsize;
        let snapshot = valid_scalar_adoption_snapshot();
        super::super::verify::validate(&snapshot).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        let mut baseline = AllocationContext::new();
        decoded.validate_scalar_scope_with(&mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        // Exercise early, middle and final refusal points across the distinct
        // copy/collection families without repeating the whole full validator
        // for every unrelated digest scratch growth.
        for nth in [1, (growths / 2).max(1), growths] {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            assert!(matches!(decoded.validate_scalar_scope_with(&mut refused), Err(crate::product_store::scope::ScalarValidationFailure::Allocation(_))), "refusal {nth}/{growths}");
            assert_eq!(before, crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap());
            decoded.validate_scalar_scope_with(&mut AllocationContext::new()).unwrap();
        }
    }

    fn assert_scalar_validator_error_parity(snapshot: &ProjectSnapshot) -> String {
        let legacy = super::super::verify::validate(snapshot).map_err(|error| format!("{error:?}"));
        let decoded = decoded_snapshot(snapshot)
            .validate_scalar_scope_with(&mut AllocationContext::new())
            .map_err(|error| match error {
                crate::product_store::scope::ScalarValidationFailure::Store(error) => format!("{error:?}"),
                crate::product_store::scope::ScalarValidationFailure::Allocation(error) => format!("allocation: {error:?}"),
            });
        assert_eq!(decoded, legacy, "legacy and decoded scalar verifier outcomes differ");
        legacy.expect_err("the fixture must be rejected")
    }

    #[test]
    fn decoded_scalar_history_rejects_damaged_initialization_and_pair_receipts_like_legacy() {
        let mut initialization = valid_scalar_adoption_snapshot();
        initialization.scalar_scope_mut().unwrap().initializations[0].after_data =
            crate::product_contract::canonical_digest(IdentityDomain::Adoption, &"tampered-after-data").unwrap();
        assert!(assert_scalar_validator_error_parity(&initialization).contains(
            "metadata initialization does not independently regenerate"
        ));

        // A duplicated pair marker must be rejected before reverse graph
        // reconstruction can accept an otherwise valid adoption receipt.
        let mut paired_marker = valid_scalar_adoption_snapshot();
        let marker = crate::product_contract::canonical_digest(
            IdentityDomain::Evidence,
            &"managed-evolution-pair/1",
        ).unwrap();
        paired_marker.adoptions[0].plan.evidence.extend([marker.clone(), marker]);
        assert!(assert_scalar_validator_error_parity(&paired_marker).contains(
            "paired recording tag is duplicated, misplaced or incomplete"
        ));
    }

    #[test]
    fn decoded_scalar_reverse_birth_checkpoint_and_receipt_match_legacy() {
        let snapshot = valid_scalar_decision_birth_snapshot();
        super::super::verify::validate(&snapshot).unwrap();
        decoded_snapshot(&snapshot)
            .validate_scalar_scope_with(&mut AllocationContext::new())
            .unwrap();

        let mut missing_scoped_decision = snapshot;
        missing_scoped_decision.scalar_scope_mut().unwrap().adoptions[0].decisions.clear();
        assert!(assert_scalar_validator_error_parity(&missing_scoped_decision).contains(
            "scoped receipt differs from the exact newly activated intention set"
        ));
    }

    #[test]
    fn decoded_scalar_rehearsal_recording_chronology_matches_legacy() {
        let snapshot = valid_scalar_rehearsal_snapshot();
        super::super::verify::validate(&snapshot).unwrap();
        decoded_snapshot(&snapshot)
            .validate_scalar_scope_with(&mut AllocationContext::new())
            .unwrap();

        let mut corrupted = snapshot;
        let rehearsal = corrupted.scalar_scope_mut().unwrap().rehearsals.values_mut().next().unwrap();
        rehearsal.seed = crate::product_contract::canonical_digest(IdentityDomain::Adoption, &"forged-seed").unwrap();
        let error = assert_scalar_validator_error_parity(&corrupted);
        assert!(error.contains("rehearsal seed lacks exact provenance")
            || error.contains("paired recording proof inventory differs from its tagged evidence"), "{error}");
    }

    #[test]
    fn decoded_scalar_live_history_preserves_specific_legacy_failure() {
        let mut damaged = valid_scalar_adoption_snapshot();
        damaged.data.records[0].values.insert(
            "name".into(),
            crate::product_contract::DataValue::Text { value: "outside-history".into() },
        );
        let error = assert_scalar_validator_error_parity(&damaged);
        assert!(error.contains("current business record differs from recorded facts"), "{error}");
    }

    fn adoption_with_direct_correspondence() -> ProjectSnapshot {
        let mut snapshot = valid_scalar_adoption_snapshot();
        let layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap();
        let source = snapshot.programs[0].clone();
        let target = snapshot.programs[1].clone();
        let original = layer.basis.data.clone();
        let day = layer.basis.day;
        let registry = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap();
        let mapped = crate::product_runtime::merged_data(&target, &original).unwrap();
        let (registry, _) = registry.project_seed(&source, &original, &target, &mapped, day).unwrap();
        let proofs = registry.correspondence_proofs();
        assert_eq!(proofs.len(), 1);
        let (id, proof) = proofs.into_iter().next().unwrap();
        let adoption = snapshot.adoptions.last().unwrap();
        let operation = adoption.plan.id.clone();
        let revision = adoption.revision;
        snapshot.scalar_scope_mut().unwrap().correspondences.insert(id, super::super::ScopeCorrespondenceReceipt {
            proof,
            operation,
            revision,
        });
        snapshot
    }

    #[test]
    fn decoded_scalar_correspondence_receipt_and_target_chronology_match_legacy() {
        let snapshot = adoption_with_direct_correspondence();
        super::super::verify::validate(&snapshot).unwrap();
        decoded_snapshot(&snapshot)
            .validate_scalar_scope_with(&mut AllocationContext::new())
            .unwrap();

        let mut damaged = snapshot;
        damaged.scalar_scope_mut().unwrap().correspondences.values_mut().next().unwrap().revision += 1;
        assert!(assert_scalar_validator_error_parity(&damaged).contains(
            "invalid historical correspondence identity"
        ));
    }

    fn decoded_snapshot(snapshot: &ProjectSnapshot) -> crate::product_store::fallible_project_snapshot_decode::DecodedProjectSnapshot {
        let bytes = serde_json::to_vec(snapshot).unwrap();
        crate::product_store::fallible_project_snapshot_decode::decode_project_snapshot(
            syntax(&bytes),
            &mut AllocationContext::new(),
        ).unwrap()
    }

    fn composed_snapshot_with_observable_patch() -> ProjectSnapshot {
        let mut snapshot = minimal_snapshot();
        let program_revision = crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Source, &snapshot.programs[0]).unwrap();
        let decisions_digest = crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Decision, &snapshot.decisions).unwrap();
        let basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Data, &snapshot).unwrap(),
            revision: snapshot.revision,
            active: snapshot.active_revision.clone(),
            data: snapshot.data.clone(),
            session: snapshot.session.clone(),
            day: snapshot.clock_day,
            decisions: decisions_digest,
        };
        let request = EffectPatchRequest {
            destination: EffectDestination::Observable { observable: "status".into() },
            entity: "item".into(),
            subject: "record".into(),
            value_type: Type::Text,
        };
        let literal = |value: &str| Expr::Literal {
            value: DataValue::Text { value: value.into() },
            value_type: Type::Text,
        };
        let layer = super::super::ScopeLayer {
            version: super::super::COMPILER_VERSION,
            operation: "edit".into(),
            basis: basis.clone(),
            candidate: program_revision.clone(),
            request: ScopeRequest {
                population: ScopePopulation::All,
                operations: BTreeSet::from(["noop".into()]),
                excluded_records: vec![],
                lifecycles: vec![],
                patches: vec![request.clone()],
            },
            patches: vec![EffectPatch { request, before: literal("before"), after: literal("after") }],
        };
        let layer_id = layer.identity().unwrap();
        let mut state = super::super::ScopeState::default();
        state.layers.insert(layer_id.clone(), layer);
        state.compositions.insert(snapshot.active_revision.clone(), super::super::CompositionManifest {
            version: super::super::COMPILER_VERSION,
            basis,
            transition: super::super::ScopeTransition::Adoption,
            rewrites: vec![],
            business: program_revision,
            layers: vec![layer_id.clone()],
            active: BTreeSet::from([layer_id.clone()]),
            previous: None,
            operation: "compose".into(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
            output: snapshot.active_revision.clone(),
        });
        snapshot.scope = super::super::ScopeEnvelope::ScalarV1(state);
        snapshot
    }

    fn composed_snapshot_with_view_projection_patch() -> ProjectSnapshot {
        let mut snapshot = minimal_snapshot();
        let mut app = snapshot.programs[0].program.clone();
        app.views[0].kind = crate::product_contract::ViewKind::List {
            entity: "item".into(),
            rows: Expr::Query {
                entity: "item".into(),
                binding: "row".into(),
                predicate: Box::new(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }),
                sort: vec![],
                limit: MAX_COLLECTION,
                include_archived: false,
            },
            columns: vec![crate::product_contract::Column {
                id: "name".into(),
                label: "Name".into(),
                value: Expr::Field {
                    record: Box::new(Expr::Variable { name: "row".into() }),
                    field: "name".into(),
                },
            }],
            controls: vec![],
            selection: None,
        };
        let source_bytes = crate::product_contract::canonical_bytes(&app).unwrap();
        let captured = crate::product_contract::CapturedProgram::capture(
            &source_bytes, "project", crate::product_contract::Producer::Fixture { name: "compiler-view-test".into() }, None,
        ).unwrap();
        snapshot.programs = vec![captured.clone()];
        snapshot.active_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &captured).unwrap();
        snapshot.data = crate::product_contract::DataSnapshot::empty("project", &captured.program).unwrap();
        snapshot.session = crate::product_contract::SessionState::initial(&captured.program).unwrap();
        let program_revision = snapshot.active_revision.clone();
        let decisions = crate::product_contract::canonical_digest(IdentityDomain::Decision, &snapshot.decisions).unwrap();
        let basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            revision: snapshot.revision,
            active: snapshot.active_revision.clone(),
            data: snapshot.data.clone(),
            session: snapshot.session.clone(),
            day: snapshot.clock_day,
            decisions,
        };
        let destination = EffectDestination::ViewColumn { view: "main".into(), column: "name".into() };
        let request = EffectPatchRequest { destination: destination.clone(), entity: "item".into(), subject: "row".into(), value_type: Type::Text };
        let before = Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "name".into() };
        let after = Expr::If {
            condition: Box::new(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }),
            then_value: Box::new(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "unknown".into() } }),
            else_value: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "name".into() }),
        };
        let layer = super::super::ScopeLayer {
            version: super::super::COMPILER_VERSION,
            operation: "view-edit".into(),
            basis: basis.clone(),
            candidate: program_revision.clone(),
            request: ScopeRequest {
                population: ScopePopulation::All,
                operations: BTreeSet::from(["noop".into()]),
                excluded_records: vec![],
                lifecycles: vec![super::super::LifecycleBinding {
                    entity: "item".into(),
                    completed: Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } },
                    source: crate::product_contract::canonical_digest(IdentityDomain::Adoption, &"view-lifecycle").unwrap(),
                }],
                patches: vec![request.clone()],
            },
            patches: vec![EffectPatch { request, before, after }],
        };
        let layer_id = layer.identity().unwrap();
        let mut state = super::super::ScopeState::default();
        state.layers.insert(layer_id.clone(), layer);
        state.compositions.insert(snapshot.active_revision.clone(), super::super::CompositionManifest {
            version: super::super::COMPILER_VERSION,
            basis,
            transition: super::super::ScopeTransition::Adoption,
            rewrites: vec![],
            business: program_revision,
            layers: vec![layer_id.clone()],
            active: BTreeSet::from([layer_id]),
            previous: None,
            operation: "view-compose".into(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
            output: snapshot.active_revision.clone(),
        });
        snapshot.scope = super::super::ScopeEnvelope::ScalarV1(state);
        snapshot
    }

    fn initialization_snapshot() -> ProjectSnapshot {
        let mut snapshot = minimal_snapshot();
        let mut app = snapshot.programs[0].program.clone();
        app.views[0].kind = crate::product_contract::ViewKind::List {
            entity: "item".into(),
            rows: Expr::Query { entity: "item".into(), binding: "row".into(),
                predicate: Box::new(Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } }),
                sort: vec![], limit: MAX_COLLECTION, include_archived: false },
            columns: vec![crate::product_contract::Column { id: "name".into(), label: "Name".into(),
                value: Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "name".into() } }],
            controls: vec![], selection: None,
        };
        let capture = crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&app).unwrap(), "project",
            crate::product_contract::Producer::Fixture { name: "initialization-test".into() }, None,
        ).unwrap();
        snapshot.programs = vec![capture.clone()];
        snapshot.active_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &capture).unwrap();
        snapshot.data = crate::product_contract::DataSnapshot::empty("project", &capture.program).unwrap();
        snapshot.session = crate::product_contract::SessionState::initial(&capture.program).unwrap();
        let mut record_values = BTreeMap::new();
        record_values.insert("name".into(), DataValue::Text { value: "Ada".into() });
        snapshot.data.records.push(crate::product_contract::Record {
            entity: "item".into(), id: "row-1".into(), revision: 1,
            created_program: capture.artifact.program_digest.clone(), archived: false,
            values: record_values,
        });
        snapshot.data.validate().unwrap();
        let active = snapshot.active_revision.clone();
        let decision_identity = crate::product_contract::canonical_digest(IdentityDomain::Decision, &snapshot.decisions).unwrap();
        let basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            revision: snapshot.revision, active: active.clone(), data: snapshot.data.clone(),
            session: snapshot.session.clone(), day: 3, decisions: decision_identity,
        };
        let destination = EffectDestination::ViewColumn { view: "main".into(), column: "name".into() };
        let request = EffectPatchRequest { destination: destination.clone(), entity: "item".into(), subject: "row".into(), value_type: Type::Text };
        let layer = super::super::ScopeLayer {
            version: super::super::COMPILER_VERSION, operation: "adopt-row".into(), basis: basis.clone(),
            candidate: active.clone(),
            request: ScopeRequest {
                population: ScopePopulation::All, operations: BTreeSet::from(["noop".into()]),
                excluded_records: vec![],
                lifecycles: vec![super::super::LifecycleBinding {
                    entity: "item".into(),
                    completed: Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } },
                    source: crate::product_contract::canonical_digest(IdentityDomain::Adoption, &"lifecycle-source").unwrap(),
                }],
                patches: vec![request.clone()],
            },
            patches: vec![EffectPatch {
                request,
                before: Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "name".into() },
                after: Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "new".into() } },
            }],
        };
        let layer_id = layer.identity().unwrap();
        let mut scope = super::super::ScopeState::default();
        scope.layers.insert(layer_id, layer);
        snapshot.scope = super::super::ScopeEnvelope::ScalarV1(scope);
        snapshot.clock_day = 3;
        snapshot
    }

    fn initialization_target(snapshot: &ProjectSnapshot) -> crate::product_contract::CapturedProgram {
        let layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap();
        let id = layer.identity().unwrap();
        let mut app = snapshot.programs[0].program.clone();
        for (suffix, typ, label) in [
            ("member", Type::Boolean, "Scope membership"),
            ("sealed", Type::Boolean, "Preserved result"),
            ("day", Type::Date, "Result capture day"),
            ("origin", Type::Text, "Result provenance"),
        ] {
            app.entities[0].fields.push(FieldDefinition {
                id: compiler::key(&id, "item", suffix), label: label.into(), value_type: optional(&typ),
            });
        }
        app.entities[0].fields.push(FieldDefinition {
            id: compiler::saved_key(&id, &layer.patches[0]).unwrap(),
            label: "Preserved completed result".into(), value_type: optional(&Type::Text),
        });
        crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&app).unwrap(), "project",
            crate::product_contract::Producer::Fixture { name: "initialization-target".into() }, None,
        ).unwrap()
    }

    fn correspondence_fixture(
        historical: bool,
    ) -> (
        ProjectSnapshot,
        crate::product_contract::CapturedProgram,
        crate::product_contract::DataSnapshot,
        i32,
    ) {
        use crate::product_contract::{BusinessEvent, RecordChange};

        let mut snapshot = initialization_snapshot();
        let original = snapshot.data.clone();
        if historical {
            let mut later = original.clone();
            let values = later.records[0].values.clone();
            later.generation = 1;
            later.records[0].revision = 2;
            later.events.push(BusinessEvent {
                id: "event-1".into(),
                sequence: 1,
                operation_id: "operation-1".into(),
                action: "noop".into(),
                day: 1,
                program: later.records[0].created_program.clone(),
                changes: vec![RecordChange {
                    entity: "item".into(),
                    record: "row-1".into(),
                    before: Some(values.clone()),
                    after: values,
                    archived: false,
                }],
                outputs: vec![],
            });
            snapshot.data = later;
            snapshot.revision += 1;
        }

        let source = snapshot.programs[0].clone();
        let source_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source,
            &source,
        ).unwrap();
        let decisions = crate::product_contract::canonical_digest(
            IdentityDomain::Decision,
            &snapshot.decisions,
        ).unwrap();
        let mut basis_root = snapshot.clone();
        basis_root.scope = super::super::ScopeEnvelope::ScalarV1(
            super::super::ScopeState::default(),
        );
        let basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(IdentityDomain::Data, &basis_root).unwrap(),
            revision: snapshot.revision,
            active: source_revision.clone(),
            data: snapshot.data.clone(),
            session: snapshot.session.clone(),
            day: 3,
            decisions,
        };

        let original_layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap();
        let mut state = super::super::ScopeState::default();
        let layer = super::super::ScopeLayer {
            version: super::super::COMPILER_VERSION,
            operation: "historical-adoption".into(),
            basis: basis.clone(),
            candidate: source_revision.clone(),
            request: original_layer.request.clone(),
            patches: original_layer.patches.clone(),
        };
        let layer_id = layer.identity().unwrap();
        state.layers.insert(layer_id.clone(), layer);
        snapshot.scope = super::super::ScopeEnvelope::ScalarV1(state);
        let target = initialization_target(&snapshot);
        let target_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source,
            &target,
        ).unwrap();
        let manifest = super::super::CompositionManifest {
            version: super::super::COMPILER_VERSION,
            basis,
            transition: super::super::ScopeTransition::Adoption,
            rewrites: vec![],
            business: source_revision.clone(),
            layers: vec![layer_id.clone()],
            active: BTreeSet::from([layer_id.clone()]),
            previous: None,
            operation: "historical-composition".into(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
            output: target_revision.clone(),
        };
        snapshot.scalar_scope_mut().unwrap().compositions.insert(target_revision, manifest);
        snapshot.programs.push(target.clone());
        (snapshot, target, original, if historical { 2 } else { 3 })
    }

    fn decoded_captured(
        capture: &crate::product_contract::CapturedProgram,
    ) -> crate::product_contract::FallibleDecodedCapturedProgram {
        crate::product_contract::decode_captured_program_fallible(
            syntax(&serde_json::to_vec(capture).unwrap()),
            &mut AllocationContext::new(),
        ).unwrap()
    }

    fn captured_program_with_observable(value: &str) -> crate::product_contract::CapturedProgram {
        let source = format!(r#"{{"version":1,"id":"app","label":"App","entities":[{{"id":"item","label":"Item","fields":[{{"id":"name","label":"Name","value_type":{{"kind":"text"}}}}],"unique":[],"constraints":[]}}],"state":[],"actions":[{{"id":"noop","label":"No-op","parameters":{{}},"guards":[],"steps":[{{"kind":"create","entity":"item","values":{{"name":{{"kind":"literal","value_type":{{"kind":"text"}},"value":{{"kind":"text","value":"x"}}}}}},"bind":"created"}}],"ensures":[]}}],"outputs":[],"observables":[{{"id":"status","label":"Status","value":{{"kind":"literal","value_type":{{"kind":"text"}},"value":{{"kind":"text","value":"{value}"}}}}}}],"views":[{{"id":"main","label":"Main","kind":{{"kind":"form","action":"noop","fields":[],"defaults":{{}}}},"actions":[],"keys":[]}}],"initial_view":"main"}}"#);
        crate::product_contract::CapturedProgram::capture(
            source.as_bytes(),
            "project",
            crate::product_contract::Producer::Fixture { name: "compiler-test-candidate".into() },
            None,
        ).unwrap()
    }

    fn environment_entries<M: StringValueMap<Type>>(map: &M) -> Vec<(String, Type)> {
        map.iter().map(|(key, value)| (key.clone(), value.clone())).collect()
    }

    #[test]
    fn decoded_and_legacy_context_share_nested_statement_and_type_lookup() {
        let (app, request) = fixture();
        let decoded = decoded_app(&app);
        let request_dto = decoded_request(&request);
        let legacy = locate_context(&app, &request).unwrap();
        let mut cx = AllocationContext::new();
        let fallible = locate_context_fallible(&decoded, &request_dto, &mut cx).unwrap();
        assert_eq!(environment_entries(&legacy), environment_entries(&fallible));
        assert_eq!(legacy.get("record"), Some(&Type::reference("item")));

        let own_row_expr = Expr::Field {
            record: Box::new(Expr::Variable { name: "record".into() }),
            field: "name".into(),
        };
        own_row(&own_row_expr, "record", "item", &app, &legacy, true).unwrap();
        own_row(&own_row_expr, "record", "item", AppRef::from(&decoded), &fallible, true).unwrap();

        let collection = Expr::Variable { name: "items".into() };
        assert_eq!(
            list_entity(&collection, &app, &legacy),
            list_entity(&collection, AppRef::from(&decoded), &fallible),
        );
    }

    #[test]
    fn decoded_context_refusal_is_typed_and_fresh_retry_succeeds() {
        let (app, request) = fixture();
        let decoded = decoded_app(&app);
        let request = decoded_request(&request);
        let mut baseline = AllocationContext::new();
        locate_context_fallible(&decoded, &request, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < MAX_ITEMS, "growth count: {growths}");

        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(
                locate_context_fallible(&decoded, &request, &mut refused),
                Err(CompilerFailure::Allocation(_))
            ));
            let mut retry = AllocationContext::new();
            locate_context_fallible(&decoded, &request, &mut retry).unwrap();
        }
    }

    #[test]
    fn decoded_patch_derivation_matches_legacy_and_refuses_atomically() {
        let (baseline_app, patch) = fixture();
        let mut candidate_app = baseline_app.clone();
        let Statement::ForEach { steps, .. } = &mut candidate_app.actions[0].steps[0] else { panic!("fixture loop") };
        let Statement::Update { values, .. } = &mut steps[0] else { panic!("fixture update") };
        values.insert("name".into(), Expr::Literal {
            value: DataValue::Text { value: "new".into() },
            value_type: Type::Text,
        });
        let request = ScopeRequest {
            population: ScopePopulation::All,
            operations: BTreeSet::from(["edit".into()]),
            excluded_records: vec![],
            lifecycles: vec![LifecycleBinding {
                entity: "item".into(),
                completed: Expr::Literal { value: DataValue::Boolean { value: true }, value_type: Type::Boolean },
                source: canonical_digest(IdentityDomain::Adoption, &"test").unwrap(),
            }],
            patches: vec![patch],
        };
        let baseline = decoded_capture(&baseline_app);
        let candidate = decoded_capture(&candidate_app);
        let request_wire = serde_json::to_vec(&request).unwrap();
        let request = super::super::fallible_scalar_decode::decode_scope_request(
            syntax(&request_wire), &mut AllocationContext::new(),
        ).unwrap();
        let expected = derive_patches(&baseline_app, &candidate_app, &serde_json::from_slice::<ScopeRequest>(&request_wire).unwrap()).unwrap();
        let mut success = AllocationContext::new();
        let actual = derive_patches_fallible(&baseline, &candidate, &request, &mut success).unwrap();
        assert_eq!(crate::product_contract::canonical_bytes(&expected).unwrap(), crate::product_contract::canonical_bytes(&actual).unwrap());
        let growths = success.growth_requests();
        assert!(growths > 0, "expected bounded fallible compiler growth");
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(derive_patches_fallible(&baseline, &candidate, &request, &mut refused), Err(CompilerFailure::Allocation(_))), "growth refusal {nth}/{growths}");
            let mut retry = AllocationContext::new();
            let retried = derive_patches_fallible(&baseline, &candidate, &request, &mut retry).unwrap();
            assert_eq!(crate::product_contract::canonical_bytes(&expected).unwrap(), crate::product_contract::canonical_bytes(&retried).unwrap());
        }
    }

    #[test]
    fn fallible_patch_derivation_preserves_reserved_namespace_error_priority() {
        let (baseline_app, _) = fixture();
        let mut reserved_candidate = baseline_app.clone();
        reserved_candidate.label = format!("{}test", super::RESERVED_PREFIX);
        let baseline = decoded_capture(&baseline_app);
        let candidate = decoded_capture(&reserved_candidate);
        let empty = ScopeRequest {
            population: ScopePopulation::All,
            operations: BTreeSet::new(),
            excluded_records: vec![],
            lifecycles: vec![],
            patches: vec![],
        };
        let expected = derive_patches(&baseline_app, &reserved_candidate, &empty).unwrap_err();
        let empty_wire = serde_json::to_vec(&empty).unwrap();
        let empty_dto = super::super::fallible_scalar_decode::decode_scope_request(
            syntax(&empty_wire), &mut AllocationContext::new(),
        ).unwrap();
        let actual = derive_patches_fallible(&baseline, &candidate, &empty_dto, &mut AllocationContext::new()).unwrap_err();
        let expected_message = match expected {
            StoreError::Invalid(message) => message,
            other => panic!("unexpected legacy classification: {other:?}"),
        };
        let actual_message = match actual {
            CompilerFailure::Store(StoreError::Invalid(message)) => message,
            other => panic!("unexpected fallible classification: {other:?}"),
        };
        assert_eq!(actual_message, expected_message);
    }

    #[test]
    fn decoded_and_legacy_slot_mutation_preserve_same_wire_graph() {
        let (mut app, request) = fixture();
        let mut decoded = decoded_app(&app);
        let request_dto = decoded_request(&request);
        let replacement = Expr::Variable { name: "new_value".into() };
        *slot(&mut app, &request.destination).unwrap() = replacement.clone();
        *slot_fallible(&mut decoded, &request_dto.destination).unwrap() = replacement;
        assert_eq!(
            crate::product_contract::canonical_bytes(&app).unwrap(),
            crate::product_contract::canonical_bytes(&decoded).unwrap(),
        );
    }

    #[test]
    fn fallible_rebind_matches_legacy_and_refuses_with_retry() {
        let expr = Expr::Query {
            entity: "item".into(),
            binding: "row".into(),
            predicate: Box::new(Expr::And { values: vec![
                Expr::Variable { name: "record".into() },
                Expr::Field {
                    record: Box::new(Expr::Variable { name: "record".into() }),
                    field: "name".into(),
                },
            ] }),
            sort: vec![crate::product_contract::SortKey {
                value: Expr::Variable { name: "record".into() },
                descending: false,
            }],
            limit: 3,
            include_archived: false,
        };
        let expected = rebind(&expr, "record", "row").unwrap();
        let mut baseline = AllocationContext::new();
        let actual = rebind_fallible(&expr, "record", "row", &mut baseline).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected).unwrap(),
            crate::product_contract::canonical_bytes(&actual).unwrap(),
        );
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < MAX_ITEMS);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(rebind_fallible(&expr, "record", "row", &mut refused), Err(CompilerFailure::Allocation(_))));
            let mut retry = AllocationContext::new();
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected).unwrap(),
                crate::product_contract::canonical_bytes(&rebind_fallible(&expr, "record", "row", &mut retry).unwrap()).unwrap(),
            );
        }
    }

    #[test]
    fn borrowed_dependency_walk_matches_legacy_rules_without_allocation() {
        let base = captured_program_with_observable("status").program;
        let mut cases = vec![base.clone()];

        let mut unique = base.clone();
        unique.entities[0].unique.push(vec!["name".into()]);
        cases.push(unique);

        let mut constraint = base.clone();
        constraint.entities[0].constraints.push(Expr::Field {
            record: Box::new(Expr::Variable { name: "record".into() }),
            field: "name".into(),
        });
        cases.push(constraint);

        let mut operator = base.clone();
        operator.observables[0].value = Expr::Map {
            items: Box::new(Expr::Variable { name: "items".into() }),
            binding: "row".into(),
            value: Box::new(Expr::Field {
                record: Box::new(Expr::Variable { name: "row".into() }),
                field: "name".into(),
            }),
        };
        cases.push(operator);

        let mut view_binding = base.clone();
        view_binding.views[0].actions.push(crate::product_contract::ActionBinding {
            id: "inspect".into(),
            label: "Inspect".into(),
            placement: crate::product_contract::ActionPlacement::Toolbar,
            action: "noop".into(),
            arguments: BTreeMap::new(),
            enabled: Expr::Field {
                record: Box::new(Expr::Variable { name: "row".into() }),
                field: "name".into(),
            },
        });
        cases.push(view_binding);

        let mut own_row = base.clone();
        own_row.observables[0].value = Expr::Field {
            record: Box::new(Expr::Variable { name: "row".into() }),
            field: "name".into(),
        };
        cases.push(own_row);

        let mut collection = base.clone();
        collection.actions[0].steps.push(Statement::Collection {
            target: CollectionTarget::Field {
                record: Expr::Variable { name: "record".into() },
                field: "name".into(),
            },
            operation: crate::product_contract::CollectionOperation::Insert,
            items: Expr::Literal {
                value_type: Type::Text,
                value: DataValue::Text { value: "x".into() },
            },
        });
        cases.push(collection);

        for app in cases {
            let expected = check_dependencies(&app, "item", "name").map_err(|error| error.to_string());
            let decoded = decoded_app(&app);
            let actual = check_dependencies_core(AppRef::from(&decoded), "item", "name")
                .map_err(|failure| match failure {
                    CompilerFailure::Store(error) => error.to_string(),
                    CompilerFailure::Allocation(failure) => format!("allocation refusal: {failure:?}"),
                });
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn decoded_business_projection_matches_legacy_and_refusal_retries() {
        let snapshot = minimal_snapshot();
        let decoded = decoded_snapshot(&snapshot);
        let expected = business_program(&snapshot).unwrap();
        let mut baseline = AllocationContext::new();
        let actual = business_program_fallible(&decoded, &mut baseline).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected).unwrap(),
            crate::product_contract::canonical_bytes(&actual).unwrap(),
        );
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < crate::product_contract::MAX_COLLECTION, "business projection growth count: {growths}");
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(business_program_fallible(&decoded, &mut refused), Err(CompilerFailure::Allocation(_))));
            let mut retry = AllocationContext::new();
            let retried = business_program_fallible(&decoded, &mut retry).unwrap();
            assert_eq!(crate::product_contract::canonical_bytes(&expected).unwrap(), crate::product_contract::canonical_bytes(&retried).unwrap());
        }
    }

    #[test]
    fn decoded_business_projection_replays_composed_patch_and_refusal() {
        let snapshot = composed_snapshot_with_observable_patch();
        let decoded = decoded_snapshot(&snapshot);
        let expected = business_program(&snapshot).unwrap();
        let mut baseline = AllocationContext::new();
        let actual = business_program_fallible(&decoded, &mut baseline).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected).unwrap(),
            crate::product_contract::canonical_bytes(&actual).unwrap(),
        );
        assert!(matches!(actual.observables[0].value, Expr::Literal { value: DataValue::Text { ref value }, .. } if value == "after"));
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < crate::product_contract::MAX_COLLECTION);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(business_program_fallible(&decoded, &mut refused), Err(CompilerFailure::Allocation(_))));
            let mut retry = AllocationContext::new();
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected).unwrap(),
                crate::product_contract::canonical_bytes(&business_program_fallible(&decoded, &mut retry).unwrap()).unwrap(),
            );
        }
    }

    #[test]
    fn fallible_scalar_initialization_matches_legacy_frame_and_receipt() {
        let snapshot = initialization_snapshot();
        let scope = snapshot.scalar_scope().unwrap();
        let (layer_id, layer) = scope.layers.iter().next().unwrap();
        let target = initialization_target(&snapshot);
        let (legacy_after, legacy_receipt) = super::super::history::initialize(&snapshot, layer, &target).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let decoded_layer = decoded.scalar_scope().unwrap().layer(layer_id).unwrap();
        let decoded_target = decoded_capture(&target.program);

        let mut cx = AllocationContext::new();
        let (after, additions) = crate::product_store::scope::initialize_scalar_fallible(
            &decoded, decoded_layer, &decoded_target, &mut cx,
        ).unwrap();
        assert_eq!(after.canonical_bytes_with(&mut AllocationContext::new()).unwrap(), crate::product_contract::canonical_bytes(&legacy_after).unwrap());
        assert_eq!(crate::product_contract::canonical_bytes(&additions).unwrap(), crate::product_contract::canonical_bytes(&legacy_receipt.additions).unwrap());

        let mut receipt_cx = AllocationContext::new();
        let receipt = crate::product_store::scope::scalar_initialization_receipt(
            &decoded, decoded_layer, decoded_layer.basis().data(), &after, &additions, &mut receipt_cx,
        ).unwrap();
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &receipt, &mut AllocationContext::new(),
        ).unwrap(), crate::product_contract::canonical_bytes(&legacy_receipt).unwrap());
        let member = compiler::key(layer_id, "item", "member");
        let sealed = compiler::key(layer_id, "item", "sealed");
        let day = compiler::key(layer_id, "item", "day");
        let origin = compiler::key(layer_id, "item", "origin");
        let record = DataSnapshotView::from(&after).record("item", "row-1").unwrap();
        assert_eq!(record.values().get(&member), Some(&DataValue::Boolean { value: true }));
        assert_eq!(record.values().get(&sealed), Some(&DataValue::Boolean { value: true }));
        assert_eq!(record.values().get(&day), Some(&DataValue::Date { days: 3 }));
        assert_eq!(record.values().get(&origin), Some(&DataValue::Text { value: "CapturedAtAdoption".into() }));
        let saved = compiler::saved_key(layer_id, &layer.patches[0]).unwrap();
        assert_eq!(record.values().get(&saved), Some(&DataValue::Text { value: "Ada".into() }));
    }

    #[test]
    fn fallible_scalar_initialization_refusal_is_atomic_and_retryable() {
        use std::num::NonZeroUsize;
        let snapshot = initialization_snapshot();
        let before = crate::product_contract::canonical_bytes(&snapshot).unwrap();
        let scope = snapshot.scalar_scope().unwrap();
        let (layer_id, layer) = scope.layers.iter().next().unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let decoded_layer = decoded.scalar_scope().unwrap().layer(layer_id).unwrap();
        let target_capture = initialization_target(&snapshot);
        let target = decoded_capture(&target_capture.program);
        let decoded_before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        let mut counter = AllocationContext::new();
        crate::product_store::scope::initialize_scalar_fallible(&decoded, decoded_layer, &target, &mut counter).unwrap();
        let growths = counter.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            assert!(matches!(crate::product_store::scope::initialize_scalar_fallible(
                &decoded, decoded_layer, &target, &mut refused,
            ), Err(crate::product_store::scope::InitializationFailure::Allocation(_))), "growth {nth}/{growths}");
            assert!(refused.first_failure().is_some(), "allocation context must retain refusal at growth {nth}");
            assert_eq!(crate::product_contract::canonical_bytes(&snapshot).unwrap(), before);
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap(), decoded_before);
            let mut retry = AllocationContext::new();
            crate::product_store::scope::initialize_scalar_fallible(&decoded, decoded_layer, &target, &mut retry).unwrap();
        }

        let mut success_cx = AllocationContext::new();
        let (after, additions) = crate::product_store::scope::initialize_scalar_fallible(
            &decoded, decoded_layer, &target, &mut success_cx,
        ).unwrap();
        let mut count_cx = AllocationContext::new();
        crate::product_store::scope::scalar_initialization_receipt(
            &decoded, decoded_layer, decoded_layer.basis().data(), &after, &additions, &mut count_cx,
        ).unwrap();
        let receipt_growths = count_cx.growth_requests();
        assert!(receipt_growths > 0);
        let after_bytes = crate::product_backup::fallible_codec::canonical_bytes(&after, &mut AllocationContext::new()).unwrap();
        let additions_bytes: Vec<_> = additions.iter().map(|addition|
            crate::product_backup::fallible_codec::canonical_bytes(addition, &mut AllocationContext::new()).unwrap()
        ).collect();
        for nth in 1..=receipt_growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            assert!(matches!(crate::product_store::scope::scalar_initialization_receipt(
                &decoded, decoded_layer, decoded_layer.basis().data(), &after, &additions, &mut refused,
            ), Err(crate::product_store::scope::InitializationFailure::Allocation(_))), "receipt growth {nth}/{receipt_growths}");
            assert!(refused.first_failure().is_some(), "receipt allocation context must retain refusal at growth {nth}");
            assert_eq!(crate::product_contract::canonical_bytes(&snapshot).unwrap(), before);
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(&after, &mut AllocationContext::new()).unwrap(), after_bytes);
            assert_eq!(additions.len(), additions_bytes.len());
            for (addition, expected) in additions.iter().zip(&additions_bytes) {
                assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(addition, &mut AllocationContext::new()).unwrap(), *expected);
            }
            let mut retry = AllocationContext::new();
            crate::product_store::scope::scalar_initialization_receipt(
                &decoded, decoded_layer, decoded_layer.basis().data(), &after, &additions, &mut retry,
            ).unwrap();
        }
    }

    #[test]
    fn fallible_scalar_initialization_rejects_bad_completion_without_input_mutation() {
        let mut snapshot = initialization_snapshot();
        let old_layer_id = snapshot.scalar_scope().unwrap().layers.keys().next().unwrap().clone();
        let scope = snapshot.scalar_scope_mut().unwrap();
        let mut layer = scope.layers.remove(&old_layer_id).unwrap();
        layer.request.lifecycles[0].completed = Expr::Literal {
            value_type: Type::Text, value: DataValue::Text { value: "not boolean".into() },
        };
        let layer_id = layer.identity().unwrap();
        scope.layers.insert(layer_id.clone(), layer);
        let before = crate::product_contract::canonical_bytes(&snapshot).unwrap();
        let legacy_layer = snapshot.scalar_scope().unwrap().layers.get(&layer_id).unwrap();
        let target_capture = initialization_target(&snapshot);
        let target = &target_capture;
        assert!(super::super::history::initialize(&snapshot, legacy_layer, target).is_err());

        let decoded = decoded_snapshot(&snapshot);
        let decoded_layer = decoded.scalar_scope().unwrap().layer(&layer_id).unwrap();
        let target_capture = initialization_target(&snapshot);
        let target = decoded_capture(&target_capture.program);
        assert!(matches!(crate::product_store::scope::initialize_scalar_fallible(
            &decoded, decoded_layer, &target, &mut AllocationContext::new(),
        ), Err(crate::product_store::scope::InitializationFailure::Store(StoreError::Runtime(_)))));
        assert_eq!(crate::product_contract::canonical_bytes(&snapshot).unwrap(), before);
    }

    fn assert_correspondence_parity(historical: bool) {
        let (snapshot, target_capture, original, day) = correspondence_fixture(historical);
        let source_capture = &snapshot.programs[0];
        let initialized = snapshot.data.clone();
        let context = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap();
        let expected = super::super::correspondence::derive(
            &context, source_capture, &original, &target_capture, day, &initialized,
        ).unwrap();

        let decoded = decoded_snapshot(&snapshot);
        let source = decoded_captured(source_capture);
        let target = decoded_captured(&target_capture);
        let original_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let initialized_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&initialized).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let actual = super::super::derive_correspondence_fallible(
            &decoded, &[], &source, &original_decoded, &target, day, &initialized_decoded, &mut cx,
        ).unwrap();

        assert_eq!(
            crate::product_contract::canonical_bytes(&expected.proof).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(actual.proof(), &mut AllocationContext::new()).unwrap(),
        );
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected.seed).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(actual.seed(), &mut AllocationContext::new()).unwrap(),
        );
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected.operation_seed).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(actual.operation_seed(), &mut AllocationContext::new()).unwrap(),
        );
        assert_eq!(expected.frames.len(), actual.frames().len());
        for (expected_frame, actual_frame) in expected.frames.iter().zip(actual.frames()) {
            assert_eq!(expected_frame.layer, *actual_frame.layer());
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected_frame.before).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(
                    actual_frame.before(), &mut AllocationContext::new(),
                ).unwrap(),
            );
            assert_eq!(expected_frame.additions.len(), actual_frame.additions().len());
            for (expected_addition, actual_addition) in expected_frame.additions.iter().zip(actual_frame.additions()) {
                assert_eq!(
                    crate::product_contract::canonical_bytes(expected_addition).unwrap(),
                    crate::product_backup::fallible_codec::canonical_bytes(
                        actual_addition, &mut AllocationContext::new(),
                    ).unwrap(),
                );
            }
        }
        assert!(cx.growth_requests() > 0);
    }

    #[test]
    fn fallible_scalar_correspondence_matches_direct_and_historical_legacy_derivation() {
        assert_correspondence_parity(false);
        assert_correspondence_parity(true);
    }

    fn stored_correspondence_snapshot(
        historical: bool,
    ) -> (ProjectSnapshot, crate::product_contract::CapturedProgram, crate::product_contract::DataSnapshot) {
        let (mut snapshot, target, original, day) = correspondence_fixture(historical);
        let layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap();
        let (_, initialization) = super::super::history::initialize(&snapshot, layer, &target).unwrap();
        snapshot.scalar_scope_mut().unwrap().initializations.push(initialization);
        let source = snapshot.programs[0].clone();
        let context = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap();
        let mapped = crate::product_runtime::merged_data(&target, &original).unwrap();
        let (context, _) = context.project_seed(&source, &original, &target, &mapped, day).unwrap();
        let proofs = context.correspondence_proofs();
        let revision = snapshot.revision;
        for (id, proof) in proofs {
            snapshot.scalar_scope_mut().unwrap().correspondences.insert(id, super::super::ScopeCorrespondenceReceipt {
                proof,
                operation: "recorded-operation".into(),
                revision,
            });
        }
        (snapshot, target, original)
    }

    fn assert_scalar_replay_context_parity(historical: bool) {
        let (snapshot, target_capture, original_legacy) = stored_correspondence_snapshot(historical);
        let legacy_proofs: BTreeMap<_, _> = snapshot.scalar_scope().unwrap().correspondences.iter()
            .map(|(id, receipt)| (id.clone(), receipt.proof.clone())).collect();
        let registry = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap();
        let legacy = registry.with_correspondences(&legacy_proofs).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let source = decoded_captured(&snapshot.programs[0]);
        let target = decoded_captured(&target_capture);
        let original = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original_legacy).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let day = legacy_proofs.values().next().expect("fixture proof exists").day;
        let mapped_legacy = crate::product_runtime::merged_data(&target_capture, &original_legacy).unwrap();
        let legacy_projection_registry = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap();
        let (_, expected_projection) = legacy_projection_registry.project_seed(
            &snapshot.programs[0], &original_legacy, &target_capture, &mapped_legacy, day,
        ).unwrap();
        let mut decoded_registry_cx = AllocationContext::new();
        let decoded_registry = super::super::FallibleScalarReplayContext::registry(
            &decoded, &mut decoded_registry_cx,
        ).unwrap();
        let mapped_decoded = original.merge_app_with(target.app(), &mut decoded_registry_cx).unwrap();
        let (projected_context, actual_projection) = decoded_registry.project_seed(
            &source, &original, &target, &mapped_decoded, day, &mut decoded_registry_cx,
        ).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected_projection).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(
                &actual_projection, &mut AllocationContext::new(),
            ).unwrap(),
        );
        assert!(projected_context.verify_seed(
            &target, &actual_projection, day, &mut AllocationContext::new(),
        ).is_ok());
        if day > 1 {
            assert!(matches!(
                projected_context.verify_seed(
                    &target, &actual_projection, day - 1, &mut AllocationContext::new(),
                ),
                Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
            ));
        }
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap()
            .with_correspondences(&mut cx).unwrap();
        let actual = context.correspondence_proofs().collect::<Vec<_>>();
        let [(id, proof)] = actual.as_slice() else {
            panic!("expected one independently checked correspondence")
        };
        let expected = legacy.correspondence_proofs();
        let expected = expected.get(*id).expect("legacy context has the same proof key");
        let target_revision = expected.target.clone();
        assert_eq!(
            crate::product_contract::canonical_bytes(expected).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(*proof, &mut AllocationContext::new()).unwrap(),
        );
        assert!(context.projected_seed(proof.projected()));
        assert!(context.frames_for_source_seed(&target_revision, proof.projected()).is_some());
        assert!(target.validate_with(&mut AllocationContext::new()).is_ok());
    }

    #[test]
    fn fallible_scalar_replay_context_matches_direct_history_proof() {
        assert_scalar_replay_context_parity(false);
    }

    #[test]
    fn fallible_scalar_replay_context_rejects_uninitialized_target_basis() {
        let (snapshot, target_capture, original, day) = correspondence_fixture(false);
        let decoded = decoded_snapshot(&snapshot);
        let target = decoded_captured(&target_capture);
        let original = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(matches!(
            context.verify_seed(&target, &original, day, &mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap(), before);
    }

    #[test]
    fn fallible_scalar_replay_context_rejects_fabricated_live_metadata_and_event_history() {
        use crate::product_contract::{BusinessEvent, DataValue, RecordChange};

        let make_initialized = || {
            let (mut snapshot, target, _, day) = correspondence_fixture(false);
            let layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap();
            let (data, receipt) = super::super::history::initialize(&snapshot, layer, &target).unwrap();
            (snapshot, target, data, receipt, day)
        };

        // A receipt digest cannot authorize fabricated protected cells. Even
        // when its digest is changed to name the supplied data, the receipt's
        // additions must regenerate from the frozen basis.
        let (mut snapshot, target, mut data, mut receipt, day) = make_initialized();
        let layer_id = snapshot.scalar_scope().unwrap().layers.keys().next().unwrap().clone();
        let member = compiler::key(&layer_id, "item", "member");
        data.records[0].values.insert(member, DataValue::Boolean { value: false });
        receipt.after_data = data.identity().unwrap();
        snapshot.data = data;
        snapshot.scalar_scope_mut().unwrap().initializations.push(receipt);
        let decoded = decoded_snapshot(&snapshot);
        let target = decoded_captured(&target);
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(matches!(
            context.verify_seed(&target, decoded.data(), day, &mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));

        // A later event cannot rewrite membership while claiming an otherwise
        // valid retained producer and refreshed target data digest.
        let (mut snapshot, target, mut data, mut receipt, day) = make_initialized();
        let layer_id = snapshot.scalar_scope().unwrap().layers.keys().next().unwrap().clone();
        let member = compiler::key(&layer_id, "item", "member");
        let before = data.records[0].values.clone();
        let mut after = before.clone();
        after.insert(member, DataValue::Boolean { value: false });
        data.generation = data.generation.saturating_add(1);
        data.records[0].revision = data.records[0].revision.saturating_add(1);
        data.events.push(BusinessEvent {
            id: "fabricated-member-change".into(),
            sequence: data.generation,
            operation_id: "event-operation".into(),
            action: "noop".into(),
            day,
            program: target.artifact.program_digest.clone(),
            changes: vec![RecordChange {
                entity: "item".into(), record: "row-1".into(), before: Some(before),
                after, archived: false,
            }],
            outputs: vec![],
        });
        receipt.after_data = data.identity().unwrap();
        snapshot.data = data;
        snapshot.scalar_scope_mut().unwrap().initializations.push(receipt);
        let decoded = decoded_snapshot(&snapshot);
        let target = decoded_captured(&target);
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(matches!(
            context.verify_seed(&target, decoded.data(), day, &mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));
    }

    #[test]
    fn fallible_scalar_replay_context_requires_a_frame_for_every_target_layer() {
        let (snapshot, target_capture, original_legacy) = stored_correspondence_snapshot(false);
        let decoded = decoded_snapshot(&snapshot);
        let source = decoded_captured(&snapshot.programs[0]);
        let target = decoded_captured(&target_capture);
        let original = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original_legacy).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        let mapped = original.merge_app_with(target.app(), &mut cx).unwrap();
        let (mut context, projected) = context.project_seed(
            &source, &original, &target, &mapped, 3, &mut cx,
        ).unwrap();
        assert!(context.verify_seed(&target, &projected, 3, &mut cx).is_ok());
        context.strip_history_evidence_for_test();
        assert!(matches!(
            context.verify_seed(&target, &projected, 3, &mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));
    }

    #[test]
    fn fallible_scalar_replay_context_allows_arbitrary_seed_for_layerless_target() {
        let mut snapshot = composed_snapshot_with_view_projection_patch();
        let target = snapshot.programs.last().unwrap().clone();
        snapshot.scalar_scope_mut().unwrap().layers.clear();
        snapshot.scalar_scope_mut().unwrap().compositions.values_mut().next().unwrap().layers.clear();
        snapshot.scalar_scope_mut().unwrap().compositions.values_mut().next().unwrap().active.clear();
        let mut value: serde_json::Value = serde_json::from_slice(&serde_json::to_vec(&snapshot.data).unwrap()).unwrap();
        value["generation"] = serde_json::json!(17);
        let arbitrary = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&value).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let target = decoded_captured(&target);
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(context.verify_seed(&target, &arbitrary, snapshot.clock_day, &mut cx).is_ok());
    }

    #[test]
    fn fallible_scalar_replay_context_accepts_target_bound_evolution_rehearsal_seed() {
        let mut snapshot = initialization_snapshot();
        let layer = snapshot.scalar_scope().unwrap().layers.values().next().unwrap().clone();
        let source_revision = snapshot.active_revision.clone();
        let layer_id = layer.identity().unwrap();

        // Build and retain a real adoption target with its live initialization
        // receipt; the evolution rehearsal below inherits this exact layer
        // lineage and frozen initialized state.
        let adoption_manifest = super::super::CompositionManifest {
            version: super::super::COMPILER_VERSION,
            basis: layer.basis.clone(),
            transition: super::super::ScopeTransition::Adoption,
            rewrites: vec![],
            business: source_revision.clone(),
            layers: vec![layer_id.clone()],
            active: BTreeSet::from([layer_id.clone()]),
            previous: None,
            operation: "initial-adoption".into(),
            runtime: crate::product_runtime::RUNTIME_VERSION.into(),
            driver: crate::product_runtime::DRIVER_VERSION.into(),
            output: source_revision,
        };
        let adopted = compiler::compile(&snapshot, &adoption_manifest).unwrap();
        let adoption_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source, &adopted,
        ).unwrap();
        let mut adoption_manifest = adoption_manifest;
        adoption_manifest.output = adoption_revision.clone();
        let (initialized, receipt) = super::super::history::initialize(&snapshot, &layer, &adopted).unwrap();
        snapshot.programs.push(adopted.clone());
        snapshot.active_revision = adoption_revision.clone();
        snapshot.data = initialized;
        snapshot.session = crate::product_contract::SessionState::initial(&adopted.program).unwrap();
        let scope = snapshot.scalar_scope_mut().unwrap();
        scope.initializations.push(receipt);
        scope.compositions.insert(adoption_revision.clone(), adoption_manifest.clone());

        // An additive app evolution creates a distinct target program while
        // preserving the inherited protected layer and its verified history.
        let mut candidate_app = snapshot.programs[0].program.clone();
        candidate_app.entities[0].fields.push(FieldDefinition {
            id: "evolution_note".into(),
            label: "Evolution note".into(),
            value_type: Type::Optional { item: Box::new(Type::Text) },
        });
        candidate_app.entities.push(EntityDefinition {
            id: "audit".into(),
            label: "Audit".into(),
            fields: vec![FieldDefinition { id: "note".into(), label: "Note".into(), value_type: Type::Text }],
            unique: vec![],
            constraints: vec![],
        });
        let candidate = crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&candidate_app).unwrap(),
            "project",
            crate::product_contract::Producer::Fixture { name: "evolution-rehearsal-candidate".into() },
            None,
        ).unwrap();
        let candidate_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source, &candidate,
        ).unwrap();
        snapshot.programs.push(candidate.clone());

        let decisions = crate::product_contract::canonical_digest(
            IdentityDomain::Decision, &snapshot.decisions,
        ).unwrap();
        let evolution_basis = super::super::FrozenBasis {
            snapshot: crate::product_contract::canonical_digest(IdentityDomain::Data, &snapshot).unwrap(),
            revision: snapshot.revision,
            active: adoption_revision.clone(),
            data: snapshot.data.clone(),
            session: snapshot.session.clone(),
            day: snapshot.clock_day,
            decisions,
        };
        let mut evolution = adoption_manifest;
        evolution.basis = evolution_basis.clone();
        evolution.transition = super::super::ScopeTransition::Evolution;
        evolution.business = candidate_revision;
        let protected_destination = layer.patches[0].request.destination.clone();
        evolution.rewrites = vec![super::super::ScopeSlotMapping {
            layer: layer_id.clone(),
            patch: 0,
            from_source: adoption_revision.clone(),
            from: protected_destination.clone(),
            to_source: evolution.business.clone(),
            to: protected_destination.clone(),
            subject: "row".into(),
        }];
        evolution.previous = Some(adoption_revision.clone());
        evolution.operation = "retained-evolution".into();
        // As in prepare_evolution, compile against the predecessor output and
        // then bind the completed manifest to the resulting target revision.
        let evolved = compiler::compile(&snapshot, &evolution).unwrap();
        let evolved_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source, &evolved,
        ).unwrap();
        evolution.output = evolved_revision.clone();
        let seed = crate::product_runtime::merged_data(&evolved, &evolution_basis.data).unwrap();
        let seed_digest = seed.identity().unwrap();
        let compatibility = crate::product_runtime::LocalRuntime::default()
            .compatibility_at(&evolved, &seed, evolution_basis.day).unwrap();
        assert_eq!(compatibility.state, crate::product_contract::CompatibilityState::Compatible);
        let rehearsal = super::super::ManagedRehearsal {
            version: super::super::COMPILER_VERSION,
            manifest: evolution.clone(),
            seed: seed_digest.clone(),
            layer: None,
            initialization: None,
            compatibility,
            recorded_by: "record-evolution".into(),
            recorded_revision: snapshot.revision + 1,
            witnesses: BTreeMap::from([(
                "retained-choice".into(),
                crate::product_contract::canonical_digest(IdentityDomain::Decision, &"witness").unwrap(),
            )]),
        };
        snapshot.programs.push(evolved.clone());
        // A later Adoption inherits the same layer with a different app
        // schema. Arrange its output to sort first so the old "first
        // adoption containing this layer" receipt lookup would select the
        // wrong target.
        let mut other_candidate_app = snapshot.programs[0].program.clone();
        other_candidate_app.entities[0].fields.push(FieldDefinition {
            id: "other_evolution_note".into(),
            label: "Other evolution note".into(),
            value_type: Type::Optional { item: Box::new(Type::Text) },
        });
        let other_candidate = crate::product_contract::CapturedProgram::capture(
            &crate::product_contract::canonical_bytes(&other_candidate_app).unwrap(),
            "project",
            crate::product_contract::Producer::Fixture { name: "other-evolution-candidate".into() },
            None,
        ).unwrap();
        let other_candidate_revision = crate::product_contract::canonical_digest(
            IdentityDomain::Source, &other_candidate,
        ).unwrap();
        snapshot.programs.push(other_candidate);
        let mut other_evolution = evolution.clone();
        other_evolution.business = other_candidate_revision;
        other_evolution.transition = super::super::ScopeTransition::Adoption;
        other_evolution.previous = Some(adoption_revision.clone());
        other_evolution.rewrites[0].to_source = other_evolution.business.clone();
        let (other_evolved, other_revision) = (0..128)
            .find_map(|index| {
                other_evolution.operation = format!("later-adoption-{index}");
                let target = compiler::compile(&snapshot, &other_evolution).unwrap();
                let revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &target).unwrap();
                (revision < adoption_revision).then_some((target, revision))
            })
            .expect("bounded fixture search should find an output that sorts before its predecessor");
        other_evolution.output = other_revision.clone();
        snapshot.programs.push(other_evolved.clone());
        snapshot.scalar_scope_mut().unwrap().compositions.insert(other_revision, other_evolution);
        let first_layer_adoption = snapshot.scalar_scope().unwrap().compositions.values()
            .find(|manifest| manifest.transition == super::super::ScopeTransition::Adoption && manifest.layers.contains(&layer_id))
            .unwrap();
        assert_eq!(first_layer_adoption.output, crate::product_contract::canonical_digest(IdentityDomain::Source, &other_evolved).unwrap());
        snapshot.scalar_scope_mut().unwrap().rehearsals.insert(evolved_revision.clone(), rehearsal);

        let decoded = decoded_snapshot(&snapshot);
        let target = decoded_captured(&evolved);
        let decoded_seed = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&seed).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(context.projected_seed(&seed_digest));
        let evolution_manifest = context.snapshot().scalar_scope().unwrap().composition(&target.revision_with(&mut cx).unwrap()).unwrap();
        let rebuilt_evolution = compiler::compile_fallible(context.snapshot(), evolution_manifest, &mut cx).unwrap();
        assert_eq!(
            crate::product_backup::fallible_codec::canonical_bytes(&rebuilt_evolution, &mut AllocationContext::new()).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&target, &mut AllocationContext::new()).unwrap(),
        );
        let rebuilt_seed = evolution_manifest.basis().data().merge_app_with(target.app(), &mut cx).unwrap();
        assert_eq!(rebuilt_seed.identity_with(&mut cx).unwrap(), seed_digest);
        let seed_result = context.verify_seed(&target, &decoded_seed, evolution_basis.day, &mut cx);
        assert!(seed_result.is_ok(), "target-bound evolution rehearsal seed rejected: {seed_result:?}");
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap(), before);

        // A structurally decoded rehearsal cannot relabel altered unrelated
        // business data with its stored seed digest. The added audit entity is
        // outside the lifecycle layer; the per-layer checks alone leave it
        // untouched, so exact frozen-basis seed regeneration must reject it.
        let mut altered_seed = seed.clone();
        altered_seed.records.push(crate::product_contract::Record {
            entity: "audit".into(),
            id: "untracked-audit-row".into(),
            revision: 1,
            created_program: evolved.artifact.program_digest.clone(),
            archived: false,
            values: BTreeMap::from([("note".into(), crate::product_contract::DataValue::Text { value: "forged".into() })]),
        });
        altered_seed.validate().unwrap();
        let altered_digest = altered_seed.identity().unwrap();
        let mut forged_snapshot = snapshot.clone();
        forged_snapshot.scalar_scope_mut().unwrap().rehearsals
            .get_mut(&evolved_revision).unwrap().seed = altered_digest;
        let forged_decoded = decoded_snapshot(&forged_snapshot);
        let altered_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&altered_seed).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let mut forged_cx = AllocationContext::new();
        let forged_context = super::super::FallibleScalarReplayContext::registry(&forged_decoded, &mut forged_cx).unwrap();
        assert!(matches!(
            forged_context.verify_seed(&target, &altered_decoded, evolution_basis.day, &mut forged_cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));

        // The same data digest cannot borrow this rehearsal's authority under
        // the later Adoption output that shares the layer but has a different
        // app schema and frozen basis.
        let unrelated = decoded_captured(&other_evolved);
        let unrelated_result = context.verify_seed(&unrelated, &decoded_seed, evolution_basis.day, &mut cx);
        assert!(unrelated_result.is_err(), "unrelated retained output borrowed rehearsal authority: {unrelated_result:?}");

        // Exercise refusal inside target-bound rehearsal regeneration, after
        // the context itself has been constructed successfully.
        let mut count_cx = AllocationContext::new();
        let _count_context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut count_cx).unwrap();
        let registry_growths = count_cx.growth_requests();
        let mut verify_cx = AllocationContext::new();
        let verify_context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut verify_cx).unwrap();
        let verify_start = verify_cx.growth_requests();
        verify_context.verify_seed(&target, &decoded_seed, evolution_basis.day, &mut verify_cx).unwrap();
        let verify_growths = verify_cx.growth_requests() - verify_start;
        assert!(verify_growths > 0);
        for relative in [1, (verify_growths / 2).max(1), verify_growths] {
            let mut refused = AllocationContext::fail_nth_growth(
                std::num::NonZeroUsize::new(registry_growths + relative).unwrap(), true,
            );
            let retry_context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut refused).unwrap();
            assert!(matches!(
                retry_context.verify_seed(&target, &decoded_seed, evolution_basis.day, &mut refused),
                Err(super::super::ScalarReplayFailure::Allocation(_)),
            ), "target seed verification refusal at growth {relative}/{verify_growths}");
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
                &decoded, &mut AllocationContext::new(),
            ).unwrap(), before);
            let mut retry = AllocationContext::new();
            let retry_context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut retry).unwrap();
            retry_context.verify_seed(&target, &decoded_seed, evolution_basis.day, &mut retry).unwrap();
        }
    }

    #[test]
    fn fallible_scalar_replay_context_matches_scenario_projection_and_empty_registry() {
        use crate::product_contract::ScenarioSpec;

        let (empty_snapshot, _, _, _) = correspondence_fixture(false);
        let empty_decoded = decoded_snapshot(&empty_snapshot);
        let empty_before = crate::product_backup::fallible_codec::canonical_bytes(
            &empty_decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut empty_cx = AllocationContext::new();
        let empty = super::super::FallibleScalarReplayContext::registry(
            &empty_decoded, &mut empty_cx,
        ).unwrap().with_correspondences(&mut empty_cx).unwrap();
        assert_eq!(empty.correspondence_proofs().count(), 0);
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &empty_decoded, &mut AllocationContext::new(),
        ).unwrap(), empty_before);

        let (snapshot, target_capture, _) = stored_correspondence_snapshot(false);
        let legacy_proofs: BTreeMap<_, _> = snapshot.scalar_scope().unwrap().correspondences.iter()
            .map(|(id, receipt)| (id.clone(), receipt.proof.clone())).collect();
        let legacy = super::super::replay::ScopedExecutionContext::registry(&snapshot).unwrap()
            .with_correspondences(&legacy_proofs).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let source = &snapshot.programs[0];
        let decoded_source = decoded_captured(source);
        let target = decoded_captured(&target_capture);
        let original_scenario = ScenarioSpec {
            version: 1,
            id: "context-scenario".into(),
            label: "Replay context parity".into(),
            seed: snapshot.data.clone(),
            session: snapshot.session.clone(),
            clock_day: 3,
            random_seed: 19,
            inputs: vec![],
            validity: vec![],
        };
        let mut mapped_scenario = original_scenario.clone();
        mapped_scenario.seed = crate::product_runtime::merged_data(&target_capture, &original_scenario.seed).unwrap();
        let (expected_context, expected) = legacy.project_scenario(
            source, &original_scenario, &target_capture, &mapped_scenario,
        ).unwrap();
        assert!(expected_context.has_scenario_correspondence(&expected));

        let mut scenario_snapshot = snapshot.clone();
        let recorded_revision = scenario_snapshot.revision;
        for (id, proof) in expected_context.correspondence_proofs() {
            scenario_snapshot.scalar_scope_mut().unwrap().correspondences.insert(
                id.clone(), super::super::ScopeCorrespondenceReceipt {
                    proof: proof.clone(), operation: "scenario-replay".into(), revision: recorded_revision,
                },
            );
        }
        let decoded_scenario_snapshot = decoded_snapshot(&scenario_snapshot);
        let mut stored_cx = AllocationContext::new();
        let stored_context = super::super::FallibleScalarReplayContext::registry(
            &decoded_scenario_snapshot, &mut stored_cx,
        ).unwrap().with_correspondences(&mut stored_cx).unwrap();
        let expected_frame = crate::product_contract::decode_scenario_spec_fallible(
            syntax(&serde_json::to_vec(&expected).unwrap()), &mut stored_cx,
        ).unwrap();
        assert!(stored_context.has_scenario_correspondence(&expected_frame));

        let mut decode_cx = AllocationContext::new();
        let decoded_original = crate::product_contract::decode_scenario_spec_fallible(
            syntax(&serde_json::to_vec(&original_scenario).unwrap()), &mut decode_cx,
        ).unwrap();
        let decoded_mapped = crate::product_contract::decode_scenario_spec_fallible(
            syntax(&serde_json::to_vec(&mapped_scenario).unwrap()), &mut decode_cx,
        ).unwrap();
        let mut context_cx = AllocationContext::new();
        let decoded_context = super::super::FallibleScalarReplayContext::registry(
            &decoded, &mut context_cx,
        ).unwrap().with_correspondences(&mut context_cx).unwrap();
        let decoded_before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let context_proofs_before: Vec<_> = decoded_context.correspondence_proofs()
            .map(|(id, proof)| (id.clone(), crate::product_backup::fallible_codec::canonical_bytes(
                proof, &mut AllocationContext::new(),
            ).unwrap()))
            .collect();
        let scenario_before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded_original, &mut AllocationContext::new(),
        ).unwrap();
        let mut scenario_cx = AllocationContext::new();
        let (actual_context, actual) = decoded_context.project_scenario(
            &decoded_source, &decoded_original, &target, &decoded_mapped, &mut scenario_cx,
        ).unwrap();
        assert!(actual_context.has_scenario_correspondence(&actual));
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected).unwrap(),
            crate::product_backup::fallible_codec::canonical_bytes(&actual, &mut AllocationContext::new()).unwrap(),
        );
        let scenario_growths = scenario_cx.growth_requests();
        assert!(scenario_growths > 0);
        for nth in [1, scenario_growths / 2, scenario_growths] {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(
                decoded_context.project_scenario(
                    &decoded_source, &decoded_original, &target, &decoded_mapped, &mut refused,
                ),
                Err(super::super::ScalarReplayFailure::Allocation(_)),
            ), "scenario allocation refusal {nth}/{scenario_growths}");
            assert!(refused.first_failure().is_some());
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
                &decoded, &mut AllocationContext::new(),
            ).unwrap(), decoded_before);
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
                &decoded_original, &mut AllocationContext::new(),
            ).unwrap(), scenario_before);
            let context_proofs_after: Vec<_> = decoded_context.correspondence_proofs()
                .map(|(id, proof)| (id.clone(), crate::product_backup::fallible_codec::canonical_bytes(
                    proof, &mut AllocationContext::new(),
                ).unwrap()))
                .collect();
            assert_eq!(context_proofs_after, context_proofs_before);
            let mut retry_cx = AllocationContext::new();
            let (_, retried) = decoded_context.project_scenario(
                &decoded_source, &decoded_original, &target, &decoded_mapped, &mut retry_cx,
            ).unwrap();
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected).unwrap(),
                crate::product_backup::fallible_codec::canonical_bytes(&retried, &mut AllocationContext::new()).unwrap(),
            );
        }
    }

    #[test]
    fn fallible_scalar_replay_context_registry_and_proofs_refuse_atomically_and_retry() {
        use std::num::NonZeroUsize;
        let (snapshot, _, _) = stored_correspondence_snapshot(false);
        let decoded = decoded_snapshot(&snapshot);
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        let mut baseline = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut baseline).unwrap()
            .with_correspondences(&mut baseline).unwrap();
        assert_eq!(context.correspondence_proofs().count(), 1);
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in [1, growths / 2, growths] {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let result = match super::super::FallibleScalarReplayContext::registry(&decoded, &mut refused) {
                Ok(context) => context.with_correspondences(&mut refused),
                Err(error) => Err(error),
            };
            let Err(error) = result else { panic!("refusal {nth}/{growths} unexpectedly succeeded") };
            assert!(matches!(error, super::super::ScalarReplayFailure::Allocation(_)), "growth {nth}/{growths}: {error:?}");
            assert!(refused.first_failure().is_some());
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap(), before);
            let mut retry = AllocationContext::new();
            let retried = super::super::FallibleScalarReplayContext::registry(&decoded, &mut retry).unwrap()
                .with_correspondences(&mut retry).unwrap();
            assert_eq!(retried.correspondence_proofs().count(), 1);
        }
    }

    #[test]
    fn fallible_scalar_replay_context_rejects_cyclic_and_forged_correspondences() {
        use crate::product_contract::{DataValue, IdentityDomain};
        use super::super::ScopeCorrespondence;

        let (base, target, _, day) = correspondence_fixture(false);
        let source = base.programs[0].clone();
        let target_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &target).unwrap();
        let mut a = base.data.clone();
        a.records[0].values.insert("name".into(), DataValue::Text { value: "untrusted-a".into() });
        let mut b = base.data.clone();
        b.records[0].values.insert("name".into(), DataValue::Text { value: "untrusted-b".into() });
        let a_id = a.identity().unwrap();
        let b_id = b.identity().unwrap();
        let mut snapshot = base.clone();
        let revision = snapshot.revision;
        for (original, projected) in [(a, b_id), (b, a_id)] {
            let operation_seed = original.identity().unwrap();
            let proof = ScopeCorrespondence {
                source: source.clone(), original, target: target_revision.clone(), day,
                projected, operation_seed, scenario: None,
            };
            let id = proof.identity().unwrap();
            snapshot.scalar_scope_mut().unwrap().correspondences.insert(id, super::super::ScopeCorrespondenceReceipt {
                proof, operation: "cycle-probe".into(), revision,
            });
        }
        let decoded = decoded_snapshot(&snapshot);
        let cycle_before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(matches!(
            context.with_correspondences(&mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap(), cycle_before);

        let (mut snapshot, target, original) = stored_correspondence_snapshot(false);
        let scope = snapshot.scalar_scope_mut().unwrap();
        let id = scope.correspondences.keys().next().unwrap().clone();
        let mut receipt = scope.correspondences.remove(&id).unwrap();
        receipt.proof.projected = crate::product_contract::canonical_digest(
            IdentityDomain::Data, &"forged-projected-seed",
        ).unwrap();
        let forged_id = receipt.proof.identity().unwrap();
        scope.correspondences.insert(forged_id, receipt);
        let decoded = decoded_snapshot(&snapshot);
        let forged_before = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut cx = AllocationContext::new();
        let context = super::super::FallibleScalarReplayContext::registry(&decoded, &mut cx).unwrap();
        assert!(matches!(
            context.with_correspondences(&mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(_))),
        ));
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap(), forged_before);
        assert!(target.validate().is_ok());
        assert!(original.identity().is_ok());
    }

    #[test]
    fn fallible_scalar_replay_context_rejects_ambiguous_scenario_bindings() {
        use crate::product_contract::{DataValue, IdentityDomain, ScenarioSpec};

        let (snapshot, _, _) = stored_correspondence_snapshot(false);
        let verified_snapshot = decoded_snapshot(&snapshot);
        let mut source_cx = AllocationContext::new();
        let verified = super::super::FallibleScalarReplayContext::registry(&verified_snapshot, &mut source_cx).unwrap()
            .with_correspondences(&mut source_cx).unwrap();
        let template = verified.correspondence_proofs().next().unwrap().1;

        let scenario = ScenarioSpec {
            version: 1,
            id: "ambiguous-scenario".into(),
            label: "Ambiguous test binding".into(),
            seed: snapshot.data.clone(),
            session: snapshot.session.clone(),
            clock_day: 3,
            random_seed: 23,
            inputs: vec![],
            validity: vec![],
        };
        let mut cx = AllocationContext::new();
        let original = crate::product_contract::decode_scenario_spec_fallible(
            syntax(&serde_json::to_vec(&scenario).unwrap()), &mut cx,
        ).unwrap();
        let projected = original.try_clone_with(&mut cx).unwrap();

        let mut first_proof = template.copy_with(&mut cx).unwrap();
        let first_operation_seed = verified_snapshot.data().try_clone_with(&mut cx).unwrap();
        first_proof.bind_scenario(
            original.try_clone_with(&mut cx).unwrap(), projected.try_clone_with(&mut cx).unwrap(),
            first_operation_seed.identity_with(&mut cx).unwrap(),
        );
        let first = super::super::DecodedVerifiedCorrespondence::new_checked(
            first_proof, verified_snapshot.data().try_clone_with(&mut cx).unwrap(), vec![],
            first_operation_seed,
        );

        let mut altered = snapshot.data.clone();
        altered.records[0].values.insert("name".into(), DataValue::Text { value: "different-operation-frame".into() });
        let altered_bytes = crate::product_contract::canonical_bytes(&altered).unwrap();
        let mut second_operation_seed = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&altered_bytes), &mut cx,
        ).unwrap();
        let second_operation_identity = second_operation_seed.identity_with(&mut cx).unwrap();
        let mut second_proof = template.copy_with(&mut cx).unwrap();
        second_proof.bind_scenario(
            original.try_clone_with(&mut cx).unwrap(), projected.try_clone_with(&mut cx).unwrap(),
            second_operation_identity,
        );
        let second = super::super::DecodedVerifiedCorrespondence::new_checked(
            second_proof, verified_snapshot.data().try_clone_with(&mut cx).unwrap(), vec![],
            second_operation_seed,
        );

        let mut empty_snapshot = snapshot.clone();
        empty_snapshot.scalar_scope_mut().unwrap().correspondences.clear();
        let empty_decoded = decoded_snapshot(&empty_snapshot);
        let mut context = super::super::FallibleScalarReplayContext::registry(&empty_decoded, &mut cx).unwrap();
        context.insert_correspondence(first, &mut cx).unwrap();
        assert!(matches!(
            context.insert_correspondence(second, &mut cx),
            Err(super::super::ScalarReplayFailure::Store(StoreError::Invalid(message)))
                if message.contains("ambiguous historical correspondence")
        ));
        assert_eq!(template.target(), &crate::product_contract::canonical_digest(IdentityDomain::Source, &snapshot.programs[1]).unwrap());
    }

    #[test]
    fn fallible_scalar_correspondence_refusal_is_atomic_retryable_and_releases_partial_owners() {
        use std::num::NonZeroUsize;

        let (snapshot, target_capture, original, day) = correspondence_fixture(true);
        let source_capture = &snapshot.programs[0];
        let initialized = snapshot.data.clone();
        let decoded = decoded_snapshot(&snapshot);
        let source = decoded_captured(source_capture);
        let target = decoded_captured(&target_capture);
        let original_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let initialized_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&initialized).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let root_bytes = crate::product_contract::canonical_bytes(&snapshot).unwrap();
        let decoded_root_bytes = crate::product_backup::fallible_codec::canonical_bytes(
            &decoded, &mut AllocationContext::new(),
        ).unwrap();
        let mut baseline = AllocationContext::new();
        let expected = super::super::derive_correspondence_fallible(
            &decoded, &[], &source, &original_decoded, &target, day, &initialized_decoded, &mut baseline,
        ).unwrap();
        let expected_proof = crate::product_backup::fallible_codec::canonical_bytes(
            expected.proof(), &mut AllocationContext::new(),
        ).unwrap();
        let expected_seed = crate::product_backup::fallible_codec::canonical_bytes(
            expected.seed(), &mut AllocationContext::new(),
        ).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0);

        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            let error = super::super::derive_correspondence_fallible(
                &decoded, &[], &source, &original_decoded, &target, day, &initialized_decoded, &mut refused,
            ).unwrap_err();
            assert!(matches!(error, super::super::CorrespondenceFailure::Allocation(_)), "growth {nth}/{growths}: {error:?}");
            assert!(refused.first_failure().is_some());
            assert_eq!(crate::product_contract::canonical_bytes(&snapshot).unwrap(), root_bytes);
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
                &decoded, &mut AllocationContext::new(),
            ).unwrap(), decoded_root_bytes);
        }
        let mut retry = AllocationContext::new();
        let retried = super::super::derive_correspondence_fallible(
            &decoded, &[], &source, &original_decoded, &target, day, &initialized_decoded, &mut retry,
        ).unwrap();
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            retried.proof(), &mut AllocationContext::new(),
        ).unwrap(), expected_proof);
        assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(
            retried.seed(), &mut AllocationContext::new(),
        ).unwrap(), expected_seed);
    }

    #[test]
    fn fallible_scalar_correspondence_rejects_unsupported_day_and_forged_cohort() {
        let (snapshot, target_capture, original, _) = correspondence_fixture(false);
        let source = decoded_captured(&snapshot.programs[0]);
        let target = decoded_captured(&target_capture);
        let decoded = decoded_snapshot(&snapshot);
        let original_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&original).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let initialized_decoded = crate::product_contract::decode_data_snapshot_fallible(
            syntax(&serde_json::to_vec(&snapshot.data).unwrap()), &mut AllocationContext::new(),
        ).unwrap();
        let unsupported = super::super::derive_correspondence_fallible(
            &decoded, &[], &source, &original_decoded, &target, 4, &initialized_decoded, &mut AllocationContext::new(),
        ).unwrap_err();
        assert!(matches!(unsupported, super::super::CorrespondenceFailure::Store(error)
            if error.to_string().contains("historical input has no actual frozen-history correspondence")));

        let mut forged = snapshot.clone();
        let target_revision = crate::product_contract::canonical_digest(IdentityDomain::Source, &target_capture).unwrap();
        let scope = forged.scalar_scope_mut().unwrap();
        let manifest = scope.compositions.get_mut(&target_revision).unwrap();
        let old_layer_id = manifest.layers[0].clone();
        let mut layer = scope.layers.remove(&old_layer_id).unwrap();
        layer.basis.data.records.clear();
        let new_layer_id = layer.identity().unwrap();
        scope.layers.insert(new_layer_id.clone(), layer);
        manifest.layers[0] = new_layer_id.clone();
        manifest.active = BTreeSet::from([new_layer_id]);
        let forged_decoded = decoded_snapshot(&forged);
        let forged_error = super::super::derive_correspondence_fallible(
            &forged_decoded, &[], &source, &original_decoded, &target, 3, &initialized_decoded,
            &mut AllocationContext::new(),
        ).unwrap_err();
        assert!(matches!(forged_error, super::super::CorrespondenceFailure::Store(error)
            if error.to_string().contains("historical record is absent from the actual frozen cohort")));
    }

    #[test]
    fn decoded_composition_compiler_and_capture_match_legacy_with_refusal_retry() {
        let snapshot = composed_snapshot_with_observable_patch();
        let decoded = decoded_snapshot(&snapshot);
        let manifest = match &snapshot.scope {
            super::super::ScopeEnvelope::ScalarV1(state) => state.compositions.get(&snapshot.active_revision).unwrap(),
            _ => unreachable!(),
        };
        let expected = compile(&snapshot, manifest).unwrap();
        let decoded_manifest = decoded.scalar_scope().unwrap().composition(&snapshot.active_revision).unwrap();
        let mut baseline = AllocationContext::new();
        let actual = compile_fallible(&decoded, decoded_manifest, &mut baseline).unwrap();
        let expected_bytes = crate::product_contract::canonical_bytes(&expected).unwrap();
        let actual_bytes = crate::product_backup::fallible_codec::canonical_bytes(&actual, &mut AllocationContext::new()).unwrap();
        assert_eq!(actual_bytes, expected_bytes);
        assert_eq!(
            actual.revision_with(&mut AllocationContext::new()).unwrap(),
            crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Source, &expected).unwrap(),
        );
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < crate::product_contract::MAX_COLLECTION, "compiler growth count: {growths}");
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(compile_fallible(&decoded, decoded_manifest, &mut refused), Err(CompilerFailure::Allocation(_))), "expected allocation refusal at growth {nth}");
            let mut retry = AllocationContext::new();
            let retried = compile_fallible(&decoded, decoded_manifest, &mut retry).unwrap();
            assert_eq!(
                crate::product_backup::fallible_codec::canonical_bytes(&retried, &mut AllocationContext::new()).unwrap(),
                expected_bytes,
            );
            assert_eq!(
                crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap(),
                before,
            );
        }
    }

    #[test]
    fn decoded_composition_compiler_matches_lifecycle_sealing_passes() {
        let mut snapshot = composed_snapshot_with_observable_patch();
        let layer_id = match &snapshot.scope {
            super::super::ScopeEnvelope::ScalarV1(state) => state.compositions.get(&snapshot.active_revision).unwrap().layers[0].clone(),
            _ => unreachable!(),
        };
        let active_revision = snapshot.active_revision.clone();
        {
            let super::super::ScopeEnvelope::ScalarV1(state) = &mut snapshot.scope else { unreachable!() };
            state.layers.get_mut(&layer_id).unwrap().request.lifecycles.push(super::super::LifecycleBinding {
                entity: "item".into(),
                completed: Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value: true } },
                source: canonical_digest(IdentityDomain::Adoption, &"lifecycle-test").unwrap(),
            });
        }
        let super::super::ScopeEnvelope::ScalarV1(state) = &snapshot.scope else { unreachable!() };
        let manifest = state.compositions.get(&active_revision).unwrap();
        let expected = compile(&snapshot, manifest).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let decoded_manifest = decoded.scalar_scope().unwrap().composition(&snapshot.active_revision).unwrap();
        let mut baseline = AllocationContext::new();
        let actual = compile_fallible(&decoded, decoded_manifest, &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        assert_eq!(
            crate::product_backup::fallible_codec::canonical_bytes(&actual, &mut AllocationContext::new()).unwrap(),
            crate::product_contract::canonical_bytes(&expected).unwrap(),
        );
        assert!(growths > 0 && growths < crate::product_contract::MAX_COLLECTION);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(compile_fallible(&decoded, decoded_manifest, &mut refused), Err(CompilerFailure::Allocation(_))), "lifecycle allocation refusal {nth}/{growths}");
            let mut retry = AllocationContext::new();
            let retried = compile_fallible(&decoded, decoded_manifest, &mut retry).unwrap();
            assert_eq!(
                crate::product_backup::fallible_codec::canonical_bytes(&retried, &mut AllocationContext::new()).unwrap(),
                crate::product_contract::canonical_bytes(&expected).unwrap(),
            );
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap(), before);
        }
    }

    #[test]
    fn decoded_composition_compiler_matches_projection_provenance() {
        let snapshot = composed_snapshot_with_view_projection_patch();
        let manifest = match &snapshot.scope {
            super::super::ScopeEnvelope::ScalarV1(state) => state.compositions.get(&snapshot.active_revision).unwrap(),
            _ => unreachable!(),
        };
        let expected = compile(&snapshot, manifest).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let manifest = decoded.scalar_scope().unwrap().composition(&snapshot.active_revision).unwrap();
        let actual = compile_fallible(&decoded, manifest, &mut AllocationContext::new()).unwrap();
        assert_eq!(
            crate::product_backup::fallible_codec::canonical_bytes(&actual, &mut AllocationContext::new()).unwrap(),
            crate::product_contract::canonical_bytes(&expected).unwrap(),
        );
    }

    #[test]
    fn decoded_envelope_app_matches_pre_instrumentation_legacy_and_refuses_safely() {
        use std::num::NonZeroUsize;

        let snapshot = composed_snapshot_with_view_projection_patch();
        let state = snapshot.scalar_scope().unwrap();
        let manifest = state.compositions.get(&snapshot.active_revision).unwrap();
        let expected = envelope_app(&snapshot, manifest).unwrap();
        let decoded = decoded_snapshot(&snapshot);
        let decoded_manifest = decoded.scalar_scope().unwrap().composition(&snapshot.active_revision).unwrap();
        let mut baseline = AllocationContext::new();
        let actual = envelope_app_fallible(&decoded, decoded_manifest, &mut baseline).unwrap();
        let expected_bytes = crate::product_contract::canonical_bytes(&expected).unwrap();
        let actual_bytes = crate::product_backup::fallible_codec::canonical_bytes(&actual, &mut AllocationContext::new()).unwrap();
        assert_eq!(actual_bytes, expected_bytes);

        // Instrumentation and provenance belong only to the captured program,
        // not the envelope queried by historical projection destinations.
        let compiled = compile_fallible(&decoded, decoded_manifest, &mut AllocationContext::new()).unwrap();
        assert_ne!(
            crate::product_backup::fallible_codec::canonical_bytes(compiled.app(), &mut AllocationContext::new()).unwrap(),
            actual_bytes,
        );

        let growths = baseline.growth_requests();
        assert!(growths > 0);
        let before = crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap();
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), false);
            assert!(matches!(envelope_app_fallible(&decoded, decoded_manifest, &mut refused), Err(CompilerFailure::Allocation(_))), "envelope allocation refusal {nth}/{growths}");
            let mut retry = AllocationContext::new();
            let retried = envelope_app_fallible(&decoded, decoded_manifest, &mut retry).unwrap();
            assert_eq!(
                crate::product_backup::fallible_codec::canonical_bytes(&retried, &mut AllocationContext::new()).unwrap(),
                expected_bytes,
            );
            assert_eq!(crate::product_backup::fallible_codec::canonical_bytes(&decoded, &mut AllocationContext::new()).unwrap(), before);
        }
    }

    #[test]
    fn decoded_mapping_validation_matches_legacy_and_refusal_retries() {
        let snapshot = composed_snapshot_with_observable_patch();
        let decoded = decoded_snapshot(&snapshot);
        let scalar = match &snapshot.scope { super::super::ScopeEnvelope::ScalarV1(state) => state, _ => unreachable!() };
        let previous = scalar.compositions.get(&snapshot.active_revision).unwrap();
        let candidate = captured_program_with_observable("candidate");
        let candidate_revision = crate::product_contract::canonical_digest(crate::product_contract::IdentityDomain::Source, &candidate).unwrap();
        let layer = previous.layers.first().unwrap();
        let destination = EffectDestination::Observable { observable: "status".into() };
        let mapping = super::super::ScopeSlotMapping {
            layer: layer.clone(),
            patch: 0,
            from_source: previous.output.clone(),
            from: destination.clone(),
            to_source: candidate_revision,
            to: destination,
            subject: "record".into(),
        };
        validate_mappings(&snapshot, previous, &candidate, std::slice::from_ref(&mapping)).unwrap();
        let previous_dto = decoded.scalar_scope().unwrap().composition(&snapshot.active_revision).unwrap();
        let candidate_bytes = serde_json::to_vec(&candidate).unwrap();
        let candidate_dto = crate::product_contract::decode_captured_program_fallible(
            syntax(&candidate_bytes), &mut AllocationContext::new(),
        ).unwrap();
        let mapping_bytes = serde_json::to_vec(&mapping).unwrap();
        let mapping_dto = super::super::decode_scope_slot_mapping_fallible(
            syntax(&mapping_bytes), &mut AllocationContext::new(),
        ).unwrap();
        let mut baseline = AllocationContext::new();
        validate_mappings_fallible(&decoded, previous_dto, &candidate_dto, std::slice::from_ref(&mapping_dto), &mut baseline).unwrap();
        let growths = baseline.growth_requests();
        assert!(growths > 0 && growths < crate::product_contract::MAX_COLLECTION);
        for nth in 1..=growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(validate_mappings_fallible(&decoded, previous_dto, &candidate_dto, std::slice::from_ref(&mapping_dto), &mut refused), Err(CompilerFailure::Allocation(_))));
            let mut retry = AllocationContext::new();
            validate_mappings_fallible(&decoded, previous_dto, &candidate_dto, std::slice::from_ref(&mapping_dto), &mut retry).unwrap();
        }
        let duplicate = [mapping.clone(), mapping.clone()];
        let legacy_error = validate_mappings(&snapshot, previous, &candidate, &duplicate).unwrap_err();
        let duplicate_dtos = duplicate.iter().map(|mapping| {
            let bytes = serde_json::to_vec(mapping).unwrap();
            super::super::decode_scope_slot_mapping_fallible(syntax(&bytes), &mut AllocationContext::new()).unwrap()
        }).collect::<Vec<_>>();
        let decoded_error = validate_mappings_fallible(&decoded, previous_dto, &candidate_dto, &duplicate_dtos, &mut AllocationContext::new()).unwrap_err();
        assert!(matches!(decoded_error, CompilerFailure::Store(error) if error.to_string() == legacy_error.to_string()));

        let mut stale_source = mapping.clone();
        stale_source.from_source = crate::product_contract::Digest::try_from("a5".repeat(32)).unwrap();
        let stale_error = validate_mappings(&snapshot, previous, &candidate, std::slice::from_ref(&stale_source)).unwrap_err();
        let stale_bytes = serde_json::to_vec(&stale_source).unwrap();
        let stale_dto = super::super::decode_scope_slot_mapping_fallible(
            syntax(&stale_bytes), &mut AllocationContext::new(),
        ).unwrap();
        let mut stale_refusal = AllocationContext::fail_nth_growth(NonZeroUsize::new(1).unwrap(), true);
        let stale_decoded_error = validate_mappings_fallible(
            &decoded, previous_dto, &candidate_dto, std::slice::from_ref(&stale_dto), &mut stale_refusal,
        ).unwrap_err();
        assert!(matches!(stale_decoded_error, CompilerFailure::Store(error) if error.to_string() == stale_error.to_string()));
        assert_eq!(stale_refusal.growth_requests(), 0, "stale source is rejected before candidate revision work");

        let mut mapped_snapshot = snapshot.clone();
        mapped_snapshot.programs.push(candidate.clone());
        if let super::super::ScopeEnvelope::ScalarV1(state) = &mut mapped_snapshot.scope {
            state.compositions.get_mut(&mapped_snapshot.active_revision).unwrap().rewrites.push(mapping.clone());
        }
        let mapped_decoded = decoded_snapshot(&mapped_snapshot);
        let mapped_manifest = match &mapped_snapshot.scope {
            super::super::ScopeEnvelope::ScalarV1(state) => state.compositions.get(&mapped_snapshot.active_revision).unwrap(),
            _ => unreachable!(),
        };
        let mapped_manifest_dto = mapped_decoded.scalar_scope().unwrap().composition(&mapped_snapshot.active_revision).unwrap();
        let expected_patch = effective_patch(&mapped_snapshot, mapped_manifest, &layer, 0).unwrap();
        let mut patch_baseline = AllocationContext::new();
        let actual_patch = effective_patch_fallible(&mapped_decoded, mapped_manifest_dto, &layer, 0, &mut patch_baseline).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected_patch).unwrap(),
            crate::product_contract::canonical_bytes(&actual_patch).unwrap(),
        );
        let patch_growths = patch_baseline.growth_requests();
        assert!(patch_growths > 0 && patch_growths < crate::product_contract::MAX_COLLECTION);
        for nth in 1..=patch_growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(
                effective_patch_fallible(&mapped_decoded, mapped_manifest_dto, &layer, 0, &mut refused),
                Err(CompilerFailure::Allocation(_))
            ));
            let mut retry = AllocationContext::new();
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected_patch).unwrap(),
                crate::product_contract::canonical_bytes(
                    &effective_patch_fallible(&mapped_decoded, mapped_manifest_dto, &layer, 0, &mut retry).unwrap()
                ).unwrap(),
            );
        }
        let expected_business = business_program(&mapped_snapshot).unwrap();
        let mut business_cx = AllocationContext::new();
        let actual_business = business_program_fallible(&mapped_decoded, &mut business_cx).unwrap();
        assert_eq!(
            crate::product_contract::canonical_bytes(&expected_business).unwrap(),
            crate::product_contract::canonical_bytes(&actual_business).unwrap(),
        );
        let business_growths = business_cx.growth_requests();
        assert!(business_growths > 0 && business_growths < crate::product_contract::MAX_COLLECTION);
        for nth in 1..=business_growths {
            let mut refused = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(
                business_program_fallible(&mapped_decoded, &mut refused),
                Err(CompilerFailure::Allocation(_))
            ));
            let mut retry = AllocationContext::new();
            assert_eq!(
                crate::product_contract::canonical_bytes(&expected_business).unwrap(),
                crate::product_contract::canonical_bytes(&business_program_fallible(&mapped_decoded, &mut retry).unwrap()).unwrap(),
            );
        }
    }
}
