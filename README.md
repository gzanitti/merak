# Merak

A language for Ethereum smart contracts that combines liquid types with explicit specifications to provide compile-time safety guarantees with minimal runtime overhead. Merak is a research project exploring the intersection of formal verification, type theory, and blockchain security. It represents a long-term vision for how smart contracts should be written: with mathematical certainty of correctness built into the development process itself.

> ⚠️ **Development Status**: Merak is currently in active development (Phase 7 of 15). While the core architecture is solid and several phases are complete, the compiler does not yet produce executable bytecode. This is a long-term research and engineering project with the goal of eventual production readiness.

## Overview

Merak is designed to address the fundamental challenge of smart contract security: vulnerabilities that lead to catastrophic financial losses. Rather than relying primarily on runtime checks or post-deployment audits, Merak uses **refinement types** (liquid types) to verify safety properties at compile time.

### Key Design Principles

- **Verification then Erasure**: Type refinements are checked at compile time and mostly erased before codegen, minimizing bytecode bloat
- **Trust Boundaries**: Runtime checks are inserted only where truly needed—external inputs, storage reads after potential reentrancy, and external calls
- **State Machine Paradigm**: Contracts are explicitly modeled as state machines with clear transition semantics
- **Explicit Specifications**: Function contracts use `requires`/`ensures` clauses (Hoare Logic style) rather than pure inference, giving developers fine-grained control over verification
- **Fold/Unfold Semantics**: Storage operations follow a rigorous model that captures EVM semantics and enables reentrancy safety analysis

## Language Features

### Refinement Types

Merak extends base types (int, address, bool) with logical predicates that constrain their values:

```merak
state var balance: {int | balance >= 0} = 0;

function withdraw(amount: {int | amount > 0 && amount <= balance}) {
    balance = balance - amount;
    // Compiler proves balance remains non-negative
}
```

The type system ensures that `balance` can never become negative, catching underflow bugs at compile time.

### State Machine Contracts

Contracts declare possible states upfront, then define functions per-state with explicit transitions:

```merak
contract Vault[Open, Closed] {
    state var balance: {int | balance >= 0} = 0;
    state const maxBalance: int = 1000;
    
    constructor(owner: address) {
        balance = 0;
    }
}

Vault@Open(any) {
    entrypoint deposit(amount: {int | amount > 0 && amount <= 100}) {
        balance = balance + amount;
        if (balance >= maxBalance) {
            become Closed;  // Explicit state transition
        }
    }
}

Vault@Closed(any) {
    entrypoint reset() {
        balance = 0;
        become Open;
    }
}
```

This paradigm makes contract behavior explicit.

### Function Contracts

Developers can specify preconditions and postconditions using standard Hoare Logic notation:

```merak
function transfer(to: address, amount: {int | amount > 0})
requires (balance >= amount, msg.sender == owner)
ensures (balance == old(balance) - amount)
{
    balance = balance - amount;
    // Transfer logic...
}
```

The compiler generates verification conditions to prove the implementation satisfies its specification.

### Loop Invariants and Variants

Loops require explicit invariants (what remains true each iteration) and variants (what decreases to prove termination):

```merak
while (i < n) 
with @invariant(0 <= i && i <= n) 
     @variant(n - i)
{
    sum = sum + i;
    i = i + 1;
}
```

This enables verification of loops without needing unbounded unrolling.

## Compiler Architecture

Merak follows a multi-phase pipeline inspired by established compiler designs (LLVM, rustc, GCC):

```
AST
 ↓
[✓] Phase 1: Symbol Resolution
 ↓
[✓] Phase 2: Basic Type Checking
 ↓
[✓] Phase 3: CFG Construction
 ↓
[✓] Phase 4: Dominance Analysis & Loop Detection
 ↓
[✓] Phase 5: SSA Transformation
 ↓
[✓] Phase 6: Storage Analysis (fold/unfold + reentrancy)
 ↓
[🔄] Phase 7: Refinement Inference / Type Checking (CURRENT)
 ↓
[ ] Phase 8: ANF Transformation
 ↓
[ ] Phase 9: Verification Condition Generation
 ↓
[ ] Phase 10: SMT Solving (Z3 integration)
 ↓
[ ] Phase 11: EVM Bytecode Generation
 ↓
[ ] Phase 12: EVM Optimizations
 ↓
[ ] Phase 13: ANF Optimizations
 ↓
[ ] Phase 14: SSA Optimizations
```

### Current Progress

**Completed Phases (1-6)**:
- Full AST with refinement type syntax
- Basic type checking for base types
- Control Flow Graph with basic blocks
- Dominance tree and natural loop detection
- SSA transformation
- Storage analysis providing fold/unfold semantics with invalidation tracking

**In Progress (Phase 7)**:
- Liquid types constraint generation system
- Four-phase inference algorithm: template assignment → constraint generation → iterative weakening → solution extraction
- Z3 SMT solver integration for constraint solving

**Planned (Phases 8-15)**:
- Full verification pipeline from type checking through SMT solving
- EVM bytecode generation
- Multi-level optimization passes (EVM, ANF, SSA)


## Example: Reentrancy Protection

One of Merak's goals is catching reentrancy vulnerabilities at compile time. Consider this pattern:

```merak
contract Bank[Active] {
    state var balances: mapping<address, {int | balance >= 0}>;
    
    function withdraw(amount: {int | amount > 0})
    requires (balances[msg.sender] >= amount)
    guarded  // Marks potential reentrancy
    {
        // Storage fold: assume balances[msg.sender] >= amount
        let balance = balances[msg.sender];
        
        // External call - invalidates ALL storage assumptions
        msg.sender.call.value(amount)("");
        
        // Storage unfold: verify balances[msg.sender] still valid
        // Compiler ERROR: balances[msg.sender] invalidated by external call
        balances[msg.sender] = balance - amount;  
    }
}
```

The `guarded` modifier tells Merak to perform reentrancy analysis. The compiler tracks storage invalidation across external calls and rejects unsafe patterns.


> ⚠️ **Testing Disclaimer**: All tests in this project were generated by AI and reviewed only briefly. They should not be considered reliable indicators of correctness. Comprehensive test coverage and validation remain a critical TODO.

## Future Roadmap

### Near-term (Phases 7-12)
- Complete liquid types inference engine with Z3 integration
- Implement full verification pipeline
- Generate optimized EVM bytecode
- Extend language with advanced features arrays, mappings and other structures
- Extensive testing on real-world contract patterns

### Medium-term (Optimizations)
- SSA-level optimizations (constant/copy propagation, dead code elimination)
- ANF-level optimizations (inlining, CSE, storage access coalescing)
- EVM-level optimizations (peephole patterns, stack optimization, gas optimization)

### Long-term (UX - Developer tooling)
- Integration with existing Solidity contracts
- Language server and IDE support
- Documentation and tutorials for beginners and advanced users

## Contributing

Merak is an ambitious long-term project. While it's not ready for production use, I believe formal verification is essential for blockchain's future. At this time, we are not accepting pull requests, but you can support the project via donations:

**Ethereum Donation Address**: `0x7ff4408bf503cdd3991771a18e8f8c364eace215`

Donations will go toward:
- Development time and research
- Security audits when approaching production readiness
- Documentation and developer tooling

## Contact

For questions, suggestions, or collaboration inquiries, please open an issue on this repository.

---