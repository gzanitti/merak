use merak_ast::contract::Param;
use merak_ast::function::{Modifier, Visibility};
use merak_ast::types::Type;
use merak_ast::NodeId;
use merak_errors::{MerakError, MerakResult};
use std::collections::HashMap;
use std::fmt;

pub type ScopeId = usize;

/// Unique identifier for symbols in the symbol table
/// Acts as an index into the symbol arena for O(1) access
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolId {
    /// Normal symbol from user code (in symbol table)
    Named(usize),

    /// Synthetic temporary for intermediate values (NOT in symbol table)
    Temp(usize),
}

impl SymbolId {
    fn new(index: usize) -> Self {
        SymbolId::Named(index)
    }

    pub fn synthetic_temp(id: usize) -> Self {
        SymbolId::Temp(id)
    }

    pub fn is_temp(&self) -> bool {
        matches!(self, SymbolId::Temp(_))
    }

    fn as_usize(&self) -> usize {
        let (&Self::Named(n) | &Self::Temp(n)) = self;
        n
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolId::Named(n) => write!(f, "{}", n),
            SymbolId::Temp(n) => write!(f, "__tempid({})", n),
        }
    }
}

/// Namespace for symbols to allow same name in different categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolNamespace {
    Value,    // Variables, constants, parameters
    Callable, // Functions, entrypoints
    Type,     // Contracts, states
}

#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// Arena: central storage for all SymbolInfo - single source of truth
    symbol_arena: Vec<SymbolInfo>,
    /// Scope hierarchy for name resolution
    scopes: Vec<Scope>,
    current_scope: ScopeId,
    /// Maps NodeId -> SymbolId for O(1) lookup during type checking
    node_to_symbol: HashMap<NodeId, SymbolId>,
}

#[derive(Debug, Clone)]
pub struct Scope {
    pub parent: Option<ScopeId>,
    /// Maps (name, namespace) -> SymbolId (index into arena)
    pub symbols: HashMap<(String, SymbolNamespace), SymbolId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        let global_scope = Scope {
            parent: None,
            symbols: HashMap::new(),
        };

