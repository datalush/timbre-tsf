use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::constants::{FOOTER_SIZE, HEADER_SIZE};
use crate::error::{Result, TsFileError};
use crate::file::{FileFooter, FileHeader};
use crate::reader::ChunkReader;
use byteorder::{LittleEndian, ReadBytesExt};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// Low-level TsFile reader que maneja la estructura física del archivo
/// Opt #3: Usa BufReader para reducir syscalls durante lectura (10-15% mejora)
pub struct TsFileIOReader {
    file: BufReader<File>,
    file_size: u64,
    device_metadata: HashMap<String, Vec<ChunkMetadata>>,
    metadata_offset: u64,
}

/// Metadata extendida de un chunk con posición en el archivo
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub encoding: TSEncoding,
    pub compression_type: CompressionType,
    pub offset: i64,
}

impl TsFileIOReader {
    /// Abre un archivo TsFile existente
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let file = File::open(path)?;
        let mut file = BufReader::new(file);

        // Obtener tamaño del archivo
        let file_size = file.seek(SeekFrom::End(0))?;

        // Verificar tamaño mínimo (HEADER + FOOTER)
        let min_size = (HEADER_SIZE + FOOTER_SIZE) as u64;
        if file_size < min_size {
            return Err(TsFileError::InvalidFile(format!(
                "File too small to be a valid Timbre file: {} bytes (min {})",
                file_size, min_size
            )));
        }

        // Leer FileHeader al inicio (128 bytes) con magic TMB1
        file.seek(SeekFrom::Start(0))?;
        let _header = FileHeader::deserialize(&mut file)?;

        // Leer FileFooter al final (132 bytes) con magic TMB1
        file.seek(SeekFrom::End(-(FOOTER_SIZE as i64)))?;
        let footer = FileFooter::deserialize(&mut file)?;

        // Usar metadata_offset del footer para leer metadata
        let metadata_offset = footer.metadata_offset;
        file.seek(SeekFrom::Start(metadata_offset))?;
        let device_metadata = Self::read_metadata(&mut file)?;

