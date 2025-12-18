use std::{collections::HashMap, fmt};

use crate::meta::SourceRef;
use crate::node_id::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Predicate {
    True(NodeId, SourceRef),
    False(NodeId, SourceRef),
    Var(String, NodeId, SourceRef),
    UninterpFnCall {
        name: String,
        args: Vec<RefinementExpr>,
        id: NodeId,
        source_ref: SourceRef,
    },
    BinRel {
        op: RelOp,
        lhs: RefinementExpr,
        rhs: RefinementExpr,
        id: NodeId,
        source_ref: SourceRef,
    },
    And(Box<Predicate>, Box<Predicate>, NodeId, SourceRef),
    Or(Box<Predicate>, Box<Predicate>, NodeId, SourceRef),
    Not(Box<Predicate>, NodeId, SourceRef),
    Implies(Box<Predicate>, Box<Predicate>, NodeId, SourceRef),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelOp {
    Eq,  // ==
    Neq, // !=
    Lt,  // >
    Leq, // <=
    Gt,  // >
    Geq, // >=
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RefinementExpr {
    Var(String, NodeId, SourceRef),
    IntLit(i64, NodeId, SourceRef),
    AddressLit(String, NodeId, SourceRef),

    MsgSender(NodeId, SourceRef),
    MsgValue(NodeId, SourceRef),
    BlockTimestamp(NodeId, SourceRef),

    BinOp {
        op: ArithOp,
        lhs: Box<RefinementExpr>,
        rhs: Box<RefinementExpr>,
        id: NodeId,
        source_ref: SourceRef,
    },

    UnaryOp {
        op: UnaryOp,
        expr: Box<RefinementExpr>,
        id: NodeId,
        source_ref: SourceRef,
    },

    UninterpFn {
        name: String,
        args: Vec<RefinementExpr>,
        id: NodeId,
        source_ref: SourceRef,
    },

    Old {
        expr: Box<RefinementExpr>,
        id: NodeId,
        source_ref: SourceRef,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArithOp {
    Add, // +
    Sub, // -
    Mul, // *
    Div, // /
    Mod, // %
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Negate, // -
}

impl Predicate {
    /// Get the NodeId of this predicate
    pub fn id(&self) -> NodeId {
        match self {
            Predicate::True(id, _) => *id,
            Predicate::False(id, _) => *id,
            Predicate::Var(_, id, _) => *id,
            Predicate::UninterpFnCall { id, .. } => *id,
            Predicate::BinRel { id, .. } => *id,
            Predicate::And(_, _, id, _) => *id,
            Predicate::Or(_, _, id, _) => *id,
            Predicate::Not(_, id, _) => *id,
            Predicate::Implies(_, _, id, _) => *id,
        }
    }

    /// Get the SourceRef of this predicate
    pub fn source_ref(&self) -> &SourceRef {
        match self {
            Predicate::True(_, sr) => sr,
            Predicate::False(_, sr) => sr,
            Predicate::Var(_, _, sr) => sr,
            Predicate::UninterpFnCall { source_ref, .. } => source_ref,
            Predicate::BinRel { source_ref, .. } => source_ref,
            Predicate::And(_, _, _, sr) => sr,
            Predicate::Or(_, _, _, sr) => sr,
            Predicate::Not(_, _, sr) => sr,
            Predicate::Implies(_, _, _, sr) => sr,
        }
    }

    pub fn contains_old(&self) -> bool {
        match self {
            Predicate::True(_, _) | Predicate::False(_, _) | Predicate::Var(_, _, _) => false,
            Predicate::BinRel { lhs, rhs, .. } => lhs.contains_old() || rhs.contains_old(),
            Predicate::And(left, right, _, _) => left.contains_old() || right.contains_old(),
            Predicate::Or(left, right, _, _) => left.contains_old() || right.contains_old(),
            Predicate::Not(pred, _, _) => pred.contains_old(),
            Predicate::UninterpFnCall {
                name,
                args,
                id,
                source_ref,
            } => args.iter().any(|arg| arg.contains_old()),
            Predicate::Implies(left, right, _, _) => left.contains_old() || right.contains_old(),
        }
    }

    pub fn negate(&self) -> Predicate {
        match self {
            Predicate::True(id, sr) => Predicate::False(*id, sr.clone()),
            Predicate::False(id, sr) => Predicate::True(*id, sr.clone()),

            // Boolean variable: simply negate with Not
            Predicate::Var(name, id, sr) => Predicate::Not(Box::new(self.clone()), *id, sr.clone()),

            // Uninterpreted function: negate with Not
            Predicate::UninterpFnCall {
                name,
                args,
                id,
                source_ref,
            } => Predicate::Not(Box::new(self.clone()), *id, source_ref.clone()),

            // Binary relation: negate the operator
            // ¬(x < y) = (x >= y)
            // ¬(x <= y) = (x > y)
            // ¬(x == y) = (x != y)
            Predicate::BinRel {
                op,
                lhs,
                rhs,
                id,
                source_ref,
            } => {
                let negated_op = match op {
                    RelOp::Eq => RelOp::Neq,
                    RelOp::Neq => RelOp::Eq,
                    RelOp::Lt => RelOp::Geq,
                    RelOp::Leq => RelOp::Gt,
                    RelOp::Gt => RelOp::Leq,
                    RelOp::Geq => RelOp::Lt,
                };

                Predicate::BinRel {
                    op: negated_op,
                    lhs: lhs.clone(),
                    rhs: rhs.clone(),
                    id: *id,
                    source_ref: source_ref.clone(),
                }
            }

            // De Morgan's law: ¬(P ∧ Q) = (¬P ∨ ¬Q)
            Predicate::And(p, q, id, sr) => {
                Predicate::Or(Box::new(p.negate()), Box::new(q.negate()), *id, sr.clone())
            }

            // De Morgan's law: ¬(P ∨ Q) = (¬P ∧ ¬Q)
            Predicate::Or(p, q, id, sr) => {
                Predicate::And(Box::new(p.negate()), Box::new(q.negate()), *id, sr.clone())
            }

            // Double negation: ¬(¬P) = P
            Predicate::Not(p, _id, _sr) => (**p).clone(),

            // Implication: ¬(P ⇒ Q) = P ∧ ¬Q
            // Because (P ⇒ Q) ≡ (¬P ∨ Q)
            // So ¬(P ⇒ Q) ≡ ¬(¬P ∨ Q) ≡ P ∧ ¬Q
            Predicate::Implies(p, q, id, sr) => {
                Predicate::And(p.clone(), Box::new(q.negate()), *id, sr.clone())
            }
        }
    }

    pub fn substitute_vars(&self, stacks: &HashMap<String, String>) -> Predicate {
        match self {
            Predicate::True(node_id, source_ref) => {
                Predicate::True(*node_id, source_ref.clone())
            }
            Predicate::False(node_id, source_ref) => {
                Predicate::False(*node_id, source_ref.clone())
            }
            Predicate::Var(name, node_id, source_ref) => {
                if let Some(new_name) = stacks.get(name) {
                    Predicate::Var(new_name.clone(), *node_id, source_ref.clone())
                } else {
                    // Keep the original variable if not in mapping
                    Predicate::Var(name.clone(), *node_id, source_ref.clone())
                }
            }
            Predicate::UninterpFnCall { name, args, id, source_ref } => Predicate::UninterpFnCall {
                name: name.clone(),
                args: args.iter().map(|arg| arg.substitute_vars(stacks)).collect(),
                id: *id,
                source_ref: source_ref.clone(),
            },
            Predicate::BinRel { op, lhs, rhs, id, source_ref } => Predicate::BinRel {
                op: *op,
                lhs: lhs.substitute_vars(stacks),
                rhs: rhs.substitute_vars(stacks),
                id: *id,
                source_ref: source_ref.clone(),
            },
            Predicate::And(left, right, node_id, source_ref) => Predicate::And(
                Box::new(left.substitute_vars(stacks)),
                Box::new(right.substitute_vars(stacks)),
                *node_id,
                source_ref.clone(),
            ),
            Predicate::Or(left, right, node_id, source_ref) => Predicate::Or(
                Box::new(left.substitute_vars(stacks)),
                Box::new(right.substitute_vars(stacks)),
                *node_id,
                source_ref.clone(),
            ),
            Predicate::Not(pred, node_id, source_ref) => Predicate::Not(
                Box::new(pred.substitute_vars(stacks)),
                *node_id,
                source_ref.clone(),
            ),
            Predicate::Implies(left, right, node_id, source_ref) => Predicate::Implies(
                Box::new(left.substitute_vars(stacks)),
                Box::new(right.substitute_vars(stacks)),
                *node_id,
                source_ref.clone(),
            ),
        }
    }
}

impl fmt::Display for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Predicate::True(_, _) => write!(f, "true"),
            Predicate::False(_, _) => write!(f, "false"),
            Predicate::Var(name, _, _) => write!(f, "{}", name),
            Predicate::UninterpFnCall { name, args, .. } => {
                write!(f, "{}(", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            Predicate::BinRel { op, lhs, rhs, .. } => write!(f, "{} {} {}", lhs, op, rhs),
            Predicate::And(left, right, _, _) => write!(f, "({} && {})", left, right),
            Predicate::Or(left, right, _, _) => write!(f, "({} || {})", left, right),
            Predicate::Not(pred, _, _) => write!(f, "!{}", pred),
            Predicate::Implies(left, right, _, _) => write!(f, "({} ==> {})", left, right),
        }
    }
}

impl fmt::Display for RelOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelOp::Eq => write!(f, "=="),
            RelOp::Neq => write!(f, "!="),
            RelOp::Lt => write!(f, "<"),
            RelOp::Leq => write!(f, "<="),
            RelOp::Gt => write!(f, ">"),
            RelOp::Geq => write!(f, ">="),
        }
    }
}

