//! One renderer for adopted tools and copied comparison runs. All displayed
//! values and enabled actions come from runtime observations, never AST evaluation.
use crate::product_contract::*;
use crate::product_protocol::RuntimeView;
use chrono::{Days, NaiveDate};
use egui::{Button, Key, Response, TextEdit, Ui};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Default)]
pub struct WidgetTrace {
    pub(crate) interacted: bool,
    #[cfg(test)]
    pub controls: BTreeMap<String, ControlGeometry>,
    #[cfg(test)]
    pub text: Vec<String>,
}
#[cfg(test)]
pub struct ControlGeometry {
    pub id: egui::Id,
    pub rect: egui::Rect,
    pub enabled: bool,
}
impl WidgetTrace {
    pub(crate) fn control(&mut self, key: &str, response: Response) -> Response {
        self.interacted |= response.clicked() || response.changed() || response.gained_focus();
        #[cfg(test)]
        self.controls.insert(
            key.into(),
            ControlGeometry {
                id: response.id,
                rect: response.rect,
                enabled: response.enabled(),
            },
        );
        #[cfg(not(test))]
        let _ = key;
        response
    }
    pub(crate) fn label(&mut self, ui: &mut Ui, text: impl Into<String>) {
        let text = text.into();
        #[cfg(test)]
        self.text.push(text.clone());
        ui.add(egui::Label::new(text).wrap());
    }
    pub(crate) fn button(&mut self, ui: &mut Ui, key: &str, label: &str, enabled: bool) -> bool {
        self.control(key, ui.add_enabled(enabled, Button::new(label).wrap()))
            .clicked()
    }
    pub(crate) fn append(&mut self, other: Self) {
        self.interacted |= other.interacted;
        #[cfg(test)]
        {
            self.controls.extend(other.controls);
            self.text.extend(other.text);
        }
        #[cfg(not(test))]
        let _ = other;
    }
}

pub(crate) fn value_text(value: &DataValue) -> String {
    match value {
        DataValue::Null => "Not set".into(),
        DataValue::Boolean { value } => if *value { "Yes" } else { "No" }.into(),
        DataValue::Integer { value } => value.to_string(),
        DataValue::Text { value } => value.clone(),
        DataValue::Date { days } => date(*days)
            .map(|d| d.to_string())
            .unwrap_or_else(|| "Invalid date".into()),
        DataValue::Reference { entity, record } => format!("{entity}: {record}"),
        DataValue::List { items, .. } => {
            items.iter().map(value_text).collect::<Vec<_>>().join(", ")
        }
    }
}
fn date(days: i32) -> Option<NaiveDate> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)?;
    if days >= 0 {
        epoch.checked_add_days(Days::new(days as u64))
    } else {
        epoch.checked_sub_days(Days::new(days.unsigned_abs() as u64))
    }
}

