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

//! Arrow -> Timbre conversion

use crate::arrow::types::ArrowConversionConfig;
use crate::common::{ColumnCategory, MeasurementSchema, Tablet, TsValue};
use crate::error::{Result, TimbreError};
use crate::writer::FileWriter;
use arrow::array::*;
use arrow::datatypes::{DataType, TimeUnit};
use arrow::record_batch::RecordBatch;
use rustc_hash::{FxHashMap, FxHashSet}; // OPT: 3-5x faster than SipHash for short strings
use std::path::Path;
use std::sync::Arc;

/// Converts Arrow RecordBatches to Timbre format
///
/// # Example
///
/// ```no_run
/// use timbre_tsf::arrow::FromArrowConverter;
/// use arrow::record_batch::RecordBatch;
///
/// let mut converter = FromArrowConverter::builder("output.timbreile")
///     .with_device_column("device_id")
///     .with_timestamp_column("timestamp")
///     .build()?;
///
/// // Write a batch (example assumes you have a RecordBatch called 'batch')
/// // converter.write_batch(&batch)?;
///
/// converter.finish()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct FromArrowConverter {
    writer: FileWriter,
    config: ArrowConversionConfig,
    device_column: String,
    timestamp_column: String,
    schema_initialized: bool,
}

/// Builder for FromArrowConverter
pub struct FromArrowConverterBuilder {
    path: String,
    config: ArrowConversionConfig,
    device_column: Option<String>,
    timestamp_column: Option<String>,
}

impl FromArrowConverter {
    /// Create a new converter builder
    pub fn builder<P: AsRef<Path>>(path: P) -> FromArrowConverterBuilder {
        FromArrowConverterBuilder {
            path: path.as_ref().to_string_lossy().to_string(),
            config: ArrowConversionConfig::default(),
            device_column: None,
            timestamp_column: None,
        }
    }

    /// Write an Arrow RecordBatch to Timbre (optimized columnar processing with parallelization)
    pub fn write_batch(&mut self, batch: &RecordBatch) -> Result<()> {
        log::debug!(
            "FromArrowConverter::write_batch - Processing {} rows",
            batch.num_rows()
        );

        // Initialize schema on first batch
        if !self.schema_initialized {
            self.initialize_schema(batch)?;
            self.schema_initialized = true;
        }

        let num_rows = batch.num_rows();
        let arrow_schema = batch.schema();

        // Extract device and timestamp columns
        let device_array = self.get_device_array(batch)?;
        let timestamp_array = self.get_timestamp_array(batch)?;

        // Collect measurement columns metadata (avoid re-extracting in loops)
        let mut measurement_cols: Vec<(String, Arc<dyn arrow::array::Array>, DataType)> =
            Vec::new();
        for field in arrow_schema.fields() {
            let field_name = field.name();
            if field_name != &self.device_column && field_name != &self.timestamp_column {
                let column = Arc::clone(batch.column_by_name(field_name).unwrap());
                // Clones necessary: field_name and data_type stored in Vec for later use
                measurement_cols.push((field_name.clone(), column, field.data_type().clone()));
            }
        }

        // OPT #2 OPTIMIZED: Zero-allocation device grouping using &str keys
        // BEFORE: 2M rows × to_string() = 2M allocations (~150-200ms overhead)
        // AFTER: Only allocate unique device strings (typically 5-10 devices)

        // OPT #3: FxHash is 3-5x faster than SipHash for short strings
        // Perf data: SipHash was 12% of CPU time (7.30% BuildHasher + 4.76% SipHash)
        // Expected improvement: ~9% total speedup

        // Pre-scan to find unique devices (amortized O(n), but only ~5 unique strings allocated)
        let unique_devices: FxHashSet<&str> = (0..num_rows)
            .filter(|&i| !device_array.is_null(i))
            .map(|i| device_array.value(i))
            .collect();

        // Now group with zero allocations per row (use &str keys)
        let mut device_indices: FxHashMap<&str, Vec<usize>> =
            FxHashMap::with_capacity_and_hasher(unique_devices.len(), Default::default());
        let expected_rows_per_device = num_rows / unique_devices.len().max(1);

        for row_idx in 0..num_rows {
            if device_array.is_null(row_idx) {
                continue;
            }
            // ZERO ALLOCATION: device_str is &str borrowing from device_array
            let device_str = device_array.value(row_idx);
            device_indices
                .entry(device_str)
                .or_insert_with(|| Vec::with_capacity(expected_rows_per_device))
                .push(row_idx);
        }

        log::debug!("  Grouped into {} devices", device_indices.len());

        // Adaptive parallelization: use parallel processing only when beneficial
        // Threshold: 4 devices (empirically determined - parallel overhead ~= 4 device processing)
        const PARALLEL_THRESHOLD: usize = 4;

        if device_indices.len() >= PARALLEL_THRESHOLD {
            // Parallel path: process devices in parallel, then write sequentially
            log::debug!(
                "  Using parallel device processing ({} devices)",
                device_indices.len()
            );

            use rayon::prelude::*;

            let tablets: Vec<Tablet> = device_indices
                .par_iter()
                .filter(|(_, indices)| !indices.is_empty())
                .map(|(device_id, indices)| {
                    self.build_tablet_for_device(
                        device_id,
                        indices,
                        &arrow_schema,
                        &measurement_cols,
                        &timestamp_array,
                    )
                })
                .collect::<Result<Vec<_>>>()?;

            // Write tablets sequentially (writer requires mut access)
            for tablet in tablets {
                self.writer.write_tablet(&tablet)?;
            }
        } else {
            // Sequential path: process and write devices one at a time
            log::debug!(
                "  Using sequential device processing ({} devices)",
                device_indices.len()
            );

            for (device_id, indices) in device_indices {
                if indices.is_empty() {
                    continue;
                }

                let tablet = self.build_tablet_for_device(
                    &device_id,
                    &indices,
                    &arrow_schema,
                    &measurement_cols,
                    &timestamp_array,
                )?;

                self.writer.write_tablet(&tablet)?;
            }
        }

        Ok(())
    }

