/// Optimization passes over `LoweredCfg`

pub mod dead_code;

pub use dead_code::DeadInstructionElimination;
