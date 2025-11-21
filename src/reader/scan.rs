//! Fluent scan API with predicate push down
//!
//! Provides a builder pattern for scanning Timbre data with filters.

use crate::error::Result;
use crate::query::{Predicate, TimeFilter, ValueFilter};
use crate::reader::{DecodedChunk, FileReader};

/// Fluent scan builder
///
/// # Example
/// ```no_run
/// use timbre_tsf::reader::FileReader;
/// use timbre_tsf::query::ValueFilter;
/// use timbre_tsf::common::TsValue;
///
/// let mut reader = FileReader::open("data.timbre")?;
///
/// let results = reader.scan()
///     .device("sensor-001")
///     .measurement("temperature")
///     .time_range(1000, 2000)
///     .where_value(ValueFilter::GreaterThan(TsValue::Float(25.0)))
///     .execute()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct ScanBuilder<'a> {
    reader: &'a mut FileReader,
    device_id: Option<String>,
    measurement_name: Option<String>,
    predicates: Vec<Predicate>,
}

impl<'a> ScanBuilder<'a> {
    pub(crate) fn new(reader: &'a mut FileReader) -> Self {
        Self {
            reader,
            device_id: None,
            measurement_name: None,
            predicates: Vec::new(),
        }
    }

    /// Set device ID to scan
    pub fn device(mut self, device_id: impl Into<String>) -> Self {
        self.device_id = Some(device_id.into());
        self
    }

    /// Set measurement name to scan
    pub fn measurement(mut self, measurement_name: impl Into<String>) -> Self {
        self.measurement_name = Some(measurement_name.into());
        self
    }

    /// Add time range filter
    pub fn time_range(mut self, min_time: i64, max_time: i64) -> Self {
        self.predicates
            .push(Predicate::Time(TimeFilter::Between(min_time, max_time)));
        self
    }

    /// Add value filter on the measurement
    pub fn where_value(mut self, filter: ValueFilter) -> Self {
        let measurement = self
            .measurement_name
            .clone()
            .expect("Must call .measurement() before .where_value()");
        self.predicates.push(Predicate::Value(measurement, filter));
        self
    }

    /// Add custom predicate
    pub fn where_predicate(mut self, predicate: Predicate) -> Self {
        self.predicates.push(predicate);
        self
    }

    /// Execute the scan with predicate push down
    pub fn execute(self) -> Result<DecodedChunk> {
        let device_id = self.device_id.ok_or_else(|| {
            crate::error::TimbreError::InvalidState("Device ID not specified".to_string())
        })?;

        let measurement_name = self.measurement_name.ok_or_else(|| {
            crate::error::TimbreError::InvalidState("Measurement name not specified".to_string())
        })?;

        // Combine all predicates with AND
        let predicate = if self.predicates.is_empty() {
            // No predicates - read all data
            return self.reader.read(&device_id, &measurement_name).cloned();
        } else if self.predicates.len() == 1 {
            self.predicates.into_iter().next().unwrap()
        } else {
            Predicate::And(self.predicates)
        };

        // Simplify predicate tree for better performance
        let predicate = predicate.simplify();

        // Execute scan with predicate push down
        self.reader
            .scan_with_predicate(&device_id, &measurement_name, predicate)
    }
}

impl FileReader {
    /// Start building a scan with fluent API
    ///
    /// # Example
    /// ```no_run
    /// use timbre_tsf::reader::FileReader;
    /// use timbre_tsf::query::ValueFilter;
    /// use timbre_tsf::common::TsValue;
    ///
    /// let mut reader = FileReader::open("data.timbre")?;
    ///
    /// let chunk = reader.scan()
    ///     .device("device1")
    ///     .measurement("temp")
    ///     .time_range(1000, 2000)
    ///     .where_value(ValueFilter::GreaterThan(TsValue::Float(25.0)))
    ///     .execute()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn scan(&mut self) -> ScanBuilder<'_> {
        ScanBuilder::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::{
        CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue,
    };
    use crate::writer::FileWriter;
    use tempfile::NamedTempFile;

    #[test]
    fn test_scan_builder_basic() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        {
            let mut writer = FileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "temp",
                TSDataType::Float,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..20 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("temp", TsValue::Float(20.0 + i as f32));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Scan with fluent API
        let mut reader = FileReader::open(path).unwrap();

        let result = reader
            .scan()
            .device("device1")
            .measurement("temp")
            .time_range(500, 1500)
            .execute()
            .unwrap();

        // Should filter to timestamps [500, 1500]
        assert!(result.len() > 0);
        assert!(result.len() <= 11); // 500-1500 with step 100
    }

    #[test]
    fn test_scan_builder_with_value_filter() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        {
            let mut writer = FileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "sensor",
                TSDataType::Int32,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..30 {
                let record = TsRecord::new(i * 100, "device1")
                    .with_value("sensor", TsValue::Int32(i as i32));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Scan with value filter
        let mut reader = FileReader::open(path).unwrap();

        let result = reader
            .scan()
            .device("device1")
            .measurement("sensor")
            .where_value(ValueFilter::GreaterThan(TsValue::Int32(20)))
            .execute()
            .unwrap();

        // Should have values > 20
        assert!(result.len() > 0);
        assert!(result.len() < 30);
    }

    #[test]
    fn test_scan_builder_combined_filters() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        // Write test data
        {
            let mut writer = FileWriter::new(path).unwrap();
            let schema = MeasurementSchema::new(
                "data",
                TSDataType::Double,
                TSEncoding::Plain,
                CompressionType::Uncompressed,
            );
            writer.register_timeseries("device1", schema).unwrap();

            for i in 0..50 {
                let record = TsRecord::new(i * 10, "device1")
                    .with_value("data", TsValue::Double(i as f64 * 2.0));
                writer.write_record(record).unwrap();
            }
            writer.close().unwrap();
        }

        // Scan with combined filters
        let mut reader = FileReader::open(path).unwrap();

        let result = reader
            .scan()
            .device("device1")
            .measurement("data")
            .time_range(100, 300)
            .where_value(ValueFilter::LessThan(TsValue::Double(50.0)))
            .execute()
            .unwrap();

        // Should have both time and value filters applied
        assert!(result.len() > 0);
    }
}
