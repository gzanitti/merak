use indexmap::IndexMap;
use merak_analyzer::analyze;
use merak_ast::contract::Program;
use merak_ast::types::BaseType;
use merak_parser::parse_program;
use merak_symbols::SymbolKind;

// ============================================================================
// HELPER MACROS
// ============================================================================

macro_rules! test_success {
    ($name:ident, $input:expr, $checks:expr) => {
        #[test]
        fn $name() {
            let contract = parse_program($input).expect("Failed to parse");
            let mut contracts = IndexMap::new();
            contracts.insert(contract.data.name.clone(), contract);
            let program = Program { contracts };
            let result = analyze(&program);
            assert!(
                result.is_ok(),
                "Expected success but got error: {:?}",
                result.err()
            );

            let symbol_table = result.unwrap();
            $checks(&symbol_table);
        }
    };
}

macro_rules! test_error {
    ($name:ident, $input:expr) => {
        #[test]
        fn $name() {
            let contract = parse_program($input).expect("Failed to parse");
            let mut contracts = IndexMap::new();
            contracts.insert(contract.data.name.clone(), contract);
            let program = Program { contracts };
            let result = analyze(&program);
            assert!(result.is_err(), "Expected error but analysis succeeded");
        }
    };
}

// ============================================================================
// HELPER FUNCTIONS FOR ROBUST SYMBOL VERIFICATION
// ============================================================================

/// Verify a symbol exists with the expected SymbolKind and qualified name
fn assert_symbol_with_kind(
    table: &merak_symbols::SymbolTable,
    simple_name: &str,
    expected_kind: SymbolKind,
    expected_qualified_name: &str,
) {
    let symbols = table.find_symbols_by_name(simple_name);

    assert!(
        !symbols.is_empty(),
        "Symbol '{}' not found in symbol table",
        simple_name
    );

    let matching = symbols.iter().find(|(_, _, info)| {
        info.kind == expected_kind && info.qualified_name.to_string() == expected_qualified_name
    });

    assert!(
        matching.is_some(),
        "Symbol '{}' found but with wrong kind or qualified name.\nExpected: kind={:?}, qname={}\nFound: {:?}",
        simple_name,
        expected_kind,
        expected_qualified_name,
        symbols.iter().map(|(_, _, info)| (&info.kind, info.qualified_name.to_string())).collect::<Vec<_>>()
    );
}

/// Verify a state variable exists with correct properties
fn assert_state_var(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
    base_type: BaseType,
) {
    assert_symbol_with_kind(table, name, SymbolKind::StateVar, qualified_name);

    // Also verify the type
    let symbols = table.find_symbols_by_name(name);
    let with_type = symbols.iter().find(|(_, _, info)| {
        info.ty
            .as_ref()
            .map(|t| t.base == base_type)
            .unwrap_or(false)
    });

    assert!(
        with_type.is_some(),
        "State var '{}' found but with wrong type. Expected base type: {:?}",
        name,
        base_type
    );
}

/// Verify a state constant exists with correct properties
fn assert_state_const(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
    base_type: BaseType,
) {
    assert_symbol_with_kind(table, name, SymbolKind::StateConst, qualified_name);

    // Also verify the type
    let symbols = table.find_symbols_by_name(name);
    let with_type = symbols.iter().find(|(_, _, info)| {
        info.ty
            .as_ref()
            .map(|t| t.base == base_type)
            .unwrap_or(false)
    });

    assert!(
        with_type.is_some(),
        "State const '{}' found but with wrong type. Expected base type: {:?}",
        name,
        base_type
    );
}

/// Verify an entrypoint exists with correct state
fn assert_entrypoint(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
    state: &str,
) {
    let symbols = table.find_symbols_by_name(name);

    assert!(
        !symbols.is_empty(),
        "Entrypoint '{}' not found in symbol table",
        name
    );

    let matching = symbols.iter().find(|(_, _, info)| {
        matches!(&info.kind, SymbolKind::Entrypoint { state: s, .. } if s == state)
            && info.qualified_name.to_string() == qualified_name
    });

    assert!(
        matching.is_some(),
        "Entrypoint '{}' found but with wrong state or qualified name.\nExpected: state={}, qname={}\nFound: {:?}",
        name, state, qualified_name,
        symbols.iter().map(|(_, _, info)| (&info.kind, info.qualified_name.to_string())).collect::<Vec<_>>()
    );
}

