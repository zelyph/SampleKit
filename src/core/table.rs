//! Indexed series of measurements: named columns, rows identified by their
//! index values, and cells that are full quantities rather than bare numbers.
//!
//! It owns one thing no other module can: a row is **identified** by its index
//! values, and never by where it happens to sit.
//!

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::core::dependency_graph::Node;
use crate::core::formatting::Presentation;
use crate::core::identifier::Identifier;
use crate::core::property::{ComputeError, DeclaredStatistics, FillConflict, InputName, Property};
use crate::core::uncertainty::Uncertainty;
use crate::core::value::{Value, total_order};

/// What a column says for all of its cells.
///
/// No derivation, because a derivation is not a column's property — it belongs
/// to the table, and may fill several columns at once. A statistic is: a name
/// from a closed set, said once for every cell.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColumnMeta {
    pub presentation: Presentation,
    /// Which statistic of a cell's readings stands for its value, and which for
    /// its uncertainty. A cell with readings takes them whenever it enters the
    /// table.
    pub statistics: DeclaredStatistics,
}

/// What the sample looks like from inside a table: its properties and its
/// attributes, read-only, **and no tables at all**.
///
/// Declared here and implemented by `sample`, the same inversion `property`
/// makes with `Compute`: the lower module names what it needs, the higher one
/// supplies it, and no dependency points upward. A cell of another table's
/// column, with its index, as a derivation reads it.
pub type ForeignCell = (Vec<Value>, Value, Option<Uncertainty>);

pub trait Scope {
    fn read(&self, name: &Identifier) -> Result<Value, ComputeError>;
    fn read_list(&self, name: &Identifier) -> Result<Vec<Value>, ComputeError>;
    fn read_uncertainty(&self, name: &Identifier) -> Result<Option<Uncertainty>, ComputeError>;
    /// The generation counter of a sample-level name, which a derived cell
    /// records like any other input.
    fn generation(&self, name: &Identifier) -> u64;
    /// A counter that moves whenever any sample-level counter moves, or `None`
    /// when the scope cannot say: a table whose inputs have not moved since it
    /// last found nothing to run skips walking its rows.
    fn epoch(&self) -> Option<u64> {
        None
    }
    /// The counter of a whole column of another table of the same sample, which
    /// a derivation reading that column records.
    fn column_generation(&self, _table: &Identifier, _column: &Identifier) -> u64 {
        0
    }
    /// The cells of a whole column of another table of the same sample, each
    /// with its index, read-only.
    fn read_column(
        &self,
        table: &Identifier,
        _column: &Identifier,
    ) -> Result<Vec<ForeignCell>, ComputeError> {
        Err(ComputeError::failed(std::io::Error::other(format!(
            "no table '{table}' is reachable from here"
        ))))
    }
}

/// What a formula fills in one cell. Which channel it filled is discovered from
/// the run: a formula is opaque, and asking the declaration to promise what the
/// code returns would be a second thing to keep in step with it.
#[derive(Debug, Clone, PartialEq)]
pub enum CellOutput {
    Value(Value),
    Uncertainty(Uncertainty),
    Quantity(Value, Option<Uncertainty>),
}

/// One run per row, filling one column or several.
pub trait ComputeRow {
    fn compute(
        &self,
        row: &RowView,
        scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError>;
}

/// One run per table, filling every cell of one column or several.
pub trait ComputeColumn {
    fn compute(
        &self,
        columns: &ColumnSet,
        scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError>;
}

/// A formula, what it fills, and what it reads.
///
/// Two axes, and they are independent: a span decides how often it runs, and
/// `outputs` how many columns one run fills.
pub enum Derivation {
    Row {
        outputs: Vec<Identifier>,
        inputs: Vec<InputName>,
        formula: Rc<dyn ComputeRow>,
    },
    Column {
        outputs: Vec<Identifier>,
        inputs: Vec<InputName>,
        formula: Rc<dyn ComputeColumn>,
    },
}

impl Derivation {
    fn outputs(&self) -> &[Identifier] {
        match self {
            Derivation::Row { outputs, .. } | Derivation::Column { outputs, .. } => outputs,
        }
    }

    pub fn inputs(&self) -> &[InputName] {
        match self {
            Derivation::Row { inputs, .. } | Derivation::Column { inputs, .. } => inputs,
        }
    }
}

impl fmt::Debug for Derivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let span = match self {
            Derivation::Row { .. } => "Row",
            Derivation::Column { .. } => "Column",
        };
        write!(f, "Derivation::{span} {{ outputs: {:?} }}", self.outputs())
    }
}

/// A row. Private, because every read goes through a [`RowView`], which is what
/// makes *a derivation reads its span and no other row* structural.
struct Row {
    index: Vec<Value>,
    cells: IndexMap<Identifier, Property>,
    /// One counter per column, this row alone.
    generations: HashMap<Identifier, u64>,
    /// Per output column, the input counters its last run saw.
    recorded: HashMap<Identifier, HashMap<InputName, u64>>,
}

/// Index values are hashed for lookup, one part per index column.
///
/// A key is a **tuple of `Value`s, never a `Value` that is a tuple**: the arity
/// belongs to the table's declaration and each part is an ordinary scalar.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct IndexKey(Vec<KeyPart>);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum KeyPart {
    /// An integer, or a number that is a whole number in the integer range:
    /// `20` and `20.0` are the same row, and two distinct integers are two rows
    /// however large they are.
    Integer(i64),
    /// Any other number, by its bits.
    Number(u64),
    Text(String),
    /// Each kind of its own: `true` and `"true"` are two rows, as `equals`
    /// says they are two values.
    Boolean(bool),
    Date(String),
    DateTime(String),
    Absent,
    NotApplicable,
}

impl IndexKey {
    fn of(values: &[Value]) -> IndexKey {
        IndexKey(values.iter().map(KeyPart::of).collect())
    }
}

impl KeyPart {
    fn of(value: &Value) -> KeyPart {
        match value {
            Value::Integer(integer) => KeyPart::Integer(*integer),
            // A whole number in the integer range is that integer, which is
            // also what makes -0.0 the same row as 0. The conversion is exact
            // there, and a number outside that range keeps its bits.
            Value::Number(number) => {
                if number.fract() == 0.0
                    && *number >= -9_223_372_036_854_775_808.0
                    && *number < 9_223_372_036_854_775_808.0
                {
                    KeyPart::Integer(*number as i64)
                } else {
                    KeyPart::Number(number.to_bits())
                }
            }
            Value::Text(text) => KeyPart::Text(text.clone()),
            Value::Boolean(flag) => KeyPart::Boolean(*flag),
            Value::Date(date) => KeyPart::Date(date.iso()),
            Value::DateTime(date_time) => KeyPart::DateTime(date_time.iso()),
            Value::Absent => KeyPart::Absent,
            Value::NotApplicable => KeyPart::NotApplicable,
        }
    }
}

/// Where a cell stands against the derivation that fills it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellRun {
    /// No derivation fills this column.
    NotDerived,
    /// Its derivation has not run for this cell.
    NeverRan,
    /// It ran on the inputs as they stand.
    Current,
    /// An input has moved since it ran.
    Moved,
}

/// How a row is reached.
///
/// `Index` is what identifies the row and the only form ever written down;
/// `Ordinal` is where it sits, and is never serialized. A position is a lookup,
/// never an identity.
#[derive(Debug, Clone, PartialEq)]
pub enum RowAddress {
    Index(Vec<Value>),
    Ordinal(usize),
    /// Where it sits counted from the last row: `#-1` is the last, `1` here. A
    /// lookup too, never written down.
    FromEnd(usize),
}

