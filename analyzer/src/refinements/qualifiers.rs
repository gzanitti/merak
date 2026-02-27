use merak_ast::{
    meta::SourceRef,
    predicate::{ArithOp, Predicate, RefinementExpr, RelOp},
    types::BaseType,
    NodeId,
};
use std::collections::HashSet;

use crate::refinements::constraints::TypeContext;

/// Synthetic NodeId for compiler-generated predicates
const SYNTHETIC_NODE_ID: NodeId = NodeId::new(0);

/// A qualifier is a predicate template with placeholders (?)
///
/// Example: "? <= v" can be instantiated with any variable in scope
/// to produce "x <= v", "y <= v", etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Qualifier {
    /// The predicate pattern with placeholders
    pattern: QualifierPattern,

    /// Number of placeholder variables needed
    arity: usize,
}

/// Qualifier patterns with placeholders
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum QualifierPattern {
    /// Comparison: "? op v"  or  "v op ?"
    Comparison {
        lhs_is_nu: bool, // true if V is on the left
        op: RelOp,
    },

    /// Constant comparison: "c op v"  or  "v op c"
    ConstantComparison {
        constant: i64,
        lhs_is_nu: bool,
        op: RelOp,
    },

    /// Binary arithmetic: "v = ? op ?"  or  "v op ? op ?"
    BinaryArithmetic {
        op: ArithOp,
        result_op: Option<RelOp>, // None for "v = ? + ?", Some(Geq) for "v >= ? + ?"
    },

    /// Boolean: "v"  or  "!v"
    Boolean { negated: bool },

    /// Uninterpreted function: "v = f(?)"  or  "v op f(?)"
    UninterpFunction {
        function_name: String,
        result_op: Option<RelOp>,
    },

    /// Blockchain global: "v = msg.sender", "v >= block.timestamp", etc.
    BlockchainGlobal { global: BlockchainGlobal, op: RelOp },
}

/// Blockchain-specific global variables
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BlockchainGlobal {
    MsgSender,
    MsgValue,
    BlockTimestamp,
}

impl Qualifier {
    /// Create a comparison qualifier: ? op v
    pub fn comparison(lhs_is_nu: bool, op: RelOp) -> Self {
        Qualifier {
            pattern: QualifierPattern::Comparison { lhs_is_nu, op },
            arity: 1,
        }
    }

    /// Create a constant comparison: c op v
    pub fn constant_comparison(constant: i64, lhs_is_nu: bool, op: RelOp) -> Self {
        Qualifier {
            pattern: QualifierPattern::ConstantComparison {
                constant,
                lhs_is_nu,
                op,
            },
            arity: 0,
        }
    }

    /// Create a binary arithmetic qualifier: v = ? + ?
    pub fn binary_arithmetic(op: ArithOp, result_op: Option<RelOp>) -> Self {
        Qualifier {
            pattern: QualifierPattern::BinaryArithmetic { op, result_op },
            arity: 2,
        }
    }

    /// Create a boolean qualifier: "v" or "!v"
    pub fn boolean(negated: bool) -> Self {
        Qualifier {
            pattern: QualifierPattern::Boolean { negated },
            arity: 0,
        }
    }

    /// Create an uninterpreted function qualifier: v = len(?)
    pub fn uninterp_function(function_name: String, result_op: Option<RelOp>) -> Self {
        Qualifier {
            pattern: QualifierPattern::UninterpFunction {
                function_name,
                result_op,
            },
            arity: 1,
        }
    }

    /// Create a blockchain global qualifier: v = msg.sender
    pub fn blockchain_global(global: BlockchainGlobal, op: RelOp) -> Self {
        Qualifier {
            pattern: QualifierPattern::BlockchainGlobal { global, op },
            arity: 0,
        }
    }

