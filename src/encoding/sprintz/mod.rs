//! Sprintz encoding implementation
//!
//! Sprintz is a state-of-the-art lossless compression algorithm specifically
//! designed for integer and floating-point time series data. It achieves high
//! compression ratios through a combination of predictive coding, adaptive learning,
//! and efficient bit packing.
//!
//! # Algorithm Overview
//!
//! Sprintz operates in blocks of 8 values and uses a multi-stage approach:
//!
//! 1. **Prediction**: Choose between delta encoding or FIRE (Finite Impulse Response)
//! 2. **Residual encoding**: Compute prediction errors
//! 3. **Zigzag encoding**: Map signed residuals to unsigned values
//! 4. **Bit packing**: Pack values using the minimum bits required
//!
//! # FIRE Predictor
//!
//! The FIRE (Finite Impulse Response) predictor is an adaptive algorithm that learns
//! patterns in the data stream. Unlike simple delta encoding, FIRE maintains an
//! accumulator and adjusts its predictions based on observed errors, making it
//! effective for data with trends and patterns.
//!
//! # Performance Characteristics
//!
//! - **Encoding**: O(1) per value with block buffering
//! - **Decoding**: O(1) per value with block unpacking
//! - **Compression**: Excellent for regular patterns (better than Gorilla for integers)
//! - **Block size**: 8 values (optimal for SIMD operations and cache efficiency)
//!
//! # Compression Ratio
//!
//! - **Regular sequences**: 1-2 bits per value
//! - **Slowly changing data**: 2-4 bits per value
//! - **Random data**: Falls back to near-plain encoding
//!
//! # Data Type Support
//!
//! Sprintz provides specialized implementations for:
//! - `Int32`: 32-bit signed integers
//! - `Int64`: 64-bit signed integers
//! - `Float`: 32-bit IEEE 754 floating-point (via bit casting)
//! - `Double`: 64-bit IEEE 754 floating-point (via bit casting)
//!
//! # Example
//!
//! ```
//! use timbre_tsf::encoding::{SprintzEncoder, SprintzDecoder, Encoder, Decoder};
//!
//! use timbre_tsf::common::TSDataType;
//!
//! let mut encoder = SprintzEncoder::new(TSDataType::Int32);
//! let mut buffer = Vec::new();
//!
//! // Encode a sequence with regular pattern
//! for i in 0..100 {
//!     encoder.encode_i32(i * 10, &mut buffer).unwrap();
//! }
//! encoder.flush(&mut buffer).unwrap();
//!
//! let mut decoder = SprintzDecoder::new(TSDataType::Int32);
//! let mut pos = 0;
//! for i in 0..100 {
//!     assert_eq!(decoder.read_i32(&buffer, &mut pos).unwrap(), i * 10);
//! }
//! ```
//!
//! # References
//!
//! - Blalock & Guttag, "Sprintz: Time Series Compression for the Internet of Things", 2018

mod base;
mod double;
mod float;
mod int32;
mod int64;

pub use base::{FireI32, FireI64, PredictMethod};
pub use double::{DoubleSprintzDecoder, DoubleSprintzEncoder};
pub use float::{FloatSprintzDecoder, FloatSprintzEncoder};
pub use int32::{Int32SprintzDecoder, Int32SprintzEncoder};
pub use int64::{Int64SprintzDecoder, Int64SprintzEncoder};

use super::{Decoder, Encoder};
use crate::common::{TSDataType, TSEncoding};
use crate::error::{Result, TimbreError};

/// Sprintz encoder wrapper that dispatches to type-specific implementations
///
/// This wrapper implements the generic `Encoder` trait and delegates to
/// specialized encoders based on the data type. This design allows for
/// type-specific optimizations while maintaining a unified interface.
pub struct SprintzEncoder {
    int32_encoder: Option<Int32SprintzEncoder>,
    int64_encoder: Option<Int64SprintzEncoder>,
    float_encoder: Option<FloatSprintzEncoder>,
    double_encoder: Option<DoubleSprintzEncoder>,
}

impl SprintzEncoder {
    /// Creates a new Sprintz encoder for the specified data type
    pub fn new(data_type: TSDataType) -> Self {
        let (int32_encoder, int64_encoder, float_encoder, double_encoder) = match data_type {
            TSDataType::Int32 => (Some(Int32SprintzEncoder::new()), None, None, None),
            TSDataType::Int64 => (None, Some(Int64SprintzEncoder::new()), None, None),
            TSDataType::Float => (None, None, Some(FloatSprintzEncoder::new()), None),
            TSDataType::Double => (None, None, None, Some(DoubleSprintzEncoder::new())),
            _ => (None, None, None, None),
        };

        Self {
            int32_encoder,
            int64_encoder,
            float_encoder,
            double_encoder,
        }
    }
}

