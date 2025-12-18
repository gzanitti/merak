use std::cell::RefCell;
use std::iter::zip;

use merak_ast::{
    contract::{Contract, File},
    expression::{BinaryOperator, Expression, Literal},
    meta::{SourceRef, SourceRefGuard},
    node_id::NodeId,
    predicate::Predicate,
    statement::{Block, Statement},
    types::{BaseType, Type},
};
use merak_errors::MerakError;
use merak_symbols::{SymbolKind, SymbolNamespace, SymbolTable};

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
        for (_contract_name, file) in program.files.iter() {
            if let Err(e) = self.check_contract(&file.contract) {
                return Err(e);
            }
        }
        Ok(self.symbol_table)
    }

    fn check_contract(&mut self, contract: &Contract) -> Result<(), MerakError> {

        for state_var in &contract.variables {
            let infered_type = self.infer_basetype(&state_var.expr)?;
            self.check_basetype(&infered_type, &state_var.ty.base)?;
        }

        for state_const in &contract.constants {
            let infered_type = self.infer_basetype(&state_const.expr)?;
            self.check_basetype(&infered_type, &state_const.ty.base)?;
        }

        if let Some(constructor) = &contract.constructor {
            // Constructor has no return type (implicitly void)
            let previous = self.expected_return_type.replace(None);
            let result = self.check_block(&constructor.body);
            self.expected_return_type.replace(previous);
            result?;
        }


        for function in &contract.functions {
            // Set the expected return type context before checking the function body
            let previous = self
                .expected_return_type
                .replace(function.return_type.clone());
            let result = self.check_block(&function.body);
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
                self.symbol_table.insert_expr_type(*node_id, infered_type);

                // TODO: Unused return value if infered_type != ()
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

                // Clone to avoid holding the borrow across mutable calls to infer_basetype
                let expected_return_opt = self.expected_return_type.borrow().clone();

                match (expression, expected_return_opt.as_ref()) {
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
                            explicit_annotation: false,
                            source_ref: source_ref.clone(),
                        },
                    )?;
                }
            }
        }

        Ok(())
    }

    // Requires `&mut self` because MemberCall resolution (`object.method()`) needs type information
    // to construct the qualified name (`Contract::method`). This can only be done during type checking
    // after inferring the object's type, not during name resolution phase.
    fn infer_basetype(&mut self, expr: &Expression) -> Result<BaseType, MerakError> {
        match expr {
            Expression::Literal(literal, id, ..) => {
                let base_type = match literal {
                    Literal::Integer(_) => BaseType::Int,
                    Literal::Boolean(_) => BaseType::Bool,
                    Literal::String(_) => BaseType::String,
                    Literal::Address(_) => BaseType::Address,
                };

                self.symbol_table.insert_expr_type(*id, base_type.clone());
                Ok(base_type)
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
                left, op, right, id, ..
            } => {
                let base_left = self.infer_basetype(left)?;
                let base_right = self.infer_basetype(right)?;

                let base_type = self.assert_binary_type(op, &base_left, &base_right)?;
                self.symbol_table.insert_expr_type(*id, base_type.clone());

                Ok(base_type) 
            }
            Expression::UnaryOp { op, expr, id, .. } => {
                let base_type = self.infer_basetype(expr)?;
                let base_type = self.assert_unary_type(op, &base_type)?;
                self.symbol_table.insert_expr_type(*id, base_type.clone());

                Ok(base_type)
            }
            Expression::Grouped(expr, ..) => self.infer_basetype(expr),
            Expression::FunctionCall {
                name,
                args,
                id,
                source_ref,
            } => {
                let info = self
                    .symbol_table
                    .get_symbol_by_node_id(*id)
                    .expect("Name resolution should have resolved this function reference");

                if matches!(info.kind, SymbolKind::Contract { .. } | SymbolKind::Interface { .. }) {
                    if args.len() != 1 {
                        return Err(MerakError::ArityMismatch {
                            name: name.clone(),
                            expected: 1,
                            found: args.len(),
                            source_ref: source_ref.clone(),
                        });
                    }
                    
                    let arg_type = self.infer_basetype(&args[0])?;
                    if !matches!(arg_type, BaseType::Address) {
                        return Err(MerakError::TypeMismatch {
                            expected: "address".to_string(),
                            found: arg_type.to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }
                    
                    return Ok(BaseType::Contract(name.clone()));
                }

                let (return_type, parameters) = match &info.kind {
                    SymbolKind::Function {
                        return_type,
                        parameters,
                        ..
                    } | SymbolKind::Entrypoint {
                        return_type,
                        parameters,
                        ..
                    } => (return_type, parameters),
                    _ => {
                        return Err(MerakError::TypeMismatch {
                            expected: "Function or Entrypoint".to_string(),
                            found: info.kind.to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }
                };

                if parameters.len() != args.len() {
                    return Err(MerakError::ArityMismatch {
                        name: name.to_string(),
                        expected: parameters.len(),
                        found: args.len(),
                        source_ref: source_ref.clone(),
                    });
                }

                // Clone parameter types to avoid holding immutable borrow during mutable infer_basetype calls
                let param_types: Vec<BaseType> = parameters.iter().map(|p| p.ty.base.clone()).collect();
                let return_type_base = return_type.base.clone();

                for (arg, param_type) in zip(args, param_types) {
                    let infered_type = self.infer_basetype(arg)?;
                    self.check_basetype(&infered_type, &param_type)?;
                }

                Ok(return_type_base)
            }
            Expression::MemberCall { object, method, args, id, source_ref } => {
                let object_type = self.infer_basetype(object)?;
                
                let contract_name = match &object_type {
                    BaseType::Contract(name) => name,
                    _ => {
                        return Err(MerakError::MemberCallOnNonContract {
                            found: object_type.to_string(),
                            source_ref: source_ref.clone(),
                        });
                    }
                };
                
                let qualified_method_name = format!("{}::{}", contract_name, method);
                let method_symbol_id = self.symbol_table.resolve_reference(
                    *id, 
                    &qualified_method_name, 
                    SymbolNamespace::Callable)
                    .ok_or_else(|| MerakError::UndefinedMethod {
                        method: method.clone(),
                        contract: contract_name.clone(),
                        source_ref: source_ref.clone(),
                    })?;

                //println!("MemberCall id: {id:?}");
                //println!("Method symbol id: {}", method_symbol_id);
                let method_symbol = self.symbol_table.get_symbol(method_symbol_id);
                
                let (return_type, parameters) = match &method_symbol.kind {
                    SymbolKind::InterfaceFunction { params, return_type, .. } => {
                        (return_type, params)
                    }
                    SymbolKind::Function { parameters, return_type, .. } => {
                        (return_type, parameters)
                    }
                    SymbolKind::Entrypoint { parameters, return_type, .. } => {
                        (return_type, parameters)
                    }
                    _ => {
                        return Err(MerakError::NotCallable {
                            name: method.clone(),
                            source_ref: source_ref.clone(),
                        });
                    }
                };
                
                if parameters.len() != args.len() {
                    return Err(MerakError::ArityMismatch {
                        name: method.clone(),
                        expected: parameters.len(),
                        found: args.len(),
                        source_ref: source_ref.clone(),
                    });
                }

                // Clone parameter types to avoid holding immutable borrow during mutable infer_basetype calls
                let param_types: Vec<BaseType> = parameters.iter().map(|p| p.ty.base.clone()).collect();
                let return_type_base = return_type.base.clone();

                for (arg, param_type) in zip(args, param_types) {
                    let inferred_arg_type = self.infer_basetype(arg)?;
                    self.check_basetype(&inferred_arg_type, &param_type)?;
                }

                Ok(return_type_base)
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
            (BaseType::Contract(c1), BaseType::Contract(c2)) => {
                if c1 == c2 {
                    Ok(())
                } else {
                    Err(MerakError::TypeMismatch {
                        expected: c1.to_string(),
                        found: c2.to_string(),
                        source_ref: SourceRef::unknown(),
                    })
                }
            }
            _ => Err(MerakError::TypeMismatch {
                expected: expected_type.to_string(),
                found: inferred_type.to_string(),
                source_ref: SourceRefGuard::current(), // Automatically uses the current source_ref from the guard
            }),
        }
    }

    // Requires `&mut self` because it may recursively call `infer_basetype` on nested expressions,
    // which needs mutable access for MemberCall resolution.
    fn assert_binary_type(
        &mut self,
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

    // Requires `&mut self` for consistency with `assert_binary_type` and potential future
    // extensions that may need mutable access during type checking.
    fn assert_unary_type(
        &mut self,
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
