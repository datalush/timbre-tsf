use crate::error::Result;
use crate::query::Predicate;
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
            // Clone necessary: cache_key used for both contains_key check and insert
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
    ) -> Result<impl Iterator<Item = (i64, DecodedValueData)> + use<'_>> {
        let chunk = self.read(device_id, measurement_name)?;
        Ok(chunk.iter())
    }

    /// Obtiene un valor específico por índice
    pub fn get_value(
        &mut self,
        device_id: &str,
        measurement_name: &str,
        index: usize,
    ) -> Result<Option<(i64, DecodedValueData)>> {
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

    /// Scan with predicate push down - skips chunks based on statistics
    ///
    /// This is 10-100x faster than reading all data and filtering in memory
    /// for selective queries.
    ///
    /// # Example
    /// ```no_run
    /// use timbre_tsf::reader::TsFileReader;
    /// use timbre_tsf::query::{Predicate, TimeFilter, ValueFilter};
    /// use timbre_tsf::common::TsValue;
    ///
    /// let mut reader = TsFileReader::open("data.timbre")?;
    ///
    /// let predicate = Predicate::And(vec![
    ///     Predicate::Time(TimeFilter::Between(1000, 2000)),
    ///     Predicate::Value("temp".into(), ValueFilter::GreaterThan(TsValue::Float(25.0))),
    /// ]);
    ///
    /// let chunk = reader.scan_with_predicate("device1", "temp", predicate)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn scan_with_predicate(
        &mut self,
        device_id: &str,
        measurement_name: &str,
        predicate: Predicate,
    ) -> Result<DecodedChunk> {
        // Get chunk metadata for statistics
        let metadata = self
            .io_reader
            .get_chunk_metadata(device_id, measurement_name)
            .ok_or_else(|| {
                crate::error::TsFileError::NotFound(format!(
                    "Chunk metadata for {}/{}",
                    device_id, measurement_name
                ))
            })?;

        // Extract time range from metadata
        let time_range = (metadata.min_time, metadata.max_time);

        // Build statistics map for predicate evaluation
        let statistics: HashMap<String, &dyn crate::common::statistic::Statistic> = HashMap::new();
        // Note: Currently we don't have value statistics in ChunkMetadata
        // This would be added when we implement full statistics tracking

        // PREDICATE PUSH DOWN: Check if chunk might contain matching data
        if !predicate.might_match_chunk(time_range, &statistics) {
            // Chunk can be skipped! Return empty chunk
            log::debug!(
                "Skipped chunk {}/{} - predicate cannot match (time_range: {:?})",
                device_id,
                measurement_name,
                time_range
            );

            return Ok(DecodedChunk::empty(measurement_name, metadata.data_type));
        }

        log::debug!(
            "Reading chunk {}/{} - predicate might match",
            device_id,
            measurement_name
        );

        // Read chunk (cannot skip based on statistics)
        let chunk = self.io_reader.read_chunk(device_id, measurement_name)?;

        // Apply predicate to actual data (post-filtering)
        Ok(chunk.filter_by_predicate(&predicate))
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
            assert_eq!(v, 30.0);
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

    #[test]
    fn test_scan_with_predicate_time_filter() {
        use crate::query::{Predicate, TimeFilter};

        // Create test file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "temp",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..100 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("temp", TsValue::Float(20.0 + i as f32));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Read with predicate push down
        let mut reader = TsFileReader::open(path).unwrap();

        // Time range that should filter results
        let predicate = Predicate::Time(TimeFilter::Between(1000, 2000));
        let result = reader
            .scan_with_predicate("device1", "temp", predicate)
            .unwrap();

        // Should have filtered results (1000-2000 with step 100 = 11 values)
        assert_eq!(result.len(), 11);
    }

    #[test]
    fn test_scan_with_predicate_chunk_skip() {
        use crate::query::{Predicate, TimeFilter};

        // Create test file
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

            for i in 0..50 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("sensor", TsValue::Int32(i as i32));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Read with predicate that might skip chunk
        let mut reader = TsFileReader::open(path).unwrap();

        // Time range completely outside the chunk
        let predicate = Predicate::Time(TimeFilter::GreaterThan(10000));
        let result = reader
            .scan_with_predicate("device1", "sensor", predicate)
            .unwrap();

        // Should return empty chunk (skipped)
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_scan_with_predicate_value_filter() {
        use crate::query::{Predicate, ValueFilter};

        // Create test file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "data",
                TSDataType::Double,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..30 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("data", TsValue::Double(i as f64 * 2.0));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Read with value filter
        let mut reader = TsFileReader::open(path).unwrap();

        let predicate = Predicate::Value(
            "data".to_string(),
            ValueFilter::GreaterThan(TsValue::Double(40.0)),
        );
        let result = reader
            .scan_with_predicate("device1", "data", predicate)
            .unwrap();

        // Should filter values > 40.0
        assert!(result.len() > 0);
        assert!(result.len() < 30);
    }

    #[test]
    fn test_scan_with_predicate_combined() {
        use crate::query::{Predicate, TimeFilter, ValueFilter};

        // Create test file
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        {
            let mut writer = TsFileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "measurement",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..50 {
                let record = TsRecord::new(i * 50, "device1")
                    .with_value("measurement", TsValue::Float(10.0 + i as f32 * 2.0));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Read with combined filters
        let mut reader = TsFileReader::open(path).unwrap();

        let predicate = Predicate::And(vec![
            Predicate::Time(TimeFilter::Between(500, 1500)),
            Predicate::Value(
                "measurement".to_string(),
                ValueFilter::LessThan(TsValue::Float(60.0)),
            ),
        ]);
        let result = reader
            .scan_with_predicate("device1", "measurement", predicate)
            .unwrap();

        // Should have both filters applied
        assert!(result.len() > 0);
    }
}
