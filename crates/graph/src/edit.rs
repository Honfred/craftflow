//! Editing a schema, with undo and redo.
//!
//! This lives here rather than in `app` because rule 2 keeps logic in the
//! library crates where it can be tested: removing a node has to take its
//! links with it, dragging a node must not fill the history with one entry
//! per frame, and none of that is the GUI's business.
//!
//! History is kept as whole-schema snapshots. Schemas are a few hundred nodes
//! at most, and a snapshot cannot drift out of step with the document the way
//! a hand-written inverse operation can.

use model::{Id, ParamValue};

use crate::node::{Flow, Link, Network, Node, NodeId, NodeKind};
use crate::schema::Schema;

/// How many steps back the history goes.
pub const HISTORY_LIMIT: usize = 100;

/// Identifies an edit that should merge with the one before it.
///
/// Dragging a node emits an edit per frame; they all carry the same key, so
/// only the first takes a snapshot and one undo returns to where the drag
/// started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditKey {
    MoveNode(NodeId),
    Label(NodeId),
    Count(NodeId),
    Param(NodeId, String),
    SourceRate(NodeId),
    OutputTarget(NodeId),
    NoteText(NodeId),
    LinkCapacity(usize),
}

#[derive(Debug, Clone)]
struct Entry {
    schema: Schema,
    label: &'static str,
}

/// A schema being edited.
#[derive(Debug, Clone)]
pub struct Editor {
    schema: Schema,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    /// The key of the last edit, for merging the next one into it.
    coalesce_with: Option<EditKey>,
    saved: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new(Schema::default())
    }
}

