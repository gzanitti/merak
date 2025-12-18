// Common test utilities for SSA transformation tests

use indexmap::IndexMap;
use merak_analyzer::analyze;
use merak_analyzer::storage::analyze_storage;
use merak_ast::contract::Program;
use merak_ast::NodeIdGenerator;
use merak_parser::parse_program;
use merak_symbols::SymbolTable;

use merak_ir::ssa_ir::{BasicBlock, BlockId, SsaCfg, SsaInstruction, SsaProgram};
use merak_ir::transformers::ssa::SsaBuilder;

use merak_analyzer::refinements::inference::LiquidInferenceEngine;

/// Builds SSA IR from Merak source code
///
/// Returns both the SSA program and symbol table for tests that need symbol information
pub fn build_ssa_from_source(source: &str) -> Result<(SsaProgram, SymbolTable), String> {
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
    let ssa_program = ssa_builder
        .build(&program)
        .map_err(|e| format!("SSA build error: {:?}", e))?;

    Ok((ssa_program, symbol_table))
}

/// Extracts a specific function's CFG by name
pub fn get_function_cfg<'a>(
    program: &'a SsaProgram,
    contract_name: &str,
    function_name: &str,
) -> &'a SsaCfg {
    let file = program
        .files
        .get(contract_name)
        .unwrap_or_else(|| panic!("Contract '{}' not found", contract_name));

    file.contract
        .functions
        .iter()
        .find(|cfg| cfg.name == function_name)
        .unwrap_or_else(|| {
            panic!(
                "Function '{}' not found. Available functions: {:?}",
                function_name,
                file.contract
                    .functions
                    .iter()
                    .map(|f| &f.name)
                    .collect::<Vec<_>>()
            )
        })
}

/// Helper for tests with a single function - returns the first function in a state
pub fn get_single_function_cfg<'a>(
    program: &'a SsaProgram,
    contract_name: &str,
    state_name: &str,
) -> &'a SsaCfg {
    let file = program
        .files
        .get(contract_name)
        .unwrap_or_else(|| panic!("Contract '{}' not found", contract_name));

    file.contract
        .functions
        .first()
        .unwrap_or_else(|| panic!("No functions found in contract '{}'", contract_name))
}

/// Finds a block that matches a predicate
pub fn find_block_with<F>(cfg: &SsaCfg, predicate: F) -> Option<&BasicBlock>
where
    F: Fn(&BasicBlock) -> bool,
{
    cfg.blocks.values().find(|block| predicate(block))
}

/// Counts instructions of a specific type in a block
pub fn count_instructions_of_type<F>(block: &BasicBlock, predicate: F) -> usize
where
    F: Fn(&SsaInstruction) -> bool,
{
    block.instructions.iter().filter(|i| predicate(i)).count()
}

/// Asserts that a block has exactly the expected number of predecessors
pub fn assert_predecessors(block: &BasicBlock, expected: usize) {
    assert_eq!(
        block.predecessors.len(),
        expected,
        "Block bb{} should have {} predecessors, but has {}. Predecessors: {:?}",
        block.id,
        expected,
        block.predecessors.len(),
        block.predecessors
    );
}

/// Asserts that a block has exactly the expected number of successors
pub fn assert_successors(block: &BasicBlock, expected: usize) {
    assert_eq!(
        block.successors.len(),
        expected,
        "Block bb{} should have {} successors, but has {}. Successors: {:?}",
        block.id,
        expected,
        block.successors.len(),
        block.successors
    );
}

/// Extracts all phi nodes from a block
pub fn get_phi_nodes(block: &BasicBlock) -> Vec<&SsaInstruction> {
    block
        .instructions
        .iter()
        .filter(|i| matches!(i, SsaInstruction::Phi { .. }))
        .collect()
}

