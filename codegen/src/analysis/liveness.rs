/// Liveness analysis for stack slot allocation
///
/// Computes live ranges for stack slots to enable efficient slot reuse.
///
/// For each basic block B the analysis produces:
///   live_in[B]  — slots live at the entry of B (will be read on some path from B)
///   live_out[B] — slots live at the exit of B (will be read on some path from B's successors)
///   intervals   — live interval for each slot (first–last instruction index in BFS order)
use merak_ir::{
    ssa_ir::{BlockId, Operand, Terminator},
    ControlFlowGraph, Instruction,
};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::lowering::{LoweredBlock, LoweredCfg, LoweredOperand, LoweredTerminator, StackSlot};

/// Liveness information computed for a single function's CFG.
#[derive(Debug, Clone)]
pub struct LivenessInfo {
    /// Slots live at the entry of each block.
    pub live_in: HashMap<BlockId, HashSet<StackSlot>>,
    /// Slots live at the exit of each block.
    pub live_out: HashMap<BlockId, HashSet<StackSlot>>,
    /// Global live interval (first–last instruction index in BFS order) for each slot.
    pub intervals: HashMap<StackSlot, LiveInterval>,
}

/// Contiguous range of global instruction indices during which a slot is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveInterval {
    /// Index of the first instruction where this slot is defined or used.
    pub start: usize,
    /// Index of the last instruction where this slot is used.
    pub end: usize,
}

impl LiveInterval {
    pub fn overlaps(&self, other: &LiveInterval) -> bool {
        self.start <= other.end && other.start <= self.end
    }
}

/// Extract the `StackSlot` from a lowered operand.
fn slot_of_operand(op: &LoweredOperand) -> Option<StackSlot> {
    match op {
        Operand::Location(slot) => Some(*slot),
        Operand::Constant(_) => None,
    }
}

/// Return all `StackSlot`s READ by a block terminator.
///
/// Terminators never define a slot — they only consume values — so they
/// contribute exclusively to the GEN set of their enclosing block.
fn terminator_uses(term: &LoweredTerminator) -> Vec<StackSlot> {
    match term {
        Terminator::Branch { condition, .. } => slot_of_operand(condition).into_iter().collect(),
        Terminator::Return { value: Some(v), .. } => slot_of_operand(v).into_iter().collect(),
        Terminator::Return { value: None, .. }
        | Terminator::Jump { .. }
        | Terminator::Unreachable => vec![],
    }
}

/// Compute the GEN and KILL sets for a single basic block.
///
/// - **GEN**: slots the block B needs from its predecessors (reads before written in B).
/// - **KILL**: stack slots that are written in the block
fn compute_gen_kill(block: &LoweredBlock) -> (HashSet<StackSlot>, HashSet<StackSlot>) {
    let mut gen: HashSet<StackSlot> = HashSet::new();
    let mut kill: HashSet<StackSlot> = HashSet::new();

    for inst in &block.instructions {
        for op in inst.operands() {
            if let Some(slot) = slot_of_operand(op) {
                if !kill.contains(&slot) {
                    gen.insert(slot);
                }
            }
        }

        if let Some(dest) = inst.destination() {
            kill.insert(dest);
        }
    }

    // Terminators have no destination -> GEN only.
    for slot in terminator_uses(&block.terminator) {
        if !kill.contains(&slot) {
            gen.insert(slot);
        }
    }

    (gen, kill)
}

/// Extend (or initialise) the live interval of `slot` to include index `idx`.
fn extend_interval(intervals: &mut HashMap<StackSlot, LiveInterval>, slot: StackSlot, idx: usize) {
    intervals
        .entry(slot)
        .and_modify(|iv| {
            if idx < iv.start {
                iv.start = idx;
            }
            if idx > iv.end {
                iv.end = idx;
            }
        })
        .or_insert(LiveInterval {
            start: idx,
            end: idx,
        });
}

