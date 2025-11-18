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
}

impl TableSchema {
    pub fn new(
        table_name: impl Into<String>,
        columns: Vec<(MeasurementSchema, ColumnCategory)>,
    ) -> Self {
        let mut column_schemas = Vec::with_capacity(columns.len());
        let mut column_categories = Vec::with_capacity(columns.len());
        let mut column_index = HashMap::with_capacity(columns.len());

        for (idx, (schema, category)) in columns.into_iter().enumerate() {
            column_index.insert(schema.measurement_name.clone(), idx);
            column_schemas.push(schema);
            column_categories.push(category);
        }

        Self {
            table_name: table_name.into(),
            column_schemas,
            column_categories,
            column_index,
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
}
