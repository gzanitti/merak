use merak_ast::contract::Param;
use merak_ast::function::{Modifier, Visibility};
use merak_ast::predicate::Predicate;
use merak_ast::types::{BaseType, Type};
use merak_ast::NodeId;
use merak_errors::{MerakError, MerakResult};
use std::collections::HashMap;
use std::fmt;

pub type ScopeId = usize;

/// Unique identifier for symbols in the symbol table
/// Acts as an index into the symbol arena for O(1) access
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolId {
    /// Normal symbol from user code (in symbol table)
    Named(String, usize),

    /// Synthetic temporary for intermediate values (NOT in symbol table)
    Temp(usize),
}

impl SymbolId {
    pub fn new(name: String, index: usize) -> Self {
        SymbolId::Named(name, index)
    }

    pub fn synthetic_temp(id: usize) -> Self {
        SymbolId::Temp(id)
    }

    pub fn is_temp(&self) -> bool {
        matches!(self, SymbolId::Temp(_))
    }

    fn as_usize(&self) -> usize {
        let (&Self::Named(_, i) | &Self::Temp(i)) = self;
        i
    }
}

impl fmt::Display for SymbolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolId::Named(n, _) => write!(f, "{}", n),
            SymbolId::Temp(n) => write!(f, "__temp_{}", n),
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
    /// Arena: central storage for all SymbolInfo
    symbol_arena: Vec<SymbolInfo>,
    /// Scope hierarchy for name resolution
    scopes: Vec<Scope>,
    current_scope: ScopeId,
    /// Maps NodeId -> SymbolId for O(1) lookup during type checking
    node_to_symbol: HashMap<NodeId, SymbolId>,
    expr_to_type: HashMap<NodeId, BaseType>,
    qualified_names_to_symbol: HashMap<String, SymbolId>,
}

#[derive(Debug, Clone)]
pub struct Scope {
    //pub parent: Option<ScopeId>,
    /// Maps (name, namespace) -> SymbolId (index into arena)
    pub symbols: HashMap<(String, SymbolNamespace), SymbolId>,
}

impl SymbolTable {
    pub fn new() -> Self {
        let global_scope = Scope {
            //parent: None,
            symbols: HashMap::new(),
        };

        Self {
            symbol_arena: Vec::new(),
            scopes: vec![global_scope],
            current_scope: 0,
            node_to_symbol: HashMap::new(),
            expr_to_type: HashMap::new(),
            qualified_names_to_symbol: HashMap::new(),
        }
    }

    pub fn push_scope(&mut self) -> ScopeId {
        let new_scope = Scope {
            //parent: Some(self.current_scope),
            symbols: HashMap::new(),
        };

        let scope_id = self.scopes.len();
        self.scopes.push(new_scope);
        self.current_scope = scope_id;
        scope_id
    }

    pub fn pop_scope(&mut self) {
        // if let Some(parent) = self.scopes[self.current_scope].parent {
        //     self.current_scope = parent;
        // }
        self.scopes.pop();
        self.current_scope = self.current_scope - 1;
    }

    pub fn clean_scopes(&mut self) {
        let global_scope = Scope {
            symbols: HashMap::new(),
        };

        self.scopes.clear();
        self.scopes = vec![global_scope];
        self.current_scope = 0;
    }

    pub fn insert_expr_type(&mut self, node_id: NodeId, ty: BaseType) {
        self.expr_to_type.insert(node_id, ty);
    }

    pub fn expr_to_type(&self, node_id: NodeId) -> Option<&BaseType> {
        self.expr_to_type.get(&node_id)
    }

    /// Insert a symbol ID into the current scope
    pub fn insert(&mut self, name: String, namespace: SymbolNamespace, symbol_id: SymbolId) {
        self.scopes[self.current_scope]
            .symbols
            .insert((name, namespace), symbol_id);
    }

    /// Lookup a symbol by name, returning its SymbolId if found
    pub fn lookup(&self, name: &str, namespace: SymbolNamespace) -> Option<SymbolId> {
        let mut current = self.current_scope;

        loop {
            let scope = &self.scopes[current];

            if let Some(symbol_id) = scope.symbols.get(&(name.to_string(), namespace)) {
                return Some(symbol_id.clone());
            }

            current = match current.checked_sub(1) {
                Some(parent) => parent,
                None => {
                    return self.qualified_names_to_symbol.get(name).cloned() 
                }, 
            };
        }
    }

