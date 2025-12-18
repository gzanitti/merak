pub type StateId = SymbolId;
pub type ContractId = SymbolId;
pub type BlockId = usize;
pub type LoopId = usize;

// ============================================================================
// OPERANDS AND VALUES
// ============================================================================

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use indexmap::IndexMap;
use merak_ast::contract::InterfaceDecl;
use merak_ast::predicate::{Predicate, RefinementExpr};
use merak_ast::types::{BaseType, Type};
use merak_ast::{
    expression::{BinaryOperator, Expression, UnaryOperator},
    meta::SourceRef,
    statement::{StateConst, StateVar},
    Import, NodeId,
};
use merak_symbols::SymbolId;
use primitive_types::H256;

/// Operand of an instruction (input value)
#[derive(Debug, Clone)]
pub enum Operand {
    Register(Register),
    Constant(Constant),
}

impl Operand {
    pub fn symbol_id(&self) -> Option<SymbolId> {
        match self {
            Operand::Register(r) => Some(r.symbol),
            Operand::Constant(_) => None,
        }
    }
}

/// SSA-versioned register
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct Register {
    pub symbol: SymbolId, // Links to symbol table entry
    pub version: usize,   // SSA version: x₀, x₁, x₂, ...
}

impl std::fmt::Display for Register {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}_{}", self.symbol, self.version)
    }
}

#[derive(Debug, Clone)]
pub enum Constant {
    Int(i64),
    Bool(bool),
    Address(H256),
    String(String), // TODO: Decimal, etc.
}

// ============================================================================
// SSA INSTRUCTIONS
// ============================================================================

#[derive(Clone)]
pub enum SsaInstruction {
    // ------------------------------------------------------------------------
    // DATA MOVEMENT
    // ------------------------------------------------------------------------
    /// Simple copy: dest = source
    Copy {
        dest: Register,
        source: Operand,
        source_ref: SourceRef,
    },

    /// Phi node - merges values from different predecessor blocks
    /// Used at control flow join points in SSA form
    Phi {
        dest: Register,
        /// (predecessor_block, register_from_that_block)
        sources: Vec<(BlockId, Register)>,
    },

    // ------------------------------------------------------------------------
    // ARITHMETIC & LOGIC
    // ------------------------------------------------------------------------
    /// Binary operation: dest = left op right
    BinaryOp {
        dest: Register,
        op: BinaryOperator,
        left: Operand,
        right: Operand,
        source_ref: SourceRef,
    },

    /// Unary operation: dest = op operand
    UnaryOp {
        dest: Register,
        op: UnaryOperator,
        operand: Operand,
        source_ref: SourceRef,
    },

    // ------------------------------------------------------------------------
    // STORAGE (state variables)
    // ------------------------------------------------------------------------
    /// Reads a state variable/constant
    /// State variables/constants are declared with "state var"/"state const" keyword
    StorageLoad {
        dest: Register,
        var: SymbolId, // The state variable symbol
        source_ref: SourceRef,
    },

    /// Writes to a state variable
    StorageStore {
        var: SymbolId,
        value: Operand,
        source_ref: SourceRef,
    },

    // ------------------------------------------------------------------------
    // FUNCTION CALLS
    // ------------------------------------------------------------------------
    /// Function call (internal or external)
    Call {
        dest: Option<Register>, // None if function is void
        target: CallTarget,
        args: Vec<Operand>,
        source_ref: SourceRef,
    },

    // ------------------------------------------------------------------------
    // STORAGE ANALYSIS
    // ------------------------------------------------------------------------
    // Unfold storage abstraction (transition: Folded → Unfolded)
    /// Semantics: Assume invariant is valid, expose concrete value
    Unfold {
        var: SymbolId,
        source_ref: SourceRef,
    },
    
    /// Fold storage abstraction (transition: Unfolded → Folded)
    /// Semantics: Verify invariant still holds, close abstraction
    Fold {
        var: SymbolId,
        source_ref: SourceRef,
    },



    // ------------------------------------------------------------------------
    // VERIFICATION & ASSERTIONS
    // ------------------------------------------------------------------------
    /// Assertion for formal verification
    /// Different kinds have different verification semantics:
    /// - Precondition: assumed at entry (caller must prove)
    /// - Postcondition: must be proven at exit (callee guarantees)
    /// - UserAssert: explicit assertion in code
    Assert {
        condition: Operand,
        kind: AssertKind,
        source_ref: SourceRef,
    },
    // ------------------------------------------------------------------------
    // FUTURE EXTENSIONS (commented out - add when needed)
    // ------------------------------------------------------------------------

