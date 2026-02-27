// Debug storage operations

mod common;

use common::*;
use primitive_types::U256;

#[test]
fn debug_simple_storage() {
    let source = r#"
        contract Test {
            state var value: int = 0;

            external function set_value() -> int {
                value = 42;
                return value;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Test").expect("Contract not found");

    println!("\n=== DEBUG: Simple Storage Test ===");
    println!("Bytecode ({} bytes):", bytecode.len());
    for (i, byte) in bytecode.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!("\n");

    let mut runtime = TestRuntime::new(bytecode);

    // Call set_value()
    let result = runtime.call(encode_call_no_args("set_value()"));

    println!("Call succeeded: {}", result.success);
    if let Some(val) = result.as_uint() {
        println!("Returned value: {}", val);
    }

    // Read storage directly
    let storage_value = runtime.read_storage(U256::from(0));
    println!("Storage slot 0: {}", storage_value);

    assert!(result.success, "set_value() should succeed");
    assert_eq!(result.as_uint().unwrap(), U256::from(42), "Should return 42");
    assert_eq!(storage_value, U256::from(42), "Storage should be 42");
}

#[test]
fn debug_storage_read() {
    let source = r#"
        contract Test {
            state var value: int = 0;

            external function get_value() -> int {
                return value;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Test").expect("Contract not found");

    println!("\n=== DEBUG: Storage Read Test ===");
    println!("Bytecode ({} bytes):", bytecode.len());
    for (i, byte) in bytecode.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!("\n");

    let mut runtime = TestRuntime::new(bytecode);

    // Manually set storage slot 0 to 42
    println!("Setting storage slot 0 to 42 manually...");
    // Note: This requires adding a method to TestRuntime to set storage

    // Call get_value()
    let result = runtime.call(encode_call_no_args("get_value()"));

    println!("Call succeeded: {}", result.success);
    if let Some(val) = result.as_uint() {
        println!("Returned value: {}", val);
        println!("Expected: 0 (since we can't set storage manually yet)");
    }

    assert!(result.success, "get_value() should succeed");
    // Should return 0 since storage is initially 0
    assert_eq!(result.as_uint().unwrap(), U256::from(0), "Should return 0");
}
