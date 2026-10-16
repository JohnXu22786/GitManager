//! Exact development-request codec regressions; synthetic provider only.
#[path = "../src/product_studio/change_adapter.rs"]
mod change_adapter;
#[path = "../src/product_studio/discovery_flow.rs"]
mod discovery_flow;
#[path = "fixtures/product_scope/mod.rs"]
mod fixture;
#[path = "../src/product_backup.rs"]
mod product_backup;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_decisions/mod.rs"]
mod product_decisions;
#[path = "../src/product_discovery/mod.rs"]
mod product_discovery;
#[path = "../src/product_locations.rs"]
mod product_locations;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_provider/mod.rs"]
mod product_provider;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/product_scenarios/mod.rs"]
mod product_scenarios;
#[path = "../src/product_store/mod.rs"]
mod product_store;
#[path = "../src/product_studio/rule_discovery.rs"]
mod rule_discovery;
use fixture::*;
use product_contract::*;
use product_decisions::*;
use product_discovery::*;
use product_runtime::LocalRuntime;
use product_store::{scope::*, ProductStore};
use rule_discovery::*;
use std::sync::{atomic::AtomicBool, Arc};

fn cancel() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}
fn engine(store: &ProductStore) -> DecisionEngine<LocalRuntime> {
    DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
}
const ORIGINAL_NEED: &str = "Change how waiting contributes to production; preserve commitments";

