//! Zstd compressor implementation.

use super::Compressor;
use crate::common::CompressionType;
use crate::error::{Result, TimbreError};

/// Zstd compressor implementation with configurable compression level.
///
/// Zstd (Zstandard) is the default compression algorithm for Timbre, providing
/// excellent balance between speed and compression ratio. It achieves 2-3x better
/// compression than Snappy while maintaining competitive speed.
///
/// # Performance
///
/// - Compression: ~200-400 MB/s (level 3, default)
/// - Decompression: ~600-1200 MB/s
/// - Ratio: ~65-80% size reduction
///
/// # Compression levels
///
/// - Level 1: Fastest, lower ratio (~Snappy-like speed)
/// - Level 3 (default): Balanced speed/ratio (recommended)
/// - Level 9-19: Higher ratios, slower compression
///
/// # Use cases
///
/// - Default compression for Timbre files
/// - High-throughput ingestion with excellent compression
/// - Most workloads benefit from this algorithm
pub struct ZstdCompressor {
    compression_level: i32,
}

impl ZstdCompressor {
    /// Creates a new Zstd compressor with the specified compression level.
    ///
    /// # Arguments
    ///
    /// * `level` - Compression level (1-22, where 3 is default for Timbre)
    pub fn new(level: i32) -> Self {
        Self {
            compression_level: level,
        }
    }
}

impl Default for ZstdCompressor {
    /// Creates a Zstd compressor with level 3 (Timbre default).
    fn default() -> Self {
        Self::new(3)
    }
}

impl Compressor for ZstdCompressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        zstd::bulk::compress(input, self.compression_level)
            .map_err(|e| TimbreError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        zstd::bulk::decompress(input, _uncompressed_size)
            .map_err(|e| TimbreError::DecompressionError(e.to_string()))
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Zstd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zstd() {
        let mut compressor = ZstdCompressor::default();
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }
}
