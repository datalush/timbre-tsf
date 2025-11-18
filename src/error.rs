use thiserror::Error;

pub type Result<T> = std::result::Result<T, TsFileError>;

#[derive(Error, Debug)]
pub enum TsFileError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid data type: {0}")]
    InvalidDataType(u8),

    #[error("Invalid encoding: {0}")]
    InvalidEncoding(u8),

    #[error("Invalid compression: {0}")]
    InvalidCompression(u8),

    #[error("Encoding error: {0}")]
    EncodingError(String),

    #[error("Decoding error: {0}")]
    DecodingError(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Decompression error: {0}")]
    DecompressionError(String),

    #[error("Invalid file format: {0}")]
    InvalidFormat(String),

    #[error("Invalid file: {0}")]
    InvalidFile(String),

    #[error("Invalid magic string")]
    InvalidMagicString,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unsupported version: {0}")]
    UnsupportedVersion(u8),

    #[error("Schema error: {0}")]
    SchemaError(String),

    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("Buffer overflow")]
    BufferOverflow,

    #[error("EOF reached unexpectedly")]
    UnexpectedEof,

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Not implemented: {0}")]
    NotImplemented(String),

    #[error("Other error: {0}")]
    Other(String),
}