/// Verify a function exists with correct state
fn assert_function(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
    state: &str,
) {
    let symbols = table.find_symbols_by_name(name);

    assert!(
        !symbols.is_empty(),
        "Function '{}' not found in symbol table",
        name
    );

    let matching = symbols.iter().find(|(_, _, info)| {
        matches!(&info.kind, SymbolKind::Function { state: s, .. } if s == state)
            && info.qualified_name.to_string() == qualified_name
    });

    assert!(
        matching.is_some(),
        "Function '{}' found but with wrong state or qualified name.\nExpected: state={}, qname={}\nFound: {:?}",
        name, state, qualified_name,
        symbols.iter().map(|(_, _, info)| (&info.kind, info.qualified_name.to_string())).collect::<Vec<_>>()
    );
}

/// Verify a constructor exists
fn assert_constructor(table: &merak_symbols::SymbolTable, contract: &str) {
    let symbols = table.find_symbols_by_name("constructor");

    assert!(!symbols.is_empty(), "Constructor not found in symbol table");

    let matching = symbols.iter().find(|(_, _, info)| {
        matches!(&info.kind, SymbolKind::Constructor { contract: c } if c == contract)
    });

    assert!(
        matching.is_some(),
        "Constructor found but for wrong contract. Expected: {}\nFound: {:?}",
        contract,
        symbols
            .iter()
            .map(|(_, _, info)| &info.kind)
            .collect::<Vec<_>>()
    );
}

// ============================================================================
// STATE VARIABLE AND CONSTANT TESTS
// ============================================================================

test_success!(
    state_var_registered_in_contract_scope,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint getBalance() -> int {
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "getBalance", "Test::Active::getBalance", "Active");
    }
);

test_success!(
    state_const_registered_in_contract_scope,
    r#"
    contract Test[Active] {
        state const MAX: int = 100;
    }

    Test@Active(any) {
        entrypoint getMax() -> int {
            return MAX;
        }
    }
"#,
    |table| {
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_entrypoint(table, "getMax", "Test::Active::getMax", "Active");
    }
);

test_error!(
    duplicate_state_var_names,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
        state var balance: int = 100;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    state_var_shadowing_state_const,
    r#"
    contract Test[Active] {
        state const balance: int = 0;
        state var balance: int = 100;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    state_symbols_visible_in_all_states,
    r#"
    contract Test[StateA, StateB] {
        state var x: int = 0;
    }

    Test@StateA(any) {
        entrypoint readX() -> int {
            return x;
        }
    }

    Test@StateB(any) {
        entrypoint writeX() {
            x = 10;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "readX", "Test::StateA::readX", "StateA");
        assert_entrypoint(table, "writeX", "Test::StateB::writeX", "StateB");
    }
);

// ============================================================================
// FUNCTION SYMBOL TESTS
// ============================================================================

test_success!(
    functions_registered_in_state_scope,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint doSomething() {
            return;
        }
    }
"#,
    |table| {
        assert_entrypoint(table, "doSomething", "Test::Active::doSomething", "Active");
    }
);

test_success!(
    same_function_name_in_different_states,
    r#"
    contract Test[StateA, StateB] {
        state var x: int = 0;
    }

    Test@StateA(any) {
        entrypoint action() {
            return;
        }
    }

    Test@StateB(any) {
        entrypoint actionB() {
            return;
        }
    }
"#,
    |table| {
        assert_entrypoint(table, "action", "Test::StateA::action", "StateA");
        assert_entrypoint(table, "actionB", "Test::StateB::actionB", "StateB");
    }
);

test_error!(
    same_function_name_in_same_state,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint action() {
            return;
        }

        entrypoint action() {
            return;
        }
    }
"#
);

test_success!(
    function_visibility_stored,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint entry() {
            return;
        }

        external function ext() {
            return;
        }

        internal function intern() {
            return;
        }
    }
