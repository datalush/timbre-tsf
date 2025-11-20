use timbre_tsf::common::{TSDataType, TSEncoding};
use timbre_tsf::encoding::create_encoder;
use rand::Rng;

fn generate_iot_temperature(n: usize, stability: f64) -> Vec<f64> {
    let mut rng = rand::thread_rng();
    let mut result = Vec::with_capacity(n);

    let possible_temps: Vec<f64> = (180..=250)
        .map(|t| t as f64 / 10.0)
        .collect();

    let mut current_idx = 20;

    for _ in 0..n {
        if rng.gen_range(0.0..1.0) > stability {
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
    println!("=== Análisis Detallado de Chimp128 Compression ===\n");

    let data = generate_iot_temperature(10000, 0.85);

    // Análisis 1: Conteo de valores únicos
    let mut unique_values = data.clone();
    unique_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    unique_values.dedup();
    println!("📊 Valores únicos: {} de {} ({:.2}%)",
             unique_values.len(),
             data.len(),
             (unique_values.len() as f64 / data.len() as f64) * 100.0);

    // Análisis 2: Conteo de valores IDÉNTICOS consecutivos
    let mut identical_count = 0;
    for i in 1..data.len() {
        if data[i] == data[i-1] {
            identical_count += 1;
        }
    }
    let identical_pct = (identical_count as f64 / (data.len() - 1) as f64) * 100.0;
    println!("🔁 Valores idénticos consecutivos: {} de {} ({:.2}%)",
             identical_count,
             data.len() - 1,
             identical_pct);

    // Análisis 3: Verificar bits idénticos a nivel binario
    let mut bitwise_identical = 0;
    for i in 1..data.len() {
        if data[i].to_bits() == data[i-1].to_bits() {
            bitwise_identical += 1;
        }
    }
    println!("🔢 Valores bit-a-bit idénticos: {} de {} ({:.2}%)",
             bitwise_identical,
             data.len() - 1,
             (bitwise_identical as f64 / (data.len() - 1) as f64) * 100.0);

    // Análisis 4: Distribución de XOR valores
    println!("\n📈 Distribución de XOR entre valores consecutivos:");
    let mut xor_zero = 0;
    let mut xor_small = 0;  // < 100 bits diferentes
    let mut xor_large = 0;  // >= 100 bits diferentes

    for i in 1..std::cmp::min(data.len(), 1000) {
        let xor = data[i].to_bits() ^ data[i-1].to_bits();
        if xor == 0 {
            xor_zero += 1;
        } else if xor.count_ones() < 10 {
            xor_small += 1;
        } else {
            xor_large += 1;
        }
    }

    println!("  XOR = 0 (idénticos):     {} ({:.1}%)", xor_zero, xor_zero as f64 / 10.0);
    println!("  XOR pequeño (<10 bits):  {} ({:.1}%)", xor_small, xor_small as f64 / 10.0);
    println!("  XOR grande (>=10 bits):  {} ({:.1}%)", xor_large, xor_large as f64 / 10.0);

    // Análisis 5: Primeros 30 valores para inspección visual
    println!("\n🔍 Primeros 30 valores (con bits):");
    for i in 0..30 {
        let bits = data[i].to_bits();
        let is_same = if i > 0 && data[i] == data[i-1] { "✓ IGUAL" } else { "" };
        println!("  [{:3}] {:.4}°C (0x{:016x}) {}", i, data[i], bits, is_same);
    }

    // Análisis 6: Compresión Chimp128
    println!("\n💾 Compresión Chimp128:");
    let encoded = compress_chimp128(&data);
    let raw_size = data.len() * 8;
    let bits_per_value = (encoded.len() * 8) as f64 / data.len() as f64;
    let ratio = raw_size as f64 / encoded.len() as f64;

    println!("  Raw size:        {} bytes", raw_size);
    println!("  Encoded size:    {} bytes", encoded.len());
    println!("  Bits per value:  {:.2}", bits_per_value);
    println!("  Compression:     {:.2}x", ratio);

    // Análisis 7: Compresión teórica esperada
    println!("\n🧮 Compresión teórica esperada:");
    let first_value_bits = 64;
    let identical_values_bits = identical_count;  // 1 bit cada uno
    let different_values = (data.len() - 1) - identical_count;
    let different_values_bits = different_values * 12;  // ~12 bits promedio para valores diferentes

    let theoretical_bits = first_value_bits + identical_values_bits + different_values_bits;
    let theoretical_bytes = theoretical_bits.div_ceil(8);
    let theoretical_bits_per_value = theoretical_bits as f64 / data.len() as f64;

    println!("  Primera valor:        {} bits", first_value_bits);
    println!("  {} idénticos × 1 bit:   {} bits", identical_count, identical_values_bits);
    println!("  {} diferentes × ~12 bits: {} bits", different_values, different_values_bits);
    println!("  Total teórico:        {} bits = {} bytes", theoretical_bits, theoretical_bytes);
    println!("  Bits/valor teórico:   {:.2}", theoretical_bits_per_value);

    println!("\n⚠️  Comparación:");
    println!("  Teórico:  {:.2} bits/valor ({} bytes)", theoretical_bits_per_value, theoretical_bytes);
    println!("  Real:     {:.2} bits/valor ({} bytes)", bits_per_value, encoded.len());
    println!("  Overhead: {:.2}x ({} bytes extra)",
             bits_per_value / theoretical_bits_per_value,
             encoded.len() as i32 - theoretical_bytes as i32);

    if bits_per_value > theoretical_bits_per_value * 2.0 {
        println!("\n❌ PROBLEMA: Chimp128 está usando 2x+ más bits de lo esperado!");
        println!("   Posibles causas:");
        println!("   1. No detecta valores idénticos correctamente");
        println!("   2. Overhead de metadata demasiado alto");
        println!("   3. Bug en la implementación");
    } else if bits_per_value > theoretical_bits_per_value * 1.2 {
        println!("\n⚠️  ADVERTENCIA: Chimp128 tiene ~20-100% overhead sobre teórico");
        println!("   Esto puede ser normal debido a metadata y casos edge.");
    } else {
        println!("\n✅ OK: Chimp128 está dentro de rango esperado (< 20% overhead)");
    }
}
