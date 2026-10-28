// R07a standalone harness: one actual allocation module, no evaluator routing.
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[path = "../src/product_json.rs"] mod product_json;
#[path = "../src/product_app.rs"] mod product_app;
#[cfg(not(contract_value_external))]
#[path = "../src/product_contract.rs"] mod product_contract;
#[cfg(contract_value_external)]
use contract_value_contract::product_contract;
#[path = "../src/product_runtime/owned_env.rs"] mod owned_env;
use owned_env::ManagedEnv;
use product_contract::{Type, DataValue as Value};
use product_alloc::{AllocationContext as Cx, AllocationFailure,
    AllocationFailureKind as Kind, AllocationSite as Site};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, collections::BTreeMap,
    fmt::{self, Write}, mem::size_of, num::NonZeroUsize,
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// A passive observer, never a failing global allocator. Exact emitted and
// linked hooks must be qualified before these observations count as evidence.
// TLS is const/no-Drop and primed before observation. No locks, formatting,
// allocation, panic, or thread identity lookup occur inside allocator hooks.
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }
const RECORDS: usize = 16384;
const SLOTS: usize = RECORDS * 2;
static PTR: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static SIZE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static ALIGN: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static LIVE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
// Slots contain index+1. Keeping a dead pointer's slot avoids broken probe
// chains; allocator address reuse replaces that slot's record index.
static TABLE: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
static USED: AtomicUsize = AtomicUsize::new(0);
static BAD: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
struct Observer;
fn observing() -> bool {
    match ACTIVE.try_with(Cell::get) {
        Ok(a) => a,
        Err(_) => { BAD.fetch_add(1, Ordering::Relaxed); false }
    }
}
fn slot(p: usize) -> Option<usize> {
    let start = (p >> 3).wrapping_mul(0x9e3779b1) & (SLOTS - 1);
    for offset in 0..SLOTS {
        let i = (start + offset) & (SLOTS - 1);
        let record = TABLE[i].load(Ordering::Relaxed);
        if record == 0 || PTR[record - 1].load(Ordering::Relaxed) == p {
            return Some(i);
        }
    }
    BAD.fetch_add(1, Ordering::Relaxed);
    None
}
fn register(p: *mut u8, size: usize, align: usize) {
    if p.is_null() { BAD.fetch_add(1, Ordering::Relaxed); return; }
    let i = USED.fetch_add(1, Ordering::Relaxed);
    if i >= RECORDS { BAD.fetch_add(1, Ordering::Relaxed); return; }
    if let Some(s) = slot(p as usize) {
        let old = TABLE[s].load(Ordering::Relaxed);
        if old != 0 && LIVE[old - 1].load(Ordering::Relaxed) != 0 {
            BAD.fetch_add(1, Ordering::Relaxed);
        }
        PTR[i].store(p as usize, Ordering::Relaxed);
        SIZE[i].store(size, Ordering::Relaxed);
        ALIGN[i].store(align, Ordering::Relaxed);
        LIVE[i].store(1, Ordering::Relaxed);
        TABLE[s].store(i + 1, Ordering::Relaxed);
    }
}
fn release(p: *mut u8, size: usize, align: usize) {
    if let Some(s) = slot(p as usize) {
        let record = TABLE[s].load(Ordering::Relaxed);
        if record == 0 { BAD.fetch_add(1, Ordering::Relaxed); return; }
        let i = record - 1;
        if LIVE[i].swap(0, Ordering::Relaxed) != 1
            || SIZE[i].load(Ordering::Relaxed) != size
            || ALIGN[i].load(Ordering::Relaxed) != align {
            BAD.fetch_add(1, Ordering::Relaxed);
        }
    }
}
unsafe impl GlobalAlloc for Observer {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if observing() { register(p, l.size(), l.align()); } p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if observing() { register(p, l.size(), l.align()); } p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if observing() { release(p, l.size(), l.align()); }
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, n) };
        if observing() {
            if !q.is_null() { release(p, l.size(), l.align()); }
            register(q, n, l.align());
        } q
    }
}
#[global_allocator] static ALLOCATOR: Observer = Observer;
struct Watch;
impl Watch {
    fn begin() -> Self {
        ACTIVE.with(|a| a.set(false));
        USED.store(0, Ordering::Relaxed); BAD.store(0, Ordering::Relaxed);
        for slot in &TABLE { slot.store(0, Ordering::Relaxed); }
        ACTIVE.with(|a| a.set(true)); Self
    }
    fn pause(&self) { ACTIVE.with(|a| a.set(false)); }
    fn resume(&self) { ACTIVE.with(|a| a.set(true)); }
    fn clean(&self, allocations: usize) {
        self.pause();
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer invariant");
        assert_eq!(USED.load(Ordering::Relaxed), allocations);
    }
    fn empty(&self, allocations: usize) {
        self.clean(allocations);
        for i in 0..allocations { assert_eq!(LIVE[i].load(Ordering::Relaxed), 0); }
    }
}
impl Drop for Watch { fn drop(&mut self) { self.pause(); } }

