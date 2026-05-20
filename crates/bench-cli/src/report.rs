use crate::{
    models::{
        BenchmarkSuite, CompileFailure, CompileSet, CompiledArtifact, GasRecord, Language,
        Provenance, ScenarioFile, Toolchains,
    },
    scale::ScaleManifest,
    scenarios::ScenarioCatalog,
    util::ensure_dir,
};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SOL_CODEGEN_BASELINE: &str = "solc-latest-legacy-runs200";
const SOL_NOOPT_CODEGEN: &str = "solc-latest-noopt";
const SOL_VIAIR_CODEGEN: &str = "solc-latest-viair-runs200";
const VYPER_GAS_CODEGEN: &str = "vyper-latest-gas";
const VYPER_GAS_VENOM_CODEGEN: &str = "vyper-latest-gas-venom";
const VYPER_CODESIZE_CODEGEN: &str = "vyper-latest-codesize";
const VYPER_CODESIZE_VENOM_CODEGEN: &str = "vyper-latest-codesize-venom";
const VYPER_NONE_CODEGEN: &str = "vyper-latest-none";
const VYPER_NONE_VENOM_CODEGEN: &str = "vyper-latest-none-venom";
const VYPER_ALPHA_GAS_CODEGEN: &str = "vyper-0.5.0a1-gas";
const VYPER_ALPHA_GAS_VENOM_CODEGEN: &str = "vyper-0.5.0a1-gas-venom";
const VYPER_ALPHA_CODESIZE_CODEGEN: &str = "vyper-0.5.0a1-codesize";
const VYPER_ALPHA_CODESIZE_VENOM_CODEGEN: &str = "vyper-0.5.0a1-codesize-venom";
const VYPER_ALPHA_NONE_CODEGEN: &str = "vyper-0.5.0a1-none";
const VYPER_ALPHA_NONE_VENOM_CODEGEN: &str = "vyper-0.5.0a1-none-venom";
const PRIMARY_CODEGEN_PROFILES: [&str; 14] = [
    SOL_CODEGEN_BASELINE,
    SOL_VIAIR_CODEGEN,
    VYPER_NONE_CODEGEN,
    VYPER_GAS_CODEGEN,
    VYPER_CODESIZE_CODEGEN,
    VYPER_NONE_VENOM_CODEGEN,
    VYPER_GAS_VENOM_CODEGEN,
    VYPER_CODESIZE_VENOM_CODEGEN,
    VYPER_ALPHA_NONE_CODEGEN,
    VYPER_ALPHA_GAS_CODEGEN,
    VYPER_ALPHA_CODESIZE_CODEGEN,
    VYPER_ALPHA_NONE_VENOM_CODEGEN,
    VYPER_ALPHA_GAS_VENOM_CODEGEN,
    VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
];
const SOL_CODEGEN_PROFILES: [&str; 3] =
    [SOL_CODEGEN_BASELINE, SOL_VIAIR_CODEGEN, SOL_NOOPT_CODEGEN];
const VYPER_CODEGEN_PROFILES: [&str; 12] = [
    VYPER_NONE_CODEGEN,
    VYPER_GAS_CODEGEN,
    VYPER_CODESIZE_CODEGEN,
    VYPER_NONE_VENOM_CODEGEN,
    VYPER_GAS_VENOM_CODEGEN,
    VYPER_CODESIZE_VENOM_CODEGEN,
    VYPER_ALPHA_NONE_CODEGEN,
    VYPER_ALPHA_GAS_CODEGEN,
    VYPER_ALPHA_CODESIZE_CODEGEN,
    VYPER_ALPHA_NONE_VENOM_CODEGEN,
    VYPER_ALPHA_GAS_VENOM_CODEGEN,
    VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
];
const VYPER_DELTA_PROFILES: [&str; 11] = [
    VYPER_NONE_CODEGEN,
    VYPER_CODESIZE_CODEGEN,
    VYPER_NONE_VENOM_CODEGEN,
    VYPER_GAS_VENOM_CODEGEN,
    VYPER_CODESIZE_VENOM_CODEGEN,
    VYPER_ALPHA_NONE_CODEGEN,
    VYPER_ALPHA_GAS_CODEGEN,
    VYPER_ALPHA_CODESIZE_CODEGEN,
    VYPER_ALPHA_NONE_VENOM_CODEGEN,
    VYPER_ALPHA_GAS_VENOM_CODEGEN,
    VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
];
const REPORT_ASSETS: [(&str, &[u8]); 3] = [
    (
        "vega-6.2.0.min.js",
        include_bytes!("../assets/vega-6.2.0.min.js"),
    ),
    (
        "vega-lite-6.4.3.min.js",
        include_bytes!("../assets/vega-lite-6.4.3.min.js"),
    ),
    (
        "vega-embed-7.1.0.min.js",
        include_bytes!("../assets/vega-embed-7.1.0.min.js"),
    ),
];

pub struct ReportPaths {
    pub normalized_results: PathBuf,
    pub run_manifest: PathBuf,
    pub html_report: PathBuf,
    pub methodology_report: PathBuf,
}

#[derive(Debug, Default)]
struct ReportSummary {
    ok_rows: usize,
    compile_failures: usize,
    profiles: BTreeSet<String>,
    benchmarks: BTreeSet<String>,
    fixed_benchmarks: BTreeSet<String>,
    fixed_scenarios: BTreeSet<String>,
    scale_families: BTreeSet<String>,
    scale_values: BTreeSet<u64>,
    real_benchmarks: BTreeSet<String>,
    real_scenarios: BTreeSet<String>,
    successful_artifacts: BTreeSet<String>,
    failed_artifacts: BTreeSet<String>,
    scenario_status_pass: usize,
    scenario_status_fail: usize,
    baseline_differential_rows: usize,
    randomized_rows: usize,
    property_rows: usize,
    golden_rows: usize,
    log_rows: usize,
}

impl ReportSummary {
    fn from_rows(rows: &[serde_json::Value]) -> Self {
        let mut summary = Self::default();

        for row in rows {
            let status = str_at(row, "/status").unwrap_or_default();
            let suite = str_at(row, "/suite").unwrap_or_default();
            let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
            let profile = str_at(row, "/profile_id").unwrap_or_default();
            if !profile.is_empty() {
                summary.profiles.insert(profile);
            }
            if !benchmark.is_empty() {
                summary.benchmarks.insert(benchmark.clone());
            }

            match suite.as_str() {
                "fixed" => {
                    summary.fixed_benchmarks.insert(benchmark.clone());
                    if let Some(scenario) = str_at(row, "/gas/scenario") {
                        summary
                            .fixed_scenarios
                            .insert(format!("{benchmark}\0{scenario}"));
                    }
                }
                "scale" => {
                    if let Some(family) = str_at(row, "/family") {
                        summary.scale_families.insert(family);
                    }
                    if let Some(value) = row
                        .pointer("/parameter_value")
                        .and_then(|value| value.as_u64())
                    {
                        summary.scale_values.insert(value);
                    }
                }
                "real_derived" => {
                    summary.real_benchmarks.insert(benchmark.clone());
                    if let Some(scenario) = str_at(row, "/gas/scenario") {
                        summary
                            .real_scenarios
                            .insert(format!("{benchmark}\0{scenario}"));
                    }
                }
                _ => {}
            }

            if status == "ok" {
                summary.ok_rows += 1;
                summary
                    .successful_artifacts
                    .insert(artifact_key_from_row(row));
                if str_at(row, "/correctness/scenario_status_check").as_deref() == Some("pass") {
                    summary.scenario_status_pass += 1;
                } else {
                    summary.scenario_status_fail += 1;
                }
                if str_at(row, "/correctness/baseline_differential_check").as_deref()
                    == Some("baseline_only")
                {
                    summary.baseline_differential_rows += 1;
                }
                if str_at(row, "/correctness/randomized_differential_check").as_deref()
                    == Some("pass")
                {
                    summary.randomized_rows += 1;
                }
                if str_at(row, "/correctness/property_tests").as_deref() == Some("pass") {
                    summary.property_rows += 1;
                }
                if !matches!(
                    str_at(row, "/correctness/golden_behavior_check").as_deref(),
                    Some("not_run" | "not_applicable") | None
                ) {
                    summary.golden_rows += 1;
                }
                if !matches!(
                    str_at(row, "/correctness/log_check").as_deref(),
                    Some("not_run" | "not_applicable") | None
                ) {
                    summary.log_rows += 1;
                }
            } else if status == "compile_error" {
                summary.compile_failures += 1;
                summary.failed_artifacts.insert(artifact_key_from_row(row));
            }
        }
        summary
    }

    fn attempted_artifacts(&self) -> usize {
        self.successful_artifacts.len() + self.failed_artifacts.len()
    }
}

pub fn write_outputs(
    root: &Path,
    toolchains: &Toolchains,
    compiled: &CompileSet,
    gas_records: &[GasRecord],
    scenarios: &ScenarioCatalog,
    scale_manifest: &ScaleManifest,
) -> Result<ReportPaths> {
    let normalized_dir = root.join("results/normalized");
    let reports_dir = root.join("results/reports");
    ensure_dir(&normalized_dir)?;
    ensure_dir(&reports_dir)?;
    write_report_assets(&reports_dir)?;

    let rows = normalized_rows(root, compiled, gas_records, scenarios)?;
    let normalized_results = normalized_dir.join("results.json");
    fs::write(&normalized_results, serde_json::to_string_pretty(&rows)?)?;

    let run_manifest = normalized_dir.join("run-manifest.json");
    let manifest = json!({
        "run_id": Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        "started_at": Utc::now(),
        "evm_version": toolchains.evm_version,
        "toolchains": toolchains.compilers.values().collect::<Vec<_>>(),
        "profiles": compiled.profiles,
        "scale_generator": {
            "version": scale_manifest.generator_version.clone(),
            "config_hash": scale_manifest.config_hash.clone(),
            "parameter_name": scale_manifest.parameter_name.clone(),
            "values": scale_manifest.values.clone(),
            "benchmarks": scale_manifest.benchmarks.clone()
        },
        "real_derived": {
            "benchmarks": real_derived_manifest(compiled)
        },
        "environment": environment_manifest(root),
        "artifacts": compiled.artifacts.len(),
        "compile_failures": compiled.failures.len(),
        "gas_records": gas_records.len(),
        "cache": cache_manifest(compiled, gas_records)
    });
    fs::write(&run_manifest, serde_json::to_string_pretty(&manifest)?)?;

    let html_report = reports_dir.join("index.html");
    fs::write(&html_report, render_html(&rows, toolchains, &manifest)?)?;
    let methodology_report = reports_dir.join("methodology.html");
    fs::write(
        &methodology_report,
        render_methodology_html(&rows, toolchains, &manifest)?,
    )?;

    Ok(ReportPaths {
        normalized_results,
        run_manifest,
        html_report,
        methodology_report,
    })
}

fn write_report_assets(reports_dir: &Path) -> Result<()> {
    let assets_dir = reports_dir.join("assets");
    ensure_dir(&assets_dir)?;
    for (name, bytes) in REPORT_ASSETS {
        fs::write(assets_dir.join(name), bytes)?;
    }
    Ok(())
}

fn cache_manifest(compiled: &CompileSet, gas_records: &[GasRecord]) -> serde_json::Value {
    let mut compile = BTreeMap::<String, usize>::new();
    let mut gas = BTreeMap::<String, usize>::new();
    for artifact in &compiled.artifacts {
        *compile.entry(artifact.cache.status.clone()).or_default() += 1;
    }
    for failure in &compiled.failures {
        *compile.entry(failure.cache.status.clone()).or_default() += 1;
    }
    for record in gas_records {
        *gas.entry(record.cache.status.clone()).or_default() += 1;
    }
    json!({
        "enabled": !compile.contains_key("disabled") || compile.len() > 1 || !gas.contains_key("disabled") || gas.len() > 1,
        "root": ".cache/bench-cli",
        "compile": compile,
        "gas": gas,
        "statuses": {
            "hit": "entry was reused from cache",
            "miss": "no prior entry matched this logical benchmark/config",
            "stale": "a prior entry existed but one or more fingerprint fields changed",
            "refreshed": "a valid gas cache entry existed but the Foundry batch was rerun because another row missed",
            "disabled": "cache was bypassed for this run"
        }
    })
}

fn normalized_rows(
    root: &Path,
    compiled: &CompileSet,
    gas_records: &[GasRecord],
    scenarios: &ScenarioCatalog,
) -> Result<Vec<serde_json::Value>> {
    let mut artifacts = BTreeMap::new();
    for artifact in &compiled.artifacts {
        artifacts.insert(
            artifact_key(
                &artifact.benchmark_id,
                &artifact.implementation_id,
                &artifact.profile_id,
            ),
            artifact,
        );
    }

    let failure_links = failure_links_by_benchmark(root)?;
    let differential_benchmarks = differential_benchmarks(compiled);
    let mut rows = Vec::with_capacity(gas_records.len() + compiled.failures.len());
    for gas in gas_records {
        let artifact = artifacts
            .get(&artifact_key(
                &gas.benchmark_id,
                &gas.implementation_id,
                &gas.profile_id,
            ))
            .with_context(|| {
                format!(
                    "missing artifact for {}/{}/{}",
                    gas.benchmark_id, gas.implementation_id, gas.profile_id
                )
            })?;
        let scenario_file = scenarios.get(&artifact.benchmark_id)?;
        let failures = failure_links
            .get(&artifact.benchmark_id)
            .cloned()
            .unwrap_or_default();
        rows.push(row(
            artifact,
            gas,
            scenario_file,
            failures,
            differential_benchmarks.contains(&artifact.benchmark_id),
        ));
    }
    for failure in &compiled.failures {
        rows.push(failure_row(failure));
    }
    rows.sort_by(|a, b| {
        let left = sort_key(a);
        let right = sort_key(b);
        left.cmp(&right)
    });
    Ok(rows)
}

fn row(
    artifact: &CompiledArtifact,
    gas: &GasRecord,
    scenario_file: &ScenarioFile,
    failure_links: Vec<String>,
    differential_available: bool,
) -> serde_json::Value {
    let scenario = scenario_file
        .scenarios
        .iter()
        .find(|scenario| scenario.name == gas.scenario);
    let has_observers = scenario.is_some_and(|scenario| !scenario.observers.is_empty());
    let baseline_status = if differential_available {
        "baseline_only"
    } else {
        "not_applicable"
    };
    let randomized_status = correctness_status(
        scenario_file.randomized.is_some(),
        &failure_links,
        "randomized_differential",
    );
    let property_status = correctness_status(
        !scenario_file.properties.is_empty(),
        &failure_links,
        "property",
    );
    json!({
        "status": "ok",
        "benchmark_id": gas.benchmark_id,
        "implementation_id": gas.implementation_id,
        "profile_id": artifact.profile_id,
        "suite": artifact.suite.as_str(),
        "family": artifact.family.clone(),
        "parameter_name": artifact.parameter_name.clone(),
        "parameter_value": artifact.parameter_value,
        "generated": {
            "generator_version": artifact.generator_version.clone(),
            "scenario_path": artifact.scenario_path.clone(),
            "scenario_hash": artifact.scenario_hash.clone()
        },
        "provenance": provenance_value(
            artifact.provenance.as_ref(),
            artifact.language,
            &artifact.implementation_id
        ),
        "language": artifact.language.as_str(),
        "compiler": {
            "name": artifact.compiler.name,
            "version": artifact.compiler.version,
            "binary_path": artifact.compiler.binary_path,
            "binary_sha256": artifact.compiler.binary_sha256,
            "download_source": artifact.compiler.download_source,
            "metadata": artifact.compiler.metadata,
            "settings": artifact.compiler_settings
        },
        "source_hash": artifact.source_hash,
        "cache": {
            "compile": artifact.cache,
            "gas": gas.cache
        },
        "compile": {
            "status": "ok",
            "wall_ms_samples": artifact.compile.wall_ms_samples.clone(),
            "cpu_ms_samples": artifact.compile.cpu_ms_samples.clone(),
            "peak_rss_kib": artifact.compile.peak_rss_kib
        },
        "bytecode": artifact.bytecode,
        "gas": {
            "scenario": gas.scenario,
            "evm_fork": artifact.compiler_settings.get("evmVersion").cloned().unwrap_or_else(|| json!("unknown")),
            "state_access_profile": gas.state_access_profile.as_str(),
            "metadata_mode": gas.metadata_mode.as_str(),
            "internal_create_gas": gas.internal_create_gas,
            "harness_call_gas": gas.harness_call_gas,
            "intrinsic_gas": gas.intrinsic_gas,
            "calldata_gas": gas.calldata_gas,
            "harness_estimated_tx_gas": gas.harness_estimated_tx_gas,
            "total_tx_gas": null,
            "expected_success": gas.expected_success,
            "call_succeeded": gas.call_succeeded,
            "scenario_status_ok": gas.scenario_status_ok,
            "measurement_scope": "foundry_internal_call_harness"
        },
        "correctness": {
            "scenario_status_check": if gas.scenario_status_ok { "pass" } else { "fail" },
            "golden_behavior_check": "not_run",
            "baseline_differential_check": baseline_status,
            "profile_behavior_check": "not_run",
            "observer_check": if has_observers { baseline_status } else { "not_applicable" },
            "return_data_check": baseline_status,
            "log_check": "not_run",
            "randomized_differential_check": randomized_status,
            "property_tests": property_status,
            "properties": scenario_file.properties.iter().map(|property| property.name.clone()).collect::<Vec<_>>(),
            "failure_artifacts": failure_links,
            "scenario_status_ok": gas.scenario_status_ok
        }
    })
}

fn differential_benchmarks(compiled: &CompileSet) -> BTreeSet<String> {
    let mut solidity = BTreeSet::new();
    let mut vyper = BTreeSet::new();
    for artifact in &compiled.artifacts {
        match artifact.profile_id.as_str() {
            SOL_CODEGEN_BASELINE => {
                solidity.insert(artifact.benchmark_id.clone());
            }
            VYPER_GAS_CODEGEN => {
                vyper.insert(artifact.benchmark_id.clone());
            }
            _ => {}
        }
    }
    solidity.intersection(&vyper).cloned().collect()
}

fn failure_row(failure: &CompileFailure) -> serde_json::Value {
    json!({
        "status": "compile_error",
        "benchmark_id": failure.benchmark_id,
        "implementation_id": failure.implementation_id,
        "profile_id": failure.profile_id,
        "suite": failure.suite.as_str(),
        "family": failure.family.clone(),
        "parameter_name": failure.parameter_name.clone(),
        "parameter_value": failure.parameter_value,
        "generated": {
            "generator_version": failure.generator_version.clone(),
            "scenario_path": failure.scenario_path.clone(),
            "scenario_hash": failure.scenario_hash.clone()
        },
        "provenance": provenance_value(
            failure.provenance.as_ref(),
            failure.language,
            &failure.implementation_id
        ),
        "language": failure.language.as_str(),
        "compiler": {
            "name": failure.compiler.name,
            "version": failure.compiler.version,
            "binary_path": failure.compiler.binary_path,
            "binary_sha256": failure.compiler.binary_sha256,
            "download_source": failure.compiler.download_source,
            "metadata": failure.compiler.metadata,
            "settings": failure.compiler_settings
        },
        "source_hash": failure.source_hash,
        "cache": {
            "compile": failure.cache,
            "gas": null
        },
        "compile": {
            "status": "error",
            "error": failure.error
        },
        "bytecode": null,
        "gas": null,
        "correctness": {
            "scenario_status_check": "not_applicable",
            "golden_behavior_check": "not_applicable",
            "baseline_differential_check": "not_applicable",
            "profile_behavior_check": "not_applicable",
            "observer_check": "not_applicable",
            "return_data_check": "not_applicable",
            "log_check": "not_applicable",
            "randomized_differential_check": "not_applicable",
            "property_tests": "not_applicable",
            "properties": [],
            "failure_artifacts": [],
            "scenario_status_ok": false
        }
    })
}

fn real_derived_manifest(compiled: &CompileSet) -> Vec<serde_json::Value> {
    let mut benchmarks = BTreeMap::new();
    for artifact in &compiled.artifacts {
        if let Some(provenance) = &artifact.provenance {
            benchmarks
                .entry(artifact.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&artifact.benchmark_id, provenance));
        }
    }
    for failure in &compiled.failures {
        if let Some(provenance) = &failure.provenance {
            benchmarks
                .entry(failure.benchmark_id.clone())
                .or_insert_with(|| provenance_manifest_value(&failure.benchmark_id, provenance));
        }
    }
    benchmarks.into_values().collect()
}

