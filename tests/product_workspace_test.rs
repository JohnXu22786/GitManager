//! Presentation/controller-input fixtures only; not live generation or desktop acceptance.
#[path = "support/egui_harness.rs"]
mod egui_harness;
#[path = "fixtures/product_runtime/mod.rs"]
mod executed_fixture;
#[path = "support/product_contract_fixture.rs"]
mod fixture;
#[path = "../src/product_contract.rs"]
mod product_contract;
#[path = "../src/product_protocol.rs"]
mod product_protocol;
#[path = "../src/product_runtime/mod.rs"]
mod product_runtime;
#[path = "../src/ui/product_runtime_view.rs"]
mod product_runtime_view;
#[path = "../src/ui/product_workspace.rs"]
mod product_workspace;

use egui::{Key, Modifiers, Vec2};
use egui_harness::EguiHarness;
use product_contract::*;
use product_protocol::*;
use product_workspace::{ProductWorkspace, WorkspaceOutput};
use std::collections::{BTreeMap, BTreeSet};

fn digest(s: &str) -> Digest {
    canonical_digest(IdentityDomain::Program, &s).unwrap()
}
fn text(s: &str) -> DataValue {
    DataValue::Text { value: s.into() }
}
fn record(id: &str) -> RecordRef {
    RecordRef {
        entity: "person".into(),
        record: id.into(),
    }
}
fn entry() -> WorkspaceView {
    WorkspaceView {
        session_id: "workspace".into(),
        next_operation_id: "op-1".into(),
        active: None,
        comparison: None,
        scope: None,
        adoption: None,
        operation: None,
    }
}
fn runtime() -> RuntimeView {
    let program =
        AppDefinition::parse(&serde_json::to_vec(&fixture::organizer()).unwrap()).unwrap();
    RuntimeView {
        program,
        observation: ViewObservation {
            view: "people".into(),
            rows: vec![PresentedRow {
                record: record("one"),
                cells: [("name".into(), text("小明 🦉"))].into(),
                enabled_actions: BTreeSet::new(),
            }],
            controls: [("search_input".into(), text(""))].into(),
            selected: vec![],
            enabled_actions: ["export_button".into()].into(),
            form_values: BTreeMap::new(),
        },
        artifacts: vec![LocalArtifact::from_rows(
            "roster",
            OutputFormat::Csv,
            vec![FieldDefinition {
                id: "name".into(),
                label: "Name".into(),
                value_type: Type::Text,
            }],
            vec![[("name".into(), text("实际导出"))].into()],
        )
        .unwrap()],
        retained_records: vec![Record {
            entity: "person".into(),
            id: "one".into(),
            revision: 1,
            created_program: digest("one"),
            archived: false,
            values: [("name".into(), text("小明 🦉"))].into(),
        }],
        read_only: false,
        issues: vec![],
    }
}
fn daily() -> WorkspaceView {
    let mut view = entry();
    let runtime = runtime();
    view.active = Some(ActiveToolView {
        project_id: "project".into(),
        name: "My tool".into(),
        generation: 1,
        program: runtime.program.identity().unwrap(),
        data: digest("data"),
        session: digest("session"),
        runtime,
        capabilities: RuntimeCapabilities {
            adapter: "native".into(),
            version: "1".into(),
            artifact_kinds: vec![],
            features: BTreeSet::new(),
            unavailable: vec![],
        },
        decisions: vec![],
    });
    view
}
fn artifact(runtime: &RuntimeView) -> ArtifactRef {
    ArtifactRef {
        kind: ArtifactKind::GeneratedApp,
        version: 1,
        language: LANGUAGE_VERSION.into(),
        raw_digest: digest("raw"),
        program_digest: runtime.program.identity().unwrap(),
        semantic_digest: runtime.program.semantic_identity().unwrap(),
    }
}
fn comparison() -> WorkspaceView {
    let mut view = daily();
    let before = runtime();
    let mut after = before.clone();
    after.program.label = "Alternative".into();
    after.observation.rows[0]
        .cells
        .insert("name".into(), text("Changed result"));
    let side = |runtime: RuntimeView| OutcomeView {
        artifact: artifact(&runtime),
        runtime,
        state: EvidenceState::Observed,
        origin: ExecutionOrigin::TestFixture,
        evidence: Some(digest("evidence")),
        uncovered: vec!["Later work not checked".into()],
    };
    view.comparison = Some(ComparisonView {
        witness_id: "witness".into(),
        question: "Which result works for you?".into(),
        before: side(before),
        after: side(after),
        inputs: vec![SemanticInput::Control {
            view: "people".into(),
            control: "search_input".into(),
            value: text("North"),
        }],
        replay_index: 0,
        minimization: None,
        unknowns: vec!["Deletion was not checked".into()],
        stale: false,
    });
    view
}
fn scope_view() -> WorkspaceView {
    let mut view = comparison();
    view.active.as_mut().unwrap().runtime.observation.selected = vec![record("one")];
    view.scope = Some(ScopeView {
        choice_id: "choice".into(),
        witness_id: "witness".into(),
        outcome: DecisionOutcome::EitherAcceptable,
        proposed_scope: DecisionScope {
            operations: ["export_people".into()].into(),
            population: Population::NewWork,
            conditions: BTreeMap::new(),
            excluded_records: vec![record("one")],
            unknowns: vec![UnknownBoundary {
                id: "deletion".into(),
                operations: ["archive".into()].into(),
                description: "Deletion remains undecided".into(),
            }],
        },
        rationale: None,
        impact: vec!["Completed work stays unchanged".into()],
        rehearsal_state: EvidenceState::Observed,
    });
    view
}
fn frame(
    h: &mut EguiHarness,
    state: &mut ProductWorkspace,
    view: &WorkspaceView,
) -> WorkspaceOutput {
    h.frame(|ctx| {
        egui::CentralPanel::default()
            .show(ctx, |ui| state.show(ui, view))
            .inner
    })
}
fn click(
    h: &mut EguiHarness,
    state: &mut ProductWorkspace,
    view: &WorkspaceView,
    key: &str,
) -> WorkspaceOutput {
    let result = frame(h, state, view);
    let rect = result
        .trace
        .controls
        .get(key)
        .unwrap_or_else(|| panic!("missing {key}: {:?}", result.trace.controls.keys()))
        .rect;
    h.press_at(rect.center());
    frame(h, state, view);
    h.release_at(rect.center());
    frame(h, state, view)
}
fn type_in(
    h: &mut EguiHarness,
    state: &mut ProductWorkspace,
    view: &WorkspaceView,
    key: &str,
    value: &str,
) -> WorkspaceOutput {
    click(h, state, view, key);
    h.key(Key::A, true, Modifiers::COMMAND);
    frame(h, state, view);
    h.key(Key::A, false, Modifiers::NONE);
    h.text(value);
    frame(h, state, view)
}
fn ack(view: &mut WorkspaceView, request: &WorkspaceRequest, number: u64) {
    view.operation = Some(OperationStatus {
        delivery_id: number,
        context: request.context.clone(),
        outcome: OperationOutcome::PreviewReady,
    });
    view.next_operation_id = format!("op-{}", number + 1);
}
fn setup() -> (EguiHarness, ProductWorkspace) {
    (
        EguiHarness::new(Vec2::new(1000.0, 1800.0)),
        ProductWorkspace::default(),
    )
}

#[test]
fn first_create_uses_plain_text_and_open_needs_no_path_or_json() {
    let (mut h, mut state) = setup();
    let mut view = entry();
    for c in "管理我的器材 🦉".chars() {
        click_if_first(&mut h, &mut state, &view, c == '管');
        h.text(&c.to_string());
        frame(&mut h, &mut state, &view);
    }
    let request = click(&mut h, &mut state, &view, "create").request.unwrap();
    assert_eq!(
        request.action,
        WorkspaceAction::Create {
            need: "管理我的器材 🦉".into()
        }
    );
    assert!(click(&mut h, &mut state, &view, "create").request.is_none());
    ack(&mut view, &request, 1);
    assert_eq!(
        click(&mut h, &mut state, &view, "open")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Open
    );
}
fn click_if_first(h: &mut EguiHarness, s: &mut ProductWorkspace, v: &WorkspaceView, first: bool) {
    if first {
        click(h, s, v, "need");
    }
}

