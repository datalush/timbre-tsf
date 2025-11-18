use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};

/// Encoder Gorilla para flotantes (algoritmo de Facebook)
/// Usa XOR delta encoding optimizado para series temporales
pub struct GorillaEncoder {
    data_type: TSDataType,
    first_value: Option<u64>,
    previous_value: u64,
    previous_leading: u32,
    previous_trailing: u32,
    buffer: Vec<u8>,
    bit_buffer: u64,
    bits_in_buffer: u8,
    // Number of bits for encoding leading/significant bits (5 for 32-bit, 6 for 64-bit)
    leading_bits_width: u8,
    significant_bits_width: u8,
    value_bits: u8, // 32 or 64
}

impl GorillaEncoder {
    pub fn new(data_type: TSDataType) -> Self {
        let (leading_bits_width, significant_bits_width, value_bits) = match data_type {
            TSDataType::Float => (5, 5, 32),
            TSDataType::Double => (6, 6, 64),
            TSDataType::Int32 => (5, 5, 32),
            TSDataType::Int64 => (6, 6, 64),
            _ => (6, 6, 64), // Default to 64-bit
        };

        Self {
            data_type,
            first_value: None,
            previous_value: 0,
            // Initialize to INT32_MAX to ensure first XOR always writes new leading/trailing
            previous_leading: i32::MAX as u32,
            previous_trailing: 0,
            buffer: Vec::new(),
            bit_buffer: 0,
            bits_in_buffer: 0,
            leading_bits_width,
            significant_bits_width,
            value_bits,
        }
    }

    fn write_bits(&mut self, value: u64, num_bits: u8) {
        if num_bits == 0 {
            return;
        }

        let shift_amount = 64u8
            .saturating_sub(self.bits_in_buffer)
            .saturating_sub(num_bits);
        self.bit_buffer |= value << shift_amount;
        self.bits_in_buffer += num_bits;

        while self.bits_in_buffer >= 8 {
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer <<= 8;
            self.bits_in_buffer -= 8;
        }
    }

    fn write_bit(&mut self, bit: bool) {
        self.write_bits(bit as u64, 1);
    }

    fn flush_bits(&mut self) {
        if self.bits_in_buffer > 0 {
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        }
    }

    fn encode_value(&mut self, bits: u64) {
        if self.first_value.is_none() {
            self.first_value = Some(bits);
            self.previous_value = bits;
            // Write full value (32 or 64 bits depending on type)
            self.write_bits(bits, self.value_bits);
            return;
        }

        let xor = self.previous_value ^ bits;

        if xor == 0 {
            self.write_bit(false);
        } else {
            self.write_bit(true);

            // For 32-bit values, we need to count leading zeros from bit 31, not bit 63
            let leading = if self.value_bits == 32 {
                (xor as u32).leading_zeros()
            } else {
                xor.leading_zeros()
            };

            let trailing = if self.value_bits == 32 {
                (xor as u32).trailing_zeros()
            } else {
                xor.trailing_zeros()
            };

            if leading >= self.previous_leading && trailing >= self.previous_trailing {
                self.write_bit(false);
                let significant_bits =
                    self.value_bits as u32 - self.previous_leading - self.previous_trailing;
                self.write_bits(xor >> self.previous_trailing, significant_bits as u8);
            } else {
                self.write_bit(true);
                self.write_bits(leading as u64, self.leading_bits_width);
                let significant_bits = self.value_bits as u32 - leading - trailing;
                // Store significant_bits - 1 (to match C++ implementation)
                self.write_bits((significant_bits - 1) as u64, self.significant_bits_width);
                self.write_bits(xor >> trailing, significant_bits as u8);

                self.previous_leading = leading;
                self.previous_trailing = trailing;
            }
        }

        self.previous_value = bits;
    }
}

impl Encoder for GorillaEncoder {
    fn encode_bool(&mut self, value: bool, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as u64);
        Ok(())
    }

    fn encode_i32(&mut self, value: i32, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as u32 as u64);
        Ok(())
    }

    fn encode_i64(&mut self, value: i64, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value as u64);
        Ok(())
    }

    fn encode_f32(&mut self, value: f32, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value.to_bits() as u64);
        Ok(())
    }

    fn encode_f64(&mut self, value: f64, _out: &mut Vec<u8>) -> Result<()> {
        self.encode_value(value.to_bits());
        Ok(())
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TsFileError::EncodingError(
            "Gorilla encoding not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.flush_bits();
        out.extend_from_slice(&self.buffer);
        self.buffer.clear();
        Ok(())
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Gorilla
    }
}

