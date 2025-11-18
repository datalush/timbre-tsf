use super::types::TSDataType;
use crate::error::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use std::io::Write;

/// Trait para estadísticas de datos
pub trait Statistic: Send + Sync + std::fmt::Debug {
    fn update_bool(&mut self, timestamp: i64, value: bool);
    fn update_i32(&mut self, timestamp: i64, value: i32);
    fn update_i64(&mut self, timestamp: i64, value: i64);
    fn update_f32(&mut self, timestamp: i64, value: f32);
    fn update_f64(&mut self, timestamp: i64, value: f64);
    fn update_string(&mut self, timestamp: i64, value: &str);

    fn count(&self) -> i32;
    fn start_time(&self) -> i64;
    fn end_time(&self) -> i64;

    fn serialize_to(&self, writer: &mut dyn Write) -> Result<()>;
    fn data_type(&self) -> TSDataType;
}

/// Estadísticas base compartidas por todos los tipos
#[derive(Debug, Clone)]
pub struct BaseStats {
    pub count: i32,
    pub start_time: i64,
    pub end_time: i64,
}

impl BaseStats {
    pub fn new() -> Self {
        Self {
            count: 0,
            start_time: i64::MAX,
            end_time: i64::MIN,
        }
    }

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

/// Estadísticas para Boolean
#[derive(Debug, Clone)]
pub struct BooleanStatistic {
    base: BaseStats,
    sum_value: i64,
    first_value: bool,
    last_value: bool,
}

impl BooleanStatistic {
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
}

/// Estadísticas para Int32
#[derive(Debug, Clone)]
pub struct Int32Statistic {
    base: BaseStats,
    sum_value: i64,
    min_value: i32,
    max_value: i32,
    first_value: i32,
    last_value: i32,
}

impl Int32Statistic {
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
}

/// Estadísticas para Int64
#[derive(Debug, Clone)]
pub struct Int64Statistic {
    base: BaseStats,
    sum_value: f64, // Usa f64 para evitar overflow
    min_value: i64,
    max_value: i64,
    first_value: i64,
    last_value: i64,
}

impl Int64Statistic {
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
}

/// Estadísticas para Float
#[derive(Debug, Clone)]
pub struct FloatStatistic {
    base: BaseStats,
    sum_value: f64,
    min_value: f32,
    max_value: f32,
    first_value: f32,
    last_value: f32,
}

impl FloatStatistic {
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
}

/// Estadísticas para Double
#[derive(Debug, Clone)]
pub struct DoubleStatistic {
    base: BaseStats,
    sum_value: f64,
    min_value: f64,
    max_value: f64,
    first_value: f64,
    last_value: f64,
}

impl DoubleStatistic {
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
}

/// Estadísticas para String/Text
#[derive(Debug, Clone)]
pub struct StringStatistic {
    base: BaseStats,
    first_value: String,
    last_value: String,
}

impl StringStatistic {
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
}

/// Factory para crear estadísticas según tipo de dato
pub fn create_statistic(data_type: TSDataType) -> Box<dyn Statistic> {
    match data_type {
        TSDataType::Boolean => Box::new(BooleanStatistic::new()),
        TSDataType::Int32 | TSDataType::Date => Box::new(Int32Statistic::new()),
        TSDataType::Int64 | TSDataType::Timestamp => Box::new(Int64Statistic::new()),
        TSDataType::Float => Box::new(FloatStatistic::new()),
        TSDataType::Double => Box::new(DoubleStatistic::new()),
        TSDataType::Text | TSDataType::String => Box::new(StringStatistic::new()),
        _ => Box::new(Int32Statistic::new()), // Fallback
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
