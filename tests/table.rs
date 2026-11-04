//! The tests of `table`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::IndexMap;
use samplekit::core::formatting::Presentation;
use samplekit::core::identifier::Identifier;
use samplekit::core::property::{ComputeError, DeclaredStatistics, InputName, Property};
use samplekit::core::statistics::Location;
use samplekit::core::table::{
    CellOutput, ColumnMeta, ColumnSet, ComputeColumn, ComputeRow, Derivation, RowAddress, RowView,
    Scope, Table, TableError,
};
use samplekit::core::uncertainty::{Convention, Uncertainty};
use samplekit::core::value::{Readings, Value};

fn id(name: &str) -> Identifier {
    Identifier::new(name).unwrap()
}

fn number(x: f64) -> Value {
    Value::number(x).unwrap()
}

fn columns(names: &[&str]) -> IndexMap<Identifier, ColumnMeta> {
    names
        .iter()
        .map(|name| (id(name), ColumnMeta::default()))
        .collect()
}

fn cells(pairs: &[(&str, f64)]) -> Vec<(Identifier, Property)> {
    pairs
        .iter()
        .map(|(name, value)| (id(name), Property::stored(number(*value))))
        .collect()
}

fn by_index(values: &[f64]) -> RowAddress {
    RowAddress::Index(values.iter().copied().map(number).collect())
}

fn value_at(table: &Table, row: &RowAddress, column: &str) -> f64 {
    match table.at(row, &id(column)).unwrap().value().unwrap() {
        Value::Number(number) => number,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// A temperature-indexed soak: three rows, a tension column, and a
/// rigidity column for the derivations to fill.
fn soak() -> Table {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        Vec::new(),
    )
    .unwrap();
    for (t, r) in [(65.0, 12.5), (200.0, 15.0), (78.5, 20.0)] {
        table
            .add_row(cells(&[("temperature", t), ("tension", r)]))
            .unwrap();
    }
    table
}

fn derived(derivations: Vec<Derivation>, column_names: &[&str]) -> Table {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(column_names),
        derivations,
    )
    .unwrap();
    for (t, r) in [(65.0, 12.5), (200.0, 15.0), (78.5, 20.0)] {
        table
            .add_row(cells(&[("temperature", t), ("tension", r)]))
            .unwrap();
    }
    table
}

// ------------------------------------------------------------------- scope

/// The slab, as a table sees it: properties and attributes, and no tables.
#[derive(Default)]
struct Slab {
    values: RefCell<HashMap<Identifier, Value>>,
    generations: RefCell<HashMap<Identifier, u64>>,
}

impl Slab {
    fn with(name: &str, value: Value) -> Slab {
        let slab = Slab::default();
        slab.set(name, value);
        slab
    }

    fn set(&self, name: &str, value: Value) {
        self.values.borrow_mut().insert(id(name), value);
        *self.generations.borrow_mut().entry(id(name)).or_insert(0) += 1;
    }
}

impl Scope for Slab {
    fn read(&self, name: &Identifier) -> Result<Value, ComputeError> {
        Ok(self
            .values
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(Value::absent))
    }

    fn read_list(&self, _: &Identifier) -> Result<Vec<Value>, ComputeError> {
        Ok(Vec::new())
    }

    fn read_uncertainty(&self, _: &Identifier) -> Result<Option<Uncertainty>, ComputeError> {
        Ok(None)
    }

    fn generation(&self, name: &Identifier) -> u64 {
        self.generations.borrow().get(name).copied().unwrap_or(0)
    }
}

fn reads(row: &RowView, column: &str) -> f64 {
    match row.cell(&id(column)).unwrap().value().unwrap() {
        Value::Number(number) => number,
        other => panic!("expected a number, got {other:?}"),
    }
}

// ------------------------------------------------------------------ fakes

/// A row derivation that always raises, to see what a failure leaves behind.
struct Raises;

impl ComputeRow for Raises {
    fn compute(
        &self,
        _: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        Err(ComputeError::failed(std::io::Error::other(
            "no good for this row",
        )))
    }
}

#[test]
fn a_cell_whose_formula_raised_records_the_failure() {
    // A value whose formula failed keeps what it last gave and says that it
    // failed. A property's own cache answers for it; a derived cell has no
    // cache of its own, so unless the failure is written onto the cell it is
    // written nowhere — and `status` goes on calling the cell merely stale,
    // with the failure gone from the repository by the next morning.
    let columns = ["temperature", "rigidity"]
        .iter()
        .map(|name| (id(name), ColumnMeta::default()))
        .collect::<IndexMap<Identifier, ColumnMeta>>();
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns,
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("temperature"))],
            formula: Rc::new(Raises),
        }],
    )
    .unwrap();
    table
        .add_row(vec![(id("temperature"), Property::stored(number(65.0)))])
        .unwrap();

    let outcome = table.resolve(&Slab::default());
    assert!(outcome.is_err(), "the derivation raises");

    let at = samplekit::core::table::RowAddress::Index(vec![number(65.0)]);
    let cell = table.at(&at, &id("rigidity")).unwrap();
    let failure = cell.records().failure.clone();
    assert!(
        failure
            .as_deref()
            .is_some_and(|said| said.contains("no good")),
        "the cell records its failure, got {failure:?}"
    );
}

/// `rigidity = 1 / tension`, one cell per row.
struct Reciprocal {
    calls: Cell<usize>,
}

impl Reciprocal {
    fn new() -> Rc<Reciprocal> {
        Rc::new(Reciprocal {
            calls: Cell::new(0),
        })
    }

    fn derivation(self: &Rc<Self>) -> Derivation {
        Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: self.clone(),
        }
    }
}

impl ComputeRow for Reciprocal {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let mut out = IndexMap::new();
        out.insert(
            id("rigidity"),
            CellOutput::Value(number(1.0 / reads(row, "tension"))),
        );
        Ok(out)
    }
}

/// `rigidity = 1 / (tension * wear)`: one value from the row, one
/// from the slab.
struct OverHardness {
    calls: Cell<usize>,
}

impl OverHardness {
    fn new() -> Rc<OverHardness> {
        Rc::new(OverHardness {
            calls: Cell::new(0),
        })
    }

    fn derivation(self: &Rc<Self>) -> Derivation {
        Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension")), InputName::Named(id("wear"))],
            formula: self.clone(),
        }
    }
}

impl ComputeRow for OverHardness {
    fn compute(
        &self,
        row: &RowView,
        scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let wear = match scope.read(&id("wear"))? {
            Value::Number(number) => number,
            other => panic!("expected a number, got {other:?}"),
        };
        let mut out = IndexMap::new();
        out.insert(
            id("rigidity"),
            CellOutput::Value(number(1.0 / (reads(row, "tension") * wear))),
        );
        Ok(out)
    }
}

