use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

/// Encoder RLE (Run-Length Encoding)
pub struct RleEncoder {
    data_type: TSDataType,
    previous_value: Option<i64>,
    run_length: i32,
}

impl RleEncoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            previous_value: None,
            run_length: 0,
        }
    }

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

/// Decoder RLE
pub struct RleDecoder {
    data_type: TSDataType,
    current_value: Option<i64>,
    remaining: i32,
}

impl RleDecoder {
    pub fn new(data_type: TSDataType) -> Self {
        Self {
            data_type,
            current_value: None,
            remaining: 0,
        }
    }

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
