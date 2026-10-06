//! Fictional, hand-authored production-language fixtures, not provider evidence.
use crate::product_contract::*;
use crate::product_store::{scope::*, ProductStore, ProjectSnapshot};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn lit(value: DataValue, value_type: Type) -> Expr {
    Expr::Literal { value_type, value }
}
pub fn int(value: i64) -> Expr {
    lit(DataValue::Integer { value }, Type::Integer)
}
pub fn boolean(value: bool) -> Expr {
    lit(DataValue::Boolean { value }, Type::Boolean)
}
pub fn var(name: &str) -> Expr {
    Expr::Variable { name: name.into() }
}
pub fn field(binding: &str, name: &str) -> Expr {
    Expr::Field {
        record: Box::new(var(binding)),
        field: name.into(),
    }
}
pub fn text(value: &str) -> DataValue {
    DataValue::Text {
        value: value.into(),
    }
}
pub fn reference(row: &Record) -> DataValue {
    DataValue::Reference {
        entity: row.entity.clone(),
        record: row.id.clone(),
    }
}
pub fn invoke(action: &str, values: &[(&str, DataValue)]) -> SemanticInput {
    SemanticInput::Invoke {
        action: action.into(),
        arguments: values
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}
pub fn elapsed(binding: &str, pause: bool) -> Expr {
    let elapsed = Expr::DateDifference {
        later: Box::new(Expr::Today),
        earlier: Box::new(field(binding, "started")),
    };
    if !pause {
        return elapsed;
    }
    Expr::Subtract {
        left: Box::new(elapsed),
        right: Box::new(Expr::Add {
            left: Box::new(field(binding, "waited")),
            right: Box::new(Expr::If {
                condition: Box::new(field(binding, "waiting")),
                then_value: Box::new(Expr::DateDifference {
                    later: Box::new(Expr::Today),
                    earlier: Box::new(field(binding, "wait_start")),
                }),
                else_value: Box::new(int(0)),
            }),
        }),
    }
}
pub fn program(pause: bool) -> CapturedProgram {
    let query = json!({"kind":"query","entity":"job","binding":"q","predicate":boolean(true),"sort":[],"limit":1000,"include_archived":true});
    let value = json!({"version":1,"id":"studio","label":"Fictional frame studio",
        "entities":[{"id":"job","label":"Work","fields":[
            {"id":"name","label":"Name","value_type":{"kind":"text"}},
            {"id":"started","label":"Started","value_type":{"kind":"date"}},
            {"id":"promised","label":"Customer commitment","value_type":{"kind":"date"}},
            {"id":"waiting","label":"Waiting","value_type":{"kind":"boolean"}},
            {"id":"wait_start","label":"Wait began","value_type":{"kind":"date"}},
            {"id":"waited","label":"Completed waiting intervals","value_type":{"kind":"integer"}},
            {"id":"done","label":"Completed","value_type":{"kind":"boolean"}},
            {"id":"production","label":"Recorded production days","value_type":{"kind":"integer"}}
        ],"unique":[],"constraints":[]}],"state":[],"actions":[
            {"id":"add","label":"Add work","parameters":{"name":{"kind":"text"},"promised":{"kind":"date"}},"guards":[],"steps":[
                {"kind":"create","entity":"job","bind":"new","values":{"name":var("name"),"started":{"kind":"today"},"promised":var("promised"),"waiting":boolean(false),"wait_start":{"kind":"today"},"waited":int(0),"done":boolean(false),"production":int(0)}}],"ensures":[]},
            {"id":"wait","label":"Wait for materials","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"waiting":boolean(true),"wait_start":{"kind":"today"}}}],"ensures":[]},
            {"id":"resume","label":"Resume production","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"waiting":boolean(false),"waited":{"kind":"add","left":field("row","waited"),"right":{"kind":"date_difference","later":{"kind":"today"},"earlier":field("row","wait_start")}}}}],"ensures":[]},
            {"id":"calculate","label":"Record production","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"production":elapsed("row",pause)}}],"ensures":[]},
            {"id":"complete","label":"Complete work","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"update","record":var("row"),"values":{"production":elapsed("row",pause),"done":boolean(true)}}],"ensures":[]},
            {"id":"archive","label":"Archive work","parameters":{"row":{"kind":"reference","entity":"job"}},"guards":[],"steps":[{"kind":"archive","record":var("row")}],"ensures":[]},
            {"id":"export","label":"Export current and preserved results","parameters":{},"guards":[],"steps":[{"kind":"emit","output":"sheet","items":query,"binding":"row","columns":{"name":field("row","name"),"production":elapsed("row",pause),"promised":field("row","promised"),"reminder":field("row","waiting")}}],"ensures":[]}
        ],"outputs":[{"id":"sheet","label":"Work results","format":"csv","columns":[{"id":"name","label":"Name","value_type":{"kind":"text"}},{"id":"production","label":"Production days","value_type":{"kind":"integer"}},{"id":"promised","label":"Customer commitment","value_type":{"kind":"date"}},{"id":"reminder","label":"Follow up materials","value_type":{"kind":"boolean"}}]}],"observables":[],
        "views":[{"id":"work","label":"Work","kind":{"kind":"list","entity":"job","rows":query,"columns":[{"id":"name","label":"Name","value":field("row","name")},{"id":"production","label":"Production days","value":elapsed("row",pause)},{"id":"promised","label":"Customer commitment","value":field("row","promised")},{"id":"reminder","label":"Follow up materials","value":field("row","waiting")}],"controls":[],"selection":null},"actions":[],"keys":[]}],"initial_view":"work"});
    capture(value)
}
pub fn capture(value: Value) -> CapturedProgram {
    CapturedProgram::capture(
        &serde_json::to_vec(&value).unwrap(),
        "scope-project",
        Producer::Fixture {
            name: "scoped production workflow".into(),
        },
        None,
    )
    .unwrap()
}
pub fn request(snapshot: &ProjectSnapshot, population: ScopePopulation) -> ScopeRequest {
    ScopeRequest {
        population,
        operations: ["calculate", "complete", "export"]
            .into_iter()
            .map(String::from)
            .collect::<BTreeSet<_>>(),
        excluded_records: vec![],
        lifecycles: vec![LifecycleBinding {
            entity: "job".into(),
            completed: field("record", "done"),
            source: snapshot.active_revision.clone(),
        }],
        patches: vec![
            patch(
                EffectDestination::Update {
                    action: "calculate".into(),
                    path: vec![0],
                    field: "production".into(),
                },
                "row",
            ),
            patch(
                EffectDestination::Update {
                    action: "complete".into(),
                    path: vec![0],
                    field: "production".into(),
                },
                "row",
            ),
            patch(
                EffectDestination::EmitColumn {
                    action: "export".into(),
                    path: vec![0],
                    column: "production".into(),
                },
                "row",
            ),
            patch(
                EffectDestination::ViewColumn {
                    view: "work".into(),
                    column: "production".into(),
                },
                "row",
            ),
        ],
    }
}
fn patch(destination: EffectDestination, subject: &str) -> EffectPatchRequest {
    EffectPatchRequest {
        destination,
        entity: "job".into(),
        subject: subject.into(),
        value_type: Type::Integer,
    }
}
pub fn apply(store: &ProductStore, id: &str, input: SemanticInput) -> ProjectSnapshot {
    store
        .apply(
            store.load().unwrap().revision,
            id,
            &input,
            RuntimeLimits::default(),
        )
        .unwrap()
}
pub fn add(store: &ProductStore, id: &str, name: &str) -> Record {
    let s = apply(
        store,
        id,
        invoke(
            "add",
            &[
                ("name", text(name)),
                ("promised", DataValue::Date { days: 20020 }),
            ],
        ),
    );
    s.data.records.last().unwrap().clone()
}
pub fn tick(store: &ProductStore, id: &str, day: i32) -> ProjectSnapshot {
    apply(
        store,
        id,
        SemanticInput::AdvanceClock {
            days: (day - store.load().unwrap().clock_day) as u32,
        },
    )
}
pub fn action(store: &ProductStore, id: &str, name: &str, row: &Record) -> ProjectSnapshot {
    apply(store, id, invoke(name, &[("row", reference(row))]))
}
pub fn row<'a>(snapshot: &'a ProjectSnapshot, original: &Record) -> &'a Record {
    snapshot
        .data
        .records
        .iter()
        .find(|r| r.id == original.id)
        .unwrap()
}
