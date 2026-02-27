// Common test utilities for codegen integration tests

use indexmap::IndexMap;
use merak_analyzer::{analyze, analyze_ssa};
use merak_ast::contract::Program;
use merak_ast::NodeIdGenerator;
use merak_codegen::{Codegen, CompiledProgram};
use merak_ir::transformers::ssa::SsaBuilder;
use merak_parser::parse_program;
use merak_symbols::SymbolTable;
use primitive_types::{H160, U256};
use revm::primitives::{AccountInfo, Address, ExecutionResult, Output, TransactTo, KECCAK_EMPTY};
use revm::{Database, DatabaseCommit, Evm, InMemoryDB};

/// Compiles Merak source code to EVM bytecode
///
/// Returns both the compiled program and symbol table
pub fn compile_from_source(source: &str) -> Result<(CompiledProgram, SymbolTable), String> {
    // Parse source
    let id_gen = NodeIdGenerator::new();
    let file = parse_program(source, &id_gen).map_err(|e| format!("Parse error: {:?}", e))?;

    println!("Contract: {}", file);

    // Build program structure
    let mut files = IndexMap::new();
    files.insert(file.contract.name.clone(), file);
    let program = Program { files };

    // Run symbol analysis
    let symbol_table = analyze(&program).map_err(|e| format!("Analysis error: {:?}", e))?;

    // Build SSA IR
    let mut ssa_builder = SsaBuilder::new(symbol_table.clone());
    let mut ssa_program = ssa_builder
        .build(&program)
        .map_err(|e| format!("SSA build error: {:?}", e))?;

    // Run storage analysis
    for file in ssa_program.files.values_mut() {
        analyze_ssa(&mut file.contract, &symbol_table)
            .map_err(|e| format!("Storage analysis error: {:?}", e))?;
    }

    // Compile to bytecode
    let codegen = Codegen::new();
    let compiled = codegen
        .compile_program(&mut ssa_program, &symbol_table)
        .map_err(|e| format!("Codegen error: {:?}", e))?;

    Ok((compiled, symbol_table))
}

/// Execution result from calling a contract function
#[derive(Debug)]
pub struct CallResult {
    pub success: bool,
    pub return_data: Vec<u8>,
    pub gas_used: u64,
}

impl CallResult {
    /// Decode return data as U256
    pub fn as_uint(&self) -> Option<U256> {
        if self.return_data.len() == 32 {
            Some(U256::from_big_endian(&self.return_data))
        } else {
            None
        }
    }

    /// Decode return data as bool
    pub fn as_bool(&self) -> Option<bool> {
        self.as_uint().map(|v| !v.is_zero())
    }

    /// Decode return data as address
    pub fn as_address(&self) -> Option<H160> {
        if self.return_data.len() == 32 {
            let mut bytes = [0u8; 20];
            bytes.copy_from_slice(&self.return_data[12..32]);
            Some(H160::from(bytes))
        } else {
            None
        }
    }
}

/// Test runtime for executing compiled contracts
pub struct TestRuntime {
    db: InMemoryDB,
    contract_address: Address,
}

impl TestRuntime {
    /// Deploy a contract using its creation bytecode (deploy section + runtime section).
    ///
    /// Runs a CREATE transaction so the deploy section executes, the runtime is
    /// returned via CODECOPY+RETURN, and `revm` stores it as the contract's code.
    /// Subsequent `call()` invocations target the deployed runtime.
    pub fn new(creation_bytecode: &[u8]) -> Self {
        let mut db = InMemoryDB::default();

        // Fund the deployer so it can pay for gas.
        let mut deployer_bytes = [0u8; 20];
        deployer_bytes[19] = 0x01;
        let deployer = Address::from(deployer_bytes);
        db.insert_account_info(
            deployer,
            AccountInfo {
                balance: revm::primitives::U256::from(u64::MAX),
                nonce: 0,
                code_hash: KECCAK_EMPTY,
                code: None,
            },
        );

        // Execute a CREATE transaction with the creation bytecode.
        let contract_address = {
            let mut evm = Evm::builder()
                .with_db(&mut db)
                .modify_tx_env(|tx| {
                    tx.caller = deployer;
                    tx.transact_to = TransactTo::Create;
                    tx.data = creation_bytecode.to_vec().into();
                    tx.value = revm::primitives::U256::ZERO;
                    tx.gas_limit = 10_000_000;
                    tx.nonce = Some(0);
                })
                .build();

            let result = evm.transact().expect("CREATE transaction failed");
            evm.context.evm.db.commit(result.state.clone());

            match result.result {
                ExecutionResult::Success { output, .. } => match output {
                    Output::Create(_, Some(addr)) => addr,
                    other => panic!("Contract deployment returned unexpected output: {:?}", other),
                },
                ExecutionResult::Revert { output, .. } => {
                    panic!("Contract deployment reverted: {:?}", output)
                }
                ExecutionResult::Halt { reason, .. } => {
                    panic!("Contract deployment halted: {:?}", reason)
                }
            }
        }; // `evm` is dropped here, releasing the &mut db borrow

        Self {
            db,
            contract_address,
        }
    }