impl RowAddress {
    pub fn index(values: impl Into<Vec<Value>>) -> RowAddress {
        RowAddress::Index(values.into())
    }

    pub fn ordinal(position: usize) -> RowAddress {
        RowAddress::Ordinal(position)
    }
}

/// A read-only borrow of one row: the whole of what a [`ComputeRow`] receives.
pub struct RowView<'a> {
    row: &'a Row,
    position: usize,
}

impl RowView<'_> {
    pub fn index(&self) -> Vec<&Value> {
        self.row.index.iter().collect()
    }

    /// The row's ordinal, counted from zero in declaration order. A number a
    /// reader may have, and never a key anything writes down.
    pub fn position(&self) -> usize {
        self.position
    }

    pub fn cell(&self, column: &Identifier) -> Result<&Property, TableError> {
        self.row
            .cells
            .get(column)
            .ok_or_else(|| unknown_column(column, self.row.cells.keys()))
    }

    pub fn column_names(&self) -> Vec<&Identifier> {
        self.row.cells.keys().collect()
    }
}

/// One column, borrowed with its indexes.
pub struct ColumnView<'a> {
    name: &'a Identifier,
    meta: &'a ColumnMeta,
    rows: &'a [Row],
}

impl<'a> ColumnView<'a> {
    /// Where the column's rows live, and how many: the same column for as long
    /// as its table is not changed.
    pub fn identity(&self) -> (usize, usize) {
        (self.rows.as_ptr().addr(), self.rows.len())
    }

    pub fn name(&self) -> &'a Identifier {
        self.name
    }

    /// The **column's** metadata, which is what a header needs. A cell's
    /// effective metadata is `presentation_of`, a different question. Which
    /// statistics of its cells' readings the column declares.
    pub fn statistics(&self) -> DeclaredStatistics {
        self.meta.statistics
    }

    pub fn presentation(&self) -> &'a Presentation {
        &self.meta.presentation
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Each cell **with the index value that identifies it**, never a bare
    /// sequence: a column detached from its indexes is a list of numbers nobody
    /// can locate.
    pub fn cells(&self) -> impl Iterator<Item = (Vec<&'a Value>, &'a Property)> {
        let name = self.name;
        self.rows.iter().map(move |row| {
            (
                row.index.iter().collect(),
                row.cells.get(name).expect("every row has every column"),
            )
        })
    }
}

/// Every column of the table, borrowed with its indexes: what a
/// [`ComputeColumn`] receives.
pub struct ColumnSet<'a> {
    table: &'a Table,
}

impl<'a> ColumnSet<'a> {
    /// The name of the table these columns belong to.
    pub fn table_name(&self) -> &'a Identifier {
        &self.table.name
    }

    pub fn column(&self, name: &Identifier) -> Result<ColumnView<'a>, TableError> {
        self.table.column(name)
    }

    pub fn len(&self) -> usize {
        self.table.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.rows.is_empty()
    }

    pub fn index_tuples(&self) -> Vec<Vec<&'a Value>> {
        self.table.index_tuples()
    }
}

/// One named table of one sample.
pub struct Table {
    name: Identifier,
    title: Option<String>,
    index: Vec<Identifier>,
    columns: IndexMap<Identifier, ColumnMeta>,
    rows: Vec<Row>,
    lookup: HashMap<IndexKey, usize>,
    derivations: Vec<Derivation>,
    /// One counter per column, every row: what a `Column` derivation records.
    column_generations: HashMap<Identifier, u64>,
    /// Per output column of a `Column` derivation, the counters its last run saw.
    column_recorded: HashMap<Identifier, HashMap<InputName, u64>>,
    /// Moves with every write a derivation could read, and with every
    /// forgotten run.
    epoch: u64,
    /// The table's and the scope's epochs when a resolution last ran nothing.
    settled: Option<(u64, u64)>,
    /// The cells a derivation wrote in this session, by row and column: what
    /// stamping records.
    ran: std::collections::HashSet<(IndexKey, Identifier)>,
    /// The derived cells held as overrides, which their derivation does not
    /// write.
    held: std::collections::HashSet<(IndexKey, Identifier)>,
}

impl fmt::Debug for Table {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("name", &self.name.as_str())
            .field("index", &self.index)
            .field("columns", &self.columns.keys().collect::<Vec<_>>())
            .field("rows", &self.rows.len())
            .field("derivations", &self.derivations)
            .finish()
    }
}

impl Table {
    pub fn new(
        name: Identifier,
        index: Vec<Identifier>,
        columns: IndexMap<Identifier, ColumnMeta>,
        derivations: Vec<Derivation>,
    ) -> Result<Table, TableError> {
        if name.as_str().starts_with('_') {
            return Err(TableError::PrivateName { name });
        }
        if index.is_empty() {
            return Err(TableError::EmptyIndex);
        }
        // `row` prefixes a cell's own-row inputs, so it cannot also be a column.
        if let Some(reserved) = columns.keys().find(|name| name.as_str() == "row") {
            return Err(TableError::ReservedColumnName {
                name: reserved.clone(),
            });
        }
        if let Some(private) = columns.keys().find(|name| name.as_str().starts_with('_')) {
            return Err(TableError::PrivateName {
                name: private.clone(),
            });
        }
        for column in &index {
            if !columns.contains_key(column) {
                return Err(TableError::IndexColumnNotDeclared {
                    index: column.clone(),
                });
            }
        }

        let mut filled: Vec<&Identifier> = Vec::new();
        for derivation in &derivations {
            for output in derivation.outputs() {
                if !columns.contains_key(output) {
                    return Err(unknown_column(output, columns.keys()));
                }
                if filled.contains(&output) {
                    return Err(TableError::ColumnDerivedTwice {
                        column: output.clone(),
                    });
                }
                filled.push(output);
            }
            for input in derivation.inputs() {
                check_input(&name, derivation, input, &columns)?;
            }
        }

        Ok(Table {
            name,
            title: None,
            index,
            columns,
            rows: Vec::new(),
            lookup: HashMap::new(),
            derivations,
            column_generations: HashMap::new(),
            column_recorded: HashMap::new(),
            epoch: 0,
            settled: None,
            ran: std::collections::HashSet::new(),
            held: std::collections::HashSet::new(),
        })
    }

    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    /// Forgets what this row's derivations recorded, and what a column
    /// derivation spanning it recorded, so the next `resolve` runs them again.
    pub fn invalidate_row(&mut self, row: &RowAddress) -> Result<(), TableError> {
        let position = self.position_of(row)?;
        self.rows[position].recorded.clear();
        self.column_recorded.clear();
        self.epoch += 1;
        Ok(())
    }

