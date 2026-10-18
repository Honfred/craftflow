//! Reference scenarios 1 and 4 from DESIGN.md §11.

use std::path::PathBuf;

use graph::{Flow, Link, MachineNode, Network, Node, NodeKind, OutputNode, Schema, SourceNode};
use model::{Catalog, Id, TimeUnit, approx_eq};
use solver::{Report, SolveError, solve};

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

fn machine(id_: &str, machine: &str, recipe: &str, count: Option<f64>) -> Node {
    Node::new(
        id_,
        NodeKind::Machine(MachineNode {
            machine: id(machine),
            recipe: id(recipe),
            params: Default::default(),
            count,
            networks: Default::default(),
        }),
    )
}

fn source(id_: &str, resource: &str, per_minute: Option<f64>) -> Node {
    Node::new(
        id_,
        NodeKind::Source(SourceNode {
            resource: id(resource),
            rate: per_minute.map(|amount| Flow::new(amount, TimeUnit::Minute)),
        }),
    )
}

fn output(id_: &str, resource: &str, per_minute: Option<f64>) -> Node {
    Node::new(
        id_,
        NodeKind::Output(OutputNode {
            resource: id(resource),
            target: per_minute.map(|amount| Flow::new(amount, TimeUnit::Minute)),
        }),
    )
}

fn per_minute(report: &Report, node: &str) -> f64 {
    report
        .outputs
        .iter()
        .find(|o| o.node.as_str() == node)
        .map(|o| o.achieved.as_per(TimeUnit::Minute))
        .expect("output node in report")
}

fn load_of(report: &Report, node: &str) -> f64 {
    report
        .machines
        .iter()
        .find(|m| m.node.as_str() == node)
        .and_then(|m| m.load)
        .expect("machine load in report")
}

// ---------------------------------------------------------------------------
// Scenario 1: iron ore into ingots in N furnaces, with fuel.
// ---------------------------------------------------------------------------

/// Eight furnaces, 200 ticks per smelt, fed from a source.
fn furnaces(ore_per_minute: Option<f64>, count: Option<f64>) -> Schema {
    let mut schema = Schema {
        nodes: vec![
            source("ore", "minecraft:iron_ore", ore_per_minute),
            machine(
                "furnaces",
                "minecraft:furnace",
                "minecraft:smelting/iron_ingot_from_ore",
                count,
            ),
            output("ingots", "minecraft:iron_ingot", None),
        ],
        links: vec![
            Link::new("ore", "furnaces", id("minecraft:iron_ore")),
            Link::new("furnaces", "ingots", id("minecraft:iron_ingot")),
        ],
        ..Schema::default()
    };
    schema.networks.insert(
        "fuel".to_owned(),
        Network {
            kind: id("vanilla:burn_time"),
            label: None,
        },
    );
    if let NodeKind::Machine(node) = &mut schema.nodes[1].kind {
        node.networks.insert("fuel".to_owned(), "fuel".to_owned());
    }
    schema
}

#[test]
fn scenario_1_machines_are_the_bottleneck() {
    let registry = vanilla();
    // Plenty of ore, eight furnaces: the furnaces decide the answer.
    let report =
        solve(&furnaces(Some(600.0), Some(8.0)), &registry as &dyn Catalog).expect("solves");

    // 8 furnaces / 200 ticks = 0.04 smelts per tick = 48 per minute.
    assert!(approx_eq(per_minute(&report, "ingots"), 48.0));
    assert!(approx_eq(load_of(&report, "furnaces"), 1.0));
    assert_eq!(report.bottlenecks().len(), 1);

    // The ore that was not needed stays at the source.
    let used = report.sources[0].achieved.as_per(TimeUnit::Minute);
    assert!(approx_eq(used, 48.0), "used {used}");
}

#[test]
fn scenario_1_the_supply_can_be_the_bottleneck_instead() {
    let registry = vanilla();
    // Only 30 ore a minute for the same eight furnaces.
    let report =
        solve(&furnaces(Some(30.0), Some(8.0)), &registry as &dyn Catalog).expect("solves");

    assert!(approx_eq(per_minute(&report, "ingots"), 30.0));
    // 30/min needs 6.25 of the 8 furnaces.
    assert!(approx_eq(load_of(&report, "furnaces"), 30.0 / 48.0));
    assert!(report.bottlenecks().is_empty());
}

