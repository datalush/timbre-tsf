use super::base::*;
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;

const BLOCK_SIZE: usize = 8;
const GROUP_MAX: usize = 16;

/// Int64 Sprintz Encoder
pub struct Int64SprintzEncoder {
    values: Vec<i64>,
    byte_cache: Vec<u8>,
    group_num: usize,
    predict_method: PredictMethod,
    fire_pred: FireI64,
    is_first_cached: bool,
}

impl Int64SprintzEncoder {
    pub fn new() -> Self {
        Self {
            values: Vec::with_capacity(BLOCK_SIZE + 1),
            byte_cache: Vec::new(),
            group_num: 0,
            predict_method: PredictMethod::Fire,
            fire_pred: FireI64::new(2),
            is_first_cached: false,
        }
    }

    pub fn set_predict_method(&mut self, method: PredictMethod) {
        self.predict_method = method;
    }

    pub fn encode(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
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

    fn predict(&mut self, value: i64, prev: i64) -> i64 {
        let pred = match self.predict_method {
            PredictMethod::Delta => value.wrapping_sub(prev),
            PredictMethod::Fire => {
                let prediction = self.fire_pred.predict(prev);
                let err = value.wrapping_sub(prediction);
                self.fire_pred.train(prev, value, err);
                err
            }
        };

        zigzag_encode_i64(pred)
    }

    fn bit_pack(&mut self, pre_value: i64) -> Result<()> {
        let bit_width = get_max_bit_width_i64(&self.values[1..]);

        let mut pack_buf = Vec::new();
        pack_8values_i64(&self.values[1..], bit_width, &mut pack_buf);

        self.byte_cache.push(bit_width);
        write_var_uint64(pre_value as u64, &mut self.byte_cache)?;
        self.byte_cache.extend_from_slice(&pack_buf);

        Ok(())
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        self.flush_internal(out)?;

        if !self.values.is_empty() {
            let size = (self.values.len() as u8) | (1 << 7);
            out.push(size);

            for &val in &self.values {
                out.write_i64::<LittleEndian>(val)?;
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

impl Default for Int64SprintzEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Int64 Sprintz Decoder
pub struct Int64SprintzDecoder {
    current_buffer: Vec<i64>,
    current_count: usize,
    decode_size: usize,
    is_block_read: bool,
    predict_method: PredictMethod,
    fire_pred: FireI64,
}

impl Int64SprintzDecoder {
    pub fn new() -> Self {
        Self {
            current_buffer: vec![0; BLOCK_SIZE + 1],
            current_count: 0,
            decode_size: 0,
            is_block_read: false,
            predict_method: PredictMethod::Fire,
            fire_pred: FireI64::new(2),
        }
    }

    pub fn set_predict_method(&mut self, method: PredictMethod) {
        self.predict_method = method;
    }

    pub fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        (self.is_block_read && self.current_count < self.decode_size) || pos + 9 <= input.len()
    }

    pub fn read_int64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
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
                if *pos + 8 > input.len() {
                    return Err(TsFileError::DecodingError(
                        "Insufficient data for RLE block".to_string(),
                    ));
                }
                let mut cursor = Cursor::new(&input[*pos..*pos + 8]);
                self.current_buffer[i] = cursor.read_i64::<LittleEndian>()?;
                *pos += 8;
            }
        } else {
            self.decode_size = BLOCK_SIZE + 1;

            let pre_value = read_var_uint64(input, pos)? as i64;
            self.current_buffer[0] = pre_value;

            if *pos + bit_width as usize > input.len() {
                return Err(TsFileError::DecodingError(
                    "Insufficient data for packed values".to_string(),
                ));
            }

            let pack_buf = &input[*pos..*pos + bit_width as usize];
            *pos += bit_width as usize;

            let mut unpacked = Vec::new();
            unpack_8values_i64(pack_buf, bit_width, &mut unpacked);

            for i in 0..8 {
                self.current_buffer[i + 1] = unpacked[i];
            }

            self.recalculate()?;
        }

        self.is_block_read = true;
        Ok(())
    }

    fn recalculate(&mut self) -> Result<()> {
        for i in 1..=BLOCK_SIZE {
            self.current_buffer[i] = zigzag_decode_i64(self.current_buffer[i]);
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

impl Default for Int64SprintzDecoder {
    fn default() -> Self {
        Self::new()
    }
}

fn write_var_uint64(mut value: u64, out: &mut Vec<u8>) -> Result<()> {
    while value > 0x7F {
        out.push((value & 0x7F) as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
    Ok(())
}

fn read_var_uint64(input: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift = 0;

    loop {
        if *pos >= input.len() {
            return Err(TsFileError::DecodingError(
                "Insufficient data for varint".to_string(),
            ));
        }

        let byte = input[*pos];
        *pos += 1;

        value |= ((byte & 0x7F) as u64) << shift;

        if (byte & 0x80) == 0 {
            break;
        }

        shift += 7;
        if shift >= 64 {
            return Err(TsFileError::DecodingError("Varint too large".to_string()));
        }
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int64_single_value() {
        let mut encoder = Int64SprintzEncoder::new();
        let mut out = Vec::new();

        let value: i64 = (i32::MAX as i64) + 10;
        encoder.encode(value, &mut out).unwrap();
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int64SprintzDecoder::new();
        let mut pos = 0;
        let val = decoder.read_int64(&out, &mut pos).unwrap();
        assert_eq!(val, value);
    }

    #[test]
    fn test_int64_edge_values() {
        let values = vec![i64::MIN, -1, 0, 1, i64::MAX];
        let mut encoder = Int64SprintzEncoder::new();
        let mut out = Vec::new();

        for v in &values {
            encoder.encode(*v, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int64SprintzDecoder::new();
        let mut pos = 0;

        for &expected in &values {
            let actual = decoder.read_int64(&out, &mut pos).unwrap();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn test_int64_increasing() {
        let mut encoder = Int64SprintzEncoder::new();
        let mut out = Vec::new();

        for i in 0..100 {
            encoder.encode(7 + 2 * i, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = Int64SprintzDecoder::new();
        let mut pos = 0;

        for i in 0..100 {
            let val = decoder.read_int64(&out, &mut pos).unwrap();
            assert_eq!(val, 7 + 2 * i);
        }
    }
}
