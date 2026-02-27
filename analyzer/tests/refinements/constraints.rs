use crate::common::*;
use merak_analyzer::refinements::constraints::{Constraint, ConstraintSet};
use merak_ast::expression::BinaryOperator;
use merak_ast::predicate::{Predicate, RelOp};

/// Count constraints matching a predicate
fn count_constraints<F>(cs: &ConstraintSet, predicate: F) -> usize
where
    F: Fn(&Constraint) -> bool,
{
    cs.iter().filter(|c| predicate(c)).count()
}

fn is_subtype(c: &Constraint) -> bool {
    matches!(c, Constraint::Subtype { .. })
}

fn is_wellformed(c: &Constraint) -> bool {
    matches!(c, Constraint::WellFormed { .. })
}

fn is_binop(c: &Constraint) -> bool {
    matches!(c, Constraint::BinaryOp { .. })
}

#[allow(dead_code)]
fn is_unaryop(c: &Constraint) -> bool {
    matches!(c, Constraint::UnaryOp { .. })
}

fn is_requires(c: &Constraint) -> bool {
    matches!(c, Constraint::Requires { .. })
}

fn is_ensures(c: &Constraint) -> bool {
    matches!(c, Constraint::Ensures { .. })
}

fn is_fold(c: &Constraint) -> bool {
    matches!(c, Constraint::Fold { .. })
}

fn is_loop_invariant_entry(c: &Constraint) -> bool {
    matches!(c, Constraint::LoopInvariantEntry { .. })
}

fn is_loop_invariant_preservation(c: &Constraint) -> bool {
    matches!(c, Constraint::LoopInvariantPreservation { .. })
}

fn is_loop_variant_nonneg(c: &Constraint) -> bool {
    matches!(c, Constraint::LoopVariantNonNegative { .. })
}

fn is_loop_variant_decreases(c: &Constraint) -> bool {
    matches!(c, Constraint::LoopVariantDecreases { .. })
}

#[test]
fn test_copy_generates_subtype() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int) {
                var x = a;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    // Copy(a_0 -> x_0) should generate at least one Subtype constraint
    let subtypes: Vec<&Constraint> = foo.iter().filter(|c| is_subtype(c)).collect();
    assert!(
        !subtypes.is_empty(),
        "Copy instruction should generate at least one Subtype constraint"
    );

    // Find the specific Subtype for the copy: source <: dest
    // The sub template should reference a_0, the sup should reference x_0
    let copy_subtype = subtypes.iter().find(|c| {
        if let Constraint::Subtype { sub, sup, .. } = c {
            sub.binder() == "a_0" && sup.binder() == "x_0"
        } else {
            false
        }
    });

    assert!(
        copy_subtype.is_some(),
        "Should find Subtype(a_0 <: x_0) from Copy instruction. \
         Available subtypes: {:?}",
        subtypes
            .iter()
            .map(|c| format!("{}", c))
            .collect::<Vec<_>>()
    );

    // The context should have bindings for a_0 (parameter in scope)
    if let Some(Constraint::Subtype { context, .. }) = copy_subtype {
        assert!(
            context.in_scope("a_0"),
            "Context should contain binding for parameter a_0"
        );
    }
}

#[test]
fn test_arithmetic_generates_wellformed_and_binop() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var z = a + b;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    // Should have at least 3 WellFormed constraints (left, right, dest)
    let wf_count = count_constraints(foo, is_wellformed);
    assert!(
        wf_count >= 3,
        "BinaryOp should generate at least 3 WellFormed constraints \
         (left, right, dest), got {}",
        wf_count
    );

    // Should have exactly 1 BinaryOp constraint
    let binop_count = count_constraints(foo, is_binop);
    assert_eq!(
        binop_count, 1,
        "Should generate exactly 1 BinaryOp constraint for a + b"
    );

    // Inspect the BinaryOp constraint
    let binop = foo.iter().find(|c| is_binop(c)).unwrap();
    if let Constraint::BinaryOp {
        op,
        result,
        context,
        ..
    } = binop
    {
        assert_eq!(*op, BinaryOperator::Add, "Operator should be Add");
        // The result template should be Concrete (ty(+) from paper: fully determined)
        assert!(
            result.is_concrete(),
            "Arithmetic result template should be Concrete (ty(+) from paper)"
        );
        // Context should have the operands in scope
        assert!(
            context.in_scope("a_0") && context.in_scope("b_0"),
            "Context should have both operands in scope"
        );
    } else {
        panic!("Expected BinaryOp constraint");
    }
}