    // /// Emit event/log
    // /// TODO: Add when implementing event system
    // EmitEvent {
    //     event: EventId,
    //     args: Vec<Operand>,
    //     source_ref: SourceRef,
    // },
    //
    // /// Built-in function call (keccak256, ecrecover, etc.)
    // /// TODO: Add when implementing built-in functions
    // /// Separate from regular calls because built-ins have special semantics:
    // /// - They're pure (no side effects)
    // /// - Cannot re-enter
    // /// - Have known gas costs
    // BuiltinCall {
    //     dest: Register,
    //     builtin: BuiltinFunction,
    //     args: Vec<Operand>,
    //     source_ref: SourceRef,
    // },
    //
    // /// Memory array/struct access
    // /// TODO: Add when implementing complex data types
    // MemoryLoad {
    //     dest: Register,
    //     base: Register,
    //     offset: Operand,
    //     source_ref: SourceRef,
    // },
    //
    // MemoryStore {
    //     base: Register,
    //     offset: Operand,
    //     value: Operand,
    //     source_ref: SourceRef,
    // },
}

impl SsaInstruction {
    pub fn dest_register(&self) -> Option<Register> {
        match self {
            SsaInstruction::Copy { dest, .. }
            | SsaInstruction::Phi { dest, .. }
            | SsaInstruction::BinaryOp { dest, .. }
            | SsaInstruction::UnaryOp { dest, .. }
            | SsaInstruction::StorageLoad { dest, .. } => Some(*dest),

            SsaInstruction::Call { dest, .. } => *dest,

            SsaInstruction::StorageStore { .. }
            | SsaInstruction::Fold { .. } | SsaInstruction::Unfold { .. }
            | SsaInstruction::Assert { .. } => None,
        }
    }
}

// ============================================================================
// CALL TARGETS
// ============================================================================

#[derive(Debug, Clone)]
pub enum CallTarget {
    /// Function in the same contract
    Internal(SymbolId),

    /// Function in an imported contract
    External {
        object: Operand,          
        method: SymbolId,          // La función específica en esa interface/contrato
    },
}

// ============================================================================
// ASSERTION KINDS
// ============================================================================

#[derive(Debug, Clone, Copy)]
pub enum AssertKind {
    // /// From "requires" clause in function signature
    // /// Verification: ASSUMED at function entry (caller must prove)
    // Precondition,

    // /// From "ensures" clause in function signature
    // /// Verification: MUST BE PROVEN at function exit (callee guarantees)
    // Postcondition,
    /// Explicit assert statement in user code
    /// Verification: Must be proven at assertion point
    UserAssert,
    // /// Runtime check for refinement types from external sources
    // /// COMMENTED OUT - Will be needed soon for external call return values
    // ///
    // /// Why needed: When calling external contracts (especially non-Merak contracts
    // /// where we only see bytecode), we cannot statically verify refinement types.
    // /// We need runtime checks to ensure returned values satisfy constraints.
    // ///
    // /// Example:
    // /// ```
    // /// let x: {v: int | v > 0} = untrustedContract.getValue();
    // /// // ↑ Need runtime check: assert(x > 0, "refinement violation")
    // /// ```
    // ///
    // /// Why commented: Complex to implement correctly:
    // /// - For Merak-to-Merak calls: can verify statically, no runtime check needed
    // /// - For Merak-to-bytecode calls: need runtime checks, but requires:
    // ///   * ABI decoding with refinement constraints
    // ///   * Proper error handling for violations
    // ///   * Gas cost considerations
    // ///
    // /// TODO: Implement when adding external contract interaction support
    // RefinementCheck,
}

// ============================================================================
// TERMINATORS (control flow)
// ============================================================================

/// Terminator instruction - exactly one per basic block
#[derive(Debug, Clone)]
pub enum Terminator {
    /// Unconditional jump: goto target
    Jump { target: BlockId },

    /// Conditional branch: if condition then goto then_block else goto else_block
    Branch {
        condition: Operand,
        then_block: BlockId,
        else_block: BlockId,
        invariants: Vec<Predicate>, // empty for ifs, >= 1 for loops
        variants: Vec<RefinementExpr>, // empty for ifs, >= 1 for loops
        source_ref: SourceRef,
    },

    /// Return from function
    Return {
        value: Option<Operand>, // None for void functions
        source_ref: SourceRef,
    },

    /// Unreachable code (for dead code analysis)
    Unreachable,
}

// ============================================================================
// BASIC BLOCK
// ============================================================================

pub struct BasicBlock {
    pub id: BlockId,

    /// Sequential instructions (no control flow)
    pub instructions: Vec<SsaInstruction>,

    /// Control flow
    pub terminator: Terminator,

    // Metadata for analysis and traversal
    pub predecessors: Vec<BlockId>,
    pub successors: Vec<BlockId>,

