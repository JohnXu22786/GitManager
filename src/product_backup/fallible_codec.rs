//! Private, bounded serialization bridge for the modern backup graph.
//!
//! This is intentionally not a general replacement for `canonical_bytes`.
//! Only the closed backup payload/envelope types implement `BackupWire`.

use crate::product_alloc::{
    self, ordered::OrderedMap, AllocationContext, AllocationFailure, AllocationSite,
};
use crate::product_contract::{Digest, IdentityDomain};
use crate::product_json::{self, Number, Value};
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use serde::Serialize;
use std::fmt;

const MAX_DEPTH: usize = 127;
const MAX_SERIALIZED_BYTES: usize = super::MAX_BACKUP_BYTES;

#[path = "../product_digest.rs"]
mod digest_impl;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CodecError {
    Allocation(AllocationFailure),
    DepthLimit,
    InvalidNumber,
    InvalidMapKey,
    DuplicateMapKey,
    OutputLimitExceeded,
    SerializationInvariant,
    Digest(digest_impl::DigestError),
    DigestText,
}
impl From<AllocationFailure> for CodecError {
    fn from(value: AllocationFailure) -> Self {
        Self::Allocation(value)
    }
}
impl From<product_json::canonical::EncodeError> for CodecError {
    fn from(value: product_json::canonical::EncodeError) -> Self {
        match value {
            product_json::canonical::EncodeError::Allocation(failure) => Self::Allocation(failure),
            product_json::canonical::EncodeError::OutputLimitExceeded => Self::OutputLimitExceeded,
            _ => Self::SerializationInvariant,
        }
    }
}
impl ser::Error for CodecError {
    fn custom<T: fmt::Display>(_: T) -> Self {
        Self::SerializationInvariant
    }
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Allocation(_) => "backup codec allocation failure",
            Self::DepthLimit => "backup codec nesting limit",
            Self::InvalidNumber => "backup codec number out of range",
            Self::InvalidMapKey => "backup codec map key is not a string",
            Self::DuplicateMapKey => "backup codec duplicate map key",
            Self::OutputLimitExceeded => "backup codec output limit",
            Self::SerializationInvariant => "backup codec serialization invariant",
            Self::Digest(_) => "backup codec digest failure",
            Self::DigestText => "backup codec digest text invariant",
        })
    }
}
impl std::error::Error for CodecError {}

/// Sealed by module privacy: the only callers are the modern backup graph.
pub(crate) trait BackupWire: Serialize {}
impl BackupWire for super::Payload {}
impl BackupWire for super::Envelope {}
impl BackupWire for crate::product_contract::FallibleDecodedDataSnapshot {}

pub(crate) fn canonical_bytes<T: BackupWire + ?Sized>(
    value: &T,
    cx: &mut AllocationContext,
) -> Result<Vec<u8>, CodecError> {
    let value = try_serialize_value(value, cx)?;
    product_json::canonical::try_canonical_bytes_bounded(&value, MAX_SERIALIZED_BYTES, cx)
        .map_err(Into::into)
}

/// Build the same closed, allocation-aware JSON value used by the canonical
/// writer. This stays inside the sealed backup graph and is used by typed
/// fallible owner copies, which decode the returned tree with their domain
/// decoder rather than converting to a legacy model.
pub(crate) fn try_serialize_value<T: BackupWire + ?Sized>(
    value: &T,
    cx: &mut AllocationContext,
) -> Result<Value, CodecError> {
    value.serialize(ValueSerializer { cx, depth: 0 })
}

pub(crate) fn canonical_digest<T: BackupWire + ?Sized>(
    domain: IdentityDomain,
    value: &T,
    cx: &mut AllocationContext,
) -> Result<Digest, CodecError> {
    let bytes = canonical_bytes(value, cx)?;
    let digest = digest_impl::try_bytes_digest(domain, &bytes).map_err(CodecError::Digest)?;
    let text = digest.try_hex_string(cx)?;
    Digest::from_owned_lower_hex(text).ok_or(CodecError::DigestText)
}

