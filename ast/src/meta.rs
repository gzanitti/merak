use core::fmt;
use std::cell::RefCell;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceRef {
    pub start: usize,
    pub end: usize,
}

impl SourceRef {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn span(&self) -> std::ops::Range<usize> {
        self.start..self.end
    }

    pub fn unknown() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl fmt::Display for SourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

// Thread-local stack for tracking current source references during type checking
thread_local! {
    static SOURCE_REF_STACK: RefCell<Vec<SourceRef>> = RefCell::new(vec![]);
}

/// RAII guard for automatically managing source references.
///
/// This guard pushes a source reference onto the thread-local stack when created
/// and automatically pops it when dropped, ensuring proper cleanup even if errors occur.
///
/// # Example
/// ```ignore
/// let _guard = SourceRefGuard::new(source_ref.clone());
/// // Now SourceRefGuard::current() will return this source_ref
/// // When _guard goes out of scope, the source_ref is automatically removed
/// ```
pub struct SourceRefGuard;

impl SourceRefGuard {
    /// Creates a new guard and pushes the given source reference onto the stack.
    pub fn new(source_ref: SourceRef) -> Self {
        SOURCE_REF_STACK.with(|stack| {
            stack.borrow_mut().push(source_ref);
        });
        SourceRefGuard
    }

    /// Returns the current source reference from the top of the stack,
    /// or `SourceRef::unknown()` if the stack is empty.
    pub fn current() -> SourceRef {
        SOURCE_REF_STACK.with(|stack| {
            stack.borrow()
                .last()
                .cloned()
                .unwrap_or_else(SourceRef::unknown)
        })
    }
}

impl Drop for SourceRefGuard {
    fn drop(&mut self) {
        SOURCE_REF_STACK.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}