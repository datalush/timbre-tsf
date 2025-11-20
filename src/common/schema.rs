//! Schema definitions for measurements and tables.
//!
//! This module provides schema types that define the structure and configuration
//! of time series data. Schemas specify data types, encoding methods, compression
//! algorithms, and optional metadata properties.
//!
//! # Schema Types
//!
//! - [`MeasurementSchema`]: Defines a single measurement/metric with its type and encoding
//! - [`TableSchema`]: Defines a table with multiple columns (tags and fields) with
//!   efficient O(1) lookups by column name and category
//!
//! # Examples
//!
//! ```rust
//! use tsfile_rs::common::*;
//!
//! // Simple measurement schema with recommended settings
//! let temp_schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float);
//!
//! // Custom measurement schema with properties
//! let pressure_schema = MeasurementSchema::new(
//!     "pressure",
//!     TSDataType::Double,
//!     TSEncoding::Gorilla,
//!     CompressionType::Lz4,
//! )
//! .with_property("unit", "Pa")
//! .with_property("description", "Atmospheric pressure");
//!
//! // Table schema with tags and fields
//! let table_schema = TableSchema::new(
//!     "sensor_data",
//!     vec![
//!         (MeasurementSchema::with_defaults("device_id", TSDataType::String), ColumnCategory::Tag),
//!         (temp_schema, ColumnCategory::Field),
//!         (pressure_schema, ColumnCategory::Field),
//!     ],
//! );
//! ```

use super::types::{ColumnCategory, CompressionType, TSDataType, TSEncoding};
use std::collections::HashMap;

/// Schema definition for a single measurement or time series.
///
/// A measurement schema specifies how a particular metric should be encoded,
/// compressed, and stored. It includes the measurement name, data type, encoding
/// method, compression algorithm, and optional user-defined properties.
///
/// # Performance Considerations
///
/// The choice of encoding and compression significantly impacts both storage
/// efficiency and query performance:
///
/// - Use [`TSEncoding::Gorilla`] for floating-point sensor data
/// - Use [`TSEncoding::Ts2Diff`] for sequential integers or timestamps
/// - Use [`TSEncoding::Dictionary`] for repetitive string values
/// - Use [`CompressionType::Lz4`] for general-purpose compression
///
/// # Examples
///
/// ```rust
/// use tsfile_rs::common::*;
///
/// // Quick schema with recommended settings
/// let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float);
///
/// // Custom schema with specific encoding
/// let schema = MeasurementSchema::new(
///     "temperature",
///     TSDataType::Float,
///     TSEncoding::Gorilla,
///     CompressionType::Lz4,
/// )
/// .with_property("unit", "celsius")
/// .with_property("sensor_id", "TMP-001");
/// ```
#[derive(Debug, Clone)]
pub struct MeasurementSchema {
    /// Name of the measurement (e.g., "temperature", "pressure").
    pub measurement_name: String,
    /// Logical data type of the values.
    pub data_type: TSDataType,
    /// Encoding method to apply before compression.
    pub encoding: TSEncoding,
    /// Compression algorithm to apply after encoding.
    pub compression: CompressionType,
    /// Optional user-defined properties for metadata.
    pub props: HashMap<String, String>,
}

impl MeasurementSchema {
    /// Creates a new measurement schema with explicit configuration.
    ///
    /// # Arguments
    ///
    /// * `measurement_name` - Name of the measurement (e.g., "temperature")
    /// * `data_type` - Logical data type for values
    /// * `encoding` - Encoding method to use
    /// * `compression` - Compression algorithm to apply
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let schema = MeasurementSchema::new(
    ///     "temperature",
    ///     TSDataType::Float,
    ///     TSEncoding::Gorilla,
    ///     CompressionType::Lz4,
    /// );
    /// ```
    pub fn new(
        measurement_name: impl Into<String>,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression: CompressionType,
    ) -> Self {
        Self {
            measurement_name: measurement_name.into(),
            data_type,
            encoding,
            compression,
            props: HashMap::new(),
        }
    }

    /// Creates a schema with recommended encoding and compression for the data type.
    ///
    /// This is the recommended way to create schemas as it automatically selects
    /// optimal settings based on the data type:
    ///
    /// - Float/Double → Gorilla + LZ4
    /// - Int32/Int64/Timestamp → TS_2DIFF + LZ4
    /// - Boolean → RLE + LZ4
    /// - Text/String → Dictionary + LZ4
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float);
    /// assert_eq!(schema.encoding, TSEncoding::Gorilla);
    /// assert_eq!(schema.compression, CompressionType::Lz4);
    /// ```
    pub fn with_defaults(measurement_name: impl Into<String>, data_type: TSDataType) -> Self {
        Self::new(
            measurement_name,
            data_type,
            TSEncoding::recommended_for(data_type),
            CompressionType::recommended_for(data_type),
        )
    }

