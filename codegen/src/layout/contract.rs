/// Contract-level layout and function dispatcher
use merak_symbols::SymbolId;
use std::collections::HashMap;

use crate::evm::{abi, BytecodeBuilder, Label, Opcode};
use crate::lowering::StackSlot;

/// Function entry in dispatch table
#[derive(Debug, Clone)]
pub struct FunctionEntry {
    /// Function symbol ID
    pub symbol_id: SymbolId,
    /// Function selector (4-byte hash)
    pub selector: [u8; 4],
    /// ABI signature string
    pub abi_signature: String,

    /// Universal body entry label — JUMPDEST at the start of the function body.
    pub entry_label: Label,

    /// External-call prologue label inside the dispatcher.
    pub external_prologue_label: Label,

    /// External-call continuation label.
    pub external_continuation_label: Label,

    /// Memory base for this function's register file.
    pub frame_base: u64,

    /// Parameter stack slots, in source order.
    /// `frame_base + param_slots[i].0 * 32` is the calldata-load destination.
    pub param_slots: Vec<StackSlot>,
}

impl FunctionEntry {
    pub fn new(
        symbol_id: SymbolId,
        signature: &str,
        entry_label: Label,
        external_prologue_label: Label,
        external_continuation_label: Label,
        frame_base: u64,
        param_slots: Vec<StackSlot>,
    ) -> Self {
        let selector = abi::compute_selector(signature);
        Self {
            symbol_id,
            selector,
            abi_signature: signature.to_string(),
            entry_label,
            external_prologue_label,
            external_continuation_label,
            frame_base,
            param_slots,
        }
    }
}

/// Contract layout with function dispatch table
#[derive(Debug, Clone)]
pub struct ContractLayout {
    /// Function dispatch table: selector → function entry
    pub dispatch_table: HashMap<[u8; 4], FunctionEntry>,
    /// Constructor function (if present)
    pub constructor: Option<FunctionEntry>,
    /// Fallback function (if present)
    pub fallback: Option<FunctionEntry>,
}

impl ContractLayout {
    pub fn new() -> Self {
        Self {
            dispatch_table: HashMap::new(),
            constructor: None,
            fallback: None,
        }
    }

    /// Add a function to the dispatch table
    pub fn add_function(&mut self, entry: FunctionEntry) {
        self.dispatch_table.insert(entry.selector, entry);
    }

    /// Set constructor
    pub fn set_constructor(&mut self, entry: FunctionEntry) {
        self.constructor = Some(entry);
    }

    /// Set fallback function
    pub fn set_fallback(&mut self, entry: FunctionEntry) {
        self.fallback = Some(entry);
    }

    /// Get function by selector
    pub fn get_function(&self, selector: &[u8; 4]) -> Option<&FunctionEntry> {
        self.dispatch_table.get(selector)
    }

    /// Generate dispatcher bytecode.
    ///
    /// Layout emitted:
    /// ```text
    /// [selector load + shift]
    ///
    /// for each function:
    ///   DUP1, PUSH expected_selector, EQ, JUMPI → external_prologue_label
    /// POP, PUSH 0, PUSH 0, REVERT
    ///
    /// for each function:
    ///   external_prologue_label: JUMPDEST
    ///     POP selector
    ///     for each param: PUSH calldata_offset, CALLDATALOAD, PUSH frame_slot, MSTORE
    ///     PUSH2 external_continuation_label
    ///     PUSH2 entry_label, JUMP
    ///   external_continuation_label: JUMPDEST
    ///     PUSH 0, MSTORE          ; store return_val (on stack) to mem[0]
    ///     PUSH 32, PUSH 0, RETURN
    /// ```
    pub fn generate_dispatcher(&self, builder: &mut BytecodeBuilder) {
        if self.dispatch_table.is_empty() {
            builder.emit(Opcode::STOP);
            return;
        }

        self.load_function_selection(builder);

        for (_, entry) in &self.dispatch_table {
            self.emit_external_call_wrapper(entry, builder);
        }
    }

