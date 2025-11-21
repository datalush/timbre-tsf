/// MICRO-BENCHMARK: Decompose read pipeline into individual operations
///
/// This benchmark measures each phase INDIVIDUALLY to identify the actual bottleneck:
/// - Decompression only (LZ4)
/// - Timestamp decoding only (DeltaOfDelta)
/// - Value decoding only (Gorilla)
/// - Arrow array building only
/// - Complete end-to-end read
///
/// Run with: cargo bench --bench read_pipeline_breakdown
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;
use tempfile::NamedTempFile;
use timbre_tsf::common::*;
use timbre_tsf::compress::{Compressor, Lz4Compressor};
use timbre_tsf::encoding::create_decoder;
use timbre_tsf::reader::TsFileIOReader;
use timbre_tsf::writer::TsFileWriter;

/// Generate a test file for benchmarking
fn generate_test_file(num_rows: usize) -> NamedTempFile {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    let mut writer = TsFileWriter::new(path).unwrap();

    // Single device, single measurement for simplicity
    let schema = MeasurementSchema::new(
        "temperature",
        TSDataType::Float,
        TSEncoding::Gorilla,
        CompressionType::Lz4,
    );
    writer.register_timeseries("device1", schema).unwrap();

    // Write data using tablet (batch)
    let mut tablet = Tablet::new(
        "device1",
        vec![MeasurementSchema::new(
            "temperature",
            TSDataType::Float,
            TSEncoding::Gorilla,
            CompressionType::Lz4,
        )],
        vec![ColumnCategory::Field],
        num_rows,
    );

    for i in 0..num_rows {
        let timestamp = 1000 + i as i64 * 100;
        let value = 25.0 + (i % 100) as f32 * 0.1;
        tablet
            .add_row(timestamp, vec![Some(TsValue::Float(value))])
            .unwrap();
    }

    writer.write_tablet(&tablet).unwrap();
    writer.close().unwrap();

    temp_file
}

/// Extract compressed page data from file for isolated benchmarking
fn extract_compressed_pages(path: &std::path::Path) -> Vec<(Vec<u8>, usize)> {
    // Read the file and extract compressed page data
    // Returns: Vec<(compressed_data, uncompressed_size)>

    let mut io_reader = TsFileIOReader::open(path).unwrap();
    let _chunk = io_reader.read_chunk("device1", "temperature").unwrap();

    // For this benchmark, we'll re-read and extract raw pages
    // In a real scenario, we'd instrument the reader to capture this

    // Workaround: Re-write to memory and capture
    vec![] // Placeholder - would need source modification for exact extraction
}

