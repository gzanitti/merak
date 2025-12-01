use std::fmt;

use merak_ast::{
    expression::{BinaryOperator, UnaryOperator},
    meta::SourceRef,
    predicate::Predicate,
};

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
        lhs: Template,
        rhs: Template,
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
}

impl Constraint {
    /// Create a well-formedness constraint
    pub fn well_formed(context: TypeContext, template: Template, location: SourceRef) -> Self {
        Constraint::WellFormed {
            context,
            template,
            location,
        }
    }

    /// Create a subtyping constraint
    pub fn subtype(
        context: TypeContext,
        lhs: Template,
        rhs: Template,
        location: SourceRef,
    ) -> Self {
        Constraint::Subtype {
            context,
            lhs,
            rhs,
            location,
        }
    }

    /// Create a guard constraint
    // pub fn guard(
    //     context: TypeContext,
    //     condition: Predicate,
    //     then_constraints: Vec<Constraint>,
    //     location: SourceRef,
    // ) -> Self {
    //     Constraint::Guard {
    //         context,
    //         condition,
    //         then_constraints,
    //         location,
    //     }
    // }

    /// Get the location of this constraint
    pub fn location(&self) -> &SourceRef {
        match self {
            Constraint::WellFormed { location, .. } => location,
            Constraint::Subtype { location, .. } => location,
            Constraint::BinaryOp { location, .. } => location,
            Constraint::UnaryOp { location, .. } => location,
            Constraint::Ensures { location, .. } => location,
        }
    }

    /// Get the context of this constraint
    pub fn context(&self) -> &TypeContext {
        match self {
            Constraint::WellFormed { context, .. } => context,
            Constraint::Subtype { context, .. } => context,
            Constraint::BinaryOp { context, .. } => context,
            Constraint::UnaryOp { context, .. } => context,
            Constraint::Ensures { context, .. } => context,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Constraint::WellFormed { template, .. } => {
                write!(f, "WF({template})")
            }
            Constraint::Subtype { lhs, rhs, .. } => {
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
            Constraint::Ensures { condition, .. } => {
                write!(f, "ENSURE({condition})")
            }
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
}

impl TypeContext {
    /// Create an empty context
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
            assumptions: Vec::new(),
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

    /// Create a copy of this context with an additional binding
    pub fn with_binding(&self, name: String, template: Template) -> Self {
        let mut new_ctx = self.clone();
        new_ctx.bind(name, template);
        new_ctx
    }

    /// Create a copy of this context with an additional assumption
    pub fn with_assumption(&self, predicate: Predicate) -> Self {
        let mut new_ctx = self.clone();
        new_ctx.assume(predicate);
        new_ctx
    }

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