"#,
    |table| {
        assert_entrypoint(table, "entry", "Test::Active::entry", "Active");
        assert_function(table, "ext", "Test::Active::ext", "Active");
        assert_function(table, "intern", "Test::Active::intern", "Active");
    }
);

// ============================================================================
// FUNCTION PARAMETER TESTS
// ============================================================================

test_success!(
    parameters_registered_in_function_scope,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(amount: int) {
            x = amount;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        // Note: parameter 'amount' is also registered but in function scope
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::Active::test::amount",
        );
    }
);

test_error!(
    duplicate_parameters_in_same_function,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(amount: int, amount: int) {
            return;
        }
    }
"#
);

test_success!(
    parameters_visible_in_function_body,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint deposit(amount: int) {
            balance = balance + amount;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "deposit", "Test::Active::deposit", "Active");
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::Active::deposit::amount",
        );
    }
);

test_success!(
    parameters_can_shadow_state_variables,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) -> int {
            return x;
        }
    }
"#,
    |table| {
        // Both state var and parameter 'x' should exist
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::Active::test::x");
    }
);

// ============================================================================
// LOCAL VARIABLE AND CONSTANT TESTS
// ============================================================================

test_success!(
    local_vars_registered_in_block_scope,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var y: int = 10;
            x = y;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::Active::test::y");
    }
);

test_success!(
    local_consts_registered_in_block_scope,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            const Y: int = 10;
            x = Y;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "Y", SymbolKind::LocalVar, "Test::Active::test::Y");
    }
);

test_success!(
    nested_blocks_can_shadow_outer_variables,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var x: int = 5;
            if (x > 0) {
                var x: int = 10;
                result = x;
            }
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        // Both 'x' variables should exist in different scopes
        let x_symbols = table.find_symbols_by_name("x");
        assert_eq!(
            x_symbols.len(),
            2,
            "Should have two 'x' symbols in different scopes"
        );
    }
);

test_error!(
    variable_cannot_redeclare_in_same_block,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var y: int = 0;
            var y: int = 1;
        }
    }
"#
);

// ============================================================================
// IDENTIFIER REFERENCE TESTS
// ============================================================================

test_success!(
    references_resolve_to_declarations,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            balance = 100;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
    }
);

test_success!(
    resolution_follows_scope_chain,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) {
            var y: int = x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::Active::test::x");
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::Active::test::y");
    }
);

test_error!(
    unresolved_identifiers_are_errors,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            x = undefinedVar;
        }
    }
"#
);

test_success!(
    state_variable_accessed_from_function,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint getBalance() -> int {
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "getBalance", "Test::Active::getBalance", "Active");
    }
);

test_success!(
    state_constant_accessed_from_function,
    r#"
    contract Test[Active] {
        state const MAX: int = 1000;
    }

    Test@Active(any) {
        entrypoint check() -> int {
            return MAX;
        }
    }
"#,
    |table| {
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_entrypoint(table, "check", "Test::Active::check", "Active");
    }
);

test_success!(
    parameter_accessed_in_function_body,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint set(value: int) {
            x = value;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "set", "Test::Active::set", "Active");
        assert_symbol_with_kind(
            table,
            "value",
            SymbolKind::Parameter,
            "Test::Active::set::value",
        );
    }
);

test_success!(
    local_variable_accessed_after_declaration,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var temp: int = 5;
            x = temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::Active::test::temp",
        );
    }
);

test_success!(
    multiple_references_to_same_symbol,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var temp: int = 5;
            x = temp;
            x = temp + temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::Active::test::temp",
        );
    }
);

// ============================================================================
// ERROR CASES: DUPLICATE DECLARATIONS
// ============================================================================

test_error!(
    duplicate_state_vars_detected,
    r#"
    contract Test[Active] {
        state var x: int = 0;
        state var x: int = 1;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_state_consts_detected,
    r#"
    contract Test[Active] {
        state const X: int = 0;
        state const X: int = 1;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_function_names_in_same_state_detected,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint action() {
            return;
        }
        entrypoint action() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_parameters_detected,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(param: int, param: int) {
            return;
        }
    }
"#
);

test_error!(
    duplicate_locals_in_same_block_detected,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var y: int = 0;
            var y: int = 1;
        }
    }
