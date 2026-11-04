//! Allocation-aware expression semantics for decoded common-fact rules.
//!
//! This is kept separate from the legacy verifier until the decoded verifier
//! is assembled. It consumes decoded app/schema owners and never converts
//! them back to legacy maps.
use super::common_fact_admission::{OwnedProjectionDestination, OwnedProjectionRecipe, ProofMeter};
use super::fallible_common_fact_inventory::{CommonFactInventoryFailure, DecodedCommonFactInventory};
use super::fallible_common_fact_schemas::{DecodedOwnedSchemaKind, DecodedOwnedSchemaKinds};
use super::owned_contract::{OwnedSourceSide, OwnedStatementKind};
use super::fallible_owned_source_decode::{DecodedOwnedRootBinding, DecodedOwnedSpanEndpoint, DecodedOwnedStructuralReport, DecodedOwnedWriter};
use crate::product_alloc::{self, ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationSite};
use crate::product_contract::{self, AppRef, DataValue, Digest, Expr, IdentityDomain, StringValueMap, Type, MAX_COLLECTION, MAX_TEXT_BYTES};
use crate::product_contract::fallible_app_decode::{DecodedActionDefinition, DecodedStatement, FallibleMap};
use crate::product_store::StoreError;
use serde::Serialize;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DecodedCommonFactCertificate {
    pub(crate) side: OwnedSourceSide,
    pub(crate) site: String,
    pub(crate) rule: String,
    pub(crate) identity: Digest,
}
impl crate::product_backup::fallible_codec::BackupWire for (&DecodedOwnedRootBinding, &String) {}
impl crate::product_backup::fallible_codec::BackupWire for (&String, &Expr, &Option<String>) {}
impl crate::product_backup::fallible_codec::BackupWire for crate::product_contract::fallible_app_decode::DecodedViewDefinition {}

pub(crate) struct DecodedRuleValue {
    pub(crate) ty: Type,
    pub(crate) root: Option<String>,
    pub(crate) active_root: bool,
    pub(crate) policy: OrderedSet<String>,
    pub(crate) facets: OrderedSet<String>,
    pub(crate) private_identity: bool,
    pub(crate) frame_dependent: bool,
    pub(crate) session_dependent: bool,
}
impl DecodedRuleValue {
    pub(crate) fn typed(ty: Type) -> Self { Self::plain(ty) }
    fn plain(ty: Type) -> Self {
        Self { ty, root: None, active_root: false, policy: OrderedSet::new(), facets: OrderedSet::new(), private_identity: false, frame_dependent: false, session_dependent: false }
    }
    fn try_clone_with(&self, cx: &mut AllocationContext) -> Result<Self, CommonFactInventoryFailure> {
        let mut policy = OrderedSet::new();
        for value in self.policy.iter() { policy.try_insert(copy_string(value, cx)?, cx)?; }
        let mut facets = OrderedSet::new();
        for value in self.facets.iter() { facets.try_insert(copy_string(value, cx)?, cx)?; }
        Ok(Self { ty: copy_type(&self.ty, cx)?, root: self.root.as_deref().map(|v| copy_string(v, cx)).transpose()?, active_root: self.active_root, policy, facets, private_identity: self.private_identity, frame_dependent: self.frame_dependent, session_dependent: self.session_dependent })
    }
    fn merge(&mut self, other: &Self, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        for value in other.policy.iter() { self.policy.try_insert(copy_string(value, cx)?, cx)?; }
        for value in other.facets.iter() { self.facets.try_insert(copy_string(value, cx)?, cx)?; }
        self.private_identity |= other.private_identity;
        self.frame_dependent |= other.frame_dependent;
        self.session_dependent |= other.session_dependent;
        Ok(())
    }
    fn item(&self, cx: &mut AllocationContext) -> Result<Self, CommonFactInventoryFailure> {
        let Type::List { item } = &self.ty else { return Err(refuse(cx, "expression", "expected list")); };
        let mut value = self.try_clone_with(cx)?;
        value.ty = copy_type(item, cx)?;
        value.policy = OrderedSet::new();
        value.facets = OrderedSet::new();
        Ok(value)
    }
}

pub(crate) struct DecodedRuleEnvironment(OrderedMap<String, DecodedRuleValue>);
impl DecodedRuleEnvironment {
    pub(crate) fn new() -> Self { Self(OrderedMap::new()) }
    fn get(&self, name: &str) -> Option<&DecodedRuleValue> { self.0.get(name) }
    pub(crate) fn insert(&mut self, name: &str, value: DecodedRuleValue, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        self.0.try_insert(copy_string(name, cx)?, value, cx)?;
        Ok(())
    }
    fn try_clone_with(&self, cx: &mut AllocationContext) -> Result<Self, CommonFactInventoryFailure> {
        let mut result = Self::new();
        for (name, value) in self.0.iter() { result.insert(name, value.try_clone_with(cx)?, cx)?; }
        Ok(result)
    }
}

pub(crate) struct TypeIter<'a>(std::slice::Iter<'a, (String, DecodedRuleValue)>);
impl<'a> Iterator for TypeIter<'a> {
    type Item = (&'a String, &'a Type);
    fn next(&mut self) -> Option<Self::Item> { self.0.next().map(|(key, value)| (key, &value.ty)) }
}
impl StringValueMap<Type> for DecodedRuleEnvironment {
    type Iter<'a> = TypeIter<'a> where Self: 'a;
    fn len(&self) -> usize { self.0.len() }
    fn get(&self, key: &str) -> Option<&Type> { self.0.get(key).map(|value| &value.ty) }
    fn contains_key(&self, key: &str) -> bool { self.0.contains_key(key) }
    fn iter(&self) -> Self::Iter<'_> { TypeIter(self.0.iter_entries()) }
}

