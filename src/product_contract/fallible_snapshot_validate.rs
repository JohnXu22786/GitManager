//! Shared Product DataSnapshot semantic validation for the legacy and
//! fallible ordered-owner representations. The validator has one execution
//! path so wire and error precedence cannot drift between the two models.
use super::*;
use std::collections::BTreeSet;

trait ValueMap {
    fn len(&self) -> usize;
    fn get(&self, key: &str) -> Option<&DataValue>;
    fn contains_key(&self, key: &str) -> bool;
    fn visit(&self, f: &mut dyn FnMut(&str, &DataValue) -> Result<()>) -> Result<()>;
}
impl ValueMap for Values {
    fn len(&self) -> usize { BTreeMap::len(self) }
    fn get(&self, key: &str) -> Option<&DataValue> { BTreeMap::get(self, key) }
    fn contains_key(&self, key: &str) -> bool { BTreeMap::contains_key(self, key) }
    fn visit(&self, f: &mut dyn FnMut(&str, &DataValue) -> Result<()>) -> Result<()> {
        for (key, value) in self { f(key, value)?; }
        Ok(())
    }
}
impl ValueMap for FallibleValues {
    fn len(&self) -> usize { FallibleValues::len(self) }
    fn get(&self, key: &str) -> Option<&DataValue> { FallibleValues::get(self, key) }
    fn contains_key(&self, key: &str) -> bool { FallibleValues::contains_key(self, key) }
    fn visit(&self, f: &mut dyn FnMut(&str, &DataValue) -> Result<()>) -> Result<()> {
        for (key, value) in self.iter() { f(key, value)?; }
        Ok(())
    }
}
trait Row {
    type Values: ValueMap;
    fn entity(&self) -> &str;
    fn id(&self) -> &str;
    fn revision(&self) -> u64;
    fn archived(&self) -> bool;
    fn values(&self) -> &Self::Values;
}
impl Row for super::Record {
    type Values = Values;
    fn entity(&self) -> &str { &self.entity }
    fn id(&self) -> &str { &self.id }
    fn revision(&self) -> u64 { self.revision }
    fn archived(&self) -> bool { self.archived }
    fn values(&self) -> &Values { &self.values }
}
impl Row for super::fallible_persisted_decode::DecodedRecord {
    type Values = FallibleValues;
    fn entity(&self) -> &str { &self.entity }
    fn id(&self) -> &str { &self.id }
    fn revision(&self) -> u64 { self.revision }
    fn archived(&self) -> bool { self.archived }
    fn values(&self) -> &FallibleValues { &self.values }
}
trait Change {
    type Values: ValueMap;
    fn entity(&self) -> &str;
    fn record(&self) -> &str;
    fn before(&self) -> Option<&Self::Values>;
    fn after(&self) -> &Self::Values;
}
impl Change for super::RecordChange {
    type Values = Values;
    fn entity(&self) -> &str { &self.entity }
    fn record(&self) -> &str { &self.record }
    fn before(&self) -> Option<&Values> { self.before.as_ref() }
    fn after(&self) -> &Values { &self.after }
}
impl Change for super::fallible_persisted_decode::DecodedRecordChange {
    type Values = FallibleValues;
    fn entity(&self) -> &str { &self.entity }
    fn record(&self) -> &str { &self.record }
    fn before(&self) -> Option<&FallibleValues> { self.before.as_ref() }
    fn after(&self) -> &FallibleValues { &self.after }
}
trait Event {
    type Change: Change;
    fn id(&self) -> &str;
    fn operation(&self) -> &str;
    fn action(&self) -> &str;
    fn day(&self) -> i32;
    fn sequence(&self) -> u64;
    fn changes(&self) -> &[Self::Change];
    fn outputs_len(&self) -> usize;
}
impl Event for super::BusinessEvent {
    type Change = super::RecordChange;
    fn id(&self) -> &str { &self.id }
    fn operation(&self) -> &str { &self.operation_id }
    fn action(&self) -> &str { &self.action }
    fn day(&self) -> i32 { self.day }
    fn sequence(&self) -> u64 { self.sequence }
    fn changes(&self) -> &[Self::Change] { &self.changes }
    fn outputs_len(&self) -> usize { self.outputs.len() }
}
impl Event for super::fallible_persisted_decode::DecodedBusinessEvent {
    type Change = super::fallible_persisted_decode::DecodedRecordChange;
    fn id(&self) -> &str { &self.id }
    fn operation(&self) -> &str { &self.operation_id }
    fn action(&self) -> &str { &self.action }
    fn day(&self) -> i32 { self.day }
    fn sequence(&self) -> u64 { self.sequence }
    fn changes(&self) -> &[Self::Change] { &self.changes }
    fn outputs_len(&self) -> usize { self.outputs.len() }
}
trait Snapshot: capture_canonical::ManualShape {
    type Row: Row;
    type Event: Event;
    fn version(&self) -> u32;
    fn project_id(&self) -> &str;
    fn generation(&self) -> u64;
    fn schema(&self) -> &[EntityDefinition];
    fn records(&self) -> &[Self::Row];
    fn events(&self) -> &[Self::Event];
}
impl Snapshot for super::DataSnapshot {
    type Row = super::Record;
    type Event = super::BusinessEvent;
    fn version(&self) -> u32 { self.version }
    fn project_id(&self) -> &str { &self.project_id }
    fn generation(&self) -> u64 { self.generation }
    fn schema(&self) -> &[EntityDefinition] { &self.schema }
    fn records(&self) -> &[Self::Row] { &self.records }
    fn events(&self) -> &[Self::Event] { &self.events }
}
impl Snapshot for super::fallible_snapshot_decode::DecodedDataSnapshot {
    type Row = super::fallible_persisted_decode::DecodedRecord;
    type Event = super::fallible_persisted_decode::DecodedBusinessEvent;
    fn version(&self) -> u32 { self.version }
    fn project_id(&self) -> &str { &self.project_id }
    fn generation(&self) -> u64 { self.generation }
    fn schema(&self) -> &[EntityDefinition] { &self.schema }
    fn records(&self) -> &[Self::Row] { &self.records }
    fn events(&self) -> &[Self::Event] { &self.events }
}

