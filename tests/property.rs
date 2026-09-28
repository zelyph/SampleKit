//! The tests of `property`.

use std::cell::{Cell, RefCell};
use std::error::Error;
use std::fmt;
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use samplekit::core::formatting::{Precision, Presentation};
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{
    Compute, ComputeError, ComputeQuantity, Fingerprint, InputName, InputRecord, Property, Records,
};
use samplekit::core::statistics::Location;
use samplekit::core::uncertainty::{Convention, Uncertainty};
use samplekit::core::value::{Readings, Value};

// ------------------------------------------------------------------ fakes

/// A formula that counts its calls, so laziness is observable.
struct Counting {
    calls: Cell<usize>,
    result: f64,
}

impl Counting {
    fn new(result: f64) -> Rc<Counting> {
        Rc::new(Counting {
            calls: Cell::new(0),
            result,
        })
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl Compute for Counting {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        Ok(Value::number(self.result).unwrap())
    }
}

/// A formula that returns whatever it is given, to exercise the conversion a
/// computed uncertainty goes through.
struct Returning(Value);

impl Compute for Returning {
    fn compute(&self) -> Result<Value, ComputeError> {
        Ok(self.0.clone())
    }
}

#[derive(Debug)]
struct Boom;

impl fmt::Display for Boom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the model raised")
    }
}

impl Error for Boom {}

struct Raising {
    calls: Cell<usize>,
}

impl Raising {
    fn new() -> Rc<Raising> {
        Rc::new(Raising {
            calls: Cell::new(0),
        })
    }
}

impl Compute for Raising {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        Err(ComputeError::failed(Boom))
    }
}

/// One formula, both numbers, and a result that moves every run — so a value
/// from one run beside an uncertainty from another would be visible.
struct Drifting {
    calls: Cell<usize>,
}

impl Drifting {
    fn new() -> Rc<Drifting> {
        Rc::new(Drifting {
            calls: Cell::new(0),
        })
    }

    fn calls(&self) -> usize {
        self.calls.get()
    }
}

impl ComputeQuantity for Drifting {
    fn compute(&self) -> Result<(Value, Option<Uncertainty>), ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let value = 10.0 + self.calls.get() as f64;
        Ok((
            Value::number(value).unwrap(),
            Some(Uncertainty::new(value / 100.0).unwrap()),
        ))
    }
}

/// A formula that reads the very property it computes.
struct SelfReferential {
    property: RefCell<Weak<RefCell<Property>>>,
    blame: Option<Identifier>,
}

impl SelfReferential {
    fn build(blame: Option<&str>) -> (Rc<SelfReferential>, Rc<RefCell<Property>>) {
        let formula = Rc::new(SelfReferential {
            property: RefCell::new(Weak::new()),
            blame: blame.map(|name| Identifier::new(name).unwrap()),
        });
        let property = Rc::new(RefCell::new(Property::computed(formula.clone())));
        *formula.property.borrow_mut() = Rc::downgrade(&property);
        (formula, property)
    }
}

impl Compute for SelfReferential {
    fn compute(&self) -> Result<Value, ComputeError> {
        let property = self.property.borrow().upgrade().expect("still alive");
        // Read it again: this is the re-entry, and it must not recurse.
        let outcome = property.borrow().value();
        match (outcome, &self.blame) {
            // The frame that knows a name completes the path on the way out.
            (Err(error), Some(name)) => Err(error.within(name)),
            (other, _) => other,
        }
    }
}

fn readings(values: &[f64]) -> Readings {
    Readings::new(values.to_vec()).unwrap()
}

fn reference() -> Readings {
    readings(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0])
}

fn number(property: &Property) -> f64 {
    match property.value().unwrap() {
        Value::Number(number) => number,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-12 * left.abs().max(right.abs()).max(1.0)
}

fn a_record() -> Records {
    let mut computed = IndexMap::new();
    computed.insert(
        InputName::Named(Identifier::new("malt").unwrap()),
        InputRecord::Digest(Fingerprint::new("a91f42c8f0d1")),
    );
    Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::new("5b2c9e1f0a34")),
        computed: Some(computed),
    }
}

// ------------------------------------------------------------- laziness

#[test]
fn constructing_computed_does_not_run_the_formula() {
    let formula = Counting::new(12.5);
    let _property = Property::computed(formula.clone());
    assert_eq!(formula.calls(), 0);
}

#[test]
fn first_read_runs_the_formula_once() {
    let formula = Counting::new(12.5);
    let property = Property::computed(formula.clone());
    assert_eq!(number(&property), 12.5);
    assert_eq!(formula.calls(), 1);
    assert_eq!(number(&property), 12.5);
    assert_eq!(formula.calls(), 1, "a second read ran it again");
}

