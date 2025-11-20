# Timbre Time Series Format

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

**Timbre** is a high-performance columnar file format for IoT time series data, built in Rust. It combines proven architectural principles from TsFile and Parquet with modern compression innovations, delivering exceptional compression ratios and query performance.

**This is a file format library** - it provides encoding, compression, and I/O primitives with intelligent recommendations, but leaves buffering and database-level decisions to applications.

## What is Timbre?

Timbre (`.timbre` extension) stores time series data in a columnar layout optimized for:

- **IoT sensor data** with high compression needs (typically 10-100x)
- **Analytical queries** over large time ranges
- **Parallel processing** with mini-block architecture
- **Flexible schemas** with per-measurement encoding control

### Key Innovations

- **Adaptive encoding recommendations**: Analyze data patterns and suggest optimal encodings (Quantized, DictionaryRLE, Chimp128)
- **State-of-the-art encodings**: Chimp128 (5-15% better than Gorilla), Simple8b (10-100x), Quantized (26x on regular data)
- **Benchmark-driven compression**: LZ4 for high-entropy data (40x faster), Zstd for compact data (220-284x ratios)
- **PageWriterBuilder pattern**: Idiomatic Rust API with fully automatic, semi-automatic, and manual configuration modes
- **Mini-block parallelism**: 4-8 blocks per page for fine-grained parallel decoding with Rayon

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
timbre-tsf = "1.0.0"
```

### Writing Data

```rust
use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;

// Create writer and register schema
let mut writer = TsFileWriter::new("sensor.timbre")?;
let schema = MeasurementSchema::new(
    "temperature",
    TSDataType::Float,
    TSEncoding::Chimp128,
    CompressionType::Zstd,
);
writer.register_timeseries("device_001", schema)?;

// Write data points
let record = TsRecord::new(1000, "device_001")
    .with_value("temperature", TsValue::Float(25.5));
writer.write_record(record)?;
writer.close()?;
```

### Reading Data

```rust
use timbre_tsf::reader::TsFileReader;

let mut reader = TsFileReader::open("sensor.timbre")?;
let chunk = reader.read("device_001", "temperature")?;

for (timestamp, value) in chunk.iter() {
    println!("{}: {:?}", timestamp, value);
}
```

### Adaptive Encoding (Recommended)

Let Timbre analyze your data and choose the optimal encoding:

```rust
use timbre_tsf::writer::PageWriterBuilder;
use timbre_tsf::common::TSDataType;

// Option 1: Fully automatic (recommended)
let sample_data = vec![20.0, 20.1, 20.2, 20.1, 20.0]; // Quantized pattern
let writer = PageWriterBuilder::new()
    .data_type(TSDataType::Double)
    .analyze_and_recommend(&sample_data)
    .build()?;
// Result: encoding=Quantized, compression=Zstd (optimal for this pattern)

// Option 2: Manual encoding, recommended compression
let writer = PageWriterBuilder::new()
    .data_type(TSDataType::Double)
    .encoding(TSEncoding::Chimp128)
    .compression_recommended()  // Selects LZ4 for Chimp128
    .build()?;

// Option 3: Full manual override
let writer = PageWriterBuilder::new()
    .data_type(TSDataType::Double)
    .encoding(TSEncoding::Chimp128)
    .compression(CompressionType::Zstd)
    .build()?;
```

## Features

### Encodings

Timbre provides 8 specialized encodings optimized for different data patterns:

| Encoding | Best For | Typical Ratio | Notes |
|----------|----------|---------------|-------|
| **Chimp128** | Continuous floating-point | 5-8x | State-of-the-art, 5-15% better than Gorilla |
| **Quantized** | Regular step patterns (0.1°C sensors) | 26x | Lossless for quantized data |
| **DictionaryRLE** | High repetition, discrete values | 10-40x | Combines dictionary + run-length |
| **Simple8b** | Integers with small ranges | 10-100x | Fast integer packing |
| **Gorilla** | Floating-point time series | 3-6x | Facebook's XOR delta encoding |
| **DeltaOfDelta** | Timestamps, monotonic sequences | 8-16x | Second-order differences |
| **RLE** | Repetitive values | 8-16x | Classic run-length encoding |
| **Plain** | High-entropy data | 1x | No encoding overhead |

### Compression

Four compression algorithms with different trade-offs:

| Algorithm | Speed | Ratio | Use Case |
|-----------|-------|-------|----------|
| **Zstd** (default) | Fast | High | Default for most workloads, 2-3x better than Snappy |
| **LZ4** | Very Fast | Medium | Recommended for Chimp128/Gorilla (40x speedup, minimal ratio loss) |
| **Snappy** | Very Fast | Medium | Google's compression, compatibility |
| **GZIP** | Slow | Very High | Maximum compression when space is critical |

### Adaptive Recommendations

Timbre analyzes your data and recommends optimal encoding/compression combinations:

#### Pattern Detection

```rust
use timbre_tsf::encoding::adaptive::recommend_encoding;

