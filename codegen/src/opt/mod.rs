/// Optimization framework
///
/// Optimizations are organized by the IR level they operate on:
///
/// ```text
/// opt/
///   mod.rs          — Pass<IR> trait + Pipeline<IR>  (this file)
///   lowered/        — Passes over LoweredCfg
///   ssa/            — Passes over SsaCfg  (future)
///   bytecode/       — Passes over raw bytecode  (future)
/// ```
///
/// Adding a new pass for any IR:
/// 1. Implement `Pass<YourIR>` for a unit struct.
/// 2. Add it to the appropriate submodule.
/// 3. Compose it into a `Pipeline<YourIR>` at the call site.

pub mod lowered;

/// A single optimization pass over an IR value of type `IR`.
///
/// Returns `true` if the pass made any change — used by `Pipeline` to
/// decide whether to iterate again when running to fixed point.
pub trait Pass<IR> {
    fn name(&self) -> &str;
    fn run(&self, ir: &mut IR) -> bool;
}

/// An ordered sequence of passes over the same IR type.
///
/// Passes are applied in order. The pipeline can be run once or iterated
/// until no pass makes further progress (fixed point).
///
/// # Example
/// ```ignore
/// let pipeline: Pipeline<LoweredCfg> = Pipeline::new(vec![
///     Box::new(DeadInstructionElimination),
/// ]);
/// pipeline.run_to_fixed_point(&mut lowered_cfg);
/// ```
pub struct Pipeline<IR> {
    passes: Vec<Box<dyn Pass<IR>>>,
}

impl<IR> Pipeline<IR> {
    pub fn new(passes: Vec<Box<dyn Pass<IR>>>) -> Self {
        Self { passes }
    }

    pub fn empty() -> Self {
        Self { passes: vec![] }
    }

    /// Run all passes once in order.
    /// Returns `true` if any pass made a change.
    pub fn run_once(&self, ir: &mut IR) -> bool {
        self.passes
            .iter()
            .fold(false, |changed, pass| pass.run(ir) || changed)
    }

    /// Run all passes repeatedly until none of them makes a change.
    pub fn run_to_fixed_point(&self, ir: &mut IR) {
        while self.run_once(ir) {}
    }

    /// Names of the registered passes (useful for debug output).
    pub fn pass_names(&self) -> Vec<&str> {
        self.passes.iter().map(|p| p.name()).collect()
    }
}
