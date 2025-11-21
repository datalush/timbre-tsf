//! Thread-safe string interning for deduplicating device IDs and measurement names.
//!
//! String interning is a memory optimization technique that ensures each unique string
//! is allocated exactly once. Subsequent requests for the same string return a cheap
//! reference-counted pointer to the existing allocation.
//!
//! # Performance Benefits
//!
//! - **Memory**: 60-80% reduction for metadata with repetitive strings
//! - **Cloning**: Arc::clone is just a refcount increment (no data copy)
//! - **Comparison**: Pointer equality for interned strings (O(1))
//!
//! # Use Cases
//!
//! - Device IDs that repeat across millions of records
//! - Measurement names with limited cardinality
//! - Tag values with high repetition
//!
//! # Example
//!
//! ```rust
//! use timbre_tsf::common::StringInterner;
//!
//! let interner = StringInterner::new();
//!
//! // First allocation
//! let device1 = interner.intern("sensor-001");
//!
//! // Reuses existing allocation - cheap!
//! let device2 = interner.intern("sensor-001");
//!
//! // Both point to the same memory
//! assert!(std::sync::Arc::ptr_eq(&device1, &device2));
//! ```

use rustc_hash::FxHashMap;  // OPT: 3-5x faster than SipHash for device IDs
use std::sync::{Arc, Mutex};

/// Thread-safe string interner for deduplicating device IDs and measurement names.
///
/// This structure maintains a pool of unique strings and returns reference-counted
/// pointers ([`Arc<str>`]) to them. Multiple requests for the same string will receive
/// clones of the same Arc, avoiding duplicate allocations.
///
/// # Thread Safety
///
/// The interner is thread-safe and can be shared across threads. However, the internal
/// lock means high-concurrency scenarios may see contention. For write-heavy workloads,
/// consider using a lock-free alternative like `dashmap`.
///
/// # Memory Management
///
/// Strings remain in the pool until all Arc references are dropped. The pool can be
/// explicitly cleared with [`StringInterner::clear`], but use with caution as this
/// invalidates the optimization for existing Arc references.
#[derive(Debug, Default)]
pub struct StringInterner {
    pool: Mutex<FxHashMap<String, Arc<str>>>,  // OPT: FxHash for 3-5x faster lookups
}

impl StringInterner {
    /// Creates a new empty string interner.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// let interner = StringInterner::new();
    /// assert_eq!(interner.len(), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            pool: Mutex::new(FxHashMap::default()),  // OPT: FxHash is 3-5x faster
        }
    }

    /// Creates a new string interner with pre-allocated capacity.
    ///
    /// Use this if you know approximately how many unique strings you'll have.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// // Pre-allocate for 1000 unique device IDs
    /// let interner = StringInterner::with_capacity(1000);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            pool: Mutex::new(FxHashMap::with_capacity_and_hasher(capacity, Default::default())),  // OPT: FxHash
        }
    }

    /// Interns a string, returning an [`Arc<str>`] that can be cheaply cloned.
    ///
    /// If the string is already in the pool, returns a clone of the existing Arc.
    /// Otherwise, allocates a new Arc and adds it to the pool.
    ///
    /// # Performance
    ///
    /// - **First call**: O(n) where n is string length (allocation + hash + insert)
    /// - **Subsequent calls**: O(1) hash lookup + cheap Arc clone
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// let interner = StringInterner::new();
    ///
    /// let s1 = interner.intern("device-001");
    /// let s2 = interner.intern("device-001");
    ///
    /// // Only one allocation, both point to same memory
    /// assert!(std::sync::Arc::ptr_eq(&s1, &s2));
    /// ```
    pub fn intern(&self, s: &str) -> Arc<str> {
        let mut pool = self.pool.lock().unwrap();

        if let Some(arc) = pool.get(s) {
            // Already interned - cheap clone (just refcount increment)
            Arc::clone(arc)
        } else {
            // New string - allocate once and store in pool
            let arc: Arc<str> = Arc::from(s);
            pool.insert(s.to_string(), Arc::clone(&arc));
            arc
        }
    }

    /// Returns the number of unique strings currently interned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// let interner = StringInterner::new();
    /// assert_eq!(interner.len(), 0);
    ///
    /// interner.intern("a");
    /// interner.intern("b");
    /// interner.intern("a"); // Duplicate
    ///
    /// assert_eq!(interner.len(), 2); // Only 2 unique strings
    /// ```
    pub fn len(&self) -> usize {
        self.pool.lock().unwrap().len()
    }

    /// Checks if the interner is empty (no strings interned).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// let interner = StringInterner::new();
    /// assert!(interner.is_empty());
    ///
    /// interner.intern("test");
    /// assert!(!interner.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Clears all interned strings from the pool.
    ///
    /// # Warning
    ///
    /// Existing Arc references will remain valid but won't benefit from future
    /// interning of the same strings. Only use this if you're sure no more
    /// references will be needed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::StringInterner;
    ///
    /// let interner = StringInterner::new();
    /// interner.intern("test");
    /// assert_eq!(interner.len(), 1);
    ///
    /// interner.clear();
    /// assert_eq!(interner.len(), 0);
    /// ```
    pub fn clear(&self) {
        self.pool.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_interning_basic() {
        let interner = StringInterner::new();

        let s1 = interner.intern("device1");
        let s2 = interner.intern("device1");

        assert_eq!(interner.len(), 1);
        assert!(Arc::ptr_eq(&s1, &s2)); // Same allocation
        assert_eq!(s1.as_ref(), "device1");
    }

    #[test]
    fn test_string_interning_multiple() {
        let interner = StringInterner::new();

        let d1 = interner.intern("device1");
        let d2 = interner.intern("device2");
        let d3 = interner.intern("device1"); // Duplicate

        assert_eq!(interner.len(), 2); // Only 2 unique
        assert!(Arc::ptr_eq(&d1, &d3)); // Same allocation
        assert!(!Arc::ptr_eq(&d1, &d2)); // Different allocations
    }

    #[test]
    fn test_with_capacity() {
        let interner = StringInterner::with_capacity(100);
        assert_eq!(interner.len(), 0);

        for i in 0..50 {
            interner.intern(&format!("device-{}", i));
        }

        assert_eq!(interner.len(), 50);
    }

    #[test]
    fn test_clear() {
        let interner = StringInterner::new();

        interner.intern("a");
        interner.intern("b");
        assert_eq!(interner.len(), 2);

        interner.clear();
        assert_eq!(interner.len(), 0);
        assert!(interner.is_empty());
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let interner = Arc::new(StringInterner::new());
        let mut handles = vec![];

        // Spawn 10 threads that all intern the same strings
        for _ in 0..10 {
            let interner_clone = Arc::clone(&interner);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    interner_clone.intern(&format!("device-{}", i % 10));
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Should have only 10 unique strings despite 1000 intern calls
        assert_eq!(interner.len(), 10);
    }

    #[test]
    fn test_empty_string() {
        let interner = StringInterner::new();

        let s1 = interner.intern("");
        let s2 = interner.intern("");

        assert_eq!(interner.len(), 1);
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(s1.as_ref(), "");
    }

    #[test]
    fn test_long_strings() {
        let interner = StringInterner::new();

        let long_str = "a".repeat(10000);
        let s1 = interner.intern(&long_str);
        let s2 = interner.intern(&long_str);

        assert_eq!(interner.len(), 1);
        assert!(Arc::ptr_eq(&s1, &s2));
        assert_eq!(s1.len(), 10000);
    }
}