// Independent historical-shaped shallow oracle. It deliberately does not use
// managed copy, encoder, or PreparedAppEq to obtain expected values.
#[derive(Clone, Debug, Eq, PartialEq)]
enum OldType { Boolean, Integer, Text, Date, Reference(String), List(Box<OldType>), Optional(Box<OldType>) }
#[derive(Clone, Debug, Eq, PartialEq)]
enum OldValue { Null, Boolean(bool), Integer(i64), Text(String), Date(i32),
    Reference(String, String), List(OldType, Vec<OldValue>) }
fn old_type(t: &Type) -> OldType {
    match t {
        Type::Boolean => OldType::Boolean, Type::Integer => OldType::Integer,
        Type::Text => OldType::Text, Type::Date => OldType::Date,
        Type::Reference { entity } => OldType::Reference(entity.clone()),
        Type::List { item } => OldType::List(Box::new(old_type(item))),
        Type::Optional { item } => OldType::Optional(Box::new(old_type(item))),
    }
}
fn old_value(v: &Value) -> OldValue {
    match v {
        Value::Null => OldValue::Null, Value::Boolean { value } => OldValue::Boolean(*value),
        Value::Integer { value } => OldValue::Integer(*value),
        Value::Text { value } => OldValue::Text(value.clone()),
        Value::Date { days } => OldValue::Date(*days),
        Value::Reference { entity, record } => OldValue::Reference(entity.clone(), record.clone()),
        Value::List { item_type, items } => OldValue::List(old_type(item_type), items.iter().map(old_value).collect()),
    }
}
fn old_env(env: &ManagedEnv) -> BTreeMap<String, (OldType, OldValue)> {
    env.iter().map(|(k,t,v)| (k.to_owned(), (old_type(t), old_value(v)))).collect()
}

// Fixed fixtures, not input policy: all seven Type and seven DataValue variants,
// primitive extremes, empty/nonempty strings/lists, and nested mixed payloads.
fn fixtures() -> Vec<(Type, Value)> {
    vec![
        (Type::Boolean, Value::Null),
        (Type::Boolean, Value::Boolean { value: false }),
        (Type::Boolean, Value::Boolean { value: true }),
        (Type::Integer, Value::Integer { value: i64::MIN }),
        (Type::Integer, Value::Integer { value: i64::MAX }),
        (Type::Text, Value::Text { value: String::new() }),
        (Type::Text, Value::Text { value: "ΟΣ ΟΣΑ İ e\u{301}\0中".into() }),
        (Type::Date, Value::Date { days: i32::MIN }),
        (Type::Date, Value::Date { days: i32::MAX }),
        (Type::Reference { entity: String::new() }, Value::Reference { entity: String::new(), record: String::new() }),
        (Type::Reference { entity: "entity".into() }, Value::Reference { entity: "entity".into(), record: "record".into() }),
        (Type::List { item: Box::new(Type::Text) }, Value::List { item_type: Type::Text, items: vec![] }),
        (Type::Optional { item: Box::new(Type::Text) }, Value::Null),
        (Type::Optional { item: Box::new(Type::List { item: Box::new(Type::Reference { entity: "e".into() }) }) },
            Value::List { item_type: Type::Optional { item: Box::new(Type::Reference { entity: "e".into() }) },
                items: vec![Value::Null, Value::Reference { entity: "e".into(), record: "r".into() },
                    Value::List { item_type: Type::Text, items: vec![Value::Text { value: "Σ".into() }] }] }),
    ]
}
const KEYS: [&str; 16] = ["", "A", "a", "a\0", "a\0b", "b", "c", "d", "e\u{301}", "k", "z", "é", "Σ", "σ", "中", "😀"];

