//! PageWriter con soporte nativo para mini-blocks (Timbre format)
//!
//! Innovación core de Timbre:
//! - Acumula datos RAW (timestamps + values sin encodear)
//! - En finish(), divide en 4-8 mini-blocks
//! - Cada mini-block se encodea y comprime independientemente
//! - Permite decodificación paralela (8x speedup potencial)

use crate::common::statistic::{Statistic, create_statistic};
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::compress::create_compressor;
use crate::encoding::create_encoder;
use crate::error::{Result, TsFileError};
use crate::file::{MiniBlock, MiniBlockConfig, MiniBlockHeader, PageData, PageHeader};

/// Writer para páginas con mini-blocks (Timbre format)
pub struct PageWriter {
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,
    miniblock_config: MiniBlockConfig,

    // Datos RAW acumulados (sin encodear)
    timestamps: Vec<i64>,
    // ValueData es un enum para diferentes tipos
    value_data: ValueData,

    statistic: Box<dyn Statistic>,
}

/// Datos de valores acumulados por tipo
#[derive(Debug)]
enum ValueData {
    Boolean(Vec<bool>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    String(Vec<String>),
}

impl ValueData {
    fn new(data_type: TSDataType) -> Self {
        match data_type {
            TSDataType::Boolean => ValueData::Boolean(Vec::new()),
            TSDataType::Int32 | TSDataType::Date => ValueData::Int32(Vec::new()),
            TSDataType::Int64 | TSDataType::Timestamp => ValueData::Int64(Vec::new()),
            TSDataType::Float => ValueData::Float(Vec::new()),
            TSDataType::Double => ValueData::Double(Vec::new()),
            TSDataType::Text | TSDataType::String => ValueData::String(Vec::new()),
            _ => ValueData::Int32(Vec::new()), // Default
        }
    }

    fn len(&self) -> usize {
        match self {
            ValueData::Boolean(v) => v.len(),
            ValueData::Int32(v) => v.len(),
            ValueData::Int64(v) => v.len(),
            ValueData::Float(v) => v.len(),
            ValueData::Double(v) => v.len(),
            ValueData::String(v) => v.len(),
        }
    }
}

impl PageWriter {
    pub fn new(
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        Self {
            data_type,
            encoding,
            compression_type,
            miniblock_config: MiniBlockConfig::default(),
            timestamps: Vec::new(),
            value_data: ValueData::new(data_type),
            statistic: create_statistic(data_type),
        }
    }

    /// Escribe un valor booleano
    pub fn write_bool(&mut self, timestamp: i64, value: bool) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Boolean(v) => v.push(value),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "Boolean".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_bool(timestamp, value);
        Ok(())
    }

