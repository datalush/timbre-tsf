//! Compression algorithms for reducing TsFile storage size.
//!
//! This module provides compression/decompression implementations for the TsFile format.
//! Compression is applied after encoding to further reduce storage size, especially effective
//! for already-encoded data that still contains patterns (e.g., Gorilla-encoded floats).
//!
//! # Supported algorithms
//!
//! - **Uncompressed**: No compression (pass-through)
//! - **Snappy**: Fast compression with moderate ratio, good for real-time ingestion
//! - **LZ4**: Very fast compression/decompression, configurable speed/ratio trade-off
//! - **GZIP**: Slower but higher compression ratio, good for archival data
//!
//! # Performance considerations
//!
//! The module provides two dispatch mechanisms:
//!
//! - **Dynamic dispatch**: [`create_compressor_boxed`] returns `Box<dyn Compressor>` for
//!   runtime flexibility (legacy API).
//! - **Static dispatch**: [`create_compressor`] returns [`CompressorImpl`] enum for better
//!   performance by eliminating vtable lookups (~5-10% faster in hot paths).
//!
//! For high-performance code paths (encoding/decoding loops), prefer static dispatch via
//! [`CompressorImpl`].
//!
//! # Design rationale
//!
//! LZ4 uses FAST mode (not HIGHCOMPRESSION) because:
//! - Gorilla encoding already provides excellent compression
//! - LZ4-HC is 3-10x slower with minimal extra gain on pre-encoded data
//! - Fast decompression is critical for query performance
//!
//! # Examples
//!
//! ```rust
//! use tsfile::compress::{create_compressor, Compressor};
//! use tsfile::common::CompressionType;
//!
//! // Static dispatch (faster)
//! let mut compressor = create_compressor(CompressionType::Lz4);
//! let data = b"Hello, World!".repeat(100);
//! let compressed = compressor.compress(&data).unwrap();
//! let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
//! assert_eq!(data.to_vec(), decompressed);
//! ```

mod compressor;

pub use compressor::*;

use crate::common::CompressionType;
use crate::error::{Result, TsFileError};

/// Trait for implementing compression algorithms.
///
/// This trait defines the interface for all compressor implementations in the TsFile format.
/// Implementations must be `Send + Sync` to support multi-threaded encoding/decoding.
///
/// # Methods
///
/// - [`compress`](Compressor::compress): Transforms input bytes to compressed representation
/// - [`decompress`](Compressor::decompress): Restores original bytes from compressed data
/// - [`compression_type`](Compressor::compression_type): Returns the algorithm identifier
///
/// # Implementation notes
///
/// Compressors take `&mut self` to allow stateful implementations (e.g., dictionary-based
/// compression), though most current implementations are stateless.
pub trait Compressor: Send + Sync {
    /// Compresses the input data.
    ///
    /// # Arguments
    ///
    /// * `input` - The uncompressed bytes to compress
    ///
    /// # Returns
    ///
    /// The compressed bytes, which may be larger than input for incompressible data.
    ///
    /// # Errors
    ///
    /// Returns [`TsFileError::CompressionError`] if compression fails.
    fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>>;

    /// Decompresses the input data.
    ///
    /// # Arguments
    ///
    /// * `input` - The compressed bytes to decompress
    /// * `uncompressed_size` - The expected size of decompressed output (hint for allocation)
    ///
    /// # Returns
    ///
    /// The decompressed bytes.
    ///
    /// # Errors
    ///
    /// Returns [`TsFileError::DecompressionError`] if:
    /// - Input is corrupted
    /// - Input is not valid compressed data
    /// - Decompressed size doesn't match expected size (for some algorithms)
    fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>>;

    /// Returns the compression algorithm type identifier.
    fn compression_type(&self) -> CompressionType;
}

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
/// - Gorilla/TS2DIFF encoding already reduces data size significantly
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
        lz4::block::compress(
            input,
            Some(lz4::block::CompressionMode::FAST(1)),
            false,
        )
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

/// Enum-based compressor for static dispatch optimization.
///
/// This enum wraps all compressor types and provides static dispatch via monomorphization,
/// eliminating the virtual function call overhead of `Box<dyn Compressor>`. This results
/// in ~5-10% better performance in tight encoding/decoding loops.
///
/// # Performance
///
/// Static dispatch via this enum avoids:
/// - Vtable pointer dereference (~1-2 cycles)
/// - Indirect function call (prevents inlining)
/// - Better compiler optimization (can inline compress/decompress)
///
/// # Usage
///
/// Use [`create_compressor`] instead of [`create_compressor_boxed`] for performance-critical
/// code paths.
///
/// # Examples
///
/// ```rust
/// use tsfile::compress::{CompressorImpl, create_compressor};
/// use tsfile::common::CompressionType;
///
/// let mut compressor = create_compressor(CompressionType::Lz4);
/// let data = vec![1, 2, 3, 4, 5];
/// let compressed = compressor.compress(&data).unwrap();
/// ```
pub enum CompressorImpl {
    Uncompressed(UncompressedCompressor),
    Snappy(SnappyCompressor),
    Lz4(Lz4Compressor),
    Gzip(GzipCompressor),
}

