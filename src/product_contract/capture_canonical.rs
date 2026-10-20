//! Paid descriptors for the closed derived-Serialize callers in the capture route.
//! This is not a cost contract for arbitrary user-defined Serialize code.
use super::bounded_input::capture_payment_core as native;
use super::capture_work::{Audit, WorkState};
use super::*;
use serde::ser::{
    self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
    SerializeTupleStruct, SerializeTupleVariant,
};
use std::mem::{align_of, size_of};

impl ser::Error for ContractError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(message.to_string())
    }
}

pub(super) struct Shape<'w> {
    pub(super) audit: Audit<'w>,
    pub(super) bytes: u64,
    pub(super) nodes: u64,
    pub(super) pairs: u64,
    native: u64,
    canonical: bool,
    pub(super) digest_copies: u64,
    escaped: u64,
    simple_app: bool,
}
impl<'w> Shape<'w> {
    pub(super) fn new(audit: Audit<'w>) -> Result<Self> {
        // The leaf-only X scratch is funded once per fresh descriptor. Constants
        // fail compilation on target-width overflow, before any native operation.
        const SCRATCH: usize = 4 * (16 + 4 * size_of::<usize>() + size_of::<Option<u64>>());
        audit.pay((12 + size_of::<Self>() + 8 + 2 * size_of::<u64>()
            + 2 * size_of::<bool>() + SCRATCH) as u64)?;
        Ok(Self {
            audit,
            bytes: 0,
            nodes: 0,
            pairs: 0,
            native: 0,
            canonical: true,
            digest_copies: 0,
            escaped: 0,
            simple_app: false,
        })
    }
    fn add(&mut self, bytes: u64, native: u64) -> Result<()> {
        self.audit.pay(6)?;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .ok_or_else(|| self.audit.overflow())?;
        self.native = self
            .native
            .checked_add(native)
            .ok_or_else(|| self.audit.overflow())?;
        Ok(())
    }
    fn node(&mut self) -> Result<()> {
        self.audit.pay(3)?;
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| self.audit.overflow())?;
        self.add(0, 12)
    }
    pub(super) fn string(&mut self, value: &str) -> Result<()> {
        self.node()?;
        self.audit.pay(12)?;
        let mut encoded = 2u64;
        self.audit.pay(2)?;
        if self.simple_app {
        for byte in value.bytes() {
            self.audit.pay(14)?;
            let n = match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                0..=31 => 6,
                _ => 1,
            };
            encoded = encoded
                .checked_add(n)
                .ok_or_else(|| self.audit.overflow())?;
            if n != 1 {
                self.escaped = self.escaped.checked_add(1)
                    .ok_or_else(|| self.audit.overflow())?;
            }
        }
        } else {
        for byte in value.bytes() {
            self.audit.pay(8)?;
            let n = match byte {
                b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 8 | 12 => 2,
                0..=31 => 6,
                _ => 1,
            };
            encoded = encoded
                .checked_add(n)
                .ok_or_else(|| self.audit.overflow())?;
        }
        }
        self.audit.pay(12)?;
        let copy = if self.canonical {
            native::string_copy_bytes(value.len()).ok_or_else(|| self.audit.overflow())?
        } else {
            0
        };
        // to_value string ownership; final serializer input inspection and output writes.
        let amount = copy
            .checked_add(
                (value.len() as u64)
                    .checked_mul(6)
                    .ok_or_else(|| self.audit.overflow())?,
            )
            .and_then(|n| n.checked_add(encoded))
            .ok_or_else(|| self.audit.overflow())?;
        self.add(encoded, amount)
    }
    pub(super) fn digest(&mut self, value: &Digest) -> Result<()> {
        // Never call Digest::serialize here: derive(into=String) would clone first.
        self.audit.pay(10)?;
        self.digest_copies = self
            .digest_copies
            .checked_add(
                native::string_copy_bytes(value.0.len()).ok_or_else(|| self.audit.overflow())?,
            )
            .ok_or_else(|| self.audit.overflow())?;
        self.string(&value.0)
    }
    pub(super) fn integer(&mut self, negative: bool, mut magnitude: u64) -> Result<()> {
        self.node()?;
        self.audit.pay(6)?;
        let mut digits = 1u64;
        while magnitude >= 10 {
            self.audit.pay(4)?;
            magnitude /= 10;
            digits += 1;
        }
        if negative {
            digits += 1;
        }
        self.add(digits, 32 + digits)
    }
    pub(super) fn boolean(&mut self, value: bool) -> Result<()> {
        self.node()?;
        self.add(if value { 4 } else { 5 }, 5)
    }
    pub(super) fn null(&mut self) -> Result<()> {
        self.node()?;
        self.add(4, 4)
    }
    pub(super) fn array(&mut self, len: usize) -> Result<Container<'_, 'w>> {
        self.node()?;
        self.audit.pay(20)?;
        let n = self.audit.usize(len)?;
        let s = size_of::<JsonValue>() as u64;
        let amount = n
            .checked_mul(s)
            .and_then(|n| n.checked_mul(2))
            .and_then(|n| n.checked_add((len as u64).checked_mul(9)?))
            .and_then(|n| n.checked_add(8))
            .ok_or_else(|| self.audit.overflow())?;
        let writer = n
            .checked_mul(3)
            .and_then(|n| n.checked_add(4))
            .ok_or_else(|| self.audit.overflow())?;
        self.add(2, if self.canonical { amount } else { writer })?;
        Ok(Container {
            shape: self,
            count: 0,
            object: false,
            outer: false,
        })
    }
    pub(super) fn object(&mut self) -> Result<Container<'_, 'w>> {
        self.node()?;
        self.add(2, 8)?;
        Ok(Container {
            shape: self,
            count: 0,
            object: true,
            outer: false,
        })
    }
    fn reserve_output(&mut self) -> Result<()> {
        self.audit.pay(64 + 4 * usize::BITS as u64)?;
        let c = self
            .bytes
            .checked_mul(2)
            .map(|n| n.max(128))
            .ok_or_else(|| self.audit.overflow())?;
        let growth =
            native::growth_count(usize::try_from(self.bytes).map_err(|_| self.audit.overflow())?)
                .ok_or_else(|| self.audit.overflow())?;
        let amount = c
            .checked_mul(3)
            .and_then(|n| n.checked_add(128))
            .and_then(|n| n.checked_add(growth.checked_mul(9)?))
            .and_then(|n| n.checked_add(8))
            .ok_or_else(|| self.audit.overflow())?;
        self.add(0, amount)?;
        self.audit.pay(64)?;
        self.audit.pay(2)?;
        let writes = if self.simple_app {
            self.nodes.checked_mul(4)
                .and_then(|n| n.checked_add(self.escaped.checked_mul(2)?))
                .and_then(|n| n.checked_add(2))
                .ok_or_else(|| self.audit.overflow())?
        } else {
            self
                .bytes
            .checked_mul(2)
            .and_then(|n| n.checked_add(self.nodes.checked_mul(4)?))
            .and_then(|n| n.checked_add(self.pairs.checked_mul(4)?))
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| self.audit.overflow())?
        };
        self.add(
            0,
            writes.checked_mul(8).ok_or_else(|| self.audit.overflow())?,
        )?;
        // Closed typed serializers can still transport a finite native error.
        self.audit.format(128)?;
        self.audit
            .scaled(size_of::<std::io::Error>() + 4 * size_of::<usize>(), 2, 5)?;
        self.audit.pay(self.native)
    }
}
pub(super) struct Container<'a, 'w> {
    shape: &'a mut Shape<'w>,
    count: usize,
    object: bool,
    outer: bool,
}
impl Container<'_, '_> {
    fn separator(&mut self) -> Result<()> {
        self.shape.audit.pay(6)?;
        if self.count > 0 {
            self.shape.add(1, 1)?;
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| self.shape.audit.overflow())?;
        Ok(())
    }
    pub(super) fn element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<()> {
        self.separator()?;
        self.shape.audit.pay(2)?;
        value.serialize(&mut *self.shape)
    }
    pub(super) fn manual_element(&mut self, value: &impl ManualShape) -> Result<()> {
        self.separator()?;
        self.shape.audit.pay(2)?;
        value.describe(self.shape)
    }
    pub(super) fn field<T: Serialize + ?Sized>(&mut self, key: &str, value: &T) -> Result<()> {
        self.key(key)?;
        self.shape.audit.pay(2)?;
        value.serialize(&mut *self.shape)
    }
    pub(super) fn manual_field<T: ManualShape + ?Sized>(
        &mut self,
        key: &str,
        value: &T,
    ) -> Result<()> {
        self.key(key)?;
        self.shape.audit.pay(2)?;
        value.describe(self.shape)
    }
    fn key(&mut self, key: &str) -> Result<()> {
        let index = self.count;
        self.separator()?;
        self.shape.string(key)?;
        self.shape.add(1, 1)?;
        if self.shape.canonical {
            self.shape.audit.pay(native::INSERT_DESCRIPTOR + 12)?;
            let c = self.shape.audit.comparator(key.len())?;
            self.shape.add(
                0,
                native::btree_insert::<String, JsonValue>(index, c)
                    .ok_or_else(|| self.shape.audit.overflow())?,
            )?;
        }
        self.shape.pairs = self
            .shape
            .pairs
            .checked_add(1)
            .ok_or_else(|| self.shape.audit.overflow())?;
        Ok(())
    }
    pub(super) fn end(self) -> Result<()> {
        self.shape.audit.pay(8)?;
        if self.object && self.shape.canonical {
            // Initial Value insertion/ownership remains paid at each key. With
            // the pinned BTree representation, sort_all_objects is a no-op;
            // only the final serializer's complete map traversal remains here.
            self.shape.audit.pay(8)?;
            let iter = native::btree_iter(self.count).ok_or_else(|| self.shape.audit.overflow())?;
            self.shape.add(0, iter)?;
        }
        if self.outer {
            self.shape.add(1, 1)?;
        }
        Ok(())
    }
}
fn sort_bound<T>(len: usize, comparator: u64, a: Audit<'_>) -> Result<u64> {
    // Same native sort interface as runtime: SC32, normal/tiny maxima, exact
    // payload width, and explicit logical native frame slots. Arithmetic only.
    a.pay(160 + 8 * usize::BITS as u64)?;
    let bound = (|| -> Option<u64> {
        if len < 2 {
            return Some(0);
        }
        let n = u64::try_from(len).ok()?;
        let size = u64::try_from(size_of::<T>()).ok()?;
        let pointer = u64::try_from(size_of::<usize>()).ok()?;
        let mut q = len.checked_sub(1)?;
        let mut h = 0u64;
        while q > 0 {
            q /= 2;
            h = h.checked_add(1)?;
        }
        let l = h.checked_sub(u64::from(!len.is_power_of_two()))?;
        let (normal_c, normal_m) = if n <= 20 {
            let c = n.checked_mul(n.checked_sub(1)?)?.checked_div(2)?;
            (c, c.checked_add(n.checked_sub(1)?.checked_mul(2)?)?)
        } else {
            (
                n.checked_mul(
                    h.checked_mul(2)?
                        .checked_add(l.checked_mul(8)?)?
                        .checked_add(40)?,
                )?,
                n.checked_mul(
                    h.checked_mul(4)?
                        .checked_add(l.checked_mul(12)?)?
                        .checked_add(48)?,
                )?,
            )
        };
        let comparisons = normal_c.max(n.checked_mul(h)?);
        let moves = normal_m.max(n.checked_mul(h)?.checked_mul(3)?);
        let control = comparisons
            .checked_add(moves)?
            .checked_add(n)?
            .checked_add(1)?
            .checked_mul(32)?;
        let slots = n
            .checked_add(1)?
            .checked_div(2)?
            .max(n.min(8_000_000 / size.max(1)))
            .max(48)
            .max(n / 2);
        let request = slots.checked_mul(size)?;
        let heap = if request > 4096 { request } else { 0 };
        let mut x = n;
        let mut pivot = 1u64;
        while x > 1 {
            x = x.checked_add(7)?.checked_div(8)?;
            pivot = pivot.checked_add(1)?;
        }
        let drift = 66u64
            .checked_mul(pointer)?
            .checked_add(66)?
            .checked_mul(2)?;
        let quick = l
            .checked_mul(2)?
            .checked_add(2)?
            .checked_mul(pointer.checked_mul(16)?.checked_add(size)?)?;
        let pivot_slots = pivot.checked_mul(pointer.checked_mul(8)?)?;
        let merge = size.checked_mul(4)?.checked_add(pointer.checked_mul(32)?)?;
        let storage = 4096u64
            .checked_add(heap)?
            .checked_add(drift)?
            .checked_add(quick)?
            .checked_add(pivot_slots)?
            .checked_add(merge)?;
        comparisons
            .checked_mul(comparator)?
            .checked_add(moves.checked_mul(size)?)?
            .checked_add(control)?
            .checked_add(storage)?
            .checked_add(12)
    })();
    bound.ok_or_else(|| a.overflow())
}

