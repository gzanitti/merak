use std::collections::HashMap;
use std::fmt;

use merak_ast::{
    expression::{BinaryOperator, UnaryOperator},
    meta::SourceRef,
    predicate::{Predicate, RefinementExpr},
};
use merak_symbols::SymbolId;

use crate::refinements::templates::Template;

/// A refinement type constraint
///
/// Constraints represent typing obligations that must be satisfied
/// for the program to be well-typed.
#[derive(Debug, Clone)]
pub enum Constraint {
    /// Well-formedness: a type is valid in a context
    ///
    /// Γ ⊢ τ WF
    ///
    /// Checks that all free variables in the refinement are in scope
    WellFormed {
        context: TypeContext,
        template: Template,
        location: SourceRef,
    },

    /// Subtyping: one type is a subtype of another
    ///
    /// Γ ⊢ τ₁ <: τ₂
    ///
    /// Core constraint: τ₁ <: τ₂ iff ∀x. P₁(x) ⇒ P₂(x)
    Subtype {
        context: TypeContext,
        sub: Template,
        sup: Template,
        location: SourceRef,
    },

    // Binary operation: result of applying a binary operator
    ///
    /// Γ ⊢ result = left ⊗ right
    ///
    /// Encodes the semantic relationship between operands and result.
    /// For example: dest = x + y where x: {v | v ≥ 0}, y: {v | v > 0}
    /// SMT encoding includes the arithmetic fact: result = left + right
    /// This allows Z3 to reason about properties like:
    ///   if left ≥ 0 and right > 0, then result > 0
    BinaryOp {
        context: TypeContext,
        op: BinaryOperator,
        left: Template,
        right: Template,
        result: Template,
        location: SourceRef,
    },

    /// Unary operation: result of applying a unary operator
    ///
    /// Γ ⊢ result = op(operand)
    ///
    /// Encodes unary operations like negation or boolean not.
    /// For example: dest = -x where x: {v | v > 0}
    /// SMT can deduce: dest: {v | v < 0}
    UnaryOp {
        context: TypeContext,
        op: UnaryOperator,
        operand: Template,
        result: Template,
        location: SourceRef,
    },

    /// Precondition: caller must establish this predicate
    ///
    /// Γ ⊢ P  (at call site)
    ///
    /// Requires clause verification: before calling a function,
    /// the precondition P must be provable from the current context
    Requires {
        context: TypeContext,
        condition: Predicate,
        location: SourceRef,
    },

    /// Postcondition: a predicate must hold at function exit
    ///
    /// Γ ⊢ P
    ///
    /// Ensures clause verification: given the current context (including
    /// requires as assumptions and all transformations from the function body),
    /// the postcondition P must be provable.
    Ensures {
        context: TypeContext,
        condition: Predicate,
        location: SourceRef,
    },

    // Loop invariant must hold at loop entry
    /// Γ ⊢ I
    LoopInvariantEntry {
        context: TypeContext,
        invariant: Predicate,
        location: SourceRef,
    },
    
    /// Loop invariant must be preserved by loop body
    /// Γ, I ⊢ body ⇒ I
    LoopInvariantPreservation {
        context: TypeContext,
        invariant: Predicate,
        location: SourceRef,
    },
    
    /// Loop variant must be non-negative
    /// Γ ⊢ V ≥ 0
    LoopVariantNonNegative {
        context: TypeContext,
        variant: RefinementExpr,
        location: SourceRef,
    },
    
    /// Loop variant must decrease
    /// Γ ⊢ V_after < V_before
    LoopVariantDecreases {
        context: TypeContext,
        variant_before: RefinementExpr,
        variant_after: RefinementExpr,
        location: SourceRef,
    },

    /// Fold: verify storage invariant
    ///
    /// Γ ⊢ fold(var) : assert(refinement(var))
    ///
    /// At fold point, must prove that the storage variable
    /// satisfies its declared refinement
    Fold {
        context: TypeContext,
        var: SymbolId,           // Storage variable
        refinement: Predicate,   // Its declared refinement
        location: SourceRef,
    },