#[test]
fn generated_forms_keep_each_character_and_submit_typed_arguments() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    active.runtime.observation.view = "new_person".into();
    click(&mut h, &mut state, &view, "daily.field.name");
    for c in "Zoë 李".chars() {
        h.text(&c.to_string());
        frame(&mut h, &mut state, &view);
    }
    type_in(&mut h, &mut state, &view, "daily.field.area", "North");
    let request = click(&mut h, &mut state, &view, "daily.submit")
        .request
        .unwrap();
    assert_eq!(
        request.action,
        WorkspaceAction::Daily {
            input: SemanticInput::Submit {
                view: "new_person".into(),
                arguments: [
                    ("name".into(), text("Zoë 李")),
                    ("area".into(), text("North"))
                ]
                .into()
            }
        }
    );
}

#[test]
fn filter_edits_survive_pending_input_without_losing_unicode() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let first = type_in(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input",
        "北",
    )
    .request
    .unwrap();
    h.text("方");
    assert!(frame(&mut h, &mut state, &view).request.is_none());
    view.active
        .as_mut()
        .unwrap()
        .runtime
        .observation
        .controls
        .insert("search_input".into(), text("北"));
    view.active.as_mut().unwrap().session = digest("new-session");
    ack(&mut view, &first, 1);
    let second = frame(&mut h, &mut state, &view).request.unwrap();
    assert_eq!(
        second.action,
        WorkspaceAction::Daily {
            input: SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: text("北方")
            }
        }
    );
}

#[test]
fn selection_preserves_hidden_rows_and_shortcuts_respect_enabled_and_text_focus() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    view.active.as_mut().unwrap().runtime.observation.selected = vec![record("hidden")];
    let select = click(&mut h, &mut state, &view, "daily.select.person.one")
        .request
        .unwrap();
    assert_eq!(
        select.action,
        WorkspaceAction::Daily {
            input: SemanticInput::Control {
                view: "people".into(),
                control: "pick".into(),
                value: DataValue::List {
                    item_type: Type::reference("person"),
                    items: vec![
                        DataValue::Reference {
                            entity: "person".into(),
                            record: "hidden".into()
                        },
                        DataValue::Reference {
                            entity: "person".into(),
                            record: "one".into()
                        }
                    ]
                }
            }
        }
    );
    ack(&mut view, &select, 1);
    frame(&mut h, &mut state, &view);
    h.key(Key::E, true, Modifiers::CTRL);
    assert!(matches!(
        frame(&mut h, &mut state, &view).request.unwrap().action,
        WorkspaceAction::Daily {
            input: SemanticInput::Activate { .. }
        }
    ));
    h.key(Key::E, false, Modifiers::NONE);
    frame(&mut h, &mut state, &view);
}

#[test]
fn comparison_plays_identical_inputs_displays_outputs_and_delays_scope() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let shown = frame(&mut h, &mut state, &view);
    assert!(!shown.trace.controls.contains_key("scope.rationale"));
    assert!(shown.trace.text.iter().any(|s| s.contains("实际导出")));
    assert!(shown.trace.text.iter().any(|s| s.contains("Test fixture")));
    let replay = click(&mut h, &mut state, &view, "replay").request.unwrap();
    assert_eq!(
        replay.action,
        WorkspaceAction::Replay {
            witness: "witness".into(),
            input: view.comparison.as_ref().unwrap().inputs[0].clone()
        }
    );
    ack(&mut view, &replay, 1);
    assert_eq!(
        click(&mut h, &mut state, &view, "restart")
            .request
            .unwrap()
            .action,
        WorkspaceAction::RestartComparison {
            witness: "witness".into()
        }
    );
}

#[test]
fn every_outcome_emits_its_exact_nonbinary_choice_without_adoption() {
    for (key, outcome) in [
        ("choice.keep", DecisionOutcome::KeepCurrent),
        ("choice.either", DecisionOutcome::EitherAcceptable),
        ("choice.both", DecisionOutcome::BothNeeded),
        ("choice.neither", DecisionOutcome::NeitherFits),
        ("choice.defer", DecisionOutcome::Deferred),
    ] {
        let (mut h, mut state) = setup();
        let view = comparison();
        assert_eq!(
            click(&mut h, &mut state, &view, key)
                .request
                .unwrap()
                .action,
            WorkspaceAction::Choose {
                witness: "witness".into(),
                outcome
            }
        );
    }
    for key in ["choice.before", "choice.after"] {
        let (mut h, mut state) = setup();
        let view = comparison();
        let comp = view.comparison.as_ref().unwrap();
        let expected = if key.ends_with("before") {
            &comp.before.artifact
        } else {
            &comp.after.artifact
        };
        assert_eq!(
            click(&mut h, &mut state, &view, key)
                .request
                .unwrap()
                .action,
            WorkspaceAction::Choose {
                witness: "witness".into(),
                outcome: DecisionOutcome::Accept {
                    artifact: expected.program_digest.clone()
                }
            }
        );
    }
}

#[test]
fn scope_edits_preserve_exclusions_unknowns_and_optional_rationale() {
    let (mut h, mut state) = setup();
    let view = scope_view();
    click(&mut h, &mut state, &view, "scope.population.selected");
    type_in(
        &mut h,
        &mut state,
        &view,
        "scope.rationale",
        "这次只改当前工作",
    );
    let request = click(&mut h, &mut state, &view, "scope.preview")
        .request
        .unwrap();
    let WorkspaceAction::SetScope {
        scope, rationale, ..
    } = request.action
    else {
        panic!("wrong request")
    };
    assert!(matches!(scope.population, Population::Records { .. }));
    assert_eq!(scope.operations, ["export_people".into()].into());
    assert_eq!(scope.excluded_records, vec![record("one")]);
    assert_eq!(scope.unknowns.len(), 1);
    assert_eq!(rationale.as_deref(), Some("这次只改当前工作"));
}

#[test]
fn cancel_back_close_and_late_replies_cannot_reactivate_an_old_request() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    click(&mut h, &mut state, &view, "change");
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.text",
        "Use the selected result",
    );
    let request = click(&mut h, &mut state, &view, "change.submit")
        .request
        .unwrap();
    // The controller reserves a fresh operation ID as soon as it consumes a request.
    view.next_operation_id = "op-2".into();
    let cancel = click(&mut h, &mut state, &view, "cancel").request.unwrap();
    assert_eq!(
        cancel.action,
        WorkspaceAction::Cancel {
            operation: request.context.request_id.clone()
        }
    );
    ack(&mut view, &request, 5);
    let shown = frame(&mut h, &mut state, &view);
    assert!(!shown.trace.text.iter().any(|s| s == "Preview ready"));
    assert!(click(&mut h, &mut state, &view, "back").request.is_some());
    view.session_id = "reopened".into();
    view.operation = None;
    view.next_operation_id = "op-1".into();
    assert_eq!(
        click(&mut h, &mut state, &view, "close")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Close
    );
}

#[test]
fn stale_unknown_and_identity_changes_disable_evidence_use_and_drop_old_drafts() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    view.comparison.as_mut().unwrap().stale = true;
    assert!(!frame(&mut h, &mut state, &view).trace.controls["choice.after"].enabled);
    assert!(click(&mut h, &mut state, &view, "choice.after")
        .request
        .is_none());
    view.comparison.as_mut().unwrap().stale = false;
    view.comparison.as_mut().unwrap().after.state = EvidenceState::Unsupported;
    assert!(!frame(&mut h, &mut state, &view).trace.controls["choice.after"].enabled);
    view.comparison = None;
    click(&mut h, &mut state, &view, "change");
    type_in(&mut h, &mut state, &view, "change.text", "old draft");
    view.active.as_mut().unwrap().program = digest("replacement");
    assert!(!frame(&mut h, &mut state, &view)
        .trace
        .controls
        .contains_key("change.submit"));
}

