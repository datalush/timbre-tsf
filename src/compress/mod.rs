//! Compression algorithms for reducing Timbre file storage size.
//!
//! This module provides compression/decompression implementations for the Timbre format.
//! Compression is applied after encoding to further reduce storage size, especially effective
//! for already-encoded data that still contains patterns (e.g., Gorilla-encoded floats).
//!
//! # Supported algorithms
//!
//! - **Uncompressed**: No compression (pass-through)
//! - **Zstd**: Default compression (level 3, 2-3x better than Snappy)
//! - **LZ4**: Very fast compression/decompression, low latency option
//! - **Snappy**: Ultra-fast compression with moderate ratio
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
//! use timbre_tsf::compress::{create_compressor, Compressor};
//! use timbre_tsf::common::CompressionType;
//!
//! // Static dispatch (faster)
//! let mut compressor = create_compressor(CompressionType::Lz4);
//! let data = b"Hello, World!".repeat(100);
//! let compressed = compressor.compress(&data).unwrap();
//! let decompressed = compressor.decompress(&compressed, data.len()).unwrap();
//! assert_eq!(data.to_vec(), decompressed);
//! ```

mod gzip;
mod lz4;
mod snappy;
mod uncompressed;
mod zstd;

pub use gzip::GzipCompressor;
pub use lz4::Lz4Compressor;
pub use snappy::SnappyCompressor;
pub use uncompressed::UncompressedCompressor;
pub use zstd::ZstdCompressor;

use crate::common::CompressionType;
use crate::error::Result;

/// Trait for implementing compression algorithms.
///
/// This trait defines the interface for all compressor implementations in the Timbre format.
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
    /// Returns [`TimbreError::CompressionError`] if compression fails.
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
    /// Returns [`TimbreError::DecompressionError`] if:
    /// - Input is corrupted
    /// - Input is not valid compressed data
    /// - Decompressed size doesn't match expected size (for some algorithms)
    fn decompress(&mut self, input: &[u8], uncompressed_size: usize) -> Result<Vec<u8>>;

    /// Returns the compression algorithm type identifier.
    fn compression_type(&self) -> CompressionType;
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
/// use timbre_tsf::compress::{CompressorImpl, create_compressor};
/// use timbre_tsf::common::CompressionType;
///
/// let mut compressor = create_compressor(CompressionType::Lz4);
/// let data = vec![1, 2, 3, 4, 5];
/// let compressed = compressor.compress(&data).unwrap();
/// ```
pub enum CompressorImpl {
    Uncompressed(UncompressedCompressor),
    Zstd(ZstdCompressor),
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
            Self::Zstd(c) => c.compress(input),
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
            Self::Zstd(c) => c.decompress(input, uncompressed_size),
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
            Self::Zstd(c) => c.compression_type(),
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
/// use timbre_tsf::compress::{create_compressor_boxed, Compressor};
/// use timbre_tsf::common::CompressionType;
///
/// let mut compressor = create_compressor_boxed(CompressionType::Snappy);
/// let data = b"Hello, World!";
/// let compressed = compressor.compress(data).unwrap();
/// ```
pub fn create_compressor_boxed(compression_type: CompressionType) -> Box<dyn Compressor> {
    match compression_type {
        CompressionType::Uncompressed => Box::new(UncompressedCompressor),
        CompressionType::Zstd => Box::new(ZstdCompressor::default()),
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
/// use timbre_tsf::compress::create_compressor;
/// use timbre_tsf::common::CompressionType;
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
        CompressionType::Zstd => CompressorImpl::Zstd(ZstdCompressor::default()),
        CompressionType::Snappy => CompressorImpl::Snappy(SnappyCompressor),
        CompressionType::Lz4 => CompressorImpl::Lz4(Lz4Compressor),
        CompressionType::Gzip => CompressorImpl::Gzip(GzipCompressor::default()),
        _ => CompressorImpl::Uncompressed(UncompressedCompressor),
    }
}
