//! The tests of `fingerprint`.

use std::cell::Cell;
use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::dependency_graph::Node;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{
    Compute, ComputeError, Fingerprint, InputName, InputRecord, Property, Records,
};
use samplekit::core::sample::{AttributeValue, Sample};
use samplekit::core::table::{
    CellOutput, ColumnMeta, ComputeRow, Derivation, RowView, Scope, Table,
};
use samplekit::core::value::Value;
use samplekit::format::canonicalization::value_form;
use samplekit::format::fingerprint::{self, Freshness};
use samplekit::format::schema::{self, PropertySchema};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn shape(value: Option<Value>) -> PropertySchema {
    PropertySchema {
        statistics: None,
        value,
        ..PropertySchema::default()
    }
}

/// A formula, so a sample can have real derivations.
struct Constant {
    value: f64,
    calls: Cell<usize>,
}

impl Constant {
    fn new(value: f64) -> Rc<Constant> {
        Rc::new(Constant {
            value,
            calls: Cell::new(0),
        })
    }
}

impl Compute for Constant {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        Ok(number(self.value))
    }
}

/// malt and volume entered, plato computed from both.
fn derived_sample() -> Sample {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("volume"), Property::stored(number(4.1)))
        .unwrap();
    sample
        .set_property(id("plato"), Property::computed(Constant::new(3.0488)))
        .unwrap();
    sample
        .declare_dependencies(
            &id("plato"),
            &[Node::Named(id("malt")), Node::Named(id("volume"))],
        )
        .unwrap();
    sample
}

/// Resolve, record, materialize — the order `save_computed` follows, and the
/// order a check depends on: `check` reads what is **stored**, so a computed
/// property whose cache has been invalidated has nothing for it to read.
fn stamp(sample: &mut Sample) {
    for name in sample
        .property_names()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
    {
        let _ = sample.property(&name).unwrap().value();
    }
    let records = fingerprint::record(sample).unwrap();
    sample.materialize().unwrap();
    for (name, record) in records {
        sample.property(&name).unwrap().set_records(record);
    }
}

fn freshness(sample: &Sample, name: &str) -> Freshness {
    fingerprint::check_property(sample, &id(name)).unwrap()
}

// ------------------------------------------------------------- the digest

#[test]
fn matches_sha256sum_truncated() {
    // A known value against what a shell produces, so the recipe is checkable
    // and not merely stated: `printf %s '{v: 12.5}' | sha256sum | cut -c1-12`.
    // The pre-image spells its channels the way the file does, so these two
    // digests moved when that spelling did — which is the whole of what
    // changed, and why a collection is recomputed once after it.
    assert_eq!(
        fingerprint::of(&shape(Some(number(12.5)))).as_str(),
        "87c43f2ca298"
    );
    let with_uncertainty = PropertySchema {
        statistics: None,
        value: Some(number(12.5)),
        uncertainty: Some(0.05),
        ..PropertySchema::default()
    };
    assert_eq!(value_form(&with_uncertainty), "{v: 12.5, u: 0.05}");
    assert_eq!(fingerprint::of(&with_uncertainty).as_str(), "55f170b69eca");
    assert_eq!(
        fingerprint::of(&shape(Some(number(12.5)))).as_str().len(),
        12
    );
}

#[test]
fn is_stable_across_runs() {
    // Same input, same hex, twice.
    let one = fingerprint::of(&shape(Some(number(12.5))));
    let other = fingerprint::of(&shape(Some(number(12.5))));
    assert_eq!(one, other);
}

#[test]
fn value_change_changes_the_fingerprint() {
    assert_ne!(
        fingerprint::of(&shape(Some(number(12.5)))),
        fingerprint::of(&shape(Some(number(12.6))))
    );
}

#[test]
fn uncertainty_change_changes_the_fingerprint() {
    // Uncertainty is part of the quantity.
    let a = PropertySchema {
        statistics: None,
        uncertainty: Some(0.05),
        ..shape(Some(number(12.5)))
    };
    let b = PropertySchema {
        statistics: None,
        uncertainty: Some(0.06),
        ..shape(Some(number(12.5)))
    };
    assert_ne!(fingerprint::of(&a), fingerprint::of(&b));
}

#[test]
fn observation_change_changes_the_fingerprint() {
    // `readings` is covered, not only the mean.
    let a = PropertySchema {
        statistics: None,
        readings: Some(vec![2.011, 2.017]),
        ..shape(Some(number(2.014)))
    };
    let b = PropertySchema {
        statistics: None,
        readings: Some(vec![2.012, 2.016]),
        ..shape(Some(number(2.014)))
    };
    assert_ne!(fingerprint::of(&a), fingerprint::of(&b));
}

#[test]
fn unit_change_leaves_the_fingerprint() {
    let plain = shape(Some(number(12.5)));
    let dressed = PropertySchema {
        statistics: None,
        unit: Some("g".to_string()),
        ..plain.clone()
    };
    assert_eq!(fingerprint::of(&plain), fingerprint::of(&dressed));
}

#[test]
fn symbol_change_leaves_the_fingerprint() {
    let plain = shape(Some(number(12.5)));
    let dressed = PropertySchema {
        statistics: None,
        symbol: Some("m".to_string()),
        ..plain.clone()
    };
    assert_eq!(fingerprint::of(&plain), fingerprint::of(&dressed));
}

#[test]
fn is_independent_of_neighbouring_metadata() {
    // Adding a unit to a value-only property does not change its hash, because
    // `value_form` never uses the bare-scalar shorthand.
    let plain = shape(Some(number(12.5)));
    let dressed = PropertySchema {
        statistics: None,
        unit: Some("g".to_string()),
        symbol: Some("m".to_string()),
        fingerprint: Some(Fingerprint::new("deadbeef0000")),
        ..plain.clone()
    };
    assert_eq!(fingerprint::of(&plain), fingerprint::of(&dressed));
}

// ------------------------------------------------------------- recording

#[test]
fn a_property_outside_every_derivation_carries_nothing() {
    let mut sample = Sample::new();
    sample
        .set_property(id("label"), Property::stored(Value::text("x")))
        .unwrap();
    let records = fingerprint::record(&sample).unwrap();
    assert!(records.is_empty());
}

#[test]
fn becoming_an_input_adds_a_fingerprint() {
    // And ceasing to be one removes it.
    let mut sample = derived_sample();
    stamp(&mut sample);
    assert!(
        sample
            .property(&id("malt"))
            .unwrap()
            .records()
            .fingerprint
            .is_some()
    );
    assert!(
        sample
            .property(&id("malt"))
            .unwrap()
            .records()
            .computed
            .is_none()
    );

    let mut alone = Sample::new();
    alone
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    assert!(
        fingerprint::record(&alone)
            .unwrap()
            .get(&id("malt"))
            .is_none()
    );
}

#[test]
fn recording_an_unknown_input_fails() {
    // Rather than recording a shorter dependency list.
    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::computed(Constant::new(3.0)))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[Node::Named(id("vanished"))])
        .unwrap();
    let _ = sample.property(&id("plato")).unwrap().value();
    let error = fingerprint::record(&sample).unwrap_err();
    assert!(error.to_string().contains("vanished"), "{error}");
}

#[test]
fn recording_after_materialize_records_no_derivation() {
    // The one ordering that silently forgets which values were derived.
    let mut sample = derived_sample();
    sample.materialize().unwrap();
    let records = fingerprint::record(&sample).unwrap();
    assert!(
        records
            .get(&id("plato"))
            .and_then(|r| r.computed.as_ref())
            .is_none(),
        "the derivation survived materialize, which it cannot"
    );
}

#[test]
fn an_attribute_input_is_recorded_as_its_value() {
    // Hash what does not fit, store what does.
    let mut sample = derived_sample();
    sample
        .set_attribute(id("batch"), Value::integer(7))
        .unwrap();
    sample
        .declare_dependencies(
            &id("plato"),
            &[Node::Named(id("malt")), Node::Named(id("batch"))],
        )
        .unwrap();
    stamp(&mut sample);
    let computed = sample
        .property(&id("plato"))
        .unwrap()
        .records()
        .computed
        .unwrap();
    assert_eq!(
        computed[&InputName::Named(id("batch"))],
        InputRecord::Literal(Value::integer(7))
    );
    assert!(matches!(
        computed[&InputName::Named(id("malt"))],
        InputRecord::Digest(_)
    ));
}

#[test]
fn a_list_attribute_input_is_recorded_and_checked_by_digest() {
    let mut sample = derived_sample();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    sample
        .declare_dependencies(
            &id("plato"),
            &[Node::Named(id("malt")), Node::Named(id("temperatures"))],
        )
        .unwrap();
    stamp(&mut sample);
    let computed = sample
        .property(&id("plato"))
        .unwrap()
        .records()
        .computed
        .unwrap();
    assert!(matches!(
        computed[&InputName::Named(id("temperatures"))],
        InputRecord::Digest(_)
    ));
    assert_eq!(freshness(&sample, "plato"), Freshness::Current);

    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(40)]).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        freshness(&sample, "plato"),
        Freshness::Stale { changed, .. }
            if changed == vec![InputName::Named(id("temperatures"))]
    ));
}

