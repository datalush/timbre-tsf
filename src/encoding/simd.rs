//! SIMD acceleration for encoding algorithms
//!
//! This module provides hardware-accelerated encoding using CPU SIMD extensions:
//! - **AVX2**: 256-bit vectors (8 floats or 4 doubles) - Intel Haswell+, AMD Excavator+
//! - **SSE4.2**: 128-bit vectors (4 floats or 2 doubles) - fallback for older CPUs
//! - **Scalar**: Portable fallback for all architectures
//!
//! # Runtime Detection
//!
//! SIMD features are detected at runtime using `is_x86_feature_detected!()`.
//! The fastest available implementation is selected automatically:
//!
//! ```text
//! if AVX2 available → use AVX2 (8-wide)
//! else if SSE4.2 available → use SSE (4-wide)
//! else → use scalar (1-wide)
//! ```
//!
//! # Safety
//!
//! All SIMD functions are marked `unsafe` and use `#[target_feature]` to ensure
//! they're only called on CPUs that support the required instructions. Runtime
//! detection wrappers provide safe APIs.
//!
//! # Performance
//!
//! Expected speedups for Chimp128/Gorilla encoding:
//! - **AVX2**: 1.3-1.5x (XOR + identical detection vectorized)
//! - **SSE4.2**: 1.15-1.25x (partial vectorization)
//! - **Scalar**: 1.0x (baseline)
//!
//! # Example
//!
//! ```rust
//! use timbre_tsf::encoding::simd::xor_detect_identical;
//!
//! let current = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
//! let previous = [1.0f32, 2.1, 3.0, 4.1, 5.0, 6.1, 7.0, 8.1];
//!
//! // Automatically selects best implementation (AVX2/SSE/scalar)
//! let (xors, identical_mask) = xor_detect_identical(&current, &previous);
//!
//! // identical_mask bits: 1=identical, 0=different
//! // Index:  0    1    2    3    4    5    6    7
//! // Match:  Y    N    Y    N    Y    N    Y    N
//! // Mask:   1    0    1    0    1    0    1    0  = 0b10101010
//! assert_eq!(identical_mask, 0b10101010);
//! ```

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

/// XOR detection result for a batch of 8 floats
///
/// Contains the XOR values and a bitmask indicating which values are identical.
#[derive(Debug, Clone, PartialEq)]
pub struct XorBatch {
    /// XOR values for each of the 8 floats (as u32 bits)
    pub xors: [u32; 8],
    /// Bitmask: bit i = 1 if values[i] == prev_values[i]
    pub identical_mask: u8,
}

