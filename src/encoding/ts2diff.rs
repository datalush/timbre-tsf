//! Second-order difference encoding (TS2DIFF) for time series data
//!
//! TS2DIFF compresses time series by encoding the delta-of-deltas rather than
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
//! - **Subsequent delta-of-deltas**: 8 bytes each (i64, little-endian)
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
//! use tsfile_rs::encoding::{Ts2DiffEncoder, Ts2DiffDecoder, Encoder, Decoder};
//! 
//! use tsfile_rs::common::TSDataType;
//!
//! let mut encoder = Ts2DiffEncoder::new(TSDataType::Int64);
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
//! let mut decoder = Ts2DiffDecoder::new(TSDataType::Int64);
//! let mut pos = 0;
//! for &expected in &values {
//!     assert_eq!(decoder.read_i64(&buffer, &mut pos).unwrap(), expected);
//! }
//! ```

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Second-order difference encoder for time series with regular patterns
///
/// Stores the first value and first delta explicitly, then encodes each
/// subsequent value as the difference from the expected value based on
/// the previous delta.
pub struct Ts2DiffEncoder {
    /// The first value in the sequence (stored unmodified)
    first_value: Option<i64>,
    /// The most recent value encoded
    previous_value: i64,
    /// The delta between the two most recent values
    previous_delta: i64,
}

impl Ts2DiffEncoder {
    /// Creates a new TS2DIFF encoder for the specified data type
    pub fn new(_data_type: TSDataType) -> Self {
        Self {
            first_value: None,
            previous_value: 0,
            previous_delta: 0,
        }
    }

    /// Encodes a value using second-order differencing
    ///
    /// The first value is stored directly, the second value's delta is stored,
    /// and all subsequent values are encoded as delta-of-delta.
    fn encode_value(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        if self.first_value.is_none() {
            self.first_value = Some(value);
            self.previous_value = value;
            out.write_i64::<LittleEndian>(value)?;
            return Ok(());
        }

        let delta = value - self.previous_value;

        if self.previous_delta == 0 && delta != 0 {
            self.previous_delta = delta;
            out.write_i64::<LittleEndian>(delta)?;
            self.previous_value = value;
            return Ok(());
        }

        let delta_of_delta = delta - self.previous_delta;
        out.write_i64::<LittleEndian>(delta_of_delta)?;

        self.previous_delta = delta;
        self.previous_value = value;
        Ok(())
    }
}

impl Encoder for Ts2DiffEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "TS2DIFF not supported for boolean".to_string(),
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
        Err(TsFileError::EncodingError(
            "TS2DIFF not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, _out: &mut Vec<u8>) -> Result<()> {
        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Ts2Diff
    }
}

/// Second-order difference decoder for time series
///
/// Reconstructs original values by applying delta-of-delta operations,
/// maintaining the previous value and delta to compute each new value.
pub struct Ts2DiffDecoder {
    /// The first value in the sequence
    first_value: Option<i64>,
    /// The most recently decoded value
    previous_value: i64,
    /// The delta between the two most recent values
    previous_delta: i64,
}

impl Ts2DiffDecoder {
    /// Creates a new TS2DIFF decoder for the specified data type
    pub fn new(_data_type: TSDataType) -> Self {
        Self {
            first_value: None,
            previous_value: 0,
            previous_delta: 0,
        }
    }

    /// Decodes a value using second-order differencing
    ///
    /// Reads the first value directly, then the first delta, and reconstructs
    /// all subsequent values by adding the computed delta to the previous value.
    fn decode_value(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        if self.first_value.is_none() {
            let value = (&input[*pos..]).read_i64::<LittleEndian>()?;
            *pos += 8;
            self.first_value = Some(value);
            self.previous_value = value;
            return Ok(value);
        }

        let delta_of_delta = (&input[*pos..]).read_i64::<LittleEndian>()?;
        *pos += 8;

        if self.previous_delta == 0 {
            self.previous_delta = delta_of_delta;
            self.previous_value += delta_of_delta;
            return Ok(self.previous_value);
        }

        let delta = self.previous_delta + delta_of_delta;
        let value = self.previous_value + delta;

        self.previous_delta = delta;
        self.previous_value = value;

        Ok(value)
    }
}

impl Decoder for Ts2DiffDecoder {
    fn read_bool(&mut self, _input: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TsFileError::DecodingError(
            "TS2DIFF not supported for boolean".to_string(),
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
        Err(TsFileError::DecodingError(
            "TS2DIFF not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        pos < input.len()
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Ts2Diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ts2diff_i64() {
        let mut encoder = Ts2DiffEncoder::new(TSDataType::Int64);
        let mut out = Vec::new();

        // Serie temporal regular: 1000, 1001, 1002, 1003 (delta=1, delta_of_delta=0)
        let values = vec![1000i64, 1001, 1002, 1003, 1004];
        for &val in &values {
            encoder.encode_i64(val, &mut out).unwrap();
        }

        let mut decoder = Ts2DiffDecoder::new(TSDataType::Int64);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_i64(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }
}