    /// Escribe un valor i32
    pub fn write_i32(&mut self, timestamp: i64, value: i32) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Int32(v) => v.push(value),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "Int32".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_i32(timestamp, value);
        Ok(())
    }

    /// Escribe un valor i64
    pub fn write_i64(&mut self, timestamp: i64, value: i64) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Int64(v) => v.push(value),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "Int64".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_i64(timestamp, value);
        Ok(())
    }

    /// Escribe un valor f32
    pub fn write_f32(&mut self, timestamp: i64, value: f32) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Float(v) => v.push(value),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "Float".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_f32(timestamp, value);
        Ok(())
    }

    /// Escribe un valor f64
    pub fn write_f64(&mut self, timestamp: i64, value: f64) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Double(v) => v.push(value),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "Double".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_f64(timestamp, value);
        Ok(())
    }

    /// Escribe un valor string
    pub fn write_string(&mut self, timestamp: i64, value: &str) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::String(v) => v.push(value.to_string()),
            _ => return Err(TsFileError::TypeMismatch {
                expected: "String".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
        self.statistic.update_string(timestamp, value);
        Ok(())
    }

    /// Finaliza la página y genera mini-blocks
    pub fn finish(&mut self) -> Result<PageData> {
        let total_points = self.timestamps.len();
        if total_points == 0 {
            return Err(TsFileError::InvalidState("No data to write".to_string()));
        }

        // Dividir en ranges de mini-blocks
        let ranges = self.miniblock_config.split_into_ranges(total_points);
        let mut miniblocks = Vec::with_capacity(ranges.len());

        // Crear mini-block para cada range
        for (start, end) in ranges {
            let miniblock = self.create_miniblock(start, end)?;
            miniblocks.push(miniblock);
        }

        // Crear header de página
        let mut header = PageHeader::new();
        header.num_of_values = total_points as i32;
        header.min_timestamp = self.statistic.start_time();
        header.max_timestamp = self.statistic.end_time();

        // Calcular tamaños totales
        let total_compressed: usize = miniblocks.iter().map(|mb| mb.size()).sum();
        header.compressed_size = total_compressed as u32;
        header.uncompressed_size = 0; // No relevante con mini-blocks

        Ok(PageData::with_miniblocks(header, miniblocks))
    }

    /// Crea un mini-block para un rango específico
    fn create_miniblock(&self, start: usize, end: usize) -> Result<MiniBlock> {
        // Extraer timestamps del rango
        let timestamps = &self.timestamps[start..end];
        let point_count = timestamps.len() as u32;
        let min_timestamp = timestamps[0];
        let max_timestamp = timestamps[timestamps.len() - 1];

        // Encodear timestamps
        let mut time_encoder = create_encoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
        let mut time_buffer = Vec::new();
        for &ts in timestamps {
            time_encoder.encode_i64(ts, &mut time_buffer)?;
        }
        time_encoder.flush(&mut time_buffer)?;

        // Encodear values según tipo
        let mut value_buffer = Vec::new();
        let mut value_encoder = create_encoder(self.encoding, self.data_type);

        match &self.value_data {
            ValueData::Boolean(v) => {
                for &val in &v[start..end] {
                    value_encoder.encode_bool(val, &mut value_buffer)?;
                }
            }
            ValueData::Int32(v) => {
                for &val in &v[start..end] {
                    value_encoder.encode_i32(val, &mut value_buffer)?;
                }
            }
            ValueData::Int64(v) => {
                for &val in &v[start..end] {
                    value_encoder.encode_i64(val, &mut value_buffer)?;
                }
            }
            ValueData::Float(v) => {
                for &val in &v[start..end] {
                    value_encoder.encode_f32(val, &mut value_buffer)?;
                }
            }
            ValueData::Double(v) => {
                for &val in &v[start..end] {
                    value_encoder.encode_f64(val, &mut value_buffer)?;
                }
            }
            ValueData::String(v) => {
                for val in &v[start..end] {
                    value_encoder.encode_string(val, &mut value_buffer)?;
                }
            }
        }
        value_encoder.flush(&mut value_buffer)?;

        // Comprimir timestamps y values independientemente
        let mut compressor = create_compressor(self.compression_type);
        let timestamp_uncompressed_size = time_buffer.len() as u32;
        let value_uncompressed_size = value_buffer.len() as u32;

        let timestamp_data = compressor.compress(&time_buffer)?;
        let value_data = compressor.compress(&value_buffer)?;

        // Crear header de mini-block con tamaños compressed y uncompressed
        let header = MiniBlockHeader::new(
            point_count,
            min_timestamp,
            max_timestamp,
            timestamp_data.len() as u32,
            timestamp_uncompressed_size,
            value_data.len() as u32,
            value_uncompressed_size,
        );

        Ok(MiniBlock::new(header, timestamp_data, value_data))
    }

    /// Retorna el número de valores escritos
    pub fn value_count(&self) -> i32 {
        self.timestamps.len() as i32
    }

    /// Retorna las estadísticas actuales
    pub fn statistic(&self) -> &dyn Statistic {
        self.statistic.as_ref()
    }

    /// Resetea el writer para reutilización
    pub fn reset(&mut self) {
        self.timestamps.clear();
        self.value_data = ValueData::new(self.data_type);
        self.statistic = create_statistic(self.data_type);
    }

    /// Tamaño estimado de los datos acumulados
    pub fn estimated_size(&self) -> usize {
        let ts_size = self.timestamps.len() * 8;
        let value_size = match &self.value_data {
            ValueData::Boolean(v) => v.len(),
            ValueData::Int32(v) => v.len() * 4,
            ValueData::Int64(v) => v.len() * 8,
            ValueData::Float(v) => v.len() * 4,
            ValueData::Double(v) => v.len() * 8,
            ValueData::String(v) => v.iter().map(|s| s.len()).sum(),
        };
        ts_size + value_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_writer_miniblock_basic() {
        let mut writer = PageWriter::new(
            TSDataType::Float,
            TSEncoding::Chimp128,
            CompressionType::Zstd,
        );

        // Escribir 1000 puntos (debería crear ~4 mini-blocks)
        for i in 0..1000 {
            writer.write_f32(1000 + i, 20.0 + (i as f32) * 0.1).unwrap();
        }

        let page_data = writer.finish().unwrap();

        // Verificar que se crearon mini-blocks
        assert!(!page_data.miniblocks.is_empty());
        assert!(page_data.miniblocks.len() >= 4);
        assert!(page_data.miniblocks.len() <= 8);

        // Verificar conteo total
        let total_points: u32 = page_data.miniblocks.iter()
            .map(|mb| mb.header.point_count)
            .sum();
        assert_eq!(total_points, 1000);
    }

    #[test]
    fn test_page_writer_miniblock_few_points() {
        let mut writer = PageWriter::new(
            TSDataType::Int32,
            TSEncoding::Simple8b,
            CompressionType::Lz4,
        );

        // Solo 100 puntos -> debería crear 1 mini-block
        for i in 0..100 {
            writer.write_i32(1000 + i, i as i32).unwrap();
        }

        let page_data = writer.finish().unwrap();
        assert_eq!(page_data.miniblocks.len(), 1);
        assert_eq!(page_data.miniblocks[0].header.point_count, 100);
    }
}
