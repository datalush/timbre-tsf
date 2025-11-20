//! Time-based filters for timestamp filtering
//!
//! TimeFilter enables efficient filtering at multiple levels:
//! - Row level: Filter individual timestamps
//! - Statistics level: Skip chunks based on min/max time ranges

/// Time-based filters for timestamp columns
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeFilter {
    /// timestamp > value
    GreaterThan(i64),

    /// timestamp >= value
    GreaterThanOrEqual(i64),

    /// timestamp < value
    LessThan(i64),

    /// timestamp <= value
    LessThanOrEqual(i64),

    /// min_time <= timestamp <= max_time
    Between(i64, i64),

    /// timestamp == value
    Equals(i64),

    /// timestamp != value
    NotEquals(i64),

    /// timestamp IN [set]
    In(Vec<i64>),

    /// timestamp NOT IN [set]
    NotIn(Vec<i64>),
}

impl TimeFilter {
    /// Check if a single timestamp matches the filter
    ///
    /// # Example
    /// ```
    /// use timbre_tsf::query::TimeFilter;
    ///
    /// let filter = TimeFilter::Between(1000, 2000);
    /// assert!(filter.matches(1500));
    /// assert!(!filter.matches(500));
    /// ```
    pub fn matches(&self, timestamp: i64) -> bool {
        match self {
            TimeFilter::GreaterThan(t) => timestamp > *t,
            TimeFilter::GreaterThanOrEqual(t) => timestamp >= *t,
            TimeFilter::LessThan(t) => timestamp < *t,
            TimeFilter::LessThanOrEqual(t) => timestamp <= *t,
            TimeFilter::Between(min, max) => timestamp >= *min && timestamp <= *max,
            TimeFilter::Equals(t) => timestamp == *t,
            TimeFilter::NotEquals(t) => timestamp != *t,
            TimeFilter::In(set) => set.contains(&timestamp),
            TimeFilter::NotIn(set) => !set.contains(&timestamp),
        }
    }

    /// Check if time range might contain matching values
    ///
    /// This is used for statistics-based chunk skipping. Returns false
    /// if the chunk can definitely be skipped, true if it might contain
    /// matching values.
    ///
    /// # Example
    /// ```
    /// use timbre_tsf::query::TimeFilter;
    ///
    /// let filter = TimeFilter::GreaterThan(5000);
    ///
    /// // Range [1000, 2000] is completely before 5000, can skip
    /// assert!(!filter.might_match_range(1000, 2000));
    ///
    /// // Range [4000, 6000] overlaps, cannot skip
    /// assert!(filter.might_match_range(4000, 6000));
    /// ```
    pub fn might_match_range(&self, min_time: i64, max_time: i64) -> bool {
        match self {
            TimeFilter::GreaterThan(t) => max_time > *t,
            TimeFilter::GreaterThanOrEqual(t) => max_time >= *t,
            TimeFilter::LessThan(t) => min_time < *t,
            TimeFilter::LessThanOrEqual(t) => min_time <= *t,
            TimeFilter::Between(min, max) => !(max_time < *min || min_time > *max),
            TimeFilter::Equals(t) => *t >= min_time && *t <= max_time,
            TimeFilter::NotEquals(_) => true, // Can't skip based on range
            TimeFilter::In(set) => {
                // Check if any value in set is within range
                set.iter().any(|t| *t >= min_time && *t <= max_time)
            }
            TimeFilter::NotIn(_) => true, // Can't skip based on range
        }
    }

