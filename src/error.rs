//! Error types for TsFile operations.
//!
//! This module defines [`TsFileError`], a comprehensive error type that covers
//! all possible failure modes when working with TsFile format files. It uses
//! the `thiserror` crate for ergonomic error handling and implements proper
//! error chaining.
//!
//! # Examples
//!
//! ```rust
//! use timbre_tsf::{Result, TsFileError};
//!
//! fn validate_version(version: u8) -> Result<()> {
//!     if version != 3 {
//!         return Err(TsFileError::UnsupportedVersion(version));
//!     }
//!     Ok(())
//! }
//! ```

use std::io;
use thiserror::Error;

/// Comprehensive error type for all TsFile operations.
///
/// This enum covers error cases across encoding, compression, I/O, schema
/// validation, and file format parsing. Each variant includes contextual
/// information to aid in debugging and error reporting.
///
/// # Error Categories
///
/// - **I/O Errors**: File system and stream operations
/// - **Format Errors**: Invalid file structure or magic strings
/// - **Encoding/Decoding**: Data transformation failures
/// - **Compression**: Compression and decompression failures
/// - **Schema Errors**: Type mismatches and invalid schemas
/// - **State Errors**: Invalid operations for current state
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::TsFileError;
///
/// // Create type mismatch error
/// let err = TsFileError::TypeMismatch {
///     expected: "Float".to_string(),
///     actual: "Int32".to_string(),
/// };
///
/// // Pattern match on error type
/// match err {
///     TsFileError::TypeMismatch { expected, actual } => {
///         println!("Type error: expected {}, got {}", expected, actual);
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Error)]
pub enum TsFileError {
    /// I/O error from underlying file or stream operations.
    ///
    /// This variant wraps [`std::io::Error`] and is automatically converted
    /// using the `From` trait, allowing `?` operator usage with I/O operations.
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Invalid or unrecognized data type byte value.
    ///
    /// Occurs when reading a type discriminator that doesn't match any
    /// defined [`TSDataType`](crate::common::TSDataType) variant.
    #[error("Invalid data type: {0}")]
    InvalidDataType(u8),

    /// Invalid or unrecognized encoding type byte value.
    ///
    /// Occurs when reading an encoding discriminator that doesn't match any
    /// defined [`TSEncoding`](crate::common::TSEncoding) variant.
    #[error("Invalid encoding: {0}")]
    InvalidEncoding(u8),

    /// Invalid or unrecognized compression type byte value.
    ///
    /// Occurs when reading a compression discriminator that doesn't match any
    /// defined [`CompressionType`](crate::common::CompressionType) variant.
    #[error("Invalid compression: {0}")]
    InvalidCompression(u8),

    /// Error during data encoding operation.
    ///
    /// Contains a descriptive message about what went wrong during encoding,
    /// such as buffer overflow or invalid input values.
    #[error("Encoding error: {0}")]
    EncodingError(String),

    /// Error during data decoding operation.
    ///
    /// Contains a descriptive message about what went wrong during decoding,
    /// such as corrupted data or truncated input.
    #[error("Decoding error: {0}")]
    DecodingError(String),

    /// Error during compression operation.
    ///
    /// Typically occurs due to insufficient buffer space or invalid input data
    /// for the selected compression algorithm.
    #[error("Compression error: {0}")]
    CompressionError(String),

    /// Error during decompression operation.
    ///
    /// Usually indicates corrupted compressed data or incorrect decompressed
    /// size specification.
    #[error("Decompression error: {0}")]
    DecompressionError(String),

    /// Invalid TsFile format structure.
    ///
    /// Occurs when the file structure doesn't conform to the TsFile specification,
    /// such as missing required sections or invalid metadata.
    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

    /// Invalid file state or content.
    ///
    /// General file validation error for issues like empty files or
    /// incomplete writes.
    #[error("Invalid file: {0}")]
    InvalidFile(String),

    /// TsFile magic string not found or incorrect.
    ///
    /// The file doesn't start with the expected "TsFile" magic bytes,
    /// indicating it's not a valid TsFile or is corrupted.
    #[error("Invalid magic string")]
    InvalidMagicString,

    /// Requested resource not found.
    ///
    /// Occurs when attempting to access a device, measurement, or other
    /// resource that doesn't exist in the file.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Unsupported TsFile format version.
    ///
    /// The file uses a format version that this library doesn't support.
    /// This implementation supports version 3.
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),

    /// Schema validation or consistency error.
    ///
    /// Occurs during schema registration, validation, or when schemas
    /// conflict between write and read operations.
    #[error("Schema error: {0}")]
    SchemaError(String),

    /// Type mismatch between expected and actual types.
    ///
    /// Occurs when a value's actual type doesn't match the schema-defined
    /// type for that measurement.
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// The expected type name.
        expected: String,
        /// The actual type name encountered.
        actual: String,
    },

    /// Buffer capacity exceeded.
    ///
    /// Occurs when attempting to write more data than the allocated buffer
    /// can hold, typically in encoding operations.
    #[error("Buffer overflow")]
    BufferOverflow,

    /// End of file reached unexpectedly.
    ///
    /// Occurs when reading operations expect more data but reach EOF,
    /// indicating a truncated or corrupted file.
    #[error("EOF reached unexpectedly")]
    UnexpectedEof,

    /// Invalid state for the requested operation.
    ///
    /// Occurs when an operation is attempted in an inappropriate state,
    /// such as writing after closing a writer.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Feature or operation not yet implemented.
    ///
    /// Used for planned features that haven't been completed yet.
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Catch-all for other errors.
    ///
    /// Used for error cases that don't fit other categories.
    #[error("Other error: {0}")]
    Other(String),
}

/// Convenience type alias for [`Result`](std::result::Result) with [`TsFileError`].
///
/// This alias reduces boilerplate in function signatures throughout the library.
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::Result;
///
/// fn read_value() -> Result<i32> {
///     Ok(42)
/// }
/// ```
pub type Result<T> = std::result::Result<T, TsFileError>;