fn modify(
    store: &ProductStore,
    candidate: &CapturedProgram,
    id: &str,
) -> (DevelopmentRequest, DevelopmentResult) {
    let current = store.load().unwrap();
    let req = engine(store)
        .development_request(
            &current,
            &format!("modify-{id}"),
            DevelopmentOperation::Modify,
            ORIGINAL_NEED,
            DevelopmentContext {
                view: Some("work".into()),
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(current.data.identity().unwrap()),
                session_digest: Some(current.session.identity().unwrap()),
            },
        )
        .unwrap();
    let result = DevelopmentResult {
        producer: candidate.binding.producer.clone(),
        response: DevelopmentResponse {
            version: 1,
            request_digest: req.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "changed".into(),
                source_json: String::from_utf8(candidate.source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    (req, result)
}
fn draft_from(
    store: &ProductStore,
    req: DevelopmentRequest,
    result: DevelopmentResult,
    population: ScopePopulation,
    id: &str,
) -> RuleDiscoveryDraft {
    let snapshot = store.load().unwrap();
    let source = CapturedProgram::capture(
        result.response.candidates[0].source_json.as_bytes(),
        &req.project_id,
        result.producer.clone(),
        None,
    )
    .unwrap();
    let scope = change_adapter::request(
        &snapshot,
        &source,
        population,
        fixture::request(&snapshot, ScopePopulation::All).lifecycles,
        Default::default(),
    )
    .unwrap();
    let prepared = store.prepare_scoped_change(&source, &scope, id).unwrap();
    let selection =
        RuleSelection::checked(store, &snapshot, prepared, scope, id, cancel()).unwrap();
    RuleDiscoveryDraft::after_modify(
        store,
        &snapshot,
        req,
        result,
        "changed",
        selection,
        &format!("discover-{id}"),
        ORIGINAL_NEED,
        cancel(),
    )
    .unwrap()
}
fn compact_program(pause: bool) -> CapturedProgram {
    // A stored production result is the sole changed semantic slot. Completion
    // preserves that result; both the view and export expose it unchanged.
    let mut raw = serde_json::to_value(program(pause).program).unwrap();
    raw["actions"][4]["steps"][0]["values"]
        .as_object_mut()
        .unwrap()
        .remove("production");
    raw["actions"][6]["steps"][0]["columns"]["production"] =
        serde_json::to_value(field("row", "production")).unwrap();
    raw["views"][0]["kind"]["columns"][1]["value"] =
        serde_json::to_value(field("row", "production")).unwrap();
    capture(raw)
}
fn managed(store: &ProductStore, compact: bool) -> (Record, Record, CapturedProgram) {
    let selected = add(store, "selected", "Selected commitment");
    let archived = add(store, "archived", "Completed history");
    action(store, "finish-old", "complete", &archived);
    action(store, "archive-old", "archive", &archived);
    action(store, "wait-selected", "wait", &selected);
    tick(store, "day", 20003);
    let current = store.load().unwrap();
    let authored = if compact {
        compact_program(true)
    } else {
        program(true)
    };
    let population = ScopePopulation::SelectedUnfinished {
        records: vec![RecordRef {
            entity: selected.entity.clone(),
            record: selected.id.clone(),
        }],
    };
    let scope = change_adapter::request(
        &current,
        &authored,
        population,
        fixture::request(&current, ScopePopulation::All).lifecycles,
        Default::default(),
    )
    .unwrap();
    let first = store
        .prepare_scoped_change(&authored, &scope, "selected-rule")
        .unwrap();
    let scene = ScenarioSpec {
        version: 1,
        id: "selected-promise".into(),
        label: "Keep selected timing and commitment".into(),
        seed: first.seed().clone(),
        session: current.session.clone(),
        clock_day: current.clock_day,
        random_seed: 42,
        inputs: vec![
            invoke("calculate", &[("row", reference(&selected))]),
            invoke("complete", &[("row", reference(&selected))]),
            invoke("export", &[]),
            SemanticInput::Observe {
                point: "result".into(),
            },
        ],
        validity: vec![],
    };
    let e = engine(store);
    let accepted = e
        .accept_scoped_scene(store, &first, &scene, Disclosure::Synthetic)
        .unwrap();
    let choice = Choice {
        id: "selected-promise".into(),
        request: "Keep selected timing, customer commitment and completed history".into(),
        rationale: None,
        scope: first.scope().clone(),
        outcome: DecisionOutcome::Accept {
            artifact: first.target().artifact.program_digest.clone(),
        },
        obligations: vec![],
        binding: IntentionBinding::ObservedOutcome,
    };
    let change = e
        .prepare_scoped_choice(store, first, choice, vec![accepted], "selected-rule")
        .unwrap();
    e.adopt(store, &change).unwrap();
    let mut raw = serde_json::to_value(authored.program).unwrap();
    let pointers: &[&str] = if compact {
        &["/actions/3/steps/0/values/production"]
    } else {
        &[
            "/actions/3/steps/0/values/production",
            "/actions/4/steps/0/values/production",
            "/actions/6/steps/0/columns/production",
            "/views/0/kind/columns/1/value",
        ]
    };
    for pointer in pointers {
        let old = raw.pointer(pointer).unwrap().clone();
        *raw.pointer_mut(pointer).unwrap() =
            serde_json::json!({"kind":"add","left":old,"right":int(1)});
    }
    (selected, archived, capture(raw))
}
#[cfg(unix)]
fn fixture_transport(
    root: &std::path::Path,
    response: &DevelopmentResponse,
) -> product_provider::ProviderTransport {
    use product_provider::{ProviderKind, ProviderTransport};
    use std::{fs, os::unix::fs::PermissionsExt};
    fs::create_dir_all(root).unwrap();
    let bin = root.join("fixture.py");
    let script = include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "{'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
        "cfg['response']",
    );
    {
        let _guard = product_provider::fixture_executable_write_guard();
        fs::write(&bin, script).unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            bin.with_extension("json"),
            serde_json::to_vec(&serde_json::json!({"response":response})).unwrap(),
        )
        .unwrap();
    }
    let home = root.join("home");
    fs::create_dir(&home).unwrap();
    ProviderTransport::new_fixture(root.join("jobs"), ProviderKind::Codex, bin, home).unwrap()
}
#[cfg(unix)]
fn transport(
    root: &std::path::Path,
    request: &DevelopmentRequest,
    result: &DevelopmentResult,
) -> (DevelopmentResult, product_provider::JobReceipt) {
    use product_provider::{unix_ms, ConsentReceipt};
    let t = fixture_transport(root, &result.response);
    let p = prepare_development(t, request, ProviderOptions::default()).unwrap_or_else(|error| {
        panic!(
            "request {} ({} serialized bytes): {error:?}",
            request.id,
            serde_json::to_vec(request).unwrap().len()
        )
    });
    let c = ConsentReceipt {
        disclosure_digest: p.disclosure().digest(),
        approval_reference: "Fictional offline test only".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let provider = p.authorize(c);
    let mut stale = request.clone();
    stale.request.push('!');
    assert!(matches!(
        provider.develop(&stale, &|| false),
        Err(AdapterError::Stale(_))
    ));
    let result = provider.develop(request, &|| false).unwrap();
    let raw = provider.raw_response().unwrap();
    assert_eq!(DevelopmentResponse::parse(&raw).unwrap(), result.response);
    (result, provider.receipt().unwrap())
}

#[cfg(unix)]
#[test]
fn authentic_four_slot_discover_prepares_without_losing_context() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let (_, _, candidate) = managed(&store, false);
    let current = store.load().unwrap();
    let (modify_request, modify_result) = modify(&store, &candidate, "wide-managed");
    let modify_wire = encode_request(&modify_request, &ProviderOptions::default()).unwrap();
    let d = draft_from(
        &store,
        modify_request,
        modify_result,
        ScopePopulation::FutureWork,
        "wide-managed",
    );
    let request = d.request();
    assert_eq!(request.operation, DevelopmentOperation::Discover);
    assert!(!request.accepted_scenes.is_empty());
    assert_eq!(request.decisions, current.decisions);
    let original = serde_json::to_vec(request).unwrap();
    let legacy = legacy_prompt(request);
    let wire = encode_request(request, &ProviderOptions::default()).unwrap();
    let prompt: serde_json::Value = serde_json::from_slice(&wire.prompt).unwrap();
    let restored = parse_codec(&prompt["request"])
        .unwrap()
        .to_request()
        .unwrap();
    assert_eq!(&restored, request);
    assert_eq!(serde_json::to_vec(&restored).unwrap(), original);
    assert_eq!(
        prompt["request_digest"],
        serde_json::to_value(request.identity().unwrap()).unwrap()
    );
    assert!(legacy.len() > product_provider::MAX_PROMPT_BYTES);
    assert!(wire.prompt.len() <= product_provider::MAX_PROMPT_BYTES);
    assert_eq!(
        wire.source_digest,
        request.sources[1].binding.identity().unwrap().as_str()
    );
    eprintln!(
        "authentic complete prompts: legacy={} exact_utf8={}",
        legacy.len(),
        wire.prompt.len()
    );
    assert!(original.len() > product_provider::MAX_PROMPT_BYTES);
    eprintln!(
        "authentic four-slot domain={} preceding_modify_prompt={} cap={}",
        original.len(),
        modify_wire.prompt.len(),
        product_provider::MAX_PROMPT_BYTES
    );
    for (index, source) in request.sources.iter().enumerate() {
        eprintln!(
            "source {index}: raw={} decimal_array={} parsed_ast={} utf8_string={}",
            source.source_bytes.len(),
            serde_json::to_vec(&source.source_bytes).unwrap().len(),
            serde_json::to_vec(&source.program).unwrap().len(),
            serde_json::to_vec(std::str::from_utf8(&source.source_bytes).unwrap())
                .unwrap()
                .len()
        );
    }
    if let Ok(path) = std::env::var("CODEC_EVIDENCE_DIR") {
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(
            std::path::Path::new(&path).join("authentic-request.json"),
            &original,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&path).join("legacy-prompt.json"),
            &legacy,
        )
        .unwrap();
        std::fs::write(
            std::path::Path::new(&path).join("exact-utf8-prompt.json"),
            &wire.prompt,
        )
        .unwrap();
    }
    let expected = DevelopmentResult {
        producer: Producer::Fixture {
            name: "Synthetic correlated discover".into(),
        },
        response: DevelopmentResponse {
            version: 1,
            request_digest: request.identity().unwrap(),
            candidates: vec![GeneratedCandidate {
                id: "after".into(),
                source_json: String::from_utf8(request.sources[1].source_bytes.clone()).unwrap(),
            }],
            hypotheses: vec![],
            evolutions: vec![],
            unsupported: vec![],
        },
    };
    let (actual, receipt) = transport(&dir.path().join("discover"), request, &expected);
    actual.validate_for(request).unwrap();
    assert_eq!(actual.response, expected.response);
    assert_eq!(
        receipt.state,
        product_provider::JobState::TransportValidated
    );
    assert_eq!(
        receipt.disclosure.prompt_digest,
        product_provider::digest(&wire.prompt)
    );
    assert_eq!(receipt.disclosure.request_digest, wire.digest().unwrap());
    assert_eq!(receipt.disclosure.source_digest, wire.source_digest);
    assert_eq!(
        receipt.provenance,
        product_provider::InvocationProvenance::TransportFixture
    );
    assert_eq!(store.load().unwrap(), current);
}

