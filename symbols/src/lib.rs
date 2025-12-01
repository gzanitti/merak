pub mod symbol_table;

// Re-export commonly used types
pub use symbol_table::{
    QualifiedName, ScopeId, SymbolId, SymbolInfo, SymbolKind, SymbolNamespace, SymbolTable,
};
