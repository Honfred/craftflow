//! Checking a schema against the packs it was drawn with.

use std::collections::{BTreeMap, BTreeSet};

use model::{Catalog, Id, Issue, ResourceRef};

use crate::node::{Link, Node, NodeKind};
use crate::schema::Schema;

/// Reports everything wrong with `schema`, given the loaded packs.
///
/// Nothing here stops the solver from running: a schema with problems still
/// produces an answer for the parts that are sound, and the issues are shown
/// alongside it.
pub fn validate(schema: &Schema, catalog: &dyn Catalog) -> Vec<Issue> {
    let mut issues = Vec::new();

    let ids = check_unique_ids(schema, &mut issues);
    check_nodes(schema, catalog, &mut issues);
    check_links(schema, catalog, &ids, &mut issues);

    issues
}

fn check_unique_ids(schema: &Schema, issues: &mut Vec<Issue>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    for node in &schema.nodes {
        if node.id.is_empty() {
            issues.push(Issue::error("node", "a node has an empty id"));
            continue;
        }
        if !seen.insert(node.id.to_string()) {
            issues.push(Issue::error(
                format!("node:{}", node.id),
                "two nodes share this id",
            ));
        }
    }
    seen
}

fn check_nodes(schema: &Schema, catalog: &dyn Catalog, issues: &mut Vec<Issue>) {
    for node in &schema.nodes {
        let where_ = format!("node:{}", node.id);

        match &node.kind {
            NodeKind::Machine(machine_node) => {
                let Some(machine) = catalog.machine(&machine_node.machine) else {
                    issues.push(Issue::error(
                        &where_,
                        format!("machine `{}` is not in any pack", machine_node.machine),
                    ));
                    continue;
                };

                let Some(recipe) = catalog.recipe(&machine_node.recipe) else {
                    issues.push(Issue::error(
                        &where_,
                        format!("recipe `{}` is not in any pack", machine_node.recipe),
                    ));
                    continue;
                };

                if !machine.categories.contains(&recipe.category) {
                    issues.push(Issue::error(
                        &where_,
                        format!(
                            "machine `{}` cannot run `{}`: the recipe is `{}` and the machine does \
                             {}",
                            machine.id,
                            recipe.id,
                            recipe.category,
                            list(machine.categories.iter().map(ToString::to_string))
                        ),
                    ));
                }

                check_params(&where_, machine, machine_node, issues);
                check_networks(schema, &where_, machine, machine_node, issues);

                if let Some(count) = machine_node.count
                    && count < 0.0
                {
                    issues.push(Issue::error(
                        &where_,
                        format!("machine count {count} is negative"),
                    ));
                }
            }

            NodeKind::Source(source) => {
                check_resource(catalog, &where_, &source.resource, issues);
                if let Some(rate) = source.rate
                    && rate.amount < 0.0
                {
                    issues.push(Issue::error(&where_, "source rate is negative"));
                }
            }

            NodeKind::Output(output) => {
                check_resource(catalog, &where_, &output.resource, issues);
                if let Some(target) = output.target
                    && target.amount < 0.0
                {
                    issues.push(Issue::error(&where_, "output target is negative"));
                }
            }

            NodeKind::Hub(_) | NodeKind::Note(_) => {}
        }
    }
}

fn check_resource(catalog: &dyn Catalog, where_: &str, id: &Id, issues: &mut Vec<Issue>) {
    if catalog.resource(id).is_none() {
        issues.push(Issue::error(
            where_,
            format!("resource `{id}` is not in any pack"),
        ));
    }
}

fn check_params(
    where_: &str,
    machine: &model::Machine,
    node: &crate::node::MachineNode,
    issues: &mut Vec<Issue>,
) {
    for (name, value) in &node.params {
        let Some(spec) = machine.params.get(name) else {
            issues.push(Issue::error(
                where_,
                format!(
                    "`{name}` is not a parameter of `{}` ({})",
                    machine.id,
                    list(machine.params.keys().cloned())
                ),
            ));
            continue;
        };
        if let Err(error) = spec.validate(name, value) {
            issues.push(Issue::error(where_, error.to_string()));
        }
    }
}

fn check_networks(
    schema: &Schema,
    where_: &str,
    machine: &model::Machine,
    node: &crate::node::MachineNode,
    issues: &mut Vec<Issue>,
) {
    for (role, name) in &node.networks {
        let Some(expected) = machine.networks.get(role) else {
            issues.push(Issue::error(
                where_,
                format!("machine `{}` has no `{role}` network", machine.id),
            ));
            continue;
        };
        let Some(network) = schema.networks.get(name) else {
            issues.push(Issue::error(
                where_,
                format!("`{role}` points at network `{name}`, which the schema does not define"),
            ));
            continue;
        };
        if &network.kind != expected {
            issues.push(Issue::error(
                where_,
                format!(
                    "`{role}` points at network `{name}`, which is `{}`, but the machine needs \
                     `{expected}`",
                    network.kind
                ),
            ));
        }
    }
}

