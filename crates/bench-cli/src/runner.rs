use crate::{
    catalog::scenarios,
    models::{CallSpec, CompileSet, CompiledArtifact, GasRecord, Scenario},
    util::{ensure_dir, require_success, run_measured},
};
use anyhow::{Context, Result, bail};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

const GAS_JSONL: &str = "../results/raw/foundry-gas.jsonl";
const SOL_BASELINE: &str = "solc-latest-legacy-runs200";
const VYPER_BASELINE: &str = "vyper-latest-gas";

pub fn run_foundry(
    root: &Path,
    evm_version: &str,
    compiled: &CompileSet,
) -> Result<Vec<GasRecord>> {
    ensure_dir(&root.join("results/raw"))?;
    let test_path = root.join("foundry/test/GeneratedBench.t.sol");
    fs::write(&test_path, generate_test(compiled)?)?;
    require_success(
        run_measured(
            Command::new("forge")
                .arg("test")
                .arg("--root")
                .arg(root.join("foundry"))
                .arg("--match-path")
                .arg("test/GeneratedBench.t.sol")
                .arg("--evm-version")
                .arg(evm_version)
                .arg("-q"),
            None,
        )?,
        "forge test",
    )?;
    let rows_path = root.join("results/raw/foundry-gas.jsonl");
    let rows = fs::read_to_string(&rows_path)
        .with_context(|| format!("reading {}", rows_path.display()))?;
    let mut records = Vec::new();
    for (index, line) in rows.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        records.push(
            serde_json::from_str(line)
                .with_context(|| format!("parsing gas jsonl line {}", index + 1))?,
        );
    }
    Ok(records)
}

fn generate_test(compiled: &CompileSet) -> Result<String> {
    if compiled.artifacts.is_empty() {
        bail!("no compiled artifacts for Foundry runner");
    }
    let mut out = String::new();
    out.push_str("// SPDX-License-Identifier: MIT\n");
    out.push_str("pragma solidity ^0.8.20;\n\n");
    out.push_str("interface Vm {\n");
    out.push_str("    function writeFile(string calldata path, string calldata data) external;\n");
    out.push_str("    function writeLine(string calldata path, string calldata data) external;\n");
    out.push_str("    function toString(uint256 value) external pure returns (string memory);\n");
    out.push_str("    function prank(address sender) external;\n");
    out.push_str("    function deal(address account, uint256 newBalance) external;\n");
    out.push_str("}\n\n");
    out.push_str("contract GeneratedBenchTest {\n");
    out.push_str(
        "    Vm constant vm = Vm(address(uint160(uint256(keccak256(\"hevm cheat code\")))));\n",
    );
    out.push_str("    address constant BOB = address(0xB0B);\n");
    out.push_str("    address constant CAROL = address(0xCAFe);\n");
    out.push_str("    address constant IMPLEMENTATION = address(0x1000000000000000000000000000000000000001);\n");
    out.push_str("    bytes32 constant SALT = keccak256(\"evm-compiler-bench\");\n");
    out.push_str("    bytes32 constant LEAF = keccak256(\"leaf\");\n");
    out.push_str("    bytes32 constant SIBLING = keccak256(\"sibling\");\n");
    out.push_str("    bytes32 constant ROOT = LEAF < SIBLING ? keccak256(abi.encodePacked(LEAF, SIBLING)) : keccak256(abi.encodePacked(SIBLING, LEAF));\n\n");
    out.push_str("    receive() external payable {}\n\n");
    out.push_str("    function setUp() public {\n");
    out.push_str("        vm.writeFile(\"");
    out.push_str(GAS_JSONL);
    out.push_str("\", \"\");\n");
    out.push_str("        vm.deal(address(this), 1000000 ether);\n");
    out.push_str("        vm.deal(BOB, 1000000 ether);\n");
    out.push_str("        vm.deal(CAROL, 1000000 ether);\n");
    out.push_str("    }\n\n");
    out.push_str(helper_functions());

    for (index, artifact) in compiled.artifacts.iter().enumerate() {
        write_deploy_function(&mut out, index, artifact);
    }

    for (index, artifact) in compiled.artifacts.iter().enumerate() {
        for scenario in scenarios(&artifact.benchmark_id) {
            write_gas_test(&mut out, index, artifact, &scenario);
        }
    }

    let baselines = baseline_pairs(&compiled.artifacts);
    for (benchmark_id, (solidity_idx, vyper_idx)) in baselines {
        for scenario in scenarios(&benchmark_id) {
            write_diff_test(
                &mut out,
                &benchmark_id,
                solidity_idx,
                vyper_idx,
                compiled
                    .artifacts
                    .get(solidity_idx)
                    .context("missing solidity baseline")?,
                compiled
                    .artifacts
                    .get(vyper_idx)
                    .context("missing vyper baseline")?,
                &scenario,
            );
        }
    }

    out.push_str("}\n");
    Ok(out)
}

