// Debug test to isolate the arithmetic problem

mod common;

use common::*;
use primitive_types::U256;

#[test]
fn test_simple_subtraction() {
    let source = r#"
        contract Math {
            external function subtract() -> int {
                var a: int = 100;
                var b: int = 50;
                var result: int = a - b;
                return result;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode: {:02x?}", bytecode);

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("subtract()"));

    println!("Result: {:?}", result.as_uint());
    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(50),
        "Should return 100 - 50 = 50"
    );
}

#[test]
fn test_simple_multiplication() {
    let source = r#"
        contract Math {
            external function multiply() -> int {
                var a: int = 50;
                var b: int = 2;
                var result: int = a * b;
                return result;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode: {:02x?}", bytecode);

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("multiply()"));

    println!("Result: {:?}", result.as_uint());
    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(100),
        "Should return 50 * 2 = 100"
    );
}

#[test]
fn test_two_step_arithmetic() {
    let source = r#"
        contract Math {
            external function compute() -> int {
                var a: int = 100;
                var b: int = 50;
                var temp: int = a - b;
                var c: int = 2;
                var result: int = temp * c;
                return result;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("Bytecode: {:02x?}", bytecode);

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("compute()"));

    println!("Result: {:?}", result.as_uint());
    assert!(result.success, "Call should succeed");
    assert_eq!(
        result.as_uint().expect("Should return uint256"),
        U256::from(100),
        "Should return (100 - 50) * 2 = 100"
    );
}
