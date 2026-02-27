use merak_ast::contract::{File, Program};
use merak_ast::expression::{Expression};
use merak_ast::function::{Function, Modifier, Visibility};
use merak_ast::statement::{Block, Statement};
use merak_ast::types::Type;
use merak_errors::MerakError;
use merak_ir::ssa_ir::SsaContract;
use merak_symbols::{QualifiedName, SymbolId, SymbolKind, SymbolNamespace, SymbolTable};

pub mod refinements;
pub mod storage;
mod typechecker;
use typechecker::Typechecker;


pub fn analyze(program: &Program) -> Result<SymbolTable, MerakError> {
    let mut errors = Vec::new();
    let mut symbol_table = SymbolTable::new();

    // For each contract, collect symbols then resolve references
    for (contract_name, file) in &program.files {
        println!("Collecting symbols for {contract_name}");

        for import in &file.imports {
            if let Some(alias) = &import.alias {
                let symbol_id = symbol_table.lookup(&import.contract_name, SymbolNamespace::Type).unwrap();
                symbol_table.insert(alias.clone(), SymbolNamespace::Type, symbol_id);
            }
        }
        collect_and_resolve_interfaces(file, &mut symbol_table, &mut errors);
        if let Err(e) = collect_and_resolve_contract(file, &mut symbol_table, &mut errors) {
            errors.push(e);
        }
        if !errors.is_empty() {
            return Err(errors.into_iter().next().unwrap());
        }
        
        
        symbol_table.clean_scopes();
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
    contract: &mut SsaContract,
    symbol_table: &SymbolTable,
) -> Result<(), MerakError> {
    storage::analyze_storage_contract(contract, symbol_table)?;
    // TODO: refinements::analyze_refinements(contract, symbol_table)?;
    Ok(())
}

fn collect_and_resolve_contract(
    file: &File,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
) -> Result<(), MerakError> {
    let contract_name = &file.contract.name;
    let contract_node_id = file.contract.id;
    // Save the current scope (should be global)
    //let global_scope = symbol_table.get_current_scope();

    let contract_qname = QualifiedName::from_string(contract_name.clone());
    let _ = symbol_table
        .add_symbol(
            contract_node_id,
            contract_qname.clone(),
            SymbolKind::Contract,
            None,
        )
        .map_err(|e| errors.push(e));

    // Enter contract scope
    let _ = symbol_table.push_scope();

    // Add state variables
    for state_var in &file.contract.variables {
        let var_qname = QualifiedName::new(vec![contract_name.clone(), state_var.name.clone()]);
        if state_var.ty.constraint.contains_old() {
            errors.push(MerakError::OldInvalidUse {
                source_ref: state_var.source_ref.clone(),
            });
        }
        
        resolve_names_in_expression(&state_var.expr, symbol_table, errors);

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
    for state_const in &file.contract.constants {
        let const_qname = QualifiedName::new(vec![contract_name.clone(), state_const.name.clone()]);
        if state_const.ty.constraint.contains_old() {
            errors.push(MerakError::OldInvalidUse {
                source_ref: state_const.source_ref.clone(),
            });
        }

        resolve_names_in_expression(&state_const.expr, symbol_table, errors);

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
    if let Some(constructor) = &file.contract.constructor {
        // Register constructor itself in symbol table
        let constructor_qname =
            QualifiedName::new(vec![contract_name.clone(), "constructor".to_string()]);

        let _ = symbol_table
            .add_symbol(
                constructor.id(),
                constructor_qname,
                SymbolKind::ContractInit {
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
            &vec![contract_name.to_string(), "constructor".to_string()],
            symbol_table,
            errors,
        );

        // Pop constructor scope
        symbol_table.pop_scope();
    }

    let mut declared_functions = std::collections::HashSet::new();
    for function in &file.contract.functions {
        if declared_functions.contains(&function.name) {
            errors.push(MerakError::FunctionRedefinition {
                name: function.name.clone(),
                source_ref: function.source_ref.clone(),
            });
        }
        collect_and_resolve_function(
            function,
            contract_name.to_string(),
            symbol_table,
            errors,
        );

        if !errors.is_empty() {
            // TODO: Vec<MErakError>
            return Err(errors.swap_remove(0));
        }

        declared_functions.insert(function.name.clone());
    }

    // Pop contract scope
    symbol_table.pop_scope();

    // Restore global scope
    //symbol_table.set_current_scope(global_scope);

    Ok(())
}

fn collect_and_resolve_interfaces(
    file: &File,
    symbol_table: &mut SymbolTable, 
    errors: &mut Vec<MerakError>) 
{
    for interface in &file.interfaces {
        let interface_qname = QualifiedName::new(vec![file.contract.name.clone(), interface.name.clone()]);

        let mut interface_fn_symbols = vec![];
        for interface_fn in &interface.functions {
            let interface_fn_qname = QualifiedName::new(vec![
                file.contract.name.clone(),
                interface.name.clone(),
                interface_fn.name.clone(),
            ]);

            let return_type = match interface_fn.return_type.clone() {
                Some(ty) => ty,
                None => Type::empty_tuple("__self".to_string()),
            };
            
            let symbol_fn = symbol_table
                .add_symbol(
                    interface_fn.id.clone(),
                    interface_fn_qname,
                    SymbolKind::InterfaceFunction {
                        params: interface_fn.params.clone(),
                        return_type: return_type,
                    },
                None)
                .unwrap_or_else(|e| {
                    errors.push(e);
                    SymbolId::new("".to_string(), 0) // default value in case of error
                });

            interface_fn_symbols.push(symbol_fn);
        }

        let _ = symbol_table
            .add_symbol(
                interface.id.clone(),
                interface_qname,
                SymbolKind::Interface { functions: interface_fn_symbols },
                None,
            )
            .map_err(|e| errors.push(e));
    }
}



fn collect_and_resolve_function(
    function: &Function,
    contract_name: String,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
) {        
        // Old(..) is only valid on function ensure
        for p in &function.requires {
            if p.contains_old() {
                errors.push(MerakError::OldInvalidUse {
                    source_ref: p.source_ref().clone(),
                });
            }
        }

        let func_qname = QualifiedName::new(vec![
            contract_name.to_string(),
            function.name.clone(),
        ]);

        let reentrancy = match validate_modifiers(function) {
            Ok(r) => r,
            Err(e) => {
                errors.push(e);
                return;
            }
        };


        let kind = match function.visibility {
            Visibility::Entrypoint => SymbolKind::Entrypoint {
                reentrancy,
                parameters: function.params.clone(),
                ensures: function.ensures.clone(),
                requires: function.requires.clone(),
                return_type: function
                    .return_type
                    .clone()
                    .unwrap_or_else(|| Type::empty_tuple("__self".to_string())),
            },
            Visibility::Internal | Visibility::External => SymbolKind::Function {
                visibility: function.visibility.clone(),
                reentrancy,
                parameters: function.params.clone(),
                ensures: function.ensures.clone(),
                requires: function.requires.clone(),
                return_type: function
                    .return_type
                    .clone()
                    .unwrap_or_else(|| Type::empty_tuple("__self".to_string())),
            },
        };


        let func_symbol_id = symbol_table
            .add_symbol(
                function.id(),
                func_qname.clone(),
                kind,
                function.return_type.clone(),
            );

        let func_symbol_id = match func_symbol_id {
            Ok(id) => Some(id),
            Err(e) => {
                errors.push(e);
                None
            }
        };

        // Push scope for function
        symbol_table.push_scope();

        // Add parameters
        for param in &function.params {
            let param_qname = QualifiedName::new(vec![
                contract_name.to_string(),
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

        let path_components = vec![contract_name, function.name.clone()];
        collect_and_resolve_block(
            &function.body,
            &path_components,
            symbol_table,
            errors,
        );

        // Normalize return type binder: after processing the body,
        // all params and local vars are in scope, so we can determine
        // which variables in the return type constraint are the binder.
        if let (Some(ref func_sid), Some(ref return_type)) = (&func_symbol_id, &function.return_type) {
            match normalize_return_type_binder(return_type, symbol_table) {
                Ok(normalized) => {
                    let sym = symbol_table.get_symbol_mut(func_sid.clone());
                    match &mut sym.kind {
                        SymbolKind::Function { return_type, .. }
                        | SymbolKind::Entrypoint { return_type, .. } => {
                            *return_type = normalized.clone();
                        }
                        _ => {}
                    }
                    sym.ty = Some(normalized);
                }
                Err(e) => errors.push(e),
            }
        }

        // Pop function scope
        symbol_table.pop_scope();
    
}

/// Validates function modifiers for correctness
fn validate_modifiers(function: &Function) -> Result<Modifier, MerakError> {
    let has_guarded = function.modifiers.contains(&Modifier::Guarded);
    let has_reentrant = function.modifiers.contains(&Modifier::Reentrant);
    let is_internal = function.visibility == Visibility::Internal;

    // Rule 1: guarded and reentrant are mutually exclusive
    if has_guarded && has_reentrant {
        return Err(MerakError::SemanticError(format!(
            "Function '{}' cannot have both 'guarded' and 'reentrant' modifiers. \
                These reentrancy modes are mutually exclusive at {}",
            function.name, function.source_ref
        )));
        // panic!(
        //     "Function '{}' cannot have both 'guarded' and 'reentrant' modifiers. \
        //         These reentrancy modes are mutually exclusive at {}. ¡¡TODO: Make this an error!!",
        //     function.name, function.source_ref
        // );
    }

    // Rule 2: guarded and reentrant only on external/entrypoint functions
    if is_internal && has_guarded {
        return Err(MerakError::SemanticError(format!(
                "Internal function '{}' cannot have 'guarded' modifier. \
                Reentrancy protection is only applicable to external functions and entrypoints at {}",
                function.name, function.source_ref
            )));
        // panic!(
        //         "Internal function '{}' cannot have 'guarded' modifier. \
        //         Reentrancy protection is only applicable to external functions and entrypoints at {}. ¡¡TODO: Make this an error!!",
        //         function.name, function.source_ref
        //     );
    }

    if is_internal && has_reentrant {
        return Err(MerakError::SemanticError(format!(
            "Internal function '{}' cannot have 'reentrant' modifier. \
                Reentrancy control es only applicable to external functions and entrypoints at {}",
            function.name, function.source_ref
        )));
        // panic!(
        //     "Internal function '{}' cannot have 'reentrant' modifier. \
        //         Reentrancy control is only applicable to external functions and entrypoints at {}. ¡¡TODO: Make this an error!!",
        //     function.name, function.source_ref
        // );
    }

    // Rule 3: Check for duplicate modifiers
    let mut seen_modifiers = std::collections::HashSet::new();
    for modifier in &function.modifiers {
        if !seen_modifiers.insert(modifier) {
            return Err(MerakError::SemanticError(format!(
                "Function '{}' has duplicate modifier '{:?}' at {}",
                function.name, modifier, function.source_ref
            )));
            // panic!(
            //     "Function '{}' has duplicate modifier '{:?}' at {}. ¡¡TODO: Make this an error!!",
            //     function.name, modifier, function.source_ref
            // );
        }
    }

    if seen_modifiers.len() == 0 {
        Ok(Modifier::Checked)
    } else if seen_modifiers.len() == 1 {
        Ok(seen_modifiers.into_iter().next().unwrap().clone())
    } else {
        Err(MerakError::SemanticError(format!(
            "Function '{}' has more than one unique modifier at {}",
            function.name, function.source_ref
        )))
    }
}

/// Normalizes the binder variable in a function return type refinement.
///
/// Handles two cases:
/// 1. Explicit binder (`{v: int | v > 0}`): `v` is the binder, substitute `v` → `__self`.
/// 2. Implicit binder (`{int | v > 0}`): binder is already `__self`, but the constraint
///    uses `v`. We use the symbol table to find variables in the constraint that are
///    NOT declared in any scope → that's the binder. Must be exactly 0 or 1.
fn normalize_return_type_binder(
    return_type: &Type,
    symbol_table: &SymbolTable,
) -> Result<Type, MerakError> {
    let mut result = return_type.clone();

    if result.binder != "__self" {
        // Explicit binder: {v: int | v > 0} → substitute v → __self
        let mut subst = std::collections::HashMap::new();
        subst.insert(result.binder.clone(), "__self".to_string());
        result.constraint = result.constraint.substitute_vars(&subst);
        result.binder = "__self".to_string();
    } else {
        // Implicit binder: {int | v > 0} → find undeclared variables
        let free_vars = result.constraint.free_variables();
        let undeclared: Vec<String> = free_vars
            .into_iter()
            .filter(|var| var != "__self")
            .filter(|var| symbol_table.lookup(var, SymbolNamespace::Value).is_none())
            .collect();

        if undeclared.len() == 1 {
            let mut subst = std::collections::HashMap::new();
            subst.insert(undeclared[0].clone(), "__self".to_string());
            result.constraint = result.constraint.substitute_vars(&subst);
        } else if undeclared.len() > 1 {
            return Err(MerakError::SemanticError(format!(
                "Ambiguous binder in return type refinement: variables {:?} are not declared in scope. \
                 Expected exactly one binder variable. Use explicit binder syntax like {{v: int | v > 0}} at {}",
                undeclared, return_type.source_ref
            )));
        }
        // undeclared.len() == 0: no binder variable found, constraint uses only known variables
    }

    Ok(result)
}

fn collect_and_resolve_block(
    block: &Block,
    path: &Vec<String>,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
) {
    for statement in &block.statements {
        collect_and_resolve_statement(
            statement,
            path,
            symbol_table,
            errors,
        );
    }
}

fn collect_and_resolve_statement(
    statement: &Statement,
    path: &Vec<String>,
    symbol_table: &mut SymbolTable,
    errors: &mut Vec<MerakError>,
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

            let var_qname = QualifiedName::new(
                path.iter()
                    .cloned()
                    .chain(std::iter::once(name.clone()))
                    .collect()
            );
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

            let const_qname = QualifiedName::new(
                path.iter()
                    .cloned()
                    .chain(std::iter::once(name.clone()))
                    .collect()
            );
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
                path,
                symbol_table,
                errors,
            );
            symbol_table.pop_scope();

            if let Some(else_blk) = else_block {
                symbol_table.push_scope();
                collect_and_resolve_block(
                    else_blk,
                    path,
                    symbol_table,
                    errors,
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
                path,
                symbol_table,
                errors,
            );
            symbol_table.pop_scope();
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

            if symbol_table
                .resolve_reference(*id, name, SymbolNamespace::Callable) 
                .is_none()
            {
                if symbol_table.resolve_reference(*id, name, SymbolNamespace::Type).is_none() {
                    println!("Symbol table {:?}", symbol_table);
                    errors.push(MerakError::UndefinedFunction {
                        name: name.clone(),
                        source_ref: source_ref.clone(),
                    });
                }
            }
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
        Expression::Literal(..) => {}
        Expression::Grouped(inner, ..) => {
            resolve_names_in_expression(inner, symbol_table, errors);
        }
        Expression::MemberCall { object, args, .. } => {
            resolve_names_in_expression(object, symbol_table, errors);
    
            for arg in args {
                resolve_names_in_expression(arg, symbol_table, errors);
            }
        }
    }
}