pub(crate) struct DecodedCommonFactRules<'a> {
    app: AppRef<'a>,
    kinds: &'a DecodedOwnedSchemaKinds,
    root_entity: &'a str,
    lifecycle_field: &'a str,
    lifecycle_value: DataValue,
    side: OwnedSourceSide,
    policy: &'a OrderedSet<(String, String)>,
    writers: &'a [DecodedOwnedWriter],
    structure: Option<&'a DecodedOwnedStructuralReport>,
    all_roots_active: bool,
    meter: &'a mut ProofMeter,
    certificates: Vec<DecodedCommonFactCertificate>,
    discovered: OrderedSet<(String, String)>,
}
impl<'a> DecodedCommonFactRules<'a> {
    pub(crate) fn new(
        app: AppRef<'a>, kinds: &'a DecodedOwnedSchemaKinds, root_entity: &'a str,
        lifecycle_field: &'a str, lifecycle_value: DataValue,
        side: OwnedSourceSide, policy: &'a OrderedSet<(String, String)>, writers: &'a [DecodedOwnedWriter],
        all_roots_active: bool, meter: &'a mut ProofMeter,
    ) -> Self {
        Self { app, kinds, root_entity, lifecycle_field, lifecycle_value,
            side, policy, writers, structure: None, all_roots_active, meter, certificates: Vec::new(), discovered: OrderedSet::new() }
    }
    pub(crate) fn with_structure(mut self, structure: &'a DecodedOwnedStructuralReport) -> Self { self.structure = Some(structure); self }
    pub(crate) fn from_inventory(
        app: AppRef<'a>, kinds: &'a DecodedOwnedSchemaKinds, inventory: &'a DecodedCommonFactInventory,
        lifecycle_field: &'a str, lifecycle_value: DataValue, side: OwnedSourceSide,
        policy: &'a OrderedSet<(String, String)>, all_roots_active: bool, meter: &'a mut ProofMeter,
    ) -> Self {
        Self::new(app, kinds, &inventory.structure.root_entity, lifecycle_field, lifecycle_value, side,
            policy, &inventory.structure.writers, all_roots_active, meter).with_structure(&inventory.structure)
    }
    pub(crate) fn certificates(&self) -> &[DecodedCommonFactCertificate] { &self.certificates }
    pub(crate) fn discovered(&self) -> &OrderedSet<(String, String)> { &self.discovered }
    pub(crate) fn take_outputs(self) -> (Vec<DecodedCommonFactCertificate>, OrderedSet<(String, String)>) {
        (self.certificates, self.discovered)
    }
    pub(crate) fn environment(&self, parameters: &impl StringValueMap<Type>, prefix: &str, cx: &mut AllocationContext) -> Result<DecodedRuleEnvironment, CommonFactInventoryFailure> {
        let mut environment = DecodedRuleEnvironment::new();
        let mut previous: Option<&str> = None;
        while let Some((name, ty)) = next_string_value(parameters, previous) {
            let mut value = DecodedRuleValue::plain(copy_type(ty, cx)?);
            if let Type::Reference { entity } = ty {
                match self.kinds.get(entity) {
                    Some(DecodedOwnedSchemaKind::Root) => value.root = Some(format_owned(cx, format_args!("{prefix}/{name}"))?),
                    Some(DecodedOwnedSchemaKind::Link { owner }) => value.root = Some(format_owned(cx, format_args!("{prefix}/{name}.{owner}"))?),
                    Some(DecodedOwnedSchemaKind::Registry { .. }) => (),
                    _ => return Err(refuse(cx, prefix, "direct reference to private/terminal relation is unsupported")),
                }
            }
            if value.root.is_some() { value.active_root = self.all_roots_active; }
            environment.insert(name, value, cx)?;
            previous = Some(name);
        }
        Ok(environment)
    }
    fn ty(&self, expr: &Expr, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> {
        match self.app {
            AppRef::Decoded(app) => product_contract::decoded_expression_type_in_with(app, expr, env, cx).map_err(app_error),
            _ => Err(refuse(cx, "expression", "decoded app owner required")),
        }
    }
    fn receiver(&self, expr: &Expr, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<Option<String>, CommonFactInventoryFailure> {
        match expr {
            Expr::Variable { name } => env.get(name).and_then(|value| value.root.as_deref()).map(|root| copy_string(root, cx)).transpose(),
            Expr::Field { record, field } => {
                let Type::Reference { entity } = self.ty(record, env, cx)? else { return Ok(None); };
                if self.kinds.get(&entity).and_then(DecodedOwnedSchemaKind::owner) == Some(field) { self.receiver(record, env, cx) } else { Ok(None) }
            }
            _ => Ok(None),
        }
    }
    fn facet(&self, expr: &Expr, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<Option<String>, CommonFactInventoryFailure> {
        let Expr::Equal { left, right } = expr else { return Ok(None); };
        if literal(right) != Some(&self.lifecycle_value) { return Ok(None); }
        let Expr::Field { record, field } = left.as_ref() else { return Ok(None); };
        if field != self.lifecycle_field || self.ty(record, env, cx)? != reference_type(self.root_entity, cx)? { return Ok(None); }
        self.receiver(record, env, cx)
    }
    fn correlation(&self, predicate: &Expr, entity: &str, binding: &str, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<Option<String>, CommonFactInventoryFailure> {
        match predicate {
            Expr::And { values } => { for value in values { if let Some(root) = self.correlation(value, entity, binding, env, cx)? { return Ok(Some(root)); } } return Ok(None); }
            Expr::Equal { left, right } => {
                for (a, b) in [(left.as_ref(), right.as_ref()), (right.as_ref(), left.as_ref())] {
                    let matches = if entity == self.root_entity { variable(a, binding) } else {
                        matches!(a, Expr::Field { record, field } if variable(record, binding) && self.kinds.get(entity).and_then(DecodedOwnedSchemaKind::owner) == Some(field))
                    };
                    if matches && self.ty(b, env, cx)? == reference_type(self.root_entity, cx)? {
                        if let Some(root) = self.receiver(b, env, cx)? { return Ok(Some(root)); }
                    }
                }
            }
            _ => (),
        }
        Ok(None)
    }
    fn certify<T: Serialize + crate::product_backup::fallible_codec::BackupWire>(&mut self, site: &str, rule: &str, value: &T, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        self.meter.charge(1)?;
        let identity = crate::product_backup::fallible_codec::canonical_digest(IdentityDomain::Evidence, value, cx).map_err(codec_error)?;
        product_alloc::try_push(&mut self.certificates, DecodedCommonFactCertificate { side: self.side, site: copy_string(site, cx)?, rule: copy_string(rule, cx)?, identity }, cx, AllocationSite::Vector)?;
        Ok(())
    }
    pub(crate) fn expression(&mut self, expr: &Expr, env: &DecodedRuleEnvironment, facets: bool, display: bool, at: &str, cx: &mut AllocationContext) -> Result<DecodedRuleValue, CommonFactInventoryFailure> {
        self.meter.charge(1)?;
        let ty = self.ty(expr, env, cx)?;
        let mut out = DecodedRuleValue::plain(ty);
        if facets {
            if let Some(root) = self.facet(expr, env, cx)? {
                out.facets.try_insert(root, cx)?;
                self.certify(at, "exact accepted lifecycle facet occurrence", expr, cx)?;
                return Ok(out);
            }
        }
        match expr {
            Expr::Literal { value_type, .. } => if relation(value_type).is_some() { return Err(refuse(cx, at, "literal references have no authenticated provenance")); },
            Expr::Variable { name } => return env.get(name).ok_or_else(|| refuse(cx, at, "missing typed binding"))?.try_clone_with(cx),
            Expr::Today => out.frame_dependent = true,
            Expr::State { .. } => { out.frame_dependent = true; out.session_dependent = true; }
            Expr::Field { record, field } => {
                let receiver = self.expression(record, env, facets, display, &format_owned(cx, format_args!("{at}.record"))?, cx)?;
                let Type::Reference { entity } = &receiver.ty else { return Err(refuse(cx, at, "unproved field reference")); };
                out.merge(&receiver, cx)?;
                let kind = self.kinds.get(entity).ok_or_else(|| refuse(cx, at, "unknown entity"))?;
                if self.policy.iter().any(|(e, f)| e == entity && f == field) || (matches!(kind, DecodedOwnedSchemaKind::Aux { .. }) && kind.owner() != Some(field)) {
                    let taint = match receiver.root.as_deref() { Some(root) => copy_string(root, cx)?, None => copy_string("global", cx)? };
                    out.policy.try_insert(taint, cx)?;
                }
                if matches!(out.ty, Type::Reference { .. }) && kind.owner() == Some(field) {
                    out.root = receiver.root.as_deref().map(|v| copy_string(v, cx)).transpose()?;
                    out.active_root = receiver.active_root;
                }
                if matches!(kind, DecodedOwnedSchemaKind::History { .. }) && !display { return Err(refuse(cx, at, "terminal history field has a business consumer")); }
            }
            Expr::Query { entity, binding, predicate, sort, limit, include_archived } => {
                if matches!(self.kinds.get(entity), Some(DecodedOwnedSchemaKind::History { .. })) { return Err(refuse(cx, at, "terminal history query is not an exact bookkeeping or inert history surface")); }
                let correlated = self.correlation(predicate, entity, binding, env, cx)?;
                let root = if let Some(root) = correlated { Some(root) }
                    else if entity == self.root_entity { Some(format_owned(cx, format_args!("{at}/{binding}"))?) }
                    else { None };
                let mut local = env.try_clone_with(cx)?;
                let mut row = DecodedRuleValue::plain(reference_type(entity, cx)?);
                row.root = root.as_deref().map(|v| copy_string(v, cx)).transpose()?;
                row.active_root = self.all_roots_active || (entity == self.root_entity && !include_archived) || env.0.iter().any(|(_, value)| value.active_root && value.root.is_some() && value.root == root);
                out.active_root = row.active_root;
                local.insert(binding, row, cx)?;
                let p = self.expression(predicate, &local, facets, display, &format_owned(cx, format_args!("{at}.predicate"))?, cx)?;
                out.merge(&p, cx)?;
                out.root = root.as_deref().map(|v| copy_string(v, cx)).transpose()?;
                if self.kinds.get(entity).is_some_and(|kind| matches!(kind, DecodedOwnedSchemaKind::Aux { .. })) {
                    out.policy.try_insert(root.as_deref().map(|v| copy_string(v, cx)).transpose()?.ok_or_else(|| refuse(cx, at, "auxiliary membership lacks exact owner correlation"))?, cx)?;
                }
                if !out.policy.is_empty() && (!out.policy.iter().all(|r| Some(r.as_str()) == root.as_deref()) || *limit != MAX_COLLECTION) { return Err(refuse(cx, at, "policy selection is cross-root or truncated")); }
                for key in sort {
                    let value = self.expression(&key.value, &local, false, display, &format_owned(cx, format_args!("{at}.sort"))?, cx)?;
                    if !value.policy.is_empty() || !out.policy.is_empty() { return Err(refuse(cx, at, "policy-dependent query order is unsupported")); }
                    out.merge(&value, cx)?;
                }
            }
            Expr::All { items, binding, predicate } | Expr::Any { items, binding, predicate } | Expr::Filter { items, binding, predicate } => {
                let items = self.expression(items, env, facets, display, &format_owned(cx, format_args!("{at}.items"))?, cx)?;
                let mut local = env.try_clone_with(cx)?;
                local.insert(binding, items.item(cx)?, cx)?;
                let p = self.expression(predicate, &local, facets, display, &format_owned(cx, format_args!("{at}.predicate"))?, cx)?;
                out.merge(&items, cx)?; out.merge(&p, cx)?;
                if matches!(expr, Expr::Filter { .. }) { out.root = items.root.as_deref().map(|v| copy_string(v, cx)).transpose()?; out.active_root = items.active_root; }
            }
            Expr::Map { items, binding, value } => {
                if !display { return Err(refuse(cx, at, "business map/order fold is unsupported")); }
                let items = self.expression(items, env, facets, true, &format_owned(cx, format_args!("{at}.items"))?, cx)?;
                let mut local = env.try_clone_with(cx)?; local.insert(binding, items.item(cx)?, cx)?;
                let value = self.expression(value, &local, facets, true, &format_owned(cx, format_args!("{at}.value"))?, cx)?;
                out.merge(&items, cx)?; out.merge(&value, cx)?;
            }
            Expr::Count { items } => { let value = self.expression(items, env, facets, display, &format_owned(cx, format_args!("{at}.items"))?, cx)?; out.merge(&value, cx)?; }
            Expr::Add { left, right } => {
                if let Some((entity, _, archived)) = bookkeeping(expr) {
                let mut permitted = false;
                for writer in self.writers {
                    if writer.side != self.side || writer.entity != entity || writer.kind != OwnedStatementKind::Create || !matches!(self.kinds.get(entity), Some(DecodedOwnedSchemaKind::History { sequence, .. }) if writer.field.as_ref() == Some(sequence)) { continue; }
                    if let Some(field) = writer.field.as_deref() {
                        let site = format_owned(cx, format_args!("{} {:?}.{}", writer.action, writer.path, field))?;
                        if site == at { permitted = true; break; }
                    }
                }
                    if !permitted { return Err(refuse(cx, at, "terminal count-plus-one used outside exact append bookkeeping site")); }
                    if !matches!(self.kinds.get(entity), Some(DecodedOwnedSchemaKind::History { archived: value, .. }) if *value == archived) { return Err(refuse(cx, at, "count-plus-one is not verified terminal bookkeeping")); }
                    self.certify(at, "bounded count-plus-one terminal bookkeeping", expr, cx)?;
                } else if let (Some(DataValue::Integer { value: a }), Some(DataValue::Integer { value: b })) = (literal(left), literal(right)) {
                    if a.checked_add(*b).is_none() { return Err(refuse(cx, at, "integer addition is not total")); }
                } else { return Err(refuse(cx, at, "arithmetic range/totality is unproved")); }
            }
            Expr::Subtract { left, right } => if !matches!((literal(left), literal(right)), (Some(DataValue::Integer { value: a }), Some(DataValue::Integer { value: b })) if a.checked_sub(*b).is_some()) { return Err(refuse(cx, at, "subtraction range/totality is unproved")); },
            Expr::DateAdd { .. } | Expr::DateDifference { .. } | Expr::Sum { .. } | Expr::Concat { .. } => return Err(refuse(cx, at, "partial date/arithmetic/text operation is outside total grammar")),
            Expr::Equal { left, right } | Expr::Less { left, right } => { let a = self.expression(left, env, facets, display, &format_owned(cx, format_args!("{at}.left"))?, cx)?; out.merge(&a, cx)?; let b = self.expression(right, env, facets, display, &format_owned(cx, format_args!("{at}.right"))?, cx)?; out.merge(&b, cx)?; }
            Expr::TextContains { text, search } => { let a = self.expression(text, env, facets, display, at, cx)?; out.merge(&a, cx)?; let b = self.expression(search, env, facets, display, at, cx)?; out.merge(&b, cx)?; }
            Expr::Contains { items, value } => { let a = self.expression(items, env, facets, display, at, cx)?; out.merge(&a, cx)?; let b = self.expression(value, env, facets, display, at, cx)?; out.merge(&b, cx)?; }
            Expr::Lower { value } => {
                let Some(DataValue::Text { value: text }) = literal(value) else { return Err(refuse(cx, at, "lowercase output byte range is unproved")); };
                self.meter.charge(text.len() as u64)?;
                if lowercase_len(text).is_none_or(|length| length > MAX_TEXT_BYTES) { return Err(refuse(cx, at, "lowercase expansion exceeds the text byte bound")); }
                let child = self.expression(value, env, facets, display, &format_owned(cx, format_args!("{at}.value"))?, cx)?; out.merge(&child, cx)?;
                self.certify(at, "literal lowercase output fits the text byte bound", expr, cx)?;
            }
            Expr::Not { value } => { let child = self.expression(value, env, facets, display, &format_owned(cx, format_args!("{at}.value"))?, cx)?; out.merge(&child, cx)?; }
            Expr::And { values } | Expr::Or { values } => for (i, value) in values.iter().enumerate() { let child = self.expression(value, env, facets, display, &format_owned(cx, format_args!("{at}[{i}]"))?, cx)?; out.merge(&child, cx)?; },
            Expr::If { condition, then_value, else_value } => for (label, value) in [("condition", condition.as_ref()), ("then", then_value.as_ref()), ("else", else_value.as_ref())] { let child = self.expression(value, env, facets, display, &format_owned(cx, format_args!("{at}.{label}"))?, cx)?; out.merge(&child, cx)?; },
            Expr::Coalesce { value, fallback } => { let a = self.expression(value, env, facets, display, at, cx)?; out.merge(&a, cx)?; let b = self.expression(fallback, env, facets, display, at, cx)?; out.merge(&b, cx)?; }
        }
        Ok(out)
    }

    fn variant(&self, action: &str, path: &[usize]) -> Option<&DecodedOwnedSpanEndpoint> {
        let structure = self.structure?;
        structure.edits.iter().map(|edit| if self.side == OwnedSourceSide::Before { &edit.before } else { &edit.after }).find(|endpoint| {
            endpoint.action == action && path.len() > endpoint.list_path.len() && path.starts_with(&endpoint.list_path)
                && endpoint.start.checked_add(endpoint.len).is_some_and(|end| (endpoint.start..end).contains(&path[endpoint.list_path.len()]))
        })
    }
    fn finite_unfinished(&self, expr: &Expr, root: &str, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<bool, CommonFactInventoryFailure> {
        if let Some(value) = literal(expr) { return Ok(value != &self.lifecycle_value); }
        if let Expr::Field { record, field } = expr {
            if field != self.lifecycle_field || self.ty(record, env, cx)? != reference_type(self.root_entity, cx)? { return Ok(false); }
            return Ok(self.receiver(record, env, cx)?.as_deref() == Some(root));
        }
        if let Expr::If { condition, then_value, else_value } = expr {
            if self.facet(condition, env, cx)?.as_deref() == Some(root) { return self.finite_unfinished(else_value, root, env, cx); }
            return Ok(self.finite_unfinished(then_value, root, env, cx)? && self.finite_unfinished(else_value, root, env, cx)?);
        }
        Ok(false)
    }
    fn unfinished(&self, expr: &Expr, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<OrderedSet<String>, CommonFactInventoryFailure> {
        let mut result = OrderedSet::new();
        match expr {
            Expr::And { values } => for value in values { for root in self.unfinished(value, env, cx)?.iter() { result.try_insert(copy_string(root, cx)?, cx)?; } },
            Expr::Not { value } => if let Some(root) = self.facet(value, env, cx)? { result.try_insert(root, cx)?; },
            Expr::Equal { left, right } => if let (Expr::Field { record, field }, Some(value)) = (left.as_ref(), literal(right)) {
                if field == self.lifecycle_field && value != &self.lifecycle_value && self.ty(record, env, cx)? == reference_type(self.root_entity, cx)? {
                    if let Some(root) = self.receiver(record, env, cx)? { result.try_insert(root, cx)?; }
                }
            },
            _ => (),
        }
        Ok(result)
    }
    fn mixed(&self, action: &str) -> bool {
        self.writers.iter().any(|writer| writer.side == self.side && writer.action == action && match self.kinds.get(&writer.entity) {
            Some(DecodedOwnedSchemaKind::Root) => writer.kind == OwnedStatementKind::Create,
            Some(DecodedOwnedSchemaKind::Aux { .. }) => false,
            Some(_) => true,
            None => false,
        })
    }
    fn shared_update(&self, action: &str) -> bool {
        self.writers.iter().any(|writer| writer.side == self.side && writer.action == action && writer.kind == OwnedStatementKind::Update
            && matches!(self.kinds.get(&writer.entity), Some(DecodedOwnedSchemaKind::Registry { .. })))
    }
    fn auxiliary_absence(&self, action: &DecodedActionDefinition, path: &[usize], entity: &str, root: &str, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<bool, CommonFactInventoryFailure> {
        if path.len() < 2 { return Ok(false); }
        let mut steps = action.steps.as_slice();
        let mut parent: Option<&Expr> = None;
        for index in &path[..path.len() - 1] {
            let Some(DecodedStatement::ForEach { items, steps: inner, .. }) = steps.get(*index) else { return Ok(false); };
            parent = Some(items);
            steps = inner;
        }
        let Some(Expr::Query { entity: gate_entity, binding, predicate, limit, include_archived: false, .. }) = parent else { return Ok(false); };
        if gate_entity != self.root_entity || *limit != MAX_COLLECTION || self.correlation(predicate, gate_entity, binding, env, cx)?.as_deref() != Some(root) { return Ok(false); }
        fn absent(rules: &DecodedCommonFactRules<'_>, expr: &Expr, entity: &str, root: &str, env: &DecodedRuleEnvironment, cx: &mut AllocationContext) -> Result<bool, CommonFactInventoryFailure> {
            if let Expr::And { values } = expr { for value in values { if absent(rules, value, entity, root, env, cx)? { return Ok(true); } } return Ok(false); }
            let Expr::Equal { left, right } = expr else { return Ok(false); };
            if literal(right) != Some(&DataValue::Integer { value: 0 }) { return Ok(false); }
            let Expr::Count { items } = left.as_ref() else { return Ok(false); };
            let Expr::Query { entity: candidate, binding, predicate, limit, sort, include_archived: false } = items.as_ref() else { return Ok(false); };
            Ok(candidate == entity && *limit == MAX_COLLECTION && sort.is_empty() && matches!(predicate.as_ref(), Expr::Equal { .. })
                && rules.correlation(predicate, candidate, binding, env, cx)?.as_deref() == Some(root))
        }
        let sole_create = matches!(steps, [DecodedStatement::Create { entity: created, .. }] if created == entity);
        Ok(sole_create && absent(self, predicate, entity, root, env, cx)?)
    }
    fn statements(&mut self, action: &DecodedActionDefinition, steps: &[DecodedStatement], prefix: &[usize], env: &mut DecodedRuleEnvironment,
        control: &DecodedRuleValue, unfinished: &OrderedSet<String>, owner: Option<&str>, mixed: bool, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        for (index, step) in steps.iter().enumerate() {
            self.meter.charge(1)?;
            let mut path = copy_indices(prefix, cx)?;
            product_alloc::try_push(&mut path, index, cx, AllocationSite::Vector)?;
            let at = format_owned(cx, format_args!("{} {path:?}", action.id))?;
            let endpoint = self.variant(&action.id, &path);
            let variant = endpoint.is_some();
            let mut derived_owner = owner.map(|value| copy_string(value, cx)).transpose()?;
            let root_binding = endpoint.and_then(|value| value.root.as_ref()).map(|root| copy_string(&root.name, cx)).transpose()?;
            if let Some(binding) = root_binding.as_deref() {
                let root_type = reference_type(self.root_entity, cx)?;
                let actual = env.get(binding).filter(|value| value.ty == root_type)
                    .and_then(|value| value.root.as_deref()).ok_or_else(|| refuse(cx, &at, "declared span root has no derived owner identity"))?;
                if owner.is_some_and(|outer| outer != actual) { return Err(refuse(cx, &at, "declared span root differs from its enclosing frontier")); }
                let actual = copy_string(actual, cx)?;
                let root = endpoint.and_then(|value| value.root.as_ref()).ok_or_else(|| refuse(cx, &at, "declared span root disappeared"))?;
                let root = DecodedOwnedRootBinding { name: copy_string(&root.name, cx)?, value_type: copy_type(&root.value_type, cx)? };
                self.certify(&at, "endpoint root authenticated against exact derived owner", &(&root, &actual), cx)?;
                derived_owner = Some(actual);
            }
            let owner = derived_owner.as_deref();
            match step {
                DecodedStatement::ForEach { items, binding, steps: nested } => {
                    let selection = self.expression(items, env, true, false, &format_owned(cx, format_args!("{at}.items"))?, cx)?;
                    if variant && owner.is_none() {
                        if !matches!(items, Expr::Query { entity, limit, include_archived: false, .. } if entity == self.root_entity && *limit == MAX_COLLECTION) {
                            return Err(refuse(cx, &at, "unbound insertion lacks an exact active root traversal"));
                        }
                        self.certify(&at, "one-sided insertion establishes an exact per-row root frontier", &(binding, items, &selection.root), cx)?;
                    }
                    if variant && owner.is_some() && selection.root.as_deref() != owner { return Err(refuse(cx, &at, "nested variant traversal escapes its exact root frontier")); }
                    let mut local = env.try_clone_with(cx)?;
                    local.insert(binding, selection.item(cx)?, cx)?;
                    let mut controls = control.try_clone_with(cx)?;
                    controls.merge(&selection, cx)?;
                    let mut remaining = copy_set(unfinished, cx)?;
                    if let Expr::Query { entity, binding: query_binding, predicate, .. } = items {
                        let mut query_env = env.try_clone_with(cx)?;
                        let mut row = DecodedRuleValue::plain(reference_type(entity, cx)?);
                        row.root = selection.root.as_deref().map(|root| copy_string(root, cx)).transpose()?;
                        query_env.insert(query_binding, row, cx)?;
                        for root in self.unfinished(predicate, &query_env, cx)?.iter() { remaining.try_insert(copy_string(root, cx)?, cx)?; }
                    }
                    let frontier = selection.root.as_deref().or(owner);
                    if !controls.policy.is_empty() && !controls.policy.iter().all(|root| Some(root.as_str()) == frontier) { return Err(refuse(cx, &at, "loop policy control crosses root ownership")); }
                    self.statements(action, nested, &path, &mut local, &controls, &remaining, frontier, mixed, cx)?;
                }
                DecodedStatement::Update { record, values } => {
                    let target = self.expression(record, env, false, false, &format_owned(cx, format_args!("{at}.target"))?, cx)?;
                    let Type::Reference { entity } = &target.ty else { return Err(refuse(cx, &at, "update target is not a required reference")); };
                    let kind = self.kinds.get(entity).ok_or_else(|| refuse(cx, &at, "unknown entity"))?;
                    if matches!(kind, DecodedOwnedSchemaKind::History { .. }) { return Err(refuse(cx, &at, "terminal history update is forbidden")); }
                    if variant && !matches!(kind, DecodedOwnedSchemaKind::Root | DecodedOwnedSchemaKind::Aux { .. }) { return Err(refuse(cx, &at, "variant writes a common relation")); }
                    if matches!(kind, DecodedOwnedSchemaKind::Root) && !target.root.as_ref().is_some_and(|root| unfinished.contains(root)) { return Err(refuse(cx, &at, "business update can touch previously completed history")); }
                    if variant && (owner.is_none() || target.root.as_deref() != owner) { return Err(refuse(cx, &at, "owned write targets a different root")); }
                    let mut previous = None;
                    while let Some((field, value)) = next_map_value(values, previous) {
                        if kind.owner() == Some(field) || matches!(kind, DecodedOwnedSchemaKind::Registry { key } if key == field) { return Err(refuse(cx, &at, "immutable owner/registry key reassignment")); }
                        let v = self.expression(value, env, true, false, &format_owned(cx, format_args!("{at}.{field}"))?, cx)?;
                        let mut deps = control.try_clone_with(cx)?; deps.merge(&target, cx)?; deps.merge(&v, cx)?;
                        if matches!(kind, DecodedOwnedSchemaKind::Root) && (variant || (mixed && !deps.policy.is_empty())) && !target.active_root { return Err(refuse(cx, &at, "policy update lacks unarchived root-target provenance")); }
                        match kind {
                            DecodedOwnedSchemaKind::Root | DecodedOwnedSchemaKind::Aux { .. } => {
                                if variant || !deps.policy.is_empty() { self.discovered.try_insert((copy_string(entity, cx)?, copy_string(field, cx)?), cx)?; }
                                if !deps.policy.iter().all(|root| Some(root.as_str()) == target.root.as_deref()) { return Err(refuse(cx, &at, "owned value depends on another root's policy")); }
                if matches!(kind, DecodedOwnedSchemaKind::Root) && field == self.lifecycle_field {
                                    let root = target.root.as_deref().ok_or_else(|| refuse(cx, &at, "lifecycle target lacks root identity"))?;
                    let finite = self.finite_unfinished(value, root, env, cx)?;
                    if !finite && (mixed || variant || steps.len() != 1 || !prefix.is_empty() || index != 0 || literal(value) != Some(&self.lifecycle_value)) { return Err(refuse(cx, &at, "mixed or unproved lifecycle transition")); }
                                    if !finite { self.certify(&at, "isolated explicit completion transition; capture then seal required", step, cx)?; }
                                }
                            }
                            DecodedOwnedSchemaKind::Link { .. } => if !deps.policy.is_empty() || !matches!(literal(value), Some(DataValue::Boolean { .. })) { return Err(refuse(cx, &at, "link non-owner update must be common literal Boolean")); },
                            DecodedOwnedSchemaKind::Registry { .. } => {
                                let explicit = matches!(record, Expr::Variable { name } if matches!(StringValueMap::<Type>::get(&action.parameters, name), Some(Type::Reference { entity: parameter_entity }) if parameter_entity == entity));
                            if !explicit || !deps.policy.is_empty() || !deps.facets.is_empty() { return Err(refuse(cx, &at, "shared existing fact update requires explicit shared parameter and policy-independent admission")); }
                                if !matches!(value, Expr::Literal { .. } | Expr::Today) && !matches!(value, Expr::Variable { name } if action.parameters.get(name).is_some_and(Type::is_scalar)) { return Err(refuse(cx, &at, "shared fact value is not explicit input/literal/clock")); }
                            }
                            DecodedOwnedSchemaKind::History { .. } => unreachable!(),
                        }
                        previous = Some(field.as_str());
                    }
                    self.certify(&at, if variant { "exact owner policy write with total expressions" } else { "exact common or isolated owner writer" }, step, cx)?;
                }
                DecodedStatement::Create { entity, bind, values } => {
                    let kind = self.kinds.get(entity).ok_or_else(|| refuse(cx, &at, "unknown entity"))?;
                    if variant && !matches!(kind, DecodedOwnedSchemaKind::Aux { .. }) { return Err(refuse(cx, &at, "variant may create only a declared owned auxiliary")); }
                    let mut deps = control.try_clone_with(cx)?;
                    let mut evaluated = OrderedMap::new();
                    let mut previous = None;
                    while let Some((field, value)) = next_map_value(values, previous) {
                        let v = self.expression(value, env, true, false, &format_owned(cx, format_args!("{at}.{field}"))?, cx)?;
                        deps.merge(&v, cx)?;
                        evaluated.try_insert(copy_string(field, cx)?, v, cx)?;
                        previous = Some(field.as_str());
                    }
                    let root = kind.owner().and_then(|field| evaluated.get(field)).and_then(|value| value.root.as_deref());
                    match kind {
                        DecodedOwnedSchemaKind::Aux { .. } => {
                            if !match root { Some(root) => self.auxiliary_absence(action, &path, entity, root, env, cx)?, None => false } { return Err(refuse(cx, &at, "auxiliary uniqueness can reject common transaction: exact current owner-absence gate required")); }
                            if root.is_none() || owner != root || !deps.policy.iter().all(|value| Some(value.as_str()) == root) { return Err(refuse(cx, &at, "auxiliary create lacks exact local owner")); }
                        }
                        DecodedOwnedSchemaKind::Root => {
                            let initially_unfinished = match values.get(self.lifecycle_field) { Some(value) => self.finite_unfinished(value, "new-root", env, cx)?, None => false };
                            if !deps.policy.is_empty() || !initially_unfinished { return Err(refuse(cx, &at, "root creation must be policy-independent and initially unfinished")); }
                        }
                        DecodedOwnedSchemaKind::Registry { key } => {
                            if !deps.policy.is_empty() || deps.private_identity { return Err(refuse(cx, &at, "policy-derived shared registry create")); }
                            let mut previous = None;
                            while let Some((field, value)) = next_map_value(values, previous) {
                                if field == key {
                                    if let Expr::Field { record, field: source_field } = value {
                                        let Type::Reference { entity: source_entity } = self.ty(record, env, cx)? else { return Err(refuse(cx, &at, "registry key origin")); };
                                        if self.writers.iter().any(|writer| writer.entity == source_entity && writer.field.as_ref() == Some(source_field) && writer.kind != OwnedStatementKind::Create) { return Err(refuse(cx, &at, "registry name must come from immutable legacy fact or explicit scalar input")); }
                                    }
                                } else if !matches!(literal(value), Some(DataValue::Boolean { value: false } | DataValue::Null)) && !matches!(literal(value), Some(DataValue::Text { value }) if value.is_empty()) { return Err(refuse(cx, &at, "registry initial receipt fields must be empty/false/null literals")); }
                                previous = Some(field.as_str());
                            }
                        }
                        DecodedOwnedSchemaKind::Link { .. } => {
                            if root.is_none() || !deps.policy.is_empty() || deps.private_identity { return Err(refuse(cx, &at, "common link create has unproved owner or private dependency")); }
                            let mut previous = None;
                            while let Some((field, value)) = next_map_value(values, previous) {
                                if self.app.entities().iter().find(|candidate| candidate.id == *entity).and_then(|candidate| candidate.fields.iter().find(|candidate| candidate.id == *field)).is_some_and(|candidate| candidate.value_type == Type::Boolean) && !matches!(literal(value), Some(DataValue::Boolean { .. })) { return Err(refuse(cx, &at, "new link flags must be literals")); }
                                previous = Some(field.as_str());
                            }
                        }
                        DecodedOwnedSchemaKind::History { sequence, owner: history_owner, .. } => {
                            if !deps.policy.is_empty() || deps.private_identity || (!deps.facets.is_empty() && history_owner.is_none()) { return Err(refuse(cx, &at, "terminal append has policy/lifecycle-selected common admission")); }
                            if !values.get(sequence).and_then(bookkeeping).is_some_and(|(candidate, _, _)| candidate == entity) { return Err(refuse(cx, &at, "terminal producer bookkeeping differs")); }
                        }
                    }
                    let mut created = DecodedRuleValue::plain(reference_type(entity, cx)?);
                    created.root = if matches!(kind, DecodedOwnedSchemaKind::Root) { Some(format_owned(cx, format_args!("{at}/{bind}"))?) } else { root.map(|value| copy_string(value, cx)).transpose()? };
                    created.active_root = matches!(kind, DecodedOwnedSchemaKind::Root) || self.all_roots_active || env.0.iter().any(|(_, value)| value.active_root && value.root.is_some() && value.root == created.root);
                    created.private_identity = variant;
                    env.insert(bind, created, cx)?;
                    self.certify(&at, if variant { "exact owned auxiliary create" } else { "exact common creation with complete producer/consumer closure" }, step, cx)?;
                }
                DecodedStatement::SetState { state, value } => {
                    if variant || !control.policy.is_empty() || prefix.len() != 0 || index + 1 != steps.len() { return Err(refuse(cx, &at, "input reset must be literal empty, common, top-level and final")); }
                    // Non-empty continuation markers require retained-transition evidence, which is outside this decoded rule unit.
                    if matches!(literal(value), Some(DataValue::Text { value }) if value.is_empty()) {
                        let rule = format_owned(cx, format_args!("closed input reset {state}"))?;
                        self.certify(&at, &rule, step, cx)?;
                    }
                    else { return Err(refuse(cx, &at, "input reset must be literal empty, common, top-level and final")); }
                }
                DecodedStatement::Assert { condition, .. } => {
                    let value = self.expression(condition, env, false, false, &at, cx)?;
                    if variant || !value.policy.is_empty() || !control.policy.is_empty() { return Err(refuse(cx, &at, "dependent assertion is unsupported")); }
                }
                DecodedStatement::Archive { .. } | DecodedStatement::Collection { .. } | DecodedStatement::Emit { .. } => return Err(refuse(cx, &at, "archive/collection/output effect has no closed preview rule")),
            }
        }
        Ok(())
    }
    pub(crate) fn actions(&mut self, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        if self.structure.is_none() { return Err(refuse(cx, "actions", "decoded structural inventory required")); }
        let AppRef::Decoded(app) = self.app else { return Err(refuse(cx, "actions", "decoded app owner required")); };
        for action in &app.actions {
            let mut env = self.environment(&action.parameters, &action.id, cx)?;
            let mut controls = DecodedRuleValue::plain(Type::Boolean);
            let mut unfinished = OrderedSet::new();
            let mixed = self.mixed(&action.id);
            for (index, guard) in action.guards.iter().enumerate() {
                let at = format_owned(cx, format_args!("{} guard[{index}]", action.id))?;
                let value = self.expression(guard, &env, true, false, &at, cx)?;
                controls.merge(&value, cx)?;
                for root in self.unfinished(guard, &env, cx)?.iter() { unfinished.try_insert(copy_string(root, cx)?, cx)?; }
            }
            if mixed && !controls.policy.is_empty() { return Err(refuse(cx, &action.id, "common factual admission depends on private policy rather than exact lifecycle facet")); }
            if !action.ensures.is_empty() { return Err(refuse(cx, &action.id, "action postconditions are outside closed total admission grammar")); }
            self.statements(action, &action.steps, &[], &mut env, &controls, &unfinished, None, mixed, cx)?;
        }
        Ok(())
    }

    fn stable_own_row(expr: &Expr, binding: &str) -> bool {
        match expr {
            Expr::Literal { .. } => true,
            Expr::Field { record, .. } => variable(record, binding),
            Expr::Not { value } => Self::stable_own_row(value, binding),
            Expr::Equal { left, right } | Expr::Less { left, right } => Self::stable_own_row(left, binding) && Self::stable_own_row(right, binding),
            Expr::And { values } | Expr::Or { values } => values.iter().all(|value| Self::stable_own_row(value, binding)),
            Expr::If { condition, then_value, else_value } => [condition, then_value, else_value].iter().all(|value| Self::stable_own_row(value, binding)),
            Expr::Coalesce { value, fallback } => Self::stable_own_row(value, binding) && Self::stable_own_row(fallback, binding),
            _ => false,
        }
    }
    fn historical_rows(&mut self, view: &crate::product_contract::fallible_app_decode::DecodedViewDefinition, rows: &Expr, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        let Expr::Query { entity, binding, predicate, sort, limit, include_archived } = rows else {
            return Err(refuse(cx, &view.id, "historical root rows require an exact root query"));
        };
        let mut row = DecodedRuleValue::plain(reference_type(entity, cx)?);
        row.root = Some(copy_string("historical-view-root", cx)?);
        let mut env = DecodedRuleEnvironment::new();
        env.insert(binding, row, cx)?;
        let first = match predicate.as_ref() { Expr::And { values } => values.first(), other => Some(other) };
        let excludes_history = if !include_archived {
            if let Some(Expr::Not { value }) = first { self.facet(value, &env, cx)?.as_deref() == Some("historical-view-root") } else { false }
        } else { false };
        if entity != self.root_entity || *limit != MAX_COLLECTION || (!Self::stable_own_row(predicate, binding) && !excludes_history)
            || !sort.iter().all(|key| Self::stable_own_row(&key.value, binding)) {
            return Err(refuse(cx, &view.id, "historical root membership/order lacks stable own-row or completed-exclusion rule"));
        }
        let site = format_owned(cx, format_args!("view {} rows", view.id))?;
        self.certify(&site, "untruncated stable historical root membership/order or leading completed exclusion", rows, cx)
    }
    fn history_view(&mut self, view: &crate::product_contract::fallible_app_decode::DecodedViewDefinition, entity: &str, rows: &Expr, columns: &[crate::product_contract::fallible_app_decode::DecodedColumn], cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        let Some(DecodedOwnedSchemaKind::History { sequence, .. }) = self.kinds.get(entity) else { return Err(refuse(cx, &view.id, "history must be an inert direct query")); };
        let Expr::Query { entity: queried, binding, predicate, sort, limit, .. } = rows else {
            return Err(refuse(cx, &view.id, "history must be an inert direct query"));
        };
        if queried != entity || literal(predicate) != Some(&DataValue::Boolean { value: true }) || *limit != MAX_COLLECTION
            || sort.len() != 1 || !matches!(&sort[0].value, Expr::Field { record, field } if variable(record, binding) && field == sequence)
            || !view.actions.is_empty() || view.has_keys() {
            return Err(refuse(cx, &view.id, "fresh-identity history consumer has unsupported order, selection or action"));
        }
        if let crate::product_contract::fallible_app_decode::DecodedViewKind::List { controls, selection, .. } = &view.kind {
            if !controls.is_empty() || selection.is_some() { return Err(refuse(cx, &view.id, "history controls/selection would expose shifted row identities")); }
        }
        let mut env = DecodedRuleEnvironment::new();
        env.insert("row", DecodedRuleValue::plain(reference_type(entity, cx)?), cx)?;
        for column in columns {
            if !matches!(&column.value, Expr::Field { record, .. } if variable(record, "row")) {
                return Err(refuse(cx, &view.id, "history columns must be direct stored scalar fields"));
            }
            let value = self.expression(&column.value, &env, false, true, &view.id, cx)?;
            if !value.ty.is_scalar() && !matches!(value.ty, Type::Optional { .. }) {
                return Err(refuse(cx, &view.id, "history cannot expose references or list identity consumers"));
            }
        }
        let site = format_owned(cx, format_args!("view {}", view.id))?;
        self.certify(&site, "inert stored-field history, distinct retained/inductive sequence order; actual row IDs retained", view, cx)
    }
    pub(crate) fn observations(&mut self, cx: &mut AllocationContext) -> Result<(), CommonFactInventoryFailure> {
        let structure = self.structure.ok_or_else(|| refuse(cx, "observations", "decoded structural inventory required"))?;
        let AppRef::Decoded(app) = self.app else { return Err(refuse(cx, "observations", "decoded app owner required")); };
        for view in &app.views {
            let mut row_env = DecodedRuleEnvironment::new();
            match &view.kind {
                crate::product_contract::fallible_app_decode::DecodedViewKind::List { entity, rows, columns, controls, selection } => {
                    if matches!(self.kinds.get(entity), Some(DecodedOwnedSchemaKind::History { .. })) {
                        self.history_view(view, entity, rows, columns, cx)?;
                        continue;
                    }
                    if entity == &structure.root_entity { self.historical_rows(view, rows, cx)?; }
                    let rows_at = format_owned(cx, format_args!("view {} rows", view.id))?;
                    let rows_value = self.expression(rows, &DecodedRuleEnvironment::new(), true, true, &rows_at, cx)?;
                    if !rows_value.policy.is_empty() { return Err(refuse(cx, &view.id, "row identity/selection depends on private policy")); }
                    let mut row = rows_value.item(cx)?;
                    if entity == &structure.root_entity || matches!(self.kinds.get(entity), Some(DecodedOwnedSchemaKind::Link { .. } | DecodedOwnedSchemaKind::Aux { .. })) {
                        row.root = Some(format_owned(cx, format_args!("view/{}/row", view.id))?);
                    }
                    row_env.insert("row", row.try_clone_with(cx)?, cx)?;
                    for column in columns {
                        let at = format_owned(cx, format_args!("view {} column {}", view.id, column.id))?;
                        let value = self.expression(&column.value, &row_env, true, true, &at, cx)?;
                        if entity == &structure.root_entity && value.frame_dependent { return Err(refuse(cx, &at, "historical time/session-dependent column is unsupported")); }
                        if !value.policy.iter().all(|root| Some(root.as_str()) == row.root.as_deref()) { return Err(refuse(cx, &view.id, "display reads another root's private policy")); }
                    }
                    if controls.iter().any(|control| control.on_change.is_some()) || selection.is_some() {
                        return Err(refuse(cx, &view.id, "callback/selection state ingress lacks a closed input rule"));
                    }
                }
                crate::product_contract::fallible_app_decode::DecodedViewKind::Detail { .. } => return Err(refuse(cx, &view.id, "detail identity/history selection is unsupported")),
                crate::product_contract::fallible_app_decode::DecodedViewKind::Form { defaults, .. } => {
                    if defaults.iter().any(|(_, value)| matches!(value, DataValue::Reference { .. } | DataValue::List { .. })) {
                        return Err(refuse(cx, &view.id, "form reference defaults lack independent provenance"));
                    }
                }
            }
            for action in &view.actions {
                let empty = DecodedRuleEnvironment::new();
                let env = if action.placement == crate::product_contract::ActionPlacement::Row { &row_env } else { &empty };
                let enabled_at = format_owned(cx, format_args!("view {} binding {} enabled", view.id, action.id))?;
                let enabled = self.expression(&action.enabled, env, true, true, &enabled_at, cx)?;
                let target = app.actions.iter().find(|candidate| candidate.id == action.action).ok_or_else(|| refuse(cx, &view.id, "binding target action missing"))?;
                if (self.mixed(&target.id) && !enabled.policy.is_empty()) || (self.shared_update(&action.action) && !enabled.facets.is_empty()) {
                    let at = format_owned(cx, format_args!("view {} binding {}", view.id, action.id))?;
                    return Err(refuse(cx, &at, "common factual UI admission depends on private policy or unrelated lifecycle"));
                }
                if !enabled.policy.iter().all(|root| Some(root.as_str()) == env.get("row").and_then(|row| row.root.as_deref())) {
                    return Err(refuse(cx, &view.id, "nonlocal policy enablement"));
                }
                let mut previous = None;
                while let Some((name, arg)) = next_map_value(&action.arguments, previous) {
                    let at = format_owned(cx, format_args!("view {} binding {} argument {name}", view.id, action.id))?;
                    let value = self.expression(arg, env, false, false, &at, cx)?;
                    if !value.policy.is_empty() || value.private_identity { return Err(refuse(cx, &view.id, "policy-derived scalar/reference binding laundering")); }
                    previous = Some(name);
                }
            }
        }
        for observable in &app.observables {
            let at = format_owned(cx, format_args!("observable {}", observable.id))?;
            let value = self.expression(&observable.value, &DecodedRuleEnvironment::new(), true, true, &at, cx)?;
            if value.session_dependent { return Err(refuse(cx, &observable.id, "historical session-dependent observable is unsupported")); }
            if value.frame_dependent {
                let guarded = if let Expr::Count { items } = &observable.value {
                    if let Expr::Query { entity, binding, predicate, .. } = items.as_ref() {
                        let first = match predicate.as_ref() { Expr::And { values } => values.first(), other => Some(other) };
                        let mut row = DecodedRuleValue::plain(reference_type(entity, cx)?);
                        row.root = Some(copy_string("historical-count-root", cx)?);
                        let mut env = DecodedRuleEnvironment::new(); env.insert(binding, row, cx)?;
                        entity == &structure.root_entity && matches!(first, Some(Expr::Not { value }) if self.facet(value, &env, cx)?.as_deref() == Some("historical-count-root"))
                    } else { false }
                } else { false };
                if !guarded { return Err(refuse(cx, &observable.id, "time/session historical count lacks leading completed-false short-circuit")); }
            }
            if !value.policy.is_empty() {
                let Expr::Count { items } = &observable.value else { return Err(refuse(cx, &observable.id, "private aggregate is not Count(Query(root))")); };
                let Expr::Query { entity, limit, sort, .. } = items.as_ref() else { return Err(refuse(cx, &observable.id, "private count has no exact root query")); };
                if entity != &structure.root_entity || *limit != MAX_COLLECTION || !sort.is_empty() { return Err(refuse(cx, &observable.id, "private count is cross-root, sorted or truncated")); }
            }
        }
        Ok(())
    }
}

/// Derive projection recipes from decoded before/after app owners. This mirrors
/// the legacy rules while keeping every owned recipe allocation fallible.
pub(crate) fn decoded_rule_projections(
    before: AppRef<'_>,
    after: AppRef<'_>,
    root: &str,
    cx: &mut AllocationContext,
) -> Result<Vec<OwnedProjectionRecipe>, CommonFactInventoryFailure> {
    let (AppRef::Decoded(before), AppRef::Decoded(after)) = (before, after) else {
        return Err(refuse(cx, "projections", "decoded app owners required"));
    };
    let mut result = Vec::new();
    let mut row_environment = DecodedRuleEnvironment::new();
    row_environment.insert("row", DecodedRuleValue::plain(reference_type(root, cx)?), cx)?;

    for view in &after.views {
        let crate::product_contract::fallible_app_decode::DecodedViewKind::List { entity, rows, columns, .. } = &view.kind else { continue; };
        if entity != root { continue; }
        let original = before.views.iter().find(|candidate| candidate.id == view.id)
            .ok_or_else(|| refuse(cx, &view.id, "new root views unsupported"))?;
        let crate::product_contract::fallible_app_decode::DecodedViewKind::List { rows: old_rows, columns: old_columns, .. } = &original.kind
            else { return Err(refuse(cx, &view.id, "view kind changed")); };
        if rows != old_rows { return Err(refuse(cx, &view.id, "row selection/identity changed")); }

        for column in columns {
            let prior = old_columns.iter().find(|candidate| candidate.id == column.id);
            let value_type = product_contract::decoded_expression_type_in_with(after, &column.value, &row_environment, cx).map_err(app_error)?;
            if prior.is_none() && value_type != Type::Boolean {
                let at = format_owned(cx, format_args!("{}.{}", view.id, column.id))?;
                return Err(refuse(cx, &at, "added column has no nullable Boolean availability rule"));
            }
            if let Some(prior) = prior {
                let prior_type = product_contract::decoded_expression_type_in_with(before, &prior.value, &row_environment, cx).map_err(app_error)?;
                if prior_type != value_type { return Err(refuse(cx, &view.id, "changed projection type is unsupported")); }
            }

            let host_value_type = if prior.is_none() {
                Type::Optional { item: product_alloc::try_box(copy_type(&value_type, cx)?, cx)? }
            } else { copy_type(&value_type, cx)? };
            let recipe = OwnedProjectionRecipe {
                location: format_owned(cx, format_args!("view/{}/{}", view.id, column.id))?,
                destination: OwnedProjectionDestination::ViewColumn { view: copy_string(&view.id, cx)?, column: copy_string(&column.id, cx)? },
                binding: copy_string("row", cx)?,
                host_value_type,
                absent_value: if prior.is_none() { Some(DataValue::Null) } else { None },
                value_type: copy_type(&value_type, cx)?,
                baseline: prior.map(|value| copy_expr(&value.value, cx)).transpose()?,
                candidate: copy_expr(&column.value, cx)?,
                capture_required: true,
            };
            product_alloc::try_push(&mut result, recipe, cx, AllocationSite::Vector)?;
        }
    }

    for observable in &after.observables {
        let Expr::Count { items } = &observable.value else { continue; };
        let Expr::Query { entity, binding, predicate, sort, limit, include_archived } = items.as_ref() else { continue; };
        if entity != root { continue; }
        if *limit != MAX_COLLECTION || !sort.is_empty() {
            return Err(refuse(cx, &observable.id, "root count must be untruncated and unordered before mixed per-root selection"));
        }
        let prior = before.observables.iter().find(|candidate| candidate.id == observable.id);
        let baseline = if let Some(prior) = prior {
            let Expr::Count { items } = &prior.value else { return Err(refuse(cx, &observable.id, "count changed shape")); };
            let Expr::Query { entity: old_entity, binding: old_binding, predicate: old_predicate, sort: old_sort, limit: old_limit, include_archived: old_include_archived } = items.as_ref()
                else { return Err(refuse(cx, &observable.id, "count changed query")); };
            if (old_entity, old_binding, old_sort, old_limit, old_include_archived) != (entity, binding, sort, limit, include_archived) {
                return Err(refuse(cx, &observable.id, "count header changed"));
            }
            Some(copy_expr(old_predicate, cx)?)
        } else { None };

        let recipe = OwnedProjectionRecipe {
            location: format_owned(cx, format_args!("count/{}", observable.id))?,
            destination: OwnedProjectionDestination::RootCount { observable: copy_string(&observable.id, cx)?, include_archived: *include_archived },
            binding: copy_string(binding, cx)?,
            value_type: Type::Boolean,
            host_value_type: Type::Boolean,
            absent_value: if baseline.is_none() { Some(DataValue::Boolean { value: false }) } else { None },
            baseline,
            candidate: copy_expr(predicate, cx)?,
            capture_required: true,
        };
        product_alloc::try_push(&mut result, recipe, cx, AllocationSite::Vector)?;
    }
    Ok(result)
}

fn app_error(value: product_contract::AppValidationError) -> CommonFactInventoryFailure { match value { product_contract::AppValidationError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), product_contract::AppValidationError::Contract(v) => CommonFactInventoryFailure::Store(v.into()) } }
fn codec_error(value: crate::product_backup::fallible_codec::CodecError) -> CommonFactInventoryFailure { match value { crate::product_backup::fallible_codec::CodecError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), _ => CommonFactInventoryFailure::Store(StoreError::Invalid("Common factual admission: identity encoding invariant".into())) } }
fn copy_string(value: &str, cx: &mut AllocationContext) -> Result<String, CommonFactInventoryFailure> { product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into) }
fn copy_indices(value: &[usize], cx: &mut AllocationContext) -> Result<Vec<usize>, CommonFactInventoryFailure> { let mut result = Vec::new(); product_alloc::try_reserve(&mut result, value.len(), cx, AllocationSite::Vector)?; result.extend_from_slice(value); Ok(result) }
fn copy_set(value: &OrderedSet<String>, cx: &mut AllocationContext) -> Result<OrderedSet<String>, CommonFactInventoryFailure> { let mut result = OrderedSet::new(); for item in value.iter() { result.try_insert(copy_string(item, cx)?, cx)?; } Ok(result) }
fn next_string_value<'a, M, V>(map: &'a M, after: Option<&str>) -> Option<(&'a String, &'a V)>
where M: StringValueMap<V>, V: 'a {
    map.iter().filter(|(key, _)| after.is_none_or(|previous| key.as_str() > previous)).min_by(|left, right| left.0.cmp(right.0))
}
fn next_map_value<'a>(map: &'a FallibleMap<Expr>, after: Option<&str>) -> Option<(&'a String, &'a Expr)> {
    next_string_value(map, after)
}
fn copy_type(value: &Type, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> { product_contract::try_clone_type_with(value, cx).map_err(Into::into) }
fn copy_expr(value: &Expr, cx: &mut AllocationContext) -> Result<Expr, CommonFactInventoryFailure> { product_contract::try_clone_expr_with(value, cx).map_err(app_error) }
fn format_owned(cx: &mut AllocationContext, args: std::fmt::Arguments<'_>) -> Result<String, CommonFactInventoryFailure> { let mut value = String::new(); product_alloc::try_append_fmt(&mut value, args, cx, AllocationSite::Formatting).map_err(|failure| match failure { product_alloc::FormatFailure::Allocation(v) => CommonFactInventoryFailure::Allocation(v), product_alloc::FormatFailure::Formatter => CommonFactInventoryFailure::Store(StoreError::Invalid("Common factual admission: formatting invariant".into())) })?; Ok(value) }
fn refuse(cx: &mut AllocationContext, at: &str, rule: &str) -> CommonFactInventoryFailure { match format_owned(cx, format_args!("Common factual admission: {at}: {rule}")) { Ok(text) => CommonFactInventoryFailure::Store(StoreError::Invalid(text)), Err(error) => error } }
fn literal(expr: &Expr) -> Option<&DataValue> { if let Expr::Literal { value, .. } = expr { Some(value) } else { None } }
fn variable(expr: &Expr, name: &str) -> bool { matches!(expr, Expr::Variable { name: value } if value == name) }
fn relation(ty: &Type) -> Option<&str> { match ty { Type::Reference { entity } => Some(entity), Type::Optional { item } | Type::List { item } => relation(item), _ => None } }
fn reference_type(entity: &str, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> { Ok(Type::Reference { entity: copy_string(entity, cx)? }) }
fn lowercase_len(value: &str) -> Option<usize> { value.chars().try_fold(0_usize, |length, ch| { let mut encoded = [0_u8; 4]; ch.to_lowercase().try_fold(length, |length, lowered| { let bytes = lowered.encode_utf8(&mut encoded); length.checked_add(bytes.len()) }) }) }
fn bookkeeping(expr: &Expr) -> Option<(&str, &str, bool)> {
    let Expr::Add { left, right } = expr else { return None; };
    if literal(right) != Some(&DataValue::Integer { value: 1 }) { return None; }
    let Expr::Count { items } = left.as_ref() else { return None; };
    let Expr::Query { entity, binding, predicate, sort, limit, include_archived } = items.as_ref() else { return None; };
    if literal(predicate) != Some(&DataValue::Boolean { value: true }) || !sort.is_empty() || *limit != MAX_COLLECTION { return None; }
    Some((entity, binding, *include_archived))
}
pub(crate) fn lifecycle_criteria<'a>(predicate: &'a Expr, cx: &mut AllocationContext) -> Result<(&'a str, &'a DataValue), CommonFactInventoryFailure> {
    let Expr::Equal { left, right } = predicate else { return Err(refuse(cx, "lifecycle", "only typed own-row field equals literal is supported")); };
    let Expr::Field { record, field } = left.as_ref() else { return Err(refuse(cx, "lifecycle", "completion receiver is not own row")); };
    if !variable(record, "record") { return Err(refuse(cx, "lifecycle", "completion receiver is not exact record binder")); }
    let Some(value) = literal(right) else { return Err(refuse(cx, "lifecycle", "completion value must be a literal")); };
    if !matches!(value, DataValue::Text { .. } | DataValue::Boolean { .. }) { return Err(refuse(cx, "lifecycle", "unsupported lifecycle type")); }
    Ok((field, value))
}

#[cfg(test)]
mod tests {
    use super::*;
    mod fixture {
        include!("../../../tests/fixtures/product_scope/mod.rs");
    }
    use crate::product_store::scope::common_fact_admission::{ProofBudget, ProofMeter};
    use std::sync::{atomic::AtomicBool, Arc};

    fn app() -> crate::product_contract::FallibleDecodedCapturedProgram {
        let mut source = fixture::program(false).program;
        source.entities.push(serde_json::from_value(serde_json::json!({"id":"requirement","label":"Requirement","fields":[{"id":"job","label":"Job","value_type":{"kind":"reference","entity":"job"}},{"id":"secret","label":"Secret","value_type":{"kind":"boolean"}}],"unique":[["job"]],"constraints":[]})).unwrap());
        source.entities.push(serde_json::from_value(serde_json::json!({"id":"history","label":"History","fields":[{"id":"job","label":"Job","value_type":{"kind":"reference","entity":"job"}},{"id":"sequence","label":"Sequence","value_type":{"kind":"integer"}},{"id":"archived","label":"Archived","value_type":{"kind":"boolean"}}],"unique":[["job","sequence"]],"constraints":[]})).unwrap());
        source.state.push(crate::product_contract::StateDefinition { id: "input".into(), label: "Input".into(), value_type: Type::Text, initial: DataValue::Text { value: String::new() } });
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut cx).unwrap();
        crate::product_contract::decode_captured_program_fallible(value, &mut cx).unwrap()
    }
    fn proof_meter() -> ProofMeter {
        ProofMeter::new(ProofBudget::default(), Arc::new(AtomicBool::new(false))).unwrap()
    }
    fn setup() -> (crate::product_contract::FallibleDecodedCapturedProgram, DecodedOwnedSchemaKinds, OrderedSet<(String, String)>) {
        let app = app();
        let kinds = DecodedOwnedSchemaKinds::test_from([
            ("job".into(), DecodedOwnedSchemaKind::Root),
            ("requirement".into(), DecodedOwnedSchemaKind::Aux { owner: "job".into() }),
            ("history".into(), DecodedOwnedSchemaKind::History { owner: Some("job".into()), sequence: "sequence".into(), archived: false }),
        ]);
        (app, kinds, OrderedSet::new())
    }
    fn integer(value: i64) -> Expr { Expr::Literal { value_type: Type::Integer, value: DataValue::Integer { value } } }
    fn boolean(value: bool) -> Expr { Expr::Literal { value_type: Type::Boolean, value: DataValue::Boolean { value } } }
    fn empty_rules<'a>(
        app: &'a crate::product_contract::FallibleDecodedCapturedProgram,
        kinds: &'a DecodedOwnedSchemaKinds,
        policy: &'a OrderedSet<(String, String)>,
        meter: &'a mut ProofMeter,
    ) -> DecodedCommonFactRules<'a> {
        DecodedCommonFactRules::new(AppRef::Decoded(app.app()), kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, policy, &[], false, meter)
    }
    fn root_env() -> DecodedRuleEnvironment {
        let mut env = DecodedRuleEnvironment::new();
        let mut value = DecodedRuleValue::typed(Type::reference("job"));
        value.root = Some("argument/root".into());
        value.active_root = true;
        env.insert("root", value, &mut AllocationContext::new()).unwrap();
        env
    }
    fn empty_structure(root: &str) -> DecodedOwnedStructuralReport {
        let digest = || Digest::from_owned_lower_hex("0000000000000000000000000000000000000000000000000000000000000000".into()).unwrap();
        DecodedOwnedStructuralReport { before: digest(), after: digest(), root_entity: root.into(), edits: vec![], common_statements: vec![], writers: vec![], changes: vec![] }
    }

    fn app_with_view(kind: crate::product_contract::ViewKind) -> crate::product_contract::FallibleDecodedCapturedProgram {
        let mut source = fixture::program(false).program;
        source.initial_view = "view".into();
        source.views = vec![crate::product_contract::ViewDefinition { id: "view".into(), label: "View".into(), kind, actions: vec![], keys: vec![] }];
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut cx).unwrap();
        crate::product_contract::decode_captured_program_fallible(value, &mut cx).unwrap()
    }

