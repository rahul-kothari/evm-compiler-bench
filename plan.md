## Recommended direction

Build **EVM Compiler Bench** as two benchmarks, not one:

1. **Equivalent-contract benchmark**: small-to-medium contracts generated from a shared behavioral spec and implemented in Solidity, Vyper, Yul, maybe Huff/Fe. This is where you compare compilation time, bytecode size, deployment gas, and runtime gas fairly.
2. **Native-large-project benchmark**: real projects such as Uniswap-scale Solidity codebases, plus Vyper-native projects where available. This is mainly for **compiler throughput, cache behavior, optimizer performance, and artifact size**, not cross-language Òequivalence.Ó

That split matters because Ethereum.org currently treats **Solidity and Vyper** as the two most active and maintained smart-contract languages, while Yul is lower-level/intermediate and Fe is still emerging; Huff is explicitly low-level and optimized around direct EVM control. A benchmark that claims ÒSolidity vs Vyper vs Huff on UniswapÓ would be misleading unless the benchmark clearly separates generated equivalents from native ecosystems. ([ethereum.org][1])

## What to benchmark

Use four primary metric families.

| Area               |                                                                                                              Metrics | Notes                                                                                                                                                                                                                                                |
| ------------------ | -------------------------------------------------------------------------------------------------------------------: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Compilation**    |                                        wall-clock time, CPU time, peak RSS, output artifact count/size, failure rate | Separate compiler-only runs from framework builds.                                                                                                                                                                                                   |
| **Bytecode**       | creation bytecode bytes, runtime bytecode bytes, metadata-stripped bytes, initcode bytes, deployed-code limit margin | Track whether contracts approach EIP-170Õs 24,576-byte deployed-code limit and EIP-3860Õs 49,152-byte initcode limit. ([Ethereum Improvement Proposals][2])                                                                                          |
| **Deployment gas** |                                     constructor execution gas, initcode cost, code-deposit cost, total create tx gas | Report with and without metadata where possible; metadata affects deploy size and deploy gas.                                                                                                                                                        |
| **Runtime gas**    |            per-scenario execution gas, tx gas including calldata, cold/warm variants, revert-path gas, log/event gas | Pin the EVM fork because gas rules and compiler decisions are fork-sensitive. Solidity and Vyper both expose explicit EVM-version settings and warn that compiling for the wrong EVM version can produce bad behavior. ([Solidity Documentation][3]) |

Do **not** collapse these into one ÒwinnerÓ score. Publish per-benchmark results, group summaries, and Pareto plots: compile time vs runtime gas, runtime bytecode size vs runtime gas, deploy gas vs runtime gas.

## Compiler matrix

Start with a focused matrix and expand later.

| Tier                               | Include                                                                                | Why                                                                                                                                                                                                                                          |
| ---------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Core**                           | `solc` legacy codegen, `solc --via-ir`, Vyper normal pipeline                          | These are the important production-relevant comparisons. SolidityÕs IR pipeline is a distinct bytecode-generation path via Yul and can enable stronger cross-function optimization. ([Solidity Documentation][4])                            |
| **Optimizer profiles**             | Solidity optimizer off, runs=1, runs=200, runs=10_000; Vyper `none`, `codesize`, `gas` | Solidity optimizer runs trade deployment cost against expected runtime calls; Vyper exposes `none`, `codesize`, and `gas`, with different selector-table and loop/code-size behavior. ([Solidity Documentation][5])                          |
| **Fork profiles**                  | Shanghai, Cancun, Prague, and Òcompiler defaultÓ                                       | Defaults change. Solidity 0.8.30 changed its default EVM version from Cancun to Prague, and Vyper 0.4.3 also made Prague the default. Pinning avoids accidental benchmark drift. ([Solidity Programming Language][6])                        |
| **Experimental**                   | Vyper Venom, Solidity EOF/SSA-CFG where available, Fe                                  | Keep separate from the headline benchmark because experimental features and non-production-ready languages can distort conclusions. Vyper documents Venom as experimental codegen, and Fe 26.x says it is not production-ready. ([Vyper][7]) |
| **Low-level reference**            | Yul, Huff                                                                              | Useful as Òhow low can bytecode/gas go?Ó baselines, not fair high-level-language comparisons. Yul is SolidityÕs intermediate language and can be used stand-alone; Huff exposes the EVM stack directly. ([Solidity Documentation][8])        |
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

