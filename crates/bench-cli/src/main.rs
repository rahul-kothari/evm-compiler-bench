use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, default_value = ".")]
    root: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the full milestone-1 benchmark pipeline.
    Run {
        /// Do not download missing compilers.
        #[arg(long)]
        offline: bool,
        /// Restrict execution to one benchmark id.
        #[arg(long)]
        benchmark: Option<String>,
    },
    /// Resolve compilers and print the selected shared EVM target.
    Toolchains {
        /// Do not download missing compilers.
        #[arg(long)]
        offline: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Run { offline, benchmark } => {
            println!(
                "benchmark pipeline is not implemented yet: root={}, offline={}, benchmark={:?}",
                cli.root.display(),
                offline,
                benchmark
            );
        }
        Command::Toolchains { offline } => {
            println!(
                "toolchain resolution is not implemented yet: root={}, offline={}",
                cli.root.display(),
                offline
            );
        }
    }
    Ok(())
}
