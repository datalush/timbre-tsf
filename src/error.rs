use std::io;
use thiserror::Error;

/// TsFile error types
#[derive(Debug, Error)]
pub enum TsFileError {
    /// I/O error
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Invalid data type
    #[error("Invalid data type: {0}")]
    InvalidDataType(u8),

    /// Invalid encoding
    #[error("Invalid encoding: {0}")]
    InvalidEncoding(u8),

    /// Invalid compression type
    #[error("Invalid compression: {0}")]
    InvalidCompression(u8),

    /// Encoding error
    #[error("Encoding error: {0}")]
    EncodingError(String),

    /// Decoding error
    #[error("Decoding error: {0}")]
    DecodingError(String),

    /// Compression error
    #[error("Compression error: {0}")]
    CompressionError(String),

    /// Decompression error
    #[error("Decompression error: {0}")]
    DecompressionError(String),

    /// Invalid file format
    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

    /// Invalid file
    #[error("Invalid file: {0}")]
    InvalidFile(String),

    /// Invalid magic string
    #[error("Invalid magic string")]
    InvalidMagicString,

    /// Not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Unsupported version
    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),

    /// Schema error
    #[error("Schema error: {0}")]
    SchemaError(String),

    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    /// Buffer overflow
    #[error("Buffer overflow")]
    BufferOverflow,

    /// EOF reached unexpectedly
    #[error("EOF reached unexpectedly")]
    UnexpectedEof,

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Not implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

/// Result type alias
pub type Result<T> = std::result::Result<T, TsFileError>;