#[test]
fn invalidate_forces_recomputation() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    let _ = property.value();
    let _ = property.value();
    assert_eq!(formula.calls(), 1);
    property.invalidate();
    let _ = property.value();
    assert_eq!(formula.calls(), 2);
}

// -------------------------------------------------------------- measured

#[test]
fn readings_with_no_statistic_have_no_value() {
    // No mean is taken on its own. Which statistic stands for readings is the
    // model's to say, and it said nothing here.
    let property = Property::measured(readings(&[1.0, 2.0, 3.0]), None);
    assert_eq!(property.value().unwrap(), Value::Absent);
    assert_eq!(property.peek_value(), Some(Value::Absent));
    assert_eq!(property.readings().unwrap().as_slice(), &[1.0, 2.0, 3.0]);
}

#[test]
fn a_declared_statistic_is_the_value_of_readings() {
    let mut property = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    property.declare_statistics(Some(Location::Mean), None);
    assert_eq!(number(&property), 4.0);
    property.declare_statistics(Some(Location::Median), None);
    assert_eq!(number(&property), 2.0);
}

#[test]
fn measured_uncertainty_is_the_standard_error() {
    let property = Property::measured(reference(), Some(Convention::StandardError));
    let derived = property.uncertainty().unwrap().unwrap();
    assert!(
        close(derived.magnitude(), 0.755_928_946_018_454_4),
        "{}",
        derived.magnitude()
    );
}

#[test]
fn measured_uncertainty_follows_its_convention() {
    let series = reference();
    let error = Property::measured(series.clone(), Some(Convention::StandardError));
    let stdev = Property::measured(series, Some(Convention::SampleStdev));
    let error = error.uncertainty().unwrap().unwrap().magnitude();
    let stdev = stdev.uncertainty().unwrap().unwrap().magnitude();
    assert!(close(stdev, 2.138_089_935_299_395), "{stdev}");
    assert!(close(stdev / 8f64.sqrt(), error), "{error} against {stdev}");
}

#[test]
fn measured_uncertainty_follows_changed_observations() {
    // Proving it is derived on read and not stored.
    let mut property = Property::measured(reference(), Some(Convention::StandardError));
    let before = property.uncertainty().unwrap().unwrap().magnitude();
    property.set_readings(readings(&[5.0, 5.0, 5.1]), Some(Convention::StandardError));
    let after = property.uncertainty().unwrap().unwrap().magnitude();
    assert_ne!(before, after);
    assert!(after < before);
}

#[test]
fn observations_without_a_convention_have_no_uncertainty() {
    // Absent, never zero and never a default.
    let property = Property::measured(reference(), None);
    assert_eq!(property.uncertainty().unwrap(), None);
}

#[test]
fn explicit_uncertainty_overrides_derived() {
    let mut property = Property::measured(reference(), Some(Convention::StandardError));
    property.set_uncertainty(Some(Uncertainty::new(0.5).unwrap()));
    assert_eq!(property.uncertainty().unwrap().unwrap().magnitude(), 0.5);
    // And the value still comes from the readings, through the statistic the
    // model declares for it.
    property.declare_statistics(Some(Location::Mean), None);
    assert_eq!(number(&property), 5.0);
}

#[test]
fn a_computed_uncertainty_that_is_not_a_magnitude_fails() {
    let mut property = Property::stored(Value::number(12.5).unwrap());

    property.set_uncertainty_formula(Rc::new(Returning(Value::number(-0.5).unwrap())));
    let error = property.uncertainty().unwrap_err();
    assert!(error.to_string().contains("0.5"), "{error}");

    property.set_uncertainty_formula(Rc::new(Returning(Value::text("wide"))));
    let error = property.uncertainty().unwrap_err();
    assert!(error.to_string().contains("text"), "{error}");

    // Absent is the formula declining to give one, which is not a failure.
    property.set_uncertainty_formula(Rc::new(Returning(Value::absent())));
    assert_eq!(property.uncertainty().unwrap(), None);

    property.set_uncertainty_formula(Rc::new(Returning(Value::number(0.05).unwrap())));
    assert_eq!(property.uncertainty().unwrap().unwrap().magnitude(), 0.05);
}

// ------------------------------------------------------------ assignment

