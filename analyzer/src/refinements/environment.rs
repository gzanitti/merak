use crate::refinements::constraints::TypeContext;
use crate::refinements::templates::{LiquidVar, LiquidVarGenerator, Template};
use merak_ast::predicate::Predicate;
use merak_ast::types::Type;
use merak_symbols::{SymbolId, SymbolInfo, SymbolNamespace, SymbolTable};
use std::collections::HashMap;

/// Type environment for refinement inference
///
/// Manages the typing context during the inference process, including:
/// - Access to the symbol table
/// - Local variable bindings (temporaries, SSA variables)
/// - Current function context
/// - Path assumptions (from if/while conditions)
/// - Invalidation points from StorageAnalysis
pub struct TypeEnvironment<'a> {
    /// Reference to the global symbol table
    symbol_table: &'a mut SymbolTable,

    /// Liquid variable generator
    pub liquid_gen: LiquidVarGenerator,

    /// Local bindings (for temporaries and SSA variables)
    /// Maps variable names to their templates
    local_bindings: HashMap<String, Template>,

    /// Current path assumptions (from guards)
    assumptions: Vec<Predicate>,

    /// Current function being analyzed (if any)
    current_function: Option<SymbolId>,
}

impl<'a> TypeEnvironment<'a> {
    /// Create a new type environment
    pub fn new(symbol_table: &'a mut SymbolTable, ) -> Self {
        Self {
            symbol_table,
            liquid_gen: LiquidVarGenerator::new(),
            local_bindings: HashMap::new(),
            assumptions: Vec::new(),
            current_function: None,
        }
    }