    /// Negate the filter
    pub fn negate(self) -> Self {
        match self {
            TimeFilter::GreaterThan(t) => TimeFilter::LessThanOrEqual(t),
            TimeFilter::GreaterThanOrEqual(t) => TimeFilter::LessThan(t),
            TimeFilter::LessThan(t) => TimeFilter::GreaterThanOrEqual(t),
            TimeFilter::LessThanOrEqual(t) => TimeFilter::GreaterThan(t),
            TimeFilter::Equals(t) => TimeFilter::NotEquals(t),
            TimeFilter::NotEquals(t) => TimeFilter::Equals(t),
            TimeFilter::In(set) => TimeFilter::NotIn(set),
            TimeFilter::NotIn(set) => TimeFilter::In(set),
            TimeFilter::Between(_, _) => {
                // Cannot directly negate Between, caller should use NOT predicate
                self
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_filter_greater_than() {
        let filter = TimeFilter::GreaterThan(1000);
        assert!(!filter.matches(500));
        assert!(!filter.matches(1000));
        assert!(filter.matches(1001));
        assert!(filter.matches(2000));
    }

    #[test]
    fn test_time_filter_greater_than_or_equal() {
        let filter = TimeFilter::GreaterThanOrEqual(1000);
        assert!(!filter.matches(999));
        assert!(filter.matches(1000));
        assert!(filter.matches(1001));
    }

    #[test]
    fn test_time_filter_less_than() {
        let filter = TimeFilter::LessThan(1000);
        assert!(filter.matches(999));
        assert!(!filter.matches(1000));
        assert!(!filter.matches(1001));
    }

    #[test]
    fn test_time_filter_between() {
        let filter = TimeFilter::Between(1000, 2000);
        assert!(!filter.matches(500));
        assert!(filter.matches(1000));
        assert!(filter.matches(1500));
        assert!(filter.matches(2000));
        assert!(!filter.matches(2500));
    }

    #[test]
    fn test_time_filter_equals() {
        let filter = TimeFilter::Equals(1000);
        assert!(!filter.matches(999));
        assert!(filter.matches(1000));
        assert!(!filter.matches(1001));
    }

    #[test]
    fn test_time_filter_in() {
        let filter = TimeFilter::In(vec![100, 200, 300]);
        assert!(filter.matches(100));
        assert!(filter.matches(200));
        assert!(filter.matches(300));
        assert!(!filter.matches(150));
        assert!(!filter.matches(400));
    }

    #[test]
    fn test_time_filter_range_skip_greater_than() {
        let filter = TimeFilter::GreaterThan(5000);

        // Range completely before threshold - can skip
        assert!(!filter.might_match_range(1000, 2000));
        assert!(!filter.might_match_range(4000, 5000));

        // Range overlaps or after threshold - cannot skip
        assert!(filter.might_match_range(4000, 6000));
        assert!(filter.might_match_range(6000, 7000));
    }

    #[test]
    fn test_time_filter_range_skip_less_than() {
        let filter = TimeFilter::LessThan(5000);

        // Range completely after threshold - can skip
        assert!(!filter.might_match_range(6000, 7000));
        assert!(!filter.might_match_range(5000, 6000));

        // Range overlaps or before threshold - cannot skip
        assert!(filter.might_match_range(4000, 6000));
        assert!(filter.might_match_range(1000, 2000));
    }

    #[test]
    fn test_time_filter_range_skip_between() {
        let filter = TimeFilter::Between(2000, 5000);

        // Range completely before - can skip
        assert!(!filter.might_match_range(100, 1000));

        // Range completely after - can skip
        assert!(!filter.might_match_range(6000, 7000));

        // Range overlaps - cannot skip
        assert!(filter.might_match_range(1000, 3000));
        assert!(filter.might_match_range(4000, 6000));
        assert!(filter.might_match_range(2500, 3500));
    }

    #[test]
    fn test_time_filter_range_skip_equals() {
        let filter = TimeFilter::Equals(2500);

        // Range doesn't contain value - can skip
        assert!(!filter.might_match_range(1000, 2000));
        assert!(!filter.might_match_range(3000, 4000));

        // Range contains value - cannot skip
        assert!(filter.might_match_range(2000, 3000));
        assert!(filter.might_match_range(2500, 2500));
    }

    #[test]
    fn test_time_filter_range_skip_in() {
        let filter = TimeFilter::In(vec![1000, 2000, 3000]);

        // Range contains none of the values - can skip
        assert!(!filter.might_match_range(100, 500));
        assert!(!filter.might_match_range(3500, 4000));

        // Range contains at least one value - cannot skip
        assert!(filter.might_match_range(500, 1500));
        assert!(filter.might_match_range(1900, 2100));
        assert!(filter.might_match_range(500, 3500));
    }

    #[test]
    fn test_time_filter_negate() {
        let filter = TimeFilter::GreaterThan(1000);
        let negated = filter.negate();
        assert_eq!(negated, TimeFilter::LessThanOrEqual(1000));

        let filter = TimeFilter::Equals(1000);
        let negated = filter.negate();
        assert_eq!(negated, TimeFilter::NotEquals(1000));
    }
}
