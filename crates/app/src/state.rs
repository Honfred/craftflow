//! What the window is looking at.
//!
//! Rule 2 keeps this thin: the schema and its history live in
//! `graph::Editor`, the packs in `packs::Registry`, and the answer comes from
//! `solver`. Nothing here decides anything the CLI could not.

use std::path::PathBuf;

use graph::{Editor, Schema};
use model::{Catalog, Issue, TimeUnit};
use solver::Report;

/// Which half of the application is in front (DESIGN.md §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Editor,
    Calculator,
}

pub struct AppState {
    pub editor: Editor,
    pub registry: packs::Registry,
    /// Where the packs came from, for reloading them later.
    #[allow(dead_code, reason = "used once pack reloading lands")]
    pub pack_roots: Vec<PathBuf>,

    /// Where the schema came from, for plain Save.
    pub path: Option<PathBuf>,
    pub tab: Tab,
    pub unit: TimeUnit,
    pub selection: Option<graph::NodeId>,

    /// The last answer, absent until the first solve or after a failure.
    pub report: Option<Report>,
    pub solve_error: Option<String>,
    pub schema_issues: Vec<Issue>,
    pub solve_ms: f64,

    /// The content revision the current answer was computed from.
    solved_for: Option<u64>,
    pub status: Option<String>,
}

impl AppState {
    pub fn new(registry: packs::Registry, pack_roots: Vec<PathBuf>) -> Self {
        Self {
            editor: Editor::default(),
            registry,
            pack_roots,
            path: None,
            tab: Tab::Editor,
            unit: TimeUnit::Minute,
            selection: None,
            report: None,
            solve_error: None,
            schema_issues: Vec::new(),
            solve_ms: 0.0,
            solved_for: None,
            status: None,
        }
    }

    pub fn catalog(&self) -> &dyn Catalog {
        &self.registry
    }

    /// Re-solves when the schema has changed in a way that could change the
    /// answer. Moving a node around the canvas does not count, which is why
    /// this watches `content_revision` rather than `revision`.
    pub fn refresh(&mut self) {
        let revision = self.editor.content_revision();
        if self.solved_for == Some(revision) {
            return;
        }
        self.solved_for = Some(revision);
        self.solve_now();
    }

    /// Solves regardless of whether anything changed.
    pub fn solve_now(&mut self) {
        let schema = self.editor.schema().clone();
        self.schema_issues = graph::validate(&schema, &self.registry);

        let started = std::time::Instant::now();
        match solver::solve(&schema, &self.registry) {
            Ok(report) => {
                self.report = Some(report);
                self.solve_error = None;
            }
            Err(error) => {
                self.report = None;
                self.solve_error = Some(error.to_string());
            }
        }
        self.solve_ms = started.elapsed().as_secs_f64() * 1000.0;
    }

    /// Everything wrong right now, schema and solve together.
    pub fn issues(&self) -> Vec<&Issue> {
        self.schema_issues
            .iter()
            .chain(self.report.iter().flat_map(|report| &report.issues))
            .collect()
    }

    pub fn error_count(&self) -> usize {
        self.issues()
            .iter()
            .filter(|issue| issue.is_error())
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues()
            .iter()
            .filter(|issue| !issue.is_error())
            .count()
    }

    /// Title bar text: the file, with a marker when it has unsaved changes.
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || "untitled".to_owned(),
                |name| name.to_string_lossy().into(),
            );
        let dirty = if self.editor.has_unsaved_changes() {
            " *"
        } else {
            ""
        };
        format!("Craftflow — {name}{dirty}")
    }

    pub fn load_schema(&mut self, schema: Schema, path: Option<PathBuf>) {
        self.editor.replace(schema);
        self.path = path;
        self.selection = None;
        self.solved_for = None;
    }
}
