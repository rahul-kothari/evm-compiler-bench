use crate::models::{Benchmark, Language, Provenance};

pub fn fixed_benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark::fixed(
            "counter",
            "Counter",
            "benches/implementations/counter/solidity/Counter.sol",
            "benches/implementations/counter/vyper/Counter.vy",
        ),
        Benchmark::fixed(
            "erc20_minimal",
            "Erc20Minimal",
            "benches/implementations/erc20_minimal/solidity/Erc20Minimal.sol",
            "benches/implementations/erc20_minimal/vyper/Erc20Minimal.vy",
        ),
        Benchmark::fixed(
            "erc20_permit_hashing",
            "Erc20PermitHashing",
            "benches/implementations/erc20_permit_hashing/solidity/Erc20PermitHashing.sol",
            "benches/implementations/erc20_permit_hashing/vyper/Erc20PermitHashing.vy",
        ),
        Benchmark::fixed(
            "ownable_pausable",
            "OwnablePausable",
            "benches/implementations/ownable_pausable/solidity/OwnablePausable.sol",
            "benches/implementations/ownable_pausable/vyper/OwnablePausable.vy",
        ),
        Benchmark::fixed(
            "vault_deposit_withdraw",
            "VaultDepositWithdraw",
            "benches/implementations/vault_deposit_withdraw/solidity/VaultDepositWithdraw.sol",
            "benches/implementations/vault_deposit_withdraw/vyper/VaultDepositWithdraw.vy",
        ),
        Benchmark::fixed(
            "factory_create2",
            "FactoryCreate2",
            "benches/implementations/factory_create2/solidity/FactoryCreate2.sol",
            "benches/implementations/factory_create2/vyper/FactoryCreate2.vy",
        ),
        Benchmark::fixed(
            "minimal_proxy",
            "MinimalProxy",
            "benches/implementations/minimal_proxy/solidity/MinimalProxy.sol",
            "benches/implementations/minimal_proxy/vyper/MinimalProxy.vy",
        ),
        Benchmark::fixed(
            "merkle_verifier",
            "MerkleVerifier",
            "benches/implementations/merkle_verifier/solidity/MerkleVerifier.sol",
            "benches/implementations/merkle_verifier/vyper/MerkleVerifier.vy",
        ),
        Benchmark::fixed(
            "amm_pair_subset",
            "AmmPairSubset",
            "benches/implementations/amm_pair_subset/solidity/AmmPairSubset.sol",
            "benches/implementations/amm_pair_subset/vyper/AmmPairSubset.vy",
        ),
        Benchmark::fixed(
            "scaling_dispatch_N",
            "ScalingDispatchN",
            "benches/implementations/scaling_dispatch_N/solidity/ScalingDispatchN.sol",
            "benches/implementations/scaling_dispatch_N/vyper/ScalingDispatchN.vy",
        ),
    ]
}

