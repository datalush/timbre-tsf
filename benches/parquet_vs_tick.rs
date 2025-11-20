use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;
use std::time::Duration;
use tempfile::NamedTempFile;

// Arrow imports
use arrow::array::{Float32Array, StringArray, TimestampMillisecondArray};
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::record_batch::RecordBatch;

// Parquet imports
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;

// TsFile imports
use timbre_tsf::arrow::{ArrowToTsFileConverter, TsFileRecordBatchReader};
use timbre_tsf::common::{ColumnCategory, CompressionType, MeasurementSchema, TSDataType, TSEncoding, Tablet, TsValue};
use timbre_tsf::writer::TsFileWriter;

/// Genera datos de prueba con 1M de filas
fn generate_test_data(num_rows: usize) -> RecordBatch {
    println!("Generando {} filas de datos de prueba...", num_rows);

    let mut timestamps = Vec::with_capacity(num_rows);
    let mut device_ids = Vec::with_capacity(num_rows);
    let mut temperatures = Vec::with_capacity(num_rows);
    let mut humidity = Vec::with_capacity(num_rows);
    let mut pressure = Vec::with_capacity(num_rows);

    // Generar datos realistas
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];
    let base_time = 1700000000000i64; // Base timestamp en milisegundos

    for i in 0..num_rows {
        timestamps.push(base_time + (i as i64 * 1000)); // 1 segundo entre muestras
        device_ids.push(devices[i % devices.len()]);

        // Datos simulados con algo de variación
        let device_offset = (i % devices.len()) as f32 * 5.0;
        temperatures.push(20.0 + device_offset + (i as f32 * 0.001) % 10.0);
        humidity.push(50.0 + device_offset + (i as f32 * 0.002) % 20.0);
        pressure.push(1013.0 + device_offset + (i as f32 * 0.0005) % 50.0);
    }

    // Crear schema de Arrow
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

    // Crear arrays
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

/// Escribe datos a un archivo Parquet
fn write_parquet(batch: &RecordBatch, path: &std::path::Path) {
    println!("Escribiendo {} filas a Parquet...", batch.num_rows());

    let file = std::fs::File::create(path).unwrap();

    let props = WriterProperties::builder()
        .set_compression(parquet::basic::Compression::SNAPPY)
        .build();

    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props)).unwrap();

    writer.write(batch).unwrap();
    writer.close().unwrap();

    println!("Parquet escrito: {} bytes", std::fs::metadata(path).unwrap().len());
}

