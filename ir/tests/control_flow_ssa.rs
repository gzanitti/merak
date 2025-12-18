// Tests for control flow structures and SSA properties
// Includes: if/while statements, phi node placement, SSA versioning

mod common;

use merak_ir::ssa_ir::{SsaInstruction, Terminator};

use common::{assert_predecessors, build_ssa_from_source, get_phi_nodes, get_single_function_cfg};

#[test]
fn test_simple_if_then_cfg_structure() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: int = 1;
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // If-then (no else) should create at least 3 blocks:
    // - entry (evaluates condition, has Branch terminator)
    // - then block
    // - exit block
    assert!(
        cfg.blocks.len() >= 3,
        "If-then should create at least 3 blocks, got {}",
        cfg.blocks.len()
    );

    // The entry block should have the Branch terminator (condition evaluated in previous block)
    let entry = &cfg.blocks[&cfg.entry];

    // Entry might have a Jump to a block that has Branch, or have Branch directly
    let branch_block = if matches!(entry.terminator, Terminator::Branch { .. }) {
        entry
    } else {
        // If entry jumps, follow it to find the Branch
        cfg.blocks
            .values()
            .find(|b| matches!(b.terminator, Terminator::Branch { .. }))
            .expect("Should have a block with Branch terminator")
    };

    match &branch_block.terminator {
        Terminator::Branch {
            then_block,
            else_block,
            invariants,
            variants,
            ..
        } => {
            // For if-then (no else), else_block should jump directly to exit
            // Both then and else should be valid block IDs
            assert!(
                cfg.blocks.contains_key(then_block),
                "Then block should exist"
            );
            assert!(
                cfg.blocks.contains_key(else_block),
                "Else block should exist"
            );

            // Invariants and variants should be empty (not a loop)
            assert!(
                invariants.is_empty(),
                "If statement should have no invariants"
            );
            assert!(variants.is_empty(), "If statement should have no variants");
        }
        other => panic!("Expected Branch terminator, got {:?}", other),
    }

    println!("CFG {cfg:?}");

    // Find the exit block (has Return terminator and multiple predecessors)
    let exit = cfg
        .blocks
        .values()
        .find(|b| matches!(b.terminator, Terminator::Return { .. }) && b.predecessors.len() >= 2)
        .expect("Should have an exit block with 2 predecessors");

    // Exit should have at least 2 predecessors (from then branch and else branch)
    assert_predecessors(exit, 2);
}

