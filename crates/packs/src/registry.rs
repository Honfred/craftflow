//! Layering packs into one view of the world.
//!
//! Order follows DESIGN.md §5: later layers win. Within that, a pack is
//! always placed after everything it depends on, so a dependency's entities
//! are in place before anything edits them.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use expr::{Environment, Expr, FunctionDef};
use model::{Id, Machine, Recipe, Resource, Tag};
use serde_json::Value;

use crate::error::PackError;
use crate::manifest::PackManifest;
use crate::overlay::{EntityKind, EntityRef, Operation, remove_path, set_path};
use crate::raw::{Entry, RawPack};
use model::Issue;

use crate::validate::validate;

/// Everything the rest of the application reads pack data through.
#[derive(Debug, Default)]
pub struct Registry {
    pub resources: BTreeMap<Id, Resource>,
    pub tags: BTreeMap<Id, Tag>,
    pub machines: BTreeMap<Id, Machine>,
    pub recipes: BTreeMap<Id, Recipe>,
    /// Machine formulas, compiled once against [`Self::env`].
    pub formulas: BTreeMap<Id, BTreeMap<String, Expr>>,
    pub env: Environment,
    /// The packs that went into this, in the order they were applied.
    pub packs: Vec<PackManifest>,
}

/// A registry plus whatever was wrong with it.
///
/// Loading succeeds even with issues, so that one bad recipe does not hide
/// everything else; the caller decides whether errors are fatal.
#[derive(Debug)]
pub struct Loaded {
    pub registry: Registry,
    pub issues: Vec<Issue>,
}

impl Loaded {
    /// True when nothing worse than a warning turned up.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        !self.issues.iter().any(Issue::is_error)
    }

    pub fn errors(&self) -> impl Iterator<Item = &Issue> {
        self.issues.iter().filter(|issue| issue.is_error())
    }
}

impl Registry {
    /// Resolves a tag to the resources it stands for; a plain resource
    /// reference stands for itself.
    ///
    /// The returned ids borrow from the registry, not from `reference`, which
    /// is why the resource arm hands back the map's own key rather than the
    /// one it was asked about: they are equal, but they do not live equally
    /// long, and the caller keeps these past the reference's lifetime.
    #[must_use]
    pub fn resolve<'a>(&'a self, reference: &model::ResourceRef) -> Vec<&'a Id> {
        match reference {
            model::ResourceRef::Resource(id) => self
                .resources
                .get_key_value(id)
                .map(|(key, _)| key)
                .into_iter()
                .collect(),
            model::ResourceRef::Tag(id) => self
                .tags
                .get(id)
                .map(|tag| tag.values.iter().collect())
                .unwrap_or_default(),
        }
    }

    /// Machines that can run `category`.
    pub fn machines_for(&self, category: &Id) -> impl Iterator<Item = &Machine> {
        self.machines
            .values()
            .filter(move |machine| machine.categories.contains(category))
    }
}

/// Builds a registry from packs that have already been read off disk.
pub fn build(packs: Vec<RawPack>) -> Result<Loaded, PackError> {
    let ordered = order_by_dependency(packs)?;

    // Merge raw JSON first; overlays need something to edit.
    let mut merged: BTreeMap<EntityKind, BTreeMap<Id, Entry>> = EntityKind::ALL
        .into_iter()
        .map(|kind| (kind, BTreeMap::new()))
        .collect();
    let mut issues = Vec::new();
    let mut functions: BTreeMap<String, FunctionDef> = BTreeMap::new();

    for pack in &ordered {
        for kind in EntityKind::ALL {
            if let (Some(incoming), Some(target)) =
                (pack.entities.get(&kind), merged.get_mut(&kind))
            {
                for (id, entry) in incoming {
                    target.insert(id.clone(), entry.clone());
                }
            }
        }

        // A later pack's function of the same name replaces the earlier one,
        // the same rule entities follow.
        for (name, def) in &pack.functions {
            functions.insert(name.clone(), def.clone());
        }

        apply_overlays(pack, &mut merged, &mut issues)?;
    }

    let env = build_environment(functions, &mut issues);
    let mut registry = deserialize(merged, &mut issues);
    registry.env = env;
    registry.packs = ordered.into_iter().map(|pack| pack.manifest).collect();

    issues.extend(validate(&mut registry));

    Ok(Loaded { registry, issues })
}

