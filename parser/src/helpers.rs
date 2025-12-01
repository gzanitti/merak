use std::error::Error;

use hex;
use merak_ast::{
    function::Function,
    meta::SourceRef,
    predicate::{RefinementExpr, Predicate},
    statement::{StateConst, StateVar, Statement},
    NodeIdGenerator,
};

#[derive(Debug)]
pub enum StateItem {
    Var(StateVar),
    Const(StateConst),
}

#[derive(Debug)]
pub enum LoopProperty {
    Invariant(Predicate),
    Variant(RefinementExpr),
}
// Helper function to ensure functions have return statements
pub fn ensure_return_statement(mut function: Function) -> Function {
    if !has_return_statement(&function.body.statements) {
        // Always add "return;" (with None) regardless of return type
        let temp_gen = NodeIdGenerator::new();
        let return_stmt = Statement::Return(None, temp_gen.next(), SourceRef::unknown());
        function.body.statements.push(return_stmt);
    }
    function
}

// Check if a block has any return statement (recursively)
fn has_return_statement(statements: &[Statement]) -> bool {
    for stmt in statements {
        match stmt {
            Statement::Return(_, _, _) => return true,
            Statement::If {
                then_block,
                else_block,
                ..
            } => {
                if has_return_statement(&then_block.statements) {
                    if let Some(else_block) = else_block {
                        if has_return_statement(&else_block.statements) {
                            return true;
                        }
                    }
                }
            }
            Statement::While { .. } => {
                // While loops don't guarantee execution, so we don't consider them
                // as having guaranteed returns
            }
            _ => {}
        }
    }
    false
}

/// Parses a hexadecimal string into an H256 value.
///
/// This function handles hexadecimal strings of any length (up to 64 hex digits),
/// with or without the "0x" prefix. If the input is shorter than 32 bytes,
/// it will be left-padded with zeros.
///
/// # Arguments
///
/// * `hex_str` - A string slice containing the hexadecimal value.
///
/// # Returns
///
/// A `Result` which is either:
/// * `Ok(H256)` containing the decoded value.
/// * `Err` if the input string is not a valid hexadecimal format or too long.
pub fn parse_hexa_h256(hex_str: &str) -> Result<primitive_types::H256, Box<dyn Error>> {
    let hex_to_decode = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let bytes_vec = hex::decode(hex_to_decode)?;

    if bytes_vec.len() > 32 {
        return Err("Hex string too long for H256".into());
    }

    let mut padded = [0u8; 32];
    // Left-pad with zeros
    padded[32 - bytes_vec.len()..].copy_from_slice(&bytes_vec);

    Ok(primitive_types::H256::from(padded))
}
