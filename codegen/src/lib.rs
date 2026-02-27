/// Merak Codegen - EVM Bytecode Generation
/// This crate translates Merak SSA IR to EVM bytecode.
pub mod analysis;
pub mod codegen;
pub mod evm;
pub mod layout;
pub mod lowering;
pub mod opt;

pub use evm::{BytecodeBuilder, CodegenError, Label, Opcode};
pub use layout::{ContractLayout, StorageLayout};
pub use lowering::{EvmType, LoweredBlock, LoweredCfg, StackSlot};
use merak_ast::meta::SourceRef;
use merak_ir::{BlockId, Terminator};

use crate::analysis::liveness::analyze_liveness;
use crate::analysis::stack_allocator;
use crate::codegen::instructions::{self, StackState};
use crate::codegen::{terminators, FunctionCallInfo, FunctionCallMap};
use crate::lowering::{type_lowering, LoweredTerminator};
use crate::opt::lowered::DeadInstructionElimination;
use crate::opt::Pipeline;
use layout::FunctionEntry;
use lowering::{LoweredInstruction, LoweredOperand};
use merak_ast::function::Visibility;
use merak_ir::ssa_ir::{Operand, Register, SsaCfg, SsaContract, SsaInstruction, SsaProgram};
use merak_ir::ControlFlowGraph;
use merak_symbols::SymbolId;
use merak_symbols::SymbolTable;
use std::collections::HashMap;

pub struct Codegen {}

impl Codegen {
    pub fn new() -> Self {
        Self {}
    }

    /// Compile an SSA program to EVM bytecode
    pub fn compile_program(
        &self,
        ssa_program: &mut SsaProgram,
        symbol_table: &SymbolTable,
    ) -> Result<CompiledProgram, CodegenError> {
        let mut compiled = CompiledProgram::new();
        for (name, file) in &mut ssa_program.files {
            let contract = self.compile_contract(&mut file.contract, symbol_table)?;
            compiled.contracts.push((name.to_string(), contract));
        }

        Ok(compiled)
    }