        Self {
            symbol_arena: Vec::new(),
            scopes: vec![global_scope],
            current_scope: 0,
            node_to_symbol: HashMap::new(),
        }
    }

    pub fn push_scope(&mut self) -> ScopeId {
        let new_scope = Scope {
            parent: Some(self.current_scope),
            symbols: HashMap::new(),
        };

        let scope_id = self.scopes.len();
        self.scopes.push(new_scope);
        self.current_scope = scope_id;
        scope_id
    }

    pub fn pop_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current_scope].parent {
            self.current_scope = parent;
        }
    }

    /// Set the current scope to a specific scope ID
    /// Used during name resolution to navigate to scopes created during symbol collection
    pub fn set_current_scope(&mut self, scope_id: ScopeId) {
        assert!(scope_id < self.scopes.len(), "Invalid scope ID");
        self.current_scope = scope_id;
    }

    /// Get the current scope ID
    pub fn get_current_scope(&self) -> ScopeId {
        self.current_scope
    }

    /// Insert a symbol ID into the current scope
    fn insert(&mut self, name: String, namespace: SymbolNamespace, symbol_id: SymbolId) {
        self.scopes[self.current_scope]
            .symbols
            .insert((name, namespace), symbol_id);
    }

    /// Lookup a symbol by name, returning its SymbolId if found
    pub fn lookup(&self, name: &str, namespace: SymbolNamespace) -> Option<SymbolId> {
        let mut current = Some(self.current_scope);

        while let Some(scope_id) = current {
            let scope = &self.scopes[scope_id];

            if let Some(&symbol_id) = scope.symbols.get(&(name.to_string(), namespace)) {
                return Some(symbol_id);
            }

            current = scope.parent;
        }

        None
    }

    /// Get SymbolInfo from the arena by SymbolId - O(1) access
    pub fn get_symbol(&self, symbol_id: SymbolId) -> &SymbolInfo {
        &self.symbol_arena[symbol_id.as_usize()]
    }

    /// Add a symbol to the table, checking for duplicates
    /// The node_id parameter connects this symbol to the corresponding AST node
    /// Returns the SymbolId of the newly created symbol
    pub fn add_symbol(
        &mut self,
        node_id: NodeId,
        qualified_name: QualifiedName,
        kind: SymbolKind,
        ty: Option<Type>,
    ) -> MerakResult<SymbolId> {
        // Get simple name (last component) for HashMap key
        let simple_name = qualified_name
            .parts
            .last()
            .ok_or_else(|| MerakError::NameResolution {
                message: "Qualified name cannot be empty".to_string(),
            })?
            .clone();

        // Determine namespace from symbol kind
        let namespace = kind.namespace();

        // Check if symbol already exists in current scope and namespace
        let key = (simple_name.clone(), namespace);
        if let Some(existing_id) = self.scopes[self.current_scope].symbols.get(&key) {
            let existing = &self.symbol_arena[existing_id.as_usize()];
            return Err(MerakError::NameResolution {
                message: format!(
                    "Duplicate symbol '{}' in {:?} namespace: already defined as '{}' (kind: {:?}), \
                     attempted redefinition as '{}' (kind: {:?})",
                    simple_name, namespace, existing.qualified_name, existing.kind, qualified_name, kind
                ),
            });
        }

        // Create the symbol info
        let info = SymbolInfo {
            qualified_name: qualified_name.clone(),
            kind,
            ty,
        };

        // Add to arena and get its ID
        let symbol_id = SymbolId::new(self.symbol_arena.len());
        self.symbol_arena.push(info);

        // Insert into scope tree
        self.insert(simple_name, namespace, symbol_id);

        // Connect with NodeId for O(1) access during type checking
        self.node_to_symbol.insert(node_id, symbol_id);

        Ok(symbol_id)
    }

    /// Resolve a reference to a symbol and connect it to the AST node via node_id
    /// This is used when you encounter a usage of a symbol (e.g., a variable reference)
    /// Returns the SymbolId if found
    pub fn resolve_reference(
        &mut self,
        node_id: NodeId,
        name: &str,
        namespace: SymbolNamespace,
    ) -> Option<SymbolId> {
        // Look up the symbol in the scope tree
        if let Some(symbol_id) = self.lookup(name, namespace) {
            // Connect this node to the symbol
            self.node_to_symbol.insert(node_id, symbol_id);
            Some(symbol_id)
        } else {
            None
        }
    }

    /// Get symbol information by NodeId - O(1) lookup for type checking
    /// Returns reference to SymbolInfo from the arena
    pub fn get_symbol_by_node_id(&self, node_id: NodeId) -> Option<&SymbolInfo> {
        self.node_to_symbol
            .get(&node_id)
            .map(|&symbol_id| self.get_symbol(symbol_id))
    }

    pub fn get_symbol_id_by_node_id(&self, node_id: NodeId) -> Option<SymbolId> {
        self.node_to_symbol
            .get(&node_id)
            .map(|&symbol_id| symbol_id)
    }

    pub fn get_symbol_mut(&mut self, symbol_id: SymbolId) -> &mut SymbolInfo {
        &mut self.symbol_arena[symbol_id.as_usize()]
    }

    /// Update the type of a symbol identified by NodeId
    /// Used during type checking when inferring types for declarations
    /// Returns Ok(()) if the symbol was found and updated, Err if not found
    pub fn update_type(&mut self, node_id: NodeId, ty: Type) -> MerakResult<()> {
        let symbol_id =
            self.node_to_symbol
                .get(&node_id)
                .ok_or_else(|| MerakError::NameResolution {
                    message: format!(
                        "Cannot update type: no symbol found for node_id {:?}",
                        node_id
                    ),
                })?;

        self.symbol_arena[symbol_id.as_usize()].ty = Some(ty);
        Ok(())
    }
}

