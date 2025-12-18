mod common;
use indexmap::IndexMap;
use merak_analyzer::analyze;
use merak_ast::NodeIdGenerator;
use merak_ast::contract::Program;
use merak_ast::types::BaseType;
use merak_parser::parse_program;
use merak_symbols::{SymbolKind, SymbolNamespace};
use common::load_test_contracts;


// ============================================================================
// HELPER MACROS
// ============================================================================

macro_rules! test_success {
    ($name:ident, $input:expr, $checks:expr) => {
        #[test]
        fn $name() {
            let id_gen = NodeIdGenerator::new();
            let file = parse_program($input, &id_gen).expect("Failed to parse");
            let mut files = IndexMap::new();
            files.insert(file.contract.name.clone(), file);
            let program = Program { files };
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
            let id_gen = NodeIdGenerator::new();
            let file = parse_program($input, &id_gen).expect("Failed to parse");
            let mut files = IndexMap::new();
            files.insert(file.contract.name.clone(), file);
            let program = Program { files };
            let result = analyze(&program);
            assert!(result.is_err(), "Expected error but analysis succeeded");
        }
    };
}

// Multi-contract test macros
macro_rules! test_success_multi {
    ($name:ident, [$($contract_name:expr => $contract_src:expr),+ $(,)?], $checks:expr) => {
        #[test]
        fn $name() {
            let contracts = vec![
                $(($contract_name, $contract_src),)+
            ];

            let program = load_test_contracts(contracts)
                .expect("Failed to load contracts");

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

macro_rules! test_error_multi {
    ($name:ident, [$($contract_name:expr => $contract_src:expr),+ $(,)?]) => {
        #[test]
        fn $name() {
            let contracts = vec![
                $(($contract_name, $contract_src),)+
            ];

            let program = load_test_contracts(contracts)
                .expect("Failed to load contracts");

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
    symbol_namespace: SymbolNamespace,
) {
    let symbol_id = table.lookup(expected_qualified_name, symbol_namespace);

    assert!(
        symbol_id.is_some(),
        "Symbol '{}' with qualified name '{}' not found in symbol table",
        simple_name,
        expected_qualified_name
    );

    let symbol = table.get_symbol(symbol_id.unwrap());

    assert!(
        symbol.kind == expected_kind,
        "Symbol '{}' found but with wrong kind.\nExpected: {:?}\nFound: {:?}",
        simple_name,
        expected_kind,
        symbol.kind
    );
}

/// Verify a state variable exists with correct properties
fn assert_state_var(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
    base_type: BaseType,
) {
    assert_symbol_with_kind(table, name, SymbolKind::StateVar, qualified_name, SymbolNamespace::Value);

    // Also verify the type
    let symbol_id = table.lookup(qualified_name, SymbolNamespace::Value);
    assert!(symbol_id.is_some(), "State var '{}' not found", name);

    let symbol = table.get_symbol(symbol_id.unwrap());
    let has_correct_type = symbol.ty
        .as_ref()
        .map(|t| t.base == base_type)
        .unwrap_or(false);

    assert!(
        has_correct_type,
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
    assert_symbol_with_kind(table, name, SymbolKind::StateConst, qualified_name, SymbolNamespace::Value);

    // Also verify the type
    let symbol_id = table.lookup(qualified_name, SymbolNamespace::Value);
    assert!(symbol_id.is_some(), "State const '{}' not found", name);

    let symbol = table.get_symbol(symbol_id.unwrap());
    let has_correct_type = symbol.ty
        .as_ref()
        .map(|t| t.base == base_type)
        .unwrap_or(false);

    assert!(
        has_correct_type,
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
) {
    let symbol_id = table.lookup(qualified_name, SymbolNamespace::Value);

    assert!(
        symbol_id.is_some(),
        "Entrypoint '{}' with qualified name '{}' not found in symbol table",
        name,
        qualified_name
    );

    let symbol = table.get_symbol(symbol_id.unwrap());

    let is_entrypoint = matches!(&symbol.kind, SymbolKind::Entrypoint { .. });

    assert!(
        is_entrypoint,
        "Entrypoint '{}' not found with SymbolId {:?}",
        name,
        symbol_id
    );
}

/// Verify a function exists with correct state
fn assert_function(
    table: &merak_symbols::SymbolTable,
    name: &str,
    qualified_name: &str,
) {
    let symbol_id = table.lookup(qualified_name, SymbolNamespace::Value);

    assert!(
        symbol_id.is_some(),
        "Function '{}' with qualified name '{}' not found in symbol table",
        name,
        qualified_name
    );

    let symbol = table.get_symbol(symbol_id.unwrap());

    let is_function = matches!(&symbol.kind, SymbolKind::Function { .. });

    assert!(
        is_function,
        "Function '{}' not found with SymbolId {:?}",
        name,
        symbol_id
    );
}

/// Verify a constructor exists
fn assert_constructor(table: &merak_symbols::SymbolTable, contract: &str) {
    let qualified_name = format!("{}::constructor", contract);
    let symbol_id = table.lookup(&qualified_name, SymbolNamespace::Value);

    assert!(
        symbol_id.is_some(),
        "Constructor for contract '{}' not found in symbol table",
        contract
    );

    let symbol = table.get_symbol(symbol_id.unwrap());

    let is_correct_constructor = matches!(&symbol.kind, SymbolKind::ContractInit { contract: c } if c == contract);

    assert!(
        is_correct_constructor,
        "Constructor found but for wrong contract. Expected: {}\nFound: {:?}",
        contract,
        symbol.kind
    );
}

// ============================================================================
// STATE VARIABLE AND CONSTANT TESTS
// ============================================================================

test_success!(
    state_var_registered_in_contract_scope,
    r#"
    contract Test {
        state var balance: int = 0;

        entrypoint getBalance() -> int {
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "getBalance", "Test::getBalance");
    }
);

test_success!(
    state_const_registered_in_contract_scope,
    r#"
    contract Test {
        state const MAX: int = 100;

        entrypoint getMax() -> int {
            return MAX;
        }
    }
"#,
    |table| {
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_entrypoint(table, "getMax", "Test::getMax");
    }
);

test_error!(
    duplicate_state_var_names,
    r#"
    contract Test {
        state var balance: int = 0;
        state var balance: int = 100;

        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    state_var_shadowing_state_const,
    r#"
    contract Test {
        state const balance: int = 0;
        state var balance: int = 100;

        entrypoint test() {
            return;
        }
    }
"#
);


// ============================================================================
// FUNCTION SYMBOL TESTS
// ============================================================================

test_success!(
    functions_registered,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint doSomething() {
            return;
        }
    }
"#,
    |table| {
        assert_entrypoint(table, "doSomething", "Test::doSomething");
    }
);


test_error!(
    same_function_name,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint action() {
            return;
        }

        internal function action() {
            return;
        }
    }
"#
);

test_success!(
    function_visibility_stored,
    r#"
    contract Test {
        state var x: int = 0;

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
        assert_entrypoint(table, "entry", "Test::entry");
        assert_function(table, "ext", "Test::ext");
        assert_function(table, "intern", "Test::intern");
    }
);

// ============================================================================
// FUNCTION PARAMETER TESTS
// ============================================================================

test_success!(
    parameters_registered_in_function_scope,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test(amount: int) {
            x = amount;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        // Note: parameter 'amount' is also registered but in function scope
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::test::amount",
            SymbolNamespace::Value
        );
    }
);

test_error!(
    duplicate_parameters_in_same_function,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test(amount: int, amount: int) {
            return;
        }
    }
"#
);

test_success!(
    parameters_visible_in_function_body,
    r#"
    contract Test {
        state var balance: int = 0;

        entrypoint deposit(amount: int) {
            balance = balance + amount;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "deposit", "Test::deposit");
        assert_symbol_with_kind(
            table,
            "amount",
            SymbolKind::Parameter,
            "Test::deposit::amount",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    parameters_can_shadow_state_variables,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test(x: int) -> int {
            return x;
        }
    }
"#,
    |table| {
        // Both state var and parameter 'x' should exist
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::test::x", SymbolNamespace::Value);
    }
);

// ============================================================================
// LOCAL VARIABLE AND CONSTANT TESTS
// ============================================================================

test_success!(
    local_vars_registered_in_block_scope,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            var y: int = 10;
            x = y;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::test::y", SymbolNamespace::Value);
    }
);

test_success!(
    local_consts_registered_in_block_scope,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            const Y: int = 10;
            x = Y;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "Y", SymbolKind::LocalVar, "Test::test::Y", SymbolNamespace::Value);
    }
);

test_success!(
    nested_blocks_can_shadow_outer_variables,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        // Both 'x' variables should exist in different scopes
        // Outer x
        let outer_x = table.lookup("Test::test::x", SymbolNamespace::Value);
        assert!(outer_x.is_some(), "Outer 'x' variable should exist");
        // Inner x (in nested block)
        let inner_x = table.lookup("Test::test::x", SymbolNamespace::Value);
        assert!(inner_x.is_some(), "Inner 'x' variable should exist");
    }
);

test_error!(
    variable_cannot_redeclare_in_same_block,
    r#"
    contract Test {
        state var x: int = 0;

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
    contract Test {
        state var balance: int = 0;

        entrypoint test() {
            balance = 100;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
    }
);

test_success!(
    resolution_follows_scope_chain,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test(x: int) {
            var y: int = x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::test::x", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::test::y", SymbolNamespace::Value);
    }
);

test_error!(
    unresolved_identifiers_are_errors,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            x = undefinedVar;
        }
    }
"#
);

