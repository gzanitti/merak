use merak_ast::contract::{Contract, Program, StateDef};
use merak_ast::expression::Expression;
use merak_ast::function::{Function, Modifier, Visibility};
use merak_ast::statement::{Block, Statement};
use merak_ast::types::Type;
use merak_ast::NodeIdGenerator;
use merak_errors::MerakError;
use merak_ir::ssa_ir::{SsaCfg, SsaContract};
use merak_symbols::{QualifiedName, SymbolKind, SymbolNamespace, SymbolTable};

mod refinements;
mod storage;
mod typechecker;
use typechecker::Typechecker;

use crate::storage::analyze_storage;

pub fn analyze(program: &Program) -> Result<SymbolTable, MerakError> {
    let mut errors = Vec::new();
    let mut symbol_table = SymbolTable::new();

    // For each contract, collect symbols then resolve references
    for (_contract_name, contract) in &program.contracts {
        // Phase 1: Collect symbol definitions for this contract
        if let Err(e) = collect_and_resolve_contract(contract, &mut symbol_table, &mut errors) {
            errors.push(e);
        }
    }

    // Return error if any were collected
    if !errors.is_empty() {
        return Err(errors.into_iter().next().unwrap());
    }

    // PHASE 3: Type checking
    println!("Start analyzing contracts...");
    let symbol_table = Typechecker::new(symbol_table).check(&program)?;
    println!("Finished analyzing contracts...");

    Ok(symbol_table)
}

pub fn analyze_ssa(
    contract: SsaContract,
    cfg: SsaCfg,
    symbol_table: &mut SymbolTable,
) -> Result<(), MerakError> {
    let storage_results = analyze_storage(&contract, &cfg, &symbol_table)?;
    //analyze_refinements(&cfg, symbol_table)
    Ok(())
}

fn collect_and_resolve_contract(
    contract: &Contract,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
) -> Result<(), MerakError> {
    let contract_name = &contract.data.name;

    // Save the current scope (should be global)
    let global_scope = symbol_table.get_current_scope();

    // Add contract itself
    let temp_gen = NodeIdGenerator::new();
    let contract_node_id = temp_gen.next();

    let contract_qname = QualifiedName::from_string(contract_name.clone());
    let _ = symbol_table
        .add_symbol(
            contract_node_id,
            contract_qname.clone(),
            SymbolKind::Contract {
                states: contract.data.states.clone(),
            },
            None,
        )
        .map_err(|e| errors.push(e));

    // Enter contract scope
    let _ = symbol_table.push_scope();

    // Add state variables
    for state_var in &contract.data.variables {
        let var_qname = QualifiedName::new(vec![contract_name.clone(), state_var.name.clone()]);
        if state_var.ty.constraint.contains_old() {
            errors.push(MerakError::OldInvalidUse {
                source_ref: state_var.source_ref.clone(),
            });
        }
        let _ = symbol_table
            .add_symbol(
                state_var.id(),
                var_qname,
                SymbolKind::StateVar,
                Some(state_var.ty.clone()),
            )
            .map_err(|e| errors.push(e));
    }

    // Add state constants
    for state_const in &contract.data.constants {
        let const_qname = QualifiedName::new(vec![contract_name.clone(), state_const.name.clone()]);
        if state_const.ty.constraint.contains_old() {
            errors.push(MerakError::OldInvalidUse {
                source_ref: state_const.source_ref.clone(),
            });
        }
        let _ = symbol_table
            .add_symbol(
                state_const.id(),
                const_qname,
                SymbolKind::StateConst,
                Some(state_const.ty.clone()),
            )
            .map_err(|e| errors.push(e));
    }

    // Handle constructor
    if let Some(constructor) = &contract.data.constructor {
        // Register constructor itself in symbol table
        let constructor_qname =
            QualifiedName::new(vec![contract_name.clone(), "constructor".to_string()]);

        let _ = symbol_table
            .add_symbol(
                constructor.id(),
                constructor_qname,
                SymbolKind::Constructor {
                    contract: contract_name.clone(),
                },
                None, // Constructors have no return type
            )
            .map_err(|e| errors.push(e));

        let _ = symbol_table.push_scope();

        // Add parameters
        for param in &constructor.params {
            let param_qname = QualifiedName::new(vec![
                contract_name.clone(),
                "constructor".to_string(),
                param.name.clone(),
            ]);
            if param.ty.constraint.contains_old() {
                errors.push(MerakError::OldInvalidUse {
                    source_ref: param.source_ref.clone(),
                });
            }
            let _ = symbol_table
                .add_symbol(
                    param.id(),
                    param_qname,
                    SymbolKind::Parameter,
                    Some(param.ty.clone()),
                )
                .map_err(|e| errors.push(e));
        }

        // Collect symbols from constructor body
        collect_and_resolve_block(
            &constructor.body,
            contract_name,
            &["constructor"],
            symbol_table,
            errors,
            &contract.data.states,
        );

        // Pop constructor scope
        symbol_table.pop_scope();
    }

    // Validate and collect/resolve state definitions
    let mut defined_states = std::collections::HashSet::new();

    // Phase 1: Add all state symbols to the symbol table first
    // This ensures all states are available before resolving any references (forward references)
    for (state_name, state_def) in &contract.state_defs {
        // Validate state is declared
        if !contract.data.states.contains(state_name) {
            errors.push(MerakError::UndeclaredState {
                state: state_name.clone(),
                source_ref: state_def.source_ref.clone(),
            });
        }

        defined_states.insert(state_name.clone());

        // Add state symbol
        let temp_state_id = temp_gen.next();
        let state_qname = QualifiedName::new(vec![contract_name.clone(), state_name.clone()]);
        let _ = symbol_table
            .add_symbol(
                temp_state_id,
                state_qname.clone(),
                SymbolKind::State {
                    contract: contract_name.clone(),
                },
                None,
            )
            .map_err(|e| errors.push(e));
    }

    // Phase 2: Process functions in each state
    // Now all state symbols are in the table, so 'become' statements can reference any state
    for (state_name, state_def) in &contract.state_defs {
        collect_and_resolve_state(
            state_def,
            contract_name,
            state_name,
            symbol_table,
            errors,
            &contract.data.states,
        );
    }

    // Validate all declared states have definitions
    for declared_state in &contract.data.states {
        if !defined_states.contains(declared_state) {
            errors.push(MerakError::UndefinedState {
                state: declared_state.clone(),
                source_ref: merak_ast::meta::SourceRef { start: 0, end: 0 },
            });
        }
    }

    // Pop contract scope
    symbol_table.pop_scope();

    // Restore global scope
    symbol_table.set_current_scope(global_scope);

    if !errors.is_empty() {
        // TODO: Vec<MErakError>
        return Err(errors.swap_remove(0));
    }

    Ok(())
}

