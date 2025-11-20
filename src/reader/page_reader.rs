use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::compress::{CompressorImpl, create_compressor};
use crate::encoding::{DecoderImpl, create_decoder};
use crate::error::{Result, TsFileError};
use crate::file::{PageData, PageHeader};
use std::io::Read;

/// Reader para páginas individuales
/// OPT-READ-1: Use static dispatch (CompressorImpl) instead of Box<dyn Compressor>
/// Eliminates virtual calls in hot decompression path (5-10% speedup)
pub struct PageReader {
    data_type: TSDataType,
    encoding: TSEncoding,
    compressor: CompressorImpl,
}

impl PageReader {
    /// Crea un nuevo PageReader
    pub fn new(
        data_type: TSDataType,
        encoding: TSEncoding,
        compression_type: CompressionType,
    ) -> Self {
        let compressor = create_compressor(compression_type);
        Self {
            data_type,
            encoding,
            compressor,
        }
    }

    /// Lee una página de un reader y retorna los datos decodificados
    pub fn read_page<R: Read>(&mut self, reader: &mut R) -> Result<DecodedPage> {
        // Leer header de página
        let header = PageHeader::deserialize(reader)?;

        // Leer datos comprimidos
        let mut compressed_data = vec![0u8; header.compressed_size as usize];
        reader.read_exact(&mut compressed_data)?;

        // Descomprimir (reutilizando compressor instance) - OPT-READ-1
        let uncompressed =
            self.compressor.decompress(&compressed_data, header.uncompressed_size as usize)?;

        // Leer tamaños y decodificar
        use byteorder::{LittleEndian, ReadBytesExt};
        let mut cursor = std::io::Cursor::new(&uncompressed);

        // Leer tamaño del time buffer
        let time_size = cursor.read_u32::<LittleEndian>()? as usize;
        let time_start = cursor.position() as usize;
        let time_end = time_start + time_size;

        // Decodificar timestamps - OPT-READ-5: static dispatch
        let time_buffer = &uncompressed[time_start..time_end];
        let mut time_decoder = create_decoder(TSEncoding::Ts2Diff, TSDataType::Int64);
        let mut timestamps = Vec::with_capacity(header.num_of_values as usize);
        let mut pos = 0;

        while time_decoder.has_remaining(time_buffer, pos)
            && timestamps.len() < header.num_of_values as usize
        {
            let ts = time_decoder.read_i64(time_buffer, &mut pos)?;
            timestamps.push(ts);
        }

        // Posicionar cursor después del time buffer
        cursor.set_position((time_end) as u64);

        // Leer tamaño del value buffer
        let value_size = cursor.read_u32::<LittleEndian>()? as usize;
        let value_start = cursor.position() as usize;
        let value_end = value_start + value_size;

        // Decodificar valores - OPT-READ-5: static dispatch
        let value_buffer = &uncompressed[value_start..value_end];
        let mut value_decoder = create_decoder(self.encoding, self.data_type);
        let mut value_pos = 0;
        let values = self.decode_values(
            &mut value_decoder,
            value_buffer,
            &mut value_pos,
            header.num_of_values as usize,
        )?;

        Ok(DecodedPage {
            timestamps,
            values,
            num_of_values: header.num_of_values as usize,
        })
    }