fn helper_functions() -> &'static str {
    r#"
    function proofOne() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](1);
        proof[0] = SIBLING;
    }

    function proofEmpty() internal pure returns (bytes32[] memory proof) {
        proof = new bytes32[](0);
    }

    function _deploy(bytes memory code) internal returns (address target) {
        assembly {
            target := create(0, add(code, 0x20), mload(code))
        }
        require(target != address(0), "deploy failed");
    }

    function _run(address target, bytes memory data, uint256 value, address sender)
        internal
        returns (bool ok, bytes32 retHash, uint256 gasUsed)
    {
        bytes memory ret;
        uint256 startGas = gasleft();
        if (sender == address(this)) {
            (ok, ret) = target.call{value: value}(data);
        } else {
            vm.prank(sender);
            (ok, ret) = target.call{value: value}(data);
        }
        gasUsed = startGas - gasleft();
        retHash = keccak256(ret);
    }

    function _observe(address target, bytes memory data) internal returns (bytes32) {
        (bool ok, bytes memory ret) = target.call(data);
        return keccak256(abi.encode(ok, ret));
    }

    function _bool(bool value) internal pure returns (string memory) {
        return value ? "true" : "false";
    }

    function _writeRow(
        string memory benchmarkId,
        string memory implementationId,
        string memory profileId,
        string memory scenario,
        uint256 deployGas,
        uint256 executionGas,
        bool success
    ) internal {
        vm.writeLine(
            "../results/raw/foundry-gas.jsonl",
            string.concat(
                "{\"benchmark_id\":\"", benchmarkId,
                "\",\"implementation_id\":\"", implementationId,
                "\",\"profile_id\":\"", profileId,
                "\",\"scenario\":\"", scenario,
                "\",\"deploy_gas\":", vm.toString(deployGas),
                ",\"execution_gas\":", vm.toString(executionGas),
                ",\"success\":", _bool(success),
                "}"
            )
        );
    }

"#
}

fn write_deploy_function(out: &mut String, index: usize, artifact: &CompiledArtifact) {
    out.push_str("    function deployArtifact");
    out.push_str(&index.to_string());
    out.push_str("() internal returns (address target, uint256 deployGas) {\n");
    out.push_str("        bytes memory code = hex\"");
    out.push_str(artifact.creation_bytecode.trim_start_matches("0x"));
    out.push_str("\";\n");
    if let Some(args) = constructor_args(&artifact.benchmark_id) {
        out.push_str("        code = abi.encodePacked(code, ");
        out.push_str(args);
        out.push_str(");\n");
    }
    out.push_str("        uint256 startGas = gasleft();\n");
    out.push_str("        target = _deploy(code);\n");
    out.push_str("        deployGas = startGas - gasleft();\n");
    out.push_str("    }\n\n");
}

fn write_gas_test(
    out: &mut String,
    index: usize,
    artifact: &CompiledArtifact,
    scenario: &Scenario,
) {
    out.push_str("    function testGas_");
    out.push_str(&index.to_string());
    out.push('_');
    out.push_str(&sanitize(scenario.name));
    out.push_str("() public {\n");
    out.push_str("        (address target, uint256 deployGas) = deployArtifact");
    out.push_str(&index.to_string());
    out.push_str("();\n");
    write_setup(out, "target", &scenario.setup);
    out.push_str("        (bool ok,, uint256 executionGas) = _run(target, ");
    write_call_args(out, &scenario.measured);
    out.push_str(");\n");
    out.push_str("        require(ok == ");
    out.push_str(if scenario.expect_success {
        "true"
    } else {
        "false"
    });
    out.push_str(", \"unexpected scenario status\");\n");
    out.push_str("        _writeRow(\"");
    out.push_str(&artifact.benchmark_id);
    out.push_str("\", \"");
    out.push_str(&artifact.implementation_id);
    out.push_str("\", \"");
    out.push_str(&artifact.profile_id);
    out.push_str("\", \"");
    out.push_str(scenario.name);
    out.push_str("\", deployGas, executionGas, ok);\n");
    out.push_str("    }\n\n");
}

