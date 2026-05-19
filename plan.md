## Recommended direction

Build **EVM Compiler Bench** as two benchmarks, not one:

1. **Equivalent-contract benchmark**: small-to-medium contracts generated from a shared behavioral spec and implemented in Solidity, Vyper, Yul, maybe Huff/Fe. This is where you compare compilation time, bytecode size, deployment gas, and runtime gas fairly.
2. **Native-large-project benchmark**: real projects such as Uniswap-scale Solidity codebases, plus Vyper-native projects where available. This is mainly for **compiler throughput, cache behavior, optimizer performance, and artifact size**, not cross-language “equivalence.”

That split matters because Ethereum.org currently treats **Solidity and Vyper** as the two most active and maintained smart-contract languages, while Yul is lower-level/intermediate and Fe is still emerging; Huff is explicitly low-level and optimized around direct EVM control. A benchmark that claims “Solidity vs Vyper vs Huff on Uniswap” would be misleading unless the benchmark clearly separates generated equivalents from native ecosystems. ([ethereum.org][1])

## Locked scope for first implementation

The first implementation is the equivalent-contract benchmark only. Native-large-project, Yul, Huff, Fe, historical compiler-version, multi-fork, and experimental-codegen work stay out of the MVP.

Implement the ten-contract starter set:

1. `counter`
2. `erc20_minimal`
3. `erc20_permit_hashing`
4. `ownable_pausable`
5. `vault_deposit_withdraw`
6. `factory_create2`
7. `minimal_proxy`
8. `merkle_verifier`
9. `amm_pair_subset`
10. `scaling_dispatch_N`

Use only the latest stable Solidity and latest stable Vyper releases. The harness must be able to discover, download, checksum, cache, and reuse those compilers if the expected binaries are not already available locally. Every result must record the resolved compiler version, binary path, binary SHA-256, download source, and compiler settings.

Use one EVM target: the latest EVM version supported by both selected compilers. As of the current Solidity/Vyper combination this is Prague, because Solidity has moved on to Osaka while Vyper 0.4.3 documents Prague as its latest/default supported EVM target. This should be a capability check in the harness, not a hard-coded forever assumption. ([Solidity 0.8.35][17], [Vyper release notes][18])

Use a Foundry-based scenario runner for the MVP, orchestrated by a Rust CLI. A `revm` execution path is acceptable for deterministic execution or cross-checking, but the MVP should not block on replacing Foundry. Emit both machine-readable results and a static HTML report in the first deliverable.

For each benchmark, find a credible native implementation in one language first, then hand-write the opposing-language version against the same behavioral spec. Do not auto-translate code directly. Keep source provenance in the spec or implementation metadata so later readers can tell what was adapted and what was handwritten.

## What to benchmark

Use four primary metric families.

| Area               |                                                                                                              Metrics | Notes                                                                                                                                                                                                                                                |
| ------------------ | -------------------------------------------------------------------------------------------------------------------: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Compilation**    |                                        wall-clock time, CPU time, peak RSS, output artifact count/size, failure rate | Separate compiler-only runs from framework builds.                                                                                                                                                                                                   |
| **Bytecode**       | creation bytecode bytes, runtime bytecode bytes, metadata-stripped bytes, initcode bytes, deployed-code limit margin | Track whether contracts approach EIP-170’s 24,576-byte deployed-code limit and EIP-3860’s 49,152-byte initcode limit. ([Ethereum Improvement Proposals][2])                                                                                          |
| **Deployment gas** |                                     constructor execution gas, initcode cost, code-deposit cost, total create tx gas | Report with and without metadata where possible; metadata affects deploy size and deploy gas.                                                                                                                                                        |
| **Runtime gas**    |            per-scenario execution gas, tx gas including calldata, cold/warm variants, revert-path gas, log/event gas | Pin the EVM fork because gas rules and compiler decisions are fork-sensitive. Solidity and Vyper both expose explicit EVM-version settings and warn that compiling for the wrong EVM version can produce bad behavior. ([Solidity Documentation][3]) |

Do **not** collapse these into one “winner” score. Publish per-benchmark results, group summaries, and Pareto plots: compile time vs runtime gas, runtime bytecode size vs runtime gas, deploy gas vs runtime gas.

## Compiler matrix

Start with a focused matrix and expand later. The locked MVP intentionally uses latest stable `solc`, latest stable `vyper`, and the latest EVM target both can compile for. Historical compiler versions and historical forks are later trend work, not initial scope.

