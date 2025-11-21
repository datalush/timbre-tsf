use crate::common::statistic::Statistic;
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::constants::{HEADER_SIZE, MAGIC, VERSION_MAJOR, VERSION_MINOR};
use crate::error::{Result, TsFileError};
use crate::index::BloomFilter;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

bitflags::bitflags! {
    /// Flags de configuración del archivo Timbre.
    ///
    /// Estos flags indican qué características opcionales están habilitadas en el archivo.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FileFlags: u32 {
        /// Archivo contiene diccionario global para strings
        const HAS_GLOBAL_DICTIONARY = 0b00000001;
        /// Layout compatible con Apache Arrow (zero-copy)
        const ARROW_COMPATIBLE_LAYOUT = 0b00000010;
        /// Archivo contiene índice invertido para tags
        const HAS_INVERTED_INDEX = 0b00000100;
        /// Archivo contiene bloom filters multi-nivel
        const HAS_BLOOM_FILTERS = 0b00001000;
        /// Timestamps están alineados (regular intervals)
        const ALIGNED_TIMESTAMPS = 0b00010000;
    }
}

/// File Header de Timbre (128 bytes, alineado).
///
/// Estructura que aparece al inicio de cada archivo .timbre, comenzando con el
/// magic number TMB1 y conteniendo metadata esencial del archivo.
///
/// # Formato Binario
///
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
/// 0       4     Magic number (TMB1)
/// 4       2     Version major
/// 6       2     Version minor
/// 8       4     Flags (bitfield)
/// 12      8     Created timestamp (microseconds)
/// 20      32    Writer version string
/// 52      8     Schema offset
/// 60      8     Index offset
/// 68      8     Dictionary offset
/// 76      4     Number of device groups
/// 80      8     Total data points
/// 88      32    File checksum (BLAKE3)
/// 120     8     Reserved
/// Total: 128 bytes
/// ```
#[derive(Debug, Clone)]
pub struct FileHeader {
    /// Versión major del formato
    pub version_major: u16,
    /// Versión minor del formato
    pub version_minor: u16,
    /// Flags de configuración
    pub flags: FileFlags,
    /// Timestamp de creación (microsegundos desde epoch)
    pub created_timestamp: i64,
    /// Versión del writer que creó el archivo (max 32 bytes UTF-8)
    pub writer_version: String,
    /// Offset al inicio de la sección de schema
    pub schema_offset: u64,
    /// Offset al inicio de la sección de índices
    pub index_offset: u64,
    /// Offset al diccionario global (0 si no existe)
    pub dictionary_offset: u64,
    /// Número de device groups en el archivo
    pub num_device_groups: u32,
    /// Total de data points en el archivo
    pub total_data_points: u64,
    /// Checksum del archivo completo (BLAKE3, 32 bytes)
    pub file_checksum: [u8; 32],
}

impl FileHeader {
    /// Crea un nuevo FileHeader con valores por defecto.
    pub fn new() -> Self {
        Self {
            version_major: VERSION_MAJOR,
            version_minor: VERSION_MINOR,
            flags: FileFlags::empty(),
            created_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros() as i64,
            writer_version: "timbre-tsf-1.0".to_string(),
            schema_offset: 0,
            index_offset: 0,
            dictionary_offset: 0,
            num_device_groups: 0,
            total_data_points: 0,
            file_checksum: [0u8; 32],
        }
    }

    /// Serializa el header a bytes (siempre 128 bytes).
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Magic number (4 bytes)
        writer.write_all(MAGIC)?;

        // Version (2 + 2 = 4 bytes)
        writer.write_u16::<LittleEndian>(self.version_major)?;
        writer.write_u16::<LittleEndian>(self.version_minor)?;

        // Flags (4 bytes)
        writer.write_u32::<LittleEndian>(self.flags.bits())?;

        // Created timestamp (8 bytes)
        writer.write_i64::<LittleEndian>(self.created_timestamp)?;

