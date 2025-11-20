use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;

fn compress_chimp128_step_by_step(data: &[f64]) -> Vec<u8> {
    let mut encoder = create_encoder(TSEncoding::Chimp128, TSDataType::Double);
    let mut output = Vec::new();

    println!("Encoding {} values step by step...\n", data.len());

    for (i, &value) in data.iter().enumerate() {
        let _before_len = output.len();
        encoder.encode_f64(value, &mut output).unwrap();

        // Note: encode_f64 doesn't write to output immediately in Chimp128
        // It accumulates in internal buffer

        if i < 20 || (i > 0 && value != data[i-1]) {
            let same = if i > 0 && value == data[i-1] { "SAME" } else { "DIFF" };
            println!("[{:5}] {:8.4}°C  {} (buffer not yet written)", i, value, same);
        }
    }

    encoder.flush(&mut output).unwrap();
    println!("\nAfter flush: {} bytes total", output.len());

    output
}

fn main() {
    println!("=== Test 1: Perfect repetition (should be ~1 bit/value) ===\n");

    let constant_data = vec![20.0; 100];
    let encoded = compress_chimp128_step_by_step(&constant_data);

    let expected_bits: usize = 64 + 99;  // First value (64) + 99 × 1 bit
    let expected_bytes = expected_bits.div_ceil(8);

    println!("\nExpected: {} bits = {} bytes", expected_bits, expected_bytes);
    println!("Got:      {} bytes", encoded.len());
    println!("Bits per value: {:.2}", (encoded.len() * 8) as f64 / 100.0);

    if encoded.len() <= expected_bytes * 2 {
        println!("✅ Within reasonable overhead (<2x)");
    } else {
        println!("❌ Too much overhead (>2x expected)");
    }

    println!("\n=== Test 2: Pattern with transitions (should show encoding behavior) ===\n");

    // Create pattern: 10 × 20.0, then 10 × 20.1, then 10 × 20.0 again
    let mut pattern_data = Vec::new();
    pattern_data.extend(vec![20.0; 10]);
    pattern_data.extend(vec![20.1; 10]);
    pattern_data.extend(vec![20.0; 10]);

    let encoded = compress_chimp128_step_by_step(&pattern_data);

    // Expected:
    // - First value: 64 bits
    // - 9 × 20.0: 9 bits (identical)
    // - 1st 20.1: ~15 bits (new range)
    // - 9 × 20.1: 9 bits (identical)
    // - 1st 20.0 again: ~15 bits (back to old range)
    // - 9 × 20.0: 9 bits (identical)
    // Total: 64 + 9 + 15 + 9 + 15 + 9 = 121 bits = 16 bytes

    let expected_bits = 121;
    let expected_bytes = (expected_bits + 7) / 8;

    println!("\nExpected: ~{} bits = ~{} bytes", expected_bits, expected_bytes);
    println!("Got:      {} bytes", encoded.len());
    println!("Bits per value: {:.2}", (encoded.len() * 8) as f64 / 30.0);

    println!("\n=== Test 3: Byte-level inspection ===\n");

    let tiny_data = vec![20.0, 20.0, 20.0, 20.1, 20.1];
    let encoded = compress_chimp128_step_by_step(&tiny_data);

    println!("\nRaw bytes (first 20): {:02x?}", &encoded[..std::cmp::min(20, encoded.len())]);
    println!("Total size: {} bytes", encoded.len());

    // Expected: 64 + 2 + 15 + 1 = 82 bits = 11 bytes
    let expected: usize = 64 + 2 + 15 + 1;
    let expected = expected.div_ceil(8);
    println!("Expected: ~{} bytes", expected);

    if encoded.len() > expected * 3 {
        println!("\n❌ SERIOUS PROBLEM: 3x+ overhead detected");
        println!("   This suggests the encoder is writing much more data than expected");
        println!("   Possible causes:");
        println!("   1. BitVec::to_bytes() is inefficient");
        println!("   2. Encoder is writing metadata we don't expect");
        println!("   3. Bits are not being packed correctly");
    }
}
