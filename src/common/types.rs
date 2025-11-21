//! Core type definitions for Timbre data model.
//!
//! This module defines the fundamental enumerations that describe data types,
//! encodings, compression methods, and value representations in the Timbre format.
//!
//! # Type System
//!
//! The Timbre type system consists of three orthogonal dimensions:
//!
//! 1. **Data Type** ([`TSDataType`]): The logical type of the data (Int32, Float, etc.)
//! 2. **Encoding** ([`TSEncoding`]): How the data is transformed before compression
//! 3. **Compression** ([`CompressionType`]): How the encoded data is compressed
//!
//! These three dimensions can be combined independently, though certain combinations
//! are more efficient than others. See [`TSEncoding::recommended_for`] and
//! [`CompressionType::recommended_for`] for recommended pairings.
//!
//! # Examples
//!
//! ```rust
//! use timbre_tsf::common::*;
//!
//! // Get recommended encoding for a data type
//! let encoding = TSEncoding::recommended_for(TSDataType::Float);
//! assert_eq!(encoding, TSEncoding::Chimp128);
//!
//! // Check size of fixed-size types
//! assert_eq!(TSDataType::Int32.size(), Some(4));
//! assert_eq!(TSDataType::Text.size(), None); // variable size
//!
//! // Create and inspect values
//! let value = TsValue::Float(25.5);
//! assert_eq!(value.data_type(), TSDataType::Float);
//! ```

use std::fmt;

/// Time series data types supported by Timbre.
///
/// This enum represents all logical data types that can be stored in a Timbre file.
/// Each type has a fixed byte discriminator used in the binary format for
/// serialization and deserialization.
///
/// # Fixed-Size vs Variable-Size Types
///
/// - **Fixed-size**: Boolean, Int32, Int64, Float, Double, Timestamp, Date
/// - **Variable-size**: Text, String, Blob, Vector
///
/// Use [`TSDataType::size()`] to get the size of fixed-size types.
///
/// # Wire Format
///
/// Each variant has a corresponding `u8` discriminator that appears in the
/// Timbre binary format. Use [`TSDataType::from_u8`] and [`TSDataType::to_u8`]
/// for conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TSDataType {
    /// Boolean value (true/false), stored as 1 byte.
    Boolean = 0,
    /// 32-bit signed integer, 4 bytes.
    Int32 = 1,
    /// 64-bit signed integer, 8 bytes.
    Int64 = 2,
    /// 32-bit IEEE 754 floating-point, 4 bytes.
    Float = 3,
    /// 64-bit IEEE 754 floating-point, 8 bytes.
    Double = 4,
    /// UTF-8 encoded text, variable length.
    Text = 5,
    /// Vector type (multidimensional data), variable length.
    Vector = 6,
    /// Unknown/unspecified type.
    Unknown = 7,
    /// Timestamp in milliseconds since epoch, 8 bytes.
    Timestamp = 8,
    /// Date value, 4 bytes.
    Date = 9,
    /// Binary large object, variable length.
    Blob = 10,
    /// String type (alternative to Text), variable length.
    String = 11,
    /// Null value marker.
    Null = 254,
    /// Invalid/unrecognized type marker.
    Invalid = 255,
}

impl TSDataType {
    /// Converts a byte value to a [`TSDataType`].
    ///
    /// If the byte doesn't match any known type, returns [`TSDataType::Invalid`].
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::TSDataType;
    ///
    /// assert_eq!(TSDataType::from_u8(1), TSDataType::Int32);
    /// assert_eq!(TSDataType::from_u8(3), TSDataType::Float);
    /// assert_eq!(TSDataType::from_u8(99), TSDataType::Invalid);
    /// ```
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Boolean,
            1 => Self::Int32,
            2 => Self::Int64,
            3 => Self::Float,
            4 => Self::Double,
            5 => Self::Text,
            6 => Self::Vector,
            7 => Self::Unknown,
            8 => Self::Timestamp,
            9 => Self::Date,
            10 => Self::Blob,
            11 => Self::String,
            254 => Self::Null,
            _ => Self::Invalid,
        }
    }

    /// Converts this [`TSDataType`] to its byte representation.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::TSDataType;
    ///
    /// assert_eq!(TSDataType::Int32.to_u8(), 1);
    /// assert_eq!(TSDataType::Double.to_u8(), 4);
    /// ```
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Returns the size in bytes for fixed-size types.
    ///
    /// For variable-size types (Text, String, Blob, Vector), returns `None`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::TSDataType;
    ///
    /// assert_eq!(TSDataType::Boolean.size(), Some(1));
    /// assert_eq!(TSDataType::Int32.size(), Some(4));
    /// assert_eq!(TSDataType::Double.size(), Some(8));
    /// assert_eq!(TSDataType::Text.size(), None);
    /// ```
    pub fn size(&self) -> Option<usize> {
        match self {
            Self::Boolean => Some(1),
            Self::Int32 | Self::Float | Self::Date => Some(4),
            Self::Int64 | Self::Double | Self::Timestamp => Some(8),
            _ => None,
        }
    }
}

