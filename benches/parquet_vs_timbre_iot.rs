//! Real IoT dataset benchmark: Parquet (Snappy) vs Timbre (adaptive encoding)
//!
//! Dataset: data/iot_dataset.arrow (1.04 GB)
//! - 20M rows from 40 IoT devices
//! - 9 columns: timestamp, device_id, temp, humidity, pressure, CO2, light, battery, status
//!
//! Measures:
//! 1. Write time Arrow → Parquet (Snappy)
//! 2. Write time Arrow → Timbre (adaptive encoding per column)
//! 3. Compression ratios
//! 4. File sizes
//!
//! Run with: cargo bench --bench parquet_vs_timbre_iot

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use arrow::ipc::reader::FileReader;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::{WriterProperties, WriterVersion};
use parquet::basic::Compression as ParquetCompression;
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use timbre_tsf::arrow::ArrowToTsFileConverter;

const DATASET_PATH: &str = "data/iot_dataset.arrow";

fn load_dataset() -> Vec<RecordBatch> {
    let file = File::open(DATASET_PATH)
        .expect("Failed to open iot_dataset.arrow - run: cargo run --example generate_iot_dataset");

    let reader = FileReader::try_new(file, None)
        .expect("Failed to create Arrow reader");

    reader.collect::<Result<Vec<_>, _>>()
        .expect("Failed to read batches")
}

