// Reuse the existing one-observer harness and every environment assertion. The
// contract_value_external mode imports one real non-test contract rlib; normal
// discovery imports the unchanged contract source and its ordinary test closure.
include!("../product_runtime_owned_env_test.rs");
use product_app::OwnedAppGraph as Root;

// Test-only closed dispatch, never a product owner/extension trait. Expected
// schedules and comparisons use the unchanged independent harness walkers.
#[derive(Clone, Copy)]
enum RootInput<'a> { Ty(&'a Type), Val(&'a Value) }
enum RootCopy { Ty(Root<Type>), Val(Root<Value>) }
enum RootEq<'a> {
    Ty(product_app::PreparedAppEq<'a, Type>),
    Val(product_app::PreparedAppEq<'a, Value>),
}
impl RootEq<'_> {
    fn equal(&mut self) -> bool {
        match self { Self::Ty(p) => p.equal(), Self::Val(p) => p.equal() }
    }
}
impl<'a> RootInput<'a> {
    fn node(self) -> Node<'a> {
        match self { Self::Ty(t) => Node::Ty(t), Self::Val(v) => Node::Val(v) }
    }
    fn copy(self, cx: &mut Cx) -> Result<RootCopy, AllocationFailure> {
        match self {
            Self::Ty(t) => Root::<Type>::try_copy(t,cx).map(RootCopy::Ty),
            Self::Val(v) => Root::<Value>::try_copy(v,cx).map(RootCopy::Val),
        }
    }
    fn prepare(self, other: Self, cx: &mut Cx) -> Result<RootEq<'a>, AllocationFailure> {
        match (self,other) {
            (Self::Ty(a),Self::Ty(b)) => Root::<Type>::try_prepare_eq(a,b,cx).map(RootEq::Ty),
            (Self::Val(a),Self::Val(b)) => Root::<Value>::try_prepare_eq(a,b,cx).map(RootEq::Val),
            _ => unreachable!("test pairs have the same actual contract type"),
        }
    }
    fn schedule(self) -> Vec<(Site,usize)> {
        match self { Self::Ty(t) => schedule(t,None), Self::Val(v) => schedule(&Type::Boolean,Some(v)) }
    }
    fn same_copy(self, copy: &RootCopy) {
        match (self,copy) {
            (Self::Ty(a),RootCopy::Ty(b)) => same_type(a,b.as_ref()),
            (Self::Val(a),RootCopy::Val(b)) => same_value(a,b.as_ref()),
            _ => panic!("test root type changed"),
        }
    }
    fn marks(self) -> Vec<Mark> {
        let mut out=Vec::new(); let mut pending=vec![self.node()];
        while let Some(node)=pending.pop() {
            match node {
                Node::Ty(Type::List { item }|Type::Optional { item }) => {
                    out.push((&**item as *const Type as usize,1,1)); pending.push(Node::Ty(item));
                }
                Node::Ty(Type::Reference { entity }) => pending.push(Node::Text(entity)),
                Node::Val(Value::Text { value }) => pending.push(Node::Text(value)),
                Node::Val(Value::Reference { entity,record }) => {
                    pending.push(Node::Text(record)); pending.push(Node::Text(entity));
                }
                Node::Val(Value::List { item_type,items }) => {
                    if items.capacity()!=0 { out.push((items.as_ptr() as usize,items.len(),items.capacity())); }
                    pending.extend(items.iter().rev().map(Node::Val)); pending.push(Node::Ty(item_type));
                }
                Node::Text(s) if !s.is_empty() => out.push((s.as_ptr() as usize,s.len(),s.len())),
                _ => (),
            }
        }
        out
    }
    fn plan_schedule(self) -> Vec<(Site,usize)> {
        let mut pending=vec![self.node()]; let mut tokens=0usize;
        while let Some(node)=pending.pop() {
            tokens=tokens.checked_add(1).unwrap();
            match node {
                Node::Ty(Type::List { item }|Type::Optional { item }) => {
                    tokens+=1; pending.push(Node::Ty(item));
                }
                Node::Ty(Type::Reference { entity }) => pending.push(Node::Text(entity)),
                Node::Val(Value::Boolean { .. }|Value::Integer { .. }|Value::Date { .. }) => tokens+=1,
                Node::Val(Value::Text { value }) => pending.push(Node::Text(value)),
                Node::Val(Value::Reference { entity,record }) => {
                    pending.push(Node::Text(record)); pending.push(Node::Text(entity));
                }
                Node::Val(Value::List { item_type,items }) => {
                    pending.push(Node::Items(items)); pending.push(Node::Ty(item_type));
                }
                Node::Items(items) => pending.extend(items.iter().map(Node::Val)),
                _ => (),
            }
        }
        let mut result=Vec::new(); let mut capacity=1usize;
        loop {
            result.push((Site::Vector,capacity.checked_mul(product_app::BORROWED_RECORD_BYTES).unwrap()));
            if capacity>=tokens { return result; }
            capacity=capacity.checked_mul(2).unwrap();
        }
    }
}
impl RootCopy {
    fn borrowed(&self) -> RootInput<'_> {
        match self { Self::Ty(t) => RootInput::Ty(t.as_ref()), Self::Val(v) => RootInput::Val(v.as_ref()) }
    }
}
static ROOT_FAULTS: AtomicUsize=AtomicUsize::new(0);
fn root_fault(nth: usize, sticky: bool) -> Cx {
    assert!(ROOT_FAULTS.fetch_add(1,Ordering::Relaxed)<20000,"finite bridge fault census; replan, never truncate");
    Cx::fail_nth_growth(NonZeroUsize::new(nth).unwrap(),sticky)
}
fn sweep_root(source: RootInput<'_>) {
    let expected=source.schedule(); let g=expected.len(); assert!(g<=5000);
    let original=source.marks();
    for mode in 0..3 {
        let attempts=if mode==2 { 1 } else { g+1 };
        for nth in 1..=attempts {
            let mut cx=if mode==2 { Cx::new() } else { root_fault(nth,mode==1) };
            let watch=Watch::begin(); let result=source.copy(&mut cx); watch.pause();
            if mode!=2 && nth<=g {
                let err=match result { Err(e)=>e,Ok(_)=>panic!("expected root refusal") };
                assert_eq!(err,AllocationFailure { kind:Kind::Injected,site:expected[nth-1].0,
                    requested_bytes:Some(expected[nth-1].1) });
                assert_eq!(cx.first_failure(),Some(err)); assert_eq!(cx.growth_requests(),nth);
                watch.empty(nth-1);
            } else {
                let copy=result.unwrap(); source.same_copy(&copy);
                let copied=copy.borrowed().marks(); assert_eq!(copied.len(),original.len());
                for (a,b) in original.iter().zip(copied) {
                    assert_ne!(a.0,b.0,"copied payload aliases its borrowed source");
                    assert_eq!(a.1,b.1);
                }
                assert_eq!(cx.first_failure(),None); assert_eq!(cx.growth_requests(),g);
                watch.clean(g); watch.resume(); drop(copy); watch.empty(g);
            }
            assert_eq!(source.marks(),original);
        }
    }
}
fn sweep_eq<'a>(left: RootInput<'a>, right: RootInput<'a>, equal: bool) {
    let mut expected=left.plan_schedule(); expected.extend(right.plan_schedule());
    let g=expected.len(); assert!(g<=32);
    let before=(left.marks(),right.marks());
    for mode in 0..3 {
        let attempts=if mode==2 { 1 } else { g+1 };
        for nth in 1..=attempts {
            let mut cx=if mode==2 { Cx::new() } else { root_fault(nth,mode==1) };
            let watch=Watch::begin(); let result=left.prepare(right,&mut cx); watch.pause();
            if mode!=2 && nth<=g {
                let err=match result { Err(e)=>e,Ok(_)=>panic!("expected equality preparation refusal") };
                assert_eq!(err,AllocationFailure { kind:Kind::Injected,site:expected[nth-1].0,
                    requested_bytes:Some(expected[nth-1].1) });
                assert_eq!(cx.first_failure(),Some(err)); assert_eq!(cx.growth_requests(),nth);
                watch.empty(nth-1);
            } else {
                let mut prepared=result.unwrap();
                assert_eq!(cx.first_failure(),None); assert_eq!(cx.growth_requests(),g);
                watch.clean(g); watch.resume();
                let first=prepared.equal(); let second=prepared.equal(); drop(prepared); watch.empty(g);
                assert_eq!(first,equal); assert_eq!(second,equal);
            }
            assert_eq!((left.marks(),right.marks()),before);
        }
    }
}
fn bridge_deep_fixture() -> Deep {
    let mut source=deep_fixture();
    // Exercise a deep actual Type *inside* DataValue::List as well as its deep
    // item vector chain. The inherited Deep guard dismantles both iteratively.
    if let Value::List { item_type,.. }=source.value.as_mut().unwrap() {
        for i in 0..2048 {
            let old=std::mem::replace(item_type,Type::Boolean);
            *item_type=if i%2==0 { Type::Optional { item:Box::new(old) } }
                else { Type::List { item:Box::new(old) } };
        }
    }
    source
}
fn wide_bridge_value() -> Value {
    Value::List { item_type:Type::Optional { item:Box::new(Type::Text) },
        items:(0..64).map(|i| Value::List { item_type:Type::Reference { entity:"wide".into() },
            items:vec![Value::Null,Value::Reference { entity:"wide".into(),record:format!("r-{i}") },
                Value::Text { value:format!("value-{i}") }] }).collect() }
}

