//! The model's description, `.samplekit/model.json`: what the model declares,
//! written by the model itself as the worker imports it, and read by everything
//! that only needs the declarations — so that `status`, `new`, `set`, `list
//! figures` and the TUI start no Python while it is current.
//!

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::{
    Availability, ModelError, Planned, Reason, Template, VERSION, Worker, availability, digest_of,
    template_files, template_of,
};
use crate::config::project_config::ProjectConfig;
use crate::core::dependency_graph::Node;
use crate::core::formatting::Presentation;
use crate::core::identifier::Identifier;
use crate::core::property::{
    ChannelInputs, Compute, ComputeError, ComputeQuantity, InputName, Property,
};
use crate::core::sample::{AttributeValue, Sample};
use crate::core::statistics::Location;
use crate::core::table::{
    CellOutput, CellRun, ColumnMeta, ColumnSet, ComputeColumn, ComputeRow, Derivation, RowAddress,
    RowView, Scope, Table,
};
use crate::core::uncertainty::{Convention, Uncertainty};
use crate::core::value::{Readings, Value, recognize_text};
use crate::format::document;
use crate::format::fingerprint;

/// This layout's number: 2 since a table's `rows` holds the rows a model gives
/// it rather than their count.
pub const FORMAT: u32 = 2;

// ------------------------------------------------------------------- types

/// What a model declares, as `.samplekit/model.json` holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelDescription {
    pub format: u32,
    /// The version of SampleKit that wrote it.
    pub samplekit: String,
    pub model: DescribedModel,
    /// In the order the model declares them, as its class holds them.
    pub properties: IndexMap<String, DescribedProperty>,
    pub tables: IndexMap<String, DescribedTable>,
    pub attributes: IndexMap<String, Json>,
    /// In the order the model declares them.
    pub figures: Vec<DescribedFigure>,
}

/// The model it was taken from: its file, relative to the configuration's
/// folder where it lies below it, its class, and the digest of its files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescribedModel {
    pub path: String,
    pub class: String,
    pub digest: String,
}

/// A property as the model declares it. No symbol and no precision: both are
/// the project's. A description written before, with a `symbol`, still reads,
/// the field ignored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DescribedProperty {
    pub unit: Option<String>,
    pub value: Origin,
    pub uncertainty: Option<Origin>,
    /// Everything it is declared to read, as the dependency graph holds it.
    pub reads: Vec<String>,
    /// Its formula's digest.
    pub formula: Option<String>,
    /// A value the model gives where no file does.
    pub default: Option<Json>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DescribedTable {
    pub title: Option<String>,
    pub index: Vec<String>,
    /// The rows the model gives it itself (`rows=`), each its cells by column,
    /// so that a plan made from the description holds them as the model's
    /// sample does.
    pub rows: Vec<IndexMap<String, DescribedCell>>,
    /// In the table's order.
    pub columns: Vec<DescribedColumn>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DescribedColumn {
    pub name: String,
    pub unit: Option<String>,
    pub value: Origin,
    pub uncertainty: Option<Origin>,
    pub formula: Option<String>,
}

/// A cell of a row the model gives: what it holds, as a file would — a
/// value, an uncertainty, readings. A derived cell no formula has filled
/// holds none of them and is not written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DescribedCell {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncertainty: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readings: Option<Vec<f64>>,
}

impl DescribedCell {
    /// Whether it holds nothing to write.
    pub fn is_empty(&self) -> bool {
        self.value.is_none() && self.uncertainty.is_none() && self.readings.is_none()
    }

    /// The cell a table is given, as a file's cell is read: readings with the
    /// value written beside them, else the value; an uncertainty over either.
    fn cell(&self) -> Result<Property, ModelError> {
        let value = self.value.as_ref().map(cell_value_from_json);
        let mut cell = match &self.readings {
            Some(readings) => {
                let readings = Readings::new(readings.clone())
                    .map_err(|error| described_badly(error.to_string()))?;
                let mut measured = Property::measured(readings, None);
                measured.set_written_value(value);
                measured
            }
            None => Property::stored(value.unwrap_or_else(Value::absent)),
        };
        if let Some(magnitude) = self.uncertainty {
            cell.set_uncertainty(Some(
                Uncertainty::new(magnitude).map_err(|error| described_badly(error.to_string()))?,
            ));
        }
        Ok(cell)
    }
}