/// Uncommitted widget state, never an executable plan or authoritative session.
#[derive(Clone)]
pub(crate) struct ValueEditor {
    text: String,
    checked: bool,
    present: bool,
    reference: Option<RecordRef>,
    children: Vec<ValueEditor>,
}
impl ValueEditor {
    pub(crate) fn new(typ: &Type, value: Option<&DataValue>) -> Self {
        let mut result = Self {
            text: String::new(),
            checked: false,
            present: false,
            reference: None,
            children: vec![],
        };
        match typ {
            Type::Optional { item } => {
                result.present = value.is_some_and(|v| *v != DataValue::Null);
                result
                    .children
                    .push(Self::new(item, value.filter(|v| **v != DataValue::Null)));
            }
            Type::List { item } => {
                if let Some(DataValue::List { items, .. }) = value {
                    result.children = items.iter().map(|v| Self::new(item, Some(v))).collect();
                }
            }
            Type::Reference { .. } => {
                if let Some(DataValue::Reference { entity, record }) = value {
                    result.reference = Some(RecordRef {
                        entity: entity.clone(),
                        record: record.clone(),
                    });
                }
            }
            Type::Boolean => {
                result.checked = matches!(value, Some(DataValue::Boolean { value: true }))
            }
            _ => result.text = value.map(value_text).unwrap_or_default(),
        }
        result
    }
    pub(crate) fn value(&self, typ: &Type) -> Option<DataValue> {
        Some(match typ {
            Type::Text => {
                if self.text.len() > MAX_TEXT_BYTES {
                    return None;
                }
                DataValue::Text {
                    value: self.text.clone(),
                }
            }
            Type::Integer => DataValue::Integer {
                value: self.text.parse().ok()?,
            },
            Type::Date => {
                let parsed = NaiveDate::parse_from_str(&self.text, "%Y-%m-%d").ok()?;
                let days = parsed
                    .signed_duration_since(date(0)?)
                    .num_days()
                    .try_into()
                    .ok()?;
                let value = DataValue::Date { days };
                validate_value(&value, typ, 0).ok()?;
                value
            }
            Type::Boolean => DataValue::Boolean {
                value: self.checked,
            },
            Type::Reference { entity } => {
                let reference = self.reference.as_ref()?;
                if &reference.entity != entity {
                    return None;
                }
                DataValue::Reference {
                    entity: entity.clone(),
                    record: reference.record.clone(),
                }
            }
            Type::Optional { item } => {
                if self.present {
                    self.children.first()?.value(item)?
                } else {
                    DataValue::Null
                }
            }
            Type::List { item } => DataValue::List {
                item_type: *item.clone(),
                items: self
                    .children
                    .iter()
                    .map(|c| c.value(item))
                    .collect::<Option<Vec<_>>>()?,
            },
        })
    }
    pub(crate) fn show(
        &mut self,
        ui: &mut Ui,
        typ: &Type,
        records: &[Record],
        key: &str,
        trace: &mut WidgetTrace,
    ) -> bool {
        let mut changed = false;
        ui.push_id(key, |ui| match typ {
            Type::Optional { item } => {
                changed |= trace
                    .control(
                        &format!("{key}.present"),
                        ui.checkbox(&mut self.present, "Set a value"),
                    )
                    .changed();
                if self.present {
                    changed |= self.children[0].show(ui, item, records, key, trace);
                }
            }
            Type::Boolean => {
                changed |= trace
                    .control(key, ui.checkbox(&mut self.checked, "Yes"))
                    .changed()
            }
            Type::Reference { entity } => {
                trace.label(
                    ui,
                    self.reference
                        .as_ref()
                        .map(|r| record_label(records, r))
                        .unwrap_or_else(|| "Choose a record".into()),
                );
                for record in records
                    .iter()
                    .filter(|r| &r.entity == entity && !r.archived)
                {
                    let reference = RecordRef {
                        entity: record.entity.clone(),
                        record: record.id.clone(),
                    };
                    let selected = self.reference.as_ref() == Some(&reference);
                    if trace
                        .control(
                            &format!("{key}.{}.{}", record.entity, record.id),
                            ui.add(
                                Button::new(record_label(records, &reference))
                                    .selected(selected)
                                    .wrap(),
                            ),
                        )
                        .clicked()
                    {
                        self.reference = Some(reference);
                        changed = true;
                    }
                }
            }
            Type::List { item } => {
                let mut remove = None;
                for (index, child) in self.children.iter_mut().enumerate() {
                    ui.group(|ui| {
                        changed |= child.show(ui, item, records, &format!("{key}.{index}"), trace);
                        if trace.button(ui, &format!("{key}.{index}.remove"), "Remove item", true) {
                            remove = Some(index);
                        }
                    });
                }
                if let Some(index) = remove {
                    self.children.remove(index);
                    changed = true;
                }
                if trace.button(
                    ui,
                    &format!("{key}.add"),
                    "Add item",
                    self.children.len() < MAX_COLLECTION,
                ) {
                    self.children.push(Self::new(item, None));
                    changed = true;
                }
            }
            _ => {
                let hint = match typ {
                    Type::Integer => "Whole number",
                    Type::Date => "YYYY-MM-DD",
                    _ => "",
                };
                changed |= trace
                    .control(
                        key,
                        ui.add(
                            TextEdit::singleline(&mut self.text)
                                .id(ui.make_persistent_id(key))
                                .desired_width(ui.available_width())
                                .hint_text(hint),
                        ),
                    )
                    .changed();
                if self.value(typ).is_none() {
                    trace.label(ui, "Enter a valid value before continuing");
                }
            }
        });
        changed
    }
}
fn record_label(records: &[Record], reference: &RecordRef) -> String {
    let name = records
        .iter()
        .find(|r| r.entity == reference.entity && r.id == reference.record)
        .and_then(|r| {
            r.values
                .values()
                .find(|v| matches!(v, DataValue::Text { .. }))
        })
        .map(value_text);
    name.map(|name| format!("{name} ({})", reference.record))
        .unwrap_or_else(|| reference.record.clone())
}

