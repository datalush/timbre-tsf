//! Timbre writing components
//!
//! This module provides the writer infrastructure for creating Timbre files. The
//! writing process follows a hierarchical structure:
//!
//! 1. **Timbre Writer**: Top-level interface for creating Timbre files
//! 2. **Chunk Writers**: Handle time series chunks (aligned or standard)
//! 3. **Page Writers**: Encode individual pages within chunks
//!
//! # Writing Process
//!
//! Data flows through the following stages:
//! ```text
//! Raw Values -> Page Writer -> Chunk Writer -> Timbre Writer -> File
//! ```
//!
//! Each stage applies encoding and compression according to the schema configuration.
//!
//! # Performance Optimizations
//!
//! - Pre-allocated buffers to reduce allocations
//! - Batch writing to minimize I/O operations
//! - Static dispatch for encoding selection
//! - LZ4 FAST compression for balance of speed and ratio

// pub mod aligned_chunk_writer; // Temporarily disabled for Timbre mini-blocks migration
pub mod chunk_writer;
pub mod file_writer;
pub mod io_writer;
pub mod page_writer;
pub mod page_writer_builder;

// pub use aligned_chunk_writer::*;
pub use chunk_writer::*;
pub use file_writer::*;
pub use io_writer::*;
pub use page_writer::*;
pub use page_writer_builder::*;
