//! Timbre format components
//!
//! This module defines the core Timbre format structures and utilities:
//!
//! - **Metadata**: File-level, chunk-level, and page-level metadata structures
//! - **Byte Stream**: Utilities for reading/writing binary data with specific endianness
//!
//! # Timbre Format
//!
//! A Timbre file consists of:
//! 1. Magic bytes (header)
//! 2. Data chunks (compressed time series data)
//! 3. Metadata index
//! 4. Bloom filters (optional)
//! 5. Footer with offsets
//!
//! The format is designed for:
//! - Efficient sequential writes
//! - Fast random reads using index
//! - Compatibility with Apache IoTDB

pub mod byte_stream;
pub mod dictionary;
pub mod metadata;
pub mod miniblock;

pub use byte_stream::*;
pub use dictionary::*;
pub use metadata::*;
pub use miniblock::*;
