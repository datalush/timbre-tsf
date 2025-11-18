use crate::common::statistic::{create_statistic, Statistic};
use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::error::{Result, TsFileError};
use crate::file::{ChunkHeader, PageData};
use crate::writer::PageWriter;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

/// Writer para chunks (colección de páginas para una medición)
pub struct ChunkWriter {
    measurement_name: String,
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,

    page_writer: PageWriter,
    pages: Vec<PageData>,

    chunk_statistic: Box<dyn Statistic>,
    max_page_size: usize,
    current_page_size: usize,
}

impl ChunkWriter {
    /// Tamaño máximo por defecto de una página (64KB)
    pub const DEFAULT_MAX_PAGE_SIZE: usize = 64 * 1024;

    pub fn new(
        measurement_name: String,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        let page_writer = PageWriter::new(data_type, encoding, compression_type);
        let chunk_statistic = create_statistic(data_type);

        Self {
            measurement_name,
            data_type,
            encoding,
            compression_type,
            page_writer,
            pages: Vec::new(),
            chunk_statistic,
            max_page_size: Self::DEFAULT_MAX_PAGE_SIZE,
            current_page_size: 0,
        }
    }

    /// Crea un ChunkWriter con tamaño de página personalizado
    pub fn with_page_size(
        measurement_name: String,
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
    pub fn write_bool(&mut self, timestamp: i64, value: bool) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_bool(timestamp, value)?;
        self.chunk_statistic.update_bool(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor i32
    pub fn write_i32(&mut self, timestamp: i64, value: i32) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_i32(timestamp, value)?;
        self.chunk_statistic.update_i32(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor i64
    pub fn write_i64(&mut self, timestamp: i64, value: i64) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_i64(timestamp, value)?;
        self.chunk_statistic.update_i64(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor f32
    pub fn write_f32(&mut self, timestamp: i64, value: f32) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_f32(timestamp, value)?;
        self.chunk_statistic.update_f32(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor f64
    pub fn write_f64(&mut self, timestamp: i64, value: f64) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_f64(timestamp, value)?;
        self.chunk_statistic.update_f64(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
        Ok(())
    }

    /// Escribe un valor string
    pub fn write_string(&mut self, timestamp: i64, value: &str) -> Result<()> {
        self.check_page_size_and_flush()?;
        self.page_writer.write_string(timestamp, value)?;
        self.chunk_statistic.update_string(timestamp, value);
        self.current_page_size = self.page_writer.estimated_size();
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
            return Err(TsFileError::InvalidState(
                "No pages to write".to_string(),
            ));
        }

        let mut total_bytes = 0;

        // Crear y escribir chunk header
        let mut header = ChunkHeader::new(
            self.measurement_name.clone(),
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

        // Escribir todas las páginas
        for page in &self.pages {
            // Escribir page header
            page.header.serialize(writer)?;
            total_bytes += crate::file::PageHeader::SERIALIZED_SIZE;

            // Escribir datos comprimidos
            writer.write_all(&page.compressed_data)?;
            total_bytes += page.compressed_data.len();
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

        // Páginas selladas
        for page in &self.pages {
            size += crate::file::PageHeader::SERIALIZED_SIZE;
            size += page.compressed_data.len();
        }

        // Página actual
        size += self.current_page_size;

        size
    }

    /// Estadísticas del chunk
    pub fn statistic(&self) -> &dyn Statistic {
        self.chunk_statistic.as_ref()
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
