//! Whether a derived value still rests on the inputs it was computed from.
//!
//! It answers with **no model and no interpreter**: the records are in the file,
//! so `samplekit validate` over a collection reports what is no longer
//! trustworthy at file-reading speed, on a fresh clone, three weeks later.
//!

use std::fmt;

use indexmap::IndexMap;
use sha2::{Digest, Sha256};

use crate::core::dependency_graph::Node;
use crate::core::identifier::Identifier;
use crate::core::property::{
    DeclaredStatistics, Fingerprint, InputName, InputRecord, Produced, Property, Records,
};
use crate::core::sample::{AttributeValue, Sample};
use crate::core::table::{CellRun, ColumnView, RowAddress};
use crate::core::value::Value;
use crate::format::canonicalization::{attribute_form, value_form};
use crate::format::schema::{self, PropertySchema, SchemaError};

/// The first twelve characters of the lowercase hex SHA-256 — what
/// `sha256sum | cut -c1-12` produces.
///
/// Stating the recipe rather than the function is the difference between a
/// digest a person can check and one they can only trust. Whether a formula
/// gives this property something — its value, its uncertainty, or both. The
/// record belongs to the property, not to a channel.
fn is_derived(handle: &crate::core::sample::PropertyHandle) -> bool {
    // A declared statistic is a formula that happens to be named rather than
    // written, and a property it gives something to records like any other.
    // Without this arm such a property answered `Source`, and a corrected
    // reading left it unreported.
    handle.is_computed()
        || handle.has_uncertainty_formula()
        || handle.peek(crate::core::property::Property::has_declared_statistic)
}

/// Whether every formula of this property has given what it owes, running
/// nothing: a computed value present, and a computed uncertainty present.
fn has_produced(handle: &crate::core::sample::PropertyHandle) -> bool {
    handle.peek(|property| {
        (!property.is_computed() || property.peek_value().is_some())
            && (!property.has_uncertainty_formula() || property.peek_uncertainty().is_some())
    })
}

fn digest(bytes: &str) -> Fingerprint {
    let hash = Sha256::digest(bytes.as_bytes());
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    Fingerprint::new(&hex[..12])
}

/// The content hash of one quantity: `value`, `readings` and `uncertainty`,
/// rendered by `canonicalization` in its flow-mapping form and never the
/// bare-scalar shorthand — so the hash does not change when unrelated metadata
/// is added beside it.
pub fn of(property: &PropertySchema) -> Fingerprint {
    digest(&value_form(property))
}

/// The quantity **without** its observations: what a property whose record names
/// `readings` holds as its own digest.
///
/// The two must not overlap. `of` covers the readings, and a file's stored value
/// and uncertainty are the statistics of the readings *as they were* — so a
/// corrected reading moves `of` while nothing in the quantity was touched, and
/// the property reads `Edited` where *stale — readings* is owed. Taking the own
/// digest over `value` and `uncertainty` alone keeps each question answerable:
/// this one says a hand changed the number, the `readings` input says the
/// observations moved.
///
/// **Nor an uncertainty the readings give**. Where the file states a statistic
/// for the uncertainty, that channel is a function of the readings and nothing
/// else: read without a model it is the number the file stored, and read with
/// one it follows the readings live — so a corrected reading moved this digest
/// under a model and not without, and Python answered *edited* where the
/// command line answered *stale — readings* over one file. A hand changing that
/// number is `validation`'s to say, against the statistic the file names.
pub fn of_quantity(property: &PropertySchema) -> Fingerprint {
    let mut without = property.clone();
    without.readings = None;
    if without
        .statistics
        .as_ref()
        .is_some_and(|stated| stated.uncertainty.is_some())
    {
        without.uncertainty = None;
    }
    digest(&value_form(&without))
}

/// The digest a property's own `fingerprint` holds, decided from the **shape**:
/// `of_quantity` where its record names its own readings, `of` otherwise.
///
/// The schema-form twin of `own_digest`, for a caller holding a `PropertySchema`
/// rather than a live property — save_computed refreshes an input's own digest
/// at every save, and refreshing it by `of` there silently undid what `stamp`
/// had just recorded.
pub fn own_of(shape: &PropertySchema, produced: Option<Produced>) -> Fingerprint {
    let names_readings = shape.computed.as_ref().is_some_and(|computed| {
        computed
            .inputs()
            .contains_key(&InputName::Named(readings_key()))
    });
    narrowed(shape, produced, names_readings)
}

/// The digest over the channels a formula produced, with the readings
/// narrowing applied on top.
fn narrowed(
    shape: &PropertySchema,
    produced: Option<Produced>,
    over_readings: bool,
) -> Fingerprint {
    let held;
    let shape = match produced {
        // The value is a formula's; the uncertainty beside it is not.
        Some(Produced::Value) => {
            held = PropertySchema {
                uncertainty: None,
                ..shape.clone()
            };
            &held
        }
        // The uncertainty is a formula's; the value beside it was entered, and
        // a hand changing an entered value overrides nothing.
        Some(Produced::Uncertainty) => {
            held = PropertySchema {
                value: None,
                ..shape.clone()
            };
            &held
        }
        None => shape,
    };
    if over_readings {
        of_quantity(shape)
    } else {
        of(shape)
    }
}

/// The digest of **one channel** of a quantity: what an input named `p.v`
/// records.
///
/// The quantity's other number is left out, which is what makes `u ← v`
/// recordable at all: were the whole quantity hashed, the uncertainty's record
/// would move when the uncertainty itself was written, and the input would be
/// stale the moment it was computed.
pub fn of_channel(shape: &PropertySchema, channel: Produced) -> Fingerprint {
    narrowed(shape, Some(channel), false)
}

/// The digest a property's own `fingerprint` holds, whichever of the several
/// it is.
///
/// **It covers what a formula produced, and nothing else**. That is the same
/// narrowing already made for readings and for a stated
/// uncertainty, and for the same reason: a digest that moves when something no
/// formula owns moves could only ever answer `Edited`, and the coarse question
/// swallows the fine one.
///
/// It is safe for everything downstream because this digest answers **one**
/// question — *has a hand touched what a formula made* — and nothing else
/// reads it. A consumer records `of`, the whole quantity's digest, and
/// `compare` recomputes `of` to check it: staleness never passes through here.
fn own_digest(
    shape: &PropertySchema,
    computed: Option<&IndexMap<InputName, InputRecord>>,
    produced: Option<Produced>,
) -> Fingerprint {
    narrowed(shape, produced, over_readings(computed))
}

/// Whether a record names this property's own readings, and its own digest is
/// therefore taken without them.
fn over_readings(computed: Option<&IndexMap<InputName, InputRecord>>) -> bool {
    computed.is_some_and(|inputs| inputs.contains_key(&InputName::Named(readings_key())))
}

/// The readings alone, or `None` for a property with none.
pub fn of_readings(property: &PropertySchema) -> Option<Fingerprint> {
    crate::format::canonicalization::readings_form(property).map(|form| digest(&form))
}

/// A column's cells in **row order**, each through `of`.
///
/// Row order and not index order: the file's order is what a reader sees, and
/// sorting first would make the digest depend on a comparison the file does not
/// record.
pub fn of_column(column: &ColumnView) -> Result<Fingerprint, SchemaError> {
    let key = (column.name().to_string(), column.identity());
    if let Some(taken) = PASS.with(|pass| {
        pass.borrow()
            .as_ref()
            .and_then(|taken| taken.get(&key).cloned())
    }) {
        return Ok(taken);
    }
    let mut joined = String::new();
    for (_, cell) in column.cells() {
        joined.push_str(&value_form(&schema::property_as_is(cell)));
        joined.push('\n');
    }
    let taken = digest(&joined);
    PASS.with(|pass| {
        if let Some(held) = pass.borrow_mut().as_mut() {
            held.insert(key, taken.clone());
        }
    });
    Ok(taken)
}