    // Metada for loop checking
    // Lives here to avoid duplicate information
    pub loop_invariants: Option<Vec<Predicate>>,
    pub loop_variants: Option<Vec<RefinementExpr>>,
}

// ============================================================================
// CFG
// ============================================================================

pub struct SsaCfg {
    pub name: String,
    pub function_id: SymbolId,
    pub blocks: HashMap<BlockId, BasicBlock>,
    pub entry: BlockId,
    pub next_id: usize,

    // Function parameters (live-in at entry)
    /// These are not "defined" by any instruction, they simply exist.
    pub parameters: Vec<SymbolId>,

    /// Requires clauses for the function
    pub requires: Vec<Predicate>, // TODO Better representation

    /// Ensures clauses for the function
    pub ensures: Vec<Predicate>,

    /// Dominance information (computed after CFG construction)
    pub dominance: Option<DominanceInfo>,

    /// Loop information (computed after dominance analysis)
    pub loops: Option<LoopForest>,

    pub local_temps: HashMap<Register, BaseType>,

    /// Cached reverse post-order traversal
    rpo_cache: OnceLock<Vec<BlockId>>,

    /// Cached RPO indices for fast dominance computation
    rpo_indices_cache: OnceLock<HashMap<BlockId, usize>>,    
}

impl SsaCfg {
    pub fn new(name: String, function_id: SymbolId) -> Self {
        Self {
            name,
            function_id,
            blocks: HashMap::new(),
            entry: 0,
            next_id: 0,
            parameters: Vec::new(),
            requires: Vec::new(),
            ensures: Vec::new(),
            dominance: None,
            loops: None,
            local_temps: HashMap::new(),
            rpo_cache: OnceLock::new(),
            rpo_indices_cache: OnceLock::new(),
        }
    }

    pub fn new_block(&mut self) -> BlockId {
        let id = self.next_id;
        self.next_id += 1;
        self.blocks.insert(
            id,
            BasicBlock {
                id,
                instructions: Vec::new(),
                terminator: Terminator::Unreachable,
                predecessors: Vec::new(),
                successors: Vec::new(),
                loop_invariants: None,
                loop_variants: None,
            },
        );
        id
    }

