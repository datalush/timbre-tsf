//! Timbre - High-Performance Columnar Format for IoT Time Series
//!
//! `timbre-tsf` is a next-generation columnar file format specifically designed for
//! efficient storage and querying of IoT time series data, combining proven architectural
//! principles with modern compression and indexing innovations.
//!
//! # Overview
//!
//! Timbre organizes time series data in a columnar hierarchy that enables:
//! - **Superior compression** through modern encodings (Chimp128, Simple8b) and Zstd
//! - **Fast queries** via ART indexes, inverted indexes, and multi-level bloom filters
//! - **Parallel processing** with mini-blocks for fine-grained parallelism
//! - **Zero-copy reads** with Arrow-native layout for maximum performance
//!
//! # Architecture
//!
//! The format follows a hierarchical structure:
//!
//! ```text
//! Timbre File (.timbre)
//! ├── File Header (128 bytes, TMB1 magic)
//! ├── Device Groups (per device/entity)
//! │   ├── Series Chunks (per measurement/metric)
//! │   │   └── Pages (64KB-1MB compressed)
//! │   │       └── Mini-Blocks (4-8 blocks, parallel decode)
//! │   └── ...
//! ├── Index Area (ART, inverted index, bloom filters)
//! └── Footer (128 bytes + TMB1 magic)
//! ```
//!
//! # Quick Start
//!
//! ## Writing Data
//!
//! ```rust,no_run
//! use timbre_tsf::common::*;
//! use timbre_tsf::writer::FileWriter;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! // Define schema for temperature measurements with modern Timbre encodings
//! let schema = MeasurementSchema::new(
//!     "temperature",
//!     TSDataType::Float,
//!     TSEncoding::Chimp128,  // Modern float encoding
//!     CompressionType::Zstd, // Default Zstd compression
//! );
//!
//! // Create writer and register schema
//! let mut writer = FileWriter::new("sensor.timbre")?;
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
//! use timbre_tsf::common::*;
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
//! use timbre_tsf::reader::FileReader;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut reader = FileReader::open("sensor.timbre")?;
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
//! - **CHIMP128**: State-of-the-art float encoding (5-15% better than Gorilla)
//! - **SIMPLE8B**: High-efficiency integer packing (10-100x improvement)
//! - **GORILLA**: XOR-based delta encoding for floating-point (Facebook)
//! - **TS_2DIFF**: Second-order delta encoding for timestamps/counters
//! - **RLE**: Run-length encoding for repetitive values
//! - **DICTIONARY**: Dictionary encoding with global + local dictionaries
//! - **ZIGZAG**: Signed integer optimization
//! - **SPRINTZ**: Advanced time series compression with bit packing
//!
//! ## Compression
//!
//! - **Zstd**: Default compression (level 3, 2-3x better than Snappy)
//! - **LZ4**: Low latency option
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
//! # Timbre Format
//!
//! This library implements the Timbre Time Series Format v1.0, a next-generation
//! columnar format optimized for IoT workloads. File extension: `.timbre`
//! MIME type: `application/vnd.timbre`
//!
//! # Modules
//!
//! - [`common`]: Core types, schemas, and data structures
//! - [`encoding`]: Data encoding implementations (Gorilla, DeltaOfDelta, etc.)
//! - [`compress`]: Compression algorithms (LZ4, Snappy, GZIP)
//! - [`writer`]: Timbre writing and serialization
//! - [`reader`]: Timbre reading and deserialization
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
pub mod utils;
pub mod writer;

// Re-export core types for convenience
pub use common::*;
pub use compress::{Compressor, create_compressor};
pub use encoding::{Decoder, Encoder, create_decoder, create_encoder};
pub use error::{Result, TimbreError};

/// Timbre format constants and magic numbers.
///
/// These constants define the binary format markers and version information
/// used to identify and validate Timbre format files (.timbre extension).
pub mod constants {
    /// Magic number marker for Timbre files (TMB1 - Timbre Binary v1).
    ///
    /// This 4-byte sequence appears at both the beginning (header) and end (footer)
    /// of every valid Timbre file, similar to Parquet's dual-marker approach.
    /// This enables fast integrity validation without reading the entire file.
    pub const MAGIC: &[u8] = b"TMB1";

    /// Major version number (1.x).
    pub const VERSION_MAJOR: u16 = 1;

    /// Minor version number (1.0).
    pub const VERSION_MINOR: u16 = 0;

    /// File header size in bytes (128 bytes, aligned).
    pub const HEADER_SIZE: usize = 128;

    /// File footer size in bytes (128 bytes + 4 byte magic).
    pub const FOOTER_SIZE: usize = 132;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_workflow() {
        // Schema
        let schema = MeasurementSchema::with_defaults("temp", TSDataType::Float);
        assert_eq!(schema.data_type, TSDataType::Float);
        assert_eq!(schema.encoding, TSEncoding::Chimp128);

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