type ColumnKey = (String, (usize, usize));

thread_local! {
    /// The column digests taken during one pass over a sample. A column read by
    /// every cell of another was hashed once per cell, which made a table of n
    /// rows cost n² — minutes for twenty thousand.
    static PASS: std::cell::RefCell<Option<std::collections::HashMap<ColumnKey, Fingerprint>>> =
        const { std::cell::RefCell::new(None) };
    static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One reading or recording of a sample, during which no value changes: what
/// it hashes once it need not hash again. Nested passes share the outer one, so
/// a caller asking about every cell in turn opens one around its loop.
pub struct Pass;

impl Pass {
    pub fn begin() -> Pass {
        DEPTH.with(|depth| {
            if depth.get() == 0 {
                PASS.with(|pass| *pass.borrow_mut() = Some(std::collections::HashMap::new()));
            }
            depth.set(depth.get() + 1);
        });
        Pass
    }
}

impl Drop for Pass {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            depth.set(depth.get() - 1);
            if depth.get() == 0 {
                PASS.with(|pass| *pass.borrow_mut() = None);
            }
        });
    }
}

/// What one check has already concluded about one sample, by property, by cell
/// and by column.
#[derive(Default)]
struct Concluded {
    sample: usize,
    properties: std::collections::HashMap<Identifier, Freshness>,
    cells: std::collections::HashMap<(Identifier, Identifier, usize), Freshness>,
    columns: std::collections::HashMap<(Identifier, Identifier), bool>,
}

thread_local! {
    /// The verdicts reached during one check. Every cell of a column read whole
    /// was judged again for each value reading it, and each of those judged its
    /// own upstream again: a sample of three tables of eight rows took a tenth
    /// of a second.
    static VERDICTS: std::cell::RefCell<Option<Concluded>> =
        const { std::cell::RefCell::new(None) };
    static CHECKING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// One check of one sample, during which nothing a verdict rests on changes: no
/// value, no record, no hold. Every `check` opens one; nested checks share the
/// outer one, so a caller asking about each value of a sample in turn opens one
/// around its loop.
///
/// It is not the pass. A verdict rests on records, and `stamp` writes records
/// between the checks it makes inside one pass.
pub struct Verdicts;

impl Verdicts {
    pub fn begin() -> Verdicts {
        CHECKING.with(|depth| {
            if depth.get() == 0 {
                VERDICTS.with(|held| *held.borrow_mut() = Some(Concluded::default()));
            }
            depth.set(depth.get() + 1);
        });
        Verdicts
    }
}

impl Drop for Verdicts {
    fn drop(&mut self) {
        CHECKING.with(|depth| {
            depth.set(depth.get() - 1);
            if depth.get() == 0 {
                VERDICTS.with(|held| *held.borrow_mut() = None);
            }
        });
    }
}

/// A verdict already reached in this check, or `judge`'s, kept for the next
/// asker. An error is never kept: it ends the check that met it.
fn remembered<K: std::hash::Hash + Eq, T: Clone>(
    sample: &Sample,
    kept: fn(&mut Concluded) -> &mut std::collections::HashMap<K, T>,
    key: K,
    judge: impl FnOnce() -> Result<T, FingerprintError>,
) -> Result<T, FingerprintError> {
    let at = std::ptr::from_ref(sample).addr();
    let known = VERDICTS.with(|held| {
        held.borrow_mut().as_mut().and_then(|concluded| {
            // Another sample's verdicts say nothing about this one.
            if concluded.sample != at {
                *concluded = Concluded {
                    sample: at,
                    ..Concluded::default()
                };
            }
            kept(concluded).get(&key).cloned()
        })
    });
    if let Some(known) = known {
        return Ok(known);
    }
    let verdict = judge()?;
    VERDICTS.with(|held| {
        if let Some(concluded) = held.borrow_mut().as_mut()
            && concluded.sample == at
        {
            kept(concluded).insert(key, verdict.clone());
        }
    });
    Ok(verdict)
}

/// One table's cells, per column, keyed by the index tuple of their row.
pub type PerCell<T> = IndexMap<Identifier, Vec<(Vec<Value>, T)>>;

/// How a derived value stands against what it was computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// It records no inputs. It was entered or measured, not derived.
    Source,
    /// Every input exists, every digest matches, and every derived input is
    /// itself current.
    Current,
    /// `changed` names inputs whose content differs from what was recorded;
    /// `upstream` names inputs that are themselves not current. Two lists,
    /// because they send the reader to different places.
    Stale {
        changed: Vec<InputName>,
        upstream: Vec<InputName>,
    },
    /// A record describing a file that no longer exists in that shape.
    Broken {
        missing: Vec<InputName>,
        changed_kind: Vec<InputName>,
    },
    /// The property's own values no longer match the fingerprint recorded when
    /// it was computed. Reported **before** `Stale`: a value no formula
    /// produced makes the question *have its inputs moved* moot.
    Edited,
    /// Its formula raised, and no value stands.
    Failed { message: String },
    /// A value its file holds for a formula, with no record of how it came: an
    /// override until forced back to its formula.
    RecordMissing,
    /// The walk that would have judged it could not finish: its records lie on
    /// a cycle or lead to one. `reason` is the error's own sentence.
    Unjudged { reason: String },
}

// ------------------------------------------------------------- recording

/// One `Records` per property, taken **while the formulas still exist**.
///
/// Recording after `materialize` is too late: a materialized property is
/// `Stored`, so nothing can tell a formula that read nothing from a value
/// somebody typed. The reserved name a declared statistic's readings are
/// recorded under. `sample::RESERVED` refuses it as a property name, so the key
/// is unambiguous. What the model declared, so the file states it. The
/// *judging* side reads it back from the file's own records, never from the
/// property.
fn declared_of(handle: &crate::core::sample::PropertyHandle) -> Option<DeclaredStatistics> {
    let declared = DeclaredStatistics {
        value: handle.peek(crate::core::property::Property::declared_location),
        uncertainty: handle.peek(crate::core::property::Property::declared_convention),
    };
    (!declared.is_empty()).then_some(declared)
}

pub fn readings_key() -> Identifier {
    Identifier::new("readings").expect("a reserved name is a name")
}

