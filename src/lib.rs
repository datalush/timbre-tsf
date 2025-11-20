//! Apache TsFile - Columnar Storage for Time Series Data
//!
//! `tsfile` is a high-performance Rust implementation of the Apache TsFile columnar file format,
//! specifically designed for efficient storage and querying of time series data in IoT and
//! monitoring systems.
//!
//! # Overview
//!
//! TsFile organizes time series data in a columnar hierarchy that enables:
//! - **Efficient compression** through specialized encodings (Gorilla, TS2DIFF, RLE, SPRINTZ)
//! - **Fast queries** via bloom filters, statistics, and predicate pushdown
//! - **Batch operations** using the Tablet API for high-throughput writes
//! - **Type safety** with compile-time guarantees through Rust's type system
//!
//! # Architecture
//!
//! The format follows a hierarchical structure:
//!
//! ```text
//! TsFile
//! ├── ChunkGroup (per device/entity)
//! │   ├── Chunk (per measurement/metric)
//! │   │   └── Page (compressed & encoded data blocks)
//! │   └── ...
//! └── Metadata (statistics, bloom filters, indices)
//! ```
//!
//! # Quick Start
//!
//! ## Writing Data
//!
//! ```rust,no_run
//! use tsfile_rs::common::*;
//! use tsfile_rs::writer::TsFileWriter;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Define schema for temperature measurements
//! let schema = MeasurementSchema::new(
//!     "temperature",
//!     TSDataType::Float,
//!     TSEncoding::Gorilla,
//!     CompressionType::Lz4,
//! );
//!
//! // Create writer and register schema
//! let mut writer = TsFileWriter::new("sensor.tsfile")?;
//! writer.register_timeseries("device_001", schema)?;
//!
//! // Write time series data
//! let record = TsRecord::new(1000, "device_001")
//!     .with_value("temperature", TsValue::Float(25.5));
//! writer.write_record(record)?;
//! writer.close()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Batch Writing with Tablet
//!
//! For high-throughput scenarios, use the Tablet API to write data in batches:
//!
//! ```rust
//! use tsfile_rs::common::*;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float);
//!
//! // Create tablet with buffer capacity
//! let mut tablet = Tablet::new(
//!     "device_001",
//!     vec![schema],
//!     vec![ColumnCategory::Field],
//!     1000, // buffer capacity
//! );
//!
//! // Add rows efficiently
//! tablet.add_row(1000, vec![Some(TsValue::Float(25.5))])?;
//! tablet.add_row(2000, vec![Some(TsValue::Float(26.0))])?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Reading Data
//!
//! ```rust,no_run
//! use tsfile_rs::reader::TsFileReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut reader = TsFileReader::open("sensor.tsfile")?;
//! let chunk = reader.read("device_001", "temperature")?;
//!
//! for (timestamp, value) in chunk.iter() {
//!     println!("{}: {:?}", timestamp, value);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Features
//!
//! ## Encodings
//!
//! - **PLAIN**: Direct encoding without transformation
//! - **RLE**: Run-length encoding for repetitive values
//! - **TS_2DIFF**: Second-order delta encoding for timestamps/counters
//! - **GORILLA**: XOR-based delta encoding for floating-point (Facebook)
//! - **DICTIONARY**: Dictionary encoding for string deduplication
//! - **ZIGZAG**: Signed integer optimization
//! - **SPRINTZ**: Advanced time series compression with bit packing
//!
//! ## Compression
//!
//! - **LZ4**: Balanced speed and compression ratio
//! - **Snappy**: Ultra-fast compression (Google)
//! - **GZIP**: Maximum compression ratio
//! - **Uncompressed**: No compression overhead
//!
//! ## Query Optimization
//!
//! The library implements a three-level optimization strategy:
//!
//! 1. **Bloom filters**: Skip chunks that definitely don't contain data
//! 2. **Statistics**: Skip chunks using min/max bounds
//! 3. **Row-level filtering**: Decode and filter remaining data
//!
//! # Performance
//!
//! This implementation includes several optimizations:
//!
//! - Zero-copy decoding where possible
//! - Batch processing via Tablet API
//! - Lazy metadata loading
//! - Efficient bit packing in SPRINTZ encoding
//! - Static dispatch for encoding/compression selection
//!
//! # Compatibility
//!
//! This library is binary compatible with Apache TsFile format version 2.1.0,
//! ensuring interoperability with Java and C++ implementations.
//!
//! # Modules
//!
//! - [`common`]: Core types, schemas, and data structures
//! - [`encoding`]: Data encoding implementations (Gorilla, TS2DIFF, etc.)
//! - [`compress`]: Compression algorithms (LZ4, Snappy, GZIP)
//! - [`writer`]: TsFile writing and serialization
//! - [`reader`]: TsFile reading and deserialization
//! - [`query`]: Query filters and predicates
//! - [`index`]: Bloom filters and indexing structures
//! - [`arrow`]: Apache Arrow integration
//! - [`file`]: Low-level file format and metadata