impl<'a, 'w> ser::Serializer for &'a mut Shape<'w> {
    type Ok = ();
    type Error = ContractError;
    type SerializeSeq = Container<'a, 'w>;
    type SerializeTuple = Container<'a, 'w>;
    type SerializeTupleStruct = Container<'a, 'w>;
    type SerializeTupleVariant = Container<'a, 'w>;
    type SerializeMap = Container<'a, 'w>;
    type SerializeStruct = Container<'a, 'w>;
    type SerializeStructVariant = Container<'a, 'w>;
    fn serialize_bool(self, v: bool) -> Result<()> {
        self.boolean(v)
    }
    fn serialize_i8(self, v: i8) -> Result<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> Result<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> Result<()> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> Result<()> {
        self.integer(v < 0, v.unsigned_abs())
    }
    fn serialize_u8(self, v: u8) -> Result<()> {
        self.integer(false, v.into())
    }
    fn serialize_u16(self, v: u16) -> Result<()> {
        self.integer(false, v.into())
    }
    fn serialize_u32(self, v: u32) -> Result<()> {
        self.integer(false, v.into())
    }
    fn serialize_u64(self, v: u64) -> Result<()> {
        self.integer(false, v)
    }
    fn serialize_f32(self, _: f32) -> Result<()> {
        Err(self.audit.error("unexpected floating capture serializer")?)
    }
    fn serialize_f64(self, _: f64) -> Result<()> {
        Err(self.audit.error("unexpected floating capture serializer")?)
    }
    fn serialize_char(self, v: char) -> Result<()> {
        self.audit.pay(8)?;
        let mut b = [0; 4];
        self.string(v.encode_utf8(&mut b))
    }
    fn serialize_str(self, v: &str) -> Result<()> {
        self.string(v)
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<()> {
        let mut c = self.array(v.len())?;
        for n in v {
            c.element(n)?;
        }
        c.end()
    }
    fn serialize_none(self) -> Result<()> {
        self.null()
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<()> {
        self.audit.pay(3)?;
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<()> {
        self.null()
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        self.null()
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, v: &'static str) -> Result<()> {
        self.string(v)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, v: &T) -> Result<()> {
        self.audit.pay(3)?;
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        k: &'static str,
        v: &T,
    ) -> Result<()> {
        let mut c = self.object()?;
        c.field(k, v)?;
        c.end()
    }
    fn collect_seq<I>(self, values: I) -> Result<()>
    where
        I: IntoIterator,
        I::Item: Serialize,
    {
        // Closed callers are slices/Vecs or the four-variant KeyModifier set.
        // Pay the lazy iterator/header region BEFORE IntoIterator and size_hint.
        self.audit.pay(10 + 8)?;
        let mut iter = values.into_iter();
        let (lower, upper) = iter.size_hint();
        self.audit.pay(4)?;
        let hint = if upper == Some(lower) {
            Some(lower)
        } else {
            None
        };
        if let Some(len) = hint {
            if len <= 4 {
                // All possible set inputs are in this finite family. Short slices
                // conservatively receive the same separate tree traversal credits.
                self.audit.map_iter(len)?;
                self.audit.pay(8)?;
                let traversal = native::btree_iter(len).ok_or_else(|| self.audit.overflow())?;
                self.add(0, traversal)?;
            }
        }
        let mut serializer = self.serialize_seq(hint)?;
        iter.try_for_each(|item| SerializeSeq::serialize_element(&mut serializer, &item))?;
        SerializeSeq::end(serializer)
    }
    fn collect_map<K, V, I>(self, values: I) -> Result<()>
    where
        K: Serialize,
        V: Serialize,
        I: IntoIterator<Item = (K, V)>,
    {
        // The approved App callers establish map length <= MAX_ITEMS (256).
        // Native BTree initialization is lazy; its first next remains below the
        // complete descriptor traversal debit in serialize_map.
        self.audit.pay(26 + 8)?;
        let mut iter = values.into_iter();
        let (lower, upper) = iter.size_hint();
        self.audit.pay(4)?;
        let hint = if upper == Some(lower) {
            Some(lower)
        } else {
            None
        };
        let mut serializer = self.serialize_map(hint)?;
        iter.try_for_each(|(key, value)| {
            SerializeMap::serialize_entry(&mut serializer, &key, &value)
        })?;
        SerializeMap::end(serializer)
    }
    fn serialize_seq(self, n: Option<usize>) -> Result<Self::SerializeSeq> {
        self.audit.slice(n.unwrap_or(0))?;
        self.array(n.unwrap_or(0))
    }
    fn serialize_tuple(self, n: usize) -> Result<Self::SerializeTuple> {
        self.audit.slice(n)?;
        self.array(n)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        n: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.array(n)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        k: &'static str,
        n: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        {
            let mut c = self.object()?;
            c.key(k)?;
            c.end()?;
        }
        let mut c = self.array(n)?;
        c.outer = true;
        Ok(c)
    }
    fn serialize_map(self, n: Option<usize>) -> Result<Self::SerializeMap> {
        let len = n.unwrap_or(0);
        self.audit.map_iter(len)?;
        self.audit.pay(8)?;
        self.add(
            0,
            native::btree_iter(len).ok_or_else(|| self.audit.overflow())?,
        )?;
        self.object()
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        self.object()
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        k: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        {
            let mut c = self.object()?;
            c.key(k)?;
            c.end()?;
        }
        let mut c = self.object()?;
        c.outer = true;
        Ok(c)
    }
}
impl SerializeSeq for Container<'_, '_> {
    type Ok = ();
    type Error = ContractError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        self.element(v)
    }
    fn end(self) -> Result<()> {
        self.end()
    }
}
macro_rules! tuple_impl {
    ($trait:ident,$method:ident) => {
        impl $trait for Container<'_, '_> {
            type Ok = ();
            type Error = ContractError;
            fn $method<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
                self.element(v)
            }
            fn end(self) -> Result<()> {
                self.end()
            }
        }
    };
}
tuple_impl!(SerializeTuple, serialize_element);
tuple_impl!(SerializeTupleStruct, serialize_field);
tuple_impl!(SerializeTupleVariant, serialize_field);
impl SerializeStruct for Container<'_, '_> {
    type Ok = ();
    type Error = ContractError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, k: &'static str, v: &T) -> Result<()> {
        self.field(k, v)
    }
    fn end(self) -> Result<()> {
        self.end()
    }
}
impl SerializeStructVariant for Container<'_, '_> {
    type Ok = ();
    type Error = ContractError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, k: &'static str, v: &T) -> Result<()> {
        self.field(k, v)
    }
    fn end(self) -> Result<()> {
        self.end()
    }
}
// Closed callers have only String keys. The key serializer observes their borrowed bytes.
struct Key<'a, 'b, 'w>(&'a mut Container<'b, 'w>);
impl<'a, 'b, 'w> ser::Serializer for Key<'a, 'b, 'w> {
    type Ok = ();
    type Error = ContractError;
    type SerializeSeq = ser::Impossible<(), ContractError>;
    type SerializeTuple = ser::Impossible<(), ContractError>;
    type SerializeTupleStruct = ser::Impossible<(), ContractError>;
    type SerializeTupleVariant = ser::Impossible<(), ContractError>;
    type SerializeMap = ser::Impossible<(), ContractError>;
    type SerializeStruct = ser::Impossible<(), ContractError>;
    type SerializeStructVariant = ser::Impossible<(), ContractError>;
    fn serialize_str(self, v: &str) -> Result<()> {
        self.0.key(v)
    }
    fn serialize_bool(self, _: bool) -> Result<()> {
        Err(self.0.shape.audit.error("unsupported capture map key")?)
    }
    fn serialize_i8(self, _: i8) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_i16(self, _: i16) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_i32(self, _: i32) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_i64(self, _: i64) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_u8(self, _: u8) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_u16(self, _: u16) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_u32(self, _: u32) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_u64(self, _: u64) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_f32(self, _: f32) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_f64(self, _: f64) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_char(self, _: char) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_none(self) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, _: &T) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_unit(self) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_unit_variant(self, _: &'static str, _: u32, _: &'static str) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(self, _: &'static str, _: &T) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<()> {
        self.serialize_bool(false)
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(self.0.shape.audit.error("unsupported capture map key")?)
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(None)
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(None)
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.serialize_seq(None)
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        self.serialize_seq(None)
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        self.serialize_seq(None)
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.serialize_seq(None)
    }
}
impl SerializeMap for Container<'_, '_> {
    type Ok = ();
    type Error = ContractError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        v.serialize(Key(self))
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<()> {
        self.shape.audit.pay(2)?;
        v.serialize(&mut *self.shape)
    }
    fn end(self) -> Result<()> {
        self.end()
    }
}