pub fn record(sample: &Sample) -> Result<IndexMap<Identifier, Records>, FingerprintError> {
    let _pass = Pass::begin();
    let names: Vec<Identifier> = sample.property_names().into_iter().cloned().collect();
    // A property that something derives from carries a fingerprint and no
    // `computed`; the two records are on different sets.
    let mut is_input: Vec<Identifier> = Vec::new();
    for name in &names {
        for input in sample.dependencies_of(name).unwrap_or_default() {
            if let Node::Named(named) = input
                && !is_input.contains(&named)
            {
                is_input.push(named);
            }
        }
    }

    let mut records = IndexMap::new();
    for name in &names {
        let handle = sample.property(name).expect("just enumerated");
        // A value a file supplied keeps the record describing it until its
        // formula runs again, which clears the record. Restamping it would set
        // today's inputs beside a number computed from earlier ones.
        let carried = handle.records();
        if carried.computed.is_some() {
            records.insert(name.clone(), carried);
            continue;
        }
        let derived = is_derived(&handle);
        let mut channel_only = IndexMap::new();
        let computed = if derived {
            let mut inputs = IndexMap::new();
            // A declared statistic reads its own readings, which no graph edge
            // can name — `SelfDependency` forbids a property naming itself — so
            // the record names them under the reserved key. Without it the
            // property would write `computed: {}`, which is defined as a
            // formula that reads nothing of the sample.
            if handle.peek(crate::core::property::Property::has_declared_statistic)
                && let Some(readings) = of_readings(&handle.peek(schema::property_as_is))
            {
                inputs.insert(
                    InputName::Named(readings_key()),
                    InputRecord::Digest(readings),
                );
            }
            // A declared statistic with no readings left has nothing to record:
            // writing `computed: {}` would say *a formula that reads nothing*,
            // which is defined and which is not what happened.
            if inputs.is_empty() && !handle.is_computed() && !handle.has_uncertainty_formula() {
                records.insert(
                    name.clone(),
                    Records {
                        failure: None,
                        fingerprint: Some(of(&handle.peek(schema::property_as_is))),
                        computed: None,
                        produced: None,
                        statistics: declared_of(&handle),
                        channel_only: IndexMap::new(),
                    },
                );
                continue;
            }
            channel_only = declared_inputs(sample, name, &mut inputs)?;
            Some(inputs)
        } else {
            None
        };
        let fingerprint = if derived || is_input.contains(name) {
            Some(of(&handle.peek(schema::property_as_is)))
        } else {
            None
        };
        if computed.is_some() || fingerprint.is_some() {
            let produced = produced_channel(&handle);
            let fingerprint = fingerprint.map(|_| {
                own_digest(
                    &handle.peek(schema::property_as_is),
                    computed.as_ref(),
                    produced,
                )
            });
            records.insert(
                name.clone(),
                Records {
                    failure: None,
                    fingerprint,
                    computed,
                    produced,
                    statistics: declared_of(&handle),
                    channel_only,
                },
            );
        }
    }
    Ok(records)
}

/// Which channel this property's formulas produced, where exactly one did.
///
/// Asked **here**, while the formulas exist. By the time a file is written
/// `save_computed` has materialized the sample and every property answers
/// *not computed*, which is why the obvious place — keying the record where it
/// becomes a schema — could never have worked.
fn produced_channel(handle: &crate::core::sample::PropertyHandle) -> Option<Produced> {
    // One call giving both numbers computes the *quantity*, and names no
    // channel: it cannot be recomputed by halves.
    if handle.peek(Property::is_joint) {
        return None;
    }
    match (handle.is_computed(), handle.has_uncertainty_formula()) {
        (true, false) => Some(Produced::Value),
        (false, true) => Some(Produced::Uncertainty),
        // Both, or neither: the quantity as a whole, which is the honest
        // answer — a hand on either number is an override of the quantity.
        _ => None,
    }
}

/// Every input of a property's formulas, and which channel reads one alone.
///
/// The graph holds every input that is an edge between two values. A property's
/// own value, read by its own uncertainty, is not one — nothing outside the
/// property is involved — so it is added from what the model declared channel
/// by channel.
fn declared_inputs(
    sample: &Sample,
    name: &Identifier,
    inputs: &mut IndexMap<InputName, InputRecord>,
) -> Result<IndexMap<InputName, Produced>, FingerprintError> {
    for node in sample.dependencies_of(name).unwrap_or_default() {
        let (key, record) = input_record(sample, name, &node)?;
        inputs.insert(key, record);
    }
    let Some(channels) = sample.channel_inputs_of(name) else {
        return Ok(IndexMap::new());
    };
    for input in channels.value.iter().chain(channels.uncertainty.iter()) {
        if let InputName::Column { table, column } = input
            && table == name
            && let Some(channel) = Produced::parse(column.as_str())
        {
            let shape = sample
                .property(name)
                .map_err(|_| missing(name))?
                .peek(schema::property_as_is);
            inputs.insert(
                input.clone(),
                InputRecord::Digest(of_channel(&shape, channel)),
            );
        }
    }
    let mut only = IndexMap::new();
    for input in inputs.keys() {
        if let Some(channel) = channels.only(input) {
            only.insert(input.clone(), channel);
        }
    }
    Ok(only)
}

fn input_record(
    sample: &Sample,
    dependent: &Identifier,
    node: &Node,
) -> Result<(InputName, InputRecord), FingerprintError> {
    match node {
        Node::Named(name) => {
            if let Ok(handle) = sample.property(name) {
                let shape = handle.peek(schema::property_as_is);
                let digest = if handle.is_edited() {
                    of(&shape).marked_edited()
                } else {
                    of(&shape)
                };
                return Ok((InputName::Named(name.clone()), InputRecord::Digest(digest)));
            }
            if let Ok(value) = sample.attribute(name) {
                let record = match value {
                    AttributeValue::Scalar(value) => InputRecord::Literal(value.clone()),
                    AttributeValue::List(_) => InputRecord::Digest(digest(&attribute_form(value))),
                };
                return Ok((InputName::Named(name.clone()), record));
            }
            Err(FingerprintError::UnknownInput {
                dependent: dependent.clone(),
                input: name.clone(),
            })
        }
        Node::Column { table, column } => {
            let held = sample
                .table(table)
                .map_err(|_| FingerprintError::UnknownInput {
                    dependent: dependent.clone(),
                    input: column.clone(),
                })?;
            let view = held
                .column(column)
                .map_err(|_| FingerprintError::UnknownInput {
                    dependent: dependent.clone(),
                    input: column.clone(),
                })?;
            let hash = of_column(&view).map_err(|error| FingerprintError::Unconvertible {
                property: column.clone(),
                reason: error.to_string(),
            })?;
            Ok((
                InputName::Column {
                    table: table.clone(),
                    column: column.clone(),
                },
                InputRecord::Digest(hash),
            ))
        }
    }
}

/// The same operation at cell scale: one `Records` per derived cell, keyed by
/// the index tuple of its row.
pub fn record_table(
    sample: &Sample,
    table: &Identifier,
) -> Result<PerCell<Records>, FingerprintError> {
    let _pass = Pass::begin();
    let held = sample
        .table(table)
        .map_err(|_| FingerprintError::UnknownInput {
            dependent: table.clone(),
            input: table.clone(),
        })?;
    let mut out = IndexMap::new();
    for column in held.column_names().into_iter().cloned().collect::<Vec<_>>() {
        let Some(inputs) = held.inputs_of(&column) else {
            continue;
        };
        let inputs: Vec<InputName> = inputs.to_vec();
        let mut per_row = Vec::new();
        for row in held.rows() {
            let index: Vec<Value> = row.index().into_iter().cloned().collect();
            // As for a property: a cell the file supplied keeps its record.
            if let Ok(cell) = row.cell(&column)
                && cell.records().computed.is_some()
            {
                per_row.push((index, cell.records().clone()));
                continue;
            }
            let mut computed = IndexMap::new();
            for input in &inputs {
                computed.insert(
                    input.clone(),
                    cell_input(sample, held, &row, input, &column)?,
                );
            }
            let cell = row
                .cell(&column)
                .map_err(|_| FingerprintError::UnknownInput {
                    dependent: column.clone(),
                    input: column.clone(),
                })?;
            let shape = schema::property_as_is(cell);
            per_row.push((
                index,
                Records {
                    failure: None,
                    fingerprint: Some(of(&shape)),
                    computed: Some(computed),
                    // A cell has one number, so there is no channel to name.
                    produced: None,
                    // No column declares a statistic.
                    statistics: None,
                    channel_only: IndexMap::new(),
                },
            ));
        }
        out.insert(column, per_row);
    }
    Ok(out)
}