#[test]
fn assigning_observations_restores_derived_uncertainty() {
    let mut property = Property::stored(Value::number(99.0).unwrap());
    assert_eq!(property.uncertainty().unwrap(), None);
    property.set_readings(reference(), Some(Convention::SampleStdev));
    assert!(property.uncertainty().unwrap().is_some());
    // The value its readings give is the statistic declared for it.
    property.declare_statistics(Some(Location::Mean), None);
    assert_eq!(number(&property), 5.0);
}

#[test]
fn metadata_change_does_not_invalidate() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    let _ = property.value();
    property.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: None,
        precision: None,
    });
    let _ = property.value();
    assert_eq!(formula.calls(), 1);
}

#[test]
fn metadata_change_does_not_alter_the_value() {
    let mut property = Property::stored(Value::number(12.498_700_000_000_001).unwrap());
    let before = number(&property).to_bits();
    property.set_presentation(Presentation {
        unit: None,
        symbol: None,
        precision: Some(Precision::both(".2f").unwrap()),
    });
    assert_eq!(number(&property).to_bits(), before);
}

// --------------------------------------------------------------- failure

#[test]
fn failed_computation_is_cached() {
    let formula = Raising::new();
    let property = Property::computed(formula.clone());
    assert!(property.value().is_err());
    assert!(property.value().is_err());
    assert_eq!(formula.calls.get(), 1, "a raising formula ran twice");
}

#[test]
fn an_interruption_is_not_cached_as_a_failure() {
    // A formula stopped from outside did not fail. Nothing is cached, so the
    // next read runs it again, and the property never reports a failure it did
    // not have.
    struct Stopped(Cell<usize>);
    impl Compute for Stopped {
        fn compute(&self) -> Result<Value, ComputeError> {
            self.0.set(self.0.get() + 1);
            if self.0.get() == 1 {
                return Err(ComputeError::interrupted(Boom));
            }
            Ok(Value::number(2.0).unwrap())
        }
    }
    let formula = Rc::new(Stopped(Cell::new(0)));
    let property = Property::computed(formula.clone());
    assert!(matches!(
        property.value(),
        Err(ComputeError::Interrupted { .. })
    ));
    assert!(!property.has_failed());
    assert_eq!(property.peek_failure(), None);
    assert_eq!(property.value().unwrap(), Value::number(2.0).unwrap());
    assert_eq!(formula.0.get(), 2);
}

#[test]
fn failed_computation_returns_the_same_error_twice() {
    // Reading is idempotent in the failure case too: the same error, not a
    // second attempt with a different outcome.
    let property = Property::computed(Raising::new());
    let first = property.value().unwrap_err();
    let second = property.value().unwrap_err();
    assert_eq!(first, second);
    assert_eq!(first.to_string(), "the model raised");
}

// ----------------------------------------------------------------- cycle

#[test]
fn self_referential_formula_yields_cycle_not_overflow() {
    let (_formula, property) = SelfReferential::build(None);
    let error = property.borrow().value().unwrap_err();
    assert_eq!(error, ComputeError::Cycle { path: Vec::new() });
}

#[test]
fn a_cycle_names_the_path_it_travelled() {
    // Empty where the cycle is found, complete where it surfaces.
    let (_formula, property) = SelfReferential::build(Some("plato"));
    let error = property.borrow().value().unwrap_err();
    let plato = Identifier::new("plato").unwrap();
    assert_eq!(
        error,
        ComputeError::Cycle {
            path: vec![plato.clone()]
        }
    );
    // Each further frame that knows a name prepends its own.
    let haze = Identifier::new("haze").unwrap();
    assert_eq!(
        error.within(&haze),
        ComputeError::Cycle {
            path: vec![haze, plato]
        }
    );
}

// ---------------------------------------------------------- invalidation

#[test]
fn invalidate_clears_the_uncertainty_cache_too() {
    let value = Counting::new(12.5);
    let uncertainty = Counting::new(0.05);
    let mut property = Property::computed(value.clone());
    property.set_uncertainty_formula(uncertainty.clone());
    let _ = property.value();
    let _ = property.uncertainty();
    assert_eq!((value.calls(), uncertainty.calls()), (1, 1));
    property.invalidate();
    let _ = property.value();
    let _ = property.uncertainty();
    assert_eq!((value.calls(), uncertainty.calls()), (2, 2));
}

#[test]
fn targeted_invalidation_leaves_the_other_cache() {
    // Re-running one formula does not cost the other one a run.
    let value = Counting::new(12.5);
    let uncertainty = Counting::new(0.05);
    let mut property = Property::computed(value.clone());
    property.set_uncertainty_formula(uncertainty.clone());
    let _ = property.value();
    let _ = property.uncertainty();

    property.invalidate_value();
    let _ = property.value();
    let _ = property.uncertainty();
    assert_eq!((value.calls(), uncertainty.calls()), (2, 1));

    property.invalidate_uncertainty();
    let _ = property.value();
    let _ = property.uncertainty();
    assert_eq!((value.calls(), uncertainty.calls()), (2, 2));
}

