use crate::models::{Benchmark, CallSpec, Scenario};

pub fn benchmarks() -> Vec<Benchmark> {
    vec![
        Benchmark {
            id: "counter",
            contract_name: "Counter",
            constructor_args: "abi.encode(uint256(3))",
            solidity_path: "benches/implementations/counter/solidity/Counter.sol",
            vyper_path: "benches/implementations/counter/vyper/Counter.vy",
        },
        Benchmark {
            id: "erc20_minimal",
            contract_name: "Erc20Minimal",
            constructor_args: "abi.encode(uint256(1000 ether))",
            solidity_path: "benches/implementations/erc20_minimal/solidity/Erc20Minimal.sol",
            vyper_path: "benches/implementations/erc20_minimal/vyper/Erc20Minimal.vy",
        },
        Benchmark {
            id: "erc20_permit_hashing",
            contract_name: "Erc20PermitHashing",
            constructor_args: "",
            solidity_path: "benches/implementations/erc20_permit_hashing/solidity/Erc20PermitHashing.sol",
            vyper_path: "benches/implementations/erc20_permit_hashing/vyper/Erc20PermitHashing.vy",
        },
        Benchmark {
            id: "ownable_pausable",
            contract_name: "OwnablePausable",
            constructor_args: "",
            solidity_path: "benches/implementations/ownable_pausable/solidity/OwnablePausable.sol",
            vyper_path: "benches/implementations/ownable_pausable/vyper/OwnablePausable.vy",
        },
        Benchmark {
            id: "vault_deposit_withdraw",
            contract_name: "VaultDepositWithdraw",
            constructor_args: "",
            solidity_path: "benches/implementations/vault_deposit_withdraw/solidity/VaultDepositWithdraw.sol",
            vyper_path: "benches/implementations/vault_deposit_withdraw/vyper/VaultDepositWithdraw.vy",
        },
        Benchmark {
            id: "factory_create2",
            contract_name: "FactoryCreate2",
            constructor_args: "",
            solidity_path: "benches/implementations/factory_create2/solidity/FactoryCreate2.sol",
            vyper_path: "benches/implementations/factory_create2/vyper/FactoryCreate2.vy",
        },
        Benchmark {
            id: "minimal_proxy",
            contract_name: "MinimalProxy",
            constructor_args: "",
            solidity_path: "benches/implementations/minimal_proxy/solidity/MinimalProxy.sol",
            vyper_path: "benches/implementations/minimal_proxy/vyper/MinimalProxy.vy",
        },
        Benchmark {
            id: "merkle_verifier",
            contract_name: "MerkleVerifier",
            constructor_args: "",
            solidity_path: "benches/implementations/merkle_verifier/solidity/MerkleVerifier.sol",
            vyper_path: "benches/implementations/merkle_verifier/vyper/MerkleVerifier.vy",
        },
        Benchmark {
            id: "amm_pair_subset",
            contract_name: "AmmPairSubset",
            constructor_args: "",
            solidity_path: "benches/implementations/amm_pair_subset/solidity/AmmPairSubset.sol",
            vyper_path: "benches/implementations/amm_pair_subset/vyper/AmmPairSubset.vy",
        },
        Benchmark {
            id: "scaling_dispatch_N",
            contract_name: "ScalingDispatchN",
            constructor_args: "",
            solidity_path: "benches/implementations/scaling_dispatch_N/solidity/ScalingDispatchN.sol",
            vyper_path: "benches/implementations/scaling_dispatch_N/vyper/ScalingDispatchN.vy",
        },
    ]
}

