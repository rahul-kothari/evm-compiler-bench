use crate::{catalog::benchmarks, scenarios::load_scenario_catalog};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy)]
pub struct ValidationSummary {
    pub specs: usize,
    pub scenario_files: usize,
    pub result_rows: usize,
}

pub fn validate_all(root: &Path) -> Result<ValidationSummary> {
    let specs = validate_specs(root)?;
    let scenario_files = validate_scenarios(root)?;
    let result_rows = validate_outputs_if_present(root)?;
    Ok(ValidationSummary {
        specs,
        scenario_files,
        result_rows,
    })
}

fn validate_specs(root: &Path) -> Result<usize> {
    let mut count = 0;
    let bench_ids: BTreeSet<_> = benchmarks()
        .into_iter()
        .map(|bench| bench.id.to_string())
        .collect();
    for path in yaml_files(&root.join("benches/specs"))? {
        let text = fs::read_to_string(&path)?;
        let value: serde_yaml::Value =
            serde_yaml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let id = value
            .get("id")
            .and_then(|value| value.as_str())
            .with_context(|| format!("{} missing id", path.display()))?;
        if !bench_ids.contains(id) {
            bail!("{} id {id} is not in the benchmark catalog", path.display());
        }
        require_sequence(&value, "abi", &path)?;
        require_sequence(&value, "scenarios", &path)?;
        let implementations = value
            .get("implementations")
            .with_context(|| format!("{} missing implementations", path.display()))?;
        for language in ["solidity", "vyper"] {
            let implementation = implementations
                .get(language)
                .and_then(|value| value.as_str())
                .with_context(|| format!("{} missing {language} implementation", path.display()))?;
            let implementation_path = root.join(implementation);
            if !implementation_path.exists() {
                bail!(
                    "{} references missing {language} implementation {}",
                    path.display(),
                    implementation_path.display()
                );
            }
        }
        count += 1;
    }
    if count != bench_ids.len() {
        bail!(
            "expected {} benchmark specs, found {count}",
            bench_ids.len()
        );
    }
    Ok(count)
}

fn validate_scenarios(root: &Path) -> Result<usize> {
    let catalog = load_scenario_catalog(root, None)?;
    let bench_ids: BTreeSet<_> = benchmarks()
        .into_iter()
        .map(|bench| bench.id.to_string())
        .collect();
    let mut count = 0;
    for file in catalog.iter() {
        if !bench_ids.contains(&file.benchmark_id) {
            bail!(
                "scenario file references unknown benchmark {}",
                file.benchmark_id
            );
        }
        count += 1;
    }
    if count != bench_ids.len() {
        bail!("expected {} scenario files, found {count}", bench_ids.len());
    }
    Ok(count)
}

fn validate_outputs_if_present(root: &Path) -> Result<usize> {
    let results_path = root.join("results/normalized/results.json");
    let manifest_path = root.join("results/normalized/run-manifest.json");
    let mut rows = 0;
    if results_path.exists() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&results_path)?)
            .with_context(|| format!("parsing {}", results_path.display()))?;
        let array = value
            .as_array()
            .with_context(|| format!("{} must be an array", results_path.display()))?;
        for row in array {
            require_json_pointer(row, "/benchmark_id", &results_path)?;
            require_json_pointer(row, "/implementation_id", &results_path)?;
            require_json_pointer(row, "/compiler/name", &results_path)?;
            require_json_pointer(row, "/compiler/version", &results_path)?;
            require_json_pointer(row, "/compiler/settings", &results_path)?;
            require_json_pointer(row, "/bytecode/runtime_bytes", &results_path)?;
            require_json_pointer(row, "/gas/scenario", &results_path)?;
            require_json_pointer(row, "/gas/state_access_profile", &results_path)?;
            require_json_pointer(row, "/gas/metadata_mode", &results_path)?;
            require_json_pointer(row, "/correctness/golden_tests", &results_path)?;
            rows += 1;
        }
    }
    if manifest_path.exists() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&manifest_path)?)
            .with_context(|| format!("parsing {}", manifest_path.display()))?;
        for pointer in [
            "/run_id",
            "/started_at",
            "/evm_version",
            "/toolchains",
            "/profiles",
        ] {
            require_json_pointer(&value, pointer, &manifest_path)?;
        }
    }
    Ok(rows)
}

fn require_sequence(value: &serde_yaml::Value, key: &str, path: &Path) -> Result<()> {
    if !value
        .get(key)
        .and_then(|value| value.as_sequence())
        .is_some_and(|items| !items.is_empty())
    {
        bail!("{} missing non-empty {key}", path.display());
    }
    Ok(())
}

fn require_json_pointer(value: &Value, pointer: &str, path: &Path) -> Result<()> {
    if value.pointer(pointer).is_none() {
        bail!("{} missing JSON pointer {pointer}", path.display());
    }
    Ok(())
}

fn yaml_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if matches!(
            path.extension().and_then(|ext| ext.to_str()),
            Some("yaml" | "yml")
        ) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::validate_all;
    use std::path::Path;

    #[test]
    fn validates_checked_in_inputs() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap();
        let summary = validate_all(root).unwrap();
        assert_eq!(summary.specs, 10);
        assert_eq!(summary.scenario_files, 10);
    }
}
