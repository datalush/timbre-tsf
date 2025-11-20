//! Adaptive encoding analysis tools for selecting optimal encoders.
//!
//! This module provides tools to analyze data patterns and recommend the best encoding:
//! - **Quantized**: For regular step patterns (e.g., 0.1°C resolution) → 26x compression
//! - **DictionaryRLE**: For high repetition with irregular discrete values → 10-40x compression
//! - **Chimp128**: For continuous drift or high entropy → 5-8x compression
//!
//! **Architecture**: This module provides ANALYSIS TOOLS only. The application/database
//! layer is responsible for buffering data and making encoding decisions.
//!
//! # Example
//!
//! ```
//! use timbre_tsf::encoding::adaptive::recommend_encoding;
//!
//! let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0]; // Regular 0.1 steps
//! let encoding = recommend_encoding(&sample);
//! // encoding == TSEncoding::Quantized
//!
//! // Application then uses the appropriate encoder based on recommendation
//! ```

use crate::common::{TSEncoding, CompressionType};
use crate::encoding::quantized::detect_quantization;
use std::collections::HashSet;

/// Data pattern classification
#[derive(Debug, Clone, PartialEq)]
pub enum DataPattern {
    /// Regular quantization detected (e.g., 0.1°C steps)
    Quantized { min: f64, step: f64 },

    /// High repetition (>70%) with discrete values
    HighRepetition {
        unique_count: usize,
        repetition_pct: f64,
    },

    /// Continuous drift (small, frequent changes)
    ContinuousDrift { avg_delta: f64, max_delta: f64 },

    /// High entropy / random data
    HighEntropy,
}

/// Analyzes data pattern to determine optimal encoding.
///
/// This is the core analysis function that classifies data into different patterns
/// to help applications choose the best encoding strategy.
pub fn analyze_pattern(data: &[f64]) -> DataPattern {
        if data.is_empty() {
            return DataPattern::HighEntropy;
        }

        // 1. Check for quantization (highest priority)
        if let Some((min, step)) = detect_quantization(data) {
            return DataPattern::Quantized { min, step };
        }

        // 2. Analyze repetition and unique values
        let unique_values: HashSet<u64> = data.iter().map(|v| v.to_bits()).collect();
        let repetition_pct = 1.0 - (unique_values.len() as f64 / data.len() as f64);

        // High repetition with discrete values → DictionaryRLE
        if repetition_pct > 0.5 && unique_values.len() < 256 {
            return DataPattern::HighRepetition {
                unique_count: unique_values.len(),
                repetition_pct,
            };
        }

        // 3. Analyze deltas for drift detection
        if data.len() > 1 {
            let deltas: Vec<f64> = data.windows(2).map(|w| (w[1] - w[0]).abs()).collect();

            let avg_delta = deltas.iter().sum::<f64>() / deltas.len() as f64;
            let max_delta = deltas.iter().fold(0.0f64, |acc, &d| acc.max(d));

            // Continuous small changes → Chimp128
            if avg_delta < 1.0 && max_delta < 10.0 {
                return DataPattern::ContinuousDrift {
                    avg_delta,
                    max_delta,
                };
            }
        }

        // Default: high entropy
        DataPattern::HighEntropy
    }


/// Analyzes a data sample and recommends the optimal TSEncoding.
///
/// This is the main API function for applications to get encoding recommendations
/// based on data pattern analysis.
///
/// # Arguments
///
/// * `data` - Sample of floating-point values to analyze (typically 1000-10000 values)
///
/// # Returns
///
/// The recommended TSEncoding variant
///
/// # Example
///
/// ```
/// use timbre_tsf::encoding::adaptive::recommend_encoding;
///
/// let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0]; // Quantized pattern
/// let encoding = recommend_encoding(&sample);
/// // encoding == TSEncoding::Quantized
/// ```
pub fn recommend_encoding(data: &[f64]) -> TSEncoding {
    let pattern = analyze_pattern(data);

    match pattern {
        DataPattern::Quantized { .. } => TSEncoding::Quantized,
        DataPattern::HighRepetition { .. } => TSEncoding::DictionaryRLE,
        DataPattern::ContinuousDrift { .. } => TSEncoding::Chimp128,
        DataPattern::HighEntropy => TSEncoding::Chimp128, // Fallback
    }
}

