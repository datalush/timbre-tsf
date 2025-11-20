use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;
use std::hint::black_box;

fn main() {
    let path = "/tmp/profile_write.ts";

    // Benchmark pequeño: 10K rows para profiling rápido
    for _ in 0..5 {
        let _ = std::fs::remove_file(path);
        let mut writer = TsFileWriter::new(path).unwrap();

        // 5 devices × 3 measurements
        for device_idx in 1..=5 {
            let device_id = format!("device_{}", device_idx);

            let schemas = vec![
                MeasurementSchema::new(
                    "temperature",
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
                MeasurementSchema::new(
                    "humidity",
                    TSDataType::Float,
                    TSEncoding::Gorilla,
                    CompressionType::Lz4,
                ),
            ];

            for schema in &schemas {
                writer.register_timeseries(&device_id, schema.clone()).unwrap();
            }

            let mut tablet = Tablet::new(
                &device_id,
                schemas.clone(),
                vec![ColumnCategory::Field; 3],
                2000, // 2K rows per device
            );

            for i in 0..2000 {
                let timestamp = 1000 + i as i64 * 100;
                tablet.add_row(
                    timestamp,
                    vec![
                        Some(TsValue::Float(25.0 + (i % 100) as f32 * 0.1)),
                        Some(TsValue::Float(1013.25 + (i % 50) as f32 * 0.5)),
                        Some(TsValue::Float(60.0 + (i % 40) as f32 * 0.25)),
                    ],
                ).unwrap();
            }

            black_box(writer.write_tablet(&tablet).unwrap());
        }

        black_box(writer.close().unwrap());
    }

    std::fs::remove_file(path).ok();
}
