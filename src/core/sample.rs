//! One sample: its name, its tags, its attributes, its properties, its
//! tables, its note, and the derivations declared between them.
//!
//! It is deliberately thin. It owns *composition and consistency* — that two
//! kinds cannot share a name, that removing a property removes its edges, that
//! changing a value invalidates what depends on it. It owns no arithmetic, no
//! formatting, no persistence.
//!

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::core::dependency_graph::{Change, DependencyError, DependencyGraph, Node};
use crate::core::formatting::Presentation;
use crate::core::identifier::Identifier;
use crate::core::property::{ChannelInputs, ComputeError, FillConflict, Property, Records};
use crate::core::table::{CellRun, RowAddress, Scope, Table, TableError};
use crate::core::uncertainty::{Convention, Uncertainty};
use crate::core::value::{Readings, Value, ValueKind};

/// The names no property, table or attribute may take.
///
/// The first two belong here. `path` and `filename` do not, and holding them
/// anyway is the argument `identifier` makes for excluding `.`: a low layer
/// restricts the alphabet so that a high one can be unambiguous. `readings` is
/// reserved for the record a declared statistic writes: a property of that name
/// would make the key ambiguous, and resolving it by looking up whichever
/// exists is the failure principle 4 names. `project` is the project a sample
/// belongs to, a field every sample answers, and `state` how it stands, as
/// `status` and `validate` say.
pub const RESERVED: [&str; 7] = [
    "name", "tags", "path", "filename", "readings", "project", "state",
];

/// A sample fact: one scalar, or one ordered homogeneous list of scalars.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Scalar(Value),
    List(Vec<Value>),
}

impl AttributeValue {
    pub fn scalar(value: Value) -> AttributeValue {
        AttributeValue::Scalar(value)
    }

    pub fn list(values: Vec<Value>) -> Result<AttributeValue, AttributeError> {
        let expected = values
            .iter()
            .find(|value| !value.is_absent())
            .map(Value::kind);
        for (position, value) in values.iter().enumerate() {
            if value.is_absent() {
                return Err(AttributeError::AbsentItem { position });
            }
            if let Some(expected) = expected
                && !compatible_kinds(expected, value.kind())
            {
                return Err(AttributeError::MixedKinds {
                    position,
                    expected,
                    found: value.kind(),
                });
            }
        }
        Ok(AttributeValue::List(values))
    }

    pub fn as_scalar(&self) -> Option<&Value> {
        match self {
            AttributeValue::Scalar(value) => Some(value),
            AttributeValue::List(_) => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            AttributeValue::Scalar(_) => None,
            AttributeValue::List(values) => Some(values),
        }
    }

    pub fn is_empty(&self) -> bool {
        match self {
            AttributeValue::Scalar(value) => matches!(value, Value::Absent),
            AttributeValue::List(values) => values.is_empty(),
        }
    }
}

impl From<Value> for AttributeValue {
    fn from(value: Value) -> AttributeValue {
        AttributeValue::scalar(value)
    }
}

impl PartialEq<Value> for AttributeValue {
    fn eq(&self, other: &Value) -> bool {
        matches!(self, AttributeValue::Scalar(value) if value == other)
    }
}

impl PartialEq<AttributeValue> for Value {
    fn eq(&self, other: &AttributeValue) -> bool {
        other == self
    }
}

fn compatible_kinds(left: ValueKind, right: ValueKind) -> bool {
    left == right
        || matches!(
            (left, right),
            (ValueKind::Integer, ValueKind::Number)
                | (ValueKind::Number, ValueKind::Integer)
                | (ValueKind::Date, ValueKind::DateTime)
                | (ValueKind::DateTime, ValueKind::Date)
        )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeError {
    AbsentItem {
        position: usize,
    },
    MixedKinds {
        position: usize,
        expected: ValueKind,
        found: ValueKind,
    },
}

impl fmt::Display for AttributeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttributeError::AbsentItem { position } => write!(
                f,
                "attribute list item {position} has no value: omit the item or give it a value"
            ),
            AttributeError::MixedKinds {
                position,
                expected,
                found,
            } => write!(
                f,
                "attribute list item {position} is {found}, after {expected}: a list holds one kind"
            ),
        }
    }
}

impl std::error::Error for AttributeError {}

/// The Markdown body, verbatim, byte for byte.
///
/// A newtype so that no code path can trim it, normalize its line endings, or
/// append to it casually.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Note(String);

impl Note {
    pub fn new(text: impl Into<String>) -> Note {
        Note(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a handle shares with its sample: the counters, what each derivation
/// recorded, and the graph. **Not the sample.**
#[derive(Debug, Default)]
struct SampleCore {
    generations: RefCell<HashMap<Node, u64>>,
    /// Per dependent, the input counters its last computation saw.
    recorded: RefCell<HashMap<Node, HashMap<Node, u64>>>,
    graph: RefCell<DependencyGraph>,
    /// What each channel of a property declared it reads, where a model said so
    /// channel by channel. The graph above holds their union; this holds the
    /// split, which the record needs and invalidation does not.
    channels: RefCell<HashMap<Identifier, ChannelInputs>>,
    /// Moves whenever any counter moves.
    epoch: Cell<u64>,
    /// The properties holding an override, where currency stops.
    edited: RefCell<HashSet<Node>>,
    /// Every property of the sample, by name, held weakly: what a formula reads
    /// is looked at for *not applicable* before it runs.
    properties: RefCell<HashMap<Identifier, std::rc::Weak<RefCell<Property>>>>,
}

impl SampleCore {
    fn generation(&self, node: &Node) -> u64 {
        self.generations.borrow().get(node).copied().unwrap_or(0)
    }

    fn bump(&self, node: &Node) {
        self.epoch.set(self.epoch.get() + 1);
        *self
            .generations
            .borrow_mut()
            .entry(node.clone())
            .or_insert(0) += 1;
    }

    /// A dependent is current only if every declared input matches the counter
    /// recorded for it **and** is itself current.
    ///
    /// Comparing the direct inputs alone would miss the case that matters:
    /// `malt` changes, nobody reads `plato`, and `haze` then finds
    /// `plato`'s counter unchanged and answers with a stale value.
    fn is_current(&self, node: &Node) -> bool {
        // Nothing reruns an override's formula, so nothing upstream of it can
        // make it, or what reads it, out of date.
        if self.edited.borrow().contains(node) {
            return true;
        }
        let inputs = self.graph.borrow().inputs_of(node).clone();
        if inputs.is_empty() {
            return true;
        }
        let recorded = self.recorded.borrow();
        let Some(seen) = recorded.get(node) else {
            return false;
        };
        inputs
            .iter()
            .all(|input| seen.get(input) == Some(&self.generation(input)) && self.is_current(input))
    }

    fn record(&self, node: &Node) {
        let inputs = self.graph.borrow().inputs_of(node).clone();
        let seen = inputs
            .into_iter()
            .map(|input| {
                let generation = self.generation(&input);
                (input, generation)
            })
            .collect();
        self.recorded.borrow_mut().insert(node.clone(), seen);
    }
}

/// A named, shared reference to one quantity.
///
/// A handle knows its name; a `Property` still does not. The name is what the
/// counter is keyed by and what a cycle path is written in, and it belongs to
/// the *reference* rather than to the quantity.
#[derive(Clone)]
pub struct PropertyHandle {
    name: Identifier,
    property: Rc<RefCell<Property>>,
    core: Rc<SampleCore>,
}

impl fmt::Debug for PropertyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PropertyHandle")
            .field("name", &self.name.as_str())
            .field("property", &self.property.borrow())
            .finish()
    }
}

impl PropertyHandle {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    /// Reading pulls: the inputs' counters are compared first, and a stale
    /// cache is cleared before the formula is consulted.
    pub fn value(&self) -> Result<Value, ComputeError> {
        self.refresh();
        if self.inputs_not_applicable() {
            return self.finish(Ok(Value::NotApplicable));
        }
        let outcome = self.property.borrow().value();
        self.finish(outcome)
    }