    /// Instantiate this qualifier with concrete variables
    ///
    /// Example: "? <= v" with ["x"] becomes "x <= v"
    pub fn instantiate(&self, args: &[String]) -> Option<Predicate> {
        if args.len() != self.arity {
            return None;
        }

        let source_ref = SourceRef::unknown();

        Some(match &self.pattern {
            QualifierPattern::Comparison { lhs_is_nu, op } => {
                let var_expr =
                    RefinementExpr::Var(args[0].clone(), SYNTHETIC_NODE_ID, source_ref.clone());
                let nu_expr =
                    RefinementExpr::Var("ν".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());

                if *lhs_is_nu {
                    Predicate::BinRel {
                        op: *op,
                        lhs: nu_expr,
                        rhs: var_expr,
                        id: SYNTHETIC_NODE_ID,
                        source_ref,
                    }
                } else {
                    Predicate::BinRel {
                        op: *op,
                        lhs: var_expr,
                        rhs: nu_expr,
                        id: SYNTHETIC_NODE_ID,
                        source_ref,
                    }
                }
            }

            QualifierPattern::ConstantComparison {
                constant,
                lhs_is_nu,
                op,
            } => {
                let const_expr =
                    RefinementExpr::IntLit(*constant, SYNTHETIC_NODE_ID, source_ref.clone());
                let nu_expr = RefinementExpr::Var(
                    "__self".to_string(),
                    SYNTHETIC_NODE_ID,
                    source_ref.clone(),
                );

                if *lhs_is_nu {
                    Predicate::BinRel {
                        op: *op,
                        lhs: nu_expr,
                        rhs: const_expr,
                        id: SYNTHETIC_NODE_ID,
                        source_ref,
                    }
                } else {
                    Predicate::BinRel {
                        op: *op,
                        lhs: const_expr,
                        rhs: nu_expr,
                        id: SYNTHETIC_NODE_ID,
                        source_ref,
                    }
                }
            }

            QualifierPattern::BinaryArithmetic { op, result_op } => {
                let lhs_expr =
                    RefinementExpr::Var(args[0].clone(), SYNTHETIC_NODE_ID, source_ref.clone());
                let rhs_expr =
                    RefinementExpr::Var(args[1].clone(), SYNTHETIC_NODE_ID, source_ref.clone());
                let nu_expr = RefinementExpr::Var(
                    "__self".to_string(),
                    SYNTHETIC_NODE_ID,
                    source_ref.clone(),
                );

                let arith_expr = RefinementExpr::BinOp {
                    op: *op,
                    lhs: Box::new(lhs_expr),
                    rhs: Box::new(rhs_expr),
                    id: SYNTHETIC_NODE_ID,
                    source_ref: source_ref.clone(),
                };

                Predicate::BinRel {
                    op: result_op.unwrap_or(RelOp::Eq),
                    lhs: nu_expr,
                    rhs: arith_expr,
                    id: SYNTHETIC_NODE_ID,
                    source_ref,
                }
            }

            QualifierPattern::Boolean { negated } => {
                let nu_pred =
                    Predicate::Var("__self".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());
                if *negated {
                    Predicate::Not(Box::new(nu_pred), SYNTHETIC_NODE_ID, source_ref)
                } else {
                    nu_pred
                }
            }

            QualifierPattern::UninterpFunction {
                function_name,
                result_op,
            } => {
                // TODO: Always arity 1?
                let arg_expr =
                    RefinementExpr::Var(args[0].clone(), SYNTHETIC_NODE_ID, source_ref.clone());

                let func_expr = RefinementExpr::UninterpFn {
                    name: function_name.clone(),
                    args: vec![arg_expr],
                    id: SYNTHETIC_NODE_ID,
                    source_ref: source_ref.clone(),
                };

                let nu_expr = RefinementExpr::Var(
                    "__self".to_string(),
                    SYNTHETIC_NODE_ID,
                    source_ref.clone(),
                );

                Predicate::BinRel {
                    op: result_op.unwrap_or(RelOp::Eq),
                    lhs: nu_expr,
                    rhs: func_expr,
                    id: SYNTHETIC_NODE_ID,
                    source_ref,
                }
            }

            QualifierPattern::BlockchainGlobal { global, op } => {
                let global_expr = match global {
                    BlockchainGlobal::MsgSender => {
                        RefinementExpr::MsgSender(SYNTHETIC_NODE_ID, source_ref.clone())
                    }
                    BlockchainGlobal::MsgValue => {
                        RefinementExpr::MsgValue(SYNTHETIC_NODE_ID, source_ref.clone())
                    }
                    BlockchainGlobal::BlockTimestamp => {
                        RefinementExpr::BlockTimestamp(SYNTHETIC_NODE_ID, source_ref.clone())
                    }
                };

                let nu_expr =
                    RefinementExpr::Var("ν".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());

                Predicate::BinRel {
                    op: *op,
                    lhs: nu_expr,
                    rhs: global_expr,
                    id: SYNTHETIC_NODE_ID,
                    source_ref,
                }
            }
        })
    }
}

/// Set of qualifiers available for refinement inference
pub struct QualifierSet {
    qualifiers: Vec<Qualifier>,
    extended: bool, // Include blockchain-specific qualifiers
}

impl QualifierSet {
    /// Create a qualifier set with core qualifiers only
    pub fn core() -> Self {
        Self::with_constants(vec![])
    }

