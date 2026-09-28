//! The tests of `sample`.

use std::cell::Cell;
use std::error::Error;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::dependency_graph::Node;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{
    Compute, ComputeError, Fingerprint, InputName, InputRecord, Property, Records,
};
use samplekit::core::sample::{
    AttributeError, AttributeValue, NameKind, PropertyHandle, Sample, SampleError,
};
use samplekit::core::table::{
    CellOutput, ColumnMeta, ComputeRow, Derivation, RowView, Scope, Table,
};
use samplekit::core::value::Value;

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn named(name: &str) -> Node {
    Node::Named(id(name))
}

fn column(table: &str, column: &str) -> Node {
    Node::Column {
        table: id(table),
        column: id(column),
    }
}

fn read(handle: &PropertyHandle) -> f64 {
    match handle.value().unwrap() {
        Value::Number(number) => number,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// A formula standing in for a model: it reads through the sample it was given.
struct Reader {
    source: PropertyHandle,
    calls: Cell<usize>,
}

impl Reader {
    fn of(handle: PropertyHandle) -> Rc<Reader> {
        Rc::new(Reader {
            source: handle,
            calls: Cell::new(0),
        })
    }
}

impl Compute for Reader {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        match self.source.value()? {
            Value::Number(number) => Ok(Value::number(number * 2.0).unwrap()),
            other => Ok(other),
        }
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

struct Raising;

impl Compute for Raising {
    fn compute(&self) -> Result<Value, ComputeError> {
        Err(ComputeError::failed(Boom))
    }
}

/// A formula that answers the same number every time.
struct Constant(f64);

impl Compute for Constant {
    fn compute(&self) -> Result<Value, ComputeError> {
        Ok(Value::number(self.0).unwrap())
    }
}

struct RaisingRow;

impl ComputeRow for RaisingRow {
    fn compute(
        &self,
        _: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        Err(ComputeError::failed(Boom))
    }
}

/// `clarity = 1 / wort`, filled per row.
struct Reciprocal;

impl ComputeRow for Reciprocal {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        let wort = match row.cell(&id("wort")).unwrap().value().unwrap() {
            Value::Number(number) => number,
            other => panic!("{other:?}"),
        };
        let mut out = IndexMap::new();
        out.insert(id("clarity"), CellOutput::Value(number(1.0 / wort)));
        Ok(out)
    }
}

fn columns(names: &[&str]) -> IndexMap<Identifier, ColumnMeta> {
    names
        .iter()
        .map(|name| (id(name), ColumnMeta::default()))
        .collect()
}

fn boil(derivations: Vec<Derivation>) -> Table {
    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature", "wort", "clarity"]),
        derivations,
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
    table
}

/// malt -> plato -> haze, each doubling the last.
fn chain() -> Sample {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    let malt = sample.property(&id("malt")).unwrap();
    sample
        .set_property(id("plato"), Property::computed(Reader::of(malt)))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[named("malt")])
        .unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    sample
        .set_property(id("haze"), Property::computed(Reader::of(plato)))
        .unwrap();
    sample
        .declare_dependencies(&id("haze"), &[named("plato")])
        .unwrap();
    sample
}

// -------------------------------------------------------------- properties

#[test]
fn property_round_trips_by_name() {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(12.5)))
        .unwrap();
    assert_eq!(read(&sample.property(&id("malt")).unwrap()), 12.5);
    assert!(sample.has_property(&id("malt")));
    assert_eq!(
        read(&sample.property(&id("malt")).unwrap()),
        read(&sample.property(&id("malt")).unwrap())
    );
}

#[test]
fn declaration_order_is_preserved() {
    let mut sample = Sample::new();
    for name in ["zeta", "alpha", "mu"] {
        sample
            .set_property(id(name), Property::stored(number(1.0)))
            .unwrap();
    }
    let order: Vec<String> = sample
        .property_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    assert_eq!(order, ["zeta", "alpha", "mu"], "not sorted");
}

#[test]
fn unknown_property_lists_available_names() {
    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::stored(number(1.0)))
        .unwrap();
    let error = sample.property(&id("plto")).unwrap_err();
    let SampleError::UnknownProperty {
        available,
        suggestion,
        ..
    } = &error
    else {
        panic!("expected an unknown property, got {error:?}");
    };
    assert_eq!(available.len(), 1);
    assert_eq!(
        suggestion.as_ref().map(|n| n.to_string()),
        Some("plato".to_string())
    );
}

// ------------------------------------------------------------ namespace

#[test]
fn property_and_table_cannot_share_a_name() {
    // Rejected in both orders of creation.
    let mut one = Sample::new();
    one.set_property(id("mashing"), Property::stored(number(1.0)))
        .unwrap();
    assert_eq!(
        one.set_table(id("mashing"), boil(Vec::new())).unwrap_err(),
        SampleError::NameCollision {
            name: id("mashing"),
            existing: NameKind::Property
        }
    );

    let mut other = Sample::new();
    other.set_table(id("mashing"), boil(Vec::new())).unwrap();
    assert_eq!(
        other
            .set_property(id("mashing"), Property::stored(number(1.0)))
            .unwrap_err(),
        SampleError::NameCollision {
            name: id("mashing"),
            existing: NameKind::Table
        }
    );
}

#[test]
fn attribute_and_property_cannot_share_a_name() {
    let mut sample = Sample::new();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    let error = sample
        .set_property(id("batch"), Property::stored(number(1.0)))
        .unwrap_err();
    assert_eq!(
        error,
        SampleError::NameCollision {
            name: id("batch"),
            existing: NameKind::Attribute
        }
    );
    // And the error says which kind holds it.
    assert!(error.to_string().contains("attribute"), "{error}");

    let mut other = Sample::new();
    other
        .set_property(id("batch"), Property::stored(number(1.0)))
        .unwrap();
    assert!(other.set_attribute(id("batch"), Value::integer(3)).is_err());
}

#[test]
fn a_reserved_name_is_refused() {
    // Taken before any map is consulted.
    let mut sample = Sample::new();
    for reserved in ["name", "tags", "path", "filename"] {
        assert_eq!(
            sample
                .set_property(id(reserved), Property::stored(number(1.0)))
                .unwrap_err(),
            SampleError::NameCollision {
                name: id(reserved),
                existing: NameKind::Reserved
            },
            "{reserved}"
        );
        assert!(
            sample
                .set_attribute(id(reserved), Value::integer(1))
                .is_err()
        );
    }
}

#[test]
fn a_leading_underscore_is_refused_for_every_field_kind() {
    let mut sample = Sample::new();
    for error in [
        sample
            .set_property(id("_malt"), Property::stored(number(1.0)))
            .unwrap_err(),
        sample
            .set_attribute(id("_batch"), Value::integer(1))
            .unwrap_err(),
    ] {
        assert!(matches!(error, SampleError::PrivateName { .. }));
        assert!(
            error.to_string().contains("private Python state"),
            "{error}"
        );
    }
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature"]),
        Vec::new(),
    )
    .unwrap();
    assert!(matches!(
        sample.set_table(id("_mashing"), table),
        Err(SampleError::PrivateName { .. })
    ));
}

