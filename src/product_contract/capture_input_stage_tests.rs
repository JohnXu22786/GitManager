// Test-only descendant: proposed paid parser APIs are absent in the baseline.
use super::*;
#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;
#[path = "../../tests/support/capture_native_oracle/tool_proposal_input.rs"]
mod old;
#[derive(Clone,Debug,PartialEq,Eq)]
enum Stop { Fuel, Cancelled }

#[test]
fn paid_native_intake_keeps_all_actual_old_error_prefixes() {
    for (name,input) in fixture::malformed_sources(MAX_INPUT_BYTES) {
        let expected=old::parse_json_bytes(&input).map_err(|e|e.to_string());
        let mut work=|_:u64|Ok::<(),Stop>(());
        let actual=parse_json_bytes_with_work(&input,&mut work).map_err(|e|match e {
            InputWorkError::Input(e)=>e.to_string(),
            _=>panic!("unexpected resource refusal"),
        });
        assert_eq!(actual,expected,"{name}");
    }
}

#[test]
fn native_lexical_shape_literals_include_escapes_and_malformed_suffixes() {
    // (N,A,K,S,X,M,E): hand-counted raw shapes, independent of the paid scanner.
    let cases:&[(&[u8],(usize,usize,usize,usize,usize,usize,usize))]=&[
        (b"{}",(2,1,0,0,0,0,0)),
        (br#"{"x":"a"}"#,(9,1,2,2,0,0,0)),
        (br#"{"x":"a\nb"}"#,(12,1,2,5,4,4,1)),
        (br#"{"\u0078":0}"#,(12,1,1,6,6,6,1)),
        (br#"["\uD83D\uDE00"]"#,(16,1,1,12,12,12,2)),
        (br#"{"a":[]}"#,(8,2,1,1,0,0,0)),
        (br#"{"x":0,"\u0078":1}"#,(18,1,2,7,6,6,1)),
        (b"\"\\",(2,0,1,1,1,1,1)),
        (br#"{"x":?,"later":"a\nb"}"#,(22,1,3,10,4,4,1)),
    ];
    for (input,expected) in cases {
        let mut calls=vec![];
        let shape=lexical_envelope_with_work(input,&mut |n|{calls.push(n);Ok::<(),Stop>(())}).unwrap();
        assert_eq!((shape.raw_len,shape.container_depth,shape.spans,shape.string_bytes,shape.escaped_bytes,shape.max_escaped,shape.escapes),*expected);
        assert_eq!(calls.iter().sum::<u64>(),19+18*input.len() as u64);
    }
}

#[cfg(target_pointer_width="64")]
#[test]
fn native_refusal_boundary_and_error_before_post_poll_are_literal() {
    // '?' scalar: lexical prefix39; descriptor352; native reserve2974 (EB48 plus inline write and prepaid EC64=2646); no Value callback.
    for limit in [3364u64,3365,3366] {
        let mut left=limit;let mut calls=vec![];
        let result=parse_json_bytes_with_work(b"?",&mut |n| {
            calls.push(n);
            if n>left {Err(Stop::Fuel)}else{left-=n;Ok(())}
        });
        assert_eq!(calls,vec![0,2,10,18,9,352,2974]);
        match result {
            Err(InputWorkError::Resource(Stop::Fuel))=>assert_eq!(limit,3364),
            Err(InputWorkError::Input(error))=>{
                assert!(limit>=3365);
                assert_eq!(error.to_string(),old::parse_json_bytes(b"?").unwrap_err().to_string());
            },
            _=>panic!("wrong pre-native/native refusal family"),
        }
    }
    let mut calls=0;
    let result=parse_json_bytes_with_work(b"?",&mut |_| {
        calls+=1;if calls==8 {Err(Stop::Cancelled)}else{Ok(())}
    });
    assert!(matches!(result,Err(InputWorkError::Input(_))));
    assert_eq!(calls,7,"native Err must return before a new post-call poll");
}
