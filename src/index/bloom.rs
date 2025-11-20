//! Bloom filter implementation for probabilistic set membership testing
//!
//! A Bloom filter is a space-efficient probabilistic data structure that tests
//! whether an element is a member of a set. It can return:
//! - Definitely not in set (100% accurate)
//! - Maybe in set (false positives possible)
//!
//! Use case in TsFile: Skip reading chunks that definitely don't contain a queried value.

use crate::error::{Result, TsFileError};
use bit_vec::BitVec;
use std::hash::{Hash, Hasher};

/// Bloom filter for probabilistic set membership testing
#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits: BitVec,
    num_hash_functions: u32,
    num_bits: usize,
    num_items: usize,
}

impl BloomFilter {
    /// Create new bloom filter
    ///
    /// # Arguments
    /// * `expected_items` - Expected number of items (n)
    /// * `false_positive_rate` - Desired false positive rate (p)
    ///
    /// Calculates optimal m and k:
    /// - m = -n * ln(p) / (ln(2)^2)
    /// - k = m/n * ln(2)
    ///
    /// # Example
    /// ```
    /// use timbre_tsf::index::BloomFilter;
    ///
    /// let mut bloom = BloomFilter::new(1000, 0.01);
    /// bloom.insert(&"device001");
    /// assert!(bloom.might_contain(&"device001"));
    /// assert!(!bloom.might_contain(&"device999"));
    /// ```
    pub fn new(expected_items: usize, false_positive_rate: f64) -> Self {
        let num_bits = Self::calculate_num_bits(expected_items, false_positive_rate);
        let num_hash_functions = Self::calculate_num_hash_functions(expected_items, num_bits);

        Self {
            bits: BitVec::from_elem(num_bits, false),
            num_hash_functions,
            num_bits,
            num_items: 0,
        }
    }

    /// Create bloom filter with explicit parameters
    pub fn with_parameters(num_bits: usize, num_hash_functions: u32) -> Self {
        Self {
            bits: BitVec::from_elem(num_bits, false),
            num_hash_functions,
            num_bits,
            num_items: 0,
        }
    }

    /// Insert an item into the bloom filter
    pub fn insert<T: Hash>(&mut self, item: &T) {
        for i in 0..self.num_hash_functions {
            let hash = self.hash_with_seed(item, i);
            let index = (hash % self.num_bits as u64) as usize;
            self.bits.set(index, true);
        }
        self.num_items += 1;
    }

    /// Check if item might be in set
    ///
    /// Returns false if definitely not in set
    /// Returns true if maybe in set (could be false positive)
    pub fn might_contain<T: Hash>(&self, item: &T) -> bool {
        for i in 0..self.num_hash_functions {
            let hash = self.hash_with_seed(item, i);
            let index = (hash % self.num_bits as u64) as usize;
            if !self.bits.get(index).unwrap_or(false) {
                return false; // Definitely not in set
            }
        }
        true // Maybe in set
    }