fn provenance_manifest_value(benchmark_id: &str, provenance: &Provenance) -> serde_json::Value {
    json!({
        "benchmark_id": benchmark_id,
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_language": provenance.source_language.as_str(),
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "production_equivalence": provenance.production_equivalence,
        "api_compatibility": &provenance.api_compatibility,
        "storage_layout_compatibility": provenance.storage_layout_compatibility,
        "external_token_semantics": &provenance.external_token_semantics,
        "source_derivation": &provenance.source_derivation,
        "equivalence_scope": &provenance.equivalence_scope,
        "scenario_coverage": &provenance.scenario_coverage,
        "mock_assumptions": &provenance.mock_assumptions,
        "included_features": &provenance.included_features,
        "excluded_features": &provenance.excluded_features,
    })
}

fn provenance_value(
    provenance: Option<&Provenance>,
    port_language: Language,
    port_version: &str,
) -> serde_json::Value {
    let Some(provenance) = provenance else {
        return serde_json::Value::Null;
    };
    json!({
        "upstream_project": &provenance.upstream_project,
        "repository_url": &provenance.repository_url,
        "source_commit": &provenance.source_commit,
        "source_path": &provenance.source_path,
        "source_language": provenance.source_language.as_str(),
        "source_contract": &provenance.source_contract,
        "source_blob": &provenance.source_blob,
        "upstream_license": &provenance.upstream_license,
        "checked_at": &provenance.checked_at,
        "model_kind": &provenance.model_kind,
        "production_equivalence": provenance.production_equivalence,
        "api_compatibility": &provenance.api_compatibility,
        "storage_layout_compatibility": provenance.storage_layout_compatibility,
        "external_token_semantics": &provenance.external_token_semantics,
        "source_derivation": &provenance.source_derivation,
        "port_language": port_language.as_str(),
        "port_version": port_version,
        "equivalence_scope": &provenance.equivalence_scope,
        "scenario_coverage": &provenance.scenario_coverage,
        "mock_assumptions": &provenance.mock_assumptions,
        "included_features": &provenance.included_features,
        "excluded_features": &provenance.excluded_features,
    })
}

fn environment_manifest(root: &Path) -> serde_json::Value {
    json!({
        "os": env::consts::OS,
        "arch": env::consts::ARCH,
        "family": env::consts::FAMILY,
        "command_line": env::args().collect::<Vec<_>>(),
        "git": {
            "commit": command_output(root, "git", &["rev-parse", "HEAD"]),
            "dirty": command_output(root, "git", &["status", "--porcelain"]).is_some_and(|output| !output.is_empty())
        },
        "tools": {
            "forge": command_output(root, "forge", &["--version"]),
            "cargo": command_output(root, "cargo", &["--version"]),
            "rustc": command_output(root, "rustc", &["--version"]),
            "uv": command_output(root, "uv", &["--version"])
        },
        "host": {
            "kernel": command_output(root, "uname", &["-a"]),
            "cpu": cpu_model(root)
        }
    })
}

fn cpu_model(root: &Path) -> Option<String> {
    command_output(root, "sysctl", &["-n", "machdep.cpu.brand_string"]).or_else(|| {
        fs::read_to_string("/proc/cpuinfo").ok().and_then(|text| {
            text.lines()
                .find_map(|line| line.split_once(':'))
                .and_then(|(key, value)| {
                    if key.trim() == "model name" {
                        Some(value.trim().to_string())
                    } else {
                        None
                    }
                })
        })
    })
}

fn command_output(root: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn correctness_status(applicable: bool, failure_links: &[String], kind: &str) -> &'static str {
    if !applicable {
        return "not_applicable";
    }
    if failure_links.iter().any(|link| link.contains(kind)) {
        "fail"
    } else {
        "pass"
    }
}

fn failure_links_by_benchmark(root: &Path) -> Result<BTreeMap<String, Vec<String>>> {
    let mut links: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let failure_dir = root.join("results/raw/failures");
    if !failure_dir.exists() {
        return Ok(links);
    }
    for entry in fs::read_dir(&failure_dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some((benchmark_id, _)) = file_name
            .split_once("-randomized_differential-")
            .or_else(|| file_name.split_once("-property-"))
        else {
            continue;
        };
        links
            .entry(benchmark_id.to_string())
            .or_default()
            .push(format!("results/raw/failures/{file_name}"));
    }
    for benchmark_links in links.values_mut() {
        benchmark_links.sort();
    }
    Ok(links)
}

fn render_html(
    rows: &[serde_json::Value],
    toolchains: &Toolchains,
    manifest: &serde_json::Value,
) -> Result<String> {
    let summary = ReportSummary::from_rows(rows);

    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>EVM Compiler Bench Report</title>");
    html.push_str("<style>");
    html.push_str(":root{color-scheme:light;--bg:#f6f7f8;--panel:#fff;--ink:#172033;--muted:#657083;--line:#d9dee7;--soft:#eef2f7;--blue:#2563eb;--green:#0f8a5f;--amber:#b45309;--red:#b91c1c;--purple:#7c3aed}");
    html.push_str("*{box-sizing:border-box}body{font-family:Inter,system-ui,-apple-system,sans-serif;margin:0;background:var(--bg);color:var(--ink)}");
    html.push_str("main{max-width:1320px;margin:0 auto;padding:28px 24px 56px}h1{font-size:34px;margin:0 0 6px;letter-spacing:0}h2{font-size:21px;margin:34px 0 12px}h3{font-size:15px;margin:18px 0 8px}");
    html.push_str("p{line-height:1.45}.meta{color:var(--muted);margin-bottom:16px}.hero{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:18px;margin-bottom:14px}");
    html.push_str(".lede{font-size:16px;max-width:920px;margin:8px 0;color:#273449}.scope{display:grid;grid-template-columns:2fr 1fr;gap:14px;align-items:start}.grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px}");
    html.push_str(".two{display:grid;grid-template-columns:1fr 1fr;gap:14px}.three{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:14px}.card{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:14px}.k{font-size:12px;color:var(--muted);line-height:1.3}.v{font-size:23px;font-weight:700;margin-top:3px}.subv{font-size:12px;color:var(--muted);margin-top:3px}");
    html.push_str("nav{display:flex;flex-wrap:wrap;gap:8px;margin:18px 0 4px}nav a{font-size:12px;text-decoration:none;color:#1d4ed8;background:#e8eefc;border:1px solid #c7d2fe;border-radius:999px;padding:6px 10px}");
    html.push_str("table{width:100%;border-collapse:collapse;background:var(--panel);border:1px solid var(--line);border-radius:8px;overflow:hidden}th,td{font-size:12px;text-align:left;padding:8px 10px;border-bottom:1px solid #edf0f4;vertical-align:top}th{background:#eef1f5;color:#2c3444}tbody tr:hover{background:#fbfcff}");
    html.push_str(".chart{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px;overflow:auto}.vega-chart{min-height:300px}.vega-chart .error{color:#991b1b;font-size:12px}.notice{background:#fff7ed;border:1px solid #fdba74;border-radius:8px;padding:12px;margin:12px 0;color:#7c2d12}.info{background:#eff6ff;border-color:#bfdbfe;color:#1e3a8a}.critical{background:#fff7ed;border-color:#fb923c;color:#7c2d12}.muted{color:var(--muted)}.small{font-size:11px;line-height:1.35}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}");
    html.push_str(".pill{display:inline-flex;align-items:center;border-radius:999px;padding:3px 8px;font-size:11px;border:1px solid var(--line);background:#f9fafb;white-space:nowrap}.pass{background:#ecfdf5;color:#065f46;border-color:#a7f3d0}.warn{background:#fffbeb;color:#92400e;border-color:#fde68a}.fail{background:#fef2f2;color:#991b1b;border-color:#fecaca}.na{background:#f4f4f5;color:#52525b;border-color:#e4e4e7}");
    html.push_str(".bar{height:9px;background:#e5e7eb;border-radius:999px;min-width:92px;position:relative;overflow:hidden}.bar span{display:block;height:100%;border-radius:999px;background:var(--blue)}.bar.good span{background:var(--green)}.bar.warn span{background:var(--amber)}.bar.fail span{background:var(--red)}.ratio{display:grid;grid-template-columns:58px 1fr;gap:8px;align-items:center;min-width:160px}");
    html.push_str(".heat-ok{background:#dcfce7;color:#166534;text-align:center}.heat-fail{background:#fee2e2;color:#991b1b;text-align:center}.heat-na{background:#f4f4f5;color:#71717a;text-align:center}.heat-warn{background:#fef3c7;color:#92400e;text-align:center}.heat-info{background:#dbeafe;color:#1e40af;text-align:center}.legend{display:flex;flex-wrap:wrap;gap:12px;align-items:center;font-size:12px;color:var(--muted);margin:8px 0}.dot{display:inline-block;width:9px;height:9px;border-radius:50%;margin-right:4px}.sol{background:var(--blue)}.vy{background:var(--green)}details{margin-top:12px}details>summary{cursor:pointer;font-weight:700;margin:12px 0}");
    html.push_str(".brief{display:grid;grid-template-columns:1.25fr .75fr;gap:14px;align-items:start}.scope-cards{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:10px;margin-top:12px}.scope-card{border:1px solid var(--line);border-radius:8px;padding:12px;background:#f8fafc}.scope-card strong{display:block;margin-bottom:4px}.findings{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:10px;margin-top:12px}.finding{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px}.finding strong{display:block;margin-bottom:5px}.takeaways{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;margin-top:12px}.takeaway{background:#f8fafc;border:1px solid var(--line);border-radius:8px;padding:12px}.takeaway strong{display:block;margin-bottom:5px}.answer{font-size:18px;font-weight:700;margin:0 0 6px}.section-lede{font-size:13px;color:var(--muted);max-width:900px}.mini-table td,.mini-table th{font-size:12px}.callout{background:#f8fafc;border:1px solid var(--line);border-radius:8px;padding:12px;margin:10px 0}.strip{display:flex;height:22px;border-radius:999px;overflow:hidden;border:1px solid var(--line);background:#eef2f7;margin:8px 0}.seg{display:flex;align-items:center;justify-content:center;min-width:2px;color:#fff;font-size:10px;white-space:nowrap}.seg-fuzz{background:#0f8a5f}.seg-baseline{background:#2563eb}.seg-status{background:#b45309}.seg-fail{background:#b91c1c}.seg-na{background:#6b7280}.chart-grid{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:14px}.svg-card{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px;overflow:auto}.tag{display:inline-flex;border-radius:999px;border:1px solid var(--line);padding:2px 7px;font-size:10px;background:#f9fafb;color:#374151}");
    html.push_str("@media(max-width:900px){.grid,.three,.two,.scope,.brief,.takeaways,.scope-cards,.findings,.chart-grid{grid-template-columns:1fr}main{padding:20px 14px}.v{font-size:20px}}");
    html.push_str("</style>");
    html.push_str(vega_runtime_scripts());
    html.push_str("</head><body><main>");
    html.push_str("<h1>EVM Compiler Bench</h1>");
    html.push_str(&render_run_identity(toolchains, manifest, &summary));
    html.push_str(&render_overview(rows, &summary));
    html.push_str("<nav><a href=\"#scorecards\">Scorecards</a><a href=\"#versions\">Version Axis</a><a href=\"#reliability\">Reliability</a><a href=\"#fixed\">Fixed Suite</a><a href=\"#scale\">Scale Suite</a><a href=\"#real\">Real-Derived</a><a href=\"#profiles\">Profiles</a><a href=\"#benchmarks\">Benchmarks</a><a href=\"#compile\">Compile Resources</a><a href=\"methodology.html\">Methodology</a><a href=\"#raw\">Data Export</a></nav>");

    html.push_str("<section id=\"scorecards\"><h2>Compiler Config Scorecards</h2>");
    html.push_str(&render_config_scorecards(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"versions\"><h2>Compiler Version Axis</h2>");
    html.push_str(&render_version_axis(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"reliability\"><h2>Compile Reliability</h2>");
    html.push_str(&render_reliability_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"fixed\"><h2>Fixed Matched Suite</h2>");
    html.push_str(&render_fixed_suite_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"scale\"><h2>Generated Scale Studies</h2>");
    html.push_str(&render_scale_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"real\"><h2>Real-Derived Benchmark Models</h2>");
    html.push_str(&render_real_derived_summary(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"profiles\"><h2>Compiler Profile Tradeoffs</h2>");
    html.push_str(&render_profile_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"benchmarks\"><h2>Per-Benchmark Summary</h2>");
    html.push_str(&render_benchmark_summary(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"compile\"><h2>Compile Time And RSS</h2>");
    html.push_str(&render_compile_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"raw\"><h2>Data Export</h2>");
    html.push_str(&render_data_export(rows));
    html.push_str("</section></main></body></html>");
    Ok(html)
}

fn vega_runtime_scripts() -> &'static str {
    r##"<script src="assets/vega-6.2.0.min.js"></script><script src="assets/vega-lite-6.4.3.min.js"></script><script src="assets/vega-embed-7.1.0.min.js"></script><script>
window.__evmBenchCharts = [];
function evmBenchChart(id, spec) {
  window.__evmBenchCharts.push({ id: id, spec: spec });
}
document.addEventListener("DOMContentLoaded", function () {
  if (!window.vegaEmbed) {
    document.querySelectorAll(".vega-chart").forEach(function (node) {
      node.innerHTML = "<div class=\"error\">Vega-Lite assets failed to load.</div>";
    });
    return;
  }
  window.__evmBenchCharts.forEach(function (chart) {
    window.vegaEmbed("#" + chart.id, chart.spec, { actions: false, renderer: "svg" })
      .catch(function (error) {
        var node = document.getElementById(chart.id);
        if (node) {
          node.innerHTML = "<div class=\"error\">Chart render failed: " + String(error) + "</div>";
        }
      });
  });
});
</script>"##
}

fn render_methodology_html(
    rows: &[serde_json::Value],
    toolchains: &Toolchains,
    manifest: &serde_json::Value,
) -> Result<String> {
    let summary = ReportSummary::from_rows(rows);
    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>EVM Compiler Bench Methodology</title>");
    html.push_str("<style>");
    html.push_str(":root{color-scheme:light;--bg:#f6f7f8;--panel:#fff;--ink:#172033;--muted:#657083;--line:#d9dee7;--soft:#eef2f7;--blue:#2563eb;--green:#0f8a5f;--amber:#b45309;--red:#b91c1c}");
    html.push_str("*{box-sizing:border-box}body{font-family:Inter,system-ui,-apple-system,sans-serif;margin:0;background:var(--bg);color:var(--ink)}main{max-width:1180px;margin:0 auto;padding:28px 24px 56px}h1{font-size:32px;margin:0 0 6px}h2{font-size:21px;margin:34px 0 12px}h3{font-size:15px;margin:18px 0 8px}p{line-height:1.45}.meta{color:var(--muted);margin-bottom:16px}.lede{font-size:16px;max-width:920px;margin:8px 0;color:#273449}");
    html.push_str("nav{display:flex;flex-wrap:wrap;gap:8px;margin:18px 0 4px}nav a{font-size:12px;text-decoration:none;color:#1d4ed8;background:#e8eefc;border:1px solid #c7d2fe;border-radius:999px;padding:6px 10px}.grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px}.two{display:grid;grid-template-columns:1fr 1fr;gap:14px}.card,.scope-card,.callout{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:14px}.scope-cards{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:10px;margin-top:12px}.scope-card{background:#f8fafc}.scope-card strong{display:block;margin-bottom:4px}");
    html.push_str(".k{font-size:12px;color:var(--muted);line-height:1.3}.v{font-size:23px;font-weight:700;margin-top:3px}.subv{font-size:12px;color:var(--muted);margin-top:3px}.small{font-size:11px;line-height:1.35}.muted{color:var(--muted)}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}.section-lede{font-size:13px;color:var(--muted);max-width:900px}.answer{font-size:18px;font-weight:700;margin:0 0 6px}.pill{display:inline-flex;align-items:center;border-radius:999px;padding:3px 8px;font-size:11px;border:1px solid var(--line);background:#f9fafb;white-space:nowrap}.info{background:#eff6ff;border-color:#bfdbfe;color:#1e3a8a}.critical{background:#fff7ed;border-color:#fb923c;color:#7c2d12}");
    html.push_str("table{width:100%;border-collapse:collapse;background:var(--panel);border:1px solid var(--line);border-radius:8px;overflow:hidden}th,td{font-size:12px;text-align:left;padding:8px 10px;border-bottom:1px solid #edf0f4;vertical-align:top}th{background:#eef1f5;color:#2c3444}.notice{background:#fff7ed;border:1px solid #fdba74;border-radius:8px;padding:12px;margin:12px 0;color:#7c2d12}.bar{height:9px;background:#e5e7eb;border-radius:999px;min-width:92px;position:relative;overflow:hidden}.bar span{display:block;height:100%;border-radius:999px;background:var(--blue)}.bar.good span{background:var(--green)}.bar.warn span{background:var(--amber)}.bar.fail span{background:var(--red)}.ratio{display:grid;grid-template-columns:58px 1fr;gap:8px;align-items:center;min-width:160px}");
    html.push_str(".heat-ok{background:#dcfce7;color:#166534;text-align:center}.heat-fail{background:#fee2e2;color:#991b1b;text-align:center}.heat-na{background:#f4f4f5;color:#71717a;text-align:center}.heat-warn{background:#fef3c7;color:#92400e;text-align:center}.heat-info{background:#dbeafe;color:#1e40af;text-align:center}.legend{display:flex;flex-wrap:wrap;gap:12px;align-items:center;font-size:12px;color:var(--muted);margin:8px 0}.dot{display:inline-block;width:9px;height:9px;border-radius:50%;margin-right:4px}.strip{display:flex;height:22px;border-radius:999px;overflow:hidden;border:1px solid var(--line);background:#eef2f7;margin:8px 0}.seg{display:flex;align-items:center;justify-content:center;min-width:2px;color:#fff;font-size:10px;white-space:nowrap}.seg-fuzz{background:#0f8a5f}.seg-baseline{background:#2563eb}.seg-status{background:#b45309}.seg-fail{background:#b91c1c}.seg-na{background:#6b7280}details{margin-top:12px}details>summary{cursor:pointer;font-weight:700;margin:12px 0}pre{white-space:pre-wrap;overflow:auto;background:#111827;color:#e5e7eb;border-radius:8px;padding:12px}@media(max-width:900px){.grid,.two,.scope-cards{grid-template-columns:1fr}main{padding:20px 14px}.v{font-size:20px}}");
    html.push_str("</style></head><body><main>");
    html.push_str("<h1>EVM Compiler Bench Methodology</h1>");
    html.push_str(&render_run_identity(toolchains, manifest, &summary));
    html.push_str("<p class=\"lede\">This companion file carries the scope, correctness, measurement, and reproducibility details that would otherwise crowd the findings report.</p>");
    html.push_str("<nav><a href=\"index.html\">Findings Report</a><a href=\"#scope\">Scope</a><a href=\"#correctness\">Correctness</a><a href=\"#environment\">Environment</a><a href=\"#data\">Data Export</a></nav>");

    html.push_str("<section id=\"scope\"><h2>Scope Summary</h2>");
    html.push_str(&render_scope_cards(rows, &summary));
    html.push_str(&render_coverage_strip(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"correctness\"><h2>Correctness And Measurement Scope</h2>");
    html.push_str(&render_validity_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"environment\"><h2>Environment And Settings</h2>");
    html.push_str(&render_methodology(rows, toolchains, manifest));
    html.push_str("</section>");

    html.push_str("<section id=\"data\"><h2>Data Export</h2>");
    html.push_str(&render_data_export(rows));
    html.push_str("</section></main></body></html>");
    Ok(html)
}

fn render_run_identity(
    toolchains: &Toolchains,
    manifest: &serde_json::Value,
    summary: &ReportSummary,
) -> String {
    let commit = str_at(manifest, "/environment/git/commit")
        .map(|value| short_hash(&value))
        .unwrap_or_else(|| "unknown".to_string());
    let host = str_at(manifest, "/environment/host/cpu").unwrap_or_else(|| "unknown host".into());
    let started = str_at(manifest, "/started_at").unwrap_or_else(|| "unknown start".into());
    let solc_hash = short_hash(&toolchains.solc.binary_sha256);
    let vyper_hash = short_hash(&toolchains.vyper.binary_sha256);
    let vyper_alpha_hash = short_hash(&toolchains.vyper_alpha.binary_sha256);
    let historical_count = toolchains.compilers.len().saturating_sub(3);
    format!(
        "<div class=\"meta\">commit <span class=\"mono\">{}</span> &middot; {} &middot; started {} &middot; default EVM {} &middot; solc {} (<span class=\"mono\">{}</span>) &middot; Vyper {} (<span class=\"mono\">{}</span>) &middot; Vyper alpha {} (<span class=\"mono\">{}</span>) &middot; {} historical toolchains &middot; {} profiles &middot; {} benchmarks &middot; {} rows</div>",
        escape(&commit),
        escape(&host),
        escape(&started),
        escape(&toolchains.evm_version),
        escape(&toolchains.solc.version),
        escape(&solc_hash),
        escape(&toolchains.vyper.version),
        escape(&vyper_hash),
        escape(&toolchains.vyper_alpha.version),
        escape(&vyper_alpha_hash),
        historical_count,
        summary.profiles.len(),
        summary.benchmarks.len(),
        summary.ok_rows + summary.compile_failures,
    )
}

fn render_overview(rows: &[serde_json::Value], summary: &ReportSummary) -> String {
    let mut html = String::new();
    html.push_str("<section class=\"hero brief\"><div>");
    html.push_str("<div class=\"pill info\">Findings brief</div>");
    html.push_str("<p class=\"lede\">This run compares pinned compiler profiles across fixed matched workloads, generated scale studies, real-derived benchmark models, and historical compiler versions. The main report leads with tradeoffs and version-axis movement; methodology, correctness coverage, profile settings, and scope details live in <a href=\"methodology.html\">methodology.html</a>.</p>");
    html.push_str(&render_signature_findings(rows, summary));
    html.push_str("</div><div class=\"card\">");
    html.push_str("<h3>Run basis</h3>");
    html.push_str("<p class=\"small muted\">Metadata-disabled artifacts where supported, stripped-bytecode comparisons everywhere, per-profile compiler EVM targets, latest pinned solc/Vyper, Vyper 0.5.0a1 alpha, selected historical solc/Vyper releases, Foundry internal-call gas, and per-language baseline ratios.</p>");
    html.push_str("<p class=\"small muted\">The full method, correctness heatmap, profile settings JSON, and limitations are split out so this page can stay focused on comparisons.</p>");
    html.push_str("<p><a class=\"pill info\" href=\"methodology.html\">Open methodology</a></p>");
    html.push_str("</div></section>");

    html.push_str("<section class=\"grid\">");
    html.push_str(&metric_card(
        "Fixed matched suite",
        &format!(
            "{} contracts / {} scenarios",
            summary.fixed_benchmarks.len(),
            summary.fixed_scenarios.len()
        ),
        "strongest cross-language claim class",
    ));
    html.push_str(&metric_card(
        "Generated scale suite",
        &format!(
            "{} families / {} N values",
            summary.scale_families.len(),
            summary.scale_values.len()
        ),
        "compiler stress and failure cliffs",
    ));
    html.push_str(&metric_card(
        "Real-derived models",
        &format!(
            "{} models / {} scenarios",
            summary.real_benchmarks.len(),
            summary.real_scenarios.len()
        ),
        "production_equivalence=false",
    ));
    html.push_str(&metric_card(
        "Profiles",
        &summary.profiles.len().to_string(),
        "compiler and optimizer matrix",
    ));
    html.push_str("</section>");

    html.push_str("<section class=\"grid\">");
    html.push_str(&metric_card(
        "Compile artifacts",
        &format!(
            "{} ok / {} failed",
            summary.successful_artifacts.len(),
            summary.failed_artifacts.len()
        ),
        &format!("{} attempted", summary.attempted_artifacts()),
    ));
    html.push_str(&metric_card(
        "Scenario gas rows",
        &summary.ok_rows.to_string(),
        "successful measured scenario rows",
    ));
    html.push_str(&metric_card(
        "Fixed fuzz+property",
        &format!(
            "{}/{}",
            behavior_fuzz_benchmark_count(rows, "fixed"),
            summary.fixed_benchmarks.len()
        ),
        "benchmarks with strongest behavioral checks",
    ));
    html.push_str(&metric_card(
        "Methodology",
        "separate file",
        "correctness, scope, settings, and limitations",
    ));
    html.push_str("</section>");
    html
}

fn render_config_scorecards(rows: &[serde_json::Value]) -> String {
    let facets = [
        (
            BenchmarkSuite::Fixed.as_str(),
            "Fixed matched suite",
            "Matched handwritten workloads. This is the strongest aggregate comparison surface.",
        ),
        (
            BenchmarkSuite::Scale.as_str(),
            "Generated scale suite",
            "Generated N-family stress tests. Read the ratios together with the compile OK column.",
        ),
        (
            BenchmarkSuite::RealDerived.as_str(),
            "Real-derived models",
            "Recognizable benchmark models with production_equivalence=false.",
        ),
    ];

    let mut html = String::new();
    html.push_str(&format!(
        "<p class=\"section-lede\">Facet-local aggregates give one row per compiler config without creating a global score. Ratios use the same facet, benchmark, scenario, state profile, and artifact basis against <span class=\"mono\">{}</span>; lower is cheaper/smaller/faster. If the baseline does not compile, that point is excluded from ratio cells and still visible in Compile OK.</p>",
        escape(SOL_CODEGEN_BASELINE)
    ));
    for (suite, title, note) in facets {
        html.push_str(&render_facet_scorecard(rows, suite, title, note));
    }
    html
}

#[derive(Debug, Default)]
struct VersionAxisAggregate {
    attempted_artifacts: BTreeSet<String>,
    ok_artifacts: BTreeSet<String>,
    gas_by_benchmark: BTreeMap<String, Vec<f64>>,
    runtime_bytes: Vec<f64>,
    compile_ms: Vec<f64>,
    evm_versions: BTreeSet<String>,
    source_variants: BTreeSet<String>,
    failure_reasons: BTreeMap<String, usize>,
}

fn render_version_axis(rows: &[serde_json::Value]) -> String {
    let aggregates = version_axis_aggregates(rows);
    if aggregates.is_empty() {
        return "<p class=\"section-lede\">No compiler-version axis rows are present in this result set.</p>".to_string();
    }

    let mut chart_values = Vec::new();
    for ((suite, profile), aggregate) in &aggregates {
        let Some(gas_ratio) = geomean_by_group(&aggregate.gas_by_benchmark) else {
            continue;
        };
        chart_values.push(json!({
            "suite": suite,
            "profile": profile_short(profile),
            "profile_id": profile,
            "compiler": compiler_family(profile),
            "compiler_line": compiler_line(profile),
            "config": version_axis_config(profile).unwrap_or_else(|| "other".to_string()),
            "series": version_axis_series(profile),
            "version": compiler_version_label(profile),
            "version_order": compiler_version_order(profile),
            "gas_ratio": gas_ratio,
            "compile_ok": aggregate.ok_artifacts.len(),
            "compile_attempted": aggregate.attempted_artifacts.len(),
        }));
    }

    let spec = json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
        "data": { "values": chart_values },
        "facet": {
            "field": "suite",
            "type": "nominal",
            "columns": 3,
            "title": null,
            "header": {
                "labelFontSize": 12,
                "labelColor": "#374151",
                "labelFontWeight": "bold"
            }
        },
        "spec": {
            "width": 310,
            "height": 230,
            "mark": {
                "type": "line",
                "point": { "filled": true, "size": 48 },
                "strokeWidth": 2
            },
            "encoding": {
                "x": {
                    "field": "version_order",
                    "type": "quantitative",
                    "title": "compiler version",
                    "axis": version_axis_axis()
                },
                "y": {
                    "field": "gas_ratio",
                    "type": "quantitative",
                    "title": "harness gas ratio vs latest same config",
                    "scale": { "zero": false },
                    "axis": { "grid": true, "format": ".2f" }
                },
                "color": version_axis_color_encoding(),
                "detail": { "field": "series", "type": "nominal" },
                "tooltip": [
                    { "field": "suite", "type": "nominal", "title": "Suite" },
                    { "field": "compiler", "type": "nominal", "title": "Compiler" },
                    { "field": "profile_id", "type": "nominal", "title": "Profile" },
                    { "field": "config", "type": "nominal", "title": "Config" },
                    { "field": "version", "type": "nominal", "title": "Version" },
                    { "field": "gas_ratio", "type": "quantitative", "title": "Gas ratio", "format": ".3f" },
                    { "field": "compile_ok", "type": "quantitative", "title": "Compiled artifacts" },
                    { "field": "compile_attempted", "type": "quantitative", "title": "Attempted artifacts" }
                ]
            }
        },
        "config": vega_chart_config()
    });

    let mut html = String::new();
    html.push_str("<p class=\"answer\">How do profiles move across compiler versions?</p>");
    html.push_str("<p class=\"section-lede\">The first chart traces a single benchmark/scenario across compiler versions. Pick a scenario and metric, then read left-to-right within each compiler panel. Historical Solidity and Vyper profiles use explicit source variants and older EVM targets when the old compiler cannot parse or target the latest surface.</p>");
    html.push_str(&render_version_axis_trace_chart(rows));
    html.push_str("<h3>Suite-level ratio summary</h3>");
    html.push_str("<p class=\"small muted\">This aggregate chart compares each compiler version against the latest release on the same compiler line and config where a matching baseline exists. It is a navigation aid, not a global score.</p>");
    html.push_str(&vega_chart("compiler-version-axis-chart", &spec));
    html.push_str(&render_version_axis_table(&aggregates));
    html
}

fn render_version_axis_trace_chart(rows: &[serde_json::Value]) -> String {
    let mut chart_values = Vec::new();
    let mut benchmark_options = BTreeSet::new();
    let mut seen_artifact_metric_rows = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let Some(config) = version_axis_config(&profile) else {
            continue;
        };
        let suite = str_at(row, "/suite").unwrap_or_default();
        let benchmark_scenario = format!("{suite} / {}", scenario_label(row));
        benchmark_options.insert(benchmark_scenario.clone());
        let base = json!({
            "suite": suite,
            "benchmark": str_at(row, "/benchmark_id").unwrap_or_default(),
            "scenario": str_at(row, "/gas/scenario").unwrap_or_default(),
            "state": str_at(row, "/gas/state_access_profile").unwrap_or_default(),
            "benchmark_scenario": benchmark_scenario,
            "profile": profile_short(&profile),
            "profile_id": profile,
            "compiler": compiler_family(&profile),
            "compiler_line": compiler_line(&profile),
            "config": config,
            "series": version_axis_series(&profile),
            "version": compiler_version_label(&profile),
            "version_order": compiler_version_order(&profile),
            "evm": str_at(row, "/compiler/settings/evmVersion").unwrap_or_default(),
            "source_variant": str_at(row, "/compiler/settings/sourceVariant").unwrap_or_default(),
        });
        push_version_axis_metric(
            &mut chart_values,
            &base,
            "harness call gas",
            u64_at(row, "/gas/harness_call_gas") as f64,
        );
        push_version_axis_metric(
            &mut chart_values,
            &base,
            "internal create gas",
            u64_at(row, "/gas/internal_create_gas") as f64,
        );
        let artifact_metric_key = format!(
            "{}\0{}\0{}",
            artifact_key_from_row(row),
            str_at(row, "/gas/scenario").unwrap_or_default(),
            str_at(row, "/gas/state_access_profile").unwrap_or_default()
        );
        if seen_artifact_metric_rows.insert(artifact_metric_key) {
            push_version_axis_metric(
                &mut chart_values,
                &base,
                "runtime bytes stripped",
                u64_at(row, "/bytecode/runtime_bytes_stripped") as f64,
            );
            push_version_axis_metric(
                &mut chart_values,
                &base,
                "compile wall ms",
                mean_f64_array_at(row, "/compile/wall_ms_samples"),
            );
        }
    }
    if chart_values.is_empty() {
        return "<p class=\"section-lede\">No successful metadata-off scenario rows are available for per-benchmark version traces.</p>".to_string();
    }
    let options = benchmark_options.into_iter().collect::<Vec<_>>();
    let default_benchmark = options.first().cloned().unwrap_or_default();
    let spec = json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
        "data": { "values": chart_values },
        "params": [
            {
                "name": "selectedBenchmarkScenario",
                "value": default_benchmark,
                "bind": {
                    "input": "select",
                    "name": "benchmark / scenario ",
                    "options": options
                }
            },
            {
                "name": "selectedMetric",
                "value": "harness call gas",
                "bind": {
                    "input": "select",
                    "name": "metric ",
                    "options": [
                        "harness call gas",
                        "internal create gas",
                        "runtime bytes stripped",
                        "compile wall ms"
                    ]
                }
            }
        ],
        "transform": [
            { "filter": "datum.benchmark_scenario == selectedBenchmarkScenario && datum.metric == selectedMetric" }
        ],
        "facet": {
            "field": "compiler_line",
            "type": "nominal",
            "columns": 2,
            "title": null,
            "header": {
                "labelFontSize": 12,
                "labelColor": "#374151",
                "labelFontWeight": "bold"
            }
        },
        "spec": {
            "width": 430,
            "height": 270,
            "mark": {
                "type": "line",
                "point": { "filled": true, "size": 56 },
                "strokeWidth": 2
            },
            "encoding": {
                "x": {
                    "field": "version_order",
                    "type": "quantitative",
                    "title": "compiler version",
                    "axis": version_axis_axis()
                },
                "y": {
                    "field": "value",
                    "type": "quantitative",
                    "title": "selected metric",
                    "scale": { "zero": false },
                    "axis": { "grid": true, "format": "~s" }
                },
                "color": version_axis_color_encoding(),
                "detail": { "field": "series", "type": "nominal" },
                "tooltip": [
                    { "field": "benchmark_scenario", "type": "nominal", "title": "Benchmark / scenario" },
                    { "field": "metric", "type": "nominal", "title": "Metric" },
                    { "field": "value", "type": "quantitative", "title": "Value", "format": ",.2f" },
                    { "field": "profile_id", "type": "nominal", "title": "Profile" },
                    { "field": "version", "type": "nominal", "title": "Version" },
                    { "field": "config", "type": "nominal", "title": "Config" },
                    { "field": "evm", "type": "nominal", "title": "EVM target" },
                    { "field": "source_variant", "type": "nominal", "title": "Source variant" }
                ]
            }
        },
        "resolve": { "scale": { "x": "independent", "y": "shared" } },
        "config": vega_chart_config()
    });
    vega_chart("compiler-version-trace-chart", &spec)
}

