

use crate::common::*;

/// Constant satisfies annotation: Ty(5) = {int | ν = 5}, and ν = 5 ⇒ ν > 0.
#[test]
fn test_constant_satisfies_annotation() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "5 satisfies {{x > 0}}: {:?}", result.err());
}

/// Constant violates annotation: Ty(-5) = {int | ν = -5}, and ν = -5 ⇏ ν > 0.
#[test]
fn test_constant_violates_annotation() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = -5;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "-5 does not satisfy {{x > 0}}");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("cannot satisfy subtyping"),
        "Expected subtyping error, got: {error}"
    );
}

/// Unannotated copy: the solver infers κ via iterative weakening.
/// No hard constraints are violated, so the solver converges trivially.
#[test]
fn test_unannotated_copy_infers_type() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x = 5;
                var y = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Unannotated copies always have a valid assignment: {:?}",
        result.err()
    );
}

/// Annotated copy where source refinement entails target refinement:
/// {int | x > 0} <: {int | y > 0}  via  x > 0 ⇒ y > 0 (after binder substitution).
#[test]
fn test_annotated_copy_compatible() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
                var y: {int | y > 0} = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "{{x > 0}} <: {{y > 0}}: {:?}", result.err());
}

/// Annotated copy with incompatible refinements:
/// {int | x > 0} ⊄: {int | y < 0}  because  x > 0 ⇏ y < 0.
#[test]
fn test_annotated_copy_incompatible() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
                var y: {int | y < 0} = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "{{x > 0}} is not a subtype of {{y < 0}}");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("cannot satisfy subtyping"),
        "Expected subtyping error, got: {error}"
    );
}

/// Sum of positive values: x > 0 ∧ y > 0 ∧ z = x + y ⇒ z > 0.
/// The solver infers κ_z ≡ True (or stronger) since no annotation constrains z.
#[test]
fn test_addition_of_positives() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
                var y: {int | y > 0} = 3;
                var z = x + y;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Addition of positives is safe: {:?}",
        result.err()
    );
}

/// Subtraction under a guard: the path assumption `a >= b` makes
/// the fact a - b >= 0 provable via a >= b ∧ result = a - b ⇒ result >= 0.
#[test]
fn test_guarded_subtraction_non_negative() {
    let source = r#"
        contract Test {
            entrypoint safe_sub(a: {a: int | a >= 0}, b: {b: int | b >= 0}) {
                if (a >= b) {
                    var result: {int | result >= 0} = a - b;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "a >= b ∧ result = a - b ⇒ result >= 0: {:?}",
        result.err()
    );
}

/// Unary negation: x > 0 ∧ y = -x ⇒ y < 0.
/// The UnaryOp constraint encodes result = -operand.
#[test]
fn test_unary_negation() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
                var y: {int | y < 0} = -x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "-(positive) is negative: {:?}",
        result.err()
    );
}

