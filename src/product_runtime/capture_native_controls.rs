//! Direct native-interface parity/payment controls. No default-row fit credit.
use super::*;
use crate::product_contract::{ContractWork, WorkFailure};
use std::time::Duration;

struct NativeAccount {
    left: u64,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
}
impl NativeAccount {
    fn new(left:u64)->Self {
        Self{left,deadline:Instant::now()+Duration::from_secs(60),cancelled:Arc::new(AtomicBool::new(false))}
    }
}
impl ContractWork for NativeAccount {
    fn charge(&mut self,units:u64)->std::result::Result<(),WorkFailure> {
        if self.cancelled.load(Ordering::Relaxed) {return Err(WorkFailure::Cancelled)}
        if Instant::now()>=self.deadline {return Err(WorkFailure::BudgetExhausted("elapsed time limit"))}
        self.left=self.left.checked_sub(units)
            .ok_or(WorkFailure::BudgetExhausted("evaluation fuel exhausted"))?;
        Ok(())
    }
}

// Independent literal ledger, never production descriptor/cost code.
fn lower_allowance(s:&str)->u64 {
    let b=s.len() as u64;
    let chars:Vec<char>=s.chars().collect();
    let m=chars.len() as u64;
    let p=s.bytes().take_while(u8::is_ascii).count() as u64;
    let positions:Vec<usize>=chars.iter().enumerate().filter_map(|(i,c)|(*c=='Σ').then_some(i)).collect();
    let k=positions.len() as u64;
    let d=chars.iter().filter(|c|**c=='İ').count() as u64;
    let e=chars.iter().filter(|c|matches!(**c,'İ'|'Ⱥ'|'Ⱦ')).count() as u64;
    let (cb,cc)=if let (Some(first),Some(last))=(positions.first(),positions.last()) {
        let b0:usize=chars[..*first].iter().map(|c|c.len_utf8()).sum();
        let bk:usize=chars[last+1..].iter().map(|c|c.len_utf8()).sum();
        (2*b-b0 as u64-bk as u64-4,2*m-*first as u64-(chars.len()-last-1) as u64-2)
    }else{(0,0)};
    let q=p/16;let r=p%16;
    let f=u64::from(b-16*q>=16&&p<b);let z=u64::from(p<b);
    let g=u64::from(e>0);let c=if g==0{b}else{8.max(2*b)};
    let descriptor=16+23*(m+1);
    let prefix=20+16*q+16*f+r+z+2*p+32*(q+f)+16;
    let decode=b-p+4*(m-p);
    let mapping=58*(m-p-k);
    let sigma=8*k+cb+cc*(4+1047)+(2*k).min(cc)*83;
    let output=8*(m-p+d)+(b+e-p);
    let storage=b+g*c+g*b+6+4*g+16+12;
    descriptor+prefix+decode+mapping+sigma+output+storage+192+12*std::mem::size_of::<usize>() as u64
}

#[test]
fn capture_native_lower_full_unicode_and_exact_prepaid_boundary() {
    let mut cases=vec![String::new(),"ASCII\0\tZ".into(),"Σ".into(),"AΣ".into(),"ΣΣ".into(),
        "AΣ\u{0301}A".into(),"AΣ'Σ.AΣ".into(),"İȺȾK𐐀😀".into(),"A\u{0301}Σ\u{0301}".into(),
        "İ".repeat(2048)];
    for n in [0,1,15,16,17,31,32] {cases.push(format!("{}Σ","A".repeat(n)));}
    for input in cases {
        let original=input.clone();
        let expected=input.to_lowercase(); // Unchanged full-string native oracle.
        let allowance=lower_allowance(&input);
        let mut exact=NativeAccount::new(allowance);
        let got=capture_native::lower_paid(&input,&mut exact).unwrap();
        assert_eq!(got,expected);
        assert_eq!(exact.left,0);
        assert_eq!(input,original);
        let mut short=NativeAccount::new(allowance-1);
        assert!(matches!(capture_native::lower_paid(&input,&mut short),Err(AdapterError::BudgetExhausted(_))));
        assert_eq!(input,original);
        let mut extra=NativeAccount::new(allowance+1);
        assert_eq!(capture_native::lower_paid(&input,&mut extra).unwrap(),expected);
        assert_eq!(extra.left,1);
    }
    assert_eq!("İ".repeat(2048).to_lowercase().len(),6144);
}

#[test]
fn capture_native_contains_matches_exact_utf8_substring_and_refuses_before_work() {
    for (a,b) in [("",""),("a",""),("","a"),("aaaaab","aaab"),("aβγβ","γβ"),
        ("a\0b\nc","\0b"),("Σσς","σ"),("long","longer"),("abc","x")] {
        let mut paid=NativeAccount::new(1_000_000);
        assert_eq!(capture_native::text_contains_paid(a,b,&mut paid).unwrap(),a.contains(b));
        assert!(paid.left<1_000_000);
        let mut zero=NativeAccount::new(0);
        assert!(matches!(capture_native::text_contains_paid(a,b,&mut zero),Err(AdapterError::BudgetExhausted(_))));
    }
}

