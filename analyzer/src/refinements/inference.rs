use std::collections::{HashMap, HashSet};

use merak_ast::{NodeId, expression::{BinaryOperator, UnaryOperator}, meta::SourceRef, predicate::{ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp}, types::BaseType};
use merak_errors::MerakError;
use merak_ir::ssa_ir::{
    CallTarget, Constant, Operand, Register, SsaCfg, SsaInstruction, SsaOperand, Terminator
};
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};

use crate::refinements::{
        constraints::{Constraint, ConstraintSet, TypeContext}, environment::TypeEnvironment, qualifiers::QualifierSet, solver::ConstraintSolver, templates::Template
    };

pub struct LiquidInferenceEngine<'a> {
    // El environment maneja symbol table, liquid vars, assumptions
    env: TypeEnvironment<'a>,

    // Store de constraints acumulados
    constraints: ConstraintSet,

    // Set estático de qualifiers
    qualifiers: QualifierSet,

    // Tracks the last register associated with each state variable.
    // Used to emit equality assumptions connecting multiple loads/stores.
    storage_load_regs: HashMap<SymbolId, String>,


    branch_path_assumptions: HashMap<usize, Predicate>,
}

impl<'a> LiquidInferenceEngine<'a> {
    pub fn new(symbol_table: &'a mut SymbolTable) -> Self {
        Self {
            env: TypeEnvironment::new(symbol_table),
            constraints: ConstraintSet::new(),
            qualifiers: QualifierSet::core(),
            storage_load_regs: HashMap::new(),
            branch_path_assumptions: HashMap::new()
        }
    }

    /// Runs only the template assignment phase and returns the resulting bindings.
    /// Intended for testing template assignment in isolation.
    pub fn assign_templates_only(
        &mut self,
        cfg: &SsaCfg,
    ) -> Result<HashMap<String, Template>, MerakError> {
        self.env.enter_function(cfg.function_id.clone());
        self.storage_load_regs.clear();
        self.assign_templates(cfg)?;
        let bindings = self.env.local_bindings().clone();
        self.env.exit_function();
        Ok(bindings)
    }

    /// Runs template assignment followed by constraint generation, then returns
    /// the generated ConstraintSet. Does not invoke the solver.
    /// Intended for testing constraint generation in isolation.
    pub fn generate_constraints_only(
        &mut self,
        cfg: &SsaCfg,
    ) -> Result<ConstraintSet, MerakError> {
        self.env.enter_function(cfg.function_id.clone());
        self.storage_load_regs.clear();
        self.constraints = ConstraintSet::new();

        self.assign_templates(cfg)?;
        self.generate_constraints(cfg)?;

        let result = self.constraints.clone();
        self.env.exit_function();
        Ok(result)
    }

    pub fn infer_function(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        // Setup function context
        self.env.enter_function(cfg.function_id.clone());
        self.storage_load_regs.clear();

        // Extract constants from program and rebuild qualifiers
        let program_constants = Self::extract_constants_from_cfg(cfg);
        println!("[INFERENCE] Extracted {} constants from program: {:?}",
                 program_constants.len(), program_constants);
        self.qualifiers = QualifierSet::with_constants(program_constants);

        self.assign_templates(cfg)?;

        self.generate_constraints(cfg)?;

        self.solve_constraints()?;

        // Cleanup
        self.env.exit_function();

        Ok(())
    }

    /// Extract all integer constants that appear in the program
    fn extract_constants_from_cfg(cfg: &SsaCfg) -> Vec<i64> {
        let mut constants = HashSet::new();

        // Extract from instructions
        for block in cfg.blocks.values() {
            for instr in &block.instructions {
                Self::extract_constants_from_instruction(instr, &mut constants);
            }

            // Extract from terminators
            if let Terminator::Branch { condition, .. } = &block.terminator {
                if let Operand::Constant(Constant::Int(n)) = condition {
                    constants.insert(*n);
                }
            }
        }

        // Extract from function contracts (requires/ensures)
        for predicate in cfg.requires.iter().chain(cfg.ensures.iter()) {
            Self::extract_constants_from_predicate(predicate, &mut constants);
        }

        constants.into_iter().collect()
    }

    fn extract_constants_from_instruction(instr: &SsaInstruction, constants: &mut HashSet<i64>) {
        match instr {
            SsaInstruction::Copy { source, .. } => {
                if let Operand::Constant(Constant::Int(n)) = source {
                    constants.insert(*n);
                }
            }
            SsaInstruction::BinaryOp { left, right, .. } => {
                if let Operand::Constant(Constant::Int(n)) = left {
                    constants.insert(*n);
                }
                if let Operand::Constant(Constant::Int(n)) = right {
                    constants.insert(*n);
                }
            }
            SsaInstruction::UnaryOp { operand, .. } => {
                if let Operand::Constant(Constant::Int(n)) = operand {
                    constants.insert(*n);
                }
            }
            SsaInstruction::Call { args, .. } => {
                for arg in args {
                    if let Operand::Constant(Constant::Int(n)) = arg {
                        constants.insert(*n);
                    }
                }
            }
            SsaInstruction::StorageStore { value, .. } => {
                if let Operand::Constant(Constant::Int(n)) = value {
                    constants.insert(*n);
                }
            }
            SsaInstruction::Assert { condition, .. } => {
                if let Operand::Constant(Constant::Int(n)) = condition {
                    constants.insert(*n);
                }
            }
            _ => {}
        }
    }

