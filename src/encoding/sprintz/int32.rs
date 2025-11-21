use super::base::*;
use crate::error::{Result, TimbreError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

const BLOCK_SIZE: usize = 8;
const GROUP_MAX: usize = 16;

/// Int32 Sprintz Encoder
///
/// Compresses int32 values using:
/// 1. FIRE prediction or delta encoding
/// 2. Zigzag encoding for signed values
/// 3. Bit packing to minimal width
pub struct Int32SprintzEncoder {
    values: Vec<i32>,
    byte_cache: Vec<u8>,
    group_num: usize,
    predict_method: PredictMethod,
    fire_pred: FireI32,
    is_first_cached: bool,
}

impl Int32SprintzEncoder {
    pub fn new() -> Self {
        Self {
            values: Vec::with_capacity(BLOCK_SIZE + 1),
            byte_cache: Vec::new(),
            group_num: 0,
            predict_method: PredictMethod::Fire,
            fire_pred: FireI32::new(2),
            is_first_cached: false,
        }
    }

    pub fn set_predict_method(&mut self, method: PredictMethod) {
        self.predict_method = method;
    }

    pub fn encode(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        if !self.is_first_cached {
            self.values.push(value);
            self.is_first_cached = true;
            return Ok(());
        }

        self.values.push(value);

        if self.values.len() == BLOCK_SIZE + 1 {
            self.encode_block()?;

            if self.group_num == GROUP_MAX {
                self.flush_internal(out)?;
            }
        }

        Ok(())
    }

    fn encode_block(&mut self) -> Result<()> {
        let mut prev = self.values[0];
        self.fire_pred.reset();

        // Apply prediction to values[1..9]
        // Match C++ implementation: save original value before prediction
        for i in 1..=BLOCK_SIZE {
            let temp = self.values[i];
            self.values[i] = self.predict(self.values[i], prev);
            prev = temp; // Use original value as previous for next iteration
        }

        self.bit_pack(self.values[0])?;

        self.values.clear();
        self.is_first_cached = false;
        self.group_num += 1;

        Ok(())
    }

    fn predict(&mut self, value: i32, prev: i32) -> i32 {
        let pred = match self.predict_method {
            PredictMethod::Delta => value.wrapping_sub(prev),
            PredictMethod::Fire => {
                let prediction = self.fire_pred.predict(prev);
                let err = value.wrapping_sub(prediction);
                self.fire_pred.train(prev, value, err);
                err
            }
        };

        zigzag_encode_i32(pred)
    }

    fn bit_pack(&mut self, pre_value: i32) -> Result<()> {
        // Calculate bit width for values[1..9]
        let bit_width = get_max_bit_width_i32(&self.values[1..]);

        // Pack into bytes
        let mut pack_buf = Vec::new();
        pack_8values_i32(&self.values[1..], bit_width, &mut pack_buf);

        // Write to cache: bit_width + pre_value + packed_data
        self.byte_cache.push(bit_width);
        write_var_uint(pre_value as u32, &mut self.byte_cache)?;
        self.byte_cache.extend_from_slice(&pack_buf);

        Ok(())
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.flush_internal(out)?;

        // Handle remaining partial block
        if !self.values.is_empty() {
            let size = (self.values.len() as u8) | (1 << 7);
            out.push(size);

            // Use RLE for partial block
            for &val in &self.values {
                out.write_i32::<LittleEndian>(val)?;
            }
        }

        self.reset();
        Ok(())
    }

    fn flush_internal(&mut self, out: &mut Vec<u8>) -> Result<()> {
        if !self.byte_cache.is_empty() {
            out.extend_from_slice(&self.byte_cache);
            self.byte_cache.clear();
        }
        self.group_num = 0;
        Ok(())
    }

    fn reset(&mut self) {
        self.values.clear();
        self.byte_cache.clear();
        self.is_first_cached = false;
        self.group_num = 0;
    }
}

impl Default for Int32SprintzEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Int32 Sprintz Decoder
pub struct Int32SprintzDecoder {
    current_buffer: Vec<i32>,
    current_count: usize,
    decode_size: usize,
    is_block_read: bool,
    predict_method: PredictMethod,
    fire_pred: FireI32,
}

impl Int32SprintzDecoder {
    pub fn new() -> Self {
        Self {
            current_buffer: vec![0; BLOCK_SIZE + 1],
            current_count: 0,
            decode_size: 0,
            is_block_read: false,
            predict_method: PredictMethod::Fire,
            fire_pred: FireI32::new(2),
        }
    }

    pub fn set_predict_method(&mut self, method: PredictMethod) {
        self.predict_method = method;
    }

    pub fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        (self.is_block_read && self.current_count < self.decode_size) || pos + 5 <= input.len()
    }

    pub fn read_int32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        if !self.is_block_read {
            self.decode_block(input, pos)?;
        }

        let value = self.current_buffer[self.current_count];
        self.current_count += 1;

        if self.current_count == self.decode_size {
            self.is_block_read = false;
            self.current_count = 0;
        }

        Ok(value)
    }

    fn decode_block(&mut self, input: &[u8], pos: &mut usize) -> Result<()> {
        if *pos >= input.len() {
            return Err(TimbreError::DecodingError("Insufficient data".to_string()));
        }

        let bit_width = input[*pos];
        *pos += 1;

        // Check for partial block (MSB set)
        if (bit_width & (1 << 7)) != 0 {
            let size = (bit_width & !(1 << 7)) as usize;
            self.decode_size = size;

            for i in 0..size {
                if *pos + 4 > input.len() {
                    return Err(TimbreError::DecodingError(
                        "Insufficient data for RLE block".to_string(),
                    ));
                }
                let mut cursor = Cursor::new(&input[*pos..*pos + 4]);
                self.current_buffer[i] = cursor.read_i32::<LittleEndian>()?;
                *pos += 4;
            }
        } else {
            self.decode_size = BLOCK_SIZE + 1;

            // Read varint pre_value
            let pre_value = read_var_uint(input, pos)? as i32;
            self.current_buffer[0] = pre_value;

            // Read packed data
            if *pos + bit_width as usize > input.len() {
                return Err(TimbreError::DecodingError(
                    "Insufficient data for packed values".to_string(),
                ));
            }

            let pack_buf = &input[*pos..*pos + bit_width as usize];
            *pos += bit_width as usize;

            // Unpack values
            let mut unpacked = Vec::new();
            unpack_8values_i32(pack_buf, bit_width, &mut unpacked);

            self.current_buffer[1..9].copy_from_slice(&unpacked[..8]);

            self.recalculate()?;
        }

        self.is_block_read = true;
        Ok(())
    }

    fn recalculate(&mut self) -> Result<()> {
        // Zigzag decode
        for i in 1..=BLOCK_SIZE {
            self.current_buffer[i] = zigzag_decode_i32(self.current_buffer[i]);
        }

        // Reverse prediction using wrapping arithmetic
        match self.predict_method {
            PredictMethod::Delta => {
                for i in 1..self.current_buffer.len() {
                    self.current_buffer[i] =
                        self.current_buffer[i].wrapping_add(self.current_buffer[i - 1]);
                }
            }
            PredictMethod::Fire => {
                self.fire_pred.reset();
                for i in 1..=BLOCK_SIZE {
                    let pred = self.fire_pred.predict(self.current_buffer[i - 1]);
                    let err = self.current_buffer[i];
                    self.current_buffer[i] = pred.wrapping_add(err);
                    self.fire_pred
                        .train(self.current_buffer[i - 1], self.current_buffer[i], err);
                }
            }
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        self.current_buffer.fill(0);
        self.current_count = 0;
        self.decode_size = 0;
        self.is_block_read = false;
    }
}

