use std::collections::{HashMap, HashSet};

use merak_ast::{NodeId, expression::{BinaryOperator, UnaryOperator}, meta::SourceRef, predicate::{ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp}, types::BaseType};
use merak_errors::MerakError;
use merak_ir::ssa_ir::{
    AssertKind, BasicBlock, CallTarget, Constant, Operand, Register, SsaCfg, SsaInstruction, Terminator
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
    pub fn new(symbol_table: &'a mut SymbolTable) -> Self {
        Self {
            env: TypeEnvironment::new(symbol_table),
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
                    println!("Register -> {}", dest);
                    let template = match &dest.symbol {
                    SymbolId::Named(_) => {
                        let ty = self.env.get_symbol_type(dest.symbol)
                            .ok_or_else(|| MerakError::NameResolution {
                                message: format!("Named symbol {:?} not in symbol table", dest.symbol)
                            })?;

                        if ty.is_explicit_annotation() {
                            Template::Concrete {
                                base: ty.base.clone(),
                                binder: ty.binder.clone(),
                                refinement: ty.constraint.clone(),
                                source_ref: ty.source_ref.clone(),
                            }
                        } else {
                            // Clonar los datos antes del préstamo mutable
                            let base = ty.base.clone();
                            let binder = ty.binder.clone();
                            let source_ref = ty.source_ref.clone();

                            // Ahora podemos llamar fresh_liquid_var sin conflicto
                            let liquid_var = self.env.fresh_liquid_var();

                            Template::Liquid {
                                base,
                                binder,
                                liquid_var,
                                source_ref,
                            }
                        }
                    }
                    
                    SymbolId::Temp(_) => {
                        self.infer_template_from_instruction(instr, cfg)?
                    }
                };
                
                self.env.bind_local(dest.to_string(), template);
                }
            }
        }

        Ok(())
    }


    /// Infiere el template de un registro basándose en la instrucción que lo define
    fn infer_template_from_instruction(
        &mut self,
        instr: &SsaInstruction,
        cfg: &SsaCfg
    ) -> Result<Template, MerakError> {
        match instr {
            SsaInstruction::BinaryOp { left, op, right, dest, source_ref } => {
                let base_type = cfg.local_temps.get(dest)
                    .ok_or_else(|| MerakError::NameResolution { // TODO: Name resolution? Do better man
                        message: format!("Missing base type for {:?}", dest)
                    })?;
                
                self.infer_binary_op_template(left, op, right, base_type, source_ref)
            }
            
            SsaInstruction::UnaryOp { op, operand, dest, source_ref } => {
                let base_type = cfg.local_temps.get(dest)
                    .ok_or_else(|| MerakError::NameResolution {
                        message: format!("Missing base type for {:?}", dest)
                    })?;
                
                self.infer_unary_op_template(op, operand, base_type, source_ref)
            }
            
            SsaInstruction::Copy { source, dest, source_ref } => {
                Ok(self.operand_to_template(source, cfg))
            }
            
            SsaInstruction::StorageLoad { var, dest, source_ref } => {
                let ty = self.env.get_symbol_type(*var)
                    .ok_or_else(|| MerakError::NameResolution {
                        message: format!("Storage variable {:?} not found", var)
                    })?;
                
                Ok(Template::Concrete {
                    base: ty.base.clone(),
                    binder: ty.binder.clone(),
                    refinement: ty.constraint.clone(),
                    source_ref: source_ref.clone(),
                })
            }
            SsaInstruction::Phi { sources, dest } => {
                let base_type = cfg.local_temps.get(dest)
                    .ok_or_else(|| MerakError::NameResolution {
                        message: format!("Missing base type for {:?}", dest)
                    })?;
                
                // Phi requiere inferencia - crear template líquido
                Ok(Template::Liquid {
                    base: base_type.clone(),
                    binder: "v".to_string(),
                    liquid_var: self.env.fresh_liquid_var(),
                    source_ref: SourceRef::unknown(),
                })
            }
            
            SsaInstruction::Call { 
                dest: Some(dest_reg), 
                target: CallTarget::Internal(fn_id),
                args, 
                source_ref 
            } => {
                let fn_symbol = self.env.get_symbol(*fn_id);
                
                let return_type = match &fn_symbol.kind {
                    SymbolKind::Function { return_type, .. } |
                    SymbolKind::Entrypoint { return_type, .. } => return_type.clone(),
                    _ => return Err(MerakError::NameResolution {
                        message: format!("Symbol {:?} is not a function", fn_id)
                    })
                };
                
                // TODO: El tipo de retorno puede contener referencias a parámetros formales
                // Necesitamos sustituir los formales con los actuales
                // Por ahora, retornamos el tipo tal cual (la sustitución se hace en generate_constraints)
                Ok(Template::from_type(&return_type, &mut self.env.liquid_gen))
            }
            
            // ========== CALL (EXTERNAL) ==========
            SsaInstruction::Call { 
                dest: Some(_), 
                target: CallTarget::External { object, method }, 
                source_ref, .. 
            } => {
                // Para external calls, buscar el return type en la interface
                // Por ahora, crear liquid genérico (esto necesita más info de interfaces)
                Ok(Template::Liquid {
                    base: BaseType::Int, // TODO: placeholder - necesita lookup real
                    binder: "v".to_string(),
                    liquid_var: self.env.fresh_liquid_var(),
                    source_ref: source_ref.clone(),
                })
            }
            

            SsaInstruction::Call { dest: None, .. } |
            SsaInstruction::StorageStore { .. } |
            SsaInstruction::Assert { .. } |
            SsaInstruction::Fold { .. } |
            SsaInstruction::Unfold { .. } => {
                Err(MerakError::NameResolution {
                    message: format!("Instruction {:?} does not produce a value", instr)
                })
            }
        }
    }
    
    fn infer_binary_op_template(
        &mut self,
        left: &Operand,
        op: &BinaryOperator,
        right: &Operand,
        base_type: &BaseType,
        source_ref: &SourceRef
    ) -> Result<Template, MerakError> {
        match op {
            // Refinement: v = left op right
            BinaryOperator::Add | BinaryOperator::Subtract | 
            BinaryOperator::Multiply | BinaryOperator::Divide | 
            BinaryOperator::Modulo => {
                let arith_op = match op {
                    BinaryOperator::Add => ArithOp::Add,
                    BinaryOperator::Subtract => ArithOp::Sub,
                    BinaryOperator::Multiply => ArithOp::Mul,
                    BinaryOperator::Divide => ArithOp::Div,
                    BinaryOperator::Modulo => ArithOp::Mod,
                    _ => unreachable!()
                };
                
                Ok(Template::Concrete {
                    base: base_type.clone(),
                    binder: "v".to_string(),
                    refinement: Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: RefinementExpr::Var(
                            "v".to_string(),
                            NodeId::new(0),
                            SourceRef::unknown()
                        ),
                        rhs: RefinementExpr::BinOp {
                            op: arith_op,
                            lhs: Box::new(self.operand_to_refinement_expr(left)),
                            rhs: Box::new(self.operand_to_refinement_expr(right)),
                            id: NodeId::new(0),
                            source_ref: SourceRef::unknown(),
                        },
                        id: NodeId::new(0),
                        source_ref: source_ref.clone(),
                    },
                    source_ref: source_ref.clone(),
                })
            }
            
            // ========== COMPARACIONES ==========
            // Refinement: v ⇔ (left op right)
            BinaryOperator::Less | BinaryOperator::LessEqual |
            BinaryOperator::Greater | BinaryOperator::GreaterEqual => {
                let rel_op = match op {
                    BinaryOperator::Less => RelOp::Lt,
                    BinaryOperator::LessEqual => RelOp::Leq,
                    BinaryOperator::Greater => RelOp::Gt,
                    BinaryOperator::GreaterEqual => RelOp::Geq,
                    _ => unreachable!()
                };
                
                // Construir: v ⇔ (left rel_op right)
                let comparison = Predicate::BinRel {
                    op: rel_op,
                    lhs: self.operand_to_refinement_expr(left),
                    rhs: self.operand_to_refinement_expr(right),
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                };
                
                // v ⇔ comparison (necesitamos Iff en Predicate)
                // Por ahora, simplificamos: si v es bool, solo ponemos la comparación
                Ok(Template::Concrete {
                    base: BaseType::Bool,
                    binder: "v".to_string(),
                    refinement: comparison,
                    source_ref: source_ref.clone(),
                })
            }
            
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                let rel_op = match op {
                    BinaryOperator::Equal => RelOp::Eq,
                    BinaryOperator::NotEqual => RelOp::Neq,
                    _ => unreachable!()
                };
                
                let comparison = Predicate::BinRel {
                    op: rel_op,
                    lhs: self.operand_to_refinement_expr(left),
                    rhs: self.operand_to_refinement_expr(right),
                    id: NodeId::new(0),
                    source_ref: SourceRef::unknown(),
                };
                
                Ok(Template::Concrete {
                    base: BaseType::Bool,
                    binder: "v".to_string(),
                    refinement: comparison,
                    source_ref: source_ref.clone(),
                })
            }
            
            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr => {
                let left_pred = self.operand_to_predicate(left);
                let right_pred = self.operand_to_predicate(right);
                
                let logical_pred = match op {
                    BinaryOperator::LogicalAnd => Predicate::And(
                        Box::new(left_pred),
                        Box::new(right_pred),
                        NodeId::new(0),
                        SourceRef::unknown()
                    ),
                    BinaryOperator::LogicalOr => Predicate::Or(
                        Box::new(left_pred),
                        Box::new(right_pred),
                        NodeId::new(0),
                        SourceRef::unknown()
                    ),
                    _ => unreachable!()
                };
                
                Ok(Template::Concrete {
                    base: BaseType::Bool,
                    binder: "v".to_string(),
                    refinement: logical_pred,
                    source_ref: source_ref.clone(),
                })
            }
        }
    }
    
    fn infer_unary_op_template(
        &mut self,
        op: &UnaryOperator,
        operand: &Operand,
        base_type: &BaseType,
        source_ref: &SourceRef
    ) -> Result<Template, MerakError> {
        match op {
            // Refinement: v = -operand
            UnaryOperator::Negate => {
                Ok(Template::Concrete {
                    base: base_type.clone(),
                    binder: "v".to_string(),
                    refinement: Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: RefinementExpr::Var(
                            "v".to_string(),
                            NodeId::new(0),
                            SourceRef::unknown()
                        ),
                        rhs: RefinementExpr::UnaryOp {
                            op: UnaryOp::Negate,
                            expr: Box::new(self.operand_to_refinement_expr(operand)),
                            id: NodeId::new(0),
                            source_ref: SourceRef::unknown(),
                        },
                        id: NodeId::new(0),
                        source_ref: source_ref.clone(),
                    },
                    source_ref: source_ref.clone(),
                })
            }
            
            // Refinement: v ⇔ ¬operand
            UnaryOperator::Not => {
                let operand_pred = self.operand_to_predicate(operand);
                
                Ok(Template::Concrete {
                    base: BaseType::Bool,
                    binder: "v".to_string(),
                    refinement: Predicate::Not(
                        Box::new(operand_pred),
                        NodeId::new(0),
                        source_ref.clone()
                    ),
                    source_ref: source_ref.clone(),
                })
            }
        }
    }
    
    /// Convierte un Operand a RefinementExpr para usar en refinements
    fn operand_to_refinement_expr(&self, operand: &Operand) -> RefinementExpr {
        match operand {
            Operand::Register(reg) => {
                // Referencia al registro como variable en el refinement
                RefinementExpr::Var(
                    reg.to_string(), // "x_0", "temp_5", etc.
                    NodeId::new(0),
                    SourceRef::unknown()
                )
            }
            
            Operand::Constant(constant) => {
                match constant {
                    Constant::Int(value) => RefinementExpr::IntLit(
                        *value,
                        NodeId::new(0),
                        SourceRef::unknown()
                    ),
                    Constant::Bool(value) => {
                        // Los booleanos no son RefinementExpr, necesitaríamos convertirlos
                        // o agregar un BoolLit variant
                        // Por ahora, usamos 0/1 como workaround
                        RefinementExpr::IntLit(
                            if *value { 1 } else { 0 },
                            NodeId::new(0),
                            SourceRef::unknown()
                        )
                    }
                    Constant::Address(addr) => RefinementExpr::AddressLit(
                        addr.to_string(),
                        NodeId::new(0),
                        SourceRef::unknown()
                    ),
                    Constant::String(_) => {
                        // Strings no tienen representación en refinements
                        // Usar placeholder
                        RefinementExpr::IntLit(0, NodeId::new(0), SourceRef::unknown())
                    }
                }
            }
        }
    }
    
    // Convierte un Operand a Predicate (para operaciones lógicas)
    // fn operand_to_predicate(&self, operand: &Operand) -> Result<Predicate, MerakError> {
    //     match operand {
    //         Operand::Register(reg) => {
    //             // Un registro booleano se convierte en un Predicate::Var
    //             Ok(Predicate::Var(
    //                 reg.to_string(),
    //                 NodeId::new(0),
    //                 SourceRef::unknown()
    //             ))
    //         }
            
    //         Operand::Constant(constant) => {
    //             match constant {
    //                 Constant::Bool(value) => {
    //                     Ok(if *value {
    //                         Predicate::True(NodeId::new(0), SourceRef::unknown())
    //                     } else {
    //                         Predicate::False(NodeId::new(0), SourceRef::unknown())
    //                     })
    //                 }
    //                 _ => Err(MerakError::TypeError {
    //                     message: "Expected boolean operand for logical operation".to_string()
    //                 })
    //             }
    //         }
    //     }
    // }