    pub fn uncertainty(&self) -> Result<Option<Uncertainty>, ComputeError> {
        self.refresh();
        if self.inputs_not_applicable() {
            return self.finish(Ok(None));
        }
        let outcome = self.property.borrow().uncertainty();
        self.finish(outcome)
    }

    /// Whether a formula of this property reads a value *not applicable*, and
    /// so gives it too, settled without running. A value entered, an override,
    /// and a formula whose inputs all apply, are not.
    fn inputs_not_applicable(&self) -> bool {
        {
            let property = self.property.borrow();
            if !(property.is_computed() || property.has_uncertainty_formula())
                || property.is_edited()
            {
                return false;
            }
        }
        let inputs: Vec<Node> = self
            .core
            .graph
            .borrow()
            .inputs_of(&self.node())
            .iter()
            .cloned()
            .collect();
        let applies = |name: &Identifier| {
            let held = self.core.properties.borrow().get(name).cloned();
            let Some(property) = held.and_then(|weak| weak.upgrade()) else {
                return true;
            };
            let input = PropertyHandle {
                name: name.clone(),
                property,
                core: self.core.clone(),
            };
            !input.value().is_ok_and(|value| value.is_not_applicable())
        };
        let not_applicable = inputs.iter().any(|input| match input {
            Node::Named(name) => !applies(name),
            Node::Column { .. } => false,
        });
        if not_applicable {
            self.property.borrow().settle_not_applicable();
        }
        not_applicable
    }

    /// Whether a read would be free, inputs included — the whole question, of
    /// which `Property::is_resolved` answers only its own half.
    pub fn is_resolved(&self) -> bool {
        self.core.is_current(&self.node()) && self.property.borrow().is_resolved()
    }

    pub fn with<T>(&self, read: impl FnOnce(&Property) -> T) -> T {
        self.refresh();
        let outcome = read(&self.property.borrow());
        if crate::core::property::reads_compute() {
            self.core.record(&self.node());
        }
        outcome
    }

    /// Reads the property as it stands: no refresh and no record, so the
    /// freshness check discards no cache. Given `Property::peek_value`, it
    /// runs nothing at all.
    pub fn peek<T>(&self, read: impl FnOnce(&Property) -> T) -> T {
        read(&self.property.borrow())
    }

    /// Whether every input, transitively, still has the counter this
    /// property's last run recorded.
    pub fn is_current(&self) -> bool {
        self.core.is_current(&self.node())
    }

    /// Records the inputs' counters as they stand, so that the next read does
    /// not rerun the formula: what a value whose record still matches its
    /// inputs is owed.
    pub fn mark_current(&self) {
        self.core.record(&self.node());
    }

    fn node(&self) -> Node {
        Node::Named(self.name.clone())
    }

    fn refresh(&self) {
        // Where reads do not compute, a stale value is kept and served as it
        // is: clearing it would lose the only value there is.
        if !crate::core::property::reads_compute() || self.core.is_current(&self.node()) {
            return;
        }
        // `try_borrow_mut` rather than `borrow_mut`: a re-entrant read is
        // already inside a shared borrow, and it does not need refreshing —
        // it will meet `Cache::Running` and report a cycle.
        if let Ok(mut property) = self.property.try_borrow_mut() {
            property.invalidate();
        }
    }

    fn finish<T>(&self, outcome: Result<T, ComputeError>) -> Result<T, ComputeError> {
        match outcome {
            Ok(value) => {
                // A value served without computing is not made current by it.
                if crate::core::property::reads_compute() {
                    self.core.record(&self.node());
                }
                Ok(value)
            }
            // Each frame that knows a name completes the path on the way out.
            Err(error) => Err(error.within(&self.name)),
        }
    }

    // ------------------------------------------------------------- writing

    /// An override on a computed property, which moves the counter like any
    /// assignment.
    pub fn set_value(&self, value: Value) {
        self.property.borrow_mut().set_value(value);
        self.sync_edited();
        self.core.bump(&self.node());
    }

    pub fn is_edited(&self) -> bool {
        self.property.borrow().is_edited()
    }

    pub fn has_failed(&self) -> bool {
        self.property.borrow().has_failed()
    }

    /// Drops an override for its formula, and moves the counter when there was
    /// one: what read the override reads the formula's value next.
    pub fn restore_formula(&self) {
        let restored = self.property.borrow_mut().restore_formula();
        self.sync_edited();
        if restored {
            self.core.bump(&self.node());
        }
    }