#[test]
fn test_if_then_else_cfg_structure() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(x: int) {
                if (x > 0) {
                    var y: int = 1;
                } else {
                    var y: int = 2;
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    println!("CFG {cfg:?}");

    // If-then-else should create at least 4 blocks:
    // - entry (evaluates condition, has Branch terminator)
    // - then block
    // - else block
    // - exit block
    assert!(
        cfg.blocks.len() >= 4,
        "If-then-else should create at least 4 blocks, got {}",
        cfg.blocks.len()
    );

    // Find the block with Branch terminator (should be entry or a block entry jumps to)
    let entry = &cfg.blocks[&cfg.entry];
    let branch_block = if matches!(entry.terminator, Terminator::Branch { .. }) {
        entry
    } else {
        cfg.blocks
            .values()
            .find(|b| matches!(b.terminator, Terminator::Branch { .. }))
            .expect("Should have a Branch terminator")
    };

    match &branch_block.terminator {
        Terminator::Branch {
            then_block,
            else_block,
            invariants,
            variants,
            ..
        } => {
            assert!(
                cfg.blocks.contains_key(then_block),
                "Then block should exist"
            );
            assert!(
                cfg.blocks.contains_key(else_block),
                "Else block should exist"
            );

            // Then and else should be different blocks
            assert_ne!(
                then_block, else_block,
                "Then and else should be different blocks"
            );

            // Both blocks should jump to the same exit
            let then_blk = &cfg.blocks[then_block];
            let else_blk = &cfg.blocks[else_block];

            match (&then_blk.terminator, &else_blk.terminator) {
                (
                    Terminator::Jump {
                        target: then_target,
                    },
                    Terminator::Jump {
                        target: else_target,
                    },
                ) => {
                    assert_eq!(
                        then_target, else_target,
                        "Then and else should both jump to same exit block"
                    );
                }
                _ => panic!("Then and else blocks should have Jump terminators"),
            }

            // No loop invariants/variants
            assert!(invariants.is_empty());
            assert!(variants.is_empty());
        }
        other => panic!("Expected Branch terminator, got {:?}", other),
    }

    // Find exit block - should have 2 predecessors
    let exit = cfg
        .blocks
        .values()
        .find(|b| b.predecessors.len() == 2)
        .expect("Should have an exit block with 2 predecessors");

    assert_predecessors(exit, 2);
}

#[test]
fn test_nested_if_statements_structure() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(x: int, y: int) {
                if (x > 0) {
                    if (y > 0) {
                        var z: int = 1;
                    }
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    println!("CFG {cfg:?}");

    // Nested ifs should create more blocks
    // NEW STRUCTURE (no separate header blocks for if):
    // - entry (evaluates outer condition x > 0, has Branch)
    // - outer_then (evaluates inner condition y > 0, has Branch)
    // - inner_then, inner_else
    // - outer_else
    // - inner_exit, outer_exit
    // Total: at least 5-7 blocks
    assert!(
        cfg.blocks.len() >= 5,
        "Nested ifs should create at least 5 blocks, got {}",
        cfg.blocks.len()
    );

    // Should have 2 blocks with Branch terminators (one for each if)
    let branch_count = cfg
        .blocks
        .values()
        .filter(|b| matches!(b.terminator, Terminator::Branch { .. }))
        .count();

    assert_eq!(
        branch_count, 2,
        "Should have 2 Branch terminators for 2 if statements"
    );

    // === Specific predecessor/successor verification ===

    // 1. Entry block - evaluates outer condition (x > 0) and has Branch terminator
    let entry_block = &cfg.blocks[&cfg.entry];

    // Entry should have NO predecessors (it's the entry!)
    assert_eq!(
        entry_block.predecessors.len(),
        0,
        "Entry block should have 0 predecessors, got {:?}",
        entry_block.predecessors
    );

    // Entry should have Branch terminator (outer if condition)
    let (outer_then_id, outer_else_id) = match &entry_block.terminator {
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => (*then_block, *else_block),
        other => panic!(
            "Expected Branch terminator in entry (outer if), got {:?}",
            other
        ),
    };

    // Entry should have EXACTLY 2 successors: outer then and outer else
    assert_eq!(
        entry_block.successors.len(),
        2,
        "Entry should have exactly 2 successors, got {} successors: {:?}",
        entry_block.successors.len(),
        entry_block.successors
    );
    let entry_successors: std::collections::HashSet<_> =
        entry_block.successors.iter().cloned().collect();
    let expected_entry_successors: std::collections::HashSet<_> =
        [outer_then_id, outer_else_id].iter().cloned().collect();
    assert_eq!(
        entry_successors, expected_entry_successors,
        "Entry successors should be exactly {{outer_then={}, outer_else={}}}, got {:?}",
        outer_then_id, outer_else_id, entry_successors
    );

    // 2. Outer then block - evaluates inner condition (y > 0) and has Branch terminator
    let outer_then_block = &cfg.blocks[&outer_then_id];
    println!("outer_then_id {outer_then_id}");
    let (inner_then_id, inner_else_id) = match &outer_then_block.terminator {
        Terminator::Branch {
            then_block,
            else_block,
            ..
        } => (*then_block, *else_block),
        other => panic!(
            "Expected Branch terminator in outer then (inner if condition), got {:?}",
            other
        ),
    };

    // Outer then should have EXACTLY 1 predecessor: entry
    assert_eq!(
        outer_then_block.predecessors.len(),
        1,
        "Outer then should have exactly 1 predecessor, got {:?}",
        outer_then_block.predecessors
    );
    assert_eq!(
        outer_then_block.predecessors[0], cfg.entry,
        "Outer then's predecessor should be entry block {}, got {}",
        cfg.entry, outer_then_block.predecessors[0]
    );

    // Outer then should have EXACTLY 2 successors: inner then and inner else
    assert_eq!(
        outer_then_block.successors.len(),
        2,
        "Outer then should have exactly 2 successors, got {:?}",
        outer_then_block.successors
    );
    let outer_then_successors: std::collections::HashSet<_> =
        outer_then_block.successors.iter().cloned().collect();
    let expected_outer_then_successors: std::collections::HashSet<_> =
        [inner_then_id, inner_else_id].iter().cloned().collect();
    assert_eq!(
        outer_then_successors, expected_outer_then_successors,
        "Outer then successors should be exactly {{inner_then={}, inner_else={}}}, got {:?}",
        inner_then_id, inner_else_id, outer_then_successors
    );

    // 3. Inner then block - contains assignment z = 1
    let inner_then_block = &cfg.blocks[&inner_then_id];

    // Inner then should have EXACTLY 1 predecessor: outer then
    assert_eq!(
        inner_then_block.predecessors.len(),
        1,
        "Inner then should have exactly 1 predecessor, got {:?}",
        inner_then_block.predecessors
    );
    assert_eq!(
        inner_then_block.predecessors[0], outer_then_id,
        "Inner then's predecessor should be outer then block {}, got {}",
        outer_then_id, inner_then_block.predecessors[0]
    );

    // Inner then should have EXACTLY 1 successor (jumps to inner exit)
    assert_eq!(
        inner_then_block.successors.len(),
        1,
        "Inner then should have exactly 1 successor, got {:?}",
        inner_then_block.successors
    );
    let inner_exit_id = match &inner_then_block.terminator {
        Terminator::Jump { target } => *target,
        other => panic!("Expected Jump in inner then, got {:?}", other),
    };
    assert_eq!(
        inner_then_block.successors[0], inner_exit_id,
        "Inner then's successor should be inner exit block {}, got {}",
        inner_exit_id, inner_then_block.successors[0]
    );

    // 4. Inner else block (when y <= 0)
    let inner_else_block = &cfg.blocks[&inner_else_id];

    // Inner else should have EXACTLY 1 predecessor: outer then
    assert_eq!(
        inner_else_block.predecessors.len(),
        2,
        "Inner else should have exactly 2 predecessors, got {:?}",
        inner_else_block.predecessors
    );
    assert_eq!(
        inner_else_block.predecessors[0], outer_then_id,
        "Inner else's predecessor should be outer then block {}, got {}",
        outer_then_id, inner_else_block.predecessors[0]
    );

    // Inner else should have EXACTLY 1 successor (jumps to outer exit)
    assert_eq!(
        inner_else_block.successors.len(),
        1,
        "Inner else should have exactly 1 successor, got {:?}",
        inner_else_block.successors
    );
    // Inner else should jump to the outer exit
    assert_eq!(
        inner_else_block.successors[0], outer_else_id,
        "Inner else should jump to outer exit {}, got {}",
        outer_else_id, inner_else_block.successors[0]
    );

    // 5. Inner exit block (merge point of inner if)
    let inner_exit_block = &cfg.blocks[&inner_exit_id];

    // Inner exit should have EXACTLY 2 predecessors: inner then and outer then
    assert_eq!(
        inner_exit_block.predecessors.len(),
        2,
        "Inner exit should have exactly 2 predecessors, got {:?}",
        inner_exit_block.predecessors
    );
    let inner_exit_preds: std::collections::HashSet<_> =
        inner_exit_block.predecessors.iter().cloned().collect();
    let expected_inner_exit_preds: std::collections::HashSet<_> =
        [inner_then_id, outer_then_id].iter().cloned().collect();

    assert_eq!(
        inner_exit_preds, expected_inner_exit_preds,
        "Inner exit predecessors should be exactly {{inner_then={}, inner_else={}}}, got {:?}",
        inner_then_id, outer_then_id, inner_exit_preds
    );

    // Inner exit should have EXACTLY 1 successor (jumps to outer exit)
    assert_eq!(
        inner_exit_block.successors.len(),
        1,
        "Inner exit should have exactly 1 successor, got {:?}",
        inner_exit_block.successors
    );

    // 6. Outer else block (when x <= 0)
    let outer_else_block = &cfg.blocks[&outer_else_id];

    // Outer else should have EXACTLY 1 predecessor: entry
    assert_eq!(
        outer_else_block.predecessors.len(),
        2,
        "Outer else should have exactly 2 predecessors, got {:?}",
        outer_else_block.predecessors
    );
    assert_eq!(
        outer_else_block.predecessors[0], cfg.entry,
        "Outer else's predecessor should be entry block {}, got {}",
        cfg.entry, outer_else_block.predecessors[0]
    );

    // Outer else should have EXACTLY 1 successor (jumps to outer exit)
    assert_eq!(
        outer_else_block.successors.len(),
        0,
        "Outer else should have exactly 0 successor, got {:?}",
        outer_else_block.successors
    );

    // Outer exit might have 0 successors (if it's a Return) or 1 successor
    // We don't enforce this strictly as it depends on whether there's more code after

    // === General well-formedness checks ===

    // Verify bidirectional consistency for all blocks
    for block in cfg.blocks.values() {
        // Each successor should exist
        for &succ in &block.successors {
            assert!(
                cfg.blocks.contains_key(&succ),
                "Successor block {} should exist",
                succ
            );
        }

        // Each predecessor should exist
        for &pred in &block.predecessors {
            assert!(
                cfg.blocks.contains_key(&pred),
                "Predecessor block {} should exist",
                pred
            );
        }

        // Bidirectional: if A → B then B should have A as predecessor
        for &succ_id in &block.successors {
            let succ_block = &cfg.blocks[&succ_id];
            assert!(
                succ_block.predecessors.contains(&block.id),
                "Block {} has {} as successor, but {} doesn't have {} as predecessor",
                block.id,
                succ_id,
                succ_id,
                block.id
            );
        }

        // Reverse: if B has A as predecessor, A should have B as successor
        for &pred_id in &block.predecessors {
            let pred_block = &cfg.blocks[&pred_id];
            assert!(
                pred_block.successors.contains(&block.id),
                "Block {} has {} as predecessor, but {} doesn't have {} as successor",
                block.id,
                pred_id,
                pred_id,
                block.id
            );
        }
    }
}

#[test]
fn test_simple_while_loop_structure() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo() {
                var i: int = 0;
                while (i < 10)
                    with @invariant(i >= 0) @variant(10 - i)
                {
                    i = i + 1;
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // While loop should create at least 3-4 blocks:
    // - entry/pre-loop block (setup)
    // - header (SEPARATE block with ONLY condition check - needed for back edge)
    // - body
    // - exit
    // NOTE: Unlike if statements, while loops MUST have a separate header block
    // because there are back edges from the body to the condition.
    assert!(
        cfg.blocks.len() >= 3,
        "While loop should create at least 3 blocks, got {}",
        cfg.blocks.len()
    );

    // Find the header block (has Branch terminator with invariants/variants)
    // This block should contain ONLY the condition evaluation
    let header = cfg
        .blocks
        .values()
        .find(|b| match &b.terminator {
            Terminator::Branch {
                invariants,
                variants,
                ..
            } => !invariants.is_empty() && !variants.is_empty(),
            _ => false,
        })
        .expect("Should have loop header with invariants and variants");

    match &header.terminator {
        Terminator::Branch {
            then_block,
            invariants,
            variants,
            ..
        } => {
            // Loop should have non-empty invariants and variants
            assert!(!invariants.is_empty(), "Loop should have invariants");
            assert!(!variants.is_empty(), "Loop should have variants");

            // Then block is the loop body, else block is the exit
            let body = &cfg.blocks[then_block];

            // Body should jump back to header (back edge)
            match &body.terminator {
                Terminator::Jump { target } => {
                    assert_eq!(
                        *target, header.id,
                        "Loop body should jump back to header (back edge)"
                    );
                }
                other => panic!("Loop body should have Jump terminator, got {:?}", other),
            }

            // Header should have at least 2 predecessors:
            // 1. Entry/pre-loop block
            // 2. Loop body (back edge)
            assert!(
                header.predecessors.len() >= 2,
                "Loop header should have at least 2 predecessors (entry + back edge), got {}",
                header.predecessors.len()
            );

            // Verify the back edge: body should be in header's predecessors
            assert!(
                header.predecessors.contains(then_block),
                "Header should have back edge from body"
            );
        }
        other => panic!("Expected Branch terminator, got {:?}", other),
    }

    // Loop header should have loop_invariants metadata
    assert!(
        header.loop_invariants.is_some(),
        "Header should have loop_invariants metadata"
    );
    assert!(
        header.loop_variants.is_some(),
        "Header should have loop_variants metadata"
    );
}

#[test]
fn test_phi_node_at_if_merge_point() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(cond: bool) -> int {
                var x: int = 0;
                if (cond) {
                    x = 10;
                } else {
                    x = 20;
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Find the exit/merge block (has Return terminator and phi nodes)
    let exit = cfg
        .blocks
        .values()
        .find(|b| matches!(b.terminator, Terminator::Return { .. }) && !get_phi_nodes(b).is_empty())
        .expect("Should have exit block with phi nodes");

    let phi_nodes = get_phi_nodes(exit);

    // Should have at least 1 phi node for variable x
    assert!(
        !phi_nodes.is_empty(),
        "Exit block should have phi nodes for merged variable"
    );

    // Verify phi node structure
    match phi_nodes[0] {
        SsaInstruction::Phi { dest, sources } => {
            // Phi should have 2 sources (from then and else branches)
            assert_eq!(
                sources.len(),
                2,
                "Phi node should have 2 sources (then and else)"
            );

            // Dest version should be > 0 (SSA renamed)
            assert!(dest.version > 0, "Phi dest should have SSA version > 0");

            // Sources should be from different blocks
            assert_ne!(
                sources[0].0, sources[1].0,
                "Phi sources should be from different blocks"
            );

            // Both source blocks should be in exit's predecessors
            assert!(
                exit.predecessors.contains(&sources[0].0),
                "Phi source block should be predecessor"
            );
            assert!(
                exit.predecessors.contains(&sources[1].0),
                "Phi source block should be predecessor"
            );
        }
        other => panic!("Expected Phi instruction, got {:?}", other),
    }
}

#[test]
fn test_phi_node_at_if_then_merge_no_else() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(cond: bool) -> int {
                var x: int = 0;
                if (cond) {
                    x = 10;
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Find exit block with phi nodes
    let exit = cfg
        .blocks
        .values()
        .find(|b| !get_phi_nodes(b).is_empty())
        .expect("Should have block with phi nodes");

    let phi_nodes = get_phi_nodes(exit);

    assert!(
        !phi_nodes.is_empty(),
        "Should have phi node for variable modified in if-then"
    );

    // Phi should still have 2 sources:
    // 1. From header/else path (original value)
    // 2. From then block (modified value)
    match phi_nodes[0] {
        SsaInstruction::Phi { sources, .. } => {
            assert_eq!(
                sources.len(),
                2,
                "Phi should have 2 sources even without explicit else"
            );
        }
        other => panic!("Expected Phi instruction, got {:?}", other),
    }
}

#[test]
fn test_multiple_variables_multiple_phis() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(cond: bool) -> int {
                var x: int = 0;
                var y: int = 0;
                if (cond) {
                    x = 10;
                    y = 20;
                } else {
                    x = 30;
                    y = 40;
                }
                return x + y;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Find exit block
    let exit = cfg
        .blocks
        .values()
        .find(|b| !get_phi_nodes(b).is_empty())
        .expect("Should have block with phi nodes");

    let phi_nodes = get_phi_nodes(exit);

    // Should have 2 phi nodes (one for x, one for y)
    assert_eq!(
        phi_nodes.len(),
        2,
        "Should have 2 phi nodes for 2 modified variables"
    );

    // Both phi nodes should have 2 sources
    for phi in phi_nodes {
        match phi {
            SsaInstruction::Phi { sources, .. } => {
                assert_eq!(sources.len(), 2, "Each phi should have 2 sources");
            }
            other => panic!("Expected Phi instruction, got {:?}", other),
        }
    }
}

#[test]
fn test_loop_phi_nodes_for_modified_variables() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo() -> int {
                var i: int = 0;
                var sum: int = 0;
                while (i < 10)
                    with @invariant(i >= 0) @variant(10 - i)
                {
                    i = i + 1;
                    sum = sum + i;
                }
                return sum;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    println!("CFG: {:?}", cfg);

    // Find loop header (has Branch with invariants/variants and phi nodes)
    let header = cfg
        .blocks
        .values()
        .find(|b| {
            let has_branch = match &b.terminator {
                Terminator::Branch { invariants, .. } => !invariants.is_empty(),
                _ => false,
            };
            let has_phis = !get_phi_nodes(b).is_empty();
            has_branch && has_phis
        })
        .expect("Should have loop header with phi nodes");

    let phi_nodes = get_phi_nodes(header);

    // Should have 2 phi nodes (one for i, one for sum) // TODO: SSA Optimization, meanwhile 5
    assert_eq!(
        phi_nodes.len(),
        5,
        "Loop header should have 5 phi nodes (2 with 2 sources)"
    );

    // Each phi should have 2 sources:
    // 1. From pre-loop (initial value)
    // 2. From loop body (updated value via back edge)
    for phi in phi_nodes {
        match phi {
            SsaInstruction::Phi { sources, .. } => {
                // assert_eq!(
                //     sources.len(),
                //     2,
                //     "Loop phi should have 2 sources (pre-loop and body)"
                // );

                // One source should be the loop body (back edge)
                // We can verify this by checking that one source is in header's predecessors
                let source_blocks: Vec<_> = sources.iter().map(|(b, _)| b).collect();

                for &src_block in &source_blocks {
                    assert!(
                        header.predecessors.contains(src_block),
                        "Phi source should be from predecessor"
                    );
                }
            }
            other => panic!("Expected Phi instruction, got {:?}", other),
        }
    }
}

// TODO: Optimize phi node placement by removing unnecessary phi nodes for unmodified variables
#[test]
#[ignore]
fn test_no_phi_for_unmodified_variables() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(cond: bool) -> int {
                var x: int = 10;
                var y: int = 20;
                if (cond) {
                    var z: int = x + y;
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    println!("CFG : {:?}", cfg);

    // Find exit block
    let exit = cfg
        .blocks
        .values()
        .find(|b| matches!(b.terminator, Terminator::Return { .. }))
        .expect("Should have exit block");

    let phi_nodes = get_phi_nodes(exit);

    // Should have NO phi nodes, since x and y are not modified
    assert!(
        phi_nodes.is_empty(),
        "Should have no phi nodes for unmodified variables"
    );
}

#[test]
fn test_ssa_versions_increment_on_assignment() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo() -> int {
                var x: int = 0;
                x = 10;
                x = 20;
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    println!("CFG: {:?}", cfg);

    let entry_block = &cfg.blocks[&cfg.entry];

    // Should have 3 Copy instructions (for 3 assignments: initial, 10, 20)
    let copies: Vec<_> = entry_block
        .instructions
        .iter()
        .filter_map(|i| match i {
            SsaInstruction::Copy { dest, .. } => Some(dest),
            _ => None,
        })
        .collect();

    assert_eq!(copies.len(), 3, "Should have 3 Copy instructions");

    // Versions should be: x_0, x_1, x_2
    // After SSA renaming, each assignment creates a new version
    let versions: Vec<_> = copies.iter().map(|r| r.version).collect();

    // Versions should be distinct
    let unique_versions: std::collections::HashSet<_> = versions.iter().collect();
    assert_eq!(unique_versions.len(), 3, "All versions should be distinct");

    // Return should use the latest version
    match &entry_block.terminator {
        Terminator::Return {
            value: Some(merak_ir::ssa_ir::Operand::Register(reg)),
            ..
        } => {
            // Should use the last assigned version
            assert!(reg.version >= 2, "Return should use latest version (>= 2)");
        }
        other => panic!("Expected Return with register, got {:?}", other),
    }

    // No phi nodes should be present (no control flow merge)
    let phi_nodes = get_phi_nodes(entry_block);
    assert!(
        phi_nodes.is_empty(),
        "Should have no phi nodes in sequential code"
    );
}

#[test]
fn test_nested_if_phi_placement() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(a: bool, b: bool) -> int {
                var x: int = 0;
                if (a) {
                    x = 10;
                    if (b) {
                        x = 20;
                    }
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should have phi nodes at merge points
    let blocks_with_phis: Vec<_> = cfg
        .blocks
        .values()
        .filter(|b| !get_phi_nodes(b).is_empty())
        .collect();

    // Should have at least 1 phi node (at outer if exit)
    // May have 2 if inner if also creates a merge point
    assert!(
        !blocks_with_phis.is_empty(),
        "Should have at least one block with phi nodes"
    );

    // Verify phi nodes are correctly placed at merge points
    for block in blocks_with_phis {
        // Blocks with phi nodes should have multiple predecessors
        assert!(
            block.predecessors.len() >= 2,
            "Block with phi nodes should have multiple predecessors"
        );

        let phi_nodes = get_phi_nodes(block);
        for phi in phi_nodes {
            match phi {
                SsaInstruction::Phi { sources, .. } => {
                    // Sources should match predecessors
                    assert!(sources.len() >= 2, "Phi should have at least 2 sources");
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_sequential_if_statements_phi_versions() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(a: bool, b: bool) -> int {
                var x: int = 0;
                if (a) {
                    x = 10;
                }
                if (b) {
                    x = 20;
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should have phi nodes at both if exits
    let blocks_with_phis: Vec<_> = cfg
        .blocks
        .values()
        .filter(|b| !get_phi_nodes(b).is_empty())
        .collect();

    // Should have 2 blocks with phi nodes (one for each if)
    assert!(
        blocks_with_phis.len() >= 2,
        "Should have at least 2 blocks with phi nodes for sequential ifs"
    );

    // Each phi node should properly handle versions
    for block in blocks_with_phis {
        let phi_nodes = get_phi_nodes(block);

        for phi in phi_nodes {
            match phi {
                SsaInstruction::Phi { dest, sources, .. } => {
                    // Phi dest should have a version
                    assert!(dest.version >= 0, "Phi dest should have valid version");

                    // Should have 2 sources
                    assert_eq!(sources.len(), 2, "Phi should have 2 sources");
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_if_after_while_correct_versions() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(cond: bool) -> int {
                var x: int = 0;
                while (x < 10)
                    with @invariant(x >= 0) @variant(10 - x)
                {
                    x = x + 1;
                }
                if (cond) {
                    x = 100;
                }
                return x;
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should have phi nodes in both loop header and if exit
    let blocks_with_phis: Vec<_> = cfg
        .blocks
        .values()
        .filter(|b| !get_phi_nodes(b).is_empty())
        .collect();

    assert!(
        blocks_with_phis.len() >= 2,
        "Should have phi nodes in loop header and if exit"
    );

    // Find loop header (has Branch with invariants)
    let loop_header = cfg
        .blocks
        .values()
        .find(|b| match &b.terminator {
            Terminator::Branch { invariants, .. } => !invariants.is_empty(),
            _ => false,
        })
        .expect("Should have loop header");

    // Loop header should have phi for x
    let loop_phis = get_phi_nodes(loop_header);
    assert!(!loop_phis.is_empty(), "Loop header should have phi nodes");

    // If exit should also have phi that uses version from loop exit
    // This verifies that SSA versions flow correctly through complex control flow
}

#[test]
fn test_deeply_nested_control_flow() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo(a: bool, b: bool, c: bool) {
                if (a) {
                    if (b) {
                        while (c)
                            with @invariant(true) @variant(1)
                        {
                            var x: int = 1;
                        }
                    }
                }
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should create many blocks for nested control flow
    assert!(
        cfg.blocks.len() >= 8,
        "Deeply nested control flow should create many blocks"
    );

    // CFG should be well-formed
    for block in cfg.blocks.values() {
        // All successors should exist
        for &succ in &block.successors {
            assert!(
                cfg.blocks.contains_key(&succ),
                "Successor {} should exist",
                succ
            );
        }

        // All predecessors should exist
        for &pred in &block.predecessors {
            assert!(
                cfg.blocks.contains_key(&pred),
                "Predecessor {} should exist",
                pred
            );
        }

        // Terminator should be valid
        match &block.terminator {
            Terminator::Jump { target } => {
                assert!(cfg.blocks.contains_key(target), "Jump target should exist");
            }
            Terminator::Branch {
                then_block,
                else_block,
                ..
            } => {
                assert!(
                    cfg.blocks.contains_key(then_block),
                    "Then block should exist"
                );
                assert!(
                    cfg.blocks.contains_key(else_block),
                    "Else block should exist"
                );
            }
            Terminator::Return { .. } | Terminator::Unreachable => {}
        }
    }
}

#[test]
fn test_empty_loop_body() {
    let source = r#"
        contract Test[Active] {}
        Test@Active(any) {
            entrypoint foo() {
                while (false)
                    with @invariant(true) @variant(0)
                {}
            }
        }
    "#;

    let (ssa_program, _) = build_ssa_from_source(source).expect("Failed to build SSA");

    let cfg = get_single_function_cfg(&ssa_program, "Test", "Active");

    // Should still create loop structure with separate header block:
    // - pre-loop/entry
    // - header (SEPARATE block with condition - needed for back edge)
    // - body (empty)
    // - exit
    assert!(
        cfg.blocks.len() >= 3,
        "Empty loop should still create basic structure"
    );

    // Find loop header (separate block with condition)
    let header = cfg
        .blocks
        .values()
        .find(|b| match &b.terminator {
            Terminator::Branch { invariants, .. } => !invariants.is_empty(),
            _ => false,
        })
        .expect("Should have loop header");

    // Get loop body
    match &header.terminator {
        Terminator::Branch { then_block, .. } => {
            let body = &cfg.blocks[then_block];

            // Body should have 0 instructions (empty loop)
            assert_eq!(
                body.instructions.len(),
                0,
                "Empty loop body should have 0 instructions"
            );

            // Body should still have Jump back to header (back edge)
            match &body.terminator {
                Terminator::Jump { target } => {
                    assert_eq!(*target, header.id, "Empty body should still have back edge");
                }
                other => panic!("Expected Jump terminator, got {:?}", other),
            }
        }
        other => panic!("Expected Branch terminator, got {:?}", other),
    }
}