impl fmt::Display for TSDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean => write!(f, "BOOLEAN"),
            Self::Int32 => write!(f, "INT32"),
            Self::Int64 => write!(f, "INT64"),
            Self::Float => write!(f, "FLOAT"),
            Self::Double => write!(f, "DOUBLE"),
            Self::Text => write!(f, "TEXT"),
            Self::Vector => write!(f, "VECTOR"),
            Self::Unknown => write!(f, "UNKNOWN"),
            Self::Timestamp => write!(f, "TIMESTAMP"),
            Self::Date => write!(f, "DATE"),
            Self::Blob => write!(f, "BLOB"),
            Self::String => write!(f, "STRING"),
            Self::Null => write!(f, "NULL"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Encoding methods supported by Timbre.
///
/// Encodings transform data before compression to improve compression ratios
/// and query performance. Different encodings are optimized for different
/// data characteristics:
///
/// - **Plain**: No transformation, direct storage
/// - **RLE**: Run-length encoding for repetitive values
/// - **TS_2DIFF**: Second-order delta encoding for timestamps and counters
/// - **Gorilla**: XOR-based delta encoding for floating-point values
/// - **Dictionary**: String deduplication via dictionary encoding
/// - **Zigzag**: Signed integer optimization using zigzag encoding
/// - **Sprintz**: Advanced time series compression with bit packing
///
/// # Performance Characteristics
///
/// | Encoding | Best For | Typical Ratio | Speed |
/// |----------|----------|---------------|-------|
/// | Plain | Random data | 1x | Fastest |
/// | RLE | Repetitive values | 8-16x | Very Fast |
/// | TS_2DIFF | Sequential values | 6-12x | Fast |
/// | Gorilla | Floats with small deltas | 3-6x | Fast |
/// | Dictionary | Repetitive strings | 10-50x | Medium |
/// | Sprintz | Correlated time series | 4-8x | Medium |
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::common::{TSDataType, TSEncoding};
///
/// // Get recommended encoding for a data type
/// let encoding = TSEncoding::recommended_for(TSDataType::Float);
/// assert_eq!(encoding, TSEncoding::Chimp128);
///
/// let encoding = TSEncoding::recommended_for(TSDataType::Boolean);
/// assert_eq!(encoding, TSEncoding::Rle);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TSEncoding {
    /// Plain encoding with no transformation.
    Plain = 0,
    /// Dictionary encoding for string deduplication.
    Dictionary = 1,
    /// Run-length encoding for repetitive values.
    Rle = 2,
    /// First-order delta encoding.
    Diff = 3,
    /// Delta-of-Delta encoding for timestamps/counters (second-order differencing).
    DeltaOfDelta = 4,
    /// Bitmap encoding.
    Bitmap = 5,
    /// Gorilla encoding version 1 (deprecated).
    GorillaV1 = 6,
    /// Regular encoding.
    Regular = 7,
    /// Gorilla encoding - XOR delta for floats (Facebook).
    Gorilla = 8,
    /// Zigzag encoding for signed integers.
    Zigzag = 9,
    /// Frequency-based encoding.
    Freq = 10,
    /// SPRINTZ encoding with bit packing for time series.
    Sprintz = 12,
    /// Chimp128 encoding - improved float encoding (5-15% better than Gorilla).
    Chimp128 = 13,
    /// Simple8b encoding - high-efficiency integer packing (10-100x improvement).
    Simple8b = 14,
    /// Quantized encoding - for regular step patterns (0.1°C resolution, etc).
    Quantized = 15,
    /// DictionaryRLE encoding - for high repetition with discrete values.
    DictionaryRLE = 16,
    /// Invalid/unrecognized encoding.
    Invalid = 255,
}

