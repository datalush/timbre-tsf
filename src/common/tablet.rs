//! Batch-oriented data structures for efficient time-series ingestion.
//!
//! This module provides the [`Tablet`] abstraction, which enables efficient batch writing
//! of time-series data by collecting multiple rows in columnar format before encoding.
//! This approach significantly outperforms row-by-row writes by:
//!
//! - Amortizing encoding overhead across many values
//! - Enabling SIMD and vectorized operations
//! - Improving CPU cache locality
//! - Reducing function call overhead
//!
//! # Data model
//!
//! A [`Tablet`] represents a batch of time-series data for a single device with multiple
//! measurements (columns). It stores:
//!
//! - **Timestamps**: Shared across all measurements in a row
//! - **Values**: Columnar storage per measurement (type-specific vectors)
//! - **Null bitmaps**: Track which values are null
//! - **Schema metadata**: Measurement names and types
//!
//! # Alignment modes
//!
//! Tablets support two modes:
//!
//! - **Non-aligned** (default): Each measurement can have independent timestamps (sparse data)
//! - **Aligned**: All measurements share the same timestamps (dense data, better compression)
//!
//! Aligned mode enforces strictly increasing timestamps and requires all measurements to
//! have values at each timestamp (individual values can still be null).
//!
//! # Performance
//!
//! For bulk operations from Arrow or other columnar formats, use [`Tablet::add_rows_bulk`]
//! which is 3-5x faster than calling [`Tablet::add_row`] in a loop.
//!
//! # Examples
//!
//! ```rust
//! use timbre_tsf::common::tablet::Tablet;
//! use timbre_tsf::common::schema::MeasurementSchema;
//! use timbre_tsf::common::types::{TSDataType, TsValue, ColumnCategory};
//!
//! // Create a tablet for a temperature sensor device
//! let schemas = vec![
//!     MeasurementSchema::with_defaults("temperature", TSDataType::Float),
//!     MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
//! ];
//!
//! let mut tablet = Tablet::new(
//!     "sensor_01",
//!     schemas,
//!     vec![ColumnCategory::Field, ColumnCategory::Field],
//!     1000, // max rows per batch
//! );
//!
//! // Add a row with both measurements
//! tablet.add_row(
//!     1000,
//!     vec![Some(TsValue::Float(22.5)), Some(TsValue::Int32(65))],
//! ).unwrap();
//!
//! // Add a row with a null value
//! tablet.add_row(
//!     2000,
//!     vec![Some(TsValue::Float(23.1)), None],
//! ).unwrap();
//! ```

use super::schema::MeasurementSchema;
use super::types::{ColumnCategory, TSDataType, TsValue};
use crate::error::{Result, TsFileError};
use std::borrow::Cow;
use std::sync::Arc;

/// Compact bitmap for tracking null values in a column.
///
/// Uses a bit-packed representation where each bit indicates whether the value
/// at that index is null (1) or not null (0). This provides 8x memory efficiency
/// compared to storing booleans in a `Vec<bool>`.
///
/// # Layout
///
/// Bits are packed into bytes in little-endian order within each byte:
/// - Byte 0, bit 0 = index 0
/// - Byte 0, bit 1 = index 1
/// - Byte 0, bit 7 = index 7
/// - Byte 1, bit 0 = index 8
/// - etc.
#[derive(Debug, Clone)]
pub struct BitMap {
    bits: Vec<u8>,
    size: usize,
}

impl BitMap {
    /// Creates a new bitmap with all bits initialized to 0 (not null).
    ///
    /// # Arguments
    ///
    /// * `size` - The number of bits to track
    pub fn new(size: usize) -> Self {
        let byte_count = size.div_ceil(8);
        Self {
            bits: vec![0; byte_count],
            size,
        }
    }