pub mod arrow;
pub mod common;
pub mod compress;
pub mod encoding;
pub mod error;
pub mod file;
pub mod index;
pub mod query;
pub mod reader;
pub mod writer;

// Re-export core types for convenience
pub use common::*;
pub use compress::{create_compressor, Compressor};
pub use encoding::{create_decoder, create_encoder, Decoder, Encoder};
pub use error::{Result, TsFileError};

/// TsFile format constants and magic numbers.
///
/// These constants define the binary format markers and version information
/// used to identify and validate TsFile format files.
pub mod constants {
    /// Magic string marker at the beginning of a TsFile.
    ///
    /// This 6-byte sequence must appear at offset 0 of every valid TsFile
    /// to identify the file format.
    pub const MAGIC_STRING: &[u8] = b"TsFile";

    /// Magic string marker at the end of a TsFile.
    ///
    /// This 6-byte sequence appears at the end of the file, immediately
    /// before the metadata footer, to validate file integrity.
    pub const MAGIC_STRING_END: &[u8] = b"TsFile";

    /// TsFile format version number.
    ///
    /// This implementation supports format version 3, which is compatible
    /// with Apache IoTDB 2.1.0 and later.
    pub const VERSION: u8 = 3;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_workflow() {
        // Schema
        let schema = MeasurementSchema::with_defaults("temp", TSDataType::Float);
        assert_eq!(schema.data_type, TSDataType::Float);
        assert_eq!(schema.encoding, TSEncoding::Gorilla);

        // Tablet
        let mut tablet = Tablet::new(
            "device1",
            vec![schema.clone()],
            vec![ColumnCategory::Field],
            10,
        );
        assert_eq!(tablet.column_count(), 1);

        // Add data
        tablet
            .add_row(1000, vec![Some(TsValue::Float(25.5))])
            .unwrap();
        assert_eq!(tablet.row_count(), 1);
    }

    #[test]
    fn test_encoding_compression() {
        // Test Plain encoder
        let mut encoder = create_encoder(TSEncoding::Plain, TSDataType::Int32);
        let mut out = Vec::new();
        encoder.encode_i32(42, &mut out).unwrap();
        assert!(!out.is_empty());

        // Test decoder
        let mut decoder = create_decoder(TSEncoding::Plain, TSDataType::Int32);
        let mut pos = 0;
        let value = decoder.read_i32(&out, &mut pos).unwrap();
        assert_eq!(value, 42);

        // Test compressor
        let mut compressor = create_compressor(CompressionType::Lz4);
        let data = b"Hello, World!";
        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data.as_slice(), decompressed.as_slice());
    }

    #[test]
    fn test_statistics() {
        use crate::common::statistic::*;

        let mut stat = Int32Statistic::new();
        stat.update_i32(1000, 10);
        stat.update_i32(2000, 20);
        stat.update_i32(3000, 5);

        assert_eq!(stat.count(), 3);
        assert_eq!(stat.start_time(), 1000);
        assert_eq!(stat.end_time(), 3000);
    }
}
