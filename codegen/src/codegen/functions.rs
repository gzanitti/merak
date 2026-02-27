/// Function calling conventions for EVM
///
/// Arguments are passed on the EVM stack:
///
///   Caller pushes: arg0, arg1, …, argN (left-to-right, so argN is closest to TOS)
///   Caller pushes: call_continuation label (TOS)
///   Caller JUMPs to callee entry.
///
/// Stack at callee entry:
///   [ call_cont | argN | … | arg0 | caller_block_stack | caller_entry_layout | outer_cont ]
///     TOS
use crate::analysis::stack_allocator::StackAllocation;
use crate::codegen::instructions::{push_operand, store_to_slot, StackState};
use crate::codegen::FunctionCallMap;
use crate::evm::{BytecodeBuilder, CodegenError, Opcode};
use crate::lowering::{LoweredOperand, StackSlot};
use merak_ir::ssa_ir::CallTarget;

/// Translate a function call instruction to EVM bytecode.
///
/// Args are pushed onto the EVM stack (left-to-right order), then the continuation
/// label is pushed as TOS, then JUMP to callee.  Caller's locals remain below.
pub fn translate_call(
    dest: Option<&StackSlot>,
    target: &CallTarget,
    args: &[LoweredOperand],
    builder: &mut BytecodeBuilder,
    stack_state: &StackState,
    function_metas: &FunctionCallMap,
    entry_layout: &[StackSlot],
    block_stack: &mut Vec<StackSlot>,
    allocation: &StackAllocation,
) -> Result<(), CodegenError> {
    match target {
        CallTarget::Internal(function_id) => {
            let meta = function_metas.get(function_id).ok_or_else(|| {
                CodegenError::Other(format!(
                    "No call info for internal function {:?}",
                    function_id
                ))
            })?;

            if args.len() != meta.param_slots.len() {
                return Err(CodegenError::Other(format!(
                    "Argument count mismatch: expected {}, got {}",
                    meta.param_slots.len(),
                    args.len()
                )));
            }

            // Push arguments left-to-right onto the EVM stack.
            // After this loop: [...caller_locals..., arg0, arg1, ..., argN]
            // argN is closest to TOS (will be at DUP2 when call_cont is TOS).
            //
            // extra_depth increases with each arg pushed because the stack grows.
            for (i, arg) in args.iter().enumerate() {
                push_operand(arg, builder, stack_state, entry_layout, block_stack, i)?;
            }

            // Push continuation label (becomes TOS).
            let call_continuation = builder.new_label();
            builder.push_label(call_continuation);

            // JUMP to callee.  Stack at callee entry:
            //   [call_cont | argN | … | arg0 | caller_block_stack | entry_layout | outer_cont]
            builder.jump_to(meta.entry_label);

            // Continuation JUMPDEST.
            builder.mark_label(call_continuation);
            builder.emit(Opcode::JUMPDEST);

            // After return, the callee did SWAP1+JUMP (non-void) or JUMP (void).
            // The callee's terminator plan_shuffle already consumed the entire callee frame
            // (including the args that the caller pushed). After SWAP1+JUMP the stack is:
            //   [return_val (if non-void) | caller_block_stack | caller_entry_layout | outer_cont]
            // No arg cleanup needed here.

            // Store result to dest.
            if let Some(dest_slot) = dest {
                // return_val is at TOS; store it.
                store_to_slot(
                    *dest_slot,
                    builder,
                    stack_state,
                    entry_layout,
                    block_stack,
                    allocation,
                )?;
            }

            Ok(())
        }

        CallTarget::External { .. } => Err(CodegenError::Other(
            "External function calls not yet implemented".to_string(),
        )),
    }
}
