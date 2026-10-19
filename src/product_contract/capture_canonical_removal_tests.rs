//! Focused normalization-removal controls; no existing oracle/control is changed.
use super::*;
use serde::ser::SerializeMap;
use serde_json::json;
use std::cell::Cell;

#[path = "../../tests/support/capture_source_fixture.rs"]
mod fixture;

// Frozen body from source303 product_contract.rs:188-205, SHA256
// abffde7b53093492497a3ced59e92e8d163e64ed168469a6f2b98c1581318614.
fn old_canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    fn sorted(value: JsonValue) -> JsonValue {
        match value {
            JsonValue::Object(map) => JsonValue::Object(
                map.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, sorted(v)))
                    .collect(),
            ),
            JsonValue::Array(array) => JsonValue::Array(array.into_iter().map(sorted).collect()),
            scalar => scalar,
        }
    }
    serde_json::to_value(value)
        .and_then(|v| serde_json::to_vec(&sorted(v)))
        .map_err(|e| ContractError(e.to_string()))
}

fn same_bytes<T: Serialize>(value: &T) {
    assert_eq!(canonical_bytes(value), old_canonical_bytes(value));
}

#[test]
fn canonical_removal_preserves_nested_and_typed_native_bytes() {
    for value in [
        json!(null),
        json!({}),
        json!([]),
        json!({"only": [{}, []]}),
        json!({"z": [{"b": 2, "a": 1}, ["β", "\n", "\""]],
               "a": {"z": null, "a": true}, "é": -1}),
        json!([u64::MAX, i64::MIN, -0.0, 1.25]),
        json!({"quote\"": "slash\\", "\n": "\0", "β": "é"}),
    ] {
        same_bytes(&value);
    }
    assert_eq!(
        canonical_bytes(&json!({"z": [{"b": 2, "a": 1}], "a": {}})).unwrap(),
        br#"{"a":{},"z":[{"a":1,"b":2}]}"#,
    );
    assert_eq!(
        canonical_digest(IdentityDomain::Program, &DataValue::Integer { value: 7 }).unwrap().as_str(),
        "e0d4cfc890f9fa2d8c5c70d329532b6df8cbbff3db76c504030c5dc578cebb78",
    );
    let app: AppDefinition = serde_json::from_value(fixture::minimal()).unwrap();
    same_bytes(&app);
    let data = DataSnapshot::empty("project", &app).unwrap();
    same_bytes(&data);
    same_bytes(&bytes_digest(IdentityDomain::Source, b"canonical fixture"));
    let expression = Expr::Literal {
        value_type: Type::Text,
        value: DataValue::Text { value: "β\n\"".into() },
    };
    same_bytes(&(0usize, "row", "record", &expression, &Type::Text, &RuntimeLimits::default()));
    same_bytes(&DataValue::List {
        item_type: Type::Text,
        items: vec![DataValue::Text { value: "z".into() }, DataValue::Text { value: "a".into() }],
    });
}

// This custom serializer exercises only the ordinary public helper. It is not
// admitted as a new paid Shape caller or assigned any native-cost contract.
struct OrderedCalls<'a> {
    calls: &'a Cell<usize>,
    fail: bool,
}
impl Serialize for OrderedCalls<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        self.calls.set(self.calls.get() + 1);
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("z", &1u8)?;
        if self.fail {
            return Err(serde::ser::Error::custom("canonical fixture failure"));
        }
        map.serialize_entry("a", &[2u8, 1])?;
        map.serialize_entry("z", &2u8)?;
        map.end()
    }
}

#[test]
fn canonical_removal_preserves_native_errors_and_one_serializer_call() {
    for fail in [false, true] {
        let old_calls = Cell::new(0);
        let new_calls = Cell::new(0);
        let expected = old_canonical_bytes(&OrderedCalls { calls: &old_calls, fail });
        let actual = canonical_bytes(&OrderedCalls { calls: &new_calls, fail });
        assert_eq!(actual, expected);
        assert_eq!((old_calls.get(), new_calls.get()), (1, 1));
        if fail {
            assert_eq!(actual, Err(ContractError("canonical fixture failure".into())));
        } else {
            assert_eq!(actual.unwrap(), br#"{"a":[2,1],"z":2}"#);
        }
    }
    let invalid_key = BTreeMap::from([(vec![1u8, 2], 3u8)]);
    assert_eq!(canonical_bytes(&invalid_key), old_canonical_bytes(&invalid_key));
    assert_eq!(canonical_bytes(&invalid_key), Err(ContractError("key must be a string".into())));
}

struct Work {
    calls: Vec<u64>,
    left: u64,
}
impl Work {
    fn new(left: u64) -> Self { Self { calls: Vec::new(), left } }
}
impl ContractWork for Work {
    fn charge(&mut self, units: u64) -> std::result::Result<(), WorkFailure> {
        self.calls.push(units);
        self.left = self.left.checked_sub(units)
            .ok_or(WorkFailure::BudgetExhausted("canonical test fuel"))?;
        Ok(())
    }
}

#[test]
#[cfg(target_pointer_width = "64")]
fn canonical_removal_retains_literal_native_region_and_refusal_prefix() {
    assert_eq!(std::mem::size_of::<JsonValue>(), 32);
    assert_eq!(std::mem::size_of::<std::io::Error>(), 8);
    assert_eq!(std::mem::size_of::<capture_work::WorkState<'_>>(), 56);
    assert_eq!(std::mem::size_of::<capture_canonical::Shape<'_>>(), 56);
    let value = DataValue::Integer { value: 7 };
    let mut work = Work::new(6941);
    let actual = canonical_bytes_with_work(&value, &mut work).unwrap();
    assert_eq!(actual, br#"{"kind":"integer","value":7}"#);
    assert_eq!(actual, old_canonical_bytes(&value).unwrap());
    // Independent literal native region: typed object 1540 (including final
    // BT_iter(2)=62), output growth547, writer688 =>2775. Descriptor fuel is
    // separate. FMT(128)=1855, S(128)=260 and serde-error owner85 remain paid.
    assert!(work.calls.ends_with(&[320, 1855, 8, 260, 5, 85, 2775, 0]));
    assert_eq!(work.left, 0);
    // Independent preparation prefix4166 + native2775 =6941; one unit less
    // reaches and refuses that native region, before its call or completion poll.
    let mut work = Work::new(6940);
    assert_eq!(
        canonical_bytes_with_work(&value, &mut work),
        Err(ContractWorkError::Resource(WorkFailure::BudgetExhausted("canonical test fuel"))),
    );
    assert!(work.calls.ends_with(&[320, 1855, 8, 260, 5, 85, 2775]));
    assert_eq!(work.left, 2774);
}
