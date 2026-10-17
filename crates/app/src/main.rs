//! Craftflow desktop application.

use anyhow::Context as _;
use tracing_subscriber::EnvFilter;

/// The whole application state. Empty for now: stage 0 only opens a window.
#[derive(Default)]
struct Craftflow {}

impl eframe::App for Craftflow {
    // eframe hands us a `Ui` already wrapped in a central panel, so there is
    // nothing to lay out around it yet.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("Craftflow");
    }
}

fn main() -> anyhow::Result<()> {
    // RUST_LOG overrides the default; without it we keep everything at info.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting Craftflow");

    let options = eframe::NativeOptions::default();

    // `run_native` owns the event loop and returns once the window closes.
    // Its error type is not `Send + Sync`, so it cannot flow into anyhow
    // through `?`; we flatten it to a message by hand.
    eframe::run_native(
        "Craftflow",
        options,
        Box::new(|_cc| Ok(Box::<Craftflow>::default())),
    )
    .map_err(|err| anyhow::anyhow!("{err}"))
    .context("failed to run the Craftflow window")
}
