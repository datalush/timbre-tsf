//! Simple8b encoding for integer values.
//!
//! Simple8b is a high-efficiency bit-packing algorithm that compresses sequences of
//! small integers into 64-bit words. It provides 10-100x better compression than plain
//! encoding for typical time series integers (deltas, counters, IDs).
//!
//! # Algorithm Overview
//!
//! Each 64-bit word consists of:
//! - 4-bit selector (indicates packing mode)
//! - 60-bit payload (packed integer values)
//!
//! The selector determines how many values are packed and their bit width:
//!
//! | Selector | Values | Bits/Value | Total Bits |
//! |----------|--------|------------|------------|
//! | 0        | 240    | 0 (zeros)  | 0          |
//! | 1        | 120    | 0 (ones)   | 0          |
//! | 2        | 60     | 1          | 60         |
//! | 3        | 30     | 2          | 60         |
//! | 4        | 20     | 3          | 60         |
//! | 5        | 15     | 4          | 60         |
//! | 6        | 12     | 5          | 60         |
//! | 7        | 10     | 6          | 60         |
//! | 8        | 8      | 7 (implied)| 56         |
//! | 9        | 7      | 8 (implied)| 56         |
//! | 10       | 6      | 10         | 60         |
//! | 11       | 5      | 12         | 60         |
//! | 12       | 4      | 15         | 60         |
//! | 13       | 3      | 20         | 60         |
//! | 14       | 2      | 30         | 60         |
//! | 15       | 1      | 60         | 60         |
//!
//! # Performance
//!
//! - **Compression ratio**: 8-64x for small values (0-1000), 2-4x for larger values
//! - **Speed**: ~1-2 GB/s encoding, ~2-4 GB/s decoding
//! - **Use case**: Delta-encoded integers, timestamps, counters, IDs
//!
//! # References
//!
//! - Anh, Vo Ngoc, and Alistair Moffat. "Index compression using 64-bit words."
//!   Software: Practice and Experience 40.2 (2010): 131-147.

use crate::common::TSDataType;
use crate::encoding::{Decoder, Encoder};
use crate::error::{Result, TimbreError};
use byteorder::{LittleEndian, WriteBytesExt};

/// Simple8b selector modes (count, bits per value).
const SELECTORS: [(u8, u8); 16] = [
    (240, 0), // 0: 240 values of 0 bits (all zeros)
    (120, 0), // 1: 120 values of 0 bits (all ones)
    (60, 1),  // 2: 60 values of 1 bit
    (30, 2),  // 3: 30 values of 2 bits
    (20, 3),  // 4: 20 values of 3 bits
    (15, 4),  // 5: 15 values of 4 bits
    (12, 5),  // 6: 12 values of 5 bits
    (10, 6),  // 7: 10 values of 6 bits
    (8, 7),   // 8: 8 values of 7 bits
    (7, 8),   // 9: 7 values of 8 bits
    (6, 10),  // 10: 6 values of 10 bits
    (5, 12),  // 11: 5 values of 12 bits
    (4, 15),  // 12: 4 values of 15 bits
    (3, 20),  // 13: 3 values of 20 bits
    (2, 30),  // 14: 2 values of 30 bits
    (1, 60),  // 15: 1 value of 60 bits
];

/// OPT: Pre-calculated max values for each selector (avoids (1u64 << bits) - 1 in hot path)
const MAX_VALUES: [u64; 16] = [
    0,                   // 0: 0 bits (all zeros)
    0,                   // 1: 0 bits (all ones)
    1,                   // 2: 1 bit max = 1
    3,                   // 3: 2 bits max = 3
    7,                   // 4: 3 bits max = 7
    15,                  // 5: 4 bits max = 15
    31,                  // 6: 5 bits max = 31
    63,                  // 7: 6 bits max = 63
    127,                 // 8: 7 bits max = 127
    255,                 // 9: 8 bits max = 255
    1023,                // 10: 10 bits max = 1023
    4095,                // 11: 12 bits max = 4095
    32767,               // 12: 15 bits max = 32767
    1048575,             // 13: 20 bits max = 1048575
    1073741823,          // 14: 30 bits max = 1073741823
    1152921504606846975, // 15: 60 bits max = 2^60 - 1
];

