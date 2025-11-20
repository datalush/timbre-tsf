//! Core data types, schemas, and structures for TsFile.
//!
//! This module contains the fundamental building blocks for working with TsFile data:
//!
//! - **Types**: Data type enums ([`TSDataType`]), encoding types ([`TSEncoding`]),
//!   compression types ([`CompressionType`]), and value wrappers ([`TsValue`])
//! - **Schemas**: Schema definitions for measurements and tables ([`MeasurementSchema`], [`TableSchema`])
//! - **Tablets**: Batch data structures for efficient bulk operations ([`Tablet`])
//! - **Statistics**: Statistical summaries for query optimization ([`Statistics`] trait)
//!
//! # Overview
//!
//! The common module provides the type system that ensures compile-time safety
//! when working with time series data. All data operations flow through these
//! types, which enforce proper encoding, compression, and schema constraints.
//!
//! # Example
//!
//! ```rust
//! use timbre_tsf::common::*;
//!
//! // Define a measurement schema
//! let schema = MeasurementSchema::new(
//!     "temperature",
//!     TSDataType::Float,
//!     TSEncoding::Gorilla,
//!     CompressionType::Lz4,
//! );
//!
//! // Create a value
//! let value = TsValue::Float(25.5);
//! assert_eq!(value.data_type(), TSDataType::Float);
//! ```

pub mod schema;
pub mod statistic;
pub mod string_interner;
pub mod tablet;
pub mod types;

pub use schema::*;
pub use statistic::*;
pub use string_interner::StringInterner;
pub use tablet::*;
pub use types::*;
