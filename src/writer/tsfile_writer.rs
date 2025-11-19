use crate::common::{MeasurementSchema, Tablet, TsRecord, TsValue};
use crate::error::{Result, TsFileError};
use crate::writer::{ChunkWriter, TsFileIOWriter};
use std::collections::HashMap;
use std::path::Path;

/// Configuración para TsFileWriter
#[derive(Debug, Clone)]
pub struct TsFileConfig {
    /// Tamaño máximo de página en bytes
    pub max_page_size: usize,
    /// Tamaño de chunk group antes de flush
    pub chunk_group_size: usize,
}

impl Default for TsFileConfig {
    fn default() -> Self {
        Self {
            max_page_size: 64 * 1024,     // 64KB
            chunk_group_size: 100 * 1024, // 100KB
        }
    }
}

/// High-level TsFile writer con API conveniente
pub struct TsFileWriter {
    io_writer: TsFileIOWriter,
    config: TsFileConfig,
    // OPT-1: Vec en lugar de HashMap interno para acceso O(1) sin allocations
    schemas: HashMap<String, Vec<MeasurementSchema>>,
    current_device: Option<String>,
    // OPT-1: Vec de writers alineado con schemas (índice directo, sin HashMap lookups)
    current_writers: Vec<ChunkWriter>,
}