    /// Adds a user-defined property to the schema.
    ///
    /// Properties can store arbitrary metadata like units, descriptions,
    /// sensor IDs, or any other contextual information.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float)
    ///     .with_property("unit", "celsius")
    ///     .with_property("location", "room_101");
    /// ```
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }
}

/// Schema definition for a table with multiple columns.
///
/// A table schema organizes multiple measurements into a structured table format
/// with tags (dimensions/metadata) and fields (metrics/measurements). It provides
/// efficient O(1) lookups by column name and category through internal indices.
///
/// # Table Model
///
/// Tables consist of three types of columns:
///
/// - **Tags**: Metadata or dimension columns (e.g., device_id, location)
/// - **Fields**: Measurement columns (e.g., temperature, pressure)
/// - **Time**: Optional timestamp column
///
/// # Performance
///
/// The schema maintains multiple internal hash maps for O(1) lookup performance:
///
/// - `column_index`: All columns by name
/// - `tag_indices`: Tag columns only
/// - `field_indices`: Field columns only
///
/// This allows efficient filtering and access patterns common in time series queries.
///
/// # Validation
///
/// A valid table schema must have at least one field column. Use [`TableSchema::validate()`]
/// to check schema validity.
///
/// # Examples
///
/// ```rust
/// use tsfile_rs::common::*;
///
/// let schema = TableSchema::new(
///     "sensor_data",
///     vec![
///         (MeasurementSchema::with_defaults("device_id", TSDataType::String), ColumnCategory::Tag),
///         (MeasurementSchema::with_defaults("location", TSDataType::String), ColumnCategory::Tag),
///         (MeasurementSchema::with_defaults("temperature", TSDataType::Float), ColumnCategory::Field),
///         (MeasurementSchema::with_defaults("humidity", TSDataType::Int32), ColumnCategory::Field),
///     ],
/// );
///
/// // Efficient lookups
/// assert_eq!(schema.tag_count(), 2);
/// assert_eq!(schema.field_count(), 2);
/// assert!(schema.get_field_schema("temperature").is_some());
/// ```
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Name of the table.
    pub table_name: String,
    /// Schemas for all columns in order.
    pub column_schemas: Vec<MeasurementSchema>,
    /// Categories for all columns (parallel to column_schemas).
    pub column_categories: Vec<ColumnCategory>,
    /// Index mapping column names to positions for O(1) lookup.
    column_index: HashMap<String, usize>,
    /// Index mapping tag column names to positions for O(1) lookup.
    tag_indices: HashMap<String, usize>,
    /// Index mapping field column names to positions for O(1) lookup.
    field_indices: HashMap<String, usize>,
    /// Position of the time column if present.
    time_column_index: Option<usize>,
}

impl TableSchema {
    /// Creates a new table schema from a list of columns with their categories.
    ///
    /// The constructor builds internal indices for efficient O(1) lookups by
    /// column name and category. Column order is preserved.
    ///
    /// # Arguments
    ///
    /// * `table_name` - Name of the table
    /// * `columns` - Vector of (schema, category) pairs defining the columns
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let schema = TableSchema::new(
    ///     "sensor_data",
    ///     vec![
    ///         (MeasurementSchema::with_defaults("device_id", TSDataType::String), ColumnCategory::Tag),
    ///         (MeasurementSchema::with_defaults("temperature", TSDataType::Float), ColumnCategory::Field),
    ///     ],
    /// );
    /// ```
    pub fn new(
        table_name: impl Into<String>,
        columns: Vec<(MeasurementSchema, ColumnCategory)>,
    ) -> Self {
        let mut column_schemas = Vec::with_capacity(columns.len());
        let mut column_categories = Vec::with_capacity(columns.len());
        let mut column_index = HashMap::with_capacity(columns.len());
        let mut tag_indices = HashMap::new();
        let mut field_indices = HashMap::new();
        let mut time_column_index = None;

        for (idx, (schema, category)) in columns.into_iter().enumerate() {
            column_index.insert(schema.measurement_name.clone(), idx);

            // Build category-specific indices
            match category {
                ColumnCategory::Tag => {
                    tag_indices.insert(schema.measurement_name.clone(), idx);
                }
                ColumnCategory::Field => {
                    field_indices.insert(schema.measurement_name.clone(), idx);
                }
                ColumnCategory::Time => {
                    time_column_index = Some(idx);
                }
            }

            column_schemas.push(schema);
            column_categories.push(category);
        }

        Self {
            table_name: table_name.into(),
            column_schemas,
            column_categories,
            column_index,
            tag_indices,
            field_indices,
            time_column_index,
        }
    }

    /// Gets the index of a column by name.
    ///
    /// Returns `None` if the column doesn't exist. Lookup is O(1).
    pub fn get_column_index(&self, name: &str) -> Option<usize> {
        self.column_index.get(name).copied()
    }

    /// Gets the schema for a column by name.
    ///
    /// Returns `None` if the column doesn't exist. Lookup is O(1).
    pub fn get_column_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.get_column_index(name)
            .and_then(|idx| self.column_schemas.get(idx))
    }

