//! PageWriter with native support for mini-blocks (Timbre format)
//!
//! Timbre's core innovation:
//! - Accumulates RAW data (timestamps + unencoded values)
//! - On finish(), divides into 4-8 mini-blocks
//! - Each mini-block is encoded and compressed independently
//! - Enables parallel decoding (potential 8x speedup)

use crate::common::statistic::{Statistic, create_statistic};
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::compress::create_compressor;
use crate::encoding::{EncoderImpl, create_encoder};
use crate::error::{Result, TsFileError};
use crate::file::{MiniBlock, MiniBlockConfig, MiniBlockHeader, PageData, PageHeader};

/// Writer for pages with mini-blocks (Timbre format)
pub struct PageWriter {
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,
    pub miniblock_config: MiniBlockConfig,

    // Accumulated RAW data (unencoded)
    timestamps: Vec<i64>,
    // ValueData is an enum for different types
    value_data: ValueData,

    statistic: Box<dyn Statistic>,

    // OPT: Reusable encoders (avoids 16-32 allocations per page)
    time_encoder: EncoderImpl,
    value_encoder: EncoderImpl,
}

/// Accumulated value data by type
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
}

impl PageWriter {
    pub fn new(
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        // OPT: Pre-create encoders for reuse across mini-blocks
        let time_encoder = create_encoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
        let value_encoder = create_encoder(encoding, data_type);

        Self {
            data_type,
            encoding,
            compression_type,
            miniblock_config: MiniBlockConfig::default(),
            timestamps: Vec::new(),
            value_data: ValueData::new(data_type),
            statistic: create_statistic(data_type),
            time_encoder,
            value_encoder,
        }
    }

    /// Writes a boolean value
    pub fn write_bool(&mut self, timestamp: i64, value: bool) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Boolean(v) => v.push(value),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "Boolean".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
        }
        self.statistic.update_bool(timestamp, value);
        Ok(())
    }

    /// Writes an i32 value
    pub fn write_i32(&mut self, timestamp: i64, value: i32) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Int32(v) => v.push(value),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "Int32".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
        }
        self.statistic.update_i32(timestamp, value);
        Ok(())
    }

    /// Writes an i64 value
    pub fn write_i64(&mut self, timestamp: i64, value: i64) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Int64(v) => v.push(value),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "Int64".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
        }
        self.statistic.update_i64(timestamp, value);
        Ok(())
    }

    /// Writes an f32 value
    pub fn write_f32(&mut self, timestamp: i64, value: f32) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Float(v) => v.push(value),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "Float".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
        }
        self.statistic.update_f32(timestamp, value);
        Ok(())
    }

    /// Writes an f64 value
    pub fn write_f64(&mut self, timestamp: i64, value: f64) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::Double(v) => v.push(value),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "Double".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
        }
        self.statistic.update_f64(timestamp, value);
        Ok(())
    }

    /// Writes a string value
    pub fn write_string(&mut self, timestamp: i64, value: &str) -> Result<()> {
        self.timestamps.push(timestamp);
        match &mut self.value_data {
            ValueData::String(v) => v.push(value.to_string()),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "String".to_string(),
                    actual: format!("{:?}", self.data_type),
                });
            }
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
    fn create_miniblock(&mut self, start: usize, end: usize) -> Result<MiniBlock> {
        // Extraer timestamps del rango
        let timestamps = &self.timestamps[start..end];
        let point_count = timestamps.len() as u32;
        let min_timestamp = timestamps[0];
        let max_timestamp = timestamps[timestamps.len() - 1];

        // OPT: Reset reusable encoders instead of creating new ones
        self.time_encoder.reset();
        self.value_encoder.reset();

        // Encodear timestamps usando batch API (DeltaOfDelta encoding)
        let mut time_buffer = Vec::new();
        self.time_encoder.encode_i64_batch(timestamps, &mut time_buffer)?;
        self.time_encoder.flush(&mut time_buffer)?;

        // Encodear values según tipo
        // OPT-BATCH-API: Use batch encoding to eliminate function call overhead
        // This provides 30-40% improvement by:
        // - Single function call + match dispatch instead of N calls
        // - Better cache locality with sequential access
        // - Compiler optimizations enabled for tight loops
        let mut value_buffer = Vec::new();

        match &self.value_data {
            ValueData::Boolean(v) => {
                // Booleans: no batch API yet, use loop
                for &val in &v[start..end] {
                    self.value_encoder.encode_bool(val, &mut value_buffer)?;
                }
            }
            ValueData::Int32(v) => {
                // Batch encode i32 values
                self.value_encoder.encode_i32_batch(&v[start..end], &mut value_buffer)?;
            }
            ValueData::Int64(v) => {
                // Batch encode i64 values
                self.value_encoder.encode_i64_batch(&v[start..end], &mut value_buffer)?;
            }
            ValueData::Float(v) => {
                // Batch encode f32 values (HOT PATH for sensor data)
                self.value_encoder.encode_f32_batch(&v[start..end], &mut value_buffer)?;
            }
            ValueData::Double(v) => {
                // Batch encode f64 values (HOT PATH for high-precision sensors)
                self.value_encoder.encode_f64_batch(&v[start..end], &mut value_buffer)?;
            }
            ValueData::String(v) => {
                // Strings: no batch API yet, use loop
                for val in &v[start..end] {
                    self.value_encoder.encode_string(val, &mut value_buffer)?;
                }
            }
        }
        self.value_encoder.flush(&mut value_buffer)?;

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

    /// Returns the number of values written
    pub fn value_count(&self) -> i32 {
        self.timestamps.len() as i32
    }

    /// Returns current statistics
    pub fn statistic(&self) -> &dyn Statistic {
        self.statistic.as_ref()
    }

    /// Resetea el writer para reutilización
    pub fn reset(&mut self) {
        self.timestamps.clear();
        self.value_data = ValueData::new(self.data_type);
        self.statistic = create_statistic(self.data_type);
        // OPT: Reset encoders to reuse them
        self.time_encoder.reset();
        self.value_encoder.reset();
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
        let total_points: u32 = page_data
            .miniblocks
            .iter()
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
