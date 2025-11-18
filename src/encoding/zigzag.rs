/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * License); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Zigzag encoding for signed integers
//!
//! Zigzag encoding maps signed integers to unsigned integers in a way that
//! small absolute values result in small positive integers, which can then
//! be encoded more efficiently with variable-length encoding.
//!
//! Mapping:
//! - 0 -> 0
//! - -1 -> 1
//! - 1 -> 2
//! - -2 -> 3
//! - 2 -> 4
//! - etc.
//!
//! Formula:
//! - Encode: (n << 1) ^ (n >> 31) for i32
//! - Encode: (n << 1) ^ (n >> 63) for i64
//! - Decode: (n >> 1) ^ -(n & 1)

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};

/// Zigzag encoder for signed integers
pub struct ZigzagEncoder {
    data_type: TSDataType,
    /// Buffered encoded bytes
    encoded_bytes: Vec<u8>,
    /// Number of input values
    value_count: u32,
}

impl ZigzagEncoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            encoded_bytes: Vec::new(),
            value_count: 0,
        }
    }

    /// Encode i32 using zigzag + varint encoding
    fn encode_i32_value(&mut self, value: i32) -> Result<()> {
        // Zigzag encoding: map signed to unsigned
        let zigzag = ((value << 1) ^ (value >> 31)) as u32;

        // Varint encoding: 7 bits per byte with continuation bit
        let mut n = zigzag;
        loop {
            if n <= 0x7F {
                // Last byte: no continuation bit
                self.encoded_bytes.push(n as u8);
                break;
            } else {
                // More bytes to come: set continuation bit (0x80)
                self.encoded_bytes.push((n as u8 & 0x7F) | 0x80);
                n >>= 7;
            }
        }

        self.value_count += 1;
        Ok(())
    }

    /// Encode i64 using zigzag + varint encoding
    fn encode_i64_value(&mut self, value: i64) -> Result<()> {
        // Zigzag encoding: map signed to unsigned
        let zigzag = ((value << 1) ^ (value >> 63)) as u64;

        // Varint encoding
        let mut n = zigzag;
        loop {
            if n <= 0x7F {
                self.encoded_bytes.push(n as u8);
                break;
            } else {
                self.encoded_bytes.push((n as u8 & 0x7F) | 0x80);
                n >>= 7;
            }
        }

        self.value_count += 1;
        Ok(())
    }

    /// Write variable-length unsigned integer
    fn write_varuint(&self, value: u32, out: &mut Vec<u8>) {
        let mut n = value;
        loop {
            if n <= 0x7F {
                out.push(n as u8);
                break;
            } else {
                out.push((n as u8 & 0x7F) | 0x80);
                n >>= 7;
            }
        }
    }
}

impl Encoder for ZigzagEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Zigzag encoding not supported for booleans".to_string(),
        ))
    }

    fn encode_i32(&mut self, value: i32, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_i32_value(value)
    }

    fn encode_i64(&mut self, value: i64, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_i64_value(value)
    }

    fn encode_f32(&mut self, _value: f32, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Zigzag encoding not supported for floats".to_string(),
        ))
    }

    fn encode_f64(&mut self, _value: f64, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Zigzag encoding not supported for doubles".to_string(),
        ))
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Zigzag encoding not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        // Write header: [encoded_length][value_count]
        self.write_varuint(self.encoded_bytes.len() as u32, out);
        self.write_varuint(self.value_count, out);

        // Write encoded bytes
        out.extend_from_slice(&self.encoded_bytes);

        // Reset state
        self.encoded_bytes.clear();
        self.value_count = 0;

        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Zigzag
    }
}

/// Zigzag decoder for signed integers
pub struct ZigzagDecoder {
    data_type: TSDataType,
    /// Decoded bytes buffer
    decoded_bytes: Vec<u8>,
    /// Current position in decoded_bytes
    position: usize,
    /// Number of values to decode
    value_count: u32,
    /// Number of values already decoded
    values_read: u32,
}

