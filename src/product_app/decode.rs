//! Closed decoder helpers. Mode is inherited by EVERY descendant; internal
//! tagged payloads switch to BufferedContent exactly once at that boundary.
//! Vec<Value>/map pair owners are consumed, never cloned or rebuilt as Content.
use super::error::{invalid_type, invalid_value};
use super::{AppDecodeError as Error, Expected, SchemaFailure};
use crate::product_alloc::{self, AllocationContext, AllocationSite, ordered::{OrderedMap, OrderedSet}};
use crate::product_json::{self, Number, Value};
pub(super) mod sealed { pub trait Sealed {} }
#[derive(Clone, Copy, Debug)]
pub(crate) enum InputMode { DirectValue, BufferedContent }
pub(crate) trait DecodeOwned: sealed::Sealed + Sized {
    fn decode(v:Value, mode:InputMode, cx:&mut AllocationContext)->Result<Self,Error>;
    fn missing(name:&'static str)->Result<Self,Error>{Err(SchemaFailure::MissingField(name).into())}
}
pub(super) fn decode_json<T:DecodeOwned>(bytes:&[u8],cx:&mut AllocationContext)->Result<T,Error>{
    let value=product_json::parse_json_bytes(bytes,cx).map_err(Error::Json)?;
    T::decode(value,InputMode::DirectValue,cx)
}
#[cfg(test)]
pub(crate) fn decode_value<T:DecodeOwned>(v:Value,mode:InputMode,cx:&mut AllocationContext)->Result<T,Error>{T::decode(v,mode,cx)}

impl sealed::Sealed for String {}
impl DecodeOwned for String {
    fn decode(v:Value,_:InputMode,_:&mut AllocationContext)->Result<Self,Error>{
        match v{Value::String(s)=>Ok(s),v=>Err(invalid_type(v,Expected::Text("a string")))}
    }
}
impl sealed::Sealed for bool {}
impl DecodeOwned for bool {
    fn decode(v:Value,_:InputMode,_:&mut AllocationContext)->Result<Self,Error>{
        match v{Value::Bool(b)=>Ok(b),v=>Err(invalid_type(v,Expected::Text("a boolean")))}
    }
}
macro_rules! integer {
    ($($t:ty),+) => {$ (
        impl sealed::Sealed for $t {}
        impl DecodeOwned for $t {
            fn decode(v:Value,_:InputMode,_:&mut AllocationContext)->Result<Self,Error>{
                let expected=Expected::Text(stringify!($t));
                match v {
                    Value::Number(Number::U64(n))=>Self::try_from(n).map_err(|_|invalid_value(Value::Number(Number::U64(n)),expected)),
                    Value::Number(Number::I64(n))=>Self::try_from(n).map_err(|_|invalid_value(Value::Number(Number::I64(n)),expected)),
                    v=>Err(invalid_type(v,expected)),
                }
            }
        }
    )+};
}
integer!(i64,i32,u32,usize);
impl<T:DecodeOwned> sealed::Sealed for Option<T>{}
impl<T:DecodeOwned> DecodeOwned for Option<T>{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{match v{Value::Null=>Ok(None),v=>T::decode(v,m,cx).map(Some)}}
    fn missing(_:&'static str)->Result<Self,Error>{Ok(None)}
}
impl<T:DecodeOwned> sealed::Sealed for Box<T>{}
impl<T:DecodeOwned> DecodeOwned for Box<T>{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let child=T::decode(v,m,cx)?;Ok(product_alloc::try_box(child,cx)?)
    }
}
impl<T:DecodeOwned> sealed::Sealed for Vec<T>{}
impl<T:DecodeOwned> DecodeOwned for Vec<T>{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let Value::Array(values)=v else{return Err(invalid_type(v,Expected::Text("a sequence")))};
        let mut out=Vec::new();product_alloc::try_reserve(&mut out,values.len(),cx,AllocationSite::Vector)?;
        for v in values{out.push(T::decode(v,m,cx)?);}Ok(out)
    }
}
impl sealed::Sealed for OrderedSet<super::KeyModifier>{}
impl DecodeOwned for OrderedSet<super::KeyModifier>{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let Value::Array(values)=v else{return Err(invalid_type(v,Expected::Text("a sequence")))};
        let mut out=Self::new();
        // Decode all elements before duplicate collapse; no untrusted callbacks.
        for v in values{out.try_insert(super::KeyModifier::decode(v,m,cx)?,cx)?;}Ok(out)
    }
}

// Closed concrete dictionary implementations only. Keys are arbitrary user
// strings (including kind/type/value/default), never schema field names.
macro_rules! dictionary {
    ($($ty:ty),+)=>{$(
        impl sealed::Sealed for OrderedMap<String,$ty>{}
        impl DecodeOwned for OrderedMap<String,$ty>{
            fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
                let Value::Object(values)=v else{return Err(invalid_type(v,Expected::Text("a map")))};
                let mut out=Self::new();out.try_reserve(values.len(),cx)?;
                for(key,value)in values.into_entries(){
                    let value=<$ty as DecodeOwned>::decode(value,m,cx)?;
                    out.try_insert(key,value,cx)?;
                }
                Ok(out)
            }
        }
    )+};
}
dictionary!(super::Expr,super::Type,super::DataValue);

