//! The node canvas, on egui-snarl.
//!
//! The schema stays the source of truth. Snarl keeps its own node store, so
//! it is rebuilt whenever the schema's content changes; a drag only moves
//! things, which is why `Editor` separates `content_revision` from
//! `revision` — rebuilding mid-drag would fight the mouse.
//!
//! Ports are implicit in the schema (DESIGN.md §6.2) but snarl wants numbered
//! pins, so each node is given an ordered list of [`PinSlot`]s and a link is
//! matched to the slot that accepts its resource.

use std::collections::BTreeMap;

use egui::{Color32, Pos2, Ui, pos2};
use egui_snarl::ui::{PinInfo, SnarlStyle, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId as SnarlId, OutPin, OutPinId, Snarl};
use graph::{HubNode, Link, Node, NodeId, NodeKind, NoteNode, Schema};
use model::{Catalog, Id, ResourceRef, TimeUnit};
use solver::Report;

use crate::state::AppState;

/// What one pin will accept.
#[derive(Debug, Clone, PartialEq)]
pub enum PinSlot {
    /// A resource, or a tag standing for several of them.
    Fixed(ResourceRef),
    /// A hub's spare pin: it takes whatever the other end is carrying.
    Any,
}

impl PinSlot {
    fn accepts(&self, catalog: &dyn Catalog, resource: &Id) -> bool {
        match self {
            Self::Any => true,
            Self::Fixed(ResourceRef::Resource(id)) => id == resource,
            Self::Fixed(ResourceRef::Tag(tag)) => catalog
                .tag(tag)
                .is_some_and(|tag| tag.values.contains(resource)),
        }
    }

    /// The one resource this slot stands for, when there is exactly one.
    fn concrete(&self) -> Option<&Id> {
        match self {
            Self::Fixed(ResourceRef::Resource(id)) => Some(id),
            _ => None,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Any => "+".to_owned(),
            Self::Fixed(reference) => short_name(reference),
        }
    }
}

fn short_name(reference: &ResourceRef) -> String {
    let id = reference.id();
    let prefix = if reference.is_tag() { "#" } else { "" };
    format!(
        "{prefix}{}",
        id.path().rsplit('/').next().unwrap_or(id.path())
    )
}

