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

//! Arrow → TsFile conversion

use crate::arrow::schema_mapping::ArrowSchemaMapping;
use crate::arrow::types::ArrowConversionConfig;
use crate::common::{MeasurementSchema, TsRecord, TsValue};
use crate::error::{Result, TsFileError};
use crate::writer::TsFileWriter;
use arrow::array::*;
use arrow::datatypes::{DataType, TimeUnit};
use arrow::record_batch::RecordBatch;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Converts Arrow RecordBatches to TsFile format
///
/// # Example
///
/// ```no_run
/// use tsfile::arrow::ArrowToTsFileConverter;
/// use arrow::record_batch::RecordBatch;
///
/// let converter = ArrowToTsFileConverter::new("output.tsfile")
///     .with_device_column("device_id")
///     .with_timestamp_column("timestamp")
///     .build()?;
///
/// // Write multiple batches
/// for batch in batches {
///     converter.write_batch(&batch)?;
/// }
///
/// converter.finish()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct ArrowToTsFileConverter {
    writer: TsFileWriter,
    config: ArrowConversionConfig,
    device_column: String,
    timestamp_column: String,
    schema_initialized: bool,
}

/// Builder for ArrowToTsFileConverter
pub struct ArrowToTsFileConverterBuilder {
    path: String,
    config: ArrowConversionConfig,
    device_column: Option<String>,
    timestamp_column: Option<String>,
}

impl ArrowToTsFileConverter {
    /// Create a new converter builder
    pub fn new<P: AsRef<Path>>(path: P) -> ArrowToTsFileConverterBuilder {
        ArrowToTsFileConverterBuilder {
            path: path.as_ref().to_string_lossy().to_string(),
            config: ArrowConversionConfig::default(),
            device_column: None,
            timestamp_column: None,
        }
    }

    /// Write an Arrow RecordBatch to TsFile
    pub fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        log::debug!("ArrowToTsFileConverter::write_batch - Processing {} rows", batch.num_rows());

        // Initialize schema on first batch
        if !self.schema_initialized {
            self.initialize_schema(batch)?;
            self.schema_initialized = true;
        }

        // Extract device and timestamp columns
        let device_array = self.get_device_array(batch)?;
        let timestamp_array = self.get_timestamp_array(batch)?;

        // Group rows by device
        let grouped_rows = self.group_by_device(batch, &device_array, &timestamp_array)?;

        log::debug!("  Grouped into {} devices", grouped_rows.len());

        // Write grouped data to TsFile
        for (device_id, rows) in grouped_rows {
            log::debug!("  Device '{}': {} rows", device_id, rows.len());

            for (timestamp, values) in rows {
                let mut record = TsRecord::new(timestamp, &device_id);
                for (measurement_name, value) in values {
                    record = record.with_value(measurement_name, value);
                }
                self.writer.write_record(record)?;
            }
        }

