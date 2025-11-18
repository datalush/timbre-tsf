use std::fmt;

/// Tipos de datos soportados en TsFile
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TSDataType {
    Boolean = 0,
    Int32 = 1,
    Int64 = 2,
    Float = 3,
    Double = 4,
    Text = 5,
    Vector = 6,
    Unknown = 7,
    Timestamp = 8,
    Date = 9,
    Blob = 10,
    String = 11,
    Null = 254,
    Invalid = 255,
}

impl TSDataType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Boolean,
            1 => Self::Int32,
            2 => Self::Int64,
            3 => Self::Float,
            4 => Self::Double,
            5 => Self::Text,
            6 => Self::Vector,
            7 => Self::Unknown,
            8 => Self::Timestamp,
            9 => Self::Date,
            10 => Self::Blob,
            11 => Self::String,
            254 => Self::Null,
            _ => Self::Invalid,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn size(&self) -> Option<usize> {
        match self {
            Self::Boolean => Some(1),
            Self::Int32 | Self::Float | Self::Date => Some(4),
            Self::Int64 | Self::Double | Self::Timestamp => Some(8),
            _ => None,
        }
    }
}

impl fmt::Display for TSDataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Boolean => write!(f, "BOOLEAN"),
            Self::Int32 => write!(f, "INT32"),
            Self::Int64 => write!(f, "INT64"),
            Self::Float => write!(f, "FLOAT"),
            Self::Double => write!(f, "DOUBLE"),
            Self::Text => write!(f, "TEXT"),
            Self::Vector => write!(f, "VECTOR"),
            Self::Unknown => write!(f, "UNKNOWN"),
            Self::Timestamp => write!(f, "TIMESTAMP"),
            Self::Date => write!(f, "DATE"),
            Self::Blob => write!(f, "BLOB"),
            Self::String => write!(f, "STRING"),
            Self::Null => write!(f, "NULL"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Métodos de encoding soportados
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TSEncoding {
    Plain = 0,
    Dictionary = 1,
    Rle = 2,
    Diff = 3,
    Ts2Diff = 4,
    Bitmap = 5,
    GorillaV1 = 6,
    Regular = 7,
    Gorilla = 8,
    Zigzag = 9,
    Freq = 10,
    Sprintz = 12,
    Invalid = 255,
}

impl TSEncoding {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Plain,
            1 => Self::Dictionary,
            2 => Self::Rle,
            3 => Self::Diff,
            4 => Self::Ts2Diff,
            5 => Self::Bitmap,
            6 => Self::GorillaV1,
            7 => Self::Regular,
            8 => Self::Gorilla,
            9 => Self::Zigzag,
            10 => Self::Freq,
            12 => Self::Sprintz,
            _ => Self::Invalid,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Retorna el encoding recomendado para un tipo de dato
    pub fn recommended_for(data_type: TSDataType) -> Self {
        match data_type {
            TSDataType::Boolean => Self::Rle,
            TSDataType::Int32 | TSDataType::Date => Self::Ts2Diff,
            TSDataType::Int64 | TSDataType::Timestamp => Self::Ts2Diff,
            TSDataType::Float | TSDataType::Double => Self::Gorilla,
            TSDataType::Text | TSDataType::String => Self::Dictionary,
            _ => Self::Plain,
        }
    }
}

impl fmt::Display for TSEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => write!(f, "PLAIN"),
            Self::Dictionary => write!(f, "DICTIONARY"),
            Self::Rle => write!(f, "RLE"),
            Self::Diff => write!(f, "DIFF"),
            Self::Ts2Diff => write!(f, "TS_2DIFF"),
            Self::Bitmap => write!(f, "BITMAP"),
            Self::GorillaV1 => write!(f, "GORILLA_V1"),
            Self::Regular => write!(f, "REGULAR"),
            Self::Gorilla => write!(f, "GORILLA"),
            Self::Zigzag => write!(f, "ZIGZAG"),
            Self::Freq => write!(f, "FREQ"),
            Self::Sprintz => write!(f, "SPRINTZ"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Métodos de compresión soportados
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CompressionType {
    Uncompressed = 0,
    Snappy = 1,
    Gzip = 2,
    Lzo = 3,
    Sdt = 4,
    Paa = 5,
    Pla = 6,
    Lz4 = 7,
    Invalid = 255,
}

impl CompressionType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Uncompressed,
            1 => Self::Snappy,
            2 => Self::Gzip,
            3 => Self::Lzo,
            4 => Self::Sdt,
            5 => Self::Paa,
            6 => Self::Pla,
            7 => Self::Lz4,
            _ => Self::Invalid,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }

    /// Retorna la compresión recomendada para un tipo de dato
    pub fn recommended_for(_data_type: TSDataType) -> Self {
        Self::Lz4 // LZ4 es la recomendación general por su balance velocidad/ratio
    }
}

impl fmt::Display for CompressionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncompressed => write!(f, "UNCOMPRESSED"),
            Self::Snappy => write!(f, "SNAPPY"),
            Self::Gzip => write!(f, "GZIP"),
            Self::Lzo => write!(f, "LZO"),
            Self::Sdt => write!(f, "SDT"),
            Self::Paa => write!(f, "PAA"),
            Self::Pla => write!(f, "PLA"),
            Self::Lz4 => write!(f, "LZ4"),
            Self::Invalid => write!(f, "INVALID"),
        }
    }
}

/// Categoría de columna en modelo tabla
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnCategory {
    Tag,
    Field,
    Time,
}

/// Valor de dato en TsFile (enum para todos los tipos)
#[derive(Debug, Clone, PartialEq)]
pub enum TsValue {
    Boolean(bool),
    Int32(i32),
    Int64(i64),
    Float(f32),
    Double(f64),
    Text(String),
    String(String),
    Blob(Vec<u8>),
    Null,
}

impl TsValue {
    pub fn data_type(&self) -> TSDataType {
        match self {
            Self::Boolean(_) => TSDataType::Boolean,
            Self::Int32(_) => TSDataType::Int32,
            Self::Int64(_) => TSDataType::Int64,
            Self::Float(_) => TSDataType::Float,
            Self::Double(_) => TSDataType::Double,
            Self::Text(_) => TSDataType::Text,
            Self::String(_) => TSDataType::String,
            Self::Blob(_) => TSDataType::Blob,
            Self::Null => TSDataType::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_type_conversion() {
        assert_eq!(TSDataType::from_u8(0), TSDataType::Boolean);
        assert_eq!(TSDataType::Int32.to_u8(), 1);
        assert_eq!(TSDataType::Float.size(), Some(4));
        assert_eq!(TSDataType::Text.size(), None);
    }

    #[test]
    fn test_encoding_recommended() {
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Boolean),
            TSEncoding::Rle
        );
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Float),
            TSEncoding::Gorilla
        );
        assert_eq!(
            TSEncoding::recommended_for(TSDataType::Int32),
            TSEncoding::Ts2Diff
        );
    }

    #[test]
    fn test_compression_recommended() {
        assert_eq!(
            CompressionType::recommended_for(TSDataType::Int32),
            CompressionType::Lz4
        );
    }
}
