use crate::models::Benchmark;

pub fn benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark {
            id: "counter",
            contract_name: "Counter",
            solidity_path: "benches/implementations/counter/solidity/Counter.sol",
            vyper_path: "benches/implementations/counter/vyper/Counter.vy",
        },
        Benchmark {
            id: "erc20_minimal",
            contract_name: "Erc20Minimal",
            solidity_path: "benches/implementations/erc20_minimal/solidity/Erc20Minimal.sol",
            vyper_path: "benches/implementations/erc20_minimal/vyper/Erc20Minimal.vy",
        },
        Benchmark {
            id: "erc20_permit_hashing",
            contract_name: "Erc20PermitHashing",
            solidity_path: "benches/implementations/erc20_permit_hashing/solidity/Erc20PermitHashing.sol",
            vyper_path: "benches/implementations/erc20_permit_hashing/vyper/Erc20PermitHashing.vy",
        },
        Benchmark {
            id: "ownable_pausable",
            contract_name: "OwnablePausable",
            solidity_path: "benches/implementations/ownable_pausable/solidity/OwnablePausable.sol",
            vyper_path: "benches/implementations/ownable_pausable/vyper/OwnablePausable.vy",
        },
        Benchmark {
            id: "vault_deposit_withdraw",
            contract_name: "VaultDepositWithdraw",
            solidity_path: "benches/implementations/vault_deposit_withdraw/solidity/VaultDepositWithdraw.sol",
            vyper_path: "benches/implementations/vault_deposit_withdraw/vyper/VaultDepositWithdraw.vy",
        },
        Benchmark {
            id: "factory_create2",
            contract_name: "FactoryCreate2",
            solidity_path: "benches/implementations/factory_create2/solidity/FactoryCreate2.sol",
            vyper_path: "benches/implementations/factory_create2/vyper/FactoryCreate2.vy",
        },
        Benchmark {
            id: "minimal_proxy",
            contract_name: "MinimalProxy",
            solidity_path: "benches/implementations/minimal_proxy/solidity/MinimalProxy.sol",
            vyper_path: "benches/implementations/minimal_proxy/vyper/MinimalProxy.vy",
        },
        Benchmark {
            id: "merkle_verifier",
            contract_name: "MerkleVerifier",
            solidity_path: "benches/implementations/merkle_verifier/solidity/MerkleVerifier.sol",
            vyper_path: "benches/implementations/merkle_verifier/vyper/MerkleVerifier.vy",
        },
        Benchmark {
            id: "amm_pair_subset",
            contract_name: "AmmPairSubset",
            solidity_path: "benches/implementations/amm_pair_subset/solidity/AmmPairSubset.sol",
            vyper_path: "benches/implementations/amm_pair_subset/vyper/AmmPairSubset.vy",
        },
        Benchmark {
            id: "scaling_dispatch_N",
            contract_name: "ScalingDispatchN",
            solidity_path: "benches/implementations/scaling_dispatch_N/solidity/ScalingDispatchN.sol",
            vyper_path: "benches/implementations/scaling_dispatch_N/vyper/ScalingDispatchN.vy",
        },
    ]
}