fn push_version_axis_metric(
    values: &mut Vec<serde_json::Value>,
    base: &serde_json::Value,
    metric: &str,
    value: f64,
) {
    if value <= 0.0 {
        return;
    }
    let mut row = base.clone();
    if let Some(object) = row.as_object_mut() {
        object.insert("metric".to_string(), json!(metric));
        object.insert("value".to_string(), json!(value));
    }
    values.push(row);
}

fn version_axis_aggregates(
    rows: &[serde_json::Value],
) -> BTreeMap<(String, String), VersionAxisAggregate> {
    let mut baseline_gas = BTreeMap::new();
    let mut baseline_artifacts = BTreeMap::new();
    let mut seen_baseline_artifacts = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let Some(config) = version_axis_config(&profile) else {
            continue;
        };
        if !is_latest_version_axis_profile(&profile) {
            continue;
        }
        let suite = str_at(row, "/suite").unwrap_or_default();
        let line = compiler_line(&profile);
        baseline_gas.insert(
            format!("{suite}\0{line}\0{config}\0{}", scenario_key(row)),
            u64_at(row, "/gas/harness_call_gas"),
        );
        let artifact_key = artifact_key_from_row(row);
        if seen_baseline_artifacts.insert(artifact_key) {
            baseline_artifacts.insert(
                format!(
                    "{suite}\0{line}\0{config}\0{}",
                    str_at(row, "/benchmark_id").unwrap_or_default()
                ),
                artifact_metrics(row),
            );
        }
    }

    let mut aggregates: BTreeMap<(String, String), VersionAxisAggregate> = BTreeMap::new();
    let mut seen_artifacts_for_metrics = BTreeSet::new();
    for row in rows {
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let Some(config) = version_axis_config(&profile) else {
            continue;
        };
        let suite = str_at(row, "/suite").unwrap_or_default();
        let key = (suite.clone(), profile.clone());
        let artifact_key = artifact_key_from_row(row);
        let aggregate = aggregates.entry(key).or_default();
        aggregate.attempted_artifacts.insert(artifact_key.clone());
        if let Some(evm) = str_at(row, "/compiler/settings/evmVersion") {
            aggregate.evm_versions.insert(evm);
        }
        if let Some(source_variant) = str_at(row, "/compiler/settings/sourceVariant") {
            aggregate.source_variants.insert(source_variant);
        }
        if str_at(row, "/status").as_deref() == Some("compile_error") {
            *aggregate
                .failure_reasons
                .entry(short_error(row))
                .or_default() += 1;
            continue;
        }
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        aggregate.ok_artifacts.insert(artifact_key.clone());
        let line = compiler_line(&profile);
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        if let Some(base_gas) = baseline_gas
            .get(&format!("{suite}\0{line}\0{config}\0{}", scenario_key(row)))
            .copied()
        {
            push_ratio(
                aggregate
                    .gas_by_benchmark
                    .entry(benchmark.clone())
                    .or_default(),
                u64_at(row, "/gas/harness_call_gas"),
                base_gas,
            );
        }
        if seen_artifacts_for_metrics.insert(artifact_key) {
            if let Some(base) = baseline_artifacts
                .get(&format!("{suite}\0{line}\0{config}\0{benchmark}"))
                .copied()
            {
                let current = artifact_metrics(row);
                push_ratio(
                    &mut aggregate.runtime_bytes,
                    current.runtime_bytes,
                    base.runtime_bytes,
                );
                push_float_ratio(
                    &mut aggregate.compile_ms,
                    current.compile_ms,
                    base.compile_ms,
                );
            }
        }
    }
    aggregates
}

fn render_version_axis_table(
    aggregates: &BTreeMap<(String, String), VersionAxisAggregate>,
) -> String {
    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Suite</th><th>Compiler Config</th><th>Compile OK</th><th>Harness Gas</th><th>Runtime Bytes</th><th>Compile Wall</th><th>EVM Target</th><th>Source Variant</th><th>Notes</th></tr></thead><tbody>");
    for ((suite, profile), aggregate) in aggregates {
        html.push_str("<tr><td>");
        html.push_str(&escape(suite));
        html.push_str("</td><td class=\"mono\">");
        html.push_str(&escape(&profile_short(profile)));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(profile));
        html.push_str("</span></td><td>");
        html.push_str(&count_fraction_cell(
            aggregate.ok_artifacts.len(),
            aggregate.attempted_artifacts.len(),
        ));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean_by_group(&aggregate.gas_by_benchmark)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.runtime_bytes)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.compile_ms)));
        html.push_str("</td><td class=\"mono\">");
        html.push_str(&escape(
            &aggregate
                .evm_versions
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        ));
        html.push_str("</td><td class=\"mono\">");
        html.push_str(&escape(
            &aggregate
                .source_variants
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", "),
        ));
        html.push_str("</td><td class=\"small\">");
        html.push_str(&failure_reason_summary(&aggregate.failure_reasons));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn version_axis_config(profile: &str) -> Option<String> {
    if profile.starts_with("solc-") {
        if profile.contains("legacy-runs200") {
            return Some("legacy runs=200".to_string());
        }
        if profile.contains("viair-runs200") {
            return Some("viaIR runs=200".to_string());
        }
        if profile.contains("noopt") {
            return Some("no optimizer".to_string());
        }
    }
    if profile.starts_with("vyper-") {
        if profile.ends_with("-default") {
            return Some("default".to_string());
        }
        let venom = profile.ends_with("-venom");
        for mode in ["none", "gas", "codesize"] {
            let needle = format!("-{mode}");
            if profile.contains(&needle) {
                return Some(if venom {
                    format!("{mode} venom")
                } else {
                    mode.to_string()
                });
            }
        }
    }
    None
}

fn is_latest_version_axis_profile(profile: &str) -> bool {
    profile.starts_with("solc-latest-") || profile.starts_with("vyper-latest-")
}

fn compiler_line(profile: &str) -> &'static str {
    if profile.starts_with("solc-") {
        "solc"
    } else if profile.starts_with("vyper-") {
        "vyper"
    } else {
        "unknown"
    }
}

fn compiler_version_label(profile: &str) -> String {
    if let Some(rest) = profile.strip_prefix("solc-") {
        return rest
            .split_once('-')
            .map(|(version, _)| version.to_string())
            .unwrap_or_else(|| "unknown".to_string());
    }
    if let Some(rest) = profile.strip_prefix("vyper-") {
        return rest
            .split_once('-')
            .map(|(version, _)| version.to_string())
            .unwrap_or_else(|| "unknown".to_string());
    }
    "unknown".to_string()
}

fn compiler_version_order(profile: &str) -> u64 {
    match compiler_version_label(profile).as_str() {
        "latest" if profile.starts_with("solc-") => 835,
        "latest" if profile.starts_with("vyper-") => 430,
        "0.8.30" => 830,
        "0.8.20" => 820,
        "0.8.13" => 813,
        "0.8.0" => 800,
        "0.7.6" => 706,
        "0.6.12" => 612,
        "0.5.17" => 517,
        "0.5.0a1" => 500,
        "0.4.26" => 426,
        "0.4.0" => 400,
        "0.3.10" => 310,
        "0.2.16" => 216,
        _ => 0,
    }
}