Do not ask Codex to Òtranslate Solidity to VyperÓ directly as the main workflow. That biases the Vyper code toward Solidity structure and can create bad idioms. Instead:

1. Write a **language-neutral spec** per benchmark.
2. Generate Solidity, Vyper, Yul, etc. from the spec.
3. Compile.
4. Run differential tests.
5. Keep only implementations that pass.

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

1. **Golden scenario tests**: deterministic call sequences with exact expected return values, logs, and state.
2. **Differential tests**: run the same randomized call sequence against every implementation and compare observable outputs.
3. **Property tests**: invariants such as total supply conservation, no negative balances, authorization rules, idempotent views, revert conditions.

Store failing fuzz seeds and minimized counterexamples as regression fixtures. For revert equivalence, use two modes: **semantic revert** means Òboth revert,Ó while **ABI-exact revert** means Òsame encoded custom error/string.Ó The first is better for cross-language comparison; the second is useful for stricter ABI compatibility.

## Gas measurement design

Use a custom EVM runner as the primary source of truth, with Foundry/Hardhat as validation and developer ergonomics.

Foundry is useful because it can emit gas reports, deployment size, deployment cost, per-function min/avg/median/max, gas snapshots, and section snapshots; it also has isolated test mode, which improves gas accounting by executing top-level calls in separate EVM contexts. ([foundry - Ethereum Development Framework][10])

For the canonical benchmark, I would build a Rust runner on `revm` or a similarly pinned EVM implementation. `revm` is a Rust EVM implementation used broadly in Ethereum tooling and exposes an execution API suitable for deterministic transaction execution. ([GitHub][11])

Each scenario should report:

```text
execution_gas
tx_intrinsic_gas
calldata_gas
calldata_floor_gas_if_applicable
total_tx_gas
state_access_profile: cold | warm | mixed
evm_fork: shanghai | cancun | prague | ...
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
| **Framework clean build**       | Foundry/Hardhat/Vyper project build after cache deletion   |
| **Framework incremental build** | One-file change or no-op rebuild                           |
| **Matrix build**                | Multiple optimizer/fork/compiler versions                  |

Use `solc --standard-json` for automated Solidity compilation because Solidity documents it as the recommended interface for complex automated use. For Vyper, use `vyper-json` or Vyper archives for reproducible inputs/settings. ([Solidity Documentation][3])

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

SolidityÕs docs warn that requesting all outputs can slow compilation unnecessarily, so define a minimal output profile for timing and a separate full-artifact profile for artifact-size/reporting tests. ([Solidity Documentation][3])

## Repository structure

```text
evm-compiler-bench/
  benches/
    specs/
      erc20_minimal.yaml
      vault.yaml
      amm_pair_subset.yaml
      scaling_dispatch_n.yaml
    implementations/
      erc20_minimal/
        solidity/
        vyper/
        yul/
        huff/
    scenarios/
      erc20_minimal.scenarios.json
  compiler-profiles/
    solc-0.8.x-legacy-runs200-prague.toml
    solc-0.8.x-viair-runs200-prague.toml
    vyper-0.4.x-gas-prague.toml
    vyper-0.4.x-codesize-prague.toml
  harness/
    compiler_runner/
    evm_runner/
    bytecode_analyzer/
    report_generator/
  native-projects/
    solidity/
    vyper/
  results/
    raw/
    normalized/
    reports/
  schemas/
    benchmark_spec.schema.json
    result.schema.json
