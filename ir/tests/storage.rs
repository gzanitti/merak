// Tests for storage vs local variable operations (CRITICAL)
// These tests verify that storage operations use StorageLoad/StorageStore
// while local variables use Copy instructions with SSA versioning

mod common;

use merak_ir::ssa_ir::{Operand, SsaInstruction, Terminator};

use common::{
    build_ssa_from_source, count_instructions_of_type, get_phi_nodes, get_single_function_cfg,
};

#[test]
fn test_storage_variable_read_generates_load() {
    let source = r#"
        contract Test[Active] {
            state var balance: int = 0;
        }
        Test@Active(any) {
            entrypoint get_balance() -> int {
                return balance;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 StorageLoad instruction
    let storage_loads = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });

    assert_eq!(
        storage_loads, 1,
        "Reading a state var should generate exactly 1 StorageLoad"
    );

    // Verify the StorageLoad instruction details
    match &entry_block.instructions[0] {
        SsaInstruction::StorageLoad { dest, var, .. } => {
            // StorageLoad should store result in a temp register
            assert!(
                dest.symbol.is_temp() || dest.version == 0,
                "StorageLoad dest should be a temp register"
            );

            // var should be the balance symbol ID
            // (we can't verify the exact ID without symbol table lookups, but it should exist)
            assert!(!var.is_temp(), "var should be a real symbol, not temp");
        }
        other => panic!("Expected StorageLoad as first instruction, got {:?}", other),
    }

    // Return should use the temp register from StorageLoad
    match &entry_block.terminator {
        Terminator::Return {
            value: Some(Operand::Register(reg)),
            ..
        } => {
            // The register should be from the StorageLoad
            assert!(reg.version == 0, "Return should use the loaded register");
        }
        other => panic!("Expected Return with register operand, got {:?}", other),
    }
}

#[test]
fn test_storage_variable_write_generates_store() {
    let source = r#"
        contract Test[Active] {
            state var balance: int = 0;
        }
        Test@Active(any) {
            entrypoint set_balance(amount: int) {
                balance = amount;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 StorageStore instruction
    let storage_stores = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(
        storage_stores, 1,
        "Writing a state var should generate exactly 1 StorageStore"
    );

    // Verify the StorageStore instruction details
    match &entry_block.instructions[0] {
        SsaInstruction::StorageStore { var, value, .. } => {
            // var should be the balance symbol
            assert!(
                !var.is_temp(),
                "var should be a real symbol, not temp"
            );

            // value should be a register (the parameter 'amount')
            match value {
                Operand::Register(reg) => {
                    // The parameter should be version 0
                    assert_eq!(reg.version, 0, "Parameter should be version 0");
                }
                other => panic!("Expected Register operand, got {:?}", other),
            }
        }
        other => panic!("Expected StorageStore, got {:?}", other),
    }
}

#[test]
fn test_local_variable_uses_copy_not_storage() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo() -> int {
                var x: int = 10;
                x = 20;
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have NO StorageLoad or StorageStore
    let storage_loads = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });
    let storage_stores = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(
        storage_loads, 0,
        "Local variables should not generate StorageLoad"
    );
    assert_eq!(
        storage_stores, 0,
        "Local variables should not generate StorageStore"
    );

    // Should only have Copy instructions
    let copies = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::Copy { .. })
    });

    assert_eq!(
        copies, 2,
        "Should have 2 Copy instructions for local variable (initial + reassignment)"
    );

    // Verify SSA versions are different
    let mut versions = vec![];
    for instr in &entry_block.instructions {
        if let SsaInstruction::Copy { dest, .. } = instr {
            versions.push(dest.version);
        }
    }

    // After SSA transformation, versions should be: x_0 (initial), x_1 (reassignment)
    // Note: The exact versions depend on SSA renaming, but they should be distinct
    assert_eq!(versions.len(), 2, "Should have 2 Copy destinations");
}

