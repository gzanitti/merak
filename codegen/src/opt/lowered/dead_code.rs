/// Dead instruction elimination for LoweredCfg
///
/// Removes instructions whose result is never read anywhere in the function,
/// provided the instruction has no side effects.
///
/// An instruction is considered dead when:
/// - It has a destination slot, AND
/// - That slot does not appear as a source operand in any instruction or
///   terminator in the entire function, AND
/// - The instruction has no side effects (not StorageStore, not Call).
use std::collections::HashSet;

use merak_ir::{Instruction, Terminator};

use crate::lowering::{LoweredCfg, LoweredOperand, StackSlot};
use crate::opt::Pass;

pub struct DeadInstructionElimination;

impl Pass<LoweredCfg> for DeadInstructionElimination {
    fn name(&self) -> &str {
        "dead-instruction-elimination"
    }

    fn run(&self, cfg: &mut LoweredCfg) -> bool {
        let used = collect_used_slots(cfg);
        let mut changed = false;

        for block in cfg.blocks.values_mut() {
            let before = block.instructions.len();
            block.instructions.retain(|inst| {
                // Always keep side-effecting instructions (SSTORE, CALLs).
                if inst.has_side_effects() {
                    return true;
                }
                // Keep if the destination slot is read somewhere.
                match inst.destination() {
                    None => true,
                    Some(slot) => used.contains(&slot),
                }
            });
            if block.instructions.len() != before {
                changed = true;
            }
        }

        changed
    }
}

/// Collect every `StackSlot` that appears as a *source* operand anywhere
/// in the function (instructions + terminators across all blocks).
fn collect_used_slots(cfg: &LoweredCfg) -> HashSet<StackSlot> {
    let mut used = HashSet::new();

    for block in cfg.blocks.values() {
        // Instruction operands
        for inst in &block.instructions {
            for operand in inst.operands() {
                if let LoweredOperand::Location(slot) = operand {
                    used.insert(*slot);
                }
            }
        }

        // Terminator operands (Return value, Branch condition)
        match &block.terminator {
            Terminator::Return {
                value: Some(LoweredOperand::Location(slot)),
                ..
            } => {
                used.insert(*slot);
            }
            Terminator::Branch {
                condition: LoweredOperand::Location(slot),
                ..
            } => {
                used.insert(*slot);
            }
            _ => {}
        }
    }

    used
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::storage::StorageLayout;
    use crate::lowering::LoweredInstruction;
    use merak_ast::expression::BinaryOperator;
    use merak_ir::{ssa_ir::Constant, BasicBlock, Operand, Terminator};
    use merak_symbols::SymbolId;
    use std::collections::HashMap;

    fn make_cfg(instructions: Vec<LoweredInstruction>) -> LoweredCfg {
        let mut block = BasicBlock::new(0);
        block.instructions = instructions;
        block.terminator = Terminator::Return {
            value: Some(LoweredOperand::Location(StackSlot(0))),
            meta: (),
        };

        let mut blocks = HashMap::new();
        blocks.insert(0, block);

        LoweredCfg {
            name: "test".to_string(),
            function_id: SymbolId::new("test".to_string(), 0),
            blocks,
            entry: 0,
            register_map: HashMap::new(),
            storage_layout: StorageLayout::new(),
            register_types: HashMap::new(),
            parameters: vec![],
            frame_base: 0x80,
            is_external: true,
        }
    }

    #[test]
    fn test_removes_dead_instruction() {
        // slot(1) = 42  →  slot(1) never read  →  should be removed
        // slot(0) = 10  →  slot(0) used by Return  →  kept
        let insts = vec![
            LoweredInstruction::LoadConstant {
                dest: StackSlot(1),
                value: Constant::Int(42),
            },
            LoweredInstruction::LoadConstant {
                dest: StackSlot(0),
                value: Constant::Int(10),
            },
        ];

        let mut cfg = make_cfg(insts);
        let changed = DeadInstructionElimination.run(&mut cfg);

        assert!(changed);
        let block = &cfg.blocks[&0];
        assert_eq!(block.instructions.len(), 1);
        match &block.instructions[0] {
            LoweredInstruction::LoadConstant { dest, .. } => assert_eq!(*dest, StackSlot(0)),
            _ => panic!("unexpected instruction"),
        }
    }

    #[test]
    fn test_keeps_live_instruction() {
        // slot(0) = slot(1) + slot(1) — both slots are used
        let insts = vec![
            LoweredInstruction::LoadConstant {
                dest: StackSlot(1),
                value: Constant::Int(5),
            },
            LoweredInstruction::BinaryOp {
                dest: StackSlot(0),
                op: BinaryOperator::Add,
                left: Operand::Location(StackSlot(1)),
                right: Operand::Location(StackSlot(1)),
            },
        ];

        let mut cfg = make_cfg(insts);
        let changed = DeadInstructionElimination.run(&mut cfg);

        assert!(!changed);
        assert_eq!(cfg.blocks[&0].instructions.len(), 2);
    }

    #[test]
    fn test_keeps_side_effecting_instructions() {
        // StorageStore has side effects even though it has no destination slot.
        let insts = vec![LoweredInstruction::StorageStore {
            storage_slot: 0,
            value: LoweredOperand::Constant(Constant::Int(99)),
        }];

        let mut cfg = make_cfg(insts);
        let changed = DeadInstructionElimination.run(&mut cfg);

        assert!(!changed);
        assert_eq!(cfg.blocks[&0].instructions.len(), 1);
    }
}
