// Integration tests for codegen - from source code to bytecode execution

mod common;

use common::*;
use primitive_types::U256;

// ── Internal function call tests ───────────────────────────────────────────────

#[test]
fn test_internal_function_call_basic() {
    // internal function double(x) -> int { return x * 2; }
    // external function compute(y) -> int { return double(y); }
    // compute(21) should return 42
    let source = r#"
        contract Math {
            internal function double(x: int) -> int {
                return x * 2;
            }

            external function compute(y: int) -> int {
                return double(y);
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    let calldata = encode_call_uint("compute(uint256)", &[U256::from(21)]);
    let result = runtime.call(calldata);

    assert!(result.success, "compute(21) should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(42),
        "compute(21) should return double(21) = 42"
    );
}

#[test]
fn test_internal_function_call_with_arithmetic() {
    // internal function add(a, b) -> int { return a + b; }
    // external function sum_and_double(x, y) -> int { var s = add(x,y); return s * 2; }
    let source = r#"
        contract Math {
            internal function add(a: int, b: int) -> int {
                return a + b;
            }

            external function sum_and_double(x: int, y: int) -> int {
                var s: int = add(x, y);
                return s * 2;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // sum_and_double(10, 5) = (10 + 5) * 2 = 30
    let calldata =
        encode_call_uint("sum_and_double(uint256,uint256)", &[U256::from(10), U256::from(5)]);
    let result = runtime.call(calldata);

    assert!(result.success, "sum_and_double(10, 5) should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(30),
        "sum_and_double(10, 5) should return (10+5)*2 = 30"
    );
}

#[test]
fn test_simple_return_constant() {
    let source = r#"
        contract Test {
            entrypoint get_answer() -> int {
                return 42;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Test").expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Call get_answer() - signature: "get_answer()"
    let calldata = encode_call_no_args("get_answer()");
    let result = runtime.call(calldata);

    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(42),
        "Should return 42"
    );
}

#[test]
fn test_simple_arithmetic() {
    let source = r#"
        contract Calculator {
            external function add_two_numbers() -> int {
                var x: int = 10;
                var y: int = 32;
                return x + y;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("Calculator")
        .expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Call add_two_numbers()
    let calldata = encode_call_no_args("add_two_numbers()");
    let result = runtime.call(calldata);

    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(42),
        "Should return 10 + 32 = 42"
    );
}

#[test]
fn test_arithmetic_operations() {
    let source = r#"
        contract Math {
            external function compute() -> int {
                var a: int = 100;
                var b: int = 50;
                var c: int = 2;
                var result: int = (a - b) * c;
                return result;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Call compute()
    let calldata = encode_call_no_args("compute()");
    let result = runtime.call(calldata);

    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(100),
        "Should return (100 - 50) * 2 = 100"
    );
}

#[test]
fn test_multiple_functions() {
    let source = r#"
        contract Multi {
            external function get_ten() -> int {
                return 10;
            }

            external function get_twenty() -> int {
                return 20;
            }

            external function sum_both() -> int {
                var x: int = 10;
                var y: int = 20;
                return x + y;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Multi").expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Test get_ten()
    let result = runtime.call(encode_call_no_args("get_ten()"));
    assert!(result.success, "get_ten() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(10),
        "get_ten() should return 10"
    );

    // Test get_twenty()
    let result = runtime.call(encode_call_no_args("get_twenty()"));
    assert!(result.success, "get_twenty() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(20),
        "get_twenty() should return 20"
    );

    // Test sum_both()
    let result = runtime.call(encode_call_no_args("sum_both()"));
    assert!(result.success, "sum_both() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(30),
        "sum_both() should return 30"
    );
}

#[test]
fn test_storage_write_and_read() {
    let source = r#"
        contract Storage {
            state var counter: int = 0;

            external function set_counter() -> int {
                counter = 99;
                return counter;
            }

            external function get_counter() -> int {
                return counter;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("Storage")
        .expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Initially counter should be 0
    let result = runtime.call(encode_call_no_args("get_counter()"));
    assert!(result.success, "get_counter() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(0),
        "Initial counter should be 0"
    );

    // Set counter to 99
    let result = runtime.call(encode_call_no_args("set_counter()"));
    assert!(result.success, "set_counter() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(99),
        "set_counter() should return 99"
    );

    // Verify counter is now 99
    let result = runtime.call(encode_call_no_args("get_counter()"));
    assert!(result.success, "get_counter() should succeed after set");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(99),
        "Counter should be 99 after set"
    );

    // Verify storage slot directly
    let storage_value = runtime.read_storage(U256::from(0));
    assert_eq!(
        storage_value,
        U256::from(99),
        "Storage slot 0 should contain 99"
    );
}

#[test]
fn test_storage_increment() {
    let source = r#"
        contract Counter {
            state var value: int = 0;

            external function increment() -> int {
                var current: int = value;
                var new_value: int = current + 1;
                value = new_value;
                return value;
            }

            external function get_value() -> int {
                return value;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("Counter")
        .expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Initial value should be 0
    let result = runtime.call(encode_call_no_args("get_value()"));
    assert_eq!(result.as_uint().unwrap(), U256::from(0));

    // Increment once
    let result = runtime.call(encode_call_no_args("increment()"));
    assert!(result.success, "First increment should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(1),
        "First increment should return 1"
    );

    // Verify value is 1
    let result = runtime.call(encode_call_no_args("get_value()"));
    assert_eq!(result.as_uint().unwrap(), U256::from(1));

    // Increment again
    let result = runtime.call(encode_call_no_args("increment()"));
    assert!(result.success, "Second increment should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(2),
        "Second increment should return 2"
    );

    // Verify value is 2
    let result = runtime.call(encode_call_no_args("get_value()"));
    assert_eq!(result.as_uint().unwrap(), U256::from(2));
}

#[test]
fn test_boolean_operations() {
    let source = r#"
        contract Logic {
            external function always_true() -> bool {
                return true;
            }

            external function always_false() -> bool {
                return false;
            }

            external function compare_numbers() -> bool {
                var x: int = 10;
                var y: int = 20;
                return x < y;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Logic").expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Test always_true()
    let result = runtime.call(encode_call_no_args("always_true()"));
    assert!(result.success, "always_true() should succeed");
    assert_eq!(
        result.as_bool().expect("Should return bool"),
        true,
        "always_true() should return true"
    );

    // Test always_false()
    let result = runtime.call(encode_call_no_args("always_false()"));
    assert!(result.success, "always_false() should succeed");
    assert_eq!(
        result.as_bool().expect("Should return bool"),
        false,
        "always_false() should return false"
    );

    // Test compare_numbers()
    let result = runtime.call(encode_call_no_args("compare_numbers()"));
    assert!(result.success, "compare_numbers() should succeed");
    assert_eq!(
        result.as_bool().expect("Should return bool"),
        true,
        "10 < 20 should be true"
    );
}

#[test]
fn test_conditional_branches() {
    let source = r#"
        contract Conditional {
            external function max(a: int, b: int) -> int {
                if (a > b) {
                    return a;
                } else {
                    return b;
                }
            }

            external function is_positive(x: int) -> bool {
                if (x > 0) {
                    return true;
                } else {
                    return false;
                }
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("Conditional")
        .expect("Contract not found");

    println!("Bytecode ({}  bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);

    // Test max(10, 5) - should return 10
    let calldata = encode_call_uint("max(uint256,uint256)", &[U256::from(10), U256::from(5)]);
    let result = runtime.call(calldata);
    assert!(result.success, "max(10, 5) should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(10),
        "max(10, 5) should return 10"
    );

    // Test max(3, 15) - should return 15
    let calldata = encode_call_uint("max(uint256,uint256)", &[U256::from(3), U256::from(15)]);
    let result = runtime.call(calldata);
    assert!(result.success, "max(3, 15) should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(15),
        "max(3, 15) should return 15"
    );

    // Test is_positive(42) - should return true
    let calldata = encode_call_uint("is_positive(uint256)", &[U256::from(42)]);
    let result = runtime.call(calldata);
    assert!(result.success, "is_positive(42) should succeed");
    assert_eq!(
        result.as_bool().expect("Should return bool"),
        true,
        "is_positive(42) should return true"
    );
}

#[test]
fn test_constructor_no_params() {
    // Constructor with no params sets storage; external function reads it.
    let source = r#"
        contract WithConstructor {
            state var initial: int = 0;

            constructor() {
                initial = 99;
            }

            external function get_initial() -> int {
                return initial;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("WithConstructor")
        .expect("Contract not found");

    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    // TestRuntime::new runs the creation bytecode via a CREATE transaction.
    let mut runtime = TestRuntime::new(bytecode);

    let result = runtime.call(encode_call_no_args("get_initial()"));
    assert!(result.success, "get_initial() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(99),
        "Constructor should have set initial = 99"
    );
}

#[test]
fn test_constructor_with_param() {
    // Constructor takes a parameter and stores it; external function reads it.
    // This validates Solidity-ABI constructor arg loading (calldata offset 0).
    let source = r#"
        contract Initialized {
            state var value: int = 0;

            constructor(v: int) {
                value = v;
            }

            external function get_value() -> int {
                return value;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled
        .get_contract("Initialized")
        .expect("Contract not found");

    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    // Deploy with constructor argument 42 (ABI: 32 bytes, no selector prefix).
    let mut deploy_input = bytecode.to_vec();
    let arg = U256::from(42).to_big_endian();
    deploy_input.extend_from_slice(&arg);

    let mut runtime = TestRuntime::new(&deploy_input);

    let result = runtime.call(encode_call_no_args("get_value()"));
    assert!(result.success, "get_value() should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(42),
        "Constructor should have stored value = 42"
    );
}
