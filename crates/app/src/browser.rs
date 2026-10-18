//! The item and recipe browser (DESIGN.md §12).
//!
//! Search by name or id, then drop something onto the schema. Clicking adds
//! the node; dragging comes with the icon atlas in a later stage.

use egui::{ScrollArea, Ui};
use graph::{MachineNode, Node, NodeKind, OutputNode, SourceNode};
use model::Id;

use crate::state::AppState;

#[derive(Default)]
pub struct Browser {
    query: String,
    show: Kind,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum Kind {
    #[default]
    Recipes,
    Resources,
}

/// How many matches to list before asking the user to narrow the search.
const LIMIT: usize = 200;

impl Browser {
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.heading("Browser");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.show, Kind::Recipes, "Recipes");
            ui.selectable_value(&mut self.show, Kind::Resources, "Items");
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("search")
                .desired_width(f32::INFINITY),
        );
        ui.separator();

        let query = self.query.to_lowercase();
        ScrollArea::vertical().show(ui, |ui| match self.show {
            Kind::Recipes => self.recipes(ui, state, &query),
            Kind::Resources => self.resources(ui, state, &query),
        });
    }

    fn recipes(&self, ui: &mut Ui, state: &mut AppState, query: &str) {
        let matches: Vec<(Id, Id)> = state
            .registry
            .recipes
            .iter()
            .filter(|(id, _)| matches_query(id, query))
            .filter_map(|(id, recipe)| {
                // Only offer a recipe some machine can actually run.
                state
                    .registry
                    .machines_for(&recipe.category)
                    .next()
                    .map(|machine| (id.clone(), machine.id.clone()))
            })
            .take(LIMIT)
            .collect();

        if matches.is_empty() {
            ui.weak("nothing matches");
        }

        for (recipe, machine) in matches {
            if ui
                .button(recipe.path())
                .on_hover_text(recipe.to_string())
                .clicked()
            {
                let id = state.editor.fresh_node_id("machine");
                let node = Node::new(
                    id,
                    NodeKind::Machine(MachineNode {
                        machine,
                        recipe,
                        params: Default::default(),
                        count: Some(1.0),
                        networks: Default::default(),
                    }),
                );
                let id = state.editor.add_node(node);
                state.selection = Some(id);
            }
        }
    }

    fn resources(&self, ui: &mut Ui, state: &mut AppState, query: &str) {
        let matches: Vec<Id> = state
            .registry
            .resources
            .keys()
            .filter(|id| matches_query(id, query))
            .take(LIMIT)
            .cloned()
            .collect();

        if matches.is_empty() {
            ui.weak("nothing matches");
        }

        for resource in matches {
            ui.horizontal(|ui| {
                ui.label(resource.path());
                if ui.small_button("source").clicked() {
                    let id = state.editor.fresh_node_id("source");
                    let node = Node::new(
                        id,
                        NodeKind::Source(SourceNode {
                            resource: resource.clone(),
                            rate: None,
                        }),
                    );
                    let id = state.editor.add_node(node);
                    state.selection = Some(id);
                }
                if ui.small_button("output").clicked() {
                    let id = state.editor.fresh_node_id("output");
                    let node = Node::new(
                        id,
                        NodeKind::Output(OutputNode {
                            resource: resource.clone(),
                            target: None,
                        }),
                    );
                    let id = state.editor.add_node(node);
                    state.selection = Some(id);
                }
            });
        }
    }
}

fn matches_query(id: &Id, query: &str) -> bool {
    query.is_empty() || id.to_string().to_lowercase().contains(query)
}