fn apply_overlays(
    pack: &RawPack,
    merged: &mut BTreeMap<EntityKind, BTreeMap<Id, Entry>>,
    issues: &mut Vec<Issue>,
) -> Result<(), PackError> {
    for (file, overlay) in &pack.overlays {
        for operation in &overlay.operations {
            match operation {
                Operation::Set {
                    target,
                    path,
                    value,
                } => {
                    let reference = EntityRef::parse(target, file)?;
                    let Some(entry) = entry_mut(merged, &reference) else {
                        issues.push(Issue::error(
                            target.clone(),
                            format!(
                                "overlay in {} edits `{target}`, which does not exist",
                                file.display()
                            ),
                        ));
                        continue;
                    };
                    if let Err(message) = set_path(&mut entry.value, path, value.clone()) {
                        issues.push(Issue::error(
                            target.clone(),
                            format!("overlay in {}: {message}", file.display()),
                        ));
                    }
                }

                Operation::Remove { target, path } => {
                    let reference = EntityRef::parse(target, file)?;
                    match path {
                        Some(path) => {
                            let Some(entry) = entry_mut(merged, &reference) else {
                                issues.push(Issue::error(
                                    target.clone(),
                                    format!(
                                        "overlay in {} edits `{target}`, which does not exist",
                                        file.display()
                                    ),
                                ));
                                continue;
                            };
                            if let Err(message) = remove_path(&mut entry.value, path) {
                                issues.push(Issue::error(
                                    target.clone(),
                                    format!("overlay in {}: {message}", file.display()),
                                ));
                            }
                        }
                        None => {
                            if let Some(map) = merged.get_mut(&reference.kind) {
                                map.remove(&reference.id);
                            }
                        }
                    }
                }

                Operation::Add { kind, value } => {
                    let Some(id) = value.get("id").and_then(Value::as_str) else {
                        return Err(PackError::Overlay {
                            path: file.clone(),
                            message: "an `add` operation has no `id` in its value".to_owned(),
                        });
                    };
                    let id: Id = id.parse().map_err(|e| PackError::Overlay {
                        path: file.clone(),
                        message: format!("{e}"),
                    })?;
                    if let Some(map) = merged.get_mut(kind) {
                        map.insert(
                            id,
                            Entry {
                                value: value.clone(),
                                pack: pack.manifest.id.clone(),
                                file: file.clone(),
                            },
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

fn entry_mut<'a>(
    merged: &'a mut BTreeMap<EntityKind, BTreeMap<Id, Entry>>,
    reference: &EntityRef,
) -> Option<&'a mut Entry> {
    merged.get_mut(&reference.kind)?.get_mut(&reference.id)
}

fn build_environment(
    functions: BTreeMap<String, FunctionDef>,
    issues: &mut Vec<Issue>,
) -> Environment {
    let mut env = Environment::with_builtins();
    let defs: Vec<FunctionDef> = functions.into_values().collect();

    if let Err(error) = env.define_functions(defs.clone()) {
        // One bad body would reject the whole batch, so fall back to adding
        // them one at a time and blame only the ones that deserve it.
        issues.push(Issue::error("functions.json".to_owned(), error.to_string()));

        env = Environment::with_builtins();
        for def in defs {
            let name = def.name.clone();
            if let Err(error) = env.define_functions([def]) {
                issues.push(Issue::error(
                    format!("function `{name}`"),
                    error.to_string(),
                ));
            }
        }
    }

    env
}

/// Turns the merged JSON into model types, blaming individual entities.
fn deserialize(
    merged: BTreeMap<EntityKind, BTreeMap<Id, Entry>>,
    issues: &mut Vec<Issue>,
) -> Registry {
    let mut registry = Registry::default();

    for (kind, entries) in merged {
        for (id, entry) in entries {
            let where_ = format!("{kind}:{id}");
            let blame = |issues: &mut Vec<Issue>, error: serde_json::Error| {
                issues.push(Issue::error(
                    where_.clone(),
                    format!("{} ({})", error, entry.file.display()),
                ));
            };

            match kind {
                EntityKind::Resource => {
                    match serde_json::from_value::<Resource>(entry.value.clone()) {
                        Ok(value) => {
                            registry.resources.insert(id, value);
                        }
                        Err(error) => blame(issues, error),
                    }
                }
                EntityKind::Tag => match serde_json::from_value::<Tag>(entry.value.clone()) {
                    Ok(value) => {
                        registry.tags.insert(id, value);
                    }
                    Err(error) => blame(issues, error),
                },
                EntityKind::Machine => {
                    match serde_json::from_value::<Machine>(entry.value.clone()) {
                        Ok(value) => {
                            registry.machines.insert(id, value);
                        }
                        Err(error) => blame(issues, error),
                    }
                }
                EntityKind::Recipe => match serde_json::from_value::<Recipe>(entry.value.clone()) {
                    Ok(value) => {
                        registry.recipes.insert(id, value);
                    }
                    Err(error) => blame(issues, error),
                },
            }
        }
    }

    registry
}

/// Dependency-first ordering, keeping the caller's order where it is free to.
fn order_by_dependency(packs: Vec<RawPack>) -> Result<Vec<RawPack>, PackError> {
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (index, pack) in packs.iter().enumerate() {
        if by_id.insert(pack.manifest.id.clone(), index).is_some() {
            return Err(PackError::DuplicatePack {
                id: pack.manifest.id.clone(),
            });
        }
    }

    // Check every dependency exists and its version fits before sorting.
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); packs.len()];
    for (index, pack) in packs.iter().enumerate() {
        let manifest_path = pack.root.join("pack.toml");
        for dependency in pack.manifest.dependencies(&manifest_path)? {
            let Some(&other) = by_id.get(&dependency.id) else {
                return Err(PackError::MissingDependency {
                    pack: pack.manifest.id.clone(),
                    dependency: dependency.source.clone(),
                });
            };

            let other_pack = &packs[other];
            let found = other_pack
                .manifest
                .semver(&other_pack.root.join("pack.toml"))?;
            if !dependency.requirement.matches(&found) {
                return Err(PackError::UnsatisfiedDependency {
                    pack: pack.manifest.id.clone(),
                    dependency: dependency.id.clone(),
                    requirement: dependency.requirement.to_string(),
                    found: found.to_string(),
                });
            }

            edges[index].push(other);
        }
    }

    // Depth-first post-order: a pack is emitted once its dependencies are.
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InProgress,
        Done,
    }

    let mut marks = vec![Mark::Unvisited; packs.len()];
    let mut order = Vec::with_capacity(packs.len());

    for start in 0..packs.len() {
        // An explicit stack, so a deep dependency chain cannot blow the real
        // one. Each frame remembers how many of its edges are handled.
        let mut stack = vec![(start, 0usize)];

        while let Some(&mut (node, ref mut cursor)) = stack.last_mut() {
            match marks[node] {
                Mark::Done => {
                    stack.pop();
                    continue;
                }
                Mark::Unvisited => marks[node] = Mark::InProgress,
                Mark::InProgress => {}
            }

            if let Some(&next) = edges[node].get(*cursor) {
                *cursor += 1;
                match marks[next] {
                    Mark::Done => {}
                    Mark::InProgress => {
                        let mut cycle: Vec<String> = stack
                            .iter()
                            .skip_while(|&&(n, _)| n != next)
                            .map(|&(n, _)| packs[n].manifest.id.clone())
                            .collect();
                        cycle.push(packs[next].manifest.id.clone());
                        return Err(PackError::DependencyCycle { cycle });
                    }
                    Mark::Unvisited => stack.push((next, 0)),
                }
            } else {
                marks[node] = Mark::Done;
                order.push(node);
                stack.pop();
            }
        }
    }

    // `order` is in post-order, which is already dependencies-first.
    let mut packs: Vec<Option<RawPack>> = packs.into_iter().map(Some).collect();
    let mut sorted = Vec::with_capacity(order.len());
    for index in order {
        if let Some(pack) = packs[index].take() {
            sorted.push(pack);
        }
    }

    Ok(sorted)
}

/// Variables a machine formula may read beyond its own params and constants.
pub const RECIPE_FIELDS: &[&str] = &["recipe.duration"];

/// Compiles a machine's formulas, reporting anything that will not work.
pub(crate) fn compile_formulas(
    machine: &Machine,
    env: &Environment,
    issues: &mut Vec<Issue>,
) -> BTreeMap<String, Expr> {
    let mut known: BTreeSet<&str> = machine.params.keys().map(String::as_str).collect();
    known.extend(machine.constants.keys().map(String::as_str));
    known.extend(RECIPE_FIELDS.iter().copied());

    let mut compiled = BTreeMap::new();
    for (name, source) in &machine.formulas {
        match expr::compile(source, env) {
            Ok(formula) => {
                for variable in formula.variables() {
                    if !known.contains(variable) {
                        let mut options: Vec<&str> = known.iter().copied().collect();
                        options.sort_unstable();
                        issues.push(Issue::error(
                            format!("machine:{}", machine.id),
                            format!(
                                "formula `{name}` reads `{variable}`, which is not a parameter, \
                                 a constant or a recipe field (available: {})",
                                options.join(", ")
                            ),
                        ));
                    }
                }
                compiled.insert(name.clone(), formula);
            }
            Err(error) => issues.push(Issue::error(
                format!("machine:{}", machine.id),
                format!("formula `{name}`: {}", error.report(source)),
            )),
        }
    }

    compiled
}

/// Lets `graph` and `solver` read pack data without depending on this crate
/// (DESIGN.md §3): the trait lives in `model`, which all three share.
impl model::Catalog for Registry {
    fn resource(&self, id: &Id) -> Option<&model::Resource> {
        self.resources.get(id)
    }

    fn tag(&self, id: &Id) -> Option<&model::Tag> {
        self.tags.get(id)
    }

    fn machine(&self, id: &Id) -> Option<&Machine> {
        self.machines.get(id)
    }

    fn recipe(&self, id: &Id) -> Option<&Recipe> {
        self.recipes.get(id)
    }

    fn evaluate(
        &self,
        machine: &Id,
        formula: &str,
        recipe: &Recipe,
        params: &BTreeMap<String, model::ParamValue>,
    ) -> Result<Option<f64>, model::CatalogError> {
        let Some(machine_def) = self.machines.get(machine) else {
            return Err(model::CatalogError::NoMachine {
                machine: machine.clone(),
            });
        };
        let Some(compiled) = self.formulas.get(machine).and_then(|f| f.get(formula)) else {
            return Ok(None);
        };

        // Three sources, consulted in order: what the user set, the machine's
        // constants, and the recipe's own fields.
        let values: BTreeMap<String, f64> = params
            .iter()
            .map(|(name, value)| (name.clone(), value.as_f64()))
            .collect();
        let defaults: BTreeMap<String, f64> = machine_def
            .default_params()
            .iter()
            .map(|(name, value)| (name.clone(), value.as_f64()))
            .collect();
        let recipe_fields: BTreeMap<String, f64> =
            [("recipe.duration".to_owned(), recipe.duration.0)]
                .into_iter()
                .collect();

        let scope = expr::Scope::new()
            .with(&values)
            .with(&defaults)
            .with(&machine_def.constants)
            .with(&recipe_fields);

        compiled
            .eval(&self.env, &scope)
            .map(Some)
            .map_err(|error| model::CatalogError::Eval {
                machine: machine.clone(),
                formula: formula.to_owned(),
                message: error.to_string(),
            })
    }
}