fn collect_and_resolve_state(
    state_def: &StateDef,
    contract_name: &str,
    state_name: &str,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
    valid_states: &[String],
) {
    let mut declared_functions = std::collections::HashSet::new();
    for function in &state_def.functions {
        if declared_functions.contains(&function.name) {
            errors.push(MerakError::FunctionRedefinition {
                name: function.name.clone(),
                state: state_name.to_string(),
                source_ref: function.source_ref.clone(),
            });
        }

        // Old(..) is only valid on function ensure
        for p in &function.requires {
            if p.contains_old() {
                errors.push(MerakError::OldInvalidUse {
                    source_ref: p.source_ref().clone(),
                });
            }
        }

        declared_functions.insert(function.name.clone());

        let func_qname = QualifiedName::new(vec![
            contract_name.to_string(),
            state_name.to_string(),
            function.name.clone(),
        ]);

        // TODO: This functrion should return Result<(), MerakError>
        let reentrancy = validate_modifiers(function);

        let kind = match function.visibility {
            Visibility::Entrypoint => SymbolKind::Entrypoint {
                state: state_name.to_string(),
                reentrancy,
                parameters: function.params.clone(),
                return_type: function
                    .return_type
                    .clone()
                    .unwrap_or_else(|| Type::empty_tuple("__self".to_string())),
            },
            Visibility::Internal | Visibility::External => SymbolKind::Function {
                state: state_name.to_string(),
                visibility: function.visibility.clone(),
                reentrancy,
                parameters: function.params.clone(),
                return_type: function
                    .return_type
                    .clone()
                    .unwrap_or_else(|| Type::empty_tuple("__self".to_string())),
            },
        };

        let _ = symbol_table
            .add_symbol(
                function.id(),
                func_qname.clone(),
                kind,
                function.return_type.clone(),
            )
            .map_err(|e| errors.push(e));

        // Push scope for function
        symbol_table.push_scope();

        // Add parameters
        for param in &function.params {
            let param_qname = QualifiedName::new(vec![
                contract_name.to_string(),
                state_name.to_string(),
                function.name.clone(),
                param.name.clone(),
            ]);
            if param.ty.constraint.contains_old() {
                errors.push(MerakError::OldInvalidUse {
                    source_ref: param.source_ref.clone(),
                });
            }
            let _ = symbol_table
                .add_symbol(
                    param.id(),
                    param_qname,
                    SymbolKind::Parameter,
                    Some(param.ty.clone()),
                )
                .map_err(|e| errors.push(e));
        }

        let path_components = vec![state_name, &function.name];
        collect_and_resolve_block(
            &function.body,
            contract_name,
            &path_components,
            symbol_table,
            errors,
            valid_states,
        );

        // Pop function scope
        symbol_table.pop_scope();
    }
}