    /// Lee una página desde PageData con mini-blocks (Timbre format)
    ///
    /// **Innovación core de Timbre**: Decodifica 4-8 mini-blocks en PARALELO con Rayon.
    /// Speedup potencial: 8x en máquinas con 8+ cores.
    pub fn read_page_data(&mut self, page_data: &PageData) -> Result<DecodedPage> {
        use rayon::prelude::*;

        // Decodificar cada mini-block en paralelo
        let decoded_miniblocks: Vec<(Vec<i64>, DecodedValues)> = page_data
            .miniblocks
            .par_iter()
            .map(|miniblock| {
                // Crear instancias fresh de compressor y decoders (thread-local)
                let compression_type = self.compressor.compression_type();
                let mut mb_compressor = create_compressor(compression_type);

                // Descomprimir timestamps y values independientemente usando tamaños del header
                let time_uncompressed = mb_compressor
                    .decompress(&miniblock.timestamp_data, miniblock.header.timestamp_uncompressed_size as usize)
                    .map_err(|e| TsFileError::DecompressionError(format!("Timestamp decompression failed: {}", e)))?;

                let value_uncompressed = mb_compressor
                    .decompress(&miniblock.value_data, miniblock.header.value_uncompressed_size as usize)
                    .map_err(|e| TsFileError::DecompressionError(format!("Value decompression failed: {}", e)))?;

                // Decodificar timestamps
                let mut time_decoder = create_decoder(TSEncoding::Ts2Diff, TSDataType::Int64);
                let mut timestamps = Vec::with_capacity(miniblock.header.point_count as usize);
                let mut pos = 0;

                while time_decoder.has_remaining(&time_uncompressed, pos)
                    && timestamps.len() < miniblock.header.point_count as usize
                {
                    let ts = time_decoder.read_i64(&time_uncompressed, &mut pos)
                        .map_err(|e| TsFileError::EncodingError(format!("Timestamp decode failed: {}", e)))?;
                    timestamps.push(ts);
                }

                // Decodificar values
                let mut value_decoder = create_decoder(self.encoding, self.data_type);
                let mut value_pos = 0;
                let values = self.decode_values(
                    &mut value_decoder,
                    &value_uncompressed,
                    &mut value_pos,
                    miniblock.header.point_count as usize,
                )?;

                Ok::<_, crate::error::TsFileError>((timestamps, values))
            })
            .collect::<Result<Vec<_>>>()?;

        // Concatenar resultados de mini-blocks (mantener orden)
        let mut all_timestamps = Vec::with_capacity(page_data.header.num_of_values as usize);
        let mut all_values_vecs: Vec<DecodedValues> = Vec::new();

        for (ts, vals) in decoded_miniblocks {
            all_timestamps.extend(ts);
            all_values_vecs.push(vals);
        }

        // Merge values según tipo
        let merged_values = Self::merge_decoded_values(all_values_vecs, self.data_type)?;

        Ok(DecodedPage {
            timestamps: all_timestamps,
            values: merged_values,
            num_of_values: page_data.header.num_of_values as usize,
        })
    }

    /// Merge múltiples DecodedValues en uno solo
    fn merge_decoded_values(
        mut values_vec: Vec<DecodedValues>,
        data_type: TSDataType,
    ) -> Result<DecodedValues> {
        if values_vec.is_empty() {
            return Err(TsFileError::InvalidState("No values to merge".to_string()));
        }

        if values_vec.len() == 1 {
            return Ok(values_vec.pop().unwrap());
        }

        // Merge según tipo
        match data_type {
            TSDataType::Boolean => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Boolean(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Boolean(merged))
            }
            TSDataType::Int32 | TSDataType::Date => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Int32(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Int32(merged))
            }
            TSDataType::Int64 | TSDataType::Timestamp => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Int64(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Int64(merged))
            }
            TSDataType::Float => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Float(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Float(merged))
            }
            TSDataType::Double => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Double(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Double(merged))
            }
            TSDataType::Text | TSDataType::String => {
                let mut merged = Vec::new();
                for dv in values_vec {
                    if let DecodedValues::Text(v) = dv {
                        merged.extend(v);
                    }
                }
                Ok(DecodedValues::Text(merged))
            }
            _ => Err(TsFileError::TypeMismatch {
                expected: "supported type".to_string(),
                actual: format!("{:?}", data_type),
            }),
        }
    }

    /// Decodifica valores según el tipo de dato
    /// OPT-READ-4: Pre-allocate with exact capacity and use unsafe set_len
    /// OPT-READ-5: Use DecoderImpl (static dispatch) instead of Box<dyn Decoder>
    fn decode_values(
        &self,
        decoder: &mut DecoderImpl,
        data: &[u8],
        pos: &mut usize,
        count: usize,
    ) -> Result<DecodedValues> {
        match self.data_type {
            TSDataType::Boolean => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decoder.read_bool(data, pos)?);
                }
                Ok(DecodedValues::Boolean(values))
            }
            TSDataType::Int32 => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decoder.read_i32(data, pos)?);
                }
                Ok(DecodedValues::Int32(values))
            }
            TSDataType::Int64 => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decoder.read_i64(data, pos)?);
                }
                Ok(DecodedValues::Int64(values))
            }
            TSDataType::Float => {
                // OPT-READ-4: Hot path for Float - most common in benchmarks
                // Use unsafe to avoid bounds checks in tight decode loop
                let mut values: Vec<f32> = Vec::with_capacity(count);
                unsafe {
                    let ptr = values.as_mut_ptr();
                    for i in 0..count {
                        ptr.add(i).write(decoder.read_f32(data, pos)?);
                    }
                    values.set_len(count);
                }
                Ok(DecodedValues::Float(values))
            }
            TSDataType::Double => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decoder.read_f64(data, pos)?);
                }
                Ok(DecodedValues::Double(values))
            }
            TSDataType::Text => {
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decoder.read_string(data, pos)?);
                }
                Ok(DecodedValues::Text(values))
            }
            _ => Err(crate::error::TsFileError::TypeMismatch {
                expected: "supported type".to_string(),
                actual: format!("{:?}", self.data_type),
            }),
        }
    }
}