fn simple_request() -> DevelopmentRequest {
    DevelopmentRequest {
        version: 1,
        id: "codec-request".into(),
        project_id: "scope-project".into(),
        operation: DevelopmentOperation::Discover,
        request: "Preserve exact source and selected context".into(),
        sources: vec![program(false), program(true)],
        context: DevelopmentContext {
            view: Some("work".into()),
            selected: vec![],
            recent_inputs: vec![],
            data_digest: None,
            session_digest: None,
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: DecisionGraph {
            version: 1,
            revision: 0,
            decisions: vec![],
        },
        unknowns: vec![UnknownBoundary {
            id: "unknown".into(),
            operations: ["calculate".into()].into(),
            description: "Unresolved waiting behavior".into(),
        }],
        required_capabilities: ["local-runtime".into()].into(),
    }
}
fn encoded_value(request: &DevelopmentRequest) -> serde_json::Value {
    serde_json::to_value(ExactUtf8DevelopmentRequest::from_request(request).unwrap()).unwrap()
}
fn parse_codec(value: &serde_json::Value) -> Result<ExactUtf8DevelopmentRequest, ContractError> {
    ExactUtf8DevelopmentRequest::parse(&serde_json::to_vec(value).unwrap())
}

#[test]
fn exact_utf8_roundtrip_retains_noncanonical_source_and_domain_identities() {
    let mut request = simple_request();
    for (index, source) in request.sources.iter_mut().enumerate() {
        let mut value = serde_json::to_value(&source.program).unwrap();
        value["label"] = serde_json::json!("Café 中文 \\\" \n\t\u{0}");
        let members: Vec<_> = value
            .as_object()
            .unwrap()
            .iter()
            .rev()
            .map(|(key, value)| {
                format!(
                    "{} : {}",
                    serde_json::to_string(key).unwrap(),
                    serde_json::to_string(value).unwrap()
                )
            })
            .collect();
        let raw = format!("\r\n {{\n {} \n}} \t", members.join(",\n "));
        let raw = if index == 0 {
            raw.replace("Café", "Caf\\u00e9")
        } else {
            raw
        };
        *source = CapturedProgram::capture(
            raw.as_bytes(),
            &request.project_id,
            Producer::ExternalAuthor {
                description: "Exact external UTF-8 capture".into(),
            },
            Some(TaskSource {
                task_id: format!("task-{index}"),
                source_fingerprint: "source fingerprint with spaces".into(),
            }),
        )
        .unwrap()
        .at_path(&format!("captured/{index}/program.json"))
        .unwrap();
        assert_ne!(
            source.source_bytes,
            serde_json::to_vec(&source.program).unwrap()
        );
    }
    let original = serde_json::to_vec(&request).unwrap();
    let identity = request.identity().unwrap();
    let value = encoded_value(&request);
    assert_eq!(
        value["encoding"],
        "gitmanager.development-request.exact-utf8"
    );
    assert_eq!(value["version"], 1);
    assert_eq!(
        value["request_digest"],
        serde_json::to_value(&identity).unwrap()
    );
    for (index, source) in request.sources.iter().enumerate() {
        assert!(value["request"]["sources"][index]
            .get("source_bytes")
            .is_none());
        assert_eq!(
            value["request"]["sources"][index]["source_utf8"]
                .as_str()
                .unwrap()
                .as_bytes(),
            source.source_bytes
        );
        assert_eq!(
            value["request"]["sources"][index]["program"],
            serde_json::to_value(&source.program).unwrap()
        );
    }
    let restored = parse_codec(&value).unwrap().to_request().unwrap();
    assert_eq!(restored, request);
    assert_eq!(serde_json::to_vec(&restored).unwrap(), original);
    assert_eq!(restored.identity().unwrap(), identity);
    for (old, new) in request.sources.iter().zip(&restored.sources) {
        assert_eq!(
            new.binding.identity().unwrap(),
            old.binding.identity().unwrap()
        );
        assert_eq!(new.artifact, old.artifact);
        assert_eq!(
            new.program.identity().unwrap(),
            old.program.identity().unwrap()
        );
        assert_eq!(
            new.program.semantic_identity().unwrap(),
            old.program.semantic_identity().unwrap()
        );
    }
}

#[test]
fn codec_refuses_unknown_duplicate_deep_truncated_and_tampered_input() {
    use serde_json::json;
    let request = simple_request();
    let good = encoded_value(&request);
    let edits = [
        ("/encoding", json!("unknown")),
        ("/version", json!(2)),
        ("/request/version", json!(2)),
        ("/request/project_id", json!("other")),
        ("/request_digest", json!("0".repeat(64))),
        (
            "/request/sources/0/artifact/raw_digest",
            json!("0".repeat(64)),
        ),
        (
            "/request/sources/0/artifact/program_digest",
            json!("0".repeat(64)),
        ),
        (
            "/request/sources/0/artifact/semantic_digest",
            json!("0".repeat(64)),
        ),
        (
            "/request/sources/0/binding/source_digest",
            json!("0".repeat(64)),
        ),
        ("/request/sources/0/binding/project_id", json!("other")),
        ("/request/sources/0/program/label", json!("Mismatched AST")),
        (
            "/request/sources/0/source_utf8",
            json!(String::from_utf8(request.sources[1].source_bytes.clone()).unwrap()),
        ),
        ("/request/request", json!("x".repeat(MAX_TEXT_BYTES + 1))),
    ];
    for (pointer, value) in edits {
        let mut invalid = good.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            parse_codec(&invalid).is_err(),
            "accepted tampering at {pointer}"
        );
    }
    for path in [
        "",
        "/request",
        "/request/sources/0",
        "/request/sources/0/program",
        "/request/context",
    ] {
        let mut invalid = good.clone();
        invalid
            .pointer_mut(path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown-field".into(), json!(true));
        assert!(parse_codec(&invalid).is_err(), "accepted unknown at {path}");
    }
    let mut reordered = good.clone();
    reordered["request"]["sources"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(parse_codec(&reordered).is_err());
    let mut too_many = good.clone();
    too_many["request"]["sources"] = json!(vec![good["request"]["sources"][0].clone(); 9]);
    assert!(parse_codec(&too_many).is_err());
    let bytes = serde_json::to_vec(&good).unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    let duplicate = format!("{{\"version\":1,{}", &text[1..]);
    assert!(ExactUtf8DevelopmentRequest::parse(duplicate.as_bytes())
        .unwrap_err()
        .0
        .contains("duplicate"));
    let nested_duplicate = text.replacen(
        "\"project_id\":",
        "\"project_id\":\"scope-project\",\"project_id\":",
        1,
    );
    assert!(
        ExactUtf8DevelopmentRequest::parse(nested_duplicate.as_bytes())
            .unwrap_err()
            .0
            .contains("duplicate")
    );
    let deep = format!("{}0{}", "[".repeat(65), "]".repeat(65));
    assert!(ExactUtf8DevelopmentRequest::parse(deep.as_bytes())
        .unwrap_err()
        .0
        .contains("nesting"));
    assert!(ExactUtf8DevelopmentRequest::parse(&bytes[..bytes.len() - 1]).is_err());
    assert!(ExactUtf8DevelopmentRequest::parse(format!("{text} null").as_bytes()).is_err());
    let mut invalid_utf8 = bytes;
    invalid_utf8[1] = 0xff;
    assert!(ExactUtf8DevelopmentRequest::parse(&invalid_utf8).is_err());
}

#[test]
fn codec_preserves_source_validation_and_exact_input_cap() {
    let request = simple_request();
    let mut bytes =
        serde_json::to_vec(&ExactUtf8DevelopmentRequest::from_request(&request).unwrap()).unwrap();
    bytes.resize(MAX_WIRE_BYTES, b' ');
    assert_eq!(
        ExactUtf8DevelopmentRequest::parse(&bytes)
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    bytes.push(b' ');
    assert!(ExactUtf8DevelopmentRequest::parse(&bytes)
        .unwrap_err()
        .0
        .contains("exceeds"));
    for bad in [vec![0xff], vec![b' '; MAX_WIRE_BYTES + 1]] {
        let mut invalid = request.clone();
        invalid.sources[0].source_bytes = bad;
        assert!(ExactUtf8DevelopmentRequest::from_request(&invalid).is_err());
    }
    // Raw and parsed AST agree, so only the existing executable limits reject these.
    for (expression, expected) in [
        (
            (0..MAX_EXPR_DEPTH + 1).fold(int(1), |left, _| Expr::Add {
                left: Box::new(left),
                right: Box::new(int(1)),
            }),
            "budget",
        ),
        (
            (0..13).fold(int(1), |left, _| Expr::Add {
                left: Box::new(left.clone()),
                right: Box::new(left),
            }),
            "budget",
        ),
    ] {
        let mut invalid = request.clone();
        let mut ast = serde_json::to_value(&invalid.sources[0].program).unwrap();
        ast["actions"][3]["steps"][0]["values"]["production"] =
            serde_json::to_value(expression).unwrap();
        invalid.sources[0].source_bytes = serde_json::to_vec(&ast).unwrap();
        invalid.sources[0].program = serde_json::from_value(ast).unwrap();
        assert!(ExactUtf8DevelopmentRequest::from_request(&invalid)
            .unwrap_err()
            .0
            .contains(expected));
    }
}

fn pad_source(request: &mut DevelopmentRequest, padding: &[u8]) {
    let old = &request.sources[0];
    let mut bytes = old.source_bytes.clone();
    bytes.extend_from_slice(padding);
    request.sources[0] = CapturedProgram::capture(
        &bytes,
        &request.project_id,
        old.binding.producer.clone(),
        old.binding.task.clone(),
    )
    .unwrap()
    .at_path(&old.binding.program_path)
    .unwrap();
}

#[test]
fn prompt_fallback_obeys_exact_legacy_and_compact_boundaries() {
    let mut request = simple_request();
    request.operation = DevelopmentOperation::Modify;
    request.sources.truncate(1);
    let options = ProviderOptions::default();
    let original = encode_request(&request, &options).unwrap();
    let gap = product_provider::MAX_PROMPT_BYTES - original.prompt.len();
    pad_source(&mut request, &vec![b' '; gap / 3]);
    request.request.push_str(&"x".repeat(gap % 3));
    let at_legacy_limit = encode_request(&request, &options).unwrap();
    assert_eq!(
        at_legacy_limit.prompt.len(),
        product_provider::MAX_PROMPT_BYTES
    );
    assert_eq!(at_legacy_limit.prompt, legacy_prompt(&request));
    request.request.push('x');
    let first_compact = encode_request(&request, &options).unwrap();
    assert!(first_compact.prompt.len() < product_provider::MAX_PROMPT_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&first_compact.prompt).unwrap();
    assert_eq!(
        parse_codec(&value["request"])
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    // Escape-heavy whitespace remains exact and counts against the complete prompt.
    pad_source(&mut request, b"\r\n\t\r\n\t");
    let gap = product_provider::MAX_PROMPT_BYTES
        - encode_request(&request, &options).unwrap().prompt.len();
    pad_source(&mut request, &vec![b' '; gap]);
    let at_limit = encode_request(&request, &options).unwrap();
    assert_eq!(at_limit.prompt.len(), product_provider::MAX_PROMPT_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&at_limit.prompt).unwrap();
    assert_eq!(
        parse_codec(&value["request"])
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    pad_source(&mut request, b" ");
    assert!(
        format!("{:?}", encode_request(&request, &options).unwrap_err())
            .contains("prompt must be bounded")
    );
    // CLI fixture preparation is supported on Linux/macOS only. Keep every
    // codec and complete-prompt boundary assertion above portable.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let dir = tempdir();
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let jobs = dir.path().join("jobs");
        let transport = product_provider::ProviderTransport::new_fixture(
            jobs.clone(),
            product_provider::ProviderKind::Codex,
            dir.path().join("missing-cli"),
            home,
        )
        .unwrap();
        let error = prepare_development(transport, &request, options)
            .err()
            .expect("still-too-large must fail before provider preparation");
        assert!(format!("{error:?}").contains("prompt must be bounded"));
        assert_eq!(std::fs::read_dir(jobs).unwrap().count(), 0);
    }
}

fn legacy_prompt(request: &DevelopmentRequest) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "instructions": include_str!("fixtures/development_request_legacy_instructions.txt"),
        "request_digest": request.identity().unwrap(),
        "request": request,
        "application_schema": serde_json::from_str::<serde_json::Value>(APP_SCHEMA).unwrap(),
    }))
    .unwrap()
}

