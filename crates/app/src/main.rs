//! Craftflow desktop application.
//!
//! Rule 2 keeps this a shell: the schema and its history belong to
//! `graph::Editor`, pack data to `packs`, and the answer to `solver`. Nothing
//! here computes anything the CLI cannot.

mod bottom;
mod browser;
mod canvas;
mod files;
mod inspector;
mod state;

use std::path::PathBuf;

use anyhow::Context as _;
use egui::{Key, KeyboardShortcut, Modifiers};
use model::TimeUnit;
use tracing_subscriber::EnvFilter;

use crate::bottom::BottomPanel;
use crate::browser::Browser;
use crate::canvas::Canvas;
use crate::state::{AppState, Tab};

struct Craftflow {
    state: AppState,
    canvas: Canvas,
    browser: Browser,
    bottom: BottomPanel,
}

impl Craftflow {
    fn new(state: AppState) -> Self {
        Self {
            state,
            canvas: Canvas::default(),
            browser: Browser::default(),
            bottom: BottomPanel::default(),
        }
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        let pressed = |ui: &mut egui::Ui, modifiers: Modifiers, key: Key| {
            ui.input_mut(|input| input.consume_shortcut(&KeyboardShortcut::new(modifiers, key)))
        };

        if pressed(ui, Modifiers::COMMAND, Key::N) {
            files::new_schema(&mut self.state);
        }
        if pressed(ui, Modifiers::COMMAND, Key::O) {
            files::open(&mut self.state);
        }
        if pressed(ui, Modifiers::COMMAND, Key::S) {
            files::save(&mut self.state);
        }
        // Shift first: Ctrl+Shift+Z would otherwise be eaten by plain Ctrl+Z.
        if pressed(ui, Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) {
            self.state.editor.redo();
        } else if pressed(ui, Modifiers::COMMAND, Key::Z) {
            self.state.editor.undo();
        }
        if pressed(ui, Modifiers::NONE, Key::Delete)
            && let Some(selected) = self.state.selection.clone()
        {
            self.state.editor.remove_node(&selected);
            self.state.selection = None;
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.state.tab, Tab::Editor, "Editor");
            ui.selectable_value(&mut self.state.tab, Tab::Calculator, "Calculator");
            ui.separator();

            egui::ComboBox::from_id_salt("unit")
                .selected_text(self.state.unit.suffix())
                .width(60.0)
                .show_ui(ui, |ui| {
                    for unit in [
                        TimeUnit::Tick,
                        TimeUnit::Second,
                        TimeUnit::Minute,
                        TimeUnit::Hour,
                    ] {
                        ui.selectable_value(&mut self.state.unit, unit, unit.suffix());
                    }
                });

            ui.separator();
            ui.add_enabled_ui(self.state.editor.can_undo(), |ui| {
                let hint = self.state.editor.undo_label().unwrap_or("nothing");
                if ui.button("Undo").on_hover_text(hint).clicked() {
                    self.state.editor.undo();
                }
            });
            ui.add_enabled_ui(self.state.editor.can_redo(), |ui| {
                let hint = self.state.editor.redo_label().unwrap_or("nothing");
                if ui.button("Redo").on_hover_text(hint).clicked() {
                    self.state.editor.redo();
                }
            });

            ui.separator();
            if ui.button("New").clicked() {
                files::new_schema(&mut self.state);
            }
            if ui.button("Open").clicked() {
                files::open(&mut self.state);
            }
            if ui.button("Save").clicked() {
                files::save(&mut self.state);
            }
            if ui.button("Save as").clicked() {
                files::save_as(&mut self.state);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let errors = self.state.error_count();
                let warnings = self.state.warning_count();
                if errors > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 90, 70),
                        format!("{errors} errors"),
                    );
                }
                if warnings > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(210, 170, 60),
                        format!("{warnings} warnings"),
                    );
                }
                if self.state.solve_error.is_some() {
                    ui.colored_label(egui::Color32::from_rgb(220, 90, 70), "not solved");
                } else {
                    ui.label(format!("solved in {:.1} ms", self.state.solve_ms));
                }
            });
        });

        if let Some(status) = &self.state.status {
            ui.horizontal(|ui| ui.weak(status));
        }
    }
}

impl eframe::App for Craftflow {
    // eframe 0.36 hands the application a `Ui` that is already inside a
    // central panel, so the panels here are the `_inside` variants.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.state.refresh();
        self.shortcuts(ui);

        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(self.state.title()));

        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));

        egui::Panel::left("browser")
            .resizable(true)
            .default_size(240.0)
            .show(ui, |ui| self.browser.show(ui, &mut self.state));

        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| inspector::show(ui, &mut self.state));
            });

        egui::Panel::bottom("balance")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| self.bottom.show(ui, &mut self.state));

        egui::CentralPanel::default().show(ui, |ui| match self.state.tab {
            Tab::Editor => self.canvas.show(ui, &mut self.state),
            Tab::Calculator => {
                ui.centered_and_justified(|ui| {
                    ui.weak("the calculator arrives in a later stage");
                });
            }
        });
    }
}

/// Finds the built-in packs the way the CLI does.
fn find_packs() -> Vec<PathBuf> {
    let mut found = Vec::new();
    let plugins = std::path::Path::new("plugins");
    if let Ok(entries) = std::fs::read_dir(plugins) {
        for entry in entries.flatten() {
            let packs = entry.path().join("packs");
            if let Ok(inner) = std::fs::read_dir(&packs) {
                found.extend(inner.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
            }
        }
    }
    found.sort();
    found
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Craftflow");

    let roots = find_packs();
    let loaded = packs::load(&roots).context("loading packs")?;
    for issue in &loaded.issues {
        tracing::warn!("{issue}");
    }
    tracing::info!(
        packs = loaded.registry.packs.len(),
        recipes = loaded.registry.recipes.len(),
        "packs loaded"
    );

    let mut state = AppState::new(loaded.registry, roots);
    // A path on the command line opens straight away, which is how you get
    // from `cli solve` to the same schema on screen.
    if let Some(path) = std::env::args_os().nth(1) {
        files::open_path(&mut state, PathBuf::from(path));
    }

    let options = eframe::NativeOptions::default();

    // `run_native` owns the event loop and returns once the window closes.
    // Its error type is not `Send + Sync`, so it cannot flow into anyhow
    // through `?`; we flatten it to a message by hand.
    eframe::run_native(
        "Craftflow",
        options,
        Box::new(move |_cc| Ok(Box::new(Craftflow::new(state)))),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))
    .context("failed to run the Craftflow window")
}
