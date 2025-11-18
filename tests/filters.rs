//! Integration tests for bloom filters and query filters

use std::collections::HashMap;
use tsfile::common::TsValue;
use tsfile::index::BloomFilter;
use tsfile::query::{Predicate, TimeFilter, ValueFilter};

#[test]
fn test_bloom_filter_integration() {
    // Create bloom filter for device IDs
    let mut bloom = BloomFilter::new(1000, 0.01);

    // Simulate inserting device IDs during write
    let devices = vec!["device001", "device002", "device003", "device004"];
    for device in &devices {
        bloom.insert(device);
    }

    // Query for existing devices - should all return true
    for device in &devices {
        assert!(
            bloom.might_contain(device),
            "Device {} should be found",
            device
        );
    }

    // Query for non-existent device - should return false (or rarely true for false positive)
    assert!(
        !bloom.might_contain(&"device999"),
        "Non-existent device should not be found"
    );

    // Test serialization round-trip
    let serialized = bloom.serialize();
    let deserialized = BloomFilter::deserialize(&serialized).unwrap();

    for device in &devices {
        assert!(
            deserialized.might_contain(device),
            "Device {} should be found after deserialization",
            device
        );
    }
}

#[test]
fn test_time_filter_basic() {
    let filter = TimeFilter::Between(1000, 2000);

    // Test individual timestamps
    assert!(filter.matches(1000));
    assert!(filter.matches(1500));
    assert!(filter.matches(2000));
    assert!(!filter.matches(500));
    assert!(!filter.matches(2500));

    // Test range-based skipping
    assert!(filter.might_match_range(1500, 1800)); // Overlap
    assert!(!filter.might_match_range(100, 500)); // Before
    assert!(!filter.might_match_range(3000, 4000)); // After
}

#[test]
fn test_value_filter_basic() {
    let filter = ValueFilter::GreaterThan(TsValue::Float(25.0));

    // Test individual values
    assert!(filter.matches(Some(&TsValue::Float(30.0))));
    assert!(!filter.matches(Some(&TsValue::Float(20.0))));
    assert!(!filter.matches(None));

    // Test range-based skipping
    assert!(filter.might_match_range(Some(&TsValue::Float(20.0)), Some(&TsValue::Float(30.0)))); // Overlap
    assert!(!filter.might_match_range(Some(&TsValue::Float(10.0)), Some(&TsValue::Float(20.0)))); // Can skip
}

#[test]
fn test_predicate_time_and_value() {
    // Create predicate: timestamp > 1000 AND temperature > 25.0
    let predicate = Predicate::And(vec![
        Predicate::Time(TimeFilter::GreaterThan(1000)),
        Predicate::Value(
            "temperature".to_string(),
            ValueFilter::GreaterThan(TsValue::Float(25.0)),
        ),
    ]);

    // Test case 1: Both conditions met
    let mut values = HashMap::new();
    values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
    assert!(predicate.evaluate(1500, &values));

    // Test case 2: Time condition not met
    assert!(!predicate.evaluate(500, &values));

    // Test case 3: Value condition not met
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    assert!(!predicate.evaluate(1500, &values));

    // Test case 4: Neither condition met
    assert!(!predicate.evaluate(500, &values));
}

#[test]
fn test_predicate_or_logic() {
    // Create predicate: timestamp < 1000 OR temperature > 30.0
    let predicate = Predicate::Or(vec![
        Predicate::Time(TimeFilter::LessThan(1000)),
        Predicate::Value(
            "temperature".to_string(),
            ValueFilter::GreaterThan(TsValue::Float(30.0)),
        ),
    ]);

    let mut values = HashMap::new();

    // Test case 1: First condition met
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    assert!(predicate.evaluate(500, &values));

    // Test case 2: Second condition met
    values.insert("temperature".to_string(), Some(TsValue::Float(35.0)));
    assert!(predicate.evaluate(1500, &values));

    // Test case 3: Both conditions met
    assert!(predicate.evaluate(500, &values));

    // Test case 4: Neither condition met
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    assert!(!predicate.evaluate(1500, &values));
}

