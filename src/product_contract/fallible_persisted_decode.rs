//! Fallible decoding for persisted Product Contract records, events and sessions.
//!
//! The DTOs retain `FallibleValues` directly. This slice only constructs bounded
//! structural owners; higher-level snapshot composition and semantic validation
//! remain separate.
use crate::product_alloc::{self, AllocationContext, AllocationSite};
use crate::product_json::{Number, Value};
use serde::Serialize;

use super::fallible_value_decode::{decode_values, JsonKind, NumericValue, ValueDecodeError};
use super::{Digest, FallibleValues, RecordRef, MAX_ITEMS};

const RECORD_FIELDS: &[&str] = &["entity", "id", "revision", "created_program", "archived", "values"];
const RECORD_CHANGE_FIELDS: &[&str] = &["entity", "record", "before", "after", "archived"];
const BUSINESS_EVENT_FIELDS: &[&str] = &["id", "sequence", "operation_id", "action", "day", "program", "changes", "outputs"];
const RECORD_REF_FIELDS: &[&str] = &["entity", "record"];
const SESSION_STATE_FIELDS: &[&str] = &["view", "values", "focused_record"];

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedRecord {
    pub(super) entity: String,
    pub(super) id: String,
    pub(super) revision: u64,
    pub(super) created_program: Digest,
    pub(super) archived: bool,
    pub(super) values: FallibleValues,
}
impl DecodedRecord {
    pub(crate) fn entity(&self) -> &str { &self.entity }
    pub(crate) fn id(&self) -> &str { &self.id }
    pub(crate) fn revision(&self) -> u64 { self.revision }
    pub(crate) fn created_program(&self) -> &Digest { &self.created_program }
    pub(crate) fn archived(&self) -> bool { self.archived }
    pub(crate) fn values(&self) -> &FallibleValues { &self.values }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedRecordChange {
    pub(super) entity: String,
    pub(super) record: String,
    pub(super) before: Option<FallibleValues>,
    pub(super) after: FallibleValues,
    pub(super) archived: bool,
}
impl DecodedRecordChange {
    pub(crate) fn entity(&self) -> &str { &self.entity }
    pub(crate) fn record(&self) -> &str { &self.record }
    pub(crate) fn before(&self) -> Option<&FallibleValues> { self.before.as_ref() }
    pub(crate) fn after(&self) -> &FallibleValues { &self.after }
    pub(crate) fn archived(&self) -> bool { self.archived }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DecodedBusinessEvent {
    pub(super) id: String,
    pub(super) sequence: u64,
    pub(super) operation_id: String,
    pub(super) action: String,
    pub(super) day: i32,
    pub(super) program: Digest,
    pub(super) changes: Vec<DecodedRecordChange>,
    pub(super) outputs: Vec<Digest>,
}
impl DecodedBusinessEvent {
    pub(crate) fn id(&self) -> &str { &self.id }
    pub(crate) fn sequence(&self) -> u64 { self.sequence }
    pub(crate) fn operation_id(&self) -> &str { &self.operation_id }
    pub(crate) fn action(&self) -> &str { &self.action }
    pub(crate) fn day(&self) -> i32 { self.day }
    pub(crate) fn program(&self) -> &Digest { &self.program }
    pub(crate) fn changes(&self) -> &[DecodedRecordChange] { &self.changes }
    pub(crate) fn outputs(&self) -> &[Digest] { &self.outputs }
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct DecodedSessionState {
    pub(super) view: String,
    pub(super) values: FallibleValues,
    pub(super) focused_record: Option<RecordRef>,
}

impl DecodedSessionState {
    pub(super) fn view(&self) -> &str { &self.view }
    pub(super) fn values(&self) -> &FallibleValues { &self.values }
    pub(super) fn focused_record(&self) -> Option<&RecordRef> { self.focused_record.as_ref() }
    pub(super) fn try_set_view_and_clear_focus(
        &mut self,
        view: &str,
        cx: &mut AllocationContext,
    ) -> std::result::Result<(), product_alloc::AllocationFailure> {
        self.view = product_alloc::try_copy_str(view, cx, AllocationSite::String)?;
        self.focused_record = None;
        Ok(())
    }
}

pub(super) fn decode_record(value: Value, cx: &mut AllocationContext) -> Result<DecodedRecord, ValueDecodeError> {
    let fields = object(value, "a struct Record", RECORD_FIELDS, cx)?;
    let mut entity = None;
    let mut id = None;
    let mut revision = None;
    let mut created_program = None;
    let mut archived = None;
    let mut values = None;
    for (key, value) in fields.into_iter() {
        match key.as_str() {
            "entity" => entity = Some(string(value)?),
            "id" => id = Some(string(value)?),
            "revision" => revision = Some(integer_u64(value)?),
            "created_program" => created_program = Some(digest(value)?),
            "archived" => archived = Some(boolean(value)?),
            "values" => values = Some(values_map(value, cx)?),
            _ => return Err(unknown_field(key, RECORD_FIELDS)),
        }
    }
    Ok(DecodedRecord {
        entity: required(entity, "entity")?,
        id: required(id, "id")?,
        revision: required(revision, "revision")?,
        created_program: required(created_program, "created_program")?,
        archived: required(archived, "archived")?,
        values: required(values, "values")?,
    })
}

pub(super) fn decode_record_change(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedRecordChange, ValueDecodeError> {
    let fields = object(value, "a struct RecordChange", RECORD_CHANGE_FIELDS, cx)?;
    let mut entity = None;
    let mut record = None;
    let mut before = None;
    let mut after = None;
    let mut archived = None;
    for (key, value) in fields.into_iter() {
        match key.as_str() {
            "entity" => entity = Some(string(value)?),
            "record" => record = Some(string(value)?),
            "before" => before = Some(optional_values(value, cx)?),
            "after" => after = Some(values_map(value, cx)?),
            "archived" => archived = Some(boolean(value)?),
            _ => return Err(unknown_field(key, RECORD_CHANGE_FIELDS)),
        }
    }
    Ok(DecodedRecordChange {
        entity: required(entity, "entity")?,
        record: required(record, "record")?,
        before: before.unwrap_or(None),
        after: required(after, "after")?,
        archived: required(archived, "archived")?,
    })
}

pub(super) fn decode_business_event(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedBusinessEvent, ValueDecodeError> {
    let fields = object(value, "a struct BusinessEvent", BUSINESS_EVENT_FIELDS, cx)?;
    let mut id = None;
    let mut sequence = None;
    let mut operation_id = None;
    let mut action = None;
    let mut day = None;
    let mut program = None;
    let mut record_changes = None;
    let mut outputs = None;
    for (key, value) in fields.into_iter() {
        match key.as_str() {
            "id" => id = Some(string(value)?),
            "sequence" => sequence = Some(integer_u64(value)?),
            "operation_id" => operation_id = Some(string(value)?),
            "action" => action = Some(string(value)?),
            "day" => day = Some(integer_i32(value)?),
            "program" => program = Some(digest(value)?),
            "changes" => record_changes = Some(changes(value, cx)?),
            "outputs" => outputs = Some(digests(value, cx)?),
            _ => return Err(unknown_field(key, BUSINESS_EVENT_FIELDS)),
        }
    }
    Ok(DecodedBusinessEvent {
        id: required(id, "id")?,
        sequence: required(sequence, "sequence")?,
        operation_id: required(operation_id, "operation_id")?,
        action: required(action, "action")?,
        day: required(day, "day")?,
        program: required(program, "program")?,
        changes: required(record_changes, "changes")?,
        outputs: required(outputs, "outputs")?,
    })
}

pub(super) fn decode_record_ref(value: Value, cx: &mut AllocationContext) -> Result<RecordRef, ValueDecodeError> {
    let fields = match value {
        Value::Object(fields) => fields,
        Value::Array(values) => {
            if values.len() != RECORD_REF_FIELDS.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: RECORD_REF_FIELDS.len() });
            }
            let mut fields = crate::product_alloc::ordered::OrderedMap::new();
            fields.try_reserve(RECORD_REF_FIELDS.len(), cx)?;
            for (name, value) in RECORD_REF_FIELDS.iter().zip(values.into_iter().take(RECORD_REF_FIELDS.len())) {
                let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.try_insert(name, value, cx)?;
            }
            fields
        }
        value => return Err(invalid_type(value, "a struct RecordRef")),
    };
    let mut entity = None;
    let mut record = None;
    for (key, value) in fields.into_entries() {
        match key.as_str() {
            "entity" => entity = Some(string(value)?),
            "record" => record = Some(string(value)?),
            _ => return Err(unknown_field(key, RECORD_REF_FIELDS)),
        }
    }
    Ok(RecordRef { entity: required(entity, "entity")?, record: required(record, "record")? })
}

pub(super) fn decode_session_state(
    value: Value,
    cx: &mut AllocationContext,
) -> Result<DecodedSessionState, ValueDecodeError> {
    let fields = object(value, "a struct SessionState", SESSION_STATE_FIELDS, cx)?;
    let mut view = None;
    let mut values = None;
    let mut focused_record = None;
    for (key, value) in fields.into_iter() {
        match key.as_str() {
            "view" => view = Some(string(value)?),
            "values" => values = Some(values_map(value, cx)?),
            "focused_record" => focused_record = Some(optional_record_ref(value, cx)?),
            _ => return Err(unknown_field(key, SESSION_STATE_FIELDS)),
        }
    }
    Ok(DecodedSessionState {
        view: required(view, "view")?,
        values: required(values, "values")?,
        focused_record: focused_record.unwrap_or(None),
    })
}

fn values_map(value: Value, cx: &mut AllocationContext) -> Result<FallibleValues, ValueDecodeError> {
    let entries = match value {
        Value::Object(entries) => entries,
        value => return Err(invalid_type(value, "a map")),
    };
    bounded_length(entries.len(), "value map entry count")?;
    decode_values(Value::Object(entries), cx)
}

fn optional_values(value: Value, cx: &mut AllocationContext) -> Result<Option<FallibleValues>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => values_map(value, cx).map(Some),
    }
}

