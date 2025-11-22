/// Benchmark: QueryBuilder fast path vs RecordBatch path
///
/// This benchmark measures the performance improvement of using the QueryBuilder
/// fast path for single-device, single-measurement queries.
///
/// Expected results:
/// - Fast path (query().device().measurement().execute()): 10-20% faster
/// - RecordBatch path (read_next_batch()): baseline
///
/// Run with: cargo bench --bench query_builder_fast_path

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use tempfile::NamedTempFile;
use timbre_tsf::arrow::RecordBatchReader;
use timbre_tsf::common::{CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue};
use timbre_tsf::writer::FileWriter;

/// Create a test Timbre file with IoT sensor data
fn create_test_file(num_rows: usize) -> NamedTempFile {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    let mut writer = FileWriter::new(path).unwrap();

    // Register multiple measurements to simulate realistic IoT scenario
    let temp_schema = MeasurementSchema::new(
        "temperature",
        TSDataType::Float,
        TSEncoding::Gorilla,
        CompressionType::Lz4,
    );
    let humidity_schema = MeasurementSchema::new(
        "humidity",
        TSDataType::Int32,
        TSEncoding::Plain,
        CompressionType::Lz4,
    );
    let pressure_schema = MeasurementSchema::new(
        "pressure",
        TSDataType::Double,
        TSEncoding::Gorilla,
        CompressionType::Lz4,
    );

    writer.register_timeseries("sensor_01", temp_schema).unwrap();
    writer.register_timeseries("sensor_01", humidity_schema).unwrap();
    writer.register_timeseries("sensor_01", pressure_schema).unwrap();

    // Write test data
    for i in 0..num_rows {
        let record = TsRecord::new(1000 + i as i64 * 100, "sensor_01")
            .with_value("temperature", TsValue::Float(20.0 + (i % 50) as f32 * 0.1))
            .with_value("humidity", TsValue::Int32(60 + (i % 40) as i32))
            .with_value("pressure", TsValue::Double(1013.25 + (i % 30) as f64 * 0.5));
        writer.write_record(record).unwrap();
    }

    writer.close().unwrap();
    temp_file
}

/// Benchmark: Fast path (single measurement)
fn bench_fast_path(c: &mut Criterion) {
    let sizes = vec![1000, 10000, 100000];

    for size in sizes {
        let temp_file = create_test_file(size);
        let path = temp_file.path();

        c.bench_with_input(
            BenchmarkId::new("fast_path", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut reader = RecordBatchReader::try_new(path).unwrap();
                    let (timestamps, values) = reader
                        .query()
                        .device("sensor_01")
                        .measurement("temperature")
                        .execute()
                        .unwrap();

                    black_box(timestamps);
                    black_box(values);
                });
            },
        );
    }
}

/// Benchmark: Traditional RecordBatch path (all measurements)
fn bench_record_batch_path(c: &mut Criterion) {
    let sizes = vec![1000, 10000, 100000];

    for size in sizes {
        let temp_file = create_test_file(size);
        let path = temp_file.path();

        c.bench_with_input(
            BenchmarkId::new("record_batch_path", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let mut reader = RecordBatchReader::try_new(path).unwrap();
                    let batch = reader
                        .query()
                        .device("sensor_01")
                        .execute_batch()
                        .unwrap();

                    // Extract same measurement for fair comparison
                    let temp_column = batch.column_by_name("temperature").unwrap();
                    black_box(temp_column);
                });
            },
        );
    }
}

/// Benchmark: Iterator path (baseline - original implementation)
fn bench_iterator_path(c: &mut Criterion) {
    let sizes = vec![1000, 10000, 100000];

    for size in sizes {
        let temp_file = create_test_file(size);
        let path = temp_file.path();

        c.bench_with_input(
            BenchmarkId::new("iterator_baseline", size),
            &size,
            |b, _| {
                b.iter(|| {
                    let reader = RecordBatchReader::try_new(path).unwrap();

                    // Original way: iterate all batches
                    for batch_result in reader {
                        let batch = batch_result.unwrap();
                        let temp_column = batch.column_by_name("temperature").unwrap();
                        black_box(temp_column);
                    }
                });
            },
        );
    }
}

criterion_group!(
    benches,
    bench_fast_path,
    bench_record_batch_path,
    bench_iterator_path
);
criterion_main!(benches);
