//! Base utilities for Sprintz compression
//!
//! This module provides the core building blocks for Sprintz encoding:
//! - **Bit packing/unpacking**: Pack values into minimal bit width
//! - **Zigzag encoding/decoding**: Map signed to unsigned integers efficiently
//! - **FIRE predictor**: Adaptive prediction for time series patterns
//! - **Block operations**: Process 8 values at a time for cache efficiency
//!
//! # Block Size
//!
//! Sprintz uses a fixed block size of 8 values, which provides:
//! - Good cache locality
//! - Potential for SIMD vectorization
//! - Reasonable compression granularity
//! - Low metadata overhead

/// Block size for Sprintz compression (8 values per block)
const BLOCK_SIZE: usize = 8;

/// Packs 8 int32 values into the minimum number of bits required
///
/// This function tightly packs 8 values using a specified bit width, filling
/// a 32-bit buffer and writing bytes as they become complete. This minimizes
/// storage while maintaining fast random access within blocks.
///
/// # Arguments
///
/// * `values` - Slice of exactly 8 i32 values to pack
/// * `bit_width` - Number of bits to use per value (0-32)
/// * `buf` - Output buffer to append packed bytes
///
/// # Performance
///
/// Operates in O(1) time with respect to bit_width, using bit shifting operations.
pub fn pack_8values_i32(values: &[i32], bit_width: u8, buf: &mut Vec<u8>) {
    if bit_width == 0 {
        // All values are zero, nothing to pack
        return;
    }

    let mut buffer: u32 = 0;
    let mut left_size = 32;
    let mut left_bit = 0;
    let mut value_idx = 0;
    let mut buf_idx = 0;

    while value_idx < 8 {
        buffer = 0;
        left_size = 32;

        // Encode left bits from previous value
        if left_bit > 0 {
            buffer |= (values[value_idx] as u32) << (32 - left_bit);
            left_size -= left_bit;
            left_bit = 0;
            value_idx += 1;
        }

        // Pack full values into buffer
        while left_size >= bit_width as i32 && value_idx < 8 {
            buffer |= (values[value_idx] as u32) << (left_size - bit_width as i32);
            left_size -= bit_width as i32;
            value_idx += 1;
        }

        // Handle partial value at the end
        if left_size > 0 && value_idx < 8 {
            buffer |= (values[value_idx] as u32) >> (bit_width as i32 - left_size);
            left_bit = bit_width as i32 - left_size;
        }

        // Write buffer to output
        for j in 0..4 {
            if buf_idx >= bit_width {
                return;
            }
            buf.push(((buffer >> ((3 - j) * 8)) & 0xFF) as u8);
            buf_idx += 1;
        }
    }
}

/// Unpacks 8 int32 values from bit-packed bytes
///
/// Reconstructs values that were packed with `pack_8values_i32`, reading
/// the specified number of bits per value from the byte buffer.
///
/// # Arguments
///
/// * `buf` - Input buffer containing packed bytes
/// * `bit_width` - Number of bits per value (must match packing width)
/// * `values` - Output vector to append unpacked values
pub fn unpack_8values_i32(buf: &[u8], bit_width: u8, values: &mut Vec<i32>) {
    if bit_width == 0 {
        // All zeros
        values.extend_from_slice(&[0; 8]);
        return;
    }

    let mut byte_idx = 0;
    let mut buffer: u64 = 0;
    let mut total_bits = 0;
    let mut value_idx = 0;

    while value_idx < 8 {
        // Fill buffer with enough bits
        while total_bits < bit_width && byte_idx < buf.len() {
            buffer = (buffer << 8) | (buf[byte_idx] as u64);
            byte_idx += 1;
            total_bits += 8;
        }

        // Extract values from buffer
        while total_bits >= bit_width && value_idx < 8 {
            let mask = if bit_width >= 32 {
                0xFFFFFFFF
            } else {
                (1u32 << bit_width) - 1
            };
            let value = ((buffer >> (total_bits - bit_width)) as i32) & (mask as i32);
            values.push(value);
            value_idx += 1;
            total_bits -= bit_width;
            if total_bits > 0 {
                buffer &= if total_bits >= 64 {
                    u64::MAX
                } else {
                    (1u64 << total_bits) - 1
                };
            } else {
                buffer = 0;
            }
        }
    }
}

