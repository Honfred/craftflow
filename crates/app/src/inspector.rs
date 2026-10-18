//! The inspector for the selected node (DESIGN.md §12).

use egui::{ComboBox, DragValue, Ui};
use graph::{Flow, NodeId, NodeKind};
use model::{Id, ParamSpec, ParamValue, TimeUnit};

use crate::state::AppState;

pub fn show(ui: &mut Ui, state: &mut AppState) {
    ui.heading("Inspector");

    let Some(selected) = state.selection.clone() else {
        ui.weak("select a node on the canvas");
        return;
    };
    let Some(node) = state.editor.schema().node(&selected).cloned() else {
        state.selection = None;
        return;
    };

    ui.label(format!("{}  ({})", node.id, node.kind.type_name()));

    let mut label = node.label.clone().unwrap_or_default();
    if ui
        .add(egui::TextEdit::singleline(&mut label).hint_text("label"))
        .changed()
    {
        let value = (!label.trim().is_empty()).then_some(label);
        state.editor.set_label(&selected, value);
    }
    ui.separator();

    match &node.kind {
        NodeKind::Machine(_) => machine(ui, state, &selected),
        NodeKind::Source(_) => source(ui, state, &selected),
        NodeKind::Output(_) => output(ui, state, &selected),
        NodeKind::Hub(_) => {
            ui.weak("a hub mixes what arrives and passes it on");
        }
        NodeKind::Note(note) => {
            let mut text = note.text.clone();
            if ui.text_edit_multiline(&mut text).changed() {
                state.editor.set_note_text(&selected, text);
            }
        }
    }

    ui.separator();
    if ui.button("Delete node").clicked() {
        state.editor.remove_node(&selected);
        state.selection = None;
    }
}

fn machine(ui: &mut Ui, state: &mut AppState, id: &NodeId) {
    let Some(node) = state
        .editor
        .schema()
        .node(id)
        .and_then(graph::Node::machine)
        .cloned()
    else {
        return;
    };

    // Which recipes this machine can run, and which machines run this recipe.
    let Some(machine_def) = state.registry.machines.get(&node.machine).cloned() else {
        ui.colored_label(
            egui::Color32::from_rgb(220, 90, 70),
            format!("machine `{}` is not in any pack", node.machine),
        );
        return;
    };

    let category = state
        .registry
        .recipes
        .get(&node.recipe)
        .map(|recipe| recipe.category.clone());

    if let Some(category) = &category {
        let options: Vec<Id> = state
            .registry
            .machines_for(category)
            .map(|machine| machine.id.clone())
            .collect();
        let mut chosen = node.machine.clone();
        ComboBox::from_label("Machine")
            .selected_text(chosen.path())
            .show_ui(ui, |ui| {
                for option in &options {
                    ui.selectable_value(&mut chosen, option.clone(), option.path());
                }
            });
        if chosen != node.machine {
            state.editor.set_machine(id, chosen);
            return;
        }
    }

    let recipes: Vec<Id> = state
        .registry
        .recipes
        .iter()
        .filter(|(_, recipe)| machine_def.categories.contains(&recipe.category))
        .map(|(id, _)| id.clone())
        .collect();
    let mut chosen = node.recipe.clone();
    ComboBox::from_label("Recipe")
        .selected_text(chosen.path())
        .show_ui(ui, |ui| {
            for option in &recipes {
                ui.selectable_value(&mut chosen, option.clone(), option.path());
            }
        });
    if chosen != node.recipe {
        state.editor.set_recipe(id, chosen);
        return;
    }

    // Count: absent means the solver may use as many as it likes.
    let mut limited = node.count.is_some();
    if ui.checkbox(&mut limited, "Limit machine count").changed() {
        state.editor.set_count(id, limited.then_some(1.0));
        return;
    }
    if let Some(count) = node.count {
        let mut value = count;
        if ui
            .add(DragValue::new(&mut value).speed(1.0).range(0.0..=1e6))
            .changed()
        {
            state.editor.set_count(id, Some(value));
        }
    }

    if !machine_def.params.is_empty() {
        ui.separator();
        ui.label("Parameters");
        for (name, spec) in &machine_def.params {
            let current = node
                .params
                .get(name)
                .copied()
                .unwrap_or_else(|| spec.default_value());
            if let Some(changed) = param_widget(ui, name, spec, current) {
                state.editor.set_param(id, name, Some(changed));
            }
        }
    }

    if !machine_def.networks.is_empty() {
        ui.separator();
        ui.label("Networks");
        let available: Vec<String> = state.editor.schema().networks.keys().cloned().collect();
        for (role, kind) in &machine_def.networks {
            let current = node.networks.get(role).cloned();
            let mut chosen = current.clone().unwrap_or_default();
            ComboBox::from_label(format!("{role} ({kind})"))
                .selected_text(if chosen.is_empty() { "none" } else { &chosen })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut chosen, String::new(), "none");
                    for name in &available {
                        ui.selectable_value(&mut chosen, name.clone(), name);
                    }
                });
            let next = (!chosen.is_empty()).then_some(chosen);
            if next != current {
                state.editor.set_node_network(id, role, next);
                return;
            }
        }
    }

    // What the recipe actually does, so the numbers on the canvas make sense.
    if let Some(recipe) = state.registry.recipes.get(&node.recipe) {
        ui.separator();
        ui.label(format!("{} ticks per craft", recipe.duration.0));
        for input in &recipe.inputs {
            ui.weak(format!("in   {} x{}", input.resource, input.amount));
        }
        for output in &recipe.outputs {
            let chance = if output.chance < 1.0 {
                format!("  ({:.0}%)", output.chance * 100.0)
            } else {
                String::new()
            };
            ui.weak(format!(
                "out  {} x{}{chance}",
                output.resource, output.amount
            ));
        }
    }
}

