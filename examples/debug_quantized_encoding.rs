use timbre_tsf::encoding::quantized::{QuantizedEncoder, detect_quantization};

fn main() {
    // Generate IoT-like data
    let mut data = Vec::new();
    let mut temp: f64 = 20.0;

    for i in 0..100 {
        data.push(temp);

        // 85% stay same, 15% change
        if i % 7 == 0 {
            temp += if i % 2 == 0 { 0.1 } else { -0.1 };
            temp = temp.clamp(19.0, 21.0);
        }
    }

    println!("=== Data Analysis ===");
    println!("First 20 values: {:?}", &data[..20]);

    // Detect quantization
    let (min, step) = detect_quantization(&data).expect("Should detect");
    println!("\nDetected: min={}, step={}", min, step);

    // Quantize manually to see indices
    let indices: Vec<i64> = data
        .iter()
        .map(|&v| {
            let offset = v - min;
            (offset / step).round() as i64
        })
        .collect();

    println!("\nFirst 20 indices: {:?}", &indices[..20]);

    // Compute deltas
    let deltas: Vec<i64> = indices.windows(2).map(|w| w[1] - w[0]).collect();

    println!("\nFirst 20 deltas: {:?}", &deltas[..20]);

    // Count zeros
    let zero_count = deltas.iter().filter(|&&d| d == 0).count();
    println!(
        "\nZero deltas: {}/{} ({:.1}%)",
        zero_count,
        deltas.len(),
        100.0 * zero_count as f64 / deltas.len() as f64
    );

    // Encode
    let mut encoder = QuantizedEncoder::new(min, step);
    let encoded = encoder.encode(&data).expect("Should encode");

    println!("\n=== Encoding Results ===");
    println!("Raw size: {} bytes", data.len() * 8);
    println!("Encoded size: {} bytes", encoded.len());
    println!(
        "Compression: {:.2}x",
        (data.len() * 8) as f64 / encoded.len() as f64
    );

    // Breakdown
    let header_size = 8 + 8 + 4 + 8; // min + step + count + first_value
    let simple8b_size = encoded.len() - header_size;
    println!("\nBreakdown:");
    println!("  Header: {} bytes", header_size);
    println!("  Simple8b data: {} bytes", simple8b_size);
    println!(
        "  Bits per delta: {:.2}",
        (simple8b_size * 8) as f64 / deltas.len() as f64
    );

    // Show first bytes
    println!("\nFirst 40 bytes of encoded data:");
    for (i, chunk) in encoded.chunks(8).take(5).enumerate() {
        print!("  [{:2}] ", i * 8);
        for &b in chunk {
            print!("{:02x} ", b);
        }
        println!();
    }

    // Decode to verify
    let decoded = encoder.decode(&encoded).expect("Should decode");
    assert_eq!(data.len(), decoded.len());

    for (i, (&orig, &dec)) in data.iter().zip(decoded.iter()).enumerate() {
        assert!(
            (orig - dec).abs() < 1e-6,
            "Mismatch at {}: {} != {}",
            i,
            orig,
            dec
        );
    }

    println!("\n✅ Lossless roundtrip verified");
}
