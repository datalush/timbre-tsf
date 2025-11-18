use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::error::Result;
use crate::file::ChunkHeader;
use crate::reader::{DecodedPage, DecodedValue, DecodedValues, PageReader};
use std::io::Read;

/// Reader para chunks (colección de páginas)
pub struct ChunkReader {
    measurement_name: String,
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,
    page_reader: PageReader,
}

impl ChunkReader {
    /// Crea un nuevo ChunkReader
    pub fn new(
        measurement_name: String,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        let page_reader = PageReader::new(data_type, encoding, compression_type);
        Self {
            measurement_name,
            data_type,
            encoding,
            compression_type,
            page_reader,
        }
    }

    /// Lee un chunk completo desde un reader
    pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> Result<DecodedChunk> {
        // Leer header del chunk
        let header = ChunkHeader::deserialize(reader)?;

        // Leer todas las páginas
        let mut all_timestamps = Vec::new();
        let mut all_values_data: Option<Vec<DecodedValueData>> = None;

        for _ in 0..header.num_of_pages {
            let page = self.page_reader.read_page(reader)?;

            // Agregar timestamps
            all_timestamps.extend_from_slice(&page.timestamps);

            // Agregar valores
            match (all_values_data.as_mut(), &page.values) {
                (None, DecodedValues::Boolean(v)) => {
                    all_values_data = Some(v.iter().map(|&x| DecodedValueData::Boolean(x)).collect());
                }
                (None, DecodedValues::Int32(v)) => {
                    all_values_data = Some(v.iter().map(|&x| DecodedValueData::Int32(x)).collect());
                }
                (None, DecodedValues::Int64(v)) => {
                    all_values_data = Some(v.iter().map(|&x| DecodedValueData::Int64(x)).collect());
                }
                (None, DecodedValues::Float(v)) => {
                    all_values_data = Some(v.iter().map(|&x| DecodedValueData::Float(x)).collect());
                }
                (None, DecodedValues::Double(v)) => {
                    all_values_data = Some(v.iter().map(|&x| DecodedValueData::Double(x)).collect());
                }
                (None, DecodedValues::Text(v)) => {
                    all_values_data = Some(v.iter().map(|x| DecodedValueData::Text(x.clone())).collect());
                }
                (Some(data), DecodedValues::Boolean(v)) => {
                    data.extend(v.iter().map(|&x| DecodedValueData::Boolean(x)));
                }
                (Some(data), DecodedValues::Int32(v)) => {
                    data.extend(v.iter().map(|&x| DecodedValueData::Int32(x)));
                }
                (Some(data), DecodedValues::Int64(v)) => {
                    data.extend(v.iter().map(|&x| DecodedValueData::Int64(x)));
                }
                (Some(data), DecodedValues::Float(v)) => {
                    data.extend(v.iter().map(|&x| DecodedValueData::Float(x)));
                }
                (Some(data), DecodedValues::Double(v)) => {
                    data.extend(v.iter().map(|&x| DecodedValueData::Double(x)));
                }
                (Some(data), DecodedValues::Text(v)) => {
                    data.extend(v.iter().map(|x| DecodedValueData::Text(x.clone())));
                }
            }
        }

        Ok(DecodedChunk {
            measurement_name: self.measurement_name.clone(),
            data_type: self.data_type,
            timestamps: all_timestamps,
            values: all_values_data.unwrap_or_default(),
        })
    }

    /// Nombre de la medición
    pub fn measurement_name(&self) -> &str {
        &self.measurement_name
    }

    /// Tipo de dato
    pub fn data_type(&self) -> TSDataType {
        self.data_type
    }
}

/// Chunk decodificado con todas las páginas agregadas
#[derive(Debug, Clone)]
pub struct DecodedChunk {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub timestamps: Vec<i64>,
    pub values: Vec<DecodedValueData>,
}

/// Valor decodificado para agregar múltiples páginas
#[derive(Debug, Clone)]
pub enum DecodedValueData {
    Boolean(bool),
    Int32(i32),
    Int64(i64),
    Float(f32),
    Double(f64),
    Text(String),
}

impl DecodedChunk {
    /// Número de puntos en el chunk
    pub fn len(&self) -> usize {
        self.timestamps.len()
    }

    /// Verifica si el chunk está vacío
    pub fn is_empty(&self) -> bool {
        self.timestamps.is_empty()
    }