    /// Build a tablet for a single device (extracted for parallel/sequential processing)
    fn build_tablet_for_device<'a>(
        &self,
        device_id: &str,
        indices: &[usize],
        arrow_schema: &arrow::datatypes::SchemaRef,
        measurement_cols: &'a [(String, Arc<dyn arrow::array::Array>, DataType)],
        timestamp_array: &[i64],
    ) -> Result<Tablet<'a>> {
        log::debug!(
            "  Device '{}': {} rows (bulk extraction)",
            device_id,
            indices.len()
        );

        // Build schemas from first row only once
        let mut schemas = Vec::with_capacity(measurement_cols.len());
        for (field_name, _column, data_type) in measurement_cols {
            let ts_data_type = crate::arrow::schema_mapping::arrow_type_to_timbre(data_type)?;

            // Check for encoding hint in field metadata
            let field = arrow_schema
                .field_with_name(field_name)
                .expect("field should exist");
            let encoding = self.get_encoding_for_field(field, ts_data_type);

            schemas.push(MeasurementSchema::new(
                field_name.as_str(),
                ts_data_type,
                encoding,
                self.config.default_compression,
            ));
        }

        // Create tablet with exact capacity
        let column_categories = vec![ColumnCategory::Field; schemas.len()];
        let mut tablet = Tablet::new(device_id, schemas, column_categories, indices.len());

        // Extract data column-by-column (bulk operations)
        let device_timestamps: Vec<i64> = indices.iter().map(|&idx| timestamp_array[idx]).collect();

        // OPT-ZERO-COPY: Extract directly to ValueMatrix instead of Vec<Option<TsValue>>
        // BEFORE: Arrow -> Vec<Option<TsValue>> -> unwrap in add_rows_bulk -> Vec<T>
        // AFTER:  Arrow -> Vec<T> (direct, zero intermediate allocations)
        //
        // Benchmark impact: Eliminates 6M TsValue allocations for 2M rows × 3 measurements
        // Expected speedup: ~25-30% (100-120ms saved)
        let mut value_matrices: Vec<crate::common::ValueMatrix> =
            Vec::with_capacity(measurement_cols.len());
        let mut bitmaps: Vec<crate::common::BitMap> = Vec::with_capacity(measurement_cols.len());

        for (_, column, data_type) in measurement_cols {
            let (values, bitmap) = self.extract_column_direct(column, indices, data_type)?;
            value_matrices.push(values);
            bitmaps.push(bitmap);
        }

        // Directly populate tablet's internal structures (bypassing add_rows_bulk validation)
        tablet.timestamps = device_timestamps;
        tablet.values = value_matrices;
        tablet.bitmaps = bitmaps;

        Ok(tablet)
    }

    /// Extract column directly to ValueMatrix + BitMap (zero-copy optimization)
    ///
    /// OPT-P0: Direct Arrow -> ValueMatrix conversion without TsValue intermediate
    /// BEFORE: Arrow -> Vec<Option<TsValue>> -> Pattern match unwrap -> Vec<T>
    /// AFTER:  Arrow -> Cow<[T]> + BitMap (zero-copy when possible)
    ///
    /// Returns: (ValueMatrix, BitMap) where bitmap marks null positions
    #[inline]
    fn extract_column_direct<'a>(
        &self,
        array: &'a Arc<dyn arrow::array::Array>,
        indices: &[usize],
        data_type: &DataType,
    ) -> Result<(crate::common::ValueMatrix<'a>, crate::common::BitMap)> {
        use crate::common::{BitMap, ValueMatrix};

        let num_values = indices.len();
        let mut bitmap = BitMap::new(num_values);

        let value_matrix = match data_type {
            DataType::Boolean => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<BooleanArray>().unwrap();
                let mut values = Vec::with_capacity(num_values);
                for (i, &idx) in indices.iter().enumerate() {
                    if arr.is_null(idx) {
                        values.push(false);
                        bitmap.set(i, true);
                    } else {
                        values.push(arr.value(idx));
                    }
                }
                ValueMatrix::Boolean(Cow::Owned(values))
            }
            DataType::Int32 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Int32Array>().unwrap();

                // OPT-ARROW: Direct buffer access
                let arrow_buffer = arr.values();
                let is_contiguous =
                    indices.len() > 1 && indices.windows(2).all(|w| w[1] == w[0] + 1);

                let values = if is_contiguous && arr.null_count() == 0 {
                    // ZERO-COPY: Borrow directly from Arrow buffer
                    let start = indices[0];
                    let end = indices[indices.len() - 1] + 1;
                    Cow::Borrowed(&arrow_buffer[start..end])
                } else if arr.null_count() == 0 {
                    // ONE COPY: Gather scattered indices
                    Cow::Owned(indices.iter().map(|&idx| arrow_buffer[idx]).collect())
                } else {
                    // ONE COPY: Handle nulls
                    let mut vals = Vec::with_capacity(num_values);
                    for (i, &idx) in indices.iter().enumerate() {
                        if arr.is_null(idx) {
                            vals.push(0);
                            bitmap.set(i, true);
                        } else {
                            vals.push(arrow_buffer[idx]);
                        }
                    }
                    Cow::Owned(vals)
                };

                ValueMatrix::Int32(values)
            }
            DataType::Int64 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Int64Array>().unwrap();

                // OPT-ARROW: Direct buffer access
                let arrow_buffer = arr.values();
                let is_contiguous =
                    indices.len() > 1 && indices.windows(2).all(|w| w[1] == w[0] + 1);

                let values = if is_contiguous && arr.null_count() == 0 {
                    // ZERO-COPY: Borrow directly from Arrow buffer
                    let start = indices[0];
                    let end = indices[indices.len() - 1] + 1;
                    Cow::Borrowed(&arrow_buffer[start..end])
                } else if arr.null_count() == 0 {
                    // ONE COPY: Gather scattered indices
                    Cow::Owned(indices.iter().map(|&idx| arrow_buffer[idx]).collect())
                } else {
                    // ONE COPY: Handle nulls
                    let mut vals = Vec::with_capacity(num_values);
                    for (i, &idx) in indices.iter().enumerate() {
                        if arr.is_null(idx) {
                            vals.push(0);
                            bitmap.set(i, true);
                        } else {
                            vals.push(arrow_buffer[idx]);
                        }
                    }
                    Cow::Owned(vals)
                };

                ValueMatrix::Int64(values)
            }
            DataType::Float32 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();

                // OPT-ARROW: Direct buffer access (20-30% faster than arr.value())
                let arrow_buffer = arr.values(); // &[f32] - zero-copy!

                // OPT-CONTIGUOUS: Check if indices are contiguous (common case: single device)
                let is_contiguous =
                    indices.len() > 1 && indices.windows(2).all(|w| w[1] == w[0] + 1);

                let values = if is_contiguous && arr.null_count() == 0 {
                    // ZERO-COPY: Borrow directly from Arrow buffer (HOT PATH for IoT sensors)
                    let start = indices[0];
                    let end = indices[indices.len() - 1] + 1;
                    Cow::Borrowed(&arrow_buffer[start..end])
                } else if arr.null_count() == 0 {
                    // ONE COPY: Gather scattered indices (multi-device IoT benchmark)
                    Cow::Owned(indices.iter().map(|&idx| arrow_buffer[idx]).collect())
                } else {
                    // ONE COPY: Handle nulls
                    let mut vals = Vec::with_capacity(num_values);
                    for (i, &idx) in indices.iter().enumerate() {
                        if arr.is_null(idx) {
                            vals.push(0.0);
                            bitmap.set(i, true);
                        } else {
                            vals.push(arrow_buffer[idx]);
                        }
                    }
                    Cow::Owned(vals)
                };

                ValueMatrix::Float(values)
            }
            DataType::Float64 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();

                // OPT-ARROW: Direct buffer access
                let arrow_buffer = arr.values();

                // OPT-CONTIGUOUS: Check if indices are contiguous
                let is_contiguous =
                    indices.len() > 1 && indices.windows(2).all(|w| w[1] == w[0] + 1);

                let values = if is_contiguous && arr.null_count() == 0 {
                    // ZERO-COPY: Borrow directly from Arrow buffer
                    let start = indices[0];
                    let end = indices[indices.len() - 1] + 1;
                    Cow::Borrowed(&arrow_buffer[start..end])
                } else if arr.null_count() == 0 {
                    // ONE COPY: Gather scattered indices
                    Cow::Owned(indices.iter().map(|&idx| arrow_buffer[idx]).collect())
                } else {
                    // ONE COPY: Handle nulls
                    let mut vals = Vec::with_capacity(num_values);
                    for (i, &idx) in indices.iter().enumerate() {
                        if arr.is_null(idx) {
                            vals.push(0.0);
                            bitmap.set(i, true);
                        } else {
                            vals.push(arrow_buffer[idx]);
                        }
                    }
                    Cow::Owned(vals)
                };

                ValueMatrix::Double(values)
            }
            DataType::Utf8 => {
                let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
                let mut values = Vec::with_capacity(num_values);
                for (i, &idx) in indices.iter().enumerate() {
                    if arr.is_null(idx) {
                        values.push(String::new());
                        bitmap.set(i, true);
                    } else {
                        values.push(arr.value(idx).to_string());
                    }
                }
                ValueMatrix::Text(values)
            }
            // Handle type promotions (Int8/16 -> Int32, UInt -> Int)
            DataType::Int8 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Int8Array>().unwrap();
                let mut values = Vec::with_capacity(num_values);
                for (i, &idx) in indices.iter().enumerate() {
                    if arr.is_null(idx) {
                        values.push(0);
                        bitmap.set(i, true);
                    } else {
                        values.push(arr.value(idx) as i32);
                    }
                }
                ValueMatrix::Int32(Cow::Owned(values))
            }
            DataType::Int16 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<Int16Array>().unwrap();
                let mut values = Vec::with_capacity(num_values);
                for (i, &idx) in indices.iter().enumerate() {
                    if arr.is_null(idx) {
                        values.push(0);
                        bitmap.set(i, true);
                    } else {
                        values.push(arr.value(idx) as i32);
                    }
                }
                ValueMatrix::Int32(Cow::Owned(values))
            }
            DataType::UInt8 | DataType::UInt16 | DataType::UInt32 => {
                use std::borrow::Cow;
                // Promote all UInt to Int32
                let mut values = Vec::with_capacity(num_values);
                match data_type {
                    DataType::UInt8 => {
                        let arr = array.as_any().downcast_ref::<UInt8Array>().unwrap();
                        for (i, &idx) in indices.iter().enumerate() {
                            if arr.is_null(idx) {
                                values.push(0);
                                bitmap.set(i, true);
                            } else {
                                values.push(arr.value(idx) as i32);
                            }
                        }
                    }
                    DataType::UInt16 => {
                        let arr = array.as_any().downcast_ref::<UInt16Array>().unwrap();
                        for (i, &idx) in indices.iter().enumerate() {
                            if arr.is_null(idx) {
                                values.push(0);
                                bitmap.set(i, true);
                            } else {
                                values.push(arr.value(idx) as i32);
                            }
                        }
                    }
                    DataType::UInt32 => {
                        let arr = array.as_any().downcast_ref::<UInt32Array>().unwrap();
                        for (i, &idx) in indices.iter().enumerate() {
                            if arr.is_null(idx) {
                                values.push(0);
                                bitmap.set(i, true);
                            } else {
                                values.push(arr.value(idx) as i32);
                            }
                        }
                    }
                    _ => unreachable!(),
                }
                ValueMatrix::Int32(Cow::Owned(values))
            }
            DataType::UInt64 => {
                use std::borrow::Cow;
                let arr = array.as_any().downcast_ref::<UInt64Array>().unwrap();
                let mut values = Vec::with_capacity(num_values);
                for (i, &idx) in indices.iter().enumerate() {
                    if arr.is_null(idx) {
                        values.push(0);
                        bitmap.set(i, true);
                    } else {
                        values.push(arr.value(idx) as i64);
                    }
                }
                ValueMatrix::Int64(Cow::Owned(values))
            }
            _ => {
                return Err(TimbreError::NotImplemented(format!(
                    "Unsupported Arrow data type for direct extraction: {:?}",
                    data_type
                )));
            }
        };

        Ok((value_matrix, bitmap))
    }

    /// Extract an entire column's values for given row indices (bulk extraction)
    ///
    /// DEPRECATED: Use extract_column_direct() for better performance
    ///
    /// OPT #3: Optimized bulk extraction with better null handling
    /// This is ~2-3x faster than row-by-row extraction because:
    /// - Single match on data type instead of per-row
    /// - Better CPU cache locality (sequential access)
    /// - Pre-allocated result vector
    /// - Efficient null bitmap checks
    #[inline]
    #[allow(dead_code)]
    fn extract_column_bulk(
        &self,
        array: &Arc<dyn arrow::array::Array>,
        indices: &[usize],
        data_type: &DataType,
    ) -> Result<Vec<Option<TsValue>>> {
        // Pre-allocate with exact capacity to avoid reallocations
        let mut result = Vec::with_capacity(indices.len());

        match data_type {
            DataType::Boolean => {
                let arr = array.as_any().downcast_ref::<BooleanArray>().unwrap();
                // Check if array has any nulls at all (fast path for non-null data)
                if arr.null_count() == 0 {
                    // Fast path: no null checks needed
                    for &idx in indices {
                        result.push(Some(TsValue::Boolean(arr.value(idx))));
                    }
                } else {
                    // Slow path: check each value
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Boolean(arr.value(idx)))
                        });
                    }
                }
            }
            DataType::Int8 => {
                // Promote Int8 to Int32
                let arr = array.as_any().downcast_ref::<Int8Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx) as i32)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx) as i32))
                        });
                    }
                }
            }
            DataType::Int16 => {
                // Promote Int16 to Int32
                let arr = array.as_any().downcast_ref::<Int16Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx) as i32)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx) as i32))
                        });
                    }
                }
            }
            DataType::Int32 => {
                let arr = array.as_any().downcast_ref::<Int32Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx))));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx)))
                        });
                    }
                }
            }
            DataType::Int64 => {
                let arr = array.as_any().downcast_ref::<Int64Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int64(arr.value(idx))));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int64(arr.value(idx)))
                        });
                    }
                }
            }
            DataType::UInt8 => {
                // Promote UInt8 to Int32
                let arr = array.as_any().downcast_ref::<UInt8Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx) as i32)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx) as i32))
                        });
                    }
                }
            }
            DataType::UInt16 => {
                // Promote UInt16 to Int32
                let arr = array.as_any().downcast_ref::<UInt16Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx) as i32)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx) as i32))
                        });
                    }
                }
            }
            DataType::UInt32 => {
                // Promote UInt32 to Int32 (note: may overflow for values > i32::MAX)
                let arr = array.as_any().downcast_ref::<UInt32Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int32(arr.value(idx) as i32)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int32(arr.value(idx) as i32))
                        });
                    }
                }
            }
            DataType::UInt64 => {
                // Promote UInt64 to Int64 (note: may overflow for values > i64::MAX)
                let arr = array.as_any().downcast_ref::<UInt64Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Int64(arr.value(idx) as i64)));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Int64(arr.value(idx) as i64))
                        });
                    }
                }
            }
            DataType::Float32 => {
                let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();
                if arr.null_count() == 0 {
                    // Fast path: no null checks
                    for &idx in indices {
                        result.push(Some(TsValue::Float(arr.value(idx))));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Float(arr.value(idx)))
                        });
                    }
                }
            }
            DataType::Float64 => {
                let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Double(arr.value(idx))));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Double(arr.value(idx)))
                        });
                    }
                }
            }
            DataType::Utf8 => {
                let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
                if arr.null_count() == 0 {
                    for &idx in indices {
                        result.push(Some(TsValue::Text(arr.value(idx).to_string())));
                    }
                } else {
                    for &idx in indices {
                        result.push(if arr.is_null(idx) {
                            None
                        } else {
                            Some(TsValue::Text(arr.value(idx).to_string()))
                        });
                    }
                }
            }
            _ => {
                return Err(TimbreError::NotImplemented(format!(
                    "Unsupported Arrow data type for bulk extraction: {:?}",
                    data_type
                )));
            }
        }

        Ok(result)
    }

    /// Finish writing and close the Timbre
    pub fn finish(self) -> Result<()> {
        self.writer.close()
    }

    /// Initialize Timbre schema from Arrow schema
    fn initialize_schema(&mut self, _batch: &RecordBatch) -> Result<()> {
        // Register all measurements for all potential devices
        // Note: We'll register schemas lazily as we encounter new devices
        Ok(())
    }

    /// Extract device ID column as string array
    fn get_device_array(&self, batch: &RecordBatch) -> Result<Arc<StringArray>> {
        let device_col = batch.column_by_name(&self.device_column).ok_or_else(|| {
            TimbreError::InvalidState(format!(
                "Device column '{}' not found in RecordBatch",
                self.device_column
            ))
        })?;

        match device_col.data_type() {
            DataType::Utf8 => Ok(device_col
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .to_owned()
                .into()),
            _ => Err(TimbreError::InvalidState(format!(
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
                TimbreError::InvalidState(format!(
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
                            .map(|_| {
                                timestamp_col
                                    .as_any()
                                    .downcast_ref::<TimestampMillisecondArray>()
                                    .unwrap()
                            })
                    })
                    .ok_or_else(|| {
                        TimbreError::InvalidState(format!(
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
                        TimbreError::InvalidState(format!(
                            "Failed to downcast Int64 timestamp column '{}'",
                            self.timestamp_column
                        ))
                    })?;

                let timestamps: Vec<i64> = (0..int_array.len())
                    .map(|i| {
                        if int_array.is_null(i) {
                            0
                        } else {
                            int_array.value(i)
                        }
                    })
                    .collect();

                Ok(timestamps)
            }
            _ => Err(TimbreError::InvalidState(format!(
                "Timestamp column '{}' must be Timestamp or Int64, got {:?}",
                self.timestamp_column,
                timestamp_col.data_type()
            ))),
        }
    }

    /// Get encoding for a field, checking metadata hints first, then falling back to defaults
    ///
    /// Checks for encoding hints in Arrow field metadata with keys:
    /// - "timbre:encoding" (preferred)
    /// - "encoding"
    ///
    /// If no hint is found or parsing fails, falls back to default encoding for the data type.
    fn get_encoding_for_field(
        &self,
        field: &arrow::datatypes::Field,
        ts_data_type: crate::common::TSDataType,
    ) -> crate::common::TSEncoding {
        use crate::common::TSEncoding;

        // Check for encoding hint in field metadata
        let metadata = field.metadata();

        // Try "timbre:encoding" key first (preferred)
        if let Some(encoding_str) = metadata.get("timbre:encoding")
            && let Some(encoding) = TSEncoding::parse_encoding(encoding_str)
        {
            log::debug!(
                "  Using metadata encoding hint for '{}': {} (from timbre:encoding)",
                field.name(),
                encoding
            );
            return encoding;
        }

        // Try "encoding" key as fallback
        if let Some(encoding_str) = metadata.get("encoding")
            && let Some(encoding) = TSEncoding::parse_encoding(encoding_str)
        {
            log::debug!(
                "  Using metadata encoding hint for '{}': {} (from encoding)",
                field.name(),
                encoding
            );
            return encoding;
        }

        // No hint found or parsing failed - use default encoding
        match ts_data_type {
            crate::common::TSDataType::Boolean => self.config.default_encoding_bool,
            crate::common::TSDataType::Int32 => self.config.default_encoding_i32,
            crate::common::TSDataType::Int64 => self.config.default_encoding_i64,
            crate::common::TSDataType::Float => self.config.default_encoding_f32,
            crate::common::TSDataType::Double => self.config.default_encoding_f64,
            crate::common::TSDataType::Text => self.config.default_encoding_string,
            _ => TSEncoding::Plain,
        }
    }
}

impl FromArrowConverterBuilder {
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
    pub fn build(self) -> Result<FromArrowConverter> {
        let device_column = self
            .device_column
            .ok_or_else(|| TimbreError::InvalidState("Device column not specified".to_string()))?;

        let timestamp_column = self.timestamp_column.ok_or_else(|| {
            TimbreError::InvalidState("Timestamp column not specified".to_string())
        })?;

        let writer = FileWriter::new(&self.path)?;

        Ok(FromArrowConverter {
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
    fn test_arrow_to_timbre_basic() {
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

        let batch =
            RecordBatch::try_new(schema, vec![timestamp_array, device_array, temp_array]).unwrap();

        // Convert to Timbre
        let mut converter = FromArrowConverter::builder(path)
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
    fn test_arrow_to_timbre_multiple_devices() {
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
            RecordBatch::try_new(schema, vec![timestamp_array, device_array, value_array]).unwrap();

        // Convert to Timbre
        let mut converter = FromArrowConverter::builder(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();

        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();

        assert!(path.exists());
    }

    #[test]
    fn test_arrow_encoding_hints_from_metadata() {
        use std::collections::HashMap;

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create Arrow schema with encoding hints in metadata
        let mut temp_metadata = HashMap::new();
        temp_metadata.insert("timbre:encoding".to_string(), "chimp128".to_string());

        let mut humidity_metadata = HashMap::new();
        humidity_metadata.insert("encoding".to_string(), "gorilla".to_string());

        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Int64, false),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("temperature", DataType::Float32, true).with_metadata(temp_metadata),
            Field::new("humidity", DataType::Float32, true).with_metadata(humidity_metadata),
        ]));

        // Create RecordBatch
        let timestamp_array = Arc::new(Int64Array::from(vec![1000, 2000, 3000]));
        let device_array = Arc::new(StringArray::from(vec!["device1", "device1", "device1"]));
        let temp_array = Arc::new(Float32Array::from(vec![25.5, 26.0, 26.5]));
        let humid_array = Arc::new(Float32Array::from(vec![60.0, 65.0, 70.0]));

        let batch = RecordBatch::try_new(
            schema,
            vec![timestamp_array, device_array, temp_array, humid_array],
        )
        .unwrap();

        // Convert to Timbre
        let mut converter = FromArrowConverter::builder(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();

        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();

        // Verify file exists (encoding verification would require reading back)
        assert!(path.exists());
    }

    #[test]
    fn test_arrow_parallel_device_processing() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Create Arrow schema
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Int64, false),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("value", DataType::Float32, true),
        ]));

        // Create RecordBatch with >= 4 devices to trigger parallel processing
        let timestamp_array = Arc::new(Int64Array::from(vec![
            1000, 1000, 1000, 1000, 1000, 2000, 2000, 2000, 2000, 2000,
        ]));
        let device_array = Arc::new(StringArray::from(vec![
            "device1", "device2", "device3", "device4", "device5", "device1", "device2", "device3",
            "device4", "device5",
        ]));
        let value_array = Arc::new(Float32Array::from(vec![
            10.0, 20.0, 30.0, 40.0, 50.0, 11.0, 21.0, 31.0, 41.0, 51.0,
        ]));

        let batch =
            RecordBatch::try_new(schema, vec![timestamp_array, device_array, value_array]).unwrap();

        // Convert to Timbre (should trigger parallel processing path)
        let mut converter = FromArrowConverter::builder(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();

        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();

        assert!(path.exists());
    }
}