| Tier                               | Include                                                                                | Why                                                                                                                                                                                                                                          |
| ---------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Core**                           | `solc` legacy codegen, `solc --via-ir`, Vyper normal pipeline                          | These are the important production-relevant comparisons. Solidity’s IR pipeline is a distinct bytecode-generation path via Yul and can enable stronger cross-function optimization. ([Solidity Documentation][4])                            |
| **Optimizer profiles**             | Solidity optimizer off, Solidity optimizer runs=200, Solidity viaIR runs=200; Vyper `none`, `codesize`, `gas` | Solidity optimizer runs trade deployment cost against expected runtime calls; Vyper exposes `none`, `codesize`, and `gas`, with different selector-table and loop/code-size behavior. ([Solidity Documentation][5])                          |
| **Fork profiles**                  | Latest mutually supported EVM target only                                              | Defaults change, and the two compilers can support different newest targets at the same time. The harness should select the newest shared target by compiler capability and record it explicitly. ([Solidity Programming Language][6])        |
| **Experimental**                   | Vyper Venom, Solidity EOF/SSA-CFG where available, Fe                                  | Keep separate from the headline benchmark because experimental features and non-production-ready languages can distort conclusions. Vyper documents Venom as experimental codegen, and Fe 26.x says it is not production-ready. ([Vyper][7]) |
| **Low-level reference**            | Yul, Huff                                                                              | Useful as “how low can bytecode/gas go?” baselines, not fair high-level-language comparisons. Yul is Solidity’s intermediate language and can be used stand-alone; Huff exposes the EVM stack directly. ([Solidity Documentation][8])        |
| **Exclude from EVM bytecode core** | Solang                                                                                 | Solang is interesting, but its docs state it generates WebAssembly or Solana SBF rather than EVM bytecode, so it belongs outside an EVM-bytecode benchmark unless a specific EVM target is validated. ([Solang][9])                          |

## Contract corpus

Use four layers.

### 1. Microbenchmarks

These isolate compiler/codegen behavior:

| Category       | Examples                                                                         |
| -------------- | -------------------------------------------------------------------------------- |
| Dispatch       | 1, 4, 16, 64 external functions; overloaded-looking ABI shapes; fallback/receive |
| Arithmetic     | checked vs unchecked arithmetic, `mulDiv`, exponentiation, fixed-point math      |
| ABI/calldata   | static args, dynamic arrays, strings/bytes, nested structs where supported       |
| Memory         | copying, hashing, ABI encode/decode, return large bytes                          |
| Storage        | single slot, packed slots, mappings, nested mappings, dynamic arrays             |
| Control flow   | loops with static bounds, loops with calldata bounds, branches, modifiers        |
| External calls | ERC20 transfer, delegatecall proxy path, staticcall, call with return data       |
| Errors/events  | revert strings, custom errors where supported, indexed/non-indexed events        |

Generated scaling families are especially valuable: `N` functions, `N` storage fields, `N` loop iterations, `N` ABI arguments, `N` inherited modules. This exposes compile-time scaling and optimizer cliffs.

### 2. Standard-contract suite

Use small, recognizable contracts with precise behavioral specs:

* Counter, Ownable, Pausable
* ERC20 minimal, ERC20 with permit-like hashing, ERC721-lite
* Vault with deposits/withdrawals
* Factory with CREATE2
* Minimal proxy / upgradeable proxy
* Multisig-lite
* AMM pair subset: mint, burn, swap, skim/sync, but not full Uniswap scale
* Oracle accumulator / TWAP-like rolling state
* Merkle proof verifier
* Reentrancy guard patterns

### 3. Realistic DeFi fragments

Use extracted components, not whole projects: pair math, router path loop, transfer-helper patterns, vault accounting, permit/EIP-712 hashing, concentrated-liquidity math fragments. These are feasible to translate and validate.

### 4. Native-large-project suite

Use full native codebases for stress testing:

* Solidity-native projects: Uniswap-like, OpenZeppelin-heavy, governance, account abstraction, proxy-heavy repos.
* Vyper-native projects: Curve-like or other Vyper repositories where licensing and build reproducibility are clean.
* This suite reports compile time, memory, artifact size, and bytecode sizes **within that ecosystem only**.

## How to get equivalent contracts with Codex safely

Do not ask Codex to “translate Solidity to Vyper” directly as the main workflow. That biases the Vyper code toward Solidity structure and can create bad idioms. For the MVP, start from credible implementations and produce equivalent counterparts deliberately:

1. Write a **language-neutral spec** per benchmark.
2. Find a credible native implementation in one language: canonical examples, audited library fragments, widely used minimal implementations, or official docs examples with compatible licenses.
3. Normalize that implementation to the benchmark spec without preserving irrelevant framework, inheritance, or dependency structure.
4. Hand-write the opposing-language implementation against the same spec, using idioms native to that language.
5. Compile.
6. Run golden and differential tests.
7. Keep only implementations that pass.