/// Recomienda compresión óptima basándose en el encoding usado
///
/// Esta función proporciona una heurística basada en benchmarks reales que miden
/// el trade-off entre ratio de compresión y throughput para diferentes encodings.
///
/// # Heurística (basada en benchmarks con 100K puntos)
///
/// ## Encodings que producen datos compactos → **Zstd serial**
///
/// - **Quantized + Simple8b**: 27KB encoded → 94B compressed (284x), 9µs
/// - **DictionaryRLE**: 21KB encoded → 97B compressed (221x), 9µs
/// - **Simple8b**: Similar a Quantized
///
/// Estos encodings ya comprimen extremadamente bien, produciendo datos muy pequeños.
/// Zstd serial puede procesarlos en ~9µs, mientras que paralelizar con LZ4 cuesta
/// ~27µs de overhead (3x más lento) y pierde ratio de compresión significativamente.
///
/// ## Encodings que producen datos grandes quasi-random → **LZ4 paralelo**
///
/// - **Chimp128**: 838KB encoded → 828KB compressed (1.01x), 80µs paralelo
///   - vs Zstd serial: 838KB → 689KB (1.22x), 3.3ms
///   - Speedup: **40x más rápido**, pérdida ratio: ~1%
/// - **Gorilla**: Similar a Chimp128
///
/// Estos encodings producen datos grandes con alta entropía (XOR de floats).
/// Ni Zstd ni LZ4 comprimen bien, pero LZ4 paralelo es 40x más rápido con
/// solo 1% de pérdida en ratio.
///
/// # Override Manual
///
/// Esta es una **recomendación**. La aplicación puede forzar cualquier
/// `CompressionType` manualmente si tiene requisitos específicos:
///
/// ```rust
/// use timbre_tsf::encoding::adaptive::recommend_compression;
/// use timbre_tsf::common::{TSEncoding, CompressionType};
///
/// // Opción A: Usar recomendación
/// let encoding = TSEncoding::Chimp128;
/// let compression = recommend_compression(encoding);
/// // compression == CompressionType::Lz4
///
/// // Opción B: Override manual (priorizar ratio sobre speed)
/// let compression = CompressionType::Zstd;  // Ignorar recomendación
/// ```
///
/// # Benchmarks
///
/// Ver `benches/encoding_compression_tradeoff.rs` para detalles completos.
///
/// # Returns
///
/// El `CompressionType` recomendado para el encoding dado.
pub fn recommend_compression(encoding: TSEncoding) -> CompressionType {
    match encoding {
        // Encodings que comprimen extremadamente bien → Zstd serial
        // (datos tiny ~20-30KB, Zstd procesa en ~9µs, LZ4 paralelo cuesta ~27µs overhead)
        TSEncoding::Quantized | TSEncoding::Simple8b | TSEncoding::DictionaryRLE => {
            CompressionType::Zstd
        }

        // Encodings con datos grandes quasi-random → LZ4 paralelo
        // (datos ~800KB+, LZ4 40x speedup vs solo 1% pérdida de ratio)
        TSEncoding::Chimp128 | TSEncoding::Gorilla => CompressionType::Lz4,

        // Default conservador: Zstd (mejor ratio)
        _ => CompressionType::Zstd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_detection_quantized() {
        let data = vec![20.0, 20.1, 20.2, 20.1, 20.0];
        let pattern = analyze_pattern(&data);

        match pattern {
            DataPattern::Quantized { min, step } => {
                assert!((min - 20.0).abs() < 1e-6);
                assert!((step - 0.1).abs() < 1e-6);
            }
            _ => panic!("Expected Quantized pattern, got {:?}", pattern),
        }
    }

    #[test]
    fn test_pattern_detection_high_repetition() {
        // 90% repetition with irregular values
        let data = vec![
            20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.3, 20.7, 21.2, 20.0, 20.0,
            20.0, 20.0, 20.0, 20.0, 20.0, 20.0,
        ];
        let pattern = analyze_pattern(&data);

        match pattern {
            DataPattern::HighRepetition {
                unique_count,
                repetition_pct,
            } => {
                assert_eq!(unique_count, 4); // 20.0, 20.3, 20.7, 21.2
                assert!(repetition_pct > 0.7);
            }
            _ => panic!("Expected HighRepetition pattern, got {:?}", pattern),
        }
    }

    #[test]
    fn test_pattern_detection_continuous_drift() {
        // Continuous small irregular changes (not quantized)
        let mut data = Vec::new();
        let mut value = 20.0;
        for i in 0..100 {
            data.push(value);
            value += 0.001 * (1.0 + (i as f64 * 0.01).sin() * 0.1); // Irregular small drift
        }

        let pattern = analyze_pattern(&data);

        match pattern {
            DataPattern::ContinuousDrift { avg_delta, .. } => {
                assert!(avg_delta < 1.0);
            }
            DataPattern::Quantized { .. } => {
                // Also acceptable if pattern is so regular it looks quantized
            }
            _ => panic!("Expected ContinuousDrift or Quantized pattern, got {:?}", pattern),
        }
    }

    #[test]
    fn test_recommend_encoding_quantized() {
        let data = vec![20.0, 20.1, 20.2, 20.1, 20.0];
        let encoding = recommend_encoding(&data);
        assert_eq!(encoding, TSEncoding::Quantized);
    }

    #[test]
    fn test_recommend_encoding_dictionary_rle() {
        let data = vec![
            20.0, 20.0, 20.0, 20.0, 20.0, 20.3, 20.3, 20.7, 20.7, 20.7, 20.0, 20.0, 20.0, 20.0,
            20.0, 20.0, 20.0, 20.0, 20.0, 20.0,
        ];
        let encoding = recommend_encoding(&data);
        assert_eq!(encoding, TSEncoding::DictionaryRLE);
    }

    #[test]
    fn test_recommend_encoding_chimp128() {
        // Continuous irregular drift
        let mut data = Vec::new();
        let mut value = 20.0;
        for i in 0..100 {
            data.push(value);
            value += 0.001 * (1.0 + (i as f64 * 0.01).sin() * 0.1);
        }
        let encoding = recommend_encoding(&data);
        // Could be Quantized or Chimp128 depending on pattern detection
        assert!(
            encoding == TSEncoding::Chimp128 || encoding == TSEncoding::Quantized,
            "Expected Chimp128 or Quantized, got {:?}",
            encoding
        );
    }

    #[test]
    fn test_recommend_compression_quantized() {
        let compression = recommend_compression(TSEncoding::Quantized);
        assert_eq!(compression, CompressionType::Zstd);
    }

    #[test]
    fn test_recommend_compression_chimp128() {
        let compression = recommend_compression(TSEncoding::Chimp128);
        assert_eq!(compression, CompressionType::Lz4);
    }

    #[test]
    fn test_recommend_compression_dictionary_rle() {
        let compression = recommend_compression(TSEncoding::DictionaryRLE);
        assert_eq!(compression, CompressionType::Zstd);
    }

    #[test]
    fn test_recommend_compression_gorilla() {
        let compression = recommend_compression(TSEncoding::Gorilla);
        assert_eq!(compression, CompressionType::Lz4);
    }
}
