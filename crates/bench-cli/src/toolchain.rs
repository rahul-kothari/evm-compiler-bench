use crate::{
    models::{Toolchain, Toolchains},
    util::{ensure_dir, require_success, run_measured, sha256_bytes, sha256_file},
};
use anyhow::{Context, Result, anyhow, bail};
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const SOLC_INDEX_ROOT: &str = "https://binaries.soliditylang.org";
const PYPI_VYPER_JSON: &str = "https://pypi.org/pypi/vyper/json";
const EVM_ORDER: &[&str] = &["osaka", "prague", "cancun", "shanghai", "paris", "london"];

pub fn resolve_toolchains(root: &Path, offline: bool) -> Result<Toolchains> {
    let solc = resolve_solc(root, offline)?;
    let vyper = resolve_vyper(root, offline)?;
    let evm_version = latest_shared_evm(&solc, &vyper)?;
    Ok(Toolchains {
        solc,
        vyper,
        evm_version,
    })
}

fn resolve_solc(root: &Path, offline: bool) -> Result<Toolchain> {
    if offline {
        return resolve_path_toolchain("solc", env::var_os("EVM_BENCH_SOLC").map(PathBuf::from));
    }
    let index = fetch_solc_index()?;
    let release_path = index
        .releases
        .get(&index.latest_release)
        .with_context(|| format!("solc latest release {} missing from index", index.latest_release))?;
    let build = index
        .builds
        .iter()
        .find(|build| build.path == *release_path)
        .with_context(|| format!("solc build {release_path} missing from index"))?;
    let platform = solc_platform()?;
    let target_dir = root.join(".cache/toolchains/solc").join(&index.latest_release);
    ensure_dir(&target_dir)?;
    let target = target_dir.join(&build.path);
    let source = format!("{SOLC_INDEX_ROOT}/{platform}/{}", build.path);
    if !target.exists() {
        let bytes = reqwest::blocking::get(&source)
            .with_context(|| format!("downloading {source}"))?
            .error_for_status()?
            .bytes()?;
        let actual = sha256_bytes(&bytes);
        let expected = build.sha256.trim_start_matches("0x");
        if actual != expected {
            bail!("solc checksum mismatch for {source}: expected {expected}, got {actual}");
        }
        fs::write(&target, bytes)?;
        make_executable(&target)?;
    }
    let version_output = command_stdout(Command::new(&target).arg("--version"))?;
    Ok(Toolchain {
        name: "solc".to_string(),
        version: parse_solc_version(&version_output)?,
        binary_sha256: sha256_file(&target)?,
        binary_path: target,
        download_source: source,
        version_output,
    })
}

fn resolve_vyper(root: &Path, offline: bool) -> Result<Toolchain> {
    if offline {
        return resolve_path_toolchain("vyper", env::var_os("EVM_BENCH_VYPER").map(PathBuf::from));
    }
    let latest = fetch_vyper_latest()?;
    if let Some(local) = local_toolchain_if_version("vyper", env::var_os("EVM_BENCH_VYPER").map(PathBuf::from), &latest)? {
        return Ok(local);
    }

    let venv = root.join(".cache/toolchains/vyper").join(&latest);
    let binary = venv.join(bin_dir()).join(binary_name("vyper"));
    if !binary.exists() {
        ensure_dir(&venv)?;
        require_success(
            run_measured(Command::new("uv").arg("venv").arg(&venv), None)?,
            "uv venv",
        )?;
        let python = venv.join(bin_dir()).join(binary_name("python"));
        require_success(
            run_measured(
                Command::new("uv")
                    .arg("pip")
                    .arg("install")
                    .arg("--python")
                    .arg(&python)
                    .arg(format!("vyper=={latest}")),
                None,
            )?,
            "uv pip install vyper",
        )?;
    }
    let version_output = command_stdout(Command::new(&binary).arg("--version"))?;
    Ok(Toolchain {
        name: "vyper".to_string(),
        version: parse_vyper_version(&version_output)?,
        binary_sha256: sha256_file(&binary)?,
        binary_path: binary,
        download_source: format!("https://pypi.org/project/vyper/{latest}/"),
        version_output,
    })
}

