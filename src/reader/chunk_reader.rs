use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::error::Result;
use crate::file::{ChunkHeader, PageData, PageHeader};
use crate::reader::{DecodedPage, DecodedValues, PageReader};
use rayon::prelude::*;
use std::io::Read;

/// Reader para chunks (colección de páginas)
pub struct ChunkReader {
    measurement_name: String,
    data_type: TSDataType,
    encoding: TSEncoding,
    compression_type: CompressionType,
}

impl ChunkReader {
    /// Crea un nuevo ChunkReader
    pub fn new(
        measurement_name: String,
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        Self {
            measurement_name,
            data_type,
            encoding,
            compression_type,
        }
    }

    /// Lee un chunk completo desde un reader
    /// Optimizado: Paralelismo con Rayon para decodificar páginas
    /// Estrategia:
    /// 1. Leer page data (headers + compressed) secuencialmente (I/O bound, rápido)
    /// 2. Decodificar páginas en paralelo (CPU bound, lento) ← 40-60% speedup
    /// 3. Merge secuencial (rápido)
    pub fn read_chunk<R: Read>(&mut self, reader: &mut R) -> Result<DecodedChunk> {
        // Leer header del chunk
        let header = ChunkHeader::deserialize(reader)?;

        // Paso 1: Leer todos los page data secuencialmente (I/O)
        // Timbre: Cada página contiene 4-8 mini-blocks
        let mut page_data_list = Vec::with_capacity(header.num_of_pages as usize);
        for _ in 0..header.num_of_pages {
            let page_header = PageHeader::deserialize(reader)?;

            // Leer número de mini-blocks
            use byteorder::{LittleEndian, ReadBytesExt};
            let miniblock_count = reader.read_u32::<LittleEndian>()? as usize;

            // Leer cada mini-block
            let mut miniblocks = Vec::with_capacity(miniblock_count);
            for _ in 0..miniblock_count {
                let miniblock = crate::file::MiniBlock::deserialize(reader)?;
                miniblocks.push(miniblock);
            }

            page_data_list.push(PageData {
                header: page_header,
                miniblocks,
            });
        }

        // Paso 2: Decodificar páginas EN PARALELO (CPU bound)
        let pages: Vec<DecodedPage> = page_data_list
            .par_iter()
            .map(|page_data| {
                // Cada thread crea su propio PageReader para decodificar
                let mut page_reader = PageReader::new(
                    self.data_type,
                    self.encoding,
                    self.compression_type,
                );
                page_reader
                    .read_page_data(page_data)
                    .map_err(|e| format!("Failed to decode page: {:?}", e))
            })
            .collect::<std::result::Result<Vec<_>, String>>()
            .map_err(|e| crate::error::TsFileError::DecodingError(e))?;

        // Paso 3: Merge secuencial (rápido, solo concatena vectores)
        let mut all_timestamps = Vec::new();
        let mut all_values: Option<DecodedValues> = None;

        for page in pages {
            // Agregar timestamps
            all_timestamps.extend_from_slice(&page.timestamps);

            // Merge valores sin boxing: usa append() en lugar de map()
            match (&mut all_values, page.values) {
                (None, values) => all_values = Some(values),
                (Some(DecodedValues::Boolean(dest)), DecodedValues::Boolean(mut src)) => {
                    dest.append(&mut src);
                }
                (Some(DecodedValues::Int32(dest)), DecodedValues::Int32(mut src)) => {
                    dest.append(&mut src);
                }
                (Some(DecodedValues::Int64(dest)), DecodedValues::Int64(mut src)) => {
                    dest.append(&mut src);
                }
                (Some(DecodedValues::Float(dest)), DecodedValues::Float(mut src)) => {
                    dest.append(&mut src);
                }
                (Some(DecodedValues::Double(dest)), DecodedValues::Double(mut src)) => {
                    dest.append(&mut src);
                }
                (Some(DecodedValues::Text(dest)), DecodedValues::Text(mut src)) => {
                    dest.append(&mut src);
                }
                _ => {
                    return Err(crate::error::TsFileError::TypeMismatch {
                        expected: format!("{:?}", self.data_type),
                        actual: "mismatched types across pages".to_string(),
                    })
                }
            }
        }

        Ok(DecodedChunk {
            measurement_name: self.measurement_name.clone(),
            data_type: self.data_type,
            timestamps: all_timestamps,
            values: all_values.unwrap_or_else(|| DecodedValues::Int32(Vec::new())),
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
/// Optimizado: usa DecodedValues (Vec<T>) en lugar de Vec<DecodedValueData>
/// para eliminar boxing y mejorar cache locality
#[derive(Debug, Clone)]
pub struct DecodedChunk {
    pub measurement_name: String,
    pub data_type: TSDataType,
    pub timestamps: Vec<i64>,
    pub values: DecodedValues,
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
    pub fn get(&self, index: usize) -> Option<(i64, DecodedValueData)> {
        if index >= self.len() {
            return None;
        }

        let timestamp = self.timestamps[index];
        let value = match &self.values {
            DecodedValues::Boolean(vec) => DecodedValueData::Boolean(vec[index]),
            DecodedValues::Int32(vec) => DecodedValueData::Int32(vec[index]),
            DecodedValues::Int64(vec) => DecodedValueData::Int64(vec[index]),
            DecodedValues::Float(vec) => DecodedValueData::Float(vec[index]),
            DecodedValues::Double(vec) => DecodedValueData::Double(vec[index]),
            DecodedValues::Text(vec) => DecodedValueData::Text(vec[index].clone()),
        };

        Some((timestamp, value))
    }

    /// Itera sobre todos los valores
    pub fn iter(&self) -> DecodedChunkIter<'_> {
        DecodedChunkIter {
            chunk: self,
            index: 0,
        }
    }

    /// Filtra valores por rango de tiempo
    pub fn filter_time_range(&self, min_time: i64, max_time: i64) -> DecodedChunk {
        let mut filtered_timestamps = Vec::new();

        // Build filtered indices first
        let filtered_indices: Vec<usize> = self.timestamps
            .iter()
            .enumerate()
            .filter_map(|(i, &ts)| {
                if ts >= min_time && ts <= max_time {
                    filtered_timestamps.push(ts);
                    Some(i)
                } else {
                    None
                }
            })
            .collect();

        // Build filtered values based on type
        let filtered_values = match &self.values {
            DecodedValues::Boolean(vec) => {
                DecodedValues::Boolean(filtered_indices.iter().map(|&i| vec[i]).collect())
            }
            DecodedValues::Int32(vec) => {
                DecodedValues::Int32(filtered_indices.iter().map(|&i| vec[i]).collect())
            }
            DecodedValues::Int64(vec) => {
                DecodedValues::Int64(filtered_indices.iter().map(|&i| vec[i]).collect())
            }
            DecodedValues::Float(vec) => {
                DecodedValues::Float(filtered_indices.iter().map(|&i| vec[i]).collect())
            }
            DecodedValues::Double(vec) => {
                DecodedValues::Double(filtered_indices.iter().map(|&i| vec[i]).collect())
            }
            DecodedValues::Text(vec) => {
                DecodedValues::Text(filtered_indices.iter().map(|&i| vec[i].clone()).collect())
            }
        };

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
    type Item = (i64, DecodedValueData);

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
                assert_eq!(v, 25.0 + i as f32);
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
                assert_eq!(v, i as i32 * 5);
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
