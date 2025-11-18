//! Value-based filters for data column filtering
//!
//! ValueFilter supports filtering on actual data values with type-aware comparisons.

use crate::common::TsValue;

/// Value-based filters for data columns
#[derive(Debug, Clone, PartialEq)]
pub enum ValueFilter {
    /// value > threshold
    GreaterThan(TsValue),

    /// value >= threshold
    GreaterThanOrEqual(TsValue),

    /// value < threshold
    LessThan(TsValue),

    /// value <= threshold
    LessThanOrEqual(TsValue),

    /// value == target
    Equals(TsValue),

    /// value != target
    NotEquals(TsValue),

    /// value IN [set]
    In(Vec<TsValue>),

    /// value NOT IN [set]
    NotIn(Vec<TsValue>),

    /// value IS NULL
    IsNull,

    /// value IS NOT NULL
    IsNotNull,
}

impl ValueFilter {
    /// Check if value matches filter
    ///
    /// # Example
    /// ```
    /// use tsfile::common::TsValue;
    /// use tsfile::query::ValueFilter;
    ///
    /// let filter = ValueFilter::GreaterThan(TsValue::Float(25.0));
    /// assert!(filter.matches(Some(&TsValue::Float(30.0))));
    /// assert!(!filter.matches(Some(&TsValue::Float(20.0))));
    /// assert!(!filter.matches(None));
    /// ```
    pub fn matches(&self, value: Option<&TsValue>) -> bool {
        match self {
            ValueFilter::IsNull => value.is_none(),
            ValueFilter::IsNotNull => value.is_some(),
            _ => {
                let val = match value {
                    Some(v) => v,
                    None => return false,
                };

                match self {
                    ValueFilter::GreaterThan(threshold) => Self::compare_gt(val, threshold),
                    ValueFilter::GreaterThanOrEqual(threshold) => Self::compare_gte(val, threshold),
                    ValueFilter::LessThan(threshold) => Self::compare_lt(val, threshold),
                    ValueFilter::LessThanOrEqual(threshold) => Self::compare_lte(val, threshold),
                    ValueFilter::Equals(target) => val == target,
                    ValueFilter::NotEquals(target) => val != target,
                    ValueFilter::In(set) => set.contains(val),
                    ValueFilter::NotIn(set) => !set.contains(val),
                    _ => unreachable!(),
                }
            }
        }
    }

    /// Check if value range might contain matching values (for statistics-based skip)
    ///
    /// Returns true if the chunk might contain matching values, false if it can definitely be skipped.
    pub fn might_match_range(&self, min: Option<&TsValue>, max: Option<&TsValue>) -> bool {
        match self {
            ValueFilter::IsNull => true,    // Conservative: might have nulls
            ValueFilter::IsNotNull => true, // Conservative: might have non-nulls
            ValueFilter::GreaterThan(threshold) => {
                // Can skip if max <= threshold
                match max {
                    Some(max_val) => !Self::compare_lte(max_val, threshold),
                    None => true,
                }
            }
            ValueFilter::GreaterThanOrEqual(threshold) => {
                // Can skip if max < threshold
                match max {
                    Some(max_val) => !Self::compare_lt(max_val, threshold),
                    None => true,
                }
            }
            ValueFilter::LessThan(threshold) => {
                // Can skip if min >= threshold
                match min {
                    Some(min_val) => !Self::compare_gte(min_val, threshold),
                    None => true,
                }
            }
            ValueFilter::LessThanOrEqual(threshold) => {
                // Can skip if min > threshold
                match min {
                    Some(min_val) => !Self::compare_gt(min_val, threshold),
                    None => true,
                }
            }
            ValueFilter::Equals(target) => {
                // Can skip if target < min or target > max
                match (min, max) {
                    (Some(min_val), Some(max_val)) => {
                        !Self::compare_lt(target, min_val) && !Self::compare_gt(target, max_val)
                    }
                    _ => true,
                }
            }
            ValueFilter::NotEquals(_) => true, // Can't skip
            ValueFilter::In(set) => {
                // Can skip if all values in set are outside [min, max]
                match (min, max) {
                    (Some(min_val), Some(max_val)) => set
                        .iter()
                        .any(|v| !Self::compare_lt(v, min_val) && !Self::compare_gt(v, max_val)),
                    _ => true,
                }
            }
            ValueFilter::NotIn(_) => true, // Can't skip
        }
    }

    /// Negate the filter
    pub fn negate(self) -> Self {
        match self {
            ValueFilter::GreaterThan(v) => ValueFilter::LessThanOrEqual(v),
            ValueFilter::GreaterThanOrEqual(v) => ValueFilter::LessThan(v),
            ValueFilter::LessThan(v) => ValueFilter::GreaterThanOrEqual(v),
            ValueFilter::LessThanOrEqual(v) => ValueFilter::GreaterThan(v),
            ValueFilter::Equals(v) => ValueFilter::NotEquals(v),
            ValueFilter::NotEquals(v) => ValueFilter::Equals(v),
            ValueFilter::In(set) => ValueFilter::NotIn(set),
            ValueFilter::NotIn(set) => ValueFilter::In(set),
            ValueFilter::IsNull => ValueFilter::IsNotNull,
            ValueFilter::IsNotNull => ValueFilter::IsNull,
        }
    }