"#
);

// ============================================================================
// ERROR CASES: SCOPE VIOLATIONS
// ============================================================================

test_error!(
    local_variable_not_accessible_outside_function,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint funcA() {
            var local: int = 5;
        }

        entrypoint funcB() {
            x = local;
        }
    }
"#
);

test_error!(
    parameter_not_accessible_outside_function,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint funcA(param: int) {
            x = param;
        }

        entrypoint funcB() {
            x = param;
        }
    }
"#
);

// ============================================================================
// SUCCESS CASES: SHADOWING
// ============================================================================

test_success!(
    parameter_shadows_state_variable_correctly,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) -> int {
            return x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::Active::test::x");
    }
);

test_success!(
    nested_block_shadows_outer_block_correctly,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var x: int = 5;
            if (x > 0) {
                var x: int = 10;
                result = x;
            }
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        // Both 'x' variables should exist
        let x_symbols = table.find_symbols_by_name("x");
        assert_eq!(
            x_symbols.len(),
            2,
            "Should have two 'x' symbols in different scopes"
        );
    }
);

test_success!(
    after_nested_block_outer_symbol_visible_again,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var x: int = 5;
            if (x > 0) {
                var y: int = 10;
                result = y;
            }
            result = x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::Active::test::x");
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::Active::test::y");
    }
);

// ============================================================================
// SPECIAL CASES
// ============================================================================

test_success!(
    become_statement_validates_states,
    r#"
    contract Test[StateA, StateB] {
        state var x: int = 0;
    }

    Test@StateA(any) {
        entrypoint transition() {
            become StateB;
        }
    }

    Test@StateB(any) {
        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "transition", "Test::StateA::transition", "StateA");
        assert_entrypoint(table, "test", "Test::StateB::test", "StateB");
    }
);

test_error!(
    become_statement_undefined_state,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            become UndefinedState;
        }
    }
"#
);

// ============================================================================
// CONSTRUCTOR TESTS
// ============================================================================

test_success!(
    constructor_parameters_registered,
    r#"
    contract Test[Active] {
        state var balance: int = 0;

        constructor(initial: int) {
            balance = initial;
        }
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
        );
    }
);

test_success!(
    constructor_can_access_state_vars,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
        state const MAX: int = 1000;

        constructor(initial: int) {
            balance = initial;
            if (balance > MAX) {
                balance = MAX;
            }
        }
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
        );
    }
);

test_error!(
    constructor_duplicate_parameters,
    r#"
    contract Test[Active] {
        state var balance: int = 0;

        constructor(initial: int, initial: int) {
            balance = initial;
        }
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    constructor_local_variables,
    r#"
    contract Test[Active] {
        state var balance: int = 0;

        constructor(initial: int) {
            var adjusted: int = initial + 10;
            balance = adjusted;
        }
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
        );
        assert_symbol_with_kind(
            table,
            "adjusted",
            SymbolKind::LocalVar,
            "Test::constructor::adjusted",
        );
    }
);

// ============================================================================
// FUNCTION CALL TESTS
// ============================================================================

test_success!(
    function_call_resolves_to_function,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function helper() -> int {
            return 42;
        }

        entrypoint test() {
            var temp: int = helper();
            x = temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_function(table, "helper", "Test::Active::helper", "Active");
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::Active::test::temp",
        );
    }
);

test_error!(
    function_call_to_undefined_function,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            x = undefinedFunction();
        }
    }
"#
);

test_success!(
    function_call_with_arguments,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function add(a: int, b: int) -> int {
            return a + b;
        }

        entrypoint test() {
            var result: int = add(5, 10);
            x = result;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_function(table, "add", "Test::Active::add", "Active");
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "a", SymbolKind::Parameter, "Test::Active::add::a");
        assert_symbol_with_kind(table, "b", SymbolKind::Parameter, "Test::Active::add::b");
        assert_symbol_with_kind(
            table,
            "result",
            SymbolKind::LocalVar,
            "Test::Active::test::result",
        );
    }
);