    /// Renames the table. A column-span input qualified by the old name
    /// follows, since that name could only ever have been this table's own.
    pub fn set_name(&mut self, name: Identifier) -> Result<(), TableError> {
        if name.as_str().starts_with('_') {
            return Err(TableError::PrivateName { name });
        }
        let old = std::mem::replace(&mut self.name, name.clone());
        self.epoch += 1;
        for derivation in &mut self.derivations {
            let (Derivation::Row { inputs, .. } | Derivation::Column { inputs, .. }) = derivation;
            for input in inputs.iter_mut() {
                if let InputName::Column { table, .. } = input
                    && *table == old
                {
                    *table = name.clone();
                }
            }
        }
        // An input now qualified by this table's own name is checked as its own.
        for derivation in &self.derivations {
            for input in derivation.inputs() {
                check_input(&self.name, derivation, input, &self.columns)?;
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------- mutation

    /// Every row has every column: an unsupplied cell is `Absent`, which is a
    /// recorded statement that nothing was measured, where a *missing* cell
    /// would be an unrecorded question.
    pub fn add_row(&mut self, cells: Vec<(Identifier, Property)>) -> Result<(), TableError> {
        let supplied = self.check_cells(cells)?;
        let index = self.index_of(&supplied)?;
        let key = IndexKey::of(&index);
        if let Some(existing) = self.lookup.get(&key) {
            return Err(TableError::DuplicateIndex {
                index,
                existing_position: *existing,
            });
        }

        let mut supplied = supplied;
        let mut row = Row {
            index,
            cells: IndexMap::new(),
            generations: HashMap::new(),
            recorded: HashMap::new(),
        };
        let mut held = Vec::new();
        for column in self.columns.keys() {
            let cell = supplied
                .shift_remove(column)
                .unwrap_or_else(|| Property::stored(Value::absent()));
            // A derived cell supplied by hand with the row is an override, as
            // one written by hand into an existing row is.
            if self.is_derived(column) && cell.peek_value().is_some_and(|value| !value.is_absent())
            {
                held.push(column.clone());
            }
            let mut cell = cell;
            self.declare_on(column, &mut cell);
            row.cells.insert(column.clone(), cell);
            row.generations.insert(column.clone(), 0);
        }
        for column in held {
            self.held.insert((key.clone(), column));
        }
        self.lookup.insert(key, self.rows.len());
        self.rows.push(row);
        for column in self.columns.keys().cloned().collect::<Vec<_>>() {
            self.bump_column(&column);
        }
        Ok(())
    }

    pub fn update_row(
        &mut self,
        row: &RowAddress,
        cells: Vec<(Identifier, Property)>,
    ) -> Result<(), TableError> {
        let position = self.position_of(row)?;
        let supplied = self.check_cells(cells)?;
        // An index cell written moves the row's key, and whatever is held by
        // it: never to nothing, never onto another row's.
        if self
            .index
            .iter()
            .any(|column| supplied.contains_key(column))
        {
            let mut index = Vec::new();
            for column in &self.index {
                let cell = supplied
                    .get(column)
                    .unwrap_or(&self.rows[position].cells[column]);
                let value = cell.value().map_err(|source| TableError::Compute {
                    column: column.clone(),
                    index: None,
                    source,
                })?;
                if value.is_absent() {
                    return Err(TableError::MissingIndexValue {
                        column: column.clone(),
                    });
                }
                index.push(value);
            }
            let key = IndexKey::of(&index);
            let old = IndexKey::of(&self.rows[position].index);
            if key != old {
                if let Some(&existing) = self.lookup.get(&key) {
                    return Err(TableError::DuplicateIndex {
                        index,
                        existing_position: existing,
                    });
                }
                self.lookup.remove(&old);
                self.lookup.insert(key.clone(), position);
                let moved: Vec<Identifier> = self
                    .held
                    .iter()
                    .filter(|(at, _)| *at == old)
                    .map(|(_, column)| column.clone())
                    .collect();
                for column in moved {
                    self.held.remove(&(old.clone(), column.clone()));
                    self.held.insert((key.clone(), column));
                }
                self.rows[position].index = index;
            }
        }
        for (column, mut cell) in supplied {
            self.declare_on(&column, &mut cell);
            // A derived cell written by hand is an override.
            if self.is_derived(&column) {
                self.held
                    .insert((IndexKey::of(&self.rows[position].index), column.clone()));
            }
            self.rows[position].cells.insert(column.clone(), cell);
            *self.rows[position]
                .generations
                .entry(column.clone())
                .or_insert(0) += 1;
            self.bump_column(&column);
        }
        Ok(())
    }

    /// Gives one cell back to the statistic its column declares of its
    /// readings: a value written beside them goes, an uncertainty the
    /// declaration names follows them again, and the record of the statistic
    /// taken before goes with the number it described. A computation asked for,
    /// as `restore_formula` is for a property.
    pub fn retake_statistic(
        &mut self,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<(), TableError> {
        let position = self.position_of(row)?;
        let statistics = self
            .columns
            .get(column)
            .ok_or_else(|| unknown_column(column, self.columns.keys()))?
            .statistics;
        let cell = self.rows[position]
            .cells
            .get_mut(column)
            .ok_or_else(|| unknown_column(column, self.columns.keys()))?;
        if cell.readings().is_none() || statistics.is_empty() {
            return Ok(());
        }
        cell.restore_formula();
        // A number the file stored for a declared spread is the statistic of
        // the readings as they were: dropped, so the declaration derives it.
        if statistics.uncertainty.is_some() {
            cell.set_uncertainty(None);
        }
        cell.declare_statistics(statistics.value, statistics.uncertainty);
        cell.set_records(crate::core::property::Records::none());
        *self.rows[position]
            .generations
            .entry(column.clone())
            .or_insert(0) += 1;
        self.bump_column(column);
        Ok(())
    }

    /// A cell's readings take the statistics its column declares, on every path
    /// a cell enters by: one left without them would read as readings with no
    /// statistic in a column that states one.
    fn declare_on(&self, column: &Identifier, cell: &mut Property) {
        let Some(meta) = self.columns.get(column) else {
            return;
        };
        if cell.readings().is_some() && !meta.statistics.is_empty() {
            cell.declare_statistics(meta.statistics.value, meta.statistics.uncertainty);
        }
    }

    fn bump_column(&mut self, column: &Identifier) {
        self.epoch += 1;
        *self.column_generations.entry(column.clone()).or_insert(0) += 1;
    }

    fn check_cells(
        &self,
        cells: Vec<(Identifier, Property)>,
    ) -> Result<IndexMap<Identifier, Property>, TableError> {
        let mut supplied = IndexMap::new();
        for (column, cell) in cells {
            if !self.columns.contains_key(&column) {
                return Err(unknown_column(&column, self.columns.keys()));
            }
            supplied.insert(column, cell);
        }
        Ok(supplied)
    }

    fn index_of(
        &self,
        supplied: &IndexMap<Identifier, Property>,
    ) -> Result<Vec<Value>, TableError> {
        let mut index = Vec::new();
        for column in &self.index {
            let cell = supplied
                .get(column)
                .ok_or_else(|| TableError::MissingIndexValue {
                    column: column.clone(),
                })?;
            let value = cell.value().map_err(|source| TableError::Compute {
                column: column.clone(),
                index: None,
                source,
            })?;
            if value.is_absent() {
                return Err(TableError::MissingIndexValue {
                    column: column.clone(),
                });
            }
            index.push(value);
        }
        Ok(index)
    }

    // ------------------------------------------------------------ addressing

    pub fn at(&self, row: &RowAddress, column: &Identifier) -> Result<&Property, TableError> {
        let position = self.position_of(row)?;
        self.rows[position]
            .cells
            .get(column)
            .ok_or_else(|| unknown_column(column, self.columns.keys()))
    }

    pub fn row(&self, row: &RowAddress) -> Result<RowView<'_>, TableError> {
        let position = self.position_of(row)?;
        Ok(RowView {
            row: &self.rows[position],
            position,
        })
    }

    pub fn rows(&self) -> impl Iterator<Item = RowView<'_>> {
        self.rows
            .iter()
            .enumerate()
            .map(|(position, row)| RowView { row, position })
    }

    pub fn column(&self, name: &Identifier) -> Result<ColumnView<'_>, TableError> {
        let meta = self
            .columns
            .get(name)
            .ok_or_else(|| unknown_column(name, self.columns.keys()))?;
        let name = self.columns.get_key_value(name).expect("just found").0;
        Ok(ColumnView {
            name,
            meta,
            rows: &self.rows,
        })
    }

    pub fn index_columns(&self) -> &[Identifier] {
        &self.index
    }

    pub fn index_tuples(&self) -> Vec<Vec<&Value>> {
        self.rows
            .iter()
            .map(|row| row.index.iter().collect())
            .collect()
    }

    /// The distinct values of **one** index column, which is what a filter
    /// offering *all the temperatures* needs and what the not-found diagnostic
    /// is built from.
    pub fn index_values(&self, column: &Identifier) -> Result<Vec<&Value>, TableError> {
        let at = self
            .index
            .iter()
            .position(|declared| declared == column)
            .ok_or_else(|| unknown_column(column, self.index.iter()))?;
        let mut values: Vec<&Value> = Vec::new();
        for row in &self.rows {
            let value = &row.index[at];
            if !values.contains(&value) {
                values.push(value);
            }
        }
        Ok(values)
    }

    pub fn column_names(&self) -> Vec<&Identifier> {
        self.columns.keys().collect()
    }

    /// The column's metadata composed with the cell's, field by field: a cell's
    /// `Some` wins and its `None` inherits.
    pub fn presentation_of(
        &self,
        column: &Identifier,
        row: &RowAddress,
    ) -> Result<Presentation, TableError> {
        let meta = self
            .columns
            .get(column)
            .ok_or_else(|| unknown_column(column, self.columns.keys()))?;
        let cell = self.at(row, column)?.presentation();
        Ok(Presentation {
            unit: cell.unit.clone().or_else(|| meta.presentation.unit.clone()),
            symbol: cell
                .symbol
                .clone()
                .or_else(|| meta.presentation.symbol.clone()),
            precision: cell
                .precision
                .clone()
                .or_else(|| meta.presentation.precision.clone()),
        })
    }

    fn position_of(&self, row: &RowAddress) -> Result<usize, TableError> {
        match row {
            RowAddress::Ordinal(ordinal) => {
                if *ordinal < self.rows.len() {
                    Ok(*ordinal)
                } else {
                    Err(TableError::OrdinalOutOfRange {
                        ordinal: *ordinal,
                        length: self.rows.len(),
                    })
                }
            }
            RowAddress::FromEnd(back) => {
                if (1..=self.rows.len()).contains(back) {
                    Ok(self.rows.len() - back)
                } else {
                    Err(TableError::FromEndOutOfRange {
                        back: *back,
                        length: self.rows.len(),
                    })
                }
            }
            RowAddress::Index(values) => {
                if values.len() != self.index.len() {
                    return Err(TableError::IndexArity {
                        expected: self.index.len(),
                        found: values.len(),
                    });
                }
                self.lookup
                    .get(&IndexKey::of(values))
                    .copied()
                    .ok_or_else(|| self.unknown_index(values))
            }
        }
    }

    /// Per dimension, because a flat "nearest row" over a composite index would
    /// need a distance across degrees and litres, which does not exist.
    fn unknown_index(&self, wanted: &[Value]) -> TableError {
        let mut per_column = Vec::new();
        for (at, column) in self.index.iter().enumerate() {
            let available: Vec<Value> = {
                let mut seen: Vec<Value> = Vec::new();
                for row in &self.rows {
                    if !seen.contains(&row.index[at]) {
                        seen.push(row.index[at].clone());
                    }
                }
                seen.sort_by(total_order);
                seen
            };
            let matched = available.contains(&wanted[at]);
            per_column.push(IndexReport {
                column: column.clone(),
                nearest: if matched {
                    None
                } else {
                    nearest(&available, &wanted[at])
                },
                available,
            });
        }
        TableError::UnknownIndex {
            index: wanted.to_vec(),
            per_column,
        }
    }

    // ------------------------------------------------------------ resolution

    /// The columns of the sample's other tables its derivations read, each once,
    /// in the order they are named.
    pub fn foreign_columns(&self) -> Vec<(Identifier, Identifier)> {
        let mut out: Vec<(Identifier, Identifier)> = Vec::new();
        for derivation in &self.derivations {
            for input in derivation.inputs() {
                if let InputName::Column { table, column } = input
                    && *table != self.name
                    && !out.iter().any(|(t, c)| t == table && c == column)
                {
                    out.push((table.clone(), column.clone()));
                }
            }
        }
        out
    }

    /// The other tables of the sample its derivations read.
    pub fn foreign_tables(&self) -> Vec<Identifier> {
        let mut out: Vec<Identifier> = Vec::new();
        for (table, _) in self.foreign_columns() {
            if !out.contains(&table) {
                out.push(table);
            }
        }
        out
    }

    /// The edges this table contributes to its sample's graph: one per output,
    /// with the inputs resolved from their scopes into nodes.
    ///
    /// A derivation filling three columns contributes three nodes sharing one
    /// input set, which is what makes a fit invalidate as one.
    pub fn declarations(&self) -> Vec<(Node, Vec<Node>)> {
        let mut edges = Vec::new();
        for derivation in &self.derivations {
            let inputs: Vec<Node> = derivation
                .inputs()
                .iter()
                .map(|input| match input {
                    InputName::Named(name) => Node::Named(name.clone()),
                    InputName::Cell(column) => Node::Column {
                        table: self.name.clone(),
                        column: column.clone(),
                    },
                    InputName::Column { table, column } => Node::Column {
                        table: table.clone(),
                        column: column.clone(),
                    },
                })
                .collect();
            for output in derivation.outputs() {
                edges.push((
                    Node::Column {
                        table: self.name.clone(),
                        column: output.clone(),
                    },
                    inputs.clone(),
                ));
            }
        }
        edges
    }

    /// What a derived column reads, **with its scopes intact**.
    ///
    /// `declarations` flattens `Cell` and `Column` into one node shape because
    /// a graph does not care which; a record does, since the two hash different
    /// things.
    pub fn inputs_of(&self, column: &Identifier) -> Option<&[InputName]> {
        self.derivations
            .iter()
            .find(|derivation| derivation.outputs().contains(column))
            .map(Derivation::inputs)
    }

    /// A column's counter: what a dependent records, at column scale.
    pub fn column_generation(&self, column: &Identifier) -> u64 {
        self.column_generations.get(column).copied().unwrap_or(0)
    }

    pub fn is_derived(&self, column: &Identifier) -> bool {
        self.derivations
            .iter()
            .any(|derivation| derivation.outputs().contains(column))
    }

    pub fn is_resolved(&self) -> bool {
        self.derivations.iter().all(|derivation| match derivation {
            Derivation::Column { outputs, .. } => outputs
                .iter()
                .all(|output| self.column_recorded.contains_key(output)),
            Derivation::Row { outputs, .. } => self.rows.iter().all(|row| {
                outputs
                    .iter()
                    .all(|output| row.recorded.contains_key(output))
            }),
        })
    }

    // --------------------------------------------------------------- filling

    /// A declaration filled by what a file recorded: the file wins wherever it
    /// speaks, the declaration keeps its derivations, and every span the file
    /// supplied is recorded as current so that resolving does not rerun it.
    /// Refused before anything moves.
    pub fn fill_from(&mut self, file: Table, scope: &dyn Scope) -> Result<(), TableError> {
        let filled = self.fill_rows(file)?;
        self.record_filled(&filled, scope);
        Ok(())
    }

    /// The fill without its recording, which a sample makes once every table is
    /// in. Returns, row by row, whether the file supplied it.
    pub fn fill_rows(&mut self, file: Table) -> Result<Vec<bool>, TableError> {
        self.check_fill(&file)?;
        // The rows are the file's now: none was written by a derivation.
        self.ran.clear();
        self.held.clear();
        let Table {
            title,
            columns: file_columns,
            rows: file_rows,
            ..
        } = file;
        if title.is_some() {
            self.title = title;
        }
        for (column, meta) in file_columns {
            match self.columns.get_mut(&column) {
                Some(declared) => {
                    let presentation = std::mem::take(&mut declared.presentation);
                    declared.presentation = Presentation {
                        unit: meta.presentation.unit.or(presentation.unit),
                        symbol: meta.presentation.symbol.or(presentation.symbol),
                        precision: meta.presentation.precision.or(presentation.precision),
                    };
                    // The model says what a number should be; the file's
                    // statement fills only what it leaves.
                    declared.statistics = DeclaredStatistics {
                        value: declared.statistics.value.or(meta.statistics.value),
                        uncertainty: declared
                            .statistics
                            .uncertainty
                            .or(meta.statistics.uncertainty),
                    };
                }
                None => {
                    self.columns.insert(column, meta);
                }
            }
        }

        let positions = std::mem::take(&mut self.lookup);
        let mut declared: Vec<Option<Row>> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(Some)
            .collect();
        // Each row with whether the file supplied it: only those are current.
        let mut rows: Vec<(Row, bool)> = Vec::with_capacity(file_rows.len() + declared.len());
        for file_row in file_rows {
            let held = positions
                .get(&IndexKey::of(&file_row.index))
                .and_then(|&at| declared[at].take());
            rows.push(match held {
                Some(mut row) => {
                    for (column, cell) in file_row.cells {
                        match row.cells.get_mut(&column) {
                            Some(existing) => existing
                                .fill_from(cell)
                                .expect("conflicts are checked before anything moves"),
                            None => {
                                row.cells.insert(column, cell);
                            }
                        }
                    }
                    (row, true)
                }
                None => (file_row, true),
            });
        }
        rows.extend(declared.into_iter().flatten().map(|row| (row, false)));

        let mut filled = Vec::with_capacity(rows.len());
        for (position, (mut row, from_file)) in rows.into_iter().enumerate() {
            // Every row has every column, in declaration order.
            let mut cells = IndexMap::with_capacity(self.columns.len());
            for column in self.columns.keys() {
                let mut cell = row
                    .cells
                    .shift_remove(column)
                    .unwrap_or_else(|| Property::stored(Value::absent()));
                self.declare_on(column, &mut cell);
                cells.insert(column.clone(), cell);
                row.generations.entry(column.clone()).or_insert(0);
            }
            row.cells = cells;
            row.recorded.clear();
            self.lookup.insert(IndexKey::of(&row.index), position);
            self.rows.push(row);
            filled.push(from_file);
        }
        self.column_recorded.clear();
        for column in self.columns.keys().cloned().collect::<Vec<_>>() {
            self.bump_column(&column);
        }
        Ok(filled)
    }

    /// Everything that can refuse a fill, so that a refusal changes nothing.
    pub(crate) fn check_fill(&self, file: &Table) -> Result<(), TableError> {
        if file.index != self.index {
            return Err(TableError::IndexMismatch {
                declared: self.index.clone(),
                file: file.index.clone(),
            });
        }
        for row in &file.rows {
            let Some(&at) = self.lookup.get(&IndexKey::of(&row.index)) else {
                continue;
            };
            for (column, cell) in &row.cells {
                let Some(declared) = self.rows[at].cells.get(column) else {
                    continue;
                };
                if let Some(conflict) = declared.conflict_with(cell) {
                    return Err(TableError::FillConflict {
                        column: column.clone(),
                        index: row.index.clone(),
                        conflict,
                    });
                }
            }
        }
        Ok(())
    }

    /// A span whose outputs the file supplied is recorded against the counters
    /// it would have read, so the next `resolve` runs only what the file lacks.
    pub(crate) fn record_filled(&mut self, filled: &[bool], scope: &dyn Scope) {
        for at in 0..self.derivations.len() {
            let (outputs, inputs, row_span) = match &self.derivations[at] {
                Derivation::Row {
                    outputs, inputs, ..
                } => (outputs.clone(), inputs.clone(), true),
                Derivation::Column {
                    outputs, inputs, ..
                } => (outputs.clone(), inputs.clone(), false),
            };
            let supplied = |row: &Row| outputs.iter().all(|output| holds_value(&row.cells[output]));
            if row_span {
                for (position, from_file) in filled.iter().enumerate() {
                    if !from_file || !supplied(&self.rows[position]) {
                        continue;
                    }
                    let current = self.counters(&inputs, Some(position), scope);
                    for output in &outputs {
                        self.rows[position]
                            .recorded
                            .insert(output.clone(), current.clone());
                    }
                }
            } else if filled.iter().all(|from_file| *from_file) && self.rows.iter().all(supplied) {
                let current = self.counters(&inputs, None, scope);
                for output in &outputs {
                    self.column_recorded.insert(output.clone(), current.clone());
                }
            }
        }
    }

    /// Run every derivation whose recorded input counters no longer match.
    ///
    /// A table cannot resolve itself: a derivation reads the sample, so it
    /// needs a `Scope`, and a table is *given* one by whoever holds both.
    pub fn resolve(&mut self, scope: &dyn Scope) -> Result<(), TableError> {
        if self.is_settled(scope) {
            return Ok(());
        }
        let (start, scope_epoch) = (self.epoch, scope.epoch());
        for at in 0..self.derivations.len() {
            match &self.derivations[at] {
                Derivation::Row { .. } => self.resolve_row_span(at, scope)?,
                Derivation::Column { .. } => self.resolve_column_span(at, scope)?,
            }
        }
        // Settled only by a pass that wrote nothing: a pass that wrote may
        // have moved an input of a derivation it ran before, which the next
        // pass reruns.
        if let Some(epoch) = scope_epoch
            && self.epoch == start
            && scope.epoch() == Some(epoch)
        {
            self.settled = Some((start, epoch));
        }
        Ok(())
    }

    /// The derivations named columns need, in declaration order: theirs, and
    /// those filling a column of this table they read, transitively.
    fn needed(&self, columns: &[Identifier]) -> Vec<usize> {
        let mut needed: Vec<usize> = Vec::new();
        let mut wanted: Vec<Identifier> = columns.to_vec();
        while let Some(column) = wanted.pop() {
            let Some(at) = self
                .derivations
                .iter()
                .position(|derivation| derivation.outputs().contains(&column))
            else {
                continue;
            };
            if needed.contains(&at) {
                continue;
            }
            needed.push(at);
            for input in self.derivations[at].inputs() {
                match input {
                    InputName::Cell(own) => wanted.push(own.clone()),
                    InputName::Column { table, column } if *table == self.name => {
                        wanted.push(column.clone())
                    }
                    _ => {}
                }
            }
        }
        needed.sort_unstable();
        needed
    }

    /// Every column the derivations named columns need fill.
    pub fn needed_outputs(&self, columns: &[Identifier]) -> Vec<Identifier> {
        self.needed(columns)
            .into_iter()
            .flat_map(|at| self.derivations[at].outputs().to_vec())
            .collect()
    }

    /// Runs only the derivations named columns need. The table is not settled
    /// by it: what was not needed may still be pending.
    pub fn resolve_for(
        &mut self,
        columns: &[Identifier],
        scope: &dyn Scope,
    ) -> Result<(), TableError> {
        let needed = self.needed(columns);
        // Passes until one writes nothing, as a full resolution settles: a
        // derivation run first may read what one after it writes.
        for _ in 0..=needed.len() {
            let start = self.epoch;
            for &at in &needed {
                match &self.derivations[at] {
                    Derivation::Row { .. } => self.resolve_row_span(at, scope)?,
                    Derivation::Column { .. } => self.resolve_column_span(at, scope)?,
                }
            }
            if self.epoch == start {
                break;
            }
        }
        Ok(())
    }

    /// Forgets what the derivation filling `column` recorded — in `row`, or in
    /// every row — so that it runs again, and it alone.
    pub fn invalidate_column(
        &mut self,
        column: &Identifier,
        row: Option<&RowAddress>,
    ) -> Result<(), TableError> {
        if !self.columns.contains_key(column) {
            return Err(unknown_column(column, self.columns.keys()));
        }
        let Some(derivation) = self
            .derivations
            .iter()
            .find(|derivation| derivation.outputs().contains(column))
        else {
            return Ok(());
        };
        let (outputs, row_span) = match derivation {
            Derivation::Row { outputs, .. } => (outputs.clone(), true),
            Derivation::Column { outputs, .. } => (outputs.clone(), false),
        };
        if row_span {
            let positions: Vec<usize> = match row {
                Some(address) => vec![self.position_of(address)?],
                None => (0..self.rows.len()).collect(),
            };
            for position in positions {
                for output in &outputs {
                    self.rows[position].recorded.remove(output);
                }
            }
        } else {
            for output in &outputs {
                self.column_recorded.remove(output);
            }
        }
        self.epoch += 1;
        Ok(())
    }

    /// Holds a derived cell as an override: its derivation does not write it
    /// until it is released.
    pub fn hold_cell(&mut self, row: &RowAddress, column: &Identifier) -> Result<(), TableError> {
        if !self.columns.contains_key(column) {
            return Err(unknown_column(column, self.columns.keys()));
        }
        let position = self.position_of(row)?;
        self.held
            .insert((IndexKey::of(&self.rows[position].index), column.clone()));
        Ok(())
    }

    /// Gives the held cells of a column back to their derivation, in one row or
    /// in every row.
    pub fn release_cells(
        &mut self,
        column: &Identifier,
        row: Option<&RowAddress>,
    ) -> Result<(), TableError> {
        match row {
            Some(address) => {
                let position = self.position_of(address)?;
                self.held
                    .remove(&(IndexKey::of(&self.rows[position].index), column.clone()));
            }
            None => self.held.retain(|(_, held)| held != column),
        }
        self.epoch += 1;
        Ok(())
    }

    /// Whether a derived cell is held as an override.
    pub fn is_held(&self, row: &RowAddress, column: &Identifier) -> Result<bool, TableError> {
        let position = self.position_of(row)?;
        Ok(self.held_at(position, column))
    }

    fn held_at(&self, position: usize, column: &Identifier) -> bool {
        self.held
            .contains(&(IndexKey::of(&self.rows[position].index), column.clone()))
    }

    /// Whether a derivation wrote this cell in this session.
    pub fn ran(&self, row: &RowAddress, column: &Identifier) -> Result<bool, TableError> {
        let position = self.position_of(row)?;
        Ok(self
            .ran
            .contains(&(IndexKey::of(&self.rows[position].index), column.clone())))
    }

    /// Whether a resolution would run nothing, answered without walking a
    /// row: nothing the table or its scope holds has moved since a resolution
    /// last found nothing to run.
    pub fn is_settled(&self, scope: &dyn Scope) -> bool {
        scope
            .epoch()
            .is_some_and(|epoch| self.settled == Some((self.epoch, epoch)))
    }

    /// Where one cell stands against the derivation that fills it.
    pub fn cell_run(
        &self,
        row: &RowAddress,
        column: &Identifier,
        scope: &dyn Scope,
    ) -> Result<CellRun, TableError> {
        let position = self.position_of(row)?;
        if !self.columns.contains_key(column) {
            return Err(unknown_column(column, self.columns.keys()));
        }
        let Some(derivation) = self
            .derivations
            .iter()
            .find(|derivation| derivation.outputs().contains(column))
        else {
            return Ok(CellRun::NotDerived);
        };
        let (recorded, span) = match derivation {
            Derivation::Row { .. } => (self.rows[position].recorded.get(column), Some(position)),
            Derivation::Column { .. } => (self.column_recorded.get(column), None),
        };
        Ok(match recorded {
            None => CellRun::NeverRan,
            Some(seen) if *seen == self.counters(derivation.inputs(), span, scope) => {
                CellRun::Current
            }
            Some(_) => CellRun::Moved,
        })
    }

    /// Records a cell's inputs as they stand, so that it is not rerun: what
    /// a value whose record still matches its inputs is owed.
    pub fn mark_cell_current(
        &mut self,
        row: &RowAddress,
        column: &Identifier,
        scope: &dyn Scope,
    ) -> Result<(), TableError> {
        let position = self.position_of(row)?;
        let Some(derivation) = self
            .derivations
            .iter()
            .find(|derivation| derivation.outputs().contains(column))
        else {
            return Err(unknown_column(column, self.columns.keys()));
        };
        let span = matches!(derivation, Derivation::Row { .. }).then_some(position);
        let current = self.counters(derivation.inputs(), span, scope);
        match span {
            Some(position) => {
                self.rows[position].recorded.insert(column.clone(), current);
            }
            None => {
                self.column_recorded.insert(column.clone(), current);
            }
        }
        Ok(())
    }

    /// A cell's records, as the loading path and recording set them. Moves no
    /// counter: a record says where a value came from, not what it is.
    pub fn set_cell_records(
        &mut self,
        row: &RowAddress,
        column: &Identifier,
        records: crate::core::property::Records,
    ) -> Result<(), TableError> {
        let position = self.position_of(row)?;
        let cell = self.rows[position]
            .cells
            .get_mut(column)
            .ok_or_else(|| unknown_column(column, self.columns.keys()))?;
        cell.set_records(records);
        Ok(())
    }

    fn resolve_row_span(&mut self, at: usize, scope: &dyn Scope) -> Result<(), TableError> {
        let Derivation::Row {
            outputs,
            inputs,
            formula,
        } = &self.derivations[at]
        else {
            unreachable!("checked by the caller")
        };
        let (outputs, inputs, formula) = (outputs.clone(), inputs.clone(), formula.clone());

        for position in 0..self.rows.len() {
            let current = self.counters(&inputs, Some(position), scope);
            if outputs
                .iter()
                .all(|output| self.rows[position].recorded.get(output) == Some(&current))
            {
                continue;
            }
            // A held cell is an override: its derivation does not write it.
            if outputs.iter().all(|output| self.held_at(position, output)) {
                continue;
            }
            let attempt = {
                let view = RowView {
                    row: &self.rows[position],
                    position,
                };
                formula.compute(&view, scope)
            };
            let produced = match attempt {
                Ok(produced) => produced,
                Err(source) => {
                    // A cell whose formula raised keeps the value it last gave
                    // and **says so**, exactly as a property does. Nothing else
                    // can record it: a derived cell has no formula of its own
                    // to ask afterwards, so the failure is written here or
                    // nowhere — and `status` would go on calling it merely
                    // stale, with the failure gone from the repository.
                    let message = source.to_string();
                    let held: Vec<bool> = outputs
                        .iter()
                        .map(|column| self.held_at(position, column))
                        .collect();
                    for (column, held) in outputs.iter().zip(held) {
                        if held {
                            continue;
                        }
                        if let Some(cell) = self.rows[position].cells.get_mut(column) {
                            let mut records = cell.records().clone();
                            records.failure = Some(message.clone());
                            cell.set_records(records);
                        }
                    }
                    return Err(TableError::Compute {
                        column: outputs[0].clone(),
                        index: Some(self.rows[position].index.clone()),
                        source,
                    });
                }
            };
            check_outputs(&outputs, &produced)?;
            for (column, output) in produced {
                if self.held_at(position, &column) {
                    continue;
                }
                self.write(position, &column, output);
            }
            for output in &outputs {
                if self.held_at(position, output) {
                    continue;
                }
                self.rows[position]
                    .recorded
                    .insert(output.clone(), current.clone());
            }
        }
        Ok(())
    }

    fn resolve_column_span(&mut self, at: usize, scope: &dyn Scope) -> Result<(), TableError> {
        let Derivation::Column {
            outputs,
            inputs,
            formula,
        } = &self.derivations[at]
        else {
            unreachable!("checked by the caller")
        };
        let (outputs, inputs, formula) = (outputs.clone(), inputs.clone(), formula.clone());

        let current = self.counters(&inputs, None, scope);
        if outputs
            .iter()
            .all(|output| self.column_recorded.get(output) == Some(&current))
        {
            return Ok(());
        }
        let attempt = {
            let set = ColumnSet { table: self };
            formula.compute(&set, scope)
        };
        let produced = match attempt {
            Ok(produced) => produced,
            Err(source) => {
                // The whole column failed, so every one of its cells says so —
                // the same rule as a row's, and for the same reason.
                let message = source.to_string();
                let rows = self.rows.len();
                for column in &outputs {
                    for position in 0..rows {
                        if self.held_at(position, column) {
                            continue;
                        }
                        if let Some(cell) = self.rows[position].cells.get_mut(column) {
                            let mut records = cell.records().clone();
                            records.failure = Some(message.clone());
                            cell.set_records(records);
                        }
                    }
                }
                return Err(TableError::Compute {
                    column: outputs[0].clone(),
                    index: None,
                    source,
                });
            }
        };
        check_outputs(&outputs, &produced)?;
        for (column, cells) in &produced {
            if cells.len() != self.rows.len() {
                return Err(TableError::OutputLength {
                    column: column.clone(),
                    expected: self.rows.len(),
                    produced: cells.len(),
                });
            }
        }
        for (column, cells) in produced {
            for (position, output) in cells.into_iter().enumerate() {
                if self.held_at(position, &column) {
                    continue;
                }
                self.write(position, &column, output);
            }
        }
        for output in &outputs {
            self.column_recorded.insert(output.clone(), current.clone());
        }
        Ok(())
    }

    /// A cell goes stale for exactly the counters it recorded, which is what
    /// makes the two granularities need no rule of their own.
    fn counters(
        &self,
        inputs: &[InputName],
        position: Option<usize>,
        scope: &dyn Scope,
    ) -> HashMap<InputName, u64> {
        let mut counters = HashMap::new();
        for input in inputs {
            let generation = match input {
                InputName::Named(name) => scope.generation(name),
                InputName::Cell(column) => position
                    .and_then(|position| self.rows[position].generations.get(column).copied())
                    .unwrap_or(0),
                InputName::Column { table, column } if *table != self.name => {
                    scope.column_generation(table, column)
                }
                InputName::Column { column, .. } => {
                    self.column_generations.get(column).copied().unwrap_or(0)
                }
            };
            counters.insert(input.clone(), generation);
        }
        counters
    }

    fn write(&mut self, position: usize, column: &Identifier, output: CellOutput) {
        let cell = self.rows[position]
            .cells
            .get_mut(column)
            .expect("every row has every column");
        match output {
            CellOutput::Value(value) => cell.set_value(value),
            CellOutput::Uncertainty(uncertainty) => cell.set_uncertainty(Some(uncertainty)),
            CellOutput::Quantity(value, uncertainty) => {
                cell.set_value(value);
                cell.set_uncertainty(uncertainty);
            }
        }
        self.ran
            .insert((IndexKey::of(&self.rows[position].index), column.clone()));
        // Writing a cell bumps both scales: its row's entry and its column's.
        *self.rows[position]
            .generations
            .entry(column.clone())
            .or_insert(0) += 1;
        self.bump_column(column);
    }
}

/// Whether a cell holds a value that reading it would not compute.
fn holds_value(cell: &Property) -> bool {
    cell.is_resolved() && cell.value().is_ok_and(|value| !value.is_absent())
}

fn check_outputs<T>(
    declared: &[Identifier],
    produced: &IndexMap<Identifier, T>,
) -> Result<(), TableError> {
    let matches = produced.len() == declared.len()
        && declared.iter().all(|output| produced.contains_key(output));
    if matches {
        return Ok(());
    }
    Err(TableError::OutputMismatch {
        expected: declared.to_vec(),
        produced: produced.keys().cloned().collect(),
    })
}

/// A span decides which input scopes are admissible, and the wrong one is
/// refused where the table is built rather than producing a plausible reading.
fn check_input(
    table: &Identifier,
    derivation: &Derivation,
    input: &InputName,
    columns: &IndexMap<Identifier, ColumnMeta>,
) -> Result<(), TableError> {
    match (derivation, input) {
        (_, InputName::Named(_)) => Ok(()),
        (Derivation::Row { .. }, InputName::Cell(column)) => {
            if columns.contains_key(column) {
                Ok(())
            } else {
                Err(unknown_column(column, columns.keys()))
            }
        }
        // A whole column of another table: the sample, which holds both, checks
        // it and resolves that table first.
        (_, InputName::Column { table: named, .. }) if named != table => Ok(()),
        (Derivation::Column { .. }, InputName::Column { column, .. }) => {
            if columns.contains_key(column) {
                Ok(())
            } else {
                Err(unknown_column(column, columns.keys()))
            }
        }
        // A cell resting on a whole column of its own table is what a `Column`
        // derivation is.
        (Derivation::Row { .. }, InputName::Column { .. }) => Err(TableError::WrongInputScope {
            input: input.clone(),
            span: "row",
        }),
        (Derivation::Column { .. }, InputName::Cell(_)) => Err(TableError::WrongInputScope {
            input: input.clone(),
            span: "column",
        }),
    }
}

fn unknown_column<'a>(
    name: &Identifier,
    available: impl Iterator<Item = &'a Identifier>,
) -> TableError {
    let available: Vec<Identifier> = available.cloned().collect();
    let suggestion = nearest_name(&available, name);
    TableError::UnknownColumn {
        name: name.clone(),
        available,
        suggestion,
    }
}

