//! Editing a schema: what each operation touches, and undo/redo.

use graph::{
    Editor, Flow, HISTORY_LIMIT, Link, MachineNode, Network, Node, NodeId, NodeKind, OutputNode,
    Schema, SourceNode,
};
use model::{Id, ParamValue, TimeUnit};

fn id(text: &str) -> Id {
    text.parse().expect("valid id")
}

fn editor() -> Editor {
    Editor::new(Schema {
        nodes: vec![
            Node::new(
                "ore",
                NodeKind::Source(SourceNode {
                    resource: id("minecraft:iron_ore"),
                    rate: None,
                }),
            ),
            Node::new(
                "furnaces",
                NodeKind::Machine(MachineNode {
                    machine: id("minecraft:furnace"),
                    recipe: id("minecraft:smelting/iron_ingot_from_ore"),
                    params: Default::default(),
                    count: Some(4.0),
                    networks: Default::default(),
                }),
            ),
            Node::new(
                "ingots",
                NodeKind::Output(OutputNode {
                    resource: id("minecraft:iron_ingot"),
                    target: None,
                }),
            ),
        ],
        links: vec![
            Link::new("ore", "furnaces", id("minecraft:iron_ore")),
            Link::new("furnaces", "ingots", id("minecraft:iron_ingot")),
        ],
        ..Schema::default()
    })
}

fn count_of(editor: &Editor, node: &str) -> Option<f64> {
    editor
        .schema()
        .node(&NodeId::new(node))
        .and_then(graph::Node::machine)
        .and_then(|machine| machine.count)
}

#[test]
fn a_new_editor_is_clean_and_has_no_history() {
    let editor = editor();
    assert!(!editor.has_unsaved_changes());
    assert!(!editor.can_undo());
    assert!(!editor.can_redo());
}

#[test]
fn removing_a_node_takes_its_links_with_it() {
    let mut editor = editor();
    editor.remove_node(&NodeId::new("furnaces"));

    assert_eq!(editor.schema().nodes.len(), 2);
    assert!(
        editor.schema().links.is_empty(),
        "links left dangling: {:?}",
        editor.schema().links
    );
}

#[test]
fn undo_restores_the_links_too() {
    let mut editor = editor();
    let before = editor.schema().clone();

    editor.remove_node(&NodeId::new("furnaces"));
    assert_eq!(editor.undo(), Some("remove node"));

    assert_eq!(editor.schema(), &before);
    assert!(editor.can_redo());
    assert_eq!(editor.redo(), Some("remove node"));
    assert_eq!(editor.schema().nodes.len(), 2);
}

#[test]
fn dragging_a_node_leaves_one_undo_step() {
    let mut editor = editor();
    let node = NodeId::new("furnaces");

    for frame in 0..60 {
        editor.move_node(&node, [frame as f32, 0.0]);
    }

    assert_eq!(
        editor.schema().node(&node).and_then(|n| n.position),
        Some([59.0, 0.0])
    );

    // One undo returns to where the drag started.
    editor.undo();
    assert_eq!(editor.schema().node(&node).and_then(|n| n.position), None);
    assert!(!editor.can_undo());
}

#[test]
fn moving_a_different_node_starts_a_new_step() {
    let mut editor = editor();
    editor.move_node(&NodeId::new("furnaces"), [1.0, 1.0]);
    editor.move_node(&NodeId::new("ore"), [2.0, 2.0]);

    editor.undo();
    assert_eq!(
        editor
            .schema()
            .node(&NodeId::new("ore"))
            .and_then(|n| n.position),
        None
    );
    assert_eq!(
        editor
            .schema()
            .node(&NodeId::new("furnaces"))
            .and_then(|n| n.position),
        Some([1.0, 1.0])
    );
}

#[test]
fn a_different_kind_of_edit_breaks_the_merge() {
    let mut editor = editor();
    let node = NodeId::new("furnaces");

    editor.set_count(&node, Some(8.0));
    editor.set_count(&node, Some(9.0));
    // Same node, different field: must not merge into the count edits.
    editor.set_label(&node, Some("Bank".to_owned()));

    editor.undo();
    assert_eq!(count_of(&editor, "furnaces"), Some(9.0));
    editor.undo();
    assert_eq!(count_of(&editor, "furnaces"), Some(4.0));
}

#[test]
fn a_new_edit_after_undo_clears_the_redo_stack() {
    let mut editor = editor();
    editor.set_count(&NodeId::new("furnaces"), Some(8.0));
    editor.undo();
    assert!(editor.can_redo());

    editor.set_count(&NodeId::new("furnaces"), Some(2.0));
    assert!(!editor.can_redo());
    assert_eq!(count_of(&editor, "furnaces"), Some(2.0));
}

#[test]
fn an_edit_right_after_undo_does_not_merge_into_the_uncovered_step() {
    let mut editor = editor();
    let node = NodeId::new("furnaces");

    editor.set_count(&node, Some(8.0));
    editor.undo();
    assert_eq!(count_of(&editor, "furnaces"), Some(4.0));

    // Same key as the undone edit; it must still take its own snapshot.
    editor.set_count(&node, Some(16.0));
    editor.undo();
    assert_eq!(count_of(&editor, "furnaces"), Some(4.0));
}

