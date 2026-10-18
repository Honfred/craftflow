//! Schema round-tripping and validation against the vanilla pack.

use std::path::PathBuf;

use graph::{
    CURRENT_FORMAT_VERSION, Flow, Link, MachineNode, Network, Node, NodeKind, OutputNode, Schema,
    SourceNode, validate,
};
use model::{Catalog, Id, Issue, TimeUnit};

fn id(text: &str) -> Id {
    text.parse().expect("valid id")
}

fn vanilla() -> packs::Registry {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/vanilla/packs/vanilla");
    let loaded = packs::load([root]).expect("loads");
    assert!(loaded.is_clean());
    loaded.registry
}

/// Scenario 1 from DESIGN.md §11, as a drawing: ore in, furnaces, ingots out.
fn furnace_schema() -> Schema {
    Schema {
        format_version: CURRENT_FORMAT_VERSION,
        nodes: vec![
            Node::new(
                "ore",
                NodeKind::Source(SourceNode {
                    resource: id("minecraft:iron_ore"),
                    rate: Some(Flow::new(600.0, TimeUnit::Minute)),
                }),
            ),
            Node::new(
                "furnaces",
                NodeKind::Machine(MachineNode {
                    machine: id("minecraft:furnace"),
                    recipe: id("minecraft:smelting/iron_ingot_from_ore"),
                    params: Default::default(),
                    count: Some(8.0),
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
    }
}

fn errors(issues: &[Issue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.is_error())
        .map(ToString::to_string)
        .collect()
}

#[test]
fn a_schema_round_trips_through_json() {
    let schema = furnace_schema();
    let text = schema.to_json_pretty().expect("serialises");
    let back = Schema::from_json(&text).expect("parses");
    assert_eq!(back, schema);
}

#[test]
fn the_json_shape_is_what_the_design_describes() {
    let text = furnace_schema().to_json_pretty().expect("serialises");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parses");

    assert_eq!(value["format_version"], 1);
    assert_eq!(value["nodes"][0]["type"], "source");
    assert_eq!(value["nodes"][1]["type"], "machine");
    assert_eq!(value["nodes"][1]["machine"], "minecraft:furnace");
    // A link names the resource it carries; there are no port identifiers.
    assert_eq!(value["links"][0]["resource"], "minecraft:iron_ore");
    assert!(value["links"][0].get("port").is_none());
    // Empty collections stay out of the file.
    assert!(value["nodes"][1].get("params").is_none());
}

#[test]
fn flows_say_which_unit_they_mean() {
    let text = furnace_schema().to_json_pretty().expect("serialises");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parses");
    assert_eq!(
        value["nodes"][0]["rate"],
        serde_json::json!({ "amount": 600.0, "unit": "minute" })
    );

    // 600 per minute is half per tick, which is what the core works in.
    let flow = Flow::new(600.0, TimeUnit::Minute);
    assert!(model::approx_eq(flow.rate().as_per_tick(), 0.5));
}

#[test]
fn a_flow_without_a_unit_means_per_minute() {
    let flow: Flow = serde_json::from_str(r#"{ "amount": 60 }"#).expect("parses");
    assert_eq!(flow.unit, TimeUnit::Minute);
    assert!(model::approx_eq(flow.rate().as_per(TimeUnit::Second), 1.0));
}

#[test]
fn the_furnace_schema_validates_against_vanilla() {
    let registry = vanilla();
    let issues = validate(&furnace_schema(), &registry as &dyn Catalog);
    assert!(errors(&issues).is_empty(), "got {:#?}", errors(&issues));
}

#[test]
fn a_link_carrying_something_the_node_does_not_make_is_reported() {
    let mut schema = furnace_schema();
    schema.links[1].resource = id("minecraft:glass");

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found
            .iter()
            .any(|m| m.contains("does not produce") && m.contains("glass")),
        "got {found:#?}"
    );
    // The message points at the recipe, since switching it is the usual cause.
    assert!(
        found.iter().any(|m| m.contains("it runs")),
        "got {found:#?}"
    );
}

#[test]
fn a_machine_running_the_wrong_category_is_reported() {
    let mut schema = furnace_schema();
    if let NodeKind::Machine(machine) = &mut schema.nodes[1].kind {
        machine.recipe = id("minecraft:blasting/iron_ingot_from_ore");
    }

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found.iter().any(|m| m.contains("cannot run")),
        "got {found:#?}"
    );
}

#[test]
fn dangling_links_and_duplicate_ids_are_reported() {
    let mut schema = furnace_schema();
    schema
        .links
        .push(Link::new("ore", "nowhere", id("minecraft:iron_ore")));
    schema.nodes.push(Node::new(
        "ore",
        NodeKind::Output(OutputNode {
            resource: id("minecraft:iron_ingot"),
            target: None,
        }),
    ));

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found.iter().any(|m| m.contains("not a node")),
        "got {found:#?}"
    );
    assert!(
        found.iter().any(|m| m.contains("share this id")),
        "got {found:#?}"
    );
}

#[test]
fn unknown_machines_recipes_and_resources_are_reported() {
    let mut schema = furnace_schema();
    if let NodeKind::Machine(machine) = &mut schema.nodes[1].kind {
        machine.machine = id("nope:machine");
    }
    if let NodeKind::Source(source) = &mut schema.nodes[0].kind {
        source.resource = id("nope:ore");
    }

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found.iter().any(|m| m.contains("nope:machine")),
        "got {found:#?}"
    );
    assert!(
        found.iter().any(|m| m.contains("nope:ore")),
        "got {found:#?}"
    );
}

#[test]
fn a_network_of_the_wrong_kind_is_reported() {
    let mut schema = furnace_schema();
    schema.networks.insert(
        "main".to_owned(),
        Network {
            kind: id("create:kinetic"),
            label: None,
        },
    );
    if let NodeKind::Machine(machine) = &mut schema.nodes[1].kind {
        machine
            .networks
            .insert("fuel".to_owned(), "main".to_owned());
    }

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found.iter().any(|m| m.contains("vanilla:burn_time")),
        "got {found:#?}"
    );
}