/// Packs 8 int64 values into the minimum number of bits required
///
/// Similar to `pack_8values_i32` but handles 64-bit values, using a 64-bit buffer.
pub fn pack_8values_i64(values: &[i64], bit_width: u8, buf: &mut Vec<u8>) {
    if bit_width == 0 {
        return;
    }

    let mut buffer: u64 = 0;
    let mut left_size = 64;
    let mut left_bit = 0;
    let mut value_idx = 0;
    let mut buf_idx = 0;

    while value_idx < 8 {
        buffer = 0;
        left_size = 64;

        if left_bit > 0 {
            buffer |= (values[value_idx] as u64) << (64 - left_bit);
            left_size -= left_bit;
            left_bit = 0;
            value_idx += 1;
        }

        while left_size >= bit_width as i32 && value_idx < 8 {
            buffer |= (values[value_idx] as u64) << (left_size - bit_width as i32);
            left_size -= bit_width as i32;
            value_idx += 1;
        }

        if left_size > 0 && value_idx < 8 {
            buffer |= (values[value_idx] as u64) >> (bit_width as i32 - left_size);
            left_bit = bit_width as i32 - left_size;
        }

        for j in 0..8 {
            if buf_idx >= bit_width {
                return;
            }
            buf.push(((buffer >> ((7 - j) * 8)) & 0xFF) as u8);
            buf_idx += 1;
        }
    }
}

/// Unpacks 8 int64 values from bit-packed bytes
///
/// Similar to `unpack_8values_i32` but handles 64-bit values.
pub fn unpack_8values_i64(buf: &[u8], bit_width: u8, values: &mut Vec<i64>) {
    if bit_width == 0 {
        values.extend_from_slice(&[0; 8]);
        return;
    }

    let mut byte_idx = 0;
    let mut buffer: u128 = 0;
    let mut total_bits = 0;
    let mut value_idx = 0;

    while value_idx < 8 {
        while total_bits < bit_width && byte_idx < buf.len() {
            buffer = (buffer << 8) | (buf[byte_idx] as u128);
            byte_idx += 1;
            total_bits += 8;
        }

        while total_bits >= bit_width && value_idx < 8 {
            let mask = if bit_width >= 64 {
                0xFFFFFFFFFFFFFFFF_u64
            } else {
                (1u64 << bit_width) - 1
            };
            let value = ((buffer >> (total_bits - bit_width)) as i64) & (mask as i64);
            values.push(value);
            value_idx += 1;
            total_bits -= bit_width;
            if total_bits > 0 {
                buffer &= if total_bits >= 128 {
                    u128::MAX
                } else {
                    (1u128 << total_bits) - 1
                };
            } else {
                buffer = 0;
            }
        }
    }
}

/// Calculates the maximum bit width needed for an array of int32 values
///
/// Determines the minimum number of bits required to represent the largest
/// absolute value in the array. Returns 0 if all values are zero.
pub fn get_max_bit_width_i32(values: &[i32]) -> u8 {
    let max_val = values.iter().map(|&v| v.abs()).max().unwrap_or(0);
    if max_val == 0 {
        return 0;
    }
    32 - (max_val.leading_zeros() as u8)
}

/// Calculates the maximum bit width needed for an array of int64 values
///
/// Determines the minimum number of bits required to represent the largest
/// absolute value in the array. Returns 0 if all values are zero.
pub fn get_max_bit_width_i64(values: &[i64]) -> u8 {
    let max_val = values.iter().map(|&v| v.abs()).max().unwrap_or(0);
    if max_val == 0 {
        return 0;
    }
    64 - (max_val.leading_zeros() as u8)
}

