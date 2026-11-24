//! R05a declarations: same schema spellings/scalar widths, product-owned set.
use super::decode::{self, sealed, DecodeOwned, InputMode, Fields, fields, finish_sequence};
use super::{AppDecodeError as Error, Expected, SchemaFailure};
use crate::product_alloc::{AllocationContext, ordered::OrderedSet};
use crate::product_json::Value;
pub type Id=String;
pub enum Type { Boolean, Integer, Text, Date, Reference{entity:Id}, List{item:Box<Type>}, Optional{item:Box<Type>} }
pub enum DataValue {
    Null, Boolean{value:bool}, Integer{value:i64}, Text{value:String}, Date{days:i32},
    Reference{entity:Id,record:Id}, List{item_type:Type,items:Vec<DataValue>},
}
macro_rules! entry {
    ($($name:ident),+)=>{$(
        impl sealed::Sealed for $name{}
        impl $name {
            /// Structural decode only; successful decoding is NOT validation.
            pub fn decode_json(bytes:&[u8],cx:&mut AllocationContext)->Result<Self,Error>{decode::decode_json(bytes,cx)}
        }
    )+};
}
entry!(Type,DataValue);
impl DecodeOwned for Type {
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let(tag,payload)=decode::tagged(v,m,&["boolean","integer","text","date","reference","list","optional"],"internally tagged enum Type")?;
        let m=InputMode::BufferedContent;
        match tag {
            0=>{payload.unit()?;Ok(Self::Boolean)},1=>{payload.unit()?;Ok(Self::Integer)},
            2=>{payload.unit()?;Ok(Self::Text)},3=>{payload.unit()?;Ok(Self::Date)},
            4=>fields!(payload,m,cx,Expected::Variant{name:"Type::Reference",fields:1},Self::Reference,{entity:Id}),
            5=>fields!(payload,m,cx,Expected::Variant{name:"Type::List",fields:1},Self::List,{item:Box<Type>}),
            6=>fields!(payload,m,cx,Expected::Variant{name:"Type::Optional",fields:1},Self::Optional,{item:Box<Type>}),
            _=>unreachable!("closed identifier table"),
        }
    }
}
impl DecodeOwned for DataValue {
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let(tag,payload)=decode::tagged(v,m,&["null","boolean","integer","text","date","reference","list"],"internally tagged enum DataValue")?;
        let m=InputMode::BufferedContent;
        match tag {
            0=>{payload.unit()?;Ok(Self::Null)},
            1=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::Boolean",fields:1},Self::Boolean,{value:bool}),
            2=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::Integer",fields:1},Self::Integer,{value:i64}),
            3=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::Text",fields:1},Self::Text,{value:String}),
            4=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::Date",fields:1},Self::Date,{days:i32}),
            5=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::Reference",fields:2},Self::Reference,{entity:Id,record:Id}),
            6=>fields!(payload,m,cx,Expected::Variant{name:"DataValue::List",fields:2},Self::List,{item_type:Type,items:Vec<DataValue>}),
            _=>unreachable!("closed identifier table"),
        }
    }
}
macro_rules! structure {
    ($name:ident,$count:literal,{$($field:ident:$ty:ty),+})=>{
        pub struct $name{$(pub $field:$ty),+}
        entry!($name);
        impl DecodeOwned for $name{
            fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
                let fields=Fields::new(v,concat!("struct ",stringify!($name)))?;
                fields!(fields,m,cx,Expected::Structure{name:stringify!($name),fields:$count},Self,{$($field:$ty),+})
            }
        }
    };
}
structure!(FieldDefinition,3,{id:Id,label:String,value_type:Type});
structure!(StateControl,4,{id:Id,label:String,state:Id,on_change:Option<Id>});
structure!(KeyBinding,3,{key:String,modifiers:OrderedSet<KeyModifier>,binding:Id});
macro_rules! external {
    ($name:ident{$($variant:ident=>$wire:literal),+})=>{
        #[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
        pub enum $name{$($variant),+}
        entry!($name);
        impl DecodeOwned for $name{
            fn decode(v:Value,m:InputMode,_:&mut AllocationContext)->Result<Self,Error>{
                let tag=decode::external(v,m,&[$($wire),+])?;
                Ok([$(Self::$variant),+][tag])
            }
        }
    };
}
external!(CollectionOperation{Insert=>"insert",Remove=>"remove"});
external!(OutputFormat{Csv=>"csv",Json=>"json"});
external!(ActionPlacement{Toolbar=>"toolbar",Row=>"row"});
external!(KeyModifier{Control=>"control",Alt=>"alt",Shift=>"shift",Command=>"command"});
