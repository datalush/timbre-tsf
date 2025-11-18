use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Encoder TS2DIFF (second-order difference) para series temporales
pub struct Ts2DiffEncoder {
    data_type: TSDataType,
    first_value: Option<i64>,
    previous_value: i64,
    previous_delta: i64,
}

impl Ts2DiffEncoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            first_value: None,
            previous_value: 0,
            previous_delta: 0,
        }
    }

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

/// Decoder TS2DIFF
pub struct Ts2DiffDecoder {
    data_type: TSDataType,
    first_value: Option<i64>,
    previous_value: i64,
    previous_delta: i64,
}

impl Ts2DiffDecoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            first_value: None,
            previous_value: 0,
            previous_delta: 0,
        }
    }

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