    pub fn add_instruction_at(&mut self, block_id: usize, instruction: SsaInstruction) {
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.instructions.push(instruction);
        }
    }

    pub fn add_instruction(&mut self, instruction: SsaInstruction) {
        if let Some(block) = self.blocks.get_mut(&(&(self.next_id) - 1)) {
            block.instructions.push(instruction);
        }
    }

    pub fn add_terminator_at(&mut self, block_id: usize, terminator: Terminator) {
        if let Some(block) = self.blocks.get_mut(&block_id) {
            block.terminator = terminator;
        }
    }

    pub fn add_terminator(&mut self, terminator: Terminator) {
        if let Some(block) = self.blocks.get_mut(&(&(self.next_id) - 1)) {
            block.terminator = terminator;
        }
    }

    pub fn add_edge(&mut self, from: usize, to: usize) {
        if let Some(block) = self.blocks.get_mut(&from) {
            if !block.successors.contains(&to) {
                block.successors.push(to);
            }
        }

        if let Some(block) = self.blocks.get_mut(&to) {
            if !block.predecessors.contains(&from) {
                block.predecessors.push(from);
            }
        }
    }

    pub fn add_param(&mut self, param: SymbolId) {
        self.parameters.push(param);
    }

    pub fn exit_blocks(&self) -> Vec<BlockId> {
        self.blocks.keys().copied().filter(|id| matches!(self.blocks[id].terminator, Terminator::Return{..})).collect()
    }

    pub fn reverse_post_order(&self) -> &Vec<BlockId> {
        self.rpo_cache.get_or_init(|| {
            let mut visited = HashSet::new();
            let mut post_order = Vec::new();

            self.post_order_dfs(self.entry, &mut visited, &mut post_order);

            post_order.reverse();
            post_order
        })
    }

    fn post_order_dfs(
        &self,
        block_id: usize,
        visited: &mut HashSet<BlockId>,
        post_order: &mut Vec<BlockId>,
    ) {
        if !visited.contains(&block_id) {
            visited.insert(block_id);
            let successors: Vec<_> = self.blocks[&block_id].successors.iter().copied().collect();
            for succ in successors {
                self.post_order_dfs(succ, visited, post_order);
            }
            post_order.push(block_id);
        }
    }

    pub fn compute_dominance(&mut self) {
        let rpo = self.reverse_post_order();
        let mut idom: HashMap<BlockId, BlockId> = HashMap::new();

        // Entry dominates itself
        idom.insert(self.entry, self.entry);

        let mut changed = true;
        while changed {
            changed = false;

            for &block in rpo.iter().skip(1) {
                let preds = &self.blocks[&block].predecessors;
                let new_idom = self.compute_idom_for_block(preds, &idom);

                if let Some(new_val) = new_idom {
                    if idom.get(&block) != Some(&new_val) {
                        idom.insert(block, new_val);
                        changed = true;
                    }
                }
            }
        }

        let dom_tree_children = self.compute_dom_tree(&idom);
        let dom_frontier = self.compute_dominance_frontiers(&idom);

        self.dominance = Some(DominanceInfo {
            idom,
            dom_tree_children,
            dom_frontier,
        });
    }

    fn compute_dom_tree(&self, idom: &HashMap<BlockId, BlockId>) -> HashMap<BlockId, Vec<BlockId>> {
        let mut children: HashMap<BlockId, Vec<BlockId>> = HashMap::new();

        // Initialize empty vectors
        for block_id in self.blocks.keys() {
            children.insert(*block_id, Vec::new());
        }

        // Build parent → children relationships
        for (node, &dominator) in idom {
            if *node != dominator {
                // Skip entry (it dominates itself)
                children.get_mut(&dominator).unwrap().push(*node);
            }
        }

        children
    }

    fn compute_idom_for_block(
        &self,
        preds: &[BlockId],
        idom: &HashMap<BlockId, BlockId>,
    ) -> Option<BlockId> {
        if preds.is_empty() {
            return None;
        }

        // Find first processed predecessor
        let mut new_idom = None;
        for &pred in preds {
            if idom.contains_key(&pred) {
                new_idom = Some(pred);
                break;
            }
        }

        let mut new_idom = new_idom?;

        // Intersect with remaining processed predecessors
        for &pred in preds {
            if pred != new_idom && idom.contains_key(&pred) {
                new_idom = self.intersect(new_idom, pred, idom);
            }
        }

        Some(new_idom)
    }

    fn intersect(
        &self,
        mut b1: BlockId,
        mut b2: BlockId,
        idom: &HashMap<BlockId, BlockId>,
    ) -> BlockId {
        let rpo_index = self.compute_rpo_indices();

        while b1 != b2 {
            while rpo_index[&b1] > rpo_index[&b2] {
                b1 = idom[&b1];
            }
            while rpo_index[&b2] > rpo_index[&b1] {
                b2 = idom[&b2];
            }
        }

        b1
    }

    fn compute_rpo_indices(&self) -> &HashMap<BlockId, usize> {
        self.rpo_indices_cache.get_or_init(|| {
            self.reverse_post_order()
                .iter()
                .enumerate()
                .map(|(idx, &block)| (block, idx))
                .collect()
        })
    }

    fn compute_dominance_frontiers(
        &self,
        idom: &HashMap<BlockId, BlockId>,
    ) -> HashMap<BlockId, HashSet<BlockId>> {
        let mut df: HashMap<BlockId, HashSet<BlockId>> = HashMap::new();

        for block_id in self.blocks.keys() {
            df.insert(*block_id, HashSet::new());
        }

        for (node, block) in &self.blocks {
            if block.predecessors.len() >= 2 {
                // This is a join point
                for pred in &block.predecessors {
                    let mut runner = *pred;

                    // Add to DF while runner doesn't strictly dominate node
                    while runner != idom[node] {
                        df.get_mut(&runner).unwrap().insert(*node);
                        runner = idom[&runner];
                    }
                }
            }
        }

        df
    }

    pub fn build_loop_forest(&mut self) {
        match self.dominance {
            None => self.compute_dominance(),
            Some(_) => {}
        }

        let back_edges = self.find_back_edges();

        if back_edges.is_empty() {
            // No loops in this function
            self.loops = Some(LoopForest {
                loops: HashMap::new(),
                top_level: vec![],
            });
        }

        let mut loops = HashMap::new();
        let mut next_loop_id = 0;

        for (tail, head) in back_edges {
            let body = self.compute_natural_loop(tail, head);

            let loop_info = LoopInfo {
                id: next_loop_id,
                header: head,
                back_edges: vec![tail],
                body,
                parent: None,
                children: vec![],
                source_while: None,
                invariants: vec![],
            };

            loops.insert(next_loop_id, loop_info);
            next_loop_id += 1;
        }

        // Step 3: Detect loop nesting
        self.detect_loop_nesting(&mut loops);

        // Step 4: Build top-level list
        let top_level = loops
            .iter()
            .filter_map(|(id, info)| {
                if info.parent.is_none() {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        // Step 5: Store in self
        self.loops = Some(LoopForest { loops, top_level });
    }

    /// Finds all back edges in the CFG.
    /// A back edge is an edge (tail -> head) where head dominates tail.
    fn find_back_edges(&self) -> Vec<(BlockId, BlockId)> {
        let mut back_edges = Vec::new();

        for (block_id, block) in &self.blocks {
            for &successor in &block.successors {
                // Back edge: successor dominates current block
                if self
                    .dominance
                    .as_ref()
                    .expect("Dominance should be computed at this point")
                    .dominates(successor, *block_id)
                {
                    back_edges.push((*block_id, successor));
                }
            }
        }

        back_edges
    }

    /// Computes the natural loop for a given back edge.
    /// Uses backward traversal from tail to head.
    fn compute_natural_loop(&self, tail: BlockId, head: BlockId) -> HashSet<BlockId> {
        let mut body = HashSet::new();
        let mut worklist = Vec::new();

        // Loop contains at least the header and tail
        body.insert(head);
        body.insert(tail);

        // Special case: self-loop
        if tail == head {
            return body;
        }

        // Walk backwards from tail to head
        worklist.push(tail);

        while let Some(current) = worklist.pop() {
            for &pred in &self.blocks[&current].predecessors {
                if !body.contains(&pred) {
                    body.insert(pred);
                    worklist.push(pred);
                }
            }
        }

        body
    }

    /// Detects nesting relationships between loops.
    /// Loop A contains loop B if header_B is in body_A.
    fn detect_loop_nesting(&self, loops: &mut HashMap<LoopId, LoopInfo>) {
        let loop_ids: Vec<LoopId> = loops.keys().copied().collect();

        for &outer_id in &loop_ids {
            for &inner_id in &loop_ids {
                if outer_id == inner_id {
                    continue;
                }

                let outer_body = &loops[&outer_id].body;
                let inner_header = loops[&inner_id].header;

                // If inner's header is in outer's body, outer contains inner
                if outer_body.contains(&inner_header) {
                    // Only set if it doesn't have a parent yet,
                    // or if this outer is more specific (smaller body)
                    let should_set = match loops[&inner_id].parent {
                        None => true,
                        Some(current_parent) => {
                            // If the new outer has a smaller body, it's more specific
                            loops[&outer_id].body.len() < loops[&current_parent].body.len()
                        }
                    };

                    if should_set {
                        // Remove from the previous parent's children if present
                        if let Some(old_parent) = loops[&inner_id].parent {
                            loops
                                .get_mut(&old_parent)
                                .unwrap()
                                .children
                                .retain(|&id| id != inner_id);
                        }

                        loops.get_mut(&inner_id).unwrap().parent = Some(outer_id);
                        loops.get_mut(&outer_id).unwrap().children.push(inner_id);
                    }
                }
            }
        }
    }

    fn def_sites(&self) -> HashMap<SymbolId, Vec<BlockId>> {
        let mut def_sites = HashMap::new();

        for &param in &self.parameters {
            def_sites
                .entry(param)
                .or_insert_with(Vec::new)
                .push(self.entry);
        }

        for (block_id, block) in &self.blocks {
            for inst in &block.instructions {
                match inst {
                    SsaInstruction::Copy { dest, .. } => {
                        def_sites
                            .entry(dest.symbol)
                            .or_insert_with(Vec::new)
                            .push(*block_id);
                    }
                    SsaInstruction::Phi { .. } => {}
                    SsaInstruction::BinaryOp { dest, .. } => {
                        def_sites
                            .entry(dest.symbol)
                            .or_insert_with(Vec::new)
                            .push(*block_id);
                    }
                    SsaInstruction::UnaryOp { dest, .. } => {
                        def_sites
                            .entry(dest.symbol)
                            .or_insert_with(Vec::new)
                            .push(*block_id);
                    }
                    SsaInstruction::StorageLoad { dest, .. } => {
                        def_sites
                            .entry(dest.symbol)
                            .or_insert_with(Vec::new)
                            .push(*block_id);
                    }
                    SsaInstruction::StorageStore { .. } => {}
                    SsaInstruction::Call { dest, .. } => {
                        if let Some(d) = dest {
                            def_sites
                                .entry(d.symbol)
                                .or_insert_with(Vec::new)
                                .push(*block_id);
                        }
                    }
                    SsaInstruction::Fold { .. } | SsaInstruction::Unfold { .. } | SsaInstruction::Assert { .. } => {}
                };
            }
        }

        def_sites
    }

    pub fn insert_phi_nodes_and_rename(&mut self) {
        self.phi_nodes_placement();
        self.ssa_variable_renaming();
    }

    fn phi_nodes_placement(&mut self) {
        let def_sites = self.def_sites();

        for (symbol, sites) in &def_sites {
            self.place_phis_for_variable(*symbol, sites);
        }
    }

    fn place_phis_for_variable(&mut self, symbol: SymbolId, def_sites: &[BlockId]) {
        let mut has_phi = HashSet::new();
        let mut worklist = def_sites.to_vec();

        while let Some(block) = worklist.pop() {
            let df = &self.dominance.as_ref().unwrap().dom_frontier[&block];

            for &df_block in df {
                if !has_phi.contains(&df_block) {
                    self.blocks
                        .get_mut(&df_block)
                        .expect("Block not found")
                        .instructions
                        .insert(
                            0,
                            SsaInstruction::Phi {
                                dest: Register { symbol, version: 0 },
                                sources: vec![],
                            },
                        );
                    has_phi.insert(df_block);
                    worklist.push(df_block);
                }
            }
        }
    }

    /// Performs SSA renaming phase.
    /// Requires phi nodes to be placed first.
    fn ssa_variable_renaming(&mut self) {
        match self.dominance {
            None => self.compute_dominance(),
            Some(_) => {}
        }

        let mut ctx = RenamingContext::new();

        for param in &self.parameters {
            let version = ctx.next_version(*param);
            ctx.push(*param, version);
        }

        self.recursive_rename_blocks(self.entry, &mut ctx);
    }

    fn recursive_rename_blocks(&mut self, block_id: usize, ctx: &mut RenamingContext) {
        let mut definitions_made: Vec<(SymbolId, usize)> = Vec::new();

        self.process_phi_nodes(block_id, ctx, &mut definitions_made);
        self.process_instructions_and_terminators(block_id, ctx, &mut definitions_made);
        self.fill_successor_phis(block_id, ctx);

        let children = self
            .dominance
            .as_ref()
            .expect("Dominance should be computed at this point")
            .dom_tree_children
            .get(&block_id)
            .cloned()
            .unwrap_or_default();

        for child in children {
            self.recursive_rename_blocks(child, ctx);
        }

        for (symbol, _) in definitions_made {
            ctx.pop(symbol);
        }
    }

    /// Process phi nodes in a block.
    /// Update the symbol of the block's phi nodes with their respective version, but not their sources.
    fn process_phi_nodes(
        &mut self,
        block_id: usize,
        ctx: &mut RenamingContext,
        definitions_made: &mut Vec<(SymbolId, usize)>,
    ) {
        let block = self.blocks.get_mut(&block_id).expect("Block not found");

        let phi_indices: Vec<usize> = block
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(idx, inst)| matches!(inst, SsaInstruction::Phi { .. }).then_some(idx))
            .collect();

        for idx in phi_indices {
            if let SsaInstruction::Phi { dest, .. } = &mut block.instructions[idx] {
                let symbol = dest.symbol;

                let version = ctx.next_version(symbol);
                dest.version = version;

                ctx.push(symbol, version);
                definitions_made.push((symbol, version));
            }
        }
    }

    fn process_instructions_and_terminators(
        &mut self,
        block_id: usize,
        ctx: &mut RenamingContext,
        definitions_made: &mut Vec<(SymbolId, usize)>,
    ) {
        let block = self.blocks.get_mut(&block_id).expect("Block not found");

        for inst in &mut block.instructions {
            // Skip phi-nodes (already processed in `process_phi_nodes`)
            if matches!(inst, SsaInstruction::Phi { .. }) {
                continue;
            }

            Self::rename_instruction_operands(inst, ctx);

            match inst {
                SsaInstruction::Copy { dest, .. }
                | SsaInstruction::BinaryOp { dest, .. }
                | SsaInstruction::UnaryOp { dest, .. }
                | SsaInstruction::StorageLoad { dest, .. } => {
                    let symbol = dest.symbol;
                    let version = ctx.next_version(symbol);
                    dest.version = version;

                    ctx.push(symbol, version);
                    definitions_made.push((symbol, version));
                }
                SsaInstruction::Call { dest, .. } => {
                    if let Some(d) = dest.as_mut() {
                        let symbol = d.symbol;
                        let version = ctx.next_version(symbol);
                        d.version = version;

                        ctx.push(symbol, version);
                        definitions_made.push((symbol, version));
                    }
                }
                SsaInstruction::Phi { .. }
                | SsaInstruction::StorageStore { .. }
                | SsaInstruction::Fold { .. }
                | SsaInstruction::Unfold { .. }
                | SsaInstruction::Assert { .. } => {}
            }
        }

        // Rename the terminator's operands
        let block = self.blocks.get_mut(&block_id).expect("Block not found");
        Self::rename_terminator_operands(&mut block.terminator, ctx);
    }

    fn rename_instruction_operands(inst: &mut SsaInstruction, ctx: &RenamingContext) {
        match inst {
            SsaInstruction::Copy { source, .. } => {
                Self::rename_operand(source, ctx);
            }
            SsaInstruction::BinaryOp { left, right, .. } => {
                Self::rename_operand(left, ctx);
                Self::rename_operand(right, ctx);
            }
            SsaInstruction::UnaryOp { operand, .. } => {
                Self::rename_operand(operand, ctx);
            }
            SsaInstruction::StorageStore { value, .. } => {
                Self::rename_operand(value, ctx);
            }
            SsaInstruction::Call { args, .. } => {
                for arg in args {
                    Self::rename_operand(arg, ctx);
                }
            }
            SsaInstruction::Assert { condition, .. } => {
                Self::rename_operand(condition, ctx);
            }
            // These don't have operands or already processed
            SsaInstruction::Phi { .. }
            | SsaInstruction::StorageLoad { .. }
            | SsaInstruction::Fold { .. }
            | SsaInstruction::Unfold { .. } => {}
        }
    }

    /// Rename a single operand to its current version
    fn rename_operand(operand: &mut Operand, ctx: &RenamingContext) {
        if let Operand::Register(reg) = operand {
            let version = ctx
                .top(reg.symbol)
                .expect(&format!("Variable {:?} used before definition", reg.symbol));
            reg.version = version;
        }
    }

    fn rename_terminator_operands(terminator: &mut Terminator, ctx: &RenamingContext) {
        match terminator {
            Terminator::Branch { condition, .. } => {
                Self::rename_operand(condition, ctx);
            }
            Terminator::Return { value, .. } => {
                if let Some(operand) = value {
                    Self::rename_operand(operand, ctx);
                }
            }
            Terminator::Jump { .. } | Terminator::Unreachable => {
            
            }
        }
    }

    fn fill_successor_phis(&mut self, block_id: usize, ctx: &mut RenamingContext) {
        let successors = self
            .blocks
            .get(&block_id)
            .map(|b| b.successors.clone())
            .unwrap_or_default();

        for succ_id in successors {
            let succ_block = self
                .blocks
                .get_mut(&succ_id)
                .expect("Successor block not found");

            for inst in &mut succ_block.instructions {
                if let SsaInstruction::Phi { dest, sources, .. } = inst {
                    let symbol = dest.symbol;

                    if let Some(version) = ctx.top(symbol) {
                        sources.push((block_id, Register { symbol, version }));
                    }
                }
            }
        }
    }
}

/// Context for SSA renaming phase
struct RenamingContext {
    /// Counter for next version of each variable
    counters: HashMap<SymbolId, usize>,
    /// Stack of active versions for each variable
    stacks: HashMap<SymbolId, Vec<usize>>,
}

impl RenamingContext {
    fn new() -> Self {
        Self {
            counters: HashMap::new(),
            stacks: HashMap::new(),
        }
    }

    /// Get the next version number for a variable
    fn next_version(&mut self, symbol: SymbolId) -> usize {
        let counter = self.counters.entry(symbol).or_insert(0);
        let version = *counter;
        *counter += 1;
        version
    }

    /// Push a new version onto the stack for a variable
    fn push(&mut self, symbol: SymbolId, version: usize) {
        self.stacks
            .entry(symbol)
            .or_insert_with(Vec::new)
            .push(version);
    }

    /// Get the current active version for a variable
    fn top(&self, symbol: SymbolId) -> Option<usize> {
        self.stacks.get(&symbol)?.last().copied()
    }

    /// Pop the top version from the stack
    fn pop(&mut self, symbol: SymbolId) {
        if let Some(stack) = self.stacks.get_mut(&symbol) {
            stack.pop();
        }
    }
}

#[derive(Debug)]
pub struct DominanceInfo {
    idom: HashMap<BlockId, BlockId>,
    dom_tree_children: HashMap<BlockId, Vec<BlockId>>,
    dom_frontier: HashMap<BlockId, HashSet<BlockId>>,
}

impl DominanceInfo {
    /// Checks if `dominator` dominates `dominated`.
    /// A block dominates itself.
    pub fn dominates(&self, dominator: BlockId, dominated: BlockId) -> bool {
        // A block dominates itself
        if dominator == dominated {
            return true;
        }

        // Walk up the dominator tree from dominated
        let mut current = dominated;
        while let Some(&idom) = self.idom.get(&current) {
            if idom == dominator {
                return true;
            }

            // Stop if we reach the entry (idom of entry is itself)
            if idom == current {
                break;
            }

            current = idom;
        }

        false
    }
}

#[derive(Debug)]
pub struct LoopForest {
    /// All loops indexed by their ID
    pub loops: HashMap<LoopId, LoopInfo>,
    /// Top-level loops (not nested inside any other loop)
    pub top_level: Vec<LoopId>,
}

#[derive(Debug)]
pub struct LoopInfo {
    pub id: LoopId,
    pub header: BlockId,
    pub back_edges: Vec<BlockId>,
    pub body: HashSet<BlockId>,
    pub parent: Option<LoopId>,
    pub children: Vec<LoopId>, // Direct nested loops
    // AST connection for error reporting
    pub source_while: Option<NodeId>,
    pub invariants: Vec<Expression>,
}
// ============================================================================
// DEBUG IMPLEMENTATIONS
// ============================================================================

impl std::fmt::Debug for SsaInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Copy { dest, source, .. } => write!(f, "{:?} = {:?}", dest, source),
            Self::Phi { dest, sources, .. } => {
                write!(f, "{:?} = φ(", dest)?;
                for (i, (block, reg)) in sources.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "bb{}: {:?}", block, reg)?;
                }
                write!(f, ")")
            }
            Self::BinaryOp {
                dest,
                op,
                left,
                right,
                ..
            } => {
                write!(f, "{:?} = {:?} {:?} {:?}", dest, left, op, right)
            }
            Self::UnaryOp {
                dest, op, operand, ..
            } => {
                write!(f, "{:?} = {:?} {:?}", dest, op, operand)
            }
            Self::StorageLoad { dest, var, .. } => {
                write!(f, "{:?} = storage[{:?}]", dest, var)
            }
            Self::StorageStore { var, value, .. } => {
                write!(f, "storage[{:?}] = {:?}", var, value)
            }
            Self::Call {
                dest, target, args, ..
            } => {
                if let Some(d) = dest {
                    write!(f, "{:?} = ", d)?;
                }
                write!(f, "{:?}(", target)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{:?}", arg)?;
                }
                write!(f, ")")
            }
            Self::Fold { var, .. } => {
                write!(f, "fold {:?}", var)
            },
            Self::Unfold { var, .. } => {
                write!(f, "unfold {:?}", var)
            }
            Self::Assert {
                condition, kind, ..
            } => {
                write!(f, "assert({:?}, {:?})", condition, kind)
            }
        }
    }
}

