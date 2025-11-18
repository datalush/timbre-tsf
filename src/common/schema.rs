use super::types::{ColumnCategory, CompressionType, TSDataType, TSEncoding};
use std::collections::HashMap;

/// Schema de una medición/serie temporal
#[derive(Debug, Clone)]
pub struct MeasurementSchema {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub encoding: TSEncoding,
    pub compression: CompressionType,
    pub props: HashMap<String, String>,
}

impl MeasurementSchema {
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

    /// Crea un schema con configuración recomendada para el tipo de dato
    pub fn with_defaults(measurement_name: impl Into<String>, data_type: TSDataType) -> Self {
        Self::new(
            measurement_name,
            data_type,
            TSEncoding::recommended_for(data_type),
            CompressionType::recommended_for(data_type),
        )
    }

    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.props.insert(key.into(), value.into());
        self
    }
}

/// Schema de una tabla (modelo tabla)
#[derive(Debug, Clone)]
pub struct TableSchema {
    pub table_name: String,
    pub column_schemas: Vec<MeasurementSchema>,
    pub column_categories: Vec<ColumnCategory>,
    column_index: HashMap<String, usize>,
    /// Position indices for O(1) lookup of tag columns by name
    tag_indices: HashMap<String, usize>,
    /// Position indices for O(1) lookup of field columns by name
    field_indices: HashMap<String, usize>,
    /// Index of time column if present
    time_column_index: Option<usize>,
}

impl TableSchema {
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

    pub fn get_column_index(&self, name: &str) -> Option<usize> {
        self.column_index.get(name).copied()
    }

    pub fn get_column_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.get_column_index(name)
            .and_then(|idx| self.column_schemas.get(idx))
    }

    pub fn column_count(&self) -> usize {
        self.column_schemas.len()
    }

    /// Get tag column schema by name (O(1) lookup)
    pub fn get_tag_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.tag_indices
            .get(name)
            .and_then(|&idx| self.column_schemas.get(idx))
    }

    /// Get field column schema by name (O(1) lookup)
    pub fn get_field_schema(&self, name: &str) -> Option<&MeasurementSchema> {
        self.field_indices
            .get(name)
            .and_then(|&idx| self.column_schemas.get(idx))
    }

    /// Get all tag column indices
    pub fn tag_indices(&self) -> &HashMap<String, usize> {
        &self.tag_indices
    }

    /// Get all field column indices
    pub fn field_indices(&self) -> &HashMap<String, usize> {
        &self.field_indices
    }

    /// Get time column index
    pub fn time_column_index(&self) -> Option<usize> {
        self.time_column_index
    }

    /// Get number of tag columns
    pub fn tag_count(&self) -> usize {
        self.tag_indices.len()
    }

    /// Get number of field columns
    pub fn field_count(&self) -> usize {
        self.field_indices.len()
    }

    /// Validate schema: at least one FIELD column required
    pub fn validate(&self) -> Result<(), String> {
        if self.field_indices.is_empty() {
            return Err("TableSchema must have at least one FIELD column".to_string());
        }
        Ok(())
    }

    /// Get all tag column names
    pub fn tag_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.tag_indices.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Get all field column names
    pub fn field_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.field_indices.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Check if a column is a tag column
    pub fn is_tag_column(&self, name: &str) -> bool {
        self.tag_indices.contains_key(name)
    }

    /// Check if a column is a field column
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