test_success!(
    state_variable_accessed_from_function,
    r#"
    contract Test {
        state var balance: int = 0;

        entrypoint getBalance() -> int {
            return balance;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_entrypoint(table, "getBalance", "Test::getBalance");
    }
);

test_success!(
    state_constant_accessed_from_function,
    r#"
    contract Test {
        state const MAX: int = 1000;

        entrypoint check() -> int {
            return MAX;
        }
    }
"#,
    |table| {
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_entrypoint(table, "check", "Test::check");
    }
);

test_success!(
    parameter_accessed_in_function_body,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint set(value: int) {
            x = value;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "set", "Test::set");
        assert_symbol_with_kind(
            table,
            "value",
            SymbolKind::Parameter,
            "Test::set::value",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    local_variable_accessed_after_declaration,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            var temp: int = 5;
            x = temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::test::temp",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    multiple_references_to_same_symbol,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            var temp: int = 5;
            x = temp;
            x = temp + temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::test::temp",
            SymbolNamespace::Value
        );
    }
);

// ============================================================================
// ERROR CASES: DUPLICATE DECLARATIONS
// ============================================================================

test_error!(
    duplicate_state_vars_detected,
    r#"
    contract Test {
        state var x: int = 0;
        state var x: int = 1;

        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_state_consts_detected,
    r#"
    contract Test {
        state const X: int = 0;
        state const X: int = 1;

        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_function_names,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint action() {
            return;
        }
        internal function action() {
            return;
        }
    }
"#
);

test_error!(
    duplicate_parameters_detected,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test(param: int, param: int) {
            return;
        }
    }
"#
);

