use std::{fs, path::PathBuf};

use lalrpop_util::{lalrpop_mod, ParseError};
use merak_ast::{contract::Contract, NodeIdGenerator};
use merak_errors::MerakError;

mod helpers;
lalrpop_mod!(merak);

pub fn parse_program(source: &str) -> Result<Contract, MerakError> {
    let id_gen = NodeIdGenerator::new();
    match merak::ProgramParser::new().parse(&id_gen, source) {
        Ok(program) => Ok(program),
        Err(err) => match err.clone() {
            ParseError::InvalidToken { location } => todo!("Better error: {err}"),
            ParseError::UnrecognizedEof { location, expected } => todo!("Better error: {err}"),
            ParseError::UnrecognizedToken { token, expected } => todo!("Better error: {err}"),
            ParseError::ExtraToken { token } => todo!("Better error: {err}"),
            ParseError::User { error } => todo!("Better error: {err}"),
        },
    }
}

pub fn parse_file(path: &PathBuf) -> Result<Contract, MerakError> {
    let source = fs::read_to_string(path).expect("Failed to read file");
    parse_program(&source)
}