#[test]
fn narrow_layout_long_text_remains_wrapped_and_outputs_are_exported_by_digest() {
    let (mut h, mut state) = setup();
    h.resize(Vec2::new(340.0, 2200.0));
    let mut view = daily();
    view.active.as_mut().unwrap().runtime.observation.rows[0]
        .cells
        .insert("name".into(), text(&"长い名前🦉".repeat(70)));
    let shown = frame(&mut h, &mut state, &view);
    for (key, control) in &shown.trace.controls {
        assert!(control.rect.right() <= 340.5, "{key}: {:?}", control.rect);
    }
    let artifact = view.active.as_ref().unwrap().runtime.artifacts[0]
        .digest
        .clone();
    assert_eq!(
        click(&mut h, &mut state, &view, "daily.output.0.save")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Export { artifact }
    );
}

#[test]
fn typed_form_validates_dates_numbers_lists_optional_values_and_record_choices() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    let mut app = fixture::organizer();
    app["actions"][0]["parameters"]["count"] = serde_json::json!({"kind":"integer"});
    app["actions"][0]["parameters"]["due"] = serde_json::json!({"kind":"date"});
    app["actions"][0]["parameters"]["confirmed"] = serde_json::json!({"kind":"boolean"});
    app["actions"][0]["parameters"]["note"] =
        serde_json::json!({"kind":"optional","item":{"kind":"text"}});
    app["actions"][0]["parameters"]["tags"] =
        serde_json::json!({"kind":"list","item":{"kind":"text"}});
    app["actions"][0]["parameters"]["person"] =
        serde_json::json!({"kind":"reference","entity":"person"});
    for name in ["count", "due", "confirmed", "note", "tags", "person"] {
        app["views"][1]["kind"]["fields"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"parameter":name,"label":name}));
    }
    active.runtime.program = AppDefinition::parse(&serde_json::to_vec(&app).unwrap()).unwrap();
    active.program = active.runtime.program.identity().unwrap();
    active.runtime.observation.view = "new_person".into();
    assert!(!frame(&mut h, &mut state, &view).trace.controls["daily.submit"].enabled);
    type_in(
        &mut h,
        &mut state,
        &view,
        "daily.field.count",
        "9223372036854775808",
    );
    type_in(&mut h, &mut state, &view, "daily.field.due", "2026-02-30");
    assert!(!frame(&mut h, &mut state, &view).trace.controls["daily.submit"].enabled);
    type_in(&mut h, &mut state, &view, "daily.field.count", "-12");
    type_in(&mut h, &mut state, &view, "daily.field.due", "2026-10-05");
    click(&mut h, &mut state, &view, "daily.field.confirmed");
    click(&mut h, &mut state, &view, "daily.field.note.present");
    type_in(&mut h, &mut state, &view, "daily.field.note", "Unicode 🦉");
    click(&mut h, &mut state, &view, "daily.field.tags.add");
    type_in(&mut h, &mut state, &view, "daily.field.tags.0", "fragile");
    click(&mut h, &mut state, &view, "daily.field.person.person.one");
    let WorkspaceAction::Daily {
        input: SemanticInput::Submit { arguments, .. },
    } = click(&mut h, &mut state, &view, "daily.submit")
        .request
        .unwrap()
        .action
    else {
        panic!("submit expected")
    };
    assert_eq!(arguments["count"], DataValue::Integer { value: -12 });
    assert_eq!(arguments["confirmed"], DataValue::Boolean { value: true });
    assert_eq!(arguments["note"], text("Unicode 🦉"));
    assert_eq!(
        arguments["tags"],
        DataValue::List {
            item_type: Type::Text,
            items: vec![text("fragile")]
        }
    );
    assert_eq!(
        arguments["person"],
        DataValue::Reference {
            entity: "person".into(),
            record: "one".into()
        }
    );
    assert_eq!(arguments["due"], DataValue::Date { days: 20731 });
}

#[test]
fn row_actions_detail_and_keyboard_use_observed_guards_and_exact_row_context() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    active.runtime.program =
        AppDefinition::parse(&serde_json::to_vec(&fixture::equipment()).unwrap()).unwrap();
    active.program = active.runtime.program.identity().unwrap();
    active.runtime.observation.view = "assets".into();
    active.runtime.observation.rows[0].record = RecordRef {
        entity: "asset".into(),
        record: "one".into(),
    };
    assert!(click(
        &mut h,
        &mut state,
        &view,
        "daily.row.asset.one.borrow_asset"
    )
    .request
    .is_none());
    view.active.as_mut().unwrap().runtime.observation.rows[0]
        .enabled_actions
        .insert("borrow_asset".into());
    let request = click(
        &mut h,
        &mut state,
        &view,
        "daily.row.asset.one.borrow_asset",
    )
    .request
    .unwrap();
    assert_eq!(
        request.action,
        WorkspaceAction::Daily {
            input: SemanticInput::Activate {
                view: "assets".into(),
                binding: "borrow_asset".into(),
                row: Some(RecordRef {
                    entity: "asset".into(),
                    record: "one".into()
                })
            }
        }
    );
    ack(&mut view, &request, 1);
    let active = view.active.as_mut().unwrap();
    let columns = match active.runtime.program.views[0].kind.clone() {
        ViewKind::List { columns, .. } => columns,
        _ => unreachable!(),
    };
    active.runtime.program.views[0].kind = ViewKind::Detail {
        entity: "asset".into(),
        record: Expr::Literal {
            value_type: Type::reference("asset"),
            value: DataValue::Reference {
                entity: "asset".into(),
                record: "one".into(),
            },
        },
        columns,
    };
    active.program = active.runtime.program.identity().unwrap();
    assert!(frame(&mut h, &mut state, &view)
        .trace
        .text
        .iter()
        .any(|t| t.contains("小明")));
    let (mut h, mut state) = setup();
    let view = daily();
    click(&mut h, &mut state, &view, "daily.control.search_input");
    h.key(Key::E, true, Modifiers::CTRL);
    assert!(
        frame(&mut h, &mut state, &view).request.is_none(),
        "text focus must not export"
    );
}

#[test]
fn comparison_widgets_route_play_to_both_and_never_save_preview_outputs() {
    let (mut h, mut state) = setup();
    let view = comparison();
    let shown = frame(&mut h, &mut state, &view);
    assert!(!shown.trace.controls.contains_key("before.output.0.save"));
    assert!(!shown.trace.controls.contains_key("after.output.0.save"));
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "after.control.search_input",
        "South",
    )
    .request
    .unwrap();
    assert_eq!(
        request.action,
        WorkspaceAction::Replay {
            witness: "witness".into(),
            input: SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: text("South")
            }
        }
    );
}

fn add_adoption(view: &mut WorkspaceView) {
    let active = view.active.as_ref().unwrap();
    let target = view.comparison.as_ref().unwrap().after.artifact.clone();
    view.adoption = Some(AdoptionView {
        plan_id: "plan".into(),
        target: target.clone(),
        compatibility: CompatibilityReport {
            state: CompatibilityState::Compatible,
            current_data: active.data.clone(),
            target_program: target.program_digest,
            retained_records: vec![],
            retained_events: vec![],
            retained_fields: vec![],
            issues: vec![],
        },
        checks: vec![],
        retired_decisions: vec![],
        issues: vec![],
        can_confirm: true,
    });
}
#[test]
fn edited_scope_and_changed_identity_cannot_adopt_an_old_controller_plan() {
    let (mut h, mut state) = setup();
    let mut view = scope_view();
    add_adoption(&mut view);
    assert!(frame(&mut h, &mut state, &view).trace.controls["adopt"].enabled);
    click(&mut h, &mut state, &view, "scope.population.all");
    assert!(!frame(&mut h, &mut state, &view).trace.controls["adopt"].enabled);
    let request = click(&mut h, &mut state, &view, "scope.preview")
        .request
        .unwrap();
    assert!(matches!(request.action, WorkspaceAction::SetScope { .. }));
    let (mut h, mut state) = setup();
    let mut view = comparison();
    add_adoption(&mut view);
    frame(&mut h, &mut state, &view);
    view.active.as_mut().unwrap().data = digest("unrelated-new-data");
    assert!(
        !frame(&mut h, &mut state, &view).trace.controls["adopt"].enabled,
        "an unchanged plan must not outlive the viewed identity"
    );
}

