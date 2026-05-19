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

const SOL_BASELINE: &str = "solc-latest-legacy-runs200-metadata-on";
const VYPER_BASELINE: &str = "vyper-latest-gas-metadata-on";
const SOL_CODEGEN_BASELINE: &str = "solc-latest-legacy-runs200-metadata-off";
const SOL_VIAIR_CODEGEN: &str = "solc-latest-viair-runs200-metadata-off";
const VYPER_GAS_CODEGEN: &str = "vyper-latest-gas-metadata-off";
const VYPER_CODESIZE_CODEGEN: &str = "vyper-latest-codesize-metadata-off";
const PRIMARY_CODEGEN_PROFILES: [&str; 4] = [
    SOL_CODEGEN_BASELINE,
    SOL_VIAIR_CODEGEN,
    VYPER_GAS_CODEGEN,
    VYPER_CODESIZE_CODEGEN,
];

pub struct ReportPaths {
    pub normalized_results: PathBuf,
    pub run_manifest: PathBuf,
    pub html_report: PathBuf,
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

    let rows = normalized_rows(root, compiled, gas_records, scenarios)?;
    let normalized_results = normalized_dir.join("results.json");
    fs::write(&normalized_results, serde_json::to_string_pretty(&rows)?)?;

    let run_manifest = normalized_dir.join("run-manifest.json");
    let manifest = json!({
        "run_id": Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        "started_at": Utc::now(),
        "evm_version": toolchains.evm_version,
        "toolchains": [toolchains.solc, toolchains.vyper],
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
        "gas_records": gas_records.len()
    });
    fs::write(&run_manifest, serde_json::to_string_pretty(&manifest)?)?;

    let html_report = reports_dir.join("index.html");
    fs::write(&html_report, render_html(&rows, toolchains)?)?;

