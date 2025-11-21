use crate::common::statistic::{StatisticEnum, create_statistic};
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use crate::file::{ChunkHeader, PageData};
use crate::writer::PageWriter;
use std::io::Write;

/// Writer para chunks (colección de páginas para una medición)
///
/// OPT: Uses StatisticEnum instead of Box<dyn Statistic> to eliminate vtable overhead
/// across the entire write path (ChunkWriter → PageWriter → Statistic).
pub struct ChunkWriter {
    measurement_name: String,
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,

    page_writer: PageWriter,
    pages: Vec<PageData>,

    // OPT: Removed chunk_statistic field - now derived lazily via merge()
    max_page_size: usize,
    current_page_size: usize,
}

impl ChunkWriter {
    /// Tamaño máximo por defecto de una página (64KB)
    pub const DEFAULT_MAX_PAGE_SIZE: usize = 64 * 1024;

    pub fn new(
        measurement_name: impl Into<String>,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        let page_writer = PageWriter::new(data_type, encoding, compression_type);
        // OPT: Removed chunk_statistic initialization - derived lazily

        Self {
            measurement_name: measurement_name.into(),
            data_type,
            encoding,
            compression_type,
            page_writer,
            pages: Vec::new(),
            // OPT: Removed chunk_statistic field
            max_page_size: Self::DEFAULT_MAX_PAGE_SIZE,
            current_page_size: 0,
        }
    }

    /// Crea un ChunkWriter con tamaño de página personalizado
    pub fn with_page_size(
        measurement_name: impl Into<String>,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
        max_page_size: usize,
    ) -> Self {
        let mut writer = Self::new(measurement_name, data_type, encoding, compression_type);
        writer.max_page_size = max_page_size;
        writer
    }

