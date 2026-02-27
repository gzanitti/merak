pub mod ssa_ir;
pub mod transformers;

// Re-export commonly used types for convenience
pub use ssa_ir::{
    BasicBlock, BlockId, CallTarget, Constant, Instruction, Operand, Register, SsaBlock, SsaCfg,
    SsaInstruction, SsaOperand, SsaTerminator, Terminator, TerminatorMeta,
};

use std::collections::HashSet;

/// Common interface for control flow graphs.
///
/// Implementors provide the four primitive graph operations; derived algorithms
/// (reverse postorder, reachability, etc.) are available as default methods.
pub trait ControlFlowGraph {
    fn entry(&self) -> BlockId;
    fn successors(&self, block: BlockId) -> Vec<BlockId>;
    fn predecessors(&self, block: BlockId) -> Vec<BlockId>;
    fn block_ids(&self) -> Vec<BlockId>;

    /// Reverse postorder — each block appears after all its predecessors.
    fn reverse_post_order(&self) -> Vec<BlockId> {
        let mut visited = HashSet::new();
        let mut postorder = Vec::new();
        self.postorder_dfs(self.entry(), &mut visited, &mut postorder);
        postorder.reverse();
        postorder
    }

    /// Recursive postorder DFS.
    fn postorder_dfs(
        &self,
        block: BlockId,
        visited: &mut HashSet<BlockId>,
        out: &mut Vec<BlockId>,
    ) {
        if visited.insert(block) {
            for succ in self.successors(block) {
                self.postorder_dfs(succ, visited, out);
            }
            out.push(block);
        }
    }
}