pub fn scenarios(benchmark_id: &str) -> Vec<Scenario> {
    match benchmark_id {
        "counter" => vec![
            Scenario {
                name: "read_initial",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("value()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("value()")"#)],
            },
            Scenario {
                name: "increment",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("increment()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("value()")"#)],
            },
            Scenario {
                name: "add_five",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("add(uint256)", uint256(5))"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("value()")"#)],
            },
            Scenario {
                name: "reset",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("add(uint256)", uint256(11))"#)],
                measured: CallSpec::new(r#"abi.encodeWithSignature("reset()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("value()")"#)],
            },
        ],
        "erc20_minimal" => vec![
            Scenario {
                name: "total_supply",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("totalSupply()")"#),
                expect_success: true,
                observers: erc20_observers(),
            },
            Scenario {
                name: "transfer_success",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("transfer(address,uint256)", BOB, uint256(10 ether))"#),
                expect_success: true,
                observers: erc20_observers(),
            },
            Scenario {
                name: "approve_success",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("approve(address,uint256)", BOB, uint256(7 ether))"#),
                expect_success: true,
                observers: erc20_observers(),
            },
            Scenario {
                name: "transfer_from_success",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("approve(address,uint256)", BOB, uint256(9 ether))"#)],
                measured: CallSpec::sender(
                    r#"abi.encodeWithSignature("transferFrom(address,address,uint256)", address(this), CAROL, uint256(5 ether))"#,
                    "BOB",
                ),
                expect_success: true,
                observers: erc20_observers(),
            },
            Scenario {
                name: "transfer_revert",
                setup: vec![],
                measured: CallSpec::sender(
                    r#"abi.encodeWithSignature("transfer(address,uint256)", CAROL, uint256(1 ether))"#,
                    "BOB",
                ),
                expect_success: false,
                observers: erc20_observers(),
            },
        ],
        "erc20_permit_hashing" => vec![
            Scenario {
                name: "hash_static",
                setup: vec![],
                measured: CallSpec::new(
                    r#"abi.encodeWithSignature("hashPermit(address,address,uint256,uint256,uint256)", address(this), BOB, uint256(1), uint256(0), uint256(999))"#,
                ),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("nonces(address)", address(this))"#)],
            },
            Scenario {
                name: "use_nonce",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("useNonce(address)", address(this))"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("nonces(address)", address(this))"#)],
            },
            Scenario {
                name: "hash_current_after_nonce",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("useNonce(address)", address(this))"#)],
                measured: CallSpec::new(
                    r#"abi.encodeWithSignature("hashCurrentPermit(address,address,uint256,uint256)", address(this), BOB, uint256(1), uint256(999))"#,
                ),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("nonces(address)", address(this))"#)],
            },
        ],
        "ownable_pausable" => vec![
            Scenario {
                name: "guarded_increment",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("guardedIncrement()")"#),
                expect_success: true,
                observers: ownable_observers(),
            },
            Scenario {
                name: "pause",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("pause()")"#),
                expect_success: true,
                observers: ownable_observers(),
            },
            Scenario {
                name: "paused_revert",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("pause()")"#)],
                measured: CallSpec::new(r#"abi.encodeWithSignature("guardedIncrement()")"#),
                expect_success: false,
                observers: ownable_observers(),
            },
            Scenario {
                name: "non_owner_pause_revert",
                setup: vec![],
                measured: CallSpec::sender(r#"abi.encodeWithSignature("pause()")"#, "BOB"),
                expect_success: false,
                observers: ownable_observers(),
            },
        ],
        "vault_deposit_withdraw" => vec![
            Scenario {
                name: "deposit_one_eth",
                setup: vec![],
                measured: CallSpec::value(r#"abi.encodeWithSignature("deposit()")"#, "1 ether"),
                expect_success: true,
                observers: vault_observers(),
            },
            Scenario {
                name: "withdraw_half_eth",
                setup: vec![CallSpec::value(r#"abi.encodeWithSignature("deposit()")"#, "1 ether")],
                measured: CallSpec::new(r#"abi.encodeWithSignature("withdraw(uint256)", uint256(0.5 ether))"#),
                expect_success: true,
                observers: vault_observers(),
            },
            Scenario {
                name: "withdraw_revert",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("withdraw(uint256)", uint256(1 ether))"#),
                expect_success: false,
                observers: vault_observers(),
            },
        ],
        "factory_create2" => vec![
            Scenario {
                name: "init_code_hash",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("initCodeHash(uint256)", uint256(7))"#),
                expect_success: true,
                observers: vec![],
            },
            Scenario {
                name: "deploy_value",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("deploy(bytes32,uint256)", SALT, uint256(7))"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("deployedValue(bytes32)", SALT)"#)],
            },
            Scenario {
                name: "deployed_value",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("deploy(bytes32,uint256)", SALT, uint256(7))"#)],
                measured: CallSpec::new(r#"abi.encodeWithSignature("deployedValue(bytes32)", SALT)"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("deployedValue(bytes32)", SALT)"#)],
            },
        ],
        "minimal_proxy" => vec![
            Scenario {
                name: "proxy_code_hash",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("proxyCodeHash(address)", IMPLEMENTATION)"#),
                expect_success: true,
                observers: vec![],
            },
            Scenario {
                name: "clone_and_set",
                setup: vec![],
                measured: CallSpec::new(
                    r#"abi.encodeWithSignature("cloneAndSet(address,bytes32,uint256)", IMPLEMENTATION, SALT, uint256(42))"#,
                ),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("cloneValue(bytes32)", SALT)"#)],
            },
            Scenario {
                name: "clone_value",
                setup: vec![CallSpec::new(
                    r#"abi.encodeWithSignature("cloneAndSet(address,bytes32,uint256)", IMPLEMENTATION, SALT, uint256(42))"#,
                )],
                measured: CallSpec::new(r#"abi.encodeWithSignature("cloneValue(bytes32)", SALT)"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("cloneValue(bytes32)", SALT)"#)],
            },
        ],
        "merkle_verifier" => vec![
            Scenario {
                name: "hash_pair",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("hashPair(bytes32,bytes32)", LEAF, SIBLING)"#),
                expect_success: true,
                observers: vec![],
            },
            Scenario {
                name: "verify_single",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("verify(bytes32[],bytes32,bytes32)", proofOne(), ROOT, LEAF)"#),
                expect_success: true,
                observers: vec![],
            },
            Scenario {
                name: "verify_empty_false",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("verify(bytes32[],bytes32,bytes32)", proofEmpty(), ROOT, LEAF)"#),
                expect_success: true,
                observers: vec![],
            },
        ],
        "amm_pair_subset" => vec![
            Scenario {
                name: "mint",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("mint(uint256,uint256)", uint256(100), uint256(200))"#),
                expect_success: true,
                observers: amm_observers(),
            },
            Scenario {
                name: "burn",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("mint(uint256,uint256)", uint256(100), uint256(200))"#)],
                measured: CallSpec::new(r#"abi.encodeWithSignature("burn(uint256)", uint256(150))"#),
                expect_success: true,
                observers: amm_observers(),
            },
            Scenario {
                name: "swap",
                setup: vec![CallSpec::new(r#"abi.encodeWithSignature("mint(uint256,uint256)", uint256(100), uint256(200))"#)],
                measured: CallSpec::new(
                    r#"abi.encodeWithSignature("swap(uint256,uint256,uint256,uint256)", uint256(10), uint256(0), uint256(0), uint256(20))"#,
                ),
                expect_success: true,
                observers: amm_observers(),
            },
            Scenario {
                name: "sync",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("sync(uint256,uint256)", uint256(5), uint256(9))"#),
                expect_success: true,
                observers: amm_observers(),
            },
        ],
        "scaling_dispatch_N" => vec![
            Scenario {
                name: "first_selector",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("f00()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("sink()")"#)],
            },
            Scenario {
                name: "middle_selector",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("f15()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("sink()")"#)],
            },
            Scenario {
                name: "last_selector",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("f31()")"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("sink()")"#)],
            },
            Scenario {
                name: "set_sink",
                setup: vec![],
                measured: CallSpec::new(r#"abi.encodeWithSignature("setSink(uint256)", uint256(99))"#),
                expect_success: true,
                observers: vec![CallSpec::new(r#"abi.encodeWithSignature("sink()")"#)],
            },
        ],
        _ => vec![],
    }
}

fn erc20_observers() -> Vec<CallSpec> {
    vec![
        CallSpec::new(r#"abi.encodeWithSignature("totalSupply()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("balanceOf(address)", address(this))"#),
        CallSpec::new(r#"abi.encodeWithSignature("balanceOf(address)", BOB)"#),
        CallSpec::new(r#"abi.encodeWithSignature("balanceOf(address)", CAROL)"#),
        CallSpec::new(r#"abi.encodeWithSignature("allowance(address,address)", address(this), BOB)"#),
    ]
}

fn ownable_observers() -> Vec<CallSpec> {
    vec![
        CallSpec::new(r#"abi.encodeWithSignature("owner()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("paused()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("counter()")"#),
    ]
}

fn vault_observers() -> Vec<CallSpec> {
    vec![
        CallSpec::new(r#"abi.encodeWithSignature("balanceOf(address)", address(this))"#),
        CallSpec::new(r#"abi.encodeWithSignature("totalShares()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("totalAssets()")"#),
    ]
}

fn amm_observers() -> Vec<CallSpec> {
    vec![
        CallSpec::new(r#"abi.encodeWithSignature("reserve0()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("reserve1()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("totalLiquidity()")"#),
        CallSpec::new(r#"abi.encodeWithSignature("getReserves()")"#),
    ]
}