/// Benchmark: LZ4 decompression only
fn bench_lz4_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("lz4_decompress");
    group.measurement_time(Duration::from_secs(10));

    // Create test data: compress some float arrays
    let mut compressor = Lz4Compressor;

    let sizes = vec![1000, 10_000, 100_000];

    for size in sizes {
        // Generate float data
        let data: Vec<f32> = (0..size).map(|i| 25.0 + (i % 100) as f32 * 0.1).collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(
                data.as_ptr() as *const u8,
                data.len() * std::mem::size_of::<f32>(),
            )
        };

        let compressed = compressor.compress(bytes).unwrap();
        let uncompressed_size = bytes.len();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_values", size)),
            &(compressed, uncompressed_size),
            |b, (comp, uncomp_size)| {
                b.iter(|| {
                    let mut c = Lz4Compressor;
                    let result = c
                        .decompress(black_box(comp), black_box(*uncomp_size))
                        .unwrap();
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Gorilla decoding only (value decoding)
fn bench_gorilla_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("gorilla_decode");
    group.measurement_time(Duration::from_secs(10));

    let sizes = vec![1000, 10_000, 100_000];

    for size in sizes {
        // Encode data first
        use timbre_tsf::encoding::{Encoder, GorillaEncoder};
        let mut encoder = GorillaEncoder::new(TSDataType::Float);
        let mut out = Vec::new();

        for i in 0..size {
            let value = 25.0 + (i % 100) as f32 * 0.1;
            encoder.encode_f32(value, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_values", size)),
            &(out, size),
            |b, (data, count)| {
                b.iter(|| {
                    let mut decoder = create_decoder(TSEncoding::Gorilla, TSDataType::Float);
                    let mut pos = 0;
                    let mut values = Vec::with_capacity(*count);

                    for _ in 0..*count {
                        let v = decoder.read_f32(black_box(data), &mut pos).unwrap();
                        values.push(v);
                    }

                    black_box(values);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: DeltaOfDelta decoding only (timestamp decoding)
fn bench_dod_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("dod_decode");
    group.measurement_time(Duration::from_secs(10));

    let sizes = vec![1000, 10_000, 100_000];

    for size in sizes {
        // Encode timestamps first
        use timbre_tsf::encoding::{DeltaOfDeltaEncoder, Encoder};
        let mut encoder = DeltaOfDeltaEncoder::new(TSDataType::Int64);
        let mut out = Vec::new();

        for i in 0..size {
            let timestamp = 1000 + i as i64 * 100;
            encoder.encode_i64(timestamp, &mut out).unwrap();
        }
        encoder.flush(&mut out).unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_timestamps", size)),
            &(out, size),
            |b, (data, count)| {
                b.iter(|| {
                    let mut decoder = create_decoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
                    let mut pos = 0;
                    let mut timestamps = Vec::with_capacity(*count);

                    while decoder.has_remaining(black_box(data), pos) && timestamps.len() < *count {
                        let ts = decoder.read_i64(black_box(data), &mut pos).unwrap();
                        timestamps.push(ts);
                    }

                    black_box(timestamps);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Arrow array building from Vec
fn bench_arrow_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("arrow_build");
    group.measurement_time(Duration::from_secs(10));

    use arrow::array::{Float32Array, Int64Array};

    let sizes = vec![1000, 10_000, 100_000, 1_000_000];

    for size in sizes {
        let float_data: Vec<f32> = (0..size).map(|i| 25.0 + (i % 100) as f32 * 0.1).collect();
        let int_data: Vec<i64> = (0..size).map(|i| 1000 + i as i64 * 100).collect();

        group.bench_with_input(
            BenchmarkId::new("f32_array", size),
            &float_data,
            |b, data| {
                b.iter(|| {
                    let array = Float32Array::from(black_box(data.clone()));
                    black_box(array);
                });
            },
        );

        group.bench_with_input(BenchmarkId::new("i64_array", size), &int_data, |b, data| {
            b.iter(|| {
                let array = Int64Array::from(black_box(data.clone()));
                black_box(array);
            });
        });
    }

    group.finish();
}

/// Benchmark: Complete end-to-end read
fn bench_end_to_end_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_read");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    let sizes = vec![10_000, 100_000, 1_000_000];

    for size in sizes {
        let temp_file = generate_test_file(size);
        let path = temp_file.path().to_path_buf();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_rows", size)),
            &path,
            |b, path| {
                b.iter(|| {
                    let mut io_reader = TsFileIOReader::open(black_box(path)).unwrap();
                    let chunk = io_reader.read_chunk("device1", "temperature").unwrap();
                    black_box(chunk);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Page read (decompress + decode)
fn bench_page_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("page_read");
    group.measurement_time(Duration::from_secs(10));

    // This would benchmark PageReader::read_page_data() directly
    // but requires setting up PageData manually

    // For now, we'll use chunk read as proxy
    let sizes = vec![10_000, 100_000];

    for size in sizes {
        let temp_file = generate_test_file(size);
        let path = temp_file.path().to_path_buf();

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}_rows", size)),
            &path,
            |b, path| {
                b.iter(|| {
                    let mut io_reader = TsFileIOReader::open(black_box(path)).unwrap();
                    let chunk = io_reader.read_chunk("device1", "temperature").unwrap();
                    black_box(chunk);
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_lz4_decompress,
    bench_gorilla_decode,
    bench_dod_decode,
    bench_arrow_build,
    bench_end_to_end_read,
    bench_page_read,
);

criterion_main!(benches);
