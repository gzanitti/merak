/// Lowering passes and intermediate representations
///
/// This module contains the transformations from SSA IR to a stack-based
/// lowered IR suitable for EVM bytecode generation.
pub mod type_lowering;

use merak_ast::expression::{BinaryOperator, UnaryOperator};
use merak_ir::{
    ssa_ir::{BlockId, CallTarget, Constant, Operand, Register, Terminator},
    BasicBlock, ControlFlowGraph,
};
use merak_symbols::SymbolId;
use std::collections::HashMap;

use crate::layout::storage::StorageLayout;

/// Stack slot abstraction (result of register allocation)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StackSlot(pub usize);

impl StackSlot {
    pub fn new(id: usize) -> Self {
        StackSlot(id)
    }

    pub fn id(&self) -> usize {
        self.0
    }
}

/// EVM type (refinements and complex types erased)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EvmType {
    /// 256-bit unsigned integer (EVM native type)
    Uint256,
    /// 160-bit address
    Address,
    /// Boolean (represented as 0/1 in uint256)
    Bool,
    /// Bytes (dynamic length)
    Bytes,
    // TODO: Future types
    // Array(Box<EvmType>, Option<usize>),  // Dynamic or fixed-size arrays
    // Mapping(Box<EvmType>, Box<EvmType>),  // Mappings
    // Struct(Vec<(String, EvmType)>),  // Structs
}

impl EvmType {
    /// Size in storage slots (256-bit words)
    pub fn storage_size(&self) -> usize {
        match self {
            EvmType::Uint256 | EvmType::Address | EvmType::Bool => 1,
            EvmType::Bytes => 1, // Dynamic types store length + data
                                 // TODO: Arrays, mappings, structs have complex layouts
        }
    }

    /// Size in bytes (for memory layout)
    pub fn memory_size(&self) -> usize {
        match self {
            EvmType::Uint256 => 32,
            EvmType::Address => 32, // Padded to 32 bytes
            EvmType::Bool => 32,    // Padded to 32 bytes
            EvmType::Bytes => 32,   // Dynamic, stores length
        }
    }
}

/// Type alias for lowered basic blocks
///
/// Uses the generic BasicBlock with LoweredInstruction and LoweredTerminator.
/// The loop_invariants and loop_variants fields will be None (not used in lowered IR).
pub type LoweredBlock = BasicBlock<LoweredInstruction, LoweredTerminator>;

#[derive(Debug, Clone)]
pub struct LoweredCfg {
    pub name: String,
    pub function_id: SymbolId,
    pub blocks: HashMap<BlockId, LoweredBlock>,
    pub entry: BlockId,
    pub register_map: HashMap<Register, StackSlot>,
    pub storage_layout: StorageLayout,
    pub register_types: HashMap<Register, EvmType>,
    /// Function parameters (in order) - each parameter has its symbol ID and stack slot
    pub parameters: Vec<(SymbolId, StackSlot)>,
    /// Base memory address for this function's register file.
    /// Each slot n lives at `frame_base + n * 32`.
    /// Assigned at compile time so no two concurrently-live functions overlap.
    pub frame_base: u64,
    /// True for external/entrypoint functions (params arrive via calldata, stored in memory).
    /// False for internal functions (params arrive on the EVM stack from the caller).
    pub is_external: bool,
}

impl LoweredCfg {
    pub fn block(&self, id: BlockId) -> Option<&LoweredBlock> {
        self.blocks.get(&id)
    }

    pub fn block_mut(&mut self, id: BlockId) -> Option<&mut LoweredBlock> {
        self.blocks.get_mut(&id)
    }

    pub fn blocks_iter(&self) -> impl Iterator<Item = (&BlockId, &LoweredBlock)> {
        self.blocks.iter()
    }
}

/// Operand for lowered instructions (can be a stack slot or a constant)
/// This is a type alias for the generic Operand with StackSlot as the location type
pub type LoweredOperand = Operand<StackSlot>;

#[derive(Debug, Clone)]
pub enum LoweredInstruction {
    /// Binary operation: dest = left op right
    BinaryOp {
        dest: StackSlot,
        op: BinaryOperator,
        left: LoweredOperand,
        right: LoweredOperand,
    },