#[cfg(test)]
pub(super) fn test_full_backup_oracles(backup: &super::VerifiedBackup) -> usize {
    use std::num::NonZeroUsize;

    fn check_bytes<T: BackupWire>(value: &T) -> usize {
        let expected = super::canonical_bytes(value).unwrap();
        let mut cx = AllocationContext::new();
        let actual = canonical_bytes(value, &mut cx).unwrap();
        assert_eq!(actual, expected);
        let growths = cx.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let result = canonical_bytes(value, &mut cx);
            match result {
                Err(CodecError::Allocation(failure)) => assert_eq!(cx.first_failure(), Some(failure)),
                other => panic!("refusal {nth}/{growths}: {other:?}"),
            }
        }
        growths
    }
    fn check_digest<T: BackupWire>(value: &T) -> usize {
        let expected = super::canonical_digest(IdentityDomain::Evidence, value).unwrap();
        let mut cx = AllocationContext::new();
        let actual = canonical_digest(IdentityDomain::Evidence, value, &mut cx).unwrap();
        assert_eq!(actual, expected);
        let growths = cx.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            let result = canonical_digest(IdentityDomain::Evidence, value, &mut cx);
            match result {
                Err(CodecError::Allocation(failure)) => assert_eq!(cx.first_failure(), Some(failure)),
                other => panic!("digest refusal {nth}/{growths}: {other:?}"),
            }
        }
        growths
    }
    let bytes = check_bytes(&backup.envelope);
    let payload = check_digest(&backup.envelope.payload);
    let envelope = check_digest(&backup.envelope);
    bytes + payload + envelope
}

struct ValueSerializer<'a> {
    cx: &'a mut AllocationContext,
    depth: usize,
}
impl ValueSerializer<'_> {
    fn child(&self) -> Result<usize, CodecError> {
        self.descendant(1)
    }
    fn descendant(&self, levels: usize) -> Result<usize, CodecError> {
        let depth = self.depth.checked_add(levels).ok_or(CodecError::DepthLimit)?;
        if depth > MAX_DEPTH {
            return Err(CodecError::DepthLimit);
        }
        Ok(depth)
    }
}