// ----------------------------------------------------------------- joint

#[test]
fn a_joint_formula_runs_once_for_both_channels() {
    let formula = Drifting::new();
    let property = Property::joint(formula.clone());
    let _ = property.value();
    let _ = property.uncertainty();
    assert_eq!(formula.calls(), 1);
}

#[test]
fn a_joint_uncertainty_follows_its_value() {
    // Never one from each run: one cache, one pair, filled together.
    let formula = Drifting::new();
    let mut property = Property::joint(formula.clone());
    for _ in 0..3 {
        property.invalidate();
        let value = number(&property);
        let uncertainty = property.uncertainty().unwrap().unwrap().magnitude();
        assert!(
            close(uncertainty, value / 100.0),
            "{value} and {uncertainty}"
        );
    }
    assert_eq!(formula.calls(), 3);
}

#[test]
fn an_explicit_uncertainty_overrides_a_joint_one() {
    let formula = Drifting::new();
    let mut property = Property::joint(formula.clone());
    property.set_uncertainty(Some(Uncertainty::new(0.5).unwrap()));
    assert_eq!(property.uncertainty().unwrap().unwrap().magnitude(), 0.5);
    // The value formula survives: only the overridden half stops being read.
    assert_eq!(number(&property), 11.0);
    assert!(property.is_computed());
}

#[test]
fn materializing_a_joint_property_keeps_both_numbers() {
    let formula = Drifting::new();
    let mut property = Property::joint(formula.clone());
    property.materialize().unwrap();
    assert!(!property.is_computed());
    assert_eq!(number(&property), 11.0);
    // Explicit now, rather than a marker pointing at a formula that is gone.
    assert_eq!(property.uncertainty().unwrap().unwrap().magnitude(), 0.11);
    let _ = property.uncertainty();
    assert_eq!(formula.calls(), 1);
}

// --------------------------------------------------------- materialization

#[test]
fn materialize_replaces_the_formula_with_its_value() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    property.materialize().unwrap();
    assert!(!property.is_computed());
    assert_eq!(number(&property), 12.5);
    let _ = property.value();
    assert_eq!(formula.calls(), 1);
}

#[test]
fn materialize_leaves_the_property_unchanged_on_failure() {
    let mut property = Property::computed(Raising::new());
    assert!(property.materialize().is_err());
    assert!(property.is_computed(), "a failing materialize half-applied");
}

#[test]
fn materialize_runs_an_independent_uncertainty_formula() {
    // Nothing else ever runs it: the value beside it is not computed, so a
    // check that read only the source would call this resolved and let a save
    // write the property with no uncertainty at all.
    let formula = Counting::new(0.05);
    let mut property = Property::stored(Value::number(12.5).unwrap());
    property.set_uncertainty_formula(formula.clone());
    assert!(!property.is_resolved(), "a formula that has never run");

    property.materialize().unwrap();
    assert!(property.is_resolved());
    assert_eq!(
        property.uncertainty().unwrap().map(|u| u.magnitude()),
        Some(0.05)
    );
    // Materialized, so reading it again runs nothing.
    let _ = property.uncertainty();
    assert_eq!(formula.calls(), 1);
}

#[test]
fn materialize_keeps_the_records() {
    // Losing the formula does not lose what it depended on.
    let mut property = Property::computed(Counting::new(12.5));
    property.set_records(a_record());
    property.materialize().unwrap();
    assert_eq!(property.records(), &a_record());
}

// --------------------------------------------------------------- records

#[test]
fn setting_presentation_keeps_the_records() {
    // A unit correction is not a re-derivation.
    let mut property = Property::computed(Counting::new(12.5));
    property.set_records(a_record());
    property.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: None,
        precision: None,
    });
    assert_eq!(property.records(), &a_record());
    property.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    assert_eq!(property.records(), &a_record());
}

// ----------------------------------------------------------------- filling

fn file_value(value: f64) -> Property {
    let mut file = Property::stored(Value::number(value).unwrap());
    file.set_records(a_record());
    file
}

#[test]
fn filling_keeps_the_formula_and_the_file_value() {
    let formula = Counting::new(9.0);
    let mut declared = Property::computed(formula.clone());
    declared.fill_from(file_value(2.61)).unwrap();
    assert!(declared.is_computed());
    assert!(declared.is_resolved());
    assert_eq!(declared.value().unwrap(), Value::number(2.61).unwrap());
    assert_eq!(formula.calls(), 0);
    assert_eq!(declared.records(), &a_record());
}