/// Decoder Gorilla
pub struct GorillaDecoder {
    data_type: TSDataType,
    first_value: Option<u64>,
    previous_value: u64,
    previous_leading: u32,
    previous_trailing: u32,
    // Persistent bit position across multiple values
    byte_pos: usize,
    bit_pos: u8,
    // Number of bits for decoding leading/significant bits (5 for 32-bit, 6 for 64-bit)
    leading_bits_width: u8,
    significant_bits_width: u8,
    value_bits: u8, // 32 or 64
}

impl GorillaDecoder {
    pub fn new(data_type: TSDataType) -> Self {
        let (leading_bits_width, significant_bits_width, value_bits) = match data_type {
            TSDataType::Float => (5, 5, 32),
            TSDataType::Double => (6, 6, 64),
            TSDataType::Int32 => (5, 5, 32),
            TSDataType::Int64 => (6, 6, 64),
            _ => (6, 6, 64), // Default to 64-bit
        };

        Self {
            data_type,
            first_value: None,
            previous_value: 0,
            // Initialize to INT32_MAX to ensure first XOR always writes new leading/trailing
            previous_leading: i32::MAX as u32,
            previous_trailing: 0,
            byte_pos: 0,
            bit_pos: 0,
            leading_bits_width,
            significant_bits_width,
            value_bits,
        }
    }

    fn read_bits(input: &[u8], pos: &mut usize, bit_pos: &mut u8, num_bits: u8) -> Result<u64> {
        if num_bits == 0 {
            return Ok(0);
        }

        let mut result = 0u64;
        let mut bits_read = 0u8;

        while bits_read < num_bits {
            if *pos >= input.len() {
                return Err(TsFileError::UnexpectedEof);
            }

            let bits_to_read = num_bits - bits_read;
            let bits_available = 8 - *bit_pos;
            let bits_this_iter = bits_to_read.min(bits_available);

            let byte = input[*pos];
            let mask = if bits_this_iter == 8 {
                0xFF
            } else {
                ((1u16 << bits_this_iter) - 1) as u8
            };
            let shift = bits_available - bits_this_iter;
            let value = (byte >> shift) & mask;

            result = (result << bits_this_iter) | (value as u64);
            bits_read += bits_this_iter;
            *bit_pos += bits_this_iter;

            if *bit_pos >= 8 {
                *bit_pos = 0;
                *pos += 1;
            }
        }

        Ok(result)
    }

    fn read_bit(input: &[u8], pos: &mut usize, bit_pos: &mut u8) -> Result<bool> {
        Ok(Self::read_bits(input, pos, bit_pos, 1)? != 0)
    }

    fn decode_value(&mut self, input: &[u8]) -> Result<u64> {
        if self.first_value.is_none() {
            let value = Self::read_bits(
                input,
                &mut self.byte_pos,
                &mut self.bit_pos,
                self.value_bits,
            )?;
            self.first_value = Some(value);
            self.previous_value = value;
            return Ok(value);
        }

        let is_different = Self::read_bit(input, &mut self.byte_pos, &mut self.bit_pos)?;
        if !is_different {
            return Ok(self.previous_value);
        }

        let use_previous_block = !Self::read_bit(input, &mut self.byte_pos, &mut self.bit_pos)?;

        let (_leading, significant_bits) = if use_previous_block {
            let bits = self.value_bits as u32 - self.previous_leading - self.previous_trailing;
            (self.previous_leading, bits)
        } else {
            let leading = Self::read_bits(
                input,
                &mut self.byte_pos,
                &mut self.bit_pos,
                self.leading_bits_width,
            )? as u32;
            let mut significant_bits = Self::read_bits(
                input,
                &mut self.byte_pos,
                &mut self.bit_pos,
                self.significant_bits_width,
            )? as u32;
            // Add 1 back (was stored as significant_bits - 1)
            significant_bits += 1;
            self.previous_leading = leading;
            self.previous_trailing = self.value_bits as u32 - leading - significant_bits;
            (leading, significant_bits)
        };

        let xor_value = Self::read_bits(
            input,
            &mut self.byte_pos,
            &mut self.bit_pos,
            significant_bits as u8,
        )?;
        let xor = xor_value << self.previous_trailing;
        let value = self.previous_value ^ xor;
        self.previous_value = value;

        Ok(value)
    }
}

