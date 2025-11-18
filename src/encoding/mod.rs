mod dictionary;
mod gorilla;
mod plain;
mod rle;
mod sprintz;
mod ts2diff;
mod zigzag;

pub use dictionary::*;
pub use gorilla::*;
pub use plain::*;
pub use rle::*;
pub use sprintz::*;
pub use ts2diff::*;
pub use zigzag::*;

use crate::common::{TSDataType, TSEncoding};
use crate::error::Result;

/// Trait para encoders
pub trait Encoder: Send + Sync {
    fn encode_bool(&mut self, value: bool, out: &mut Vec<u8>) -> Result<()>;
    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()>;
    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()>;
    fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()>;
    fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()>;
    fn encode_string(&mut self, value: &str, out: &mut Vec<u8>) -> Result<()>;
    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()>;
    fn encoding_type(&self) -> TSEncoding;
}

/// Trait para decoders
pub trait Decoder: Send + Sync {
    fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool>;
    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32>;
    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64>;
    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32>;
    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64>;
    fn read_string(&mut self, input: &[u8], pos: &mut usize) -> Result<String>;
    fn has_remaining(&self, input: &[u8], pos: usize) -> bool;
    fn encoding_type(&self) -> TSEncoding;
}

/// Factory para crear encoders
pub fn create_encoder(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Encoder> {
    match encoding {
        TSEncoding::Plain => Box::new(PlainEncoder::new(data_type)),
        TSEncoding::Dictionary => Box::new(DictionaryEncoder::new(data_type)),
        TSEncoding::Gorilla => Box::new(GorillaEncoder::new(data_type)),
        TSEncoding::Ts2Diff => Box::new(Ts2DiffEncoder::new(data_type)),
        TSEncoding::Rle => Box::new(RleEncoder::new(data_type)),
        TSEncoding::Zigzag => Box::new(ZigzagEncoder::new(data_type)),
        TSEncoding::Sprintz => Box::new(SprintzEncoder::new(data_type)),
        _ => Box::new(PlainEncoder::new(data_type)), // Fallback
    }
}

/// Factory para crear decoders
pub fn create_decoder(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Decoder> {
    match encoding {
        TSEncoding::Plain => Box::new(PlainDecoder::new(data_type)),
        TSEncoding::Dictionary => Box::new(DictionaryDecoder::new(data_type)),
        TSEncoding::Gorilla => Box::new(GorillaDecoder::new(data_type)),
        TSEncoding::Ts2Diff => Box::new(Ts2DiffDecoder::new(data_type)),
        TSEncoding::Rle => Box::new(RleDecoder::new(data_type)),
        TSEncoding::Zigzag => Box::new(ZigzagDecoder::new(data_type)),
        TSEncoding::Sprintz => Box::new(SprintzDecoder::new(data_type)),
        _ => Box::new(PlainDecoder::new(data_type)), // Fallback
    }
}
