use std::fmt;

use crate::{
    contract::Param, node_id::NodeId, predicate::Predicate, statement::Block, types::Type,
};

use super::meta::SourceRef;

#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Internal,
    External,
    Entrypoint,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Modifier {
    Payable,
    Guarded,
    Reentrant,
    Checked,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub visibility: Visibility,
    pub payable: bool,
    pub modifiers: Vec<Modifier>,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub requires: Vec<Predicate>,
    pub ensures: Vec<Predicate>,
    pub body: Block,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

impl Function {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.visibility {
            Visibility::Internal => write!(f, "internal function")?,
            Visibility::External => write!(f, "external function")?,
            Visibility::Entrypoint => write!(f, "entrypoint")?,
        }
        write!(f, " {}(", self.name)?;
        for (i, param) in self.params.iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", param)?;
        }
        write!(f, ")")?;
        for modifier in &self.modifiers {
            match modifier {
                Modifier::Payable => write!(f, " payable")?,
                Modifier::Guarded => write!(f, " guarded")?,
                Modifier::Reentrant => write!(f, " reentrant")?,
                Modifier::Checked => write!(f, "")?,
            }
        }
        if let Some(ret) = &self.return_type {
            write!(f, " -> {}", ret)?;
        }
        if !self.requires.is_empty() {
            write!(f, " requires {{")?;
            for (i, req) in self.requires.iter().enumerate() {
                if i != 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", req)?;
            }
            write!(f, "}}")?;
        }
        if !self.ensures.is_empty() {
            write!(f, " ensures {{")?;
            for (i, ens) in self.ensures.iter().enumerate() {
                if i != 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", ens)?;
            }
            write!(f, "}}")?;
        }
        writeln!(f, " {{")?;
        writeln!(f, "{}", self.body)?;
        write!(f, "}}")
    }
}