    /// Holds a filled value as an override and moves nothing: loading is not
    /// an edit.
    pub fn hold_as_edited(&self) {
        self.property.borrow_mut().hold_as_edited();
        self.sync_edited();
    }

    /// The record beside a value written next to readings vouches for it.
    pub fn confirm_written_as_recorded(&self) {
        self.property.borrow_mut().confirm_written_as_recorded();
    }

    /// Holds a loaded value with no record of its formula as an override, and
    /// says whether it did.
    pub fn hold_record_missing(&self) -> bool {
        let held = self.property.borrow_mut().hold_record_missing();
        self.sync_edited();
        held
    }

    fn sync_edited(&self) {
        let mut edited = self.core.edited.borrow_mut();
        if self.property.borrow().is_edited() {
            edited.insert(self.node());
        } else {
            edited.remove(&self.node());
        }
    }

    pub fn set_readings(&self, readings: Readings, convention: Option<Convention>) {
        self.property
            .borrow_mut()
            .set_readings(readings, convention);
        self.sync_edited();
        self.core.bump(&self.node());
    }

    pub fn set_uncertainty(&self, uncertainty: Option<Uncertainty>) {
        self.property.borrow_mut().set_uncertainty(uncertainty);
        self.core.bump(&self.node());
    }

    /// Bumps nothing: display metadata changes how a quantity is written, never
    /// what it is. This is `Change::Presentation` expressed as an absence.
    pub fn set_presentation(&self, presentation: Presentation) {
        self.property.borrow_mut().set_presentation(presentation);
    }

    pub fn set_records(&self, records: Records) {
        self.property.borrow_mut().set_records(records);
    }

    /// Whether this property is in the middle of computing: its formula is
    /// running, and nothing may change it until it returns. What a caller
    /// asks before writing, so that a formula writing to the value it is
    /// producing is refused rather than panicking on the borrow.
    pub fn is_computing(&self) -> bool {
        self.property.try_borrow_mut().is_err()
    }

    pub fn records(&self) -> Records {
        self.property.borrow().records().clone()
    }

    pub fn readings(&self) -> Option<Readings> {
        self.property.borrow().readings().cloned()
    }

    pub fn presentation(&self) -> Presentation {
        self.property.borrow().presentation().clone()
    }

    pub fn is_computed(&self) -> bool {
        self.property.borrow().is_computed()
    }

    pub fn has_uncertainty_formula(&self) -> bool {
        self.property.borrow().has_uncertainty_formula()
    }

    /// A recomputation asked for by name: both caches are cleared, and the
    /// counter moves so that what derives from this property recomputes too.
    /// A caller asking for a new value is asking for its consequences.
    pub fn invalidate(&self) {
        self.property.borrow_mut().invalidate();
        self.core.bump(&self.node());
    }
}

/// What kind already holds a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameKind {
    Property,
    Table,
    Attribute,
    Reserved,
}

impl fmt::Display for NameKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NameKind::Property => "a property",
            NameKind::Table => "a table",
            NameKind::Attribute => "an attribute",
            NameKind::Reserved => "a reserved name",
        })
    }
}

/// One sample.
#[derive(Debug, Default)]
pub struct Sample {
    name: Option<String>,
    /// The name of the file it was read from or saved to, without its
    /// extension: what the sample is called when its file writes no `name:`.
    /// Never written.
    file_name: Option<String>,
    tags: Vec<Identifier>,
    /// Tags its file holds that are not identifiers, kept to be written back
    /// and reported.
    unusable_tags: Vec<String>,
    /// Tables its file holds that could not be read, with why.
    set_aside: Vec<(Identifier, String)>,
    attributes: IndexMap<Identifier, AttributeValue>,
    properties: IndexMap<Identifier, PropertyHandle>,
    tables: IndexMap<Identifier, Table>,
    note: Note,
    core: Rc<SampleCore>,
}

/// The sample as a table sees it: properties and attributes, and no tables.
///
/// A borrow of three fields rather than of the whole sample, so that a table
/// can be resolved while it is mutably borrowed from the same sample.
struct SampleScope<'a> {
    properties: &'a IndexMap<Identifier, PropertyHandle>,
    attributes: &'a IndexMap<Identifier, AttributeValue>,
    core: &'a Rc<SampleCore>,
    /// The sample's other tables, whose declared columns a derivation reads.
    tables: &'a IndexMap<Identifier, Table>,
}

impl Scope for SampleScope<'_> {
    fn read(&self, name: &Identifier) -> Result<Value, ComputeError> {
        if let Some(handle) = self.properties.get(name) {
            return handle.value();
        }
        if let Some(value) = self.attributes.get(name) {
            return value.as_scalar().cloned().ok_or_else(|| {
                ComputeError::failed(std::io::Error::other(format!(
                    "'{name}' is a list attribute; read it as a list rather than as one scalar"
                )))
            });
        }
        Err(ComputeError::failed(unknown(
            name,
            self.properties.keys().chain(self.attributes.keys()),
            NameKind::Property,
        )))
    }

    fn read_list(&self, name: &Identifier) -> Result<Vec<Value>, ComputeError> {
        match self.attributes.get(name) {
            Some(AttributeValue::List(values)) => Ok(values.clone()),
            Some(AttributeValue::Scalar(_)) | None if self.properties.contains_key(name) => {
                Err(ComputeError::failed(std::io::Error::other(format!(
                    "'{name}' is scalar; read it as one value rather than as a list"
                ))))
            }
            Some(AttributeValue::Scalar(_)) => {
                Err(ComputeError::failed(std::io::Error::other(format!(
                    "'{name}' is a scalar attribute; read it as one value rather than as a list"
                ))))
            }
            None => Err(ComputeError::failed(unknown(
                name,
                self.properties.keys().chain(self.attributes.keys()),
                NameKind::Attribute,
            ))),
        }
    }

    fn read_uncertainty(&self, name: &Identifier) -> Result<Option<Uncertainty>, ComputeError> {
        match self.properties.get(name) {
            Some(handle) => handle.uncertainty(),
            // An attribute is one `Value` and has nowhere to put one.
            None => Ok(None),
        }
    }

    fn generation(&self, name: &Identifier) -> u64 {
        self.core.generation(&Node::Named(name.clone()))
    }

    fn epoch(&self) -> Option<u64> {
        Some(self.core.epoch.get())
    }

    fn column_generation(&self, table: &Identifier, column: &Identifier) -> u64 {
        self.core.generation(&Node::Column {
            table: table.clone(),
            column: column.clone(),
        })
    }

    fn read_column(
        &self,
        table: &Identifier,
        column: &Identifier,
    ) -> Result<Vec<crate::core::table::ForeignCell>, ComputeError> {
        let held = self.tables.get(table).ok_or_else(|| {
            ComputeError::failed(unknown(table, self.tables.keys(), NameKind::Table))
        })?;
        let view = held
            .column(column)
            .map_err(|error| ComputeError::failed(std::io::Error::other(error.to_string())))?;
        view.cells()
            .map(|(index, cell)| {
                Ok((
                    index.into_iter().cloned().collect(),
                    cell.value()?,
                    cell.uncertainty()?,
                ))
            })
            .collect()
    }
}

