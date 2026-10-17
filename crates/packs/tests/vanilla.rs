//! The hand-written vanilla pack must load clean.

use packs::{Severity, load};

fn vanilla_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/vanilla/packs/vanilla")
}

#[test]
fn the_vanilla_pack_loads_without_errors() {
    let loaded = load([vanilla_root()]).expect("loads");

    for issue in &loaded.issues {
        println!("{issue}");
    }
    assert!(
        loaded.is_clean(),
        "expected no errors, got: {:#?}",
        loaded.errors().collect::<Vec<_>>()
    );
}

#[test]
fn the_vanilla_pack_has_no_warnings_either() {
    let loaded = load([vanilla_root()]).expect("loads");
    let warnings: Vec<String> = loaded
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .map(ToString::to_string)
        .collect();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:#?}");
}

#[test]
fn it_contains_what_the_scenarios_need() {
    let loaded = load([vanilla_root()]).expect("loads");
    let registry = &loaded.registry;

    assert!(
        registry
            .machines
            .contains_key(&"minecraft:furnace".parse().expect("id"))
    );
    assert!(
        registry.recipes.contains_key(
            &"minecraft:smelting/iron_ingot_from_ore"
                .parse()
                .expect("id")
        )
    );

    // The iron ore tag resolves to both ore blocks.
    let ore: model::ResourceRef = "#c:ores/iron".parse().expect("ref");
    let resolved: Vec<String> = registry
        .resolve(&ore)
        .into_iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(
        resolved,
        ["minecraft:iron_ore", "minecraft:deepslate_iron_ore"]
    );

    // A plain resource resolves to itself.
    let ingot: model::ResourceRef = "minecraft:iron_ingot".parse().expect("ref");
    assert_eq!(registry.resolve(&ingot).len(), 1);
}

#[test]
fn machine_formulas_are_compiled_and_runnable() {
    use expr::{Scope, Variables};

    let loaded = load([vanilla_root()]).expect("loads");
    let registry = &loaded.registry;

    let blast: model::Id = "minecraft:blast_furnace".parse().expect("id");
    let formulas = &registry.formulas[&blast];

    let recipe: std::collections::BTreeMap<String, f64> = [("recipe.duration".to_owned(), 200.0)]
        .into_iter()
        .collect();
    let constants: std::collections::BTreeMap<String, f64> =
        [("coal_burn_time".to_owned(), 1600.0)]
            .into_iter()
            .collect();
    let scope = Scope::new()
        .with(&recipe as &dyn Variables)
        .with(&constants);

    // blasting_speed(base) = base / 2
    assert_eq!(
        formulas["duration"]
            .eval(&registry.env, &scope)
            .expect("evaluates"),
        100.0
    );
    // fuel_per_tick(burn_time) = 1 / burn_time
    assert_eq!(
        formulas["fuel_per_tick"]
            .eval(&registry.env, &scope)
            .expect("evaluates"),
        1.0 / 1600.0
    );
}