fn version_axis_axis() -> serde_json::Value {
    json!({
        "values": [216, 310, 400, 426, 430, 500, 517, 612, 706, 800, 813, 820, 830, 835],
        "labelExpr": "datum.value == 216 ? 'vy 0.2.16' : datum.value == 310 ? 'vy 0.3.10' : datum.value == 400 ? 'vy 0.4.0' : datum.value == 426 ? 'sol 0.4.26' : datum.value == 430 ? 'vy latest' : datum.value == 500 ? 'vy 0.5a1' : datum.value == 517 ? 'sol 0.5.17' : datum.value == 612 ? 'sol 0.6.12' : datum.value == 706 ? 'sol 0.7.6' : datum.value == 800 ? 'sol 0.8.0' : datum.value == 813 ? 'sol 0.8.13' : datum.value == 820 ? 'sol 0.8.20' : datum.value == 830 ? 'sol 0.8.30' : datum.value == 835 ? 'sol latest' : datum.value"
    })
}

fn version_axis_series(profile: &str) -> String {
    let line = compiler_line(profile);
    let config = version_axis_config(profile).unwrap_or_else(|| "other".to_string());
    format!("{line} {config}")
}

fn version_axis_color_encoding() -> serde_json::Value {
    json!({
        "field": "series",
        "type": "nominal",
        "scale": {
            "domain": [
                "solc legacy runs=200",
                "solc viaIR runs=200",
                "solc no optimizer",
                "vyper default",
                "vyper none",
                "vyper gas",
                "vyper codesize",
                "vyper none venom",
                "vyper gas venom",
                "vyper codesize venom"
            ],
            "range": [
                "#2563eb",
                "#1d4ed8",
                "#93c5fd",
                "#6b7280",
                "#86efac",
                "#16a34a",
                "#166534",
                "#4ade80",
                "#15803d",
                "#14532d"
            ]
        },
        "legend": {
            "title": "Compiler config",
            "orient": "bottom",
            "columns": 3
        }
    })
}

#[derive(Debug, Default)]
struct FacetConfigScorecard {
    attempted_artifacts: BTreeSet<String>,
    ok_artifacts: BTreeSet<String>,
    scenario_rows: usize,
    strengths: BTreeMap<&'static str, usize>,
    failure_reasons: BTreeMap<String, usize>,
    gas_by_benchmark: BTreeMap<String, Vec<f64>>,
    runtime_bytes: Vec<f64>,
    internal_create_gas: Vec<f64>,
    compile_ms: Vec<f64>,
    peak_rss: Vec<f64>,
}

fn render_facet_scorecard(
    rows: &[serde_json::Value],
    suite: &str,
    title: &str,
    note: &str,
) -> String {
    let aggregates = facet_scorecard_aggregates(rows, suite);
    if aggregates.is_empty() {
        return String::new();
    }

    let mut html = String::new();
    html.push_str("<h3>");
    html.push_str(&escape(title));
    html.push_str("</h3><p class=\"small muted\">");
    html.push_str(&escape(note));
    html.push_str("</p>");
    html.push_str("<table><thead><tr><th>Compiler Config</th><th>Compile OK</th><th>Scenario Rows</th><th>Harness Gas</th><th>Runtime Bytes</th><th>Internal Create</th><th>Compile Wall</th><th>Peak RSS</th><th>Correctness Mix</th><th>Notes</th></tr></thead><tbody>");
    for profile in ordered_scorecard_profiles(&aggregates) {
        let Some(aggregate) = aggregates.get(&profile) else {
            continue;
        };
        let gas_ratio = geomean_by_group(&aggregate.gas_by_benchmark);
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile_short(&profile)));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(&profile));
        html.push_str("</span></td><td>");
        html.push_str(&count_fraction_cell(
            aggregate.ok_artifacts.len(),
            aggregate.attempted_artifacts.len(),
        ));
        html.push_str("</td><td>");
        html.push_str(&aggregate.scenario_rows.to_string());
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(gas_ratio));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.runtime_bytes)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.internal_create_gas)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.compile_ms)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.peak_rss)));
        html.push_str("</td><td>");
        html.push_str(&strength_mix_cell(&aggregate.strengths));
        html.push_str("</td><td class=\"small\">");
        html.push_str(&failure_reason_summary(&aggregate.failure_reasons));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn facet_scorecard_aggregates(
    rows: &[serde_json::Value],
    suite: &str,
) -> BTreeMap<String, FacetConfigScorecard> {
    let mut baseline_gas = BTreeMap::new();
    let mut baseline_artifacts = BTreeMap::new();
    let mut seen_baseline_artifacts = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/suite").as_deref() != Some(suite)
            || str_at(row, "/profile_id").as_deref() != Some(SOL_CODEGEN_BASELINE)
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        baseline_gas.insert(scenario_key(row), u64_at(row, "/gas/harness_call_gas"));
        let artifact_key = artifact_key_from_row(row);
        if seen_baseline_artifacts.insert(artifact_key) {
            baseline_artifacts.insert(
                str_at(row, "/benchmark_id").unwrap_or_default(),
                artifact_metrics(row),
            );
        }
    }

    let mut aggregates: BTreeMap<String, FacetConfigScorecard> = BTreeMap::new();
    let mut seen_artifacts_for_metrics = BTreeSet::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some(suite) {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        if profile.is_empty() {
            continue;
        }
        let artifact_key = artifact_key_from_row(row);
        let aggregate = aggregates.entry(profile.clone()).or_default();
        aggregate.attempted_artifacts.insert(artifact_key.clone());

        if str_at(row, "/status").as_deref() == Some("compile_error") {
            *aggregate
                .failure_reasons
                .entry(short_error(row))
                .or_default() += 1;
            *aggregate.strengths.entry(row_strength(row)).or_default() += 1;
            continue;
        }

        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }

        aggregate.ok_artifacts.insert(artifact_key.clone());
        aggregate.scenario_rows += 1;
        *aggregate.strengths.entry(row_strength(row)).or_default() += 1;

        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        if let Some(base_gas) = baseline_gas.get(&scenario_key(row)).copied() {
            push_ratio(
                aggregate
                    .gas_by_benchmark
                    .entry(benchmark.clone())
                    .or_default(),
                u64_at(row, "/gas/harness_call_gas"),
                base_gas,
            );
        }

        if seen_artifacts_for_metrics.insert(artifact_key) {
            let Some(base) = baseline_artifacts.get(&benchmark).copied() else {
                continue;
            };
            let current = artifact_metrics(row);
            push_ratio(
                &mut aggregate.runtime_bytes,
                current.runtime_bytes,
                base.runtime_bytes,
            );
            push_ratio(
                &mut aggregate.internal_create_gas,
                current.internal_create_gas,
                base.internal_create_gas,
            );
            push_float_ratio(
                &mut aggregate.compile_ms,
                current.compile_ms,
                base.compile_ms,
            );
            push_float_ratio(
                &mut aggregate.peak_rss,
                current.peak_rss_kib as f64,
                base.peak_rss_kib as f64,
            );
        }
    }
    aggregates
}

fn ordered_scorecard_profiles(aggregates: &BTreeMap<String, FacetConfigScorecard>) -> Vec<String> {
    let mut profiles = Vec::new();
    for profile in SOL_CODEGEN_PROFILES
        .iter()
        .chain(VYPER_CODEGEN_PROFILES.iter())
    {
        if aggregates.contains_key(*profile) {
            profiles.push((*profile).to_string());
        }
    }
    for profile in aggregates.keys() {
        if !profiles.iter().any(|known| known == profile) {
            profiles.push(profile.clone());
        }
    }
    profiles
}

fn geomean_by_group(groups: &BTreeMap<String, Vec<f64>>) -> Option<f64> {
    let values = groups
        .values()
        .filter_map(|group| geomean(group))
        .collect::<Vec<_>>();
    geomean(&values)
}

fn count_fraction_cell(ok: usize, attempted: usize) -> String {
    if attempted == 0 {
        return "<span class=\"pill na\">n/a</span>".to_string();
    }
    let ratio = ok as f64 / attempted as f64;
    let width = (ratio * 100.0).clamp(2.0, 100.0);
    let class = if ok == attempted {
        "good"
    } else if ratio >= 0.95 {
        "warn"
    } else {
        "fail"
    };
    format!(
        "<div class=\"ratio\"><span>{ok}/{attempted}</span><div class=\"bar {class}\"><span style=\"width:{width:.1}%\"></span></div></div>"
    )
}

fn strength_mix_cell(counts: &BTreeMap<&'static str, usize>) -> String {
    if counts.is_empty() {
        return "<span class=\"pill na\">n/a</span>".to_string();
    }
    let mut parts = Vec::new();
    for (strength, label) in [
        ("behavior-fuzz", "fuzz"),
        ("baseline-smoke", "baseline"),
        ("status-only", "status"),
        ("compile-fail", "fail"),
        ("not-applicable", "n/a"),
    ] {
        let count = counts.get(strength).copied().unwrap_or(0);
        if count > 0 {
            parts.push(format!("{} {}", count, label));
        }
    }
    escape(&parts.join(" / "))
}

fn failure_reason_summary(reasons: &BTreeMap<String, usize>) -> String {
    if reasons.is_empty() {
        return "<span class=\"pill pass\">no compile failures</span>".to_string();
    }
    reasons
        .iter()
        .map(|(reason, count)| format!("{} x{}", escape(reason), count))
        .collect::<Vec<_>>()
        .join("<br>")
}

fn render_scope_cards(rows: &[serde_json::Value], summary: &ReportSummary) -> String {
    let strong_fixed = behavior_fuzz_benchmark_count(rows, "fixed");
    let fixed_count = summary.fixed_benchmarks.len();

    let mut html = String::new();
    html.push_str("<div class=\"scope-cards\">");
    html.push_str("<div class=\"scope-card critical\"><strong>Measurement</strong><span class=\"small\">Runtime gas is <span class=\"mono\">harness_call_gas</span>, not signed transaction gas. Calldata and intrinsic estimates are separate columns; EIP-7623 effects are not folded into the headline.</span></div>");
    html.push_str("<div class=\"scope-card\"><strong>Correctness</strong><span class=\"small\">");
    html.push_str(&format!(
        "{strong_fixed}/{fixed_count} fixed benchmarks have fuzz+property checks. The rest of the successful rows are status/baseline-smoke coverage; golden and log checks are not run."
    ));
    html.push_str("</span></div><div class=\"scope-card\"><strong>Bytecode basis</strong><span class=\"small\">Compiler metadata is disabled in the active profile matrix. Size comparisons use no-metadata artifacts and stripped bytecode metrics where available, so metadata is not a benchmark dimension.</span></div>");
    html.push_str("</div>");
    html
}

fn render_coverage_strip(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"callout\"><strong>Coverage by workload class</strong>");
    for suite in ["fixed", "scale", "real_derived"] {
        html.push_str("<div class=\"small muted\">");
        html.push_str(&escape(suite));
        html.push_str("</div>");
        html.push_str(&coverage_strip_for_suite(rows, suite));
    }
    html.push_str("<div class=\"legend\"><span><span class=\"dot seg-fuzz\"></span>fuzz+property</span><span><span class=\"dot seg-baseline\"></span>baseline-smoke</span><span><span class=\"dot seg-status\"></span>status-only</span><span><span class=\"dot seg-fail\"></span>compile-fail/fail</span></div>");
    html.push_str("</div>");
    html
}

fn render_signature_findings(rows: &[serde_json::Value], summary: &ReportSummary) -> String {
    let dispatch = dispatch_signature(rows);
    let mut html = String::new();
    html.push_str("<div class=\"findings\">");
    html.push_str("<div class=\"finding\"><strong>Codegen cliff</strong><span class=\"small\">");
    html.push_str(&escape(&compile_failure_story(rows)));
    html.push_str("</span></div>");
    html.push_str("<div class=\"finding\"><strong>Scale behavior</strong><span class=\"small\">");
    html.push_str(&escape(&dispatch));
    html.push_str("</span></div>");
    html.push_str("<div class=\"finding\"><strong>Codegen basis</strong><span class=\"small\">Profiles disable bytecode metadata when the compiler supports it, and compare stripped bytecode across all versions. Metadata deltas are no longer measured or aggregated.</span></div>");
    html.push_str("</div>");
    html.push_str(&format!(
        "<p class=\"small muted\">Inventory: {} successful scenario rows, {} compile failures, {} profiles. No composite score is computed.</p>",
        summary.ok_rows, summary.compile_failures, summary.profiles.len()
    ));
    html
}

fn render_validity_brief(rows: &[serde_json::Value]) -> String {
    let summary = ReportSummary::from_rows(rows);
    let fixed_fuzz = behavior_fuzz_benchmark_count(rows, "fixed");
    let mut html = String::new();
    html.push_str("<p class=\"answer\">Can I trust the rows enough to compare gas and size?</p>");
    html.push_str(&format!(
        "<p class=\"section-lede\">Enough for scoped compiler-profile tradeoffs, not enough for a global language or production-protocol claim. Cross-language behavioral confidence is strongest for {}/{} fixed benchmarks with fuzz+property coverage; other successful rows are status or baseline-smoke checks and should be read with lower claim strength.</p>",
        fixed_fuzz,
        summary.fixed_benchmarks.len()
    ));
    html.push_str("<section class=\"grid\">");
    html.push_str(&metric_card(
        "Scenario status",
        &format!(
            "{} pass / {} fail",
            summary.scenario_status_pass, summary.scenario_status_fail
        ),
        "success or revert matched expectation",
    ));
    html.push_str(&metric_card(
        "Baseline differential",
        &format!("{} rows", summary.baseline_differential_rows),
        "baseline pair coverage, not every profile pair",
    ));
    html.push_str(&metric_card(
        "Randomized/property",
        &format!("{} / {}", summary.randomized_rows, summary.property_rows),
        "selected fixed benchmarks",
    ));
    html.push_str(&metric_card(
        "Golden/log checks",
        &format!("{} / {}", summary.golden_rows, summary.log_rows),
        "not run in this result set",
    ));
    html.push_str("</section>");
    html.push_str(&render_measurement_scope());
    html.push_str("<h3>Correctness Heatmap</h3>");
    html.push_str(&render_correctness_heatmap(rows));
    html.push_str("<details><summary>Show correctness coverage matrix</summary>");
    html.push_str(&render_correctness_matrix(rows));
    html.push_str("</details>");
    html
}

fn render_reliability_brief(rows: &[serde_json::Value]) -> String {
    let summary = ReportSummary::from_rows(rows);
    let mut html = String::new();
    html.push_str("<p class=\"answer\">Did everything compile?</p>");
    html.push_str("<p class=\"section-lede\">");
    html.push_str(&escape(&compile_failure_story(rows)));
    html.push_str("</p><section class=\"grid\">");
    html.push_str(&metric_card(
        "Compiled artifacts",
        &summary.successful_artifacts.len().to_string(),
        "successful compiler/profile artifacts",
    ));
    html.push_str(&metric_card(
        "Compile failures",
        &summary.failed_artifacts.len().to_string(),
        "kept as first-class benchmark results",
    ));
    html.push_str(&metric_card(
        "Affected suite",
        &compile_failure_suite_label(rows),
        "where compiler errors occurred",
    ));
    html.push_str(&metric_card(
        "Failure reason",
        &compile_failure_reason_label(rows),
        "summarized from compiler diagnostics",
    ));
    html.push_str("</section>");
    html.push_str(&render_compile_failure_brief_table(rows));
    html.push_str("<details><summary>Show full compile matrix</summary>");
    html.push_str(&render_compile_failure_matrix(rows));
    html.push_str("</details>");
    html
}