fn cell_input(
    sample: &Sample,
    table: &crate::core::table::Table,
    row: &crate::core::table::RowView,
    input: &InputName,
    dependent: &Identifier,
) -> Result<InputRecord, FingerprintError> {
    match input {
        // This row's cell, which is what makes each cell answerable on its own.
        InputName::Cell(column) => {
            let cell = row
                .cell(column)
                .map_err(|_| FingerprintError::UnknownInput {
                    dependent: dependent.clone(),
                    input: column.clone(),
                })?;
            let shape = schema::property_as_is(cell);
            Ok(InputRecord::Digest(of(&shape)))
        }
        // A sample-level input appears, with the same digest, in every cell of
        // the column that reads it. That is not redundancy: it is what makes
        // each cell answerable on its own.
        InputName::Named(_) => {
            let node = Node::Named(match input {
                InputName::Named(name) => name.clone(),
                _ => unreachable!(),
            });
            Ok(input_record(sample, dependent, &node)?.1)
        }
        InputName::Column {
            table: named,
            column,
        } => {
            // A column of this table, or of another table of the sample.
            let held = if named == table.name() {
                table
            } else {
                sample
                    .table(named)
                    .map_err(|_| FingerprintError::UnknownInput {
                        dependent: dependent.clone(),
                        input: named.clone(),
                    })?
            };
            let view = held
                .column(column)
                .map_err(|_| FingerprintError::UnknownInput {
                    dependent: dependent.clone(),
                    input: column.clone(),
                })?;
            let hash = of_column(&view).map_err(|error| FingerprintError::Unconvertible {
                property: column.clone(),
                reason: error.to_string(),
            })?;
            Ok(InputRecord::Digest(hash))
        }
    }
}

// -------------------------------------------------------------- checking

/// Every property of the sample, with no model and nothing read from outside it.
pub fn check(sample: &Sample) -> Result<IndexMap<Identifier, Freshness>, FingerprintError> {
    let _pass = Pass::begin();
    let _verdicts = Verdicts::begin();
    let mut out = IndexMap::new();
    for name in sample
        .property_names()
        .into_iter()
        .cloned()
        .collect::<Vec<_>>()
    {
        out.insert(name.clone(), judged(check_property(sample, &name))?);
    }
    Ok(out)
}

/// An answer for many values does not fail as one: a walk that could not finish
/// costs that value its verdict and nothing else. Only a caller's own mistake —
/// a name the sample does not hold — is still an error.
fn judged(verdict: Result<Freshness, FingerprintError>) -> Result<Freshness, FingerprintError> {
    match verdict {
        // The path and nothing more: the reason is read in a table's cell.
        Err(FingerprintError::CyclicRecords { path }) => Ok(Freshness::Unjudged {
            reason: format!("its records form a cycle: {}", drawn(&path)),
        }),
        Err(error @ FingerprintError::Unconvertible { .. }) => Ok(Freshness::Unjudged {
            reason: error.to_string(),
        }),
        other => other,
    }
}

thread_local! {
    /// The properties being judged, across the whole walk. A cell whose record
    /// names a property asks about it through `check_property`, which used to
    /// start a fresh path: a property reading a column whose cell read the
    /// property went round forever, and the stack overflowed — `status`,
    /// `validate` and `explain` aborted on a file that must still open.
    static WALK: std::cell::RefCell<Vec<Identifier>> = const { std::cell::RefCell::new(Vec::new()) };
}

pub fn check_property(sample: &Sample, name: &Identifier) -> Result<Freshness, FingerprintError> {
    let _pass = Pass::begin();
    let _verdicts = Verdicts::begin();
    // A walk already under way is continued, not restarted, so that a cycle
    // through a table is met rather than followed.
    let mut visiting = WALK.with(|walk| walk.borrow().clone());
    check_named(sample, name, &mut visiting)
}

/// Keeps `name` on the walk while it is judged, and takes it off however the
/// judging ends.
struct Walking;

impl Walking {
    fn on(name: &Identifier) -> Walking {
        WALK.with(|walk| walk.borrow_mut().push(name.clone()));
        Walking
    }
}

impl Drop for Walking {
    fn drop(&mut self) {
        WALK.with(|walk| {
            walk.borrow_mut().pop();
        });
    }
}

fn check_named(
    sample: &Sample,
    name: &Identifier,
    visiting: &mut Vec<Identifier>,
) -> Result<Freshness, FingerprintError> {
    // A cycle is reported rather than making the load fail: a file whose
    // records were hand-edited into a loop must still open, because otherwise
    // it cannot be repaired.
    if let Some(at) = visiting.iter().position(|seen| seen == name) {
        let mut path = visiting[at..].to_vec();
        path.push(name.clone());
        return Err(FingerprintError::CyclicRecords { path });
    }
    // Kept whatever the path that led here: a verdict reached is one whose
    // walk met no cycle, and no later path can put one below it.
    remembered(
        sample,
        |concluded| &mut concluded.properties,
        name.clone(),
        || judge_named(sample, name, visiting),
    )
}

fn judge_named(
    sample: &Sample,
    name: &Identifier,
    visiting: &mut Vec<Identifier>,
) -> Result<Freshness, FingerprintError> {
    let Ok(handle) = sample.property(name) else {
        return Ok(Freshness::Source);
    };
    // An override, in a session before any record says so, or marked.
    let records = handle.records();
    // Peeked, never read: a read refreshes, and would clear the records it asks about.
    if handle.peek(Property::is_record_missing) {
        return Ok(Freshness::RecordMissing);
    }
    if handle.is_edited() || records_marked(&records) {
        return Ok(Freshness::Edited);
    }
    let failure = if is_derived(&handle) {
        handle.peek(Property::peek_failure)
    } else {
        records.failure.clone()
    };
    if let Some(message) = failure {
        return Ok(Freshness::Failed { message });
    }
    let Some(computed) = records.computed else {
        return Ok(Freshness::Source);
    };

    // `Edited` outranks everything: a value no formula produced makes the
    // question *have its inputs moved* moot. **Not for a property whose record
    // names its own readings**. Its value and its uncertainty are statistics
    // *of* those readings, so its own digest moves whenever they do and could
    // only ever answer `Edited` — the coarse question swallowing the fine one.
    // The `readings` input carries the verdict instead, and a hand that wrote a
    // value here is caught by the mark `records_marked` tested above, which is
    // the override's mechanism and needs no exception in this order.
    if let Some(recorded) = &records.fingerprint {
        let current = handle.peek(schema::property_as_is);
        if !own_digest(&current, Some(&computed), records.produced).same_digest(recorded) {
            return Ok(Freshness::Edited);
        }
    }

    visiting.push(name.clone());
    let walking = Walking::on(name);
    let verdict = compare(sample, name, &computed, visiting);
    drop(walking);
    visiting.pop();
    verdict
}

