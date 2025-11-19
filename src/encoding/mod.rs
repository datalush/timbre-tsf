//! Time-series data encoding algorithms for TsFiles.
//!
//! This module provides encoding and decoding implementations that transform time-series values
//! into compact binary representations. Encoding is the first stage of data reduction (before
//! compression) and exploits temporal patterns in time-series data.
//!
//! # Supported encodings
//!
//! - **PLAIN**: Unencoded binary representation (baseline)
//! - **DICTIONARY**: Maps repeated values to small indices (good for low cardinality)
//! - **RLE**: Run-length encoding for sequences of identical values
//! - **ZIGZAG**: Efficient encoding of small signed integers
//! - **TS2DIFF**: Delta-of-delta encoding for timestamps
//! - **GORILLA**: Facebook's Gorilla algorithm for floating-point values (excellent compression)
//! - **SPRINTZ**: Forecast-based encoding for numeric time-series
//!
//! # Design: Static vs Dynamic dispatch
//!
//! The module provides two dispatch mechanisms for performance optimization:
//!
//! - **Dynamic dispatch**: `Box<dyn Encoder/Decoder>` via [`create_encoder_boxed`] /
//!   [`create_decoder_boxed`] (legacy API)
//! - **Static dispatch**: [`EncoderImpl`] / [`DecoderImpl`] enums via [`create_encoder`] /
//!   [`create_decoder`] (recommended)
//!
//! Static dispatch eliminates virtual function calls (vtable lookups), providing ~15-20%
//! better performance in encoding/decoding hot paths. Decoders use `#[inline(always)]` since
//! they're called millions of times during reads.
//!
//! # Examples
//!
//! ```rust
//! use tsfile::encoding::{create_encoder, Encoder};
//! use tsfile::common::{TSEncoding, TSDataType};
//!
//! // Create a Gorilla encoder for float data (static dispatch)
//! let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
//! let mut output = Vec::new();
//!
//! // Encode values
//! encoder.encode_f32(22.5, &mut output).unwrap();
//! encoder.encode_f32(22.7, &mut output).unwrap();
//! encoder.flush(&mut output).unwrap();
//!
//! // Encoded data is now in `output`
//! ```

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

/// Trait for encoding time-series values into compact binary format.
///
/// Encoders transform typed values (bool, i32, f32, etc.) into byte sequences that exploit
/// temporal patterns in time-series data. Each encoding algorithm has different performance
/// characteristics and compression ratios depending on data patterns.
///
/// # Lifecycle
///
/// 1. Create encoder via [`create_encoder`] or [`create_encoder_boxed`]
/// 2. Call `encode_*` methods for each value
/// 3. Call [`flush`](Encoder::flush) to write any buffered data
/// 4. Repeat steps 2-3 for multiple pages/chunks
///
/// # Buffering
///
/// Some encoders (e.g., Gorilla) buffer data internally for better compression. Use
/// [`buffered_size`](Encoder::buffered_size) to check buffer size and [`flush`](Encoder::flush)
/// to write buffered data to output.
///
/// # Thread safety
///
/// Encoders are `Send + Sync` to support multi-threaded encoding of different columns/chunks.
pub trait Encoder: Send + Sync {
    /// Encodes a boolean value.
    fn encode_bool(&mut self, value: bool, out: &mut Vec<u8>) -> Result<()>;

    /// Encodes a 32-bit signed integer.
    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()>;

    /// Encodes a 64-bit signed integer.
    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()>;

    /// Encodes a 32-bit floating-point value.
    fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()>;

    /// Encodes a 64-bit floating-point value.
    fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()>;

    /// Encodes a string value.
    fn encode_string(&mut self, value: &str, out: &mut Vec<u8>) -> Result<()>;

    /// Flushes any buffered data to the output.
    ///
    /// Must be called after encoding the last value in a page/chunk to ensure all data
    /// is written. For non-buffering encoders (e.g., PLAIN), this is a no-op.
    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()>;

    /// Returns the encoding algorithm type.
    fn encoding_type(&self) -> TSEncoding;

    /// Returns the size of data buffered internally by this encoder.
    ///
    /// For encoders that write directly to `out` (like PlainEncoder), this returns 0.
    /// For encoders that use internal buffers (like GorillaEncoder), this returns
    /// the size of buffered data that hasn't been flushed yet.
    ///
    /// This is useful for determining when to flush to avoid excessive memory usage.
    fn buffered_size(&self) -> usize {
        0 // Default: no internal buffering
    }
}

