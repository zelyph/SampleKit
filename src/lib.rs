//! SampleKit: sample data in plain Markdown — computed, current, tracked.
//!
//! # Reading this crate
//!
//! Modules are stacked in layers, and a module never depends on a higher
//! layer. The `samplekit` binary (`src/interfaces/cli.rs`) and the terminal
//! interface ([`tui`]) are on top; the Python extension, built only with the
//! `python` feature, is beside them. A module's tests are in `tests/`, in the
//! file of its name: `src/core/value.rs` is tested by `tests/value.rs`.
//!
//! | Layer | Modules |
//! | --- | --- |
//! | 0 · foundations | [`core::value`], [`core::statistics`], [`core::uncertainty`], [`core::formatting`], [`core::identifier`] |
//! | 1 · data model | [`core::property`], [`core::dependency_graph`], [`core::table`], [`core::sample`] |
//! | 2 · portable format | [`format::schema`], [`format::canonicalization`], [`format::fingerprint`], [`format::document`], [`format::migration`] |
//! | 3 · project | [`config::project_config`], [`config::discovery`], [`config::profiles`], [`config::version_control`], [`config::model_runtime`] |
//! | 4 · query | [`query::field_addressing`], [`query::filter_language`], [`query::ordering`] |
//! | 5 · collection | [`collection::sample_list`], [`collection::named_queries`], [`collection::validation`], [`collection::tagging`] |
//! | 6 · presentation | [`presentation::export_formats`], [`presentation::terminal_rendering`], [`collection::exports`] |
//! | 8 · Python | `python::pyo3_bridge`, `python::python_api` — with the `python` feature |
//!
//! # The rule that shapes the code
//!
//! *Silence is a defect.* An operation that cannot do what was asked says so,
//! with diagnostics: what was attempted, what exists instead, and a suggestion.
//! An unknown field is an error, not an empty result; a comparison that has no
//! answer fails rather than returning `false`. The domain is scientific
//! measurement that ends up in manuscripts, where a crash costs minutes and a
//! silent wrong answer costs a paper.

pub mod collection;
pub mod config;
pub mod core;
pub mod format;
pub mod presentation;
pub mod query;
pub mod tui;

// Built only for the extension.
#[cfg(feature = "python")]
pub mod python;
