use std::cell::RefCell;
use std::iter::zip;

use merak_ast::{
    contract::{Contract, ContractInit},
    expression::{BinaryOperator, Expression, Literal},
    meta::{SourceRef, SourceRefGuard},
    node_id::NodeId,
    predicate::Predicate,
    statement::{Block, Statement},
    types::{BaseType, Type},
};
use merak_errors::MerakError;
use merak_symbols::{SymbolKind, SymbolTable};

use crate::Program;
pub struct Typechecker {
    symbol_table: SymbolTable,
    expected_return_type: RefCell<Option<Type>>,
}

impl Typechecker {
    pub fn new(symbol_table: SymbolTable) -> Self {
        Self {
            symbol_table,
            expected_return_type: RefCell::new(None),
        }
    }

    pub fn check(mut self, program: &Program) -> Result<SymbolTable, MerakError> {
        for (_contract_name, contract) in program.contracts.iter() {
            if let Err(e) = self.check_contract(contract) {
                return Err(e);
            }
        }
        Ok(self.symbol_table)
    }

    fn check_contract(&mut self, contract: &Contract) -> Result<(), MerakError> {
        self.check_contract_init(&contract.data)?;
        for (_, state_def) in contract.state_defs.iter() {
            for function in &state_def.functions {
                // Set the expected return type context before checking the function body
                let previous = self
                    .expected_return_type
                    .replace(function.return_type.clone());
                let result = self.check_block(&function.body);
                self.expected_return_type.replace(previous);
                result?;
            }
        }
        Ok(())
    }

    fn check_contract_init(&mut self, contract_data: &ContractInit) -> Result<(), MerakError> {
        for state_var in &contract_data.variables {
            let infered_type = self.infer_basetype(&state_var.expr)?;
            self.check_basetype(&infered_type, &state_var.ty.base)?;
        }

        for state_const in &contract_data.constants {
            let infered_type = self.infer_basetype(&state_const.expr)?;
            self.check_basetype(&infered_type, &state_const.ty.base)?;
        }

        if let Some(constructor) = &contract_data.constructor {
            // Constructor has no return type (implicitly void)
            let previous = self.expected_return_type.replace(None);
            let result = self.check_block(&constructor.body);
            self.expected_return_type.replace(previous);
            result?;
        }

        Ok(())
    }

    fn check_block(&mut self, body: &Block) -> Result<(), MerakError> {
        for statement in &body.statements {
            self.check_statement(statement)?;
        }
        Ok(())
    }