/// One fit per row, yielding three columns at once.
struct Fit {
    calls: Cell<usize>,
}

impl Fit {
    fn new() -> Rc<Fit> {
        Rc::new(Fit {
            calls: Cell::new(0),
        })
    }

    fn derivation(self: &Rc<Self>) -> Derivation {
        Derivation::Row {
            outputs: vec![id("r"), id("q"), id("pitch")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: self.clone(),
        }
    }
}

impl ComputeRow for Fit {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let tension = reads(row, "tension");
        let mut out = IndexMap::new();
        out.insert(id("r"), CellOutput::Value(number(tension)));
        out.insert(id("q"), CellOutput::Value(number(tension * 100.0)));
        out.insert(id("pitch"), CellOutput::Value(number(tension / 10.0)));
        Ok(out)
    }
}

/// A value and its uncertainty from one run, per cell.
struct WithUncertainty {
    calls: Cell<usize>,
}

impl WithUncertainty {
    fn new() -> Rc<WithUncertainty> {
        Rc::new(WithUncertainty {
            calls: Cell::new(0),
        })
    }

    fn derivation(self: &Rc<Self>) -> Derivation {
        Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: self.clone(),
        }
    }
}

impl ComputeRow for WithUncertainty {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let rigidity = 1.0 / reads(row, "tension");
        let mut out = IndexMap::new();
        out.insert(
            id("rigidity"),
            CellOutput::Quantity(
                number(rigidity),
                Some(Uncertainty::new(rigidity / 100.0).unwrap()),
            ),
        );
        Ok(out)
    }
}

/// `normalized = tension / max(tension)`: one run for the whole table.
struct Normalize {
    calls: Cell<usize>,
    short: bool,
}

impl Normalize {
    fn new(short: bool) -> Rc<Normalize> {
        Rc::new(Normalize {
            calls: Cell::new(0),
            short,
        })
    }

    fn derivation(self: &Rc<Self>) -> Derivation {
        Derivation::Column {
            outputs: vec![id("normalized")],
            inputs: vec![InputName::Column {
                table: id("preheating"),
                column: id("tension"),
            }],
            formula: self.clone(),
        }
    }
}

impl ComputeColumn for Normalize {
    fn compute(
        &self,
        set: &ColumnSet,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        self.calls.set(self.calls.get() + 1);
        let column = set.column(&id("tension")).unwrap();
        let values: Vec<f64> = column
            .cells()
            .map(|(_, cell)| match cell.value().unwrap() {
                Value::Number(number) => number,
                other => panic!("expected a number, got {other:?}"),
            })
            .collect();
        let peak = values.iter().cloned().fold(f64::MIN, f64::max);
        let mut produced: Vec<CellOutput> = values
            .iter()
            .map(|value| CellOutput::Value(number(value / peak)))
            .collect();
        if self.short {
            produced.pop();
        }
        let mut out = IndexMap::new();
        out.insert(id("normalized"), produced);
        Ok(out)
    }
}

/// Returns a column nobody declared.
struct Wrong;

impl ComputeRow for Wrong {
    fn compute(
        &self,
        _: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        let mut out = IndexMap::new();
        out.insert(id("tension"), CellOutput::Value(number(0.0)));
        Ok(out)
    }
}

// ---------------------------------------------------------------- structure

#[test]
fn column_order_is_declaration_order() {
    let mut table = soak();
    let before: Vec<String> = table.column_names().iter().map(|n| n.to_string()).collect();
    assert_eq!(before, ["temperature", "tension", "rigidity"]);
    table
        .update_row(&by_index(&[65.0]), cells(&[("tension", 9.0)]))
        .unwrap();
    let after: Vec<String> = table.column_names().iter().map(|n| n.to_string()).collect();
    assert_eq!(before, after, "a mutation reordered the columns");
}

#[test]
fn every_row_has_every_column() {
    // An unsupplied cell is Absent, which is a recorded statement.
    let table = soak();
    let cell = table.at(&by_index(&[65.0]), &id("rigidity")).unwrap();
    assert_eq!(cell.value().unwrap(), Value::absent());
    assert_eq!(
        table.row(&by_index(&[65.0])).unwrap().column_names().len(),
        3
    );
}

#[test]
fn rows_iterate_in_declaration_order() {
    let table = soak();
    let order: Vec<f64> = table
        .rows()
        .map(|row| match row.index()[0] {
            Value::Number(number) => *number,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, [65.0, 200.0, 78.5]);
    assert_eq!(table.rows().count(), 3);
}

#[test]
fn a_column_view_pairs_cells_with_their_index() {
    // Never a bare sequence: a column detached from its indexes is a list of
    // numbers nobody can locate.
    let table = soak();
    let column = table.column(&id("tension")).unwrap();
    let pairs: Vec<(f64, f64)> = column
        .cells()
        .map(|(index, cell)| {
            let index = match index[0] {
                Value::Number(number) => *number,
                other => panic!("{other:?}"),
            };
            let value = match cell.value().unwrap() {
                Value::Number(number) => number,
                other => panic!("{other:?}"),
            };
            (index, value)
        })
        .collect();
    assert_eq!(pairs, [(65.0, 12.5), (200.0, 15.0), (78.5, 20.0)]);
    assert_eq!(column.len(), 3);
}

// --------------------------------------------------------------- addressing

#[test]
fn at_addresses_by_index_value() {
    // Whatever its position.
    let table = soak();
    assert_eq!(value_at(&table, &by_index(&[78.5]), "tension"), 20.0);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "tension"), 12.5);
}

#[test]
fn integer_and_float_index_match() {
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    table
        .add_row(vec![
            (id("k"), Property::stored(number(20.0))),
            (id("v"), Property::stored(number(1.0))),
        ])
        .unwrap();
    let by_integer = RowAddress::Index(vec![Value::integer(20)]);
    assert_eq!(value_at(&table, &by_integer, "v"), 1.0);
}

