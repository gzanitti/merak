use std::cell::Cell;

use indexmap::IndexMap;
use merak_ast::{
    contract::{Contract, File, Program},
    expression::{Expression, Literal},
    function::Function,
    statement::{Block, Statement},
    types::BaseType,
};
use merak_errors::MerakError;
use merak_symbols::{SymbolId, SymbolKind, SymbolTable};

use crate::ssa_ir::{
    BlockId, CallTarget, Constant, Operand, Register, SsaCfg, SsaContract, SsaFile, SsaInstruction, SsaProgram, Terminator
};

pub struct SsaBuilder {
    symbol_table: SymbolTable,
    current_block: BlockId,
    next_temp_id: Cell<usize>,
}

// TODO: For now we use a simple `SsaBuilder` to handle the transformation directly.
// Later this will evolve into a pass-based architecture (e.g. AST → SSA, AST → ANF),
// where each transformation is an independent pass sharing a common context.
impl SsaBuilder {
    pub fn new(symbol_table: SymbolTable) -> Self {
        Self {
            symbol_table,
            current_block: 0,
            next_temp_id: Cell::new(0),
        }
    }

    fn new_temp_register(&self) -> Register {
        let temp_id = self.next_temp_id.get();
        self.next_temp_id.set(temp_id + 1);

        Register {
            symbol: SymbolId::synthetic_temp(temp_id),
            version: 0,
        }
    }

    pub fn build(&mut self, program: &Program) -> Result<SsaProgram, MerakError> {
        let mut ssa_program = SsaProgram {
            files: IndexMap::new(),
        };

        for contract_data in program.files.iter() {
            self.build_file(contract_data, &mut ssa_program)?;
        }

        Ok(ssa_program)
    }

    fn build_file(
        &mut self,
        contract_data: (&String, &File),
        ssa_program: &mut SsaProgram,
    ) -> Result<(), MerakError> {
        let (contract_name, file) = contract_data;

        let ssa_contract = self.build_contract(&file.contract)?;

        let ssa_file = SsaFile {
            imports: file.imports.clone(),
            interfaces: file.interfaces.clone(),
            contract: ssa_contract,
        };

        ssa_program
            .files
            .insert(contract_name.clone(), ssa_file);

        Ok(())
    }

    fn build_contract(&mut self, contract: &Contract) -> Result<SsaContract, MerakError> {
        let constructor_cfg = if let Some(constructor) = &contract.constructor {
            let constructor_id = self
                .symbol_table
                .get_symbol_id_by_node_id(constructor.id())
                .expect("Constructor should be defined");

            let mut ssa_cfg = SsaCfg::new("constructor".to_string(), constructor_id);

            constructor.params.iter().for_each(|param| {
                ssa_cfg.add_param(
                    self.symbol_table
                        .get_symbol_id_by_node_id(param.id)
                        .expect("Constructor param should be there!"),
                )
            });

            let entry_block = ssa_cfg.new_block();
            ssa_cfg.entry = entry_block;
            self.current_block = entry_block;

            self.transform_block(&constructor.body, &mut ssa_cfg)?;
            ssa_cfg.compute_dominance();
            ssa_cfg.build_loop_forest();
            ssa_cfg.insert_phi_nodes_and_rename();
            Some(ssa_cfg)
        } else {
            None
        };

        let mut functions = vec![];
        for function in contract.functions.iter() {
            let ssa_function = self.build_function(function)?;
            functions.push(ssa_function);
        }

        Ok(SsaContract {
            name: contract.name.clone(),
            variables: contract.variables.clone(),
            constants: contract.constants.clone(),
            constructor: constructor_cfg,
            functions,
        })
    }

    // fn build_state_def(&mut self, state_def: &StateDef) -> Result<SsaStateDef, MerakError> {
    //     let mut ssa_functions = vec![];
    //     for function in &state_def.functions {
    //         let ssa_function = self.build_function(function)?;
    //         ssa_functions.push(ssa_function);
    //     }

    //     Ok(SsaStateDef {
    //         contract: state_def.contract.clone(),
    //         name: state_def.name.clone(),
    //         owner: state_def.owner.clone(),
    //         functions: ssa_functions,
    //         source_ref: state_def.source_ref.clone(),
    //     })
    // }

