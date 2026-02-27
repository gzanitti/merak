/// Instruction translation to EVM opcodes
use crate::analysis::stack_allocator::{SlotLocation, StackAllocation};
use crate::codegen::{functions, storage_ops, FunctionCallMap};
use crate::evm::{BytecodeBuilder, CodegenError, Opcode};
use crate::lowering::{LoweredInstruction, LoweredOperand, StackSlot};
use merak_ast::expression::{BinaryOperator, UnaryOperator};
use merak_ir::ssa_ir::Constant;

/// Stack state tracker for managing EVM memory offsets during codegen.
pub struct StackState {
    /// Base memory address for this function's register file.
    frame_base: u64,
}

impl StackState {
    pub fn with_frame_base(frame_base: u64) -> Self {
        Self { frame_base }
    }

    pub fn memory_offset_for_slot(&self, slot: usize) -> u64 {
        self.frame_base + (slot as u64 * 32)
    }
}

impl Default for StackState {
    fn default() -> Self {
        Self::with_frame_base(0x80)
    }
}

/// Translate a lowered instruction to EVM bytecode.
///
/// `entry_layout`: StackPos slots at block entry (from predecessor shuffle).
/// `allocation`: global slot location assignments for this function.
/// `block_stack`: StackPos defs made so far in this block, TOS-first (mutated).
pub fn translate_instruction(
    inst: &LoweredInstruction,
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    function_metas: &FunctionCallMap,
    entry_layout: &[StackSlot],
    allocation: &StackAllocation,
    block_stack: &mut Vec<StackSlot>,
) -> Result<(), CodegenError> {
    match inst {
        LoweredInstruction::BinaryOp {
            dest,
            op,
            left,
            right,
        } => {
            // EVM SUB: TOS − second.  Push right first (extra_depth=0), left second (extra_depth=1).
            push_operand(right, builder, stack_state, entry_layout, block_stack, 0)?;
            push_operand(left, builder, stack_state, entry_layout, block_stack, 1)?;
            match op {
                BinaryOperator::LessEqual => {
                    builder.emit(Opcode::GT);
                    builder.emit(Opcode::ISZERO);
                }
                BinaryOperator::GreaterEqual => {
                    builder.emit(Opcode::LT);
                    builder.emit(Opcode::ISZERO);
                }
                BinaryOperator::NotEqual => {
                    builder.emit(Opcode::EQ);
                    builder.emit(Opcode::ISZERO);
                }
                _ => {
                    builder.emit(binary_op_to_opcode(op)?);
                }
            }
            store_to_slot(
                *dest,
                builder,
                stack_state,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }

        LoweredInstruction::UnaryOp { dest, op, operand } => {
            match op {
                UnaryOperator::Negate => {
                    builder.push(&[0]);
                    push_operand(operand, builder, stack_state, entry_layout, block_stack, 1)?;
                    builder.emit(Opcode::SUB);
                }
                _ => {
                    push_operand(operand, builder, stack_state, entry_layout, block_stack, 0)?;
                    builder.emit(unary_op_to_opcode(op)?);
                }
            }
            store_to_slot(
                *dest,
                builder,
                stack_state,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }

        LoweredInstruction::LoadConstant { dest, value } => {
            push_constant(value, builder)?;
            store_to_slot(
                *dest,
                builder,
                stack_state,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }

        LoweredInstruction::Copy { dest, source } => {
            push_operand(source, builder, stack_state, entry_layout, block_stack, 0)?;
            store_to_slot(
                *dest,
                builder,
                stack_state,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }

        LoweredInstruction::StorageLoad { dest, storage_slot } => {
            storage_ops::translate_storage_load(
                *dest,
                *storage_slot,
                builder,
                stack_state,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }

        LoweredInstruction::StorageStore {
            storage_slot,
            value,
        } => storage_ops::translate_storage_store(
            *storage_slot,
            value.clone(),
            builder,
            stack_state,
            entry_layout,
            block_stack,
        ),

        LoweredInstruction::Call { dest, target, args } => {
            functions::translate_call(
                dest.as_ref(),
                target,
                args,
                builder,
                stack_state,
                function_metas,
                entry_layout,
                block_stack,
                allocation,
            )?;
            Ok(())
        }
    }
}

/// Push an operand onto the EVM stack.
///
/// Search order (TOS-closest first):
/// 1. `block_stack[i]`  → `DUP(i + 1 + extra_depth)`
/// 2. `entry_layout[i]` → `DUP(block_stack.len() + i + 1 + extra_depth)`
/// 3. Neither           → Memory slot → `PUSH offset + MLOAD`
/// 4. Constant          → `PUSH constant`
pub fn push_operand(
    operand: &LoweredOperand,
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    entry_layout: &[StackSlot],
    block_stack: &[StackSlot],
    extra_depth: usize,
) -> Result<(), CodegenError> {
    match operand {
        LoweredOperand::Constant(c) => push_constant(c, builder),
        LoweredOperand::Location(slot) => {
            if let Some(i) = block_stack.iter().position(|s| s == slot) {
                let pos = i + 1 + extra_depth;
                builder.emit(Opcode::dup_for_position(pos).expect("≤ 16 slots"));
                return Ok(());
            }
            if let Some(i) = entry_layout.iter().position(|s| s == slot) {
                let pos = block_stack.len() + i + 1 + extra_depth;
                builder.emit(Opcode::dup_for_position(pos).expect("≤ 16 slots"));
                return Ok(());
            }
            // Memory slot.
            let offset = stack_state.memory_offset_for_slot(slot.0);
            builder.push_u64(offset);
            builder.emit(Opcode::MLOAD);
            Ok(())
        }
    }
}

/// Store the value at TOS to `dest`, dispatching on its `SlotLocation`.
///
/// At call time TOS holds the new value (1 item above block_stack base).
///
/// | dest location                 | emitted opcodes                              |
/// |-------------------------------|----------------------------------------------|
/// | StackPos, in block_stack[d]   | SWAP(d+2), POP  (depth of old = d+2)        |
/// | StackPos, in entry_layout[e]  | SWAP(bs+e+2), POP  (bs = block_stack.len()) |
/// | StackPos, new first def       | nothing emitted; block_stack.insert(0, dest)|
/// | Memory / unknown              | PUSH offset, MSTORE                          |
///
/// EVM SWAPk swaps TOS (depth 1) with depth k+1.
/// "Old dest at depth D" → SWAP(D-1).
pub fn store_to_slot(
    dest: StackSlot,
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    entry_layout: &[StackSlot],
    block_stack: &mut Vec<StackSlot>,
    allocation: &StackAllocation,
) -> Result<(), CodegenError> {
    match allocation.assignment.get(&dest) {
        Some(SlotLocation::StackPos(_)) => {
            let bs = block_stack.len();
            if let Some(d) = block_stack.iter().position(|s| *s == dest) {
                // Old value at depth d+2 (1 for TOS result + d+1 for block_stack position).
                // SWAP(d+1) swaps TOS with depth d+2.
                builder.emit(Opcode::swap_for_position(d + 1).expect("≤ 16 slots"));
                builder.emit(Opcode::POP);
            } else if let Some(e) = entry_layout.iter().position(|s| *s == dest) {
                // Old value at depth bs+e+2.  SWAP(bs+e+1).
                builder.emit(Opcode::swap_for_position(bs + e + 1).expect("≤ 16 slots"));
                builder.emit(Opcode::POP);
            } else {
                // New StackPos def: value already at TOS; just record it.
                block_stack.insert(0, dest);
            }
            Ok(())
        }
        _ => {
            // Memory slot: PUSH offset + MSTORE.
            let offset = stack_state.memory_offset_for_slot(dest.0);
            builder.push_u64(offset);
            builder.emit(Opcode::MSTORE);
            Ok(())
        }
    }
}

pub fn push_constant(
    constant: &Constant,
    builder: &mut BytecodeBuilder,
) -> Result<(), CodegenError> {
    match constant {
        Constant::Int(value) => {
            let mut bytes = [0u8; 32];
            bytes[24..32].copy_from_slice(&value.to_be_bytes());
            builder.push_u256(&bytes);
            Ok(())
        }
        Constant::Bool(value) => {
            builder.push(&[if *value { 1 } else { 0 }]);
            Ok(())
        }
        Constant::Address(addr) => {
            builder.push_u256(&addr.0);
            Ok(())
        }
        Constant::String(_) => Err(CodegenError::Other(
            "String constants not yet supported".to_string(),
        )),
    }
}

fn binary_op_to_opcode(op: &BinaryOperator) -> Result<Opcode, CodegenError> {
    match op {
        BinaryOperator::Add => Ok(Opcode::ADD),
        BinaryOperator::Subtract => Ok(Opcode::SUB),
        BinaryOperator::Multiply => Ok(Opcode::MUL),
        BinaryOperator::Divide => Ok(Opcode::DIV),
        BinaryOperator::Modulo => Ok(Opcode::MOD),
        BinaryOperator::Less => Ok(Opcode::LT),
        BinaryOperator::Greater => Ok(Opcode::GT),
        BinaryOperator::Equal => Ok(Opcode::EQ),
        BinaryOperator::LessEqual => Ok(Opcode::GT),
        BinaryOperator::GreaterEqual => Ok(Opcode::LT),
        BinaryOperator::NotEqual => Ok(Opcode::EQ),
        BinaryOperator::LogicalAnd => Ok(Opcode::AND),
        BinaryOperator::LogicalOr => Ok(Opcode::OR),
    }
}

fn unary_op_to_opcode(op: &UnaryOperator) -> Result<Opcode, CodegenError> {
    match op {
        UnaryOperator::Not => Ok(Opcode::ISZERO),
        UnaryOperator::Negate => Err(CodegenError::Other(
            "Negation requires special handling (0 - x)".to_string(),
        )),
    }
}
