use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::common::statistic::Statistic;
use crate::error::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

/// Tipo de chunk
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ChunkType {
    NonAligned = 0,
    Aligned = 1,
}

impl ChunkType {
    pub fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Aligned,
            _ => Self::NonAligned,
        }
    }

    pub fn to_u8(self) -> u8 {
        self as u8
    }
}

/// Header de un chunk
#[derive(Debug, Clone)]
pub struct ChunkHeader {
    pub chunk_type: ChunkType,
    pub measurement_name: String,
    pub data_size: u32,
    pub data_type: TSDataType,
    pub compression_type: CompressionType,
    pub encoding_type: TSEncoding,
    pub num_of_pages: i32,
}

impl ChunkHeader {
    pub fn new(
        measurement_name: String,
        data_type: TSDataType,
        compression_type: CompressionType,
        encoding_type: TSEncoding,
    ) -> Self {
        Self {
            chunk_type: ChunkType::NonAligned,
            measurement_name,
            data_size: 0,
            data_type,
            compression_type,
            encoding_type,
            num_of_pages: 0,
        }
    }

    /// Serializa el header a bytes
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u8(self.chunk_type.to_u8())?;

        // Escribir measurement name
        let name_bytes = self.measurement_name.as_bytes();
        writer.write_i32::<LittleEndian>(name_bytes.len() as i32)?;
        writer.write_all(name_bytes)?;

        writer.write_u32::<LittleEndian>(self.data_size)?;
        writer.write_u8(self.data_type.to_u8())?;
        writer.write_u8(self.compression_type.to_u8())?;
        writer.write_u8(self.encoding_type.to_u8())?;
        writer.write_i32::<LittleEndian>(self.num_of_pages)?;

        Ok(())
    }

    /// Deserializa el header desde bytes
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        let chunk_type = ChunkType::from_u8(reader.read_u8()?);

        let name_len = reader.read_i32::<LittleEndian>()? as usize;
        let mut name_bytes = vec![0u8; name_len];
        reader.read_exact(&mut name_bytes)?;
        let measurement_name = String::from_utf8(name_bytes)
            .map_err(|e| crate::error::TsFileError::DecodingError(e.to_string()))?;

        let data_size = reader.read_u32::<LittleEndian>()?;
        let data_type = TSDataType::from_u8(reader.read_u8()?);
        let compression_type = CompressionType::from_u8(reader.read_u8()?);
        let encoding_type = TSEncoding::from_u8(reader.read_u8()?);
        let num_of_pages = reader.read_i32::<LittleEndian>()?;

        Ok(Self {
            chunk_type,
            measurement_name,
            data_size,
            data_type,
            compression_type,
            encoding_type,
            num_of_pages,
        })
    }

    /// Tamaño serializado del header
    pub fn serialized_size(&self) -> usize {
        1 + // chunk_type
        4 + self.measurement_name.len() + // name length + name
        4 + // data_size
        1 + // data_type
        1 + // compression_type
        1 + // encoding_type
        4   // num_of_pages
    }
}

/// Metadatos de un chunk
#[derive(Debug)]
pub struct ChunkMeta {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub offset_of_chunk_header: i64,
    pub statistic: Option<Box<dyn Statistic>>,
    pub encoding: TSEncoding,
    pub compression_type: CompressionType,
}

impl ChunkMeta {
    pub fn new(
        measurement_name: String,
        data_type: TSDataType,
        offset: i64,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        Self {
            measurement_name,
            data_type,
            offset_of_chunk_header: offset,
            statistic: None,
            encoding,
            compression_type,
        }
    }
}

/// Header de una página
#[derive(Debug, Clone)]
pub struct PageHeader {
    pub uncompressed_size: u32,
    pub compressed_size: u32,
    pub num_of_values: i32,
    pub max_timestamp: i64,
    pub min_timestamp: i64,
}

impl PageHeader {
    pub fn new() -> Self {
        Self {
            uncompressed_size: 0,
            compressed_size: 0,
            num_of_values: 0,
            max_timestamp: i64::MIN,
            min_timestamp: i64::MAX,
        }
    }

    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_u32::<LittleEndian>(self.uncompressed_size)?;
        writer.write_u32::<LittleEndian>(self.compressed_size)?;
        writer.write_i32::<LittleEndian>(self.num_of_values)?;
        writer.write_i64::<LittleEndian>(self.max_timestamp)?;
        writer.write_i64::<LittleEndian>(self.min_timestamp)?;
        Ok(())
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        Ok(Self {
            uncompressed_size: reader.read_u32::<LittleEndian>()?,
            compressed_size: reader.read_u32::<LittleEndian>()?,
            num_of_values: reader.read_i32::<LittleEndian>()?,
            max_timestamp: reader.read_i64::<LittleEndian>()?,
            min_timestamp: reader.read_i64::<LittleEndian>()?,
        })
    }

    pub const SERIALIZED_SIZE: usize = 4 + 4 + 4 + 8 + 8; // 28 bytes
}

impl Default for PageHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// Datos de una página (comprimidos y sin comprimir)
#[derive(Debug)]
pub struct PageData {
    pub uncompressed_data: Vec<u8>,
    pub compressed_data: Vec<u8>,
    pub header: PageHeader,
}

impl PageData {
    pub fn new() -> Self {
        Self {
            uncompressed_data: Vec::new(),
            compressed_data: Vec::new(),
            header: PageHeader::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            uncompressed_data: Vec::with_capacity(capacity),
            compressed_data: Vec::with_capacity(capacity),
            header: PageHeader::new(),
        }
    }
}

impl Default for PageData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_header_serialization() {
        let header = ChunkHeader::new(
            "temperature".to_string(),
            TSDataType::Float,
            CompressionType::Lz4,
            TSEncoding::Gorilla,
        );

        let mut buffer = Vec::new();
        header.serialize(&mut buffer).unwrap();

        let deserialized = ChunkHeader::deserialize(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized.measurement_name, "temperature");
        assert_eq!(deserialized.data_type, TSDataType::Float);
        assert_eq!(deserialized.compression_type, CompressionType::Lz4);
        assert_eq!(deserialized.encoding_type, TSEncoding::Gorilla);
    }

    #[test]
    fn test_page_header_serialization() {
        let mut header = PageHeader::new();
        header.uncompressed_size = 1000;
        header.compressed_size = 500;
        header.num_of_values = 100;
        header.min_timestamp = 1000;
        header.max_timestamp = 2000;

        let mut buffer = Vec::new();
        header.serialize(&mut buffer).unwrap();

        let deserialized = PageHeader::deserialize(&mut buffer.as_slice()).unwrap();

        assert_eq!(deserialized.uncompressed_size, 1000);
        assert_eq!(deserialized.compressed_size, 500);
        assert_eq!(deserialized.num_of_values, 100);
        assert_eq!(deserialized.min_timestamp, 1000);
        assert_eq!(deserialized.max_timestamp, 2000);
    }
}