    fn extract_constants_from_predicate(pred: &Predicate, constants: &mut HashSet<i64>) {
        match pred {
            Predicate::BinRel { lhs, rhs, .. } => {
                Self::extract_constants_from_expr(lhs, constants);
                Self::extract_constants_from_expr(rhs, constants);
            }
            Predicate::And(p1, p2, ..) | Predicate::Or(p1, p2, ..) | Predicate::Implies(p1, p2, ..) => {
                Self::extract_constants_from_predicate(p1, constants);
                Self::extract_constants_from_predicate(p2, constants);
            }
            Predicate::Not(p, ..) => {
                Self::extract_constants_from_predicate(p, constants);
            }
            _ => {}
        }
    }

    fn extract_constants_from_expr(expr: &RefinementExpr, constants: &mut HashSet<i64>) {
        match expr {
            RefinementExpr::IntLit(n, ..) => {
                constants.insert(*n);
            }
            RefinementExpr::BinOp { lhs, rhs, .. } => {
                Self::extract_constants_from_expr(lhs, constants);
                Self::extract_constants_from_expr(rhs, constants);
            }
            RefinementExpr::UnaryOp { expr, .. } => {
                Self::extract_constants_from_expr(expr, constants);
            }
            RefinementExpr::Old { expr, .. } => {
                Self::extract_constants_from_expr(expr, constants);
            }
            RefinementExpr::UninterpFn { args, .. } => {
                for arg in args {
                    Self::extract_constants_from_expr(arg, constants);
                }
            }
            _ => {}
        }
    }