impl Editor {
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            undo: Vec::new(),
            redo: Vec::new(),
            coalesce_with: None,
            saved: true,
        }
    }

    #[must_use]
    pub const fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Replaces the document wholesale, as opening a file does, and forgets
    /// the history: undoing into a file you no longer have open is not useful.
    pub fn replace(&mut self, schema: Schema) {
        self.schema = schema;
        self.undo.clear();
        self.redo.clear();
        self.coalesce_with = None;
        self.saved = true;
    }

    #[must_use]
    pub const fn has_unsaved_changes(&self) -> bool {
        !self.saved
    }

    pub const fn mark_saved(&mut self) {
        self.saved = true;
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What undoing would take back, for the menu item's label.
    #[must_use]
    pub fn undo_label(&self) -> Option<&'static str> {
        self.undo.last().map(|entry| entry.label)
    }

    #[must_use]
    pub fn redo_label(&self) -> Option<&'static str> {
        self.redo.last().map(|entry| entry.label)
    }

    pub fn undo(&mut self) -> Option<&'static str> {
        let entry = self.undo.pop()?;
        let label = entry.label;
        let current = std::mem::replace(&mut self.schema, entry.schema);
        self.redo.push(Entry {
            schema: current,
            label,
        });
        // A fresh edit after an undo must never merge into the entry it
        // uncovered.
        self.coalesce_with = None;
        self.saved = false;
        Some(label)
    }

    pub fn redo(&mut self) -> Option<&'static str> {
        let entry = self.redo.pop()?;
        let label = entry.label;
        let current = std::mem::replace(&mut self.schema, entry.schema);
        self.undo.push(Entry {
            schema: current,
            label,
        });
        self.coalesce_with = None;
        self.saved = false;
        Some(label)
    }

    /// Runs `change` against the schema, recording it in the history.
    fn apply<T>(
        &mut self,
        label: &'static str,
        key: Option<EditKey>,
        change: impl FnOnce(&mut Schema) -> T,
    ) -> T {
        let merges = key.is_some() && key == self.coalesce_with;

        if !merges {
            self.undo.push(Entry {
                schema: self.schema.clone(),
                label,
            });
            if self.undo.len() > HISTORY_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }

        self.coalesce_with = key;
        self.saved = false;
        change(&mut self.schema)
    }

    // -- nodes ------------------------------------------------------------

    /// An id no node in the schema is using, of the form `prefix-1`.
    #[must_use]
    pub fn fresh_node_id(&self, prefix: &str) -> NodeId {
        for n in 1.. {
            let candidate = NodeId::new(format!("{prefix}-{n}"));
            if self.schema.node(&candidate).is_none() {
                return candidate;
            }
        }
        // `1..` only ends by overflowing, long after a free id is found.
        unreachable!("ran out of node ids")
    }

    pub fn add_node(&mut self, node: Node) -> NodeId {
        let id = node.id.clone();
        self.apply("add node", None, move |schema| schema.nodes.push(node));
        id
    }

    /// Removes a node and every link that touched it: a link to nothing is
    /// not a drawing the solver can read.
    pub fn remove_node(&mut self, id: &NodeId) {
        let id = id.clone();
        self.apply("remove node", None, move |schema| {
            schema.nodes.retain(|node| node.id != id);
            schema.links.retain(|link| link.from != id && link.to != id);
        });
    }

    pub fn move_node(&mut self, id: &NodeId, position: [f32; 2]) {
        let key = EditKey::MoveNode(id.clone());
        let id = id.clone();
        self.apply("move node", Some(key), move |schema| {
            if let Some(node) = schema.nodes.iter_mut().find(|node| node.id == id) {
                node.position = Some(position);
            }
        });
    }

    pub fn set_label(&mut self, id: &NodeId, label: Option<String>) {
        let key = EditKey::Label(id.clone());
        let id = id.clone();
        self.apply("rename node", Some(key), move |schema| {
            if let Some(node) = schema.nodes.iter_mut().find(|node| node.id == id) {
                node.label = label;
            }
        });
    }

    // -- links ------------------------------------------------------------

    /// Adds a link unless it would duplicate one or join a node to itself.
    pub fn add_link(&mut self, link: Link) -> bool {
        let exists = self.schema.links.iter().any(|existing| {
            existing.from == link.from
                && existing.to == link.to
                && existing.resource == link.resource
        });
        if exists || link.from == link.to {
            return false;
        }
        self.apply("connect", None, move |schema| schema.links.push(link));
        true
    }

    pub fn remove_link(&mut self, index: usize) {
        if index >= self.schema.links.len() {
            return;
        }
        self.apply("disconnect", None, move |schema| {
            schema.links.remove(index);
        });
    }

    pub fn set_link_capacity(&mut self, index: usize, capacity: Option<Flow>) {
        if index >= self.schema.links.len() {
            return;
        }
        let key = EditKey::LinkCapacity(index);
        self.apply("set link capacity", Some(key), move |schema| {
            if let Some(link) = schema.links.get_mut(index) {
                link.capacity = capacity;
            }
        });
    }

    // -- machine nodes ----------------------------------------------------

    pub fn set_recipe(&mut self, id: &NodeId, recipe: Id) {
        let id = id.clone();
        self.apply("change recipe", None, move |schema| {
            if let Some(NodeKind::Machine(machine)) = node_kind_mut(schema, &id) {
                machine.recipe = recipe;
            }
        });
    }

    pub fn set_machine(&mut self, id: &NodeId, machine_id: Id) {
        let id = id.clone();
        self.apply("change machine", None, move |schema| {
            if let Some(NodeKind::Machine(machine)) = node_kind_mut(schema, &id) {
                machine.machine = machine_id;
            }
        });
    }

    pub fn set_count(&mut self, id: &NodeId, count: Option<f64>) {
        let key = EditKey::Count(id.clone());
        let id = id.clone();
        self.apply("set machine count", Some(key), move |schema| {
            if let Some(NodeKind::Machine(machine)) = node_kind_mut(schema, &id) {
                machine.count = count;
            }
        });
    }

    /// Sets a parameter, or clears it back to the machine's default.
    pub fn set_param(&mut self, id: &NodeId, name: &str, value: Option<ParamValue>) {
        let key = EditKey::Param(id.clone(), name.to_owned());
        let id = id.clone();
        let name = name.to_owned();
        self.apply("set parameter", Some(key), move |schema| {
            if let Some(NodeKind::Machine(machine)) = node_kind_mut(schema, &id) {
                match value {
                    Some(value) => {
                        machine.params.insert(name, value);
                    }
                    None => {
                        machine.params.remove(&name);
                    }
                }
            }
        });
    }

    pub fn set_node_network(&mut self, id: &NodeId, role: &str, network: Option<String>) {
        let id = id.clone();
        let role = role.to_owned();
        self.apply("set network", None, move |schema| {
            if let Some(NodeKind::Machine(machine)) = node_kind_mut(schema, &id) {
                match network {
                    Some(name) => {
                        machine.networks.insert(role, name);
                    }
                    None => {
                        machine.networks.remove(&role);
                    }
                }
            }
        });
    }

    // -- source, output, note ---------------------------------------------

    pub fn set_source_rate(&mut self, id: &NodeId, rate: Option<Flow>) {
        let key = EditKey::SourceRate(id.clone());
        let id = id.clone();
        self.apply("set source rate", Some(key), move |schema| {
            if let Some(NodeKind::Source(source)) = node_kind_mut(schema, &id) {
                source.rate = rate;
            }
        });
    }

    pub fn set_output_target(&mut self, id: &NodeId, target: Option<Flow>) {
        let key = EditKey::OutputTarget(id.clone());
        let id = id.clone();
        self.apply("set output target", Some(key), move |schema| {
            if let Some(NodeKind::Output(output)) = node_kind_mut(schema, &id) {
                output.target = target;
            }
        });
    }

    pub fn set_resource(&mut self, id: &NodeId, resource: Id) {
        let id = id.clone();
        self.apply("change resource", None, move |schema| {
            match node_kind_mut(schema, &id) {
                Some(NodeKind::Source(source)) => source.resource = resource,
                Some(NodeKind::Output(output)) => output.resource = resource,
                _ => {}
            }
        });
    }

    pub fn set_note_text(&mut self, id: &NodeId, text: String) {
        let key = EditKey::NoteText(id.clone());
        let id = id.clone();
        self.apply("edit note", Some(key), move |schema| {
            if let Some(NodeKind::Note(note)) = node_kind_mut(schema, &id) {
                note.text = text;
            }
        });
    }

    // -- networks ----------------------------------------------------------

    pub fn add_network(&mut self, name: String, network: Network) {
        self.apply("add network", None, move |schema| {
            schema.networks.insert(name, network);
        });
    }

    /// Removes a network and detaches every machine that was on it.
    pub fn remove_network(&mut self, name: &str) {
        let name = name.to_owned();
        self.apply("remove network", None, move |schema| {
            schema.networks.remove(&name);
            for node in &mut schema.nodes {
                if let NodeKind::Machine(machine) = &mut node.kind {
                    machine.networks.retain(|_, value| value != &name);
                }
            }
        });
    }
}

fn node_kind_mut<'a>(schema: &'a mut Schema, id: &NodeId) -> Option<&'a mut NodeKind> {
    schema
        .nodes
        .iter_mut()
        .find(|node| &node.id == id)
        .map(|node| &mut node.kind)
}