// -------------------------------------------------------------- checking

#[test]
fn unchanged_inputs_report_current() {
    let mut sample = derived_sample();
    stamp(&mut sample);
    assert_eq!(freshness(&sample, "plato"), Freshness::Current);
}

#[test]
fn a_property_without_a_computed_record_is_source() {
    // An entered value is never reported as anything else.
    let mut sample = derived_sample();
    stamp(&mut sample);
    assert_eq!(freshness(&sample, "malt"), Freshness::Source);
}

#[test]
fn an_empty_computed_record_still_means_calculated() {
    // `computed: {}` is Current or Edited, never Source.
    let mut sample = Sample::new();
    let mut property = Property::stored(number(0.05));
    property.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(fingerprint::of(&shape(Some(number(0.05))))),
        computed: Some(IndexMap::new()),
    });
    sample.set_property(id("nominal"), property).unwrap();
    assert_eq!(freshness(&sample, "nominal"), Freshness::Current);
}

#[test]
fn changed_input_is_named() {
    // Stale.changed holds the input that moved, not all of them.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert_eq!(
        freshness(&sample, "plato"),
        Freshness::Stale {
            changed: vec![InputName::Named(id("malt"))],
            upstream: Vec::new()
        }
    );
}

#[test]
fn two_changed_inputs_are_both_named() {
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    sample
        .property(&id("volume"))
        .unwrap()
        .set_value(number(1.0));
    let Freshness::Stale { changed, .. } = freshness(&sample, "plato") else {
        panic!("expected stale");
    };
    assert_eq!(changed.len(), 2);
}

#[test]
fn stale_input_propagates_upstream() {
    // A derived input that is stale makes its dependent stale, in `upstream`.
    let mut sample = derived_sample();
    sample
        .set_property(id("haze"), Property::computed(Constant::new(0.2)))
        .unwrap();
    sample
        .declare_dependencies(&id("haze"), &[Node::Named(id("plato"))])
        .unwrap();
    stamp(&mut sample);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert!(matches!(
        freshness(&sample, "plato"),
        Freshness::Stale { .. }
    ));
    assert_eq!(
        freshness(&sample, "haze"),
        Freshness::Stale {
            changed: Vec::new(),
            upstream: vec![InputName::Named(id("plato"))]
        }
    );
}

#[test]
fn deep_chain_propagates() {
    // Three levels: a root change reaches the far end.
    let mut sample = derived_sample();
    for (name, from) in [("haze", "plato"), ("rating", "haze")] {
        sample
            .set_property(id(name), Property::computed(Constant::new(1.0)))
            .unwrap();
        sample
            .declare_dependencies(&id(name), &[Node::Named(id(from))])
            .unwrap();
    }
    stamp(&mut sample);
    assert_eq!(freshness(&sample, "rating"), Freshness::Current);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert!(matches!(
        freshness(&sample, "rating"),
        Freshness::Stale { .. }
    ));
}

#[test]
fn sibling_branch_is_untouched() {
    // Transitivity does not over-report.
    let mut sample = derived_sample();
    sample
        .set_property(id("colour"), Property::stored(Value::text("grey")))
        .unwrap();
    sample
        .set_property(id("shade"), Property::computed(Constant::new(1.0)))
        .unwrap();
    sample
        .declare_dependencies(&id("shade"), &[Node::Named(id("colour"))])
        .unwrap();
    stamp(&mut sample);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert_eq!(freshness(&sample, "shade"), Freshness::Current);
}

#[test]
fn missing_input_is_broken_not_stale() {
    // The two are distinguishable by a caller.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample.remove_property(&id("volume")).unwrap();
    assert_eq!(
        freshness(&sample, "plato"),
        Freshness::Broken {
            missing: vec![InputName::Named(id("volume"))],
            changed_kind: Vec::new()
        }
    );
}

#[test]
fn an_edited_value_is_reported_as_edited() {
    // Inputs unchanged, value changed: not Current.
    let mut sample = derived_sample();
    stamp(&mut sample);
    let plato = sample.property(&id("plato")).unwrap();
    let records = plato.records();
    plato.set_value(number(99.0));
    plato.set_records(records);
    assert_eq!(freshness(&sample, "plato"), Freshness::Edited);
}

#[test]
fn edited_outranks_stale() {
    // Both conditions true reports Edited.
    let mut sample = derived_sample();
    stamp(&mut sample);
    let plato = sample.property(&id("plato")).unwrap();
    let records = plato.records();
    plato.set_value(number(99.0));
    plato.set_records(records);
    sample.property(&id("malt")).unwrap().set_value(number(1.0));
    assert_eq!(freshness(&sample, "plato"), Freshness::Edited);
}

#[test]
fn a_presentation_edit_is_not_an_edit() {
    // Changing a unit leaves the property Current.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample
        .property(&id("plato"))
        .unwrap()
        .set_presentation(Presentation {
            unit: Some("g/dl".to_string()),
            symbol: None,
            precision: None,
        });
    assert_eq!(freshness(&sample, "plato"), Freshness::Current);
}

#[test]
fn a_stale_fingerprint_on_an_input_is_not_a_defect() {
    // Correcting a measurement is not reported as a fault: the input itself is
    // Source, and only its dependent is stale.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert_eq!(freshness(&sample, "malt"), Freshness::Source);
    assert!(matches!(
        freshness(&sample, "plato"),
        Freshness::Stale { .. }
    ));
}

#[test]
fn check_recomputes_and_does_not_compare_records() {
    // Two stored hashes agreeing proves nothing: the check hashes the current
    // content of the input, never the input's own recorded fingerprint.
    let mut sample = derived_sample();
    stamp(&mut sample);
    // Corrupt the input's own fingerprint without touching its value.
    let malt = sample.property(&id("malt")).unwrap();
    malt.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::new("000000000000")),
        computed: None,
    });
    assert_eq!(
        freshness(&sample, "plato"),
        Freshness::Current,
        "the check read the input's stored hash instead of its content"
    );
}

#[test]
fn cyclic_records_are_reported_with_the_path() {
    // And the walk terminates.
    let mut sample = Sample::new();
    for name in ["a", "b"] {
        sample
            .set_property(id(name), Property::stored(number(1.0)))
            .unwrap();
    }
    for (name, from) in [("a", "b"), ("b", "a")] {
        let handle = sample.property(&id(name)).unwrap();
        let mut computed = IndexMap::new();
        computed.insert(
            InputName::Named(id(from)),
            InputRecord::Digest(fingerprint::of(&shape(Some(number(1.0))))),
        );
        handle.set_records(Records {
            produced: None,
            channel_only: Default::default(),
            statistics: None,
            failure: None,
            fingerprint: Some(fingerprint::of(&shape(Some(number(1.0))))),
            computed: Some(computed),
        });
    }
    let error = fingerprint::check_property(&sample, &id("a")).unwrap_err();
    assert!(error.to_string().contains('\u{2192}'), "{error}");
    assert!(
        error.to_string().contains('a') && error.to_string().contains('b'),
        "{error}"
    );
}

#[test]
fn a_cycle_leaves_the_rest_of_the_sample_judged() {
    // `check` answers `Unjudged` with the path for the values on the cycle and
    // below it, and judges an unrelated value as in a sound file.
    let mut sample = Sample::new();
    for name in ["a", "b", "below", "malt", "plato"] {
        sample
            .set_property(id(name), Property::stored(number(1.0)))
            .unwrap();
    }
    let recorded = || InputRecord::Digest(fingerprint::of(&shape(Some(number(1.0)))));
    for (name, from, digest) in [
        ("a", "b", recorded()),
        ("b", "a", recorded()),
        ("below", "a", recorded()),
        // What `plato` read is no longer what `malt` holds.
        (
            "plato",
            "malt",
            InputRecord::Digest(fingerprint::of(&shape(Some(number(2.0))))),
        ),
    ] {
        let handle = sample.property(&id(name)).unwrap();
        let mut computed = IndexMap::new();
        computed.insert(InputName::Named(id(from)), digest);
        handle.set_records(Records {
            produced: None,
            channel_only: Default::default(),
            statistics: None,
            failure: None,
            fingerprint: Some(fingerprint::of(&shape(Some(number(1.0))))),
            computed: Some(computed),
        });
    }
    let report = fingerprint::check(&sample).unwrap();
    for name in ["a", "b", "below"] {
        let Freshness::Unjudged { reason } = &report[&id(name)] else {
            panic!("{name}: {:?}", report[&id(name)]);
        };
        assert!(reason.contains('\u{2192}'), "{reason}");
    }
    assert_eq!(report[&id("malt")], Freshness::Source);
    assert!(
        matches!(&report[&id("plato")], Freshness::Stale { changed, .. } if changed.len() == 1),
        "{:?}",
        report[&id("plato")]
    );
}

