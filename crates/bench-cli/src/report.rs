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
    fs,
    path::{Path, PathBuf},
};

const SOL_BASELINE: &str = "solc-latest-legacy-runs200-metadata-on";
const VYPER_BASELINE: &str = "vyper-latest-gas-metadata-on";

pub struct ReportPaths {
    pub normalized_results: PathBuf,
    pub run_manifest: PathBuf,
    pub html_report: PathBuf,
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
            "deploy_gas": gas.deploy_gas,
            "execution_gas": gas.execution_gas,
            "intrinsic_gas": gas.intrinsic_gas,
            "calldata_gas": gas.calldata_gas,
            "total_tx_gas": gas.total_tx_gas,
        },
        "correctness": {
            "call_semantics": "pass",
            "golden_tests": "not_run",
            "differential_tests": baseline_status,
            "baseline_differential": baseline_status,
            "profile_differential": "not_run",
            "observer_check": if has_observers { baseline_status } else { "not_applicable" },
            "return_data_check": baseline_status,
            "log_check": "not_run",
            "randomized_differential": randomized_status,
            "property_tests": property_status,
            "properties": scenario_file.properties.iter().map(|property| property.name.clone()).collect::<Vec<_>>(),
            "failure_artifacts": failure_links,
            "success": gas.success
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
            "call_semantics": "not_applicable",
            "golden_tests": "not_applicable",
            "differential_tests": "not_applicable",
            "baseline_differential": "not_applicable",
            "profile_differential": "not_applicable",
            "observer_check": "not_applicable",
            "return_data_check": "not_applicable",
            "log_check": "not_applicable",
            "randomized_differential": "not_applicable",
            "property_tests": "not_applicable",
            "properties": [],
            "failure_artifacts": [],
            "success": false
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
    let mut by_profile: BTreeMap<String, (u64, usize)> = BTreeMap::new();
    let mut by_benchmark: BTreeMap<String, usize> = BTreeMap::new();
    let mut compile_failures = 0;
    for row in rows {
        if str_at(row, "/status").as_deref() == Some("ok") {
            let profile_id = str_at(row, "/profile_id").unwrap_or_default();
            let gas = u64_at(row, "/gas/execution_gas");
            let entry = by_profile.entry(profile_id).or_default();
            entry.0 += gas;
            entry.1 += 1;
        } else if str_at(row, "/status").as_deref() == Some("compile_error") {
            compile_failures += 1;
        }
        *by_benchmark
            .entry(str_at(row, "/benchmark_id").unwrap_or_default())
            .or_default() += 1;
    }

    let mut html = String::new();
    html.push_str("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>EVM Compiler Bench</title>");
    html.push_str("<style>");
    html.push_str("body{font-family:Inter,system-ui,-apple-system,sans-serif;margin:0;background:#f7f7f4;color:#1f2933}");
    html.push_str("main{max-width:1180px;margin:0 auto;padding:32px 24px 56px}");
    html.push_str("h1{font-size:32px;margin:0 0 6px}h2{font-size:20px;margin-top:32px}");
    html.push_str(".meta{color:#5b6472;margin-bottom:24px}.grid{display:grid;grid-template-columns:repeat(4,minmax(0,1fr));gap:12px}");
    html.push_str(".card{background:#fff;border:1px solid #ddd;border-radius:8px;padding:14px}.k{font-size:12px;color:#667085}.v{font-size:22px;font-weight:700}");
    html.push_str("table{width:100%;border-collapse:collapse;background:#fff;border:1px solid #ddd;border-radius:8px;overflow:hidden}");
    html.push_str("th,td{font-size:12px;text-align:left;padding:8px 10px;border-bottom:1px solid #ececec}th{background:#efefea}");
    html.push_str(".chart{background:#fff;border:1px solid #ddd;border-radius:8px;padding:12px;overflow:auto}circle{fill:#2563eb;opacity:.68}");
    html.push_str(".notice{background:#fff7ed;border:1px solid #fdba74;border-radius:8px;padding:12px;margin:12px 0;color:#7c2d12}");
    html.push_str(".muted{color:#667085}.small{font-size:11px;line-height:1.35}");
    html.push_str("</style></head><body><main>");
    html.push_str("<h1>EVM Compiler Bench</h1>");
    html.push_str(&format!(
        "<div class=\"meta\">EVM target: {}. solc {}. vyper {}.</div>",
        escape(&toolchains.evm_version),
        escape(&toolchains.solc.version),
        escape(&toolchains.vyper.version)
    ));
    html.push_str("<section class=\"grid\">");
    html.push_str(&card("Rows", rows.len()));
    html.push_str(&card("Benchmarks", by_benchmark.len()));
    html.push_str(&card("Profiles", by_profile.len()));
    html.push_str(&card("Compile Failures", compile_failures));
    html.push_str("</section>");

    html.push_str("<h2>Runtime Gas By Profile</h2><table><thead><tr><th>Profile</th><th>Average Execution Gas</th><th>Samples</th></tr></thead><tbody>");
    for (profile, (total, count)) in by_profile {
        html.push_str("<tr><td>");
        html.push_str(&escape(&profile));
        html.push_str("</td><td>");
        html.push_str(&(total / count as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(&count.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");

    html.push_str("<h2>Scale Studies</h2>");
    html.push_str(&render_scale_summary(rows));

    html.push_str("<h2>Real-Derived Benchmarks</h2>");
    html.push_str(&render_real_derived_summary(rows));

    html.push_str("<h2>Runtime Size vs Runtime Gas</h2><div class=\"chart\"><svg width=\"1080\" height=\"360\" viewBox=\"0 0 1080 360\" role=\"img\">");
    html.push_str("<line x1=\"45\" y1=\"315\" x2=\"1040\" y2=\"315\" stroke=\"#999\"/><line x1=\"45\" y1=\"20\" x2=\"45\" y2=\"315\" stroke=\"#999\"/>");
    let max_size = rows
        .iter()
        .map(|row| u64_at(row, "/bytecode/runtime_bytes"))
        .max()
        .unwrap_or(1);
    let max_gas = rows
        .iter()
        .map(|row| u64_at(row, "/gas/execution_gas"))
        .max()
        .unwrap_or(1);
    for row in rows {
        let size = u64_at(row, "/bytecode/runtime_bytes");
        let gas = u64_at(row, "/gas/execution_gas");
        let x = 45 + (size * 980 / max_size) as i32;
        let y = 315 - (gas * 285 / max_gas) as i32;
        html.push_str(&format!(
            "<circle cx=\"{x}\" cy=\"{y}\" r=\"3\"><title>{}</title></circle>",
            escape(&tooltip(row))
        ));
    }
    html.push_str("</svg></div>");

    html.push_str("<h2>Result Rows</h2><table><thead><tr><th>Status</th><th>Suite</th><th>Family</th><th>N</th><th>Benchmark</th><th>Implementation</th><th>Compiler</th><th>Profile</th><th>Metadata</th><th>State</th><th>Scenario</th><th>Differential</th><th>Randomized</th><th>Property</th><th>Failures</th><th>Runtime Bytes</th><th>Deploy Gas</th><th>Execution Gas</th><th>Calldata Gas</th><th>Total Tx Gas</th></tr></thead><tbody>");
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
            &str_at(row, "/correctness/differential_tests").unwrap_or_default(),
        ));
        html.push_str("</td><td>");
        html.push_str(&escape(
            &str_at(row, "/correctness/randomized_differential").unwrap_or_default(),
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
        html.push_str(&u64_at(row, "/gas/deploy_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/execution_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/calldata_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/total_tx_gas").to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table></main></body></html>");
    Ok(html)
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
    html.push_str("<table><thead><tr><th>Upstream</th><th>Benchmark</th><th>Model</th><th>Production Equivalent</th><th>Source</th><th>Port</th><th>Profile</th><th>Status</th><th>Scenario</th><th>Assumptions / Exclusions</th><th>Runtime Bytes</th><th>Deploy Gas</th><th>Runtime Gas</th><th>Total Tx Gas</th></tr></thead><tbody>");
    for row in selected {
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
        html.push_str(&u64_at(row, "/gas/deploy_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/execution_gas").to_string());
        html.push_str("</td><td>");
        html.push_str(&u64_at(row, "/gas/total_tx_gas").to_string());
        html.push_str("</td></tr>");
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
    runtime_bytes: u64,
    deploy_gas: u64,
    execution_gas: u64,
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
            profile_id,
            ..ScaleAggregate::default()
        });
        if str_at(row, "/status").as_deref() == Some("compile_error") {
            entry.compile_failures += 1;
        } else {
            entry.compile_wall_ms += f64_at(row, "/compile/wall_ms_samples/0");
            entry.runtime_bytes += u64_at(row, "/bytecode/runtime_bytes");
            entry.deploy_gas += u64_at(row, "/gas/deploy_gas");
            entry.execution_gas += u64_at(row, "/gas/execution_gas");
            entry.metric_samples += 1;
        }
    }

    if groups.is_empty() {
        return "<div class=\"card\">No generated scale rows in this report.</div>".to_string();
    }

    let mut html = String::new();
    html.push_str("<table><thead><tr><th>Family</th><th>N</th><th>Language</th><th>Profile</th><th>Avg Compile ms</th><th>Avg Runtime Bytes</th><th>Avg Deploy Gas</th><th>Avg Runtime Gas</th><th>Run Samples</th><th>Compile Failures</th></tr></thead><tbody>");
    for aggregate in groups.values() {
        let samples = aggregate.metric_samples.max(1) as f64;
        html.push_str("<tr><td>");
        html.push_str(&escape(&aggregate.family));
        html.push_str("</td><td>");
        html.push_str(&aggregate.parameter_value.to_string());
        html.push_str("</td><td>");
        html.push_str(&escape(&aggregate.language));
        html.push_str("</td><td>");
        html.push_str(&escape(&aggregate.profile_id));
        html.push_str("</td><td>");
        html.push_str(&format!("{:.2}", aggregate.compile_wall_ms / samples));
        html.push_str("</td><td>");
        html.push_str(&((aggregate.runtime_bytes as f64 / samples).round() as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(&((aggregate.deploy_gas as f64 / samples).round() as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(&((aggregate.execution_gas as f64 / samples).round() as u64).to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.metric_samples.to_string());
        html.push_str("</td><td>");
        html.push_str(&aggregate.compile_failures.to_string());
        html.push_str("</td></tr>");
    }
    html.push_str("</tbody></table>");
    html
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
        "{} / {} / {} / {} / gas {}",
        str_at(row, "/benchmark_id").unwrap_or_default(),
        str_at(row, "/implementation_id").unwrap_or_default(),
        str_at(row, "/profile_id").unwrap_or_default(),
        str_at(row, "/gas/scenario").unwrap_or_default(),
        u64_at(row, "/gas/execution_gas")
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

fn f64_at(row: &serde_json::Value, pointer: &str) -> f64 {
    row.pointer(pointer)
        .and_then(|value| value.as_f64())
        .unwrap_or(0.0)
}

fn joined_array_at(row: &serde_json::Value, pointer: &str) -> String {
    row.pointer(pointer)
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
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

fn card(label: &str, value: usize) -> String {
    format!(
        "<div class=\"card\"><div class=\"k\">{}</div><div class=\"v\">{}</div></div>",
        escape(label),
        value
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
