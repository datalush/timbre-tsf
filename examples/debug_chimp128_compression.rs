use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use rand::Rng;

fn generate_iot_temperature(n: usize, stability: f64) -> Vec<f64> {
    let mut rng = rand::thread_rng();
    let mut result = Vec::with_capacity(n);

    // Valores discretos de temperatura (cuantizados a 0.1°C, realista para sensores)
    let possible_temps: Vec<f64> = (180..=250)
        .map(|t| t as f64 / 10.0)  // 18.0, 18.1, 18.2, ..., 25.0
        .collect();

    // Temperatura actual (índice en possible_temps)
    let mut current_idx = 20; // Empieza en 20.0°C

    for _ in 0..n {
        // Con probabilidad 'stability', mantener el mismo valor
        if rng.gen_range(0.0..1.0) > stability {
            // Cambio a temperatura adyacente (±1 índice = ±0.1°C)
            let delta = if rng.gen_bool(0.5) { 1 } else { -1 };
            current_idx = (current_idx + delta).clamp(0, possible_temps.len() as i32 - 1);
        }

        result.push(possible_temps[current_idx as usize]);
    }

    result
}

fn compress_chimp128(data: &[f64]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut output = Vec::new();

    for &value in data {
        encoder.encode_f64(value, &mut output).unwrap();
    }
    encoder.flush(&mut output).unwrap();

    output
}

fn main() {
    println!("=== Test 1: Perfectly constant data (vec![20.0; 10000]) ===");
    let constant_data = vec![20.0; 10000];
    let encoded = compress_chimp128(&constant_data);
    let raw_size = constant_data.len() * 8;
    let bits_per_value = (encoded.len() * 8) as f64 / constant_data.len() as f64;
    let ratio = raw_size as f64 / encoded.len() as f64;

    println!("Raw size: {} bytes", raw_size);
    println!("Encoded size: {} bytes", encoded.len());
    println!("Bits per value: {:.2}", bits_per_value);
    println!("Compression ratio: {:.2}x", ratio);
    println!("Expected: ~1 bit/value for constant data");
    println!();

    println!("=== Test 2: IoT data with 85% stability ===");
    let iot_data = generate_iot_temperature(10000, 0.85);

    // Check uniqueness
    let mut unique_values = iot_data.clone();
    unique_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique_values.dedup();
    println!("Unique values: {} out of {}", unique_values.len(), iot_data.len());
    println!("Uniqueness: {:.2}%", (unique_values.len() as f64 / iot_data.len() as f64) * 100.0);

    // Show first 20 values
    println!("\nFirst 20 values:");
    for (i, &val) in iot_data.iter().take(20).enumerate() {
        print!("{:.4} ", val);
        if (i + 1) % 10 == 0 { println!(); }
    }
    println!();

    let encoded = compress_chimp128(&iot_data);
    let raw_size = iot_data.len() * 8;
    let bits_per_value = (encoded.len() * 8) as f64 / iot_data.len() as f64;
    let ratio = raw_size as f64 / encoded.len() as f64;

    println!("\nRaw size: {} bytes", raw_size);
    println!("Encoded size: {} bytes", encoded.len());
    println!("Bits per value: {:.2}", bits_per_value);
    println!("Compression ratio: {:.2}x", ratio);
    println!("Expected: 40-60x for stable IoT data");

    if ratio < 20.0 {
        println!("\n⚠️  WARNING: Compression ratio FAR below expected!");
        println!("   This suggests a problem with Chimp128 encoder or data generation");
    }
}
