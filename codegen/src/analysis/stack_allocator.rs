/// Stack register allocator
///
/// Assigns each StackSlot to either a fixed EVM stack position (1–16, used as
/// virtual registers) or to Memory.  The assignment is global and consistent
/// across every block in a function: a slot assigned to StackPos(3) is always
/// at DUP3-distance from TOS whenever it is live.
///
/// # Algorithm
///
/// 1. **Interference graph**: two slots interfere iff their live intervals overlap.
/// 2. **Greedy graph coloring**: slots sorted by interval-start; assign the
///    lowest color not taken by any already-colored neighbour.
///    Colors 0‥MAX_STACK_REGS-1 → StackPos(1)‥StackPos(MAX_STACK_REGS).
///    Overflow → Memory.
/// 3. **Entry / exit layouts**: for each block, collect its live-in / live-out
///    slots that have a StackPos assignment and sort them by position (TOS first).
///
/// # Why no recursion matters
///
/// Because Merak forbids recursion, the CFG is a finite DAG-with-back-edges
/// (only natural loops).  This means the live intervals are bounded and the
/// coloring result is fully deterministic at compile time.
use std::collections::{HashMap, HashSet};

use merak_ir::{ssa_ir::BlockId, ControlFlowGraph};

use crate::analysis::liveness::LivenessInfo;
use crate::lowering::{LoweredCfg, StackSlot};

/// Maximum EVM stack positions available as virtual registers.
///
/// DUP1–DUP16 = 16 positions; we reserve one slot for the continuation address
/// that the calling convention keeps at the bottom of the frame.
pub const MAX_STACK_REGS: usize = 15;

/// The compiled location assigned to a single stack slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotLocation {
    /// Slot lives on the EVM stack at position `n` (1 = TOS / DUP1, 15 = DUP15).
    StackPos(usize),
    /// Slot lives in EVM Memory at `frame_base + slot_id * 32` (MLOAD / MSTORE).
    Memory,
}

/// Result of stack register allocation for a single function.
#[derive(Debug, Clone)]
pub struct StackAllocation {
    /// Location assigned to every slot that appears in the function.
    pub assignment: HashMap<StackSlot, SlotLocation>,
    /// Canonical EVM stack layout expected at each block's entry (TOS-first).
    /// Contains only stack-assigned slots from live_in[B].
    pub entry_layout: HashMap<BlockId, Vec<StackSlot>>,
    /// Canonical EVM stack layout expected at each block's exit (TOS-first).
    /// Contains only stack-assigned slots from live_out[B].
    pub exit_layout: HashMap<BlockId, Vec<StackSlot>>,
}

/// Run the stack register allocator for one lowered function CFG.
pub fn allocate(cfg: &LoweredCfg, liveness: &LivenessInfo) -> StackAllocation {
    let assignment = color_slots(liveness);
    let entry_layout = derive_layouts(cfg, &liveness.live_in, &assignment);
    let exit_layout = derive_layouts(cfg, &liveness.live_out, &assignment);
    StackAllocation {
        assignment,
        entry_layout,
        exit_layout,
    }
}

/// Assign a color (stack position or Memory) to every slot via greedy coloring.
///
/// Slots are processed in order of interval start so that the earliest-live
/// slots get the lowest (TOS-closest) positions, which typically yields the
/// most DUP1-heavy (cheapest) access patterns.
fn color_slots(liveness: &LivenessInfo) -> HashMap<StackSlot, SlotLocation> {
    // Sort all slots by interval start, breaking ties by slot id.
    let mut slots: Vec<StackSlot> = liveness.intervals.keys().copied().collect();
    slots.sort_by_key(|s| (liveness.intervals[s].start, s.0));

    let mut color: HashMap<StackSlot, usize> = HashMap::with_capacity(slots.len());

    for &slot in &slots {
        let iv = &liveness.intervals[&slot];

        // Collect colors used by slots whose interval overlaps `iv`.
        let used: HashSet<usize> = color
            .iter()
            .filter(|(&other, _)| liveness.intervals[&other].overlaps(iv))
            .map(|(_, &c)| c)
            .collect();

        // Pick the lowest free color.
        let c = (0..).find(|c| !used.contains(c)).unwrap();
        color.insert(slot, c);
    }

    // Convert color indices to SlotLocation.
    color
        .into_iter()
        .map(|(slot, c)| {
            let loc = if c < MAX_STACK_REGS {
                SlotLocation::StackPos(c + 1) // 1-based (DUP1 = position 1)
            } else {
                SlotLocation::Memory
            };
            (slot, loc)
        })
        .collect()
}

// ── Layout derivation ─────────────────────────────────────────────────────────

/// Build a per-block layout map from a liveness set (live_in or live_out).
///
/// For each block, the layout is the subset of that block's live set that was
/// assigned to a stack position, sorted ascending by position (position 1 = TOS
/// is first in the `Vec`).
fn derive_layouts(
    cfg: &LoweredCfg,
    live: &HashMap<BlockId, HashSet<StackSlot>>,
    assignment: &HashMap<StackSlot, SlotLocation>,
) -> HashMap<BlockId, Vec<StackSlot>> {
    cfg.block_ids()
        .into_iter()
        .map(|block_id| {
            let layout = stack_positioned_slots(&live[&block_id], assignment);
            (block_id, layout)
        })
        .collect()
}

