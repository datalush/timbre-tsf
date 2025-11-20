//! Statistical tracking for time-series data in TsFiles.
//!
//! This module provides comprehensive statistical metadata collection for time-series data,
//! including count, temporal bounds, and type-specific aggregates (min, max, sum, first, last).
//! Statistics are computed incrementally during write operations and serialized into the TsFile
//! format's metadata sections for efficient query planning and data skipping.
//!
//! # Design
//!
//! The module uses a trait-based design ([`Statistic`]) with type-specific implementations
//! that maintain statistics tailored to each data type's characteristics:
//!
//! - **Numeric types** (Int32, Int64, Float, Double): Track min/max/sum for range queries
//! - **Boolean**: Tracks sum (true count) for aggregation queries
//! - **String/Text**: Tracks only first/last values (min/max undefined for strings)
//!
//! All statistics share common temporal metadata (count, start_time, end_time) via [`BaseStats`].
//!
//! # Performance
//!
//! Statistics are updated incrementally during encoding with O(1) cost per data point.
//! The serialization format matches the Apache IoTDB TsFile specification for compatibility.
//!
//! # Examples
//!
//! ```rust
//! use timbre_tsf::common::statistic::{create_statistic, Statistic};
//! use timbre_tsf::common::types::TSDataType;
//!
//! // Create a statistic tracker for Int32 data
//! let mut stat = create_statistic(TSDataType::Int32);
//!
//! // Update with time-series data points
//! stat.update_i32(1000, 42);
//! stat.update_i32(2000, 17);
//! stat.update_i32(3000, 99);
//!
//! // Access aggregated statistics
//! assert_eq!(stat.count(), 3);
//! assert_eq!(stat.start_time(), 1000);
//! assert_eq!(stat.end_time(), 3000);
//! ```

use super::types::TSDataType;
use super::TsValue;
use crate::error::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

/// Trait for collecting and serializing statistical metadata for time-series data.
///
/// This trait provides a unified interface for tracking statistics across different data types.
/// Each implementation maintains type-specific statistics (e.g., min/max for numeric types)
/// while sharing common temporal metadata (count, time range).
///
/// The trait is `Send + Sync` to support concurrent access in multi-threaded encoding scenarios,
/// and `Debug` for diagnostic purposes.
///
/// # Type-specific methods
///
/// Each `update_*` method corresponds to a specific data type. Implementations should only
/// process updates matching their type and ignore others (no-op for mismatched types).
///
/// # Serialization
///
/// The serialization format follows the Apache IoTDB TsFile specification and varies by type:
/// - Common fields: count (i32), start_time (i64), end_time (i64)
/// - Type-specific fields: sum, min, max, first, last (type varies)
pub trait Statistic: Send + Sync + std::fmt::Debug {
    /// Updates statistics with a boolean data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The boolean value to incorporate into statistics
    fn update_bool(&mut self, timestamp: i64, value: bool);

    /// Updates statistics with an Int32 data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The 32-bit integer value to incorporate into statistics
    fn update_i32(&mut self, timestamp: i64, value: i32);

    /// Updates statistics with an Int64 data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The 64-bit integer value to incorporate into statistics
    fn update_i64(&mut self, timestamp: i64, value: i64);

    /// Updates statistics with a Float (f32) data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The 32-bit floating point value to incorporate into statistics
    fn update_f32(&mut self, timestamp: i64, value: f32);

    /// Updates statistics with a Double (f64) data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The 64-bit floating point value to incorporate into statistics
    fn update_f64(&mut self, timestamp: i64, value: f64);

    /// Updates statistics with a String/Text data point.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp of the data point in milliseconds
    /// * `value` - The string value to incorporate into statistics
    fn update_string(&mut self, timestamp: i64, value: &str);

    /// Returns the total number of data points incorporated.
    fn count(&self) -> i32;

    /// Returns the earliest timestamp observed (in milliseconds).
    fn start_time(&self) -> i64;

    /// Returns the latest timestamp observed (in milliseconds).
    fn end_time(&self) -> i64;

    /// Serializes the statistics to a writer in TsFile binary format.
    ///
    /// The format is type-specific but always starts with count, start_time, end_time
    /// followed by type-specific fields (sum, min, max, first, last).
    ///
    /// # Arguments
    ///
    /// * `writer` - The writer to serialize statistics to
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the underlying writer fails.
    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()>;

    /// Returns the data type associated with this statistic.
    fn data_type(&self) -> TSDataType;

    /// Returns the minimum value observed, if applicable for this type.
    ///
    /// Returns None for types where min/max is not meaningful (Boolean, String).
    fn min_value(&self) -> Option<TsValue>;

