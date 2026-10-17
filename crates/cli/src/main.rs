//! Craftflow headless utility.

use clap::Parser;
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
        .init();

    tracing::debug!(version = env!("CARGO_PKG_VERSION"), "craftflow cli");

    // Stage 2 adds `solve <schema.json>` here.
    println!("craftflow {}", env!("CARGO_PKG_VERSION"));
    println!("No commands yet. Run with --help.");

    Ok(())
}
