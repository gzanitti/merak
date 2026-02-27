// Tests for specific instruction types and selection
// Includes: arithmetic, comparisons, unary ops, function calls, state transitions, assertions

mod common;

use merak_ast::expression::{BinaryOperator, UnaryOperator};
use merak_ir::ssa_ir::{CallTarget, Operand, SsaInstruction, Terminator};

use common::{
    build_ssa_from_source, count_instructions_of_type, get_function_cfg, get_single_function_cfg,
};

#[test]
fn test_arithmetic_operations_generate_binary_ops() {
    let source = r#"
        contract Test {
            entrypoint arithmetic(a: int, b: int) -> int {
                var sum: int = a + b;
                var diff: int = a - b;
                var prod: int = a * b;
                var quot: int = a / b;
                return sum;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 4 BinaryOp instructions for +, -, *, /
    let binops = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::BinaryOp { .. })
    });

    assert_eq!(
        binops, 4,
        "Should have 4 BinaryOp instructions for arithmetic operations"
    );

    // Verify each operation type
    let mut found_ops = vec![];

    for instr in &entry_block.instructions {
        if let SsaInstruction::BinaryOp {
            op,
            left,
            right,
            dest,
            ..
        } = instr
        {
            found_ops.push(op.clone());

            // Operands should be registers (parameters or previous results)
            match (left, right) {
                (Operand::Location(_), Operand::Location(_)) => {
                    // Both registers - valid
                }
                _ => {
                    // At least one should be a register
                    assert!(
                        matches!(left, Operand::Location(_))
                            || matches!(right, Operand::Location(_)),
                        "At least one operand should be a register"
                    );
                }
            }

            // Dest should be a valid register
            assert!(dest.version >= 0, "Dest should have valid version");
        }
    }

    // Should have found all 4 operations
    assert!(found_ops.contains(&BinaryOperator::Add), "Should have Add");
    assert!(
        found_ops.contains(&BinaryOperator::Subtract),
        "Should have Subtract"
    );
    assert!(
        found_ops.contains(&BinaryOperator::Multiply),
        "Should have Multiply"
    );
    assert!(
        found_ops.contains(&BinaryOperator::Divide),
        "Should have Divide"
    );
}

#[test]
fn test_comparison_operations() {
    let source = r#"
        contract Test {
            entrypoint compare(x: int, y: int) -> bool {
                var gt: bool = x > y;
                var lt: bool = x < y;
                var eq: bool = x == y;
                return eq;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 3 BinaryOp instructions for comparisons
    let binops = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::BinaryOp { .. })
    });

    assert_eq!(
        binops, 3,
        "Should have 3 BinaryOp instructions for comparisons"
    );

    // Verify comparison operators
    let mut found_ops = vec![];

    for instr in &entry_block.instructions {
        if let SsaInstruction::BinaryOp { op, .. } = instr {
            found_ops.push(op.clone());
        }
    }

    assert!(
        found_ops.contains(&BinaryOperator::Greater),
        "Should have Greater"
    );
    assert!(
        found_ops.contains(&BinaryOperator::Less),
        "Should have Less"
    );
    assert!(
        found_ops.contains(&BinaryOperator::Equal),
        "Should have Equal"
    );
}

#[test]
fn test_unary_operations() {
    let source = r#"
        contract Test {
            entrypoint unary(x: int, flag: bool) -> int {
                var neg: int = -x;
                var notflag: bool = !flag;
                return neg;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 2 UnaryOp instructions (negate, not)
    let unary_ops =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::UnaryOp { .. }));

    assert_eq!(unary_ops, 2, "Should have 2 UnaryOp instructions");

    // Verify operators
    let mut found_ops = vec![];

    for instr in &entry_block.instructions {
        if let SsaInstruction::UnaryOp {
            op, operand, dest, ..
        } = instr
        {
            found_ops.push(op.clone());

            // Operand should be a register (parameter)
            match operand {
                Operand::Location(reg) => {
                    assert!(reg.version >= 0, "Operand should be valid register");
                }
                _ => panic!("Operand should be a register"),
            }

            // Dest should be valid
            assert!(dest.version >= 0, "Dest should have valid version");
        }
    }

    assert!(
        found_ops.contains(&UnaryOperator::Negate),
        "Should have Negate"
    );
    assert!(found_ops.contains(&UnaryOperator::Not), "Should have Not");
}