#[test]
fn distinct_integers_beyond_the_float_range_are_distinct_rows() {
    // Integers compare as integers. 2^53 and 2^53 + 1 are one f64 and two i64;
    // keying rows through the float made them one row.
    let mut table = Table::new(
        id("counts"),
        vec![id("n")],
        columns(&["n", "v"]),
        Vec::new(),
    )
    .unwrap();
    let below = 9_007_199_254_740_992_i64;
    let above = below + 1;
    table
        .add_row(vec![
            (id("n"), Property::stored(Value::integer(below))),
            (id("v"), Property::stored(number(1.0))),
        ])
        .unwrap();
    table
        .add_row(vec![
            (id("n"), Property::stored(Value::integer(above))),
            (id("v"), Property::stored(number(2.0))),
        ])
        .unwrap();
    assert_eq!(table.rows().count(), 2);
    let at = |n: i64| {
        table
            .at(&RowAddress::Index(vec![Value::integer(n)]), &id("v"))
            .unwrap()
            .value()
            .unwrap()
    };
    assert_eq!(at(below), number(1.0));
    assert_eq!(at(above), number(2.0));
    // And a whole number still finds the integer row.
    assert_eq!(
        table
            .at(
                &RowAddress::Index(vec![number(9_007_199_254_740_992.0)]),
                &id("v")
            )
            .unwrap()
            .value()
            .unwrap(),
        number(1.0)
    );
}

#[test]
fn negative_zero_index_matches_zero() {
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    table
        .add_row(vec![
            (id("k"), Property::stored(number(0.0))),
            (id("v"), Property::stored(number(1.0))),
        ])
        .unwrap();
    assert_eq!(value_at(&table, &by_index(&[-0.0]), "v"), 1.0);
}

#[test]
fn index_matching_is_exact() {
    // Tolerant matching would need a tolerance, and any tolerance is wrong for
    // some casting.
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    table
        .add_row(vec![
            (id("k"), Property::stored(number(0.3))),
            (id("v"), Property::stored(number(1.0))),
        ])
        .unwrap();
    let error = table.at(&by_index(&[0.1 + 0.2]), &id("v")).unwrap_err();
    let TableError::UnknownIndex { per_column, .. } = &error else {
        panic!("expected an unknown index, got {error:?}");
    };
    assert_eq!(per_column[0].nearest, Some(number(0.3)));
}

#[test]
fn an_ordinal_addresses_a_row() {
    let table = soak();
    assert_eq!(value_at(&table, &RowAddress::ordinal(2), "tension"), 20.0);
    assert_eq!(table.row(&RowAddress::ordinal(1)).unwrap().position(), 1);
}

#[test]
fn an_ordinal_and_an_index_value_are_distinguishable() {
    // On an integer-indexed table both exist and name different rows.
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    for (k, v) in [(10i64, 1.0), (3, 2.0), (7, 3.0)] {
        table
            .add_row(vec![
                (id("k"), Property::stored(Value::integer(k))),
                (id("v"), Property::stored(number(v))),
            ])
            .unwrap();
    }
    let by_value = RowAddress::Index(vec![Value::integer(3)]);
    assert_eq!(value_at(&table, &by_value, "v"), 2.0);
    assert_eq!(value_at(&table, &RowAddress::ordinal(0), "v"), 1.0);
}

#[test]
fn an_ordinal_is_never_a_tuple_component() {
    // It addresses the whole row whatever the index arity, and a mixed address
    // is unrepresentable: RowAddress::Ordinal holds one number.
    let table = soak();
    assert!(table.at(&RowAddress::ordinal(0), &id("tension")).is_ok());
    // A composite table refuses an address of the wrong arity rather than
    // reading one component as a position.
    let mut composite = Table::new(
        id("t"),
        vec![id("k"), id("f")],
        columns(&["k", "f", "v"]),
        Vec::new(),
    )
    .unwrap();
    composite
        .add_row(cells(&[("k", 65.0), ("f", 1e9), ("v", 1.0)]))
        .unwrap();
    assert!(matches!(
        composite.at(&by_index(&[65.0]), &id("v")),
        Err(TableError::IndexArity { .. })
    ));
    assert!(composite.at(&RowAddress::ordinal(0), &id("v")).is_ok());
}

#[test]
fn an_ordinal_past_the_end_is_an_error() {
    // Not an empty result.
    let table = soak();
    assert_eq!(
        table
            .at(&RowAddress::ordinal(9), &id("tension"))
            .unwrap_err(),
        TableError::OrdinalOutOfRange {
            ordinal: 9,
            length: 3
        }
    );
}

// ------------------------------------------------------------- diagnostics

#[test]
fn unknown_column_lists_available_columns() {
    let table = soak();
    let error = table
        .at(&by_index(&[65.0]), &id("nitridation_level"))
        .unwrap_err();
    let TableError::UnknownColumn { available, .. } = &error else {
        panic!("expected an unknown column, got {error:?}");
    };
    let names: Vec<String> = available.iter().map(|n| n.to_string()).collect();
    assert_eq!(names, ["temperature", "tension", "rigidity"]);
    assert!(error.to_string().contains("tension"), "{error}");
}

#[test]
fn unknown_column_suggests_nearest_name() {
    let table = soak();
    let error = table.at(&by_index(&[65.0]), &id("tensionn")).unwrap_err();
    let TableError::UnknownColumn { suggestion, .. } = &error else {
        panic!("expected an unknown column, got {error:?}");
    };
    assert_eq!(
        suggestion.as_ref().map(|n| n.to_string()),
        Some("tension".to_string())
    );
}

#[test]
fn unknown_index_lists_available_indexes() {
    let table = soak();
    let error = table.at(&by_index(&[999.0]), &id("tension")).unwrap_err();
    let TableError::UnknownIndex { per_column, .. } = &error else {
        panic!("expected an unknown index, got {error:?}");
    };
    assert_eq!(per_column.len(), 1);
    assert_eq!(per_column[0].column, id("temperature"));
    assert_eq!(per_column[0].available.len(), 3);
}

#[test]
fn unknown_index_suggests_nearest_value() {
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    for k in [10.0, 15.0, 20.5] {
        table.add_row(cells(&[("k", k), ("v", 1.0)])).unwrap();
    }
    let error = table.at(&by_index(&[20.0]), &id("v")).unwrap_err();
    let TableError::UnknownIndex { per_column, .. } = &error else {
        panic!("expected an unknown index, got {error:?}");
    };
    assert_eq!(per_column[0].nearest, Some(number(20.5)));
}

#[test]
fn the_wrong_number_of_components_is_its_own_error() {
    // Counting the columns is a different fix from "that row does not exist".
    let mut table = Table::new(
        id("t"),
        vec![id("k"), id("f")],
        columns(&["k", "f", "v"]),
        Vec::new(),
    )
    .unwrap();
    table
        .add_row(cells(&[("k", 65.0), ("f", 1e9), ("v", 1.0)]))
        .unwrap();
    assert_eq!(
        table.at(&by_index(&[65.0]), &id("v")).unwrap_err(),
        TableError::IndexArity {
            expected: 2,
            found: 1
        }
    );
}

