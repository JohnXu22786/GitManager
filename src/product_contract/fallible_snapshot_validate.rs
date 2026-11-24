//! Shared Product DataSnapshot semantic validation for the legacy and
//! fallible ordered-owner representations. The validator has one execution
//! path so wire and error precedence cannot drift between the two models.
use super::*;
use crate::product_alloc::{self, ordered::OrderedSet, AllocationContext, AllocationFailure, AllocationSite};
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SnapshotValidationError {
    Contract(ContractError),
    Allocation(AllocationFailure),
}

pub(super) fn validate<T: Snapshot>(snapshot: &T, a: Audit<'_>) -> Result<()> {
    let mut cx = AllocationContext::new();
    match validate_with(snapshot, a, &mut cx) {
        Ok(()) => Ok(()),
        Err(SnapshotValidationError::Contract(error)) => Err(error),
        Err(SnapshotValidationError::Allocation(_)) => Err(ContractError(String::new())),
    }
}

pub(super) fn validate_with<T: Snapshot>(
    snapshot: &T,
    a: Audit<'_>,
    cx: &mut AllocationContext,
) -> std::result::Result<(), SnapshotValidationError> {
    if let Some(failure) = cx.first_failure() {
        return Err(SnapshotValidationError::Allocation(failure));
    }
    validate_inner(snapshot, a, cx).map_err(|error| {
        match cx.first_failure() {
            Some(failure) => SnapshotValidationError::Allocation(failure),
            None => SnapshotValidationError::Contract(error),
        }
    })
}

fn validate_inner<T: Snapshot>(snapshot: &T, a: Audit<'_>, cx: &mut AllocationContext) -> Result<()> {
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
    let mut records = OrderedSet::new();
    a.slice(records_slice.len())?;
    for record in records_slice {
        a.pay(12)?;
        a.check_id(record.id())?;
        a.require(record.revision() > 0, "record revision must be positive")?;
        a.insert::<(&str, &str), ()>(records.len(), a.pair_comparator(record.entity(), record.id())?)?;
        let inserted = records.try_insert((record.entity(), record.id()), cx).map_err(|_| ContractError(String::new()))?;
        a.require(inserted, "duplicate record identity")?;
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
            let mut tuples = OrderedSet::new();
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
                let mut tuple = Vec::new();
                product_alloc::try_reserve(&mut tuple, fields.len(), cx, AllocationSite::Vector)
                    .map_err(|_| ContractError(String::new()))?;
                for field in fields {
                    tuple.push(record.values().get(field).unwrap_or(&null));
                }
                let mut has_null = false;
                a.pay(1)?;
                for value in &tuple { a.pay(12)?; a.pay(4)?; if matches!(value, DataValue::Null) { has_null = true; break; } }
                a.pay(1)?;
                if has_null { continue; }
                let bytes = if a.enabled() {
                    capture_canonical::canonical_bytes_audited(&tuple, a)?
                } else {
                    crate::product_backup::fallible_codec::canonical_bytes(&DataValueTuple(&tuple), cx)
                        .map_err(|_| ContractError(String::new()))?
                };
                a.insert::<Vec<u8>, ()>(tuples.len(), a.comparator(bytes.len())?)?;
                let inserted = tuples.try_insert(bytes, cx).map_err(|_| ContractError(String::new()))?;
                a.require(inserted, "duplicate unique field tuple")?;
            }
        }
    }
    a.pay(17)?;
    let mut event_ids = OrderedSet::new();
    let mut operations = OrderedSet::new();
    let mut previous = 0;
    a.slice(events_slice.len())?;
    for event in events_slice {
        a.pay(12)?;
        a.check_id(event.id())?;
        a.check_id(event.operation())?;
        a.check_id(event.action())?;
        a.day(event.day())?;
        a.insert::<&String, ()>(event_ids.len(), a.comparator(event.id().len())?)?;
        let new_event = event_ids.try_insert(event.id(), cx).map_err(|_| ContractError(String::new()))?;
        let distinct = if new_event {
            a.insert::<&String, ()>(operations.len(), a.comparator(event.operation().len())?)?;
            operations.try_insert(event.operation(), cx).map_err(|_| ContractError(String::new()))?
        } else { false };
        a.require(distinct, "duplicate event or operation receipt")?;
        a.require(event.sequence() > previous && event.sequence() <= snapshot.generation(),
            "event sequence is not ordered within generation")?;
        previous = event.sequence();
        let changes = event.changes();
        a.bounded(changes.len())?;
        a.bounded(event.outputs_len())?;
        a.pay(8)?;
        let mut changed = OrderedSet::new();
        a.slice(changes.len())?;
        for change in changes {
            a.pay(12)?;
            a.check_id(change.record())?;
            a.insert::<(&String, &String), ()>(changed.len(), a.pair_comparator(change.entity(), change.record())?)?;
            let inserted = changed.try_insert((change.entity(), change.record()), cx).map_err(|_| ContractError(String::new()))?;
            a.require(inserted, "duplicate event record change")?;
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

pub(crate) struct DataValueTuple<'a>(pub(crate) &'a [&'a DataValue]);
impl Serialize for DataValueTuple<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 { sequence.serialize_element(value)?; }
        sequence.end()
    }
}
