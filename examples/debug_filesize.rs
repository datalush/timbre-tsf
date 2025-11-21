/// Debug file size issue
use timbre_tsf::common::*;
use timbre_tsf::writer::TsFileWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = "/tmp/debug_timbre.timbre";
    let mut writer = TsFileWriter::new(path)?;

    let temp_schema = MeasurementSchema::new(
        "temperature",
        TSDataType::Float,
        TSEncoding::Chimp128,
        CompressionType::Zstd,
    );

    writer.register_timeseries("sensor_01", temp_schema)?;

    // Write 10000 points
    println!("Writing 10,000 points...");
    for i in 0..10_000 {
        let record = TsRecord::new(1000 + i, "sensor_01").with_value(
            "temperature",
            TsValue::Float(20.0 + ((i as f32 * 0.001).sin() * 5.0)),
        );
        writer.write_record(record)?;
    }

    writer.close()?;

    let size = std::fs::metadata(path)?.len();
    let raw_size = 10_000 * (8 + 4); // timestamp (i64) + float (f32)

    println!("\n=== File Size Analysis ===");
    println!(
        "Raw data size:     {} bytes ({:.2} KB)",
        raw_size,
        raw_size as f64 / 1024.0
    );
    println!(
        "Timbre file size:  {} bytes ({:.2} KB)",
        size,
        size as f64 / 1024.0
    );
    println!("Compression ratio: {:.2}x", raw_size as f64 / size as f64);

    if size > raw_size as u64 {
        println!("\n⚠️  WARNING: File is LARGER than raw data!");
        println!("   This indicates a compression problem.");
    }

    Ok(())
}
