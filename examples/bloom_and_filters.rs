//! Example demonstrating Bloom Filters and Query Filters
//!
//! This example shows how to use:
//! 1. Bloom filters for probabilistic membership testing
//! 2. Time filters for timestamp-based queries
//! 3. Value filters for data filtering
//! 4. Complex predicates with AND/OR/NOT logic

use std::collections::HashMap;
use tsfile_rs::common::TsValue;
use tsfile_rs::index::BloomFilter;
use tsfile_rs::query::{Predicate, TimeFilter, ValueFilter};

fn main() {
    println!("=== Bloom Filter Example ===\n");
    bloom_filter_example();

    println!("\n=== Time Filter Example ===\n");
    time_filter_example();

    println!("\n=== Value Filter Example ===\n");
    value_filter_example();

    println!("\n=== Complex Predicate Example ===\n");
    complex_predicate_example();

    println!("\n=== Chunk Skip Optimization Example ===\n");
    chunk_skip_example();
}

fn bloom_filter_example() {
    // Create a bloom filter for 1000 expected items with 1% false positive rate
    let mut bloom = BloomFilter::new(1000, 0.01);

    // Insert device IDs
    let devices = vec![
        "sensor_001",
        "sensor_002",
        "sensor_003",
        "gateway_alpha",
        "gateway_beta",
    ];

    for device in &devices {
        bloom.insert(device);
    }

    println!("Inserted {} devices", devices.len());

    // Query for devices
    for device in &devices {
        if bloom.might_contain(device) {
            println!("  ✓ Found: {}", device);
        }
    }

    // Query for non-existent device
    let unknown = "sensor_999";
    if bloom.might_contain(&unknown) {
        println!("  ? Maybe found: {} (false positive)", unknown);
    } else {
        println!("  ✗ Definitely not found: {}", unknown);
    }

    println!(
        "\nBloom filter stats: {} bits, {} hash functions, {} items",
        bloom.num_bits(),
        bloom.num_hash_functions(),
        bloom.num_items()
    );
    println!(
        "Calculated false positive probability: {:.4}",
        bloom.false_positive_probability()
    );
}

fn time_filter_example() {
    // Create various time filters
    let filters = vec![
        ("After 2024-01-01", TimeFilter::GreaterThan(1704067200000)),
        (
            "Between 2024-01-01 and 2024-12-31",
            TimeFilter::Between(1704067200000, 1735689599000),
        ),
        (
            "Exactly 2024-06-15 12:00:00",
            TimeFilter::Equals(1718452800000),
        ),
    ];

    let test_timestamps = vec![
        ("2023-12-31", 1703980800000),
        ("2024-06-15 12:00:00", 1718452800000),
        ("2025-01-01", 1735689600000),
    ];

    for (filter_name, filter) in &filters {
        println!("Filter: {}", filter_name);
        for (ts_name, timestamp) in &test_timestamps {
            if filter.matches(*timestamp) {
                println!("  ✓ Matches: {}", ts_name);
            } else {
                println!("  ✗ No match: {}", ts_name);
            }
        }
        println!();
    }
}

fn value_filter_example() {
    // Create value filters for sensor data
    let temperature_filter = ValueFilter::GreaterThan(TsValue::Float(25.0));
    let humidity_filter = ValueFilter::LessThan(TsValue::Float(60.0));
    let status_filter = ValueFilter::Equals(TsValue::Text("active".to_string()));

    // Test data
    let readings = vec![
        ("Reading 1", TsValue::Float(28.5)),
        ("Reading 2", TsValue::Float(22.0)),
        ("Reading 3", TsValue::Float(30.0)),
    ];

    println!("Temperature > 25°C:");
    for (name, value) in &readings {
        if temperature_filter.matches(Some(value)) {
            println!("  ✓ {}: {:?}", name, value);
        } else {
            println!("  ✗ {}: {:?}", name, value);
        }
    }

    println!("\nHumidity readings:");
    let humidity_readings = vec![
        ("Sensor A", Some(TsValue::Float(55.0))),
        ("Sensor B", Some(TsValue::Float(70.0))),
        ("Sensor C", None),
    ];

    for (name, value) in &humidity_readings {
        if humidity_filter.matches(value.as_ref()) {
            println!("  ✓ {}: {:?}", name, value);
        } else {
            println!("  ✗ {}: {:?}", name, value);
        }
    }

    println!("\nStatus checks:");
    let statuses = vec![
        TsValue::Text("active".to_string()),
        TsValue::Text("idle".to_string()),
    ];

    for status in &statuses {
        if status_filter.matches(Some(status)) {
            println!("  ✓ Status: {:?}", status);
        } else {
            println!("  ✗ Status: {:?}", status);
        }
    }
}