    fn build_function(&mut self, function: &Function) -> Result<SsaCfg, MerakError> {
        let function_id = self
            .symbol_table
            .get_symbol_id_by_node_id(function.id())
            .expect("Function should be defined");
        let mut ssa_cfg = SsaCfg::new(function.name.clone(), function_id);

        function.params.iter().for_each(|param| {
            ssa_cfg.add_param(
                self.symbol_table
                    .get_symbol_id_by_node_id(param.id)
                    .expect("Param should be there!"),
            )
        });

        ssa_cfg.requires = function.requires.clone();
        ssa_cfg.ensures = function.ensures.clone();

        let entry_block = ssa_cfg.new_block();
        ssa_cfg.entry = entry_block;

        self.current_block = entry_block;

        self.transform_block(&function.body, &mut ssa_cfg)?;
        ssa_cfg.compute_dominance();
        ssa_cfg.build_loop_forest();
        ssa_cfg.insert_phi_nodes_and_rename();

        Ok(ssa_cfg)
    }

    fn transform_block(&mut self, block: &Block, ssa_cfg: &mut SsaCfg) -> Result<(), MerakError> {
        for statement in &block.statements {
            self.transform_statement(statement, ssa_cfg)?;
        }

        Ok(())
    }

    fn transform_statement(
        &mut self,
        statement: &Statement,
        ssa_cfg: &mut SsaCfg,
    ) -> Result<(), MerakError> {
        match statement {
            Statement::Expression(expr, _, _) => {
                self.transform_expression(expr, ssa_cfg);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                id: _,
                source_ref,
            } => {
                let header_bb = self.current_block;
                let then_bb = ssa_cfg.new_block();
                let (else_bb, exit_bb) = if else_block.is_some() {
                    let else_bb = ssa_cfg.new_block();
                    let exit_bb = ssa_cfg.new_block();
                    (else_bb, exit_bb)
                } else {
                    let exit_bb = ssa_cfg.new_block();
                    (exit_bb, exit_bb)
                };

                // ssa_cfg
                //     .add_terminator_at(self.current_block, Terminator::Jump { target: header_bb });
                // ssa_cfg.add_edge(self.current_block, header_bb);
                //self.current_block = header_bb;

                let cond_operand = self
                    .transform_expression(condition, ssa_cfg)
                    .expect("condition in if statement cannot be void");
                ssa_cfg.add_terminator_at(
                    header_bb,
                    Terminator::Branch {
                        condition: cond_operand,
                        then_block: then_bb,
                        else_block: else_bb,
                        invariants: vec![], // empty for ifs, >= 1 for loops
                        variants: vec![], // empty for ifs, >= 1 for loops
                        source_ref: source_ref.clone(),
                    },
                );

                ssa_cfg.add_edge(header_bb, then_bb);
                ssa_cfg.add_edge(header_bb, else_bb);

                self.current_block = then_bb;

                self.transform_block(then_block, ssa_cfg)?;
                ssa_cfg.add_edge(self.current_block, exit_bb);
                if matches!(ssa_cfg.blocks.get(&self.current_block).unwrap().terminator, Terminator::Unreachable) {
                    ssa_cfg.add_terminator_at(self.current_block, Terminator::Jump { target: exit_bb });
                }
                //ssa_cfg.add_terminator_at(then_bb, Terminator::Jump { target: exit_bb });

                if let Some(else_block) = else_block {
                    self.current_block = else_bb;
                    self.transform_block(else_block, ssa_cfg)?;
                    ssa_cfg.add_edge(self.current_block, exit_bb);
                    if matches!(ssa_cfg.blocks.get(&self.current_block).unwrap().terminator, Terminator::Unreachable) {
                        ssa_cfg.add_terminator_at(self.current_block, Terminator::Jump { target: exit_bb });
                    }
                }

                self.current_block = exit_bb;
            }
            Statement::While {
                condition,
                invariants,
                variants,
                body,
                id: _,
                source_ref,
            } => {
                let header_bb = ssa_cfg.new_block();
                let body_bb = ssa_cfg.new_block();
                let exit_bb = ssa_cfg.new_block();

                ssa_cfg
                    .add_terminator_at(self.current_block, Terminator::Jump { target: header_bb });
                ssa_cfg.add_edge(self.current_block, header_bb); // TODO: Info already on Jump, maybe redundant
                self.current_block = header_bb;

                ssa_cfg.blocks.get_mut(&header_bb).unwrap().loop_invariants =
                    Some(invariants.clone());
                ssa_cfg.blocks.get_mut(&header_bb).unwrap().loop_variants = Some(variants.clone());

                let cond_operand = self
                    .transform_expression(condition, ssa_cfg)
                    .expect("condition in while statement cannot be void");
                ssa_cfg.add_terminator_at(
                    header_bb,
                    Terminator::Branch {
                        condition: cond_operand,
                        then_block: body_bb,
                        else_block: exit_bb,
                        invariants: invariants.to_vec(),
                        variants: variants.to_vec(),
                        source_ref: source_ref.clone(),
                    },
                );

                ssa_cfg.add_edge(header_bb, body_bb);
                ssa_cfg.add_edge(body_bb, header_bb);
                self.current_block = body_bb;

                self.transform_block(body, ssa_cfg)?;
                ssa_cfg.add_terminator_at(body_bb, Terminator::Jump { target: header_bb });

                ssa_cfg.add_edge(header_bb, exit_bb);
                self.current_block = exit_bb;
            }
            Statement::Return(expression, _, source_ref) => {
                let return_value = expression
                    .as_ref()
                    .and_then(|expr| self.transform_expression(expr, ssa_cfg));

                let terminator = Terminator::Return {
                    value: return_value,
                    source_ref: source_ref.clone(),
                };

                ssa_cfg.add_terminator_at(self.current_block, terminator);
            }
            Statement::Assignment {
                target: _,
                expr,
                id,
                source_ref,
            } => {
                let symbol_info = self.symbol_table.get_symbol_by_node_id(*id).unwrap();
                let value = self
                    .transform_expression(expr, ssa_cfg)
                    .expect("cannot assign void value");
                let symbol = self.symbol_table.get_symbol_id_by_node_id(*id).unwrap();
                match symbol_info.kind {
                    SymbolKind::StateVar | SymbolKind::StateConst => {
                        ssa_cfg.add_instruction_at(self.current_block, SsaInstruction::StorageStore {
                            var: symbol,
                            value,
                            source_ref: source_ref.clone(),
                        });
                    }
                    SymbolKind::LocalVar | SymbolKind::Parameter => {
                        let dest = Register { symbol, version: 0 };

                        ssa_cfg.add_instruction_at(self.current_block, SsaInstruction::Copy {
                            dest,
                            source: value,
                            source_ref: source_ref.clone(),
                        });
                    }
                    _ => unreachable!("Invalid kind in assigment"),
                }
            }
            Statement::VarDeclaration {
                name: _,
                ty: _,
                expr,
                id,
                source_ref,
            }
            | Statement::ConstDeclaration {
                name: _,
                ty: _,
                expr,
                id,
                source_ref,
            } => {
                let symbol = self.symbol_table.get_symbol_id_by_node_id(*id).unwrap();
                let value = self
                    .transform_expression(expr, ssa_cfg)
                    .expect("variable initializer cannot be void");
                let dest = Register { symbol, version: 0 };

                ssa_cfg.add_instruction_at(self.current_block, SsaInstruction::Copy {
                    dest,
                    source: value,
                    source_ref: source_ref.clone(),
                });
            }
        }
        Ok(())
    }