#[test]
fn check_needs_no_model() {
    // The whole check runs on a sample loaded without Python.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample.materialize().unwrap();
    let shape = schema::from_sample(&sample).unwrap();
    let reloaded = schema::into_sample(shape).unwrap();
    assert!(!reloaded.property(&id("plato")).unwrap().is_computed());
    let report = fingerprint::check(&reloaded).unwrap();
    assert_eq!(report[&id("plato")], Freshness::Current);
    assert_eq!(report[&id("malt")], Freshness::Source);
}

#[test]
fn records_survive_a_modelless_round_trip() {
    // Load without a model, edit the note, save: records are byte-identical.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample.materialize().unwrap();
    let before = schema::from_sample(&sample).unwrap();
    let mut reloaded = schema::into_sample(before.clone()).unwrap();
    reloaded.set_note("edited".to_string());
    let after = schema::from_sample(&reloaded).unwrap();
    assert_eq!(before.properties, after.properties);
}

#[test]
fn assigning_a_value_clears_the_records() {
    // A property assigned by hand stops claiming a derivation.
    let mut sample = derived_sample();
    stamp(&mut sample);
    sample
        .property(&id("plato"))
        .unwrap()
        .set_value(number(99.0));
    assert!(sample.property(&id("plato")).unwrap().records().is_empty());
    assert_eq!(freshness(&sample, "plato"), Freshness::Source);
}

// ----------------------------------------------------------------- cells

/// `clarity = 1 / wort`, over the cask's foam.
struct OverGravity;

impl ComputeRow for OverGravity {
    fn compute(
        &self,
        row: &RowView,
        scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        let wort = match row.cell(&id("wort")).unwrap().value().unwrap() {
            Value::Number(n) => n,
            other => panic!("{other:?}"),
        };
        let foam = match scope.read(&id("foam"))? {
            Value::Number(n) => n,
            other => panic!("{other:?}"),
        };
        let mut out = IndexMap::new();
        out.insert(
            id("clarity"),
            CellOutput::Value(number(1.0 / (wort * foam))),
        );
        Ok(out)
    }
}

fn table_sample() -> Sample {
    let mut sample = Sample::new();
    sample
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let columns: IndexMap<Identifier, ColumnMeta> = ["temperature", "wort", "clarity"]
        .iter()
        .map(|name| (id(name), ColumnMeta::default()))
        .collect();
    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns,
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort")), InputName::Named(id("foam"))],
            formula: Rc::new(OverGravity),
        }],
    )
    .unwrap();
    for (t, r) in [(65.0, 12.5), (78.5, 20.0)] {
        table
            .add_row(vec![
                (id("temperature"), Property::stored(number(t))),
                (id("wort"), Property::stored(number(r))),
            ])
            .unwrap();
    }
    sample.set_table(id("mashing"), table).unwrap();
    sample.materialize().unwrap();
    sample
}

#[test]
fn a_cell_records_the_values_of_its_own_row() {
    // Two cells of one derived column carry two different records.
    let sample = table_sample();
    let records = fingerprint::record_table(&sample, &id("mashing")).unwrap();
    let clarity = &records[&id("clarity")];
    assert_eq!(clarity.len(), 2);
    let first = clarity[0].1.computed.as_ref().unwrap();
    let second = clarity[1].1.computed.as_ref().unwrap();
    assert_ne!(
        first[&InputName::Cell(id("wort"))],
        second[&InputName::Cell(id("wort"))]
    );
}

#[test]
fn a_cell_records_a_sample_input_too() {
    // The cask's foam appears in both, with one digest.
    let sample = table_sample();
    let records = fingerprint::record_table(&sample, &id("mashing")).unwrap();
    let clarity = &records[&id("clarity")];
    let first = clarity[0].1.computed.as_ref().unwrap();
    let second = clarity[1].1.computed.as_ref().unwrap();
    let key = InputName::Named(id("foam"));
    assert_eq!(first[&key], second[&key]);
    assert!(matches!(first[&key], InputRecord::Digest(_)));
}

#[test]
fn a_cell_is_checked_on_its_own_row() {
    // Correcting one row's input reports that row's derived cell stale and
    // leaves the other Current.
    let mut sample = table_sample();
    let records = fingerprint::record_table(&sample, &id("mashing")).unwrap();
    // Apply the records by rebuilding the table with them on its cells.
    let rebuilt = {
        let held = sample.table(&id("mashing")).unwrap();
        let columns: IndexMap<Identifier, ColumnMeta> = held
            .column_names()
            .into_iter()
            .map(|name| (name.clone(), ColumnMeta::default()))
            .collect();
        let mut table =
            Table::new(id("mashing"), vec![id("temperature")], columns, Vec::new()).unwrap();
        for (at, row) in held.rows().enumerate() {
            let mut cells = Vec::new();
            for column in ["temperature", "wort", "clarity"] {
                let cell = row.cell(&id(column)).unwrap();
                let mut built = Property::stored(cell.value().unwrap());
                if column == "clarity" {
                    built.set_records(records[&id("clarity")][at].1.clone());
                }
                cells.push((id(column), built));
            }
            table.add_row(cells).unwrap();
        }
        table
    };
    sample.remove_table(&id("mashing")).unwrap();
    sample.set_table(id("mashing"), rebuilt).unwrap();

    assert_eq!(
        fingerprint::check_cell(&sample, &id("mashing"), &id("clarity"), &[number(65.0)]).unwrap(),
        Freshness::Current
    );

    // Change the 65 K wort, and only that row's clarity moves.
    let held = sample.table(&id("mashing")).unwrap();
    let mut rows: Vec<Vec<(Identifier, Property)>> = Vec::new();
    for (at, row) in held.rows().enumerate() {
        let mut cells = Vec::new();
        for column in ["temperature", "wort", "clarity"] {
            let cell = row.cell(&id(column)).unwrap();
            let mut built = if at == 0 && column == "wort" {
                Property::stored(number(99.0))
            } else {
                Property::stored(cell.value().unwrap())
            };
            built.set_records(cell.records().clone());
            cells.push((id(column), built));
        }
        rows.push(cells);
    }
    let columns: IndexMap<Identifier, ColumnMeta> = held
        .column_names()
        .into_iter()
        .map(|name| (name.clone(), ColumnMeta::default()))
        .collect();
    let mut table =
        Table::new(id("mashing"), vec![id("temperature")], columns, Vec::new()).unwrap();
    for cells in rows {
        table.add_row(cells).unwrap();
    }
    sample.remove_table(&id("mashing")).unwrap();
    sample.set_table(id("mashing"), table).unwrap();

    assert!(matches!(
        fingerprint::check_cell(&sample, &id("mashing"), &id("clarity"), &[number(65.0)]).unwrap(),
        Freshness::Stale { .. }
    ));
    assert_eq!(
        fingerprint::check_cell(&sample, &id("mashing"), &id("clarity"), &[number(78.5)]).unwrap(),
        Freshness::Current
    );
    let report = fingerprint::check_table(&sample, &id("mashing")).unwrap();
    assert_eq!(report[&id("temperature")][0].1, Freshness::Source);
}

#[test]
fn a_sample_wide_check_composes_properties_and_cells() {
    // `check_sample` is `check` and every table's `check_table`, and nothing
    // else: three callers composed them by hand, each dropping the error in its
    // own way.
    let sample = table_sample();
    let standing = fingerprint::check_sample(&sample);
    assert_eq!(standing.properties, fingerprint::check(&sample).unwrap());
    let tables: Vec<&Identifier> = standing.tables.keys().collect();
    assert_eq!(tables, sample.table_names());
    for table in sample.table_names() {
        assert_eq!(
            standing.tables[table],
            fingerprint::check_table(&sample, table).unwrap()
        );
    }
    assert!(!standing.tables[&id("mashing")].is_empty());
}

