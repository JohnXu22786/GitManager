//! Allocation-aware expression semantics for decoded common-fact rules.
//!
//! This is kept separate from the legacy verifier until the decoded verifier
//! is assembled. It consumes decoded app/schema owners and never converts
//! them back to legacy maps.
use super::common_fact_admission::ProofMeter;
use super::fallible_common_fact_inventory::CommonFactInventoryFailure;
use super::fallible_common_fact_schemas::{DecodedOwnedSchemaKind, DecodedOwnedSchemaKinds};
use super::fallible_owned_source_decode::DecodedOwnedWriter;
use super::owned_contract::{OwnedSourceSide, OwnedStatementKind};
use crate::product_alloc::{self, ordered::{OrderedMap, OrderedSet}, AllocationContext, AllocationSite};
use crate::product_contract::{self, AppRef, DataValue, Digest, Expr, IdentityDomain, StringValueMap, Type, MAX_COLLECTION, MAX_TEXT_BYTES};
use crate::product_store::StoreError;
use serde::Serialize;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct DecodedCommonFactCertificate {
    pub(crate) side: OwnedSourceSide,
    pub(crate) site: String,
    pub(crate) rule: String,
    pub(crate) identity: Digest,
}

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
    all_roots_active: bool,
    meter: &'a mut ProofMeter,
    certificates: Vec<DecodedCommonFactCertificate>,
}
impl<'a> DecodedCommonFactRules<'a> {
    pub(crate) fn new(
        app: AppRef<'a>, kinds: &'a DecodedOwnedSchemaKinds, root_entity: &'a str,
        lifecycle_field: &'a str, lifecycle_value: DataValue,
        side: OwnedSourceSide, policy: &'a OrderedSet<(String, String)>, writers: &'a [DecodedOwnedWriter],
        all_roots_active: bool, meter: &'a mut ProofMeter,
    ) -> Self {
        Self { app, kinds, root_entity, lifecycle_field, lifecycle_value,
            side, policy, writers, all_roots_active, meter, certificates: Vec::new() }
    }
    pub(crate) fn certificates(&self) -> &[DecodedCommonFactCertificate] { &self.certificates }
    pub(crate) fn environment(&self, parameters: &impl StringValueMap<Type>, prefix: &str, cx: &mut AllocationContext) -> Result<DecodedRuleEnvironment, CommonFactInventoryFailure> {
        let mut environment = DecodedRuleEnvironment::new();
        for (name, ty) in parameters.iter() {
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
}

fn app_error(value: product_contract::AppValidationError) -> CommonFactInventoryFailure { match value { product_contract::AppValidationError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), product_contract::AppValidationError::Contract(v) => CommonFactInventoryFailure::Store(v.into()) } }
fn codec_error(value: crate::product_backup::fallible_codec::CodecError) -> CommonFactInventoryFailure { match value { crate::product_backup::fallible_codec::CodecError::Allocation(v) => CommonFactInventoryFailure::Allocation(v), _ => CommonFactInventoryFailure::Store(StoreError::Invalid("Common factual admission: identity encoding invariant".into())) } }
fn copy_string(value: &str, cx: &mut AllocationContext) -> Result<String, CommonFactInventoryFailure> { product_alloc::try_copy_str(value, cx, AllocationSite::String).map_err(Into::into) }
fn copy_type(value: &Type, cx: &mut AllocationContext) -> Result<Type, CommonFactInventoryFailure> { product_contract::try_clone_type_with(value, cx).map_err(Into::into) }
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
    use crate::product_store::scope::common_fact_admission::{ProofBudget, ProofMeter};
    use std::sync::{atomic::AtomicBool, Arc};

    fn app() -> crate::product_contract::FallibleDecodedCapturedProgram {
        let mut source = crate::fixture::program(false).program;
        source.entities.push(serde_json::from_value(serde_json::json!({"id":"requirement","label":"Requirement","fields":[{"id":"job","label":"Job","value_type":{"kind":"reference","entity":"job"}},{"id":"secret","label":"Secret","value_type":{"kind":"boolean"}}],"unique":[["job"]],"constraints":[]})).unwrap());
        source.entities.push(serde_json::from_value(serde_json::json!({"id":"history","label":"History","fields":[{"id":"job","label":"Job","value_type":{"kind":"reference","entity":"job"}},{"id":"sequence","label":"Sequence","value_type":{"kind":"integer"}},{"id":"archived","label":"Archived","value_type":{"kind":"boolean"}}],"unique":[["job","sequence"]],"constraints":[]})).unwrap());
        source.state.push(crate::product_contract::StateDefinition { id: "input".into(), label: "Input".into(), value_type: Type::Text, initial: DataValue::Text { value: String::new() } });
        let captured = crate::fixture::capture(serde_json::to_value(source).unwrap());
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