// ============================================================================
// TEST HELPER METHODS
// These methods are intended for use in tests to verify symbol table contents.
// They are always compiled (not behind #[cfg(test)]) so that other crates
// can use them in their integration tests.
// ============================================================================
impl SymbolTable {
    /// Find all symbols with a given name across ALL scopes
    /// Returns a vector of (ScopeId, SymbolId, &SymbolInfo) tuples
    /// This allows tests to verify symbols exist in the correct scope with the correct properties
    pub fn find_symbols_by_name(&self, name: &str) -> Vec<(ScopeId, SymbolId, &SymbolInfo)> {
        let mut results = Vec::new();
        for (scope_id, scope) in self.scopes.iter().enumerate() {
            for ((symbol_name, _namespace), &symbol_id) in &scope.symbols {
                if symbol_name == name {
                    let info = self.get_symbol(symbol_id);
                    results.push((scope_id, symbol_id, info));
                }
            }
        }
        results
    }

    /// Return the ScopeId where a symbol was defined
    /// Useful for verifying symbols are registered in the expected scope
    pub fn get_scope_for_symbol(&self, symbol_id: SymbolId) -> Option<ScopeId> {
        for (scope_id, scope) in self.scopes.iter().enumerate() {
            if scope.symbols.values().any(|&id| id == symbol_id) {
                return Some(scope_id);
            }
        }
        None
    }

    /// Get all symbols in the symbol table
    /// Returns an iterator over (SymbolId, &SymbolInfo) pairs
    pub fn all_symbols(&self) -> impl Iterator<Item = (SymbolId, &SymbolInfo)> {
        self.symbol_arena
            .iter()
            .enumerate()
            .map(|(idx, info)| (SymbolId::new(idx), info))
    }
}

#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub qualified_name: QualifiedName,
    pub kind: SymbolKind,
    pub ty: Option<Type>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SymbolKind {
    Contract {
        states: Vec<String>,
    },
    State {
        contract: String,
    },
    StateVar,
    StateConst,
    Function {
        state: String,
        visibility: Visibility,
        reentrancy: Modifier,
        parameters: Vec<Param>,
        return_type: Type,
    },
    Entrypoint {
        state: String,
        reentrancy: Modifier,
        parameters: Vec<Param>,
        return_type: Type,
    },
    Constructor {
        contract: String,
    },
    Parameter,
    LocalVar,
}

impl SymbolKind {
    /// Returns the namespace this symbol kind belongs to
    pub fn namespace(&self) -> SymbolNamespace {
        match self {
            SymbolKind::Contract { .. } => SymbolNamespace::Type,
            SymbolKind::State { .. } => SymbolNamespace::Type,
            SymbolKind::StateVar => SymbolNamespace::Value,
            SymbolKind::StateConst => SymbolNamespace::Value,
            SymbolKind::Function { .. } => SymbolNamespace::Callable,
            SymbolKind::Entrypoint { .. } => SymbolNamespace::Callable,
            SymbolKind::Constructor { .. } => SymbolNamespace::Callable,
            SymbolKind::Parameter => SymbolNamespace::Value,
            SymbolKind::LocalVar => SymbolNamespace::Value,
        }
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolKind::Contract { states } => {
                if states.is_empty() {
                    write!(f, "Contract[]")
                } else {
                    write!(f, "Contract[{}]", states.join(", "))
                }
            }
            SymbolKind::State { contract } => write!(f, "State in contract '{}'", contract),
            SymbolKind::StateVar => write!(f, "State variable"),
            SymbolKind::StateConst => write!(f, "State constant"),
            SymbolKind::Function {
                state, return_type, ..
            } => {
                write!(f, "Function in state '{}'", state)?;
                write!(f, "  return_type: {}", return_type)
            }
            SymbolKind::Entrypoint {
                state, return_type, ..
            } => {
                write!(f, "Entrypoint in state '{}'", state)?;
                write!(f, "  return_type: {}", return_type)
            }
            SymbolKind::Constructor { contract } => {
                write!(f, "Constructor for contract '{}'", contract)
            }
            SymbolKind::Parameter => write!(f, "Parameter"),
            SymbolKind::LocalVar => write!(f, "Local variable"),
        }
    }
}

