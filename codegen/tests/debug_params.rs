// Debug parameter loading

mod common;

use common::*;
use primitive_types::U256;

#[test]
fn debug_simple_param() {
    let source = r#"
        contract Test {
            external function identity(x: int) -> int {
                return x;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Test").expect("Contract not found");

    println!("\n=== DEBUG: Simple Parameter Test ===");
    println!("Bytecode ({} bytes):", bytecode.len());
    for (i, byte) in bytecode.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!("\n");

    let mut runtime = TestRuntime::new(bytecode);

    // Call identity(42)
    // Note: Use ABI type names, not Merak type names (int → uint256)
    let calldata = encode_call_uint("identity(uint256)", &[U256::from(42)]);
    println!("Calling identity(42)");
    println!("Calldata: {:02x?}", calldata);

    let result = runtime.call(calldata);

    println!("Call succeeded: {}", result.success);
    println!("Gas used: {}", result.gas_used);
    if let Some(val) = result.as_uint() {
        println!("Returned value: {}", val);
    } else {
        println!("Return data: {:02x?}", result.return_data);
    }

    assert!(result.success, "identity(42) should succeed");
    assert_eq!(result.as_uint().unwrap(), U256::from(42), "Should return 42");
}

#[test]
fn debug_two_params() {
    let source = r#"
        contract Test {
            external function add(a: int, b: int) -> int {
                return a + b;
            }
        }
    "#;

    let (compiled, _symbols) = compile_from_source(source).expect("Compilation failed");
    let bytecode = compiled.get_contract("Test").expect("Contract not found");

    println!("\n=== DEBUG: Two Parameters Test ===");
    println!("Bytecode ({} bytes):", bytecode.len());
    for (i, byte) in bytecode.iter().enumerate() {
        if i % 16 == 0 {
            print!("\n{:04x}: ", i);
        }
        print!("{:02x} ", byte);
    }
    println!("\n");

    let mut runtime = TestRuntime::new(bytecode);

    // Call add(10, 32)
    // Note: Use ABI type names (int → uint256)
    let calldata = encode_call_uint("add(uint256,uint256)", &[U256::from(10), U256::from(32)]);
    println!("Calling add(10, 32)");
    println!("Calldata ({} bytes): {:02x?}", calldata.len(), calldata);

    let result = runtime.call(calldata);

    println!("Call succeeded: {}", result.success);
    println!("Gas used: {}", result.gas_used);
    if let Some(val) = result.as_uint() {
        println!("Returned value: {}", val);
    } else {
        println!("Return data: {:02x?}", result.return_data);
    }

    assert!(result.success, "add(10, 32) should succeed");
    assert_eq!(result.as_uint().unwrap(), U256::from(42), "Should return 42");
}