    /// Serialize bloom filter to bytes
    ///
    /// Format: [num_hash_functions:4][num_bits:8][num_items:8][bits:variable]
    pub fn serialize(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&self.num_hash_functions.to_le_bytes());
        output.extend_from_slice(&(self.num_bits as u64).to_le_bytes());
        output.extend_from_slice(&(self.num_items as u64).to_le_bytes());
        output.extend_from_slice(&self.bits.to_bytes());
        output
    }

    /// Deserialize bloom filter from bytes
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        if data.len() < 20 {
            return Err(TsFileError::DecodingError(
                "Bloom filter data too short".to_string(),
            ));
        }

        // Read header
        let num_hash_functions =
            u32::from_le_bytes(data[0..4].try_into().map_err(|_| {
                TsFileError::DecodingError("Invalid num_hash_functions".to_string())
            })?);
        let num_bits = u64::from_le_bytes(
            data[4..12]
                .try_into()
                .map_err(|_| TsFileError::DecodingError("Invalid num_bits".to_string()))?,
        ) as usize;
        let num_items = u64::from_le_bytes(
            data[12..20]
                .try_into()
                .map_err(|_| TsFileError::DecodingError("Invalid num_items".to_string()))?,
        ) as usize;

        // Read bits
        let bits = BitVec::from_bytes(&data[20..]);

        Ok(Self {
            bits,
            num_hash_functions,
            num_bits,
            num_items,
        })
    }

    /// Get the number of items inserted
    pub fn num_items(&self) -> usize {
        self.num_items
    }

    /// Get the number of bits in the filter
    pub fn num_bits(&self) -> usize {
        self.num_bits
    }

    /// Get the number of hash functions used
    pub fn num_hash_functions(&self) -> u32 {
        self.num_hash_functions
    }

    /// Calculate optimal number of bits
    ///
    /// Formula: m = -n * ln(p) / (ln(2)^2)
    fn calculate_num_bits(n: usize, p: f64) -> usize {
        if n == 0 || p <= 0.0 || p >= 1.0 {
            return 1024; // Default fallback
        }
        let m = -(n as f64) * p.ln() / (2.0_f64.ln().powi(2));
        m.ceil().max(1.0) as usize
    }

    /// Calculate optimal number of hash functions
    ///
    /// Formula: k = m/n * ln(2)
    fn calculate_num_hash_functions(n: usize, m: usize) -> u32 {
        if n == 0 {
            return 1;
        }
        let k = (m as f64 / n as f64) * 2.0_f64.ln();
        k.ceil().clamp(1.0, 10.0) as u32 // Cap at 10 for performance
    }

    /// Hash with seed for multiple hash functions
    ///
    /// Uses double hashing technique: h(i) = h1 + i * h2
    fn hash_with_seed<T: Hash>(&self, item: &T, seed: u32) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        item.hash(&mut hasher);
        hasher.finish()
    }

    /// Calculate current false positive probability
    ///
    /// Formula: (1 - e^(-k*n/m))^k
    pub fn false_positive_probability(&self) -> f64 {
        if self.num_items == 0 {
            return 0.0;
        }
        let k = self.num_hash_functions as f64;
        let m = self.num_bits as f64;
        let n = self.num_items as f64;

        (1.0 - ((-k * n / m).exp())).powf(k)
    }

    /// Clear all bits and reset item count
    pub fn clear(&mut self) {
        self.bits.clear();
        self.bits.grow(self.num_bits, false);
        self.num_items = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_basic() {
        let mut bloom = BloomFilter::new(100, 0.01);

        bloom.insert(&"device001");
        bloom.insert(&"device002");
        bloom.insert(&"device003");

        assert!(bloom.might_contain(&"device001"));
        assert!(bloom.might_contain(&"device002"));
        assert!(bloom.might_contain(&"device003"));
        assert!(!bloom.might_contain(&"device999"));
    }

    #[test]
    fn test_bloom_integers() {
        let mut bloom = BloomFilter::new(100, 0.01);

        for i in 0..50 {
            bloom.insert(&i);
        }

        for i in 0..50 {
            assert!(bloom.might_contain(&i));
        }

        // Check false positive rate (should be very low for items 50-100)
        let mut false_positives = 0;
        for i in 50..100 {
            if bloom.might_contain(&i) {
                false_positives += 1;
            }
        }

        // Should have very few false positives
        assert!(false_positives < 5);
    }

    #[test]
    fn test_bloom_false_positive_rate() {
        let mut bloom = BloomFilter::new(1000, 0.01);

        // Insert 1000 items
        for i in 0..1000 {
            bloom.insert(&format!("item{}", i));
        }

        // All inserted items should be found
        for i in 0..1000 {
            assert!(bloom.might_contain(&format!("item{}", i)));
        }

        // Test false positive rate
        let mut false_positives = 0;
        for i in 1000..2000 {
            if bloom.might_contain(&format!("item{}", i)) {
                false_positives += 1;
            }
        }

        let actual_rate = false_positives as f64 / 1000.0;
        println!("Actual false positive rate: {}", actual_rate);
        println!("Expected: ~0.01");
        println!("Calculated FPP: {}", bloom.false_positive_probability());

        // Should be close to 0.01, allow some margin
        assert!(actual_rate < 0.03);
    }

    #[test]
    fn test_bloom_serialization() {
        let mut bloom = BloomFilter::new(100, 0.01);
        bloom.insert(&"test1");
        bloom.insert(&"test2");
        bloom.insert(&"test3");

        let serialized = bloom.serialize();
        let deserialized = BloomFilter::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.num_items(), bloom.num_items());
        assert_eq!(deserialized.num_bits(), bloom.num_bits());
        assert_eq!(
            deserialized.num_hash_functions(),
            bloom.num_hash_functions()
        );

        assert!(deserialized.might_contain(&"test1"));
        assert!(deserialized.might_contain(&"test2"));
        assert!(deserialized.might_contain(&"test3"));
        assert!(!deserialized.might_contain(&"test999"));
    }

    #[test]
    fn test_bloom_clear() {
        let mut bloom = BloomFilter::new(100, 0.01);
        bloom.insert(&"test1");
        bloom.insert(&"test2");

        assert!(bloom.might_contain(&"test1"));
        assert_eq!(bloom.num_items(), 2);

        bloom.clear();

        assert_eq!(bloom.num_items(), 0);
        assert!(!bloom.might_contain(&"test1"));
        assert!(!bloom.might_contain(&"test2"));
    }

    #[test]
    fn test_bloom_empty() {
        let bloom = BloomFilter::new(100, 0.01);
        assert!(!bloom.might_contain(&"anything"));
        assert_eq!(bloom.false_positive_probability(), 0.0);
    }

    #[test]
    fn test_bloom_parameters() {
        let bloom = BloomFilter::with_parameters(1024, 5);
        assert_eq!(bloom.num_bits(), 1024);
        assert_eq!(bloom.num_hash_functions(), 5);
    }
}
