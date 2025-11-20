use crate::common::tablet::BitMap;
use crate::common::TSDataType;
#[cfg(test)]
use crate::common::{CompressionType, TSEncoding};
use crate::compress::create_compressor;
use crate::encoding::{DecoderImpl, create_decoder};
use crate::error::{Result, TsFileError};
use crate::file::{ChunkHeader, ChunkType, PageHeader};
use crate::reader::{DecodedValue, DecodedValues};
use std::collections::HashMap;
use std::io::Read;

/// Decoded column data for aligned chunks
#[derive(Debug, Clone)]
pub struct DecodedColumn {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub values: DecodedValues,
    pub bitmap: Option<BitMap>,
}

impl DecodedColumn {
    /// Get value at specific index
    pub fn get(&self, index: usize) -> Option<DecodedValue> {
        // Check if null
        if let Some(ref bitmap) = self.bitmap {
            if bitmap.get(index) {
                return None; // Value is null
            }
        }

        match &self.values {
            DecodedValues::Boolean(v) => v.get(index).map(|&val| DecodedValue::Boolean(val)),
            DecodedValues::Int32(v) => v.get(index).map(|&val| DecodedValue::Int32(val)),
            DecodedValues::Int64(v) => v.get(index).map(|&val| DecodedValue::Int64(val)),
            DecodedValues::Float(v) => v.get(index).map(|&val| DecodedValue::Float(val)),
            DecodedValues::Double(v) => v.get(index).map(|&val| DecodedValue::Double(val)),
            DecodedValues::Text(v) => v.get(index).map(|val| DecodedValue::Text(val.clone())),
        }
    }

    /// Get the number of values
    pub fn len(&self) -> usize {
        match &self.values {
            DecodedValues::Boolean(v) => v.len(),
            DecodedValues::Int32(v) => v.len(),
            DecodedValues::Int64(v) => v.len(),
            DecodedValues::Float(v) => v.len(),
            DecodedValues::Double(v) => v.len(),
            DecodedValues::Text(v) => v.len(),
        }
    }

    /// Check if column is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Reader for aligned chunks (shared timestamp column)
pub struct AlignedChunkReader {
    device_id: String,
    timestamps: Vec<i64>,
    columns: HashMap<String, DecodedColumn>,
}

impl AlignedChunkReader {
    /// Creates a new aligned chunk reader
    pub fn new(device_id: String) -> Self {
        Self {
            device_id,
            timestamps: Vec::new(),
            columns: HashMap::new(),
        }
    }

    /// Reads an aligned chunk group from a reader
    /// This reads the time chunk first, then all value chunks
    pub fn read_aligned_chunk<R: Read>(
        &mut self,
        reader: &mut R,
        time_header: ChunkHeader,
    ) -> Result<()> {
        // Verify this is an aligned chunk
        if time_header.chunk_type != ChunkType::Aligned {
            return Err(TsFileError::InvalidState(
                "Expected aligned chunk type".to_string(),
            ));
        }

        // Read time chunk
        self.timestamps = self.read_time_chunk(reader, &time_header)?;

        Ok(())
    }

    /// Reads a value column chunk
    pub fn read_value_chunk<R: Read>(&mut self, reader: &mut R, header: ChunkHeader) -> Result<()> {
        if header.chunk_type != ChunkType::Aligned {
            return Err(TsFileError::InvalidState(
                "Expected aligned chunk type".to_string(),
            ));
        }

        let column = self.read_value_column_data(reader, &header)?;

        // Verify column length matches timestamps
        if column.len() != self.timestamps.len() {
            return Err(TsFileError::InvalidState(format!(
                "Value column length {} does not match timestamp length {}",
                column.len(),
                self.timestamps.len()
            )));
        }

        self.columns.insert(header.measurement_name.clone(), column);
        Ok(())
    }

    fn read_time_chunk<R: Read>(&self, reader: &mut R, header: &ChunkHeader) -> Result<Vec<i64>> {
        let mut all_timestamps = Vec::new();

        // Read all pages in the time chunk
        for _ in 0..header.num_of_pages {
            let page_header = PageHeader::deserialize(reader)?;
            let mut compressed_data = vec![0u8; page_header.compressed_size as usize];
            reader.read_exact(&mut compressed_data)?;

            // Decompress
            let mut compressor = create_compressor(header.compression_type);
            let uncompressed =
                compressor.decompress(&compressed_data, page_header.uncompressed_size as usize)?;

            // Decode timestamps (time column doesn't have time+value structure, just timestamps)
            let mut decoder = create_decoder(header.encoding_type, TSDataType::Int64);
            let mut pos = 0;
            let mut timestamps = Vec::with_capacity(page_header.num_of_values as usize);

            while decoder.has_remaining(&uncompressed, pos)
                && timestamps.len() < page_header.num_of_values as usize
            {
                let ts = decoder.read_i64(&uncompressed, &mut pos)?;
                timestamps.push(ts);
            }

            all_timestamps.extend(timestamps);
        }

        Ok(all_timestamps)
    }

