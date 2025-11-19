use crate::common::statistic::{Statistic, create_statistic};
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::compress::{Compressor, create_compressor};
use crate::encoding::{Encoder, create_encoder};
use crate::error::Result;
use crate::file::{PageData, PageHeader};

use crate::encoding::EncoderImpl;

/// Writer para páginas individuales
/// Responsable de encoding y compresión de datos
pub struct PageWriter {
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,

    // OPT-2: EncoderImpl en lugar de Box<dyn Encoder> para static dispatch
    time_encoder: EncoderImpl,
    value_encoder: EncoderImpl,
    compressor: Box<dyn Compressor>,

    time_buffer: Vec<u8>,
    value_buffer: Vec<u8>,

    statistic: Box<dyn Statistic>,
    value_count: i32,

    /// Cached size to avoid recalculating on every call (optimization)
    cached_size: usize,
    /// Flag to indicate if cached_size needs recalculation
    size_dirty: bool,
}

impl PageWriter {
    pub fn new(
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        // Encoders separados para timestamps y valores
        let time_encoder = create_encoder(TSEncoding::Ts2Diff, TSDataType::Int64);
        let value_encoder = create_encoder(encoding, data_type);
        let compressor = create_compressor(compression_type);
        let statistic = create_statistic(data_type);

        Self {
            data_type,
            encoding,
            compression_type,
            time_encoder,
            value_encoder,
            compressor,
            time_buffer: Vec::new(),
            value_buffer: Vec::new(),
            statistic,
            value_count: 0,
            cached_size: 0,
            size_dirty: true,
        }
    }

    /// Escribe un valor booleano
    pub fn write_bool(&mut self, timestamp: i64, value: bool) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_bool(value, &mut self.value_buffer)?;
        self.statistic.update_bool(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true; // Mark size as needing recalculation
        Ok(())
    }

    /// Escribe un valor i32
    pub fn write_i32(&mut self, timestamp: i64, value: i32) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_i32(value, &mut self.value_buffer)?;
        self.statistic.update_i32(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true;
        Ok(())
    }

    /// Escribe un valor i64
    pub fn write_i64(&mut self, timestamp: i64, value: i64) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_i64(value, &mut self.value_buffer)?;
        self.statistic.update_i64(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true;
        Ok(())
    }

    /// Escribe un valor f32
    pub fn write_f32(&mut self, timestamp: i64, value: f32) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_f32(value, &mut self.value_buffer)?;
        self.statistic.update_f32(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true;
        Ok(())
    }

    /// Escribe un valor f64
    pub fn write_f64(&mut self, timestamp: i64, value: f64) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_f64(value, &mut self.value_buffer)?;
        self.statistic.update_f64(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true;
        Ok(())
    }

    /// Escribe un valor string
    pub fn write_string(&mut self, timestamp: i64, value: &str) -> Result<()> {
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.value_encoder
            .encode_string(value, &mut self.value_buffer)?;
        self.statistic.update_string(timestamp, value);
        self.value_count += 1;
        self.size_dirty = true;
        Ok(())
    }

    /// Finaliza la escritura y genera PageData
    pub fn finish(&mut self) -> Result<PageData> {
        // Flush encoders
        self.time_encoder.flush(&mut self.time_buffer)?;
        self.value_encoder.flush(&mut self.value_buffer)?;

        // Crear buffer no comprimido (time + value)
        let mut uncompressed_data = Vec::new();

        // Escribir tamaños de time y value
        use byteorder::{LittleEndian, WriteBytesExt};
        uncompressed_data.write_u32::<LittleEndian>(self.time_buffer.len() as u32)?;
        uncompressed_data.extend_from_slice(&self.time_buffer);
        uncompressed_data.write_u32::<LittleEndian>(self.value_buffer.len() as u32)?;
        uncompressed_data.extend_from_slice(&self.value_buffer);

        let uncompressed_size = uncompressed_data.len() as u32;

        // Comprimir
        let compressed_data = self.compressor.compress(&uncompressed_data)?;
        let compressed_size = compressed_data.len() as u32;

        // Crear header
        let mut header = PageHeader::new();
        header.uncompressed_size = uncompressed_size;
        header.compressed_size = compressed_size;
        header.num_of_values = self.value_count;
        header.min_timestamp = self.statistic.start_time();
        header.max_timestamp = self.statistic.end_time();

        Ok(PageData {
            uncompressed_data,
            compressed_data,
            header,
        })
    }