enum CompoundKind {
    Sequence(Vec<Value>),
    Map(OrderedMap<String, Value>),
    TupleVariant { name: &'static str, values: Vec<Value> },
    StructVariant { name: &'static str, values: OrderedMap<String, Value> },
}
struct Compound<'a> {
    cx: &'a mut AllocationContext,
    child_depth: usize,
    kind: CompoundKind,
    pending_key: Option<String>,
}
impl<'a> Compound<'a> {
    fn sequence(cx: &'a mut AllocationContext, child_depth: usize, capacity: usize) -> Result<Self, CodecError> {
        let mut values = Vec::new();
        product_alloc::try_reserve(&mut values, capacity, cx, AllocationSite::Vector)?;
        Ok(Self { cx, child_depth, kind: CompoundKind::Sequence(values), pending_key: None })
    }
    fn map(cx: &'a mut AllocationContext, child_depth: usize, capacity: usize) -> Result<Self, CodecError> {
        let mut values = OrderedMap::new();
        values.try_reserve(capacity, cx)?;
        Ok(Self { cx, child_depth, kind: CompoundKind::Map(values), pending_key: None })
    }
    fn tuple_variant(cx: &'a mut AllocationContext, child_depth: usize, name: &'static str, capacity: usize) -> Result<Self, CodecError> {
        let mut values = Vec::new();
        product_alloc::try_reserve(&mut values, capacity, cx, AllocationSite::Vector)?;
        Ok(Self { cx, child_depth, kind: CompoundKind::TupleVariant { name, values }, pending_key: None })
    }
    fn struct_variant(cx: &'a mut AllocationContext, child_depth: usize, name: &'static str, capacity: usize) -> Result<Self, CodecError> {
        let mut values = OrderedMap::new();
        values.try_reserve(capacity, cx)?;
        Ok(Self { cx, child_depth, kind: CompoundKind::StructVariant { name, values }, pending_key: None })
    }
    fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<Value, CodecError> {
        value.serialize(ValueSerializer { cx: self.cx, depth: self.child_depth })
    }
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), CodecError> {
        let value = self.value(value)?;
        let (CompoundKind::Sequence(values) | CompoundKind::TupleVariant { values, .. }) = &mut self.kind else {
            return Err(CodecError::SerializationInvariant);
        };
        product_alloc::try_push(values, value, self.cx, AllocationSite::Vector)?;
        Ok(())
    }
    fn key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), CodecError> {
        if self.pending_key.is_some() {
            return Err(CodecError::SerializationInvariant);
        }
        let key = key.serialize(KeySerializer { cx: self.cx })?;
        self.pending_key = Some(key);
        Ok(())
    }
    fn pair<K: Serialize + ?Sized, V: Serialize + ?Sized>(&mut self, key: &K, value: &V) -> Result<(), CodecError> {
        self.key(key)?;
        self.map_value(value)
    }
    fn field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), CodecError> {
        self.pending_key = Some(product_alloc::try_copy_str(key, self.cx, AllocationSite::String)?);
        self.map_value(value)
    }
    fn map_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), CodecError> {
        let key = self.pending_key.take().ok_or(CodecError::SerializationInvariant)?;
        let value = self.value(value)?;
        let target = match &mut self.kind {
            CompoundKind::Map(values) | CompoundKind::StructVariant { values, .. } => values,
            _ => return Err(CodecError::SerializationInvariant),
        };
        if target.try_insert(key, value, self.cx)?.is_some() {
            return Err(CodecError::DuplicateMapKey);
        }
        Ok(())
    }
    fn finish(self) -> Result<Value, CodecError> {
        if self.pending_key.is_some() {
            return Err(CodecError::SerializationInvariant);
        }
        match self.kind {
            CompoundKind::Sequence(values) => Ok(Value::Array(values)),
            CompoundKind::Map(values) => Ok(Value::Object(values)),
            CompoundKind::TupleVariant { name, values } => {
                let mut payload = Value::Array(values);
                let mut outer = OrderedMap::new();
                let key = product_alloc::try_copy_str(name, self.cx, AllocationSite::String)?;
                outer.try_insert(key, payload, self.cx)?;
                payload = Value::Object(outer);
                Ok(payload)
            }
            CompoundKind::StructVariant { name, values } => {
                let mut outer = OrderedMap::new();
                let key = product_alloc::try_copy_str(name, self.cx, AllocationSite::String)?;
                outer.try_insert(key, Value::Object(values), self.cx)?;
                Ok(Value::Object(outer))
            }
        }
    }
}

impl<'a> serde::Serializer for ValueSerializer<'a> {
    type Ok = Value;
    type Error = CodecError;
    type SerializeSeq = Compound<'a>;
    type SerializeTuple = Compound<'a>;
    type SerializeTupleStruct = Compound<'a>;
    type SerializeTupleVariant = Compound<'a>;
    type SerializeMap = Compound<'a>;
    type SerializeStruct = Compound<'a>;
    type SerializeStructVariant = Compound<'a>;