impl TSEncoding {
    /// Converts a byte value to a [`TSEncoding`].
    ///
    /// If the byte doesn't match any known encoding, returns [`TSEncoding::Invalid`].
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Plain,
            1 => Self::Dictionary,
            2 => Self::Rle,
            3 => Self::Diff,
            4 => Self::DeltaOfDelta,
            5 => Self::Bitmap,
            6 => Self::GorillaV1,
            7 => Self::Regular,
            8 => Self::Gorilla,
            9 => Self::Zigzag,
            10 => Self::Freq,
            12 => Self::Sprintz,
            13 => Self::Chimp128,
            14 => Self::Simple8b,
            _ => Self::Invalid,
        }
    }

    /// Converts this [`TSEncoding`] to its byte representation.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Returns the recommended encoding for a given data type.
    ///
    /// This method selects encodings that typically provide the best balance
    /// of compression ratio and performance for each data type in Timbre:
    ///
    /// - **Boolean**: RLE (excellent for sparse boolean flags)
    /// - **Int32/Int64**: Simple8b (10-100x improvement over plain)
    /// - **Timestamp**: DeltaOfDelta + Simple8b (optimal for time series)
    /// - **Float/Double**: Chimp128 (5-15% better than Gorilla)
    /// - **Text/String**: Dictionary (deduplicates repetitive strings)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::{TSDataType, TSEncoding};
    ///
    /// assert_eq!(
    ///     TSEncoding::recommended_for(TSDataType::Float),
    ///     TSEncoding::Chimp128
    /// );
    /// assert_eq!(
    ///     TSEncoding::recommended_for(TSDataType::Int32),
    ///     TSEncoding::Simple8b
    /// );
    /// ```
    pub fn recommended_for(data_type: TSDataType) -> Self {
        match data_type {
            TSDataType::Boolean => Self::Rle,
            TSDataType::Int32 | TSDataType::Date => Self::Simple8b,
            TSDataType::Int64 => Self::Simple8b,
            TSDataType::Timestamp => Self::DeltaOfDelta, // Timestamps use Delta-of-Delta + Simple8b
            TSDataType::Float | TSDataType::Double => Self::Chimp128,
            TSDataType::Text | TSDataType::String => Self::Dictionary,
            _ => Self::Plain,
        }
    }

    /// Parse an encoding from a string name (case-insensitive).
    ///
    /// Supports common encoding names like "plain", "gorilla", "chimp128", etc.
    /// Returns `None` if the string doesn't match any known encoding.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::TSEncoding;
    ///
    /// assert_eq!(TSEncoding::parse_encoding("gorilla"), Some(TSEncoding::Gorilla));
    /// assert_eq!(TSEncoding::parse_encoding("CHIMP128"), Some(TSEncoding::Chimp128));
    /// assert_eq!(TSEncoding::parse_encoding("plain"), Some(TSEncoding::Plain));
    /// assert_eq!(TSEncoding::parse_encoding("unknown"), None);
    /// ```
    pub fn parse_encoding(s: &str) -> Option<Self> {
        let s_upper = s.to_uppercase();
        match s_upper.as_str() {
            "PLAIN" => Some(Self::Plain),
            "DICTIONARY" | "DICT" => Some(Self::Dictionary),
            "RLE" => Some(Self::Rle),
            "DIFF" => Some(Self::Diff),
            "DELTAOFDELTA" | "DELTA_OF_DELTA" | "DOD" => Some(Self::DeltaOfDelta),
            "BITMAP" => Some(Self::Bitmap),
            "GORILLAV1" | "GORILLA_V1" => Some(Self::GorillaV1),
            "REGULAR" => Some(Self::Regular),
            "GORILLA" => Some(Self::Gorilla),
            "ZIGZAG" => Some(Self::Zigzag),
            "FREQ" => Some(Self::Freq),
            "SPRINTZ" => Some(Self::Sprintz),
            "CHIMP128" | "CHIMP" => Some(Self::Chimp128),
            "SIMPLE8B" | "SIMPLE_8B" => Some(Self::Simple8b),
            _ => None,
        }
    }
}