    /// Get the symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
        self.symbol_table
    }

    /// Get mutable access to symbol table
    pub fn symbol_table_mut(&mut self) -> &mut SymbolTable {
        self.symbol_table
    }

    /// Generate a fresh liquid variable
    pub fn fresh_liquid_var(&mut self) -> LiquidVar {
        self.liquid_gen.fresh()
    }

    /// Set the current function context
    pub fn enter_function(&mut self, func_id: SymbolId) {
        self.current_function = Some(func_id);
    }

    /// Exit the current function context
    pub fn exit_function(&mut self) {
        self.current_function = None;
        self.local_bindings.clear();
        self.assumptions.clear();
    }

    /// Get the current function
    pub fn current_function(&self) -> Option<SymbolId> {
        self.current_function
    }

    /// Bind a local variable to a template
    pub fn bind_local(&mut self, name: String, template: Template) {
        self.local_bindings.insert(name, template);
    }

    /// Unbind a local variable
    pub fn unbind_local(&mut self, name: &str) -> Option<Template> {
        self.local_bindings.remove(name)
    }

    pub fn get_local(&self, name: &str) -> Option<Template> {
        self.local_bindings.get(name).cloned()
    }

    pub fn lookup(&self, name: &str) -> Option<Template> {
        self.local_bindings.get(name).cloned()
    }

    /// Look up a variable (checks locals first, then symbol table)
    // pub fn lookup(&self, name: &str) -> Option<Template> {
    //     // First check local bindings
    //     if let Some(template) = self.local_bindings.get(name) {
    //         return Some(template.clone());
    //     }

    //     // Then check symbol table
    //     if let Some(symbol_id) = self.symbol_table.lookup(name, SymbolNamespace::Value) {
    //         let symbol = self.symbol_table.get_symbol(symbol_id);
    //         if let Some(ty) = &symbol.ty {
    //             // Clone liquid_gen for the conversion
    //             let mut gen = self.liquid_gen.clone();
    //             return Some(Template::from_type(ty, &mut gen));
    //         }
    //     }

    //     None
    // }

    /// Add a path assumption (from if/while guard)
    pub fn add_assumption(&mut self, predicate: Predicate) {
        self.assumptions.push(predicate);
    }

    /// Remove the most recent assumption
    pub fn pop_assumption(&mut self) -> Option<Predicate> {
        self.assumptions.pop()
    }

    /// Get all current assumptions
    pub fn assumptions(&self) -> &[Predicate] {
        &self.assumptions
    }

    /// Clear all assumptions (when exiting a guarded scope)
    pub fn clear_assumptions(&mut self) {
        self.assumptions.clear();
    }

    // /// Execute a closure with an additional assumption
    // ///
    // /// This is the safe way to work with guarded scopes (if/while bodies).
    // /// The assumption is automatically removed when the closure returns.
    // ///
    // /// # Example
    // /// ```
    // /// env.with_assumption_scoped(condition, |env| {
    // ///     // Inside this scope, env has the additional assumption
    // ///     generate_constraints_for_then_branch(env);
    // /// });
    // /// // Outside the scope, assumption is automatically removed
    // /// ```
    // pub fn with_assumption_scoped<F, R>(&mut self, predicate: Predicate, f: F) -> R
    // where
    //     F: FnOnce(&mut TypeEnvironment<'a>) -> R,
    // {
    //     // Push assumption
    //     self.assumptions.push(predicate);

    //     // Execute closure
    //     let result = f(self);

    //     // Pop assumption (cleanup)
    //     self.assumptions.pop();

    //     result
    // }

    // /// Execute a closure with multiple additional assumptions
    // ///
    // /// All assumptions are automatically removed when the closure returns.
    // pub fn with_assumptions_scoped<F, R>(&mut self, predicates: Vec<Predicate>, f: F) -> R
    // where
    //     F: FnOnce(&mut TypeEnvironment<'a>) -> R,
    // {
    //     let count = predicates.len();

    //     // Push all assumptions
    //     self.assumptions.extend(predicates);

    //     // Execute closure
    //     let result = f(self);

    //     // Pop all assumptions (cleanup)
    //     self.assumptions.truncate(self.assumptions.len() - count);

    //     result
    // }

    /// Create a TypeContext snapshot for constraint generation
    pub fn to_type_context(&self) -> TypeContext {
        let mut ctx = TypeContext::new();

        // Add local bindings
        for (name, template) in &self.local_bindings {
            ctx.bind(name.clone(), template.clone());
        }

        // Add assumptions
        for assumption in &self.assumptions {
            ctx.assume(assumption.clone());
        }

        ctx
    }

    pub fn get_symbol(&self, symbol_id: SymbolId) -> &SymbolInfo {
        self.symbol_table.get_symbol(symbol_id)
    } 

    /// Get the type of a symbol from the symbol table
    pub fn get_symbol_type(&self, symbol_id: SymbolId) -> Option<&Type> {
        self.symbol_table.get_symbol(symbol_id).ty.as_ref()
    }

    /// Update the type of a symbol in the symbol table
    pub fn update_symbol_type(&mut self, symbol_id: SymbolId, ty: Type) {
        self.symbol_table.get_symbol_mut(symbol_id).ty = Some(ty);
    }

    /// Get the template for a symbol (liquid/concrete)
    pub fn get_symbol_template(&mut self, symbol_id: SymbolId) -> Option<Template> {
        self.symbol_table
            .get_symbol(symbol_id)
            .ty
            .as_ref()
            .map(|ty| Template::from_type(ty, &mut self.liquid_gen))
    }

    /// Check if a variable is in scope (local or global)
    pub fn in_scope(&self, name: &str) -> bool {
        self.local_bindings.contains_key(name)
            || self
                .symbol_table
                .lookup(name, SymbolNamespace::Value)
                .is_some()
    }

    /// Get all variables in scope
    pub fn get_variables_in_scope(&self) -> Vec<String> {
        let mut vars = Vec::new();

        // Add local bindings
        vars.extend(self.local_bindings.keys().cloned());

        // Add symbols from symbol table
        for (symbol_id, symbol) in self.symbol_table.all_symbols() {
            let name = symbol.qualified_name.parts.last().unwrap().clone();
            if !vars.contains(&name) {
                vars.push(name);
            }
        }

        vars
    }

    /// Get the number of current assumptions
    pub fn assumption_depth(&self) -> usize {
        self.assumptions.len()
    }

    /// Check if we're currently in a guarded scope
    pub fn is_guarded(&self) -> bool {
        !self.assumptions.is_empty()
    }
}

// /// Helper struct for managing scoped local bindings
// ///
// /// This ensures bindings are cleaned up even in early returns or panics
// pub struct ScopedBinding<'a, 'env> {
//     env: &'a mut TypeEnvironment<'env>,
//     name: String,
// }

// impl<'a, 'env> ScopedBinding<'a, 'env> {
//     /// Create a new scoped binding
//     pub fn new(env: &'a mut TypeEnvironment<'env>, name: String, template: Template) -> Self {
//         env.bind_local(name.clone(), template);
//         Self { env, name }
//     }
// }

// impl<'a, 'env> Drop for ScopedBinding<'a, 'env> {
//     fn drop(&mut self) {
//         self.env.unbind_local(&self.name);
//     }
// }

// impl<'a> TypeEnvironment<'a> {
//     /// Create a scoped local binding that is automatically cleaned up
//     ///
//     /// # Example
//     /// ```
//     /// {
//     ///     let _binding = env.scoped_binding("temp".to_string(), template);
//     ///     // temp is in scope here
//     /// }
//     /// // temp is automatically removed when _binding is dropped
//     /// ```
//     pub fn scoped_binding(&mut self, name: String, template: Template) -> ScopedBinding<'_, 'a> {
//         ScopedBinding::new(self, name, template)
//     }
// }
