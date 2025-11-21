/// Example: Using encoding recommendation tools
///
/// This example demonstrates how an application/database would use
/// Timbre's analysis tools to select the optimal encoder.
///
/// Architecture:
/// - Application: Buffers data, analyzes with recommend_encoding(), makes decisions
/// - Timbre: Provides analysis tools and individual encoders
use timbre_tsf::common::TSEncoding;
use timbre_tsf::encoding::adaptive::recommend_encoding;
use timbre_tsf::encoding::dictionary_rle::DictionaryRLEEncoder;
use timbre_tsf::encoding::quantized::{QuantizedEncoder, detect_quantization};

fn main() {
    println!("=== Encoding Recommendation Example ===\n");

    // Scenario 1: Temperature sensor (quantized 0.1°C)
    println!("📊 Scenario 1: Temperature Sensor");
    let temp_data = vec![20.0, 20.1, 20.2, 20.1, 20.0, 20.1, 20.2, 20.3, 20.2, 20.1];

    // Application analyzes sample and gets recommendation
    let recommended = recommend_encoding(&temp_data);
    println!("  Sample: {:?}", &temp_data[..5]);
    println!("  Recommended encoding: {:?}\n", recommended);

    // Application uses recommendation to select encoder
    match recommended {
        TSEncoding::Quantized => {
            println!("  ✅ Using Quantized encoder (application's choice)");
            if let Some((min, step)) = detect_quantization(&temp_data) {
                let mut encoder = QuantizedEncoder::new(min, step);
                let encoded = encoder.encode(&temp_data).unwrap();
                let decoded = encoder.decode(&encoded).unwrap();

                let lossless = temp_data
                    .iter()
                    .zip(&decoded)
                    .all(|(a, b)| (a - b).abs() < 1e-10);

                println!("    Raw: {} bytes", temp_data.len() * 8);
                println!("    Encoded: {} bytes", encoded.len());
                println!(
                    "    Compression: {:.2}x",
                    (temp_data.len() * 8) as f64 / encoded.len() as f64
                );
                println!("    Lossless: {}\n", lossless);
            }
        }
        _ => println!("  Unexpected encoding\n"),
    }

    // Scenario 2: IoT sensor with high repetition
    println!("📊 Scenario 2: IoT Sensor (High Repetition)");
    let iot_data = vec![
        20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.3, 20.3, 20.7, 20.7, 20.7, 20.0,
        20.0, 20.0, 20.0, 20.0, 20.0,
    ];

    let recommended = recommend_encoding(&iot_data);
    println!("  Sample: {:?}", &iot_data[..5]);
    println!("  Recommended encoding: {:?}\n", recommended);

    match recommended {
        TSEncoding::DictionaryRLE => {
            println!("  ✅ Using DictionaryRLE encoder (application's choice)");
            let mut encoder = DictionaryRLEEncoder::new();
            let encoded = encoder.encode(&iot_data).unwrap();
            let decoded = encoder.decode(&encoded).unwrap();

            println!("    Raw: {} bytes", iot_data.len() * 8);
            println!("    Encoded: {} bytes", encoded.len());
            println!(
                "    Compression: {:.2}x",
                (iot_data.len() * 8) as f64 / encoded.len() as f64
            );
            println!("    Lossless: {}\n", iot_data == decoded);
        }
        _ => println!("  Unexpected encoding\n"),
    }

    // Scenario 3: Continuous drift
    println!("📊 Scenario 3: Continuous Drift");
    let mut drift_data = Vec::new();
    let mut value = 20.0;
    for i in 0..20 {
        drift_data.push(value);
        value += 0.001 * (1.0 + (i as f64 * 0.01).sin() * 0.1);
    }

    let recommended = recommend_encoding(&drift_data);
    println!("  Sample: {:?}", &drift_data[..5]);
    println!("  Recommended encoding: {:?}", recommended);
    println!("  (Chimp128 would be used via create_encoder())\n");

    // Summary
    println!("=== Architecture Summary ===");
    println!("📦 Timbre Fileformat (Library):");
    println!("   - Provides: recommend_encoding() analysis tool");
    println!("   - Provides: Individual encoders (Quantized, DictionaryRLE, Chimp128)");
    println!("   - Provides: File I/O and encoding/decoding");
    println!("   - Does NOT: Buffer data or make encoding decisions\n");

    println!("🗄️  Application/Database Layer:");
    println!("   - Buffers incoming data points");
    println!("   - Calls recommend_encoding() on buffered samples");
    println!("   - Makes encoding decision based on recommendation");
    println!("   - Caches encoding choice per series/chunk");
    println!("   - Uses appropriate encoder to write data");
}