    fn assign_templates(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        for param in &cfg.parameters {
            let param_reg = Register {
                symbol: param.clone(),
                version: 0,
            };

            let ty = self
                .env
                .get_symbol_type(&param_reg.symbol)
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
            let block = &cfg.blocks[&block_id];
            for instr in &block.instructions {
                if let Some(dest) = instr.dest_register() {
                    let template = match &dest.symbol {
                        SymbolId::Named(_, _) => {
                            let ty = self.env.get_symbol_type(&dest.symbol)
                                .ok_or_else(|| MerakError::NameResolution {
                                    message: format!("Named symbol {:?} not in symbol table", dest.symbol)
                                })?;

                            let symbol_info = self.env.get_symbol(&dest.symbol);
                            let base = ty.base.clone();
                            let binder = ty.binder.clone();
                            let source_ref = ty.source_ref.clone();
                            let refinement = ty.constraint.clone();

                            if matches!(symbol_info.kind, SymbolKind::Parameter) {
                                Template::Concrete {
                                    base,
                                    binder,
                                    refinement,
                                    source_ref,
                                }
                            } else if ty.is_explicit_annotation() && !ty.is_true_literal() {
                                Template::Concrete {
                                    base,
                                    binder,
                                    refinement,
                                    source_ref,
                                }
                            } else {
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
                            let mut t = self.infer_template_from_instruction(instr, cfg)?;
                            t.replace_binder(&dest.to_string());
                            t
                        }
                    };
                
                    self.env.bind_local(dest.to_string(), template);
                }
            }
        }

        Ok(())
    }


    fn infer_template_from_instruction(
        &mut self,
        instr: &SsaInstruction,
        cfg: &SsaCfg
    ) -> Result<Template, MerakError> {
        match instr {
            SsaInstruction::BinaryOp { left, op, right, dest, source_ref } => {
                println!(" Local temps: {:?}", cfg.local_temps);
                let base_type = cfg.local_temps.get(&dest.symbol)
                    .ok_or_else(|| MerakError::NameResolution { // TODO: Name resolution?
                        message: format!("Missing base type (binary) for {:?}", dest)
                    })?;
                
                self.infer_binary_op_template(left, op, right, base_type, source_ref)
            }
            
            SsaInstruction::UnaryOp { op, operand, dest, source_ref } => {
                let base_type = cfg.local_temps.get(&dest.symbol)
                    .ok_or_else(|| MerakError::NameResolution {
                        message: format!("Missing base type (unary) for {:?}", dest)
                    })?;
                
                self.infer_unary_op_template(op, operand, base_type, source_ref)
            }
            
            SsaInstruction::Copy { source, .. } => {
                Ok(self.operand_to_template(source))
            }
            
            SsaInstruction::StorageLoad { var, dest: _, source_ref } => {
                let ty = self.env.get_symbol_type(var)
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
            SsaInstruction::Phi { dest, .. } => {
                let base_type = cfg.local_temps.get(&dest.symbol)
                    .ok_or_else(|| MerakError::NameResolution {
                        message: format!("Missing base type (phi) for {:?}", dest)
                    })?;
                
                // Phi requiere inferencia - crear template líquido
                Ok(Template::Liquid {
                    base: base_type.clone(),
                    binder: "__self".to_string(),
                    liquid_var: self.env.fresh_liquid_var(),
                    source_ref: SourceRef::unknown(),
                })
            }
            
            SsaInstruction::Call { 
                dest: Some(_), 
                target: CallTarget::Internal(fn_id),
                ..
            } => {
                let fn_symbol = self.env.get_symbol(fn_id);
                
                let return_type = match &fn_symbol.kind {
                    SymbolKind::Function { return_type, .. } |
                    SymbolKind::Entrypoint { return_type, .. } => return_type.clone(),
                    _ => return Err(MerakError::NameResolution {
                        message: format!("Symbol {:?} is not a function", fn_id)
                    })
                };

                Ok(Template::from_type(&return_type, &mut self.env.liquid_gen))
            }
            SsaInstruction::Call { 
                dest: Some(_), 
                target: CallTarget::External { .. }, 
                source_ref, .. 
            } => {
                // Para external calls, buscar el return type en la interface
                // Por ahora, crear liquid genérico (esto necesita más info de interfaces)
                Ok(Template::Liquid {
                    base: BaseType::Int, // TODO: placeholder - necesita lookup real
                    binder:"__self".to_string(),
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
        &self,
        left: &SsaOperand,
        op: &BinaryOperator,
        right: &SsaOperand,
        base_type: &BaseType,
        source_ref: &SourceRef
    ) -> Result<Template, MerakError> {
        match op {
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
                    binder: "__self".to_string(),
                    refinement: Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: RefinementExpr::Var(
                            "__self".to_string(),
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
                    binder: "__self".to_string(),
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
                    binder: "__self".to_string(),
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
                    binder: "__self".to_string(),
                    refinement: logical_pred,
                    source_ref: source_ref.clone(),
                })
            }
        }
    }
    
    fn infer_unary_op_template(
        &self,
        op: &UnaryOperator,
        operand: &SsaOperand,
        base_type: &BaseType,
        source_ref: &SourceRef
    ) -> Result<Template, MerakError> {
        match op {
            UnaryOperator::Negate => {
                Ok(Template::Concrete {
                    base: base_type.clone(),
                    binder: "__self".to_string(),
                    refinement: Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: RefinementExpr::Var(
                            "__self".to_string(),
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
            
            UnaryOperator::Not => {
                let operand_pred = self.operand_to_predicate(operand);
                
                Ok(Template::Concrete {
                    base: BaseType::Bool,
                    binder: "__self".to_string(),
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
    
    fn operand_to_refinement_expr(&self, operand: &SsaOperand) -> RefinementExpr {
        match operand {
            Operand::Location(reg) => {
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
                        RefinementExpr::BoolLit(
                            *value,
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
                        // TODO: Strings no tienen representación en refinements
                        // Usar placeholder
                        RefinementExpr::IntLit(0, NodeId::new(0), SourceRef::unknown())
                    }
                }
            }
        }
    }

    fn operand_to_template(&self, operand: &SsaOperand) -> Template {
        println!("OPERAND TO TEMPLATE: {operand:?}");
        match operand {
            Operand::Location(reg) => self.env.get_local(&reg.to_string()).unwrap(),
            Operand::Constant(constant) => {
                match constant {
                    Constant::Int(value) => {
                        Template::Concrete {
                            base: BaseType::Int,
                            binder: "__self".to_string(),
                            refinement: Predicate::BinRel {
                                op: RelOp::Eq,
                                lhs: RefinementExpr::Var("__self".to_string(), NodeId::new(0), SourceRef::unknown()),
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
                            binder: "__self".to_string(),
                            refinement: Predicate::BinRel {
                                op: RelOp::Eq,
                                lhs: RefinementExpr::Var("__self".to_string(), NodeId::new(0), SourceRef::unknown()),
                                rhs: RefinementExpr::BoolLit(*value, NodeId::new(0), SourceRef::unknown()),
                                id: NodeId::new(0),
                                source_ref: SourceRef::unknown(),
                            },
                            source_ref: SourceRef::unknown(),
                        }
                    }
                    Constant::Address(addr) => {
                        Template::Concrete {
                            base: BaseType::Address,
                            binder: "__self".to_string(),
                            refinement: Predicate::BinRel {
                                op: RelOp::Eq,
                                lhs: RefinementExpr::Var("__self".to_string(), NodeId::new(0), SourceRef::unknown()),
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
                            binder: "__self".to_string(),
                            refinement: Predicate::True(NodeId::new(0), SourceRef::unknown()),
                            source_ref: SourceRef::unknown(),
                        }
                    }
                }
            }
        }
    }

    /// Transforma un operando a template, generando restricción de contrato si es necesario.
    ///
    /// Solo genera read checks para PARAMETERS, no para LocalVar.
    /// Para LocalVar, el contrato ya se verificó en la asignación (Copy destination check).
    fn operand_to_template_with_contract_check(
        &mut self,
        operand: &SsaOperand,
        context: &TypeContext,
        location: &SourceRef,
    ) -> Template {
        let template = self.operand_to_template(operand);

        // Solo generar read check para PARAMETERS, no para LocalVar
        if let Operand::Location(reg) = operand {
            if let SymbolId::Named(_, _) = &reg.symbol {
                // Check if it's a Parameter (not LocalVar)
                let symbol_info = self.env.get_symbol(&reg.symbol);
                if matches!(symbol_info.kind, SymbolKind::Parameter) {
                    if let Some(ty) = self.env.get_symbol_type(&reg.symbol) {
                        //  TODO: Needed? Related with storage maybe?
                        if template.is_concrete() && ty.is_explicit_annotation() && !ty.is_true_literal(){
                            // Generar restricción: Liquid <: Concrete (contrato del usuario)
                            let mut subst = std::collections::HashMap::new();
                            subst.insert(ty.binder.clone(), reg.to_string());

                            let user_contract = Template::Concrete {
                                base: ty.base.clone(),
                                binder: reg.to_string(),
                                refinement: ty.constraint.substitute_vars(&subst),
                                source_ref: ty.source_ref.clone(),
                            };

                            let sub = Constraint::Subtype {
                                context: context.clone(),
                                sub: template.clone(),
                                sup: user_contract,
                                location: location.clone(),
                            };
                            println!("[SUBTYPE] operand_to_template_with_contract_check: operand <: contract, {sub}");
                            self.constraints.add(sub);
                        }
                    }
                }
            }
        }

        template
    }

    pub fn generate_constraints(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        // Seed source-to-SSA mapping with parameters (version 0)
        for param in &cfg.parameters {
            if let SymbolId::Named(name, _) = param {
                let param_reg = Register { symbol: param.clone(), version: 0 };
                self.env.track_source_ssa(name.clone(), param_reg.to_string());
            }
        }

        // Validate: requires/ensures must reference storage state, not just parameters
        self.validate_storage_contracts(&cfg.requires, "requires")?;
        self.validate_storage_contracts(&cfg.ensures, "ensures")?;

        // Add requires as assumptions
        for req_pred in &cfg.requires {
            println!("Require: {req_pred}");
            let resolved = req_pred.substitute_vars(self.env.source_to_ssa_mapping());
            println!("Resolved require: {resolved}");
            self.env.add_assumption(resolved);
        }

        // Add parameter contracts as assumptions
        for param in &cfg.parameters {
            println!("Add parameter contracts as assumptions {param}");
            if let Some(ty) = self.env.get_symbol_type(param) {
                if ty.is_explicit_annotation() && !ty.is_true_literal() {
                    // Get the parameter name with SSA version
                    let param_name = self.env.get_symbol(param).qualified_name.last();
                    let param_versioned = format!("{}_0", param_name);

                    // Substitute binder with versioned name + resolve cross-refs
                    let mut subst = self.env.source_to_ssa_mapping().clone();
                    subst.insert(ty.binder.clone(), param_versioned);
                    println!("Subst: {subst:?}");

                    let param_contract = ty.constraint.substitute_vars(&subst);
                    self.env.add_assumption(param_contract);
                }
            }
        }

        let mut visited = HashSet::new();
        self.generate_constraints_from_block(cfg.entry, cfg, &mut visited)?;

        println!("[PHI_PASS] Starting second pass for Phi nodes");
        self.generate_phi_constraints(cfg)?;
        Ok(())
    }

    fn generate_phi_constraints(&mut self, cfg: &SsaCfg) -> Result<(), MerakError> {
        let context = self.env.to_type_context();

        for (_, block) in &cfg.blocks {
            for instr in &block.instructions {
                if let SsaInstruction::Phi { dest, sources } = instr {
                    let sup = self
                        .env
                        .get_local(&dest.to_string())
                        .expect("Rompio en un dest (phi)");

                    for (pred_block_id, source_reg) in sources {
                        let sub = self
                            .env
                            .get_local(&source_reg.to_string())
                            .expect("Rompio en un source (phi)");

                        let mut path_context = context.clone();
                        if let Some(path_assumption) = self.branch_path_assumptions.get(pred_block_id) {
                            println!("[PHI_PASS] Using assumption for bb{}: {}", pred_block_id, path_assumption);
                            path_context.assume(path_assumption.clone());
                        } else {
                            println!("[PHI_PASS] No assumption found for bb{}", pred_block_id);
                        }

                        println!("[PHI_PASS] Generating constraint: {} <: {} (from bb{})", sub, sup, pred_block_id);
                        self.constraints.add(Constraint::Subtype {
                            context: path_context,
                            sub,
                            sup: sup.clone(),
                            location: SourceRef::unknown(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Validate that requires/ensures predicates reference storage state.
    /// Pure parameter/local constraints should use parameter refinements instead.
    fn validate_storage_contracts(
        &self,
        predicates: &[Predicate],
        clause_kind: &str,
    ) -> Result<(), MerakError> {
        for pred in predicates {
            // old() expressions imply storage state → always valid
            if pred.contains_old() {
                continue;
            }
            let free_vars = pred.free_variables();
            let has_storage_ref = free_vars.iter().any(|var_name| {
                self.env.symbol_table().all_symbols().any(|(_, sym)| {
                    &sym.qualified_name.last() == var_name
                        && matches!(sym.kind, SymbolKind::StateVar | SymbolKind::StateConst)
                })
            });
            if !has_storage_ref {
                return Err(MerakError::SemanticError(format!(
                    "{} clause '{}' only references parameters/locals. \
                     Use parameter refinements instead (e.g., x: {{x: int | x > 0}})",
                    clause_kind, pred
                )));
            }
        }
        Ok(())
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

        // Save pre-phi SSA mapping for loop entry checks
        let pre_phi_ssa_map = self.env.source_to_ssa_mapping().clone();

        // Generate constraints for instructions
        for instr in &block.instructions {
            // Track source-to-SSA mapping for named registers at current program point
            if let Some(dest) = instr.dest_register() {
                if let SymbolId::Named(name, _) = &dest.symbol {
                    println!("Tracking source ssa instr: {name} {}", dest.to_string());
                    self.env.track_source_ssa(name.clone(), dest.to_string());
                }
            }
            self.generate_constraints_instruction(instr)?;
        }

        // Follow control flow with path-sensitive assumptions
        match &block.terminator {
            Terminator::Branch {
                condition,
                then_block,
                else_block,
                meta,
            } if meta.loop_invariants.is_empty() && meta.loop_variants.is_empty() => {
                // Variants & Invariants with len = 0 => Branch is if/else
                let context = self.env.to_type_context();
                let cond_temp = self.operand_to_template_with_contract_check(
                    condition, &context, &meta.source_ref
                );

                // Extraer el predicado de la condición
                // Si tiene refinement (e.g., comparisons), usarlo
                // Si no (e.g., bool params), crear predicado simple con el nombre
                let cond_pred = match cond_temp.refinement() {
                    Some(refinement) => refinement.clone(),
                    None => {
                        // Liquid variable sin refinement (e.g., bool parameter)
                        // Usar el operand directamente como predicado
                        self.operand_to_predicate(condition)
                    }
                };

                let saved_storage = self.storage_load_regs.clone();

                self.env.add_assumption(cond_pred.clone());
                println!("[PATH_ASSUMPTION] Capturing for bb{}: {}", then_block, cond_pred);
                self.branch_path_assumptions.insert(*then_block, cond_pred.clone());
                let then_result = self.generate_constraints_from_block(*then_block, cfg, visited);
                self.env.pop_assumption();
                then_result?;

                self.storage_load_regs = saved_storage;

                self.env.add_assumption(cond_pred.negate());
                println!("[PATH_ASSUMPTION] Capturing for bb{}: {}", else_block, cond_pred.negate());
                self.branch_path_assumptions.insert(*else_block, cond_pred.negate());
                let else_result = self.generate_constraints_from_block(*else_block, cfg, visited);
                self.env.pop_assumption();
                else_result?; 
            }

            Terminator::Branch { condition, then_block, else_block, meta } => {
                // Variants & Invariants with len > 0 => Branch is loop

                // Use pre-phi SSA mapping for entry checks (maps to pre-loop values like i_0)
                // and post-phi mapping for invariant assumptions (maps to phi values like i_1)
                let post_phi_ssa_map = self.env.source_to_ssa_mapping().clone();
                let entry_ctx = self.env.to_type_context();

                // Entry constraints: resolve source names to pre-phi SSA names
                for inv in &meta.loop_invariants {
                    let resolved_inv = inv.substitute_vars(&pre_phi_ssa_map);
                    self.constraints.add(Constraint::LoopInvariantEntry {
                        context: entry_ctx.clone(),
                        invariant: resolved_inv,
                        location: inv.source_ref().clone(),
                    });
                }

                for var in &meta.loop_variants {
                    let resolved_var = var.substitute_vars(&pre_phi_ssa_map);
                    self.constraints.add(Constraint::LoopVariantNonNegative {
                        context: entry_ctx.clone(),
                        variant: resolved_var,
                        location: var.source_ref().clone(),
                    });
                }

                // Add resolved invariants as assumptions using post-phi names (i_1)
                for inv in &meta.loop_invariants {
                    self.env.add_assumption(inv.substitute_vars(&post_phi_ssa_map));
                }

                let _ = self.operand_to_template_with_contract_check(
                    condition, &entry_ctx, &meta.source_ref
                );
                let cond_pred = self.operand_to_predicate(condition);

                let saved_storage = self.storage_load_regs.clone();

                self.env.add_assumption(cond_pred.clone());
                let then_result = self.generate_constraints_from_block(*then_block, cfg, visited);

                // Snapshot source-to-SSA mapping after body (preservation)
                let preservation_ssa_map = self.env.source_to_ssa_mapping().clone();
                let preservation_ctx = self.env.to_type_context();

                for inv in &meta.loop_invariants {
                    let resolved_inv = inv.substitute_vars(&preservation_ssa_map);
                    self.constraints.add(Constraint::LoopInvariantPreservation {
                        context: preservation_ctx.clone(),
                        invariant: resolved_inv,
                        location: inv.source_ref().clone(),
                    });
                }

                // Variant decreasing: compare post-phi SSA names (i_1) vs post-body SSA names (i_2)
                for var in &meta.loop_variants {
                    self.constraints.add(Constraint::LoopVariantDecreases {
                        context: preservation_ctx.clone(),
                        variant_before: var.substitute_vars(&post_phi_ssa_map),
                        variant_after: var.substitute_vars(&preservation_ssa_map),
                        location: var.source_ref().clone(),
                    });
                }
                self.env.pop_assumption(); // clean condition
                for _ in &meta.loop_invariants {
                    self.env.pop_assumption();
                }
                then_result?;

                self.storage_load_regs = saved_storage;

                self.env.add_assumption(cond_pred.negate());
                let else_result = self.generate_constraints_from_block(*else_block, cfg, visited);
                else_result?; 
            }

            Terminator::Jump { target } => {
                self.generate_constraints_from_block(*target, cfg, visited)?;
            }

            Terminator::Return { value, meta: _ } => {
                if let Some(ret_operand) = value {
                    self.generate_return_constraint(ret_operand)?;
                }

                for postcond in &cfg.ensures {
                    let ctx = self.env.to_type_context();
                    let ssa_map = self.env.source_to_ssa_mapping().clone();
                    let resolved = postcond.substitute_vars(&ssa_map);
                    self.constraints.add(Constraint::Ensures {
                        context: ctx,
                        condition: resolved,
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
    ) -> Result<(), MerakError> {
        let context = self.env.to_type_context();
        //println!("Type context: {:?}", context);

        println!("Procesando instruccion: {instr:?}");

        match instr {
            SsaInstruction::Copy {
                dest,
                source,
                source_ref,
            } => {
                // Low-Level Liquid Types: verificamos contrato al LEER source
                let sub = self.operand_to_template_with_contract_check(
                    source, &context, source_ref
                );
                let mut sup = self
                    .env
                    .get_local(&dest.to_string())
                    .expect("Rompio en un dest (copy)");

                let ssa_map = self.env.source_to_ssa_mapping().clone();
                println!("Subs source ssa copy: {ssa_map:?}");
                sup.resolve_cross_refs(&ssa_map);
                //TODO ??? sub.resolve_cross_refs(&ssa_map);


                // Flujo de datos: source <: dest (Liquid <: Liquid)
                println!("[SUBTYPE] generate_constraints_instruction/Copy: source <: dest (Liquid <: Liquid), sub={}, sup={}", sub, sup);
                self.constraints.add(Constraint::Subtype {
                    context: context.clone(),
                    sub: sub.clone(),
                    sup,
                    location: source_ref.clone(),
                });

                // Add equality fact: dest = source (for Z3 reasoning about chains like i_2 = __temp_1_1)
                let dest_expr = RefinementExpr::Var(dest.to_string(), NodeId::new(0), SourceRef::unknown());
                let source_expr = operand_to_refinement_expr(source);
                self.env.add_local_fact(Predicate::BinRel {
                    op: RelOp::Eq,
                    lhs: dest_expr,
                    rhs: source_expr,
                    id: NodeId::new(0),
                    source_ref: source_ref.clone(),
                });

                // Si dest es Named con contrato, verificar: source <: contrato(dest)
                if let SymbolId::Named(_, _) = &dest.symbol {
                    if let Some(ty) = self.env.get_symbol_type(&dest.symbol) {
                        if ty.is_explicit_annotation() && !ty.is_true_literal() {
                            let mut subst = std::collections::HashMap::new();
                            subst.insert(ty.binder.clone(), dest.to_string());

                            println!("BINDER Y DEST: {}, {}",ty.binder.clone(), dest.to_string());

                            let mut user_contract = Template::Concrete {
                                base: ty.base.clone(),
                                binder: dest.to_string(),
                                refinement: ty.constraint.substitute_vars(&subst),
                                source_ref: ty.source_ref.clone(),
                            };
                            println!("USER CONTRACT ORIGINAL: {user_contract}");

                            // Resolve cross-variable references: source names → SSA names
                            // e.g., {g: int | g > a} becomes {g: int | g > a_0}
                            //let ssa_map = self.env.source_to_ssa_mapping().clone();
                            user_contract.resolve_cross_refs(&ssa_map);

                            println!("SUB: {}", sub.clone());
                            println!("USER CONSTRACT RESOLVED: {}", user_contract);

                            println!("[SUBTYPE] generate_constraints_instruction/Copy(contract_dest): source <: contrato(dest), sub={}, sup={}", sub, user_contract);
                            self.constraints.add(Constraint::Subtype {
                                context,
                                sub,  // El valor que estamos asignando
                                sup: user_contract,  // El contrato del usuario
                                location: source_ref.clone(),
                            });
                        }
                    }
                }
            }
            SsaInstruction::Phi { dest: _, sources: _ } => {
                // Phi constraints are generated in a second pass after all branch
                // assumptions have been captured (see generate_phi_constraints)
            }
            SsaInstruction::BinaryOp {
                dest,
                op,
                left,
                right,
                source_ref,
            } => {
                let dest_tmpl = self.env.get_local(&dest.to_string()).unwrap();
                let left_tmpl = self.operand_to_template_with_contract_check(
                    left, &context, source_ref
                );
                println!("Left_tmpl: {left_tmpl}");
                let right_tmpl = self.operand_to_template_with_contract_check(
                    right, &context, source_ref
                );
                println!("Right_tmpl: {right_tmpl}");

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

                // Add equality fact: dest = left op right (for arithmetic ops only)
                if let Some(arith_op) = binary_op_to_arith(op) {
                    let dest_expr = RefinementExpr::Var(dest.to_string(), NodeId::new(0), SourceRef::unknown());
                    let left_expr = operand_to_refinement_expr(left);
                    let right_expr = operand_to_refinement_expr(right);
                    self.env.add_local_fact(Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: dest_expr,
                        rhs: RefinementExpr::BinOp {
                            op: arith_op,
                            lhs: Box::new(left_expr),
                            rhs: Box::new(right_expr),
                            id: NodeId::new(0),
                            source_ref: SourceRef::unknown(),
                        },
                        id: NodeId::new(0),
                        source_ref: source_ref.clone(),
                    });
                }

                // Add biconditional fact for comparison ops: dest ↔ (left rel right)
                // Connects boolean condition variables to actual comparisons for branch reasoning
                if let Some(rel_op) = binary_op_to_rel(op) {
                    let dest_var = Predicate::Var(dest.to_string(), NodeId::new(0), SourceRef::unknown());
                    let comparison = Predicate::BinRel {
                        op: rel_op,
                        lhs: operand_to_refinement_expr(left),
                        rhs: operand_to_refinement_expr(right),
                        id: NodeId::new(0),
                        source_ref: SourceRef::unknown(),
                    };
                    let forward = Predicate::Implies(
                        Box::new(dest_var.clone()),
                        Box::new(comparison.clone()),
                        NodeId::new(0),
                        SourceRef::unknown(),
                    );
                    let backward = Predicate::Implies(
                        Box::new(comparison),
                        Box::new(dest_var),
                        NodeId::new(0),
                        SourceRef::unknown(),
                    );
                    self.env.add_local_fact(Predicate::And(
                        Box::new(forward),
                        Box::new(backward),
                        NodeId::new(0),
                        SourceRef::unknown(),
                    ));
                }
            }
            SsaInstruction::UnaryOp {
                dest,
                op,
                operand,
                source_ref,
            } => {
                let dest_tmpl = self
                    .env
                    .get_local(&dest.to_string())
                    .expect("Rompio en un dest (unry)");
                let operand_tmpl = self.operand_to_template_with_contract_check(
                    operand, &context, source_ref
                );

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
                    .get_local(&dest.to_string())
                    .expect("Rompio en un dest (storageLoad)");

                let sub = self
                    .env
                    .get_symbol_template(var)
                    .expect("Rompio en storagelod var");

                println!("[SUBTYPE] generate_constraints_instruction/StorageLoad: var <: dest, sub={}, sup={}", sub, sup);
                self.constraints.add(Constraint::Subtype {
                    context,
                    sub,
                    sup,
                    location: source_ref.clone(),
                });

                // Connect to previous load/store of same state variable
                if let Some(prev_name) = self.storage_load_regs.get(var) {
                    let binrel = Predicate::BinRel {
                        op: RelOp::Eq,
                        lhs: RefinementExpr::Var(dest.to_string(), NodeId::new(0), SourceRef::unknown()),
                        rhs: RefinementExpr::Var(prev_name.clone(), NodeId::new(0), SourceRef::unknown()),
                        id: NodeId::new(0),
                        source_ref: source_ref.clone(),
                    };
                    self.env.add_assumption(binrel);
                }
                self.storage_load_regs.insert(var.clone(), dest.to_string());
            }
            SsaInstruction::StorageStore {
                var,
                value,
                source_ref,
            } => {
                let sub = self.operand_to_template_with_contract_check(
                    value, &context, source_ref
                );
                let sup = self
                    .env
                    .get_symbol_template(var)
                    .expect("Rompio en storageStore");

                // Constraint: value type <: storage type
                println!("[SUBTYPE] generate_constraints_instruction/StorageStore: value <: storage, sub={}, sup={}", sub, sup);
                self.constraints.add(Constraint::Subtype {
                    context,
                    sub,
                    sup,
                    location: source_ref.clone(),
                });

                // Update tracker: next load of this var connects to the stored value
                match value {
                    Operand::Location(reg) => {
                        self.storage_load_regs.insert(var.clone(), reg.to_string());
                    }
                    _ => {
                        self.storage_load_regs.remove(var);
                    }
                }
            }
            SsaInstruction::Call {
                dest,
                target,
                args,
                source_ref,
            } => {
                match target {
                    CallTarget::Internal(fn_id) => {
                        let fn_kind = self.env.get_symbol(fn_id).kind.clone();
                        let (parameters, ensures, requires) = match fn_kind {
                            SymbolKind::Function { parameters, ensures, requires, .. } |
                            SymbolKind::Entrypoint { parameters, ensures, requires, .. } => {
                                (parameters, ensures, requires)
                            }
                            _ => panic!("Expected Function or Entrypoint symbol"),
                        };

                        // Create substitution map: formal params -> actual args
                        let mut subst_map: HashMap<String, RefinementExpr> = HashMap::new();
                        for (param, arg) in parameters.iter().zip(args) {
                            let expr = operand_to_refinement_expr(arg);
                            subst_map.insert(param.name.clone(), expr);
                        }

                        // Substitute parameters in return type template (if destination exists)
                        if let Some(dest_reg) = dest {
                            let dest_tmpl = self
                                .env
                                .get_local(&dest_reg.to_string())
                                .expect("Rompio el call");

                            // Apply parameter substitution to the template's refinement
                            let substituted_tmpl = match dest_tmpl {
                                Template::Concrete { base, binder, refinement, source_ref } => {
                                    let substituted_refinement = refinement.substitute_exprs(&subst_map);
                                    Template::Concrete {
                                        base,
                                        binder,
                                        refinement: substituted_refinement,
                                        source_ref,
                                    }
                                }
                                _ => dest_tmpl.clone(),
                            };

                            // Update the binding with the substituted template
                            self.env.bind_local(dest_reg.to_string(), substituted_tmpl.clone());

                            // Check well-formedness with the substituted template
                            self.constraints.add(Constraint::WellFormed {
                                context: context.clone(),
                                template: substituted_tmpl,
                                location: source_ref.clone(),
                            });
                        }

                        for req in requires {
                            let substituted = req.substitute_exprs(&subst_map);

                            self.constraints.add(Constraint::Requires {
                                context: context.clone(),
                                condition: substituted,
                                location: source_ref.clone(),
                            });
                        }

                        for post in ensures {
                            let substituted = post.substitute_exprs(&subst_map);

                            self.constraints.add(Constraint::Ensures {
                                context: context.clone(),
                                condition: substituted,
                                location: source_ref.clone(),
                            });
                        }

                        for (param, arg) in parameters.iter().zip(args) {
                            let sub = self.operand_to_template_with_contract_check(
                                arg, &context, source_ref
                            );

                            let sup = Template::Concrete {
                                base: param.ty.base.clone(),
                                binder: param.ty.binder.clone(),
                                refinement: param.ty.constraint.clone(),
                                source_ref: param.ty.source_ref.clone(),
                            };

                            println!("[SUBTYPE] generate_constraints_instruction/Call: arg <: param, sub={}, sup={}", sub, sup);
                            self.constraints.add(Constraint::Subtype {
                                context: context.clone(),
                                sub,
                                sup: sup.clone(),
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
                condition: _,
                kind: _,
                source_ref: _,
            } => {
                unimplemented!("User defined asserts not implemented yet.")
            }
            SsaInstruction::Fold { var, source_ref } => {
                let var_type = self.env.get_symbol_type(var)
                    .expect("Storage var should have type");
                
                self.constraints.add(Constraint::Fold {
                    context,
                    var: var.clone(),
                    refinement: var_type.constraint.clone(),
                    location: source_ref.clone(),
                });
            }
            SsaInstruction::Unfold { var, source_ref: _ } => {
                let constraint = self.env.get_symbol_type(var)
                    .expect("Storage var should have type")
                    .constraint.clone();

                self.env.add_assumption(constraint.clone());
            }
        }

        Ok(())
    }

    fn generate_return_constraint(
        &mut self,
        ret_operand: &SsaOperand,
    ) -> Result<(), MerakError> {
        let func_id = self.env.current_function().unwrap();

        let func_symbol = self.env.symbol_table().get_symbol(&func_id);

        let declared_return_type = match &func_symbol.kind {
            SymbolKind::Function { return_type, .. } => return_type.clone(), // TODO: Clone
            _ => {
                panic!("Current function is not a function symbol")
            }
        };

        let mut expected_template =
            Template::from_type(&declared_return_type, &mut self.env.liquid_gen);

        // Resolve cross-variable references: source names → SSA names
        // e.g., {v: int | v > x} becomes {v: int | v > x_0}
        let ssa_map = self.env.source_to_ssa_mapping().clone();
        expected_template.resolve_cross_refs(&ssa_map);

        // Low-Level Liquid Types: verificamos contrato al LEER el valor de retorno
        let ctx = self.env.to_type_context();
        let actual_template = self.operand_to_template_with_contract_check(
            ret_operand, &ctx, &SourceRef::unknown()
        );

        // Generate subtyping constraint: actual <: expected
        println!("[SUBTYPE] generate_return_constraint: actual <: expected, sub={}, sup={}", actual_template, expected_template);
        self.constraints.add(Constraint::Subtype {
            context: ctx,
            sub: actual_template,
            sup: expected_template,
            location: SourceRef::unknown(), //ret_operand.source_ref(),
        });

        Ok(())
    }

    fn operand_to_predicate(&self, operand: &SsaOperand) -> Predicate {
        match operand {
            Operand::Location(register) => {
                // A register used as a predicate represents a boolean variable
                Predicate::Var(
                    register.to_string(),
                    NodeId::new(0),
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
        println!("[INFERENCE] Passing {} constraints to solver", self.constraints.len());
        let z3_context = z3::Context::new(&z3::Config::new());
        let solver = ConstraintSolver::new(self.constraints.clone(), &z3_context);

        let _ = solver
            .solve()
            .map_err(|e| MerakError::ConstraintSolvingFailed {
                message: format!("{:?}", e),
            })?;

        Ok(())
    }
}

/// Convert an SSA Operand to a RefinementExpr for use in equality facts
fn operand_to_refinement_expr(operand: &SsaOperand) -> RefinementExpr {
    match operand {
        Operand::Location(reg) => {
            RefinementExpr::Var(reg.to_string(), NodeId::new(0), SourceRef::unknown())
        }
        Operand::Constant(c) => match c {
            Constant::Int(n) => RefinementExpr::IntLit(*n, NodeId::new(0), SourceRef::unknown()),
            Constant::Bool(b) => RefinementExpr::BoolLit(*b, NodeId::new(0), SourceRef::unknown()),
            Constant::Address(addr) => {
                RefinementExpr::AddressLit(format!("{:?}", addr), NodeId::new(0), SourceRef::unknown())
            }
            Constant::String(_) => {
                // Strings aren't representable in refinement expressions; use a placeholder
                RefinementExpr::IntLit(0, NodeId::new(0), SourceRef::unknown())
            }
        },
    }
}

/// Convert a BinaryOperator to an ArithOp, returning None for non-arithmetic ops
fn binary_op_to_arith(op: &BinaryOperator) -> Option<ArithOp> {
    match op {
        BinaryOperator::Add => Some(ArithOp::Add),
        BinaryOperator::Subtract => Some(ArithOp::Sub),
        BinaryOperator::Multiply => Some(ArithOp::Mul),
        BinaryOperator::Divide => Some(ArithOp::Div),
        BinaryOperator::Modulo => Some(ArithOp::Mod),
        _ => None,
    }
}

/// Convert a BinaryOperator to a RelOp, returning None for non-comparison ops
fn binary_op_to_rel(op: &BinaryOperator) -> Option<RelOp> {
    match op {
        BinaryOperator::Equal => Some(RelOp::Eq),
        BinaryOperator::NotEqual => Some(RelOp::Neq),
        BinaryOperator::Less => Some(RelOp::Lt),
        BinaryOperator::LessEqual => Some(RelOp::Leq),
        BinaryOperator::Greater => Some(RelOp::Gt),
        BinaryOperator::GreaterEqual => Some(RelOp::Geq),
        _ => None,
    }
}
