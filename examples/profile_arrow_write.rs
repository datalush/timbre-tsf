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

fn main() {
    println!("Generating 100,000 rows of test data...");
    let batch = generate_test_data(100_000);

    println!("\n=== PROFILING ARROW -> TSFILE ===\n");

    // Warm up
    for _ in 0..3 {
        let temp_file = NamedTempFile::new().unwrap();
        let mut converter = ArrowToTsFileConverter::builder(temp_file.path())
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()
            .unwrap();
        converter.write_batch(&batch).unwrap();
        converter.finish().unwrap();
    }

    println!("Warm-up complete. Starting profiling...\n");

    // Profile multiple iterations
    let iterations = 10;
    let mut times = Vec::new();

    for i in 0..iterations {
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
        converter.write_batch(&batch).unwrap();
        let phase2_time = start_phase.elapsed();

        // PHASE 3: finish
        let start_phase = Instant::now();
        converter.finish().unwrap();
        let phase3_time = start_phase.elapsed();

        let total_time = start_total.elapsed();
        times.push((phase1_time, phase2_time, phase3_time, total_time));

        if i == 0 {
            println!(
                "Iteration {}: {:>7.2} ms (init: {:>6.2}, write: {:>6.2}, finish: {:>6.2})",
                i + 1,
                total_time.as_secs_f64() * 1000.0,
                phase1_time.as_secs_f64() * 1000.0,
                phase2_time.as_secs_f64() * 1000.0,
                phase3_time.as_secs_f64() * 1000.0
            );
        }
    }

    // Calculate statistics
    let avg_phase1 = times
        .iter()
        .map(|(p1, _, _, _)| p1.as_secs_f64())
        .sum::<f64>()
        / times.len() as f64
        * 1000.0;
    let avg_phase2 = times
        .iter()
        .map(|(_, p2, _, _)| p2.as_secs_f64())
        .sum::<f64>()
        / times.len() as f64
        * 1000.0;
    let avg_phase3 = times
        .iter()
        .map(|(_, _, p3, _)| p3.as_secs_f64())
        .sum::<f64>()
        / times.len() as f64
        * 1000.0;
    let avg_total = times
        .iter()
        .map(|(_, _, _, t)| t.as_secs_f64())
        .sum::<f64>()
        / times.len() as f64
        * 1000.0;

    let min_total = times
        .iter()
        .map(|(_, _, _, t)| t.as_secs_f64())
        .min_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap()
        * 1000.0;
    let max_total = times
        .iter()
        .map(|(_, _, _, t)| t.as_secs_f64())
        .max_by(|a, b| a.partial_cmp(b).unwrap())
        .unwrap()
        * 1000.0;

    println!(
        "\n=== PROFILING RESULTS ({} iterations, 100K rows) ===",
        iterations
    );
    println!(
        "Phase 1 (Converter init):  {:>8.2} ms ({:>5.1}%)",
        avg_phase1,
        avg_phase1 / avg_total * 100.0
    );
    println!(
        "Phase 2 (write_batch):     {:>8.2} ms ({:>5.1}%)",
        avg_phase2,
        avg_phase2 / avg_total * 100.0
    );
    println!(
        "Phase 3 (finish):          {:>8.2} ms ({:>5.1}%)",
        avg_phase3,
        avg_phase3 / avg_total * 100.0
    );
    println!("─────────────────────────────────────");
    println!("Average total:             {:>8.2} ms", avg_total);
    println!("Min total:                 {:>8.2} ms", min_total);
    println!("Max total:                 {:>8.2} ms", max_total);
    println!("\n─────────────────────────────────────");
    println!("Target (Parquet):          {:>8.2} ms", 14.99);
    println!(
        "Gap to close:              {:>8.2} ms ({:>5.1}%)",
        avg_total - 14.99,
        (avg_total - 14.99) / 14.99 * 100.0
    );
    println!("─────────────────────────────────────");

    println!("\n=== BOTTLENECK ANALYSIS ===");
    if avg_phase2 / avg_total > 0.7 {
        println!(
            "HOT PATH: write_batch() is the dominant bottleneck ({:.1}%)",
            avg_phase2 / avg_total * 100.0
        );
        println!("  - Focus on: extract_column_bulk, tablet operations, device grouping");
    }
    if avg_phase3 / avg_total > 0.2 {
        println!(
            "SECONDARY: finish() takes {:.1}% of time",
            avg_phase3 / avg_total * 100.0
        );
        println!("  - Focus on: I/O operations, encoding, compression");
    }
}
