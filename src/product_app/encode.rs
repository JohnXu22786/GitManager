//! Exact old canonical bytes of all closed App types, without a Value/Serde
//! intermediate, copying the App, validation, projection, routing, or new caps.
//! Both passes share the same lexical field/index walk. Only active container
//! cursors are retained: O(depth) borrowed frames, geometric fallible growth in
//! measurement, one output reservation, and no growth in emission. Local Drop
//! is shallow; eventual source-owner Drop remains the caller's responsibility.
//! O(nodes + key/string bytes + encoded bytes), including zero initialization,
//! both numeric/string scans, frame relocation, error returns and local release.
//! Predecessor funding must cover this work; measured size grants no credit.
//! Number helpers require the existing pinned default/std serde_json feature
//! qualification. usize follows old Serialize's as-u64 spelling on 32/64 bits.
use super::*;
use crate::product_alloc::{try_reserve,AllocationContext,AllocationSite,
    ordered::{OrderedMap,OrderedSet}};
use crate::product_json::{Number,canonical::{EncodeError,Invariant,Sink,string,number,reserve_output}};
use std::mem;

// All data are immutable borrows or copied scalars. No owning descendants,
// callback traits, dynamic schema, sibling stack, or recursive dispatch.
#[derive(Clone,Copy)]
enum Node<'a>{
 Null,Str(&'a str),Bool(bool),Signed(i64),Unsigned(u64),
 Type(&'a Type),
 DataValue(&'a DataValue),
 Expr(&'a Expr),
 SortKey(&'a SortKey),
 FieldDefinition(&'a FieldDefinition),
 EntityDefinition(&'a EntityDefinition),
 StateDefinition(&'a StateDefinition),
 ActionDefinition(&'a ActionDefinition),
 Statement(&'a Statement),
 CollectionTarget(&'a CollectionTarget),
 CollectionOperation(&'a CollectionOperation),
 OutputFormat(&'a OutputFormat),
 OutputDefinition(&'a OutputDefinition),
 ObservableDefinition(&'a ObservableDefinition),
 Column(&'a Column),
 StateControl(&'a StateControl),
 SelectionBinding(&'a SelectionBinding),
 FormInput(&'a FormInput),
 ViewKind(&'a ViewKind),
 ActionPlacement(&'a ActionPlacement),
 ActionBinding(&'a ActionBinding),
 KeyModifier(&'a KeyModifier),
 KeyBinding(&'a KeyBinding),
 ViewDefinition(&'a ViewDefinition),
 AppDefinition(&'a AppDefinition),
 VecDataValue(&'a [DataValue]),
 VecFieldDefinition(&'a [FieldDefinition]),
 VecStrings(&'a [Vec<Id>]),
 VecId(&'a [Id]),
 VecExpr(&'a [Expr]),
 VecSortKey(&'a [SortKey]),
 VecStatement(&'a [Statement]),
 VecColumn(&'a [Column]),
 VecStateControl(&'a [StateControl]),
 VecFormInput(&'a [FormInput]),
 VecActionBinding(&'a [ActionBinding]),
 VecKeyBinding(&'a [KeyBinding]),
 VecEntityDefinition(&'a [EntityDefinition]),
 VecStateDefinition(&'a [StateDefinition]),
 VecActionDefinition(&'a [ActionDefinition]),
 VecOutputDefinition(&'a [OutputDefinition]),
 VecObservableDefinition(&'a [ObservableDefinition]),
 VecViewDefinition(&'a [ViewDefinition]),
 MapType(&'a OrderedMap<String,Type>),
 MapExpr(&'a OrderedMap<String,Expr>),
 MapData(&'a OrderedMap<String,DataValue>),
 Modifiers(&'a OrderedSet<KeyModifier>),
}
struct Frame<'a>{node:Node<'a>,next:usize}
#[cfg(test)]pub(crate) const FRAME_BYTES:usize=mem::size_of::<Frame<'static>>();
fn optional_id(v:&Option<Id>)->Node<'_>{match v{Some(v)=>Node::Str(v),None=>Node::Null}}
fn optional_selection(v:&Option<SelectionBinding>)->Node<'_>{match v{Some(v)=>Node::SelectionBinding(v),None=>Node::Null}}
fn collectionoperation(v:&CollectionOperation)->&'static str{match v{CollectionOperation::Insert=>"insert",CollectionOperation::Remove=>"remove"}}
fn outputformat(v:&OutputFormat)->&'static str{match v{OutputFormat::Csv=>"csv",OutputFormat::Json=>"json"}}
fn actionplacement(v:&ActionPlacement)->&'static str{match v{ActionPlacement::Toolbar=>"toolbar",ActionPlacement::Row=>"row"}}
fn keymodifier(v:&KeyModifier)->&'static str{match v{KeyModifier::Control=>"control",KeyModifier::Alt=>"alt",KeyModifier::Shift=>"shift",KeyModifier::Command=>"command"}}
impl<'a> Node<'a>{
 fn atom(self,sink:&mut Sink<'_>)->Result<bool,EncodeError>{
  match self{
   Self::Null=>sink.append(b"null")?,Self::Str(v)=>string(v,sink)?,
   Self::Bool(true)=>sink.append(b"true")?,Self::Bool(false)=>sink.append(b"false")?,
   Self::Signed(v)=>number(&Number::I64(v),sink)?,Self::Unsigned(v)=>number(&Number::U64(v),sink)?,
   Self::CollectionOperation(v)=>string(collectionoperation(v),sink)?,
   Self::OutputFormat(v)=>string(outputformat(v),sink)?,
   Self::ActionPlacement(v)=>string(actionplacement(v),sink)?,
   Self::KeyModifier(v)=>string(keymodifier(v),sink)?,
   Self::Modifiers(values)=>{
    sink.append(b"[")?;
    // OrderedSet iterates the four closed variants in declaration/Ord order,
    // not lexical wire-string order. No nth rescan or allocation.
    for(i,value)in values.iter().enumerate(){if i>0{sink.append(b",")?;}string(keymodifier(value),sink)?;}
    sink.append(b"]")?;
   }
   Self::SortKey(_)|Self::FieldDefinition(_)|Self::EntityDefinition(_)|Self::StateDefinition(_)|Self::ActionDefinition(_)|Self::OutputDefinition(_)|Self::ObservableDefinition(_)|Self::Column(_)|Self::StateControl(_)|Self::SelectionBinding(_)|Self::FormInput(_)|Self::ActionBinding(_)|Self::KeyBinding(_)|Self::ViewDefinition(_)|Self::AppDefinition(_)|Self::Type(_)|Self::DataValue(_)|Self::Expr(_)|Self::Statement(_)|Self::CollectionTarget(_)|Self::ViewKind(_)|Self::VecDataValue(_)|Self::VecFieldDefinition(_)|Self::VecStrings(_)|Self::VecId(_)|Self::VecExpr(_)|Self::VecSortKey(_)|Self::VecStatement(_)|Self::VecColumn(_)|Self::VecStateControl(_)|Self::VecFormInput(_)|Self::VecActionBinding(_)|Self::VecKeyBinding(_)|Self::VecEntityDefinition(_)|Self::VecStateDefinition(_)|Self::VecActionDefinition(_)|Self::VecOutputDefinition(_)|Self::VecObservableDefinition(_)|Self::VecViewDefinition(_)|Self::MapType(_)|Self::MapExpr(_)|Self::MapData(_)=>return Ok(false),
  }Ok(true)
 }
 fn object(self)->bool{match self{
  Self::SortKey(_)|Self::FieldDefinition(_)|Self::EntityDefinition(_)|Self::StateDefinition(_)|Self::ActionDefinition(_)|Self::OutputDefinition(_)|Self::ObservableDefinition(_)|Self::Column(_)|Self::StateControl(_)|Self::SelectionBinding(_)|Self::FormInput(_)|Self::ActionBinding(_)|Self::KeyBinding(_)|Self::ViewDefinition(_)|Self::AppDefinition(_)|Self::Type(_)|Self::DataValue(_)|Self::Expr(_)|Self::Statement(_)|Self::CollectionTarget(_)|Self::ViewKind(_)|Self::MapType(_)|Self::MapExpr(_)|Self::MapData(_)=>true,
  Self::VecDataValue(_)|Self::VecFieldDefinition(_)|Self::VecStrings(_)|Self::VecId(_)|Self::VecExpr(_)|Self::VecSortKey(_)|Self::VecStatement(_)|Self::VecColumn(_)|Self::VecStateControl(_)|Self::VecFormInput(_)|Self::VecActionBinding(_)|Self::VecKeyBinding(_)|Self::VecEntityDefinition(_)|Self::VecStateDefinition(_)|Self::VecActionDefinition(_)|Self::VecOutputDefinition(_)|Self::VecObservableDefinition(_)|Self::VecViewDefinition(_)|Self::CollectionOperation(_)|Self::OutputFormat(_)|Self::ActionPlacement(_)|Self::KeyModifier(_)|Self::Str(_)|Self::Bool(_)|Self::Signed(_)|Self::Unsigned(_)|Self::Modifiers(_)|Self::Null=>false,
 }}
 // Static objects have at most ten entries, selected directly by index. Exact
 // destructuring makes field additions fail compilation instead of disappearing.
 // The SAME mapping is used in measurement and emission.
 fn child_at(self,index:usize)->Option<(Option<&'a str>,Node<'a>)>{match self{
  Self::SortKey(v)=>{let SortKey{descending,value}=v;
   match index{
    0=>Some((Some("descending"),Node::Bool(*descending))),
    1=>Some((Some("value"),Node::Expr(value))),
    _=>None,
   }
  }
  Self::FieldDefinition(v)=>{let FieldDefinition{id,label,value_type}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("label"),Node::Str(label))),
    2=>Some((Some("value_type"),Node::Type(value_type))),
    _=>None,
   }
  }
  Self::EntityDefinition(v)=>{let EntityDefinition{constraints,fields,id,label,unique}=v;
   match index{
    0=>Some((Some("constraints"),Node::VecExpr(constraints))),
    1=>Some((Some("fields"),Node::VecFieldDefinition(fields))),
    2=>Some((Some("id"),Node::Str(id))),
    3=>Some((Some("label"),Node::Str(label))),
    4=>Some((Some("unique"),Node::VecStrings(unique))),
    _=>None,
   }
  }
  Self::StateDefinition(v)=>{let StateDefinition{id,initial,label,value_type}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("initial"),Node::DataValue(initial))),
    2=>Some((Some("label"),Node::Str(label))),
    3=>Some((Some("value_type"),Node::Type(value_type))),
    _=>None,
   }
  }
  Self::ActionDefinition(v)=>{let ActionDefinition{ensures,guards,id,label,parameters,steps}=v;
   match index{
    0=>Some((Some("ensures"),Node::VecExpr(ensures))),
    1=>Some((Some("guards"),Node::VecExpr(guards))),
    2=>Some((Some("id"),Node::Str(id))),
    3=>Some((Some("label"),Node::Str(label))),
    4=>Some((Some("parameters"),Node::MapType(parameters))),
    5=>Some((Some("steps"),Node::VecStatement(steps))),
    _=>None,
   }
  }
  Self::OutputDefinition(v)=>{let OutputDefinition{columns,format,id,label}=v;
   match index{
    0=>Some((Some("columns"),Node::VecFieldDefinition(columns))),
    1=>Some((Some("format"),Node::OutputFormat(format))),
    2=>Some((Some("id"),Node::Str(id))),
    3=>Some((Some("label"),Node::Str(label))),
    _=>None,
   }
  }
  Self::ObservableDefinition(v)=>{let ObservableDefinition{id,label,value}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("label"),Node::Str(label))),
    2=>Some((Some("value"),Node::Expr(value))),
    _=>None,
   }
  }
  Self::Column(v)=>{let Column{id,label,value}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("label"),Node::Str(label))),
    2=>Some((Some("value"),Node::Expr(value))),
    _=>None,
   }
  }
  Self::StateControl(v)=>{let StateControl{id,label,on_change,state}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("label"),Node::Str(label))),
    2=>Some((Some("on_change"),optional_id(on_change))),
    3=>Some((Some("state"),Node::Str(state))),
    _=>None,
   }
  }
  Self::SelectionBinding(v)=>{let SelectionBinding{id,on_change,state}=v;
   match index{
    0=>Some((Some("id"),Node::Str(id))),
    1=>Some((Some("on_change"),optional_id(on_change))),
    2=>Some((Some("state"),Node::Str(state))),
    _=>None,
   }
  }
  Self::FormInput(v)=>{let FormInput{label,parameter}=v;
   match index{
    0=>Some((Some("label"),Node::Str(label))),
    1=>Some((Some("parameter"),Node::Str(parameter))),
    _=>None,
   }
  }
  Self::ActionBinding(v)=>{let ActionBinding{action,arguments,enabled,id,label,placement}=v;
   match index{
    0=>Some((Some("action"),Node::Str(action))),
    1=>Some((Some("arguments"),Node::MapExpr(arguments))),
    2=>Some((Some("enabled"),Node::Expr(enabled))),
    3=>Some((Some("id"),Node::Str(id))),
    4=>Some((Some("label"),Node::Str(label))),
    5=>Some((Some("placement"),Node::ActionPlacement(placement))),
    _=>None,
   }
  }
  Self::KeyBinding(v)=>{let KeyBinding{binding,key,modifiers}=v;
   match index{
    0=>Some((Some("binding"),Node::Str(binding))),
    1=>Some((Some("key"),Node::Str(key))),
    2=>Some((Some("modifiers"),Node::Modifiers(modifiers))),
    _=>None,
   }
  }
  Self::ViewDefinition(v)=>{let ViewDefinition{actions,id,keys,kind,label}=v;
   match index{
    0=>Some((Some("actions"),Node::VecActionBinding(actions))),
    1=>Some((Some("id"),Node::Str(id))),
    2=>Some((Some("keys"),Node::VecKeyBinding(keys))),
    3=>Some((Some("kind"),Node::ViewKind(kind))),
    4=>Some((Some("label"),Node::Str(label))),
    _=>None,
   }
  }
  Self::AppDefinition(v)=>{let AppDefinition{actions,entities,id,initial_view,label,observables,outputs,state,version,views}=v;
   match index{
    0=>Some((Some("actions"),Node::VecActionDefinition(actions))),
    1=>Some((Some("entities"),Node::VecEntityDefinition(entities))),
    2=>Some((Some("id"),Node::Str(id))),
    3=>Some((Some("initial_view"),Node::Str(initial_view))),
    4=>Some((Some("label"),Node::Str(label))),
    5=>Some((Some("observables"),Node::VecObservableDefinition(observables))),
    6=>Some((Some("outputs"),Node::VecOutputDefinition(outputs))),
    7=>Some((Some("state"),Node::VecStateDefinition(state))),
    8=>Some((Some("version"),Node::Unsigned(u64::from(*version)))),
    9=>Some((Some("views"),Node::VecViewDefinition(views))),
    _=>None,
   }
  }
  Self::Type(v)=>match v{
   Type::Boolean=>match index{
     0=>Some((Some("kind"),Node::Str("boolean"))),
     _=>None,
    },
   Type::Integer=>match index{
     0=>Some((Some("kind"),Node::Str("integer"))),
     _=>None,
    },
   Type::Text=>match index{
     0=>Some((Some("kind"),Node::Str("text"))),
     _=>None,
    },
   Type::Date=>match index{
     0=>Some((Some("kind"),Node::Str("date"))),
     _=>None,
    },
   Type::Reference{entity}=>match index{
     0=>Some((Some("entity"),Node::Str(entity))),
     1=>Some((Some("kind"),Node::Str("reference"))),
     _=>None,
    },
   Type::List{item}=>match index{
     0=>Some((Some("item"),Node::Type(item))),
     1=>Some((Some("kind"),Node::Str("list"))),
     _=>None,
    },
   Type::Optional{item}=>match index{
     0=>Some((Some("item"),Node::Type(item))),
     1=>Some((Some("kind"),Node::Str("optional"))),
     _=>None,
    },
  },
  Self::DataValue(v)=>match v{
   DataValue::Null=>match index{
     0=>Some((Some("kind"),Node::Str("null"))),
     _=>None,
    },
   DataValue::Boolean{value}=>match index{
     0=>Some((Some("kind"),Node::Str("boolean"))),
     1=>Some((Some("value"),Node::Bool(*value))),
     _=>None,
    },
   DataValue::Integer{value}=>match index{
     0=>Some((Some("kind"),Node::Str("integer"))),
     1=>Some((Some("value"),Node::Signed(*value))),
     _=>None,
    },
   DataValue::Text{value}=>match index{
     0=>Some((Some("kind"),Node::Str("text"))),
     1=>Some((Some("value"),Node::Str(value))),
     _=>None,
    },
   DataValue::Date{days}=>match index{
     0=>Some((Some("days"),Node::Signed(i64::from(*days)))),
     1=>Some((Some("kind"),Node::Str("date"))),
     _=>None,
    },
   DataValue::Reference{entity,record}=>match index{
     0=>Some((Some("entity"),Node::Str(entity))),
     1=>Some((Some("kind"),Node::Str("reference"))),
     2=>Some((Some("record"),Node::Str(record))),
     _=>None,
    },
   DataValue::List{item_type,items}=>match index{
     0=>Some((Some("item_type"),Node::Type(item_type))),
     1=>Some((Some("items"),Node::VecDataValue(items))),
     2=>Some((Some("kind"),Node::Str("list"))),
     _=>None,
    },
  },
  Self::Expr(v)=>match v{
   Expr::Literal{value_type,value}=>match index{
     0=>Some((Some("kind"),Node::Str("literal"))),
     1=>Some((Some("value"),Node::DataValue(value))),
     2=>Some((Some("value_type"),Node::Type(value_type))),
     _=>None,
    },
   Expr::Variable{name}=>match index{
     0=>Some((Some("kind"),Node::Str("variable"))),
     1=>Some((Some("name"),Node::Str(name))),
     _=>None,
    },
   Expr::Field{field,record}=>match index{
     0=>Some((Some("field"),Node::Str(field))),
     1=>Some((Some("kind"),Node::Str("field"))),
     2=>Some((Some("record"),Node::Expr(record))),
     _=>None,
    },
   Expr::State{state}=>match index{
     0=>Some((Some("kind"),Node::Str("state"))),
     1=>Some((Some("state"),Node::Str(state))),
     _=>None,
    },
   Expr::Today=>match index{
     0=>Some((Some("kind"),Node::Str("today"))),
     _=>None,
    },
   Expr::Query{binding,entity,predicate,sort,limit,include_archived}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("entity"),Node::Str(entity))),
     2=>Some((Some("include_archived"),Node::Bool(*include_archived))),
     3=>Some((Some("kind"),Node::Str("query"))),
     4=>Some((Some("limit"),Node::Unsigned(*limit as u64))),
     5=>Some((Some("predicate"),Node::Expr(predicate))),
     6=>Some((Some("sort"),Node::VecSortKey(sort))),
     _=>None,
    },
   Expr::Filter{binding,items,predicate}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("items"),Node::Expr(items))),
     2=>Some((Some("kind"),Node::Str("filter"))),
     3=>Some((Some("predicate"),Node::Expr(predicate))),
     _=>None,
    },
   Expr::Map{binding,items,value}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("items"),Node::Expr(items))),
     2=>Some((Some("kind"),Node::Str("map"))),
     3=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Expr::Count{items}=>match index{
     0=>Some((Some("items"),Node::Expr(items))),
     1=>Some((Some("kind"),Node::Str("count"))),
     _=>None,
    },
   Expr::Any{binding,items,predicate}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("items"),Node::Expr(items))),
     2=>Some((Some("kind"),Node::Str("any"))),
     3=>Some((Some("predicate"),Node::Expr(predicate))),
     _=>None,
    },
   Expr::All{binding,items,predicate}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("items"),Node::Expr(items))),
     2=>Some((Some("kind"),Node::Str("all"))),
     3=>Some((Some("predicate"),Node::Expr(predicate))),
     _=>None,
    },
   Expr::Sum{items}=>match index{
     0=>Some((Some("items"),Node::Expr(items))),
     1=>Some((Some("kind"),Node::Str("sum"))),
     _=>None,
    },
   Expr::Contains{items,value}=>match index{
     0=>Some((Some("items"),Node::Expr(items))),
     1=>Some((Some("kind"),Node::Str("contains"))),
     2=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Expr::Equal{left,right}=>match index{
     0=>Some((Some("kind"),Node::Str("equal"))),
     1=>Some((Some("left"),Node::Expr(left))),
     2=>Some((Some("right"),Node::Expr(right))),
     _=>None,
    },
   Expr::Less{left,right}=>match index{
     0=>Some((Some("kind"),Node::Str("less"))),
     1=>Some((Some("left"),Node::Expr(left))),
     2=>Some((Some("right"),Node::Expr(right))),
     _=>None,
    },
   Expr::Add{left,right}=>match index{
     0=>Some((Some("kind"),Node::Str("add"))),
     1=>Some((Some("left"),Node::Expr(left))),
     2=>Some((Some("right"),Node::Expr(right))),
     _=>None,
    },
   Expr::Subtract{left,right}=>match index{
     0=>Some((Some("kind"),Node::Str("subtract"))),
     1=>Some((Some("left"),Node::Expr(left))),
     2=>Some((Some("right"),Node::Expr(right))),
     _=>None,
    },
   Expr::Concat{left,right}=>match index{
     0=>Some((Some("kind"),Node::Str("concat"))),
     1=>Some((Some("left"),Node::Expr(left))),
     2=>Some((Some("right"),Node::Expr(right))),
     _=>None,
    },
   Expr::TextContains{search,text}=>match index{
     0=>Some((Some("kind"),Node::Str("text_contains"))),
     1=>Some((Some("search"),Node::Expr(search))),
     2=>Some((Some("text"),Node::Expr(text))),
     _=>None,
    },
   Expr::Lower{value}=>match index{
     0=>Some((Some("kind"),Node::Str("lower"))),
     1=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Expr::And{values}=>match index{
     0=>Some((Some("kind"),Node::Str("and"))),
     1=>Some((Some("values"),Node::VecExpr(values))),
     _=>None,
    },
   Expr::Or{values}=>match index{
     0=>Some((Some("kind"),Node::Str("or"))),
     1=>Some((Some("values"),Node::VecExpr(values))),
     _=>None,
    },
   Expr::Not{value}=>match index{
     0=>Some((Some("kind"),Node::Str("not"))),
     1=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Expr::If{condition,else_value,then_value}=>match index{
     0=>Some((Some("condition"),Node::Expr(condition))),
     1=>Some((Some("else_value"),Node::Expr(else_value))),
     2=>Some((Some("kind"),Node::Str("if"))),
     3=>Some((Some("then_value"),Node::Expr(then_value))),
     _=>None,
    },
   Expr::Coalesce{fallback,value}=>match index{
     0=>Some((Some("fallback"),Node::Expr(fallback))),
     1=>Some((Some("kind"),Node::Str("coalesce"))),
     2=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Expr::DateAdd{date,days}=>match index{
     0=>Some((Some("date"),Node::Expr(date))),
     1=>Some((Some("days"),Node::Expr(days))),
     2=>Some((Some("kind"),Node::Str("date_add"))),
     _=>None,
    },
   Expr::DateDifference{earlier,later}=>match index{
     0=>Some((Some("earlier"),Node::Expr(earlier))),
     1=>Some((Some("kind"),Node::Str("date_difference"))),
     2=>Some((Some("later"),Node::Expr(later))),
     _=>None,
    },
  },
  Self::Statement(v)=>match v{
   Statement::Create{bind,entity,values}=>match index{
     0=>Some((Some("bind"),Node::Str(bind))),
     1=>Some((Some("entity"),Node::Str(entity))),
     2=>Some((Some("kind"),Node::Str("create"))),
     3=>Some((Some("values"),Node::MapExpr(values))),
     _=>None,
    },
   Statement::Update{record,values}=>match index{
     0=>Some((Some("kind"),Node::Str("update"))),
     1=>Some((Some("record"),Node::Expr(record))),
     2=>Some((Some("values"),Node::MapExpr(values))),
     _=>None,
    },
   Statement::Archive{record}=>match index{
     0=>Some((Some("kind"),Node::Str("archive"))),
     1=>Some((Some("record"),Node::Expr(record))),
     _=>None,
    },
   Statement::SetState{state,value}=>match index{
     0=>Some((Some("kind"),Node::Str("set_state"))),
     1=>Some((Some("state"),Node::Str(state))),
     2=>Some((Some("value"),Node::Expr(value))),
     _=>None,
    },
   Statement::Collection{items,operation,target}=>match index{
     0=>Some((Some("items"),Node::Expr(items))),
     1=>Some((Some("kind"),Node::Str("collection"))),
     2=>Some((Some("operation"),Node::CollectionOperation(operation))),
     3=>Some((Some("target"),Node::CollectionTarget(target))),
     _=>None,
    },
   Statement::ForEach{binding,items,steps}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("items"),Node::Expr(items))),
     2=>Some((Some("kind"),Node::Str("for_each"))),
     3=>Some((Some("steps"),Node::VecStatement(steps))),
     _=>None,
    },
   Statement::Assert{message,condition}=>match index{
     0=>Some((Some("condition"),Node::Expr(condition))),
     1=>Some((Some("kind"),Node::Str("assert"))),
     2=>Some((Some("message"),Node::Str(message))),
     _=>None,
    },
   Statement::Emit{binding,output,items,columns}=>match index{
     0=>Some((Some("binding"),Node::Str(binding))),
     1=>Some((Some("columns"),Node::MapExpr(columns))),
     2=>Some((Some("items"),Node::Expr(items))),
     3=>Some((Some("kind"),Node::Str("emit"))),
     4=>Some((Some("output"),Node::Str(output))),
     _=>None,
    },
  },
  Self::CollectionTarget(v)=>match v{
   CollectionTarget::State{state}=>match index{
     0=>Some((Some("kind"),Node::Str("state"))),
     1=>Some((Some("state"),Node::Str(state))),
     _=>None,
    },
   CollectionTarget::Field{field,record}=>match index{
     0=>Some((Some("field"),Node::Str(field))),
     1=>Some((Some("kind"),Node::Str("field"))),
     2=>Some((Some("record"),Node::Expr(record))),
     _=>None,
    },
  },
  Self::ViewKind(v)=>match v{
   ViewKind::List{entity,rows,columns,controls,selection}=>match index{
     0=>Some((Some("columns"),Node::VecColumn(columns))),
     1=>Some((Some("controls"),Node::VecStateControl(controls))),
     2=>Some((Some("entity"),Node::Str(entity))),
     3=>Some((Some("kind"),Node::Str("list"))),
     4=>Some((Some("rows"),Node::Expr(rows))),
     5=>Some((Some("selection"),optional_selection(selection))),
     _=>None,
    },
   ViewKind::Detail{entity,record,columns}=>match index{
     0=>Some((Some("columns"),Node::VecColumn(columns))),
     1=>Some((Some("entity"),Node::Str(entity))),
     2=>Some((Some("kind"),Node::Str("detail"))),
     3=>Some((Some("record"),Node::Expr(record))),
     _=>None,
    },
   ViewKind::Form{action,fields,defaults}=>match index{
     0=>Some((Some("action"),Node::Str(action))),
     1=>Some((Some("defaults"),Node::MapData(defaults))),
     2=>Some((Some("fields"),Node::VecFormInput(fields))),
     3=>Some((Some("kind"),Node::Str("form"))),
     _=>None,
    },
  },
  Self::VecDataValue(values)=>values.get(index).map(|v|(None,Node::DataValue(v))),
  Self::VecFieldDefinition(values)=>values.get(index).map(|v|(None,Node::FieldDefinition(v))),
  Self::VecStrings(values)=>values.get(index).map(|v|(None,Node::VecId(v))),
  Self::VecId(values)=>values.get(index).map(|v|(None,Node::Str(v))),
  Self::VecExpr(values)=>values.get(index).map(|v|(None,Node::Expr(v))),
  Self::VecSortKey(values)=>values.get(index).map(|v|(None,Node::SortKey(v))),
  Self::VecStatement(values)=>values.get(index).map(|v|(None,Node::Statement(v))),
  Self::VecColumn(values)=>values.get(index).map(|v|(None,Node::Column(v))),
  Self::VecStateControl(values)=>values.get(index).map(|v|(None,Node::StateControl(v))),
  Self::VecFormInput(values)=>values.get(index).map(|v|(None,Node::FormInput(v))),
  Self::VecActionBinding(values)=>values.get(index).map(|v|(None,Node::ActionBinding(v))),
  Self::VecKeyBinding(values)=>values.get(index).map(|v|(None,Node::KeyBinding(v))),
  Self::VecEntityDefinition(values)=>values.get(index).map(|v|(None,Node::EntityDefinition(v))),
  Self::VecStateDefinition(values)=>values.get(index).map(|v|(None,Node::StateDefinition(v))),
  Self::VecActionDefinition(values)=>values.get(index).map(|v|(None,Node::ActionDefinition(v))),
  Self::VecOutputDefinition(values)=>values.get(index).map(|v|(None,Node::OutputDefinition(v))),
  Self::VecObservableDefinition(values)=>values.get(index).map(|v|(None,Node::ObservableDefinition(v))),
  Self::VecViewDefinition(values)=>values.get(index).map(|v|(None,Node::ViewDefinition(v))),
  Self::MapType(values)=>values.get_index(index).map(|(k,v)|(Some(k.as_str()),Node::Type(v))),
  Self::MapExpr(values)=>values.get_index(index).map(|(k,v)|(Some(k.as_str()),Node::Expr(v))),
  Self::MapData(values)=>values.get_index(index).map(|(k,v)|(Some(k.as_str()),Node::DataValue(v))),
  Self::CollectionOperation(_)|Self::OutputFormat(_)|Self::ActionPlacement(_)|Self::KeyModifier(_)|Self::Str(_)|Self::Bool(_)|Self::Signed(_)|Self::Unsigned(_)|Self::Modifiers(_)|Self::Null=>None,
 }}
}
fn growth_target(capacity:usize,required:usize,layout_max:usize)->usize{
 required.max(capacity.saturating_mul(2).min(layout_max))
}
fn advance(index:usize)->Result<usize,EncodeError>{index.checked_add(1).ok_or(EncodeError::Invariant(Invariant::Traversal))}
fn push_frame<'a>(stack:&mut Vec<Frame<'a>>,frame:Frame<'a>,measure:bool,cx:&mut AllocationContext)->Result<(),EncodeError>{
 if stack.len()==stack.capacity() && measure{
  let required=advance(stack.len())?;
  // Clamp desired doubling, never the feasible required next slot. Foundation
  // extent records CapacityOverflow at the real layout boundary, not a depth cap.
  let target=growth_target(stack.capacity(),required,isize::MAX as usize/mem::size_of::<Frame<'a>>());
  try_reserve(stack,target-stack.len(),cx,AllocationSite::Vector)?;
 }
 if stack.len()>=stack.capacity(){return Err(EncodeError::Invariant(Invariant::WorkspaceCapacity));}
 stack.push(frame);Ok(()) // Checked capacity: emission cannot silently grow.
}
fn walk<'a>(root:Node<'a>,stack:&mut Vec<Frame<'a>>,sink:&mut Sink<'_>,cx:&mut AllocationContext)->Result<(),EncodeError>{
 if !stack.is_empty(){return Err(EncodeError::Invariant(Invariant::Traversal));}
 let mut current=Some(root);
 loop{
  if let Some(node)=current.take(){
   if !node.atom(sink)?{
    let object=node.object();
    if node.child_at(0).is_none(){sink.append(if object{b"{}"}else{b"[]"})?;}
    else{sink.append(if object{b"{"}else{b"["})?;push_frame(stack,Frame{node,next:0},sink.measuring(),cx)?;}
   }
  }
  loop{
   let Some(frame)=stack.last_mut() else{return Ok(());};
   if let Some((key,child))=frame.node.child_at(frame.next){
    if frame.next>0{sink.append(b",")?;}
    if let Some(key)=key{string(key,sink)?;sink.append(b":")?;}
    frame.next=advance(frame.next)?;current=Some(child);break;
   }
   sink.append(if frame.node.object(){b"}"}else{b"]"})?;stack.pop();
  }
 }
}
fn encode(root:Node<'_>,cx:&mut AllocationContext)->Result<Vec<u8>,EncodeError>{
 let mut stack=Vec::new();let mut measure=Sink::measure();walk(root,&mut stack,&mut measure,cx)?;
 let length=measure.position();let mut output=reserve_output(length,cx)?;
 let mut emit=Sink::emit(&mut output);walk(root,&mut stack,&mut emit,cx)?;emit.finish(length)?;
 if !stack.is_empty(){return Err(EncodeError::Invariant(Invariant::Traversal));}
 Ok(output)
}
macro_rules! entry{($($t:ident),+)=>{$(impl $t{
 /// Exact compact, lexically keyed old canonical payload; no validation or
 /// semantic-label projection. Borrows all source owners, with recoverable
 /// local allocation errors and shallow cleanup. Does not bound source Drop,
 /// grant work credit, hash, publish, or route an existing product caller.
 pub fn try_canonical_bytes(&self,cx:&mut AllocationContext)->Result<Vec<u8>,EncodeError>{encode(Node::$t(self),cx)}
})+};}
entry!(Type,DataValue,Expr,SortKey,FieldDefinition,EntityDefinition,StateDefinition,ActionDefinition,Statement,CollectionTarget,CollectionOperation,OutputFormat,OutputDefinition,ObservableDefinition,Column,StateControl,SelectionBinding,FormInput,ViewKind,ActionPlacement,ActionBinding,KeyModifier,KeyBinding,ViewDefinition,AppDefinition);

#[cfg(test)]
mod tests {
 use super::*;
 use crate::product_alloc::{AllocationFailureKind,AllocationFailure};
 #[test]fn geometry_and_cursor_boundaries_do_not_add_policy_limits(){
  for(c,r,m,e)in [(0,1,7,1),(1,2,7,2),(4,5,7,7),(7,8,7,8),(usize::MAX/2+1,usize::MAX/2+2,usize::MAX,usize::MAX)]{assert_eq!(growth_target(c,r,m),e)}
  assert_eq!(advance(usize::MAX),Err(EncodeError::Invariant(Invariant::Traversal)));
  let mut cx=AllocationContext::new();let err=reserve_output(isize::MAX as usize+1,&mut cx).unwrap_err();
  assert_eq!(err,EncodeError::Allocation(AllocationFailure{kind:AllocationFailureKind::CapacityOverflow,site:AllocationSite::Bytes,requested_bytes:Some(isize::MAX as usize+1)}));assert_eq!(cx.growth_requests(),0);
 }
 #[test]fn emission_reuses_workspace_and_exact_output_without_growth(){
  let source=Type::List{item:Box::new(Type::Reference{entity:String::from("e")})};let mut stack=Vec::new();let mut cx=AllocationContext::new();let mut measure=Sink::measure();
  walk(Node::Type(&source),&mut stack,&mut measure,&mut cx).unwrap();let len=measure.position();let capacity=stack.capacity();let before=cx.growth_requests();let mut output=reserve_output(len,&mut cx).unwrap();assert_eq!(cx.growth_requests(),before+1);let before=cx.growth_requests();
  let mut emit=Sink::emit(&mut output);walk(Node::Type(&source),&mut stack,&mut emit,&mut cx).unwrap();emit.finish(len).unwrap();assert_eq!(cx.growth_requests(),before);assert_eq!(stack.capacity(),capacity);assert!(stack.is_empty());
  assert_eq!(output,br#"{"item":{"entity":"e","kind":"reference"},"kind":"list"}"#);
  let mut short=[0;1];let mut emit=Sink::emit(&mut short);assert_eq!(walk(Node::Type(&source),&mut stack,&mut emit,&mut cx),Err(EncodeError::Invariant(Invariant::OutputBounds)));assert_eq!(cx.growth_requests(),before);
 }
 #[test]fn invalid_workspace_is_refused_without_emission_growth(){
  let source=Type::Boolean;let mut stack=Vec::new();let mut cx=AllocationContext::new();
  assert_eq!(push_frame(&mut stack,Frame{node:Node::Type(&source),next:0},false,&mut cx),Err(EncodeError::Invariant(Invariant::WorkspaceCapacity)));assert_eq!(cx.growth_requests(),0);
  let mut output=[0;32];let mut sink=Sink::emit(&mut output);assert_eq!(walk(Node::Type(&source),&mut stack,&mut sink,&mut cx),Err(EncodeError::Invariant(Invariant::WorkspaceCapacity)));assert_eq!(cx.growth_requests(),0);
  push_frame(&mut stack,Frame{node:Node::Type(&source),next:0},true,&mut cx).unwrap();let before=cx.growth_requests();let mut sink=Sink::measure();assert_eq!(walk(Node::Type(&source),&mut stack,&mut sink,&mut cx),Err(EncodeError::Invariant(Invariant::Traversal)));assert_eq!(cx.growth_requests(),before);
 }
}