fn compare(
    sample: &Sample,
    dependent: &Identifier,
    computed: &IndexMap<InputName, InputRecord>,
    visiting: &mut Vec<Identifier>,
) -> Result<Freshness, FingerprintError> {
    let (mut missing, mut changed_kind) = (Vec::new(), Vec::new());
    let (mut changed, mut upstream) = (Vec::new(), Vec::new());

    for (input, recorded) in computed {
        match input {
            // The reserved key is **this property's own observations**, not a
            // sibling of that name — which cannot exist, `sample::RESERVED`
            // refusing it. Looking it up as a property is what made a freshly
            // taken record read `Broken`.
            InputName::Named(name) if *name == readings_key() => {
                let shape = sample
                    .property(dependent)
                    .map(|handle| handle.peek(schema::property_as_is))
                    .ok();
                match (shape.as_ref().and_then(of_readings), recorded) {
                    (None, _) => missing.push(input.clone()),
                    (Some(now), InputRecord::Digest(recorded)) => {
                        if !now.same_digest(recorded) {
                            changed.push(input.clone());
                        }
                    }
                    (Some(_), InputRecord::Literal(_)) => changed_kind.push(input.clone()),
                }
            }
            InputName::Named(name) => {
                let property = sample.property(name).ok();
                let attribute = sample.attribute(name).ok();
                match (property, attribute, recorded) {
                    (None, None, _) => missing.push(input.clone()),
                    // An input that was a property and is now an attribute, or
                    // the reverse: the record cannot be interpreted at all.
                    (Some(_), None, InputRecord::Literal(_)) => changed_kind.push(input.clone()),
                    (Some(handle), _, InputRecord::Digest(recorded)) => {
                        let shape = handle.peek(schema::property_as_is);
                        if !of(&shape).same_digest(recorded) {
                            changed.push(input.clone());
                        // An override is where staleness stops.
                        } else if !matches!(
                            check_named(sample, name, visiting)?,
                            Freshness::Source
                                | Freshness::Current
                                | Freshness::Edited
                                | Freshness::RecordMissing
                        ) {
                            upstream.push(input.clone());
                        }
                    }
                    (_, Some(AttributeValue::Scalar(value)), InputRecord::Literal(recorded)) => {
                        if !crate::core::value::equals(value, recorded) {
                            changed.push(input.clone());
                        }
                    }
                    (_, Some(value @ AttributeValue::List(_)), InputRecord::Digest(recorded)) => {
                        if !digest(&attribute_form(value)).same_digest(recorded) {
                            changed.push(input.clone());
                        }
                    }
                    (_, Some(_), _) => changed_kind.push(input.clone()),
                }
            }
            // One channel of a quantity, `p.v`: this property's own value read
            // by its own uncertainty, which is what an instrument rated at a
            // percentage of its reading declares.
            InputName::Column { table, column }
                if sample.table(table).is_err()
                    && sample.has_property(table)
                    && Produced::parse(column.as_str()).is_some() =>
            {
                let channel = Produced::parse(column.as_str()).expect("just read");
                let shape = sample
                    .property(table)
                    .map(|handle| handle.peek(schema::property_as_is));
                match (shape, recorded) {
                    (Err(_), _) => missing.push(input.clone()),
                    (Ok(shape), InputRecord::Digest(recorded)) => {
                        if !of_channel(&shape, channel).same_digest(recorded) {
                            changed.push(input.clone());
                        // Its own channel carries nothing upstream that the
                        // property's other inputs do not already carry, and
                        // following it would be following itself.
                        } else if table != dependent
                            && !matches!(
                                check_named(sample, table, visiting)?,
                                Freshness::Source
                                    | Freshness::Current
                                    | Freshness::Edited
                                    | Freshness::RecordMissing
                            )
                        {
                            upstream.push(input.clone());
                        }
                    }
                    (Ok(_), InputRecord::Literal(_)) => changed_kind.push(input.clone()),
                }
            }
            InputName::Cell(_) | InputName::Column { .. } => {
                // A cell's own-row inputs are compared by `check_cell`, which
                // has the row; a whole column by its digest below.
                if let InputName::Column { table, column } = input {
                    let view = sample
                        .table(table)
                        .ok()
                        .and_then(|held| held.column(column).ok());
                    match view {
                        None => missing.push(input.clone()),
                        Some(view) => {
                            let hash = of_column(&view).map_err(|error| {
                                FingerprintError::Unconvertible {
                                    property: column.clone(),
                                    reason: error.to_string(),
                                }
                            })?;
                            if !same_record(&InputRecord::Digest(hash), recorded) {
                                changed.push(input.clone());
                            } else if column_is_stale(sample, table, column)? {
                                upstream.push(input.clone());
                            }
                        }
                    }
                }
            }
        }
    }

    if !missing.is_empty() || !changed_kind.is_empty() {
        return Ok(Freshness::Broken {
            missing,
            changed_kind,
        });
    }
    if !changed.is_empty() || !upstream.is_empty() {
        return Ok(Freshness::Stale { changed, upstream });
    }
    Ok(Freshness::Current)
}

/// Whether a cell of a column no longer rests on its inputs. A property reading
/// the whole column is then stale upstream, though the column's digest has not
/// moved yet: the table was changed and nothing computed since.
fn column_is_stale(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
) -> Result<bool, FingerprintError> {
    remembered(
        sample,
        |concluded| &mut concluded.columns,
        (table.clone(), column.clone()),
        || {
            let Ok(held) = sample.table(table) else {
                return Ok(false);
            };
            for row in held.rows() {
                let index: Vec<Value> = row.index().into_iter().cloned().collect();
                if !matches!(
                    check_cell(sample, table, column, &index)?,
                    Freshness::Source
                        | Freshness::Current
                        | Freshness::Edited
                        | Freshness::RecordMissing
                ) {
                    return Ok(true);
                }
            }
            Ok(false)
        },
    )
}

pub fn check_table(
    sample: &Sample,
    table: &Identifier,
) -> Result<PerCell<Freshness>, FingerprintError> {
    let _pass = Pass::begin();
    let _verdicts = Verdicts::begin();
    let held = sample
        .table(table)
        .map_err(|_| FingerprintError::UnknownInput {
            dependent: table.clone(),
            input: table.clone(),
        })?;
    let mut out = IndexMap::new();
    for column in held.column_names().into_iter().cloned().collect::<Vec<_>>() {
        let mut per_row = Vec::new();
        for row in held.rows() {
            let index: Vec<Value> = row.index().into_iter().cloned().collect();
            let verdict = judged(check_cell(sample, table, &column, &index))?;
            per_row.push((index.clone(), verdict));
        }
        out.insert(column, per_row);
    }
    Ok(out)
}

pub fn check_cell(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
) -> Result<Freshness, FingerprintError> {
    let _pass = Pass::begin();
    let _verdicts = Verdicts::begin();
    let held = sample
        .table(table)
        .map_err(|_| FingerprintError::UnknownInput {
            dependent: table.clone(),
            input: table.clone(),
        })?;
    let address = RowAddress::Index(index.to_vec());
    let row = held
        .row(&address)
        .map_err(|_| FingerprintError::UnknownInput {
            dependent: column.clone(),
            input: column.clone(),
        })?;
    let cell = row
        .cell(column)
        .map_err(|_| FingerprintError::UnknownInput {
            dependent: column.clone(),
            input: column.clone(),
        })?;
    remembered(
        sample,
        |concluded| &mut concluded.cells,
        (table.clone(), column.clone(), row.position()),
        || judge_cell(sample, table, column, held, &address, &row, cell),
    )
}

/// How every derived value of a sample stands: `check`, and `check_table` for
/// each table, composed — what `status`, `validate`, a terminal's marks and a
/// screen of the workbench all ask, so that none of them composes it again.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SampleFreshness {
    pub properties: IndexMap<Identifier, Freshness>,
    pub tables: IndexMap<Identifier, PerCell<Freshness>>,
}

