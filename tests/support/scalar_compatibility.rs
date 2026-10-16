//! Reproduction uses only the public fictional scope fixture and production APIs.
//! Normal compatibility tests read literal fixtures and never call `generate`.
use crate::fixture::*;
use crate::product_backup::VerifiedBackup;
use crate::product_contract::*;
use crate::product_decisions::*;
use crate::product_runtime::LocalRuntime;
use crate::product_store::{scope::*, ProductStore, ProjectSnapshot};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub fn sha256(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// Every nested digest is inventoried by its exact JSON pointer, including
// source/program/semantic, birth basis, scene, mapping, operation and receipt
// links. Whole-payload hashes additionally freeze non-digest fields/provenance.
fn digest_paths(value: &Value, path: &str, found: &mut BTreeMap<String, String>) {
    match value {
        Value::String(s) if s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()) => {
            found.insert(path.into(), s.clone());
        }
        Value::Object(map) => {
            for (k, v) in map {
                let key = k.replace('~', "~0").replace('/', "~1");
                digest_paths(v, &format!("{path}/{key}"), found);
            }
        }
        Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                digest_paths(v, &format!("{path}/{i}"), found);
            }
        }
        _ => (),
    }
}

pub fn inventory(name: &str, bytes: &[u8]) -> Value {
    let envelope: Value = serde_json::from_slice(bytes).unwrap();
    let snapshot = &envelope["payload"]["snapshot"];
    let bundle = &envelope["payload"]["intentions"];
    let mut links = BTreeMap::new();
    digest_paths(snapshot, "", &mut links);
    let mut objects = vec![];
    for entry in bundle["objects"].as_array().unwrap() {
        let bytes: Vec<u8> = serde_json::from_value(entry["bytes"].clone()).unwrap();
        let object: Value = serde_json::from_slice(&bytes).unwrap();
        let mut identities = BTreeMap::new();
        digest_paths(&object, "", &mut identities);
        objects.push(json!({"digest":entry["digest"], "sha256":sha256(&bytes),
            "bytes":bytes.len(), "kind":object["content"]["kind"], "identities":identities}));
    }
    let mut proofs = BTreeMap::new();
    for (category, values) in snapshot["scope"].as_object().into_iter().flatten() {
        if category == "version" {
            continue;
        }
        proofs.insert(
            category.clone(),
            canonical_digest(IdentityDomain::Adoption, values).unwrap(),
        );
    }
    let sources = snapshot["programs"].as_array().unwrap().iter().map(|source| {
        let raw: Vec<u8> = serde_json::from_value(source["source_bytes"].clone()).unwrap();
        json!({"capture":canonical_digest(IdentityDomain::Source, source).unwrap(),
            "raw_sha256":sha256(&raw), "artifact":source["artifact"], "producer":source["binding"]["producer"]})
    }).collect::<Vec<_>>();
    let mut individual_proofs = BTreeMap::new();
    if let Some(scope) = snapshot.get("scope") {
        for category in [
            "layers",
            "compositions",
            "initializations",
            "adoptions",
            "rehearsals",
            "correspondences",
        ] {
            let value = &scope[category];
            let entries: Vec<(String, &Value)> = match value {
                Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v)).collect(),
                Value::Array(list) => list
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i.to_string(), v))
                    .collect(),
                _ => panic!("missing scalar inventory"),
            };
            for (key, proof) in entries {
                individual_proofs.insert(format!("{category}/{key}"), json!({
                    "payload_sha256":sha256(&canonical_bytes(proof).unwrap()),
                    "identity":match category {
                        "layers" | "compositions" | "initializations" => serde_json::to_value(canonical_digest(IdentityDomain::Adoption, proof).unwrap()).unwrap(),
                        "correspondences" => serde_json::to_value(canonical_digest(IdentityDomain::Scenario, &proof["proof"]).unwrap()).unwrap(),
                        _ => Value::Null,
                    }
                }));
            }
        }
    }
    json!({"name":name, "file_sha256":sha256(bytes), "bytes":bytes.len(),
        "snapshot":canonical_digest(IdentityDomain::Data, snapshot).unwrap(),
        "snapshot_sha256":sha256(&canonical_bytes(snapshot).unwrap()),
        "scope_payloads":proofs, "proofs":individual_proofs, "sources":sources, "identities":links,
        "bundle":canonical_digest(IdentityDomain::Evidence, bundle).unwrap(),
        "bundle_sha256":sha256(&canonical_bytes(bundle).unwrap()),
        "backup":envelope["digest"], "objects":objects,
        "checkpoint_name":format!("checkpoint-{:020}-{}.gmbak",snapshot["revision"].as_u64().unwrap(),envelope["digest"].as_str().unwrap())})
}