    fn transform_expression(
        &self,
        expression: &Expression,
        ssa_cfg: &mut SsaCfg,
    ) -> Option<Operand> {
        match expression {
            Expression::Literal(lit, _, _) => match lit {
                Literal::Integer(i) => Some(Operand::Constant(Constant::Int(*i))),
                Literal::Address(a) => Some(Operand::Constant(Constant::Address(*a))),
                Literal::Boolean(b) => Some(Operand::Constant(Constant::Bool(*b))),
                Literal::String(s) => Some(Operand::Constant(Constant::String(s.clone()))),
            },
            Expression::Identifier(_, node_id, source_ref) => {
                let symbol_info = self.symbol_table.get_symbol_by_node_id(*node_id).unwrap();
                let symbol = self
                    .symbol_table
                    .get_symbol_id_by_node_id(*node_id)
                    .unwrap();
                match symbol_info.kind {
                    SymbolKind::StateVar | SymbolKind::StateConst => {
                        let temp = self.new_temp_register();
                        ssa_cfg.local_temps.insert(temp, symbol_info.ty.as_ref().expect("Deberia estar definido (Identifier State)").base.clone());
                        ssa_cfg.add_instruction_at(
                            self.current_block,
                            SsaInstruction::StorageLoad {
                                dest: temp,
                                var: symbol,
                                source_ref: source_ref.clone(),
                            },
                        );
                        Some(Operand::Register(temp))
                    }
                    SymbolKind::LocalVar | SymbolKind::Parameter => { // TODO: LocalVar v = 0?
                        Some(Operand::Register(Register { symbol, version: 0 }))
                    }
                    _ => unreachable!("Invalid kind in assigment: {:?}", symbol_info.kind),
                }
            }
            Expression::BinaryOp {
                left,
                op,
                right,
                id,
                source_ref,
            } => {
                let left_operand = self
                    .transform_expression(left, ssa_cfg)
                    .expect("binary operation left operand cannot be void");
                let right_operand = self
                    .transform_expression(right, ssa_cfg)
                    .expect("binary operation right operand cannot be void");
                let target = self.new_temp_register();

                let symbol_info = self.symbol_table.expr_to_type(*id).expect("Ya se cargo");
                ssa_cfg.local_temps.insert(target, symbol_info.clone());

                ssa_cfg.add_instruction_at(
                    self.current_block,
                    SsaInstruction::BinaryOp {
                        left: left_operand,
                        right: right_operand,
                        op: op.clone(),
                        dest: target,
                        source_ref: source_ref.clone(),
                    },
                );
                Some(Operand::Register(target))
            }
            Expression::UnaryOp {
                op,
                expr,
                id,
                source_ref,
            } => {
                let operand = self
                    .transform_expression(expr, ssa_cfg)
                    .expect("unary operation operand cannot be void");
                let target = self.new_temp_register();
                let symbol_info = self.symbol_table.expr_to_type(*id).expect("Ya se cargo");
                ssa_cfg.local_temps.insert(target, symbol_info.clone());
                ssa_cfg.add_instruction_at(
                    self.current_block,
                    SsaInstruction::UnaryOp {
                        op: op.clone(),
                        operand,
                        dest: target,
                        source_ref: source_ref.clone(),
                    },
                );
                Some(Operand::Register(target))
            }
            Expression::Grouped(expression, _, _) => self.transform_expression(expression, ssa_cfg),
            Expression::FunctionCall {
                name,
                args,
                id,
                source_ref,
            } => {
                let symbol_id = self.symbol_table.get_symbol_id_by_node_id(*id).unwrap();
                let dest = match &self.symbol_table.get_symbol_by_node_id(*id).unwrap().kind {
                    SymbolKind::Function {
                        return_type,
                        ..
                    }
                    | SymbolKind::Entrypoint {
                        return_type,
                        ..
                    } => {
                        // Check if return type is unit type (empty tuple)
                        if matches!(return_type.base, BaseType::Tuple { ref elems } if elems.is_empty())
                        {
                            None
                        } else {
                            let target = self.new_temp_register();
                            ssa_cfg.local_temps.insert(target, return_type.base.clone());
                            Some(target)
                        }
                    }
                    SymbolKind::Contract => {
                        let target = self.new_temp_register();
                        ssa_cfg.local_temps.insert(target, BaseType::Contract(name.clone()));
                        Some(target)
                    }
                    e => {
                        panic!("Function '{}' not found. Kind: {}", name, e);
                    }
                };

                let mut args_operands = vec![];
                for arg in args {
                    let operand = self
                        .transform_expression(arg, ssa_cfg)
                        .expect("function argument cannot be void");
                    args_operands.push(operand);
                }

                ssa_cfg.add_instruction_at(
                    self.current_block,
                    SsaInstruction::Call {
                        dest,
                        target: CallTarget::Internal(symbol_id),
                        args: args_operands,
                        source_ref: source_ref.clone(),
                    },
                );

                dest.map(Operand::Register)
            }
            Expression::MemberCall { 
                object, 
                method: _, 
                args, 
                id, 
                source_ref 
            } => {

                let object_operand = self
                    .transform_expression(object, ssa_cfg)
                    .expect("member call object cannot be void");
                
                let mut args_operands = vec![];
                for arg in args {
                    let operand = self
                        .transform_expression(arg, ssa_cfg)
                        .expect("external call argument cannot be void");
                    args_operands.push(operand);
                }
                
                let method_symbol = self.symbol_table
                    .get_symbol_id_by_node_id(*id)
                    .expect("Method should be resolved in type checking");
                let symbol_info = self.symbol_table.get_symbol_by_node_id(*id).unwrap();
                
                let dest = match &symbol_info.kind {
                    SymbolKind::Function { return_type, .. } 
                    | SymbolKind::Entrypoint { return_type, .. } => {
                        if matches!(return_type.base, BaseType::Tuple { ref elems } if elems.is_empty()) {
                            None
                        } else {
                            let target = self.new_temp_register();
                            ssa_cfg.local_temps.insert(target, return_type.base.clone());
                            Some(target)
                        }
                    }
                    _ => panic!("MemberCall method resolved to non-function: {:?}", symbol_info.kind),
                };
                
                ssa_cfg.add_instruction_at(
                    self.current_block,
                    SsaInstruction::Call {
                        dest,
                        target: CallTarget::External {
                            object: object_operand,
                            method: method_symbol,
                        },
                        args: args_operands,
                        source_ref: source_ref.clone(),
                    },
                );
                
                dest.map(Operand::Register)
            }
        }
    }

}