/// Enum-based encoder for static dispatch optimization.
///
/// This enum wraps all encoder types and provides static dispatch via monomorphization,
/// eliminating virtual function call overhead. This is critical for write performance
/// where encoders are called millions of times.
///
/// # Performance impact
///
/// - **Before** (dynamic): `Box<dyn Encoder>` → 3 virtual calls per value (~15-20 cycles overhead)
/// - **After** (static): `EncoderImpl` enum → direct dispatch via match (0 overhead + inlining)
///
/// The `#[inline]` attributes allow the compiler to optimize away the enum dispatch and
/// directly inline the underlying encoder's implementation.
///
/// # Usage
///
/// Prefer [`create_encoder`] over [`create_encoder_boxed`] for performance-critical code paths.
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

/// Trait for decoding time-series values from binary format.
///
/// Decoders reverse the encoding process, reading values from byte sequences. They maintain
/// internal state (e.g., previous values for delta encoding) across multiple read calls.
///
/// # Lifecycle
///
/// 1. Create decoder via [`create_decoder`] or [`create_decoder_boxed`]
/// 2. Call `read_*` methods to decode values sequentially
/// 3. Check [`has_remaining`](Decoder::has_remaining) to detect end of data
///
/// # Position tracking
///
/// Decoders use a mutable position parameter (`pos: &mut usize`) that tracks the current
/// read offset in the input buffer. This allows stateless input handling while maintaining
/// decoder state for delta/differential encodings.
///
/// # Thread safety
///
/// Decoders are `Send + Sync` to support multi-threaded decoding of different columns/chunks.
pub trait Decoder: Send + Sync {
    /// Decodes a boolean value from the input.
    fn read_bool(&mut self, input: &[u8], pos: &mut usize) -> Result<bool>;

    /// Decodes a 32-bit signed integer from the input.
    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32>;

    /// Decodes a 64-bit signed integer from the input.
    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64>;

    /// Decodes a 32-bit floating-point value from the input.
    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32>;

    /// Decodes a 64-bit floating-point value from the input.
    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64>;

    /// Decodes a string value from the input.
    fn read_string(&mut self, input: &[u8], pos: &mut usize) -> Result<String>;

    /// Checks if there is more data to decode.
    ///
    /// Returns `true` if the current position is before the end of input.
    fn has_remaining(&self, input: &[u8], pos: usize) -> bool;

    /// Returns the encoding algorithm type.
    fn encoding_type(&self) -> TSEncoding;
}

/// Enum-based decoder for static dispatch optimization.
///
/// This enum wraps all decoder types and provides static dispatch via monomorphization.
/// This is CRITICAL for read performance as decoders are called millions of times during
/// query execution.
///
/// # Performance impact
///
/// Dynamic dispatch (`Box<dyn Decoder>`) has significant overhead in read paths:
/// - Vtable pointer dereference (~2 cycles)
/// - Indirect function call (prevents inlining)
/// - Poor branch prediction across different decoder types
///
/// Static dispatch via this enum eliminates these overheads and allows aggressive inlining.
///
/// # Inlining strategy
///
/// The `#[inline(always)]` attribute is used on read methods (not just `#[inline]`) because:
/// - These are hot-path functions called millions of times
/// - Inlining allows further optimizations (e.g., loop unrolling in caller)
/// - The compiler might not inline across crate boundaries without the hint
///
/// # Usage
///
/// Always use [`create_decoder`] (not [`create_decoder_boxed`]) for query paths.
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

