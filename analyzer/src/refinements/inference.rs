use std::collections::HashSet;

use merak_ast::{meta::SourceRef, predicate::Predicate, types::BaseType, NodeId};
use merak_errors::MerakError;
use merak_ir::ssa_ir::{
    AssertKind, BasicBlock, Constant, Operand, Register, SsaCfg, SsaInstruction, Terminator,
};
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};
use z3::Context;

use crate::{
    refinements::{
        constraints::{Constraint, ConstraintSet},
        environment::TypeEnvironment,
        qualifiers::QualifierSet,
        smt::SmtSolver,
        solver::ConstraintSolver,
        templates::{LiquidAssignment, Template},
    },
    storage::StorageAnalysis,
};

pub struct LiquidInferenceEngine<'a> {
    // El environment maneja symbol table, liquid vars, assumptions
    env: TypeEnvironment<'a>,

    // Store de constraints acumulados
    constraints: ConstraintSet,

    // Set estático de qualifiers
    qualifiers: QualifierSet,
}

impl<'a> LiquidInferenceEngine<'a> {
    pub fn new(symbol_table: &'a mut SymbolTable, storage_analysis: StorageAnalysis) -> Self {
        Self {
            env: TypeEnvironment::new(symbol_table, storage_analysis),
            constraints: ConstraintSet::new(),
            qualifiers: QualifierSet::core(),
        }
    }

    pub fn infer_function(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        // Setup function context
        self.env.enter_function(cfg.function_id);

        self.assign_templates(cfg)?;

        self.generate_constraints(cfg)?;

        self.solve_constraints()?;

        // Cleanup
        self.env.exit_function();

        Ok(())
    }

    fn assign_templates(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        for param in &cfg.parameters {
            let param_reg = Register {
                symbol: *param,
                version: 0,
            };

            // Param are always concrete
            let ty = self
                .env
                .get_symbol_type(param_reg.symbol)
                .cloned()
                .ok_or_else(|| panic!("Rompio assign_template con param {:?}, revisar", param_reg,))
                .unwrap();

            self.env.bind_local(
                param_reg.to_string(),
                Template::Concrete {
                    base: ty.base,
                    binder: ty.binder,
                    refinement: ty.constraint,
                    source_ref: ty.source_ref,
                },
            );
        }

        // Order doesn't matter and we already have RPO implemented
        for block_id in cfg.reverse_post_order() {
            for instr in &cfg.blocks[&block_id].instructions {
                if let Some(dest) = instr.dest_register() {
                    let template = self.register_to_template(&dest, *block_id);
                    self.env.bind_local(dest.to_string(), template);
                }
            }
        }

        Ok(())
    }

    fn register_to_template(&mut self, reg: &Register, block_id: usize) -> Template {
        let ty = self
            .env
            .get_symbol_type(reg.symbol)
            .cloned()
            .ok_or_else(|| {
                panic!(
                    "Rompio assign_template_for_register con registro {:?}, revisar",
                    reg,
                )
            })
            .unwrap();

        let storage_state = self
            .env
            .storage_analysis
            .storage_states
            .entry_state(block_id);

        if storage_state.is_invalidated(reg.symbol) {
            // Invalid storage, we can't assume refinements
            Template::Unrefined {
                base: ty.base,
                source_ref: ty.source_ref,
            }
        } else {
            if ty.is_true_literal() {
                Template::Liquid {
                    base: ty.base,
                    binder: ty.binder,
                    liquid_var: self.env.fresh_liquid_var(),
                    source_ref: ty.source_ref,
                }
            } else {
                Template::Concrete {
                    base: ty.base,
                    binder: ty.binder,
                    refinement: ty.constraint,
                    source_ref: ty.source_ref,
                }
            }
        }
    }

    fn operand_to_template(&mut self, operand: &Operand, block_id: usize) -> Template {
        match operand {
            Operand::Register(reg) => self.register_to_template(reg, block_id),
            Operand::Constant(constant) => {
                let base = match constant {
                    Constant::Int(constant) => BaseType::Int,
                    Constant::Bool(constant) => BaseType::Bool,
                    Constant::Address(h256) => BaseType::Address,
                    Constant::String(_) => BaseType::String,
                };
                Template::Concrete {
                    base,
                    binder: "v".to_string(),
                    refinement: Predicate::True(NodeId::new(0), SourceRef::unknown()), // TODO: True o var == value?
                    source_ref: SourceRef::unknown(),
                }
            }
        }
    }

