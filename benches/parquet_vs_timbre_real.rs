//! Real-world benchmark: Parquet (Snappy) vs Timbre (adaptive encoding)
//!
//! Uses a real Kaggle dataset (11.5M rows, 79 columns, network traffic data)
//! to compare compression ratios and write speeds.
//!
//! Dataset: data/dataset.arrow (2.84 GB uncompressed)
//! - 79 columns: Int8/16/32, Float32/64, Dictionary
//! - 11,503,556 rows across 11,234 batches
//! - Real network traffic with DDoS/DoS/Benign labels
//!
//! Run with: cargo bench --bench parquet_vs_timbre_real

use arrow::ipc::reader::FileReader;
use arrow::record_batch::RecordBatch;
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression as ParquetCompression;
use parquet::file::properties::{WriterProperties, WriterVersion};
use std::fs::File;
use std::path::Path;
use std::time::Instant;
use timbre_tsf::arrow::ArrowToTsFileConverter;
use timbre_tsf::common::{CompressionType, TSDataType, TSEncoding};

const DATASET_PATH: &str = "data/dataset.arrow";

/// Load the Arrow dataset
fn load_arrow_dataset() -> Vec<RecordBatch> {
    let file =
        File::open(DATASET_PATH).expect("Failed to open dataset.arrow - did you download it?");

    let reader = FileReader::try_new(file, None).expect("Failed to create Arrow IPC reader");

    let mut batches = Vec::new();
    for batch_result in reader {
        batches.push(batch_result.expect("Failed to read batch"));
    }

    println!(
        "✓ Loaded {} batches ({} total rows)",
        batches.len(),
        batches.iter().map(|b| b.num_rows()).sum::<usize>()
    );

    batches
}

/// Write to Parquet with Snappy compression
fn write_parquet_snappy(batches: &[RecordBatch], output_path: &Path) -> std::io::Result<u64> {
    let schema = batches[0].schema();
    let file = File::create(output_path)?;

    // Configure Parquet with Snappy (default compression)
    let props = WriterProperties::builder()
        .set_compression(ParquetCompression::SNAPPY)
        .set_writer_version(WriterVersion::PARQUET_2_0)
        .build();

    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Write all batches
    for batch in batches {
        writer
            .write(batch)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    writer
        .close()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Get file size
    let metadata = std::fs::metadata(output_path)?;
    Ok(metadata.len())
}

/// Write to Timbre with adaptive encoding recommendations
fn write_timbre_adaptive(batches: &[RecordBatch], output_path: &Path) -> std::io::Result<u64> {
    let schema = batches[0].schema();

    // Build converter with automatic encoding selection
    let mut converter = ArrowToTsFileConverter::builder(output_path)
        .with_device_column("Label") // Use Label as device_id (will fail, need to fix)
        .with_timestamp_column("Flow Duration") // Use Flow Duration as timestamp
        .build()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Write all batches
    for batch in batches {
        converter
            .write_batch(batch)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    }

    converter
        .finish()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    // Get file size
    let metadata = std::fs::metadata(output_path)?;
    Ok(metadata.len())
}

fn benchmark_write_formats(c: &mut Criterion) {
    // Load dataset once
    println!("\n=== Loading Arrow dataset ===");
    let batches = load_arrow_dataset();

    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    let num_columns = batches[0].num_columns();

    println!("Dataset: {} rows × {} columns", total_rows, num_columns);
    println!("Source size: 2.84 GB (Arrow IPC)\n");

    let mut group = c.benchmark_group("parquet_vs_timbre");
    group.sample_size(10); // Fewer samples for large dataset

    // Benchmark: Write to Parquet with Snappy
    group.bench_function("parquet_snappy", |b| {
        b.iter(|| {
            let output = Path::new("/tmp/benchmark.parquet");
            let size =
                write_parquet_snappy(black_box(&batches), output).expect("Failed to write Parquet");
            std::fs::remove_file(output).ok();
            size
        })
    });

    // Note: Timbre benchmark commented out until we fix the converter API
    // The current ArrowToTsFileConverter expects device_id and timestamp columns
    // but this dataset doesn't have that structure (it's network traffic, not time series)
    //
    // group.bench_function("timbre_adaptive", |b| {
    //     b.iter(|| {
    //         let output = Path::new("/tmp/benchmark.timbre");
    //         let size = write_timbre_adaptive(black_box(&batches), output)
    //             .expect("Failed to write Timbre");
    //         std::fs::remove_file(output).ok();
    //         size
    //     })
    // });

    group.finish();

    // Manual timing for file size comparison
    println!("\n=== File Size Comparison ===");

    println!("Writing Parquet with Snappy...");
    let parquet_path = Path::new("/tmp/comparison.parquet");
    let start = Instant::now();
    let parquet_size =
        write_parquet_snappy(&batches, parquet_path).expect("Failed to write Parquet");
    let parquet_time = start.elapsed();

    println!(
        "✓ Parquet: {} MB in {:?}",
        parquet_size / 1_000_000,
        parquet_time
    );
    println!(
        "  Ratio: {:.2}x vs source (2.84 GB)",
        2_840_000_000.0 / parquet_size as f64
    );

    // Cleanup
    std::fs::remove_file(parquet_path).ok();

    // Note about Timbre
    println!("\n⚠️  Timbre comparison skipped:");
    println!("    This dataset is not time series (no device_id/timestamp)");
    println!("    Use a proper IoT dataset for Timbre benchmarks");
}

criterion_group!(benches, benchmark_write_formats);
criterion_main!(benches);
