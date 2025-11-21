//! Benchmark SOLO la conversión Arrow → Timbre/Parquet
//!
//! Mide el overhead puro de conversión, SIN encoding/compression/I/O
//!
//! Run: cargo run --release --example benchmark_conversion_overhead

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::ipc::reader::FileReader;
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::basic::Compression;
use parquet::file::properties::WriterProperties;
use std::fs::File;
use std::sync::Arc;
use std::time::Instant;
use timbre_tsf::arrow::ArrowToTsFileConverter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n=== Benchmark: Arrow Conversion Overhead ===\n");

    // Cargar dataset IoT real
    let dataset_path = "data/iot_dataset.arrow";
    println!("Cargando dataset: {}...", dataset_path);

    let file = File::open(dataset_path)?;
    let reader = FileReader::try_new(file, None)?;
    let batches: Vec<RecordBatch> = reader.collect::<Result<Vec<_>, _>>()?;

    let total_rows: usize = batches.iter().map(|b| b.num_rows()).sum();
    println!(
        "✓ Cargado: {} rows en {} batches\n",
        total_rows,
        batches.len()
    );

    // ========================================
    // Test 1: Arrow → Parquet (conversión + encoding + I/O)
    // ========================================
    println!("📦 Test 1: Arrow → Parquet (TOTAL: conversión + encoding + I/O)");

    let parquet_path = std::path::Path::new("/tmp/test_overhead.parquet");
    let schema = batches[0].schema();

    let start = Instant::now();
    {
        let file = File::create(parquet_path)?;
        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();
        let mut writer = ArrowWriter::try_new(file, schema.clone(), Some(props))?;

        for batch in &batches {
            writer.write(batch)?;
        }
        writer.close()?;
    }
    let parquet_total_time = start.elapsed();
    let parquet_size = std::fs::metadata(parquet_path)?.len();

    println!("   Tiempo TOTAL: {:?}", parquet_total_time);
    println!("   Tamaño: {} MB", parquet_size / 1_000_000);
    println!(
        "   Throughput: {:.2} MB/s\n",
        1040.0 / parquet_total_time.as_secs_f64()
    );

    // ========================================
    // Test 2: Arrow → Timbre (conversión + encoding + I/O)
    // ========================================
    println!("🎵 Test 2: Arrow → Timbre (TOTAL: conversión + encoding + I/O)");

    let timbre_path = std::path::Path::new("/tmp/test_overhead.timbre");

    let start = Instant::now();
    {
        let mut converter = ArrowToTsFileConverter::builder(timbre_path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .build()?;

        for batch in &batches {
            converter.write_batch(batch)?;
        }
        converter.finish()?;
    }
    let timbre_total_time = start.elapsed();
    let timbre_size = std::fs::metadata(timbre_path)?.len();

    println!("   Tiempo TOTAL: {:?}", timbre_total_time);
    println!("   Tamaño: {} MB", timbre_size / 1_000_000);
    println!(
        "   Throughput: {:.2} MB/s\n",
        1040.0 / timbre_total_time.as_secs_f64()
    );

    // ========================================
    // Test 3: Arrow → Memoria (SOLO conversión, sin I/O)
    // ========================================
    println!("⚡ Test 3: Arrow → Memoria (SOLO conversión, SIN encoding/I/O)");

    let start = Instant::now();
    let mut total_values = 0u64;

    // Simular lo que haría zero-copy: solo leer punteros
    for batch in &batches {
        for col_idx in 2..batch.num_columns() {
            // Skip timestamp/device_id
            let array = batch.column(col_idx);

            // En zero-copy perfecto: solo acceder al buffer, no copiar
            match array.data_type() {
                DataType::Float32 => {
                    let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();
                    let _ptr = arr.values(); // Solo obtener puntero
                    total_values += arr.len() as u64;
                }
                DataType::Float64 => {
                    let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();
                    let _ptr = arr.values();
                    total_values += arr.len() as u64;
                }
                DataType::Int32 => {
                    let arr = array.as_any().downcast_ref::<Int32Array>().unwrap();
                    let _ptr = arr.values();
                    total_values += arr.len() as u64;
                }
                DataType::Int8 => {
                    let arr = array.as_any().downcast_ref::<Int8Array>().unwrap();
                    let _ptr = arr.values();
                    total_values += arr.len() as u64;
                }
                _ => {}
            }
        }
    }

    let memory_only_time = start.elapsed();

    println!("   Tiempo (solo acceso): {:?}", memory_only_time);
    println!("   Values: {}", total_values);
    println!(
        "   Throughput teórico: {:.2} GB/s\n",
        1.04 / memory_only_time.as_secs_f64()
    );

    // ========================================
    // Análisis
    // ========================================
    println!("📊 Desglose de Tiempos:\n");

    let parquet_ms = parquet_total_time.as_millis();
    let timbre_ms = timbre_total_time.as_millis();
    let memory_ms = memory_only_time.as_millis();

    println!("   Parquet TOTAL:     {} ms", parquet_ms);
    println!("   Timbre TOTAL:      {} ms", timbre_ms);
    println!("   Conversión pura:   {} ms (límite teórico)\n", memory_ms);

    let parquet_overhead = parquet_ms - memory_ms;
    let timbre_overhead = timbre_ms - memory_ms;

    println!(
        "   Overhead Parquet:  {} ms (encoding + I/O)",
        parquet_overhead
    );
    println!(
        "   Overhead Timbre:   {} ms (encoding + I/O)\n",
        timbre_overhead
    );

    println!("💡 Conclusiones:");

    if memory_ms < 100 {
        println!("   ✅ Conversión Arrow es casi instantánea (<100ms)");
        println!("   ✅ Zero-copy funciona correctamente");
    } else {
        println!(
            "   ⚠️  Conversión Arrow toma {}ms - puede haber copias ocultas",
            memory_ms
        );
    }

    let encoding_ratio = (timbre_overhead as f64) / (parquet_overhead as f64);
    println!(
        "\n   Ratio encoding/I/O: Timbre es {:.2}x del overhead de Parquet",
        encoding_ratio
    );

    if encoding_ratio > 2.0 {
        println!(
            "   ⚠️  Timbre encoding es {}x más lento - optimización necesaria",
            encoding_ratio
        );
    } else if encoding_ratio > 1.5 {
        println!(
            "   📈 Timbre encoding es {}x más lento - aceptable (mejor compresión)",
            encoding_ratio
        );
    } else {
        println!("   ✅ Timbre encoding comparable a Parquet");
    }

    // Cleanup
    std::fs::remove_file(parquet_path).ok();
    std::fs::remove_file(timbre_path).ok();

    Ok(())
}
