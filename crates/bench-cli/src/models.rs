use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Solidity,
    Vyper,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solidity => "solidity",
            Self::Vyper => "vyper",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Benchmark {
    pub id: &'static str,
    pub contract_name: &'static str,
    pub constructor_args: &'static str,
    pub solidity_path: &'static str,
    pub vyper_path: &'static str,
}

#[derive(Debug, Clone)]
pub struct CallSpec {
    pub sender: Option<&'static str>,
    pub value: &'static str,
    pub data: &'static str,
}

impl CallSpec {
    pub const fn new(data: &'static str) -> Self {
        Self {
            sender: None,
            value: "0",
            data,
        }
    }

    pub const fn sender(data: &'static str, sender: &'static str) -> Self {
        Self {
            sender: Some(sender),
            value: "0",
            data,
        }
    }

    pub const fn value(data: &'static str, value: &'static str) -> Self {
        Self {
            sender: None,
            value,
            data,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Scenario {
    pub name: &'static str,
    pub setup: Vec<CallSpec>,
    pub measured: CallSpec,
    pub expect_success: bool,
    pub observers: Vec<CallSpec>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompilerProfile {
    pub id: String,
    pub language: Language,
    pub compiler: String,
    #[serde(default)]
    pub optimizer: bool,
    #[serde(default)]
    pub optimizer_runs: u32,
    #[serde(default)]
    pub optimizer_mode: Option<String>,
    #[serde(default)]
    pub via_ir: bool,
    pub evm_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Toolchain {
    pub name: String,
    pub version: String,
    pub binary_path: PathBuf,
    pub binary_sha256: String,
    pub download_source: String,
    pub version_output: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Toolchains {
    pub solc: Toolchain,
    pub vyper: Toolchain,
    pub evm_version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommandStats {
    pub wall_ms: f64,
    pub cpu_ms: f64,
    pub peak_rss_kib: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompileMetrics {
    pub wall_ms_samples: Vec<f64>,
    pub cpu_ms_samples: Vec<f64>,
    pub peak_rss_kib: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BytecodeMetrics {
    pub creation_bytes: usize,
    pub creation_bytes_stripped: usize,
    pub runtime_bytes: usize,
    pub runtime_bytes_stripped: usize,
    pub initcode_bytes: usize,
    pub linked_runtime_bytes: usize,
    pub eip170_margin_bytes: isize,
    pub eip3860_margin_bytes: isize,
    pub code_deposit_gas: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompiledArtifact {
    pub benchmark_id: String,
    pub implementation_id: String,
    pub language: Language,
    pub contract_name: String,
    pub profile_id: String,
    pub compiler: Toolchain,
    pub compiler_settings: serde_json::Value,
    pub source_path: PathBuf,
    pub source_hash: String,
    pub abi: serde_json::Value,
    pub creation_bytecode: String,
    pub runtime_bytecode: String,
    pub compile: CompileMetrics,
    pub bytecode: BytecodeMetrics,
}

#[derive(Debug, Clone)]
pub struct CompileSet {
    pub profiles: Vec<CompilerProfile>,
    pub artifacts: Vec<CompiledArtifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GasRecord {
    pub benchmark_id: String,
    pub implementation_id: String,
    pub profile_id: String,
    pub scenario: String,
    pub deploy_gas: u64,
    pub execution_gas: u64,
    pub success: bool,
}