fn heights(n:usize)->(u64,u64) {
    if n<2{return(0,0)}
    let l=(usize::BITS-1-n.leading_zeros()) as u64;
    (l+u64::from(!n.is_power_of_two()),l)
}
fn sort_allowance(n:usize,size:usize,payload:u64,descriptor:u64)->u64 {
    let (h,l)=heights(n);let n=n as u64;let size=size as u64;
    let (cn,mn)=if n<2{(0,0)}else if n<=20{
        let c=n*(n-1)/2;(c,c+2*(n-1))
    }else{(n*(2*h+8*l+40),n*(4*h+12*l+48))};
    let c=cn.max(n*h);let moves=mn.max(3*n*h);
    let control=32*(c+moves+n+1);
    let storage=if n<2 {0}else {
        let a=((n+1)/2).max(n.min(8_000_000/size)).max(48).max(n/2);
        let heap=if a*size>4096{a*size}else{0};
        let mut x=n;let mut pivot=1;while x>1{x=(x+7)/8;pivot+=1;}
        4096+heap+2*(66*std::mem::size_of::<usize>() as u64+66)
            +(2*l+2)*(16*std::mem::size_of::<usize>() as u64+size)
            +pivot*8*std::mem::size_of::<usize>() as u64
            +4*size+32*std::mem::size_of::<usize>() as u64
    };
    // Formula arithmetic is a fixed128 operation pre-reserve, separate from descriptor.
    descriptor+128+c*payload+moves*size+control+storage+12
}
fn record(id:String,ordinal:usize)->Record {
    Record{entity:"person".into(),id,revision:ordinal as u64+1,
        created_program:Digest::try_from("0".repeat(64)).unwrap(),archived:false,values:Values::new()}
}

#[test]
fn capture_native_candidate_sort_preserves_native_stability_and_refusal_ownership() {
    for n in [0,1,2,19,20,21,31,32,47,48,63,64,65,512,4096,4097,10_000] {
        for shape in 0..4 {
            let records:Vec<_>=(0..n).map(|i| {
                let key=match shape{0=>i,1=>n-i,2=>i%3,_=>(i*37)%n.max(1)};
                record(format!("shared_prefix_{key:05}"),i)
            }).collect();
            let original:Vec<_>=records.iter().collect();
            let mut oracle=original.clone();
            oracle.sort_by(|a,b|a.id.cmp(&b.id));
            let id_width=records.iter().map(|r|r.id.len()).max().unwrap_or(0) as u64;
            let allowance=sort_allowance(n,std::mem::size_of::<&Record>(),3+2*id_width,4+6*n as u64);
            let mut actual=original.clone();let mut exact=NativeAccount::new(allowance);
            capture_native::sort_candidates_paid(&mut actual,&mut exact).unwrap();
            assert_eq!(actual,oracle);
            assert_eq!(exact.left,0);
            let mut refused=original.clone();let mut short=NativeAccount::new(allowance-1);
            assert!(matches!(capture_native::sort_candidates_paid(&mut refused,&mut short),Err(AdapterError::BudgetExhausted(_))));
            assert_eq!(refused,original); // Pre-reserve must precede in-place native mutation.
        }
    }
}

#[test]
fn capture_native_final_sort_matches_original_all_scalar_families_and_ties() {
    for descending in [false,true] {
        for family in 0..5 {
            let mut rows:Vec<(Id,Vec<DataValue>,DataValue)>=(0..21).rev().map(|i| {
                let key=match family{
                    0=>DataValue::Boolean{value:i%2==0},1=>DataValue::Integer{value:i%3},
                    2=>DataValue::Date{days:20000+(i%3) as i32},
                    3=>DataValue::Text{value:format!("β{}",i%3)},
                    _=>DataValue::Reference{entity:"person".into(),record:format!("r{}",i%3)}};
                (format!("r{i:03}"),vec![key],DataValue::Integer{value:i})
            }).collect();
            let sort=vec![SortKey{value:Expr::Today,descending}]; // Only descending is consumed by this native seam.
            let mut expected=rows.clone();
            expected.sort_by(|a,b| {
                let order=eval::compare(&a.1[0],&b.1[0]);
                if order!=std::cmp::Ordering::Equal {if descending{order.reverse()}else{order}}
                else{a.0.cmp(&b.0)}
            });
            let mut paid=NativeAccount::new(100_000_000);
            capture_native::sort_rows_paid(&mut rows,&sort,&mut paid).unwrap();
            assert_eq!(rows,expected);
            assert!(paid.left<100_000_000);
        }
    }
}

#[test]
fn capture_native_counting_sink_matches_compact_serde_grammar_without_wire_buffer() {
    let optional=Type::Optional{item:Box::new(Type::Boolean)};
    let cases=vec![DataValue::Null,DataValue::Boolean{value:true},DataValue::Boolean{value:false},
        DataValue::Integer{value:i64::MIN},DataValue::Integer{value:i64::MAX},DataValue::Date{days:-719162},
        DataValue::Text{value:"\0\u{1}\u{8}\t\n\u{c}\r\"\\/β😀".into()},
        DataValue::Reference{entity:"person".into(),record:"r0".into()},
        DataValue::List{item_type:optional,items:vec![DataValue::Null,DataValue::Boolean{value:false}]},
        DataValue::List{item_type:Type::list(Type::Text),items:vec![DataValue::List{item_type:Type::Text,items:vec![]}]}];
    for value in cases {
        let native=serde_json::to_vec(&value).unwrap();
        let mut paid=NativeAccount::new(1_000_000);
        assert_eq!(capture_native::count_output_paid(&value,&mut paid).unwrap(),native.len());
        assert!(paid.left<1_000_000);
        let mut zero=NativeAccount::new(0);
        assert!(matches!(capture_native::count_output_paid(&value,&mut zero),Err(AdapterError::BudgetExhausted(_))));
    }
    let mut exact=NativeAccount::new(6);
    assert_eq!(capture_native::count_output_paid(&DataValue::Null,&mut exact).unwrap(),15);
    assert_eq!(exact.left,0); // Traversal6, emitted15 is a SEPARATE row debit.
    let mut short=NativeAccount::new(5);
    assert!(matches!(capture_native::count_output_paid(&DataValue::Null,&mut short),Err(AdapterError::BudgetExhausted(_))));
}