#[test]
fn duplicate_index_is_rejected() {
    let mut table = soak();
    let error = table
        .add_row(cells(&[("temperature", 65.0), ("tension", 1.0)]))
        .unwrap_err();
    assert_eq!(
        error,
        TableError::DuplicateIndex {
            index: vec![number(65.0)],
            existing_position: 0
        }
    );
}

// ------------------------------------------------------------- composite

#[test]
fn a_composite_index_identifies_a_row() {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature"), id("ferrite")],
        columns(&["temperature", "ferrite", "tension"]),
        Vec::new(),
    )
    .unwrap();
    for t in [65.0, 200.0, 78.5] {
        for f in [1e9, 2e9] {
            table
                .add_row(cells(&[
                    ("temperature", t),
                    ("ferrite", f),
                    ("tension", t + f / 1e9),
                ]))
                .unwrap();
        }
    }
    assert_eq!(table.rows().count(), 6);
    assert_eq!(value_at(&table, &by_index(&[65.0, 2e9]), "tension"), 67.0);
}

#[test]
fn a_repeated_component_is_not_a_duplicate() {
    // Rows share a temperature and differ by ferrite.
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature"), id("ferrite")],
        columns(&["temperature", "ferrite", "tension"]),
        Vec::new(),
    )
    .unwrap();
    table
        .add_row(cells(&[
            ("temperature", 65.0),
            ("ferrite", 1e9),
            ("tension", 1.0),
        ]))
        .unwrap();
    table
        .add_row(cells(&[
            ("temperature", 65.0),
            ("ferrite", 2e9),
            ("tension", 2.0),
        ]))
        .unwrap();
    assert_eq!(table.index_values(&id("temperature")).unwrap().len(), 1);
    assert_eq!(table.index_values(&id("ferrite")).unwrap().len(), 2);
}

#[test]
fn a_composite_miss_reports_each_dimension() {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature"), id("ferrite")],
        columns(&["temperature", "ferrite", "tension"]),
        Vec::new(),
    )
    .unwrap();
    for f in [1e9, 2e9] {
        table
            .add_row(cells(&[
                ("temperature", 20.5),
                ("ferrite", f),
                ("tension", 1.0),
            ]))
            .unwrap();
    }
    let error = table
        .at(&by_index(&[20.0, 2e9]), &id("tension"))
        .unwrap_err();
    let TableError::UnknownIndex { per_column, .. } = &error else {
        panic!("expected an unknown index, got {error:?}");
    };
    assert_eq!(per_column[0].nearest, Some(number(20.5)), "temperature");
    assert_eq!(per_column[1].nearest, None, "ferrite matched");
    assert!(error.to_string().contains("matched"), "{error}");
}

#[test]
fn lookup_stays_consistent_after_updates() {
    // Property test: every row is still found by the index it carries.
    let mut table = Table::new(id("t"), vec![id("k")], columns(&["k", "v"]), Vec::new()).unwrap();
    let mut seed = 0xBEEFu64;
    let mut next = move || {
        seed ^= seed >> 12;
        seed ^= seed << 25;
        seed ^= seed >> 27;
        seed.wrapping_mul(0x2545_F491_4F6C_DD1D)
    };
    for step in 0..60u64 {
        if step % 4 == 0 && table.rows().count() > 0 {
            let position = (next() % table.rows().count() as u64) as usize;
            let key = table.row(&RowAddress::ordinal(position)).unwrap().index()[0].clone();
            table
                .update_row(
                    &RowAddress::Index(vec![key]),
                    cells(&[("v", (next() % 100) as f64)]),
                )
                .unwrap();
        } else {
            let _ = table.add_row(cells(&[("k", step as f64), ("v", 1.0)]));
        }
        for position in 0..table.rows().count() {
            let row = table.row(&RowAddress::ordinal(position)).unwrap();
            let key = row.index()[0].clone();
            let found = table.row(&RowAddress::Index(vec![key])).unwrap();
            assert_eq!(found.position(), position, "lookup disagrees with rows");
        }
    }
}

// ------------------------------------------------------------- derivation

#[test]
fn computed_cell_is_lazy() {
    // Adding rows runs no formula.
    let formula = Reciprocal::new();
    let table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    assert_eq!(formula.calls.get(), 0);
    assert!(!table.is_resolved());
}

#[test]
fn computed_cell_sees_its_own_row() {
    let formula = Reciprocal::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(value_at(&table, &by_index(&[65.0]), "rigidity"), 1.0 / 12.5);
    assert_eq!(value_at(&table, &by_index(&[78.5]), "rigidity"), 1.0 / 20.0);
    assert_eq!(formula.calls.get(), 3, "once per row");
    assert!(table.is_resolved());
}

#[test]
fn a_cell_cannot_reach_another_row() {
    // A RowView holds one row: everything reachable from it is that row's.
    let formula = Reciprocal::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    table.resolve(&Slab::default()).unwrap();
    for row in table.rows() {
        let tension = reads(&row, "tension");
        let rigidity = reads(&row, "rigidity");
        assert!(
            (rigidity - 1.0 / tension).abs() < 1e-12,
            "a cell was computed from another row"
        );
        // And nothing on the view reaches a second row.
        assert_eq!(row.index().len(), 1);
    }
}

#[test]
fn a_cell_reads_a_sample_property() {
    let formula = OverHardness::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    let slab = Slab::with("wear", number(2.0));
    table.resolve(&slab).unwrap();
    assert_eq!(
        value_at(&table, &by_index(&[65.0]), "rigidity"),
        1.0 / (12.5 * 2.0)
    );
}

#[test]
fn a_cell_goes_stale_alone() {
    let formula = Reciprocal::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    let slab = Slab::default();
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 3);

    table
        .update_row(&by_index(&[65.0]), cells(&[("tension", 10.0)]))
        .unwrap();
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 4, "one row changed, one run");
    assert_eq!(value_at(&table, &by_index(&[65.0]), "rigidity"), 0.1);
    assert_eq!(value_at(&table, &by_index(&[78.5]), "rigidity"), 1.0 / 20.0);
}

#[test]
fn a_sample_input_stales_the_whole_column() {
    // All eleven read it, so all eleven go.
    let formula = OverHardness::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    let slab = Slab::with("wear", number(2.0));
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 3);
    slab.set("wear", number(4.0));
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 6, "every row read the wear");
    assert_eq!(
        value_at(&table, &by_index(&[65.0]), "rigidity"),
        1.0 / (12.5 * 4.0)
    );
}

