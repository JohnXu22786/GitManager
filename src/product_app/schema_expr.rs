//! R05b: four recursive schema types, 37 variants and 78 named fields.
//! Decode only: no semantic validation, legacy conversion, clone or encoding.
use super::decode::{self, sealed, DecodeOwned, InputMode, Fields, fields, finish_sequence};
use super::{AppDecodeError as Error, Expected, SchemaFailure, Id, Type, DataValue, CollectionOperation};
use crate::product_alloc::{AllocationContext, ordered::OrderedMap};
use crate::product_json::Value;

macro_rules! entry {
    ($name:ident)=>{
        impl sealed::Sealed for $name{}
        impl $name {
            /// Structural decode only; successful decoding is NOT validation.
            pub fn decode_json(bytes:&[u8],cx:&mut AllocationContext)->Result<Self,Error>{decode::decode_json(bytes,cx)}
        }
    };
}
macro_rules! variant {
    ($payload:ident,$cx:ident,$name:ident::$variant:ident)=>{{
        $payload.unit()?;Ok($name::$variant)
    }};
    ($payload:ident,$cx:ident,$name:ident::$variant:ident,{$($field:ident:$ty:ty),+})=>{
        fields!($payload,InputMode::BufferedContent,$cx,
            Expected::Variant{name:concat!(stringify!($name),"::",stringify!($variant)),fields:[$(stringify!($field)),+].len()},
            $name::$variant,{$($field:$ty),+})
    };
}
// A concrete declaration and exhaustive decoder branch for every schema case.
// The tag table proves the final unreachable arm; it is not a decode fallback.
macro_rules! internal {
    ($name:ident{$($index:literal:$variant:ident$({$($field:ident:$ty:ty),+})?=>$wire:literal),+$(,)?})=>{
        pub enum $name{$($variant$({$($field:$ty),+})?),+}
        entry!($name);
        impl DecodeOwned for $name{
            fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
                let(tag,payload)=decode::tagged(v,m,&[$($wire),+],concat!("internally tagged enum ",stringify!($name)))?;
                match tag{
                    $($index=>variant!(payload,cx,$name::$variant$(,{$($field:$ty),+})?),)+
                    _=>unreachable!("closed identifier table"),
                }
            }
        }
    };
}
internal!(Expr{
    0:Literal{value_type:Type,value:DataValue}=>"literal",
    1:Variable{name:Id}=>"variable",
    2:Field{record:Box<Expr>,field:Id}=>"field",
    3:State{state:Id}=>"state",
    4:Today=>"today",
    5:Query{entity:Id,binding:Id,predicate:Box<Expr>,sort:Vec<SortKey>,limit:usize,include_archived:bool}=>"query",
    6:Filter{items:Box<Expr>,binding:Id,predicate:Box<Expr>}=>"filter",
    7:Map{items:Box<Expr>,binding:Id,value:Box<Expr>}=>"map",
    8:Count{items:Box<Expr>}=>"count",
    9:Any{items:Box<Expr>,binding:Id,predicate:Box<Expr>}=>"any",
    10:All{items:Box<Expr>,binding:Id,predicate:Box<Expr>}=>"all",
    11:Sum{items:Box<Expr>}=>"sum",
    12:Contains{items:Box<Expr>,value:Box<Expr>}=>"contains",
    13:Equal{left:Box<Expr>,right:Box<Expr>}=>"equal",
    14:Less{left:Box<Expr>,right:Box<Expr>}=>"less",
    15:Add{left:Box<Expr>,right:Box<Expr>}=>"add",
    16:Subtract{left:Box<Expr>,right:Box<Expr>}=>"subtract",
    17:Concat{left:Box<Expr>,right:Box<Expr>}=>"concat",
    18:TextContains{text:Box<Expr>,search:Box<Expr>}=>"text_contains",
    19:Lower{value:Box<Expr>}=>"lower",
    20:And{values:Vec<Expr>}=>"and",
    21:Or{values:Vec<Expr>}=>"or",
    22:Not{value:Box<Expr>}=>"not",
    23:If{condition:Box<Expr>,then_value:Box<Expr>,else_value:Box<Expr>}=>"if",
    24:Coalesce{value:Box<Expr>,fallback:Box<Expr>}=>"coalesce",
    25:DateAdd{date:Box<Expr>,days:Box<Expr>}=>"date_add",
    26:DateDifference{later:Box<Expr>,earlier:Box<Expr>}=>"date_difference",
});
pub struct SortKey{pub value:Expr,pub descending:bool}
entry!(SortKey);
impl DecodeOwned for SortKey{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let fields=Fields::new(v,"struct SortKey")?;
        fields!(fields,m,cx,Expected::Structure{name:"SortKey",fields:2},Self,{value:Expr,descending:bool})
    }
}
internal!(Statement{
    0:Create{entity:Id,values:OrderedMap<Id,Expr>,bind:Id}=>"create",
    1:Update{record:Expr,values:OrderedMap<Id,Expr>}=>"update",
    2:Archive{record:Expr}=>"archive",
    3:SetState{state:Id,value:Expr}=>"set_state",
    4:Collection{target:CollectionTarget,operation:CollectionOperation,items:Expr}=>"collection",
    5:ForEach{items:Expr,binding:Id,steps:Vec<Statement>}=>"for_each",
    6:Assert{condition:Expr,message:String}=>"assert",
    7:Emit{output:Id,items:Expr,binding:Id,columns:OrderedMap<Id,Expr>}=>"emit",
});
internal!(CollectionTarget{
    0:State{state:Id}=>"state",
    1:Field{record:Expr,field:Id}=>"field",
});