#[test]
fn test_predicate_not_logic() {
    // Create predicate: NOT (temperature > 25.0)
    let predicate = Predicate::Not(Box::new(Predicate::Value(
        "temperature".to_string(),
        ValueFilter::GreaterThan(TsValue::Float(25.0)),
    )));

    let mut values = HashMap::new();

    // Test case 1: Value is 20.0 (NOT true = true)
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    assert!(predicate.evaluate(1000, &values));

    // Test case 2: Value is 30.0 (NOT false = true)
    values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
    assert!(!predicate.evaluate(1000, &values));
}

#[test]
fn test_predicate_complex() {
    // Create complex predicate:
    // (timestamp > 1000 AND temperature > 25) OR (humidity < 50 AND pressure > 1000)
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
                "pressure".to_string(),
                ValueFilter::GreaterThan(TsValue::Float(1000.0)),
            ),
        ]),
    ]);

    let mut values = HashMap::new();

    // Test case 1: First AND condition met
    values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
    values.insert("humidity".to_string(), Some(TsValue::Float(60.0)));
    values.insert("pressure".to_string(), Some(TsValue::Float(900.0)));
    assert!(predicate.evaluate(1500, &values));

    // Test case 2: Second AND condition met
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    values.insert("humidity".to_string(), Some(TsValue::Float(40.0)));
    values.insert("pressure".to_string(), Some(TsValue::Float(1100.0)));
    assert!(predicate.evaluate(500, &values));

    // Test case 3: Neither condition fully met
    values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
    values.insert("humidity".to_string(), Some(TsValue::Float(60.0)));
    values.insert("pressure".to_string(), Some(TsValue::Float(900.0)));
    assert!(!predicate.evaluate(500, &values));
}

#[test]
fn test_predicate_chunk_skip_optimization() {
    // Create predicate: timestamp > 5000
    let predicate = Predicate::Time(TimeFilter::GreaterThan(5000));
    let stats = HashMap::new();

    // Chunk with time range [1000, 2000] should be skipped
    assert!(!predicate.might_match_chunk((1000, 2000), &stats));

    // Chunk with time range [4000, 6000] should NOT be skipped (overlaps)
    assert!(predicate.might_match_chunk((4000, 6000), &stats));

    // Chunk with time range [6000, 7000] should NOT be skipped (completely after)
    assert!(predicate.might_match_chunk((6000, 7000), &stats));
}

#[test]
fn test_predicate_chunk_skip_and_optimization() {
    // Create predicate: timestamp > 2000 AND timestamp < 8000
    let predicate = Predicate::And(vec![
        Predicate::Time(TimeFilter::GreaterThan(2000)),
        Predicate::Time(TimeFilter::LessThan(8000)),
    ]);
    let stats = HashMap::new();

    // Chunk [1000, 1500] fails first condition
    assert!(!predicate.might_match_chunk((1000, 1500), &stats));

    // Chunk [9000, 10000] fails second condition
    assert!(!predicate.might_match_chunk((9000, 10000), &stats));

    // Chunk [3000, 4000] satisfies both conditions
    assert!(predicate.might_match_chunk((3000, 4000), &stats));

    // Chunk [1000, 3000] partially satisfies (overlaps)
    assert!(predicate.might_match_chunk((1000, 3000), &stats));
}

#[test]
fn test_predicate_chunk_skip_or_optimization() {
    // Create predicate: timestamp < 2000 OR timestamp > 8000
    let predicate = Predicate::Or(vec![
        Predicate::Time(TimeFilter::LessThan(2000)),
        Predicate::Time(TimeFilter::GreaterThan(8000)),
    ]);
    let stats = HashMap::new();

    // Chunk [1000, 1500] satisfies first condition
    assert!(predicate.might_match_chunk((1000, 1500), &stats));

    // Chunk [9000, 10000] satisfies second condition
    assert!(predicate.might_match_chunk((9000, 10000), &stats));

    // Chunk [4000, 6000] satisfies neither condition, can skip
    assert!(!predicate.might_match_chunk((4000, 6000), &stats));
}