// ----------------------------------------------------------- invalidation

#[test]
fn value_change_invalidates_dependents() {
    // Without any explicit invalidation call.
    let sample = chain();
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 20.0);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(50.0));
    assert_eq!(read(&plato), 100.0);
}

#[test]
fn value_change_invalidates_transitively() {
    // Three levels deep.
    let sample = chain();
    let haze = sample.property(&id("haze")).unwrap();
    assert_eq!(read(&haze), 40.0);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(50.0));
    assert_eq!(read(&haze), 200.0);
}

#[test]
fn an_unread_intermediate_does_not_hide_a_change() {
    // `malt` changes, nobody reads `plato`, and reading `haze` still
    // recomputes — the case a non-recursive check answers wrongly.
    let sample = chain();
    let haze = sample.property(&id("haze")).unwrap();
    assert_eq!(read(&haze), 40.0);
    sample.property(&id("malt")).unwrap().set_value(number(3.0));
    // Nothing reads plato here, deliberately.
    assert_eq!(read(&haze), 12.0);
}

#[test]
fn presentation_change_invalidates_nothing() {
    // A unit correction leaves caches warm.
    let sample = chain();
    let malt = sample.property(&id("malt")).unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 20.0);
    assert!(plato.is_resolved());
    malt.set_presentation(Presentation {
        unit: Some("g".to_string()),
        symbol: None,
        precision: None,
    });
    assert!(plato.is_resolved(), "a unit correction invalidated a value");
}

#[test]
fn mutation_through_a_shared_handle_invalidates() {
    // A handle obtained earlier still triggers invalidation.
    let sample = chain();
    let early = sample.property(&id("malt")).unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 20.0);
    early.set_value(number(7.0));
    assert_eq!(read(&plato), 14.0);
}

#[test]
fn replacing_a_property_keeps_existing_handles_live() {
    // A handle taken before replacement observes the new value.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    let early = sample.property(&id("malt")).unwrap();
    sample
        .set_property(id("malt"), Property::stored(number(99.0)))
        .unwrap();
    assert_eq!(read(&early), 99.0, "the handle became a detached copy");
}

#[test]
fn changing_an_attribute_invalidates_its_dependents() {
    // An input like a malt, through a different mutation path.
    let mut sample = Sample::new();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    sample
        .set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    let malt = sample.property(&id("malt")).unwrap();
    sample
        .set_property(id("plato"), Property::computed(Reader::of(malt)))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[named("malt"), named("batch")])
        .unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    assert!(!plato.is_resolved());
    assert_eq!(read(&plato), 20.0);
    assert!(plato.is_resolved());

    sample
        .set_attribute(id("batch"), Value::integer(8))
        .unwrap();
    assert!(
        !plato.is_resolved(),
        "an attribute change left its dependent current"
    );
}

// ---------------------------------------------------------------- edges

#[test]
fn cycle_declaration_is_rejected() {
    let mut sample = chain();
    let error = sample
        .declare_dependencies(&id("malt"), &[named("haze")])
        .unwrap_err();
    let SampleError::Dependency(inner) = &error else {
        panic!("expected a dependency error, got {error:?}");
    };
    assert!(inner.to_string().contains("malt"), "{inner}");
    assert!(error.to_string().contains('\u{2192}'), "{error}");
}

#[test]
fn removing_a_property_removes_its_edges() {
    // A later change to a former input affects nothing.
    let mut sample = chain();
    sample.remove_property(&id("plato")).unwrap();
    assert!(sample.dependents_of(&id("malt")).unwrap().is_empty());
    assert!(
        sample
            .affected_by_change(&named("malt"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn removing_an_attribute_removes_its_edges() {
    let mut sample = Sample::new();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    sample
        .set_property(id("label"), Property::stored(Value::text("x")))
        .unwrap();
    sample
        .declare_dependencies(&id("label"), &[named("batch")])
        .unwrap();
    assert_eq!(
        sample.dependents_of(&id("batch")).unwrap(),
        [named("label")]
    );
    sample.remove_attribute(&id("batch")).unwrap();
    assert!(sample.dependents_of(&id("batch")).unwrap().is_empty());
}

#[test]
fn removing_an_input_leaves_its_dependents_broken() {
    // The record still names it, rather than being silently pruned.
    let mut sample = chain();
    let plato = sample.property(&id("plato")).unwrap();
    let mut computed = IndexMap::new();
    computed.insert(
        InputName::Named(id("malt")),
        InputRecord::Digest(Fingerprint::new("a91f42c8")),
    );
    plato.set_records(Records {
        failure: None,
        fingerprint: Some(Fingerprint::new("5b2c9e1f")),
        computed: Some(computed),
        produced: None,
        channel_only: Default::default(),
        statistics: None,
    });
    sample.remove_property(&id("malt")).unwrap();
    let still = plato.records();
    assert!(
        still
            .computed
            .unwrap()
            .contains_key(&InputName::Named(id("malt"))),
        "the record was pruned"
    );
}

#[test]
fn a_table_declaration_joins_the_sample_graph() {
    // Setting a table makes its columns reachable from what they read.
    let mut sample = Sample::new();
    sample
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature", "wort", "clarity"]),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort")), InputName::Named(id("foam"))],
            formula: Rc::new(Reciprocal),
        }],
    )
    .unwrap();
    sample.set_table(id("mashing"), table).unwrap();
    assert_eq!(
        sample.affected_by_change(&named("foam")).unwrap(),
        [column("mashing", "clarity")]
    );
}

#[test]
fn a_cycle_through_a_column_is_rejected() {
    // A property reading a column that reads that property.
    let mut sample = Sample::new();
    sample
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature", "wort", "clarity"]),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Named(id("foam"))],
            formula: Rc::new(Reciprocal),
        }],
    )
    .unwrap();
    sample.set_table(id("mashing"), table).unwrap();
    let error = sample
        .declare_dependencies(&id("foam"), &[column("mashing", "clarity")])
        .unwrap_err();
    let SampleError::Dependency(inner) = &error else {
        panic!("expected a dependency error, got {error:?}");
    };
    let drawn = inner.to_string();
    assert!(drawn.contains("mashing.clarity"), "{drawn}");
    assert!(drawn.contains("foam"), "{drawn}");
}

#[test]
fn removing_a_table_removes_its_column_edges() {
    let mut sample = Sample::new();
    sample
        .set_property(id("foam"), Property::stored(number(2.0)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        columns(&["temperature", "wort", "clarity"]),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Named(id("foam"))],
            formula: Rc::new(Reciprocal),
        }],
    )
    .unwrap();
    sample.set_table(id("mashing"), table).unwrap();
    sample.remove_table(&id("mashing")).unwrap();
    assert!(
        sample
            .affected_by_change(&named("foam"))
            .unwrap()
            .is_empty(),
        "a dangling column node survived"
    );
}