impl Decoder for GorillaDecoder {
    fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool> {
        let value = self.decode_value(input)?;
        // Update pos to reflect bytes consumed (byte_pos is the actual position)
        *pos = self.byte_pos;
        Ok(value != 0)
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        let value = self.decode_value(input)?;
        *pos = self.byte_pos;
        Ok(value as u32 as i32)
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        let value = self.decode_value(input)?;
        *pos = self.byte_pos;
        Ok(value as i64)
    }

    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
        let value = self.decode_value(input)?;
        *pos = self.byte_pos;
        Ok(f32::from_bits(value as u32))
    }

    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64> {
        let value = self.decode_value(input)?;
        *pos = self.byte_pos;
        Ok(f64::from_bits(value))
    }

    fn read_string(&mut self, _input: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TsFileError::DecodingError(
            "Gorilla decoding not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, input: &[u8], _pos: usize) -> bool {
        // Check if we have more bytes to read based on internal position
        self.byte_pos < input.len() || (self.byte_pos == input.len() && self.bit_pos > 0)
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Gorilla
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gorilla_f32_simple() {
        // Test with just two values first
        let mut encoder = GorillaEncoder::new(TSDataType::Float);
        let mut out = Vec::new();

        let v1 = 1.5f32;
        let v2 = 1.5f32; // Same value to test XOR == 0 case

        encoder.encode_f32(v1, &mut out).unwrap();
        encoder.encode_f32(v2, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = GorillaDecoder::new(TSDataType::Float);
        let mut pos = 0;

        let d1 = decoder.read_f32(&out, &mut pos).unwrap();
        assert_eq!(d1, v1);

        let d2 = decoder.read_f32(&out, &mut pos).unwrap();
        assert_eq!(d2, v2);
    }

    #[test]
    fn test_gorilla_f32_two_diff() {
        // Test with two different values
        let mut encoder = GorillaEncoder::new(TSDataType::Float);
        let mut out = Vec::new();

        let v1 = 1.5f32;
        let v2 = 1.6f32;

        encoder.encode_f32(v1, &mut out).unwrap();
        encoder.encode_f32(v2, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = GorillaDecoder::new(TSDataType::Float);
        let mut pos = 0;

        let d1 = decoder.read_f32(&out, &mut pos).unwrap();
        assert_eq!(d1, v1);

        let d2 = decoder.read_f32(&out, &mut pos).unwrap();
        assert_eq!(d2, v2);
    }

    #[test]
    fn test_gorilla_f32() {
        let mut encoder = GorillaEncoder::new(TSDataType::Float);
        let mut out = Vec::new();

        let values = vec![1.5f32, 1.6, 1.55, 1.52, 1.58];
        for &val in &values {
            encoder.encode_f32(val, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = GorillaDecoder::new(TSDataType::Float);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_f32(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn test_gorilla_f64_two_diff() {
        // Test with two different values
        let mut encoder = GorillaEncoder::new(TSDataType::Double);
        let mut out = Vec::new();

        let v1 = 1.5f64;
        let v2 = 1.6f64;

        encoder.encode_f64(v1, &mut out).unwrap();
        encoder.encode_f64(v2, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = GorillaDecoder::new(TSDataType::Double);
        let mut pos = 0;

        let d1 = decoder.read_f64(&out, &mut pos).unwrap();
        assert_eq!(d1, v1);

        let d2 = decoder.read_f64(&out, &mut pos).unwrap();
        assert_eq!(d2, v2);
    }

    #[test]
    fn test_gorilla_f64() {
        let mut encoder = GorillaEncoder::new(TSDataType::Double);
        let mut out = Vec::new();

        let values = vec![1.5f64, 1.6, 1.55, 1.52, 1.58];
        for &val in &values {
            encoder.encode_f64(val, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = GorillaDecoder::new(TSDataType::Double);
        let mut pos = 0;
        for &expected in &values {
            let decoded = decoder.read_f64(&out, &mut pos).unwrap();
            assert_eq!(decoded, expected);
        }
    }
}