/// Computes XOR and detects identical values for 8 floats using best available SIMD
///
/// This is the main entry point for SIMD-accelerated XOR computation. It automatically
/// selects the best implementation based on runtime CPU feature detection:
///
/// - **AVX2** (preferred): Processes all 8 floats in one 256-bit vector
/// - **SSE4.2** (fallback): Processes in two 128-bit vectors (2x4 floats)
/// - **Scalar** (portable): Processes one float at a time
///
/// # Arguments
///
/// * `current` - Array of 8 current float values
/// * `previous` - Array of 8 previous float values
///
/// # Returns
///
/// * `XorBatch` containing XOR values and identical mask
///
/// # Performance
///
/// - **AVX2**: ~8x faster than scalar for XOR + comparison
/// - **SSE4.2**: ~4x faster than scalar
/// - **Scalar**: Baseline (no vectorization)
///
/// # Example
///
/// ```rust
/// use timbre_tsf::encoding::simd::xor_detect_identical;
///
/// let current = [20.0f32, 20.1, 20.0, 20.2, 20.0, 20.3, 20.0, 20.4];
/// let previous = [20.0f32, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0];
///
/// let result = xor_detect_identical(&current, &previous);
///
/// // Bits set where values are identical
/// assert_eq!(result.identical_mask & 0b00000001, 0b00000001); // Index 0: identical
/// assert_eq!(result.identical_mask & 0b00000010, 0b00000000); // Index 1: different
/// ```
pub fn xor_detect_identical(current: &[f32; 8], previous: &[f32; 8]) -> XorBatch {
    #[cfg(target_arch = "x86_64")]
    {
        // Runtime detection: prefer AVX2, fallback to SSE4.2, then scalar
        if is_x86_feature_detected!("avx2") {
            unsafe { xor_detect_identical_avx2(current, previous) }
        } else if is_x86_feature_detected!("sse4.2") {
            unsafe { xor_detect_identical_sse42(current, previous) }
        } else {
            xor_detect_identical_scalar(current, previous)
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        xor_detect_identical_scalar(current, previous)
    }
}

/// AVX2 implementation: Process all 8 floats in a single 256-bit vector
///
/// # Safety
///
/// Requires AVX2 support. Caller must ensure `is_x86_feature_detected!("avx2")` is true.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn xor_detect_identical_avx2(current: &[f32; 8], previous: &[f32; 8]) -> XorBatch {
    // SAFETY: All operations in this function are inside an unsafe fn with #[target_feature]
    // annotation, ensuring AVX2 is available. Pointer operations are valid because we're
    // working with properly aligned arrays.
    unsafe {
        // Load 8 floats into 256-bit AVX2 registers
        let curr_vec = _mm256_loadu_ps(current.as_ptr());
        let prev_vec = _mm256_loadu_ps(previous.as_ptr());

        // Compare: returns 0xFFFFFFFF for equal, 0x00000000 for different
        let cmp = _mm256_cmp_ps(curr_vec, prev_vec, _CMP_EQ_OQ);

        // Extract comparison mask (8 bits, one per float)
        let identical_mask = _mm256_movemask_ps(cmp) as u8;

        // Cast floats to integers for XOR
        let curr_int = _mm256_castps_si256(curr_vec);
        let prev_int = _mm256_castps_si256(prev_vec);

        // XOR all 8 values in parallel
        let xor_vec = _mm256_xor_si256(curr_int, prev_int);

        // Extract XOR values to array
        let mut xors = [0u32; 8];
        _mm256_storeu_si256(xors.as_mut_ptr() as *mut __m256i, xor_vec);

        XorBatch { xors, identical_mask }
    }
}

/// SSE4.2 implementation: Process 8 floats as two 128-bit vectors (2x4)
///
/// # Safety
///
/// Requires SSE4.2 support. Caller must ensure `is_x86_feature_detected!("sse4.2")` is true.
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse4.2")]
unsafe fn xor_detect_identical_sse42(current: &[f32; 8], previous: &[f32; 8]) -> XorBatch {
    // SAFETY: All operations in this function are inside an unsafe fn with #[target_feature]
    // annotation, ensuring SSE4.2 is available. Pointer operations are valid because we're
    // working with properly aligned arrays.
    unsafe {
        // Process first 4 floats
        let curr_lo = _mm_loadu_ps(current.as_ptr());
        let prev_lo = _mm_loadu_ps(previous.as_ptr());
        let cmp_lo = _mm_cmpeq_ps(curr_lo, prev_lo);
        let mask_lo = _mm_movemask_ps(cmp_lo) as u8;

        let curr_lo_int = _mm_castps_si128(curr_lo);
        let prev_lo_int = _mm_castps_si128(prev_lo);
        let xor_lo = _mm_xor_si128(curr_lo_int, prev_lo_int);

        // Process second 4 floats
        let curr_hi = _mm_loadu_ps(current[4..].as_ptr());
        let prev_hi = _mm_loadu_ps(previous[4..].as_ptr());
        let cmp_hi = _mm_cmpeq_ps(curr_hi, prev_hi);
        let mask_hi = _mm_movemask_ps(cmp_hi) as u8;

        let curr_hi_int = _mm_castps_si128(curr_hi);
        let prev_hi_int = _mm_castps_si128(prev_hi);
        let xor_hi = _mm_xor_si128(curr_hi_int, prev_hi_int);

        // Combine masks: low 4 bits from mask_lo, high 4 bits from mask_hi
        let identical_mask = mask_lo | (mask_hi << 4);

        // Extract XOR values
        let mut xors = [0u32; 8];
        _mm_storeu_si128(xors.as_mut_ptr() as *mut __m128i, xor_lo);
        _mm_storeu_si128(xors[4..].as_mut_ptr() as *mut __m128i, xor_hi);

        XorBatch { xors, identical_mask }
    }
}