    /// Returns the total number of columns in the table.
    pub fn column_count(&self) -> usize {
        self.column_schemas.len()
    }

    /// Gets the schema for a tag column by name.
    ///
    /// Returns `None` if the column doesn't exist or is not a tag column.
    /// Lookup is O(1) as it uses the internal tag index.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let schema = TableSchema::new(
    ///     "data",
    ///     vec![
    ///         (MeasurementSchema::with_defaults("device_id", TSDataType::String), ColumnCategory::Tag),
    ///         (MeasurementSchema::with_defaults("temperature", TSDataType::Float), ColumnCategory::Field),
    ///     ],
    /// );
    ///
    /// assert!(schema.get_tag_schema("device_id").is_some());
    /// assert!(schema.get_tag_schema("temperature").is_none()); // Not a tag
    /// ```
    pub fn get_tag_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.tag_indices
            .get(name)
            .and_then(|&idx| self.column_schemas.get(idx))
    }

    /// Gets the schema for a field column by name.
    ///
    /// Returns `None` if the column doesn't exist or is not a field column.
    /// Lookup is O(1) as it uses the internal field index.
    pub fn get_field_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.field_indices
            .get(name)
            .and_then(|&idx| self.column_schemas.get(idx))
    }

    /// Returns a reference to the tag column index map.
    ///
    /// The map contains tag column names as keys and their positions as values.
    pub fn tag_indices(&self) -> &HashMap<String, usize> {
        &self.tag_indices
    }

    /// Returns a reference to the field column index map.
    ///
    /// The map contains field column names as keys and their positions as values.
    pub fn field_indices(&self) -> &HashMap<String, usize> {
        &self.field_indices
    }

    /// Returns the index of the time column if present.
    pub fn time_column_index(&self) -> Option<usize> {
        self.time_column_index
    }

    /// Returns the number of tag columns.
    pub fn tag_count(&self) -> usize {
        self.tag_indices.len()
    }

    /// Returns the number of field columns.
    pub fn field_count(&self) -> usize {
        self.field_indices.len()
    }

    /// Validates the table schema.
    ///
    /// A valid schema must have at least one field column. Returns an error
    /// if validation fails.
    ///
    /// # Errors
    ///
    /// Returns `Err` if the schema has no field columns.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tsfile_rs::common::*;
    ///
    /// let valid = TableSchema::new(
    ///     "data",
    ///     vec![(MeasurementSchema::with_defaults("temp", TSDataType::Float), ColumnCategory::Field)],
    /// );
    /// assert!(valid.validate().is_ok());
    ///
    /// let invalid = TableSchema::new(
    ///     "data",
    ///     vec![(MeasurementSchema::with_defaults("device", TSDataType::String), ColumnCategory::Tag)],
    /// );
    /// assert!(invalid.validate().is_err());
    /// ```
    pub fn validate(&self) -> Result<(), String> {
        if self.field_indices.is_empty() {
            return Err("TableSchema must have at least one FIELD column".to_string());
        }
        Ok(())
    }

    /// Returns a sorted vector of all tag column names.
    pub fn tag_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.tag_indices.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Returns a sorted vector of all field column names.
    pub fn field_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.field_indices.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Checks if a column is a tag column.
    ///
    /// Returns `true` if the column exists and is categorized as a tag.
    pub fn is_tag_column(&self, name: &str) -> bool {
        self.tag_indices.contains_key(name)
    }

    /// Checks if a column is a field column.
    ///
    /// Returns `true` if the column exists and is categorized as a field.
    pub fn is_field_column(&self, name: &str) -> bool {
        self.field_indices.contains_key(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_measurement_schema() {
        let schema = MeasurementSchema::with_defaults("temperature", TSDataType::Float)
            .with_property("unit", "celsius");

        assert_eq!(schema.measurement_name, "temperature");
        assert_eq!(schema.data_type, TSDataType::Float);
        assert_eq!(schema.encoding, TSEncoding::Gorilla);
        assert_eq!(schema.props.get("unit").unwrap(), "celsius");
    }

    #[test]
    fn test_table_schema() {
        let schema = TableSchema::new(
            "device_data",
            vec![
                (
                    MeasurementSchema::with_defaults("device_id", TSDataType::String),
                    ColumnCategory::Tag,
                ),
                (
                    MeasurementSchema::with_defaults("temperature", TSDataType::Float),
                    ColumnCategory::Field,
                ),
                (
                    MeasurementSchema::with_defaults("humidity", TSDataType::Float),
                    ColumnCategory::Field,
                ),
            ],
        );

        assert_eq!(schema.column_count(), 3);
        assert_eq!(schema.get_column_index("temperature"), Some(1));
        assert!(schema.get_column_schema("temperature").is_some());
    }

    #[test]
    fn test_table_schema_indices() {
        let schema = TableSchema::new(
            "sensor_data",
            vec![
                (
                    MeasurementSchema::with_defaults("timestamp", TSDataType::Timestamp),
                    ColumnCategory::Time,
                ),
                (
                    MeasurementSchema::with_defaults("device_id", TSDataType::String),
                    ColumnCategory::Tag,
                ),
                (
                    MeasurementSchema::with_defaults("location", TSDataType::String),
                    ColumnCategory::Tag,
                ),
                (
                    MeasurementSchema::with_defaults("temperature", TSDataType::Float),
                    ColumnCategory::Field,
                ),
                (
                    MeasurementSchema::with_defaults("humidity", TSDataType::Float),
                    ColumnCategory::Field,
                ),
                (
                    MeasurementSchema::with_defaults("pressure", TSDataType::Float),
                    ColumnCategory::Field,
                ),
            ],
        );

        // Test counts
        assert_eq!(schema.column_count(), 6);
        assert_eq!(schema.tag_count(), 2);
        assert_eq!(schema.field_count(), 3);

        // Test tag lookups
        assert!(schema.get_tag_schema("device_id").is_some());
        assert!(schema.get_tag_schema("location").is_some());
        assert!(schema.get_tag_schema("temperature").is_none());

        // Test field lookups
        assert!(schema.get_field_schema("temperature").is_some());
        assert!(schema.get_field_schema("humidity").is_some());
        assert!(schema.get_field_schema("pressure").is_some());
        assert!(schema.get_field_schema("device_id").is_none());

        // Test time column
        assert_eq!(schema.time_column_index(), Some(0));

        // Test is_tag/is_field
        assert!(schema.is_tag_column("device_id"));
        assert!(schema.is_tag_column("location"));
        assert!(!schema.is_tag_column("temperature"));

        assert!(schema.is_field_column("temperature"));
        assert!(schema.is_field_column("humidity"));
        assert!(!schema.is_field_column("device_id"));

        // Test tag/field names
        assert_eq!(schema.tag_names(), vec!["device_id", "location"]);
        assert_eq!(
            schema.field_names(),
            vec!["humidity", "pressure", "temperature"]
        );
    }

    #[test]
    fn test_table_schema_validation() {
        // Valid schema with at least one field
        let valid_schema = TableSchema::new(
            "valid",
            vec![(
                MeasurementSchema::with_defaults("temp", TSDataType::Float),
                ColumnCategory::Field,
            )],
        );
        assert!(valid_schema.validate().is_ok());

        // Invalid schema with no fields
        let invalid_schema = TableSchema::new(
            "invalid",
            vec![(
                MeasurementSchema::with_defaults("device", TSDataType::String),
                ColumnCategory::Tag,
            )],
        );
        assert!(invalid_schema.validate().is_err());
    }
}