fn optional_record_ref(value: Value, cx: &mut AllocationContext) -> Result<Option<RecordRef>, ValueDecodeError> {
    match value {
        Value::Null => Ok(None),
        value => decode_record_ref(value, cx).map(Some),
    }
}

fn changes(value: Value, cx: &mut AllocationContext) -> Result<Vec<DecodedRecordChange>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), "record change count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(decode_record_change(value, cx)?);
    }
    Ok(decoded)
}

fn digests(value: Value, cx: &mut AllocationContext) -> Result<Vec<Digest>, ValueDecodeError> {
    let values = array(value)?;
    bounded_length(values.len(), "event output count")?;
    let mut decoded = Vec::new();
    product_alloc::try_reserve(&mut decoded, values.len(), cx, AllocationSite::Vector)?;
    for value in values {
        decoded.push(digest(value)?);
    }
    Ok(decoded)
}

fn digest(value: Value) -> Result<Digest, ValueDecodeError> {
    let value = string(value)?;
    Digest::from_owned_lower_hex(value).ok_or(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))
}

fn object(
    value: Value,
    expected: &'static str,
    names: &'static [&'static str],
    cx: &mut AllocationContext,
) -> Result<std::vec::IntoIter<(String, Value)>, ValueDecodeError> {
    match value {
        Value::Object(fields) => Ok(fields.into_entries()),
        Value::Array(values) => {
            if values.len() != names.len() {
                return Err(ValueDecodeError::InvalidLength { actual: values.len(), expected: names.len() });
            }
            let mut fields = Vec::new();
            product_alloc::try_reserve(&mut fields, names.len(), cx, AllocationSite::Vector)?;
            for (name, value) in names.iter().zip(values.into_iter().take(names.len())) {
                let name = product_alloc::try_copy_str(name, cx, AllocationSite::String)?;
                fields.push((name, value));
            }
            Ok(fields.into_iter())
        }
        value => Err(invalid_type(value, expected)),
    }
}