    fn read_value_column_data<R: Read>(
        &self,
        reader: &mut R,
        header: &ChunkHeader,
    ) -> Result<DecodedColumn> {
        let mut all_values: Option<DecodedValues> = None;
        let bitmap = BitMap::new(self.timestamps.len());

        // Read all pages in the value chunk
        for _ in 0..header.num_of_pages {
            let page_header = PageHeader::deserialize(reader)?;
            let mut compressed_data = vec![0u8; page_header.compressed_size as usize];
            reader.read_exact(&mut compressed_data)?;

            // Decompress
            let mut compressor = create_compressor(header.compression_type);
            let uncompressed =
                compressor.decompress(&compressed_data, page_header.uncompressed_size as usize)?;

            // Decode values (aligned value columns don't have timestamps, just values)
            let mut decoder = create_decoder(header.encoding_type, header.data_type);
            let mut pos = 0;
            let page_values = self.decode_values(
                &mut decoder,
                &uncompressed,
                &mut pos,
                page_header.num_of_values as usize,
                header.data_type,
            )?;

            // Merge with existing values
            if let Some(ref mut existing) = all_values {
                Self::merge_values(existing, page_values)?;
            } else {
                all_values = Some(page_values);
            }
        }

        let values = all_values.ok_or_else(|| {
            TsFileError::InvalidState("No values read from value chunk".to_string())
        })?;

        Ok(DecodedColumn {
            measurement_name: header.measurement_name.clone(),
            data_type: header.data_type,
            values,
            bitmap: Some(bitmap),
        })
    }

    fn decode_values(
        &self,
        decoder: &mut DecoderImpl,
        data: &[u8],
        pos: &mut usize,
        count: usize,
        data_type: TSDataType,
    ) -> Result<DecodedValues> {
        match data_type {
            TSDataType::Boolean => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_bool(data, pos)?);
                }
                Ok(DecodedValues::Boolean(values))
            }
            TSDataType::Int32 | TSDataType::Date => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_i32(data, pos)?);
                }
                Ok(DecodedValues::Int32(values))
            }
            TSDataType::Int64 | TSDataType::Timestamp => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_i64(data, pos)?);
                }
                Ok(DecodedValues::Int64(values))
            }
            TSDataType::Float => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_f32(data, pos)?);
                }
                Ok(DecodedValues::Float(values))
            }
            TSDataType::Double => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_f64(data, pos)?);
                }
                Ok(DecodedValues::Double(values))
            }
            TSDataType::Text | TSDataType::String => {
                let mut values = Vec::with_capacity(count);
                while decoder.has_remaining(data, *pos) && values.len() < count {
                    values.push(decoder.read_string(data, pos)?);
                }
                Ok(DecodedValues::Text(values))
            }
            _ => Err(TsFileError::TypeMismatch {
                expected: "supported type".to_string(),
                actual: format!("{:?}", data_type),
            }),
        }
    }

    fn merge_values(existing: &mut DecodedValues, new_values: DecodedValues) -> Result<()> {
        match (existing, new_values) {
            (DecodedValues::Boolean(e), DecodedValues::Boolean(n)) => e.extend(n),
            (DecodedValues::Int32(e), DecodedValues::Int32(n)) => e.extend(n),
            (DecodedValues::Int64(e), DecodedValues::Int64(n)) => e.extend(n),
            (DecodedValues::Float(e), DecodedValues::Float(n)) => e.extend(n),
            (DecodedValues::Double(e), DecodedValues::Double(n)) => e.extend(n),
            (DecodedValues::Text(e), DecodedValues::Text(n)) => e.extend(n),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: "matching types".to_string(),
                    actual: "mismatched types".to_string(),
                });
            }
        }
        Ok(())
    }

    /// Gets a complete row (timestamp + all values) at the given index
    pub fn get_row(&self, index: usize) -> Option<(i64, HashMap<String, Option<DecodedValue>>)> {
        if index >= self.timestamps.len() {
            return None;
        }

        let timestamp = self.timestamps[index];
        let mut row = HashMap::new();

        for (name, column) in &self.columns {
            row.insert(name.clone(), column.get(index));
        }

        Some((timestamp, row))
    }

    /// Gets a specific column by measurement name
    pub fn get_column(&self, measurement_name: &str) -> Option<&DecodedColumn> {
        self.columns.get(measurement_name)
    }

    /// Gets all timestamps
    pub fn timestamps(&self) -> &[i64] {
        &self.timestamps
    }

    /// Gets all columns
    pub fn columns(&self) -> &HashMap<String, DecodedColumn> {
        &self.columns
    }

    /// Iterator over all rows
    pub fn iter_rows(&self) -> AlignedChunkIterator<'_> {
        AlignedChunkIterator {
            reader: self,
            index: 0,
        }
    }

    /// Filter rows by time range
    pub fn filter_time_range(
        &self,
        min_time: i64,
        max_time: i64,
    ) -> Vec<(i64, HashMap<String, Option<DecodedValue>>)> {
        self.iter_rows()
            .filter(|(ts, _)| *ts >= min_time && *ts <= max_time)
            .collect()
    }

    /// Number of rows
    pub fn row_count(&self) -> usize {
        self.timestamps.len()
    }

    /// Number of columns
    pub fn column_count(&self) -> usize {
        self.columns.len()
    }

    /// Device ID
    pub fn device_id(&self) -> &str {
        &self.device_id
    }
}