/// Assign a contiguous instruction-index range to each block via BFS from the entry.
///
/// Each block B receives `[block_start, block_end]` where:
///   block_start = cumulative instruction count before B
///   block_end   = block_start + |B.instructions|  (terminator index)
fn bfs_block_ranges(cfg: &LoweredCfg) -> HashMap<BlockId, (usize, usize)> {
    let mut block_ranges: HashMap<BlockId, (usize, usize)> = HashMap::new();
    let mut current_idx: usize = 0;
    let mut visited: HashSet<BlockId> = HashSet::new();
    let mut queue: VecDeque<BlockId> = VecDeque::new();

    queue.push_back(cfg.entry());
    visited.insert(cfg.entry());

    while let Some(block_id) = queue.pop_front() {
        let block = &cfg.blocks[&block_id];
        let block_start = current_idx;

        current_idx += block.instructions.len() + 1;
        let block_end = current_idx - 1;
        block_ranges.insert(block_id, (block_start, block_end));

        for succ in cfg.successors(block_id) {
            if !visited.contains(&succ) {
                visited.insert(succ);
                queue.push_back(succ);
            }
        }
    }

    block_ranges
}

/// Derive per-slot live intervals from the block-level liveness sets.
fn compute_intervals(
    cfg: &LoweredCfg,
    live_in: &HashMap<BlockId, HashSet<StackSlot>>,
    live_out: &HashMap<BlockId, HashSet<StackSlot>>,
) -> HashMap<StackSlot, LiveInterval> {
    let block_ranges = bfs_block_ranges(cfg);

    let mut intervals: HashMap<StackSlot, LiveInterval> = HashMap::new();

    for (block_id, (block_start, block_end)) in &block_ranges {
        let block = &cfg.blocks[block_id];

        // Slots live at block entry span from block_start at the earliest.
        for &slot in &live_in[block_id] {
            extend_interval(&mut intervals, slot, *block_start);
        }

        // Slots live at block exit span until block_end at the latest.
        for &slot in &live_out[block_id] {
            extend_interval(&mut intervals, slot, *block_end);
        }

        // Walk instructions to narrow the interval to precise use/def points.
        for (i, inst) in block.instructions.iter().enumerate() {
            let inst_idx = block_start + i;
            for op in inst.operands() {
                if let Some(slot) = slot_of_operand(op) {
                    extend_interval(&mut intervals, slot, inst_idx);
                }
            }
            if let Some(dest) = inst.destination() {
                extend_interval(&mut intervals, dest, inst_idx);
            }
        }

        // Terminator uses live until block_end.
        for slot in terminator_uses(&block.terminator) {
            extend_interval(&mut intervals, slot, *block_end);
        }
    }

    intervals
}

