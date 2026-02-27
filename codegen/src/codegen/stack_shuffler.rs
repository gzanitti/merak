/// EVM stack shuffle planning and emission
///
/// A *shuffle* is the sequence of EVM stack operations (DUP, SWAP, POP, and
/// fallback MLOAD/MSTORE) needed to transform one stack layout into another at
/// a block boundary.
use std::collections::HashSet;

use crate::codegen::instructions::StackState;
use crate::evm::{BytecodeBuilder, Opcode};
use crate::lowering::StackSlot;

/// A primitive EVM stack manipulation emitted by the shuffler.
///
/// Used to transform one stack layout into another cost-efficiently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShuffleOp {
    /// Duplicate the item at 1-based depth `n` (EVM DUP1..DUP16).
    Dup(usize),
    /// Swap TOS with the item at 1-based depth `n` (EVM SWAP1 = swap with depth 2).
    Swap(usize),
    /// Discard TOS (EVM POP).
    Pop,
    /// Load a slot from memory onto TOS (EVM PUSH offset + MLOAD).
    MLoad(StackSlot),
    /// Save TOS to a slot's memory location (EVM PUSH offset + MSTORE).
    MStore(StackSlot),
}

impl ShuffleOp {
    /// Approximate gas cost of this operation.
    pub fn gas_cost(&self, warm_slots: &HashSet<StackSlot>) -> u64 {
        match self {
            ShuffleOp::Dup(_) | ShuffleOp::Swap(_) => 3,
            ShuffleOp::Pop => 2,
            ShuffleOp::MLoad(slot) | ShuffleOp::MStore(slot) => {
                if warm_slots.contains(slot) {
                    6
                } else {
                    21
                }
            }
        }
    }
}

/// Emit `n` POP opcodes to discard `n` pre-loaded slots from the EVM stack.
pub fn emit_drain(n: usize, builder: &mut BytecodeBuilder) {
    for _ in 0..n {
        builder.emit(Opcode::POP);
    }
}

/// Drain the `n_below` EVM stack items that sit **below TOS**, using SWAP1+POP
/// pairs, leaving TOS intact.
pub fn emit_drain_except_tos(n_below: usize, builder: &mut BytecodeBuilder) {
    for _ in 0..n_below {
        builder.emit(Opcode::SWAP1);
        builder.emit(Opcode::POP);
    }
}
/// Plan a sequence of `ShuffleOp`s to transform `current` into `target`.
pub fn plan_shuffle(
    current: &[Option<StackSlot>],
    target: &[StackSlot],
    _warm_slots: &HashSet<StackSlot>,
) -> Vec<ShuffleOp> {
    // Fast path: already correct.
    if current.len() == target.len()
        && current
            .iter()
            .zip(target.iter())
            .all(|(c, t)| *c == Some(*t))
    {
        return vec![];
    }

    let m = current.len();
    let n = target.len();

    // Special case: empty target → just pop everything.
    if n == 0 {
        return vec![ShuffleOp::Pop; m];
    }

    let rotation = m % n;
    let mut ops = Vec::with_capacity(n + m * 2);
    let mut sim: Vec<Option<StackSlot>> = current.to_vec();

    for j in (0..n).rev() {
        let t = target[(j + n - rotation) % n];
        if let Some(pos) = sim.iter().position(|s| *s == Some(t)) {
            ops.push(ShuffleOp::Dup(pos + 1));
        } else {
            ops.push(ShuffleOp::MLoad(t));
        }
        sim.insert(0, Some(t));
    }

    for _ in 0..m {
        ops.push(ShuffleOp::Swap(n));
        ops.push(ShuffleOp::Pop);
    }

    ops
}

/// Total estimated gas cost of a sequence of `ShuffleOp`s.
pub fn shuffle_cost(ops: &[ShuffleOp], warm_slots: &HashSet<StackSlot>) -> u64 {
    ops.iter().map(|op| op.gas_cost(warm_slots)).sum()
}

