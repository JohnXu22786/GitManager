//! Native parity and independent reserve/payment boundaries for typed intake.
use super::*;
use super::super::capture_work::WorkState;
use serde_json::json;
use std::mem::align_of;

#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;

struct Work {
    calls: Vec<u64>,
    left: u64,
    zeroes: usize,
    cancel_zero: Option<usize>,
}
impl Work {
    fn new(left: u64) -> Self {
        Self { calls: Vec::new(), left, zeroes: 0, cancel_zero: None }
    }
}
impl ContractWork for Work {
    fn charge(&mut self, units: u64) -> std::result::Result<(), WorkFailure> {
        self.calls.push(units);
        if units == 0 {
            self.zeroes += 1;
            if self.cancel_zero == Some(self.zeroes) {
                return Err(WorkFailure::Cancelled);
            }
        }
        self.left = self.left.checked_sub(units)
            .ok_or(WorkFailure::BudgetExhausted("typed test fuel"))?;
        Ok(())
    }
}
fn paid(bytes: &[u8], work: &mut Work) -> ContractWorkResult<AppDefinition> {
    let state = WorkState::new(work);
    state.finish(parse_app(bytes, Audit(Some(&state))))
}
fn parity(value: JsonValue, valid: bool) {
    // The expected result uses the independent, unchanged owned native entry.
    let expected = serde_json::from_value::<AppDefinition>(value.clone())
        .map_err(|error| ContractWorkError::Contract(ContractError(error.to_string())));
    assert_eq!(expected.is_ok(), valid, "{value}");
    let mut work = Work::new(u64::MAX);
    assert_eq!(paid(&serde_json::to_vec(&value).unwrap(), &mut work), expected);
    assert_eq!(work.zeroes, if valid { 3 } else { 2 });
}

