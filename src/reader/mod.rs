//! TsFile reading components
//!
//! This module provides the reader infrastructure for accessing TsFile data.
//! Reading follows the inverse of the writing hierarchy:
//!
//! 1. **TsFile Reader**: Top-level interface for opening and querying TsFiles
//! 2. **Chunk Readers**: Extract time series chunks (aligned or standard)
//! 3. **Page Readers**: Decode individual pages within chunks
//!
//! # Reading Process
//!
//! Data flows through the following stages:
//! ```text
//! File → TsFile Reader → Chunk Reader → Page Reader → Decoded Values
//! ```
//!
//! Each stage handles decompression and decoding according to the file's schema.
//!
//! # Performance Optimizations
//!
//! - Metadata caching to avoid repeated I/O
//! - Batch decompression with LZ4
//! - Efficient bit reading for Gorilla encoding (30% faster)
//! - Index-based filtering to skip irrelevant chunks

pub mod aligned_chunk_reader;
pub mod chunk_reader;
pub mod page_reader;
pub mod tsfile_io_reader;
pub mod tsfile_reader;

pub use aligned_chunk_reader::*;
pub use chunk_reader::*;
pub use page_reader::*;
pub use tsfile_io_reader::*;
pub use tsfile_reader::*;