// --------------------------------------------------------- materialization

#[test]
fn materialize_resolves_every_lazy_value() {
    let mut sample = chain();
    sample.materialize().unwrap();
    for name in ["malt", "plato", "haze"] {
        let handle = sample.property(&id(name)).unwrap();
        assert!(!handle.is_computed(), "{name} is still lazy");
    }
    assert_eq!(read(&sample.property(&id("haze")).unwrap()), 40.0);
}

#[test]
fn materialize_resolves_an_uncertainty_beside_a_stored_value() {
    // A sample materialized on the value alone reaches the file with an
    // uncertainty missing and nothing said about it.
    let mut sample = Sample::new();
    let mut malt = Property::stored(Value::number(12.5).unwrap());
    malt.set_uncertainty_formula(Rc::new(Constant(0.05)));
    sample.set_property(id("malt"), malt).unwrap();

    sample.materialize().unwrap();
    let handle = sample.property(&id("malt")).unwrap();
    assert!(handle.is_resolved(), "the formula never ran");
    assert_eq!(
        handle.uncertainty().unwrap().map(|u| u.magnitude()),
        Some(0.05)
    );
}

#[test]
fn materialize_reports_which_property_failed() {
    let mut sample = Sample::new();
    sample
        .set_property(id("plato"), Property::computed(Rc::new(Raising)))
        .unwrap();
    let error = sample.materialize().unwrap_err();
    let SampleError::Compute { property, .. } = &error else {
        panic!("expected a compute failure, got {error:?}");
    };
    assert_eq!(property, &id("plato"));
    assert!(error.to_string().contains("plato"), "{error}");
}

#[test]
fn materialize_reports_which_cell_failed() {
    // And which row of which column, by index.
    let mut sample = Sample::new();
    sample
        .set_table(
            id("mashing"),
            boil(vec![Derivation::Row {
                outputs: vec![id("clarity")],
                inputs: vec![InputName::Cell(id("wort"))],
                formula: Rc::new(RaisingRow),
            }]),
        )
        .unwrap();
    let error = sample.materialize().unwrap_err();
    let SampleError::ComputeCell {
        table,
        column,
        index,
        ..
    } = &error
    else {
        panic!("expected a cell failure, got {error:?}");
    };
    assert_eq!(table, &id("mashing"));
    assert_eq!(column, &id("clarity"));
    assert_eq!(index.as_deref(), Some(&[number(65.0)][..]));
}

// ------------------------------------------------------------------ note

#[test]
fn note_survives_a_modify_cycle() {
    let mut sample = Sample::new();
    sample.set_note("Cut from the left edge.\n".to_string());
    sample
        .set_property(id("malt"), Property::stored(number(1.0)))
        .unwrap();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    sample.add_tag(id("reference"));
    sample.materialize().unwrap();
    assert_eq!(sample.note(), "Cut from the left edge.\n");
}

#[test]
fn note_with_unusual_whitespace_is_preserved() {
    // Trailing spaces, CRLF and a missing final newline all survive.
    let awkward = "First line.  \r\n\r\n---\r\n\ttabbed   ";
    let mut sample = Sample::new();
    sample.set_note(awkward.to_string());
    sample
        .set_property(id("malt"), Property::stored(number(1.0)))
        .unwrap();
    assert_eq!(sample.note(), awkward);
    assert_eq!(sample.note().len(), awkward.len());
}

// ---------------------------------------------------- attributes and tags

#[test]
fn attribute_round_trips_by_name() {
    let mut sample = Sample::new();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    assert_eq!(
        sample.attribute(&id("batch")).unwrap(),
        &AttributeValue::scalar(Value::integer(3))
    );
    assert!(sample.has_attribute(&id("batch")));
    assert_eq!(
        sample.remove_attribute(&id("batch")).unwrap(),
        AttributeValue::scalar(Value::integer(3))
    );
    assert!(!sample.has_attribute(&id("batch")));
}

#[test]
fn a_list_attribute_keeps_order_and_duplicates() {
    let values = vec![Value::integer(20), Value::integer(30), Value::integer(20)];
    let list = AttributeValue::list(values.clone()).unwrap();
    assert_eq!(list.as_list(), Some(values.as_slice()));

    let mut sample = Sample::new();
    sample.set_attribute(id("temperatures"), list).unwrap();
    assert_eq!(
        sample.attribute(&id("temperatures")).unwrap().as_list(),
        Some(values.as_slice())
    );
}

#[test]
fn a_list_attribute_is_available_to_computation_scope() {
    let mut sample = Sample::new();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(vec![Value::integer(20), Value::integer(30)]).unwrap(),
        )
        .unwrap();
    assert_eq!(
        Scope::read_list(&sample, &id("temperatures")).unwrap(),
        [Value::integer(20), Value::integer(30)]
    );
    assert!(Scope::read(&sample, &id("temperatures")).is_err());
}

#[test]
fn a_list_attribute_is_homogeneous() {
    assert!(AttributeValue::list(vec![Value::integer(1), number(2.5)]).is_ok());
    assert!(matches!(
        AttributeValue::list(vec![Value::integer(1), Value::text("two")]),
        Err(AttributeError::MixedKinds { position: 1, .. })
    ));
    assert_eq!(
        AttributeValue::list(vec![Value::integer(1), Value::absent()]),
        Err(AttributeError::AbsentItem { position: 1 })
    );
}

#[test]
fn an_empty_or_absent_attribute_stays_in_memory() {
    let mut sample = Sample::new();
    sample
        .set_attribute(id("comment"), Value::absent())
        .unwrap();
    sample
        .set_attribute(
            id("temperatures"),
            AttributeValue::list(Vec::new()).unwrap(),
        )
        .unwrap();
    assert!(sample.has_attribute(&id("comment")));
    assert!(sample.attribute(&id("comment")).unwrap().is_empty());
    assert!(sample.has_attribute(&id("temperatures")));
    assert!(sample.attribute(&id("temperatures")).unwrap().is_empty());
}

#[test]
fn attributes_keep_declaration_order() {
    let mut sample = Sample::new();
    for name in ["zeta", "alpha", "mu"] {
        sample.set_attribute(id(name), Value::integer(1)).unwrap();
    }
    let order: Vec<String> = sample
        .attribute_names()
        .iter()
        .map(|name| name.to_string())
        .collect();
    assert_eq!(order, ["zeta", "alpha", "mu"]);
}

#[test]
fn unknown_attribute_lists_available_names() {
    let mut sample = Sample::new();
    sample
        .set_attribute(id("batch"), Value::integer(3))
        .unwrap();
    let error = sample.attribute(&id("batche")).unwrap_err();
    let SampleError::UnknownAttribute {
        available,
        suggestion,
        ..
    } = &error
    else {
        panic!("expected an unknown attribute, got {error:?}");
    };
    assert_eq!(available, &[id("batch")]);
    assert_eq!(
        suggestion.as_ref().map(|n| n.to_string()),
        Some("batch".to_string())
    );
}