    /// Call a function by selector with encoded arguments
    pub fn call(&mut self, calldata: Vec<u8>) -> CallResult {
        // Create caller address
        let mut caller_bytes = [0u8; 20];
        caller_bytes[18] = 0x99; // 0x0000...9999
        caller_bytes[19] = 0x99;
        let caller_address = Address::from(caller_bytes);

        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller_address;
                tx.transact_to = TransactTo::Call(self.contract_address);
                tx.data = calldata.into();
                tx.value = revm::primitives::U256::from(0);
                tx.gas_limit = 10_000_000;
            })
            .build();

        let result = evm.transact().unwrap();

        // Commit state changes to the database
        // This is essential for storage writes (SSTORE) to persist
        evm.context.evm.db.commit(result.state.clone());

        match result.result {
            ExecutionResult::Success {
                output, gas_used, ..
            } => match output {
                Output::Call(data) => CallResult {
                    success: true,
                    return_data: data.to_vec(),
                    gas_used,
                },
                _ => CallResult {
                    success: true,
                    return_data: vec![],
                    gas_used,
                },
            },
            ExecutionResult::Revert { output, gas_used } => CallResult {
                success: false,
                return_data: output.to_vec(),
                gas_used,
            },
            ExecutionResult::Halt { reason, gas_used } => {
                println!("Halted: {:?}", reason);
                CallResult {
                    success: false,
                    return_data: vec![],
                    gas_used,
                }
            }
        }
    }

    /// Read a storage slot
    pub fn read_storage(&mut self, slot: U256) -> U256 {
        // Convert primitive_types::U256 to revm::primitives::U256
        let revm_slot = revm::primitives::U256::from_limbs(slot.0);
        let revm_value = self
            .db
            .storage(self.contract_address, revm_slot)
            .unwrap_or_default();
        // Convert back to primitive_types::U256
        U256(revm_value.into_limbs())
    }

    /// Get contract address
    pub fn contract_address(&self) -> Address {
        self.contract_address
    }
}

/// Encode a function selector from signature
pub fn encode_selector(signature: &str) -> [u8; 4] {
    use tiny_keccak::{Hasher, Keccak};

    let mut keccak = Keccak::v256();
    keccak.update(signature.as_bytes());
    let mut hash = [0u8; 32];
    keccak.finalize(&mut hash);

    let mut selector = [0u8; 4];
    selector.copy_from_slice(&hash[0..4]);
    selector
}

/// Encode calldata for a function call with uint256 arguments
pub fn encode_call_uint(signature: &str, args: &[U256]) -> Vec<u8> {
    let selector = encode_selector(signature);
    let mut calldata = selector.to_vec();

    for arg in args {
        let mut bytes = [0u8; 32];
        // Convert U256 to big endian bytes
        for i in 0..4 {
            let limb_bytes = arg.0[3 - i].to_be_bytes();
            bytes[i * 8..(i + 1) * 8].copy_from_slice(&limb_bytes);
        }
        calldata.extend_from_slice(&bytes);
    }

    calldata
}

/// Encode calldata for a function call with no arguments
pub fn encode_call_no_args(signature: &str) -> Vec<u8> {
    encode_selector(signature).to_vec()
}
