pub mod constraints;
mod environment;
pub mod inference;
mod qualifiers;
mod smt;
mod solver;
pub mod templates;

// pub fn analyze_refinements(cfg: &SsaCfg, symbol_table: &mut SymbolTable) -> Result<(), MerakError> {
//     let z3_config = Config::new();
//     let z3_ctx = Context::new(&z3_config);
//     LiquidInferenceEngine::new(symbol_table, &z3_ctx).infer_function(cfg)?;
//     Ok(())
// }