fn write_diff_test(
    out: &mut String,
    benchmark_id: &str,
    solidity_idx: usize,
    vyper_idx: usize,
    solidity: &CompiledArtifact,
    vyper: &CompiledArtifact,
    scenario: &Scenario,
) {
    out.push_str("    function testDiff_");
    out.push_str(&sanitize(benchmark_id));
    out.push('_');
    out.push_str(&sanitize(scenario.name));
    out.push_str("() public {\n");
    out.push_str("        (address solTarget,) = deployArtifact");
    out.push_str(&solidity_idx.to_string());
    out.push_str("();\n");
    out.push_str("        (address vyperTarget,) = deployArtifact");
    out.push_str(&vyper_idx.to_string());
    out.push_str("();\n");
    write_setup(out, "solTarget", &scenario.setup);
    write_setup(out, "vyperTarget", &scenario.setup);
    out.push_str("        (bool solOk, bytes32 solHash,) = _run(solTarget, ");
    write_call_args(out, &scenario.measured);
    out.push_str(");\n");
    out.push_str("        (bool vyperOk, bytes32 vyperHash,) = _run(vyperTarget, ");
    write_call_args(out, &scenario.measured);
    out.push_str(");\n");
    out.push_str("        require(solOk == vyperOk, \"differential status mismatch\");\n");
    out.push_str("        require(solOk == ");
    out.push_str(if scenario.expect_success {
        "true"
    } else {
        "false"
    });
    out.push_str(", \"differential unexpected status\");\n");
    out.push_str(
        "        if (solOk) require(solHash == vyperHash, \"differential return mismatch\");\n",
    );
    out.push_str("        require(_observeAll_");
    out.push_str(&sanitize(&solidity.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(scenario.name));
    out.push_str("(solTarget) == _observeAll_");
    out.push_str(&sanitize(&vyper.benchmark_id));
    out.push('_');
    out.push_str(&sanitize(scenario.name));
    out.push_str("(vyperTarget), \"differential observer mismatch\");\n");
    out.push_str("    }\n\n");
    write_observer_function(out, &solidity.benchmark_id, scenario);
}

fn write_observer_function(out: &mut String, benchmark_id: &str, scenario: &Scenario) {
    let name = format!(
        "_observeAll_{}_{}",
        sanitize(benchmark_id),
        sanitize(scenario.name)
    );
    if out.contains(&format!("function {name}(")) {
        return;
    }
    out.push_str("    function ");
    out.push_str(&name);
    out.push_str("(address target) internal returns (bytes32 observed) {\n");
    out.push_str("        observed = bytes32(0);\n");
    for observer in &scenario.observers {
        out.push_str("        observed = keccak256(abi.encode(observed, _observe(target, ");
        out.push_str(observer.data);
        out.push_str(")));\n");
    }
    out.push_str("    }\n\n");
}

fn write_setup(out: &mut String, target: &str, setup: &[CallSpec]) {
    for call in setup {
        out.push_str("        _run(");
        out.push_str(target);
        out.push_str(", ");
        write_call_args(out, call);
        out.push_str(");\n");
    }
}

fn write_call_args(out: &mut String, call: &CallSpec) {
    out.push_str(call.data);
    out.push_str(", ");
    out.push_str(call.value);
    out.push_str(", ");
    out.push_str(call.sender.unwrap_or("address(this)"));
}

fn baseline_pairs(artifacts: &[CompiledArtifact]) -> BTreeMap<String, (usize, usize)> {
    let mut pairs = BTreeMap::new();
    for (index, artifact) in artifacts.iter().enumerate() {
        if artifact.profile_id != SOL_BASELINE {
            continue;
        }
        if let Some((vyper_idx, _)) = artifacts.iter().enumerate().find(|(_, other)| {
            other.benchmark_id == artifact.benchmark_id && other.profile_id == VYPER_BASELINE
        }) {
            pairs.insert(artifact.benchmark_id.clone(), (index, vyper_idx));
        }
    }
    pairs
}

fn constructor_args(benchmark_id: &str) -> Option<&'static str> {
    match benchmark_id {
        "counter" => Some("abi.encode(uint256(3))"),
        "erc20_minimal" => Some("abi.encode(uint256(1000 ether))"),
        _ => None,
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}