    Ok(ReportPaths {
        normalized_results,
        run_manifest,
        html_report,
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
            SOL_BASELINE => {
                solidity.insert(artifact.benchmark_id.clone());
            }
            VYPER_BASELINE => {
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

fn render_html(rows: &[serde_json::Value], toolchains: &Toolchains) -> Result<String> {
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
    html.push_str(".chart{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:12px;overflow:auto}.notice{background:#fff7ed;border:1px solid #fdba74;border-radius:8px;padding:12px;margin:12px 0;color:#7c2d12}.info{background:#eff6ff;border-color:#bfdbfe;color:#1e3a8a}.muted{color:var(--muted)}.small{font-size:11px;line-height:1.35}.mono{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}");
    html.push_str(".pill{display:inline-flex;align-items:center;border-radius:999px;padding:3px 8px;font-size:11px;border:1px solid var(--line);background:#f9fafb;white-space:nowrap}.pass{background:#ecfdf5;color:#065f46;border-color:#a7f3d0}.warn{background:#fffbeb;color:#92400e;border-color:#fde68a}.fail{background:#fef2f2;color:#991b1b;border-color:#fecaca}.na{background:#f4f4f5;color:#52525b;border-color:#e4e4e7}");
    html.push_str(".bar{height:9px;background:#e5e7eb;border-radius:999px;min-width:92px;position:relative;overflow:hidden}.bar span{display:block;height:100%;border-radius:999px;background:var(--blue)}.bar.good span{background:var(--green)}.bar.warn span{background:var(--amber)}.bar.fail span{background:var(--red)}.ratio{display:grid;grid-template-columns:58px 1fr;gap:8px;align-items:center;min-width:160px}");
    html.push_str(".heat-ok{background:#dcfce7;color:#166534;text-align:center}.heat-fail{background:#fee2e2;color:#991b1b;text-align:center}.heat-na{background:#f4f4f5;color:#71717a;text-align:center}.legend{display:flex;gap:12px;align-items:center;font-size:12px;color:var(--muted);margin:8px 0}.dot{display:inline-block;width:9px;height:9px;border-radius:50%;margin-right:4px}.sol{background:var(--blue)}.vy{background:var(--green)}details{margin-top:12px}details>summary{cursor:pointer;font-weight:700;margin:12px 0}");
    html.push_str(".brief{display:grid;grid-template-columns:1.35fr .65fr;gap:14px;align-items:start}.takeaways{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:10px;margin-top:12px}.takeaway{background:#f8fafc;border:1px solid var(--line);border-radius:8px;padding:12px}.takeaway strong{display:block;margin-bottom:5px}.answer{font-size:18px;font-weight:700;margin:0 0 6px}.section-lede{font-size:13px;color:var(--muted);max-width:900px}.mini-table td,.mini-table th{font-size:12px}.callout{background:#f8fafc;border:1px solid var(--line);border-radius:8px;padding:12px;margin:10px 0}");
    html.push_str("@media(max-width:900px){.grid,.three,.two,.scope,.brief,.takeaways{grid-template-columns:1fr}main{padding:20px 14px}.v{font-size:20px}}");
    html.push_str("</style></head><body><main>");
    html.push_str("<h1>EVM Compiler Bench</h1>");
    html.push_str(&format!(
        "<div class=\"meta\">Behaviorally matched Solidity/Vyper compiler-profile evaluation. EVM: {} &middot; solc {} &middot; Vyper {}.</div>",
        escape(&toolchains.evm_version),
        escape(&toolchains.solc.version),
        escape(&toolchains.vyper.version)
    ));
    html.push_str(&render_overview(rows, &summary));
    html.push_str("<nav><a href=\"#validity\">Validity</a><a href=\"#reliability\">Reliability</a><a href=\"#fixed\">Fixed Suite</a><a href=\"#profiles\">Profiles</a><a href=\"#metadata\">Metadata</a><a href=\"#compile\">Compile Resources</a><a href=\"#scale\">Scale Studies</a><a href=\"#real\">Real-Derived Models</a><a href=\"#raw\">Raw Rows</a></nav>");

    html.push_str("<section id=\"validity\"><h2>Correctness And Measurement Scope</h2>");
    html.push_str(&render_validity_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"reliability\"><h2>Compile Reliability</h2>");
    html.push_str(&render_reliability_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"fixed\"><h2>Fixed Matched Suite</h2>");
    html.push_str(&render_fixed_suite_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"profiles\"><h2>Compiler Profile Tradeoffs</h2>");
    html.push_str(&render_profile_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"metadata\"><h2>Metadata Overhead</h2>");
    html.push_str(&render_metadata_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"compile\"><h2>Compile Time And RSS</h2>");
    html.push_str(&render_compile_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"scale\"><h2>Generated Scale Studies</h2>");
    html.push_str(&render_scale_brief(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"real\"><h2>Real-Derived Benchmark Models</h2>");
    html.push_str(&render_real_derived_summary(rows));
    html.push_str("</section>");

    html.push_str("<section id=\"raw\"><h2>Raw Rows</h2><details><summary>Show normalized result rows</summary>");
    html.push_str(&render_raw_rows_table(rows));
    html.push_str("</details></section></main></body></html>");
    Ok(html)
}

fn render_overview(rows: &[serde_json::Value], summary: &ReportSummary) -> String {
    let mut html = String::new();
    html.push_str("<section class=\"hero brief\"><div>");
    html.push_str("<div class=\"pill info\">Eval brief</div>");
    html.push_str("<p class=\"lede\">This run compares compiler profiles across behaviorally matched workloads. The useful question is not which language wins globally; it is where compiler/profile choices change reliability, bytecode size, deployment cost, runtime gas, and compile resources.</p>");
    html.push_str(&render_takeaways(rows, summary));
    html.push_str("</div><div class=\"card\">");
    html.push_str(
        "<div class=\"k\">Safe reading</div><div class=\"v\">Tradeoffs, not a leaderboard</div>",
    );
    html.push_str("<div class=\"subv\">Default comparisons use metadata-off codegen profiles and scenario-level ratios. Full matrices and raw rows remain available as drilldowns.</div>");
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
        "compiler, optimizer, metadata matrix",
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
        "Scenario status",
        &format!(
            "{} pass / {} fail",
            summary.scenario_status_pass, summary.scenario_status_fail
        ),
        "success or revert matched expectation",
    ));
    html.push_str(&metric_card(
        "Golden/log checks",
        &format!("{} / {}", summary.golden_rows, summary.log_rows),
        "exact golden behavior / log checks currently run",
    ));
    html.push_str("</section>");

    html.push_str("<section class=\"two\">");
    html.push_str("<div class=\"card\"><h3>What can be safely inferred?</h3><p class=\"small muted\">Fixed rows support matched-workload compiler/profile comparisons. Scale rows support compiler stress and growth-shape claims. Real-derived rows support scoped model hot-path claims only after reading their exclusions and mock assumptions.</p></div>");
    html.push_str("<div class=\"card\"><h3>Where are the tradeoffs?</h3><p class=\"small muted\">Read reliability before gas, compare ratios only within the same benchmark/scenario/metadata/state profile, and treat bytecode, deployment, runtime gas, compile time, and RSS as separate dimensions.</p></div>");
    html.push_str("</section>");
    html
}

fn render_takeaways(rows: &[serde_json::Value], summary: &ReportSummary) -> String {
    let metadata = metadata_overhead_summary(rows);
    let fixed_deltas = collect_scenario_deltas(
        rows,
        "fixed",
        &[SOL_VIAIR_CODEGEN, VYPER_GAS_CODEGEN, VYPER_CODESIZE_CODEGEN],
    );
    let largest_delta = fixed_deltas.iter().max_by(|left, right| {
        left.ratio
            .ln()
            .abs()
            .partial_cmp(&right.ratio.ln().abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let failure = compile_failure_story(rows);

    let mut html = String::new();
    html.push_str("<div class=\"takeaways\">");
    html.push_str("<div class=\"takeaway\"><strong>Reliability</strong><span class=\"small\">");
    html.push_str(&escape(&failure));
    html.push_str("</span></div>");
    html.push_str(
        "<div class=\"takeaway\"><strong>Correctness coverage</strong><span class=\"small\">",
    );
    html.push_str(&format!(
        "{} scenario rows matched expected success/revert; {} rows have randomized checks and {} rows have property checks. Golden/log checks are not yet run.",
        summary.scenario_status_pass, summary.randomized_rows, summary.property_rows
    ));
    html.push_str("</span></div>");
    html.push_str(
        "<div class=\"takeaway\"><strong>Runtime comparisons</strong><span class=\"small\">",
    );
    if let Some(delta) = largest_delta {
        html.push_str(&format!(
            "Largest fixed-suite headline delta is {} at {} ({:+} gas vs baseline). Ratios are scenario-level, metadata-off internal-call measurements.",
            ratio_text(delta.ratio),
            escape(&format!("{} / {}", delta.label, profile_short(&delta.profile))),
            delta.delta
        ));
    } else {
        html.push_str("No fixed-suite runtime ratios were available for the headline profiles.");
    }
    html.push_str("</span></div>");
    html.push_str(
        "<div class=\"takeaway\"><strong>Metadata is separate</strong><span class=\"small\">",
    );
    html.push_str(&format!(
        "Across {} paired artifacts, metadata-on changes creation bytecode by about {} bytes on average; codegen views default to metadata-off.",
        metadata.pairs,
        signed(metadata.avg_creation_delta)
    ));
    html.push_str("</span></div>");
    html.push_str("</div>");
    html
}

fn render_validity_brief(rows: &[serde_json::Value]) -> String {
    let summary = ReportSummary::from_rows(rows);
    let mut html = String::new();
    html.push_str("<p class=\"answer\">Can I trust the rows enough to compare gas and size?</p>");
    html.push_str("<p class=\"section-lede\">Enough for scoped compiler-profile tradeoffs, not enough for a global language or production-protocol claim. Scenario status checks are broad; deeper semantic checks are layered and currently limited.</p>");
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
    let deltas = collect_scenario_deltas(
        rows,
        "fixed",
        &[SOL_VIAIR_CODEGEN, VYPER_GAS_CODEGEN, VYPER_CODESIZE_CODEGEN],
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

    let mut html = String::new();
    html.push_str("<p class=\"answer\">Where does runtime gas actually move?</p>");
    html.push_str(&format!(
        "<p class=\"section-lede\">Headline runtime comparisons are same benchmark, same scenario, same state profile, metadata-off, relative to <span class=\"mono\">{}</span>. This keeps the first view readable and leaves the full scenario table as a drilldown.</p>",
        escape(SOL_CODEGEN_BASELINE)
    ));
    html.push_str("<section class=\"two\">");
    html.push_str(&render_delta_table(
        "Largest cheaper headline rows",
        &cheaper,
        8,
    ));
    html.push_str(&render_delta_table(
        "Largest costlier headline rows",
        &costlier,
        8,
    ));
    html.push_str("</section>");
    html.push_str(
        "<details><summary>Show all fixed-suite scenario ratios and Pareto scatter</summary>",
    );
    html.push_str(&render_fixed_scenario_ratios(rows));
    html.push_str(&render_fixed_pareto_svg(rows));
    html.push_str("</details>");
    html
}

fn render_profile_brief(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<p class=\"answer\">What do the main compiler profiles buy or cost?</p>");
    html.push_str("<p class=\"section-lede\">The default profile view hides metadata duplicates and noopt diagnostics. It compares the production-relevant codegen profiles first; the full profile table remains available below.</p>");
    html.push_str(&render_profile_tradeoff_table(
        rows,
        Some(&PRIMARY_CODEGEN_PROFILES),
    ));
    html.push_str("<details><summary>Show all metadata-off profile tradeoffs, including noopt diagnostics</summary>");
    html.push_str(&render_fixed_profile_tradeoffs(rows));
    html.push_str("</details>");
    html
}

fn render_metadata_brief(rows: &[serde_json::Value]) -> String {
    let summary = metadata_overhead_summary(rows);
    let mut html = String::new();
    html.push_str("<p class=\"answer\">How much of bytecode/deploy cost is metadata?</p>");
    html.push_str("<p class=\"section-lede\">Metadata is a deployment/verifiability dimension, not a codegen winner signal. The paired deltas below compare the same artifact/profile with metadata on versus off.</p>");
    html.push_str("<section class=\"grid\">");
    html.push_str(&metric_card(
        "Paired artifacts",
        &summary.pairs.to_string(),
        "same benchmark/profile, metadata on vs off",
    ));
    html.push_str(&metric_card(
        "Runtime bytes delta",
        &signed(summary.avg_runtime_delta),
        "average metadata-on minus metadata-off",
    ));
    html.push_str(&metric_card(
        "Creation bytes delta",
        &signed(summary.avg_creation_delta),
        "average metadata-on minus metadata-off",
    ));
    html.push_str(&metric_card(
        "Internal create gas delta",
        &signed(summary.avg_create_gas_delta),
        "average metadata-on minus metadata-off",
    ));
    html.push_str("</section>");
    html.push_str("<details><summary>Show metadata deltas by profile family</summary>");
    html.push_str(&render_metadata_delta_summary(rows));
    html.push_str("</details>");
    html
}

fn render_compile_brief(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<p class=\"answer\">What are the compile-time and memory tradeoffs?</p>");
    html.push_str("<p class=\"section-lede\">Compile resources are artifact-level measurements. The default view shows the main codegen profiles; all metadata/noopt variants are available as a drilldown.</p>");
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
    html.push_str("<details><summary>Show scale max-N matrix and aggregate scale table</summary>");
    html.push_str(&render_scale_max_n(rows));
    html.push_str(&render_scale_summary(rows));
    html.push_str("</details>");
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
    html.push_str("<div class=\"notice small info\"><strong>Measurement semantics:</strong> <span class=\"mono\">scenario_status_check=pass</span> means the call succeeded or reverted as expected. It is not exact semantic equivalence unless paired with return, observer, log, randomized, property, or golden checks.</div>");
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
    let selected_profiles = [SOL_VIAIR_CODEGEN, VYPER_GAS_CODEGEN, VYPER_CODESIZE_CODEGEN];
    let mut baseline = BTreeMap::new();
    let mut values: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut labels = BTreeMap::new();
    for row in rows {
        if !is_ok_fixed_metadata_off(row) {
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
    html.push_str("<h3>Scenario Gas Ratios</h3>");
    html.push_str(&format!(
        "<p class=\"small muted\">Ratios compare the same benchmark, scenario, state profile, and metadata-off mode against <span class=\"mono\">{}</span>. Lower is less harness call gas.</p>",
        escape(SOL_CODEGEN_BASELINE)
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

fn render_fixed_pareto_svg(rows: &[serde_json::Value]) -> String {
    let selected = rows
        .iter()
        .filter(|row| is_ok_fixed_metadata_off(row))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return String::new();
    }
    let max_size = selected
        .iter()
        .map(|row| u64_at(row, "/bytecode/runtime_bytes"))
        .max()
        .unwrap_or(1)
        .max(1);
    let max_gas = selected
        .iter()
        .map(|row| u64_at(row, "/gas/harness_call_gas"))
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::new();
    html.push_str("<h3>Fixed-Suite Pareto Surface</h3>");
    html.push_str("<div class=\"legend\"><span><span class=\"dot sol\"></span>Solidity profile</span><span><span class=\"dot vy\"></span>Vyper profile</span><span>metadata-off only</span></div>");
    html.push_str("<div class=\"chart\"><svg width=\"1080\" height=\"390\" viewBox=\"0 0 1080 390\" role=\"img\" aria-label=\"Runtime bytecode size versus harness call gas for fixed suite metadata-off rows\">");
    html.push_str("<line x1=\"64\" y1=\"330\" x2=\"1030\" y2=\"330\" stroke=\"#9ca3af\"/><line x1=\"64\" y1=\"24\" x2=\"64\" y2=\"330\" stroke=\"#9ca3af\"/>");
    html.push_str("<text x=\"540\" y=\"374\" text-anchor=\"middle\" font-size=\"12\" fill=\"#657083\">runtime bytes</text>");
    html.push_str("<text x=\"18\" y=\"190\" text-anchor=\"middle\" font-size=\"12\" fill=\"#657083\" transform=\"rotate(-90 18 190)\">harness call gas</text>");
    for row in selected {
        let size = u64_at(row, "/bytecode/runtime_bytes");
        let gas = u64_at(row, "/gas/harness_call_gas");
        let x = 64 + (size * 940 / max_size) as i32;
        let y = 330 - (gas * 292 / max_gas) as i32;
        let color = if str_at(row, "/language").as_deref() == Some("vyper") {
            "#0f8a5f"
        } else {
            "#2563eb"
        };
        html.push_str(&format!(
            "<circle cx=\"{x}\" cy=\"{y}\" r=\"3.5\" fill=\"{color}\" opacity=\".68\"><title>{}</title></circle>",
            escape(&tooltip(row))
        ));
    }
    html.push_str("</svg></div>");
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
    creation_bytes: u64,
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

#[derive(Debug, Default)]
struct MetadataOverheadSummary {
    pairs: usize,
    avg_runtime_delta: i64,
    avg_creation_delta: i64,
    avg_create_gas_delta: i64,
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
        "<p class=\"small muted\">Geometric mean ratios over fixed-suite metadata-off rows, relative to <span class=\"mono\">{}</span>. Runtime gas is scenario-level; bytecode, deploy, and compile metrics are artifact-level.</p>",
        escape(SOL_CODEGEN_BASELINE)
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
        if !is_ok_fixed_metadata_off(row)
            || str_at(row, "/profile_id").as_deref() != Some(SOL_CODEGEN_BASELINE)
        {
            continue;
        }
        baseline_gas.insert(scenario_key(row), u64_at(row, "/gas/harness_call_gas"));
        let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
        let artifact_key = artifact_key_from_row(row);
        if seen.insert(artifact_key) {
            baseline_artifacts.insert(benchmark, artifact_metrics(row));
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
        if let Some(base_gas) = baseline_gas.get(&scenario_key(row)).copied() {
            push_ratio(
                &mut entry.gas,
                u64_at(row, "/gas/harness_call_gas"),
                base_gas,
            );
        }
        let artifact_key = artifact_key_from_row(row);
        if seen_artifacts.insert(artifact_key) {
            let benchmark = str_at(row, "/benchmark_id").unwrap_or_default();
            if let Some(base) = baseline_artifacts.get(&benchmark).copied() {
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

fn metadata_overhead_summary(rows: &[serde_json::Value]) -> MetadataOverheadSummary {
    let mut pairs: BTreeMap<String, MetadataPair> = BTreeMap::new();
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
        let base_profile = profile_without_metadata(&profile);
        let key = format!(
            "{}\0{}\0{}",
            str_at(row, "/benchmark_id").unwrap_or_default(),
            str_at(row, "/implementation_id").unwrap_or_default(),
            base_profile
        );
        let entry = pairs.entry(key).or_default();
        match str_at(row, "/gas/metadata_mode").as_deref() {
            Some("off") => entry.off = Some(artifact_metrics(row)),
            Some("on") => entry.on = Some(artifact_metrics(row)),
            _ => {}
        }
    }

    let mut summary = MetadataOverheadSummary::default();
    let mut runtime_delta = 0;
    let mut creation_delta = 0;
    let mut create_gas_delta = 0;
    for pair in pairs.values() {
        let Some(off) = pair.off else {
            continue;
        };
        let Some(on) = pair.on else {
            continue;
        };
        summary.pairs += 1;
        runtime_delta += on.runtime_bytes as i64 - off.runtime_bytes as i64;
        creation_delta += on.creation_bytes as i64 - off.creation_bytes as i64;
        create_gas_delta += on.internal_create_gas as i64 - off.internal_create_gas as i64;
    }
    let pairs = summary.pairs.max(1) as i64;
    summary.avg_runtime_delta = runtime_delta / pairs;
    summary.avg_creation_delta = creation_delta / pairs;
    summary.avg_create_gas_delta = create_gas_delta / pairs;
    summary
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
        aggregate.artifacts += 1;
        aggregate.total_wall_ms += metrics.compile_ms;
        aggregate.total_rss_kib += metrics.peak_rss_kib;
        aggregate.max_rss_kib = aggregate.max_rss_kib.max(metrics.peak_rss_kib);
    }
    let max_wall = aggregates
        .values()
        .map(|aggregate| aggregate.total_wall_ms / aggregate.artifacts.max(1) as f64)
        .fold(0.0, f64::max)
        .max(1.0);
    let max_rss = aggregates
        .values()
        .map(|aggregate| aggregate.max_rss_kib)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Profile</th><th>Artifacts</th><th>Avg Wall ms</th><th>Max RSS KiB</th><th>Avg RSS KiB</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        let artifacts = aggregate.artifacts.max(1);
        let avg_wall = aggregate.total_wall_ms / artifacts as f64;
        let avg_rss = aggregate.total_rss_kib / artifacts as u64;
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile_short(&profile)));
        html.push_str("<br><span class=\"small muted\">");
        html.push_str(&escape(&profile));
        html.push_str("</span></td><td>");
        html.push_str(&aggregate.artifacts.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(avg_wall, max_wall, &format!("{avg_wall:.2}")));
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
struct MetadataPair {
    off: Option<ArtifactMetrics>,
    on: Option<ArtifactMetrics>,
}

#[derive(Debug, Default)]
struct MetadataAggregate {
    pairs: usize,
    runtime_delta: i64,
    creation_delta: i64,
    create_gas_delta: i64,
}

fn render_metadata_delta_summary(rows: &[serde_json::Value]) -> String {
    let mut pairs: BTreeMap<String, MetadataPair> = BTreeMap::new();
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
        let base_profile = profile_without_metadata(&profile);
        let key = format!(
            "{}\0{}\0{}",
            str_at(row, "/benchmark_id").unwrap_or_default(),
            str_at(row, "/implementation_id").unwrap_or_default(),
            base_profile
        );
        let entry = pairs.entry(key).or_default();
        match str_at(row, "/gas/metadata_mode").as_deref() {
            Some("off") => entry.off = Some(artifact_metrics(row)),
            Some("on") => entry.on = Some(artifact_metrics(row)),
            _ => {}
        }
    }

    let mut aggregates: BTreeMap<String, MetadataAggregate> = BTreeMap::new();
    for (key, pair) in pairs {
        let Some(off) = pair.off else {
            continue;
        };
        let Some(on) = pair.on else {
            continue;
        };
        let base_profile = key.rsplit('\0').next().unwrap_or_default().to_string();
        let aggregate = aggregates.entry(base_profile).or_default();
        aggregate.pairs += 1;
        aggregate.runtime_delta += on.runtime_bytes as i64 - off.runtime_bytes as i64;
        aggregate.creation_delta += on.creation_bytes as i64 - off.creation_bytes as i64;
        aggregate.create_gas_delta +=
            on.internal_create_gas as i64 - off.internal_create_gas as i64;
    }

    let mut html = String::new();
    html.push_str("<p class=\"small muted\">Paired metadata-on minus metadata-off deltas for the same benchmark, implementation, and optimizer profile. These are additive deltas, not ratios, because metadata dominates tiny contracts.</p>");
    html.push_str("<table><thead><tr><th>Profile Family</th><th>Paired Artifacts</th><th>Avg Runtime Bytes Delta</th><th>Avg Creation Bytes Delta</th><th>Avg Internal Create Gas Delta</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        let pairs = aggregate.pairs.max(1) as i64;
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile));
        html.push_str("</td><td>");
        html.push_str(&aggregate.pairs.to_string());
        html.push_str("</td><td>");
        html.push_str(&(aggregate.runtime_delta / pairs).to_string());
        html.push_str("</td><td>");
        html.push_str(&(aggregate.creation_delta / pairs).to_string());
        html.push_str("</td><td>");
        html.push_str(&(aggregate.create_gas_delta / pairs).to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

#[derive(Debug, Default)]
struct CompileAggregate {
    artifacts: usize,
    total_wall_ms: f64,
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
        aggregate.artifacts += 1;
        aggregate.total_wall_ms += metrics.compile_ms;
        aggregate.total_rss_kib += metrics.peak_rss_kib;
        aggregate.max_rss_kib = aggregate.max_rss_kib.max(metrics.peak_rss_kib);
    }
    let max_wall = aggregates
        .values()
        .map(|aggregate| aggregate.total_wall_ms / aggregate.artifacts.max(1) as f64)
        .fold(0.0, f64::max)
        .max(1.0);
    let max_rss = aggregates
        .values()
        .map(|aggregate| aggregate.max_rss_kib)
        .max()
        .unwrap_or(1)
        .max(1);

    let mut html = String::new();
    html.push_str("<p class=\"small muted\">Compile metrics are artifact-level. Wall time is the mean of the recorded samples for each artifact; RSS is the recorded peak for that compiler invocation set.</p>");
    html.push_str("<table><thead><tr><th>Profile</th><th>Artifacts</th><th>Avg Wall ms</th><th>Max RSS KiB</th><th>Avg RSS KiB</th></tr></thead><tbody>");
    for (profile, aggregate) in aggregates {
        let artifacts = aggregate.artifacts.max(1);
        let avg_wall = aggregate.total_wall_ms / artifacts as f64;
        let avg_rss = aggregate.total_rss_kib / artifacts as u64;
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&profile));
        html.push_str("</td><td>");
        html.push_str(&aggregate.artifacts.to_string());
        html.push_str("</td><td>");
        html.push_str(&bar_value(avg_wall, max_wall, &format!("{avg_wall:.2}")));
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

#[derive(Debug, Default)]
struct ScaleCell {
    max_ok: Option<u64>,
    failed_values: BTreeSet<u64>,
}

fn render_scale_max_n(rows: &[serde_json::Value]) -> String {
    let mut profiles = BTreeSet::new();
    let mut cells: BTreeMap<String, BTreeMap<String, ScaleCell>> = BTreeMap::new();
    let mut seen_ok = BTreeSet::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some("scale") {
            continue;
        }
        let family = str_at(row, "/family").unwrap_or_default();
        let profile = str_at(row, "/profile_id").unwrap_or_default();
        let value = u64_at(row, "/parameter_value");
        profiles.insert(profile.clone());
        let cell = cells.entry(family).or_default().entry(profile).or_default();
        match str_at(row, "/status").as_deref() {
            Some("ok") => {
                let key = artifact_key_from_row(row);
                if seen_ok.insert(key) {
                    cell.max_ok = Some(cell.max_ok.unwrap_or(0).max(value));
                }
            }
            Some("compile_error") => {
                cell.failed_values.insert(value);
            }
            _ => {}
        }
    }
    let mut html = String::new();
    html.push_str("<h3>Scale Failure And Max-N Matrix</h3>");
    html.push_str("<p class=\"small muted\">Cells show the largest N compiled for each generated family/profile and any failed N values. This keeps compiler reliability cliffs visible instead of averaging them away.</p>");
    html.push_str("<div class=\"chart\"><table><thead><tr><th>Family</th>");
    for profile in &profiles {
        html.push_str("<th>");
        html.push_str(&escape(profile));
        html.push_str("</th>");
    }
    html.push_str("</tr></thead><tbody>");
    for (family, by_profile) in cells {
        html.push_str("<tr><td class=\"mono\">");
        html.push_str(&escape(&family));
        html.push_str("</td>");
        for profile in &profiles {
            let Some(cell) = by_profile.get(profile) else {
                html.push_str("<td class=\"heat-na\">n/a</td>");
                continue;
            };
            let class = if cell.failed_values.is_empty() {
                "heat-ok"
            } else {
                "heat-fail"
            };
            html.push_str("<td class=\"");
            html.push_str(class);
            html.push_str("\">");
            if let Some(max_ok) = cell.max_ok {
                html.push_str("max ");
                html.push_str(&max_ok.to_string());
            } else {
                html.push_str("none");
            }
            if !cell.failed_values.is_empty() {
                html.push_str("<br><span class=\"small\">fail ");
                html.push_str(
                    &cell
                        .failed_values
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                );
                html.push_str("</span>");
            }
            html.push_str("</td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
    html
}

fn render_raw_rows_table(rows: &[serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Status</th><th>Suite</th><th>Family</th><th>N</th><th>Benchmark</th><th>Implementation</th><th>Compiler</th><th>Profile</th><th>Metadata</th><th>State</th><th>Scenario</th><th>Status Check</th><th>Baseline Diff</th><th>Randomized</th><th>Property</th><th>Failures</th><th>Runtime Bytes</th><th>Internal Create Gas</th><th>Harness Call Gas</th><th>Calldata Gas</th><th>Harness Est. Tx Gas</th></tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr><td>");
        html.push_str(&escape(&str_at(row, "/status").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/suite").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/family").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/parameter_value").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/benchmark_id").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/implementation_id").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(&format!(
            "{} {}",
            str_at(row, "/compiler/name").unwrap_or_default(),
            str_at(row, "/compiler/version").unwrap_or_default()
        )));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/profile_id").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/gas/metadata_mode").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/gas/state_access_profile").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/gas/scenario").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/correctness/scenario_status_check").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/correctness/baseline_differential_check").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/correctness/randomized_differential_check").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/correctness/property_tests").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&failure_links_html(row));
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/bytecode/runtime_bytes").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/internal_create_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/harness_call_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/calldata_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/harness_estimated_tx_gas").to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
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
        &[SOL_VIAIR_CODEGEN, VYPER_GAS_CODEGEN, VYPER_CODESIZE_CODEGEN],
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
    html.push_str(
        "<details><summary>Show real-derived scenario ratios and row-level provenance</summary>",
    );
    html.push_str(&render_real_derived_scenario_ratios(&selected));
    html.push_str(&render_real_derived_detail_table(&selected));
    html.push_str("</details>");
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
    html.push_str("<table><thead><tr><th>Model</th><th>Scope</th><th>Coverage</th><th>Reader Guardrails</th></tr></thead><tbody>");
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
        html.push_str("<span class=\"muted\">mocked:</span> ");
        html.push_str(&escape(&summarize_set(&scope.assumptions, 2)));
        html.push_str("<br><span class=\"muted\">excluded:</span> ");
        html.push_str(&escape(&summarize_set(&scope.exclusions, 3)));
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_real_derived_detail_table(rows: &[&serde_json::Value]) -> String {
    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Upstream</th><th>Benchmark</th><th>Model</th><th>Production Equivalent</th><th>Source</th><th>Port</th><th>Profile</th><th>Status</th><th>Scenario</th><th>Assumptions / Exclusions</th><th>Runtime Bytes</th><th>Internal Create Gas</th><th>Harness Call Gas</th><th>Harness Est. Tx Gas</th></tr></thead><tbody>");
    for row in rows {
        html.push_str("<tr><td>");
        html.push_str(&escape(
            &str_at(row, "/provenance/upstream_project").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/benchmark_id").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/provenance/model_kind").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/provenance/production_equivalence").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(&format!(
            "{}:{}",
            str_at(row, "/provenance/source_language").unwrap_or_default(),
            str_at(row, "/provenance/source_path").unwrap_or_default()
        )));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/language").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/profile_id").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/status").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str(&escape(&str_at(row, "/gas/scenario").unwrap_or_default()));
        html.push_str("</td><td>");
        html.push_str("<div class=\"small\"><span class=\"muted\">assumptions:</span> ");
        html.push_str(&escape(&joined_array_at(
            row,
            "/provenance/mock_assumptions",
        )));
        html.push_str("<br><span class=\"muted\">excluded:</span> ");
        html.push_str(&escape(&joined_array_at(
            row,
            "/provenance/excluded_features",
        )));
        html.push_str("</div>");
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/bytecode/runtime_bytes").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/internal_create_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/harness_call_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/harness_estimated_tx_gas").to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
}

fn render_real_derived_scenario_ratios(rows: &[&serde_json::Value]) -> String {
    let selected_profiles = [SOL_VIAIR_CODEGEN, VYPER_GAS_CODEGEN, VYPER_CODESIZE_CODEGEN];
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
        "<p class=\"small muted\">Metadata-off hot-path model rows compared against <span class=\"mono\">{}</span> within the same model, scenario, and state profile. These bars are scoped to the benchmark model, not production protocol gas.</p>",
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
struct ScaleAggregate {
    family: String,
    parameter_value: u64,
    language: String,
    profile_id: String,
    compile_wall_ms: f64,
    compile_metric_artifacts: BTreeSet<String>,
    runtime_bytes: u64,
    internal_create_gas: u64,
    harness_call_gas: u64,
    metric_samples: u64,
    compile_failures: u64,
}

fn render_scale_summary(rows: &[serde_json::Value]) -> String {
    let mut groups: BTreeMap<String, ScaleAggregate> = BTreeMap::new();
    for row in rows {
        if str_at(row, "/suite").as_deref() != Some(BenchmarkSuite::Scale.as_str()) {
            continue;
        }
        let family = str_at(row, "/family").unwrap_or_default();
        let parameter_value = u64_at(row, "/parameter_value");
        let language = str_at(row, "/language").unwrap_or_default();
        let profile_id = str_at(row, "/profile_id").unwrap_or_default();
        let key = format!("{family}\0{parameter_value:020}\0{language}\0{profile_id}");
        let entry = groups.entry(key).or_insert_with(|| ScaleAggregate {
            family,
            parameter_value,
            language,
            profile_id: profile_id.clone(),
            ..ScaleAggregate::default()
        });
        if str_at(row, "/status").as_deref() == Some("compile_error") {
            entry.compile_failures += 1;
        } else {
            let compile_key = format!(
                "{}\0{}\0{}",
                str_at(row, "/benchmark_id").unwrap_or_default(),
                str_at(row, "/implementation_id").unwrap_or_default(),
                profile_id
            );
            if entry.compile_metric_artifacts.insert(compile_key) {
                entry.compile_wall_ms += mean_f64_array_at(row, "/compile/wall_ms_samples");
            }
            entry.runtime_bytes += u64_at(row, "/bytecode/runtime_bytes");
            entry.internal_create_gas += u64_at(row, "/gas/internal_create_gas");
            entry.harness_call_gas += u64_at(row, "/gas/harness_call_gas");
            entry.metric_samples += 1;
        }
    }

    if groups.is_empty() {
        return "<div class=\"card\">No generated scale rows in this report.</div>".to_string();
    }

    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Family</th><th>N</th><th>Language</th><th>Profile</th><th>Avg Compile ms</th><th>Avg Runtime Bytes</th><th>Avg Internal Create Gas</th><th>Avg Harness Call Gas</th><th>Run Samples</th><th>Compile Failures</th></tr></thead><tbody>");
    for aggregate in groups.values() {
        let samples = aggregate.metric_samples.max(1) as f64;
        let compile_samples = aggregate.compile_metric_artifacts.len().max(1) as f64;
        html.push_str("<tr><td>");
        html.push_str(&escape(&aggregate.family));
        html.push_str("</td><td>");
        html.push_str(&aggregate.parameter_value.to_string());
        html.push_str("</td><td>");
        html.push_str(&escape(&aggregate.language));
        html.push_str("</td><td>");
        html.push_str(&escape(&aggregate.profile_id));
        html.push_str("</td><td>");
        html.push_str(&format!(
            "{:.2}",
            aggregate.compile_wall_ms / compile_samples
        ));
        html.push_str("</td><td>");
        html.push_str(&((aggregate.runtime_bytes as f64 / samples).round() as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(
            &((aggregate.internal_create_gas as f64 / samples).round() as u64).to_string(),
        );
        html.push_str("</td><td>");
        html.push_str(&((aggregate.harness_call_gas as f64 / samples).round() as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.metric_samples.to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.compile_failures.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
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
        creation_bytes: u64_at(row, "/bytecode/creation_bytes"),
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

fn profile_without_metadata(profile: &str) -> String {
    profile
        .strip_suffix("-metadata-on")
        .or_else(|| profile.strip_suffix("-metadata-off"))
        .unwrap_or(profile)
        .to_string()
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
    let base = profile_without_metadata(profile);
    let label = match base.as_str() {
        "solc-latest-legacy-runs200" => "solc legacy",
        "solc-latest-viair-runs200" => "solc viaIR",
        "solc-latest-noopt" => "solc noopt",
        "vyper-latest-gas" => "vyper gas",
        "vyper-latest-codesize" => "vyper codesize",
        "vyper-latest-none" => "vyper none",
        _ => profile,
    };
    if profile.ends_with("-metadata-on") {
        format!("{label} metadata-on")
    } else {
        label.to_string()
    }
}

fn ratio_text(ratio: f64) -> String {
    format!("{ratio:.2}x")
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

fn tooltip(row: &serde_json::Value) -> String {
    if str_at(row, "/status").as_deref() == Some("compile_error") {
        return format!(
            "{} / {} / {} / compile error",
            str_at(row, "/benchmark_id").unwrap_or_default(),
            str_at(row, "/implementation_id").unwrap_or_default(),
            str_at(row, "/profile_id").unwrap_or_default()
        );
    }
    format!(
        "{} / {} / {} / {} / harness call gas {}",
        str_at(row, "/benchmark_id").unwrap_or_default(),
        str_at(row, "/implementation_id").unwrap_or_default(),
        str_at(row, "/profile_id").unwrap_or_default(),
        str_at(row, "/gas/scenario").unwrap_or_default(),
        u64_at(row, "/gas/harness_call_gas")
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

fn joined_array_at(row: &serde_json::Value, pointer: &str) -> String {
    array_strings_at(row, pointer).join("; ")
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

fn failure_links_html(row: &serde_json::Value) -> String {
    let Some(links) = row
        .pointer("/correctness/failure_artifacts")
        .and_then(|value| value.as_array())
    else {
        return String::new();
    };
    if links.is_empty() {
        return String::new();
    }
    links
        .iter()
        .filter_map(|link| link.as_str())
        .map(|link| {
            let escaped = escape(link);
            format!("<a href=\"../../{escaped}\">{escaped}</a>")
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