pub(crate) fn canonical_bytes_with_work<T: Serialize + ?Sized>(
    value: &T,
    work: &mut dyn ContractWork,
) -> ContractWorkResult<Vec<u8>> {
    work.charge(0).map_err(ContractWorkError::Resource)?;
    work.charge(8 + size_of::<WorkState<'_>>() as u64)
        .map_err(ContractWorkError::Resource)?;
    let state = WorkState::new(work);
    let audit = Audit(Some(&state));
    state.finish(canonical_bytes_audited(value, audit))
}
#[cfg_attr(test, track_caller)]
pub(super) fn canonical_bytes_audited<T: Serialize + ?Sized>(
    value: &T,
    a: Audit<'_>,
) -> Result<Vec<u8>> {
    #[cfg(test)]
    let _diagnostic = super::capture_work::diagnostic::enter(
        super::capture_work::diagnostic::Phase::Canonical,
    );
    if !a.enabled() {
        return canonical_bytes(&value);
    }
    a.pay(0)?;
    let mut shape = Shape::new(a)?;
    value.serialize(&mut shape)?;
    shape.reserve_output()?;
    // Conversion and final serialization retain their original native boundaries.
    let bytes = canonical_bytes(&value)?;
    a.pay(0)?;
    Ok(bytes)
}

struct AppLengthCounter<'w> {
    audit: Audit<'w>,
    size: usize,
    overflowed: bool,
}
impl AppLengthCounter<'_> {
    fn add_len(&mut self, n: usize) -> std::io::Result<()> {
        if self.overflowed {
            return Err(std::io::ErrorKind::Other.into());
        }
        let Some(size) = self.size.checked_add(n) else {
            self.overflowed = true;
            let _ = self.audit.overflow();
            return Err(std::io::ErrorKind::Other.into());
        };
        self.size = size;
        Ok(())
    }
    fn finish(&self, native: serde_json::Result<()>) -> Result<usize> {
        if self.overflowed {
            return Err(ContractError(String::new()));
        }
        native.map_err(|error| ContractError(error.to_string()))?;
        self.audit.pay(0)?;
        Ok(self.size)
    }
}
impl std::io::Write for AppLengthCounter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.add_len(bytes.len())?;
        Ok(bytes.len())
    }
    fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.add_len(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn reserve_app_count(shape: &Shape<'_>) -> Result<()> {
    let a = shape.audit;
    a.pay(96)?;
    usize::try_from(shape.bytes).map_err(|_| a.overflow())?;
    a.pay(2)?;
    let writes = if shape.simple_app {
        shape.nodes.checked_mul(4)
            .and_then(|n| n.checked_add(shape.escaped.checked_mul(2)?))
            .and_then(|n| n.checked_add(2))
            .ok_or_else(|| a.overflow())?
    } else {
        shape
            .bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(shape.nodes.checked_mul(4)?))
        .and_then(|n| n.checked_add(shape.pairs.checked_mul(4)?))
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| a.overflow())?
    };
    let counter = a.usize(size_of::<AppLengthCounter<'_>>())?;
    let serializer = a.usize(size_of::<serde_json::Serializer<&mut AppLengthCounter<'_>>>())?;
    let result = a.usize(size_of::<serde_json::Result<()>>())?;
    let root = counter
        .checked_mul(2)
        .and_then(|n| n.checked_add(serializer.checked_mul(2)?))
        .and_then(|n| n.checked_add(result.checked_mul(2)?))
        .and_then(|n| n.checked_add(12))
        .ok_or_else(|| a.overflow())?;
    // Native traversal, each complete Write chunk, fixed owners and first failure.
    let amount = writes
        .checked_mul(14)
        .and_then(|n| shape.native.checked_add(n))
        .and_then(|n| n.checked_add(root))
        .and_then(|n| n.checked_add(72))
        .ok_or_else(|| a.overflow())?;
    let error_bytes = size_of::<usize>()
        .checked_mul(4)
        .and_then(|n| n.checked_add(size_of::<Box<str>>().max(size_of::<std::io::Error>())))
        .ok_or_else(|| a.overflow())?;
    a.format(128)?;
    a.scaled(error_bytes, 2, 5)?;
    a.pay(amount)
}

// Only the App validator's successful semantic prefix establishes this depth bound.
#[cfg_attr(test, track_caller)]
pub(super) fn app_canonical_len_audited(app: &AppDefinition, a: Audit<'_>) -> Result<usize> {
    #[cfg(test)]
    let _diagnostic = super::capture_work::diagnostic::enter(
        super::capture_work::diagnostic::Phase::Canonical,
    );
    if !a.enabled() {
        return canonical_bytes(app).map(|bytes| bytes.len());
    }
    a.pay(0)?;
    a.pay(96)?;
    // H = 8 + 3*(expr+1) + 2*(type+1) + (type+1).
    let height = a
        .usize(MAX_EXPR_DEPTH)?
        .checked_add(a.usize(MAX_TYPE_DEPTH)?)
        .and_then(|n| n.checked_add(2))
        .and_then(|n| n.checked_mul(3))
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| a.overflow())?;
    let frame = a
        .usize(size_of::<usize>())?
        .checked_mul(64)
        .and_then(|n| n.checked_add(64))
        .ok_or_else(|| a.overflow())?;
    // Separate descriptor/native logical frames, including their inline writes.
    let stack = height
        .checked_mul(frame)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| a.overflow())?;
    a.pay(stack)?;
    let mut shape = Shape::new(a)?;
    a.pay(1)?;
    shape.canonical = false;
    a.pay(1)?;
    shape.simple_app = true;
    app.serialize(&mut shape)?;
    reserve_app_count(&shape)?;
    let mut counter = AppLengthCounter {
        audit: a,
        size: 0,
        overflowed: false,
    };
    let native = serde_json::to_writer(&mut counter, app);
    counter.finish(native)
}