    fn serialize_bool(self, value: bool) -> Result<Value, CodecError> { Ok(Value::Bool(value)) }
    fn serialize_i8(self, value: i8) -> Result<Value, CodecError> { self.serialize_i64(value.into()) }
    fn serialize_i16(self, value: i16) -> Result<Value, CodecError> { self.serialize_i64(value.into()) }
    fn serialize_i32(self, value: i32) -> Result<Value, CodecError> { self.serialize_i64(value.into()) }
    fn serialize_i64(self, value: i64) -> Result<Value, CodecError> { Ok(Value::Number(Number::I64(value))) }
    fn serialize_i128(self, value: i128) -> Result<Value, CodecError> {
        if let Ok(value) = i64::try_from(value) { self.serialize_i64(value) }
        else if let Ok(value) = u64::try_from(value) { self.serialize_u64(value) }
        else { Err(CodecError::InvalidNumber) }
    }
    fn serialize_u8(self, value: u8) -> Result<Value, CodecError> { self.serialize_u64(value.into()) }
    fn serialize_u16(self, value: u16) -> Result<Value, CodecError> { self.serialize_u64(value.into()) }
    fn serialize_u32(self, value: u32) -> Result<Value, CodecError> { self.serialize_u64(value.into()) }
    fn serialize_u64(self, value: u64) -> Result<Value, CodecError> { Ok(Value::Number(Number::U64(value))) }
    fn serialize_u128(self, value: u128) -> Result<Value, CodecError> {
        u64::try_from(value).map_err(|_| CodecError::InvalidNumber).and_then(|value| self.serialize_u64(value))
    }
    fn serialize_f32(self, value: f32) -> Result<Value, CodecError> { self.serialize_f64(value.into()) }
    fn serialize_f64(self, value: f64) -> Result<Value, CodecError> {
        if value.is_finite() { Ok(Value::Number(Number::F64(value))) } else { Ok(Value::Null) }
    }
    fn serialize_char(self, value: char) -> Result<Value, CodecError> {
        let mut bytes = [0; 4];
        self.serialize_str(value.encode_utf8(&mut bytes))
    }
    fn serialize_str(self, value: &str) -> Result<Value, CodecError> {
        Ok(Value::String(product_alloc::try_copy_str(value, self.cx, AllocationSite::String)?))
    }
    fn serialize_bytes(self, value: &[u8]) -> Result<Value, CodecError> {
        let _array_depth = self.child()?;
        let mut values = Vec::new();
        product_alloc::try_reserve(&mut values, value.len(), self.cx, AllocationSite::Vector)?;
        for byte in value { values.push(Value::Number(Number::U64((*byte).into()))); }
        Ok(Value::Array(values))
    }
    fn serialize_none(self) -> Result<Value, CodecError> { Ok(Value::Null) }
    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, CodecError> { value.serialize(self) }
    fn serialize_unit(self) -> Result<Value, CodecError> { Ok(Value::Null) }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Value, CodecError> { Ok(Value::Null) }
    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result<Value, CodecError> { self.serialize_str(variant) }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<Value, CodecError> { value.serialize(self) }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, variant: &'static str, value: &T) -> Result<Value, CodecError> {
        let depth = self.child()?;
        let mut values = OrderedMap::new();
        let key = product_alloc::try_copy_str(variant, self.cx, AllocationSite::String)?;
        let value = value.serialize(ValueSerializer { cx: self.cx, depth })?;
        values.try_insert(key, value, self.cx)?;
        Ok(Value::Object(values))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<Compound<'a>, CodecError> { let depth = self.child()?; Compound::sequence(self.cx, depth, len.unwrap_or(0)) }
    fn serialize_tuple(self, len: usize) -> Result<Compound<'a>, CodecError> { let depth = self.child()?; Compound::sequence(self.cx, depth, len) }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<Compound<'a>, CodecError> { let depth = self.child()?; Compound::sequence(self.cx, depth, len) }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, variant: &'static str, len: usize) -> Result<Compound<'a>, CodecError> { let inner_depth = self.descendant(2)?; Compound::tuple_variant(self.cx, inner_depth, variant, len) }
    fn serialize_map(self, len: Option<usize>) -> Result<Compound<'a>, CodecError> { let depth = self.child()?; Compound::map(self.cx, depth, len.unwrap_or(0)) }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<Compound<'a>, CodecError> { let depth = self.child()?; Compound::map(self.cx, depth, len) }
    fn serialize_struct_variant(self, _: &'static str, _: u32, variant: &'static str, len: usize) -> Result<Compound<'a>, CodecError> { let inner_depth = self.descendant(2)?; Compound::struct_variant(self.cx, inner_depth, variant, len) }
    fn collect_seq<I: IntoIterator>(self, iter: I) -> Result<Value, CodecError> where I::Item: Serialize {
        let mut sequence = self.serialize_seq(None)?;
        for value in iter { SerializeSeq::serialize_element(&mut sequence, &value)?; }
        SerializeSeq::end(sequence)
    }
    fn collect_str<T: fmt::Display + ?Sized>(self, _: &T) -> Result<Value, CodecError> { Err(CodecError::SerializationInvariant) }
    fn is_human_readable(&self) -> bool { true }
}

