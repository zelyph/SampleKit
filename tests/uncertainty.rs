//! The tests of `uncertainty`.

use samplekit::core::statistics::summarize;
use samplekit::core::uncertainty::{Convention, Uncertainty, UncertaintyError, from_readings};
use samplekit::core::value::Readings;

/// Eight readings, the same series `tests/statistics.rs` summarizes.
fn reference() -> Readings {
    Readings::new(vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap()
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-12 * left.abs().max(right.abs()).max(1.0)
}

#[test]
fn rejects_negative_magnitude() {
    // An error, not silently made positive: the sign means the caller computed
    // something wrong, and correcting it discards the evidence.
    let error = Uncertainty::new(-0.05).unwrap_err();
    assert_eq!(error, UncertaintyError::Negative { magnitude: -0.05 });
    // The message quotes the magnitude, which identifies the cause immediately.
    assert!(error.to_string().contains("0.05"), "{error}");
}

#[test]
fn rejects_non_finite() {
    assert_eq!(Uncertainty::new(f64::NAN), Err(UncertaintyError::NotFinite));
    assert_eq!(
        Uncertainty::new(f64::INFINITY),
        Err(UncertaintyError::NotFinite)
    );
    assert_eq!(
        Uncertainty::new(f64::NEG_INFINITY),
        Err(UncertaintyError::NotFinite)
    );
}

#[test]
fn zero_is_permitted() {
    // An exactly known quantity — a defined constant, a count, a set point.
    assert_eq!(Uncertainty::new(0.0).unwrap().magnitude(), 0.0);
    assert_eq!(Uncertainty::zero().magnitude(), 0.0);
}

#[test]
fn zero_differs_from_absent() {
    let known_exactly: Option<Uncertainty> = Some(Uncertainty::zero());
    let unknown: Option<Uncertainty> = None;
    assert_ne!(known_exactly, unknown);
    assert!(known_exactly.is_some());
}

#[test]
fn single_observation_derives_none() {
    // One reading is not a perfectly known value; it is a value with unknown
    // uncertainty, and the two must never be conflated.
    let one = Readings::new(vec![12.5]).unwrap();
    assert_eq!(from_readings(&one, Convention::StandardError), None);
}

#[test]
fn single_observation_derives_none_under_every_convention() {
    // Including population_stdev, whose statistic is defined and equals zero.
    // That zero is a true description of the series and a false uncertainty.
    let one = Readings::new(vec![12.5]).unwrap();
    assert_eq!(summarize(&one).population_stdev, 0.0);
    for convention in [
        Convention::StandardError,
        Convention::SampleStdev,
        Convention::PopulationStdev,
    ] {
        assert_eq!(
            from_readings(&one, convention),
            None,
            "{convention:?} derived a value from one reading"
        );
    }
}

#[test]
fn two_observations_derive_a_value() {
    let two = Readings::new(vec![12.4, 12.6]).unwrap();
    for convention in [
        Convention::StandardError,
        Convention::SampleStdev,
        Convention::PopulationStdev,
    ] {
        let derived = from_readings(&two, convention)
            .unwrap_or_else(|| panic!("{convention:?} derived nothing from two readings"));
        assert!(derived.magnitude() > 0.0);
    }
}

#[test]
fn each_convention_matches_its_summary_field() {
    // Three conventions, three statistics, and no second implementation of any
    // of them.
    let readings = reference();
    let summary = summarize(&readings);
    let pairs = [
        (Convention::StandardError, summary.standard_error.unwrap()),
        (Convention::SampleStdev, summary.sample_stdev.unwrap()),
        (Convention::PopulationStdev, summary.population_stdev),
    ];
    for (convention, expected) in pairs {
        let derived = from_readings(&readings, convention).unwrap();
        assert_eq!(derived.magnitude(), expected, "{convention:?}");
    }
}

#[test]
fn derivation_matches_reference_series() {
    let derived = from_readings(&reference(), Convention::StandardError).unwrap();
    assert!(
        close(derived.magnitude(), 0.755_928_946_018_454_4),
        "{}",
        derived.magnitude()
    );
}

#[test]
fn standard_error_is_not_sample_stdev() {
    // They differ by sqrt(n) — a factor of 2.83 on these eight readings — and
    // they answer different questions.
    let readings = reference();
    let standard_error = from_readings(&readings, Convention::StandardError).unwrap();
    let sample_stdev = from_readings(&readings, Convention::SampleStdev).unwrap();
    assert_ne!(standard_error, sample_stdev);
    let root_n = (readings.len().get() as f64).sqrt();
    assert!(
        close(
            sample_stdev.magnitude() / root_n,
            standard_error.magnitude()
        ),
        "the standard error is {} and the sample standard deviation is {}; \
         they should differ by sqrt(n) = {root_n}",
        standard_error.magnitude(),
        sample_stdev.magnitude()
    );
}

#[test]
fn relative_to_a_reference_is_a_fraction() {
    let uncertainty = Uncertainty::new(0.05).unwrap();
    assert!(close(uncertainty.relative_to(12.5).unwrap(), 0.004));
    // The reference's magnitude is what counts: a relative uncertainty of
    // -0.4% is not a thing.
    assert!(close(uncertainty.relative_to(-12.5).unwrap(), 0.004));
}

#[test]
fn relative_to_zero_is_none() {
    // No infinity is produced from a zero reference.
    let uncertainty = Uncertainty::new(0.05).unwrap();
    assert_eq!(uncertainty.relative_to(0.0), None);
    assert_eq!(uncertainty.relative_to(-0.0), None);
}