#[test]
fn reopened_running_operation_is_cancellable_but_unmatched_success_is_ignored() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    click(&mut h, &mut state, &view, "change");
    type_in(
        &mut h,
        &mut state,
        &view,
        "change.text",
        "Change the current result",
    );
    let request = click(&mut h, &mut state, &view, "change.submit")
        .request
        .unwrap();
    view.next_operation_id = "op-2".into();
    view.operation = Some(OperationStatus {
        delivery_id: 1,
        context: request.context.clone(),
        outcome: OperationOutcome::Running,
    });
    let mut reopened = ProductWorkspace::default();
    assert!(frame(&mut h, &mut reopened, &view)
        .trace
        .controls
        .contains_key("cancel"));
    assert_eq!(
        click(&mut h, &mut reopened, &view, "cancel")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Cancel {
            operation: request.context.request_id
        }
    );
    let mut unrelated = ProductWorkspace::default();
    view.operation.as_mut().unwrap().outcome = OperationOutcome::Committed { generation: 99 };
    assert!(!frame(&mut h, &mut unrelated, &view)
        .trace
        .text
        .iter()
        .any(|t| t.contains("Saved (revision 99)")));
}

#[test]
fn comparison_preserves_queued_text_when_acknowledged_replay_refreshes_evidence() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let first = type_in(
        &mut h,
        &mut state,
        &view,
        "after.control.search_input",
        "北",
    )
    .request
    .unwrap();
    h.text("方");
    assert!(frame(&mut h, &mut state, &view).request.is_none());
    let comparison = view.comparison.as_mut().unwrap();
    comparison
        .before
        .runtime
        .observation
        .controls
        .insert("search_input".into(), text("北"));
    comparison
        .after
        .runtime
        .observation
        .controls
        .insert("search_input".into(), text("北"));
    comparison.before.evidence = Some(digest("new-before-evidence"));
    comparison.after.evidence = Some(digest("new-after-evidence"));
    ack(&mut view, &first, 1);
    let next = frame(&mut h, &mut state, &view).request.unwrap();
    assert_eq!(
        next.action,
        WorkspaceAction::Replay {
            witness: "witness".into(),
            input: SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: text("北方")
            }
        }
    );
}

#[test]
fn queued_comparison_input_disables_choice_on_the_dispatch_frame() {
    let (mut h, mut state) = setup();
    let view = comparison();
    let result = type_in(
        &mut h,
        &mut state,
        &view,
        "after.control.search_input",
        "new input",
    );
    assert!(matches!(
        result.request.unwrap().action,
        WorkspaceAction::Replay { .. }
    ));
    for key in [
        "choice.after",
        "choice.before",
        "choice.either",
        "choice.both",
        "choice.neither",
        "choice.defer",
        "choice.keep",
    ] {
        assert!(
            !result.trace.controls[key].enabled,
            "{key} must not replace a just-emitted replay"
        );
    }
}

#[test]
fn all_named_shortcuts_emit_once_and_ignore_held_key_repeats() {
    for (name, key) in [
        ("enter", Key::Enter),
        ("escape", Key::Escape),
        ("space", Key::Space),
        ("arrow_up", Key::ArrowUp),
        ("arrow_down", Key::ArrowDown),
        ("arrow_left", Key::ArrowLeft),
        ("arrow_right", Key::ArrowRight),
        ("delete", Key::Delete),
        ("backspace", Key::Backspace),
        ("tab", Key::Tab),
    ] {
        let (mut h, mut state) = setup();
        let mut view = daily();
        let active = view.active.as_mut().unwrap();
        active.runtime.program.views[0].keys[0].key = name.into();
        active.program = active.runtime.program.identity().unwrap();
        frame(&mut h, &mut state, &view);
        h.key(key, true, Modifiers::CTRL);
        let request = frame(&mut h, &mut state, &view)
            .request
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(matches!(
            request.action,
            WorkspaceAction::Daily {
                input: SemanticInput::Activate { .. }
            }
        ));
        ack(&mut view, &request, 1);
        h.key(key, true, Modifiers::CTRL);
        assert!(
            frame(&mut h, &mut state, &view).request.is_none(),
            "held {name} repeated"
        );
        h.key(key, false, Modifiers::NONE);
        frame(&mut h, &mut state, &view);
        click(&mut h, &mut state, &view, "daily.control.search_input");
        h.key(key, true, Modifiers::CTRL);
        assert!(
            !matches!(
                frame(&mut h, &mut state, &view).request.map(|r| r.action),
                Some(WorkspaceAction::Daily {
                    input: SemanticInput::Activate { .. }
                })
            ),
            "{name} escaped a text editor"
        );
    }
}

#[test]
fn reverted_scope_edit_survives_an_older_rehearsal_and_blocks_its_adoption() {
    let (mut h, mut state) = setup();
    let mut view = scope_view();
    click(&mut h, &mut state, &view, "scope.population.all");
    let request = click(&mut h, &mut state, &view, "scope.preview")
        .request
        .unwrap();
    view.next_operation_id = "op-2".into();
    click(&mut h, &mut state, &view, "scope.population.new");
    view.scope.as_mut().unwrap().proposed_scope.population = Population::All;
    ack(&mut view, &request, 1);
    add_adoption(&mut view);
    assert!(!frame(&mut h, &mut state, &view).trace.controls["adopt"].enabled);
    let WorkspaceAction::SetScope { scope, .. } = click(&mut h, &mut state, &view, "scope.preview")
        .request
        .unwrap()
        .action
    else {
        panic!("scope expected")
    };
    assert_eq!(scope.population, Population::NewWork);
}

#[test]
fn incomplete_numeric_and_date_control_drafts_survive_prior_valid_acknowledgement() {
    for (typ, initial, first, intermediate, last, expected) in [
        (
            Type::Integer,
            DataValue::Integer { value: 0 },
            "1",
            "-",
            "2",
            DataValue::Integer { value: -2 },
        ),
        (
            Type::Date,
            DataValue::Date { days: 0 },
            "2026-10-05",
            "2026-",
            "11-06",
            DataValue::Date { days: 20763 },
        ),
    ] {
        let (mut h, mut state) = setup();
        let mut view = daily();
        let active = view.active.as_mut().unwrap();
        active.runtime.program.state[0].value_type = typ;
        active.runtime.program.state[0].initial = initial.clone();
        active
            .runtime
            .observation
            .controls
            .insert("search_input".into(), initial);
        active.program = active.runtime.program.identity().unwrap();
        let request = type_in(
            &mut h,
            &mut state,
            &view,
            "daily.control.search_input",
            first,
        )
        .request
        .unwrap();
        assert!(type_in(
            &mut h,
            &mut state,
            &view,
            "daily.control.search_input",
            intermediate
        )
        .request
        .is_none());
        let WorkspaceAction::Daily {
            input: SemanticInput::Control { value, .. },
        } = &request.action
        else {
            panic!("control expected")
        };
        view.active
            .as_mut()
            .unwrap()
            .runtime
            .observation
            .controls
            .insert("search_input".into(), value.clone());
        view.active.as_mut().unwrap().session = digest("after-valid-input");
        ack(&mut view, &request, 1);
        assert!(frame(&mut h, &mut state, &view).request.is_none());
        h.text(last);
        let WorkspaceAction::Daily {
            input: SemanticInput::Control { value, .. },
        } = frame(&mut h, &mut state, &view).request.unwrap().action
        else {
            panic!("control expected")
        };
        assert_eq!(value, expected);
    }
}

#[test]
fn delivery_counters_are_scoped_to_the_exact_request_not_previous_operations() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let first = click(&mut h, &mut state, &view, "daily.action.export_button")
        .request
        .unwrap();
    ack(&mut view, &first, 1);
    let second = click(&mut h, &mut state, &view, "daily.action.export_button")
        .request
        .unwrap();
    ack(&mut view, &second, 1);
    view.next_operation_id = "op-3".into();
    assert!(frame(&mut h, &mut state, &view).trace.controls["daily.action.export_button"].enabled);
}

