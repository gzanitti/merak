use merak_parser::parse_program;
use merak_ast::{NodeIdGenerator, statement::Statement};

#[test]
fn test_function_with_return_type_gets_void_return() {
    let input = r#"
    contract C {
        state var x: int = 0;

        entrypoint foo() -> int {
            var y: int = 1;
            return;
        }
    }"#;
    let id_gen = NodeIdGenerator::new();
    let program = parse_program(input, &id_gen).expect("Should parse successfully");
    let function = program.contract.functions.first().expect("Should have state def");
    
    // Should have 2 statements: var declaration + auto-inserted return
    assert_eq!(function.body.statements.len(), 2);
    
    // Last statement should be a void return statement (even for functions with return type)
    match function.body.statements.last().unwrap() {
        Statement::Return(None, _, _) => {
            println!("✓ Inserted void return statement");
        }
        other => panic!("Expected void return statement, got: {:?}", other),
    }
}

#[test]
fn test_function_without_return_type_gets_void_return() {
    let input = r#"
    contract C {
        state var x: int = 0;

        internal function bar() {
            var y: int = 1;
        }
    }"#;
    
    let id_gen = NodeIdGenerator::new();

    let program = parse_program(input, &id_gen).expect("Should parse successfully");
    let function = program.contract.functions.first().expect("Should have state def");
    
    // Should have 2 statements: var declaration + auto-inserted return
    assert_eq!(function.body.statements.len(), 2);
    
    // Last statement should be a void return statement
    match function.body.statements.last().unwrap() {
        Statement::Return(None, _, _) => {
            println!("✓ Auto-inserted void return statement");
        }
        other => panic!("Expected void return statement, got: {:?}", other),
    }
}

#[test]
fn test_function_with_existing_return_unchanged() {
    let input = r#"
    contract C {
        state var x: int = 0;

        external function baz() -> int {
            var y: int = 1;
            return y;
        }
    }"#;

    let id_gen = NodeIdGenerator::new();
    let program = parse_program(input, &id_gen).expect("Should parse successfully");
    let function = program.contract.functions.first().expect("Should have state def");

    // Should have exactly 2 statements: var declaration + existing return
    assert_eq!(function.body.statements.len(), 2);

    // Last statement should be the original return statement
    match function.body.statements.last().unwrap() {
        Statement::Return(Some(_), _, _) => {
            println!("✓ Existing return statement preserved");
        }
        other => panic!("Expected return statement, got: {:?}", other),
    }
}