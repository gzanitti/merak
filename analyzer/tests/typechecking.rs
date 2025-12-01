use indexmap::IndexMap;
use merak_analyzer::analyze;
use merak_ast::contract::Program;
use merak_parser::parse_program;

// ============================================================================
// HELPER MACROS
// ============================================================================

macro_rules! test_success {
    ($name:ident, $input:expr) => {
        #[test]
        fn $name() {
            let contract = parse_program($input).expect("Failed to parse");
            let mut contracts = IndexMap::new();
            contracts.insert(contract.data.name.clone(), contract);
            let program = Program { contracts };
            let result = analyze(&program);
            assert!(
                result.is_ok(),
                "Expected type checking to succeed but got error: {:?}",
                result.err()
            );
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
            assert!(
                result.is_err(),
                "Expected type checking to fail but it succeeded"
            );
        }
    };
}

// ============================================================================
// A. LITERAL TYPE ASSIGNMENT TESTS
// ============================================================================

test_success!(
    literal_integer_has_int_type,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 42;
        }
    }
"#
);

test_success!(
    literal_boolean_has_bool_type,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var flag: bool = true;
            var other: bool = false;
        }
    }
"#
);

test_success!(
    literal_string_has_string_type,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var message: string = "hello";
        }
    }
"#
);

test_success!(
    literal_address_has_address_type,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var addr: address = 0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef;
        }
    }
"#
);

// ============================================================================
// B. UNARY OPERATOR TESTS
// ============================================================================

test_success!(
    unary_negation_on_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 5;
            var result: int = -num;
        }
    }
"#
);

test_error!(
    unary_negation_on_bool_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var flag: bool = true;
            var result: int = -flag;
        }
    }
"#
);

test_success!(
    unary_not_on_bool_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var flag: bool = true;
            var result: bool = !flag;
        }
    }
"#
);

test_error!(
    unary_not_on_int_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 5;
            var result: bool = !num;
        }
    }
"#
);

// ============================================================================
// C. BINARY ARITHMETIC OPERATOR TESTS
// ============================================================================

test_success!(
    arithmetic_add_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: int = a + b;
        }
    }
"#
);

test_success!(
    arithmetic_subtract_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 10;
            var b: int = 5;
            var result: int = a - b;
        }
    }
"#
);

test_success!(
    arithmetic_multiply_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 3;
            var result: int = a * b;
        }
    }
"#
);

test_success!(
    arithmetic_divide_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 10;
            var b: int = 2;
            var result: int = a / b;
        }
    }
"#
);

test_success!(
    arithmetic_modulo_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 10;
            var b: int = 3;
            var result: int = a % b;
        }
    }
"#
);

test_error!(
    arithmetic_add_int_bool_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: bool = true;
            var result: int = a + b;
        }
    }
"#
);

test_error!(
    arithmetic_add_bool_bool_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: bool = false;
            var result: bool = a + b;
        }
    }
"#
);

test_error!(
    arithmetic_multiply_string_int_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: string = "hello";
            var b: int = 5;
            var result: int = a * b;
        }
    }
"#
);

// ============================================================================
// D. BINARY COMPARISON OPERATOR TESTS
// ============================================================================

test_success!(
    comparison_less_than_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: bool = a < b;
        }
    }
"#
);

test_success!(
    comparison_less_equal_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: bool = a <= b;
        }
    }
"#
);

test_success!(
    comparison_greater_than_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 10;
            var b: int = 5;
            var result: bool = a > b;
        }
    }
"#
);

test_success!(
    comparison_greater_equal_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 10;
            var b: int = 5;
            var result: bool = a >= b;
        }
    }
"#
);

test_error!(
    comparison_less_than_bool_bool_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: bool = false;
            var result: bool = a < b;
        }
    }
"#
);

test_success!(
    equality_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 5;
            var result: bool = a == b;
        }
    }
"#
);

test_success!(
    equality_bool_bool_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: bool = false;
            var result: bool = a == b;
        }
    }
"#
);

test_success!(
    equality_string_string_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: string = "hello";
            var b: string = "world";
            var result: bool = a == b;
        }
    }
"#
);

test_success!(
    equality_address_address_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: address = 0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef;
            var b: address = 0xfedcba0987654321fedcba0987654321fedcba0987654321fedcba0987654321;
            var result: bool = a == b;
        }
    }
"#
);

test_success!(
    not_equal_int_int_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: bool = a != b;
        }
    }
"#
);

test_error!(
    equality_int_bool_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: bool = true;
            var result: bool = a == b;
        }
    }
"#
);

test_error!(
    equality_string_int_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: string = "hello";
            var b: int = 5;
            var result: bool = a == b;
        }
    }
"#
);

// ============================================================================
// E. BINARY LOGICAL OPERATOR TESTS
// ============================================================================

test_success!(
    logical_and_bool_bool_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: bool = false;
            var result: bool = a && b;
        }
    }