    /// Obtiene un valor específico por índice
    pub fn get(&self, index: usize) -> Option<(i64, &DecodedValueData)> {
        if index >= self.len() {
            return None;
        }
        Some((self.timestamps[index], &self.values[index]))
    }

    /// Itera sobre todos los valores
    pub fn iter(&self) -> DecodedChunkIter {
        DecodedChunkIter {
            chunk: self,
            index: 0,
        }
    }

    /// Filtra valores por rango de tiempo
    pub fn filter_time_range(&self, min_time: i64, max_time: i64) -> DecodedChunk {
        let mut filtered_timestamps = Vec::new();
        let mut filtered_values = Vec::new();

        for (ts, value) in self.iter() {
            if ts >= min_time && ts <= max_time {
                filtered_timestamps.push(ts);
                filtered_values.push(value.clone());
            }
        }

        DecodedChunk {
            measurement_name: self.measurement_name.clone(),
            data_type: self.data_type,
            timestamps: filtered_timestamps,
            values: filtered_values,
        }
    }
}

/// Iterador para DecodedChunk
pub struct DecodedChunkIter<'a> {
    chunk: &'a DecodedChunk,
    index: usize,
}

impl<'a> Iterator for DecodedChunkIter<'a> {
    type Item = (i64, &'a DecodedValueData);

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.chunk.get(self.index);
        self.index += 1;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::ChunkWriter;

    #[test]
    fn test_chunk_reader_single_page() {
        // Escribir chunk
        let mut writer = ChunkWriter::new(
            "temperature".to_string(),
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        for i in 0..10 {
            writer.write_f32(1000 + i * 100, 25.0 + i as f32).unwrap();
        }

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Leer chunk
        let mut reader = ChunkReader::new(
            "temperature".to_string(),
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = reader.read_chunk(&mut cursor).unwrap();

        assert_eq!(decoded.len(), 10);
        assert_eq!(decoded.measurement_name, "temperature");

        // Verificar valores
        for (i, (ts, value)) in decoded.iter().enumerate() {
            assert_eq!(ts, 1000 + i as i64 * 100);
            if let DecodedValueData::Float(v) = value {
                assert_eq!(*v, 25.0 + i as f32);
            } else {
                panic!("Expected Float value");
            }
        }
    }

    #[test]
    fn test_chunk_reader_multiple_pages() {
        // Escribir chunk con múltiples páginas (tamaño pequeño para forzar múltiples páginas)
        let mut writer = ChunkWriter::with_page_size(
            "sensor".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
            100, // Tamaño muy pequeño
        );

        for i in 0..50 {
            writer.write_i32(1000 + i * 10, i as i32 * 5).unwrap();
        }

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Leer chunk
        let mut reader = ChunkReader::new(
            "sensor".to_string(),
            TSDataType::Int32,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = reader.read_chunk(&mut cursor).unwrap();

        assert_eq!(decoded.len(), 50);

        // Verificar que todos los valores están presentes
        for (i, (ts, value)) in decoded.iter().enumerate() {
            assert_eq!(ts, 1000 + i as i64 * 10);
            if let DecodedValueData::Int32(v) = value {
                assert_eq!(*v, i as i32 * 5);
            } else {
                panic!("Expected Int32 value");
            }
        }
    }

    #[test]
    fn test_chunk_filter_time_range() {
        // Escribir chunk
        let mut writer = ChunkWriter::new(
            "data".to_string(),
            TSDataType::Int64,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        for i in 0..20 {
            writer.write_i64(i * 100, i).unwrap();
        }

        let mut buffer = Vec::new();
        writer.serialize_to(&mut buffer).unwrap();

        // Leer chunk
        let mut reader = ChunkReader::new(
            "data".to_string(),
            TSDataType::Int64,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let mut cursor = std::io::Cursor::new(buffer);
        let decoded = reader.read_chunk(&mut cursor).unwrap();

        // Filtrar por rango de tiempo [500, 1500]
        let filtered = decoded.filter_time_range(500, 1500);

        // Debería tener valores de timestamp 500 a 1500 (5 a 15 inclusive)
        assert_eq!(filtered.len(), 11); // 500, 600, 700, ..., 1500

        for (i, (ts, _)) in filtered.iter().enumerate() {
            assert_eq!(ts, 500 + i as i64 * 100);
        }
    }
}
