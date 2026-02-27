/// Memory layout for EVM
///
/// Memory layout conventions:
/// - 0x00-0x3f: Scratch space (64 bytes)
/// - 0x40-0x5f: Free memory pointer (32 bytes)
/// - 0x60+: Allocated memory

/// Memory allocator for temporary data
pub struct MemoryAllocator {
    /// Next free memory position
    next_free: usize,
}

impl MemoryAllocator {
    /// Create allocator starting at free memory pointer position
    pub fn new() -> Self {
        Self {
            next_free: 0x60, // Start after scratch space and free pointer
        }
    }

    /// Allocate memory for a value of given size (in bytes)
    pub fn allocate(&mut self, size: usize) -> usize {
        let offset = self.next_free;
        self.next_free += size;
        // Align to 32-byte boundary
        self.next_free = (self.next_free + 31) & !31;
        offset
    }

    /// Get current free memory pointer value
    pub fn free_pointer(&self) -> usize {
        self.next_free
    }
}

impl Default for MemoryAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Standard memory regions
pub mod regions {
    /// Scratch space: 0x00-0x3f (64 bytes)
    /// Used for hashing, temporary calculations
    pub const SCRATCH_SPACE: usize = 0x00;
    pub const SCRATCH_SPACE_SIZE: usize = 64;

    /// Free memory pointer: 0x40 (32 bytes)
    /// Points to next free memory location
    pub const FREE_MEMORY_POINTER: usize = 0x40;

    /// Zero slot: 0x60 (32 bytes)
    /// Reserved for representing null pointers
    pub const ZERO_SLOT: usize = 0x60;

    /// Start of allocatable memory
    pub const ALLOCATABLE_START: usize = 0x60;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_allocation() {
        let mut alloc = MemoryAllocator::new();

        let offset1 = alloc.allocate(32);
        let offset2 = alloc.allocate(64);

        assert_eq!(offset1, 0x60);
        assert_eq!(offset2, 0x80); // 0x60 + 32 bytes aligned
    }

    #[test]
    fn test_memory_alignment() {
        let mut alloc = MemoryAllocator::new();

        // Allocate unaligned size
        let offset1 = alloc.allocate(10);
        let offset2 = alloc.allocate(32);

        assert_eq!(offset1, 0x60);
        assert_eq!(offset2, 0x80); // Aligned to 32-byte boundary
    }
}
