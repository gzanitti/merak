use merak_ast::{
    expression::{BinaryOperator, UnaryOperator},
    meta::SourceRef,
    predicate::{ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp},
    NodeId,
};
use merak_symbols::SymbolId;
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

    /// Type mismatch between refinements
    TypeMismatch {
        message: String
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
            SolverError::TypeMismatch { message } => {
                write!(f, "Type mismatch: {}", message)
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
    // pub fn add_constraints(&mut self, constraints: ConstraintSet) {
    //     self.constraints.extend(constraints.into_vec());
    // }

    /// Solve all constraints and return the liquid variable assignment
    pub fn solve(mut self) -> SolverResult<LiquidAssignment> {
        println!("[SOLVER] Starting with {} constraints", self.constraints.len());

        let mut subtype_count = 0;
        let mut binary_op_count = 0;
        let mut other_count = 0;
        for c in self.constraints.iter() {
            match c {
                Constraint::Subtype { context, sub, sup, .. } => {
                    println!("  - Subtype: {} <: {}", sub, sup);
                    println!("  - Context (asumps): ");
                    for ctx in context.assumptions() {
                        println!("    - {} ", ctx);
                    }
                    subtype_count += 1;
                }
                Constraint::BinaryOp { .. } => binary_op_count += 1,
                _ => other_count += 1,
            }
        }
        println!("[SOLVER] Breakdown: {} Subtype, {} BinaryOp, {} Other", subtype_count, binary_op_count, other_count);

        // Step 1: Collect all liquid variables
        let liquid_vars = self.collect_liquid_vars();
        println!("[SOLVER] Found {} liquid variables: {:?}", liquid_vars.len(), liquid_vars);

        // Step 2: Initialize assignment (start with True for all)
        for var in &liquid_vars {
            self.assignment
                .assign(*var, Predicate::True(NodeId::new(0), SourceRef::unknown()));
        }

        // Step 3: Iteratively refine assignment
        let max_iterations = 100;
        for iteration in 0..max_iterations {
            println!("[SOLVER] Iteration {}", iteration);
            let mut changed = false;

            // Temporarily take constraints to allow &mut self in loop
            // (safe: check/strengthen don't access self.constraints)
            let mut constraints = std::mem::take(&mut self.constraints);
            let constraints_vec = constraints.constraints_mut();

            // Use indices instead of iter_mut to avoid borrow conflicts
            for i in 0..constraints_vec.len() {
                let check_result = self.check_constraint(&mut constraints_vec[i])?;
                if !check_result {
                    println!("[SOLVER] Constraint {} FAILED, attempting strengthening", i);
                    // Constraint not satisfied, try to strengthen liquid variables
                    match self.strengthen_for_constraint(&constraints_vec[i], constraints_vec) {
                        Ok(did_change) => {
                            changed |= did_change;
                        }
                        Err(e) => {
                            // Hard constraint failed (loop invariants/variants)
                            // These are fatal errors
                            self.constraints = constraints;
                            return Err(e);
                        }
                    }
                } else {
                    println!("[SOLVER] Constraint {} PASSED", i);
                }
            }

            // Restore constraints to their original location
            self.constraints = constraints;

            if !changed {
                // Fixed point reached
                println!("[SOLVER] Fixed point reached after {} iterations", iteration);
                println!("[SOLVER] Final assignment:");
                for (var, pred) in self.assignment.iter() {
                    println!("  κ{} = {}", var.0, pred);
                }
                break;
            }

            if iteration == max_iterations - 1 {
                return Err(SolverError::InternalError {
                    message: "Maximum iterations reached without convergence".to_string(),
                });
            }
        }

        // Step 4: Final verification - check all constraints are satisfied
        println!("[SOLVER] Final verification: checking all constraints");
        let mut constraints = std::mem::take(&mut self.constraints);
        let constraints_vec = constraints.constraints_mut();

        for (i, constraint) in constraints_vec.iter_mut().enumerate() {
            println!("[SOLVER] Final verification: checking constraint {}", i);
            if !self.check_constraint(constraint)? {
                // Failed constraint after fixed point - report error
                let error_msg = match constraint {
                    Constraint::Subtype { sub, sup, location, .. } => {
                        format!(
                            "Type error: cannot satisfy subtyping constraint\n\
                            Expected: {}\n\
                            Found: {}\n\
                            Location: {:?}",
                            sup, sub, location
                        )
                    }
                    _ => format!("Constraint {} failed verification", i),
                };
                self.constraints = constraints;
                return Err(SolverError::TypeMismatch { message: error_msg });
            }
        }

        self.constraints = constraints;
        println!("[SOLVER] All constraints verified successfully");
        Ok(self.assignment)
    }

    /// Collect all liquid variables appearing in constraints
    fn collect_liquid_vars(&self) -> HashSet<LiquidVar> {
        let mut vars = HashSet::new();

        for constraint in self.constraints.iter() {
            match constraint {
                Constraint::Subtype { sub: lhs, sup: rhs, .. } => {
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
                Constraint::Ensures { .. } | 
                Constraint::Requires { .. } |
                Constraint::Fold { .. } | 
                //Constraint::Unfold { .. } |
                Constraint::LoopInvariantEntry { .. } | 
                Constraint::LoopInvariantPreservation { .. } | 
                Constraint::LoopVariantDecreases { .. } | 
                Constraint::LoopVariantNonNegative { .. } => {}
            }
        }

        vars
    }

    /// Check if a constraint is satisfied under current assignment
    fn check_constraint(&mut self, constraint: &mut Constraint) -> SolverResult<bool> {
        match constraint {
            Constraint::WellFormed {
                context, template, ..
            } => {
                //println!("[DEBUG check_constraint] WellFormed: context={:?}, template={:?}", context, template);
                self.check_well_formed(context, template)
            }

            Constraint::Subtype {
                context, sub: lhs, sup: rhs, ..
            } => {
                println!("Subtype: {} <: {}", lhs, rhs);
                //println!("[DEBUG check_constraint] Subtype: context={:?}, lhs={:?}, rhs={:?}", context, lhs, rhs);
                self.check_subtype(context, lhs, rhs)
            }
            Constraint::BinaryOp {
                context,
                op,
                left,
                right,
                result,
                ..
            } => {
                println!("BinaryOp");
                //println!("[DEBUG check_constraint] BinaryOp: context={:?}, op={:?}, left={:?}, right={:?}, result={:?}", context, op, left, right, result);
                self.check_binary_op(context, op, left, right, result)
            }

            Constraint::UnaryOp {
                context,
                op,
                operand,
                result,
                ..
            } => {
                //println!("[DEBUG check_constraint] UnaryOp: context={:?}, op={:?}, operand={:?}, result={:?}", context, op, operand, result);
                self.check_unary_op(context, op, operand, result)
            }
            Constraint::Ensures {
                context,
                condition,
                ..
            } => {
                //println!("[DEBUG check_constraint] Ensures: context={:?}, condition={:?}", context, condition);
                self.check_ensures(context, condition)
            }
            Constraint::Requires {
                context,
                condition,
                ..
            } => {
                //println!("[DEBUG check_constraint] Requires: context={:?}, condition={:?}", context, condition);
                self.check_requires(context, condition)
            }
            Constraint::LoopInvariantEntry {
                context,
                invariant,
                ..
            } => {
                //println!("[DEBUG check_constraint] LoopInvariantEntry: context={:?}, invariant={:?}", context, invariant);
                self.check_loop_invariant_entry(context, invariant)
            }
            Constraint::LoopInvariantPreservation {
                context,
                invariant,
                ..
            } => {
                //println!("[DEBUG check_constraint] LoopInvariantPreservation: context={:?}, invariant={:?}", context, invariant);
                self.check_loop_invariant_preservation(context, invariant)
            }
            Constraint::LoopVariantDecreases {
                context,
                variant_before,
                variant_after,
                ..
            } => {
                self.check_variant_decreases(context, variant_before, variant_after)
            }
            Constraint::LoopVariantNonNegative {
                context,
                variant,
                ..
            } => {
                //println!("[DEBUG check_constraint] LoopVariantNonNegative: context={:?}, variant={:?}", context, variant);
                self.check_variant_non_negative(context, variant)
            }
            Constraint::Fold {
                context,
                var,
                refinement,
                ..
            } => {
                //println!("[DEBUG check_constraint] Fold: context={:?}, var={:?}, refinement={:?}", context, var, refinement);
                self.check_fold(context, var, refinement)
            }
        }
    }

    /// Check if a constraint is satisfied with a specific context
    // fn check_constraint_with_context(
    //     &mut self,
    //     constraint: &mut Constraint,
    //     context: &TypeContext,
    // ) -> SolverResult<bool> {
    //     match constraint {
    //         Constraint::WellFormed { template, .. } => self.check_well_formed(context, template),
    //         Constraint::Subtype { sub: lhs, sup: rhs, .. } => self.check_subtype(context, lhs, rhs),
    //         Constraint::BinaryOp {
    //             op,
    //             left,
    //             right,
    //             result,
    //             ..
    //         } => self.check_binary_op(context, op, left, right, result),

    //         Constraint::UnaryOp {
    //             op,
    //             operand,
    //             result,
    //             ..
    //         } => self.check_unary_op(context, op, operand, result),
    //         Constraint::Ensures {
    //             condition,
    //             ..
    //         } => self.check_ensures(context, condition),
    //         Constraint::Requires { 
    //             condition,
    //             .. 
    //         } => self.check_requires(context, condition),
    //         Constraint::LoopInvariantEntry { 
    //             invariant, 
    //             .. 
    //         } => self.check_loop_invariant_entry(context, invariant),
    //         Constraint::LoopInvariantPreservation { 
    //             invariant, 
    //             .. 
    //         } => self.check_loop_invariant_preservation(context, invariant),
    //         Constraint::LoopVariantDecreases {
    //             entry_context, 
    //             variant, 
    //             .. 
    //         } => self.check_variant_decreases(entry_context, context, variant),
    //         Constraint::LoopVariantNonNegative { 
    //             variant, 
    //             .. 
    //         } => self.check_variant_non_negative(context, variant),
    //         Constraint::Fold { 
    //             var,
    //             refinement, 
    //             ..
    //         } => self.check_fold(context, var, refinement),
    //         // Constraint::Unfold { 
    //         //     var, 
    //         //     refinement, 
    //         //     ..
    //         // } => self.check_unfold(context, var, refinement)
    //     }
    // }

    /// Check well-formedness: all free variables in refinement are in scope
    fn check_well_formed(&self, context: &TypeContext, template: &Template) -> SolverResult<bool> {
        println!("[CHECK_WF] Template: {}", template);
        if let Some(refinement) = template.refinement() {
            let free_vars = self.extract_free_variables(refinement);
            println!("[CHECK_WF] Free vars in refinement: {:?}", free_vars);
            for var in &free_vars {
                println!("[CHECK_WF] Checking if '{}' is in scope", var);
                if !context.in_scope(var) {
                    println!("[CHECK_WF] Variable '{}' NOT in scope!", var);
                    let binding_names: Vec<_> = context.bindings().iter().map(|(name, _)| name).collect();
                    println!("[CHECK_WF] Context bindings: {:?}", binding_names);
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
        lhs: &mut Template,
        rhs: &mut Template,
    ) -> SolverResult<bool> {
        // Check base types match
        if lhs.base_type() != rhs.base_type() {
            return Ok(false);
        }

        // Clone RHS to avoid permanent mutation of the constraint
        let mut rhs_aligned = rhs.clone();
        rhs_aligned.replace_binder(lhs.binder());

        let lhs_pred = self.get_predicate(lhs);
        let rhs_pred = self.get_predicate(&rhs_aligned);

        // If RHS has liquid variable that is still True, force strengthening
        if rhs_aligned.liquid_var().is_some() && matches!(rhs_pred, Predicate::True(..)) {
            println!("[CHECK_SUBTYPE] Forcing strengthening for {} with liquid var κ{}", rhs_aligned, rhs_aligned.liquid_var().unwrap().0);
            return Ok(false);
        }

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
    ) -> Result<bool, SolverError> {
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            condition,
        )
    }

    /// Check precondition at call site
    ///
    /// Γ ⊢ P
    ///
    /// At the call site, verify that the precondition P (declared in the
    /// function being called) is satisfied by the current context.
    fn check_requires(
        &mut self,
        context: &TypeContext,
        condition: &Predicate,
    ) -> Result<bool, SolverError> {
        // Verify: context ⇒ condition
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            condition,
        )
    }

    /// Check fold operation
    ///
    /// fold(var) → assert(refinement(var))
    ///
    /// Fold is an ASSERTION. We must prove that after manipulating the storage
    /// variable, its declared refinement still holds. This is the core of
    /// storage safety verification.
    fn check_fold(
        &mut self,
        context: &TypeContext,
        var: &SymbolId,
        refinement: &Predicate,
    ) -> Result<bool, SolverError> {
        // Verify: context ⇒ refinement
        // The context includes all transformations that happened while
        // the variable was unfolded (loads, stores, computations)
        
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            refinement,
        )
    }

    fn check_loop_invariant_entry(
        &mut self,
        context: &TypeContext,
        invariant: &Predicate,
    ) -> Result<bool, SolverError> {
        // Γ ⊢ I (invariant must hold at entry)
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            invariant,
        )
    }

    fn check_loop_invariant_preservation(
        &mut self,
        context: &TypeContext,
        invariant: &Predicate,
    ) -> Result<bool, SolverError> {
        // Context already includes invariant as assumption
        // and all transformations from loop body
        // We just need to check that invariant still holds
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            invariant,
        )
    }

    fn check_variant_non_negative(
        &mut self,
        context: &TypeContext,
        variant: &RefinementExpr,
    ) -> Result<bool, SolverError> {
        // Convert variant ≥ 0 to predicate and check
        let zero = RefinementExpr::IntLit(0, NodeId::new(0), SourceRef::unknown());
        let pred = Predicate::BinRel {
            op: RelOp::Geq,
            lhs: variant.clone(),
            rhs: zero,
            id: NodeId::new(0),
            source_ref: SourceRef::unknown(),
        };
        
        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            &pred,
        )
    }

    fn check_variant_decreases(
        &mut self,
        context: &TypeContext,
        variant_before: &RefinementExpr,
        variant_after: &RefinementExpr,
    ) -> Result<bool, SolverError> {
        // Build predicate: V_after < V_before (strict decrease)
        let pred = Predicate::BinRel {
            op: RelOp::Lt,
            lhs: variant_after.clone(),
            rhs: variant_before.clone(),
            id: NodeId::new(0),
            source_ref: SourceRef::unknown(),
        };

        self.check_implication(
            context,
            &Predicate::True(NodeId::new(0), SourceRef::unknown()),
            &pred,
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

        // Collect explicit assumptions
        let mut all_assumptions: Vec<Predicate> = context.assumptions().to_vec();

        // Collect variables mentioned in antecedent and consequent
        let mut vars = HashSet::new();
        self.collect_vars_from_predicate(antecedent, &mut vars);
        self.collect_vars_from_predicate(consequent, &mut vars);

        // Add refinements for relevant bindings (both Concrete and Liquid)
        for var_name in &vars {
            // Try direct lookup first (SSA name), then resolve source name → SSA name
            let template = if let Some(t) = context.lookup(var_name) {
                t
            } else if let Some(ssa_name) = context.source_to_ssa().get(var_name) {
                if let Some(t) = context.lookup(ssa_name) {
                    t
                } else {
                    continue;
                }
            } else {
                continue;
            };

            let pred = if let Some(refinement) = template.refinement() {
                Some(refinement.clone())
            } else if let Some(liquid_var) = template.liquid_var() {
                self.assignment.get(liquid_var).cloned()
            } else {
                None
            };

            if let Some(pred) = pred {
                // Substitute binder with the variable name used in the predicate
                let binder = template.binder();
                if binder != var_name {
                    let mut subst = std::collections::HashMap::new();
                    subst.insert(binder.to_string(), var_name.clone());
                    all_assumptions.push(pred.substitute_vars(&subst));
                } else {
                    all_assumptions.push(pred);
                }
            }
        }

        self.smt_solver
            .is_valid_implication(&all_assumptions, antecedent, consequent)
            .map_err(|e| SolverError::InternalError {
                message: format!("SMT error: {}", e),
            })
    }

    /// Strengthen assignment to satisfy a constraint
    fn strengthen_for_constraint(&mut self, constraint: &Constraint, all_constraints: &[Constraint]) -> SolverResult<bool> {
        match constraint {
            Constraint::Subtype {
                context, sub, sup, ..
            } => {
                // If rhs has a liquid variable, try to strengthen it
                if let Some(kappa) = sup.liquid_var() {
                    return self.strengthen_liquid_var_multi_path(kappa, context, sub, all_constraints);
                }
                // No liquid variable - can't strengthen during this phase
                // Will be checked at final verification
                Ok(false)
            }
            // Hard constraints: if they fail, the program is ill-typed
            Constraint::LoopInvariantEntry { invariant, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Loop invariant does not hold at entry\n\
                        Invariant: {}\n\
                        Location: {:?}",
                        invariant, location
                    )
                })
            }
            Constraint::LoopInvariantPreservation { invariant, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Loop invariant is not preserved by loop body\n\
                        Invariant: {}\n\
                        Location: {:?}",
                        invariant, location
                    )
                })
            }
            Constraint::LoopVariantDecreases { variant_before, variant_after, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Loop variant does not decrease\n\
                        Before: {}\n\
                        After: {}\n\
                        Location: {:?}",
                        variant_before, variant_after, location
                    )
                })
            }
            Constraint::LoopVariantNonNegative { variant, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Loop variant may be negative\n\
                        Variant: {}\n\
                        Location: {:?}",
                        variant, location
                    )
                })
            }
            Constraint::Requires { condition, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Requires clause not satisfied at call site\n\
                        Condition: {}\n\
                        Location: {:?}",
                        condition, location
                    )
                })
            }
            Constraint::Ensures { condition, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Ensures clause not provable\n\
                        Condition: {}\n\
                        Location: {:?}",
                        condition, location
                    )
                })
            }
            Constraint::Fold { var, refinement, location, .. } => {
                Err(SolverError::TypeMismatch {
                    message: format!(
                        "Fold: storage invariant not satisfied\n\
                        Variable: {}\n\
                        Refinement: {}\n\
                        Location: {:?}",
                        var, refinement, location
                    )
                })
            }
            _ => Ok(false), // WellFormed, BinaryOp, UnaryOp — strengthened via liquid vars
        }
    }

    /// Strengthen a liquid variable by adding qualifiers
    fn strengthen_liquid_var_multi_path(
        &mut self,
        var: LiquidVar,
        context: &TypeContext,
        lhs: &Template,
        all_constraints: &[Constraint],
    ) -> SolverResult<bool> {
        let all_constraints_for_var: Vec<_> = all_constraints
            .iter()
            .filter_map(|c| {
                if let Constraint::Subtype { context, sub, sup, .. } = c {
                    if sup.liquid_var() == Some(var) {
                        Some((context.clone(), sub.clone(), sup.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        println!("[MULTI_PATH] Strengthening κ{} with {} constraints", var.0, all_constraints_for_var.len());

        // Extract relevant variables from the LHS template (for relevance pruning)
        // Only include variables that actually appear in the LHS refinement
        let mut relevant_vars = HashSet::new();
        if let Some(lhs_refinement) = lhs.refinement() {
            self.collect_vars_from_predicate(lhs_refinement, &mut relevant_vars);
        }
        // If no specific variables in LHS, use empty set (all candidates equally relevant)

        // Get candidates with relevance prioritization
        let candidates = self.qualifiers.instantiate_all_with_relevance(context, &relevant_vars);
        println!("[MULTI_PATH] Testing {} candidates (relevant vars: {:?})",
                 candidates.len(),
                 if relevant_vars.is_empty() {
                     "none (all equally relevant)".to_string()
                 } else {
                     format!("{:?}", relevant_vars)
                 });

        let mut current = self
            .assignment
            .get(var)
            .cloned()
            .unwrap_or(Predicate::True(NodeId::new(0), SourceRef::unknown()));

        // Greedy strengthening: agregar qualifiers mientras funcionen
        // Use adaptive limits: more simple qualifiers, fewer complex ones
        let mut changed = false;
        let mut simple_count = 0; // arity 0-1
        let mut complex_count = 0; // arity 2+

        // Adaptive limits based on qualifier complexity
        const MAX_SIMPLE_QUALIFIERS: usize = 5;  // Allow more simple qualifiers
        const MAX_COMPLEX_QUALIFIERS: usize = 2; // Restrict complex ones

        for candidate in candidates {
            // Determine qualifier complexity by checking if it mentions multiple variables
            let complexity = Self::estimate_predicate_complexity(&candidate);
            let is_simple = complexity <= 1;

            // Check if we've hit the limit for this complexity class
            if is_simple && simple_count >= MAX_SIMPLE_QUALIFIERS {
                continue;
            }
            if !is_simple && complex_count >= MAX_COMPLEX_QUALIFIERS {
                continue;
            }

            let strengthened = self.conjoin(&current, &candidate);

            let mut works_for_all_paths = true;
            for (path_context, path_lhs, _path_rhs) in &all_constraints_for_var {
                let lhs_pred = self.get_predicate(path_lhs);
                if !self.check_implication(path_context, &lhs_pred, &strengthened)? {
                    works_for_all_paths = false;
                    break;
                }
            }

            if works_for_all_paths {
                println!("[MULTI_PATH] Adding {} qualifier: {}",
                         if is_simple { "simple" } else { "complex" }, candidate);
                current = strengthened;
                changed = true;
                if is_simple {
                    simple_count += 1;
                } else {
                    complex_count += 1;
                }
            }
        }

        if changed {
            println!("[MULTI_PATH] Final: κ{} = {}", var.0, current);
            self.assignment.assign(var, current);
            return Ok(true);
        }

        // Si no se encontró qualifier, propagar refinement concreto del LHS
        // SIN hacer substitución de binders - confiar en que el contexto tenga las equalities
        if all_constraints_for_var.len() == 1 {
            let (_, lhs_template, _rhs_template) = &all_constraints_for_var[0];
            let lhs_pred = self.get_predicate(lhs_template);

            if !matches!(lhs_pred, Predicate::True(..)) {
                println!("[MULTI_PATH] Propagating LHS refinement AS-IS: {}", lhs_pred);
                self.assignment.assign(var, lhs_pred);
                return Ok(true);
            }
        }

        println!("[MULTI_PATH] No qualifier worked for κ{}", var.0);
        Ok(false)
    }

    // fn strengthen_liquid_var(
    //     &mut self,
    //     var: LiquidVar,
    //     context: &TypeContext,
    //     lhs: &Template,
    // ) -> SolverResult<bool> {
    //     // Extract relevant variables for relevance pruning
    //     let mut relevant_vars = HashSet::new();
    //     if let Some(lhs_refinement) = lhs.refinement() {
    //         self.collect_vars_from_predicate(lhs_refinement, &mut relevant_vars);
    //     }

    //     // Generate candidate qualifiers with relevance
    //     let candidates = self.qualifiers.instantiate_all_with_relevance(context, &relevant_vars);

    //     // Current predicate
    //     let current = self
    //         .assignment
    //         .get(var)
    //         .cloned()
    //         .unwrap_or(Predicate::True(NodeId::new(0), SourceRef::unknown()));

    //     // Try adding each candidate
    //     for candidate in candidates {
    //         let strengthened = self.conjoin(&current, &candidate);
    //         println!("Strengthened: {strengthened}");

    //         // Check if this helps satisfy the constraint
    //         let lhs_pred = self.get_predicate(lhs);
    //         if self.check_implication(context, &lhs_pred, &strengthened)? {
    //             self.assignment.assign(var, strengthened);
    //             return Ok(true);
    //         }
    //     }

    //     Ok(false)
    // }

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
                if name != "__self" {
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

    /// Estimate the complexity of a predicate based on unique variables mentioned
    /// (excluding special binders like "ν" and "__self")
    ///
    /// Returns:
    /// - 0: constant predicates (no variables)
    /// - 1: predicates with one variable
    /// - 2+: predicates with multiple variables (more complex)
    fn estimate_predicate_complexity(pred: &Predicate) -> usize {
        let mut vars = HashSet::new();
        Self::collect_vars_for_complexity(pred, &mut vars);
        vars.len()
    }

    fn collect_vars_for_complexity(pred: &Predicate, vars: &mut HashSet<String>) {
        match pred {
            Predicate::Var(name, ..) => {
                if name != "ν" && name != "__self" {
                    vars.insert(name.clone());
                }
            }
            Predicate::BinRel { lhs, rhs, .. } => {
                Self::collect_expr_vars_for_complexity(lhs, vars);
                Self::collect_expr_vars_for_complexity(rhs, vars);
            }
            Predicate::And(p1, p2, ..) | Predicate::Or(p1, p2, ..) | Predicate::Implies(p1, p2, ..) => {
                Self::collect_vars_for_complexity(p1, vars);
                Self::collect_vars_for_complexity(p2, vars);
            }
            Predicate::Not(p, ..) => {
                Self::collect_vars_for_complexity(p, vars);
            }
            _ => {}
        }
    }

    fn collect_expr_vars_for_complexity(expr: &RefinementExpr, vars: &mut HashSet<String>) {
        match expr {
            RefinementExpr::Var(name, ..) => {
                if name != "ν" && name != "__self" {
                    vars.insert(name.clone());
                }
            }
            RefinementExpr::BinOp { lhs, rhs, .. } => {
                Self::collect_expr_vars_for_complexity(lhs, vars);
                Self::collect_expr_vars_for_complexity(rhs, vars);
            }
            RefinementExpr::UnaryOp { expr, .. } => {
                Self::collect_expr_vars_for_complexity(expr, vars);
            }
            RefinementExpr::UninterpFn { args, .. } => {
                for arg in args {
                    Self::collect_expr_vars_for_complexity(arg, vars);
                }
            }
            RefinementExpr::Old { expr, .. } => {
                Self::collect_expr_vars_for_complexity(expr, vars);
            }
            _ => {}
        }
    }
}
