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

    /// Returns the size of data buffered internally by this encoder.
    /// For encoders that write directly to `out` (like PlainEncoder), this returns 0.
    /// For encoders that use internal buffers (like GorillaEncoder), this returns
    /// the size of buffered data that hasn't been flushed yet.
    fn buffered_size(&self) -> usize {
        0 // Default: no internal buffering
    }
}

/// OPT-2: Enum-based encoder for static dispatch (eliminates virtual calls)
/// BEFORE: Box<dyn Encoder> → 3 virtual calls per value (15-20 cycles overhead)
/// AFTER: EncoderImpl enum → direct dispatch via match (0 overhead + inlining)
pub enum EncoderImpl {
    Plain(PlainEncoder),
    Dictionary(DictionaryEncoder),
    Gorilla(GorillaEncoder),
    Ts2Diff(Ts2DiffEncoder),
    Rle(RleEncoder),
    Zigzag(ZigzagEncoder),
    Sprintz(SprintzEncoder),
}

impl EncoderImpl {
    #[inline]
    pub fn encode_bool(&mut self, value: bool, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_bool(value, out),
            Self::Dictionary(e) => e.encode_bool(value, out),
            Self::Gorilla(e) => e.encode_bool(value, out),
            Self::Ts2Diff(e) => e.encode_bool(value, out),
            Self::Rle(e) => e.encode_bool(value, out),
            Self::Zigzag(e) => e.encode_bool(value, out),
            Self::Sprintz(e) => e.encode_bool(value, out),
        }
    }

    #[inline]
    pub fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_i32(value, out),
            Self::Dictionary(e) => e.encode_i32(value, out),
            Self::Gorilla(e) => e.encode_i32(value, out),
            Self::Ts2Diff(e) => e.encode_i32(value, out),
            Self::Rle(e) => e.encode_i32(value, out),
            Self::Zigzag(e) => e.encode_i32(value, out),
            Self::Sprintz(e) => e.encode_i32(value, out),
        }
    }

    #[inline]
    pub fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_i64(value, out),
            Self::Dictionary(e) => e.encode_i64(value, out),
            Self::Gorilla(e) => e.encode_i64(value, out),
            Self::Ts2Diff(e) => e.encode_i64(value, out),
            Self::Rle(e) => e.encode_i64(value, out),
            Self::Zigzag(e) => e.encode_i64(value, out),
            Self::Sprintz(e) => e.encode_i64(value, out),
        }
    }

    #[inline]
    pub fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_f32(value, out),
            Self::Dictionary(e) => e.encode_f32(value, out),
            Self::Gorilla(e) => e.encode_f32(value, out),
            Self::Ts2Diff(e) => e.encode_f32(value, out),
            Self::Rle(e) => e.encode_f32(value, out),
            Self::Zigzag(e) => e.encode_f32(value, out),
            Self::Sprintz(e) => e.encode_f32(value, out),
        }
    }

    #[inline]
    pub fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_f64(value, out),
            Self::Dictionary(e) => e.encode_f64(value, out),
            Self::Gorilla(e) => e.encode_f64(value, out),
            Self::Ts2Diff(e) => e.encode_f64(value, out),
            Self::Rle(e) => e.encode_f64(value, out),
            Self::Zigzag(e) => e.encode_f64(value, out),
            Self::Sprintz(e) => e.encode_f64(value, out),
        }
    }

    #[inline]
    pub fn encode_string(&mut self, value: &str, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.encode_string(value, out),
            Self::Dictionary(e) => e.encode_string(value, out),
            Self::Gorilla(e) => e.encode_string(value, out),
            Self::Ts2Diff(e) => e.encode_string(value, out),
            Self::Rle(e) => e.encode_string(value, out),
            Self::Zigzag(e) => e.encode_string(value, out),
            Self::Sprintz(e) => e.encode_string(value, out),
        }
    }

    #[inline]
    pub fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Plain(e) => e.flush(out),
            Self::Dictionary(e) => e.flush(out),
            Self::Gorilla(e) => e.flush(out),
            Self::Ts2Diff(e) => e.flush(out),
            Self::Rle(e) => e.flush(out),
            Self::Zigzag(e) => e.flush(out),
            Self::Sprintz(e) => e.flush(out),
        }
    }

    #[inline]
    pub fn buffered_size(&self) -> usize {
        match self {
            Self::Plain(e) => e.buffered_size(),
            Self::Dictionary(e) => e.buffered_size(),
            Self::Gorilla(e) => e.buffered_size(),
            Self::Ts2Diff(e) => e.buffered_size(),
            Self::Rle(e) => e.buffered_size(),
            Self::Zigzag(e) => e.buffered_size(),
            Self::Sprintz(e) => e.buffered_size(),
        }
    }

    #[inline]
    pub fn encoding_type(&self) -> TSEncoding {
        match self {
            Self::Plain(e) => e.encoding_type(),
            Self::Dictionary(e) => e.encoding_type(),
            Self::Gorilla(e) => e.encoding_type(),
            Self::Ts2Diff(e) => e.encoding_type(),
            Self::Rle(e) => e.encoding_type(),
            Self::Zigzag(e) => e.encoding_type(),
            Self::Sprintz(e) => e.encoding_type(),
        }
    }
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

