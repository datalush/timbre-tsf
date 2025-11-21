//! Utility modules for Timbre TSF.
//!
//! This module contains cross-cutting utilities that don't fit into the core
//! encoding, compression, or I/O modules.

pub mod encoding_analyzer;

pub use encoding_analyzer::{recommend_encoding, recommend_compression, DataPattern};