/// The pins on a node's input side, in a fixed order.
pub fn input_slots(schema: &Schema, catalog: &dyn Catalog, node: &Node) -> Vec<PinSlot> {
    match &node.kind {
        NodeKind::Machine(machine) => catalog
            .recipe(&machine.recipe)
            .map(|recipe| {
                recipe
                    .inputs
                    .iter()
                    .map(|input| PinSlot::Fixed(input.resource.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        NodeKind::Output(output) => {
            vec![PinSlot::Fixed(ResourceRef::Resource(
                output.resource.clone(),
            ))]
        }
        // A hub takes anything, so its pins are whatever is already wired in,
        // plus one spare to drag a new wire onto.
        NodeKind::Hub(_) => hub_slots(schema, &node.id, true),
        NodeKind::Source(_) | NodeKind::Note(_) => Vec::new(),
    }
}

pub fn output_slots(schema: &Schema, catalog: &dyn Catalog, node: &Node) -> Vec<PinSlot> {
    match &node.kind {
        NodeKind::Machine(machine) => catalog
            .recipe(&machine.recipe)
            .map(|recipe| {
                recipe
                    .outputs
                    .iter()
                    .map(|output| PinSlot::Fixed(output.resource.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        NodeKind::Source(source) => {
            vec![PinSlot::Fixed(ResourceRef::Resource(
                source.resource.clone(),
            ))]
        }
        NodeKind::Hub(_) => hub_slots(schema, &node.id, false),
        NodeKind::Output(_) | NodeKind::Note(_) => Vec::new(),
    }
}

fn hub_slots(schema: &Schema, id: &NodeId, incoming: bool) -> Vec<PinSlot> {
    let mut seen: Vec<Id> = Vec::new();
    for link in &schema.links {
        let touches = if incoming { &link.to } else { &link.from };
        if touches == id && !seen.contains(&link.resource) {
            seen.push(link.resource.clone());
        }
    }
    let mut slots: Vec<PinSlot> = seen
        .into_iter()
        .map(|id| PinSlot::Fixed(ResourceRef::Resource(id)))
        .collect();
    slots.push(PinSlot::Any);
    slots
}

/// Index of the slot that carries `resource`, preferring an exact slot over
/// a hub's spare one.
fn slot_index(slots: &[PinSlot], catalog: &dyn Catalog, resource: &Id) -> Option<usize> {
    slots
        .iter()
        .position(|slot| slot.concrete() == Some(resource))
        .or_else(|| {
            slots
                .iter()
                .position(|slot| !matches!(slot, PinSlot::Any) && slot.accepts(catalog, resource))
        })
}

/// The canvas's own state, rebuilt from the schema when its content changes.
pub struct Canvas {
    snarl: Snarl<NodeId>,
    style: SnarlStyle,
    built_for: Option<u64>,
    /// Positions the canvas invented for nodes a file did not place.
    ///
    /// Without this, opening a schema that carries no layout would
    /// immediately mark it as changed: the write-back below would see the
    /// made-up positions and record them as edits.
    laid_out: BTreeMap<NodeId, [f32; 2]>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            snarl: Snarl::new(),
            style: SnarlStyle::new(),
            built_for: None,
            laid_out: BTreeMap::new(),
        }
    }
}

impl Canvas {
    pub fn show(&mut self, ui: &mut Ui, state: &mut AppState) {
        self.sync(state);

        let mut viewer = Viewer {
            state,
            requests: Vec::new(),
        };
        self.snarl.show(
            &mut viewer,
            &self.style,
            egui::Id::new("craftflow-canvas"),
            ui,
        );
        let requests = std::mem::take(&mut viewer.requests);

        // Positions are read back after the frame, so dragging feels direct
        // and the schema still ends up holding the layout.
        let moves: Vec<(NodeId, [f32; 2])> = self
            .snarl
            .nodes_pos_ids()
            .map(|(_, pos, id)| (id.clone(), [pos.x, pos.y]))
            .collect();
        for (id, pos) in moves {
            let current = state
                .editor
                .schema()
                .node(&id)
                .and_then(|node| node.position);
            if current == Some(pos) {
                continue;
            }
            // A position the canvas invented is not a change the user made.
            if current.is_none() && self.laid_out.get(&id) == Some(&pos) {
                continue;
            }
            state.editor.move_node(&id, pos);
        }

        for request in requests {
            request.apply(state);
            // The schema changed under us, so the snarl has to be rebuilt.
            self.built_for = None;
        }
    }

    fn sync(&mut self, state: &AppState) {
        let revision = state.editor.content_revision();
        if self.built_for == Some(revision) {
            return;
        }
        self.built_for = Some(revision);
        self.rebuild(state);
    }

    fn rebuild(&mut self, state: &AppState) {
        let schema = state.editor.schema();
        let catalog = state.catalog();

        self.snarl = Snarl::new();
        self.laid_out.clear();
        let mut ids: BTreeMap<NodeId, SnarlId> = BTreeMap::new();

        for (index, node) in schema.nodes.iter().enumerate() {
            let position = match node.position {
                Some(p) => pos2(p[0], p[1]),
                None => {
                    // Nodes from a file without a layout get a readable
                    // default rather than all landing on top of one another.
                    let placed = [
                        40.0 + (index % 4) as f32 * 260.0,
                        40.0 + (index / 4) as f32 * 200.0,
                    ];
                    self.laid_out.insert(node.id.clone(), placed);
                    pos2(placed[0], placed[1])
                }
            };
            ids.insert(
                node.id.clone(),
                self.snarl.insert_node(position, node.id.clone()),
            );
        }

        for link in &schema.links {
            let (Some(&from), Some(&to)) = (ids.get(&link.from), ids.get(&link.to)) else {
                continue;
            };
            let (Some(from_node), Some(to_node)) = (schema.node(&link.from), schema.node(&link.to))
            else {
                continue;
            };
            let outputs = output_slots(schema, catalog, from_node);
            let inputs = input_slots(schema, catalog, to_node);
            let (Some(output), Some(input)) = (
                slot_index(&outputs, catalog, &link.resource),
                slot_index(&inputs, catalog, &link.resource),
            ) else {
                // A link the pins cannot express: validate() reports it, and
                // the canvas simply does not draw it.
                continue;
            };
            self.snarl
                .connect(OutPinId { node: from, output }, InPinId { node: to, input });
        }
    }
}

/// A change the viewer wants made, applied once the canvas is done drawing.
enum Request {
    Connect {
        from: NodeId,
        to: NodeId,
        resource: Id,
    },
    Disconnect {
        from: NodeId,
        to: NodeId,
        resource: Id,
    },
    Remove(NodeId),
    Add(Node),
    Select(NodeId),
    Status(String),
}

impl Request {
    fn apply(self, state: &mut AppState) {
        match self {
            Self::Connect { from, to, resource } => {
                if !state.editor.add_link(Link::new(from, to, resource)) {
                    state.status = Some("those two are already connected".to_owned());
                }
            }
            Self::Disconnect { from, to, resource } => {
                let index = state.editor.schema().links.iter().position(|link| {
                    link.from == from && link.to == to && link.resource == resource
                });
                if let Some(index) = index {
                    state.editor.remove_link(index);
                }
            }
            Self::Remove(id) => {
                if state.selection.as_ref() == Some(&id) {
                    state.selection = None;
                }
                state.editor.remove_node(&id);
            }
            Self::Add(node) => {
                let id = state.editor.add_node(node);
                state.selection = Some(id);
            }
            Self::Select(id) => state.selection = Some(id),
            Self::Status(message) => state.status = Some(message),
        }
    }
}

struct Viewer<'a> {
    state: &'a AppState,
    requests: Vec<Request>,
}

impl Viewer<'_> {
    fn node_of(&self, snarl: &Snarl<NodeId>, id: SnarlId) -> Option<&Node> {
        let node_id = snarl.get_node(id)?;
        self.state.editor.schema().node(node_id)
    }

    /// What one pin stands for, resolved through the schema.
    fn slot(
        &self,
        snarl: &Snarl<NodeId>,
        node: SnarlId,
        index: usize,
        input: bool,
    ) -> Option<PinSlot> {
        let schema = self.state.editor.schema();
        let node = self.node_of(snarl, node)?;
        let slots = if input {
            input_slots(schema, self.state.catalog(), node)
        } else {
            output_slots(schema, self.state.catalog(), node)
        };
        slots.get(index).cloned()
    }
}

/// Green when balanced, amber when idling, red when it cannot keep up.
fn load_colour(load: Option<f64>) -> Color32 {
    match load {
        Some(load) if load >= 0.999 => Color32::from_rgb(220, 90, 70),
        Some(load) if load > 0.001 => Color32::from_rgb(210, 170, 60),
        Some(_) => Color32::GRAY,
        None => Color32::from_rgb(110, 180, 110),
    }
}

fn pin_colour(rate: Option<f64>) -> Color32 {
    match rate {
        Some(rate) if rate > 1e-9 => Color32::from_rgb(110, 180, 110),
        _ => Color32::GRAY,
    }
}

/// Flow on the link between two nodes carrying `resource`, if solved.
fn flow_between(report: Option<&Report>, from: &NodeId, to: &NodeId, resource: &Id) -> Option<f64> {
    report?
        .links
        .iter()
        .find(|link| &link.from == from && &link.to == to && &link.resource == resource)
        .map(|link| link.rate.as_per_tick())
}

impl SnarlViewer<NodeId> for Viewer<'_> {
    fn title(&mut self, node: &NodeId) -> String {
        let Some(found) = self.state.editor.schema().node(node) else {
            return node.to_string();
        };
        let name = found.label.clone().unwrap_or_else(|| found.id.to_string());
        format!("{name}  ·  {}", found.kind.type_name())
    }

    fn inputs(&mut self, node: &NodeId) -> usize {
        let schema = self.state.editor.schema();
        schema
            .node(node)
            .map(|node| input_slots(schema, self.state.catalog(), node).len())
            .unwrap_or_default()
    }

    fn outputs(&mut self, node: &NodeId) -> usize {
        let schema = self.state.editor.schema();
        schema
            .node(node)
            .map(|node| output_slots(schema, self.state.catalog(), node).len())
            .unwrap_or_default()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let slot = self.slot(snarl, pin.id.node, pin.id.input, true);
        let label = slot.as_ref().map_or_else(|| "?".to_owned(), PinSlot::label);
        ui.label(label);

        let rate = slot
            .as_ref()
            .and_then(PinSlot::concrete)
            .zip(snarl.get_node(pin.id.node))
            .and_then(|(resource, to)| {
                pin.remotes.first().and_then(|remote| {
                    let from = snarl.get_node(remote.node)?;
                    flow_between(self.state.report.as_ref(), from, to, resource)
                })
            });
        PinInfo::circle().with_fill(pin_colour(rate))
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) -> impl egui_snarl::ui::SnarlPin + 'static {
        let slot = self.slot(snarl, pin.id.node, pin.id.output, false);
        let label = slot.as_ref().map_or_else(|| "?".to_owned(), PinSlot::label);
        ui.label(label);
        PinInfo::circle().with_fill(Color32::GRAY)
    }

    fn has_body(&mut self, _node: &NodeId) -> bool {
        true
    }

    fn show_body(
        &mut self,
        node: SnarlId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let Some(id) = snarl.get_node(node).cloned() else {
            return;
        };
        let Some(found) = self.state.editor.schema().node(&id) else {
            return;
        };
        let unit = self.state.unit;
        let report = self.state.report.as_ref();

        match &found.kind {
            NodeKind::Machine(_) => {
                let load = report.and_then(|report| report.machines.iter().find(|m| m.node == id));
                match load {
                    Some(load) => {
                        let text = match (load.count, load.load) {
                            (Some(count), Some(fraction)) => format!(
                                "{:.2}/{count:.0} machines  {:.0}%",
                                load.running,
                                fraction * 100.0
                            ),
                            _ => format!("{:.2} machines", load.running),
                        };
                        ui.colored_label(load_colour(load.load), text);
                    }
                    None => {
                        ui.weak("not solved");
                    }
                }
            }
            NodeKind::Source(source) => {
                let shipped = report
                    .and_then(|report| report.sources.iter().find(|s| s.node == id))
                    .map(|s| s.achieved.as_per(unit));
                ui.label(match (shipped, source.rate) {
                    (Some(shipped), Some(limit)) => format!(
                        "{shipped:.1} of {:.1}{}",
                        limit.rate().as_per(unit),
                        unit.suffix()
                    ),
                    (Some(shipped), None) => format!("{shipped:.1}{}", unit.suffix()),
                    (None, _) => "not solved".to_owned(),
                });
            }
            NodeKind::Output(output) => {
                let got = report.and_then(|report| report.outputs.iter().find(|o| o.node == id));
                match got {
                    Some(got) => {
                        let short = got.shortfall().map(|s| s.as_per(unit)).unwrap_or(0.0);
                        let text = format!("{:.1}{}", got.achieved.as_per(unit), unit.suffix());
                        if short > 1e-9 {
                            ui.colored_label(
                                Color32::from_rgb(220, 90, 70),
                                format!("{text}  short {short:.1}"),
                            );
                        } else {
                            ui.colored_label(Color32::from_rgb(110, 180, 110), text);
                        }
                    }
                    None => {
                        ui.weak("not solved");
                    }
                }
                let _ = output;
            }
            NodeKind::Hub(_) => {
                ui.weak("passes through");
            }
            NodeKind::Note(note) => {
                ui.label(&note.text);
            }
        }
    }

    /// Clicking anywhere on a node selects it, which is less fiddly than a
    /// button inside the body.
    fn final_node_rect(
        &mut self,
        node: SnarlId,
        rect: egui::Rect,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let Some(id) = snarl.get_node(node).cloned() else {
            return;
        };
        let response = ui.interact(
            rect,
            egui::Id::new(("craftflow-node", node)),
            egui::Sense::click(),
        );
        if response.clicked() {
            self.requests.push(Request::Select(id.clone()));
        }
        if self.state.selection.as_ref() == Some(&id) {
            ui.painter().rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(2.0, Color32::from_rgb(120, 170, 230)),
                egui::StrokeKind::Outside,
            );
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        let (Some(from_id), Some(to_id)) = (
            snarl.get_node(from.id.node).cloned(),
            snarl.get_node(to.id.node).cloned(),
        ) else {
            return;
        };

        let out_slot = self.slot(snarl, from.id.node, from.id.output, false);
        let in_slot = self.slot(snarl, to.id.node, to.id.input, true);

        // The output side names the resource; a hub's spare pin does not, and
        // then the input side has to.
        let resource = out_slot
            .as_ref()
            .and_then(PinSlot::concrete)
            .or_else(|| in_slot.as_ref().and_then(PinSlot::concrete))
            .cloned();

        match resource {
            Some(resource) => self.requests.push(Request::Connect {
                from: from_id,
                to: to_id,
                resource,
            }),
            None => self.requests.push(Request::Status(
                "neither end says which resource this carries; connect it to a \
                 machine or set the hub's resource first"
                    .to_owned(),
            )),
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        let (Some(from_id), Some(to_id)) = (
            snarl.get_node(from.id.node).cloned(),
            snarl.get_node(to.id.node).cloned(),
        ) else {
            return;
        };
        let out_slot = self.slot(snarl, from.id.node, from.id.output, false);
        let in_slot = self.slot(snarl, to.id.node, to.id.input, true);
        let Some(resource) = out_slot
            .as_ref()
            .and_then(PinSlot::concrete)
            .or_else(|| in_slot.as_ref().and_then(PinSlot::concrete))
            .cloned()
        else {
            return;
        };
        self.requests.push(Request::Disconnect {
            from: from_id,
            to: to_id,
            resource,
        });
    }

    fn has_wire_widget(&mut self, _from: &OutPinId, _to: &InPinId, _snarl: &Snarl<NodeId>) -> bool {
        self.state.report.is_some()
    }

    /// The flow the solver put on this link, written on the wire itself.
    fn show_wire_widget(
        &mut self,
        from: &OutPin,
        to: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let (Some(from_id), Some(to_id)) = (
            snarl.get_node(from.id.node).cloned(),
            snarl.get_node(to.id.node).cloned(),
        ) else {
            return;
        };
        let Some(resource) = self
            .slot(snarl, from.id.node, from.id.output, false)
            .as_ref()
            .and_then(PinSlot::concrete)
            .cloned()
        else {
            return;
        };
        let Some(rate) = flow_between(self.state.report.as_ref(), &from_id, &to_id, &resource)
        else {
            return;
        };

        let unit = self.state.unit;
        let text = format!(
            "{:.1}{}",
            model::Rate::per_tick(rate).as_per(unit),
            unit.suffix()
        );
        let colour = if rate > 1e-9 {
            Color32::from_rgb(110, 180, 110)
        } else {
            Color32::GRAY
        };
        ui.label(egui::RichText::new(text).small().color(colour));
    }

    fn has_node_menu(&mut self, _node: &NodeId) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: SnarlId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        let Some(id) = snarl.get_node(node).cloned() else {
            return;
        };
        if ui.button("Select").clicked() {
            self.requests.push(Request::Select(id.clone()));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            self.requests.push(Request::Remove(id));
            ui.close();
        }
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<NodeId>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: Pos2, ui: &mut Ui, _snarl: &mut Snarl<NodeId>) {
        ui.label("Add node");
        let position = Some([pos.x, pos.y]);

        let fresh = |prefix: &str| self.state.editor.fresh_node_id(prefix);

        if ui.button("Hub").clicked() {
            let mut node = Node::new(fresh("hub"), NodeKind::Hub(HubNode::default()));
            node.position = position;
            self.requests.push(Request::Add(node));
            ui.close();
        }
        if ui.button("Note").clicked() {
            let mut node = Node::new(
                fresh("note"),
                NodeKind::Note(NoteNode {
                    text: "note".to_owned(),
                }),
            );
            node.position = position;
            self.requests.push(Request::Add(node));
            ui.close();
        }
        ui.separator();
        ui.weak("drag an item from the browser for machines, sources and outputs");
    }
}

/// Formats a rate for the canvas and panels.
pub fn format_rate(rate: model::Rate, unit: TimeUnit) -> String {
    format!("{:.2}{}", rate.as_per(unit), unit.suffix())
}