#[test]
fn a_column_digest_covers_every_cell() {
    // Changing one cell changes the digest a column derivation recorded.
    let sample = table_sample();
    let before = {
        let held = sample.table(&id("mashing")).unwrap();
        fingerprint::of_column(&held.column(&id("wort")).unwrap()).unwrap()
    };
    let mut moved = table_sample();
    {
        let held = moved.table(&id("mashing")).unwrap();
        let columns: IndexMap<Identifier, ColumnMeta> = held
            .column_names()
            .into_iter()
            .map(|name| (name.clone(), ColumnMeta::default()))
            .collect();
        let mut table =
            Table::new(id("mashing"), vec![id("temperature")], columns, Vec::new()).unwrap();
        for (at, row) in held.rows().enumerate() {
            let mut cells = Vec::new();
            for column in ["temperature", "wort", "clarity"] {
                let value = if at == 0 && column == "wort" {
                    number(99.0)
                } else {
                    row.cell(&id(column)).unwrap().value().unwrap()
                };
                cells.push((id(column), Property::stored(value)));
            }
            table.add_row(cells).unwrap();
        }
        moved.remove_table(&id("mashing")).unwrap();
        moved.set_table(id("mashing"), table).unwrap();
    }
    let after = {
        let held = moved.table(&id("mashing")).unwrap();
        fingerprint::of_column(&held.column(&id("wort")).unwrap()).unwrap()
    };
    assert_ne!(before, after);
}

#[test]
fn a_column_digest_follows_row_order() {
    // Not index order, so it does not depend on a comparison the file never
    // records.
    let build = |pairs: &[(f64, f64)]| {
        let columns: IndexMap<Identifier, ColumnMeta> = [id("k"), id("v")]
            .into_iter()
            .map(|name| (name, ColumnMeta::default()))
            .collect();
        let mut table = Table::new(id("t"), vec![id("k")], columns, Vec::new()).unwrap();
        for (k, v) in pairs {
            table
                .add_row(vec![
                    (id("k"), Property::stored(number(*k))),
                    (id("v"), Property::stored(number(*v))),
                ])
                .unwrap();
        }
        fingerprint::of_column(&table.column(&id("v")).unwrap()).unwrap()
    };
    // The same cells, entered in two orders, are two digests.
    assert_ne!(
        build(&[(1.0, 10.0), (2.0, 20.0)]),
        build(&[(2.0, 20.0), (1.0, 10.0)])
    );
}

// ----------------------------------------------------------------- filling

/// A record that no longer matches the inputs it names: a stale file.
fn stale_record() -> samplekit::core::property::Records {
    use samplekit::core::property::{Fingerprint, InputName, InputRecord, Records};
    Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::new("0123456789ab")),
        computed: Some(
            [
                (
                    InputName::Named(id("malt")),
                    InputRecord::Digest(Fingerprint::new("000000000000")),
                ),
                (
                    InputName::Named(id("volume")),
                    InputRecord::Digest(Fingerprint::new("000000000000")),
                ),
            ]
            .into_iter()
            .collect(),
        ),
    }
}

/// `derived_sample`'s declaration, filled by a file whose plato carries a
/// stale record.
fn filled_plato() -> Sample {
    let mut file = Sample::new();
    file.set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    file.set_property(id("volume"), Property::stored(number(4.1)))
        .unwrap();
    let mut plato = Property::stored(number(3.0488));
    plato.set_records(stale_record());
    file.set_property(id("plato"), plato).unwrap();
    let mut sample = derived_sample();
    sample.fill_from(file).unwrap();
    sample
}

/// A cell formula, so a model's table has a derivation to keep.
struct FillCell;

impl samplekit::core::table::ComputeRow for FillCell {
    fn compute(
        &self,
        _: &samplekit::core::table::RowView,
        _: &dyn samplekit::core::table::Scope,
    ) -> Result<indexmap::IndexMap<Identifier, samplekit::core::table::CellOutput>, ComputeError>
    {
        Ok([(
            id("clarity"),
            samplekit::core::table::CellOutput::Value(number(1.0)),
        )]
        .into_iter()
        .collect())
    }
}

#[test]
fn a_value_a_file_supplied_keeps_its_record() {
    use samplekit::core::property::InputName;
    use samplekit::core::table::{ColumnMeta, Derivation, Table};

    let sample = filled_plato();
    let records = fingerprint::record(&sample).unwrap();
    assert_eq!(records[&id("plato")], stale_record());

    // And a derived cell the file supplied, likewise.
    let columns = || {
        ["temperature", "wort", "clarity"]
            .iter()
            .map(|name| (id(name), ColumnMeta::default()))
            .collect::<indexmap::IndexMap<_, _>>()
    };
    let mut model = Sample::new();
    model
        .set_table(
            id("mashing"),
            Table::new(
                id("mashing"),
                vec![id("temperature")],
                columns(),
                vec![Derivation::Row {
                    outputs: vec![id("clarity")],
                    inputs: vec![InputName::Cell(id("wort"))],
                    formula: Rc::new(FillCell),
                }],
            )
            .unwrap(),
        )
        .unwrap();
    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(),
        Vec::new(),
    )
    .unwrap();
    let mut cell = Property::stored(number(0.08));
    cell.set_records(stale_record());
    table
        .add_row(vec![
            (id("temperature"), Property::stored(number(65.0))),
            (id("wort"), Property::stored(number(12.5))),
            (id("clarity"), cell),
        ])
        .unwrap();
    let mut file = Sample::new();
    file.set_table(id("mashing"), table).unwrap();
    model.fill_from(file).unwrap();
    let cells = fingerprint::record_table(&model, &id("mashing")).unwrap();
    assert_eq!(cells[&id("clarity")][0].1, stale_record());
}

#[test]
fn a_recomputed_value_is_restamped() {
    use samplekit::core::property::{Fingerprint, InputName, InputRecord};
    let sample = filled_plato();
    let plato = sample.property(&id("plato")).unwrap();
    plato.invalidate();
    let _ = plato.value().unwrap();
    let records = fingerprint::record(&sample).unwrap();
    let computed = records[&id("plato")].computed.clone().unwrap();
    assert_ne!(
        computed[&InputName::Named(id("malt"))],
        InputRecord::Digest(Fingerprint::new("000000000000"))
    );
}

// -------------------------------------------------------------- stamping

/// malt and volume entered, plato computed from both, its runs counted.
fn counted_plato() -> (Sample, Rc<Constant>) {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("volume"), Property::stored(number(4.1)))
        .unwrap();
    let formula = Constant::new(3.0488);
    sample
        .set_property(id("plato"), Property::computed(formula.clone()))
        .unwrap();
    sample
        .declare_dependencies(
            &id("plato"),
            &[Node::Named(id("malt")), Node::Named(id("volume"))],
        )
        .unwrap();
    (sample, formula)
}

#[test]
fn stamping_records_a_value_computed_in_session() {
    let (mut sample, _) = counted_plato();
    sample.property(&id("plato")).unwrap().value().unwrap();
    assert!(fingerprint::stamp(&mut sample).unwrap().is_empty());
    let records = sample.property(&id("plato")).unwrap().records();
    assert_eq!(records.computed.unwrap().len(), 2);
    assert!(records.fingerprint.is_some());
    assert_eq!(freshness(&sample, "plato"), Freshness::Current);
}

#[test]
fn a_stamped_value_goes_stale_when_its_input_changes() {
    let (mut sample, formula) = counted_plato();
    sample.property(&id("plato")).unwrap().value().unwrap();
    fingerprint::stamp(&mut sample).unwrap();
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(20.0));
    assert!(matches!(
        freshness(&sample, "plato"),
        Freshness::Stale { .. }
    ));
    assert!(fingerprint::is_stale(&sample, &id("plato")).unwrap());
    assert!(fingerprint::stamp(&mut sample).unwrap().is_empty());
    assert_eq!(formula.calls.get(), 1);
}

#[test]
fn a_value_overtaken_before_recording_cannot_be_stamped() {
    let (mut sample, _) = counted_plato();
    sample.property(&id("plato")).unwrap().value().unwrap();
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(20.0));
    assert_eq!(
        fingerprint::stamp(&mut sample).unwrap(),
        vec!["plato".to_string()]
    );
    let plato = sample.property(&id("plato")).unwrap();
    assert!(plato.records().computed.is_none());
    assert!(fingerprint::is_stale(&sample, &id("plato")).unwrap());
}

#[test]
fn a_recomputation_that_changed_nothing_is_confirmed_current() {
    let (mut sample, formula) = counted_plato();
    let plato = sample.property(&id("plato")).unwrap();
    plato.value().unwrap();
    fingerprint::stamp(&mut sample).unwrap();
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(12.5));
    assert!(!plato.is_current());
    fingerprint::stamp(&mut sample).unwrap();
    assert!(plato.is_current());
    plato.value().unwrap();
    assert_eq!(formula.calls.get(), 1);
}

#[test]
fn a_value_never_computed_is_not_stale() {
    let (sample, formula) = counted_plato();
    assert!(!fingerprint::is_stale(&sample, &id("plato")).unwrap());
    assert_eq!(formula.calls.get(), 0);
}

