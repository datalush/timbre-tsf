//! Generate a realistic 1GB IoT dataset in Arrow IPC format
//!
//! Creates time series data from 20 IoT devices with various sensors:
//! - Temperature (0.1°C precision - quantized)
//! - Humidity (0.1% precision - quantized)
//! - Pressure (floating point - continuous)
//! - CO2 (integer - discrete)
//! - Light (integer - high variation)
//! - Battery (0.01V precision - slow drift)
//!
//! Output: data/iot_dataset.arrow (~1GB)
//!
//! Run with: cargo run --release --example generate_iot_dataset

use arrow::array::*;
use arrow::datatypes::{DataType, Field, Schema, TimeUnit};
use arrow::ipc::writer::FileWriter;
use arrow::record_batch::RecordBatch;
use std::fs::File;
use std::sync::Arc;
use std::time::Instant;

const NUM_DEVICES: usize = 40; // Doubled for 1GB target
const MEASUREMENTS_PER_DEVICE: usize = 500_000; // ~20M total rows
const BATCH_SIZE: usize = 10_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start = Instant::now();
    println!("🔧 Generating realistic IoT dataset...");
    println!("   Devices: {}", NUM_DEVICES);
    println!("   Measurements per device: {}", MEASUREMENTS_PER_DEVICE);
    println!("   Total rows: {}", NUM_DEVICES * MEASUREMENTS_PER_DEVICE);
    println!();

    // Create schema
    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            false,
        ),
        Field::new("device_id", DataType::Utf8, false),
        Field::new("temperature", DataType::Float32, true), // °C, 0.1° precision
        Field::new("humidity", DataType::Float32, true),    // %, 0.1% precision
        Field::new("pressure", DataType::Float64, true),    // hPa, continuous
        Field::new("co2", DataType::Int32, true),           // ppm, discrete
        Field::new("light", DataType::Int32, true),         // lux, high variation
        Field::new("battery", DataType::Float32, true),     // V, slow drift
        Field::new("status", DataType::Int8, true),         // 0=ok, 1=warning, 2=error
    ]));

    // Create output file
    std::fs::create_dir_all("data")?;
    let file = File::create("data/iot_dataset.arrow")?;
    let mut writer = FileWriter::try_new(file, &schema)?;

    let mut total_rows = 0;
    let base_timestamp = 1_700_000_000_000i64; // Nov 2023

    // Generate data device by device
    for device_idx in 0..NUM_DEVICES {
        let device_id = format!("sensor-{:03}", device_idx);
        println!("  Generating data for {}...", device_id);

        // Device-specific base values
        let temp_base = 20.0 + (device_idx as f32 * 0.5);
        let humidity_base = 50.0 + (device_idx as f32 * 2.0);
        let pressure_base = 1013.25 + (device_idx as f64 * 0.1);
        let co2_base = 400 + (device_idx as i32 * 10);

        // Generate in batches
        let num_batches = (MEASUREMENTS_PER_DEVICE + BATCH_SIZE - 1) / BATCH_SIZE;

        for batch_idx in 0..num_batches {
            let start_idx = batch_idx * BATCH_SIZE;
            let end_idx = (start_idx + BATCH_SIZE).min(MEASUREMENTS_PER_DEVICE);
            let batch_size = end_idx - start_idx;

            let mut timestamps = Vec::with_capacity(batch_size);
            let mut device_ids = Vec::with_capacity(batch_size);
            let mut temperatures = Vec::with_capacity(batch_size);
            let mut humidities = Vec::with_capacity(batch_size);
            let mut pressures = Vec::with_capacity(batch_size);
            let mut co2_values = Vec::with_capacity(batch_size);
            let mut light_values = Vec::with_capacity(batch_size);
            let mut battery_values = Vec::with_capacity(batch_size);
            let mut status_values = Vec::with_capacity(batch_size);

            for i in start_idx..end_idx {
                // Timestamp: 1 reading per second
                let timestamp = base_timestamp + (i as i64 * 1000);
                timestamps.push(timestamp);
                device_ids.push(device_id.clone());

                // Time of day effect (sine wave over 24h)
                let time_of_day = (i as f64 / (86400.0 / 1.0)) * 2.0 * std::f64::consts::PI;

                // Temperature: quantized to 0.1°C with daily cycle + random walk
                let temp_daily = (time_of_day.sin() * 5.0) as f32;
                let temp_noise = ((i * 7919) % 21) as f32 * 0.1 - 1.0; // Deterministic "random"
                let temperature = (temp_base + temp_daily + temp_noise * 0.3).round() / 10.0 * 10.0;
                temperatures.push(Some(temperature));

                // Humidity: quantized to 0.1%, inverse of temperature
                let humidity_daily = (-time_of_day.sin() * 10.0) as f32;
                let humidity_noise = ((i * 9973) % 21) as f32 * 0.1 - 1.0;
                let humidity =
                    (humidity_base + humidity_daily + humidity_noise * 0.5).round() / 10.0 * 10.0;
                humidities.push(Some(humidity.clamp(0.0, 100.0)));

                // Pressure: continuous drift (not quantized)
                let pressure_wave = (time_of_day / 2.0).sin() * 2.5;
                let pressure_noise = ((i * 8191) % 1000) as f64 * 0.001 - 0.5;
                let pressure = pressure_base + pressure_wave + pressure_noise;
                pressures.push(Some(pressure));

                // CO2: discrete levels (400, 450, 500, 600, 800, 1000, 1200)
                let co2_levels = [400, 450, 500, 600, 800, 1000, 1200];
                let co2_idx = ((i / 100) + device_idx) % co2_levels.len();
                let co2 = co2_base + co2_levels[co2_idx];
                co2_values.push(Some(co2));

                // Light: high variation (0-5000 lux), step changes
                let light = if i % 43200 < 21600 {
                    // "Day" period
                    (((i * 6421) % 4000) + 1000) as i32
                } else {
                    // "Night" period
                    ((i * 3571) % 100) as i32
                };
                light_values.push(Some(light));

                // Battery: slow linear drain 4.2V -> 3.0V over time
                let battery_drain = (i as f32 / MEASUREMENTS_PER_DEVICE as f32) * 1.2;
                let battery = 4.2 - battery_drain;
                battery_values.push(Some(battery));

                // Status: mostly OK, occasional warnings
                let status = if i % 1000 == 0 {
                    2 // Error
                } else if i % 100 == 0 {
                    1 // Warning
                } else {
                    0 // OK
                };
                status_values.push(Some(status));
            }

            // Create Arrow arrays
            let timestamp_array = Arc::new(TimestampMillisecondArray::from(timestamps));
            let device_array = Arc::new(StringArray::from(device_ids));
            let temp_array = Arc::new(Float32Array::from(temperatures));
            let humidity_array = Arc::new(Float32Array::from(humidities));
            let pressure_array = Arc::new(Float64Array::from(pressures));
            let co2_array = Arc::new(Int32Array::from(co2_values));
            let light_array = Arc::new(Int32Array::from(light_values));
            let battery_array = Arc::new(Float32Array::from(battery_values));
            let status_array = Arc::new(Int8Array::from(status_values));

            // Create record batch
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    timestamp_array,
                    device_array,
                    temp_array,
                    humidity_array,
                    pressure_array,
                    co2_array,
                    light_array,
                    battery_array,
                    status_array,
                ],
            )?;

            writer.write(&batch)?;
            total_rows += batch.num_rows();
        }
    }

    writer.finish()?;

    let duration = start.elapsed();
    let file_size = std::fs::metadata("data/iot_dataset.arrow")?.len();

    println!();
    println!("✅ Dataset generated successfully!");
    println!("   File: data/iot_dataset.arrow");
    println!("   Size: {:.2} GB", file_size as f64 / 1_000_000_000.0);
    println!("   Rows: {}", total_rows);
    println!("   Columns: {}", schema.fields().len());
    println!("   Time: {:?}", duration);
    println!();
    println!("Dataset characteristics:");
    println!("  - Temperature: Quantized 0.1°C, daily cycle, realistic range");
    println!("  - Humidity: Quantized 0.1%, inverse correlation with temp");
    println!("  - Pressure: Continuous float, slow atmospheric variation");
    println!("  - CO2: Discrete levels (400-1200 ppm)");
    println!("  - Light: High variation day/night cycles");
    println!("  - Battery: Linear drain 4.2V -> 3.0V");
    println!("  - Status: Mostly OK with occasional warnings/errors");

    Ok(())
}