#[test]
fn scenario_1_reports_the_fuel_network() {
    let registry = vanilla();
    let report =
        solve(&furnaces(Some(600.0), Some(8.0)), &registry as &dyn Catalog).expect("solves");

    let fuel = report
        .networks
        .iter()
        .find(|n| n.name == "fuel")
        .expect("fuel network");

    // fuel_per_tick = 1/1600 per running furnace; at full load 8 are running,
    // so 8/1600 per tick, which is 6 coal a minute.
    assert!(
        approx_eq(fuel.consumed.as_per(TimeUnit::Minute), 6.0),
        "got {}",
        fuel.consumed.as_per(TimeUnit::Minute)
    );
    // Nothing generates burn time in this schema, so the network is short.
    assert!(fuel.is_short());
}

#[test]
fn scenario_1_units_survive_the_round_trip() {
    let registry = vanilla();
    let report =
        solve(&furnaces(Some(600.0), Some(8.0)), &registry as &dyn Catalog).expect("solves");

    let ingots = &report.outputs[0];
    assert!(approx_eq(ingots.achieved.as_per_tick(), 0.04));
    assert!(approx_eq(ingots.achieved.as_per(TimeUnit::Second), 0.8));
    assert!(approx_eq(ingots.achieved.as_per(TimeUnit::Minute), 48.0));
    assert!(approx_eq(ingots.achieved.as_per(TimeUnit::Hour), 2880.0));
}

#[test]
fn an_output_target_caps_production_without_making_it_infeasible() {
    let registry = vanilla();
    let mut schema = furnaces(Some(600.0), Some(8.0));

    // Ask for less than the furnaces could make.
    if let NodeKind::Output(output) = &mut schema.nodes[2].kind {
        output.target = Some(Flow::new(24.0, TimeUnit::Minute));
    }
    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    assert!(approx_eq(per_minute(&report, "ingots"), 24.0));
    assert!(approx_eq(load_of(&report, "furnaces"), 0.5));
    assert!(report.outputs[0].shortfall().expect("target").as_per_tick() == 0.0);

    // Ask for more than they can: a shortfall, not an unsolvable problem.
    if let NodeKind::Output(output) = &mut schema.nodes[2].kind {
        output.target = Some(Flow::new(120.0, TimeUnit::Minute));
    }
    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    assert!(approx_eq(per_minute(&report, "ingots"), 48.0));
    let short = report.outputs[0].shortfall().expect("target");
    assert!(approx_eq(short.as_per(TimeUnit::Minute), 72.0));
}

#[test]
fn a_tag_input_is_fed_by_any_member_of_the_tag() {
    let registry = vanilla();
    // The recipe wants `#c:ores/iron`; feed it the deepslate variant.
    let mut schema = furnaces(Some(600.0), Some(8.0));
    if let NodeKind::Source(source) = &mut schema.nodes[0].kind {
        source.resource = id("minecraft:deepslate_iron_ore");
    }
    schema.links[0].resource = id("minecraft:deepslate_iron_ore");

    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    assert!(approx_eq(per_minute(&report, "ingots"), 48.0));
    // The ore is eaten, not left lying around.
    assert!(
        report.surplus.iter().all(|s| s.node.as_str() != "furnaces"),
        "got {:#?}",
        report.surplus
    );
}

#[test]
fn a_schema_with_no_limits_anywhere_says_so_plainly() {
    let registry = vanilla();
    let schema = furnaces(None, None);
    let error = solve(&schema, &registry as &dyn Catalog).expect_err("unbounded");
    assert!(matches!(error, SolveError::Unbounded), "got {error}");
}

#[test]
fn a_link_capacity_limits_the_flow() {
    let registry = vanilla();
    let mut schema = furnaces(Some(600.0), Some(8.0));
    schema.links[1].capacity = Some(Flow::new(12.0, TimeUnit::Minute));

    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    assert!(approx_eq(per_minute(&report, "ingots"), 12.0));
    assert!(report.links[1].at_capacity);
    assert!(approx_eq(load_of(&report, "furnaces"), 0.25));
}

