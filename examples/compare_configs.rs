use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

// Arrow imports
use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

// Parquet imports
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

// TsFile imports
use timbre_tsf::arrow::{ArrowToTsFileConverter, ArrowConversionConfig};

/// Generate test data
fn generate_test_data(num_rows: usize) -> RecordBatch {
    let mut timestamps = Vec::with_capacity(num_rows);
    let mut device_ids = Vec::with_capacity(num_rows);
    let mut temperatures = Vec::with_capacity(num_rows);
    let mut humidity = Vec::with_capacity(num_rows);
    let mut pressure = Vec::with_capacity(num_rows);

    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64;

    for i in 0..num_rows {
        timestamps.push(base_time + (i as i64 * 1000));
        device_ids.push(devices[i % devices.len()]);

        let device_offset = (i % devices.len()) as f32 * 5.0;
        temperatures.push(20.0 + device_offset + (i as f32 * 0.001) % 10.0);
        humidity.push(50.0 + device_offset + (i as f32 * 0.002) % 20.0);
        pressure.push(1013.0 + device_offset + (i as f32 * 0.0005) % 50.0);
    }

    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, false),
        Field::new("humidity", DataType::Float32, false),
        Field::new("pressure", DataType::Float32, false),
    ]));

    let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps));
    let device_array = Arc::new(StringArray::from(device_ids));
    let temp_array = Arc::new(Float32Array::from(temperatures));
    let humidity_array = Arc::new(Float32Array::from(humidity));
    let pressure_array = Arc::new(Float32Array::from(pressure));

    RecordBatch::try_new(
        schema,
        vec![
            timestamp_array,
            device_array,
            temp_array,
            humidity_array,
            pressure_array,
        ],
    )
    .unwrap()
}

fn benchmark_parquet(batch: &RecordBatch) -> (f64, usize) {
    let temp_file = NamedTempFile::new().unwrap();

    let start = Instant::now();
    let file = std::fs::File::create(temp_file.path()).unwrap();
    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props)).unwrap();
    writer.write(batch).unwrap();
    writer.close().unwrap();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    let file_size = std::fs::metadata(temp_file.path()).unwrap().len() as usize;
    (elapsed, file_size)
}

fn benchmark_tsfile(batch: &RecordBatch, config: ArrowConversionConfig, _name: &str) -> (f64, usize) {
    let temp_file = NamedTempFile::new().unwrap();

    let start = Instant::now();
    let mut converter = ArrowToTsFileConverter::new(temp_file.path())
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .with_config(config)
        .build()
        .unwrap();
    converter.write_batch(batch).unwrap();
    converter.finish().unwrap();
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;

    let file_size = std::fs::metadata(temp_file.path()).unwrap().len() as usize;
    (elapsed, file_size)
}