fn array(value: Value) -> Result<Vec<Value>, ValueDecodeError> {
    match value {
        Value::Array(values) => Ok(values),
        value => Err(invalid_type(value, "a sequence")),
    }
}

fn string(value: Value) -> Result<String, ValueDecodeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(invalid_type(value, "a string")),
    }
}

fn boolean(value: Value) -> Result<bool, ValueDecodeError> {
    match value {
        Value::Bool(value) => Ok(value),
        value => Err(invalid_type(value, "a boolean")),
    }
}

fn integer_u64(value: Value) -> Result<u64, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => Ok(value),
        Value::Number(Number::I64(value)) => u64::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "u64" }
        }),
        value => Err(invalid_type(value, "u64")),
    }
}

fn integer_i32(value: Value) -> Result<i32, ValueDecodeError> {
    match value {
        Value::Number(Number::U64(value)) => i32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Unsigned(value), expected: "i32" }
        }),
        Value::Number(Number::I64(value)) => i32::try_from(value).map_err(|_| {
            ValueDecodeError::InvalidValue { actual: NumericValue::Signed(value), expected: "i32" }
        }),
        value => Err(invalid_type(value, "i32")),
    }
}

fn invalid_type(value: Value, expected: &'static str) -> ValueDecodeError {
    let actual = match value {
        Value::Null => JsonKind::Null,
        Value::Bool(_) => JsonKind::Boolean,
        Value::Number(Number::U64(_)) => JsonKind::UnsignedInteger,
        Value::Number(Number::I64(_)) => JsonKind::SignedInteger,
        Value::Number(Number::F64(_)) => JsonKind::FloatingPoint,
        Value::String(_) => JsonKind::String,
        Value::Array(_) => JsonKind::Sequence,
        Value::Object(_) => JsonKind::Map,
    };
    ValueDecodeError::InvalidType { actual, expected }
}

