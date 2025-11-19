//! TsFile format components
//!
//! This module defines the core TsFile format structures and utilities:
//!
//! - **Metadata**: File-level, chunk-level, and page-level metadata structures
//! - **Byte Stream**: Utilities for reading/writing binary data with specific endianness
//!
//! # TsFile Format
//!
//! A TsFile consists of:
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
pub mod metadata;

pub use byte_stream::*;
pub use metadata::*;