#[test]
fn an_unchanged_input_confirms_its_dependent_below_a_stale_upstream() {
    use samplekit::core::property::{Fingerprint, InputName, InputRecord, Records};
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    let vfi = Constant::new(7.25);
    sample
        .set_property(id("grist"), Property::computed(Constant::new(25.0)))
        .unwrap();
    sample
        .set_property(id("brix"), Property::computed(Constant::new(6.25)))
        .unwrap();
    sample
        .set_property(id("vfi"), Property::computed(vfi.clone()))
        .unwrap();
    for (dependent, input) in [("grist", "malt"), ("brix", "grist"), ("vfi", "brix")] {
        sample
            .declare_dependencies(&id(dependent), &[Node::Named(id(input))])
            .unwrap();
    }
    for name in ["grist", "brix", "vfi"] {
        sample.property(&id(name)).unwrap().value().unwrap();
    }
    fingerprint::stamp(&mut sample).unwrap();

    // The grist's record no longer matches its malt, as a corrected file leaves it.
    sample.property(&id("grist")).unwrap().set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: None,
        computed: Some(
            [(
                InputName::Named(id("malt")),
                InputRecord::Digest(Fingerprint::new("000000000000")),
            )]
            .into_iter()
            .collect(),
        ),
    });
    // The plato is computed again, to the value it had.
    let brix = sample.property(&id("brix")).unwrap();
    brix.invalidate();
    brix.value().unwrap();
    fingerprint::stamp(&mut sample).unwrap();

    let below = sample.property(&id("vfi")).unwrap();
    assert!(below.is_current());
    below.value().unwrap();
    assert_eq!(vfi.calls.get(), 1);
    assert!(fingerprint::is_stale(&sample, &id("vfi")).unwrap());
}

// ------------------------------------------------------------- overrides

/// malt entered; grist, brix and vfi computed, each from the last.
fn dosed() -> Sample {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    for (name, value) in [("grist", 25.0), ("brix", 6.25), ("vfi", 7.25)] {
        sample
            .set_property(id(name), Property::computed(Constant::new(value)))
            .unwrap();
    }
    for (dependent, input) in [("grist", "malt"), ("brix", "grist"), ("vfi", "brix")] {
        sample
            .declare_dependencies(&id(dependent), &[Node::Named(id(input))])
            .unwrap();
    }
    sample
}

fn overridden_then_read(value: f64, reads: &[&str]) -> Sample {
    let mut sample = dosed();
    sample
        .property(&id("grist"))
        .unwrap()
        .set_value(number(value));
    for name in reads {
        sample.property(&id(name)).unwrap().value().unwrap();
    }
    fingerprint::stamp(&mut sample).unwrap();
    sample
}

#[test]
fn an_override_is_edited_and_not_stale() {
    // Never computed.
    let sample = dosed();
    sample
        .property(&id("grist"))
        .unwrap()
        .set_value(number(3.2));
    assert!(matches!(freshness(&sample, "grist"), Freshness::Edited));
    assert!(!fingerprint::is_stale(&sample, &id("grist")).unwrap());

    // Computed and recorded first.
    let mut sample = dosed();
    sample.property(&id("grist")).unwrap().value().unwrap();
    fingerprint::stamp(&mut sample).unwrap();
    sample
        .property(&id("grist"))
        .unwrap()
        .set_value(number(3.2));
    assert!(matches!(freshness(&sample, "grist"), Freshness::Edited));
    assert!(!fingerprint::is_stale(&sample, &id("grist")).unwrap());

    // Marked in a record with no formula, as a file read alone has it.
    let mut loose = Sample::new();
    let mut held = Property::stored(number(3.2));
    held.set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(Fingerprint::edited("7c01e2b3a9f4")),
        computed: None,
    });
    loose.set_property(id("grist"), held).unwrap();
    assert!(matches!(freshness(&loose, "grist"), Freshness::Edited));
}

#[test]
fn an_overridden_input_is_recorded_with_its_mark() {
    let sample = overridden_then_read(3.2, &["brix"]);
    let grist = sample.property(&id("grist")).unwrap();
    let current = fingerprint::of(&grist.peek(schema::property_as_is));

    let records = sample.property(&id("brix")).unwrap().records();
    let InputRecord::Digest(recorded) =
        &records.computed.as_ref().unwrap()[&InputName::Named(id("grist"))]
    else {
        panic!("a property input is a digest");
    };
    assert!(recorded.is_edited());
    assert!(recorded.same_digest(&current));

    let own = grist.records().fingerprint.unwrap();
    assert!(own.is_edited());
    assert!(own.same_digest(&current));
}

#[test]
fn what_reads_an_override_is_not_stale_because_of_it() {
    let sample = overridden_then_read(3.2, &["brix"]);
    assert!(matches!(freshness(&sample, "brix"), Freshness::Current));

    // The override's own input moves: it holds, and nothing is stale.
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert!(matches!(freshness(&sample, "brix"), Freshness::Current));
    assert!(!fingerprint::is_stale(&sample, &id("brix")).unwrap());

    // The override is set again: what read it is stale, as changed.
    sample
        .property(&id("grist"))
        .unwrap()
        .set_value(number(4.0));
    match freshness(&sample, "brix") {
        Freshness::Stale { changed, upstream } => {
            assert_eq!(changed, vec![InputName::Named(id("grist"))]);
            assert!(upstream.is_empty());
        }
        other => panic!("expected stale, got {other:?}"),
    }
}

#[test]
fn edited_upstream_follows_the_records_through_a_chain() {
    let sample = overridden_then_read(3.2, &["brix", "vfi"]);
    assert_eq!(
        fingerprint::edited_upstream(&sample, &id("vfi")),
        vec![id("grist")]
    );
    assert_eq!(
        fingerprint::edited_upstream(&sample, &id("brix")),
        vec![id("grist")]
    );
    // Nothing past an override: its own record describes its formula.
    assert!(fingerprint::edited_upstream(&sample, &id("grist")).is_empty());
    assert!(fingerprint::edited_upstream(&sample, &id("malt")).is_empty());
}

#[test]
fn a_marked_record_compares_by_its_digest() {
    let mut sample = overridden_then_read(3.2, &["brix"]);
    // The same content, no longer an override.
    sample
        .set_property(id("grist"), Property::stored(number(3.2)))
        .unwrap();
    assert!(matches!(freshness(&sample, "brix"), Freshness::Current));
}

struct Doubling;

impl ComputeRow for Doubling {
    fn compute(
        &self,
        _: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        let mut out = IndexMap::new();
        out.insert(id("double"), CellOutput::Value(number(2.0)));
        Ok(out)
    }
}

#[test]
fn a_cell_resting_on_a_stale_value_is_stale() {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    sample
        .set_property(id("brix"), Property::computed(Constant::new(6.25)))
        .unwrap();
    sample
        .declare_dependencies(&id("brix"), &[Node::Named(id("malt"))])
        .unwrap();
    let mut table = Table::new(
        id("boil"),
        vec![id("step")],
        ["step", "double"]
            .iter()
            .map(|name| (id(name), ColumnMeta::default()))
            .collect(),
        vec![Derivation::Row {
            outputs: vec![id("double")],
            inputs: vec![InputName::Named(id("brix"))],
            formula: Rc::new(Doubling),
        }],
    )
    .unwrap();
    table
        .add_row(vec![(id("step"), Property::stored(number(1.0)))])
        .unwrap();
    sample.set_table(id("boil"), table).unwrap();
    sample.property(&id("brix")).unwrap().value().unwrap();
    sample.resolve_table(&id("boil")).unwrap();
    fingerprint::stamp(&mut sample).unwrap();
    let index = vec![number(1.0)];
    assert!(matches!(
        fingerprint::check_cell(&sample, &id("boil"), &id("double"), &index).unwrap(),
        Freshness::Current
    ));

    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(13.0));
    match fingerprint::check_cell(&sample, &id("boil"), &id("double"), &index).unwrap() {
        Freshness::Stale { changed, upstream } => {
            assert!(changed.is_empty());
            assert_eq!(upstream, vec![InputName::Named(id("brix"))]);
        }
        other => panic!("expected stale, got {other:?}"),
    }
    assert!(fingerprint::is_cell_stale(&sample, &id("boil"), &id("double"), &index).unwrap());
}

#[test]
fn edited_upstream_follows_the_graph_before_anything_is_read() {
    let sample = dosed();
    sample
        .property(&id("grist"))
        .unwrap()
        .set_value(number(3.2));
    assert_eq!(
        fingerprint::edited_upstream(&sample, &id("vfi")),
        vec![id("grist")]
    );
    assert_eq!(
        fingerprint::edited_upstream(&sample, &id("brix")),
        vec![id("grist")]
    );
    assert!(fingerprint::edited_upstream(&sample, &id("grist")).is_empty());
}

