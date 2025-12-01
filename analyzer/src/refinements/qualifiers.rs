use merak_ast::{
    meta::SourceRef,
    predicate::{ArithOp, Predicate, RefinementExpr, RelOp},
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
                let nu_expr =
                    RefinementExpr::Var("v".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());

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
                let nu_expr =
                    RefinementExpr::Var("v".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());

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
                    Predicate::Var("v".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());
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

                let nu_expr =
                    RefinementExpr::Var("v".to_string(), SYNTHETIC_NODE_ID, source_ref.clone());

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

        // Constants: 0 op v
        for op in [RelOp::Leq, RelOp::Geq, RelOp::Lt, RelOp::Gt] {
            qualifiers.push(Qualifier::constant_comparison(0, false, op));
            qualifiers.push(Qualifier::constant_comparison(0, true, op));
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
    pub fn instantiate_all(&self, context: &TypeContext) -> HashSet<Predicate> {
        let mut instantiations = Vec::new();
        let var_names: Vec<String> = context
            .bindings()
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        for qualifier in &self.qualifiers {
            match qualifier.arity {
                0 => {
                    // No arguments needed
                    if let Some(pred) = qualifier.instantiate(&[]) {
                        instantiations.push(pred);
                    }
                }
                1 => {
                    // One argument: try each variable
                    for var in &var_names {
                        if let Some(pred) = qualifier.instantiate(&[var.clone()]) {
                            instantiations.push(pred);
                        }
                    }
                }
                2 => {
                    // Two arguments: try all pairs
                    for v1 in &var_names {
                        for v2 in &var_names {
                            if let Some(pred) = qualifier.instantiate(&[v1.clone(), v2.clone()]) {
                                instantiations.push(pred);
                            }
                        }
                    }
                }
                _ => {
                    // Higher arity not supported yet
                }
            }
        }

        // Remove duplicates
        let unique: HashSet<_> = instantiations.into_iter().collect();
        unique
    }

    /// Number of qualifiers in this set
    pub fn len(&self) -> usize {
        self.qualifiers.len()
    }

    /// Is extended mode enabled?
    pub fn is_extended(&self) -> bool {
        self.extended
    }
}