A benchmark spec should include:

```yaml
id: erc20_minimal
abi:
  - "transfer(address,uint256) returns (bool)"
  - "balanceOf(address) returns (uint256)"
  - "totalSupply() returns (uint256)"
state_model:
  balances: "mapping(address => uint256)"
  total_supply: "uint256"
semantics:
  transfer:
    pre:
      - "balance[msg.sender] >= amount"
    post:
      - "balance[msg.sender] decreases by amount"
      - "balance[to] increases by amount"
      - "totalSupply unchanged"
    logs:
      - "Transfer(msg.sender, to, amount)"
scenarios:
  - name: transfer_warm_success
  - name: transfer_cold_success
  - name: transfer_revert_insufficient_balance
invariants:
  - "sum(sampled_balances) <= totalSupply"
equivalence:
  compare:
    - success_or_revert
    - returndata
    - logs
    - sampled_storage_observables
```

For most benchmarks, compare **external behavior**, not raw storage layout. Storage layout should become part of the spec only for proxy/layout benchmarks.

## Correctness strategy

A robust gas benchmark is useless if implementations differ. Use three correctness layers:

1. **Golden scenario tests**: deterministic call sequences with exact expected return values, logs, and state. This is required in the MVP before a result row is accepted.
2. **Differential tests**: run the same deterministic and randomized call sequences against every implementation and compare observable outputs. Deterministic differential checks belong in the MVP; randomized/property fuzzing can harden later.
3. **Property tests**: invariants such as total supply conservation, no negative balances, authorization rules, idempotent views, revert conditions.

Store failing fuzz seeds and minimized counterexamples as regression fixtures. For revert equivalence, use two modes: **semantic revert** means “both revert,” while **ABI-exact revert** means “same encoded custom error/string.” The first is better for cross-language comparison; the second is useful for stricter ABI compatibility.

## Gas measurement design

Use a Foundry-based runner as the MVP source of truth, with the exact scenario pre-state, calldata, sender, value, fork target, and warm/cold assumptions generated by the Rust harness.

Foundry is useful because it can emit gas reports, deployment size, deployment cost, per-function min/avg/median/max, gas snapshots, and section snapshots; it also has isolated test mode, which improves gas accounting by executing top-level calls in separate EVM contexts. ([foundry - Ethereum Development Framework][10])

A `revm` runner remains acceptable for deterministic execution, cross-checking Foundry gas, or eventually replacing Foundry as the canonical execution path. `revm` is a Rust EVM implementation used broadly in Ethereum tooling and exposes an execution API suitable for deterministic transaction execution. ([GitHub][11])

Each scenario should report:

```text
execution_gas
tx_intrinsic_gas
calldata_gas
calldata_floor_gas_if_applicable
total_tx_gas
state_access_profile: cold | warm | mixed
evm_fork: latest-shared:<resolved-name>
```

Include both **execution gas** and **transaction gas**. EIP-7623 changes calldata pricing for data-heavy transactions, so calldata-heavy ABI benchmarks can look different if you only report raw EVM execution gas. ([Ethereum Improvement Proposals][12])

## Bytecode measurement rules

Measure at least six bytecode sizes:

1. Creation bytecode, metadata included
2. Creation bytecode, metadata stripped or disabled
3. Runtime bytecode, metadata included
4. Runtime bytecode, metadata stripped or disabled
5. Initcode length
6. Linked runtime bytecode length

This is important because Solidity appends metadata by default and exposes `metadata.bytecodeHash` / `appendCBOR` settings, while Vyper has `bytecodeMetadata` settings. Metadata is valuable for verification but can skew bytecode-size and deployment-gas comparisons. ([Solidity Documentation][13])

Also compute:

```text
eip170_margin_bytes = 24576 - runtime_code_bytes
eip3860_margin_bytes = 49152 - initcode_bytes
code_deposit_gas = 200 * runtime_code_bytes
```

EIP-3860 explicitly notes the deployed-code deposit cost of 200 gas per byte and adds initcode metering, so deployment gas should be decomposed rather than shown only as one opaque number. ([Ethereum Improvement Proposals][14])

## Compilation-time measurement rules

Separate these modes:

| Mode                            | What it measures                                           |
| ------------------------------- | ---------------------------------------------------------- |
| **Compiler-only cold**          | Start compiler process, no framework cache, clean temp dir |
| **Compiler-only warm**          | Same compiler invocation after warmup                      |
| **Framework clean build**       | Foundry/Vyper project build after cache deletion           |
| **Framework incremental build** | One-file change or no-op rebuild                           |
| **Matrix build**                | Multiple optimizer/fork/compiler versions                  |

