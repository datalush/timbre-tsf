//! Delta-of-Delta encoding for time series data
//!
//! Delta-of-Delta compresses time series by encoding the delta-of-deltas rather than
//! the values themselves. This is highly effective for data with consistent
//! trends or regular sampling intervals.
//!
//! # Encoding Strategy
//!
//! For a sequence of values `[v0, v1, v2, v3, ...]`:
//!
//! 1. Store first value `v0` directly
//! 2. Store first delta `d1 = v1 - v0`
//! 3. For subsequent values, store delta-of-delta: `dd = (vi - vi-1) - (vi-1 - vi-2)`
//!
//! # Format Specification
//!
//! - **First value**: 8 bytes (i64, little-endian)
//! - **First delta**: 8 bytes (i64, little-endian)
//! - **Subsequent delta-of-deltas**: Compressed with Simple8b (1-8 bytes average)
//!
//! # Performance Characteristics
//!
//! - **Encoding**: O(1) per value
//! - **Decoding**: O(1) per value
//! - **Compression ratio**: Excellent for regular patterns (often near-zero deltas)
//! - **Worst case**: Equal size to plain encoding for random data
//! - **Best case**: Near-zero storage for linear trends
//!
//! # Ideal Use Cases
//!
//! - Monotonically increasing timestamps with regular intervals
//! - Sensor data with consistent trends
//! - Counter values that increment steadily
//! - Temperature/pressure readings with slow, smooth changes
//!
//! # Example
//!
//! ```
//! use timbre_tsf::encoding::{DeltaOfDeltaEncoder, DeltaOfDeltaDecoder, Encoder, Decoder};
//!
//! use timbre_tsf::common::TSDataType;
//!
//! let mut encoder = DeltaOfDeltaEncoder::new(TSDataType::Int64);
//! let mut buffer = Vec::new();
//!
//! // Regular sequence: 1000, 1010, 1020, 1030 (constant delta of 10)
//! let values = vec![1000i64, 1010, 1020, 1030];
//! for &v in &values {
//!     encoder.encode_i64(v, &mut buffer).unwrap();
//! }
//! encoder.flush(&mut buffer).unwrap();
//!
//! // Delta-of-deltas are all zero after the first two values
//! let mut decoder = DeltaOfDeltaDecoder::new(TSDataType::Int64);
//! let mut pos = 0;
//! for &expected in &values {
//!     assert_eq!(decoder.read_i64(&buffer, &mut pos).unwrap(), expected);
//! }
//! ```

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::encoding::simple8b::{Simple8bDecoder, Simple8bEncoder};
use crate::error::{Result, TimbreError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Delta-of-Delta encoder for time series with regular patterns
///
/// Stores the first value and first delta explicitly, then encodes each
/// subsequent value as the difference from the expected value based on
/// the previous delta. Delta-of-deltas are compressed using Simple8b.
pub struct DeltaOfDeltaEncoder {
    /// The most recent value encoded
    previous_value: i64,
    /// The delta between the two most recent values
    previous_delta: i64,
    /// Simple8b encoder for delta-of-deltas
    simple8b: Simple8bEncoder,
    /// Number of values encoded (OPT: replaces Option + bool with single counter)
    /// 0 = first value, 1 = first delta, 2+ = delta-of-delta
    count: u32,
}

impl DeltaOfDeltaEncoder {
    /// Creates a new Delta-of-Delta encoder for the specified data type
    pub fn new(_data_type: TSDataType) -> Self {
        // Delta-of-deltas are always encoded as i64, regardless of input data type
        Self {
            previous_value: 0,
            previous_delta: 0,
            simple8b: Simple8bEncoder::new(TSDataType::Int64),
            count: 0, // OPT: Single counter replaces Option + bool
        }
    }

    /// Encodes a value using second-order differencing with Simple8b compression
    ///
    /// Format:
    /// - First value: stored directly as i64 (8 bytes)
    /// - First delta: stored directly as i64 (8 bytes)
    /// - Subsequent delta-of-deltas: compressed with Simple8b
    ///
    /// The first value is stored directly, the second value's delta is stored,
    /// and all subsequent values are encoded as delta-of-delta compressed with Simple8b.
    #[inline(always)] // OPT: Hot path - inline to avoid call overhead (12.76% CPU)
    fn encode_value(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        // OPT: Use match on counter instead of Option + bool (eliminates 2 branches)
        match self.count {
            0 => {
                // First value: write directly
                self.previous_value = value;
                self.count = 1;
                out.write_i64::<LittleEndian>(value)?;
            }
            1 => {
                // First delta: write directly
                let delta = value - self.previous_value;
                self.previous_delta = delta;
                self.previous_value = value;
                self.count = 2;
                out.write_i64::<LittleEndian>(delta)?;
            }
            _ => {
                // Subsequent values: encode delta-of-delta with Simple8b
                let delta = value - self.previous_value;
                let delta_of_delta = delta - self.previous_delta;

                // Simple8b will accumulate this value (no immediate write to out)
                self.simple8b.encode_i64(delta_of_delta, out)?;

                self.previous_delta = delta;
                self.previous_value = value;
                self.count += 1;
            }
        }
        Ok(())
    }

    /// Resets the encoder state for reuse.
    ///
    /// This allows the encoder to be reused for encoding a new sequence of values
    /// without needing to allocate a new encoder instance.
    ///
    /// # Performance
    ///
    /// Reusing encoders avoids:
    /// - Heap allocation of new encoder (~100-200ns)
    /// - Simple8b encoder allocation (~50-100ns)
    /// - Potential memory fragmentation
    ///
    /// For 8 mini-blocks per page, this saves ~1-2μs per page.
    pub fn reset(&mut self) {
        self.previous_value = 0;
        self.previous_delta = 0;
        self.count = 0; // OPT: Single counter reset
        // Note: We create a new Simple8bEncoder since it doesn't have reset()
        // This is still faster than creating the entire DeltaOfDeltaEncoder
        self.simple8b = Simple8bEncoder::new(TSDataType::Int64);
    }
}

impl Encoder for DeltaOfDeltaEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "DELTA_OF_DELTA not supported for boolean".to_string(),
        ))
    }

    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as i64, out)
    }

    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value, out)
    }

    fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value.to_bits() as i64, out)
    }

    fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value.to_bits() as i64, out)
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "DELTA_OF_DELTA not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        // Flush any remaining Simple8b-encoded delta-of-deltas
        self.simple8b.flush(out)?;
        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::DeltaOfDelta
    }
}