        // Writer version (32 bytes, zero-padded)
        let mut writer_bytes = [0u8; 32];
        let bytes = self.writer_version.as_bytes();
        let len = bytes.len().min(32);
        writer_bytes[..len].copy_from_slice(&bytes[..len]);
        writer.write_all(&writer_bytes)?;

        // Offsets (8 + 8 + 8 = 24 bytes)
        writer.write_u64::<LittleEndian>(self.schema_offset)?;
        writer.write_u64::<LittleEndian>(self.index_offset)?;
        writer.write_u64::<LittleEndian>(self.dictionary_offset)?;

        // Counts (4 + 8 = 12 bytes)
        writer.write_u32::<LittleEndian>(self.num_device_groups)?;
        writer.write_u64::<LittleEndian>(self.total_data_points)?;

        // File checksum (32 bytes)
        writer.write_all(&self.file_checksum)?;

        // Reserved (8 bytes)
        writer.write_all(&[0u8; 8])?;

        Ok(())
    }

    /// Deserializa el header desde bytes.
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        // Magic number (4 bytes)
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(TsFileError::InvalidFormat(format!(
                "Invalid magic number: expected TMB1, got {:?}",
                magic
            )));
        }

        // Version (4 bytes)
        let version_major = reader.read_u16::<LittleEndian>()?;
        let version_minor = reader.read_u16::<LittleEndian>()?;

        // Flags (4 bytes)
        let flags_bits = reader.read_u32::<LittleEndian>()?;
        let flags = FileFlags::from_bits_truncate(flags_bits);

        // Created timestamp (8 bytes)
        let created_timestamp = reader.read_i64::<LittleEndian>()?;

        // Writer version (32 bytes)
        let mut writer_bytes = [0u8; 32];
        reader.read_exact(&mut writer_bytes)?;
        let end = writer_bytes.iter().position(|&b| b == 0).unwrap_or(32);
        let writer_version = String::from_utf8_lossy(&writer_bytes[..end]).to_string();

        // Offsets (24 bytes)
        let schema_offset = reader.read_u64::<LittleEndian>()?;
        let index_offset = reader.read_u64::<LittleEndian>()?;
        let dictionary_offset = reader.read_u64::<LittleEndian>()?;

        // Counts (12 bytes)
        let num_device_groups = reader.read_u32::<LittleEndian>()?;
        let total_data_points = reader.read_u64::<LittleEndian>()?;

        // File checksum (32 bytes)
        let mut file_checksum = [0u8; 32];
        reader.read_exact(&mut file_checksum)?;

        // Reserved (8 bytes) - skip
        let mut reserved = [0u8; 8];
        reader.read_exact(&mut reserved)?;

        Ok(Self {
            version_major,
            version_minor,
            flags,
            created_timestamp,
            writer_version,
            schema_offset,
            index_offset,
            dictionary_offset,
            num_device_groups,
            total_data_points,
            file_checksum,
        })
    }

    /// Tamaño serializado del header (siempre 128 bytes).
    pub const fn serialized_size() -> usize {
        HEADER_SIZE
    }
}

impl Default for FileHeader {
    fn default() -> Self {
        Self::new()
    }
}

