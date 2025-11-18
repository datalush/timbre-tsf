mod compressor;

pub use compressor::*;

use crate::common::CompressionType;
use crate::error::{Result, TsFileError};

/// Trait para compresores
pub trait Compressor: Send + Sync {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>>;
    fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>>;
    fn compression_type(&self) -> CompressionType;
}

/// Compresor sin compresión (pass-through)
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

/// Compresor Snappy
pub struct SnappyCompressor;

impl Compressor for SnappyCompressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        snap::raw::Encoder::new()
            .compress_vec(input)
            .map_err(|e| TsFileError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        snap::raw::Decoder::new()
            .decompress_vec(input)
            .map_err(|e| TsFileError::DecompressionError(e.to_string()))
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Snappy
    }
}

/// Compresor LZ4
pub struct Lz4Compressor;

impl Compressor for Lz4Compressor {
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        lz4::block::compress(input, Some(lz4::block::CompressionMode::HIGHCOMPRESSION(9)), false)
            .map_err(|e| TsFileError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>> {
        lz4::block::decompress(input, Some(uncompressed_size as i32))
            .map_err(|e| TsFileError::DecompressionError(e.to_string()))
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Lz4
    }
}

/// Compresor GZIP
pub struct GzipCompressor {
    compression_level: flate2::Compression,
}

impl GzipCompressor {
    pub fn new(level: u32) -> Self {
        Self {
            compression_level: flate2::Compression::new(level),
        }
    }
}

impl Default for GzipCompressor {
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
            .map_err(|e| TsFileError::CompressionError(e.to_string()))?;
        encoder
            .finish()
            .map_err(|e| TsFileError::CompressionError(e.to_string()))
    }

    fn decompress(&mut self, input: &[u8], _uncompressed_size: usize) -> Result<Vec<u8>> {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let mut decoder = GzDecoder::new(input);
        let mut result = Vec::new();
        decoder
            .read_to_end(&mut result)
            .map_err(|e| TsFileError::DecompressionError(e.to_string()))?;
        Ok(result)
    }

    fn compression_type(&self) -> CompressionType {
        CompressionType::Gzip
    }
}

/// Factory para crear compresores
pub fn create_compressor(compression_type: CompressionType) -> Box<dyn Compressor> {
    match compression_type {
        CompressionType::Uncompressed => Box::new(UncompressedCompressor),
        CompressionType::Snappy => Box::new(SnappyCompressor),
        CompressionType::Lz4 => Box::new(Lz4Compressor),
        CompressionType::Gzip => Box::new(GzipCompressor::default()),
        _ => Box::new(UncompressedCompressor), // Fallback
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

    #[test]
    fn test_snappy() {
        let mut compressor = SnappyCompressor;
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }

    #[test]
    fn test_lz4() {
        let mut compressor = Lz4Compressor;
        let data = b"Hello, World! ".repeat(100);
        let compressed = compressor.compress(&data).unwrap();
        assert!(compressed.len() < data.len());
        let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
        assert_eq!(data, decompressed);
    }

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