    /// Retorna el número de valores escritos
    pub fn value_count(&self) -> i32 {
        self.value_count
    }

    /// Retorna las estadísticas actuales
    pub fn statistic(&self) -> &dyn Statistic {
        self.statistic.as_ref()
    }

    /// Resetea el writer para reutilización
    pub fn reset(&mut self) {
        self.time_buffer.clear();
        self.value_buffer.clear();
        self.value_count = 0;
        self.statistic = create_statistic(self.data_type);
        self.cached_size = 0;
        self.size_dirty = true; // Mark for recalculation

        // Recrear encoders
        self.time_encoder = create_encoder(TSEncoding::Ts2Diff, TSDataType::Int64);
        self.value_encoder = create_encoder(self.encoding, self.data_type);
    }

    /// Tamaño actual de los buffers (sin comprimir)
    /// Incluye datos en buffers explícitos Y datos buffereados internamente por encoders
    /// Optimización: Solo recalcula cuando size_dirty==true (lazy evaluation)
    #[inline]
    pub fn estimated_size(&mut self) -> usize {
        if self.size_dirty {
            self.cached_size = self.time_buffer.len()
                + self.value_buffer.len()
                + self.time_encoder.buffered_size()
                + self.value_encoder.buffered_size();
            self.size_dirty = false;
        }
        self.cached_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_writer_i32() {
        let mut writer = PageWriter::new(
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        writer.write_i32(1000, 10).unwrap();
        writer.write_i32(2000, 20).unwrap();
        writer.write_i32(3000, 30).unwrap();

        assert_eq!(writer.value_count(), 3);

        let page_data = writer.finish().unwrap();
        assert_eq!(page_data.header.num_of_values, 3);
        assert_eq!(page_data.header.min_timestamp, 1000);
        assert_eq!(page_data.header.max_timestamp, 3000);
    }

    #[test]
    fn test_page_writer_float() {
        let mut writer =
            PageWriter::new(TSDataType::Float, TSEncoding::Plain, CompressionType::Lz4);

        writer.write_f32(1000, 1.5).unwrap();
        writer.write_f32(2000, 2.5).unwrap();
        writer.write_f32(3000, 3.5).unwrap();

        assert_eq!(writer.value_count(), 3);

        let page_data = writer.finish().unwrap();
        assert_eq!(page_data.header.num_of_values, 3);

        // Con compresión, el tamaño comprimido debería ser <= sin comprimir
        assert!(page_data.header.compressed_size <= page_data.header.uncompressed_size);
    }

    #[test]
    fn test_page_writer_reset() {
        let mut writer = PageWriter::new(
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        writer.write_i32(1000, 10).unwrap();
        writer.write_i32(2000, 20).unwrap();
        assert_eq!(writer.value_count(), 2);

        writer.reset();
        assert_eq!(writer.value_count(), 0);

        writer.write_i32(3000, 30).unwrap();
        assert_eq!(writer.value_count(), 1);
    }

    #[test]
    fn test_page_writer_string() {
        let mut writer = PageWriter::new(
            TSDataType::Text,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        writer.write_string(1000, "Hello").unwrap();
        writer.write_string(2000, "World").unwrap();

        assert_eq!(writer.value_count(), 2);

        let page_data = writer.finish().unwrap();
        assert_eq!(page_data.header.num_of_values, 2);
    }
}