fn unknown_field(key: String, expected: &'static [&'static str]) -> ValueDecodeError {
    ValueDecodeError::UnknownField { key, expected }
}

fn required<T>(value: Option<T>, field: &'static str) -> Result<T, ValueDecodeError> {
    value.ok_or(ValueDecodeError::MissingField(field))
}

fn bounded_length(len: usize, resource: &'static str) -> Result<(), ValueDecodeError> {
    if len > MAX_ITEMS {
        Err(ValueDecodeError::LimitExceeded { resource, limit: MAX_ITEMS })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::product_alloc::{AllocationFailureKind, AllocationSite};
    use crate::product_contract::{BusinessEvent, Record, RecordChange, SessionState};
    use crate::product_json::{self, Cause};
    use std::num::NonZeroUsize;

    const D: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn syntax(source: &str) -> Value {
        product_json::parse_json_bytes(source.as_bytes(), &mut AllocationContext::new()).unwrap()
    }

    fn typed<T: serde::de::DeserializeOwned>(source: &str) -> T {
        serde_json::from_str(source).unwrap()
    }

    fn allocation(error: ValueDecodeError, cx: &AllocationContext) {
        let ValueDecodeError::Allocation(failure) = error else { panic!("expected allocation refusal") };
        assert_eq!(Some(failure), cx.first_failure());
        assert_eq!(failure.kind, AllocationFailureKind::Injected);
        assert!(matches!(failure.site, AllocationSite::String | AllocationSite::OrderedMap | AllocationSite::Vector | AllocationSite::Box));
    }

    #[test]
    fn record_and_session_dtos_preserve_wire_bytes_and_move_owned_values() {
        let record_source = format!(r#"{{"entity":"person","id":"p1","revision":7,"created_program":"{D}","archived":false,"values":{{"age":{{"kind":"integer","value":4}},"items":{{"kind":"list","item_type":{{"kind":"optional","item":{{"kind":"text"}}}},"items":[{{"kind":"text","value":"雪"}}]}}}}}}"#);
        let expected_record: Record = typed(&record_source);
        let mut cx = AllocationContext::new();
        let decoded_record = decode_record(syntax(&record_source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected_record).unwrap(), super::super::canonical_bytes(&decoded_record).unwrap());

        let session_source = format!(r#"{{"view":"people","values":{{"state":{{"kind":"text","value":"ok"}}}},"focused_record":{{"entity":"person","record":"p1"}}}}"#);
        let expected_session: SessionState = typed(&session_source);
        let mut cx = AllocationContext::new();
        let decoded_session = decode_session_state(syntax(&session_source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected_session).unwrap(), super::super::canonical_bytes(&decoded_session).unwrap());
        assert!(decode_session_state(syntax(r#"{"view":"people","values":{}}"#), &mut AllocationContext::new()).unwrap().focused_record.is_none());

        let fields = serde_json::to_value(&expected_session).unwrap();
        let sequence = serde_json::json!([fields["view"].clone(), fields["values"].clone(), fields["focused_record"].clone(), false]).to_string();
        assert!(serde_json::from_value::<SessionState>(serde_json::from_str(&sequence).unwrap()).is_err());
        assert!(matches!(decode_session_state(syntax(&sequence), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 4, expected: 3 })));
    }

    #[test]
    fn changes_events_and_refs_match_serde_and_handle_absent_before() {
        let change_source = r#"{"entity":"person","record":"p1","before":{"age":{"kind":"integer","value":3}},"after":{"age":{"kind":"integer","value":4}},"archived":false}"#;
        let expected_change: RecordChange = typed(change_source);
        let mut cx = AllocationContext::new();
        let decoded_change = decode_record_change(syntax(change_source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected_change).unwrap(), super::super::canonical_bytes(&decoded_change).unwrap());

        let null_before = r#"{"entity":"person","record":"p1","before":null,"after":{},"archived":true}"#;
        assert!(decode_record_change(syntax(null_before), &mut AllocationContext::new()).unwrap().before.is_none());
        let omitted_before = r#"{"entity":"person","record":"p1","after":{},"archived":true}"#;
        assert!(decode_record_change(syntax(omitted_before), &mut AllocationContext::new()).unwrap().before.is_none());

        let event_source = format!(r#"{{"id":"event1","sequence":9,"operation_id":"op1","action":"edit","day":-20,"program":"{D}","changes":[{change_source}],"outputs":["{D}"]}}"#);
        let expected_event: BusinessEvent = typed(&event_source);
        let mut cx = AllocationContext::new();
        let decoded_event = decode_business_event(syntax(&event_source), &mut cx).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected_event).unwrap(), super::super::canonical_bytes(&decoded_event).unwrap());

        let record_ref_source = r#"{"entity":"person","record":"p1"}"#;
        let expected_ref: RecordRef = typed(record_ref_source);
        let decoded_ref = decode_record_ref(syntax(record_ref_source), &mut AllocationContext::new()).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected_ref).unwrap(), super::super::canonical_bytes(&decoded_ref).unwrap());
    }

    #[test]
    fn allocation_refusal_at_every_reached_owner_growth_cleans_and_retries() {
        let source = format!(r#"{{"id":"event1","sequence":9,"operation_id":"op1","action":"edit","day":20,"program":"{D}","changes":[{{"entity":"person","record":"p1","before":{{"age":{{"kind":"integer","value":3}}}},"after":{{"items":{{"kind":"list","item_type":{{"kind":"optional","item":{{"kind":"text"}}}},"items":[{{"kind":"text","value":"x"}}]}}}},"archived":false}}],"outputs":["{D}","{D}"]}}"#);
        let expected: BusinessEvent = typed(&source);
        let mut cx = AllocationContext::new();
        assert_eq!(decode_business_event(syntax(&source), &mut cx).unwrap(), DecodedBusinessEvent {
            id: expected.id.clone(), sequence: expected.sequence, operation_id: expected.operation_id.clone(),
            action: expected.action.clone(), day: expected.day, program: expected.program.clone(),
            changes: vec![DecodedRecordChange { entity: expected.changes[0].entity.clone(), record: expected.changes[0].record.clone(),
                before: Some(decode_values(syntax(r#"{"age":{"kind":"integer","value":3}}"#), &mut AllocationContext::new()).unwrap()),
                after: decode_values(syntax(r#"{"items":{"kind":"list","item_type":{"kind":"optional","item":{"kind":"text"}},"items":[{"kind":"text","value":"x"}]}}"#), &mut AllocationContext::new()).unwrap(), archived: false }],
            outputs: expected.outputs.clone(),
        });
        let growths = cx.growth_requests();
        assert_eq!(growths, 6); // changes vec, two maps, Type box, item vec, outputs vec
        for nth in 1..=growths {
            let mut cx = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_business_event(syntax(&source), &mut cx).unwrap_err(), &cx);
            let mut retry = AllocationContext::new();
            assert_eq!(decode_business_event(syntax(&source), &mut retry).unwrap().sequence, expected.sequence);
        }
    }

    #[test]
    fn malformed_unknown_duplicate_and_numeric_inputs_are_rejected() {
        assert!(matches!(decode_record(syntax(r#"{"id":"x"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::MissingField("entity"))));
        assert!(matches!(decode_record_ref(syntax(r#"{"entity":"e","record":"r","extra":true}"#), &mut AllocationContext::new()), Err(ValueDecodeError::UnknownField { key, expected: RECORD_REF_FIELDS }) if key == "extra"));
        assert!(matches!(decode_record_ref(syntax(r#"{"entity":1,"record":"x"}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidType { actual: JsonKind::UnsignedInteger, .. })));
        assert!(matches!(decode_record(syntax(&format!(r#"{{"entity":"e","id":"r","revision":-1,"created_program":"{D}","archived":false,"values":{{}}}}"#)), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "u64", .. })));
        assert!(matches!(decode_business_event(syntax(&format!(r#"{{"id":"e","sequence":1,"operation_id":"o","action":"a","day":2147483648,"program":"{D}","changes":[],"outputs":[]}}"#)), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidValue { expected: "i32", .. })));
        assert!(matches!(decode_record(syntax(r#"{"entity":"e","id":"r","revision":1,"created_program":"bad","archived":false,"values":{}}"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidFormat("lowercase SHA-256 digest"))));
        let mut cx = AllocationContext::new();
        let error = match product_json::parse_json_bytes(br#"{"entity":"e","entity":"f"}"#, &mut cx) {
            Err(error) => error,
            Ok(_) => panic!("duplicate object member was accepted"),
        };
        assert_eq!(error.cause, Cause::DuplicateObjectMember);
    }

    #[test]
    fn persisted_maps_and_event_vectors_obey_max_items() {
        let mut pairs = Vec::new();
        for index in 0..=MAX_ITEMS {
            pairs.push(format!("\"k{index:03}\":{{\"kind\":\"null\"}}"));
        }
        let values = format!("{{{}}}", pairs.join(","));
        let record = format!(r#"{{"entity":"e","id":"r","revision":1,"created_program":"{D}","archived":false,"values":{values}}}"#);
        assert!(matches!(decode_record(syntax(&record), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "value map entry count", limit: MAX_ITEMS })));

        let change = r#"{"entity":"e","record":"r","before":null,"after":{},"archived":false}"#;
        let changes = vec![change; MAX_ITEMS + 1].join(",");
        let event = format!(r#"{{"id":"e","sequence":1,"operation_id":"o","action":"a","day":0,"program":"{D}","changes":[{changes}],"outputs":[]}}"#);
        assert!(matches!(decode_business_event(syntax(&event), &mut AllocationContext::new()), Err(ValueDecodeError::LimitExceeded { resource: "record change count", limit: MAX_ITEMS })));

        let exact_items = vec![r#"{"kind":"null"}"#; super::super::MAX_COLLECTION].join(",");
        let exact_list = format!(r#"{{"entity":"e","id":"r","revision":1,"created_program":"{D}","archived":false,"values":{{"items":{{"kind":"list","item_type":{{"kind":"text"}},"items":[{exact_items}]}}}}}}"#);
        let mut cx = AllocationContext::new();
        let decoded = decode_record(syntax(&exact_list), &mut cx).unwrap();
        assert!(matches!(decoded.values.get("items"), Some(crate::product_contract::DataValue::List { items, .. }) if items.len() == super::super::MAX_COLLECTION));

        let items = vec![r#"{"kind":"null"}"#; super::super::MAX_COLLECTION + 1].join(",");
        let oversized_list = format!(r#"{{"entity":"e","id":"r","revision":1,"created_program":"{D}","archived":false,"values":{{"items":{{"kind":"list","item_type":{{"kind":"text"}},"items":[{items}]}}}}}}"#);
        let mut cx = AllocationContext::new();
        assert!(matches!(decode_record(syntax(&oversized_list), &mut cx), Err(ValueDecodeError::LimitExceeded { resource: "list item count", limit: super::super::MAX_COLLECTION })));
        assert_eq!(cx.growth_requests(), 0, "oversized list is rejected before its item vector or value map is reserved");
    }

    #[test]
    fn string_owners_transfer_without_string_growth() {
        let value = decode_record_ref(syntax(r#"{"entity":"é","record":"雪"}"#), &mut AllocationContext::new()).unwrap();
        assert_eq!(value.entity, "é");
    }

    #[test]
    fn record_ref_struct_sequence_matches_serde_and_reserves_fallibly() {
        let source = r#"["person","p1"]"#;
        let expected: RecordRef = typed(source);
        let mut baseline = AllocationContext::new();
        let decoded = decode_record_ref(syntax(source), &mut baseline).unwrap();
        assert_eq!(super::super::canonical_bytes(&expected).unwrap(), super::super::canonical_bytes(&decoded).unwrap());
        assert!(serde_json::from_value::<RecordRef>(serde_json::json!(["person", "p1", null])).is_err());
        assert!(matches!(decode_record_ref(syntax(r#"["person","p1",null]"#), &mut AllocationContext::new()), Err(ValueDecodeError::InvalidLength { actual: 3, expected: 2 })));
        let growths = baseline.growth_requests();
        assert!(growths > 0);
        for nth in 1..=growths {
            let mut fail = AllocationContext::fail_nth_growth(NonZeroUsize::new(nth).unwrap(), true);
            allocation(decode_record_ref(syntax(source), &mut fail).unwrap_err(), &fail);
            let mut retry = AllocationContext::new();
            assert_eq!(
                super::super::canonical_bytes(&expected).unwrap(),
                super::super::canonical_bytes(&decode_record_ref(syntax(source), &mut retry).unwrap()).unwrap(),
            );
        }
    }
}
