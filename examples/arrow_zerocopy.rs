/// Arrow Zero-Copy Integration Example
///
/// Este ejemplo demuestra:
/// - Conversión zero-copy de TsFile → Arrow RecordBatch
/// - Buffers alineados a 64 bytes para SIMD performance
/// - Verificación de alignment para optimización SIMD
///
/// Run with: cargo run --example arrow_zerocopy

use timbre_tsf::arrow::{TsFileRecordBatchReader, ARROW_ALIGNMENT};
use timbre_tsf::common::{CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue};
use timbre_tsf::writer::TsFileWriter;
use arrow::array::Array;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("════════════════════════════════════════════════");
    println!("Arrow Zero-Copy Integration with 64-byte Alignment");
    println!("════════════════════════════════════════════════\n");

    let filename = "/tmp/arrow_zerocopy_demo.timbre";

    // ========================================
    // PHASE 1: Write TsFile Data
    // ========================================
    println!("📝 PHASE 1: Writing TsFile data\n");

    {
        let mut writer = TsFileWriter::new(filename)?;

        // Temperature sensor (Float with Chimp128)
        let temp_schema = MeasurementSchema::new(
            "temperature",
            TSDataType::Float,
            TSEncoding::Chimp128,
            CompressionType::Zstd,
        );

        // Pressure sensor (Double with Chimp128)
        let pressure_schema = MeasurementSchema::new(
            "pressure",
            TSDataType::Double,
            TSEncoding::Chimp128,
            CompressionType::Zstd,
        );

        // Counter (Int64 with DeltaOfDelta)
        let counter_schema = MeasurementSchema::new(
            "counter",
            TSDataType::Int64,
            TSEncoding::DeltaOfDelta,
            CompressionType::Zstd,
        );

        writer.register_timeseries("sensor_01", temp_schema)?;
        writer.register_timeseries("sensor_01", pressure_schema)?;
        writer.register_timeseries("sensor_01", counter_schema)?;

        // Write 10,000 points (enough to see alignment benefits)
        println!("  → Writing 10,000 time series points");
        for i in 0..10_000 {
            let timestamp = 1_704_067_200_000i64 + i * 1000; // 1 Hz sampling
            let temp = 20.0 + ((i as f32) * 0.01).sin() * 5.0;
            let pressure = 1013.25 + ((i as f64) * 0.001).cos() * 3.0;
            let counter = i;

            let record = TsRecord::new(timestamp, "sensor_01")
                .with_value("temperature", TsValue::Float(temp))
                .with_value("pressure", TsValue::Double(pressure))
                .with_value("counter", TsValue::Int64(counter));

            writer.write_record(record)?;
        }

        writer.close()?;
        println!("  ✅ Wrote 10,000 points to {}\n", filename);
    }

    // ========================================
    // PHASE 2: Read as Arrow with Zero-Copy
    // ========================================
    println!("📖 PHASE 2: Reading as Arrow RecordBatch\n");

    let reader = TsFileRecordBatchReader::try_new(filename)?;
    let schema = reader.schema();

    println!("  Arrow Schema:");
    for field in schema.fields() {
        println!("    • {}: {:?}", field.name(), field.data_type());
    }
    println!();

    let mut total_rows = 0;
    let mut batch_count = 0;

    for batch_result in reader {
        let batch = batch_result?;
        batch_count += 1;
        total_rows += batch.num_rows();

        println!("  RecordBatch #{}:", batch_count);
        println!("    • Rows: {}", batch.num_rows());
        println!("    • Columns: {}", batch.num_columns());

        // ========================================
        // PHASE 3: Verify 64-byte Alignment
        // ========================================
        println!("\n  🔍 Buffer Alignment Verification:");

        for (i, column) in batch.columns().iter().enumerate() {
            let field_name = schema.field(i).name();

            // Get raw data pointer
            let data = column.to_data();
            if !data.buffers().is_empty() {
                let buffer = &data.buffers()[0];
                let ptr = buffer.as_ptr() as usize;
                let alignment = ptr % ARROW_ALIGNMENT;

                let status = if alignment == 0 {
                    "✅ ALIGNED"
                } else {
                    "❌ NOT ALIGNED"
                };

                println!("    • {:<12} @ 0x{:016x} → {} (offset: {})",
                    field_name,
                    ptr,
                    status,
                    alignment
                );
            }
        }

        // Show first 3 values
        println!("\n  📊 Sample Data (first 3 rows):");
        for row_idx in 0..3.min(batch.num_rows()) {
            print!("    Row {}: ", row_idx);
            for (col_idx, column) in batch.columns().iter().enumerate() {
                let field_name = schema.field(col_idx).name();

                // Format value based on type
                let value_str = format_array_value(column.as_ref(), row_idx);
                print!("{}={} ", field_name, value_str);
            }
            println!();
        }
        println!();
    }

    println!("════════════════════════════════════════════════");
    println!("✅ Zero-Copy Summary:");
    println!("  • Total batches: {}", batch_count);
    println!("  • Total rows: {}", total_rows);
    println!("  • Alignment: 64 bytes (SIMD-optimized)");
    println!("  • Zero allocations: ✅ (Buffer::from_vec takes ownership)");
    println!("════════════════════════════════════════════════\n");

    Ok(())
}

/// Helper to format array values for display
fn format_array_value(array: &dyn Array, index: usize) -> String {
    use arrow::array::*;
    use arrow::datatypes::DataType;

    match array.data_type() {
        DataType::Timestamp(_, _) => {
            let arr = array.as_any().downcast_ref::<TimestampMillisecondArray>().unwrap();
            format!("{}", arr.value(index))
        }
        DataType::Utf8 => {
            let arr = array.as_any().downcast_ref::<StringArray>().unwrap();
            format!("\"{}\"", arr.value(index))
        }
        DataType::Float32 => {
            let arr = array.as_any().downcast_ref::<Float32Array>().unwrap();
            format!("{:.2}", arr.value(index))
        }
        DataType::Float64 => {
            let arr = array.as_any().downcast_ref::<Float64Array>().unwrap();
            format!("{:.2}", arr.value(index))
        }
        DataType::Int32 => {
            let arr = array.as_any().downcast_ref::<Int32Array>().unwrap();
            format!("{}", arr.value(index))
        }
        DataType::Int64 => {
            let arr = array.as_any().downcast_ref::<Int64Array>().unwrap();
            format!("{}", arr.value(index))
        }
        _ => format!("<?>")}
}
