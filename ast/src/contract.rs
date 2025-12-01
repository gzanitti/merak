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
    pub contracts: IndexMap<String, Contract>,
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
    /// Source location for error reporting
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct Contract {
    pub imports: Vec<Import>,
    pub data: ContractInit,
    pub state_defs: Vec<(String, StateDef)>,
}

#[derive(Debug, Clone)]
pub struct ContractInit {
    pub name: String,
    pub states: Vec<String>,
    pub variables: Vec<StateVar>,
    pub constants: Vec<StateConst>,
    pub constructor: Option<Constructor>,
}

#[derive(Debug, Clone)]
pub struct Constructor {
    pub params: Vec<Param>,
    pub body: Block,
    pub id: NodeId,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub struct StateDef {
    pub contract: String,
    pub name: String,
    pub owner: Owner,
    pub functions: Vec<Function>,
    pub source_ref: SourceRef,
}

#[derive(Debug, Clone)]
pub enum Owner {
    Address(String),
    Ident(String),
    Any,
}

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

impl Contract {
    /// Returns all functions from all state definitions
    pub fn all_functions(&self) -> Vec<Function> {
        self.state_defs
            .iter()
            .flat_map(|(_, state_def)| state_def.functions.clone())
            .collect()
    }
}

impl fmt::Display for Contract {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Print imports
        for import in &self.imports {
            writeln!(f, "{}", import)?;
        }
        if !self.imports.is_empty() {
            writeln!(f)?;
        }

        writeln!(f, "{}", self.data)?;
        for (_, state_def) in self.state_defs.iter() {
            writeln!(f, "{}", state_def)?;
        }
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

impl fmt::Display for ContractInit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "contract {}", self.name)?;
        if !self.states.is_empty() {
            write!(f, "[{}]", self.states.join(", "))?;
        } else {
            write!(f, "[]")?; // TODO: Invalid?
        }
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

impl fmt::Display for StateDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}@{}(", self.contract, self.name)?;
        match &self.owner {
            Owner::Address(addr) => write!(f, "{}", addr)?,
            Owner::Ident(id) => write!(f, "{}", id)?,
            Owner::Any => write!(f, "any")?,
        }
        writeln!(f, ") {{")?;
        for func in &self.functions {
            writeln!(f, "{}", func)?;
        }
        write!(f, "}}")
    }
}

impl fmt::Display for Param {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.ty)
    }
}
