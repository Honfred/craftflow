//! Layering, overlays, dependency resolution and validation.

use std::path::{Path, PathBuf};

use model::Id;
use packs::{PackError, Registry, load};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn id(text: &str) -> Id {
    text.parse().expect("valid id")
}

fn registry(names: &[&str]) -> Registry {
    let loaded = load(names.iter().map(|n| fixture(n))).expect("loads");
    assert!(
        loaded.is_clean(),
        "unexpected errors: {:#?}",
        loaded.errors().collect::<Vec<_>>()
    );
    loaded.registry
}

#[test]
fn a_later_pack_replaces_an_entity_wholesale() {
    let base = registry(&["base"]);
    assert_eq!(base.recipes[&id("test:smelt/ingot")].duration.0, 200.0);

    let both = registry(&["base", "addon"]);
    let recipe = &both.recipes[&id("test:smelt/ingot")];
    assert_eq!(recipe.duration.0, 100.0);
    assert_eq!(recipe.outputs[0].amount, 2.0);

    // The addon's own resource is there too, alongside the base ones.
    assert!(both.resources.contains_key(&id("test:dust")));
    assert!(both.resources.contains_key(&id("test:ore")));
}

#[test]
fn dependencies_are_applied_before_the_packs_that_need_them() {
    // Listed the wrong way round on purpose: `addon` depends on `base`, so it
    // must still end up on top.
    let registry = registry(&["addon", "base"]);
    assert_eq!(registry.recipes[&id("test:smelt/ingot")].duration.0, 100.0);

    let order: Vec<&str> = registry.packs.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(order, ["base", "addon"]);
}

#[test]
fn an_overlay_changes_one_field_without_restating_the_entity() {
    let registry = registry(&["base", "tweaks"]);

    let machine = &registry.machines[&id("test:furnace")];
    assert_eq!(machine.constants["base_energy"], 75.0);
    // Everything else about the machine survived the edit.
    assert_eq!(machine.categories, [id("test:smelting")]);
    assert_eq!(machine.formulas["duration"], "recipe.duration");

    // An array index works too.
    assert_eq!(
        registry.recipes[&id("test:smelt/ingot")].outputs[0].amount,
        3.0
    );

    // And `add` introduces something new.
    assert!(registry.resources.contains_key(&id("test:scrap")));
}

#[test]
fn an_overlay_can_remove_an_entity_or_one_field() {
    let registry = registry(&["base", "remover"]);
    assert!(!registry.recipes.contains_key(&id("test:smelt/ingot")));

    let machine = &registry.machines[&id("test:furnace")];
    assert!(!machine.formulas.contains_key("energy"));
    // The rest of the machine is untouched.
    assert!(machine.formulas.contains_key("duration"));
    assert_eq!(machine.constants["base_energy"], 50.0);
}

#[test]
fn removing_a_constant_a_formula_still_reads_is_caught() {
    let loaded = load([fixture("base"), fixture("orphanconst")]).expect("loads");
    assert!(!loaded.is_clean());
    assert!(
        loaded
            .errors()
            .any(|issue| issue.message.contains("base_energy")),
        "got {:#?}",
        loaded.issues
    );
}

#[test]
fn an_overlay_aimed_at_nothing_is_reported() {
    let loaded = load([fixture("base"), fixture("ghost")]).expect("loads");
    assert!(!loaded.is_clean());
    assert!(
        loaded
            .errors()
            .any(|issue| issue.message.contains("does not exist")),
        "got {:#?}",
        loaded.issues
    );
}

#[test]
fn unknown_references_are_reported_per_entity() {
    let loaded = load([fixture("badref")]).expect("loads");
    let messages: Vec<String> = loaded.errors().map(ToString::to_string).collect();

    assert!(
        messages.iter().any(|m| m.contains("`bad:missing`")),
        "got {messages:#?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("output") && m.contains("tag")),
        "got {messages:#?}"
    );
}

#[test]
fn broken_formulas_are_reported_with_the_offending_text() {
    let loaded = load([fixture("badformula")]).expect("loads");
    let messages: Vec<String> = loaded.errors().map(ToString::to_string).collect();

    assert!(
        messages.iter().any(|m| m.contains("mystery")),
        "got {messages:#?}"
    );
    // The report from `expr` is carried through verbatim, carets and all.
    assert!(
        messages
            .iter()
            .any(|m| m.contains("column") && m.contains('^')),
        "got {messages:#?}"
    );
}

#[test]
fn a_missing_dependency_stops_the_load() {
    let error = load([fixture("needsmissing")]).expect_err("rejects");
    assert!(
        matches!(&error, PackError::MissingDependency { dependency, .. } if dependency.contains("nosuch")),
        "got {error}"
    );
}

#[test]
fn a_version_that_does_not_fit_stops_the_load() {
    let error = load([fixture("base"), fixture("needsnewer")]).expect_err("rejects");
    assert!(
        matches!(error, PackError::UnsatisfiedDependency { .. }),
        "got {error}"
    );
}

#[test]
fn a_dependency_cycle_is_named() {
    let error = load([fixture("cyclea"), fixture("cycleb")]).expect_err("rejects");
    let PackError::DependencyCycle { cycle } = &error else {
        panic!("got {error}");
    };
    assert!(cycle.contains(&"cyclea".to_owned()) && cycle.contains(&"cycleb".to_owned()));
}

#[test]
fn two_packs_with_the_same_id_are_refused() {
    let error = load([fixture("base"), fixture("base")]).expect_err("rejects");
    assert!(
        matches!(error, PackError::DuplicatePack { .. }),
        "got {error}"
    );
}

#[test]
fn a_future_pack_format_is_refused_rather_than_guessed_at() {
    let error = load([fixture("futurefmt")]).expect_err("rejects");
    assert!(
        matches!(error, PackError::FormatVersion { found: 99, .. }),
        "got {error}"
    );
}

#[test]
fn machines_can_be_found_by_category() {
    let registry = registry(&["base"]);
    let found: Vec<String> = registry
        .machines_for(&id("test:smelting"))
        .map(|m| m.id.to_string())
        .collect();
    assert_eq!(found, ["test:furnace"]);
}
