//! The uncertainty of a quantity: what one is, what makes one valid, and how
//! one is derived from repeated readings.
//!

use std::fmt;

use crate::core::statistics::summarize;
use crate::core::value::Readings;

/// One uncertainty: finite, non-negative, **absolute**, and in the same unit as
/// the value it accompanies.
///
/// The magnitude is a quantity at **coverage factor 1** — one standard
/// deviation of something. It therefore has no sides, carries no `k`, and has
/// no components: an asymmetric interval, an expanded `U` and a
/// statistical/systematic split are all different objects, and storing one here
/// would force every reader of a single number to pick a half.
///
/// *Which* standard deviation it is — the mean's, the sample's, the
/// population's — is not fixed here, and is not recorded in the file. That is
/// the template's choice; see [`Convention`].
///
/// Zero is permitted and means *exactly known*. Absence is `Option<Uncertainty>`
/// at the use site, never a sentinel.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Uncertainty(f64);

impl Uncertainty {
    /// A negative magnitude is rejected rather than made positive: the sign
    /// means the caller computed something wrong — a difference taken in the
    /// wrong order — and silently correcting it discards the evidence.
    pub fn new(magnitude: f64) -> Result<Uncertainty, UncertaintyError> {
        if !magnitude.is_finite() {
            return Err(UncertaintyError::NotFinite);
        }
        if magnitude < 0.0 {
            return Err(UncertaintyError::Negative { magnitude });
        }
        // `-0.0` is 0: written `± -0` it read as a sign on nothing.
        Ok(Uncertainty(magnitude + 0.0))
    }

    /// *Exactly known* — a defined constant, a count, a nominal set point.
    /// Meaningful, and distinct from absence.
    pub fn zero() -> Uncertainty {
        Uncertainty(0.0)
    }

    pub fn magnitude(&self) -> f64 {
        self.0
    }

    /// This uncertainty as a fraction of a reference value.
    ///
    /// The reference's *magnitude* is used, so a negative reference yields a
    /// positive fraction: a relative uncertainty of −0.4% is not a thing, and
    /// the sign of the value it is relative to is not part of the question.
    ///
    /// A zero reference yields `None` rather than an infinity.
    pub fn relative_to(&self, value: f64) -> Option<f64> {
        if !value.is_finite() || value == 0.0 {
            return None;
        }
        Some(self.0 / value.abs())
    }
}

/// Which spread statistic a template asks for.
///
/// **Three of `Summary`'s ten fields are conventions, and the other seven are
/// not.** Only these three answer *how far from the value might the truth be*;
/// `u = stats.median` is the same shape and means nothing. A template naming
/// any other field is refused by `model-runtime`, where the name is read, which
/// is what keeps this enum from needing a fourth variant meaning *something the
/// user typed*.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Convention {
    StandardError,
    SampleStdev,
    PopulationStdev,
}

impl Convention {
    pub const ALL: [Convention; 3] = [
        Convention::StandardError,
        Convention::SampleStdev,
        Convention::PopulationStdev,
    ];

    /// The name a file writes, and the field of `Summary` it reads.
    pub fn name(&self) -> &'static str {
        match self {
            Convention::StandardError => "standard_error",
            Convention::SampleStdev => "sample_stdev",
            Convention::PopulationStdev => "population_stdev",
        }
    }

    pub fn from_name(name: &str) -> Option<Convention> {
        Convention::ALL.into_iter().find(|one| one.name() == name)
    }
}

/// Derive an uncertainty from repeated readings, under a named convention.
///
/// **There is no default, and that is the point.** The standard error and the
/// sample standard deviation differ by `sqrt(n)` — a factor of 3.3 for ten
/// readings — and they answer different questions. Deriving the standard error
/// automatically is the standard choice, and its failure mode is
/// the one this project exists to remove: it shrinks as `sqrt(n)`, so a hundred
/// readings of a drifting quantity produce a confident, tiny, wrong number.
///
/// **A single reading derives `None` under every convention**, including
/// `population_stdev`, whose statistic is defined and equals zero. That zero is
/// a correct description of a one-element series and a false uncertainty: no
/// reading count of one supports any statement about spread. The rule lives
/// here, in the module that makes the claim, and not in the one that describes.
pub fn from_readings(readings: &Readings, convention: Convention) -> Option<Uncertainty> {
    if readings.len().get() == 1 {
        return None;
    }
    let summary = summarize(readings);
    let magnitude = match convention {
        Convention::StandardError => summary.standard_error,
        Convention::SampleStdev => summary.sample_stdev,
        Convention::PopulationStdev => Some(summary.population_stdev),
    }?;
    // A spread of finite readings can still overflow — `1e160, 3e160` — and
    // an uncertainty that is no number is none, never a panic.
    Uncertainty::new(magnitude).ok()
}

/// The two ways a magnitude fails to be an uncertainty.
#[derive(Debug, Clone, PartialEq)]
pub enum UncertaintyError {
    Negative { magnitude: f64 },
    NotFinite,
}

impl fmt::Display for UncertaintyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The message quotes the magnitude, because the usual cause is a
            // subtraction performed in the wrong order and seeing the number
            // identifies it immediately.
            UncertaintyError::Negative { magnitude } => write!(
                f,
                "an uncertainty cannot be negative, and {magnitude} is: it is a distance \
                 from a value, not a difference between two"
            ),
            UncertaintyError::NotFinite => write!(
                f,
                "an uncertainty must be a finite number: NaN and infinity describe no \
                 measurement"
            ),
        }
    }
}

impl std::error::Error for UncertaintyError {}