#[test]
fn a_supplied_cell_without_a_record_is_not_given_one() {
    // Only what a formula wrote in this session is recorded.
    let columns = || {
        ["temperature", "wort", "clarity"]
            .iter()
            .map(|name| (id(name), ColumnMeta::default()))
            .collect::<IndexMap<Identifier, ColumnMeta>>()
    };
    let mut declared = Sample::new();
    declared
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort")), InputName::Named(id("foam"))],
            formula: Rc::new(OverGravity),
        }],
    )
    .unwrap();
    declared.set_table(id("mashing"), table).unwrap();
    let mut file = Sample::new();
    let mut supplied = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(),
        Vec::new(),
    )
    .unwrap();
    supplied
        .add_row(vec![
            (id("temperature"), Property::stored(number(65.0))),
            (id("wort"), Property::stored(number(12.5))),
            (id("clarity"), Property::stored(number(99.0))),
        ])
        .unwrap();
    file.set_table(id("mashing"), supplied).unwrap();
    declared.fill_from(file).unwrap();
    let unrecordable = fingerprint::stamp(&mut declared).unwrap();
    assert!(unrecordable.is_empty(), "{unrecordable:?}");
    let at = samplekit::core::table::RowAddress::Index(vec![number(65.0)]);
    let held = declared.table(&id("mashing")).unwrap();
    let cell = held.at(&at, &id("clarity")).unwrap();
    assert!(cell.records().computed.is_none());
    assert_eq!(cell.value().unwrap(), number(99.0));
}

#[test]
fn a_value_without_its_record_is_not_stamped_edited() {
    // *record missing* is a state of its own — "someone removed the record"
    // sends the reader somewhere else than "someone changed the value". It is
    // held in the same cache state as an override, which is what made `stamp`
    // write `{edited: …}` over it: writing anything else into a migrated sample
    // then made its file claim a hand had typed a number nobody touched, and a
    // migrated collection is exactly where every derived value arrives without
    // a record.
    let mut declared = derived_sample();
    let mut file = Sample::new();
    file.set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    file.set_property(id("volume"), Property::stored(number(4.1)))
        .unwrap();
    // The value a formula owns, supplied by the file with no record of how it
    // came — what `load_into` holds as record missing.
    file.set_property(id("plato"), Property::stored(number(3.0488)))
        .unwrap();
    declared.fill_from(file).unwrap();
    let held = declared
        .property(&id("plato"))
        .unwrap()
        .hold_record_missing();
    assert!(held, "the value is held as record missing");

    fingerprint::stamp(&mut declared).unwrap();

    let records = declared.property(&id("plato")).unwrap().records();
    assert!(
        records
            .fingerprint
            .as_ref()
            .is_none_or(|fingerprint| !fingerprint.is_edited()),
        "a value with no record is not marked edited, got {:?}",
        records.fingerprint
    );
}

#[test]
fn a_derived_cell_without_its_record_is_record_missing() {
    // A value no record explains, in a column a model derives.
    let columns = || {
        ["temperature", "wort", "clarity"]
            .iter()
            .map(|name| (id(name), ColumnMeta::default()))
            .collect::<IndexMap<Identifier, ColumnMeta>>()
    };
    let mut declared = Sample::new();
    declared
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort")), InputName::Named(id("foam"))],
            formula: Rc::new(OverGravity),
        }],
    )
    .unwrap();
    declared.set_table(id("mashing"), table).unwrap();
    let mut file = Sample::new();
    let mut supplied = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(),
        Vec::new(),
    )
    .unwrap();
    supplied
        .add_row(vec![
            (id("temperature"), Property::stored(number(65.0))),
            (id("wort"), Property::stored(number(12.5))),
            (id("clarity"), Property::stored(number(99.0))),
        ])
        .unwrap();
    file.set_table(id("mashing"), supplied).unwrap();
    declared.fill_from(file).unwrap();
    let state = fingerprint::check_cell(&declared, &id("mashing"), &id("clarity"), &[number(65.0)])
        .unwrap();
    assert!(
        matches!(state, fingerprint::Freshness::RecordMissing),
        "{state:?}"
    );
}

#[test]
fn a_check_spanning_two_samples_keeps_their_verdicts_apart() {
    // A verdict is one sample's: a check opened around a loop over two samples
    // never answers for one what it concluded about the other.
    let mut current = derived_sample();
    stamp(&mut current);
    let mut moved = derived_sample();
    stamp(&mut moved);
    moved.property(&id("malt")).unwrap().set_value(number(99.0));

    let _verdicts = fingerprint::Verdicts::begin();
    assert_eq!(freshness(&current, "plato"), Freshness::Current);
    assert_eq!(
        freshness(&moved, "plato"),
        Freshness::Stale {
            changed: vec![InputName::Named(id("malt"))],
            upstream: Vec::new()
        }
    );
    assert_eq!(freshness(&current, "plato"), Freshness::Current);
}

#[test]
fn a_declared_statistic_carries_a_record() {
    // `sk.stats.mean` and `sk.stats.standard_error` are formulas that happen to
    // be named. The property records the readings they were taken over, under
    // the reserved key, and is current.
    let mut sample = Sample::new();
    let malt = Property::measured(
        samplekit::core::value::Readings::new(vec![3.0571, 3.0557, 3.0584]).unwrap(),
        Some(samplekit::core::uncertainty::Convention::StandardError),
    );
    sample.set_property(id("malt"), malt).unwrap();
    let _ = sample.property(&id("malt")).unwrap().uncertainty();

    let records = fingerprint::record(&sample).unwrap();
    let malt_records = records
        .get(&id("malt"))
        .expect("a declared statistic records");
    let inputs = malt_records.computed.as_ref().expect("a computed record");
    assert_eq!(
        inputs.keys().map(|k| k.to_string()).collect::<Vec<_>>(),
        vec!["readings".to_string()],
        "its own observations, never an input the sample holds"
    );
    assert!(malt_records.fingerprint.is_some());

    for (name, records) in records {
        sample.property(&name).unwrap().set_records(records);
    }
    let verdict = fingerprint::check_property(&sample, &id("malt")).unwrap();
    assert!(
        matches!(verdict, Freshness::Current),
        "a record just taken is current: {verdict:?}"
    );
}

/// `malt` as a model declaring both statistics holds it: the uncertainty follows
/// the readings live.
fn malt_by_statistics(values: Vec<f64>) -> Property {
    use samplekit::core::uncertainty::Convention;
    let mut malt = Property::measured(
        samplekit::core::value::Readings::new(values).unwrap(),
        Some(Convention::StandardError),
    );
    malt.declare_statistics(
        Some(samplekit::core::statistics::Location::Mean),
        Some(Convention::StandardError),
    );
    malt
}

#[test]
fn an_uncertainty_the_readings_give_is_outside_the_own_digest() {
    // Under a model the uncertainty follows the readings live, so a corrected
    // dosing moved the own digest and read *edited* from Python where the
    // command line, holding the file's stored number, read *stale — readings*.
    // The channel the readings give is left out of that digest.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), malt_by_statistics(vec![1.0, 2.0, 3.0]))
        .unwrap();
    let handle = sample.property(&id("malt")).unwrap();
    let _ = handle.uncertainty();
    let taken = fingerprint::record(&sample).unwrap();
    let recorded = taken.get(&id("malt")).expect("a record").clone();
    let value = handle
        .peek(samplekit::format::schema::property_as_is)
        .value
        .unwrap();

    // As a model loads the corrected file: new readings, the file's value
    // written beside them, and an uncertainty nobody pinned.
    let mut corrected = malt_by_statistics(vec![1.0, 2.0, 6.0]);
    corrected.set_written_value(Some(value));
    sample.remove_property(&id("malt")).unwrap();
    sample.set_property(id("malt"), corrected).unwrap();
    sample.property(&id("malt")).unwrap().set_records(recorded);

    let verdict = fingerprint::check_property(&sample, &id("malt")).unwrap();
    let Freshness::Stale { changed, .. } = &verdict else {
        panic!("stale by its readings, under a model too: {verdict:?}");
    };
    assert_eq!(changed[0].to_string(), "readings");
}

#[test]
fn a_value_written_over_a_statistic_is_stamped_edited() {
    // A save gave the override a fresh, unmarked digest, and the file then read
    // *current* — `set` marked its own and a save from Python did not. With no
    // record to be measured against, nothing is stamped.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), malt_by_statistics(vec![1.0, 2.0, 3.0]))
        .unwrap();
    let _ = sample.property(&id("malt")).unwrap().uncertainty();
    stamp(&mut sample);
    let handle = sample.property(&id("malt")).unwrap();
    assert!(!handle.records().fingerprint.unwrap().is_edited());
    handle.set_value(number(4.75));
    // `fingerprint::stamp` itself: the path `save_computed` takes.
    fingerprint::stamp(&mut sample).unwrap();
    let handle = sample.property(&id("malt")).unwrap();
    assert!(handle.records().fingerprint.unwrap().is_edited());
    assert_eq!(freshness(&sample, "malt"), Freshness::Edited);

    let mut migrated = Sample::new();
    let mut held = malt_by_statistics(vec![1.0, 2.0, 9.0]);
    held.set_written_value(Some(number(5.0)));
    migrated.set_property(id("malt"), held).unwrap();
    fingerprint::stamp(&mut migrated).unwrap();
    assert!(
        migrated.property(&id("malt")).unwrap().records().is_empty(),
        "no record says what the statistic gave, so none is invented"
    );
}