#[test]
fn a_filled_value_is_recomputed_after_invalidation() {
    let formula = Counting::new(9.0);
    let mut declared = Property::computed(formula.clone());
    declared.fill_from(file_value(2.61)).unwrap();
    declared.invalidate();
    assert_eq!(declared.value().unwrap(), Value::number(9.0).unwrap());
    assert_eq!(formula.calls(), 1);
    assert!(declared.records().is_empty());
}

#[test]
fn filling_without_a_file_value_leaves_the_cache_empty() {
    let formula = Counting::new(9.0);
    let mut declared = Property::computed(formula.clone());
    declared
        .fill_from(Property::stored(Value::absent()))
        .unwrap();
    assert!(!declared.is_resolved());
    assert_eq!(declared.value().unwrap(), Value::number(9.0).unwrap());
    assert_eq!(formula.calls(), 1);
}

/// A joint formula that counts its runs.
struct FillPair {
    calls: Cell<usize>,
}

impl ComputeQuantity for FillPair {
    fn compute(&self) -> Result<(Value, Option<Uncertainty>), ComputeError> {
        self.calls.set(self.calls.get() + 1);
        Ok((
            Value::number(9.0).unwrap(),
            Some(Uncertainty::new(0.5).unwrap()),
        ))
    }
}

#[test]
fn filling_a_joint_formula_holds_both_channels() {
    let formula = Rc::new(FillPair {
        calls: Cell::new(0),
    });
    let mut declared = Property::joint(formula.clone());
    let mut file = Property::stored(Value::number(2.61).unwrap());
    file.set_uncertainty(Some(Uncertainty::new(0.02).unwrap()));
    declared.fill_from(file).unwrap();
    assert_eq!(declared.value().unwrap(), Value::number(2.61).unwrap());
    assert_eq!(
        declared.uncertainty().unwrap(),
        Some(Uncertainty::new(0.02).unwrap())
    );
    assert_eq!(formula.calls.get(), 0);
}

#[test]
fn filling_an_uncertainty_formula_holds_the_file_uncertainty() {
    let formula = Counting::new(0.5);
    let mut declared = Property::stored(Value::absent());
    declared.set_uncertainty_formula(formula.clone());
    let mut file = Property::stored(Value::number(3.0).unwrap());
    file.set_uncertainty(Some(Uncertainty::new(0.1).unwrap()));
    declared.fill_from(file).unwrap();
    assert_eq!(declared.value().unwrap(), Value::number(3.0).unwrap());
    assert_eq!(
        declared.uncertainty().unwrap(),
        Some(Uncertainty::new(0.1).unwrap())
    );
    assert!(declared.is_resolved());
    assert_eq!(formula.calls(), 0);
}

#[test]
fn file_presentation_wins_and_the_declaration_fills_gaps() {
    let mut declared = Property::stored(Value::absent());
    declared.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        precision: None,
    });
    let mut file = Property::stored(Value::number(3.0).unwrap());
    file.set_presentation(Presentation {
        unit: Some("kg".to_string()),
        ..Presentation::default()
    });
    declared.fill_from(file).unwrap();
    let presentation = declared.presentation();
    assert_eq!(presentation.unit.as_deref(), Some("kg"));
    assert_eq!(presentation.symbol.as_deref(), Some("m"));
    // Neither holds a precision: it is the project's.
    assert_eq!(presentation.precision, None);
}

#[test]
fn readings_under_a_formula_are_refused() {
    let formula = Counting::new(9.0);
    let mut declared = Property::computed(formula.clone());
    let refused = declared.fill_from(Property::measured(readings(&[1.0, 2.0]), None));
    assert_eq!(
        refused,
        Err(samplekit::core::property::FillConflict::ReadingsUnderFormula)
    );
    assert!(declared.is_computed());
    assert!(declared.readings().is_none());
    assert!(!declared.is_resolved());
}

#[test]
fn a_declared_convention_applies_to_file_readings() {
    let mut declared = Property::measured(readings(&[0.0]), Some(Convention::SampleStdev));
    declared
        .fill_from(Property::measured(readings(&[1.0, 2.0, 4.0]), None))
        .unwrap();
    let expected = samplekit::core::uncertainty::from_readings(
        &readings(&[1.0, 2.0, 4.0]),
        Convention::SampleStdev,
    );
    assert!(expected.is_some());
    assert_eq!(declared.uncertainty().unwrap(), expected);
}