#[derive(Default)]
pub struct RuntimeOutput {
    pub input: Option<SemanticInput>,
    pub export: Option<Digest>,
    pub pending_edits: bool,
    pub trace: WidgetTrace,
}
enum QueuedInput {
    Control(Id, DataValue, u64),
    Shortcut(SemanticInput),
}
#[derive(Default)]
pub struct ProductRuntimeView {
    view_key: Option<(Digest, Id)>,
    fields: BTreeMap<Id, ValueEditor>,
    controls: BTreeMap<Id, ValueEditor>,
    observed_controls: Values,
    queued_inputs: VecDeque<QueuedInput>,
    dirty_controls: BTreeSet<Id>,
    failed_controls: BTreeSet<Id>,
    pending_control: Option<(Id, u64)>,
    control_revisions: BTreeMap<Id, u64>,
    edit_revision: u64,
    focused_row: Option<RecordRef>,
}
impl ProductRuntimeView {
    /// Local renderer focus is context, never a saved semantic input.
    pub(crate) fn focused_record(&self) -> Option<&RecordRef> {
        self.focused_row.as_ref()
    }
    /// Called only for the exact controller-owned request acknowledgement.
    pub(crate) fn acknowledge(&mut self, successful: bool) {
        let pending = self.pending_control.take();
        if !successful {
            self.queued_inputs.clear();
            self.failed_controls
                .extend(self.dirty_controls.iter().cloned());
        } else if let Some((control, revision)) = pending {
            if self.control_revisions.get(&control) == Some(&revision) {
                self.dirty_controls.remove(&control);
                self.failed_controls.remove(&control);
                self.controls.remove(&control);
                self.observed_controls.remove(&control);
            }
        }
    }
    pub(crate) fn pending_edits(&self) -> bool {
        !self.dirty_controls.is_empty() || !self.queued_inputs.is_empty()
    }
    /// `interactive` allows one request now. Local text editing remains available
    /// while a request is pending; valid controls retain their event order.
    /// `read_only` forbids editing, including in stale copied runs.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        model: &RuntimeView,
        prefix: &str,
        interactive: bool,
        allow_export: bool,
        shortcuts: &[Id],
    ) -> RuntimeOutput {
        let mut output = RuntimeOutput::default();
        let Some(view) = model
            .program
            .views
            .iter()
            .find(|v| v.id == model.observation.view)
        else {
            output.trace.label(
                ui,
                "This view is unavailable. Return to your tool and try again.",
            );
            return output;
        };
        let Ok(program) = model.program.identity() else {
            output
                .trace
                .label(ui, "The tool definition is invalid; editing is unavailable");
            return output;
        };
        let key = (program, view.id.clone());
        if self.view_key.as_ref() != Some(&key) {
            *self = Self {
                view_key: Some(key),
                ..Self::default()
            };
        }
        let interactive = interactive && !model.read_only;
        let trace = &mut output.trace;
        if !model.read_only && self.dirty_controls.is_empty() {
            for shortcut in shortcuts {
                let Some(binding) = view.actions.iter().find(|action| &action.id == shortcut)
                else {
                    continue;
                };
                let row = if binding.placement == ActionPlacement::Row {
                    self.focused_row.clone()
                } else {
                    None
                };
                if binding_enabled(model, binding, row.as_ref()) {
                    if self.queued_inputs.len() == MAX_ITEMS {
                        trace.label(ui, "Too many pending inputs. Wait or cancel before pressing more shortcuts.");
                        break;
                    }
                    self.queued_inputs
                        .push_back(QueuedInput::Shortcut(SemanticInput::Activate {
                            view: view.id.clone(),
                            binding: binding.id.clone(),
                            row,
                        }));
                }
            }
        }
        ui.push_id(prefix, |ui| {
            trace.label(ui, &model.program.label);
            for issue in &model.issues { trace.label(ui, issue); }
            if model.read_only { trace.label(ui, "Read-only: changes are unavailable"); }
            ui.horizontal_wrapped(|ui| for target in &model.program.views {
                if trace.button(ui, &format!("{prefix}.navigate.{}",target.id), &target.label, interactive && self.dirty_controls.is_empty() && self.queued_inputs.is_empty() && target.id != view.id) { output.input = Some(SemanticInput::Navigate { view: target.id.clone() }); }
            });
            match &view.kind {
                ViewKind::List { controls, selection, columns, entity, .. } => {
                    for control in controls {
                        let Some(state) = model.program.state.iter().find(|s| s.id == control.state) else { continue; };
                        let observed = model.observation.controls.get(&control.id);
                        if self.observed_controls.get(&control.id) != observed && !self.dirty_controls.contains(&control.id) {
                            self.controls.insert(control.id.clone(), ValueEditor::new(&state.value_type, observed));
                        }
                        if let Some(value) = observed { self.observed_controls.insert(control.id.clone(), value.clone()); }
                        let editor = self.controls.entry(control.id.clone()).or_insert_with(|| ValueEditor::new(&state.value_type, observed));
                        trace.label(ui, &control.label);
                        ui.add_enabled_ui(!model.read_only && self.queued_inputs.len() < MAX_ITEMS, |ui| {
                            if editor.show(ui, &state.value_type, &model.retained_records, &format!("{prefix}.control.{}",control.id), trace) {
                                self.dirty_controls.insert(control.id.clone());
                                self.failed_controls.remove(&control.id);
                                self.edit_revision = self.edit_revision.wrapping_add(1);
                                self.control_revisions.insert(control.id.clone(), self.edit_revision);
                                if let Some(value) = editor.value(&state.value_type) { self.queued_inputs.push_back(QueuedInput::Control(control.id.clone(), value, self.edit_revision)); }
                            }
                        });
                        if self.failed_controls.contains(&control.id) {
                            trace.label(ui,"This value was not applied. Correct it, retry, or use the last observed value.");
                            if trace.button(ui,&format!("{prefix}.control.{}.retry",control.id),"Try this value again",interactive && editor.value(&state.value_type).is_some()) {
                                self.queued_inputs.push_back(QueuedInput::Control(control.id.clone(),editor.value(&state.value_type).unwrap(),self.control_revisions[&control.id]));
                                self.failed_controls.remove(&control.id);
                            }
                            if trace.button(ui,&format!("{prefix}.control.{}.discard",control.id),"Use the last observed value",interactive) {
                                *editor=ValueEditor::new(&state.value_type,observed);
                                self.failed_controls.remove(&control.id);
                                self.dirty_controls.remove(&control.id);
                            }
                        }
                    }
                    if self.queued_inputs.len() >= MAX_ITEMS { trace.label(ui, "Wait for pending inputs to finish, or cancel the operation, before editing more"); }
                    let actions_ready = interactive && self.dirty_controls.is_empty() && self.queued_inputs.is_empty();
                    if let Some(selection) = selection { trace.label(ui, format!("{} selected (including hidden rows)", model.observation.selected.len()));
                        for row in &model.observation.rows {
                            ui.group(|ui| {
                                let mut selected = model.observation.selected.contains(&row.record);
                                if trace.control(&format!("{prefix}.select.{}.{}",row.record.entity,row.record.record), ui.add_enabled(actions_ready, egui::Checkbox::new(&mut selected, "Select"))).changed() {
                                    let mut records = model.observation.selected.clone();
                                    records.retain(|r| r != &row.record);
                                    if selected { records.push(row.record.clone()); }
                                    output.input = Some(SemanticInput::Control { view: view.id.clone(), control: selection.id.clone(), value: DataValue::List { item_type: Type::reference(entity), items: records.into_iter().map(|r| DataValue::Reference { entity:r.entity, record:r.record }).collect() } });
                                    self.focused_row = Some(row.record.clone());
                                }
                                render_row(ui, trace, prefix, view, row, columns, actions_ready, &mut self.focused_row, &mut output.input);
                            });
                        }
                    } else {
                        for row in &model.observation.rows { ui.group(|ui| render_row(ui, trace, prefix, view, row, columns, actions_ready, &mut self.focused_row, &mut output.input)); }
                    }
                    if model.observation.rows.is_empty() { trace.label(ui, "No matching records"); }
                }
                ViewKind::Detail { columns, .. } => {
                    for row in &model.observation.rows { render_row(ui, trace, prefix, view, row, columns, interactive && self.queued_inputs.is_empty(), &mut self.focused_row, &mut output.input); }
                    if model.observation.rows.is_empty() { trace.label(ui, "No record is available"); }
                }
                ViewKind::Form { action, fields, defaults } => {
                    if let Some(action) = model.program.actions.iter().find(|a| &a.id == action) {
                        for field in fields {
                            let Some(typ) = action.parameters.get(&field.parameter) else { continue; };
                            let editor = self.fields.entry(field.parameter.clone()).or_insert_with(|| ValueEditor::new(typ, model.observation.form_values.get(&field.parameter).or_else(|| defaults.get(&field.parameter))));
                            trace.label(ui, &field.label);
                            ui.add_enabled_ui(!model.read_only, |ui| editor.show(ui, typ, &model.retained_records, &format!("{prefix}.field.{}",field.parameter),trace));
                        }
                        let arguments = action.parameters.iter().map(|(id, typ)| {
                            let value = if let Some(editor) = self.fields.get(id) { editor.value(typ) } else { model.observation.form_values.get(id).or_else(|| defaults.get(id)).cloned() };
                            value.map(|value| (id.clone(), value))
                        }).collect::<Option<Values>>();
                        let valid = arguments.as_ref().is_some_and(|args| validate_input(&SemanticInput::Submit { view: view.id.clone(), arguments: args.clone() }, &model.program).is_ok());
                        if trace.button(ui,&format!("{prefix}.submit"),&action.label,interactive && valid && self.queued_inputs.is_empty()) {
                            output.input = Some(SemanticInput::Submit { view:view.id.clone(),arguments:arguments.unwrap() });
                        }
                    }
                }
            }
            let actions_ready = interactive && self.dirty_controls.is_empty() && self.queued_inputs.is_empty();
            ui.horizontal_wrapped(|ui| for binding in view.actions.iter().filter(|a| a.placement == ActionPlacement::Toolbar) {
                if trace.button(ui,&format!("{prefix}.action.{}",binding.id),&binding.label,actions_ready && model.observation.enabled_actions.contains(&binding.id)) {
                    output.input = Some(SemanticInput::Activate { view:view.id.clone(),binding:binding.id.clone(),row:None });
                }
            });
            for (index, artifact) in model.artifacts.iter().enumerate() {
                ui.separator();
                let label = model.program.outputs.iter().find(|o| o.id == artifact.output).map(|o| o.label.as_str()).unwrap_or(&artifact.output);
                trace.label(ui,format!("{label}: {} rows, {} bytes ({:?})",artifact.rows.len(),artifact.bytes.len(),artifact.format));
                for row in &artifact.rows { for column in &artifact.columns { if let Some(value) = row.get(&column.id) { trace.label(ui,format!("{}: {}",column.label,value_text(value))); } } ui.separator(); }
                if allow_export && trace.button(ui,&format!("{prefix}.output.{index}.save"),"Save output…",interactive && self.queued_inputs.is_empty() && self.dirty_controls.is_empty() && artifact.validate().is_ok()) { output.export=Some(artifact.digest.clone()); }
                if !allow_export { trace.label(ui,"Preview output stays in this comparison"); }
            }
            if !model.history.results.is_empty() {
                egui::CollapsingHeader::new("Preserved completed results").default_open(true).show(ui,|ui| {
                    for result in &model.history.results {
                        let origin=match result.origin {
                            crate::product_protocol::HistoryOrigin::CapturedAtAdoption=>"Captured when this change was adopted; the earlier completion value was not reconstructed",
                            crate::product_protocol::HistoryOrigin::ObservedAtCompletion=>"Recorded by the actual completion action",
                            crate::product_protocol::HistoryOrigin::ObservedAtArchive=>"Recorded by the actual archive action",
                        };
                        trace.label(ui,format!("{} / {} · {}: {}",result.record.entity,result.record.record,result.label,value_text(&result.value)));
                        trace.label(ui,format!("{origin} · {}",date(result.day).map(|d|d.to_string()).unwrap_or_else(||"Invalid date".into())));
                        trace.label(ui,format!("Source {}{}",result.program.as_str(),result.event.as_ref().map(|id|format!(" · Event {id}")).unwrap_or_default()));
                    }
                });
            }
            if !model.history.events.is_empty() {
                ui.collapsing("Recorded business events",|ui|for event in &model.history.events {
                    trace.label(ui,format!("{} · {} · {}",event.id,event.action,date(event.day).map(|d|d.to_string()).unwrap_or_else(||"Invalid date".into())));
                    for change in &event.changes {trace.label(ui,format!("{} / {}{}",change.entity,change.record,if change.archived {" (archived)"}else{""}));for (key,value) in &change.after {if !key.starts_with(crate::product_runtime::PROTECTED_FIELD_PREFIX){trace.label(ui,format!("{key}: {}",value_text(value)));}}}
                });
            }
            if !model.retained_records.is_empty() {
                ui.collapsing("All retained data", |ui| for record in &model.retained_records { trace.label(ui,format!("{} / {}{}",record.entity,record.id,if record.archived {" (archived)"} else {""})); for (field,value) in &record.values { trace.label(ui,format!("{field}: {}",value_text(value))); } });
            }
        });
        if interactive && output.input.is_none() && output.export.is_none() {
            match self.queued_inputs.pop_front() {
                Some(QueuedInput::Control(control, value, revision)) => {
                    self.pending_control = Some((control.clone(), revision));
                    output.input = Some(SemanticInput::Control {
                        view: view.id.clone(),
                        control,
                        value,
                    });
                }
                Some(QueuedInput::Shortcut(input)) => {
                    if let SemanticInput::Activate { binding, row, .. } = &input {
                        if view
                            .actions
                            .iter()
                            .find(|action| &action.id == binding)
                            .is_some_and(|action| binding_enabled(model, action, row.as_ref()))
                        {
                            output.input = Some(input);
                        } else {
                            self.queued_inputs.clear();
                            self.failed_controls
                                .extend(self.dirty_controls.iter().cloned());
                            trace.label(ui, "A queued action is no longer available. Remaining inputs were not applied.");
                        }
                    }
                }
                None => {}
            }
        }
        output.pending_edits = self.pending_edits();
        output
    }
}
fn render_row(
    ui: &mut Ui,
    trace: &mut WidgetTrace,
    prefix: &str,
    view: &ViewDefinition,
    row: &PresentedRow,
    columns: &[Column],
    interactive: bool,
    focused: &mut Option<RecordRef>,
    input: &mut Option<SemanticInput>,
) {
    for column in columns {
        let value = row
            .cells
            .get(&column.id)
            .map(value_text)
            .unwrap_or_else(|| "Not observed".into());
        trace.label(ui, format!("{}: {value}", column.label));
    }
    if trace
        .control(
            &format!("{prefix}.focus.{}.{}", row.record.entity, row.record.record),
            ui.add(
                Button::new("Focus record")
                    .selected(focused.as_ref() == Some(&row.record))
                    .wrap(),
            ),
        )
        .clicked()
    {
        *focused = Some(row.record.clone());
    }
    ui.horizontal_wrapped(|ui| {
        for binding in view
            .actions
            .iter()
            .filter(|a| a.placement == ActionPlacement::Row)
        {
            if trace.button(
                ui,
                &format!(
                    "{prefix}.row.{}.{}.{}",
                    row.record.entity, row.record.record, binding.id
                ),
                &binding.label,
                interactive && row.enabled_actions.contains(&binding.id),
            ) {
                *focused = Some(row.record.clone());
                *input = Some(SemanticInput::Activate {
                    view: view.id.clone(),
                    binding: binding.id.clone(),
                    row: Some(row.record.clone()),
                });
            }
        }
    });
}