"#
);

test_success!(
    logical_or_bool_bool_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: bool = false;
            var result: bool = a || b;
        }
    }
"#
);

test_error!(
    logical_and_int_int_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: bool = a && b;
        }
    }
"#
);

test_error!(
    logical_or_bool_int_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: bool = true;
            var b: int = 5;
            var result: bool = a || b;
        }
    }
"#
);

test_error!(
    logical_and_string_string_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: string = "hello";
            var b: string = "world";
            var result: bool = a && b;
        }
    }
"#
);

// ============================================================================
// F. VARIABLE DECLARATION TESTS
// ============================================================================

test_success!(
    var_declaration_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 42;
        }
    }
"#
);

test_error!(
    var_declaration_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = true;
        }
    }
"#
);

test_success!(
    const_declaration_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            const MAX: int = 100;
        }
    }
"#
);

test_error!(
    const_declaration_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            const FLAG: bool = 42;
        }
    }
"#
);

test_success!(
    state_var_declaration_matching_type_succeeds,
    r#"
    contract Test[Active] {
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
    state_var_declaration_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state var balance: int = true;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    state_const_declaration_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state const MAX: int = 1000;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_error!(
    state_const_declaration_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state const MAX: int = "hello";
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    refinement_type_base_extraction_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: {int | num > 0} = 5;
        }
    }
"#
);

test_error!(
    refinement_type_base_mismatch_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: {int | num > 0} = true;
        }
    }
"#
);

// ============================================================================
// G. ASSIGNMENT TESTS
// ============================================================================

test_success!(
    assignment_matching_types_succeeds,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            balance = 100;
        }
    }
"#
);

test_error!(
    assignment_mismatched_types_fails,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            balance = true;
        }
    }
"#
);

test_success!(
    assignment_to_local_var_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 5;
            num = 10;
        }
    }
"#
);

test_success!(
    assignment_to_parameter_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(value: int) {
            value = 100;
        }
    }
"#
);

test_error!(
    assignment_expression_type_mismatch_fails,
    r#"
    contract Test[Active] {
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 5;
            num = balance + true;
        }
    }
"#
);

// ============================================================================
// H. FUNCTION CALL TESTS
// ============================================================================

test_success!(
    function_call_correct_arity_and_types_succeeds,
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
        }
    }
"#
);

test_error!(
    function_call_too_few_arguments_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function add(a: int, b: int) -> int {
            return a + b;
        }

        entrypoint test() {
            var result: int = add(5);
        }
    }
"#
);

test_error!(
    function_call_too_many_arguments_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function add(a: int, b: int) -> int {
            return a + b;
        }

        entrypoint test() {
            var result: int = add(5, 10, 15);
        }
    }
"#
);

test_error!(
    function_call_first_arg_type_mismatch_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function add(a: int, b: int) -> int {
            return a + b;
        }

        entrypoint test() {
            var result: int = add(true, 10);
        }
    }
"#
);

test_error!(
    function_call_second_arg_type_mismatch_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function add(a: int, b: int) -> int {
            return a + b;
        }

        entrypoint test() {
            var result: int = add(5, true);
        }
    }
"#
);

test_success!(
    function_call_return_type_correctly_inferred,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function getValue() -> int {
            return 42;
        }

        entrypoint test() {
            var num: int = getValue();
        }
    }
"#
);

test_success!(
    function_call_no_return_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function doSomething() {
            x = 10;
        }

        entrypoint test() {
            doSomething();
        }
    }
"#
);

test_success!(
    function_call_with_refinement_types_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function positive(n: {int | n > 0}) -> {int | positive >= 0} {
            return n;
        }

        entrypoint test() {
            var result: int = positive(5);
        }
    }
"#
);

// ============================================================================
// I. CONTROL FLOW TESTS
// ============================================================================

test_success!(
    if_statement_bool_condition_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            if (true) {
                x = 10;
            }
        }
    }
"#
);

test_error!(
    if_statement_int_condition_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            if (5) {
                x = 10;
            }
        }
    }
"#
);

test_success!(
    if_else_statement_bool_condition_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var flag: bool = true;
            if (flag) {
                x = 10;
            } else {
                x = 20;
            }
        }
    }
"#
);

test_success!(
    while_statement_bool_condition_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var i: int = 0;
            while (i < 10) with @invariant(i >= 0) @variant(10 - i) {
                i = i + 1;
            }
        }
    }
"#
);

test_error!(
    while_statement_int_condition_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var i: int = 0;
            while (i) with @invariant(i >= 0) @variant(10 - i) {
                i = i + 1;
            }
        }
    }
"#
);

test_error!(
    if_statement_string_condition_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var msg: string = "hello";
            if (msg) {
                x = 10;
            }
        }
    }
"#
);

// ============================================================================
// J. RETURN STATEMENT TESTS
// ============================================================================

