use std::fs::File;
use timbre_tsf::common::{CompressionType, TSDataType, TSEncoding};
use timbre_tsf::writer::ChunkWriter;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== TsFile Write Example ===\n");

    // Crear un ChunkWriter para datos de temperatura
    let mut temp_writer = ChunkWriter::new(
        "temperature".to_string(),
        TSDataType::Float,
        TSEncoding::Plain,
        CompressionType::Lz4,
    );

    println!("Writing temperature data...");
    // Escribir datos de temperatura (100 puntos)
    for i in 0..100 {
        let timestamp = 1000 + i * 1000; // Cada segundo
        let value = 20.0 + (i as f32 * 0.1); // Temperatura de 20°C a 30°C
        temp_writer.write_f32(timestamp, value)?;
    }

    println!("  - Wrote 100 temperature values");
    println!("  - Number of pages: {}", temp_writer.num_of_pages());
    println!("  - Estimated size: {} bytes", temp_writer.estimated_size());

    // Crear archivo de salida
    let mut file = File::create("/tmp/example.tick.chunk")?;

    // Serializar el chunk al archivo
    let bytes_written = temp_writer.serialize_to(&mut file)?;
    println!("  - Bytes written to file: {}", bytes_written);

    // Estadísticas del chunk
    let stats = temp_writer.statistic();
    println!("\nChunk Statistics:");
    println!("  - Count: {}", stats.count());
    println!(
        "  - Time range: {} - {}",
        stats.start_time(),
        stats.end_time()
    );

    // Crear otro chunk para humedad
    let mut humidity_writer = ChunkWriter::new(
        "humidity".to_string(),
        TSDataType::Int32,
        TSEncoding::Ts2Diff,
        CompressionType::Lz4,
    );

    println!("\nWriting humidity data...");
    for i in 0..100 {
        let timestamp = 1000 + i * 1000;
        let value = 50 + (i % 30) as i32; // Humedad entre 50% y 80%
        humidity_writer.write_i32(timestamp, value)?;
    }

    println!("  - Wrote 100 humidity values");
    println!("  - Number of pages: {}", humidity_writer.num_of_pages());

    // Crear segundo archivo
    let mut file2 = File::create("/tmp/example.tick.chunk2")?;
    let bytes_written2 = humidity_writer.serialize_to(&mut file2)?;
    println!("  - Bytes written to file: {}", bytes_written2);

    println!("\n=== Write Complete ===");
    println!("Files created:");
    println!("  - /tmp/example.tick.chunk (temperature)");
    println!("  - /tmp/example.tick.chunk2 (humidity)");

    Ok(())
}