/// Scalar fallback: Process floats one at a time (portable)
///
/// Used when no SIMD extensions are available, or on non-x86_64 architectures.
fn xor_detect_identical_scalar(current: &[f32; 8], previous: &[f32; 8]) -> XorBatch {
    let mut xors = [0u32; 8];
    let mut identical_mask = 0u8;

    for i in 0..8 {
        let curr_bits = current[i].to_bits();
        let prev_bits = previous[i].to_bits();
        xors[i] = curr_bits ^ prev_bits;

        if xors[i] == 0 {
            identical_mask |= 1 << i;
        }
    }

    XorBatch { xors, identical_mask }
}

/// Counts leading zeros for a batch of 8 u32 values
///
/// **Note**: This is NOT vectorized because x86 SIMD does not have a native
/// `lzcnt` instruction for vectors. We use scalar processing but with optimized
/// memory layout to help the compiler auto-vectorize if possible.
///
/// # Arguments
///
/// * `values` - Array of 8 u32 values
///
/// # Returns
///
/// * Array of 8 u8 counts (0-32 leading zeros per value)
///
/// # Performance
///
/// - **Scalar**: ~8 cycles (1 cycle per lzcnt instruction)
/// - **Auto-vectorized**: ~4 cycles (compiler may use SIMD for load/store)
///
/// This is a bottleneck for SIMD Gorilla/Chimp128 encoding because there's no
/// efficient way to vectorize leading_zeros. Future optimizations could use:
/// - Lookup tables (trade memory for speed)
/// - Approximate leading zeros (trade precision for speed)
/// - Different encoding that doesn't need leading zeros
#[inline]
pub fn count_leading_zeros_batch(values: &[u32; 8]) -> [u8; 8] {
    let mut result = [0u8; 8];

    // Process all 8 values (compiler may auto-vectorize this loop)
    for i in 0..8 {
        result[i] = values[i].leading_zeros() as u8;
    }

    result
}

/// Counts trailing zeros for a batch of 8 u32 values
///
/// Similar to `count_leading_zeros_batch`, this is scalar because x86 SIMD
/// does not have a native `tzcnt` vector instruction.
#[inline]
pub fn count_trailing_zeros_batch(values: &[u32; 8]) -> [u8; 8] {
    let mut result = [0u8; 8];

    for i in 0..8 {
        result[i] = values[i].trailing_zeros() as u8;
    }

    result
}