#[test]
fn the_history_is_bounded() {
    let mut editor = editor();
    for n in 0..(HISTORY_LIMIT + 50) {
        editor.set_label(&NodeId::new("ore"), Some(format!("{n}")));
        // Break the merge each time, so every edit is its own step.
        editor.set_label(&NodeId::new("furnaces"), Some(format!("{n}")));
    }

    let mut steps = 0;
    while editor.undo().is_some() {
        steps += 1;
        assert!(steps <= HISTORY_LIMIT, "history grew past the limit");
    }
    assert_eq!(steps, HISTORY_LIMIT);
}

#[test]
fn duplicate_and_self_links_are_refused() {
    let mut editor = editor();

    assert!(!editor.add_link(Link::new("ore", "furnaces", id("minecraft:iron_ore"))));
    assert!(!editor.add_link(Link::new("ore", "ore", id("minecraft:iron_ore"))));
    assert_eq!(editor.schema().links.len(), 2);
    // A refused link is not an edit, so there is nothing to undo.
    assert!(!editor.can_undo());

    // A different resource between the same nodes is a real link.
    assert!(editor.add_link(Link::new("ore", "furnaces", id("minecraft:coal"))));
    assert_eq!(editor.schema().links.len(), 3);
}

#[test]
fn removing_a_network_detaches_the_machines_on_it() {
    let mut editor = editor();
    editor.add_network(
        "main".to_owned(),
        Network {
            kind: id("vanilla:burn_time"),
            label: None,
        },
    );
    editor.set_node_network(&NodeId::new("furnaces"), "fuel", Some("main".to_owned()));

    editor.remove_network("main");

    assert!(editor.schema().networks.is_empty());
    let machine = editor
        .schema()
        .node(&NodeId::new("furnaces"))
        .and_then(graph::Node::machine)
        .expect("machine");
    assert!(
        machine.networks.is_empty(),
        "still attached: {:?}",
        machine.networks
    );
}

#[test]
fn parameters_can_be_set_and_cleared_back_to_the_default() {
    let mut editor = editor();
    let node = NodeId::new("furnaces");

    editor.set_param(&node, "speed", Some(ParamValue::Int(3)));
    let machine = editor
        .schema()
        .node(&node)
        .and_then(graph::Node::machine)
        .expect("machine");
    assert_eq!(machine.params["speed"], ParamValue::Int(3));

    editor.set_param(&node, "speed", None);
    let machine = editor
        .schema()
        .node(&node)
        .and_then(graph::Node::machine)
        .expect("machine");
    assert!(machine.params.is_empty());
}

#[test]
fn fresh_ids_do_not_collide() {
    let mut editor = editor();
    let first = editor.fresh_node_id("node");
    editor.add_node(Node::new(first.clone(), NodeKind::Hub(Default::default())));
    let second = editor.fresh_node_id("node");

    assert_ne!(first, second);
    assert!(editor.schema().node(&second).is_none());
}

#[test]
fn the_dirty_flag_follows_the_document() {
    let mut editor = editor();
    assert!(!editor.has_unsaved_changes());

    editor.set_count(&NodeId::new("furnaces"), Some(8.0));
    assert!(editor.has_unsaved_changes());

    editor.mark_saved();
    assert!(!editor.has_unsaved_changes());

    // Undoing is a change too: the file on disk no longer matches.
    editor.undo();
    assert!(editor.has_unsaved_changes());
}

#[test]
fn opening_a_file_forgets_the_history() {
    let mut editor = editor();
    editor.set_count(&NodeId::new("furnaces"), Some(8.0));

    editor.replace(Schema::default());
    assert!(!editor.can_undo());
    assert!(!editor.can_redo());
    assert!(!editor.has_unsaved_changes());
}

#[test]
fn source_rates_and_output_targets_round_trip() {
    let mut editor = editor();
    editor.set_source_rate(
        &NodeId::new("ore"),
        Some(Flow::new(600.0, TimeUnit::Minute)),
    );
    editor.set_output_target(
        &NodeId::new("ingots"),
        Some(Flow::new(48.0, TimeUnit::Minute)),
    );

    let text = editor.schema().to_json_pretty().expect("serialises");
    let back = Schema::from_json(&text).expect("parses");
    assert_eq!(&back, editor.schema());
}

#[test]
fn moving_a_node_does_not_count_as_a_content_change() {
    let mut editor = editor();
    let before = editor.content_revision();

    editor.move_node(&NodeId::new("furnaces"), [10.0, 10.0]);

    // The drawing moved, so something changed...
    assert!(editor.revision() > 0);
    // ...but nothing that changes the answer or the node's pins, so a drag
    // triggers neither a re-solve nor a canvas rebuild.
    assert_eq!(editor.content_revision(), before);

    editor.set_count(&NodeId::new("furnaces"), Some(8.0));
    assert!(editor.content_revision() > before);
}

#[test]
fn undo_is_a_content_change_even_when_it_undoes_a_move() {
    let mut editor = editor();
    editor.move_node(&NodeId::new("furnaces"), [10.0, 10.0]);
    let before = editor.content_revision();

    editor.undo();
    assert!(editor.content_revision() > before);
}
