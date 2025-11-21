//! Builder pattern para PageWriter con recomendaciones adaptativas
//!
//! Proporciona una API idiomática de Rust para configurar PageWriter con
//! opciones de encoding y compresión, soportando tanto configuración manual
//! como recomendaciones automáticas basadas en análisis de datos.

use crate::common::{CompressionType, TSDataType, TSEncoding};
use crate::utils::encoding_analyzer::{recommend_compression, recommend_encoding};
use crate::error::{Result, TimbreError};
use crate::file::MiniBlockConfig;
use crate::writer::PageWriter;

/// Builder para crear instancias de PageWriter con configuración flexible
///
/// # Ejemplos
///
/// ```rust
/// use timbre_tsf::writer::PageWriterBuilder;
/// use timbre_tsf::common::TSDataType;
///
/// // Ejemplo 1: Fully automatic (recommended)
/// let sample_data = vec![20.0, 20.1, 20.2, 20.1, 20.0]; // Quantized pattern
/// # let writer = PageWriterBuilder::new()
/// #    .data_type(TSDataType::Double)
/// #    .analyze_and_recommend(&sample_data)
/// #    .build();
/// // Resultado: encoding=Quantized, compression=Zstd (optimal para datos cuantizados)
///
/// // Ejemplo 2: Manual encoding, recommended compression
/// # let writer = PageWriterBuilder::new()
/// #    .data_type(TSDataType::Double)
/// #    .encoding(timbre_tsf::common::TSEncoding::Chimp128)
/// #    .compression_recommended()
/// #    .build();
/// // Resultado: encoding=Chimp128 (manual), compression=Lz4 (recomendado para Chimp128)
///
/// // Ejemplo 3: Full manual override
/// # let writer = PageWriterBuilder::new()
/// #    .data_type(TSDataType::Double)
/// #    .encoding(timbre_tsf::common::TSEncoding::Chimp128)
/// #    .compression(timbre_tsf::common::CompressionType::Zstd)
/// #    .build();
/// // Resultado: todo manual, ignora recomendaciones
/// ```
#[derive(Debug, Clone)]
pub struct PageWriterBuilder {
    data_type: Option<TSDataType>,
    encoding: Option<TSEncoding>,
    compression: Option<CompressionType>,
    miniblock_config: MiniBlockConfig,
}

impl PageWriterBuilder {
    /// Crea un nuevo builder vacío
    pub fn new() -> Self {
        Self {
            data_type: None,
            encoding: None,
            compression: None,
            miniblock_config: MiniBlockConfig::default(),
        }
    }

    /// Establece el tipo de datos (requerido)
    pub fn data_type(mut self, dt: TSDataType) -> Self {
        self.data_type = Some(dt);
        self
    }

    /// Establece el encoding manualmente
    pub fn encoding(mut self, enc: TSEncoding) -> Self {
        self.encoding = Some(enc);
        self
    }

    /// Establece la compresión manualmente
    pub fn compression(mut self, comp: CompressionType) -> Self {
        self.compression = Some(comp);
        self
    }

    /// Usa la compresión recomendada basada en el encoding actual
    ///
    /// **Requiere** que `encoding()` haya sido llamado previamente.
    /// Si no hay encoding seteado, usa el default (Gorilla) para la recomendación.
    ///
    /// # Ejemplo
    ///
    /// ```rust
    /// use timbre_tsf::writer::PageWriterBuilder;
    /// use timbre_tsf::common::{TSDataType, TSEncoding};
    ///
    /// # let writer = PageWriterBuilder::new()
    /// #    .data_type(TSDataType::Double)
    /// #    .encoding(TSEncoding::Quantized)  // Set encoding first
    /// #    .compression_recommended()        // Then get recommendation
    /// #    .build();
    /// // compression será Zstd (optimal para Quantized)
    /// ```
    pub fn compression_recommended(mut self) -> Self {
        let enc = self.encoding.unwrap_or(TSEncoding::Gorilla);
        self.compression = Some(recommend_compression(enc));
        self
    }

