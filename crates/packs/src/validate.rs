//! Checking that a merged registry hangs together.

use std::collections::BTreeSet;

use model::ResourceRef;

use crate::registry::{Registry, compile_formulas};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The data is wrong and whatever depends on it will not work.
    Error,
    /// Suspicious, but usable — a partial pack looks like this.
    Warning,
}

/// Something worth telling the user about one entity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// What the issue is about, e.g. `recipe:minecraft:smelting/iron`.
    pub where_: String,
    pub message: String,
}

impl Issue {
    pub fn error(where_: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            where_: where_.into(),
            message: message.into(),
        }
    }

    pub fn warning(where_: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            where_: where_.into(),
            message: message.into(),
        }
    }

    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error)
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{label}: {}: {}", self.where_, self.message)
    }
}

/// Checks references, numbers and formulas, filling in the registry's
/// compiled formulas as it goes.
pub(crate) fn validate(registry: &mut Registry) -> Vec<Issue> {
    let mut issues = Vec::new();

    check_tags(registry, &mut issues);
    check_recipes(registry, &mut issues);
    check_categories(registry, &mut issues);

    // Formulas are compiled here rather than at deserialisation time because
    // they need the environment, which only exists once every pack's
    // functions.json has been merged.
    let mut formulas = std::collections::BTreeMap::new();
    for (id, machine) in &registry.machines {
        formulas.insert(
            id.clone(),
            compile_formulas(machine, &registry.env, &mut issues),
        );
    }
    registry.formulas = formulas;

    issues
}

fn check_tags(registry: &Registry, issues: &mut Vec<Issue>) {
    for (id, tag) in &registry.tags {
        for value in &tag.values {
            if !registry.resources.contains_key(value) {
                issues.push(Issue::error(
                    format!("tag:{id}"),
                    format!("lists `{value}`, which no pack defines"),
                ));
            }
        }
        if tag.values.is_empty() {
            issues.push(Issue::warning(format!("tag:{id}"), "is empty"));
        }
    }
}

fn check_recipes(registry: &Registry, issues: &mut Vec<Issue>) {
    for (id, recipe) in &registry.recipes {
        let where_ = format!("recipe:{id}");

        if recipe.duration.0 <= 0.0 {
            issues.push(Issue::error(
                &where_,
                format!("duration must be positive, got {}", recipe.duration.0),
            ));
        }

        for reference in recipe.referenced_resources() {
            check_reference(registry, &where_, reference, issues);
        }

        for ingredient in recipe.inputs.iter().chain(&recipe.catalysts) {
            if ingredient.amount <= 0.0 {
                issues.push(Issue::error(
                    &where_,
                    format!(
                        "input `{}` has a non-positive amount {}",
                        ingredient.resource, ingredient.amount
                    ),
                ));
            }
        }

        for output in &recipe.outputs {
            if output.resource.is_tag() {
                issues.push(Issue::error(
                    &where_,
                    format!(
                        "output `{}` is a tag; an output must name one resource",
                        output.resource
                    ),
                ));
            }
            if output.amount <= 0.0 {
                issues.push(Issue::error(
                    &where_,
                    format!(
                        "output `{}` has a non-positive amount {}",
                        output.resource, output.amount
                    ),
                ));
            }
            if !(0.0..=1.0).contains(&output.chance) {
                issues.push(Issue::error(
                    &where_,
                    format!(
                        "output `{}` has chance {}, which is outside 0..=1",
                        output.resource, output.chance
                    ),
                ));
            }
        }

        if recipe.outputs.is_empty() {
            issues.push(Issue::warning(&where_, "produces nothing"));
        }
    }
}

fn check_reference(
    registry: &Registry,
    where_: &str,
    reference: &ResourceRef,
    issues: &mut Vec<Issue>,
) {
    match reference {
        ResourceRef::Resource(id) => {
            if !registry.resources.contains_key(id) {
                issues.push(Issue::error(
                    where_,
                    format!("refers to resource `{id}`, which no pack defines"),
                ));
            }
        }
        ResourceRef::Tag(id) => {
            if !registry.tags.contains_key(id) {
                issues.push(Issue::error(
                    where_,
                    format!("refers to tag `#{id}`, which no pack defines"),
                ));
            }
        }
    }
}

/// A category with recipes but no machine (or the other way round) usually
/// means a half-finished pack rather than broken data, so these are warnings.
fn check_categories(registry: &Registry, issues: &mut Vec<Issue>) {
    let machine_categories: BTreeSet<_> = registry
        .machines
        .values()
        .flat_map(|machine| machine.categories.iter())
        .collect();
    let recipe_categories: BTreeSet<_> = registry
        .recipes
        .values()
        .map(|recipe| &recipe.category)
        .collect();

    for (id, recipe) in &registry.recipes {
        if !machine_categories.contains(&recipe.category) {
            issues.push(Issue::warning(
                format!("recipe:{id}"),
                format!(
                    "category `{}` has no machine that can run it",
                    recipe.category
                ),
            ));
        }
    }

    for (id, machine) in &registry.machines {
        for category in &machine.categories {
            if !recipe_categories.contains(category) {
                issues.push(Issue::warning(
                    format!("machine:{id}"),
                    format!("category `{category}` has no recipes"),
                ));
            }
        }
    }
}