/// File Footer de Timbre (132 bytes: 128 bytes metadata + 4 bytes magic).
///
/// Estructura que aparece al final de cada archivo .timbre, terminando con el
/// magic number TMB1 para validación de integridad.
///
/// # Formato Binario
///
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
/// 0       8     Metadata offset
/// 8       4     Metadata size
/// 12      8     Index offset
/// 20      4     Index size
/// 24      8     Dictionary offset (0 si no existe)
/// 32      4     Dictionary size
/// 36      8     First device group offset
/// 44      8     Last device group offset
/// 52      4     Total device groups
/// 56      8     Total data points
/// 64      8     Min timestamp
/// 72      8     Max timestamp
/// 80      32    Footer checksum (BLAKE3)
/// 112     16    Reserved
/// 128     4     Magic number (TMB1)
/// Total: 132 bytes
/// ```
#[derive(Debug, Clone)]
pub struct FileFooter {
    /// Offset a la sección de metadata
    pub metadata_offset: u64,
    /// Tamaño de la sección de metadata
    pub metadata_size: u32,
    /// Offset a la sección de índices
    pub index_offset: u64,
    /// Tamaño de la sección de índices
    pub index_size: u32,
    /// Offset al diccionario global (0 si no existe)
    pub dictionary_offset: u64,
    /// Tamaño del diccionario
    pub dictionary_size: u32,
    /// Offset al primer device group
    pub first_device_group_offset: u64,
    /// Offset al último device group
    pub last_device_group_offset: u64,
    /// Total de device groups
    pub total_device_groups: u32,
    /// Total de data points
    pub total_data_points: u64,
    /// Timestamp mínimo en el archivo
    pub min_timestamp: i64,
    /// Timestamp máximo en el archivo
    pub max_timestamp: i64,
    /// Checksum del footer (BLAKE3, 32 bytes)
    pub footer_checksum: [u8; 32],
}

impl FileFooter {
    /// Tamaño serializado del footer en bytes
    pub const SERIALIZED_SIZE: usize = 132;

    /// Crea un nuevo FileFooter con valores por defecto.
    pub fn new() -> Self {
        Self {
            metadata_offset: 0,
            metadata_size: 0,
            index_offset: 0,
            index_size: 0,
            dictionary_offset: 0,
            dictionary_size: 0,
            first_device_group_offset: 0,
            last_device_group_offset: 0,
            total_device_groups: 0,
            total_data_points: 0,
            min_timestamp: i64::MAX,
            max_timestamp: i64::MIN,
            footer_checksum: [0u8; 32],
        }
    }

    /// Serializa el footer a bytes (siempre 132 bytes).
    pub fn serialize<W: Write>(&self, writer: &mut W) -> Result<()> {
        // Metadata info (8 + 4 = 12 bytes)
        writer.write_u64::<LittleEndian>(self.metadata_offset)?;
        writer.write_u32::<LittleEndian>(self.metadata_size)?;

        // Index info (8 + 4 = 12 bytes)
        writer.write_u64::<LittleEndian>(self.index_offset)?;
        writer.write_u32::<LittleEndian>(self.index_size)?;

        // Dictionary info (8 + 4 = 12 bytes)
        writer.write_u64::<LittleEndian>(self.dictionary_offset)?;
        writer.write_u32::<LittleEndian>(self.dictionary_size)?;

        // Device groups info (8 + 8 + 4 = 20 bytes)
        writer.write_u64::<LittleEndian>(self.first_device_group_offset)?;
        writer.write_u64::<LittleEndian>(self.last_device_group_offset)?;
        writer.write_u32::<LittleEndian>(self.total_device_groups)?;

        // Data points and timestamps (8 + 8 + 8 = 24 bytes)
        writer.write_u64::<LittleEndian>(self.total_data_points)?;
        writer.write_i64::<LittleEndian>(self.min_timestamp)?;
        writer.write_i64::<LittleEndian>(self.max_timestamp)?;

        // Footer checksum (32 bytes)
        writer.write_all(&self.footer_checksum)?;

        // Reserved (16 bytes)
        writer.write_all(&[0u8; 16])?;

        // Magic number at the end (4 bytes)
        writer.write_all(MAGIC)?;

        Ok(())
    }

