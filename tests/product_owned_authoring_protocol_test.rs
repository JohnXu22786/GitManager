//! Fresh synthetic source/transport cases. No provider, operator or old-A evidence.
#[path = "fixtures/common_fact/mod.rs"]
mod fixture;
#[path = "../src/product_studio/owned_authoring.rs"]
mod owned_authoring;
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
use owned_authoring::*;
use product_contract::*;
use product_decisions::{DecisionEngine, IntentArchive};
use product_discovery::ProviderOptions;
use product_provider::*;
use product_runtime::LocalRuntime;
use product_store::{ProductStore, ProjectSnapshot};
use serde_json::{json, Value};

struct Case {
    _directory: tempfile::TempDir,
    snapshot: ProjectSnapshot,
    domain: DevelopmentRequest,
    request: OwnedAuthorRequestV1,
    candidate: CapturedProgram,
    response: Value,
}
fn case() -> Case {
    let (before, candidate) = fixture::pair();
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = ProductStore::create(directory.path().join("tool"), &before, 20000).unwrap();
    let snapshot = store.load().unwrap();
    let domain = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store.clone()))
        .development_request(
            &snapshot,
            "owned-author-fixture",
            DevelopmentOperation::Modify,
            "Keep exact current work while proposing a changed waiting rule",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(snapshot.data.identity().unwrap()),
                session_digest: Some(snapshot.session.identity().unwrap()),
            },
        )
        .unwrap();
    let request = OwnedAuthorRequestV1::new(&snapshot, &domain, domain.sources.len() - 1).unwrap();
    let core = fixture::request(&store, &before, &candidate);
    let locator = |e: &product_store::scope::OwnedSpanEndpoint, side: &str| {
        json!({
            "role": side, "action":e.action, "list_path":e.list_path, "start":e.start, "len":e.len,
            "root":e.root,
        })
    };
    let response = json!({
        "encoding":"gitmanager.owned-author-response", "version":1,
        "request_digest": request.identity().unwrap(),
        "response": {"version":1,"request_digest":domain.identity().unwrap(),
            "candidates":[{"id":"changed","source_json":String::from_utf8(candidate.source_bytes.clone()).unwrap()}],
            "hypotheses":[],"evolutions":[],"unsupported":[]},
        "suggestions":[{"candidate":"changed","raw_digest":candidate.artifact.raw_digest,
            "root_entity":"job", "edits":core.inventory.sources.edits.iter().map(|e| json!({
                "id":e.id,"before":locator(&e.before,"current"),"after":locator(&e.after,"candidate")
            })).collect::<Vec<_>>(),
            "auxiliary":core.auxiliary,"lifecycle":core.inventory.lifecycle}]
    });
    Case {
        _directory: directory,
        snapshot,
        domain,
        request,
        candidate,
        response,
    }
}
fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn parsed(c: &Case) -> ParsedOwnedAuthorResponse {
    ParsedOwnedAuthorResponse::parse_for(&bytes(&c.response), &c.request).unwrap()
}
fn refuse(c: &Case, value: Value) {
    assert!(
        ParsedOwnedAuthorResponse::parse_for(&bytes(&value), &c.request)
            .and_then(|r| r.materialize(&c.request, &c.snapshot, "changed", &c.candidate))
            .is_err()
    );
}
#[test]
fn exact_wrapper_and_host_anchors_materialize_without_acceptance() {
    let c = case();
    let mut raw = b" \n".to_vec();
    raw.extend(bytes(&c.response));
    raw.push(b' ');
    let r = ParsedOwnedAuthorResponse::parse_for(&raw, &c.request).unwrap();
    assert_eq!(r.raw_bytes(), raw);
    let p = r
        .materialize(&c.request, &c.snapshot, "changed", &c.candidate)
        .unwrap()
        .unwrap();
    assert_eq!(
        p.sources.after,
        canonical_digest(IdentityDomain::Source, &c.candidate).unwrap()
    );
    assert_eq!(p.structure.edits, p.sources.edits);
    assert_eq!(p.sources.edits[0].before.len, 0);
    assert_eq!(p.sources.edits[1].before.list_path, vec![2]);
    assert_eq!(
        p.sources.edits[0].before.digest,
        canonical_digest(
            IdentityDomain::Evidence,
            &("owned-span/1", Vec::<Statement>::new())
        )
        .unwrap()
    );
    assert!(p.sources.edits[0].before.previous.is_none());
    assert!(p.sources.edits[0].before.next.is_some());
    assert_eq!(
        p.lifecycle.meaning,
        c.response["suggestions"][0]["lifecycle"]["meaning"]
    );
    let encoded = c.request.to_bytes().unwrap();
    let restored = OwnedAuthorRequestV1::parse_for(&encoded, &c.snapshot, &c.domain).unwrap();
    assert_eq!(restored.identity().unwrap(), c.request.identity().unwrap());
    assert_eq!(restored.domain_request().unwrap(), c.domain);
}
#[test]
fn request_identity_layers_and_legacy_codec_stay_distinct() {
    let c = case();
    let options = ProviderOptions::default();
    let legacy = product_discovery::encode_request(&c.domain, &options).unwrap();
    let wire = c.request.encode(&options).unwrap();
    assert_ne!(wire.digest().unwrap(), legacy.digest().unwrap());
    assert_ne!(
        wire.digest().unwrap(),
        c.request.identity().unwrap().as_str()
    );
    assert_ne!(c.request.identity().unwrap(), c.domain.identity().unwrap());
    assert_eq!(
        product_discovery::encode_request(&c.domain, &options).unwrap(),
        legacy
    );
    assert_eq!(wire.schema, OWNED_AUTHOR_RESPONSE_SCHEMA.as_bytes());
    assert!(DevelopmentResponse::parse(&bytes(&c.response)).is_err());
    for digest in [
        c.domain.identity().unwrap().as_str().to_owned(),
        wire.digest().unwrap(),
    ] {
        let mut v = c.response.clone();
        v["request_digest"] = json!(digest);
        refuse(&c, v);
    }
    let mut v = c.response.clone();
    v["response"]["request_digest"] = json!(c.request.identity().unwrap());
    refuse(&c, v);
}
#[test]
fn snapshot_current_capture_and_complete_request_are_checked() {
    let c = case();
    let mut snapshot = c.snapshot.clone();
    snapshot.revision += 1;
    assert!(parsed(&c)
        .materialize(&c.request, &snapshot, "changed", &c.candidate)
        .is_err());
    assert!(
        OwnedAuthorRequestV1::parse_for(&c.request.to_bytes().unwrap(), &snapshot, &c.domain)
            .is_err()
    );
    let mut domain = c.domain.clone();
    domain.request.push_str(" stale");
    assert!(
        OwnedAuthorRequestV1::parse_for(&c.request.to_bytes().unwrap(), &c.snapshot, &domain)
            .is_err()
    );
    domain = c.domain.clone();
    domain.sources[0].binding.producer = Producer::ExternalAuthor {
        description: "different author".into(),
    };
    assert!(OwnedAuthorRequestV1::new(&c.snapshot, &domain, 0).is_err());
    domain = c.domain.clone();
    domain.sources[0].binding.task = Some(TaskSource {
        task_id: "foreign".into(),
        source_fingerprint: "different".into(),
    });
    assert!(OwnedAuthorRequestV1::new(&c.snapshot, &domain, 0).is_err());
    assert!(OwnedAuthorRequestV1::new(&c.snapshot, &c.domain, usize::MAX).is_err());
    let mut candidate = c.candidate.clone();
    candidate.source_bytes.push(b' ');
    candidate = CapturedProgram::capture(
        &candidate.source_bytes,
        &c.domain.project_id,
        candidate.binding.producer.clone(),
        None,
    )
    .unwrap();
    assert_eq!(candidate.program, c.candidate.program);
    assert!(parsed(&c)
        .materialize(&c.request, &c.snapshot, "changed", &candidate)
        .is_err());
}
#[test]
fn syntax_unknown_fields_duplicates_utf8_depth_and_sizes_refuse() {
    let c = case();
    for pointer in [
        "",
        "/response",
        "/response/candidates/0",
        "/suggestions/0",
        "/suggestions/0/edits/0",
        "/suggestions/0/edits/0/before",
        "/suggestions/0/lifecycle",
    ] {
        let mut v = c.response.clone();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("accepted".into(), json!(true));
        refuse(&c, v);
    }
    for key in [
        "encoding",
        "version",
        "request_digest",
        "response",
        "suggestions",
    ] {
        let mut v = c.response.clone();
        v.as_object_mut().unwrap().remove(key);
        refuse(&c, v);
    }
    for pointer in ["/version", "/response/version"] {
        let mut v = c.response.clone();
        *v.pointer_mut(pointer).unwrap() = json!(2);
        refuse(&c, v);
    }
    let raw = String::from_utf8(bytes(&c.response)).unwrap();
    for fragment in [
        "\"version\":1",
        "\"candidate\":\"changed\"",
        "\"role\":\"current\"",
        "\"meaning\":",
    ] {
        let duplicate = if fragment.ends_with(':') {
            raw.replace(fragment, "\"meaning\":\"first\",\"meaning\":")
        } else {
            raw.replace(fragment, &format!("{fragment},{fragment}"))
        };
        assert!(ParsedOwnedAuthorResponse::parse_for(duplicate.as_bytes(), &c.request).is_err());
    }
    for raw in [
        vec![0xff],
        vec![b' '; MAX_WIRE_BYTES + 1],
        b"{".to_vec(),
        format!("{}0{}", "[".repeat(65), "]".repeat(65)).into_bytes(),
    ] {
        assert!(ParsedOwnedAuthorResponse::parse_for(&raw, &c.request).is_err());
    }
    let mut v = c.response.clone();
    v["response"]["candidates"][0]["source_json"] = json!("{\"version\":1,\"version\":1}");
    refuse(&c, v);
}
#[test]
fn foreign_duplicate_candidates_and_bad_suggestions_never_fall_back() {
    let c = case();
    let mut v = c.response.clone();
    v["suggestions"][0]["candidate"] = json!("foreign");
    refuse(&c, v);
    let mut v = c.response.clone();
    let s = v["suggestions"][0].clone();
    v["suggestions"].as_array_mut().unwrap().push(s);
    refuse(&c, v);
    let mut v = c.response.clone();
    let s = v["response"]["candidates"][0].clone();
    v["response"]["candidates"].as_array_mut().unwrap().push(s);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["raw_digest"] = json!(c.snapshot.program().unwrap().artifact.raw_digest);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"] = json!([]);
    let p = ParsedOwnedAuthorResponse::parse_for(&bytes(&v), &c.request).unwrap();
    assert!(p
        .materialize(&c.request, &c.snapshot, "changed", &c.candidate)
        .unwrap()
        .is_none());
    assert!(p
        .materialize(&c.request, &c.snapshot, "missing", &c.candidate)
        .is_err());
}
#[test]
fn structural_locator_errors_and_overlap_are_not_repaired() {
    let c = case();
    for (pointer, value) in [
        ("/suggestions/0/root_entity", json!("missing")),
        ("/suggestions/0/edits/0/before/role", json!("candidate")),
        ("/suggestions/0/edits/0/after/action", json!("arrive")),
        ("/suggestions/0/edits/0/before/start", json!(usize::MAX)),
        ("/suggestions/0/edits/0/before/len", json!(usize::MAX)),
        ("/suggestions/0/edits/1/before/list_path", json!([999])),
        ("/suggestions/0/edits/1/before/root/name", json!("missing")),
        ("/suggestions/0/edits/0/after/len", json!(0)),
    ] {
        let mut v = c.response.clone();
        *v.pointer_mut(pointer).unwrap() = value;
        refuse(&c, v);
    }
    let mut v = c.response.clone();
    let mut e = v["suggestions"][0]["edits"][0].clone();
    e["id"] = json!("overlap");
    v["suggestions"][0]["edits"].as_array_mut().unwrap().push(e);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["edits"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    refuse(&c, v);
}
#[test]
fn false_lifecycle_and_auxiliary_names_are_only_untrusted_proposals() {
    let c = case();
    let mut v = c.response.clone();
    v["suggestions"][0]["auxiliary"][0]["owner_field"] = json!("plausible_but_false");
    v["suggestions"][0]["lifecycle"]["meaning"] = json!("Author claims every row is complete");
    let r = ParsedOwnedAuthorResponse::parse_for(&bytes(&v), &c.request).unwrap();
    let p = r
        .materialize(&c.request, &c.snapshot, "changed", &c.candidate)
        .unwrap()
        .unwrap();
    assert_eq!(p.auxiliary[0].owner_field, "plausible_but_false");
    assert_eq!(p.lifecycle.meaning, "Author claims every row is complete");
    // This result has no population, acceptance, preparation, adoption or readiness.
    assert_eq!(c.snapshot.revision, 0);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn run_fixture(
    c: &Case,
    options: &ProviderOptions,
) -> (
    tempfile::TempDir,
    DataDisclosure,
    ConsentReceipt,
    ProviderResult,
) {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        thread,
        time::{Duration, Instant},
    };
    let directory = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let script = directory.path().join("synthetic-owned-author.py");
    let fixture=include_str!("fixtures/provider_transport/fake_cli.py").replace(
        "envelope['payload'] = {'passed': True, 'text': wire['prompt'], 'command': 'untrusted-do-not-execute'}",
        "envelope['payload'] = cfg['payload']");
    {
        let _guard = fixture_executable_write_guard();
        fs::write(&script, fixture).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::write(
        script.with_extension("json"),
        bytes(&json!({"provider":"codex","mode":"good","payload":c.response})),
    )
    .unwrap();
    let home = directory.path().join("home");
    fs::create_dir(&home).unwrap();
    let transport = ProviderTransport::new_fixture(
        directory.path().join("jobs"),
        ProviderKind::Codex,
        script,
        home,
    )
    .unwrap();
    let prepared = transport
        .prepare(c.request.encode(options).unwrap())
        .unwrap();
    let disclosure = prepared.disclosure.clone();
    let consent = ConsentReceipt {
        disclosure_digest: disclosure.digest(),
        approval_reference: "synthetic-local-test-only".into(),
        expires_at_unix_ms: unix_ms() + 60_000,
    };
    let mut job = transport.submit(prepared, &consent).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if job.poll().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    let wire = c.request.encode(options).unwrap();
    let result = transport
        .ingest(&c.domain.id, &wire.digest().unwrap(), &wire.source_digest)
        .unwrap()
        .unwrap();
    (directory, disclosure, consent, result)
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn actual_transport_fixture_binds_outer_raw_and_rejects_provenance_swaps() {
    let c = case();
    let options = ProviderOptions::default();
    let (_directory, disclosure, consent, result) = run_fixture(&c, &options);
    let lineage =
        OwnedCliLineage::validate(&c.request, &options, &disclosure, &consent, &result).unwrap();
    assert!(matches!(lineage.producer(), Producer::Fixture { .. }));
    assert_eq!(lineage.raw_bytes(), result.final_bytes);
    let capture = lineage.capture_candidate("changed").unwrap();
    assert!(lineage
        .materialize(&c.snapshot, "changed")
        .unwrap()
        .is_some());
    let mut substituted = capture.clone();
    substituted.binding.producer = Producer::ExternalAuthor {
        description: "native worker".into(),
    };
    assert!(lineage.validate_capture("changed", &substituted).is_err());
    substituted = capture.clone();
    substituted.binding.task = Some(TaskSource {
        task_id: "foreign".into(),
        source_fingerprint: "fake".into(),
    });
    assert!(lineage.validate_capture("changed", &substituted).is_err());
    for alteration in 0..10 {
        let mut wrong = result.clone();
        match alteration {
            0 => wrong.final_bytes.push(b' '),
            1 => wrong.receipt.result_digest = Some(digest(&bytes(&c.response["response"]))),
            2 => wrong.receipt.request_digest = c.domain.identity().unwrap().as_str().into(),
            3 => wrong.receipt.request_digest = c.request.identity().unwrap().as_str().into(),
            4 => wrong.receipt.provenance = InvocationProvenance::LiveCli,
            5 => wrong.receipt.capabilities.executable_digest = digest(b"different executable"),
            6 => wrong.receipt.disclosure.nonce = "0".repeat(32),
            7 => wrong.receipt.state = JobState::InvalidOutput,
            8 => wrong.receipt.started_at_unix_ms = Some(consent.expires_at_unix_ms + 1),
            _ => wrong.receipt.session_id = None,
        }
        assert!(
            OwnedCliLineage::validate(&c.request, &options, &disclosure, &consent, &wrong).is_err(),
            "alteration {alteration}"
        );
    }
    let mut wrong = consent.clone();
    wrong.approval_reference = "".into();
    assert!(OwnedCliLineage::validate(&c.request, &options, &disclosure, &wrong, &result).is_err());
}

#[test]
fn bounded_counts_and_expression_types_refuse_before_materialization() {
    let c = case();
    let mut v = c.response.clone();
    v["suggestions"][0]["edits"] =
        json!(vec![v["suggestions"][0]["edits"][0].clone(); MAX_ITEMS + 1]);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["auxiliary"] = json!(vec![
        v["suggestions"][0]["auxiliary"][0].clone();
        MAX_ITEMS + 1
    ]);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["lifecycle"]["before"] = json!({"kind":"variable","name":"missing"});
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["lifecycle"]["before"] = serde_json::to_value(fixture::int(0)).unwrap();
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["lifecycle"]["meaning"] = json!("x".repeat(MAX_TEXT_BYTES + 1));
    refuse(&c, v);
    let mut v = c.response.clone();
    v["suggestions"][0]["edits"][1]["before"]["list_path"] = json!(vec![0; MAX_EXPR_DEPTH + 1]);
    refuse(&c, v);
    let mut v = c.response.clone();
    v["response"]["candidates"]=json!((0..9).map(|i|json!({"id":format!("c{i}"),"source_json":String::from_utf8(c.candidate.source_bytes.clone()).unwrap()})).collect::<Vec<_>>());
    refuse(&c, v);
}

#[test]
fn explicit_removal_gap_keeps_exact_adjacent_statement_anchors() {
    let c = case();
    let after = c.snapshot.program().unwrap().clone();
    let mut app = after.program.clone();
    let action = app.actions.iter_mut().find(|a| a.id == "withdraw").unwrap();
    let start = action.steps.len();
    action.steps.push(Statement::Assert {
        condition: fixture::boolean(true),
        message: "Synthetic removable assertion".into(),
    });
    let before = fixture::capture(&app);
    let store = ProductStore::create(c._directory.path().join("removal"), &before, 20000).unwrap();
    let snapshot = store.load().unwrap();
    let domain = DecisionEngine::new(LocalRuntime::default(), IntentArchive::new(store))
        .development_request(
            &snapshot,
            "removal",
            DevelopmentOperation::Modify,
            "Remove the synthetic assertion",
            DevelopmentContext {
                view: None,
                selected: vec![],
                recent_inputs: vec![],
                data_digest: Some(snapshot.data.identity().unwrap()),
                session_digest: Some(snapshot.session.identity().unwrap()),
            },
        )
        .unwrap();
    let request = OwnedAuthorRequestV1::new(&snapshot, &domain, domain.sources.len() - 1).unwrap();
    let mut response = c.response.clone();
    response["request_digest"] = json!(request.identity().unwrap());
    response["response"]["request_digest"] = json!(domain.identity().unwrap());
    response["response"]["candidates"][0]["source_json"] =
        json!(String::from_utf8(after.source_bytes.clone()).unwrap());
    response["suggestions"][0]["raw_digest"] = json!(after.artifact.raw_digest);
    response["suggestions"][0]["auxiliary"] = json!([]);
    response["suggestions"][0]["edits"] = json!([{"id":"remove_assertion",
        "before":{"role":"current","action":"withdraw","list_path":[],"start":start,"len":1,"root":null},
        "after":{"role":"candidate","action":"withdraw","list_path":[],"start":start,"len":0,"root":null}}]);
    let proposal = ParsedOwnedAuthorResponse::parse_for(&bytes(&response), &request)
        .unwrap()
        .materialize(&request, &snapshot, "changed", &after)
        .unwrap()
        .unwrap();
    let edit = &proposal.sources.edits[0];
    assert!(edit.after.next.is_none());
    assert_eq!(edit.before.previous, edit.after.previous);
    assert!(edit.after.previous.is_some());
    assert_eq!(edit.after.len, 0);
}

#[test]
fn retained_request_requires_all_exact_host_bindings_and_closed_schema() {
    let c = case();
    let value: Value = serde_json::from_slice(&c.request.to_bytes().unwrap()).unwrap();
    for pointer in [
        "",
        "/capabilities",
        "/tool",
        "/request",
        "/request/request",
        "/request/request/sources/0",
    ] {
        let mut v = value.clone();
        v.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("accepted".into(), json!(true));
        assert!(
            OwnedAuthorRequestV1::parse_for(&bytes(&v), &c.snapshot, &c.domain).is_err(),
            "{pointer}"
        );
    }
    for key in [
        "encoding",
        "version",
        "request",
        "snapshot",
        "tool",
        "current_source_index",
        "current_capture",
        "capabilities",
    ] {
        let mut v = value.clone();
        v.as_object_mut().unwrap().remove(key);
        assert!(
            OwnedAuthorRequestV1::parse_for(&bytes(&v), &c.snapshot, &c.domain).is_err(),
            "{key}"
        );
    }
    for (pointer, replacement) in [
        ("/version", json!(2)),
        ("/current_source_index", json!(99)),
        ("/current_capture", json!(c.candidate.artifact.raw_digest)),
        (
            "/tool/first_program",
            json!(c.candidate.artifact.raw_digest),
        ),
        ("/snapshot", json!(c.candidate.artifact.raw_digest)),
        ("/capabilities/lowering", json!(99)),
        ("/capabilities/runtime", json!("invented runtime")),
    ] {
        let mut v = value.clone();
        *v.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            OwnedAuthorRequestV1::parse_for(&bytes(&v), &c.snapshot, &c.domain).is_err(),
            "{pointer}"
        );
    }
    let raw = String::from_utf8(bytes(&value))
        .unwrap()
        .replace("\"version\":1", "\"version\":1,\"version\":1");
    assert!(OwnedAuthorRequestV1::parse_for(raw.as_bytes(), &c.snapshot, &c.domain).is_err());
    for pointer in [
        "/suggestions/0/raw_digest",
        "/suggestions/0/edits/0/before/root",
    ] {
        let mut response = c.response.clone();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        response
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(key);
        refuse(&c, response);
    }
}
