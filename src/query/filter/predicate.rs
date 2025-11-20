//! Complex predicates combining filters with boolean logic
//!
//! Predicates allow combining multiple filters with AND, OR, and NOT operations
//! to create sophisticated query conditions.

use super::{TimeFilter, ValueFilter};
use crate::common::{TsValue, statistic::Statistic};
use std::collections::HashMap;

/// Combine filters with boolean logic
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Single time filter
    Time(TimeFilter),

    /// Single value filter on a measurement
    /// (measurement_name, filter)
    Value(String, ValueFilter),

    /// AND combination - all predicates must be true
    And(Vec<Predicate>),

    /// OR combination - at least one predicate must be true
    Or(Vec<Predicate>),

    /// NOT negation - predicate must be false
    Not(Box<Predicate>),
}

impl Predicate {
    /// Evaluate predicate for a single row
    ///
    /// # Arguments
    /// * `timestamp` - The timestamp for this row
    /// * `values` - Map of measurement names to their values
    ///
    /// # Example
    /// ```
    /// use timbre_tsf::common::TsValue;
    /// use timbre_tsf::query::{Predicate, TimeFilter, ValueFilter};
    /// use std::collections::HashMap;
    ///
    /// let predicate = Predicate::And(vec![
    ///     Predicate::Time(TimeFilter::GreaterThan(1000)),
    ///     Predicate::Value("temperature".to_string(), ValueFilter::GreaterThan(TsValue::Float(25.0))),
    /// ]);
    ///
    /// let mut values = HashMap::new();
    /// values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
    ///
    /// assert!(predicate.evaluate(1500, &values));
    /// ```
    pub fn evaluate(&self, timestamp: i64, values: &HashMap<String, Option<TsValue>>) -> bool {
        match self {
            Predicate::Time(filter) => filter.matches(timestamp),
            Predicate::Value(measurement, filter) => {
                let value = values.get(measurement).and_then(|v| v.as_ref());
                filter.matches(value)
            }
            Predicate::And(predicates) => predicates.iter().all(|p| p.evaluate(timestamp, values)),
            Predicate::Or(predicates) => predicates.iter().any(|p| p.evaluate(timestamp, values)),
            Predicate::Not(predicate) => !predicate.evaluate(timestamp, values),
        }
    }

    /// Check if predicate might match based on statistics (for chunk skip optimization)
    ///
    /// Returns true if the chunk might contain matching data, false if it can definitely be skipped.
    ///
    /// # Arguments
    /// * `time_range` - (min_time, max_time) for the chunk
    /// * `statistics` - Map of measurement names to their statistics
    pub fn might_match_chunk(
        &self,
        time_range: (i64, i64),
        statistics: &HashMap<String, &dyn Statistic>,
    ) -> bool {
        match self {
            Predicate::Time(filter) => filter.might_match_range(time_range.0, time_range.1),
            Predicate::Value(measurement, _filter) => {
                // Conservative: if we don't have statistics, assume it might match
                if !statistics.contains_key(measurement) {
                    return true;
                }
                // TODO: Implement statistics-based evaluation using min/max from statistics
                // For now, conservative approach
                true
            }
            Predicate::And(predicates) => {
                // All predicates must potentially match
                predicates
                    .iter()
                    .all(|p| p.might_match_chunk(time_range, statistics))
            }
            Predicate::Or(predicates) => {
                // At least one predicate must potentially match
                predicates
                    .iter()
                    .any(|p| p.might_match_chunk(time_range, statistics))
            }
            Predicate::Not(_predicate) => {
                // Conservative: hard to determine negative conditions
                true
            }
        }
    }

    /// Create an AND predicate from multiple predicates
    pub fn and(predicates: Vec<Predicate>) -> Self {
        Predicate::And(predicates)
    }

    /// Create an OR predicate from multiple predicates
    pub fn or(predicates: Vec<Predicate>) -> Self {
        Predicate::Or(predicates)
    }

    /// Create a NOT predicate
    pub fn negate(predicate: Predicate) -> Self {
        Predicate::Not(Box::new(predicate))
    }

