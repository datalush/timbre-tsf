use crate::error::{Result, TsFileError};
use crate::file::ChunkMeta;
use crate::writer::ChunkWriter;
use byteorder::{LittleEndian, WriteBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::Path;

/// Magic string para TsFile
pub const MAGIC_STRING: &[u8] = b"TsFile";
/// Versión del formato
pub const VERSION: u8 = 3;

/// Low-level TsFile writer que maneja la estructura física del archivo
pub struct TsFileIOWriter {
    file: BufWriter<File>,
    device_chunk_groups: HashMap<String, Vec<ChunkMeta>>,
    current_position: u64,
}

impl TsFileIOWriter {
    /// Crea un nuevo TsFileIOWriter
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::create(path)?;

        // OPT-A: Wrap file in BufWriter with 256KB buffer (vs 8KB default)
        // Reduces system calls from thousands to dozens
        const BUFFER_SIZE: usize = 256 * 1024; // 256KB
        let mut file = BufWriter::with_capacity(BUFFER_SIZE, file);

        // Escribir magic string y versión
        file.write_all(MAGIC_STRING)?;
        file.write_u8(VERSION)?;

        let current_position = file.stream_position()?;

        Ok(Self {
            file,
            device_chunk_groups: HashMap::new(),
            current_position,
        })
    }

    /// Inicia un nuevo chunk group para un dispositivo
    pub fn start_chunk_group(&mut self, device_id: &str) -> Result<()> {
        if !self.device_chunk_groups.contains_key(device_id) {
            self.device_chunk_groups
                .insert(device_id.to_string(), Vec::new());
        }
        Ok(())
    }

    /// Escribe un chunk al archivo
    pub fn write_chunk(&mut self, device_id: &str, mut chunk_writer: ChunkWriter) -> Result<u64> {
        let chunk_offset = self.current_position;

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
            .or_insert_with(Vec::new)
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
        // Escribir metadata
        let metadata_offset = self.file.stream_position()?;
        self.write_metadata()?;

        // Escribir offset de metadata
        self.file.write_u64::<LittleEndian>(metadata_offset)?;

        // Magic string al final
        self.file.write_all(MAGIC_STRING)?;

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
}