#[test]
fn an_estimated_input_marks_the_whole_result() {
    let registry = vanilla();
    // The cobblestone generator is `estimated` in the pack.
    let schema = Schema {
        nodes: vec![
            machine(
                "gen",
                "vanilla:cobblestone_generator",
                "vanilla:source/cobblestone_generator",
                Some(1.0),
            ),
            output("cobble", "minecraft:cobblestone", None),
        ],
        links: vec![Link::new("gen", "cobble", id("minecraft:cobblestone"))],
        ..Schema::default()
    };

    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    assert!(report.accuracy.is_estimated());
    // One generator, 20 ticks a cobble: 60 a minute.
    assert!(approx_eq(per_minute(&report, "cobble"), 60.0));
}

// ---------------------------------------------------------------------------
// Scenario 4: a byproduct fed back into the input.
// ---------------------------------------------------------------------------

/// Nuggets are the loop: ingots craft into nine nuggets, nine nuggets craft
/// back into an ingot. Feeding the nuggets back must converge rather than
/// spin up forever.
fn nugget_loop(ingots_per_minute: f64) -> Schema {
    Schema {
        nodes: vec![
            source("ingots_in", "minecraft:iron_ingot", Some(ingots_per_minute)),
            Node::new("chest", NodeKind::Hub(Default::default())),
            machine(
                "to_nuggets",
                "minecraft:crafting_table",
                "minecraft:crafting/iron_nugget",
                None,
            ),
            machine(
                "to_ingots",
                "minecraft:crafting_table",
                "minecraft:crafting/iron_ingot_from_nuggets",
                None,
            ),
            output("nuggets_out", "minecraft:iron_nugget", None),
        ],
        links: vec![
            Link::new("ingots_in", "chest", id("minecraft:iron_ingot")),
            Link::new("chest", "to_nuggets", id("minecraft:iron_ingot")),
            Link::new("to_nuggets", "nuggets_out", id("minecraft:iron_nugget")),
            // The loop: some nuggets go back to be re-pressed into ingots.
            Link::new("to_nuggets", "to_ingots", id("minecraft:iron_nugget")),
            Link::new("to_ingots", "chest", id("minecraft:iron_ingot")),
        ],
        ..Schema::default()
    }
}

#[test]
fn scenario_4_a_cycle_converges() {
    let registry = vanilla();
    let report = solve(&nugget_loop(60.0), &registry as &dyn Catalog).expect("solves");

    // Recycling nine nuggets back into one ingot neither creates nor destroys
    // iron, so the best the loop can do is 9 nuggets per ingot fed in.
    assert!(
        approx_eq(per_minute(&report, "nuggets_out"), 540.0),
        "got {}",
        per_minute(&report, "nuggets_out")
    );
}

#[test]
fn scenario_4_the_loop_does_not_invent_material() {
    let registry = vanilla();
    let report = solve(&nugget_loop(60.0), &registry as &dyn Catalog).expect("solves");

    // Iron in equals iron out, counted in ingots: 60 in, 540 nuggets out.
    let ingots_in = report.sources[0].achieved.as_per(TimeUnit::Minute);
    let nuggets_out = per_minute(&report, "nuggets_out");
    assert!(approx_eq(ingots_in, 60.0), "in {ingots_in}");
    assert!(approx_eq(nuggets_out / 9.0, ingots_in));

    // Nothing piles up anywhere.
    assert!(
        report.surplus.is_empty(),
        "unexpected surplus: {:#?}",
        report.surplus
    );
}

#[test]
fn scenario_4_surplus_shows_up_when_the_loop_is_cut() {
    let registry = vanilla();
    let mut schema = nugget_loop(60.0);
    // Remove the output, so the nuggets have nowhere to go.
    schema
        .nodes
        .retain(|node| node.id.as_str() != "nuggets_out");
    schema
        .links
        .retain(|link| link.to.as_str() != "nuggets_out");

    let report = solve(&schema, &registry as &dyn Catalog).expect("solves");
    // With nothing to maximise the solver does nothing, and says so.
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains("no output node"))
    );
}