fn resolve_path_toolchain(name: &str, env_path: Option<PathBuf>) -> Result<Toolchain> {
    let binary_path = match env_path {
        Some(path) => path,
        None => which::which(name).with_context(|| format!("{name} not found on PATH"))?,
    };
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let version = match name {
        "solc" => parse_solc_version(&version_output)?,
        "vyper" => parse_vyper_version(&version_output)?,
        _ => return Err(anyhow!("unknown toolchain {name}")),
    };
    Ok(Toolchain {
        name: name.to_string(),
        version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source: "local".to_string(),
        version_output,
    })
}

fn local_toolchain_if_version(name: &str, env_path: Option<PathBuf>, latest: &str) -> Result<Option<Toolchain>> {
    let binary_path = match env_path {
        Some(path) => path,
        None => match which::which(name) {
            Ok(path) => path,
            Err(_) => return Ok(None),
        },
    };
    let version_output = command_stdout(Command::new(&binary_path).arg("--version"))?;
    let version = match name {
        "vyper" => parse_vyper_version(&version_output)?,
        "solc" => parse_solc_version(&version_output)?,
        _ => return Err(anyhow!("unknown toolchain {name}")),
    };
    if version != latest {
        return Ok(None);
    }
    Ok(Some(Toolchain {
        name: name.to_string(),
        version,
        binary_sha256: sha256_file(&binary_path)?,
        binary_path,
        download_source: "local".to_string(),
        version_output,
    }))
}

fn latest_shared_evm(solc: &Toolchain, vyper: &Toolchain) -> Result<String> {
    let solc_help = command_stdout(Command::new(&solc.binary_path).arg("--help"))?;
    let vyper_help = command_stdout(Command::new(&vyper.binary_path).arg("--help"))?;
    for evm in EVM_ORDER {
        if solc_help.contains(evm) && vyper_help.contains(evm) {
            return Ok((*evm).to_string());
        }
    }
    bail!("could not find shared EVM target between solc and vyper");
}

fn fetch_solc_index() -> Result<SolcIndex> {
    let platform = solc_platform()?;
    let url = format!("{SOLC_INDEX_ROOT}/{platform}/list.json");
    Ok(reqwest::blocking::get(&url)
        .with_context(|| format!("fetching {url}"))?
        .error_for_status()?
        .json()?)
}

fn fetch_vyper_latest() -> Result<String> {
    let payload: PypiPackage = reqwest::blocking::get(PYPI_VYPER_JSON)
        .with_context(|| format!("fetching {PYPI_VYPER_JSON}"))?
        .error_for_status()?
        .json()?;
    Ok(payload.info.version)
}

fn command_stdout(command: &mut Command) -> Result<String> {
    let output = require_success(run_measured(command, None)?, "command")?.output;
    Ok(String::from_utf8(output.stdout)?)
}

fn parse_solc_version(output: &str) -> Result<String> {
    parse_version(output, r"Version:\s*([0-9]+\.[0-9]+\.[0-9]+)")
}

fn parse_vyper_version(output: &str) -> Result<String> {
    parse_version(output, r"([0-9]+\.[0-9]+\.[0-9]+)")
}

fn parse_version(output: &str, pattern: &str) -> Result<String> {
    let re = Regex::new(pattern)?;
    let captures = re
        .captures(output)
        .with_context(|| format!("could not parse version from {output:?}"))?;
    Ok(captures[1].to_string())
}

fn solc_platform() -> Result<&'static str> {
    match env::consts::OS {
        "macos" => Ok("macosx-amd64"),
        "linux" => Ok("linux-amd64"),
        "windows" => Ok("windows-amd64"),
        other => bail!("unsupported solc platform {other}"),
    }
}

fn bin_dir() -> &'static str {
    if cfg!(windows) { "Scripts" } else { "bin" }
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SolcIndex {
    latest_release: String,
    releases: BTreeMap<String, String>,
    builds: Vec<SolcBuild>,
}

#[derive(Debug, Deserialize)]
struct SolcBuild {
    path: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
struct PypiPackage {
    info: PypiInfo,
}

#[derive(Debug, Deserialize)]
struct PypiInfo {
    version: String,
}

#[cfg(test)]
mod tests {
    use super::{parse_solc_version, parse_vyper_version};

    #[test]
    fn parses_solc_version() {
        assert_eq!(
            parse_solc_version("Version: 0.8.35+commit.whatever.Darwin.appleclang").unwrap(),
            "0.8.35"
        );
    }

    #[test]
    fn parses_vyper_version() {
        assert_eq!(parse_vyper_version("0.4.3+commit.bff19ea2").unwrap(), "0.4.3");
    }
}