    // /// Unfold: assume storage invariant
    // ///
    // /// Γ ⊢ unfold(var) : assume(refinement(var))
    // ///
    // /// At unfold point, add the storage variable's refinement
    // /// as an assumption (it was verified at last fold)
    // Unfold {
    //     context: TypeContext,
    //     var: SymbolId,           // Storage variable
    //     refinement: Predicate,   // Its declared refinement
    //     location: SourceRef,
    // },
}

impl Constraint {
    /// Get the location of this constraint
    pub fn location(&self) -> &SourceRef {
        match self {
            Constraint::WellFormed { location, .. } => location,
            Constraint::Subtype { location, .. } => location,
            Constraint::BinaryOp { location, .. } => location,
            Constraint::UnaryOp { location, .. } => location,
            Constraint::Requires { location, .. } => location,
            Constraint::Ensures { location, .. } => location,
            Constraint::LoopInvariantEntry { location, .. } => location,
            Constraint::LoopInvariantPreservation { location, .. } => location,
            Constraint::LoopVariantNonNegative { location, .. } => location,
            Constraint::LoopVariantDecreases { location, .. } => location,
            Constraint::Fold { location, .. } => location,
        }
    }

    /// Get the context of this constraint
    pub fn context(&self) -> &TypeContext {
        match self {
            Constraint::WellFormed { context, .. } => context,
            Constraint::Subtype { context, .. } => context,
            Constraint::BinaryOp { context, .. } => context,
            Constraint::UnaryOp { context, .. } => context,
            Constraint::Requires { context, .. } => context,
            Constraint::Ensures { context, .. } => context,
            Constraint::LoopInvariantEntry { context, .. } => context,
            Constraint::LoopInvariantPreservation { context, .. } => context,
            Constraint::LoopVariantNonNegative { context, .. } => context,
            Constraint::LoopVariantDecreases { context, .. } => context,
            Constraint::Fold { context, .. } => context,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Constraint::WellFormed { template, .. } => {
                write!(f, "WF({template})")
            }
            Constraint::Subtype { sub: lhs, sup: rhs, .. } => {
                write!(f, "{lhs} <: {rhs}")
            }
            Constraint::BinaryOp {
                left,
                right,
                result,
                ..
            } => {
                write!(f, "{result} = {left} ⊗ {right}")
            }
            Constraint::UnaryOp {
                op,
                operand,
                result,
                ..
            } => {
                write!(f, "{op} = {operand}{result}")
            }
            Constraint::Requires {  condition, .. } => {
                write!(f, "REQUIRES({condition})")
            }
            Constraint::Ensures { condition, .. } => {
                write!(f, "ENSURE({condition})")
            }
            Constraint::LoopInvariantEntry { invariant, .. } => {
                write!(f, "LOOP_INVARIANT_ENTRY({invariant})")
            }
            Constraint::LoopInvariantPreservation { invariant, .. } => {
                write!(f, "LOOP_INVARIANT_PRESERVATION({invariant})")
            }
            Constraint::LoopVariantNonNegative { variant, .. } => {
                write!(f, "LOOP_INVARIANT_NON_NEGATIVE({variant})")
            }
            Constraint::LoopVariantDecreases { variant_before, variant_after, .. } => {
                write!(f, "LOOP_VARIANT_DECREASES({variant_after} < {variant_before})")
            }
            Constraint::Fold { var, refinement, .. } => {
                write!(f, "FOLD({var}, {refinement})")
            }
            // Constraint::Unfold { var, refinement, .. } => {
            //     write!(f, "UNFOLD({var}, {refinement})")
            // }

        }
    }
}

/// Type context (Γ): maps variables to their types and tracks assumptions
///
/// Represents the typing environment at a given program point.
#[derive(Debug, Clone)]
pub struct TypeContext {
    /// Variable bindings: maps variable names to their templates
    bindings: Vec<(String, Template)>,

    /// Path conditions: predicates assumed to be true at this point
    ///
    /// Example: inside `if (x > 0) { ... }`, we have (x > 0) in assumptions
    assumptions: Vec<Predicate>,

    /// Source-level variable names to SSA register names
    /// Allows the solver to resolve cross-variable references in refinements
    /// (e.g., `{v: int | v > x}` where `x` is a source name but context has `x_0`)
    source_to_ssa: HashMap<String, String>,
}

