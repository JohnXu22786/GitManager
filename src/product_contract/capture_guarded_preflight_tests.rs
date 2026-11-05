use super::super::super::app_length_tests::{capture_minimal, Work};
use super::*;
fn nested(depth: usize, optional: bool) -> Type {
    let mut t = Type::Boolean;
    for i in 0..depth {
        t = if optional && i % 2 == 0 {
            Type::Optional { item: Box::new(t) }
        } else {
            Type::List { item: Box::new(t) }
        };
    }
    t
}
fn dispose(mut t: Type) {
    loop {
        match t {
            Type::List { item } | Type::Optional { item } => t = *item,
            _ => break,
        }
    }
}
fn guard(app: &AppDefinition, w: &mut Work) -> ContractWorkResult<bool> {
    let state = WorkState::new(w);
    let a = Audit(Some(&state));
    state.finish((|| {
        a.pay(HEAD)?;
        preflight(app, a)
    })())
}
#[test]
fn guarded_preflight_depth_caps_and_late_entity_unsafe_clone_witness() {
    for depth in 0..=5 {
        for optional in [false, true] {
            let mut c = capture_minimal();
            c.program.entities[0].fields[0].value_type = nested(depth, optional);
            assert_eq!(
                guard(&c.program, &mut Work::default()).unwrap(),
                depth <= MAX_TYPE_DEPTH
            );
        }
    }
    let mut c = capture_minimal();
    let mut late = c.program.entities[0].clone();
    late.id = "late".into();
    late.fields[0].id = "payload".into();
    late.fields[0].value_type = nested(50000, false);
    c.program.entities.push(late);
    c.program.entities[0].constraints.push(Expr::Equal {
        left: Box::new(Expr::Field {
            record: Box::new(Expr::Literal {
                value_type: Type::Reference {
                    entity: "late".into(),
                },
                value: DataValue::Reference {
                    entity: "late".into(),
                    record: "r".into(),
                },
            }),
            field: "payload".into(),
        }),
        right: Box::new(Expr::Literal {
            value_type: Type::Boolean,
            value: DataValue::Boolean { value: true },
        }),
    });
    // Never invoke unguarded V1 or derived Clone/Serialize/Drop on this owner.
    let mut w = Work::default();
    let guard_result = guard(&c.program, &mut w);
    let guard_calls = w.calls.len();
    c.source_bytes = b"{".to_vec();
    reset_validation_counts();
    // A failed negative must not invoke the unsafe clone just to fail the test.
    let source_result = if matches!(guard_result, Ok(false)) {
        let mut w = Work::default();
        let state = WorkState::new(&mut w);
        Some(state.finish(c.validate_audited(Audit(Some(&state))).map(|_| ())))
    } else {
        None
    };
    let attempted = validation_counts().apps;
    // Always dispose the deep owner iteratively, even when an assertion fails.
    let t = std::mem::replace(
        &mut c.program.entities[1].fields[0].value_type,
        Type::Boolean,
    );
    dispose(t);
    assert_eq!(guard_result, Ok(false));
    assert!(
        guard_calls < 30,
        "borrowed preflight never traverses the arbitrary tail"
    );
    assert!(matches!(
        source_result,
        Some(Err(ContractWorkError::Contract(_)))
    ));
    assert_eq!(
        attempted, 0,
        "guard must miss before the unsafe semantic clone"
    );
    let mut app = c.program;
    app.entities.clear();
    assert!(guard(&app, &mut Work::default()).unwrap());
    let entity = capture_minimal().program.entities.remove(0);
    app.entities = vec![entity.clone(); MAX_ITEMS];
    assert!(guard(&app, &mut Work::default()).unwrap());
    app.entities.push(entity.clone());
    assert!(!guard(&app, &mut Work::default()).unwrap());
    app.entities = vec![entity];
    let field = app.entities[0].fields[0].clone();
    app.entities[0].fields = vec![field.clone(); MAX_ITEMS];
    assert!(guard(&app, &mut Work::default()).unwrap());
    app.entities[0].fields.push(field);
    assert!(!guard(&app, &mut Work::default()).unwrap());
}
#[test]
fn guarded_preflight_refusals_never_advance_or_clear_first_cause() {
    let c = capture_minimal();
    let mut w = Work::default();
    assert!(guard(&c.program, &mut w).unwrap());
    for i in 1..=w.calls.len() {
        for cause in [
            WorkFailure::Cancelled,
            WorkFailure::BudgetExhausted("guard"),
        ] {
            let mut stopped = Work {
                refuse: Some(i),
                cause: Some(cause.clone()),
                ..Work::default()
            };
            assert_eq!(
                guard(&c.program, &mut stopped).unwrap_err(),
                ContractWorkError::Resource(cause)
            );
            assert_eq!(stopped.calls, &w.calls[..i]);
        }
    }
}
#[test]
fn guarded_packet_layouts_and_exact_closure_captures_are_funded() {
    assert!(
        COMMON >= 4096
            && SOURCE_DELTA_ROOT >= 2048
            && HEAD >= 2048
            && ENTITY >= 256
            && FIELD >= 256
            && TYPE_STEP >= 128
    );
    assert!(MATERIAL_ROOT >= 4096 && VALUE_FRAME >= 1024);
    #[cfg(target_pointer_width = "64")]
    {
        assert_eq!(ENTITY, 264);
        assert_eq!(TYPE_STEP, 184);
    }
    eprintln!(
        "GUARDED_LAYOUT {}",
        serde_json::json!({"common_slots":COMMON_SLOTS,"head_slots":HEAD_SLOTS,"entity_slots":ENTITY_SLOTS,"field_slots":FIELD_SLOTS,"type_slots":TYPE_SLOTS,"delta_slots":SOURCE_DELTA_SLOTS,"material_root_slots":MATERIAL_ROOT_SLOTS,"value_iter":VALUE_ITER,"common":COMMON,"delta_root":SOURCE_DELTA_ROOT,"head":HEAD,"entity":ENTITY,"field":FIELD,"type_step":TYPE_STEP,"value_frame":VALUE_FRAME,"material_root":MATERIAL_ROOT,"material_stack":MATERIAL_STACK,"output_frame":OUTPUT_FRAME,"output_stack":OUTPUT_STACK_ROOT,"shape":size_of::<Shape>(),"scalars":size_of::<SemanticScalars>(),"native_value_result":size_of::<serde_json::Result<JsonValue>>(),"native_map":size_of::<<serde_json::value::Serializer as serde::Serializer>::SerializeMap>(),"native_seq":size_of::<<serde_json::value::Serializer as serde::Serializer>::SerializeSeq>()})
    );
    let c = capture_minimal();
    let a = Audit::none();
    let lexical = move |n: u64| a.pay(n);
    assert_eq!(std::mem::size_of_val(&lexical), size_of::<Audit>());
    let context = (&c, a);
    let closure = move || {
        let (c, a) = context;
        (c.source_bytes.len(), a.enabled())
    };
    assert!(std::mem::size_of_val(&closure) <= size_of::<Context>());
}