    /// Returns the maximum value observed, if applicable for this type.
    ///
    /// Returns None for types where min/max is not meaningful (Boolean, String).
    fn max_value(&self) -> Option<TsValue>;
}

/// Shared statistical metadata common to all data types.
///
/// This structure tracks temporal bounds and count information that is relevant
/// regardless of the value type. It is embedded in all type-specific statistic
/// implementations to avoid code duplication.
///
/// # Initialization
///
/// On creation, timestamps are initialized to extreme values (i64::MAX, i64::MIN)
/// to ensure the first update correctly establishes the actual bounds.
#[derive(Debug, Clone)]
pub struct BaseStats {
    /// Total number of data points incorporated
    pub count: i32,
    /// Earliest timestamp observed (milliseconds)
    pub start_time: i64,
    /// Latest timestamp observed (milliseconds)
    pub end_time: i64,
}

impl BaseStats {
    /// Creates a new `BaseStats` with initial values.
    ///
    /// The timestamp bounds are initialized to extreme values (i64::MAX for start,
    /// i64::MIN for end) to ensure the first update correctly establishes bounds.
    pub fn new() -> Self {
        Self {
            count: 0,
            start_time: i64::MAX,
            end_time: i64::MIN,
        }
    }

    /// Updates temporal metadata with a new timestamp.
    ///
    /// Increments count and adjusts start_time/end_time if the new timestamp
    /// extends the observed range.
    ///
    /// # Arguments
    ///
    /// * `timestamp` - The timestamp to incorporate (in milliseconds)
    pub fn update_time(&mut self, timestamp: i64) {
        self.count += 1;
        if timestamp < self.start_time {
            self.start_time = timestamp;
        }
        if timestamp > self.end_time {
            self.end_time = timestamp;
        }
    }
}

impl Default for BaseStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistical metadata for boolean time-series data.
///
/// Tracks the count of true values (sum), along with first and last observed values.
/// Boolean statistics do not have min/max as these concepts are undefined for booleans.
///
/// # Fields
///
/// - `sum_value`: Count of true values (false contributes 0, true contributes 1)
/// - `first_value`: The first boolean value observed
/// - `last_value`: The most recent boolean value observed
#[derive(Debug, Clone)]
pub struct BooleanStatistic {
    base: BaseStats,
    sum_value: i64,
    first_value: bool,
    last_value: bool,
}

impl BooleanStatistic {
    /// Creates a new `BooleanStatistic` with initial values.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            sum_value: 0,
            first_value: false,
            last_value: false,
        }
    }
}

impl Default for BooleanStatistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for BooleanStatistic {
    fn update_bool(&mut self, timestamp: i64, value: bool) {
        if self.base.count == 0 {
            self.first_value = value;
        }
        self.last_value = value;
        self.sum_value += value as i64;
        self.base.update_time(timestamp);
    }

    fn update_i32(&mut self, _: i64, _: i32) {}
    fn update_i64(&mut self, _: i64, _: i64) {}
    fn update_f32(&mut self, _: i64, _: f32) {}
    fn update_f64(&mut self, _: i64, _: f64) {}
    fn update_string(&mut self, _: i64, _: &str) {}

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_i64::<LittleEndian>(self.sum_value)?;
        writer.write_u8(self.first_value as u8)?;
        writer.write_u8(self.last_value as u8)?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Boolean
    }

    fn min_value(&self) -> Option<TsValue> {
        None // Boolean doesn't have min/max
    }

    fn max_value(&self) -> Option<TsValue> {
        None // Boolean doesn't have min/max
    }
}

/// Statistical metadata for 32-bit integer time-series data.
///
/// Tracks min, max, sum, first, and last values along with temporal metadata.
/// The sum is stored as i64 to prevent overflow when accumulating many values.
///
/// # Overflow handling
///
/// While individual values are i32, the sum is accumulated in i64 to prevent
/// overflow for typical workloads. For extremely large datasets, sum may still
/// overflow but this matches the TsFile specification behavior.
#[derive(Debug, Clone)]
pub struct Int32Statistic {
    base: BaseStats,
    /// Sum of all values (i64 to prevent overflow)
    sum_value: i64,
    /// Minimum value observed
    min_value: i32,
    /// Maximum value observed
    max_value: i32,
    /// First value observed
    first_value: i32,
    /// Last value observed
    last_value: i32,
}

impl Int32Statistic {
    /// Creates a new `Int32Statistic` with initial values.
    ///
    /// Min and max are initialized to extreme values (i32::MAX, i32::MIN) to ensure
    /// the first update correctly establishes bounds.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            sum_value: 0,
            min_value: i32::MAX,
            max_value: i32::MIN,
            first_value: 0,
            last_value: 0,
        }
    }
}

