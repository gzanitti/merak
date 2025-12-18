use indexmap::IndexMap;

use crate::{
    function::Function,
    node_id::NodeId,
    statement::{Block, StateConst, StateVar},
    types::Type,
};
use std::fmt;
use std::path::PathBuf;

use super::meta::SourceRef;

pub struct Program {
    pub files: IndexMap<String, File>,
}

/// Represents an import statement.
/// Examples:
/// - `import SimpleVault from simple_vault;`
/// - `import Math from ./lib/math;`
/// - `import SimpleVault from simple_vault as Vault;`
#[derive(Debug, Clone)]
pub struct Import {
    /// The name of the contract being imported
    pub contract_name: String,
    /// The file path (relative or bare module name)
    pub file_path: PathBuf,
    /// Optional alias for the imported contract
    pub alias: Option<String>,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct InterfaceDecl {
    pub name: String,
    pub functions: Vec<InterfaceFunctionSig>,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct InterfaceFunctionSig {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct File {
    pub imports: Vec<Import>,
    pub interfaces: Vec<InterfaceDecl>,
    pub contract: Contract,
    // pub state_defs: Vec<(String, StateDef)>,
}

#[derive(Debug, Clone)]
pub struct Contract{
    pub name: String,
    pub id: NodeId,
    pub variables: Vec<StateVar>,
    pub constants: Vec<StateConst>,
    pub constructor: Option<Constructor>,
    pub functions: Vec<Function>,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct Constructor {
    pub params: Vec<Param>,
    pub body: Block,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

// #[derive(Debug, Clone)]
// pub struct StateDef {
//     pub contract: String,
//     pub name: String,
//     pub owner: Owner,
//     pub functions: Vec<Function>,
//     pub source_ref: SourceRef,
// }

// #[derive(Debug, Clone)]
// pub enum Owner {
//     Address(String),
//     Ident(String),
//     Any,
// }

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

impl Param {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

impl Constructor {
    pub fn id(&self) -> NodeId {
        self.id
    }
}

impl fmt::Display for File {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Print imports
        for import in &self.imports {
            writeln!(f, "{}", import)?;
        }
        if !self.imports.is_empty() {
            writeln!(f)?;
        }

        writeln!(f, "{}", self.contract)?;
        Ok(())
    }
}

impl fmt::Display for Import {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "import {} from {}",
            self.contract_name,
            self.file_path.display()
        )?;
        if let Some(alias) = &self.alias {
            write!(f, " as {}", alias)?;
        }
        write!(f, ";")
    }
}

impl fmt::Display for Contract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contract {}", self.name)?;
        // if !self.states.is_empty() {
        //     write!(f, "[{}]", self.states.join(", "))?;
        // } else {
        //     write!(f, "[]")?; // TODO: Invalid?
        // }
        writeln!(f, " {{")?;

        for var in &self.variables {
            writeln!(f, "    {}", var)?;
        }
        for constant in &self.constants {
            writeln!(f, "    {}", constant)?;
        }

        if let Some(constructor) = &self.constructor {
            writeln!(f)?;
            writeln!(f, "    {}", constructor)?;
        }

        for func in &self.functions {
            writeln!(f)?;
            writeln!(f, "    {}", func)?;
        }

        writeln!(f, "}}")?;

        Ok(())
    }
}

impl fmt::Display for Constructor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "constructor(")?;
        for (i, param) in self.params.iter().enumerate() {
            if i != 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", param)?;
        }
        writeln!(f, ") {{")?;
        write!(f, "{}", self.body)?;
        write!(f, "    }}")
    }
}

// impl fmt::Display for StateDef {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         write!(f, "{}@{}(", self.contract, self.name)?;
//         match &self.owner {
//             Owner::Address(addr) => write!(f, "{}", addr)?,
//             Owner::Ident(id) => write!(f, "{}", id)?,
//             Owner::Any => write!(f, "any")?,
//         }
//         writeln!(f, ") {{")?;
//         for func in &self.functions {
//             writeln!(f, "{}", func)?;
//         }
//         write!(f, "}}")
//     }
// }

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.ty)
    }
}