impl ZigzagDecoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            decoded_bytes: Vec::new(),
            position: 0,
            value_count: 0,
            values_read: 0,
        }
    }

    /// Read variable-length unsigned integer
    fn read_varuint(&self, input: &[u8], pos: &mut usize) -> Result<u32> {
        let mut result: u32 = 0;
        let mut shift = 0;

        loop {
            if *pos >= input.len() {
                return Err(TsFileError::DecodingError(
                    "Unexpected end of input reading varuint".to_string(),
                ));
            }

            let byte = input[*pos];
            *pos += 1;

            result |= ((byte & 0x7F) as u32) << shift;

            if byte & 0x80 == 0 {
                break;
            }

            shift += 7;
            if shift >= 32 {
                return Err(TsFileError::DecodingError("Varuint too large".to_string()));
            }
        }

        Ok(result)
    }

    /// Initialize decoder by reading header and loading bytes
    fn init(&mut self, input: &[u8], pos: &mut usize) -> Result<()> {
        if !self.decoded_bytes.is_empty() {
            return Ok(()); // Already initialized
        }

        // Read header
        let encoded_length = self.read_varuint(input, pos)? as usize;
        self.value_count = self.read_varuint(input, pos)?;

        // Load encoded bytes
        if *pos + encoded_length > input.len() {
            return Err(TsFileError::DecodingError(
                "Not enough data for zigzag encoded values".to_string(),
            ));
        }

        self.decoded_bytes
            .extend_from_slice(&input[*pos..*pos + encoded_length]);
        *pos += encoded_length;
        self.position = 0;
        self.values_read = 0;

        Ok(())
    }

    /// Decode next i32 value
    fn decode_i32(&mut self) -> Result<i32> {
        if self.values_read >= self.value_count {
            return Err(TsFileError::DecodingError(
                "No more values to decode".to_string(),
            ));
        }

        // Read varint from decoded_bytes
        let mut result: u32 = 0;
        let mut shift = 0;

        loop {
            if self.position >= self.decoded_bytes.len() {
                return Err(TsFileError::DecodingError(
                    "Unexpected end of decoded bytes".to_string(),
                ));
            }

            let byte = self.decoded_bytes[self.position];
            self.position += 1;

            result |= ((byte & 0x7F) as u32) << shift;

            if byte & 0x80 == 0 {
                break;
            }

            shift += 7;
        }

        // Zigzag decode
        let value = ((result >> 1) as i32) ^ -((result & 1) as i32);
        self.values_read += 1;

        Ok(value)
    }

    /// Decode next i64 value
    fn decode_i64(&mut self) -> Result<i64> {
        if self.values_read >= self.value_count {
            return Err(TsFileError::DecodingError(
                "No more values to decode".to_string(),
            ));
        }

        // Read varint from decoded_bytes
        let mut result: u64 = 0;
        let mut shift = 0;

        loop {
            if self.position >= self.decoded_bytes.len() {
                return Err(TsFileError::DecodingError(
                    "Unexpected end of decoded bytes".to_string(),
                ));
            }

            let byte = self.decoded_bytes[self.position];
            self.position += 1;

            result |= ((byte & 0x7F) as u64) << shift;

            if byte & 0x80 == 0 {
                break;
            }

            shift += 7;
        }

        // Zigzag decode
        let value = ((result >> 1) as i64) ^ -((result & 1) as i64);
        self.values_read += 1;

        Ok(value)
    }
}

impl Decoder for ZigzagDecoder {
    fn read_bool(&mut self, _input: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TsFileError::DecodingError(
            "Zigzag decoding not supported for booleans".to_string(),
        ))
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        self.init(input, pos)?;
        self.decode_i32()
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        self.init(input, pos)?;
        self.decode_i64()
    }

    fn read_f32(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f32> {
        Err(TsFileError::DecodingError(
            "Zigzag decoding not supported for floats".to_string(),
        ))
    }

    fn read_f64(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f64> {
        Err(TsFileError::DecodingError(
            "Zigzag decoding not supported for doubles".to_string(),
        ))
    }

    fn read_string(&mut self, _input: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TsFileError::DecodingError(
            "Zigzag decoding not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, _input: &[u8], _pos: usize) -> bool {
        self.values_read < self.value_count
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Zigzag
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zigzag_i32_basic() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        let values = vec![0, -1, 1, -2, 2, -100, 100, i32::MIN, i32::MAX];

        for &v in &values {
            encoder.encode_i32(v, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int32);
        let mut pos = 0;

        for &expected in &values {
            let decoded = decoder.read_i32(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_zigzag_i64_basic() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int64);
        let mut output = Vec::new();

        let values = vec![0i64, -1, 1, -2, 2, -1000, 1000, i64::MIN, i64::MAX];

        for &v in &values {
            encoder.encode_i64(v, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int64);
        let mut pos = 0;

        for &expected in &values {
            let decoded = decoder.read_i64(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_zigzag_negative_numbers() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        // Test many negative numbers
        for i in -1000..0 {
            encoder.encode_i32(i, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int32);
        let mut pos = 0;

        for expected in -1000..0 {
            let decoded = decoder.read_i32(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_zigzag_positive_numbers() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        // Test many positive numbers
        for i in 0..1000 {
            encoder.encode_i32(i, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int32);
        let mut pos = 0;

        for expected in 0..1000 {
            let decoded = decoder.read_i32(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_zigzag_mixed_values() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        // Alternating positive and negative
        let mut values = Vec::new();
        for i in -500..500 {
            values.push(i);
        }

        for &v in &values {
            encoder.encode_i32(v, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int32);
        let mut pos = 0;

        for &expected in &values {
            let decoded = decoder.read_i32(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_zigzag_compression_efficiency() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        // Small numbers should compress well
        for _ in 0..1000 {
            encoder.encode_i32(0, &mut output).unwrap();
            encoder.encode_i32(-1, &mut output).unwrap();
            encoder.encode_i32(1, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        // Should be much smaller than 3000 * 4 bytes
        assert!(
            output.len() < 3000 * 4 / 2,
            "Zigzag should compress small values efficiently"
        );
    }

    #[test]
    fn test_zigzag_unsupported_types() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Float);
        let mut output = Vec::new();

        assert!(encoder.encode_bool(true, &mut output).is_err());
        assert!(encoder.encode_f32(3.14, &mut output).is_err());
        assert!(encoder.encode_f64(3.14, &mut output).is_err());
        assert!(encoder.encode_string("test", &mut output).is_err());
    }

    #[test]
    fn test_zigzag_empty() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        encoder.flush(&mut output).unwrap();

        let decoder = ZigzagDecoder::new(TSDataType::Int32);
        assert!(!decoder.has_remaining(&output, 0));
    }

    #[test]
    fn test_zigzag_single_value() {
        let mut encoder = ZigzagEncoder::new(TSDataType::Int64);
        let mut output = Vec::new();

        encoder.encode_i64(42, &mut output).unwrap();
        encoder.flush(&mut output).unwrap();

        let mut decoder = ZigzagDecoder::new(TSDataType::Int64);
        let mut pos = 0;

        assert_eq!(decoder.read_i64(&output, &mut pos).unwrap(), 42);
        assert!(!decoder.has_remaining(&output, pos));
    }
}