#[test]
fn a_duplicate_tag_is_not_added_twice() {
    let mut sample = Sample::new();
    assert!(sample.add_tag(id("reference")));
    assert!(!sample.add_tag(id("reference")), "it reported adding again");
    assert_eq!(sample.tags(), [id("reference")]);
    assert!(sample.remove_tag(&id("reference")));
    assert!(!sample.remove_tag(&id("reference")));
}

#[test]
fn tags_keep_the_order_they_were_added() {
    // Not sorted, because canonicalization writes them back as written.
    let mut sample = Sample::new();
    sample.set_tags(vec![id("zeta"), id("alpha"), id("zeta"), id("mu")]);
    assert_eq!(sample.tags(), [id("zeta"), id("alpha"), id("mu")]);
    assert!(sample.has_tag(&id("alpha")));
}

// ----------------------------------------------------------------- filling

/// plato = malt / volume, counting its runs.
struct FillDensity {
    malt: PropertyHandle,
    volume: PropertyHandle,
    runs: Cell<usize>,
}

impl Compute for FillDensity {
    fn compute(&self) -> Result<Value, ComputeError> {
        self.runs.set(self.runs.get() + 1);
        Ok(number(read(&self.malt) / read(&self.volume)))
    }
}

/// A model's declaration: two inputs without values, and a plato with a
/// formula and a unit.
fn fill_declaration() -> (Sample, Rc<FillDensity>) {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(Value::absent()))
        .unwrap();
    sample
        .set_property(id("volume"), Property::stored(Value::absent()))
        .unwrap();
    let formula = Rc::new(FillDensity {
        malt: sample.property(&id("malt")).unwrap(),
        volume: sample.property(&id("volume")).unwrap(),
        runs: Cell::new(0),
    });
    let mut plato = Property::computed(formula.clone());
    plato.set_presentation(Presentation {
        unit: Some("g/L".to_string()),
        ..Presentation::default()
    });
    sample.set_property(id("plato"), plato).unwrap();
    sample
        .declare_dependencies(&id("plato"), &[named("malt"), named("volume")])
        .unwrap();
    (sample, formula)
}

/// What a file read on its own holds: values, a name and a note, no formula.
fn fill_file() -> Sample {
    let mut file = Sample::new();
    file.set_name(Some("Keg 42".to_string()));
    file.set_property(id("malt"), Property::stored(number(12.4)))
        .unwrap();
    file.set_property(id("volume"), Property::stored(number(4.0)))
        .unwrap();
    // Not 12.4 / 4.0, so that a run would show.
    file.set_property(id("plato"), Property::stored(number(3.0)))
        .unwrap();
    file.set_note("# Notes\n".to_string());
    file
}

#[test]
fn filling_reads_the_file_into_a_declaration() {
    let (mut sample, formula) = fill_declaration();
    sample.fill_from(fill_file()).unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 3.0);
    assert_eq!(formula.runs.get(), 0);
    assert!(plato.is_computed());
    assert!(plato.is_resolved());
    assert_eq!(plato.presentation().unit.as_deref(), Some("g/L"));
    assert_eq!(sample.name(), Some("Keg 42"));
    assert_eq!(sample.note(), "# Notes\n");
}

#[test]
fn a_filled_value_recomputes_when_an_input_changes() {
    let (mut sample, formula) = fill_declaration();
    sample.fill_from(fill_file()).unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 3.0);
    sample.property(&id("malt")).unwrap().set_value(number(8.0));
    assert_eq!(read(&plato), 2.0);
    assert_eq!(formula.runs.get(), 1);
}

#[test]
fn filling_keeps_what_only_the_declaration_holds() {
    let (mut sample, _) = fill_declaration();
    sample
        .set_attribute(id("batch"), Value::integer(7))
        .unwrap();
    sample
        .set_property(id("haze"), Property::stored(number(0.1)))
        .unwrap();
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        [(id("temperature"), ColumnMeta::default())]
            .into_iter()
            .collect(),
        Vec::new(),
    )
    .unwrap();
    sample.set_table(id("mashing"), table).unwrap();
    sample.fill_from(fill_file()).unwrap();
    assert!(*sample.attribute(&id("batch")).unwrap() == Value::integer(7));
    assert_eq!(read(&sample.property(&id("haze")).unwrap()), 0.1);
    assert!(sample.table(&id("mashing")).is_ok());
    assert_eq!(read(&sample.property(&id("malt")).unwrap()), 12.4);
}

#[test]
fn filling_refuses_a_name_held_as_another_kind() {
    let (mut sample, _) = fill_declaration();
    sample
        .set_attribute(id("batch"), Value::integer(7))
        .unwrap();
    let mut file = fill_file();
    file.set_property(id("batch"), Property::stored(number(7.0)))
        .unwrap();
    let refused = sample.fill_from(file);
    assert_eq!(
        refused,
        Err(SampleError::NameCollision {
            name: id("batch"),
            existing: NameKind::Attribute,
        })
    );
    assert!(!sample.has_property(&id("batch")));
    assert!(
        sample
            .property(&id("malt"))
            .unwrap()
            .value()
            .unwrap()
            .is_absent()
    );
}

#[test]
fn invalidating_a_handle_recomputes_its_dependents() {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(2.0)))
        .unwrap();
    let first = Reader::of(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("double"), Property::computed(first.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[named("malt")])
        .unwrap();
    let second = Reader::of(sample.property(&id("double")).unwrap());
    sample
        .set_property(id("quadruple"), Property::computed(second.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("quadruple"), &[named("double")])
        .unwrap();
    let quadruple = sample.property(&id("quadruple")).unwrap();
    assert_eq!(read(&quadruple), 8.0);

    sample.property(&id("double")).unwrap().invalidate();
    assert_eq!(read(&quadruple), 8.0);
    assert_eq!(first.calls.get(), 2);
    assert_eq!(second.calls.get(), 2);
}

// ------------------------------------------------------ naming and resolving

/// Every cell of a column halved, so a column span has something to declare.
struct FillHalves;

impl samplekit::core::table::ComputeColumn for FillHalves {
    fn compute(
        &self,
        columns: &samplekit::core::table::ColumnSet,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        let cells = columns
            .column(&id("wort"))
            .unwrap()
            .cells()
            .map(|(_, cell)| match cell.value().unwrap() {
                Value::Number(number) => CellOutput::Value(number_value(number / 2.0)),
                other => CellOutput::Value(other),
            })
            .collect();
        Ok([(id("halved"), cells)].into_iter().collect())
    }
}

fn number_value(x: f64) -> Value {
    Value::number(x).unwrap()
}

#[test]
fn a_table_is_named_by_the_name_it_is_set_under() {
    let table = Table::new(
        id("draft"),
        vec![id("temperature")],
        [
            (id("temperature"), ColumnMeta::default()),
            (id("wort"), ColumnMeta::default()),
            (id("halved"), ColumnMeta::default()),
        ]
        .into_iter()
        .collect(),
        vec![Derivation::Column {
            outputs: vec![id("halved")],
            inputs: vec![InputName::Column {
                table: id("draft"),
                column: id("wort"),
            }],
            formula: Rc::new(FillHalves),
        }],
    )
    .unwrap();
    let mut sample = Sample::new();
    sample.set_table(id("mashing"), table).unwrap();

    let held = sample.table(&id("mashing")).unwrap();
    assert_eq!(held.name(), &id("mashing"));
    assert_eq!(
        held.inputs_of(&id("halved")).unwrap(),
        &[InputName::Column {
            table: id("mashing"),
            column: id("wort"),
        }]
    );
    assert!(
        sample
            .affected_by_change(&column("mashing", "wort"))
            .unwrap()
            .contains(&column("mashing", "halved"))
    );
}

#[test]
fn resolve_keeps_the_formulas() {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(2.0)))
        .unwrap();
    let formula = Reader::of(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("double"), Property::computed(formula.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[named("malt")])
        .unwrap();

    sample.resolve().unwrap();
    let double = sample.property(&id("double")).unwrap();
    assert!(double.is_computed());
    assert!(double.is_resolved());
    assert_eq!(formula.calls.get(), 1);

    sample.property(&id("malt")).unwrap().set_value(number(3.0));
    assert_eq!(read(&double), 6.0);
    assert_eq!(formula.calls.get(), 2);
}