    /// Unary operation: dest = op operand
    UnaryOp {
        dest: StackSlot,
        op: UnaryOperator,
        operand: LoweredOperand,
    },

    /// Load from storage: dest = storage[slot]
    StorageLoad { dest: StackSlot, storage_slot: u64 },

    /// Store to storage: storage[slot] = value
    StorageStore {
        storage_slot: u64,
        value: LoweredOperand,
    },

    /// Function call: [dest =] target(args)
    Call {
        dest: Option<StackSlot>,
        target: CallTarget,
        args: Vec<LoweredOperand>,
    },

    /// Load constant: dest = constant (deprecated - use Copy with Constant operand instead)
    LoadConstant { dest: StackSlot, value: Constant },

    /// Copy: dest = source
    Copy {
        dest: StackSlot,
        source: LoweredOperand,
    },
    // TODO: Future instructions
    // MemoryLoad { dest: StackSlot, offset: StackSlot },
    // MemoryStore { offset: StackSlot, value: StackSlot },
    // Assert { condition: StackSlot, message: String },
}

/// Instruction trait implementation for Lowered IR
impl merak_ir::Instruction for LoweredInstruction {
    type Operand = LoweredOperand;
    type Destination = StackSlot;

    fn destination(&self) -> Option<Self::Destination> {
        match self {
            LoweredInstruction::Copy { dest, .. }
            | LoweredInstruction::BinaryOp { dest, .. }
            | LoweredInstruction::UnaryOp { dest, .. }
            | LoweredInstruction::StorageLoad { dest, .. }
            | LoweredInstruction::LoadConstant { dest, .. } => Some(*dest),
            LoweredInstruction::Call { dest, .. } => *dest,
            LoweredInstruction::StorageStore { .. } => None,
        }
    }

    fn operands(&self) -> Vec<&Self::Operand> {
        match self {
            LoweredInstruction::Copy { source, .. } => vec![source],
            LoweredInstruction::BinaryOp { left, right, .. } => vec![left, right],
            LoweredInstruction::UnaryOp { operand, .. } => vec![operand],
            LoweredInstruction::StorageStore { value, .. } => vec![value],
            LoweredInstruction::Call { args, .. } => args.iter().collect(),
            // No operands
            LoweredInstruction::StorageLoad { .. } | LoweredInstruction::LoadConstant { .. } => {
                vec![]
            }
        }
    }

    fn operands_mut(&mut self) -> Vec<&mut Self::Operand> {
        match self {
            LoweredInstruction::Copy { source, .. } => vec![source],
            LoweredInstruction::BinaryOp { left, right, .. } => vec![left, right],
            LoweredInstruction::UnaryOp { operand, .. } => vec![operand],
            LoweredInstruction::StorageStore { value, .. } => vec![value],
            LoweredInstruction::Call { args, .. } => args.iter_mut().collect(),
            // No operands
            LoweredInstruction::StorageLoad { .. } | LoweredInstruction::LoadConstant { .. } => {
                vec![]
            }
        }
    }

    fn is_verification_only(&self) -> bool {
        // Lowered IR has no verification-only instructions
        false
    }

    fn has_side_effects(&self) -> bool {
        matches!(
            self,
            LoweredInstruction::StorageStore { .. } | LoweredInstruction::Call { .. }
        )
    }
}

/// Block terminator after lowering
/// This is a type alias for the generic Terminator without metadata
pub type LoweredTerminator = Terminator<LoweredOperand, ()>;

impl ControlFlowGraph for LoweredCfg {
    fn entry(&self) -> BlockId {
        self.entry
    }

    fn successors(&self, block: BlockId) -> Vec<BlockId> {
        self.blocks[&block].successors.clone()
    }

    fn predecessors(&self, block: BlockId) -> Vec<BlockId> {
        self.blocks[&block].predecessors.clone()
    }

    fn block_ids(&self) -> Vec<BlockId> {
        self.blocks.keys().copied().collect()
    }
}
