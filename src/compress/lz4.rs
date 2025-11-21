//! LZ4 compressor implementation.

use super::Compressor;
use crate::common::CompressionType;
use crate::error::{Result, TimbreError};

/// LZ4 compressor implementation.
///
/// LZ4 provides excellent balance between speed and compression ratio. This implementation
/// uses FAST mode (not HIGHCOMPRESSION) because time-series data is typically already
/// encoded (e.g., with Gorilla), making the speed/ratio trade-off favor faster compression.
///
/// # Performance
///
/// - Compression: ~300-600 MB/s (FAST mode)
/// - Decompression: ~2000-4000 MB/s
/// - Ratio: ~50-75% size reduction (on top of encoding)
///
/// # Design choice: FAST vs HC mode
///
/// LZ4 FAST mode is used instead of HIGHCOMPRESSION because:
/// - Gorilla/DeltaOfDelta encoding already reduces data size significantly
/// - LZ4-HC(9) is 3-10x slower with only ~5% better ratio on pre-encoded data
/// - Decompression speed is identical between modes
/// - Fast compression enables better write throughput
///
/// # Use cases
///
/// - Default compression for most workloads
/// - High-throughput ingestion with good compression
/// - Fast query paths requiring low-latency decompression
pub struct Lz4Compressor;

impl Compressor for Lz4Compressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        // Use FAST(1) mode - optimal for pre-encoded time-series data
        // See module documentation for rationale
        lz4::block::compress(input, Some(lz4::block::CompressionMode::FAST(1)), false)
            .map_err(|e| TimbreError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>> {
        lz4::block::decompress(input, Some(uncompressed_size as i32))
            .map_err(|e| TimbreError::DecompressionError(e.to_string()))
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Lz4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lz4() {
        let mut compressor = Lz4Compressor;
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }
}
