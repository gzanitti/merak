use std::fmt;

use crate::{
    expression::Expression,
    node_id::NodeId,
    predicate::{Predicate, RefinementExpr},
    types::Type,
};

use super::meta::SourceRef;

#[derive(Debug, Clone)]
pub enum Statement {
    Expression(Expression, NodeId, SourceRef),
    If {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
        id: NodeId,
        source_ref: SourceRef,
    },
    While {
        condition: Expression,
        invariants: Vec<Predicate>,
        variants: Vec<RefinementExpr>,
        body: Block,
        id: NodeId,
        source_ref: SourceRef,
    },
    Return(Option<Expression>, NodeId, SourceRef),
    Assignment {
        target: String,
        expr: Expression,
        id: NodeId,
        source_ref: SourceRef,
    },
    VarDeclaration {
        name: String,
        ty: Option<Type>,
        expr: Expression,
        id: NodeId,
        source_ref: SourceRef,
    },
    ConstDeclaration {
        name: String,
        ty: Option<Type>,
        expr: Expression,
        id: NodeId,
        source_ref: SourceRef,
    },
    //Become(String, NodeId, SourceRef),
}

impl Statement {
    pub fn id(&self) -> NodeId {
        match self {
            Statement::Expression(_, id, _) => *id,
            Statement::If { id, .. } => *id,
            Statement::While { id, .. } => *id,
            Statement::Return(_, id, _) => *id,
            Statement::Assignment { id, .. } => *id,
            Statement::VarDeclaration { id, .. } => *id,
            Statement::ConstDeclaration { id, .. } => *id,
            //Statement::Become(_, id, _) => *id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateVar {
    pub name: String,
    pub ty: Type,
    pub expr: Expression,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

impl StateVar {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

#[derive(Debug, Clone)]
pub struct StateConst {
    pub name: String,
    pub ty: Type,
    pub expr: Expression,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

impl StateConst {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

#[derive(Debug, Clone)]
pub struct Block {
    pub statements: Vec<Statement>,
}

impl From<Vec<Statement>> for Block {
    fn from(statements: Vec<Statement>) -> Self {
        Block { statements }
    }
}

impl IntoIterator for Block {
    type Item = Statement;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.statements.into_iter()
    }
}

impl<'a> IntoIterator for &'a Block {
    type Item = &'a Statement;
    type IntoIter = std::slice::Iter<'a, Statement>;

    fn into_iter(self) -> Self::IntoIter {
        self.statements.iter()
    }
}

impl fmt::Display for Block {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for stmt in &self.statements {
            match stmt {
                // No agregar punto y coma después de bloques de control
                Statement::If { .. } | Statement::While { .. } => {
                    writeln!(f, "    {}", stmt)?;
                }
                // Para otros statements, agregar punto y coma
                _ => writeln!(f, "    {};", stmt)?,
            }
        }
        Ok(())
    }
}

impl fmt::Display for Statement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Statement::Expression(expr, ..) => write!(f, "{}", expr),
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                write!(f, "if ({}) {{\n", condition)?;
                // Indentamos el contenido del bloque then
                for stmt in &then_block.statements {
                    writeln!(f, "    {};", stmt)?;
                }
                write!(f, "}}")?;

                if let Some(else_block) = else_block {
                    write!(f, " else {{\n")?;
                    // Indentamos el contenido del bloque else
                    for stmt in &else_block.statements {
                        writeln!(f, "    {};", stmt)?;
                    }
                    write!(f, "}}")?;
                }
                Ok(())
            }
            Statement::While {
                condition, body, ..
            } => {
                write!(f, "while ({}) {{\n", condition)?;
                // Indentamos el contenido del bloque while
                for stmt in &body.statements {
                    writeln!(f, "    {};", stmt)?;
                }
                write!(f, "}}")
            }
            Statement::Return(Some(expr), _, _) => write!(f, "return {}", expr),
            Statement::Return(None, _, _) => write!(f, "return"),
            Statement::Assignment { target, expr, .. } => write!(f, "{} = {}", target, expr),
            Statement::VarDeclaration {
                name,
                ty,
                expr: value,
                ..
            } => match ty {
                Some(t) => write!(f, "var {}: {} = {}", name, t, value),
                None => write!(f, "var {} = {}", name, value),
            },
            Statement::ConstDeclaration { name, ty, expr, .. } => match ty {
                Some(t) => write!(f, "const {}: {} = {}", name, t, expr),
                None => write!(f, "const {} = {}", name, expr),
            },
            //Statement::Become(new_state, _, _) => write!(f, "become {}", new_state),
        }
    }
}

impl fmt::Display for StateVar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        //if let Some(expr) = &self.expr {
        write!(f, "state var {}: {} = {};", self.name, self.ty, self.expr)
        //} else {
        //    write!(f, "state var {}: {};", self.name, self.ty)
        //}
    }
}

impl fmt::Display for StateConst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "state const {}: {} = {};", self.name, self.ty, self.expr)
    }
}
