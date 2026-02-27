// Analyze bytecode generation to find the bug

mod common;

use common::*;
use primitive_types::U256;

#[test]
fn analyze_simple_case() {
    // Test the simplest possible case to isolate the bug
    let source = r#"
        contract Math {
            external function test() -> int {
                var a: int = 100;
                return a;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("\n=== TEST: Return single variable ===");
    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("test()"));

    println!("Result: {:?}", result.as_uint());
    assert_eq!(result.as_uint().unwrap(), U256::from(100), "Should return 100");
}

#[test]
fn analyze_subtraction_order() {
    // Check if SUB operand order is correct
    let source = r#"
        contract Math {
            external function test() -> int {
                var a: int = 100;
                var b: int = 50;
                return a - b;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("\n=== TEST: a - b where a=100, b=50 ===");
    println!("Bytecode ({} bytes):", bytecode.len());

    // Print bytecode in readable format
    for (i, byte) in bytecode.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!();

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("test()"));

    println!("\nExpected: 50 (100 - 50)");
    println!("Got: {:?}", result.as_uint());

    if let Some(value) = result.as_uint() {
        if value == U256::from(50) {
            println!("✓ CORRECT");
        } else {
            // Check if it's the negative (complemento a 2)
            let max = U256::max_value();
            let as_negative = max - value + U256::from(1);
            println!("✗ WRONG - Got complement of: {}", as_negative);
        }
    }
}

#[test]
fn analyze_memory_slots() {
    // Test if memory slot assignment is working correctly
    let source = r#"
        contract Math {
            external function test() -> int {
                var a: int = 10;
                var b: int = 20;
                var c: int = 30;
                return c;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Math").expect("Contract not found");

    println!("\n=== TEST: Multiple variables, return last ===");
    println!("Testing if memory slots work correctly");
    println!("Bytecode ({} bytes): {:02x?}", bytecode.len(), bytecode);

    let mut runtime = TestRuntime::new(bytecode);
    let result = runtime.call(encode_call_no_args("test()"));

    println!("Expected: 30");
    println!("Got: {:?}", result.as_uint());
    assert_eq!(result.as_uint().unwrap(), U256::from(30), "Should return 30");
}
