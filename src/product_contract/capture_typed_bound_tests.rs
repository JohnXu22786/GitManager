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
// Test-only root adapter mirrors parse_app's paid root Boolean edge. The
// production descriptor has one recursive method and no forwarding frame.
fn root_value(descriptor: &mut Conversion, value: &JsonValue, depth: usize, a: Audit<'_>) -> Result<()> {
    a.pay(6)?;
    descriptor.value(value, depth, false, a)
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
    // All five eligible names may also be arbitrary map keys. These collisions
    // overreserve locally; they neither validate fields nor suppress descendants.
    for key in ["parameters", "values", "arguments", "columns", "defaults", "ordinary"] {
        object["actions"][0]["parameters"][key] = json!({"kind":"text"});
        object["actions"][0]["steps"][0]["values"][key] = json!({"kind":"today"});
        object["views"][0]["kind"]["defaults"][key] = json!({"kind":"boolean","value":false});
    }
    object["actions"][0]["steps"].as_array_mut().unwrap().extend([
        json!({"kind":"emit","output":"out","items":{"kind":"today"},"binding":"item","columns":{
            "a":{"kind":"today"},"columns":{"kind":"today"},"z":{"kind":"today"}
        }}),
        json!({"kind":"update","record":{"kind":"today"},"values":{
            "a":{"kind":"today"},"values":{"kind":"today"},"z":{"kind":"today"}
        }}),
    ]);
    object["views"][0]["actions"] = json!([{
        "id":"run","label":"Run","placement":"toolbar","action":"check",
        "arguments":{"a":{"kind":"today"},"arguments":{"kind":"today"},"z":{"kind":"today"}},
        "enabled":{"kind":"today"}
    }]);
    parity(object.clone(), true);
    let mut unknown = object.clone();
    unknown["parameters"] = json!({"a":{"kind":"text"}});
    parity(unknown, false); // An eligible spelling cannot accept an unknown field.

    // Ordinary structs and nested internally tagged families both accept sequences.
    let mut sequence = object.clone();
    sequence["actions"][0] = json!(["check","Check",{
        "a":["list",["optional",["text"]]],"z":["boolean"]
    },[],[
        ["assert",["not",["literal",["boolean"],["boolean",false]]],"m"],
        ["create","row",{"a":["today"],"z":["today"]},"new"],
        ["update",["today"],{"a":["today"],"z":["today"]}],
        ["emit","out",["today"],"item",{"a":["today"],"z":["today"]}]
    ],[]]);
    sequence["views"][0]["actions"][0] = json!([
        "run","Run","toolbar","check",{"a":["today"],"z":["today"]},["today"]
    ]);
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
        ("/actions/0/steps/2/columns/z", json!({"kind":"missing"})),
        ("/actions/0/steps/3/values/z", json!({"kind":"missing"})),
        ("/views/0/actions/0/arguments/z", json!({"kind":"missing"})),
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
        ("/observables/0/value", json!(["not"])),
        ("/observables/0/value", json!(["not",["today"],false])),
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
    assert!((120..=1024).contains(&t));
    for (value, depth, expected, fuel, bytes) in [
        (json!({}), 0, 49 + 2*h, 406, 0),
        (json!({}), 1, 61 + 2*h, 406, 0),
        (json!({}), 2, 86 + 2*h, 406, 0),
        (json!([]), 0, 54 + 2*h, 759, 0),
        (json!([]), 1, 66 + 2*h, 759, 0),
        (json!([]), 2, 91 + 2*h, 759, 0),
        (json!([null]), 0, 602 + 4*h + 4*t, 1149, 0),
        (json!([null,null,null,null,null]), 0, 2794 + 12*h + 20*t, 2709, 0),
        (json!([null,null,null,null,null,null,null,null,null]), 3, 5471 + 20*h + 36*t, 4269, 0),
        (json!({"a":null}), 0, 1410 + 4*h, 553, 1),
        (json!({"a":null,"b":null}), 3, 3007 + 6*h, 700, 2),
        // Preserve the old three-map maximum at an independently reached array edge.
        (json!([{"a":null}]), 0, 2227 + 6*h + 12*p + 4*t, 2779, 1),
    ] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        state.finish(root_value(&mut descriptor, &value, depth, Audit(Some(&state)))).unwrap();
        assert_eq!((descriptor.bytes, descriptor.native), (bytes, expected), "{value} at {depth}");
        assert_eq!(work.left, 0);
    }
    // Empty native Vec storage is now zero, but all F(0)=13 controls survive.
    // The selected layout lower bound also keeps the final Vec above the set
    // alternative in these finite n<=10 rows; no narrower Vec layout is selected.
    // The added scalar is paid separately from the old descriptor/frame reserve.
    assert_eq!((size_of::<Conversion>(), align_of::<Conversion>()), (24, 8));
    // Independent literal string controls retain the aggregate sum and native
    // allowance while the maximum grows once and never shrinks for shorter inputs.
    let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
    let mut work = Work::new(77);
    let state = WorkState::new(&mut work);
    for text in ["abc", "é", ""] {
        state.finish(descriptor.string(text, Audit(Some(&state)))).unwrap();
    }
    assert_eq!((descriptor.bytes, descriptor.max_bytes, descriptor.native), (5, 3, 534));
    assert_eq!(work.calls, [6,5,13,3,6,5,12,3,6,5,10,3]);
    assert_eq!(work.left, 0);
    // Refusing the new maximum prepayment leaves every field unchanged. With
    // exactly six units it can update the max, then the old scaled debit fails.
    for (fuel, maximum, calls) in [(5, 5, vec![6]), (6, 8, vec![6,5])] {
        let mut descriptor = Conversion { bytes: 11, max_bytes: 5, native: 17 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(descriptor.string("abcdefgh", Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!((descriptor.bytes, descriptor.max_bytes, descriptor.native), (11, maximum, 17));
        assert_eq!(work.calls, calls);
        assert_eq!(work.left, if fuel == 5 { 5 } else { 0 });
    }
    // One below each new operation's literal prepayment: V fixed split, U,
    // array maximum/final addition, checked modifier sum and map maximum.
    for (value, fuel, calls, requested, left) in [
        (json!([]), 374, 9, 8, 7),
        (json!([]), 390, 10, 16, 15),
        (json!([]), 755, 16, 2, 1),
        (json!([]), 758, 17, 3, 2),
        (json!([null]), 1119, 17, 3, 2),
        (json!([{"a":null}]), 2691, 40, 12, 11),
    ] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(root_value(&mut descriptor, &value, 0, Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!((work.calls.len(), work.calls.last().copied(), work.left), (calls, Some(requested), left));
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_bounds_keep_native_refusal_and_success_only_poll_order() {
    assert_eq!((size_of::<usize>(),size_of::<JsonValue>(),size_of::<Box<str>>(),size_of::<std::io::Error>()), (8,32,16,8));
    let h = size_of::<Expr>().max(size_of::<Type>()) as u64;
    let reserve = 49 + 2*h; // Empty object, including both Content envelopes.
    let prefix = [
        320,1561,8,218, // Existing FMT(107), retained string owner.
        0,2,10,18,18,9,352,3197,2,9,1,0, // Paid parser for {}.
        4185,6,12,3,3,8,8,320,16,8,16,3,3, // Descriptor plus new scalar owner/init.
        320,41082,320,14426,8,2052,5,101, // D=1024 and both error owners.
    ];
    assert_eq!(prefix.iter().sum::<u64>(), 68630);
    // Setup, root edge, exact-hint selection and retained D arithmetic must be
    // funded before their operations; no later callback is reached on refusal.
    for (fuel, count, left) in [(9909, 17, 4184), (9915, 18, 5), (10285, 25, 15), (10635, 30, 319)] {
        let mut refused = Work::new(fuel);
        assert_eq!(paid(b"{}", &mut refused),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!(refused.calls, prefix[..count]);
        assert_eq!(refused.left, left);
        assert_eq!(refused.zeroes, 2);
    }
    let mut refused = Work::new(68630 + reserve - 1);
    assert_eq!(paid(b"{}", &mut refused),
        Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
    assert_eq!(&refused.calls[..prefix.len()], &prefix);
    assert_eq!(&refused.calls[prefix.len()..], &[reserve]);
    assert_eq!(refused.left, reserve - 1);

    let mut exact = Work::new(68630 + reserve);
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
    assert!(cancelled.calls.ends_with(&[5999 + 22*h + 40*t, 0]));
    let mut success = Work::new(u64::MAX);
    assert_eq!(paid(good, &mut success).unwrap(), serde_json::from_slice::<AppDefinition>(good).unwrap());
    assert_eq!(success.calls, cancelled.calls);
}


#[test]
fn typed_error_bound_describes_complete_keys_and_values() {
    let mut key_only = serde_json::Map::new();
    key_only.insert("k".repeat(257), JsonValue::Null);
    let mut nested_key = serde_json::Map::new();
    nested_key.insert("k".repeat(97), json!("v".repeat(64)));
    let mut nested_value = serde_json::Map::new();
    nested_value.insert("k".repeat(80), json!("v".repeat(96)));
    let mut short_strings = vec![json!("x"); 128];
    short_strings.push(json!("q".repeat(32)));
    for (label, value, bytes, maximum) in [
        ("no strings", json!([null, false, 7]), 0, 0),
        ("keys without string leaves", JsonValue::Object(key_only), 257, 257),
        ("UTF-8 bytes, not characters", json!(["a", "bb", "é", "🙂"]), 9, 4),
        // Native App decoding would fail at the first unknown key `a`; the
        // descriptor must still include both keys and values in the later tail.
        ("long nested key after failed prefix", json!({"a":null,"z":[nested_key]}), 163, 97),
        ("long nested value after failed prefix", json!({"a":false,"z":["short",nested_value,"tail"]}), 187, 96),
        ("many short unrelated strings", JsonValue::Array(short_strings), 160, 32),
        ("same aggregate, one long string", json!(["x".repeat(160)]), 160, 160),
    ] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(u64::MAX);
        let state = WorkState::new(&mut work);
        state.finish(root_value(&mut descriptor, &value, 0, Audit(Some(&state)))).unwrap();
        assert_eq!((descriptor.bytes, descriptor.max_bytes), (bytes, maximum), "{label}");
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_error_bound_preserves_exact_native_errors() {
    // These messages name the intended originating error, independently of the
    // paid path. A different native error cannot make a negative fixture pass.
    let app_fields = "one of `version`, `id`, `label`, `entities`, `state`, `actions`, `outputs`, `observables`, `views`, `initial_view`";
    let expr_variants = "one of `literal`, `variable`, `field`, `state`, `today`, `query`, `filter`, `map`, `count`, `any`, `all`, `sum`, `contains`, `equal`, `less`, `add`, `subtract`, `concat`, `text_contains`, `lower`, `and`, `or`, `not`, `if`, `coalesce`, `date_add`, `date_difference`";
    let type_variants = "one of `boolean`, `integer`, `text`, `date`, `reference`, `list`, `optional`";
    let empty = json!([1,"","",[],[],[],[],[],[],""]);
    let long = "x".repeat(2048);
    let mut unknown_key = serde_json::Map::new();
    unknown_key.insert(long.clone(), JsonValue::Null);
    let mut unknown_expr = empty.clone();
    unknown_expr[7] = json!([["","",{"kind":long}]]);
    let mut escaped_scalar = empty.clone();
    escaped_scalar[0] = json!("\u{1b}".repeat(256));
    let mut external_payload = empty.clone();
    external_payload[6] = json!([["","",{"csv":"\u{1b}".repeat(80)},[]]]);
    let mut partial_map = empty.clone();
    partial_map[5] = json!([["","",{
        "a":["text"],"z":["list",["optional",["missing"]]],"zz":["text"]
    },[],[],[]]]);
    let mut partial_exprs = empty.clone();
    partial_exprs[7] = json!([["","",["and",[["today"],["not",["missing"]],["today"]]]]]);
    let mut partial_values = empty.clone();
    partial_values[4] = json!([["","",["boolean"],["list",["text"],[["text","first"],["text",false],["text","tail"]]]]]);
    let mut unrelated = empty;
    unrelated[0] = json!("bad");
    unrelated[3] = json!(vec!["x"; 128]);
    // The D/custom/format/copy columns are literal reductions of the retained
    // native primitives, not values obtained from the descriptor or its helpers.
    for (label, value, intended, maximum, diagnostic, custom, formatted, copy) in [
        ("long unknown key", JsonValue::Object(unknown_key), format!("unknown field `{long}`, expected {app_fields}"), 2048, 13312, 532638, 186494, 26628),
        ("long Expr tag and complete static list", unknown_expr, format!("unknown variant `{long}`, expected {expr_variants}"), 2048, 13312, 532638, 186494, 26628),
        ("escaped wrong scalar", escaped_scalar, format!("invalid type: string \"{}\", expected u32", "\\u{1b}".repeat(256)), 256, 2560, 102540, 35948, 5124),
        ("recognized external unit key, wrong payload", external_payload, format!("invalid type: string \"{}\", expected unit", "\\u{1b}".repeat(80)), 80, 1504, 60291, 21155, 3012),
        ("typed map prefix and nested tagged sequences", partial_map, format!("unknown variant `missing`, expected {type_variants}"), 8, 1072, 43011, 15107, 2148),
        ("Expr sequence prefix and unconsumed tail", partial_exprs, format!("unknown variant `missing`, expected {expr_variants}"), 7, 1066, 42771, 15023, 2136),
        ("DataValue sequence partial prefix", partial_values, "invalid type: boolean `false`, expected a string".into(), 7, 1066, 42771, 15023, 2136),
        ("no-string null", json!(null), "invalid type: null, expected struct AppDefinition".into(), 0, 1024, 41082, 14426, 2052),
        ("no-string number", json!(0), "invalid type: integer `0`, expected struct AppDefinition".into(), 0, 1024, 41082, 14426, 2052),
        ("no-string short sequence", json!([]), "invalid length 0, expected struct AppDefinition with 10 elements".into(), 0, 1024, 41082, 14426, 2052),
        ("no-string missing field", json!({}), "missing field `version`".into(), 0, 1024, 41082, 14426, 2052),
        ("unrelated strings after early wrong scalar", unrelated, "invalid type: string \"bad\", expected u32".into(), 3, 1042, 41811, 14687, 2088),
    ] {
        let expected = serde_json::from_value::<AppDefinition>(value.clone()).unwrap_err().to_string();
        assert_eq!(expected, intended, "{label}: wrong originating error");
        assert!(expected.len() <= diagnostic, "{label}: diagnostic bound");
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut scan = Work::new(u64::MAX);
        let state = WorkState::new(&mut scan);
        state.finish(root_value(&mut descriptor, &value, 0, Audit(Some(&state)))).unwrap();
        assert_eq!(descriptor.max_bytes, maximum, "{label}");
        let mut work = Work::new(u64::MAX);
        assert_eq!(paid(&serde_json::to_vec(&value).unwrap(), &mut work),
            Err(ContractWorkError::Contract(ContractError(expected))), "{label}");
        assert_eq!(work.zeroes, 2, "{label}: native Err has no success poll");
        assert!(work.calls.ends_with(&[320,custom,320,formatted,8,copy,5,101,descriptor.native]), "{label}: distinct error owners and native debit");
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_map_edges_keep_local_fallback_and_paid_classification() {
    // Each name is classified anew through the paid ordinary equality helper.
    // Near matches and dynamic map keys cannot become schema validation.
    for (key, expected, calls) in [
        ("parameters", true, vec![40,3,5,20]),
        ("values", true, vec![40,3,3,5,12]),
        ("arguments", true, vec![40,3,3,3,5,18]),
        ("columns", true, vec![40,3,3,3,3,5,14]),
        ("defaults", true, vec![40,3,3,3,3,3,5,16]),
        ("ordinary", false, vec![40,3,3,3,3,3,5,16]),
        ("Parameters", false, vec![40,3,5,20,3,3,3,3]),
        ("", false, vec![40,3,3,3,3,3]),
    ] {
        let mut work = Work::new(calls.iter().sum());
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(Conversion::map_field(key, Audit(Some(&state)))).unwrap(), expected);
        assert_eq!(work.calls, calls, "{key}");
        assert_eq!(work.left, 0);
    }
    // Independently reach helper header, equality header, scaled arithmetic and
    // comparison bytes; a refused comparison must never produce a classification.
    for (fuel, calls, left) in [
        (39, vec![40], 39),
        (42, vec![40,3], 2),
        (47, vec![40,3,5], 4),
        (67, vec![40,3,5,20], 19),
    ] {
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(Conversion::map_field("parameters", Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!((work.calls, work.left), (calls, left));
    }
    let h = size_of::<Expr>().max(size_of::<Type>()) as u64;
    let t = [size_of::<Expr>(),size_of::<Statement>(),size_of::<DataValue>(),size_of::<EntityDefinition>(),size_of::<ViewDefinition>(),size_of::<ActionDefinition>()]
        .into_iter().max().unwrap() as u64;
    let p = 24 + [size_of::<Type>(),size_of::<Expr>(),size_of::<DataValue>()]
        .into_iter().max().unwrap() as u64;
    assert!((120..=1024).contains(&t));
    for (value, expected, fuel, bytes, maximum) in [
        (json!({"a":null}), 1410 + 4*h, 553, 1, 1),
        (json!([{"a":null}]), 2227 + 6*h + 12*p + 4*t, 2779, 1, 1),
        (json!([{"a":null,"b":null}]), 3752 + 8*h + 14*p + 4*t, 4027, 2, 1),
        (json!({"parameters":{"a":null}}), 4205 + 6*h + 12*p, 2205, 11, 10),
        (json!({"ordinary":{"a":null}}), 3742 + 6*h, 1110, 9, 8),
        // An excluded parent must not suppress its eligible grandchild.
        (json!({"ordinary":{"parameters":{"a":null}}}), 6580 + 8*h + 12*p, 2762, 19, 10),
        // An eligible parent must not make an unrelated child eligible.
        (json!({"parameters":{"ordinary":{"a":null}}}), 6580 + 8*h + 12*p, 2762, 19, 10),
        // A colliding dynamic key retains a harmless extra universal maximum.
        (json!({"parameters":{"parameters":{"a":null}}}), 7043 + 8*h + 24*p, 3857, 21, 10),
    ] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        state.finish(root_value(&mut descriptor, &value, 0, Audit(Some(&state)))).unwrap();
        assert_eq!((descriptor.native, descriptor.bytes, descriptor.max_bytes), (expected, bytes, maximum), "{value}");
        assert_eq!(work.left, 0, "{value}");
    }
    // These budgets are independent literal prefixes, not taken from a previous
    // descriptor run. They stop before each new edge/header/branch operation.
    for (value, fuel, count, request, left, bytes, maximum, native) in [
        (json!({"a":null}), 5, 1, 6, 5, 0, 0, 0),
        (json!({"a":null}), 473, 20, 2, 1, 1, 1, 1383 + 2*h),
        (json!({"a":null}), 513, 21, 40, 39, 1, 1, 1383 + 2*h),
        (json!({"a":null}), 534, 27, 6, 5, 1, 1, 1383 + 2*h),
        (json!([null]), 1125, 18, 6, 5, 0, 0, 559 + 2*h),
        (json!([{"a":null}]), 1593, 37, 2, 1, 1, 1, 1969 + 4*h),
    ] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(fuel);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(root_value(&mut descriptor, &value, 0, Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
        assert_eq!((work.calls.len(), work.calls.last().copied(), work.left), (count, Some(request), left));
        assert_eq!((descriptor.bytes, descriptor.max_bytes, descriptor.native), (bytes, maximum, native));
        assert_eq!(work.zeroes, 0);
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn typed_exact_hints_keep_cap_fallback_and_paid_arithmetic() {
    // Serde's cap is a native-source pin, not the host budget or a wire-limit
    // alias. No gigantic input/allocation is needed to exercise this arithmetic.
    assert_eq!(NATIVE_HINT_CAP_BYTES, 1_048_576);
    for (len, slot, amount, fixed) in [
        (0, 1, 13, 13),
        (0, 128, 13, 13),
        (1, 128, 528, 16),
        (2, 128, 1043, 19),
        (8192, 128, 4218983, 24679),
        (8193, 128, 7365491, 24691),
        (16384, 64, 4243568, 49264),
        (16385, 64, 7389692, 49276),
        (1_048_576, 1, 7340198, 3145894),
        (1_048_577, 1, 10485944, 3145906),
    ] {
        let mut work = Work::new(344);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(Conversion::vector(len, slot, Audit(Some(&state)))).unwrap(), (amount, fixed), "len={len}, slot={slot}");
        assert_eq!(work.calls, [320,16,8]);
        assert_eq!(work.left, 0);
    }
    // Both temporary owners and the old ancestry-weighted fixed surcharge stay
    // funded even when no backing storage is requested.
    for (depth, expected) in [(0,26), (1,26), (2,39), (3,52)] {
        let mut descriptor = Conversion { bytes: 0, max_bytes: 0, native: 0 };
        let mut work = Work::new(363);
        let state = WorkState::new(&mut work);
        state.finish(descriptor.content(0, 128, depth, Audit(Some(&state)))).unwrap();
        assert_eq!(descriptor.native, expected);
        assert_eq!(work.calls, [320,16,8,16,3]);
        assert_eq!(work.left, 0);
    }
    for (len, slot) in [(0,128), (8192,128), (8193,128)] {
        for (fuel, calls, left) in [
            (319, vec![320], 319),
            (335, vec![320,16], 15),
            (343, vec![320,16,8], 7),
        ] {
            let mut work = Work::new(fuel);
            let state = WorkState::new(&mut work);
            assert_eq!(state.finish(Conversion::vector(len, slot, Audit(Some(&state)))),
                Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("typed test fuel"))));
            assert_eq!((work.calls, work.left), (calls, left));
        }
    }
    // Checked n*S, old doubled-capacity fallback, old storage multiplication,
    // then final requests+F overflow. None becomes a smaller successful reserve.
    for (len, slot, calls, used) in [
        (usize::MAX, 2, vec![320,16], 336),
        (usize::MAX / 2 + 1, 1, vec![320,16], 336),
        (usize::MAX / 8, 8, vec![320,16], 336),
        (usize::MAX / 8, 1, vec![320,16,8], 344),
    ] {
        let mut work = Work::new(u64::MAX);
        let state = WorkState::new(&mut work);
        assert_eq!(state.finish(Conversion::vector(len, slot, Audit(Some(&state)))),
            Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("capture work arithmetic overflow"))));
        assert_eq!((work.calls, work.left), (calls, u64::MAX - used));
    }
}
