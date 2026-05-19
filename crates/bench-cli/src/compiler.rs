use crate::{
    catalog::benchmarks,
    models::{
        Benchmark, BytecodeMetrics, CompileMetrics, CompileSet, CompiledArtifact, CompilerProfile, Language,
        Toolchain, Toolchains,
    },
    util::{byte_len, require_success, run_measured, sha256_bytes, stripped_cbor_len},
};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::{fs, path::Path, process::Command};

pub fn compile_all(root: &Path, toolchains: &Toolchains, only_benchmark: Option<&str>) -> Result<CompileSet> {
    let profiles = load_profiles(root)?;
    let mut artifacts = Vec::new();
    for benchmark in benchmarks() {
        if only_benchmark.is_some_and(|id| id != benchmark.id) {
            continue;
        }
        for profile in &profiles {
            let artifact = match profile.language {
                Language::Solidity => compile_solidity(root, &benchmark, profile, &toolchains.solc, &toolchains.evm_version)?,
                Language::Vyper => compile_vyper(root, &benchmark, profile, &toolchains.vyper, &toolchains.evm_version)?,
            };
            artifacts.push(artifact);
        }
    }
    Ok(CompileSet { profiles, artifacts })
}

fn load_profiles(root: &Path) -> Result<Vec<CompilerProfile>> {
    let mut profiles: Vec<CompilerProfile> = Vec::new();
    for entry in fs::read_dir(root.join("compiler-profiles"))? {
        let entry = entry?;
        if entry.path().extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(entry.path())?;
        profiles.push(toml::from_str(&text).with_context(|| format!("parsing {}", entry.path().display()))?);
    }
    profiles.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(profiles)
}

fn compile_solidity(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    solc: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = root.join(benchmark.solidity_path);
    let source = fs::read_to_string(&source_path)?;
    let file_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("solidity file name")?;
    let input = json!({
        "language": "Solidity",
        "sources": {
            file_name: { "content": source }
        },
        "settings": {
            "evmVersion": evm_version,
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
    let measured = require_success(
        run_measured(
            Command::new(&solc.binary_path).arg("--standard-json"),
            Some(serde_json::to_string(&input)?.as_bytes()),
        )?,
        "solc --standard-json",
    )?;
    let output: serde_json::Value = serde_json::from_slice(&measured.output.stdout)?;
    reject_solc_errors(&output)?;
    let contract = output
        .pointer(&format!("/contracts/{file_name}/{}", benchmark.contract_name))
        .with_context(|| format!("missing solc contract {}", benchmark.contract_name))?;
    let abi = contract.pointer("/abi").context("missing solidity abi")?.clone();
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
        measured.stats.wall_ms,
        measured.stats.cpu_ms,
        measured.stats.peak_rss_kib,
        json!({
            "evmVersion": evm_version,
            "optimizer": profile.optimizer,
            "optimizerRuns": profile.optimizer_runs,
            "viaIR": profile.via_ir
        }),
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

fn compile_vyper(
    root: &Path,
    benchmark: &Benchmark,
    profile: &CompilerProfile,
    vyper: &Toolchain,
    evm_version: &str,
) -> Result<CompiledArtifact> {
    let source_path = root.join(benchmark.vyper_path);
    let optimizer_mode = profile.optimizer_mode.as_deref().unwrap_or("gas");
    let measured = require_success(
        run_measured(
            Command::new(&vyper.binary_path)
                .arg("-f")
                .arg("abi,bytecode,bytecode_runtime")
                .arg("--evm-version")
                .arg(evm_version)
                .arg("-O")
                .arg(optimizer_mode)
                .arg(&source_path),
            None,
        )?,
        "vyper compile",
    )?;
    let stdout = String::from_utf8(measured.output.stdout)?;
    let mut lines = stdout.lines();
    let abi_line = lines.next().context("missing vyper abi output")?;
    let creation = lines.next().context("missing vyper bytecode output")?.to_string();
    let runtime = lines.next().context("missing vyper runtime output")?.to_string();
    let abi: serde_json::Value = serde_json::from_str(abi_line)?;
    artifact(
        benchmark,
        profile,
        vyper,
        &source_path,
        abi,
        creation,
        runtime,
        measured.stats.wall_ms,
        measured.stats.cpu_ms,
        measured.stats.peak_rss_kib,
        json!({
            "evmVersion": evm_version,
            "optimize": optimizer_mode
        }),
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
    wall_ms: f64,
    cpu_ms: f64,
    peak_rss_kib: u64,
    compiler_settings: serde_json::Value,
) -> Result<CompiledArtifact> {
    let source = fs::read(source_path)?;
    let bytecode = bytecode_metrics(&creation_bytecode, &runtime_bytecode)?;
    let language = profile.language;
    Ok(CompiledArtifact {
        benchmark_id: benchmark.id.to_string(),
        implementation_id: format!("{}/handwritten/v1", language.as_str()),
        language,
        contract_name: benchmark.contract_name.to_string(),
        profile_id: profile.id.clone(),
        compiler: toolchain.clone(),
        compiler_settings,
        source_path: source_path.to_path_buf(),
        source_hash: sha256_bytes(&source),
        abi,
        creation_bytecode,
        runtime_bytecode,
        compile: CompileMetrics {
            wall_ms_samples: vec![wall_ms],
            cpu_ms_samples: vec![cpu_ms],
            peak_rss_kib,
        },
        bytecode,
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