impl RefinementExpr {
    /// Get the NodeId of this refinement expression
    pub fn id(&self) -> NodeId {
        match self {
            RefinementExpr::Var(_, id, _) => *id,
            RefinementExpr::IntLit(_, id, _) => *id,
            RefinementExpr::AddressLit(_, id, _) => *id,
            RefinementExpr::MsgSender(id, _) => *id,
            RefinementExpr::MsgValue(id, _) => *id,
            RefinementExpr::BlockTimestamp(id, _) => *id,
            RefinementExpr::BinOp { id, .. } => *id,
            RefinementExpr::UnaryOp { id, .. } => *id,
            RefinementExpr::UninterpFn { id, .. } => *id,
            RefinementExpr::Old { id, .. } => *id,
        }
    }

    /// Get the SourceRef of this refinement expression
    pub fn source_ref(&self) -> &SourceRef {
        match self {
            RefinementExpr::Var(_, _, sr) => sr,
            RefinementExpr::IntLit(_, _, sr) => sr,
            RefinementExpr::AddressLit(_, _, sr) => sr,
            RefinementExpr::MsgSender(_, sr) => sr,
            RefinementExpr::MsgValue(_, sr) => sr,
            RefinementExpr::BlockTimestamp(_, sr) => sr,
            RefinementExpr::BinOp { source_ref, .. } => source_ref,
            RefinementExpr::UnaryOp { source_ref, .. } => source_ref,
            RefinementExpr::UninterpFn { source_ref, .. } => source_ref,
            RefinementExpr::Old { source_ref, .. } => source_ref,
        }
    }