test_success!(
    return_with_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint getValue() -> int {
            return 42;
        }
    }
"#
);

test_error!(
    return_with_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint getValue() -> int {
            return true;
        }
    }
"#
);

test_success!(
    return_without_value_in_void_function_succeeds,
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
"#
);

test_error!(
    return_with_value_in_void_function_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint doSomething() {
            return 42;
        }
    }
"#
);

test_error!(
    return_without_value_in_non_void_function_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint getValue() -> int {
            return;
        }
    }
"#
);

test_success!(
    return_type_from_refinement_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint getPositive() -> {int | getPositive > 0} {
            return 42;
        }
    }
"#
);

test_success!(
    constructor_void_return_succeeds,
    r#"
    contract Test[Active] {
        state var balance: int = 0;

        constructor(initial: int) {
            balance = initial;
            return;
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
    return_state_variable_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state var balance: int = 100;
    }

    Test@Active(any) {
        entrypoint getBalance() -> int {
            return balance;
        }
    }
"#
);

test_success!(
    return_expression_result_matching_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint compute() -> int {
            var a: int = 5;
            var b: int = 10;
            return a + b;
        }
    }
"#
);

test_error!(
    return_expression_result_mismatched_type_fails,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint compute() -> int {
            var a: int = 5;
            var b: int = 10;
            return a > b;
        }
    }
"#
);

// ============================================================================
// K. CONSTRAINT EXPRESSION TESTS
// ============================================================================

test_success!(
    constraint_bool_expression_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: {int | num > 0} = 5;
        }
    }
"#
);

test_success!(
    constraint_with_binder_variable_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var value: {n: int | n >= 0 && n <= 100} = 50;
        }
    }
"#
);

test_success!(
    constraint_with_implicit_self_binder_succeeds,
    r#"
    contract Test[Active] {
        state var balance: {int | balance >= 0} = 100;
    }

    Test@Active(any) {
        entrypoint test() {
            return;
        }
    }
"#
);

test_success!(
    parameter_with_constraint_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint deposit(amount: {int | amount > 0}) {
            x = x + amount;
        }
    }
"#
);

// ============================================================================
// L. COMPLEX EXPRESSION TESTS
// ============================================================================

test_success!(
    nested_arithmetic_expression_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var c: int = 15;
            var d: int = 20;
            var result: int = (a + b) * (c - d);
        }
    }
"#
);

test_error!(
    nested_expression_type_error_propagates,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: bool = true;
            var c: int = 15;
            var result: int = (a + b) * c;
        }
    }
"#
);

test_success!(
    function_call_in_expression_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        internal function getValue() -> int {
            return 42;
        }

        entrypoint test() {
            var result: int = getValue() + 10;
        }
    }
"#
);

test_success!(
    chained_comparisons_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var c: int = 15;
            var result: bool = (a < b) && (b < c);
        }
    }
"#
);

test_success!(
    mixed_operators_with_correct_precedence_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var c: int = 15;
            var result: bool = a + b < c * 2;
        }
    }
"#
);

test_success!(
    grouped_expression_preserves_type,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var a: int = 5;
            var b: int = 10;
            var result: int = (a + b);
        }
    }
"#
);

// ============================================================================
// M. EDGE CASE TESTS
// ============================================================================

test_success!(
    empty_function_body_with_return_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint doNothing() {
            return;
        }
    }
"#
);

test_success!(
    multiple_state_variables_same_type_succeeds,
    r#"
    contract Test[Active] {
        state var a: int = 1;
        state var b: int = 2;
        state var c: int = 3;
    }

    Test@Active(any) {
        entrypoint sum() -> int {
            return a + b + c;
        }
    }
"#
);

test_success!(
    shadowing_with_same_type_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) {
            var y: int = x;
            if (x > 0) {
                var x: int = 100;
                y = x;
            }
        }
    }
"#
);

test_success!(
    deeply_nested_blocks_maintain_types,
    r#"
    contract Test[Active] {
        state var result: int = 0;
    }

    Test@Active(any) {
        entrypoint test(x: int) {
            if (x > 0) {
                if (x > 10) {
                    if (x > 100) {
                        var temp: int = x * 2;
                        result = temp;
                    }
                }
            }
        }
    }
"#
);

test_success!(
    state_const_in_expression_succeeds,
    r#"
    contract Test[Active] {
        state const MAX: int = 1000;
        state var balance: int = 0;
    }

    Test@Active(any) {
        entrypoint deposit(amount: int) {
            if (balance + amount <= MAX) {
                balance = balance + amount;
            }
        }
    }
"#
);

test_success!(
    all_base_types_in_single_function_succeeds,
    r#"
    contract Test[Active] {
        state var x: int = 0;
    }

    Test@Active(any) {
        entrypoint test() {
            var num: int = 42;
            var flag: bool = true;
            var msg: string = "hello";
            var addr: address = 0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef;
        }
    }
"#
);
