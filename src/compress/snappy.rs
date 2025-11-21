//! Snappy compressor implementation.

use super::Compressor;
use crate::common::CompressionType;
use crate::error::{Result, TimbreError};

/// Snappy compressor implementation.
///
/// Snappy prioritizes speed over compression ratio, making it suitable for real-time
/// ingestion workloads. Typical compression ratios are 1.5-3x for time-series data.
///
/// # Performance
///
/// - Compression: ~250-500 MB/s
/// - Decompression: ~500-1000 MB/s
/// - Ratio: ~50-70% size reduction
///
/// # Use cases
///
/// - Real-time data ingestion
/// - Fast query paths where decompression latency matters
/// - Workloads with moderate storage requirements
pub struct SnappyCompressor;

impl Compressor for SnappyCompressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        snap::raw::Encoder::new()
            .compress_vec(input)
            .map_err(|e| TimbreError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        snap::raw::Decoder::new()
            .decompress_vec(input)
            .map_err(|e| TimbreError::DecompressionError(e.to_string()))
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Snappy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_snappy() {
        let mut compressor = SnappyCompressor;
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }
}
