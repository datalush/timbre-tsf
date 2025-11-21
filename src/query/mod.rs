//! Query and filtering utilities for Timbre
//!
//! This module provides filtering capabilities for efficient data retrieval:
//! - Statistics-level filtering to skip chunks
//! - Time range filtering
//! - Value filtering with predicates
//! - Complex boolean logic (AND/OR/NOT)

pub mod filter;

pub use filter::{Predicate, TimeFilter, ValueFilter};