impl Encoder for SprintzEncoder {
    fn encode_bool(&mut self, _value: bool, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Sprintz not supported for booleans".to_string(),
        ))
    }

    fn encode_i32(&mut self, value: i32, out: &mut Vec<u8>) -> Result<()> {
        if let Some(encoder) = &mut self.int32_encoder {
            encoder.encode(value, out)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz encoder not initialized for Int32".to_string(),
            ))
        }
    }

    fn encode_i64(&mut self, value: i64, out: &mut Vec<u8>) -> Result<()> {
        if let Some(encoder) = &mut self.int64_encoder {
            encoder.encode(value, out)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz encoder not initialized for Int64".to_string(),
            ))
        }
    }

    fn encode_f32(&mut self, value: f32, out: &mut Vec<u8>) -> Result<()> {
        if let Some(encoder) = &mut self.float_encoder {
            encoder.encode(value, out)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz encoder not initialized for Float".to_string(),
            ))
        }
    }

    fn encode_f64(&mut self, value: f64, out: &mut Vec<u8>) -> Result<()> {
        if let Some(encoder) = &mut self.double_encoder {
            encoder.encode(value, out)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz encoder not initialized for Double".to_string(),
            ))
        }
    }

    fn encode_string(&mut self, _value: &str, _out: &mut Vec<u8>) -> Result<()> {
        Err(TimbreError::EncodingError(
            "Sprintz not supported for strings".to_string(),
        ))
    }

    fn flush(&mut self, out: &mut Vec<u8>) -> Result<()> {
        if let Some(encoder) = &mut self.int32_encoder {
            encoder.flush(out)
        } else if let Some(encoder) = &mut self.int64_encoder {
            encoder.flush(out)
        } else if let Some(encoder) = &mut self.float_encoder {
            encoder.flush(out)
        } else if let Some(encoder) = &mut self.double_encoder {
            encoder.flush(out)
        } else {
            Ok(())
        }
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Sprintz
    }
}

/// Sprintz decoder wrapper that dispatches to type-specific implementations
///
/// This wrapper implements the generic `Decoder` trait and delegates to
/// specialized decoders based on the data type, mirroring the encoder structure.
pub struct SprintzDecoder {
    int32_decoder: Option<Int32SprintzDecoder>,
    int64_decoder: Option<Int64SprintzDecoder>,
    float_decoder: Option<FloatSprintzDecoder>,
    double_decoder: Option<DoubleSprintzDecoder>,
}

impl SprintzDecoder {
    /// Creates a new Sprintz decoder for the specified data type
    pub fn new(data_type: TSDataType) -> Self {
        let (int32_decoder, int64_decoder, float_decoder, double_decoder) = match data_type {
            TSDataType::Int32 => (Some(Int32SprintzDecoder::new()), None, None, None),
            TSDataType::Int64 => (None, Some(Int64SprintzDecoder::new()), None, None),
            TSDataType::Float => (None, None, Some(FloatSprintzDecoder::new()), None),
            TSDataType::Double => (None, None, None, Some(DoubleSprintzDecoder::new())),
            _ => (None, None, None, None),
        };

        Self {
            int32_decoder,
            int64_decoder,
            float_decoder,
            double_decoder,
        }
    }
}

impl Decoder for SprintzDecoder {
    fn read_bool(&mut self, _input: &[u8], _pos: &mut usize) -> Result<bool> {
        Err(TimbreError::EncodingError(
            "Sprintz not supported for booleans".to_string(),
        ))
    }

    fn read_i32(&mut self, input: &[u8], pos: &mut usize) -> Result<i32> {
        if let Some(decoder) = &mut self.int32_decoder {
            decoder.read_int32(input, pos)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz decoder not initialized for Int32".to_string(),
            ))
        }
    }

    fn read_i64(&mut self, input: &[u8], pos: &mut usize) -> Result<i64> {
        if let Some(decoder) = &mut self.int64_decoder {
            decoder.read_int64(input, pos)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz decoder not initialized for Int64".to_string(),
            ))
        }
    }

    fn read_f32(&mut self, input: &[u8], pos: &mut usize) -> Result<f32> {
        if let Some(decoder) = &mut self.float_decoder {
            decoder.read_float(input, pos)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz decoder not initialized for Float".to_string(),
            ))
        }
    }

    fn read_f64(&mut self, input: &[u8], pos: &mut usize) -> Result<f64> {
        if let Some(decoder) = &mut self.double_decoder {
            decoder.read_double(input, pos)
        } else {
            Err(TimbreError::EncodingError(
                "Sprintz decoder not initialized for Double".to_string(),
            ))
        }
    }

    fn read_string(&mut self, _input: &[u8], _pos: &mut usize) -> Result<String> {
        Err(TimbreError::EncodingError(
            "Sprintz not supported for strings".to_string(),
        ))
    }

    fn has_remaining(&self, input: &[u8], pos: usize) -> bool {
        if let Some(decoder) = &self.int32_decoder {
            decoder.has_remaining(input, pos)
        } else if let Some(decoder) = &self.int64_decoder {
            decoder.has_remaining(input, pos)
        } else if let Some(decoder) = &self.float_decoder {
            decoder.has_remaining(input, pos)
        } else if let Some(decoder) = &self.double_decoder {
            decoder.has_remaining(input, pos)
        } else {
            false
        }
    }

    fn encoding_type(&self) -> TSEncoding {
        TSEncoding::Sprintz
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprintz_encoder_decoder_int32() {
        let mut encoder = SprintzEncoder::new(TSDataType::Int32);
        let mut out = Vec::new();

        for i in 0..100 {
            encoder.encode_i32(i * 10, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = SprintzDecoder::new(TSDataType::Int32);
        let mut pos = 0;

        for i in 0..100 {
            let val = decoder.read_i32(&out, &mut pos).unwrap();
            assert_eq!(val, i * 10);
        }
    }

    #[test]
    fn test_sprintz_encoder_decoder_float() {
        let mut encoder = SprintzEncoder::new(TSDataType::Float);
        let mut out = Vec::new();

        for i in 0..100 {
            encoder.encode_f32(10.5 + i as f32, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        let mut decoder = SprintzDecoder::new(TSDataType::Float);
        let mut pos = 0;

        for i in 0..100 {
            let val = decoder.read_f32(&out, &mut pos).unwrap();
            assert!((val - (10.5 + i as f32)).abs() < 0.001);
        }
    }
}