#[test]
fn test_storage_read_after_write() {
    let source = r#"
        contract Test[Active] {
            state var balance: int = 0;
        }
        Test@Active(any) {
            entrypoint increment() {
                balance = balance + 1;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have: StorageLoad, BinaryOp, StorageStore (in that order)
    assert!(
        entry_block.instructions.len() >= 3,
        "Should have at least 3 instructions (Load, BinaryOp, Store)"
    );

    // First instruction should be StorageLoad
    match &entry_block.instructions[0] {
        SsaInstruction::StorageLoad { dest, .. } => {
            // Save the dest register for later verification
            let load_dest = dest;

            // Next should be BinaryOp using the loaded value
            match &entry_block.instructions[1] {
                SsaInstruction::BinaryOp {
                    left, right, dest, ..
                } => {
                    use merak_ast::expression::BinaryOperator;

                    // One operand should be the loaded register
                    let uses_loaded = match left {
                        Operand::Register(reg) => reg == load_dest,
                        _ => false,
                    };

                    // The other operand should be constant 1
                    let has_one = match right {
                        Operand::Constant(merak_ir::ssa_ir::Constant::Int(1)) => true,
                        _ => false,
                    };

                    assert!(
                        uses_loaded,
                        "BinaryOp should use the loaded register"
                    );
                    assert!(has_one, "BinaryOp should add 1");

                    let binop_dest = dest;

                    // Last should be StorageStore using the BinaryOp result
                    match &entry_block.instructions[2] {
                        SsaInstruction::StorageStore { value, .. } => {
                            match value {
                                Operand::Register(reg) => {
                                    assert_eq!(
                                        reg, binop_dest,
                                        "StorageStore should use BinaryOp result"
                                    );
                                }
                                other => panic!("Expected Register operand, got {:?}", other),
                            }
                        }
                        other => panic!("Expected StorageStore as 3rd instruction, got {:?}", other),
                    }
                }
                other => panic!("Expected BinaryOp as 2nd instruction, got {:?}", other),
            }
        }
        other => panic!("Expected StorageLoad as first instruction, got {:?}", other),
    }
}

#[test]
fn test_multiple_storage_variables() {
    let source = r#"
        contract Test[Active] {
            state var x: int = 0;
            state var y: int = 0;
        }
        Test@Active(any) {
            entrypoint swap() {
                var temp: int = x;
                x = y;
                y = temp;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 2 StorageLoads (for x and y)
    let loads = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });

    assert_eq!(
        loads, 2,
        "Should have 2 StorageLoads (one for x, one for y)"
    );

    // Should have 2 StorageStores (to x and y)
    let stores = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(
        stores, 2,
        "Should have 2 StorageStores (one to x, one to y)"
    );

    // Should have 1 Copy for local variable 'temp'
    let copies = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::Copy { .. })
    });

    assert_eq!(
        copies, 1,
        "Should have 1 Copy for local variable temp"
    );

    // Total instructions: 2 loads + 1 copy + 2 stores = 5
    assert_eq!(
        entry_block.instructions.len(),
        5,
        "Should have 5 total instructions"
    );
}