    /// Deserializa el footer desde bytes.
    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Self> {
        // Metadata info (12 bytes)
        let metadata_offset = reader.read_u64::<LittleEndian>()?;
        let metadata_size = reader.read_u32::<LittleEndian>()?;

        // Index info (12 bytes)
        let index_offset = reader.read_u64::<LittleEndian>()?;
        let index_size = reader.read_u32::<LittleEndian>()?;

        // Dictionary info (12 bytes)
        let dictionary_offset = reader.read_u64::<LittleEndian>()?;
        let dictionary_size = reader.read_u32::<LittleEndian>()?;

        // Device groups info (20 bytes)
        let first_device_group_offset = reader.read_u64::<LittleEndian>()?;
        let last_device_group_offset = reader.read_u64::<LittleEndian>()?;
        let total_device_groups = reader.read_u32::<LittleEndian>()?;

        // Data points and timestamps (24 bytes)
        let total_data_points = reader.read_u64::<LittleEndian>()?;
        let min_timestamp = reader.read_i64::<LittleEndian>()?;
        let max_timestamp = reader.read_i64::<LittleEndian>()?;

        // Footer checksum (32 bytes)
        let mut footer_checksum = [0u8; 32];
        reader.read_exact(&mut footer_checksum)?;

        // Reserved (16 bytes) - skip
        let mut reserved = [0u8; 16];
        reader.read_exact(&mut reserved)?;

        // Magic number at the end (4 bytes)
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if magic != MAGIC {
            return Err(TsFileError::InvalidFormat(format!(
                "Invalid footer magic number: expected TMB1, got {:?}",
                magic
            )));
        }

        Ok(Self {
            metadata_offset,
            metadata_size,
            index_offset,
            index_size,
            dictionary_offset,
            dictionary_size,
            first_device_group_offset,
            last_device_group_offset,
            total_device_groups,
            total_data_points,
            min_timestamp,
            max_timestamp,
            footer_checksum,
        })
    }

    /// Tamaño serializado del footer (siempre 132 bytes).
    pub const fn serialized_size() -> usize {
        132 // 128 bytes metadata + 4 bytes magic
    }
}

impl Default for FileFooter {
    fn default() -> Self {
        Self::new()
    }
}

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
        measurement_name: impl Into<String>,
        data_type: TSDataType,
        compression_type: CompressionType,
        encoding_type: TSEncoding,
    ) -> Self {
        Self {
            chunk_type: ChunkType::NonAligned,
            measurement_name: measurement_name.into(),
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
        4 // num_of_pages
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
    pub bloom_filter: Option<BloomFilter>,
    pub min_time: i64,
    pub max_time: i64,
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
            bloom_filter: None,
            min_time: i64::MAX,
            max_time: i64::MIN,
        }
    }

    /// Get the bloom filter if present
    pub fn bloom_filter(&self) -> Option<&BloomFilter> {
        self.bloom_filter.as_ref()
    }

    /// Set the bloom filter
    pub fn set_bloom_filter(&mut self, bloom: BloomFilter) {
        self.bloom_filter = Some(bloom);
    }

    /// Check if a value might be present in this chunk using bloom filter
    pub fn might_contain<T: std::hash::Hash>(&self, value: &T) -> bool {
        match &self.bloom_filter {
            Some(bloom) => bloom.might_contain(value),
            None => true, // If no bloom filter, assume it might contain the value
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

/// Datos de una página usando mini-blocks para paralelismo fino
///
/// Timbre SIEMPRE usa mini-blocks (4-8 bloques por página).
/// Cada mini-block puede decodificarse independientemente en paralelo.
/// Sin backward compatibility con TSFile.
#[derive(Debug)]
pub struct PageData {
    pub header: PageHeader,
    /// Mini-blocks (4-8) para decompresión paralela (innovación core de Timbre)
    pub miniblocks: Vec<crate::file::miniblock::MiniBlock>,
}

impl PageData {
    /// Creates a new empty PageData
    pub fn new() -> Self {
        Self {
            header: PageHeader::new(),
            miniblocks: Vec::new(),
        }
    }

    /// Creates a PageData with mini-blocks (Timbre format)
    pub fn with_miniblocks(
        header: PageHeader,
        miniblocks: Vec<crate::file::miniblock::MiniBlock>,
    ) -> Self {
        Self { header, miniblocks }
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