impl TypeContext {
    /// Create an empty context
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            assumptions: Vec::new(),
            source_to_ssa: HashMap::new(),
        }
    }

    /// Add a variable binding to the context
    pub fn bind(&mut self, name: String, template: Template) {
        self.bindings.push((name, template));
    }

    /// Add an assumption to the context
    pub fn assume(&mut self, predicate: Predicate) {
        self.assumptions.push(predicate);
    }

    /// Look up a variable in the context
    pub fn lookup(&self, name: &str) -> Option<&Template> {
        self.bindings
            .iter()
            .rev() // Search from most recent
            .find(|(n, _)| n == name)
            .map(|(_, t)| t)
    }

    /// Get all bindings
    pub fn bindings(&self) -> &[(String, Template)] {
        &self.bindings
    }

    /// Get all assumptions
    pub fn assumptions(&self) -> &[Predicate] {
        &self.assumptions
    }

    /// Set the source-to-SSA name mapping for cross-variable reference resolution
    pub fn set_source_to_ssa(&mut self, mapping: HashMap<String, String>) {
        self.source_to_ssa = mapping;
    }

    /// Get the source-to-SSA name mapping
    pub fn source_to_ssa(&self) -> &HashMap<String, String> {
        &self.source_to_ssa
    }

    // /// Create a copy of this context with an additional binding
    // pub fn with_binding(&self, name: String, template: Template) -> Self {
    //     let mut new_ctx = self.clone();
    //     new_ctx.bind(name, template);
    //     new_ctx
    // }

    // /// Create a copy of this context with an additional assumption
    // pub fn with_assumption(&self, predicate: Predicate) -> Self {
    //     let mut new_ctx = self.clone();
    //     new_ctx.assume(predicate);
    //     new_ctx
    // }

    /// Check if a variable is in scope
    pub fn in_scope(&self, name: &str) -> bool {
        self.lookup(name).is_some()
    }
}

impl fmt::Display for TypeContext {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Γ{{")?;

        // Write bindings
        for (i, (name, template)) in self.bindings.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", name, template)?;
        }

        // Write assumptions
        if !self.assumptions.is_empty() {
            write!(f, " | ")?;
            for (i, pred) in self.assumptions.iter().enumerate() {
                if i > 0 {
                    write!(f, " ∧ ")?;
                }
                write!(f, "{:?}", pred)?;
            }
        }

        write!(f, "}}")
    }
}

/// Collection of constraints generated from a function or statement
#[derive(Debug, Clone)]
pub struct ConstraintSet {
    constraints: Vec<Constraint>,
}

impl ConstraintSet {
    /// Create an empty constraint set
    pub fn new() -> Self {
        Self {
            constraints: Vec::new(),
        }
    }

    /// Add a constraint to the set
    pub fn add(&mut self, constraint: Constraint) {
        self.constraints.push(constraint);
    }

    /// Add multiple constraints
    pub fn extend(&mut self, constraints: Vec<Constraint>) {
        self.constraints.extend(constraints);
    }

    /// Merge another constraint set into this one
    pub fn merge(&mut self, other: ConstraintSet) {
        self.constraints.extend(other.constraints);
    }

    /// Get all constraints
    pub fn constraints(&self) -> &[Constraint] {
        &self.constraints
    }

    /// Number of constraints
    pub fn len(&self) -> usize {
        self.constraints.len()
    }

    /// Is empty?
    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    /// Iterate over constraints
    pub fn iter(&self) -> impl Iterator<Item = &Constraint> {
        self.constraints.iter()
    }

    /// Iterate over mutable constraints
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Constraint> {
        self.constraints.iter_mut()
    }

    /// Get mutable access to the internal Vec
    pub fn constraints_mut(&mut self) -> &mut Vec<Constraint> {
        &mut self.constraints
    }

    /// Consume and get all constraints
    pub fn into_vec(self) -> Vec<Constraint> {
        self.constraints
    }
}

impl Default for ConstraintSet {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for TypeContext {
    fn default() -> Self {
        Self::new()
    }
}