/// Extract the stack-assigned slots from `set`, sorted by position (TOS first).
fn stack_positioned_slots(
    set: &HashSet<StackSlot>,
    assignment: &HashMap<StackSlot, SlotLocation>,
) -> Vec<StackSlot> {
    let mut positioned: Vec<(usize, StackSlot)> = set
        .iter()
        .filter_map(|&slot| match assignment.get(&slot) {
            Some(SlotLocation::StackPos(pos)) => Some((*pos, slot)),
            _ => None,
        })
        .collect();
    positioned.sort_by_key(|&(pos, _)| pos);
    positioned.into_iter().map(|(_, slot)| slot).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::liveness::LiveInterval;

    fn slot(id: usize) -> StackSlot {
        StackSlot::new(id)
    }

    fn iv(start: usize, end: usize) -> LiveInterval {
        LiveInterval { start, end }
    }

    fn make_intervals(pairs: &[(StackSlot, LiveInterval)]) -> HashMap<StackSlot, LiveInterval> {
        pairs.iter().copied().collect()
    }

    fn make_liveness(intervals: HashMap<StackSlot, LiveInterval>) -> LivenessInfo {
        LivenessInfo {
            live_in: Default::default(),
            live_out: Default::default(),
            intervals,
        }
    }

    // ── color_slots ───────────────────────────────────────────────────────────

    #[test]
    fn non_overlapping_slots_share_same_color() {
        // slot A: [0,5], slot B: [7,12] — disjoint → same color → same StackPos
        let liveness = make_liveness(make_intervals(&[(slot(0), iv(0, 5)), (slot(1), iv(7, 12))]));
        let assignment = color_slots(&liveness);
        assert_eq!(assignment[&slot(0)], assignment[&slot(1)]);
        assert!(matches!(assignment[&slot(0)], SlotLocation::StackPos(_)));
    }

    #[test]
    fn overlapping_slots_get_different_positions() {
        // slot A: [0,10], slot B: [5,15] — overlap → different colors
        let liveness = make_liveness(make_intervals(&[
            (slot(0), iv(0, 10)),
            (slot(1), iv(5, 15)),
        ]));
        let assignment = color_slots(&liveness);
        assert_ne!(assignment[&slot(0)], assignment[&slot(1)]);
        assert!(matches!(assignment[&slot(0)], SlotLocation::StackPos(_)));
        assert!(matches!(assignment[&slot(1)], SlotLocation::StackPos(_)));
    }

    #[test]
    fn overflow_beyond_max_regs_goes_to_memory() {
        // Create MAX_STACK_REGS + 1 slots that all overlap → the last one spills to Memory.
        let pairs: Vec<(StackSlot, LiveInterval)> = (0..=MAX_STACK_REGS)
            .map(|i| (slot(i), iv(0, 100)))
            .collect();
        let liveness = make_liveness(make_intervals(&pairs));
        let assignment = color_slots(&liveness);

        let memory_count = assignment
            .values()
            .filter(|&&l| l == SlotLocation::Memory)
            .count();
        assert_eq!(memory_count, 1, "exactly one slot should spill to Memory");
    }

    #[test]
    fn three_slots_two_colors() {
        // A: [0,5], B: [6,10], C: [0,10]
        // A and B don't overlap with each other but both overlap with C.
        // → A and B share a color; C gets a different color. Total: 2 colors.
        let liveness = make_liveness(make_intervals(&[
            (slot(0), iv(0, 5)),  // A
            (slot(1), iv(6, 10)), // B
            (slot(2), iv(0, 10)), // C — overlaps A and B
        ]));
        let assignment = color_slots(&liveness);

        assert_eq!(
            assignment[&slot(0)],
            assignment[&slot(1)],
            "A and B share a color"
        );
        assert_ne!(assignment[&slot(0)], assignment[&slot(2)], "A and C differ");
        assert_ne!(assignment[&slot(1)], assignment[&slot(2)], "B and C differ");

        let distinct: HashSet<_> = assignment.values().collect();
        assert_eq!(distinct.len(), 2, "only 2 distinct locations needed");
    }

    #[test]
    fn single_slot_gets_stack_pos_1() {
        let liveness = make_liveness(make_intervals(&[(slot(0), iv(0, 5))]));
        let assignment = color_slots(&liveness);
        assert_eq!(assignment[&slot(0)], SlotLocation::StackPos(1));
    }

    // ── stack_positioned_slots ────────────────────────────────────────────────

    #[test]
    fn layout_sorted_by_position_tos_first() {
        let set: HashSet<StackSlot> = [slot(0), slot(1), slot(2)].into_iter().collect();
        let assignment: HashMap<StackSlot, SlotLocation> = [
            (slot(0), SlotLocation::StackPos(3)),
            (slot(1), SlotLocation::StackPos(1)),
            (slot(2), SlotLocation::StackPos(2)),
        ]
        .into_iter()
        .collect();

        let layout = stack_positioned_slots(&set, &assignment);
        // Should be sorted: pos 1 (slot 1), pos 2 (slot 2), pos 3 (slot 0)
        assert_eq!(layout, vec![slot(1), slot(2), slot(0)]);
    }

    #[test]
    fn memory_slots_excluded_from_layout() {
        let set: HashSet<StackSlot> = [slot(0), slot(1)].into_iter().collect();
        let assignment: HashMap<StackSlot, SlotLocation> = [
            (slot(0), SlotLocation::StackPos(1)),
            (slot(1), SlotLocation::Memory),
        ]
        .into_iter()
        .collect();

        let layout = stack_positioned_slots(&set, &assignment);
        assert_eq!(layout, vec![slot(0)]);
    }
}
