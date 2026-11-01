//! Unrouted managed environment snapshots (R07a).
//!
//! Only closed String and managed Type/DataValue owners enter this map. This
//! helper performs no validation, metering, cancellation polling or error
//! rendering, and adds no input limits. Caller inputs are borrowed; their own
//! teardown remains the caller's responsibility. No mutable/raw owner escape.
use crate::product_alloc::{ordered::OrderedMap, try_append_str, try_copy_str, AllocationContext,
    AllocationFailure, AllocationSite};
use crate::product_app::OwnedAppGraph;
use crate::product_contract::{DataValue, Type};

struct Binding {
    ty: OwnedAppGraph<Type>,
    value: OwnedAppGraph<DataValue>,
}
impl Binding {
    fn try_new(ty: &Type, value: &DataValue, cx: &mut AllocationContext)
        -> Result<Self, AllocationFailure> {
        // The first root is already guarded if copying the second fails.
        let ty = OwnedAppGraph::<Type>::try_copy(ty, cx)?;
        let value = OwnedAppGraph::<DataValue>::try_copy(value, cx)?;
        Ok(Self { ty, value })
    }
    fn try_copy(&self, cx: &mut AllocationContext) -> Result<Self, AllocationFailure> {
        Self::try_new(self.ty.as_ref(), self.value.as_ref(), cx)
    }
    fn try_reference(type_entity: &str, value_entity: &str, record: &str, cx: &mut AllocationContext)
        -> Result<Self, AllocationFailure> {
        let mut owned_type_entity = String::new();
        try_append_str(&mut owned_type_entity, type_entity, cx, AllocationSite::String)?;
        let ty = Type::Reference { entity: owned_type_entity };
        let ty = OwnedAppGraph::<Type>::try_copy(&ty, cx)?;

        let mut owned_value_entity = String::new();
        try_append_str(&mut owned_value_entity, value_entity, cx, AllocationSite::String)?;
        let mut value_record = String::new();
        try_append_str(&mut value_record, record, cx, AllocationSite::String)?;
        let value = DataValue::Reference { entity: owned_value_entity, record: value_record };
        let value = OwnedAppGraph::<DataValue>::try_copy(&value, cx)?;
        Ok(Self { ty, value })
    }
    fn get(&self) -> (&Type, &DataValue) { (self.ty.as_ref(), self.value.as_ref()) }
}

// Default field Drop is closed: the map iterates bindings, and each managed
// root dismantles its finite token graph using its retained workspace. Neither
// a failed partial snapshot nor replacement/final teardown needs allocation or
// a call stack proportional to graph depth.
pub(super) struct ManagedEnv { bindings: OrderedMap<String, Binding> }
impl ManagedEnv {
    pub(super) const fn new() -> Self { Self { bindings: OrderedMap::new() } }
    pub(super) fn len(&self) -> usize { self.bindings.len() }
    pub(super) fn get(&self, name: &str) -> Option<(&Type, &DataValue)> {
        self.bindings.get(name).map(Binding::get)
    }
    /// Stage key, Type and value in that order, then install. Any refusal before
    /// installation preserves the original map's contents and capacity. Equal
    /// keys retain the original key. No fallible work follows this frontier.
    pub(super) fn try_bind(&mut self, name: &str, ty: &Type, value: &DataValue,
        cx: &mut AllocationContext) -> Result<(), AllocationFailure> {
        let key = try_copy_str(name, cx, AllocationSite::String)?;
        let binding = Binding::try_new(ty, value, cx)?;
        let old = self.bindings.try_insert(key, binding, cx)?;
        drop(old);
        Ok(())
    }
    pub(super) fn try_bind_reference(
        &mut self,
        name: &str,
        type_entity: &str,
        value_entity: &str,
        record: &str,
        cx: &mut AllocationContext,
    ) -> Result<(), AllocationFailure> {
        let key = try_copy_str(name, cx, AllocationSite::String)?;
        let binding = Binding::try_reference(type_entity, value_entity, record, cx)?;
        let old = self.bindings.try_insert(key, binding, cx)?;
        drop(old);
        Ok(())
    }
    /// Independent snapshot: reserve map storage first, then copy entries in
    /// String order. These callbacks are fixed closed operations, never caller
    /// callbacks; all copied prefixes contain managed bindings only.
    pub(super) fn try_copy(&self, cx: &mut AllocationContext)
        -> Result<Self, AllocationFailure> {
        let bindings = self.bindings.try_clone_with(cx,
            |key, cx| try_copy_str(key, cx, AllocationSite::String), Binding::try_copy)?;
        Ok(Self { bindings })
    }
    pub(super) fn try_from_iter<'a>(
        entries: impl IntoIterator<Item = (&'a str, &'a Type, &'a DataValue)>,
        cx: &mut AllocationContext,
    ) -> Result<Self, AllocationFailure> {
        let mut env = Self::new();
        for (key, ty, value) in entries {
            env.try_bind(key, ty, value, cx)?;
        }
        Ok(env)
    }
    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize { self.bindings.capacity() }
    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &Type, &DataValue)> {
        self.bindings.iter().map(|(key, binding)| {
            let (ty, value) = binding.get(); (key.as_str(), ty, value)
        })
    }
}
