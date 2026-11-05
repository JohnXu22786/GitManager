#![allow(unexpected_cfgs)] // Explicit bounded standalone mode; ordinary Cargo imports the real source.
include!("support/contract_owned_value_cases.rs");
use product_app::ManagedListBuilder;

// These walkers count occurrences independently of retained capacity or the
// product Plan. Raw fixtures and allocating assertions stay outside observation.
fn transfer_tokens(input: RootInput<'_>) -> usize {
    let mut pending=vec![input.node()]; let mut n=0usize;
    while let Some(node)=pending.pop() {
        n=n.checked_add(1).unwrap();
        match node {
            Node::Ty(Type::List { item }|Type::Optional { item }) => {
                n=n.checked_add(1).unwrap(); pending.push(Node::Ty(item));
            }
            Node::Ty(Type::Reference { entity }) => pending.push(Node::Text(entity)),
            Node::Val(Value::Boolean { .. }|Value::Integer { .. }|Value::Date { .. }) => n+=1,
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
    n
}
fn workspace_bytes(tokens: usize) -> usize {
    tokens.checked_mul(product_app::OWNED_NODE_BYTES).unwrap()
}
fn verify_state(state: (usize,usize,usize), tokens: usize) {
    assert_eq!(state.0,tokens); assert!(state.1>=tokens); assert_ne!(state.2,0);
}
fn move_ok<T,S>(result: Result<T,(AllocationFailure,S)>) -> T {
    match result { Ok(value)=>value,Err(_)=>panic!("unexpected managed transfer refusal") }
}
fn transfer_failure(cx: &Cx, err: AllocationFailure, expected: (Site,usize), nth: usize) {
    assert_eq!(err,AllocationFailure { kind:Kind::Injected,site:expected.0,requested_bytes:Some(expected.1) });
    assert_eq!(cx.first_failure(),Some(err)); assert_eq!(cx.growth_requests(),nth);
}
static TRANSFER_FAULTS: AtomicUsize=AtomicUsize::new(0);
fn transfer_fault(nth: usize, sticky: bool) -> Cx {
    assert!(TRANSFER_FAULTS.fetch_add(1,Ordering::Relaxed)<1000,"finite transfer census; replan, never truncate");
    Cx::fail_nth_growth(NonZeroUsize::new(nth).unwrap(),sticky)
}
fn list_parts(value: &Value) -> (&Type,&[Value]) {
    match value { Value::List { item_type,items }=>(item_type,items),_=>panic!("builder lost List root") }
}
fn list_capacity(value: &Value) -> usize {
    match value { Value::List { items,.. }=>items.capacity(),_=>panic!("builder lost List root") }
}
fn append_schedule(builder: &ManagedListBuilder, incoming_tokens: usize) -> Vec<(Site,usize)> {
    let state=builder.test_transfer_state(); let n=state.0.checked_add(incoming_tokens).unwrap();
    let (_,items)=list_parts(builder.as_ref()); let mut out=Vec::new();
    if state.1<n { out.push((Site::Vector,workspace_bytes(n))); }
    if items.len()==list_capacity(builder.as_ref()) {
        out.push((Site::Vector,(items.len()+1).checked_mul(size_of::<Value>()).unwrap()));
    }
    out
}
fn construct_leaf(input: RootInput<'_>, cx: &mut Cx) -> Result<RootCopy,AllocationFailure> {
    match input {
        RootInput::Ty(t) => match t {
            Type::Boolean=>Root::<Type>::try_boolean(cx),
            Type::Integer=>Root::<Type>::try_integer(cx),
            Type::Text=>Root::<Type>::try_text(cx),
            Type::Date=>Root::<Type>::try_date(cx),
            Type::Reference { entity }=>Root::<Type>::try_reference(entity,cx),
            _=>panic!("test leaf input is composite"),
        }.map(RootCopy::Ty),
        RootInput::Val(v) => match v {
            Value::Null=>Root::<Value>::try_null(cx),
            Value::Boolean { value }=>Root::<Value>::try_boolean(*value,cx),
            Value::Integer { value }=>Root::<Value>::try_integer(*value,cx),
            Value::Date { days }=>Root::<Value>::try_date(*days,cx),
            Value::Text { value }=>Root::<Value>::try_text(value,cx),
            Value::Reference { entity,record }=>Root::<Value>::try_reference(entity,record,cx),
            _=>panic!("test leaf input is composite"),
        }.map(RootCopy::Val),
    }
}
fn leaf_schedule(input: RootInput<'_>) -> Vec<(Site,usize)> {
    let mut out=vec![(Site::Vector,workspace_bytes(transfer_tokens(input)))];
    let mut add=|s: &str| { if !s.is_empty() { out.push((Site::String,s.len())); } };
    match input {
        RootInput::Ty(Type::Reference { entity })=>add(entity),
        RootInput::Val(Value::Text { value })=>add(value),
        RootInput::Val(Value::Reference { entity,record })=>{ add(entity); add(record); },
        _=>(),
    }
    out
}
fn sweep_leaf(input: RootInput<'_>) {
    let expected=leaf_schedule(input); let g=expected.len(); let original=input.marks();
    let n=transfer_tokens(input);
    for mode in 0..3 { for nth in 1..=if mode==2 { 1 } else { g+1 } {
        let mut cx=if mode==2 { Cx::new() } else { transfer_fault(nth,mode==1) };
        let watch=Watch::begin(); let result=construct_leaf(input,&mut cx); watch.pause();
        if mode!=2 && nth<=g {
            let err=match result { Err(e)=>e,Ok(_)=>panic!("expected leaf refusal") };
            transfer_failure(&cx,err,expected[nth-1],nth); watch.empty(nth-1);
        } else {
            let root=result.unwrap(); input.same_copy(&root);
            let state=match &root { RootCopy::Ty(t)=>t.test_transfer_state(),RootCopy::Val(v)=>v.test_transfer_state() };
            verify_state(state,n); let copied=root.borrowed().marks();
            for (a,b) in original.iter().zip(&copied) { assert_ne!(a.0,b.0); assert_eq!(a.1,b.1); }
            assert_eq!(copied.len(),original.len()); assert_eq!(cx.growth_requests(),g);
            assert_eq!(cx.first_failure(),None); watch.clean(g); watch.resume(); drop(root); watch.empty(g);
        }
        assert_eq!(input.marks(),original);
    }}
}
#[test]
fn closed_leaf_constructors_preserve_all_scalar_and_borrowed_values() {
    let _lock=LOCK.lock().unwrap();
    let types=[Type::Boolean,Type::Integer,Type::Text,Type::Date,
        Type::Reference { entity:String::new() },Type::Reference { entity:"e\0中".into() }];
    let values=[Value::Null,Value::Boolean { value:false },Value::Boolean { value:true },
        Value::Integer { value:i64::MIN },Value::Integer { value:i64::MAX },
        Value::Date { days:i32::MIN },Value::Date { days:i32::MAX },
        Value::Text { value:String::new() },Value::Text { value:"\0".into() },
        Value::Text { value:"ΟΣ ΟΣΑ İ e\u{301}中".into() },
        Value::Reference { entity:String::new(),record:String::new() },
        Value::Reference { entity:"e\0".into(),record:String::new() },
        Value::Reference { entity:String::new(),record:"中".into() },
        Value::Reference { entity:"e\0中".into(),record:"r\0😀".into() }];
    for t in &types { sweep_leaf(RootInput::Ty(t)); }
    for v in &values { sweep_leaf(RootInput::Val(v)); }
}
#[test]
fn text_adoption_retains_identity_on_success_and_every_refusal() {
    let _lock=LOCK.lock().unwrap();
    for text in ["","\0","ΟΣ İ e\u{301}中"] {
        for mode in 0..3 { for nth in 1..=if mode==2 { 1 } else { 2 } {
            let mut cx=if mode==2 { Cx::new() } else { transfer_fault(nth,mode==1) };
            let watch=Watch::begin(); let mut setup=Cx::new();
            let source=product_alloc::try_copy_str(text,&mut setup,Site::String).unwrap();
            let pointer=source.as_ptr(); let capacity=source.capacity();
            let result=Root::<Value>::try_take_text(source,&mut cx); watch.pause();
            let mut allocations=setup.growth_requests();
            let root=if mode!=2 && nth==1 {
                let (err,source)=match result { Err(e)=>e,Ok(_)=>panic!("expected adoption refusal") };
                transfer_failure(&cx,err,(Site::Vector,workspace_bytes(2)),1);
                assert_eq!(source,text); assert_eq!(source.as_ptr(),pointer); assert_eq!(source.capacity(),capacity);
                let mut retry=Cx::new(); watch.resume(); let root=move_ok(Root::<Value>::try_take_text(source,&mut retry)); watch.pause();
                assert_eq!(retry.growth_requests(),1); allocations+=1; root
            } else { allocations+=1; assert_eq!(cx.growth_requests(),1); result.ok().unwrap() };
            let Value::Text { value }=root.as_ref() else { panic!("adoption changed tag") };
            assert_eq!(value,text); assert_eq!(value.as_ptr(),pointer); assert_eq!(value.capacity(),capacity);
            verify_state(root.test_transfer_state(),2); watch.clean(allocations); watch.resume(); drop(root); watch.empty(allocations);
        }}
    }
}
fn sweep_wrap(source: &Type) {
    let input=RootInput::Ty(source); let n=transfer_tokens(input); let setup_g=input.schedule().len();
    let expected=[(Site::Vector,workspace_bytes(n+2)),(Site::Box,size_of::<Type>())];
    for mode in 0..3 { for nth in 1..=if mode==2 { 1 } else { 3 } {
        let mut cx=if mode==2 { Cx::new() } else { transfer_fault(nth,mode==1) };
        let watch=Watch::begin(); let root=Root::<Type>::try_copy(source,&mut Cx::new()).unwrap(); watch.pause();
        let original=RootInput::Ty(root.as_ref()).marks(); let state=root.test_transfer_state();
        verify_state(state,n); watch.resume(); let result=Root::<Type>::try_list(root,&mut cx); watch.pause();
        let mut allocations=setup_g;
        let wrapped=if mode!=2 && nth<=2 {
            let (err,root)=match result { Err(e)=>e,Ok(_)=>panic!("expected wrap refusal") };
            transfer_failure(&cx,err,expected[nth-1],nth); same_type(root.as_ref(),source);
            assert_eq!(root.test_transfer_state(),state); assert_eq!(RootInput::Ty(root.as_ref()).marks(),original);
            allocations+=nth-1; let mut retry=Cx::new(); watch.resume();
            let wrapped=move_ok(Root::<Type>::try_list(root,&mut retry)); watch.pause();
            assert_eq!(retry.growth_requests(),2); allocations+=2; wrapped
        } else { assert_eq!(cx.growth_requests(),2); allocations+=2; result.ok().unwrap() };
        let Type::List { item }=wrapped.as_ref() else { panic!("wrap lost List") };
        same_type(item,source); assert_eq!(RootInput::Ty(item).marks(),original);
        let wrapped_state=wrapped.test_transfer_state(); verify_state(wrapped_state,n+2);
        watch.clean(allocations); watch.resume();
        let item=match wrapped.into_list_item() { Ok(v)=>v,Err(_)=>panic!("List refused unwrap") }; watch.pause();
        verify_state(item.test_transfer_state(),n);
        assert_eq!((item.test_transfer_state().1,item.test_transfer_state().2),(wrapped_state.1,wrapped_state.2));
        same_type(item.as_ref(),source); assert_eq!(RootInput::Ty(item.as_ref()).marks(),original);
        watch.clean(allocations); watch.resume(); drop(item); watch.empty(allocations);
    }}
}
#[test]
fn type_wrapping_and_unwrapping_transfer_deep_and_mixed_children() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures(); for (t,_) in &all { sweep_wrap(t); }
    let deep=bridge_deep_fixture(); sweep_wrap(deep.ty.as_ref().unwrap());
    let wide=wide_bridge_value(); sweep_wrap(list_parts(&wide).0);
}
fn sweep_begin(source: &Type, hint: usize) {
    let n=transfer_tokens(RootInput::Ty(source)); let setup_g=schedule(source,None).len();
    let mut expected=vec![(Site::Vector,workspace_bytes(n+2))];
    if hint!=0 { expected.push((Site::Vector,hint*size_of::<Value>())); } let g=expected.len();
    for mode in 0..3 { for nth in 1..=if mode==2 { 1 } else { g+1 } {
        let mut cx=if mode==2 { Cx::new() } else { transfer_fault(nth,mode==1) };
        let watch=Watch::begin(); let ty=Root::<Type>::try_copy(source,&mut Cx::new()).unwrap(); watch.pause();
        let original=RootInput::Ty(ty.as_ref()).marks(); let state=ty.test_transfer_state();
        watch.resume(); let result=ManagedListBuilder::try_new(ty,hint,&mut cx); watch.pause();
        let mut allocations=setup_g;
        let builder=if mode!=2 && nth<=g {
            let (err,ty)=match result { Err(e)=>e,Ok(_)=>panic!("expected list begin refusal") };
            transfer_failure(&cx,err,expected[nth-1],nth); assert_eq!(ty.test_transfer_state(),state);
            assert_eq!(RootInput::Ty(ty.as_ref()).marks(),original); same_type(ty.as_ref(),source);
            allocations+=nth-1; let mut retry=Cx::new(); watch.resume();
            let builder=move_ok(ManagedListBuilder::try_new(ty,hint,&mut retry)); watch.pause();
            assert_eq!(retry.growth_requests(),g); allocations+=g; builder
        } else { assert_eq!(cx.growth_requests(),g); allocations+=g; result.ok().unwrap() };
        let (ty,items)=list_parts(builder.as_ref()); same_type(ty,source); assert!(items.is_empty());
        assert!(list_capacity(builder.as_ref())>=hint); assert_eq!(RootInput::Ty(ty).marks(),original);
        verify_state(builder.test_transfer_state(),n+2); watch.clean(allocations); watch.resume();
        let root=builder.finish(); drop(root); watch.empty(allocations);
    }}
}
#[test]
fn list_begin_preserves_type_and_hint_at_every_refusal() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures(); let deep=bridge_deep_fixture(); let wide=wide_bridge_value();
    for hint in [0,3] {
        for (t,_) in &all { sweep_begin(t,hint); }
        sweep_begin(deep.ty.as_ref().unwrap(),hint); sweep_begin(list_parts(&wide).0,hint);
    }
}
fn sweep_append(source: &Value, hint: usize) {
    let n=transfer_tokens(RootInput::Val(source)); let copy_g=schedule(&Type::Boolean,Some(source)).len();
    for mode in 0..3 { for nth in 1..=if mode==2 { 1 } else { if hint==0 { 3 } else { 2 } } {
        let mut cx=if mode==2 { Cx::new() } else { transfer_fault(nth,mode==1) };
        let watch=Watch::begin(); let mut setup=Cx::new();
        let ty=Root::<Type>::try_text(&mut setup).unwrap(); let mut builder=move_ok(ManagedListBuilder::try_new(ty,hint,&mut setup));
        let incoming=Root::<Value>::try_copy(source,&mut setup).unwrap(); watch.pause();
        assert_eq!(setup.growth_requests(),copy_g+2+usize::from(hint!=0));
        let original=RootInput::Val(incoming.as_ref()).marks(); let source_state=incoming.test_transfer_state();
        let before=builder.test_transfer_state(); let expected=append_schedule(&builder,n); let g=expected.len();
        assert_eq!(g,if hint==0 { 2 } else { 1 });
        watch.resume(); let result=builder.try_push(incoming,&mut cx); watch.pause();
        let mut allocations=setup.growth_requests();
        if mode!=2 && nth<=g {
            let (err,incoming)=result.err().expect("expected append refusal");
            transfer_failure(&cx,err,expected[nth-1],nth);
            assert_eq!(incoming.test_transfer_state(),source_state); assert_eq!(RootInput::Val(incoming.as_ref()).marks(),original);
            same_value(incoming.as_ref(),source); assert_eq!(builder.test_transfer_state().0,before.0);
            let (ty,items)=list_parts(builder.as_ref()); assert_eq!(ty,&Type::Text); assert!(items.is_empty());
            if nth==2 { assert!(builder.test_transfer_state().1>=before.0+n); }
            allocations+=nth-1; let retry_expected=append_schedule(&builder,n); let mut retry=Cx::new();
            watch.resume(); move_ok(builder.try_push(incoming,&mut retry)); watch.pause();
            assert_eq!(retry.growth_requests(),retry_expected.len()); allocations+=retry_expected.len();
        } else { assert!(result.is_ok()); assert_eq!(cx.growth_requests(),g); allocations+=g; }
        let (ty,items)=list_parts(builder.as_ref()); assert_eq!(ty,&Type::Text); assert_eq!(items.len(),1);
        same_value(&items[0],source); assert_eq!(RootInput::Val(&items[0]).marks(),original);
        verify_state(builder.test_transfer_state(),3+n); assert_eq!(transfer_tokens(RootInput::Val(builder.as_ref())),3+n);
        watch.clean(allocations); watch.resume(); let root=builder.finish(); drop(root); watch.empty(allocations);
    }}
}
#[test]
fn list_append_preserves_prefix_and_incoming_at_every_refusal() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures(); let deep=bridge_deep_fixture(); let wide=wide_bridge_value();
    for hint in [0,4] {
        for (_,v) in &all { sweep_append(v,hint); }
        sweep_append(deep.value.as_ref().unwrap(),hint); sweep_append(&wide,hint);
    }
}
#[test]
fn first_middle_last_append_refusal_leaves_complete_guarded_prefix() {
    let _lock=LOCK.lock().unwrap(); let raw=bridge_deep_fixture(); let source=raw.value.as_ref().unwrap();
    for prefix in [0,31,63] { for sticky in [false,true] {
        let mut cx=transfer_fault(1,sticky); let watch=Watch::begin(); let mut setup=Cx::new();
        let ty=Root::<Type>::try_text(&mut setup).unwrap(); let mut builder=move_ok(ManagedListBuilder::try_new(ty,64,&mut setup));
        for _ in 0..prefix { let item=Root::<Value>::try_null(&mut setup).unwrap(); move_ok(builder.try_push(item,&mut setup)); }
        let incoming=Root::<Value>::try_copy(source,&mut setup).unwrap(); watch.pause();
        let original=RootInput::Val(incoming.as_ref()).marks(); let n=transfer_tokens(RootInput::Val(source));
        let expected=append_schedule(&builder,n); assert_eq!(expected.len(),1); let old=builder.test_transfer_state().0;
        watch.resume(); let (err,incoming)=builder.try_push(incoming,&mut cx).err().unwrap(); watch.pause();
        transfer_failure(&cx,err,expected[0],1); assert_eq!(builder.test_transfer_state().0,old);
        let (_,items)=list_parts(builder.as_ref()); assert_eq!(items.len(),prefix); assert!(items.iter().all(|v|matches!(v,Value::Null)));
        assert_eq!(RootInput::Val(incoming.as_ref()).marks(),original);
        let allocations=if sticky {
            // Dropping the closed refusal tuple must also drain the deep source
            // without losing the already accumulated destination prefix.
            watch.resume(); drop((err,incoming)); watch.pause();
            verify_state(builder.test_transfer_state(),old);
            let (_,items)=list_parts(builder.as_ref()); assert_eq!(items.len(),prefix);
            assert!(items.iter().all(|v|matches!(v,Value::Null))); setup.growth_requests()
        } else {
            let mut retry=Cx::new(); watch.resume(); move_ok(builder.try_push(incoming,&mut retry)); watch.pause();
            assert_eq!(retry.growth_requests(),1); let (_,items)=list_parts(builder.as_ref());
            same_value(&items[prefix],source); assert_eq!(RootInput::Val(&items[prefix]).marks(),original);
            verify_state(builder.test_transfer_state(),old+n); setup.growth_requests()+1
        };
        watch.clean(allocations); watch.resume(); drop(builder); watch.empty(allocations);
        assert_eq!(cx.first_failure(),Some(err)); assert_eq!(cx.growth_requests(),1);
    }}
}
#[test]
fn long_append_sequence_keeps_order_exact_credits_and_transfer_identity() {
    let _lock=LOCK.lock().unwrap(); let watch=Watch::begin(); let mut cx=Cx::new();
    let ty=Root::<Type>::try_integer(&mut cx).unwrap(); let mut builder=move_ok(ManagedListBuilder::try_new(ty,0,&mut cx));
    for i in 0..256 {
        let item=Root::<Value>::try_integer(i,&mut cx).unwrap(); move_ok(builder.try_push(item,&mut cx));
        watch.pause(); verify_state(builder.test_transfer_state(),3+2*(i as usize+1));
        assert_eq!(transfer_tokens(RootInput::Val(builder.as_ref())),3+2*(i as usize+1)); watch.resume();
    }
    let root=builder.finish(); watch.pause(); let (ty,items)=list_parts(root.as_ref()); assert_eq!(ty,&Type::Integer);
    assert_eq!(items.len(),256); for (i,value) in items.iter().enumerate() { assert_eq!(value,&Value::Integer { value:i as i64 }); }
    verify_state(root.test_transfer_state(),515); let allocations=cx.growth_requests(); assert_eq!(allocations,770);
    watch.clean(allocations); watch.resume(); drop(root); watch.empty(allocations); assert_eq!(cx.growth_requests(),allocations);
}
#[test]
fn wrong_tag_and_copied_list_unwrap_remain_nonallocating_when_sticky() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures();
    for (source,_) in &all {
        let mut cx=transfer_fault(1,true); let watch=Watch::begin(); let mut setup=Cx::new();
        let root=Root::<Type>::try_copy(source,&mut setup).unwrap();
        let first=product_alloc::try_reserve(&mut Vec::<u8>::new(),1,&mut cx,Site::Bytes).unwrap_err(); watch.pause();
        let state=root.test_transfer_state(); let original=RootInput::Ty(root.as_ref()).marks();
        let expected_child=match root.as_ref() { Type::List { item }=>Some(RootInput::Ty(item).marks()),_=>None };
        watch.resume(); let result=root.into_list_item(); watch.pause();
        match (source,result) {
            (Type::List { item },Ok(root)) => {
                same_type(item,root.as_ref()); assert_eq!(RootInput::Ty(root.as_ref()).marks(),expected_child.unwrap());
                assert_eq!(root.test_transfer_state(),(state.0-2,state.1,state.2)); watch.resume(); drop(root);
            }
            (_,Err(root)) => { same_type(source,root.as_ref()); assert_eq!(root.test_transfer_state(),state);
                assert_eq!(RootInput::Ty(root.as_ref()).marks(),original); watch.resume(); drop(root); }
            _=>panic!("wrong unwrap tag decision"),
        }
        watch.empty(setup.growth_requests()); assert_eq!(cx.first_failure(),Some(first)); assert_eq!(cx.growth_requests(),1);
    }
}
#[test]
fn finished_and_unfinished_lists_drop_without_growth_after_sticky_failure() {
    let _lock=LOCK.lock().unwrap(); let all=fixtures(); let source=&all[13].1;
    for len in [0,1,64] { for finish in [false,true] {
        let mut cx=transfer_fault(1,true); let watch=Watch::begin(); let mut setup=Cx::new();
        let ty=Root::<Type>::try_copy(&all[13].0,&mut setup).unwrap();
        let mut builder=move_ok(ManagedListBuilder::try_new(ty,len,&mut setup));
        for _ in 0..len { let incoming=Root::<Value>::try_copy(source,&mut setup).unwrap(); move_ok(builder.try_push(incoming,&mut setup)); }
        let first=product_alloc::try_reserve(&mut Vec::<u8>::new(),1,&mut cx,Site::Bytes).unwrap_err();
        let allocations=setup.growth_requests();
        if finish { let root=builder.finish(); drop(root); } else { drop(builder); }
        watch.empty(allocations); assert_eq!(cx.first_failure(),Some(first)); assert_eq!(cx.growth_requests(),1);
    }}
}
#[test]
fn synthetic_credit_and_payload_extents_record_overflow_before_move() {
    let _lock=LOCK.lock().unwrap();
    for size in [product_app::OWNED_NODE_BYTES,size_of::<Value>()] {
        let cases=[(usize::MAX,1,None), (usize::MAX/size+1,0,None),
            (isize::MAX as usize/size+1,0,Some((isize::MAX as usize/size+1)*size))];
        for (len,additional,bytes) in cases { for sticky in [false,true] {
            let mut cx=transfer_fault(1,sticky); let watch=Watch::begin();
            let root=Root::<Type>::try_boolean(&mut Cx::new()).unwrap(); watch.pause();
            let state=root.test_transfer_state(); watch.resume();
            let first=cx.extent(len,additional,size,Site::Vector).unwrap_err();
            let result=Root::<Type>::try_list(root,&mut cx); watch.pause();
            assert_eq!(first,AllocationFailure { kind:Kind::CapacityOverflow,site:Site::Vector,requested_bytes:bytes });
            let (later,root)=result.err().unwrap(); assert_eq!(later.kind,Kind::Injected);
            assert_eq!(cx.first_failure(),Some(first)); assert_eq!(cx.growth_requests(),1);
            assert_eq!(root.test_transfer_state(),state); assert_eq!(root.as_ref(),&Type::Boolean);
            watch.resume(); drop(root); watch.empty(1);
        }}
    }
    for hint in [usize::MAX,isize::MAX as usize/size_of::<Value>()+1] { for sticky in [false,true] {
        let mut cx=transfer_fault(2,sticky); let watch=Watch::begin();
        let root=Root::<Type>::try_boolean(&mut Cx::new()).unwrap(); watch.pause(); let state=root.test_transfer_state();
        watch.resume(); let (first,root)=ManagedListBuilder::try_new(root,hint,&mut cx).err().unwrap();
        let later=Root::<Type>::try_boolean(&mut cx).err().unwrap(); watch.pause();
        assert_eq!(first,AllocationFailure { kind:Kind::CapacityOverflow,site:Site::Vector,
            requested_bytes:hint.checked_mul(size_of::<Value>()) });
        assert_eq!(later.kind,Kind::Injected); assert_eq!(cx.first_failure(),Some(first)); assert_eq!(cx.growth_requests(),2);
        assert_eq!(root.test_transfer_state(),state); watch.resume(); drop(root); watch.empty(2);
    }}
    // The largest representable valid extent is arithmetic-only, not an allocation.
    let mut cx=Cx::new(); let n=isize::MAX as usize/product_app::OWNED_NODE_BYTES;
    assert_eq!(cx.extent(n,0,product_app::OWNED_NODE_BYTES,Site::Vector).unwrap(),(n,n*product_app::OWNED_NODE_BYTES));
    assert_eq!(cx.growth_requests(),0); assert_eq!(cx.first_failure(),None);
}
#[test]
fn shared_constructor_context_keeps_prefix_and_first_cause() {
    let _lock=LOCK.lock().unwrap();
    for sticky in [false,true] {
        let mut cx=transfer_fault(2,sticky); let watch=Watch::begin();
        let prefix=product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap();
        let first=Root::<Value>::try_reference("e","r",&mut cx).err().unwrap();
        let later=Root::<Value>::try_reference("e","r",&mut cx); watch.pause();
        assert_eq!(first,AllocationFailure { kind:Kind::Injected,site:Site::Vector,requested_bytes:Some(workspace_bytes(3)) });
        assert_eq!(cx.first_failure(),Some(first));
        if sticky { assert!(later.is_err()); assert_eq!(cx.growth_requests(),3); }
        else { let root=later.unwrap(); assert_eq!(root.as_ref(),&Value::Reference { entity:"e".into(),record:"r".into() });
            assert_eq!(cx.growth_requests(),5); watch.resume(); drop(root); watch.pause(); }
        let allocations=if sticky { 1 } else { 4 }; watch.resume(); drop(prefix); watch.empty(allocations);
    }
}
