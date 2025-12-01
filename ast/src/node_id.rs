use std::cell::Cell;

/// Unique identifier for AST nodes
/// This allows connecting AST nodes with symbol table information in O(1) time
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

impl NodeId {
    pub const fn new(id: usize) -> Self {
        NodeId(id)
    }

    pub fn as_usize(&self) -> usize {
        self.0
    }
}

impl From<usize> for NodeId {
    fn from(value: usize) -> Self {
        NodeId(value)
    }
}

/// Generator for unique NodeIds
/// Used during parsing to assign a unique ID to each AST node
pub struct NodeIdGenerator {
    next_id: Cell<usize>,
}

impl NodeIdGenerator {
    pub fn new() -> Self {
        Self {
            next_id: Cell::new(0),
        }
    }

    /// Generate a new unique NodeId
    pub fn next(&self) -> NodeId {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        NodeId(id)
    }
}

impl Default for NodeIdGenerator {
    fn default() -> Self {
        Self::new()
    }
}