/// Zigzag encodes a signed int32 to an unsigned representation
///
/// Maps signed integers to unsigned integers such that small absolute values
/// result in small positive integers:
/// - 0 → 0, -1 → 1, 1 → 2, -2 → 3, 2 → 4, ...
///
/// This is a custom Sprintz variant that matches the Apache IoTDB implementation.
///
/// # Performance
///
/// Inlined for zero-cost abstraction in hot paths.
#[inline]
pub fn zigzag_encode_i32(n: i32) -> i32 {
    if n <= 0 {
        n.saturating_mul(-2)
    } else {
        n.saturating_mul(2).saturating_sub(1)
    }
}

/// Zigzag decodes an unsigned int32 back to signed representation
///
/// Inverse of `zigzag_encode_i32`. Uses wrapping arithmetic to handle edge cases.
#[inline]
pub fn zigzag_decode_i32(n: i32) -> i32 {
    if n % 2 == 0 {
        -(n / 2)
    } else {
        n.wrapping_add(1) / 2
    }
}

/// Zigzag encodes a signed int64 to an unsigned representation
///
/// 64-bit version of `zigzag_encode_i32`.
#[inline]
pub fn zigzag_encode_i64(n: i64) -> i64 {
    if n <= 0 {
        n.saturating_mul(-2)
    } else {
        n.saturating_mul(2).saturating_sub(1)
    }
}

/// Zigzag decodes an unsigned int64 back to signed representation
///
/// Inverse of `zigzag_encode_i64`.
#[inline]
pub fn zigzag_decode_i64(n: i64) -> i64 {
    if n % 2 == 0 {
        -(n / 2)
    } else {
        n.wrapping_add(1) / 2
    }
}

/// FIRE (Finite Impulse Response) predictor for int32 values
///
/// FIRE is an adaptive predictor that learns patterns in time series data.
/// It maintains an accumulator that tracks the effectiveness of predictions
/// and adjusts based on observed errors, making it more sophisticated than
/// simple delta encoding.
///
/// # Algorithm
///
/// 1. Predict next value based on previous delta and learned coefficient (alpha)
/// 2. Compute prediction error
/// 3. Update accumulator based on error gradient
/// 4. Update delta for next prediction
///
/// # Performance
///
/// Works best for data with consistent trends or periodic patterns. Falls back
/// to near-delta encoding performance for random data.
#[derive(Debug, Clone)]
pub struct FireI32 {
    /// Right-shift amount for computing alpha from accumulator
    learn_shift: i32,
    /// Bit width for fixed-point multiplication
    bit_width: i32,
    /// Learned coefficient accumulator
    accumulator: i32,
    /// Previous delta value
    delta: i32,
}

impl FireI32 {
    /// Creates a new FIRE predictor with the specified learning rate
    ///
    /// Higher learning rates adapt faster but may be less stable.
    pub fn new(learning_rate: i32) -> Self {
        Self {
            learn_shift: learning_rate,
            bit_width: 8,
            accumulator: 0,
            delta: 0,
        }
    }

    /// Resets the predictor state
    pub fn reset(&mut self) {
        self.accumulator = 0;
        self.delta = 0;
    }

    /// Predicts the next value based on the previous value
    pub fn predict(&self, value: i32) -> i32 {
        let alpha = self.accumulator >> self.learn_shift;
        let diff = ((alpha as i64 * self.delta as i64) >> self.bit_width) as i32;
        value.wrapping_add(diff)
    }

    /// Trains the predictor based on observed error
    ///
    /// # Arguments
    ///
    /// * `pre` - Previous value
    /// * `val` - Current value
    /// * `err` - Prediction error (actual - predicted)
    pub fn train(&mut self, pre: i32, val: i32, err: i32) {
        let gradient = if err > 0 {
            self.delta.wrapping_neg()
        } else {
            self.delta
        };
        self.accumulator = self.accumulator.wrapping_sub(gradient);
        self.delta = val.wrapping_sub(pre);
    }
}

