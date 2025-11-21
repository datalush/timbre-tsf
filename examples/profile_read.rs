use timbre_tsf::arrow::RecordBatchReader;
use timbre_tsf::common::*;
use timbre_tsf::writer::FileWriter;

fn main() {
    let path = "/tmp/profile_tsfile.ts";

    // Generar archivo de prueba
    generate_test_file(path, 1_000_000);

    println!("Profiling read de {} rows...", 1_000_000);

    // Ejecutar lectura 10 veces para obtener muestra representativa
    for run in 0..10 {
        let start = std::time::Instant::now();
        read_tsfile(path);
        let elapsed = start.elapsed();
        println!("Run {}: {:?}", run + 1, elapsed);
    }
}

fn generate_test_file(path: &str, total_rows: usize) {
    let _ = std::fs::remove_file(path);

    let mut writer = FileWriter::new(path).unwrap();

    // 5 devices × 3 measurements
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);

        for measurement in ["temperature", "pressure", "humidity"] {
            let schema = MeasurementSchema::new(
                measurement,
                TSDataType::Float,
                TSEncoding::Gorilla,
                CompressionType::Lz4,
            );
            writer.register_timeseries(&device_id, schema).unwrap();
        }
    }

    // Escribir rows_per_device rows por cada dispositivo
    let rows_per_device = total_rows / 5;
    for device_idx in 1..=5 {
        let device_id = format!("device_{}", device_idx);
        for i in 0..rows_per_device {
            let timestamp = 1000 + i as i64 * 100;
            let record = TsRecord::new(timestamp, &device_id)
                .with_value("temperature", TsValue::Float(25.0 + (i % 100) as f32 * 0.1))
                .with_value("pressure", TsValue::Float(1013.25 + (i % 50) as f32 * 0.5))
                .with_value("humidity", TsValue::Float(60.0 + (i % 40) as f32 * 0.25));
            writer.write_record(record).unwrap();
        }
    }

    writer.close().unwrap();
    println!(
        "Generated test file: {} bytes",
        std::fs::metadata(path).unwrap().len()
    );
}

fn read_tsfile(path: &str) {
    let reader = RecordBatchReader::try_new(path).unwrap();

    let mut total_rows = 0;
    for batch_result in reader {
        let batch = batch_result.unwrap();
        total_rows += batch.num_rows();
    }

    assert_eq!(total_rows, 1_000_000);
}
