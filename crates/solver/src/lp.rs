//! Building and solving the linear programme (DESIGN.md §6.4).
//!
//! Variables: one flow per link, one activity per process, and — for a recipe
//! input written as a tag — one "pull" per resource the tag stands for, so the
//! solver can decide which of them to actually eat.
//!
//! Constraints: at every node and for every resource, what leaves cannot
//! exceed what arrives. The inequality is deliberate: the difference is
//! surplus, which §6.4 asks to be shown rather than forbidden.
//!
//! Objective: the sum of the output nodes' activities.

use std::collections::{BTreeMap, BTreeSet};

use good_lp::{
    Expression, ProblemVariables, Solution, SolverModel, Variable, default_solver, variable,
};
use graph::{NodeId, Schema};
use model::{Accuracy, Catalog, Id, Issue, Rate, ResourceRef, approx_eq};

use crate::error::SolveError;
use crate::process::{Process, ProcessRole};
use crate::reduce::reduce;
use crate::report::{LinkFlow, MachineLoad, NetworkBalance, OutputResult, Report, Surplus};

/// Suffix of the machine formula that feeds a named network: a machine on the
/// `energy` role is read from its `energy_per_tick` formula.
pub const NETWORK_FORMULA_SUFFIX: &str = "_per_tick";

/// Below this a computed flow is treated as zero, so that solver noise does
/// not show up as a surplus of 10^-15 items (rule 4).
const NOISE: f64 = 1e-9;

/// Solves `schema` against the loaded packs.
pub fn solve(schema: &Schema, catalog: &dyn Catalog) -> Result<Report, SolveError> {
    let reduced = reduce(schema, catalog);
    let processes = reduced.processes;

    let mut vars = ProblemVariables::new();

    // One variable per link, capped by its capacity when it has one.
    let flows: Vec<Variable> = schema
        .links
        .iter()
        .map(|link| {
            let mut definition = variable().min(0.0);
            if let Some(capacity) = link.capacity {
                definition = definition.max(capacity.rate().as_per_tick());
            }
            vars.add(definition)
        })
        .collect();

    // One variable per process.
    let activities: Vec<Variable> = processes
        .iter()
        .map(|process| {
            let mut definition = variable().min(process.min_activity);
            if let Some(max) = process.max_activity {
                definition = definition.max(max);
            }
            vars.add(definition)
        })
        .collect();

    // One variable per (process, tag input, candidate resource).
    let mut pulls: Vec<BTreeMap<(usize, Id), Variable>> = Vec::with_capacity(processes.len());
    for process in &processes {
        let mut per_process = BTreeMap::new();
        for (index, input) in process.inputs.iter().enumerate() {
            if let ResourceRef::Tag(tag) = &input.resource {
                for candidate in tag_members(catalog, tag) {
                    per_process.insert((index, candidate), vars.add(variable().min(0.0)));
                }
            }
        }
        pulls.push(per_process);
    }

    // Maximise what the output nodes take in.
    let mut objective = Expression::from(0.0);
    for (index, process) in processes.iter().enumerate() {
        if process.role == ProcessRole::Output {
            objective += activities[index];
        }
    }

    let mut model = vars.maximise(objective).using(default_solver);

    // A tag input must be satisfied from the resources the tag stands for.
    for (index, process) in processes.iter().enumerate() {
        for (input_index, input) in process.inputs.iter().enumerate() {
            if !matches!(input.resource, ResourceRef::Tag(_)) {
                continue;
            }
            let mut chosen = Expression::from(0.0);
            for ((i, _), variable) in &pulls[index] {
                if *i == input_index {
                    chosen += *variable;
                }
            }
            model = model.with(chosen.eq(Expression::from(activities[index]) * input.amount));
        }
    }

    // Balance, per node and resource.
    for (node, resource) in balance_keys(schema, catalog, &processes) {
        let mut available = Expression::from(0.0);

        for (index, link) in schema.links.iter().enumerate() {
            if link.resource != resource {
                continue;
            }
            if link.to == node {
                available += flows[index];
            }
            if link.from == node {
                available -= flows[index];
            }
        }

        for (index, process) in processes.iter().enumerate() {
            if process.node != node {
                continue;
            }
            available += produced(process, &resource) * activities[index];
            available -= consumed(process, &resource, &pulls[index], activities[index]);
        }

        model = model.with(available.geq(0.0));
    }

    let solution = model.solve()?;

    // Read the tag decisions back as plain numbers: the report has to
    // subtract exactly what the solver chose to eat.
    let pull_values: Vec<BTreeMap<(usize, Id), f64>> = pulls
        .iter()
        .map(|per_process| {
            per_process
                .iter()
                .map(|(key, variable)| (key.clone(), clean(solution.value(*variable))))
                .collect()
        })
        .collect();

    Ok(build_report(
        schema,
        catalog,
        &processes,
        &flows,
        &activities,
        &pull_values,
        &solution,
        reduced.issues,
    ))
}