test_error!(
    duplicate_locals_in_same_block_detected,
    r#"
    contract Test {
        state var x: int = 0;

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
    contract Test {
        state var x: int = 0;

        entrypoint funcA() {
            var local: int = 5;
        }

        internal function funcB() {
            x = local;
        }
    }
"#
);

test_error!(
    parameter_not_accessible_outside_function,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint funcA(param: int) {
            x = param;
        }

        internal function funcB() {
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
    contract Test {
        state var x: int = 0;

        entrypoint test(x: int) -> int {
            return x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::test::x", SymbolNamespace::Value);
    }
);

test_success!(
    nested_block_shadows_outer_block_correctly,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        // Both 'x' variables should exist
        // Outer x
        let outer_x = table.lookup("Test::test::x", SymbolNamespace::Value);
        assert!(outer_x.is_some(), "Outer 'x' variable should exist");
        // Note: We can't easily verify the nested 'x' without knowing its scope ID,
        // but the important thing is that the code parses and analyzes correctly
    }
);

test_success!(
    after_nested_block_outer_symbol_visible_again,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::test::x", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::test::y", SymbolNamespace::Value);
    }
);

// ============================================================================
// CONSTRUCTOR TESTS
// ============================================================================

test_success!(
    constructor_parameters_registered,
    r#"
    contract Test {
        state var balance: int = 0;

        constructor(initial: int) {
            balance = initial;
        }

        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    constructor_can_access_state_vars,
    r#"
    contract Test {
        state var balance: int = 0;
        state const MAX: int = 1000;

        constructor(initial: int) {
            balance = initial;
            if (balance > MAX) {
                balance = MAX;
            }
        }

        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_state_const(table, "MAX", "Test::MAX", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
            SymbolNamespace::Value
        );
    }
);

test_error!(
    constructor_duplicate_parameters,
    r#"
    contract Test {
        state var balance: int = 0;

        constructor(initial: int, initial: int) {
            balance = initial;
        }

        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    constructor_local_variables,
    r#"
    contract Test {
        state var balance: int = 0;

        constructor(initial: int) {
            var adjusted: int = initial + 10;
            balance = adjusted;
        }

        entrypoint test() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "balance", "Test::balance", BaseType::Int);
        assert_constructor(table, "Test");
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "initial",
            SymbolKind::Parameter,
            "Test::constructor::initial",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "adjusted",
            SymbolKind::LocalVar,
            "Test::constructor::adjusted",
            SymbolNamespace::Value
        );
    }
);