// Independent iterative walk also handles raw depth-2048 fixtures safely.
#[derive(Clone, Copy)] enum Node<'a> { Ty(&'a Type), Val(&'a Value), BoxEnd, Items(&'a [Value]), Text(&'a str) }
fn same_type(a: &Type, b: &Type) {
    let (mut a, mut b) = (a,b);
    loop {
        match (a,b) {
            (Type::Boolean,Type::Boolean)|(Type::Integer,Type::Integer)|
            (Type::Text,Type::Text)|(Type::Date,Type::Date) => return,
            (Type::Reference { entity:a },Type::Reference { entity:b }) => { assert_eq!(a,b); return; }
            (Type::List { item:a1 },Type::List { item:b1 })|
            (Type::Optional { item:a1 },Type::Optional { item:b1 }) => { a=a1; b=b1; }
            _ => panic!("type variant mismatch"),
        }
    }
}
fn same_value(a: &Value, b: &Value) {
    let mut pending = vec![(a,b)];
    while let Some((a,b)) = pending.pop() {
        match (a,b) {
            (Value::Null,Value::Null) => (),
            (Value::Boolean { value:a },Value::Boolean { value:b }) => assert_eq!(a,b),
            (Value::Integer { value:a },Value::Integer { value:b }) => assert_eq!(a,b),
            (Value::Text { value:a },Value::Text { value:b }) => assert_eq!(a,b),
            (Value::Date { days:a },Value::Date { days:b }) => assert_eq!(a,b),
            (Value::Reference { entity:a, record:x },Value::Reference { entity:b, record:y }) => { assert_eq!(a,b); assert_eq!(x,y); }
            (Value::List { item_type:a, items:x },Value::List { item_type:b, items:y }) => {
                same_type(a,b); assert_eq!(x.len(),y.len()); pending.extend(x.iter().zip(y));
            }
            _ => panic!("value variant mismatch"),
        }
    }
}
type Mark = (usize,usize,usize);
fn marks(env: &ManagedEnv) -> Vec<Mark> {
    let mut out = vec![(env.len(),env.capacity(),0)];
    let mut pending = Vec::new();
    for (key,t,v) in env.iter() {
        out.push((key.as_ptr() as usize,key.len(),0));
        out.push((t as *const Type as usize,v as *const Value as usize,0));
        pending.push(Node::Ty(t)); pending.push(Node::Val(v));
    }
    while let Some(n) = pending.pop() {
        match n {
            Node::Ty(Type::List { item }|Type::Optional { item }) => {
                out.push((&**item as *const Type as usize,0,0)); pending.push(Node::Ty(item));
            }
            Node::Ty(Type::Reference { entity }) => pending.push(Node::Text(entity)),
            Node::Val(Value::Text { value }) => pending.push(Node::Text(value)),
            Node::Val(Value::Reference { entity,record }) => { pending.push(Node::Text(entity)); pending.push(Node::Text(record)); }
            Node::Val(Value::List { item_type,items }) => {
                out.push((items.as_ptr() as usize,items.len(),items.capacity()));
                pending.push(Node::Ty(item_type)); pending.extend(items.iter().map(Node::Val));
            }
            Node::Text(s) => out.push((s.as_ptr() as usize,s.len(),0)),
            _ => (),
        }
    }
    out
}

