use merak_parser::parse_program;
use merak_ast::statement::Statement;

#[test]
fn test_function_with_return_type_gets_void_return() {
    let input = r#"
    contract C[Open] {
        state var x: int = 0;
    }
    C@Open(any) {
        function foo() -> int {
            var y: int = 1;
        }
    }"#;
    
    let program = parse_program(input).expect("Should parse successfully");
    let (_, state_def) = program.state_defs.first().expect("Should have state def");
    let function = state_def.functions.first().expect("Should have function");
    
    // Should have 2 statements: var declaration + auto-inserted return
    assert_eq!(function.body.statements.len(), 2);
    
    // Last statement should be a void return statement (even for functions with return type)
    match function.body.statements.last().unwrap() {
        Statement::Return(None, _, _) => {
            println!("✓ Auto-inserted void return statement");
        }
        other => panic!("Expected void return statement, got: {:?}", other),
    }
}

#[test]
fn test_function_without_return_type_gets_void_return() {
    let input = r#"
    contract C[Open] {
        state var x: int = 0;
    }
    C@Open(any) {
        function bar() {
            var y: int = 1;
        }
    }"#;
    
    let program = parse_program(input).expect("Should parse successfully");
    let (_, state_def) = program.state_defs.first().expect("Should have state def");
    let function = state_def.functions.first().expect("Should have function");
    
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
    contract C[Open] {
        state var x: int = 0;
    }
    C@Open(any) {
        function baz() -> int {
            var y: int = 1;
            return y;
        }
    }"#;

    let program = parse_program(input).expect("Should parse successfully");
    let (_, state_def) = program.state_defs.first().expect("Should have state def");
    let function = state_def.functions.first().expect("Should have function");

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