impl Default for Int32Statistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for Int32Statistic {
    fn update_bool(&mut self, _: i64, _: bool) {}

    fn update_i32(&mut self, timestamp: i64, value: i32) {
        if self.base.count == 0 {
            self.first_value = value;
        }
        self.last_value = value;
        self.sum_value += value as i64;
        self.min_value = self.min_value.min(value);
        self.max_value = self.max_value.max(value);
        self.base.update_time(timestamp);
    }

    fn update_i64(&mut self, _: i64, _: i64) {}
    fn update_f32(&mut self, _: i64, _: f32) {}
    fn update_f64(&mut self, _: i64, _: f64) {}
    fn update_string(&mut self, _: i64, _: &str) {}

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_i64::<LittleEndian>(self.sum_value)?;
        writer.write_i32::<LittleEndian>(self.min_value)?;
        writer.write_i32::<LittleEndian>(self.max_value)?;
        writer.write_i32::<LittleEndian>(self.first_value)?;
        writer.write_i32::<LittleEndian>(self.last_value)?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Int32
    }

    fn min_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Int32(self.min_value))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Int32(self.max_value))
        } else {
            None
        }
    }
}

/// Statistical metadata for 64-bit integer time-series data.
///
/// Tracks min, max, sum, first, and last values along with temporal metadata.
/// The sum is stored as f64 to prevent overflow when accumulating many large i64 values.
///
/// # Overflow handling
///
/// Since i64 sums can overflow even in i64 storage, the sum is accumulated as f64.
/// This sacrifices some precision (f64 has 53 bits of mantissa vs 64 bits in i64)
/// but prevents overflow for typical workloads and matches TsFile specification behavior.
#[derive(Debug, Clone)]
pub struct Int64Statistic {
    base: BaseStats,
    /// Sum of all values (f64 to prevent overflow, with slight precision loss)
    sum_value: f64,
    /// Minimum value observed
    min_value: i64,
    /// Maximum value observed
    max_value: i64,
    /// First value observed
    first_value: i64,
    /// Last value observed
    last_value: i64,
}

impl Int64Statistic {
    /// Creates a new `Int64Statistic` with initial values.
    ///
    /// Min and max are initialized to extreme values (i64::MAX, i64::MIN) to ensure
    /// the first update correctly establishes bounds.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            sum_value: 0.0,
            min_value: i64::MAX,
            max_value: i64::MIN,
            first_value: 0,
            last_value: 0,
        }
    }
}

impl Default for Int64Statistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for Int64Statistic {
    fn update_bool(&mut self, _: i64, _: bool) {}
    fn update_i32(&mut self, _: i64, _: i32) {}

    fn update_i64(&mut self, timestamp: i64, value: i64) {
        if self.base.count == 0 {
            self.first_value = value;
        }
        self.last_value = value;
        self.sum_value += value as f64;
        self.min_value = self.min_value.min(value);
        self.max_value = self.max_value.max(value);
        self.base.update_time(timestamp);
    }

    fn update_f32(&mut self, _: i64, _: f32) {}
    fn update_f64(&mut self, _: i64, _: f64) {}
    fn update_string(&mut self, _: i64, _: &str) {}

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_f64::<LittleEndian>(self.sum_value)?;
        writer.write_i64::<LittleEndian>(self.min_value)?;
        writer.write_i64::<LittleEndian>(self.max_value)?;
        writer.write_i64::<LittleEndian>(self.first_value)?;
        writer.write_i64::<LittleEndian>(self.last_value)?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Int64
    }

    fn min_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Int64(self.min_value))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Int64(self.max_value))
        } else {
            None
        }
    }
}

/// Statistical metadata for 32-bit floating-point time-series data.
///
/// Tracks min, max, sum, first, and last values along with temporal metadata.
/// The sum is stored as f64 to improve precision when accumulating many f32 values.
///
/// # Precision
///
/// Accumulating many f32 values in f32 can lead to significant rounding errors.
/// Using f64 for the sum provides better precision for typical workloads while
/// maintaining compatibility with the TsFile specification.
///
/// # Special values
///
/// NaN and infinity values are compared using normal floating-point comparison,
/// which may produce unexpected results (NaN < x is always false). This matches
/// the TsFile specification behavior.
#[derive(Debug, Clone)]
pub struct FloatStatistic {
    base: BaseStats,
    /// Sum of all values (f64 for better accumulation precision)
    sum_value: f64,
    /// Minimum value observed
    min_value: f32,
    /// Maximum value observed
    max_value: f32,
    /// First value observed
    first_value: f32,
    /// Last value observed
    last_value: f32,
}