// Detects quantization (e.g., 0.1°C resolution)
let sample = vec![20.0, 20.1, 20.2, 20.1, 20.0];
let encoding = recommend_encoding(&sample);
// Returns: TSEncoding::Quantized
```

**Detected patterns:**
- **Quantized**: Regular step patterns → 26x compression with Simple8b
- **High Repetition**: >70% repetition, <256 unique values → 10-40x with DictionaryRLE
- **Continuous Drift**: Small frequent changes → 5-8x with Chimp128
- **High Entropy**: Random/varied data → Chimp128 fallback

#### Compression Recommendations

Based on real benchmarks with 100K data points:

```rust
use timbre_tsf::encoding::adaptive::recommend_compression;

// Compact encodings → Zstd serial
recommend_compression(TSEncoding::Quantized);
// Returns: CompressionType::Zstd
// Rationale: 27KB → 94B (284x ratio), 9µs processing

// High-entropy encodings → LZ4 parallel
recommend_compression(TSEncoding::Chimp128);
// Returns: CompressionType::Lz4
// Rationale: 838KB → 828KB (1.01x ratio), 80µs vs 3.3ms serial (40x speedup)
```

**These are recommendations** - applications can always override with manual configuration.

## Architecture

### Data Model

```
Timbre File (.timbre)
├── File Header (128 bytes, TMB1 magic)
├── Device Groups
│   ├── Series Chunks (per measurement)
│   │   └── Pages (64KB-1MB compressed)
│   │       └── Mini-Blocks (4-8 blocks, parallel decode)
│   └── ...
├── Index Area (ART, inverted index, bloom filters)
└── Footer (128 bytes + TMB1 magic)
```

### Mini-Block Parallelism

Pages are divided into 4-8 mini-blocks that can be decoded in parallel using Rayon:

- **Fine-grained parallelism**: Decode multiple blocks simultaneously
- **Better cache utilization**: Smaller blocks fit in CPU cache
- **Configurable**: Adjust block count and size per workload

### Supported Data Types

| Type | Size | Default Encoding |
|------|------|------------------|
| `BOOLEAN` | 1 byte | RLE |
| `INT32` | 4 bytes | DeltaOfDelta |
| `INT64` | 8 bytes | DeltaOfDelta |
| `FLOAT` | 4 bytes | Chimp128 |
| `DOUBLE` | 8 bytes | Chimp128 |
| `TEXT` | Variable | Plain |
| `TIMESTAMP` | 8 bytes | DeltaOfDelta |

## Performance

### Compression Benchmarks

Real-world IoT sensor data (100K temperature readings, 0.1°C quantization):

| Configuration | Size | Ratio | Time | Notes |
|---------------|------|-------|------|-------|
| Raw (uncompressed) | 800 KB | 1x | - | Baseline |
| Plain + Zstd | 689 KB | 1.16x | 3.3 ms | No encoding |
| Chimp128 + Zstd | 689 KB | 1.22x | 3.3 ms | High entropy |
| **Chimp128 + LZ4** | **828 KB** | **1.01x** | **80 µs** | **40x faster, 1% ratio loss** |
| Quantized + Zstd | 94 B | 284x | 9 µs | **Best for quantized data** |
| DictionaryRLE + Zstd | 97 B | 221x | 9 µs | Best for repetitive data |

**Key Insight**: Encoding matters more than compression algorithm. Choose encoding based on data pattern, then pick compression based on encoded data size.

### Encoding Throughput

Measured on AMD Ryzen 9 5950X, 100K values:

| Encoding | Throughput | Latency |
|----------|------------|---------|
| Plain | 800 MB/s | 1.2 µs |
| DeltaOfDelta | 500 MB/s | 2.0 µs |
| Chimp128 | 400 MB/s | 2.5 µs |
| Gorilla | 350 MB/s | 2.8 µs |
| Quantized | 450 MB/s | 2.2 µs |
| DictionaryRLE | 600 MB/s | 1.7 µs |

## Examples

```bash
# Full write/read workflow
cargo run --example end_to_end