/// OPT-READ-5: Enum-based decoder for static dispatch (eliminates virtual calls in hot decode path)
/// CRITICAL for read performance - decoders are called millions of times per read
pub enum DecoderImpl {
    Plain(PlainDecoder),
    Dictionary(DictionaryDecoder),
    Gorilla(GorillaDecoder),
    Ts2Diff(Ts2DiffDecoder),
    Rle(RleDecoder),
    Zigzag(ZigzagDecoder),
    Sprintz(SprintzDecoder),
}

impl DecoderImpl {
    #[inline(always)]
    pub fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool> {
        match self {
            Self::Plain(d) => d.read_bool(input, pos),
            Self::Dictionary(d) => d.read_bool(input, pos),
            Self::Gorilla(d) => d.read_bool(input, pos),
            Self::Ts2Diff(d) => d.read_bool(input, pos),
            Self::Rle(d) => d.read_bool(input, pos),
            Self::Zigzag(d) => d.read_bool(input, pos),
            Self::Sprintz(d) => d.read_bool(input, pos),
        }
    }

    #[inline(always)]
    pub fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        match self {
            Self::Plain(d) => d.read_i32(input, pos),
            Self::Dictionary(d) => d.read_i32(input, pos),
            Self::Gorilla(d) => d.read_i32(input, pos),
            Self::Ts2Diff(d) => d.read_i32(input, pos),
            Self::Rle(d) => d.read_i32(input, pos),
            Self::Zigzag(d) => d.read_i32(input, pos),
            Self::Sprintz(d) => d.read_i32(input, pos),
        }
    }

    #[inline(always)]
    pub fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        match self {
            Self::Plain(d) => d.read_i64(input, pos),
            Self::Dictionary(d) => d.read_i64(input, pos),
            Self::Gorilla(d) => d.read_i64(input, pos),
            Self::Ts2Diff(d) => d.read_i64(input, pos),
            Self::Rle(d) => d.read_i64(input, pos),
            Self::Zigzag(d) => d.read_i64(input, pos),
            Self::Sprintz(d) => d.read_i64(input, pos),
        }
    }

    #[inline(always)]
    pub fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
        match self {
            Self::Plain(d) => d.read_f32(input, pos),
            Self::Dictionary(d) => d.read_f32(input, pos),
            Self::Gorilla(d) => d.read_f32(input, pos),
            Self::Ts2Diff(d) => d.read_f32(input, pos),
            Self::Rle(d) => d.read_f32(input, pos),
            Self::Zigzag(d) => d.read_f32(input, pos),
            Self::Sprintz(d) => d.read_f32(input, pos),
        }
    }

    #[inline(always)]
    pub fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64> {
        match self {
            Self::Plain(d) => d.read_f64(input, pos),
            Self::Dictionary(d) => d.read_f64(input, pos),
            Self::Gorilla(d) => d.read_f64(input, pos),
            Self::Ts2Diff(d) => d.read_f64(input, pos),
            Self::Rle(d) => d.read_f64(input, pos),
            Self::Zigzag(d) => d.read_f64(input, pos),
            Self::Sprintz(d) => d.read_f64(input, pos),
        }
    }

    #[inline(always)]
    pub fn read_string(&mut self, input: &[u8], pos: &mut usize) -> Result<String> {
        match self {
            Self::Plain(d) => d.read_string(input, pos),
            Self::Dictionary(d) => d.read_string(input, pos),
            Self::Gorilla(d) => d.read_string(input, pos),
            Self::Ts2Diff(d) => d.read_string(input, pos),
            Self::Rle(d) => d.read_string(input, pos),
            Self::Zigzag(d) => d.read_string(input, pos),
            Self::Sprintz(d) => d.read_string(input, pos),
        }
    }

    #[inline]
    pub fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        match self {
            Self::Plain(d) => d.has_remaining(input, pos),
            Self::Dictionary(d) => d.has_remaining(input, pos),
            Self::Gorilla(d) => d.has_remaining(input, pos),
            Self::Ts2Diff(d) => d.has_remaining(input, pos),
            Self::Rle(d) => d.has_remaining(input, pos),
            Self::Zigzag(d) => d.has_remaining(input, pos),
            Self::Sprintz(d) => d.has_remaining(input, pos),
        }
    }

    #[inline]
    pub fn encoding_type(&self) -> TSEncoding {
        match self {
            Self::Plain(d) => d.encoding_type(),
            Self::Dictionary(d) => d.encoding_type(),
            Self::Gorilla(d) => d.encoding_type(),
            Self::Ts2Diff(d) => d.encoding_type(),
            Self::Rle(d) => d.encoding_type(),
            Self::Zigzag(d) => d.encoding_type(),
            Self::Sprintz(d) => d.encoding_type(),
        }
    }
}