#[test]
fn recovered_unknown_operation_disables_comparison_edits_until_its_exact_reply() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "after.control.search_input",
        "first",
    )
    .request
    .unwrap();
    view.operation = Some(OperationStatus {
        delivery_id: 1,
        context: request.context.clone(),
        outcome: OperationOutcome::Running,
    });
    view.next_operation_id = "op-2".into();
    let mut reopened = ProductWorkspace::default();
    assert!(
        !frame(&mut h, &mut reopened, &view).trace.controls["after.control.search_input"].enabled
    );
    view.comparison.as_mut().unwrap().after.evidence = Some(digest("recovered-run"));
    ack(&mut view, &request, 2);
    assert!(
        frame(&mut h, &mut reopened, &view).trace.controls["after.control.search_input"].enabled
    );
    assert!(matches!(
        type_in(
            &mut h,
            &mut reopened,
            &view,
            "after.control.search_input",
            "next"
        )
        .request
        .unwrap()
        .action,
        WorkspaceAction::Replay { .. }
    ));
}

#[test]
fn pending_controls_preserve_event_order_and_each_valid_on_change_input() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    for name in ["aaa", "zzz"] {
        let mut state = active.runtime.program.state[0].clone();
        state.id = name.into();
        active.runtime.program.state.push(state);
        let ViewKind::List { controls, .. } = &mut active.runtime.program.views[0].kind else {
            unreachable!()
        };
        controls.push(StateControl {
            id: name.into(),
            label: name.into(),
            state: name.into(),
            on_change: None,
        });
        active
            .runtime
            .observation
            .controls
            .insert(name.into(), text(""));
    }
    active.program = active.runtime.program.identity().unwrap();
    let first_frame = type_in(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input",
        "start",
    );
    assert!(!first_frame.trace.controls["daily.action.export_button"].enabled);
    assert!(!first_frame.trace.controls["daily.select.person.one"].enabled);
    let first = first_frame.request.unwrap();
    for (key, value) in [("zzz", "z1"), ("aaa", "a1"), ("zzz", "z2")] {
        assert!(type_in(
            &mut h,
            &mut state,
            &view,
            &format!("daily.control.{key}"),
            value
        )
        .request
        .is_none());
    }
    ack(&mut view, &first, 1);
    for (index, key, value) in [(2, "zzz", "z1"), (3, "aaa", "a1"), (4, "zzz", "z2")] {
        let request = frame(&mut h, &mut state, &view).request.unwrap();
        assert_eq!(
            request.action,
            WorkspaceAction::Daily {
                input: SemanticInput::Control {
                    view: "people".into(),
                    control: key.into(),
                    value: text(value)
                }
            }
        );
        let active = view.active.as_mut().unwrap();
        active
            .runtime
            .observation
            .controls
            .insert(key.into(), text(value));
        active.session = digest(&format!("session-{index}"));
        ack(&mut view, &request, index);
    }
}

#[test]
fn comparison_keeps_one_editing_side_until_its_ordered_inputs_finish() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let first = type_in(
        &mut h,
        &mut state,
        &view,
        "after.control.search_input",
        "one",
    )
    .request
    .unwrap();
    h.text(" two");
    let pending = frame(&mut h, &mut state, &view);
    assert!(!pending.trace.controls["before.control.search_input"].enabled);
    assert!(pending.trace.controls["after.control.search_input"].enabled);
    ack(&mut view, &first, 1);
    let second_frame = frame(&mut h, &mut state, &view);
    let second = second_frame.request.unwrap();
    assert!(!second_frame.trace.controls["before.control.search_input"].enabled);
    assert_eq!(
        second.action,
        WorkspaceAction::Replay {
            witness: "witness".into(),
            input: SemanticInput::Control {
                view: "people".into(),
                control: "search_input".into(),
                value: text("one two")
            }
        }
    );
    ack(&mut view, &second, 2);
    assert!(frame(&mut h, &mut state, &view).trace.controls["before.control.search_input"].enabled);
}

#[test]
fn invalid_comparison_draft_keeps_its_editor_available_for_correction() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let comparison = view.comparison.as_mut().unwrap();
    for side in [&mut comparison.before, &mut comparison.after] {
        side.runtime.program.state[0].value_type = Type::Integer;
        side.runtime.program.state[0].initial = DataValue::Integer { value: 0 };
        side.runtime
            .observation
            .controls
            .insert("search_input".into(), DataValue::Integer { value: 0 });
        side.artifact = artifact(&side.runtime);
    }
    assert!(type_in(
        &mut h,
        &mut state,
        &view,
        "before.control.search_input",
        "-"
    )
    .request
    .is_none());
    let shown = frame(&mut h, &mut state, &view);
    assert!(shown.trace.controls["before.control.search_input"].enabled);
    assert!(!shown.trace.controls["after.control.search_input"].enabled);
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "before.control.search_input",
        "2",
    )
    .request
    .unwrap();
    assert!(matches!(
        request.action,
        WorkspaceAction::Replay {
            input: SemanticInput::Control {
                value: DataValue::Integer { value: 2 },
                ..
            },
            ..
        }
    ));
}

#[test]
fn failed_or_cancelled_receipts_cannot_rebase_queued_edits_onto_changed_data() {
    for outcome in [
        OperationOutcome::Rejected("Data changed".into()),
        OperationOutcome::Retryable("Work changed".into()),
        OperationOutcome::Cancelled,
    ] {
        let (mut h, mut state) = setup();
        let mut view = daily();
        let request = type_in(&mut h, &mut state, &view, "daily.control.search_input", "x")
            .request
            .unwrap();
        h.text("y");
        assert!(frame(&mut h, &mut state, &view).request.is_none());
        ack(&mut view, &request, 1);
        view.operation.as_mut().unwrap().outcome = outcome;
        let active = view.active.as_mut().unwrap();
        active.data = digest("unrelated-data");
        active.session = digest("unrelated-session");
        active.generation += 1;
        assert!(frame(&mut h, &mut state, &view).request.is_none());
        assert!(frame(&mut h, &mut state, &view).request.is_none());
    }
}

#[test]
fn comparison_row_shortcut_uses_the_side_the_user_last_focused() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let comparison = view.comparison.as_mut().unwrap();
    for side in [&mut comparison.before, &mut comparison.after] {
        let mut binding = side.runtime.program.views[0].actions[0].clone();
        binding.id = "row_export".into();
        binding.placement = ActionPlacement::Row;
        side.runtime.program.views[0].actions.push(binding);
        side.runtime.program.views[0].keys.push(KeyBinding {
            key: "r".into(),
            modifiers: [KeyModifier::Control].into(),
            binding: "row_export".into(),
        });
        side.runtime.observation.rows[0]
            .enabled_actions
            .insert("row_export".into());
        let mut row = side.runtime.observation.rows[0].clone();
        row.record = record("two");
        side.runtime.observation.rows.push(row);
        side.artifact = artifact(&side.runtime);
    }
    click(&mut h, &mut state, &view, "before.focus.person.one");
    click(&mut h, &mut state, &view, "after.focus.person.two");
    h.key(Key::R, true, Modifiers::CTRL);
    let WorkspaceAction::Replay {
        input: SemanticInput::Activate { row, .. },
        ..
    } = frame(&mut h, &mut state, &view).request.unwrap().action
    else {
        panic!("row replay expected")
    };
    assert_eq!(row, Some(record("two")));
}

#[test]
fn change_entry_waits_for_every_already_entered_control_input() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let first = type_in(&mut h, &mut state, &view, "daily.control.search_input", "a")
        .request
        .unwrap();
    h.text("b");
    frame(&mut h, &mut state, &view);
    h.text("c");
    frame(&mut h, &mut state, &view);
    ack(&mut view, &first, 1);
    let second = frame(&mut h, &mut state, &view);
    assert!(!second.trace.controls["change"].enabled);
    let request = second.request.unwrap();
    ack(&mut view, &request, 2);
    let third = frame(&mut h, &mut state, &view);
    assert!(!third.trace.controls["change"].enabled);
    let request = third.request.unwrap();
    ack(&mut view, &request, 3);
    assert!(frame(&mut h, &mut state, &view).trace.controls["change"].enabled);
}

