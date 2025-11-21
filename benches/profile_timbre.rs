// Manual profiling with instrumented timers
// Run: cargo bench --bench profile_timbre

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use std::sync::Arc;
use std::time::Instant;
use timbre_tsf::arrow::ArrowToTsFileConverter;

macro_rules! time_section {
    ($label:expr, $code:block) => {{
        let start = Instant::now();
        let result = $code;
        let elapsed = start.elapsed();
        println!("  {} took {:.3}ms", $label, elapsed.as_secs_f64() * 1000.0);
        result
    }};
}

fn main() {
    // Create IoT-like data: 5 devices, 2M rows
    let num_devices = 5;
    let rows_per_device = 400_000;
    let total_rows = num_devices * rows_per_device;

    println!(
        "Generating {} rows ({} devices)...",
        total_rows, num_devices
    );

    // Create schema
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

    // Generate data
    let mut timestamps = Vec::with_capacity(total_rows);
    let mut devices = Vec::with_capacity(total_rows);
    let mut temps = Vec::with_capacity(total_rows);
    let mut humidity = Vec::with_capacity(total_rows);
    let mut pressure = Vec::with_capacity(total_rows);

    for i in 0..total_rows {
        let device_id = i % num_devices;
        timestamps.push(1000000 + (i as i64) * 1000);
        devices.push(format!("device{}", device_id));
        temps.push(20.0 + (i as f32) * 0.001 + (device_id as f32) * 5.0);
        humidity.push(60.0 + (i as f32) * 0.0005);
        pressure.push(1013.25 + (i as f32) * 0.0002);
    }

    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMillisecondArray::from(timestamps)),
            Arc::new(StringArray::from(devices)),
            Arc::new(Float32Array::from(temps)),
            Arc::new(Float32Array::from(humidity)),
            Arc::new(Float32Array::from(pressure)),
        ],
    )
    .unwrap();

    println!("Writing to Timbre format...");
    let start = std::time::Instant::now();

    let mut converter = ArrowToTsFileConverter::builder("/tmp/profile_timbre.tsfile")
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();

    converter.write_batch(&batch).unwrap();
    converter.finish().unwrap();

    let elapsed = start.elapsed();
    let mb = (total_rows * 32) as f64 / 1024.0 / 1024.0; // ~32 bytes/row
    println!(
        "Done in {:.2}s ({:.2} MB/s)",
        elapsed.as_secs_f64(),
        mb / elapsed.as_secs_f64()
    );
}