/// Delta-of-Delta decoder for time series
///
/// Reconstructs original values by applying delta-of-delta operations,
/// maintaining the previous value and delta to compute each new value.
/// Delta-of-deltas are decompressed using Simple8b.
pub struct DeltaOfDeltaDecoder {
    /// The most recently decoded value
    previous_value: i64,
    /// The delta between the two most recent values
    previous_delta: i64,
    /// Simple8b decoder for delta-of-deltas
    simple8b: Simple8bDecoder,
    /// Number of values decoded (OPT: replaces Option + bool)
    count: u32,
}

impl DeltaOfDeltaDecoder {
    /// Creates a new Delta-of-Delta decoder for the specified data type
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            previous_value: 0,
            previous_delta: 0,
            simple8b: Simple8bDecoder::new(data_type),
            count: 0, // OPT: Single counter replaces Option + bool
        }
    }

    /// Decodes a value using second-order differencing with Simple8b decompression
    ///
    /// Format:
    /// - First value: read directly as i64 (8 bytes)
    /// - First delta: read directly as i64 (8 bytes)
    /// - Subsequent delta-of-deltas: decompressed with Simple8b
    ///
    /// Reads the first value directly, then the first delta, and reconstructs
    /// all subsequent values by adding the computed delta to the previous value.
    #[inline(always)] // OPT: Inline for consistency with encoder
    fn decode_value(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        // OPT: Use match on counter instead of Option + bool
        match self.count {
            0 => {
                // First value: read directly
                let value = (&input[*pos..]).read_i64::<LittleEndian>()?;
                *pos += 8;
                self.previous_value = value;
                self.count = 1;
                Ok(value)
            }
            1 => {
                // First delta: read directly
                let delta = (&input[*pos..]).read_i64::<LittleEndian>()?;
                *pos += 8;
                self.previous_delta = delta;
                self.previous_value += delta;
                self.count = 2;
                Ok(self.previous_value)
            }
            _ => {
                // Subsequent values: decode delta-of-delta with Simple8b
                let delta_of_delta = self.simple8b.read_i64(input, pos)?;

                let delta = self.previous_delta + delta_of_delta;
                let value = self.previous_value + delta;

                self.previous_delta = delta;
                self.previous_value = value;
                self.count += 1;

                Ok(value)
            }
        }
    }
}

impl Decoder for DeltaOfDeltaDecoder {
    fn read_bool(&mut self, _input: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TimbreError::DecodingError(
            "DELTA_OF_DELTA not supported for boolean".to_string(),
        ))
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        Ok(self.decode_value(input, pos)? as i32)
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        self.decode_value(input, pos)
    }

    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
        let bits = self.decode_value(input, pos)? as u32;
        Ok(f32::from_bits(bits))
    }

    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64> {
        let bits = self.decode_value(input, pos)? as u64;
        Ok(f64::from_bits(bits))
    }

    fn read_string(&mut self, _input: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TimbreError::DecodingError(
            "DELTA_OF_DELTA not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        // Check if there's more data to read OR if simple8b has pending values in current word
        pos < input.len() || self.simple8b.has_pending_values()
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::DeltaOfDelta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_of_delta_i64() {
        let mut encoder = DeltaOfDeltaEncoder::new(TSDataType::Int64);
        let mut out = Vec::new();

        // Regular time series: 1000, 1001, 1002, 1003 (delta=1, delta_of_delta=0)
        let values = vec![1000i64, 1001, 1002, 1003, 1004];
        for &val in &values {
            encoder.encode_i64(val, &mut out).unwrap();
        }

        // Flush to write remaining Simple8b-encoded delta-of-deltas
        encoder.flush(&mut out).unwrap();

        let mut decoder = DeltaOfDeltaDecoder::new(TSDataType::Int64);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_i64(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }
}