#[test]
fn test_value_filter_multiple_types() {
    // Test Int32
    let filter = ValueFilter::Equals(TsValue::Int32(42));
    assert!(filter.matches(Some(&TsValue::Int32(42))));
    assert!(!filter.matches(Some(&TsValue::Int32(43))));

    // Test String
    let filter = ValueFilter::Equals(TsValue::Text("device001".to_string()));
    assert!(filter.matches(Some(&TsValue::Text("device001".to_string()))));
    assert!(!filter.matches(Some(&TsValue::Text("device002".to_string()))));

    // Test Boolean
    let filter = ValueFilter::Equals(TsValue::Boolean(true));
    assert!(filter.matches(Some(&TsValue::Boolean(true))));
    assert!(!filter.matches(Some(&TsValue::Boolean(false))));
}

#[test]
fn test_value_filter_in_set() {
    let filter = ValueFilter::In(vec![
        TsValue::Int32(10),
        TsValue::Int32(20),
        TsValue::Int32(30),
    ]);

    assert!(filter.matches(Some(&TsValue::Int32(10))));
    assert!(filter.matches(Some(&TsValue::Int32(20))));
    assert!(filter.matches(Some(&TsValue::Int32(30))));
    assert!(!filter.matches(Some(&TsValue::Int32(15))));
    assert!(!filter.matches(Some(&TsValue::Int32(40))));
}

#[test]
fn test_value_filter_null_handling() {
    let filter = ValueFilter::IsNull;
    assert!(filter.matches(None));
    assert!(!filter.matches(Some(&TsValue::Int32(42))));

    let filter = ValueFilter::IsNotNull;
    assert!(!filter.matches(None));
    assert!(filter.matches(Some(&TsValue::Int32(42))));
}

#[test]
fn test_predicate_simplification() {
    // Test single-element AND simplification
    let predicate = Predicate::And(vec![Predicate::Time(TimeFilter::GreaterThan(1000))]).simplify();
    assert!(matches!(predicate, Predicate::Time(_)));

    // Test double negation elimination
    let predicate = Predicate::Not(Box::new(Predicate::Not(Box::new(Predicate::Time(
        TimeFilter::GreaterThan(1000),
    )))))
    .simplify();
    assert!(matches!(predicate, Predicate::Time(_)));

    // Test nested AND flattening
    let predicate = Predicate::And(vec![
        Predicate::Time(TimeFilter::GreaterThan(1000)),
        Predicate::And(vec![
            Predicate::Time(TimeFilter::LessThan(2000)),
            Predicate::Time(TimeFilter::NotEquals(1500)),
        ]),
    ])
    .simplify();

    if let Predicate::And(predicates) = predicate {
        assert_eq!(predicates.len(), 3); // Should be flattened
    } else {
        panic!("Expected And predicate");
    }
}

#[test]
fn test_bloom_filter_false_positive_rate() {
    let mut bloom = BloomFilter::new(1000, 0.01);

    // Insert 1000 items
    for i in 0..1000 {
        bloom.insert(&i);
    }

    // All inserted items should be found
    for i in 0..1000 {
        assert!(bloom.might_contain(&i));
    }

    // Check false positive rate on non-inserted items
    let mut false_positives = 0;
    for i in 1000..2000 {
        if bloom.might_contain(&i) {
            false_positives += 1;
        }
    }

    let actual_fpp = false_positives as f64 / 1000.0;
    println!("Actual false positive rate: {:.4}", actual_fpp);
    println!("Target false positive rate: 0.0100");

    // Allow some margin - actual rate should be close to target
    assert!(
        actual_fpp < 0.03,
        "False positive rate {} exceeds threshold",
        actual_fpp
    );
}
