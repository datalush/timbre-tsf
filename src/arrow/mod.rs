/*
 * Licensed to the Apache Software Foundation (ASF) under one
 * or more contributor license agreements.  See the NOTICE file
 * distributed with this work for additional information
 * regarding copyright ownership.  The ASF licenses this file
 * to you under the Apache License, Version 2.0 (the
 * "License"); you may not use this file except in compliance
 * with the License.  You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing,
 * software distributed under the License is distributed on an
 * "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
 * KIND, either express or implied.  See the License for the
 * specific language governing permissions and limitations
 * under the License.
 */

//! Apache Arrow integration for Timbre
//!
//! This module provides bidirectional conversion between Apache Arrow and Timbre formats:
//!
//! - **Arrow -> Timbre**: Convert Arrow RecordBatches to Timbre (write path)
//! - **Timbre -> Arrow**: Read Timbre data as Arrow RecordBatches (read path)
//!
//! # Features
//!
//! - Zero-copy optimizations where possible
//! - Streaming support for large datasets
//! - Automatic encoding selection based on Arrow data types
//! - DataFusion integration for SQL queries on Timbre
//!
//! # Example: Arrow -> Timbre
//!
//! ```no_run
//! use timbre_tsf::arrow::FromArrowConverter;
//! use arrow::record_batch::RecordBatch;
//!
//! let mut converter = FromArrowConverter::builder("output.timbreile")
//!     .with_device_column("device_id")
//!     .with_timestamp_column("timestamp")
//!     .build()?;
//!
//! // converter.write_batch(&record_batch)?;
//! converter.finish()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Example: Timbre -> Arrow
//!
//! ```no_run
//! use timbre_tsf::arrow::RecordBatchReader;
//!
//! let reader = RecordBatchReader::try_new("input.timbreile")?;
//!
//! for batch in reader {
//!     let batch = batch?;
//!     // Process RecordBatch
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod aligned_buffer;
mod from_arrow;
mod schema_mapping;
mod to_arrow;
mod types;

pub use aligned_buffer::{ARROW_ALIGNMENT, AlignedVec, alloc_aligned_vec};
pub use from_arrow::FromArrowConverter;
pub use schema_mapping::{ArrowSchemaMapping, arrow_type_to_timbre, timbre_type_to_arrow};
pub use to_arrow::RecordBatchReader;
pub use types::{ArrowConversionConfig, EncodingHint};
