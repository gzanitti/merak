
use merak_ir::ssa_ir::SsaInstruction;
mod common;
use common::*;

/// Test 1: Simple storage load - should generate unfold before + fold after
#[test]
fn test_simple_storage_load() {
    let source = r#"
            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo() {
                    var x: int = balance;
                }
            }
        "#;

    let (program, _) = build_ssa_with_storage(source).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    // Expected SSA IR:
    //   unfold balance       <- Before load
    //   %1 = load balance
    //   fold balance         <- Cleanup
    //   return

    let instructions = &entry_block.instructions;

    // Find positions of each instruction type
    let unfold_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::Unfold { .. }))
        .expect("Should have unfold instruction");

    let load_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::StorageLoad { .. }))
        .expect("Should have load instruction");

    let fold_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::Fold { .. }))
        .expect("Should have fold instruction");

    // Verify order: unfold < load < fold
    assert!(unfold_idx < load_idx, "Unfold should come before load");
    assert!(load_idx < fold_idx, "Load should come before fold");
}

/// Test 2: Storage write - should generate unfold before + fold after
#[test]
fn test_simple_storage_write() {
    let source = r#"
            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo() {
                    balance = 100;
                }
            }
        "#;

    let (program, _) = build_ssa_with_storage(source).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    // Expected SSA IR:
    //   unfold balance
    //   store balance, 100
    //   fold balance
    //   return

    let instructions = &entry_block.instructions;

    let unfold_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::Unfold { .. }))
        .expect("Should have unfold");

    let store_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::StorageStore { .. }))
        .expect("Should have store");

    let fold_idx = instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::Fold { .. }))
        .expect("Should have fold");

    assert!(unfold_idx < store_idx, "Unfold before store");
    assert!(store_idx < fold_idx, "Store before fold");
}

/// Test 3: CEI Violation - write after external call should be rejected
#[test]
fn test_cei_violation_write_after_call() {
    let contracts = [
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo(ext: External) {
                    ext.bar();
                    balance = 100;  // ERROR: Write after external call
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts.to_vec());

    assert!(
        result.is_err(),
        "Write after external call should be rejected"
    );

    let error_msg = format!("{:?}", result.unwrap_err());
    assert!(
        error_msg.contains("StorageAccessAfterExternalCall") || error_msg.contains("write"),
        "Error should mention StorageAccessAfterExternalCall violation, got: {}",
        error_msg
    );
}

/// Test 4: Load then store same variable - should only unfold once
#[test]
fn test_load_then_store_single_unfold() {
    let source = r#"
            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo() {
                    balance = balance + 100;
                }
            }
        "#;

    let (program, _) = build_ssa_with_storage(source).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    let unfold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Unfold { .. }));

    let fold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Fold { .. }));

    assert_eq!(unfold_count, 1, "Should only unfold once for load+store");
    assert_eq!(fold_count, 1, "Should fold once at the end");
}

/// Test 5: Multiple storage variables with external call
#[test]
fn test_external_call_multiple_storage_vars() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                state var count: {int | count >= 0} = 0;
                
                entrypoint foo(ext: External) {
                    ext.bar();
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let (program, _) = load_test_contracts_with_storage(contracts).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    let unfold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Unfold { .. }));

    let fold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Fold { .. }));

    assert_eq!(
        unfold_count, 2,
        "Should unfold both storage vars before external call"
    );
    assert_eq!(
        fold_count, 2,
        "Should fold both storage vars after external call"
    );
}

/// Test 6: Read after external call is ALLOWED
#[test]
fn test_read_after_external_call_allowed() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo(ext: External) {
                    ext.bar();
                    var x: int = balance;  // READ after call - OK
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts);
    assert!(result.is_ok(), "Read after external call should be allowed");
}

/// Test 7: Write in conditional after external call is FORBIDDEN
#[test]
fn test_write_in_conditional_after_external_call() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo(ext: External, cond: bool) {
                    ext.bar();
                    if (cond) {
                        balance = 100;  // ERROR even in conditional
                    }
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts);
    assert!(
        result.is_err(),
        "Write after external call should be forbidden even in conditional"
    );

    let error_msg = format!("{:?}", result.unwrap_err());
    assert!(
        error_msg.contains("StorageAccessAfterExternalCall"),
        "Error should mention storage access after external call"
    );
}

/// Test 8: Write to state const is FORBIDDEN
#[test]
fn test_write_to_const_forbidden() {
    let source = r#"
            contract Test {
                state const MAX: int = 1000;
                
                entrypoint foo() {
                    MAX = 2000;  // ERROR: Cannot write to const
                }
            }
        "#;

    let result = build_ssa_with_storage(source);
    assert!(
        result.is_err(),
        "Writing to state const should be forbidden"
    );

    let error_msg = format!("{:?}", result.unwrap_err());
    assert!(
        error_msg.contains("WriteToImmutable") || error_msg.contains("const"),
        "Error should mention write to immutable/const, got: {}",
        error_msg
    );
}

/// Test 9: Internal call with external calls inside
#[test]
fn test_internal_with_external_call() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                internal function helper(ext: External) {
                    ext.bar();  // External call inside internal
                }
                
                entrypoint foo(ext: External) {
                    helper(ext);
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let (program, _) = load_test_contracts_with_storage(contracts).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");

    // Buscar el call a helper
    let call_block = find_block_with(cfg, |b| {
        b.instructions
            .iter()
            .any(|i| matches!(i, SsaInstruction::Call { .. }))
    })
    .expect("Should have call");

    let has_pre_call_unfold = call_block
        .instructions
        .iter()
        .take_while(|i| !matches!(i, SsaInstruction::Call { .. }))
        .any(|i| matches!(i, SsaInstruction::Unfold { .. }));

    assert!(
        has_pre_call_unfold,
        "Should unfold before internal call that contains external calls"
    );
}

