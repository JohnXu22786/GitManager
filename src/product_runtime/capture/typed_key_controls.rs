//! Independent native canonical oracle. Component budgets are deliberately not
//! product-fit evidence; real-row controls retain the original host budget.
use super::*;
use std::cmp::Ordering as Cmp;
use serde_json::{json, Value};
#[derive(Default)]
struct Work { packets: Vec<u64>, fail: Option<usize> }
impl ContractWork for Work {
    fn charge(&mut self, n:u64)->std::result::Result<(),WorkFailure> {
        let i=self.packets.len(); self.packets.push(n);
        if self.fail==Some(i) { Err(WorkFailure::Cancelled) } else { Ok(()) }
    }
}
fn boolean()->Value {json!({"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":true}})}
fn variants()->Vec<Expr> {
    let b=boolean();let mut out=vec![b.clone(),json!({"kind":"variable","name":"x"}),json!({"kind":"field","record":b,"field":"f"}),json!({"kind":"state","state":"s"}),json!({"kind":"today"}),json!({"kind":"query","entity":"e","binding":"b","predicate":b,"sort":[{"value":b,"descending":true}],"limit":17,"include_archived":true})];
    for kind in ["filter","any","all"] {out.push(json!({"kind":kind,"items":b,"binding":"b","predicate":b}));}
    out.push(json!({"kind":"map","items":b,"binding":"b","value":b}));
    for kind in ["count","sum"] {out.push(json!({"kind":kind,"items":b}));}
    out.push(json!({"kind":"contains","items":b,"value":b}));
    for kind in ["equal","less","add","subtract","concat"] {out.push(json!({"kind":kind,"left":b,"right":b}));}
    out.push(json!({"kind":"text_contains","text":b,"search":b}));
    for kind in ["lower","not"] {out.push(json!({"kind":kind,"value":b}));}
    for kind in ["and","or"] {out.push(json!({"kind":kind,"values":[b,b]}));}
    out.push(json!({"kind":"if","condition":b,"then_value":b,"else_value":b}));
    out.push(json!({"kind":"coalesce","value":b,"fallback":b}));
    out.push(json!({"kind":"date_add","date":b,"days":b}));
    out.push(json!({"kind":"date_difference","later":b,"earlier":b}));
    assert_eq!(out.len(),27);out.into_iter().map(|x|serde_json::from_value(x).unwrap()).collect()
}
fn oracle(k:&OwnedCaptureKey)->Vec<u8> {canonical_bytes(&(0usize,&k.entity,&k.binding,&k.expression,&k.expected,&k.limits)).unwrap()}
fn make(e:&Expr,t:&Type,l:&RuntimeLimits)->OwnedCaptureKey {paid_key("entity","binding",e,t,l,&mut Work::default()).unwrap().unwrap()}
#[test]
fn typed_key_schema_is_closed_over_all_fields_and_serde_attributes() {
    let source=include_str!("../../product_contract.rs").replace("\r\n","\n");
    let mut parts=Vec::new();
    for (name,kind) in [("Type","enum"),("DataValue","enum"),("Expr","enum"),("SortKey","struct"),("RuntimeLimits","struct")] {
        let at=source.find(&format!("pub {kind} {name} {{")).unwrap();let start=source[..at].rfind("#[derive").unwrap();let end=start+source[start..].find("\n}").unwrap()+2;parts.push(&source[start..end]);
    }
    assert_eq!(parts.join("\n\n")+"\n",include_str!("typed_key_schema.txt").replace("\r\n","\n"));
}
#[test]
fn typed_key_all_variants_copy_length_and_total_order_match_native_equality() {
    let l=RuntimeLimits::default();let mut keys=Vec::new();
    let types=vec![Type::Boolean,Type::Integer,Type::Text,Type::Date,Type::reference("e"),Type::list(Type::Text),Type::Optional{item:Box::new(Type::reference("z"))}];
    let values=vec![DataValue::Null,DataValue::Boolean{value:false},DataValue::Integer{value:i64::MIN},DataValue::Text{value:"text".into()},DataValue::Date{days:i32::MIN},DataValue::Reference{entity:"e".into(),record:"r".into()},DataValue::List{item_type:Type::Text,items:vec![DataValue::Text{value:"nested".into()}]}];
    for e in variants().into_iter().chain(values.into_iter().map(|value|Expr::Literal{value_type:Type::Text,value})) {
        for t in &types {let k=make(&e,t,&l);let native=canonical_bytes(&(0usize,"entity","binding",&e,t,&l)).unwrap();assert_eq!(k.wire_len,native.len());assert_eq!(oracle(&k),native);keys.push(k);}
    }
    for (i,a) in keys.iter().enumerate() {let b=&keys[(i*17+1)%keys.len()];let c=&keys[(i*37+3)%keys.len()];let ab=paid_compare(a,b,&mut Work::default()).unwrap();let ba=paid_compare(b,a,&mut Work::default()).unwrap();assert_eq!(ab,ba.reverse());assert_eq!(ab==Cmp::Equal,oracle(a)==oracle(b));let bc=paid_compare(b,c,&mut Work::default()).unwrap();if ab!=Cmp::Greater&&bc!=Cmp::Greater{assert_ne!(paid_compare(a,c,&mut Work::default()).unwrap(),Cmp::Greater);}assert_eq!(paid_compare(a,a,&mut Work::default()).unwrap(),Cmp::Equal);}
}
#[test]
fn typed_key_escapes_integer_widths_and_owned_input_lifetime() {
    let mut texts=vec![String::new(),"\"\\/\u{7f}é😀\u{2028}\u{2029}e\u{301}".into(),"é".into()];texts.push((0u8..32).map(char::from).collect());
    for text in texts {let e=Expr::Literal{value_type:Type::Text,value:DataValue::Text{value:text}};let k=make(&e,&Type::Text,&RuntimeLimits::default());assert_eq!(k.wire_len,oracle(&k).len());drop(e);assert_eq!(k.wire_len,oracle(&k).len());}
    for n in [i64::MIN,-100,-10,-1,0,1,9,10,99,100,i64::MAX] {let k=make(&Expr::Literal{value_type:Type::Integer,value:DataValue::Integer{value:n}},&Type::Integer,&RuntimeLimits::default());assert_eq!(k.wire_len,oracle(&k).len());}
    let mut l=RuntimeLimits::default();l.fuel=u64::MAX;l.elapsed_millis=0;l.collection_items=usize::MAX;l.action_steps=0;l.output_bytes=usize::MAX;l.transaction_writes=10;
    for days in [i32::MIN,0,i32::MAX] {let k=make(&Expr::Literal{value_type:Type::Date,value:DataValue::Date{days}},&Type::Date,&l);assert_eq!(k.wire_len,oracle(&k).len());}
}
#[test]
fn typed_key_every_packet_refusal_propagates_and_shape_rejection_is_none() {
    for e in variants() {let mut w=Work::default();let k=paid_key("entity","binding",&e,&Type::Boolean,&RuntimeLimits::default(),&mut w).unwrap().unwrap();for i in 0..w.packets.len(){let mut w=Work{fail:Some(i),..Work::default()};assert!(matches!(paid_key("entity","binding",&e,&Type::Boolean,&RuntimeLimits::default(),&mut w),Err(AdapterError::Cancelled)));}
        let mut w=Work::default();paid_compare(&k,&k,&mut w).unwrap();for i in 0..w.packets.len(){let mut w=Work{fail:Some(i),..Work::default()};assert_eq!(paid_compare(&k,&k,&mut w),Err(AdapterError::Cancelled));}}
    assert!(paid_key(&"x".repeat(MAX_TEXT_BYTES+1),"b",&Expr::Today,&Type::Date,&RuntimeLimits::default(),&mut Work::default()).unwrap().is_none());
}
fn mutations(v:&Value)->Vec<Value>{
    let mut out=Vec::new();
    match v {
        Value::String(s)=>out.push(Value::String(format!("{s}x"))),
        Value::Bool(b)=>out.push(Value::Bool(!b)),
        Value::Number(n)=>{if let Some(n)=n.as_i64(){out.push(json!(if n==i64::MAX {n-1}else{n+1}));}},
        Value::Array(a)=>{
            if !a.is_empty(){let mut b=a.clone();b.pop();out.push(Value::Array(b));let mut b=a.clone();b.push(a[0].clone());out.push(Value::Array(b));let mut b=a.clone();b.reverse();if b!=*a{out.push(Value::Array(b));}}
            for (i,item) in a.iter().enumerate(){for m in mutations(item){let mut b=a.clone();b[i]=m;out.push(Value::Array(b));}}
        },
        Value::Object(a)=>for (field,item) in a{if field!="kind"{for m in mutations(item){let mut b=a.clone();b.insert(field.clone(),m);out.push(Value::Object(b));}}},
        Value::Null=>{},
    }out
}
#[test]
fn typed_key_every_field_and_limit_deep_mutation_changes_identity(){
    let l=RuntimeLimits::default();
    for e in variants(){let original=make(&e,&Type::Boolean,&l);for m in mutations(&serde_json::to_value(&e).unwrap()){let changed:Expr=serde_json::from_value(m).unwrap();let changed=make(&changed,&Type::Boolean,&l);assert_ne!(oracle(&original),oracle(&changed));assert_ne!(paid_compare(&original,&changed,&mut Work::default()).unwrap(),Cmp::Equal);}}
    let e=Expr::Today;let t=Type::Optional{item:Box::new(Type::list(Type::reference("entity")))};let original=make(&e,&t,&l);
    for m in mutations(&serde_json::to_value(&t).unwrap()){let changed=make(&e,&serde_json::from_value(m).unwrap(),&l);assert_ne!(paid_compare(&original,&changed,&mut Work::default()).unwrap(),Cmp::Equal);}
    let ls=mutations(&serde_json::to_value(&l).unwrap());assert_eq!(ls.len(),6);for m in ls{let changed=make(&e,&t,&serde_json::from_value(m).unwrap());assert_ne!(paid_compare(&original,&changed,&mut Work::default()).unwrap(),Cmp::Equal);}
    for (entity,binding) in [("entityx","binding"),("entity","bindingx")]{let changed=paid_key(entity,binding,&e,&t,&l,&mut Work::default()).unwrap().unwrap();assert_ne!(paid_compare(&original,&changed,&mut Work::default()).unwrap(),Cmp::Equal);}
    let a=make(&Expr::Variable{name:"aaaa".into()},&Type::Text,&l);let b=make(&Expr::Variable{name:"aaab".into()},&Type::Text,&l);assert_eq!(a.wire_len,b.wire_len);assert_ne!(paid_compare(&a,&b,&mut Work::default()).unwrap(),Cmp::Equal);
}
fn snapshot(keys:&[Box<OwnedCaptureKey>])->Vec<Vec<u8>>{keys.iter().map(|key|oracle(key)).collect()}
fn named(s:&str)->OwnedCaptureKey{paid_key("e",s,&Expr::Today,&Type::Date,&RuntimeLimits::default(),&mut Work::default()).unwrap().unwrap()}
fn exact_size(n:usize,escaped:bool)->OwnedCaptureKey{
    let mut items=vec![];
    loop {
        let e=Expr::Literal{value_type:Type::list(Type::Text),value:DataValue::List{item_type:Type::Text,items:items.clone()}};
        let k=make(&e,&Type::list(Type::Text),&RuntimeLimits::default());let size=oracle(&k).len();
        if size==n{return k}
        assert!(size<n);
        // A new tagged text contributes26 bytes plus a comma when non-first.
        let overhead=canonical_bytes(&DataValue::Text{value:String::new()}).unwrap().len()+usize::from(!items.is_empty());
        let remaining=n-size;assert!(remaining>=overhead);
        let chunk=if escaped{(MAX_TEXT_BYTES-5)*6}else{MAX_TEXT_BYTES};
        let mut take=(remaining-overhead).min(chunk);
        if remaining-overhead>chunk && remaining-overhead-chunk<overhead+1 {take-=overhead+1;}
        items.push(DataValue::Text{value:if escaped{"\u{0001}".repeat(take/6)+&"x".repeat(take%6)}else{"x".repeat(take)}});
    }
}
#[test]
fn typed_key_publication_exact_caps_duplicates_and_refusal_atomicity(){
    for escaped in [false,true]{for n in [MAX_WIRE_BYTES-1,MAX_WIRE_BYTES,MAX_WIRE_BYTES+1]{let k=exact_size(n,escaped);assert_eq!(oracle(&k).len(),n);assert_eq!(k.wire_len,n);let mut cache=Vec::new();let mut bytes=0;paid_publish(&mut cache,&mut bytes,Some(k),&mut Work::default()).unwrap();assert_eq!(cache.len(),usize::from(n<=MAX_WIRE_BYTES));assert_eq!(bytes,if n<=MAX_WIRE_BYTES{n}else{0});}}
    let mut cache=Vec::new();let mut bytes=0;for i in 0..MAX_ITEMS{paid_publish(&mut cache,&mut bytes,Some(named(&format!("{i:03}"))),&mut Work::default()).unwrap();}
    let before=snapshot(&cache);let old=bytes;paid_publish(&mut cache,&mut bytes,Some(named("new")),&mut Work::default()).unwrap();assert_eq!(snapshot(&cache),before);assert_eq!(bytes,old);assert!(paid_search(&cache,&named("127"),&mut Work::default()).unwrap().is_ok());
    for grow in [false,true]{
        let fixture=||{let mut c=Vec::with_capacity(if grow{2}else{8});let mut b=0;for name in ["a","z"]{paid_publish(&mut c,&mut b,Some(named(name)),&mut Work::default()).unwrap();}(c,b)};
        let(mut c,mut b)=fixture();let old=b;paid_publish(&mut c,&mut b,Some(named("a")),&mut Work::default()).unwrap();assert_eq!(b,old);assert_eq!(c.len(),2);
        let mut w=Work::default();paid_publish(&mut c,&mut b,Some(named("m")),&mut w).unwrap();
        for i in 0..w.packets.len(){let(mut c,mut b)=fixture();let before=snapshot(&c);let old=b;let cap=c.capacity();let mut w=Work{fail:Some(i),..Work::default()};assert_eq!(paid_publish(&mut c,&mut b,Some(named("m")),&mut w),Err(AdapterError::Cancelled));assert_eq!(snapshot(&c),before);assert_eq!(b,old);assert_eq!(c.capacity(),cap);}
    }
}
#[test]
fn typed_key_shape_boundaries_remain_uncached_rejection_not_success(){
    let l=RuntimeLimits::default();
    let admitted=|e:&Expr,t:&Type|paid_key("e","b",e,t,&l,&mut Work::default()).unwrap().is_some();
    let mut e=Expr::Today;for _ in 0..MAX_EXPR_DEPTH{e=Expr::Not{value:Box::new(e)}}assert!(admitted(&e,&Type::Date));e=Expr::Not{value:Box::new(e)};assert!(!admitted(&e,&Type::Date));
    let mut t=Type::Boolean;for _ in 0..MAX_TYPE_DEPTH{t=Type::list(t)}assert!(admitted(&Expr::Today,&t));t=Type::list(t);assert!(!admitted(&Expr::Today,&t));
    for n in [MAX_ITEMS,MAX_ITEMS+1]{assert_eq!(admitted(&Expr::And{values:vec![Expr::Today;n]},&Type::Boolean),n==MAX_ITEMS);}
    for n in [MAX_COLLECTION,MAX_COLLECTION+1]{assert_eq!(admitted(&Expr::Literal{value_type:Type::Integer,value:DataValue::List{item_type:Type::Integer,items:vec![DataValue::Integer{value:0};n]}},&Type::Integer),n==MAX_COLLECTION);}
    for n in [MAX_TEXT_BYTES,MAX_TEXT_BYTES+1]{assert_eq!(admitted(&Expr::Variable{name:"x".repeat(n)},&Type::Boolean),n==MAX_TEXT_BYTES);}
    // 1 root +256 interior nodes +7935 leaves =8192 total.
    let mut values=vec![Expr::And{values:vec![Expr::Today;31]};256];
    if let Expr::And{values}=values.last_mut().unwrap(){values.pop();}
    let mut e=Expr::And{values};assert!(admitted(&e,&Type::Boolean));
    if let Expr::And{values}=&mut e{if let Expr::And{values}=values.last_mut().unwrap(){values.push(Expr::Today);}}
    assert!(!admitted(&e,&Type::Boolean));
}
#[test]
fn typed_key_deep_values_vector_order_and_sort_direction_are_identity(){
    let mut expressions=variants();
    for value in [DataValue::Null,DataValue::Integer{value:9},DataValue::Date{days:9},DataValue::Reference{entity:"entity".into(),record:"record".into()},DataValue::List{item_type:Type::reference("entity"),items:vec![DataValue::Reference{entity:"entity".into(),record:"first".into()},DataValue::Reference{entity:"entity".into(),record:"second".into()}]}]{expressions.push(Expr::Literal{value_type:Type::list(Type::reference("entity")),value});}
    let first=Expr::Variable{name:"first".into()};let second=Expr::Variable{name:"second".into()};
    expressions.push(Expr::And{values:vec![first.clone(),second.clone()]});
    expressions.push(Expr::Query{entity:"entity".into(),binding:"binding".into(),predicate:Box::new(first.clone()),sort:vec![SortKey{value:first,descending:false},SortKey{value:second,descending:true}],limit:10,include_archived:false});
    for e in expressions {let a=make(&e,&Type::Boolean,&RuntimeLimits::default());for changed in mutations(&serde_json::to_value(&e).unwrap()){let e:Expr=serde_json::from_value(changed).unwrap();let b=make(&e,&Type::Boolean,&RuntimeLimits::default());assert_ne!(oracle(&a),oracle(&b));assert_ne!(paid_compare(&a,&b,&mut Work::default()).unwrap(),Cmp::Equal);assert_eq!(b.wire_len,oracle(&b).len());}}
}
#[test]
fn typed_key_layout_and_exact_sample_debits(){
    let l=RuntimeLimits::default();let e:Expr=serde_json::from_value(boolean()).unwrap();
    let mut old=Work::default();let bytes=canonical_bytes_with_work(&(0usize,"person","record",&e,&Type::Boolean,&l),&mut old).unwrap();
    let mut new=Work::default();let key=paid_key("person","record",&e,&Type::Boolean,&l,&mut new).unwrap().unwrap();assert_eq!(oracle(&key),bytes);
    eprintln!("TYPED_KEY_COMPONENT_LAYOUT key={} expr={} type={} value={} sort={} limits={} old_canonical_only={} new_key_including_shape_and_frame={} canonical_bytes={}",size_of::<OwnedCaptureKey>(),size_of::<Expr>(),size_of::<Type>(),size_of::<DataValue>(),size_of::<SortKey>(),size_of::<RuntimeLimits>(),old.packets.iter().sum::<u64>(),new.packets.iter().sum::<u64>(),bytes.len());
}
#[test]
fn typed_key_exact_debit_boundaries_refuse_before_unfunded_work(){
    struct Fuel{left:u64,packets:usize}
    impl ContractWork for Fuel{fn charge(&mut self,n:u64)->std::result::Result<(),WorkFailure>{self.packets+=1;self.left=self.left.checked_sub(n).ok_or(WorkFailure::BudgetExhausted("test fuel exhausted"))?;Ok(())}}
    let e:Expr=serde_json::from_value(boolean()).unwrap();let l=RuntimeLimits::default();let mut work=Work::default();paid_key("e","b",&e,&Type::Boolean,&l,&mut work).unwrap();
    let mut prefix=0;for n in work.packets.iter().copied(){prefix+=n;if n>0{let mut fuel=Fuel{left:prefix-1,packets:0};assert!(matches!(paid_key("e","b",&e,&Type::Boolean,&l,&mut fuel),Err(AdapterError::BudgetExhausted(_))));}}
    let mut fuel=Fuel{left:prefix,packets:0};let key=paid_key("e","b",&e,&Type::Boolean,&l,&mut fuel).unwrap().unwrap();assert_eq!(fuel.left,0);assert_eq!(key.wire_len,oracle(&key).len());
}