fn write_parquet_snappy(batches: &[RecordBatch], path: &Path) -> std::io::Result<u64> {
    let schema = batches[0].schema();
    let file = File::create(path)?;

    let props = WriterProperties::builder()
        .set_compression(ParquetCompression::SNAPPY)
        .set_writer_version(WriterVersion::PARQUET_2_0)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for batch in batches {
        writer.write(batch)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    writer.close()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    Ok(std::fs::metadata(path)?.len())
}

fn write_timbre_adaptive(batches: &[RecordBatch], path: &Path) -> std::io::Result<u64> {
    // Default config: LZ4 compression, Gorilla encoding, 10K rows/chunk
    let mut converter = ArrowToTsFileConverter::builder(path)
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for batch in batches {
        converter.write_batch(batch)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    converter.finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    Ok(std::fs::metadata(path)?.len())
}

fn write_timbre_fast(batches: &[RecordBatch], path: &Path) -> std::io::Result<u64> {
    use timbre_tsf::arrow::ArrowConversionConfig;
    use timbre_tsf::common::{CompressionType, TSEncoding};

    // Fast config: Snappy compression, Chimp128 for floats, larger chunks
    let config = ArrowConversionConfig::default()
        .with_compression(CompressionType::Snappy)
        .with_f32_encoding(TSEncoding::Chimp128)
        .with_f64_encoding(TSEncoding::Chimp128)
        .with_string_encoding(TSEncoding::Plain)  // Avoid Dictionary overhead
        .with_max_rows_per_chunk(50_000);  // Larger chunks

    let mut converter = ArrowToTsFileConverter::builder(path)
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .with_config(config)
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    for batch in batches {
        converter.write_batch(batch)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    converter.finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    Ok(std::fs::metadata(path)?.len())
}

fn benchmark_compression(c: &mut Criterion) {
    println!("\n=== Loading IoT dataset ===");
    let batches = load_dataset();
    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();

    println!("✓ Loaded {} rows in {} batches", total_rows, batches.len());
    println!("✓ Source size: 1.04 GB (Arrow IPC)\n");

    let mut group = c.benchmark_group("parquet_vs_timbre_iot");
    group.sample_size(10);  // Fewer samples for large dataset

    // Benchmark Parquet with Snappy
    group.bench_function("parquet_snappy", |b| {
        b.iter(|| {
            let path = Path::new("/tmp/bench.parquet");
            let size = write_parquet_snappy(black_box(&batches), path).unwrap();
            std::fs::remove_file(path).ok();
            size
        })
    });

    // Benchmark Timbre with default config (LZ4, Gorilla, small chunks)
    group.bench_function("timbre_default", |b| {
        b.iter(|| {
            let path = Path::new("/tmp/bench.timbre");
            let size = write_timbre_adaptive(black_box(&batches), path).unwrap();
            std::fs::remove_file(path).ok();
            size
        })
    });

    // Benchmark Timbre with fast config (Snappy, Chimp128, large chunks)
    group.bench_function("timbre_fast", |b| {
        b.iter(|| {
            let path = Path::new("/tmp/bench_fast.timbre");
            let size = write_timbre_fast(black_box(&batches), path).unwrap();
            std::fs::remove_file(path).ok();
            size
        })
    });

    group.finish();

    // Detailed comparison
    println!("\n=== Detailed Comparison ===\n");

    // Parquet
    println!("📦 Parquet (Snappy compression)");
    let parquet_path = Path::new("/tmp/comparison.parquet");
    let start = Instant::now();
    let parquet_size = write_parquet_snappy(&batches, parquet_path).unwrap();
    let parquet_time = start.elapsed();

    println!("   Write time: {:?}", parquet_time);
    println!("   File size:  {} MB", parquet_size / 1_000_000);
    println!("   Ratio:      {:.2}x vs source", 1_040_000_000.0 / parquet_size as f64);
    println!("   Throughput: {:.2} MB/s\n",
             (1040.0 / parquet_time.as_secs_f64()));

    // Timbre Default
    println!("🎵 Timbre (default: LZ4, Gorilla, 10K chunks)");
    let timbre_default_path = Path::new("/tmp/comparison_default.timbre");
    let start = Instant::now();
    let timbre_default_size = write_timbre_adaptive(&batches, timbre_default_path).unwrap();
    let timbre_default_time = start.elapsed();

    println!("   Write time: {:?}", timbre_default_time);
    println!("   File size:  {} MB", timbre_default_size / 1_000_000);
    println!("   Ratio:      {:.2}x vs source", 1_040_000_000.0 / timbre_default_size as f64);
    println!("   Throughput: {:.2} MB/s\n",
             (1040.0 / timbre_default_time.as_secs_f64()));

    // Timbre Fast
    println!("🚀 Timbre (fast: Snappy, Chimp128, 50K chunks)");
    let timbre_fast_path = Path::new("/tmp/comparison_fast.timbre");
    let start = Instant::now();
    let timbre_fast_size = write_timbre_fast(&batches, timbre_fast_path).unwrap();
    let timbre_fast_time = start.elapsed();

    println!("   Write time: {:?}", timbre_fast_time);
    println!("   File size:  {} MB", timbre_fast_size / 1_000_000);
    println!("   Ratio:      {:.2}x vs source", 1_040_000_000.0 / timbre_fast_size as f64);
    println!("   Throughput: {:.2} MB/s\n",
             (1040.0 / timbre_fast_time.as_secs_f64()));

    // Comparison: Parquet vs Timbre Fast
    println!("📊 Comparison: Parquet vs Timbre (fast config)");
    let speedup = parquet_time.as_secs_f64() / timbre_fast_time.as_secs_f64();
    let ratio_improvement = (1_040_000_000.0 / timbre_fast_size as f64) /
                            (1_040_000_000.0 / parquet_size as f64);

    if speedup > 1.0 {
        println!("   ✅ Timbre is {:.2}x FASTER", speedup);
    } else {
        println!("   ⚠️  Parquet is {:.2}x faster", 1.0 / speedup);
    }

    if ratio_improvement > 1.0 {
        println!("   ✅ Timbre compresses {:.2}x BETTER", ratio_improvement);
    } else {
        println!("   ⚠️  Parquet compresses {:.2}x better", 1.0 / ratio_improvement);
    }

    println!("   Size difference: {} MB", (parquet_size as i64 - timbre_fast_size as i64).abs() / 1_000_000);

    // Cleanup
    std::fs::remove_file(parquet_path).ok();
    std::fs::remove_file(timbre_default_path).ok();
    std::fs::remove_file(timbre_fast_path).ok();
}

criterion_group!(benches, benchmark_compression);
criterion_main!(benches);