#[test]
fn peeking_runs_no_formula() {
    let formula = Counting::new(9.0);
    let mut property = Property::computed(formula.clone());
    property.set_uncertainty_formula(Counting::new(0.5));
    assert_eq!(property.peek_value(), None);
    assert_eq!(property.peek_uncertainty(), None);
    assert_eq!(formula.calls(), 0);
    property.value().unwrap();
    property.uncertainty().unwrap();
    assert_eq!(property.peek_value(), Some(Value::number(9.0).unwrap()));
    assert_eq!(
        property.peek_uncertainty(),
        Some(Some(Uncertainty::new(0.5).unwrap()))
    );
    assert_eq!(formula.calls(), 1);
    assert_eq!(
        Property::stored(Value::absent()).peek_uncertainty(),
        Some(None)
    );
}

// ------------------------------------------------------------- overrides

#[test]
fn assigning_a_computed_value_overrides_it() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    let _ = property.value();
    property.set_value(Value::number(99.0).unwrap());
    assert_eq!(number(&property), 99.0);
    assert!(property.is_computed());
    assert!(property.is_edited());
    assert_eq!(property.peek_value(), Some(Value::number(99.0).unwrap()));
    assert_eq!(formula.calls(), 1, "the override is read, not the formula");
}

#[test]
fn assigning_a_joint_value_overrides_the_pair() {
    let formula = Drifting::new();
    let mut property = Property::joint(formula.clone());
    let _ = property.value();
    property.set_value(Value::number(99.0).unwrap());
    assert_eq!(number(&property), 99.0);
    assert!(property.is_computed() && property.is_edited());
    assert_eq!(property.uncertainty().unwrap(), None);
    property.set_uncertainty(Some(Uncertainty::new(0.5).unwrap()));
    assert_eq!(property.uncertainty().unwrap().unwrap().magnitude(), 0.5);
    assert_eq!(formula.calls(), 1);
}

#[test]
fn an_override_survives_invalidation() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    property.set_value(Value::number(99.0).unwrap());
    property.invalidate();
    property.invalidate_value();
    assert_eq!(number(&property), 99.0);
    assert!(property.is_edited());
    assert_eq!(formula.calls(), 0);
}

#[test]
fn an_override_keeps_its_records() {
    let mut property = Property::computed(Counting::new(12.5));
    property.set_records(a_record());
    property.set_value(Value::number(99.0).unwrap());
    property.invalidate();
    assert_eq!(property.records(), &a_record());
}

#[test]
fn restoring_the_formula_runs_it_again() {
    let formula = Counting::new(12.5);
    let mut property = Property::computed(formula.clone());
    property.set_records(a_record());
    property.set_value(Value::number(99.0).unwrap());
    assert!(property.restore_formula());
    assert!(!property.is_edited());
    assert!(property.records().is_empty());
    assert_eq!(number(&property), 12.5);
    assert_eq!(formula.calls(), 1);
    assert!(!property.restore_formula(), "nothing is left to restore");
    assert!(!Property::stored(Value::number(1.0).unwrap()).restore_formula());
}

#[test]
fn assigning_an_entered_value_clears_the_records() {
    // A typed-in value stops claiming a derivation.
    let mut stored = Property::stored(Value::number(1.0).unwrap());
    stored.set_records(a_record());
    stored.set_value(Value::number(99.0).unwrap());
    assert!(stored.records().is_empty());
    assert!(!stored.is_edited());

    let mut measured = Property::measured(reference(), None);
    measured.set_records(a_record());
    measured.set_readings(readings(&[1.0, 2.0]), None);
    assert!(measured.records().is_empty());
}

#[test]
fn a_fingerprint_marked_edited_fills_an_override() {
    let formula = Counting::new(12.5);
    let mut declared = Property::computed(formula.clone());
    let mut file = Property::stored(Value::number(3.2).unwrap());
    file.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::edited("7c01e2b3a9f4")),
        computed: None,
    });
    declared.fill_from(file).unwrap();
    assert!(declared.is_edited());
    declared.invalidate();
    assert_eq!(number(&declared), 3.2);
    assert_eq!(formula.calls(), 0);
    assert!(declared.records().fingerprint.as_ref().unwrap().is_edited());
}

#[test]
fn holding_a_filled_value_as_edited_makes_it_an_override() {
    let formula = Counting::new(12.5);
    let mut declared = Property::computed(formula.clone());
    declared
        .fill_from(Property::stored(Value::number(3.2).unwrap()))
        .unwrap();
    assert!(!declared.is_edited());
    declared.hold_as_edited();
    assert!(declared.is_edited());
    declared.invalidate();
    assert_eq!(number(&declared), 3.2);
    assert_eq!(formula.calls(), 0);

    let mut never = Property::computed(Counting::new(1.0));
    never.hold_as_edited();
    assert!(!never.is_edited());
}