    pub fn generate_constraints(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        for req_pred in &cfg.requires {
            self.env.add_assumption(req_pred.clone());
        }

        let mut visited = HashSet::new();
        self.generate_constraints_from_block(cfg.entry, cfg, &mut visited)
    }

    fn generate_constraints_from_block(
        &mut self,
        block_id: usize,
        cfg: &SsaCfg,
        visited: &mut HashSet<usize>,
    ) -> Result<(), MerakError> {
        // Avoid re-visiting blocks (handles loops/back edges)
        if visited.contains(&block_id) {
            return Ok(());
        }
        visited.insert(block_id);

        let block = &cfg.blocks[&block_id];

        // Generate constraints for instructions
        for instr in &block.instructions {
            self.generate_constraints_instruction(instr, block.id)?;
        }

        // Follow control flow with path-sensitive assumptions
        match &block.terminator {
            Terminator::Branch {
                condition,
                then_block,
                else_block,
                source_ref,
            } => {
                let cond_pred = self.operand_to_predicate(condition);

                // Then-branch: add assumption, recurse, remove assumption
                self.env.add_assumption(cond_pred.clone());
                let then_result = self.generate_constraints_from_block(*then_block, cfg, visited);
                self.env.pop_assumption();
                then_result?; // Propagate error AFTER cleanup

                // Else-branch: add negated assumption, recurse, remove assumption
                self.env.add_assumption(cond_pred.negate());
                let else_result = self.generate_constraints_from_block(*else_block, cfg, visited);
                self.env.pop_assumption();
                else_result?; // Propagate error AFTER cleanup
            }

            Terminator::Jump { target } => {
                self.generate_constraints_from_block(*target, cfg, visited)?;
            }

            Terminator::Return { value, source_ref } => {
                if let Some(ret_operand) = value {
                    self.generate_return_constraint(ret_operand, block_id)?;
                }

                for postcond in &cfg.ensures {
                    let ctx = self.env.to_type_context();
                    self.constraints.add(Constraint::Ensures {
                        context: ctx,
                        condition: postcond.clone(),
                        location: postcond.source_ref().clone(),
                    });
                }
            }
            Terminator::Unreachable => {}
        }

        Ok(())
    }

