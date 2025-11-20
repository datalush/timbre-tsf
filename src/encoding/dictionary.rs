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

//! Dictionary encoding for repetitive strings
//!
//! This encoder builds a dictionary of unique strings and encodes them as integer IDs.
//! The format is: [dictionary_size][string1][string2]...[encoded_ids]
//!
//! This is particularly efficient for:
//! - Device names
//! - Tags with limited cardinality
//! - Repetitive string values

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use std::collections::HashMap;

/// Dictionary encoder for strings with high repetition
pub struct DictionaryEncoder {
    /// Map from string to integer ID
    entry_index: HashMap<String, i32>,
    /// Map from integer ID to string (for ordered storage)
    index_entry: Vec<String>,
    /// Encoded integer IDs (using RLE for better compression)
    encoded_ids: Vec<i32>,
}

impl DictionaryEncoder {
    pub fn new(_data_type: TSDataType) -> Self {
        Self {
            entry_index: HashMap::new(),
            index_entry: Vec::new(),
            encoded_ids: Vec::new(),
        }
    }

    /// Write the dictionary header: [count][string1][string2]...
    fn write_dictionary(&self, out: &mut Vec<u8>) -> Result<()> {
        // Write number of unique strings using varint encoding
        self.write_varint(self.index_entry.len() as i32, out)?;

        // Write each unique string
        for s in &self.index_entry {
            self.write_var_string(s, out)?;
        }

        Ok(())
    }

    /// Write the encoded IDs using RLE encoding
    fn write_encoded_ids(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.encoded_ids.is_empty() {
            return Ok(());
        }

        // Simple RLE encoding: [value, count, value, count, ...]
        let mut i = 0;
        while i < self.encoded_ids.len() {
            let current = self.encoded_ids[i];
            let mut count = 1;

            // Count consecutive equal values
            while i + count < self.encoded_ids.len() && self.encoded_ids[i + count] == current {
                count += 1;
            }

            // Write value and count using varint
            self.write_varint(current, out)?;
            self.write_varint(count as i32, out)?;

            i += count;
        }

        Ok(())
    }

    /// Write variable-length integer (compatible with C++ implementation)
    fn write_varint(&self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        // Zigzag encoding for signed integers
        let encoded = ((value << 1) ^ (value >> 31)) as u32;

        let mut n = encoded;
        loop {
            if n <= 0x7F {
                out.push(n as u8);
                break;
            } else {
                out.push((n as u8 & 0x7F) | 0x80);
                n >>= 7;
            }
        }
        Ok(())
    }

    /// Write variable-length string
    fn write_var_string(&self, s: &str, out: &mut Vec<u8>) -> Result<()> {
        // Write length as varint
        self.write_varint(s.len() as i32, out)?;
        // Write string bytes
        out.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

impl Encoder for DictionaryEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Dictionary encoding not supported for booleans".to_string(),
        ))
    }

    fn encode_i32(&mut self, _value: i32, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Dictionary encoding not supported for integers".to_string(),
        ))
    }

    fn encode_i64(&mut self, _value: i64, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Dictionary encoding not supported for long integers".to_string(),
        ))
    }

    fn encode_f32(&mut self, _value: f32, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Dictionary encoding not supported for floats".to_string(),
        ))
    }

    fn encode_f64(&mut self, _value: f64, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Dictionary encoding not supported for doubles".to_string(),
        ))
    }

    fn encode_string(&mut self, value: &str, _out: &mut Vec<u8>) -> Result<()> {
        // Get or create ID for this string
        let id = if let Some(&existing_id) = self.entry_index.get(value) {
            existing_id
        } else {
            let new_id = self.index_entry.len() as i32;
            self.entry_index.insert(value.to_string(), new_id);
            self.index_entry.push(value.to_string());
            new_id
        };

        // Store the ID for later encoding
        self.encoded_ids.push(id);
        Ok(())
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        // Write dictionary first
        self.write_dictionary(out)?;

        // Then write the encoded IDs
        self.write_encoded_ids(out)?;

        // Reset state
        self.entry_index.clear();
        self.index_entry.clear();
        self.encoded_ids.clear();

        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Dictionary
    }
}