/// By edit distance for names, through the one definition of nearest.
fn nearest_name(available: &[Identifier], wanted: &Identifier) -> Option<Identifier> {
    let names: Vec<&str> = available.iter().map(Identifier::as_str).collect();
    crate::core::identifier::nearest(wanted.as_str(), names)
        .and_then(|name| Identifier::new(&name).ok())
}

/// The index value nearest `wanted` among `available`, by the rule a table's
/// own miss is said by: every surface that looks a row up — a filter, a column
/// asked for, `explain` — offers the same one.
pub fn nearest_index(available: &[Value], wanted: &Value) -> Option<Value> {
    nearest(available, wanted)
}

/// By numeric proximity for indexes; a text index by its spelling — the same
/// letters in another case first, then by edit distance — so that
/// `tasting.score[lea]` names `'Lea'`. Any other index has no nearest.
fn nearest(available: &[Value], wanted: &Value) -> Option<Value> {
    if let Value::Text(text) = wanted {
        let texts: Vec<&str> = available
            .iter()
            .filter_map(|candidate| match candidate {
                Value::Text(held) => Some(held.as_str()),
                _ => None,
            })
            .collect();
        let found = texts
            .iter()
            .find(|held| held.to_lowercase() == text.to_lowercase())
            .map(|held| held.to_string())
            .or_else(|| crate::core::identifier::nearest(text, texts.iter().copied()))?;
        return Some(Value::Text(found));
    }
    let target = numeric(wanted)?;
    available
        .iter()
        .filter_map(|candidate| numeric(candidate).map(|number| (number, candidate)))
        .min_by(|a, b| {
            (a.0 - target)
                .abs()
                .partial_cmp(&(b.0 - target).abs())
                .expect("finite")
        })
        .map(|(_, candidate)| candidate.clone())
}