impl Default for Int32SprintzDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Write variable-length unsigned integer
fn write_var_uint(mut value: u32, out: &mut Vec<u8>) -> Result<()> {
    while value > 0x7F {
        out.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    Ok(())
}

/// Read variable-length unsigned integer
fn read_var_uint(input: &[u8], pos: &mut usize) -> Result<u32> {
    let mut value: u32 = 0;
    let mut shift = 0;

    loop {
        if *pos >= input.len() {
            return Err(TimbreError::DecodingError(
                "Insufficient data for varint".to_string(),
            ));
        }

        let byte = input[*pos];
        *pos += 1;

        value |= ((byte & 0x7F) as u32) << shift;

        if (byte & 0x80) == 0 {
            break;
        }

        shift += 7;
        if shift >= 32 {
            return Err(TimbreError::DecodingError("Varint too large".to_string()));
        }
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int32_single_value() {
        let mut encoder = Int32SprintzEncoder::new();
        let mut out = Vec::new();

        encoder.encode(777, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int32SprintzDecoder::new();
        let mut pos = 0;
        assert!(decoder.has_remaining(&out, pos));
        let val = decoder.read_int32(&out, &mut pos).unwrap();
        assert_eq!(val, 777);
        assert!(!decoder.has_remaining(&out, pos));
    }

    #[test]
    fn test_int32_edge_values() {
        let values = vec![i32::MIN, -1, 0, 1, i32::MAX];
        let mut encoder = Int32SprintzEncoder::new();
        let mut out = Vec::new();

        for v in &values {
            encoder.encode(*v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int32SprintzDecoder::new();
        let mut pos = 0;

        for &expected in &values {
            assert!(decoder.has_remaining(&out, pos));
            let actual = decoder.read_int32(&out, &mut pos).unwrap();
            assert_eq!(actual, expected);
        }
        assert!(!decoder.has_remaining(&out, pos));
    }

    #[test]
    fn test_int32_increasing_sequence() {
        let mut encoder = Int32SprintzEncoder::new();
        let mut out = Vec::new();

        for i in 0..100 {
            encoder.encode(7 + 2 * i, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int32SprintzDecoder::new();
        let mut pos = 0;

        for i in 0..100 {
            assert!(decoder.has_remaining(&out, pos));
            let val = decoder.read_int32(&out, &mut pos).unwrap();
            if val != 7 + 2 * i {
                eprintln!("ERROR at i={}: expected={}, got={}", i, 7 + 2 * i, val);
                eprintln!("Buffer state: {:?}", &decoder.current_buffer[..9]);
            }
            assert_eq!(val, 7 + 2 * i);
        }
        assert!(!decoder.has_remaining(&out, pos));
    }

    #[test]
    fn test_int32_compression_ratio() {
        let mut encoder = Int32SprintzEncoder::new();
        let mut out = Vec::new();

        // Slowly changing values (good compression)
        for i in 0..100 {
            encoder.encode(1000 + i / 10, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        // Without compression: 100 * 4 = 400 bytes
        // With Sprintz: should be much smaller
        println!(
            "Uncompressed: 400 bytes, Sprintz: {} bytes, ratio: {:.2}x",
            out.len(),
            400.0 / out.len() as f64
        );
        assert!(out.len() < 200);
    }

    #[test]
    fn test_int32_zeros() {
        let mut encoder = Int32SprintzEncoder::new();
        let mut out = Vec::new();

        for _ in 0..16 {
            encoder.encode(0, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int32SprintzDecoder::new();
        let mut pos = 0;

        for _ in 0..16 {
            let val = decoder.read_int32(&out, &mut pos).unwrap();
            assert_eq!(val, 0);
        }
    }
}