// ============================================================================
// COMPLEX SCOPE TESTS
// ============================================================================

test_success!(
    complex_nested_scopes,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test(param: int) {
            var outer: int = param;
            if (outer > 0) {
                var inner: int = outer * 2;
                result = inner;
                if (inner > 10) {
                    var deepest: int = inner + 5;
                    result = deepest;
                }
            }
            result = outer;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "param",
            SymbolKind::Parameter,
            "Test::Active::test::param",
        );
        assert_symbol_with_kind(
            table,
            "outer",
            SymbolKind::LocalVar,
            "Test::Active::test::outer",
        );
        assert_symbol_with_kind(
            table,
            "inner",
            SymbolKind::LocalVar,
            "Test::Active::test::inner",
        );
        assert_symbol_with_kind(
            table,
            "deepest",
            SymbolKind::LocalVar,
            "Test::Active::test::deepest",
        );
    }
);

test_success!(
    multiple_functions_same_local_var_names,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint funcA() {
            var temp: int = 5;
            x = temp;
        }

        entrypoint funcB() {
            var temp: int = 10;
            x = temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "funcA", "Test::Active::funcA", "Active");
        assert_entrypoint(table, "funcB", "Test::Active::funcB", "Active");
        // Both functions should have their own 'temp' variable
        let temp_symbols = table.find_symbols_by_name("temp");
        assert_eq!(
            temp_symbols.len(),
            2,
            "Should have two 'temp' symbols in different function scopes"
        );
    }
);

test_success!(
    while_loop_block_scope,
    r#"
    contract Test[Active] {
        state var counter: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var i: int = 0;
            while (i < 10) with @invariant(i >= 0) @variant(10 - i) {
                var temp: int = i * 2;
                counter = counter + temp;
                i = i + 1;
            }
        }
    }
"#,
    |table| {
        assert_state_var(table, "counter", "Test::counter", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "i", SymbolKind::LocalVar, "Test::Active::test::i");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::Active::test::temp",
        );
    }
);

test_success!(
    if_else_separate_scopes,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test(condition: int) {
            if (condition > 0) {
                var x: int = 10;
                result = x;
            } else {
                var y: int = 20;
                result = y;
            }
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(
            table,
            "condition",
            SymbolKind::Parameter,
            "Test::Active::test::condition",
        );
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::Active::test::x");
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::Active::test::y");
    }
);

// ============================================================================
// EXPRESSION TESTS
// ============================================================================

test_success!(
    binary_operations_resolve_operands,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            result = a + b;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "a", SymbolKind::LocalVar, "Test::Active::test::a");
        assert_symbol_with_kind(table, "b", SymbolKind::LocalVar, "Test::Active::test::b");
    }
);

test_success!(
    unary_operations_resolve_operand,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var x: int = 5;
            result = -x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::Active::test::x");
    }
);

test_success!(
    complex_expression_resolution,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var c: int = 15;
            result = (a + b) * c - a;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "a", SymbolKind::LocalVar, "Test::Active::test::a");
        assert_symbol_with_kind(table, "b", SymbolKind::LocalVar, "Test::Active::test::b");
        assert_symbol_with_kind(table, "c", SymbolKind::LocalVar, "Test::Active::test::c");
    }
);

// ============================================================================
// RETURN STATEMENT TESTS
// ============================================================================

test_success!(
    return_with_expression,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint getValue() -> int {
            return x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "getValue", "Test::Active::getValue", "Active");
    }
);

test_success!(
    return_with_local_variable,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint compute() -> int {
            var result: int = x * 2;
            return result;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "compute", "Test::Active::compute", "Active");
        assert_symbol_with_kind(
            table,
            "result",
            SymbolKind::LocalVar,
            "Test::Active::compute::result",
        );
    }
);

test_success!(
    return_without_expression,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint doSomething() {
            x = 10;
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "doSomething", "Test::Active::doSomething", "Active");
    }
);

// ============================================================================
// MULTIPLE STATES TESTS
// ============================================================================