    pub fn contains_old(&self) -> bool {
        match self {
            RefinementExpr::Var(_, _, _)
            | RefinementExpr::IntLit(_, _, _)
            | RefinementExpr::AddressLit(_, _, _)
            | RefinementExpr::MsgValue(_, _)
            | RefinementExpr::BlockTimestamp(_, _)
            | RefinementExpr::MsgSender(_, _) => false,
            RefinementExpr::BinOp { lhs, rhs, .. } => lhs.contains_old() || rhs.contains_old(),
            RefinementExpr::UnaryOp { expr, .. } => expr.contains_old(),
            RefinementExpr::UninterpFn { args, .. } => args.iter().any(|arg| arg.contains_old()),
            RefinementExpr::Old { expr, .. } => expr.contains_old(),
        }
    }

    pub fn substitute_vars(&self, stacks: &HashMap<String, String>) -> RefinementExpr {
        match self {
            RefinementExpr::Var(var, node_id, source_ref) => {
                if let Some(new_var) = stacks.get(var) {
                    RefinementExpr::Var(new_var.clone(), *node_id, source_ref.clone())
                } else {
                    // Keep the original variable if not in mapping
                    RefinementExpr::Var(var.clone(), *node_id, source_ref.clone())
                }
            }
            RefinementExpr::IntLit(value, node_id, source_ref) => {
                RefinementExpr::IntLit(*value, *node_id, source_ref.clone())
            }
            RefinementExpr::AddressLit(value, node_id, source_ref) => {
                RefinementExpr::AddressLit(value.clone(), *node_id, source_ref.clone())
            }
            RefinementExpr::MsgSender(node_id, source_ref) => {
                RefinementExpr::MsgSender(*node_id, source_ref.clone())
            }
            RefinementExpr::MsgValue(node_id, source_ref) => {
                RefinementExpr::MsgValue(*node_id, source_ref.clone())
            }
            RefinementExpr::BlockTimestamp(node_id, source_ref) => {
                RefinementExpr::BlockTimestamp(*node_id, source_ref.clone())
            }
            RefinementExpr::BinOp { op, lhs, rhs, id, source_ref } => RefinementExpr::BinOp {
                op: *op,
                lhs: Box::new(lhs.substitute_vars(stacks)),
                rhs: Box::new(rhs.substitute_vars(stacks)),
                id: *id,
                source_ref: source_ref.clone(),
            },
            RefinementExpr::UnaryOp { op, expr, id, source_ref } => RefinementExpr::UnaryOp {
                op: *op,
                expr: Box::new(expr.substitute_vars(stacks)),
                id: *id,
                source_ref: source_ref.clone(),
            },
            RefinementExpr::UninterpFn { name, args, id, source_ref } => RefinementExpr::UninterpFn {
                name: name.clone(),
                args: args.iter().map(|arg| arg.substitute_vars(stacks)).collect(),
                id: *id,
                source_ref: source_ref.clone(),
            },
            RefinementExpr::Old { expr, id, source_ref } => RefinementExpr::Old {
                expr: Box::new(expr.substitute_vars(stacks)),
                id: *id,
                source_ref: source_ref.clone(),
            },
        }
    }
}

