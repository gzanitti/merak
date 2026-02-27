use crate::common::*;
use merak_analyzer::refinements::templates::Template;
use merak_ast::predicate::{ArithOp, Predicate, RefinementExpr, RelOp, UnaryOp};
use merak_ast::types::BaseType;
use std::collections::HashSet;

/// Helper: find the first temp register with a Concrete template
fn find_concrete_temp<'a>(
    bindings: &'a std::collections::HashMap<String, Template>,
) -> Option<(&'a String, &'a Template)> {
    bindings
        .iter()
        .find(|(name, tmpl)| name.contains("__temp_") && tmpl.is_concrete())
}

/// Helper: collect all temp registers with Concrete templates
fn find_all_concrete_temps<'a>(
    bindings: &'a std::collections::HashMap<String, Template>,
) -> Vec<(&'a String, &'a Template)> {
    bindings
        .iter()
        .filter(|(name, tmpl)| name.contains("__temp_") && tmpl.is_concrete())
        .collect()
}

// Function parameters receive Concrete templates
#[test]
fn test_parameter_templates() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var x = a;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // Parameters are registered as "{name}_0" (SSA version 0)
    let a_tmpl = &foo["a_0"];
    let b_tmpl = &foo["b_0"];

    // Both parameters must be Concrete templates (they have explicit types)
    assert!(
        a_tmpl.is_concrete(),
        "Parameter 'a' should get a Concrete template (explicit type in signature)"
    );
    assert!(
        b_tmpl.is_concrete(),
        "Parameter 'b' should get a Concrete template (explicit type in signature)"
    );

    // Base types must match declared types
    assert_eq!(*a_tmpl.base_type(), BaseType::Int);
    assert_eq!(*b_tmpl.base_type(), BaseType::Int);
}

// Named variables receive Liquid templates
#[test]
fn test_named_variable_constant_assignment() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x = 5;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    let x_tmpl = &foo["x_0"];
    assert!(
        x_tmpl.is_liquid(),
        "Named variable 'x' should get a Liquid template (SSA adaptation of let-rule), got {:?}",
        x_tmpl
    );
    assert_eq!(*x_tmpl.base_type(), BaseType::Int);
}


// Explicit annotations make Named variables Concrete
#[test]
fn test_named_variable_with_annotation_gets_concrete() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: {int | x > 0} = 5;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    let x_tmpl = &foo["x_0"];
    assert!(
        x_tmpl.is_concrete(),
        "Named variable with explicit annotation gets Concrete template \
         (annotation is enforced directly, not deferred to constraint solving)"
    );
    assert_eq!(*x_tmpl.base_type(), BaseType::Int);
}

// Arithmetic operations → Concrete templates
#[test]
fn test_arithmetic_op_template() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var z = a + b;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // Find the temp register holding the addition result
    let (add_name, add_tmpl) = find_concrete_temp(foo)
        .expect("Should find a Concrete temp for the addition");

    assert_eq!(
        *add_tmpl.base_type(),
        BaseType::Int,
        "ty(+) result should be Int"
    );

    // Refinement structure: dest_name = a_0 + b_0
    // This corresponds to ty(+) = x:int → y:int → {ν:int | ν = x + y}
    let refinement = add_tmpl.refinement().unwrap();
    match refinement {
        Predicate::BinRel {
            op: RelOp::Eq,
            lhs,
            rhs,
            ..
        } => {
            // LHS: the binder variable (replaced to dest register name)
            assert!(
                matches!(lhs, RefinementExpr::Var(name, ..) if name == add_name),
                "LHS should be the dest register '{}', got {:?}",
                add_name, lhs
            );
            // RHS: BinOp(Add, a_0, b_0)
            match rhs {
                RefinementExpr::BinOp {
                    op: ArithOp::Add,
                    lhs: inner_lhs,
                    rhs: inner_rhs,
                    ..
                } => {
                    assert!(
                        matches!(inner_lhs.as_ref(), RefinementExpr::Var(name, ..) if name == "a_0"),
                        "Left operand should be a_0, got {:?}", inner_lhs
                    );
                    assert!(
                        matches!(inner_rhs.as_ref(), RefinementExpr::Var(name, ..) if name == "b_0"),
                        "Right operand should be b_0, got {:?}", inner_rhs
                    );
                }
                _ => panic!("RHS should be BinOp(Add, a_0, b_0), got {:?}", rhs),
            }
        }
        _ => panic!(
            "Arithmetic template refinement should be BinRel(Eq, v, a+b), got {:?}",
            refinement
        ),
    }

    // The named destination 'z' should be Liquid
    assert!(foo["z_0"].is_liquid(), "Named var 'z' should be Liquid");
}