/// Creates an encoder instance with dynamic dispatch (legacy API).
///
/// Returns a boxed trait object that uses dynamic dispatch. Prefer [`create_encoder`]
/// for better performance (~15-20% faster encoding).
///
/// # Arguments
///
/// * `encoding` - The encoding algorithm to use
/// * `data_type` - The data type being encoded (determines type-specific behavior)
///
/// # Fallback
///
/// If an unsupported encoding type is specified, returns a [`PlainEncoder`].
pub fn create_encoder_boxed(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Encoder> {
    match encoding {
        TSEncoding::Plain => Box::new(PlainEncoder::new(data_type)),
        TSEncoding::Dictionary => Box::new(DictionaryEncoder::new(data_type)),
        TSEncoding::Gorilla => Box::new(GorillaEncoder::new(data_type)),
        TSEncoding::Ts2Diff => Box::new(Ts2DiffEncoder::new(data_type)),
        TSEncoding::Rle => Box::new(RleEncoder::new(data_type)),
        TSEncoding::Zigzag => Box::new(ZigzagEncoder::new(data_type)),
        TSEncoding::Sprintz => Box::new(SprintzEncoder::new(data_type)),
        _ => Box::new(PlainEncoder::new(data_type)),
    }
}

/// Creates an encoder instance with static dispatch (recommended).
///
/// Returns an [`EncoderImpl`] enum that uses static dispatch for ~15-20% better encoding
/// performance compared to [`create_encoder_boxed`].
///
/// # Arguments
///
/// * `encoding` - The encoding algorithm to use
/// * `data_type` - The data type being encoded (determines type-specific behavior)
///
/// # Fallback
///
/// If an unsupported encoding type is specified, returns a [`PlainEncoder`].
///
/// # Examples
///
/// ```rust
/// use tsfile::encoding::create_encoder;
/// use tsfile::common::{TSEncoding, TSDataType};
///
/// let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
/// let mut output = Vec::new();
/// encoder.encode_f32(22.5, &mut output).unwrap();
/// encoder.flush(&mut output).unwrap();
/// ```
pub fn create_encoder(encoding: TSEncoding, data_type: TSDataType) -> EncoderImpl {
    match encoding {
        TSEncoding::Plain => EncoderImpl::Plain(PlainEncoder::new(data_type)),
        TSEncoding::Dictionary => EncoderImpl::Dictionary(DictionaryEncoder::new(data_type)),
        TSEncoding::Gorilla => EncoderImpl::Gorilla(GorillaEncoder::new(data_type)),
        TSEncoding::Ts2Diff => EncoderImpl::Ts2Diff(Ts2DiffEncoder::new(data_type)),
        TSEncoding::Rle => EncoderImpl::Rle(RleEncoder::new(data_type)),
        TSEncoding::Zigzag => EncoderImpl::Zigzag(ZigzagEncoder::new(data_type)),
        TSEncoding::Sprintz => EncoderImpl::Sprintz(SprintzEncoder::new(data_type)),
        _ => EncoderImpl::Plain(PlainEncoder::new(data_type)),
    }
}

/// Creates a decoder instance with dynamic dispatch (legacy API).
///
/// Returns a boxed trait object that uses dynamic dispatch. Prefer [`create_decoder`]
/// for significantly better performance (critical for read paths).
///
/// # Arguments
///
/// * `encoding` - The encoding algorithm to decode
/// * `data_type` - The data type being decoded (determines type-specific behavior)
///
/// # Fallback
///
/// If an unsupported encoding type is specified, returns a [`PlainDecoder`].
pub fn create_decoder_boxed(encoding: TSEncoding, data_type: TSDataType) -> Box<dyn Decoder> {
    match encoding {
        TSEncoding::Plain => Box::new(PlainDecoder::new(data_type)),
        TSEncoding::Dictionary => Box::new(DictionaryDecoder::new(data_type)),
        TSEncoding::Gorilla => Box::new(GorillaDecoder::new(data_type)),
        TSEncoding::Ts2Diff => Box::new(Ts2DiffDecoder::new(data_type)),
        TSEncoding::Rle => Box::new(RleDecoder::new(data_type)),
        TSEncoding::Zigzag => Box::new(ZigzagDecoder::new(data_type)),
        TSEncoding::Sprintz => Box::new(SprintzDecoder::new(data_type)),
        _ => Box::new(PlainDecoder::new(data_type)),
    }
}

/// Creates a decoder instance with static dispatch (strongly recommended).
///
/// Returns a [`DecoderImpl`] enum that uses static dispatch. This is CRITICAL for read
/// performance as it eliminates vtable overhead in the hot decoding path.
///
/// # Arguments
///
/// * `encoding` - The encoding algorithm to decode
/// * `data_type` - The data type being decoded (determines type-specific behavior)
///
/// # Fallback
///
/// If an unsupported encoding type is specified, returns a [`PlainDecoder`].
///
/// # Examples
///
/// ```rust
/// use tsfile::encoding::create_decoder;
/// use tsfile::common::{TSEncoding, TSDataType};
///
/// let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
/// let input = vec![/* encoded data */];
/// let mut pos = 0;
/// // let value = decoder.read_f32(&input, &mut pos).unwrap();
/// ```
pub fn create_decoder(encoding: TSEncoding, data_type: TSDataType) -> DecoderImpl {
    match encoding {
        TSEncoding::Plain => DecoderImpl::Plain(PlainDecoder::new(data_type)),
        TSEncoding::Dictionary => DecoderImpl::Dictionary(DictionaryDecoder::new(data_type)),
        TSEncoding::Gorilla => DecoderImpl::Gorilla(GorillaDecoder::new(data_type)),
        TSEncoding::Ts2Diff => DecoderImpl::Ts2Diff(Ts2DiffDecoder::new(data_type)),
        TSEncoding::Rle => DecoderImpl::Rle(RleDecoder::new(data_type)),
        TSEncoding::Zigzag => DecoderImpl::Zigzag(ZigzagDecoder::new(data_type)),
        TSEncoding::Sprintz => DecoderImpl::Sprintz(SprintzDecoder::new(data_type)),
        _ => DecoderImpl::Plain(PlainDecoder::new(data_type)),
    }
}
