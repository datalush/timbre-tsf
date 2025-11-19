use crate::common::statistic::{Statistic, create_statistic};
use crate::common::tablet::BitMap;
use crate::common::{CompressionType, TSDataType, TSEncoding, TsValue};
use crate::compress::{Compressor, CompressorImpl, create_compressor};
use crate::encoding::{Encoder, create_encoder_boxed};
use crate::error::{Result, TsFileError};
use crate::file::{ChunkHeader, ChunkType, PageData, PageHeader};
use std::io::Write;

/// Writer for a single value column in an aligned chunk
struct ValueColumnWriter {
    measurement_name: String,
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,
    encoder: Box<dyn Encoder>,
    compressor: CompressorImpl,
    value_buffer: Vec<u8>,
    bitmap: BitMap,
    statistic: Box<dyn Statistic>,
    max_size: usize,
    value_count: i32,
}

impl ValueColumnWriter {
    fn new(
        measurement_name: String,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
        max_size: usize,
    ) -> Self {
        let encoder = create_encoder_boxed(encoding, data_type);
        let compressor = create_compressor(compression_type);
        let statistic = create_statistic(data_type);
        let bitmap = BitMap::new(max_size);

        Self {
            measurement_name,
            data_type,
            encoding,
            compression_type,
            encoder,
            compressor,
            value_buffer: Vec::new(),
            bitmap,
            statistic,
            max_size,
            value_count: 0,
        }
    }