impl FloatStatistic {
    /// Creates a new `FloatStatistic` with initial values.
    ///
    /// Min and max are initialized to extreme values (f32::MAX, f32::MIN) to ensure
    /// the first update correctly establishes bounds.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            sum_value: 0.0,
            min_value: f32::MAX,
            max_value: f32::MIN,
            first_value: 0.0,
            last_value: 0.0,
        }
    }
}

impl Default for FloatStatistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for FloatStatistic {
    fn update_bool(&mut self, _: i64, _: bool) {}
    fn update_i32(&mut self, _: i64, _: i32) {}
    fn update_i64(&mut self, _: i64, _: i64) {}

    fn update_f32(&mut self, timestamp: i64, value: f32) {
        if self.base.count == 0 {
            self.first_value = value;
        }
        self.last_value = value;
        self.sum_value += value as f64;
        if value < self.min_value {
            self.min_value = value;
        }
        if value > self.max_value {
            self.max_value = value;
        }
        self.base.update_time(timestamp);
    }

    fn update_f64(&mut self, _: i64, _: f64) {}
    fn update_string(&mut self, _: i64, _: &str) {}

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_f64::<LittleEndian>(self.sum_value)?;
        writer.write_f32::<LittleEndian>(self.min_value)?;
        writer.write_f32::<LittleEndian>(self.max_value)?;
        writer.write_f32::<LittleEndian>(self.first_value)?;
        writer.write_f32::<LittleEndian>(self.last_value)?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Float
    }

    fn min_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Float(self.min_value))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Float(self.max_value))
        } else {
            None
        }
    }
}

/// Statistical metadata for 64-bit floating-point time-series data.
///
/// Tracks min, max, sum, first, and last values along with temporal metadata.
/// All values are stored as f64.
///
/// # Special values
///
/// NaN and infinity values are compared using normal floating-point comparison,
/// which may produce unexpected results (NaN < x is always false). This matches
/// the TsFile specification behavior.
#[derive(Debug, Clone)]
pub struct DoubleStatistic {
    base: BaseStats,
    /// Sum of all values
    sum_value: f64,
    /// Minimum value observed
    min_value: f64,
    /// Maximum value observed
    max_value: f64,
    /// First value observed
    first_value: f64,
    /// Last value observed
    last_value: f64,
}

impl DoubleStatistic {
    /// Creates a new `DoubleStatistic` with initial values.
    ///
    /// Min and max are initialized to extreme values (f64::MAX, f64::MIN) to ensure
    /// the first update correctly establishes bounds.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            sum_value: 0.0,
            min_value: f64::MAX,
            max_value: f64::MIN,
            first_value: 0.0,
            last_value: 0.0,
        }
    }
}

impl Default for DoubleStatistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for DoubleStatistic {
    fn update_bool(&mut self, _: i64, _: bool) {}
    fn update_i32(&mut self, _: i64, _: i32) {}
    fn update_i64(&mut self, _: i64, _: i64) {}
    fn update_f32(&mut self, _: i64, _: f32) {}

    fn update_f64(&mut self, timestamp: i64, value: f64) {
        if self.base.count == 0 {
            self.first_value = value;
        }
        self.last_value = value;
        self.sum_value += value;
        if value < self.min_value {
            self.min_value = value;
        }
        if value > self.max_value {
            self.max_value = value;
        }
        self.base.update_time(timestamp);
    }

    fn update_string(&mut self, _: i64, _: &str) {}

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_f64::<LittleEndian>(self.sum_value)?;
        writer.write_f64::<LittleEndian>(self.min_value)?;
        writer.write_f64::<LittleEndian>(self.max_value)?;
        writer.write_f64::<LittleEndian>(self.first_value)?;
        writer.write_f64::<LittleEndian>(self.last_value)?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Double
    }

    fn min_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Double(self.min_value))
        } else {
            None
        }
    }

    fn max_value(&self) -> Option<TsValue> {
        if self.base.count > 0 {
            Some(TsValue::Double(self.max_value))
        } else {
            None
        }
    }
}

/// Statistical metadata for String/Text time-series data.
///
/// Tracks only first and last values along with temporal metadata.
/// Unlike numeric types, strings do not have well-defined min/max or sum operations,
/// so only boundary values (first/last) are tracked.
///
/// # Memory
///
/// String values are stored as owned `String` instances. For very long strings,
/// this may consume significant memory. The TsFile specification does not provide
/// length limits for these statistics.
#[derive(Debug, Clone)]
pub struct StringStatistic {
    base: BaseStats,
    /// First string value observed
    first_value: String,
    /// Last string value observed
    last_value: String,
}

