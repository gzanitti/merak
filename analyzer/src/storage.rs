use lazy_static::lazy_static;
use merak_ast::{function::Modifier, meta::SourceRef, types::Type};
use merak_errors::MerakError;
use merak_ir::ssa_ir::{BlockId, CallTarget, SsaCfg, SsaContract, SsaInstruction};
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};
use std::{collections::{HashMap, HashSet}, fmt::Debug};

lazy_static! {
    static ref EMPTY_STATE: StorageState = StorageState::empty();
}


/// Pre-computed analysis of which functions contain external calls
pub struct CallGraphAnalysis {
    /// Set of function IDs that contain external calls (directly or transitively)
    functions_with_external_calls: HashSet<SymbolId>,
}

impl CallGraphAnalysis {
    /// Analyzes the entire contract and builds the call graph
    pub fn analyze(contract: &SsaContract) -> Self {
        let mut analysis = CallGraphAnalysis {
            functions_with_external_calls: HashSet::new(),
        };
        
        // Analyze each function in the contract
        for cfg in &contract.functions {
            let mut visited = HashSet::new();
            if Self::contains_external_calls_recursive(
                cfg.function_id,
                contract,
                &mut visited,
            ) {
                analysis.functions_with_external_calls.insert(cfg.function_id);
            }
        }
        
        analysis
    }
    
    /// Query: Does this function contain external calls?
    pub fn contains_external_calls(&self, fn_id: SymbolId) -> bool {
        self.functions_with_external_calls.contains(&fn_id)
    }
    
    /// Recursive helper: Check if function contains external calls transitively
    fn contains_external_calls_recursive(
        fn_id: SymbolId,
        contract: &SsaContract,
        visited: &mut HashSet<SymbolId>,
    ) -> bool {
        // Prevent infinite recursion
        if visited.contains(&fn_id) {
            return false;
        }
        visited.insert(fn_id);
        
        // Find the function's CFG
        let cfg = contract.functions.iter()
            .find(|f| f.function_id == fn_id)
            .expect("Function must exist in contract");
        
        // Check all instructions in all blocks
        for block in cfg.blocks.values() {
            for instruction in &block.instructions {
                match instruction {
                    // Found direct external call
                    SsaInstruction::Call { 
                        target: CallTarget::External { .. }, 
                        .. 
                    } => {
                        return true;
                    }
                    
                    // Recurse into internal calls
                    SsaInstruction::Call { 
                        target: CallTarget::Internal(callee_id), 
                        .. 
                    } => {
                        // Check if the callee contains external calls
                        if Self::contains_external_calls_recursive(
                            *callee_id,
                            contract,
                            visited,
                        ) {
                            return true;
                        }
                    }
                    
                    _ => {}
                }
            }
        }
        
        false
    }
}

pub fn analyze_storage(
    contract: &SsaContract,
    cfg: &mut SsaCfg,
    symbol_table: &SymbolTable,
) -> Result<(), MerakError> {
    let locations = StorageLocationSet::from_contract(contract, symbol_table);

    let call_graph = CallGraphAnalysis::analyze(contract);

    let storage_states = compute_storage_states(cfg, symbol_table, &call_graph)?;
    
    insert_fold_unfold_instructions(cfg, &storage_states, &locations, symbol_table, &call_graph);
    
    validate_cei_pattern(cfg, &symbol_table)?;
    validate_immutability(cfg, &locations)?;
    
    Ok(())
}

// TODO: Implementar en StorageStateMap
pub fn compute_storage_states(
    cfg: &SsaCfg,
    symbol_table: &SymbolTable,
    call_graph: &CallGraphAnalysis,
) -> Result<StorageStateMap, MerakError> {
    let mut storage_states = StorageStateMap::new();

    for (block_id, _) in &cfg.blocks {
        storage_states.set_entry_state(*block_id, StorageState::empty());
        storage_states.update_exit_state(*block_id, StorageState::empty());
    }

    let mut worklist = vec![cfg.entry];

    while let Some(block_id) = worklist.pop() {
        let block = cfg.blocks.get(&block_id).unwrap();
        let mut block_entry_state = StorageState::empty();
        for predecessors in &block.predecessors {
            block_entry_state = block_entry_state.merge(storage_states.exit_state(*predecessors))
        }

        let block_exit_state = apply_transfer_function(
            &block_entry_state,
            &block.instructions,
            symbol_table,
            call_graph
        );

        storage_states.set_entry_state(block_id, block_entry_state);
        if storage_states.update_exit_state(block_id, block_exit_state) {
            worklist.extend(block.successors.clone());
        }
    }

    Ok(storage_states)
}

