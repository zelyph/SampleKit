//! The terminal workbench, as a prototype (proto/tui): `samplekit` alone in a
//! terminal opens it. `model` holds the state and the keys, and draws nothing;
//! `typing` the editors of what is typed in it; `view` draws it; `run` owns the
//! terminal; `start` is the page shown where there is no project.

pub mod model;
pub mod run;
pub mod start;
pub mod theme;
pub mod typing;
pub mod view;