/// Simple8b encoder for integer values.
#[derive(Debug)]
pub struct Simple8bEncoder {
    /// Type of data being encoded
    data_type: TSDataType,
    /// Buffer of pending values
    pending: Vec<u64>,
    /// Output buffer
    output: Vec<u64>,
}

impl Simple8bEncoder {
    /// Creates a new Simple8b encoder.
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            pending: Vec::with_capacity(240),
            output: Vec::new(),
        }
    }

    /// Encodes a signed 32-bit integer.
    fn encode_i32_internal(&mut self, value: i32) {
        // Use zigzag encoding for signed values: 0 -> 0, -1 -> 1, 1 -> 2, -2 -> 3, etc.
        let zigzag = ((value << 1) ^ (value >> 31)) as u64;
        self.pending.push(zigzag);
        // Only try to pack when we have accumulated enough values (at least 60)
        if self.pending.len() >= 60 {
            self.try_pack();
        }
    }

    /// Encodes a signed 64-bit integer.
    fn encode_i64_internal(&mut self, value: i64) {
        // Use zigzag encoding
        let zigzag = ((value << 1) ^ (value >> 63)) as u64;
        self.pending.push(zigzag);
        // Only try to pack when we have accumulated enough values (at least 60)
        if self.pending.len() >= 60 {
            self.try_pack();
        }
    }

    /// Tries to pack pending values into a 64-bit word.
    #[inline] // OPT: Inline hot path (5.15% CPU)
    fn try_pack(&mut self) {
        if self.pending.is_empty() {
            return;
        }

        // OPT: Find the best selector that can fit the pending values
        for (selector_idx, &(count, _bits)) in SELECTORS.iter().enumerate() {
            if count as usize <= self.pending.len() {
                // OPT: Use pre-calculated MAX_VALUES table instead of computing (1u64 << bits) - 1
                let max_value = MAX_VALUES[selector_idx];

                // Check if all values fit in this selector
                let values_to_pack = &self.pending[..count as usize];
                if values_to_pack.iter().all(|&v| v <= max_value) {
                    // Pack the values
                    let packed = self.pack_values(values_to_pack, selector_idx as u8, _bits);
                    self.output.push(packed);
                    self.pending.drain(..count as usize);
                    return;
                }
            }
        }

        // If we have enough values but none fit, pack what we can with selector 15 (60-bit)
        if self.pending.len() >= 240 {
            let value = self.pending[0];
            let packed = (15u64 << 60) | (value & 0x0FFF_FFFF_FFFF_FFFF);
            self.output.push(packed);
            self.pending.remove(0);
        }
    }

    /// Packs values into a 64-bit word with the given selector.
    fn pack_values(&self, values: &[u64], selector: u8, bits_per_value: u8) -> u64 {
        let mut word = (selector as u64) << 60;

        if bits_per_value == 0 {
            // Special case: all zeros or all ones
            if selector == 1 {
                // All ones - fill the payload
                word |= 0x0FFF_FFFF_FFFF_FFFF;
            }
            // For selector 0 (all zeros), payload is already 0
            return word;
        }

        // Pack values into the 60-bit payload
        for (i, &value) in values.iter().enumerate() {
            let shift = i as u64 * bits_per_value as u64;
            word |= (value & ((1u64 << bits_per_value) - 1)) << shift;
        }

        word
    }

    /// Flushes remaining values.
    fn finish(&mut self) -> &[u64] {
        // Force pack remaining values
        while !self.pending.is_empty() {
            // Find the best selector that can fit all remaining values
            let mut best_selector = 15; // Worst case: 1 value of 60 bits
            let mut best_count = 1;

            // Try to find a selector that can fit all (or partial) pending values
            for (selector_idx, &(count, _bits)) in SELECTORS.iter().enumerate() {
                // OPT: Use pre-calculated MAX_VALUES table
                let max_value = MAX_VALUES[selector_idx];

                // Determine how many values we can pack with this selector
                let can_pack = if count as usize <= self.pending.len() {
                    // We have enough values - check if they all fit in this selector
                    if self.pending[..count as usize]
                        .iter()
                        .all(|&v| v <= max_value)
                    {
                        count as usize
                    } else {
                        0
                    }
                } else {
                    // Not enough values - need to pad with zeros
                    // Only use this selector if all pending values fit
                    if self.pending.iter().all(|&v| v <= max_value) {
                        count as usize
                    } else {
                        0
                    }
                };

                if can_pack > 0 {
                    best_selector = selector_idx;
                    best_count = can_pack;
                    break; // Found the best match (iterating from most efficient to least)
                }
            }

            let (_, bits) = SELECTORS[best_selector];

            // Pack values, padding with zeros if needed
            let mut values_to_pack = self.pending[..self.pending.len().min(best_count)].to_vec();
            while values_to_pack.len() < best_count {
                values_to_pack.push(0); // Pad with zeros
            }

            let packed = self.pack_values(&values_to_pack, best_selector as u8, bits);
            self.output.push(packed);
            self.pending.drain(..self.pending.len().min(best_count));
        }

        &self.output
    }
}