impl SerializeSeq for Compound<'_> {
    type Ok = Value;
    type Error = CodecError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), CodecError> { self.push(value) }
    fn end(self) -> Result<Value, CodecError> { self.finish() }
}
macro_rules! tuple_compound {
    ($trait:ident, $method:ident) => {
        impl $trait for Compound<'_> {
            type Ok = Value;
            type Error = CodecError;
            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), CodecError> { self.push(value) }
            fn end(self) -> Result<Value, CodecError> { self.finish() }
        }
    };
}
tuple_compound!(SerializeTuple, serialize_element);
tuple_compound!(SerializeTupleStruct, serialize_field);
tuple_compound!(SerializeTupleVariant, serialize_field);
impl SerializeMap for Compound<'_> {
    type Ok = Value;
    type Error = CodecError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), CodecError> { self.key(key) }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), CodecError> { self.map_value(value) }
    fn serialize_entry<K: Serialize + ?Sized, V: Serialize + ?Sized>(&mut self, key: &K, value: &V) -> Result<(), CodecError> { self.pair(key, value) }
    fn end(self) -> Result<Value, CodecError> { self.finish() }
}
impl SerializeStruct for Compound<'_> {
    type Ok = Value;
    type Error = CodecError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), CodecError> { self.field(key, value) }
    fn end(self) -> Result<Value, CodecError> { self.finish() }
}
impl SerializeStructVariant for Compound<'_> {
    type Ok = Value;
    type Error = CodecError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), CodecError> { self.field(key, value) }
    fn end(self) -> Result<Value, CodecError> { self.finish() }
}