impl std::fmt::Debug for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "bb{}: {{", self.id)?;

        if !self.predecessors.is_empty() {
            write!(f, "  preds: [")?;
            for (i, pred) in self.predecessors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb{}", pred)?;
            }
            writeln!(f, "]")?;
        }

        // if let Some(loop_id) = self.loop_header {
        //     writeln!(f, "  loop_header: {}", loop_id)?;
        // }

        for instr in &self.instructions {
            writeln!(f, "    {:?}", instr)?;
        }

        writeln!(f, "    {:?}", self.terminator)?;

        if !self.successors.is_empty() {
            write!(f, "  succs: [")?;
            for (i, succ) in self.successors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb{}", succ)?;
            }
            writeln!(f, "]")?;
        }

        write!(f, "}}")
    }
}

impl std::fmt::Debug for SsaCfg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SsaCfg {{")?;
        writeln!(f, "  entry: bb{}", self.entry)?;

        let exits = self.exit_blocks();
        if exits.is_empty() {
            writeln!(f, "  exit: []")?;
        } else {
            write!(f, "  exit: [")?;
            for (i, exit) in exits.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb{}", exit)?;
            }
            writeln!(f, "]")?;
        }

        if !self.parameters.is_empty() {
            write!(f, "  params: [")?;
            for (i, param) in self.parameters.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{:?}", param)?;
            }
            writeln!(f, "]")?;
        }

        writeln!(f)?;

        // Print blocks in order
        let mut block_ids: Vec<_> = self.blocks.keys().collect();
        block_ids.sort();

        for (i, block_id) in block_ids.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            if let Some(block) = self.blocks.get(block_id) {
                writeln!(f, "  {:?}", block)?;
            }
        }

        write!(f, "}}")
    }
}

// ============================================================================
// Program
// ============================================================================

#[derive(Debug)]
pub struct SsaProgram {
    pub files: IndexMap<String, SsaFile>,
}

#[derive(Debug)]
pub struct SsaFile {
    pub imports: Vec<Import>,
    pub interfaces: Vec<InterfaceDecl>,
    pub contract: SsaContract,
}

#[derive(Debug)]
pub struct SsaContract {
    pub name: String,
    pub variables: Vec<StateVar>,
    pub constants: Vec<StateConst>,
    pub constructor: Option<SsaCfg>,
    pub functions: Vec<SsaCfg>
}

// #[derive(Debug)]
// pub struct SsaStateDef {
//     pub contract: String,
//     pub name: String,
//     pub owner: Owner,
//     pub functions: Vec<SsaCfg>,
//     pub source_ref: SourceRef,
// }