/// Compute liveness information for a lowered CFG function.
pub fn analyze_liveness(cfg: &LoweredCfg) -> LivenessInfo {
    let gen_kill: HashMap<BlockId, (HashSet<StackSlot>, HashSet<StackSlot>)> = cfg
        .block_ids()
        .into_iter()
        .map(|id| (id, compute_gen_kill(&cfg.blocks[&id])))
        .collect();

    // live_in[B] = all slots that need to be available at the start of B (B or some succesor of B need it).
    let mut live_in: HashMap<BlockId, HashSet<StackSlot>> = cfg
        .block_ids()
        .into_iter()
        .map(|id| (id, HashSet::new()))
        .collect();

    // live_out[B] =all slots that need to be available at the end of B (some succesor need it).
    let mut live_out: HashMap<BlockId, HashSet<StackSlot>> = cfg
        .block_ids()
        .into_iter()
        .map(|id| (id, HashSet::new()))
        .collect();

    let mut worklist: VecDeque<BlockId> = cfg.block_ids().into_iter().collect();
    let mut in_worklist: HashSet<BlockId> = worklist.iter().copied().collect();

    while let Some(block_id) = worklist.pop_front() {
        in_worklist.remove(&block_id);

        // live_out[B] = live_in[S] for all successors S
        let new_live_out: HashSet<StackSlot> = cfg
            .successors(block_id)
            .iter()
            .flat_map(|s| live_in[s].iter().copied())
            .collect();

        // live_in[B] = GEN[B] U (live_out[B] − KILL[B])
        let (gen, kill) = &gen_kill[&block_id];
        let new_live_in: HashSet<StackSlot> = gen
            .iter()
            .copied()
            .chain(new_live_out.iter().copied().filter(|s| !kill.contains(s)))
            .collect();

        if new_live_in != live_in[&block_id] || new_live_out != live_out[&block_id] {
            live_in.insert(block_id, new_live_in);
            live_out.insert(block_id, new_live_out);

            for pred in cfg.predecessors(block_id) {
                if !in_worklist.contains(&pred) {
                    worklist.push_back(pred);
                    in_worklist.insert(pred);
                }
            }
        }
    }

    let intervals = compute_intervals(cfg, &live_in, &live_out);

    LivenessInfo {
        live_in,
        live_out,
        intervals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merak_ir::ssa_ir::{Constant, Operand, Terminator};

    #[test]
    fn interval_overlap_touching() {
        // [0,5] and [5,8] share index 5 → overlap
        let a = LiveInterval { start: 0, end: 5 };
        let b = LiveInterval { start: 5, end: 8 };
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
    }

    #[test]
    fn interval_no_overlap() {
        let a = LiveInterval { start: 0, end: 4 };
        let b = LiveInterval { start: 5, end: 9 };
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }

    #[test]
    fn interval_contained() {
        let outer = LiveInterval { start: 0, end: 10 };
        let inner = LiveInterval { start: 3, end: 7 };
        assert!(outer.overlaps(&inner));
    }

    #[test]
    fn slot_of_location_operand() {
        let slot = StackSlot::new(3);
        let op: LoweredOperand = Operand::Location(slot);
        assert_eq!(slot_of_operand(&op), Some(slot));
    }

    #[test]
    fn slot_of_constant_operand_is_none() {
        let op: LoweredOperand = Operand::Constant(Constant::Int(42));
        assert_eq!(slot_of_operand(&op), None);
    }

    #[test]
    fn terminator_uses_jump_is_empty() {
        let term: LoweredTerminator = Terminator::Jump { target: 1 };
        assert!(terminator_uses(&term).is_empty());
    }

    #[test]
    fn terminator_uses_unreachable_is_empty() {
        let term: LoweredTerminator = Terminator::Unreachable;
        assert!(terminator_uses(&term).is_empty());
    }

    #[test]
    fn terminator_uses_return_none_is_empty() {
        let term: LoweredTerminator = Terminator::Return {
            value: None,
            meta: (),
        };
        assert!(terminator_uses(&term).is_empty());
    }

    #[test]
    fn terminator_uses_return_slot() {
        let slot = StackSlot::new(5);
        let term: LoweredTerminator = Terminator::Return {
            value: Some(Operand::Location(slot)),
            meta: (),
        };
        assert_eq!(terminator_uses(&term), vec![slot]);
    }

    #[test]
    fn terminator_uses_return_constant_is_empty() {
        let term: LoweredTerminator = Terminator::Return {
            value: Some(Operand::Constant(Constant::Bool(true))),
            meta: (),
        };
        assert!(terminator_uses(&term).is_empty());
    }

    #[test]
    fn terminator_uses_branch_slot() {
        let slot = StackSlot::new(2);
        let term: LoweredTerminator = Terminator::Branch {
            condition: Operand::Location(slot),
            then_block: 1,
            else_block: 2,
            meta: (),
        };
        assert_eq!(terminator_uses(&term), vec![slot]);
    }

    #[test]
    fn terminator_uses_branch_constant_is_empty() {
        let term: LoweredTerminator = Terminator::Branch {
            condition: Operand::Constant(Constant::Bool(false)),
            then_block: 1,
            else_block: 2,
            meta: (),
        };
        assert!(terminator_uses(&term).is_empty());
    }

    #[test]
    fn extend_interval_creates_new() {
        let mut intervals = HashMap::new();
        let slot = StackSlot::new(0);
        extend_interval(&mut intervals, slot, 7);
        assert_eq!(intervals[&slot], LiveInterval { start: 7, end: 7 });
    }

    #[test]
    fn extend_interval_expands_start() {
        let mut intervals = HashMap::new();
        let slot = StackSlot::new(0);
        extend_interval(&mut intervals, slot, 5);
        extend_interval(&mut intervals, slot, 2); // earlier
        assert_eq!(intervals[&slot].start, 2);
        assert_eq!(intervals[&slot].end, 5);
    }

    #[test]
    fn extend_interval_expands_end() {
        let mut intervals = HashMap::new();
        let slot = StackSlot::new(0);
        extend_interval(&mut intervals, slot, 3);
        extend_interval(&mut intervals, slot, 9); // later
        assert_eq!(intervals[&slot].start, 3);
        assert_eq!(intervals[&slot].end, 9);
    }
}
