use merak_ast::{
    expression::{BinaryOperator, UnaryOperator},
    meta::SourceRef,
    predicate::{ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp},
    NodeId,
};
use std::collections::HashSet;
use z3::Context;

use crate::refinements::{
    constraints::{Constraint, ConstraintSet, TypeContext},
    qualifiers::QualifierSet,
    smt::SmtSolver,
    templates::{LiquidAssignment, LiquidVar, Template},
};
/// Result type for solver operations
pub type SolverResult<T> = Result<T, SolverError>;

/// Errors that can occur during constraint solving
#[derive(Debug, Clone)]
pub enum SolverError {
    /// A constraint cannot be satisfied
    UnsatisfiableConstraint {
        constraint: String,
        reason: String,
    },

    /// SMT solver timeout
    Timeout {
        constraint: String,
    },

    /// Internal solver error
    InternalError {
        message: String,
    },

    UnsatisfiableEnsures {
        message: String,
    },
}

impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SolverError::UnsatisfiableConstraint { constraint, reason } => {
                write!(f, "Unsatisfiable constraint: {} ({})", constraint, reason)
            }
            SolverError::Timeout { constraint } => {
                write!(f, "SMT solver timeout on: {}", constraint)
            }
            SolverError::InternalError { message } => {
                write!(f, "Internal solver error: {}", message)
            }
            SolverError::UnsatisfiableEnsures { message } => {
                write!(f, "Unsatisfiable ensures: {}", message)
            }
        }
    }
}

impl std::error::Error for SolverError {}

/// The constraint solver using iterative weakening
///
/// Implements the core Liquid Types algorithm:
/// 1. Generate candidate predicates from qualifiers
/// 2. For each liquid variable κ, find the weakest predicate that satisfies all constraints
/// 3. Use SMT solver to verify implications
pub struct ConstraintSolver<'ctx> {
    /// Available qualifiers for generating predicates
    qualifiers: QualifierSet,

    /// Current assignment of liquid variables to predicates
    assignment: LiquidAssignment,

    /// Constraints to solve
    constraints: ConstraintSet,

    /// SMT solver (reused across all checks)
    smt_solver: SmtSolver<'ctx>,
}

impl<'ctx> ConstraintSolver<'ctx> {
    /// Create a new constraint solver
    pub fn new(constraints: ConstraintSet, z3_context: &'ctx Context) -> Self {
        let qualifiers = QualifierSet::core();
        Self {
            qualifiers,
            assignment: LiquidAssignment::new(),
            constraints,
            smt_solver: SmtSolver::new(z3_context),
        }
    }

    /// Add constraints to solve
    pub fn add_constraints(&mut self, constraints: ConstraintSet) {
        self.constraints.extend(constraints.into_vec());
    }

    /// Solve all constraints and return the liquid variable assignment
    pub fn solve(mut self) -> SolverResult<LiquidAssignment> {
        // Step 1: Collect all liquid variables
        let liquid_vars = self.collect_liquid_vars();

        // Step 2: Initialize assignment (start with True for all)
        for var in &liquid_vars {
            self.assignment
                .assign(*var, Predicate::True(NodeId::new(0), SourceRef::unknown()));
        }

        // Step 3: Iteratively refine assignment
        let max_iterations = 100;
        for iteration in 0..max_iterations {
            let mut changed = false;

            // Temporarily take constraints to allow &mut self in loop
            // (safe: check/strengthen don't access self.constraints)
            let constraints = std::mem::take(&mut self.constraints);

            for constraint in constraints.iter() {
                if !self.check_constraint(constraint)? {
                    // Constraint not satisfied, strengthen liquid variables
                    changed |= self.strengthen_for_constraint(constraint)?;
                }
            }

            // Restore constraints to their original location
            self.constraints = constraints;

            if !changed {
                // Fixed point reached
                break;
            }

            if iteration == max_iterations - 1 {
                return Err(SolverError::InternalError {
                    message: "Maximum iterations reached without convergence".to_string(),
                });
            }
        }

        Ok(self.assignment)
    }

