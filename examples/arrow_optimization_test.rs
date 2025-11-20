use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;

use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

use timbre_tsf::arrow::ArrowToTsFileConverter;

fn generate_test_batch(num_rows: usize) -> RecordBatch {
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64;

    let timestamps: Vec<i64> = (0..num_rows).map(|i| base_time + (i as i64 * 1000)).collect();
    let device_ids: Vec<&str> = (0..num_rows).map(|i| devices[i % devices.len()]).collect();
    let temperatures: Vec<f32> = (0..num_rows).map(|i| 20.0 + (i as f32 * 0.001) % 10.0).collect();
    let humidity: Vec<f32> = (0..num_rows).map(|i| 50.0 + (i as f32 * 0.002) % 20.0).collect();
    let pressure: Vec<f32> = (0..num_rows).map(|i| 1013.0 + (i as f32 * 0.0005) % 50.0).collect();

    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Timestamp(TimeUnit::Millisecond, None), false),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, false),
        Field::new("humidity", DataType::Float32, false),
        Field::new("pressure", DataType::Float32, false),
    ]));

    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMillisecondArray::from(timestamps)),
            Arc::new(StringArray::from(device_ids)),
            Arc::new(Float32Array::from(temperatures)),
            Arc::new(Float32Array::from(humidity)),
            Arc::new(Float32Array::from(pressure)),
        ],
    )
    .unwrap()
}

fn main() {
    println!("Arrow to TsFile Conversion Performance Test\n");
    println!("============================================\n");

    let test_sizes = vec![10_000, 50_000, 100_000];

    for num_rows in test_sizes {
        println!("Testing with {} rows:", num_rows);

        let batch = generate_test_batch(num_rows);

        // Warmup
        {
            let temp_file = NamedTempFile::new().unwrap();
            let mut converter = ArrowToTsFileConverter::new(temp_file.path())
                .with_device_column("device_id")
                .with_timestamp_column("timestamp")
                .build()
                .unwrap();
            converter.write_batch(&batch).unwrap();
            converter.finish().unwrap();
        }

        // Actual measurement (5 iterations)
        let mut times = Vec::new();
        for _ in 0..5 {
            let temp_file = NamedTempFile::new().unwrap();

            let start = Instant::now();
            let mut converter = ArrowToTsFileConverter::new(temp_file.path())
                .with_device_column("device_id")
                .with_timestamp_column("timestamp")
                .build()
                .unwrap();
            converter.write_batch(&batch).unwrap();
            converter.finish().unwrap();
            let elapsed = start.elapsed();

            times.push(elapsed);
        }

        let avg = times.iter().sum::<std::time::Duration>() / times.len() as u32;
        let min = times.iter().min().unwrap();
        let max = times.iter().max().unwrap();

        println!("  Average: {:.2?}", avg);
        println!("  Min:     {:.2?}", min);
        println!("  Max:     {:.2?}", max);
        println!("  Throughput: {:.0} rows/sec\n", num_rows as f64 / avg.as_secs_f64());
    }
}