    /// Create a qualifier set with additional constants from program
    pub fn with_constants(program_constants: Vec<i64>) -> Self {
        let mut qualifiers = Vec::new();

        // Comparisons: ? op v
        for op in [
            RelOp::Lt,
            RelOp::Leq,
            RelOp::Gt,
            RelOp::Geq,
            RelOp::Eq,
            RelOp::Neq,
        ] {
            qualifiers.push(Qualifier::comparison(false, op));
            qualifiers.push(Qualifier::comparison(true, op));
        }

        // Constants: Base set + program constants
        let mut all_constants: HashSet<i64> = HashSet::new();

        // Base constants (common values)
        for c in [0, 1] {
            all_constants.insert(c);
        }

        // Add program constants
        for c in program_constants {
            all_constants.insert(c);
        }

        // Generate qualifier patterns for each constant
        for constant in all_constants {
            for op in [RelOp::Leq, RelOp::Geq, RelOp::Lt, RelOp::Gt, RelOp::Eq, RelOp::Neq] {
                qualifiers.push(Qualifier::constant_comparison(constant, false, op));
                qualifiers.push(Qualifier::constant_comparison(constant, true, op));
            }
        }

        // Linear arithmetic: v = ? + ?, v >= ? + ?, etc.
        for op in [ArithOp::Add, ArithOp::Sub] {
            qualifiers.push(Qualifier::binary_arithmetic(op, None)); // v = ? + ?
            qualifiers.push(Qualifier::binary_arithmetic(op, Some(RelOp::Geq))); // v >= ? + ?
            qualifiers.push(Qualifier::binary_arithmetic(op, Some(RelOp::Leq)));
            // v <= ? + ?
        }

        // Boolean: v, !v
        qualifiers.push(Qualifier::boolean(false));
        qualifiers.push(Qualifier::boolean(true));

        Self {
            qualifiers,
            extended: false,
        }
    }

    /// Create a qualifier set with blockchain-specific qualifiers
    pub fn extended() -> Self {
        let mut set = Self::core();
        set.extended = true;

        // Blockchain globals
        set.qualifiers.push(Qualifier::blockchain_global(
            BlockchainGlobal::MsgSender,
            RelOp::Eq,
        ));
        set.qualifiers.push(Qualifier::blockchain_global(
            BlockchainGlobal::BlockTimestamp,
            RelOp::Geq,
        ));
        set.qualifiers.push(Qualifier::blockchain_global(
            BlockchainGlobal::MsgValue,
            RelOp::Geq,
        ));

        // Uninterpreted functions
        set.qualifiers
            .push(Qualifier::uninterp_function("len".to_string(), None));
        set.qualifiers.push(Qualifier::uninterp_function(
            "len".to_string(),
            Some(RelOp::Lt),
        ));
        set.qualifiers
            .push(Qualifier::uninterp_function("balance_of".to_string(), None));

        set
    }

    /// Generate all instantiations of qualifiers with variables from context
    /// Optimizations:
    /// - Well-formedness pruning: only use variables in scope
    /// - Type-based pruning: filter qualifiers that don't make sense for the type
    /// - Ordering: simpler qualifiers first (by arity)
    pub fn instantiate_all(&self, context: &TypeContext) -> Vec<Predicate> {
        self.instantiate_all_with_relevance(context, &HashSet::new())
    }

