use crate::{
    models::{
        Benchmark, BytecodeMetrics, CommandStats, CompileFailure, CompileMetrics, CompileSet,
        CompiledArtifact, CompilerProfile, Language, MetadataMode, Toolchain, Toolchains,
    },
    util::{byte_len, require_success, run_measured, sha256_bytes, stripped_cbor_len},
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{env, fs, path::Path, process::Command};

const DEFAULT_COMPILE_SAMPLES: usize = 3;

pub fn compile_all(
    root: &Path,
    toolchains: &Toolchains,
    benchmarks: &[Benchmark],
) -> Result<CompileSet> {
    let profiles = load_profiles(root)?;
    let mut artifacts = Vec::new();
    let mut failures = Vec::new();
    for benchmark in benchmarks {
        for profile in &profiles {
            let result = match profile.language {
                Language::Solidity => compile_solidity(
                    root,
                    benchmark,
                    profile,
                    &toolchains.solc,
                    &toolchains.evm_version,
                ),
                Language::Vyper => compile_vyper(
                    root,
                    benchmark,
                    profile,
                    &toolchains.vyper,
                    &toolchains.evm_version,
                ),
            };
            match result {
                Ok(artifact) => artifacts.push(artifact),
                Err(error) => failures.push(compile_failure(
                    root,
                    benchmark,
                    profile,
                    match profile.language {
                        Language::Solidity => &toolchains.solc,
                        Language::Vyper => &toolchains.vyper,
                    },
                    &toolchains.evm_version,
                    error.to_string(),
                )?),
            }
        }
    }
    Ok(CompileSet {
        profiles,
        artifacts,
        failures,
    })
}

fn load_profiles(root: &Path) -> Result<Vec<CompilerProfile>> {
    let mut profiles: Vec<CompilerProfile> = Vec::new();
    for entry in fs::read_dir(root.join("compiler-profiles"))? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        let base: CompilerProfile =
            toml::from_str(&text).with_context(|| format!("parsing {}", entry.path().display()))?;
        profiles.extend(expand_metadata_profiles(&base));
    }
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(profiles)
}

fn expand_metadata_profiles(base: &CompilerProfile) -> Vec<CompilerProfile> {
    [MetadataMode::On, MetadataMode::Off]
        .into_iter()
        .map(|metadata_mode| {
            let mut profile = base.clone();
            profile.metadata_mode = metadata_mode;
            profile.id = format!("{}-metadata-{}", base.id, metadata_mode.as_str());
            profile
        })
        .collect()
}

fn compile_solidity(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    solc: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = root.join(&benchmark.solidity_path);
    let source = fs::read_to_string(&source_path)?;
    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("solidity file name")?;
    let metadata_settings = solidity_metadata_settings(profile.metadata_mode);
    let input = json!({
        "language": "Solidity",
        "sources": {
            file_name: { "content": source }
        },
        "settings": {
            "evmVersion": evm_version,
            "metadata": metadata_settings,
            "optimizer": {
                "enabled": profile.optimizer,
                "runs": profile.optimizer_runs
            },
            "viaIR": profile.via_ir,
            "outputSelection": {
                "*": {
                    "*": ["abi", "evm.bytecode.object", "evm.deployedBytecode.object"]
                }
            }
        }
    });
    let input = serde_json::to_vec(&input)?;
    let measured = repeat_compile_samples(
        || {
            let mut command = Command::new(&solc.binary_path);
            command.arg("--standard-json");
            command
        },
        Some(&input),
        "solc --standard-json",
    )?;
    let output: serde_json::Value = serde_json::from_slice(&measured.output_stdout)?;
    reject_solc_errors(&output)?;
    let contract = output
        .pointer(&format!(
            "/contracts/{file_name}/{}",
            benchmark.contract_name
        ))
        .with_context(|| format!("missing solc contract {}", benchmark.contract_name))?;
    let abi = contract
        .pointer("/abi")
        .context("missing solidity abi")?
        .clone();
    let creation = contract
        .pointer("/evm/bytecode/object")
        .and_then(|value| value.as_str())
        .context("missing solidity creation bytecode")?
        .to_string();
    let runtime = contract
        .pointer("/evm/deployedBytecode/object")
        .and_then(|value| value.as_str())
        .context("missing solidity runtime bytecode")?
        .to_string();
    artifact(
        benchmark,
        profile,
        solc,
        &source_path,
        abi,
        creation,
        runtime,
        measured.wall_ms_samples,
        measured.cpu_ms_samples,
        measured.peak_rss_kib,
        solidity_compiler_settings(profile, evm_version),
    )
}