#[test]
fn test_storage_in_loop() {
    let source = r#"
        contract Test[Active] {
            state var counter: int = 0;
        }
        Test@Active(any) {
            entrypoint increment_n(n: int) {
                var i: int = 0;
                while (i < n)
                    with @invariant(i >= 0) @variant(n - i)
                {
                    counter = counter + 1;
                    i = i + 1;
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Find the loop body block (should contain the counter increment)
    let body_block = cfg
        .blocks
        .values()
        .find(|block| {
            // Loop body should have StorageLoad and StorageStore
            let has_load = block
                .instructions
                .iter()
                .any(|i| matches!(i, SsaInstruction::StorageLoad { .. }));
            let has_store = block
                .instructions
                .iter()
                .any(|i| matches!(i, SsaInstruction::StorageStore { .. }));
            has_load && has_store
        })
        .expect("Loop body block not found");

    // Loop body should contain:
    // 1. StorageLoad for counter
    // 2. BinaryOp for counter + 1
    // 3. StorageStore for counter
    // 4. BinaryOp for i + 1 (or Copy for i assignment)
    let loads = count_instructions_of_type(body_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });
    let stores = count_instructions_of_type(body_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });
    let binops = count_instructions_of_type(body_block, |i| {
        matches!(i, SsaInstruction::BinaryOp { .. })
    });

    assert_eq!(
        loads, 1,
        "Loop body should have 1 StorageLoad for counter"
    );
    assert_eq!(
        stores, 1,
        "Loop body should have 1 StorageStore for counter"
    );
    assert!(
        binops >= 2,
        "Loop body should have at least 2 BinaryOps (counter+1, i+1)"
    );

    // Find the loop header (should have phi nodes for local i, but NOT for counter)
    let header_block = cfg
        .blocks
        .values()
        .find(|block| {
            // Header should have phi nodes and a Branch terminator
            let has_phis = get_phi_nodes(block).len() > 0;
            let has_branch = matches!(block.terminator, Terminator::Branch { .. });
            has_phis && has_branch
        })
        .expect("Loop header block not found");

    let phi_nodes = get_phi_nodes(header_block);

    // Should have phi nodes for local variable i, but NOT for storage variable counter
    assert!(
        phi_nodes.len() > 0,
        "Loop header should have phi nodes for local variable i"
    );

    // Counter should NOT have phi nodes (it's storage, not SSA)
    // We can't easily verify this without symbol table, but we can check that
    // there are no StorageLoad/Store in the header (only in body)
    let header_loads = count_instructions_of_type(header_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });
    let header_stores = count_instructions_of_type(header_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(
        header_loads, 0,
        "Loop header should not have StorageLoad"
    );
    assert_eq!(
        header_stores, 0,
        "Loop header should not have StorageStore"
    );
}

#[test]
fn test_parameter_vs_storage_distinction() {
    let source = r#"
        contract Test[Active] {
            state var value: int = 0;
        }
        Test@Active(any) {
            entrypoint set_value(new_value: int) {
                value = new_value;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should have 1 parameter (new_value)
    assert_eq!(cfg.parameters.len(), 1, "Should have 1 parameter");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 StorageStore (to value)
    let stores = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(stores, 1, "Should have 1 StorageStore");

    // Should have NO StorageLoad for the parameter
    let loads = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });

    assert_eq!(
        loads, 0,
        "Parameter should not generate StorageLoad"
    );

    // Verify StorageStore uses parameter register directly
    match &entry_block.instructions[0] {
        SsaInstruction::StorageStore { value, .. } => {
            match value {
                Operand::Register(reg) => {
                    // Parameter should be version 0
                    assert_eq!(
                        reg.version, 0,
                        "Parameter should be version 0"
                    );
                }
                other => panic!("Expected Register operand, got {:?}", other),
            }
        }
        other => panic!("Expected StorageStore, got {:?}", other),
    }
}

#[test]
fn test_state_const_read_only() {
    let source = r#"
        contract Test[Active] {
            state const MAX: int = 100;
        }
        Test@Active(any) {
            entrypoint get_max() -> int {
                return MAX;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 1 StorageLoad (reading the const)
    let loads = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageLoad { .. })
    });

    assert_eq!(loads, 1, "Should have 1 StorageLoad for const");

    // Should have NO StorageStore (const is read-only)
    let stores = count_instructions_of_type(entry_block, |i| {
        matches!(i, SsaInstruction::StorageStore { .. })
    });

    assert_eq!(
        stores, 0,
        "State const should not generate StorageStore"
    );

    // Verify the load
    match &entry_block.instructions[0] {
        SsaInstruction::StorageLoad { .. } => {
            // This is correct
        }
        other => panic!("Expected StorageLoad, got {:?}", other),
    }
}