// Independent allocation schedule: geometric borrowed plan, two retained
// owner workspaces, then declaration-order payload traversal/Box assembly.
// Counting includes scalar/string fields, Vec containers, and Box edges.
fn schedule(t: &Type, v: Option<&Value>) -> Vec<(Site,usize)> {
    let root = match v { Some(v) => Node::Val(v), None => Node::Ty(t) };
    let mut pending = vec![root];
    let mut payload = Vec::new();
    let mut tokens = 0usize;
    while let Some(node) = pending.pop() {
        match node {
            Node::BoxEnd => { payload.push((Site::Box,size_of::<Type>())); continue; }
            _ => tokens = tokens.checked_add(1).unwrap(),
        }
        match node {
            Node::Ty(Type::List { item }|Type::Optional { item }) => {
                tokens += 1; pending.push(Node::BoxEnd); pending.push(Node::Ty(item));
            }
            Node::Ty(Type::Reference { entity }) => pending.push(Node::Text(entity)),
            Node::Val(Value::Boolean { .. }|Value::Integer { .. }|Value::Date { .. }) => tokens += 1,
            Node::Val(Value::Text { value }) => pending.push(Node::Text(value)),
            Node::Val(Value::Reference { entity,record }) => { pending.push(Node::Text(record)); pending.push(Node::Text(entity)); }
            Node::Val(Value::List { item_type,items }) => { pending.push(Node::Items(items)); pending.push(Node::Ty(item_type)); }
            Node::Items(items) => {
                if !items.is_empty() { payload.push((Site::Vector,items.len().checked_mul(size_of::<Value>()).unwrap())); }
                pending.extend(items.iter().rev().map(Node::Val));
            }
            Node::Text(s) => if !s.is_empty() { payload.push((Site::String,s.len())); },
            _ => (),
        }
    }
    let mut expected = Vec::new();
    let mut capacity = 1usize;
    loop {
        expected.push((Site::Vector,capacity.checked_mul(product_app::BORROWED_RECORD_BYTES).unwrap()));
        if capacity >= tokens { break; } capacity = capacity.checked_mul(2).unwrap();
    }
    for _ in 0..2 { expected.push((Site::Vector,tokens.checked_mul(product_app::OWNED_NODE_BYTES).unwrap())); }
    expected.extend(payload);
    assert!(expected.len() <= tokens.checked_mul(2).unwrap().checked_add(2).unwrap());
    expected
}
fn binding_schedule(key: &str, t: &Type, v: &Value) -> Vec<(Site,usize)> {
    let mut result = Vec::new();
    if !key.is_empty() { result.push((Site::String,key.len())); }
    result.extend(schedule(t,None)); result.extend(schedule(t,Some(v))); result
}
// All current fields have alignment equal to usize. The test verifies actual
// requested bytes against this independent composition on the exact target.
fn entry_bytes() -> usize {
    size_of::<String>() + size_of::<product_app::OwnedAppGraph<Type>>()
        + size_of::<product_app::OwnedAppGraph<Value>>()
}
fn snapshot_schedule(env: &ManagedEnv) -> Vec<(Site,usize)> {
    let mut out = Vec::new();
    if env.len() != 0 { out.push((Site::OrderedMap,env.len().checked_mul(entry_bytes()).unwrap())); }
    for (k,t,v) in env.iter() { out.extend(binding_schedule(k,t,v)); } out
}
fn seeded(t: &Type, v: &Value, cx: &mut Cx) -> ManagedEnv {
    let mut env = ManagedEnv::new();
    env.try_bind("before",&Type::Integer,&Value::Integer { value: -7 },cx).unwrap();
    env.try_bind("replace",t,v,cx).unwrap();
    env.try_bind("after",&Type::Boolean,&Value::Boolean { value: true },cx).unwrap(); env
}
fn check_seed(env: &ManagedEnv, t: &Type, v: &Value) {
    assert_eq!(env.len(),3);
    for (key,t,v) in [("before",&Type::Integer,&Value::Integer { value: -7 }),
        ("replace",t,v),("after",&Type::Boolean,&Value::Boolean { value: true })] {
        let (a,b) = env.get(key).unwrap(); same_type(a,t); same_value(b,v);
    }
}
#[derive(Clone,Copy)] enum Operation { Vacant, Replace, Snapshot }
fn perform(env: &mut ManagedEnv, op: Operation, t: &Type, v: &Value, cx: &mut Cx)
    -> Result<Option<ManagedEnv>,AllocationFailure> {
    match op {
        Operation::Vacant => { env.try_bind("new",t,v,cx)?; Ok(None) }
        Operation::Replace => { env.try_bind("replace",t,v,cx)?; Ok(None) }
        Operation::Snapshot => env.try_copy(cx).map(Some),
    }
}
static FAULTS: AtomicUsize = AtomicUsize::new(0);
fn fault(n: usize, sticky: bool) -> Cx {
    assert!(FAULTS.fetch_add(1,Ordering::Relaxed) < 40000, "finite fault census");
    Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),sticky)
}
fn sweep(t: &Type, v: &Value, op: Operation, deep: bool) {
    // The old binding is independently deep too. Replacement must dismantle it
    // without allocation; snapshot refusal must preserve it and copied prefixes.
    let reference = seeded(t,v,&mut Cx::new());
    let expected = match op {
        Operation::Snapshot => snapshot_schedule(&reference),
        Operation::Vacant => {
            let mut s=binding_schedule("new",t,v);
            if reference.len()==reference.capacity() { s.push((Site::OrderedMap,(reference.len()+1)*entry_bytes())); } s
        }
        Operation::Replace => binding_schedule("replace",t,v),
    };
    let g = expected.len(); assert!(g <= 5000);
    let oracle = if deep { None } else { Some(old_env(&reference)) };
    drop(reference);
    // Two sticky modes, every refusal ordinal, and G+1 successful execution.
    // A separate normal context is also measured to rule out an injected-only path.
    for mode in 0..3 {
        let attempts = if mode==2 { 1 } else { g+1 };
        for nth in 1..=attempts {
            let mut cx = if mode==2 { Cx::new() } else { fault(nth,mode==1) };
            let watch = Watch::begin();
            let mut initial = Cx::new(); let mut env = seeded(t,v,&mut initial);
            let prefix = initial.growth_requests(); watch.clean(prefix);
            let original = marks(&env);
            let original_live:Vec<_>=(0..prefix).map(|i| LIVE[i].load(Ordering::Relaxed)).collect();
            watch.resume(); let result = perform(&mut env,op,t,v,&mut cx); watch.pause();
            let failed = mode!=2 && nth<=g;
            if failed {
                let err = result.err().expect("expected refusal");
                assert_eq!(err,AllocationFailure { kind:Kind::Injected,site:expected[nth-1].0,requested_bytes:Some(expected[nth-1].1) });
                assert_eq!(cx.first_failure(),Some(err)); assert_eq!(cx.growth_requests(),nth);
                assert_eq!(marks(&env),original); check_seed(&env,t,v);
                for (i,live) in original_live.iter().enumerate() { assert_eq!(LIVE[i].load(Ordering::Relaxed),*live); }
                if let Some(old) = &oracle { assert_eq!(&old_env(&env),old); }
                watch.clean(prefix+nth-1);
                // Every initial live block survived; all partial new blocks died.
                for i in prefix..prefix+nth-1 { assert_eq!(LIVE[i].load(Ordering::Relaxed),0); }
            } else {
                assert_eq!(cx.growth_requests(),g); assert_eq!(cx.first_failure(),None);
                let copied=result.unwrap();
                if let Some(copy)=&copied {
                    check_seed(copy,t,v); check_seed(&env,t,v); assert_eq!(marks(&env),original);
                    if let Some(old)=&oracle { assert_eq!(&old_env(copy),old); }
                    for (k,a,b) in copy.iter() {
                        let (x,y)=env.get(k).unwrap();
                        assert!(!std::ptr::eq(a,x)); assert!(!std::ptr::eq(b,y));
                    }
                } else {
                    let key=if matches!(op,Operation::Vacant){"new"}else{"replace"};
                    let (a,b)=env.get(key).unwrap(); same_type(a,t); same_value(b,v);
                    if let Some(old)=&oracle {
                        let mut old=old.clone(); old.insert(key.to_owned(),(old_type(t),old_value(v)));
                        assert_eq!(old_env(&env),old);
                    }
                }
                watch.clean(prefix+g); watch.resume(); drop(copied); watch.pause();
            }
            let requests=cx.growth_requests(); let allocations=USED.load(Ordering::Relaxed);
            watch.resume(); drop(env); watch.empty(allocations);
            assert_eq!(cx.growth_requests(),requests);
        }
    }
}