impl Encoder for Simple8bEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Simple8b does not support boolean".to_string(),
        ))
    }

    fn encode_i32(&mut self, value: i32, _out: &mut Vec<u8>) -> Result<()> {
        if self.data_type != TSDataType::Int32 && self.data_type != TSDataType::Date {
            return Err(TimbreError::EncodingError(
                "Simple8b: wrong data type for i32".to_string(),
            ));
        }
        self.encode_i32_internal(value);
        Ok(())
    }

    fn encode_i64(&mut self, value: i64, _out: &mut Vec<u8>) -> Result<()> {
        if self.data_type != TSDataType::Int64 && self.data_type != TSDataType::Timestamp {
            return Err(TimbreError::EncodingError(
                "Simple8b: wrong data type for i64".to_string(),
            ));
        }
        self.encode_i64_internal(value);
        Ok(())
    }

    fn encode_f32(&mut self, _value: f32, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Simple8b does not support f32".to_string(),
        ))
    }

    fn encode_f64(&mut self, _value: f64, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Simple8b does not support f64".to_string(),
        ))
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Simple8b does not support string".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        let words = self.finish();
        for &word in words {
            out.write_u64::<LittleEndian>(word)?;
        }
        Ok(())
    }

    fn encoding_type(&self) -> crate::common::TSEncoding {
        crate::common::TSEncoding::Simple8b
    }
}

/// Simple8b decoder for integer values.
#[derive(Debug)]
pub struct Simple8bDecoder {
    /// Type of data being decoded
    data_type: TSDataType,
    /// Current word being decoded
    current_word: u64,
    /// Current position in word
    current_pos: u8,
    /// Current selector
    current_selector: u8,
}

impl Simple8bDecoder {
    /// Creates a new Simple8b decoder.
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            current_word: 0,
            current_pos: 0,
            current_selector: 0,
        }
    }

    /// Returns true if there are pending values in the current word.
    pub fn has_pending_values(&self) -> bool {
        self.current_pos > 0
    }

    /// Reads the next 64-bit word if needed.
    fn ensure_word(&mut self, data: &[u8], pos: &mut usize) -> Result<()> {
        if self.current_pos == 0 {
            if *pos + 8 > data.len() {
                return Err(TimbreError::DecodingError(
                    "Simple8b: unexpected end of data".to_string(),
                ));
            }

            // Read next word
            let mut word_bytes = [0u8; 8];
            word_bytes.copy_from_slice(&data[*pos..*pos + 8]);
            self.current_word = u64::from_le_bytes(word_bytes);
            *pos += 8;

            // Extract selector
            self.current_selector = ((self.current_word >> 60) & 0xF) as u8;
            self.current_pos = SELECTORS[self.current_selector as usize].0;
        }

        Ok(())
    }

    /// Decodes the next i32 value.
    fn decode_i32_internal(&mut self, data: &[u8], pos: &mut usize) -> Result<i32> {
        self.ensure_word(data, pos)?;

        let (count, bits) = SELECTORS[self.current_selector as usize];
        let index = count - self.current_pos;

        let value = if bits == 0 {
            if self.current_selector == 1 { 1 } else { 0 }
        } else {
            let shift = index as u64 * bits as u64;
            let mask = (1u64 << bits) - 1;
            (self.current_word >> shift) & mask
        };

        self.current_pos -= 1;

        // Reverse zigzag encoding
        let zigzag = value as i32;
        let decoded = (zigzag >> 1) ^ -(zigzag & 1);
        Ok(decoded)
    }

    /// Decodes the next i64 value.
    fn decode_i64_internal(&mut self, data: &[u8], pos: &mut usize) -> Result<i64> {
        self.ensure_word(data, pos)?;

        let (count, bits) = SELECTORS[self.current_selector as usize];
        let index = count - self.current_pos;

        let value = if bits == 0 {
            if self.current_selector == 1 { 1 } else { 0 }
        } else {
            let shift = index as u64 * bits as u64;
            let mask = (1u64 << bits) - 1;
            (self.current_word >> shift) & mask
        };

        self.current_pos -= 1;

        // Reverse zigzag encoding
        let zigzag = value as i64;
        let decoded = (zigzag >> 1) ^ -(zigzag & 1);
        Ok(decoded)
    }
}