fn apply_transfer_function(
    block_entry_state: &StorageState,
    instructions: &[SsaInstruction],
    symbol_table: &SymbolTable,
    call_graph: &CallGraphAnalysis
) -> StorageState {
    let mut new_entry_state = block_entry_state.clone();
    for instruction in instructions {
        match instruction {
            SsaInstruction::StorageLoad {
                dest: _,
                var,
                source_ref: _,
            } => {
                new_entry_state.unfold(*var);
            }
            SsaInstruction::StorageStore {
                var,
                value: _,
                source_ref: _,
            } => new_entry_state.fold(*var),
            SsaInstruction::Call { target: CallTarget::External { .. }, .. } => {
                new_entry_state.fold_all();
            }
            SsaInstruction::Call {
                target: CallTarget::Internal(fn_id), ..
            } => {
                let symbol_info = symbol_table.get_symbol(*fn_id);
                //let reentrancy = get_reentrancy_modifier(*fn_id, symbol_table);

                // TODO: Check contract
                //*reentrancy == Modifier::Reentrant ||
                if call_graph.contains_external_calls(*fn_id) {
                    new_entry_state.fold_all();
                } 
            }
            _ => {}
        }
    }

    new_entry_state
}

fn insert_fold_unfold_instructions(
    cfg: &mut SsaCfg,
    state_map: &StorageStateMap,
    locations: &StorageLocationSet,
    symbol_table: &SymbolTable,
    call_graph: &CallGraphAnalysis,
) {
    for (block_id, block) in &mut cfg.blocks {
        let entry_state = state_map.entry_state(*block_id);
        let mut current_state = entry_state.clone();
        let mut new_instructions = Vec::new();
        
        for instruction in &block.instructions {
            match instruction {
                SsaInstruction::StorageLoad { dest, var, source_ref } => {
                    // If var is folded, insert unfold before load
                    if current_state.is_folded(*var) {
                        new_instructions.push(SsaInstruction::Unfold {
                            var: *var,
                            source_ref: source_ref.clone(),
                        });
                        current_state.unfold(*var);
                    }
                    
                    // Insert original load
                    new_instructions.push(instruction.clone());
                    // var stays unfolded
                }
                
                SsaInstruction::StorageStore { var, value, source_ref } => {
                    // If var is folded, insert unfold before store
                    if current_state.is_folded(*var) {
                        new_instructions.push(SsaInstruction::Unfold {
                            var: *var,
                            source_ref: source_ref.clone(),
                        });
                        current_state.unfold(*var);
                    }
                    
                    // Insert original store
                    new_instructions.push(instruction.clone());
                    
                    // Insert fold after store
                    new_instructions.push(SsaInstruction::Fold {
                        var: *var,
                        source_ref: source_ref.clone(),
                    });
                    current_state.fold(*var);
                }
                
                SsaInstruction::Call { 
                    dest, 
                    target: CallTarget::External { object, method }, 
                    args, 
                    source_ref 
                } => {
                    // PRE-CALL: Unfold all MUTABLE storage vars
                    for (var_id, location) in locations.iter() {
                        if location.mutability == StorageMutability::Mutable 
                           && current_state.is_folded(*var_id) {
                            new_instructions.push(SsaInstruction::Unfold {
                                var: *var_id,
                                source_ref: source_ref.clone(),
                            });
                            current_state.unfold(*var_id);
                        }
                    }
                    
                    // Insert the call
                    new_instructions.push(instruction.clone());
                    
                    // POST-CALL: Fold all UNFOLDED vars
                    for (var_id, location) in locations.iter() {
                        if location.mutability == StorageMutability::Mutable 
                           && current_state.is_unfolded(*var_id) {
                            new_instructions.push(SsaInstruction::Fold {
                                var: *var_id,
                                source_ref: source_ref.clone(),
                            });
                            current_state.fold(*var_id);
                        }
                    }
                }
                
                SsaInstruction::Call { 
                    target: CallTarget::Internal(fn_id), 
                    source_ref,
                    ..
                } => {
                    let reentrancy = get_reentrancy_modifier(*fn_id, symbol_table);

                    let needs_fold_unfold = *reentrancy == Modifier::Reentrant 
                        || call_graph.contains_external_calls(*fn_id);
                    
                    
                    if needs_fold_unfold {
                        // Same as external call
                        
                        // PRE-CALL: Unfold all mutable storage vars
                        for (var_id, location) in locations.iter() {
                            if location.mutability == StorageMutability::Mutable 
                               && current_state.is_folded(*var_id) {
                                new_instructions.push(SsaInstruction::Unfold {
                                    var: *var_id,
                                    source_ref: source_ref.clone(),
                                });
                                current_state.unfold(*var_id);
                            }
                        }
                        
                        // Insert the call
                        new_instructions.push(instruction.clone());
                        
                        // POST-CALL: Fold all unfolded vars
                        for (var_id, location) in locations.iter() {
                            if location.mutability == StorageMutability::Mutable 
                               && current_state.is_unfolded(*var_id) {
                                new_instructions.push(SsaInstruction::Fold {
                                    var: *var_id,
                                    source_ref: source_ref.clone(),
                                });
                                current_state.fold(*var_id);
                            }
                        }
                    } else {
                        // Guarded or Checked: just insert the call
                        new_instructions.push(instruction.clone());
                    }
                }
                
                _ => {
                    new_instructions.push(instruction.clone());
                }
            }
        }
        
        // CLEANUP: Fold any remaining unfolded vars at end of block
        for (var_id, _) in locations.iter() {
            if current_state.is_unfolded(*var_id) {
                new_instructions.push(SsaInstruction::Fold {
                    var: *var_id,
                    source_ref: SourceRef::unknown(),
                });
            }
        }
        
        // Replace block's instructions
        block.instructions = new_instructions;
    }
}