#[test]
fn one_row_derivation_fills_several_columns() {
    // A fit yielding R, ph and pitch runs once per row, not three times.
    let formula = Fit::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "r", "q", "pitch"],
    );
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(formula.calls.get(), 3);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "r"), 12.5);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "q"), 1250.0);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "pitch"), 1.25);
}

#[test]
fn a_row_derivation_stales_its_outputs_together() {
    // They came from one run, so they go stale as one.
    let formula = Fit::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "r", "q", "pitch"],
    );
    let slab = Slab::default();
    table.resolve(&slab).unwrap();
    table
        .update_row(&by_index(&[200.0]), cells(&[("tension", 30.0)]))
        .unwrap();
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 4, "one row, one run, three columns");
    assert_eq!(value_at(&table, &by_index(&[200.0]), "r"), 30.0);
    assert_eq!(value_at(&table, &by_index(&[200.0]), "q"), 3000.0);
    assert_eq!(value_at(&table, &by_index(&[200.0]), "pitch"), 3.0);
}

#[test]
fn a_joint_column_runs_once_per_cell() {
    // Both channels filled, one run each.
    let formula = WithUncertainty::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(formula.calls.get(), 3);
    let cell = table.at(&by_index(&[65.0]), &id("rigidity")).unwrap();
    let value = 1.0 / 12.5;
    assert_eq!(cell.value().unwrap(), number(value));
    assert_eq!(
        cell.uncertainty().unwrap().unwrap().magnitude(),
        value / 100.0
    );
}

#[test]
fn a_column_derivation_fills_every_row() {
    let formula = Normalize::new(false);
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "normalized"],
    );
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(formula.calls.get(), 1, "once for the table");
    assert_eq!(value_at(&table, &by_index(&[78.5]), "normalized"), 1.0);
    assert_eq!(
        value_at(&table, &by_index(&[65.0]), "normalized"),
        12.5 / 20.0
    );
}

#[test]
fn a_column_derivation_stales_entirely() {
    // One changed cell restages the whole column, which is what a maximum
    // depends on.
    let formula = Normalize::new(false);
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "normalized"],
    );
    let slab = Slab::default();
    table.resolve(&slab).unwrap();
    table
        .update_row(&by_index(&[65.0]), cells(&[("tension", 40.0)]))
        .unwrap();
    table.resolve(&slab).unwrap();
    assert_eq!(formula.calls.get(), 2);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "normalized"), 1.0);
    assert_eq!(value_at(&table, &by_index(&[78.5]), "normalized"), 0.5);
}

#[test]
fn a_derived_cell_carries_no_record_until_it_is_saved() {
    // Resolving writes values and never a claim about where they came from.
    let formula = Reciprocal::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    table.resolve(&Slab::default()).unwrap();
    let cell = table.at(&by_index(&[65.0]), &id("rigidity")).unwrap();
    assert!(cell.records().is_empty());
    assert!(table.is_derived(&id("rigidity")));
    assert!(!table.is_derived(&id("tension")));
}

// --------------------------------------------------------------- refusals

#[test]
fn an_undeclared_output_is_refused() {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: Vec::new(),
            formula: Rc::new(Wrong),
        }],
    )
    .unwrap();
    table
        .add_row(cells(&[("temperature", 65.0), ("tension", 1.0)]))
        .unwrap();
    let error = table.resolve(&Slab::default()).unwrap_err();
    assert!(
        matches!(error, TableError::OutputMismatch { .. }),
        "{error:?}"
    );
}

#[test]
fn a_short_column_result_is_refused() {
    // Ten cells for eleven rows is an error, not a partly filled column.
    let formula = Normalize::new(true);
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "normalized"],
    );
    assert_eq!(
        table.resolve(&Slab::default()),
        Err(TableError::OutputLength {
            column: id("normalized"),
            expected: 3,
            produced: 2
        })
    );
}

#[test]
fn two_derivations_cannot_fill_one_column() {
    let one = Reciprocal::new();
    let other = Reciprocal::new();
    assert_eq!(
        Table::new(
            id("preheating"),
            vec![id("temperature")],
            columns(&["temperature", "tension", "rigidity"]),
            vec![one.derivation(), other.derivation()],
        )
        .unwrap_err(),
        TableError::ColumnDerivedTwice {
            column: id("rigidity")
        }
    );
}

#[test]
fn a_derivation_naming_an_unknown_column_is_refused() {
    // Whether the name is an output or an input.
    let formula = Reciprocal::new();
    let outputs_wrong = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension"]),
        vec![formula.derivation()],
    );
    assert!(matches!(
        outputs_wrong,
        Err(TableError::UnknownColumn { .. })
    ));

    let inputs_wrong = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "rigidity"]),
        vec![formula.derivation()],
    );
    assert!(matches!(
        inputs_wrong,
        Err(TableError::UnknownColumn { .. })
    ));
}

#[test]
fn a_column_named_row_is_refused() {
    // Where the columns are declared, so the `row.` prefix stays unambiguous.
    assert_eq!(
        Table::new(id("t"), vec![id("k")], columns(&["k", "row"]), Vec::new()).unwrap_err(),
        TableError::ReservedColumnName { name: id("row") }
    );
}

#[test]
fn a_leading_underscore_is_refused_on_tables_and_columns() {
    assert!(matches!(
        Table::new(
            id("_preheating"),
            vec![id("temperature")],
            columns(&["temperature"]),
            Vec::new()
        ),
        Err(TableError::PrivateName { .. })
    ));
    assert!(matches!(
        Table::new(
            id("preheating"),
            vec![id("_temperature")],
            columns(&["_temperature"]),
            Vec::new()
        ),
        Err(TableError::PrivateName { .. })
    ));
}

// -------------------------------------------------------------- metadata

#[test]
fn column_metadata_is_inherited_by_cells() {
    let mut declared = columns(&["k", "v"]);
    declared[&id("v")] = ColumnMeta {
        presentation: Presentation {
            unit: Some("lintner".to_string()),
            symbol: Some("R".to_string()),
            precision: None,
        },
        statistics: Default::default(),
    };
    let mut table = Table::new(id("t"), vec![id("k")], declared, Vec::new()).unwrap();
    table.add_row(cells(&[("k", 1.0), ("v", 2.0)])).unwrap();
    let effective = table.presentation_of(&id("v"), &by_index(&[1.0])).unwrap();
    assert_eq!(effective.unit.as_deref(), Some("lintner"));
    assert_eq!(effective.symbol.as_deref(), Some("R"));
}