fn scenario(
    snapshot: &ProjectSnapshot,
    target: &CapturedProgram,
    seed: &DataSnapshot,
) -> ScenarioSpec {
    ScenarioSpec {
        version: 1,
        id: "frozen-scene".into(),
        label: "Fictional waiting work".into(),
        seed: seed.clone(),
        session: SessionState::initial(&target.program).unwrap(),
        clock_day: snapshot.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    }
}

fn ordinary_promise(store: &ProductStore, engine: &DecisionEngine<LocalRuntime>) {
    let s = store.load().unwrap();
    let scene = scenario(&s, s.program().unwrap(), &s.data);
    let accepted = engine
        .accept_current_scene(&s, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "ordinary-promise".into(),
        request: "Keep the original promised date".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::KeepCurrent,
        obligations: vec![AcceptedProperty {
            id: "promised-date".into(),
            description: "The promised date stays fixed".into(),
            predicate: PropertyPredicate::Equal {
                left: PropertyTerm::OutputColumn {
                    point: "result".into(),
                    output: "sheet".into(),
                    column: "promised".into(),
                    value_type: Type::Date,
                },
                right: PropertyTerm::Literal {
                    value_type: Type::list(Type::Date),
                    value: DataValue::List {
                        item_type: Type::Date,
                        items: vec![DataValue::Date { days: 20020 }],
                    },
                },
            },
        }],
        binding: IntentionBinding::PropertiesOnly,
    };
    let change = engine
        .prepare_choice(
            store,
            s.program().unwrap(),
            choice,
            vec![accepted],
            "record-ordinary",
        )
        .unwrap();
    engine.adopt(store, &change).unwrap();
}

fn evolution(store: &ProductStore, value: i64, id: &str) -> PreparedScopedChange {
    let s = store.load().unwrap();
    let mut raw = serde_json::to_value(program(true).program).unwrap();
    raw["observables"][0]["value"] = serde_json::to_value(int(value)).unwrap();
    let candidate = capture(raw);
    let editable = s.editable_scope_context().unwrap().unwrap();
    let mappings = editable
        .slots
        .iter()
        .map(|slot| ScopeSlotMapping {
            layer: slot.layer.clone(),
            patch: slot.patch,
            from_source: s.active_revision.clone(),
            from: slot.destination.clone(),
            to_source: canonical_digest(IdentityDomain::Source, &candidate).unwrap(),
            to: slot.destination.clone(),
            subject: slot.subject.clone(),
        })
        .collect::<Vec<_>>();
    store
        .prepare_managed_evolution(&candidate, &mappings, id)
        .unwrap()
}