#[test]
fn a_marked_digest_compares_by_its_digest() {
    let plain = Fingerprint::new("3f2a1b09c4d1");
    let marked = Fingerprint::edited("3f2a1b09c4d1");
    assert!(plain.same_digest(&marked));
    assert_ne!(plain, marked);
    assert_eq!(plain.clone().marked_edited(), marked);
    assert_eq!(marked.to_string(), "3f2a1b09c4d1");
    assert!(!plain.same_digest(&Fingerprint::new("000000000000")));
}

#[test]
fn an_uncertainty_formula_is_reported_as_one() {
    let mut property = Property::stored(Value::number(2.0).unwrap());
    assert!(!property.has_uncertainty_formula());
    property.set_uncertainty_formula(Rc::new(Returning(Value::number(0.01).unwrap())));
    assert!(property.has_uncertainty_formula());
    assert!(!property.is_computed());
    property.set_uncertainty(Some(Uncertainty::new(0.02).unwrap()));
    assert!(!property.has_uncertainty_formula());
}

struct Fails;

impl Compute for Fails {
    fn compute(&self) -> Result<Value, ComputeError> {
        Err(ComputeError::failed(Boom))
    }
}

#[test]
fn a_failed_formula_is_reported_as_failed() {
    let property = Property::computed(Rc::new(Fails));
    assert!(!property.has_failed());
    assert!(property.value().is_err());
    assert!(property.has_failed());
    assert!(!Property::stored(Value::number(1.0).unwrap()).has_failed());
}

#[test]
fn a_filled_failure_keeps_its_value_as_the_last_good_one() {
    let formula = Counting::new(12.5);
    let mut declared = Property::computed(formula.clone());
    let mut file = Property::stored(Value::number(8.4).unwrap());
    file.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: Some("ZeroDivisionError: division by zero".to_string()),
        fingerprint: None,
        computed: None,
    });
    declared.fill_from(file).unwrap();
    assert!(declared.has_failed());
    assert!(declared.peek_value().is_none());
    let last = declared.last_good().expect("the value the file kept");
    assert_eq!(last.value, Value::number(8.4).unwrap());
    assert!(last.records.failure.is_none());
    assert_eq!(formula.calls(), 0);
}

#[test]
fn a_written_value_outranks_the_statistic_its_readings_declare() {
    // The written value first, then the declared statistic — then none, never
    // the mean.
    let mut measured = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    assert_eq!(measured.value().unwrap(), Value::Absent);
    measured.declare_statistics(Some(Location::Median), Some(Convention::SampleStdev));
    assert_eq!(number(&measured), 2.0);
    assert!(measured.uncertainty().unwrap().is_some());
    measured.set_written_value(Some(Value::number(2.5).unwrap()));
    assert_eq!(
        number(&measured),
        2.5,
        "the written value outranks the statistic"
    );
    measured.set_written_value(None);
    assert_eq!(number(&measured), 2.0);
}

#[test]
fn setting_a_value_beside_readings_keeps_them() {
    // A value written beside readings is a trial, not an erasure. The readings
    // stand, and a derived uncertainty goes on following them.
    let mut measured = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    measured.declare_statistics(Some(Location::Mean), Some(Convention::SampleStdev));
    let derived = measured
        .uncertainty()
        .unwrap()
        .expect("a convention derives one");
    measured.set_value(Value::number(2.5).unwrap());
    assert_eq!(number(&measured), 2.5, "the value written is the value");
    assert!(
        measured.readings().is_some(),
        "the readings are the evidence and stay"
    );
    assert_eq!(
        measured.readings().unwrap().as_slice(),
        &[1.0, 2.0, 9.0],
        "every one of them"
    );
    assert_eq!(
        measured.uncertainty().unwrap(),
        Some(derived),
        "the uncertainty goes on following the readings"
    );
}

#[test]
fn new_readings_leave_a_written_value_standing() {
    // The mirror: evidence replaces no writing either. What moved is the
    // statistic the written value outranks, never the value itself.
    let mut measured = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    measured.set_written_value(Some(Value::number(2.5).unwrap()));
    measured.set_readings(readings(&[3.0, 7.0]), None);
    assert_eq!(
        number(&measured),
        2.5,
        "the written value still stands over the new readings"
    );
    assert_eq!(measured.readings().unwrap().as_slice(), &[3.0, 7.0]);
}