// Mirror the production caller's pre-entry payments, so injected refusal tests
// include these boundaries as well as every internal traversal/native packet.
fn paid_key(entity:&str,binding:&str,expression:&Expr,expected:&Type,limits:&RuntimeLimits,work:&mut dyn ContractWork)->Result<Option<OwnedCaptureKey>>{pay(work,KEY_FRAME)?;key(entity,binding,expression,expected,limits,work)}
fn paid_compare(a:&OwnedCaptureKey,b:&OwnedCaptureKey,work:&mut dyn ContractWork)->Result<Cmp>{pay(work,COMPARE_FRAME)?;compare(a,b,work)}
fn paid_search(cache:&[Box<OwnedCaptureKey>],key:&OwnedCaptureKey,work:&mut dyn ContractWork)->Result<std::result::Result<usize,usize>>{pay(work,SEARCH_FRAME)?;search(cache,key,work)}
fn paid_publish(cache:&mut Vec<Box<OwnedCaptureKey>>,bytes:&mut usize,key:Option<OwnedCaptureKey>,work:&mut dyn ContractWork)->Result<()>{pay(work,PUBLISH_FRAME)?;publish(cache,bytes,key,work)}

#[test]
fn typed_key_long_equal_prefix_and_distinct_allocations_are_exact(){
    let l=RuntimeLimits::default();let prefix="x".repeat(MAX_TEXT_BYTES-1);
    let a=make(&Expr::Variable{name:prefix.clone()+"a"},&Type::Text,&l);
    let same=make(&Expr::Variable{name:prefix.clone()+"a"},&Type::Text,&l);
    let b=make(&Expr::Variable{name:prefix.clone()+"b"},&Type::Text,&l);
    let shorter=make(&Expr::Variable{name:prefix},&Type::Text,&l);
    assert_eq!(paid_compare(&a,&same,&mut Work::default()).unwrap(),Cmp::Equal);
    assert_eq!(paid_compare(&a,&b,&mut Work::default()).unwrap(),Cmp::Less);
    assert_eq!(paid_compare(&shorter,&a,&mut Work::default()).unwrap(),Cmp::Less);
    assert_eq!(oracle(&a),oracle(&same));assert_ne!(oracle(&a),oracle(&b));
}
