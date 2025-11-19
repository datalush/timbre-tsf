/// Compara diferentes configuraciones de encoding/compresión
/// para entender la ventaja de Gorilla encoding en TsFile
use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::{EnabledStatistics, WriterProperties};
use parquet::basic::{Compression, Encoding};
use std::sync::Arc;
use std::time::Instant;
use tempfile::NamedTempFile;
use tsfile::arrow::{ArrowConversionConfig, ArrowToTsFileConverter};
use tsfile::common::{CompressionType, TSEncoding};

fn generate_timeseries_data(num_rows: usize) -> RecordBatch {
    let mut timestamps = Vec::with_capacity(num_rows);
    let mut device_ids = Vec::with_capacity(num_rows);
    let mut temperatures = Vec::with_capacity(num_rows);
    let mut humidity = Vec::with_capacity(num_rows);
    let mut pressure = Vec::with_capacity(num_rows);

    let devices = ["sensor_1", "sensor_2", "sensor_3", "sensor_4", "sensor_5"];
    let base_time = 1700000000000i64;

    // Generar datos REALISTAS de series temporales (cambios pequeños entre muestras)
    let mut last_temp = [20.0, 21.0, 22.0, 23.0, 24.0];
    let mut last_humid = [50.0, 51.0, 52.0, 53.0, 54.0];
    let mut last_press = [1013.0, 1014.0, 1015.0, 1016.0, 1017.0];

    for i in 0..num_rows {
        timestamps.push(base_time + (i as i64 * 1000));
        let device_idx = i % devices.len();
        device_ids.push(devices[device_idx]);

        // Cambios pequeños (típico de series temporales reales)
        last_temp[device_idx] += (i as f32 * 0.001).sin() * 0.1;
        last_humid[device_idx] += (i as f32 * 0.002).cos() * 0.2;
        last_press[device_idx] += (i as f32 * 0.0005).sin() * 0.05;

        temperatures.push(last_temp[device_idx]);
        humidity.push(last_humid[device_idx]);
        pressure.push(last_press[device_idx]);
    }

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

fn benchmark_parquet(batch: &RecordBatch, compression: Compression, use_byte_stream_split: bool) -> (f64, usize) {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    let mut props_builder = WriterProperties::builder()
        .set_compression(compression)
        .set_statistics_enabled(EnabledStatistics::None);

    if use_byte_stream_split {
        // Habilitar BYTE_STREAM_SPLIT para columnas Float32
        props_builder = props_builder
            .set_column_encoding("temperature".into(), Encoding::BYTE_STREAM_SPLIT)
            .set_column_encoding("humidity".into(), Encoding::BYTE_STREAM_SPLIT)
            .set_column_encoding("pressure".into(), Encoding::BYTE_STREAM_SPLIT);
    }

    let props = props_builder.build();

    let iterations = 10;
    let mut times = Vec::new();

    for _ in 0..iterations {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props.clone())).unwrap();

        let start = Instant::now();
        writer.write(batch).unwrap();
        writer.close().unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let avg_time = times.iter().sum::<f64>() / times.len() as f64;
    let file_size = std::fs::metadata(path).unwrap().len() as usize;

    (avg_time, file_size)
}

