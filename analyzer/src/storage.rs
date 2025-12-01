use lazy_static::lazy_static;
use merak_ast::{function::Modifier, meta::SourceRef, types::Type};
use merak_errors::MerakError;
use merak_ir::ssa_ir::{BlockId, CallTarget, SsaCfg, SsaContract, SsaInstruction};
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};
use std::collections::{HashMap, HashSet};

lazy_static! {
    static ref EMPTY_STATE: StorageState = StorageState::empty();
}

pub struct StorageAnalysis {
    /// All storage locations in the contract
    pub locations: StorageLocationSet,

    /// Storage state at entry/exit of each basic block
    pub storage_states: StorageStateMap,

    /// Points where storage is invalidated (for warnings/diagnostics)
    pub invalidation_points: InvalidationPointSet,
}

pub fn analyze_storage(
    contract: &SsaContract,
    cfg: &SsaCfg,
    symbol_table: &SymbolTable,
) -> Result<StorageAnalysis, MerakError> {
    let locations = StorageLocationSet::from_contract(contract, symbol_table);

    let storage_states = compute_storage_states(cfg, &locations, symbol_table)?;

    let invalidation_points = detect_invalidation_points(cfg);

    verify_storage_safety(cfg, &invalidation_points, &locations, symbol_table)?;

    Ok(StorageAnalysis {
        locations,
        storage_states,
        invalidation_points,
    })
}

