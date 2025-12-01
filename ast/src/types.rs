use std::fmt;

use crate::expression::{BinaryOperator, Expression, Literal};

use crate::meta::SourceRef;
use crate::node_id::NodeId;
use crate::predicate::Predicate;

#[derive(Debug, Clone, Eq, Hash)]
pub struct Type {
    pub base: BaseType,
    pub binder: String,
    pub constraint: Predicate,
    pub source_ref: SourceRef,
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.constraint == other.constraint
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BaseType {
    Int,
    Address,
    Bool,
    String,
    Tuple {
        elems: Vec<Type>,
    },
    Function {
        name: String,
        parameters: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Type {
    pub fn new(base: BaseType, binder: String) -> Self {
        Type {
            base,
            binder,
            constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
            source_ref: SourceRef::unknown(),
        }
    }

    pub fn with_binder(&mut self, binder: String) {
        self.binder = binder;
    }

    pub fn empty_tuple(binder: String) -> Self {
        Type {
            base: BaseType::Tuple { elems: vec![] },
            binder: binder.clone(),
            constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
            source_ref: SourceRef::unknown(),
        }
    }

    pub fn is_true_literal(&self) -> bool {
        matches!(&self.constraint, Predicate::True(_, _))
    }
}

impl fmt::Display for BaseType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BaseType::Int => write!(f, "int"),
            BaseType::Address => write!(f, "address"),
            BaseType::Bool => write!(f, "bool"),
            BaseType::String => write!(f, "string"),
            BaseType::Tuple { elems } => {
                let elems_str: Vec<String> = elems.iter().map(|e| e.to_string()).collect();
                write!(f, "({})", elems_str.join(", "))
            }
            BaseType::Function {
                name,
                parameters,
                return_type,
            } => {
                let params_str: Vec<String> = parameters.iter().map(|p| p.to_string()).collect();
                write!(f, "{name}({} -> {return_type})", params_str.join(", "))
            }
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.constraint {
            Predicate::True(_, _) => {
                write!(f, "{}", self.base)
            }
            _ => {
                write!(
                    f,
                    "{{{}: {} | {}}}",
                    self.binder, self.base, self.constraint
                )
            }
        }
    }
}