    /// Compile a single contract.
    fn compile_contract(
        &self,
        contract: &mut SsaContract,
        symbol_table: &SymbolTable,
    ) -> Result<Vec<u8>, CodegenError> {
        // tx.data = [ deploy section ][ runtime section ][ constructor_arg_0 ][ constructor_arg_1 ] ...

        let storage_layout = storage_layout(contract, symbol_table)?;
        let lowered_functions = self.lower_functions(contract, symbol_table, &storage_layout)?;

        let mut builder = BytecodeBuilder::new();
        let mut contract_layout = ContractLayout::new();
        let mut function_metas: FunctionCallMap = HashMap::new();
        let mut entry_labels: Vec<Label> = Vec::new();

        // Allocate labels for all functions upfront so any internal caller
        // can reference them before the bodies are emitted.
        for (lowered_cfg, abi_sig) in &lowered_functions {
            let entry_label = builder.new_label();
            let prologue_label = builder.new_label();
            let continuation_label = builder.new_label();

            let param_slots: Vec<StackSlot> =
                lowered_cfg.parameters.iter().map(|(_, s)| *s).collect();

            function_metas.insert(
                lowered_cfg.function_id.clone(),
                FunctionCallInfo {
                    entry_label,
                    frame_base: lowered_cfg.frame_base,
                    param_slots: param_slots.clone(),
                },
            );

            let entry = FunctionEntry::new(
                lowered_cfg.function_id.clone(),
                abi_sig,
                entry_label,
                prologue_label,
                continuation_label,
                lowered_cfg.frame_base,
                param_slots,
            );
            contract_layout.add_function(entry);
            entry_labels.push(entry_label);
        }

        // Emit constructor body (if present) then the CODECOPY+RETURN sequence.
        let mut constructor_arg_patch_positions: Vec<usize> = Vec::new();

        let codecopy_pos = if let Some(constructor_cfg) = &mut contract.constructor {
            let mut lowered_constructor =
                self.lower_function(constructor_cfg, &storage_layout, symbol_table)?;
            // Constructor params arrive in memory (via CODECOPY), same as external functions.
            lowered_constructor.is_external = true;

            // Assign frame base after the last regular function.
            let constructor_base = lowered_functions
                .last()
                .map(|(cfg, _)| cfg.frame_base + compute_frame_size(cfg))
                .unwrap_or(0x80);
            lowered_constructor.frame_base = constructor_base;

            // Run optimizations on the constructor too.
            let opt: Pipeline<LoweredCfg> =
                Pipeline::new(vec![Box::new(DeadInstructionElimination)]);
            opt.run_to_fixed_point(&mut lowered_constructor);

            let constructor_label = builder.new_label();
            let codecopy_label = builder.new_label();

            // For each arg i we emit:
            //   PUSH1 32                  ; size
            //   PUSH2 <placeholder>       ; src = (total_bytecode_size + i*32), patched later
            //   PUSH  frame_offset        ; dest in memory = frame slot for this param
            //   CODECOPY                  ; copies 32 bytes directly into the frame slot
            //
            // The placeholder is patched after finalization once the total bytecode
            // size is known.
            for (_, (_, param_slot)) in lowered_constructor.parameters.iter().enumerate() {
                let frame_offset = lowered_constructor.frame_base + param_slot.0 as u64 * 32;

                // size = 32
                builder.push_u64(32);

                // src offset placeholder (CODECOPY stack order: dest=TOS, src=TOS-1, size=TOS-2)
                let patch_pos = builder.position() + 1;
                builder.emit(Opcode::PUSH2);
                builder.emit_bytes(&[0xFF, 0xFF]); // placeholder
                constructor_arg_patch_positions.push(patch_pos);

                // dest = frame slot in memory
                builder.push_u64(frame_offset);

                builder.emit(Opcode::CODECOPY);
            }

            // Push codecopy_label as the constructor's continuation, then jump.
            builder.push_label(codecopy_label);
            builder.jump_to(constructor_label);

            builder.mark_label(constructor_label);
            self.generate_function(&lowered_constructor, &mut builder, &function_metas)?;

            // Constructors always return void; the JUMP continuation lands here.
            builder.mark_label(codecopy_label);
            builder.emit(Opcode::JUMPDEST);

            let pos = builder.position();
            emit_codecopy_placeholder(&mut builder);
            pos
        } else {
            // No constructor: deploy section is just the 15-byte CODECOPY sequence.
            let pos = builder.position();
            emit_codecopy_placeholder(&mut builder);
            pos
        };

        builder.mark_runtime_start();
        let runtime_start = builder.position(); // equals deploy_size

        contract_layout.generate_dispatcher(&mut builder);

        for (i, (lowered_cfg, _)) in lowered_functions.iter().enumerate() {
            builder.mark_label(entry_labels[i]);
            self.generate_function(lowered_cfg, &mut builder, &function_metas)?;
        }

        // Resolve all labels, then patch the PUSH2 size arguments in the
        // CODECOPY section now that runtime_size and deploy_size are known.
        let mut bytecode = builder.finalize()?;

        let runtime_size = (bytecode.len() - runtime_start) as u16;
        let deploy_size = runtime_start as u16;
        patch_codecopy_section(&mut bytecode, codecopy_pos, runtime_size, deploy_size);

        // Patch constructor arg calldata offsets.
        // Each arg i is at calldata[total_bytecode_size + i*32] because
        // tx.data = creation_bytecode ++ abi_args during CREATE.
        let total_bytecode_size = bytecode.len() as u16;
        for (i, patch_pos) in constructor_arg_patch_positions.iter().enumerate() {
            let offset = total_bytecode_size + (i as u16 * 32);
            let [hi, lo] = offset.to_be_bytes();
            bytecode[*patch_pos] = hi;
            bytecode[*patch_pos + 1] = lo;
        }

        Ok(bytecode)
    }

    fn lower_functions(
        &self,
        contract: &mut SsaContract,
        symbol_table: &SymbolTable,
        storage_layout: &StorageLayout,
    ) -> Result<Vec<(LoweredCfg, String)>, CodegenError> {
        let mut lowered_functions: Vec<(LoweredCfg, String)> = Vec::new();
        for ssacfg in &mut contract.functions {
            let lowered_cfg = self.lower_function(ssacfg, storage_layout, symbol_table)?;

            let abi_signature = evm::abi::build_abi_signature(
                &lowered_cfg.name,
                symbol_table,
                &lowered_cfg.function_id,
            )?;

            lowered_functions.push((lowered_cfg, abi_signature));
        }

        let opt: Pipeline<LoweredCfg> = Pipeline::new(vec![Box::new(DeadInstructionElimination)]);
        for (lowered_cfg, _) in &mut lowered_functions {
            opt.run_to_fixed_point(lowered_cfg);
        }

        // frame_base(i) = 0x80 + sum(frame_sizes[0..i])
        let mut current_base: u64 = 0x80;
        for (lowered_cfg, _) in &mut lowered_functions {
            let frame_size = compute_frame_size(lowered_cfg);
            lowered_cfg.frame_base = current_base;
            current_base += frame_size;
        }

        Ok(lowered_functions)
    }