#[test]
fn empty_lookup_and_copy_do_not_request_growth_even_after_sticky_failure() {
    let _lock=LOCK.lock().unwrap();
    let mut cx=fault(1,true); let mut bytes=Vec::<u8>::new();
    let err=product_alloc::try_reserve(&mut bytes,1,&mut cx,Site::Bytes).unwrap_err();
    let watch=Watch::begin(); let empty=ManagedEnv::new();
    let missing=empty.get("missing").is_none(); let copy=empty.try_copy(&mut cx).unwrap();
    let len=copy.len(); drop(copy); drop(empty); watch.empty(0);
    assert!(missing); assert_eq!(len,0); assert_eq!(cx.growth_requests(),1);
    assert_eq!(cx.first_failure(),Some(err));
}
#[test]
fn variants_and_key_orders_match_independent_btree_insert_and_clone() {
    let _lock=LOCK.lock().unwrap(); let fixtures=fixtures();
    for order in [ (0..16).collect::<Vec<_>>(), (0..16).rev().collect(),
        vec![7,0,15,3,12,1,14,6,9,2,13,4,11,5,10,8] ] {
        let mut env=ManagedEnv::new(); let mut old=BTreeMap::new();
        for i in order {
            let (t,v)=&fixtures[i%fixtures.len()];
            env.try_bind(KEYS[i],t,v,&mut Cx::new()).unwrap();
            old.insert(KEYS[i].to_owned(),(old_type(t),old_value(v)));
        }
        assert_eq!(old_env(&env),old);
        let key_pointer=env.iter().find(|(k,_,_)| *k=="a").unwrap().0.as_ptr();
        for (t,v) in &fixtures {
            env.try_bind("a",t,v,&mut Cx::new()).unwrap();
            old.insert("a".to_owned(),(old_type(t),old_value(v)));
            assert_eq!(old_env(&env),old);
            assert_eq!(env.iter().find(|(k,_,_)| *k=="a").unwrap().0.as_ptr(),key_pointer);
        }
        let copy=env.try_copy(&mut Cx::new()).unwrap(); assert_eq!(old_env(&copy),old.clone());
        assert_eq!(env.iter().map(|(k,_,_)|k).collect::<Vec<_>>(),old.keys().map(String::as_str).collect::<Vec<_>>());
    }
}
#[test]
fn snapshots_remain_independent_after_source_rebinding_and_drop() {
    let _lock=LOCK.lock().unwrap(); let f=fixtures(); let (t,v)=&f[13];
    let mut env=seeded(t,v,&mut Cx::new()); let expected=old_env(&env);
    let copy=env.try_copy(&mut Cx::new()).unwrap();
    let a=env.get("replace").unwrap(); let b=copy.get("replace").unwrap();
    assert!(!std::ptr::eq(a.0,b.0)); assert!(!std::ptr::eq(a.1,b.1));
    env.try_bind("replace",&Type::Date,&Value::Date { days: 0 },&mut Cx::new()).unwrap();
    env.try_bind("new",&Type::Text,&Value::Text { value:"new".into() },&mut Cx::new()).unwrap();
    assert_ne!(old_env(&env),expected); drop(env); assert_eq!(old_env(&copy),expected);
}
#[test]
fn every_shallow_growth_preserves_failure_identity_order_and_owners() {
    let _lock=LOCK.lock().unwrap();
    for (t,v) in fixtures() {
        for op in [Operation::Vacant,Operation::Replace,Operation::Snapshot] { sweep(&t,&v,op,false); }
    }
}