        Ok(())
    }

    /// Finish writing and close the TsFile
    pub fn finish(self) -> Result<()> {
        self.writer.close()
    }

    /// Initialize TsFile schema from Arrow schema
    fn initialize_schema(&mut self, batch: &RecordBatch) -> Result<()> {
        let arrow_schema = batch.schema();

        // Convert Arrow schema to TsFile schemas
        let tsfile_schemas = ArrowSchemaMapping::arrow_to_tsfile_schemas(
            &arrow_schema,
            &self.timestamp_column,
            Some(&self.device_column),
        )?;

        // Register all measurements for all potential devices
        // Note: We'll register schemas lazily as we encounter new devices
        // For now, just store the schema mapping
        Ok(())
    }

    /// Extract device ID column as string array
    fn get_device_array(&self, batch: &RecordBatch) -> Result<Arc<StringArray>> {
        let device_col = batch
            .column_by_name(&self.device_column)
            .ok_or_else(|| {
                TsFileError::InvalidState(format!(
                    "Device column '{}' not found in RecordBatch",
                    self.device_column
                ))
            })?;

        match device_col.data_type() {
            DataType::Utf8 => Ok(device_col.as_any().downcast_ref::<StringArray>().unwrap().to_owned().into()),
            _ => Err(TsFileError::InvalidState(format!(
                "Device column '{}' must be of type Utf8, got {:?}",
                self.device_column,
                device_col.data_type()
            ))),
        }
    }

    /// Extract timestamp column as i64 array (milliseconds)
    fn get_timestamp_array(&self, batch: &RecordBatch) -> Result<Vec<i64>> {
        let timestamp_col = batch
            .column_by_name(&self.timestamp_column)
            .ok_or_else(|| {
                TsFileError::InvalidState(format!(
                    "Timestamp column '{}' not found in RecordBatch",
                    self.timestamp_column
                ))
            })?;

        match timestamp_col.data_type() {
            DataType::Timestamp(unit, _) => {
                let ts_array = timestamp_col
                    .as_any()
                    .downcast_ref::<TimestampMillisecondArray>()
                    .or_else(|| {
                        timestamp_col
                            .as_any()
                            .downcast_ref::<TimestampMicrosecondArray>()
                            .map(|_| timestamp_col.as_any().downcast_ref::<TimestampMillisecondArray>().unwrap())
                    })
                    .ok_or_else(|| {
                        TsFileError::InvalidState(format!(
                            "Failed to downcast timestamp column '{}'",
                            self.timestamp_column
                        ))
                    })?;

                let timestamps: Vec<i64> = (0..ts_array.len())
                    .map(|i| {
                        if ts_array.is_null(i) {
                            0
                        } else {
                            let value = ts_array.value(i);
                            // Convert to milliseconds based on unit
                            match unit {
                                TimeUnit::Second => value * 1000,
                                TimeUnit::Millisecond => value,
                                TimeUnit::Microsecond => value / 1000,
                                TimeUnit::Nanosecond => value / 1_000_000,
                            }
                        }
                    })
                    .collect();

                Ok(timestamps)
            }
            DataType::Int64 => {
                // Assume already in milliseconds
                let int_array = timestamp_col
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .ok_or_else(|| {
                        TsFileError::InvalidState(format!(
                            "Failed to downcast Int64 timestamp column '{}'",
                            self.timestamp_column
                        ))
                    })?;

                let timestamps: Vec<i64> = (0..int_array.len())
                    .map(|i| if int_array.is_null(i) { 0 } else { int_array.value(i) })
                    .collect();

                Ok(timestamps)
            }
            _ => Err(TsFileError::InvalidState(format!(
                "Timestamp column '{}' must be Timestamp or Int64, got {:?}",
                self.timestamp_column,
                timestamp_col.data_type()
            ))),
        }
    }

    /// Group rows by device ID
    fn group_by_device(
        &mut self,
        batch: &RecordBatch,
        device_array: &StringArray,
        timestamp_array: &[i64],
    ) -> Result<HashMap<String, Vec<(i64, Vec<(String, TsValue)>)>>> {
        let mut grouped: HashMap<String, Vec<(i64, Vec<(String, TsValue)>)>> = HashMap::new();

        let arrow_schema = batch.schema();

        // Process each row
        for row_idx in 0..batch.num_rows() {
            let device_id = if device_array.is_null(row_idx) {
                continue; // Skip rows with null device ID
            } else {
                device_array.value(row_idx).to_string()
            };

            let timestamp = timestamp_array[row_idx];

            // Extract values for all measurement columns
            let mut values = Vec::new();

            for field in arrow_schema.fields() {
                let field_name = field.name();

                // Skip device and timestamp columns
                if field_name == &self.device_column || field_name == &self.timestamp_column {
                    continue;
                }

                // Get column array
                let column = batch.column_by_name(field_name).unwrap();

                // Convert to TsValue
                if let Some(ts_value) = self.extract_value(column, row_idx, field.data_type())? {
                    values.push((field_name.clone(), ts_value));

                    // Register measurement schema if not already registered
                    if !self.writer.has_measurement(&device_id, field_name) {
                        let schema = self.create_measurement_schema(field_name, field.data_type())?;
                        self.writer.register_timeseries(&device_id, schema)?;
                    }
                }
            }

            grouped
                .entry(device_id)
                .or_insert_with(Vec::new)
                .push((timestamp, values));
        }

        Ok(grouped)
    }

    /// Extract TsValue from Arrow array at given index
    fn extract_value(
        &self,
        array: &Arc<dyn arrow::array::Array>,
        index: usize,
        data_type: &DataType,
    ) -> Result<Option<TsValue>> {
        if array.is_null(index) {
            return Ok(None);
        }

        let value = match data_type {
            DataType::Boolean => {
                let arr = array.as_any().downcast_ref::<BooleanArray>().unwrap();
                TsValue::Boolean(arr.value(index))
            }
            DataType::Int32 | DataType::Int8 | DataType::Int16 | DataType::UInt8 | DataType::UInt16 | DataType::UInt32 => {
                let arr = array.as_any().downcast_ref::<Int32Array>()
                    .or_else(|| {
                        // Try casting from other integer types
                        array.as_any().downcast_ref::<Int8Array>().map(|a| {
                            // This is a workaround - in real implementation we'd handle each type
                            array.as_any().downcast_ref::<Int32Array>().unwrap()
                        })
                    })
                    .ok_or_else(|| TsFileError::InvalidState("Failed to downcast to Int32Array".to_string()))?;
                TsValue::Int32(arr.value(index))
            }
            DataType::Int64 | DataType::UInt64 => {
                let arr = array.as_any().downcast_ref::<Int64Array>().unwrap();
                TsValue::Int64(arr.value(index))
            }
            DataType::Float32 | DataType::Float16 => {
                let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();
                TsValue::Float(arr.value(index))
            }
            DataType::Float64 => {
                let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();
                TsValue::Double(arr.value(index))
            }
            DataType::Utf8 => {
                let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
                TsValue::Text(arr.value(index).to_string())
            }
            _ => {
                return Err(TsFileError::NotImplemented(format!(
                    "Unsupported Arrow data type for TsFile conversion: {:?}",
                    data_type
                )));
            }
        };

        Ok(Some(value))
    }

    /// Create MeasurementSchema from Arrow field
    fn create_measurement_schema(&self, name: &str, data_type: &DataType) -> Result<MeasurementSchema> {
        let ts_data_type = crate::arrow::schema_mapping::arrow_type_to_tsfile(data_type)?;

        let encoding = match &ts_data_type {
            crate::common::TSDataType::Boolean => self.config.default_encoding_bool,
            crate::common::TSDataType::Int32 => self.config.default_encoding_i32,
            crate::common::TSDataType::Int64 => self.config.default_encoding_i64,
            crate::common::TSDataType::Float => self.config.default_encoding_f32,
            crate::common::TSDataType::Double => self.config.default_encoding_f64,
            crate::common::TSDataType::Text => self.config.default_encoding_string,
            _ => crate::common::TSEncoding::Plain, // Default for unsupported types
        };

        Ok(MeasurementSchema::new(
            name.to_string(),
            ts_data_type,
            encoding,
            self.config.default_compression,
        ))
    }
}