Use `solc --standard-json` for automated Solidity compilation because Solidity documents it as the recommended interface for complex automated use. For Vyper, use `vyper-json` or Vyper archives for reproducible inputs/settings. ([Solidity Documentation][3])

The harness should resolve compilers through a toolchain manager:

* Prefer configured local paths when they match the expected latest stable version and checksum.
* Download missing Solidity binaries from the official binary index and missing Vyper releases from the configured official release source.
* Cache downloads outside result directories, then record the resolved binary path, version output, SHA-256, and source URL in every run manifest.
* Fail closed if the latest release cannot be discovered or a checksum does not match.

Use a CLI benchmark tool such as `hyperfine` for first-pass timing because it supports warmups, repeated runs, cache-clearing commands, outlier detection, and JSON/CSV/Markdown export. For the serious benchmark harness, store raw timing samples and environment metadata yourself. ([GitHub][15])

Control for:

* CPU model, core count, governor, turbo/boost setting
* OS/kernel/container image
* compiler binary hash
* exact compiler version
* EVM fork target
* optimizer settings
* import/dependency graph hash
* cache state
* output selection
* wall time, CPU time, peak RSS

Solidity’s docs warn that requesting all outputs can slow compilation unnecessarily, so define a minimal output profile for timing and a separate full-artifact profile for artifact-size/reporting tests. ([Solidity Documentation][3])

## Repository structure

```text
evm-compiler-bench/
  Cargo.toml
  benches/
    specs/
      counter.yaml
      erc20_minimal.yaml
      erc20_permit_hashing.yaml
      ownable_pausable.yaml
      vault.yaml
      factory_create2.yaml
      minimal_proxy.yaml
      merkle_verifier.yaml
      amm_pair_subset.yaml
      scaling_dispatch_n.yaml
    implementations/
      erc20_minimal/
        solidity/
        vyper/
    scenarios/
      erc20_minimal.scenarios.json
  compiler-profiles/
    solc-latest-legacy-runs200-latest-shared-evm.toml
    solc-latest-viair-runs200-latest-shared-evm.toml
    solc-latest-noopt-latest-shared-evm.toml
    vyper-latest-none-latest-shared-evm.toml
    vyper-latest-gas-latest-shared-evm.toml
    vyper-latest-codesize-latest-shared-evm.toml
  foundry/
    foundry.toml
    src/
    test/
  crates/
    bench-cli/
    compiler-runner/
    foundry-runner/
    revm-runner/
    bytecode-analyzer/
    report-generator/
    toolchain-manager/
  results/
    raw/
    normalized/
    reports/
      index.html
  schemas/
    benchmark_spec.schema.json
    result.schema.json
    run_manifest.schema.json
```

## Result schema

Every result row should be joinable by benchmark, implementation, compiler, and scenario:

```json
{
  "benchmark_id": "erc20_minimal",
  "implementation_id": "vyper/handwritten/v1",
  "language": "vyper",
  "compiler": {
    "name": "vyper",
    "version": "<resolved-latest>",
    "binary_path": ".cache/toolchains/vyper/<version>/vyper",
    "binary_sha256": "...",
    "download_source": "https://...",
    "settings": {
      "evmVersion": "latest-shared:<resolved-name>",
      "optimize": "gas",
      "bytecodeMetadata": false
    }
  },
  "source_hash": "...",
  "compile": {
    "wall_ms_samples": [123.4, 124.1, 122.9],
    "cpu_ms_samples": [118.2, 119.0, 118.7],
    "peak_rss_kib": 184320
  },
  "bytecode": {
    "creation_bytes": 4210,
    "runtime_bytes": 2780,
    "runtime_bytes_stripped": 2712,
    "initcode_bytes": 4210
  },
  "gas": {
    "scenario": "transfer_cold_success",
    "evm_fork": "latest-shared:<resolved-name>",
    "execution_gas": 51732,
    "total_tx_gas": 73988
  },
  "correctness": {
    "golden_tests": "pass",
    "differential_tests": "pass",
    "fuzz_seed": "0x..."
  }
}
```

## Reporting

Publish machine-readable outputs and an HTML report in the MVP:

* `results/raw/*.jsonl`: one row per benchmark/profile/scenario sample.
* `results/normalized/*.json`: joined, schema-validated results ready for downstream analysis.
* `results/reports/index.html`: static report with tables, ratios, and Pareto-style charts.

The report should grow toward three views:

1. **Benchmark detail page**: one contract/scenario with all compiler profiles.
2. **Compiler profile page**: one compiler/settings profile across all benchmarks.
3. **Trend page**: compiler versions over time. This is out of MVP because the first scope uses latest compilers only.

Use relative ratios against a clear baseline, for example:

```text
baseline = solc latest, optimizer=true, runs=200, viaIR=false, evmVersion=latest-shared
secondary_baseline = solc latest, optimizer=true, runs=200, viaIR=true, evmVersion=latest-shared
```

For aggregation, use group-weighted geometric means for ratios, but keep raw per-scenario data prominent. Otherwise a large number of artificial microbenchmarks will dominate the score.

## MVP plan

### Milestone 1: minimal but credible

Implement:

* 10 specs: Counter, ERC20 minimal, ERC20 permit hashing, Ownable/Pausable, Vault deposit/withdraw, Factory CREATE2, Minimal proxy, Merkle verifier, AMM pair subset, `scaling_dispatch_N`
* Solidity and Vyper only, with credible source provenance for the first implementation and a hand-written opposing-language implementation
* Compiler profiles:

  * Solidity optimizer off
  * Solidity optimizer runs=200
  * Solidity viaIR runs=200
  * Vyper optimize none
  * Vyper optimize gas
  * Vyper optimize codesize
* Latest stable `solc` and latest stable `vyper` only, resolved by the harness
* EVM fork: latest mutually supported target only
* Toolchain manager that downloads missing compilers, verifies checksums, and records binary metadata
* Foundry-based scenario runner orchestrated by Rust, with `revm` available as an optional cross-check path
* Golden scenario tests and deterministic differential checks before result rows are accepted
* Metrics: compile wall time, peak RSS, creation/runtime size, deploy gas, runtime gas for 5–10 scenarios per contract
* Outputs: raw machine-readable results, normalized JSON, run manifests, and a static HTML report

### Milestone 2: correctness and result hardening

Keep the language/compiler matrix fixed: Solidity and Vyper only, latest stable compiler versions only, latest mutually supported EVM target only. Do not add Yul, Huff, Fe, historical compiler trends, or native-large-project benchmarks in this milestone.

Implement:

* YAML scenario files under `benches/scenarios/*.yaml`. The Rust harness should load hand-authored scenario data from YAML instead of hard-coding benchmark semantics in `catalog.rs`.
* Scenario schema coverage for setup calls, measured calls, observers, expected success/revert, sender/value overrides, deterministic seeds, and `state_access_profile: cold | warm | mixed`.
* Strict validation for benchmark specs, scenario files, result rows, and run manifests. Add a `validate` command or equivalent test path that fails on malformed inputs or outputs.
* Metadata-on and metadata-off compilation variants. Results must expose metadata mode explicitly and keep creation/runtime bytes, stripped bytes, and deployment gas comparable without ambiguity.
* Explicit cold/warm scenario execution. Warm scenarios must perform declared warmup calls before measurement and report `state_access_profile` in raw and normalized outputs.
* Seeded randomized differential checks for suitable benchmarks: `counter`, `erc20_minimal`, `vault_deposit_withdraw`, `ownable_pausable`, and `amm_pair_subset`.
* Failing-seed capture. Persist the RNG seed and call trace for any randomized differential or property failure under `results/raw/failures/` or a similarly clear regression-fixture path.
* Property checks in the generated Foundry harness:
  * ERC20 sampled holder balances do not exceed total supply and total supply stays fixed.
  * Vault sampled balances match total shares after supported deposit/withdraw flows.
  * AMM reserves and liquidity stay coherent after mint, burn, swap, and sync flows.
  * Ownable/Pausable authorization and paused-state invariants hold after state transitions.
  * Counter state matches the accumulated model after randomized increments/adds/resets.
* Report updates for metadata mode, state access profile, randomized differential status, property status, and failing-seed links when failures exist.

Acceptance criteria:

* `cargo test` validates schemas and unit-level parsing/model logic.
* `cargo run -- validate` passes on all checked-in specs and scenarios.
* `cargo run -- run` compiles all profiles, runs golden scenarios, deterministic differential checks, seeded randomized differential checks, and property checks.
* The run produces raw JSONL, normalized JSON, run manifest, and static HTML with metadata mode and cold/warm profile fields populated.
* A failing randomized/property test can be reproduced from the saved seed and trace.

### Milestone 3: scale studies

Keep the language/compiler matrix fixed: Solidity and Vyper only, latest stable compiler versions only, latest mutually supported EVM target only. Do not add Yul, Huff, Fe, historical compiler trends, or native-large-project benchmarks in this milestone.

