use std::{collections::HashMap, fmt};

use primitive_types::H256;

use crate::meta::SourceRef;
use crate::node_id::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Expression {
    Literal(Literal, NodeId, SourceRef),
    Identifier(String, NodeId, SourceRef),
    BinaryOp {
        left: Box<Expression>,
        op: BinaryOperator,
        right: Box<Expression>,
        id: NodeId,
        source_ref: SourceRef,
    },
    UnaryOp {
        op: UnaryOperator,
        expr: Box<Expression>,
        id: NodeId,
        source_ref: SourceRef,
    },
    Grouped(Box<Expression>, NodeId, SourceRef),
    FunctionCall {
        name: String,
        args: Vec<Expression>,
        id: NodeId,
        source_ref: SourceRef,
    },
    MemberCall {
        object: Box<Expression>,
        method: String,
        args: Vec<Expression>,
        id: NodeId,
        source_ref: SourceRef,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinaryOperator {
    LogicalOr,
    LogicalAnd,
    Equal,
    NotEqual,
    LessEqual,
    Less,
    GreaterEqual,
    Greater,
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UnaryOperator {
    Negate,
    Not,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Literal {
    Integer(i64),
    Address(H256),
    Boolean(bool),
    String(String),
}

impl Expression {
    /// Get the NodeId of this expression
    pub fn id(&self) -> NodeId {
        match self {
            Expression::Literal(_, id, _) => *id,
            Expression::Identifier(_, id, _) => *id,
            Expression::BinaryOp { id, .. } => *id,
            Expression::UnaryOp { id, .. } => *id,
            Expression::Grouped(_, id, _) => *id,
            Expression::FunctionCall { id, .. } => *id,
            Expression::MemberCall {  id, .. } => *id,
        }
    }

    /// Get the SourceRef of this expression
    pub fn source_ref(&self) -> &SourceRef {
        match self {
            Expression::Literal(_, _, sr) => sr,
            Expression::Identifier(_, _, sr) => sr,
            Expression::BinaryOp { source_ref, .. } => source_ref,
            Expression::UnaryOp { source_ref, .. } => source_ref,
            Expression::Grouped(_, _, sr) => sr,
            Expression::FunctionCall { source_ref, .. } => source_ref,
            Expression::MemberCall { source_ref, .. } => source_ref,
        }
    }

    pub fn substitute_vars(&self, stacks: &HashMap<String, String>) -> Expression {
        match self {
            Expression::Literal(lit, id, sr) => Expression::Literal(lit.clone(), *id, sr.clone()),

            Expression::Identifier(name, id, sr) => {
                if let Some(stack) = stacks.get(name) {
                    Expression::Identifier(stack.clone(), *id, sr.clone())
                } else {
                    panic!("Variable '{}' no encontrada en el stack", name);
                }
            }

            Expression::BinaryOp {
                left,
                op,
                right,
                id,
                source_ref,
            } => Expression::BinaryOp {
                left: Box::new(left.substitute_vars(stacks)),
                op: op.clone(),
                right: Box::new(right.substitute_vars(stacks)),
                id: *id,
                source_ref: source_ref.clone(),
            },

            Expression::UnaryOp {
                op,
                expr,
                id,
                source_ref,
            } => Expression::UnaryOp {
                op: op.clone(),
                expr: Box::new(expr.substitute_vars(stacks)),
                id: *id,
                source_ref: source_ref.clone(),
            },

            Expression::Grouped(inner, id, sr) => {
                Expression::Grouped(Box::new(inner.substitute_vars(stacks)), *id, sr.clone())
            }
            Expression::FunctionCall {
                name,
                args,
                id,
                source_ref,
            } => Expression::FunctionCall {
                name: name.clone(),
                args: args.iter().map(|arg| arg.substitute_vars(stacks)).collect(),
                id: *id,
                source_ref: source_ref.clone(),
            },
            Expression::MemberCall { object, method, args, id, source_ref } => {
                Expression::MemberCall {
                    object: Box::new(object.substitute_identifiers(stacks)),
                    method: method.clone(),
                    args: args.iter().map(|arg| arg.substitute_vars(stacks)).collect(),
                    id: *id,
                    source_ref: source_ref.clone(),
                }
            }
        }
    }

    /// Substitute identifiers in the expression using the provided mapping.
    /// If an identifier is not in the mapping, it is left unchanged.
    /// This is useful for SSA transformation where we want to replace variable names
    /// with their SSA versions, but leave other identifiers (like constants) unchanged.
    pub fn substitute_identifiers(&self, mapping: &HashMap<String, String>) -> Expression {
        match self {
            Expression::Literal(lit, id, sr) => Expression::Literal(lit.clone(), *id, sr.clone()),

            Expression::Identifier(name, id, sr) => {
                if let Some(new_name) = mapping.get(name) {
                    Expression::Identifier(new_name.clone(), *id, sr.clone())
                } else {
                    // Keep the original identifier if not in mapping
                    Expression::Identifier(name.clone(), *id, sr.clone())
                }
            }

            Expression::BinaryOp {
                left,
                op,
                right,
                id,
                source_ref,
            } => Expression::BinaryOp {
                left: Box::new(left.substitute_identifiers(mapping)),
                op: op.clone(),
                right: Box::new(right.substitute_identifiers(mapping)),
                id: *id,
                source_ref: source_ref.clone(),
            },

            Expression::UnaryOp {
                op,
                expr,
                id,
                source_ref,
            } => Expression::UnaryOp {
                op: op.clone(),
                expr: Box::new(expr.substitute_identifiers(mapping)),
                id: *id,
                source_ref: source_ref.clone(),
            },

            Expression::Grouped(inner, id, sr) => Expression::Grouped(
                Box::new(inner.substitute_identifiers(mapping)),
                *id,
                sr.clone(),
            ),

            Expression::FunctionCall {
                name,
                args,
                id,
                source_ref,
            } => Expression::FunctionCall {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| arg.substitute_identifiers(mapping))
                    .collect(),
                id: *id,
                source_ref: source_ref.clone(),
            },
            Expression::MemberCall { object, method, args, id, source_ref } => {
                Expression::MemberCall {
                    object: Box::new(object.substitute_identifiers(mapping)),
                    method: method.clone(),
                    args: args
                        .iter()
                        .map(|arg| arg.substitute_identifiers(mapping))
                        .collect(),
                    id: *id,
                    source_ref: source_ref.clone(),
                }
            }
        }
    }

    pub fn get_used_vars(&self) -> Vec<String> {
        match self {
            Expression::Literal(_, _, _) => vec![],
            Expression::Identifier(name, _, _) => vec![name.clone()],
            Expression::BinaryOp { left, right, .. } => {
                let mut vars = left.get_used_vars();
                vars.extend(right.get_used_vars());
                vars
            }
            Expression::UnaryOp { expr, .. } => expr.get_used_vars(),
            Expression::Grouped(inner, ..) => inner.get_used_vars(),
            Expression::FunctionCall { name, args, .. } => {
                let mut vars = vec![name.clone()];
                for arg in args {
                    vars.extend(arg.get_used_vars());
                }
                vars
            }
            Expression::MemberCall { object, method, args, .. } => {
                let mut vars = object.get_used_vars();
                for arg in args {
                    vars.extend(arg.get_used_vars());
                }
                vars
            }
        }
    }

    // pub fn rename_placeholder_vars(&self, new_name: &str) -> Expression {
    //     match self {
    //         Expression::Literal(lit, id, sr) => Expression::Literal(lit.clone(), *id, sr.clone()),
    //         Expression::Identifier(name, id, sr) if name.starts_with("__merak_infer_") => {
    //             Expression::Identifier(new_name.to_string(), *id, sr.clone())
    //         }
    //         Expression::Identifier(name, id, sr) => {
    //             Expression::Identifier(name.clone(), *id, sr.clone())
    //         }
    //         Expression::BinaryOp {
    //             left,
    //             op,
    //             right,
    //             id,
    //             source_ref,
    //         } => Expression::BinaryOp {
    //             left: Box::new(left.rename_placeholder_vars(new_name)),
    //             op: op.clone(),
    //             right: Box::new(right.rename_placeholder_vars(new_name)),
    //             id: *id,
    //             source_ref: source_ref.clone(),
    //         },
    //         Expression::UnaryOp {
    //             op,
    //             expr,
    //             id,
    //             source_ref,
    //         } => Expression::UnaryOp {
    //             op: op.clone(),
    //             expr: Box::new(expr.rename_placeholder_vars(new_name)),
    //             id: *id,
    //             source_ref: source_ref.clone(),
    //         },
    //         Expression::Grouped(inner, id, sr) => Expression::Grouped(
    //             Box::new(inner.rename_placeholder_vars(new_name)),
    //             *id,
    //             sr.clone(),
    //         ),
    //         Expression::FunctionCall {
    //             name,
    //             args,
    //             id,
    //             source_ref,
    //         } => Expression::FunctionCall {
    //             name: name.clone(),
    //             args: args
    //                 .iter()
    //                 .map(|arg| arg.rename_placeholder_vars(new_name))
    //                 .collect(),
    //             id: *id,
    //             source_ref: source_ref.clone(),
    //         },
    //     }
    // }
}

impl fmt::Display for Expression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expression::Literal(lit, _, _) => write!(f, "{}", lit),
            Expression::Identifier(id, _, _) => write!(f, "{}", id),
            Expression::BinaryOp {
                left, op, right, ..
            } => {
                write!(f, "{} {} {}", left, op, right)
            }
            Expression::UnaryOp { op, expr, .. } => write!(f, "{}{}", op, expr),
            Expression::Grouped(inner, _, _) => {
                write!(f, "({})", inner)
            }
            Expression::FunctionCall { name, args, .. } => {
                let args_str: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
                write!(f, "{}({})", name, args_str.join(", "))
            }
            Expression::MemberCall { object, method, args, .. } => {
                let args_str: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
                write!(f, "{}.{}({})", object, method, args_str.join(", "))
            }
        }
    }
}

impl fmt::Display for BinaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinaryOperator::LogicalOr => "||",
            BinaryOperator::LogicalAnd => "&&",
            BinaryOperator::Equal => "==",
            BinaryOperator::NotEqual => "!=",
            BinaryOperator::LessEqual => "<=",
            BinaryOperator::Less => "<",
            BinaryOperator::GreaterEqual => ">=",
            BinaryOperator::Greater => ">",
            BinaryOperator::Add => "+",
            BinaryOperator::Subtract => "-",
            BinaryOperator::Multiply => "*",
            BinaryOperator::Divide => "/",
            BinaryOperator::Modulo => "%",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for UnaryOperator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            UnaryOperator::Negate => "-",
            UnaryOperator::Not => "!",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Literal::Integer(i) => write!(f, "{}", i),
            Literal::Address(a) => write!(f, "{}", a),
            Literal::Boolean(true) => write!(f, "true"),
            Literal::Boolean(false) => write!(f, "false"),
            Literal::String(s) => write!(f, "\"{}\"", s),
        }
    }
}
