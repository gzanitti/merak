use crate::refinements::constraints::TypeContext;
use crate::refinements::templates::{LiquidVar, LiquidVarGenerator, Template};
use merak_ast::predicate::Predicate;
use merak_ast::types::Type;
use merak_symbols::{SymbolId, SymbolInfo, SymbolTable};
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

    /// Maps source-level variable names to their current SSA register names.
    /// e.g., "i" -> "i_0", updated to "i" -> "i_1" after reassignment in loop body.
    source_to_ssa: HashMap<String, String>,

    /// Equality facts from Copy/BinaryOp instructions (e.g., i_2 = __temp_1_1).
    /// Separate from assumptions so they don't interfere with push/pop for path conditions.
    local_facts: Vec<Predicate>,
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
            source_to_ssa: HashMap::new(),
            local_facts: Vec::new(),
        }
    }

    /// Get the symbol table
    pub fn symbol_table(&self) -> &SymbolTable {
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
        self.source_to_ssa.clear();
        self.local_facts.clear();
    }

    /// Get the current function
    pub fn current_function(&self) -> Option<SymbolId> {
        self.current_function.clone()
    }

    /// Bind a local variable to a template
    pub fn bind_local(&mut self, name: String, template: Template) {
        self.local_bindings.insert(name, template);
    }

    /// Add an equality fact from a Copy or BinaryOp instruction.
    /// Unlike assumptions, these are not managed by push/pop.
    pub fn add_local_fact(&mut self, fact: Predicate) {
        self.local_facts.push(fact);
    }

    /// Update the source-to-SSA mapping for a named variable
    pub fn track_source_ssa(&mut self, source_name: String, ssa_name: String) {
        self.source_to_ssa.insert(source_name, ssa_name);
    }

    /// Get the current source-to-SSA mapping (snapshot for constraint generation)
    pub fn source_to_ssa_mapping(&self) -> &HashMap<String, String> {
        &self.source_to_ssa
    }

    /// Unbind a local variable
    // pub fn unbind_local(&mut self, name: &str) -> Option<Template> {
    //     self.local_bindings.remove(name)
    // }

    /// Returns a reference to the local bindings map (register name -> Template)
    pub fn local_bindings(&self) -> &HashMap<String, Template> {
        &self.local_bindings
    }

    pub fn get_local(&self, name: &str) -> Option<Template> {
        self.local_bindings.get(name).cloned().map(|mut t| {
            println!("Template (name: {name}): {t}");
            t.replace_binder(name);
            println!("New: {t}");
            t
        })
    }

    /// Add a path assumption (from if/while guard)
    pub fn add_assumption(&mut self, predicate: Predicate) {
        self.assumptions.push(predicate);
    }

    /// Remove the most recent assumption
    pub fn pop_assumption(&mut self) -> Option<Predicate> {
        self.assumptions.pop()
    }

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

        // Add equality facts from Copy/BinaryOp instructions
        for fact in &self.local_facts {
            ctx.assume(fact.clone());
        }

        // Carry source-to-SSA mapping for cross-variable reference resolution
        ctx.set_source_to_ssa(self.source_to_ssa.clone());

        ctx
    }

    pub fn get_symbol(&self, symbol_id: &SymbolId) -> &SymbolInfo {
        self.symbol_table.get_symbol(symbol_id)
    } 

    /// Get the type of a symbol from the symbol table
    pub fn get_symbol_type(&self, symbol_id: &SymbolId) -> Option<&Type> {
        self.symbol_table.get_symbol(symbol_id).ty.as_ref()
    }

    /// Get the template for a symbol (liquid/concrete)
    pub fn get_symbol_template(&mut self, symbol_id: &SymbolId) -> Option<Template> {
        self.symbol_table
            .get_symbol(symbol_id)
            .ty
            .as_ref()
            .map(|ty| Template::from_type(ty, &mut self.liquid_gen))
    }
}