struct KeySerializer<'a> { cx: &'a mut AllocationContext }
impl serde::Serializer for KeySerializer<'_> {
    type Ok = String;
    type Error = CodecError;
    type SerializeSeq = ser::Impossible<String, CodecError>;
    type SerializeTuple = ser::Impossible<String, CodecError>;
    type SerializeTupleStruct = ser::Impossible<String, CodecError>;
    type SerializeTupleVariant = ser::Impossible<String, CodecError>;
    type SerializeMap = ser::Impossible<String, CodecError>;
    type SerializeStruct = ser::Impossible<String, CodecError>;
    type SerializeStructVariant = ser::Impossible<String, CodecError>;
    fn serialize_str(self, value: &str) -> Result<String, CodecError> { Ok(product_alloc::try_copy_str(value, self.cx, AllocationSite::String)?) }
    fn serialize_char(self, value: char) -> Result<String, CodecError> { let mut b=[0;4]; self.serialize_str(value.encode_utf8(&mut b)) }
    fn serialize_bool(self, _: bool) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_i8(self, _: i8) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_i16(self, _: i16) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_i32(self, _: i32) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_i64(self, _: i64) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_i128(self, _: i128) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_u8(self, _: u8) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_u16(self, _: u16) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_u32(self, _: u32) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_u64(self, _: u64) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_u128(self, _: u128) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_f32(self, _: f32) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_f64(self, _: f64) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_none(self) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_unit(self) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, value: &T) -> Result<String, CodecError> { value.serialize(self) }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(self, _: &'static str, _: u32, _: &'static str, _: &T) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self::SerializeTupleVariant, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct, CodecError> { Err(CodecError::InvalidMapKey) }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self::SerializeStructVariant, CodecError> { Err(CodecError::InvalidMapKey) }
    fn collect_seq<I: IntoIterator>(self, _: I) -> Result<String, CodecError> where I::Item: Serialize { Err(CodecError::InvalidMapKey) }
    fn collect_str<T: fmt::Display + ?Sized>(self, _: &T) -> Result<String, CodecError> { Err(CodecError::InvalidMapKey) }
    fn is_human_readable(&self) -> bool { true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Serialize)]
    struct EdgeWire {
        z_nested: BTreeMap<String, Vec<Option<String>>>,
        a_text: String,
        f_value: f64,
        b_bytes: Vec<u8>,
        c_unit: EdgeTag,
        d_variants: Vec<EdgeTag>,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "snake_case")]
    enum EdgeTag {
        Unit,
        Newtype(String),
        Tuple(u32, bool),
        Struct { z: String, a: u64 },
    }
    impl BackupWire for EdgeWire {}

    #[test]
    fn bounded_serializer_matches_serde_canonical_shapes_and_digest() {
        let mut map = BTreeMap::new();
        map.insert("é\n".to_owned(), vec![Some("雪\0\"".to_owned()), None]);
        let wire = EdgeWire {
            z_nested: map,
            a_text: "é雪\n\0\"\\".to_owned(),
            f_value: -0.0,
            b_bytes: vec![0, 1, 255],
            c_unit: EdgeTag::Unit,
            d_variants: vec![
                EdgeTag::Newtype("newtype雪".into()),
                EdgeTag::Tuple(7, true),
                EdgeTag::Struct { z: "z".into(), a: 1 },
            ],
        };
        let mut cx = AllocationContext::new();
        let actual = canonical_bytes(&wire, &mut cx).unwrap();
        let expected = super::super::canonical_bytes(&wire).unwrap();
        assert_eq!(actual, expected);
        assert!(cx.growth_requests() > 0);
        let mut cx = AllocationContext::new();
        assert_eq!(canonical_digest(IdentityDomain::Evidence, &wire, &mut cx).unwrap(),
            super::super::canonical_digest(IdentityDomain::Evidence, &wire).unwrap());
    }

    #[test]
    fn every_emitted_container_counts_against_the_depth_cap() {
        let tuple = EdgeTag::Tuple(1, true);
        let mut cx = AllocationContext::new();
        assert!(tuple
            .serialize(ValueSerializer { cx: &mut cx, depth: MAX_DEPTH - 2 })
            .is_ok());
        let mut cx = AllocationContext::new();
        assert!(matches!(tuple.serialize(ValueSerializer { cx: &mut cx, depth: MAX_DEPTH - 1 }),
            Err(CodecError::DepthLimit)));

        let structure = EdgeTag::Struct { z: "z".into(), a: 1 };
        let mut cx = AllocationContext::new();
        assert!(structure
            .serialize(ValueSerializer { cx: &mut cx, depth: MAX_DEPTH - 2 })
            .is_ok());
        let mut cx = AllocationContext::new();
        assert!(matches!(structure.serialize(ValueSerializer { cx: &mut cx, depth: MAX_DEPTH - 1 }),
            Err(CodecError::DepthLimit)));

        let bytes = [1, 2];
        let mut cx = AllocationContext::new();
        assert!(serde::Serializer::serialize_bytes(
            ValueSerializer { cx: &mut cx, depth: MAX_DEPTH - 1 }, &bytes).is_ok());
        let mut cx = AllocationContext::new();
        assert!(matches!(serde::Serializer::serialize_bytes(
            ValueSerializer { cx: &mut cx, depth: MAX_DEPTH }, &bytes), Err(CodecError::DepthLimit)));
    }
}