pub fn real_derived_benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark::real_derived(
            "uniswap_v2_pair",
            "UniswapV2PairReal",
            "benches/implementations/uniswap_v2_pair/solidity/UniswapV2PairReal.sol",
            "benches/implementations/uniswap_v2_pair/vyper/UniswapV2PairReal.vy",
            Provenance {
                upstream_project: "Uniswap V2 Core".to_string(),
                repository_url: "https://github.com/Uniswap/v2-core".to_string(),
                source_commit: "6a9e7c97860676e0992f22a49665760444c1cdf5".to_string(),
                source_path: "contracts/UniswapV2Pair.sol".to_string(),
                source_language: Language::Solidity,
                source_contract: "UniswapV2Pair".to_string(),
                source_blob: Some("f87a1db262fba132862eae377d8cdaef74c79f97".to_string()),
                upstream_license: "GPL-3.0-or-later".to_string(),
                checked_at: "2026-05-19".to_string(),
                equivalence_scope: strings(&[
                    "reserve packing shape and getReserves view",
                    "mint, burn, swap, skim, sync state transitions",
                    "constant-product fee check with 0.30% LP fee",
                    "fee-on protocol liquidity minting through kLast",
                    "cumulative price update behavior",
                    "ERC20-like liquidity share accounting",
                ]),
                scenario_coverage: strings(&[
                    "initialize",
                    "mint first and subsequent liquidity",
                    "burn liquidity",
                    "swap with fee invariant",
                    "skim and sync",
                    "fee-on mint path",
                    "revert paths for locked/uninitialized/invalid swaps",
                ]),
                mock_assumptions: strings(&[
                    "token balances are modeled as internal reserves and shadow balances instead of external ERC20 calls",
                    "block timestamp is advanced by explicit benchmark calls for deterministic cumulative price math",
                ]),
            },
        ),
        Benchmark::real_derived(
            "curve_stableswap_2coin",
            "CurveStableSwap2CoinReal",
            "benches/implementations/curve_stableswap_2coin/solidity/CurveStableSwap2CoinReal.sol",
            "benches/implementations/curve_stableswap_2coin/vyper/CurveStableSwap2CoinReal.vy",
            Provenance {
                upstream_project: "Curve Stableswap NG".to_string(),
                repository_url: "https://github.com/curvefi/stableswap-ng".to_string(),
                source_commit: "2abe778f40206a6c0fd108a0a53ad3266cbedeee".to_string(),
                source_path: "contracts/main/CurveStableSwapNG.vy".to_string(),
                source_language: Language::Vyper,
                source_contract: "CurveStableSwapNG".to_string(),
                source_blob: Some("f192932a22958735acad4ca24c977cc4e058f1aa".to_string()),
                upstream_license:
                    "Curve copyright notice; benchmark port is hand-written source material only"
                        .to_string(),
                checked_at: "2026-05-19".to_string(),
                equivalence_scope: strings(&[
                    "2-coin invariant and get_D/get_y math",
                    "add_liquidity, remove_liquidity, remove_liquidity_one_coin, and exchange flows",
                    "LP token accounting",
                    "admin-free fee accounting with deterministic balances",
                    "virtual price and rate-free standard-token path",
                ]),
                scenario_coverage: strings(&[
                    "initial and imbalanced add liquidity",
                    "exchange both directions",
                    "balanced and single-coin withdrawal",
                    "fee path",
                    "revert paths for slippage and invalid coin index",
                ]),
                mock_assumptions: strings(&[
                    "coins are standard 1:1 precision assets with internal pool balances",
                    "admin fee balances are tracked but no external token transfers are performed",
                    "oracle, rebasing, ERC4626, and external rate paths are intentionally out of scope",
                ]),
            },
        ),
        Benchmark::real_derived(
            "yearn_vault_v3",
            "YearnVaultV3Real",
            "benches/implementations/yearn_vault_v3/solidity/YearnVaultV3Real.sol",
            "benches/implementations/yearn_vault_v3/vyper/YearnVaultV3Real.vy",
            Provenance {
                upstream_project: "Yearn Vaults V3".to_string(),
                repository_url: "https://github.com/yearn/yearn-vaults-v3".to_string(),
                source_commit: "104a2b233bc6d43ba40720d68355b04d2dc31795".to_string(),
                source_path: "contracts/VaultV3.vy".to_string(),
                source_language: Language::Vyper,
                source_contract: "VaultV3".to_string(),
                source_blob: Some("97f4a60dd2485c6400f994e46b442d0424716eed".to_string()),
                upstream_license: "GNU AGPLv3".to_string(),
                checked_at: "2026-05-19".to_string(),
                equivalence_scope: strings(&[
                    "ERC4626-style deposit, mint, withdraw, and redeem",
                    "share conversion and profit-unlocking accounting",
                    "strategy registry, max debt, debt update, and withdrawal queue",
                    "profit/loss report processing with fee/refund hooks simplified to benchmark storage",
                    "shutdown and permission checks",
                    "permit digest and nonce path",
                ]),
                scenario_coverage: strings(&[
                    "initialize",
                    "deposit/mint and withdraw/redeem",
                    "strategy add and debt update",
                    "profit and loss reports",
                    "withdrawal from strategy queue",
                    "shutdown and permission/limit reverts",
                    "permit nonce/digest path",
                ]),
                mock_assumptions: strings(&[
                    "asset and strategy balances are deterministic internal ledgers",
                    "mock strategy profit/loss is configured through benchmark helper calls",
                    "accountant behavior is represented by fee basis points and refund fields",
                ]),
            },
        ),
    ]
}

pub fn checked_in_benchmarks() -> Vec<Benchmark> {
    fixed_benchmarks()
        .into_iter()
        .chain(real_derived_benchmarks())
        .collect()
}

pub fn all_benchmarks(generated: Vec<Benchmark>, only_benchmark: Option<&str>) -> Vec<Benchmark> {
    checked_in_benchmarks()
        .into_iter()
        .chain(generated)
        .filter(|benchmark| only_benchmark.is_none_or(|id| id == benchmark.id))
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}
