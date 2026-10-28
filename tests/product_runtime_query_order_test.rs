#![allow(unexpected_cfgs)] // Intentional standalone switch; not a compiler-error allowance.
// Unrouted helper harness. Ordinary Cargo uses the real source contract; bounded
// standalone validation uses that same unchanged contract compiled without tests.
#[path = "../src/product_runtime/query_order.rs"] mod query_order;
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[cfg(query_contract_external)]
use query_contract::product_contract;
#[cfg(not(query_contract_external))]
#[path = "../src/product_contract.rs"] mod product_contract;
use product_contract::{DataValue as V, Expr, Record, SortKey, Type};
use query_order::{try_sort_candidate_ids, try_sort_rows, QueryOrderError, QueryRow};
use product_alloc::{AllocationContext as Cx, AllocationFailure,
    AllocationFailureKind as Kind, AllocationSite as Site};
use std::{alloc::{GlobalAlloc, Layout, System}, cell::Cell, cmp::Ordering as Cmp,
    collections::BTreeMap, fmt::{self, Write}, mem::size_of, num::NonZeroUsize,
    sync::{Mutex, atomic::{AtomicUsize, Ordering}}};

// Observation only: never inject null into the global allocator. This is the
// accepted R09 observation pattern; its exact emitted/linked hooks need fresh
// qualification. Oracles, inputs, and assertion formatting stay outside Watch.
thread_local! { static ACTIVE: Cell<bool> = const { Cell::new(false) }; }
const RECORDS: usize = 8;
static PTR: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static SIZE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static ALIGN: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static LIVE: [AtomicUsize; RECORDS] = [const { AtomicUsize::new(0) }; RECORDS];
static USED: AtomicUsize = AtomicUsize::new(0);
static BAD: AtomicUsize = AtomicUsize::new(0);
static LOCK: Mutex<()> = Mutex::new(());
struct Observer;
fn observing() -> bool {
    match ACTIVE.try_with(Cell::get) {
        Ok(active) => active,
        Err(_) => { BAD.fetch_add(1, Ordering::Relaxed); false }
    }
}
fn register(p: *mut u8, size: usize, align: usize) {
    if p.is_null() { BAD.fetch_add(1, Ordering::Relaxed); return; }
    let i = USED.fetch_add(1, Ordering::Relaxed);
    if i >= RECORDS { BAD.fetch_add(1, Ordering::Relaxed); return; }
    PTR[i].store(p as usize, Ordering::Relaxed);
    SIZE[i].store(size, Ordering::Relaxed);
    ALIGN[i].store(align, Ordering::Relaxed);
    LIVE[i].store(1, Ordering::Relaxed);
}
fn release(p: *mut u8, size: usize, align: usize) {
    for i in (0..USED.load(Ordering::Relaxed).min(RECORDS)).rev() {
        if PTR[i].load(Ordering::Relaxed) == p as usize
            && LIVE[i].load(Ordering::Relaxed) == 1 {
            if SIZE[i].load(Ordering::Relaxed) != size
                || ALIGN[i].load(Ordering::Relaxed) != align {
                BAD.fetch_add(1, Ordering::Relaxed);
            }
            LIVE[i].store(0, Ordering::Relaxed);
            return;
        }
    }
    BAD.fetch_add(1, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for Observer {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if observing() { register(p, l.size(), l.align()); }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if observing() { register(p, l.size(), l.align()); }
        p
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
        }
        q
    }
}
#[global_allocator]
static ALLOCATOR: Observer = Observer;
struct Watch;
impl Watch {
    fn begin() -> Self {
        ACTIVE.with(|a| a.set(false));
        USED.store(0, Ordering::Relaxed);
        BAD.store(0, Ordering::Relaxed);
        ACTIVE.with(|a| a.set(true));
        Self
    }
    fn pause(&self) { ACTIVE.with(|a| a.set(false)); }
    fn resume(&self) { ACTIVE.with(|a| a.set(true)); }
    fn clean(&self, count: usize) {
        self.pause();
        assert_eq!(BAD.load(Ordering::Relaxed), 0, "observer invariant");
        assert_eq!(USED.load(Ordering::Relaxed), count);
    }
    fn empty(&self, count: usize) {
        self.clean(count);
        for i in 0..count { assert_eq!(LIVE[i].load(Ordering::Relaxed), 0); }
    }
}
impl Drop for Watch { fn drop(&mut self) { self.pause(); } }