#[test]
fn typed_bounds_preserve_native_values_errors_and_partial_prefixes() {
    let mut object = fixture::minimal();
    object["actions"][0]["parameters"] = json!({
        "a":{"kind":"list","item":{"kind":"optional","item":{"kind":"text"}}},
        "z":{"kind":"boolean"}
    });
    object["actions"][0]["steps"] = json!([
        {"kind":"create","entity":"row","values":{
            "a":{"kind":"not","value":{"kind":"and","values":[
                {"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}
            ]}},"z":{"kind":"today"}},"bind":"new"},
        {"kind":"assert","condition":{"kind":"today"},"message":"tail"}
    ]);
    object["views"][0]["kind"] = json!({"kind":"form","action":"check","fields":[],"defaults":{
        "a":{"kind":"list","item_type":{"kind":"text"},"items":[{"kind":"text","value":"first"},{"kind":"text","value":"tail"}]},
        "z":{"kind":"boolean","value":true}
    }});
    object["views"][0]["keys"] = json!([
        {"key":"x","modifiers":["control","alt","control","shift","command","alt"],"binding":"run"}
    ]);
    parity(object.clone(), true);

    // Ordinary structs and nested internally tagged families both accept sequences.
    let mut sequence = object.clone();
    sequence["actions"][0] = json!(["check","Check",{
        "a":["list",["optional",["text"]]],"z":["boolean"]
    },[],[["assert",["not",["literal",["boolean"],["boolean",false]]],"m"]],[]]);
    sequence["views"][0]["kind"] = json!(["form","check",[],{
        "a":["list",["text"],[["text","first"],["text","tail"]]],"z":["boolean",true]
    }]);
    let root = json!([
        sequence["version"],sequence["id"],sequence["label"],sequence["entities"],
        sequence["state"],sequence["actions"],sequence["outputs"],sequence["observables"],
        sequence["views"],sequence["initial_view"]
    ]);
    parity(root, true);

    // Each map destination has a valid first entry and an invalid later entry;
    // successful prefixes, the failing child and unused tails retain their owners.
    for (pointer, replacement) in [
        ("/actions/0/parameters/z", json!({"kind":"missing"})),
        ("/actions/0/steps/0/values/z", json!({"kind":"literal","value_type":{"kind":"text"},"value":{"kind":"text","value":7}})),
        ("/views/0/kind/defaults/z", json!({"kind":"boolean","value":"wrong"})),
        ("/views/0/kind/defaults/a/items/1", json!({"kind":"text","value":false})),
        ("/views/0/keys/0/modifiers", json!(["control","alt","missing","shift"])),
        ("/actions/0/steps/0/values/a", json!({"kind":"not","value":{"kind":"and","values":[{"kind":"today"},{"kind":"missing"},{"kind":"today"}]}})),
        ("/observables/0/value", json!({})),
        ("/observables/0/value", json!({"kind":1})),
        ("/observables/0/value", json!({"kind":"missing"})),
        ("/observables/0/value", json!({"kind":"not","value":{"kind":"today"},"unexpected":0})),
        ("/observables/0/value", json!(["not",["literal",["boolean"],["boolean","wrong"]]])),
        ("/observables/0/value", json!([])),
    ] {
        let mut malformed = object.clone();
        *malformed.pointer_mut(pointer).unwrap() = replacement;
        parity(malformed, false);
    }
    for bytes in [br#"{"version":1,"version":2}"#.as_slice(), br#"{"version":1,"actions":["#, b"{} false"] {
        let expected = bounded_input::parse_json_bytes(bytes).unwrap_err().to_string();
        let mut work = Work::new(u64::MAX);
        assert_eq!(paid(bytes, &mut work), Err(ContractWorkError::Contract(ContractError(expected))));
        assert_eq!(work.zeroes, 1); // Failed parsing cannot reach conversion or its poll.
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_bounds_match_literal_native_and_descriptor_controls() {
    // These layout constants do not call any reserve helper. The numeric case
    // coefficients below were derived from the reviewed V/U/BTree formulas.
    assert_eq!((size_of::<usize>(), size_of::<JsonValue>(), size_of::<Id>(), size_of::<KeyModifier>()), (8,32,24,1));
    assert_eq!((align_of::<Type>(), align_of::<Expr>(), align_of::<DataValue>()), (8,8,8));
    let h = size_of::<Expr>().max(size_of::<Type>()) as u64;
    let t = [size_of::<Expr>(),size_of::<Statement>(),size_of::<DataValue>(),size_of::<EntityDefinition>(),size_of::<ViewDefinition>(),size_of::<ActionDefinition>()]
        .into_iter().max().unwrap() as u64;
    let p = 24 + [size_of::<Type>(),size_of::<Expr>(),size_of::<DataValue>()]
        .into_iter().max().unwrap() as u64;
    assert!((16..=1024).contains(&t));
    for (value, depth, expected, fuel, bytes) in [
        (json!({}), 0, 3121 + 2*h, 384, 0),
        (json!({}), 1, 3133 + 2*h, 384, 0),
        (json!({}), 2, 3158 + 2*h, 384, 0),
        (json!([]), 0, 1590 + 2*h + 12*t, 721, 0),
        (json!([]), 1, 1602 + 2*h + 12*t, 721, 0),
        (json!([]), 2, 1627 + 2*h + 12*t, 721, 0),
        (json!([null]), 0, 1754 + 4*h + 13*t, 1105, 0),
        (json!([null,null,null,null,null]), 0, 4714 + 12*h + 35*t, 2641, 0),
        (json!([null,null,null,null,null,null,null,null,null]), 3, 8927 + 20*h + 63*t, 4177, 0),
        (json!({"a":null}), 0, 3917 + 4*h + 12*p, 1563, 1),
        (json!({"a":null,"b":null}), 3, 4883 + 6*h + 14*p, 2742, 2),
    ] {
        let mut descriptor = Conversion { bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        state.finish(descriptor.value(&value, depth, Audit(Some(&state)))).unwrap();
        assert_eq!((descriptor.bytes, descriptor.native), (bytes, expected), "{value} at {depth}");
        assert_eq!(work.left, 0);
    }
    // One below each new operation's literal prepayment: V fixed split, U,
    // array maximum/final addition, checked modifier sum and map maximum.
    for (value, fuel, calls, requested, left) in [
        (json!([]), 352, 7, 8, 7),
        (json!([]), 368, 8, 16, 15),
        (json!([]), 717, 13, 2, 1),
        (json!([]), 720, 14, 3, 2),
        (json!([null]), 1081, 14, 3, 2),
        (json!({"a":null}), 1541, 19, 12, 11),
    ] {
        let mut descriptor = Conversion { bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(descriptor.value(&value, 0, Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!((work.calls.len(), work.calls.last().copied(), work.left), (calls, Some(requested), left));
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_bounds_keep_native_refusal_and_success_only_poll_order() {
    assert_eq!((size_of::<usize>(),size_of::<JsonValue>(),size_of::<Box<str>>(),size_of::<std::io::Error>()), (8,32,16,8));
    let h = size_of::<Expr>().max(size_of::<Type>()) as u64;
    let reserve = 3121 + 2*h; // Empty object, including both Content envelopes.
    let prefix = [
        320,1561,8,218, // Existing FMT(107), retained string owner.
        0,2,10,18,18,9,352,3197,2,9,1,0, // Paid parser for {}.
        4168,12,3,3,8,8,320,8,16,3,3, // Descriptor.
        320,41082,320,14426,8,2052,5,101, // D=1024 and both error owners.
    ];
    assert_eq!(prefix.iter().sum::<u64>(), 68591);
    let mut refused = Work::new(68591 + reserve - 1);
    assert_eq!(paid(b"{}", &mut refused),
        Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
    assert_eq!(&refused.calls[..prefix.len()], &prefix);
    assert_eq!(&refused.calls[prefix.len()..], &[reserve]);
    assert_eq!(refused.left, reserve - 1);

    let mut exact = Work::new(68591 + reserve);
    let expected = serde_json::from_value::<AppDefinition>(json!({})).unwrap_err().to_string();
    assert_eq!(paid(b"{}", &mut exact), Err(ContractWorkError::Contract(ContractError(expected))));
    assert_eq!(exact.calls, refused.calls); // Native Err has no completion poll.
    assert_eq!(exact.left, 0);

    let good = br#"[1,"","",[],[],[],[],[],[],""]"#;
    let mut cancelled = Work::new(u64::MAX);
    cancelled.cancel_zero = Some(3); // Parser entry, parser completion, typed success.
    assert_eq!(paid(good, &mut cancelled), Err(ContractWorkError::Resource(WorkFailure::Cancelled)));
    assert_eq!(cancelled.zeroes, 3);
    let t = [size_of::<Expr>(),size_of::<Statement>(),size_of::<DataValue>(),size_of::<EntityDefinition>(),size_of::<ViewDefinition>(),size_of::<ActionDefinition>()]
        .into_iter().max().unwrap() as u64;
    assert!(cancelled.calls.ends_with(&[19055 + 22*h + 142*t, 0]));
    let mut success = Work::new(u64::MAX);
    assert_eq!(paid(good, &mut success).unwrap(), serde_json::from_slice::<AppDefinition>(good).unwrap());
    assert_eq!(success.calls, cancelled.calls);
}