    /// Generate instantiations with relevance information
    /// Variables in `relevant_vars` will be prioritized in the ordering
    pub fn instantiate_all_with_relevance(
        &self,
        context: &TypeContext,
        relevant_vars: &HashSet<String>,
    ) -> Vec<Predicate> {
        //let mut instantiations: Vec<_> = Vec::new();

        // Well-formedness: only variables that are actually in scope
        // Also collect type information for type-based pruning
        let var_info: Vec<(String, BaseType)> = context
            .bindings()
            .iter()
            .map(|(name, template)| (name.clone(), template.base_type().clone()))
            .collect();

        // Group qualifiers by arity for ordered processing
        let mut by_arity: Vec<Vec<&Qualifier>> = vec![Vec::new(); 3];
        for qualifier in &self.qualifiers {
            if qualifier.arity < 3 {
                by_arity[qualifier.arity].push(qualifier);
            }
        }

        // Separate into relevant and non-relevant candidates
        // Relevant candidates mention variables from relevant_vars
        let mut relevant_instantiations = Vec::new();
        let mut other_instantiations = Vec::new();

        // Process in order: arity 0, then 1, then 2
        // This prioritizes simpler qualifiers
        for arity in 0..3 {
            for qualifier in &by_arity[arity] {
                match qualifier.arity {
                    0 => {
                        // No arguments needed - always relevant (constants)
                        if let Some(pred) = qualifier.instantiate(&[]) {
                            relevant_instantiations.push(pred);
                        }
                    }
                    1 => {
                        // One argument: separate relevant from non-relevant
                        for (var, var_type) in &var_info {
                            // Type-based pruning: skip if qualifier doesn't make sense for this type
                            if !Self::is_qualifier_compatible_with_type(*qualifier, var_type) {
                                continue;
                            }
                            if let Some(pred) = qualifier.instantiate(&[var.clone()]) {
                                // Relevance: prioritize qualifiers mentioning relevant variables
                                if relevant_vars.contains(var) {
                                    relevant_instantiations.push(pred);
                                } else {
                                    other_instantiations.push(pred);
                                }
                            }
                        }
                    }
                    2 => {
                        // Two arguments: relevant if ANY variable is relevant
                        for (v1, t1) in &var_info {
                            for (v2, t2) in &var_info {
                                // Type-based pruning: both variables should be compatible
                                if !Self::is_binary_qualifier_compatible(
                                    *qualifier, t1, t2
                                ) {
                                    continue;
                                }
                                if let Some(pred) = qualifier.instantiate(&[v1.clone(), v2.clone()]) {
                                    // Relevance: relevant if either variable is relevant
                                    if relevant_vars.contains(v1) || relevant_vars.contains(v2) {
                                        relevant_instantiations.push(pred);
                                    } else {
                                        other_instantiations.push(pred);
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // Higher arity not supported yet
                    }
                }
            }
        }

        // Return relevant candidates first, then others
        relevant_instantiations.extend(other_instantiations);
        relevant_instantiations
    }

    /// Generate instantiations but return as HashSet (for backwards compatibility)
    pub fn instantiate_all_unique(&self, context: &TypeContext) -> HashSet<Predicate> {
        self.instantiate_all(context).into_iter().collect()
    }

    /// Number of qualifiers in this set
    pub fn len(&self) -> usize {
        self.qualifiers.len()
    }

    /// Is extended mode enabled?
    pub fn is_extended(&self) -> bool {
        self.extended
    }

    /// Check if a qualifier is compatible with a variable type (arity 1)
    ///
    /// Type-based pruning rules:
    /// - Address: only equality/inequality, no numeric comparisons
    /// - Bool: only boolean operations, no arithmetic
    /// - Int: all operations allowed
    /// - String: only equality/inequality
    fn is_qualifier_compatible_with_type(qualifier: &Qualifier, var_type: &BaseType) -> bool {
        match &qualifier.pattern {
            QualifierPattern::Comparison { op, .. } => {
                match var_type {
                    BaseType::Address => {
                        // Address: only equality checks make sense
                        matches!(op, RelOp::Eq | RelOp::Neq)
                    }
                    BaseType::Bool => {
                        // Bool: only equality checks
                        matches!(op, RelOp::Eq | RelOp::Neq)
                    }
                    BaseType::String => {
                        // String: only equality checks
                        matches!(op, RelOp::Eq | RelOp::Neq)
                    }
                    BaseType::Int => true, // All comparisons valid for Int
                    _ => false, // Tuples, Functions, Contracts: no comparisons
                }
            }
            QualifierPattern::ConstantComparison { .. } => {
                match var_type {
                    BaseType::Address => {
                        // Address comparisons with numeric constants don't make sense
                        false
                    }
                    BaseType::Bool => {
                        // Bool with numeric constants doesn't make sense
                        false
                    }
                    BaseType::String => false,
                    BaseType::Int => true, // All numeric comparisons valid
                    _ => false,
                }
            }
            QualifierPattern::BinaryArithmetic { .. } => {
                // Arithmetic only makes sense for Int
                matches!(var_type, BaseType::Int)
            }
            QualifierPattern::Boolean { .. } => {
                // Boolean operations only for Bool
                matches!(var_type, BaseType::Bool)
            }
            QualifierPattern::UninterpFunction { .. } => {
                // Uninterpreted functions: allow for all types (domain-specific)
                true
            }
            QualifierPattern::BlockchainGlobal { global, op } => {
                match global {
                    BlockchainGlobal::MsgSender => {
                        // msg.sender is Address
                        matches!(var_type, BaseType::Address) && matches!(op, RelOp::Eq | RelOp::Neq)
                    }
                    BlockchainGlobal::MsgValue | BlockchainGlobal::BlockTimestamp => {
                        // msg.value and block.timestamp are Int
                        matches!(var_type, BaseType::Int)
                    }
                }
            }
        }
    }

    /// Check if a binary qualifier is compatible with two variable types (arity 2)
    fn is_binary_qualifier_compatible(
        qualifier: &Qualifier,
        type1: &BaseType,
        type2: &BaseType,
    ) -> bool {
        match &qualifier.pattern {
            QualifierPattern::BinaryArithmetic { .. } => {
                // Arithmetic requires both to be Int
                matches!(type1, BaseType::Int) && matches!(type2, BaseType::Int)
            }
            _ => {
                // Other patterns don't apply to arity 2
                true
            }
        }
    }
}