#[test]
fn a_corrected_reading_stales_its_own_statistic() {
    // The whole point: a dosing corrected by hand leaves the
    // statistic behind, and the property says so instead of staying silent.
    let readings = |values: Vec<f64>| samplekit::core::value::Readings::new(values).unwrap();
    let mut sample = Sample::new();
    sample
        .set_property(
            id("malt"),
            Property::measured(
                readings(vec![3.0571, 3.0557, 3.0584]),
                Some(samplekit::core::uncertainty::Convention::StandardError),
            ),
        )
        .unwrap();
    let _ = sample.property(&id("malt")).unwrap().uncertainty();
    let taken = fingerprint::record(&sample).unwrap();
    let recorded = taken.get(&id("malt")).expect("a record").clone();

    // One dosing corrected **as a file holds it**: the readings move, and the
    // value and uncertainty the file recorded stay as they were — they are not
    // re-derived by a reader without the model, which is the state `check` is
    // asked about. Re-deriving them in memory would move the quantity too, and
    // the question would stop being *did the observations move*.
    let held = sample
        .property(&id("malt"))
        .unwrap()
        .peek(samplekit::format::schema::property_as_is);
    let handle = sample.property(&id("malt")).unwrap();
    handle.set_readings(
        readings(vec![3.0571, 3.1259, 3.0584]),
        Some(samplekit::core::uncertainty::Convention::StandardError),
    );
    handle.set_value(
        held.value
            .clone()
            .unwrap_or_else(samplekit::core::value::Value::absent),
    );
    handle.set_uncertainty(
        held.uncertainty
            .map(|magnitude| samplekit::core::uncertainty::Uncertainty::new(magnitude).unwrap()),
    );
    handle.set_records(recorded.clone());

    let verdict = fingerprint::check_property(&sample, &id("malt")).unwrap();
    let Freshness::Stale { changed, .. } = &verdict else {
        panic!("a corrected reading leaves the statistic behind: {verdict:?}");
    };
    assert_eq!(
        changed.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        vec!["readings".to_string()],
        "named as its own observations"
    );
}

#[test]
fn a_written_value_beside_readings_is_edited_not_stale() {
    // Two different sentences about two different events: the readings did not
    // move, the quantity did.
    let mut sample = Sample::new();
    sample
        .set_property(
            id("malt"),
            Property::measured(
                samplekit::core::value::Readings::new(vec![1.0, 2.0, 9.0]).unwrap(),
                Some(samplekit::core::uncertainty::Convention::SampleStdev),
            ),
        )
        .unwrap();
    let _ = sample.property(&id("malt")).unwrap().uncertainty();
    for (name, records) in fingerprint::record(&sample).unwrap() {
        sample.property(&name).unwrap().set_records(records);
    }
    sample.property(&id("malt")).unwrap().set_value(number(2.5));
    let verdict = fingerprint::check_property(&sample, &id("malt")).unwrap();
    assert!(
        matches!(verdict, Freshness::Edited),
        "the quantity moved, not the observations: {verdict:?}"
    );
}

#[test]
fn readings_hash_alone_and_with_the_quantity() {
    // `of` asks *has this quantity changed*; `of_readings` asks *have the
    // observations changed*. Neither answers the other.
    let with_readings = |value: f64, uncertainty: f64, readings: Vec<f64>| PropertySchema {
        statistics: None,
        value: Some(number(value)),
        readings: Some(readings),
        uncertainty: Some(uncertainty),
        ..PropertySchema::default()
    };
    let base = with_readings(2.0, 0.5, vec![1.0, 2.0, 3.0]);
    let moved_value = with_readings(9.0, 0.5, vec![1.0, 2.0, 3.0]);
    let moved_readings = with_readings(2.0, 0.5, vec![1.0, 2.0, 4.0]);

    let readings_of = |shape: &PropertySchema| fingerprint::of_readings(shape).unwrap();
    assert_eq!(
        readings_of(&base),
        readings_of(&moved_value),
        "a changed value leaves the observations alone"
    );
    assert_ne!(readings_of(&base), readings_of(&moved_readings));
    assert_ne!(
        fingerprint::of(&base),
        fingerprint::of(&moved_value),
        "the quantity did change"
    );
    assert!(
        fingerprint::of_readings(&shape(Some(number(2.0)))).is_none(),
        "no readings, no digest"
    );
}

#[test]
fn an_uncertainty_a_formula_gives_carries_the_record() {
    // The record belongs to the property, not to a channel. A hydrometer's
    // resolution computed beside an entered ibu is as derived as a
    // plato, and goes stale the same way.
    let mut sample = Sample::new();
    sample
        .set_property(id("resolution"), Property::stored(number(0.01)))
        .unwrap();
    let mut age = Property::stored(number(2.0));
    age.set_uncertainty_formula(Constant::new(0.0058));
    sample.set_property(id("age"), age).unwrap();
    sample
        .declare_dependencies(&id("age"), &[Node::Named(id("resolution"))])
        .unwrap();
    let _ = sample.property(&id("age")).unwrap().uncertainty();

    let records = fingerprint::record(&sample).unwrap();
    let age_records = records.get(&id("age")).expect("a record");
    assert_eq!(
        age_records.computed.as_ref().map(|inputs| inputs.len()),
        Some(1),
        "the uncertainty's input is recorded on the property"
    );
    assert!(age_records.fingerprint.is_some());
    assert!(
        records
            .get(&id("resolution"))
            .and_then(|r| r.fingerprint.as_ref())
            .is_some(),
        "the input carries a fingerprint"
    );

    // Stamped in session, it is current; once the input moves, stale, naming it.
    fingerprint::stamp(&mut sample).unwrap();
    assert_eq!(freshness(&sample, "age"), Freshness::Current);
    assert!(!fingerprint::is_stale(&sample, &id("age")).unwrap());
    sample
        .property(&id("resolution"))
        .unwrap()
        .set_value(number(0.02));
    assert!(fingerprint::is_stale(&sample, &id("age")).unwrap());
    assert!(matches!(
        freshness(&sample, "age"),
        Freshness::Stale { ref changed, .. } if changed == &[InputName::Named(id("resolution"))]
    ));
}

#[test]
fn an_uncertainty_reading_its_own_value_records_the_channel() {
    // ±0.5 % of the reading. The value is entered, the uncertainty is a
    // formula's, and what that formula reads is the value beside it — which is
    // no edge of any graph, and was inexpressible before.
    let mut sample = Sample::new();
    let mut reading = Property::stored(number(200.0));
    reading.set_uncertainty_formula(Constant::new(1.0));
    sample.set_property(id("reading"), reading).unwrap();
    let own = InputName::Column {
        table: id("reading"),
        column: id("v"),
    };
    sample
        .declare_dependencies_by_channel(
            &id("reading"),
            &[],
            samplekit::core::property::ChannelInputs {
                value: Vec::new(),
                uncertainty: vec![own.clone()],
            },
        )
        .unwrap();
    let _ = sample.property(&id("reading")).unwrap().uncertainty();

    // The record names the channel it read, and its digest covers that channel
    // alone: were it the whole quantity's, writing the uncertainty would move
    // the input the uncertainty rests on.
    let records = fingerprint::record(&sample).unwrap();
    let held = records.get(&id("reading")).expect("a record");
    let recorded = held.computed.as_ref().expect("inputs");
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let InputRecord::Digest(digest) = &recorded[&own] else {
        panic!("a channel is hashed, not stored: {recorded:?}");
    };
    let mut value_alone = schema::property_as_is(&Property::stored(number(200.0)));
    value_alone.uncertainty = None;
    assert!(fingerprint::of(&value_alone).same_digest(digest));

    // Stamped, it is current; a corrected reading stales it, naming the
    // channel, and the uncertainty is the one thing the property may recompute.
    fingerprint::stamp(&mut sample).unwrap();
    assert_eq!(freshness(&sample, "reading"), Freshness::Current);
    sample
        .property(&id("reading"))
        .unwrap()
        .set_value(number(250.0));
    assert!(fingerprint::is_stale(&sample, &id("reading")).unwrap());
    assert!(
        matches!(
            freshness(&sample, "reading"),
            Freshness::Stale { ref changed, .. } if changed == std::slice::from_ref(&own)
        ),
        "{:?}",
        freshness(&sample, "reading")
    );
}