    fn decode_program(source: crate::product_contract::AppDefinition) -> crate::product_contract::FallibleDecodedCapturedProgram {
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut cx).unwrap();
        crate::product_contract::decode_captured_program_fallible(value, &mut cx).unwrap()
    }

    #[test]
    fn decoded_rule_projections_preserve_recipe_rules_and_allocation_refusal() {
        let mut before_source = fixture::program(false).program;
        before_source.initial_view = "root-list".into();
        before_source.views.clear();
        before_source.observables.clear();
        let rows = Expr::Query {
            entity: "job".into(), binding: "item".into(), predicate: Box::new(boolean(true)),
            sort: vec![], limit: MAX_COLLECTION, include_archived: true,
        };
        let old_column = crate::product_contract::Column {
            id: "done".into(), label: "Done".into(),
            value: Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "done".into() },
        };
        before_source.views.push(crate::product_contract::ViewDefinition {
            id: "root-list".into(), label: "Jobs".into(),
            kind: crate::product_contract::ViewKind::List { entity: "job".into(), rows: rows.clone(), columns: vec![old_column.clone()], controls: vec![], selection: None },
            actions: vec![], keys: vec![],
        });
        let old_predicate = Expr::Equal {
            left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "item".into() }), field: "done".into() }),
            right: Box::new(boolean(false)),
        };
        let old_count = Expr::Count { items: Box::new(Expr::Query {
            entity: "job".into(), binding: "item".into(), predicate: Box::new(old_predicate.clone()),
            sort: vec![], limit: MAX_COLLECTION, include_archived: true,
        }) };
        before_source.observables.push(crate::product_contract::ObservableDefinition { id: "open-count".into(), label: "Open".into(), value: old_count });

        let mut after_source = before_source.clone();
        let added_column = crate::product_contract::Column { id: "available".into(), label: "Available".into(), value: boolean(true) };
        let crate::product_contract::ViewKind::List { columns, .. } = &mut after_source.views[0].kind else { panic!() };
        columns.push(added_column.clone());
        after_source.observables[0].value = Expr::Count { items: Box::new(Expr::Query {
            entity: "job".into(), binding: "item".into(), predicate: Box::new(boolean(true)),
            sort: vec![], limit: MAX_COLLECTION, include_archived: true,
        }) };
        after_source.observables.push(crate::product_contract::ObservableDefinition {
            id: "new-count".into(), label: "New count".into(),
            value: Expr::Count { items: Box::new(Expr::Query {
                entity: "job".into(), binding: "item".into(), predicate: Box::new(boolean(false)),
                sort: vec![], limit: MAX_COLLECTION, include_archived: false,
            }) },
        });
        let mut non_boolean_source = before_source.clone();
        let crate::product_contract::ViewKind::List { columns, .. } = &mut non_boolean_source.views[0].kind else { panic!() };
        columns.push(crate::product_contract::Column { id: "not-boolean".into(), label: "Not Boolean".into(), value: integer(1) });
        let mut changed_type_source = after_source.clone();
        let crate::product_contract::ViewKind::List { columns, .. } = &mut changed_type_source.views[0].kind else { panic!() };
        columns[0].value = Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "name".into() };
        let mut changed_header_source = after_source.clone();
        let Expr::Count { items } = &mut changed_header_source.observables[0].value else { panic!() };
        let Expr::Query { include_archived, .. } = items.as_mut() else { panic!() };
        *include_archived = false;
        let mut truncated_count_source = after_source.clone();
        let Expr::Count { items } = &mut truncated_count_source.observables[0].value else { panic!() };
        let Expr::Query { limit, .. } = items.as_mut() else { panic!() };
        *limit = 1;
        let before = decode_program(before_source);
        let after = decode_program(after_source);

        let mut success = AllocationContext::new();
        let recipes = decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(after.app()), "job", &mut success).unwrap();
        assert_eq!(recipes.len(), 4, "the complete stable view, new Boolean column, and root counts need recipes");
        assert_eq!(recipes[0].location, "view/root-list/done");
        assert_eq!(recipes[0].destination, OwnedProjectionDestination::ViewColumn { view: "root-list".into(), column: "done".into() });
        assert_eq!(recipes[0].binding, "row");
        assert_eq!(recipes[0].host_value_type, Type::Boolean);
        assert_eq!(recipes[0].absent_value, None);
        assert_eq!(recipes[0].baseline, Some(old_column.value.clone()));
        assert_eq!(recipes[0].candidate, old_column.value);
        assert!(recipes[0].capture_required);
        assert_eq!(recipes[1].location, "view/root-list/available");
        assert_eq!(recipes[1].host_value_type, Type::Optional { item: Box::new(Type::Boolean) });
        assert_eq!(recipes[1].absent_value, Some(DataValue::Null));
        assert_eq!(recipes[1].baseline, None);
        assert_eq!(recipes[1].candidate, added_column.value);
        assert_eq!(recipes[2].location, "count/open-count");
        assert_eq!(recipes[2].destination, OwnedProjectionDestination::RootCount { observable: "open-count".into(), include_archived: true });
        assert_eq!(recipes[2].binding, "item");
        assert_eq!(recipes[2].value_type, Type::Boolean);
        assert_eq!(recipes[2].absent_value, None);
        assert_eq!(recipes[2].baseline, Some(old_predicate));
        assert_eq!(recipes[2].candidate, boolean(true));
        assert!(recipes[2].capture_required);
        assert_eq!(recipes[3].location, "count/new-count");
        assert_eq!(recipes[3].baseline, None);
        assert_eq!(recipes[3].absent_value, Some(DataValue::Boolean { value: false }));

        let growths = success.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(after.app()), "job", &mut cx), Err(CommonFactInventoryFailure::Allocation(_))), "growth {nth} must preserve allocation refusal");
        }
        let mut retry = AllocationContext::new();
        assert_eq!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(after.app()), "job", &mut retry).unwrap(), recipes);

        let mut changed_rows_source = fixture::program(false).program;
        changed_rows_source.views.clear();
        changed_rows_source.initial_view = "root-list".into();
        changed_rows_source.views.push(crate::product_contract::ViewDefinition {
            id: "root-list".into(), label: "Jobs".into(),
            kind: crate::product_contract::ViewKind::List { entity: "job".into(), rows: rows.clone(), columns: vec![], controls: vec![], selection: None },
            actions: vec![], keys: vec![],
        });
        let mut changed_rows = changed_rows_source.clone();
        let crate::product_contract::ViewKind::List { rows, .. } = &mut changed_rows.views[0].kind else { panic!() };
        rows.clone_from(&Expr::Query { entity: "job".into(), binding: "item".into(), predicate: Box::new(boolean(false)), sort: vec![], limit: MAX_COLLECTION, include_archived: true });
        let changed_rows_before = decode_program(changed_rows_source);
        let changed_rows_after = decode_program(changed_rows);
        assert!(matches!(decoded_rule_projections(AppRef::Decoded(changed_rows_before.app()), AppRef::Decoded(changed_rows_after.app()), "job", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(message))) if message.contains("row selection/identity changed")));

        let truncated_count = decode_program(truncated_count_source);
        assert!(matches!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(truncated_count.app()), "job", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(message))) if message.contains("root count must be untruncated and unordered")));
        let non_boolean = decode_program(non_boolean_source);
        assert!(matches!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(non_boolean.app()), "job", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(message))) if message.contains("added column has no nullable Boolean availability rule")));
        let changed_type = decode_program(changed_type_source);
        assert!(matches!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(changed_type.app()), "job", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(message))) if message.contains("changed projection type is unsupported")));
        let changed_header = decode_program(changed_header_source);
        assert!(matches!(decoded_rule_projections(AppRef::Decoded(before.app()), AppRef::Decoded(changed_header.app()), "job", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(message))) if message.contains("count header changed")));
    }
    fn stable_root_list() -> crate::product_contract::ViewKind {
        crate::product_contract::ViewKind::List {
            entity: "job".into(),
            rows: Expr::Query { entity: "job".into(), binding: "r".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: true },
            columns: vec![crate::product_contract::Column { id: "done".into(), label: "Done".into(), value: Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "done".into() } }],
            controls: vec![], selection: None,
        }
    }

    #[test]
    fn decoded_rule_observations_accepts_stable_root_list_and_retries_allocation_refusal() {
        let app = app_with_view(stable_root_list());
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let structure = empty_structure("job");
        let mut meter = proof_meter();
        let mut rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
        let mut successful = AllocationContext::new();
        rules.observations(&mut successful).unwrap();
        let expected_certificates = rules.certificates().len();
        let growths = successful.growth_requests();
        assert_eq!(expected_certificates, 1);
        for nth in 1..=growths {
            let mut meter = proof_meter();
            let mut rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
            let mut cx = AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(nth).unwrap(), true);
            assert!(matches!(rules.observations(&mut cx), Err(CommonFactInventoryFailure::Allocation(_))), "growth {nth} must preserve its allocation refusal");
        }
        let mut meter = proof_meter();
        let mut retry = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
        retry.observations(&mut AllocationContext::new()).unwrap();
        assert_eq!(retry.certificates().len(), expected_certificates);
    }

    #[test]
    fn decoded_rule_observations_preserves_detail_refusal_evidence() {
        let app = app_with_view(crate::product_contract::ViewKind::Detail { entity: "job".into(), record: Expr::Literal { value_type: Type::reference("job"), value: DataValue::Reference { entity: "job".into(), record: "record".into() } }, columns: vec![] });
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let structure = empty_structure("job");
        let mut meter = proof_meter();
        let mut rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
        assert!(matches!(rules.observations(&mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text == "Common factual admission: view: detail identity/history selection is unsupported"));
    }

    #[test]
    fn decoded_rule_actions_keeps_postcondition_refusal_and_allocation_retry() {
        let mut source = fixture::program(false).program;
        source.actions = vec![crate::product_contract::ActionDefinition {
            id: "postcondition".into(), label: "Postcondition".into(), parameters: std::collections::BTreeMap::new(),
            guards: vec![], steps: vec![crate::product_contract::Statement::Assert { condition: boolean(true), message: "ok".into() }], ensures: vec![boolean(true)],
        }];
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut decode_cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut decode_cx).unwrap();
        let app = crate::product_contract::decode_captured_program_fallible(value, &mut decode_cx).unwrap();
        let before = serde_json::to_value(&app.app().actions).unwrap();
        let structure = empty_structure("job");
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter).with_structure(&structure);

        let error = rules.actions(&mut AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(1).unwrap(), true)).unwrap_err();
        assert!(matches!(error, CommonFactInventoryFailure::Allocation(_)));
        let retry = rules.actions(&mut AllocationContext::new()).unwrap_err();
        assert!(matches!(retry, CommonFactInventoryFailure::Store(StoreError::Invalid(text)) if text == "Common factual admission: postcondition: action postconditions are outside closed total admission grammar"));
        assert_eq!(serde_json::to_value(&app.app().actions).unwrap(), before);
    }

    #[test]
    fn decoded_rule_actions_matches_variant_spans_by_action_and_statement_path() {
        let (app, kinds, policy) = setup();
        let digest = || Digest::from_owned_lower_hex("0000000000000000000000000000000000000000000000000000000000000000".into()).unwrap();
        let endpoint = || DecodedOwnedSpanEndpoint { source: digest(), action: "a".into(), list_path: vec![0], start: 2, len: 1, kinds: vec![OwnedStatementKind::Update], digest: digest(), previous: None, next: None, root: None };
        let structure = DecodedOwnedStructuralReport { before: digest(), after: digest(), root_entity: "job".into(), edits: vec![super::super::fallible_owned_source_decode::DecodedOwnedSpanEdit { id: "edit".into(), before: endpoint(), after: endpoint() }], common_statements: vec![], writers: vec![], changes: vec![] };
        let mut meter = proof_meter();
        let rules = empty_rules(&app, &kinds, &policy, &mut meter).with_structure(&structure);
        assert!(rules.variant("a", &[0, 2]).is_some());
        assert!(rules.variant("a", &[0, 1]).is_none());
        assert!(rules.variant("a", &[1, 2]).is_none());
        assert!(rules.variant("b", &[0, 2]).is_none());
    }

    #[test]
    fn decoded_rule_actions_classifies_common_writers_and_shared_updates() {
        let (app, _, policy) = setup();
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root), ("shared".into(), DecodedOwnedSchemaKind::Registry { key: "name".into() })]);
        let writer = DecodedOwnedWriter { side: OwnedSourceSide::Before, source: Digest::from_owned_lower_hex("0000000000000000000000000000000000000000000000000000000000000000".into()).unwrap(), action: "shared".into(), path: vec![0], kind: OwnedStatementKind::Update, entity: "shared".into(), field: Some("secret".into()), value_type: Some(Type::Boolean) };
        let mut meter = proof_meter();
        let writers = [writer];
        let rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &writers, false, &mut meter);
        assert!(rules.mixed("shared"));
        assert!(rules.shared_update("shared"));
        assert!(!rules.mixed("other"));
    }

    #[test]
    fn decoded_rule_actions_accepts_unchanged_root_update_only_under_unfinished_guard() {
        let mut source = fixture::program(false).program;
        let root = Expr::Variable { name: "root".into() };
        let done = Expr::Field { record: Box::new(root.clone()), field: "done".into() };
        source.actions = vec![crate::product_contract::ActionDefinition {
            id: "finish".into(), label: "Finish".into(), parameters: [("root".into(), Type::reference("job"))].into_iter().collect(),
            guards: vec![Expr::Not { value: Box::new(Expr::Equal { left: Box::new(done.clone()), right: Box::new(boolean(true)) }) }],
            steps: vec![crate::product_contract::Statement::Update { record: root, values: [("done".into(), boolean(false))].into_iter().collect() }], ensures: vec![],
        }];
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut decode_cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut decode_cx).unwrap();
        let app = crate::product_contract::decode_captured_program_fallible(value, &mut decode_cx).unwrap();
        let structure = empty_structure("job");
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let mut meter = proof_meter();
        let mut rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
        rules.actions(&mut AllocationContext::new()).unwrap();
        assert_eq!(rules.certificates().len(), 2);
    }

    #[test]
    fn decoded_rule_actions_refuses_archive_delete_and_preserves_decoded_action() {
        let mut source = fixture::program(false).program;
        source.actions = vec![crate::product_contract::ActionDefinition {
            id: "archive".into(), label: "Archive".into(), parameters: [("root".into(), Type::reference("job"))].into_iter().collect(),
            guards: vec![], steps: vec![crate::product_contract::Statement::Archive { record: Expr::Variable { name: "root".into() } }], ensures: vec![],
        }];
        let captured = fixture::capture(serde_json::to_value(source).unwrap());
        let mut decode_cx = AllocationContext::new();
        let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut decode_cx).unwrap();
        let app = crate::product_contract::decode_captured_program_fallible(value, &mut decode_cx).unwrap();
        let before = serde_json::to_value(&app.app().actions).unwrap();
        let structure = empty_structure("job");
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let mut meter = proof_meter();
        let mut rules = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], true, &mut meter).with_structure(&structure);
        let error = rules.actions(&mut AllocationContext::new()).unwrap_err();
        assert!(matches!(error, CommonFactInventoryFailure::Store(StoreError::Invalid(text)) if text == "Common factual admission: archive [0]: archive/collection/output effect has no closed preview rule"));
        assert_eq!(serde_json::to_value(&app.app().actions).unwrap(), before);
    }

    #[test]
    fn decoded_rule_actions_only_accepts_empty_common_final_state_reset_without_transition_evidence() {
        let decode = |value: &str| {
            let mut source = fixture::program(false).program;
            source.state.push(crate::product_contract::StateDefinition { id: "input".into(), label: "Input".into(), value_type: Type::Text, initial: DataValue::Text { value: String::new() } });
            source.actions = vec![crate::product_contract::ActionDefinition {
                id: "reset".into(), label: "Reset".into(), parameters: Default::default(), guards: vec![],
                steps: vec![crate::product_contract::Statement::SetState { state: "input".into(), value: Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: value.into() } } }], ensures: vec![],
            }];
            let captured = fixture::capture(serde_json::to_value(source).unwrap());
            let mut cx = AllocationContext::new();
            let value = crate::product_json::parse_json_bytes(&serde_json::to_vec(&captured).unwrap(), &mut cx).unwrap();
            crate::product_contract::decode_captured_program_fallible(value, &mut cx).unwrap()
        };
        let kinds = DecodedOwnedSchemaKinds::test_from([("job".into(), DecodedOwnedSchemaKind::Root)]);
        let policy = OrderedSet::new();
        let structure = empty_structure("job");
        let empty = decode("");
        let mut empty_meter = proof_meter();
        let mut empty_rules = DecodedCommonFactRules::new(AppRef::Decoded(empty.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], false, &mut empty_meter).with_structure(&structure);
        empty_rules.actions(&mut AllocationContext::new()).unwrap();
        assert_eq!(empty_rules.certificates()[0].rule, "closed input reset input");

        let marker = decode("retained-marker");
        let mut marker_meter = proof_meter();
        let mut marker_rules = DecodedCommonFactRules::new(AppRef::Decoded(marker.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &[], false, &mut marker_meter).with_structure(&structure);
        assert!(matches!(marker_rules.actions(&mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text == "Common factual admission: reset [0]: input reset must be literal empty, common, top-level and final"));
    }

    #[test]
    fn decoded_rule_expr_accepts_nested_query_filter_and_refuses_binding_type_errors() {
        let (app, kinds, policy) = setup();
        let query = Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false };
        let nested = Expr::Filter { items: Box::new(query), binding: "item".into(), predicate: Box::new(Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "item".into() }), field: "done".into() }), right: Box::new(boolean(true)) }) };
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter);
        let value = rules.expression(&nested, &DecodedRuleEnvironment::new(), false, false, "query", &mut AllocationContext::new()).unwrap();
        assert_eq!(value.ty, Type::List { item: Box::new(Type::reference("job")) });

        let malformed = Expr::Filter { items: Box::new(Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false }), binding: "item".into(), predicate: Box::new(integer(1)) };
        let error = rules.expression(&malformed, &DecodedRuleEnvironment::new(), false, false, "query", &mut AllocationContext::new()).err().unwrap();
        assert!(matches!(error, CommonFactInventoryFailure::Store(StoreError::Invalid(text)) if text.contains("expected Boolean")));
    }

    #[test]
    fn decoded_rule_expr_preserves_arithmetic_refusals_and_map_display_boundary() {
        let (app, kinds, policy) = setup();
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter);
        let sum = Expr::Add { left: Box::new(integer(1)), right: Box::new(integer(2)) };
        assert!(rules.expression(&sum, &DecodedRuleEnvironment::new(), false, false, "sum", &mut AllocationContext::new()).is_ok());
        let overflow = Expr::Add { left: Box::new(integer(i64::MAX)), right: Box::new(integer(1)) };
        assert!(matches!(rules.expression(&overflow, &DecodedRuleEnvironment::new(), false, false, "sum", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("integer addition is not total")));
        let map = Expr::Map { items: Box::new(Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false }), binding: "item".into(), value: Box::new(boolean(true)) };
        assert!(matches!(rules.expression(&map, &DecodedRuleEnvironment::new(), false, false, "map", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("business map/order fold is unsupported")));
        assert!(rules.expression(&map, &DecodedRuleEnvironment::new(), false, true, "map", &mut AllocationContext::new()).is_ok());
    }

    #[test]
    fn decoded_rule_expr_tracks_frame_session_collection_and_total_operator_families() {
        let (app, kinds, policy) = setup();
        let env = root_env();
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter);
        let today = rules.expression(&Expr::Today, &env, false, false, "today", &mut AllocationContext::new()).unwrap();
        assert!(today.frame_dependent && !today.session_dependent);
        let state = rules.expression(&Expr::State { state: "input".into() }, &env, false, false, "state", &mut AllocationContext::new()).unwrap();
        assert!(state.frame_dependent && state.session_dependent);

        let query = Expr::Query { entity: "job".into(), binding: "row".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false };
        let count = Expr::Count { items: Box::new(query.clone()) };
        assert_eq!(rules.expression(&count, &env, false, false, "count", &mut AllocationContext::new()).unwrap().ty, Type::Integer);
        for expression in [
            Expr::Any { items: Box::new(query.clone()), binding: "row".into(), predicate: Box::new(boolean(true)) },
            Expr::All { items: Box::new(query.clone()), binding: "row".into(), predicate: Box::new(boolean(true)) },
            Expr::Contains { items: Box::new(query.clone()), value: Box::new(Expr::Variable { name: "root".into() }) },
            Expr::Less { left: Box::new(integer(1)), right: Box::new(integer(2)) },
            Expr::Not { value: Box::new(boolean(false)) },
            Expr::If { condition: Box::new(boolean(true)), then_value: Box::new(integer(1)), else_value: Box::new(integer(2)) },
            Expr::Coalesce { value: Box::new(Expr::Literal { value_type: Type::Optional { item: Box::new(Type::Integer) }, value: DataValue::Null }), fallback: Box::new(integer(2)) },
            Expr::Subtract { left: Box::new(integer(3)), right: Box::new(integer(2)) },
        ] {
            assert!(rules.expression(&expression, &env, false, false, "operator", &mut AllocationContext::new()).is_ok(), "{expression:?}");
        }
        let integer_map = Expr::Map { items: Box::new(query), binding: "row".into(), value: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "row".into() }), field: "production".into() }) };
        let partial = Expr::Sum { items: Box::new(integer_map) };
        assert!(matches!(rules.expression(&partial, &env, false, false, "sum", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("partial date/arithmetic/text operation")));
    }

    #[test]
    fn decoded_rule_expr_allocation_refusal_retries_without_mutating_expression() {
        let (app, kinds, policy) = setup();
        let lowered = Expr::Lower { value: Box::new(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "Straße".into() } }) };
        let expr = Expr::And { values: vec![boolean(true), Expr::Equal { left: Box::new(lowered), right: Box::new(Expr::Literal { value_type: Type::Text, value: DataValue::Text { value: "strasse".into() } }) }] };
        let before = serde_json::to_value(&expr).unwrap();
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter);
        let mut successful = AllocationContext::new();
        let expected = rules.expression(&expr, &DecodedRuleEnvironment::new(), false, false, "expr", &mut successful).unwrap();
        let growths = successful.growth_requests();
        let mut failures = 0;
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(std::num::NonZeroUsize::new(nth).unwrap(), true);
            let error = rules.expression(&expr, &DecodedRuleEnvironment::new(), false, false, "expr", &mut cx).err().unwrap();
            assert!(matches!(error, CommonFactInventoryFailure::Allocation(_)), "growth {nth} must retain its allocation refusal cause");
            failures += 1;
        }
        assert!(failures > 0);
        let retried = rules.expression(&expr, &DecodedRuleEnvironment::new(), false, false, "expr", &mut AllocationContext::new()).unwrap();
        assert_eq!(retried.ty, expected.ty);
        assert_eq!(serde_json::to_value(&expr).unwrap(), before);
    }

    #[test]
    fn decoded_rule_expr_preserves_query_correlation_lifecycle_facets_and_policy_taint() {
        let (app, kinds, mut policy) = setup();
        policy.try_insert(("requirement".into(), "secret".into()), &mut AllocationContext::new()).unwrap();
        let env = root_env();
        let mut meter = proof_meter();
        let mut rules = empty_rules(&app, &kinds, &policy, &mut meter);

        let facet = Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "root".into() }), field: "done".into() }), right: Box::new(boolean(true)) };
        let lifecycle_syntax = Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "record".into() }), field: "done".into() }), right: Box::new(boolean(true)) };
        assert_eq!(lifecycle_criteria(&lifecycle_syntax, &mut AllocationContext::new()).unwrap(), ("done", &DataValue::Boolean { value: true }));
        let value = rules.expression(&facet, &env, true, false, "lifecycle", &mut AllocationContext::new()).unwrap();
        assert!(value.facets.contains("argument/root"));
        assert_eq!(rules.certificates().len(), 1);
        let unfinished = Expr::And { values: vec![Expr::Not { value: Box::new(facet.clone()) }, Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "root".into() }), field: "done".into() }), right: Box::new(boolean(false)) }] };
        assert!(rules.unfinished(&unfinished, &env, &mut AllocationContext::new()).unwrap().contains("argument/root"));

        let correlated = Expr::Query {
            entity: "requirement".into(), binding: "link".into(),
            predicate: Box::new(Expr::And { values: vec![
                Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "link".into() }), field: "job".into() }), right: Box::new(Expr::Variable { name: "root".into() }) },
                Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "link".into() }), field: "secret".into() }), right: Box::new(boolean(true)) },
            ] }), sort: vec![], limit: MAX_COLLECTION, include_archived: false,
        };
        let selection = rules.expression(&correlated, &env, false, false, "aux", &mut AllocationContext::new()).unwrap();
        assert!(selection.policy.contains("argument/root"));
        assert_eq!(selection.root.as_deref(), Some("argument/root"));
        let truncated = Expr::Query { entity: "requirement".into(), binding: "link".into(), predicate: Box::new(Expr::And { values: vec![
            Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "link".into() }), field: "job".into() }), right: Box::new(Expr::Variable { name: "root".into() }) },
            Expr::Equal { left: Box::new(Expr::Field { record: Box::new(Expr::Variable { name: "link".into() }), field: "secret".into() }), right: Box::new(boolean(true)) },
        ] }), sort: vec![], limit: MAX_COLLECTION - 1, include_archived: false };
        assert!(matches!(rules.expression(&truncated, &env, false, false, "aux", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("policy selection is cross-root or truncated")));

        let uncorrelated = Expr::Query { entity: "requirement".into(), binding: "link".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false };
        assert!(matches!(rules.expression(&uncorrelated, &env, false, false, "aux", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("auxiliary membership lacks exact owner correlation")));
    }

    #[test]
    fn decoded_rule_expr_keeps_terminal_history_and_exact_append_count_boundaries() {
        let (app, kinds, policy) = setup();
        let mut meter = proof_meter();
        let mut no_writer = empty_rules(&app, &kinds, &policy, &mut meter);
        let mut history_env = DecodedRuleEnvironment::new();
        let mut history = DecodedRuleValue::typed(Type::reference("history"));
        history.root = Some("argument/root".into());
        history_env.insert("history", history, &mut AllocationContext::new()).unwrap();
        assert!(matches!(no_writer.expression(&Expr::Field { record: Box::new(Expr::Variable { name: "history".into() }), field: "sequence".into() }, &history_env, false, false, "business", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("terminal history field has a business consumer")));
        let archived_query = Expr::Query { entity: "history".into(), binding: "old".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: true };
        assert!(matches!(no_writer.expression(&archived_query, &history_env, false, true, "history", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("terminal history query")));

        let bookkeeping = Expr::Add {
            left: Box::new(Expr::Count { items: Box::new(Expr::Query { entity: "history".into(), binding: "old".into(), predicate: Box::new(boolean(true)), sort: vec![], limit: MAX_COLLECTION, include_archived: false }) }),
            right: Box::new(integer(1)),
        };
        assert!(matches!(no_writer.expression(&bookkeeping, &history_env, false, false, "append [].sequence", &mut AllocationContext::new()), Err(CommonFactInventoryFailure::Store(StoreError::Invalid(text))) if text.contains("outside exact append bookkeeping site")));

        let writer = DecodedOwnedWriter { side: OwnedSourceSide::Before, source: Digest::from_owned_lower_hex("0000000000000000000000000000000000000000000000000000000000000000".into()).unwrap(), action: "append".into(), path: vec![], kind: OwnedStatementKind::Create, entity: "history".into(), field: Some("sequence".into()), value_type: Some(Type::Integer) };
        let mut writer_meter = proof_meter();
        let writers = [writer];
        let mut authorized = DecodedCommonFactRules::new(AppRef::Decoded(app.app()), &kinds, "job", "done", DataValue::Boolean { value: true }, OwnedSourceSide::Before, &policy, &writers, false, &mut writer_meter);
        assert!(authorized.expression(&bookkeeping, &history_env, false, false, "append [].sequence", &mut AllocationContext::new()).is_ok());
        assert_eq!(authorized.certificates().len(), 1);
    }
}