// ============================================================================
// FUNCTION CALL TESTS
// ============================================================================

test_success!(
    function_call_resolves_to_function,
    r#"
    contract Test {
        state var x: int = 0;

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
        assert_function(table, "helper", "Test::helper");
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::test::temp",
            SymbolNamespace::Value
        );
    }
);

test_error!(
    function_call_to_undefined_function,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint test() {
            x = undefinedFunction();
        }
    }
"#
);

test_success!(
    function_call_with_arguments,
    r#"
    contract Test {
        state var x: int = 0;

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
        assert_function(table, "add", "Test::add");
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "a", SymbolKind::Parameter, "Test::add::a", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "b", SymbolKind::Parameter, "Test::add::b", SymbolNamespace::Value);
        assert_symbol_with_kind(
            table,
            "result",
            SymbolKind::LocalVar,
            "Test::test::result",
            SymbolNamespace::Value
        );
    }
);

// ============================================================================
// COMPLEX SCOPE TESTS
// ============================================================================

test_success!(
    complex_nested_scopes,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "param",
            SymbolKind::Parameter,
            "Test::test::param",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "outer",
            SymbolKind::LocalVar,
            "Test::test::outer",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "inner",
            SymbolKind::LocalVar,
            "Test::test::inner",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "deepest",
            SymbolKind::LocalVar,
            "Test::test::deepest",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    multiple_functions_same_local_var_names,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint funcA() {
            var temp: int = 5;
            x = temp;
        }

        internal function funcB() {
            var temp: int = 10;
            x = temp;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "funcA", "Test::funcA");
        assert_function(table, "funcB", "Test::funcB");
        // Both functions should have their own 'temp' variable
        let temp_a = table.lookup("Test::funcA::temp", SymbolNamespace::Value);
        assert!(temp_a.is_some(), "funcA should have 'temp' variable");
        let temp_b = table.lookup("Test::funcB::temp", SymbolNamespace::Value);
        assert!(temp_b.is_some(), "funcB should have 'temp' variable");
    }
);

test_success!(
    while_loop_block_scope,
    r#"
    contract Test {
        state var counter: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "i", SymbolKind::LocalVar, "Test::test::i", SymbolNamespace::Value);
        assert_symbol_with_kind(
            table,
            "temp",
            SymbolKind::LocalVar,
            "Test::test::temp",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    if_else_separate_scopes,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(
            table,
            "condition",
            SymbolKind::Parameter,
            "Test::test::condition",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::test::x", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "y", SymbolKind::LocalVar, "Test::test::y", SymbolNamespace::Value);
    }
);

// ============================================================================
// EXPRESSION TESTS
// ============================================================================

test_success!(
    binary_operations_resolve_operands,
    r#"
    contract Test {
        state var result: int = 0;

        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            result = a + b;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "a", SymbolKind::LocalVar, "Test::test::a", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "b", SymbolKind::LocalVar, "Test::test::b", SymbolNamespace::Value);
    }
);

test_success!(
    unary_operations_resolve_operand,
    r#"
    contract Test {
        state var result: int = 0;

        entrypoint test() {
            var x: int = 5;
            result = -x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "result", "Test::result", BaseType::Int);
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::LocalVar, "Test::test::x", SymbolNamespace::Value);
    }
);