/// Validates function modifiers for correctness
fn validate_modifiers(function: &Function) -> Modifier {
    let has_guarded = function.modifiers.contains(&Modifier::Guarded);
    let has_reentrant = function.modifiers.contains(&Modifier::Reentrant);
    let is_internal = function.visibility == Visibility::Internal;

    // Rule 1: guarded and reentrant are mutually exclusive
    if has_guarded && has_reentrant {
        // return Err(MerakError::SemanticError(format!(
        //     "Function '{}' cannot have both 'guarded' and 'reentrant' modifiers. \
        //         These reentrancy modes are mutually exclusive at {}",
        //     function.name, function.source_ref
        // )));
        panic!(
            "Function '{}' cannot have both 'guarded' and 'reentrant' modifiers. \
                These reentrancy modes are mutually exclusive at {}. ¡¡TODO: Make this an error!!",
            function.name, function.source_ref
        );
    }

    // Rule 2: guarded and reentrant only on external/entrypoint functions
    if is_internal && has_guarded {
        // return Err(MerakError::SemanticError(format!(
        //         "Internal function '{}' cannot have 'guarded' modifier. \
        //         Reentrancy protection is only applicable to external functions and entrypoints at {}",
        //         function.name, function.source_ref
        //     )));
        panic!(
                "Internal function '{}' cannot have 'guarded' modifier. \
                Reentrancy protection is only applicable to external functions and entrypoints at {}. ¡¡TODO: Make this an error!!",
                function.name, function.source_ref
            );
    }

    if is_internal && has_reentrant {
        // return Err(MerakError::SemanticError(format!(
        //     "Internal function '{}' cannot have 'reentrant' modifier. \
        //         Reentrancy control es only applicable to external functions and entrypoints at {}",
        //     function.name, function.source_ref
        // )));
        panic!(
            "Internal function '{}' cannot have 'reentrant' modifier. \
                Reentrancy control is only applicable to external functions and entrypoints at {}. ¡¡TODO: Make this an error!!",
            function.name, function.source_ref
        );
    }

    // Rule 3: Check for duplicate modifiers
    let mut seen_modifiers = std::collections::HashSet::new();
    for modifier in &function.modifiers {
        if !seen_modifiers.insert(modifier) {
            // return Err(MerakError::SemanticError(format!(
            //     "Function '{}' has duplicate modifier '{:?}' at {}",
            //     function.name, modifier, function.source_ref
            // )));
            panic!(
                "Function '{}' has duplicate modifier '{:?}' at {}. ¡¡TODO: Make this an error!!",
                function.name, modifier, function.source_ref
            );
        }
    }

    if seen_modifiers.len() == 0 {
        Modifier::Checked
    } else if seen_modifiers.len() == 1 {
        seen_modifiers.into_iter().next().unwrap().clone()
    } else {
        panic!(
            "Function '{}' has more than one unique modifier at {}",
            function.name, function.source_ref
        )
    }
}

fn collect_and_resolve_block(
    block: &Block,
    contract_name: &str,
    path: &[&str],
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
    valid_states: &[String],
) {
    for statement in &block.statements {
        collect_and_resolve_statement(
            statement,
            contract_name,
            path,
            symbol_table,
            errors,
            valid_states,
        );
    }
}

