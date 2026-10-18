//! Turning a schema into processes (DESIGN.md §6.3).

use graph::{NodeKind, Schema};
use model::{Accuracy, Catalog, Issue, ResourceRef};

use crate::process::{Process, ProcessRole, Term};

/// The name of the machine formula that gives the real duration of a craft.
/// Without it the recipe's own duration is used unchanged.
pub const DURATION_FORMULA: &str = "duration";

#[derive(Debug, Default)]
pub struct Reduced {
    pub processes: Vec<Process>,
    pub issues: Vec<Issue>,
}

/// Reduces every node to processes, skipping the ones that carry no flow.
pub fn reduce(schema: &Schema, catalog: &dyn Catalog) -> Reduced {
    let mut out = Reduced::default();

    for node in &schema.nodes {
        let where_ = format!("node:{}", node.id);

        match &node.kind {
            NodeKind::Machine(machine_node) => {
                let (Some(machine), Some(recipe)) = (
                    catalog.machine(&machine_node.machine),
                    catalog.recipe(&machine_node.recipe),
                ) else {
                    // validate() already said why; nothing to add here.
                    continue;
                };

                // The machine's `duration` formula turns the recipe's own
                // figure into the real one; upgrades live in there.
                let duration = match catalog.evaluate(
                    &machine_node.machine,
                    DURATION_FORMULA,
                    recipe,
                    &machine_node.params,
                ) {
                    Ok(Some(value)) => value,
                    Ok(None) => recipe.duration.0,
                    Err(error) => {
                        out.issues.push(Issue::error(&where_, error.to_string()));
                        continue;
                    }
                };

                if duration <= 0.0 {
                    out.issues.push(Issue::error(
                        &where_,
                        format!("duration works out to {duration}, which cannot be run"),
                    ));
                    continue;
                }

                let mut process = Process::new(node.id.clone(), ProcessRole::Machine);
                process.duration = Some(duration);
                process.accuracy = machine.accuracy.combine(recipe.accuracy);

                // Catalysts are needed but not consumed, so they place no
                // demand on the flow; they are reported separately instead.
                process.inputs = recipe
                    .inputs
                    .iter()
                    .map(|input| Term::new(input.resource.clone(), input.amount))
                    .collect();

                // A chance output contributes its expected value (§4.4).
                process.outputs = recipe
                    .outputs
                    .iter()
                    .map(|output| Term::new(output.resource.clone(), output.expected_amount()))
                    .collect();

                // Activity is activations per tick: one machine manages
                // 1/duration of them, so `count` machines manage count/duration.
                process.max_activity = machine_node.count.map(|count| count / duration);

                out.processes.push(process);
            }

            NodeKind::Source(source) => {
                let mut process = Process::new(node.id.clone(), ProcessRole::Source);
                process.outputs = vec![Term::new(
                    ResourceRef::Resource(source.resource.clone()),
                    1.0,
                )];
                process.max_activity = source.rate.map(|rate| rate.rate().as_per_tick());
                // Anything standing in for the world outside the schema is a
                // guess about the world outside the schema.
                process.accuracy = Accuracy::Estimated;
                out.processes.push(process);
            }

            NodeKind::Output(output) => {
                let mut process = Process::new(node.id.clone(), ProcessRole::Output);
                process.inputs = vec![Term::new(
                    ResourceRef::Resource(output.resource.clone()),
                    1.0,
                )];
                // A target is a ceiling on demand, not a promise: the solver
                // maximises up to it and a shortfall is reported, rather than
                // the whole problem becoming infeasible.
                process.max_activity = output.target.map(|target| target.rate().as_per_tick());
                out.processes.push(process);
            }

            // A hub only balances what passes through, and a note carries
            // nothing at all.
            NodeKind::Hub(_) | NodeKind::Note(_) => {}
        }
    }

    if !out
        .processes
        .iter()
        .any(|process| process.role == ProcessRole::Output)
    {
        out.issues.push(Issue::warning(
            "schema",
            "there is no output node, so there is nothing to maximise",
        ));
    }

    out
}