fn validate_cei_pattern(
    cfg: &SsaCfg,
    symbol_table: &SymbolTable,
) -> Result<(), MerakError> {
    // Only validate if the function is Checked (default)
    let reentrancy = get_reentrancy_modifier(cfg.function_id, symbol_table);
    
    if *reentrancy != Modifier::Checked {
        // Reentrant: user handles it
        // Guarded: runtime guard handles it
        return Ok(());
    }
    
    // For Checked functions: validate CEI pattern
    // Find external calls and check no stores after them
    for (block_id, block) in &cfg.blocks {
        for (idx, instr) in block.instructions.iter().enumerate() {
            // Check external calls
            if let SsaInstruction::Call { target: CallTarget::External { .. }, source_ref, .. } = instr {
                check_no_stores_after_call(cfg, *block_id, idx, source_ref)?;
            }
            
            // Also check internal calls with Reentrant modifier
            if let SsaInstruction::Call { target: CallTarget::Internal(fn_id), source_ref, .. } = instr {
                let call_reentrancy = get_reentrancy_modifier(*fn_id, symbol_table);
                if *call_reentrancy == Modifier::Reentrant {
                    check_no_stores_after_call(cfg, *block_id, idx, source_ref)?;
                }
            }
        }
    }
    
    Ok(())
}

/// Recursively checks for StorageStore instructions after a call
/// by following the CFG successors.
///
/// - For the initial block: starts checking from instruction_index + 1 (right after the call)
/// - For successor blocks: starts checking from instruction 0
fn check_no_stores_after_call(
    cfg: &SsaCfg,
    block_id: BlockId,
    call_instruction_index: usize,
    call_source_ref: &SourceRef,
) -> Result<(), MerakError> {
    let mut visited = HashSet::new();
    let mut worklist = vec![(block_id, call_instruction_index + 1)];
    
    while let Some((current_block_id, start_idx)) = worklist.pop() {
        // Avoid infinite loops
        if visited.contains(&current_block_id) {
            continue;
        }
        visited.insert(current_block_id);
        
        let block = cfg.blocks.get(&current_block_id).unwrap();
        
        // Check instructions from start_idx onward
        for instr in &block.instructions[start_idx..] {
            if let SsaInstruction::StorageStore { var, source_ref, .. } = instr {
                // FOUND A WRITE AFTER CALL - ERROR!
                return Err(MerakError::StorageAccessAfterExternalCall {
                    operation: "write".to_string(),
                    location_name: format!("{:?}", var), 
                    access_point: source_ref.clone(),
                    call_point: call_source_ref.clone(),
                });
            }
        }
        
        // Follow all successors (start from index 0)
        for &successor_id in &block.successors {
            worklist.push((successor_id, 0));
        }
    }
    
    Ok(())
}

fn validate_immutability(
    cfg: &SsaCfg,
    locations: &StorageLocationSet,
) -> Result<(), MerakError> {
    for (block_id, block) in &cfg.blocks {
        for instr in &block.instructions {
            if let SsaInstruction::StorageStore { var, source_ref, .. } = instr {
                let location = locations.get(*var).unwrap();
                
                if location.mutability == StorageMutability::Immutable {
                    return Err(MerakError::WriteToImmutable {
                        location_name: location.var_name.clone(),
                        write_point: source_ref.clone(),
                    });
                }
            }
        }
    }
    
    Ok(())
}

fn get_reentrancy_modifier(fn_id: SymbolId, symbol_table: &SymbolTable) -> &Modifier {
    let symbol_info = symbol_table.get_symbol(fn_id);
    match &symbol_info.kind {
        SymbolKind::Function { reentrancy, .. } | SymbolKind::Entrypoint { reentrancy, .. } => reentrancy,
        _ => panic!("Unsupported function kind for storage analysis: {:?}", symbol_info.kind),
    }
}


pub struct StorageLocationSet {
    pub locations: HashMap<SymbolId, StorageLocation>,
}

