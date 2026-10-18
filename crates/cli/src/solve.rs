//! The `solve` and `packs` commands.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, bail};
use graph::Schema;
use model::{Catalog, Rate, TimeUnit};
use solver::Report;

/// `--unit` as clap sees it.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum UnitArg {
    Tick,
    Second,
    Minute,
    Hour,
}

impl From<UnitArg> for TimeUnit {
    fn from(unit: UnitArg) -> Self {
        match unit {
            UnitArg::Tick => Self::Tick,
            UnitArg::Second => Self::Second,
            UnitArg::Minute => Self::Minute,
            UnitArg::Hour => Self::Hour,
        }
    }
}

/// Solves a schema and prints the result.
pub fn run(schema_path: &Path, pack_dirs: &[PathBuf], unit: TimeUnit) -> anyhow::Result<()> {
    let roots = resolve_packs(pack_dirs)?;
    let loaded = packs::load(&roots).context("loading packs")?;

    for issue in &loaded.issues {
        println!("{issue}");
    }
    if !loaded.is_clean() {
        bail!("the packs have errors, see above");
    }

    let text = std::fs::read_to_string(schema_path)
        .with_context(|| format!("reading {}", schema_path.display()))?;
    let schema =
        Schema::from_json(&text).with_context(|| format!("parsing {}", schema_path.display()))?;

    let catalog = &loaded.registry as &dyn Catalog;
    let problems = graph::validate(&schema, catalog);

    let started = Instant::now();
    let report = solver::solve(&schema, catalog)?;
    let elapsed = started.elapsed();

    print_header(schema_path, &schema, &report, elapsed);
    print_report(&report, unit);
    print_problems(&problems, &report);

    Ok(())
}

/// Loads packs and lists what they hold.
pub fn describe_packs(pack_dirs: &[PathBuf]) -> anyhow::Result<()> {
    let roots = resolve_packs(pack_dirs)?;
    let loaded = packs::load(&roots).context("loading packs")?;
    let registry = &loaded.registry;

    for manifest in &registry.packs {
        println!("{} {}", manifest.id, manifest.version);
    }
    println!();
    println!("resources {}", registry.resources.len());
    println!("tags      {}", registry.tags.len());
    println!("machines  {}", registry.machines.len());
    println!("recipes   {}", registry.recipes.len());

    if !loaded.issues.is_empty() {
        println!();
        for issue in &loaded.issues {
            println!("{issue}");
        }
    }

    Ok(())
}

/// Uses what was asked for, or finds the built-in packs below the working
/// directory.
fn resolve_packs(requested: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    if !requested.is_empty() {
        return Ok(requested.to_vec());
    }

    let mut found = Vec::new();
    let plugins = Path::new("plugins");
    if plugins.is_dir() {
        // plugins/<plugin>/packs/<pack>
        for plugin in read_dirs(plugins)? {
            let packs_dir = plugin.join("packs");
            if packs_dir.is_dir() {
                found.extend(read_dirs(&packs_dir)?);
            }
        }
    }
    found.sort();

    if found.is_empty() {
        bail!(
            "no packs given and none found under ./plugins; pass --pack <dir> or run from the \
             repository root"
        );
    }
    Ok(found)
}

fn read_dirs(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            out.push(path);
        }
    }
    Ok(out)
}

fn print_header(path: &Path, schema: &Schema, report: &Report, elapsed: std::time::Duration) {
    println!("{}", path.display());
    if !schema.packs.is_empty() {
        let names: Vec<String> = schema
            .packs
            .iter()
            .map(|pack| format!("{} {}", pack.id, pack.version))
            .collect();
        println!("packs: {}", names.join(", "));
    }
    let estimated = if report.accuracy.is_estimated() {
        "  (contains estimates)"
    } else {
        ""
    };
    println!(
        "solved in {:.1} ms{estimated}",
        elapsed.as_secs_f64() * 1000.0
    );
}

fn print_report(report: &Report, unit: TimeUnit) {
    let suffix = unit.suffix();
    let show = |rate: Rate| format!("{:.2}{suffix}", rate.as_per(unit));

    section("Outputs");
    if report.outputs.is_empty() {
        println!("  none");
    }
    for output in &report.outputs {
        let mut line = format!(
            "  {:<14} {:<32} {:>12}",
            output.node,
            output.resource,
            show(output.achieved)
        );
        if let Some(target) = output.target {
            let short = output.shortfall().unwrap_or(Rate::ZERO);
            if short.as_per_tick() > 0.0 {
                line.push_str(&format!(
                    "   target {}, short {}",
                    show(target),
                    show(short)
                ));
            } else {
                line.push_str(&format!("   target {} met", show(target)));
            }
        }
        println!("{line}");
    }

    section("Machines");
    if report.machines.is_empty() {
        println!("  none");
    }
    for machine in &report.machines {
        let load = match machine.load {
            Some(load) => format!("{:>6.1}%", load * 100.0),
            None => "     --".to_owned(),
        };
        let count = match machine.count {
            Some(count) => format!("{:.2}/{:.0}", machine.running, count),
            None => format!("{:.2}", machine.running),
        };
        let flag = if machine.is_bottleneck() {
            "  <- bottleneck"
        } else {
            ""
        };
        println!(
            "  {:<14} {:<32} {:>10} {load}{flag}",
            machine.node, machine.recipe, count
        );
    }

    if !report.sources.is_empty() {
        section("Sources");
        for source in &report.sources {
            let of = match source.target {
                Some(limit) => format!("  of {}", show(limit)),
                None => "  unlimited".to_owned(),
            };
            println!(
                "  {:<14} {:<32} {:>12}{of}",
                source.node,
                source.resource,
                show(source.achieved)
            );
        }
    }

    section("Links");
    if report.links.is_empty() {
        println!("  none");
    }
    for link in &report.links {
        let flag = if link.at_capacity {
            "  <- at capacity"
        } else {
            ""
        };
        println!(
            "  {:<28} {:<32} {:>12}{flag}",
            format!("{} -> {}", link.from, link.to),
            link.resource,
            show(link.rate)
        );
    }

    if !report.networks.is_empty() {
        section("Networks");
        for network in &report.networks {
            let state = if network.is_short() {
                format!(
                    "short {}",
                    show(Rate::per_tick(-network.net().as_per_tick()))
                )
            } else {
                format!("spare {}", show(network.net()))
            };
            println!(
                "  {:<14} {:<24} produced {:>10}  consumed {:>10}  {state}",
                network.name,
                network.kind,
                show(network.produced),
                show(network.consumed)
            );
        }
    }

    if !report.catalysts.is_empty() {
        section("Catalysts needed");
        for (resource, amount) in &report.catalysts {
            println!("  {resource:<32} {amount:>8.2}");
        }
    }

    section("Surplus");
    if report.surplus.is_empty() {
        println!("  none");
    }
    for surplus in &report.surplus {
        println!(
            "  {:<14} {:<32} {:>12}",
            surplus.node,
            surplus.resource,
            show(surplus.rate)
        );
    }
}

fn print_problems(schema_issues: &[model::Issue], report: &Report) {
    let all: Vec<&model::Issue> = schema_issues.iter().chain(&report.issues).collect();
    section("Problems");
    if all.is_empty() {
        println!("  none");
    }
    for issue in all {
        println!("  {issue}");
    }
}

fn section(title: &str) {
    println!();
    println!("{title}");
}
