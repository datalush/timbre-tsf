use crate::error::Result;
use crate::reader::{DecodedChunk, DecodedValueData, TsFileIOReader};
use std::collections::HashMap;
use std::path::Path;

/// High-level TsFile reader con API conveniente
pub struct TsFileReader {
    io_reader: TsFileIOReader,
    chunk_cache: HashMap<String, DecodedChunk>,
}

impl TsFileReader {
    /// Abre un archivo TsFile
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            io_reader: TsFileIOReader::open(path)?,
            chunk_cache: HashMap::new(),
        })
    }

    /// Lista todos los dispositivos
    pub fn devices(&self) -> Vec<String> {
        self.io_reader.get_devices()
    }

    /// Lista todas las mediciones de un dispositivo
    pub fn measurements(&self, device_id: &str) -> Option<Vec<String>> {
        self.io_reader.get_measurements(device_id)
    }

    /// Lee todos los datos de un dispositivo y medición
    pub fn read(&mut self, device_id: &str, measurement_name: &str) -> Result<&DecodedChunk> {
        let cache_key = format!("{}:{}", device_id, measurement_name);

        if !self.chunk_cache.contains_key(&cache_key) {
            let chunk = self.io_reader.read_chunk(device_id, measurement_name)?;
            self.chunk_cache.insert(cache_key.clone(), chunk);
        }

        Ok(self.chunk_cache.get(&cache_key).unwrap())
    }

    /// Lee datos filtrados por rango de tiempo
    pub fn read_time_range(
        &mut self,
        device_id: &str,
        measurement_name: &str,
        min_time: i64,
        max_time: i64,
    ) -> Result<DecodedChunk> {
        let chunk = self.read(device_id, measurement_name)?;
        Ok(chunk.filter_time_range(min_time, max_time))
    }

    /// Lee todos los datos de un dispositivo (todas las mediciones)
    pub fn read_device(&mut self, device_id: &str) -> Result<HashMap<String, DecodedChunk>> {
        let measurements = self
            .measurements(device_id)
            .ok_or_else(|| crate::error::TsFileError::NotFound(format!("Device {}", device_id)))?;

        let mut result = HashMap::new();
        for measurement in measurements {
            let chunk = self.io_reader.read_chunk(device_id, &measurement)?;
            result.insert(measurement, chunk);
        }

        Ok(result)
    }

    /// Itera sobre todos los valores de una medición
    pub fn iter_measurement(
        &mut self,
        device_id: &str,
        measurement_name: &str,
    ) -> Result<impl Iterator<Item = (i64, &DecodedValueData)>> {
        let chunk = self.read(device_id, measurement_name)?;
        Ok(chunk.iter())
    }

    /// Obtiene un valor específico por índice
    pub fn get_value(
        &mut self,
        device_id: &str,
        measurement_name: &str,
        index: usize,
    ) -> Result<Option<(i64, &DecodedValueData)>> {
        let chunk = self.read(device_id, measurement_name)?;
        Ok(chunk.get(index))
    }

    /// Número total de valores en una medición
    pub fn count(&mut self, device_id: &str, measurement_name: &str) -> Result<usize> {
        let chunk = self.read(device_id, measurement_name)?;
        Ok(chunk.len())
    }

    /// Limpia la caché de chunks
    pub fn clear_cache(&mut self) {
        self.chunk_cache.clear();
    }

    /// Tamaño del archivo
    pub fn file_size(&self) -> u64 {
        self.io_reader.file_size()
    }

    /// Información del archivo
    pub fn info(&self) -> TsFileInfo {
        let devices = self.devices();
        let mut total_chunks = 0;

        for device in &devices {
            if let Some(measurements) = self.measurements(device) {
                total_chunks += measurements.len();
            }
        }

        TsFileInfo {
            file_size: self.file_size(),
            num_devices: devices.len(),
            num_chunks: total_chunks,
            devices,
        }
    }
}

/// Información sobre un archivo TsFile
#[derive(Debug, Clone)]
pub struct TsFileInfo {
    pub file_size: u64,
    pub num_devices: usize,
    pub num_chunks: usize,
    pub devices: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{
        CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue,
    };
    use crate::writer::TsFileWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tsfile_reader_basic() {
        // Crear archivo
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "temperature",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..10 {
                let record = TsRecord::new(1000 + i * 100, "device1")
                    .with_value("temperature", TsValue::Float(25.0 + i as f32));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer archivo
        let mut reader = TsFileReader::open(path).unwrap();

        // Verificar dispositivos
        let devices = reader.devices();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0], "device1");

        // Verificar mediciones
        let measurements = reader.measurements("device1").unwrap();
        assert_eq!(measurements.len(), 1);
        assert_eq!(measurements[0], "temperature");

        // Leer datos
        let chunk = reader.read("device1", "temperature").unwrap();
        assert_eq!(chunk.len(), 10);

        // Verificar valor específico
        let (ts, value) = reader
            .get_value("device1", "temperature", 5)
            .unwrap()
            .unwrap();
        assert_eq!(ts, 1500);
        if let DecodedValueData::Float(v) = value {
            assert_eq!(*v, 30.0);
        } else {
            panic!("Expected Float value");
        }

        // Contar valores
        let count = reader.count("device1", "temperature").unwrap();
        assert_eq!(count, 10);
    }

    #[test]
    fn test_tsfile_reader_time_range() {
        // Crear archivo
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "sensor",
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..20 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("sensor", TsValue::Int32(i as i32 * 5));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer con filtro de tiempo
        let mut reader = TsFileReader::open(path).unwrap();
        let filtered = reader
            .read_time_range("device1", "sensor", 500, 1500)
            .unwrap();

        // Debe tener valores de timestamp 500 a 1500 (11 valores)
        assert_eq!(filtered.len(), 11);

        for (i, (ts, _)) in filtered.iter().enumerate() {
            assert_eq!(ts, 500 + i as i64 * 100);
        }
    }

    #[test]
    fn test_tsfile_reader_multiple_devices() {
        // Crear archivo
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();

            // Device 1
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

            // Device 2
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

        // Leer archivo
        let mut reader = TsFileReader::open(path).unwrap();

        // Verificar info
        let info = reader.info();
        assert_eq!(info.num_devices, 2);
        assert_eq!(info.num_chunks, 2);

        // Leer device completo
        let device1_data = reader.read_device("device1").unwrap();
        assert_eq!(device1_data.len(), 1);
        assert!(device1_data.contains_key("temp"));

        // Verificar iterador
        let mut count = 0;
        for (ts, _) in reader.iter_measurement("device2", "humidity").unwrap() {
            assert!(ts >= 2000 && ts < 2500);
            count += 1;
        }
        assert_eq!(count, 5);
    }

    #[test]
    fn test_tsfile_reader_cache() {
        // Crear archivo
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "data",
                TSDataType::Int64,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..10 {
                let record =
                    TsRecord::new(i * 100, "device1").with_value("data", TsValue::Int64(i));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Leer archivo
        let mut reader = TsFileReader::open(path).unwrap();

        // Primera lectura (carga en caché)
        {
            let chunk = reader.read("device1", "data").unwrap();
            assert_eq!(chunk.len(), 10);
        }

        // Segunda lectura (desde caché) - debería ser rápida
        {
            let chunk = reader.read("device1", "data").unwrap();
            assert_eq!(chunk.len(), 10);
        }

        // Limpiar caché
        reader.clear_cache();

        // Lectura después de limpiar caché
        {
            let chunk = reader.read("device1", "data").unwrap();
            assert_eq!(chunk.len(), 10);
        }
    }
}