impl Scope for Sample {
    fn read(&self, name: &Identifier) -> Result<Value, ComputeError> {
        self.scope().read(name)
    }

    fn read_list(&self, name: &Identifier) -> Result<Vec<Value>, ComputeError> {
        self.scope().read_list(name)
    }

    fn read_uncertainty(&self, name: &Identifier) -> Result<Option<Uncertainty>, ComputeError> {
        self.scope().read_uncertainty(name)
    }

    fn generation(&self, name: &Identifier) -> u64 {
        self.scope().generation(name)
    }

    fn column_generation(&self, table: &Identifier, column: &Identifier) -> u64 {
        self.scope().column_generation(table, column)
    }

    fn read_column(
        &self,
        table: &Identifier,
        column: &Identifier,
    ) -> Result<Vec<crate::core::table::ForeignCell>, ComputeError> {
        self.scope().read_column(table, column)
    }

    fn epoch(&self) -> Option<u64> {
        self.scope().epoch()
    }
}

impl Sample {
    pub fn new() -> Sample {
        Sample::default()
    }

    fn scope(&self) -> SampleScope<'_> {
        SampleScope {
            properties: &self.properties,
            attributes: &self.attributes,
            core: &self.core,
            tables: &self.tables,
        }
    }

    // ---------------------------------------------------------- properties

    /// Replacing a name keeps the **handle**, so a wrapper obtained earlier
    /// observes the new value rather than becoming a view onto a detached copy.
    /// It replaces the declaration too: the inputs the old quantity declared go
    /// with it, and what the new one reads is declared afresh. What reads this
    /// name by name keeps reading it.
    pub fn set_property(
        &mut self,
        name: Identifier,
        property: Property,
    ) -> Result<(), SampleError> {
        self.check_free(&name, NameKind::Property)?;
        match self.properties.get(&name) {
            Some(handle) => {
                *handle.property.borrow_mut() = property;
                handle.sync_edited();
                let node = Node::Named(name.clone());
                let dependents: Vec<Node> = self
                    .core
                    .graph
                    .borrow()
                    .dependents_of(&node)
                    .iter()
                    .cloned()
                    .collect();
                {
                    let mut graph = self.core.graph.borrow_mut();
                    graph.remove(&node);
                    for dependent in &dependents {
                        let mut inputs: Vec<Node> =
                            graph.inputs_of(dependent).iter().cloned().collect();
                        inputs.push(node.clone());
                        graph
                            .declare(dependent, &inputs)
                            .expect("an edge that existed introduces no cycle");
                    }
                }
                self.core.recorded.borrow_mut().remove(&node);
            }
            None => {
                let property = Rc::new(RefCell::new(property));
                self.core
                    .properties
                    .borrow_mut()
                    .insert(name.clone(), Rc::downgrade(&property));
                self.properties.insert(
                    name.clone(),
                    PropertyHandle {
                        name: name.clone(),
                        property,
                        core: self.core.clone(),
                    },
                );
                self.properties[&name].sync_edited();
            }
        }
        self.core.bump(&Node::Named(name));
        Ok(())
    }

    /// By value, not by borrow: the handle *is* the shared reference, and a
    /// Python wrapper outlives the call that produced it.
    pub fn property(&self, name: &Identifier) -> Result<PropertyHandle, SampleError> {
        self.properties
            .get(name)
            .cloned()
            .ok_or_else(|| unknown(name, self.properties.keys(), NameKind::Property))
    }

    pub fn remove_property(&mut self, name: &Identifier) -> Result<Property, SampleError> {
        let handle = self
            .properties
            .shift_remove(name)
            .ok_or_else(|| unknown(name, self.properties.keys(), NameKind::Property))?;
        self.core
            .graph
            .borrow_mut()
            .remove(&Node::Named(name.clone()));
        self.core
            .edited
            .borrow_mut()
            .remove(&Node::Named(name.clone()));
        self.core.bump(&Node::Named(name.clone()));
        // Outstanding handles see an absent quantity, which is what happened.
        Ok(std::mem::replace(
            &mut *handle.property.borrow_mut(),
            Property::stored(Value::absent()),
        ))
    }

    pub fn property_names(&self) -> Vec<&Identifier> {
        self.properties.keys().collect()
    }

    pub fn has_property(&self, name: &Identifier) -> bool {
        self.properties.contains_key(name)
    }

    // -------------------------------------------------------------- tables

    /// Setting a table **absorbs its declarations**: each derivation becomes
    /// one edge per output, and a cycle running through a sample-level property
    /// is refused here, because this is the only place that sees both halves.
    pub fn set_table(&mut self, name: Identifier, mut table: Table) -> Result<(), SampleError> {
        self.check_free(&name, NameKind::Table)?;
        // Called by the key it is held under, so that its column nodes and
        // that key cannot disagree.
        table.set_name(name.clone()).map_err(SampleError::Table)?;
        let relationships = table.declarations();
        self.core
            .graph
            .borrow_mut()
            .declare_all(&relationships)
            .map_err(SampleError::Dependency)?;
        for column in table.column_names() {
            self.core.bump(&Node::Column {
                table: name.clone(),
                column: column.clone(),
            });
        }
        self.tables.insert(name, table);
        Ok(())
    }

    pub fn table(&self, name: &Identifier) -> Result<&Table, SampleError> {
        self.tables
            .get(name)
            .ok_or_else(|| unknown(name, self.tables.keys(), NameKind::Table))
    }

    pub fn remove_table(&mut self, name: &Identifier) -> Result<Table, SampleError> {
        let table = self
            .tables
            .shift_remove(name)
            .ok_or_else(|| unknown(name, self.tables.keys(), NameKind::Table))?;
        // No dangling column node survives it.
        let mut graph = self.core.graph.borrow_mut();
        for column in table.column_names() {
            graph.remove(&Node::Column {
                table: name.clone(),
                column: column.clone(),
            });
        }
        Ok(table)
    }

    pub fn table_names(&self) -> Vec<&Identifier> {
        self.tables.keys().collect()
    }

    /// A row added to a table this sample holds, moving on every column.
    pub fn add_row(
        &mut self,
        table: &Identifier,
        cells: Vec<(Identifier, Property)>,
    ) -> Result<(), SampleError> {
        let columns: Vec<Identifier> = {
            let held = self.table_mut(table)?;
            held.add_row(cells).map_err(SampleError::Table)?;
            held.column_names().into_iter().cloned().collect()
        };
        for column in columns {
            self.core.bump(&Node::Column {
                table: table.clone(),
                column,
            });
        }
        Ok(())
    }

    /// Cells of a held table replaced, moving on the columns written.
    pub fn update_row(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
        cells: Vec<(Identifier, Property)>,
    ) -> Result<(), SampleError> {
        let written: Vec<Identifier> = cells.iter().map(|(column, _)| column.clone()).collect();
        self.table_mut(table)?
            .update_row(row, cells)
            .map_err(SampleError::Table)?;
        for column in written {
            self.core.bump(&Node::Column {
                table: table.clone(),
                column,
            });
        }
        Ok(())
    }

    /// One cell given back to the statistic its column declares of its
    /// readings; what reads the column follows.
    pub fn retake_statistic(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .retake_statistic(row, column)
            .map_err(SampleError::Table)?;
        self.core.bump(&Node::Column {
            table: table.clone(),
            column: column.clone(),
        });
        Ok(())
    }

    /// A held table's row recomputed at its next resolution.
    pub fn invalidate_row(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .invalidate_row(row)
            .map_err(SampleError::Table)
    }

    /// Where a held table's cell stands against its derivation.
    pub fn cell_run(
        &self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<CellRun, SampleError> {
        self.table(table)?
            .cell_run(row, column, &self.scope())
            .map_err(SampleError::Table)
    }

    /// A held table's cell marked as resting on its inputs as they stand.
    pub fn confirm_cell(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<(), SampleError> {
        if !self.tables.contains_key(table) {
            return Err(unknown(table, self.tables.keys(), NameKind::Table));
        }
        let no_tables = IndexMap::new();
        let scope = SampleScope {
            properties: &self.properties,
            attributes: &self.attributes,
            core: &self.core,
            tables: &no_tables,
        };
        self.tables
            .get_mut(table)
            .expect("checked just above")
            .mark_cell_current(row, column, &scope)
            .map_err(SampleError::Table)
    }

    /// A held table's cell records, set without moving a counter.
    pub fn set_cell_records(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
        records: Records,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .set_cell_records(row, column, records)
            .map_err(SampleError::Table)
    }

    fn table_mut(&mut self, name: &Identifier) -> Result<&mut Table, SampleError> {
        if !self.tables.contains_key(name) {
            return Err(unknown(name, self.tables.keys(), NameKind::Table));
        }
        Ok(self.tables.get_mut(name).expect("checked just above"))
    }

    // -------------------------------------------------- attributes and tags

    /// An input like any other: setting one bumps its counter, exactly as a
    /// value change does, through a different mutation path.
    pub fn set_attribute(
        &mut self,
        name: Identifier,
        value: impl Into<AttributeValue>,
    ) -> Result<(), SampleError> {
        self.check_free(&name, NameKind::Attribute)?;
        self.attributes.insert(name.clone(), value.into());
        self.core.bump(&Node::Named(name));
        Ok(())
    }

    pub fn attribute(&self, name: &Identifier) -> Result<&AttributeValue, SampleError> {
        self.attributes
            .get(name)
            .ok_or_else(|| unknown(name, self.attributes.keys(), NameKind::Attribute))
    }

    pub fn remove_attribute(&mut self, name: &Identifier) -> Result<AttributeValue, SampleError> {
        let value = self
            .attributes
            .shift_remove(name)
            .ok_or_else(|| unknown(name, self.attributes.keys(), NameKind::Attribute))?;
        self.core
            .graph
            .borrow_mut()
            .remove(&Node::Named(name.clone()));
        self.core.bump(&Node::Named(name.clone()));
        Ok(value)
    }

    pub fn attribute_names(&self) -> Vec<&Identifier> {
        self.attributes.keys().collect()
    }

    pub fn has_attribute(&self, name: &Identifier) -> bool {
        self.attributes.contains_key(name)
    }

    pub fn tags(&self) -> &[Identifier] {
        &self.tags
    }

    pub fn set_tags(&mut self, tags: Vec<Identifier>) {
        self.tags.clear();
        for tag in tags {
            self.add_tag(tag);
        }
    }

    /// Reports whether it added: `[reference, reference]` says nothing
    /// `[reference]` does not.
    pub fn add_tag(&mut self, tag: Identifier) -> bool {
        if self.tags.contains(&tag) {
            return false;
        }
        self.tags.push(tag);
        true
    }

    pub fn remove_tag(&mut self, tag: &Identifier) -> bool {
        match self.tags.iter().position(|held| held == tag) {
            Some(at) => {
                self.tags.remove(at);
                true
            }
            None => false,
        }
    }

    pub fn has_tag(&self, tag: &Identifier) -> bool {
        self.tags.contains(tag)
    }

    /// Tags its file holds that are not identifiers.
    pub fn unusable_tags(&self) -> &[String] {
        &self.unusable_tags
    }

    pub fn set_unusable_tags(&mut self, tags: Vec<String>) {
        self.unusable_tags = tags;
    }

    /// Tables its file held that could not be read, with why: a sample holding
    /// one is not written, which would drop it.
    pub fn set_aside_tables(&self) -> &[(Identifier, String)] {
        &self.set_aside
    }

    pub fn set_aside_table(&mut self, name: Identifier, reason: String) {
        self.set_aside.push((name, reason));
    }

    // -------------------------------------------------------- dependencies

    /// A property may derive from a table column, which is why `inputs` is a
    /// `Node`: *the mean gravity across the fermentation* is as ordinary
    /// as an alcohol from two gravities.
    pub fn declare_dependencies(
        &mut self,
        dependent: &Identifier,
        inputs: &[Node],
    ) -> Result<(), SampleError> {
        // A declaration replaces the one before it, channels included:
        // re-running a model definition says everything afresh.
        self.core.channels.borrow_mut().remove(dependent);
        self.core
            .graph
            .borrow_mut()
            .declare(&Node::Named(dependent.clone()), inputs)
            .map_err(SampleError::Dependency)
    }

    /// The same declaration, said channel by channel: `inputs` is still the
    /// union the graph holds, and `channels` says which formula reads what.
    pub fn declare_dependencies_by_channel(
        &mut self,
        dependent: &Identifier,
        inputs: &[Node],
        channels: ChannelInputs,
    ) -> Result<(), SampleError> {
        self.declare_dependencies(dependent, inputs)?;
        if !channels.is_empty() {
            self.core
                .channels
                .borrow_mut()
                .insert(dependent.clone(), channels);
        }
        Ok(())
    }

    /// What each of this property's formulas declared it reads, where a model
    /// said so per channel.
    pub fn channel_inputs_of(&self, name: &Identifier) -> Option<ChannelInputs> {
        self.core.channels.borrow().get(name).cloned()
    }

    pub fn dependencies_of(&self, name: &Identifier) -> Result<Vec<Node>, SampleError> {
        Ok(sorted(
            self.core
                .graph
                .borrow()
                .inputs_of(&Node::Named(name.clone())),
        ))
    }

    pub fn dependents_of(&self, name: &Identifier) -> Result<Vec<Node>, SampleError> {
        Ok(sorted(
            self.core
                .graph
                .borrow()
                .dependents_of(&Node::Named(name.clone())),
        ))
    }

    /// Exposed for diagnostics — *what will this edit touch* — and it is the
    /// graph traversal, which nothing else needs now that freshness is pulled.
    pub fn affected_by_change(&self, node: &Node) -> Result<Vec<Node>, SampleError> {
        Ok(self.core.graph.borrow().affected_by(node, Change::Value))
    }

    // ------------------------------------------------------- note and name

    pub fn note(&self) -> &str {
        self.note.as_str()
    }

    /// Wholesale. There is no append, no line-level edit and no formatting:
    /// every such operation is a way for the program to modify text it did not
    /// write.
    pub fn set_note(&mut self, note: String) {
        self.note = Note::new(note);
    }

    /// What the sample is called: the `name:` its file writes, else its file's
    /// name without the extension. `None` only for a sample that has no file
    /// yet and was given no name.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref().or(self.file_name.as_deref())
    }

    /// The name the file writes, `name:`, and nothing in its place: what is
    /// saved, and what an edit of the name changes.
    pub fn written_name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The name to write; `None` removes `name:`, and the file's name stands
    /// for it again.
    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    /// The file the sample is read from or saved to, whose name without its
    /// extension names a sample whose file writes none. Only its name is
    /// kept: where the file is stays its holder's.
    pub fn set_file(&mut self, path: Option<&std::path::Path>) {
        self.file_name = path
            .and_then(std::path::Path::file_stem)
            .map(|stem| stem.to_string_lossy().into_owned());
    }

    // -------------------------------------------------------------- filling

    /// A model's sample filled by a file read on its own: the file wins
    /// wherever it speaks, the declarations stay, and every value the file
    /// supplied is recorded as current. Refused before anything moves.
    pub fn fill_from(&mut self, file: Sample) -> Result<(), SampleError> {
        for (name, handle) in &file.properties {
            self.check_free(name, NameKind::Property)?;
            if let Some(declared) = self.properties.get(name)
                && let Some(conflict) = declared
                    .property
                    .borrow()
                    .conflict_with(&handle.property.borrow())
            {
                return Err(SampleError::FillConflict {
                    name: name.clone(),
                    conflict,
                });
            }
        }
        for (name, table) in &file.tables {
            self.check_free(name, NameKind::Table)?;
            if let Some(declared) = self.tables.get(name) {
                declared.check_fill(table).map_err(SampleError::Table)?;
            }
        }
        for name in file.attributes.keys() {
            self.check_free(name, NameKind::Attribute)?;
        }

        let Sample {
            name,
            file_name,
            tags,
            unusable_tags,
            set_aside,
            attributes,
            properties,
            tables,
            note,
            core: _,
        } = file;
        if name.is_some() {
            self.name = name;
        }
        if file_name.is_some() {
            self.file_name = file_name;
        }
        if !tags.is_empty() {
            self.set_tags(tags);
        }
        self.unusable_tags = unusable_tags;
        self.set_aside = set_aside;
        self.note = note;
        for (name, value) in attributes {
            self.attributes.insert(name.clone(), value);
            self.core.bump(&Node::Named(name));
        }

        let mut filled = Vec::with_capacity(properties.len());
        for (name, handle) in properties {
            let supplied = handle.property.replace(Property::stored(Value::absent()));
            match self.properties.get(&name) {
                Some(declared) => declared
                    .property
                    .borrow_mut()
                    .fill_from(supplied)
                    .expect("conflicts are checked before anything moves"),
                None => {
                    let property = Rc::new(RefCell::new(supplied));
                    self.core
                        .properties
                        .borrow_mut()
                        .insert(name.clone(), Rc::downgrade(&property));
                    self.properties.insert(
                        name.clone(),
                        PropertyHandle {
                            name: name.clone(),
                            property,
                            core: self.core.clone(),
                        },
                    );
                }
            }
            self.properties[&name].sync_edited();
            self.core.bump(&Node::Named(name.clone()));
            filled.push(name);
        }

        let mut supplied = Vec::new();
        for (name, table) in tables {
            if !self.tables.contains_key(&name) {
                self.set_table(name, table)?;
                continue;
            }
            let declared = self.tables.get_mut(&name).expect("checked just above");
            let rows = declared.fill_rows(table).map_err(SampleError::Table)?;
            for column in declared.column_names() {
                self.core.bump(&Node::Column {
                    table: name.clone(),
                    column: column.clone(),
                });
            }
            supplied.push((name, rows));
        }
        // What the file supplied in a table is recorded once every table is in:
        // a cell reading another table's column, recorded before that table was
        // filled, would find the column moved by the fill itself.
        let no_tables = IndexMap::new();
        for (name, rows) in supplied {
            let scope = SampleScope {
                properties: &self.properties,
                attributes: &self.attributes,
                core: &self.core,
                tables: &no_tables,
            };
            if let Some(table) = self.tables.get_mut(&name) {
                table.record_filled(&rows, &scope);
            }
        }

        // Last, once every counter has moved: a value recorded earlier would
        // find its inputs changed by the fill itself and run anyway.
        for name in filled {
            self.core.record(&Node::Named(name));
        }
        Ok(())
    }

    /// Records every value the sample holds as resting on its inputs as they
    /// stand, running nothing: what a fill is owed once the declarations that
    /// follow it are in place. A declaration settled after the fill gives a
    /// filled value inputs it had no record of, and the value would run again
    /// at its next computation for nothing having changed.
    pub fn confirm_filled(&mut self) {
        let names: Vec<Identifier> = self.properties.keys().cloned().collect();
        for name in names {
            let resolved = self.properties[&name].property.borrow().is_resolved();
            if resolved {
                self.core.record(&Node::Named(name));
            }
        }
    }

    // ------------------------------------------------------ materialization

    /// Run every lazy value, uncertainty and cell, and keep the formulas: what
    /// a save needs when the session goes on working after it.
    pub fn resolve(&mut self) -> Result<(), SampleError> {
        let tables: Vec<Identifier> = self.tables.keys().cloned().collect();
        for name in &tables {
            self.resolve_table(name)?;
        }

        for (name, handle) in &self.properties {
            // Both channels: a stored value can have a computed uncertainty
            // beside it that nothing else would ever run.
            let failed = |source| SampleError::Compute {
                property: name.clone(),
                source,
            };
            handle.value().map_err(failed)?;
            handle.uncertainty().map_err(failed)?;
        }
        Ok(())
    }

    /// One table's derivations and nothing else: what reading that table needs,
    /// where `resolve` would run every formula of the sample.
    pub fn resolve_table(&mut self, name: &Identifier) -> Result<(), SampleError> {
        self.resolve_table_after(name, &mut Vec::new(), None)
    }

    /// Resolves only what named columns of a table need, after the tables its
    /// derivations read.
    pub fn resolve_table_columns(
        &mut self,
        name: &Identifier,
        columns: &[Identifier],
    ) -> Result<(), SampleError> {
        self.resolve_table_after(name, &mut Vec::new(), Some(columns))
    }

    /// Holds a held table's derived cell as an override.
    pub fn hold_cell(
        &mut self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .hold_cell(row, column)
            .map_err(SampleError::Table)
    }

    /// Gives a column's held cells back to their derivation, in one row or all.
    pub fn release_cells(
        &mut self,
        table: &Identifier,
        column: &Identifier,
        row: Option<&RowAddress>,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .release_cells(column, row)
            .map_err(SampleError::Table)
    }

    /// Whether a held table's derived cell is held as an override.
    pub fn is_cell_held(
        &self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<bool, SampleError> {
        self.table(table)?
            .is_held(row, column)
            .map_err(SampleError::Table)
    }

    /// Forgets what the derivation of one column recorded, in one row or all.
    pub fn invalidate_column(
        &mut self,
        table: &Identifier,
        column: &Identifier,
        row: Option<&RowAddress>,
    ) -> Result<(), SampleError> {
        self.table_mut(table)?
            .invalidate_column(column, row)
            .map_err(SampleError::Table)
    }

    /// Whether a derivation wrote a held table's cell in this session.
    pub fn cell_ran(
        &self,
        table: &Identifier,
        row: &RowAddress,
        column: &Identifier,
    ) -> Result<bool, SampleError> {
        self.table(table)?
            .ran(row, column)
            .map_err(SampleError::Table)
    }

    /// A table resolved after the tables its derivations read, which `upstream`
    /// holds while they resolve; the graph refuses a cycle between tables where
    /// they are set, so meeting one again only stops the walk.
    fn resolve_table_after(
        &mut self,
        name: &Identifier,
        upstream: &mut Vec<Identifier>,
        only: Option<&[Identifier]>,
    ) -> Result<(), SampleError> {
        let Some(at) = self.tables.get_index_of(name) else {
            return Err(unknown(name, self.tables.keys(), NameKind::Table));
        };
        if upstream.contains(name) {
            return Ok(());
        }
        let foreign = self.tables[at].foreign_columns();
        for (table, column) in &foreign {
            if !self
                .tables
                .get(table)
                .is_some_and(|held| held.column(column).is_ok())
            {
                return Err(SampleError::Table(TableError::ForeignColumn {
                    table: table.clone(),
                    column: column.clone(),
                }));
            }
        }
        upstream.push(name.clone());
        for table in self.tables[at].foreign_tables() {
            if let Err(error) = self.resolve_table_after(&table, upstream, None) {
                upstream.pop();
                return Err(error);
            }
        }
        upstream.pop();

        // Taken out while it resolves, so that the scope can hand over the
        // other tables read-only.
        let (key, mut table) = self
            .tables
            .shift_remove_index(at)
            .expect("found just above");
        let columns: Vec<Identifier> = table.column_names().into_iter().cloned().collect();
        let before: Vec<u64> = columns
            .iter()
            .map(|column| table.column_generation(column))
            .collect();
        let outcome = {
            let scope = SampleScope {
                properties: &self.properties,
                attributes: &self.attributes,
                core: &self.core,
                tables: &self.tables,
            };
            match only {
                Some(columns) => table.resolve_for(columns, &scope),
                None => table.resolve(&scope),
            }
        };
        // Only what ran is recorded as resting on its inputs.
        let resolved = only.map(|columns| table.needed_outputs(columns));
        if outcome.is_ok() {
            // A column a derivation wrote has moved on, which anything deriving
            // from it has to see; one nothing wrote has not.
            for (column, generation) in columns.into_iter().zip(before) {
                let node = Node::Column {
                    table: name.clone(),
                    column: column.clone(),
                };
                if table.column_generation(&column) != generation {
                    self.core.bump(&node);
                }
                // A derived column resolved rests on its inputs as they stand:
                // what reads it is current once it has read it, rather than never.
                if table.is_derived(&column)
                    && resolved
                        .as_ref()
                        .is_none_or(|outputs| outputs.contains(&column))
                {
                    self.core.record(&node);
                }
            }
        }
        self.tables.shift_insert(at, key, table);
        outcome.map_err(|error| match error {
            TableError::Compute {
                column,
                index,
                source,
            } => SampleError::ComputeCell {
                table: name.clone(),
                column,
                index,
                source,
            },
            other => SampleError::Table(other),
        })
    }

    /// Resolve, then replace every formula by the value it yielded, so that a
    /// file holds values rather than promises.
    pub fn materialize(&mut self) -> Result<(), SampleError> {
        self.resolve()?;
        for (name, handle) in &self.properties {
            let mut property = handle.property.borrow_mut();
            // Unconditionally: `materialize` is a no-op on a property with no
            // formula, and a stored value can still have a computed
            // uncertainty beside it.
            property
                .materialize()
                .map_err(|source| SampleError::Compute {
                    property: name.clone(),
                    source,
                })?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ namespace

    fn check_free(&self, name: &Identifier, kind: NameKind) -> Result<(), SampleError> {
        if name.as_str().starts_with('_') {
            return Err(SampleError::PrivateName { name: name.clone() });
        }
        if RESERVED.contains(&name.as_str()) {
            return Err(SampleError::NameCollision {
                name: name.clone(),
                existing: NameKind::Reserved,
            });
        }
        let held = [
            (NameKind::Property, self.properties.contains_key(name)),
            (NameKind::Table, self.tables.contains_key(name)),
            (NameKind::Attribute, self.attributes.contains_key(name)),
        ];
        for (existing, taken) in held {
            if taken && existing != kind {
                return Err(SampleError::NameCollision {
                    name: name.clone(),
                    existing,
                });
            }
        }
        Ok(())
    }
}

fn sorted(nodes: &std::collections::HashSet<Node>) -> Vec<Node> {
    let mut ordered: Vec<Node> = nodes.iter().cloned().collect();
    ordered.sort();
    ordered
}

fn unknown<'a>(
    name: &Identifier,
    available: impl Iterator<Item = &'a Identifier>,
    kind: NameKind,
) -> SampleError {
    let available: Vec<Identifier> = available.cloned().collect();
    let suggestion = nearest(&available, name);
    match kind {
        NameKind::Table => SampleError::UnknownTable {
            name: name.clone(),
            available,
            suggestion,
        },
        NameKind::Attribute => SampleError::UnknownAttribute {
            name: name.clone(),
            available,
            suggestion,
        },
        _ => SampleError::UnknownProperty {
            name: name.clone(),
            available,
            suggestion,
        },
    }
}

fn nearest(available: &[Identifier], wanted: &Identifier) -> Option<Identifier> {
    let names: Vec<&str> = available.iter().map(Identifier::as_str).collect();
    crate::core::identifier::nearest(wanted.as_str(), names)
        .and_then(|name| Identifier::new(&name).ok())
}

/// Everything a sample refuses to guess at.
#[derive(Debug, Clone, PartialEq)]
pub enum SampleError {
    PrivateName {
        name: Identifier,
    },
    UnknownProperty {
        name: Identifier,
        available: Vec<Identifier>,
        suggestion: Option<Identifier>,
    },
    UnknownTable {
        name: Identifier,
        available: Vec<Identifier>,
        suggestion: Option<Identifier>,
    },
    UnknownAttribute {
        name: Identifier,
        available: Vec<Identifier>,
        suggestion: Option<Identifier>,
    },
    NameCollision {
        name: Identifier,
        existing: NameKind,
    },
    Dependency(DependencyError),
    Compute {
        property: Identifier,
        source: ComputeError,
    },
    ComputeCell {
        table: Identifier,
        column: Identifier,
        /// Absent for a column-span failure, which has no single row.
        index: Option<Vec<Value>>,
        source: ComputeError,
    },
    Table(TableError),
    FillConflict {
        name: Identifier,
        conflict: FillConflict,
    },
}

impl fmt::Display for SampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SampleError::PrivateName { name } => write!(
                f,
                "'{name}' cannot be a sample field: a leading '_' is reserved for private Python state"
            ),
            SampleError::UnknownProperty {
                name,
                available,
                suggestion,
            }
            | SampleError::UnknownTable {
                name,
                available,
                suggestion,
            }
            | SampleError::UnknownAttribute {
                name,
                available,
                suggestion,
            } => {
                let kind = match self {
                    SampleError::UnknownTable { .. } => "table",
                    SampleError::UnknownAttribute { .. } => "attribute",
                    _ => "property",
                };
                let names: Vec<String> = available.iter().map(Identifier::to_string).collect();
                // An empty list says nothing: `available: ` stood alone
                // under a table asked of a sample that holds none.
                if names.is_empty() {
                    write!(f, "unknown {kind} '{name}': this sample holds no {kind}")?;
                } else {
                    write!(
                        f,
                        "unknown {kind} '{name}'\n  available: {}",
                        names.join(", ")
                    )?;
                }
                match suggestion {
                    Some(nearest) => write!(f, "\n  did you mean: {nearest}?"),
                    None => Ok(()),
                }
            }
            SampleError::NameCollision { name, existing } => write!(
                f,
                "'{name}' is already {existing}: properties, tables and attributes \
                 share one namespace, so a field path cannot be ambiguous"
            ),
            SampleError::Dependency(error) => write!(f, "{error}"),
            SampleError::Compute { property, source } => {
                write!(f, "computing '{property}': {source}")
            }
            SampleError::ComputeCell {
                table,
                column,
                index,
                source,
            } => match index {
                Some(index) => {
                    let at: Vec<String> = index.iter().map(|value| format!("{value:?}")).collect();
                    write!(
                        f,
                        "computing '{table}.{column}' at ({}): {source}",
                        at.join(", ")
                    )
                }
                None => write!(f, "computing column '{table}.{column}': {source}"),
            },
            SampleError::Table(error) => write!(f, "{error}"),
            SampleError::FillConflict { name, conflict } => {
                write!(f, "filling '{name}' from the file: {conflict}")
            }
        }
    }
}

impl std::error::Error for SampleError {}