/// Represents a qualified name like `module::submodule::Contract`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedName {
    pub parts: Vec<String>,
}

impl QualifiedName {
    pub fn new(parts: Vec<String>) -> Self {
        Self { parts }
    }

    pub fn from_string(name: String) -> Self {
        Self { parts: vec![name] }
    }

    pub fn to_string(&self) -> String {
        self.parts.join("::")
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}

impl fmt::Display for SymbolTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Symbol Table")?;
        writeln!(f, "============")?;
        writeln!(f)?;

        // Display all scopes recursively starting from global scope
        self.fmt_scope(f, 0, 0)?;

        writeln!(f)?;
        writeln!(f, "Total symbols: {}", self.symbol_arena.len())?;
        writeln!(f, "Total scopes: {}", self.scopes.len())?;
        writeln!(f, "Current scope: {}", self.current_scope)?;

        Ok(())
    }
}

impl SymbolTable {
    /// Helper function to format a scope and its children recursively
    fn fmt_scope(
        &self,
        f: &mut fmt::Formatter<'_>,
        scope_id: ScopeId,
        indent: usize,
    ) -> fmt::Result {
        let indent_str = "  ".repeat(indent);
        let scope = &self.scopes[scope_id];

        writeln!(
            f,
            "{}Scope {} {}",
            indent_str,
            scope_id,
            if scope_id == self.current_scope {
                "(current)"
            } else {
                ""
            }
        )?;

        // Group symbols by namespace
        let mut value_symbols = Vec::new();
        let mut callable_symbols = Vec::new();
        let mut type_symbols = Vec::new();

        for ((name, namespace), &symbol_id) in &scope.symbols {
            match namespace {
                SymbolNamespace::Value => value_symbols.push((name, symbol_id)),
                SymbolNamespace::Callable => callable_symbols.push((name, symbol_id)),
                SymbolNamespace::Type => type_symbols.push((name, symbol_id)),
            }
        }

        // Display symbols by namespace
        if !type_symbols.is_empty() {
            writeln!(f, "{}  [Types]", indent_str)?;
            for (name, symbol_id) in type_symbols {
                let symbol = self.get_symbol(symbol_id);
                writeln!(
                    f,
                    "{}    {} : {:?} = {:?}",
                    indent_str,
                    name,
                    symbol.kind,
                    symbol
                        .ty
                        .as_ref()
                        .map(|t| format!("{}", t))
                        .unwrap_or("?".to_string())
                )?;
            }
        }

        if !callable_symbols.is_empty() {
            writeln!(f, "{}  [Callables]", indent_str)?;
            for (name, symbol_id) in callable_symbols {
                let symbol = self.get_symbol(symbol_id);
                writeln!(
                    f,
                    "{}    {} : {:?} = {:?}",
                    indent_str,
                    name,
                    symbol.kind,
                    symbol
                        .ty
                        .as_ref()
                        .map(|t| format!("{}", t))
                        .unwrap_or("?".to_string())
                )?;
            }
        }

        if !value_symbols.is_empty() {
            writeln!(f, "{}  [Values]", indent_str)?;
            for (name, symbol_id) in value_symbols {
                let symbol = self.get_symbol(symbol_id);
                writeln!(
                    f,
                    "{}    {} : {:?} = {:?}",
                    indent_str,
                    name,
                    symbol.kind,
                    symbol
                        .ty
                        .as_ref()
                        .map(|t| format!("{}", t))
                        .unwrap_or("?".to_string())
                )?;
            }
        }

        // Find and display child scopes
        for (id, child_scope) in self.scopes.iter().enumerate() {
            if let Some(parent_id) = child_scope.parent {
                if parent_id == scope_id {
                    self.fmt_scope(f, id, indent + 1)?;
                }
            }
        }

        Ok(())
    }
}
