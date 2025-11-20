use crate::error::{Result, TsFileError};
use crate::file::{ChunkMeta, FileFooter, FileHeader, FileFlags, GlobalDictionary};
use crate::writer::ChunkWriter;
use byteorder::{LittleEndian, WriteBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

/// Low-level TsFile writer que maneja la estructura física del archivo
pub struct TsFileIOWriter {
    file: BufWriter<File>,
    device_chunk_groups: HashMap<String, Vec<ChunkMeta>>,
    current_position: u64,
    first_device_group_offset: Option<u64>,
    min_timestamp: i64,
    max_timestamp: i64,
    total_data_points: u64,
    /// Global dictionary para compresión de strings (device IDs, measurements)
    dictionary: GlobalDictionary,
}

impl TsFileIOWriter {
    /// Crea un nuevo TsFileIOWriter
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::create(path)?;

        // OPT-A: Wrap file in BufWriter with 256KB buffer (vs 8KB default)
        // Reduces system calls from thousands to dozens
        const BUFFER_SIZE: usize = 256 * 1024; // 256KB
        let mut file = BufWriter::with_capacity(BUFFER_SIZE, file);

        // Escribir FileHeader (128 bytes) con magic TMB1
        let header = FileHeader::new();
        header.serialize(&mut file)?;

        let current_position = file.stream_position()?;

        Ok(Self {
            file,
            device_chunk_groups: HashMap::new(),
            current_position,
            first_device_group_offset: None,
            min_timestamp: i64::MAX,
            max_timestamp: i64::MIN,
            total_data_points: 0,
            dictionary: GlobalDictionary::new(),
        })
    }

    /// Inicia un nuevo chunk group para un dispositivo
    pub fn start_chunk_group(&mut self, device_id: &str) -> Result<()> {
        // Intern device ID en diccionario para compresión
        self.dictionary.intern(device_id);

        if !self.device_chunk_groups.contains_key(device_id) {
            self.device_chunk_groups
                .insert(device_id.to_string(), Vec::new());

            // Guardar offset del primer device group
            if self.first_device_group_offset.is_none() {
                self.first_device_group_offset = Some(self.current_position);
            }
        }
        Ok(())
    }

    /// Escribe un chunk al archivo
    pub fn write_chunk(&mut self, device_id: &str, mut chunk_writer: ChunkWriter) -> Result<u64> {
        let chunk_offset = self.current_position;

        // Intern measurement name en diccionario
        self.dictionary.intern(chunk_writer.measurement_name());

        // Actualizar estadísticas del archivo antes de serializar
        let stat = chunk_writer.statistic();
        if stat.count() > 0 {
            self.min_timestamp = self.min_timestamp.min(stat.start_time());
            self.max_timestamp = self.max_timestamp.max(stat.end_time());
            self.total_data_points += stat.count() as u64;
        }

        // Serializar el chunk
        let bytes_written = chunk_writer.serialize_to(&mut self.file)?;
        self.current_position += bytes_written as u64;

        // Guardar metadata del chunk
        let chunk_meta = ChunkMeta::new(
            chunk_writer.measurement_name().to_string(),
            chunk_writer.data_type(),
            chunk_offset as i64,
            chunk_writer.encoding(),
            chunk_writer.compression_type(),
        );

        self.device_chunk_groups
            .entry(device_id.to_string())
            .or_default()
            .push(chunk_meta);

        Ok(chunk_offset)
    }

    /// Finaliza el chunk group
    pub fn end_chunk_group(&mut self, device_id: &str) -> Result<()> {
        // En la implementación completa, aquí se escribiría un marker de separación
        // Por ahora solo validamos que el device existe
        if !self.device_chunk_groups.contains_key(device_id) {
            return Err(TsFileError::InvalidState(format!(
                "Device {} not found",
                device_id
            )));
        }
        Ok(())
    }

    /// Escribe el footer del archivo con metadata
    pub fn write_metadata(&mut self) -> Result<()> {
        // Separador antes de metadata
        self.file.write_u8(0x02)?; // Metadata marker

        // Escribir número de dispositivos
        self.file
            .write_u32::<LittleEndian>(self.device_chunk_groups.len() as u32)?;

        // Escribir metadata de cada dispositivo
        for (device_id, chunks) in &self.device_chunk_groups {
            // Device ID
            let device_bytes = device_id.as_bytes();
            self.file
                .write_u32::<LittleEndian>(device_bytes.len() as u32)?;
            self.file.write_all(device_bytes)?;

            // Número de chunks
            self.file.write_u32::<LittleEndian>(chunks.len() as u32)?;

            // Metadata de cada chunk
            for chunk in chunks {
                // Measurement name
                let name_bytes = chunk.measurement_name.as_bytes();
                self.file
                    .write_u32::<LittleEndian>(name_bytes.len() as u32)?;
                self.file.write_all(name_bytes)?;

                // Offset y data type
                self.file
                    .write_i64::<LittleEndian>(chunk.offset_of_chunk_header)?;
                self.file.write_u8(chunk.data_type.to_u8())?;
                self.file.write_u8(chunk.encoding.to_u8())?;
                self.file.write_u8(chunk.compression_type.to_u8())?;
            }
        }

        Ok(())
    }

    /// Finaliza el archivo
    pub fn close(mut self) -> Result<()> {
        // Escribir dictionary si no está vacío
        let dictionary_offset;
        let dictionary_size;
        if !self.dictionary.is_empty() {
            dictionary_offset = self.file.stream_position()?;
            let dict_start = dictionary_offset;
            self.dictionary.serialize(&mut self.file)?;
            let dict_end = self.file.stream_position()?;
            dictionary_size = (dict_end - dict_start) as u32;
        } else {
            dictionary_offset = 0;
            dictionary_size = 0;
        }

        // Escribir metadata
        let metadata_offset = self.file.stream_position()?;
        let metadata_start_pos = metadata_offset;
        self.write_metadata()?;
        let metadata_end_pos = self.file.stream_position()?;
        let metadata_size = (metadata_end_pos - metadata_start_pos) as u32;

        // Crear y escribir FileFooter (132 bytes) con TMB1 al final
        let mut footer = FileFooter::new();
        footer.metadata_offset = metadata_offset;
        footer.metadata_size = metadata_size;
        footer.index_offset = 0; // No índices por ahora
        footer.index_size = 0;
        footer.dictionary_offset = dictionary_offset;
        footer.dictionary_size = dictionary_size;
        footer.first_device_group_offset = self.first_device_group_offset.unwrap_or(0);
        footer.last_device_group_offset = self.first_device_group_offset.unwrap_or(0);
        footer.total_device_groups = self.device_chunk_groups.len() as u32;
        footer.total_data_points = self.total_data_points;
        footer.min_timestamp = if self.min_timestamp == i64::MAX {
            0
        } else {
            self.min_timestamp
        };
        footer.max_timestamp = if self.max_timestamp == i64::MIN {
            0
        } else {
            self.max_timestamp
        };

        footer.serialize(&mut self.file)?;

        // Actualizar FileHeader con flag de dictionary y offset
        if !self.dictionary.is_empty() {
            self.file.seek(std::io::SeekFrom::Start(0))?;
            let mut header = FileHeader::new();
            header.flags = FileFlags::HAS_GLOBAL_DICTIONARY;
            header.dictionary_offset = dictionary_offset;
            header.serialize(&mut self.file)?;
        }

        // Flush
        self.file.flush()?;

        Ok(())
    }

    /// Obtiene la posición actual en el archivo
    pub fn position(&self) -> u64 {
        self.current_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CompressionType, TSDataType, TSEncoding};
    use tempfile::NamedTempFile;

    #[test]
    fn test_tsfile_io_writer_basic() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let mut writer = TsFileIOWriter::new(path).unwrap();

        // Iniciar chunk group
        writer.start_chunk_group("device1").unwrap();

        // Crear y escribir un chunk
        let mut chunk = ChunkWriter::new(
            "temperature".to_string(),
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        chunk.write_f32(1000, 25.5).unwrap();
        chunk.write_f32(2000, 26.0).unwrap();

        writer.write_chunk("device1", chunk).unwrap();
        writer.end_chunk_group("device1").unwrap();

        // Cerrar archivo
        writer.close().unwrap();

        // Verificar que el archivo existe
        assert!(path.exists());
    }

    #[test]
    fn test_tsfile_io_writer_with_dictionary() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        {
            let mut writer = TsFileIOWriter::new(&path).unwrap();

            // Escribir múltiples devices con measurements repetidos
            // Esto demuestra la compresión del diccionario
            for i in 0..3 {
                let device_id = format!("sensor_{:03}", i);
                writer.start_chunk_group(&device_id).unwrap();

                // Mismo measurement name repetido -> solo 1 entrada en diccionario
                let mut chunk = ChunkWriter::new(
                    "temperature".to_string(),
                    TSDataType::Float,
                    TSEncoding::Chimp128,
                    CompressionType::Zstd,
                );

                chunk.write_f32(1000 + i, 20.0 + i as f32).unwrap();
                writer.write_chunk(&device_id, chunk).unwrap();
                writer.end_chunk_group(&device_id).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer archivo y verificar header + footer
        let mut file = File::open(&path).unwrap();

        // Leer header
        let header = FileHeader::deserialize(&mut file).unwrap();

        // Verificar flag de dictionary
        assert!(header.flags.contains(FileFlags::HAS_GLOBAL_DICTIONARY));
        assert!(header.dictionary_offset > 0);

        // Leer footer desde el final
        file.seek(std::io::SeekFrom::End(-(FileFooter::SERIALIZED_SIZE as i64))).unwrap();
        let footer = FileFooter::deserialize(&mut file).unwrap();

        assert!(footer.dictionary_offset > 0);
        assert!(footer.dictionary_size > 0);

        // Leer y verificar diccionario
        file.seek(std::io::SeekFrom::Start(footer.dictionary_offset)).unwrap();
        let dict = GlobalDictionary::deserialize(&mut file).unwrap();

        // Debe tener 4 strings: sensor_000, sensor_001, sensor_002, temperature
        assert_eq!(dict.len(), 4);
        assert!(dict.get_id("sensor_000").is_some());
        assert!(dict.get_id("sensor_001").is_some());
        assert!(dict.get_id("sensor_002").is_some());
        assert!(dict.get_id("temperature").is_some());
    }
}
