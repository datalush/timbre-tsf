/// Benchmark Riguroso: Compresión Timbre vs Baselines
///
/// Demuestra mejora real de Chimp128 + Zstd + Diccionario
/// Objetivo: 40-60x con Chimp128 solo, 6-10x vs Raw+Zstd
///
/// Run with: cargo bench --bench compression_rigorous

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use rand::Rng;
use std::time::Duration;

/// Genera datos IoT realistas con estabilidad configurable
///
/// stability: 0.85 = 85% de valores repetidos (sensor temperatura típico)
/// stability: 0.60 = 60% de valores repetidos (sensor presión variable)
///
/// Genera ~10-20 valores discretos de temperatura que se repiten frecuentemente,
/// simulando lecturas reales de sensores IoT con cuantización.
fn generate_iot_temperature(n: usize, stability: f64) -> Vec<f64> {
    let mut rng = rand::thread_rng();
    let mut result = Vec::with_capacity(n);

    // Valores discretos de temperatura (cuantizados a 0.1°C, realista para sensores)
    let possible_temps: Vec<f64> = (180..=250)
        .map(|t| t as f64 / 10.0)  // 18.0, 18.1, 18.2, ..., 25.0
        .collect();

    // Temperatura actual (índice en possible_temps)
    let mut current_idx = 20; // Empieza en 20.0°C

    for _ in 0..n {
        // Con probabilidad 'stability', mantener el mismo valor
        if rng.gen_range(0.0..1.0) > stability {
            // Cambio a temperatura adyacente (±1 índice = ±0.1°C)
            let delta = if rng.gen_bool(0.5) { 1 } else { -1 };
            current_idx = (current_idx + delta).clamp(0, possible_temps.len() as i32 - 1);
        }

        result.push(possible_temps[current_idx as usize]);
    }

    result
}

/// Baseline 1: Raw doubles como bytes
fn compress_raw(data: &[f64]) -> Vec<u8> {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    bytes.to_vec()
}

/// Baseline 2: Raw doubles + Zstd-3
fn compress_raw_zstd(data: &[f64]) -> Vec<u8> {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    zstd::bulk::compress(bytes, 3).unwrap()
}

/// Method 3: Chimp128 encoding only (sin compresión adicional)
fn compress_chimp128_only(data: &[f64]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut output = Vec::new();

    for &value in data {
        encoder.encode_f64(value, &mut output).unwrap();
    }
    encoder.flush(&mut output).unwrap();

    output
}

/// Method 4: Chimp128 + Zstd-3
fn compress_chimp128_zstd(data: &[f64]) -> Vec<u8> {
    let chimp_encoded = compress_chimp128_only(data);
    zstd::bulk::compress(&chimp_encoded, 3).unwrap()
}

/// Method 5: Chimp128 + Zstd-3 + Dictionary
fn compress_chimp128_zstd_dict(data: &[f64], dict: &[u8]) -> Vec<u8> {
    let chimp_encoded = compress_chimp128_only(data);
    let mut encoder = zstd::stream::Encoder::with_dictionary(Vec::new(), 3, dict).unwrap();
    std::io::Write::write_all(&mut encoder, &chimp_encoded).unwrap();
    encoder.finish().unwrap()
}

/// Entrena diccionario Zstd con muestras
fn train_dictionary(samples: Vec<Vec<u8>>, dict_size: usize) -> Vec<u8> {
    // Recoger tamaños y concatenar samples
    let sample_sizes: Vec<usize> = samples.iter().map(|s| s.len()).collect();
    let concatenated: Vec<u8> = samples.into_iter().flatten().collect();
    zstd::dict::from_continuous(&concatenated, &sample_sizes, dict_size).unwrap()
}