impl fmt::Display for RefinementExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RefinementExpr::Var(name, _, _) => write!(f, "{}", name),
            RefinementExpr::IntLit(val, _, _) => write!(f, "{}", val),
            RefinementExpr::AddressLit(addr, _, _) => write!(f, "{}", addr),
            RefinementExpr::MsgSender(_, _) => write!(f, "msg.sender"),
            RefinementExpr::MsgValue(_, _) => write!(f, "msg.value"),
            RefinementExpr::BlockTimestamp(_, _) => write!(f, "block.timestamp"),
            RefinementExpr::BinOp { op, lhs, rhs, .. } => write!(f, "({} {} {})", lhs, op, rhs),
            RefinementExpr::UnaryOp { op, expr, .. } => write!(f, "{}{}", op, expr),
            RefinementExpr::UninterpFn { name, args, .. } => {
                write!(f, "{}(", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            RefinementExpr::Old { expr, .. } => {
                write!(f, " old({})", expr)
            }
        }
    }
}

impl fmt::Display for ArithOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArithOp::Add => write!(f, "+"),
            ArithOp::Sub => write!(f, "-"),
            ArithOp::Mul => write!(f, "*"),
            ArithOp::Div => write!(f, "/"),
            ArithOp::Mod => write!(f, "%"),
        }
    }
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnaryOp::Negate => write!(f, "-"),
        }
    }
}
