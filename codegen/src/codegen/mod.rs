/// Bytecode generation from lowered IR
///
/// This module translates lowered IR (after phi elimination and register
/// allocation) into EVM bytecode.

pub mod functions;
pub mod instructions;
pub mod stack_shuffler;
pub mod storage_ops;
pub mod terminators;

use crate::{Label, StackSlot};
use merak_symbols::SymbolId;
use std::collections::HashMap;

/// Metadata needed by a call site to invoke an internal function.
///
/// Built once in `compile_contract` and threaded through `translate_instruction`
/// so that `translate_call` can emit the correct argument stores and jump.
#[derive(Debug, Clone)]
pub struct FunctionCallInfo {
    /// JUMPDEST label at the start of the function body.
    pub entry_label: Label,
    /// Memory base for the callee's register file.
    pub frame_base: u64,
    /// Parameter slots, in source order.
    pub param_slots: Vec<StackSlot>,
}

/// Map from function SymbolId to its call info.
pub type FunctionCallMap = HashMap<SymbolId, FunctionCallInfo>;
