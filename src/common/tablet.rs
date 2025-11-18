use super::schema::MeasurementSchema;
use super::types::{ColumnCategory, TSDataType, TsValue};
use crate::error::{Result, TsFileError};
use std::sync::Arc;

/// BitMap para rastrear valores nulos
#[derive(Debug, Clone)]
pub struct BitMap {
    bits: Vec<u8>,
    size: usize,
}

impl BitMap {
    pub fn new(size: usize) -> Self {
        let byte_count = (size + 7) / 8;
        Self {
            bits: vec![0; byte_count],
            size,
        }
    }

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

    pub fn get(&self, index: usize) -> bool {
        if index >= self.size {
            return false;
        }
        let byte_idx = index / 8;
        let bit_idx = index % 8;
        (self.bits[byte_idx] & (1 << bit_idx)) != 0
    }

    pub fn is_all_not_null(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }
}

/// Matriz de valores para Tablet
#[derive(Debug, Clone)]
pub enum ValueMatrix {
    Boolean(Vec<bool>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    Text(Vec<String>),
}

impl ValueMatrix {
    pub fn new(data_type: TSDataType, capacity: usize) -> Self {
        match data_type {
            TSDataType::Boolean => Self::Boolean(Vec::with_capacity(capacity)),
            TSDataType::Int32 | TSDataType::Date => Self::Int32(Vec::with_capacity(capacity)),
            TSDataType::Int64 | TSDataType::Timestamp => {
                Self::Int64(Vec::with_capacity(capacity))
            }
            TSDataType::Float => Self::Float(Vec::with_capacity(capacity)),
            TSDataType::Double => Self::Double(Vec::with_capacity(capacity)),
            TSDataType::Text | TSDataType::String => Self::Text(Vec::with_capacity(capacity)),
            _ => Self::Int32(Vec::with_capacity(capacity)), // Fallback
        }
    }

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

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Tablet para escritura eficiente por lotes
#[derive(Debug, Clone)]
pub struct Tablet {
    pub device_name: String,
    pub schemas: Arc<Vec<MeasurementSchema>>,
    pub column_categories: Vec<ColumnCategory>,
    pub timestamps: Vec<i64>,
    pub values: Vec<ValueMatrix>,
    pub bitmaps: Vec<BitMap>,
    pub max_rows: usize,
}

impl Tablet {
    pub fn new(
        device_name: impl Into<String>,
        schemas: Vec<MeasurementSchema>,
        column_categories: Vec<ColumnCategory>,
        max_rows: usize,
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
        }
    }

    pub fn row_count(&self) -> usize {
        self.timestamps.len()
    }

    pub fn column_count(&self) -> usize {
        self.schemas.len()
    }

    pub fn is_full(&self) -> bool {
        self.row_count() >= self.max_rows
    }

    pub fn add_row(
        &mut self,
        timestamp: i64,
        values: Vec<Option<TsValue>>,
    ) -> Result<()> {
        if self.is_full() {
            return Err(TsFileError::InvalidState(
                "Tablet is full".to_string(),
            ));
        }

        if values.len() != self.column_count() {
            return Err(TsFileError::InvalidState(format!(
                "Expected {} values, got {}",
                self.column_count(),
                values.len()
            )));
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
                    // Agregar valor por defecto para mantener alineación
                    self.add_default_value(col_idx)?;
                }
            }
        }

        Ok(())
    }

    fn add_value(&mut self, col_idx: usize, value: TsValue) -> Result<()> {
        let expected_type = self.schemas[col_idx].data_type;
        let actual_type = value.data_type();
        match (&mut self.values[col_idx], value) {
            (ValueMatrix::Boolean(v), TsValue::Boolean(val)) => v.push(val),
            (ValueMatrix::Int32(v), TsValue::Int32(val)) => v.push(val),
            (ValueMatrix::Int64(v), TsValue::Int64(val)) => v.push(val),
            (ValueMatrix::Float(v), TsValue::Float(val)) => v.push(val),
            (ValueMatrix::Double(v), TsValue::Double(val)) => v.push(val),
            (ValueMatrix::Text(v), TsValue::Text(val) | TsValue::String(val)) => v.push(val),
            _ => {
                return Err(TsFileError::TypeMismatch {
                    expected: expected_type.to_string(),
                    actual: actual_type.to_string(),
                })
            }
        }
        Ok(())
    }

    fn add_default_value(&mut self, col_idx: usize) -> Result<()> {
        match &mut self.values[col_idx] {
            ValueMatrix::Boolean(v) => v.push(false),
            ValueMatrix::Int32(v) => v.push(0),
            ValueMatrix::Int64(v) => v.push(0),
            ValueMatrix::Float(v) => v.push(0.0),
            ValueMatrix::Double(v) => v.push(0.0),
            ValueMatrix::Text(v) => v.push(String::new()),
        }
        Ok(())
    }

    pub fn clear(&mut self) {
        self.timestamps.clear();
        for value_vec in &mut self.values {
            match value_vec {
                ValueMatrix::Boolean(v) => v.clear(),
                ValueMatrix::Int32(v) => v.clear(),
                ValueMatrix::Int64(v) => v.clear(),
                ValueMatrix::Float(v) => v.clear(),
                ValueMatrix::Double(v) => v.clear(),
                ValueMatrix::Text(v) => v.clear(),
            }
        }
        for bitmap in &mut self.bitmaps {
            *bitmap = BitMap::new(self.max_rows);
        }
    }
}

/// Punto de datos individual
#[derive(Debug, Clone)]
pub struct DataPoint {
    pub measurement_name: String,
    pub value: Option<TsValue>,
}

impl DataPoint {
    pub fn new(measurement_name: impl Into<String>, value: TsValue) -> Self {
        Self {
            measurement_name: measurement_name.into(),
            value: Some(value),
        }
    }

    pub fn null(measurement_name: impl Into<String>) -> Self {
        Self {
            measurement_name: measurement_name.into(),
            value: None,
        }
    }
}

/// Registro individual de serie temporal
#[derive(Debug, Clone)]
pub struct TsRecord {
    pub timestamp: i64,
    pub device_id: String,
    pub points: Vec<DataPoint>,
}

impl TsRecord {
    pub fn new(timestamp: i64, device_id: impl Into<String>) -> Self {
        Self {
            timestamp,
            device_id: device_id.into(),
            points: Vec::new(),
        }
    }

    pub fn add_point(mut self, point: DataPoint) -> Self {
        self.points.push(point);
        self
    }

    pub fn with_value(
        mut self,
        measurement_name: impl Into<String>,
        value: TsValue,
    ) -> Self {
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

        // Con valor nulo
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
}