pub struct StorageLocation {
    pub symbol: SymbolId,

    pub refined_type: Type,

    pub mutability: StorageMutability,

    // For error reporting
    pub var_name: String,
}

#[derive(Debug, PartialEq)]
pub enum StorageMutability {
    Mutable,
    Immutable,
}

impl StorageLocationSet {
    pub fn from_contract(contract: &SsaContract, symbol_table: &SymbolTable) -> StorageLocationSet {
        let mut locations = HashMap::new();

        for state_var in &contract.variables {
            let symbol_id = symbol_table.get_symbol_id_by_node_id(state_var.id).unwrap();
            let symbol_info = symbol_table.get_symbol_by_node_id(state_var.id).unwrap();
            locations.insert(
                symbol_id,
                StorageLocation {
                    symbol: symbol_id,
                    refined_type: symbol_info
                        .ty
                        .as_ref()
                        .expect("State vars should have a type")
                        .clone(),
                    mutability: StorageMutability::Mutable,
                    var_name: symbol_info.qualified_name.to_string(),
                },
            );
        }

        for state_const in &contract.constants {
            let symbol_id = symbol_table
                .get_symbol_id_by_node_id(state_const.id)
                .unwrap();
            let symbol_info = symbol_table.get_symbol_by_node_id(state_const.id).unwrap();
            locations.insert(
                symbol_id,
                StorageLocation {
                    symbol: symbol_id,
                    refined_type: symbol_info
                        .ty
                        .as_ref()
                        .expect("State consts should have a type")
                        .clone(),
                    mutability: StorageMutability::Immutable,
                    var_name: symbol_info.qualified_name.to_string(),
                },
            );
        }

        StorageLocationSet { locations }
    }

    pub fn get(&self, symbol_id: SymbolId) -> Option<&StorageLocation> {
        self.locations.get(&symbol_id)
    }

    pub fn is_storage(&self, symbol_id: SymbolId) -> bool {
        self.locations.contains_key(&symbol_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SymbolId, &StorageLocation)> {
        self.locations.iter()
    }
}

#[derive(Debug)]
pub struct StorageStateMap {
    /// For each basic block, the storage state at ENTRY
    block_entry_states: HashMap<BlockId, StorageState>,

    /// For each basic block, the storage state at EXIT
    block_exit_states: HashMap<BlockId, StorageState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageState {
    /// Variables currently in UNFOLDED state
    /// If var not in unfolded -> var is FOLDED
    unfolded: HashSet<SymbolId>,
}

impl StorageState {
    pub fn empty() -> Self {
        StorageState {
            unfolded: HashSet::new(),
        }
    }
    
    pub fn is_folded(&self, loc: SymbolId) -> bool {
        !self.unfolded.contains(&loc)
    }
    
    pub fn is_unfolded(&self, loc: SymbolId) -> bool {
        self.unfolded.contains(&loc)
    }
    
    pub fn unfold(&mut self, loc: SymbolId) {
        self.unfolded.insert(loc);
    }
    
    pub fn fold(&mut self, loc: SymbolId) {
        self.unfolded.remove(&loc);
    }
    
    pub fn unfold_all_mutable(&mut self, locations: &StorageLocationSet) {
        for (var_id, location) in locations.iter() {
            if location.mutability == StorageMutability::Mutable {
                self.unfolded.insert(*var_id);
            }
        }
    }
    
    pub fn fold_all(&mut self) {
        self.unfolded.clear();
    }
    
    /// Merge two states (at CFG join points)
    pub fn merge(&self, other: &StorageState) -> StorageState {
        // Conservative: only unfolded if unfolded in BOTH paths
        let unfolded = self.unfolded
            .intersection(&other.unfolded)
            .copied()
            .collect();
        
        StorageState { unfolded }
    }
}

impl StorageStateMap {
    pub fn new() -> Self {
        StorageStateMap {
            block_entry_states: HashMap::new(),
            block_exit_states: HashMap::new(),
        }
    }

    pub fn entry_state(&self, block_id: BlockId) -> &StorageState {
        self.block_entry_states
            .get(&block_id)
            .unwrap_or_else(|| &EMPTY_STATE)
    }

    pub fn exit_state(&self, block_id: BlockId) -> &StorageState {
        self.block_exit_states
            .get(&block_id)
            .unwrap_or_else(|| &EMPTY_STATE)
    }

    pub fn set_entry_state(&mut self, block_id: BlockId, state: StorageState) {
        self.block_entry_states.insert(block_id, state);
    }

    /// Update exit state, returns true if it changed
    pub fn update_exit_state(&mut self, block_id: BlockId, state: StorageState) -> bool {
        if let Some(existing) = self.block_exit_states.get(&block_id) {
            if existing == &state {
                return false; // No change
            }
        }
        self.block_exit_states.insert(block_id, state);
        true
    }
}