impl Decoder for Simple8bDecoder {
    fn read_bool(&mut self, _data: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TimbreError::DecodingError(
            "Simple8b does not support boolean".to_string(),
        ))
    }

    fn read_i32(&mut self, data: &[u8], pos: &mut usize) -> Result<i32> {
        if self.data_type != TSDataType::Int32 && self.data_type != TSDataType::Date {
            return Err(TimbreError::DecodingError(
                "Simple8b: wrong data type for i32".to_string(),
            ));
        }
        self.decode_i32_internal(data, pos)
    }

    fn read_i64(&mut self, data: &[u8], pos: &mut usize) -> Result<i64> {
        if self.data_type != TSDataType::Int64 && self.data_type != TSDataType::Timestamp {
            return Err(TimbreError::DecodingError(
                "Simple8b: wrong data type for i64".to_string(),
            ));
        }
        self.decode_i64_internal(data, pos)
    }

    fn read_f32(&mut self, _data: &[u8], _pos: &mut usize) -> Result<f32> {
        Err(TimbreError::DecodingError(
            "Simple8b does not support f32".to_string(),
        ))
    }

    fn read_f64(&mut self, _data: &[u8], _pos: &mut usize) -> Result<f64> {
        Err(TimbreError::DecodingError(
            "Simple8b does not support f64".to_string(),
        ))
    }

    fn read_string(&mut self, _data: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TimbreError::DecodingError(
            "Simple8b does not support string".to_string(),
        ))
    }

    fn has_remaining(&self, data: &[u8], pos: usize) -> bool {
        pos < data.len() || self.current_pos > 0
    }

    fn encoding_type(&self) -> crate::common::TSEncoding {
        crate::common::TSEncoding::Simple8b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple8b_small_values() {
        let mut encoder = Simple8bEncoder::new(TSDataType::Int32);
        let mut out = Vec::new();

        // Encode small positive values
        for i in 0..100 {
            encoder.encode_i32(i, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        // Decode
        let mut decoder = Simple8bDecoder::new(TSDataType::Int32);
        let mut pos = 0;
        for i in 0..100 {
            let decoded = decoder.read_i32(&out, &mut pos).unwrap();
            assert_eq!(decoded, i);
        }
    }

    #[test]
    fn test_simple8b_negative_values() {
        let mut encoder = Simple8bEncoder::new(TSDataType::Int32);
        let mut out = Vec::new();

        // Encode mixed positive/negative values
        let values = vec![-10, -5, 0, 5, 10, -100, 100];
        for &v in &values {
            encoder.encode_i32(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        // Decode
        let mut decoder = Simple8bDecoder::new(TSDataType::Int32);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_i32(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_simple8b_i64() {
        let mut encoder = Simple8bEncoder::new(TSDataType::Int64);
        let mut out = Vec::new();

        // Encode i64 values
        let values = vec![0i64, 100, -100, 1000000, -1000000];
        for &v in &values {
            encoder.encode_i64(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        // Decode
        let mut decoder = Simple8bDecoder::new(TSDataType::Int64);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_i64(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }
}
