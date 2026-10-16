//! Generic hand-authored lifecycle/registry fixture. Never provider evidence.
use crate::product_contract::*;
use crate::product_store::{scope::*, ProductStore};
use serde_json::json;
use std::collections::BTreeMap;
pub fn lit(v: DataValue, t: Type) -> Expr {
    Expr::Literal {
        value_type: t,
        value: v,
    }
}
pub fn text(v: &str) -> Expr {
    lit(DataValue::Text { value: v.into() }, Type::Text)
}
pub fn boolean(v: bool) -> Expr {
    lit(DataValue::Boolean { value: v }, Type::Boolean)
}
pub fn int(v: i64) -> Expr {
    lit(DataValue::Integer { value: v }, Type::Integer)
}
pub fn var(v: &str) -> Expr {
    Expr::Variable { name: v.into() }
}
pub fn field(v: &str, f: &str) -> Expr {
    get(var(v), f)
}
pub fn get(v: Expr, f: &str) -> Expr {
    Expr::Field {
        record: Box::new(v),
        field: f.into(),
    }
}
pub fn eq(a: Expr, b: Expr) -> Expr {
    Expr::Equal {
        left: Box::new(a),
        right: Box::new(b),
    }
}
pub fn not(v: Expr) -> Expr {
    Expr::Not { value: Box::new(v) }
}
pub fn and(v: Vec<Expr>) -> Expr {
    Expr::And { values: v }
}
pub fn cond(c: Expr, a: Expr, b: Expr) -> Expr {
    Expr::If {
        condition: Box::new(c),
        then_value: Box::new(a),
        else_value: Box::new(b),
    }
}
pub fn done(r: &str) -> Expr {
    eq(field(r, "status"), text("done"))
}
pub fn query(e: &str, b: &str, p: Expr) -> Expr {
    Expr::Query {
        entity: e.into(),
        binding: b.into(),
        predicate: Box::new(p),
        sort: vec![],
        limit: MAX_COLLECTION,
        include_archived: false,
    }
}
pub fn count(v: Expr) -> Expr {
    Expr::Count { items: Box::new(v) }
}
pub fn vals(v: Vec<(&str, Expr)>) -> BTreeMap<Id, Expr> {
    v.into_iter().map(|(k, v)| (k.into(), v)).collect()
}
pub fn update(r: Expr, v: Vec<(&str, Expr)>) -> Statement {
    Statement::Update {
        record: r,
        values: vals(v),
    }
}
pub fn create(e: &str, b: &str, v: Vec<(&str, Expr)>) -> Statement {
    Statement::Create {
        entity: e.into(),
        bind: b.into(),
        values: vals(v),
    }
}
pub fn each(items: Expr, b: &str, steps: Vec<Statement>) -> Statement {
    Statement::ForEach {
        items,
        binding: b.into(),
        steps,
    }
}
pub fn action(
    id: &str,
    p: Vec<(&str, Type)>,
    g: Vec<Expr>,
    steps: Vec<Statement>,
) -> ActionDefinition {
    ActionDefinition {
        id: id.into(),
        label: id.into(),
        parameters: p.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        guards: g,
        steps,
        ensures: vec![],
    }
}
pub fn capture(app: &AppDefinition) -> CapturedProgram {
    CapturedProgram::capture(
        &serde_json::to_vec(app).unwrap(),
        "fact-fixture",
        Producer::Fixture {
            name: "generic common factual admission".into(),
        },
        None,
    )
    .unwrap()
}
fn entity(id: &str, fields: Vec<(&str, Type)>, unique: Vec<Vec<&str>>) -> EntityDefinition {
    EntityDefinition {
        id: id.into(),
        label: id.into(),
        fields: fields
            .into_iter()
            .map(|(id, value_type)| FieldDefinition {
                id: id.into(),
                label: id.into(),
                value_type,
            })
            .collect(),
        unique: unique
            .into_iter()
            .map(|x| x.into_iter().map(str::to_owned).collect())
            .collect(),
        constraints: vec![],
    }
}
pub fn pending(r: &str) -> Expr {
    not(eq(
        count(query(
            "pause",
            "p",
            and(vec![eq(field("p", "job"), var(r)), field("p", "pending")]),
        )),
        int(0),
    ))
}
pub fn status(r: &str, new: bool) -> Expr {
    let ready = Expr::All {
        items: Box::new(query(
            "requirement",
            "q",
            and(vec![eq(field("q", "job"), var(r)), field("q", "required")]),
        )),
        binding: "ready_link".into(),
        predicate: Box::new(get(field("ready_link", "material"), "received")),
    };
    let active = eq(field(r, "status"), text("active"));
    cond(
        done(r),
        text("done"),
        cond(
            ready,
            cond(
                if new {
                    Expr::Or {
                        values: vec![active, pending(r)],
                    }
                } else {
                    active
                },
                text("active"),
                text("ready"),
            ),
            text("waiting"),
        ),
    )
}
fn sync(new: bool) -> Statement {
    let mut steps = vec![update(var("job"), vec![("status", status("job", new))])];
    if new {
        steps.push(each(
            query("pause", "p", eq(field("p", "job"), var("job"))),
            "memory",
            vec![update(var("memory"), vec![("pending", boolean(false))])],
        ))
    }
    each(query("job", "j", not(done("j"))), "job", steps)
}
pub fn pair() -> (CapturedProgram, CapturedProgram) {
    let mut a = AppDefinition {
        version: 1,
        id: "common_facts".into(),
        label: "Common facts".into(),
        entities: vec![
            entity(
                "job",
                vec![("title", Type::Text), ("status", Type::Text)],
                vec![],
            ),
            entity(
                "material",
                vec![("name", Type::Text), ("received", Type::Boolean)],
                vec![vec!["name"]],
            ),
            entity(
                "requirement",
                vec![
                    ("job", Type::reference("job")),
                    ("material", Type::reference("material")),
                    ("required", Type::Boolean),
                ],
                vec![vec!["job", "material"]],
            ),
            entity(
                "event",
                vec![
                    ("material", Type::reference("material")),
                    ("sequence", Type::Integer),
                    ("note", Type::Text),
                ],
                vec![],
            ),
        ],
        state: vec![StateDefinition {
            id: "name_input".into(),
            label: "Name".into(),
            value_type: Type::Text,
            initial: DataValue::Text {
                value: String::new(),
            },
        }],
        actions: vec![],
        outputs: vec![],
        observables: vec![],
        views: vec![],
        initial_view: "jobs".into(),
    };
    a.actions.push(action(
        "add",
        vec![("title", Type::Text)],
        vec![],
        vec![create(
            "job",
            "made",
            vec![("title", var("title")), ("status", text("ready"))],
        )],
    ));
    a.actions.push(action(
        "complete",
        vec![("job", Type::reference("job"))],
        vec![eq(field("job", "status"), text("ready"))],
        vec![update(var("job"), vec![("status", text("done"))])],
    ));
    let registry = vec![
        each(
            query(
                "job",
                "j",
                and(vec![
                    eq(var("j"), var("owner")),
                    eq(
                        count(query("material", "m", eq(field("m", "name"), var("name")))),
                        int(0),
                    ),
                ]),
            ),
            "gate",
            vec![create(
                "material",
                "made",
                vec![("name", var("name")), ("received", boolean(false))],
            )],
        ),
        each(
            query("material", "m", eq(field("m", "name"), var("name"))),
            "material",
            vec![each(
                query(
                    "job",
                    "j",
                    and(vec![
                        eq(var("j"), var("owner")),
                        eq(
                            count(query(
                                "requirement",
                                "r",
                                and(vec![
                                    eq(field("r", "job"), var("owner")),
                                    eq(field("r", "material"), var("material")),
                                ]),
                            )),
                            int(0),
                        ),
                    ]),
                ),
                "link_gate",
                vec![create(
                    "requirement",
                    "made_link",
                    vec![
                        ("job", var("owner")),
                        ("material", var("material")),
                        ("required", boolean(true)),
                    ],
                )],
            )],
        ),
        sync(false),
        Statement::SetState {
            state: "name_input".into(),
            value: text(""),
        },
    ];
    a.actions.push(action(
        "register",
        vec![("owner", Type::reference("job")), ("name", Type::Text)],
        vec![not(done("owner"))],
        registry,
    ));
    a.actions.push(action(
        "remove",
        vec![("link", Type::reference("requirement"))],
        vec![not(eq(get(field("link", "job"), "status"), text("done")))],
        vec![
            update(var("link"), vec![("required", boolean(false))]),
            sync(false),
        ],
    ));
    a.actions.push(action(
        "withdraw",
        vec![
            ("material", Type::reference("material")),
            ("note", Type::Text),
        ],
        vec![field("material", "received")],
        vec![
            update(var("material"), vec![("received", boolean(false))]),
            create(
                "event",
                "event",
                vec![
                    ("material", var("material")),
                    (
                        "sequence",
                        Expr::Add {
                            left: Box::new(count(query("event", "e", boolean(true)))),
                            right: Box::new(int(1)),
                        },
                    ),
                    ("note", var("note")),
                ],
            ),
            sync(false),
        ],
    ));
    a.views.push(serde_json::from_value(json!({"id":"jobs","label":"Jobs","kind":{"kind":"list","entity":"job","rows":query("job","j",boolean(true)),"columns":[{"id":"title","label":"Title","value":field("row","title")},{"id":"status","label":"Status","value":status("row",false)}],"controls":[{"id":"name","label":"Name","state":"name_input","on_change":null}],"selection":null},"actions":[{"id":"register","label":"Register","placement":"row","action":"register","arguments":{"owner":var("row"),"name":{"kind":"state","state":"name_input"}},"enabled":not(done("row"))}],"keys":[]})).unwrap());
    let mut rows = query("event", "e", boolean(true));
    if let Expr::Query { sort, .. } = &mut rows {
        sort.push(SortKey {
            value: field("e", "sequence"),
            descending: true,
        })
    }
    a.views.push(serde_json::from_value(json!({"id":"history","label":"History","kind":{"kind":"list","entity":"event","rows":rows,"columns":[{"id":"note","label":"Note","value":field("row","note")}],"controls":[],"selection":null},"actions":[],"keys":[]})).unwrap());
    a.observables.push(ObservableDefinition {
        id: "ready_count".into(),
        label: "Ready".into(),
        value: count(query("job", "j", eq(status("j", false), text("ready")))),
    });
    let mut b = a.clone();
    b.entities.push(entity(
        "pause",
        vec![("job", Type::reference("job")), ("pending", Type::Boolean)],
        vec![vec!["job"]],
    ));
    for act in &mut b.actions {
        for step in &mut act.steps {
            if matches!(step,Statement::ForEach{binding,..} if binding=="job") {
                *step = sync(true)
            }
        }
    }
    let prelude = each(
        query("job", "j", eq(field("j", "status"), text("active"))),
        "paused_job",
        vec![each(
            query(
                "job",
                "g",
                and(vec![
                    eq(var("g"), var("paused_job")),
                    eq(
                        count(query(
                            "pause",
                            "p",
                            eq(field("p", "job"), var("paused_job")),
                        )),
                        int(0),
                    ),
                ]),
            ),
            "capture_gate",
            vec![create(
                "pause",
                "memory",
                vec![("job", var("paused_job")), ("pending", boolean(true))],
            )],
        )],
    );
    b.actions
        .iter_mut()
        .find(|x| x.id == "withdraw")
        .unwrap()
        .steps
        .insert(0, prelude);
    if let ViewKind::List { columns, .. } = &mut b.views[0].kind {
        columns[1].value = status("row", true);
        columns.push(Column {
            id: "pending".into(),
            label: "Pending".into(),
            value: pending("row"),
        })
    }
    b.observables[0].value = count(query("job", "j", eq(status("j", true), text("ready"))));
    (capture(&a), capture(&b))
}
pub fn endpoint(
    p: &CapturedProgram,
    action: &str,
    path: Vec<usize>,
    start: usize,
    len: usize,
    root: Option<&str>,
) -> OwnedSpanEndpoint {
    let mut steps = p
        .program
        .actions
        .iter()
        .find(|x| x.id == action)
        .unwrap()
        .steps
        .as_slice();
    for i in &path {
        let Statement::ForEach { steps: inner, .. } = &steps[*i] else {
            panic!()
        };
        steps = inner;
    }
    OwnedSpanEndpoint {
        source: canonical_digest(IdentityDomain::Source, p).unwrap(),
        action: action.into(),
        list_path: path,
        start,
        len,
        kinds: steps[start..start + len]
            .iter()
            .map(OwnedStatementKind::of)
            .collect(),
        digest: canonical_digest(
            IdentityDomain::Evidence,
            &("owned-span/1", &steps[start..start + len]),
        )
        .unwrap(),
        previous: start
            .checked_sub(1)
            .map(|i| canonical_digest(IdentityDomain::Evidence, &steps[i]).unwrap()),
        next: steps
            .get(start + len)
            .map(|s| canonical_digest(IdentityDomain::Evidence, s).unwrap()),
        root: root.map(|n| OwnedRootBinding {
            name: n.into(),
            value_type: Type::reference("job"),
        }),
    }
}
pub fn request(
    store: &ProductStore,
    a: &CapturedProgram,
    b: &CapturedProgram,
) -> OwnedPreviewRequest {
    let mut edits = vec![OwnedSpanEdit {
        id: "capture".into(),
        before: endpoint(a, "withdraw", vec![], 0, 0, None),
        after: endpoint(b, "withdraw", vec![], 0, 1, None),
    }];
    for (id, old, new) in [("register", 2, 2), ("remove", 1, 1), ("withdraw", 2, 3)] {
        edits.push(OwnedSpanEdit {
            id: id.into(),
            before: endpoint(a, id, vec![old], 0, 1, Some("job")),
            after: endpoint(b, id, vec![new], 0, 2, Some("job")),
        })
    }
    OwnedPreviewRequest {
        inventory: CommonFactInventoryRequest {
            snapshot: canonical_digest(IdentityDomain::Data, &store.load().unwrap()).unwrap(),
            sources: OwnedSourceRequest {
                before: canonical_digest(IdentityDomain::Source, a).unwrap(),
                after: canonical_digest(IdentityDomain::Source, b).unwrap(),
                root_entity: "job".into(),
                edits,
            },
            lifecycle: CommonFactLifecycleSuggestion {
                entity: "job".into(),
                before: done("record"),
                after: done("record"),
                meaning: "Done jobs are completed history; all other jobs remain unfinished".into(),
            },
        },
        auxiliary: vec![OwnedAuxiliary {
            entity: "pause".into(),
            owner_field: "job".into(),
        }],
        population: ScopePopulation::All,
        excluded_records: vec![],
        operations: a.program.actions.iter().map(|x| x.id.clone()).collect(),
        availability: OwnedPreviewAvailability::NullableAddedBoolean,
        purpose: "Inspect one isolated mixed-cohort preview; do not adopt".into(),
    }
}
