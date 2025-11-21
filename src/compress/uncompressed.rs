//! No-op compressor that returns data unchanged.

use super::Compressor;
use crate::common::CompressionType;
use crate::error::Result;

/// No-op compressor that returns data unchanged.
///
/// Useful for testing, debugging, or when storage space is not a concern.
/// Has zero CPU overhead but provides no size reduction.
pub struct UncompressedCompressor;

impl Compressor for UncompressedCompressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        Ok(input.to_vec())
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Uncompressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uncompressed() {
        let mut compressor = UncompressedCompressor;
        let data = b"Hello, World!";
        let compressed = compressor.compress(data).unwrap();
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed.as_slice());
    }
}