impl fmt::Display for TSEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => write!(f, "PLAIN"),
            Self::Dictionary => write!(f, "DICTIONARY"),
            Self::Rle => write!(f, "RLE"),
            Self::Diff => write!(f, "DIFF"),
            Self::DeltaOfDelta => write!(f, "DELTA_OF_DELTA"),
            Self::Bitmap => write!(f, "BITMAP"),
            Self::GorillaV1 => write!(f, "GORILLA_V1"),
            Self::Regular => write!(f, "REGULAR"),
            Self::Gorilla => write!(f, "GORILLA"),
            Self::Zigzag => write!(f, "ZIGZAG"),
            Self::Freq => write!(f, "FREQ"),
            Self::Sprintz => write!(f, "SPRINTZ"),
            Self::Chimp128 => write!(f, "CHIMP128"),
            Self::Simple8b => write!(f, "SIMPLE8B"),
            Self::Quantized => write!(f, "QUANTIZED"),
            Self::DictionaryRLE => write!(f, "DICTIONARY_RLE"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Compression algorithms supported by Timbre.
///
/// Compression is applied after encoding to further reduce data size. Timbre
/// supports several general-purpose compression algorithms with different
/// speed/ratio tradeoffs.
///
/// # Algorithm Characteristics
///
/// | Algorithm | Speed | Ratio | Use Case |
/// |-----------|-------|-------|----------|
/// | Uncompressed | Fastest | 1x | Debugging, already compressed data |
/// | Zstd | Fast | 2.5-4x | Default (recommended) |
/// | LZ4 | Very Fast | 2-3x | Low latency option |
/// | Snappy | Fastest | 1.5-2x | Maximum throughput |
/// | GZIP | Slow | 3-5x | Maximum compression |
///
/// # Recommendation
///
/// **Zstd level 3** is recommended for most use cases as it provides 2-3x better
/// compression than Snappy while maintaining competitive speed. It is the default
/// returned by [`CompressionType::recommended_for`].
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::common::{TSDataType, CompressionType};
///
/// // Get recommended compression (always Zstd)
/// let compression = CompressionType::recommended_for(TSDataType::Float);
/// assert_eq!(compression, CompressionType::Zstd);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CompressionType {
    /// No compression applied.
    Uncompressed = 0,
    /// Snappy compression (Google) - ultra fast.
    Snappy = 1,
    /// GZIP compression - maximum ratio.
    Gzip = 2,
    /// LZO compression (not implemented).
    Lzo = 3,
    /// SDT compression (not implemented).
    Sdt = 4,
    /// PAA compression (not implemented).
    Paa = 5,
    /// PLA compression (not implemented).
    Pla = 6,
    /// LZ4 compression - low latency option.
    Lz4 = 7,
    /// Zstd compression - default for Timbre (level 3, 2-3x better than Snappy).
    Zstd = 8,
    /// Invalid/unrecognized compression.
    Invalid = 255,
}

impl CompressionType {
    /// Converts a byte value to a [`CompressionType`].
    ///
    /// If the byte doesn't match any known compression type, returns
    /// [`CompressionType::Invalid`].
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Uncompressed,
            1 => Self::Snappy,
            2 => Self::Gzip,
            3 => Self::Lzo,
            4 => Self::Sdt,
            5 => Self::Paa,
            6 => Self::Pla,
            7 => Self::Lz4,
            8 => Self::Zstd,
            _ => Self::Invalid,
        }
    }

    /// Converts this [`CompressionType`] to its byte representation.
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Returns the recommended compression type for any data type.
    ///
    /// For Timbre, the default is [`CompressionType::Zstd`] at level 3, which provides
    /// 2-3x better compression than Snappy while maintaining competitive speed across
    /// all data types and encoding schemes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::{TSDataType, CompressionType};
    ///
    /// assert_eq!(
    ///     CompressionType::recommended_for(TSDataType::Float),
    ///     CompressionType::Zstd
    /// );
    /// ```
    pub fn recommended_for(_data_type: TSDataType) -> Self {
        // Zstd level 3 provides the best balance of speed and compression ratio for Timbre
        Self::Zstd
    }
}