pub(super) fn validate<T: Snapshot>(snapshot: &T, a: Audit<'_>) -> Result<()> {
    a.pay(8)?;
    a.version(snapshot.version())?;
    a.check_id(snapshot.project_id())?;
    let records_slice = snapshot.records();
    let events_slice = snapshot.events();
    let schema_slice = snapshot.schema();
    a.require(records_slice.len() <= MAX_COLLECTION && events_slice.len() <= MAX_EVENTS,
        "data record/event budget exceeded")?;
    a.pay(8)?;
    let mut validator = Validator { entities: schema_slice, state: &[], actions: &[],
        outputs: &[], app: super::AppRef::Snapshot, allocation: None, allocation_failure: None, nodes: 0, audit: a };
    validate_entities(schema_slice, &mut validator)?;
    a.pay(8)?;
    let mut records = BTreeSet::new();
    a.slice(records_slice.len())?;
    for record in records_slice {
        a.pay(12)?;
        a.check_id(record.id())?;
        a.require(record.revision() > 0, "record revision must be positive")?;
        a.insert::<(&str, &str), ()>(records.len(), a.pair_comparator(record.entity(), record.id())?)?;
        a.require(records.insert((record.entity(), record.id())), "duplicate record identity")?;
        let values = record.values();
        a.pay(4)?;
        let entity = validator.entity(record.entity())?;
        a.bounded(values.len())?;
        a.map_iter(values.len())?;
        values.visit(&mut |id, value| validate_value_audited(
            value, &validator.field(&entity.id, id)?.value_type, 0, a))?;
        a.slice(entity.fields.len())?;
        for field in &entity.fields {
            a.map_search(values.len(), field.id.len())?;
            a.require(values.contains_key(&field.id) || matches!(field.value_type, Type::Optional { .. }),
                "record lacks required field")?;
        }
    }
    a.slice(records_slice.len())?;
    for record in records_slice {
        let values = record.values();
        a.pay(12)?;
        a.map_iter(values.len())?;
        values.visit(&mut |_, value| {
            a.pay(12)?;
            validate_references_audited(value, &records, a)
        })?;
    }
    a.slice(schema_slice.len())?;
    for entity in schema_slice {
        a.pay(12)?;
        a.slice(entity.unique.len())?;
        for fields in &entity.unique {
            a.pay(8)?;
            let mut tuples = BTreeSet::new();
            a.slice(records_slice.len())?;
            for record in records_slice {
                a.pay(12)?;
                if !(a.eq_str(record.entity(), &entity.id)? && !record.archived()) { continue; }
                a.pay(12)?;
                a.scaled(fields.len(), (2 * std::mem::size_of::<&DataValue>()) as u64, 4)?;
                if a.enabled() {
                    a.slice(fields.len())?;
                    for field in fields { a.pay(5)?; a.map_search(record.values().len(), field.len())?; }
                }
                let null = DataValue::Null;
                let tuple: Vec<_> = fields.iter().map(|field|
                    record.values().get(field).unwrap_or(&null)).collect();
                let mut has_null = false;
                a.pay(1)?;
                for value in &tuple { a.pay(12)?; a.pay(4)?; if matches!(value, DataValue::Null) { has_null = true; break; } }
                a.pay(1)?;
                if has_null { continue; }
                let bytes = capture_canonical::canonical_bytes_audited(&tuple, a)?;
                a.insert::<Vec<u8>, ()>(tuples.len(), a.comparator(bytes.len())?)?;
                a.require(tuples.insert(bytes), "duplicate unique field tuple")?;
            }
        }
    }
    a.pay(17)?;
    let mut event_ids = BTreeSet::new();
    let mut operations = BTreeSet::new();
    let mut previous = 0;
    a.slice(events_slice.len())?;
    for event in events_slice {
        a.pay(12)?;
        a.check_id(event.id())?;
        a.check_id(event.operation())?;
        a.check_id(event.action())?;
        a.day(event.day())?;
        a.insert::<&String, ()>(event_ids.len(), a.comparator(event.id().len())?)?;
        let new_event = event_ids.insert(event.id());
        let distinct = if new_event {
            a.insert::<&String, ()>(operations.len(), a.comparator(event.operation().len())?)?;
            operations.insert(event.operation())
        } else { false };
        a.require(distinct, "duplicate event or operation receipt")?;
        a.require(event.sequence() > previous && event.sequence() <= snapshot.generation(),
            "event sequence is not ordered within generation")?;
        previous = event.sequence();
        let changes = event.changes();
        a.bounded(changes.len())?;
        a.bounded(event.outputs_len())?;
        a.pay(8)?;
        let mut changed = BTreeSet::new();
        a.slice(changes.len())?;
        for change in changes {
            a.pay(12)?;
            a.check_id(change.record())?;
            a.insert::<(&String, &String), ()>(changed.len(), a.pair_comparator(change.entity(), change.record())?)?;
            a.require(changed.insert((change.entity(), change.record())), "duplicate event record change")?;
            validator.entity(change.entity())?;
            if a.enabled() {
                a.pay(bounded_input::capture_payment_core::SEARCH_DESCRIPTOR)?;
                a.amount(bounded_input::capture_payment_core::btree_search(records.len(),
                    a.pair_comparator(change.entity(), change.record())?))?;
            }
            a.require(records.contains(&(change.entity(), change.record())), "event record is missing")?;
            a.pay(8)?;
            let mut validate_values = |values: &dyn ValueMap| -> Result<()> {
                a.pay(12)?;
                a.bounded(values.len())?;
                a.map_iter(values.len())?;
                values.visit(&mut |id, value| {
                    a.pay(12)?;
                    validate_value_audited(value, &validator.field(change.entity(), id)?.value_type, 0, a)?;
                    validate_references_audited(value, &records, a)
                })
            };
            if let Some(before) = change.before() { validate_values(before)?; }
            validate_values(change.after())?;
        }
    }
    capture_canonical::data_serialized_bound_audited(snapshot, a)
}