// Comparison operations → Concrete Bool templates
#[test]
fn test_comparison_op_template() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var c = a > b;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    let (_, cmp_tmpl) = find_concrete_temp(foo)
        .expect("Should find a Concrete temp for the comparison");

    assert_eq!(
        *cmp_tmpl.base_type(),
        BaseType::Bool,
        "ty(>) result should be Bool"
    );

    // Refinement: a_0 > b_0 (corresponds to ty(>) applied to a_0, b_0)
    let refinement = cmp_tmpl.refinement().unwrap();
    match refinement {
        Predicate::BinRel {
            op: RelOp::Gt,
            lhs,
            rhs,
            ..
        } => {
            assert!(
                matches!(lhs, RefinementExpr::Var(name, ..) if name == "a_0"),
                "LHS should be a_0, got {:?}", lhs
            );
            assert!(
                matches!(rhs, RefinementExpr::Var(name, ..) if name == "b_0"),
                "RHS should be b_0, got {:?}", rhs
            );
        }
        _ => panic!(
            "Comparison template should be BinRel(Gt, a_0, b_0), got {:?}",
            refinement
        ),
    }
}

// Unary negation → Concrete template
#[test]
fn test_unary_negation_template() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int) {
                var y = -a;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    let (neg_name, neg_tmpl) = find_concrete_temp(foo)
        .expect("Should find a Concrete temp for the negation");

    assert_eq!(*neg_tmpl.base_type(), BaseType::Int);

    // Refinement: dest_name = -a_0 (corresponds to ty(negate) applied to a_0)
    let refinement = neg_tmpl.refinement().unwrap();
    match refinement {
        Predicate::BinRel {
            op: RelOp::Eq,
            lhs,
            rhs,
            ..
        } => {
            assert!(
                matches!(lhs, RefinementExpr::Var(name, ..) if name == neg_name),
                "LHS should be dest register '{}', got {:?}",
                neg_name, lhs
            );
            match rhs {
                RefinementExpr::UnaryOp {
                    op: UnaryOp::Negate,
                    expr,
                    ..
                } => {
                    assert!(
                        matches!(expr.as_ref(), RefinementExpr::Var(name, ..) if name == "a_0"),
                        "Operand should be a_0, got {:?}", expr
                    );
                }
                _ => panic!("RHS should be UnaryOp(Negate, a_0), got {:?}", rhs),
            }
        }
        _ => panic!(
            "Negation template should be BinRel(Eq, v, -a_0), got {:?}",
            refinement
        ),
    }
}

// Phi nodes (join points) → Liquid templates
#[test]
fn test_phi_node_gets_liquid_template() {
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

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // After the if/else, SSA creates multiple versions of y.
    // y_0 = 0, y_1 = 1 (then), y_2 = 2 (else), and possibly y_3 = phi(y_1, y_2)
    let y_versions: Vec<(&String, &Template)> = foo
        .iter()
        .filter(|(name, _)| name.starts_with("y_"))
        .collect();

    assert!(
        y_versions.len() >= 2,
        "Should have multiple SSA versions of y due to conditional, got: {:?}",
        y_versions.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>()
    );

    // All y versions must be Liquid — both the direct assignments and the
    // phi join. Named variables always get Liquid (SSA adaptation), which
    // captures the paper's Fresh rule for if-then-else join points.
    for (name, tmpl) in &y_versions {
        assert!(
            tmpl.is_liquid(),
            "SSA version '{}' of Named var 'y' should be Liquid (Fresh for join point), got {:?}",
            name, tmpl
        );
    }
}