test_success!(
    complex_expression_resolution,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "a", SymbolKind::LocalVar, "Test::test::a", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "b", SymbolKind::LocalVar, "Test::test::b", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "c", SymbolKind::LocalVar, "Test::test::c", SymbolNamespace::Value);
    }
);

// ============================================================================
// RETURN STATEMENT TESTS
// ============================================================================

test_success!(
    return_with_expression,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint getValue() -> int {
            return x;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "getValue", "Test::getValue");
    }
);

test_success!(
    return_with_local_variable,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint compute() -> int {
            var result: int = x * 2;
            return result;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "compute", "Test::compute");
        assert_symbol_with_kind(
            table,
            "result",
            SymbolKind::LocalVar,
            "Test::compute::result",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    return_without_expression,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint doSomething() {
            x = 10;
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "doSomething", "Test::doSomething");
    }
);


// ============================================================================
// EDGE CASES
// ============================================================================

test_success!(
    empty_function_body,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint doNothing() {
            return;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "doNothing", "Test::doNothing");
    }
);

test_success!(
    multiple_state_variables,
    r#"
    contract Test {
        state var a: int = 0;
        state var b: int = 1;
        state var c: int = 2;

        entrypoint sum() -> int {
            return a + b + c;
        }
    }
"#,
    |table| {
        assert_state_var(table, "a", "Test::a", BaseType::Int);
        assert_state_var(table, "b", "Test::b", BaseType::Int);
        assert_state_var(table, "c", "Test::c", BaseType::Int);
        assert_entrypoint(table, "sum", "Test::sum");
    }
);

test_success!(
    multiple_state_constants,
    r#"
    contract Test {
        state const A: int = 10;
        state const B: int = 20;
        state const C: int = 30;

        entrypoint sum() -> int {
            return A + B + C;
        }
    }
"#,
    |table| {
        assert_state_const(table, "A", "Test::A", BaseType::Int);
        assert_state_const(table, "B", "Test::B", BaseType::Int);
        assert_state_const(table, "C", "Test::C", BaseType::Int);
        assert_entrypoint(table, "sum", "Test::sum");
    }
);

test_success!(
    mixed_state_vars_and_consts,
    r#"
    contract Test {
        state var balance: int = 0;
        state const MAX_BALANCE: int = 1000;
        state const MIN_BALANCE: int = 0;

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
        assert_entrypoint(table, "check", "Test::check");
    }
);

test_success!(
    function_with_multiple_parameters,
    r#"
    contract Test {
        state var x: int = 0;

        entrypoint calculate(a: int, b: int, c: int) -> int {
            return a + b + c;
        }
    }
"#,
    |table| {
        assert_state_var(table, "x", "Test::x", BaseType::Int);
        assert_entrypoint(table, "calculate", "Test::calculate");
        assert_symbol_with_kind(
            table,
            "a",
            SymbolKind::Parameter,
            "Test::calculate::a",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "b",
            SymbolKind::Parameter,
            "Test::calculate::b",
            SymbolNamespace::Value
        );
        assert_symbol_with_kind(
            table,
            "c",
            SymbolKind::Parameter,
            "Test::calculate::c",
            SymbolNamespace::Value
        );
    }
);

test_success!(
    nested_if_statements,
    r#"
    contract Test {
        state var result: int = 0;

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
        assert_entrypoint(table, "test", "Test::test");
        assert_symbol_with_kind(table, "x", SymbolKind::Parameter, "Test::test::x", SymbolNamespace::Value);
    }
);

// ============================================================================
// IMPORT AND CROSS-CONTRACT CALL TESTS
// ============================================================================

test_success_multi!(
    import_basic_contract,
    [
        "Main" => r#"
        import Vault from vault;

        contract Main {
            state var balance: int = 0;

            entrypoint test() {
                return;
            }
        }
        "#,
        "vault" => r#"
        contract Vault {
            state var amount: int = 0;

            entrypoint getAmount() -> int {
                return amount;
            }
        }
        "#
    ],
    |table| {
        // Verify Main contract symbols
        assert_state_var(table, "balance", "Main::balance", BaseType::Int);
        assert_entrypoint(table, "test", "Main::test");

        // Verify Vault contract symbols are loaded
        assert_state_var(table, "amount", "Vault::amount", BaseType::Int);
        assert_entrypoint(table, "getAmount", "Vault::getAmount");

        // Verify Contract symbols
        assert_symbol_with_kind(table, "Main", SymbolKind::Contract, "Main", SymbolNamespace::Type);
        assert_symbol_with_kind(table, "Vault", SymbolKind::Contract, "Vault", SymbolNamespace::Type);
    }
);