    /// Simplify the predicate tree
    ///
    /// Performs optimizations like:
    /// - Flattening nested AND/OR of same type
    /// - Removing single-element AND/OR
    /// - Double negation elimination
    pub fn simplify(self) -> Self {
        match self {
            Predicate::And(predicates) => {
                if predicates.is_empty() {
                    return Predicate::And(predicates);
                }
                if predicates.len() == 1 {
                    return predicates.into_iter().next().unwrap().simplify();
                }
                // Flatten nested ANDs
                let mut simplified = Vec::new();
                for pred in predicates {
                    let pred = pred.simplify();
                    if let Predicate::And(inner) = pred {
                        simplified.extend(inner);
                    } else {
                        simplified.push(pred);
                    }
                }
                Predicate::And(simplified)
            }
            Predicate::Or(predicates) => {
                if predicates.is_empty() {
                    return Predicate::Or(predicates);
                }
                if predicates.len() == 1 {
                    return predicates.into_iter().next().unwrap().simplify();
                }
                // Flatten nested ORs
                let mut simplified = Vec::new();
                for pred in predicates {
                    let pred = pred.simplify();
                    if let Predicate::Or(inner) = pred {
                        simplified.extend(inner);
                    } else {
                        simplified.push(pred);
                    }
                }
                Predicate::Or(simplified)
            }
            Predicate::Not(inner) => {
                let simplified = inner.simplify();
                // Double negation elimination
                if let Predicate::Not(double_inner) = simplified {
                    return double_inner.simplify();
                }
                Predicate::Not(Box::new(simplified))
            }
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predicate_time_only() {
        let predicate = Predicate::Time(TimeFilter::Between(1000, 2000));
        let values = HashMap::new();

        assert!(predicate.evaluate(1500, &values));
        assert!(!predicate.evaluate(500, &values));
    }

    #[test]
    fn test_predicate_value_only() {
        let predicate = Predicate::Value(
            "temperature".to_string(),
            ValueFilter::GreaterThan(TsValue::Float(25.0)),
        );

        let mut values = HashMap::new();
        values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
        assert!(predicate.evaluate(1000, &values));

        values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
        assert!(!predicate.evaluate(1000, &values));
    }

    #[test]
    fn test_predicate_and() {
        let predicate = Predicate::And(vec![
            Predicate::Time(TimeFilter::GreaterThan(1000)),
            Predicate::Value(
                "temperature".to_string(),
                ValueFilter::GreaterThan(TsValue::Float(25.0)),
            ),
        ]);

        let mut values = HashMap::new();
        values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));

        // Both conditions met
        assert!(predicate.evaluate(1500, &values));

        // Time condition not met
        assert!(!predicate.evaluate(500, &values));

        // Value condition not met
        values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
        assert!(!predicate.evaluate(1500, &values));
    }

    #[test]
    fn test_predicate_or() {
        let predicate = Predicate::Or(vec![
            Predicate::Time(TimeFilter::LessThan(1000)),
            Predicate::Value(
                "temperature".to_string(),
                ValueFilter::GreaterThan(TsValue::Float(30.0)),
            ),
        ]);

        let mut values = HashMap::new();

        // Time condition met
        values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
        assert!(predicate.evaluate(500, &values));

        // Value condition met
        values.insert("temperature".to_string(), Some(TsValue::Float(35.0)));
        assert!(predicate.evaluate(1500, &values));

        // Both conditions met
        assert!(predicate.evaluate(500, &values));

        // Neither condition met
        values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
        assert!(!predicate.evaluate(1500, &values));
    }

    #[test]
    fn test_predicate_not() {
        let predicate = Predicate::Not(Box::new(Predicate::Time(TimeFilter::GreaterThan(1000))));
        let values = HashMap::new();

        assert!(predicate.evaluate(500, &values));
        assert!(!predicate.evaluate(1500, &values));
    }