    /// Get SymbolInfo from the arena by SymbolId - O(1) access
    pub fn get_symbol(&self, symbol_id: &SymbolId) -> &SymbolInfo {
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
        let symbol_id = SymbolId::new(simple_name.clone(), self.symbol_arena.len());
        self.symbol_arena.push(info);
        self.qualified_names_to_symbol.insert(qualified_name.to_string(), symbol_id.clone());

        // Insert into scope tree
        self.insert(simple_name, namespace, symbol_id.clone());

        // Connect with NodeId for O(1) access during type checking
        self.node_to_symbol.insert(node_id, symbol_id.clone());

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
            self.node_to_symbol.insert(node_id, symbol_id.clone());
            Some(symbol_id)
        } else {
            None
        }
    }

    /// Get symbol information by NodeId - O(1) lookup for type checking
    pub fn get_symbol_by_node_id(&self, node_id: NodeId) -> Option<&SymbolInfo> {
        self.node_to_symbol
            .get(&node_id)
            .map(|symbol_id| self.get_symbol(symbol_id))
    }

    pub fn get_symbol_id_by_node_id(&self, node_id: NodeId) -> Option<SymbolId> {
        self.node_to_symbol
            .get(&node_id)
            .map(|symbol_id| symbol_id.clone())
    }

    pub fn get_symbol_mut(&mut self, symbol_id: SymbolId) -> &mut SymbolInfo {
        &mut self.symbol_arena[symbol_id.as_usize()]
    }

    /// Update the type of a symbol identified by NodeId
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
    /// Get all symbols in the symbol table
    /// Returns an iterator over (SymbolId, &SymbolInfo) pairs
    pub fn all_symbols(&self) -> impl Iterator<Item = (SymbolId, &SymbolInfo)> {
        self.symbol_arena
            .iter()
            .enumerate()
            .map(|(idx, info)| (SymbolId::new(info.qualified_name.last(), idx), info))
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
    Contract, 
    ContractInit {
        contract: String,
    },
    StateVar,
    StateConst,
    Function {
        visibility: Visibility,
        reentrancy: Modifier,
        parameters: Vec<Param>,
        ensures: Vec<Predicate>,
        requires: Vec<Predicate>,
        return_type: Type,
    },
    Entrypoint {
        reentrancy: Modifier,
        parameters: Vec<Param>,
        ensures: Vec<Predicate>,
        requires: Vec<Predicate>,
        return_type: Type,
    },
    Interface {
        functions: Vec<SymbolId>,  
    },
    InterfaceFunction {     
        params: Vec<Param>,       
        return_type: Type,
    },
    Parameter,
    LocalVar,
}

impl SymbolKind {
    /// Returns the namespace this symbol kind belongs to
    pub fn namespace(&self) -> SymbolNamespace {
        match self {
            SymbolKind::Contract { .. } => SymbolNamespace::Type,
            SymbolKind::ContractInit { .. } => SymbolNamespace::Callable,
            SymbolKind::StateVar => SymbolNamespace::Value,
            SymbolKind::StateConst => SymbolNamespace::Value,
            SymbolKind::Function { .. } => SymbolNamespace::Callable,
            SymbolKind::Entrypoint { .. } => SymbolNamespace::Callable,
            SymbolKind::Parameter => SymbolNamespace::Value,
            SymbolKind::LocalVar => SymbolNamespace::Value,
            SymbolKind::Interface { .. } => SymbolNamespace::Type,
            SymbolKind::InterfaceFunction { .. } => SymbolNamespace::Callable
        }
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolKind::Contract => {
                    write!(f, "Contract")
            }
            SymbolKind::ContractInit { contract } => write!(f, "ContractInit '{}'", contract),
            SymbolKind::StateVar => write!(f, "State variable"),
            SymbolKind::StateConst => write!(f, "State constant"),
            SymbolKind::Function {
                return_type, ..
            } => {
                write!(f, "  return_type: {}", return_type)
            }
            SymbolKind::Entrypoint {
                return_type, ..
            } => {
                write!(f, "  return_type: {}", return_type)
            }
            SymbolKind::Parameter => write!(f, "Parameter"),
            SymbolKind::LocalVar => write!(f, "Local variable"),
            _ => unimplemented!("Don't be lazy"),
        }
    }
}

/// Represents a qualified name like `Contract::State::Function`
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

    pub fn last(&self) -> String {
        self.parts.last().unwrap().clone()
    }
}

impl fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string())
    }
}