    fn generate_constraints_instruction(
        &mut self,
        instr: &SsaInstruction,
        block_id: usize,
    ) -> Result<(), MerakError> {
        let context = self.env.to_type_context();

        match instr {
            SsaInstruction::Copy {
                dest,
                source,
                source_ref,
            } => {
                let rhs = self.operand_to_template(source, block_id);
                let lhs = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (copy)");
                self.constraints.add(Constraint::Subtype {
                    context,
                    lhs,
                    rhs,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::Phi { dest, sources } => {
                let rhs = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (phi)");

                for (_, source_reg) in sources {
                    // TODO: Ideally we'd use the environment from pred_block
                    // For now, use current context (conservative)
                    let lhs = self
                        .env
                        .lookup(&source_reg.to_string())
                        .expect("Rompio en un source (phi)");

                    self.constraints.add(Constraint::Subtype {
                        context: context.clone(),
                        lhs,
                        rhs: rhs.clone(),
                        location: SourceRef::unknown(),
                    });
                }
            }
            SsaInstruction::BinaryOp {
                dest,
                op,
                left,
                right,
                source_ref,
            } => {
                let dest_tmpl = self.register_to_template(dest, block_id);
                let left_tmpl = self.operand_to_template(left, block_id);
                let right_tmpl = self.operand_to_template(right, block_id);

                // Check operands are well-formed
                self.constraints.add(Constraint::WellFormed {
                    context: context.clone(),
                    template: left_tmpl.clone(),
                    location: source_ref.clone(),
                });

                self.constraints.add(Constraint::WellFormed {
                    context: context.clone(),
                    template: right_tmpl.clone(),
                    location: source_ref.clone(),
                });

                self.constraints.add(Constraint::WellFormed {
                    context: context.clone(),
                    template: dest_tmpl.clone(),
                    location: source_ref.clone(),
                });

                self.constraints.add(Constraint::BinaryOp {
                    context: context.clone(),
                    op: op.clone(),
                    left: left_tmpl,
                    right: right_tmpl,
                    result: dest_tmpl,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::UnaryOp {
                dest,
                op,
                operand,
                source_ref,
            } => {
                let dest_tmpl = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (unry)");
                let operand_tmpl = self.operand_to_template(operand, block_id);

                self.constraints.add(Constraint::WellFormed {
                    context: context.clone(),
                    template: operand_tmpl.clone(),
                    location: source_ref.clone(),
                });

                self.constraints.add(Constraint::WellFormed {
                    context: context.clone(),
                    template: dest_tmpl.clone(),
                    location: source_ref.clone(),
                });

                self.constraints.add(Constraint::UnaryOp {
                    context,
                    op: op.clone(),
                    operand: operand_tmpl,
                    result: dest_tmpl,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::StorageLoad {
                dest,
                var,
                source_ref,
            } => {
                let dest_tmpl = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (storageLoad)");

                let storage_tmpl = self
                    .env
                    .get_symbol_template(*var)
                    .expect("Rompio en storagelod var");

                self.constraints.add(Constraint::Subtype {
                    context,
                    lhs: storage_tmpl,
                    rhs: dest_tmpl,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::StorageStore {
                var,
                value,
                source_ref,
            } => {
                let value_tmpl = self.operand_to_template(value, block_id);
                let storage_tmpl = self
                    .env
                    .get_symbol_template(*var)
                    .expect("Rompio en storageStore");

                // Constraint: value type <: storage type
                self.constraints.add(Constraint::Subtype {
                    context,
                    lhs: value_tmpl,
                    rhs: storage_tmpl,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::Call {
                dest,
                target,
                args,
                source_ref,
            } => {
                if let Some(dest_reg) = dest {
                    let dest_tmpl = self
                        .env
                        .lookup(&dest_reg.to_string())
                        .expect("Rompio el call");

                    self.constraints.add(Constraint::WellFormed {
                        context,
                        template: dest_tmpl,
                        location: source_ref.clone(),
                    });
                }

                unimplemented!("Calls are not implemented yet")
                // TODO: Check args satisfy preconditions
                // TODO: Assume postconditions for return value
            }

            SsaInstruction::Assert {
                condition,
                kind,
                source_ref,
            } => {
                unimplemented!("User defined asserts not implemented yet.")
            }
            SsaInstruction::StateTransition { .. } => {}
        }

        Ok(())
    }

    fn generate_return_constraint(
        &mut self,
        ret_operand: &Operand,
        block_id: usize,
    ) -> Result<(), MerakError> {
        let func_id = self.env.current_function().unwrap();

        let func_symbol = self.env.symbol_table().get_symbol(func_id);

        // Extract return type from function kind and clone to end the immutable borrow
        let declared_return_type = match &func_symbol.kind {
            SymbolKind::Function { return_type, .. } => return_type.clone(), // TODO: Clone
            _ => {
                panic!("Current function is not a function symbol")
            }
        };

        let expected_template =
            Template::from_type(&declared_return_type, &mut self.env.liquid_gen);

        let actual_template = self.operand_to_template(ret_operand, block_id);

        // Generate subtyping constraint: actual <: expected
        let ctx = self.env.to_type_context();
        self.constraints.add(Constraint::Subtype {
            context: ctx,
            lhs: actual_template,
            rhs: expected_template,
            location: SourceRef::unknown(), //ret_operand.source_ref(),
        });

        Ok(())
    }
    fn operand_to_predicate(&mut self, operand: &Operand) -> Predicate {
        match operand {
            Operand::Register(register) => {
                // A register used as a predicate represents a boolean variable
                // Convert register name to a predicate variable
                Predicate::Var(
                    format!(
                        "{}(get name from symbol_id)_{}",
                        register.symbol, register.version
                    ), // TODO: Name from SymbolId
                    NodeId::new(0), // TODO: NodeId for predicate variable
                    SourceRef::unknown(),
                )
            }

            Operand::Constant(constant) => {
                // Only boolean constants make sense as predicates
                match constant {
                    Constant::Bool(value) => {
                        if *value {
                            Predicate::True(
                                NodeId::new(0), // TODO: NodeId for predicate variable
                                SourceRef::unknown(),
                            )
                        } else {
                            Predicate::False(
                                NodeId::new(0), // TODO: NodeId for predicate variable
                                SourceRef::unknown(),
                            )
                        }
                    }
                    _ => {
                        panic!("Non-boolean constants as predicates don't make sense")
                    }
                }
            }
        }
    }

    fn solve_constraints(&mut self) -> Result<(), MerakError> {
        let z3_context = z3::Context::new(&z3::Config::new());
        let solver = ConstraintSolver::new(self.constraints.clone(), &z3_context);

        let assignment = solver
            .solve()
            .map_err(|e| MerakError::ConstraintSolvingFailed {
                message: format!("{:?}", e),
            })?;

        Ok(())
    }
}
