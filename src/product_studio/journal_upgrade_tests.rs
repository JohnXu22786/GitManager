//! Fictional v1 restart records; no real provider completion or consent is invented.
use super::upgrade::{self, Outcome, TestPoint};
use super::*;
use crate::product_discovery::ExactUtf8DevelopmentRequest;
use serde_json::{json, Value};
use std::{
    fs,
    sync::{mpsc, Mutex},
    time::{Duration, Instant},
};
#[path = "../../tests/support/egui_harness.rs"]
mod egui_harness;
#[path = "../../tests/fixtures/product_runtime/mod.rs"]
mod fixture;

fn root() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = fs::canonicalize(temp.path()).unwrap();
    (temp, path)
}
fn digest(text: &str) -> Digest {
    canonical_digest(IdentityDomain::Evidence, &text).unwrap()
}
fn source() -> CapturedProgram {
    let mut app = fixture::organizer();
    app["label"] = json!("Café 中文");
    let raw = format!("\r\n {} \t", serde_json::to_string_pretty(&app).unwrap());
    CapturedProgram::capture(
        raw.as_bytes(),
        "runtime-project",
        Producer::Fixture {
            name: "frozen migration fixture".into(),
        },
        None,
    )
    .unwrap()
}
fn tool(root: &Path) -> Association {
    Association {
        path: root.join("saved-tool"),
        identity: ToolIdentity {
            project_id: "runtime-project".into(),
            first_program: digest("first source"),
        },
    }
}
fn basis(source: &CapturedProgram) -> Basis {
    Basis {
        snapshot: digest("snapshot"),
        source: canonical_digest(IdentityDomain::Source, source).unwrap(),
        revision: 7,
        data_generation: 9,
        data: digest("data"),
        session: digest("session"),
        decisions: fixture::decisions().identity().unwrap(),
        day: 20000,
        runtime: crate::product_runtime::RUNTIME_VERSION.into(),
        driver: crate::product_runtime::DRIVER_VERSION.into(),
    }
}
fn scope() -> DecisionScope {
    DecisionScope {
        operations: ["collect".into()].into(),
        population: Population::All,
        conditions: Values::new(),
        excluded_records: vec![],
        unknowns: vec![],
    }
}
fn request(operation: DevelopmentOperation) -> DevelopmentRequest {
    let source = source();
    let basis = basis(&source);
    DevelopmentRequest {
        version: 1,
        id: "original-request".into(),
        project_id: "runtime-project".into(),
        operation,
        request: "Keep my original need".into(),
        sources: if operation == DevelopmentOperation::Generate {
            vec![]
        } else {
            vec![source]
        },
        context: DevelopmentContext {
            view: Some("people".into()),
            selected: vec![],
            recent_inputs: vec![],
            data_digest: Some(basis.data),
            session_digest: Some(basis.session),
        },
        examples: vec![],
        accepted_scenes: vec![],
        decisions: fixture::decisions(),
        unknowns: vec![],
        required_capabilities: Default::default(),
    }
}
fn provider(request: DevelopmentRequest) -> ProviderAssociation {
    let options = ProviderOptions {
        provider: ProviderKind::Claude,
        profile: CapabilityProfile::TrustedHarness,
        ..Default::default()
    };
    let wire = encode_request(&request, &options).unwrap();
    ProviderAssociation {
        request,
        provider: options.provider,
        profile: options.profile,
        wire_request: wire.digest().unwrap(),
        wire_source: wire.source_digest,
        issued: true,
        modify: None,
        reconcile: None,
    }
}
fn journal(root: &Path, operation: DevelopmentOperation) -> Journal {
    let mut request = request(operation);
    let tool = tool(root);
    let mut basis = basis(&source());
    let mut reconcile = None;
    if operation == DevelopmentOperation::Reconcile {
        let source = request.sources[0].clone();
        for (index, need) in ["need-one", "need-two"].into_iter().enumerate() {
            let mut scene = fixture::scenario(
                &source,
                vec![SemanticInput::Observe {
                    point: format!("point-{index}"),
                }],
            );
            scene.id = format!("scene-{index}");
            let hash = scene.identity().unwrap();
            let observed = LocalRuntime::default()
                .replay(
                    &source,
                    &scene,
                    &fixture::decisions(),
                    RuntimeLimits::default(),
                    &format!("run-{index}"),
                )
                .unwrap();
            request.decisions.decisions.push(ScopedDecision {
                id: need.into(),
                revision: 1,
                request: "Keep this concrete example".into(),
                rationale: None,
                scope: scope(),
                outcome: DecisionOutcome::BothNeeded,
                status: DecisionStatus::Pending,
                obligations: vec![],
                scenarios: vec![hash.clone()],
                witness: digest(need),
                supersedes: vec![],
            });
            request.accepted_scenes.push(AcceptedSceneContext {
                decision: need.into(),
                source: source.artifact.clone(),
                scenario: hash,
                observations: observed.observations,
                disclosure: Disclosure::Synthetic,
            });
            request.examples.push(SelectedScenario {
                disclosure: Disclosure::Synthetic,
                scenario: scene,
            });
        }
        basis.decisions = request.decisions.identity().unwrap();
        request.request = "Keep my original need\nHost adoption operation ID: adoption-op\nReturn the executable evolution using this exact evolution suggestion ID: evolution-id\nPreserve both accepted needs: need-one, need-two. Return an executable design, explicit input mappings and exact proposed retirements. Preserve every independent obligation.".into();
        reconcile = Some(ReconcileAssociation {
            tool: tool.clone(),
            basis: basis.clone(),
            needs: vec!["need-one".into(), "need-two".into()],
            evolution: "evolution-id".into(),
            operation: "adoption-op".into(),
        });
    }
    let mut association = provider(request);
    association.modify = (operation == DevelopmentOperation::Modify).then_some((tool, basis));
    association.reconcile = reconcile;
    Journal {
        need: "Keep my original need".into(),
        provider: Some(association),
        ..Default::default()
    }
}
fn legacy_value(value: &Journal) -> Value {
    // Explicit frozen outer v1 inventory; provider request remains the old domain shape.
    json!({ "magic":"gitmanager.generated-tool-host", "version":1,
        "need":value.need, "last":value.last, "pending":value.pending,
        "provider":value.provider, "abandoned_creation":value.abandoned_creation,
        "last_unsaved":value.last_unsaved, "local_file":value.local_file,
        "recovery_handoffs":value.recovery_handoffs, "task":value.task })
}
fn install(root: &Path, bytes: &[u8]) {
    fs::create_dir_all(root.join("studio")).unwrap();
    fs::write(root.join("studio/session.json"), bytes).unwrap();
}
fn legacy(root: &Path, value: &Journal) -> Vec<u8> {
    value.validate().unwrap();
    let bytes = serde_json::to_vec_pretty(&legacy_value(value)).unwrap();
    install(root, &bytes);
    bytes
}
fn backup(root: &Path, old: &[u8]) -> PathBuf {
    root.join("studio").join(format!(
        "session-v1-{}.json",
        crate::product_provider::digest(old)
    ))
}
fn unchanged(root: &Path, old: &[u8]) {
    assert_eq!(fs::read(root.join("studio/session.json")).unwrap(), old);
}
fn migrated(root: &Path, expected: &Journal) -> Vec<u8> {
    let outcome = upgrade::run(root, |_| {}).unwrap();
    let Outcome::RestartRequired { digest } = outcome else {
        panic!("legacy upgrade did not request fresh reopening")
    };
    let fresh = JournalFile::reopen(root, Some(&digest)).unwrap();
    assert_eq!(
        canonical_bytes(&fresh.value).unwrap(),
        canonical_bytes(expected).unwrap()
    );
    let bytes = fs::read(root.join("studio/session.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&bytes).unwrap()["version"],
        2
    );
    bytes
}

#[test]
fn fresh_and_current_only_sessions_keep_the_whole_record_and_read_only_never_migrates() {
    let (_temp, root) = root();
    let missing = root.join("missing");
    assert!(JournalFile::read_only(&missing).is_err());
    assert!(!missing.exists());
    let file = JournalFile::open(&root).unwrap();
    let current = fs::read(root.join("studio/session.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&current).unwrap()["version"],
        2
    );
    drop(file);
    assert!(matches!(
        upgrade::run(&root, |_| {}).unwrap(),
        Outcome::Current
    ));
    unchanged(&root, &current);
    let old = legacy(&root, &Journal::default());
    fs::remove_file(root.join("studio/.write.lock")).unwrap();
    let names: Vec<_> = fs::read_dir(root.join("studio"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(JournalFile::read_only(&root)
        .err()
        .unwrap()
        .contains("startup upgrade"));
    assert!(
        task_source_machine::execute(&root, "inspect", "task-id", "request-id")
            .err()
            .unwrap()
            .contains("startup upgrade")
    );
    assert_eq!(
        fs::read_dir(root.join("studio"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect::<Vec<_>>(),
        names
    );
    assert!(JournalFile::open(&root)
        .err()
        .unwrap()
        .contains("startup upgrade"));
    unchanged(&root, &old);
    assert!(!backup(&root, &old).exists());
}

#[test]
fn original_generate_modify_reconcile_domain_and_legacy_wire_identities_survive() {
    for operation in [
        DevelopmentOperation::Generate,
        DevelopmentOperation::Modify,
        DevelopmentOperation::Reconcile,
    ] {
        for issued in [false, true] {
            let (_temp, root) = root();
            let mut value = journal(&root, operation);
            value.provider.as_mut().unwrap().issued = issued;
            let old = legacy(&root, &value);
            let current = migrated(&root, &value);
            let parsed: Value = serde_json::from_slice(&current).unwrap();
            let encoded: ExactUtf8DevelopmentRequest =
                serde_json::from_value(parsed["provider"]["request"].clone()).unwrap();
            assert_eq!(
                encoded.to_request().unwrap(),
                value.provider.as_ref().unwrap().request
            );
            assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
            assert!(parsed.get("response").is_none());
            assert!(parsed["provider"].get("result").is_none());
            assert!(matches!(
                upgrade::run(&root, |_| {}).unwrap(),
                Outcome::Current
            ));
            unchanged(&root, &current);
        }
    }
}
fn interrupted(root: &Path) -> Vec<Interrupted> {
    let source = source();
    let tool = tool(root);
    let basis = basis(&source);
    let plan = AdoptionPlan {
        version: 1,
        id: "change-op".into(),
        project_id: "runtime-project".into(),
        expected_generation: basis.data_generation,
        expected_data: basis.data.clone(),
        expected_decisions: basis.decisions.clone(),
        expected_session: basis.session.clone(),
        current_source: source.binding.clone(),
        target: source.artifact.clone(),
        scope: scope(),
        compatibility: CompatibilityReport {
            state: CompatibilityState::Compatible,
            current_data: basis.data.clone(),
            target_program: source.artifact.program_digest,
            retained_records: vec![],
            retained_events: vec![],
            retained_fields: vec![],
            issues: vec![],
        },
        required_decisions: vec![],
        checks: vec![],
        evidence: vec![],
        retire_decisions: vec![],
    };
    vec![
        Interrupted::Create { tool: tool.clone() },
        Interrupted::Daily {
            tool: tool.clone(),
            basis: basis.clone(),
            operation: "daily-op".into(),
            input: fixture::add("Unconfirmed entry"),
        },
        Interrupted::Change { tool, basis, plan },
    ]
}
#[test]
fn every_interrupted_kind_and_all_sixteen_recovery_handoffs_survive_exactly() {
    let (_temp, root) = root();
    let kinds = interrupted(&root);
    for pending in &kinds {
        let mut value = Journal {
            need: "Original wording".into(),
            pending: Some(pending.clone()),
            last: Some(tool(&root)),
            abandoned_creation: Some(tool(&root)),
            last_unsaved: Some(UnsavedInput {
                tool: tool(&root),
                operation: "unsaved-op".into(),
                input: fixture::add("Kept locally"),
                summary: "Not saved".into(),
                explanation: "Original explanation".into(),
            }),
            local_file: Some(FileAttempt {
                operation: "file-op".into(),
                tool: Some(tool(&root)),
                basis: Some(basis(&source())),
                destination: root.join("kept.csv"),
                target: FileTarget::Output {
                    inventory_index: 2,
                    artifact: digest("artifact"),
                    bytes_digest: digest("bytes"),
                    byte_count: 99,
                },
            }),
            ..Default::default()
        };
        value.recovery_handoffs = (0..16)
            .map(|n| RecoveryHandoff {
                operation: format!("recovered-{n}"),
                original: kinds[n % 3].clone(),
                recovered: Association {
                    path: root.join(format!("recovered-{n}")),
                    ..tool(&root)
                },
            })
            .collect();
        let old = legacy(&root, &value);
        migrated(&root, &value);
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
    }
}
fn task(root: &Path, ticket: bool, launched: bool, completed: bool) -> TaskAssociation {
    let mut request = request(DevelopmentOperation::Modify);
    let mut source = request.sources.remove(0);
    source.binding.task = Some(TaskSource {
        task_id: "task-id".into(),
        source_fingerprint: "original complete source fingerprint".into(),
    });
    source.binding.producer = Producer::ExternalAuthor {
        description: "Fictional original task author".into(),
    };
    request.sources.push(source.clone());
    let basis = basis(&source);
    let disclosure =
        task_source_host::Disclosure::new(request.clone(), ProviderKind::Codex).unwrap();
    let mut pending =
        serde_json::to_value(task_source_flow::PendingExternal::unissued(request.clone()).unwrap())
            .unwrap();
    if ticket {
        pending["ticket"] = json!({ "version":1, "project_id":"runtime-project", "task_id":"task-id", "created_at":"2026-01-01T00:00:00Z", "repository":root.join("repository"), "worktree":root.join("worktree"), "directory":root.join("external-task-jobs/original-request"), "request_digest":request.identity().unwrap(), "request_bytes_digest":canonical_digest(IdentityDomain::Request, &serde_json::to_vec(&request).unwrap()).unwrap() });
    }
    let external = ExternalAssociation {
        pending: serde_json::from_value(pending).unwrap(),
        basis,
        provider: ProviderKind::Codex,
        disclosure: disclosure.digest,
        approval_operation: "original-approval".into(),
        launch_attempted: launched,
    };
    TaskAssociation {
        tool: tool(root),
        task_id: "task-id".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        repository: root.join("repository"),
        worktree: root.join("worktree"),
        external: (!completed).then_some(external.clone()),
        completed: completed.then_some(CompletedTask {
            external,
            capture: source,
        }),
    }
}
#[test]
fn opaque_task_pending_ticket_launch_and_completed_external_author_stay_exact() {
    for (ticket, launched, completed) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (true, true, true),
    ] {
        let (_temp, root) = root();
        let value = Journal {
            task: Some(task(&root, ticket, launched, completed)),
            ..Default::default()
        };
        let old = legacy(&root, &value);
        let current = migrated(&root, &value);
        assert_eq!(
            serde_json::from_slice::<Value>(&current).unwrap()["task"],
            legacy_value(&value)["task"]
        );
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
    }
}
#[test]
fn original_task_and_provider_validation_still_rejects_correlated_tampering() {
    for field in [
        "task",
        "source",
        "basis",
        "approval",
        "disclosure",
        "contradictory",
        "completed-launch",
    ] {
        let (_temp, root) = root();
        let mut value = Journal {
            task: Some(task(&root, true, true, true)),
            ..Default::default()
        };
        let task = value.task.as_mut().unwrap();
        let external = &mut task.completed.as_mut().unwrap().external;
        match field {
            "task" => task.task_id = "different-task".into(),
            "source" => {
                task.completed.as_mut().unwrap().capture.binding.project_id =
                    "different-project".into()
            }
            "basis" => external.basis.data = digest("different"),
            "approval" => external.approval_operation = "bad approval".into(),
            "disclosure" => external.disclosure = digest("different"),
            "contradictory" => task.external = Some(external.clone()),
            _ => external.launch_attempted = false,
        }
        let bytes = serde_json::to_vec(&legacy_value(&value)).unwrap();
        install(&root, &bytes);
        assert!(upgrade::run(&root, |_| {}).is_err(), "{field}");
        unchanged(&root, &bytes);
        assert!(!backup(&root, &bytes).exists());
    }
    for field in ["wire_request", "wire_source"] {
        let (_temp, root) = root();
        let mut value = legacy_value(&journal(&root, DevelopmentOperation::Modify));
        value["provider"][field] = json!("0".repeat(64));
        let bytes = serde_json::to_vec(&value).unwrap();
        install(&root, &bytes);
        assert!(upgrade::run(&root, |_| {}).is_err());
        unchanged(&root, &bytes);
    }
}

#[test]
fn strict_whole_record_corruption_is_rejected_without_any_replacement() {
    for version in [1, 2] {
        let value = if version == 1 {
            legacy_value(&Journal::default())
        } else {
            serde_json::from_slice(&format::encode(&Journal::default()).unwrap()).unwrap()
        };
        let original = serde_json::to_vec(&value).unwrap();
        let text = String::from_utf8(original.clone()).unwrap();
        let mut unknown = value.clone();
        unknown["unexpected"] = json!(true);
        let mut newer = value.clone();
        newer["version"] = json!(999);
        let mut invalid_utf8 = original.clone();
        invalid_utf8.push(0xff);
        let duplicate = text
            .replacen("\"magic\":", "\"ma\\u0067ic\":\"duplicate\",\"magic\":", 1)
            .into_bytes();
        let deep = format!("{{\"extra\":{}0{}}}", "[".repeat(65), "]".repeat(65)).into_bytes();
        for bad in [
            serde_json::to_vec(&unknown).unwrap(),
            serde_json::to_vec(&newer).unwrap(),
            original[..original.len() - 1].to_vec(),
            [original.clone(), b" false".to_vec()].concat(),
            invalid_utf8,
            duplicate,
            deep,
        ] {
            let (_temp, root) = root();
            install(&root, &bad);
            assert!(upgrade::run(&root, |_| {}).is_err(), "version {version}");
            unchanged(&root, &bad);
            assert!(!backup(&root, &bad).exists());
            assert!(JournalFile::read_only(&root).is_err());
        }
    }
}
#[test]
fn exact_request_marker_digest_source_and_semantic_purpose_are_checked() {
    for field in ["encoding", "digest", "source", "purpose"] {
        let (_temp, root) = root();
        let value = journal(&root, DevelopmentOperation::Reconcile);
        let mut stored: Value = serde_json::from_slice(&format::encode(&value).unwrap()).unwrap();
        let envelope = &mut stored["provider"]["request"];
        match field {
            "encoding" => envelope["encoding"] = json!("unknown"),
            "digest" => envelope["request_digest"] = serde_json::to_value(digest("wrong")).unwrap(),
            "source" => {
                envelope["request"]["sources"][0]["program"]["label"] = json!("different AST")
            }
            _ => {
                let mut request = value.provider.as_ref().unwrap().request.clone();
                request.operation = DevelopmentOperation::Modify;
                *envelope = serde_json::to_value(
                    ExactUtf8DevelopmentRequest::from_request(&request).unwrap(),
                )
                .unwrap();
                assert!(format::decode(&serde_json::to_vec(&stored).unwrap())
                    .err()
                    .unwrap()
                    .contains("generation association"));
            }
        }
        let bytes = serde_json::to_vec(&stored).unwrap();
        install(&root, &bytes);
        assert!(JournalFile::read_only(&root).is_err());
        assert!(upgrade::run(&root, |_| {}).is_err());
        unchanged(&root, &bytes);
    }
}
fn padded(root: &Path, target: usize, legacy: bool) -> Journal {
    let mut value = journal(root, DevelopmentOperation::Generate);
    let arguments: Values = (0..MAX_ITEMS)
        .map(|i| (format!("field-{i}"), fixture::string("")))
        .collect();
    value.last_unsaved = Some(UnsavedInput {
        tool: tool(root),
        operation: "retained-input".into(),
        input: SemanticInput::Invoke {
            action: "retain".into(),
            arguments,
        },
        summary: "Original input".into(),
        explanation: "Never dropped to make a request fit".into(),
    });
    let length = if legacy {
        canonical_bytes(&legacy_value(&value)).unwrap().len()
    } else {
        canonical_bytes(&format::StoredJournalV2::from_domain(&value).unwrap())
            .unwrap()
            .len()
    };
    let mut remaining = target.checked_sub(length).unwrap();
    let SemanticInput::Invoke { arguments, .. } = &mut value.last_unsaved.as_mut().unwrap().input
    else {
        unreachable!()
    };
    for entry in arguments.values_mut() {
        let size = remaining.min(MAX_TEXT_BYTES);
        *entry = fixture::string(&"x".repeat(size));
        remaining -= size;
    }
    assert_eq!(remaining, 0);
    value.validate().unwrap();
    value
}
#[test]
fn complete_current_record_cap_is_inclusive_and_overflow_never_evicts_metadata() {
    let (_temp, root) = root();
    let value = padded(&root, JOURNAL_LIMIT, false);
    let bytes = format::encode(&value).unwrap();
    assert_eq!(bytes.len(), JOURNAL_LIMIT);
    let mut file = JournalFile::open(&root).unwrap();
    file.write(value.clone()).unwrap();
    assert_eq!(
        canonical_bytes(&JournalFile::read_only(&root).unwrap()).unwrap(),
        canonical_bytes(&value).unwrap()
    );
    // Grow separate retained metadata; keep the original Generate request binding.
    let mut too_big = value.clone();
    too_big.last_unsaved.as_mut().unwrap().explanation.push('x');
    assert!(file.write(too_big).is_err());
    unchanged(&root, &bytes);
    drop(file);
    let mut plus_one = bytes;
    plus_one.push(b' ');
    install(&root, &plus_one);
    assert!(JournalFile::read_only(&root).is_err());
    assert!(upgrade::run(&root, |_| {}).is_err());
    unchanged(&root, &plus_one);
}
#[test]
fn valid_full_v1_expanding_past_v2_cap_is_kept_without_backup_or_eviction() {
    let (_temp, root) = root();
    let value = padded(&root, JOURNAL_LIMIT, true);
    let old = canonical_bytes(&legacy_value(&value)).unwrap();
    assert_eq!(old.len(), JOURNAL_LIMIT);
    assert!(
        canonical_bytes(&format::StoredJournalV2::from_domain(&value).unwrap())
            .unwrap()
            .len()
            > JOURNAL_LIMIT
    );
    install(&root, &old);
    assert!(upgrade::run(&root, |_| {})
        .err()
        .unwrap()
        .contains("bounded restart record"));
    unchanged(&root, &old);
    assert!(!backup(&root, &old).exists());
}
#[test]
fn maximum_sized_legacy_intake_and_one_extra_byte_have_distinct_outcomes() {
    for excess in [false, true] {
        let (_temp, root) = root();
        let value = Journal::default();
        let mut old = canonical_bytes(&legacy_value(&value)).unwrap();
        old.resize(JOURNAL_LIMIT + usize::from(excess), b' ');
        install(&root, &old);
        if excess {
            assert!(upgrade::run(&root, |_| {}).is_err());
            unchanged(&root, &old);
        } else {
            migrated(&root, &value);
            assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
        }
    }
}
#[test]
fn preactivation_faults_keep_original_and_restart_reuses_only_exact_backup() {
    for fault in [
        TestPoint::BeforeBackup,
        TestPoint::AfterBackup,
        TestPoint::BeforePublish,
    ] {
        let (_temp, root) = root();
        let value = journal(&root, DevelopmentOperation::Modify);
        let old = legacy(&root, &value);
        let hook = upgrade::test_hook(&root, move |point| {
            if point == fault {
                Err("simulated interruption".into())
            } else {
                Ok(())
            }
        });
        assert!(upgrade::run(&root, |_| {}).is_err());
        unchanged(&root, &old);
        drop(hook);
        migrated(&root, &value);
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
    }
}
#[test]
fn lost_activation_acknowledgement_is_reconciled_without_rollback() {
    let (_temp, root) = root();
    let value = journal(&root, DevelopmentOperation::Modify);
    let old = legacy(&root, &value);
    let hook = upgrade::test_hook(&root, |point| {
        if point == TestPoint::AfterRename {
            Err("lost rename acknowledgement".into())
        } else {
            Ok(())
        }
    });
    let current = migrated(&root, &value);
    drop(hook);
    assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
    assert!(matches!(
        upgrade::run(&root, |_| {}).unwrap(),
        Outcome::Current
    ));
    unchanged(&root, &current);
}
#[test]
fn postactivation_readback_or_sync_uncertainty_blocks_then_reopens_exact_v2() {
    for fault in [TestPoint::BeforeReadback, TestPoint::BeforeSync] {
        let (_temp, root) = root();
        let value = journal(&root, DevelopmentOperation::Generate);
        let old = legacy(&root, &value);
        let hook = upgrade::test_hook(&root, move |point| {
            if point == fault {
                Err("simulated durability uncertainty".into())
            } else {
                Ok(())
            }
        });
        let error = upgrade::run(&root, |_| {}).err().unwrap();
        assert!(
            error.contains("activated") || error.contains("uncertain"),
            "{error}"
        );
        let current = fs::read(root.join("studio/session.json")).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&current).unwrap()["version"],
            2
        );
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
        drop(hook);
        assert!(matches!(
            upgrade::run(&root, |_| {}).unwrap(),
            Outcome::Current
        ));
        unchanged(&root, &current);
        assert_eq!(
            canonical_bytes(&JournalFile::open(&root).unwrap().value).unwrap(),
            canonical_bytes(&value).unwrap()
        );
    }
}
#[test]
fn unequal_or_unsafe_backup_and_no_clobber_races_block_without_overwrite() {
    for race in [false, true] {
        for same in [false, true] {
            let (_temp, root) = root();
            let value = Journal::default();
            let old = legacy(&root, &value);
            let path = backup(&root, &old);
            let contents = if same {
                old.clone()
            } else {
                b"different backup bytes".to_vec()
            };
            let hook = if race {
                let path = path.clone();
                let contents = contents.clone();
                Some(upgrade::test_hook(&root, move |point| {
                    if point == TestPoint::BeforeBackupPublish {
                        fs::write(&path, &contents).unwrap();
                    }
                    Ok(())
                }))
            } else {
                fs::write(&path, &contents).unwrap();
                None
            };
            let result = upgrade::run(&root, |_| {});
            assert_eq!(result.is_ok(), same, "race={race} same={same}");
            if !same {
                unchanged(&root, &old);
            }
            assert_eq!(fs::read(path).unwrap(), contents);
            drop(hook);
        }
    }
    let (_temp, root) = root();
    let old = legacy(&root, &Journal::default());
    fs::create_dir(backup(&root, &old)).unwrap();
    assert!(upgrade::run(&root, |_| {}).is_err());
    unchanged(&root, &old);
}
#[test]
fn writer_contention_and_directory_replacement_preserve_original_records() {
    let (_temp, root) = root();
    let old = legacy(&root, &Journal::default());
    let folder = Folder::open(&root.join("studio")).unwrap();
    let lock = folder.lock().unwrap();
    assert!(upgrade::run(&root, |_| {}).err().unwrap().contains("busy"));
    unchanged(&root, &old);
    drop(lock);
    drop(folder);
    let original = root.join("studio");
    let moved = root.join("original-studio");
    let hook = upgrade::test_hook(&root, {
        let original = original.clone();
        let moved = moved.clone();
        move |point| {
            if point == TestPoint::BeforeBackup {
                fs::rename(&original, &moved).unwrap();
                fs::create_dir(&original).unwrap();
            }
            Ok(())
        }
    });
    assert!(upgrade::run(&root, |_| {}).is_err());
    assert_eq!(fs::read(moved.join("session.json")).unwrap(), old);
    assert!(!original.join("session.json").exists());
    assert!(!backup(&root, &old).exists());
    drop(hook);
}
#[cfg(unix)]
#[test]
fn linked_and_special_session_or_backup_are_never_followed_or_replaced() {
    use std::os::unix::fs::symlink;
    for target in ["session", "backup"] {
        for kind in ["symlink", "hardlink", "directory"] {
            let (_temp, root) = root();
            let old = legacy(&root, &Journal::default());
            let file = if target == "session" {
                root.join("studio/session.json")
            } else {
                backup(&root, &old)
            };
            let separate = root.join("unchanged-original");
            fs::write(&separate, &old).unwrap();
            if target == "session" {
                fs::remove_file(&file).unwrap();
            }
            match kind {
                "symlink" => symlink(&separate, &file).unwrap(),
                "hardlink" => fs::hard_link(&separate, &file).unwrap(),
                _ => fs::create_dir(&file).unwrap(),
            }
            assert!(upgrade::run(&root, |_| {}).is_err(), "{target}/{kind}");
            assert_eq!(fs::read(&separate).unwrap(), old);
            if target == "backup" {
                unchanged(&root, &old);
            }
            if kind == "symlink" {
                assert!(fs::symlink_metadata(file).unwrap().file_type().is_symlink());
            }
        }
    }
}

fn settle(studio: &mut ProductStudio) {
    let start = Instant::now();
    loop {
        studio.poll();
        if !studio.is_busy() {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "{}",
            studio.test_notice()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn stop(studio: ProductStudio, hooks: &TestHooks) {
    drop(studio);
    let start = Instant::now();
    while !hooks.stopped.load(Ordering::Acquire) {
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn boot_progress_blocks_mutation_and_releases_migration_before_fresh_open_and_reconciliation() {
    let (_temp, root) = root();
    let mut value = journal(&root, DevelopmentOperation::Generate);
    value.provider.as_mut().unwrap().issued = false;
    let old = legacy(&root, &value);
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let released = Arc::new(AtomicBool::new(false));
    let hook = upgrade::test_hook(&root, {
        let root = root.clone();
        let released = released.clone();
        move |point| {
            if point == TestPoint::Released {
                let folder = Folder::open(&root.join("studio")).unwrap();
                let lock = folder
                    .lock()
                    .expect("migration lock must be gone at return");
                drop(lock);
                released.store(true, Ordering::Release);
            }
            if matches!(
                point,
                TestPoint::Stage(_) | TestPoint::BeforeOpen | TestPoint::Opened
            ) {
                if point == TestPoint::BeforeOpen {
                    assert!(released.load(Ordering::Acquire));
                    let folder = Folder::open(&root.join("studio")).unwrap();
                    let lock = folder
                        .lock()
                        .expect("fresh session must reacquire rather than inherit a lock");
                    drop(lock);
                }
                if point == TestPoint::Opened {
                    let folder = Folder::open(&root.join("studio")).unwrap();
                    assert!(
                        folder.lock().is_err(),
                        "fresh session must own its new writer lock"
                    );
                    assert!(
                        JournalFile::read_only(&root).unwrap().provider.is_some(),
                        "reconciliation must wait for the fresh open"
                    );
                }
                reached_tx.send(point).unwrap();
                release_rx
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(20))
                    .unwrap();
            }
            Ok(())
        }
    });
    let hooks = TestHooks::default();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks.clone());
    let mut harness = egui_harness::EguiHarness::new(egui::vec2(1200.0, 1600.0));
    let labels = [
        (6, "Updating saved restart information"),
        (7, "Restart information staged locally"),
        (8, "Exact requests and interrupted attempts verified"),
        (9, "Activating restart information"),
        (10, "Reopening a fresh host session"),
    ];
    for expected in [
        TestPoint::Stage(6),
        TestPoint::Stage(7),
        TestPoint::Stage(8),
        TestPoint::Stage(9),
        TestPoint::BeforeOpen,
        TestPoint::Opened,
    ] {
        let actual = reached_rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(actual, expected);
        studio.poll();
        assert!(studio.is_busy());
        assert!(studio.mutation_blocked);
        assert!(studio.test_generation_blocked());
        assert!(!studio.test_cancel());
        if expected == TestPoint::Stage(8) {
            studio.test_close();
            assert!(
                studio.is_busy(),
                "closing panels cannot cancel committed startup"
            );
        }
        let stage = if let TestPoint::Stage(stage) = expected {
            stage
        } else {
            10
        };
        let trace = harness.frame(|ctx| {
            egui::CentralPanel::default()
                .show(ctx, |ui| studio.show(ui))
                .inner
        });
        assert!(
            trace
                .text
                .iter()
                .any(|text| text.contains(labels.iter().find(|(n, _)| *n == stage).unwrap().1)),
            "stage {stage}: {:?}",
            trace.text
        );
        assert!(!trace.controls["studio.cancel"].enabled);
        release_tx.send(()).unwrap();
    }
    settle(&mut studio);
    assert!(!studio.mutation_blocked);
    assert!(!studio.test_generation_blocked());
    assert!(studio.test_notice().contains("fresh host session"));
    assert!(studio.test_notice().contains("never authorized or sent"));
    assert!(
        JournalFile::read_only(&root).unwrap().provider.is_none(),
        "ordinary unissued policy must still run after fresh open"
    );
    assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
    assert!(!root.join("provider-jobs").exists());
    drop(hook);
    stop(studio, &hooks);
}
#[test]
fn fresh_reopen_contention_or_changed_activation_keeps_boot_blocked() {
    for change in [false, true] {
        let (_temp, root) = root();
        let old = legacy(&root, &Journal::default());
        let held = Arc::new(Mutex::new(None));
        let hook = upgrade::test_hook(&root, {
            let root = root.clone();
            let held = held.clone();
            move |point| {
                if point == TestPoint::Released {
                    let folder = Folder::open(&root.join("studio")).unwrap();
                    let lock = folder.lock().unwrap();
                    if change {
                        let mut value = Journal::default();
                        value.need = "Changed by another writer".into();
                        folder
                            .publish("session.json", &format::encode(&value).unwrap(), true)
                            .unwrap();
                    } else {
                        *held.lock().unwrap() = Some(lock);
                    }
                }
                Ok(())
            }
        });
        let hooks = TestHooks::default();
        let mut studio = ProductStudio::testing(root.clone(), None, hooks.clone());
        settle(&mut studio);
        assert!(studio.test_generation_blocked());
        assert!(studio.mutation_blocked);
        assert!(
            studio.test_notice().contains("fresh host session"),
            "{}",
            studio.test_notice()
        );
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
        assert_eq!(
            serde_json::from_slice::<Value>(&fs::read(root.join("studio/session.json")).unwrap())
                .unwrap()["version"],
            2
        );
        drop(hook);
        drop(held.lock().unwrap().take());
        stop(studio, &hooks);
    }
}
#[test]
fn failed_startup_keeps_saved_work_and_reports_the_upgrade_failure() {
    let (_temp, root) = root();
    let old = legacy(&root, &Journal::default());
    fs::write(backup(&root, &old), b"unequal retained copy").unwrap();
    let hooks = TestHooks::default();
    let mut studio = ProductStudio::testing(root.clone(), None, hooks.clone());
    settle(&mut studio);
    assert!(studio.mutation_blocked);
    assert!(studio.test_generation_blocked());
    assert!(
        studio.test_notice().contains("backup"),
        "{}",
        studio.test_notice()
    );
    unchanged(&root, &old);
    stop(studio, &hooks);
}

#[test]
fn migrated_issued_request_never_resends_or_imports_a_late_file_on_repeated_boot() {
    let (_temp, root) = root();
    let value = journal(&root, DevelopmentOperation::Generate);
    let old = legacy(&root, &value);
    let jobs = root.join("fixture-jobs");
    let transport = ProviderTransport::new_fixture(
        jobs.clone(),
        ProviderKind::Claude,
        root.join("never-execute-this-missing-cli"),
        root.clone(),
    )
    .unwrap();
    let job = jobs.join("original-request");
    fs::create_dir(&job).unwrap();
    let late = b"A late uncorrelated response must not become a completed result";
    fs::write(job.join("result.json"), late).unwrap();
    for _ in 0..2 {
        let hooks = TestHooks::default();
        let mut studio =
            ProductStudio::testing(root.clone(), Some(transport.clone()), hooks.clone());
        settle(&mut studio);
        assert!(studio.test_generation_blocked());
        assert_ne!(studio.test_page(), "draft");
        let current = JournalFile::read_only(&root).unwrap();
        assert_eq!(
            canonical_bytes(&current).unwrap(),
            canonical_bytes(&value).unwrap()
        );
        assert_eq!(fs::read(job.join("result.json")).unwrap(), late);
        assert!(!job.join("request.json").exists());
        assert!(!job.join("receipt.json").exists());
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
        stop(studio, &hooks);
    }
}
#[test]
fn migrated_daily_attempt_preserves_real_saved_work_and_never_auto_replays() {
    let (_temp, root) = root();
    let path = root.join("real-saved-tool");
    let store = ProductStore::create(&path, &source(), 20000).unwrap();
    super::super::test_stage_daily(&root, &path, fixture::add("Unconfirmed entry"), false);
    let expected = JournalFile::read_only(&root).unwrap();
    let before = store.load().unwrap();
    let old = legacy(&root, &expected);
    for _ in 0..2 {
        let hooks = TestHooks::default();
        let mut studio = ProductStudio::testing(root.clone(), None, hooks.clone());
        settle(&mut studio);
        assert!(studio.mutation_blocked);
        assert_eq!(store.load().unwrap(), before);
        let current = JournalFile::read_only(&root).unwrap();
        assert_eq!(
            canonical_bytes(&current.pending).unwrap(),
            canonical_bytes(&expected.pending).unwrap()
        );
        assert_eq!(fs::read(backup(&root, &old)).unwrap(), old);
        stop(studio, &hooks);
    }
}