/// Every (node, resource) pair that needs a balance constraint.
fn balance_keys(
    schema: &Schema,
    catalog: &dyn Catalog,
    processes: &[Process],
) -> BTreeSet<(NodeId, Id)> {
    let mut keys = BTreeSet::new();

    for link in &schema.links {
        keys.insert((link.from.clone(), link.resource.clone()));
        keys.insert((link.to.clone(), link.resource.clone()));
    }

    for process in processes {
        for term in process.inputs.iter().chain(&process.outputs) {
            match &term.resource {
                ResourceRef::Resource(id) => {
                    keys.insert((process.node.clone(), id.clone()));
                }
                ResourceRef::Tag(tag) => {
                    for member in tag_members(catalog, tag) {
                        keys.insert((process.node.clone(), member));
                    }
                }
            }
        }
    }

    keys
}

fn tag_members(catalog: &dyn Catalog, tag: &Id) -> Vec<Id> {
    catalog
        .tag(tag)
        .map(|tag| tag.values.clone())
        .unwrap_or_default()
}

/// How much of `resource` one activation produces.
fn produced(process: &Process, resource: &Id) -> f64 {
    process
        .outputs
        .iter()
        .filter(|term| matches!(&term.resource, ResourceRef::Resource(id) if id == resource))
        .map(|term| term.amount)
        .sum()
}

/// What one process consumes of `resource`: fixed for a concrete input, and a
/// decision for a tag one.
fn consumed(
    process: &Process,
    resource: &Id,
    pulls: &BTreeMap<(usize, Id), Variable>,
    activity: Variable,
) -> Expression {
    let mut total = Expression::from(0.0);

    for (index, input) in process.inputs.iter().enumerate() {
        match &input.resource {
            ResourceRef::Resource(id) if id == resource => {
                total += Expression::from(activity) * input.amount;
            }
            ResourceRef::Tag(_) => {
                if let Some(variable) = pulls.get(&(index, resource.clone())) {
                    total += *variable;
                }
            }
            ResourceRef::Resource(_) => {}
        }
    }

    total
}

fn clean(value: f64) -> f64 {
    if value.abs() < NOISE { 0.0 } else { value }
}

#[allow(clippy::too_many_arguments)]
fn build_report(
    schema: &Schema,
    catalog: &dyn Catalog,
    processes: &[Process],
    flows: &[Variable],
    activities: &[Variable],
    pulls: &[BTreeMap<(usize, Id), f64>],
    solution: &impl Solution,
    mut issues: Vec<Issue>,
) -> Report {
    let mut report = Report {
        accuracy: Accuracy::Exact,
        ..Report::default()
    };

    let flow_values: Vec<f64> = flows.iter().map(|v| clean(solution.value(*v))).collect();
    let activity_values: Vec<f64> = activities
        .iter()
        .map(|v| clean(solution.value(*v)))
        .collect();

    for (index, link) in schema.links.iter().enumerate() {
        let rate = flow_values[index];
        report.links.push(LinkFlow {
            index,
            from: link.from.clone(),
            to: link.to.clone(),
            resource: link.resource.clone(),
            rate: Rate::per_tick(rate),
            at_capacity: link
                .capacity
                .is_some_and(|cap| approx_eq(cap.rate().as_per_tick(), rate)),
        });
    }

    for (index, process) in processes.iter().enumerate() {
        let activity = activity_values[index];
        report.accuracy = report.accuracy.combine(process.accuracy);

        match process.role {
            ProcessRole::Machine => {
                let Some(node) = schema.node(&process.node).and_then(graph::Node::machine) else {
                    continue;
                };
                let running = process.running_machines(activity).unwrap_or(activity);
                report.machines.push(MachineLoad {
                    node: process.node.clone(),
                    machine: node.machine.clone(),
                    recipe: node.recipe.clone(),
                    activity,
                    running,
                    count: node.count,
                    load: node
                        .count
                        .filter(|count| *count > 0.0)
                        .map(|count| running / count),
                });

                if let Some(recipe) = catalog.recipe(&node.recipe) {
                    for catalyst in &recipe.catalysts {
                        if activity > 0.0 {
                            *report
                                .catalysts
                                .entry(catalyst.resource.id().clone())
                                .or_insert(0.0) += catalyst.amount;
                        }
                    }
                }
            }

            ProcessRole::Output => {
                let Some(graph::NodeKind::Output(output)) =
                    schema.node(&process.node).map(|node| &node.kind)
                else {
                    continue;
                };
                report.outputs.push(OutputResult {
                    node: process.node.clone(),
                    resource: output.resource.clone(),
                    achieved: Rate::per_tick(activity),
                    target: output.target.map(|target| target.rate()),
                });
            }

            ProcessRole::Source => {
                let Some(graph::NodeKind::Source(source)) =
                    schema.node(&process.node).map(|node| &node.kind)
                else {
                    continue;
                };
                // What a source really gave is what left it: its activity may
                // sit anywhere up to its rate, since nothing pushes it down.
                let shipped: f64 = schema
                    .links
                    .iter()
                    .enumerate()
                    .filter(|(_, link)| link.from == process.node)
                    .map(|(index, _)| flow_values[index])
                    .sum();
                report.sources.push(OutputResult {
                    node: process.node.clone(),
                    resource: source.resource.clone(),
                    achieved: Rate::per_tick(clean(shipped)),
                    target: source.rate.map(|rate| rate.rate()),
                });
            }
        }
    }

    report.surplus = surplus(
        schema,
        catalog,
        processes,
        &flow_values,
        &activity_values,
        pulls,
    );
    report.networks = networks(schema, catalog, processes, &activity_values, &mut issues);
    report.issues = issues;
    report
}

