//! Run-Length Encoding (RLE) implementation
//!
//! RLE is a simple lossless compression algorithm that encodes consecutive
//! identical values as a single value plus a count. This is extremely effective
//! for data with long sequences of repeated values.
//!
//! # Format Specification
//!
//! Each run is stored as:
//! - **Value**: 8 bytes (little-endian i64)
//! - **Count**: 4 bytes (little-endian i32)
//!
//! Boolean and integer types are cast to i64 for uniform encoding.
//!
//! # Performance Characteristics
//!
//! - **Encoding**: O(1) per value with buffering
//! - **Decoding**: O(1) amortized with run caching
//! - **Compression**: Excellent for repetitive data (e.g., constant sensor readings)
//! - **Worst case**: 12 bytes per unique value (3x overhead vs plain for all unique values)
//! - **Best case**: 12 bytes total for millions of identical values
//!
//! # Use Cases
//!
//! - Sensor data with long periods of constant readings
//! - Status flags that rarely change
//! - Fill values or padding
//! - Binary on/off states
//!
//! # Not Recommended For
//!
//! - Floating-point values (bit-level differences prevent runs)
//! - High-cardinality data
//! - Random or uniformly distributed data
//!
//! # Example
//!
//! ```
//! use tsfile_rs::encoding::rle::{RleEncoder, RleDecoder};
//! use tsfile_rs::encoding::{Encoder, Decoder};
//! use tsfile_rs::common::TSDataType;
//!
//! let mut encoder = RleEncoder::new(TSDataType::Int32);
//! let mut buffer = Vec::new();
//!
//! // Encode 1000 identical values
//! for _ in 0..1000 {
//!     encoder.encode_i32(42, &mut buffer).unwrap();
//! }
//! encoder.flush(&mut buffer).unwrap();
//!
//! // Only 12 bytes stored: value(8) + count(4)
//! assert_eq!(buffer.len(), 12);
//!
//! let mut decoder = RleDecoder::new(TSDataType::Int32);
//! let mut pos = 0;
//! for _ in 0..1000 {
//!     assert_eq!(decoder.read_i32(&buffer, &mut pos).unwrap(), 42);
//! }
//! ```

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Run-Length Encoding encoder for integer and boolean types
///
/// Maintains state to track the current run of identical values and writes
/// runs as (value, count) pairs when a new value is encountered or flush is called.
pub struct RleEncoder {
    data_type: TSDataType,
    /// The value of the current run (None if no values encoded yet)
    previous_value: Option<i64>,
    /// Number of consecutive occurrences of previous_value
    run_length: i32,
}

impl RleEncoder {
    /// Creates a new RLE encoder for the specified data type
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            previous_value: None,
            run_length: 0,
        }
    }

    /// Encodes a value by extending the current run or starting a new one
    ///
    /// When a new value differs from the previous value, the current run is
    /// written to the output buffer and a new run begins.
    fn encode_value(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        match self.previous_value {
            None => {
                self.previous_value = Some(value);
                self.run_length = 1;
            }
            Some(prev) if prev == value => {
                self.run_length += 1;
            }
            Some(prev) => {
                out.write_i64::<LittleEndian>(prev)?;
                out.write_i32::<LittleEndian>(self.run_length)?;
                self.previous_value = Some(value);
                self.run_length = 1;
            }
        }
        Ok(())
    }
}

impl Encoder for RleEncoder {
    fn encode_bool(&mut self, value: bool, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as i64, out)
    }

    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as i64, out)
    }

    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value, out)
    }

    fn encode_f32(&mut self, _value: f32, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "RLE not recommended for floats".to_string(),
        ))
    }

    fn encode_f64(&mut self, _value: f64, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "RLE not recommended for doubles".to_string(),
        ))
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "RLE not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        if let Some(value) = self.previous_value {
            out.write_i64::<LittleEndian>(value)?;
            out.write_i32::<LittleEndian>(self.run_length)?;
            self.previous_value = None;
            self.run_length = 0;
        }
        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Rle
    }
}

/// Run-Length Encoding decoder for integer and boolean types
///
/// Maintains state to cache the current run being decoded, allowing O(1)
/// amortized decoding performance by reading each (value, count) pair once
/// and returning the value multiple times.
pub struct RleDecoder {
    data_type: TSDataType,
    /// The value of the current run being decoded
    current_value: Option<i64>,
    /// Number of values remaining in the current run
    remaining: i32,
}

impl RleDecoder {
    /// Creates a new RLE decoder for the specified data type
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            current_value: None,
            remaining: 0,
        }
    }

    /// Decodes the next value from the current run or reads a new run
    ///
    /// If values remain in the current run, returns the cached value immediately.
    /// Otherwise, reads the next (value, count) pair from the input buffer.
    fn decode_value(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        if self.remaining > 0 {
            self.remaining -= 1;
            return Ok(self.current_value.unwrap());
        }

        if *pos + 12 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }

        let value = (&input[*pos..]).read_i64::<LittleEndian>()?;
        *pos += 8;
        let count = (&input[*pos..]).read_i32::<LittleEndian>()?;
        *pos += 4;

        self.current_value = Some(value);
        self.remaining = count - 1;

        Ok(value)
    }
}

impl Decoder for RleDecoder {
    fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool> {
        Ok(self.decode_value(input, pos)? != 0)
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        Ok(self.decode_value(input, pos)? as i32)
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        self.decode_value(input, pos)
    }

    fn read_f32(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f32> {
        Err(TsFileError::DecodingError(
            "RLE not recommended for floats".to_string(),
        ))
    }

    fn read_f64(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f64> {
        Err(TsFileError::DecodingError(
            "RLE not recommended for doubles".to_string(),
        ))
    }

    fn read_string(&mut self, _input: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TsFileError::DecodingError(
            "RLE not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        self.remaining > 0 || pos < input.len()
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Rle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rle_i32() {
        let mut encoder = RleEncoder::new(TSDataType::Int32);
        let mut out = Vec::new();

        // Valores repetidos: 5, 5, 5, 10, 10
        encoder.encode_i32(5, &mut out).unwrap();
        encoder.encode_i32(5, &mut out).unwrap();
        encoder.encode_i32(5, &mut out).unwrap();
        encoder.encode_i32(10, &mut out).unwrap();
        encoder.encode_i32(10, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = RleDecoder::new(TSDataType::Int32);
        let mut pos = 0;
        assert_eq!(decoder.read_i32(&out, &mut pos).unwrap(), 5);
        assert_eq!(decoder.read_i32(&out, &mut pos).unwrap(), 5);
        assert_eq!(decoder.read_i32(&out, &mut pos).unwrap(), 5);
        assert_eq!(decoder.read_i32(&out, &mut pos).unwrap(), 10);
        assert_eq!(decoder.read_i32(&out, &mut pos).unwrap(), 10);
    }
}