/// clarity = 1 / wort, counting its runs.
struct FillRowCount {
    runs: Cell<usize>,
}

impl ComputeRow for FillRowCount {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.runs.set(self.runs.get() + 1);
        let wort = match row.cell(&id("wort")).unwrap().value().unwrap() {
            Value::Number(number) => number,
            other => panic!("expected a number, got {other:?}"),
        };
        Ok(
            [(id("clarity"), CellOutput::Value(number_value(1.0 / wort)))]
                .into_iter()
                .collect(),
        )
    }
}

#[test]
fn resolving_a_table_runs_only_its_derivations() {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(2.0)))
        .unwrap();
    let double = Reader::of(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("double"), Property::computed(double.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[named("malt")])
        .unwrap();

    let rows = Rc::new(FillRowCount { runs: Cell::new(0) });
    let mut table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        [
            (id("temperature"), ColumnMeta::default()),
            (id("wort"), ColumnMeta::default()),
            (id("clarity"), ColumnMeta::default()),
        ]
        .into_iter()
        .collect(),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort"))],
            formula: rows.clone(),
        }],
    )
    .unwrap();
    table
        .add_row(vec![
            (id("temperature"), Property::stored(number(65.0))),
            (id("wort"), Property::stored(number(12.5))),
        ])
        .unwrap();
    sample.set_table(id("mashing"), table).unwrap();

    sample.resolve_table(&id("mashing")).unwrap();
    assert_eq!(rows.runs.get(), 1);
    assert_eq!(double.calls.get(), 0);

    // Nothing to run the second time.
    sample.resolve_table(&id("mashing")).unwrap();
    assert_eq!(rows.runs.get(), 1);
}

#[test]
fn a_held_table_is_edited_through_its_sample() {
    use samplekit::core::table::RowAddress;
    let rows = Rc::new(FillRowCount { runs: Cell::new(0) });
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        [
            (id("temperature"), ColumnMeta::default()),
            (id("wort"), ColumnMeta::default()),
            (id("clarity"), ColumnMeta::default()),
        ]
        .into_iter()
        .collect(),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort"))],
            formula: rows.clone(),
        }],
    )
    .unwrap();
    let mut sample = Sample::new();
    sample.set_table(id("mashing"), table).unwrap();
    sample
        .add_row(
            &id("mashing"),
            vec![
                (id("temperature"), Property::stored(number(65.0))),
                (id("wort"), Property::stored(number(12.5))),
            ],
        )
        .unwrap();
    sample.resolve_table(&id("mashing")).unwrap();
    assert_eq!(rows.runs.get(), 1);

    let at = RowAddress::index(vec![number(65.0)]);
    sample
        .update_row(
            &id("mashing"),
            &at,
            vec![(id("wort"), Property::stored(number(25.0)))],
        )
        .unwrap();
    sample.resolve_table(&id("mashing")).unwrap();
    assert_eq!(rows.runs.get(), 2);
    let clarity = sample
        .table(&id("mashing"))
        .unwrap()
        .at(&at, &id("clarity"))
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(clarity, number(1.0 / 25.0));

    sample.invalidate_row(&id("mashing"), &at).unwrap();
    sample.resolve_table(&id("mashing")).unwrap();
    assert_eq!(rows.runs.get(), 3);
}

// ---------------------------------------------------------- peeking, runs

fn doubling() -> (Sample, Rc<Reader>) {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(2.0)))
        .unwrap();
    let formula = Reader::of(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("double"), Property::computed(formula.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("double"), &[named("malt")])
        .unwrap();
    (sample, formula)
}

#[test]
fn peeking_a_handle_discards_no_stale_cache() {
    let (sample, formula) = doubling();
    let double = sample.property(&id("double")).unwrap();
    assert_eq!(read(&double), 4.0);
    sample.property(&id("malt")).unwrap().set_value(number(3.0));
    assert!(!double.is_current());
    assert_eq!(double.peek(Property::peek_value), Some(number(4.0)));
    assert!(!double.is_current());
    assert_eq!(formula.calls.get(), 1);
    assert_eq!(read(&double), 6.0);
    assert!(double.is_current());
}

#[test]
fn marking_a_handle_current_stops_a_rerun() {
    let (sample, formula) = doubling();
    let double = sample.property(&id("double")).unwrap();
    assert_eq!(read(&double), 4.0);
    sample.property(&id("malt")).unwrap().set_value(number(2.0));
    assert!(!double.is_current());
    double.mark_current();
    assert!(double.is_current());
    assert_eq!(read(&double), 4.0);
    assert_eq!(formula.calls.get(), 1);
}

