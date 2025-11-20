/// Ejemplo completo end-to-end de escritura y lectura de Tick
///
/// Este ejemplo demuestra:
/// - Escritura de múltiples dispositivos con diferentes mediciones
/// - Uso de diferentes tipos de datos y encodings
/// - Lectura de datos completos y filtrados por tiempo
/// - Iteración y acceso a datos
use timbre_tsf::common::{
    CompressionType, MeasurementSchema, TSDataType, TSEncoding, TsRecord, TsValue,
};
use timbre_tsf::reader::{DecodedValueData, TsFileReader};
use timbre_tsf::writer::TsFileWriter;

fn main() -> timbre_tsf::error::Result<()> {
    let filename = "examples/demo.tick";
    let base_time = 1704067200000i64; // 2024-01-01 00:00:00

    println!("═══════════════════════════════════════════════════");
    println!("Tick Rust - Ejemplo End-to-End");
    println!("═══════════════════════════════════════════════════\n");

    // ============================================================
    // ESCRITURA DE DATOS
    // ============================================================
    println!("📝 FASE 1: Escritura de datos\n");

    {
        let mut writer = TsFileWriter::new(filename)?;

        // Dispositivo 1: Sensor de temperatura (Float con Plain encoding)
        println!("  → Registrando dispositivo 'weather_station'");
        let temp_schema = MeasurementSchema::new(
            "temperature",
            TSDataType::Float,
            TSEncoding::Plain,
            CompressionType::Lz4,
        );
        writer.register_timeseries("weather_station", temp_schema)?;

        // Dispositivo 1: Humedad (Int32 con TS2DIFF encoding)
        let humidity_schema = MeasurementSchema::new(
            "humidity",
            TSDataType::Int32,
            TSEncoding::Ts2Diff,
            CompressionType::Lz4,
        );
        writer.register_timeseries("weather_station", humidity_schema)?;

        // Escribir datos de clima cada 5 minutos durante 2 horas
        println!("  → Escribiendo 24 registros de clima (2 horas)");
        for i in 0..24 {
            let timestamp = base_time + i * 300_000; // cada 5 minutos
            let temp = 20.0 + (i as f32 * 0.5); // temperatura aumenta gradualmente
            let hum = 65 + (i as i32 % 10); // humedad oscila

            let record = TsRecord::new(timestamp, "weather_station")
                .with_value("temperature", TsValue::Float(temp))
                .with_value("humidity", TsValue::Int32(hum));

            writer.write_record(record)?;
        }

        // Dispositivo 2: Contador de energía (Int64 con Plain encoding)
        println!("  → Registrando dispositivo 'power_meter'");
        let energy_schema = MeasurementSchema::new(
            "energy_kwh",
            TSDataType::Int64,
            TSEncoding::Plain,
            CompressionType::Lz4,
        );
        writer.register_timeseries("power_meter", energy_schema)?;

        // Escribir lecturas de energía cada hora
        println!("  → Escribiendo 24 lecturas de energía (1 día)");
        for i in 0..24 {
            let timestamp = base_time + i * 3600_000; // cada hora
            let energy = 1000 + i * 50; // consumo acumulado

            let record = TsRecord::new(timestamp, "power_meter")
                .with_value("energy_kwh", TsValue::Int64(energy));

            writer.write_record(record)?;
        }

        // Dispositivo 3: Sensor de presión (Double con Plain encoding)
        println!("  → Registrando dispositivo 'pressure_sensor'");
        let pressure_schema = MeasurementSchema::new(
            "pressure_hpa",
            TSDataType::Double,
            TSEncoding::Plain,
            CompressionType::Snappy,
        );
        writer.register_timeseries("pressure_sensor", pressure_schema)?;

        // Escribir datos de presión cada 15 minutos
        println!("  → Escribiendo 96 lecturas de presión (1 día)");
        for i in 0..96 {
            let timestamp = base_time + i * 900_000; // cada 15 minutos
            let pressure = 1013.25 + (i as f64 * 0.1).sin() * 5.0; // oscilación sinusoidal

            let record = TsRecord::new(timestamp, "pressure_sensor")
                .with_value("pressure_hpa", TsValue::Double(pressure));

            writer.write_record(record)?;
        }

        writer.close()?;
        println!("\n✅ Archivo creado exitosamente: {}", filename);
    }

    // ============================================================
    // LECTURA DE DATOS
    // ============================================================
    println!("\n\n📖 FASE 2: Lectura de datos\n");

    {
        let mut reader = TsFileReader::open(filename)?;

        // Mostrar información del archivo
        let info = reader.info();
        println!("ℹ️  Información del archivo:");
        println!("  • Tamaño: {} bytes", info.file_size);
        println!("  • Dispositivos: {}", info.num_devices);
        println!("  • Chunks: {}", info.num_chunks);
        println!("  • Dispositivos: {:?}\n", info.devices);

        // Leer y mostrar datos del weather_station
        println!("🌡️  Weather Station:");
        {
            let temp_chunk = reader.read("weather_station", "temperature")?;
            println!("  • Temperatura: {} lecturas", temp_chunk.len());
            println!("  • Primera: {:?}", temp_chunk.get(0));
            println!("  • Última: {:?}", temp_chunk.get(temp_chunk.len() - 1));

            // Calcular temperatura promedio
            let mut temp_sum = 0.0;
            let mut temp_count = 0;
            for (_, value) in temp_chunk.iter() {
                if let DecodedValueData::Float(v) = value {
                    temp_sum += v;
                    temp_count += 1;
                }
            }
            println!("  • Promedio: {:.2}°C", temp_sum / temp_count as f32);
        }

        // Filtrar datos por rango de tiempo (primera hora)
        println!("\n⏱️  Filtro de tiempo (primera hora):");
        {
            let filtered = reader.read_time_range(
                "weather_station",
                "temperature",
                base_time,
                base_time + 3600_000,
            )?;
            println!("  • Lecturas en primera hora: {}", filtered.len());
        }

        // Mostrar datos de energía
        println!("\n⚡ Power Meter:");
        {
            let energy_chunk = reader.read("power_meter", "energy_kwh")?;
            println!("  • Lecturas: {}", energy_chunk.len());

            if let Some((first_ts, first_val)) = energy_chunk.get(0) {
                if let DecodedValueData::Int64(first_energy) = first_val {
                    if let Some((last_ts, last_val)) = energy_chunk.get(energy_chunk.len() - 1) {
                        if let DecodedValueData::Int64(last_energy) = last_val {
                            println!("  • Consumo inicial: {} kWh", first_energy);
                            println!("  • Consumo final: {} kWh", last_energy);
                            println!("  • Consumo total: {} kWh", last_energy - first_energy);
                            println!("  • Período: {} horas", (last_ts - first_ts) / 3600_000);
                        }
                    }
                }
            }
        }

        // Mostrar estadísticas de presión
        println!("\n🌡️  Pressure Sensor:");
        {
            let pressure_chunk = reader.read("pressure_sensor", "pressure_hpa")?;
            println!("  • Lecturas: {}", pressure_chunk.len());

            let mut min_pressure = f64::MAX;
            let mut max_pressure = f64::MIN;
            let mut pressure_sum = 0.0;

            for (_, value) in pressure_chunk.iter() {
                if let DecodedValueData::Double(p) = value {
                    min_pressure = min_pressure.min(p);
                    max_pressure = max_pressure.max(p);
                    pressure_sum += p;
                }
            }

            println!("  • Mínima: {:.2} hPa", min_pressure);
            println!("  • Máxima: {:.2} hPa", max_pressure);
            println!(
                "  • Promedio: {:.2} hPa",
                pressure_sum / pressure_chunk.len() as f64
            );
        }

        // Iterar sobre algunas lecturas
        println!("\n📊 Primeras 5 lecturas de temperatura:");
        {
            let temp_chunk = reader.read("weather_station", "temperature")?;
            for (i, (ts, value)) in temp_chunk.iter().take(5).enumerate() {
                if let DecodedValueData::Float(v) = value {
                    println!("  [{}] Timestamp: {}, Temperatura: {:.1}°C", i, ts, v);
                }
            }
        }
    }

    println!("\n\n═══════════════════════════════════════════════════");
    println!("✅ Ejemplo completado exitosamente");
    println!("═══════════════════════════════════════════════════\n");

    Ok(())
}
