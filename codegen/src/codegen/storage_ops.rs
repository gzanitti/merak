/// Storage operation translation
use crate::analysis::stack_allocator::StackAllocation;
use crate::codegen::instructions::{push_operand, store_to_slot, StackState};
use crate::evm::{BytecodeBuilder, CodegenError, Opcode};
use crate::lowering::{LoweredOperand, StackSlot};

/// Translate storage load: dest = storage[slot]
pub fn translate_storage_load(
    dest: StackSlot,
    storage_slot: u64,
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    entry_layout: &[StackSlot],
    block_stack: &mut Vec<StackSlot>,
    allocation: &StackAllocation,
) -> Result<(), CodegenError> {
    builder.push_u64(storage_slot);
    builder.emit(Opcode::SLOAD);
    store_to_slot(
        dest,
        builder,
        stack_state,
        entry_layout,
        block_stack,
        allocation,
    )
}

/// Translate storage store: storage[slot] = value
pub fn translate_storage_store(
    storage_slot: u64,
    value: LoweredOperand,
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    entry_layout: &[StackSlot],
    block_stack: &[StackSlot],
) -> Result<(), CodegenError> {
    push_operand(&value, builder, stack_state, entry_layout, block_stack, 0)?;
    builder.push_u64(storage_slot);
    builder.emit(Opcode::SSTORE);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::stack_allocator::{SlotLocation, StackAllocation};
    use crate::evm::BytecodeBuilder;
    use std::collections::HashMap;

    fn memory_allocation(slot: StackSlot) -> StackAllocation {
        let mut assignment = HashMap::new();
        assignment.insert(slot, SlotLocation::Memory);
        StackAllocation {
            assignment,
            entry_layout: HashMap::new(),
            exit_layout: HashMap::new(),
        }
    }

    #[test]
    fn test_storage_load_basic() {
        let mut builder = BytecodeBuilder::new();
        let dest = StackSlot::new(0);
        let stack_state = StackState::with_frame_base(0x80);
        let allocation = memory_allocation(dest);
        translate_storage_load(
            dest,
            5,
            &mut builder,
            &stack_state,
            &[],
            &mut vec![],
            &allocation,
        )
        .unwrap();
        assert!(builder.code().contains(&Opcode::SLOAD.encode()));
    }

    #[test]
    fn test_storage_store_basic() {
        let mut builder = BytecodeBuilder::new();
        let value = LoweredOperand::Location(StackSlot::new(0));
        let stack_state = StackState::with_frame_base(0x80);
        translate_storage_store(7, value, &mut builder, &stack_state, &[], &[]).unwrap();
        assert!(builder.code().contains(&Opcode::SSTORE.encode()));
    }

    #[test]
    fn test_storage_store_dup_when_preloaded() {
        let mut builder = BytecodeBuilder::new();
        let slot = StackSlot::new(3);
        let value = LoweredOperand::Location(slot);
        let stack_state = StackState::with_frame_base(0x80);
        let entry_layout = [slot];
        translate_storage_store(7, value, &mut builder, &stack_state, &entry_layout, &[]).unwrap();
        assert!(builder.code().contains(&Opcode::DUP1.encode()));
        assert!(builder.code().contains(&Opcode::SSTORE.encode()));
    }
}
