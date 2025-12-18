use std::path::PathBuf;

use merak_ast::meta::SourceRef;
//use anyhow::Result;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MerakError {
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),

    #[error("File not found: {0}")]
    NotFound(PathBuf),

    #[error("Semantic error: {0}")]
    SemanticError(String),

    #[error("Internal compiler error: {0}")]
    InternalError(String),

    #[error("Duplicate states declaration in contract '{contract_name}' at {source_ref}")]
    DuplicateStatesDeclaration {
        contract_name: String,
        source_ref: SourceRef,
    },

    #[error("Duplicate states definition in contract '{contract_name}' at {source_ref}")]
    DuplicateStatesDefinition {
        contract_name: String,
        source_ref: SourceRef,
    },

    #[error("Undefined variable: {name} at {source_ref}")]
    UndefinedVariable { name: String, source_ref: SourceRef },

    #[error("Variable '{name}' already declared with type '{existing_type}' at {source_ref}")]
    VariableAlreadyDeclared {
        name: String,
        existing_type: String,
        source_ref: SourceRef,
    },

    #[error("Redefinition of '{name}' at {source_ref}")]
    ConstantRedefinition { name: String, source_ref: SourceRef },

    #[error("Function '{name}' already declared at {source_ref}")]
    FunctionRedefinition {
        name: String,
        source_ref: SourceRef,
    },

    // #[error("State '{state}' at {source_ref} is undefined")]
    // UndefinedState {
    //     state: String,
    //     source_ref: SourceRef,
    // },

    // #[error("State '{state}' is defined but not declared in contract at {source_ref}")]
    // UndeclaredState {
    //     state: String,
    //     source_ref: SourceRef,
    // },

    #[error("Type mismatch: expected {expected}, found {found} at {source_ref}")]
    TypeMismatch {
        expected: String,
        found: String,
        source_ref: SourceRef,
    },

    #[error("Base Type mismatch: expected {expected}, found {found} at {source_ref}")]
    BaseTypeMismatch {
        expected: String,
        found: String,
        source_ref: SourceRef,
    },

    #[error("Arity mismatch in '{name}' call: Expected: {expected} params, found {found} params at {source_ref}")]
    ArityMismatch {
        name: String,
        expected: usize,
        found: usize,
        source_ref: SourceRef,
    },

    #[error("Undefined function '{name}' at {source_ref}")]
    UndefinedFunction { name: String, source_ref: SourceRef },

    #[error("Return statement with value in void function at {source_ref}")]
    ReturnValueInVoidFunction { source_ref: SourceRef },

    #[error("Missing return statement in function with return type {return_type} at {source_ref}")]
    MissingReturnStatement {
        return_type: String,
        source_ref: SourceRef,
    },

    #[error("Incompatible types for operator '{operator}': cannot apply to '{left}' and '{right}' at {source_ref}")]
    IncompatibleTypes {
        left: String,
        right: String,
        operator: String,
        source_ref: SourceRef,
    },

    #[error("Invalid operator '{operator}' for type '{type_name}' at {source_ref}")]
    InvalidOperatorForType {
        operator: String,
        type_name: String,
        source_ref: SourceRef,
    },

    #[error("Loop invariant does not hold at loop entry at {source_ref}")]
    LoopInvariantEntryViolation { source_ref: SourceRef },

    #[error("Loop invariant is not preserved through loop iterations at {source_ref}")]
    LoopInvariantPreservationViolation { source_ref: SourceRef },

    #[error("Postcondition violated at {source_ref}")]
    PostconditionViolation { source_ref: SourceRef },

    #[error("Loop variant is not strictly decreasing at {source_ref}")]
    LoopVariantNotDecreasing { source_ref: SourceRef },

    #[error("Loop variant may become negative at {source_ref}")]
    LoopVariantNotBounded { source_ref: SourceRef },

    #[error("Name resolution error: {message}")]
    NameResolution { message: String },

    #[error(
        "Storage {operation} after external call. {operation} at {access_point}. Call at {call_point}"
    )]
    StorageAccessAfterExternalCall {
        operation: String,
        location_name: String,
        call_point: SourceRef,
        access_point: SourceRef,
    },

    #[error("Write to immutable variable {location_name} at {write_point}")]
    WriteToImmutable {
        location_name: String,
        write_point: SourceRef,
    },

    #[error(
        "Built-in function old can only be used in function @ensure predicates ({source_ref})"
    )]
    OldInvalidUse { source_ref: SourceRef },

    #[error("Constraint Solving Failed: {message}")]
    ConstraintSolvingFailed { message: String },

    #[error("'{name}' is not callable at {source_ref}")]
    NotCallable { name: String, source_ref: SourceRef },

    #[error("'{found}' is not callable at {source_ref}")]
    MemberCallOnNonContract { found: String, source_ref: SourceRef },

    #[error("'{method}' is not defined in contract '{contract}' at {source_ref}")]
    UndefinedMethod {method: String, contract: String, source_ref: SourceRef }
}

impl From<String> for MerakError {
    fn from(msg: String) -> Self {
        MerakError::InternalError(msg)
    }
}

impl From<&str> for MerakError {
    fn from(msg: &str) -> Self {
        MerakError::InternalError(msg.to_string())
    }
}

pub type MerakResult<T> = Result<T, MerakError>;