/// Dictionary decoder for strings
pub struct DictionaryDecoder {
    /// Decoded dictionary (ID -> String)
    dictionary: Vec<String>,
    /// Decoded IDs buffer
    decoded_ids: Vec<i32>,
    /// Current position in decoded_ids
    position: usize,
}

impl DictionaryDecoder {
    pub fn new(_data_type: TSDataType) -> Self {
        Self {
            dictionary: Vec::new(),
            decoded_ids: Vec::new(),
            position: 0,
        }
    }

    /// Read variable-length integer
    fn read_varint(&self, input: &[u8], pos: &mut usize) -> Result<i32> {
        let mut result: u32 = 0;
        let mut shift = 0;

        loop {
            if *pos >= input.len() {
                return Err(TsFileError::DecodingError(
                    "Unexpected end of input reading varint".to_string(),
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
                return Err(TsFileError::DecodingError("Varint too large".to_string()));
            }
        }

        // Zigzag decoding
        let value = ((result >> 1) as i32) ^ -((result & 1) as i32);
        Ok(value)
    }

    /// Read variable-length string
    fn read_var_string(&self, input: &[u8], pos: &mut usize) -> Result<String> {
        let length = self.read_varint(input, pos)? as usize;

        if *pos + length > input.len() {
            return Err(TsFileError::DecodingError(
                "Unexpected end of input reading string".to_string(),
            ));
        }

        let s = String::from_utf8(input[*pos..*pos + length].to_vec())
            .map_err(|e| TsFileError::DecodingError(format!("Invalid UTF-8: {}", e)))?;

        *pos += length;
        Ok(s)
    }

    /// Initialize the dictionary from the input
    fn init_dictionary(&mut self, input: &[u8], pos: &mut usize) -> Result<()> {
        if !self.dictionary.is_empty() {
            return Ok(()); // Already initialized
        }

        let count = self.read_varint(input, pos)?;

        for _ in 0..count {
            let s = self.read_var_string(input, pos)?;
            self.dictionary.push(s);
        }

        Ok(())
    }

    /// Decode all IDs from the input (RLE encoded)
    fn decode_ids(&mut self, input: &[u8], pos: &mut usize) -> Result<()> {
        if !self.decoded_ids.is_empty() {
            return Ok(()); // Already decoded
        }

        while *pos < input.len() {
            let value = self.read_varint(input, pos)?;
            let count = self.read_varint(input, pos)? as usize;

            for _ in 0..count {
                self.decoded_ids.push(value);
            }
        }

        Ok(())
    }
}

impl Decoder for DictionaryDecoder {
    fn read_bool(&mut self, _input: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TsFileError::DecodingError(
            "Dictionary decoding not supported for booleans".to_string(),
        ))
    }

    fn read_i32(&mut self, _input: &[u8], _pos: &mut usize) -> Result<i32> {
        Err(TsFileError::DecodingError(
            "Dictionary decoding not supported for integers".to_string(),
        ))
    }

    fn read_i64(&mut self, _input: &[u8], _pos: &mut usize) -> Result<i64> {
        Err(TsFileError::DecodingError(
            "Dictionary decoding not supported for long integers".to_string(),
        ))
    }

    fn read_f32(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f32> {
        Err(TsFileError::DecodingError(
            "Dictionary decoding not supported for floats".to_string(),
        ))
    }

    fn read_f64(&mut self, _input: &[u8], _pos: &mut usize) -> Result<f64> {
        Err(TsFileError::DecodingError(
            "Dictionary decoding not supported for doubles".to_string(),
        ))
    }

    fn read_string(&mut self, input: &[u8], pos: &mut usize) -> Result<String> {
        // Initialize dictionary if needed
        if self.dictionary.is_empty() {
            self.init_dictionary(input, pos)?;
        }

        // Decode IDs if needed
        if self.decoded_ids.is_empty() {
            let decode_start = *pos;
            self.decode_ids(input, pos)?;
            *pos = decode_start; // Reset position for sequential reading
            self.position = 0;
        }

        // Return the next string
        if self.position >= self.decoded_ids.len() {
            return Err(TsFileError::DecodingError(
                "No more values to decode".to_string(),
            ));
        }

        let id = self.decoded_ids[self.position] as usize;
        self.position += 1;

        if id >= self.dictionary.len() {
            return Err(TsFileError::DecodingError(format!(
                "Invalid dictionary ID: {}",
                id
            )));
        }

        // Clone necessary: returning owned String from dictionary lookup
        Ok(self.dictionary[id].clone())
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        // If we have decoded data, check if there are more values to read
        if !self.decoded_ids.is_empty() {
            return self.position < self.decoded_ids.len();
        }
        // Otherwise, check if there's more data in the input
        pos < input.len()
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Dictionary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_encode_decode_basic() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        encoder.encode_string("apple", &mut output).unwrap();
        encoder.encode_string("banana", &mut output).unwrap();
        encoder.encode_string("cherry", &mut output).unwrap();
        encoder.encode_string("apple", &mut output).unwrap();
        encoder.flush(&mut output).unwrap();

        let mut decoder = DictionaryDecoder::new(TSDataType::Text);
        let mut pos = 0;

        assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), "apple");
        assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), "banana");
        assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), "cherry");
        assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), "apple");
    }

    #[test]
    fn test_dictionary_single_value() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        encoder.encode_string("apple", &mut output).unwrap();
        encoder.flush(&mut output).unwrap();

        let mut decoder = DictionaryDecoder::new(TSDataType::Text);
        let mut pos = 0;

        assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), "apple");
        assert!(!decoder.has_remaining(&output, pos));
    }

    #[test]
    fn test_dictionary_repetitive() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        // Encode 100 instances of each letter a-z
        for c in b'a'..=b'z' {
            let s = String::from_utf8(vec![c, c, c]).unwrap();
            for _ in 0..100 {
                encoder.encode_string(&s, &mut output).unwrap();
            }
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = DictionaryDecoder::new(TSDataType::Text);
        let mut pos = 0;

        // Verify all values
        for c in b'a'..=b'z' {
            let expected = String::from_utf8(vec![c, c, c]).unwrap();
            for _ in 0..100 {
                assert_eq!(decoder.read_string(&output, &mut pos).unwrap(), expected);
            }
        }
    }

    #[test]
    fn test_dictionary_device_names() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        let devices = vec![
            "root.sg.device1",
            "root.sg.device2",
            "root.sg.device3",
            "root.sg.device1",
            "root.sg.device2",
            "root.sg.device1",
        ];

        for device in &devices {
            encoder.encode_string(device, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        let mut decoder = DictionaryDecoder::new(TSDataType::Text);
        let mut pos = 0;

        for expected in &devices {
            assert_eq!(&decoder.read_string(&output, &mut pos).unwrap(), expected);
        }
    }

    #[test]
    fn test_dictionary_unsupported_types() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Int32);
        let mut output = Vec::new();

        assert!(encoder.encode_bool(true, &mut output).is_err());
        assert!(encoder.encode_i32(42, &mut output).is_err());
        assert!(encoder.encode_i64(42, &mut output).is_err());
        assert!(encoder.encode_f32(3.14, &mut output).is_err());
        assert!(encoder.encode_f64(3.14, &mut output).is_err());
    }

    #[test]
    fn test_varint_encoding() {
        let encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        // Test various values
        let test_values = vec![0, 1, -1, 127, -127, 128, -128, 1000, -1000];

        for &val in &test_values {
            encoder.write_varint(val, &mut output).unwrap();
        }

        let decoder = DictionaryDecoder::new(TSDataType::Text);
        let mut pos = 0;

        for &expected in &test_values {
            let decoded = decoder.read_varint(&output, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_compression_efficiency() {
        let mut encoder = DictionaryEncoder::new(TSDataType::Text);
        let mut output = Vec::new();

        // Test that dictionary encoding is more efficient than plain
        let device = "root.very.long.device.path.name";
        for _ in 0..1000 {
            encoder.encode_string(device, &mut output).unwrap();
        }
        encoder.flush(&mut output).unwrap();

        // Dictionary should be much smaller than 1000 * device.len()
        let plain_size = 1000 * device.len();
        assert!(
            output.len() < plain_size / 10,
            "Dictionary encoding should compress repetitive strings. Got {} bytes vs {} plain",
            output.len(),
            plain_size
        );
    }
}