fn protocol_key(key: &str) -> Option<Key> {
    Some(match key {
        "enter" => Key::Enter,
        "escape" => Key::Escape,
        "space" => Key::Space,
        "arrow_up" => Key::ArrowUp,
        "arrow_down" => Key::ArrowDown,
        "arrow_left" => Key::ArrowLeft,
        "arrow_right" => Key::ArrowRight,
        "delete" => Key::Delete,
        "backspace" => Key::Backspace,
        "tab" => Key::Tab,
        _ => return Key::from_name(&key.to_uppercase()),
    })
}
pub(crate) fn take_shortcuts(
    ui: &mut Ui,
    view: Option<&ViewDefinition>,
    allowed: bool,
    trace: &mut WidgetTrace,
) -> Vec<Id> {
    if !allowed || text_edit_focused(ui.ctx()) {
        return Vec::new();
    }
    let chords: Vec<_> = view
        .into_iter()
        .flat_map(|view| view.keys.iter())
        .filter_map(|chord| protocol_key(&chord.key).map(|key| (key, chord)))
        .collect();
    let mut accepted = Vec::new();
    let mut ambiguous = false;
    // Resolve each physical press separately and consume it before any button
    // can turn Enter/Space into a click. Fresh repeated presses retain order.
    ui.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let mut matching = chords.iter().filter(|(candidate, chord)| {
                key == candidate && modifiers_match(*modifiers, &chord.modifiers)
            });
            let first = matching.next();
            if !repeat {
                if matching.next().is_some() {
                    ambiguous = true;
                } else if let Some((_, chord)) = first {
                    accepted.push(chord.binding.clone());
                }
            }
            first.is_none() && !(*repeat && matches!(key, Key::Enter | Key::Space))
        })
    });
    if ambiguous {
        trace.label(
            ui,
            "This shortcut has multiple actions on this keyboard. Use the labeled buttons.",
        );
    }
    accepted
}
fn binding_enabled(model: &RuntimeView, binding: &ActionBinding, row: Option<&RecordRef>) -> bool {
    if binding.placement == ActionPlacement::Row {
        row.is_some_and(|reference| {
            model
                .observation
                .rows
                .iter()
                .any(|item| &item.record == reference && item.enabled_actions.contains(&binding.id))
        })
    } else {
        model.observation.enabled_actions.contains(&binding.id)
    }
}

fn modifiers_match(actual: egui::Modifiers, required: &BTreeSet<KeyModifier>) -> bool {
    if actual.alt != required.contains(&KeyModifier::Alt)
        || actual.shift != required.contains(&KeyModifier::Shift)
    {
        return false;
    }
    let control = required.contains(&KeyModifier::Control);
    let command = required.contains(&KeyModifier::Command);
    if actual.mac_cmd {
        return actual.command && command && actual.ctrl == control;
    }
    if actual.ctrl && actual.command {
        // On non-Mac keyboards Command is egui's logical alias for Control.
        // Multiple declared bindings for that physical chord are ambiguous and
        // are rejected by the caller, never resolved by declaration order.
        return control || command;
    }
    actual.ctrl == control && actual.command == command
}

/// `wants_keyboard_input` includes focused buttons, so it cannot distinguish a
/// Tab-focused toolbar from a text editor. TextEdit stores state by widget ID.
pub(crate) fn text_edit_focused(ctx: &egui::Context) -> bool {
    ctx.memory(|memory| memory.focused())
        .is_some_and(|id| TextEdit::load_state(ctx, id).is_some())
}