/// Feature detection utilities
pub mod features {
    /// Returns true if AVX2 is available (best performance)
    pub fn has_avx2() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("avx2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Returns true if SSE4.2 is available (good performance)
    pub fn has_sse42() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            is_x86_feature_detected!("sse4.2")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// Returns a human-readable string describing the available SIMD features
    pub fn simd_capabilities() -> &'static str {
        if has_avx2() {
            "AVX2 (8-wide)"
        } else if has_sse42() {
            "SSE4.2 (4-wide)"
        } else {
            "Scalar (1-wide)"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_detect_identical_all_same() {
        let current = [1.0f32; 8];
        let previous = [1.0f32; 8];

        let result = xor_detect_identical(&current, &previous);

        // All values identical → all bits set
        assert_eq!(result.identical_mask, 0b11111111);
        assert_eq!(result.xors, [0u32; 8]);
    }

    #[test]
    fn test_xor_detect_identical_all_different() {
        let current = [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let previous = [1.1f32, 2.1, 3.1, 4.1, 5.1, 6.1, 7.1, 8.1];

        let result = xor_detect_identical(&current, &previous);

        // All values different → no bits set
        assert_eq!(result.identical_mask, 0b00000000);

        // All XORs should be non-zero
        for &xor in &result.xors {
            assert_ne!(xor, 0);
        }
    }

    #[test]
    fn test_xor_detect_identical_mixed() {
        let current =  [1.0f32, 2.0, 3.0, 2.0, 5.0, 6.0, 7.0, 6.0];
        let previous = [1.0f32, 1.0, 3.0, 2.0, 4.0, 6.0, 8.0, 6.0];

        let result = xor_detect_identical(&current, &previous);

        // Index:  0    1    2    3    4    5    6    7
        // Match:  Y    N    Y    Y    N    Y    N    Y
        // Mask:   1    0    1    1    0    1    0    1
        // LSB first: bit 0=index 0, bit 7=index 7
        // Binary: 10101101 = 0xAD = 173

        assert_eq!(result.identical_mask, 0b10101101, "Bitmask mismatch");

        // Verify specific XOR values
        assert_eq!(result.xors[0], 0, "Index 0: 1.0 XOR 1.0 should be 0");
        assert_ne!(result.xors[1], 0, "Index 1: 2.0 XOR 1.0 should be non-zero");
        assert_eq!(result.xors[2], 0, "Index 2: 3.0 XOR 3.0 should be 0");
        assert_eq!(result.xors[3], 0, "Index 3: 2.0 XOR 2.0 should be 0");
        assert_ne!(result.xors[4], 0, "Index 4: 5.0 XOR 4.0 should be non-zero");
        assert_eq!(result.xors[5], 0, "Index 5: 6.0 XOR 6.0 should be 0");
        assert_ne!(result.xors[6], 0, "Index 6: 7.0 XOR 8.0 should be non-zero");
        assert_eq!(result.xors[7], 0, "Index 7: 6.0 XOR 6.0 should be 0");
    }

    #[test]
    fn test_count_leading_zeros() {
        let values = [
            0b00000000_00000000_00000000_00000001u32, // 31 leading zeros
            0b00000000_00000000_00000001_00000000u32, // 23 leading zeros
            0b00000001_00000000_00000000_00000000u32, // 7 leading zeros
            0b10000000_00000000_00000000_00000000u32, // 0 leading zeros
            0b00000000_00000000_00000000_00000000u32, // 32 leading zeros (all zeros)
            0b11111111_11111111_11111111_11111111u32, // 0 leading zeros (all ones)
            0b00000000_00000000_00001111_11111111u32, // 20 leading zeros
            0b00000000_10000000_00000000_00000000u32, // 8 leading zeros
        ];

        let result = count_leading_zeros_batch(&values);

        assert_eq!(result, [31, 23, 7, 0, 32, 0, 20, 8]);
    }

    #[test]
    fn test_count_trailing_zeros() {
        let values = [
            0b10000000_00000000_00000000_00000000u32, // 31 trailing zeros
            0b00000000_00000001_00000000_00000000u32, // 16 trailing zeros
            0b00000000_00000000_00000000_10000000u32, // 7 trailing zeros
            0b00000000_00000000_00000000_00000001u32, // 0 trailing zeros
            0b00000000_00000000_00000000_00000000u32, // 32 trailing zeros (all zeros)
            0b11111111_11111111_11111111_11111111u32, // 0 trailing zeros (all ones)
            0b11111111_11110000_00000000_00000000u32, // 20 trailing zeros
            0b00000000_00000001_00000000_00000000u32, // 16 trailing zeros
        ];

        let result = count_trailing_zeros_batch(&values);

        assert_eq!(result, [31, 16, 7, 0, 32, 0, 20, 16]);
    }

    #[test]
    fn test_simd_capabilities() {
        let caps = features::simd_capabilities();
        println!("SIMD capabilities: {}", caps);

        // Should be one of the three valid options
        assert!(
            caps == "AVX2 (8-wide)"
            || caps == "SSE4.2 (4-wide)"
            || caps == "Scalar (1-wide)"
        );
    }

    #[test]
    fn test_avx2_vs_scalar_consistency() {
        // Ensure AVX2 and scalar implementations produce identical results
        let current = [1.5f32, 2.3, 3.7, 4.1, 5.9, 6.2, 7.8, 8.4];
        let previous = [1.5f32, 2.0, 3.7, 4.5, 5.9, 6.0, 7.8, 8.0];

        let result_safe = xor_detect_identical(&current, &previous);
        let result_scalar = xor_detect_identical_scalar(&current, &previous);

        assert_eq!(result_safe.identical_mask, result_scalar.identical_mask);
        assert_eq!(result_safe.xors, result_scalar.xors);

        #[cfg(target_arch = "x86_64")]
        if is_x86_feature_detected!("avx2") {
            let result_avx2 = unsafe { xor_detect_identical_avx2(&current, &previous) };
            assert_eq!(result_avx2.identical_mask, result_scalar.identical_mask);
            assert_eq!(result_avx2.xors, result_scalar.xors);
        }
    }
}
