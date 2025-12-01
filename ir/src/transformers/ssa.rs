use std::cell::Cell;

use indexmap::IndexMap;
use merak_ast::{
    contract::{Contract, ContractInit, Program, StateDef},
    expression::{BinaryOperator, Expression, Literal, UnaryOperator},
    function::Function,
    predicate::{self, ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp},
    statement::{Block, Statement},
    types::BaseType,
};
use merak_errors::MerakError;
use merak_symbols::{QualifiedName, SymbolId, SymbolKind, SymbolTable};

use crate::ssa_ir::{
    BlockId, CallTarget, Constant, Operand, Register, SsaCfg, SsaContract, SsaContractInit,
    SsaInstruction, SsaProgram, SsaStateDef, Terminator,
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
            contracts: IndexMap::new(),
        };

        for contract_data in program.contracts.iter() {
            self.build_contract(contract_data, &mut ssa_program)?;

            // // Debug print: log SsaCfgs found inside state_defs of the just built contract
            // let (contract_name, _contract) = contract_data;
            // if let Some(ssa_contract) = ssa_program.contracts.get(contract_name) {
            //     eprintln!("[SSA DEBUG] Contract: {}", contract_name);
            //     for (state_name, state_def) in &ssa_contract.state_defs {
            //         eprintln!("  [SSA DEBUG] StateDef: {}", state_name);
            //         for (idx, func_cfg) in state_def.functions.iter().enumerate() {
            //             // Attempt to print basic CFG info; assuming SsaCfg implements Debug will print the whole CFG
            //             // Otherwise, print a minimal summary via available fields
            //             eprintln!("    [SSA DEBUG] Function #{} CFG: {:?}", idx, func_cfg);
            //         }
            //     }
            // }
        }

        Ok(ssa_program)
    }

    fn build_contract(
        &mut self,
        contract_data: (&String, &Contract),
        ssa_program: &mut SsaProgram,
    ) -> Result<(), MerakError> {
        let (contract_name, contract) = contract_data;

        let ssa_contract_init = self.build_contract_init(&contract.data)?;
        let mut ssa_state_defs = vec![];
        for (name, state_def) in contract.state_defs.iter() {
            let ssa_state_def = self.build_state_def(state_def)?;
            ssa_state_defs.push((name.clone(), ssa_state_def));
        }

        let ssa_contract = SsaContract {
            imports: contract.imports.clone(),
            data: ssa_contract_init,
            state_defs: ssa_state_defs,
        };

        ssa_program
            .contracts
            .insert(contract_name.clone(), ssa_contract);

        Ok(())
    }

    fn build_contract_init(&mut self, init: &ContractInit) -> Result<SsaContractInit, MerakError> {
        let constructor_cfg = if let Some(constructor) = &init.constructor {
            let constructor_id = self
                .symbol_table
                .get_symbol_id_by_node_id(constructor.id())
                .expect("Constructor should be defined");

            let mut ssa_cfg = SsaCfg::new(constructor_id);

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

        Ok(SsaContractInit {
            name: init.name.clone(),
            states: init.states.clone(),
            variables: init.variables.clone(),
            constants: init.constants.clone(),
            constructor: constructor_cfg,
        })
    }

    fn build_state_def(&mut self, state_def: &StateDef) -> Result<SsaStateDef, MerakError> {
        let mut ssa_functions = vec![];
        for function in &state_def.functions {
            let ssa_function = self.build_function(function)?;
            ssa_functions.push(ssa_function);
        }

        Ok(SsaStateDef {
            contract: state_def.contract.clone(),
            name: state_def.name.clone(),
            owner: state_def.owner.clone(),
            functions: ssa_functions,
            source_ref: state_def.source_ref.clone(),
        })
    }

    fn build_function(&mut self, function: &Function) -> Result<SsaCfg, MerakError> {
        let function_id = self
            .symbol_table
            .get_symbol_id_by_node_id(function.id())
            .expect("Function should be defined");
        let mut ssa_cfg = SsaCfg::new(function_id);

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
                let header_bb = ssa_cfg.new_block();
                let then_bb = ssa_cfg.new_block();
                let (else_bb, exit_bb) = if else_block.is_some() {
                    let else_bb = ssa_cfg.new_block();
                    let exit_bb = ssa_cfg.new_block();
                    (else_bb, exit_bb)
                } else {
                    let exit_bb = ssa_cfg.new_block();
                    (exit_bb, exit_bb)
                };

                ssa_cfg
                    .add_terminator_at(self.current_block, Terminator::Jump { target: header_bb });
                ssa_cfg.add_edge(self.current_block, header_bb);
                self.current_block = header_bb;

                let cond_operand = self
                    .transform_expression(condition, ssa_cfg)
                    .expect("condition in if statement cannot be void");
                ssa_cfg.add_terminator_at(
                    header_bb,
                    Terminator::Branch {
                        condition: cond_operand,
                        then_block: then_bb,
                        else_block: else_bb,
                        source_ref: source_ref.clone(),
                    },
                );

                ssa_cfg.add_edge(header_bb, then_bb);
                self.current_block = then_bb;

                self.transform_block(then_block, ssa_cfg)?;
                ssa_cfg.add_terminator_at(then_bb, Terminator::Jump { target: exit_bb });
                ssa_cfg.add_edge(then_bb, exit_bb);

                if let Some(else_block) = else_block {
                    ssa_cfg.add_edge(header_bb, else_bb);
                    self.current_block = else_bb;
                    self.transform_block(else_block, ssa_cfg)?;
                    ssa_cfg.add_terminator_at(else_bb, Terminator::Jump { target: exit_bb });
                    ssa_cfg.add_edge(else_bb, exit_bb);
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
                        source_ref: source_ref.clone(),
                    },
                );

                ssa_cfg.add_edge(header_bb, body_bb);
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

                ssa_cfg.add_terminator(terminator);
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
                        ssa_cfg.add_instruction(SsaInstruction::StorageStore {
                            var: symbol,
                            value,
                            source_ref: source_ref.clone(),
                        });
                    }
                    SymbolKind::LocalVar | SymbolKind::Parameter => {
                        let dest = Register { symbol, version: 0 };

                        ssa_cfg.add_instruction(SsaInstruction::Copy {
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

                ssa_cfg.add_instruction(SsaInstruction::Copy {
                    dest,
                    source: value,
                    source_ref: source_ref.clone(),
                });
            }
            Statement::Become(_, node_id, source_ref) => {
                println!("Become node id: {node_id:?}");
                println!("Symbol table: {}", self.symbol_table);
                let new_state = self
                    .symbol_table
                    .get_symbol_id_by_node_id(node_id.clone())
                    .unwrap();
                ssa_cfg.add_instruction(SsaInstruction::StateTransition {
                    new_state,
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
                    SymbolKind::LocalVar | SymbolKind::Parameter => {
                        Some(Operand::Register(Register { symbol, version: 0 }))
                    }
                    _ => unreachable!("Invalid kind in assigment: {:?}", symbol_info.kind),
                }
            }
            Expression::BinaryOp {
                left,
                op,
                right,
                id: _,
                source_ref,
            } => {
                let left_operand = self
                    .transform_expression(left, ssa_cfg)
                    .expect("binary operation left operand cannot be void");
                let right_operand = self
                    .transform_expression(right, ssa_cfg)
                    .expect("binary operation right operand cannot be void");
                let target = self.new_temp_register();

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
                id: _,
                source_ref,
            } => {
                let operand = self
                    .transform_expression(expr, ssa_cfg)
                    .expect("unary operation operand cannot be void");
                let target = self.new_temp_register();
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
                let dest = match &self.symbol_table.get_symbol_by_node_id(*id).unwrap().kind {
                    SymbolKind::Function {
                        state: _,
                        return_type,
                        ..
                    }
                    | SymbolKind::Entrypoint {
                        state: _,
                        return_type,
                        ..
                    } => {
                        // Check if return type is unit type (empty tuple)
                        if matches!(return_type.base, BaseType::Tuple { ref elems } if elems.is_empty())
                        {
                            None
                        } else {
                            Some(self.new_temp_register())
                        }
                    }
                    _ => {
                        panic!("Function '{}' not found", name);
                    }
                };

                let mut args_operands = vec![];
                for arg in args {
                    let operand = self
                        .transform_expression(arg, ssa_cfg)
                        .expect("function argument cannot be void");
                    args_operands.push(operand);
                }

                let target = self
                    .symbol_table
                    .get_symbol_by_node_id(*id)
                    .map(|info| self.name_to_target(info.qualified_name.clone()))
                    .unwrap_or_else(|| panic!("Function '{}' not found", name));

                ssa_cfg.add_instruction_at(
                    self.current_block,
                    SsaInstruction::Call {
                        dest,
                        target,
                        args: args_operands,
                        source_ref: source_ref.clone(),
                    },
                );

                dest.map(Operand::Register)
            }
        }
    }

    fn name_to_target(&self, qualified_name: QualifiedName) -> CallTarget {
        use merak_symbols::SymbolNamespace;

        let last = qualified_name.parts.last().unwrap();

        if last.contains('.') {
            let parts: Vec<&str> = last.split('.').collect();
            let contract_name = parts[0];
            let function_name = parts[1];

            let contract_id = self
                .symbol_table
                .lookup(contract_name, SymbolNamespace::Type)
                .unwrap_or_else(|| panic!("Contract '{}' not found", contract_name));

            let function_id = self
                .symbol_table
                .lookup(function_name, SymbolNamespace::Callable)
                .unwrap_or_else(|| {
                    panic!(
                        "Function '{}' not found in contract '{}'",
                        function_name, contract_name
                    )
                });

            CallTarget::External {
                contract: contract_id,
                function: function_id,
            }
        } else {
            let function_id = self
                .symbol_table
                .lookup(last, SymbolNamespace::Callable)
                .unwrap_or_else(|| panic!("Function '{}' not found", last));

            CallTarget::Internal(function_id)
        }
    }

    // fn transform_refinement_expr(
    //     &self,
    //     expr: &RefinementExpr,
    //     ssa_cfg: &mut SsaCfg,
    // ) -> Option<Operand> {
    //     match expr {
    //         // Literals
    //         RefinementExpr::IntLit(val, _, _) => Some(Operand::Constant(Constant::Int(*val))),
    //         RefinementExpr::AddressLit(addr, _, _) => {
    //             // TODO: Parse address string to H256
    //             todo!("Parse address literal string '{}' to H256 constant", addr)
    //         }

    //         // Variables
    //         RefinementExpr::Var(_, node_id, source_ref) => {
    //             let symbol_info = self.symbol_table.get_symbol_by_node_id(*node_id).unwrap();
    //             let symbol = self
    //                 .symbol_table
    //                 .get_symbol_id_by_node_id(*node_id)
    //                 .unwrap();
    //             match symbol_info.kind {
    //                 SymbolKind::StateVar | SymbolKind::StateConst => {
    //                     let temp = self.new_temp_register();
    //                     ssa_cfg.add_instruction_at(
    //                         self.current_block,
    //                         SsaInstruction::StorageLoad {
    //                             dest: temp,
    //                             var: symbol,
    //                             source_ref: source_ref.clone(),
    //                         },
    //                     );
    //                     Some(Operand::Register(temp))
    //                 }
    //                 SymbolKind::LocalVar | SymbolKind::Parameter => {
    //                     Some(Operand::Register(Register { symbol, version: 0 }))
    //                 }
    //                 _ => unreachable!("Invalid kind in refinement var: {:?}", symbol_info.kind),
    //             }
    //         }

    //         // Binary operations (arithmetic)
    //         RefinementExpr::BinOp {
    //             op,
    //             lhs,
    //             rhs,
    //             id: _,
    //             source_ref,
    //         } => {
    //             let left_operand = self
    //                 .transform_refinement_expr(lhs, ssa_cfg)
    //                 .expect("refinement binary op left operand cannot be void");
    //             let right_operand = self
    //                 .transform_refinement_expr(rhs, ssa_cfg)
    //                 .expect("refinement binary op right operand cannot be void");
    //             let target = self.new_temp_register();

    //             // Map ArithOp to BinaryOperator
    //             let binary_op = match op {
    //                 ArithOp::Add => BinaryOperator::Add,
    //                 ArithOp::Sub => BinaryOperator::Subtract,
    //                 ArithOp::Mul => BinaryOperator::Multiply,
    //                 ArithOp::Div => BinaryOperator::Divide,
    //                 ArithOp::Mod => BinaryOperator::Modulo,
    //             };

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::BinaryOp {
    //                     left: left_operand,
    //                     right: right_operand,
    //                     op: binary_op,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         // Unary operations
    //         RefinementExpr::UnaryOp {
    //             op,
    //             expr,
    //             id: _,
    //             source_ref,
    //         } => {
    //             let operand = self
    //                 .transform_refinement_expr(expr, ssa_cfg)
    //                 .expect("refinement unary op operand cannot be void");
    //             let target = self.new_temp_register();

    //             // Map UnaryOp to UnaryOperator
    //             let unary_op = match op {
    //                 UnaryOp::Negate => UnaryOperator::Negate,
    //             };

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::UnaryOp {
    //                     op: unary_op,
    //                     operand,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         // Special variables
    //         RefinementExpr::MsgSender(_, _) => {
    //             todo!("Handle msg.sender as a special global variable or intrinsic")
    //         }
    //         RefinementExpr::MsgValue(_, _) => {
    //             todo!("Handle msg.value as a special global variable or intrinsic")
    //         }
    //         RefinementExpr::BlockTimestamp(_, _) => {
    //             todo!("Handle block.timestamp as a special global variable or intrinsic")
    //         }

    //         // Uninterpreted functions
    //         RefinementExpr::UninterpFn {
    //             name,
    //             args,
    //             id: _,
    //             source_ref,
    //         } => {
    //             let _ = (name, args, source_ref);
    //             todo!("Handle uninterpreted function calls in refinement expressions")
    //         }
    //     }
    // }

    // fn transform_predicate(&self, predicate: &Predicate, ssa_cfg: &mut SsaCfg) -> Option<Operand> {
    //     match predicate {
    //         // Boolean literals
    //         Predicate::True(_, _) => Some(Operand::Constant(Constant::Bool(true))),
    //         Predicate::False(_, _) => Some(Operand::Constant(Constant::Bool(false))),

    //         // Boolean variables
    //         Predicate::Var(_, node_id, source_ref) => {
    //             let symbol_info = self.symbol_table.get_symbol_by_node_id(*node_id).unwrap();
    //             let symbol = self
    //                 .symbol_table
    //                 .get_symbol_id_by_node_id(*node_id)
    //                 .unwrap();
    //             match symbol_info.kind {
    //                 SymbolKind::StateVar | SymbolKind::StateConst => {
    //                     let temp = self.new_temp_register();
    //                     ssa_cfg.add_instruction_at(
    //                         self.current_block,
    //                         SsaInstruction::StorageLoad {
    //                             dest: temp,
    //                             var: symbol,
    //                             source_ref: source_ref.clone(),
    //                         },
    //                     );
    //                     Some(Operand::Register(temp))
    //                 }
    //                 SymbolKind::LocalVar | SymbolKind::Parameter => {
    //                     Some(Operand::Register(Register { symbol, version: 0 }))
    //                 }
    //                 _ => unreachable!("Invalid kind in predicate var: {:?}", symbol_info.kind),
    //             }
    //         }

    //         // Binary relations (comparisons)
    //         Predicate::BinRel {
    //             op,
    //             lhs,
    //             rhs,
    //             id: _,
    //             source_ref,
    //         } => {
    //             let left_operand = self
    //                 .transform_refinement_expr(lhs, ssa_cfg)
    //                 .expect("predicate binary relation left operand cannot be void");
    //             let right_operand = self
    //                 .transform_refinement_expr(rhs, ssa_cfg)
    //                 .expect("predicate binary relation right operand cannot be void");
    //             let target = self.new_temp_register();

    //             // Map RelOp to BinaryOperator
    //             let binary_op = match op {
    //                 RelOp::Eq => BinaryOperator::Equal,
    //                 RelOp::Neq => BinaryOperator::NotEqual,
    //                 RelOp::Lt => BinaryOperator::Less,
    //                 RelOp::Leq => BinaryOperator::LessEqual,
    //                 RelOp::Gt => BinaryOperator::Greater,
    //                 RelOp::Geq => BinaryOperator::GreaterEqual,
    //             };

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::BinaryOp {
    //                     left: left_operand,
    //                     right: right_operand,
    //                     op: binary_op,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         // Logical operations
    //         Predicate::And(left, right, _, source_ref) => {
    //             let left_operand = self
    //                 .transform_predicate(left, ssa_cfg)
    //                 .expect("predicate AND left operand cannot be void");
    //             let right_operand = self
    //                 .transform_predicate(right, ssa_cfg)
    //                 .expect("predicate AND right operand cannot be void");
    //             let target = self.new_temp_register();

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::BinaryOp {
    //                     left: left_operand,
    //                     right: right_operand,
    //                     op: BinaryOperator::LogicalAnd,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         Predicate::Or(left, right, _, source_ref) => {
    //             let left_operand = self
    //                 .transform_predicate(left, ssa_cfg)
    //                 .expect("predicate OR left operand cannot be void");
    //             let right_operand = self
    //                 .transform_predicate(right, ssa_cfg)
    //                 .expect("predicate OR right operand cannot be void");
    //             let target = self.new_temp_register();

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::BinaryOp {
    //                     left: left_operand,
    //                     right: right_operand,
    //                     op: BinaryOperator::LogicalOr,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         Predicate::Not(pred, _, source_ref) => {
    //             let operand = self
    //                 .transform_predicate(pred, ssa_cfg)
    //                 .expect("predicate NOT operand cannot be void");
    //             let target = self.new_temp_register();

    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::UnaryOp {
    //                     op: UnaryOperator::Not,
    //                     operand,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         // Implication: a ==> b is equivalent to !a || b
    //         Predicate::Implies(left, right, _, source_ref) => {
    //             let left_operand = self
    //                 .transform_predicate(left, ssa_cfg)
    //                 .expect("predicate IMPLIES left operand cannot be void");
    //             let right_operand = self
    //                 .transform_predicate(right, ssa_cfg)
    //                 .expect("predicate IMPLIES right operand cannot be void");

    //             // Generate !left
    //             let not_left = self.new_temp_register();
    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::UnaryOp {
    //                     op: UnaryOperator::Not,
    //                     operand: left_operand,
    //                     dest: not_left,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );

    //             // Generate !left || right
    //             let target = self.new_temp_register();
    //             ssa_cfg.add_instruction_at(
    //                 self.current_block,
    //                 SsaInstruction::BinaryOp {
    //                     left: Operand::Register(not_left),
    //                     right: right_operand,
    //                     op: BinaryOperator::LogicalOr,
    //                     dest: target,
    //                     source_ref: source_ref.clone(),
    //                 },
    //             );
    //             Some(Operand::Register(target))
    //         }

    //         // Uninterpreted function calls
    //         Predicate::UninterpFnCall {
    //             name,
    //             args,
    //             id: _,
    //             source_ref,
    //         } => {
    //             let _ = (name, args, source_ref);
    //             todo!("Handle uninterpreted function calls in predicates")
    //         }
    //     }
    // }
}