test_success!(
    multiple_states_with_different_functions,
    r#"
    contract Test[Open, Closed] {
        state var balance: int = 0;
    }

    Test@Open(any) {
        entrypoint deposit(amount: int) {
            balance = balance + amount;
        }
    }

    Test@Closed(any) {
        entrypoint status() -> int {
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "deposit", "Test::Open::deposit", "Open");
        assert_entrypoint(table, "status", "Test::Closed::status", "Closed");
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::Open::deposit::amount",
        );
    }
);

test_success!(
    state_transitions_between_valid_states,
    r#"
    contract Test[Open, Closed] {
        state var balance: int = 0;
        state const MAX: int = 100;
    }

    Test@Open(any) {
        entrypoint deposit(amount: int) {
            balance = balance + amount;
            if (balance >= MAX) {
                become Closed;
            }
        }
    }

    Test@Closed(any) {
        entrypoint reopen() {
            become Open;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_entrypoint(table, "deposit", "Test::Open::deposit", "Open");
        assert_entrypoint(table, "reopen", "Test::Closed::reopen", "Closed");
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::Open::deposit::amount",
        );
    }
);

// ============================================================================
// EDGE CASES
// ============================================================================

test_success!(
    empty_function_body,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint doNothing() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "doNothing", "Test::Active::doNothing", "Active");
    }
);

test_success!(
    multiple_state_variables,
    r#"
    contract Test[Active] {
        state var a: int = 0;
        state var b: int = 1;
        state var c: int = 2;
    }

    Test@Active(any) {
        entrypoint sum() -> int {
            return a + b + c;
        }
    }
"#,
    |table| {
        assert_state_var(table, "a", "Test::a", BaseType::Int);
        assert_state_var(table, "b", "Test::b", BaseType::Int);
        assert_state_var(table, "c", "Test::c", BaseType::Int);
        assert_entrypoint(table, "sum", "Test::Active::sum", "Active");
    }
);

test_success!(
    multiple_state_constants,
    r#"
    contract Test[Active] {
        state const A: int = 10;
        state const B: int = 20;
        state const C: int = 30;
    }

    Test@Active(any) {
        entrypoint sum() -> int {
            return A + B + C;
        }
    }
"#,
    |table| {
        assert_state_const(table, "A", "Test::A", BaseType::Int);
        assert_state_const(table, "B", "Test::B", BaseType::Int);
        assert_state_const(table, "C", "Test::C", BaseType::Int);
        assert_entrypoint(table, "sum", "Test::Active::sum", "Active");
    }
);

test_success!(
    mixed_state_vars_and_consts,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
        state const MAX_BALANCE: int = 1000;
        state const MIN_BALANCE: int = 0;
    }

    Test@Active(any) {
        entrypoint check() -> int {
            if (balance > MAX_BALANCE) {
                balance = MAX_BALANCE;
            }
            if (balance < MIN_BALANCE) {
                balance = MIN_BALANCE;
            }
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_state_const(table, "MAX_BALANCE", "Test::MAX_BALANCE", BaseType::Int);
        assert_state_const(table, "MIN_BALANCE", "Test::MIN_BALANCE", BaseType::Int);
        assert_entrypoint(table, "check", "Test::Active::check", "Active");
    }
);

test_success!(
    function_with_multiple_parameters,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint calculate(a: int, b: int, c: int) -> int {
            return a + b + c;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "calculate", "Test::Active::calculate", "Active");
        assert_symbol_with_kind(
            table,
            "a",
            SymbolKind::Parameter,
            "Test::Active::calculate::a",
        );
        assert_symbol_with_kind(
            table,
            "b",
            SymbolKind::Parameter,
            "Test::Active::calculate::b",
        );
        assert_symbol_with_kind(
            table,
            "c",
            SymbolKind::Parameter,
            "Test::Active::calculate::c",
        );
    }
);

test_success!(
    nested_if_statements,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) {
            if (x > 0) {
                if (x > 10) {
                    if (x > 100) {
                        result = 3;
                    } else {
                        result = 2;
                    }
                } else {
                    result = 1;
                }
            } else {
                result = 0;
            }
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::Active::test", "Active");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::Active::test::x");
    }
);