    // Comparison helper functions
    fn compare_gt(a: &TsValue, b: &TsValue) -> bool {
        match (a, b) {
            (TsValue::Int32(av), TsValue::Int32(bv)) => av > bv,
            (TsValue::Int64(av), TsValue::Int64(bv)) => av > bv,
            (TsValue::Float(av), TsValue::Float(bv)) => av > bv,
            (TsValue::Double(av), TsValue::Double(bv)) => av > bv,
            (TsValue::Boolean(av), TsValue::Boolean(bv)) => av > bv,
            (TsValue::Text(av), TsValue::Text(bv)) => av > bv,
            (TsValue::String(av), TsValue::String(bv)) => av > bv,
            _ => false, // Type mismatch or incomparable
        }
    }

    fn compare_gte(a: &TsValue, b: &TsValue) -> bool {
        Self::compare_gt(a, b) || a == b
    }

    fn compare_lt(a: &TsValue, b: &TsValue) -> bool {
        match (a, b) {
            (TsValue::Int32(av), TsValue::Int32(bv)) => av < bv,
            (TsValue::Int64(av), TsValue::Int64(bv)) => av < bv,
            (TsValue::Float(av), TsValue::Float(bv)) => av < bv,
            (TsValue::Double(av), TsValue::Double(bv)) => av < bv,
            (TsValue::Boolean(av), TsValue::Boolean(bv)) => av < bv,
            (TsValue::Text(av), TsValue::Text(bv)) => av < bv,
            (TsValue::String(av), TsValue::String(bv)) => av < bv,
            _ => false, // Type mismatch or incomparable
        }
    }

    fn compare_lte(a: &TsValue, b: &TsValue) -> bool {
        Self::compare_lt(a, b) || a == b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_filter_int32() {
        let filter = ValueFilter::GreaterThan(TsValue::Int32(25));
        assert!(filter.matches(Some(&TsValue::Int32(30))));
        assert!(!filter.matches(Some(&TsValue::Int32(20))));
        assert!(!filter.matches(Some(&TsValue::Int32(25))));
    }

    #[test]
    fn test_value_filter_float() {
        let filter = ValueFilter::LessThanOrEqual(TsValue::Float(25.5));
        assert!(filter.matches(Some(&TsValue::Float(20.0))));
        assert!(filter.matches(Some(&TsValue::Float(25.5))));
        assert!(!filter.matches(Some(&TsValue::Float(30.0))));
    }

    #[test]
    fn test_value_filter_equals() {
        let filter = ValueFilter::Equals(TsValue::Int32(42));
        assert!(filter.matches(Some(&TsValue::Int32(42))));
        assert!(!filter.matches(Some(&TsValue::Int32(43))));
    }

    #[test]
    fn test_value_filter_in() {
        let filter = ValueFilter::In(vec![
            TsValue::Int32(10),
            TsValue::Int32(20),
            TsValue::Int32(30),
        ]);
        assert!(filter.matches(Some(&TsValue::Int32(10))));
        assert!(filter.matches(Some(&TsValue::Int32(20))));
        assert!(!filter.matches(Some(&TsValue::Int32(15))));
    }

    #[test]
    fn test_value_filter_null() {
        let filter = ValueFilter::IsNull;
        assert!(filter.matches(None));
        assert!(!filter.matches(Some(&TsValue::Int32(42))));

        let filter = ValueFilter::IsNotNull;
        assert!(!filter.matches(None));
        assert!(filter.matches(Some(&TsValue::Int32(42))));
    }

    #[test]
    fn test_value_filter_string() {
        let filter = ValueFilter::Equals(TsValue::Text("device001".to_string()));
        assert!(filter.matches(Some(&TsValue::Text("device001".to_string()))));
        assert!(!filter.matches(Some(&TsValue::Text("device002".to_string()))));
    }

    #[test]
    fn test_value_filter_range_skip_greater_than() {
        let filter = ValueFilter::GreaterThan(TsValue::Int32(50));

        // Max is 40, can skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Int32(10)), Some(&TsValue::Int32(40)));
        assert!(can_skip);

        // Max is 60, cannot skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Int32(40)), Some(&TsValue::Int32(60)));
        assert!(!can_skip);
    }

    #[test]
    fn test_value_filter_range_skip_less_than() {
        let filter = ValueFilter::LessThan(TsValue::Float(25.0));

        // Min is 30.0, can skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Float(30.0)), Some(&TsValue::Float(40.0)));
        assert!(can_skip);

        // Min is 20.0, cannot skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Float(20.0)), Some(&TsValue::Float(30.0)));
        assert!(!can_skip);
    }

    #[test]
    fn test_value_filter_range_skip_equals() {
        let filter = ValueFilter::Equals(TsValue::Int32(25));

        // Range [10, 20] doesn't contain 25, can skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Int32(10)), Some(&TsValue::Int32(20)));
        assert!(can_skip);

        // Range [20, 30] contains 25, cannot skip
        let can_skip =
            !filter.might_match_range(Some(&TsValue::Int32(20)), Some(&TsValue::Int32(30)));
        assert!(!can_skip);
    }

    #[test]
    fn test_value_filter_negate() {
        let filter = ValueFilter::GreaterThan(TsValue::Int32(10));
        let negated = filter.negate();
        assert_eq!(negated, ValueFilter::LessThanOrEqual(TsValue::Int32(10)));

        let filter = ValueFilter::IsNull;
        let negated = filter.negate();
        assert_eq!(negated, ValueFilter::IsNotNull);
    }
}