fn complex_predicate_example() {
    // Create a complex predicate:
    // (timestamp > 1000 AND temperature > 25.0) OR (humidity < 50.0 AND status == "alert")
    let predicate = Predicate::Or(vec![
        Predicate::And(vec![
            Predicate::Time(TimeFilter::GreaterThan(1000)),
            Predicate::Value(
                "temperature".to_string(),
                ValueFilter::GreaterThan(TsValue::Float(25.0)),
            ),
        ]),
        Predicate::And(vec![
            Predicate::Value(
                "humidity".to_string(),
                ValueFilter::LessThan(TsValue::Float(50.0)),
            ),
            Predicate::Value(
                "status".to_string(),
                ValueFilter::Equals(TsValue::Text("alert".to_string())),
            ),
        ]),
    ]);

    // Test cases
    let test_cases = vec![
        (
            "Case 1: High temp, recent timestamp",
            1500,
            vec![
                ("temperature", Some(TsValue::Float(30.0))),
                ("humidity", Some(TsValue::Float(60.0))),
                ("status", Some(TsValue::Text("normal".to_string()))),
            ],
        ),
        (
            "Case 2: Low humidity, alert status",
            500,
            vec![
                ("temperature", Some(TsValue::Float(20.0))),
                ("humidity", Some(TsValue::Float(40.0))),
                ("status", Some(TsValue::Text("alert".to_string()))),
            ],
        ),
        (
            "Case 3: No conditions met",
            500,
            vec![
                ("temperature", Some(TsValue::Float(20.0))),
                ("humidity", Some(TsValue::Float(60.0))),
                ("status", Some(TsValue::Text("normal".to_string()))),
            ],
        ),
    ];

    println!("Complex predicate evaluation:");
    println!("Rule: (time > 1000 AND temp > 25) OR (humidity < 50 AND status == 'alert')\n");

    for (case_name, timestamp, data) in test_cases {
        let mut values = HashMap::new();
        for (key, value) in data {
            values.insert(key.to_string(), value);
        }

        let matches = predicate.evaluate(timestamp, &values);
        println!(
            "{}: {} (timestamp={})",
            case_name,
            if matches { "✓ MATCH" } else { "✗ NO MATCH" },
            timestamp
        );
    }
}

fn chunk_skip_example() {
    // Demonstrate chunk skip optimization using statistics
    let predicate = Predicate::And(vec![
        Predicate::Time(TimeFilter::GreaterThan(5000)),
        Predicate::Time(TimeFilter::LessThan(10000)),
    ]);

    let chunks = vec![
        ("Chunk A", (1000, 2000)),
        ("Chunk B", (3000, 6000)),
        ("Chunk C", (6000, 8000)),
        ("Chunk D", (11000, 12000)),
    ];

    println!("Chunk skip optimization (time range: 5000 < t < 10000):");
    println!();

    let statistics = HashMap::new();

    for (chunk_name, time_range) in &chunks {
        let can_skip = !predicate.might_match_chunk(*time_range, &statistics);
        if can_skip {
            println!(
                "  ✗ {} [{}, {}] - SKIP (outside range)",
                chunk_name, time_range.0, time_range.1
            );
        } else {
            println!(
                "  ✓ {} [{}, {}] - READ (may contain data)",
                chunk_name, time_range.0, time_range.1
            );
        }
    }

    println!("\nOptimization result:");
    let skipped = chunks
        .iter()
        .filter(|(_, range)| !predicate.might_match_chunk(*range, &statistics))
        .count();
    println!(
        "  Skipped {} out of {} chunks ({:.1}% reduction)",
        skipped,
        chunks.len(),
        (skipped as f64 / chunks.len() as f64) * 100.0
    );
}
