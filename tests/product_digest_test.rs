// Independent focused root: exactly the original domain, not the whole contract.
#[path = "../src/product_contract/identity_domain.rs"] mod identity_domain;
mod product_contract { pub use crate::identity_domain::IdentityDomain; }
#[path = "../src/product_alloc.rs"] mod product_alloc;
#[path = "../src/product_digest.rs"] mod product_digest;
#[path = "support/digest_native_oracle.rs"] mod oracle;
use product_alloc::{AllocationContext as Cx, AllocationFailure, AllocationFailureKind,
    AllocationSite};
use product_contract::IdentityDomain as D;
use product_digest::{checked_framed_len, try_bytes_digest, DigestError};
use std::{fmt::{self, Write}, num::NonZeroUsize};

const DOMAINS: [D;16] = [D::Source,D::Program,D::SemanticProgram,D::Data,D::Schema,
    D::Input,D::Session,D::Observation,D::Output,D::OutputBytes,D::Request,
    D::Scenario,D::Decision,D::Evolution,D::Adoption,D::Evidence];
// Exhaustive matches prevent a new production or original variant being skipped.
fn original(d:D)->oracle::IdentityDomain {
    use oracle::IdentityDomain as O;
    match d { D::Source=>O::Source,D::Program=>O::Program,D::SemanticProgram=>O::SemanticProgram,
        D::Data=>O::Data,D::Schema=>O::Schema,D::Input=>O::Input,D::Session=>O::Session,
        D::Observation=>O::Observation,D::Output=>O::Output,D::OutputBytes=>O::OutputBytes,
        D::Request=>O::Request,D::Scenario=>O::Scenario,D::Decision=>O::Decision,
        D::Evolution=>O::Evolution,D::Adoption=>O::Adoption,D::Evidence=>O::Evidence }
}
fn literal(d:oracle::IdentityDomain)->&'static [u8] {
    use oracle::IdentityDomain as O;
    match d { O::Source=>b"\"source\"",O::Program=>b"\"program\"",
        O::SemanticProgram=>b"\"semantic_program\"",O::Data=>b"\"data\"",
        O::Schema=>b"\"schema\"",O::Input=>b"\"input\"",O::Session=>b"\"session\"",
        O::Observation=>b"\"observation\"",O::Output=>b"\"output\"",
        O::OutputBytes=>b"\"output_bytes\"",O::Request=>b"\"request\"",
        O::Scenario=>b"\"scenario\"",O::Decision=>b"\"decision\"",
        O::Evolution=>b"\"evolution\"",O::Adoption=>b"\"adoption\"",O::Evidence=>b"\"evidence\"" }
}
fn compare(d:D,payload:&[u8]) {
    let old=oracle::digest_bytes(original(d),payload);
    // Literal frame independent of both the new matcher and old enum serializer.
    let mut frame=b"gitmanager.product.v1\0".to_vec();
    frame.extend_from_slice(literal(original(d))); frame.push(0); frame.extend_from_slice(payload);
    let direct=ring::digest::digest(&ring::digest::SHA256,&frame);
    let result=try_bytes_digest(d,payload).unwrap();
    assert_eq!(result.as_bytes().as_slice(),direct.as_ref());
    assert_eq!(result.lower_hex_bytes().as_slice(),old.as_str().as_bytes());
    let mut cx=Cx::new();
    assert_eq!(result.try_hex_string(&mut cx).unwrap(),old.as_str());
    assert_eq!(result.try_hex_vec(&mut cx).unwrap(),old.as_str().as_bytes());
    assert_eq!(cx.growth_requests(),2);
}
#[test]
fn all_domains_exact_frames_payloads_and_padding_boundaries() {
    let all=std::array::from_fn::<_,256,_>(|i|i as u8);
    let pattern=std::array::from_fn::<_,4096,_>(|i|(i%251) as u8);
    let mut cases=0; let mut bytes=0;
    for (index,d) in DOMAINS.into_iter().enumerate() {
        assert!(!DOMAINS[..index].contains(&d));
        assert_eq!(serde_json::to_vec(&d).unwrap(),literal(original(d)));
        assert_eq!(serde_json::to_vec(&original(d)).unwrap(),literal(original(d)));
        let h=23+literal(original(d)).len();
        for payload in [b"".as_slice(),b"\0",all.as_slice(),b"\xff\xc0\x80\xfe",
            "é中🚀".as_bytes(),b" \t\r",b"\n",br#"{"a":[1,true],"z":null}"#] {
            compare(d,payload);cases+=1;bytes+=payload.len();
        }
        for extra in [0,64,128] {
            for frame_len in [55,56,63,64,65] {
                let len=frame_len+extra-h;
                compare(d,&pattern[..len]); cases+=1; bytes+=len;
            }
        }
        for len in [55,56,63,64,65,127,128,129,4096] {
            compare(d,&pattern[..len]);cases+=1;bytes+=len;
        }
    }
    assert_eq!(cases,512); assert!(bytes<=512*1024);
}
#[test]
fn checked_length_accepts_exact_ring_limit_without_hashing() {
    let limit=u64::MAX/8;
    for d in DOMAINS {
        let h=(23+literal(original(d)).len()) as u64;
        for n in [0,limit-h-1,limit-h] { assert_eq!(checked_framed_len(d,n),Ok(n+h)); }
        for n in [limit-h+1,limit,u64::MAX-h,u64::MAX] {
            assert_eq!(checked_framed_len(d,n),Err(DigestError::InputTooLong));
        }
    }
}
fn fault(n:usize)->Cx { Cx::fail_nth_growth(NonZeroUsize::new(n).unwrap(),true) }
fn expected(site:AllocationSite)->AllocationFailure {
    AllocationFailure{kind:AllocationFailureKind::Injected,site,requested_bytes:Some(64)}
}
#[test]
fn each_renderer_one_request_exact_refusal_and_fresh_retry() {
    let source=String::from("borrowed\0é"); let before=source.clone();
    let value=try_bytes_digest(D::Evidence,source.as_bytes()).unwrap();
    let fixed=value.lower_hex_bytes();
    let mut string_cx=fault(1);let mut bytes_cx=fault(1);
    let string=value.try_hex_string(&mut string_cx);
    let bytes=value.try_hex_vec(&mut bytes_cx);
    assert_eq!(string,Err(expected(AllocationSite::String)));
    assert_eq!(bytes,Err(expected(AllocationSite::Bytes)));
    assert_eq!(string_cx.first_failure(),Some(expected(AllocationSite::String)));
    assert_eq!(bytes_cx.first_failure(),Some(expected(AllocationSite::Bytes)));
    assert_eq!(string_cx.growth_requests(),1);assert_eq!(bytes_cx.growth_requests(),1);
    let mut fresh=Cx::new();assert_eq!(value.try_hex_string(&mut fresh).unwrap().as_bytes(),fixed);
    assert_eq!(fresh.growth_requests(),1);
    let mut fresh=Cx::new();assert_eq!(value.try_hex_vec(&mut fresh).unwrap(),fixed);
    assert_eq!(fresh.growth_requests(),1);assert_eq!(source,before);
}
#[test]
fn sequential_rendering_preserves_first_completed_owner() {
    let value=try_bytes_digest(D::Source,b"\0\xff").unwrap();let fixed=value.lower_hex_bytes();
    let mut cx=fault(2);let first=value.try_hex_string(&mut cx).unwrap();
    let second=value.try_hex_vec(&mut cx);
    assert_eq!(second,Err(expected(AllocationSite::Bytes)));
    assert_eq!(first.as_bytes(),fixed);assert_eq!(cx.growth_requests(),2);
    assert_eq!(cx.first_failure(),Some(expected(AllocationSite::Bytes)));
    let mut cx=fault(2);let first=value.try_hex_vec(&mut cx).unwrap();
    let second=value.try_hex_string(&mut cx);
    assert_eq!(second,Err(expected(AllocationSite::String)));
    assert_eq!(first,fixed);assert_eq!(cx.growth_requests(),2);
    assert_eq!(cx.first_failure(),Some(expected(AllocationSite::String)));
}
#[test]
fn fixed_operations_leave_product_allocation_context_untouched() {
    let cx=fault(1);let payload=[0,255,10,32];let before=payload;
    let value=try_bytes_digest(D::Data,&payload).unwrap();let copy=value;let cloned=value.clone();
    assert_eq!(copy,value);assert_eq!(cloned.as_bytes(),value.as_bytes());
    assert_eq!(copy.lower_hex_bytes(),cloned.lower_hex_bytes());
    assert_eq!(checked_framed_len(D::Data,u64::MAX),Err(DigestError::InputTooLong));
    assert_eq!(cx.growth_requests(),0);assert_eq!(cx.first_failure(),None);assert_eq!(payload,before);
    // This context-only assertion does not establish absence of native allocations.
}
struct Fixed { bytes:[u8;128],len:usize }
impl Write for Fixed {
    fn write_str(&mut self,s:&str)->fmt::Result {
        let end=self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len=end;Ok(())
    }
}
#[test]
fn earlier_failure_survives_refused_render_and_fixed_diagnostic() {
    let value=try_bytes_digest(D::Output,b"unchanged").unwrap();let mut cx=fault(1);
    let first=value.try_hex_string(&mut cx).unwrap_err();
    let later=value.try_hex_vec(&mut cx);
    let mut sink=Fixed{bytes:[0;128],len:0};write!(&mut sink,"{first}").unwrap();
    assert_eq!(later,Err(expected(AllocationSite::Bytes)));
    assert_eq!(cx.first_failure(),Some(first));assert_eq!(cx.growth_requests(),2);
    assert_eq!(&sink.bytes[..sink.len],b"allocation failure (injected) at string: 64 bytes");
}
