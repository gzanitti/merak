use merak_errors::MerakError;
use merak_ir::ssa_ir::SsaCfg;
use merak_symbols::SymbolTable;
use z3::{Config, Context};

mod constraints;
mod environment;
pub mod inference;
mod qualifiers;
mod smt;
mod solver;
mod templates;

// pub fn analyze_refinements(cfg: &SsaCfg, symbol_table: &mut SymbolTable) -> Result<(), MerakError> {
//     let z3_config = Config::new();
//     let z3_ctx = Context::new(&z3_config);
//     LiquidInferenceEngine::new(symbol_table, &z3_ctx).infer_function(cfg)?;
//     Ok(())
// }
