use merak_ast::predicate::{ArithOp, Predicate, RefinementExpr, RelOp};
use std::collections::HashMap;
use z3::ast::{Ast, Bool, Int};
use z3::{Context, SatResult, Solver};

/// SMT solver interface for checking validity of implications
///
/// This module provides an abstraction over Z3 for checking whether
/// logical formulas (predicates) are valid.
pub struct SmtSolver<'ctx> {
    /// Z3 context (must be kept alive for the lifetime of the solver)
    context: &'ctx Context,

    /// Z3 solver instance
    solver: Solver<'ctx>,

    /// Timeout for SMT queries (in milliseconds)
    timeout_ms: u32,

    /// Cache of previous queries for performance
    query_cache: HashMap<String, SmtResult>,
}

/// Result of an SMT query
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtResult {
    /// Formula is valid (always true)
    Valid,

    /// Formula is satisfiable (sometimes true)
    Satisfiable,

    /// Formula is unsatisfiable (never true)
    Unsatisfiable,

    /// Solver timed out
    Timeout,

    /// Unknown result
    Unknown,
}

/// Error type for SMT operations
#[derive(Debug, Clone)]
pub enum SmtError {
    /// Solver process failed
    SolverFailed(String),

    /// Timeout exceeded
    Timeout,

    /// Invalid formula
    InvalidFormula(String),

    /// Type conversion error
    TypeError(String),
}

impl std::fmt::Display for SmtError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SmtError::SolverFailed(msg) => write!(f, "SMT solver failed: {}", msg),
            SmtError::Timeout => write!(f, "SMT solver timeout"),
            SmtError::InvalidFormula(msg) => write!(f, "Invalid SMT formula: {}", msg),
            SmtError::TypeError(msg) => write!(f, "Type error in SMT conversion: {}", msg),
        }
    }
}

impl std::error::Error for SmtError {}

impl<'ctx> SmtSolver<'ctx> {
    /// Create a new SMT solver with default timeout (5 seconds)
    pub fn new(context: &'ctx Context) -> Self {
        let solver = Solver::new(context);
        Self {
            context,
            solver,
            timeout_ms: 5000,
            query_cache: HashMap::new(),
        }
    }

    /// Create a new SMT solver with custom timeout
    /// Timeout is in milliseconds.
    pub fn with_timeout(context: &'ctx Context, timeout_ms: u32) -> Self {
        let solver = Solver::new(context);
        let mut params = z3::Params::new(context);
        params.set_u32("timeout", timeout_ms);
        solver.set_params(&params);

        Self {
            context,
            solver,
            timeout_ms,
            query_cache: HashMap::new(),
        }
    }

    /// Check if an implication is valid: assumptions ∧ antecedent ⇒ consequent
    ///
    /// Returns true if the implication holds for all interpretations.
    /// We check this by verifying that (assumptions ∧ antecedent ∧ ¬consequent) is UNSAT.
    pub fn is_valid_implication(
        &mut self,
        assumptions: &[Predicate],
        antecedent: &Predicate,
        consequent: &Predicate,
    ) -> Result<bool, SmtError> {
        // Check cache
        let cache_key = format!("{:?} ∧ {:?} ⇒ {:?}", assumptions, antecedent, consequent);
        if let Some(cached) = self.query_cache.get(&cache_key) {
            return Ok(*cached == SmtResult::Unsatisfiable);
        }

        // Variable context for conversion
        let mut var_context = HashMap::new();

        // Reset solver
        self.solver.reset();

        // Add assumptions
        for assumption in assumptions {
            let z3_assumption = self.predicate_to_z3(assumption, &mut var_context)?;
            self.solver.assert(&z3_assumption);
        }

        // Add antecedent
        let z3_antecedent = self.predicate_to_z3(antecedent, &mut var_context)?;
        self.solver.assert(&z3_antecedent);

        // Add negation of consequent
        let z3_consequent = self.predicate_to_z3(consequent, &mut var_context)?;
        self.solver.assert(&z3_consequent.not());

        // Check satisfiability
        let result = match self.solver.check() {
            SatResult::Unsat => SmtResult::Unsatisfiable, // Valid implication!
            SatResult::Sat => SmtResult::Satisfiable,     // Counterexample exists
            SatResult::Unknown => SmtResult::Unknown,
        };

        // Cache result
        self.query_cache.insert(cache_key, result);

        Ok(result == SmtResult::Unsatisfiable)
    }

    /// Check if a predicate is satisfiable
    pub fn is_satisfiable(&mut self, predicate: &Predicate) -> Result<bool, SmtError> {
        let mut var_context = HashMap::new();

        self.solver.reset();
        let z3_pred = self.predicate_to_z3(predicate, &mut var_context)?;
        self.solver.assert(&z3_pred);

        let result = self.solver.check();
        Ok(matches!(result, SatResult::Sat))
    }

