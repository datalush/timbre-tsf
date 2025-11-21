/// Benchmark: Mini-block parallel speedup verification
///
/// This benchmark measures the actual speedup achieved by parallel mini-block
/// decoding compared to sequential decoding.
///
/// Expected results on 8+ core machines:
/// - 4 mini-blocks: 3-4x speedup
/// - 8 mini-blocks: 6-8x speedup
///
/// Run with: cargo bench --bench miniblock_speedup
use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use std::time::Duration;
use timbre_tsf::common::*;
use timbre_tsf::compress::create_compressor;
use timbre_tsf::encoding::create_decoder;
use timbre_tsf::reader::PageReader;
use timbre_tsf::writer::PageWriter;

/// Generate PageData with different number of mini-blocks
fn generate_page_data(num_points: usize, num_miniblocks: usize) -> timbre_tsf::file::PageData {
    let mut writer = PageWriter::new(TSDataType::Float, TSEncoding::Gorilla, CompressionType::Lz4);

    // Override miniblock config
    writer.miniblock_config.miniblocks_per_page = num_miniblocks;

    // Write sequential data
    for i in 0..num_points {
        let timestamp = 1000000 + (i as i64 * 1000);
        let value = 20.0 + (i as f32 * 0.01);
        writer.write_f32(timestamp, value).unwrap();
    }

    writer.finish().unwrap()
}

/// Benchmark: Sequential decoding (process mini-blocks one by one)
fn bench_sequential_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("sequential_decode");
    group.measurement_time(Duration::from_secs(10));

    for num_miniblocks in [4, 8] {
        for size in [10_000, 50_000, 100_000] {
            let page_data = generate_page_data(size, num_miniblocks);

            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{}pts_{}mb", size, num_miniblocks)),
                &page_data,
                |b, data| {
                    b.iter(|| {
                        // Sequential: iterate mini-blocks without parallelism
                        let mut compressor = create_compressor(CompressionType::Lz4);
                        let mut all_timestamps = Vec::new();
                        let mut all_values = Vec::new();

                        for miniblock in &data.miniblocks {
                            // Decompress
                            let time_uncompressed = compressor
                                .decompress(
                                    &miniblock.timestamp_data,
                                    miniblock.header.timestamp_uncompressed_size as usize,
                                )
                                .unwrap();

                            let value_uncompressed = compressor
                                .decompress(
                                    &miniblock.value_data,
                                    miniblock.header.value_uncompressed_size as usize,
                                )
                                .unwrap();

                            // Decode timestamps
                            let mut time_decoder =
                                create_decoder(TSEncoding::DeltaOfDelta, TSDataType::Int64);
                            let mut pos = 0;
                            while time_decoder.has_remaining(&time_uncompressed, pos) {
                                let ts =
                                    time_decoder.read_i64(&time_uncompressed, &mut pos).unwrap();
                                all_timestamps.push(ts);
                            }

                            // Decode values
                            let mut value_decoder =
                                create_decoder(TSEncoding::Gorilla, TSDataType::Float);
                            let mut pos = 0;
                            while value_decoder.has_remaining(&value_uncompressed, pos) {
                                let val = value_decoder
                                    .read_f32(&value_uncompressed, &mut pos)
                                    .unwrap();
                                all_values.push(val);
                            }
                        }

                        black_box((all_timestamps, all_values))
                    });
                },
            );
        }
    }

    group.finish();
}

/// Benchmark: Parallel decoding using PageReader (with Rayon)
fn bench_parallel_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_decode");
    group.measurement_time(Duration::from_secs(10));

    for num_miniblocks in [4, 8] {
        for size in [10_000, 50_000, 100_000] {
            let page_data = generate_page_data(size, num_miniblocks);

            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{}pts_{}mb", size, num_miniblocks)),
                &page_data,
                |b, data| {
                    b.iter(|| {
                        // Parallel: use PageReader's read_page_data (uses Rayon internally)
                        let mut page_reader = PageReader::new(
                            TSDataType::Float,
                            TSEncoding::Gorilla,
                            CompressionType::Lz4,
                        );

                        let decoded = page_reader.read_page_data(black_box(data)).unwrap();
                        black_box(decoded)
                    });
                },
            );
        }
    }

    group.finish();
}

/// Benchmark: Multiple pages decoded in parallel
fn bench_multi_page_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_page_decode");
    group.measurement_time(Duration::from_secs(10));

    for num_pages in [4, 8] {
        let points_per_page = 10_000;

        // Generate multiple pages
        let mut pages = Vec::new();
        for _page_idx in 0..num_pages {
            let page_data = generate_page_data(points_per_page, 8);
            pages.push(page_data);
        }

        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{}pages_{}pts", num_pages, points_per_page)),
            &pages,
            |b, page_list| {
                b.iter(|| {
                    use rayon::prelude::*;

                    // Decode all pages in parallel (like ChunkReader does)
                    let decoded_pages: Vec<_> = page_list
                        .par_iter()
                        .map(|page_data| {
                            let mut page_reader = PageReader::new(
                                TSDataType::Float,
                                TSEncoding::Gorilla,
                                CompressionType::Lz4,
                            );
                            page_reader.read_page_data(page_data).unwrap()
                        })
                        .collect();

                    black_box(decoded_pages)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sequential_decode,
    bench_parallel_decode,
    bench_multi_page_decode,
);
criterion_main!(benches);