#[test]
fn test_complex_expression_uses_temps() {
    let source = r#"
        contract Test {
            entrypoint complex(a: int, b: int, c: int) -> int {
                return (a + b) * (c - a);
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 3 BinaryOp instructions:
    // 1. a + b -> temp1
    // 2. c - a -> temp2
    // 3. temp1 * temp2 -> temp3
    let binops = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::BinaryOp { .. })
    });

    assert_eq!(binops, 3, "Complex expression should generate 3 BinaryOps");

    // Verify that intermediate results use temp registers
    let mut temp_count = 0;

    for instr in &entry_block.instructions {
        if let SsaInstruction::BinaryOp { dest, .. } = instr {
            // Dest registers should be temps or have versions
            if dest.symbol.is_temp() {
                temp_count += 1;
            }
        }
    }

    assert!(
        temp_count > 0,
        "Should use temp registers for intermediate results"
    );

    // Return should use the last computed value
    match &entry_block.terminator {
        Terminator::Return {
            value: Some(Operand::Location(_)),
            ..
        } => {
            // Correct - returns a register
        }
        other => panic!("Expected Return with register operand, got {:?}", other),
    }
}

// TODO: Enable this test once function call logic (internal/external) is implemented
#[test]
#[ignore]
fn test_function_call_internal() {
    let source = r#"
        contract Test {
            entrypoint helper(x: int) -> int {
                return x * 2;
            }
            entrypoint main(y: int) -> int {
                return helper(y);
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_function_cfg(&ssa_program, "Test", "main");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 Call instruction
    let calls =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Call { .. }));

    assert_eq!(calls, 1, "Should have 1 Call instruction");

    // Verify call details
    match &entry_block.instructions[0] {
        SsaInstruction::Call {
            dest, target, args, ..
        } => {
            // Dest should be Some (function returns int)
            assert!(
                dest.is_some(),
                "Call to function returning int should have dest"
            );

            // Target should be Internal
            match target {
                CallTarget::Internal(symbol_id) => {
                    // Symbol should be valid
                    assert!(!symbol_id.is_temp(), "Target should be real function");
                }
                other => panic!("Expected Internal call target, got {:?}", other),
            }

            // Should have 1 argument (y)
            assert_eq!(args.len(), 1, "Should have 1 argument");

            match &args[0] {
                Operand::Location(reg) => {
                    assert!(reg.version >= 0, "Argument should be valid register");
                }
                _ => panic!("Argument should be a register"),
            }
        }
        other => panic!("Expected Call instruction, got {:?}", other),
    }
}

// TODO: Enable this test once function call logic (internal/external) is implemented
#[test]
#[ignore]
fn test_function_call_void() {
    let source = r#"
        contract Test {
            entrypoint log_value(x: int) {
                // empty body for simplicity
            }
            entrypoint main() {
                log_value(42);
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_function_cfg(&ssa_program, "Test", "main");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 Call instruction
    let calls =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Call { .. }));

    assert_eq!(calls, 1, "Should have 1 Call instruction");

    // Verify call details
    match &entry_block.instructions[0] {
        SsaInstruction::Call { dest, args, .. } => {
            // Dest should be None (void function)
            assert!(
                dest.is_none(),
                "Call to void function should have dest = None"
            );

            // Should have 1 argument
            assert_eq!(args.len(), 1, "Should have 1 argument");

            // Argument should be constant 42
            match &args[0] {
                Operand::Constant(merak_ir::ssa_ir::Constant::Int(42)) => {
                    // Correct
                }
                other => panic!("Expected Constant(42), got {:?}", other),
            }
        }
        other => panic!("Expected Call instruction, got {:?}", other),
    }
}