#[cfg(unix)]
#[test]
fn legacy_prepared_and_issued_job_keeps_exact_wire_disclosure_and_receipt() {
    use product_provider::*;
    let request = simple_request();
    let expected_wire = ProviderRequest {
        request_id: request.id.clone(), provider: ProviderKind::Codex,
        source_digest: request.sources[1].binding.identity().unwrap().as_str().into(),
        prompt: legacy_prompt(&request), schema: RESPONSE_SCHEMA.as_bytes().to_vec(),
        purpose: "Local application Discover; returned suggestions require independent execution".into(),
        data_categories: vec!["Selected executable sources and their provenance".into(), "User request, selected context and active scoped decisions/unknowns".into(), "Explicitly selected synthetic or sanitized scenario examples and accepted observations".into()],
        profile: CapabilityProfile::DataOnly, limits: JobLimits::default(),
    };
    let response = DevelopmentResponse {
        version: 1,
        request_digest: request.identity().unwrap(),
        candidates: vec![GeneratedCandidate {
            id: "after".into(),
            source_json: String::from_utf8(request.sources[1].source_bytes.clone()).unwrap(),
        }],
        hypotheses: vec![],
        evolutions: vec![],
        unsupported: vec![],
    };
    let dir = tempdir();
    let transport = fixture_transport(dir.path(), &response);
    // Prepare using the frozen pre-codec projection, then regenerate by the new API.
    let prepared = transport.prepare(expected_wire.clone()).unwrap();
    let regenerated = encode_request(&request, &ProviderOptions::default()).unwrap();
    assert_eq!(
        serde_json::to_vec(&regenerated).unwrap(),
        serde_json::to_vec(&expected_wire).unwrap()
    );
    assert_eq!(
        regenerated.digest().unwrap(),
        prepared.disclosure.request_digest
    );
    assert_eq!(
        digest(&regenerated.prompt),
        prepared.disclosure.prompt_digest
    );
    assert_eq!(regenerated.source_digest, prepared.disclosure.source_digest);
    let saved_disclosure = serde_json::to_vec(&prepared.disclosure).unwrap();
    let saved_request = std::fs::read(
        dir.path()
            .join("jobs")
            .join(&request.id)
            .join("request.json"),
    )
    .unwrap();
    let stdin =
        std::fs::read(dir.path().join("jobs").join(&request.id).join("stdin.json")).unwrap();
    assert_eq!(digest(&stdin), prepared.disclosure.stdin_digest);
    let consent = ConsentReceipt {
        disclosure_digest: prepared.disclosure.digest(),
        approval_reference: "Synthetic legacy compatibility".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let mut job = transport.submit(prepared, &consent).unwrap();
    let start = std::time::Instant::now();
    let receipt = loop {
        if let Some(receipt) = job.poll().unwrap() {
            break receipt;
        }
        assert!(start.elapsed() < std::time::Duration::from_secs(30));
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(receipt.state, JobState::TransportValidated);
    assert_eq!(receipt.provenance, InvocationProvenance::TransportFixture);
    assert_eq!(
        serde_json::to_vec(&receipt.disclosure).unwrap(),
        saved_disclosure
    );
    let regenerated = encode_request(&request, &ProviderOptions::default()).unwrap();
    let retained = transport
        .ingest(
            &request.id,
            &regenerated.digest().unwrap(),
            &regenerated.source_digest,
        )
        .unwrap()
        .unwrap();
    let actual = DevelopmentResponse::parse(&retained.final_bytes).unwrap();
    actual.validate_for(&request).unwrap();
    assert_eq!(actual, response);
    assert_eq!(
        std::fs::read(
            dir.path()
                .join("jobs")
                .join(&request.id)
                .join("request.json")
        )
        .unwrap(),
        saved_request
    );
}

#[test]
fn still_oversized_selected_context_refuses_before_preparation_and_keeps_work() {
    let dir = tempdir();
    let store = ProductStore::create(dir.path().join("tool"), &program(false), 20000).unwrap();
    let (_, _, candidate) = managed(&store, false);
    let current = store.load().unwrap();
    let (req, result) = modify(&store, &candidate, "wide-selected");
    let draft = draft_from(
        &store,
        req,
        result,
        ScopePopulation::FutureWork,
        "wide-selected",
    );
    let mut request = draft.request().clone();
    // The original records, inputs and accepted observations stay present and
    // are truthfully disclosed when selected as real business copies.
    for example in &mut request.examples {
        example.disclosure = Disclosure::ExplicitlySelected;
    }
    for scene in &mut request.accepted_scenes {
        scene.disclosure = Disclosure::ExplicitlySelected;
    }
    let options = ProviderOptions {
        profile: product_provider::CapabilityProfile::TrustedHarness,
        ..ProviderOptions::default()
    };
    let wire = encode_request(&request, &options).unwrap();
    assert_eq!(
        wire.profile,
        product_provider::CapabilityProfile::TrustedHarness
    );
    assert!(wire
        .data_categories
        .iter()
        .any(|category| category.contains("real business copies, not sanitized")));
    let prompt: serde_json::Value = serde_json::from_slice(&wire.prompt).unwrap();
    assert_eq!(
        parse_codec(&prompt["request"])
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    let copy = request.examples[0].clone();
    for index in request.examples.len()..MAX_ITEMS {
        let mut selected = copy.clone();
        selected.scenario.id = format!("additional-selected-{index}");
        request.examples.push(selected);
    }
    request.validate().unwrap();
    let encoded =
        serde_json::to_vec(&ExactUtf8DevelopmentRequest::from_request(&request).unwrap()).unwrap();
    assert!(encoded.len() > product_provider::MAX_PROMPT_BYTES);
    eprintln!(
        "still-too-large authentic source context: examples={} exact_utf8_request={}",
        request.examples.len(),
        encoded.len()
    );
    assert!(
        format!("{:?}", encode_request(&request, &options).unwrap_err())
            .contains("prompt must be bounded")
    );
    // Preserve portable refusal and store checks without constructing the
    // unsupported Windows CLI fixture transport.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        let home = dir.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let jobs = dir.path().join("jobs");
        let transport = product_provider::ProviderTransport::new_fixture(
            jobs.clone(),
            product_provider::ProviderKind::Codex,
            dir.path().join("missing-cli"),
            home,
        )
        .unwrap();
        let error = prepare_development(transport, &request, options)
            .err()
            .expect("complete selected context must refuse when still too large");
        assert!(format!("{error:?}").contains("prompt must be bounded"));
        assert_eq!(std::fs::read_dir(jobs).unwrap().count(), 0);
    }
    assert_eq!(store.load().unwrap(), current);
}

fn reconciliation_property() -> AcceptedProperty {
    AcceptedProperty {
        id: "one-person".into(),
        description: "This public work retains one person".into(),
        predicate: PropertyPredicate::Equal {
            left: PropertyTerm::Observed {
                point: "done".into(),
                observable: "count".into(),
                value_type: Type::Integer,
            },
            right: PropertyTerm::Literal {
                value_type: Type::Integer,
                value: DataValue::Integer { value: 1 },
            },
        },
    }
}
fn reconciliation_request() -> DevelopmentRequest {
    let mut request = simple_request();
    request.operation = DevelopmentOperation::Reconcile;
    request.request = "Reconcile the requested need; retain independent promises".into();
    request.sources.truncate(1);
    let witness = canonical_digest(IdentityDomain::Decision, &"synthetic witness").unwrap();
    for (id, status) in [
        ("requested", DecisionStatus::Active),
        ("independent", DecisionStatus::Active),
        ("duplicate", DecisionStatus::Active),
        ("pending", DecisionStatus::Pending),
        (
            "withdrawn",
            DecisionStatus::Withdrawn {
                adoption: "removed".into(),
            },
        ),
        (
            "superseded",
            DecisionStatus::Superseded {
                by: "independent".into(),
            },
        ),
    ] {
        let mut property = reconciliation_property();
        if id != "requested" && id != "duplicate" {
            property.description = format!("Retain the {id} promise: Café 中文");
        }
        request.decisions.decisions.push(ScopedDecision {
            id: id.into(),
            revision: 1,
            request: format!("Preserve {id} work"),
            rationale: None,
            scope: DecisionScope {
                operations: ["calculate".into()].into(),
                population: Population::All,
                conditions: Values::new(),
                excluded_records: vec![],
                unknowns: vec![],
            },
            outcome: DecisionOutcome::KeepCurrent,
            status,
            obligations: if id == "independent" {
                let mut second = property.clone();
                second.id = "another-property".into();
                vec![property, second]
            } else {
                vec![property]
            },
            scenarios: vec![witness.clone()],
            witness: witness.clone(),
            supersedes: if id == "independent" {
                vec!["superseded".into()]
            } else {
                vec![]
            },
        });
    }
    request.validate().unwrap();
    request
}
fn reconciliation_prompt(request: &DevelopmentRequest) -> serde_json::Value {
    let wire = encode_request(request, &ProviderOptions::default()).unwrap();
    serde_json::from_slice(&wire.prompt).unwrap()
}

#[test]
fn reconcile_context_exposes_all_active_property_identities() {
    use serde_json::json;
    let request = reconciliation_request();
    let original = serde_json::to_vec(&request).unwrap();
    let prompt = reconciliation_prompt(&request);
    let refs = prompt["reconciliation_obligations"]["active_obligations"]
        .as_array()
        .expect(
            "released Reconcile author context must supply host-computed obligation identities",
        );
    assert_eq!(prompt["reconciliation_obligations"]["version"], 1);
    assert_eq!(
        refs.len(),
        4,
        "include every Active decision, not only the requested need"
    );
    let expected: Vec<_> = request.decisions.decisions[..3]
        .iter()
        .flat_map(|decision| {
            decision.obligations.iter().map(|property| {
                json!({
                    "decision_id": decision.id,
                    "property_id": property.id,
                    "digest": property.identity().unwrap(),
                })
            })
        })
        .collect();
    assert_eq!(refs, &expected);
    // Public canonical vector also catches accidental property-ID-only hashes,
    // a different identity domain, or hashing the serialized reference itself.
    assert_eq!(
        refs[0]["digest"],
        "1e5638c7959e8af7247d62185a13e9a324afb927f278a15dc51c3841c574b81c"
    );
    assert_eq!(refs[0]["digest"], refs[3]["digest"]);
    assert_ne!(refs[0]["digest"], refs[1]["digest"]);
    let instructions = prompt["instructions"].as_str().unwrap();
    assert!(instructions.contains("preserved_obligations"));
    assert!(instructions.contains("distinct digest"));
    assert_eq!(prompt["request"], serde_json::to_value(&request).unwrap());
    assert_eq!(prompt["request_digest"], json!(request.identity().unwrap()));
    assert_eq!(serde_json::to_vec(&request).unwrap(), original);
    let wire = encode_request(&request, &ProviderOptions::default()).unwrap();
    assert_eq!(wire.schema, RESPONSE_SCHEMA.as_bytes());
    assert_eq!(
        wire.source_digest,
        request.sources[0].binding.identity().unwrap().as_str()
    );
}

#[test]
fn reconcile_context_handles_empty_and_binds_property_content() {
    use serde_json::json;
    let mut request = reconciliation_request();
    let old = encode_request(&request, &ProviderOptions::default()).unwrap();
    let old_prompt: serde_json::Value = serde_json::from_slice(&old.prompt).unwrap();
    request.decisions.decisions[1].obligations[0]
        .description
        .push('!');
    let changed = encode_request(&request, &ProviderOptions::default()).unwrap();
    let changed_prompt: serde_json::Value = serde_json::from_slice(&changed.prompt).unwrap();
    assert_ne!(
        old_prompt["request_digest"],
        changed_prompt["request_digest"]
    );
    assert_ne!(old.digest().unwrap(), changed.digest().unwrap());
    assert_ne!(
        old_prompt["reconciliation_obligations"]["active_obligations"][1]["digest"],
        changed_prompt["reconciliation_obligations"]["active_obligations"][1]["digest"]
    );
    assert_eq!(
        old_prompt["reconciliation_obligations"]["active_obligations"][0],
        changed_prompt["reconciliation_obligations"]["active_obligations"][0]
    );
    for decision in &mut request.decisions.decisions {
        if decision.status == DecisionStatus::Active {
            decision.obligations.clear();
        }
    }
    assert_eq!(
        reconciliation_prompt(&request)["reconciliation_obligations"],
        json!({"version":1,"active_obligations":[]})
    );
    request.decisions.decisions.clear();
    assert_eq!(
        reconciliation_prompt(&request)["reconciliation_obligations"],
        json!({"version":1,"active_obligations":[]})
    );
    request.operation = DevelopmentOperation::Modify;
    assert!(reconciliation_prompt(&request)
        .get("reconciliation_obligations")
        .is_none());
    // Invalid property content must still fail before projection/preparation.
    request = reconciliation_request();
    request.decisions.decisions[0].obligations[0].id.clear();
    assert!(encode_request(&request, &ProviderOptions::default()).is_err());
}

#[test]
fn reconcile_context_survives_compaction_and_counts_toward_prompt_cap() {
    let mut request = reconciliation_request();
    let options = ProviderOptions::default();
    let original = encode_request(&request, &options).unwrap();
    let original_value: serde_json::Value = serde_json::from_slice(&original.prompt).unwrap();
    let references = original_value["reconciliation_obligations"].clone();
    assert!(references["active_obligations"].is_array());
    let gap = product_provider::MAX_PROMPT_BYTES - original.prompt.len();
    pad_source(&mut request, &vec![b' '; gap / 3]);
    request.request.push_str(&"x".repeat(gap % 3));
    let at_limit = encode_request(&request, &options).unwrap();
    assert_eq!(at_limit.prompt.len(), product_provider::MAX_PROMPT_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&at_limit.prompt).unwrap();
    assert_eq!(value["request"], serde_json::to_value(&request).unwrap());
    assert_eq!(value["reconciliation_obligations"], references);
    request.request.push('x');
    let compact = encode_request(&request, &options).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&compact.prompt).unwrap();
    assert_eq!(
        parse_codec(&value["request"])
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    assert_eq!(value["reconciliation_obligations"], references);
    assert!(value["instructions"]
        .as_str()
        .unwrap()
        .contains("preserved_obligations"));
    let gap = product_provider::MAX_PROMPT_BYTES - compact.prompt.len();
    pad_source(&mut request, &vec![b' '; gap]);
    let at_limit = encode_request(&request, &options).unwrap();
    assert_eq!(at_limit.prompt.len(), product_provider::MAX_PROMPT_BYTES);
    let value: serde_json::Value = serde_json::from_slice(&at_limit.prompt).unwrap();
    assert_eq!(value["reconciliation_obligations"], references);
    assert_eq!(
        parse_codec(&value["request"])
            .unwrap()
            .to_request()
            .unwrap(),
        request
    );
    pad_source(&mut request, b" ");
    assert!(
        format!("{:?}", encode_request(&request, &options).unwrap_err())
            .contains("prompt must be bounded")
    );
}