/// Never fails: a verdict that could not be reached is `Unjudged` on the values
/// it was owed to, and every other value is judged.
pub fn check_sample(sample: &Sample) -> SampleFreshness {
    // One pass: what a table reads of the sample is judged once.
    let _pass = Pass::begin();
    let _verdicts = Verdicts::begin();
    let unjudged = |error: &FingerprintError| Freshness::Unjudged {
        reason: error.to_string(),
    };
    let properties = check(sample).unwrap_or_else(|error| {
        sample
            .property_names()
            .into_iter()
            .map(|name| (name.clone(), unjudged(&error)))
            .collect()
    });
    let mut tables = IndexMap::new();
    for table in sample.table_names() {
        let cells = check_table(sample, table).unwrap_or_else(|error| {
            let Ok(held) = sample.table(table) else {
                return IndexMap::new();
            };
            held.column_names()
                .into_iter()
                .map(|column| {
                    let rows = held
                        .rows()
                        .map(|row| {
                            let index = row.index().into_iter().cloned().collect();
                            (index, unjudged(&error))
                        })
                        .collect();
                    (column.clone(), rows)
                })
                .collect()
        });
        tables.insert(table.clone(), cells);
    }
    SampleFreshness { properties, tables }
}

/// `check_cell` for a caller answering for many cells — a table drawn, a column
/// exported: a walk that could not finish is that cell's `Unjudged` verdict
/// rather than an error the caller can only drop.
pub fn check_cell_among(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
) -> Result<Freshness, FingerprintError> {
    judged(check_cell(sample, table, column, index))
}

fn judge_cell(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    held: &crate::core::table::Table,
    address: &RowAddress,
    row: &crate::core::table::RowView,
    cell: &Property,
) -> Result<Freshness, FingerprintError> {
    let records = cell.records().clone();
    // A derived cell holding a value that no record explains and no formula
    // wrote in this session: record missing, as a property is.
    if records.computed.is_none()
        && records.failure.is_none()
        && held.is_derived(column)
        && !held.ran(address, column).unwrap_or(false)
        && cell.peek_value().is_some_and(|value| !value.is_absent())
    {
        return Ok(Freshness::RecordMissing);
    }
    // A cell held as an override, or marked so in its file.
    if held.is_held(address, column).unwrap_or(false) || records_marked(&records) {
        return Ok(Freshness::Edited);
    }
    let failure = if cell.is_computed() {
        cell.peek_failure()
    } else {
        records.failure.clone()
    };
    if let Some(message) = failure {
        return Ok(Freshness::Failed { message });
    }
    let Some(computed) = records.computed else {
        return Ok(Freshness::Source);
    };
    let shape = schema::property_as_is(cell);
    // A statistic of the cell's own readings takes its digest without them, as
    // a property's does: a corrected reading is *stale — readings*, never a
    // hand on the number.
    if let Some(recorded) = &records.fingerprint
        && !own_digest(&shape, Some(&computed), records.produced).same_digest(recorded)
    {
        return Ok(Freshness::Edited);
    }

    let (mut changed, mut missing, mut upstream) = (Vec::new(), Vec::new(), Vec::new());
    for (input, recorded) in &computed {
        let current = match input {
            // Its own readings, which the statistic its column declares was
            // taken over.
            InputName::Named(name) if *name == readings_key() => match of_readings(&shape) {
                Some(readings) => InputRecord::Digest(readings),
                None => {
                    missing.push(input.clone());
                    continue;
                }
            },
            InputName::Cell(other) => match row.cell(other) {
                Err(_) => {
                    missing.push(input.clone());
                    continue;
                }
                Ok(cell) => InputRecord::Digest(of(&schema::property_as_is(cell))),
            },
            InputName::Named(_) | InputName::Column { .. } => {
                let node = match input {
                    InputName::Named(name) => Node::Named(name.clone()),
                    InputName::Column { table, column } => Node::Column {
                        table: table.clone(),
                        column: column.clone(),
                    },
                    InputName::Cell(_) => unreachable!(),
                };
                match input_record(sample, column, &node) {
                    Err(FingerprintError::UnknownInput { .. }) => {
                        missing.push(input.clone());
                        continue;
                    }
                    Err(other) => return Err(other),
                    Ok((_, record)) => record,
                }
            }
        };
        if !same_record(&current, recorded) {
            changed.push(input.clone());
        // A cell reading a value that is itself stale rests on it, as a
        // property does.
        } else if let InputName::Named(name) = input
            && sample.property(name).is_ok()
            && !matches!(
                check_property(sample, name)?,
                Freshness::Source
                    | Freshness::Current
                    | Freshness::Edited
                    | Freshness::RecordMissing
            )
        {
            upstream.push(input.clone());
        // A column of another table that is itself not current.
        } else if let InputName::Column {
            table: named,
            column: read,
        } = input
            && named != table
            && column_is_stale(sample, named, read)?
        {
            upstream.push(input.clone());
        }
    }
    if !missing.is_empty() {
        return Ok(Freshness::Broken {
            missing,
            changed_kind: Vec::new(),
        });
    }
    if !changed.is_empty() || !upstream.is_empty() {
        return Ok(Freshness::Stale { changed, upstream });
    }
    Ok(Freshness::Current)
}

// --------------------------------------------------------------- stamping