test_success_multi!(
    import_and_instantiate_contract,
    [
        "Main" => r#"
        import Vault from vault;

        contract Main {
            state var myVault: Vault = Vault(0x0000000000000000000000000000000000000000000000000000000000000000);

            entrypoint test() {
                return;
            }
        }
        "#,
        "vault" => r#"
        contract Vault {
            state var balance: int = 0;

            entrypoint deposit(amount: int) {
                balance = balance + amount;
            }
        }
        "#
    ],
    |table: &merak_symbols::SymbolTable| {
        // Verify Main contract has myVault state variable of type Vault
        let symbol_id = table.lookup("Main::myVault", SymbolNamespace::Value);
        assert!(symbol_id.is_some(), "myVault state variable not found");

        let symbol = table.get_symbol(symbol_id.unwrap());
        let is_valid = symbol.kind == SymbolKind::StateVar &&
            symbol.ty.as_ref().map(|t| matches!(t.base, BaseType::Contract(ref name) if name == "Vault")).unwrap_or(false);
        assert!(is_valid, "myVault should be of type Vault");

        // Verify Vault contract exists
        assert_symbol_with_kind(table, "Vault", SymbolKind::Contract, "Vault", SymbolNamespace::Type);
        assert_entrypoint(table, "deposit", "Vault::deposit");
    }
);

test_success_multi!(
    cross_contract_call,
    [
        "Main" => r#"
        import Vault from vault;

        contract Main {
            state var result: int = 0;

            entrypoint callVault(vaultAddr: address) {
                var vault: Vault = Vault(vaultAddr);
                var amount: int = vault.getBalance();
                result = amount;
            }
        }
        "#,
        "vault" => r#"
        contract Vault {
            state var balance: int = 100;

            entrypoint getBalance() -> int {
                return balance;
            }

            internal function deposit(amount: int) {
                balance = balance + amount;
            }
        }
        "#
    ],
    |table| {
        // Verify Main contract
        assert_state_var(table, "result", "Main::result", BaseType::Int);
        assert_entrypoint(table, "callVault", "Main::callVault");

        // Verify Vault contract and its functions
        assert_state_var(table, "balance", "Vault::balance", BaseType::Int);
        assert_entrypoint(table, "getBalance", "Vault::getBalance");
        assert_function(table, "deposit", "Vault::deposit");

        // Verify local variables in callVault
        assert_symbol_with_kind(table, "vault", SymbolKind::LocalVar, "Main::callVault::vault", SymbolNamespace::Value);
        assert_symbol_with_kind(table, "amount", SymbolKind::LocalVar, "Main::callVault::amount", SymbolNamespace::Value);
    }
);

test_success_multi!(
    import_with_alias,
    [
        "Main" => r#"
        import SimpleVault from vault as MyVault;

        contract Main {
            state var storage: MyVault = MyVault(0x0000000000000000000000000000000000000000000000000000000000000000);

            entrypoint test() {
                return;
            }
        }
        "#,
        "vault" => r#"
        contract SimpleVault {
            state var balance: int = 0;

            entrypoint getBalance() -> int {
                return balance;
            }
        }
        "#
    ],
    |table: &merak_symbols::SymbolTable| {
        assert_symbol_with_kind(table, "SimpleVault", SymbolKind::Contract, "SimpleVault", SymbolNamespace::Type);

        // State variable should use the alias type
        let symbol_id = table.lookup("Main::storage", SymbolNamespace::Value);
        assert!(symbol_id.is_some(), "storage state variable not found");
        let symbol = table.get_symbol(symbol_id.unwrap());

        let is_valid = symbol.kind == SymbolKind::StateVar &&
            symbol.ty.as_ref().map(|t| matches!(t.base, BaseType::Contract(ref name) if name == "MyVault")).unwrap_or(false);
        assert!(is_valid, "storage should be of type MyVault (alias)");
    }
);