fn numeric(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(integer) => Some(*integer as f64),
        Value::Number(number) => Some(*number),
        _ => None,
    }
}

/// What one index column had to say about a miss.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexReport {
    pub column: Identifier,
    pub available: Vec<Value>,
    /// `None` when the component matched.
    pub nearest: Option<Value>,
}

/// Everything a table refuses to guess at.
#[derive(Debug, Clone, PartialEq)]
pub enum TableError {
    PrivateName {
        name: Identifier,
    },
    EmptyIndex,
    ReservedColumnName {
        name: Identifier,
    },
    IndexColumnNotDeclared {
        index: Identifier,
    },
    UnknownColumn {
        name: Identifier,
        available: Vec<Identifier>,
        suggestion: Option<Identifier>,
    },
    UnknownIndex {
        index: Vec<Value>,
        per_column: Vec<IndexReport>,
    },
    IndexArity {
        expected: usize,
        found: usize,
    },
    OrdinalOutOfRange {
        ordinal: usize,
        length: usize,
    },
    FromEndOutOfRange {
        back: usize,
        length: usize,
    },
    DuplicateIndex {
        index: Vec<Value>,
        existing_position: usize,
    },
    MissingIndexValue {
        column: Identifier,
    },
    OutputMismatch {
        expected: Vec<Identifier>,
        produced: Vec<Identifier>,
    },
    OutputLength {
        column: Identifier,
        expected: usize,
        produced: usize,
    },
    ColumnDerivedTwice {
        column: Identifier,
    },
    WrongInputScope {
        input: InputName,
        span: &'static str,
    },
    ForeignColumn {
        table: Identifier,
        column: Identifier,
    },
    Compute {
        column: Identifier,
        /// Absent for a column-span failure, which has no single row.
        index: Option<Vec<Value>>,
        source: ComputeError,
    },
    IndexMismatch {
        declared: Vec<Identifier>,
        file: Vec<Identifier>,
    },
    FillConflict {
        column: Identifier,
        index: Vec<Value>,
        conflict: FillConflict,
    },
}