fn render_fixed_suite_brief(rows: &[serde_json::Value]) -> String {
    let sol_deltas = collect_language_scenario_deltas(
        rows,
        "fixed",
        "solidity",
        SOL_CODEGEN_BASELINE,
        &[SOL_VIAIR_CODEGEN, SOL_NOOPT_CODEGEN],
    );
    let vyper_deltas = collect_language_scenario_deltas(
        rows,
        "fixed",
        "vyper",
        VYPER_GAS_CODEGEN,
        &VYPER_DELTA_PROFILES,
    );
    let mut sol_changed = sol_deltas.clone();
    sol_changed.sort_by(|left, right| {
        right
            .ratio
            .ln()
            .abs()
            .partial_cmp(&left.ratio.ln().abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut vyper_changed = vyper_deltas.clone();
    vyper_changed.sort_by(|left, right| {
        right
            .ratio
            .ln()
            .abs()
            .partial_cmp(&left.ratio.ln().abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut cross_language = collect_scenario_deltas(
        rows,
        "fixed",
        &[
            VYPER_GAS_CODEGEN,
            VYPER_ALPHA_GAS_CODEGEN,
            VYPER_GAS_VENOM_CODEGEN,
            VYPER_ALPHA_GAS_VENOM_CODEGEN,
            VYPER_CODESIZE_CODEGEN,
            VYPER_ALPHA_CODESIZE_CODEGEN,
            VYPER_CODESIZE_VENOM_CODEGEN,
            VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
            VYPER_NONE_CODEGEN,
            VYPER_ALPHA_NONE_CODEGEN,
            VYPER_NONE_VENOM_CODEGEN,
            VYPER_ALPHA_NONE_VENOM_CODEGEN,
        ],
    )
    .into_iter()
    .filter(|delta| delta.profile.starts_with("vyper-"))
    .collect::<Vec<_>>();
    cross_language.sort_by(|left, right| {
        right
            .ratio
            .ln()
            .abs()
            .partial_cmp(&left.ratio.ln().abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut cheaper = sol_deltas
        .iter()
        .chain(vyper_deltas.iter())
        .filter(|delta| delta.ratio < 1.0)
        .cloned()
        .collect::<Vec<_>>();
    cheaper.sort_by(|left, right| {
        left.ratio
            .partial_cmp(&right.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut costlier = sol_deltas
        .iter()
        .chain(vyper_deltas.iter())
        .filter(|delta| delta.ratio > 1.0)
        .cloned()
        .collect::<Vec<_>>();
    costlier.sort_by(|left, right| {
        right
            .ratio
            .partial_cmp(&left.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut html = String::new();
    html.push_str("<p class=\"answer\">Where does runtime gas actually move?</p>");
    html.push_str(&format!(
        "<p class=\"section-lede\">Baseline ratios are within each language: Solidity rows compare to <span class=\"mono\">{}</span>, Vyper rows compare to <span class=\"mono\">{}</span>. Cross-language numbers are kept as side-by-side raw values or an explicitly labelled optional drilldown.</p>",
        escape(SOL_CODEGEN_BASELINE),
        escape(VYPER_GAS_CODEGEN)
    ));
    html.push_str("<section class=\"two\">");
    html.push_str(&render_delta_table(
        "Largest Solidity profile moves",
        &sol_changed,
        8,
    ));
    html.push_str(&render_delta_table(
        "Largest Vyper profile moves",
        &vyper_changed,
        8,
    ));
    html.push_str("</section>");
    html.push_str("<section class=\"two\">");
    html.push_str(&render_delta_table("Cheaper vs own baseline", &cheaper, 6));
    html.push_str(&render_delta_table(
        "Costlier vs own baseline",
        &costlier,
        6,
    ));
    html.push_str("</section>");
    html.push_str(&render_fixed_benchmark_summary(rows));
    html.push_str(&render_fixed_pareto_chart(rows));
    html.push_str(
        "<details><summary>Show cross-language ratios against Solidity baseline</summary>",
    );
    html.push_str("<p class=\"small muted\">This drilldown is intentionally labelled cross-language. It compares matched fixed-suite scenario rows against the Solidity legacy baseline and should not be aggregated into a language score.</p>");
    html.push_str(&render_delta_table(
        "Largest cross-language fixed-suite deltas",
        &cross_language,
        20,
    ));
    html.push_str("</details>");
    html.push_str("<details><summary>Show all fixed-suite scenario ratios</summary>");
    html.push_str(&render_fixed_scenario_ratios(rows));
    html.push_str("</details>");
    html
}

fn render_profile_brief(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<p class=\"answer\">What do the main compiler profiles buy or cost?</p>");
    html.push_str("<p class=\"section-lede\">The default profile view compares Solidity legacy/viaIR against the full Vyper matrix: stable and 0.5.0a1 alpha, each crossed with none/gas/codesize optimization and Venom on/off. Solidity noopt diagnostics remain available in the full profile table below.</p>");
    html.push_str(&render_profile_tradeoff_table(
        rows,
        Some(&PRIMARY_CODEGEN_PROFILES),
    ));
    html.push_str(
        "<details><summary>Show all profile tradeoffs, including noopt diagnostics</summary>",
    );
    html.push_str(&render_fixed_profile_tradeoffs(rows));
    html.push_str("</details>");
    html
}

fn render_compile_brief(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<p class=\"answer\">What are the compile-time and memory tradeoffs?</p>");
    html.push_str("<p class=\"section-lede\">Compile resources are artifact-level measurements. Wall time has three samples per successful artifact and is shown with CV; RSS is one coarse peak sample, so small memory differences should not be over-read.</p>");
    html.push_str(&render_compile_resource_summary_for_profiles(
        rows,
        Some(&PRIMARY_CODEGEN_PROFILES),
    ));
    html.push_str("<details><summary>Show compile resources for all profiles</summary>");
    html.push_str(&render_compile_resource_summary(rows));
    html.push_str("</details>");
    html
}

fn render_scale_brief(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<p class=\"answer\">Where do generated workloads expose compiler cliffs?</p>");
    html.push_str("<p class=\"section-lede\">Scale workloads are stress tests. They are best read as failure boundaries and growth curves, not as application-like gas averages.</p>");
    html.push_str(&render_compile_failure_brief_table(rows));
    html.push_str(&render_scale_failure_surface(rows));
    html.push_str(&render_scale_curve_panels(rows));
    html.push_str(&render_scale_family_summary(rows));
    html
}

fn render_correctness_matrix(rows: &[serde_json::Value]) -> String {
    let suites = ["fixed", "scale", "real_derived"];
    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Workload Class</th><th>Compile Artifacts</th><th>Scenario Status</th><th>Baseline Differential</th><th>Randomized Differential</th><th>Property Checks</th><th>Golden Behavior</th><th>Log Checks</th></tr></thead><tbody>");
    for suite in suites {
        let mut ok_artifacts = BTreeSet::new();
        let mut failed_artifacts = BTreeSet::new();
        let mut ok_rows = 0usize;
        let mut status_pass = 0usize;
        let mut baseline = 0usize;
        let mut randomized = 0usize;
        let mut property = 0usize;
        let mut golden = 0usize;
        let mut logs = 0usize;
        for row in rows {
            if str_at(row, "/suite").as_deref() != Some(suite) {
                continue;
            }
            match str_at(row, "/status").as_deref() {
                Some("ok") => {
                    ok_rows += 1;
                    ok_artifacts.insert(artifact_key_from_row(row));
                    if str_at(row, "/correctness/scenario_status_check").as_deref() == Some("pass")
                    {
                        status_pass += 1;
                    }
                    if str_at(row, "/correctness/baseline_differential_check").as_deref()
                        == Some("baseline_only")
                    {
                        baseline += 1;
                    }
                    if str_at(row, "/correctness/randomized_differential_check").as_deref()
                        == Some("pass")
                    {
                        randomized += 1;
                    }
                    if str_at(row, "/correctness/property_tests").as_deref() == Some("pass") {
                        property += 1;
                    }
                    if !matches!(
                        str_at(row, "/correctness/golden_behavior_check").as_deref(),
                        Some("not_run" | "not_applicable") | None
                    ) {
                        golden += 1;
                    }
                    if !matches!(
                        str_at(row, "/correctness/log_check").as_deref(),
                        Some("not_run" | "not_applicable") | None
                    ) {
                        logs += 1;
                    }
                }
                Some("compile_error") => {
                    failed_artifacts.insert(artifact_key_from_row(row));
                }
                _ => {}
            }
        }
        let attempts = ok_artifacts.len() + failed_artifacts.len();
        html.push_str("<tr><td>");
        html.push_str(&escape(suite));
        html.push_str("</td><td>");
        html.push_str(&format!(
            "{} ok / {} failed / {} attempted",
            ok_artifacts.len(),
            failed_artifacts.len(),
            attempts
        ));
        html.push_str("</td><td>");
        html.push_str(&format!("{status_pass}/{ok_rows} rows"));
        html.push_str("</td><td>");
        html.push_str(&format!("{baseline}/{ok_rows} rows, baseline pair only"));
        html.push_str("</td><td>");
        html.push_str(&format!("{randomized}/{ok_rows} rows"));
        html.push_str("</td><td>");
        html.push_str(&format!("{property}/{ok_rows} rows"));
        html.push_str("</td><td>");
        html.push_str(&format!("{golden}/{ok_rows} rows"));
        html.push_str("</td><td>");
        html.push_str(&format!("{logs}/{ok_rows} rows"));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_measurement_scope() -> String {
    let mut html = String::new();
    html.push_str("<div class=\"notice small critical\"><strong>Measurement semantics:</strong> <span class=\"mono\">scenario_status_check=pass</span> means the call succeeded or reverted as expected. It is not exact semantic equivalence unless paired with return, observer, log, randomized, property, or golden checks. <span class=\"mono\">harness_call_gas</span> is Foundry internal-call gas; transaction calldata pricing is separate and EIP-7623 effects are not included in a measured top-level total.</div>");
    html.push_str("<div class=\"three\">");
    html.push_str(&metric_card(
        "harness_call_gas",
        "Measured",
        "gas used by the harness target call",
    ));
    html.push_str(&metric_card(
        "internal_create_gas",
        "Measured",
        "deployment inside the Foundry harness",
    ));
    html.push_str(&metric_card(
        "total_tx_gas",
        "Not measured",
        "null until a top-level transaction runner exists",
    ));
    html.push_str("</div>");
    html
}

fn render_compile_failure_matrix(rows: &[serde_json::Value]) -> String {
    let mut profiles = BTreeSet::new();
    let mut groups: BTreeMap<String, (String, BTreeMap<String, (&'static str, String)>)> =
        BTreeMap::new();
    for row in rows {
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        if profile.is_empty() {
            continue;
        }
        profiles.insert(profile.clone());
        let suite = str_at(row, "/suite").unwrap_or_default();
        let (sort_key, label) = if suite == "scale" {
            let family = str_at(row, "/family").unwrap_or_default();
            let value = u64_at(row, "/parameter_value");
            (
                format!("2\0{family}\0{value:020}"),
                format!("scale / {family} / N={value}"),
            )
        } else {
            let rank = if suite == "fixed" { "0" } else { "1" };
            let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
            (
                format!("{rank}\0{suite}\0{benchmark}"),
                format!("{suite} / {benchmark}"),
            )
        };
        let status = str_at(row, "/status").unwrap_or_default();
        let cell = if status == "compile_error" {
            ("fail", short_error(row))
        } else {
            ("ok", String::new())
        };
        groups
            .entry(sort_key)
            .or_insert_with(|| (label, BTreeMap::new()))
            .1
            .insert(profile, cell);
    }

    let mut html = String::new();
    html.push_str("<div class=\"legend\"><span><span class=\"pill pass\">ok</span> compiled</span><span><span class=\"pill fail\">fail</span> compiler error</span><span><span class=\"pill na\">n/a</span> profile not applicable</span></div>");
    html.push_str("<div class=\"chart\"><table><thead><tr><th>Workload</th>");
    for profile in &profiles {
        html.push_str("<th>");
        html.push_str(&escape(profile));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (_, (group, cells)) in groups {
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&group));
        html.push_str("</td>");
        for profile in &profiles {
            match cells.get(profile) {
                Some(("ok", _)) => html.push_str("<td class=\"heat-ok\">ok</td>"),
                Some(("fail", error)) => {
                    html.push_str("<td class=\"heat-fail\" title=\"");
                    html.push_str(&escape(error));
                    html.push_str("\">fail</td>");
                }
                _ => html.push_str("<td class=\"heat-na\">n/a</td>"),
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

fn render_fixed_scenario_ratios(rows: &[serde_json::Value]) -> String {
    let selected_profiles = [
        SOL_VIAIR_CODEGEN,
        SOL_NOOPT_CODEGEN,
        VYPER_ALPHA_GAS_CODEGEN,
        VYPER_GAS_VENOM_CODEGEN,
        VYPER_ALPHA_GAS_VENOM_CODEGEN,
        VYPER_CODESIZE_CODEGEN,
        VYPER_ALPHA_CODESIZE_CODEGEN,
        VYPER_CODESIZE_VENOM_CODEGEN,
        VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
        VYPER_NONE_CODEGEN,
        VYPER_ALPHA_NONE_CODEGEN,
        VYPER_NONE_VENOM_CODEGEN,
        VYPER_ALPHA_NONE_VENOM_CODEGEN,
    ];
    let mut baseline = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for row in rows {
        if !is_ok_fixed_metadata_off(row) {
            continue;
        }
        let language = str_at(row, "/language").unwrap_or_default();
        let key = format!("{}\0{}", language, scenario_key(row));
        labels
            .entry(key.clone())
            .or_insert_with(|| format!("{language} / {}", scenario_label(row)));
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let gas = u64_at(row, "/gas/harness_call_gas");
        if baseline_profile_for_language(&language) == Some(profile.as_str()) {
            baseline.insert(key.clone(), gas);
        }
        values.entry(key).or_default().insert(profile, gas);
    }

    let mut html = String::new();
    html.push_str("<h3>Scenario Gas Ratios</h3>");
    html.push_str(&format!(
        "<p class=\"small muted\">Ratios compare the same benchmark, scenario, state profile, and language against that language's baseline: Solidity <span class=\"mono\">{}</span>, Vyper <span class=\"mono\">{}</span>. Lower is less harness call gas.</p>",
        escape(SOL_CODEGEN_BASELINE),
        escape(VYPER_GAS_CODEGEN)
    ));
    html.push_str("<table><thead><tr><th>Scenario</th>");
    for profile in selected_profiles {
        html.push_str("<th>");
        html.push_str(&escape(profile));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (key, label) in labels {
        let base = baseline.get(&key).copied();
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&label));
        html.push_str("</td>");
        for profile in selected_profiles {
            let ratio = base.and_then(|base| {
                values
                    .get(&key)
                    .and_then(|by_profile| by_profile.get(profile))
                    .map(|value| (*value as f64, base as f64))
            });
            html.push_str("<td>");
            html.push_str(&ratio_metric_cell(ratio));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_fixed_pareto_chart(rows: &[serde_json::Value]) -> String {
    let values = rows
        .iter()
        .filter(|row| {
            str_at(row, "/status").as_deref() == Some("ok")
                && str_at(row, "/suite").as_deref() == Some("fixed")
                && str_at(row, "/gas/metadata_mode").as_deref() == Some("off")
        })
        .map(|row| {
            let profile = str_at(row, "/profile_id").unwrap_or_default();
            json!({
                "language": str_at(row, "/language").unwrap_or_default(),
                "compiler": compiler_family(&profile),
                "profile": profile_short(&profile),
                "profile_id": profile,
                "benchmark": str_at(row, "/benchmark_id").unwrap_or_default(),
                "scenario": str_at(row, "/gas/scenario").unwrap_or_default(),
                "state": str_at(row, "/gas/state_access_profile").unwrap_or_default(),
                "runtime_bytes": u64_at(row, "/bytecode/runtime_bytes"),
                "harness_call_gas": u64_at(row, "/gas/harness_call_gas")
            })
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        return String::new();
    }
    let spec = json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
        "data": { "values": values },
        "facet": {
            "field": "language",
            "type": "nominal",
            "columns": 2,
            "title": null,
            "header": {
                "labelFontSize": 14,
                "labelFontWeight": "bold",
                "labelColor": "#172033"
            }
        },
        "spec": {
            "width": 480,
            "height": 310,
            "mark": {
                "type": "point",
                "filled": true,
                "opacity": 0.72,
                "size": 42
            },
            "encoding": {
                "x": {
                    "field": "runtime_bytes",
                    "type": "quantitative",
                    "title": "runtime bytes",
                    "scale": { "zero": true },
                    "axis": { "grid": true }
                },
                "y": {
                    "field": "harness_call_gas",
                    "type": "quantitative",
                    "title": "harness call gas",
                    "scale": { "zero": true },
                    "axis": { "grid": true }
                },
                "color": profile_color_encoding(false),
                "tooltip": [
                    { "field": "benchmark", "type": "nominal", "title": "Benchmark" },
                    { "field": "scenario", "type": "nominal", "title": "Scenario" },
                    { "field": "state", "type": "nominal", "title": "State" },
                    { "field": "compiler", "type": "nominal", "title": "Compiler" },
                    { "field": "profile_id", "type": "nominal", "title": "Profile" },
                    { "field": "runtime_bytes", "type": "quantitative", "title": "Runtime bytes", "format": "," },
                    { "field": "harness_call_gas", "type": "quantitative", "title": "Harness call gas", "format": "," }
                ]
            }
        },
        "resolve": { "scale": { "x": "shared", "y": "shared" } },
        "config": vega_chart_config()
    });
    let mut html = String::new();
    html.push_str("<h3>Fixed-Suite Pareto Surface</h3>");
    html.push_str("<div class=\"legend\"><span>faceted by language</span><span>colour = compiler family</span><span>shade = optimizer/codegen config</span><span>all points use metadata-disabled artifacts</span></div>");
    html.push_str(&compiler_palette_legend());
    html.push_str(&vega_chart("fixed-pareto-chart", &spec));
    html
}

#[derive(Debug, Default)]
struct RatioAggregate {
    gas: Vec<f64>,
    runtime_bytes: Vec<f64>,
    internal_create_gas: Vec<f64>,
    compile_ms: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
struct ArtifactMetrics {
    runtime_bytes: u64,
    internal_create_gas: u64,
    compile_ms: f64,
    peak_rss_kib: u64,
}

#[derive(Debug, Clone)]
struct ScenarioDelta {
    label: String,
    profile: String,
    ratio: f64,
    delta: i64,
    value: u64,
    baseline: u64,
}

fn collect_scenario_deltas(
    rows: &[serde_json::Value],
    suite: &str,
    profiles: &[&str],
) -> Vec<ScenarioDelta> {
    let mut baseline = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/suite").as_deref() != Some(suite)
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let key = scenario_key(row);
        labels
            .entry(key.clone())
            .or_insert_with(|| scenario_label(row));
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let gas = u64_at(row, "/gas/harness_call_gas");
        if profile == SOL_CODEGEN_BASELINE {
            baseline.insert(key.clone(), gas);
        }
        if profiles.iter().any(|candidate| *candidate == profile) {
            values.entry(key).or_default().insert(profile, gas);
        }
    }

    let mut deltas = Vec::new();
    for (key, by_profile) in values {
        let Some(base) = baseline.get(&key).copied() else {
            continue;
        };
        if base == 0 {
            continue;
        }
        let label = labels.get(&key).cloned().unwrap_or(key);
        for (profile, value) in by_profile {
            if profile == SOL_CODEGEN_BASELINE || value == 0 {
                continue;
            }
            deltas.push(ScenarioDelta {
                label: label.clone(),
                profile,
                ratio: value as f64 / base as f64,
                delta: value as i64 - base as i64,
                value,
                baseline: base,
            });
        }
    }
    deltas
}

fn render_delta_table(title: &str, deltas: &[ScenarioDelta], limit: usize) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"card\"><h3>");
    html.push_str(&escape(title));
    html.push_str("</h3>");
    if deltas.is_empty() {
        html.push_str("<p class=\"small muted\">No rows in this direction for the selected profiles.</p></div>");
        return html;
    }
    html.push_str("<table class=\"mini-table\"><thead><tr><th>Scenario</th><th>Profile</th><th>Ratio</th><th>Delta Gas</th></tr></thead><tbody>");
    for delta in deltas.iter().take(limit) {
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&delta.label));
        html.push_str("</td><td>");
        html.push_str(&escape(&profile_short(&delta.profile)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(Some(delta.ratio)));
        html.push_str("</td><td title=\"value ");
        html.push_str(&delta.value.to_string());
        html.push_str(", baseline ");
        html.push_str(&delta.baseline.to_string());
        html.push_str("\">");
        html.push_str(&signed(delta.delta));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

fn render_profile_tradeoff_table(
    rows: &[serde_json::Value],
    profile_filter: Option<&[&str]>,
) -> String {
    let aggregates = fixed_profile_tradeoff_aggregates(rows);
    let mut html = String::new();
    html.push_str(&format!(
        "<p class=\"small muted\">Geometric mean ratios over fixed-suite rows. Solidity profiles are relative to <span class=\"mono\">{}</span>; Vyper profiles are relative to <span class=\"mono\">{}</span>. Runtime gas is scenario-level; bytecode, deploy, and compile metrics are artifact-level.</p>",
        escape(SOL_CODEGEN_BASELINE),
        escape(VYPER_GAS_CODEGEN)
    ));
    html.push_str("<table><thead><tr><th>Profile</th><th>Harness Gas</th><th>Runtime Bytes</th><th>Internal Create Gas</th><th>Compile Wall</th><th>Samples</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        if let Some(filter) = profile_filter {
            if !filter.iter().any(|candidate| *candidate == profile) {
                continue;
            }
        }
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile_short(&profile)));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(&profile));
        html.push_str("</span></td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.gas)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.runtime_bytes)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.internal_create_gas)));
        html.push_str("</td><td>");
        html.push_str(&ratio_cell(geomean(&aggregate.compile_ms)));
        html.push_str("</td><td>");
        html.push_str(&format!(
            "{} gas / {} artifact",
            aggregate.gas.len(),
            aggregate.runtime_bytes.len()
        ));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn fixed_profile_tradeoff_aggregates(
    rows: &[serde_json::Value],
) -> BTreeMap<String, RatioAggregate> {
    let mut baseline_gas = BTreeMap::new();
    let mut baseline_artifacts = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        if !is_ok_fixed_metadata_off(row) {
            continue;
        }
        let language = str_at(row, "/language").unwrap_or_default();
        let Some(baseline_profile) = baseline_profile_for_language(&language) else {
            continue;
        };
        if str_at(row, "/profile_id").as_deref() != Some(baseline_profile) {
            continue;
        }
        baseline_gas.insert(
            format!("{}\0{}", language, scenario_key(row)),
            u64_at(row, "/gas/harness_call_gas"),
        );
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        let artifact_key = artifact_key_from_row(row);
        if seen.insert(artifact_key) {
            baseline_artifacts.insert(format!("{language}\0{benchmark}"), artifact_metrics(row));
        }
    }

    let mut aggregates: BTreeMap<String, RatioAggregate> = BTreeMap::new();
    let mut seen_artifacts = BTreeSet::new();
    for row in rows {
        if !is_ok_fixed_metadata_off(row) {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let entry = aggregates.entry(profile.clone()).or_default();
        let language = str_at(row, "/language").unwrap_or_default();
        if let Some(base_gas) = baseline_gas
            .get(&format!("{}\0{}", language, scenario_key(row)))
            .copied()
        {
            push_ratio(
                &mut entry.gas,
                u64_at(row, "/gas/harness_call_gas"),
                base_gas,
            );
        }
        let artifact_key = artifact_key_from_row(row);
        if seen_artifacts.insert(artifact_key) {
            let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
            if let Some(base) = baseline_artifacts
                .get(&format!("{language}\0{benchmark}"))
                .copied()
            {
                let current = artifact_metrics(row);
                push_ratio(
                    &mut entry.runtime_bytes,
                    current.runtime_bytes,
                    base.runtime_bytes,
                );
                push_ratio(
                    &mut entry.internal_create_gas,
                    current.internal_create_gas,
                    base.internal_create_gas,
                );
                push_float_ratio(&mut entry.compile_ms, current.compile_ms, base.compile_ms);
            }
        }
    }
    aggregates
}

fn render_compile_resource_summary_for_profiles(
    rows: &[serde_json::Value],
    profile_filter: Option<&[&str]>,
) -> String {
    let mut aggregates: BTreeMap<String, CompileAggregate> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok") {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        if let Some(filter) = profile_filter {
            if !filter.iter().any(|candidate| *candidate == profile) {
                continue;
            }
        }
        let artifact_key = artifact_key_from_row(row);
        if !seen.insert(artifact_key) {
            continue;
        }
        let aggregate = aggregates.entry(profile).or_default();
        let metrics = artifact_metrics(row);
        let stats = sample_stats(row, "/compile/wall_ms_samples");
        aggregate.artifacts += 1;
        aggregate.wall_medians.push(stats.median);
        aggregate.cvs.push(stats.cv);
        aggregate.total_rss_kib += metrics.peak_rss_kib;
        aggregate.max_rss_kib = aggregate.max_rss_kib.max(metrics.peak_rss_kib);
    }
    let max_wall = aggregates
        .values()
        .map(|aggregate| median(aggregate.wall_medians.clone()))
        .fold(0.0, f64::max)
        .max(1.0);
    let max_rss = aggregates
        .values()
        .map(|aggregate| aggregate.max_rss_kib)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Profile</th><th>Artifacts</th><th>Median Wall ms</th><th>Avg CV</th><th>CV &gt; 5%</th><th>Max RSS KiB</th><th>Avg RSS KiB</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        let artifacts = aggregate.artifacts.max(1);
        let median_wall = median(aggregate.wall_medians.clone());
        let avg_cv = average(&aggregate.cvs);
        let high_cv = aggregate.cvs.iter().filter(|cv| **cv > 0.05).count();
        let avg_rss = aggregate.total_rss_kib / artifacts as u64;
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile_short(&profile)));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(&profile));
        html.push_str("</span></td><td>");
        html.push_str(&aggregate.artifacts.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(
            median_wall,
            max_wall,
            &format!("{median_wall:.2}"),
        ));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.1}%", avg_cv * 100.0));
        html.push_str("</td><td>");
        html.push_str(&high_cv.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(
            aggregate.max_rss_kib as f64,
            max_rss as f64,
            &aggregate.max_rss_kib.to_string(),
        ));
        html.push_str("</td><td>");
        html.push_str(&avg_rss.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_fixed_profile_tradeoffs(rows: &[serde_json::Value]) -> String {
    render_profile_tradeoff_table(rows, None)
}

#[derive(Debug, Default)]
struct CompileAggregate {
    artifacts: usize,
    wall_medians: Vec<f64>,
    cvs: Vec<f64>,
    total_rss_kib: u64,
    max_rss_kib: u64,
}

fn render_compile_resource_summary(rows: &[serde_json::Value]) -> String {
    let mut aggregates: BTreeMap<String, CompileAggregate> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok") {
            continue;
        }
        let artifact_key = artifact_key_from_row(row);
        if !seen.insert(artifact_key) {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let aggregate = aggregates.entry(profile).or_default();
        let metrics = artifact_metrics(row);
        let stats = sample_stats(row, "/compile/wall_ms_samples");
        aggregate.artifacts += 1;
        aggregate.wall_medians.push(stats.median);
        aggregate.cvs.push(stats.cv);
        aggregate.total_rss_kib += metrics.peak_rss_kib;
        aggregate.max_rss_kib = aggregate.max_rss_kib.max(metrics.peak_rss_kib);
    }
    let max_wall = aggregates
        .values()
        .map(|aggregate| median(aggregate.wall_medians.clone()))
        .fold(0.0, f64::max)
        .max(1.0);
    let max_rss = aggregates
        .values()
        .map(|aggregate| aggregate.max_rss_kib)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::new();
    html.push_str("<p class=\"small muted\">Compile metrics are artifact-level. Wall time shows median recorded samples. CV is coefficient of variation over the recorded wall-time samples. RSS is a single peak sample and should be treated as coarse.</p>");
    html.push_str("<table><thead><tr><th>Profile</th><th>Artifacts</th><th>Median Wall ms</th><th>Avg CV</th><th>CV &gt; 5%</th><th>Max RSS KiB</th><th>Avg RSS KiB</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        let artifacts = aggregate.artifacts.max(1);
        let median_wall = median(aggregate.wall_medians.clone());
        let avg_cv = average(&aggregate.cvs);
        let high_cv = aggregate.cvs.iter().filter(|cv| **cv > 0.05).count();
        let avg_rss = aggregate.total_rss_kib / artifacts as u64;
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile));
        html.push_str("</td><td>");
        html.push_str(&aggregate.artifacts.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(
            median_wall,
            max_wall,
            &format!("{median_wall:.2}"),
        ));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.1}%", avg_cv * 100.0));
        html.push_str("</td><td>");
        html.push_str(&high_cv.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(
            aggregate.max_rss_kib as f64,
            max_rss as f64,
            &aggregate.max_rss_kib.to_string(),
        ));
        html.push_str("</td><td>");
        html.push_str(&avg_rss.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_data_export(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<div class=\"card\"><h3>Machine-readable outputs</h3>");
    html.push_str(&format!(
        "<p class=\"small muted\">The HTML report intentionally does not inline the {} normalized rows. Use the JSON artifacts for exhaustive row-level analysis.</p>",
        rows.len()
    ));
    html.push_str("<p><a href=\"../normalized/results.json\">normalized results JSON</a> &middot; <a href=\"../normalized/run-manifest.json\">run manifest JSON</a> &middot; <a href=\"../raw/foundry-gas.jsonl\">raw Foundry gas JSONL</a></p>");
    html.push_str("</div>");
    html
}

fn render_real_derived_summary(rows: &[serde_json::Value]) -> String {
    let mut selected = Vec::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() == Some(BenchmarkSuite::RealDerived.as_str()) {
            selected.push(row);
        }
    }
    if selected.is_empty() {
        return "<div class=\"card\">No real-derived rows in this report.</div>".to_string();
    }

    let mut html = String::new();
    html.push_str("<div class=\"notice small\"><strong>Scope note:</strong> real-derived rows are simplified benchmark models, not production-equivalent ports. Read the model kind, mock assumptions, and excluded features before comparing runtime gas.</div>");
    html.push_str(
        "<p class=\"answer\">What can the recognizable protocol-shaped models tell us?</p>",
    );
    html.push_str("<p class=\"section-lede\">They are useful for compiler behavior on larger DeFi-like accounting paths, but the default view keeps provenance and exclusions ahead of gas. Runtime ratios here are scoped model hot-path deltas, not Uniswap, Curve, or Yearn production-gas claims.</p>");
    html.push_str(&render_real_model_scope_table(&selected));

    let deltas = collect_scenario_deltas(
        rows,
        BenchmarkSuite::RealDerived.as_str(),
        &[
            SOL_VIAIR_CODEGEN,
            VYPER_GAS_CODEGEN,
            VYPER_ALPHA_GAS_CODEGEN,
            VYPER_GAS_VENOM_CODEGEN,
            VYPER_ALPHA_GAS_VENOM_CODEGEN,
            VYPER_CODESIZE_CODEGEN,
            VYPER_ALPHA_CODESIZE_CODEGEN,
            VYPER_CODESIZE_VENOM_CODEGEN,
            VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
            VYPER_NONE_CODEGEN,
            VYPER_ALPHA_NONE_CODEGEN,
            VYPER_NONE_VENOM_CODEGEN,
            VYPER_ALPHA_NONE_VENOM_CODEGEN,
        ],
    );
    let mut cheaper = deltas
        .iter()
        .filter(|delta| delta.ratio < 1.0)
        .cloned()
        .collect::<Vec<_>>();
    cheaper.sort_by(|left, right| {
        left.ratio
            .partial_cmp(&right.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut costlier = deltas
        .iter()
        .filter(|delta| delta.ratio > 1.0)
        .cloned()
        .collect::<Vec<_>>();
    costlier.sort_by(|left, right| {
        right
            .ratio
            .partial_cmp(&left.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    html.push_str("<section class=\"two\">");
    html.push_str(&render_delta_table(
        "Largest cheaper model rows",
        &cheaper,
        6,
    ));
    html.push_str(&render_delta_table(
        "Largest costlier model rows",
        &costlier,
        6,
    ));
    html.push_str("</section>");
    html.push_str(&render_real_model_profile_summary(&selected));
    html.push_str("<details><summary>Show real-derived scenario ratios</summary>");
    html.push_str(&render_real_derived_scenario_ratios(&selected));
    html.push_str("</details>");
    html.push_str("<p class=\"small muted\">Row-level provenance and every measured scenario remain in the normalized JSON export; the HTML keeps this section at model and scenario-ratio granularity.</p>");
    html
}

#[derive(Debug, Default)]
struct RealModelScope {
    upstream: String,
    model_kind: String,
    production_equivalence: String,
    source_language: String,
    source_path: String,
    scenarios: BTreeSet<String>,
    artifacts: BTreeSet<String>,
    assumptions: BTreeSet<String>,
    exclusions: BTreeSet<String>,
    min_eip170_margin: Option<i64>,
    min_eip3860_margin: Option<i64>,
}

fn render_real_model_scope_table(rows: &[&serde_json::Value]) -> String {
    let mut models: BTreeMap<String, RealModelScope> = BTreeMap::new();
    for row in rows {
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        let entry = models.entry(benchmark.clone()).or_default();
        if entry.upstream.is_empty() {
            entry.upstream = str_at(row, "/provenance/upstream_project").unwrap_or_default();
            entry.model_kind = str_at(row, "/provenance/model_kind").unwrap_or_default();
            entry.production_equivalence =
                str_at(row, "/provenance/production_equivalence").unwrap_or_default();
            entry.source_language = str_at(row, "/provenance/source_language").unwrap_or_default();
            entry.source_path = str_at(row, "/provenance/source_path").unwrap_or_default();
        }
        if str_at(row, "/status").as_deref() == Some("ok") {
            let scenario = str_at(row, "/gas/scenario").unwrap_or_default();
            if !scenario.is_empty() {
                entry.scenarios.insert(scenario);
            }
            entry.min_eip170_margin = Some(
                entry
                    .min_eip170_margin
                    .unwrap_or(i64::MAX)
                    .min(i64_at(row, "/bytecode/eip170_margin_bytes")),
            );
            entry.min_eip3860_margin = Some(
                entry
                    .min_eip3860_margin
                    .unwrap_or(i64::MAX)
                    .min(i64_at(row, "/bytecode/eip3860_margin_bytes")),
            );
        }
        entry.artifacts.insert(artifact_key_from_row(row));
        for assumption in array_strings_at(row, "/provenance/mock_assumptions") {
            entry.assumptions.insert(assumption);
        }
        for exclusion in array_strings_at(row, "/provenance/excluded_features") {
            entry.exclusions.insert(exclusion);
        }
    }

    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Model</th><th>Scope</th><th>Coverage</th><th>EIP Margins</th><th>Reader Guardrails</th></tr></thead><tbody>");
    for (benchmark, scope) in models {
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&benchmark));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(&scope.upstream));
        html.push_str("</span></td><td>");
        html.push_str(&escape(&scope.model_kind));
        html.push_str("<br><span class=\"small muted\">production_equivalence=");
        html.push_str(&escape(&scope.production_equivalence));
        html.push_str("<br>source ");
        html.push_str(&escape(&scope.source_language));
        html.push(':');
        html.push_str(&escape(&scope.source_path));
        html.push_str("</span></td><td>");
        html.push_str(&format!(
            "{} scenarios<br><span class=\"small muted\">{} compiled artifacts</span>",
            scope.scenarios.len(),
            scope.artifacts.len()
        ));
        html.push_str("</td><td class=\"small\">");
        html.push_str(&format!(
            "EIP-170: {} bytes<br>EIP-3860: {} bytes",
            scope
                .min_eip170_margin
                .map(signed)
                .unwrap_or_else(|| "n/a".to_string()),
            scope
                .min_eip3860_margin
                .map(signed)
                .unwrap_or_else(|| "n/a".to_string())
        ));
        html.push_str("</td><td class=\"small\">");
        html.push_str("<span class=\"muted\">mocked:</span> ");
        html.push_str(&escape(&summarize_set(&scope.assumptions, 2)));
        html.push_str("<br><span class=\"muted\">excluded:</span> ");
        html.push_str(&escape(&summarize_set(&scope.exclusions, 3)));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

#[derive(Debug, Default)]
struct RealModelProfileSummary {
    scenarios: BTreeSet<String>,
    artifacts: BTreeSet<String>,
    runtime_bytes: u64,
    internal_create_gas: u64,
    gas_total: u64,
    gas_samples: u64,
}

fn render_real_model_profile_summary(rows: &[&serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, RealModelProfileSummary> = BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        let language = str_at(row, "/language").unwrap_or_default();
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let key = format!("{benchmark}\0{language}\0{profile}");
        let entry = groups.entry(key).or_default();
        if let Some(scenario) = str_at(row, "/gas/scenario") {
            entry.scenarios.insert(scenario);
        }
        entry.gas_total += u64_at(row, "/gas/harness_call_gas");
        entry.gas_samples += 1;
        if entry.artifacts.insert(artifact_key_from_row(row)) {
            entry.runtime_bytes = u64_at(row, "/bytecode/runtime_bytes");
            entry.internal_create_gas = u64_at(row, "/gas/internal_create_gas");
        }
    }

    let mut html = String::new();
    html.push_str("<h3>Model/Profile Summary</h3>");
    html.push_str("<p class=\"small muted\">No-metadata artifact-level size/deploy values plus average harness call gas across model scenarios. This replaces row-level provenance repetition with one reader-facing profile surface per model.</p>");
    html.push_str("<table><thead><tr><th>Model</th><th>Language</th><th>Profile</th><th>Scenarios</th><th>Runtime Bytes</th><th>Internal Create Gas</th><th>Avg Harness Call Gas</th></tr></thead><tbody>");
    for (key, summary) in groups {
        let mut parts = key.split('\0');
        let benchmark = parts.next().unwrap_or_default();
        let language = parts.next().unwrap_or_default();
        let profile = parts.next().unwrap_or_default();
        let avg_gas = summary.gas_total / summary.gas_samples.max(1);
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(benchmark));
        html.push_str("</td><td>");
        html.push_str(&escape(language));
        html.push_str("</td><td class=\"mono\">");
        html.push_str(&escape(&profile_short(profile)));
        html.push_str("</td><td>");
        html.push_str(&summary.scenarios.len().to_string());
        html.push_str("</td><td>");
        html.push_str(&summary.runtime_bytes.to_string());
        html.push_str("</td><td>");
        html.push_str(&summary.internal_create_gas.to_string());
        html.push_str("</td><td>");
        html.push_str(&avg_gas.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_real_derived_scenario_ratios(rows: &[&serde_json::Value]) -> String {
    let selected_profiles = [
        SOL_VIAIR_CODEGEN,
        VYPER_GAS_CODEGEN,
        VYPER_ALPHA_GAS_CODEGEN,
        VYPER_GAS_VENOM_CODEGEN,
        VYPER_ALPHA_GAS_VENOM_CODEGEN,
        VYPER_CODESIZE_CODEGEN,
        VYPER_ALPHA_CODESIZE_CODEGEN,
        VYPER_CODESIZE_VENOM_CODEGEN,
        VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
        VYPER_NONE_CODEGEN,
        VYPER_ALPHA_NONE_CODEGEN,
        VYPER_NONE_VENOM_CODEGEN,
        VYPER_ALPHA_NONE_VENOM_CODEGEN,
    ];
    let mut baseline = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let key = scenario_key(row);
        labels.entry(key.clone()).or_insert_with(|| {
            format!(
                "{} / {} / {}",
                str_at(row, "/benchmark_id").unwrap_or_default(),
                str_at(row, "/gas/scenario").unwrap_or_default(),
                str_at(row, "/gas/state_access_profile").unwrap_or_default()
            )
        });
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let gas = u64_at(row, "/gas/harness_call_gas");
        if profile == SOL_CODEGEN_BASELINE {
            baseline.insert(key.clone(), gas);
        }
        values.entry(key).or_default().insert(profile, gas);
    }

    let mut html = String::new();
    html.push_str("<h3>Real-Derived Scenario Gas Ratios</h3>");
    html.push_str(&format!(
        "<p class=\"small muted\">No-metadata hot-path model rows compared against <span class=\"mono\">{}</span> within the same model, scenario, and state profile. These bars are scoped to the benchmark model, not production protocol gas.</p>",
        escape(SOL_CODEGEN_BASELINE)
    ));
    html.push_str("<table><thead><tr><th>Model Scenario</th>");
    for profile in selected_profiles {
        html.push_str("<th>");
        html.push_str(&escape(profile));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (key, label) in labels {
        let base = baseline.get(&key).copied();
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&label));
        html.push_str("</td>");
        for profile in selected_profiles {
            let ratio = base.and_then(|base| {
                values
                    .get(&key)
                    .and_then(|by_profile| by_profile.get(profile))
                    .map(|value| (*value as f64, base as f64))
            });
            html.push_str("<td>");
            html.push_str(&ratio_metric_cell(ratio));
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

#[derive(Debug, Default)]
struct ScaleFamilySummary {
    n_values: BTreeSet<u64>,
    profiles: BTreeSet<String>,
    successful_points: BTreeSet<String>,
    failure_profiles: BTreeSet<String>,
    compile_failures: u64,
    max_runtime_bytes: u64,
    max_harness_call_gas: u64,
    max_compile_ms: f64,
}

fn render_scale_family_summary(rows: &[serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, ScaleFamilySummary> = BTreeMap::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some(BenchmarkSuite::Scale.as_str()) {
            continue;
        }
        let family = str_at(row, "/family").unwrap_or_default();
        let n = u64_at(row, "/parameter_value");
        let profile_id = str_at(row, "/profile_id").unwrap_or_default();
        let entry = groups.entry(family).or_default();
        entry.n_values.insert(n);
        entry.profiles.insert(profile_id.clone());
        if str_at(row, "/status").as_deref() == Some("compile_error") {
            entry.compile_failures += 1;
            entry.failure_profiles.insert(profile_short(&profile_id));
        } else {
            entry.successful_points.insert(format!("{n}\0{profile_id}"));
            entry.max_runtime_bytes = entry
                .max_runtime_bytes
                .max(u64_at(row, "/bytecode/runtime_bytes"));
            entry.max_harness_call_gas = entry
                .max_harness_call_gas
                .max(u64_at(row, "/gas/harness_call_gas"));
            entry.max_compile_ms = entry
                .max_compile_ms
                .max(mean_f64_array_at(row, "/compile/wall_ms_samples"));
        }
    }

    if groups.is_empty() {
        return "<div class=\"card\">No generated scale rows in this report.</div>".to_string();
    }

    let mut html = String::new();
    html.push_str("<h3>Scale Family Summary</h3>");
    html.push_str("<p class=\"small muted\">One row per generated family. Detailed N/profile rows are intentionally left to the JSON export; curves and failure surface above are the primary scale reading path.</p>");
    html.push_str("<table><thead><tr><th>Family</th><th>N Range</th><th>Profiles</th><th>Successful N/Profile Points</th><th>Compile Failures</th><th>Failure Profiles</th><th>Max Runtime Bytes</th><th>Max Harness Call Gas</th><th>Max Compile ms</th></tr></thead><tbody>");
    for (family, aggregate) in groups {
        let min_n = aggregate
            .n_values
            .iter()
            .next()
            .copied()
            .unwrap_or_default();
        let max_n = aggregate
            .n_values
            .iter()
            .next_back()
            .copied()
            .unwrap_or_default();
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&family));
        html.push_str("</td><td>");
        html.push_str(&format!("{min_n}-{max_n}"));
        html.push_str("</td><td>");
        html.push_str(&aggregate.profiles.len().to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.successful_points.len().to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.compile_failures.to_string());
        html.push_str("</td><td class=\"small\">");
        html.push_str(&escape(
            &aggregate
                .failure_profiles
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("; "),
        ));
        html.push_str("</td><td>");
        html.push_str(&aggregate.max_runtime_bytes.to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.max_harness_call_gas.to_string());
        html.push_str("</td><td>");
        html.push_str(&format!("{:.2}", aggregate.max_compile_ms));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

#[derive(Debug, Default)]
struct BenchmarkProfileSummary {
    scenarios: usize,
    total_gas: u64,
    runtime_bytes: u64,
    compile_ms: f64,
}

fn render_fixed_benchmark_summary(rows: &[serde_json::Value]) -> String {
    let mut cells: BTreeMap<String, BTreeMap<String, BenchmarkProfileSummary>> = BTreeMap::new();
    let mut seen_artifacts = BTreeSet::new();
    for row in rows {
        if !is_ok_fixed_metadata_off(row) {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        if !SOL_CODEGEN_PROFILES.contains(&profile.as_str())
            && !VYPER_CODEGEN_PROFILES.contains(&profile.as_str())
        {
            continue;
        }
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        let entry = cells
            .entry(benchmark)
            .or_default()
            .entry(profile.clone())
            .or_default();
        entry.scenarios += 1;
        entry.total_gas += u64_at(row, "/gas/harness_call_gas");
        let artifact_key = artifact_key_from_row(row);
        if seen_artifacts.insert(artifact_key) {
            entry.runtime_bytes = u64_at(row, "/bytecode/runtime_bytes");
            entry.compile_ms = mean_f64_array_at(row, "/compile/wall_ms_samples");
        }
    }

    let profiles = [
        SOL_CODEGEN_BASELINE,
        SOL_VIAIR_CODEGEN,
        SOL_NOOPT_CODEGEN,
        VYPER_GAS_CODEGEN,
        VYPER_ALPHA_GAS_CODEGEN,
        VYPER_GAS_VENOM_CODEGEN,
        VYPER_ALPHA_GAS_VENOM_CODEGEN,
        VYPER_CODESIZE_CODEGEN,
        VYPER_ALPHA_CODESIZE_CODEGEN,
        VYPER_CODESIZE_VENOM_CODEGEN,
        VYPER_ALPHA_CODESIZE_VENOM_CODEGEN,
        VYPER_NONE_CODEGEN,
        VYPER_ALPHA_NONE_CODEGEN,
        VYPER_NONE_VENOM_CODEGEN,
        VYPER_ALPHA_NONE_VENOM_CODEGEN,
    ];
    let mut html = String::new();
    html.push_str("<h3>Per-Benchmark Snapshot</h3>");
    html.push_str("<p class=\"small muted\">Cells show runtime bytes and average harness call gas across scenarios for each benchmark/profile. This is a compact index; scenario-level rows remain in the JSON export.</p>");
    html.push_str("<table><thead><tr><th>Benchmark</th>");
    for profile in profiles {
        html.push_str("<th>");
        html.push_str(&escape(&profile_short(profile)));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (benchmark, by_profile) in cells {
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&benchmark));
        html.push_str("</td>");
        for profile in profiles {
            html.push_str("<td>");
            if let Some(summary) = by_profile.get(profile) {
                let avg_gas = summary.total_gas / summary.scenarios.max(1) as u64;
                html.push_str(&format!(
                    "<span class=\"mono\">{} B</span><br><span class=\"small muted\">avg gas {}</span>",
                    summary.runtime_bytes, avg_gas
                ));
            } else {
                html.push_str("<span class=\"pill na\">n/a</span>");
            }
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_benchmark_summary(rows: &[serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
    for row in rows {
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        if !benchmark.is_empty() {
            groups.entry(benchmark).or_default().push(row);
        }
    }
    let mut html = String::new();
    html.push_str(&format!(
        "<p class=\"section-lede\">One row per benchmark keeps the report navigable without turning the HTML into a {}-row export. Use the JSON output for full row-level analysis.</p>",
        rows.len()
    ));
    html.push_str("<table><thead><tr><th>Benchmark</th><th>Suite</th><th>Scenarios</th><th>Artifacts</th><th>Compile Failures</th><th>Correctness Strength</th><th>Worst EIP Margins</th></tr></thead><tbody>");
    for (benchmark, group) in groups {
        let suite = group
            .iter()
            .find_map(|row| str_at(row, "/suite"))
            .unwrap_or_default();
        let scenarios = group
            .iter()
            .filter_map(|row| str_at(row, "/gas/scenario"))
            .collect::<BTreeSet<_>>();
        let artifacts = group
            .iter()
            .map(|row| artifact_key_from_row(row))
            .collect::<BTreeSet<_>>();
        let failures = group
            .iter()
            .filter(|row| str_at(row, "/status").as_deref() == Some("compile_error"))
            .count();
        let strength = aggregate_strength(&group);
        let min_eip170 = group
            .iter()
            .filter(|row| str_at(row, "/status").as_deref() == Some("ok"))
            .map(|row| i64_at(row, "/bytecode/eip170_margin_bytes"))
            .min();
        let min_eip3860 = group
            .iter()
            .filter(|row| str_at(row, "/status").as_deref() == Some("ok"))
            .map(|row| i64_at(row, "/bytecode/eip3860_margin_bytes"))
            .min();
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&benchmark));
        html.push_str("</td><td>");
        html.push_str(&escape(&suite));
        html.push_str("</td><td>");
        html.push_str(&scenarios.len().to_string());
        html.push_str("</td><td>");
        html.push_str(&artifacts.len().to_string());
        html.push_str("</td><td>");
        html.push_str(&failures.to_string());
        html.push_str("</td><td>");
        html.push_str(&strength_badge(&strength));
        html.push_str("</td><td class=\"small\">");
        html.push_str(&format!(
            "EIP-170 {} / EIP-3860 {}",
            min_eip170.map(signed).unwrap_or_else(|| "n/a".to_string()),
            min_eip3860.map(signed).unwrap_or_else(|| "n/a".to_string())
        ));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_correctness_heatmap(rows: &[serde_json::Value]) -> String {
    let checks = [
        ("status", "/correctness/scenario_status_check"),
        ("baseline", "/correctness/baseline_differential_check"),
        ("return", "/correctness/return_data_check"),
        ("observer", "/correctness/observer_check"),
        ("random", "/correctness/randomized_differential_check"),
        ("property", "/correctness/property_tests"),
        ("golden", "/correctness/golden_behavior_check"),
        ("logs", "/correctness/log_check"),
    ];
    let mut groups: BTreeMap<String, Vec<&serde_json::Value>> = BTreeMap::new();
    for row in rows {
        groups
            .entry(str_at(row, "/benchmark_id").unwrap_or_default())
            .or_default()
            .push(row);
    }
    let mut html = String::new();
    html.push_str("<div class=\"legend\"><span><span class=\"pill pass\">pass</span></span><span><span class=\"pill info\">baseline_only</span></span><span><span class=\"pill warn\">not_run</span></span><span><span class=\"pill na\">not_applicable</span></span><span><span class=\"pill fail\">fail</span></span></div>");
    html.push_str("<div class=\"chart\"><table><thead><tr><th>Benchmark</th><th>Suite</th>");
    for (label, _) in checks {
        html.push_str("<th>");
        html.push_str(label);
        html.push_str("</th>");
    }
    html.push_str("<th>Strength</th></tr></thead><tbody>");
    for (benchmark, group) in groups {
        let suite = group
            .iter()
            .find_map(|row| str_at(row, "/suite"))
            .unwrap_or_default();
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&benchmark));
        html.push_str("</td><td>");
        html.push_str(&escape(&suite));
        html.push_str("</td>");
        for (_, pointer) in checks {
            let status = aggregate_check_status(&group, pointer);
            html.push_str(&heat_cell(&status));
        }
        html.push_str("<td>");
        html.push_str(&strength_badge(&aggregate_strength(&group)));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

fn render_scale_failure_surface(rows: &[serde_json::Value]) -> String {
    let mut profiles = BTreeSet::new();
    let mut cells: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some("scale") {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        profiles.insert(profile.clone());
        let family = str_at(row, "/family").unwrap_or_default();
        let value = u64_at(row, "/parameter_value");
        let key = format!("{family}\0{value:020}");
        let status = if str_at(row, "/status").as_deref() == Some("compile_error") {
            "fail"
        } else {
            "ok"
        };
        cells
            .entry(key)
            .or_default()
            .entry(profile)
            .and_modify(|current| {
                if status == "fail" {
                    *current = "fail".to_string();
                }
            })
            .or_insert_with(|| status.to_string());
    }
    let mut html = String::new();
    html.push_str("<h3>Compile Failure Surface</h3>");
    html.push_str("<p class=\"small muted\">Scale family/N/profile tiles. The visible failure boundary matters more than any average failure count.</p>");
    html.push_str("<div class=\"chart\"><table><thead><tr><th>Family / N</th>");
    for profile in &profiles {
        html.push_str("<th>");
        html.push_str(&escape(&profile_short(profile)));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (key, by_profile) in cells {
        let (family, value) = key.split_once('\0').unwrap_or((&key, ""));
        let n = value.parse::<u64>().unwrap_or(0);
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&format!("{family} / N={n}")));
        html.push_str("</td>");
        for profile in &profiles {
            match by_profile.get(profile).map(String::as_str) {
                Some("ok") => html.push_str("<td class=\"heat-ok\">ok</td>"),
                Some("fail") => html.push_str("<td class=\"heat-fail\">fail</td>"),
                _ => html.push_str("<td class=\"heat-na\">n/a</td>"),
            }
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

#[derive(Debug, Default, Clone)]
struct ScaleCurveAggregate {
    gas_total: u64,
    gas_samples: u64,
    runtime_bytes: u64,
    compile_ms: f64,
    seen_artifact: bool,
}

fn render_scale_curve_panels(rows: &[serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, BTreeMap<String, BTreeMap<u64, ScaleCurveAggregate>>> =
        BTreeMap::new();
    let mut seen_artifacts = BTreeSet::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some("scale")
            || str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        if !PRIMARY_CODEGEN_PROFILES.contains(&profile.as_str()) && profile != SOL_NOOPT_CODEGEN {
            continue;
        }
        let family = str_at(row, "/family").unwrap_or_default();
        let n = u64_at(row, "/parameter_value");
        let entry = groups
            .entry(family)
            .or_default()
            .entry(profile.clone())
            .or_default()
            .entry(n)
            .or_default();
        entry.gas_total += u64_at(row, "/gas/harness_call_gas");
        entry.gas_samples += 1;
        let artifact_key = artifact_key_from_row(row);
        if seen_artifacts.insert(artifact_key) {
            entry.seen_artifact = true;
            entry.runtime_bytes = u64_at(row, "/bytecode/runtime_bytes");
            entry.compile_ms = mean_f64_array_at(row, "/compile/wall_ms_samples");
        }
    }
    if groups.is_empty() {
        return String::new();
    }
    let mut html = String::new();
    html.push_str("<h3>Scale Curves</h3>");
    html.push_str("<p class=\"small muted\">Each family is shown as N on a log2 x-axis. Runtime gas is averaged across that family's generated scenarios for the profile/N point; compile time and bytecode are artifact-level. Line colour follows compiler family, and shade carries optimizer/codegen config.</p>");
    html.push_str(&compiler_palette_legend());
    html.push_str("<div class=\"chart-grid\">");
    for (family, by_profile) in groups {
        html.push_str("<div class=\"svg-card\"><h3>");
        html.push_str(&escape(&family));
        html.push_str("</h3>");
        html.push_str(&scale_family_vega_chart(&family, &by_profile));
        html.push_str("</div>");
    }
    html.push_str("</div>");
    html
}

fn scale_family_vega_chart(
    family: &str,
    by_profile: &BTreeMap<String, BTreeMap<u64, ScaleCurveAggregate>>,
) -> String {
    let mut values = Vec::new();
    for (profile, points) in by_profile {
        for (n, point) in points {
            let profile_label = profile_short(profile);
            let compiler = compiler_family(profile);
            values.push(json!({
                "metric": "runtime bytes",
                "n": n,
                "value": point.runtime_bytes,
                "compiler": compiler,
                "profile": profile_label,
                "profile_id": profile
            }));
            values.push(json!({
                "metric": "avg call gas",
                "n": n,
                "value": point.gas_total as f64 / point.gas_samples.max(1) as f64,
                "compiler": compiler,
                "profile": profile_label,
                "profile_id": profile
            }));
            values.push(json!({
                "metric": "compile ms",
                "n": n,
                "value": point.compile_ms,
                "compiler": compiler,
                "profile": profile_label,
                "profile_id": profile
            }));
        }
    }
    if values.is_empty() {
        return String::new();
    }
    let spec = json!({
        "$schema": "https://vega.github.io/schema/vega-lite/v6.json",
        "data": { "values": values },
        "facet": {
            "field": "metric",
            "type": "nominal",
            "columns": 3,
            "sort": ["runtime bytes", "avg call gas", "compile ms"],
            "title": null,
            "header": {
                "labelFontSize": 11,
                "labelColor": "#374151",
                "labelFontWeight": "normal"
            }
        },
        "spec": {
            "width": 170,
            "height": 150,
            "mark": {
                "type": "line",
                "point": { "filled": true, "size": 18 },
                "strokeWidth": 2
            },
            "encoding": {
                "x": {
                    "field": "n",
                    "type": "quantitative",
                    "title": "N",
                    "scale": { "type": "log", "base": 2 },
                    "axis": { "values": [1, 2, 4, 8, 16, 32, 64], "format": "d" }
                },
                "y": {
                    "field": "value",
                    "type": "quantitative",
                    "title": null,
                    "scale": { "zero": true },
                    "axis": { "grid": true }
                },
                "color": profile_color_encoding(true),
                "detail": { "field": "profile_id", "type": "nominal" },
                "tooltip": [
                    { "field": "metric", "type": "nominal", "title": "Metric" },
                    { "field": "n", "type": "quantitative", "title": "N" },
                    { "field": "compiler", "type": "nominal", "title": "Compiler" },
                    { "field": "profile_id", "type": "nominal", "title": "Profile" },
                    { "field": "value", "type": "quantitative", "title": "Value", "format": ",.2f" }
                ]
            }
        },
        "resolve": { "scale": { "y": "independent" } },
        "config": vega_chart_config()
    });
    let id = format!("scale-{}-chart", html_id(family));
    vega_chart(&id, &spec)
}

fn render_methodology(
    _rows: &[serde_json::Value],
    toolchains: &Toolchains,
    manifest: &serde_json::Value,
) -> String {
    let mut html = String::new();
    html.push_str("<section class=\"two\">");
    html.push_str("<div class=\"card\"><h3>Pinned Environment</h3><p class=\"small muted\">");
    let toolchain_summary = toolchains
        .compilers
        .iter()
        .map(|(key, toolchain)| {
            format!(
                "{} {} at <span class=\"mono\">{}</span> sha256 <span class=\"mono\">{}</span>",
                escape(key),
                escape(&toolchain.version),
                escape(&toolchain.binary_path.display().to_string()),
                escape(&toolchain.binary_sha256)
            )
        })
        .collect::<Vec<_>>()
        .join(". ");
    html.push_str(&format!(
        "Default runtime EVM fork {}. {}. Host: {}. Foundry: {}.",
        escape(&toolchains.evm_version),
        toolchain_summary,
        escape(&str_at(manifest, "/environment/host/cpu").unwrap_or_default()),
        escape(&str_at(manifest, "/environment/tools/forge").unwrap_or_default())
    ));
    html.push_str("</p></div>");
    html.push_str("<div class=\"card\"><h3>Limitations</h3><p class=\"small muted\">Gas is Foundry internal-call gas, not measured transaction gas. Compile timing is host-specific with three wall-time samples per successful artifact. Peak RSS is a single coarse sample. Cross-language behavioral equivalence is fuzz+property checked only where the correctness heatmap says so. Compiler bytecode metadata is disabled for the active matrix, so metadata overhead is intentionally outside the measured comparison surface. Vyper 0.5.0a1 is a pre-release compiler. Vyper Venom variants use <span class=\"mono\">--experimental-codegen</span>; treat their failures and wins as experimental codegen evidence, not production-default Vyper behavior.");
    html.push_str("</p></div>");
    html.push_str("<div class=\"card\"><h3>Cache</h3><p class=\"small muted\">Compile artifacts and gas rows are keyed by source hash, compiler binary hash, profile settings, EVM fork, scenario definition, and runner schema. A matching row is marked <span class=\"mono\">hit</span>; changed inputs are marked <span class=\"mono\">stale</span> with field-level reasons under <span class=\"mono\">cache.*.invalidated_by</span>. Use <span class=\"mono\">--no-cache</span> for fresh publication runs.</p><pre class=\"small\">");
    html.push_str(&escape(
        &serde_json::to_string_pretty(&manifest["cache"]).unwrap_or_default(),
    ));
    html.push_str("</pre></div></section>");
    html.push_str("<details><summary>Show profile settings JSON</summary><pre class=\"small\">");
    html.push_str(&escape(
        &serde_json::to_string_pretty(&manifest["profiles"]).unwrap_or_default(),
    ));
    html.push_str("</pre></details>");
    html
}

fn metric_card(label: &str, value: &str, subvalue: &str) -> String {
    format!(
        "<div class=\"card\"><div class=\"k\">{}</div><div class=\"v\">{}</div><div class=\"subv\">{}</div></div>",
        escape(label),
        escape(value),
        escape(subvalue)
    )
}

fn behavior_fuzz_benchmark_count(rows: &[serde_json::Value], suite: &str) -> usize {
    let mut benchmarks = BTreeSet::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() == Some(suite)
            && str_at(row, "/correctness/randomized_differential_check").as_deref() == Some("pass")
            && str_at(row, "/correctness/property_tests").as_deref() == Some("pass")
        {
            benchmarks.insert(str_at(row, "/benchmark_id").unwrap_or_default());
        }
    }
    benchmarks.len()
}

fn coverage_strip_for_suite(rows: &[serde_json::Value], suite: &str) -> String {
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut total = 0usize;
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some(suite) {
            continue;
        }
        total += 1;
        *counts.entry(row_strength(row)).or_default() += 1;
    }
    let total = total.max(1);
    let segments = [
        ("behavior-fuzz", "seg-fuzz"),
        ("baseline-smoke", "seg-baseline"),
        ("status-only", "seg-status"),
        ("compile-fail", "seg-fail"),
        ("not-applicable", "seg-na"),
    ];
    let mut html = String::new();
    html.push_str("<div class=\"strip\">");
    for (strength, class) in segments {
        let count = counts.get(strength).copied().unwrap_or(0);
        if count == 0 {
            continue;
        }
        let width = count as f64 * 100.0 / total as f64;
        html.push_str(&format!(
            "<div class=\"seg {class}\" style=\"width:{width:.2}%\" title=\"{}: {} rows\">{}</div>",
            escape(strength),
            count,
            if width > 12.0 {
                count.to_string()
            } else {
                String::new()
            }
        ));
    }
    html.push_str("</div>");
    html
}

fn row_strength(row: &serde_json::Value) -> &'static str {
    if str_at(row, "/status").as_deref() == Some("compile_error")
        || str_at(row, "/correctness/scenario_status_check").as_deref() == Some("fail")
    {
        return "compile-fail";
    }
    if str_at(row, "/correctness/randomized_differential_check").as_deref() == Some("pass")
        && str_at(row, "/correctness/property_tests").as_deref() == Some("pass")
    {
        return "behavior-fuzz";
    }
    if matches!(
        str_at(row, "/correctness/baseline_differential_check").as_deref(),
        Some("baseline_only" | "pass")
    ) {
        return "baseline-smoke";
    }
    if str_at(row, "/correctness/scenario_status_check").as_deref() == Some("pass") {
        return "status-only";
    }
    "not-applicable"
}

fn aggregate_strength(rows: &[&serde_json::Value]) -> String {
    if rows.iter().any(|row| row_strength(row) == "behavior-fuzz") {
        "behavior-fuzz".to_string()
    } else if rows.iter().any(|row| row_strength(row) == "baseline-smoke") {
        "baseline-smoke".to_string()
    } else if rows.iter().any(|row| row_strength(row) == "status-only") {
        "status-only".to_string()
    } else if rows.iter().any(|row| row_strength(row) == "compile-fail") {
        "compile-fail".to_string()
    } else {
        "not-applicable".to_string()
    }
}

fn strength_badge(strength: &str) -> String {
    let class = match strength {
        "behavior-fuzz" => "pass",
        "baseline-smoke" => "info",
        "status-only" => "warn",
        "compile-fail" => "fail",
        _ => "na",
    };
    format!("<span class=\"pill {class}\">{}</span>", escape(strength))
}

fn aggregate_check_status(rows: &[&serde_json::Value], pointer: &str) -> String {
    let mut statuses = BTreeSet::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok") {
            continue;
        }
        statuses.insert(str_at(row, pointer).unwrap_or_else(|| "not_applicable".into()));
    }
    if statuses.contains("fail") {
        "fail".to_string()
    } else if statuses.contains("pass") {
        "pass".to_string()
    } else if statuses.contains("baseline_only") {
        "baseline_only".to_string()
    } else if statuses.contains("not_run") {
        "not_run".to_string()
    } else {
        "not_applicable".to_string()
    }
}

fn heat_cell(status: &str) -> String {
    let class = match status {
        "pass" => "heat-ok",
        "baseline_only" => "heat-info",
        "not_run" => "heat-warn",
        "fail" => "heat-fail",
        _ => "heat-na",
    };
    format!("<td class=\"{class}\">{}</td>", escape(status))
}

fn dispatch_signature(rows: &[serde_json::Value]) -> String {
    let viair = scale_metric_endpoints(rows, "dispatch_N", "first_selector", SOL_VIAIR_CODEGEN);
    let vyper_gas = scale_metric_endpoints(rows, "dispatch_N", "first_selector", VYPER_GAS_CODEGEN);
    match (viair, vyper_gas) {
        (Some((viair_min, viair_max)), Some((vyper_min, vyper_max))) => format!(
            "On dispatch_N first_selector, solc viaIR moves from {viair_min} to {viair_max} gas as N grows, while Vyper gas stays near {vyper_min}-{vyper_max} gas."
        ),
        _ => "Scale curves show compiler-specific growth and failure boundaries; see the scale section.".to_string(),
    }
}

fn scale_metric_endpoints(
    rows: &[serde_json::Value],
    family: &str,
    scenario: &str,
    profile: &str,
) -> Option<(u64, u64)> {
    let mut points = BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() == Some("ok")
            && str_at(row, "/suite").as_deref() == Some("scale")
            && str_at(row, "/family").as_deref() == Some(family)
            && str_at(row, "/gas/scenario").as_deref() == Some(scenario)
            && str_at(row, "/profile_id").as_deref() == Some(profile)
            && str_at(row, "/gas/metadata_mode").as_deref() == Some("off")
        {
            points.insert(
                u64_at(row, "/parameter_value"),
                u64_at(row, "/gas/harness_call_gas"),
            );
        }
    }
    Some((*points.values().next()?, *points.values().next_back()?))
}

fn collect_language_scenario_deltas(
    rows: &[serde_json::Value],
    suite: &str,
    language: &str,
    baseline_profile: &str,
    profiles: &[&str],
) -> Vec<ScenarioDelta> {
    let mut baseline = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("ok")
            || str_at(row, "/suite").as_deref() != Some(suite)
            || str_at(row, "/language").as_deref() != Some(language)
            || str_at(row, "/gas/metadata_mode").as_deref() != Some("off")
        {
            continue;
        }
        let key = scenario_key(row);
        labels
            .entry(key.clone())
            .or_insert_with(|| scenario_label(row));
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let gas = u64_at(row, "/gas/harness_call_gas");
        if profile == baseline_profile {
            baseline.insert(key.clone(), gas);
        }
        if profiles.iter().any(|candidate| *candidate == profile) {
            values.entry(key).or_default().insert(profile, gas);
        }
    }

    let mut deltas = Vec::new();
    for (key, by_profile) in values {
        let Some(base) = baseline.get(&key).copied() else {
            continue;
        };
        if base == 0 {
            continue;
        }
        let label = labels.get(&key).cloned().unwrap_or(key);
        for (profile, value) in by_profile {
            if value == 0 {
                continue;
            }
            deltas.push(ScenarioDelta {
                label: label.clone(),
                profile,
                ratio: value as f64 / base as f64,
                delta: value as i64 - base as i64,
                value,
                baseline: base,
            });
        }
    }
    deltas
}

fn baseline_profile_for_language(language: &str) -> Option<&'static str> {
    match language {
        "solidity" => Some(SOL_CODEGEN_BASELINE),
        "vyper" => Some(VYPER_GAS_CODEGEN),
        _ => None,
    }
}

fn profile_palette() -> Vec<(String, &'static str)> {
    [
        (SOL_NOOPT_CODEGEN, "#93c5fd"),
        (SOL_CODEGEN_BASELINE, "#2563eb"),
        (SOL_VIAIR_CODEGEN, "#1e3a8a"),
        ("solc-0.8.30-noopt", "#bfdbfe"),
        ("solc-0.8.30-legacy-runs200", "#3b82f6"),
        ("solc-0.8.30-viair-runs200", "#1d4ed8"),
        (VYPER_NONE_CODEGEN, "#86efac"),
        (VYPER_GAS_CODEGEN, "#16a34a"),
        (VYPER_CODESIZE_CODEGEN, "#166534"),
        (VYPER_NONE_VENOM_CODEGEN, "#4ade80"),
        (VYPER_GAS_VENOM_CODEGEN, "#15803d"),
        (VYPER_CODESIZE_VENOM_CODEGEN, "#14532d"),
        (VYPER_ALPHA_NONE_CODEGEN, "#67e8f9"),
        (VYPER_ALPHA_GAS_CODEGEN, "#0891b2"),
        (VYPER_ALPHA_CODESIZE_CODEGEN, "#155e75"),
        (VYPER_ALPHA_NONE_VENOM_CODEGEN, "#22d3ee"),
        (VYPER_ALPHA_GAS_VENOM_CODEGEN, "#0e7490"),
        (VYPER_ALPHA_CODESIZE_VENOM_CODEGEN, "#164e63"),
        ("vyper-0.4.0-none", "#c4b5fd"),
        ("vyper-0.4.0-gas", "#8b5cf6"),
        ("vyper-0.4.0-codesize", "#5b21b6"),
        ("vyper-0.4.0-none-venom", "#a78bfa"),
        ("vyper-0.4.0-gas-venom", "#7c3aed"),
        ("vyper-0.4.0-codesize-venom", "#4c1d95"),
        ("vyper-0.3.10-none", "#fdba74"),
        ("vyper-0.3.10-gas", "#ea580c"),
        ("vyper-0.3.10-codesize", "#9a3412"),
        ("vyper-0.2.16-default", "#6b7280"),
    ]
    .into_iter()
    .map(|(profile, color)| (profile_short(profile), color))
    .collect()
}

fn profile_color_encoding(hide_legend: bool) -> serde_json::Value {
    let palette = profile_palette();
    let domain = palette
        .iter()
        .map(|(profile, _)| profile.clone())
        .collect::<Vec<_>>();
    let range = palette.iter().map(|(_, color)| *color).collect::<Vec<_>>();
    let legend = if hide_legend {
        serde_json::Value::Null
    } else {
        json!({
            "title": "Compiler config",
            "orient": "bottom",
            "columns": 3
        })
    };
    json!({
        "field": "profile",
        "type": "nominal",
        "scale": { "domain": domain, "range": range },
        "legend": legend
    })
}

fn compiler_family(profile: &str) -> &'static str {
    if profile.starts_with("solc-") {
        if profile.starts_with("solc-0.8.30-") {
            "solc 0.8.30"
        } else if profile.starts_with("solc-0.8.20-") {
            "solc 0.8.20"
        } else if profile.starts_with("solc-0.8.13-") {
            "solc 0.8.13"
        } else if profile.starts_with("solc-0.8.0-") {
            "solc 0.8.0"
        } else if profile.starts_with("solc-0.7.6-") {
            "solc 0.7.6"
        } else if profile.starts_with("solc-0.6.12-") {
            "solc 0.6.12"
        } else if profile.starts_with("solc-0.5.17-") {
            "solc 0.5.17"
        } else if profile.starts_with("solc-0.4.26-") {
            "solc 0.4.26"
        } else {
            "solc latest"
        }
    } else if profile.starts_with("vyper-0.5.0a1-") {
        "Vyper 0.5.0a1"
    } else if profile.starts_with("vyper-0.4.0-") {
        "Vyper 0.4.0"
    } else if profile.starts_with("vyper-0.3.10-") {
        "Vyper 0.3.10"
    } else if profile.starts_with("vyper-0.2.16-") {
        "Vyper 0.2.16"
    } else if profile.starts_with("vyper-") {
        "Vyper latest"
    } else {
        "unknown"
    }
}

fn vega_chart_config() -> serde_json::Value {
    json!({
        "background": "transparent",
        "font": "Inter, system-ui, -apple-system, sans-serif",
        "axis": {
            "labelFontSize": 11,
            "titleFontSize": 12,
            "labelColor": "#657083",
            "titleColor": "#657083",
            "gridColor": "#edf0f4",
            "domainColor": "#9ca3af",
            "tickColor": "#9ca3af"
        },
        "view": { "stroke": null },
        "legend": {
            "labelFontSize": 11,
            "titleFontSize": 12,
            "labelColor": "#374151",
            "titleColor": "#374151",
            "symbolType": "circle"
        }
    })
}

fn vega_chart(id: &str, spec: &serde_json::Value) -> String {
    format!(
        "<div class=\"chart\"><div id=\"{}\" class=\"vega-chart\"></div></div><script>evmBenchChart({}, {});</script>",
        escape(id),
        json_for_script(&json!(id)),
        json_for_script(spec)
    )
}

fn json_for_script(value: &serde_json::Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "null".to_string())
        .replace("</", "<\\/")
}

fn html_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn compiler_palette_legend() -> String {
    let groups = [
        ("solc", "#2563eb", "blue shades: noopt / legacy / viaIR"),
        (
            "Vyper latest",
            "#16a34a",
            "green shades: none / gas / codesize / Venom",
        ),
        (
            "Vyper 0.5.0a1",
            "#0891b2",
            "cyan shades: none / gas / codesize / Venom",
        ),
        (
            "historical Vyper",
            "#8b5cf6",
            "purple/orange/gray shades: 0.4.0 / 0.3.10 / 0.2.16",
        ),
    ];
    let mut html = String::new();
    html.push_str("<div class=\"legend\">");
    for (label, color, note) in groups {
        html.push_str(&format!(
            "<span><span class=\"dot\" style=\"background:{}\"></span><strong>{}</strong> <span class=\"small muted\">{}</span></span>",
            color,
            escape(label),
            escape(note)
        ));
    }
    html.push_str("</div>");
    html
}

fn is_ok_fixed_metadata_off(row: &serde_json::Value) -> bool {
    str_at(row, "/status").as_deref() == Some("ok")
        && str_at(row, "/suite").as_deref() == Some("fixed")
        && str_at(row, "/gas/metadata_mode").as_deref() == Some("off")
}

fn scenario_key(row: &serde_json::Value) -> String {
    format!(
        "{}\0{}\0{}",
        str_at(row, "/benchmark_id").unwrap_or_default(),
        str_at(row, "/gas/scenario").unwrap_or_default(),
        str_at(row, "/gas/state_access_profile").unwrap_or_default()
    )
}

fn scenario_label(row: &serde_json::Value) -> String {
    let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
    let scenario = str_at(row, "/gas/scenario").unwrap_or_default();
    let state = str_at(row, "/gas/state_access_profile").unwrap_or_default();
    if state.is_empty() {
        format!("{benchmark} / {scenario}")
    } else {
        format!("{benchmark} / {scenario} / {state}")
    }
}

fn artifact_key_from_row(row: &serde_json::Value) -> String {
    artifact_key(
        &str_at(row, "/benchmark_id").unwrap_or_default(),
        &str_at(row, "/implementation_id").unwrap_or_default(),
        &str_at(row, "/profile_id").unwrap_or_default(),
    )
}

fn artifact_metrics(row: &serde_json::Value) -> ArtifactMetrics {
    ArtifactMetrics {
        runtime_bytes: u64_at(row, "/bytecode/runtime_bytes"),
        internal_create_gas: u64_at(row, "/gas/internal_create_gas"),
        compile_ms: mean_f64_array_at(row, "/compile/wall_ms_samples"),
        peak_rss_kib: u64_at(row, "/compile/peak_rss_kib"),
    }
}

fn push_ratio(values: &mut Vec<f64>, value: u64, baseline: u64) {
    if baseline > 0 && value > 0 {
        values.push(value as f64 / baseline as f64);
    }
}

fn push_float_ratio(values: &mut Vec<f64>, value: f64, baseline: f64) {
    if baseline > 0.0 && value > 0.0 {
        values.push(value / baseline);
    }
}

fn geomean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut total = 0.0;
    let mut count = 0.0;
    for value in values {
        if *value > 0.0 {
            total += value.ln();
            count += 1.0;
        }
    }
    if count == 0.0 {
        None
    } else {
        Some((total / count).exp())
    }
}

fn ratio_metric_cell(value_and_base: Option<(f64, f64)>) -> String {
    let Some((value, base)) = value_and_base else {
        return "<span class=\"pill na\">n/a</span>".to_string();
    };
    if base <= 0.0 || value <= 0.0 {
        return "<span class=\"pill na\">n/a</span>".to_string();
    }
    let ratio = value / base;
    let delta = value - base;
    let class = ratio_class(ratio);
    let width = ratio_width(ratio);
    format!(
        "<div title=\"value {:.0}, baseline {:.0}, delta {:+.0}\">{}</div>",
        value,
        base,
        delta,
        ratio_bar(ratio, class, width)
    )
}

fn ratio_cell(ratio: Option<f64>) -> String {
    let Some(ratio) = ratio else {
        return "<span class=\"pill na\">n/a</span>".to_string();
    };
    ratio_bar(ratio, ratio_class(ratio), ratio_width(ratio))
}

fn ratio_bar(ratio: f64, class: &'static str, width: f64) -> String {
    format!(
        "<div class=\"ratio\"><span>{ratio:.2}x</span><div class=\"bar {class}\"><span style=\"width:{width:.1}%\"></span></div></div>"
    )
}

fn ratio_width(ratio: f64) -> f64 {
    ((ratio.min(2.0) / 2.0) * 100.0).max(3.0)
}

fn ratio_class(ratio: f64) -> &'static str {
    if ratio <= 1.0 {
        "good"
    } else if ratio <= 1.25 {
        "warn"
    } else {
        "fail"
    }
}

fn bar_value(value: f64, max: f64, label: &str) -> String {
    let width = if max <= 0.0 {
        0.0
    } else {
        ((value / max) * 100.0).clamp(2.0, 100.0)
    };
    format!(
        "<div class=\"ratio\"><span>{}</span><div class=\"bar\"><span style=\"width:{width:.1}%\"></span></div></div>",
        escape(label)
    )
}

fn short_error(row: &serde_json::Value) -> String {
    let error = str_at(row, "/compile/error").unwrap_or_default();
    if error.contains("Stack too deep") {
        return "Stack too deep".to_string();
    }
    error
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("compiler error")
        .chars()
        .take(160)
        .collect()
}

fn render_compile_failure_brief_table(rows: &[serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, (String, String, u64, String, BTreeSet<String>)> =
        BTreeMap::new();
    for row in rows {
        if str_at(row, "/status").as_deref() != Some("compile_error") {
            continue;
        }
        let suite = str_at(row, "/suite").unwrap_or_default();
        let family = str_at(row, "/family")
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| str_at(row, "/benchmark_id").unwrap_or_default());
        let n = u64_at(row, "/parameter_value");
        let reason = short_error(row);
        let key = format!("{suite}\0{family}\0{n:020}\0{reason}");
        groups
            .entry(key)
            .or_insert_with(|| (suite, family, n, reason, BTreeSet::new()))
            .4
            .insert(str_at(row, "/profile_id").unwrap_or_default());
    }

    if groups.is_empty() {
        return "<div class=\"callout\"><strong>All attempted artifacts compiled.</strong><div class=\"small muted\">No compile-error rows are present in this result set.</div></div>".to_string();
    }

    let mut html = String::new();
    html.push_str("<table class=\"mini-table\"><thead><tr><th>Suite</th><th>Family</th><th>N</th><th>Reason</th><th>Affected Profiles</th></tr></thead><tbody>");
    for (_, (suite, family, n, reason, profiles)) in groups {
        html.push_str("<tr><td>");
        html.push_str(&escape(&suite));
        html.push_str("</td><td class=\"mono\">");
        html.push_str(&escape(&family));
        html.push_str("</td><td>");
        if n == 0 {
            html.push_str("-");
        } else {
            html.push_str(&n.to_string());
        }
        html.push_str("</td><td>");
        html.push_str(&escape(&reason));
        html.push_str("</td><td class=\"small\">");
        html.push_str(
            &profiles
                .iter()
                .map(|profile| escape(&profile_short(profile)))
                .collect::<Vec<_>>()
                .join("<br>"),
        );
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn compile_failure_story(rows: &[serde_json::Value]) -> String {
    let failures = rows
        .iter()
        .filter(|row| str_at(row, "/status").as_deref() == Some("compile_error"))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        return "All attempted artifacts compile across the selected compiler profiles."
            .to_string();
    }

    let mut suites = BTreeSet::new();
    let mut families = BTreeSet::new();
    let mut values = BTreeSet::new();
    let mut profiles = BTreeSet::new();
    let mut reasons = BTreeSet::new();
    for row in failures {
        suites.insert(str_at(row, "/suite").unwrap_or_default());
        families.insert(
            str_at(row, "/family")
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| str_at(row, "/benchmark_id").unwrap_or_default()),
        );
        let value = u64_at(row, "/parameter_value");
        if value > 0 {
            values.insert(value);
        }
        profiles.insert(profile_short(
            &str_at(row, "/profile_id").unwrap_or_default(),
        ));
        reasons.insert(short_error(row));
    }
    format!(
        "{} compile failures are isolated to {} / {} at N={} across {} profiles; reason: {}.",
        rows.iter()
            .filter(|row| str_at(row, "/status").as_deref() == Some("compile_error"))
            .count(),
        suites.into_iter().collect::<Vec<_>>().join(", "),
        families.into_iter().collect::<Vec<_>>().join(", "),
        values
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        profiles.len(),
        reasons.into_iter().collect::<Vec<_>>().join(", ")
    )
}

fn compile_failure_suite_label(rows: &[serde_json::Value]) -> String {
    let suites = rows
        .iter()
        .filter(|row| str_at(row, "/status").as_deref() == Some("compile_error"))
        .filter_map(|row| str_at(row, "/suite"))
        .collect::<BTreeSet<_>>();
    if suites.is_empty() {
        "none".to_string()
    } else {
        suites.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn compile_failure_reason_label(rows: &[serde_json::Value]) -> String {
    let reasons = rows
        .iter()
        .filter(|row| str_at(row, "/status").as_deref() == Some("compile_error"))
        .map(short_error)
        .collect::<BTreeSet<_>>();
    if reasons.is_empty() {
        "none".to_string()
    } else {
        reasons.into_iter().collect::<Vec<_>>().join(", ")
    }
}

fn profile_short(profile: &str) -> String {
    let label = match profile {
        "solc-latest-legacy-runs200" => "solc legacy",
        "solc-latest-viair-runs200" => "solc viaIR",
        "solc-latest-noopt" => "solc noopt",
        "vyper-latest-gas" => "vyper gas",
        "vyper-0.5.0a1-gas" => "vyper 0.5 gas",
        "vyper-latest-gas-venom" => "vyper gas venom",
        "vyper-0.5.0a1-gas-venom" => "vyper 0.5 gas venom",
        "vyper-latest-codesize" => "vyper codesize",
        "vyper-0.5.0a1-codesize" => "vyper 0.5 codesize",
        "vyper-latest-codesize-venom" => "vyper codesize venom",
        "vyper-0.5.0a1-codesize-venom" => "vyper 0.5 codesize venom",
        "vyper-latest-none" => "vyper none",
        "vyper-0.5.0a1-none" => "vyper 0.5 none",
        "vyper-latest-none-venom" => "vyper none venom",
        "vyper-0.5.0a1-none-venom" => "vyper 0.5 none venom",
        _ => return profile_short_dynamic(profile),
    };
    label.to_string()
}

fn profile_short_dynamic(profile: &str) -> String {
    if let Some(rest) = profile.strip_prefix("solc-") {
        let version = compiler_version_label(profile);
        let label = rest
            .strip_prefix(&format!("{version}-"))
            .unwrap_or(rest)
            .replace("legacy-runs200", "legacy")
            .replace("viair-runs200", "viaIR")
            .replace("noopt", "noopt")
            .replace('-', " ");
        return format!("solc {version} {label}");
    }
    if let Some(rest) = profile.strip_prefix("vyper-") {
        let version = compiler_version_label(profile);
        let label = rest
            .strip_prefix(&format!("{version}-"))
            .unwrap_or(rest)
            .replace('-', " ");
        return format!("vyper {version} {label}");
    }
    profile.to_string()
}

fn signed(value: i64) -> String {
    if value >= 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn artifact_key(benchmark_id: &str, implementation_id: &str, profile_id: &str) -> String {
    format!("{benchmark_id}\0{implementation_id}\0{profile_id}")
}

fn sort_key(row: &serde_json::Value) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        str_at(row, "/benchmark_id").unwrap_or_default(),
        str_at(row, "/implementation_id").unwrap_or_default(),
        str_at(row, "/profile_id").unwrap_or_default(),
        str_at(row, "/gas/scenario").unwrap_or_default()
    )
}

fn str_at(row: &serde_json::Value, pointer: &str) -> Option<String> {
    row.pointer(pointer).and_then(|value| match value {
        serde_json::Value::String(text) => Some(text.clone()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    })
}

fn u64_at(row: &serde_json::Value, pointer: &str) -> u64 {
    row.pointer(pointer)
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

fn i64_at(row: &serde_json::Value, pointer: &str) -> i64 {
    row.pointer(pointer)
        .and_then(|value| value.as_i64())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy)]
struct SampleStats {
    median: f64,
    cv: f64,
}

fn sample_stats(row: &serde_json::Value, pointer: &str) -> SampleStats {
    let values = row
        .pointer(pointer)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_f64())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let median = median(values.clone());
    let mean = average(&values);
    let cv = if values.len() <= 1 || mean <= 0.0 {
        0.0
    } else {
        let variance = values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / values.len() as f64;
        variance.sqrt() / mean
    };
    SampleStats { median, cv }
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn average(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn mean_f64_array_at(row: &serde_json::Value, pointer: &str) -> f64 {
    let Some(values) = row.pointer(pointer).and_then(|value| value.as_array()) else {
        return 0.0;
    };
    let mut total = 0.0;
    let mut count = 0.0;
    for value in values {
        if let Some(sample) = value.as_f64() {
            total += sample;
            count += 1.0;
        }
    }
    if count == 0.0 { 0.0 } else { total / count }
}

fn short_hash(value: &str) -> String {
    value.chars().take(12).collect()
}

fn array_strings_at(row: &serde_json::Value, pointer: &str) -> Vec<String> {
    row.pointer(pointer)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn summarize_set(values: &BTreeSet<String>, limit: usize) -> String {
    if values.is_empty() {
        return "none declared".to_string();
    }
    let mut selected = values.iter().take(limit).cloned().collect::<Vec<_>>();
    let remaining = values.len().saturating_sub(selected.len());
    if remaining > 0 {
        selected.push(format!("+{remaining} more"));
    }
    selected.join("; ")
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
