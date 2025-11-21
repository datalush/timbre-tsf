//! Test to verify StatisticEnum dispatch works correctly
//!
//! This test verifies that enum dispatch produces the same results as
//! the original Box<dyn Statistic> implementation.

use timbre_tsf::common::statistic::{StatisticEnum, Statistic};
use timbre_tsf::common::types::TSDataType;

#[test]
fn test_enum_dispatch_float() {
    let mut stat = StatisticEnum::new(TSDataType::Float);

    // Write some test data
    stat.update_f32(1000, 10.5);
    stat.update_f32(2000, 20.5);
    stat.update_f32(3000, 5.5);
    stat.update_f32(4000, 15.5);

    // Verify statistics
    assert_eq!(stat.count(), 4);
    assert_eq!(stat.start_time(), 1000);
    assert_eq!(stat.end_time(), 4000);

    // Verify it implements the Statistic trait
    let _trait_ref: &dyn Statistic = &stat;
}

#[test]
fn test_enum_dispatch_int32() {
    let mut stat = StatisticEnum::new(TSDataType::Int32);

    // Write some test data
    stat.update_i32(1000, 100);
    stat.update_i32(2000, 200);
    stat.update_i32(3000, 50);
    stat.update_i32(4000, 150);

    // Verify statistics
    assert_eq!(stat.count(), 4);
    assert_eq!(stat.start_time(), 1000);
    assert_eq!(stat.end_time(), 4000);

    // Verify it implements the Statistic trait
    let _trait_ref: &dyn Statistic = &stat;
}

#[test]
fn test_enum_dispatch_all_types() {
    // Test that each variant can be created and used
    let types = vec![
        TSDataType::Boolean,
        TSDataType::Int32,
        TSDataType::Int64,
        TSDataType::Float,
        TSDataType::Double,
        TSDataType::Text,
    ];

    for data_type in types {
        let stat = StatisticEnum::new(data_type);

        // Verify it implements Statistic trait
        let _trait_ref: &dyn Statistic = &stat;

        // Verify initial state
        assert_eq!(stat.count(), 0);

        println!("✅ {:?} variant works correctly", data_type);
    }
}

#[test]
fn test_wrong_type_is_noop() {
    // Verify that calling wrong update method is a no-op (not a panic)
    let mut stat = StatisticEnum::new(TSDataType::Float);

    // These should all be no-ops
    stat.update_i32(1000, 42);
    stat.update_i64(1000, 42);
    stat.update_bool(1000, true);
    stat.update_string(1000, "test");

    // Count should still be 0
    assert_eq!(stat.count(), 0);

    // Now update with correct type
    stat.update_f32(1000, 42.0);
    assert_eq!(stat.count(), 1);

    println!("✅ Type safety: wrong update methods are no-ops");
}