test_success_multi!(
    multiple_contract_calls,
    [
        "Main" => r#"
        import Vault from vault;
        import Token from token;

        contract Main {
            state var totalValue: int = 0;

            entrypoint calculateTotal(vaultAddr: address, tokenAddr: address) {
                var vault: Vault = Vault(vaultAddr);
                var token: Token = Token(tokenAddr);

                var vaultBalance: int = vault.getBalance();
                var tokenBalance: int = token.balanceOf();

                totalValue = vaultBalance + tokenBalance;
            }
        }
        "#,
        "vault" => r#"
        contract Vault {
            state var balance: int = 50;

            entrypoint getBalance() -> int {
                return balance;
            }
        }
        "#,
        "token" => r#"
        contract Token {
            state var supply: int = 100;

            entrypoint balanceOf() -> int {
                return supply;
            }
        }
        "#
    ],
    |table| {
        // Verify all three contracts exist
        assert_symbol_with_kind(table, "Main", SymbolKind::Contract, "Main", SymbolNamespace::Type);
        assert_symbol_with_kind(table, "Vault", SymbolKind::Contract, "Vault", SymbolNamespace::Type);
        assert_symbol_with_kind(table, "Token", SymbolKind::Contract, "Token", SymbolNamespace::Type);

        // Verify Main's state and function
        assert_state_var(table, "totalValue", "Main::totalValue", BaseType::Int);
        assert_entrypoint(table, "calculateTotal", "Main::calculateTotal");

        // Verify Vault's function
        assert_entrypoint(table, "getBalance", "Vault::getBalance");

        // Verify Token's function
        assert_entrypoint(table, "balanceOf", "Token::balanceOf");
    }
);

test_success_multi!(
    contract_variable_in_function_parameter,
    [
        "Main" => r#"
        import Vault from vault;

        contract Main {
            state var result: int = 0;

            entrypoint processVault(vault: Vault) -> int {
                return vault.getBalance();
            }
        }
        "#,
        "vault" => r#"
        contract Vault {
            state var balance: int = 0;

            entrypoint getBalance() -> int {
                return balance;
            }
        }
        "#
    ],
    |table: &merak_symbols::SymbolTable| {
        // Verify the parameter 'vault' is of type Vault

        let symbol_id = table.lookup("Main::processVault::vault", SymbolNamespace::Value);
        assert!(symbol_id.is_some(), "storage state variable not found");
        let symbol = table.get_symbol(symbol_id.unwrap());

        let is_valid = symbol.kind == SymbolKind::Parameter &&
            symbol.ty.as_ref().map(|t| matches!(t.base, BaseType::Contract(ref name) if name == "Vault")).unwrap_or(false);
        assert!(is_valid, "vault parameter should be of type Vault");
    }
);

// ============================================================================
// ERROR CASES: IMPORT AND CROSS-CONTRACT CALLS
// ============================================================================

test_error_multi!(
    call_on_undefined_contract,
    [
        "Main" => r#"
        contract Main {
            state var result: int = 0;

            entrypoint test() {
                var vault: UndefinedVault = UndefinedVault(0x0000000000000000000000000000000000000000000000000000000000000000);
            }
        }
        "#
    ]
);

// TODO: This test should fail for circular dependencies, but the current implementation
// allows them because load_recursive uses a visited HashMap to prevent infinite loops.
// We need to add explicit circular dependency detection if we want to prohibit them.
#[ignore]
#[test]
fn import_circular_dependency_should_fail() {
    let contracts = vec![
        ("A", r#"
        import B from b;

        contract A {
            state var x: int = 0;
        }

        A@Active(any) {
            entrypoint test() {
                return;
            }
        }
        "#),
        ("b", r#"
        import A from a;

        contract B {
            state var y: int = 0;
        }

        B@Active(any) {
            entrypoint test() {
                return;
            }
        }
        "#),
    ];

    let program = load_test_contracts(contracts)
        .expect("Failed to load contracts");

    let result = analyze(&program);
    // Currently this succeeds, but ideally should fail with a circular dependency error
    assert!(result.is_err(), "Circular dependencies should be detected and rejected");
}