    fn check_statement(&mut self, statement: &Statement) -> Result<(), MerakError> {
        match statement {
            Statement::Expression(expression, node_id, source_ref) => {
                let _guard = SourceRefGuard::new(source_ref.clone());
                let infered_type = self.infer_basetype(expression)?;

                let symbol = self
                    .symbol_table
                    .get_symbol_by_node_id(*node_id)
                    .expect("Name resolution should have resolved this reference");

                let basetype_expected = &symbol
                    .ty
                    .as_ref()
                    .expect(&format!("Expression '{}' should have a type", expression))
                    .base;

                self.check_basetype(&infered_type, basetype_expected)?;
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                id: _,
                source_ref,
            } => {
                let _guard = SourceRefGuard::new(source_ref.clone());
                let inferred_type = self.infer_basetype(condition)?;
                self.check_basetype(&inferred_type, &BaseType::Bool)?;

                self.check_block(then_block)?;
                if let Some(else_block) = else_block {
                    self.check_block(&else_block)?;
                }
            }
            Statement::While {
                condition,
                invariants: _,
                variants: _,
                body,
                id: _,
                source_ref,
            } => {
                let _guard = SourceRefGuard::new(source_ref.clone());
                eprintln!("Variants and variants are not typechecked yet");
                let inferred_type = self.infer_basetype(condition)?;
                self.check_basetype(&inferred_type, &BaseType::Bool)?;
                self.check_block(body)?;
            }
            Statement::Return(expression, _node_id, source_ref) => {
                let _guard = SourceRefGuard::new(source_ref.clone());

                let expected_return = self.expected_return_type.borrow();

                match (expression, expected_return.as_ref()) {
                    // Case 1: return with value in function with return type
                    (Some(expr), Some(expected_type)) => {
                        let inferred_type = self.infer_basetype(expr)?;
                        self.check_basetype(&inferred_type, &expected_type.base)?;
                    }

                    // Case 2: return without value in void function - OK
                    (None, None) => {
                        // Valid: return; in void function or constructor
                    }

                    // Case 3: return with value in void function - ERROR
                    (Some(expr), None) => {
                        let inferred_type = self.infer_basetype(expr)?;
                        return Err(MerakError::TypeMismatch {
                            expected: "void (no return value)".to_string(),
                            found: inferred_type.to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }

                    // Case 4: return without value in function expecting a return value - ERROR
                    (None, Some(expected_type)) => {
                        return Err(MerakError::TypeMismatch {
                            expected: expected_type.base.to_string(),
                            found: "void (no return value)".to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }
                }
            }
            Statement::Assignment {
                target,
                expr,
                id,
                source_ref,
            } => {
                // RAII guard automatically manages source_ref for error reporting
                // It will be popped from the stack when this scope ends
                let _guard = SourceRefGuard::new(source_ref.clone());

                let infered_type = self.infer_basetype(expr)?;

                // Name resolution phase already connected this node_id to the symbol
                let symbol = self
                    .symbol_table
                    .get_symbol_by_node_id(*id)
                    .expect("Name resolution should have resolved this reference");

                let basetype_expected = &symbol
                    .ty
                    .as_ref()
                    .expect(&format!("Variable '{}' should have a type", target))
                    .base;

                self.check_basetype(&infered_type, basetype_expected)?;
            }
            Statement::VarDeclaration {
                name,
                ty,
                expr,
                id,
                source_ref,
            }
            | Statement::ConstDeclaration {
                name,
                ty,
                expr,
                id,
                source_ref,
            } => {
                let _guard = SourceRefGuard::new(source_ref.clone());

                let infered_type = self.infer_basetype(expr)?;

                if let Some(explicit_ty) = ty {
                    self.check_basetype(&infered_type, &explicit_ty.base)?;
                } else {
                    self.symbol_table.update_type(
                        *id,
                        Type {
                            base: infered_type,
                            binder: name.clone(),
                            constraint: Predicate::True(NodeId::from(0), SourceRef::unknown()),
                            source_ref: source_ref.clone(),
                        },
                    )?;
                }
            }
            Statement::Become(..) => {
                // State validity already checked during name resolution
            }
        }

        Ok(())
    }

    fn infer_basetype(&self, expr: &Expression) -> Result<BaseType, MerakError> {
        match expr {
            Expression::Literal(literal, ..) => match literal {
                Literal::Integer(_) => Ok(BaseType::Int),
                Literal::Boolean(_) => Ok(BaseType::Bool),
                Literal::String(_) => Ok(BaseType::String),
                Literal::Address(_) => Ok(BaseType::Address),
            },
            Expression::Identifier(var, id, ..) => {
                // Name resolution phase already connected this node_id to the symbol
                let symbol = self
                    .symbol_table
                    .get_symbol_by_node_id(*id)
                    .expect("Name resolution should have resolved this reference");

                match symbol.ty {
                    Some(ref ty) => Ok(ty.base.clone()),
                    None => panic!("Variable '{}' should have a type", var),
                }
            }
            Expression::BinaryOp {
                left, op, right, ..
            } => {
                let base_left = self.infer_basetype(left)?;
                let base_right = self.infer_basetype(right)?;

                self.assert_binary_type(op, &base_left, &base_right)
            }
            Expression::UnaryOp { op, expr, .. } => {
                let base_type = self.infer_basetype(expr)?;
                self.assert_unary_type(op, &base_type)
            }
            Expression::Grouped(expr, ..) => self.infer_basetype(expr),
            Expression::FunctionCall {
                name,
                args,
                id,
                source_ref,
            } => {
                // Name resolution phase already connected this node_id to the symbol
                let info = self
                    .symbol_table
                    .get_symbol_by_node_id(*id)
                    .expect("Name resolution should have resolved this function reference");

                // Early panic: function has no declared type
                let Some(ref ty) = info.ty else {
                    panic!("Function '{}' is not declared", name);
                };

                let (return_type, parameters) = match &info.kind {
                    SymbolKind::Function {
                        return_type,
                        parameters,
                        ..
                    } => (return_type, parameters),
                    SymbolKind::Entrypoint {
                        return_type,
                        parameters,
                        ..
                    } => (return_type, parameters),
                    _ => {
                        return Err(MerakError::TypeMismatch {
                            expected: "Function or Entrypoint".to_string(),
                            found: ty.base.to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }
                };

                // Check arity
                if parameters.len() != args.len() {
                    return Err(MerakError::ArityMismatch {
                        name: name.to_string(),
                        expected: parameters.len(),
                        found: args.len(),
                        source_ref: source_ref.clone(),
                    });
                }

                // Check argument types
                for (arg, param) in zip(args, parameters) {
                    let infered_type = self.infer_basetype(arg)?;
                    self.check_basetype(&infered_type, &param.ty.base)?;
                }

                Ok(return_type.base.clone())
            }
        }
    }

    fn check_basetype(
        &self,
        inferred_type: &BaseType,
        expected_type: &BaseType,
    ) -> Result<(), MerakError> {
        match (inferred_type, expected_type) {
            (BaseType::Int, BaseType::Int) => Ok(()),
            (BaseType::Bool, BaseType::Bool) => Ok(()),
            (BaseType::String, BaseType::String) => Ok(()),
            (BaseType::Address, BaseType::Address) => Ok(()),
            _ => Err(MerakError::TypeMismatch {
                expected: expected_type.to_string(),
                found: inferred_type.to_string(),
                source_ref: SourceRefGuard::current(), // Automatically uses the current source_ref from the guard
            }),
        }
    }

    fn assert_binary_type(
        &self,
        op: &BinaryOperator,
        left: &BaseType,
        right: &BaseType,
    ) -> Result<BaseType, MerakError> {
        use merak_ast::expression::BinaryOperator::*;

        let base_left = left;
        let base_right = right;

        match op {
            Add | Subtract | Multiply | Divide | Modulo => {
                Self::check_arithmetic_op(base_left, base_right)
            }
            Equal | NotEqual => Self::check_equality_op(base_left, base_right),
            Less | LessEqual | Greater | GreaterEqual => {
                Self::check_ordering_op(base_left, base_right)
            }
            LogicalAnd | LogicalOr => Self::check_logical_op(base_left, base_right),
        }
    }

    fn check_arithmetic_op(left: &BaseType, right: &BaseType) -> Result<BaseType, MerakError> {
        match (left, right) {
            (BaseType::Int, BaseType::Int) => Ok(BaseType::Int),
            _ => Err(MerakError::TypeMismatch {
                expected: "Int".to_string(),
                found: format!("{} and {}", left, right),
                source_ref: SourceRef::unknown(),
            }),
        }
    }

    fn check_equality_op(left: &BaseType, right: &BaseType) -> Result<BaseType, MerakError> {
        match (left, right) {
            (BaseType::Int, BaseType::Int)
            | (BaseType::Bool, BaseType::Bool)
            | (BaseType::String, BaseType::String)
            | (BaseType::Address, BaseType::Address) => Ok(BaseType::Bool),
            _ => Err(MerakError::TypeMismatch {
                expected: format!("same type on both sides"),
                found: format!("{} and {}", left, right),
                source_ref: SourceRef::unknown(),
            }),
        }
    }

    fn check_ordering_op(left: &BaseType, right: &BaseType) -> Result<BaseType, MerakError> {
        match (left, right) {
            (BaseType::Int, BaseType::Int) => Ok(BaseType::Bool),
            _ => Err(MerakError::TypeMismatch {
                expected: "Int".to_string(),
                found: format!("{} and {}", left, right),
                source_ref: SourceRef::unknown(),
            }),
        }
    }

    fn check_logical_op(left: &BaseType, right: &BaseType) -> Result<BaseType, MerakError> {
        match (left, right) {
            (BaseType::Bool, BaseType::Bool) => Ok(BaseType::Bool),
            _ => Err(MerakError::TypeMismatch {
                expected: "Bool".to_string(),
                found: format!("{} and {}", left, right),
                source_ref: SourceRef::unknown(),
            }),
        }
    }

    fn assert_unary_type(
        &self,
        op: &merak_ast::expression::UnaryOperator,
        operand: &BaseType,
    ) -> Result<BaseType, MerakError> {
        use merak_ast::expression::UnaryOperator::*;

        let base_type = operand;

        match op {
            Negate => match base_type {
                BaseType::Int => Ok(BaseType::Int),
                _ => Err(MerakError::TypeMismatch {
                    expected: "Int".to_string(),
                    found: base_type.to_string(),
                    source_ref: SourceRef::unknown(),
                }),
            },
            Not => match base_type {
                BaseType::Bool => Ok(BaseType::Bool),
                _ => Err(MerakError::TypeMismatch {
                    expected: "Bool".to_string(),
                    found: base_type.to_string(),
                    source_ref: SourceRef::unknown(),
                }),
            },
        }
    }
}
