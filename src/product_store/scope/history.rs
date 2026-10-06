use super::*;
use compiler::*;

pub(super) fn row_ref(row: &Record) -> RecordRef {
    RecordRef {
        entity: row.entity.clone(),
        record: row.id.clone(),
    }
}
pub(super) fn initial_member(layer: &ScopeLayer, row: &RecordRef) -> bool {
    !layer.request.excluded_records.contains(row)
        && match &layer.request.population {
            ScopePopulation::All => true,
            ScopePopulation::FutureWork => false,
            ScopePopulation::SelectedUnfinished { records } => records.contains(row),
        }
}
pub(super) fn completed(
    program: &CapturedProgram,
    data: &DataSnapshot,
    row: &RecordRef,
    lifecycle: &LifecycleBinding,
    day: i32,
) -> Result<bool> {
    let value = LocalRuntime::default().evaluate_record_projection(
        program,
        data,
        row,
        "record",
        &lifecycle.completed,
        &Type::Boolean,
        day,
    )?;
    if let DataValue::Boolean { value } = value {
        Ok(value)
    } else {
        Err(error("completion predicate is not Boolean"))
    }
}
pub(super) fn initialize(
    snapshot: &ProjectSnapshot,
    layer: &ScopeLayer,
    target: &CapturedProgram,
) -> Result<(DataSnapshot, MetadataInitializationReceipt)> {
    let id = layer.identity()?;
    let before = &layer.basis.data;
    let mut after = merged_data(target, before)?;
    let baseline = program(snapshot, &layer.basis.active)?;
    let mut baseline_app =
        if let Some(manifest) = snapshot.scope.compositions.get(&layer.basis.active) {
            envelope_app(snapshot, manifest)?
        } else {
            baseline.program.clone()
        };
    let mut additions = vec![];
    for row in &before.records {
        let Some(lifecycle) = layer
            .request
            .lifecycles
            .iter()
            .find(|l| l.entity == row.entity)
        else {
            continue;
        };
        let reference = row_ref(row);
        let sealed =
            row.archived || completed(baseline, before, &reference, lifecycle, layer.basis.day)?;
        let mut values = Values::from([
            (
                key(&id, &row.entity, "member"),
                DataValue::Boolean {
                    value: initial_member(layer, &reference),
                },
            ),
            (
                key(&id, &row.entity, "sealed"),
                DataValue::Boolean { value: sealed },
            ),
        ]);
        if sealed {
            values.insert(
                key(&id, &row.entity, "day"),
                DataValue::Date {
                    days: layer.basis.day,
                },
            );
            values.insert(
                key(&id, &row.entity, "origin"),
                DataValue::Text {
                    value: "CapturedAtAdoption".into(),
                },
            );
            for patch in &layer.patches {
                if patch.request.entity == row.entity && projection(&patch.request.destination) {
                    let expression = slot(&mut baseline_app, &patch.request.destination)?.clone();
                    let value = LocalRuntime::default().evaluate_record_projection(
                        baseline,
                        before,
                        &reference,
                        &patch.request.subject,
                        &expression,
                        &patch.request.value_type,
                        layer.basis.day,
                    )?;
                    values.insert(saved_key(&id, patch)?, value);
                }
            }
        }
        if values.keys().any(|k| row.values.contains_key(k)) {
            return Err(error("new system field collides with stored data"));
        }
        let changed = after
            .records
            .iter_mut()
            .find(|r| r.entity == row.entity && r.id == row.id)
            .ok_or_else(|| error("initialization lost a record"))?;
        changed.values.extend(values.clone());
        additions.push(MetadataAddition {
            record: reference,
            values,
        });
    }
    after.validate()?;
    let mut business = after.records.clone();
    for addition in &additions {
        let record = business
            .iter_mut()
            .find(|r| row_ref(r) == addition.record)
            .ok_or_else(|| error("metadata record missing"))?;
        for key in addition.values.keys() {
            record.values.remove(key);
        }
    }
    if business != before.records
        || after.events != before.events
        || after.generation != before.generation
    {
        return Err(error("initialization changed existing business facts"));
    }
    let receipt = MetadataInitializationReceipt {
        operation: layer.operation.clone(),
        layer: id,
        before_data: before.identity()?,
        after_data: after.identity()?,
        before_schema: before.schema_identity()?,
        after_schema: after.schema_identity()?,
        additions,
        business_projection: canonical_digest(IdentityDomain::Data, &business)?,
        original_events: canonical_digest(IdentityDomain::Data, &before.events)?,
        original_records: before.records.len(),
        capture_day: layer.basis.day,
        capture_program: baseline.artifact.program_digest.clone(),
        prior_snapshot: layer.basis.snapshot.clone(),
        adoption_revision: layer
            .basis
            .revision
            .checked_add(1)
            .ok_or_else(|| error("revision exhausted"))?,
    };
    Ok((after, receipt))
}
fn value_bool(values: &Values, key: &str) -> Result<bool> {
    match values.get(key) {
        None | Some(DataValue::Null) => Ok(false),
        Some(DataValue::Boolean { value }) => Ok(*value),
        _ => Err(error("damaged protected Boolean")),
    }
}
fn metadata_keys(id: &Digest, layer: &ScopeLayer, entity: &str) -> Result<Vec<Id>> {
    let mut keys = vec![
        key(id, entity, "member"),
        key(id, entity, "sealed"),
        key(id, entity, "day"),
        key(id, entity, "origin"),
    ];
    for patch in &layer.patches {
        if patch.request.entity == entity && projection(&patch.request.destination) {
            keys.push(saved_key(id, patch)?);
        }
    }
    Ok(keys)
}
fn metadata(values: &Values, keys: &[Id]) -> Values {
    keys.iter()
        .filter_map(|k| values.get(k).map(|v| (k.clone(), v.clone())))
        .collect()
}
fn producer_manifest<'a>(
    snapshot: &'a ProjectSnapshot,
    digest: &Digest,
) -> Option<&'a CompositionManifest> {
    snapshot
        .programs
        .iter()
        .find(|p| &p.artifact.program_digest == digest)
        .and_then(|p| super::super::revision(p).ok())
        .and_then(|id| snapshot.scope.compositions.get(&id))
}
/// Reconstruct every protected cell from its initialization or real creation
/// event, then enforce monotonic seals against the actual append-only events.
/// No missing value is repaired by evaluating today's changed formula.
pub(super) fn verify_history(snapshot: &ProjectSnapshot) -> Result<()> {
    for (id, layer) in &snapshot.scope.layers {
        let receipt = snapshot
            .scope
            .initializations
            .iter()
            .find(|r| &r.layer == id)
            .ok_or_else(|| error("initialization receipt missing"))?;
        if !snapshot.data.events.starts_with(&layer.basis.data.events) {
            return Err(error("pre-adoption business event history changed"));
        }
        for lifecycle in &layer.request.lifecycles {
            for row in snapshot
                .data
                .records
                .iter()
                .filter(|r| r.entity == lifecycle.entity)
            {
                let reference = row_ref(row);
                let keys = metadata_keys(id, layer, &row.entity)?;
                let member = key(id, &row.entity, "member");
                let sealed = key(id, &row.entity, "sealed");
                let before_row = layer
                    .basis
                    .data
                    .records
                    .iter()
                    .find(|r| row_ref(r) == reference);
                if let Some(original) = before_row {
                    if original.created_program != row.created_program
                        || row.revision < original.revision
                        || (original.archived
                            && (!row.archived || row.revision != original.revision))
                    {
                        return Err(error(
                            "existing record producer, revision or archive identity changed",
                        ));
                    }
                }
                let initialization = receipt.additions.iter().find(|a| a.record == reference);
                let mut expected = if before_row.is_some() {
                    initialization
                        .ok_or_else(|| error("existing row has no metadata receipt"))?
                        .values
                        .clone()
                } else {
                    Values::new()
                };
                let business_values = |values: &Values| -> Values {
                    values
                        .iter()
                        .filter(|(key, _)| !key.starts_with(RESERVED_PREFIX))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                };
                let mut business = before_row.map(|r| business_values(&r.values));
                let mut archived = before_row.is_some_and(|r| r.archived);
                let mut births = 0;
                for event in snapshot
                    .data
                    .events
                    .iter()
                    .filter(|e| e.sequence > layer.basis.data.generation)
                {
                    let Some(change) = event
                        .changes
                        .iter()
                        .find(|c| c.entity == row.entity && c.record == row.id)
                    else {
                        continue;
                    };
                    if archived || change.before.as_ref().map(&business_values) != business {
                        return Err(error("business before-values or archived facts changed outside recorded history"));
                    }
                    business = Some(business_values(&change.after));
                    archived = change.archived;
                    let before = change.before.as_ref();
                    let after = metadata(&change.after, &keys);
                    if before.is_none() {
                        births += 1;
                        if row.created_program != event.program {
                            return Err(error("record producer differs from its real birth event"));
                        }
                        if before_row.is_some() || births != 1 {
                            return Err(error("duplicate or postdated creation provenance"));
                        }
                        let source = producer_manifest(snapshot, &event.program)
                            .ok_or_else(|| error("creation has no verified composition"))?;
                        let stamped = source.active.contains(id)
                            && matches!(
                                layer.request.population,
                                ScopePopulation::FutureWork | ScopePopulation::All
                            );
                        expected = Values::from([
                            (member.clone(), DataValue::Boolean { value: stamped }),
                            (sealed.clone(), DataValue::Boolean { value: false }),
                        ]);
                        if value_bool(&after, &member)? != stamped {
                            return Err(error("birth cohort does not match its producing source"));
                        }
                    } else if metadata(before.unwrap(), &keys) != expected {
                        return Err(error("event before-value breaks protected history"));
                    }
                    if value_bool(&after, &member)? != value_bool(&expected, &member)? {
                        return Err(error("cohort membership changed after creation"));
                    }
                    if value_bool(&expected, &sealed)? {
                        if after != expected {
                            return Err(error("completed result seal was modified"));
                        }
                    } else if value_bool(&after, &sealed)? {
                        let source = producer_manifest(snapshot, &event.program)
                            .ok_or_else(|| error("seal source is not verified"))?;
                        if !source.layers.contains(id) {
                            return Err(error("seal source omitted its history envelope"));
                        }
                        if after.get(&key(id, &row.entity, "day"))
                            != Some(&DataValue::Date { days: event.day })
                        {
                            return Err(error("seal day differs from business event"));
                        }
                        let origin = after.get(&key(id, &row.entity, "origin"));
                        let archive = origin
                            == Some(&DataValue::Text {
                                value: "ObservedAtArchive".into(),
                            });
                        if !archive
                            && origin
                                != Some(&DataValue::Text {
                                    value: "ObservedAtCompletion".into(),
                                })
                        {
                            return Err(error("invalid completion provenance"));
                        }
                        let producer = program(snapshot, &source.output)?;
                        let mut data = snapshot.data.clone();
                        let r = data
                            .records
                            .iter_mut()
                            .find(|r| row_ref(r) == reference)
                            .unwrap();
                        r.values = change.after.clone();
                        r.archived = change.archived;
                        if !(archive && change.archived)
                            && !completed(producer, &data, &reference, lifecycle, event.day)?
                        {
                            return Err(error("result sealed without a real terminal transition"));
                        }
                        // Re-evaluate the actual producer's final live projection
                        // with every seal born in this event unset. Resetting only
                        // one would let two forged saved values justify each other.
                        for other in &source.layers {
                            let other_layer = &snapshot.scope.layers[other];
                            if !other_layer
                                .request
                                .lifecycles
                                .iter()
                                .any(|l| l.entity == row.entity)
                            {
                                continue;
                            }
                            let other_sealed = key(other, &row.entity, "sealed");
                            if !before
                                .map(|v| value_bool(v, &other_sealed))
                                .transpose()?
                                .unwrap_or(false)
                            {
                                let r = data
                                    .records
                                    .iter_mut()
                                    .find(|r| row_ref(r) == reference)
                                    .unwrap();
                                for key in metadata_keys(other, other_layer, &row.entity)? {
                                    if key != compiler::key(other, &row.entity, "member") {
                                        r.values.remove(&key);
                                    }
                                }
                                r.values
                                    .insert(other_sealed, DataValue::Boolean { value: false });
                            }
                        }
                        let mut producer_app = envelope_app(snapshot, source)?;
                        // Each saved value must be well typed, including archived
                        // rows which ordinary active constraints intentionally skip.
                        for (index, original) in layer.patches.iter().enumerate() {
                            let patch = effective_patch(snapshot, source, id, index)?;
                            if patch.request.entity == row.entity
                                && projection(&patch.request.destination)
                            {
                                let saved = saved_key(id, original)?;
                                let value = after
                                    .get(&saved)
                                    .ok_or_else(|| error("sealed result is missing"))?;
                                validate_value(value, &patch.request.value_type, 0)?;
                                let expression =
                                    slot(&mut producer_app, &patch.request.destination)?.clone();
                                let observed = LocalRuntime::default().evaluate_record_projection(
                                    producer,
                                    &data,
                                    &reference,
                                    &patch.request.subject,
                                    &expression,
                                    &patch.request.value_type,
                                    event.day,
                                )?;
                                if *value != observed {
                                    return Err(error(
                                        "saved result differs from its actual producing expression",
                                    ));
                                }
                            }
                        }
                    } else if after != expected {
                        return Err(error("unsealed history metadata changed"));
                    }
                    expected = after;
                }
                if before_row.is_none() && births != 1 {
                    return Err(error("future record lacks exactly one birth event"));
                }
                if business.as_ref() != Some(&business_values(&row.values))
                    || archived != row.archived
                {
                    return Err(error("current business record differs from recorded facts"));
                }
                if metadata(&row.values, &keys) != expected {
                    return Err(error(
                        "current metadata is not accounted for by real history",
                    ));
                }
                let is_sealed = value_bool(&expected, &sealed)?;
                if is_sealed
                    && !row.archived
                    && !completed(
                        snapshot.program()?,
                        &snapshot.data,
                        &reference,
                        lifecycle,
                        snapshot.clock_day,
                    )?
                {
                    return Err(error("terminal completed work cannot be silently reopened"));
                }
                if row.archived && !is_sealed {
                    return Err(error("archived result was not preserved"));
                }
                if is_sealed {
                    for patch in &layer.patches {
                        if patch.request.entity == row.entity
                            && projection(&patch.request.destination)
                        {
                            let value = expected
                                .get(&saved_key(id, patch)?)
                                .ok_or_else(|| error("sealed projection is missing"))?;
                            validate_value(value, &patch.request.value_type, 0)?;
                        }
                    }
                }
            }
        }
        for original in &layer.basis.data.records {
            if !snapshot
                .data
                .records
                .iter()
                .any(|r| row_ref(r) == row_ref(original))
            {
                return Err(error("original record identity disappeared"));
            }
        }
    }
    Ok(())
}
