/// Terminator translation to EVM jumps
///
/// Uses `plan_shuffle` to transform the current EVM stack layout into the
/// successor block's canonical `entry_layout` before every jump.
///
/// At terminator time the EVM stack above the continuation is:
///   [ block_stack (TOS) | entry_layout ]
/// where `block_stack` holds StackPos defs made within this block and
/// `entry_layout` holds slots that were pre-loaded at block entry.
use crate::analysis::stack_allocator::StackAllocation;
use crate::codegen::instructions::{push_constant, StackState};
use crate::codegen::stack_shuffler::{emit_shuffle, plan_shuffle};
use crate::evm::{BytecodeBuilder, CodegenError, Label, Opcode};
use crate::lowering::{LoweredOperand, LoweredTerminator, StackSlot};
use merak_ir::ssa_ir::BlockId;
use std::collections::{HashMap, HashSet};

/// Translate a terminator to EVM bytecode.
///
/// `entry_layout`  — StackPos slots pre-loaded at this block's entry (TOS-first).
/// `block_stack`   — StackPos slots defined within this block still on the EVM stack (TOS-first).
/// `allocation`    — Full allocation, used to look up successor entry layouts.
///
/// The "current" EVM stack frame visible to the terminator is:
///   `block_stack ++ entry_layout`  (TOS-first).
pub fn translate_terminator(
    terminator: &LoweredTerminator,
    builder: &mut BytecodeBuilder,
    block_labels: &HashMap<BlockId, Label>,
    stack_state: &StackState,
    entry_layout: &[StackSlot],
    block_stack: &[StackSlot],
    allocation: &StackAllocation,
) -> Result<(), CodegenError> {
    let warm = HashSet::new();

    match terminator {
        LoweredTerminator::Jump { target } => {
            let label = get_label(block_labels, *target)?;
            let target_layout = allocation
                .entry_layout
                .get(target)
                .cloned()
                .unwrap_or_default();
            let current = build_current(block_stack, entry_layout);
            let ops = plan_shuffle(&current, &target_layout, &warm);
            emit_shuffle(&ops, builder, stack_state);
            builder.jump_to(label);
            Ok(())
        }

        LoweredTerminator::Branch {
            condition,
            then_block,
            else_block,
            ..
        } => {
            let then_label = get_label(block_labels, *then_block)?;
            let else_label = get_label(block_labels, *else_block)?;
            let then_layout = allocation
                .entry_layout
                .get(then_block)
                .cloned()
                .unwrap_or_default();
            let else_layout = allocation
                .entry_layout
                .get(else_block)
                .cloned()
                .unwrap_or_default();
            let current = build_current(block_stack, entry_layout);

            match condition {
                LoweredOperand::Location(slot) => {
                    // Include condition slot at TOS of the then-path target so that
                    // JUMPI can consume it.  After JUMPI the condition is gone and
                    // the stack equals then_layout.
                    let mut then_pre = vec![*slot];
                    then_pre.extend_from_slice(&then_layout);
                    let ops = plan_shuffle(&current, &then_pre, &warm);
                    emit_shuffle(&ops, builder, stack_state);
                    builder.jumpi_to(then_label);

                    // Fall-through (else path): stack = then_layout.
                    let current2 = slots_to_current(&then_layout);
                    let ops2 = plan_shuffle(&current2, &else_layout, &warm);
                    emit_shuffle(&ops2, builder, stack_state);
                    builder.jump_to(else_label);
                }
                LoweredOperand::Constant(c) => {
                    // Shuffle to then_layout first, then push constant for JUMPI.
                    let ops = plan_shuffle(&current, &then_layout, &warm);
                    emit_shuffle(&ops, builder, stack_state);
                    push_constant(c, builder)?;
                    builder.jumpi_to(then_label);

                    // Fall-through: stack = then_layout.
                    let current2 = slots_to_current(&then_layout);
                    let ops2 = plan_shuffle(&current2, &else_layout, &warm);
                    emit_shuffle(&ops2, builder, stack_state);
                    builder.jump_to(else_label);
                }
            }
            Ok(())
        }

        LoweredTerminator::Return { value, .. } => {
            let current = build_current(block_stack, entry_layout);
            match value {
                Some(LoweredOperand::Location(slot)) => {
                    // Shuffle so return value is at TOS; continuation is just below.
                    let ops = plan_shuffle(&current, &[*slot], &warm);
                    emit_shuffle(&ops, builder, stack_state);
                    // Stack: [return_val, continuation]. SWAP1+JUMP returns.
                    builder.emit(Opcode::SWAP1);
                    builder.emit(Opcode::JUMP);
                }
                Some(LoweredOperand::Constant(c)) => {
                    // Drain everything (leaves only continuation on stack).
                    let ops = plan_shuffle(&current, &[], &warm);
                    emit_shuffle(&ops, builder, stack_state);
                    push_constant(c, builder)?;
                    // Stack: [constant, continuation].
                    builder.emit(Opcode::SWAP1);
                    builder.emit(Opcode::JUMP);
                }
                None => {
                    // Drain everything; continuation is now TOS.
                    let ops = plan_shuffle(&current, &[], &warm);
                    emit_shuffle(&ops, builder, stack_state);
                    builder.emit(Opcode::JUMP);
                }
            }
            Ok(())
        }

        LoweredTerminator::Unreachable => {
            let current = build_current(block_stack, entry_layout);
            let ops = plan_shuffle(&current, &[], &warm);
            emit_shuffle(&ops, builder, stack_state);
            builder.emit(Opcode::INVALID);
            Ok(())
        }
    }
}

fn get_label(
    block_labels: &HashMap<BlockId, Label>,
    block_id: BlockId,
) -> Result<Label, CodegenError> {
    block_labels
        .get(&block_id)
        .copied()
        .ok_or_else(|| CodegenError::Other(format!("Label not found for block {:?}", block_id)))
}

/// Build the `current` slice for `plan_shuffle` from `block_stack ++ entry_layout`.
fn build_current(block_stack: &[StackSlot], entry_layout: &[StackSlot]) -> Vec<Option<StackSlot>> {
    block_stack
        .iter()
        .chain(entry_layout.iter())
        .map(|s| Some(*s))
        .collect()
}

/// Convert a plain `&[StackSlot]` to the `Option<StackSlot>` form used by `plan_shuffle`.
fn slots_to_current(slots: &[StackSlot]) -> Vec<Option<StackSlot>> {
    slots.iter().map(|s| Some(*s)).collect()
}