impl StringStatistic {
    /// Creates a new `StringStatistic` with initial values.
    pub fn new() -> Self {
        Self {
            base: BaseStats::new(),
            first_value: String::new(),
            last_value: String::new(),
        }
    }
}

impl Default for StringStatistic {
    fn default() -> Self {
        Self::new()
    }
}

impl Statistic for StringStatistic {
    fn update_bool(&mut self, _: i64, _: bool) {}
    fn update_i32(&mut self, _: i64, _: i32) {}
    fn update_i64(&mut self, _: i64, _: i64) {}
    fn update_f32(&mut self, _: i64, _: f32) {}
    fn update_f64(&mut self, _: i64, _: f64) {}

    fn update_string(&mut self, timestamp: i64, value: &str) {
        if self.base.count == 0 {
            self.first_value = value.to_string();
        }
        self.last_value = value.to_string();
        self.base.update_time(timestamp);
    }

    fn count(&self) -> i32 {
        self.base.count
    }
    fn start_time(&self) -> i64 {
        self.base.start_time
    }
    fn end_time(&self) -> i64 {
        self.base.end_time
    }

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()> {
        writer.write_i32::<LittleEndian>(self.base.count)?;
        writer.write_i64::<LittleEndian>(self.base.start_time)?;
        writer.write_i64::<LittleEndian>(self.base.end_time)?;
        writer.write_i32::<LittleEndian>(self.first_value.len() as i32)?;
        writer.write_all(self.first_value.as_bytes())?;
        writer.write_i32::<LittleEndian>(self.last_value.len() as i32)?;
        writer.write_all(self.last_value.as_bytes())?;
        Ok(())
    }

    fn data_type(&self) -> TSDataType {
        TSDataType::Text
    }

    fn min_value(&self) -> Option<TsValue> {
        None // String doesn't have min/max
    }

    fn max_value(&self) -> Option<TsValue> {
        None // String doesn't have min/max
    }
}

/// Creates a statistic tracker appropriate for the given data type.
///
/// This factory function returns a boxed trait object that implements [`Statistic`]
/// with behavior tailored to the specific data type.
///
/// # Type mapping
///
/// - `Boolean` → [`BooleanStatistic`]
/// - `Int32`, `Date` → [`Int32Statistic`]
/// - `Int64`, `Timestamp` → [`Int64Statistic`]
/// - `Float` → [`FloatStatistic`]
/// - `Double` → [`DoubleStatistic`]
/// - `Text`, `String` → [`StringStatistic`]
/// - Other types → [`Int32Statistic`] (fallback)
///
/// # Examples
///
/// ```rust
/// use timbre_tsf::common::statistic::create_statistic;
/// use timbre_tsf::common::types::TSDataType;
///
/// let stat = create_statistic(TSDataType::Float);
/// // Returns a FloatStatistic wrapped in Box<dyn Statistic>
/// ```
pub fn create_statistic(data_type: TSDataType) -> Box<dyn Statistic> {
    match data_type {
        TSDataType::Boolean => Box::new(BooleanStatistic::new()),
        TSDataType::Int32 | TSDataType::Date => Box::new(Int32Statistic::new()),
        TSDataType::Int64 | TSDataType::Timestamp => Box::new(Int64Statistic::new()),
        TSDataType::Float => Box::new(FloatStatistic::new()),
        TSDataType::Double => Box::new(DoubleStatistic::new()),
        TSDataType::Text | TSDataType::String => Box::new(StringStatistic::new()),
        _ => Box::new(Int32Statistic::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int32_statistic() {
        let mut stat = Int32Statistic::new();
        stat.update_i32(1000, 10);
        stat.update_i32(2000, 20);
        stat.update_i32(3000, 5);

        assert_eq!(stat.count(), 3);
        assert_eq!(stat.start_time(), 1000);
        assert_eq!(stat.end_time(), 3000);
        assert_eq!(stat.min_value, 5);
        assert_eq!(stat.max_value, 20);
        assert_eq!(stat.sum_value, 35);
    }

    #[test]
    fn test_float_statistic() {
        let mut stat = FloatStatistic::new();
        stat.update_f32(1000, 1.5);
        stat.update_f32(2000, 2.5);
        stat.update_f32(3000, 0.5);

        assert_eq!(stat.count(), 3);
        assert!((stat.sum_value - 4.5).abs() < 0.001);
        assert!((stat.min_value - 0.5).abs() < 0.001);
        assert!((stat.max_value - 2.5).abs() < 0.001);
    }
}