/// Emit `ops` as EVM bytecode.
///
/// `stack_state` provides the frame base for computing memory offsets.
pub fn emit_shuffle(ops: &[ShuffleOp], builder: &mut BytecodeBuilder, stack_state: &StackState) {
    for op in ops {
        match op {
            ShuffleOp::Dup(n) => {
                if let Some(opcode) = Opcode::dup_for_position(*n) {
                    builder.emit(opcode);
                }
            }
            ShuffleOp::Swap(n) => {
                builder.emit(Opcode::swap_for_position(*n).expect("Swap(n): n must be 1..=16"));
            }
            ShuffleOp::Pop => {
                builder.emit(Opcode::POP);
            }
            ShuffleOp::MLoad(slot) => {
                let offset = stack_state.memory_offset_for_slot(slot.0);
                builder.push_u64(offset);
                builder.emit(Opcode::MLOAD);
            }
            ShuffleOp::MStore(slot) => {
                let offset = stack_state.memory_offset_for_slot(slot.0);
                builder.push_u64(offset);
                builder.emit(Opcode::MSTORE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evm::BytecodeBuilder;

    fn s(id: usize) -> StackSlot {
        StackSlot::new(id)
    }

    #[test]
    fn drain_zero_emits_no_pops() {
        let mut builder = BytecodeBuilder::new();
        emit_drain(0, &mut builder);
        assert!(builder.code().is_empty());
    }

    #[test]
    fn drain_emits_one_pop_per_item() {
        let mut builder = BytecodeBuilder::new();
        emit_drain(3, &mut builder);
        let code = builder.code();
        assert_eq!(code.len(), 3);
        assert!(code.iter().all(|&b| b == Opcode::POP.encode()));
    }

    #[test]
    fn drain_except_tos_zero_below_is_noop() {
        let mut builder = BytecodeBuilder::new();
        emit_drain_except_tos(0, &mut builder);
        assert!(builder.code().is_empty());
    }

    #[test]
    fn drain_except_tos_one_below_emits_swap1_pop() {
        let mut builder = BytecodeBuilder::new();
        emit_drain_except_tos(1, &mut builder);
        let code = builder.code();
        assert_eq!(code.len(), 2);
        assert_eq!(code[0], Opcode::SWAP1.encode());
        assert_eq!(code[1], Opcode::POP.encode());
    }

    #[test]
    fn drain_except_tos_two_below_emits_two_pairs() {
        let mut builder = BytecodeBuilder::new();
        emit_drain_except_tos(2, &mut builder);
        let code = builder.code();
        assert_eq!(code.len(), 4);
        assert_eq!(code[0], Opcode::SWAP1.encode());
        assert_eq!(code[1], Opcode::POP.encode());
        assert_eq!(code[2], Opcode::SWAP1.encode());
        assert_eq!(code[3], Opcode::POP.encode());
    }

    #[test]
    fn plan_shuffle_already_correct_is_noop() {
        let warm = HashSet::new();
        let current = vec![Some(s(0)), Some(s(1))];
        let target = vec![s(0), s(1)];
        let ops = plan_shuffle(&current, &target, &warm);
        assert!(ops.is_empty());
    }

    #[test]
    fn plan_shuffle_empty_to_empty_is_noop() {
        let warm = HashSet::new();
        assert!(plan_shuffle(&[], &[], &warm).is_empty());
    }

    #[test]
    fn plan_shuffle_drain_one_reload_one() {
        // current=[s(0)], target=[s(1)]: s(1) not on stack → MLoad, then SWAP(1)+POP.
        let warm = HashSet::new();
        let ops = plan_shuffle(&[Some(s(0))], &[s(1)], &warm);
        assert_eq!(
            ops,
            vec![ShuffleOp::MLoad(s(1)), ShuffleOp::Swap(1), ShuffleOp::Pop]
        );
    }

    #[test]
    fn plan_shuffle_target_is_prefix_dup_reuse() {
        // current=[s(0),s(1),s(2)], target=[s(0),s(1)]: both in current, use DUPs.
        // M=3, N=2, rotation=1. pre=[s(1),s(0)].
        // j=1: DUP s(0) from sim[0] → DUP(1). sim=[s(0),s(0),s(1),s(2)].
        // j=0: DUP s(1) from sim[2]   → DUP(3). sim=[s(1),s(0),s(0),s(1),s(2)].
        // 3×(SWAP(2)+POP) → [s(0),s(1)] ✓
        let warm = HashSet::new();
        let ops = plan_shuffle(&[Some(s(0)), Some(s(1)), Some(s(2))], &[s(0), s(1)], &warm);
        assert_eq!(
            ops,
            vec![
                ShuffleOp::Dup(1),
                ShuffleOp::Dup(3),
                ShuffleOp::Swap(2),
                ShuffleOp::Pop,
                ShuffleOp::Swap(2),
                ShuffleOp::Pop,
                ShuffleOp::Swap(2),
                ShuffleOp::Pop,
            ]
        );
    }

    #[test]
    fn plan_shuffle_empty_source_loads_target() {
        // current=[], target=[s(0),s(1)]: M=0, no SWAP+POP. MLoad deepest-first.
        // rotation=0, pre=[s(0),s(1)]. j=1: MLoad(s(1)). j=0: MLoad(s(0)).
        let warm = HashSet::new();
        let ops = plan_shuffle(&[], &[s(0), s(1)], &warm);
        assert_eq!(ops, vec![ShuffleOp::MLoad(s(1)), ShuffleOp::MLoad(s(0))]);
    }

    #[test]
    fn plan_shuffle_drain_all_no_target() {
        let warm = HashSet::new();
        let ops = plan_shuffle(&[Some(s(0)), Some(s(1))], &[], &warm);
        assert_eq!(ops, vec![ShuffleOp::Pop, ShuffleOp::Pop]);
    }

    #[test]
    fn plan_shuffle_permutation_two_elements() {
        // current=[s(0),s(1)], target=[s(1),s(0)]: simple swap.
        // M=2, N=2, rotation=0. pre=[s(1),s(0)].
        // j=1: DUP s(0) from sim[0] → DUP(1). sim=[s(0),s(0),s(1)].
        // j=0: DUP s(1) from sim[2]   → DUP(3). sim=[s(1),s(0),s(0),s(1)].
        // 2×(SWAP(2)+POP) → [s(1),s(0)] ✓
        let warm = HashSet::new();
        let ops = plan_shuffle(&[Some(s(0)), Some(s(1))], &[s(1), s(0)], &warm);
        assert_eq!(
            ops,
            vec![
                ShuffleOp::Dup(1),
                ShuffleOp::Dup(3),
                ShuffleOp::Swap(2),
                ShuffleOp::Pop,
                ShuffleOp::Swap(2),
                ShuffleOp::Pop,
            ]
        );
    }

    #[test]
    fn plan_shuffle_with_duplicate_for_branch_condition() {
        // current=[s(0),s(1)], target=[s(1),s(0),s(1)]:
        // condition=s(1) prepended to entry_layout=[s(0),s(1)].
        // M=2, N=3, rotation=2%3=2.
        // pre[j]=target[(j+3-2)%3]=target[(j+1)%3].
        // pre[0]=target[1]=s(0). pre[1]=target[2]=s(1). pre[2]=target[0]=s(1).
        // j=2: DUP s(1) from sim=[s(0),s(1)] at pos=1 → DUP(2). sim=[s(1),s(0),s(1)].
        // j=1: DUP s(1) from sim=[s(1),s(0),s(1)] at pos=0 → DUP(1). sim=[s(1),s(1),s(0),s(1)].
        // j=0: DUP s(0) from sim at pos=2 → DUP(3). sim=[s(0),s(1),s(1),s(0),s(1)].
        // 2×(SWAP(3)+POP) → [s(1),s(0),s(1)] ✓
        let warm = HashSet::new();
        let ops = plan_shuffle(&[Some(s(0)), Some(s(1))], &[s(1), s(0), s(1)], &warm);
        assert_eq!(
            ops,
            vec![
                ShuffleOp::Dup(2),
                ShuffleOp::Dup(1),
                ShuffleOp::Dup(3),
                ShuffleOp::Swap(3),
                ShuffleOp::Pop,
                ShuffleOp::Swap(3),
                ShuffleOp::Pop,
            ]
        );
    }

    #[test]
    fn shuffle_cost_dup_and_swap() {
        let warm = HashSet::new();
        let ops = vec![ShuffleOp::Dup(1), ShuffleOp::Swap(2), ShuffleOp::Pop];
        assert_eq!(shuffle_cost(&ops, &warm), 3 + 3 + 2);
    }

    #[test]
    fn shuffle_cost_cold_mload() {
        let warm = HashSet::new();
        assert_eq!(shuffle_cost(&[ShuffleOp::MLoad(s(0))], &warm), 21);
    }

    #[test]
    fn shuffle_cost_warm_mload() {
        let mut warm = HashSet::new();
        warm.insert(s(0));
        assert_eq!(shuffle_cost(&[ShuffleOp::MLoad(s(0))], &warm), 6);
    }
}