impl fmt::Display for CompressionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncompressed => write!(f, "UNCOMPRESSED"),
            Self::Snappy => write!(f, "SNAPPY"),
            Self::Gzip => write!(f, "GZIP"),
            Self::Lzo => write!(f, "LZO"),
            Self::Sdt => write!(f, "SDT"),
            Self::Paa => write!(f, "PAA"),
            Self::Pla => write!(f, "PLA"),
            Self::Lz4 => write!(f, "LZ4"),
            Self::Zstd => write!(f, "ZSTD"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Column category in table-based data model.
///
/// Columns can be classified as tags (metadata/dimensions), fields (measurements),
/// or time (timestamp column). This categorization is used in table schemas to
/// organize data for efficient querying.
///
/// # Categories
///
/// - **Tag**: Metadata or dimension column (e.g., device_id, location)
/// - **Field**: Measurement or metric column (e.g., temperature, pressure)
/// - **Time**: Timestamp column (usually just one per table)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnCategory {
    /// Tag column - metadata or dimension.
    Tag,
    /// Field column - measurement or metric.
    Field,
    /// Time column - timestamp.
    Time,
}

/// Type-safe wrapper for time series values.
///
/// This enum can hold any value type supported by Timbre. It provides type
/// safety and convenient conversion between Rust types and Timbre types.
///
/// # Null Handling
///
/// The [`TsValue::Null`] variant represents missing values, which is distinct
/// from Rust's `Option<TsValue>`. Use `Option<TsValue>` to indicate presence
/// or absence of a value, and `TsValue::Null` to represent an explicit NULL
/// value in the data.
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::common::{TsValue, TSDataType};
///
/// let value = TsValue::Float(25.5);
/// assert_eq!(value.data_type(), TSDataType::Float);
///
/// let null_value = TsValue::Null;
/// assert_eq!(null_value.data_type(), TSDataType::Null);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum TsValue {
    /// Boolean value.
    Boolean(bool),
    /// 32-bit signed integer.
    Int32(i32),
    /// 64-bit signed integer.
    Int64(i64),
    /// 32-bit floating-point.
    Float(f32),
    /// 64-bit floating-point.
    Double(f64),
    /// UTF-8 text string.
    Text(String),
    /// String value (alternative to Text).
    String(String),
    /// Binary data.
    Blob(Vec<u8>),
    /// Null/missing value.
    Null,
}

impl TsValue {
    /// Returns the [`TSDataType`] of this value.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::{TsValue, TSDataType};
    ///
    /// assert_eq!(TsValue::Int32(42).data_type(), TSDataType::Int32);
    /// assert_eq!(TsValue::Float(3.14).data_type(), TSDataType::Float);
    /// assert_eq!(TsValue::Null.data_type(), TSDataType::Null);
    /// ```
    pub fn data_type(&self) -> TSDataType {
        match self {
            Self::Boolean(_) => TSDataType::Boolean,
            Self::Int32(_) => TSDataType::Int32,
            Self::Int64(_) => TSDataType::Int64,
            Self::Float(_) => TSDataType::Float,
            Self::Double(_) => TSDataType::Double,
            Self::Text(_) => TSDataType::Text,
            Self::String(_) => TSDataType::String,
            Self::Blob(_) => TSDataType::Blob,
            Self::Null => TSDataType::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_conversion() {
        assert_eq!(TSDataType::from_u8(0), TSDataType::Boolean);
        assert_eq!(TSDataType::Int32.to_u8(), 1);
        assert_eq!(TSDataType::Float.size(), Some(4));
        assert_eq!(TSDataType::Text.size(), None);
    }

    #[test]
    fn test_encoding_recommended() {
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Boolean),
            TSEncoding::Rle
        );
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Float),
            TSEncoding::Chimp128
        );
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Int32),
            TSEncoding::Simple8b
        );
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Timestamp),
            TSEncoding::DeltaOfDelta
        );
    }

    #[test]
    fn test_compression_recommended() {
        assert_eq!(
            CompressionType::recommended_for(TSDataType::Int32),
            CompressionType::Zstd
        );
    }
}