    /// Sets the null status for a specific index.
    ///
    /// # Arguments
    ///
    /// * `index` - The position to update
    /// * `is_null` - `true` if the value is null, `false` otherwise
    ///
    /// # Behavior
    ///
    /// If `index >= size`, this is a no-op (silent failure for performance).
    pub fn set(&mut self, index: usize, is_null: bool) {
        if index >= self.size {
            return;
        }
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        if is_null {
            self.bits[byte_idx] |= 1 << bit_idx;
        } else {
            self.bits[byte_idx] &= !(1 << bit_idx);
        }
    }

    /// Returns whether the value at the given index is null.
    ///
    /// # Arguments
    ///
    /// * `index` - The position to check
    ///
    /// # Returns
    ///
    /// `true` if the value is null, `false` if not null or if `index >= size`.
    pub fn get(&self, index: usize) -> bool {
        if index >= self.size {
            return false;
        }
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        (self.bits[byte_idx] & (1 << bit_idx)) != 0
    }

    /// Returns `true` if no values are null (all bits are 0).
    ///
    /// This is useful for optimization: if all values are non-null, some encodings
    /// can omit the null bitmap entirely.
    pub fn is_all_not_null(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }
}

/// Type-erased columnar storage for a single measurement.
///
/// Stores a column of values in a type-specific vector. The variant must match
/// the measurement's [`TSDataType`] in the schema.
///
/// # Memory layout
///
/// Each variant uses a contiguous vector for cache-friendly access during encoding.
/// Default values (0, false, empty string) are used for null entries to maintain
/// alignment, with actual null status tracked in a separate [`BitMap`].
#[derive(Debug, Clone)]
/// Columnar storage for measurement values with zero-copy support.
///
/// Uses `Cow` (Clone-on-Write) to allow zero-copy when possible:
/// - `Cow::Borrowed`: References Arrow buffer directly (0 copies)
/// - `Cow::Owned`: Owns the data when gather is needed (1 copy)
///
/// The encoder only needs `&[T]`, so it doesn't care if data is borrowed or owned.
pub enum ValueMatrix<'a> {
    Boolean(Cow<'a, [bool]>),
    Int32(Cow<'a, [i32]>),
    Int64(Cow<'a, [i64]>),
    Float(Cow<'a, [f32]>),
    Double(Cow<'a, [f64]>),
    // String cannot use Cow<'a, [String]> because it needs owned Strings
    // Keep as Vec for now (text is less common in time-series)
    Text(Vec<String>),
}

impl<'a> ValueMatrix<'a> {
    /// Creates a new empty value matrix with pre-allocated capacity (owned).
    ///
    /// # Arguments
    ///
    /// * `data_type` - The data type determines which variant to create
    /// * `capacity` - Initial capacity to pre-allocate
    pub fn new(data_type: TSDataType, capacity: usize) -> Self {
        match data_type {
            TSDataType::Boolean => Self::Boolean(Cow::Owned(Vec::with_capacity(capacity))),
            TSDataType::Int32 | TSDataType::Date => {
                Self::Int32(Cow::Owned(Vec::with_capacity(capacity)))
            }
            TSDataType::Int64 | TSDataType::Timestamp => {
                Self::Int64(Cow::Owned(Vec::with_capacity(capacity)))
            }
            TSDataType::Float => Self::Float(Cow::Owned(Vec::with_capacity(capacity))),
            TSDataType::Double => Self::Double(Cow::Owned(Vec::with_capacity(capacity))),
            TSDataType::Text | TSDataType::String => Self::Text(Vec::with_capacity(capacity)),
            _ => Self::Int32(Cow::Owned(Vec::with_capacity(capacity))),
        }
    }

    /// Returns the data type of this value matrix.
    pub fn data_type(&self) -> TSDataType {
        match self {
            Self::Boolean(_) => TSDataType::Boolean,
            Self::Int32(_) => TSDataType::Int32,
            Self::Int64(_) => TSDataType::Int64,
            Self::Float(_) => TSDataType::Float,
            Self::Double(_) => TSDataType::Double,
            Self::Text(_) => TSDataType::Text,
        }
    }

    /// Returns the number of values currently stored.
    pub fn len(&self) -> usize {
        match self {
            Self::Boolean(v) => v.len(),
            Self::Int32(v) => v.len(),
            Self::Int64(v) => v.len(),
            Self::Float(v) => v.len(),
            Self::Double(v) => v.len(),
            Self::Text(v) => v.len(),
        }
    }

    /// Returns `true` if the matrix contains no values.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Batch container for time-series data written to a TsFile.
///
/// A tablet collects multiple rows of data in columnar format for a single device
/// before encoding and writing. This batch-oriented approach provides significant
/// performance benefits over row-by-row writes.
///
/// # Fields
///
/// - `device_name`: The device/entity this data belongs to
/// - `schemas`: Measurement metadata (name, type, encoding, compression)
/// - `column_categories`: Whether each column is a field or tag
/// - `timestamps`: Timestamp values (shared across all measurements in a row)
/// - `values`: Columnar storage, one [`ValueMatrix`] per measurement
/// - `bitmaps`: Null tracking, one [`BitMap`] per measurement
/// - `max_rows`: Maximum rows before the tablet must be flushed
///
/// # Alignment
///
/// The tablet can operate in two modes:
///
/// - **Non-aligned** (default): Compatible with sparse data where different measurements
///   may have values at different timestamps. Use [`Tablet::new`].
/// - **Aligned**: Optimized for dense data where all measurements share the same timestamps.
///   Enforces strictly increasing timestamps. Use [`Tablet::new_aligned`].
///
/// # Capacity
///
/// Once `row_count() >= max_rows`, the tablet is full and must be written to the file
/// before accepting more data. Attempting to add more rows will return an error.
#[derive(Debug, Clone)]
pub struct Tablet<'a> {
    pub device_name: String,
    pub schemas: Arc<Vec<MeasurementSchema>>,
    pub column_categories: Vec<ColumnCategory>,
    pub timestamps: Vec<i64>,
    pub values: Vec<ValueMatrix<'a>>,
    pub bitmaps: Vec<BitMap>,
    pub max_rows: usize,
    /// Whether this tablet uses aligned encoding (shared timestamps across measurements)
    is_aligned: bool,
}

impl<'a> Tablet<'a> {
    /// Creates a new non-aligned tablet.
    ///
    /// Non-aligned tablets support sparse data where measurements may have different
    /// timestamps. This is the default mode for backwards compatibility.
    ///
    /// # Arguments
    ///
    /// * `device_name` - The device/entity identifier
    /// * `schemas` - Measurement definitions (name, type, encoding, compression)
    /// * `column_categories` - Whether each column is a field or tag
    /// * `max_rows` - Maximum number of rows before requiring flush
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::tablet::Tablet;
    /// use timbre_tsf::common::schema::MeasurementSchema;
    /// use timbre_tsf::common::types::{TSDataType, ColumnCategory};
    ///
    /// let schemas = vec![
    ///     MeasurementSchema::with_defaults("temp", TSDataType::Float),
    /// ];
    ///
    /// let tablet = Tablet::new("device1", schemas, vec![ColumnCategory::Field], 1000);
    /// assert!(!tablet.is_aligned());
    /// ```
    pub fn new(
        device_name: impl Into<String>,
        schemas: Vec<MeasurementSchema>,
        column_categories: Vec<ColumnCategory>,
        max_rows: usize,
    ) -> Self {
        Self::new_with_alignment(device_name, schemas, column_categories, max_rows, false)
    }

    /// Creates a new aligned tablet.
    ///
    /// Aligned tablets are optimized for dense data where all measurements share the
    /// same timestamps. This mode:
    ///
    /// - Enforces strictly increasing timestamps
    /// - Requires all measurements to have values at each timestamp (can be null)
    /// - Produces better compression in the TsFile format
    ///
    /// # Arguments
    ///
    /// * `device_name` - The device/entity identifier
    /// * `schemas` - Measurement definitions (name, type, encoding, compression)
    /// * `column_categories` - Whether each column is a field or tag
    /// * `max_rows` - Maximum number of rows before requiring flush
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::tablet::Tablet;
    /// use timbre_tsf::common::schema::MeasurementSchema;
    /// use timbre_tsf::common::types::{TSDataType, ColumnCategory};
    ///
    /// let schemas = vec![
    ///     MeasurementSchema::with_defaults("temp", TSDataType::Float),
    /// ];
    ///
    /// let tablet = Tablet::new_aligned("device1", schemas, vec![ColumnCategory::Field], 1000);
    /// assert!(tablet.is_aligned());
    /// ```
    pub fn new_aligned(
        device_name: impl Into<String>,
        schemas: Vec<MeasurementSchema>,
        column_categories: Vec<ColumnCategory>,
        max_rows: usize,
    ) -> Self {
        Self::new_with_alignment(device_name, schemas, column_categories, max_rows, true)
    }

    /// Internal constructor with explicit alignment parameter.
    fn new_with_alignment(
        device_name: impl Into<String>,
        schemas: Vec<MeasurementSchema>,
        column_categories: Vec<ColumnCategory>,
        max_rows: usize,
        is_aligned: bool,
    ) -> Self {
        let schema_count = schemas.len();
        let values = schemas
            .iter()
            .map(|s| ValueMatrix::new(s.data_type, max_rows))
            .collect();
        let bitmaps = (0..schema_count).map(|_| BitMap::new(max_rows)).collect();

        Self {
            device_name: device_name.into(),
            schemas: Arc::new(schemas),
            column_categories,
            timestamps: Vec::with_capacity(max_rows),
            values,
            bitmaps,
            max_rows,
            is_aligned,
        }
    }

    /// Returns whether this tablet uses aligned encoding.
    pub fn is_aligned(&self) -> bool {
        self.is_aligned
    }

    /// Returns the number of rows currently stored in the tablet.
    pub fn row_count(&self) -> usize {
        self.timestamps.len()
    }

    /// Returns the number of measurements (columns) in the tablet.
    pub fn column_count(&self) -> usize {
        self.schemas.len()
    }

    /// Returns `true` if the tablet has reached its maximum capacity.
    ///
    /// A full tablet must be written to the file before accepting more data.
    pub fn is_full(&self) -> bool {
        self.row_count() >= self.max_rows
    }

    /// Adds a single row of data to the tablet.
    ///
    /// This method validates that:
    /// - The tablet is not full
    /// - The number of values matches the schema
    /// - For aligned tablets, timestamps are strictly increasing
    /// - Value types match the schema
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp for this row (milliseconds since epoch)
    /// * `values` - Values for each measurement (must match schema order), `None` for nulls
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The tablet is full (`is_full() == true`)
    /// - Wrong number of values provided
    /// - For aligned tablets, timestamp is not strictly increasing
    /// - Value type doesn't match the schema
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::tablet::Tablet;
    /// use timbre_tsf::common::schema::MeasurementSchema;
    /// use timbre_tsf::common::types::{TSDataType, TsValue, ColumnCategory};
    ///
    /// let schemas = vec![MeasurementSchema::with_defaults("temp", TSDataType::Float)];
    /// let mut tablet = Tablet::new("device1", schemas, vec![ColumnCategory::Field], 1000);
    ///
    /// tablet.add_row(1000, vec![Some(TsValue::Float(22.5))]).unwrap();
    /// tablet.add_row(2000, vec![None]).unwrap(); // Null value
    /// ```
    pub fn add_row(&mut self, timestamp: i64, values: Vec<Option<TsValue>>) -> Result<()> {
        if self.is_full() {
            return Err(TsFileError::InvalidState("Tablet is full".to_string()));
        }

        if values.len() != self.column_count() {
            return Err(TsFileError::InvalidState(format!(
                "Expected {} values, got {}",
                self.column_count(),
                values.len()
            )));
        }

        // Validation for aligned tablets
        if self.is_aligned {
            // Timestamps must be monotonically increasing
            if let Some(&last_ts) = self.timestamps.last()
                && timestamp <= last_ts
            {
                return Err(TsFileError::InvalidState(format!(
                    "Aligned tablet requires strictly increasing timestamps. Got {} after {}",
                    timestamp, last_ts
                )));
            }
        }

        let row_idx = self.timestamps.len();
        self.timestamps.push(timestamp);

        for (col_idx, value) in values.into_iter().enumerate() {
            match value {
                Some(val) => {
                    self.bitmaps[col_idx].set(row_idx, false);
                    self.add_value(col_idx, val)?;
                }
                None => {
                    self.bitmaps[col_idx].set(row_idx, true);
                    // Add default value to maintain alignment
                    self.add_default_value(col_idx)?;
                }
            }
        }

        Ok(())
    }

    /// Adds a typed value to a specific column.
    fn add_value(&mut self, col_idx: usize, value: TsValue) -> Result<()> {
        let expected_type = self.schemas[col_idx].data_type;
        let actual_type = value.data_type();
        match (&mut self.values[col_idx], value) {
            (ValueMatrix::Boolean(v), TsValue::Boolean(val)) => v.to_mut().push(val),
            (ValueMatrix::Int32(v), TsValue::Int32(val)) => v.to_mut().push(val),
            (ValueMatrix::Int64(v), TsValue::Int64(val)) => v.to_mut().push(val),
            (ValueMatrix::Float(v), TsValue::Float(val)) => v.to_mut().push(val),
            (ValueMatrix::Double(v), TsValue::Double(val)) => v.to_mut().push(val),
            (ValueMatrix::Text(v), TsValue::Text(val) | TsValue::String(val)) => v.push(val),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: expected_type.to_string(),
                    actual: actual_type.to_string(),
                });
            }
        }
        Ok(())
    }

    /// Adds a default value to a column (used for null entries).
    fn add_default_value(&mut self, col_idx: usize) -> Result<()> {
        match &mut self.values[col_idx] {
            ValueMatrix::Boolean(v) => v.to_mut().push(false),
            ValueMatrix::Int32(v) => v.to_mut().push(0),
            ValueMatrix::Int64(v) => v.to_mut().push(0),
            ValueMatrix::Float(v) => v.to_mut().push(0.0),
            ValueMatrix::Double(v) => v.to_mut().push(0.0),
            ValueMatrix::Text(v) => v.push(String::new()),
        }
        Ok(())
    }

    /// Clears all data from the tablet, resetting it to empty state.
    ///
    /// This is typically called after successfully writing the tablet's data to the file.
    /// The tablet can then be reused for accumulating the next batch.
    pub fn clear(&mut self) {
        self.timestamps.clear();
        for value_vec in &mut self.values {
            match value_vec {
                ValueMatrix::Boolean(v) => v.to_mut().clear(),
                ValueMatrix::Int32(v) => v.to_mut().clear(),
                ValueMatrix::Int64(v) => v.to_mut().clear(),
                ValueMatrix::Float(v) => v.to_mut().clear(),
                ValueMatrix::Double(v) => v.to_mut().clear(),
                ValueMatrix::Text(v) => v.clear(),
            }
        }
        for bitmap in &mut self.bitmaps {
            *bitmap = BitMap::new(self.max_rows);
        }
    }

    /// High-performance bulk append for batch operations.
    ///
    /// This method bypasses per-row validation and uses bulk vector operations for
    /// significantly better performance when converting from Arrow or other columnar
    /// formats.
    ///
    /// # Performance
    ///
    /// Approximately 3-5x faster than calling [`add_row`](Tablet::add_row) in a loop because:
    /// - Uses `extend_from_slice` instead of individual pushes
    /// - No per-row validation overhead
    /// - Better CPU cache locality
    /// - Fewer function calls
    ///
    /// # Arguments
    ///
    /// * `timestamps` - Timestamp values for all rows
    /// * `values` - Columnar values: `values[col_idx][row_idx]`
    ///
    /// # Validation
    ///
    /// This method validates:
    /// - Total row count doesn't exceed `max_rows`
    /// - All columns have the same number of rows
    /// - Number of columns matches schema
    ///
    /// It does NOT validate:
    /// - Timestamp ordering (caller's responsibility for aligned tablets)
    /// - Individual value types (assumed to match schema)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Adding rows would exceed `max_rows`
    /// - Column count doesn't match schema
    /// - Row counts differ across columns
    /// - Value types don't match schema
    ///
    /// # Examples
    ///
    /// ```rust
    /// use timbre_tsf::common::tablet::Tablet;
    /// use timbre_tsf::common::schema::MeasurementSchema;
    /// use timbre_tsf::common::types::{TSDataType, TsValue, ColumnCategory};
    ///
    /// let schemas = vec![MeasurementSchema::with_defaults("temp", TSDataType::Float)];
    /// let mut tablet = Tablet::new("device1", schemas, vec![ColumnCategory::Field], 1000);
    ///
    /// let timestamps = vec![1000, 2000, 3000];
    /// let values = vec![vec![
    ///     Some(TsValue::Float(22.5)),
    ///     Some(TsValue::Float(23.0)),
    ///     Some(TsValue::Float(23.5)),
    /// ]];
    ///
    /// tablet.add_rows_bulk(&timestamps, values).unwrap();
    /// assert_eq!(tablet.row_count(), 3);
    /// ```
    #[inline]
    pub fn add_rows_bulk(
        &mut self,
        timestamps: &[i64],
        values: Vec<Vec<Option<TsValue>>>,
    ) -> Result<()> {
        let num_rows = timestamps.len();

        if num_rows == 0 {
            return Ok(());
        }

        if self.row_count() + num_rows > self.max_rows {
            return Err(TsFileError::InvalidState(format!(
                "Bulk insert would exceed max_rows: {} + {} > {}",
                self.row_count(),
                num_rows,
                self.max_rows
            )));
        }

        if values.len() != self.column_count() {
            return Err(TsFileError::InvalidState(format!(
                "Expected {} columns, got {}",
                self.column_count(),
                values.len()
            )));
        }

        // Validate all columns have correct length
        for (col_idx, col_values) in values.iter().enumerate() {
            if col_values.len() != num_rows {
                return Err(TsFileError::InvalidState(format!(
                    "Column {} has {} rows, expected {}",
                    col_idx,
                    col_values.len(),
                    num_rows
                )));
            }
        }

        // Bulk extend timestamps
        self.timestamps.extend_from_slice(timestamps);

        // Bulk extend each column
        let start_row = self.row_count() - num_rows;
        for (col_idx, col_values) in values.into_iter().enumerate() {
            self.add_column_bulk(col_idx, col_values, start_row)?;
        }

        Ok(())
    }

    /// Bulk appends a single column's values with bitmap updates.
    #[inline]
    fn add_column_bulk(
        &mut self,
        col_idx: usize,
        values: Vec<Option<TsValue>>,
        start_row: usize,
    ) -> Result<()> {
        let expected_type = self.schemas[col_idx].data_type;

        match &mut self.values[col_idx] {
            ValueMatrix::Boolean(cow) => {
                let vec = cow.to_mut();
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Boolean(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(false);
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
            ValueMatrix::Int32(cow) => {
                let vec = cow.to_mut();
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Int32(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(0);
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
            ValueMatrix::Int64(cow) => {
                let vec = cow.to_mut();
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Int64(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(0);
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
            ValueMatrix::Float(cow) => {
                let vec = cow.to_mut();
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Float(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(0.0);
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
            ValueMatrix::Double(cow) => {
                let vec = cow.to_mut();
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Double(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(0.0);
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
            ValueMatrix::Text(vec) => {
                for (i, val) in values.into_iter().enumerate() {
                    match val {
                        Some(TsValue::Text(v) | TsValue::String(v)) => {
                            vec.push(v);
                            self.bitmaps[col_idx].set(start_row + i, false);
                        }
                        None => {
                            vec.push(String::new());
                            self.bitmaps[col_idx].set(start_row + i, true);
                        }
                        Some(v) => {
                            return Err(TsFileError::TypeMismatch {
                                expected: expected_type.to_string(),
                                actual: v.data_type().to_string(),
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

/// A single measurement value at a specific timestamp.
///
/// This is used in the row-oriented [`TsRecord`] API for simple insertions.
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub measurement_name: String,
    pub value: Option<TsValue>,
}

impl DataPoint {
    /// Creates a new data point with a non-null value.
    ///
    /// # Arguments
    ///
    /// * `measurement_name` - The measurement identifier
    /// * `value` - The value to store
    pub fn new(measurement_name: impl Into<String>, value: TsValue) -> Self {
        Self {
            measurement_name: measurement_name.into(),
            value: Some(value),
        }
    }

    /// Creates a new data point with a null value.
    ///
    /// # Arguments
    ///
    /// * `measurement_name` - The measurement identifier
    pub fn null(measurement_name: impl Into<String>) -> Self {
        Self {
            measurement_name: measurement_name.into(),
            value: None,
        }
    }
}

/// A row-oriented time-series record for a single device at a specific timestamp.
///
/// This provides a more intuitive API for inserting sparse data compared to [`Tablet`],
/// but is less efficient for bulk operations.
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::common::tablet::TsRecord;
/// use timbre_tsf::common::types::TsValue;
///
/// let record = TsRecord::new(1000, "device1")
///     .with_value("temperature", TsValue::Float(22.5))
///     .with_value("humidity", TsValue::Int32(65));
///
/// assert_eq!(record.timestamp, 1000);
/// assert_eq!(record.points.len(), 2);
/// ```
#[derive(Debug, Clone)]
pub struct TsRecord {
    pub timestamp: i64,
    pub device_id: String,
    pub points: Vec<DataPoint>,
}

impl TsRecord {
    /// Creates a new empty record at the given timestamp.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - Timestamp in milliseconds since epoch
    /// * `device_id` - Device/entity identifier
    pub fn new(timestamp: i64, device_id: impl Into<String>) -> Self {
        Self {
            timestamp,
            device_id: device_id.into(),
            points: Vec::new(),
        }
    }

    /// Adds a data point to this record (builder pattern).
    ///
    /// # Arguments
    ///
    /// * `point` - The data point to add
    pub fn add_point(mut self, point: DataPoint) -> Self {
        self.points.push(point);
        self
    }

    /// Adds a measurement value to this record (builder pattern).
    ///
    /// # Arguments
    ///
    /// * `measurement_name` - The measurement identifier
    /// * `value` - The value to store
    pub fn with_value(mut self, measurement_name: impl Into<String>, value: TsValue) -> Self {
        self.points.push(DataPoint::new(measurement_name, value));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bitmap() {
        let mut bitmap = BitMap::new(10);
        assert!(!bitmap.get(0));

        bitmap.set(0, true);
        assert!(bitmap.get(0));

        bitmap.set(0, false);
        assert!(!bitmap.get(0));
    }

    #[test]
    fn test_tablet() {
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new(
            "device1",
            schemas,
            vec![ColumnCategory::Field, ColumnCategory::Field],
            100,
        );

        let result = tablet.add_row(
            1000,
            vec![Some(TsValue::Float(25.5)), Some(TsValue::Int32(60))],
        );
        assert!(result.is_ok());
        assert_eq!(tablet.row_count(), 1);

        // With null value
        let result = tablet.add_row(2000, vec![Some(TsValue::Float(26.0)), None]);
        assert!(result.is_ok());
        assert_eq!(tablet.row_count(), 2);
        assert!(tablet.bitmaps[1].get(1));
    }

    #[test]
    fn test_ts_record() {
        let record = TsRecord::new(1000, "device1")
            .with_value("temp", TsValue::Float(25.5))
            .with_value("humidity", TsValue::Int32(60));

        assert_eq!(record.timestamp, 1000);
        assert_eq!(record.points.len(), 2);
    }

    #[test]
    fn test_tablet_aligned_basic() {
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new_aligned(
            "device1",
            schemas,
            vec![ColumnCategory::Field, ColumnCategory::Field],
            100,
        );

        assert!(tablet.is_aligned());
        assert_eq!(tablet.row_count(), 0);

        // Add first row
        let result = tablet.add_row(
            1000,
            vec![Some(TsValue::Float(25.5)), Some(TsValue::Int32(60))],
        );
        assert!(result.is_ok());
        assert_eq!(tablet.row_count(), 1);

        // Add second row with increasing timestamp
        let result = tablet.add_row(
            2000,
            vec![Some(TsValue::Float(26.0)), Some(TsValue::Int32(65))],
        );
        assert!(result.is_ok());
        assert_eq!(tablet.row_count(), 2);
    }

    #[test]
    fn test_tablet_aligned_validation() {
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new_aligned(
            "device1",
            schemas,
            vec![ColumnCategory::Field, ColumnCategory::Field],
            100,
        );

        // Add first row
        tablet
            .add_row(
                1000,
                vec![Some(TsValue::Float(25.5)), Some(TsValue::Int32(60))],
            )
            .unwrap();

        // Try to add row with non-increasing timestamp (should fail)
        let result = tablet.add_row(
            1000,
            vec![Some(TsValue::Float(26.0)), Some(TsValue::Int32(65))],
        );
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("strictly increasing timestamps")
        );

        // Try with decreasing timestamp (should also fail)
        let result = tablet.add_row(
            500,
            vec![Some(TsValue::Float(26.0)), Some(TsValue::Int32(65))],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_tablet_aligned_with_nulls() {
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new_aligned(
            "device1",
            schemas,
            vec![ColumnCategory::Field, ColumnCategory::Field],
            100,
        );

        // Aligned tablets can have null values (just not sparse rows)
        let result = tablet.add_row(1000, vec![Some(TsValue::Float(25.5)), None]);
        assert!(result.is_ok());
        assert!(tablet.bitmaps[1].get(0)); // Second column is null

        let result = tablet.add_row(2000, vec![None, Some(TsValue::Int32(65))]);
        assert!(result.is_ok());
        assert!(tablet.bitmaps[0].get(1)); // First column is null
    }

    #[test]
    fn test_tablet_non_aligned_compat() {
        // Ensure non-aligned tablets still work as before
        let schemas = vec![
            MeasurementSchema::with_defaults("temp", TSDataType::Float),
            MeasurementSchema::with_defaults("humidity", TSDataType::Int32),
        ];

        let mut tablet = Tablet::new(
            "device1",
            schemas,
            vec![ColumnCategory::Field, ColumnCategory::Field],
            100,
        );

        assert!(!tablet.is_aligned());

        // Non-aligned tablets allow non-monotonic timestamps
        tablet
            .add_row(1000, vec![Some(TsValue::Float(25.5)), None])
            .unwrap();
        tablet
            .add_row(500, vec![Some(TsValue::Float(24.0)), None])
            .unwrap(); // Decreasing is OK
        tablet
            .add_row(1500, vec![None, Some(TsValue::Int32(70))])
            .unwrap();

        assert_eq!(tablet.row_count(), 3);
    }
}