/// Test 10: Correct CEI pattern (effects before interactions)
#[test]
fn test_correct_cei_pattern() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint withdraw(ext: External, amount: int) {
                    balance = balance - amount;  // Effect FIRST
                    ext.transfer(amount);        // Interaction AFTER
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint transfer(amount: int) {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts);
    assert!(result.is_ok(), "Correct CEI pattern should compile");
}

/// Test 11: Internal call WITHOUT external calls - should NOT generate fold/unfold
#[test]
fn test_internal_without_external_call() {
    let source = r#"
            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                internal function safe_helper() {
                    balance = balance + 1;  // Solo modifica, no llama external
                }
                
                entrypoint foo() {
                    safe_helper();
                }
            }
        "#;

    let (program, _) = build_ssa_with_storage(source).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    // El call a safe_helper NO debería generar unfold/fold alrededor
    // porque safe_helper no contiene external calls

    // Buscar el call
    let call_idx = entry_block
        .instructions
        .iter()
        .position(|i| matches!(i, SsaInstruction::Call { .. }))
        .expect("Should have call");

    // Check si hay unfold justo antes del call
    let has_unfold_before_call = call_idx > 0
        && matches!(
            entry_block.instructions[call_idx - 1],
            SsaInstruction::Unfold { .. }
        );

    assert!(
        !has_unfold_before_call,
        "Should NOT unfold before internal call without external calls"
    );
}

/// Test 12: Guarded entrypoint - allows CEI violation with runtime guard
#[test]
fn test_guarded_entrypoint() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo(ext: External) guarded {
                    ext.bar();
                    balance = 100;  // Write after call - OK porque guarded
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts);
    assert!(
        result.is_ok(),
        "Guarded entrypoint should allow write after external call"
    );
}

/// Test 13: Nested internal calls - transitive external call detection
#[test]
fn test_nested_internal_calls() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                internal function level3(ext: External) {
                    ext.bar();  // External call aquí
                }
                
                internal function level2(ext: External) {
                    level3(ext);  // Llama a level3
                }
                
                internal function level1(ext: External) {
                    level2(ext);  // Llama a level2
                }
                
                entrypoint foo(ext: External) {
                    level1(ext);  // Llama a level1
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let (program, _) = load_test_contracts_with_storage(contracts).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    // El call a level1 debe generar fold/unfold porque transitivamente
    // level1 → level2 → level3 → external call

    let has_unfold = entry_block
        .instructions
        .iter()
        .any(|i| matches!(i, SsaInstruction::Unfold { .. }));

    let has_fold = entry_block
        .instructions
        .iter()
        .any(|i| matches!(i, SsaInstruction::Fold { .. }));

    assert!(
        has_unfold,
        "Should unfold before nested call chain with external"
    );
    assert!(
        has_fold,
        "Should fold after nested call chain with external"
    );
}

/// Test 14: Multiple consecutive external calls
#[test]
fn test_multiple_consecutive_external_calls() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                entrypoint foo(ext1: External, ext2: External, ext3: External) {
                    ext1.bar();
                    ext2.bar();
                    ext3.bar();
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let (program, _) = load_test_contracts_with_storage(contracts).unwrap();
    let entry_block = &program.files.get("Test").unwrap().contract.functions[0]
        .blocks
        .values()
        .next()
        .unwrap();

    // Should have fold/unfold around EACH external call
    let unfold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Unfold { .. }));

    let fold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Fold { .. }));

    let call_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Call { .. }));

    assert_eq!(call_count, 3, "Should have 3 calls");
    assert_eq!(unfold_count, 3, "Should unfold before each call");
    assert_eq!(fold_count, 3, "Should fold after each call");
}

/// Test 15: State const should NOT be unfolded even with external calls
#[test]
fn test_const_not_unfolded_on_external_call() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                state const MAX: int = 1000;
                
                entrypoint foo(ext: External) {
                    ext.bar();
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let (program, _) = load_test_contracts_with_storage(contracts).unwrap();
    let cfg = get_function_cfg(&program, "Test", "foo");
    let entry_block = &cfg.blocks[&cfg.entry];

    // Should only unfold/fold balance (mutable), NOT MAX (const)
    let unfold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Unfold { .. }));

    let fold_count =
        count_instructions_of_type(entry_block, |i| matches!(i, SsaInstruction::Fold { .. }));

    // Solo 1 var mutable (balance), entonces 1 unfold y 1 fold
    assert_eq!(unfold_count, 1, "Should only unfold mutable var");
    assert_eq!(fold_count, 1, "Should only fold mutable var");
}

/// Test 16: Guarded entrypoint with internal call
#[test]
fn test_guarded_with_internal_call() {
    let contracts = vec![
        (
            "Test",
            r#"
            import External from External;

            contract Test {
                state var balance: {int | balance >= 0} = 0;
                
                internal function helper(ext: External) {
                    ext.bar();
                    balance = 100;  // Esto está dentro de un contexto guarded
                }
                
                entrypoint foo(ext: External) guarded {
                    helper(ext);  // Llamado desde guarded
                }
            }
            "#,
        ),
        (
            "External",
            r#"
            contract External {
                entrypoint bar() {}
            }
            "#,
        ),
    ];

    let result = load_test_contracts_with_storage(contracts);

    assert!(
        result.is_err(),
        "Internal function should still validate CEI even if called from guarded"
    );
}
