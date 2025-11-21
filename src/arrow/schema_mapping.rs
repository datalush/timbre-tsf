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

//! Schema mapping utilities for Arrow ↔ Timbre conversion

use crate::common::{MeasurementSchema, TSDataType, TSEncoding};
use crate::error::{Result, TimbreError};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
#[cfg(test)]
use std::sync::Arc;

/// Utilities for mapping between Arrow and Timbre schemas
pub struct ArrowSchemaMapping;

impl ArrowSchemaMapping {
    /// Convert an Arrow Schema to a list of Timbre MeasurementSchemas
    ///
    /// # Arguments
    ///
    /// * `arrow_schema` - The Arrow schema to convert
    /// * `timestamp_column` - Name of the timestamp column (will be skipped)
    /// * `device_column` - Optional name of the device column (will be skipped)
    ///
    /// # Returns
    ///
    /// A vector of (field_name, MeasurementSchema) tuples
    pub fn arrow_to_timbre_schemas(
        arrow_schema: &Schema,
        timestamp_column: &str,
        device_column: Option<&str>,
    ) -> Result<Vec<(String, MeasurementSchema)>> {
        let mut schemas = Vec::new();

        for field in arrow_schema.fields() {
            let field_name = field.name();

            // Skip timestamp and device columns
            if field_name == timestamp_column || Some(field_name.as_str()) == device_column {
                continue;
            }

            let ts_data_type = arrow_type_to_timbre(field.data_type())?;
            let encoding = Self::select_default_encoding(&ts_data_type, field);

            let schema = MeasurementSchema::new(
                field_name.as_str(),
                ts_data_type,
                encoding,
                crate::common::CompressionType::Lz4,
            );

            schemas.push((field_name.clone(), schema));
        }

        Ok(schemas)
    }

    /// Convert Timbre schemas to an Arrow Schema
    ///
    /// # Arguments
    ///
    /// * `timbre_schemas` - List of (measurement_name, MeasurementSchema)
    /// * `include_timestamp` - Whether to include timestamp column
    /// * `include_device` - Whether to include device ID column
    pub fn timbre_to_arrow_schema(
        timbre_schemas: &[(String, MeasurementSchema)],
        include_timestamp: bool,
        include_device: bool,
    ) -> Result<Schema> {
        let mut fields = Vec::new();

        // Add timestamp column if requested
        if include_timestamp {
            fields.push(Field::new(
                "timestamp",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ));
        }

        // Add device column if requested
        if include_device {
            fields.push(Field::new("device_id", DataType::Utf8, false));
        }

        // Add measurement fields
        for (name, schema) in timbre_schemas {
            let arrow_type = timbre_type_to_arrow(&schema.data_type)?;
            fields.push(Field::new(name.as_str(), arrow_type, true));
        }

        Ok(Schema::new(fields))
    }

    /// Select a default encoding for a Timbre measurement based on Arrow field metadata
    fn select_default_encoding(ts_data_type: &TSDataType, _field: &Field) -> TSEncoding {
        // TODO: Support encoding hints from Arrow field metadata
        // For now, just use default encoding based on data type

        // Default encoding based on data type
        match ts_data_type {
            TSDataType::Boolean => TSEncoding::Rle,
            TSDataType::Int32 | TSDataType::Int64 => TSEncoding::DeltaOfDelta,
            TSDataType::Float | TSDataType::Double => TSEncoding::Gorilla,
            TSDataType::Text => TSEncoding::Dictionary,
            _ => TSEncoding::Plain, // Default for unsupported types
        }
    }
}

/// Convert Arrow DataType to Timbre TSDataType
pub fn arrow_type_to_timbre(arrow_type: &DataType) -> Result<TSDataType> {
    match arrow_type {
        DataType::Boolean => Ok(TSDataType::Boolean),
        DataType::Int32 => Ok(TSDataType::Int32),
        DataType::Int64 => Ok(TSDataType::Int64),
        DataType::Float32 => Ok(TSDataType::Float),
        DataType::Float64 => Ok(TSDataType::Double),
        DataType::Utf8 | DataType::LargeUtf8 => Ok(TSDataType::Text),
        DataType::Timestamp(_, _) => Ok(TSDataType::Int64), // Timestamps as int64
        DataType::Date32 | DataType::Date64 => Ok(TSDataType::Int64),
        DataType::Int8 | DataType::Int16 => Ok(TSDataType::Int32), // Promote to Int32
        DataType::UInt8 | DataType::UInt16 | DataType::UInt32 => Ok(TSDataType::Int32),
        DataType::UInt64 => Ok(TSDataType::Int64),
        DataType::Float16 => Ok(TSDataType::Float), // Promote to Float32
        _ => Err(TimbreError::NotImplemented(format!(
            "Arrow type {:?} is not supported for conversion to Timbre",
            arrow_type
        ))),
    }
}