    /// Collect all liquid variables appearing in constraints
    fn collect_liquid_vars(&self) -> HashSet<LiquidVar> {
        let mut vars = HashSet::new();

        for constraint in self.constraints.iter() {
            match constraint {
                Constraint::Subtype { lhs, rhs, .. } => {
                    if let Some(var) = lhs.liquid_var() {
                        vars.insert(var);
                    }
                    if let Some(var) = rhs.liquid_var() {
                        vars.insert(var);
                    }
                }
                Constraint::BinaryOp {
                    left,
                    right,
                    result,
                    ..
                } => {
                    if let Some(var) = left.liquid_var() {
                        vars.insert(var);
                    }
                    if let Some(var) = right.liquid_var() {
                        vars.insert(var);
                    }
                    if let Some(var) = result.liquid_var() {
                        vars.insert(var);
                    }
                }
                Constraint::UnaryOp {
                    operand, result, ..
                } => {
                    if let Some(var) = operand.liquid_var() {
                        vars.insert(var);
                    }
                    if let Some(var) = result.liquid_var() {
                        vars.insert(var);
                    }
                }
                Constraint::WellFormed { template, .. } => {
                    if let Some(var) = template.liquid_var() {
                        vars.insert(var);
                    }
                }
                Constraint::Ensures { .. } => {}
            }
        }

        vars
    }

    /// Check if a constraint is satisfied under current assignment
    fn check_constraint(&mut self, constraint: &Constraint) -> SolverResult<bool> {
        match constraint {
            Constraint::WellFormed {
                context, template, ..
            } => self.check_well_formed(context, template),

            Constraint::Subtype {
                context, lhs, rhs, ..
            } => self.check_subtype(context, lhs, rhs),
            Constraint::BinaryOp {
                context,
                op,
                left,
                right,
                result,
                ..
            } => self.check_binary_op(context, op, left, right, result),

            Constraint::UnaryOp {
                context,
                op,
                operand,
                result,
                ..
            } => self.check_unary_op(context, op, operand, result),
            Constraint::Ensures {
                context,
                condition,
                location,
            } => self.check_ensures(context, condition, location),
        }
    }

    /// Check if a constraint is satisfied with a specific context
    fn check_constraint_with_context(
        &mut self,
        constraint: &Constraint,
        context: &TypeContext,
    ) -> SolverResult<bool> {
        match constraint {
            Constraint::WellFormed { template, .. } => self.check_well_formed(context, template),

            Constraint::Subtype { lhs, rhs, .. } => self.check_subtype(context, lhs, rhs),
            Constraint::BinaryOp {
                op,
                left,
                right,
                result,
                ..
            } => self.check_binary_op(context, op, left, right, result),

            Constraint::UnaryOp {
                op,
                operand,
                result,
                ..
            } => self.check_unary_op(context, op, operand, result),
            Constraint::Ensures {
                condition,
                location,
                ..
            } => self.check_ensures(context, condition, location),
        }
    }