#[test]
fn a_parameter_the_machine_does_not_have_is_reported() {
    let mut schema = furnace_schema();
    if let NodeKind::Machine(machine) = &mut schema.nodes[1].kind {
        machine
            .params
            .insert("speed_upgrades".to_owned(), model::ParamValue::Int(4));
    }

    let registry = vanilla();
    let found = errors(&validate(&schema, &registry as &dyn Catalog));
    assert!(
        found.iter().any(|m| m.contains("speed_upgrades")),
        "got {found:#?}"
    );
}

#[test]
fn a_tag_input_accepts_any_of_its_resources() {
    // The furnace recipe takes `#c:ores/iron`, and the link carries one of the
    // two ores that tag stands for.
    let mut schema = furnace_schema();
    if let NodeKind::Source(source) = &mut schema.nodes[0].kind {
        source.resource = id("minecraft:deepslate_iron_ore");
    }
    schema.links[0].resource = id("minecraft:deepslate_iron_ore");

    let registry = vanilla();
    let issues = validate(&schema, &registry as &dyn Catalog);
    assert!(errors(&issues).is_empty(), "got {:#?}", errors(&issues));
}

#[test]
fn the_catalog_evaluates_machine_formulas() {
    let registry = vanilla();
    let catalog = &registry as &dyn Catalog;

    let recipe = catalog
        .recipe(&id("minecraft:smelting/iron_ingot_from_ore"))
        .expect("recipe");

    // The blast furnace halves the recipe duration via `blasting_speed`.
    let duration = catalog
        .evaluate(
            &id("minecraft:blast_furnace"),
            "duration",
            recipe,
            &Default::default(),
        )
        .expect("evaluates");
    assert_eq!(duration, Some(100.0));

    // A formula the machine does not define is simply absent.
    let missing = catalog
        .evaluate(
            &id("minecraft:furnace"),
            "stress",
            recipe,
            &Default::default(),
        )
        .expect("evaluates");
    assert_eq!(missing, None);
}
