// Common test utilities for SSA transformation tests

use indexmap::IndexMap;
use merak_analyzer::analyze;
use merak_ast::contract::Program;
use merak_ast::NodeIdGenerator;
use merak_parser::parse_program;
use merak_symbols::SymbolTable;

use merak_ir::ssa_ir::{BasicBlock, BlockId, SsaCfg, SsaInstruction, SsaProgram};
use merak_ir::transformers::ssa::SsaBuilder;

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

/// Helper for tests with a single function - returns the first function
pub fn get_single_function_cfg<'a>(
    program: &'a SsaProgram,
    contract_name: &str,
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
pub fn find_block_with<F>(cfg: &SsaCfg, predicate: F) -> Option<&BasicBlock<SsaInstruction>>
where
    F: Fn(&BasicBlock<SsaInstruction>) -> bool,
{
    cfg.blocks.values().find(|block| predicate(block))
}

/// Counts instructions of a specific type in a block
pub fn count_instructions_of_type<F>(block: &BasicBlock<SsaInstruction>, predicate: F) -> usize
where
    F: Fn(&SsaInstruction) -> bool,
{
    block.instructions.iter().filter(|i| predicate(i)).count()
}

/// Asserts that a block has exactly the expected number of predecessors
pub fn assert_predecessors(block: &BasicBlock<SsaInstruction>, expected: usize) {
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
pub fn assert_successors(block: &BasicBlock<SsaInstruction>, expected: usize) {
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
pub fn get_phi_nodes(block: &BasicBlock<SsaInstruction>) -> Vec<&SsaInstruction> {
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