#[test]
fn test_phi_generates_subtype_per_branch() {
    let source = r#"
        contract Test {
            entrypoint foo(x: int) {
                var y = 0;
                if (x > 0) {
                    y = 1;
                } else {
                    y = 2;
                }
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    // Phi node should produce 2 Subtype constraints (one per branch)
    // plus the Copy subtypes and comparison constraints.
    // At minimum:
    // - Copy(0 -> y_0): 1 Subtype
    // - Copy(1 -> y_1): 1 Subtype
    // - Copy(2 -> y_2): 1 Subtype
    // - Phi(y_1, y_2 -> y_N): 2 Subtypes
    let subtype_count = count_constraints(foo, is_subtype);
    assert!(
        subtype_count >= 4,
        "Should have at least 4 Subtype constraints \
         (copies + phi joins), got {}",
        subtype_count
    );
}

#[test]
fn test_return_generates_subtype_to_declared_type() {
    let source = r#"
        contract Test {
            internal function positive() -> {v: int | v > 0} {
                return 5;
            }

            entrypoint caller() {
                positive();
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let positive = &constraints["positive"];

    let subtypes: Vec<&Constraint> = positive.iter().filter(|c| is_subtype(c)).collect();

    // Return should generate a Subtype constraint
    // where sup is the declared return type {int | v > 0}
    let return_subtype = subtypes.iter().find(|c| {
        if let Constraint::Subtype { sup, .. } = c {
            if let Some(ref_pred) = sup.refinement() {
                matches!(ref_pred, Predicate::BinRel { op: RelOp::Gt, .. })
            } else {
                false
            }
        } else {
            false
        }
    });

    assert!(
        return_subtype.is_some(),
        "Return should generate Subtype(actual <: {{int | v > 0}}). \
         Available subtypes: {:?}",
        subtypes
            .iter()
            .map(|c| format!("{}", c))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_storage_load_generates_subtype() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint foo() {
                var x = balance;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    let subtype_count = count_constraints(foo, is_subtype);

    // StorageLoad produces at least 1 Subtype (storage <: dest)
    // Plus Copy produces another (dest <: x_0)
    assert!(
        subtype_count >= 2,
        "StorageLoad + Copy should produce at least 2 Subtype constraints, got {}",
        subtype_count
    );
}

#[test]
fn test_storage_store_generates_subtype() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint set_balance() {
                balance = 100;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let set_balance = &constraints["set_balance"];

    let subtype_count = count_constraints(set_balance, is_subtype);

    // StorageStore should generate Subtype(100 <: {int | balance >= 0})
    assert!(
        subtype_count >= 1,
        "StorageStore should generate at least 1 Subtype constraint for the write, got {}",
        subtype_count
    );
}

#[test]
fn test_fold_generates_fold_constraint() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint deposit(amount: {amount: int | amount >= 0}) {
                balance = balance + amount;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let deposit = &constraints["deposit"];

    let fold_count = count_constraints(deposit, is_fold);
    assert!(
        fold_count >= 1,
        "Storage mutation should generate at least 1 Fold constraint, got {}",
        fold_count
    );

    // Inspect the Fold constraint
    let fold = deposit.iter().find(|c| is_fold(c)).unwrap();
    if let Constraint::Fold { refinement, .. } = fold {
        // The refinement should be the declared storage invariant (balance >= 0)
        assert!(
            matches!(refinement, Predicate::BinRel { op: RelOp::Geq, .. }),
            "Fold refinement should be the declared storage invariant (>= 0), got {:?}",
            refinement
        );
    } else {
        panic!("Expected Fold constraint");
    }
}

#[test]
fn test_function_call_generates_requires_and_param_subtype() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            internal function helper(n: {n: int | n > 0}) requires(balance >= n) {
                var x = n;
            }

            entrypoint foo() {
                helper(5);
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    // Should have at least 1 Requires constraint
    let requires_count = count_constraints(foo, is_requires);
    assert!(
        requires_count >= 1,
        "Call to function with requires should generate at least 1 Requires constraint, got {}",
        requires_count
    );

    // Should have at least 1 Subtype for argument (5 <: {int | n > 0})
    let subtype_count = count_constraints(foo, is_subtype);
    assert!(
        subtype_count >= 1,
        "Call should generate Subtype constraints for arguments, got {}",
        subtype_count
    );

    // Inspect the Requires constraint
    let req = foo.iter().find(|c| is_requires(c)).unwrap();
    if let Constraint::Requires { condition, .. } = req {
        // The condition should be the requires clause with formals substituted
        // requires(balance >= n) with n -> 5 becomes balance >= 5
        assert!(
            matches!(condition, Predicate::BinRel { op: RelOp::Geq, .. }),
            "Requires condition should be a >= comparison (balance >= n substituted), got {:?}",
            condition
        );
    }
}

#[test]
fn test_branch_adds_path_assumptions() {
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

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    // Constraints inside branches should have path assumptions in context.
    // The then-branch constraints should have the condition (or its
    // equivalent biconditional) as an assumption.
    let subtypes: Vec<&Constraint> = foo.iter().filter(|c| is_subtype(c)).collect();

    // At least some constraints should have non-empty assumptions
    // (from the branch condition being pushed into the context)
    let has_assumptions = subtypes
        .iter()
        .any(|c| !c.context().assumptions().is_empty());

    assert!(
        has_assumptions,
        "Branch constraints should have path assumptions in their TypeContext. \
         Constraints: {:?}",
        subtypes
            .iter()
            .map(|c| {
                format!(
                    "{} (assumptions: {})",
                    c,
                    c.context().assumptions().len()
                )
            })
            .collect::<Vec<_>>()
    );

    // Furthermore, find a constraint that has at least 2 assumptions
    // (requires parameter assumption + branch condition)
    let max_assumptions = subtypes
        .iter()
        .map(|c| c.context().assumptions().len())
        .max()
        .unwrap_or(0);

    assert!(
        max_assumptions >= 1,
        "Should find constraints with at least 1 path assumption (branch condition), \
         max found: {}",
        max_assumptions
    );
}

#[test]
fn test_loop_generates_invariant_constraints() {
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

    let constraints = run_constraint_generation(source).unwrap();
    let countdown = &constraints["countdown"];

    // Should have LoopInvariantEntry
    let entry_count = count_constraints(countdown, is_loop_invariant_entry);
    assert!(
        entry_count >= 1,
        "Loop should generate at least 1 LoopInvariantEntry constraint, got {}",
        entry_count
    );

    // Should have LoopInvariantPreservation
    let preservation_count = count_constraints(countdown, is_loop_invariant_preservation);
    assert!(
        preservation_count >= 1,
        "Loop should generate at least 1 LoopInvariantPreservation constraint, got {}",
        preservation_count
    );

    // Should have LoopVariantNonNegative
    let nonneg_count = count_constraints(countdown, is_loop_variant_nonneg);
    assert!(
        nonneg_count >= 1,
        "Loop should generate at least 1 LoopVariantNonNegative constraint, got {}",
        nonneg_count
    );

    // Should have LoopVariantDecreases
    let decreases_count = count_constraints(countdown, is_loop_variant_decreases);
    assert!(
        decreases_count >= 1,
        "Loop should generate at least 1 LoopVariantDecreases constraint, got {}",
        decreases_count
    );

    // Inspect the LoopInvariantEntry constraint
    let entry = countdown
        .iter()
        .find(|c| is_loop_invariant_entry(c))
        .unwrap();
    if let Constraint::LoopInvariantEntry { invariant, .. } = entry {
        // Invariant should be i >= 0 (with SSA variable substitution)
        assert!(
            matches!(invariant, Predicate::BinRel { op: RelOp::Geq, .. }),
            "Loop invariant should be >= comparison (i >= 0), got {:?}",
            invariant
        );
    }
}

#[test]
fn test_annotated_variable_generates_contract_subtype() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
            }
        }
    "#;

    let constraints = run_constraint_generation(source).unwrap();
    let foo = &constraints["foo"];

    let subtypes: Vec<&Constraint> = foo.iter().filter(|c| is_subtype(c)).collect();

    // Should have at least 2 Subtype constraints for the annotated variable:
    // 1. source <: dest (normal Copy flow)
    // 2. source <: user_contract (annotation verification)
    assert!(
        subtypes.len() >= 2,
        "Annotated variable should generate at least 2 Subtype constraints \
         (normal flow + annotation check), got {}",
        subtypes.len()
    );

    // Find the Subtype where sup has the user annotation (x > 0)
    let annotation_check = subtypes.iter().find(|c| {
        if let Constraint::Subtype { sup, .. } = c {
            if let Some(ref_pred) = sup.refinement() {
                matches!(ref_pred, Predicate::BinRel { op: RelOp::Gt, .. })
            } else {
                false
            }
        } else {
            false
        }
    });

    assert!(
        annotation_check.is_some(),
        "Should find a Subtype constraint where sup carries the user annotation (x > 0). \
         Available: {:?}",
        subtypes
            .iter()
            .map(|c| format!("{}", c))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_ensures_at_return() {
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

    let constraints = run_constraint_generation(source).unwrap();
    let deposit = &constraints["deposit"];

    // Should have at least 1 Ensures constraint from the return
    let ensures_count = count_constraints(deposit, is_ensures);
    assert!(
        ensures_count >= 1,
        "Function with ensures clause should generate Ensures constraint at return, got {}",
        ensures_count
    );

    // Inspect the Ensures constraint
    let ensures = deposit.iter().find(|c| is_ensures(c)).unwrap();
    if let Constraint::Ensures { condition, .. } = ensures {
        // The condition should be balance >= 0
        assert!(
            matches!(condition, Predicate::BinRel { op: RelOp::Geq, .. }),
            "Ensures condition should be >= comparison (balance >= 0), got {:?}",
            condition
        );
    }
}