/// Iterator for aligned chunk rows
pub struct AlignedChunkIterator<'a> {
    reader: &'a AlignedChunkReader,
    index: usize,
}

impl<'a> Iterator for AlignedChunkIterator<'a> {
    type Item = (i64, HashMap<String, Option<DecodedValue>>);

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.reader.get_row(self.index);
        self.index += 1;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::TsValue;
    use crate::writer::AlignedChunkWriter;
    use std::io::Cursor;

    #[test]
    fn test_aligned_chunk_round_trip() {
        // Write aligned chunk
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

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Read aligned chunk back
        let mut cursor = Cursor::new(buffer);
        let mut reader = AlignedChunkReader::new("device1".to_string());

        // Read time chunk header and data
        let time_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_aligned_chunk(&mut cursor, time_header).unwrap();

        // Read temperature chunk
        let temp_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_value_chunk(&mut cursor, temp_header).unwrap();

        // Read humidity chunk
        let humidity_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader
            .read_value_chunk(&mut cursor, humidity_header)
            .unwrap();

        // Verify data
        assert_eq!(reader.row_count(), 3);
        assert_eq!(reader.column_count(), 2);

        // Check timestamps
        assert_eq!(reader.timestamps(), &[1000, 2000, 3000]);

        // Check first row
        let (ts, row) = reader.get_row(0).unwrap();
        assert_eq!(ts, 1000);

        if let Some(DecodedValue::Float(v)) = row.get("temperature").unwrap() {
            assert_eq!(*v, 25.5);
        } else {
            panic!("Expected temperature float value");
        }

        if let Some(DecodedValue::Int32(v)) = row.get("humidity").unwrap() {
            assert_eq!(*v, 60);
        } else {
            panic!("Expected humidity int32 value");
        }
    }

    #[test]
    fn test_aligned_chunk_iteration() {
        // Write aligned chunk
        let schemas = vec![(
            "sensor".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        )];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        for i in 0..5 {
            writer
                .write_row(1000 + i * 100, vec![Some(TsValue::Int32(i as i32))])
                .unwrap();
        }

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Read back
        let mut cursor = Cursor::new(buffer);
        let mut reader = AlignedChunkReader::new("device1".to_string());

        let time_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_aligned_chunk(&mut cursor, time_header).unwrap();

        let value_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_value_chunk(&mut cursor, value_header).unwrap();

        // Iterate and verify
        let rows: Vec<_> = reader.iter_rows().collect();
        assert_eq!(rows.len(), 5);

        for (i, (ts, row)) in rows.iter().enumerate() {
            assert_eq!(*ts, 1000 + i as i64 * 100);

            if let Some(DecodedValue::Int32(v)) = row.get("sensor").unwrap() {
                assert_eq!(*v, i as i32);
            }
        }
    }

    #[test]
    fn test_aligned_chunk_filter_time_range() {
        // Write aligned chunk
        let schemas = vec![(
            "value".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        )];

        let mut writer = AlignedChunkWriter::new("device1".to_string(), schemas);

        for i in 0..10 {
            writer
                .write_row(1000 + i * 100, vec![Some(TsValue::Int32(i as i32))])
                .unwrap();
        }

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Read back
        let mut cursor = Cursor::new(buffer);
        let mut reader = AlignedChunkReader::new("device1".to_string());

        let time_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_aligned_chunk(&mut cursor, time_header).unwrap();

        let value_header = ChunkHeader::deserialize(&mut cursor).unwrap();
        reader.read_value_chunk(&mut cursor, value_header).unwrap();

        // Filter time range
        let filtered = reader.filter_time_range(1200, 1600);
        assert_eq!(filtered.len(), 5); // 1200, 1300, 1400, 1500, 1600

        for (i, (ts, _)) in filtered.iter().enumerate() {
            assert_eq!(*ts, 1200 + i as i64 * 100);
        }
    }
}
