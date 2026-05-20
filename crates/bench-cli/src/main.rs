mod catalog;
mod compiler;
mod models;
mod report;
mod runner;
mod scale;
mod scenarios;
mod toolchain;
mod util;
mod validate;

use anyhow::Result;
use catalog::all_benchmarks;
use clap::{Parser, Subcommand};
use compiler::compile_all;
use report::write_outputs;
use runner::run_foundry;
use scale::generate_scale_suite;
use scenarios::load_scenario_catalog;
use std::path::PathBuf;
use toolchain::resolve_toolchains;
use validate::validate_all;

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
    /// Run the benchmark pipeline.
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
    /// Generate deterministic scale-study sources, specs, and scenarios.
    Generate {
        /// Restrict generation output used by the command to one benchmark id.
        #[arg(long)]
        benchmark: Option<String>,
    },
    /// Validate checked-in specs, scenarios, and generated outputs if present.
    Validate,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = cli.root.canonicalize()?;
    match cli.command {
        Command::Run { offline, benchmark } => {
            let toolchains = resolve_toolchains(&root, offline)?;
            let generated = generate_scale_suite(&root, benchmark.as_deref())?;
            let benchmarks = all_benchmarks(generated.benchmarks.clone(), benchmark.as_deref());
            let compiled = compile_all(&root, &toolchains, &benchmarks)?;
            let scenarios =
                load_scenario_catalog(&root, benchmark.as_deref(), &generated.scenarios)?;
            let gas_records = run_foundry(&root, &toolchains.evm_version, &compiled, &scenarios)?;
            let report = write_outputs(
                &root,
                &toolchains,
                &compiled,
                &gas_records,
                &scenarios,
                &generated.manifest,
            )?;
            println!("foundry produced {} gas records", gas_records.len());
            println!(
                "normalized results: {}",
                report.normalized_results.display()
            );
            println!("run manifest: {}", report.run_manifest.display());
            println!("html report: {}", report.html_report.display());
            println!(
                "methodology report: {}",
                report.methodology_report.display()
            );
            println!(
                "compiled {} artifacts with {} failures across {} profiles for EVM {}",
                compiled.artifacts.len(),
                compiled.failures.len(),
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
            for failure in &compiled.failures {
                println!(
                    "{} {} {} compile_error={}",
                    failure.benchmark_id,
                    failure.implementation_id,
                    failure.profile_id,
                    error_summary(&failure.error)
                );
            }
        }
        Command::Compile { offline, benchmark } => {
            let toolchains = resolve_toolchains(&root, offline)?;
            let generated = generate_scale_suite(&root, benchmark.as_deref())?;
            let benchmarks = all_benchmarks(generated.benchmarks, benchmark.as_deref());
            let compiled = compile_all(&root, &toolchains, &benchmarks)?;
            println!(
                "compiled {} artifacts with {} failures across {} profiles for EVM {}",
                compiled.artifacts.len(),
                compiled.failures.len(),
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
            for failure in &compiled.failures {
                println!(
                    "{} {} {} compile_error={}",
                    failure.benchmark_id,
                    failure.implementation_id,
                    failure.profile_id,
                    error_summary(&failure.error)
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
        Command::Generate { benchmark } => {
            let generated = generate_scale_suite(&root, benchmark.as_deref())?;
            println!(
                "generated {} scale benchmarks using {}",
                generated.manifest.benchmarks.len(),
                generated.manifest.generator_version
            );
            println!(
                "selected {} generated benchmarks for this invocation",
                generated.benchmarks.len()
            );
            println!(
                "generated root: {}",
                root.join("target/bench-generated").display()
            );
        }
        Command::Validate => {
            let summary = validate_all(&root)?;
            println!(
                "validated {} specs, {} scenario files, {} scale families, {} generated benchmarks, {} result rows",
                summary.specs,
                summary.scenario_files,
                summary.scale_families,
                summary.generated_benchmarks,
                summary.result_rows
            );
        }
    }
    Ok(())
}

fn error_summary(value: &str) -> String {
    for marker in ["Stack too deep", "CodeSizeLimit", "vyper.exceptions."] {
        if let Some(start) = value.find(marker) {
            let line = value[start..].lines().next().unwrap_or(marker);
            return line.trim_matches('"').to_string();
        }
    }
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && *line != "[" && *line != "]")
        .unwrap_or(value)
        .to_string()
}