#[test]
fn a_held_cell_reports_its_run_and_takes_its_records() {
    use samplekit::core::table::{CellRun, RowAddress};
    let rows = Rc::new(FillRowCount { runs: Cell::new(0) });
    let table = Table::new(
        id("mashing"),
        vec![id("temperature")],
        [
            (id("temperature"), ColumnMeta::default()),
            (id("wort"), ColumnMeta::default()),
            (id("clarity"), ColumnMeta::default()),
        ]
        .into_iter()
        .collect(),
        vec![Derivation::Row {
            outputs: vec![id("clarity")],
            inputs: vec![InputName::Cell(id("wort"))],
            formula: rows.clone(),
        }],
    )
    .unwrap();
    let mut sample = Sample::new();
    sample.set_table(id("mashing"), table).unwrap();
    sample
        .add_row(
            &id("mashing"),
            vec![
                (id("temperature"), Property::stored(number(65.0))),
                (id("wort"), Property::stored(number(12.5))),
            ],
        )
        .unwrap();
    let at = RowAddress::index(vec![number(65.0)]);
    let (mashing, clarity) = (id("mashing"), id("clarity"));
    let run = |sample: &Sample| sample.cell_run(&mashing, &at, &clarity).unwrap();

    assert_eq!(run(&sample), CellRun::NeverRan);
    assert_eq!(
        sample.cell_run(&mashing, &at, &id("wort")).unwrap(),
        CellRun::NotDerived
    );
    sample.resolve_table(&mashing).unwrap();
    assert_eq!(run(&sample), CellRun::Current);
    sample
        .update_row(
            &mashing,
            &at,
            vec![(id("wort"), Property::stored(number(25.0)))],
        )
        .unwrap();
    assert_eq!(run(&sample), CellRun::Moved);
    sample.confirm_cell(&mashing, &at, &clarity).unwrap();
    assert_eq!(run(&sample), CellRun::Current);

    let records = Records {
        failure: None,
        fingerprint: Some(Fingerprint::new("0123456789ab")),
        computed: None,
        produced: None,
        channel_only: Default::default(),
        statistics: None,
    };
    sample
        .set_cell_records(&mashing, &at, &clarity, records.clone())
        .unwrap();
    let held = sample.table(&mashing).unwrap();
    assert_eq!(held.at(&at, &clarity).unwrap().records(), &records);
    assert_eq!(run(&sample), CellRun::Current);
    sample.resolve_table(&mashing).unwrap();
    assert_eq!(rows.runs.get(), 1);
}

// ------------------------------------------------------------- overrides

/// malt -> plato -> haze, each doubling the last, with their formulas.
fn doubled() -> (Sample, Rc<Reader>, Rc<Reader>) {
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    let plato = Reader::of(sample.property(&id("malt")).unwrap());
    sample
        .set_property(id("plato"), Property::computed(plato.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[named("malt")])
        .unwrap();
    let haze = Reader::of(sample.property(&id("plato")).unwrap());
    sample
        .set_property(id("haze"), Property::computed(haze.clone()))
        .unwrap();
    sample
        .declare_dependencies(&id("haze"), &[named("plato")])
        .unwrap();
    (sample, plato, haze)
}

#[test]
fn an_override_moves_what_reads_it() {
    let (sample, plato_formula, haze_formula) = doubled();
    let plato = sample.property(&id("plato")).unwrap();
    let haze = sample.property(&id("haze")).unwrap();
    assert_eq!(read(&haze), 40.0);

    plato.set_value(number(7.0));
    assert!(plato.is_edited());
    assert_eq!(read(&haze), 14.0);
    assert_eq!(read(&haze), 14.0);
    assert_eq!(
        (plato_formula.calls.get(), haze_formula.calls.get()),
        (1, 2)
    );

    plato.restore_formula();
    assert!(!plato.is_edited());
    assert_eq!(read(&haze), 40.0);
    assert_eq!(
        (plato_formula.calls.get(), haze_formula.calls.get()),
        (2, 3)
    );
}

#[test]
fn an_override_stops_currency_at_itself() {
    let (sample, plato_formula, haze_formula) = doubled();
    let plato = sample.property(&id("plato")).unwrap();
    let haze = sample.property(&id("haze")).unwrap();
    plato.set_value(number(7.0));
    assert_eq!(read(&haze), 14.0);

    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert!(plato.is_current());
    assert!(haze.is_current());
    assert_eq!(read(&haze), 14.0);
    assert_eq!(
        (plato_formula.calls.get(), haze_formula.calls.get()),
        (0, 1)
    );
}

#[test]
fn a_value_reading_a_derived_column_is_current_once_read() {
    let mut sample = Sample::new();
    let table = boil(vec![Derivation::Row {
        outputs: vec![id("clarity")],
        inputs: vec![InputName::Cell(id("wort"))],
        formula: Rc::new(Reciprocal),
    }]);
    sample.set_table(id("mashing"), table).unwrap();
    sample
        .set_property(id("mean"), Property::computed(Rc::new(Constant(1.0))))
        .unwrap();
    sample
        .declare_dependencies(&id("mean"), &[column("mashing", "clarity")])
        .unwrap();
    sample.resolve_table(&id("mashing")).unwrap();
    let mean = sample.property(&id("mean")).unwrap();
    read(&mean);
    assert!(mean.is_current());
}

/// Doubles, row by row, the column `raw.f` of another table.
struct DoubledFrom {
    calls: Cell<usize>,
}

impl samplekit::core::table::ComputeColumn for DoubledFrom {
    fn compute(
        &self,
        columns: &samplekit::core::table::ColumnSet,
        scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let source = scope.read_column(&id("raw"), &id("f"))?;
        let doubled = (0..columns.len())
            .map(|at| match &source[at].1 {
                Value::Number(value) => CellOutput::Value(number(value * 2.0)),
                _ => CellOutput::Value(Value::absent()),
            })
            .collect();
        let mut out = IndexMap::new();
        out.insert(id("g"), doubled);
        Ok(out)
    }
}

fn reading_raw(formula: Rc<DoubledFrom>) -> Table {
    let mut derived = Table::new(
        id("derived"),
        vec![id("T")],
        columns(&["T", "g"]),
        vec![Derivation::Column {
            outputs: vec![id("g")],
            inputs: vec![InputName::Column {
                table: id("raw"),
                column: id("f"),
            }],
            formula,
        }],
    )
    .unwrap();
    for temperature in [20.0, 30.0] {
        derived
            .add_row(vec![(id("T"), Property::stored(number(temperature)))])
            .unwrap();
    }
    derived
}

fn g_at(sample: &Sample, temperature: f64) -> Value {
    sample
        .table(&id("derived"))
        .unwrap()
        .at(
            &samplekit::core::table::RowAddress::Index(vec![number(temperature)]),
            &id("g"),
        )
        .unwrap()
        .value()
        .unwrap()
}

#[test]
fn a_table_reads_a_column_of_another_table_resolved_first() {
    let formula = Rc::new(DoubledFrom {
        calls: Cell::new(0),
    });
    let mut sample = Sample::new();
    let mut raw = Table::new(id("raw"), vec![id("T")], columns(&["T", "f"]), Vec::new()).unwrap();
    for (temperature, f) in [(20.0, 1.0), (30.0, 2.0)] {
        raw.add_row(vec![
            (id("T"), Property::stored(number(temperature))),
            (id("f"), Property::stored(number(f))),
        ])
        .unwrap();
    }
    sample.set_table(id("raw"), raw).unwrap();
    sample
        .set_table(id("derived"), reading_raw(formula.clone()))
        .unwrap();
    sample.resolve_table(&id("derived")).unwrap();
    assert_eq!(g_at(&sample, 20.0), number(2.0));
    assert_eq!(formula.calls.get(), 1);
    // Resolved again with nothing moved, it runs nothing.
    sample.resolve_table(&id("derived")).unwrap();
    assert_eq!(formula.calls.get(), 1);
    // A cell of the column it reads moves on, and it runs again.
    sample
        .update_row(
            &id("raw"),
            &samplekit::core::table::RowAddress::Index(vec![number(20.0)]),
            vec![(id("f"), Property::stored(number(5.0)))],
        )
        .unwrap();
    sample.resolve_table(&id("derived")).unwrap();
    assert_eq!(g_at(&sample, 20.0), number(10.0));
    assert_eq!(formula.calls.get(), 2);
}

#[test]
fn a_cycle_between_tables_is_refused() {
    let mut sample = Sample::new();
    sample
        .set_table(
            id("derived"),
            reading_raw(Rc::new(DoubledFrom {
                calls: Cell::new(0),
            })),
        )
        .unwrap();
    let raw = Table::new(
        id("raw"),
        vec![id("T")],
        columns(&["T", "f"]),
        vec![Derivation::Column {
            outputs: vec![id("f")],
            inputs: vec![InputName::Column {
                table: id("derived"),
                column: id("g"),
            }],
            formula: Rc::new(DoubledFrom {
                calls: Cell::new(0),
            }),
        }],
    )
    .unwrap();
    let error = sample.set_table(id("raw"), raw).unwrap_err();
    assert!(matches!(error, SampleError::Dependency(_)), "{error:?}");
}

#[test]
fn a_column_of_a_table_the_sample_does_not_hold_is_refused_when_resolving() {
    let mut sample = Sample::new();
    sample
        .set_table(
            id("derived"),
            reading_raw(Rc::new(DoubledFrom {
                calls: Cell::new(0),
            })),
        )
        .unwrap();
    let error = sample.resolve_table(&id("derived")).unwrap_err();
    assert!(
        matches!(
            error,
            SampleError::Table(samplekit::core::table::TableError::ForeignColumn { .. })
        ),
        "{error:?}"
    );
}

#[test]
fn a_table_reading_another_loads_current_whatever_the_file_order() {
    // The file holds the reading table first: its cells are recorded once the
    // table they read is in, not before that table's fill moved its column.
    let formula = Rc::new(DoubledFrom {
        calls: Cell::new(0),
    });
    let mut declared = Sample::new();
    declared
        .set_table(
            id("raw"),
            Table::new(id("raw"), vec![id("T")], columns(&["T", "f"]), Vec::new()).unwrap(),
        )
        .unwrap();
    declared
        .set_table(
            id("derived"),
            Table::new(
                id("derived"),
                vec![id("T")],
                columns(&["T", "g"]),
                vec![Derivation::Column {
                    outputs: vec![id("g")],
                    inputs: vec![InputName::Column {
                        table: id("raw"),
                        column: id("f"),
                    }],
                    formula: formula.clone(),
                }],
            )
            .unwrap(),
        )
        .unwrap();
    let mut file = Sample::new();
    let mut derived = Table::new(
        id("derived"),
        vec![id("T")],
        columns(&["T", "g"]),
        Vec::new(),
    )
    .unwrap();
    let mut raw = Table::new(id("raw"), vec![id("T")], columns(&["T", "f"]), Vec::new()).unwrap();
    for (temperature, f) in [(20.0, 1.0), (30.0, 2.0)] {
        derived
            .add_row(vec![
                (id("T"), Property::stored(number(temperature))),
                (id("g"), Property::stored(number(2.0 * f))),
            ])
            .unwrap();
        raw.add_row(vec![
            (id("T"), Property::stored(number(temperature))),
            (id("f"), Property::stored(number(f))),
        ])
        .unwrap();
    }
    file.set_table(id("derived"), derived).unwrap();
    file.set_table(id("raw"), raw).unwrap();
    declared.fill_from(file).unwrap();
    let at = samplekit::core::table::RowAddress::Index(vec![number(20.0)]);
    let run = declared.cell_run(&id("derived"), &at, &id("g")).unwrap();
    assert!(
        matches!(run, samplekit::core::table::CellRun::Current),
        "{run:?}"
    );
    declared.resolve_table(&id("derived")).unwrap();
    assert_eq!(formula.calls.get(), 0);
}

/// `y = 2 x`, counted: a row derivation.
struct Twice {
    calls: Cell<usize>,
}

impl ComputeRow for Twice {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let x = match row.cell(&id("x")).unwrap().value().unwrap() {
            Value::Number(x) => x,
            _ => 0.0,
        };
        let mut out = IndexMap::new();
        out.insert(id("y"), CellOutput::Value(number(2.0 * x)));
        Ok(out)
    }
}

