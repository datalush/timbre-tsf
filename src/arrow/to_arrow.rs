/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! TsFile → Arrow conversion

use crate::arrow::types::ArrowConversionConfig;
use crate::error::{Result, TsFileError};
use crate::reader::{DecodedValues, TsFileIOReader};
use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use std::path::Path;
use std::sync::Arc;

/// Reads TsFile as Arrow RecordBatches
///
/// Implements the Arrow RecordBatchReader trait for streaming TsFile data.
///
/// # Example
///
/// ```no_run
/// use tsfile_rs::arrow::TsFileRecordBatchReader;
///
/// let reader = TsFileRecordBatchReader::try_new("input.tsfile")?;
///
/// for batch_result in reader {
///     let batch = batch_result?;
///     println!("Read {} rows", batch.num_rows());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct TsFileRecordBatchReader {
    io_reader: TsFileIOReader,
    arrow_schema: Arc<Schema>,
    device_index: usize,
    devices: Vec<String>,
}

impl TsFileRecordBatchReader {
    /// Create a new TsFile RecordBatch reader
    pub fn try_new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::try_new_with_config(path, ArrowConversionConfig::default())
    }

    /// Create a new reader with custom configuration
    pub fn try_new_with_config<P: AsRef<Path>>(
        path: P,
        _config: ArrowConversionConfig,
    ) -> Result<Self> {
        let io_reader = TsFileIOReader::open(path)?;
        let devices = io_reader.get_devices();

        if devices.is_empty() {
            return Err(TsFileError::InvalidState(
                "TsFile contains no devices".to_string(),
            ));
        }

        // Build Arrow schema from first device's measurements
        let first_device = &devices[0];
        let measurements = io_reader
            .get_measurements(first_device)
            .ok_or_else(|| {
                TsFileError::InvalidState(format!(
                    "Failed to get measurements for device {}",
                    first_device
                ))
            })?;

        let arrow_schema = Self::build_arrow_schema(&io_reader, first_device, &measurements)?;

        Ok(Self {
            io_reader,
            arrow_schema: Arc::new(arrow_schema),
            device_index: 0,
            devices,
        })
    }

    /// Build Arrow schema from TsFile metadata
    fn build_arrow_schema(
        io_reader: &TsFileIOReader,
        device_id: &str,
        measurements: &[String],
    ) -> Result<Schema> {
        let mut fields = Vec::new();

        // Add timestamp column
        fields.push(Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ));

        // Add device_id column
        fields.push(Field::new("device_id", DataType::Utf8, false));

        // Add measurement columns
        for measurement in measurements {
            if let Some(metadata) = io_reader.get_chunk_metadata(device_id, measurement) {
                let arrow_type =
                    crate::arrow::schema_mapping::tsfile_type_to_arrow(&metadata.data_type)?;
                fields.push(Field::new(measurement.clone(), arrow_type, true));
            }
        }

        Ok(Schema::new(fields))
    }

    /// Read next RecordBatch from TsFile
    fn read_next_batch(&mut self) -> Result<Option<RecordBatch>> {
        // Check if we've read all devices
        if self.device_index >= self.devices.len() {
            return Ok(None);
        }

        let device_id = &self.devices[self.device_index].clone();

        // Get measurements for this device
        let measurements = self
            .io_reader
            .get_measurements(device_id)
            .ok_or_else(|| {
                TsFileError::InvalidState(format!(
                    "Failed to get measurements for device {}",
                    device_id
                ))
            })?;

        if measurements.is_empty() {
            self.device_index += 1;
            return self.read_next_batch();
        }

        // Read all chunks for this device - Zero-copy approach
        let mut all_timestamps: Option<Vec<i64>> = None;
        let mut arrays: Vec<Arc<dyn arrow::array::Array>> = Vec::with_capacity(measurements.len() + 2);

        // Placeholder for timestamp and device arrays
        arrays.push(Arc::new(Int32Array::from(vec![0i32; 0])) as Arc<dyn arrow::array::Array>);
        arrays.push(Arc::new(Int32Array::from(vec![0i32; 0])) as Arc<dyn arrow::array::Array>);

        for measurement in measurements.iter() {
            match self.io_reader.read_chunk(device_id, measurement) {
                Ok(chunk) => {
                    // Save timestamps from first chunk
                    if all_timestamps.is_none() {
                        all_timestamps = Some(chunk.timestamps.clone());
                    }

                    // Convert DecodedValues directly to Arrow array (zero-copy!)
                    let array = self.decoded_values_to_arrow(chunk.values)?;
                    arrays.push(array);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to read chunk for {}/{}: {}", device_id, measurement, e);
                    continue;
                }
            }
        }

        // Move to next device
        self.device_index += 1;

        let timestamps = match all_timestamps {
            Some(ts) => ts,
            None => return self.read_next_batch(),
        };

        let num_rows = timestamps.len();

        // Replace placeholder arrays with real data
        arrays[0] = Arc::new(TimestampMillisecondArray::from(timestamps));
        // OPT-READ-3: Use from_iter_values with repeat() - avoids allocating vec
        arrays[1] = Arc::new(StringArray::from_iter_values(
            std::iter::repeat(device_id.as_str()).take(num_rows)
        ));

        // Create RecordBatch
        let batch = RecordBatch::try_new(self.arrow_schema.clone(), arrays).map_err(|e| {
            TsFileError::InvalidState(format!("Failed to create RecordBatch: {}", e))
        })?;

        Ok(Some(batch))
    }

    /// Convert DecodedValues directly to Arrow array (zero-copy)
    /// OPT-READ-2: Eliminates intermediate Vec<&str> for strings
    /// Uses StringArray::from_iter_values which is more efficient
    /// OPT-1: Zero-copy Arrow construction using Buffer::from_vec (takes ownership)
    fn decoded_values_to_arrow(
        &self,
        values: DecodedValues,
    ) -> Result<Arc<dyn arrow::array::Array>> {
        use arrow::buffer::Buffer;
        use arrow::array::ArrayData;
        use arrow::datatypes::DataType;

        let array: Arc<dyn arrow::array::Array> = match values {
            DecodedValues::Boolean(vec) => {
                // BooleanArray has special bit-packed format, can't zero-copy easily
                Arc::new(BooleanArray::from(vec))
            }
            DecodedValues::Int32(vec) => {
                // OPT-1: Zero-copy - Buffer::from_vec takes ownership without copying
                let len = vec.len();
                let buffer = Buffer::from_vec(vec);
                let data = ArrayData::builder(DataType::Int32)
                    .len(len)
                    .add_buffer(buffer)
                    .build()
                    .map_err(|e| crate::error::TsFileError::InvalidState(format!("Failed to build Int32Array: {}", e)))?;
                Arc::new(Int32Array::from(data))
            }
            DecodedValues::Int64(vec) => {
                // OPT-1: Zero-copy
                let len = vec.len();
                let buffer = Buffer::from_vec(vec);
                let data = ArrayData::builder(DataType::Int64)
                    .len(len)
                    .add_buffer(buffer)
                    .build()
                    .map_err(|e| crate::error::TsFileError::InvalidState(format!("Failed to build Int64Array: {}", e)))?;
                Arc::new(Int64Array::from(data))
            }
            DecodedValues::Float(vec) => {
                // OPT-1: Zero-copy
                let len = vec.len();
                let buffer = Buffer::from_vec(vec);
                let data = ArrayData::builder(DataType::Float32)
                    .len(len)
                    .add_buffer(buffer)
                    .build()
                    .map_err(|e| crate::error::TsFileError::InvalidState(format!("Failed to build Float32Array: {}", e)))?;
                Arc::new(Float32Array::from(data))
            }
            DecodedValues::Double(vec) => {
                // OPT-1: Zero-copy
                let len = vec.len();
                let buffer = Buffer::from_vec(vec);
                let data = ArrayData::builder(DataType::Float64)
                    .len(len)
                    .add_buffer(buffer)
                    .build()
                    .map_err(|e| crate::error::TsFileError::InvalidState(format!("Failed to build Float64Array: {}", e)))?;
                Arc::new(Float64Array::from(data))
            }
            DecodedValues::Text(vec) => {
                // OPT-READ-2: Use from_iter_values instead of collecting to Vec<&str>
                // Text arrays are complex (offsets + values), not suitable for simple zero-copy
                Arc::new(StringArray::from_iter_values(vec.iter().map(|s| s.as_str())))
            }
        };

        Ok(array)
    }

    /// Get the Arrow schema
    pub fn schema(&self) -> Arc<Schema> {
        self.arrow_schema.clone()
    }
}