fn collect_and_resolve_statement(
    statement: &Statement,
    contract_name: &str,
    path: &[&str],
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
    valid_states: &[String],
) {
    match statement {
        Statement::VarDeclaration {
            name, ty, id, expr, ..
        } => {
            if let Some(ty) = ty {
                if ty.constraint.contains_old() {
                    errors.push(MerakError::OldInvalidUse {
                        source_ref: ty.source_ref.clone(),
                    });
                }
            }

            resolve_names_in_expression(expr, symbol_table, errors);

            let mut parts = vec![contract_name.to_string()];
            parts.extend(path.iter().map(|s| s.to_string()));
            parts.push(name.clone());
            let var_qname = QualifiedName::new(parts);
            let _ = symbol_table
                .add_symbol(*id, var_qname, SymbolKind::LocalVar, ty.clone()) // Asumo LocalVar o similar
                .map_err(|e| errors.push(e));
        }
        Statement::ConstDeclaration {
            name, ty, id, expr, ..
        } => {
            if let Some(ty) = ty {
                if ty.constraint.contains_old() {
                    errors.push(MerakError::OldInvalidUse {
                        source_ref: ty.source_ref.clone(),
                    });
                }
            }
            resolve_names_in_expression(expr, symbol_table, errors);

            let mut parts = vec![contract_name.to_string()];
            parts.extend(path.iter().map(|s| s.to_string()));
            parts.push(name.clone());
            let const_qname = QualifiedName::new(parts);
            let _ = symbol_table
                .add_symbol(*id, const_qname, SymbolKind::LocalVar, ty.clone()) // Asumo LocalVar o similar
                .map_err(|e| errors.push(e));
        }
        Statement::Assignment {
            target,
            expr,
            id,
            source_ref,
        } => {
            resolve_names_in_expression(expr, symbol_table, errors);

            if symbol_table
                .resolve_reference(*id, target, SymbolNamespace::Value)
                .is_none()
            {
                errors.push(MerakError::UndefinedVariable {
                    name: target.clone(),
                    source_ref: source_ref.clone(),
                });
            }
        }
        Statement::If {
            condition,
            then_block,
            else_block,
            ..
        } => {
            resolve_names_in_expression(condition, symbol_table, errors);

            symbol_table.push_scope();
            collect_and_resolve_block(
                then_block,
                contract_name,
                path,
                symbol_table,
                errors,
                valid_states,
            );
            symbol_table.pop_scope();

            if let Some(else_blk) = else_block {
                symbol_table.push_scope();
                collect_and_resolve_block(
                    else_blk,
                    contract_name,
                    path,
                    symbol_table,
                    errors,
                    valid_states,
                );
                symbol_table.pop_scope();
            }
        }
        Statement::While {
            condition, body, ..
        } => {
            resolve_names_in_expression(condition, symbol_table, errors);

            symbol_table.push_scope();
            collect_and_resolve_block(
                body,
                contract_name,
                path,
                symbol_table,
                errors,
                valid_states,
            );
            symbol_table.pop_scope();
        }
        Statement::Become(target_state, id, source_ref) => {
            if !valid_states.contains(target_state) {
                errors.push(MerakError::UndefinedState {
                    state: target_state.clone(),
                    source_ref: source_ref.clone(),
                });
            }
            if symbol_table
                .resolve_reference(*id, target_state, SymbolNamespace::Type)
                .is_none()
            {
                errors.push(MerakError::UndefinedState {
                    state: target_state.clone(),
                    source_ref: source_ref.clone(),
                });
            }
        }
        Statement::Return(expr, ..) => {
            if let Some(ref return_expr) = expr {
                resolve_names_in_expression(return_expr, symbol_table, errors);
            }
        }
        Statement::Expression(expr, ..) => {
            resolve_names_in_expression(expr, symbol_table, errors);
        }
    }
}

fn resolve_names_in_expression(
    expr: &Expression,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
) {
    match expr {
        Expression::Identifier(name, id, source_ref) => {
            // Resolve variable reference
            if symbol_table
                .resolve_reference(*id, name, SymbolNamespace::Value)
                .is_none()
            {
                errors.push(MerakError::UndefinedVariable {
                    name: name.clone(),
                    source_ref: source_ref.clone(),
                });
            }
        }
        Expression::FunctionCall {
            name,
            args,
            id,
            source_ref,
        } => {
            // Resolve function reference
            if symbol_table
                .resolve_reference(*id, name, SymbolNamespace::Callable)
                .is_none()
            {
                errors.push(MerakError::UndefinedFunction {
                    name: name.clone(),
                    source_ref: source_ref.clone(),
                });
            }
            // Resolve references in arguments
            for arg in args {
                resolve_names_in_expression(arg, symbol_table, errors);
            }
        }
        Expression::BinaryOp { left, right, .. } => {
            resolve_names_in_expression(left, symbol_table, errors);
            resolve_names_in_expression(right, symbol_table, errors);
        }
        Expression::UnaryOp { expr: inner, .. } => {
            resolve_names_in_expression(inner, symbol_table, errors);
        }
        Expression::Literal(..) => {
            // Literals don't reference symbols
        }
        Expression::Grouped(inner, ..) => {
            // Resolve the grouped expression
            resolve_names_in_expression(inner, symbol_table, errors);
        }
    }
}