/// Escribe datos a un archivo TsFile usando el writer nativo
fn write_tsfile_native(batch: &RecordBatch, path: &std::path::Path) {
    use std::time::Instant;

    let start_total = Instant::now();
    log::info!("Escribiendo {} filas a TsFile (nativo)...", batch.num_rows());

    let mut writer = TsFileWriter::new(path).unwrap();
    log::info!("  Writer creado ({:?})", start_total.elapsed());

    // Registrar schemas para cada dispositivo
    let devices = ["device_1", "device_2", "device_3", "device_4", "device_5"];

    for device in &devices {
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new(
                    "temperature",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            )
            .unwrap();
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new(
                    "humidity",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            )
            .unwrap();
        writer
            .register_timeseries(
                *device,
                MeasurementSchema::new(
                    "pressure",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            )
            .unwrap();
    }
    log::info!("  Schemas registrados para {} dispositivos ({:?})", devices.len(), start_total.elapsed());

    // Verificar que los schemas tienen Gorilla + LZ4
    log::debug!("  Schema temperature: encoding={:?}, compression={:?}",
        TSEncoding::Gorilla, CompressionType::Lz4);

    // Extraer datos del batch
    let timestamp_array = batch
        .column(0)
        .as_any()
        .downcast_ref::<TimestampMillisecondArray>()
        .unwrap();
    let device_array = batch
        .column(1)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let temp_array = batch
        .column(2)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();
    let humidity_array = batch
        .column(3)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();
    let pressure_array = batch
        .column(4)
        .as_any()
        .downcast_ref::<Float32Array>()
        .unwrap();

    log::info!("  Arrays extraídos ({:?})", start_total.elapsed());

    // AGRUPAR POR DEVICE PRIMERO para evitar cambios constantes de device
    use std::collections::HashMap;
    let mut rows_by_device: HashMap<String, Vec<(i64, f32, f32, f32)>> = HashMap::new();

    let num_rows = batch.num_rows();
    for i in 0..num_rows {
        let timestamp = timestamp_array.value(i);
        let device_id = device_array.value(i).to_string();
        let temperature = temp_array.value(i);
        let humidity = humidity_array.value(i);
        let pressure = pressure_array.value(i);

        rows_by_device
            .entry(device_id)
            .or_insert_with(Vec::new)
            .push((timestamp, temperature, humidity, pressure));
    }

    log::info!("  Datos agrupados en {} devices ({:?})", rows_by_device.len(), start_total.elapsed());

    // Escribir cada device usando batch writing con Tablets (3.8x más rápido!)
    for (device_id, rows) in rows_by_device {
        log::info!("  Escribiendo device {}: {} rows (usando Tablet)", device_id, rows.len());

        // Crear schemas para este device
        let schemas = vec![
            MeasurementSchema::new(
                "temperature",
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            ),
            MeasurementSchema::new(
                "humidity",
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            ),
            MeasurementSchema::new(
                "pressure",
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            ),
        ];

        // Crear tablet para batch writing
        let mut tablet = Tablet::new(
            &device_id,
            schemas,
            vec![ColumnCategory::Field; 3],
            rows.len(),
        );

        // Agregar todos los rows al tablet
        for (timestamp, temperature, humidity, pressure) in rows {
            tablet
                .add_row(
                    timestamp,
                    vec![
                        Some(TsValue::Float(temperature)),
                        Some(TsValue::Float(humidity)),
                        Some(TsValue::Float(pressure)),
                    ],
                )
                .unwrap();
        }

        // Escribir todo el tablet de una vez (mucho más rápido!)
        writer.write_tablet(&tablet).unwrap();

        log::info!("  Device {} escrito: {} rows ({:?} total)",
            device_id, tablet.row_count(), start_total.elapsed());
    }

    log::info!("  Todas las filas escritas, cerrando... ({:?})", start_total.elapsed());
    writer.close().unwrap();
    let total_time = start_total.elapsed();
    log::info!("  Writer cerrado ({:?})", total_time);

    let file_size = std::fs::metadata(path).unwrap().len();
    log::info!("TsFile escrito: {} bytes ({} MB) en {:?}",
        file_size, file_size / 1_000_000, total_time);

    // VERIFICAR: ¿Por qué es tan grande?
    if file_size > 50_000_000 {
        log::error!("⚠️  ARCHIVO DEMASIADO GRANDE: {} MB (debería ser ~12 MB)", file_size / 1_000_000);
        log::error!("⚠️  Posible problema: encoding/compression no se está aplicando correctamente");
    }
}

/// Escribe datos a un archivo TsFile usando el convertidor Arrow
fn write_tsfile_arrow(batch: &RecordBatch, path: &std::path::Path) {
    println!("Escribiendo {} filas a TsFile (Arrow)...", batch.num_rows());

    let mut converter = ArrowToTsFileConverter::builder(path)
        .with_device_column("device_id")
        .with_timestamp_column("timestamp")
        .build()
        .unwrap();

    converter.write_batch(batch).unwrap();
    converter.finish().unwrap();

    println!("TsFile (Arrow) escrito: {} bytes", std::fs::metadata(path).unwrap().len());
}

/// Lee un archivo Parquet y convierte a Arrow
fn read_parquet_to_arrow(path: &std::path::Path) -> usize {
    let file = std::fs::File::open(path).unwrap();
    let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();
    let mut reader = builder.build().unwrap();

    let mut total_rows = 0;
    while let Some(Ok(batch)) = reader.next() {
        total_rows += batch.num_rows();
    }

    total_rows
}

/// Lee un archivo TsFile y convierte a Arrow
fn read_tsfile_to_arrow(path: &std::path::Path) -> usize {
    let reader = TsFileRecordBatchReader::try_new(path).unwrap();

    let mut total_rows = 0;
    for batch_result in reader {
        let batch = batch_result.unwrap();
        total_rows += batch.num_rows();
    }

    total_rows
}

/// Benchmark de lectura: Parquet vs Tick
fn benchmark_read_comparison(c: &mut Criterion) {
    let _ = env_logger::builder().is_test(true).try_init();

    let num_rows = 1_000_000;

    // Generar datos una sola vez
    let batch = generate_test_data(num_rows);

    // Crear archivos temporales
    let parquet_file = NamedTempFile::new().unwrap();
    let tsfile_native = NamedTempFile::new().unwrap();
    let tsfile_arrow = NamedTempFile::new().unwrap();

    // Escribir archivos
    write_parquet(&batch, parquet_file.path());
    write_tsfile_native(&batch, tsfile_native.path());
    write_tsfile_arrow(&batch, tsfile_arrow.path());

    println!("\n=== Tamaños de archivo ===");
    println!("Parquet: {} MB",
        std::fs::metadata(parquet_file.path()).unwrap().len() / 1_000_000);
    println!("TsFile (nativo): {} MB",
        std::fs::metadata(tsfile_native.path()).unwrap().len() / 1_000_000);
    println!("TsFile (Arrow): {} MB",
        std::fs::metadata(tsfile_arrow.path()).unwrap().len() / 1_000_000);

    let mut group = c.benchmark_group("read_to_arrow");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    // Benchmark Parquet → Arrow
    group.bench_with_input(
        BenchmarkId::new("parquet", num_rows),
        parquet_file.path(),
        |b, path| {
            b.iter(|| {
                let rows = read_parquet_to_arrow(black_box(path));
                assert_eq!(rows, num_rows);
            });
        },
    );

    // Benchmark TsFile (nativo) → Arrow
    group.bench_with_input(
        BenchmarkId::new("tsfile_native", num_rows),
        tsfile_native.path(),
        |b, path| {
            b.iter(|| {
                let rows = read_tsfile_to_arrow(black_box(path));
                assert_eq!(rows, num_rows);
            });
        },
    );

    // Benchmark TsFile (Arrow) → Arrow
    group.bench_with_input(
        BenchmarkId::new("tsfile_arrow", num_rows),
        tsfile_arrow.path(),
        |b, path| {
            b.iter(|| {
                let rows = read_tsfile_to_arrow(black_box(path));
                assert_eq!(rows, num_rows);
            });
        },
    );

    group.finish();
}

/// Benchmark de escritura: Parquet vs Tick
fn benchmark_write_comparison(c: &mut Criterion) {
    let _ = env_logger::builder().is_test(true).try_init();

    let num_rows = 100_000; // Menos filas para escritura (más lenta)

    let batch = generate_test_data(num_rows);

    let mut group = c.benchmark_group("write_from_arrow");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    // Benchmark Arrow → Parquet
    group.bench_with_input(
        BenchmarkId::new("parquet", num_rows),
        &batch,
        |b, batch| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().unwrap();
                write_parquet(black_box(batch), temp_file.path());
            });
        },
    );

    // Benchmark Arrow → TsFile (nativo)
    group.bench_with_input(
        BenchmarkId::new("tsfile_native", num_rows),
        &batch,
        |b, batch| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().unwrap();
                write_tsfile_native(black_box(batch), temp_file.path());
            });
        },
    );

    // Benchmark Arrow → TsFile (Arrow converter)
    group.bench_with_input(
        BenchmarkId::new("tsfile_arrow", num_rows),
        &batch,
        |b, batch| {
            b.iter(|| {
                let temp_file = NamedTempFile::new().unwrap();
                write_tsfile_arrow(black_box(batch), temp_file.path());
            });
        },
    );

    group.finish();
}

criterion_group!(benches, benchmark_read_comparison, benchmark_write_comparison);
criterion_main!(benches);