```

## Result schema

Every result row should be joinable by benchmark, implementation, compiler, and scenario:

```json
{
  "benchmark_id": "erc20_minimal",
  "implementation_id": "vyper/generated/v1",
  "language": "vyper",
  "compiler": {
    "name": "vyper",
    "version": "0.4.x",
    "binary_sha256": "...",
    "settings": {
      "evmVersion": "prague",
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
    "evm_fork": "prague",
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

Publish three report views:

1. **Benchmark detail page**: one contract/scenario with all compiler profiles.
2. **Compiler profile page**: one compiler/settings profile across all benchmarks.
3. **Trend page**: compiler versions over time.

Use relative ratios against a clear baseline, for example:

```text
baseline = solc stable, optimizer=true, runs=200, viaIR=false, evmVersion=prague
secondary_baseline = solc stable, optimizer=true, runs=200, viaIR=true, evmVersion=prague
```

For aggregation, use group-weighted geometric means for ratios, but keep raw per-scenario data prominent. Otherwise a large number of artificial microbenchmarks will dominate the score.

## MVP plan

### Milestone 1: minimal but credible

Implement:

* 5 specs: Counter, ERC20 minimal, Vault, Factory CREATE2, AMM pair subset
* Solidity and Vyper only
* Compiler profiles:

  * Solidity optimizer off
  * Solidity optimizer runs=200
  * Solidity viaIR runs=200
  * Vyper optimize none
  * Vyper optimize gas
  * Vyper optimize codesize
* EVM fork: Prague pinned
* Metrics: compile wall time, peak RSS, creation/runtime size, deploy gas, runtime gas for 5Ð10 scenarios per contract

### Milestone 2: correctness hardening

Add:

* Differential runner
* Property tests
* Fuzz seed storage
* Metadata-on and metadata-off bytecode modes
* Cold/warm state scenarios
* JSON result schema and static HTML report

### Milestone 3: scale studies

Add generated families:

* `dispatch_N`
* `storage_slots_N`
* `mapping_depth_N`
* `abi_args_N`
* `loop_bound_N`
* `external_calls_N`
* `events_N`

This is where compiler-time and optimizer-scaling insights will show up.

### Milestone 4: native-large-project suite

Add real projects, but label them explicitly as native/non-equivalent. Report:

* clean build time
* incremental build time
* peak memory
* artifact size
* bytecode size distribution
* number of contracts near EIP-170 limit

### Milestone 5: experimental tier

Add:

* Yul and Huff for selected specs
* Fe only if compile/runtime support is stable enough for the chosen specs
* Vyper experimental codegen
* Solidity experimental EOF/SSA profiles only as separate ÒexperimentalÓ rows

## Biggest pitfalls to avoid

The most dangerous mistake is comparing **language defaults** instead of controlled compiler profiles. Defaults change, and Solidity/Vyper have already shifted default EVM versions around Prague-era releases. ([Solidity Programming Language][6])

The second biggest mistake is measuring bytecode without controlling metadata. Solidity and Vyper both have metadata-related output settings, and metadata affects bytecode size and deployment gas. ([Solidity Documentation][3])

The third is treating gas reports from tests as exact end-user gas. Use Foundry or Hardhat for development feedback, but the canonical benchmark should execute fixed transactions in a pinned EVM with fixed pre-state, fixed fork, fixed calldata, and explicit cold/warm assumptions. FoundryÕs own docs note that section snapshot cheatcodes require isolated test mode for accuracy. ([foundry - Ethereum Development Framework][16])

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
[3]: https://docs.soliditylang.org/en/latest/using-the-compiler.html "Using the Compiler Ñ Solidity 0.8.36-develop documentation"
[4]: https://docs.soliditylang.org/en/latest/ir-breaking-changes.html "Solidity IR-based Codegen Changes Ñ Solidity 0.8.36-develop documentation"
[5]: https://docs.soliditylang.org/en/latest/using-the-compiler.html?highlight=optimize-runs "Using the Compiler Ñ Solidity 0.8.36-develop documentation"
[6]: https://www.soliditylang.org/blog/2025/05/07/solidity-0.8.30-release-announcement/ "Solidity 0.8.30 Release Announcement | Solidity Programming Language"
[7]: https://docs.vyperlang.org/en/latest/compiling-a-contract.html "Compiling a Contract - Vyper documentation"
[8]: https://docs.soliditylang.org/en/latest/yul.html "Yul Ñ Solidity 0.8.36-develop documentation"
[9]: https://solang.readthedocs.io/en/latest/language/introduction.html "Brief Language status Ñ Solang Solidity Compiler v0.3.4-29-g24f7aea documentation"
[10]: https://book.getfoundry.sh/forge/gas-reports "foundry - Ethereum Development Framework"
[11]: https://github.com/bluealloy/revm?utm_source=chatgpt.com "bluealloy/revm: Rust implementation of the Ethereum ..."
[12]: https://eips.ethereum.org/EIPS/eip-7623 "EIP-7623: Increase calldata cost"
[13]: https://docs.soliditylang.org/en/latest/metadata.html "Contract Metadata Ñ Solidity 0.8.36-develop documentation"
[14]: https://eips.ethereum.org/EIPS/eip-3860 "EIP-3860: Limit and meter initcode"
[15]: https://github.com/sharkdp/hyperfine?utm_source=chatgpt.com "sharkdp/hyperfine: A command-line benchmarking tool"
[16]: https://getfoundry.sh/forge/gas-tracking/gas-section-snapshots/ "Gas Section Snapshots Ð foundry - Ethereum Development Framework"