fn reject_solc_errors(output: &serde_json::Value) -> Result<()> {
    let Some(errors) = output.get("errors").and_then(|value| value.as_array()) else {
        return Ok(());
    };
    let fatal: Vec<_> = errors
        .iter()
        .filter(|error| error.get("severity").and_then(|value| value.as_str()) == Some("error"))
        .collect();
    if fatal.is_empty() {
        return Ok(());
    }
    bail!("{}", serde_json::to_string_pretty(&fatal)?);
}

fn solidity_metadata_settings(metadata_mode: MetadataMode) -> serde_json::Value {
    match metadata_mode {
        MetadataMode::On => json!({
            "bytecodeHash": "ipfs",
            "appendCBOR": true
        }),
        MetadataMode::Off => json!({
            "bytecodeHash": "none",
            "appendCBOR": false
        }),
    }
}

fn solidity_compiler_settings(profile: &CompilerProfile, evm_version: &str) -> serde_json::Value {
    json!({
        "evmVersion": evm_version,
        "metadataMode": profile.metadata_mode.as_str(),
        "metadata": solidity_metadata_settings(profile.metadata_mode),
        "optimizer": profile.optimizer,
        "optimizerRuns": profile.optimizer_runs,
        "viaIR": profile.via_ir
    })
}

fn vyper_compiler_settings(profile: &CompilerProfile, evm_version: &str) -> serde_json::Value {
    let optimizer_mode = profile.optimizer_mode.as_deref().unwrap_or("gas");
    json!({
        "evmVersion": evm_version,
        "metadataMode": profile.metadata_mode.as_str(),
        "bytecodeMetadata": profile.metadata_mode == MetadataMode::On,
        "optimize": optimizer_mode
    })
}

fn compile_vyper(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    vyper: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = root.join(&benchmark.vyper_path);
    let optimizer_mode = profile.optimizer_mode.as_deref().unwrap_or("gas");
    let measured = repeat_compile_samples(
        || {
            let mut command = Command::new(&vyper.binary_path);
            command
                .arg("-f")
                .arg("abi,bytecode,bytecode_runtime")
                .arg("--evm-version")
                .arg(evm_version)
                .arg("-O")
                .arg(optimizer_mode);
            if profile.metadata_mode == MetadataMode::Off {
                command.arg("--no-bytecode-metadata");
            }
            command.arg(&source_path);
            command
        },
        None,
        "vyper compile",
    )?;
    let stdout = String::from_utf8(measured.output_stdout)?;
    let mut lines = stdout.lines();
    let abi_line = lines.next().context("missing vyper abi output")?;
    let creation = lines
        .next()
        .context("missing vyper bytecode output")?
        .to_string();
    let runtime = lines
        .next()
        .context("missing vyper runtime output")?
        .to_string();
    let abi: serde_json::Value = serde_json::from_str(abi_line)?;
    artifact(
        benchmark,
        profile,
        vyper,
        &source_path,
        abi,
        creation,
        runtime,
        measured.wall_ms_samples,
        measured.cpu_ms_samples,
        measured.peak_rss_kib,
        vyper_compiler_settings(profile, evm_version),
    )
}

#[allow(clippy::too_many_arguments)]
fn artifact(
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    source_path: &Path,
    abi: serde_json::Value,
    creation_bytecode: String,
    runtime_bytecode: String,
    wall_ms_samples: Vec<f64>,
    cpu_ms_samples: Vec<f64>,
    peak_rss_kib: u64,
    compiler_settings: serde_json::Value,
) -> Result<CompiledArtifact> {
    let source = fs::read(source_path)?;
    let bytecode = bytecode_metrics(&creation_bytecode, &runtime_bytecode)?;
    let language = profile.language;
    Ok(CompiledArtifact {
        benchmark_id: benchmark.id.clone(),
        implementation_id: format!("{}/handwritten/v1", language.as_str()),
        suite: benchmark.suite,
        family: benchmark.family.clone(),
        parameter_name: benchmark.parameter_name.clone(),
        parameter_value: benchmark.parameter_value,
        scenario_path: benchmark.scenario_path.clone(),
        scenario_hash: benchmark.scenario_hash.clone(),
        generator_version: benchmark.generator_version.clone(),
        provenance: benchmark.provenance.clone(),
        language,
        contract_name: benchmark.contract_name.clone(),
        profile_id: profile.id.clone(),
        compiler: toolchain.clone(),
        compiler_settings,
        metadata_mode: profile.metadata_mode,
        source_path: source_path.to_path_buf(),
        source_hash: sha256_bytes(&source),
        abi,
        creation_bytecode,
        runtime_bytecode,
        compile: CompileMetrics {
            wall_ms_samples,
            cpu_ms_samples,
            peak_rss_kib,
        },
        bytecode,
    })
}