/// Where a value, or an uncertainty, comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum Origin {
    Entered,
    Formula {
        reads: Vec<String>,
    },
    /// One call gives the value and its uncertainty.
    Quantity {
        reads: Vec<String>,
    },
    Statistic {
        statistic: String,
    },
    /// A table's formula run once per row.
    Rows {
        reads: Vec<String>,
        fills: Vec<String>,
    },
    /// A table's formula run once for the whole column.
    Columns {
        reads: Vec<String>,
        fills: Vec<String>,
    },
}

impl Origin {
    /// Whether a formula gives it.
    pub fn is_formula(&self) -> bool {
        !matches!(self, Origin::Entered | Origin::Statistic { .. })
    }

    fn reads(&self) -> &[String] {
        match self {
            Origin::Formula { reads }
            | Origin::Quantity { reads }
            | Origin::Rows { reads, .. }
            | Origin::Columns { reads, .. } => reads,
            Origin::Entered | Origin::Statistic { .. } => &[],
        }
    }

    fn statistic(&self) -> Option<&str> {
        match self {
            Origin::Statistic { statistic } => Some(statistic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescribedFigure {
    pub name: String,
    pub draws: Draws,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Draws {
    Sample,
    Collection,
}

impl ModelDescription {
    pub fn property(&self, name: &str) -> Option<&DescribedProperty> {
        self.properties.get(name)
    }

    pub fn table(&self, name: &str) -> Option<&DescribedTable> {
        self.tables.get(name)
    }

    /// The figures' names, in the order the model declares them.
    pub fn figure_names(&self) -> Vec<String> {
        self.figures
            .iter()
            .map(|figure| figure.name.clone())
            .collect()
    }

    /// The values it declares — properties, tables and their columns — sorted.
    pub fn value_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.properties.keys().cloned().collect();
        for (name, table) in &self.tables {
            names.push(name.clone());
            names.extend(table.columns.iter().map(|column| column.name.clone()));
        }
        names.sort();
        names.dedup();
        names
    }

    /// The text written: one layout, the same bytes for the same model.
    pub fn to_text(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).expect("a description serialises");
        text.push('\n');
        text
    }
}

// ------------------------------------------------------------ the file

/// `.samplekit/model.json` beside the configuration.
pub fn description_path(config: &ProjectConfig) -> PathBuf {
    description_path_in(config.root())
}

/// The same, beside the configuration whose folder is `root`.
pub fn description_path_in(root: &Path) -> PathBuf {
    root.join(".samplekit").join("model.json")
}

/// The model file a description names, resolved against the configuration's
/// folder.
fn named_file(root: &Path, description: &ModelDescription) -> PathBuf {
    let named = PathBuf::from(&description.model.path);
    let path = if named.is_absolute() {
        named
    } else {
        root.join(named)
    };
    dunce::canonicalize(&path).unwrap_or(path)
}

/// Whether a description describes the model as its files now are.
fn describes(description: &ModelDescription, template: &Template) -> bool {
    description.format == FORMAT
        && description.samplekit == VERSION
        && named_file(template.root(), description) == template.path
        && template
            .class()
            .is_none_or(|class| class == description.model.class)
        && digest_of(template).is_ok_and(|digest| digest.as_str() == description.model.digest)
}

/// What the files a reading depends on looked like, to keep it while none moved.
type Stamp = Vec<(PathBuf, Option<(SystemTime, u64)>)>;

fn stamp_of(files: impl IntoIterator<Item = PathBuf>) -> Stamp {
    files
        .into_iter()
        .map(|file| {
            let seen = fs::metadata(&file)
                .ok()
                .and_then(|meta| Some((meta.modified().ok()?, meta.len())));
            (file, seen)
        })
        .collect()
}

thread_local! {
    static READ: RefCell<Vec<(Stamp, Option<ModelDescription>)>> = const { RefCell::new(Vec::new()) };
}

/// The description, only where it describes the model as its files now are:
/// the same file and class, the same digest, this version of SampleKit.
/// `None` otherwise — a stale description is never read. Kept while neither
/// it nor any file of the model moves.
pub fn current_description(config: &ProjectConfig) -> Option<ModelDescription> {
    let template = template_of(config)?;
    let path = description_path(config);
    let files = template_files(&template).ok()?;
    let stamp = stamp_of(std::iter::once(path.clone()).chain(files));
    if let Some(held) = READ.with(|read| {
        read.borrow()
            .iter()
            .find(|(at, _)| *at == stamp)
            .map(|(_, held)| held.clone())
    }) {
        return held;
    }
    let read = fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<ModelDescription>(&text).ok())
        .filter(|description| describes(description, &template));
    READ.with(|held| {
        let mut held = held.borrow_mut();
        held.retain(|(at, _)| at.first().map(|(file, _)| file) != Some(&path));
        held.push((stamp, read.clone()));
    });
    read
}

/// The current description, had written again by a worker where it is
/// missing or stale: the one Python start a command that needs it makes.
pub fn describe(config: &ProjectConfig, from: &Path) -> Result<ModelDescription, ModelError> {
    if let Some(current) = current_description(config) {
        return Ok(current);
    }
    let (template, python) = match availability(Some(config), from)? {
        Availability::Ready { template, python } => (template, python),
        Availability::Unavailable { reason } => return Err(ModelError::NoInterpreter { reason }),
        Availability::NoTemplate => {
            return Err(ModelError::ModelClass {
                message: "the configuration declares no model".to_string(),
            });
        }
    };
    Worker::start(&python)?.describe(&template)?;
    current_description(config).ok_or_else(|| ModelError::Unreadable {
        path: description_path(config),
        reason: "the model was described, and its description does not describe it".to_string(),
    })
}

/// Writes a description beside the configuration whose folder is `root`,
/// only where its content changed.
pub fn write_description(root: &Path, description: &ModelDescription) -> Result<(), ModelError> {
    let path = description_path_in(root);
    let text = description.to_text();
    if fs::read(&path).is_ok_and(|held| held == text.as_bytes()) {
        return Ok(());
    }
    let not_recorded = |error: std::io::Error| ModelError::NotRecorded {
        path: path.clone(),
        reason: error.to_string(),
    };
    let folder = path.parent().expect("the description lies in .samplekit/");
    fs::create_dir_all(folder).map_err(not_recorded)?;
    let written = folder.join(format!(".model.json.{}", std::process::id()));
    fs::write(&written, text).map_err(not_recorded)?;
    fs::rename(&written, &path).map_err(not_recorded)
}

/// Removes the description of a model that could not be read: none is
/// better than a stale one a reader of the file might trust.
pub fn forget_description(root: &Path) {
    let _ = fs::remove_file(description_path_in(root));
}

/// The model's file as a description names it: relative to the
/// configuration's folder, with `/` between its parts, where it lies below
/// it; absolute otherwise.
pub fn described_path(root: &Path, model: &Path) -> String {
    let own = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match own(model).strip_prefix(own(root)) {
        Ok(relative) => relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        Err(_) => model.to_string_lossy().into_owned(),
    }
}

// ------------------------------------------------- values as JSON holds them

/// A value as a description writes it: a number, a text, a yes-or-no;
/// `n/a` as its text, a date as its ISO text, nothing as `null`.
pub fn value_to_json(value: &Value) -> Json {
    match value {
        Value::Integer(integer) => Json::from(*integer),
        Value::Number(number) => {
            serde_json::Number::from_f64(*number).map_or(Json::Null, Json::Number)
        }
        Value::Text(text) => Json::String(text.clone()),
        Value::Boolean(flag) => Json::Bool(*flag),
        Value::Date(date) => Json::String(date.iso()),
        Value::DateTime(date_time) => Json::String(date_time.iso()),
        Value::Absent => Json::Null,
        Value::NotApplicable => Json::String("n/a".to_string()),
    }
}

/// A cell's value, read as a file reads one: a text shaped as a date is a
/// date, so that an index the model gives meets the same row a file holds.
fn cell_value_from_json(json: &Json) -> Value {
    match json {
        Json::String(text) => recognize_text(text).unwrap_or_else(|_| Value::Text(text.clone())),
        other => value_from_json(other),
    }
}

fn value_from_json(json: &Json) -> Value {
    match json {
        Json::Bool(flag) => Value::Boolean(*flag),
        Json::Number(number) => number
            .as_i64()
            .map(Value::Integer)
            .or_else(|| number.as_f64().and_then(|real| Value::number(real).ok()))
            .unwrap_or(Value::Absent),
        Json::String(text) if text == "n/a" => Value::NotApplicable,
        Json::String(text) => Value::Text(text.clone()),
        _ => Value::Absent,
    }
}

// ------------------------------------------------ the model, rebuilt

/// A formula held as one that never runs: what a sample built from a
/// description holds where the model holds its code. Planning reads no value
/// a formula gives, so none is ever asked for.
struct NeverRun;

fn never_run() -> ComputeError {
    ComputeError::failed(std::io::Error::other(
        "a formula described is not run: only the model computes",
    ))
}

impl Compute for NeverRun {
    fn compute(&self) -> Result<Value, ComputeError> {
        Err(never_run())
    }
}

impl ComputeQuantity for NeverRun {
    fn compute(&self) -> Result<(Value, Option<Uncertainty>), ComputeError> {
        Err(never_run())
    }
}

impl ComputeRow for NeverRun {
    fn compute(
        &self,
        _row: &RowView,
        _scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        Err(never_run())
    }
}

impl ComputeColumn for NeverRun {
    fn compute(
        &self,
        _columns: &ColumnSet,
        _scope: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        Err(never_run())
    }
}

fn identifier(name: &str) -> Result<Identifier, ModelError> {
    Identifier::new(name).map_err(|error| described_badly(format!("'{name}': {error}")))
}

fn described_badly(reason: String) -> ModelError {
    ModelError::Unreadable {
        path: PathBuf::from(".samplekit/model.json"),
        reason,
    }
}

fn node_of(text: &str) -> Result<Node, ModelError> {
    match text.split_once('.') {
        Some((table, column)) => Ok(Node::Column {
            table: identifier(table)?,
            column: identifier(column)?,
        }),
        None => Ok(Node::Named(identifier(text)?)),
    }
}

fn input_of(text: &str) -> Result<InputName, ModelError> {
    InputName::parse(text).map_err(described_badly)
}

fn location_of(name: &str) -> Result<Location, ModelError> {
    Location::from_name(name).ok_or_else(|| described_badly(format!("no statistic '{name}'")))
}

fn convention_of(name: &str) -> Result<Convention, ModelError> {
    Convention::from_name(name).ok_or_else(|| described_badly(format!("no statistic '{name}'")))
}

impl ModelDescription {
    /// A sample holding what the model declares, as the model's class holds
    /// it before a file is read: each formula held as one that never runs.
    fn declared_sample(&self) -> Result<Sample, ModelError> {
        let mut sample = Sample::new();
        let refused = |error: crate::core::sample::SampleError| described_badly(error.to_string());
        for (name, value) in &self.attributes {
            let value = match value {
                Json::Array(items) => {
                    AttributeValue::list(items.iter().map(value_from_json).collect())
                        .map_err(|error| described_badly(error.to_string()))?
                }
                other => AttributeValue::scalar(value_from_json(other)),
            };
            sample
                .set_attribute(identifier(name)?, value)
                .map_err(refused)?;
        }
        for (name, described) in &self.properties {
            let mut property = match &described.value {
                Origin::Formula { .. } => Property::computed(Rc::new(NeverRun)),
                Origin::Quantity { .. } => Property::joint(Rc::new(NeverRun)),
                _ => Property::stored(
                    described
                        .default
                        .as_ref()
                        .map_or(Value::Absent, value_from_json),
                ),
            };
            if matches!(described.uncertainty, Some(Origin::Formula { .. })) {
                property.set_uncertainty_formula(Rc::new(NeverRun));
            }
            let location = described.value.statistic().map(location_of).transpose()?;
            let convention = described
                .uncertainty
                .as_ref()
                .and_then(Origin::statistic)
                .map(convention_of)
                .transpose()?;
            property.declare_statistics(location, convention);
            property.set_presentation(Presentation {
                unit: described.unit.clone(),
                symbol: None,
                precision: None,
            });
            sample
                .set_property(identifier(name)?, property)
                .map_err(refused)?;
        }
        for (name, described) in &self.tables {
            let mut columns: IndexMap<Identifier, ColumnMeta> = IndexMap::new();
            let mut derivations: Vec<Derivation> = Vec::new();
            let mut derived: HashSet<Vec<String>> = HashSet::new();
            for column in &described.columns {
                let mut meta = ColumnMeta::default();
                meta.presentation.unit = column.unit.clone();
                meta.statistics.value = column.value.statistic().map(location_of).transpose()?;
                meta.statistics.uncertainty = column
                    .uncertainty
                    .as_ref()
                    .and_then(Origin::statistic)
                    .map(convention_of)
                    .transpose()?;
                columns.insert(identifier(&column.name)?, meta);
                let (fills, reads, by_row) = match &column.value {
                    Origin::Rows { reads, fills } => (fills, reads, true),
                    Origin::Columns { reads, fills } => (fills, reads, false),
                    _ => continue,
                };
                if !derived.insert(fills.clone()) {
                    continue;
                }
                let outputs = fills
                    .iter()
                    .map(|fill| identifier(fill))
                    .collect::<Result<Vec<_>, _>>()?;
                let inputs = reads
                    .iter()
                    .map(|read| input_of(read))
                    .collect::<Result<Vec<_>, _>>()?;
                derivations.push(if by_row {
                    Derivation::Row {
                        outputs,
                        inputs,
                        formula: Rc::new(NeverRun),
                    }
                } else {
                    Derivation::Column {
                        outputs,
                        inputs,
                        formula: Rc::new(NeverRun),
                    }
                });
            }
            let index = described
                .index
                .iter()
                .map(|column| identifier(column))
                .collect::<Result<Vec<_>, _>>()?;
            let mut table = Table::new(identifier(name)?, index, columns, derivations)
                .map_err(|error| described_badly(error.to_string()))?;
            table.set_title(described.title.clone());
            // The rows the model gives, which a file fills as it fills the
            // model's own: a cell of theirs no formula has run is owed.
            for row in &described.rows {
                let cells = row
                    .iter()
                    .map(|(column, cell)| Ok((identifier(column)?, cell.cell()?)))
                    .collect::<Result<Vec<_>, ModelError>>()?;
                table
                    .add_row(cells)
                    .map_err(|error| described_badly(error.to_string()))?;
            }
            sample
                .set_table(identifier(name)?, table)
                .map_err(refused)?;
        }
        Ok(sample)
    }

    /// Declares what each property reads, as the model settles it once its
    /// file is read.
    fn declare_inputs(&self, sample: &mut Sample) -> Result<(), ModelError> {
        for (name, described) in &self.properties {
            let reads_declared = described.value.is_formula()
                || described
                    .uncertainty
                    .as_ref()
                    .is_some_and(Origin::is_formula)
                || !described.reads.is_empty();
            if !reads_declared {
                continue;
            }
            let nodes = described
                .reads
                .iter()
                .map(|read| node_of(read))
                .collect::<Result<Vec<_>, _>>()?;
            let value = described.value.reads().to_vec();
            let uncertainty = described
                .uncertainty
                .as_ref()
                .map(|origin| origin.reads().to_vec())
                .unwrap_or_default();
            let channels = if described.value.is_formula()
                && described
                    .uncertainty
                    .as_ref()
                    .is_some_and(Origin::is_formula)
                && value != uncertainty
                && !matches!(described.value, Origin::Quantity { .. })
            {
                ChannelInputs {
                    value: value
                        .iter()
                        .map(|read| input_of(read))
                        .collect::<Result<_, _>>()?,
                    uncertainty: uncertainty
                        .iter()
                        .map(|read| input_of(read))
                        .collect::<Result<_, _>>()?,
                }
            } else {
                ChannelInputs::default()
            };
            sample
                .declare_dependencies_by_channel(&identifier(name)?, &nodes, channels)
                .map_err(|error| described_badly(error.to_string()))?;
        }
        Ok(())
    }

    /// The sample at `path` as its model reads it, built from the description.
    pub fn modelled(&self, path: &Path) -> Result<Sample, ModelError> {
        let mut sample = self.declared_sample()?;
        document::load_into(path, &mut sample).map_err(|error| ModelError::TemplateFailed {
            traceback: error.to_string(),
        })?;
        self.declare_inputs(&mut sample)?;
        sample.confirm_filled();
        Ok(sample)
    }

    /// What `status` plans without the model: the sample read from its file
    /// under the declarations the description holds, planned as the worker
    /// plans it for `status` — forced, every formula, nothing named — with the
    /// formulas whose digest differs from `recorded`.
    pub fn plan(
        &self,
        path: &Path,
        recorded: &BTreeMap<String, String>,
    ) -> Result<Vec<Planned>, ModelError> {
        let sample = self.modelled(path)?;
        let stored = document::load_sample(path).ok().map(|(sample, _)| sample);
        let mut changed: Vec<String> = Vec::new();
        for (name, property) in &self.properties {
            if let (Some(now), Some(then)) = (&property.formula, recorded.get(name))
                && now != then
            {
                changed.push(name.clone());
            }
        }
        for (name, table) in &self.tables {
            for column in &table.columns {
                let label = format!("{name}.{}", column.name);
                if let (Some(now), Some(then)) = (&column.formula, recorded.get(&label))
                    && now != then
                {
                    changed.push(label);
                }
            }
        }
        let planned = plan_of(&sample, &changed)
            .map_err(|reason| ModelError::TemplateFailed { traceback: reason })?;
        Ok(said(&sample, stored.as_ref(), planned)
            .into_iter()
            .map(|(value, said)| Planned {
                value,
                reason: Reason::from_worker(&said),
                said,
            })
            .collect())
    }
}

// ------------------------------------------------------- the plan

/// A column no formula fills whose cells' readings stand for their values
/// through the statistics it declares.
fn is_statistic_column(held: &Table, column: &Identifier) -> bool {
    !held.is_derived(column)
        && held
            .column(column)
            .is_ok_and(|view| !view.statistics().is_empty())
}

/// Readings whose declared statistic nothing has taken yet.
fn statistic_owed(handle: &crate::core::sample::PropertyHandle) -> bool {
    handle.records().computed.is_none()
        && handle.peek(|property| {
            property.has_declared_statistic()
                && property.readings().is_some()
                && property.written_value().is_none()
        })
}

/// A value its formula owes a first run.
fn owed(handle: &crate::core::sample::PropertyHandle) -> bool {
    (handle.is_computed() && handle.peek(Property::peek_value).is_none())
        || (handle.has_uncertainty_formula() && handle.peek(Property::peek_uncertainty).is_none())
        || statistic_owed(handle)
}

/// Why one cell of a derived or statistic column runs under `status`'s
/// forced plan, if it does.
fn cell_reason(
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
    rerun: bool,
) -> Result<Option<&'static str>, String> {
    let address = RowAddress::Index(index.to_vec());
    let held = sample.table(table).map_err(|error| error.to_string())?;
    if is_statistic_column(held, column) {
        let cell = held
            .at(&address, column)
            .map_err(|error| error.to_string())?;
        if cell.readings().is_none() {
            return Ok(None);
        }
        if cell.records().computed.is_none() {
            return Ok(Some(if cell.written_value().is_some() {
                "edited"
            } else {
                "never computed"
            }));
        }
        return Ok(
            match fingerprint::check_cell(sample, table, column, index)
                .map_err(|error| error.to_string())?
            {
                fingerprint::Freshness::Edited => Some("edited"),
                fingerprint::Freshness::Current => rerun.then_some("current"),
                _ => Some("outdated"),
            },
        );
    }
    if sample
        .is_cell_held(table, &address, column)
        .map_err(|error| error.to_string())?
    {
        return Ok(Some("edited"));
    }
    if sample
        .cell_run(table, &address, column)
        .map_err(|error| error.to_string())?
        == CellRun::NeverRan
    {
        return Ok(Some("never computed"));
    }
    if fingerprint::is_cell_stale(sample, table, column, index)
        .map_err(|error| error.to_string())?
    {
        return Ok(Some("outdated"));
    }
    Ok(rerun.then_some("current"))
}

/// Every value a formula gives, each with why it runs: what the worker's
/// plan gives for `status` — forced, nothing named, not rerun — in
/// dependency order, a column's cells as `table.column`.
fn plan_of(sample: &Sample, changed: &[String]) -> Result<Vec<(String, &'static str)>, String> {
    let _pass = fingerprint::Pass::begin();
    let _verdicts = fingerprint::Verdicts::begin();
    // A property, or a column once for each of its cells that runs.
    let mut chosen: Vec<(Node, &'static str)> = Vec::new();
    for name in sample.property_names() {
        let handle = sample.property(name).map_err(|error| error.to_string())?;
        if !handle.is_computed()
            && !handle.has_uncertainty_formula()
            && !handle.peek(Property::has_declared_statistic)
        {
            continue;
        }
        let reason = if handle.is_edited() || handle.peek(Property::holds_written_override) {
            Some("edited")
        } else if owed(&handle) {
            Some("never computed")
        } else if fingerprint::is_stale(sample, name).map_err(|error| error.to_string())? {
            Some("outdated")
        } else if changed.iter().any(|value| value == name.as_str()) {
            Some("formula changed")
        } else {
            None
        };
        if let Some(reason) = reason {
            chosen.push((Node::Named(name.clone()), reason));
        }
    }
    for table in sample.table_names() {
        let held = sample.table(table).map_err(|error| error.to_string())?;
        for column in held.column_names() {
            if !held.is_derived(column) && !is_statistic_column(held, column) {
                continue;
            }
            let formula_changed = changed.contains(&format!("{table}.{column}"));
            for tuple in held.index_tuples() {
                let index: Vec<Value> = tuple.into_iter().cloned().collect();
                let reason = match cell_reason(sample, table, column, &index, formula_changed)? {
                    Some("current") if formula_changed => Some("formula changed"),
                    other => other,
                };
                if let Some(reason) = reason {
                    chosen.push((
                        Node::Column {
                            table: table.clone(),
                            column: column.clone(),
                        },
                        reason,
                    ));
                }
            }
        }
    }
    Ok(in_order(sample, chosen))
}

/// What one node reads: a property's declared inputs, or a derived column's.
fn node_inputs(sample: &Sample, node: &Node) -> Vec<Node> {
    match node {
        Node::Named(name) => sample.dependencies_of(name).unwrap_or_default(),
        Node::Column { table, column } => sample
            .table(table)
            .ok()
            .and_then(|held| held.inputs_of(column).map(<[InputName]>::to_vec))
            .unwrap_or_default()
            .into_iter()
            .map(|input| match input {
                InputName::Named(name) => Node::Named(name),
                InputName::Cell(other) => Node::Column {
                    table: table.clone(),
                    column: other,
                },
                InputName::Column { table, column } => Node::Column { table, column },
            })
            .collect(),
    }
}

/// Grouped by value and ordered so that every value comes after what it reads.
fn in_order(sample: &Sample, reasoned: Vec<(Node, &'static str)>) -> Vec<(String, &'static str)> {
    let mut groups: IndexMap<Node, (String, &'static str)> = IndexMap::new();
    for (node, reason) in reasoned {
        groups
            .entry(node.clone())
            .or_insert((node.to_string(), reason));
    }
    let mut ordered = Vec::with_capacity(groups.len());
    let mut visited = HashSet::new();
    let nodes: Vec<Node> = groups.keys().cloned().collect();
    for node in &nodes {
        visit(sample, node, &groups, &mut visited, &mut ordered);
    }
    ordered
}

fn visit(
    sample: &Sample,
    node: &Node,
    groups: &IndexMap<Node, (String, &'static str)>,
    visited: &mut HashSet<Node>,
    ordered: &mut Vec<(String, &'static str)>,
) {
    if !visited.insert(node.clone()) {
        return;
    }
    for input in node_inputs(sample, node) {
        visit(sample, &input, groups, visited, ordered);
    }
    if let Some((label, reason)) = groups.get(node) {
        ordered.push((label.clone(), reason));
    }
}

/// The plan said as the worker says it for `status`: what reads a value whose
/// formula changed, a value waiting for what nobody entered, a failure the
/// file records.
fn said(
    sample: &Sample,
    stored: Option<&Sample>,
    plan: Vec<(String, &'static str)>,
) -> Vec<(String, String)> {
    let mut plan: Vec<(String, String)> = plan
        .into_iter()
        .map(|(value, reason)| (value, reason.to_string()))
        .collect();
    let mut planned: HashSet<String> = plan.iter().map(|(value, _)| value.clone()).collect();
    for (value, reason) in plan.clone() {
        if reason != "formula changed" {
            continue;
        }
        let Ok(node) = node_of(&value) else {
            continue;
        };
        for reader in sample.affected_by_change(&node).unwrap_or_default() {
            let reader = reader.to_string();
            if !planned.contains(&reader) && !edited(sample, &reader) {
                planned.insert(reader.clone());
                plan.push((reader, format!("reads {value}, whose formula changed")));
            }
        }
    }
    let (mut waiting, mut made) = (HashSet::new(), HashSet::new());
    plan.into_iter()
        .map(|(value, reason)| {
            let missing = missing_inputs(sample, stored, &value, &waiting, &made);
            if missing.is_empty() {
                made.insert(value.clone());
                let reason = if failed_before(sample, &value) {
                    "failed".to_string()
                } else {
                    reason
                };
                (value, reason)
            } else {
                waiting.insert(value.clone());
                let reason = format!("waits for {}", missing.join(", "));
                (value, reason)
            }
        })
        .collect()
}

fn property_of(sample: &Sample, name: &str) -> Option<crate::core::sample::PropertyHandle> {
    Identifier::new(name)
        .ok()
        .and_then(|name| sample.property(&name).ok())
}

/// Whether `value` holds an override: its state *edited*.
fn edited(sample: &Sample, value: &str) -> bool {
    property_of(sample, value).is_some_and(|handle| handle.is_edited())
}

/// Whether the file records a failure for `value`: its state *failed*.
fn failed_before(sample: &Sample, value: &str) -> bool {
    property_of(sample, value).is_some_and(|handle| {
        !handle.is_edited()
            && !statistic_owed(&handle)
            && (handle.is_computed() || handle.has_uncertainty_formula())
            && handle.has_failed()
    })
}

/// Whether a value that is no formula's holds one: a default counts.
fn holds_a_value(handle: &crate::core::sample::PropertyHandle) -> bool {
    handle
        .peek(Property::peek_value)
        .is_some_and(|value| !matches!(value, Value::Absent))
}

/// What `value` needs and nobody has given, as the worker's `missing_inputs`
/// says it: a value it is declared to read absent from the file and not
/// planned before it, one itself waiting, a column with no cell — and, for
/// an uncertainty's formula, the value it is the uncertainty of.
fn missing_inputs(
    sample: &Sample,
    stored: Option<&Sample>,
    value: &str,
    waiting: &HashSet<String>,
    made: &HashSet<String>,
) -> Vec<String> {
    let inputs: Vec<String> = match value.split_once('.') {
        Some((table, column)) => Identifier::new(table)
            .ok()
            .zip(Identifier::new(column).ok())
            .and_then(|(table, column)| {
                let held = sample.table(&table).ok()?;
                Some(
                    held.inputs_of(&column)
                        .map(|inputs| inputs.iter().map(ToString::to_string).collect())
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_default(),
        None => Identifier::new(value)
            .ok()
            .and_then(|name| sample.dependencies_of(&name).ok())
            .map(|nodes| nodes.iter().map(ToString::to_string).collect())
            .unwrap_or_default(),
    };
    let held = |name: &str| -> bool {
        if made.contains(name) {
            return true;
        }
        let Some(handle) = property_of(sample, name) else {
            // A table, an attribute, or a name the model does not hold.
            return true;
        };
        if !handle.is_computed() {
            return holds_a_value(&handle);
        }
        let Some(stored) = stored else {
            return false;
        };
        let Ok(name) = Identifier::new(name) else {
            return true;
        };
        match stored.property(&name) {
            Ok(held) => holds_a_value(&held),
            Err(_) => stored.has_attribute(&name) || stored.table(&name).is_ok(),
        }
    };
    let column_held = |name: &str| -> bool {
        let Some((table, column)) = name.split_once('.') else {
            return true;
        };
        let column = column.split('[').next().unwrap_or(column);
        let (Ok(table), Ok(column)) = (Identifier::new(table), Identifier::new(column)) else {
            return true;
        };
        for source in [Some(sample), stored].into_iter().flatten() {
            let Ok(held) = source.table(&table) else {
                continue;
            };
            let Ok(view) = held.column(&column) else {
                return true;
            };
            if view.cells().any(|(_, cell)| {
                cell.peek_value()
                    .is_some_and(|value| !matches!(value, Value::Absent))
            }) {
                return true;
            }
        }
        false
    };
    let mut missing = Vec::new();
    for name in inputs {
        if name.starts_with("row.") {
            continue;
        }
        if name.contains('.') {
            if !made.contains(&name) && !column_held(&name) {
                missing.push(name);
            }
            continue;
        }
        if waiting.contains(&name) || !held(&name) {
            missing.push(name);
        }
    }
    if value.contains('.') {
        return missing;
    }
    if let Some(own) = property_of(sample, value)
        && !own.is_computed()
        && !held(value)
    {
        missing.push(value.to_string());
    }
    missing
}