/// `z`, the sum of the column `y`, counted: a column derivation reading `y`.
struct Total {
    calls: Cell<usize>,
}

impl samplekit::core::table::ComputeColumn for Total {
    fn compute(
        &self,
        columns: &samplekit::core::table::ColumnSet,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let values: Vec<f64> = columns
            .column(&id("y"))
            .unwrap()
            .cells()
            .map(|(_, cell)| match cell.value().unwrap() {
                Value::Number(y) => y,
                _ => 0.0,
            })
            .collect();
        let sum: f64 = values.iter().sum();
        let mut out = IndexMap::new();
        out.insert(
            id("z"),
            values
                .iter()
                .map(|_| CellOutput::Value(number(sum)))
                .collect(),
        );
        Ok(out)
    }
}

fn chained(twice: &Rc<Twice>, total: &Rc<Total>) -> Sample {
    let mut table = Table::new(
        id("t"),
        vec![id("T")],
        columns(&["T", "x", "y", "z"]),
        vec![
            Derivation::Row {
                outputs: vec![id("y")],
                inputs: vec![InputName::Cell(id("x"))],
                formula: twice.clone(),
            },
            Derivation::Column {
                outputs: vec![id("z")],
                inputs: vec![InputName::Column {
                    table: id("t"),
                    column: id("y"),
                }],
                formula: total.clone(),
            },
        ],
    )
    .unwrap();
    for (temperature, x) in [(1.0, 1.0), (2.0, 3.0)] {
        table
            .add_row(vec![
                (id("T"), Property::stored(number(temperature))),
                (id("x"), Property::stored(number(x))),
            ])
            .unwrap();
    }
    let mut sample = Sample::new();
    sample.set_table(id("t"), table).unwrap();
    sample
}

fn counted() -> (Rc<Twice>, Rc<Total>) {
    (
        Rc::new(Twice {
            calls: Cell::new(0),
        }),
        Rc::new(Total {
            calls: Cell::new(0),
        }),
    )
}

#[test]
fn resolving_named_columns_runs_only_their_derivations() {
    let (twice, total) = counted();
    let mut sample = chained(&twice, &total);
    sample.resolve_table_columns(&id("t"), &[id("y")]).unwrap();
    assert_eq!((twice.calls.get(), total.calls.get()), (2, 0));
}