    /// Lower a single function CFG
    fn lower_function(
        &self,
        ssacfg: &mut SsaCfg,
        storage_layout: &StorageLayout,
        symbol_table: &SymbolTable,
    ) -> Result<LoweredCfg, CodegenError> {
        Self::eliminate_phi_nodes(ssacfg);

        let (lowered_types, register_map) =
            lowering::type_lowering::lower_registers(&ssacfg, symbol_table)?;

        let mut lowered_blocks = HashMap::new();

        for (block_id, block) in &ssacfg.blocks {
            let lowered_instructions = block
                .instructions
                .iter()
                .filter_map(|inst| {
                    match Self::lower_instruction(inst, &register_map, storage_layout) {
                        Ok(lowered) => Some(Ok(lowered)),
                        Err(CodegenError::VerificationOnly) => None,
                        Err(e) => Some(Err(e)),
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;

            let lowered_block = LoweredBlock {
                id: *block_id,
                instructions: lowered_instructions,
                terminator: Self::lower_terminator(&block.terminator, &register_map)?,
                predecessors: block.predecessors.clone(),
                successors: block.successors.clone(),
                loop_invariants: None,
                loop_variants: None,
            };

            lowered_blocks.insert(*block_id, lowered_block);
        }

        let parameters: Vec<(SymbolId, StackSlot)> = ssacfg
            .parameters
            .iter()
            .map(|param_id| {
                let param_reg = Register {
                    symbol: param_id.clone(),
                    version: 0,
                };
                let slot = register_map.get(&param_reg).cloned().unwrap();
                (param_id.clone(), slot)
            })
            .collect();

        // Determine if this function is external (callable from outside the contract).
        // External and Entrypoint functions receive parameters via calldata (stored in
        // memory by the dispatcher). Internal functions receive parameters on the EVM
        // stack via the Yul-like calling convention.
        let is_external = match &symbol_table.get_symbol(&ssacfg.function_id).kind {
            merak_symbols::SymbolKind::Function { visibility, .. } => {
                matches!(visibility, Visibility::External | Visibility::Entrypoint)
            }
            merak_symbols::SymbolKind::Entrypoint { .. } => true,
            _ => false,
        };

        Ok(LoweredCfg {
            name: ssacfg.name.clone(),
            function_id: ssacfg.function_id.clone(),
            blocks: lowered_blocks,
            entry: ssacfg.entry,
            register_map,
            storage_layout: storage_layout.clone(),
            register_types: lowered_types,
            parameters,
            frame_base: 0, // Assigned later in compile_contract
            is_external,
        })
    }

    /// Convert SSA instruction to lowered instruction
    fn lower_instruction(
        inst: &SsaInstruction,
        register_map: &HashMap<Register, StackSlot>,
        storage_layout: &StorageLayout,
    ) -> Result<lowering::LoweredInstruction, CodegenError> {
        match inst {
            SsaInstruction::Copy { dest, source, .. } => {
                let dest_slot = register_map.get(dest).ok_or_else(|| {
                    CodegenError::Other(format!("Register {:?} not allocated", dest))
                })?;

                let source_operand = match source {
                    Operand::Location(src_reg) => {
                        let src_slot = register_map.get(src_reg).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", src_reg))
                        })?;
                        LoweredOperand::Location(*src_slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                Ok(LoweredInstruction::Copy {
                    dest: *dest_slot,
                    source: source_operand,
                })
            }

            SsaInstruction::BinaryOp {
                dest,
                op,
                left,
                right,
                ..
            } => {
                let dest_slot = register_map.get(dest).ok_or_else(|| {
                    CodegenError::Other(format!("Register {:?} not allocated", dest))
                })?;

                let left_operand = match left {
                    Operand::Location(r) => {
                        let slot = register_map.get(r).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", r))
                        })?;
                        LoweredOperand::Location(*slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                let right_operand = match right {
                    Operand::Location(r) => {
                        let slot = register_map.get(r).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", r))
                        })?;
                        LoweredOperand::Location(*slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                Ok(LoweredInstruction::BinaryOp {
                    dest: *dest_slot,
                    op: op.clone(),
                    left: left_operand,
                    right: right_operand,
                })
            }

            SsaInstruction::UnaryOp {
                dest, op, operand, ..
            } => {
                let dest_slot = register_map.get(dest).ok_or_else(|| {
                    CodegenError::Other(format!("Register {:?} not allocated", dest))
                })?;

                let operand_lowered = match operand {
                    Operand::Location(r) => {
                        let slot = register_map.get(r).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", r))
                        })?;
                        LoweredOperand::Location(*slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                Ok(LoweredInstruction::UnaryOp {
                    dest: *dest_slot,
                    op: op.clone(),
                    operand: operand_lowered,
                })
            }

            SsaInstruction::StorageLoad { dest, var, .. } => {
                let dest_slot = register_map.get(dest).ok_or_else(|| {
                    CodegenError::Other(format!("Register {:?} not allocated", dest))
                })?;

                // Look up storage slot from storage layout
                let storage_slot = storage_layout.get(var).ok_or_else(|| {
                    CodegenError::Other(format!("Storage variable {:?} not in layout", var))
                })?;

                Ok(LoweredInstruction::StorageLoad {
                    dest: *dest_slot,
                    storage_slot: storage_slot.offset,
                })
            }

            SsaInstruction::StorageStore { var, value, .. } => {
                let value_operand = match value {
                    Operand::Location(r) => {
                        let slot = register_map.get(r).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", r))
                        })?;
                        LoweredOperand::Location(*slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                // Look up storage slot from storage layout
                let storage_slot = storage_layout.get(var).ok_or_else(|| {
                    CodegenError::Other(format!("Storage variable {:?} not in layout", var))
                })?;

                Ok(LoweredInstruction::StorageStore {
                    storage_slot: storage_slot.offset,
                    value: value_operand,
                })
            }

            SsaInstruction::Call {
                dest, target, args, ..
            } => {
                let dest_slot = dest
                    .as_ref()
                    .map(|d| {
                        register_map.get(d).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", d))
                        })
                    })
                    .transpose()?
                    .copied();

                let arg_operands = args
                    .iter()
                    .map(|arg| match arg {
                        Operand::Location(r) => {
                            let slot = register_map.get(r).ok_or_else(|| {
                                CodegenError::Other(format!("Register {:?} not allocated", r))
                            })?;
                            Ok(LoweredOperand::Location(*slot))
                        }
                        Operand::Constant(c) => Ok(LoweredOperand::Constant(c.clone())),
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(LoweredInstruction::Call {
                    dest: dest_slot,
                    target: target.clone(),
                    args: arg_operands,
                })
            }

            SsaInstruction::Phi { .. } => {
                unreachable!("Phi nodes should have been eliminated")
            }

            SsaInstruction::Unfold { .. } | SsaInstruction::Fold { .. } => {
                // Verification-only instructions - they don't generate EVM opcodes
                Err(CodegenError::VerificationOnly)
            }

            SsaInstruction::Assert { .. } => {
                println!("TODO: Assert functionality not implemented yed");
                Err(CodegenError::VerificationOnly)
            }
        }
    }

    /// Convert SSA terminator to lowered terminator
    fn lower_terminator(
        term: &Terminator,
        register_map: &HashMap<Register, StackSlot>,
    ) -> Result<LoweredTerminator, CodegenError> {
        match term {
            Terminator::Jump { target } => Ok(LoweredTerminator::Jump { target: *target }),

            Terminator::Branch {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition_operand = match condition {
                    Operand::Location(r) => {
                        let slot = register_map.get(r).ok_or_else(|| {
                            CodegenError::Other(format!("Register {:?} not allocated", r))
                        })?;
                        LoweredOperand::Location(*slot)
                    }
                    Operand::Constant(c) => LoweredOperand::Constant(c.clone()),
                };

                Ok(LoweredTerminator::Branch {
                    condition: condition_operand,
                    then_block: *then_block,
                    else_block: *else_block,
                    meta: (),
                })
            }

            Terminator::Return { value, .. } => {
                let value_operand = value
                    .as_ref()
                    .map(|v| match v {
                        Operand::Location(r) => {
                            let slot = register_map.get(r).ok_or_else(|| {
                                CodegenError::Other(format!("Register {:?} not allocated", r))
                            })?;
                            Ok(LoweredOperand::Location(*slot))
                        }
                        Operand::Constant(c) => Ok(LoweredOperand::Constant(c.clone())),
                    })
                    .transpose()?;

                Ok(LoweredTerminator::Return {
                    value: value_operand,
                    meta: (),
                })
            }

            Terminator::Unreachable => Ok(LoweredTerminator::Unreachable),
        }
    }

    fn eliminate_phi_nodes(ssacfg: &mut SsaCfg) {
        let mut copies_per_pred: HashMap<BlockId, Vec<SsaInstruction>> = HashMap::new();
        for block in ssacfg.blocks.values() {
            for inst in &block.instructions {
                if let SsaInstruction::Phi { dest, sources } = inst {
                    for (pred_block, source_reg) in sources {
                        copies_per_pred.entry(*pred_block).or_default().push(
                            SsaInstruction::Copy {
                                dest: dest.clone(),
                                source: Operand::Location(source_reg.clone()),
                                source_ref: SourceRef::unknown(),
                            },
                        );
                    }
                }
            }
        }

        for (id, block) in &mut ssacfg.blocks {
            block
                .instructions
                .retain(|inst| !matches!(inst, SsaInstruction::Phi { .. }));
            if let Some(copies) = copies_per_pred.remove(id) {
                block.instructions.extend(copies);
            }
        }
    }

    /// Generate bytecode for a lowered function body.
    ///
    /// The entry label must already be marked by the caller before this is invoked,
    /// so that dispatcher JUMPs and internal call-site JUMPs land on the JUMPDEST
    /// emitted for the first block.
    ///
    /// For external functions: the dispatcher pre-stores calldata params to memory;
    /// the entry block MLOADs them onto the stack from there.
    ///
    /// For internal functions: the caller pushed params onto the EVM stack (Yul
    /// convention) before JUMPing; a compact prologue sinks the continuation and
    /// reorders the params to match `entry_layout`.
    ///
    /// For non-entry blocks: the predecessor's terminator plan_shuffle already placed
    /// the expected `entry_layout` slots onto the EVM stack — no MLOAD needed.
    fn generate_function(
        &self,
        lowered: &LoweredCfg,
        builder: &mut BytecodeBuilder,
        function_metas: &FunctionCallMap,
    ) -> Result<(), CodegenError> {
        use crate::codegen::stack_shuffler::{emit_shuffle, plan_shuffle};
        use std::collections::HashSet;

        let block_order = lowered.reverse_post_order();

        let liveness = analyze_liveness(lowered);
        let allocation = stack_allocator::allocate(lowered, &liveness);

        // Validate that the EVM stack never overflows (debug builds only).
        #[cfg(debug_assertions)]
        crate::analysis::stack_depth::analyze_stack_depth(lowered, &allocation.entry_layout)?;

        // Label reservation (patched during finalize())
        let mut block_labels = HashMap::new();
        for &block_id in &block_order {
            let label = builder.new_label();
            block_labels.insert(block_id, label);
        }

        let stack_state = StackState::with_frame_base(lowered.frame_base);

        // Emit each block
        for &block_id in &block_order {
            let block = lowered
                .block(block_id)
                .ok_or_else(|| CodegenError::Other(format!("Block {:?} not found", block_id)))?;

            // Mark the block's label and emit its JUMPDEST.
            let label = block_labels.get(&block_id).ok_or_else(|| {
                CodegenError::Other(format!("Label not found for block {:?}", block_id))
            })?;
            builder.mark_label(*label);
            builder.emit(Opcode::JUMPDEST);

            let is_entry = block_id == lowered.entry;
            let entry_slots: Vec<StackSlot> = allocation
                .entry_layout
                .get(&block_id)
                .cloned()
                .unwrap_or_default();

            if is_entry && lowered.is_external {
                // External / constructor entry block: params stored to memory by the
                // dispatcher or CODECOPY loop. MLOAD them deepest-first so that
                // entry_slots[0] ends up at TOS (position 1 = DUP1).
                for slot in entry_slots.iter().rev() {
                    let offset = stack_state.memory_offset_for_slot(slot.0);
                    builder.push_u64(offset);
                    builder.emit(Opcode::MLOAD);
                }
            } else if is_entry {
                // Internal function entry block: caller pushed params left-to-right
                // (arg0 deepest, argN-1 closest to TOS), then pushed call_cont, then
                // JUMPed.  At JUMPDEST: [call_cont (TOS), argN-1, ..., arg0, outer_cont].
                //
                // Step 1: SWAP(N) sinks call_cont below the N args.
                //   After: [arg0 (TOS), argN-1, ..., arg1, call_cont, outer_cont]
                // Step 2: plan_shuffle from [arg0, argN-1, ..., arg1] to entry_slots.
                let n = lowered.parameters.len();
                if n > 0 {
                    // Step 1
                    builder.emit(
                        Opcode::swap_for_position(n)
                            .ok_or_else(|| CodegenError::Other(format!(
                                "Function '{}' has {} params, exceeding EVM SWAP limit (16)",
                                lowered.name, n
                            )))?,
                    );
                    // Step 2: build initial_stack = [param[0], param[N-1], ..., param[1]]
                    let params = &lowered.parameters;
                    let mut initial_stack: Vec<Option<StackSlot>> = Vec::with_capacity(n);
                    initial_stack.push(Some(params[0].1));
                    for i in (1..n).rev() {
                        initial_stack.push(Some(params[i].1));
                    }
                    let ops = plan_shuffle(&initial_stack, &entry_slots, &HashSet::new());
                    emit_shuffle(&ops, builder, &stack_state);
                }
                // n == 0: only call_cont on stack; entry_slots is empty → no prologue.
            }
            // Non-entry blocks: predecessor's terminator plan_shuffle placed entry_slots.

            let mut block_stack: Vec<StackSlot> = vec![];

            for instruction in &block.instructions {
                instructions::translate_instruction(
                    instruction,
                    builder,
                    &stack_state,
                    function_metas,
                    &entry_slots,
                    &allocation,
                    &mut block_stack,
                )?;
            }

            terminators::translate_terminator(
                &block.terminator,
                builder,
                &block_labels,
                &stack_state,
                &entry_slots,
                &block_stack,
                &allocation,
            )?;
        }

        Ok(())
    }
}

/// Emit a 15-byte CODECOPY+RETURN sequence with placeholder sizes.
///
/// Stack must be empty on entry. The sequence copies the runtime section to
/// `mem[0]` and returns it to the EVM, which stores it as the deployed code.
/// Placeholders are patched by [`patch_codecopy_section`] after finalization.
fn emit_codecopy_placeholder(builder: &mut BytecodeBuilder) {
    builder.emit(Opcode::PUSH2);
    builder.emit_bytes(&[0xFF, 0xFF]); // runtime_size placeholder
    builder.emit(Opcode::PUSH2);
    builder.emit_bytes(&[0xFF, 0xFF]); // deploy_size placeholder (source offset)
    builder.emit(Opcode::PUSH1);
    builder.emit_bytes(&[0x00]); // destOffset = 0
    builder.emit(Opcode::CODECOPY);
    builder.emit(Opcode::PUSH2);
    builder.emit_bytes(&[0xFF, 0xFF]); // runtime_size placeholder (return length)
    builder.emit(Opcode::PUSH1);
    builder.emit_bytes(&[0x00]); // return from mem[0]
    builder.emit(Opcode::RETURN);
}

/// Patch the PUSH2 size arguments in the CODECOPY section at byte offset `pos`.
fn patch_codecopy_section(bytecode: &mut Vec<u8>, pos: usize, runtime_size: u16, deploy_size: u16) {
    let [rh, rl] = runtime_size.to_be_bytes();
    let [dh, dl] = deploy_size.to_be_bytes();
    bytecode[pos + 1] = rh;
    bytecode[pos + 2] = rl;
    bytecode[pos + 4] = dh;
    bytecode[pos + 5] = dl;
    bytecode[pos + 10] = rh;
    bytecode[pos + 11] = rl;
}

fn storage_layout(
    contract: &mut SsaContract,
    symbol_table: &SymbolTable,
) -> Result<StorageLayout, CodegenError> {
    let mut storage_layout = StorageLayout::new();
    for var in &contract.variables {
        if let Some(symbol_id) = symbol_table.get_symbol_id_by_node_id(var.id) {
            let symbol_info = symbol_table.get_symbol(&symbol_id);
            if let Some(ty) = &symbol_info.ty {
                let evm_type = type_lowering::base_type_to_evm_type(&ty.base)?;
                storage_layout.assign(symbol_id.clone(), &evm_type);
            }
        }
    }
    for const_var in &contract.constants {
        if let Some(symbol_id) = symbol_table.get_symbol_id_by_node_id(const_var.id) {
            let symbol_info = symbol_table.get_symbol(&symbol_id);
            if let Some(ty) = &symbol_info.ty {
                let evm_type = type_lowering::base_type_to_evm_type(&ty.base)?;
                storage_layout.assign(symbol_id.clone(), &evm_type);
            }
        }
    }
    Ok(storage_layout)
}

/// Compute the memory frame size required by a lowered function in bytes.
///
/// The frame size is `(max_slot_index + 1) * 32`, i.e., enough 32-byte words
/// to hold every register the function may use.  Returns at least 32 bytes so
/// the frame is never empty (a zero-sized frame would make two adjacent
/// functions share the same base, which is harmless but confusing).
fn compute_frame_size(lowered: &LoweredCfg) -> u64 {
    let num_slots = lowered
        .register_map
        .values()
        .map(|s| s.0 + 1)
        .max()
        .unwrap_or(1);
    (num_slots as u64) * 32
}

/// Compiled program output
#[derive(Debug, Clone)]
pub struct CompiledProgram {
    /// List of compiled contracts (name, bytecode)
    pub contracts: Vec<(String, Vec<u8>)>,
}

impl CompiledProgram {
    pub fn new() -> CompiledProgram {
        Self { contracts: vec![] }
    }

    /// Get bytecode for a contract by name
    pub fn get_contract(&self, name: &str) -> Option<&[u8]> {
        self.contracts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, bytecode)| bytecode.as_slice())
    }

    /// Get total bytecode size
    pub fn total_size(&self) -> usize {
        self.contracts.iter().map(|(_, bc)| bc.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use merak_ast::{
        expression::BinaryOperator,
        function::{Modifier, Visibility},
        predicate::Predicate,
        types::{BaseType, Type},
        NodeId,
    };
    use merak_ir::{ssa_ir::SsaFile, BasicBlock, Constant, Terminator, TerminatorMeta};
    use merak_symbols::{QualifiedName, SymbolKind};

    use super::*;

    #[test]
    fn test_simple_program_compilation() {
        // Create a minimal program with a contract that has a simple function
        // Function just returns constant 42

        let mut symbol_table = SymbolTable::new();

        // Add function to symbol table
        let fn_id = symbol_table
            .add_symbol(
                NodeId::from(100),
                QualifiedName::from_string("test".to_string()),
                SymbolKind::Function {
                    visibility: Visibility::External,
                    reentrancy: Modifier::Checked,
                    parameters: vec![],
                    ensures: vec![],
                    requires: vec![],
                    return_type: Type {
                        base: BaseType::Int,
                        binder: "r".to_string(),
                        constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                        explicit_annotation: false,
                        source_ref: SourceRef::unknown(),
                    },
                },
                None,
            )
            .unwrap();

        // Create a simple function: fn test() -> int { return 42; }
        let mut cfg = SsaCfg::new("test".to_string(), fn_id.clone());

        // Create entry block with just a return
        let mut block = BasicBlock::new(0);
        block.terminator = Terminator::Return {
            value: Some(Operand::Constant(Constant::Int(42))),
            meta: TerminatorMeta::default(),
        };

        cfg.blocks.insert(0, block);
        cfg.entry = 0;

        // Create contract with this function
        let contract = SsaContract {
            name: "TestContract".to_string(),
            variables: vec![],
            constants: vec![],
            constructor: None,
            functions: vec![cfg],
        };

        // Create file with the contract
        let file = SsaFile {
            imports: vec![],
            interfaces: vec![],
            contract,
        };

        // Create program with the file
        let mut files = IndexMap::new();
        files.insert("test.merak".to_string(), file);

        let mut program = SsaProgram { files };

        // Compile
        let codegen = Codegen::new();
        let result = codegen.compile_program(&mut program, &symbol_table);

        // Should succeed
        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        assert_eq!(compiled.contracts.len(), 1);
        assert_eq!(compiled.contracts[0].0, "test.merak");

        // Bytecode should be non-empty
        let bytecode = &compiled.contracts[0].1;
        assert!(!bytecode.is_empty(), "Bytecode should not be empty");

        // Print bytecode as requested
        println!("Simple bytecode: {:02x?}", bytecode);

        // Should contain at least JUMPDEST and RETURN opcodes
        assert!(bytecode.contains(&Opcode::JUMPDEST.encode()));
        assert!(bytecode.contains(&Opcode::RETURN.encode()));
    }

    #[test]
    fn test_arithmetic_program_compilation() {
        // Create a simpler program: fn test() -> int { let x = 10; let y = 32; return x + y; }
        // This avoids parameters and symbol table lookups

        let mut symbol_table = SymbolTable::new();

        // Add function to symbol table
        let fn_id = symbol_table
            .add_symbol(
                NodeId::from(200),
                QualifiedName::from_string("test".to_string()),
                SymbolKind::Function {
                    visibility: Visibility::External,
                    reentrancy: Modifier::Checked,
                    parameters: vec![],
                    ensures: vec![],
                    requires: vec![],
                    return_type: Type {
                        base: BaseType::Int,
                        binder: "r".to_string(),
                        constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                        explicit_annotation: false,
                        source_ref: SourceRef::unknown(),
                    },
                },
                None,
            )
            .unwrap();

        let mut cfg = SsaCfg::new("test".to_string(), fn_id.clone());

        // No parameters
        cfg.parameters = vec![];

        // Create basic block
        let mut block = BasicBlock::new(0);

        // Temporaries for constants and result
        let temp_x = SymbolId::Temp(0);
        let temp_y = SymbolId::Temp(1);
        let temp_result = SymbolId::Temp(2);

        let reg_x = Register {
            symbol: temp_x.clone(),
            version: 0,
        };
        let reg_y = Register {
            symbol: temp_y.clone(),
            version: 0,
        };
        let reg_result = Register {
            symbol: temp_result.clone(),
            version: 0,
        };

        // x = 10
        block.instructions.push(SsaInstruction::Copy {
            dest: reg_x.clone(),
            source: Operand::Constant(Constant::Int(10)),
            source_ref: SourceRef::unknown(),
        });

        // y = 32
        block.instructions.push(SsaInstruction::Copy {
            dest: reg_y.clone(),
            source: Operand::Constant(Constant::Int(32)),
            source_ref: SourceRef::unknown(),
        });

        // result = x + y
        block.instructions.push(SsaInstruction::BinaryOp {
            dest: reg_result.clone(),
            op: BinaryOperator::Add,
            left: Operand::Location(reg_x),
            right: Operand::Location(reg_y),
            source_ref: SourceRef::unknown(),
        });

        // Return result
        block.terminator = Terminator::Return {
            value: Some(Operand::Location(reg_result)),
            meta: TerminatorMeta::default(),
        };

        cfg.blocks.insert(0, block);
        cfg.entry = 0;

        // Set types for all temporaries
        cfg.local_temps.insert(temp_x, BaseType::Int);
        cfg.local_temps.insert(temp_y, BaseType::Int);
        cfg.local_temps.insert(temp_result, BaseType::Int);

        // Create contract
        let contract = SsaContract {
            name: "ArithmeticContract".to_string(),
            variables: vec![],
            constants: vec![],
            constructor: None,
            functions: vec![cfg],
        };

        // Create file
        let file = SsaFile {
            imports: vec![],
            interfaces: vec![],
            contract,
        };

        // Create program
        let mut files = IndexMap::new();
        files.insert("arithmetic.merak".to_string(), file);
        let mut program = SsaProgram { files };

        // Compile
        let codegen = Codegen::new();
        let result = codegen.compile_program(&mut program, &symbol_table);

        // Should succeed
        assert!(result.is_ok(), "Compilation failed: {:?}", result.err());

        let compiled = result.unwrap();
        let bytecode = &compiled.contracts[0].1;

        // Print bytecode as requested
        println!("Arithmetic bytecode: {:02x?}", bytecode);

        // Should contain arithmetic opcodes
        assert!(
            bytecode.contains(&Opcode::ADD.encode()),
            "Should contain ADD opcode"
        );
        assert!(
            bytecode.contains(&Opcode::RETURN.encode()),
            "Should contain RETURN opcode"
        );
    }
}
