//! Craftflow headless utility.

mod solve;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(
    name = "craftflow",
    version,
    about = "Headless Craftflow utility: pack loading, import and schema solving"
)]
struct Cli {
    /// Increase log verbosity (repeat for more: -v debug, -vv trace).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Solve a schema and print flows, machine load and problems.
    Solve {
        /// The schema file.
        schema: PathBuf,

        /// A pack directory. Repeat for several. Without any, every
        /// `plugins/*/packs/*` below the working directory is used.
        #[arg(short, long = "pack")]
        packs: Vec<PathBuf>,

        /// Unit to print rates in.
        #[arg(short, long, default_value = "minute")]
        unit: solve::UnitArg,
    },

    /// Load packs and report what is in them, without solving anything.
    Packs {
        /// A pack directory. Repeat for several.
        #[arg(short, long = "pack")]
        packs: Vec<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let default_level = match cli.verbose {
        0 => "info",
        1 => "debug",
        _ => "trace",
    };
    // RUST_LOG, when set, wins over -v.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level)),
        )
        .with_target(false)
        .init();

    match cli.command {
        Some(Command::Solve {
            schema,
            packs,
            unit,
        }) => solve::run(&schema, &packs, unit.into()),
        Some(Command::Packs { packs }) => solve::describe_packs(&packs),
        None => {
            println!("craftflow {}", env!("CARGO_PKG_VERSION"));
            println!("Run with --help to see the commands.");
            Ok(())
        }
    }
}