    /// Check well-formedness: all free variables in refinement are in scope
    fn check_well_formed(&self, context: &TypeContext, template: &Template) -> SolverResult<bool> {
        if let Some(refinement) = template.refinement() {
            let free_vars = self.extract_free_variables(refinement);
            for var in free_vars {
                if !context.in_scope(&var) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Check subtyping: lhs <: rhs
    ///
    /// For refined types: {b | P₁} <: {b | P₂} iff ∀x. P₁(x) ⇒ P₂(x)
    fn check_subtype(
        &mut self,
        context: &TypeContext,
        lhs: &Template,
        rhs: &Template,
    ) -> SolverResult<bool> {
        // Check base types match
        if lhs.base_type() != rhs.base_type() {
            return Ok(false);
        }

        // Get predicates (applying current assignment for liquid vars)
        let lhs_pred = self.get_predicate(lhs);
        let rhs_pred = self.get_predicate(rhs);

        // Check implication: context ∧ lhs_pred ⇒ rhs_pred
        self.check_implication(context, &lhs_pred, &rhs_pred)
    }

    /// Check an ensures constraint
    ///
    /// Γ ⊢ P
    ///
    /// Given the context (which includes requires as assumptions and all
    /// transformations from the function body), verify that the postcondition P holds.
    fn check_ensures(
        &mut self,
        context: &TypeContext,
        condition: &Predicate,
        location: &SourceRef,
    ) -> Result<bool, SolverError> {
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            condition,
        )
    }

    /// Get the predicate for a template under current assignment
    fn get_predicate(&self, template: &Template) -> Predicate {
        if let Some(var) = template.liquid_var() {
            self.assignment
                .get(var)
                .cloned()
                .unwrap_or(Predicate::True(NodeId::new(0), SourceRef::unknown()))
        } else if let Some(pred) = template.refinement() {
            pred.clone()
        } else {
            Predicate::True(NodeId::new(0), SourceRef::unknown())
        }
    }

    fn check_binary_op(
        &mut self,
        context: &TypeContext,
        op: &BinaryOperator,
        left: &Template,
        right: &Template,
        result: &Template,
    ) -> SolverResult<bool> {
        // Get predicates for operands and result
        let left_pred = self.get_predicate(left);
        let right_pred = self.get_predicate(right);
        let result_pred = self.get_predicate(result);

        // Build arithmetic fact: result_val = left_val ⊗ right_val
        let arithmetic_fact =
            self.make_arithmetic_relation(op, left.binder(), right.binder(), result.binder());

        // Create combined antecedent: P_left ∧ P_right ∧ arithmetic_fact
        let antecedent = Predicate::And(
            Box::new(left_pred),
            Box::new(Predicate::And(
                Box::new(right_pred),
                Box::new(arithmetic_fact),
                NodeId::new(0),
                SourceRef::unknown(),
            )),
            NodeId::new(0),
            SourceRef::unknown(),
        );

        // Check: context ∧ antecedent ⇒ result_pred
        self.check_implication(context, &antecedent, &result_pred)
    }

    /// Check unary operation: result = op(operand)
    ///
    /// Verifies: Γ ∧ P_operand(x) ∧ (result = op(x)) ⇒ P_result(result)
    fn check_unary_op(
        &mut self,
        context: &TypeContext,
        op: &UnaryOperator,
        operand: &Template,
        result: &Template,
    ) -> SolverResult<bool> {
        let operand_pred = self.get_predicate(operand);
        let result_pred = self.get_predicate(result);

        // Build arithmetic fact: result_val = op(operand_val)
        let arithmetic_fact = self.make_unary_relation(op, operand.binder(), result.binder());

        // Create antecedent: P_operand ∧ arithmetic_fact
        let antecedent = Predicate::And(
            Box::new(operand_pred),
            Box::new(arithmetic_fact),
            NodeId::new(0),
            SourceRef::unknown(),
        );

        // Check: context ∧ antecedent ⇒ result_pred
        self.check_implication(context, &antecedent, &result_pred)
    }

    // Helper: Create arithmetic relation for binary operation
    /// Returns: result = left ⊗ right
    fn make_arithmetic_relation(
        &self,
        op: &BinaryOperator,
        left_binder: &str,
        right_binder: &str,
        result_binder: &str,
    ) -> Predicate {
        match op {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Modulo => {
                let arith_op = match op {
                    BinaryOperator::Add => ArithOp::Add,
                    BinaryOperator::Subtract => ArithOp::Sub,
                    BinaryOperator::Multiply => ArithOp::Mul,
                    BinaryOperator::Divide => ArithOp::Div,
                    BinaryOperator::Modulo => ArithOp::Mod,
                    _ => unreachable!("Unsupported binary operator"),
                };

                let left_expr = RefinementExpr::Var(
                    left_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let right_expr = RefinementExpr::Var(
                    right_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let result_expr = RefinementExpr::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let op_result = RefinementExpr::BinOp {
                    op: arith_op,
                    lhs: Box::new(left_expr),
                    rhs: Box::new(right_expr),
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                };

                Predicate::BinRel {
                    op: RelOp::Eq,
                    lhs: result_expr,
                    rhs: op_result,
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                }
            }
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual => {
                let rel_op = match op {
                    BinaryOperator::Equal => RelOp::Eq,
                    BinaryOperator::NotEqual => RelOp::Neq,
                    BinaryOperator::Less => RelOp::Lt,
                    BinaryOperator::LessEqual => RelOp::Leq,
                    BinaryOperator::Greater => RelOp::Gt,
                    BinaryOperator::GreaterEqual => RelOp::Geq,
                    _ => unreachable!(),
                };

                let left_expr = RefinementExpr::Var(
                    left_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let right_expr = RefinementExpr::Var(
                    right_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let comparison = Predicate::BinRel {
                    op: rel_op,
                    lhs: left_expr,
                    rhs: right_expr,
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                };

                let result_var = Predicate::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // result ⟺ comparison
                // Expressed as: (result → comparison) ∧ (comparison → result)
                let forward = Predicate::Implies(
                    Box::new(result_var.clone()),
                    Box::new(comparison.clone()),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let backward = Predicate::Implies(
                    Box::new(comparison),
                    Box::new(result_var),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                Predicate::And(
                    Box::new(forward),
                    Box::new(backward),
                    NodeId::new(0),
                    SourceRef::unknown(),
                )
            }
            BinaryOperator::LogicalAnd => {
                // result ⟺ (left ∧ right)

                let left_pred = Predicate::Var(
                    left_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let right_pred = Predicate::Var(
                    right_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let result_pred = Predicate::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let conjunction = Predicate::And(
                    Box::new(left_pred),
                    Box::new(right_pred),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // result ⟺ (left ∧ right)
                let forward = Predicate::Implies(
                    Box::new(result_pred.clone()),
                    Box::new(conjunction.clone()),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let backward = Predicate::Implies(
                    Box::new(conjunction),
                    Box::new(result_pred),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                Predicate::And(
                    Box::new(forward),
                    Box::new(backward),
                    NodeId::new(0),
                    SourceRef::unknown(),
                )
            }

            BinaryOperator::LogicalOr => {
                let left_pred = Predicate::Var(
                    left_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let right_pred = Predicate::Var(
                    right_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let result_pred = Predicate::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let disjunction = Predicate::Or(
                    Box::new(left_pred),
                    Box::new(right_pred),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let forward = Predicate::Implies(
                    Box::new(result_pred.clone()),
                    Box::new(disjunction.clone()),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let backward = Predicate::Implies(
                    Box::new(disjunction),
                    Box::new(result_pred),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                Predicate::And(
                    Box::new(forward),
                    Box::new(backward),
                    NodeId::new(0),
                    SourceRef::unknown(),
                )
            }
        }
    }

    /// Helper: Create arithmetic relation for unary operation
    /// Returns: result = op(operand)
    fn make_unary_relation(
        &self,
        op: &UnaryOperator,
        operand_binder: &str,
        result_binder: &str,
    ) -> Predicate {
        match op {
            UnaryOperator::Negate => {
                let operand_expr = RefinementExpr::Var(
                    operand_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let result_expr = RefinementExpr::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                let negated = RefinementExpr::UnaryOp {
                    op: UnaryOp::Negate,
                    expr: Box::new(operand_expr),
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                };

                // result = -operand
                Predicate::BinRel {
                    op: RelOp::Eq,
                    lhs: result_expr,
                    rhs: negated,
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                }
            }

            UnaryOperator::Not => {
                let operand_pred = Predicate::Var(
                    operand_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );
                let result_pred = Predicate::Var(
                    result_binder.to_string(),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // ¬operand
                let not_operand = Predicate::Not(
                    Box::new(operand_pred.clone()),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // ¬result
                let not_result = Predicate::Not(
                    Box::new(result_pred.clone()),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // result → ¬operand
                let forward = Predicate::Implies(
                    Box::new(result_pred),
                    Box::new(not_operand),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // ¬result → operand
                let backward = Predicate::Implies(
                    Box::new(not_result),
                    Box::new(operand_pred),
                    NodeId::new(0),
                    SourceRef::unknown(),
                );

                // (result → ¬operand) ∧ (¬result → operand)
                Predicate::And(
                    Box::new(forward),
                    Box::new(backward),
                    NodeId::new(0),
                    SourceRef::unknown(),
                )
            }
        }
    }

    /// Check if an implication holds: context ∧ antecedent ⇒ consequent
    ///
    /// This would call the SMT solver (Z3) in the full implementation
    fn check_implication(
        &mut self,
        context: &TypeContext,
        antecedent: &Predicate,
        consequent: &Predicate,
    ) -> SolverResult<bool> {
        if matches!(consequent, Predicate::True(..)) {
            return Ok(true); // Anything implies True
        }
        if matches!(antecedent, Predicate::False(..)) {
            return Ok(true); // False implies anything
        }

        let assumptions = context.assumptions();
        self.smt_solver
            .is_valid_implication(assumptions, antecedent, consequent)
            .map_err(|e| SolverError::InternalError {
                message: format!("SMT error: {}", e),
            })
    }

    /// Strengthen assignment to satisfy a constraint
    fn strengthen_for_constraint(&mut self, constraint: &Constraint) -> SolverResult<bool> {
        match constraint {
            Constraint::Subtype {
                context, lhs, rhs, ..
            } => {
                // If rhs has a liquid variable, try to strengthen it
                if let Some(kappa) = rhs.liquid_var() {
                    return self.strengthen_liquid_var(kappa, context, lhs);
                }
                Ok(false)
            }
            _ => Ok(false), // Only strengthen for Subtype constraints
        }
    }

    /// Strengthen a liquid variable by adding qualifiers
    fn strengthen_liquid_var(
        &mut self,
        var: LiquidVar,
        context: &TypeContext,
        lhs: &Template,
    ) -> SolverResult<bool> {
        // Generate candidate qualifiers
        let candidates = self.qualifiers.instantiate_all(context);

        // Current predicate
        let current = self
            .assignment
            .get(var)
            .cloned()
            .unwrap_or(Predicate::True(NodeId::new(0), SourceRef::unknown()));

        // Try adding each candidate
        for candidate in candidates {
            let strengthened = self.conjoin(&current, &candidate);

            // Check if this helps satisfy the constraint
            let lhs_pred = self.get_predicate(lhs);
            if self.check_implication(context, &lhs_pred, &strengthened)? {
                self.assignment.assign(var, strengthened);
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Conjoin two predicates: p₁ ∧ p₂
    fn conjoin(&self, p1: &Predicate, p2: &Predicate) -> Predicate {
        if matches!(p1, Predicate::True(..)) {
            return p2.clone();
        }
        if matches!(p2, Predicate::True(..)) {
            return p1.clone();
        }

        Predicate::And(
            Box::new(p1.clone()),
            Box::new(p2.clone()),
            NodeId::new(0),
            SourceRef::unknown(),
        )
    }

    /// Extract free variables from a predicate
    fn extract_free_variables(&self, predicate: &Predicate) -> HashSet<String> {
        let mut vars = HashSet::new();
        self.collect_vars_from_predicate(predicate, &mut vars);
        vars
    }

    /// Helper to recursively collect variables
    fn collect_vars_from_predicate(&self, predicate: &Predicate, vars: &mut HashSet<String>) {
        match predicate {
            Predicate::Var(name, ..) => {
                if name != "ν" {
                    vars.insert(name.clone());
                }
            }
            Predicate::BinRel { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs, vars);
                self.collect_vars_from_expr(rhs, vars);
            }
            Predicate::And(p1, p2, ..)
            | Predicate::Or(p1, p2, ..)
            | Predicate::Implies(p1, p2, ..) => {
                self.collect_vars_from_predicate(p1, vars);
                self.collect_vars_from_predicate(p2, vars);
            }
            Predicate::Not(p, ..) => {
                self.collect_vars_from_predicate(p, vars);
            }
            _ => {}
        }
    }

    /// Helper to collect variables from expressions
    fn collect_vars_from_expr(&self, expr: &RefinementExpr, vars: &mut HashSet<String>) {
        match expr {
            RefinementExpr::Var(name, ..) => {
                if name != "v" {
                    vars.insert(name.clone());
                }
            }
            RefinementExpr::BinOp { lhs, rhs, .. } => {
                self.collect_vars_from_expr(lhs, vars);
                self.collect_vars_from_expr(rhs, vars);
            }
            RefinementExpr::UnaryOp { expr, .. } => {
                self.collect_vars_from_expr(expr, vars);
            }
            RefinementExpr::UninterpFn { args, .. } => {
                for arg in args {
                    self.collect_vars_from_expr(arg, vars);
                }
            }
            _ => {}
        }
    }
}