// Storage load → Concrete with declared refinement
#[test]
fn test_storage_load_template() {
    let source = r#"
        contract Test {
            state var balance: {balance: int | balance >= 0} = 0;

            entrypoint foo() {
                var x = balance;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // StorageLoad produces a Temp register with Concrete template
    let (_, load_tmpl) = find_concrete_temp(foo)
        .expect("Should find a Concrete temp for storage load");

    assert_eq!(*load_tmpl.base_type(), BaseType::Int);

    // The refinement should carry the declared storage refinement (>= 0)
    // This is the "unfolded" invariant from [2]
    let refinement = load_tmpl.refinement().unwrap();
    match refinement {
        Predicate::BinRel {
            op: RelOp::Geq, ..
        } => { /* correct: the declared storage refinement is preserved */ }
        _ => panic!(
            "StorageLoad template should carry the declared storage refinement (>= 0), got {:?}",
            refinement
        ),
    }
}

// All liquid variables κ are globally unique
#[test]
fn test_liquid_variable_uniqueness() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var x = 5;
                var y = a + b;
                var z = x;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // Collect all liquid variable indices
    let liquid_vars: Vec<_> = foo
        .values()
        .filter_map(|tmpl| tmpl.liquid_var())
        .collect();

    assert!(
        !liquid_vars.is_empty(),
        "Should have at least some Liquid templates"
    );

    // Check uniqueness
    let unique: HashSet<_> = liquid_vars.iter().collect();
    assert_eq!(
        liquid_vars.len(),
        unique.len(),
        "All liquid variables should be unique. Found {:?}",
        liquid_vars
    );
}

// Function call return → Concrete templates
#[test]
fn test_function_call_return_template() {
    let source = r#"
        contract Test {
            internal function annotated_return() -> {int | v > 0} {
                return 5;
            }

            internal function plain_return() -> int {
                return 5;
            }

            entrypoint caller() {
                var a = annotated_return();
                var b = plain_return();
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let caller = &bindings["caller"];

    // Find all temp registers from function calls
    let call_temps: Vec<(&String, &Template)> = caller
        .iter()
        .filter(|(name, _)| name.contains("__temp_"))
        .collect();

    assert!(
        call_temps.len() >= 2,
        "Should have at least 2 temp registers from the two function calls, got: {:?}",
        call_temps
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );

    // BOTH calls produce Concrete templates because both functions have explicitly declared return types.
    for (name, tmpl) in &call_temps {
        assert!(
            tmpl.is_concrete(),
            "Function call temp '{}' should be Concrete (application rule: \
             return type is declared in function signature), got {:?}",
            name, tmpl
        );
    }

    // Named destinations should still be Liquid (SSA adaptation of let-rule)
    assert!(
        caller["a_0"].is_liquid(),
        "Named var 'a' should be Liquid"
    );
    assert!(
        caller["b_0"].is_liquid(),
        "Named var 'b' should be Liquid"
    );
}

// End-to-end template chain — multiple operations
#[test]
fn test_multiple_operations_template_chain() {
    let source = r#"
        contract Test {
            entrypoint foo(a: int, b: int) {
                var x = a + b;
                var y = x + 1;
            }
        }
    "#;

    let bindings = run_template_assignment(source).unwrap();
    let foo = &bindings["foo"];

    // Parameters are Concrete (explicit types), named vars are Liquid (no annotations)
    assert!(
        foo["a_0"].is_concrete(),
        "Parameter a should be Concrete (explicit type in signature)"
    );
    assert!(
        foo["b_0"].is_concrete(),
        "Parameter b should be Concrete (explicit type in signature)"
    );
    assert!(
        foo["x_0"].is_liquid(),
        "Named var x should be Liquid (no explicit annotation)"
    );
    assert!(
        foo["y_0"].is_liquid(),
        "Named var y should be Liquid (no explicit annotation)"
    );

    // Should have exactly 2 Concrete temps (for the two additions)
    // Each corresponds to ty(+) applied to its operands
    let concrete_temps = find_all_concrete_temps(foo);
    assert_eq!(
        concrete_temps.len(),
        2,
        "Should have 2 Concrete temps for 2 additions (ty(+) applied twice), found: {:?}",
        concrete_temps
            .iter()
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );

    // Both should be Int with arithmetic equality refinements
    for (_, tmpl) in &concrete_temps {
        assert_eq!(*tmpl.base_type(), BaseType::Int);
        let refinement = tmpl.refinement().unwrap();
        assert!(
            matches!(refinement, Predicate::BinRel { op: RelOp::Eq, .. }),
            "Arithmetic temp should have equality refinement (v = lhs + rhs), got {:?}",
            refinement
        );
    }
}