#[test]
fn a_record_factors_what_both_formulas_read() {
    // The declaration repeats what both formulas read; the file states it once,
    // and keys only what one formula alone reads.
    let mut sample = Sample::new();
    sample
        .set_property(id("src"), Property::stored(number(100.0)))
        .unwrap();
    let mut held = Property::computed(Constant::new(200.0));
    held.set_uncertainty_formula(Constant::new(1.1));
    sample.set_property(id("p"), held).unwrap();
    let own = InputName::Column {
        table: id("p"),
        column: id("v"),
    };
    let src = InputName::Named(id("src"));
    sample
        .declare_dependencies_by_channel(
            &id("p"),
            &[Node::Named(id("src"))],
            samplekit::core::property::ChannelInputs {
                value: vec![src.clone()],
                uncertainty: vec![src.clone(), own.clone()],
            },
        )
        .unwrap();
    let _ = sample.property(&id("p")).unwrap().value();
    let _ = sample.property(&id("p")).unwrap().uncertainty();

    let records = fingerprint::record(&sample).unwrap();
    let held = records.get(&id("p")).expect("a record");
    // Both formulas ran, so no channel is *the* one produced — and the input
    // the uncertainty alone reads is the only one keyed.
    assert_eq!(held.produced, None);
    assert_eq!(held.channel_only.len(), 1, "{:?}", held.channel_only);
    assert_eq!(
        held.channel_only.get(&own).copied(),
        Some(samplekit::core::property::Produced::Uncertainty)
    );
    assert!(held.computed.as_ref().unwrap().contains_key(&src));

    // What the file writes: the common input plainly, the channel's own under
    // its key.
    fingerprint::stamp(&mut sample).unwrap();
    let shape = sample
        .property(&id("p"))
        .unwrap()
        .peek(schema::property_as_is);
    let written = shape.computed.expect("a record");
    assert!(written.quantity.as_ref().unwrap().contains_key(&src));
    assert!(written.value.is_none(), "{written:?}");
    assert!(written.uncertainty.as_ref().unwrap().contains_key(&own));
}

#[test]
fn a_cycle_through_a_table_is_reported_not_followed() {
    // A property recording a column, whose cell records the property. A cell
    // asks about a property through the public entry point, which started a
    // fresh path: the walk went round forever and the stack overflowed, so
    // `status`, `validate` and `explain` aborted on a file that must
    // still open.
    let mut sample = Sample::new();
    sample
        .set_property(id("p"), Property::stored(number(1.0)))
        .unwrap();
    let columns: IndexMap<Identifier, ColumnMeta> = [id("k"), id("c")]
        .into_iter()
        .map(|name| (name, ColumnMeta::default()))
        .collect();
    let mut table = Table::new(id("t"), vec![id("k")], columns, Vec::new()).unwrap();
    table
        .add_row(vec![
            (id("k"), Property::stored(number(1.0))),
            (id("c"), Property::stored(number(2.0))),
        ])
        .unwrap();
    let column_digest = fingerprint::of_column(&table.column(&id("c")).unwrap()).unwrap();
    sample.set_table(id("t"), table).unwrap();

    // Both records hold what their inputs hash to now, so the walk goes on
    // past each comparison rather than stopping at a change.
    let mut reads_column = IndexMap::new();
    reads_column.insert(
        InputName::Column {
            table: id("t"),
            column: id("c"),
        },
        InputRecord::Digest(column_digest),
    );
    sample.property(&id("p")).unwrap().set_records(Records {
        produced: None,
        channel_only: Default::default(),
        statistics: None,
        failure: None,
        fingerprint: Some(fingerprint::of(&shape(Some(number(1.0))))),
        computed: Some(reads_column),
    });
    let mut reads_property = IndexMap::new();
    reads_property.insert(
        InputName::Named(id("p")),
        InputRecord::Digest(fingerprint::of(&shape(Some(number(1.0))))),
    );
    sample
        .set_cell_records(
            &id("t"),
            &samplekit::core::table::RowAddress::Index(vec![number(1.0)]),
            &id("c"),
            Records {
                produced: None,
                channel_only: Default::default(),
                statistics: None,
                failure: None,
                fingerprint: Some(fingerprint::of(&shape(Some(number(2.0))))),
                computed: Some(reads_property),
            },
        )
        .unwrap();

    let report = fingerprint::check(&sample).unwrap();
    let Freshness::Unjudged { reason } = &report[&id("p")] else {
        panic!("{:?}", report[&id("p")]);
    };
    assert!(reason.contains('\u{2192}'), "{reason}");
}

#[test]
fn a_recorded_failure_survives_a_save_without_the_model() {
    // A save without the model — `samplekit set` on another property — rewrote
    // the failed value with no verdict at all: the failure a file recorded
    // where the digest would be was dropped on the way out.
    let text = "---\nschema_version: 1\nname: F\nproperties:\n  \
        a: {v: 1.0, fingerprint: \"111111111111\"}\n  b: {v: 2.0}\n  \
        p: {v: 5.0, computed: {a: \"111111111111\"}, fingerprint: {failed: ZeroDivisionError}}\n---\n";
    let document = samplekit::format::document::parse(text).unwrap();
    let sample = schema::into_sample(document.schema).unwrap();
    sample.property(&id("b")).unwrap().set_value(number(3.0));
    let written = schema::from_sample(&sample).unwrap();
    let p = &written.properties[&id("p")];
    assert_eq!(p.failure.as_deref(), Some("ZeroDivisionError"), "{p:?}");
    assert!(p.fingerprint.is_none(), "{p:?}");
}

#[test]
fn two_integers_past_what_a_double_holds_hash_apart() {
    // Past 2^53 an integer was hashed as the nearest double, so two different
    // inputs hashed alike and a changed one read current.
    let one = fingerprint::of(&shape(Some(Value::integer(200_000_000_000_000_001))));
    let two = fingerprint::of(&shape(Some(Value::integer(200_000_000_000_000_002))));
    assert!(!one.same_digest(&two));
    // Within it, an integer and the equal number still hash alike.
    assert!(
        fingerprint::of(&shape(Some(Value::integer(200))))
            .same_digest(&fingerprint::of(&shape(Some(number(200.0)))))
    );
}

#[test]
fn a_cells_statistic_is_recorded_and_goes_stale_when_its_readings_move() {
    // A cell whose value is the statistic its column declares of its readings
    // records them, as a property's declared statistic does, and new readings
    // leave it *stale — readings*, never edited.
    use samplekit::core::property::DeclaredStatistics;
    use samplekit::core::statistics::Location;
    use samplekit::core::table::{ColumnMeta, RowAddress, Table};
    use samplekit::core::value::Readings;
    let mut columns = IndexMap::new();
    columns.insert(id("T"), ColumnMeta::default());
    columns.insert(
        id("sweetness"),
        ColumnMeta {
            presentation: Presentation::default(),
            statistics: DeclaredStatistics {
                value: Some(Location::Mean),
                uncertainty: None,
            },
        },
    );
    let mut table = Table::new(id("mouthfeel"), vec![id("T")], columns, Vec::new()).unwrap();
    table
        .add_row(vec![
            (id("T"), Property::stored(number(40.0))),
            (
                id("sweetness"),
                Property::measured(Readings::new(vec![1.0, 2.0, 3.0]).unwrap(), None),
            ),
        ])
        .unwrap();
    let mut sample = Sample::new();
    sample.set_table(id("mouthfeel"), table).unwrap();
    fingerprint::stamp(&mut sample).unwrap();
    // As a file holds it: the statistic written beside its readings.
    let mut sample = schema::into_sample(schema::from_sample_as_is(&sample).unwrap()).unwrap();
    let index = [number(40.0)];
    let verdict = |sample: &Sample| {
        fingerprint::check_cell(sample, &id("mouthfeel"), &id("sweetness"), &index).unwrap()
    };
    assert_eq!(verdict(&sample), Freshness::Current);
    let (records, written) = {
        let held = sample.table(&id("mouthfeel")).unwrap();
        let cell = held
            .at(&RowAddress::Index(index.to_vec()), &id("sweetness"))
            .unwrap();
        assert!(
            cell.records()
                .computed
                .as_ref()
                .is_some_and(|inputs| inputs.contains_key(&InputName::Named(id("readings"))))
        );
        (cell.records().clone(), cell.written_value().cloned())
    };
    assert_eq!(written, Some(number(2.0)));
    // A corrected reading: the value written beside the old ones stays, and
    // so does its record.
    let mut corrected = Property::measured(Readings::new(vec![1.0, 2.0, 6.0]).unwrap(), None);
    corrected.set_written_value(written);
    corrected.set_records(records);
    sample
        .update_row(
            &id("mouthfeel"),
            &RowAddress::Index(index.to_vec()),
            vec![(id("sweetness"), corrected)],
        )
        .unwrap();
    assert_eq!(
        verdict(&sample),
        Freshness::Stale {
            changed: vec![InputName::Named(id("readings"))],
            upstream: Vec::new(),
        }
    );
}