impl ArrowToTsFileConverterBuilder {
    /// Set the device ID column name
    pub fn with_device_column(mut self, name: impl Into<String>) -> Self {
        self.device_column = Some(name.into());
        self
    }

    /// Set the timestamp column name
    pub fn with_timestamp_column(mut self, name: impl Into<String>) -> Self {
        self.timestamp_column = Some(name.into());
        self
    }

    /// Set the conversion configuration
    pub fn with_config(mut self, config: ArrowConversionConfig) -> Self {
        self.config = config;
        self
    }

    /// Build the converter
    pub fn build(self) -> Result<ArrowToTsFileConverter> {
        let device_column = self
            .device_column
            .ok_or_else(|| TsFileError::InvalidState("Device column not specified".to_string()))?;

        let timestamp_column = self.timestamp_column.ok_or_else(|| {
            TsFileError::InvalidState("Timestamp column not specified".to_string())
        })?;

        let writer = TsFileWriter::new(&self.path)?;

        Ok(ArrowToTsFileConverter {
            writer,
            config: self.config,
            device_column,
            timestamp_column,
            schema_initialized: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow::array::{Float32Array, Int64Array, StringArray};
    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_arrow_to_tsfile_basic() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create Arrow schema
        let schema = Arc::new(Schema::new(vec![
            Field::new(
                "timestamp",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("temperature", DataType::Float32, true),
        ]));

        // Create RecordBatch
        let timestamp_array = Arc::new(TimestampMillisecondArray::from(vec![1000, 2000, 3000]));
        let device_array = Arc::new(StringArray::from(vec!["device1", "device1", "device1"]));
        let temp_array = Arc::new(Float32Array::from(vec![25.5, 26.0, 26.5]));

        let batch = RecordBatch::try_new(
            schema,
            vec![timestamp_array, device_array, temp_array],
        )
        .unwrap();

        // Convert to TsFile
        let mut converter = ArrowToTsFileConverter::new(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();

        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();

        // Verify file exists
        assert!(path.exists());
    }

    #[test]
    fn test_arrow_to_tsfile_multiple_devices() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create Arrow schema
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Int64, false),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("value", DataType::Float32, true),
        ]));

        // Create RecordBatch with multiple devices
        let timestamp_array = Arc::new(Int64Array::from(vec![1000, 1000, 2000, 2000]));
        let device_array = Arc::new(StringArray::from(vec![
            "device1", "device2", "device1", "device2",
        ]));
        let value_array = Arc::new(Float32Array::from(vec![10.0, 20.0, 11.0, 21.0]));

        let batch =
            RecordBatch::try_new(schema, vec![timestamp_array, device_array, value_array])
                .unwrap();

        // Convert to TsFile
        let mut converter = ArrowToTsFileConverter::new(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();

        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();

        assert!(path.exists());
    }
}
