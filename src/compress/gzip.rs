//! GZIP compressor implementation.

use super::Compressor;
use crate::common::CompressionType;
use crate::error::{Result, TimbreError};

/// GZIP compressor implementation with configurable compression level.
///
/// GZIP provides the highest compression ratio but at the cost of slower compression
/// and decompression. Best suited for archival data or scenarios where storage cost
/// dominates CPU cost.
///
/// # Performance
///
/// - Compression: ~10-50 MB/s (level 6, default)
/// - Decompression: ~100-300 MB/s
/// - Ratio: ~60-85% size reduction
///
/// # Compression levels
///
/// - Level 1-3: Faster compression, lower ratio
/// - Level 6 (default): Balanced speed/ratio
/// - Level 9: Maximum ratio, very slow compression
///
/// # Use cases
///
/// - Archival or cold storage
/// - Workloads with high storage costs and low read frequency
/// - Batch processing where compression time is less critical
pub struct GzipCompressor {
    compression_level: flate2::Compression,
}

impl GzipCompressor {
    /// Creates a new GZIP compressor with the specified compression level.
    ///
    /// # Arguments
    ///
    /// * `level` - Compression level (0-9, where 9 is maximum compression)
    pub fn new(level: u32) -> Self {
        Self {
            compression_level: flate2::Compression::new(level),
        }
    }
}

impl Default for GzipCompressor {
    /// Creates a GZIP compressor with level 6 (balanced speed/ratio).
    fn default() -> Self {
        Self::new(6)
    }
}

impl Compressor for GzipCompressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        use flate2::write::GzEncoder;
        use std::io::Write;

        let mut encoder = GzEncoder::new(Vec::new(), self.compression_level);
        encoder
            .write_all(input)
            .map_err(|e| TimbreError::CompressionError(e.to_string()))?;
        encoder
            .finish()
            .map_err(|e| TimbreError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let mut decoder = GzDecoder::new(input);
        let mut result = Vec::new();
        decoder
            .read_to_end(&mut result)
            .map_err(|e| TimbreError::DecompressionError(e.to_string()))?;
        Ok(result)
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Gzip
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gzip() {
        let mut compressor = GzipCompressor::default();
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }
}