#[test]
fn cell_metadata_overrides_column_metadata() {
    // Field by field: the override costs no repeated unit.
    let mut declared = columns(&["k", "v"]);
    declared[&id("v")] = ColumnMeta {
        presentation: Presentation {
            unit: Some("lintner".to_string()),
            symbol: Some("R".to_string()),
            precision: None,
        },
        statistics: Default::default(),
    };
    let mut table = Table::new(id("t"), vec![id("k")], declared, Vec::new()).unwrap();
    let mut cell = Property::stored(number(2.0));
    cell.set_presentation(Presentation {
        unit: None,
        symbol: Some("R_4w".to_string()),
        precision: None,
    });
    table
        .add_row(vec![
            (id("k"), Property::stored(number(1.0))),
            (id("v"), cell),
        ])
        .unwrap();
    let effective = table.presentation_of(&id("v"), &by_index(&[1.0])).unwrap();
    assert_eq!(effective.symbol.as_deref(), Some("R_4w"));
    assert_eq!(
        effective.unit.as_deref(),
        Some("lintner"),
        "the unit is inherited"
    );
}

// ----------------------------------------------------------------- filling

/// rigidity = 1 / tension, counting its runs.
#[derive(Default)]
struct FillReciprocal {
    runs: Cell<usize>,
}

impl ComputeRow for FillReciprocal {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        self.runs.set(self.runs.get() + 1);
        let mut out = IndexMap::new();
        out.insert(
            id("rigidity"),
            CellOutput::Value(number(1.0 / reads(row, "tension"))),
        );
        Ok(out)
    }
}

/// A model's declaration: the columns and the formula, and no rows.
fn fill_declared(formula: &Rc<FillReciprocal>) -> Table {
    Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: formula.clone(),
        }],
    )
    .unwrap()
}

/// What a file holds: rows with a rigidity, or without one.
fn fill_file(rows: &[(f64, f64, Option<f64>)]) -> Table {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        Vec::new(),
    )
    .unwrap();
    for (temperature, tension, rigidity) in rows {
        let mut row = cells(&[("temperature", *temperature), ("tension", *tension)]);
        if let Some(rigidity) = rigidity {
            row.push((id("rigidity"), Property::stored(number(*rigidity))));
        }
        table.add_row(row).unwrap();
    }
    table
}

#[test]
fn filling_keeps_derivations_and_file_rows() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = fill_declared(&formula);
    let scope = Slab::default();
    table
        .fill_from(
            fill_file(&[(65.0, 12.5, Some(0.08)), (200.0, 15.0, Some(0.0667))]),
            &scope,
        )
        .unwrap();
    assert!(table.is_derived(&id("rigidity")));
    assert!(table.is_resolved());
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 0);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "rigidity"), 0.08);
    assert_eq!(value_at(&table, &by_index(&[200.0]), "tension"), 15.0);
}

#[test]
fn filling_runs_only_what_the_file_lacks() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = fill_declared(&formula);
    let scope = Slab::default();
    table
        .fill_from(
            fill_file(&[(65.0, 12.5, Some(0.08)), (200.0, 15.0, None)]),
            &scope,
        )
        .unwrap();
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 1);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "rigidity"), 0.08);
    assert_eq!(
        value_at(&table, &by_index(&[200.0]), "rigidity"),
        1.0 / 15.0
    );
}

#[test]
fn a_filled_derived_cell_goes_stale_when_its_input_changes() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = fill_declared(&formula);
    let scope = Slab::default();
    table
        .fill_from(
            fill_file(&[(65.0, 12.5, Some(0.08)), (200.0, 15.0, Some(0.0667))]),
            &scope,
        )
        .unwrap();
    table
        .update_row(&by_index(&[200.0]), cells(&[("tension", 30.0)]))
        .unwrap();
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 1);
    assert_eq!(
        value_at(&table, &by_index(&[200.0]), "rigidity"),
        1.0 / 30.0
    );
    assert_eq!(value_at(&table, &by_index(&[65.0]), "rigidity"), 0.08);
}

#[test]
fn filling_keeps_declared_rows_and_columns_the_file_lacks() {
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        Vec::new(),
    )
    .unwrap();
    table
        .add_row(cells(&[("temperature", 300.0), ("rigidity", 0.025)]))
        .unwrap();
    let mut file = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "remark"]),
        Vec::new(),
    )
    .unwrap();
    file.add_row(cells(&[
        ("temperature", 65.0),
        ("tension", 12.5),
        ("remark", 1.0),
    ]))
    .unwrap();
    table.fill_from(file, &Slab::default()).unwrap();

    let names: Vec<&str> = table
        .column_names()
        .into_iter()
        .map(Identifier::as_str)
        .collect();
    assert_eq!(names, ["temperature", "tension", "rigidity", "remark"]);
    assert_eq!(
        table.index_tuples(),
        vec![vec![&number(65.0)], vec![&number(300.0)]]
    );
    assert_eq!(value_at(&table, &by_index(&[300.0]), "rigidity"), 0.025);
    assert_eq!(value_at(&table, &by_index(&[65.0]), "remark"), 1.0);
    assert!(
        table
            .at(&by_index(&[300.0]), &id("remark"))
            .unwrap()
            .value()
            .unwrap()
            .is_absent()
    );
}

#[test]
fn filling_refuses_a_different_index() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = fill_declared(&formula);
    table
        .add_row(cells(&[("temperature", 65.0), ("tension", 12.5)]))
        .unwrap();
    let file = Table::new(
        id("preheating"),
        vec![id("tension")],
        columns(&["temperature", "tension"]),
        Vec::new(),
    )
    .unwrap();
    let refused = table.fill_from(file, &Slab::default());
    assert_eq!(
        refused,
        Err(TableError::IndexMismatch {
            declared: vec![id("temperature")],
            file: vec![id("tension")],
        })
    );
    assert_eq!(table.index_tuples().len(), 1);
}

// ----------------------------------------------------------------- renaming

/// A column span with nothing to compute: renaming reads only its declaration.
struct FillNothing;

impl ComputeColumn for FillNothing {
    fn compute(
        &self,
        _: &ColumnSet,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        Ok(IndexMap::new())
    }
}