/// Refined parameter as precondition: argument must satisfy parameter's
/// refinement at the call site. Ty(5) = {ν = 5} <: {n > 0} holds.
#[test]
fn test_refined_param_valid_call() {
    let source = r#"
        contract Test {
            internal function helper(n: {int | n > 0}) {
                var x = n;
            }

            entrypoint foo() {
                helper(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "5 satisfies {{n > 0}}: {:?}", result.err());
}

/// Refined parameter precondition violation: Ty(-5) = {ν = -5} ⊄: {n > 0}.
#[test]
fn test_refined_param_invalid_call() {
    let source = r#"
        contract Test {
            internal function helper(n: {int | n > 0}) {
                var x = n;
            }

            entrypoint foo() {
                helper(-5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "-5 does not satisfy {{n > 0}}");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("cannot satisfy subtyping"),
        "Expected subtyping error, got: {error}"
    );
}

/// Multiple refined parameters: each argument must independently satisfy
/// its corresponding parameter refinement. 3 > 0 ∧ 7 > 0 both hold.
#[test]
fn test_multiple_refined_params() {
    let source = r#"
        contract Test {
            internal function add_positives(a: {a: int | a > 0}, b: {b: int | b > 0}) -> int {
                var result = a + b;
                return result;
            }

            entrypoint foo() {
                add_positives(3, 7);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Both args satisfy their params: {:?}",
        result.err()
    );
}

/// Return type refinement satisfied: return 5 with → {int | v > 0}.
/// Subtype constraint: Ty(5) <: {v > 0}, i.e., ν = 5 ⇒ ν > 0.
#[test]
fn test_return_type_valid() {
    let source = r#"
        contract Test {
            internal function positive() -> {int | v > 0} {
                return 5;
            }

            entrypoint foo() {
                positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "5 satisfies {{v > 0}}: {:?}", result.err());
}

#[test]
fn test_return_type_valid_var() {
    let source = r#"
        contract Test {
            internal function positive() -> {int | v > 0} {
                return 5;
            }

            entrypoint foo() {
                var res: int = positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "5 satisfies {{v > 0}}: {:?}", result.err());
}

#[test]
fn test_return_type_valid_typed_var() {
    let source = r#"
        contract Test {
            internal function positive() -> {int | v > 0} {
                return 5;
            }

            entrypoint foo() {
                var res: {int | res > 0} = positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "5 satisfies {{v > 0}}: {:?}", result.err());
}

/// Return type refinement violated: return -1 with → {int | v > 0}.
/// ν = -1 ⇏ ν > 0.
#[test]
fn test_return_type_invalid() {
    let source = r#"
        contract Test {
            internal function positive() -> {int | v > 0} {
                return -1;
            }

            entrypoint foo() {
                positive();
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "-1 does not satisfy {{v > 0}}");
}

/// Both branches carry their respective path assumptions:
///   then: x > 0, so y: {y > 0} = x is valid
///   else: ¬(x > 0), so z: {z ≤ 0} = x is valid
#[test]
fn test_path_sensitive_then_else() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: {int | y > 0} = x;
                } else {
                    var z: {int | z <= 0} = x;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Path conditions refine x in each branch: {:?}",
        result.err()
    );
}
/// Annotation contradicts the branch condition:
/// In the then-branch x > 0 holds, so x ≤ 0 is unsatisfiable.
#[test]
fn test_annotation_contradicts_branch() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: {int | y <= 0} = x;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "x > 0 contradicts y <= 0");
}

/// Nested conditionals accumulate path conditions:
/// After `if (x > 0)` then `if (x > 5)`, we have Γ ∧ x > 0 ∧ x > 5,
/// making y > 5 provable.
#[test]
fn test_nested_conditionals_accumulate() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                if (x > 0) {
                    if (x > 5) {
                        var y: {int | y > 5} = x;
                    }
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_ok(), "x > 0 ∧ x > 5 ⇒ y > 5: {:?}", result.err());
}

/// Valid fold: balance >= 0 ∧ amount >= 0 ∧ new = balance + amount ⇒ new >= 0.
#[test]
fn test_fold_valid_deposit() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint deposit(amount: {amount: int | amount >= 0}) {
                balance = balance + amount;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "balance + amount >= 0 when both >= 0: {:?}",
        result.err()
    );
}

/// Invalid fold: balance - amount can be negative for arbitrary amount.
/// No guard ensures amount ≤ balance, so the invariant may be violated.
#[test]
fn test_fold_invalid_unchecked_withdraw() {
    let source = r#"
        contract Test {
            state var balance: {int | balance >= 0} = 0;

            entrypoint withdraw(amount: int) {
                balance = balance - amount;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "balance - amount may be negative");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("subtyping") || error.contains("storage invariant"),
        "Expected fold/subtyping error, got: {error}"
    );
}

/// Classic safe withdraw: the guard `balance >= amount` combined with
/// `amount >= 0` makes balance - amount >= 0 provable.
#[test]
fn test_fold_guarded_withdraw() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint withdraw(amount: {amount: int | amount >= 0}) {
                if (balance >= amount) {
                    balance = balance - amount;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Guard ensures balance - amount >= 0: {:?}",
        result.err()
    );
}

/// Multiple storage variables: each fold must independently satisfy
/// its respective invariant. x + dx >= 0 and y + dy >= 0 both hold.
#[test]
fn test_fold_multiple_storage_variables() {
    let source = r#"
        contract Test {
            state var x: {x: int | x >= 0} = 0;
            state var y: {y: int | y >= 0} = 0;

            entrypoint increment_both(dx: {dx: int | dx >= 0}, dy: {dy: int | dy >= 0}) {
                x = x + dx;
                y = y + dy;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Each storage invariant independently maintained: {:?}",
        result.err()
    );
}

/// All four obligations satisfied: i >= 0 established by n >= 0,
/// preserved by i = i - 1 under guard i > 0 (since i > 0 ∧ i' = i - 1 ⇒ i' >= 0),
/// variant i decreases by 1, and i ≥ 0 after decrement when i > 0.
#[test]
fn test_loop_valid_countdown() {
    let source = r#"
        contract Test {
            entrypoint countdown(n: {n: int | n >= 0}) {
                var i = n;
                while (i > 0) with @invariant(i >= 0) @variant(i) {
                    i = i - 1;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Countdown satisfies all loop obligations: {:?}",
        result.err()
    );
}

/// Invariant NOT preserved: when i = 1, the body sets i = 0, violating i > 0.
/// The invariant i > 0 is too strong — only i >= 0 is preserved.
/// Fails on LoopInvariantPreservation.
#[test]
fn test_loop_invariant_not_preserved() {
    let source = r#"
        contract Test {
            entrypoint bad_loop(n: {n: int | n >= 0}) {
                var i = n;
                while (i > 0) with @invariant(i > 0) @variant(i) {
                    i = i - 1;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_err(),
        "i > 0 not preserved: i = 1 → i' = 0 violates i' > 0"
    );
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("Loop invariant") || error.contains("subtyping"),
        "Expected loop invariant error, got: {error}"
    );
}

/// Without param refinement, the body cannot prove x > 0, even though
/// the call site passes a valid argument.
#[test]
fn test_body_fails_without_param_refinement() {
    let source = r#"
        contract Test {
            internal function foo(x: int) -> int {
                var y: {int | y > 0} = x;
                return y;
            }

            entrypoint bar() {
                foo(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_err(),
        "Without param refinement, x: int has no guarantee of x > 0"
    );
}

/// requires(x > 0) where x is a parameter is rejected: use param refinements.
#[test]
fn test_requires_param_only_rejected() {
    let source = r#"
        contract Test {
            internal function foo(x: int) requires(x > 0) -> int {
                return x;
            }

            entrypoint bar() {
                foo(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "requires with param-only predicate should be rejected");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("only references parameters/locals"),
        "Expected validation error, got: {error}"
    );
}

/// ensures(x > 0) where x is a parameter is rejected: use return type refinement.
#[test]
fn test_ensures_param_only_rejected() {
    let source = r#"
        contract Test {
            internal function foo(x: int) ensures(x > 0) -> int {
                return x;
            }

            entrypoint bar() {
                foo(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "ensures with param-only predicate should be rejected");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("only references parameters/locals"),
        "Expected validation error, got: {error}"
    );
}

/// requires with multiple param-only predicates is also rejected.
#[test]
fn test_requires_multiple_params_rejected() {
    let source = r#"
        contract Test {
            internal function foo(a: int, b: int) requires(a > 0, b > 0) -> int {
                return a;
            }

            entrypoint bar() {
                foo(3, 7);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "requires with param-only predicates should be rejected");
    let error = format!("{:?}", result.unwrap_err());
    assert!(
        error.contains("only references parameters/locals"),
        "Expected validation error, got: {error}"
    );
}

/// requires referencing a storage variable is accepted by validation.
/// The predicate mixes storage (balance) and parameter (amount) — valid.
#[test]
fn test_requires_with_storage_accepted() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint withdraw(amount: {amount: int | amount >= 0})
                requires(balance >= amount)
            {
                if (balance >= amount) {
                    balance = balance - amount;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    if let Err(ref e) = result {
        let error = format!("{:?}", e);
        assert!(
            !error.contains("only references parameters/locals"),
            "Storage requires should pass validation, got: {error}"
        );
    }
}

/// ensures referencing a storage variable is accepted by validation.
#[test]
fn test_ensures_with_storage_accepted() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint deposit(amount: {amount: int | amount >= 0})
                ensures(balance >= 0)
            {
                balance = balance + amount;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    if let Err(ref e) = result {
        let error = format!("{:?}", e);
        assert!(
            !error.contains("only references parameters/locals"),
            "Storage ensures should pass validation, got: {error}"
        );
    }
}

/// Return type refinement references an input parameter: {v > x}.
/// return x + 1 satisfies v > x since (x + 1) > x is always true.
#[test]
fn test_return_type_references_input_param() {
    let source = r#"
        contract Test {
            internal function increment(x: int) -> {v: int | v > x} {
                var result = x + 1;
                return result;
            }

            entrypoint foo() {
                increment(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "x + 1 satisfies {{v > x}}: {:?}",
        result.err()
    );
}

/// Return type cross-reference violated: x - 1 does NOT satisfy v > x.
/// x - 1 > x is false for all x.
#[test]
fn test_return_type_references_input_param_invalid() {
    let source = r#"
        contract Test {
            internal function decrement(x: int) -> {v: int | v > x} {
                var result = x - 1;
                return result;
            }

            entrypoint foo() {
                decrement(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(result.is_err(), "x - 1 does not satisfy {{v > x}}");
}

/// Return type references input with refined param: x >= 0 ∧ v = x + 1 ⇒ v > 0.
/// The cross-reference to x provides the needed assumption.
#[test]
fn test_return_type_cross_ref_with_param_refinement() {
    let source = r#"
        contract Test {
            internal function inc_positive(x: {x: int | x >= 0}) -> {v: int | v > 0} {
                var result = x + 1;
                return result;
            }

            entrypoint foo() {
                inc_positive(5);
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "x >= 0 ∧ result = x + 1 ⇒ result > 0: {:?}",
        result.err()
    );
}


/// Progressive type strengthening: the solver automatically infers
/// the strongest valid refinement for unannotated variables by combining
/// parameter refinements, arithmetic facts, and path conditions.
///
/// The SOLVE algorithm:
///   - Starts with κ_sum ≡ True (weakest type for sum)
///   - Generates constraints from arithmetic: sum = a + b
///   - Adds path assumption: sum > 10 (from if-guard)
///   - Strengthens κ_sum by instantiating qualifiers until
///     a > 0 ∧ b > 5 ∧ sum = a + b ∧ sum > 10 ⊢ sum > 10 holds
#[test]
fn test_iterative_type_strengthening() {
    let source = r#"
        contract Test {
            entrypoint demo(a: {a: int | a > 0}, b: {b: int | b > 5}) {
                var sum = a + b;
                if (sum > 10) {
                    var result: {r: int | r > 10} = sum;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Solver strengthens κ_sum by combining: a > 0 ∧ b > 5 ∧ sum = a + b ∧ [sum > 10] ⊢ sum > 10: {:?}",
        result.err()
    );
}

/// Multiple usage contexts: a variable used in different assignments with
/// progressively stronger refinement requirements.
///
/// The solver must infer that x satisfies multiple constraints:
///   - From arithmetic: a > 10 ∧ b > 20 ∧ x = a + b ⊢ x > 30
///   - First use: x must satisfy y > 20 (since y = x and y has that annotation)
///   - Second use: x must satisfy z > 29 (since z = x and z has that annotation)
///
/// Without the solver combining these constraints, one of the assignments
/// would fail. The algorithm finds that κ_x must be strong enough
/// to satisfy both y > 20 AND z > 29 simultaneously.
#[test]
fn test_multiple_constraints_strengthening() {
    let source = r#"
        contract Test {
            entrypoint demo(a: {a: int | a > 10}, b: {b: int | b > 20}) {
                var x = a + b;
                var y: {y: int | y > 20} = x;
                var z: {z: int | z > 29} = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Solver infers κ_x to satisfy x > 20 ∧ x > 29 via a > 10 ∧ b > 20: {:?}",
        result.err()
    );
}

/// Join point strengthening: a variable assigned in different branches
/// must satisfy the INTERSECTION of refinements from all paths.
///
/// This is a classic scenario:
///   - In the then-branch (n > 5): x ← 50 (a positive constant)
///   - In the else-branch (n ≤ 5): x ← a + b where a > 10, b > 20
///   - At the join point (φ-node): x must satisfy result > 25
///
/// The solver must strengthen κ_x at the φ-node to satisfy:
///   1. From then-branch: 50 > 25 ✓
///   2. From else-branch: a > 10 ∧ b > 20 ∧ x = a + b ⊢ x > 30 ⊢ x > 25 ✓
///
/// This requires the solver to verify that BOTH paths produce values > 25.
#[test]
fn test_join_point_strengthening() {
    let source = r#"
        contract Test {
            entrypoint demo(n: int, a: {a: int | a > 10}, b: {b: int | b > 20}) {
                var x = 0;
                if (n > 5) {
                    x = 50;
                } else {
                    x = a + b;
                }
                var result: {r: int | r > 25} = x;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Solver verifies both paths: 50 > 25 ∧ (a + b > 30 > 25): {:?}",
        result.err()
    );
}

/// Strengthening failure: the solver cannot find a valid assignment for κ
/// that satisfies the constraint. Even though a > 0 and b > 0, we cannot
/// prove sum > 100 without additional information.
///
/// This demonstrates that the solver correctly rejects programs when
/// no amount of qualifier instantiation can satisfy the constraints.
#[test]
fn test_strengthening_insufficient_information() {
    let source = r#"
        contract Test {
            entrypoint demo(a: {a: int | a > 0}, b: {b: int | b > 0}) {
                var sum = a + b;
                var result: {r: int | r > 100} = sum;
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_err(),
        "Cannot prove sum > 100 from just a > 0 ∧ b > 0"
    );
}


/// Cross-references to parameters in refinements.
#[test]
fn test_cross_ref_to_parameter_in_refinement() {
    let source = r#"
        contract Test {
            entrypoint demo(a: {a: int | a >= 0}) {
                var x = a + 1;
                var pos: {p: int | p > 0} = x;
                if (x > a) {
                    var greater: {g: int | g > a} = x;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Cross-ref to param 'a' should resolve to 'a_0': {:?}",
        result.err()
    );
}

/// Boolean parameters in branch conditions.
#[test]
fn test_bool_param_in_branch_condition() {
    let source = r#"
        contract Test {
            entrypoint demo(flag: bool, a: {a: int | a > 0}) {
                var x = 0;
                if (flag) {
                    x = a;
                }
            }
        }
    "#;

    let result = run_refinement_inference(source);
    assert!(
        result.is_ok(),
        "Bool param in branch should not panic: {:?}",
        result.err()
    );
}
