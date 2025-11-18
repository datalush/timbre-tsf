//! # TsFile - Rust Implementation
//!
//! TsFile es un formato de archivo columnar para datos de series temporales,
//! diseñado para compresión eficiente, alto rendimiento de lectura/escritura,
//! y compatibilidad con varios frameworks como Spark y Flink.
//!
//! ## Características
//!
//! - **Almacenamiento Columnar**: Optimizado para datos de series temporales
//! - **Compresión Eficiente**: Soporte para LZ4, Snappy, GZIP
//! - **Encoding Especializado**: Gorilla, TS2DIFF, RLE, Plain
//! - **Alto Rendimiento**: Escritura y lectura por lotes (Tablet)
//! - **Compatible**: Formato compatible con implementaciones Java y C++
//!
//! ## Ejemplo Básico
//!
//! ```rust
//! use tsfile::common::*;
//!
//! // Crear un schema
//! let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float);
//!
//! // Crear un tablet para escritura por lotes
//! let mut tablet = Tablet::new(
//!     "device1",
//!     vec![schema],
//!     vec![ColumnCategory::Field],
//!     1000
//! );
//!
//! // Agregar datos
//! tablet.add_row(1000, vec![Some(TsValue::Float(25.5))]).unwrap();
//! tablet.add_row(2000, vec![Some(TsValue::Float(26.0))]).unwrap();
//! ```

#![warn(missing_docs)]
#![allow(dead_code)] // Temporalmente mientras completamos la implementación

pub mod common;
pub mod compress;
pub mod encoding;
pub mod error;
pub mod file;
pub mod index;
pub mod query;
pub mod reader;
pub mod writer;

// Re-exports principales
pub use common::*;
pub use compress::{Compressor, create_compressor};
pub use encoding::{Decoder, Encoder, create_decoder, create_encoder};
pub use error::{Result, TsFileError};

/// Constantes del formato TsFile
pub mod constants {
    /// Magic string al inicio del archivo
    pub const MAGIC_STRING: &[u8] = b"TsFile";
    /// Magic string al final del archivo
    pub const MAGIC_STRING_END: &[u8] = b"TsFile";
    /// Versión del formato
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