/// Benchmark principal
fn compression_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("compression_comparison");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    // Diferentes tamaños de dataset
    for size in [10_000, 100_000, 1_000_000] {
        // Datos IoT realistas: 85% estabilidad
        let data = generate_iot_temperature(size, 0.85);

        // Entrenar diccionario con muestras similares
        let training_samples: Vec<Vec<u8>> = (0..10)
            .map(|_| {
                let sample = generate_iot_temperature(size / 10, 0.85);
                compress_chimp128_only(&sample)
            })
            .collect();
        let dictionary = train_dictionary(training_samples, 4096);

        // Calcular throughput
        let data_size = size * std::mem::size_of::<f64>();
        group.throughput(Throughput::Bytes(data_size as u64));

        // === Benchmark 1: Raw ===
        group.bench_with_input(
            BenchmarkId::new("1_raw", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress_raw(black_box(data));
                    black_box(compressed);
                });
            },
        );

        // === Benchmark 2: Raw + Zstd ===
        group.bench_with_input(
            BenchmarkId::new("2_raw_zstd", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress_raw_zstd(black_box(data));
                    black_box(compressed);
                });
            },
        );

        // === Benchmark 3: Chimp128 only ===
        group.bench_with_input(
            BenchmarkId::new("3_chimp128_only", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress_chimp128_only(black_box(data));
                    black_box(compressed);
                });
            },
        );

        // === Benchmark 4: Chimp128 + Zstd ===
        group.bench_with_input(
            BenchmarkId::new("4_chimp128_zstd", size),
            &data,
            |b, data| {
                b.iter(|| {
                    let compressed = compress_chimp128_zstd(black_box(data));
                    black_box(compressed);
                });
            },
        );

        // === Benchmark 5: Chimp128 + Zstd + Dict ===
        group.bench_with_input(
            BenchmarkId::new("5_chimp128_zstd_dict", size),
            &(data.clone(), dictionary.clone()),
            |b, (data, dict)| {
                b.iter(|| {
                    let compressed = compress_chimp128_zstd_dict(black_box(data), dict);
                    black_box(compressed);
                });
            },
        );

        // === Imprimir Tabla de Ratios ===
        if size == 100_000 {
            println!("\n=== Compression Ratios ({} puntos, temperatura IoT, 85% estabilidad) ===\n", size);

            let raw = compress_raw(&data);
            let raw_zstd = compress_raw_zstd(&data);
            let chimp_only = compress_chimp128_only(&data);
            let chimp_zstd = compress_chimp128_zstd(&data);
            let chimp_zstd_dict = compress_chimp128_zstd_dict(&data, &dictionary);

            println!("{:<25} | {:>10} | {:>10} | {:>10} | {:>10} | {:>12}", "Método", "Bytes", "Bits/Valor", "Ratio", "vs Raw", "vs Raw+Zstd");
            println!("{}", "-".repeat(95));

            let print_row = |name: &str, size: usize, marker: &str| {
                let bits_per_value = (size * 8) as f64 / data.len() as f64;
                let ratio = raw.len() as f64 / size as f64;
                let vs_raw = raw.len() as f64 / size as f64;
                let vs_raw_zstd = raw_zstd.len() as f64 / size as f64;

                println!(
                    "{:<25} | {:>10} | {:>10.2} | {:>9.1}:1 | {:>9.2}x | {:>11.2}x {}",
                    name,
                    format_size(size),
                    bits_per_value,
                    ratio,
                    vs_raw,
                    vs_raw_zstd,
                    marker
                );
            };

            print_row("Raw doubles", raw.len(), "");
            print_row("Raw + Zstd-3", raw_zstd.len(), "");
            print_row("Chimp128 only", chimp_only.len(), "");
            print_row("Chimp128 + Zstd-3", chimp_zstd.len(), "✓");
            print_row("Chimp128 + Zstd + Dict", chimp_zstd_dict.len(), "✓✓");

            println!();
        }
    }

    group.finish();
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_chimp128_stable_data() {
        use super::compress_chimp128_only;
        // Datos 100% estables → ~1 bit/valor
        let data = vec![20.0; 10000];
        let encoded = compress_chimp128_only(&data);

        let bits_per_value = (encoded.len() * 8) as f64 / data.len() as f64;

        println!("Chimp128 stable data: {} bytes, {:.2} bits/valor", encoded.len(), bits_per_value);

        // Para datos constantes, Chimp128 debería usar ~1 bit/valor
        assert!(bits_per_value < 2.0,
                "Chimp128 debería comprimir datos constantes a <2 bits/valor, obtenido: {:.2}",
                bits_per_value);
    }

    #[test]
    fn test_compression_improvement() {
        let data = generate_iot_temperature(100_000, 0.85);

        let raw_zstd = compress_raw_zstd(&data);
        let chimp_zstd = compress_chimp128_zstd(&data);

        let improvement = raw_zstd.len() as f64 / chimp_zstd.len() as f64;

        println!("Raw+Zstd: {} bytes", raw_zstd.len());
        println!("Chimp128+Zstd: {} bytes", chimp_zstd.len());
        println!("Improvement: {:.2}x", improvement);

        assert!(improvement >= 3.0,
                "Debería tener al menos 3x mejora vs Raw+Zstd, obtenido: {:.2}x",
                improvement);
    }

    #[test]
    fn test_chimp128_ratio() {
        let data = generate_iot_temperature(100_000, 0.85);

        let raw = data.len() * 8; // bytes
        let chimp = compress_chimp128_only(&data);

        let ratio = raw as f64 / chimp.len() as f64;

        println!("Raw: {} bytes", raw);
        println!("Chimp128: {} bytes", chimp.len());
        println!("Ratio: {:.2}x", ratio);

        assert!(ratio >= 20.0,
                "Chimp128 solo debería comprimir a 20x+ para datos estables, obtenido: {:.2}x",
                ratio);
    }
}

criterion_group!(benches, compression_benchmark);
criterion_main!(benches);