    /// Analiza los datos y establece el encoding recomendado
    ///
    /// La compresión NO se setea automáticamente. Usa `compression_recommended()`
    /// o `analyze_and_recommend()` para setear ambos.
    ///
    /// # Ejemplo
    ///
    /// ```rust
    /// use timbre_tsf::writer::PageWriterBuilder;
    /// use timbre_tsf::common::TSDataType;
    ///
    /// let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0];
    /// # let writer = PageWriterBuilder::new()
    /// #    .data_type(TSDataType::Double)
    /// #    .analyze_encoding(&sample)
    /// #    .compression_recommended()  // Opcional: agregar compresión recomendada
    /// #    .build();
    /// ```
    pub fn analyze_encoding(mut self, data: &[f64]) -> Self {
        self.encoding = Some(recommend_encoding(data));
        self
    }

    /// Analiza los datos y establece AMBOS encoding y compresión recomendados
    ///
    /// Esta es la opción más conveniente para configuración totalmente automática.
    ///
    /// # Ejemplo
    ///
    /// ```rust
    /// use timbre_tsf::writer::PageWriterBuilder;
    /// use timbre_tsf::common::TSDataType;
    ///
    /// let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0];  // Quantized
    /// # let writer = PageWriterBuilder::new()
    /// #    .data_type(TSDataType::Double)
    /// #    .analyze_and_recommend(&sample)
    /// #    .build();
    /// // encoding=Quantized, compression=Zstd (ambos optimales)
    /// ```
    pub fn analyze_and_recommend(mut self, data: &[f64]) -> Self {
        let encoding = recommend_encoding(data);
        let compression = recommend_compression(encoding);
        self.encoding = Some(encoding);
        self.compression = Some(compression);
        self
    }

    /// Configura los miniblocks (opcional, usa defaults razonables)
    pub fn miniblock_config(mut self, config: MiniBlockConfig) -> Self {
        self.miniblock_config = config;
        self
    }

    /// Construye el PageWriter con la configuración actual
    ///
    /// # Errores
    ///
    /// Retorna error si `data_type` no fue seteado (es requerido).
    ///
    /// # Defaults
    ///
    /// - `encoding`: Gorilla (si no especificado)
    /// - `compression`: Zstd (si no especificado)
    /// - `miniblock_config`: 8 miniblocks, 250 puntos mínimos por block
    pub fn build(self) -> Result<PageWriter> {
        let data_type = self
            .data_type
            .ok_or_else(|| TimbreError::InvalidState("data_type is required".to_string()))?;

        let encoding = self.encoding.unwrap_or(TSEncoding::Gorilla);
        let compression = self.compression.unwrap_or(CompressionType::Zstd);

        let mut writer = PageWriter::new(data_type, encoding, compression);
        writer.miniblock_config = self.miniblock_config;

        Ok(writer)
    }
}

impl Default for PageWriterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_manual() {
        let writer = PageWriterBuilder::new()
            .data_type(TSDataType::Double)
            .encoding(TSEncoding::Chimp128)
            .compression(CompressionType::Lz4)
            .build()
            .unwrap();

        assert_eq!(writer.value_count(), 0);
    }

    #[test]
    fn test_builder_compression_recommended() {
        let writer = PageWriterBuilder::new()
            .data_type(TSDataType::Double)
            .encoding(TSEncoding::Quantized)
            .compression_recommended()
            .build()
            .unwrap();

        assert_eq!(writer.value_count(), 0);
    }

    #[test]
    fn test_builder_analyze_and_recommend() {
        let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0]; // Quantized
        let writer = PageWriterBuilder::new()
            .data_type(TSDataType::Double)
            .analyze_and_recommend(&sample)
            .build()
            .unwrap();

        assert_eq!(writer.value_count(), 0);
    }

    #[test]
    fn test_builder_missing_data_type() {
        let result = PageWriterBuilder::new()
            .encoding(TSEncoding::Chimp128)
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_builder_defaults() {
        let writer = PageWriterBuilder::new()
            .data_type(TSDataType::Double)
            .build()
            .unwrap();

        // Should use defaults: Gorilla encoding, Zstd compression
        assert_eq!(writer.value_count(), 0);
    }
}