struct CompileSamples {
    output_stdout: Vec<u8>,
    wall_ms_samples: Vec<f64>,
    cpu_ms_samples: Vec<f64>,
    peak_rss_kib: u64,
}

fn repeat_compile_samples<F>(
    mut command_factory: F,
    stdin: Option<&[u8]>,
    label: &str,
) -> Result<CompileSamples>
where
    F: FnMut() -> Command,
{
    let sample_count = compile_sample_count();
    let mut output_stdout = Vec::new();
    let mut wall_ms_samples = Vec::with_capacity(sample_count);
    let mut cpu_ms_samples = Vec::with_capacity(sample_count);
    let mut peak_rss_kib = 0;
    for sample_index in 0..sample_count {
        let mut command = command_factory();
        let measured = require_success(run_measured(&mut command, stdin)?, label)?;
        let CommandStats {
            wall_ms,
            cpu_ms,
            peak_rss_kib: sample_peak_rss_kib,
        } = measured.stats;
        wall_ms_samples.push(wall_ms);
        cpu_ms_samples.push(cpu_ms);
        peak_rss_kib = peak_rss_kib.max(sample_peak_rss_kib);
        if sample_index + 1 == sample_count {
            output_stdout = measured.output.stdout;
        }
    }
    Ok(CompileSamples {
        output_stdout,
        wall_ms_samples,
        cpu_ms_samples,
        peak_rss_kib,
    })
}

fn compile_sample_count() -> usize {
    env::var("EVM_BENCH_COMPILE_SAMPLES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=20).contains(value))
        .unwrap_or(DEFAULT_COMPILE_SAMPLES)
}

fn compile_failure(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    toolchain: &Toolchain,
    evm_version: &str,
    error: String,
) -> Result<CompileFailure> {
    let language = profile.language;
    let source_path = match language {
        Language::Solidity => root.join(&benchmark.solidity_path),
        Language::Vyper => root.join(&benchmark.vyper_path),
    };
    let source = fs::read(&source_path)?;
    let compiler_settings = match language {
        Language::Solidity => solidity_compiler_settings(profile, evm_version),
        Language::Vyper => vyper_compiler_settings(profile, evm_version),
    };
    Ok(CompileFailure {
        benchmark_id: benchmark.id.clone(),
        implementation_id: format!("{}/handwritten/v1", language.as_str()),
        suite: benchmark.suite,
        family: benchmark.family.clone(),
        parameter_name: benchmark.parameter_name.clone(),
        parameter_value: benchmark.parameter_value,
        scenario_path: benchmark.scenario_path.clone(),
        scenario_hash: benchmark.scenario_hash.clone(),
        generator_version: benchmark.generator_version.clone(),
        provenance: benchmark.provenance.clone(),
        language,
        contract_name: benchmark.contract_name.clone(),
        profile_id: profile.id.clone(),
        compiler: toolchain.clone(),
        compiler_settings,
        metadata_mode: profile.metadata_mode,
        source_path,
        source_hash: sha256_bytes(&source),
        error,
    })
}

fn bytecode_metrics(creation: &str, runtime: &str) -> Result<BytecodeMetrics> {
    let creation_bytes = byte_len(creation)?;
    let runtime_bytes = byte_len(runtime)?;
    let creation_bytes_stripped = stripped_cbor_len(creation)?;
    let runtime_bytes_stripped = stripped_cbor_len(runtime)?;
    Ok(BytecodeMetrics {
        creation_bytes,
        creation_bytes_stripped,
        runtime_bytes,
        runtime_bytes_stripped,
        initcode_bytes: creation_bytes,
        linked_runtime_bytes: runtime_bytes,
        eip170_margin_bytes: 24_576 - runtime_bytes as isize,
        eip3860_margin_bytes: 49_152 - creation_bytes as isize,
        code_deposit_gas: 200 * runtime_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::bytecode_metrics;

    #[test]
    fn computes_bytecode_metrics() {
        let metrics = bytecode_metrics("0x6001600055", "0x60016000").unwrap();
        assert_eq!(metrics.creation_bytes, 5);
        assert_eq!(metrics.runtime_bytes, 4);
        assert_eq!(metrics.code_deposit_gas, 800);
    }
}