// Independent original eval.rs59..76 and243..255 oracle. Never use the new
// byte loop/comparator to construct expected results. Oracle sorting and every
// clone/fixture/diagnostic/destructor run outside the fault/observation window.
fn old_value(a: &V, b: &V) -> Cmp {
    match (a, b) {
        (V::Boolean { value: a }, V::Boolean { value: b }) => a.cmp(b),
        (V::Integer { value: a }, V::Integer { value: b }) => a.cmp(b),
        (V::Text { value: a }, V::Text { value: b }) => a.cmp(b),
        (V::Date { days: a }, V::Date { days: b }) => a.cmp(b),
        (V::Reference { entity: a, record: x }, V::Reference { entity: b, record: y }) =>
            (a, x).cmp(&(b, y)),
        _ => Cmp::Equal,
    }
}
fn old_row(a: &QueryRow, b: &QueryRow, sort: &[SortKey]) -> Cmp {
    for (i, key) in sort.iter().enumerate() {
        let c = old_value(&a.1[i], &b.1[i]);
        if c != Cmp::Equal { return if key.descending { c.reverse() } else { c }; }
    }
    a.0.cmp(&b.0)
}
fn int(n: i64) -> V { V::Integer { value: n } }
fn txt(s: &str) -> V { V::Text { value: s.into() } }
fn reference(e: &str, r: &str) -> V { V::Reference { entity: e.into(), record: r.into() } }
fn keys(n: usize, mask: usize) -> Vec<SortKey> {
    (0..n).map(|i| SortKey { value: Expr::Literal { value_type: Type::Text, value: V::Null },
        descending: mask & (1 << i) != 0 }).collect()
}
fn row(id: &str, values: Vec<V>, label: i64) -> QueryRow {
    (id.into(), values, V::List { item_type: Type::Text,
        items: vec![int(label), txt(&format!("payload-{label}"))] })
}
fn record(id: &str, label: u64) -> Record {
    Record { entity: "entity".into(), id: id.into(), revision: label,
        created_program: product_contract::Digest::try_from("0".repeat(64)).unwrap(), archived: false, values: BTreeMap::new() }
}
fn context(mode: usize) -> Cx {
    if mode == 0 { Cx::new() } else {
        Cx::fail_nth_growth(NonZeroUsize::new((mode + 1) / 2).unwrap(), mode % 2 == 1)
    }
}
fn expected_failure(n: usize) -> AllocationFailure {
    AllocationFailure { kind: Kind::Injected, site: Site::Vector,
        requested_bytes: Some(n * size_of::<usize>()) }
}
fn height(n: usize) -> u128 {
    if n < 2 { 0 } else { (usize::BITS - (n - 1).leading_zeros()) as u128 }
}
fn bound(n: usize, k: usize, bytes: usize, success: bool) {
    let c = query_order::test_counts();
    if n < 2 || !success { assert_eq!(c, query_order::TestCounts::default()); return; }
    let n = n as u128; let h = height(n as usize);
    assert!(c.comparisons <= n * h);
    assert!(c.keys <= c.comparisons * k as u128);
    assert!(c.byte_pairs <= c.comparisons * bytes as u128);
    assert_eq!(c.init_stores, 2 * n);
    assert_eq!(c.merge_stores, n * h);
    assert_eq!(c.inverse_stores, n);
    assert_eq!(c.passes, h);
    assert!(c.swaps <= n - 1);
}
// Snapshot every owned vector/string buffer, including nested payloads; fixed
// test depth is traversed iteratively outside observation. Sorting the tuples
// makes this invariant independent of the row permutation.
fn owners(rows: &[QueryRow]) -> Vec<(usize, usize, usize)> {
    fn string(out: &mut Vec<(usize, usize, usize)>, s: &String) {
        out.push((s.as_ptr() as usize, s.len(), s.capacity()));
    }
    let mut out = vec![]; let mut pending = vec![];
    for r in rows {
        string(&mut out, &r.0); out.push((r.1.as_ptr() as usize, r.1.len(), r.1.capacity()));
        pending.extend(r.1.iter()); pending.push(&r.2);
    }
    while let Some(v) = pending.pop() {
        match v {
            V::Text { value } => string(&mut out, value),
            V::Reference { entity, record } => { string(&mut out, entity); string(&mut out, record); }
            V::List { items, .. } => {
                out.push((items.as_ptr() as usize, items.len(), items.capacity()));
                pending.extend(items.iter());
            }
            _ => {}
        }
    }
    out.sort_unstable(); out
}
fn byte_bound(rows: &[QueryRow], sort: &[SortKey]) -> usize {
    let mut b = rows.iter().map(|r| r.0.len()).max().unwrap_or(0);
    for j in 0..sort.len() {
        b += rows.iter().map(|r| match &r.1[j] {
            V::Text { value } => value.len(),
            V::Reference { entity, record } => entity.len() + record.len(), _ => 0,
        }).max().unwrap_or(0);
    }
    b
}
fn check_rows(input: &[QueryRow], sort: &[SortKey], total: bool) {
    let mut expected = input.to_vec();
    if total { expected.sort_by(|a, b| old_row(a, b, sort)); }
    let n = input.len(); let bytes = byte_bound(input, sort);
    for mode in 0..7 {
        let mut rows = input.to_vec(); let before = owners(&rows);
        let outer = (rows.as_ptr(), rows.len(), rows.capacity());
        let mut cx = context(mode);
        query_order::test_reset();
        let watch = Watch::begin();
        let result = try_sort_rows(&mut rows, sort, &mut cx);
        watch.pause();
        let refuse = n >= 2 && (1..=4).contains(&mode);
        let allocations = if n < 2 { 0 } else if refuse { (mode - 1) / 2 } else { 2 };
        watch.empty(allocations);
        assert_eq!((rows.as_ptr(), rows.len(), rows.capacity()), outer);
        assert_eq!(owners(&rows), before);
        if refuse {
            assert_eq!(result, Err(QueryOrderError::Allocation(expected_failure(n))));
            assert_eq!(cx.first_failure(), Some(expected_failure(n)));
            assert_eq!(cx.growth_requests(), (mode + 1) / 2);
            assert_eq!(rows, input);
        } else {
            assert_eq!(result, Ok(()));
            assert_eq!(cx.growth_requests(), if n < 2 { 0 } else { 2 });
            if total { assert_eq!(rows, expected); }
            else {
                let mut actual = rows.clone(); let mut want = input.to_vec();
                actual.sort_by(|a,b| a.0.cmp(&b.0)); want.sort_by(|a,b| a.0.cmp(&b.0));
                assert_eq!(actual, want);
            }
        }
        bound(n, sort.len(), bytes, !refuse);
    }
}
fn check_ids(input: &[Record]) {
    let mut expected: Vec<&Record> = input.iter().collect();
    expected.sort_by(|a,b| a.id.cmp(&b.id));
    let n = input.len(); let bytes = input.iter().map(|r| r.id.len()).max().unwrap_or(0);
    for mode in 0..7 {
        let mut refs: Vec<&Record> = input.iter().collect(); let before = refs.clone();
        let outer = (refs.as_ptr(), refs.len(), refs.capacity()); let mut cx = context(mode);
        query_order::test_reset(); let watch = Watch::begin();
        let result = try_sort_candidate_ids(&mut refs, &mut cx);
        watch.pause();
        let refuse = n >= 2 && (1..=4).contains(&mode);
        watch.empty(if n < 2 { 0 } else if refuse { (mode - 1) / 2 } else { 2 });
        assert_eq!((refs.as_ptr(), refs.len(), refs.capacity()), outer);
        assert_eq!(cx.growth_requests(), if n < 2 { 0 } else if refuse { (mode+1)/2 } else { 2 });
        if refuse { assert_eq!(result, Err(expected_failure(n))); assert_eq!(refs, before); }
        else { assert_eq!(result, Ok(())); assert_eq!(refs, expected); }
        bound(n, 0, bytes, !refuse);
    }
}
fn next_permutation(p: &mut [usize]) -> bool {
    let Some(i) = (1..p.len()).rev().find(|&i| p[i-1] < p[i]) else { return false; };
    let j = (i..p.len()).rev().find(|&j| p[j] > p[i-1]).unwrap();
    p.swap(i-1,j); p[i..].reverse(); true
}