    /// Emit the external call wrapper for a single function entry.
    fn emit_external_call_wrapper(&self, entry: &FunctionEntry, builder: &mut BytecodeBuilder) {
        // ── External prologue ──────────────────────────────────────────────────
        builder.mark_label(entry.external_prologue_label);
        builder.emit(Opcode::JUMPDEST);
        // Selector is still on the stack from the dispatch check; discard it.
        builder.emit(Opcode::POP);

        // Load calldata arguments into the callee's frame memory slots.
        // Calldata layout: [selector: 4 bytes][arg0: 32 bytes][arg1: 32 bytes]...
        for (i, &param_slot) in entry.param_slots.iter().enumerate() {
            let calldata_offset = 4 + (i as u64 * 32);
            builder.push_u64(calldata_offset);
            builder.emit(Opcode::CALLDATALOAD);
            let frame_offset = entry.frame_base + param_slot.0 as u64 * 32;
            builder.push_u64(frame_offset);
            builder.emit(Opcode::MSTORE);
        }

        // Push the continuation address so the callee can return here, then jump.
        builder.push_label(entry.external_continuation_label);
        builder.jump_to(entry.entry_label);

        // ── External continuation ──────────────────────────────────────────────
        // Stack on entry: [return_value]  (left by the callee's SWAP1+JUMP)
        builder.mark_label(entry.external_continuation_label);
        builder.emit(Opcode::JUMPDEST);
        builder.push(&[0]); // offset = 0
        builder.emit(Opcode::MSTORE);
        builder.push(&[32]); // size
        builder.push(&[0]); // offset
        builder.emit(Opcode::RETURN);
    }

    // Load function selector from the first 4 bytes of calldata.
    fn load_function_selection(&self, builder: &mut BytecodeBuilder) {
        // CALLDATALOAD reads 32 bytes; SHR 224 shifts to extract the high 4 bytes.
        builder.push_u256(&[0; 32]);
        builder.emit(Opcode::CALLDATALOAD);
        let mut shift = [0u8; 32];
        shift[31] = 224;
        // 0xE0 = 224 bits
        builder.push_u256(&shift);
        builder.emit(Opcode::SHR);
        // Stack: [selector]

        // Check each function's selector.
        for (selector_bytes, entry) in &self.dispatch_table {
            builder.emit(Opcode::DUP1);
            // Stack: [selector, selector]
            let mut selector_u256 = [0u8; 32];
            selector_u256[28..32].copy_from_slice(selector_bytes);
            builder.push_u256(&selector_u256);
            // Stack: [expected, selector, selector]
            builder.emit(Opcode::EQ);
            // Stack: [match, selector]
            builder.jumpi_to(entry.external_prologue_label);
            // Stack after (if no jump): [selector]
        }

        // No match: pop selector and revert.
        builder.emit(Opcode::POP);
        builder.push_u256(&[0; 32]);
        // offset
        builder.push_u256(&[0; 32]);
        // size
        builder.emit(Opcode::REVERT);
    }
}

impl Default for ContractLayout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_entry_creation() {
        let entry_label = Label::new(0);
        let prologue_label = Label::new(1);
        let cont_label = Label::new(2);
        let entry = FunctionEntry::new(
            SymbolId::new("transfer".to_string(), 1),
            "transfer(address,uint256)",
            entry_label,
            prologue_label,
            cont_label,
            0x80,
            vec![],
        );

        // Verify selector matches known value
        assert_eq!(entry.selector, [0xa9, 0x05, 0x9c, 0xbb]);
    }

    #[test]
    fn test_dispatch_table() {
        let mut layout = ContractLayout::new();

        let entry = FunctionEntry::new(
            SymbolId::new("transfer".to_string(), 1),
            "transfer(address,uint256)",
            Label::new(0),
            Label::new(1),
            Label::new(2),
            0x80,
            vec![],
        );

        layout.add_function(entry.clone());

        let retrieved = layout.get_function(&entry.selector);
        assert!(retrieved.is_some());
        assert_eq!(
            retrieved.unwrap().symbol_id,
            SymbolId::new("transfer".to_string(), 1)
        );
    }
}
