//! Owned inline causes: establishing a schema failure NEVER allocates.
//! Diagnostic rendering borrows that cause and commits text only on success.
use crate::product_alloc::{AllocationContext, AllocationFailure, AllocationSite,
    FallibleStringWriter, FormatFailure};
use crate::product_json::{self, Number, Value};
use std::fmt::{self, Write};

#[derive(Debug)]
pub enum AppDecodeError { Json(product_json::Error), Allocation(AllocationFailure), Schema(SchemaFailure) }
#[derive(Debug)]
pub enum UnexpectedOwned { Unit, Bool(bool), U64(u64), I64(i64), F64(f64), String(String), Sequence, Map }
#[derive(Clone, Copy, Debug)]
pub enum Expected {
    Text(&'static str),
    Structure { name: &'static str, fields: usize },
    Variant { name: &'static str, fields: usize },
    VariantIndex(usize), Sequence(usize),
}
#[derive(Debug)]
pub enum SchemaFailure {
    MissingField(&'static str),
    UnknownField { key: String, expected: &'static [&'static str] },
    UnknownVariant { tag: String, expected: &'static [&'static str] },
    InvalidType { actual: UnexpectedOwned, expected: Expected },
    InvalidValue { actual: UnexpectedOwned, expected: Expected },
    InvalidLength { actual: usize, expected: Expected },
}
impl From<AllocationFailure> for AppDecodeError { fn from(e: AllocationFailure) -> Self { Self::Allocation(e) } }
impl From<SchemaFailure> for AppDecodeError { fn from(e: SchemaFailure) -> Self { Self::Schema(e) } }
impl UnexpectedOwned {
    pub(super) fn take(v: Value) -> Self {
        match v {
            Value::Null=>Self::Unit, Value::Bool(v)=>Self::Bool(v),
            Value::Number(Number::U64(v))=>Self::U64(v), Value::Number(Number::I64(v))=>Self::I64(v),
            Value::Number(Number::F64(v))=>Self::F64(v), Value::String(v)=>Self::String(v),
            Value::Array(_)=>Self::Sequence, Value::Object(_)=>Self::Map,
        }
    }
}
pub(super) fn invalid_type(v: Value, expected: Expected) -> AppDecodeError {
    SchemaFailure::InvalidType { actual: UnexpectedOwned::take(v), expected }.into()
}
pub(super) fn invalid_value(v: Value, expected: Expected) -> AppDecodeError {
    SchemaFailure::InvalidValue { actual: UnexpectedOwned::take(v), expected }.into()
}
impl AppDecodeError {
    pub fn try_render(&self, cx: &mut AllocationContext) -> Result<String, FormatFailure> {
        let mut out = String::new();
        self.try_append_diagnostic(&mut out, cx)?;
        Ok(out)
    }
    /// Transactional append; refusal preserves preexisting content and cause.
    pub fn try_append_diagnostic(&self, out: &mut String, cx: &mut AllocationContext) -> Result<(), FormatFailure> {
        let mut sink = FallibleStringWriter::new(out, cx, AllocationSite::Formatting);
        // CLOSED formatting graph: only this module, existing inline product
        // JSON/allocation errors, primitive numbers/bools, str and str::Debug,
        // plus the specifically audited JsonFloat formatter below.
        let result = write!(&mut sink, "{}", Diagnostic(self));
        sink.finish(result)
    }
}
struct Diagnostic<'a>(&'a AppDecodeError);
impl fmt::Display for Diagnostic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 { AppDecodeError::Json(e)=>write!(f,"{e}"),
            AppDecodeError::Allocation(e)=>write!(f,"{e}"), AppDecodeError::Schema(e)=>write!(f,"{e}") }
    }
}
impl fmt::Display for Expected {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {
        match self {
            Self::Text(s)=>f.write_str(s),
            Self::Structure{name,fields:1}=>write!(f,"struct {name} with 1 element"),
            Self::Structure{name,fields}=>write!(f,"struct {name} with {fields} elements"),
            Self::Variant{name,fields:1}=>write!(f,"struct variant {name} with 1 element"),
            Self::Variant{name,fields}=>write!(f,"struct variant {name} with {fields} elements"),
            Self::VariantIndex(n)=>write!(f,"variant index 0 <= i < {n}"),
            Self::Sequence(1)=>f.write_str("1 element in sequence"),
            Self::Sequence(n)=>write!(f,"{n} elements in sequence"),
        }
    }
}
impl fmt::Display for UnexpectedOwned {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {
        match self {
            Self::Unit=>f.write_str("null"), Self::Bool(v)=>write!(f,"boolean `{v}`"),
            Self::U64(v)=>write!(f,"integer `{v}`"), Self::I64(v)=>write!(f,"integer `{v}`"),
            Self::F64(v)=>write!(f,"floating point `{}`",JsonFloat(*v)),
            Self::String(v)=>write!(f,"string {v:?}"), Self::Sequence=>f.write_str("sequence"), Self::Map=>f.write_str("map"),
        }
    }
}
impl fmt::Display for SchemaFailure {
    fn fmt(&self, f:&mut fmt::Formatter<'_>)->fmt::Result {
        match self {
            Self::MissingField(name)=>write!(f,"missing field `{name}`"),
            Self::UnknownField{key,expected}=>unknown(f,"field",key,expected),
            Self::UnknownVariant{tag,expected}=>unknown(f,"variant",tag,expected),
            Self::InvalidType{actual,expected}=>write!(f,"invalid type: {actual}, expected {expected}"),
            Self::InvalidValue{actual,expected}=>write!(f,"invalid value: {actual}, expected {expected}"),
            Self::InvalidLength{actual,expected}=>write!(f,"invalid length {actual}, expected {expected}"),
        }
    }
}
fn unknown(f:&mut fmt::Formatter<'_>,kind:&str,name:&str,names:&[&str])->fmt::Result {
    write!(f,"unknown {kind} `{name}`, ")?;
    match names {
        []=>write!(f,"there are no {kind}s"),
        [a]=>write!(f,"expected `{a}`"),
        [a,b]=>write!(f,"expected `{a}` or `{b}`"),
        _=>{f.write_str("expected one of ")?;for(i,name)in names.iter().enumerate(){if i>0{f.write_str(", ")?;}write!(f,"`{name}`")?;}Ok(())},
    }
}
// Only this diagnostic path uses serde_json. Qualification REQUIRES the pinned
// default/std build with arbitrary_precision DISABLED: from_f64 then owns an
// inline N::Float; Display streams zmij's fixed 24-byte stack buffer. With
// arbitrary_precision enabled, from_f64 allocates and this guarantee is INVALID.
// This is not a claim about arbitrary Serde formatters or configurations.
// JsonUnexpected uses the same finite formatter; nonfinite values never enter
// Number and have these exact static spellings. No parsing, Value or boxed Error.
struct JsonFloat(f64);
impl fmt::Display for JsonFloat {
    fn fmt(&self,f:&mut fmt::Formatter<'_>)->fmt::Result {
        match serde_json::Number::from_f64(self.0) {
            Some(number)=>fmt::Display::fmt(&number,f),
            None if self.0.is_nan()=>f.write_str("NaN"),
            None if self.0.is_sign_negative()=>f.write_str("-inf"),
            None=>f.write_str("inf"),
        }
    }
}