fn main() {
    println!("Generating 100,000 rows of test data...\n");
    let batch = generate_test_data(100_000);

    println!("=== BENCHMARKING DIFFERENT CONFIGURATIONS ===\n");

    // Warm up
    for _ in 0..3 {
        let temp_file = NamedTempFile::new().unwrap();
        let mut converter = ArrowToTsFileConverter::new(temp_file.path())
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();
        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();
    }

    println!("Running benchmarks (10 iterations each)...\n");

    // Benchmark Parquet
    let mut parquet_times = Vec::new();
    let mut parquet_size = 0;
    for _ in 0..10 {
        let (time, size) = benchmark_parquet(&batch);
        parquet_times.push(time);
        parquet_size = size;
    }
    let parquet_avg = parquet_times.iter().sum::<f64>() / parquet_times.len() as f64;
    let parquet_min = parquet_times.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    // Benchmark TsFile - Default (Gorilla + LZ4)
    let mut default_times = Vec::new();
    let mut default_size = 0;
    for _ in 0..10 {
        let (time, size) = benchmark_tsfile(&batch, ArrowConversionConfig::default(), "default");
        default_times.push(time);
        default_size = size;
    }
    let default_avg = default_times.iter().sum::<f64>() / default_times.len() as f64;
    let default_min = default_times.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    // Benchmark TsFile - Balanced (Plain + LZ4)
    let mut balanced_times = Vec::new();
    let mut balanced_size = 0;
    for _ in 0..10 {
        let (time, size) = benchmark_tsfile(&batch, ArrowConversionConfig::balanced(), "balanced");
        balanced_times.push(time);
        balanced_size = size;
    }
    let balanced_avg = balanced_times.iter().sum::<f64>() / balanced_times.len() as f64;
    let balanced_min = balanced_times.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    // Benchmark TsFile - Fast (Plain + Uncompressed)
    let mut fast_times = Vec::new();
    let mut fast_size = 0;
    for _ in 0..10 {
        let (time, size) = benchmark_tsfile(&batch, ArrowConversionConfig::fast(), "fast");
        fast_times.push(time);
        fast_size = size;
    }
    let fast_avg = fast_times.iter().sum::<f64>() / fast_times.len() as f64;
    let fast_min = fast_times.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    // Benchmark TsFile - Optimized (Plain + LZ4) - Best of both worlds
    use timbre_tsf::common::{TSEncoding, CompressionType};
    let optimized_config = ArrowConversionConfig::default()
        .with_compression(CompressionType::Lz4)
        .with_f32_encoding(TSEncoding::Plain)
        .with_f64_encoding(TSEncoding::Plain)
        .with_i32_encoding(TSEncoding::Plain)
        .with_i64_encoding(TSEncoding::Plain)
        .with_bool_encoding(TSEncoding::Plain)
        .with_string_encoding(TSEncoding::Plain)
        .with_max_rows_per_chunk(100_000);

    let mut optimized_times = Vec::new();
    let mut optimized_size = 0;
    for _ in 0..10 {
        let (time, size) = benchmark_tsfile(&batch, optimized_config.clone(), "optimized");
        optimized_times.push(time);
        optimized_size = size;
    }
    let optimized_avg = optimized_times.iter().sum::<f64>() / optimized_times.len() as f64;
    let optimized_min = optimized_times.iter().copied().min_by(|a, b| a.partial_cmp(b).unwrap()).unwrap();

    // Print results
    println!("┌──────────────────────────────────────────────────────────────────┐");
    println!("│                    PERFORMANCE COMPARISON                        │");
    println!("├──────────────────────────────────────────────────────────────────┤");
    println!("│ Format             │  Avg Time  │  Min Time  │   File Size      │");
    println!("├──────────────────────────────────────────────────────────────────┤");
    println!("│ Parquet (Snappy)   │ {:>8.2} ms │ {:>8.2} ms │ {:>7} KB       │",
        parquet_avg, parquet_min, parquet_size / 1024);
    println!("│ TsFile (Default)   │ {:>8.2} ms │ {:>8.2} ms │ {:>7} KB ({:>4.1}x) │",
        default_avg, default_min, default_size / 1024,
        parquet_size as f64 / default_size as f64);
    println!("│ TsFile (Balanced)  │ {:>8.2} ms │ {:>8.2} ms │ {:>7} KB ({:>4.1}x) │",
        balanced_avg, balanced_min, balanced_size / 1024,
        parquet_size as f64 / balanced_size as f64);
    println!("│ TsFile (Optimized) │ {:>8.2} ms │ {:>8.2} ms │ {:>7} KB ({:>4.1}x) │",
        optimized_avg, optimized_min, optimized_size / 1024,
        parquet_size as f64 / optimized_size as f64);
    println!("│ TsFile (Fast)      │ {:>8.2} ms │ {:>8.2} ms │ {:>7} KB ({:>4.1}x) │",
        fast_avg, fast_min, fast_size / 1024,
        parquet_size as f64 / fast_size as f64);
    println!("└──────────────────────────────────────────────────────────────────┘");

    println!("\n=== SPEED VS PARQUET ===");
    println!("Default:   {:>6.1}% {} than Parquet",
        ((default_min / parquet_min - 1.0) * 100.0).abs(),
        if default_min < parquet_min { "FASTER" } else { "slower" });
    println!("Balanced:  {:>6.1}% {} than Parquet",
        ((balanced_min / parquet_min - 1.0) * 100.0).abs(),
        if balanced_min < parquet_min { "FASTER" } else { "slower" });
    println!("Optimized: {:>6.1}% {} than Parquet",
        ((optimized_min / parquet_min - 1.0) * 100.0).abs(),
        if optimized_min < parquet_min { "FASTER" } else { "slower" });
    println!("Fast:      {:>6.1}% {} than Parquet",
        ((fast_min / parquet_min - 1.0) * 100.0).abs(),
        if fast_min < parquet_min { "FASTER" } else { "slower" });

    let best_min = optimized_min.min(fast_min);
    let best_name = if optimized_min < fast_min { "Optimized" } else { "Fast" };

    if best_min < parquet_min {
        println!("\n🎉 SUCCESS! TsFile ({}) BEATS Parquet by {:.2} ms!", best_name, parquet_min - best_min);
        println!("   Speedup: {:.1}x faster", parquet_min / best_min);
    } else {
        println!("\n⚠️  Still need {:.2} ms improvement to beat Parquet", best_min - parquet_min);
        println!("   Current gap: {:.1}%", (best_min / parquet_min - 1.0) * 100.0);
    }
}