/// Página decodificada con timestamps y valores
#[derive(Debug, Clone)]
pub struct DecodedPage {
    pub timestamps: Vec<i64>,
    pub values: DecodedValues,
    pub num_of_values: usize,
}

/// Valores decodificados según tipo
#[derive(Debug, Clone)]
pub enum DecodedValues {
    Boolean(Vec<bool>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    Text(Vec<String>),
}

impl DecodedPage {
    /// Obtiene un valor específico por índice
    pub fn get(&self, index: usize) -> Option<(i64, DecodedValue)> {
        if index >= self.num_of_values {
            return None;
        }

        let timestamp = self.timestamps[index];
        let value = match &self.values {
            DecodedValues::Boolean(v) => DecodedValue::Boolean(v[index]),
            DecodedValues::Int32(v) => DecodedValue::Int32(v[index]),
            DecodedValues::Int64(v) => DecodedValue::Int64(v[index]),
            DecodedValues::Float(v) => DecodedValue::Float(v[index]),
            DecodedValues::Double(v) => DecodedValue::Double(v[index]),
            DecodedValues::Text(v) => DecodedValue::Text(v[index].clone()),
        };

        Some((timestamp, value))
    }

    /// Itera sobre todos los valores
    pub fn iter(&self) -> DecodedPageIter<'_> {
        DecodedPageIter {
            page: self,
            index: 0,
        }
    }
}

/// Iterador para DecodedPage
pub struct DecodedPageIter<'a> {
    page: &'a DecodedPage,
    index: usize,
}

impl<'a> Iterator for DecodedPageIter<'a> {
    type Item = (i64, DecodedValue);

    fn next(&mut self) -> Option<Self::Item> {
        let result = self.page.get(self.index);
        self.index += 1;
        result
    }
}

/// Un valor decodificado
#[derive(Debug, Clone)]
pub enum DecodedValue {
    Boolean(bool),
    Int32(i32),
    Int64(i64),
    Float(f32),
    Double(f64),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::PageWriter;

    #[test]
    fn test_page_reader_float() {
        // Escribir página
        let mut writer = PageWriter::new(
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        for i in 0..10 {
            writer.write_f32(1000 + i * 100, 25.0 + i as f32).unwrap();
        }

        let page_data = writer.finish().unwrap();

        // Leer página
        let mut reader = PageReader::new(
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let decoded = reader.read_page_data(&page_data).unwrap();

        assert_eq!(decoded.num_of_values, 10);
        assert_eq!(decoded.timestamps.len(), 10);

        // Verificar valores
        if let DecodedValues::Float(values) = &decoded.values {
            assert_eq!(values.len(), 10);
            for i in 0..10 {
                assert_eq!(values[i], 25.0 + i as f32);
            }
        } else {
            panic!("Expected Float values");
        }
    }

    #[test]
    fn test_page_reader_i32() {
        // Escribir página
        let mut writer =
            PageWriter::new(TSDataType::Int32, TSEncoding::Plain, CompressionType::Lz4);

        for i in 0..20 {
            writer.write_i32(2000 + i * 50, i as i32 * 10).unwrap();
        }

        let page_data = writer.finish().unwrap();

        // Leer página
        let mut reader =
            PageReader::new(TSDataType::Int32, TSEncoding::Plain, CompressionType::Lz4);

        let decoded = reader.read_page_data(&page_data).unwrap();

        assert_eq!(decoded.num_of_values, 20);

        // Verificar con iterador
        for (i, (ts, value)) in decoded.iter().enumerate() {
            assert_eq!(ts, 2000 + i as i64 * 50);
            if let DecodedValue::Int32(v) = value {
                assert_eq!(v, i as i32 * 10);
            } else {
                panic!("Expected Int32 value");
            }
        }
    }

    #[test]
    fn test_page_reader_string() {
        // Escribir página
        let mut writer = PageWriter::new(
            TSDataType::Text,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        writer.write_string(1000, "hello").unwrap();
        writer.write_string(2000, "world").unwrap();
        writer.write_string(3000, "test").unwrap();

        let page_data = writer.finish().unwrap();

        // Leer página
        let mut reader = PageReader::new(
            TSDataType::Text,
            TSEncoding::Plain,
            CompressionType::Uncompressed,
        );

        let decoded = reader.read_page_data(&page_data).unwrap();

        assert_eq!(decoded.num_of_values, 3);

        if let DecodedValues::Text(values) = &decoded.values {
            assert_eq!(values[0], "hello");
            assert_eq!(values[1], "world");
            assert_eq!(values[2], "test");
        } else {
            panic!("Expected Text values");
        }
    }
}