//     fn register_to_template(&mut self, reg: &Register, cfg: &SsaCfg) -> Template {
//     let ty = match &reg.symbol {
//         SymbolId::Named(_) => {
//             let ty = self.env.get_symbol_type(reg.symbol)
//                 .expect("Named symbol must be in symbol table");
            
//             if ty.is_explicit_annotation() {
//                 return Template::Concrete {
//                     base: ty.base.clone(),
//                     binder: ty.binder.clone(),
//                     refinement: ty.constraint.clone(),
//                     source_ref: ty.source_ref.clone(),
//                 };
//             }
            
//             &ty.base
//         }
//         SymbolId::Temp(_) => {
//             cfg.local_temps.get(&reg)
//                 .expect("Temp register must have base type from SSA")

//         }
//     };

//     Template::Liquid {
//         base: ty.clone(),
//         binder: "v".to_string(),
//         liquid_var: self.env.fresh_liquid_var(),
//         source_ref: SourceRef::unknown(),
//     }
// }

    // fn register_to_template(&mut self, reg: &Register) -> Template {
    //     println!("Register {}", reg);
    //     println!("Symbols {:?}", self.env.symbol_table());
    //     let ty = self
    //         .env
    //         .get_symbol_type(reg.symbol)
    //         .cloned()
    //         .ok_or_else(|| {
    //             panic!(
    //                 "Rompio assign_template_for_register con registro {:?}, revisar",
    //                 reg,
    //             )
    //         })
    //         .unwrap();



    //     if ty.is_explicit_annotation() {
    //         Template::Concrete {
    //             base: ty.base,
    //             binder: ty.binder,
    //             refinement: ty.constraint,
    //             source_ref: ty.source_ref,
    //         }
    //     } else {
    //         Template::Liquid {
    //             base: ty.base,
    //             binder: ty.binder,
    //             liquid_var: self.env.fresh_liquid_var(),
    //             source_ref: ty.source_ref,
    //         }
    //     }
        
    // }

    fn operand_to_template(&mut self, operand: &Operand, cfg: &SsaCfg) -> Template {
        match operand {
            Operand::Register(reg) => self.env.get_local(&reg.to_string()).unwrap(),
            Operand::Constant(constant) => {
                match constant {
                    Constant::Int(value) => {
                        Template::Concrete {
                            base: BaseType::Int,
                            binder: "v".to_string(),
                            refinement: Predicate::BinRel {
                                op: RelOp::Eq,
                                lhs: RefinementExpr::Var("v".to_string(), NodeId::new(0), SourceRef::unknown()),
                                rhs: RefinementExpr::IntLit(*value, NodeId::new(0), SourceRef::unknown()),
                                id: NodeId::new(0),
                                source_ref: SourceRef::unknown(),
                            },
                            source_ref: SourceRef::unknown(),
                        }
                    }
                    Constant::Bool(value) => {
                        Template::Concrete {
                            base: BaseType::Bool,
                            binder: "v".to_string(),
                            refinement: if *value {
                                Predicate::True(NodeId::new(0), SourceRef::unknown())
                            } else {
                                Predicate::False(NodeId::new(0), SourceRef::unknown())
                            },
                            source_ref: SourceRef::unknown(),
                        }
                    }
                    Constant::Address(addr) => {
                        Template::Concrete {
                            base: BaseType::Address,
                            binder: "v".to_string(),
                            refinement: Predicate::BinRel {
                                op: RelOp::Eq,
                                lhs: RefinementExpr::Var("v".to_string(), NodeId::new(0), SourceRef::unknown()),
                                rhs: RefinementExpr::AddressLit(addr.to_string(), NodeId::new(0), SourceRef::unknown()),
                                id: NodeId::new(0),
                                source_ref: SourceRef::unknown(),
                            },
                            source_ref: SourceRef::unknown(),
                        }
                    }
                    Constant::String(_) => {
                        // TODO: No hay RefinementExpr para string literals, usamos True por ahora
                        Template::Concrete {
                            base: BaseType::String,
                            binder: "v".to_string(),
                            refinement: Predicate::True(NodeId::new(0), SourceRef::unknown()),
                            source_ref: SourceRef::unknown(),
                        }
                    }
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
            self.generate_constraints_instruction(instr, block.id, cfg)?;
        }

        // Follow control flow with path-sensitive assumptions
        match &block.terminator {
            Terminator::Branch {
                condition,
                then_block,
                else_block,
                invariants,
                variants,
                source_ref,
            } if invariants.is_empty() && variants.is_empty() => {
                // Variants & Invariants with len = 0 => Branch is if/else
                let cond_pred = self.operand_to_predicate(condition);

                self.env.add_assumption(cond_pred.clone());
                let then_result = self.generate_constraints_from_block(*then_block, cfg, visited);
                self.env.pop_assumption();
                then_result?; 

               
                self.env.add_assumption(cond_pred.negate());
                let else_result = self.generate_constraints_from_block(*else_block, cfg, visited);
                self.env.pop_assumption();
                else_result?; 
            }

            Terminator::Branch { condition, then_block, else_block, invariants, variants, source_ref } => {
                // Variants & Invariants with len > 0 => Branch is loop

                let entry_ctx = self.env.to_type_context();
    
                for inv in invariants {
                    self.constraints.add(Constraint::LoopInvariantEntry {
                        context: entry_ctx.clone(),
                        invariant: inv.clone(),
                        location: inv.source_ref().clone(),
                    });
                }
                
                for var in variants {
                    self.constraints.add(Constraint::LoopVariantNonNegative {
                        context: entry_ctx.clone(),
                        variant: var.clone(),
                        location: var.source_ref().clone(),
                    });
                }

                for inv in invariants {
                    self.env.add_assumption(inv.clone());
                }


                let cond_pred = self.operand_to_predicate(condition);

                self.env.add_assumption(cond_pred.clone());
                let then_result = self.generate_constraints_from_block(*then_block, cfg, visited);
                
                let preservation_ctx = self.env.to_type_context();

                for inv in invariants {
                    self.constraints.add(Constraint::LoopInvariantPreservation {
                        context: preservation_ctx.clone(),
                        invariant: inv.clone(),
                        location: inv.source_ref().clone(),
                    });
                }
                
                for var in variants {
                    self.constraints.add(Constraint::LoopVariantDecreases {
                        entry_context: entry_ctx.clone(),
                        preservation_context: preservation_ctx.clone(),
                        variant: var.clone(),
                        location: var.source_ref().clone(),
                    });
                }
                self.env.pop_assumption(); // clean condition
                for _ in invariants {
                    self.env.pop_assumption();
                }
                then_result?;

                self.env.add_assumption(cond_pred.negate());
                let else_result = self.generate_constraints_from_block(*else_block, cfg, visited);
                else_result?; 
            }

            Terminator::Jump { target } => {
                self.generate_constraints_from_block(*target, cfg, visited)?;
            }

            Terminator::Return { value, source_ref } => {
                if let Some(ret_operand) = value {
                    self.generate_return_constraint(ret_operand, block_id, cfg)?;
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
        cfg: &SsaCfg
    ) -> Result<(), MerakError> {
        let context = self.env.to_type_context();

        match instr {
            SsaInstruction::Copy {
                dest,
                source,
                source_ref,
            } => {
                let sub = self.operand_to_template(source, cfg);
                let sup = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (copy)");
                self.constraints.add(Constraint::Subtype {
                    context,
                    sub,
                    sup,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::Phi { dest, sources } => {
                let sup = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (phi)");

                for (_, source_reg) in sources {
                    // TODO: Ideally we'd use the environment from pred_block
                    // For now, use current context (conservative)
                    let sub = self
                        .env
                        .lookup(&source_reg.to_string())
                        .expect("Rompio en un source (phi)");

                    self.constraints.add(Constraint::Subtype {
                        context: context.clone(),
                        sub,
                        sup: sup.clone(),
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
                let dest_tmpl = self.env.get_local(&dest.to_string()).unwrap();
                let left_tmpl = self.operand_to_template(left, cfg);
                let right_tmpl = self.operand_to_template(right, cfg);

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
                let operand_tmpl = self.operand_to_template(operand, cfg);

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
                let sup = self
                    .env
                    .lookup(&dest.to_string())
                    .expect("Rompio en un dest (storageLoad)");

                let sub = self
                    .env
                    .get_symbol_template(*var)
                    .expect("Rompio en storagelod var");

                self.constraints.add(Constraint::Subtype {
                    context,
                    sub,
                    sup,
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::StorageStore {
                var,
                value,
                source_ref,
            } => {
                let sub = self.operand_to_template(value, cfg);
                let sup = self
                    .env
                    .get_symbol_template(*var)
                    .expect("Rompio en storageStore");

                // Constraint: value type <: storage type
                self.constraints.add(Constraint::Subtype {
                    context,
                    sub,
                    sup,
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
                        context: context.clone(),
                        template: dest_tmpl,
                        location: source_ref.clone(),
                    });
                }

                match target {
                    CallTarget::Internal(fn_id) => {
                        let fn_info = self.env.get_symbol(*fn_id);
                        let (parameters, return_type, ensures, requires) = match &fn_info.kind {
                            SymbolKind::Function { parameters, return_type, ensures, requires, .. } |
                            SymbolKind::Entrypoint { parameters, return_type, ensures, requires, .. } => {
                                (parameters, return_type, ensures, requires)
                            }
                            _ => panic!("Expected Function or Entrypoint symbol"),
                        };

                        let mut subst_map = HashMap::new();
                        for (param, arg) in parameters.iter().zip(args) {
                            match arg.symbol_id() {
                                Some(id) => {
                                    let arg_name = self.env.get_symbol(id).qualified_name.last();
                                    subst_map.insert(param.name.clone(), arg_name);
                                }
                                None => {}
                            }
                        }

                        for req in requires {
                            let substituted = req.substitute_vars(&subst_map);

                            self.constraints.add(Constraint::Requires {
                                context: context.clone(),
                                condition: substituted,
                                location: source_ref.clone(),
                            });
                        }

                        for (param, arg) in parameters.iter().zip(args) {
                            
                            let sup = match arg {
                                Operand::Register(reg) => {
                                    self.env.lookup(&param.name).expect("Arg template should exist")
                                }
                                Operand::Constant(cons) => {
                                    let (base, refinement) = match cons {
                                        Constant::Int(i) => (BaseType::Int, Predicate::BinRel { op: RelOp::Eq, lhs: RefinementExpr::Var("__self".to_string(), NodeId::new(0), SourceRef::unknown()), rhs: RefinementExpr::IntLit(*i, NodeId::new(0), SourceRef::unknown()), id: NodeId::new(0), source_ref: SourceRef::unknown() }),
                                        Constant::Bool(b) => (BaseType::Bool, if *b {Predicate::True(NodeId::new(0), SourceRef::unknown())} else {Predicate::False(NodeId::new(0), SourceRef::unknown())}),
                                        Constant::Address(a) => (BaseType::Address, Predicate::BinRel { op: RelOp::Eq, lhs: RefinementExpr::Var("__self".to_string(), NodeId::new(0), SourceRef::unknown()), rhs: RefinementExpr::AddressLit(a.to_string(), NodeId::new(0), SourceRef::unknown()), id: NodeId::new(0), source_ref: SourceRef::unknown() }),
                                        Constant::String(_) => (BaseType::String, Predicate::True(NodeId::new(0), SourceRef::unknown()))
                                    };
                                    
                                    Template::Concrete { base, binder: "__self".to_string(), refinement, source_ref: SourceRef::unknown() }
                                }
                            };

                            let sub = Template::Concrete {
                                base: param.ty.base.clone(),
                                binder: param.ty.binder.clone(),
                                refinement: param.ty.constraint.clone(),
                                source_ref: param.ty.source_ref.clone(),
                            };

                            self.constraints.add(Constraint::Subtype {
                                context: context.clone(),
                                sub,
                                sup: sup.clone(),
                                location: source_ref.clone(),
                            });
                        }

                        for post in ensures {
                            let substituted = post.substitute_vars(&subst_map);

                            self.constraints.add(Constraint::Ensures {
                                context: context.clone(),
                                condition: substituted,
                                location: source_ref.clone(),
                            });
                        }
                    }
                    
                    CallTarget::External { .. } => {
                        // External calls: no verificamos nada
                    }
                }
            }

            SsaInstruction::Assert {
                condition,
                kind,
                source_ref,
            } => {
                unimplemented!("User defined asserts not implemented yet.")
            }
            SsaInstruction::Fold { var, source_ref } => {
                let var_type = self.env.get_symbol_type(*var)
                    .expect("Storage var should have type");
                
                self.constraints.add(Constraint::Fold {
                    context,
                    var: *var,
                    refinement: var_type.constraint.clone(),
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::Unfold { var, source_ref } => {
                let constraint = self.env.get_symbol_type(*var)
                    .expect("Storage var should have type")
                    .constraint.clone();

                self.env.add_assumption(constraint.clone());

                // self.constraints.add(Constraint::Unfold {
                //     context,
                //     var: *var,
                //     refinement: constraint,
                //     location: source_ref.clone(),
                // });
            }
        }

        Ok(())
    }

    fn generate_return_constraint(
        &mut self,
        ret_operand: &Operand,
        block_id: usize,
        cfg: &SsaCfg
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

        let actual_template = self.operand_to_template(ret_operand, cfg);

        // Generate subtyping constraint: actual <: expected
        let ctx = self.env.to_type_context();
        self.constraints.add(Constraint::Subtype {
            context: ctx,
            sub: actual_template,
            sup: expected_template,
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
