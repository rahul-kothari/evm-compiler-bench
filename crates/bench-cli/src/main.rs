mod catalog;
mod compiler;
mod models;
mod toolchain;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use compiler::compile_all;
use std::path::PathBuf;
use toolchain::resolve_toolchains;

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
    /// Compile all implementations and print a compact summary.
    Compile {
        /// Do not download missing compilers.
        #[arg(long)]
        offline: bool,
        /// Restrict compilation to one benchmark id.
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
    let root = cli.root.canonicalize()?;
    match cli.command {
        Command::Run { offline, benchmark } | Command::Compile { offline, benchmark } => {
            let toolchains = resolve_toolchains(&root, offline)?;
            let compiled = compile_all(&root, &toolchains, benchmark.as_deref())?;
            println!(
                "compiled {} artifacts across {} profiles for EVM {}",
                compiled.artifacts.len(),
                compiled.profiles.len(),
                toolchains.evm_version
            );
            for artifact in &compiled.artifacts {
                println!(
                    "{} {} {} creation={} runtime={}",
                    artifact.benchmark_id,
                    artifact.implementation_id,
                    artifact.profile_id,
                    artifact.bytecode.creation_bytes,
                    artifact.bytecode.runtime_bytes
                );
            }
        }
        Command::Toolchains { offline } => {
            let toolchains = resolve_toolchains(&root, offline)?;
            println!("evm_version={}", toolchains.evm_version);
            println!(
                "solc version={} path={} sha256={}",
                toolchains.solc.version,
                toolchains.solc.binary_path.display(),
                toolchains.solc.binary_sha256
            );
            println!(
                "vyper version={} path={} sha256={}",
                toolchains.vyper.version,
                toolchains.vyper.binary_path.display(),
                toolchains.vyper.binary_sha256
            );
        }
    }
    Ok(())
}