impl CompressorImpl {
    /// Compresses data using the selected algorithm.
    ///
    /// Marked `#[inline]` to allow the compiler to optimize away the enum dispatch
    /// and directly inline the underlying compressor's implementation.
    #[inline]
    pub fn compress(&mut self, input: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Uncompressed(c) => c.compress(input),
            Self::Snappy(c) => c.compress(input),
            Self::Lz4(c) => c.compress(input),
            Self::Gzip(c) => c.compress(input),
        }
    }

    /// Decompresses data using the selected algorithm.
    ///
    /// Marked `#[inline]` to allow the compiler to optimize away the enum dispatch
    /// and directly inline the underlying compressor's implementation.
    #[inline]
    pub fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>> {
        match self {
            Self::Uncompressed(c) => c.decompress(input, uncompressed_size),
            Self::Snappy(c) => c.decompress(input, uncompressed_size),
            Self::Lz4(c) => c.decompress(input, uncompressed_size),
            Self::Gzip(c) => c.decompress(input, uncompressed_size),
        }
    }

    /// Returns the compression algorithm type.
    #[inline]
    pub fn compression_type(&self) -> CompressionType {
        match self {
            Self::Uncompressed(c) => c.compression_type(),
            Self::Snappy(c) => c.compression_type(),
            Self::Lz4(c) => c.compression_type(),
            Self::Gzip(c) => c.compression_type(),
        }
    }
}

/// Creates a compressor instance with dynamic dispatch (legacy API).
///
/// Returns a boxed trait object that uses dynamic dispatch via vtable. This provides
/// runtime flexibility at the cost of ~5-10% performance overhead in tight loops.
///
/// # Arguments
///
/// * `compression_type` - The compression algorithm to use
///
/// # Returns
///
/// A boxed compressor implementing the [`Compressor`] trait.
///
/// # Fallback
///
/// If an unsupported compression type is specified, returns [`UncompressedCompressor`].
///
/// # Examples
///
/// ```rust
/// use tsfile::compress::{create_compressor_boxed, Compressor};
/// use tsfile::common::CompressionType;
///
/// let mut compressor = create_compressor_boxed(CompressionType::Snappy);
/// let data = b"Hello, World!";
/// let compressed = compressor.compress(data).unwrap();
/// ```
pub fn create_compressor_boxed(compression_type: CompressionType) -> Box<dyn Compressor> {
    match compression_type {
        CompressionType::Uncompressed => Box::new(UncompressedCompressor),
        CompressionType::Snappy => Box::new(SnappyCompressor),
        CompressionType::Lz4 => Box::new(Lz4Compressor),
        CompressionType::Gzip => Box::new(GzipCompressor::default()),
        _ => Box::new(UncompressedCompressor),
    }
}

/// Creates a compressor instance with static dispatch (recommended).
///
/// Returns a [`CompressorImpl`] enum that uses static dispatch via monomorphization,
/// providing ~5-10% better performance than [`create_compressor_boxed`] by eliminating
/// virtual function call overhead.
///
/// # Arguments
///
/// * `compression_type` - The compression algorithm to use
///
/// # Returns
///
/// A [`CompressorImpl`] enum wrapping the selected compressor.
///
/// # Fallback
///
/// If an unsupported compression type is specified, returns [`UncompressedCompressor`].
///
/// # Examples
///
/// ```rust
/// use tsfile::compress::create_compressor;
/// use tsfile::common::CompressionType;
///
/// let mut compressor = create_compressor(CompressionType::Lz4);
/// let data = vec![1, 2, 3, 4, 5];
/// let compressed = compressor.compress(&data).unwrap();
/// let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
/// assert_eq!(data, decompressed);
/// ```
pub fn create_compressor(compression_type: CompressionType) -> CompressorImpl {
    match compression_type {
        CompressionType::Uncompressed => CompressorImpl::Uncompressed(UncompressedCompressor),
        CompressionType::Snappy => CompressorImpl::Snappy(SnappyCompressor),
        CompressionType::Lz4 => CompressorImpl::Lz4(Lz4Compressor),
        CompressionType::Gzip => CompressorImpl::Gzip(GzipCompressor::default()),
        _ => CompressorImpl::Uncompressed(UncompressedCompressor),
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
