//! Plain encoding implementation
//!
//! Plain encoding stores values in their native binary format without compression.
//! This is the simplest and fastest encoding method, providing O(1) encoding and
//! decoding performance.
//!
//! # Format Specification
//!
//! - **Boolean**: 1 byte (0 or 1)
//! - **Int32**: 4 bytes (little-endian)
//! - **Int64**: 8 bytes (little-endian)
//! - **Float**: 4 bytes (IEEE 754 single precision, little-endian)
//! - **Double**: 8 bytes (IEEE 754 double precision, little-endian)
//! - **String**: 4-byte length prefix + UTF-8 bytes
//!
//! # Performance Characteristics
//!
//! - **Encoding**: O(1) per value, minimal CPU overhead
//! - **Decoding**: O(1) per value, direct memory copy
//! - **Compression**: None, largest storage footprint
//! - **Use case**: Best for random-access patterns or already compressed data
//!
//! # Example
//!
//! ```
//! use tsfile_rs::encoding::{PlainEncoder, PlainDecoder, Encoder, Decoder};
//! 
//! use tsfile_rs::common::TSDataType;
//!
//! let mut encoder = PlainEncoder::new(TSDataType::Int32);
//! let mut buffer = Vec::new();
//!
//! encoder.encode_i32(42, &mut buffer).unwrap();
//! encoder.flush(&mut buffer).unwrap();
//!
//! let mut decoder = PlainDecoder::new(TSDataType::Int32);
//! let mut pos = 0;
//! assert_eq!(decoder.read_i32(&buffer, &mut pos).unwrap(), 42);
//! ```

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Plain encoder that stores values in their native binary representation
///
/// This encoder performs no compression and is stateless, making it suitable
/// for scenarios where data is already compressed at a higher level or where
/// random access patterns make compression ineffective.
pub struct PlainEncoder;

impl PlainEncoder {
    /// Creates a new plain encoder for the specified data type
    pub fn new(_data_type: TSDataType) -> Self {
        Self
    }
}

impl Encoder for PlainEncoder {
    fn encode_bool(&mut self, value: bool, out: &mut Vec<u8>) -> Result<()> {
        out.write_u8(value as u8)?;
        Ok(())
    }

    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        out.write_i32::<LittleEndian>(value)?;
        Ok(())
    }

    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        out.write_i64::<LittleEndian>(value)?;
        Ok(())
    }

    fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()> {
        out.write_f32::<LittleEndian>(value)?;
        Ok(())
    }

    fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()> {
        out.write_f64::<LittleEndian>(value)?;
        Ok(())
    }

    fn encode_string(&mut self, value: &str, out: &mut Vec<u8>) -> Result<()> {
        let bytes = value.as_bytes();
        out.write_i32::<LittleEndian>(bytes.len() as i32)?;
        out.extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self, _out: &mut Vec<u8>) -> Result<()> {
        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Plain
    }
}

/// Plain decoder that reads values from their native binary representation
///
/// The decoder maintains a position pointer and performs direct reads from
/// the input buffer with bounds checking.
pub struct PlainDecoder;

impl PlainDecoder {
    /// Creates a new plain decoder for the specified data type
    pub fn new(_data_type: TSDataType) -> Self {
        Self
    }
}

impl Decoder for PlainDecoder {
    fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool> {
        if *pos >= input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let value = input[*pos] != 0;
        *pos += 1;
        Ok(value)
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        if *pos + 4 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let mut cursor = &input[*pos..*pos + 4];
        let value = cursor.read_i32::<LittleEndian>()?;
        *pos += 4;
        Ok(value)
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        if *pos + 8 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let mut cursor = &input[*pos..*pos + 8];
        let value = cursor.read_i64::<LittleEndian>()?;
        *pos += 8;
        Ok(value)
    }

    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
        if *pos + 4 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let mut cursor = &input[*pos..*pos + 4];
        let value = cursor.read_f32::<LittleEndian>()?;
        *pos += 4;
        Ok(value)
    }

    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64> {
        if *pos + 8 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let mut cursor = &input[*pos..*pos + 8];
        let value = cursor.read_f64::<LittleEndian>()?;
        *pos += 8;
        Ok(value)
    }

    fn read_string(&mut self, input: &[u8], pos: &mut usize) -> Result<String> {
        if *pos + 4 > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let mut cursor = &input[*pos..*pos + 4];
        let len = cursor.read_i32::<LittleEndian>()? as usize;
        *pos += 4;

        if *pos + len > input.len() {
            return Err(TsFileError::UnexpectedEof);
        }
        let value = String::from_utf8(input[*pos..*pos + len].to_vec())
            .map_err(|e| TsFileError::DecodingError(e.to_string()))?;
        *pos += len;
        Ok(value)
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        pos < input.len()
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Plain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_bool() {
        let mut encoder = PlainEncoder::new(TSDataType::Boolean);
        let mut decoder = PlainDecoder::new(TSDataType::Boolean);
        let mut buf = Vec::new();

        encoder.encode_bool(true, &mut buf).unwrap();
        encoder.encode_bool(false, &mut buf).unwrap();

        let mut pos = 0;
        assert_eq!(decoder.read_bool(&buf, &mut pos).unwrap(), true);
        assert_eq!(decoder.read_bool(&buf, &mut pos).unwrap(), false);
    }

    #[test]
    fn test_plain_i32() {
        let mut encoder = PlainEncoder::new(TSDataType::Int32);
        let mut decoder = PlainDecoder::new(TSDataType::Int32);
        let mut buf = Vec::new();

        encoder.encode_i32(42, &mut buf).unwrap();
        encoder.encode_i32(-100, &mut buf).unwrap();

        let mut pos = 0;
        assert_eq!(decoder.read_i32(&buf, &mut pos).unwrap(), 42);
        assert_eq!(decoder.read_i32(&buf, &mut pos).unwrap(), -100);
    }

    #[test]
    fn test_plain_string() {
        let mut encoder = PlainEncoder::new(TSDataType::Text);
        let mut decoder = PlainDecoder::new(TSDataType::Text);
        let mut buf = Vec::new();

        encoder.encode_string("Hello", &mut buf).unwrap();
        encoder.encode_string("World", &mut buf).unwrap();

        let mut pos = 0;
        assert_eq!(decoder.read_string(&buf, &mut pos).unwrap(), "Hello");
        assert_eq!(decoder.read_string(&buf, &mut pos).unwrap(), "World");
    }
}
