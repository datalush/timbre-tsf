use tsfile::common::*;
use tsfile::writer::TsFileWriter;
use tsfile::encoding::{Encoder, create_encoder};
use tsfile::compress::create_compressor;
use std::time::Instant;

fn main() {
    println!("=== Análisis del Bottleneck de Escritura ===\n");

    // Test 1: Solo Encoding (sin I/O, sin compresión)
    println!("1. ENCODING GORILLA (10K valores):");
    let num_values = 10_000;
    let mut total_encoding = 0u128;

    for _ in 0..10 {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::new();

        let start = Instant::now();
        for i in 0..num_values {
            let value = 25.0 + (i % 100) as f32 * 0.1;
            encoder.encode_f32(value, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        total_encoding += start.elapsed().as_micros();
    }
    println!("   Promedio: {}µs ({:.2}ms)", total_encoding / 10, total_encoding as f64 / 10_000.0);
    println!("   Por valor: {:.2}ns\n", (total_encoding as f64 / 10.0) / num_values as f64);

    // Test 2: Encoding + Compresión
    println!("2. ENCODING + COMPRESSION LZ4 (10K valores):");
    let mut total_enc_comp = 0u128;

    for _ in 0..10 {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut encoded = Vec::new();

        let start = Instant::now();
        for i in 0..num_values {
            let value = 25.0 + (i % 100) as f32 * 0.1;
            encoder.encode_f32(value, &mut encoded).unwrap();
        }
        encoder.flush(&mut encoded).unwrap();

        let mut compressor = create_compressor(CompressionType::Lz4);
        let _compressed = compressor.compress(&encoded).unwrap();
        total_enc_comp += start.elapsed().as_micros();
    }
    println!("   Promedio: {}µs ({:.2}ms)", total_enc_comp / 10, total_enc_comp as f64 / 10_000.0);
    println!("   Overhead compresión: {}µs\n", (total_enc_comp - total_encoding) / 10);

    // Test 3: Write completo (encoding + compression + I/O)
    println!("3. WRITE COMPLETO (10K rows × 3 measurements):");
    let path = "/tmp/analyze_bottleneck.ts";
    let mut total_write = 0u128;

    for run in 0..5 {
        let _ = std::fs::remove_file(path);

        let start = Instant::now();
        let mut writer = TsFileWriter::new(path).unwrap();

        let device_id = "device_1";
        let schemas = vec![
            MeasurementSchema::new("temp", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
            MeasurementSchema::new("press", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
            MeasurementSchema::new("humid", TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4),
        ];

        for schema in &schemas {
            writer.register_timeseries(device_id, schema.clone()).unwrap();
        }

        let mut tablet = Tablet::new(
            device_id,
            schemas.clone(),
            vec![ColumnCategory::Field; 3],
            10_000,
        );

        for i in 0..10_000 {
            tablet.add_row(
                i as i64 * 100,
                vec![
                    Some(TsValue::Float(25.0 + (i % 100) as f32 * 0.1)),
                    Some(TsValue::Float(1013.0 + (i % 50) as f32 * 0.5)),
                    Some(TsValue::Float(60.0 + (i % 40) as f32 * 0.25)),
                ],
            ).unwrap();
        }

        writer.write_tablet(&tablet).unwrap();
        writer.close().unwrap();

        let elapsed = start.elapsed().as_micros();
        total_write += elapsed;
        println!("   Run {}: {}µs ({:.2}ms)", run + 1, elapsed, elapsed as f64 / 1000.0);
    }

    let avg_write = total_write / 5;
    println!("   Promedio: {}µs ({:.2}ms)\n", avg_write, avg_write as f64 / 1000.0);

    // Análisis
    let encoding_per_30k = (total_encoding / 10) * 3; // 3 measurements
    let compression_overhead_per_30k = ((total_enc_comp - total_encoding) / 10) * 3;
    let io_and_overhead = avg_write - encoding_per_30k - compression_overhead_per_30k;

    println!("=== DESGLOSE DEL TIEMPO (para 10K rows × 3 measurements) ===");
    println!("Encoding (Gorilla):      {}µs ({:.1}%)", encoding_per_30k,
        encoding_per_30k as f64 / avg_write as f64 * 100.0);
    println!("Compression (LZ4):       {}µs ({:.1}%)", compression_overhead_per_30k,
        compression_overhead_per_30k as f64 / avg_write as f64 * 100.0);
    println!("I/O + Framework overhead: {}µs ({:.1}%)", io_and_overhead,
        io_and_overhead as f64 / avg_write as f64 * 100.0);
    println!("Total:                    {}µs (100.0%)\n", avg_write);

    println!("=== CONCLUSIÓN ===");
    if io_and_overhead as f64 / avg_write as f64 > 0.4 {
        println!("⚠️  EL BOTTLENECK ES I/O + FRAMEWORK OVERHEAD (>40%)");
        println!("    Optimizaciones recomendadas:");
        println!("    - Buffer pooling para reducir allocations");
        println!("    - Batch I/O writes con BufWriter más grande");
        println!("    - Eliminar overhead de HashMap lookups");
    } else if encoding_per_30k as f64 / avg_write as f64 > 0.4 {
        println!("⚠️  EL BOTTLENECK ES ENCODING (>40%)");
        println!("    Optimizaciones recomendadas:");
        println!("    - SIMD para Gorilla encoding");
        println!("    - Enum dispatch para eliminar vtable overhead");
    } else {
        println!("⚠️  EL BOTTLENECK ES COMPRESSION (>40%)");
        println!("    Considerar compresión más rápida o async compression");
    }

    std::fs::remove_file(path).ok();
}