/// Iterator implementation for streaming RecordBatches
pub struct TsFileRecordBatchIterator {
    reader: TsFileRecordBatchReader,
}

impl Iterator for TsFileRecordBatchIterator {
    type Item = Result<RecordBatch>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.reader.read_next_batch() {
            Ok(Some(batch)) => Some(Ok(batch)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        }
    }
}

impl IntoIterator for TsFileRecordBatchReader {
    type Item = Result<RecordBatch>;
    type IntoIter = TsFileRecordBatchIterator;

    fn into_iter(self) -> Self::IntoIter {
        TsFileRecordBatchIterator { reader: self }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue};
    use crate::writer::TsFileWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_tsfile_to_arrow_basic() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write TsFile
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

        // Read as Arrow
        let reader = TsFileRecordBatchReader::try_new(path).unwrap();

        let schema = reader.schema();
        assert_eq!(schema.fields().len(), 3); // timestamp + device_id + temperature

        let batches: Vec<_> = reader.into_iter().collect();
        assert!(!batches.is_empty());

        let first_batch = batches[0].as_ref().unwrap();
        assert_eq!(first_batch.num_rows(), 10);
        assert_eq!(first_batch.num_columns(), 3);
    }

    #[test]
    fn test_tsfile_to_arrow_multiple_measurements() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write TsFile with multiple measurements
        {
            let mut writer = TsFileWriter::new(path).unwrap();

            let temp_schema = MeasurementSchema::new(
                "temperature",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );
            let humidity_schema = MeasurementSchema::new(
                "humidity",
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Lz4,
            );

            writer.register_timeseries("device1", temp_schema).unwrap();
            writer
                .register_timeseries("device1", humidity_schema)
                .unwrap();

            for i in 0..5 {
                let record = TsRecord::new(1000 + i * 100, "device1")
                    .with_value("temperature", TsValue::Float(25.0 + i as f32))
                    .with_value("humidity", TsValue::Int32(60 + i as i32));
                writer.write_record(record).unwrap();
            }

            writer.close().unwrap();
        }

        // Read as Arrow
        let reader = TsFileRecordBatchReader::try_new(path).unwrap();

        let schema = reader.schema();
        assert_eq!(schema.fields().len(), 4); // timestamp + device_id + 2 measurements

        let batches: Vec<_> = reader.into_iter().collect();
        let first_batch = batches[0].as_ref().unwrap();
        assert_eq!(first_batch.num_rows(), 5);
    }
}