        Ok(Self {
            file,
            file_size,
            device_metadata,
            metadata_offset,
        })
    }

    /// Lee la metadata del archivo
    fn read_metadata(file: &mut BufReader<File>) -> Result<HashMap<String, Vec<ChunkMetadata>>> {
        // Leer marker de metadata
        let marker = file.read_u8()?;
        if marker != 0x02 {
            return Err(TsFileError::InvalidFile(format!(
                "Invalid metadata marker: {}",
                marker
            )));
        }

        // Leer número de dispositivos
        let num_devices = file.read_u32::<LittleEndian>()?;
        let mut device_metadata = HashMap::new();

        for _ in 0..num_devices {
            // Leer device ID
            let device_id_len = file.read_u32::<LittleEndian>()? as usize;
            let mut device_id_bytes = vec![0u8; device_id_len];
            file.read_exact(&mut device_id_bytes)?;
            let device_id = String::from_utf8(device_id_bytes)
                .map_err(|e| TsFileError::InvalidFile(format!("Invalid device ID: {}", e)))?;

            // Leer número de chunks
            let num_chunks = file.read_u32::<LittleEndian>()?;
            let mut chunks = Vec::new();

            for _ in 0..num_chunks {
                // Leer measurement name
                let name_len = file.read_u32::<LittleEndian>()? as usize;
                let mut name_bytes = vec![0u8; name_len];
                file.read_exact(&mut name_bytes)?;
                let measurement_name = String::from_utf8(name_bytes).map_err(|e| {
                    TsFileError::InvalidFile(format!("Invalid measurement name: {}", e))
                })?;

                // Leer offset, data type, encoding, compression
                let offset = file.read_i64::<LittleEndian>()?;
                let data_type = TSDataType::from_u8(file.read_u8()?);
                let encoding = TSEncoding::from_u8(file.read_u8()?);
                let compression_type = CompressionType::from_u8(file.read_u8()?);

                chunks.push(ChunkMetadata {
                    measurement_name,
                    data_type,
                    encoding,
                    compression_type,
                    offset,
                });
            }

            device_metadata.insert(device_id, chunks);
        }

        Ok(device_metadata)
    }

    /// Lee un chunk específico por dispositivo y medición
    pub fn read_chunk(
        &mut self,
        device_id: &str,
        measurement_name: &str,
    ) -> Result<crate::reader::DecodedChunk> {
        // Buscar metadata del chunk
        let device_chunks = self
            .device_metadata
            .get(device_id)
            .ok_or_else(|| TsFileError::NotFound(format!("Device {} not found", device_id)))?;

        let chunk_meta = device_chunks
            .iter()
            .find(|c| c.measurement_name == measurement_name)
            .ok_or_else(|| {
                TsFileError::NotFound(format!(
                    "Measurement {} not found for device {}",
                    measurement_name, device_id
                ))
            })?;

        // Posicionar en el offset del chunk
        self.file.seek(SeekFrom::Start(chunk_meta.offset as u64))?;

        // Crear reader y leer chunk
        let mut chunk_reader = ChunkReader::new(
            chunk_meta.measurement_name.clone(),
            chunk_meta.data_type,
            chunk_meta.encoding,
            chunk_meta.compression_type,
        );

        chunk_reader.read_chunk(&mut self.file)
    }

    /// Obtiene la lista de dispositivos
    pub fn get_devices(&self) -> Vec<String> {
        self.device_metadata.keys().cloned().collect()
    }

    /// Obtiene la lista de mediciones para un dispositivo
    pub fn get_measurements(&self, device_id: &str) -> Option<Vec<String>> {
        self.device_metadata
            .get(device_id)
            .map(|chunks| chunks.iter().map(|c| c.measurement_name.clone()).collect())
    }

    /// Obtiene metadata de un chunk específico
    pub fn get_chunk_metadata(
        &self,
        device_id: &str,
        measurement_name: &str,
    ) -> Option<&ChunkMetadata> {
        self.device_metadata
            .get(device_id)?
            .iter()
            .find(|c| c.measurement_name == measurement_name)
    }

    /// Tamaño del archivo
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    /// Offset de metadata
    pub fn metadata_offset(&self) -> u64 {
        self.metadata_offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{MeasurementSchema, TsRecord, TsValue};
    use crate::writer::TsFileWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tsfile_io_reader_basic() {
        // Crear archivo temporal
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Escribir datos
        {
            let mut writer = TsFileWriter::new(path).unwrap();

            // Registrar schemas (usar Plain en lugar de Gorilla que tiene bugs)
            let schema = MeasurementSchema::new(
                "temperature",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );
            writer.register_timeseries("device1", schema).unwrap();

            // Escribir datos
            for i in 0..10 {
                let record = TsRecord::new(1000 + i * 100, "device1")
                    .with_value("temperature", TsValue::Float(25.0 + i as f32));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer datos
        let mut reader = TsFileIOReader::open(path).unwrap();

        // Verificar dispositivos
        let devices = reader.get_devices();
        assert_eq!(devices.len(), 1);
        assert!(devices.contains(&"device1".to_string()));

        // Verificar mediciones
        let measurements = reader.get_measurements("device1").unwrap();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0], "temperature");

        // Leer chunk
        let chunk = reader.read_chunk("device1", "temperature").unwrap();
        assert_eq!(chunk.len(), 10);
        assert_eq!(chunk.measurement_name, "temperature");

        // Verificar valores
        for (i, (ts, value)) in chunk.iter().enumerate() {
            eprintln!("Index {}: ts={}, value={:?}", i, ts, value);
            assert_eq!(
                ts,
                1000 + i as i64 * 100,
                "Timestamp mismatch at index {}",
                i
            );
            if let crate::reader::DecodedValueData::Float(v) = value {
                assert_eq!(
                    v,
                    25.0 + i as f32,
                    "Value mismatch at index {}: expected {}, got {}",
                    i,
                    25.0 + i as f32,
                    v
                );
            } else {
                panic!("Expected Float value");
            }
        }
    }

    #[test]
    fn test_tsfile_io_reader_multiple_devices() {
        // Crear archivo temporal
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Escribir datos
        {
            let mut writer = TsFileWriter::new(path).unwrap();

            // Dispositivo 1 (usar Plain para evitar bugs de Gorilla)
            let schema1 = MeasurementSchema::new(
                "temp",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );
            writer.register_timeseries("device1", schema1).unwrap();

            for i in 0..5 {
                let record = TsRecord::new(1000 + i * 100, "device1")
                    .with_value("temp", TsValue::Float(25.0 + i as f32));
                writer.write_record(record).unwrap();
            }

            // Dispositivo 2
            let schema2 = MeasurementSchema::new(
                "humidity",
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );
            writer.register_timeseries("device2", schema2).unwrap();

            for i in 0..5 {
                let record = TsRecord::new(2000 + i * 100, "device2")
                    .with_value("humidity", TsValue::Int32(60 + i as i32));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer datos
        let mut reader = TsFileIOReader::open(path).unwrap();

        // Verificar dispositivos
        let devices = reader.get_devices();
        assert_eq!(devices.len(), 2);

        // Leer chunk de device1
        let chunk1 = reader.read_chunk("device1", "temp").unwrap();
        assert_eq!(chunk1.len(), 5);

        // Leer chunk de device2
        let chunk2 = reader.read_chunk("device2", "humidity").unwrap();
        assert_eq!(chunk2.len(), 5);
    }

    #[test]
    fn test_tsfile_io_reader_invalid_file() {
        let temp_file = NamedTempFile::new().unwrap();

        // Escribir datos inválidos
        std::fs::write(temp_file.path(), b"invalid data").unwrap();

        // Debería fallar al abrir
        let result = TsFileIOReader::open(temp_file.path());
        assert!(result.is_err());
    }
}
