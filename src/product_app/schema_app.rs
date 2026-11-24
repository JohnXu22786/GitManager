//! R05c: the remaining twelve schema types, three variants and 62 fields.
//! Concrete owned decoding only. AppDefinition::decode_json returns an
//! UNVALIDATED App; every descendant reuses the syntax intake's context.
use super::decode::{self, sealed, DecodeOwned, InputMode, Fields, fields, finish_sequence};
use super::{AppDecodeError as Error, Expected, SchemaFailure, Id, Type, DataValue,
    Expr, Statement, FieldDefinition, OutputFormat, StateControl, ActionPlacement, KeyBinding};
use crate::product_alloc::{AllocationContext, ordered::OrderedMap};
use crate::product_json::Value;

pub type Values=OrderedMap<Id,DataValue>;
macro_rules! entry {
    ($name:ident)=>{
        impl sealed::Sealed for $name{}
        impl $name {
            /// Structural decode only; the returned schema is UNVALIDATED.
            /// Uses the ordinary product_json policy (1 MiB / 64 containers).
            pub fn decode_json(bytes:&[u8],cx:&mut AllocationContext)->Result<Self,Error>{decode::decode_json(bytes,cx)}
        }
    };
}
macro_rules! structure {
    ($name:ident,{$($field:ident:$ty:ty),+})=>{
        pub struct $name{$(pub $field:$ty),+}
        entry!($name);
        impl DecodeOwned for $name{
            fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
                let fields=Fields::new(v,concat!("struct ",stringify!($name)))?;
                fields!(fields,m,cx,Expected::Structure{name:stringify!($name),fields:[$(stringify!($field)),+].len()},Self,{$($field:$ty),+})
            }
        }
    };
}
structure!(EntityDefinition,{id:Id,label:String,fields:Vec<FieldDefinition>,unique:Vec<Vec<Id>>,constraints:Vec<Expr>});
structure!(StateDefinition,{id:Id,label:String,value_type:Type,initial:DataValue});
structure!(ActionDefinition,{id:Id,label:String,parameters:OrderedMap<Id,Type>,guards:Vec<Expr>,steps:Vec<Statement>,ensures:Vec<Expr>});
structure!(OutputDefinition,{id:Id,label:String,format:OutputFormat,columns:Vec<FieldDefinition>});
structure!(ObservableDefinition,{id:Id,label:String,value:Expr});
structure!(Column,{id:Id,label:String,value:Expr});
structure!(SelectionBinding,{id:Id,state:Id,on_change:Option<Id>});
structure!(FormInput,{parameter:Id,label:String});
pub enum ViewKind {
    List{entity:Id,rows:Expr,columns:Vec<Column>,controls:Vec<StateControl>,selection:Option<SelectionBinding>},
    Detail{entity:Id,record:Expr,columns:Vec<Column>},
    Form{action:Id,fields:Vec<FormInput>,defaults:Values},
}
entry!(ViewKind);
impl DecodeOwned for ViewKind{
    fn decode(v:Value,m:InputMode,cx:&mut AllocationContext)->Result<Self,Error>{
        let(tag,payload)=decode::tagged(v,m,&["list","detail","form"],"internally tagged enum ViewKind")?;
        let m=InputMode::BufferedContent;
        match tag{
            0=>fields!(payload,m,cx,Expected::Variant{name:"ViewKind::List",fields:5},Self::List,
                {entity:Id,rows:Expr,columns:Vec<Column>,controls:Vec<StateControl>,selection:Option<SelectionBinding>}),
            1=>fields!(payload,m,cx,Expected::Variant{name:"ViewKind::Detail",fields:3},Self::Detail,
                {entity:Id,record:Expr,columns:Vec<Column>}),
            2=>fields!(payload,m,cx,Expected::Variant{name:"ViewKind::Form",fields:3},Self::Form,
                {action:Id,fields:Vec<FormInput>,defaults:Values}),
            _=>unreachable!("closed identifier table"),
        }
    }
}
structure!(ActionBinding,{id:Id,label:String,placement:ActionPlacement,action:Id,arguments:OrderedMap<Id,Expr>,enabled:Expr});
structure!(ViewDefinition,{id:Id,label:String,kind:ViewKind,actions:Vec<ActionBinding>,keys:Vec<KeyBinding>});
structure!(AppDefinition,{version:u32,id:Id,label:String,entities:Vec<EntityDefinition>,state:Vec<StateDefinition>,actions:Vec<ActionDefinition>,outputs:Vec<OutputDefinition>,observables:Vec<ObservableDefinition>,views:Vec<ViewDefinition>,initial_view:Id});