#[test]
fn scripted_replay_waits_behind_locally_entered_inputs() {
    let (mut h, mut state) = setup();
    let mut view = comparison();
    let first = type_in(&mut h, &mut state, &view, "after.control.search_input", "a")
        .request
        .unwrap();
    h.text("b");
    frame(&mut h, &mut state, &view);
    ack(&mut view, &first, 1);
    let next = frame(&mut h, &mut state, &view);
    assert!(!next.trace.controls["replay"].enabled);
    let request = next.request.unwrap();
    assert!(
        matches!(request.action,WorkspaceAction::Replay{input:SemanticInput::Control{value:DataValue::Text{ref value},..},..} if value=="ab")
    );
    ack(&mut view, &request, 2);
    assert!(frame(&mut h, &mut state, &view).trace.controls["replay"].enabled);
    let request = click(&mut h, &mut state, &view, "replay").request.unwrap();
    assert_eq!(
        request.action,
        WorkspaceAction::Replay {
            witness: "witness".into(),
            input: view.comparison.as_ref().unwrap().inputs[0].clone()
        }
    );
}

#[test]
fn exact_mac_control_command_chords_do_not_dispatch_a_shorter_binding() {
    for (modifiers, expected) in [
        (
            Modifiers {
                ctrl: true,
                command: true,
                mac_cmd: true,
                ..Modifiers::NONE
            },
            Some("ctrl_command"),
        ),
        (Modifiers::CTRL, Some("ctrl_only")),
        (
            Modifiers {
                command: true,
                mac_cmd: true,
                ..Modifiers::NONE
            },
            Some("command_only"),
        ),
        (
            Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
            None,
        ),
    ] {
        let (mut h, mut state) = setup();
        let mut view = daily();
        let active = view.active.as_mut().unwrap();
        let prototype = active.runtime.program.views[0].actions[0].clone();
        active.runtime.program.views[0].actions.clear();
        active.runtime.program.views[0].keys.clear();
        active.runtime.observation.enabled_actions.clear();
        for (id, modifiers) in [
            ("ctrl_only", vec![KeyModifier::Control]),
            (
                "ctrl_command",
                vec![KeyModifier::Control, KeyModifier::Command],
            ),
            ("command_only", vec![KeyModifier::Command]),
        ] {
            let mut binding = prototype.clone();
            binding.id = id.into();
            active.runtime.program.views[0].actions.push(binding);
            active.runtime.program.views[0].keys.push(KeyBinding {
                key: "r".into(),
                modifiers: modifiers.into_iter().collect(),
                binding: id.into(),
            });
            active.runtime.observation.enabled_actions.insert(id.into());
        }
        active.program = active.runtime.program.identity().unwrap();
        frame(&mut h, &mut state, &view);
        h.key(Key::R, true, modifiers);
        let actual = frame(&mut h, &mut state, &view)
            .request
            .map(|r| match r.action {
                WorkspaceAction::Daily {
                    input: SemanticInput::Activate { binding, .. },
                } => binding,
                _ => panic!("shortcut expected"),
            });
        assert_eq!(actual.as_deref(), expected);
    }
}

#[test]
fn blank_optional_rationale_does_not_keep_a_rehearsed_plan_stale() {
    let (mut h, mut state) = setup();
    let mut view = scope_view();
    type_in(&mut h, &mut state, &view, "scope.rationale", "   ");
    let request = click(&mut h, &mut state, &view, "scope.preview")
        .request
        .unwrap();
    assert!(matches!(
        request.action,
        WorkspaceAction::SetScope {
            rationale: None,
            ..
        }
    ));
    ack(&mut view, &request, 1);
    add_adoption(&mut view);
    assert_eq!(
        click(&mut h, &mut state, &view, "adopt")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Adopt {
            plan: "plan".into()
        }
    );
}

// This bridge executes UI requests through the actual interpreter. It is still a
// hand-authored-program fixture, not provider generation or desktop acceptance.
fn execute_widget_request(
    runtime: &product_runtime::LocalRuntime,
    run: &mut product_runtime::ProductRun,
    view: &mut WorkspaceView,
    request: WorkspaceRequest,
    delivery: &mut u64,
) {
    let WorkspaceAction::Daily { input } = &request.action else {
        panic!("daily runtime input expected")
    };
    runtime
        .apply(run, input, &request.context.request_id)
        .unwrap();
    let active = view.active.as_mut().unwrap();
    active.runtime = runtime.view_model(run).unwrap();
    active.generation = runtime.data(run).generation;
    active.data = runtime.data(run).identity().unwrap();
    active.session = runtime.session(run).identity().unwrap();
    *delivery += 1;
    ack(view, &request, *delivery);
}

#[test]
fn generated_widgets_drive_real_runtime_forms_filter_selection_and_output() {
    let program = executed_fixture::capture(executed_fixture::filtered());
    let runtime = product_runtime::LocalRuntime::default();
    let mut run = runtime
        .start(
            &program,
            &executed_fixture::seed(&program),
            &SessionState::initial(&program.program).unwrap(),
            20000,
            42,
            RuntimeLimits::default(),
        )
        .unwrap();
    let (mut h, mut state) = setup();
    let mut view = entry();
    let mut delivery = 0;
    view.active = Some(ActiveToolView {
        project_id: runtime.data(&run).project_id.clone(),
        name: program.program.label.clone(),
        generation: runtime.data(&run).generation,
        program: program.artifact.program_digest.clone(),
        data: runtime.data(&run).identity().unwrap(),
        session: runtime.session(&run).identity().unwrap(),
        runtime: runtime.view_model(&run).unwrap(),
        capabilities: runtime.capabilities(),
        decisions: vec![],
    });
    for name in ["Ada", "Zoë"] {
        let request = click(&mut h, &mut state, &view, "daily.navigate.new_person")
            .request
            .unwrap();
        execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
        type_in(&mut h, &mut state, &view, "daily.field.name", name);
        type_in(&mut h, &mut state, &view, "daily.field.area", "North");
        let request = click(&mut h, &mut state, &view, "daily.submit")
            .request
            .unwrap();
        execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
        let request = click(&mut h, &mut state, &view, "daily.navigate.people")
            .request
            .unwrap();
        execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
    }
    assert_eq!(runtime.data(&run).records.len(), 2);
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input",
        "Ada",
    )
    .request
    .unwrap();
    execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
    let row = view.active.as_ref().unwrap().runtime.observation.rows[0]
        .record
        .clone();
    assert_eq!(
        view.active.as_ref().unwrap().runtime.observation.rows.len(),
        1
    );
    let request = click(
        &mut h,
        &mut state,
        &view,
        &format!("daily.select.{}.{}", row.entity, row.record),
    )
    .request
    .unwrap();
    execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input",
        "Zoë",
    )
    .request
    .unwrap();
    execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
    assert_eq!(
        view.active.as_ref().unwrap().runtime.observation.selected,
        vec![row]
    );
    let request = click(&mut h, &mut state, &view, "daily.action.export_button")
        .request
        .unwrap();
    execute_widget_request(&runtime, &mut run, &mut view, request, &mut delivery);
    let observed = runtime.observe(&run, "checked-output").unwrap();
    assert_eq!(
        observed.values["selected_count"],
        DataValue::Integer { value: 1 }
    );
    assert_eq!(observed.outputs[0].bytes, b"name\r\nAda\r\n");
    assert_eq!(observed.outputs[0].rows.len(), 1);
    assert!(frame(&mut h, &mut state, &view)
        .trace
        .text
        .iter()
        .any(|t| t == "Name: Ada"));
    assert_eq!(
        click(&mut h, &mut state, &view, "daily.output.0.save")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Export {
            artifact: observed.outputs[0].digest.clone()
        }
    );
}

