// Tests for basic CFG structure validation

mod common;

use merak_ir::ssa_ir::{SsaInstruction, Terminator};

use common::{assert_predecessors, assert_successors, build_ssa_from_source, get_single_function_cfg};

#[test]
fn test_empty_function_has_entry_block() {
    let source = r#"
        contract Test {
            entrypoint empty() {}
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Should have exactly 1 block (entry)
    assert_eq!(cfg.blocks.len(), 1, "Empty function should have exactly 1 block");

    // Entry should be valid
    assert!(cfg.blocks.contains_key(&cfg.entry), "Entry block should exist");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Entry block should have 0 instructions (empty function)
    assert_eq!(
        entry_block.instructions.len(),
        0,
        "Empty function should have 0 instructions"
    );

    // Terminator should be Return with None
    match &entry_block.terminator {
        Terminator::Return { value: None, .. } => {},
        other => panic!("Expected Return terminator with None value, got {:?}", other),
    }
}

#[test]
fn test_entry_block_is_set_correctly() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: int = 5;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Entry should point to a valid block
    assert!(
        cfg.blocks.contains_key(&cfg.entry),
        "Entry block should exist in blocks map"
    );

    let entry_block = &cfg.blocks[&cfg.entry];

    // Entry block should have 0 predecessors (no blocks jump to it)
    assert_predecessors(entry_block, 0);

    // Entry block should be reachable (it's the starting point)
    assert_eq!(
        entry_block.id, cfg.entry,
        "Entry block ID should match cfg.entry"
    );
}

#[test]
fn test_single_statement_single_block() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: int = 42;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Should have exactly 1 block
    assert_eq!(cfg.blocks.len(), 1, "Single statement should generate 1 block");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 Copy instruction for the variable declaration
    assert_eq!(
        entry_block.instructions.len(),
        1,
        "Should have 1 instruction"
    );

    match &entry_block.instructions[0] {
        SsaInstruction::Copy { .. } => {},
        other => panic!("Expected Copy instruction, got {:?}", other),
    }

    // Should have Return terminator
    match &entry_block.terminator {
        Terminator::Return { .. } => {},
        other => panic!("Expected Return terminator, got {:?}", other),
    }
}

#[test]
fn test_sequential_statements_same_block() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: int = 1;
                var y: int = 2;
                var z: int = 3;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Should have exactly 1 block for sequential statements
    assert_eq!(
        cfg.blocks.len(),
        1,
        "Sequential statements should be in the same block"
    );

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 3 Copy instructions
    assert_eq!(
        entry_block.instructions.len(),
        3,
        "Should have 3 instructions for 3 variable declarations"
    );

    // All should be Copy instructions
    for (i, instr) in entry_block.instructions.iter().enumerate() {
        match instr {
            SsaInstruction::Copy { .. } => {},
            other => panic!("Instruction {} should be Copy, got {:?}", i, other),
        }
    }

    // No branches - should have Return terminator
    match &entry_block.terminator {
        Terminator::Return { .. } => {},
        other => panic!("Expected Return terminator, got {:?}", other),
    }
}

#[test]
fn test_return_statement_terminates_block() {
    let source = r#"
        contract Test {
            entrypoint get_value() -> int {
                return 42;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Terminator should be Return with Some(Constant(42))
    match &entry_block.terminator {
        Terminator::Return { value: Some(operand), .. } => {
            // The operand should be the constant 42
            use merak_ir::ssa_ir::{Constant, Operand};
            match operand {
                Operand::Constant(Constant::Int(val)) => {
                    assert_eq!(*val, 42, "Return value should be 42");
                }
                other => panic!("Expected Constant(Int(42)), got {:?}", other),
            }
        }
        other => panic!("Expected Return terminator with value, got {:?}", other),
    }

    // Should have 0 instructions (return is terminator, not instruction)
    assert_eq!(
        entry_block.instructions.len(),
        0,
        "Return should be terminator, not instruction"
    );
}

#[test]
fn test_function_parameters_registered() {
    let source = r#"
        contract Test {
            entrypoint add(a: int, b: int) -> int {
                return a + b;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Should have 2 parameters registered
    assert_eq!(
        cfg.parameters.len(),
        2,
        "Function should have 2 parameters"
    );

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 BinaryOp instruction for a + b
    assert_eq!(
        entry_block.instructions.len(),
        1,
        "Should have 1 BinaryOp instruction"
    );

    match &entry_block.instructions[0] {
        SsaInstruction::BinaryOp { op, left, right, .. } => {
            use merak_ast::expression::BinaryOperator;
            assert!(
                matches!(op, BinaryOperator::Add),
                "Operation should be Add"
            );

            // Both operands should be registers (parameters)
            use merak_ir::ssa_ir::Operand;
            assert!(
                matches!(left, Operand::Location(_)),
                "Left operand should be a register"
            );
            assert!(
                matches!(right, Operand::Location(_)),
                "Right operand should be a register"
            );
        }
        other => panic!("Expected BinaryOp, got {:?}", other),
    }
}

#[test]
fn test_block_connectivity_simple_sequence() {
    let source = r#"
        contract Test {
            entrypoint foo() {
                var x: int = 1;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Entry block should have 0 predecessors (nothing jumps to it)
    assert_predecessors(entry_block, 0);

    // Entry block should have 0 successors (no branches, just returns)
    assert_successors(entry_block, 0);

    // Predecessors and successors should be empty
    assert!(
        entry_block.predecessors.is_empty(),
        "Predecessors should be empty"
    );
    assert!(
        entry_block.successors.is_empty(),
        "Successors should be empty"
    );
}

#[test]
fn test_multiple_basic_blocks_are_distinct() {
    let source = r#"
        contract Test {
            entrypoint foo(cond: bool) {
                if (cond) {
                    var x: int = 1;
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test");

    // Should have multiple blocks (if creates at least 3: entry with Branch, then, else/exit)
    // NOTE: if statements no longer create a separate header block - condition is evaluated
    // in the previous block
    assert!(
        cfg.blocks.len() >= 3,
        "If statement should create at least 3 blocks"
    );

    // All BlockIds should be unique
    let block_ids: Vec<_> = cfg.blocks.keys().collect();
    let unique_count = block_ids.iter().collect::<std::collections::HashSet<_>>().len();

    assert_eq!(
        block_ids.len(),
        unique_count,
        "All block IDs should be unique"
    );

    // Each block ID should map to exactly one block
    for &block_id in &block_ids {
        assert!(
            cfg.blocks.contains_key(block_id),
            "Block ID {} should exist in blocks map",
            block_id
        );
    }
}