pub fn generate(out: &Path) {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("main"), &program(false), 20000).unwrap();
    let job = add(&store, "job", "Fictional customer work");
    action(&store, "wait", "wait", &job);
    tick(&store, "days", 20003);
    let engine = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()));
    ordinary_promise(&store, &engine);
    let mut cases = vec![];
    let mut save = |name: &str, store: &ProductStore| {
        let bytes = VerifiedBackup::capture(store).unwrap().to_bytes().unwrap();
        fs::write(out.join(format!("{name}.gmbak")), &bytes).unwrap();
        cases.push(inventory(name, &bytes));
    };
    // Freeze a literal V1 input while the pre-migration implementation is pinned.
    // The committed fixture is subsequently read directly, never derived from
    // the current serializer in the normal compatibility tests.
    let mut legacy: Value =
        serde_json::from_slice(&VerifiedBackup::capture(&store).unwrap().to_bytes().unwrap())
            .unwrap();
    legacy["payload"]["snapshot"]
        .as_object_mut()
        .unwrap()
        .remove("scope");
    legacy["payload"]["snapshot"]["version"] = json!(1);
    legacy["payload"]["intentions"]["snapshot"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Data, &legacy["payload"]["snapshot"]).unwrap(),
    )
    .unwrap();
    legacy["digest"] = serde_json::to_value(
        canonical_digest(IdentityDomain::Evidence, &legacy["payload"]).unwrap(),
    )
    .unwrap();
    let legacy = canonical_bytes(&legacy).unwrap();
    fs::write(out.join("legacy-v1.gmbak"), &legacy).unwrap();

    let s = store.load().unwrap();
    let prepared = store
        .prepare_scoped_change(
            &program(true),
            &request(&s, ScopePopulation::FutureWork),
            "first-layer",
        )
        .unwrap();
    let layer = prepared.layer_id().unwrap().unwrap();
    let change = engine
        .prepare_managed_change(&store, prepared, &[], "first-layer")
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    let later = add(&store, "later", "Fictional future work");
    action(&store, "later-wait", "wait", &later);
    tick(&store, "later-days", 20005);
    action(&store, "later-complete", "complete", &later);
    apply(&store, "saved-export", invoke("export", &[]));
    save("active", &store);
    let prepared = evolution(&store, 1, "evolve");
    let change = engine
        .prepare_managed_change(&store, prepared, &[], "evolve")
        .unwrap();
    engine.adopt(&store, &change).unwrap();
    save("evolved", &store);
    let withdrawal = engine
        .prepare_scoped_withdrawal(&store, &[layer], "withdraw")
        .unwrap();
    engine.adopt(&store, &withdrawal).unwrap();
    save("withdrawn", &store);

    // A separate minimal lineage avoids copying the ordinary-decision package
    // and every historical mapping into both prospective alternatives.
    let pair_store = ProductStore::create(dir.path().join("pair"), &program(false), 20000).unwrap();
    let before = pair_store.load().unwrap();
    let first = pair_store
        .prepare_scoped_change(
            &program(true),
            &request(&before, ScopePopulation::All),
            "pair-layer",
        )
        .unwrap();
    pair_store.adopt_scoped(before.revision, &first).unwrap();
    add(&pair_store, "pair-job", "Fictional paired work");
    let before_pair = pair_store.load().unwrap();
    let pair_engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(pair_store.clone()),
    );
    let a = evolution(&pair_store, 2, "alternative-a");
    let b = evolution(&pair_store, 3, "alternative-b");
    let scene = scenario(&before_pair, a.target(), &before_pair.data);
    let scenes = pair_engine
        .accept_paired_scoped_scenes(&pair_store, &a, &b, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "pending-pair".into(),
        request: "Retain both experienced alternatives".into(),
        rationale: None,
        scope: DecisionScope {
            operations: ["export".into()].into_iter().collect(),
            population: Population::All,
            conditions: Values::new(),
            excluded_records: vec![],
            unknowns: vec![],
        },
        outcome: DecisionOutcome::Deferred,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = pair_engine
        .prepare_paired_rehearsed_choice(
            &pair_store,
            a,
            b,
            choice,
            scenes.to_vec(),
            &[],
            "record-pair",
        )
        .unwrap();
    pair_engine.adopt(&pair_store, &change).unwrap();
    save("paired", &pair_store);

    let prospective =
        ProductStore::create(dir.path().join("prospective"), &program(false), 20000).unwrap();
    let job = add(&prospective, "job", "Fictional pending work");
    action(&prospective, "wait", "wait", &job);
    tick(&prospective, "days", 20003);
    let current = prospective.load().unwrap();
    let engine = DecisionEngine::new(
        LocalRuntime::default(),
        IntentArchive::new(prospective.clone()),
    );
    let prepared = prospective
        .prepare_scoped_change(
            &program(true),
            &request(&current, ScopePopulation::All),
            "prospective",
        )
        .unwrap();
    let future = scenario(&current, prepared.target(), prepared.seed());
    let baseline = scenario(&current, current.program().unwrap(), &current.data);
    let a = engine
        .accept_current_scene(&current, &baseline, Disclosure::Synthetic)
        .unwrap();
    let b = engine
        .accept_scoped_scene(&prospective, &prepared, &future, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "pending-initial".into(),
        request: "Retain the first-layer alternatives".into(),
        rationale: None,
        scope: prepared.scope().clone(),
        outcome: DecisionOutcome::BothNeeded,
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = engine
        .prepare_rehearsed_choice(
            &prospective,
            prepared,
            choice,
            vec![a, b],
            &[],
            "record-prospective",
        )
        .unwrap();
    engine.adopt(&prospective, &change).unwrap();
    save("prospective", &prospective);
    cases.push(inventory("legacy-v1", &legacy));
    let manifest = json!({"baseline":"c527ef892ddec9af6ed37431898133ce36054c62","cases":cases});
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}