// Raw test sources are dismantled explicitly and iteratively, including on
// unwind. The helper never claims to manage the caller's original naked tree.
struct Deep { ty: Option<Type>, value: Option<Value> }
fn drop_type(mut t: Type) {
    loop { match t { Type::List { item }|Type::Optional { item } => t=*item, _ => break } }
}
impl Drop for Deep {
    fn drop(&mut self) {
        if let Some(t)=self.ty.take() { drop_type(t); }
        if let Some(mut v)=self.value.take() {
            loop {
                match v {
                    Value::List { item_type,mut items } => {
                        drop_type(item_type);
                        if items.len()==1 { v=items.pop().unwrap(); } else { break; }
                    }
                    _ => break,
                }
            }
        }
    }
}
fn deep_fixture() -> Deep {
    let mut source=Deep { ty:Some(Type::Reference { entity:"leaf".into() }),
        value:Some(Value::Text { value:"leaf".into() }) };
    for i in 0..2048 {
        let t=source.ty.take().unwrap();
        source.ty=Some(if i%2==0 { Type::List { item:Box::new(t) } } else { Type::Optional { item:Box::new(t) } });
        source.value=Some(Value::List { item_type:Type::Text,items:vec![source.value.take().unwrap()] });
    } source
}
#[test]
fn deep_binding_snapshot_refusal_and_replacement_teardown_are_iterative() {
    let _lock=LOCK.lock().unwrap(); let source=deep_fixture();
    let (t,v)=(source.ty.as_ref().unwrap(),source.value.as_ref().unwrap());
    for op in [Operation::Vacant,Operation::Replace,Operation::Snapshot] { sweep(t,v,op,true); }
    // Also replace a nonempty deep binding by a shallow value, so the final
    // environment cannot hide a deferred recursive destruction of the old tree.
    let watch=Watch::begin(); let mut cx=Cx::new(); let mut env=seeded(t,v,&mut cx);
    env.try_bind("replace",&Type::Text,&Value::Null,&mut cx).unwrap();
    let count=cx.growth_requests(); drop(env); watch.empty(count);
}
#[test]
fn wide_nested_values_preserve_all_elements_and_failure_prefixes() {
    let _lock=LOCK.lock().unwrap();
    let value=Value::List { item_type:Type::Text,items:(0..64).map(|i|
        Value::List { item_type:Type::Optional { item:Box::new(Type::Text) },
            items:vec![Value::Null,Value::Text { value:format!("item-{i}") }] }).collect() };
    for op in [Operation::Vacant,Operation::Replace,Operation::Snapshot] { sweep(&Type::Text,&value,op,false); }
}
#[test]
fn shared_context_prefix_and_one_shot_recovery_keep_the_first_failure() {
    let _lock=LOCK.lock().unwrap();
    for sticky in [false,true] {
        let mut cx=fault(2,sticky);
        let watch=Watch::begin();
        let prefix=product_alloc::try_copy_str("prefix",&mut cx,Site::String).unwrap();
        let mut env=ManagedEnv::new();
        let err=env.try_bind("key",&Type::Text,&Value::Null,&mut cx).unwrap_err();
        watch.pause();
        assert_eq!(cx.growth_requests(),2); assert_eq!(cx.first_failure(),Some(err));
        assert_eq!(err.site,Site::String); assert_eq!(err.requested_bytes,Some(3));
        assert_eq!(env.len(),0);
        watch.resume(); let empty=env.try_copy(&mut cx).unwrap(); drop(empty);
        let later=env.try_bind("later-key",&Type::Text,&Value::Null,&mut cx); watch.pause();
        if sticky {
            assert_eq!(later,Err(AllocationFailure { kind:Kind::Injected,site:Site::String,requested_bytes:Some(9) }));
            assert_eq!(cx.growth_requests(),3);
        }
        else { assert!(later.is_ok()); assert_eq!(cx.growth_requests(),2+binding_schedule("later-key",&Type::Text,&Value::Null).len()+1); }
        assert_eq!(cx.first_failure(),Some(err));
        let count=USED.load(Ordering::Relaxed); watch.resume(); drop(env); drop(prefix); watch.empty(count);
    }
}
struct Fixed { bytes:[u8;128], len:usize }
impl Write for Fixed {
    fn write_str(&mut self,s:&str)->fmt::Result {
        let end=self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        let target=self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(s.as_bytes()); self.len=end; Ok(())
    }
}
#[test]
fn returned_allocation_failure_formats_into_fixed_storage_without_allocation() {
    let _lock=LOCK.lock().unwrap(); let mut cx=fault(1,true); let mut env=ManagedEnv::new();
    let mut fixed=Fixed { bytes:[0;128],len:0 };
    let watch=Watch::begin();
    let err=env.try_bind("key",&Type::Text,&Value::Null,&mut cx).unwrap_err();
    let rendered=write!(&mut fixed,"{err}"); drop(env); watch.empty(0);
    assert!(rendered.is_ok()); assert_eq!(cx.first_failure(),Some(err));
    assert_eq!(&fixed.bytes[..fixed.len],b"allocation failure (injected) at string: 3 bytes");
}