fn check_links(
    schema: &Schema,
    catalog: &dyn Catalog,
    ids: &BTreeSet<String>,
    issues: &mut Vec<Issue>,
) {
    // Which resources each node can hand over or take, where that is knowable.
    let mut produces: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();
    let mut consumes: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();

    for node in &schema.nodes {
        let key = node.id.to_string();
        match &node.kind {
            NodeKind::Machine(machine_node) => {
                let Some(recipe) = catalog.recipe(&machine_node.recipe) else {
                    continue;
                };
                produces.insert(
                    key.clone(),
                    recipe
                        .outputs
                        .iter()
                        .map(|output| output.resource.id().clone())
                        .collect(),
                );
                consumes.insert(
                    key,
                    recipe
                        .inputs
                        .iter()
                        .chain(&recipe.catalysts)
                        .flat_map(|input| expand(catalog, &input.resource))
                        .collect(),
                );
            }
            NodeKind::Source(source) => {
                produces.insert(key, [source.resource.clone()].into_iter().collect());
            }
            NodeKind::Output(output) => {
                consumes.insert(key, [output.resource.clone()].into_iter().collect());
            }
            // A hub passes anything through, so nothing to check.
            NodeKind::Hub(_) => {}
            NodeKind::Note(_) => {}
        }
    }

    for link in &schema.links {
        let where_ = format!("link:{}->{}", link.from, link.to);

        let from_known = check_endpoint(ids, &where_, "from", &link.from, issues);
        let to_known = check_endpoint(ids, &where_, "to", &link.to, issues);

        if catalog.resource(&link.resource).is_none() {
            issues.push(Issue::error(
                &where_,
                format!("carries `{}`, which is not in any pack", link.resource),
            ));
            continue;
        }

        if from_known
            && let Some(outputs) = produces.get(link.from.as_str())
            && !outputs.contains(&link.resource)
        {
            issues.push(Issue::error(
                &where_,
                format!(
                    "`{}` does not produce `{}`{}",
                    link.from,
                    link.resource,
                    recipe_hint(schema, link)
                ),
            ));
        }

        if to_known
            && let Some(inputs) = consumes.get(link.to.as_str())
            && !inputs.contains(&link.resource)
        {
            issues.push(Issue::error(
                &where_,
                format!("`{}` does not consume `{}`", link.to, link.resource),
            ));
        }

        if let Some(capacity) = link.capacity
            && capacity.amount < 0.0
        {
            issues.push(Issue::error(&where_, "capacity is negative"));
        }
    }

    check_decorative_links(schema, issues);
}

/// Notes are decoration, so a link to one is a mistake rather than a flow.
fn check_decorative_links(schema: &Schema, issues: &mut Vec<Issue>) {
    for link in &schema.links {
        for (end, id) in [("from", &link.from), ("to", &link.to)] {
            if schema
                .node(id)
                .is_some_and(|node| node.kind.is_decorative())
            {
                issues.push(Issue::error(
                    format!("link:{}->{}", link.from, link.to),
                    format!("its `{end}` is a note, which carries nothing"),
                ));
            }
        }
    }
}

fn check_endpoint(
    ids: &BTreeSet<String>,
    where_: &str,
    end: &str,
    id: &crate::node::NodeId,
    issues: &mut Vec<Issue>,
) -> bool {
    if ids.contains(id.as_str()) {
        return true;
    }
    issues.push(Issue::error(
        where_,
        format!("its `{end}` is `{id}`, which is not a node in this schema"),
    ));
    false
}

/// A tag input accepts any of its resources.
fn expand(catalog: &dyn Catalog, reference: &ResourceRef) -> Vec<Id> {
    match reference {
        ResourceRef::Resource(id) => vec![id.clone()],
        ResourceRef::Tag(id) => catalog
            .tag(id)
            .map(|tag| tag.values.clone())
            .unwrap_or_default(),
    }
}

/// Switching a node's recipe is the usual way a link ends up carrying
/// something the node no longer handles, so the message points there.
fn recipe_hint(schema: &Schema, link: &Link) -> String {
    schema
        .node(&link.from)
        .and_then(Node::machine)
        .map_or_else(String::new, |machine| {
            format!(" (it runs `{}`)", machine.recipe)
        })
}

fn list(items: impl Iterator<Item = String>) -> String {
    let collected: Vec<String> = items.collect();
    if collected.is_empty() {
        "none".to_owned()
    } else {
        collected.join(", ")
    }
}