    fn write_value(&mut self, timestamp: i64, value: Option<&TsValue>) -> Result<()> {
        let idx = self.value_count as usize;

        match value {
            Some(val) => {
                self.bitmap.set(idx, false);
                match (self.data_type, val) {
                    (TSDataType::Boolean, TsValue::Boolean(v)) => {
                        self.encoder.encode_bool(*v, &mut self.value_buffer)?;
                        self.statistic.update_bool(timestamp, *v);
                    }
                    (TSDataType::Int32, TsValue::Int32(v))
                    | (TSDataType::Date, TsValue::Int32(v)) => {
                        self.encoder.encode_i32(*v, &mut self.value_buffer)?;
                        self.statistic.update_i32(timestamp, *v);
                    }
                    (TSDataType::Int64, TsValue::Int64(v))
                    | (TSDataType::Timestamp, TsValue::Int64(v)) => {
                        self.encoder.encode_i64(*v, &mut self.value_buffer)?;
                        self.statistic.update_i64(timestamp, *v);
                    }
                    (TSDataType::Float, TsValue::Float(v)) => {
                        self.encoder.encode_f32(*v, &mut self.value_buffer)?;
                        self.statistic.update_f32(timestamp, *v);
                    }
                    (TSDataType::Double, TsValue::Double(v)) => {
                        self.encoder.encode_f64(*v, &mut self.value_buffer)?;
                        self.statistic.update_f64(timestamp, *v);
                    }
                    (TSDataType::Text, TsValue::Text(v))
                    | (TSDataType::Text, TsValue::String(v))
                    | (TSDataType::String, TsValue::Text(v))
                    | (TSDataType::String, TsValue::String(v)) => {
                        self.encoder.encode_string(v, &mut self.value_buffer)?;
                        self.statistic.update_string(timestamp, v);
                    }
                    _ => {
                        return Err(TsFileError::TypeMismatch {
                            expected: self.data_type.to_string(),
                            actual: val.data_type().to_string(),
                        });
                    }
                }
            }
            None => {
                // Write null marker
                self.bitmap.set(idx, true);
                // Still need to write a placeholder value for alignment
                match self.data_type {
                    TSDataType::Boolean => {
                        self.encoder.encode_bool(false, &mut self.value_buffer)?
                    }
                    TSDataType::Int32 | TSDataType::Date => {
                        self.encoder.encode_i32(0, &mut self.value_buffer)?
                    }
                    TSDataType::Int64 | TSDataType::Timestamp => {
                        self.encoder.encode_i64(0, &mut self.value_buffer)?
                    }
                    TSDataType::Float => self.encoder.encode_f32(0.0, &mut self.value_buffer)?,
                    TSDataType::Double => self.encoder.encode_f64(0.0, &mut self.value_buffer)?,
                    TSDataType::Text | TSDataType::String => {
                        self.encoder.encode_string("", &mut self.value_buffer)?
                    }
                    _ => {
                        return Err(TsFileError::InvalidState(format!(
                            "Unsupported data type: {:?}",
                            self.data_type
                        )));
                    }
                }
            }
        }

        self.value_count += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<PageData> {
        if self.value_count == 0 {
            return Err(TsFileError::InvalidState(
                "Cannot finish empty value column".to_string(),
            ));
        }

        // Flush encoder
        self.encoder.flush(&mut self.value_buffer)?;

        // Create uncompressed data (just the value buffer for aligned chunks)
        let uncompressed_data = self.value_buffer.clone();
        let uncompressed_size = uncompressed_data.len() as u32;

        // Compress
        let compressed_data = self.compressor.compress(&uncompressed_data)?;
        let compressed_size = compressed_data.len() as u32;

        // Create header
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

    fn reset(&mut self) {
        self.value_buffer.clear();
        self.value_count = 0;
        self.bitmap = BitMap::new(self.max_size);
        self.statistic = create_statistic(self.data_type);
        self.encoder = create_encoder_boxed(self.encoding, self.data_type);
    }
}

/// Writer for aligned chunks (shared timestamp column across measurements)
pub struct AlignedChunkWriter {
    device_id: String,
    schemas: Vec<(String, TSDataType, TSEncoding, CompressionType)>,

    // Time column
    time_encoder: Box<dyn Encoder>,
    time_compressor: CompressorImpl,
    time_buffer: Vec<u8>,

    // Value columns (one per measurement)
    value_writers: Vec<ValueColumnWriter>,

    // Statistics
    time_statistic: Box<dyn Statistic>,

    // Configuration
    max_page_size: usize,
    current_page_size: usize,
    value_count: i32,

    // Pages for serialization
    time_pages: Vec<PageData>,
    value_pages: Vec<Vec<PageData>>, // One vec per measurement
}

impl AlignedChunkWriter {
    /// Default maximum page size (64KB)
    pub const DEFAULT_MAX_PAGE_SIZE: usize = 64 * 1024;

    /// Creates a new aligned chunk writer
    pub fn new(
        device_id: String,
        schemas: Vec<(String, TSDataType, TSEncoding, CompressionType)>,
    ) -> Self {
        Self::with_page_size(device_id, schemas, Self::DEFAULT_MAX_PAGE_SIZE)
    }

    /// Creates a new aligned chunk writer with custom page size
    pub fn with_page_size(
        device_id: String,
        schemas: Vec<(String, TSDataType, TSEncoding, CompressionType)>,
        max_page_size: usize,
    ) -> Self {
        let time_encoder = create_encoder_boxed(TSEncoding::Ts2Diff, TSDataType::Int64);
        let time_compressor = create_compressor(CompressionType::Uncompressed);
        let time_statistic = create_statistic(TSDataType::Int64);

        let value_writers: Vec<_> = schemas
            .iter()
            .map(|(name, data_type, encoding, compression)| {
                ValueColumnWriter::new(
                    name.clone(),
                    *data_type,
                    *encoding,
                    *compression,
                    max_page_size,
                )
            })
            .collect();

        let value_pages = (0..schemas.len()).map(|_| Vec::new()).collect();

        Self {
            device_id,
            schemas,
            time_encoder,
            time_compressor,
            time_buffer: Vec::new(),
            value_writers,
            time_statistic,
            max_page_size,
            current_page_size: 0,
            value_count: 0,
            time_pages: Vec::new(),
            value_pages,
        }
    }

    /// Writes a row with all measurement values at the given timestamp
    pub fn write_row(&mut self, timestamp: i64, values: Vec<Option<TsValue>>) -> Result<()> {
        if values.len() != self.value_writers.len() {
            return Err(TsFileError::InvalidState(format!(
                "Expected {} values, got {}",
                self.value_writers.len(),
                values.len()
            )));
        }

        // Check if we need to flush the current page
        self.check_page_size_and_flush()?;

        // Write timestamp to time column
        self.time_encoder
            .encode_i64(timestamp, &mut self.time_buffer)?;
        self.time_statistic.update_i64(timestamp, timestamp);

        // Write values to each column
        for (idx, value) in values.into_iter().enumerate() {
            self.value_writers[idx].write_value(timestamp, value.as_ref())?;
        }

        self.value_count += 1;
        self.current_page_size = self.time_buffer.len()
            + self
                .value_writers
                .iter()
                .map(|w| w.value_buffer.len())
                .sum::<usize>();

        Ok(())
    }

    fn check_page_size_and_flush(&mut self) -> Result<()> {
        if self.current_page_size >= self.max_page_size {
            self.seal_current_page()?;
        }
        Ok(())
    }

    fn seal_current_page(&mut self) -> Result<()> {
        if self.value_count == 0 {
            return Ok(());
        }

        // Flush time column
        let time_page = self.flush_time_column()?;
        self.time_pages.push(time_page);

        // Flush all value columns
        for (idx, writer) in self.value_writers.iter_mut().enumerate() {
            let value_page = writer.finish()?;
            self.value_pages[idx].push(value_page);
            writer.reset();
        }

        // Reset for next page
        self.time_buffer.clear();
        self.value_count = 0;
        self.current_page_size = 0;
        self.time_statistic = create_statistic(TSDataType::Int64);
        self.time_encoder = create_encoder_boxed(TSEncoding::Ts2Diff, TSDataType::Int64);

        Ok(())
    }

    fn flush_time_column(&mut self) -> Result<PageData> {
        if self.value_count == 0 {
            return Err(TsFileError::InvalidState(
                "Cannot flush empty time column".to_string(),
            ));
        }

        // Flush encoder
        self.time_encoder.flush(&mut self.time_buffer)?;

        let uncompressed_data = self.time_buffer.clone();
        let uncompressed_size = uncompressed_data.len() as u32;

        // Compress time column
        let compressed_data = self.time_compressor.compress(&uncompressed_data)?;
        let compressed_size = compressed_data.len() as u32;

        // Create header
        let mut header = PageHeader::new();
        header.uncompressed_size = uncompressed_size;
        header.compressed_size = compressed_size;
        header.num_of_values = self.value_count;
        header.min_timestamp = self.time_statistic.start_time();
        header.max_timestamp = self.time_statistic.end_time();

        Ok(PageData {
            uncompressed_data,
            compressed_data,
            header,
        })
    }

    /// Serializes the aligned chunk group to a writer
    pub fn serialize_to<W: Write>(&mut self, writer: &mut W) -> Result<usize> {
        // Seal current page if there's data
        self.seal_current_page()?;

        if self.time_pages.is_empty() {
            return Err(TsFileError::InvalidState("No pages to write".to_string()));
        }

        let mut total_bytes = 0;

        // Write time chunk first
        total_bytes += self.serialize_time_chunk(writer)?;

        // Write value chunks
        for idx in 0..self.value_writers.len() {
            total_bytes += self.serialize_value_chunk(writer, idx)?;
        }

        Ok(total_bytes)
    }

    fn serialize_time_chunk<W: Write>(&self, writer: &mut W) -> Result<usize> {
        let mut total_bytes = 0;

        // Create chunk header for time column
        let mut header = ChunkHeader::new(
            "TIME".to_string(),
            TSDataType::Int64,
            CompressionType::Uncompressed,
            TSEncoding::Ts2Diff,
        );
        header.chunk_type = ChunkType::Aligned;
        header.num_of_pages = self.time_pages.len() as i32;

        // Calculate data size
        let mut data_size = 0u32;
        for page in &self.time_pages {
            data_size += PageHeader::SERIALIZED_SIZE as u32;
            data_size += page.header.compressed_size;
        }
        header.data_size = data_size;

        // Write header
        header.serialize(writer)?;
        total_bytes += header.serialized_size();

        // Write pages
        for page in &self.time_pages {
            page.header.serialize(writer)?;
            total_bytes += PageHeader::SERIALIZED_SIZE;

            writer.write_all(&page.compressed_data)?;
            total_bytes += page.compressed_data.len();
        }

        Ok(total_bytes)
    }

    fn serialize_value_chunk<W: Write>(&self, writer: &mut W, column_idx: usize) -> Result<usize> {
        let mut total_bytes = 0;
        let pages = &self.value_pages[column_idx];

        if pages.is_empty() {
            return Err(TsFileError::InvalidState(format!(
                "No pages for column {}",
                column_idx
            )));
        }

        let (name, data_type, encoding, compression) = &self.schemas[column_idx];

        // Create chunk header
        let mut header = ChunkHeader::new(name.clone(), *data_type, *compression, *encoding);
        header.chunk_type = ChunkType::Aligned;
        header.num_of_pages = pages.len() as i32;

        // Calculate data size
        let mut data_size = 0u32;
        for page in pages {
            data_size += PageHeader::SERIALIZED_SIZE as u32;
            data_size += page.header.compressed_size;
        }
        header.data_size = data_size;

        // Write header
        header.serialize(writer)?;
        total_bytes += header.serialized_size();

        // Write pages
        for page in pages {
            page.header.serialize(writer)?;
            total_bytes += PageHeader::SERIALIZED_SIZE;

            writer.write_all(&page.compressed_data)?;
            total_bytes += page.compressed_data.len();
        }

        Ok(total_bytes)
    }

    /// Returns the device ID
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    /// Returns the number of measurements
    pub fn measurement_count(&self) -> usize {
        self.value_writers.len()
    }

    /// Returns the total number of values written
    pub fn total_value_count(&self) -> i32 {
        let sealed_values: i32 = self.time_pages.len() as i32; // Each page represents multiple values
        sealed_values + self.value_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aligned_chunk_writer_basic() {
        let schemas = vec![
            (
                "temperature".to_string(),
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
            (
                "humidity".to_string(),
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
        ];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        // Write some rows
        writer
            .write_row(
                1000,
                vec![Some(TsValue::Float(25.5)), Some(TsValue::Int32(60))],
            )
            .unwrap();

        writer
            .write_row(
                2000,
                vec![Some(TsValue::Float(26.0)), Some(TsValue::Int32(65))],
            )
            .unwrap();

        writer
            .write_row(
                3000,
                vec![Some(TsValue::Float(26.5)), Some(TsValue::Int32(70))],
            )
            .unwrap();

        // Serialize
        let mut buffer = Vec::new();
        let bytes_written = writer.serialize_to(&mut buffer).unwrap();

        assert!(bytes_written > 0);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_aligned_chunk_writer_with_nulls() {
        let schemas = vec![
            (
                "temp".to_string(),
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
            (
                "humidity".to_string(),
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
        ];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        // Write rows with nulls
        writer
            .write_row(1000, vec![Some(TsValue::Float(25.5)), None])
            .unwrap();

        writer
            .write_row(2000, vec![None, Some(TsValue::Int32(65))])
            .unwrap();

        let mut buffer = Vec::new();
        let bytes_written = writer.serialize_to(&mut buffer).unwrap();

        assert!(bytes_written > 0);
    }

    #[test]
    fn test_aligned_chunk_writer_multiple_pages() {
        let schemas = vec![(
            "sensor".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        )];

        let mut writer = AlignedChunkWriter::with_page_size(
            "device1".to_string(),
            schemas,
            100, // Very small page size to force multiple pages
        );

        // Write many rows
        for i in 0..50 {
            writer
                .write_row(1000 + i * 10, vec![Some(TsValue::Int32(i as i32))])
                .unwrap();
        }

        let mut buffer = Vec::new();
        let bytes_written = writer.serialize_to(&mut buffer).unwrap();

        assert!(bytes_written > 0);
        // Should have created multiple pages
        assert!(writer.time_pages.len() > 1);
    }

    #[test]
    fn test_aligned_chunk_writer_empty() {
        let schemas = vec![(
            "temp".to_string(),
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        )];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        let mut buffer = Vec::new();
        let result = writer.serialize_to(&mut buffer);

        // Should fail - cannot write empty chunk
        assert!(result.is_err());
    }

    #[test]
    fn test_aligned_chunk_writer_wrong_value_count() {
        let schemas = vec![
            (
                "temp".to_string(),
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
            (
                "humidity".to_string(),
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            ),
        ];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        // Try to write row with wrong number of values
        let result = writer.write_row(
            1000,
            vec![
                Some(TsValue::Float(25.5)),
                // Missing second value
            ],
        );

        assert!(result.is_err());
    }
}