/// Convert Timbre TSDataType to Arrow DataType
pub fn timbre_type_to_arrow(ts_type: &TSDataType) -> Result<DataType> {
    match ts_type {
        TSDataType::Boolean => Ok(DataType::Boolean),
        TSDataType::Int32 => Ok(DataType::Int32),
        TSDataType::Int64 => Ok(DataType::Int64),
        TSDataType::Float => Ok(DataType::Float32),
        TSDataType::Double => Ok(DataType::Float64),
        TSDataType::Text => Ok(DataType::Utf8),
        _ => Err(TimbreError::NotImplemented(format!(
            "Timbre type {:?} is not supported for conversion to Arrow",
            ts_type
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arrow_to_timbre_basic_types() {
        assert_eq!(
            arrow_type_to_timbre(&DataType::Boolean).unwrap(),
            TSDataType::Boolean
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Int32).unwrap(),
            TSDataType::Int32
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Int64).unwrap(),
            TSDataType::Int64
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Float32).unwrap(),
            TSDataType::Float
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Float64).unwrap(),
            TSDataType::Double
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Utf8).unwrap(),
            TSDataType::Text
        );
    }

    #[test]
    fn test_timbre_to_arrow_basic_types() {
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Boolean).unwrap(),
            DataType::Boolean
        );
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Int32).unwrap(),
            DataType::Int32
        );
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Int64).unwrap(),
            DataType::Int64
        );
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Float).unwrap(),
            DataType::Float32
        );
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Double).unwrap(),
            DataType::Float64
        );
        assert_eq!(
            timbre_type_to_arrow(&TSDataType::Text).unwrap(),
            DataType::Utf8
        );
    }

    #[test]
    fn test_arrow_to_timbre_promoted_types() {
        // Smaller integer types promoted to Int32
        assert_eq!(
            arrow_type_to_timbre(&DataType::Int8).unwrap(),
            TSDataType::Int32
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Int16).unwrap(),
            TSDataType::Int32
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::UInt8).unwrap(),
            TSDataType::Int32
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::UInt16).unwrap(),
            TSDataType::Int32
        );

        // UInt32 promoted to Int32 (may lose range)
        assert_eq!(
            arrow_type_to_timbre(&DataType::UInt32).unwrap(),
            TSDataType::Int32
        );

        // UInt64 mapped to Int64
        assert_eq!(
            arrow_type_to_timbre(&DataType::UInt64).unwrap(),
            TSDataType::Int64
        );

        // Float16 promoted to Float32
        assert_eq!(
            arrow_type_to_timbre(&DataType::Float16).unwrap(),
            TSDataType::Float
        );
    }

    #[test]
    fn test_arrow_to_timbre_timestamp() {
        assert_eq!(
            arrow_type_to_timbre(&DataType::Timestamp(TimeUnit::Millisecond, None)).unwrap(),
            TSDataType::Int64
        );
        assert_eq!(
            arrow_type_to_timbre(&DataType::Timestamp(TimeUnit::Microsecond, None)).unwrap(),
            TSDataType::Int64
        );
    }

    #[test]
    fn test_arrow_to_timbre_unsupported() {
        // Complex types should fail
        assert!(
            arrow_type_to_timbre(&DataType::List(Arc::new(Field::new(
                "item",
                DataType::Int32,
                true
            ))))
            .is_err()
        );
        let fields: Vec<Arc<Field>> = vec![];
        assert!(arrow_type_to_timbre(&DataType::Struct(fields.into())).is_err());
    }

    #[test]
    fn test_timbre_to_arrow_schema() {
        let timbre_schemas = vec![
            (
                "temperature".to_string(),
                MeasurementSchema::new(
                    "temperature",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    crate::common::CompressionType::Lz4,
                ),
            ),
            (
                "humidity".to_string(),
                MeasurementSchema::new(
                    "humidity",
                    TSDataType::Int32,
                    TSEncoding::DeltaOfDelta,
                    crate::common::CompressionType::Lz4,
                ),
            ),
        ];

        let schema =
            ArrowSchemaMapping::timbre_to_arrow_schema(&timbre_schemas, true, true).unwrap();

        assert_eq!(schema.fields().len(), 4); // timestamp + device_id + 2 measurements
        assert_eq!(schema.field(0).name(), "timestamp");
        assert_eq!(schema.field(1).name(), "device_id");
        assert_eq!(schema.field(2).name(), "temperature");
        assert_eq!(schema.field(3).name(), "humidity");
    }

    #[test]
    fn test_arrow_to_timbre_schemas() {
        let fields = vec![
            Field::new(
                "timestamp",
                DataType::Timestamp(TimeUnit::Millisecond, None),
                false,
            ),
            Field::new("device_id", DataType::Utf8, false),
            Field::new("temperature", DataType::Float32, true),
            Field::new("humidity", DataType::Int32, true),
        ];
        let arrow_schema = Schema::new(fields);

        let timbre_schemas = ArrowSchemaMapping::arrow_to_timbre_schemas(
            &arrow_schema,
            "timestamp",
            Some("device_id"),
        )
        .unwrap();

        assert_eq!(timbre_schemas.len(), 2); // Only measurement columns
        assert_eq!(timbre_schemas[0].0, "temperature");
        assert_eq!(timbre_schemas[0].1.data_type, TSDataType::Float);
        assert_eq!(timbre_schemas[1].0, "humidity");
        assert_eq!(timbre_schemas[1].1.data_type, TSDataType::Int32);
    }

    #[test]
    fn test_default_encoding_selection() {
        let field_bool = Field::new("flag", DataType::Boolean, true);
        let field_int = Field::new("count", DataType::Int32, true);
        let field_float = Field::new("temp", DataType::Float32, true);
        let field_string = Field::new("name", DataType::Utf8, true);

        assert_eq!(
            ArrowSchemaMapping::select_default_encoding(&TSDataType::Boolean, &field_bool),
            TSEncoding::Rle
        );
        assert_eq!(
            ArrowSchemaMapping::select_default_encoding(&TSDataType::Int32, &field_int),
            TSEncoding::DeltaOfDelta
        );
        assert_eq!(
            ArrowSchemaMapping::select_default_encoding(&TSDataType::Float, &field_float),
            TSEncoding::Gorilla
        );
        assert_eq!(
            ArrowSchemaMapping::select_default_encoding(&TSDataType::Text, &field_string),
            TSEncoding::Dictionary
        );
    }
}