fn param_widget(
    ui: &mut Ui,
    name: &str,
    spec: &ParamSpec,
    current: ParamValue,
) -> Option<ParamValue> {
    match (spec, current) {
        (ParamSpec::Int { min, max, .. }, ParamValue::Int(value)) => {
            let mut value = value;
            let mut drag = DragValue::new(&mut value).speed(1.0);
            if let (Some(min), Some(max)) = (min, max) {
                drag = drag.range(*min..=*max);
            }
            ui.add(drag).labelled_by(ui.label(name).id).changed();
            (value
                != match current {
                    ParamValue::Int(v) => v,
                    _ => value,
                })
            .then_some(ParamValue::Int(value))
        }
        (ParamSpec::Float { min, max, .. }, ParamValue::Float(value)) => {
            let mut value = value;
            let mut drag = DragValue::new(&mut value).speed(0.1);
            if let (Some(min), Some(max)) = (min, max) {
                drag = drag.range(*min..=*max);
            }
            let changed = ui.add(drag).changed();
            ui.label(name);
            changed.then_some(ParamValue::Float(value))
        }
        (ParamSpec::Bool { .. }, ParamValue::Bool(value)) => {
            let mut value = value;
            ui.checkbox(&mut value, name)
                .changed()
                .then_some(ParamValue::Bool(value))
        }
        _ => {
            ui.weak(format!("{name}: unexpected value type"));
            None
        }
    }
}

fn source(ui: &mut Ui, state: &mut AppState, id: &NodeId) {
    let Some(NodeKind::Source(node)) = state.editor.schema().node(id).map(|n| n.kind.clone())
    else {
        return;
    };

    resource_picker(ui, state, id, &node.resource);

    let mut limited = node.rate.is_some();
    if ui.checkbox(&mut limited, "Limit the rate").changed() {
        state
            .editor
            .set_source_rate(id, limited.then(|| Flow::new(60.0, state.unit)));
        return;
    }
    if let Some(rate) = node.rate
        && let Some(next) = flow_widget(ui, "per", rate)
    {
        state.editor.set_source_rate(id, Some(next));
    }
}

fn output(ui: &mut Ui, state: &mut AppState, id: &NodeId) {
    let Some(NodeKind::Output(node)) = state.editor.schema().node(id).map(|n| n.kind.clone())
    else {
        return;
    };

    resource_picker(ui, state, id, &node.resource);

    let mut wanted = node.target.is_some();
    if ui.checkbox(&mut wanted, "Set a target").changed() {
        state
            .editor
            .set_output_target(id, wanted.then(|| Flow::new(60.0, state.unit)));
        return;
    }
    if let Some(target) = node.target
        && let Some(next) = flow_widget(ui, "per", target)
    {
        state.editor.set_output_target(id, Some(next));
    }
    ui.weak("a target caps demand; falling short is reported, not an error");
}

/// A resource is chosen by typing its id; the browser is the other way in.
fn resource_picker(ui: &mut Ui, state: &mut AppState, id: &NodeId, current: &Id) {
    let mut text = current.to_string();
    let known = state.registry.resources.contains_key(current);
    let response = ui.add(
        egui::TextEdit::singleline(&mut text)
            .hint_text("namespace:path")
            .text_color_opt((!known).then_some(egui::Color32::from_rgb(220, 90, 70))),
    );
    if response.changed()
        && let Ok(parsed) = text.parse::<Id>()
        && state.registry.resources.contains_key(&parsed)
    {
        state.editor.set_resource(id, parsed);
    }
    if !known {
        ui.colored_label(
            egui::Color32::from_rgb(220, 90, 70),
            "not in any loaded pack",
        );
    }
}

fn flow_widget(ui: &mut Ui, label: &str, flow: Flow) -> Option<Flow> {
    let mut amount = flow.amount;
    let mut unit = flow.unit;
    let mut changed = false;

    ui.horizontal(|ui| {
        changed |= ui
            .add(DragValue::new(&mut amount).speed(1.0).range(0.0..=1e9))
            .changed();
        ui.label(label);
        ComboBox::from_id_salt(label)
            .selected_text(unit.suffix())
            .show_ui(ui, |ui| {
                for option in [
                    TimeUnit::Tick,
                    TimeUnit::Second,
                    TimeUnit::Minute,
                    TimeUnit::Hour,
                ] {
                    changed |= ui
                        .selectable_value(&mut unit, option, option.suffix())
                        .changed();
                }
            });
    });

    changed.then_some(Flow::new(amount, unit))
}