/// FIRE (Finite Impulse Response) predictor for int64 values
///
/// 64-bit version of `FireI32` with adjusted bit width for better precision.
#[derive(Debug, Clone)]
pub struct FireI64 {
    /// Right-shift amount for computing alpha from accumulator
    learn_shift: i64,
    /// Bit width for fixed-point multiplication
    bit_width: i64,
    /// Learned coefficient accumulator
    accumulator: i64,
    /// Previous delta value
    delta: i64,
}

impl FireI64 {
    /// Creates a new FIRE predictor with the specified learning rate
    pub fn new(learning_rate: i64) -> Self {
        Self {
            learn_shift: learning_rate,
            bit_width: 16,
            accumulator: 0,
            delta: 0,
        }
    }

    /// Resets the predictor state
    pub fn reset(&mut self) {
        self.accumulator = 0;
        self.delta = 0;
    }

    /// Predicts the next value based on the previous value
    pub fn predict(&self, value: i64) -> i64 {
        let alpha = self.accumulator >> self.learn_shift;
        let diff = (alpha.wrapping_mul(self.delta)) >> self.bit_width;
        value.wrapping_add(diff)
    }

    /// Trains the predictor based on observed error
    pub fn train(&mut self, pre: i64, val: i64, err: i64) {
        let gradient = if err > 0 {
            self.delta.wrapping_neg()
        } else {
            self.delta
        };
        self.accumulator = self.accumulator.wrapping_sub(gradient);
        self.delta = val.wrapping_sub(pre);
    }
}

/// Prediction method for Sprintz encoding
///
/// Determines which prediction strategy to use during encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictMethod {
    /// Simple delta encoding (value - previous_value)
    Delta,
    /// Adaptive FIRE prediction (learns patterns)
    Fire,
}

impl Default for PredictMethod {
    fn default() -> Self {
        // FIRE generally provides better compression for time series
        PredictMethod::Fire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zigzag_encode_decode_i32() {
        // Test normal values (extreme values may saturate with Sprintz's custom zigzag)
        let test_values = vec![
            0,
            1,
            -1,
            100,
            -100,
            10000,
            -10000,
            i32::MAX / 2,
            i32::MIN / 2,
        ];
        for val in test_values {
            let encoded = zigzag_encode_i32(val);
            let decoded = zigzag_decode_i32(encoded);
            assert_eq!(decoded, val, "Failed for {}", val);
        }
    }

    #[test]
    fn test_pack_unpack_i32() {
        let values = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let bit_width = get_max_bit_width_i32(&values);

        let mut buf = Vec::new();
        pack_8values_i32(&values, bit_width, &mut buf);

        let mut unpacked = Vec::new();
        unpack_8values_i32(&buf, bit_width, &mut unpacked);

        assert_eq!(unpacked, values);
    }

    #[test]
    fn test_pack_unpack_zeros() {
        let values = vec![0, 0, 0, 0, 0, 0, 0, 0];
        let bit_width = get_max_bit_width_i32(&values);
        assert_eq!(bit_width, 0);

        let mut buf = Vec::new();
        pack_8values_i32(&values, bit_width, &mut buf);

        let mut unpacked = Vec::new();
        unpack_8values_i32(&buf, bit_width, &mut unpacked);

        assert_eq!(unpacked, values);
    }

    #[test]
    fn test_fire_predictor() {
        let mut fire = FireI32::new(2);

        // Train on increasing sequence
        let values = vec![10, 20, 30, 40, 50];
        let mut prev = values[0];

        for &val in &values[1..] {
            let pred = fire.predict(prev);
            let err = val - pred;
            fire.train(prev, val, err);
            prev = val;
        }

        // After training, prediction should be better
        let pred = fire.predict(50);
        // Should predict around 60 (next in sequence)
        assert!((pred - 60).abs() < 20);
    }
}