/// Factory para crear encoders (legacy - returns Box<dyn Encoder>)
pub fn create_encoder_boxed(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Encoder> {
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

/// OPT-2: Factory para crear encoders con static dispatch
pub fn create_encoder(encoding: TSEncoding, data_type: TSDataType) -> EncoderImpl {
    match encoding {
        TSEncoding::Plain => EncoderImpl::Plain(PlainEncoder::new(data_type)),
        TSEncoding::Dictionary => EncoderImpl::Dictionary(DictionaryEncoder::new(data_type)),
        TSEncoding::Gorilla => EncoderImpl::Gorilla(GorillaEncoder::new(data_type)),
        TSEncoding::Ts2Diff => EncoderImpl::Ts2Diff(Ts2DiffEncoder::new(data_type)),
        TSEncoding::Rle => EncoderImpl::Rle(RleEncoder::new(data_type)),
        TSEncoding::Zigzag => EncoderImpl::Zigzag(ZigzagEncoder::new(data_type)),
        TSEncoding::Sprintz => EncoderImpl::Sprintz(SprintzEncoder::new(data_type)),
        _ => EncoderImpl::Plain(PlainEncoder::new(data_type)), // Fallback
    }
}

/// Factory para crear decoders (legacy - returns Box<dyn Decoder>)
pub fn create_decoder_boxed(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Decoder> {
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

/// OPT-READ-5: Factory for static dispatch decoders (preferred for performance)
pub fn create_decoder(encoding: TSEncoding, data_type: TSDataType) -> DecoderImpl {
    match encoding {
        TSEncoding::Plain => DecoderImpl::Plain(PlainDecoder::new(data_type)),
        TSEncoding::Dictionary => DecoderImpl::Dictionary(DictionaryDecoder::new(data_type)),
        TSEncoding::Gorilla => DecoderImpl::Gorilla(GorillaDecoder::new(data_type)),
        TSEncoding::Ts2Diff => DecoderImpl::Ts2Diff(Ts2DiffDecoder::new(data_type)),
        TSEncoding::Rle => DecoderImpl::Rle(RleDecoder::new(data_type)),
        TSEncoding::Zigzag => DecoderImpl::Zigzag(ZigzagDecoder::new(data_type)),
        TSEncoding::Sprintz => DecoderImpl::Sprintz(SprintzDecoder::new(data_type)),
        _ => DecoderImpl::Plain(PlainDecoder::new(data_type)), // Fallback
    }
}