#[test]
fn a_column_a_named_one_reads_is_resolved_with_it() {
    let (twice, total) = counted();
    let mut sample = chained(&twice, &total);
    sample.resolve_table_columns(&id("t"), &[id("z")]).unwrap();
    assert_eq!((twice.calls.get(), total.calls.get()), (2, 1));
}

#[test]
fn invalidating_a_column_leaves_the_others_current() {
    let (twice, total) = counted();
    let mut sample = chained(&twice, &total);
    sample.resolve_table(&id("t")).unwrap();
    assert_eq!((twice.calls.get(), total.calls.get()), (2, 1));
    sample.invalidate_column(&id("t"), &id("z"), None).unwrap();
    sample.resolve_table_columns(&id("t"), &[id("z")]).unwrap();
    assert_eq!((twice.calls.get(), total.calls.get()), (2, 2));
}

#[test]
fn a_held_cell_is_not_written_by_its_derivation() {
    let (twice, total) = counted();
    let mut sample = chained(&twice, &total);
    let at = samplekit::core::table::RowAddress::Index(vec![number(1.0)]);
    sample.hold_cell(&id("t"), &at, &id("y")).unwrap();
    sample.resolve_table(&id("t")).unwrap();
    // The other row runs; the held one keeps what it had.
    assert_eq!(twice.calls.get(), 1);
    let y = sample
        .table(&id("t"))
        .unwrap()
        .at(&at, &id("y"))
        .unwrap()
        .value()
        .unwrap();
    assert!(y.is_absent(), "{y:?}");
}

#[test]
fn a_derived_cell_updated_by_hand_is_held() {
    let (twice, total) = counted();
    let mut sample = chained(&twice, &total);
    sample.resolve_table(&id("t")).unwrap();
    let at = samplekit::core::table::RowAddress::Index(vec![number(1.0)]);
    sample
        .update_row(
            &id("t"),
            &at,
            vec![(id("y"), Property::stored(number(65.0)))],
        )
        .unwrap();
    assert!(sample.is_cell_held(&id("t"), &at, &id("y")).unwrap());
    sample.invalidate_column(&id("t"), &id("y"), None).unwrap();
    sample.resolve_table(&id("t")).unwrap();
    let y = sample
        .table(&id("t"))
        .unwrap()
        .at(&at, &id("y"))
        .unwrap()
        .value()
        .unwrap();
    assert_eq!(y, number(65.0));
}

#[test]
fn replacing_a_property_drops_its_declared_inputs() {
    // The new quantity carries its own declaration; what reads the name by name
    // keeps reading it.
    let mut sample = Sample::new();
    sample
        .set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    sample
        .set_property(id("other"), Property::stored(number(3.0)))
        .unwrap();
    let malt = sample.property(&id("malt")).unwrap();
    sample
        .set_property(id("plato"), Property::computed(Reader::of(malt)))
        .unwrap();
    sample
        .declare_dependencies(&id("plato"), &[named("malt")])
        .unwrap();
    let plato = sample.property(&id("plato")).unwrap();
    sample
        .set_property(id("haze"), Property::computed(Reader::of(plato)))
        .unwrap();
    sample
        .declare_dependencies(&id("haze"), &[named("plato")])
        .unwrap();

    let other = sample.property(&id("other")).unwrap();
    sample
        .set_property(id("plato"), Property::computed(Reader::of(other)))
        .unwrap();
    assert!(
        sample.dependencies_of(&id("plato")).unwrap().is_empty(),
        "the old declaration survived the replacement"
    );
    assert!(sample.dependents_of(&id("malt")).unwrap().is_empty());
    assert_eq!(
        sample.dependents_of(&id("plato")).unwrap(),
        vec![named("haze")],
        "what read the name keeps reading it"
    );
    sample
        .declare_dependencies(&id("plato"), &[named("other")])
        .unwrap();
    assert_eq!(
        sample.dependencies_of(&id("plato")).unwrap(),
        vec![named("other")]
    );
    let plato = sample.property(&id("plato")).unwrap();
    assert_eq!(read(&plato), 6.0);
    sample
        .property(&id("malt"))
        .unwrap()
        .set_value(number(99.0));
    assert!(plato.is_current(), "a dropped input still moved the value");
    sample
        .property(&id("other"))
        .unwrap()
        .set_value(number(4.0));
    assert!(!plato.is_current());
}

#[test]
fn confirming_a_fill_records_declarations_settled_after_it() {
    // A declaration settled after the fill gives a filled value inputs it had
    // no record of; `confirm_filled` is the record it is owed.
    let mut declared = Sample::new();
    declared
        .set_property(id("malt"), Property::stored(Value::absent()))
        .unwrap();
    let malt = declared.property(&id("malt")).unwrap();
    let formula = Reader::of(malt);
    declared
        .set_property(id("plato"), Property::computed(formula.clone()))
        .unwrap();
    let mut file = Sample::new();
    file.set_property(id("malt"), Property::stored(number(10.0)))
        .unwrap();
    file.set_property(id("plato"), Property::stored(number(20.0)))
        .unwrap();
    declared.fill_from(file).unwrap();
    declared
        .declare_dependencies(&id("plato"), &[named("malt")])
        .unwrap();
    let plato = declared.property(&id("plato")).unwrap();
    assert!(
        !plato.is_current(),
        "the record taken at the fill knew nothing of the declaration"
    );
    declared.confirm_filled();
    assert!(plato.is_current());
    assert_eq!(read(&plato), 20.0, "the file's value, and no run");
    assert_eq!(formula.calls.get(), 0);
    declared
        .property(&id("malt"))
        .unwrap()
        .set_value(number(11.0));
    assert!(!plato.is_current());
}

#[test]
fn an_unknown_name_in_a_sample_holding_none_of_its_kind_says_so() {
    // *unknown table 'fermentation' / available: * stood with an empty list.
    let sample = Sample::new();
    let said = sample.table(&id("fermentation")).unwrap_err().to_string();
    assert!(said.contains("this sample holds no table"), "{said}");
    assert!(!said.contains("available"), "{said}");
}

#[test]
fn a_sample_without_a_name_is_named_by_its_file() {
    // The file's name without its extension stands for a name not written; a
    // name written wins; a sample with no file and no name has none.
    let mut sample = Sample::new();
    assert_eq!(sample.name(), None);
    sample.set_file(Some(std::path::Path::new("brews/s-1.sample.md")));
    assert_eq!(sample.name(), Some("s-1.sample"));
    assert_eq!(sample.written_name(), None);
    sample.set_name(Some("Cask A".to_string()));
    assert_eq!(sample.name(), Some("Cask A"));
    assert_eq!(sample.written_name(), Some("Cask A"));
    sample.set_name(None);
    assert_eq!(sample.name(), Some("s-1.sample"));
    // Filled from a file, the file's name comes with it.
    let mut declared = Sample::new();
    declared.fill_from(sample).unwrap();
    assert_eq!(declared.name(), Some("s-1.sample"));
    assert_eq!(declared.written_name(), None);
}