    /// Convert a Merak predicate to a Z3 boolean expression
    fn predicate_to_z3(
        &self,
        pred: &Predicate,
        var_context: &mut HashMap<String, Z3Var<'ctx>>,
    ) -> Result<Bool<'ctx>, SmtError> {
        match pred {
            Predicate::True(..) => Ok(Bool::from_bool(self.context, true)),

            Predicate::False(..) => Ok(Bool::from_bool(self.context, false)),

            Predicate::Var(name, ..) => {
                // Boolean variable
                let var = self.get_or_create_bool_var(name, var_context);
                Ok(var)
            }

            Predicate::BinRel { op, lhs, rhs, .. } => {
                let lhs_z3 = self.expr_to_z3(lhs, var_context)?;
                let rhs_z3 = self.expr_to_z3(rhs, var_context)?;

                let result = match op {
                    RelOp::Eq => lhs_z3._eq(&rhs_z3),
                    RelOp::Neq => lhs_z3._eq(&rhs_z3).not(),
                    RelOp::Lt => lhs_z3.lt(&rhs_z3),
                    RelOp::Leq => lhs_z3.le(&rhs_z3),
                    RelOp::Gt => lhs_z3.gt(&rhs_z3),
                    RelOp::Geq => lhs_z3.ge(&rhs_z3),
                };

                Ok(result)
            }

            Predicate::And(p1, p2, ..) => {
                let z3_p1 = self.predicate_to_z3(p1, var_context)?;
                let z3_p2 = self.predicate_to_z3(p2, var_context)?;
                Ok(Bool::and(self.context, &[&z3_p1, &z3_p2]))
            }

            Predicate::Or(p1, p2, ..) => {
                let z3_p1 = self.predicate_to_z3(p1, var_context)?;
                let z3_p2 = self.predicate_to_z3(p2, var_context)?;
                Ok(Bool::or(self.context, &[&z3_p1, &z3_p2]))
            }

            Predicate::Not(p, ..) => {
                let z3_p = self.predicate_to_z3(p, var_context)?;
                Ok(z3_p.not())
            }

            Predicate::Implies(p1, p2, ..) => {
                let z3_p1 = self.predicate_to_z3(p1, var_context)?;
                let z3_p2 = self.predicate_to_z3(p2, var_context)?;
                Ok(z3_p1.implies(&z3_p2))
            }

            Predicate::UninterpFnCall { name, args, .. } => {
                // Create uninterpreted function
                // For now, treat as a boolean function over integers
                let mut z3_args = Vec::new();
                for arg in args {
                    z3_args.push(self.expr_to_z3(arg, var_context)?);
                }

                // Create function declaration dynamically
                // This is simplified - in practice this should be cached
                let func_name = format!("uninterp_{}", name);
                let var = self.get_or_create_bool_var(&func_name, var_context);
                Ok(var)
            }
        }
    }

    /// Convert a Merak expression to a Z3 integer expression
    fn expr_to_z3(
        &self,
        expr: &RefinementExpr,
        var_context: &mut HashMap<String, Z3Var<'ctx>>,
    ) -> Result<Int<'ctx>, SmtError> {
        match expr {
            RefinementExpr::Var(name, ..) => Ok(self.get_or_create_int_var(name, var_context)),
            RefinementExpr::IntLit(n, ..) => Ok(Int::from_i64(self.context, *n)),
            RefinementExpr::AddressLit(addr, ..) => {
                // Convert address to integer
                // TODO: addresses should be bit-vectors
                let hex_value = i64::from_str_radix(&addr[2..], 16).unwrap_or(0);
                Ok(Int::from_i64(self.context, hex_value))
            }
            RefinementExpr::BinOp { op, lhs, rhs, .. } => {
                let lhs_z3 = self.expr_to_z3(lhs, var_context)?;
                let rhs_z3 = self.expr_to_z3(rhs, var_context)?;

                let result = match op {
                    ArithOp::Add => lhs_z3 + rhs_z3,
                    ArithOp::Sub => lhs_z3 - rhs_z3,
                    ArithOp::Mul => lhs_z3 * rhs_z3,
                    ArithOp::Div => lhs_z3 / rhs_z3,
                    ArithOp::Mod => lhs_z3 % rhs_z3,
                };

                Ok(result)
            }
            RefinementExpr::UnaryOp { expr, .. } => {
                let z3_expr = self.expr_to_z3(expr, var_context)?;
                Ok(-z3_expr) // Unary minus via Neg trait
            }
            RefinementExpr::UninterpFn { name, args, .. } => {
                // Uninterpreted function returning integer
                // For simplicity, create a fresh variable
                let func_name = format!("{}_{}", name, args.len());
                Ok(self.get_or_create_int_var(&func_name, var_context))
            }
            RefinementExpr::MsgSender(..) => {
                Ok(self.get_or_create_int_var("msg_sender", var_context))
            }
            RefinementExpr::MsgValue(..) => {
                Ok(self.get_or_create_int_var("msg_value", var_context))
            }
            RefinementExpr::BlockTimestamp(..) => {
                Ok(self.get_or_create_int_var("block_timestamp", var_context))
            }
            RefinementExpr::Old { expr, .. } => self.expr_to_z3_inside_old(expr, var_context),
        }
    }

    /// Convert an expression inside old() to Z3, renaming variables with @pre suffix
    fn expr_to_z3_inside_old(
        &self,
        expr: &RefinementExpr,
        var_context: &mut HashMap<String, Z3Var<'ctx>>,
    ) -> Result<Int<'ctx>, SmtError> {
        match expr {
            RefinementExpr::Var(name, ..) => {
                let old_name = format!("{}@pre", name);
                Ok(self.get_or_create_int_var(&old_name, var_context))
            }

            RefinementExpr::IntLit(n, ..) => Ok(Int::from_i64(self.context, *n)),

            RefinementExpr::AddressLit(addr, ..) => {
                let hex_value = i64::from_str_radix(&addr[2..], 16).unwrap_or(0);
                Ok(Int::from_i64(self.context, hex_value))
            }

            RefinementExpr::BinOp { op, lhs, rhs, .. } => {
                let lhs_z3 = self.expr_to_z3_inside_old(lhs, var_context)?;
                let rhs_z3 = self.expr_to_z3_inside_old(rhs, var_context)?;

                let result = match op {
                    ArithOp::Add => lhs_z3 + rhs_z3,
                    ArithOp::Sub => lhs_z3 - rhs_z3,
                    ArithOp::Mul => lhs_z3 * rhs_z3,
                    ArithOp::Div => lhs_z3 / rhs_z3,
                    ArithOp::Mod => lhs_z3 % rhs_z3,
                };

                Ok(result)
            }

            RefinementExpr::UnaryOp { expr, .. } => {
                let z3_expr = self.expr_to_z3_inside_old(expr, var_context)?;
                Ok(-z3_expr)
            }

            RefinementExpr::UninterpFn { name, args, .. } => {
                let old_func_name = format!("{}@pre_{}", name, args.len());
                Ok(self.get_or_create_int_var(&old_func_name, var_context))
            }

            // Intrinsics don't change during function execution
            RefinementExpr::MsgSender(..) => {
                Ok(self.get_or_create_int_var("msg_sender", var_context))
            }

            RefinementExpr::MsgValue(..) => {
                Ok(self.get_or_create_int_var("msg_value", var_context))
            }

            RefinementExpr::BlockTimestamp(..) => {
                Ok(self.get_or_create_int_var("block_timestamp", var_context))
            }

            RefinementExpr::Old { .. } => Err(SmtError::InvalidFormula(
                "Nested old() expressions are not allowed".to_string(),
            )),
        }
    }

    /// Get or create an integer variable
    fn get_or_create_int_var(
        &self,
        name: &str,
        var_context: &mut HashMap<String, Z3Var<'ctx>>,
    ) -> Int<'ctx> {
        if let Some(Z3Var::Int(var)) = var_context.get(name) {
            var.clone()
        } else {
            let var = Int::new_const(self.context, name);
            var_context.insert(name.to_string(), Z3Var::Int(var.clone()));
            var
        }
    }

    /// Get or create a boolean variable
    fn get_or_create_bool_var(
        &self,
        name: &str,
        var_context: &mut HashMap<String, Z3Var<'ctx>>,
    ) -> Bool<'ctx> {
        if let Some(Z3Var::Bool(var)) = var_context.get(name) {
            var.clone()
        } else {
            let var = Bool::new_const(self.context, name);
            var_context.insert(name.to_string(), Z3Var::Bool(var.clone()));
            var
        }
    }

    /// Clear the query cache
    pub fn clear_cache(&mut self) {
        self.query_cache.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize, usize) {
        let total = self.query_cache.len();
        let valid = self
            .query_cache
            .values()
            .filter(|r| **r == SmtResult::Unsatisfiable)
            .count();
        let invalid = total - valid;
        (total, valid, invalid)
    }
}

/// Internal representation of Z3 variables
enum Z3Var<'ctx> {
    Int(Int<'ctx>),
    Bool(Bool<'ctx>),
}
