/// Type and register lowering pass
///
/// Converts Merak refinement types to simple EVM types and assigns stack slots
/// to all SSA registers in a single traversal:
/// - {x: int | ...} → Uint256
/// - {addr: address | ...} → Address
/// - {b: bool | ...} → Bool
///
/// Slot assignment is deterministic (first-seen order: parameters first,
/// then instruction destinations in block/instruction order).
use merak_ast::types::BaseType;
use merak_ir::ssa_ir::Register;
use merak_symbols::SymbolTable;
use std::collections::HashMap;

use super::{EvmType, StackSlot};
use crate::evm::CodegenError;
use merak_ir::ssa_ir::SsaCfg;

pub fn lower_registers(
    cfg: &SsaCfg,
    symbol_table: &SymbolTable,
) -> Result<(HashMap<Register, EvmType>, HashMap<Register, StackSlot>), CodegenError> {
    let mut type_map: HashMap<Register, EvmType> = HashMap::new();
    let mut allocation: HashMap<Register, StackSlot> = HashMap::new();

    // Parameters first (version 0)
    for param_id in &cfg.parameters {
        let symbol_info = symbol_table.get_symbol(param_id);
        if let Some(ty) = &symbol_info.ty {
            let evm_type = base_type_to_evm_type(&ty.base)?;
            let reg = Register {
                symbol: param_id.clone(),
                version: 0,
            };
            let slot = StackSlot::new(allocation.len());
            allocation.insert(reg.clone(), slot);
            type_map.insert(reg, evm_type);
        }
    }

    // Instruction destinations (skip already-seen registers)
    for block in cfg.blocks.values() {
        for instruction in &block.instructions {
            if let Some(dest) = instruction.dest_register() {
                if allocation.contains_key(&dest) {
                    continue;
                }
                let base_type = if dest.symbol.is_temp() {
                    cfg.local_temps.get(&dest.symbol).ok_or_else(|| {
                        CodegenError::Other(format!(
                            "Type not found for temporary: {}",
                            dest.symbol
                        ))
                    })?
                } else {
                    let symbol_info = symbol_table.get_symbol(&dest.symbol);
                    &symbol_info
                        .ty
                        .as_ref()
                        .ok_or_else(|| {
                            CodegenError::Other(format!("Symbol {} has no type", dest.symbol))
                        })?
                        .base
                };
                let evm_type = base_type_to_evm_type(base_type)?;
                let slot = StackSlot::new(allocation.len());
                allocation.insert(dest.clone(), slot);
                type_map.insert(dest, evm_type);
            }
        }
    }

    Ok((type_map, allocation))
}

/// Convert base type to EVM type
pub fn base_type_to_evm_type(base_type: &BaseType) -> Result<EvmType, CodegenError> {
    match base_type {
        BaseType::Int => Ok(EvmType::Uint256),
        BaseType::Address => Ok(EvmType::Address),
        BaseType::Bool => Ok(EvmType::Bool),
        // TODO: Handle other types
        _ => Err(CodegenError::Other(format!(
            "Unsupported type for EVM codegen: {:?}",
            base_type
        ))),
    }
}
