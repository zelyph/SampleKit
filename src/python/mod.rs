//! The Python extension, `samplekit._native`.
//!
//! Built only with the `python` feature, and nothing below Layer 8 knows it
//! exists: `cargo build` without the feature is the command line, linking no
//! interpreter.

pub mod pyo3_bridge;
pub mod python_api;
