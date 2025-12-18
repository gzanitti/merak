mod common;
use common::*;

/// Test 1: Variable with explicit refinement type
#[test]
fn test_explicit_refinement_type() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should accept valid explicit refinement");
}

/// Test 2: Variable without type - basic inference
#[test]
fn test_basic_type_inference() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x = 5;
                var y = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should infer types for untyped variables");
}

/// Test 3: Invalid assignment - subtyping violation
#[test]
fn test_invalid_subtyping() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = -5;  // ERROR: -5 no cumple x > 0
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "Should reject invalid refinement");

    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("cannot satisfy subtyping"),
        "Error should mention subtyping/refinement violation"
    );
}

/// Test 4: Binary operation - arithmetic constraint
#[test]
fn test_binary_operation_inference() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
                var y: {int | y > 0} = 3;
                var z = x + y;  // z debería inferirse como > 0
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Should infer type for binary operation result"
    );
}

/// Test 5: Function call - verify precondition
#[test]
fn test_function_requires_valid() {
    let source = r#"
        contract Test {
            internal function helper(n: {int | n > 0}) {
                var x = n;
            }
            
            entrypoint foo() {
                helper(5);  // OK: 5 > 0
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should accept valid precondition");
}

/// Test 5b: Function call - precondition violation
#[test]
fn test_function_requires_invalid() {
    let source = r#"
        contract Test {
            internal function helper(n: {int | n > 0}) {
                var x = n;
            }
            
            entrypoint foo() {
                helper(-5);  // ERROR: -5 > 0 is false
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "Should reject invalid precondition");

    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("Requires") || error.contains("precondition"),
        "Error should mention precondition violation"
    );
}

/// Test 6: Function ensures - postcondition must hold
#[test]
fn test_function_ensures_valid() {
    let source = r#"
        contract Test {
            internal function get_positive() -> {int | return > 0}
                ensures(return > 0)
            {
                return 10;
            }
            
            entrypoint foo() {
                var x = get_positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should accept valid postcondition");
}

/// Test 6b: Function ensures - postcondition violation
#[test]
fn test_function_ensures_invalid() {
    let source = r#"
        contract Test {
            internal function get_positive() -> {int | return > 0}
                ensures(return > 0)
            {
                return -5;  // ERROR: -5 > 0 is false
            }
            
            entrypoint foo() {
                var x = get_positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "Should reject invalid postcondition");

    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("Ensures") || error.contains("postcondition"),
        "Error should mention postcondition violation"
    );
}

/// Test 7: Storage fold - verify invariant maintained
#[test]
fn test_storage_fold_valid() {
    let source = r#"
        contract Test {
            state var balance: {int | balance >= 0} = 0;
            
            entrypoint deposit(amount: {int | amount >= 0}) {
                balance = balance + amount;  // OK: 0 + 0 >= 0
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should accept when invariant is maintained");
}

/// Test 7b: Storage fold - invariant violation
#[test]
fn test_storage_fold_invalid() {
    let source = r#"
        contract Test {
            state var balance: {int | balance >= 0} = 0;
            
            entrypoint withdraw(amount: int) {
                balance = balance - amount;  // ERROR: can be negative
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_err(),
        "Should reject when invariant can be violated"
    );

    let error = format!("{:?}", result.unwrap_err());
    println!("{error}");
    assert!(
        error.contains("subtyping"),
        "Error should mention subtyping constraint violation"
    );
}

/// Test 8: Conditional - path-sensitive typing
#[test]
fn test_conditional_path_sensitivity() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: {int | y > 0} = x;  // OK inside then branch
                } else {
                    var z: {int | z <= 0} = x;  // OK inside else branch
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "Should handle path-sensitive refinements");
}

/// Test 8b: Conditional - invalid in wrong branch
#[test]
fn test_conditional_invalid_branch() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: {int | y <= 0} = x;  // ERROR: x > 0 but we need y <= 0
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_err(),
        "Should reject refinement that contradicts branch condition"
    );
}
