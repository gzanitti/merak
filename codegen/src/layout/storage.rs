/// Storage layout for EVM state variables
///
/// Maps Merak state variables to EVM storage slots (256-bit words).
/// Storage slots are numbered sequentially starting from 0.
use merak_symbols::SymbolId;
use std::collections::HashMap;

use crate::lowering::EvmType;

/// Storage slot identifier (256-bit word at position `offset`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSlot {
    /// Base storage slot number
    pub offset: u64,
    /// Size in slots (for arrays/structs, currently always 1)
    pub size: usize,
}

impl StorageSlot {
    pub fn new(offset: u64) -> Self {
        StorageSlot { offset, size: 1 }
    }

    pub fn new_with_size(offset: u64, size: usize) -> Self {
        StorageSlot { offset, size }
    }
}

/// Storage layout mapping state variables to storage slots
#[derive(Debug, Clone)]
pub struct StorageLayout {
    /// Map from state variable symbol to storage slot
    slots: HashMap<SymbolId, StorageSlot>,
    /// Next available slot
    next_slot: u64,
}

impl StorageLayout {
    /// Create a new empty storage layout
    pub fn new() -> Self {
        Self {
            slots: HashMap::new(),
            next_slot: 0,
        }
    }

    /// Assign a storage slot to a state variable
    pub fn assign(&mut self, var: SymbolId, ty: &EvmType) -> StorageSlot {
        let size = ty.storage_size();
        let slot = StorageSlot::new_with_size(self.next_slot, size);

        self.next_slot += size as u64;
        self.slots.insert(var, slot);

        slot
    }

    /// Get the storage slot for a variable
    pub fn get(&self, var: &SymbolId) -> Option<StorageSlot> {
        self.slots.get(var).copied()
    }

    /// Get all variable-slot mappings
    pub fn all_slots(&self) -> &HashMap<SymbolId, StorageSlot> {
        &self.slots
    }

    /// Total number of slots used
    pub fn total_slots(&self) -> u64 {
        self.next_slot
    }
}

impl Default for StorageLayout {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use merak_symbols::SymbolId;

    #[test]
    fn test_storage_assignment() {
        let mut layout = StorageLayout::new();

        let var1 = SymbolId::new("var1".to_string(), 1);
        let var2 = SymbolId::new("var2".to_string(), 2);

        let slot1 = layout.assign(var1, &EvmType::Uint256);
        let slot2 = layout.assign(var2, &EvmType::Address);

        assert_eq!(slot1.offset, 0);
        assert_eq!(slot2.offset, 1);
        assert_eq!(layout.total_slots(), 2);
    }

    #[test]
    fn test_storage_retrieval() {
        let mut layout = StorageLayout::new();
        let var = SymbolId::new("var".to_string(), 1);

        layout.assign(var.clone(), &EvmType::Bool);

        let retrieved = layout.get(&var);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().offset, 0);
    }
}