/// Asserts that one block dominates another
///
/// Requires that dominance analysis has been run on the CFG
pub fn assert_dominates(cfg: &SsaCfg, dominator: BlockId, dominated: BlockId) {
    let dominance = cfg
        .dominance
        .as_ref()
        .expect("Dominance analysis not run on CFG");

    assert!(
        dominance.dominates(dominator, dominated),
        "Block bb{} does not dominate bb{}",
        dominator,
        dominated
    );
}

pub fn load_test_contracts(
    contracts: Vec<(&str, &str)>,
) -> Result<Program, merak_errors::MerakError> {
    use std::collections::HashMap;
    use std::fs;
    use tempfile::TempDir;

    // Create a temporary directory
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Write all contracts to files
    let mut file_paths = Vec::new();
    for (name, source) in &contracts {
        let file_path = temp_path.join(format!("{}.merak", name));
        fs::write(&file_path, source).expect("Failed to write contract file");
        file_paths.push(file_path);
    }

    // Load the first contract as entry point (which will load imports recursively)
    let entry_path = &file_paths[0];
    let mut loaded = Program {
        files: IndexMap::new(),
    };
    let mut visited = HashMap::new();
    let id_gen = NodeIdGenerator::new();
    merak::load_recursive(entry_path, &mut loaded, &mut visited, None, &id_gen)?;

    Ok(loaded)
}

/// Builds SSA IR from source and runs storage analysis
pub fn build_ssa_with_storage(source: &str) -> Result<(SsaProgram, SymbolTable), String> {
    // First build SSA using existing helper
    let (mut program, symbols) = build_ssa_from_source(source)?;

    for file in program.files.values_mut() {
        // Avoid overlapping mutable borrows by iterating via indices
        let len = file.contract.functions.len();
        for i in 0..len {
            // Split the mutable borrows: take contract and the i-th function separately
            let contract_ptr: *mut _ = &mut file.contract;
            // Safe because we do not use other references to contract while cfg is used
            let cfg = &mut file.contract.functions[i];
            // Reborrow contract through the raw pointer just for this call
            let contract = unsafe { &mut *contract_ptr };
            analyze_storage(contract, cfg, &symbols)
                .map_err(|e| format!("Storage analysis error: {:?}", e))?;
        }
    }

    Ok((program, symbols))
}

pub fn load_test_contracts_with_storage(
    contracts: Vec<(&str, &str)>,
) -> Result<(SsaProgram, SymbolTable), String> {
    let program = load_test_contracts(contracts).expect("Failed to build SSA");

    let symbol_table = analyze(&program).map_err(|e| format!("Analysis error: {:?}", e))?;

    // Build SSA IR
    let mut ssa_builder = SsaBuilder::new(symbol_table.clone());
    let mut ssa_program = ssa_builder
        .build(&program)
        .map_err(|e| format!("SSA build error: {:?}", e))?;

    for file in ssa_program.files.values_mut() {
        // Avoid overlapping mutable borrows by iterating via indices
        let len = file.contract.functions.len();
        for i in 0..len {
            // Split the mutable borrows: take contract and the i-th function separately
            let contract_ptr: *mut _ = &mut file.contract;
            // Safe because we do not use other references to contract while cfg is used
            let cfg = &mut file.contract.functions[i];
            // Reborrow contract through the raw pointer just for this call
            let contract = unsafe { &mut *contract_ptr };
            analyze_storage(contract, cfg, &symbol_table)
                .map_err(|e| format!("Storage analysis error: {:?}", e))?;
        }
    }

    Ok((ssa_program, symbol_table))
}

pub fn run_refinement_inference(source: &str) -> Result<(SsaProgram, SymbolTable), String> {
    let (mut program, mut symbols) = build_ssa_with_storage(source)?;

    let mut lie = LiquidInferenceEngine::new(&mut symbols);
    for file in program.files.values_mut() {
        for cfg in &mut file.contract.functions {
            lie.infer_function(cfg)
                .map_err(|e| format!("Refinement inference error: {:?}", e))?;
        }
    }

    Ok((program, symbols))
}
