use criterion::{Criterion, black_box, criterion_group, criterion_main};
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

// Arrow imports
use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

// TsFile imports
use timbre_tsf::arrow::ArrowToTsFileConverter;

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

/// Profile Arrow -> TsFile with detailed timing
fn profile_arrow_tsfile(batch: &RecordBatch) {
    let temp_file = NamedTempFile::new().unwrap();

    let start_total = Instant::now();

    // PHASE 1: Converter creation
    let start_phase = Instant::now();
    let mut converter = ArrowToTsFileConverter::builder(temp_file.path())
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();
    let phase1_time = start_phase.elapsed();

    // PHASE 2: write_batch (critical hot path)
    let start_phase = Instant::now();
    converter.write_batch(batch).unwrap();
    let phase2_time = start_phase.elapsed();

    // PHASE 3: finish
    let start_phase = Instant::now();
    converter.finish().unwrap();
    let phase3_time = start_phase.elapsed();

    let total_time = start_total.elapsed();

    eprintln!("\n=== PROFILING RESULTS (100K rows) ===");
    eprintln!(
        "Phase 1 (Converter init):  {:>8.2} ms ({:>5.1}%)",
        phase1_time.as_secs_f64() * 1000.0,
        phase1_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    eprintln!(
        "Phase 2 (write_batch):     {:>8.2} ms ({:>5.1}%)",
        phase2_time.as_secs_f64() * 1000.0,
        phase2_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    eprintln!(
        "Phase 3 (finish):          {:>8.2} ms ({:>5.1}%)",
        phase3_time.as_secs_f64() * 1000.0,
        phase3_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    eprintln!(
        "Total time:                {:>8.2} ms",
        total_time.as_secs_f64() * 1000.0
    );
    eprintln!("Target (Parquet):          {:>8.2} ms", 14.99);
    eprintln!(
        "Gap to close:              {:>8.2} ms",
        total_time.as_secs_f64() * 1000.0 - 14.99
    );
}

fn benchmark_profile(c: &mut Criterion) {
    let batch = generate_test_data(100_000);

    // Run profiling once to print results
    profile_arrow_tsfile(&batch);

    let mut group = c.benchmark_group("arrow_to_tsfile");
    group.sample_size(20);

    group.bench_function("100k_rows", |b| {
        b.iter(|| {
            let temp_file = NamedTempFile::new().unwrap();
            let mut converter = ArrowToTsFileConverter::builder(temp_file.path())
                .with_device_column("device_id")
                .with_timestamp_column("timestamp")
                .build()
                .unwrap();

            converter.write_batch(black_box(&batch)).unwrap();
            converter.finish().unwrap();
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_profile);
criterion_main!(benches);