#[test]
fn actual_contract_values_flow_from_literal_record_session_into_managed_env() {
    let _lock=LOCK.lock().unwrap(); let f=fixtures(); let (ty,value)=&f[13];
    let literal=product_contract::Expr::Literal { value_type:ty.clone(),value:value.clone() };
    let record=product_contract::Record { entity:"entity".into(),id:"record".into(),revision:1,
        created_program:product_contract::Digest::try_from("0".repeat(64)).unwrap(),archived:false,
        values:BTreeMap::from([("field".into(),value.clone())]) };
    let session=product_contract::SessionState { view:"view".into(),
        values:BTreeMap::from([("state".into(),value.clone())]),focused_record:None };
    let before=(literal.clone(),record.clone(),session.clone());
    let product_contract::Expr::Literal { value_type,value }=&literal else { unreachable!() };
    let mut env=ManagedEnv::new();
    env.try_bind("literal",value_type,value,&mut Cx::new()).unwrap();
    env.try_bind("record",value_type,&record.values["field"],&mut Cx::new()).unwrap();
    env.try_bind("session",value_type,&session.values["state"],&mut Cx::new()).unwrap();
    // These signatures require the real legacy enum identities, not an oracle
    // schema or a serialization round-trip that only happens to compare equal.
    fn legacy(ty: &product_contract::Type, value: &product_contract::DataValue) -> (OldType,OldValue) {
        (old_type(ty),old_value(value))
    }
    for name in ["literal","record","session"] {
        let (a,b)=env.get(name).unwrap(); assert_eq!(legacy(a,b),legacy(value_type,value));
    }
    let copied_type=Root::<Type>::try_copy(value_type,&mut Cx::new()).unwrap();
    let copied_record=Root::<Value>::try_copy(&record.values["field"],&mut Cx::new()).unwrap();
    let copied_session=Root::<Value>::try_copy(&session.values["state"],&mut Cx::new()).unwrap();
    assert_eq!(legacy(copied_type.as_ref(),copied_record.as_ref()),legacy(value_type,value));
    assert_eq!(copied_session.as_ref(),value);
    assert_eq!((&literal,&record,&session),(&before.0,&before.1,&before.2));
}
#[test]
fn direct_root_copies_cover_all_contract_variants_and_are_independent() {
    let _lock=LOCK.lock().unwrap();
    for (t,v) in fixtures() {
        let expected=(t.clone(),v.clone());
        let a=Root::<Type>::try_copy(&t,&mut Cx::new()).unwrap();
        let b=Root::<Value>::try_copy(&v,&mut Cx::new()).unwrap();
        assert_eq!(a.as_ref(),&expected.0); assert_eq!(b.as_ref(),&expected.1);
        assert_eq!((&t,&v),(&expected.0,&expected.1)); drop(t); drop(v);
        assert_eq!(a.as_ref(),&expected.0); assert_eq!(b.as_ref(),&expected.1);
    }
}
#[test]
fn direct_root_every_growth_is_recoverable_and_reclaimed() {
    let _lock=LOCK.lock().unwrap();
    for (t,v) in fixtures() { sweep_root(RootInput::Ty(&t)); sweep_root(RootInput::Val(&v)); }
}
#[test]
fn direct_root_deep_and_wide_copy_and_drop_use_retained_workspace() {
    let _lock=LOCK.lock().unwrap(); let deep=bridge_deep_fixture();
    sweep_root(RootInput::Ty(deep.ty.as_ref().unwrap()));
    sweep_root(RootInput::Val(deep.value.as_ref().unwrap()));
    let wide=wide_bridge_value();
    let Value::List { item_type,.. }=&wide else { unreachable!() };
    sweep_root(RootInput::Ty(item_type)); sweep_root(RootInput::Val(&wide));
}
#[test]
fn prepared_contract_equality_matches_shallow_derived_eq() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures();
    for (ta,va) in &all { for (tb,vb) in &all {
        let expected=(ta==tb,va==vb);
        let mut types=Root::<Type>::try_prepare_eq(ta,tb,&mut Cx::new()).unwrap();
        let mut values=Root::<Value>::try_prepare_eq(va,vb,&mut Cx::new()).unwrap();
        let watch=Watch::begin();
        let first=(types.equal(),values.equal()); let second=(types.equal(),values.equal());
        watch.empty(0); assert_eq!(first,expected); assert_eq!(second,expected);
    }}
}
#[test]
fn prepared_contract_equality_every_growth_is_recoverable() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures();
    for (t,v) in &all {
        sweep_eq(RootInput::Ty(t),RootInput::Ty(t),true);
        sweep_eq(RootInput::Val(v),RootInput::Val(v),true);
    }
    let a=bridge_deep_fixture(); let b=bridge_deep_fixture();
    sweep_eq(RootInput::Ty(a.ty.as_ref().unwrap()),RootInput::Ty(b.ty.as_ref().unwrap()),true);
    sweep_eq(RootInput::Val(a.value.as_ref().unwrap()),RootInput::Val(b.value.as_ref().unwrap()),true);
}
#[test]
fn prepared_contract_equality_deep_tags_and_payloads_do_not_recurse() {
    let _lock=LOCK.lock().unwrap(); let a=bridge_deep_fixture(); let mut b=bridge_deep_fixture();
    {
        let mut t=Root::<Type>::try_prepare_eq(a.ty.as_ref().unwrap(),b.ty.as_ref().unwrap(),&mut Cx::new()).unwrap();
        let mut v=Root::<Value>::try_prepare_eq(a.value.as_ref().unwrap(),b.value.as_ref().unwrap(),&mut Cx::new()).unwrap();
        let watch=Watch::begin(); let same=(t.equal(),v.equal()); watch.empty(0); assert_eq!(same,(true,true));
    }
    let mut t=b.ty.as_mut().unwrap();
    loop { match t {
        Type::List { item }|Type::Optional { item } => t=item,
        Type::Reference { entity } => { entity.push('!'); break; }
        _ => unreachable!(),
    }}
    let mut v=b.value.as_mut().unwrap();
    loop { match v {
        Value::List { items,.. } => v=&mut items[0],
        Value::Text { value } => { value.push('!'); break; }
        _ => unreachable!(),
    }}
    sweep_eq(RootInput::Ty(a.ty.as_ref().unwrap()),RootInput::Ty(b.ty.as_ref().unwrap()),false);
    sweep_eq(RootInput::Val(a.value.as_ref().unwrap()),RootInput::Val(b.value.as_ref().unwrap()),false);
}
#[test]
fn shared_context_prefix_and_sticky_root_failures_preserve_first_cause() {
    let _lock=LOCK.lock().unwrap();
    for sticky in [false,true] {
        let mut cx=root_fault(2,sticky); let watch=Watch::begin();
        let prefix=product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap();
        let first=match Root::<Type>::try_copy(&Type::Boolean,&mut cx) {
            Err(e)=>e,Ok(_)=>panic!("expected first plan refusal"),
        };
        watch.pause(); assert_eq!(cx.first_failure(),Some(first)); assert_eq!(cx.growth_requests(),2);
        assert_eq!(first,AllocationFailure { kind:Kind::Injected,site:Site::Vector,
            requested_bytes:Some(product_app::BORROWED_RECORD_BYTES) });
        watch.resume(); let copy=Root::<Type>::try_copy(&Type::Boolean,&mut cx);
        let prepared=Root::<Type>::try_prepare_eq(&Type::Boolean,&Type::Boolean,&mut cx); watch.pause();
        if sticky {
            assert!(copy.is_err()); assert!(prepared.is_err()); assert_eq!(cx.growth_requests(),4);
        } else {
            let copy=copy.unwrap(); let mut prepared=prepared.unwrap();
            assert_eq!(copy.as_ref(),&Type::Boolean); assert_eq!(cx.growth_requests(),7);
            watch.resume(); let equal=prepared.equal(); drop(copy); drop(prepared); watch.pause(); assert!(equal);
        }
        assert_eq!(cx.first_failure(),Some(first));
        let allocations=if sticky { 1 } else { 6 };
        watch.resume(); drop(prefix); watch.empty(allocations);
    }
}
#[test]
fn original_parallel_app_graph_still_copies_and_compares() {
    let _lock=LOCK.lock().unwrap();
    struct Source(Option<product_app::Type>);
    impl Drop for Source {
        fn drop(&mut self) {
            let Some(mut t)=self.0.take() else { return; };
            loop { match t {
                product_app::Type::List { item }|product_app::Type::Optional { item } => t=*item,
                _ => break,
            }}
        }
    }
    let mut source=Source(Some(product_app::Type::Reference { entity:"original".into() }));
    for _ in 0..2048 { source.0=Some(product_app::Type::Optional { item:Box::new(source.0.take().unwrap()) }); }
    let borrowed=source.0.as_ref().unwrap(); let mut cx=Cx::new();
    let watch=Watch::begin(); let copy=borrowed.try_copy(&mut cx).unwrap();
    let mut eq=borrowed.try_prepare_eq(copy.as_ref(),&mut cx).unwrap();
    let equal=eq.equal(); drop(eq); drop(copy); watch.empty(cx.growth_requests()); assert!(equal);
}