#[test]
fn renaming_requalifies_its_column_inputs() {
    let mut table = Table::new(
        id("old"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "halved"]),
        vec![Derivation::Column {
            outputs: vec![id("halved")],
            inputs: vec![InputName::Column {
                table: id("old"),
                column: id("tension"),
            }],
            formula: Rc::new(FillNothing),
        }],
    )
    .unwrap();
    table.set_name(id("new")).unwrap();
    assert_eq!(table.name(), &id("new"));
    assert_eq!(
        table.inputs_of(&id("halved")).unwrap(),
        &[InputName::Column {
            table: id("new"),
            column: id("tension"),
        }]
    );
    assert_eq!(
        table.set_name(id("_private")),
        Err(TableError::PrivateName {
            name: id("_private")
        })
    );
    assert_eq!(table.name(), &id("new"));
}

#[test]
fn an_invalidated_row_runs_again() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = fill_declared(&formula);
    table
        .add_row(cells(&[("temperature", 65.0), ("tension", 12.5)]))
        .unwrap();
    table
        .add_row(cells(&[("temperature", 200.0), ("tension", 15.0)]))
        .unwrap();
    let scope = Slab::default();
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 2);

    table.invalidate_row(&by_index(&[200.0])).unwrap();
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 3);
    assert!(matches!(
        table.invalidate_row(&by_index(&[300.0])),
        Err(TableError::UnknownIndex { .. })
    ));
}

// ------------------------------------------------------------- settling

/// A slab that tells its epoch and counts the counters read from it.
#[derive(Default)]
struct Watched {
    epoch: Cell<u64>,
    reads: Cell<usize>,
}

impl Scope for Watched {
    fn read(&self, _: &Identifier) -> Result<Value, ComputeError> {
        Ok(Value::absent())
    }

    fn read_list(&self, _: &Identifier) -> Result<Vec<Value>, ComputeError> {
        Ok(Vec::new())
    }

    fn read_uncertainty(&self, _: &Identifier) -> Result<Option<Uncertainty>, ComputeError> {
        Ok(None)
    }

    fn generation(&self, _: &Identifier) -> u64 {
        self.reads.set(self.reads.get() + 1);
        0
    }

    fn epoch(&self) -> Option<u64> {
        Some(self.epoch.get())
    }
}

#[test]
fn a_settled_table_resolves_without_reading_a_counter() {
    let formula = Rc::new(FillReciprocal::default());
    let mut table = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension")), InputName::Named(id("wear"))],
            formula: formula.clone(),
        }],
    )
    .unwrap();
    for (temperature, tension) in [(65.0, 12.5), (200.0, 15.0)] {
        table
            .add_row(cells(&[("temperature", temperature), ("tension", tension)]))
            .unwrap();
    }
    let scope = Watched::default();
    table.resolve(&scope).unwrap();
    table.resolve(&scope).unwrap();
    assert!(table.is_settled(&scope));
    let reads = scope.reads.get();
    table.resolve(&scope).unwrap();
    assert_eq!(scope.reads.get(), reads);
    assert_eq!(formula.runs.get(), 2);

    scope.epoch.set(1);
    assert!(!table.is_settled(&scope));
    table.resolve(&scope).unwrap();
    assert!(scope.reads.get() > reads);
    assert_eq!(formula.runs.get(), 2);

    table
        .update_row(&by_index(&[65.0]), cells(&[("tension", 25.0)]))
        .unwrap();
    assert!(!table.is_settled(&scope));
    table.resolve(&scope).unwrap();
    assert_eq!(formula.runs.get(), 3);
}

#[test]
fn a_column_derivation_refuses_a_row_input() {
    let column_span = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "normalized"]),
        vec![Derivation::Column {
            outputs: vec![id("normalized")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: Normalize::new(false),
        }],
    );
    assert!(matches!(
        column_span,
        Err(TableError::WrongInputScope { .. })
    ));
}

#[test]
fn a_row_derivation_refuses_a_whole_column_of_its_own_table() {
    // A cell resting on its own table's whole column is a Column derivation.
    let row_span = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Column {
                table: id("preheating"),
                column: id("tension"),
            }],
            formula: Reciprocal::new(),
        }],
    );
    assert!(matches!(row_span, Err(TableError::WrongInputScope { .. })));
}

#[test]
fn a_derivation_may_read_a_column_of_another_table() {
    let foreign = InputName::Column {
        table: id("adjustment"),
        column: id("offset"),
    };
    let row_span = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "rigidity"]),
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension")), foreign.clone()],
            formula: Reciprocal::new(),
        }],
    );
    assert!(row_span.is_ok(), "{row_span:?}");
    let column_span = Table::new(
        id("preheating"),
        vec![id("temperature")],
        columns(&["temperature", "tension", "normalized"]),
        vec![Derivation::Column {
            outputs: vec![id("normalized")],
            inputs: vec![foreign],
            formula: Normalize::new(false),
        }],
    );
    assert!(column_span.is_ok(), "{column_span:?}");
}

#[test]
fn a_derived_cell_supplied_with_its_row_is_held() {
    // A derived cell given by hand with the row `add_row` appends is an
    // override, as one written by hand into an existing row is.
    let mut table = derived(
        vec![Derivation::Row {
            outputs: vec![id("rigidity")],
            inputs: vec![InputName::Cell(id("tension"))],
            formula: Reciprocal::new(),
        }],
        &["temperature", "tension", "rigidity"],
    );
    table
        .add_row(cells(&[
            ("temperature", 300.0),
            ("tension", 4.0),
            ("rigidity", 99.0),
        ]))
        .unwrap();
    table
        .add_row(cells(&[("temperature", 400.0), ("tension", 8.0)]))
        .unwrap();
    let held = by_index(&[300.0]);
    let free = by_index(&[400.0]);
    assert!(table.is_held(&held, &id("rigidity")).unwrap());
    assert!(!table.is_held(&free, &id("rigidity")).unwrap());
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(value_at(&table, &held, "rigidity"), 99.0);
    assert_eq!(value_at(&table, &free, "rigidity"), 0.125);
    table.release_cells(&id("rigidity"), Some(&held)).unwrap();
    table.invalidate_row(&held).unwrap();
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(value_at(&table, &held, "rigidity"), 0.25);
}