# Adaptive encoding recommendations
cargo run --example encoding_recommendation

# Compare encoding performance
cargo run --example compare_encodings

# Bloom filters and query optimization
cargo run --example bloom_and_filters
```

## Benchmarks

```bash
# Run all benchmarks
cargo bench

# Specific benchmark
cargo bench --bench encoding_compression_tradeoff
cargo bench --bench miniblock_speedup
cargo bench --bench parquet_vs_timbre
```

## Design Philosophy

### This is a File Format Library

Timbre provides:
- **Encoding/decoding primitives** (Chimp128, Quantized, DictionaryRLE, etc.)
- **Compression algorithms** (Zstd, LZ4, Snappy, GZIP)
- **File I/O** (writing/reading `.timbre` files)
- **Analysis tools** (`recommend_encoding`, `recommend_compression`)
- **Indexing primitives** (Bloom filters, ART index)

Timbre **does not** provide:
- Data buffering (applications buffer before calling Timbre)
- Automatic encoding decisions (applications use recommendations and choose)
- Query engines (applications build queries using Timbre's filters)
- Networking or replication

### Recommendations are Tools, Not Mandates

```rust
// Application flow:
// 1. Application buffers data (e.g., 10K points)
// 2. Application samples and calls recommend_encoding()
// 3. Application decides: use recommendation OR override manually
// 4. Application creates encoder and writes data

let sample = buffer.sample(1000);
let recommended_encoding = recommend_encoding(&sample);
let recommended_compression = recommend_compression(recommended_encoding);

// Option A: Use recommendation
let writer = PageWriterBuilder::new()
    .data_type(TSDataType::Double)
    .encoding(recommended_encoding)
    .compression(recommended_compression)
    .build()?;

// Option B: Override (application knows better)
let writer = PageWriterBuilder::new()
    .data_type(TSDataType::Double)
    .encoding(TSEncoding::Plain)  // Force Plain for specific reason
    .compression(CompressionType::Zstd)
    .build()?;
```

## Comparison to Alternatives

| Feature | Timbre | TsFile | Parquet |
|---------|--------|--------|---------|
| Format Focus | IoT time series | Time series | General columnar |
| Modern Encodings | Chimp128, Quantized, Simple8b | Gorilla, RLE | Dictionary, RLE |
| Adaptive Recommendations | ✅ Built-in | ❌ | ❌ |
| Default Compression | Zstd | Snappy | Snappy |
| Mini-block Parallelism | ✅ 4-8 blocks | ❌ | ✅ Row groups |
| File Extension | `.timbre` | `.tsfile` | `.parquet` |
| Language | Rust | Java | C++/Java/Python |
| License | Apache 2.0 | Apache 2.0 | Apache 2.0 |

## Testing

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module
cargo test encoding::chimp128

# Coverage
cargo tarpaulin --out Html
```

## Contributing

Contributions welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Run tests and formatting (`cargo test && cargo fmt`)
4. Commit changes (`git commit -m 'Add amazing feature'`)
5. Push to branch (`git push origin feature/amazing-feature`)
6. Open a Pull Request

### Code Standards

- Format: `cargo fmt`
- Linting: `cargo clippy -- -D warnings`
- Tests: `cargo test`
- Documentation: Add `///` doc comments with examples for public APIs

## License

Apache License 2.0

## Links

- [Crates.io](https://crates.io/crates/timbre-tsf)
- [GitHub Repository](https://github.com/juanjodelasheras/timbre-tsf)

## Authors

Juan José de las Heras Herrera ([@midnattsol](https://github.com/midnattsol))

---

**timbre-tsf** - High-performance time series storage for Rust
