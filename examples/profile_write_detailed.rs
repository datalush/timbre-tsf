use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;
use timbre_tsf::encoding::create_encoder;
use timbre_tsf::compress::create_compressor;
use std::time::Instant;

fn main() {
    println!("=== Detailed Write Profiling ===\n");

    // Micro-benchmark: Solo encoding (sin I/O, sin compresión)
    benchmark_encoding_only();

    // Micro-benchmark: Encoding + compresión (sin I/O)
    benchmark_encoding_compression();

    // Full benchmark: Encoding + compresión + I/O
    benchmark_full_write();
}

fn benchmark_encoding_only() {
    println!("1. ENCODING ONLY (Gorilla XOR):");
    let num_values = 100_000;
    let mut total_time = 0u128;

    for run in 0..5 {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut out = Vec::with_capacity(num_values * 4); // Pre-allocate

        let start = Instant::now();
        for i in 0..num_values {
            let value = 25.0 + (i % 100) as f32 * 0.1;
            encoder.encode_f32(value, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();
        let elapsed = start.elapsed();

        total_time += elapsed.as_micros();
        println!("  Run {}: {:?} (size: {} bytes)", run + 1, elapsed, out.len());
    }

    let avg = total_time / 5;
    println!("  Promedio: {}µs ({:.2}ms)\n", avg, avg as f64 / 1000.0);
}

fn benchmark_encoding_compression() {
    println!("2. ENCODING + COMPRESSION (Gorilla + LZ4):");
    let num_values = 100_000;
    let mut total_time = 0u128;

    for run in 0..5 {
        let mut encoder = create_encoder(TSEncoding::Gorilla, TSDataType::Float);
        let mut encoded = Vec::with_capacity(num_values * 4);

        let start = Instant::now();
        // Encode
        for i in 0..num_values {
            let value = 25.0 + (i % 100) as f32 * 0.1;
            encoder.encode_f32(value, &mut encoded).unwrap();
        }
        encoder.flush(&mut encoded).unwrap();

        // Compress
        let mut compressor = create_compressor(CompressionType::Lz4);
        let compressed = compressor.compress(&encoded).unwrap();
        let elapsed = start.elapsed();

        total_time += elapsed.as_micros();
        println!("  Run {}: {:?} (compressed: {} → {} bytes, ratio: {:.2}x)",
                 run + 1, elapsed, encoded.len(), compressed.len(),
                 encoded.len() as f64 / compressed.len() as f64);
    }

    let avg = total_time / 5;
    println!("  Promedio: {}µs ({:.2}ms)\n", avg, avg as f64 / 1000.0);
}

fn benchmark_full_write() {
    println!("3. FULL WRITE (Encoding + Compression + I/O):");
    let path = "/tmp/profile_write_detailed.ts";
    let num_rows = 100_000;
    let mut total_time = 0u128;

    for run in 0..5 {
        let start = Instant::now();
        write_tsfile(path, num_rows);
        let elapsed = start.elapsed();

        total_time += elapsed.as_micros();
        println!("  Run {}: {:?}", run + 1, elapsed);

        let _ = std::fs::remove_file(path);
    }

    let avg = total_time / 5;
    println!("  Promedio: {}µs ({:.2}ms)\n", avg, avg as f64 / 1000.0);
}

fn write_tsfile(path: &str, total_rows: usize) {
    let _ = std::fs::remove_file(path);
    let mut writer = TsFileWriter::new(path).unwrap();

    // 5 devices × 3 measurements
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);
        for measurement in ["temperature", "pressure", "humidity"] {
            let schema = MeasurementSchema::new(
                measurement,
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            );
            writer.register_timeseries(&device_id, schema).unwrap();
        }
    }

    // Escribir rows_per_device rows por cada dispositivo
    let rows_per_device = total_rows / 5;
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);
        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            let record = TsRecord::new(timestamp, &device_id)
                .with_value("temperature", TsValue::Float(25.0 + (i % 100) as f32 * 0.1))
                .with_value("pressure", TsValue::Float(1013.25 + (i % 50) as f32 * 0.5))
                .with_value("humidity", TsValue::Float(60.0 + (i % 40) as f32 * 0.25));
            writer.write_record(record).unwrap();
        }
    }

    writer.close().unwrap();
}
