//! The bottom panel: balance, networks, patterns and problems (DESIGN.md §12).

use egui::{Color32, ScrollArea, Ui};

use crate::canvas::format_rate;
use crate::state::AppState;

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum BottomTab {
    #[default]
    Balance,
    Networks,
    Patterns,
    Problems,
}

#[derive(Default)]
pub struct BottomPanel {
    tab: BottomTab,
}

impl BottomPanel {
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        let errors = state.error_count();
        let warnings = state.warning_count();

        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, BottomTab::Balance, "Balance");
            ui.selectable_value(&mut self.tab, BottomTab::Networks, "Networks");
            ui.selectable_value(&mut self.tab, BottomTab::Patterns, "Patterns");
            let label = match (errors, warnings) {
                (0, 0) => "Problems".to_owned(),
                (0, w) => format!("Problems ({w})"),
                (e, 0) => format!("Problems ({e})"),
                (e, w) => format!("Problems ({e}+{w})"),
            };
            ui.selectable_value(&mut self.tab, BottomTab::Problems, label);
        });
        ui.separator();

        ScrollArea::vertical().show(ui, |ui| match self.tab {
            BottomTab::Balance => balance(ui, state),
            BottomTab::Networks => networks(ui, state),
            BottomTab::Patterns => patterns(ui, state),
            BottomTab::Problems => problems(ui, state),
        });
    }
}

fn balance(ui: &mut Ui, state: &AppState) {
    let Some(report) = &state.report else {
        ui.weak("not solved");
        return;
    };
    let unit = state.unit;

    egui::Grid::new("balance")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            for output in &report.outputs {
                ui.label("output");
                ui.label(output.resource.to_string());
                match output.shortfall() {
                    Some(short) if short.as_per_tick() > 1e-9 => ui.colored_label(
                        Color32::from_rgb(220, 90, 70),
                        format!(
                            "{}  (short {})",
                            format_rate(output.achieved, unit),
                            format_rate(short, unit)
                        ),
                    ),
                    _ => ui.label(format_rate(output.achieved, unit)),
                };
                ui.end_row();
            }
            for surplus in &report.surplus {
                ui.label("surplus");
                ui.label(format!("{} at {}", surplus.resource, surplus.node));
                ui.colored_label(
                    Color32::from_rgb(210, 170, 60),
                    format_rate(surplus.rate, unit),
                );
                ui.end_row();
            }
            for (resource, amount) in &report.catalysts {
                ui.label("catalyst");
                ui.label(resource.to_string());
                ui.label(format!("{amount:.2} needed"));
                ui.end_row();
            }
        });

    if report.outputs.is_empty() && report.surplus.is_empty() {
        ui.weak("nothing to report");
    }
}

fn networks(ui: &mut Ui, state: &AppState) {
    let Some(report) = &state.report else {
        ui.weak("not solved");
        return;
    };
    if report.networks.is_empty() {
        ui.weak("no networks in this schema");
        return;
    }
    let unit = state.unit;

    egui::Grid::new("networks")
        .num_columns(4)
        .striped(true)
        .show(ui, |ui| {
            for network in &report.networks {
                ui.label(&network.name);
                ui.label(network.kind.to_string());
                ui.label(format!(
                    "+{}  -{}",
                    format_rate(network.produced, unit),
                    format_rate(network.consumed, unit)
                ));
                if network.is_short() {
                    ui.colored_label(
                        Color32::from_rgb(220, 90, 70),
                        format!(
                            "short {}",
                            format_rate(model::Rate::per_tick(-network.net().as_per_tick()), unit)
                        ),
                    );
                } else {
                    ui.colored_label(
                        Color32::from_rgb(110, 180, 110),
                        format!("spare {}", format_rate(network.net(), unit)),
                    );
                }
                ui.end_row();
            }
        });
}

fn patterns(ui: &mut Ui, state: &AppState) {
    let patterns = &state.editor.schema().patterns;
    if patterns.is_empty() {
        ui.weak("no patterns; AE2 arrives in a later stage");
        return;
    }
    for pattern in patterns {
        ui.label(pattern.id.to_string());
    }
}

fn problems(ui: &mut Ui, state: &AppState) {
    if let Some(error) = &state.solve_error {
        ui.colored_label(Color32::from_rgb(220, 90, 70), error);
        ui.separator();
    }

    let issues = state.issues();
    if issues.is_empty() && state.solve_error.is_none() {
        ui.weak("none");
        return;
    }

    for issue in issues {
        let colour = if issue.is_error() {
            Color32::from_rgb(220, 90, 70)
        } else {
            Color32::from_rgb(210, 170, 60)
        };
        ui.colored_label(colour, format!("{}: {}", issue.where_, issue.message));
    }
}