fn benchmark_tsfile(batch: &RecordBatch, encoding: TSEncoding, compression: CompressionType) -> (f64, usize) {
    let temp_file = NamedTempFile::new().unwrap();
    let path = temp_file.path();

    let config = ArrowConversionConfig {
        default_encoding_f32: encoding,
        default_encoding_f64: encoding,
        default_compression: compression,
        ..Default::default()
    };

    let iterations = 10;
    let mut times = Vec::new();

    for _ in 0..iterations {
        let mut converter = ArrowToTsFileConverter::new(path)
            .with_device_column("device_id")
            .with_timestamp_column("timestamp")
            .with_config(config.clone())
            .build()
            .unwrap();

        let start = Instant::now();
        converter.write_batch(batch).unwrap();
        converter.finish().unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    let avg_time = times.iter().sum::<f64>() / times.len() as f64;
    let file_size = std::fs::metadata(path).unwrap().len() as usize;

    (avg_time, file_size)
}

fn main() {
    println!("Generando 100,000 filas de datos de series temporales...\n");
    let batch = generate_timeseries_data(100_000);

    println!("┌────────────────────────────────────────────────────────────────────┐");
    println!("│           COMPARACIÓN: GORILLA vs BYTE_STREAM_SPLIT               │");
    println!("├────────────────────────────────────────────────────────────────────┤");
    println!("│ Configuración                    │  Tiempo    │  Tamaño   │ Ratio │");
    println!("├────────────────────────────────────────────────────────────────────┤");

    // PARQUET: Diferentes configuraciones
    let configs = vec![
        ("Parquet (Plain + Snappy)", Compression::SNAPPY, false),
        ("Parquet (ByteStreamSplit + Snappy)", Compression::SNAPPY, true),
        ("Parquet (ByteStreamSplit + LZ4)", Compression::LZ4, true),
        ("Parquet (ByteStreamSplit + ZSTD)", Compression::ZSTD(Default::default()), true),
    ];

    let mut results = Vec::new();

    for (name, compression, use_bss) in configs {
        let (time, size) = benchmark_parquet(&batch, compression, use_bss);
        results.push((name, time, size));
        println!("│ {:<32} │ {:>7.2} ms │ {:>6} KB │       │", name, time, size / 1024);
    }

    // TSFILE: Diferentes configuraciones
    let tsfile_configs = vec![
        ("TsFile (Plain + Uncompressed)", TSEncoding::Plain, CompressionType::Uncompressed),
        ("TsFile (Plain + LZ4)", TSEncoding::Plain, CompressionType::Lz4),
        ("TsFile (Gorilla + Uncompressed)", TSEncoding::Gorilla, CompressionType::Uncompressed),
        ("TsFile (Gorilla + LZ4)", TSEncoding::Gorilla, CompressionType::Lz4),
    ];

    for (name, encoding, compression) in tsfile_configs {
        let (time, size) = benchmark_tsfile(&batch, encoding, compression);
        results.push((name, time, size));
        println!("│ {:<32} │ {:>7.2} ms │ {:>6} KB │       │", name, time, size / 1024);
    }

    println!("└────────────────────────────────────────────────────────────────────┘\n");

    // Análisis de compresión
    println!("═══════════════════════════════════════════════════════════════════");
    println!("                    ANÁLISIS DE COMPRESIÓN                         ");
    println!("═══════════════════════════════════════════════════════════════════\n");

    let baseline_size = results[0].2;

    for (name, time, size) in &results {
        let ratio = baseline_size as f64 / *size as f64;
        let speedup = if ratio < 1.0 {
            format!("{:.1}x más grande", 1.0 / ratio)
        } else {
            format!("{:.1}x mejor compresión", ratio)
        };
        println!("  {} → {}", name, speedup);
    }

    // Mejor combinación velocidad/compresión
    println!("\n═══════════════════════════════════════════════════════════════════");
    println!("                       RECOMENDACIONES                             ");
    println!("═══════════════════════════════════════════════════════════════════\n");

    let gorilla_lz4 = results.iter().find(|r| r.0 == "TsFile (Gorilla + LZ4)").unwrap();
    let parquet_snappy = results.iter().find(|r| r.0 == "Parquet (Plain + Snappy)").unwrap();

    let compression_ratio = parquet_snappy.2 as f64 / gorilla_lz4.2 as f64;
    let speed_ratio = gorilla_lz4.1 / parquet_snappy.1;

    println!("  📊 Para SERIES TEMPORALES (datos con cambios pequeños):");
    println!("     → TsFile (Gorilla + LZ4): {:.1}x mejor compresión", compression_ratio);
    println!("     → Pero {:.1}x más lento en escritura\n", speed_ratio);

    println!("  ⚡ Para MÁXIMA VELOCIDAD:");
    println!("     → TsFile (Plain + Uncompressed): máxima velocidad");
    println!("     → Parquet (ByteStreamSplit + Snappy): buena velocidad\n");

    println!("  🎯 BALANCE ÓPTIMO:");
    println!("     → TsFile (Gorilla + LZ4): mejor para almacenamiento largo plazo");
    println!("     → Parquet (ByteStreamSplit + LZ4): bueno para análisis rápido\n");
}