Add deterministic generated benchmark families:

* `dispatch_N`: external selector fanout with `N` callable functions.
* `storage_slots_N`: read/write paths touching `N` independent storage slots.
* `mapping_depth_N`: mapping access patterns with `N` nested or chained key lookups where both languages can express the shape.
* `abi_args_N`: external functions with `N` ABI arguments, including a separate dynamic-argument variant only if it remains equivalent in Solidity and Vyper.
* `loop_bound_N`: bounded loops over `N` iterations with fixed calldata/pre-state.
* `external_calls_N`: `N` external calls through a deterministic local callee contract.
* `events_N`: event emission paths with `N` indexed/non-indexed fields or `N` repeated emits, depending on the most equivalent shape both languages support.

The generated source files do not need to be checked in. Check in the deterministic family definitions, generator code, schemas, and tests; generate sources, specs, and scenarios into a clearly ignored directory such as `benches/generated/` or `target/bench-generated/`. The run manifest must record the generator version/config, family name, parameter value, generated source hash, and generated scenario hash so any row can be reproduced from checked-in inputs.

Use a fixed initial parameter grid for every family unless a family-specific cap is needed:

* `N = 1, 2, 4, 8, 16, 32, 64`

Generated rows must remain compatible with the Milestone 2 pipeline: metadata-on/off variants, deterministic golden scenarios, deterministic differential checks, raw JSONL, normalized JSON, run manifest, static HTML, and `cargo run -- validate`.
Compiler failures at larger `N` values are benchmark data, not an automatic reason to shrink the grid. Keep deterministic rows for attempted compiles, record the compiler/profile/error text (for example Solidity `Stack too deep`) in normalized output and the HTML report, and run Foundry only for successfully compiled artifacts.

Report updates:

* Add normalized fields for `suite: fixed | scale`, `family`, `parameter_name`, and `parameter_value`.
* Add normalized compile status fields so generated rows can distinguish runnable artifacts from compile failures.
* Add HTML tables or charts that show compile time, runtime bytes, deploy gas, and runtime gas versus `N` by family/profile/language.
* Add HTML visibility for compile failures by family/profile/language.
* Preserve existing fixed-benchmark tables while making generated scale rows visually and machine-readably distinct.

Acceptance criteria:

* All seven generated families above are implemented for both Solidity and Vyper.
* Generated artifacts are deterministic and reproducible from checked-in generator inputs.
* Generated sources/specs/scenarios are ignored rather than checked in.
* `cargo test` covers generator determinism, family schema validation, and at least one generated example per family.
* `cargo run -- validate` validates checked-in specs/scenarios plus generated family definitions and generated outputs if present.
* `cargo run -- run` regenerates scale benchmarks, compiles all fixed and generated profiles, runs deterministic golden and differential checks, and writes reports.
* Normalized results and the run manifest include family metadata and generated source/scenario hashes for every scale row.
* Compile failures are reported as first-class normalized rows and do not prevent running successfully compiled profiles.
* The HTML report includes scale-study views and compile-failure visibility for all seven families.

### Milestone 4: real-derived benchmark models

Add larger benchmark contracts derived from credible production contracts, but port them into the latest Solidity and Vyper compiler targets used by this harness. Treat the upstream contracts as source material and preserve the important public behavior, storage/accounting shape, and hot paths, rather than trying to compile the exact historical production source. Every benchmark must have a Solidity implementation and a Vyper implementation, with the opposing-language version handwritten.

These are explicitly benchmark models, not production-equivalent ports. The safe claim is that they are clean-room, simplified Solidity/Vyper models inspired by production contracts and useful for compiler/codegen comparison on selected hot paths. The unsafe claim is that they are drop-in Uniswap, Curve, or Yearn implementations.

Keep rows machine-readable about provenance:

* `suite: real_derived`
* `model_kind: real_derived_model`
* `production_equivalence: false`
* API, storage-layout, external-token, and source-derivation labels
* upstream project, repository URL, commit, source contract path, and source language
* port language and port version
* exact scenario coverage IDs for the selected hot paths
* included features, excluded production features, and mock assumptions

Locked first candidates:

* `uniswap_v2_pair`: start from Uniswap V2 Pair. Update the Solidity source shape to the latest Solidity compiler target and handwrite the Vyper counterpart. Cover initialization, first/subsequent mint, burn, simplified swap invariant, sync, skim, cumulative price update behavior, and fee-on/off paths where practical. Explicitly exclude external ERC20 transfers, token0/token1 initialization, flash callbacks, INVALID_TO checks, and reentrancy locking unless those paths are actually modeled.
* `curve_stableswap_2coin`: start from Curve Stableswap NG or an equivalent Curve 2-coin pool source. Update the Vyper source shape to the latest Vyper compiler target and handwrite the Solidity counterpart. Cover add/remove liquidity, both exchange directions, invariant math, fees, admin-fee accounting, and slippage/invalid-coin reverts. Explicitly exclude ERC20 transfer/receiver handling, exchange_received, dynamic fees, imbalanced removal, oracle/rate/rebasing/ERC4626 integrations, and nonreentrant lock behavior unless those paths are actually modeled.
* `yearn_vault_v3`: start from Yearn V3 `VaultV3.vy` in `yearn/yearn-vaults-v3`. Update the Vyper source shape to the latest Vyper compiler target and handwrite the Solidity counterpart. Cover initialization, ERC4626-style deposit/mint/withdraw/redeem flows, share price/accounting views, debt increase/decrease, profit/loss reporting through lightweight mock strategies, simplified profit unlock, shutdown, and revert-path coverage for limits and permissions. Treat permit as digest-only unless a real signature verification, allowance update, and nonce increment path is implemented.

Yearn evaluation result, checked on 2026-05-19: choose Yearn V3 Vault over Yearn V2 Vault and V3 TokenizedStrategy. V3 Vault is the best fit because it is current, Vyper-native, large enough to stress bytecode and compile behavior, self-contained apart from ERC20 and small strategy/accountant interfaces, and directly exercises vault share accounting. V2 Vault remains useful historical source material, but its repository is archived and the V3 vault gives a cleaner current target. TokenizedStrategy is current and Solidity-native, but it benchmarks strategy proxy/delegatecall plumbing and BaseStrategy integration more than a standalone vault core; keep it as a possible later `yearn_tokenized_strategy` benchmark after the first real-derived vault lands.

Yearn evaluation notes:

| Candidate | Source evidence | Decision |
| --- | --- | --- |
| Yearn V3 Vault | `yearn/yearn-vaults-v3`, active repo, `contracts/VaultV3.vy`, default branch commit `104a2b233bc6d43ba40720d68355b04d2dc31795`, source blob `97f4a60dd2485c6400f994e46b442d0424716eed`, 80,546 bytes, `# @version 0.3.7` | Use for `yearn_vault_v3` |
| Yearn V2 Vault | `yearn/yearn-vaults-v2`, archived repo, `contracts/Vault.vy`, default branch commit `97ca1b2e4fcf20f4be0ff456dabd020bfeb6697b`, source blob `1159912c093b12cf8d758f71d2d568cda69990af`, 68,942 bytes, `# @version 0.3.3` | Defer as historical source material |
| Yearn V3 TokenizedStrategy | `yearn/tokenized-strategy`, active repo, `src/TokenizedStrategy.sol`, default branch commit `9ef68041bd034353d39941e487499d111c3d3901`, source blob `61e5354d575e59ccac3a3c34793ebc841d66a8e4`, 76,675 bytes, `pragma solidity >=0.8.18` | Defer; better as a future strategy/proxy benchmark than the first vault benchmark |

Explicitly defer Aave for now. Aave V3 Pool remains interesting, but its dependency surface, library graph, oracle/configurator setup, and reserve-token interactions make it a later milestone after the real-derived AMM/vault suite proves out.

Report:

* compile success/failure and compile metrics across existing compiler profiles
* runtime bytecode size, stripped size, deploy gas, and EIP-170/EIP-3860 margins
* deterministic runtime gas for each scoped benchmark-model scenario
* execution gas, intrinsic gas, calldata gas, and total transaction gas as separate fields
* correctness status that distinguishes call success, baseline-only differential checks, profile-wide differential checks, observer checks, return-data checks, log checks, randomized checks, and property checks
* HTML views grouped by upstream project, contract, source language, port language, scenario, and compiler profile, with model-kind, mock assumptions, and excluded production features visible near the gas numbers

Acceptance criteria:

* `cargo run -- validate` checks real-derived metadata, provenance fields, scenario coverage, and output rows.
* `cargo run -- run` compiles fixed, scale, and real-derived suites, runs deterministic scenarios and differential checks, and emits normalized JSON, run manifest, raw gas JSONL, and HTML.
* The Uniswap V2 Pair, Curve 2-coin Stableswap, and Yearn V3 Vault benchmarks are implemented in both Solidity and Vyper.
* The Yearn V3 Vault benchmark records the evaluation rationale, upstream repository, source commit, source path, source language, port language, equivalence scope, and mock strategy/accountant assumptions.
* Normalized rows and the run manifest mark the suite as `real_derived_model`, set `production_equivalence: false`, and make source provenance, included features, excluded features, and mock assumptions explicit enough that downstream analysis cannot confuse these models with unmodified production deployments.
* Scenario coverage is mechanically checked: every real-derived `scenario_coverage` entry must match a checked-in scenario ID, and every checked-in scenario must be listed.
* Setup and warmup calls are fatal on unexpected failure so measured calls cannot run against unintended state.