fn tab_to_control(
    h: &mut EguiHarness,
    state: &mut ProductWorkspace,
    view: &WorkspaceView,
    key: &str,
) {
    for _ in 0..40 {
        let (shown, focused) = h.frame(|ctx| {
            let shown = egui::CentralPanel::default()
                .show(ctx, |ui| state.show(ui, view))
                .inner;
            (shown, ctx.memory(|memory| memory.focused()))
        });
        if focused == Some(shown.trace.controls[key].id) {
            return;
        }
        h.key(Key::Tab, true, Modifiers::NONE);
        frame(h, state, view);
        h.key(Key::Tab, false, Modifiers::NONE);
        frame(h, state, view);
    }
    panic!("could not reach {key} with Tab");
}

#[test]
fn configured_shortcuts_and_repeats_cannot_click_a_different_focused_button() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    let mut alternate = active.runtime.program.views[0].actions[0].clone();
    alternate.id = "alternate".into();
    alternate.label = "Other export".into();
    active.runtime.program.views[0].actions.push(alternate);
    active.runtime.program.views[0].keys = vec![KeyBinding {
        key: "enter".into(),
        modifiers: [KeyModifier::Control].into(),
        binding: "alternate".into(),
    }];
    active
        .runtime
        .observation
        .enabled_actions
        .insert("alternate".into());
    active.program = active.runtime.program.identity().unwrap();
    tab_to_control(&mut h, &mut state, &view, "daily.action.export_button");
    h.key(Key::Enter, true, Modifiers::CTRL);
    let request = frame(&mut h, &mut state, &view).request.unwrap();
    assert!(
        matches!(request.action,WorkspaceAction::Daily{input:SemanticInput::Activate{ref binding,..}} if binding=="alternate")
    );
    ack(&mut view, &request, 1);
    h.key(Key::Enter, true, Modifiers::CTRL);
    assert!(frame(&mut h, &mut state, &view).request.is_none());
    h.key(Key::Enter, false, Modifiers::NONE);
    frame(&mut h, &mut state, &view);
    tab_to_control(&mut h, &mut state, &view, "daily.action.export_button");
    h.key(Key::Enter, true, Modifiers::NONE);
    let request = frame(&mut h, &mut state, &view).request.unwrap();
    assert!(
        matches!(request.action,WorkspaceAction::Daily{input:SemanticInput::Activate{ref binding,..}} if binding=="export_button")
    );
    ack(&mut view, &request, 2);
    h.key(Key::Enter, true, Modifiers::NONE);
    assert!(frame(&mut h, &mut state, &view).request.is_none());
}

#[test]
fn rejected_runtime_control_stays_unapplied_until_explicit_retry_or_discard() {
    let mut app = executed_fixture::filtered();
    app["actions"].as_array_mut().unwrap().push(serde_json::json!({"id":"reject_search","label":"Check search","parameters":{"value":{"kind":"text"}},"guards":[{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}],"steps":[{"kind":"assert","condition":fixture::yes(),"message":"Guard passed"}],"ensures":[]}));
    app["views"][0]["kind"]["controls"][0]["on_change"] = serde_json::json!("reject_search");
    let program = executed_fixture::capture(app);
    let runtime = product_runtime::LocalRuntime::default();
    let mut run = runtime
        .start(
            &program,
            &executed_fixture::seed(&program),
            &SessionState::initial(&program.program).unwrap(),
            20000,
            42,
            RuntimeLimits::default(),
        )
        .unwrap();
    let (mut h, mut state) = setup();
    let mut view = entry();
    view.active = Some(ActiveToolView {
        project_id: runtime.data(&run).project_id.clone(),
        name: program.program.label.clone(),
        generation: runtime.data(&run).generation,
        program: program.artifact.program_digest.clone(),
        data: runtime.data(&run).identity().unwrap(),
        session: runtime.session(&run).identity().unwrap(),
        runtime: runtime.view_model(&run).unwrap(),
        capabilities: runtime.capabilities(),
        decisions: vec![],
    });
    let request = type_in(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input",
        "blocked",
    )
    .request
    .unwrap();
    let WorkspaceAction::Daily { input } = request.action.clone() else {
        panic!("control expected")
    };
    assert!(runtime
        .apply(&mut run, &input, &request.context.request_id)
        .is_err());
    view.active.as_mut().unwrap().runtime = runtime.view_model(&run).unwrap();
    assert_eq!(
        view.active.as_ref().unwrap().runtime.observation.controls["search_input"],
        text("")
    );
    ack(&mut view, &request, 1);
    view.operation.as_mut().unwrap().outcome =
        OperationOutcome::Rejected("Search guard rejected this value".into());
    let shown = frame(&mut h, &mut state, &view);
    assert!(!shown.trace.controls["daily.action.export_button"].enabled);
    let retry = click(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input.retry",
    )
    .request
    .unwrap();
    assert_eq!(retry.action, WorkspaceAction::Daily { input });
    ack(&mut view, &retry, 2);
    view.operation.as_mut().unwrap().outcome =
        OperationOutcome::Retryable("Still unavailable".into());
    assert!(click(
        &mut h,
        &mut state,
        &view,
        "daily.control.search_input.discard"
    )
    .request
    .is_none());
    assert!(frame(&mut h, &mut state, &view).trace.controls["daily.action.export_button"].enabled);
}

#[test]
fn rejected_form_submission_preserves_its_complete_typed_draft() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    view.active.as_mut().unwrap().runtime.observation.view = "new_person".into();
    type_in(
        &mut h,
        &mut state,
        &view,
        "daily.field.name",
        "Preserve this",
    );
    type_in(&mut h, &mut state, &view, "daily.field.area", "North");
    let first = click(&mut h, &mut state, &view, "daily.submit")
        .request
        .unwrap();
    ack(&mut view, &first, 1);
    view.operation.as_mut().unwrap().outcome = OperationOutcome::Rejected("A guard failed".into());
    let second = click(&mut h, &mut state, &view, "daily.submit")
        .request
        .unwrap();
    assert_eq!(second.action, first.action);
}

#[test]
fn awaiting_consent_remains_cancellable_and_accepts_no_late_success_after_cancel() {
    let (mut h, mut state) = setup();
    let mut view = entry();
    type_in(&mut h, &mut state, &view, "need", "Organize fictional work");
    let request = click(&mut h, &mut state, &view, "create").request.unwrap();
    ack(&mut view, &request, 1);
    view.operation.as_mut().unwrap().outcome =
        OperationOutcome::AwaitingConsent("Review the provider disclosure".into());
    let shown = frame(&mut h, &mut state, &view);
    assert!(!shown.trace.controls["create"].enabled);
    assert!(shown.trace.controls["cancel"].enabled);
    assert_eq!(
        click(&mut h, &mut state, &view, "cancel")
            .request
            .unwrap()
            .action,
        WorkspaceAction::Cancel {
            operation: request.context.request_id.clone()
        }
    );
    ack(&mut view, &request, 2);
    assert!(!frame(&mut h, &mut state, &view)
        .trace
        .text
        .iter()
        .any(|t| t == "Preview ready"));
}

#[test]
fn workspace_buttons_cannot_steal_registered_shortcuts_or_held_activation() {
    let (mut h, mut state) = setup();
    let mut view = daily();
    let active = view.active.as_mut().unwrap();
    active.runtime.program.views[0].keys = vec![KeyBinding {
        key: "enter".into(),
        modifiers: [KeyModifier::Control].into(),
        binding: "export_button".into(),
    }];
    active.program = active.runtime.program.identity().unwrap();
    tab_to_control(&mut h, &mut state, &view, "close");
    h.key(Key::Enter, true, Modifiers::CTRL);
    let first = frame(&mut h, &mut state, &view).request.unwrap();
    assert!(matches!(
        first.action,
        WorkspaceAction::Daily {
            input: SemanticInput::Activate { .. }
        }
    ));
    ack(&mut view, &first, 1);
    h.key(Key::Enter, true, Modifiers::CTRL);
    assert!(frame(&mut h, &mut state, &view).request.is_none());
    h.key(Key::Enter, false, Modifiers::NONE);
    frame(&mut h, &mut state, &view);
    let running = click(&mut h, &mut state, &view, "daily.action.export_button")
        .request
        .unwrap();
    view.next_operation_id = "op-3".into();
    view.operation = Some(OperationStatus {
        delivery_id: 1,
        context: running.context.clone(),
        outcome: OperationOutcome::Running,
    });
    tab_to_control(&mut h, &mut state, &view, "cancel");
    h.key(Key::Enter, true, Modifiers::CTRL);
    assert!(frame(&mut h, &mut state, &view).request.is_none());
    h.key(Key::Enter, false, Modifiers::NONE);
    frame(&mut h, &mut state, &view);
    h.key(Key::Enter, true, Modifiers::NONE);
    let cancel = frame(&mut h, &mut state, &view).request.unwrap();
    assert_eq!(
        cancel.action,
        WorkspaceAction::Cancel {
            operation: running.context.request_id
        }
    );
    ack(&mut view, &cancel, 2);
    view.next_operation_id = "op-4".into();
    view.operation.as_mut().unwrap().outcome = OperationOutcome::Cancelled;
    h.key(Key::Enter, true, Modifiers::NONE);
    assert!(frame(&mut h, &mut state, &view).request.is_none());
}

