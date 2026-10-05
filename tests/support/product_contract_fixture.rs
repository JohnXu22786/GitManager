use serde_json::{json, Value};

pub fn text(value: &str) -> Value {
    json!({"kind":"literal", "value_type":{"kind":"text"}, "value":{"kind":"text","value":value}})
}
pub fn yes() -> Value {
    json!({"kind":"literal", "value_type":{"kind":"boolean"}, "value":{"kind":"boolean","value":true}})
}
pub fn var(name: &str) -> Value {
    json!({"kind":"variable","name":name})
}
pub fn field(record: Value, name: &str) -> Value {
    json!({"kind":"field","record":record,"field":name})
}
pub fn query(entity: &str) -> Value {
    json!({"kind":"query","entity":entity,"binding":"item","predicate":yes(),"sort":[],"limit":100,"include_archived":false})
}
pub fn empty(entity: &str) -> Value {
    json!({"kind":"list","items":[],"item_type":{"kind":"reference","entity":entity}})
}

// Hand-authored structural fixtures, never live provider or product acceptance evidence.
pub fn organizer() -> Value {
    json!({
        "version":1,"id":"organizer","label":"Club organizer",
        "entities":[{"id":"person","label":"People","fields":[
            {"id":"name","label":"Name","value_type":{"kind":"text"}},
            {"id":"area","label":"Area","value_type":{"kind":"text"}}
        ],"unique":[],"constraints":[]}],
        "state":[
            {"id":"search","label":"Search","value_type":{"kind":"text"},"initial":{"kind":"text","value":""}},
            {"id":"selected","label":"Selected","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"initial":{"kind":"list","items":[],"item_type":{"kind":"reference","entity":"person"}}}
        ],
        "actions":[
            {"id":"add_person","label":"Add person","parameters":{"name":{"kind":"text"},"area":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"person","values":{"name":var("name"),"area":var("area")},"bind":"created"}],"ensures":[]},
            {"id":"collect","label":"Collect current results","parameters":{},"guards":[],"steps":[{"kind":"collection","target":{"kind":"state","state":"selected"},"operation":"insert","items":query("person")}],"ensures":[]},
            {"id":"export_people","label":"Export selected","parameters":{},"guards":[],"steps":[{"kind":"emit","output":"roster","items":{"kind":"state","state":"selected"},"binding":"person","columns":{"name":field(var("person"),"name")}}],"ensures":[]}
        ],
        "outputs":[{"id":"roster","label":"Roster","format":"csv","columns":[{"id":"name","label":"Name","value_type":{"kind":"text"}}]}],
        "observables":[{"id":"selected_count","label":"Selected count","value":{"kind":"count","items":{"kind":"state","state":"selected"}}}],
        "views":[
            {"id":"people","label":"People","kind":{"kind":"list","entity":"person","rows":query("person"),"columns":[{"id":"name","label":"Name","value":field(var("row"),"name")}],"controls":[{"id":"search_input","label":"Search","state":"search","on_change":null}],"selection":{"id":"pick","state":"selected","on_change":null}},"actions":[{"id":"export_button","label":"Export","placement":"toolbar","action":"export_people","arguments":{},"enabled":yes()}],"keys":[{"key":"e","modifiers":["control"],"binding":"export_button"}]},
            {"id":"new_person","label":"New person","kind":{"kind":"form","action":"add_person","fields":[{"parameter":"name","label":"Name"},{"parameter":"area","label":"Area"}],"defaults":{}},"actions":[],"keys":[]}
        ],"initial_view":"people"
    })
}

pub fn equipment() -> Value {
    let mut app = organizer();
    app["id"] = json!("equipment");
    app["label"] = json!("Equipment loans");
    app["entities"] = json!([
        {"id":"asset","label":"Assets","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}}],"unique":[["name"]],"constraints":[]},
        {"id":"loan","label":"Loans","fields":[{"id":"asset","label":"Asset","value_type":{"kind":"reference","entity":"asset"}},{"id":"returned","label":"Returned","value_type":{"kind":"boolean"}}],"unique":[],"constraints":[]}
    ]);
    app["state"] = json!([]);
    app["actions"] = json!([
        {"id":"borrow","label":"Borrow asset","parameters":{"asset":{"kind":"reference","entity":"asset"}},"guards":[{"kind":"not","value":{"kind":"any","items":query("loan"),"binding":"loan","predicate":{"kind":"and","values":[{"kind":"equal","left":field(var("loan"),"asset"),"right":var("asset")},{"kind":"not","value":field(var("loan"),"returned")}]}}}],"steps":[{"kind":"create","entity":"loan","values":{"asset":var("asset"),"returned":{"kind":"literal","value_type":{"kind":"boolean"},"value":{"kind":"boolean","value":false}}},"bind":"loan"}],"ensures":[]},
        {"id":"return_loan","label":"Return","parameters":{"loan":{"kind":"reference","entity":"loan"}},"guards":[],"steps":[{"kind":"update","record":var("loan"),"values":{"returned":yes()}}],"ensures":[]}
    ]);
    app["outputs"] = json!([]);
    app["observables"] = json!([{"id":"loan_count","label":"Loan count","value":{"kind":"count","items":query("loan")}}]);
    app["views"] = json!([{"id":"assets","label":"Assets","kind":{"kind":"list","entity":"asset","rows":query("asset"),"columns":[{"id":"name","label":"Name","value":field(var("row"),"name")}],"controls":[],"selection":null},"actions":[{"id":"borrow_asset","label":"Borrow","placement":"row","action":"borrow","arguments":{"asset":var("row")},"enabled":yes()}],"keys":[]}]);
    app["initial_view"] = json!("assets");
    app
}

pub fn new_composition() -> Value {
    let mut app = organizer();
    app["entities"].as_array_mut().unwrap().push(json!({"id":"collection","label":"Saved collections","fields":[{"id":"name","label":"Name","value_type":{"kind":"text"}},{"id":"members","label":"Members","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}}}],"unique":[["name"]],"constraints":[]}));
    app["actions"].as_array_mut().unwrap().push(json!({"id":"save_collection","label":"Save collection","parameters":{"name":{"kind":"text"}},"guards":[],"steps":[{"kind":"create","entity":"collection","values":{"name":var("name"),"members":{"kind":"state","state":"selected"}},"bind":"collection"},{"kind":"set_state","state":"selected","value":{"kind":"literal","value_type":{"kind":"list","item":{"kind":"reference","entity":"person"}},"value":empty("person")}}],"ensures":[]}));
    app
}