// TODO: Implementar en StorageStateMap
pub fn compute_storage_states(
    cfg: &SsaCfg,
    locations: &StorageLocationSet,
    symbol_table: &SymbolTable,
) -> Result<StorageStateMap, MerakError> {
    let mut storage_states = StorageStateMap::new();

    for (block_id, _) in &cfg.blocks {
        let entry_state = StorageState::empty();
        let exit_state = StorageState::empty();

        storage_states.set_entry_state(*block_id, entry_state);
        storage_states.update_exit_state(*block_id, exit_state);
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
            locations,
            &block.instructions,
            symbol_table,
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
    locations: &StorageLocationSet,
    instructions: &[SsaInstruction],
    symbol_table: &SymbolTable,
) -> StorageState {
    let mut new_entry_state = block_entry_state.clone();
    for instruction in instructions {
        match instruction {
            SsaInstruction::StorageLoad {
                dest: _,
                var,
                source_ref: _,
            } => {
                if !new_entry_state.is_unfolded(*var) {
                    new_entry_state.unfold(*var);
                }
            }
            SsaInstruction::StorageStore {
                var,
                value: _,
                source_ref: _,
            } => new_entry_state.fold(*var),
            SsaInstruction::Call {
                dest: _,
                target,
                args: _,
                source_ref: _,
            } => {
                // TODO: Unclear disambiguation between external and internal calls. Possible generation of bugs here.
                let symbol_id = match target {
                    CallTarget::Internal(symbol_id) => symbol_id,
                    CallTarget::External {
                        contract: _,
                        function,
                    } => function,
                };

                // TODO: This is possible wrong for now
                let symbol_info = symbol_table.get_symbol(*symbol_id);

                match &symbol_info.kind {
                    SymbolKind::Function { reentrancy, .. }
                    | SymbolKind::Entrypoint { reentrancy, .. } => match reentrancy {
                        Modifier::Guarded | Modifier::Checked => {
                            new_entry_state.invalidate_all_except_immutables(locations);
                        }
                        Modifier::Reentrant => {}

                        _ => unreachable!("Payable reentrancy during storage analysis"),
                    },
                    _ => panic!(
                        "Unsupported function kind for storage analysis: {:?}",
                        symbol_info.kind
                    ),
                }
            }
            _ => {}
        }
    }

    new_entry_state
}

fn detect_invalidation_points(cfg: &SsaCfg) -> InvalidationPointSet {
    let mut invalidation_points = InvalidationPointSet::new();

    // TODO: All loops are invalidation points for now
    // TODO: Next step: Internal calls (interprocedural analysis)
    cfg.loops
        .as_ref()
        .expect("Loop forest should have been planted")
        .loops
        .iter()
        .for_each(|(loop_id, loop_info)| {
            // TODO: Improve LoopInfo
            let point = InvalidationPoint {
                block_id: loop_info.header,
                kind: InvalidationKind::LoopInvalidation { loop_id: *loop_id },
                source_ref: SourceRef::unknown(),
            };
            invalidation_points.add(point);
        });

    for (block_id, block) in &cfg.blocks {
        for (inst_id, inst) in block.instructions.iter().enumerate() {
            match inst {
                SsaInstruction::Call {
                    dest: _,
                    target:
                        CallTarget::External {
                            contract: _,
                            function: _,
                        },
                    args: _,
                    source_ref,
                } => {
                    let point = InvalidationPoint {
                        block_id: *block_id,
                        kind: InvalidationKind::ExternalCall {
                            target: format!("FIX::ME"), // TODO: Fix this format!("{}::{}", contract, function),
                            instruction_index: inst_id,
                        },
                        source_ref: source_ref.clone(),
                    };
                    invalidation_points.add(point);
                }
                _ => {} // No invalidation
            }
        }
    }

    invalidation_points
}

fn verify_checks_effects_interactions(
    cfg: &SsaCfg,
    invalidation_points: &InvalidationPointSet,
) -> Result<(), MerakError> {
    for inv_point in invalidation_points.iter() {
        // Only check ExternalCall invalidations
        if let InvalidationKind::ExternalCall {
            instruction_index, ..
        } = &inv_point.kind
        {
            let block = cfg.blocks.get(&inv_point.block_id).unwrap();

            // Check for storage operations after the call in same block
            for instr in &block.instructions[instruction_index + 1..] {
                match instr {
                    SsaInstruction::StorageLoad {
                        var: _, source_ref, ..
                    } => {
                        return Err(MerakError::StorageAccessAfterExternalCall {
                            operation: "Read".to_string(),
                            location_name: "TODO: Fix SymbolId to location name".to_string(),
                            call_point: inv_point.source_ref.clone(),
                            access_point: source_ref.clone(),
                        });
                    }

                    SsaInstruction::StorageStore {
                        var: _, source_ref, ..
                    } => {
                        return Err(MerakError::StorageAccessAfterExternalCall {
                            operation: "Write".to_string(),
                            location_name: "TODO: Fix SymbolId to location name".to_string(),
                            call_point: inv_point.source_ref.clone(),
                            access_point: source_ref.clone(),
                        });
                    }

                    _ => continue,
                }
            }
        }
    }

    Ok(())
}

fn verify_storage_safety(
    cfg: &SsaCfg,
    invalidation_points: &InvalidationPointSet,
    locations: &StorageLocationSet,
    symbol_table: &SymbolTable,
) -> Result<(), MerakError> {
    // Get reentrancy mode
    let reentrancy = match &symbol_table.get_symbol(cfg.function_id).kind {
        SymbolKind::Function { reentrancy, .. } | SymbolKind::Entrypoint { reentrancy, .. } => {
            reentrancy
        }
        _ => unreachable!("Invalid SymbolKind derived from CFG function id"),
    };

    if reentrancy == &Modifier::Checked {
        match verify_checks_effects_interactions(cfg, invalidation_points) {
            Ok(()) => {}
            Err(e) => {
                panic!("Storage analysis error: {} (TODO: push Err up)", e);
            }
        }
    }

    // 2. Verify no writes to immutables
    for (_, block) in &cfg.blocks {
        for instr in &block.instructions {
            if let SsaInstruction::StorageStore {
                var, source_ref, ..
            } = instr
            {
                let location_info = locations.get(*var).unwrap();

                if location_info.mutability == StorageMutability::Immutable {
                    return Err(MerakError::WriteToImmutable {
                        location_name: location_info.var_name.clone(),
                        write_point: source_ref.clone(),
                    });
                }
            }
        }
    }

    Ok(())
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

#[derive(PartialEq)]
pub enum StorageMutability {
    Mutable,
    Immutable,
}

impl StorageLocationSet {
    pub fn from_contract(contract: &SsaContract, symbol_table: &SymbolTable) -> StorageLocationSet {
        let mut locations = HashMap::new();

        for state_var in &contract.data.variables {
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

        for state_const in &contract.data.constants {
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

pub struct StorageStateMap {
    /// For each basic block, the storage state at ENTRY
    block_entry_states: HashMap<BlockId, StorageState>,

    /// For each basic block, the storage state at EXIT
    block_exit_states: HashMap<BlockId, StorageState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageState {
    /// Locations that are unfolded
    unfolded: HashSet<SymbolId>,

    /// Locations that are explicitly invalidated
    /// (unfolded -> external call happened)
    invalidated: HashSet<SymbolId>,
}

impl StorageState {
    pub fn empty() -> Self {
        StorageState {
            unfolded: HashSet::new(),
            invalidated: HashSet::new(),
        }
    }

    /// The location was never unfolded in this path
    pub fn is_folded(&self, loc: SymbolId) -> bool {
        !self.unfolded.contains(&loc) && !self.invalidated.contains(&loc)
    }

    /// The location is unfolded and valid
    pub fn is_unfolded(&self, loc: SymbolId) -> bool {
        self.unfolded.contains(&loc) && !self.invalidated.contains(&loc)
    }

    /// The location was unfolded but then invalidated
    pub fn is_invalidated(&self, loc: SymbolId) -> bool {
        self.invalidated.contains(&loc)
    }

    /// Unfold a location
    pub fn unfold(&mut self, loc: SymbolId) {
        self.unfolded.insert(loc);
        self.invalidated.remove(&loc); // Clear invalidation
    }

    /// Fold a location (write it back)
    pub fn fold(&mut self, loc: SymbolId) {
        self.unfolded.remove(&loc);
        self.invalidated.remove(&loc);
    }

    /// Invalidate all locations (after external call)
    pub fn invalidate_all(&mut self) {
        // Move everything from unfolded to invalidated
        for loc in self.unfolded.drain() {
            self.invalidated.insert(loc);
        }
    }

    pub fn invalidate_all_except_immutables(&mut self, locations: &StorageLocationSet) {
        let to_invalidate: Vec<_> = self
            .unfolded
            .iter()
            .filter(|&&loc| {
                locations
                    .get(loc)
                    .map(|l| l.mutability == StorageMutability::Mutable)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        for loc in to_invalidate {
            self.invalidated.insert(loc);
        }
        self.unfolded.retain(|loc| {
            locations
                .get(*loc)
                .map(|l| l.mutability == StorageMutability::Immutable)
                .unwrap_or(false)
        });
    }

    /// Merge two states (for join points in CFG)
    pub fn merge(&self, other: &StorageState) -> StorageState {
        // Conservative: only unfolded if unfolded in BOTH paths
        let unfolded = self
            .unfolded
            .intersection(&other.unfolded)
            .copied()
            .collect();

        // Invalidated if invalidated in ANY path
        let invalidated = self
            .invalidated
            .union(&other.invalidated)
            .copied()
            .collect();

        StorageState {
            unfolded,
            invalidated,
        }
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

pub struct InvalidationPointSet {
    points: Vec<InvalidationPoint>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidationPoint {
    /// In which basic block it occurs
    pub block_id: BlockId,

    /// What type of invalidation it is
    pub kind: InvalidationKind,

    /// Source location for error messages
    pub source_ref: SourceRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvalidationKind {
    /// Call to external function (another contract)
    ExternalCall {
        /// What is being called
        target: String, // For diagnostics

        /// Index of the instruction within the block
        instruction_index: usize,
    },

    /// Call to own function that may make external calls
    /// (requires interprocedural analysis later)
    TransitiveCall { function_name: String },

    /// Loop that may contain external calls in its body
    /// (conservative: assume it iterates and calls)
    LoopInvalidation { loop_id: usize },
}

impl InvalidationPointSet {
    pub fn new() -> Self {
        InvalidationPointSet { points: Vec::new() }
    }

    pub fn add(&mut self, point: InvalidationPoint) {
        self.points.push(point);
    }

    pub fn iter(&self) -> impl Iterator<Item = &InvalidationPoint> {
        self.points.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}