pub(super) trait ManualShape {
    fn describe(&self, shape: &mut Shape<'_>) -> Result<()>;
}
impl ManualShape for String {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.string(self)
    }
}
impl ManualShape for str {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.string(self)
    }
}
impl ManualShape for bool {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.boolean(*self)
    }
}
impl ManualShape for Digest {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.digest(self)
    }
}
macro_rules! unsigned {($($t:ty),*)=>{$(impl ManualShape for $t {fn describe(&self,s:&mut Shape<'_>)->Result<()> {s.integer(false,*self as u64)}})*};}
macro_rules! signed {($($t:ty),*)=>{$(impl ManualShape for $t {fn describe(&self,s:&mut Shape<'_>)->Result<()> {s.integer(*self<0,self.unsigned_abs() as u64)}})*};}
unsigned!(u32, u64, usize);
signed!(i32, i64);
impl<T: ManualShape> ManualShape for Box<T> {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        self.as_ref().describe(s)
    }
}
impl<T: ManualShape> ManualShape for Option<T> {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(3)?;
        match self {
            Some(v) => v.describe(s),
            None => s.null(),
        }
    }
}
impl<T: ManualShape> ManualShape for Vec<T> {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.slice(self.len())?;
        let mut c = s.array(self.len())?;
        for v in self {
            c.manual_element(v)?;
        }
        c.end()
    }
}
impl<T: ManualShape> ManualShape for BTreeMap<String, T> {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.map_iter(self.len())?;
        s.audit.pay(8)?;
        s.add(
            0,
            native::btree_iter(self.len()).ok_or_else(|| s.audit.overflow())?,
        )?;
        let mut c = s.object()?;
        for (k, v) in self {
            c.manual_field(k, v)?;
        }
        c.end()
    }
}

#[cfg_attr(test, track_caller)]
pub(super) fn data_serialized_bound_audited(data: &DataSnapshot, a: Audit<'_>) -> Result<()> {
    #[cfg(test)]
    let _diagnostic = super::capture_work::diagnostic::enter(
        super::capture_work::diagnostic::Phase::DataWriter,
    );
    if !a.enabled() {
        return serialized_bound(data, MAX_WIRE_BYTES);
    }
    a.pay(0)?;
    let mut shape = Shape::new(a)?;
    shape.canonical = false;
    data.describe(&mut shape)?;
    a.pay(64)?;
    let writes = shape
        .bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(shape.nodes.checked_mul(4)?))
        .and_then(|n| n.checked_add(shape.pairs.checked_mul(4)?))
        .and_then(|n| n.checked_add(2))
        .ok_or_else(|| a.overflow())?;
    let amount = writes
        .checked_mul(8)
        .and_then(|n| n.checked_add(shape.native))
        .and_then(|n| n.checked_add(shape.digest_copies))
        .and_then(|n| n.checked_add(6 + 2 * size_of::<usize>() as u64))
        .ok_or_else(|| a.overflow())?;
    a.pay(amount)?;
    a.pay(96)?;
    let p = size_of::<usize>();
    let cb = size_of::<std::io::ErrorKind>()
        .checked_add(4 * p)
        .and_then(|n| n.checked_add(5 * align_of::<std::io::ErrorKind>().max(p).max(4)))
        .ok_or_else(|| a.overflow())?;
    let eb = size_of::<Box<str>>()
        .max(size_of::<std::io::Error>())
        .checked_add(4 * p)
        .ok_or_else(|| a.overflow())?;
    a.pay(20)?;
    a.string(38)?;
    a.boxed::<String>()?;
    a.scaled(cb, 2, 3)?;
    a.scaled(eb, 2, 5)?;
    a.format(38)?;
    // Original Counter receives exactly its original write chunks/error ordering.
    serialized_bound(data, MAX_WIRE_BYTES)?;
    a.pay(0)
}

// Manual, borrowed typed Data writer descriptor. No Serialize implementation is
// invoked here, particularly not Digest's clone-before-serialize conversion.
impl ManualShape for Type {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        match self {
            Type::Boolean => {
                let mut c = s.object()?;
                c.manual_field("kind", "boolean")?;
                c.end()
            }
            Type::Integer => {
                let mut c = s.object()?;
                c.manual_field("kind", "integer")?;
                c.end()
            }
            Type::Text => {
                let mut c = s.object()?;
                c.manual_field("kind", "text")?;
                c.end()
            }
            Type::Date => {
                let mut c = s.object()?;
                c.manual_field("kind", "date")?;
                c.end()
            }
            Type::Reference { entity } => {
                let mut c = s.object()?;
                c.manual_field("kind", "reference")?;
                c.manual_field("entity", entity)?;
                c.end()
            }
            Type::List { item } => {
                let mut c = s.object()?;
                c.manual_field("kind", "list")?;
                c.manual_field("item", item)?;
                c.end()
            }
            Type::Optional { item } => {
                let mut c = s.object()?;
                c.manual_field("kind", "optional")?;
                c.manual_field("item", item)?;
                c.end()
            }
        }
    }
}
impl ManualShape for DataValue {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        match self {
            DataValue::Null => {
                let mut c = s.object()?;
                c.manual_field("kind", "null")?;
                c.end()
            }
            DataValue::Boolean { value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "boolean")?;
                c.manual_field("value", value)?;
                c.end()
            }
            DataValue::Integer { value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "integer")?;
                c.manual_field("value", value)?;
                c.end()
            }
            DataValue::Text { value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "text")?;
                c.manual_field("value", value)?;
                c.end()
            }
            DataValue::Date { days } => {
                let mut c = s.object()?;
                c.manual_field("kind", "date")?;
                c.manual_field("days", days)?;
                c.end()
            }
            DataValue::Reference { entity, record } => {
                let mut c = s.object()?;
                c.manual_field("kind", "reference")?;
                c.manual_field("entity", entity)?;
                c.manual_field("record", record)?;
                c.end()
            }
            DataValue::List { item_type, items } => {
                let mut c = s.object()?;
                c.manual_field("kind", "list")?;
                c.manual_field("item_type", item_type)?;
                c.manual_field("items", items)?;
                c.end()
            }
        }
    }
}
impl ManualShape for Expr {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        match self {
            Expr::Literal { value_type, value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "literal")?;
                c.manual_field("value_type", value_type)?;
                c.manual_field("value", value)?;
                c.end()
            }
            Expr::Variable { name } => {
                let mut c = s.object()?;
                c.manual_field("kind", "variable")?;
                c.manual_field("name", name)?;
                c.end()
            }
            Expr::Field { record, field } => {
                let mut c = s.object()?;
                c.manual_field("kind", "field")?;
                c.manual_field("record", record)?;
                c.manual_field("field", field)?;
                c.end()
            }
            Expr::State { state } => {
                let mut c = s.object()?;
                c.manual_field("kind", "state")?;
                c.manual_field("state", state)?;
                c.end()
            }
            Expr::Today => {
                let mut c = s.object()?;
                c.manual_field("kind", "today")?;
                c.end()
            }
            Expr::Query {
                entity,
                binding,
                predicate,
                sort,
                limit,
                include_archived,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "query")?;
                c.manual_field("entity", entity)?;
                c.manual_field("binding", binding)?;
                c.manual_field("predicate", predicate)?;
                c.manual_field("sort", sort)?;
                c.manual_field("limit", limit)?;
                c.manual_field("include_archived", include_archived)?;
                c.end()
            }
            Expr::Filter {
                items,
                binding,
                predicate,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "filter")?;
                c.manual_field("items", items)?;
                c.manual_field("binding", binding)?;
                c.manual_field("predicate", predicate)?;
                c.end()
            }
            Expr::Map {
                items,
                binding,
                value,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "map")?;
                c.manual_field("items", items)?;
                c.manual_field("binding", binding)?;
                c.manual_field("value", value)?;
                c.end()
            }
            Expr::Count { items } => {
                let mut c = s.object()?;
                c.manual_field("kind", "count")?;
                c.manual_field("items", items)?;
                c.end()
            }
            Expr::Any {
                items,
                binding,
                predicate,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "any")?;
                c.manual_field("items", items)?;
                c.manual_field("binding", binding)?;
                c.manual_field("predicate", predicate)?;
                c.end()
            }
            Expr::All {
                items,
                binding,
                predicate,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "all")?;
                c.manual_field("items", items)?;
                c.manual_field("binding", binding)?;
                c.manual_field("predicate", predicate)?;
                c.end()
            }
            Expr::Sum { items } => {
                let mut c = s.object()?;
                c.manual_field("kind", "sum")?;
                c.manual_field("items", items)?;
                c.end()
            }
            Expr::Contains { items, value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "contains")?;
                c.manual_field("items", items)?;
                c.manual_field("value", value)?;
                c.end()
            }
            Expr::Equal { left, right } => {
                let mut c = s.object()?;
                c.manual_field("kind", "equal")?;
                c.manual_field("left", left)?;
                c.manual_field("right", right)?;
                c.end()
            }
            Expr::Less { left, right } => {
                let mut c = s.object()?;
                c.manual_field("kind", "less")?;
                c.manual_field("left", left)?;
                c.manual_field("right", right)?;
                c.end()
            }
            Expr::Add { left, right } => {
                let mut c = s.object()?;
                c.manual_field("kind", "add")?;
                c.manual_field("left", left)?;
                c.manual_field("right", right)?;
                c.end()
            }
            Expr::Subtract { left, right } => {
                let mut c = s.object()?;
                c.manual_field("kind", "subtract")?;
                c.manual_field("left", left)?;
                c.manual_field("right", right)?;
                c.end()
            }
            Expr::Concat { left, right } => {
                let mut c = s.object()?;
                c.manual_field("kind", "concat")?;
                c.manual_field("left", left)?;
                c.manual_field("right", right)?;
                c.end()
            }
            Expr::TextContains { text, search } => {
                let mut c = s.object()?;
                c.manual_field("kind", "text_contains")?;
                c.manual_field("text", text)?;
                c.manual_field("search", search)?;
                c.end()
            }
            Expr::Lower { value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "lower")?;
                c.manual_field("value", value)?;
                c.end()
            }
            Expr::And { values } => {
                let mut c = s.object()?;
                c.manual_field("kind", "and")?;
                c.manual_field("values", values)?;
                c.end()
            }
            Expr::Or { values } => {
                let mut c = s.object()?;
                c.manual_field("kind", "or")?;
                c.manual_field("values", values)?;
                c.end()
            }
            Expr::Not { value } => {
                let mut c = s.object()?;
                c.manual_field("kind", "not")?;
                c.manual_field("value", value)?;
                c.end()
            }
            Expr::If {
                condition,
                then_value,
                else_value,
            } => {
                let mut c = s.object()?;
                c.manual_field("kind", "if")?;
                c.manual_field("condition", condition)?;
                c.manual_field("then_value", then_value)?;
                c.manual_field("else_value", else_value)?;
                c.end()
            }
            Expr::Coalesce { value, fallback } => {
                let mut c = s.object()?;
                c.manual_field("kind", "coalesce")?;
                c.manual_field("value", value)?;
                c.manual_field("fallback", fallback)?;
                c.end()
            }
            Expr::DateAdd { date, days } => {
                let mut c = s.object()?;
                c.manual_field("kind", "date_add")?;
                c.manual_field("date", date)?;
                c.manual_field("days", days)?;
                c.end()
            }
            Expr::DateDifference { later, earlier } => {
                let mut c = s.object()?;
                c.manual_field("kind", "date_difference")?;
                c.manual_field("later", later)?;
                c.manual_field("earlier", earlier)?;
                c.end()
            }
        }
    }
}
impl ManualShape for SortKey {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("value", &self.value)?;
        c.manual_field("descending", &self.descending)?;
        c.end()
    }
}
impl ManualShape for FieldDefinition {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("id", &self.id)?;
        c.manual_field("label", &self.label)?;
        c.manual_field("value_type", &self.value_type)?;
        c.end()
    }
}
impl ManualShape for EntityDefinition {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("id", &self.id)?;
        c.manual_field("label", &self.label)?;
        c.manual_field("fields", &self.fields)?;
        c.manual_field("unique", &self.unique)?;
        c.manual_field("constraints", &self.constraints)?;
        c.end()
    }
}
impl ManualShape for Record {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("entity", &self.entity)?;
        c.manual_field("id", &self.id)?;
        c.manual_field("revision", &self.revision)?;
        c.manual_field("created_program", &self.created_program)?;
        c.manual_field("archived", &self.archived)?;
        c.manual_field("values", &self.values)?;
        c.end()
    }
}
impl ManualShape for RecordChange {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("entity", &self.entity)?;
        c.manual_field("record", &self.record)?;
        c.manual_field("before", &self.before)?;
        c.manual_field("after", &self.after)?;
        c.manual_field("archived", &self.archived)?;
        c.end()
    }
}
impl ManualShape for BusinessEvent {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("id", &self.id)?;
        c.manual_field("sequence", &self.sequence)?;
        c.manual_field("operation_id", &self.operation_id)?;
        c.manual_field("action", &self.action)?;
        c.manual_field("day", &self.day)?;
        c.manual_field("program", &self.program)?;
        c.manual_field("changes", &self.changes)?;
        c.manual_field("outputs", &self.outputs)?;
        c.end()
    }
}
impl ManualShape for DataSnapshot {
    fn describe(&self, s: &mut Shape<'_>) -> Result<()> {
        s.audit.pay(2)?;
        let mut c = s.object()?;
        c.manual_field("version", &self.version)?;
        c.manual_field("project_id", &self.project_id)?;
        c.manual_field("generation", &self.generation)?;
        c.manual_field("schema", &self.schema)?;
        c.manual_field("records", &self.records)?;
        c.manual_field("events", &self.events)?;
        c.end()
    }
}

pub(super) fn reserve_from_iter<K, V>(len: usize, comparator: u64, a: Audit<'_>) -> Result<()> {
    if !a.enabled() {
        return Ok(());
    }
    a.pay(native::BULK_DESCRIPTOR + 16)?;
    let native = native::btree_bulk::<K, V>(len, comparator).ok_or_else(|| a.overflow())?;
    let sort = sort_bound::<(K, V)>(len, comparator, a)?;
    a.scaled(len, 2 * size_of::<(K, V)>() as u64, 4)?;
    a.amount(native.checked_add(sort))
}

#[cfg(test)]
#[path = "capture_app_length_tests.rs"]
mod app_length_tests;

#[path = "capture_closed_value.rs"]
mod closed_value;
pub(super) use closed_value::validate_closed_value_capture;

#[cfg(test)]
#[path = "capture_closed_value_tests.rs"]
mod closed_value_tests;
