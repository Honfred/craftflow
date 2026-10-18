//! The reference scenarios of DESIGN.md §11, run from `tests/scenarios/`.
//!
//! Each directory holds a `schema.json` and an `expected.json` with the
//! numbers it must produce. Adding a scenario means adding a directory; no
//! Rust changes are needed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use model::{Catalog, TimeUnit, approx_eq};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Expected {
    #[allow(dead_code)]
    description: String,
    #[serde(default = "minute")]
    unit: TimeUnit,
    #[serde(default)]
    outputs: BTreeMap<String, f64>,
    #[serde(default)]
    loads: BTreeMap<String, f64>,
    #[serde(default)]
    sources: BTreeMap<String, f64>,
    #[serde(default)]
    networks: BTreeMap<String, ExpectedNetwork>,
    /// Keyed `from->to/resource`, so a loop's own flow can be pinned down.
    #[serde(default)]
    links: BTreeMap<String, f64>,
    #[serde(default)]
    surplus: BTreeMap<String, f64>,
    #[serde(default)]
    accuracy: model::Accuracy,
}

#[derive(Debug, Deserialize)]
struct ExpectedNetwork {
    #[serde(default)]
    produced: f64,
    #[serde(default)]
    consumed: f64,
}

fn minute() -> TimeUnit {
    TimeUnit::Minute
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scenario_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(repo_root().join("tests/scenarios"))
        .expect("tests/scenarios exists")
        .map(|entry| entry.expect("readable").path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The built-in packs, plus the scenario's own `pack/` when it has one.
fn registry_for(dir: &Path) -> packs::Registry {
    let mut roots = vec![repo_root().join("plugins/vanilla/packs/vanilla")];
    let own = dir.join("pack");
    if own.is_dir() {
        roots.push(own);
    }
    let loaded = packs::load(roots).expect("loads");
    assert!(
        loaded.is_clean(),
        "pack errors: {:#?}",
        loaded.errors().collect::<Vec<_>>()
    );
    loaded.registry
}

fn check(dir: &Path) {
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    let registry = registry_for(dir);
    let registry = &registry;

    let schema_text = std::fs::read_to_string(dir.join("schema.json")).expect("schema.json");
    let schema = graph::Schema::from_json(&schema_text).expect("schema parses");

    let expected_text = std::fs::read_to_string(dir.join("expected.json")).expect("expected.json");
    let expected: Expected = serde_json::from_str(&expected_text).expect("expected parses");

    let catalog = registry as &dyn Catalog;

    let problems = graph::validate(&schema, catalog);
    let errors: Vec<String> = problems
        .iter()
        .filter(|issue| issue.is_error())
        .map(ToString::to_string)
        .collect();
    assert!(errors.is_empty(), "{name}: schema is invalid: {errors:#?}");

    let report = solver::solve(&schema, catalog).expect("solves");
    let unit = expected.unit;

    for (node, want) in &expected.outputs {
        let got = report
            .outputs
            .iter()
            .find(|o| o.node.as_str() == node)
            .unwrap_or_else(|| panic!("{name}: no output node `{node}`"))
            .achieved
            .as_per(unit);
        assert!(
            approx_eq(got, *want),
            "{name}: output {node}: {got} != {want}"
        );
    }

    for (node, want) in &expected.loads {
        let got = report
            .machines
            .iter()
            .find(|m| m.node.as_str() == node)
            .unwrap_or_else(|| panic!("{name}: no machine node `{node}`"))
            .load
            .unwrap_or_else(|| panic!("{name}: machine `{node}` has no count, so no load"));
        assert!(
            approx_eq(got, *want),
            "{name}: load {node}: {got} != {want}"
        );
    }

    for (node, want) in &expected.sources {
        let got = report
            .sources
            .iter()
            .find(|s| s.node.as_str() == node)
            .unwrap_or_else(|| panic!("{name}: no source node `{node}`"))
            .achieved
            .as_per(unit);
        assert!(
            approx_eq(got, *want),
            "{name}: source {node}: {got} != {want}"
        );
    }

    for (network, want) in &expected.networks {
        let got = report
            .networks
            .iter()
            .find(|n| &n.name == network)
            .unwrap_or_else(|| panic!("{name}: no network `{network}`"));
        assert!(
            approx_eq(got.produced.as_per(unit), want.produced),
            "{name}: network {network} produced: {} != {}",
            got.produced.as_per(unit),
            want.produced
        );
        assert!(
            approx_eq(got.consumed.as_per(unit), want.consumed),
            "{name}: network {network} consumed: {} != {}",
            got.consumed.as_per(unit),
            want.consumed
        );
    }

    for (key, want) in &expected.links {
        let got = report
            .links
            .iter()
            .find(|l| format!("{}->{}/{}", l.from, l.to, l.resource) == *key)
            .unwrap_or_else(|| panic!("{name}: no link `{key}`"))
            .rate
            .as_per(unit);
        assert!(approx_eq(got, *want), "{name}: link {key}: {got} != {want}");
    }

    // `surplus` lists exactly what is allowed to pile up, keyed `node/resource`.
    let got_surplus: BTreeMap<String, f64> = report
        .surplus
        .iter()
        .map(|s| (format!("{}/{}", s.node, s.resource), s.rate.as_per(unit)))
        .collect();
    assert_eq!(
        got_surplus.keys().collect::<Vec<_>>(),
        expected.surplus.keys().collect::<Vec<_>>(),
        "{name}: surplus differs"
    );
    for (key, want) in &expected.surplus {
        let got = got_surplus[key];
        assert!(
            approx_eq(got, *want),
            "{name}: surplus {key}: {got} != {want}"
        );
    }

    assert_eq!(report.accuracy, expected.accuracy, "{name}: accuracy");
}

#[test]
fn every_scenario_matches_its_expected_numbers() {
    let dirs = scenario_dirs();
    assert!(!dirs.is_empty(), "no scenarios found");

    for dir in dirs {
        check(&dir);
    }
}

#[test]
fn the_cli_solves_a_scenario_end_to_end() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_craftflow"))
        .current_dir(repo_root())
        .args([
            "solve",
            "tests/scenarios/01-vanilla-furnaces/schema.json",
            "--pack",
            "plugins/vanilla/packs/vanilla",
        ])
        .output()
        .expect("runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "failed:\n{stdout}");
    assert!(stdout.contains("48.00/min"), "got:\n{stdout}");
    assert!(stdout.contains("bottleneck"), "got:\n{stdout}");
    assert!(stdout.contains("100.0%"), "got:\n{stdout}");
}

#[test]
fn the_cli_solves_the_cycle_scenario() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_craftflow"))
        .current_dir(repo_root())
        .args([
            "solve",
            "tests/scenarios/04-byproduct-cycle/schema.json",
            "--pack",
            "tests/scenarios/04-byproduct-cycle/pack",
        ])
        .output()
        .expect("runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "failed:\n{stdout}");
    assert!(stdout.contains("160.00/min"), "got:\n{stdout}");
}
