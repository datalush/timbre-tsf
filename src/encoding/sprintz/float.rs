use super::base::*;
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

const BLOCK_SIZE: usize = 8;
const GROUP_MAX: usize = 16;

/// Float Sprintz Encoder
///
/// Floats are converted to bits (u32), then treated as int32 for prediction
pub struct FloatSprintzEncoder {
    values: Vec<f32>,
    byte_cache: Vec<u8>,
    group_num: usize,
    predict_method: PredictMethod,
    fire_pred: FireI32,
    is_first_cached: bool,
}

impl FloatSprintzEncoder {
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

    pub fn encode(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()> {
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
        self.fire_pred.reset();

        // Convert floats to int32 bits and apply prediction
        let mut convert_buffer = Vec::with_capacity(BLOCK_SIZE);

        for i in 1..=BLOCK_SIZE {
            let pred = self.predict(self.values[i], self.values[i - 1]);
            convert_buffer.push(pred);
        }

        self.bit_pack(self.values[0], &convert_buffer)?;

        self.values.clear();
        self.is_first_cached = false;
        self.group_num += 1;

        Ok(())
    }

    fn predict(&mut self, value: f32, prev_value: f32) -> i32 {
        let curr_bits = value.to_bits() as i32;
        let prev_bits = prev_value.to_bits() as i32;

        let raw_pred = match self.predict_method {
            PredictMethod::Delta => curr_bits.wrapping_sub(prev_bits),
            PredictMethod::Fire => {
                let pred = self.fire_pred.predict(prev_bits);
                let err = curr_bits.wrapping_sub(pred);
                self.fire_pred.train(prev_bits, curr_bits, err);
                err
            }
        };

        zigzag_encode_i32(raw_pred)
    }

    fn bit_pack(&mut self, pre_value: f32, convert_buffer: &[i32]) -> Result<()> {
        let bit_width = get_max_bit_width_i32(convert_buffer);

        let mut pack_buf = Vec::new();
        pack_8values_i32(convert_buffer, bit_width, &mut pack_buf);

        self.byte_cache.push(bit_width);
        self.byte_cache.write_f32::<LittleEndian>(pre_value)?;
        self.byte_cache.extend_from_slice(&pack_buf);

        Ok(())
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.flush_internal(out)?;

        // Handle remaining partial block using simple float encoding
        if !self.values.is_empty() {
            let size = (self.values.len() as u8) | (1 << 7);
            out.push(size);

            for &val in &self.values {
                out.write_f32::<LittleEndian>(val)?;
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

impl Default for FloatSprintzEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Float Sprintz Decoder
pub struct FloatSprintzDecoder {
    current_buffer: Vec<f32>,
    current_count: usize,
    decode_size: usize,
    is_block_read: bool,
    predict_method: PredictMethod,
    fire_pred: FireI32,
}

impl FloatSprintzDecoder {
    pub fn new() -> Self {
        Self {
            current_buffer: vec![0.0; BLOCK_SIZE + 1],
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

    pub fn read_float(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
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
            return Err(TsFileError::DecodingError("Insufficient data".to_string()));
        }

        let bit_width = input[*pos];
        *pos += 1;

        if (bit_width & (1 << 7)) != 0 {
            let size = (bit_width & !(1 << 7)) as usize;
            self.decode_size = size;

            for i in 0..size {
                if *pos + 4 > input.len() {
                    return Err(TsFileError::DecodingError(
                        "Insufficient data for partial block".to_string(),
                    ));
                }
                let mut cursor = Cursor::new(&input[*pos..*pos + 4]);
                self.current_buffer[i] = cursor.read_f32::<LittleEndian>()?;
                *pos += 4;
            }
        } else {
            self.decode_size = BLOCK_SIZE + 1;

            if *pos + 4 > input.len() {
                return Err(TsFileError::DecodingError(
                    "Insufficient data for pre_value".to_string(),
                ));
            }
            let mut cursor = Cursor::new(&input[*pos..*pos + 4]);
            let pre_bits = cursor.read_f32::<LittleEndian>()?;
            *pos += 4;
            self.current_buffer[0] = pre_bits;

            if *pos + bit_width as usize > input.len() {
                return Err(TsFileError::DecodingError(
                    "Insufficient data for packed values".to_string(),
                ));
            }

            let pack_buf = &input[*pos..*pos + bit_width as usize];
            *pos += bit_width as usize;

            let mut unpacked = Vec::new();
            unpack_8values_i32(pack_buf, bit_width, &mut unpacked);

            self.recalculate(unpacked)?;
        }

        self.is_block_read = true;
        Ok(())
    }

    fn recalculate(&mut self, mut convert_buffer: Vec<i32>) -> Result<()> {
        // Zigzag decode
        for i in 0..BLOCK_SIZE {
            convert_buffer[i] = zigzag_decode_i32(convert_buffer[i]);
        }

        // Reverse prediction using wrapping arithmetic
        match self.predict_method {
            PredictMethod::Delta => {
                let mut prev_bits = self.current_buffer[0].to_bits() as i32;
                for i in 0..BLOCK_SIZE {
                    let curr_bits = prev_bits.wrapping_add(convert_buffer[i]);
                    self.current_buffer[i + 1] = f32::from_bits(curr_bits as u32);
                    prev_bits = curr_bits;
                }
            }
            PredictMethod::Fire => {
                self.fire_pred.reset();

                // First value: use current_buffer[0] as previous
                let prev_bits = self.current_buffer[0].to_bits() as i32;
                let pred = self.fire_pred.predict(prev_bits);
                let err = convert_buffer[0];
                let curr_bits = pred.wrapping_add(err);
                self.current_buffer[1] = f32::from_bits(curr_bits as u32);
                self.fire_pred.train(prev_bits, curr_bits, err);

                // Remaining values: use previously decoded value from current_buffer[i]
                for i in 1..BLOCK_SIZE {
                    let prev_bits_i = self.current_buffer[i].to_bits() as i32;
                    let pred = self.fire_pred.predict(prev_bits_i);
                    let err = convert_buffer[i];
                    let curr_bits = pred.wrapping_add(err);
                    self.current_buffer[i + 1] = f32::from_bits(curr_bits as u32);
                    self.fire_pred.train(prev_bits_i, curr_bits, err);
                }
            }
        }

        Ok(())
    }

    pub fn reset(&mut self) {
        self.current_buffer.fill(0.0);
        self.current_count = 0;
        self.decode_size = 0;
        self.is_block_read = false;
    }
}

impl Default for FloatSprintzDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_float_single_value() {
        let mut encoder = FloatSprintzEncoder::new();
        let mut out = Vec::new();

        encoder.encode(f32::MAX, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = FloatSprintzDecoder::new();
        let mut pos = 0;
        let val = decoder.read_float(&out, &mut pos).unwrap();
        assert_eq!(val, f32::MAX);
    }

    #[test]
    fn test_float_increasing() {
        let mut encoder = FloatSprintzEncoder::new();
        let mut out = Vec::new();

        // Use integer-based floats for better precision
        for i in 0..100 {
            encoder.encode(10.0 + i as f32, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = FloatSprintzDecoder::new();
        let mut pos = 0;

        for i in 0..100 {
            let val = decoder.read_float(&out, &mut pos).unwrap();
            let expected = 10.0 + i as f32;
            assert!(
                (val - expected).abs() < 0.001,
                "Failed at i={}: expected={}, got={}",
                i,
                expected,
                val
            );
        }
    }

    #[test]
    fn test_float_special_values() {
        let values = vec![f32::MIN, f32::MAX, -0.0f32, 0.0f32, f32::NAN];
        let mut encoder = FloatSprintzEncoder::new();
        let mut out = Vec::new();

        for &v in &values {
            encoder.encode(v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = FloatSprintzDecoder::new();
        let mut pos = 0;

        for &expected in &values {
            let actual = decoder.read_float(&out, &mut pos).unwrap();
            if expected.is_nan() {
                assert!(actual.is_nan());
            } else {
                assert_eq!(actual, expected);
            }
        }
    }
}