#[test]
fn small_inputs_ignore_keys_and_sticky_context_without_work() {
    let _lock = LOCK.lock().unwrap();
    for n in 0..2 { for k in [0,1,3] { for mode in 0..3 {
        let mut cx = context(mode); let mut s = String::new();
        if mode > 0 { assert!(product_alloc::try_reserve_string(&mut s, 1, &mut cx, Site::String).is_err()); }
        let prior = (cx.growth_requests(), cx.first_failure());
        let sort = keys(k, 7); let mut rows: Vec<_> = (0..n).map(|_| row("b",vec![],0)).collect();
        let expected = rows.clone(); let records = vec![record("z", 1)];
        let mut refs: Vec<_> = records.iter().take(n).collect();
        query_order::test_reset(); let watch = Watch::begin();
        let r = try_sort_rows(&mut rows, &sort, &mut cx);
        let c = try_sort_candidate_ids(&mut refs, &mut cx);
        watch.empty(0); assert_eq!(r, Ok(())); assert_eq!(c, Ok(()));
        assert_eq!(rows, expected); assert_eq!((cx.growth_requests(),cx.first_failure()),prior);
        bound(n,k,0,true);
    } } }
}
#[test]
fn candidate_id_permutations_boundaries_and_duplicate_stability() {
    let _lock = LOCK.lock().unwrap(); let mut count = 0;
    for n in 0..=6 {
        let mut p: Vec<_> = (0..n).collect();
        loop {
            let input: Vec<_> = p.iter().map(|&i| record(&format!("id{i:03}"), i as u64)).collect();
            check_ids(&input); count += 1;
            if !next_permutation(&mut p) { break; }
        }
    }
    assert_eq!(count,874);
    for n in [7,8,9,15,16,17,32,33,64,65,257] { for order in 0..3 {
        let input: Vec<_> = (0..n).map(|i| {
            let j = match order { 0 => i, 1 => n-1-i, _ => if i%2==0 {i/2} else {n-1-i/2} };
            record(&format!("id{j:03}"), j as u64)
        }).collect(); check_ids(&input);
    } }
    for n in [2,7,17,65] { for duplicate in [false,true] {
        let input: Vec<_> = (0..n).map(|i| record(if duplicate && i%3==0 {"a"} else {"b"},i as u64)).collect();
        check_ids(&input);
    } }
}
#[test]
fn total_three_key_all_permutations_directions_prefixes_and_ties() {
    let _lock = LOCK.lock().unwrap(); let mut count = 0;
    for ties in [false,true] {
        let base: Vec<_> = (0..5).map(|i| {
            let j = if ties { i/2 } else { i };
            row(if ties {"same"} else {["e","d","c","b","a"][i]},
                vec![int((j%2) as i64), txt(["z","a","a","b","a"][j]),
                    reference(["e","e","d","e","e"][j],["b","a","z","a","a"][j])],i as i64)
        }).collect();
        let mut p = vec![0,1,2,3,4];
        loop {
            let input: Vec<_> = p.iter().map(|&i| base[i].clone()).collect();
            for mask in 0..8 { for k in 0..=3 { check_rows(&input,&keys(k,mask),true); count+=1; } }
            if !next_permutation(&mut p) { break; }
        }
    }
    assert_eq!(count,7680);
}
fn matrix_values() -> Vec<V> {
    vec![V::Null, V::Boolean{value:false}, V::Boolean{value:true}, int(i64::MIN),int(0),int(i64::MAX),
        txt(""),txt("a"),txt("aa"),txt("a\0b"),txt("é"),txt("e\u{301}"),txt("中文"),txt("😀"),
        V::Date{days:i32::MIN},V::Date{days:0},V::Date{days:i32::MAX},
        reference("",""),reference("e",""),reference("e","a"),reference("e","aa"),reference("f","a"),
        V::List{item_type:Type::Text,items:vec![]},
        V::List{item_type:Type::Text,items:vec![V::List{item_type:Type::Text,items:vec![txt("nested")]}]}]
}
#[test]
fn pairwise_matrix_preserves_fallback_directions_and_ascending_id() {
    let _lock = LOCK.lock().unwrap(); let values = matrix_values(); assert_eq!(values.len(),24);
    let mut count=0;
    for a in &values { for b in &values { for descending in 0..2 {
        for (ai,bi) in [("a","b"),("same","same"),("z","a")] {
            let left=row(ai,vec![a.clone()],0); let right=row(bi,vec![b.clone()],1); let sort=keys(1,descending);
            let expected=old_row(&left,&right,&sort);
            query_order::test_reset(); let watch=Watch::begin();
            let actual=query_order::test_compare_rows(&left,&right,&sort);
            watch.empty(0); assert_eq!(actual,expected); count+=1;
        }
    } } }
    assert_eq!(count,3456);
}
#[test]
fn total_fallback_columns_and_zero_keys_keep_stable_id_order() {
    let _lock = LOCK.lock().unwrap();
    for kind in 0..3 { for order in 0..3 {
        let mut input: Vec<_> = ["b","a","b","c","a"].iter().enumerate().map(|(i,id)|
            row(id, if kind==0 {vec![]} else {vec![if kind==1 {V::Null} else {
                V::List{item_type:Type::Text,items:vec![txt("not traversed")]}}]}, i as i64)).collect();
        if order==1 {input.reverse();} else if order==2 {input.sort_by(|a,b|a.0.cmp(&b.0));}
        check_rows(&input,&keys(usize::from(kind!=0),1),true);
    } }
    check_rows(&[row("b",vec![],0),row("a",vec![],1)],&[],true);
}
#[test]
fn non_total_cycle_only_claims_pairwise_and_permutation_safety() {
    let _lock = LOCK.lock().unwrap(); let sort=keys(1,0);
    let base=vec![row("c",vec![int(0)],0),row("b",vec![V::Null],1),row("a",vec![int(1)],2)];
    assert_eq!(old_row(&base[0],&base[1],&sort),Cmp::Greater);
    assert_eq!(old_row(&base[1],&base[2],&sort),Cmp::Greater);
    assert_eq!(old_row(&base[2],&base[0],&sort),Cmp::Greater);
    let mut p=vec![0,1,2]; let mut count=0;
    loop { let input:Vec<_>=p.iter().map(|&i|base[i].clone()).collect();
        check_rows(&input,&sort,false); count+=1; if !next_permutation(&mut p){break;} }
    assert_eq!(count,6);
}
#[test]
fn long_prefix_text_reference_and_id_work_is_bounded() {
    let _lock = LOCK.lock().unwrap(); let prefix="x".repeat(4096);
    let strings=vec![format!("a{prefix}"),format!("b{prefix}"),format!("{prefix}a"),
        format!("{prefix}b"),prefix.clone(),format!("{prefix}\0"),format!("{prefix}😀")];
    for direction in 0..2 { for kind in 0..3 {
        let input:Vec<_>=strings.iter().enumerate().map(|(i,s)| row(&format!("id{i}"),
            vec![match kind {0=>txt(s),1=>reference(&prefix,s),_=>reference(s,&prefix)}],i as i64)).collect();
        check_rows(&input,&keys(1,direction),true);
    } }
    let input:Vec<_>=strings.iter().enumerate().map(|(i,s)|record(s,i as u64)).collect(); check_ids(&input);
}
#[test]
fn missing_slots_fail_before_reservation_but_extra_slots_are_ignored() {
    let _lock=LOCK.lock().unwrap(); let sort=keys(2,0);
    for mode in 0..2 {
        let mut rows=vec![row("c",vec![int(1),txt("x")],0),row("b",vec![int(2)],1),row("a",vec![],2)];
        let before=rows.clone(); let buffers=owners(&rows); let mut cx=context(mode);
        query_order::test_reset();let watch=Watch::begin();
        let actual=try_sort_rows(&mut rows,&sort,&mut cx);watch.empty(0);
        assert_eq!(actual,Err(QueryOrderError::MissingKeySlots{row:1,required:2,available:1}));
        assert_eq!(cx.growth_requests(),0);assert_eq!(cx.first_failure(),None);
        assert_eq!(rows,before);assert_eq!(owners(&rows),buffers);bound(3,2,0,false);
    }
    check_rows(&[row("b",vec![int(1),txt("ignored")],0),row("a",vec![int(1),V::Null],1)],&keys(1,1),true);
}
fn deep_row(id: &str, label: i64) -> QueryRow {
    let mut r=row(id,vec![int(label)],label);
    for _ in 0..2048 { r.2=V::List{item_type:Type::Text,items:vec![r.2]}; } r
}
fn destroy_iteratively(rows: Vec<QueryRow>) {
    let mut work=vec![];
    for (_,keys,payload) in rows {work.extend(keys);work.push(payload);}
    while let Some(v)=work.pop() {if let V::List{items,..}=v {work.extend(items);}}
}
#[test]
fn deep_payload_owners_survive_success_and_both_reservation_refusals() {
    let _lock=LOCK.lock().unwrap(); let sort=keys(1,0);
    for mode in 0..7 {
        let mut rows=vec![deep_row("b",1),deep_row("a",0)]; let before=owners(&rows);
        let outer=(rows.as_ptr(),rows.len(),rows.capacity());let mut cx=context(mode);
        query_order::test_reset();let watch=Watch::begin();
        let result=try_sort_rows(&mut rows,&sort,&mut cx);watch.pause();
        let refuse=(1..=4).contains(&mode);watch.empty(if refuse {(mode-1)/2} else {2});
        assert_eq!(owners(&rows),before);assert_eq!((rows.as_ptr(),rows.len(),rows.capacity()),outer);
        if refuse {assert_eq!(result,Err(QueryOrderError::Allocation(expected_failure(2))));assert_eq!(rows[0].0,"b");}
        else {assert_eq!(result,Ok(()));assert_eq!(rows[0].0,"a");}
        bound(2,1,1,!refuse);destroy_iteratively(rows);
    }
}
#[test]
fn context_prefix_sticky_history_and_fixed_error_format_are_preserved() {
    let _lock=LOCK.lock().unwrap();let input=vec![row("b",vec![],0),row("a",vec![],1)];
    for sticky in [false,true] {
        let mut cx=Cx::fail_nth_growth(NonZeroUsize::new(2).unwrap(),sticky);
        let mut prefix=String::new();product_alloc::try_reserve_string(&mut prefix,7,&mut cx,Site::String).unwrap();
        let mut rows=input.clone();let watch=Watch::begin();
        let result=try_sort_rows(&mut rows,&[],&mut cx);watch.empty(0);
        assert_eq!(result,Err(QueryOrderError::Allocation(expected_failure(2))));assert_eq!(cx.growth_requests(),2);
        assert_eq!(rows,input);
        // The sticky history is an earlier STRING failure, not the current
        // Vector refusal identity. A nonsticky history allows both workspaces.
        let mut prior=Cx::fail_nth_growth(NonZeroUsize::new(1).unwrap(),sticky);let mut s=String::new();
        let first=product_alloc::try_reserve_string(&mut s,13,&mut prior,Site::String).unwrap_err();
        let mut rows=input.clone();let watch=Watch::begin();
        let result=try_sort_rows(&mut rows,&[],&mut prior);watch.empty(if sticky {0} else {2});
        assert_eq!(prior.first_failure(),Some(first));assert_eq!(prior.growth_requests(),if sticky {2} else {3});
        if sticky {assert_eq!(result,Err(QueryOrderError::Allocation(expected_failure(2))));assert_eq!(rows,input);}
        else {assert_eq!(result,Ok(()));assert_eq!(rows[0].0,"a");}
    }
    struct Fixed { bytes:[u8;128], len:usize }
    impl Write for Fixed {fn write_str(&mut self,s:&str)->fmt::Result {
        if s.len()>self.bytes.len()-self.len{return Err(fmt::Error);}
        self.bytes[self.len..self.len+s.len()].copy_from_slice(s.as_bytes());self.len+=s.len();Ok(())
    }}
    let error=expected_failure(2);let mut out=Fixed{bytes:[0;128],len:0};let watch=Watch::begin();
    let written=write!(&mut out,"{error}");watch.empty(0);assert_eq!(written,Ok(()));assert!(out.len>0);
}
#[test]
fn synthetic_extent_overflow_uses_real_allocator_without_fake_slices() {
    let _lock=LOCK.lock().unwrap();
    for n in [(isize::MAX as usize/size_of::<usize>())+1,usize::MAX] {
        for sticky in [false,true] {
            let mut cx=Cx::fail_nth_growth(NonZeroUsize::new(1).unwrap(),sticky);
            query_order::test_reset();let watch=Watch::begin();
            let result=query_order::test_prepare(n,&mut cx);watch.empty(0);
            let expected=AllocationFailure{kind:Kind::CapacityOverflow,site:Site::Vector,
                requested_bytes:n.checked_mul(size_of::<usize>())};
            assert_eq!(result,Err(expected));assert_eq!(cx.first_failure(),Some(expected));assert_eq!(cx.growth_requests(),0);
            bound(0,0,0,false);
        }
    }
    println!("R07c fixed census: ID=916 row-total=7697 row-cycle=6; pairwise=3456; QueryRow_size={}",size_of::<QueryRow>());
}
