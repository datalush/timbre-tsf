/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Common types and configuration for Arrow integration

use crate::common::{CompressionType, TSEncoding};

/// Configuration for Arrow ↔ TsFile conversion
#[derive(Debug, Clone)]
pub struct ArrowConversionConfig {
    /// Default compression type for all columns
    pub default_compression: CompressionType,

    /// Default encoding for each data type
    pub default_encoding_i32: TSEncoding,
    pub default_encoding_i64: TSEncoding,
    pub default_encoding_f32: TSEncoding,
    pub default_encoding_f64: TSEncoding,
    pub default_encoding_bool: TSEncoding,
    pub default_encoding_string: TSEncoding,

    /// Maximum number of rows per chunk
    pub max_rows_per_chunk: usize,

    /// Whether to use aligned chunks when possible
    pub use_aligned_chunks: bool,

    /// Buffer size for reading/writing
    pub buffer_size: usize,
}

impl Default for ArrowConversionConfig {
    fn default() -> Self {
        Self {
            default_compression: CompressionType::Lz4,
            default_encoding_i32: TSEncoding::DeltaOfDelta,
            default_encoding_i64: TSEncoding::DeltaOfDelta,
            default_encoding_f32: TSEncoding::Gorilla,
            default_encoding_f64: TSEncoding::Gorilla,
            default_encoding_bool: TSEncoding::Rle,
            default_encoding_string: TSEncoding::Dictionary,
            max_rows_per_chunk: 10_000,
            use_aligned_chunks: true,
            buffer_size: 8192,
        }
    }
}

impl ArrowConversionConfig {
    /// Create a new configuration with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a fast configuration optimized for write speed (less compression)
    pub fn fast() -> Self {
        Self {
            default_compression: CompressionType::Uncompressed,
            default_encoding_i32: TSEncoding::Plain,
            default_encoding_i64: TSEncoding::Plain,
            default_encoding_f32: TSEncoding::Plain,
            default_encoding_f64: TSEncoding::Plain,
            default_encoding_bool: TSEncoding::Plain,
            default_encoding_string: TSEncoding::Plain,
            max_rows_per_chunk: 100_000, // Larger chunks = less overhead
            use_aligned_chunks: true,
            buffer_size: 8192,
        }
    }

    /// Create a balanced configuration (good speed + decent compression)
    pub fn balanced() -> Self {
        Self {
            default_compression: CompressionType::Lz4,
            default_encoding_i32: TSEncoding::Plain,
            default_encoding_i64: TSEncoding::Plain,
            default_encoding_f32: TSEncoding::Plain,
            default_encoding_f64: TSEncoding::Plain,
            default_encoding_bool: TSEncoding::Rle,
            default_encoding_string: TSEncoding::Plain,
            max_rows_per_chunk: 50_000,
            use_aligned_chunks: true,
            buffer_size: 8192,
        }
    }

    /// Set the default compression type
    pub fn with_compression(mut self, compression: CompressionType) -> Self {
        self.default_compression = compression;
        self
    }

    /// Set the maximum rows per chunk
    pub fn with_max_rows_per_chunk(mut self, max_rows: usize) -> Self {
        self.max_rows_per_chunk = max_rows;
        self
    }

    /// Enable or disable aligned chunks
    pub fn with_aligned_chunks(mut self, use_aligned: bool) -> Self {
        self.use_aligned_chunks = use_aligned;
        self
    }

    /// Set buffer size for I/O operations
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.buffer_size = size;
        self
    }

    /// Set encoding for int32 columns
    pub fn with_i32_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_i32 = encoding;
        self
    }

    /// Set encoding for int64 columns
    pub fn with_i64_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_i64 = encoding;
        self
    }

    /// Set encoding for float32 columns
    pub fn with_f32_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_f32 = encoding;
        self
    }

    /// Set encoding for float64 columns
    pub fn with_f64_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_f64 = encoding;
        self
    }

    /// Set encoding for boolean columns
    pub fn with_bool_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_bool = encoding;
        self
    }

    /// Set encoding for string columns
    pub fn with_string_encoding(mut self, encoding: TSEncoding) -> Self {
        self.default_encoding_string = encoding;
        self
    }
}

/// Hints for selecting the best encoding for a column
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingHint {
    /// Timestamps (monotonically increasing)
    Timestamp,

    /// Floating point values (temperature, sensor readings)
    FloatingPoint,

    /// Integer counters (monotonically increasing)
    Counter,

    /// Integer values with small deltas
    IntegerWithSmallDeltas,

    /// Boolean flags
    Boolean,

    /// String values with high repetition (device IDs, tags)
    RepetitiveString,

    /// String values with low repetition
    UniqueString,

    /// Enum-like values (small set of possible values)
    Categorical,

    /// Unknown/default
    Unknown,
}

impl EncodingHint {
    /// Select the best encoding based on this hint
    pub fn select_encoding(&self) -> TSEncoding {
        match self {
            EncodingHint::Timestamp => TSEncoding::DeltaOfDelta,
            EncodingHint::FloatingPoint => TSEncoding::Gorilla,
            EncodingHint::Counter => TSEncoding::DeltaOfDelta,
            EncodingHint::IntegerWithSmallDeltas => TSEncoding::DeltaOfDelta,
            EncodingHint::Boolean => TSEncoding::Rle,
            EncodingHint::RepetitiveString => TSEncoding::Dictionary,
            EncodingHint::UniqueString => TSEncoding::Plain,
            EncodingHint::Categorical => TSEncoding::Dictionary,
            EncodingHint::Unknown => TSEncoding::Plain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = ArrowConversionConfig::default();
        assert_eq!(config.default_compression, CompressionType::Lz4);
        assert_eq!(config.max_rows_per_chunk, 10_000);
        assert!(config.use_aligned_chunks);
    }

    #[test]
    fn test_config_builder() {
        let config = ArrowConversionConfig::new()
            .with_compression(CompressionType::Snappy)
            .with_max_rows_per_chunk(5_000)
            .with_aligned_chunks(false)
            .with_i32_encoding(TSEncoding::Plain);

        assert_eq!(config.default_compression, CompressionType::Snappy);
        assert_eq!(config.max_rows_per_chunk, 5_000);
        assert!(!config.use_aligned_chunks);
        assert_eq!(config.default_encoding_i32, TSEncoding::Plain);
    }

    #[test]
    fn test_encoding_hint_selection() {
        assert_eq!(
            EncodingHint::Timestamp.select_encoding(),
            TSEncoding::DeltaOfDelta
        );
        assert_eq!(
            EncodingHint::FloatingPoint.select_encoding(),
            TSEncoding::Gorilla
        );
        assert_eq!(
            EncodingHint::RepetitiveString.select_encoding(),
            TSEncoding::Dictionary
        );
        assert_eq!(
            EncodingHint::Boolean.select_encoding(),
            TSEncoding::Rle
        );
    }
}