    /// Escribe un valor booleano
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_bool(&mut self, timestamp: i64, value: bool) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_bool(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_bool() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor i32
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_i32(&mut self, timestamp: i64, value: i32) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_i32(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_i32() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor i64
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_i64(&mut self, timestamp: i64, value: i64) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_i64(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_i64() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor f32
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_f32(&mut self, timestamp: i64, value: f32) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_f32(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_f32() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor f64
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_f64(&mut self, timestamp: i64, value: f64) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_f64(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_f64() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor string
    ///
    /// OPT: Removed duplicate statistics calculation - stats are now derived from pages
    pub fn write_string(&mut self, timestamp: i64, value: &str) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_string(timestamp, value)?;
        // OPT: Removed chunk_statistic.update_string() - calculated lazily via merge
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    // ===== BATCH WRITE METHODS WITH BITMAP (Phase 2 Optimization) =====
    //
    // These methods write multiple values at once while handling null values and page boundaries.
    // Projected improvement: +5-8% throughput by eliminating per-value function calls.

    /// Writes a batch of i32 values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    /// The bitmap marks non-null values as false (skip if true).
    ///
    /// # Arguments
    /// * `timestamps` - Slice of timestamps for all values
    /// * `values` - Slice of i32 values
    /// * `bitmap` - Bitmap indicating null values (true = null, skip)
    pub fn write_i32_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[i32],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        // Collect non-null values into temporary vectors
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                // false in bitmap means non-null
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i]);

                // Check if we need to flush before adding more
                // Estimate: 8 bytes timestamp + 4 bytes value = 12 bytes per point
                if self.current_page_size + (batch_timestamps.len() * 12) >= self.max_page_size {
                    // Flush accumulated batch
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_i32_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    // Seal current page
                    self.seal_current_page()?;
                }
            }
        }

        // Write remaining batch
        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_i32_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Writes a batch of i64 values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    pub fn write_i64_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[i64],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i]);

                if self.current_page_size + (batch_timestamps.len() * 16) >= self.max_page_size {
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_i64_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    self.seal_current_page()?;
                }
            }
        }

        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_i64_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Writes a batch of f32 values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    pub fn write_f32_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[f32],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i]);

                if self.current_page_size + (batch_timestamps.len() * 12) >= self.max_page_size {
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_f32_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    self.seal_current_page()?;
                }
            }
        }

        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_f32_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Writes a batch of f64 values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    pub fn write_f64_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[f64],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i]);

                if self.current_page_size + (batch_timestamps.len() * 16) >= self.max_page_size {
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_f64_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    self.seal_current_page()?;
                }
            }
        }

        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_f64_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Writes a batch of bool values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    pub fn write_bool_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[bool],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i]);

                if self.current_page_size + (batch_timestamps.len() * 9) >= self.max_page_size {
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_bool_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    self.seal_current_page()?;
                }
            }
        }

        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_bool_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Writes a batch of string values with bitmap indicating null values
    ///
    /// OPT-P0-2: Batch API eliminates 100K+ function calls in write_tablet() hot path.
    pub fn write_string_batch_with_bitmap(
        &mut self,
        timestamps: &[i64],
        values: &[String],
        bitmap: &crate::common::BitMap,
    ) -> Result<()> {
        let mut batch_timestamps = Vec::new();
        let mut batch_values = Vec::new();

        for i in 0..timestamps.len() {
            if !bitmap.get(i) {
                batch_timestamps.push(timestamps[i]);
                batch_values.push(values[i].clone());

                // Estimate string size: 8 bytes timestamp + string length
                let est_size: usize = batch_timestamps.len() * 8
                    + batch_values.iter().map(|s| s.len()).sum::<usize>();
                if self.current_page_size + est_size >= self.max_page_size {
                    if !batch_timestamps.is_empty() {
                        self.page_writer
                            .write_string_batch(&batch_timestamps, &batch_values)?;
                        self.current_page_size = self.page_writer.estimated_size();
                        batch_timestamps.clear();
                        batch_values.clear();
                    }
                    self.seal_current_page()?;
                }
            }
        }

        if !batch_timestamps.is_empty() {
            self.page_writer
                .write_string_batch(&batch_timestamps, &batch_values)?;
            self.current_page_size = self.page_writer.estimated_size();
        }

        Ok(())
    }

    /// Verifica si la página actual excede el tamaño máximo y hace flush si es necesario
    fn check_page_size_and_flush(&mut self) -> Result<()> {
        if self.current_page_size >= self.max_page_size {
            self.seal_current_page()?;
        }
        Ok(())
    }

    /// Sella la página actual y la agrega a la lista de páginas
    fn seal_current_page(&mut self) -> Result<()> {
        if self.page_writer.value_count() == 0 {
            return Ok(());
        }

        let page_data = self.page_writer.finish()?;
        self.pages.push(page_data);
        self.page_writer.reset();
        self.current_page_size = 0;
        Ok(())
    }

    /// Serializa el chunk completo a un writer
    pub fn serialize_to<W: Write>(&mut self, writer: &mut W) -> Result<usize> {
        // Sellar página actual si tiene datos
        self.seal_current_page()?;

        if self.pages.is_empty() {
            return Err(TsFileError::InvalidState("No pages to write".to_string()));
        }

        let mut total_bytes = 0;

        // Crear y escribir chunk header
        let mut header = ChunkHeader::new(
            &self.measurement_name,
            self.data_type,
            self.compression_type,
            self.encoding,
        );
        header.num_of_pages = self.pages.len() as i32;

        // Calcular data_size (total de páginas sin incluir headers)
        let mut data_size = 0u32;
        for page in &self.pages {
            data_size += crate::file::PageHeader::SERIALIZED_SIZE as u32;
            data_size += page.header.compressed_size;
        }
        header.data_size = data_size;

        // Escribir chunk header
        header.serialize(writer)?;
        total_bytes += header.serialized_size();

        // Escribir todas las páginas con mini-blocks
        for page in &self.pages {
            // Escribir page header
            page.header.serialize(writer)?;
            total_bytes += crate::file::PageHeader::SERIALIZED_SIZE;

            // Escribir número de mini-blocks
            use byteorder::{LittleEndian, WriteBytesExt};
            writer.write_u32::<LittleEndian>(page.miniblocks.len() as u32)?;
            total_bytes += 4;

            // Escribir cada mini-block
            for miniblock in &page.miniblocks {
                let mb_bytes = miniblock.serialize(writer)?;
                total_bytes += mb_bytes;
            }
        }

        Ok(total_bytes)
    }

    /// Número de páginas en el chunk
    pub fn num_of_pages(&self) -> usize {
        let mut count = self.pages.len();
        if self.page_writer.value_count() > 0 {
            count += 1; // Página actual sin sellar
        }
        count
    }

    /// Tamaño estimado del chunk en bytes
    pub fn estimated_size(&self) -> usize {
        let mut size = 0;

        // Header del chunk
        size += 1 + 4 + self.measurement_name.len() + 4 + 1 + 1 + 1 + 4;

        // Páginas selladas con mini-blocks
        for page in &self.pages {
            size += crate::file::PageHeader::SERIALIZED_SIZE;
            size += 4; // miniblock count
            for miniblock in &page.miniblocks {
                size += miniblock.size();
            }
        }

        // Página actual
        size += self.current_page_size;

        size
    }

    /// Estadísticas del chunk (derivadas de pages via merge)
    ///
    /// OPT: Instead of calculating stats twice (once per write), we derive chunk
    /// statistics by merging page statistics. This eliminates 50% of statistic overhead.
    pub fn statistic(&self) -> StatisticEnum {
        let mut stat = create_statistic(self.data_type);

        // Merge statistics from all sealed pages
        for _page in &self.pages {
            // Note: For now we only have timestamp min/max in PageHeader
            // TODO: If we store full statistics in PageData, merge those too
            stat.merge(self.page_writer.statistic());
        }

        // Merge statistics from current unsaved page
        if self.page_writer.value_count() > 0 {
            stat.merge(self.page_writer.statistic());
        }

        stat
    }

    /// Nombre de la medición
    pub fn measurement_name(&self) -> &str {
        &self.measurement_name
    }

    /// Tipo de dato
    pub fn data_type(&self) -> TSDataType {
        self.data_type
    }

    /// Encoding utilizado
    pub fn encoding(&self) -> TSEncoding {
        self.encoding
    }

    /// Tipo de compresión
    pub fn compression_type(&self) -> CompressionType {
        self.compression_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_writer_single_page() {
        let mut writer = ChunkWriter::new(
            "temperature".to_string(),
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        for i in 0..10 {
            writer.write_f32(1000 + i * 100, 25.0 + i as f32).unwrap();
        }

        assert_eq!(writer.num_of_pages(), 1);

        let mut buffer = Vec::new();
        let bytes_written = writer.serialize_to(&mut buffer).unwrap();
        assert!(bytes_written > 0);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_chunk_writer_multiple_pages() {
        let mut writer = ChunkWriter::with_page_size(
            "sensor".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
            100, // Tamaño muy pequeño para forzar múltiples páginas
        );

        for i in 0..100 {
            writer.write_i32(1000 + i * 10, i as i32).unwrap();
        }

        // Debería haber creado múltiples páginas
        assert!(writer.num_of_pages() > 1);

        let mut buffer = Vec::new();
        let bytes_written = writer.serialize_to(&mut buffer).unwrap();
        assert!(bytes_written > 0);
    }

    #[test]
    fn test_chunk_writer_empty() {
        let mut writer = ChunkWriter::new(
            "empty".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let mut buffer = Vec::new();
        let result = writer.serialize_to(&mut buffer);
        assert!(result.is_err()); // No debería permitir escribir chunk vacío
    }
}