### Milestone 5: experimental tier

Add:

* Yul and Huff for selected specs
* Fe only if compile/runtime support is stable enough for the chosen specs
* Vyper experimental codegen
* Solidity experimental EOF/SSA profiles only as separate “experimental” rows

## Biggest pitfalls to avoid

The most dangerous mistake is comparing **language defaults** instead of controlled compiler profiles. Defaults change, and latest Solidity and latest Vyper can expose different newest EVM targets. The harness must resolve latest compiler versions, choose the newest shared EVM target, and record the decision in the run manifest. ([Solidity Programming Language][6])

The second biggest mistake is measuring bytecode without controlling metadata. Solidity and Vyper both have metadata-related output settings, and metadata affects bytecode size and deployment gas. ([Solidity Documentation][3])

The third is treating generic gas reports from tests as exact end-user gas. Use Foundry as the MVP runner, but execute fixed transactions in a pinned EVM with fixed pre-state, fixed fork, fixed calldata, and explicit cold/warm assumptions. Foundry’s own docs note that section snapshot cheatcodes require isolated test mode for accuracy. ([foundry - Ethereum Development Framework][16])

## My recommended first benchmark set

Start with these ten:

1. `counter`
2. `erc20_minimal`
3. `erc20_permit_hashing`
4. `ownable_pausable`
5. `vault_deposit_withdraw`
6. `factory_create2`
7. `minimal_proxy`
8. `merkle_verifier`
9. `amm_pair_subset`
10. `scaling_dispatch_N`

That is small enough to implement and validate, but broad enough to catch differences in ABI handling, dispatch, storage, hashing, constructors, external calls, deployment size, and optimizer behavior.

[1]: https://ethereum.org/developers/docs/smart-contracts/languages/ "Smart contract languages | ethereum.org"
[2]: https://eips.ethereum.org/EIPS/eip-170 "EIP-170: Contract code size limit"
[3]: https://docs.soliditylang.org/en/latest/using-the-compiler.html "Using the Compiler — Solidity 0.8.36-develop documentation"
[4]: https://docs.soliditylang.org/en/latest/ir-breaking-changes.html "Solidity IR-based Codegen Changes — Solidity 0.8.36-develop documentation"
[5]: https://docs.soliditylang.org/en/latest/using-the-compiler.html?highlight=optimize-runs "Using the Compiler — Solidity 0.8.36-develop documentation"
[6]: https://www.soliditylang.org/blog/2025/05/07/solidity-0.8.30-release-announcement/ "Solidity 0.8.30 Release Announcement | Solidity Programming Language"
[7]: https://docs.vyperlang.org/en/latest/compiling-a-contract.html "Compiling a Contract - Vyper documentation"
[8]: https://docs.soliditylang.org/en/latest/yul.html "Yul — Solidity 0.8.36-develop documentation"
[9]: https://solang.readthedocs.io/en/latest/language/introduction.html "Brief Language status — Solang Solidity Compiler v0.3.4-29-g24f7aea documentation"
[10]: https://book.getfoundry.sh/forge/gas-reports "foundry - Ethereum Development Framework"
[11]: https://github.com/bluealloy/revm?utm_source=chatgpt.com "bluealloy/revm: Rust implementation of the Ethereum ..."
[12]: https://eips.ethereum.org/EIPS/eip-7623 "EIP-7623: Increase calldata cost"
[13]: https://docs.soliditylang.org/en/latest/metadata.html "Contract Metadata — Solidity 0.8.36-develop documentation"
[14]: https://eips.ethereum.org/EIPS/eip-3860 "EIP-3860: Limit and meter initcode"
[15]: https://github.com/sharkdp/hyperfine?utm_source=chatgpt.com "sharkdp/hyperfine: A command-line benchmarking tool"
[16]: https://getfoundry.sh/forge/gas-tracking/gas-section-snapshots/ "Gas Section Snapshots — foundry - Ethereum Development Framework"
[17]: https://www.soliditylang.org/blog/2026/04/29/solidity-0.8.35-release-announcement/ "Solidity 0.8.35 Release Announcement"
[18]: https://docs.vyperlang.org/en/latest/release-notes.html "Vyper Release Notes"