#[test]
fn batched_shortcuts_keep_each_fresh_press_in_order_and_stop_on_failure() {
    for fail_first in [false, true] {
        let (mut h, mut state) = setup();
        let mut view = daily();
        let active = view.active.as_mut().unwrap();
        let mut alternate = active.runtime.program.views[0].actions[0].clone();
        alternate.id = "alternate".into();
        alternate.label = "Other export".into();
        active.runtime.program.views[0].actions.push(alternate);
        active.runtime.program.views[0].keys = vec![
            KeyBinding {
                key: "e".into(),
                modifiers: [KeyModifier::Control].into(),
                binding: "export_button".into(),
            },
            KeyBinding {
                key: "r".into(),
                modifiers: [KeyModifier::Control].into(),
                binding: "alternate".into(),
            },
        ];
        active
            .runtime
            .observation
            .enabled_actions
            .insert("alternate".into());
        active.program = active.runtime.program.identity().unwrap();
        frame(&mut h, &mut state, &view);
        for key in [Key::E, Key::R, Key::E] {
            h.key(key, true, Modifiers::CTRL);
            h.key(key, false, Modifiers::CTRL);
        }
        let mut result = frame(&mut h, &mut state, &view);
        for (index, expected) in ["export_button", "alternate", "export_button"]
            .iter()
            .enumerate()
        {
            let request = result
                .request
                .take()
                .expect("every fresh shortcut must be preserved");
            assert!(
                matches!(&request.action, WorkspaceAction::Daily { input: SemanticInput::Activate { binding, .. } } if binding == expected)
            );
            assert!(
                frame(&mut h, &mut state, &view).request.is_none(),
                "wait for the exact acknowledgement"
            );
            ack(&mut view, &request, index as u64 + 1);
            if fail_first {
                view.operation.as_mut().unwrap().outcome =
                    OperationOutcome::Rejected("Action rejected".into());
                assert!(frame(&mut h, &mut state, &view).request.is_none());
                assert!(frame(&mut h, &mut state, &view).request.is_none());
                break;
            }
            result = frame(&mut h, &mut state, &view);
        }
        if !fail_first {
            assert!(result.request.is_none());
        }
    }
}

#[test]
fn runtime_shortcuts_do_not_steal_keyboard_activation_from_workspace_editors() {
    for target in ["change.submit", "scope.preview", "adopt"] {
        let (mut h, mut state) = setup();
        let mut view = if target == "change.submit" {
            daily()
        } else {
            scope_view()
        };
        let add_enter = |runtime: &mut RuntimeView| {
            runtime.program.views[0].keys = vec![KeyBinding {
                key: "enter".into(),
                modifiers: BTreeSet::new(),
                binding: "export_button".into(),
            }];
        };
        let active = view.active.as_mut().unwrap();
        add_enter(&mut active.runtime);
        active.program = active.runtime.program.identity().unwrap();
        if let Some(comparison) = &mut view.comparison {
            add_enter(&mut comparison.before.runtime);
            add_enter(&mut comparison.after.runtime);
            comparison.before.artifact = artifact(&comparison.before.runtime);
            comparison.after.artifact = artifact(&comparison.after.runtime);
        }
        if target == "adopt" {
            add_adoption(&mut view);
        }
        if target == "change.submit" {
            click(&mut h, &mut state, &view, "change");
            type_in(
                &mut h,
                &mut state,
                &view,
                "change.text",
                "Use a clearer order",
            );
        }
        tab_to_control(&mut h, &mut state, &view, target);
        h.key(Key::Enter, true, Modifiers::NONE);
        let request = frame(&mut h, &mut state, &view)
            .request
            .expect("the focused workspace button must receive Enter");
        match target {
            "change.submit" => assert!(matches!(request.action, WorkspaceAction::Change { .. })),
            "scope.preview" => assert!(matches!(request.action, WorkspaceAction::SetScope { .. })),
            "adopt" => assert!(matches!(request.action, WorkspaceAction::Adopt { .. })),
            _ => unreachable!(),
        }
    }
}

#[test]
fn scope_review_distinguishes_exact_membership_and_predicates_before_rehearsal() {
    let populations = [
        (
            Population::Records {
                records: vec![record("one")],
            },
            "Included: person / one",
        ),
        (
            Population::Records {
                records: vec![record("two")],
            },
            "Included: person / two",
        ),
        (
            Population::Where {
                entity: "person".into(),
                predicate: Expr::Equal {
                    left: Box::new(Expr::Field {
                        record: Box::new(Expr::Variable {
                            name: "record".into(),
                        }),
                        field: "name".into(),
                    }),
                    right: Box::new(Expr::Literal {
                        value_type: Type::Text,
                        value: text("小明 🦉"),
                    }),
                },
            },
            "\"小明 🦉\"",
        ),
        (
            Population::Where {
                entity: "person".into(),
                predicate: Expr::Equal {
                    left: Box::new(Expr::Field {
                        record: Box::new(Expr::Variable {
                            name: "record".into(),
                        }),
                        field: "name".into(),
                    }),
                    right: Box::new(Expr::Literal {
                        value_type: Type::Text,
                        value: text("Other"),
                    }),
                },
            },
            "\"Other\"",
        ),
    ];
    let mut summaries = BTreeSet::new();
    for (population, expected) in populations {
        let (mut h, mut state) = setup();
        let mut view = scope_view();
        view.scope.as_mut().unwrap().proposed_scope.population = population.clone();
        let shown = frame(&mut h, &mut state, &view);
        assert!(
            shown.trace.text.iter().any(|line| line.contains(expected)),
            "missing exact boundary {expected}"
        );
        assert!(
            summaries.insert(shown.trace.text.join("\n")),
            "different populations must not look identical"
        );
        if matches!(population, Population::Where { .. }) {
            assert!(shown
                .trace
                .text
                .iter()
                .any(|line| line.contains("name") && line.contains("equals")));
        }
        let request = click(&mut h, &mut state, &view, "scope.preview")
            .request
            .unwrap();
        assert!(
            matches!(request.action, WorkspaceAction::SetScope { scope, .. } if scope.population == population)
        );
    }
}

#[test]
fn scope_operations_include_candidate_actions_and_preserve_explicit_narrowing() {
    for has_active in [true, false] {
        let (mut h, mut state) = setup();
        let mut view = scope_view();
        let after = &mut view.comparison.as_mut().unwrap().after;
        let mut added = after.runtime.program.actions[0].clone();
        added.id = "new_collection".into();
        added.label = "Gather new records".into();
        after.runtime.program.actions.push(added);
        after.artifact = artifact(&after.runtime);
        view.scope
            .as_mut()
            .unwrap()
            .proposed_scope
            .operations
            .insert("new_collection".into());
        if !has_active {
            view.active = None;
        }
        let shown = frame(&mut h, &mut state, &view);
        assert!(
            shown
                .trace
                .controls
                .contains_key("scope.operation.new_collection"),
            "candidate operations must remain editable"
        );
        click(&mut h, &mut state, &view, "scope.operation.new_collection");
        let request = click(&mut h, &mut state, &view, "scope.preview")
            .request
            .unwrap();
        assert!(
            matches!(request.action, WorkspaceAction::SetScope { scope, .. } if scope.operations == ["export_people".into()].into())
        );
    }
}