#[test]
fn restoring_a_measured_property_drops_the_written_value() {
    // --force gives the property back to its declared statistic, which is
    // The override's round trip for a formula, now available for a named one.
    let mut measured = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    measured.declare_statistics(Some(Location::Median), None);
    measured.set_value(Value::number(2.5).unwrap());
    assert_eq!(number(&measured), 2.5);
    assert!(measured.restore_formula(), "there was something to restore");
    assert_eq!(
        number(&measured),
        2.0,
        "the declared statistic stands again"
    );
    assert!(
        !measured.restore_formula(),
        "nothing to restore answers false"
    );
}

#[test]
fn a_written_value_its_record_vouches_for_is_no_override() {
    // Every file writes a value beside its readings, so *written* is not
    // *overridden*. Once the loader has confirmed the record vouches for it, the
    // value still stands and nothing is held; a hand's write makes it an
    // override again, and restoring drops both.
    let mut measured = Property::measured(readings(&[1.0, 2.0, 9.0]), None);
    measured.declare_statistics(Some(Location::Median), None);
    measured.set_written_value(Some(Value::number(4.0).unwrap()));
    assert!(measured.holds_written_override(), "unjudged, it is held");
    measured.confirm_written_as_recorded();
    assert_eq!(number(&measured), 4.0, "the recorded value is still served");
    assert!(!measured.holds_written_override());
    measured.set_value(Value::number(2.5).unwrap());
    assert!(measured.holds_written_override(), "a hand's write is one");
    assert!(measured.restore_formula());
    assert!(!measured.holds_written_override());
    assert_eq!(number(&measured), 2.0);
}

#[test]
fn holding_an_uncertainty_a_formula_gives_makes_it_an_override() {
    // A property whose only formula is its uncertainty has no value cache to
    // hold, so the hold, the report and the release all apply to the
    // uncertainty's.
    let formula = Counting::new(0.01);
    let mut declared = Property::stored(Value::number(2.0).unwrap());
    declared.set_uncertainty_formula(formula.clone());
    let mut file = Property::stored(Value::number(2.0).unwrap());
    file.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    declared.fill_from(file).unwrap();
    assert!(!declared.is_edited());
    declared.hold_as_edited();
    assert!(declared.is_edited(), "the held uncertainty is an override");
    assert_eq!(
        declared.peek_uncertainty(),
        Some(Some(Uncertainty::new(0.05).unwrap()))
    );
    declared.invalidate();
    assert_eq!(
        declared.uncertainty().unwrap(),
        Some(Uncertainty::new(0.05).unwrap()),
        "an override survives invalidation"
    );
    assert_eq!(formula.calls(), 0);
    assert!(declared.restore_formula());
    assert!(!declared.is_edited());
    assert_eq!(
        declared.uncertainty().unwrap(),
        Some(Uncertainty::new(0.01).unwrap())
    );
    assert_eq!(formula.calls(), 1);

    // A fingerprint marked edited fills the uncertainty held.
    let mut marked = Property::stored(Value::number(2.0).unwrap());
    marked.set_uncertainty_formula(Counting::new(0.01));
    let mut from_file = Property::stored(Value::number(2.0).unwrap());
    from_file.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    from_file.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::edited("3f2a1b09c4d1")),
        computed: Some(IndexMap::new()),
    });
    marked.fill_from(from_file).unwrap();
    assert!(marked.is_edited());

    // Beside a computed value, the value is what is held, as before.
    let mut both = Property::computed(Counting::new(3.0));
    both.set_uncertainty_formula(Counting::new(0.01));
    let mut supplied = Property::stored(Value::number(3.0).unwrap());
    supplied.set_uncertainty(Some(Uncertainty::new(0.05).unwrap()));
    both.fill_from(supplied).unwrap();
    both.hold_as_edited();
    assert!(both.is_edited());
    assert!(both.restore_formula());
    assert!(!both.is_edited());
}

#[test]
fn a_value_not_applicable_beside_readings_has_no_uncertainty() {
    // `fg=n/a` over readings whose standard error gave the uncertainty went on
    // giving one: a value that does not apply has no spread.
    let mut property = Property::measured(
        readings(&[1.011, 1.009, 1.010]),
        Some(Convention::StandardError),
    );
    assert!(property.uncertainty().unwrap().is_some());
    property.set_value(Value::NotApplicable);
    assert_eq!(property.uncertainty().unwrap(), None);
    assert_eq!(property.peek_uncertainty(), Some(None));
    assert!(property.readings().is_some(), "the readings stay");
}