/// Material that reached a node and stayed there.
fn surplus(
    schema: &Schema,
    catalog: &dyn Catalog,
    processes: &[Process],
    flows: &[f64],
    activities: &[f64],
    pulls: &[BTreeMap<(usize, Id), f64>],
) -> Vec<Surplus> {
    let mut out = Vec::new();

    for (node, resource) in balance_keys(schema, catalog, processes) {
        let mut available = 0.0;

        for (index, link) in schema.links.iter().enumerate() {
            if link.resource != resource {
                continue;
            }
            if link.to == node {
                available += flows[index];
            }
            if link.from == node {
                available -= flows[index];
            }
        }

        for (index, process) in processes.iter().enumerate() {
            if process.node != node {
                continue;
            }
            available += produced(process, &resource) * activities[index];
            available -= consumed_value(process, &resource, &pulls[index], activities[index]);
        }

        let available = clean(available);
        if available > 0.0 {
            out.push(Surplus {
                node,
                resource,
                rate: Rate::per_tick(available),
            });
        }
    }

    out
}

/// The numeric twin of [`consumed`], for reading a solved problem back.
fn consumed_value(
    process: &Process,
    resource: &Id,
    pulls: &BTreeMap<(usize, Id), f64>,
    activity: f64,
) -> f64 {
    let mut total = 0.0;

    for (index, input) in process.inputs.iter().enumerate() {
        match &input.resource {
            ResourceRef::Resource(id) if id == resource => total += activity * input.amount,
            ResourceRef::Tag(_) => {
                if let Some(value) = pulls.get(&(index, resource.clone())) {
                    total += value;
                }
            }
            ResourceRef::Resource(_) => {}
        }
    }

    total
}

/// Generation against consumption on each named network.
///
/// A machine on the `energy` role is read from its `energy_per_tick` formula,
/// per running machine; a negative value means the machine generates.
fn networks(
    schema: &Schema,
    catalog: &dyn Catalog,
    processes: &[Process],
    activities: &[f64],
    issues: &mut Vec<Issue>,
) -> Vec<NetworkBalance> {
    let mut totals: BTreeMap<String, (f64, f64)> = BTreeMap::new();

    for (index, process) in processes.iter().enumerate() {
        let Some(node) = schema.node(&process.node).and_then(graph::Node::machine) else {
            continue;
        };
        let Some(recipe) = catalog.recipe(&node.recipe) else {
            continue;
        };
        let running = process.running_machines(activities[index]).unwrap_or(0.0);

        for (role, network) in &node.networks {
            let formula = format!("{role}{NETWORK_FORMULA_SUFFIX}");
            let value = match catalog.evaluate(&node.machine, &formula, recipe, &node.params) {
                Ok(Some(value)) => value,
                Ok(None) => {
                    issues.push(Issue::warning(
                        format!("node:{}", process.node),
                        format!(
                            "is on network `{network}` as `{role}`, but `{}` has no `{formula}` \
                             formula, so it counts as nothing",
                            node.machine
                        ),
                    ));
                    continue;
                }
                Err(error) => {
                    issues.push(Issue::error(
                        format!("node:{}", process.node),
                        error.to_string(),
                    ));
                    continue;
                }
            };

            let entry = totals.entry(network.clone()).or_insert((0.0, 0.0));
            let draw = value * running;
            if draw >= 0.0 {
                entry.1 += draw;
            } else {
                entry.0 -= draw;
            }
        }
    }

    schema
        .networks
        .iter()
        .map(|(name, network)| {
            let (produced, consumed) = totals.get(name).copied().unwrap_or((0.0, 0.0));
            NetworkBalance {
                name: name.clone(),
                kind: network.kind.clone(),
                produced: Rate::per_tick(clean(produced)),
                consumed: Rate::per_tick(clean(consumed)),
            }
        })
        .collect()
}