impl TsFileWriter {
    /// Crea un nuevo TsFileWriter
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            io_writer: TsFileIOWriter::new(path)?,
            config: TsFileConfig::default(),
            schemas: HashMap::new(),
            current_device: None,
            current_writers: Vec::new(),
        })
    }

    /// Crea un TsFileWriter con configuración personalizada
    pub fn with_config<P: AsRef<Path>>(path: P, config: TsFileConfig) -> Result<Self> {
        let mut writer = Self::new(path)?;
        writer.config = config;
        Ok(writer)
    }

    /// Registra un schema para una medición de un dispositivo
    pub fn register_timeseries(
        &mut self,
        device_id: impl Into<String>,
        schema: MeasurementSchema,
    ) -> Result<()> {
        let device_id = device_id.into();

        // OPT-1: Agregar a Vec en lugar de HashMap (mantener orden de registro)
        self.schemas
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push(schema);

        Ok(())
    }

    /// Registra múltiples schemas para un dispositivo
    pub fn register_device(
        &mut self,
        device_id: impl Into<String>,
        schemas: Vec<MeasurementSchema>,
    ) -> Result<()> {
        let device_id = device_id.into();
        for schema in schemas {
            self.register_timeseries(device_id.clone(), schema)?;
        }
        Ok(())
    }

    /// Verifica si una medición ya está registrada para un dispositivo
    pub fn has_measurement(&self, device_id: &str, measurement_name: &str) -> bool {
        self.schemas
            .get(device_id)
            .map(|device_schemas| {
                device_schemas
                    .iter()
                    .any(|s| s.measurement_name == measurement_name)
            })
            .unwrap_or(false)
    }

    /// Escribe un registro (TsRecord)
    pub fn write_record(&mut self, record: TsRecord) -> Result<()> {
        let device_id = record.device_id.clone();

        // Iniciar chunk group si es necesario
        if self.current_device.as_ref() != Some(&device_id) {
            if let Some(prev_device) = self.current_device.clone() {
                self.flush_device(&prev_device)?;
            }
            self.io_writer.start_chunk_group(&device_id)?;
            self.current_device = Some(device_id.clone());

            // Inicializar writers para este device
            let device_schemas = self.schemas.get(&device_id).ok_or_else(|| {
                TsFileError::SchemaError(format!("No schemas registered for device {}", device_id))
            })?;

            self.current_writers.clear();
            self.current_writers.reserve(device_schemas.len());

            for schema in device_schemas.iter() {
                let writer = ChunkWriter::with_page_size(
                    schema.measurement_name.clone(),
                    schema.data_type,
                    schema.encoding,
                    schema.compression,
                    self.config.max_page_size,
                );
                self.current_writers.push(writer);
            }
        }

        // Obtener schemas del device
        let device_schemas = self.schemas.get(&device_id).ok_or_else(|| {
            TsFileError::SchemaError(format!("No schemas registered for device {}", device_id))
        })?;

        // Escribir cada punto buscando su índice
        for point in record.points {
            // Buscar índice del measurement en el Vec de schemas
            let col_idx = device_schemas
                .iter()
                .position(|s| s.measurement_name == point.measurement_name)
                .ok_or_else(|| {
                    TsFileError::SchemaError(format!(
                        "No schema for measurement {}",
                        point.measurement_name
                    ))
                })?;

            let chunk_writer = &mut self.current_writers[col_idx];

            // Escribir valor según tipo
            if let Some(value) = point.value {
                Self::write_value_to_chunk(chunk_writer, record.timestamp, value)?;
            }
        }

        Ok(())
    }

    /// Escribe un tablet (batch writing)
    pub fn write_tablet(&mut self, tablet: &Tablet) -> Result<()> {
        let device_id = tablet.device_name.clone();

        // Iniciar chunk group si es necesario
        if self.current_device.as_ref() != Some(&device_id) {
            if let Some(prev_device) = self.current_device.clone() {
                self.flush_device(&prev_device)?;
            }
            self.io_writer.start_chunk_group(&device_id)?;
            self.current_device = Some(device_id.clone());

            // OPT-1: Inicializar writers para el nuevo device
            self.current_writers.clear();
            self.current_writers.reserve(tablet.schemas.len());

            for schema in tablet.schemas.iter() {
                let writer = ChunkWriter::with_page_size(
                    schema.measurement_name.clone(),
                    schema.data_type,
                    schema.encoding,
                    schema.compression,
                    self.config.max_page_size,
                );
                self.current_writers.push(writer);
            }
        }

        // OPT-1: Acceso directo por índice (0 allocations, 0 HashMap lookups)
        // ANTES: 300K × (format! + clone + hash lookup) = 15-30ms
        // AHORA: 300K × array[idx] = ~0ms
        for (col_idx, chunk_writer) in self.current_writers.iter_mut().enumerate() {
            // Escribir todos los valores de esta columna
            for row_idx in 0..tablet.row_count() {
                if !tablet.bitmaps[col_idx].get(row_idx) {
                    let timestamp = tablet.timestamps[row_idx];
                    Self::write_column_value(
                        chunk_writer,
                        &tablet.values[col_idx],
                        row_idx,
                        timestamp,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Escribe un valor del tablet a un chunk
    fn write_column_value(
        chunk_writer: &mut ChunkWriter,
        value_matrix: &crate::common::ValueMatrix,
        row_idx: usize,
        timestamp: i64,
    ) -> Result<()> {
        use crate::common::ValueMatrix;

        match value_matrix {
            ValueMatrix::Boolean(v) => chunk_writer.write_bool(timestamp, v[row_idx])?,
            ValueMatrix::Int32(v) => chunk_writer.write_i32(timestamp, v[row_idx])?,
            ValueMatrix::Int64(v) => chunk_writer.write_i64(timestamp, v[row_idx])?,
            ValueMatrix::Float(v) => chunk_writer.write_f32(timestamp, v[row_idx])?,
            ValueMatrix::Double(v) => chunk_writer.write_f64(timestamp, v[row_idx])?,
            ValueMatrix::Text(v) => chunk_writer.write_string(timestamp, &v[row_idx])?,
        }

        Ok(())
    }

    /// Escribe un valor a un chunk
    fn write_value_to_chunk(
        chunk_writer: &mut ChunkWriter,
        timestamp: i64,
        value: TsValue,
    ) -> Result<()> {
        match value {
            TsValue::Boolean(v) => chunk_writer.write_bool(timestamp, v)?,
            TsValue::Int32(v) => chunk_writer.write_i32(timestamp, v)?,
            TsValue::Int64(v) => chunk_writer.write_i64(timestamp, v)?,
            TsValue::Float(v) => chunk_writer.write_f32(timestamp, v)?,
            TsValue::Double(v) => chunk_writer.write_f64(timestamp, v)?,
            TsValue::Text(v) | TsValue::String(v) => chunk_writer.write_string(timestamp, &v)?,
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "supported type".to_string(),
                    actual: format!("{:?}", value),
                });
            }
        }

        Ok(())
    }

    /// Hace flush de todos los chunks de un dispositivo
    fn flush_device(&mut self, device_id: &str) -> Result<()> {
        // OPT-1: Iterar sobre Vec directo en lugar de filtrar HashMap
        for chunk_writer in self.current_writers.drain(..) {
            self.io_writer.write_chunk(device_id, chunk_writer)?;
        }

        self.io_writer.end_chunk_group(device_id)?;
        Ok(())
    }

    /// Hace flush de todos los datos pendientes
    pub fn flush(&mut self) -> Result<()> {
        if let Some(device_id) = self.current_device.clone() {
            self.flush_device(&device_id)?;
            self.current_device = None;
        }
        Ok(())
    }

    /// Cierra el archivo
    pub fn close(mut self) -> Result<()> {
        self.flush()?;
        self.io_writer.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CompressionType, TSDataType, TSEncoding};
    use tempfile::NamedTempFile;

    #[test]
    fn test_tsfile_writer_record() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = TsFileWriter::new(temp_file.path()).unwrap();

        // Registrar schema
        let schema = MeasurementSchema::new(
            "temperature",
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Lz4,
        );
        writer.register_timeseries("device1", schema).unwrap();

        // Escribir registros
        let record = TsRecord::new(1000, "device1").with_value("temperature", TsValue::Float(25.5));
        writer.write_record(record).unwrap();

        let record = TsRecord::new(2000, "device1").with_value("temperature", TsValue::Float(26.0));
        writer.write_record(record).unwrap();

        // Cerrar
        writer.close().unwrap();
    }

    #[test]
    fn test_tsfile_writer_tablet() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut writer = TsFileWriter::new(temp_file.path()).unwrap();

        // Crear tablet
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new(
            "device1",
            schemas.clone(),
            vec![crate::common::ColumnCategory::Field; 2],
            100,
        );

        // Agregar datos
        tablet
            .add_row(
                1000,
                vec![Some(TsValue::Float(25.5)), Some(TsValue::Int32(60))],
            )
            .unwrap();

        tablet
            .add_row(
                2000,
                vec![Some(TsValue::Float(26.0)), Some(TsValue::Int32(65))],
            )
            .unwrap();

        // Escribir tablet
        writer.write_tablet(&tablet).unwrap();

        // Cerrar
        writer.close().unwrap();
    }
}