/// Records what can be recorded truthfully now, and marks current what still
/// rests on its inputs. Runs no formula.
///
/// A value computed in this session, whose inputs are still the ones it was
/// computed from, is given its records now — while they can still be true — so
/// that a save running no formula writes it stale once its inputs move. A value
/// whose record still matches its inputs is marked current, so that a
/// recomputation which changed nothing reruns nothing downstream.
///
/// Returns every value it could not record: computed in this session and
/// overtaken by its inputs before anything recorded it.
pub fn stamp(sample: &mut Sample) -> Result<Vec<String>, FingerprintError> {
    let _pass = Pass::begin();
    let mut unrecordable = Vec::new();
    let names: Vec<Identifier> = sample.property_names().into_iter().cloned().collect();
    for name in &names {
        let handle = sample.property(name).map_err(|_| missing(name))?;
        // A value written over a declared statistic is an override like any
        // other, and is stamped as one. Unmarked, the save gave it a fresh
        // digest and the file read *current*: `set` marked its own and a save
        // from Python did not, over one rule.
        let written_over = handle.peek(Property::holds_written_override);
        if written_over && handle.records().computed.is_none() {
            // No record says what the statistic once gave, so nothing here can
            // say a hand wrote this: the state of a migrated file, left as it
            // is rather than certified as the statistic's product.
            continue;
        }
        if handle.is_edited() || written_over {
            // A value the file supplied for a formula, with no record of how it
            // came, is *record missing* — not edited. It is held in the same
            // cache state, which is what made the two look alike here, and
            // stamping it `{edited: …}` makes the file claim a hand typed a
            // number nobody touched. That is the ordinary state of a migrated
            // collection, and writing anything else into one of its samples
            // used to convert the lot, silently.
            if handle.peek(Property::is_record_missing) {
                continue;
            }
            let mut records = handle.records();
            records.fingerprint = Some(
                own_digest(
                    &handle.peek(schema::property_as_is),
                    records.computed.as_ref(),
                    records.produced,
                )
                .marked_edited(),
            );
            handle.set_records(records);
            continue;
        }
        if handle.records().computed.is_some() {
            if !handle.is_current() && inputs_unchanged(&check_property(sample, name)?) {
                handle.mark_current();
            }
            // A record made before the channel was written carries none, and
            // this is the last moment the model can say which one it is: the
            // save that follows materializes the sample.
            //
            // **The digest beside the record is retaken in the same breath.** It
            // was taken under the old reading — the whole quantity — and naming
            // a channel changes how it is read: left as it was, the next check
            // narrowed it, found it no longer matched, called the value edited,
            // and a load then held it as an override that nothing would ever
            // recompute. And it is retaken only where it still vouches for what
            // is there: a number a hand changed is left read as it was written,
            // edited, rather than blessed by a fresh digest.
            let produced = produced_channel(&handle);
            if produced.is_some() && handle.records().produced != produced {
                let mut records = handle.records();
                let shape = handle.peek(schema::property_as_is);
                let untouched = records.fingerprint.as_ref().is_some_and(|recorded| {
                    !recorded.is_edited()
                        && own_digest(&shape, records.computed.as_ref(), records.produced)
                            .same_digest(recorded)
                });
                if untouched {
                    records.produced = produced;
                    records.fingerprint =
                        Some(own_digest(&shape, records.computed.as_ref(), produced));
                    handle.set_records(records);
                }
            }
            continue;
        }
        if !is_derived(&handle) || !has_produced(&handle) {
            continue;
        }
        if !handle.is_current() {
            unrecordable.push(name.to_string());
            continue;
        }
        let mut computed = IndexMap::new();
        // Its own observations, under the reserved key, as `record` writes
        // them. This is the save path the command line takes.
        if handle.peek(crate::core::property::Property::has_declared_statistic)
            && let Some(readings) = of_readings(&handle.peek(schema::property_as_is))
        {
            computed.insert(
                InputName::Named(readings_key()),
                InputRecord::Digest(readings),
            );
        }
        let channel_only = declared_inputs(sample, name, &mut computed)?;
        // A declared statistic whose readings are gone records nothing: an empty
        // map would say *a formula that reads nothing*.
        let computed =
            (!computed.is_empty() || handle.is_computed() || handle.has_uncertainty_formula())
                .then_some(computed);
        let produced = produced_channel(&handle);
        handle.set_records(Records {
            failure: None,
            fingerprint: Some(own_digest(
                &handle.peek(schema::property_as_is),
                computed.as_ref(),
                produced,
            )),
            computed,
            produced,
            statistics: declared_of(&handle),
            channel_only,
        });
    }

    let tables: Vec<Identifier> = sample.table_names().into_iter().cloned().collect();
    for table in &tables {
        let (columns, indexes) = {
            let held = sample.table(table).map_err(|_| missing(table))?;
            let columns: Vec<Identifier> = held
                .column_names()
                .into_iter()
                .filter(|column| held.is_derived(column))
                .cloned()
                .collect();
            let indexes: Vec<Vec<Value>> = held
                .index_tuples()
                .into_iter()
                .map(|tuple| tuple.into_iter().cloned().collect())
                .collect();
            (columns, indexes)
        };
        for column in &columns {
            for index in &indexes {
                let address = RowAddress::Index(index.clone());
                let carried = sample
                    .table(table)
                    .map_err(|_| missing(column))?
                    .at(&address, column)
                    .map_err(|_| missing(column))?
                    .records()
                    .computed
                    .is_some();
                let run = sample
                    .cell_run(table, &address, column)
                    .map_err(|_| missing(column))?;
                // A held cell keeps the record its formula left, and its own
                // fingerprint says it was edited.
                if sample
                    .is_cell_held(table, &address, column)
                    .map_err(|_| missing(column))?
                {
                    if carried {
                        let records = {
                            let held = sample.table(table).map_err(|_| missing(column))?;
                            let cell = held.at(&address, column).map_err(|_| missing(column))?;
                            let mut records = cell.records().clone();
                            records.fingerprint =
                                Some(of(&schema::property_as_is(cell)).marked_edited());
                            records
                        };
                        sample
                            .set_cell_records(table, &address, column, records)
                            .map_err(|_| missing(column))?;
                    }
                    continue;
                }
                if carried {
                    if run == CellRun::Moved
                        && inputs_unchanged(&check_cell(sample, table, column, index)?)
                    {
                        sample
                            .confirm_cell(table, &address, column)
                            .map_err(|_| missing(column))?;
                    }
                    continue;
                }
                // Only what a formula wrote in this session is recorded: a cell
                // the file supplied without a record is written as it stands.
                if !sample
                    .cell_ran(table, &address, column)
                    .map_err(|_| missing(column))?
                {
                    continue;
                }
                match run {
                    CellRun::NotDerived | CellRun::NeverRan => {}
                    CellRun::Moved => {
                        unrecordable.push(format!("{table}.{column}[{}]", show_index(index)));
                    }
                    CellRun::Current => {
                        let records = {
                            let held = sample.table(table).map_err(|_| missing(column))?;
                            let row = held.row(&address).map_err(|_| missing(column))?;
                            let inputs: Vec<InputName> =
                                held.inputs_of(column).unwrap_or_default().to_vec();
                            let mut computed = IndexMap::new();
                            for input in &inputs {
                                let record = cell_input(sample, held, &row, input, column)?;
                                computed.insert(input.clone(), record);
                            }
                            let cell = row.cell(column).map_err(|_| missing(column))?;
                            Records {
                                failure: None,
                                fingerprint: Some(of(&schema::property_as_is(cell))),
                                computed: Some(computed),
                                produced: None,
                                // No column declares a statistic.
                                statistics: None,
                                channel_only: IndexMap::new(),
                            }
                        };
                        sample
                            .set_cell_records(table, &address, column, records)
                            .map_err(|_| missing(column))?;
                    }
                }
            }
        }
    }
    stamp_cell_statistics(sample)?;
    Ok(unrecordable)
}

/// A cell whose value is the statistic its column declares of its readings is
/// recorded as computed from them, as a property's declared statistic is: the
/// readings under the reserved key, and its own digest taken without them — so
/// that new readings leave it *stale — readings* and never read as a value
/// somebody typed.
///
/// Only a cell whose value **is** the statistic now: one with a value written
/// beside its readings and no record is a value written, and stays one; one
/// already recorded keeps its record, current or stale, until a computation
/// takes the statistic again.
fn stamp_cell_statistics(sample: &mut Sample) -> Result<(), FingerprintError> {
    let tables: Vec<Identifier> = sample.table_names().into_iter().cloned().collect();
    for table in &tables {
        let mut records = Vec::new();
        {
            let held = sample.table(table).map_err(|_| missing(table))?;
            for column in held.column_names() {
                if held.is_derived(column)
                    || held
                        .column(column)
                        .map_or(true, |view| view.statistics().is_empty())
                {
                    continue;
                }
                for row in held.rows() {
                    let cell = row.cell(column).map_err(|_| missing(column))?;
                    if let Some(record) = statistic_record(cell) {
                        records.push((
                            RowAddress::Index(row.index().into_iter().cloned().collect()),
                            column.clone(),
                            record,
                        ));
                    }
                }
            }
        }
        for (address, column, record) in records {
            sample
                .set_cell_records(table, &address, &column, record)
                .map_err(|_| missing(&column))?;
        }
    }
    Ok(())
}

