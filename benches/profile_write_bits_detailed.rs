//! Detailed instrumentation of write_bits() to count byte-by-byte vs batch writes
//!
//! This benchmark instruments the hot paths to understand:
//! 1. How many times we hit the byte-by-byte path vs batch path
//! 2. Average bits written per call
//! 3. Distribution of bit buffer fill levels
//!
//! Run with: cargo bench --bench profile_write_bits_detailed

use std::sync::atomic::{AtomicUsize, Ordering};

static BYTE_BY_BYTE_COUNT: AtomicUsize = AtomicUsize::new(0);
static BATCH_WRITE_COUNT: AtomicUsize = AtomicUsize::new(0);
static TOTAL_BITS_WRITTEN: AtomicUsize = AtomicUsize::new(0);
static WRITE_BITS_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Instrumented version of Gorilla encoder for F32
struct InstrumentedGorillaF32 {
    first_value: Option<u32>,
    previous_value: u32,
    previous_leading: u32,
    previous_trailing: u32,
    buffer: Vec<u8>,
    bit_buffer: u64,
    bits_in_buffer: u8,
}

impl InstrumentedGorillaF32 {
    fn new() -> Self {
        Self {
            first_value: None,
            previous_value: 0,
            previous_leading: i32::MAX as u32,
            previous_trailing: 0,
            buffer: Vec::with_capacity(100000),
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    #[inline(always)]
    fn write_bits(&mut self, value: u64, num_bits: u8) {
        if num_bits == 0 {
            return;
        }

        WRITE_BITS_CALLS.fetch_add(1, Ordering::Relaxed);
        TOTAL_BITS_WRITTEN.fetch_add(num_bits as usize, Ordering::Relaxed);

        let shift_amount = 64u8
            .saturating_sub(self.bits_in_buffer)
            .saturating_sub(num_bits);
        self.bit_buffer |= value << shift_amount;
        self.bits_in_buffer += num_bits;

        // Optimization: write 8 bytes at once when buffer is full
        if self.bits_in_buffer >= 64 {
            BATCH_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
            let bytes = self.bit_buffer.to_be_bytes();
            self.buffer.extend_from_slice(&bytes);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        } else {
            // Write complete bytes one at a time
            while self.bits_in_buffer >= 8 {
                BYTE_BY_BYTE_COUNT.fetch_add(1, Ordering::Relaxed);
                let byte = (self.bit_buffer >> 56) as u8;
                self.buffer.push(byte);
                self.bit_buffer <<= 8;
                self.bits_in_buffer -= 8;
            }
        }
    }

    #[inline(always)]
    fn write_bit(&mut self, bit: bool) {
        let shift = 63 - self.bits_in_buffer;
        self.bit_buffer |= (bit as u64) << shift;
        self.bits_in_buffer += 1;

        if self.bits_in_buffer >= 8 {
            BYTE_BY_BYTE_COUNT.fetch_add(1, Ordering::Relaxed);
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer <<= 8;
            self.bits_in_buffer -= 8;
        }
    }

    #[inline]
    fn encode_value(&mut self, bits: u32) {
        if self.first_value.is_none() {
            self.first_value = Some(bits);
            self.previous_value = bits;
            self.write_bits(bits as u64, 32);
            return;
        }

        let xor = self.previous_value ^ bits;

        if xor == 0 {
            self.write_bit(false);
        } else {
            self.write_bit(true);

            let leading = xor.leading_zeros();
            let trailing = xor.trailing_zeros();

            if leading >= self.previous_leading && trailing >= self.previous_trailing {
                self.write_bit(false);
                let significant_bits = 32 - self.previous_leading - self.previous_trailing;
                self.write_bits((xor >> self.previous_trailing) as u64, significant_bits as u8);
            } else {
                self.write_bit(true);
                self.write_bits(leading as u64, 5);
                let significant_bits = 32 - leading - trailing;
                self.write_bits((significant_bits - 1) as u64, 5);
                self.write_bits((xor >> trailing) as u64, significant_bits as u8);

                self.previous_leading = leading;
                self.previous_trailing = trailing;
            }
        }

        self.previous_value = bits;
    }

    fn encode_f32(&mut self, value: f32) {
        self.encode_value(value.to_bits());
    }

    fn flush(&mut self) {
        if self.bits_in_buffer > 0 {
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        }
    }
}

/// Instrumented version for F64
struct InstrumentedGorillaF64 {
    first_value: Option<u64>,
    previous_value: u64,
    previous_leading: u32,
    previous_trailing: u32,
    buffer: Vec<u8>,
    bit_buffer: u64,
    bits_in_buffer: u8,
}

impl InstrumentedGorillaF64 {
    fn new() -> Self {
        Self {
            first_value: None,
            previous_value: 0,
            previous_leading: i32::MAX as u32,
            previous_trailing: 0,
            buffer: Vec::with_capacity(100000),
            bit_buffer: 0,
            bits_in_buffer: 0,
        }
    }

    #[inline(always)]
    fn write_bits(&mut self, value: u64, num_bits: u8) {
        if num_bits == 0 {
            return;
        }

        WRITE_BITS_CALLS.fetch_add(1, Ordering::Relaxed);
        TOTAL_BITS_WRITTEN.fetch_add(num_bits as usize, Ordering::Relaxed);

        let shift_amount = 64u8
            .saturating_sub(self.bits_in_buffer)
            .saturating_sub(num_bits);
        self.bit_buffer |= value << shift_amount;
        self.bits_in_buffer += num_bits;

        if self.bits_in_buffer >= 64 {
            BATCH_WRITE_COUNT.fetch_add(1, Ordering::Relaxed);
            let bytes = self.bit_buffer.to_be_bytes();
            self.buffer.extend_from_slice(&bytes);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        } else {
            while self.bits_in_buffer >= 8 {
                BYTE_BY_BYTE_COUNT.fetch_add(1, Ordering::Relaxed);
                let byte = (self.bit_buffer >> 56) as u8;
                self.buffer.push(byte);
                self.bit_buffer <<= 8;
                self.bits_in_buffer -= 8;
            }
        }
    }

    #[inline(always)]
    fn write_bit(&mut self, bit: bool) {
        let shift = 63 - self.bits_in_buffer;
        self.bit_buffer |= (bit as u64) << shift;
        self.bits_in_buffer += 1;

        if self.bits_in_buffer >= 8 {
            BYTE_BY_BYTE_COUNT.fetch_add(1, Ordering::Relaxed);
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer <<= 8;
            self.bits_in_buffer -= 8;
        }
    }

    #[inline]
    fn encode_value(&mut self, bits: u64) {
        if self.first_value.is_none() {
            self.first_value = Some(bits);
            self.previous_value = bits;
            self.write_bits(bits, 64);
            return;
        }

        let xor = self.previous_value ^ bits;

        if xor == 0 {
            self.write_bit(false);
        } else {
            self.write_bit(true);

            let leading = xor.leading_zeros();
            let trailing = xor.trailing_zeros();

            if leading >= self.previous_leading && trailing >= self.previous_trailing {
                self.write_bit(false);
                let significant_bits = 64 - self.previous_leading - self.previous_trailing;
                self.write_bits(xor >> self.previous_trailing, significant_bits as u8);
            } else {
                self.write_bit(true);
                self.write_bits(leading as u64, 6);
                let significant_bits = 64 - leading - trailing;
                self.write_bits((significant_bits - 1) as u64, 6);
                self.write_bits(xor >> trailing, significant_bits as u8);

                self.previous_leading = leading;
                self.previous_trailing = trailing;
            }
        }

        self.previous_value = bits;
    }

    fn encode_f64(&mut self, value: f64) {
        self.encode_value(value.to_bits());
    }

    fn flush(&mut self) {
        if self.bits_in_buffer > 0 {
            let byte = (self.bit_buffer >> 56) as u8;
            self.buffer.push(byte);
            self.bit_buffer = 0;
            self.bits_in_buffer = 0;
        }
    }
}

fn reset_counters() {
    BYTE_BY_BYTE_COUNT.store(0, Ordering::Relaxed);
    BATCH_WRITE_COUNT.store(0, Ordering::Relaxed);
    TOTAL_BITS_WRITTEN.store(0, Ordering::Relaxed);
    WRITE_BITS_CALLS.store(0, Ordering::Relaxed);
}

fn print_stats(label: &str, num_values: usize) {
    let byte_by_byte = BYTE_BY_BYTE_COUNT.load(Ordering::Relaxed);
    let batch = BATCH_WRITE_COUNT.load(Ordering::Relaxed);
    let total_bits = TOTAL_BITS_WRITTEN.load(Ordering::Relaxed);
    let calls = WRITE_BITS_CALLS.load(Ordering::Relaxed);

    let avg_bits_per_call = if calls > 0 { total_bits as f64 / calls as f64 } else { 0.0 };
    let bits_per_value = total_bits as f64 / num_values as f64;

    println!("\n{}", label);
    println!("  Byte-by-byte writes: {}", byte_by_byte);
    println!("  Batch writes (8 bytes): {}", batch);
    println!("  Total bytes written: {}", byte_by_byte + batch * 8);
    println!("  write_bits() calls: {}", calls);
    println!("  Avg bits per call: {:.2}", avg_bits_per_call);
    println!("  Bits per value: {:.2}", bits_per_value);
    println!("  Byte-by-byte ratio: {:.1}%", (byte_by_byte as f64 / (byte_by_byte + batch * 8) as f64) * 100.0);
}

fn main() {
    println!("\n=== write_bits() Instrumentation Analysis ===\n");

    const NUM_VALUES: usize = 100_000;

    // Pattern 1: Sensor data (typical IoT)
    let sensor_f32: Vec<f32> = {
        let mut values = Vec::with_capacity(NUM_VALUES);
        let mut val = 100.0f32;
        for i in 0..NUM_VALUES {
            val += (i as f32 * 0.001).sin() * 0.01;
            values.push(val);
        }
        values
    };
    let sensor_f64: Vec<f64> = sensor_f32.iter().map(|&x| x as f64).collect();

    // Pattern 2: Constant
    let constant_f32 = vec![42.0f32; NUM_VALUES];
    let constant_f64 = vec![42.0f64; NUM_VALUES];

    // Pattern 3: Linear ramp
    let ramp_f32: Vec<f32> = (0..NUM_VALUES).map(|i| i as f32 * 0.1).collect();
    let ramp_f64: Vec<f64> = (0..NUM_VALUES).map(|i| i as f64 * 0.1).collect();

    println!("=== F32 Encoding ===");

    // Sensor F32
    reset_counters();
    let mut encoder = InstrumentedGorillaF32::new();
    for &v in &sensor_f32 {
        encoder.encode_f32(v);
    }
    encoder.flush();
    print_stats("Sensor F32", NUM_VALUES);

    // Constant F32
    reset_counters();
    let mut encoder = InstrumentedGorillaF32::new();
    for &v in &constant_f32 {
        encoder.encode_f32(v);
    }
    encoder.flush();
    print_stats("Constant F32", NUM_VALUES);

    // Ramp F32
    reset_counters();
    let mut encoder = InstrumentedGorillaF32::new();
    for &v in &ramp_f32 {
        encoder.encode_f32(v);
    }
    encoder.flush();
    print_stats("Ramp F32", NUM_VALUES);

    println!("\n=== F64 Encoding ===");

    // Sensor F64
    reset_counters();
    let mut encoder = InstrumentedGorillaF64::new();
    for &v in &sensor_f64 {
        encoder.encode_f64(v);
    }
    encoder.flush();
    print_stats("Sensor F64", NUM_VALUES);

    // Constant F64
    reset_counters();
    let mut encoder = InstrumentedGorillaF64::new();
    for &v in &constant_f64 {
        encoder.encode_f64(v);
    }
    encoder.flush();
    print_stats("Constant F64", NUM_VALUES);

    // Ramp F64
    reset_counters();
    let mut encoder = InstrumentedGorillaF64::new();
    for &v in &ramp_f64 {
        encoder.encode_f64(v);
    }
    encoder.flush();
    print_stats("Ramp F64", NUM_VALUES);

    println!("\n=== Analysis ===");
    println!("If byte-by-byte writes are the bottleneck, we should see:");
    println!("  1. F32 has higher byte-by-byte ratio than F64 in sensor/ramp patterns");
    println!("  2. Constant pattern has lowest byte-by-byte ratio (1 bit writes)");
    println!("  3. Higher byte-by-byte ratio correlates with slower performance");
}