pub(super) enum Fields {
    Map(std::vec::IntoIter<(String,Value)>),
    Seq { values:std::vec::IntoIter<Value>, total:usize },
}
impl Fields {
    pub(super) fn new(v:Value,expected:&'static str)->Result<Self,Error>{
        match v{Value::Object(m)=>Ok(Self::Map(m.into_entries())),Value::Array(v)=>Ok(Self::sequence(v.into_iter())),v=>Err(invalid_type(v,Expected::Text(expected)))}
    }
    fn sequence(values:std::vec::IntoIter<Value>)->Self{let total=values.len();Self::Seq{values,total}}
    pub(super) fn unit(self)->Result<(),Error>{
        match self{Self::Map(_)=>Ok(()),Self::Seq{values,total}=>finish_sequence(values,total,InputMode::BufferedContent)}
    }
}
pub(super) fn finish_sequence(values:std::vec::IntoIter<Value>,total:usize,mode:InputMode)->Result<(),Error>{
    if values.len()==0{return Ok(())}
    let expected=match mode{InputMode::DirectValue=>Expected::Text("fewer elements in array"),InputMode::BufferedContent=>Expected::Sequence(total-values.len())};
    Err(SchemaFailure::InvalidLength{actual:total,expected}.into())
}
pub(super) fn tagged(v:Value,mode:InputMode,names:&'static[&'static str],expected:&'static str)->Result<(usize,Fields),Error>{
    match v {
        Value::Object(mut map)=>{
            let tag=map.remove("kind").ok_or(SchemaFailure::MissingField("kind"))?;
            let tag=identifier(tag,mode,names)?;
            Ok((tag,Fields::Map(map.into_entries())))
        },
        Value::Array(v)=>{
            let mut values=v.into_iter();let tag=values.next().ok_or(SchemaFailure::MissingField("kind"))?;
            let tag=identifier(tag,mode,names)?;Ok((tag,Fields::sequence(values)))
        },
        v=>Err(invalid_type(v,Expected::Text(expected))),
    }
}
fn identifier(v:Value,mode:InputMode,names:&'static[&'static str])->Result<usize,Error>{
    match v {
        Value::String(tag)=>names.iter().position(|s|*s==tag).ok_or_else(||SchemaFailure::UnknownVariant{tag,expected:names}.into()),
        Value::Number(Number::U64(n)) if matches!(mode,InputMode::BufferedContent)=>{
            if n<(names.len() as u64){Ok(n as usize)}else{Err(invalid_value(Value::Number(Number::U64(n)),Expected::VariantIndex(names.len())))}
        },
        v=>Err(invalid_type(v,Expected::Text("variant identifier"))),
    }
}
pub(super) fn external(v:Value,mode:InputMode,names:&'static[&'static str])->Result<usize,Error>{
    match v {
        Value::String(s)=>identifier(Value::String(s),mode,names),
        Value::Object(map)=>{
            if map.len()!=1{return Err(invalid_value(Value::Object(map),Expected::Text("map with a single key")))}
            // Exactly one pair, established above; no input allocation here.
            let (key,payload)=map.into_entries().next().expect("single entry");
            let tag=identifier(Value::String(key),mode,names)?;
            match payload {
                Value::Null=>Ok(tag),
                Value::Object(m) if matches!(mode,InputMode::BufferedContent)&&m.is_empty()=>Ok(tag),
                v=>Err(invalid_type(v,Expected::Text("unit"))),
            }
        },
        v=>Err(invalid_type(v,Expected::Text("string or map"))),
    }
}

// Concrete expansion: each field has a typed Option slot. Sorted map order
// selects the first decode error; declaration order selects missing fields.
// Sequence slots cannot use missing Option defaults. No runtime descriptors,
// dynamic field storage, collect, callbacks, conversion, or temporary Content.
macro_rules! fields {
    ($input:expr,$mode:expr,$cx:expr,$expected:expr,$($ctor:ident)::+,{$($field:ident:$ty:ty),* $(,)?}) => {{
        let mode=$mode;let cx=$cx;
        match $input {
            Fields::Map(entries)=>{
                $(let mut $field:Option<$ty>=None;)*
                for(key,value)in entries{match key.as_str(){
                    $(stringify!($field)=>$field=Some(<$ty as DecodeOwned>::decode(value,mode,cx)?),)*
                    _=>return Err(SchemaFailure::UnknownField{key,expected:&[$(stringify!($field)),*]}.into()),
                }}
                $(let $field=match $field{Some(v)=>v,None=><$ty as DecodeOwned>::missing(stringify!($field))?};)*
                Ok($($ctor)::+{$($field),*})
            },
            Fields::Seq{mut values,total}=>{
                $(let $field=<$ty as DecodeOwned>::decode(values.next().ok_or(SchemaFailure::InvalidLength{actual:total,expected:$expected})?,mode,cx)?;)*
                finish_sequence(values,total,mode)?;Ok($($ctor)::+{$($field),*})
            },
        }
    }};
}
pub(super) use fields;