/// One cell's statistic recorded as it stands, where it is one: what a
/// computation that has just taken it again writes at once, so that the cell
/// reads current before any save.
pub fn record_cell_statistic(
    sample: &mut Sample,
    table: &Identifier,
    row: &RowAddress,
    column: &Identifier,
) -> Result<(), FingerprintError> {
    let record = {
        let held = sample.table(table).map_err(|_| missing(table))?;
        let derived = held.is_derived(column)
            || held
                .column(column)
                .map_or(true, |view| view.statistics().is_empty());
        let cell = held.at(row, column).map_err(|_| missing(column))?;
        if derived {
            None
        } else {
            statistic_record(cell)
        }
    };
    if let Some(record) = record {
        sample
            .set_cell_records(table, row, column, record)
            .map_err(|_| missing(column))?;
    }
    Ok(())
}

/// The record of a cell whose value is its column's statistic of its
/// readings — `None` where a value is written beside them, where one is
/// recorded already, or where it holds no readings.
fn statistic_record(cell: &Property) -> Option<Records> {
    if cell.records().computed.is_some() || cell.written_value().is_some() {
        return None;
    }
    let shape = schema::property_as_is(cell);
    let readings = of_readings(&shape)?;
    let mut computed = IndexMap::new();
    computed.insert(
        InputName::Named(readings_key()),
        InputRecord::Digest(readings),
    );
    let fingerprint = own_digest(&shape, Some(&computed), None);
    Some(Records {
        failure: None,
        fingerprint: Some(fingerprint),
        computed: Some(computed),
        produced: None,
        // Its column states them, once for every cell.
        statistics: None,
        channel_only: IndexMap::new(),
    })
}

/// Whether a value no longer rests on its inputs: its record no longer matches
/// them, or it was computed in this session from inputs that have moved since.
/// A value never computed has nothing to be stale.
pub fn is_stale(sample: &Sample, name: &Identifier) -> Result<bool, FingerprintError> {
    let _pass = Pass::begin();
    let handle = sample.property(name).map_err(|_| missing(name))?;
    if handle.records().computed.is_some() {
        return Ok(!matches!(
            check_property(sample, name)?,
            Freshness::Current
                | Freshness::Source
                | Freshness::Edited
                | Freshness::RecordMissing
                | Freshness::Failed { .. }
        ));
    }
    Ok(is_derived(&handle) && !handle.is_edited() && has_produced(&handle) && !handle.is_current())
}

/// The same question for one derived cell.
pub fn is_cell_stale(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
) -> Result<bool, FingerprintError> {
    let _pass = Pass::begin();
    let address = RowAddress::Index(index.to_vec());
    let carried = sample
        .table(table)
        .map_err(|_| missing(table))?
        .at(&address, column)
        .map_err(|_| missing(column))?
        .records()
        .computed
        .is_some();
    if carried {
        return Ok(!matches!(
            check_cell(sample, table, column, index)?,
            Freshness::Current | Freshness::Source
        ));
    }
    Ok(sample
        .cell_run(table, &address, column)
        .map_err(|_| missing(column))?
        == CellRun::Moved)
}

/// A record whose own inputs still hash as recorded, whatever lies upstream
/// of them: the record goes on reporting that on its own, and nothing it read
/// has changed, so running its formula again would change nothing.
fn inputs_unchanged(freshness: &Freshness) -> bool {
    match freshness {
        Freshness::Current => true,
        Freshness::Stale { changed, .. } => changed.is_empty(),
        _ => false,
    }
}

/// The overrides a value rests on, following its records: an input recorded
/// edited is named, and one recorded plainly is followed in turn — never past
/// an override, whose own record describes its formula, not what it rests on.
/// Nearest first, each once.
pub fn edited_upstream(sample: &Sample, name: &Identifier) -> Vec<Identifier> {
    let mut found = Vec::new();
    let mut visited = Vec::new();
    follow_edits(sample, name, &mut visited, &mut found);
    found
}

fn follow_edits(
    sample: &Sample,
    name: &Identifier,
    visited: &mut Vec<Identifier>,
    found: &mut Vec<Identifier>,
) {
    // A cycle in the records is `check`'s to report; here it only ends the walk.
    if visited.contains(name) {
        return;
    }
    visited.push(name.clone());
    let Ok(handle) = sample.property(name) else {
        return;
    };
    // In a session the declared graph answers, whatever has been read; a sample
    // read from a file alone has only its records.
    let declared = sample.dependencies_of(name).unwrap_or_default();
    if !declared.is_empty() {
        for node in declared {
            let Node::Named(input) = node else {
                continue;
            };
            if sample
                .property(&input)
                .is_ok_and(|handle| handle.is_edited())
            {
                if !found.contains(&input) {
                    found.push(input);
                }
            } else {
                follow_edits(sample, &input, visited, found);
            }
        }
        return;
    }
    let Some(computed) = handle.records().computed else {
        return;
    };
    for (input, record) in &computed {
        let (InputName::Named(input), InputRecord::Digest(digest)) = (input, record) else {
            continue;
        };
        if digest.is_edited() {
            if !found.contains(input) {
                found.push(input.clone());
            }
        } else {
            follow_edits(sample, input, visited, found);
        }
    }
}

fn records_marked(records: &Records) -> bool {
    records
        .fingerprint
        .as_ref()
        .is_some_and(Fingerprint::is_edited)
}

/// Two records of one input agree when they hash the same content, or hold the
/// same literal; a mark changes neither.
fn same_record(current: &InputRecord, recorded: &InputRecord) -> bool {
    match (current, recorded) {
        (InputRecord::Digest(current), InputRecord::Digest(recorded)) => {
            current.same_digest(recorded)
        }
        (InputRecord::Literal(current), InputRecord::Literal(recorded)) => {
            crate::core::value::equals(current, recorded)
        }
        _ => false,
    }
}

fn missing(name: &Identifier) -> FingerprintError {
    FingerprintError::UnknownInput {
        dependent: name.clone(),
        input: name.clone(),
    }
}

/// An index tuple as a reader writes it, for a message.
fn show_index(index: &[Value]) -> String {
    let parts: Vec<String> = index
        .iter()
        .map(|value| match value {
            Value::Integer(integer) => integer.to_string(),
            Value::Number(number) => number.to_string(),
            Value::Text(text) => format!("\"{text}\""),
            Value::Boolean(flag) => flag.to_string(),
            Value::Date(date) => date.iso(),
            Value::DateTime(moment) => moment.iso(),
            Value::Absent => "-".to_string(),
            Value::NotApplicable => "n/a".to_string(),
        })
        .collect();
    parts.join(", ")
}

// ------------------------------------------------------------------ errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    UnknownInput {
        dependent: Identifier,
        input: Identifier,
    },
    CyclicRecords {
        path: Vec<Identifier>,
    },
    /// A property that cannot be put into portable form — an unread promise.
    Unconvertible {
        property: Identifier,
        reason: String,
    },
}

/// `plato → haze → plato`, as `DependencyError::Cycle` draws it.
fn drawn(path: &[Identifier]) -> String {
    let names: Vec<String> = path.iter().map(Identifier::to_string).collect();
    names.join(" \u{2192} ")
}

impl fmt::Display for FingerprintError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FingerprintError::UnknownInput { dependent, input } => write!(
                f,
                "'{dependent}' is declared to derive from '{input}', which this \
                 sample does not contain: recording it anyway would claim fewer \
                 dependencies than the formula has"
            ),
            FingerprintError::CyclicRecords { path } => write!(
                f,
                "the records in this file form a cycle: {}. A file cannot be \
                 written this way, so it was edited by hand",
                drawn(path)
            ),
            FingerprintError::Unconvertible { property, reason } => {
                write!(f, "'{property}' cannot be recorded: {reason}")
            }
        }
    }
}

impl std::error::Error for FingerprintError {}