#[test]
fn a_text_index_suggests_its_spelling() {
    // `inspection.score[lea]` was answered *'Ivo', 'Lea', 'Mia'   matched*: no
    // nearest for a text, and a miss said matched.
    let mut table = Table::new(
        id("inspection"),
        vec![id("inspector")],
        columns(&["inspector", "score"]),
        Vec::new(),
    )
    .unwrap();
    for inspector in ["Ivo", "Lea", "Mia"] {
        table
            .add_row(vec![
                (id("inspector"), Property::stored(Value::text(inspector))),
                (id("score"), Property::stored(number(40.0))),
            ])
            .unwrap();
    }
    let Err(error) = table.row(&RowAddress::index(vec![Value::text("lea")])) else {
        panic!("'lea' names no row");
    };
    let TableError::UnknownIndex { per_column, .. } = &error else {
        panic!("{error:?}");
    };
    assert_eq!(per_column[0].nearest, Some(Value::text("Lea")));
    let said = error.to_string();
    assert!(said.contains("did you mean 'Lea'?"), "{said}");
    assert!(!said.contains("matched"), "{said}");
    // Nothing near: neither a suggestion nor *matched*.
    let Err(far) = table.row(&RowAddress::index(vec![Value::text("Zygmunt")])) else {
        panic!("'Zygmunt' names no row");
    };
    let far = far.to_string();
    assert!(
        !far.contains("matched") && !far.contains("did you mean"),
        "{far}"
    );
}

#[test]
fn a_duplicate_row_is_named_counted_from_one() {
    // *at #7* read as the address `[#7]`, which is the eighth row.
    let mut table = soak();
    let said = table
        .add_row(cells(&[("temperature", 65.0), ("tension", 1.0)]))
        .unwrap_err()
        .to_string();
    assert!(
        said.contains("already exists: row 1 of the table"),
        "{said}"
    );
    assert!(!said.contains('#'), "{said}");
}

// ---------------------------------------------------------- statistics

/// A toughness table whose `stiffness` column declares which statistic of a cell's
/// readings stands for each channel.
fn toughness(value: Option<Location>, uncertainty: Option<Convention>) -> Table {
    let mut declared = columns(&["temperature", "stiffness"]);
    declared[&id("stiffness")].statistics = DeclaredStatistics { value, uncertainty };
    Table::new(
        id("toughness"),
        vec![id("temperature")],
        declared,
        Vec::new(),
    )
    .unwrap()
}

fn measured(values: &[f64]) -> Property {
    Property::measured(Readings::new(values.to_vec()).unwrap(), None)
}

#[test]
fn a_column_gives_its_statistics_to_cells_with_readings() {
    let mut table = toughness(Some(Location::Mean), Some(Convention::StandardError));
    table
        .add_row(vec![
            (id("temperature"), Property::stored(number(40.0))),
            (id("stiffness"), measured(&[1.0, 2.0, 3.0])),
        ])
        .unwrap();
    let at = by_index(&[40.0]);
    assert_eq!(value_at(&table, &at, "stiffness"), 2.0);
    let spread = table
        .at(&at, &id("stiffness"))
        .unwrap()
        .uncertainty()
        .unwrap();
    assert!((spread.unwrap().magnitude() - 1.0 / 3f64.sqrt()).abs() < 1e-12);

    // New readings, through an update, read theirs.
    table
        .update_row(&at, vec![(id("stiffness"), measured(&[4.0, 6.0]))])
        .unwrap();
    assert_eq!(value_at(&table, &at, "stiffness"), 5.0);

    // A value written beside readings outranks the statistic.
    let mut written = measured(&[4.0, 6.0]);
    written.set_written_value(Some(number(4.5)));
    table
        .update_row(&at, vec![(id("stiffness"), written)])
        .unwrap();
    assert_eq!(value_at(&table, &at, "stiffness"), 4.5);

    // A column declaring none leaves readings with no value.
    let mut bare = toughness(None, None);
    bare.add_row(vec![
        (id("temperature"), Property::stored(number(40.0))),
        (id("stiffness"), measured(&[1.0, 2.0, 3.0])),
    ])
    .unwrap();
    assert_eq!(
        bare.at(&at, &id("stiffness")).unwrap().value().unwrap(),
        Value::Absent
    );
}

#[test]
fn filling_keeps_the_declared_column_statistics() {
    let mut declared = toughness(Some(Location::Median), None);
    let mut file = toughness(Some(Location::Mean), Some(Convention::SampleStdev));
    file.add_row(vec![
        (id("temperature"), Property::stored(number(40.0))),
        (id("stiffness"), measured(&[1.0, 2.0, 9.0])),
    ])
    .unwrap();
    declared.fill_from(file, &Slab::default()).unwrap();
    let at = by_index(&[40.0]);
    // The declaration's median, not the file's mean.
    assert_eq!(value_at(&declared, &at, "stiffness"), 2.0);
    // The file's spread fills the channel the declaration left.
    let column = declared.column(&id("stiffness")).unwrap();
    assert_eq!(
        column.statistics(),
        DeclaredStatistics {
            value: Some(Location::Median),
            uncertainty: Some(Convention::SampleStdev),
        }
    );
    assert!(
        declared
            .at(&at, &id("stiffness"))
            .unwrap()
            .uncertainty()
            .unwrap()
            .is_some()
    );
}

#[test]
fn a_row_derivation_waits_for_a_cell_of_its_row_nobody_entered() {
    // Run on a row whose `tension` was never entered, the formula raised on the
    // absence and the cell said *failed*; a property waits for an input, and
    // so does a row's cell.
    let formula = Reciprocal::new();
    let mut table = derived(
        vec![formula.derivation()],
        &["temperature", "tension", "rigidity"],
    );
    table.add_row(cells(&[("temperature", 90.0)])).unwrap();
    table.resolve(&Slab::default()).unwrap();
    assert_eq!(
        formula.calls.get(),
        3,
        "the three rows with tension, not the fourth"
    );
    let waiting = table.at(&by_index(&[90.0]), &id("rigidity")).unwrap();
    assert!(waiting.value().unwrap().is_absent());
    assert!(waiting.records().failure.is_none());
}

#[test]
fn text_is_refused_in_a_measured_column_where_it_is_written() {
    let mut measured = ColumnMeta::default();
    measured.presentation.unit = Some("°P".to_string());
    let declared: IndexMap<Identifier, ColumnMeta> = [
        (id("day"), ColumnMeta::default()),
        (id("hardness"), measured),
        (id("note"), ColumnMeta::default()),
    ]
    .into_iter()
    .collect();
    let table = Table::new(id("trial"), vec![id("day")], declared, Vec::new()).unwrap();
    let text = vec![
        (id("day"), Property::stored(Value::integer(1))),
        (id("hardness"), Property::stored(Value::text("high"))),
    ];
    let refused = table.refuses_what_is_no_number(&text).unwrap_err();
    assert!(
        refused.to_string().contains("'hardness' is measured in °P"),
        "{refused}"
    );
    // A column without a unit takes text; a number is taken anywhere.
    let fine = vec![
        (id("hardness"), Property::stored(number(12.0))),
        (id("note"), Property::stored(Value::text("worn"))),
    ];
    assert!(table.refuses_what_is_no_number(&fine).is_ok());
}