    #[test]
    fn test_predicate_complex() {
        // (timestamp > 1000 AND temperature > 25) OR humidity < 50
        let predicate = Predicate::Or(vec![
            Predicate::And(vec![
                Predicate::Time(TimeFilter::GreaterThan(1000)),
                Predicate::Value(
                    "temperature".to_string(),
                    ValueFilter::GreaterThan(TsValue::Float(25.0)),
                ),
            ]),
            Predicate::Value(
                "humidity".to_string(),
                ValueFilter::LessThan(TsValue::Float(50.0)),
            ),
        ]);

        let mut values = HashMap::new();
        values.insert("temperature".to_string(), Some(TsValue::Float(30.0)));
        values.insert("humidity".to_string(), Some(TsValue::Float(60.0)));

        // First AND condition met
        assert!(predicate.evaluate(1500, &values));

        // Second condition met
        values.insert("temperature".to_string(), Some(TsValue::Float(20.0)));
        values.insert("humidity".to_string(), Some(TsValue::Float(40.0)));
        assert!(predicate.evaluate(500, &values));

        // Neither condition met
        values.insert("humidity".to_string(), Some(TsValue::Float(60.0)));
        assert!(!predicate.evaluate(500, &values));
    }

    #[test]
    fn test_predicate_chunk_skip_time() {
        let predicate = Predicate::Time(TimeFilter::GreaterThan(5000));
        let stats = HashMap::new();

        // Chunk range [1000, 2000] is before 5000, can skip
        assert!(!predicate.might_match_chunk((1000, 2000), &stats));

        // Chunk range [4000, 6000] overlaps, cannot skip
        assert!(predicate.might_match_chunk((4000, 6000), &stats));
    }

    #[test]
    fn test_predicate_chunk_skip_and() {
        let predicate = Predicate::And(vec![
            Predicate::Time(TimeFilter::GreaterThan(5000)),
            Predicate::Time(TimeFilter::LessThan(10000)),
        ]);
        let stats = HashMap::new();

        // Chunk range [1000, 2000] fails first condition, can skip
        assert!(!predicate.might_match_chunk((1000, 2000), &stats));

        // Chunk range [11000, 12000] fails second condition, can skip
        assert!(!predicate.might_match_chunk((11000, 12000), &stats));

        // Chunk range [6000, 7000] satisfies both, cannot skip
        assert!(predicate.might_match_chunk((6000, 7000), &stats));
    }

    #[test]
    fn test_predicate_chunk_skip_or() {
        let predicate = Predicate::Or(vec![
            Predicate::Time(TimeFilter::LessThan(2000)),
            Predicate::Time(TimeFilter::GreaterThan(8000)),
        ]);
        let stats = HashMap::new();

        // Chunk range [1000, 1500] matches first condition
        assert!(predicate.might_match_chunk((1000, 1500), &stats));

        // Chunk range [9000, 10000] matches second condition
        assert!(predicate.might_match_chunk((9000, 10000), &stats));

        // Chunk range [4000, 6000] matches neither, can skip
        assert!(!predicate.might_match_chunk((4000, 6000), &stats));
    }

    #[test]
    fn test_predicate_simplify_single_and() {
        let predicate =
            Predicate::And(vec![Predicate::Time(TimeFilter::GreaterThan(1000))]).simplify();

        assert!(matches!(predicate, Predicate::Time(_)));
    }

    #[test]
    fn test_predicate_simplify_nested_and() {
        let predicate = Predicate::And(vec![
            Predicate::Time(TimeFilter::GreaterThan(1000)),
            Predicate::And(vec![
                Predicate::Time(TimeFilter::LessThan(2000)),
                Predicate::Time(TimeFilter::NotEquals(1500)),
            ]),
        ])
        .simplify();

        if let Predicate::And(predicates) = predicate {
            assert_eq!(predicates.len(), 3); // Flattened
        } else {
            panic!("Expected And predicate");
        }
    }

    #[test]
    fn test_predicate_simplify_double_negation() {
        let predicate = Predicate::Not(Box::new(Predicate::Not(Box::new(Predicate::Time(
            TimeFilter::GreaterThan(1000),
        )))))
        .simplify();

        assert!(matches!(predicate, Predicate::Time(_)));
    }

    #[test]
    fn test_predicate_builder_helpers() {
        let p1 = Predicate::Time(TimeFilter::GreaterThan(1000));
        let p2 = Predicate::Time(TimeFilter::LessThan(2000));

        let and_pred = Predicate::and(vec![p1.clone(), p2.clone()]);
        assert!(matches!(and_pred, Predicate::And(_)));

        let or_pred = Predicate::or(vec![p1.clone(), p2.clone()]);
        assert!(matches!(or_pred, Predicate::Or(_)));

        let not_pred = Predicate::negate(p1);
        assert!(matches!(not_pred, Predicate::Not(_)));
    }
}