fn list(names: &[Identifier]) -> String {
    names
        .iter()
        .map(Identifier::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::PrivateName { name } => write!(
                f,
                "'{name}' cannot be a table or column name: a leading '_' is reserved for private Python state"
            ),
            TableError::EmptyIndex => write!(
                f,
                "a table needs at least one index column: without one, nothing \
                 identifies a row"
            ),
            TableError::ReservedColumnName { name } => write!(
                f,
                "'{name}' cannot be a column name: it prefixes a derivation's \
                 own-row inputs, as in row.wort"
            ),
            TableError::IndexColumnNotDeclared { index } => write!(
                f,
                "the index names '{index}', which is not one of the declared columns"
            ),
            TableError::UnknownColumn {
                name,
                available,
                suggestion,
            } => {
                write!(
                    f,
                    "unknown column '{name}'\n  available: {}",
                    list(available)
                )?;
                match suggestion {
                    Some(nearest) => write!(f, "\n  did you mean: {nearest}?"),
                    None => Ok(()),
                }
            }
            TableError::UnknownIndex { index, per_column } => {
                let wanted: Vec<String> = index.iter().map(describe).collect();
                write!(f, "no row at ({})", wanted.join(", "))?;
                for (report, asked) in per_column.iter().zip(index) {
                    let available: Vec<String> = report.available.iter().map(describe).collect();
                    write!(f, "\n  {}  {}", report.column, available.join(", "))?;
                    // *Matched* only where it did: a text with no nearest was
                    // said matched beside the list it was missing from.
                    match &report.nearest {
                        Some(nearest @ Value::Text(_)) => {
                            write!(f, "   did you mean {}?", describe(nearest))?
                        }
                        Some(nearest) => write!(f, "   nearest: {}", describe(nearest))?,
                        None if report.available.contains(asked) => write!(f, "   matched")?,
                        None => {}
                    }
                }
                Ok(())
            }
            TableError::IndexArity { expected, found } => write!(
                f,
                "this table's index has {expected} column(s) and the address has \
                 {found}: count the index columns"
            ),
            TableError::OrdinalOutOfRange { ordinal, length } => {
                write!(f, "there is no row #{ordinal}: the table has {length}")
            }
            TableError::FromEndOutOfRange { back, length } => {
                write!(f, "there is no row #-{back}: the table has {length}")
            }
            TableError::DuplicateIndex {
                index,
                existing_position,
            } => {
                let wanted: Vec<String> = index.iter().map(describe).collect();
                // Counted from one, as a reader counts rows: `#7` read as the
                // address `[#7]`, which is the eighth.
                write!(
                    f,
                    "a row at ({}) already exists: row {} of the table",
                    wanted.join(", "),
                    existing_position + 1
                )
            }
            TableError::MissingIndexValue { column } => write!(
                f,
                "this row has no value for '{column}', which is an index column: \
                 nothing would identify it"
            ),
            TableError::OutputMismatch { expected, produced } => write!(
                f,
                "a derivation declared [{}] and returned [{}]: the declaration is \
                 the contract",
                list(expected),
                list(produced)
            ),
            TableError::OutputLength {
                column,
                expected,
                produced,
            } => write!(
                f,
                "a column derivation returned {produced} cells for '{column}' and \
                 the table has {expected} rows"
            ),
            TableError::ColumnDerivedTwice { column } => write!(
                f,
                "two derivations both fill '{column}': only one formula may"
            ),
            TableError::WrongInputScope { input, span } => write!(
                f,
                "'{input}' is not admissible in a {span} derivation: a row span \
                 reads its own row's cells, a column span reads whole columns"
            ),
            TableError::ForeignColumn { table, column } => write!(
                f,
                "'{table}.{column}' is in another table, which a derivation cannot \
                 reach: a scope exposes properties and attributes, and no tables"
            ),
            TableError::Compute {
                column,
                index,
                source,
            } => match index {
                Some(index) => {
                    let at: Vec<String> = index.iter().map(describe).collect();
                    write!(f, "computing '{column}' at ({}): {source}", at.join(", "))
                }
                None => write!(f, "computing column '{column}': {source}"),
            },
            TableError::IndexMismatch { declared, file } => write!(
                f,
                "the file indexes this table by [{}] and the model by [{}]: an index \
                 identifies every row, so the two must agree",
                list(file),
                list(declared)
            ),
            TableError::FillConflict {
                column,
                index,
                conflict,
            } => {
                let at: Vec<String> = index.iter().map(describe).collect();
                write!(f, "cell '{column}' at ({}): {conflict}", at.join(", "))
            }
        }
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Text(text) => format!("'{text}'"),
        Value::Boolean(flag) => flag.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        Value::Absent => "(absent)".to_string(),
        Value::NotApplicable => "n/a".to_string(),
    }
}

impl std::error::Error for TableError {}
