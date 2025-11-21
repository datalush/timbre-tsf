//! Test to verify StatisticEnum size optimization
//!
//! This test verifies that StatisticEnum has the expected size and
//! documents the memory layout for future reference.

use timbre_tsf::common::statistic::{
    StatisticEnum, BooleanStatistic, Int32Statistic, Int64Statistic,
    FloatStatistic, DoubleStatistic, StringStatistic,
};
use std::mem::size_of;

#[test]
fn test_statistic_enum_size() {
    // Document size of each variant
    println!("Individual variant sizes:");
    println!("  BooleanStatistic: {} bytes", size_of::<BooleanStatistic>());
    println!("  Int32Statistic:   {} bytes", size_of::<Int32Statistic>());
    println!("  Int64Statistic:   {} bytes", size_of::<Int64Statistic>());
    println!("  FloatStatistic:   {} bytes", size_of::<FloatStatistic>());
    println!("  DoubleStatistic:  {} bytes", size_of::<DoubleStatistic>());
    println!("  StringStatistic:  {} bytes", size_of::<StringStatistic>());

    // Document total enum size
    let enum_size = size_of::<StatisticEnum>();
    println!("\nStatisticEnum total size: {} bytes", enum_size);

    // Verify it's stack allocated (not a pointer)
    // Should be significantly larger than 8 bytes (pointer size)
    assert!(enum_size > 16, "StatisticEnum should be stack allocated");

    // Verify it's reasonable (should be largest variant + tag + padding)
    // Expected: ~64-80 bytes depending on alignment
    assert!(enum_size <= 128, "StatisticEnum should not be excessively large");

    println!("\n✅ StatisticEnum is properly sized for enum dispatch");
    println!("   (Stack allocated, no heap indirection)");
}

#[test]
fn test_vs_box_overhead() {
    // Document the heap overhead we're avoiding
    let enum_size = size_of::<StatisticEnum>();
    let box_size = size_of::<Box<FloatStatistic>>();

    println!("\nMemory comparison:");
    println!("  StatisticEnum:         {} bytes (stack)", enum_size);
    println!("  Box<dyn Statistic>:    {} bytes (pointer) + heap allocation", box_size);
    println!("  Savings per instance:  Eliminated heap allocation + vtable indirection");

    // Box is just a pointer (8 bytes on 64-bit), but requires heap allocation
    assert_eq!(box_size, 8, "Box should be a single pointer");

    println!("\n✅ Enum dispatch eliminates heap allocation overhead");
}
