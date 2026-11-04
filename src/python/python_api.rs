//! The public Python surface: `Property`, `Column`, `RowView`, `ColumnView`,
//! `Table`, `Sample`, `SampleList`, `Summary`, `Field`, `Names`, and the
//! read-only views of what a project declares. Every rule here is the core's;
//! this module decides how Python spells it.

use std::cell::{Cell as Flag, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use indexmap::IndexMap;
use pyo3::IntoPyObjectExt;
use pyo3::basic::CompareOp;
use pyo3::exceptions::{
    PyAttributeError, PyFileExistsError, PyFileNotFoundError, PyIndexError, PyIsADirectoryError,
    PyKeyError, PyNotADirectoryError, PyOSError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyIterator, PyList, PySlice, PyString, PyTuple, PyType};
use serde::de::{Deserialize, IntoDeserializer};

use crate::collection::exports;
use crate::collection::named_queries;
use crate::collection::sample_list::{self as list, Entry, FieldWarning, ListError, SampleList};
use crate::config::discovery;
use crate::config::profiles;
use crate::config::project_config::{
    self as project, ColumnSpec, Export, ExportTarget, FigureDeclaration, Format, NamedQuery,
    Profile, ProjectConfig, PropertyDeclaration, RenderSettings,
};
use crate::core::dependency_graph::Node;
use crate::core::formatting::{self, Precision, Presentation, Resolved};
use crate::core::identifier::{self, Identifier};
use crate::core::property::{ChannelInputs, ComputeError, InputName, Produced, Property};
use crate::core::sample::{
    AttributeValue, NameKind, PropertyHandle, RESERVED, Sample, SampleError,
};
use crate::core::statistics::Summary;
use crate::core::table::{CellRun, ColumnMeta, ColumnSet, Derivation, RowAddress, RowView, Table};
use crate::core::uncertainty::Uncertainty;
use crate::core::value::{Readings, Value};
use crate::format::document::{self, Destination, DocumentError, Origin};
use crate::format::fingerprint;
use crate::format::schema::PrecisionSchema;
use crate::presentation::export_formats::{self, Cell, Dataset};
use crate::presentation::terminal_rendering;
use crate::query::field_addressing::{self as fields, Channel, Statistic};
use crate::query::filter_language as filter;
use crate::query::ordering;

use super::pyo3_bridge::{
    self as bridge, Half, IntoPyErr, OrRaise, PythonColumnCompute, PythonCompute,
    PythonQuantityCompute, PythonRowCompute, SharedSample,
};

// ---------------------------------------------------------------- helpers

/// An axis's unit and text for a field read through a channel: the value is
/// the quantity's symbol, any other channel names itself around it — `u(m)`,
/// `mean(m)` — so that an uncertainty plotted against its value is not
/// labelled as the value; a count has no unit.
fn channel_label(
    field: &str,
    quantity: &str,
    channel: Channel,
    unit: Option<String>,
    symbol: Option<String>,
) -> (Option<String>, Option<String>) {
    if channel == Channel::Value {
        return (unit, symbol);
    }
    let named = field.rsplit('.').next().unwrap_or(field);
    let of = symbol.unwrap_or_else(|| quantity.to_string());
    let unit = match channel {
        Channel::Stat(Statistic::Count) => None,
        _ => unit,
    };
    (unit, Some(format!("{named}({of})")))
}

/// A path from Python: a `str` or any `os.PathLike`.
fn path_from(object: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    let text = object
        .py()
        .import("os")?
        .call_method1("fspath", (object,))?;
    Ok(PathBuf::from(text.extract::<String>()?))
}

/// A path a sample or a list is read from or saved to, made absolute from the
/// folder the script is in now: held relative, a `chdir` later sent `save()`
/// to another folder's file, said *removed or moved*, and its advice wrote
/// the sample outside the project.
fn file_path_from(object: &Bound<'_, PyAny>) -> PyResult<PathBuf> {
    let path = path_from(object)?;
    Ok(std::path::absolute(&path).unwrap_or(path))
}

fn python_path(py: Python<'_>, path: &Path) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("pathlib")?
        .getattr("Path")?
        .call1((path.to_string_lossy().into_owned(),))?
        .unbind())
}

fn is_path_like(object: &Bound<'_, PyAny>) -> PyResult<bool> {
    Ok(object.is_instance_of::<PyString>() || object.hasattr("__fspath__")?)
}

/// What a name could have been, and the nearest of it.
fn unknown_message(kind: &str, written: &str, available: &[String]) -> String {
    unknown_among(kind, written, available, &[])
}

/// The same, the nearest name looked for among `members` too — the methods
/// and attributes an attribute's dot reaches, where `b.sav()` meant `save`.
/// The suggestion comes first, as everywhere in Python, and then what exists.
fn unknown_among(kind: &str, written: &str, available: &[String], members: &[String]) -> String {
    let mut message = format!("unknown {kind} '{written}'");
    let candidates = available.iter().chain(members).map(String::as_str);
    if let Some(nearest) = identifier::nearest(written, candidates) {
        message.push_str(&format!("\n  did you mean: {nearest}?"));
    }
    if available.is_empty() {
        message.push_str("\n  none exists here");
    } else {
        message.push_str(&format!("\n  available: {}", available.join(", ")));
    }
    message
}

/// The public methods and attributes of an object's class, for a nearest
/// name: what `dir()` lists without a leading underscore.
fn members_of(object: &Bound<'_, PyAny>) -> Vec<String> {
    let class = if object.is_instance_of::<PyType>() {
        object.clone()
    } else {
        object.get_type().into_any()
    };
    class
        .dir()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.extract::<String>().ok())
                .filter(|name| !name.starts_with('_'))
                .collect()
        })
        .unwrap_or_default()
}

/// `'Class' object has no attribute 'x'`, with the nearest public member —
/// the suggestion a class of Python's own would not get from its own
/// `__getattr__`.
fn no_attribute(object: &Bound<'_, PyAny>, name: &str) -> PyErr {
    let class = object
        .get_type()
        .name()
        .map(|name| name.to_string())
        .unwrap_or_else(|_| "object".to_string());
    let members = members_of(object);
    let mut message = format!("'{class}' object has no attribute '{name}'");
    if let Some(nearest) = identifier::nearest(name, members.iter().map(String::as_str)) {
        message.push_str(&format!("\n  did you mean: {nearest}?"));
    }
    bridge::said_in_full(PyAttributeError::new_err(message))
}

/// The same exception, with a message that says where it happened.
fn located(py: Python<'_>, error: PyErr, context: &str) -> PyErr {
    let message = format!("{context}: {}", error.value(py));
    PyErr::from_type(error.get_type(py), message)
}

fn busy() -> PyErr {
    PyRuntimeError::new_err("this object is being used by a formula that is still running")
}

/// Sets or deletes an attribute the ordinary way, for private `_x` state.
fn generic_setattr(
    object: &Bound<'_, PyAny>,
    name: &str,
    value: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let py = object.py();
    let name = PyString::new(py, name);
    let value = value.map_or(std::ptr::null_mut(), |value| value.as_ptr());
    // SAFETY: both objects are live for the call, a null value means deletion
    // as the C API documents, and the GIL is held.
    let result =
        unsafe { pyo3::ffi::PyObject_GenericSetAttr(object.as_ptr(), name.as_ptr(), value) };
    if result < 0 {
        Err(PyErr::fetch(py))
    } else {
        Ok(())
    }
}

/// `field:precision=label` spellings, from a `str`, a `Field`, or a sequence of
/// either.
fn written_columns(object: &Bound<'_, PyAny>) -> PyResult<Vec<ColumnSpec>> {
    let py = object.py();
    if let Ok(text) = object.cast::<PyString>() {
        return profiles::parse_columns(text.to_str()?).or_raise(py);
    }
    if let Ok(field) = object.cast::<PyField>() {
        return profiles::parse_column(&field.borrow().path)
            .map(|column| vec![column])
            .or_raise(py);
    }
    let mut columns = Vec::new();
    for item in object.try_iter()? {
        let item = item?;
        if let Ok(field) = item.cast::<PyField>() {
            columns.push(profiles::parse_column(&field.borrow().path).or_raise(py)?);
        } else {
            let text: String = item.extract().map_err(|_| {
                PyTypeError::new_err(format!(
                    "a column is a field path or a Field, and this is {}",
                    bridge::a_type(&item)
                ))
            })?;
            columns.push(profiles::parse_column(&text).or_raise(py)?);
        }
    }
    Ok(columns)
}

fn strings_from(object: &Bound<'_, PyAny>, what: &str) -> PyResult<Vec<String>> {
    if let Ok(text) = object.cast::<PyString>() {
        return Ok(vec![text.to_str()?.to_string()]);
    }
    let mut strings = Vec::new();
    for item in object.try_iter()? {
        let item = item?;
        strings.push(item.extract::<String>().map_err(|_| {
            PyTypeError::new_err(format!(
                "{what} are names, and {} is not one",
                bridge::a_type(&item)
            ))
        })?);
    }
    Ok(strings)
}

/// What a model said a value reads: one list for both of its formulas, or a
/// list per channel.
///
/// A list keeps meaning what it always meant — what the property reads,
/// whatever formula reads it — and that is the form almost every property
/// keeps. The mapping is for the one thing a single list cannot say: *the
/// uncertainty reads this, and the value does not*.
#[derive(Debug, Clone)]
pub enum DeclaredInputs {
    Quantity(Vec<String>),
    Channels {
        value: Option<Vec<String>>,
        uncertainty: Option<Vec<String>>,
    },
}

impl DeclaredInputs {
    /// Everything named, in declaration order and without repetition: what the
    /// graph holds, because an input moving stales the property whichever
    /// formula reads it.
    fn all(&self) -> Vec<String> {
        let mut named: Vec<String> = Vec::new();
        let lists: Vec<&Vec<String>> = match self {
            DeclaredInputs::Quantity(inputs) => vec![inputs],
            DeclaredInputs::Channels { value, uncertainty } => {
                value.iter().chain(uncertainty.iter()).collect()
            }
        };
        for text in lists.into_iter().flatten() {
            if !named.contains(text) {
                named.push(text.clone());
            }
        }
        named
    }

    fn of(&self, channel: Produced) -> Option<&[String]> {
        match (self, channel) {
            (DeclaredInputs::Quantity(inputs), _) => Some(inputs),
            (DeclaredInputs::Channels { value, .. }, Produced::Value) => value.as_deref(),
            (DeclaredInputs::Channels { uncertainty, .. }, Produced::Uncertainty) => {
                uncertainty.as_deref()
            }
        }
    }

    fn by_channel(&self) -> bool {
        matches!(self, DeclaredInputs::Channels { .. })
    }
}

/// `depends_on`, whichever of its two forms was written.
///
/// The mapping is tried first: a dict is iterable, and iterating one yields
/// its keys — so a list of names and a mapping of channels would otherwise be
/// told apart by accident.
///
/// Any mapping, not only a `dict`: a `MappingProxyType` took the list branch,
/// and its keys were then looked up as inputs named `v` and `u`.
fn declared_from(object: &Bound<'_, PyAny>, what: &str) -> PyResult<DeclaredInputs> {
    let Ok(mapping) = object.cast::<pyo3::types::PyMapping>() else {
        return Ok(DeclaredInputs::Quantity(strings_from(object, what)?));
    };
    let (mut value, mut uncertainty) = (None, None);
    for item in mapping.items()?.iter() {
        let (key, named): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
        let key: String = key.extract().map_err(|_| {
            PyTypeError::new_err(format!(
                "{what} is keyed by channel, and {} is not one",
                bridge::a_type(&key)
            ))
        })?;
        match Produced::parse(&key) {
            Some(Produced::Value) => value = Some(strings_from(&named, what)?),
            Some(Produced::Uncertainty) => uncertainty = Some(strings_from(&named, what)?),
            None => {
                return Err(PyValueError::new_err(format!(
                    "'{key}' is not a channel: {what} takes 'v' and 'u', \
                     depends_on={{\"v\": [\"width\"], \"u\": [\"width\", \"area.v\"]}}"
                )));
            }
        }
    }
    if value.is_none() && uncertainty.is_none() {
        return Err(PyValueError::new_err(format!(
            "{what} names no channel: give 'v', 'u', or both — \
             {what}=[] is how a formula says it reads nothing"
        )));
    }
    Ok(DeclaredInputs::Channels { value, uncertainty })
}

fn input_name(text: &str) -> PyResult<InputName> {
    let deserializer: serde::de::value::StrDeserializer<'_, serde::de::value::Error> =
        text.into_deserializer();
    InputName::deserialize(deserializer)
        .map_err(|error| PyValueError::new_err(format!("'{text}' is not an input: {error}")))
}

/// Refuses a name the sample does not have, with the nearest: a misspelt
/// name answered an empty list, which reads as "nothing depends on it".
fn known(py: Python<'_>, sample: &Sample, node: &Node) -> PyResult<()> {
    match node {
        Node::Named(name) => {
            if sample.has_property(name) || sample.has_attribute(name) {
                return Ok(());
            }
            let names: Vec<String> = sample
                .property_names()
                .into_iter()
                .chain(sample.attribute_names())
                .map(|name| name.to_string())
                .collect();
            Err(bridge::key_error(unknown_message(
                "value",
                name.as_str(),
                &names,
            )))
        }
        Node::Column { table, column } => {
            sample
                .table(table)
                .or_raise(py)?
                .column(column)
                .or_raise(py)?;
            Ok(())
        }
    }
}

/// Refuses to write a declared export into a file whose extension names another
/// format, as `samplekit export -o` refuses it: the format is the
/// configuration's there, and the path contradicts it. `to_csv` names its
/// format itself, and takes any file.
fn refuse_another_format(path: &Path, format: &Format, name: &str) -> PyResult<()> {
    let spelled = |format: &Format| export_formats::extension(format);
    let named = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
            "csv" => Some(Format::Csv),
            "tsv" => Some(Format::Tsv),
            "json" => Some(Format::Json),
            other if matches!(format, Format::Text(text) if text.extension == other) => {
                Some(format.clone())
            }
            _ => None,
        });
    match named {
        Some(named) if named != *format => Err(PyValueError::new_err(format!(
            "output={}: '{name}' writes {}, and the extension names {} — name the file .{}, \
             or change format in [export.{name}]; nothing was written",
            path.display(),
            spelled(format).to_ascii_uppercase(),
            spelled(&named).to_ascii_uppercase(),
            spelled(format)
        ))),
        _ => Ok(()),
    }
}

/// A dataset without its uncertainty columns empty for every sample, each said
/// with a warning as the command line says it on stderr. JSON writes a
/// quantity's uncertainty inside it, and says nothing.
fn noted_empty(py: Python<'_>, dataset: &Dataset, format: &Format) -> PyResult<Dataset> {
    let (trimmed, empty) = export_formats::without_empty_uncertainties(dataset);
    // A document writing each property in one cell has no column to leave out.
    let columns = match format {
        Format::Json => false,
        Format::Text(document) => document.property.is_none(),
        _ => true,
    };
    if columns {
        for header in empty {
            warn_value(
                py,
                &format!("{header} is left out: it is empty for every sample"),
            )?;
        }
    }
    Ok(trimmed)
}

fn node_name(node: &Node) -> String {
    node.to_string()
}

/// A precision is the project's to declare, never a model's: the keyword is
/// kept only to say where it went, a property's section and a column's, which
/// the configuration quotes.
fn refuse_precision(precision: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    if precision.is_some_and(|given| !given.is_none()) {
        return Err(PyTypeError::new_err(
            "a precision is declared in the project, not in a model: write \
             [property.<name>] precision = \".3f\" in .samplekitrc, or for a table's column \
             [property.\"<table>.<column>\"] precision = \".3f\" in .samplekitrc",
        ));
    }
    Ok(())
}

/// A symbol is the project's to declare, never a model's, as a precision is.
fn refuse_symbol(symbol: Option<&str>) -> PyResult<()> {
    if let Some(symbol) = symbol {
        return Err(PyTypeError::new_err(format!(
            "a symbol is declared in the project, not in a model: write \
             [property.<name>] symbol = {symbol:?} in .samplekitrc, or for a table's column \
             [property.\"<table>.<column>\"] symbol = {symbol:?} in .samplekitrc"
        )));
    }
    Ok(())
}

fn schema_precision_to_python(py: Python<'_>, precision: &PrecisionSchema) -> PyResult<Py<PyAny>> {
    match precision {
        PrecisionSchema::Both(spec) => spec.into_py_any(py),
        PrecisionSchema::Split(value, uncertainty) => {
            (value.as_str(), uncertainty.as_str()).into_py_any(py)
        }
    }
}

fn index_from(object: &Bound<'_, PyAny>) -> PyResult<Vec<Value>> {
    if object.cast::<PyList>().is_ok() {
        return Err(PyTypeError::new_err(
            "an index of several columns is a tuple, as in (20, 'co2'); a list is not an index",
        ));
    }
    if let Ok(tuple) = object.cast::<PyTuple>() {
        return tuple
            .iter()
            .map(|part| bridge::from_python(&part))
            .collect();
    }
    Ok(vec![bridge::from_python(object)?])
}

fn index_to_python(py: Python<'_>, index: &[Value]) -> PyResult<Py<PyAny>> {
    if index.len() == 1 {
        return bridge::to_python(py, &index[0]);
    }
    let parts: Vec<Py<PyAny>> = index
        .iter()
        .map(|part| bridge::to_python(py, part))
        .collect::<PyResult<_>>()?;
    Ok(PyTuple::new(py, parts)?.into_any().unbind())
}

/// An index as `Table.at` writes one: `20`, `'Lea'`, `2026-06-01` — never the
/// Rust spelling `Integer(20)` a message once showed.
fn describe_index(index: &[Value]) -> String {
    let parts: Vec<String> = index
        .iter()
        .map(|part| match part {
            Value::Integer(integer) => integer.to_string(),
            Value::Number(number) => number.to_string(),
            Value::Text(text) => format!("'{text}'"),
            Value::Boolean(flag) => flag.to_string(),
            Value::Date(date) => date.iso(),
            Value::DateTime(date_time) => date_time.iso(),
            Value::Absent => "(absent)".to_string(),
            Value::NotApplicable => "n/a".to_string(),
        })
        .collect();
    parts.join(", ")
}

/// A quantity copied out of a cell or a property: its value or readings, its
/// uncertainty, its presentation and its records, and no formula.
fn copy_of(property: &Property) -> Result<Property, ComputeError> {
    let mut copy = match property.readings() {
        Some(readings) => {
            // Readings keep what stands beside them: a value written, and the
            // statistics declared for them. Copied as a number, a statistic
            // would stand as a value typed, and a spread derived from them
            // would stop following them.
            let mut measured = Property::measured(readings.clone(), None);
            measured.set_written_value(property.written_value().cloned());
            measured
                .declare_statistics(property.declared_location(), property.declared_convention());
            if property.statistic_taken() {
                measured.take_statistic();
            }
            measured
        }
        None => Property::stored(property.value()?),
    };
    if property.readings().is_none() || property.convention().is_none() {
        copy.set_uncertainty(property.uncertainty()?);
    }
    copy.set_presentation(property.presentation().clone());
    copy.set_records(property.records().clone());
    Ok(copy)
}

fn readings_from(object: &Bound<'_, PyAny>) -> PyResult<Readings> {
    let py = object.py();
    // One number is no readings: said with both ways, where Python's own
    // *'float' object is not iterable* was the answer.
    if object.is_instance_of::<pyo3::types::PyFloat>()
        || object.is_instance_of::<pyo3::types::PyInt>()
    {
        return Err(PyTypeError::new_err(format!(
            "one number is not readings: readings = [{0}] for readings of one, or the \
             property's value = {0} for a value",
            object.repr()?
        )));
    }
    let mut numbers = Vec::new();
    for item in object.try_iter()? {
        let item = item?;
        match bridge::from_python(&item)? {
            Value::Number(number) => numbers.push(number),
            Value::Integer(integer) => numbers.push(integer as f64),
            other => {
                return Err(PyTypeError::new_err(format!(
                    "readings are numbers, and one of these is {}",
                    other.kind()
                )));
            }
        }
    }
    Readings::new(numbers).or_raise(py)
}

/// A table cell given in Python: a scalar, readings, or a `Property`.
fn cell_from(object: &Bound<'_, PyAny>) -> PyResult<Property> {
    let py = object.py();
    if let Ok(property) = object.cast::<PyProperty>() {
        return property.borrow().take(py).map(|(taken, _)| taken);
    }
    if object.is_instance_of::<PyList>() || object.is_instance_of::<PyTuple>() {
        return Ok(Property::measured(readings_from(object)?, None));
    }
    Ok(Property::stored(bridge::from_python(object)?))
}

fn cells_from(mapping: &Bound<'_, PyAny>) -> PyResult<Vec<(Identifier, Property)>> {
    let py = mapping.py();
    let mut cells = Vec::new();
    for item in mapping.call_method0("items")?.try_iter()? {
        let pair = item?;
        let column: String = pair.get_item(0)?.extract()?;
        cells.push((
            bridge::identifier_from(py, &column)?,
            cell_from(&pair.get_item(1)?)?,
        ));
    }
    Ok(cells)
}

fn cell_to_python(py: Python<'_>, cell: &Cell) -> PyResult<Py<PyAny>> {
    match cell {
        // `None`, as JSON writes it: pandas reads it as NaN, where `NA` made
        // the column one of objects, which `pd.to_numeric` refused.
        Cell::Scalar(Some(Value::NotApplicable))
        | Cell::Written {
            value: Value::NotApplicable,
            ..
        } => Ok(py.None()),
        Cell::Scalar(Some(value)) | Cell::Written { value, .. } => bridge::to_python(py, value),
        Cell::Scalar(None) => Ok(py.None()),
        Cell::Tags(tags) => Ok(PyList::new(py, tags)?.into_any().unbind()),
        Cell::List(values) => {
            let items: Vec<Py<PyAny>> = values
                .iter()
                .map(|value| bridge::to_python(py, value))
                .collect::<PyResult<_>>()?;
            Ok(PyList::new(py, items)?.into_any().unbind())
        }
    }
}

// --------------------------------------------------------------- the file

/// What a formula is, for its digest: a method's function rather than the
/// method bound to one sample, which would hold the sample from its own file
/// record; anything else as it was given.
fn function_of(callable: &Bound<'_, PyAny>) -> Py<PyAny> {
    callable
        .getattr("__func__")
        .unwrap_or_else(|_| callable.clone())
        .unbind()
}

/// What a Python sample knows that the core's sample does not: its file, what
/// it contained when read, its project, and whether a formula was attached.
#[derive(Default)]
pub struct SampleFile {
    path: Option<PathBuf>,
    origin: Option<Origin>,
    /// The file's text as it was read or last written: what a save writes
    /// the script's changes over.
    read: Option<String>,
    config: Option<Rc<ProjectConfig>>,
    pending: Option<PathBuf>,
    /// What a `Property(depends_on=…)` declared, waiting to be applied.
    ///
    /// A model writes `self.porosity = sk.Property(compute=…, depends_on=["diameter"])`
    /// before `diameter` need exist, so the names cannot be checked where they are
    /// written. They are kept here and applied once the model is built.
    declared: Vec<(Identifier, DeclaredInputs)>,
    /// Every value whose inputs were declared, by either route — the keyword
    /// above or `set_dependencies` — including those declared to read nothing.
    /// What separates *this formula reads nothing* from *nobody said*.
    declared_names: Vec<Identifier>,
    /// Whether the sample has been made — its model built and, when it has a
    /// file, that file read — so that a declaration made afterwards applies as
    /// it is made rather than waiting for a moment that will not come.
    settled: bool,
    /// By the value each formula gives — a property, or `table.column` — what
    /// its declaration was given: the functions, the statistics, the unit, what
    /// a table's derivation reads and fills. The worker's, to take each
    /// formula's digest from.
    formula_code: IndexMap<String, FormulaCode>,
    /// The values `compute()` ran since the sample was last saved, whose
    /// records the save keeps under the formula that ran: one giving the very
    /// number it held was left *formula changed* for ever.
    computed: Vec<String>,
}

/// A formula's declaration by role, as [`SampleFile::formula_code`] keeps it.
type FormulaCode = Vec<(String, Py<PyAny>)>;

pub type FileRef = Rc<RefCell<SampleFile>>;

/// Applies what `Property(depends_on=…)` declared, then refuses a formula that
/// says nothing about what it reads.
///
/// **The first review's finding**: a formula reading an input it never
/// declared records `computed: {}` and is never stale again, whatever happens
/// to that input. It computes, it writes, and every surface then calls it
/// current — a confident wrong number, from a model a newcomer writes on their
/// first day. Declaring is therefore not optional, and `depends_on=[]` is how a
/// formula says it reads nothing of the sample.
fn settle_declarations(py: Python<'_>, shared: &SharedSample, file: &FileRef) -> PyResult<()> {
    apply_declarations(py, shared, file)?;
    refuse_undeclared(py, shared, file)
}

/// Applies what `Property(depends_on=…)` declared and has not been applied
/// yet. Once the sample is made, a declaration is applied as it is made.
fn apply_declarations(py: Python<'_>, shared: &SharedSample, file: &FileRef) -> PyResult<()> {
    let (pending, from_file) = {
        let mut held = file.borrow_mut();
        (std::mem::take(&mut held.declared), held.path.is_some())
    };
    for (output, declared) in pending {
        shared.write(|sample| {
            // An attribute its formulas read and the sample does not hold —
            // the model described with no file, a new sample, a file that
            // has not got it yet — held empty: an input like any, which
            // what reads it waits for, and taken into the formula's digest
            // alike. The model was said unreadable, and the file not read.
            {
                for text in declared.all() {
                    if let Ok(name) = Identifier::new(&text)
                        && name != output
                        && !sample.has_property(&name)
                        && !sample.has_attribute(&name)
                        && sample.table(&name).is_err()
                        && !near_a_name_held(sample, &name)
                    {
                        let _ = sample.set_attribute(name, AttributeValue::Scalar(Value::Absent));
                    }
                }
            }
            let formulas = Formulas::in_sample(sample, &output);
            let (nodes, channels) =
                resolve_declaration(py, sample, &output, &declared, formulas, !from_file)?;
            sample
                .declare_dependencies_by_channel(&output, &nodes, channels)
                .or_raise(py)
        })?;
    }
    Ok(())
}

/// Which formulas the property being declared has, so that a declaration can
/// be judged before anything is installed: read from the sample where the
/// property is already there, from the property about to be assigned where it
/// is not.
#[derive(Debug, Clone, Copy, Default)]
struct Formulas {
    value: bool,
    uncertainty: bool,
    joint: bool,
}

impl Formulas {
    fn of(property: &Property) -> Formulas {
        Formulas {
            value: property.is_computed(),
            uncertainty: property.has_uncertainty_formula(),
            joint: property.is_joint(),
        }
    }

    fn in_sample(sample: &Sample, output: &Identifier) -> Formulas {
        sample
            .property(output)
            .map(|handle| handle.peek(Formulas::of))
            .unwrap_or_default()
    }
}

/// What a declaration names, resolved against the sample: the nodes the graph
/// holds, and the split per channel the record keeps.
///
/// One function for both routes — the constructor's keyword and
/// `set_dependencies` — which had the same resolution written twice.
fn resolve_declaration(
    py: Python<'_>,
    sample: &Sample,
    output: &Identifier,
    declared: &DeclaredInputs,
    formulas: Formulas,
    unfiled: bool,
) -> PyResult<(Vec<Node>, ChannelInputs)> {
    let mut nodes = Vec::new();
    let mut channels = ChannelInputs::default();
    if declared.by_channel() {
        refuse_half_declared(output, declared, formulas)?;
    }
    for text in declared.all() {
        let (node, input) = resolve_input(py, sample, output, &text, declared, formulas, unfiled)?;
        if let Some(node) = node
            && !nodes.contains(&node)
        {
            nodes.push(node);
        }
        // Which formula reads it, where the model said so channel by channel.
        // A property's own value is the uncertainty's whatever form was used:
        // the value's formula cannot read what it is producing.
        for channel in [Produced::Value, Produced::Uncertainty] {
            let named = declared
                .of(channel)
                .is_some_and(|inputs| inputs.contains(&text));
            let own_value = input == own_channel(output, Produced::Value);
            if named && !(own_value && channel == Produced::Value) {
                match channel {
                    Produced::Value => channels.value.push(input.clone()),
                    Produced::Uncertainty => channels.uncertainty.push(input.clone()),
                }
            }
        }
    }
    if channels.value == channels.uncertainty {
        // Both formulas read the same things, however that was written: there
        // is nothing for a record to key, and it states them once.
        channels = ChannelInputs::default();
    }
    Ok((nodes, channels))
}

/// A declaration written per channel says what **each** formula reads, or it
/// is a formula running undeclared under another name.
fn refuse_half_declared(
    output: &Identifier,
    declared: &DeclaredInputs,
    formulas: Formulas,
) -> PyResult<()> {
    // One call giving both numbers has one formula, and a list per channel
    // cannot describe it.
    if formulas.joint {
        return Err(PyValueError::new_err(format!(
            "'{output}' computes its value and its uncertainty in one formula, so there is \
             nothing to say per channel: depends_on takes one list"
        )));
    }
    for (channel, computes) in [
        (Produced::Value, formulas.value),
        (Produced::Uncertainty, formulas.uncertainty),
    ] {
        let word = match channel {
            Produced::Value => "value",
            Produced::Uncertainty => "uncertainty",
        };
        if computes && declared.of(channel).is_none() {
            return Err(PyValueError::new_err(format!(
                "the {word} of '{output}' has a formula, and depends_on does not say what it \
                 reads: give '{}', with [] if it reads nothing of the sample",
                channel.key()
            )));
        }
        // A key for a channel no formula produces says what nothing reads.
        // Accepted, its names were folded into the other channel's record,
        // and the file then claimed a formula read what it never did.
        if !computes && declared.of(channel).is_some() {
            return Err(PyValueError::new_err(format!(
                "the {word} of '{output}' has no formula, so '{}' names what nothing reads: \
                 leave it out, depends_on={{\"{}\": [...]}}",
                channel.key(),
                match channel {
                    Produced::Value => Produced::Uncertainty.key(),
                    Produced::Uncertainty => Produced::Value.key(),
                }
            )));
        }
    }
    Ok(())
}

/// Whether `name` is a slip from a name the sample holds — `srcc` for
/// `src`: a typo in `depends_on` is refused with the name it is near, where
/// a name near none — `material`, which its files give — is an attribute
/// held empty until they do.
fn near_a_name_held(sample: &Sample, name: &Identifier) -> bool {
    let held: Vec<String> = sample
        .property_names()
        .into_iter()
        .chain(sample.attribute_names())
        .chain(sample.table_names())
        .map(|held| held.to_string())
        .collect();
    crate::core::identifier::nearest(name.as_str(), held.iter().map(String::as_str)).is_some()
}

/// The address of one channel of one property: `p.v`.
fn own_channel(property: &Identifier, channel: Produced) -> InputName {
    InputName::Column {
        table: property.clone(),
        column: Identifier::new(channel.key()).expect("a channel's key is an identifier"),
    }
}

/// One name a declaration gives: the node the graph holds for it, where it is
/// an edge at all, and the name a record writes.
fn resolve_input(
    py: Python<'_>,
    sample: &Sample,
    output: &Identifier,
    text: &str,
    declared: &DeclaredInputs,
    formulas: Formulas,
    unfiled: bool,
) -> PyResult<(Option<Node>, InputName)> {
    match input_name(text)? {
        // Refused here rather than by the graph, so that it is refused before
        // anything is installed, and in words that say what to write.
        InputName::Named(name) if name == *output => Err(PyValueError::new_err(format!(
            "'{output}' declares that it reads itself, and a value cannot rest on its own \
             result\n  an uncertainty may read the value beside it — \
             depends_on={{\"u\": [\"{output}.v\"]}}"
        ))),
        InputName::Named(name) => {
            // A sample made without a file — the model described, a new
            // sample — has only what its model declared: an attribute its
            // files supply, `material`, is read from them, and is no error
            // here. The model was said unreadable, and `new` wrote what it
            // declares as attributes.
            if unfiled
                && !sample.has_property(&name)
                && !sample.has_attribute(&name)
                && !near_a_name_held(sample, &name)
            {
                return Ok((None, InputName::Named(name)));
            }
            if !sample.has_property(&name) && !sample.has_attribute(&name) {
                let available: Vec<String> = sample
                    .property_names()
                    .into_iter()
                    .chain(sample.attribute_names())
                    .map(|name| name.to_string())
                    .collect();
                let mut message = unknown_message("input", name.as_str(), &available);
                message.push_str(&format!(
                    "\n  '{output}' declares that it reads '{name}', which this sample's file \
                     does not hold: write it there, samplekit FILE set {name}=…, or declare it \
                     in __init__ — self.{name} = \"\" for a text, self.{name} = sk.Property() \
                     for a measured value"
                ));
                return Err(bridge::key_error(message));
            }
            Ok((Some(Node::Named(name.clone())), InputName::Named(name)))
        }
        InputName::Column { table, column } => {
            // A table's column first: a table holding a column called `v` is
            // ordinary, and a table and a property cannot share a name.
            if sample.table(&table).is_ok() {
                sample
                    .table(&table)
                    .or_raise(py)?
                    .column(&column)
                    .or_raise(py)?;
                return Ok((
                    Some(Node::Column {
                        table: table.clone(),
                        column: column.clone(),
                    }),
                    InputName::Column { table, column },
                ));
            }
            // A property's own value, read by its own uncertainty: the ordinary
            // instrument specification, ±0.5 % of the reading. It is no edge of
            // any graph — nothing outside the property is involved — and the
            // record names it under `u`. The property itself need not be in the
            // sample yet: a declaration is judged before the property it
            // declares is installed.
            if table == *output
                && let Some(channel) = Produced::parse(column.as_str())
            {
                own_channel_read(output, channel, declared, formulas)?;
                return Ok((None, own_channel(output, channel)));
            }
            match (
                sample.has_property(&table),
                Produced::parse(column.as_str()),
            ) {
                (true, Some(_)) => Err(PyValueError::new_err(format!(
                    "'{text}' names one channel of another value, and what a record follows \
                     is the whole property: declare '{table}'\n  \
                     a channel address is for this value's own, \
                     depends_on={{\"u\": [\"{output}.v\"]}}"
                ))),
                (true, None) => Err(PyValueError::new_err(format!(
                    "'{text}' names '{column}' of the value '{table}', and a value has no \
                     columns: its channels are 'v' and 'u'"
                ))),
                (false, _) => {
                    // Neither a table nor a value: say both, and what there is.
                    let available: Vec<String> = sample
                        .property_names()
                        .into_iter()
                        .chain(sample.table_names())
                        .map(|name| name.to_string())
                        .collect();
                    Err(bridge::key_error(format!(
                        "'{text}' names '{table}', which is neither a value nor a table of \
                         this sample{}",
                        if available.is_empty() {
                            String::new()
                        } else {
                            format!("\n  available: {}", available.join(", "))
                        }
                    )))
                }
            }
        }
        InputName::Cell(_) | InputName::CellValue(_) => Err(PyValueError::new_err(format!(
            "'{text}' names a cell of the row a table formula runs on, and a \
             property has no row"
        ))),
    }
}

/// What a property may read of itself: its value, from its uncertainty's
/// formula, and nothing else.
///
/// `v ← u` would make a value rest on the uncertainty of the value it does not
/// have yet, and a channel reading itself is a cycle of one. Both are refused
/// here rather than by the graph, which knows nothing of channels, and the
/// message says which of the two it means — *declared as its own input* was
/// true of neither.
fn own_channel_read(
    output: &Identifier,
    channel: Produced,
    declared: &DeclaredInputs,
    formulas: Formulas,
) -> PyResult<()> {
    let address = format!("{output}.{}", channel.key());
    let rests_on_uncertainty = || {
        PyValueError::new_err(format!(
            "'{address}' is this value's own uncertainty, and a value cannot be computed \
             from it\n  \
             the reverse is what an instrument rated at a percentage of its reading \
             declares — depends_on={{\"u\": [\"{output}.v\"]}}"
        ))
    };
    if declared.by_channel() {
        // The same channel on both sides: a cycle of one, and a different
        // mistake from the one below.
        if declared
            .of(channel)
            .is_some_and(|inputs| inputs.contains(&address))
        {
            return Err(PyValueError::new_err(format!(
                "'{address}' is the {} this formula is computing, and a channel cannot \
                 read itself",
                match channel {
                    Produced::Value => "value",
                    Produced::Uncertainty => "uncertainty",
                }
            )));
        }
        if channel == Produced::Uncertainty {
            return Err(rests_on_uncertainty());
        }
        return Ok(());
    }
    // Said once for both formulas. `u ← v` is what it can mean, so what the
    // property computes decides which sentence is true.
    if channel == Produced::Uncertainty {
        return Err(rests_on_uncertainty());
    }
    if formulas.joint {
        return Err(PyValueError::new_err(format!(
            "'{output}' computes both its numbers in one formula, which cannot read what \
             it produces: '{address}' is not an input it can have"
        )));
    }
    match (formulas.value, formulas.uncertainty) {
        // The instrument specification: the value entered, the uncertainty a
        // formula of it.
        (false, true) => Ok(()),
        (true, true) => Err(PyValueError::new_err(format!(
            "'{output}' computes both its numbers and declares that it reads '{address}': \
             say which formula reads what\n  \
             depends_on={{\"v\": [...], \"u\": [..., \"{address}\"]}}"
        ))),
        (true, false) => Err(PyValueError::new_err(format!(
            "'{address}' is the value this formula is computing, and a formula cannot read \
             what it produces"
        ))),
        (false, false) => Err(PyValueError::new_err(format!(
            "'{output}' has no formula, so nothing reads '{address}'"
        ))),
    }
}

/// Refuses a formula that says nothing about what it reads: at construction by
/// either route, and again at `compute` for a formula assigned afterwards, so
/// that no formula ever runs undeclared.
fn refuse_undeclared(_py: Python<'_>, shared: &SharedSample, file: &FileRef) -> PyResult<()> {
    let declared_names = file.borrow().declared_names.clone();
    let undeclared: Vec<String> = shared.read(
        |sample| {
            let mut found = Vec::new();
            for name in sample
                .property_names()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                if declared_names.contains(&name) {
                    continue;
                }
                let computes = sample.property(&name).is_ok_and(|handle| {
                    handle.peek(|property| {
                        property.is_computed() || property.has_uncertainty_formula()
                    })
                });
                if computes {
                    found.push(name.to_string());
                }
            }
            Ok(found)
        },
        |_| Ok(Vec::new()),
    )?;
    if !undeclared.is_empty() {
        return Err(PyValueError::new_err(format!(
            "{}: {}\n  \
             declare it where the value is made, sk.Property(compute=…, depends_on=[\"width\"]), \
             or with self.set_dependencies(\"…\", depends_on=[…])\n  \
             depends_on=[] says the formula reads nothing of the sample\n  \
             without it, a change to an input would never mark the value outdated",
            if undeclared.len() == 1 {
                "this value is computed and does not say what it reads"
            } else {
                "these values are computed and do not say what they read"
            },
            undeclared.join(", ")
        )));
    }
    Ok(())
}

/// The project above a sample's file, read once for the file.
fn config_for(py: Python<'_>, file: &Path) -> PyResult<Option<Rc<ProjectConfig>>> {
    let directory = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !directory.is_dir() {
        return Ok(None);
    }
    project::load_for(directory)
        .map(|config| config.map(Rc::new))
        .or_raise(py)
}

fn missing_file(path: &Path) -> PyErr {
    // A folder is samples, which `load` reads: "no sample file" left the
    // reader to guess what to write instead.
    if path.is_dir() {
        return PyIsADirectoryError::new_err(format!(
            "{} is a folder, and a Sample is one file: sk.load(\"{}\") reads the folder's \
             samples as a SampleList",
            path.display(),
            path.display()
        ));
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut message = format!("no sample file at {}", path.display());
    if !directory.is_dir() {
        message.push_str(&format!(
            "\n  the directory {} does not exist",
            directory.display()
        ));
        return PyFileNotFoundError::new_err(message);
    }
    let names: Vec<String> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            // A folder too: `sk.load("piece")` beside `pieces/` suggested nothing.
            entry.path().is_dir() || entry.file_name().to_string_lossy().ends_with(".md")
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let written = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(nearest) = identifier::nearest(&written, names.iter().map(String::as_str)) {
        message.push_str(&format!(
            "\n  nearest: {}",
            directory.join(nearest).display()
        ));
    }
    PyFileNotFoundError::new_err(message)
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (dunce::canonicalize(left), dunce::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => std::path::absolute(left).ok() == std::path::absolute(right).ok(),
    }
}

/// The class a list or a sample is loaded with: a subclass of `Sample`, none
/// for `False`, or the project's own for nothing said.
fn model_class(
    py: Python<'_>,
    model: Option<&Bound<'_, PyAny>>,
    config: Option<&ProjectConfig>,
) -> PyResult<Option<Py<PyAny>>> {
    let base = bridge::sample_class(py)?;
    match model.filter(|model| !model.is_none()) {
        Some(model) if model.is_instance_of::<PyBool>() => {
            if model.is_truthy()? {
                Err(PyTypeError::new_err(
                    "model=True names no class: pass a subclass of Sample, False, or nothing",
                ))
            } else {
                Ok(None)
            }
        }
        Some(model) => {
            let is_sample = model
                .cast::<PyType>()
                .map(|class| class.is_subclass(&base))
                .unwrap_or(Ok(false))?;
            if !is_sample {
                return Err(PyTypeError::new_err(format!(
                    "model is a subclass of samplekit.Sample, False, or nothing, and this is {}",
                    bridge::repr(model)
                )));
            }
            if model.is(&base) {
                return Ok(None);
            }
            Ok(Some(model.clone().unbind()))
        }
        None => match config {
            Some(config) => bridge::load_model(py, config),
            None => Ok(None),
        },
    }
}

// --------------------------------------------------------------- Property

/// Where a `Property` object's quantity lives.
pub enum PropertyState {
    /// Built and not yet assigned: the property itself.
    Detached(Box<Property>),
    /// A sample's property, shared with every other wrapper of it.
    Attached {
        handle: PropertyHandle,
        file: FileRef,
        sample: SharedSample,
    },
    /// A table's cell, reached through its table by index and column.
    Cell {
        table: Py<PyTable>,
        index: Vec<Value>,
        column: Identifier,
    },
}

enum Change {
    Value(Value),
    Readings(Readings),
    Uncertainty(Option<Uncertainty>),
    Presentation(Presentation),
    Invalidate,
}

fn change_property(property: &mut Property, change: Change) {
    match change {
        Change::Value(value) => property.set_value(value),
        // A declared convention stays with new readings.
        Change::Readings(readings) => {
            let convention = property.convention();
            property.set_readings(readings, convention)
        }
        Change::Uncertainty(uncertainty) => property.set_uncertainty(uncertainty),
        Change::Presentation(presentation) => property.set_presentation(presentation),
        Change::Invalidate => property.invalidate(),
    }
}

/// A formula cannot change the property it is producing: its value, its
/// uncertainty, its presentation, or the property itself. Asked before the
/// write, because the write needs the property the running formula holds, and
/// taking it panicked — a `PanicException` no `except Exception` catches, which
/// in a model run killed the worker and every value after it.
fn refuse_while_computing(handle: &PropertyHandle) -> PyResult<()> {
    if handle.is_computing() {
        return Err(PyRuntimeError::new_err(format!(
            "'{}' is being computed, and a formula cannot change the value it is \
             producing: return it instead",
            handle.name()
        )));
    }
    Ok(())
}

fn change_handle(handle: &PropertyHandle, change: Change) {
    match change {
        // The rule `set` follows too: a formula's value read without its model
        // keeps its record, marked edited, rather than turning into an entered
        // value that reads current.
        Change::Value(value) => crate::collection::editing::set_by_hand(handle, value),
        Change::Readings(readings) => {
            handle.set_readings(readings, handle.peek(Property::convention))
        }
        // Typed over one a formula or a statistic gives, an override marked
        // as `set` marks it: a save wrote it unmarked, and it read current.
        Change::Uncertainty(Some(uncertainty)) => {
            crate::collection::editing::set_uncertainty_by_hand(handle, uncertainty);
        }
        Change::Uncertainty(None) => handle.set_uncertainty(None),
        Change::Presentation(presentation) => handle.set_presentation(presentation),
        Change::Invalidate => handle.invalidate(),
    }
}

/// A property: a value, its uncertainty and its unit.
///
/// A property stands for its value in arithmetic and comparisons, so
/// ``steel.area * 2`` and ``steel.area > 25`` work on the number, and it prints
/// as the whole property, ``25.056 ± 0.053 mm²``. A model declares its properties in its
/// ``__init__``; a property a model does not declare is made the same way and
/// assigned to the sample.
///
/// A property is one of four kinds, by what is given: a value entered by hand;
/// readings, whose value and uncertainty a statistic gives; a value a formula
/// computes (``compute``, or ``compute_value`` and ``compute_uncertainty``);
/// or nothing yet.
///
/// A ``Table`` declares its columns with it too: there a property describes
/// every cell of its column, with the same keywords, and its formula is given
/// the row, ``f(row)``. A column takes no value nor uncertainty: each cell
/// holds its own, in the file.
///
/// Args:
///     value: The value — a number, text, a boolean, a date, ``samplekit.NA``
///         — or a list of readings, or a statistic from ``samplekit.stats``
///         that gives the value from the readings.
///     uncertainty: The absolute standard uncertainty, in the value's unit, or
///         a spread from ``samplekit.stats``.
///     unit: The unit, as the file writes it: ``"g"``, ``"degC"``.
///     symbol: Not taken here: a symbol is declared in ``.samplekitrc``,
///         under ``[property.NAME]``.
///     precision: Not taken here: a precision is declared in ``.samplekitrc``,
///         under ``[property.NAME]``.
///     compute: A function of no argument returning the property: a number,
///         or ``(value, uncertainty)``.
///     compute_value: A function of no argument returning the value alone,
///         beside an uncertainty entered, given by readings or computed apart.
///     compute_uncertainty: A function of no argument returning the
///         uncertainty, beside a value entered or computed apart.
///     compute_column: For a table's column only: a function given the
///         columns it reads, by name, and returning a list, one cell per row,
///         each a number or ``(value, uncertainty)``.
///     depends_on: What the formula reads: a list of names — ``["width", "thickness"]``,
///         ``"curve.force"`` for a column, ``"row.force"`` for a
///         cell of the same row — or, where the value and the uncertainty have
///         a formula each, ``{"v": [...], "u": [...]}``. ``[]`` says it reads
///         nothing of the sample. It can also be declared later with
///         ``Sample.set_dependencies``.
///
/// Raises:
///     ValueError: Two arguments give the same thing — ``value`` and a
///         formula, ``compute`` and ``compute_value``, ``compute_column`` and
///         another — or a value is not a number where a unit is given.
///     TypeError: A formula is not callable, or a symbol or a precision is
///         given.
///
/// Example:
///     >>> import samplekit as sk
///     >>> sk.Property(3.2, 0.1, unit="kg")
///     Property(value=3.2, uncertainty=0.1, unit='kg')
///     >>> width = sk.Property(unit="mm", value=sk.stats.mean,
///     ...                     uncertainty=sk.stats.standard_error)
#[pyclass(name = "Property", module = "samplekit", unsendable)]
pub struct PyProperty {
    state: RefCell<PropertyState>,
    /// `depends_on`, carried from the constructor until the sample takes this
    /// property and can record what it reads.
    declared: RefCell<Option<DeclaredInputs>>,
    /// What the constructor was given that says how the value is computed —
    /// each formula's function, the statistics declared, the unit — by role,
    /// carried to the sample that takes it, for the worker's digest of the
    /// formula. Empty for a value no formula gives.
    code: RefCell<Vec<(String, Py<PyAny>)>>,
    /// What it was given, kept as given, for a table that takes it as one of
    /// its columns: there it describes every cell, and its formulas are
    /// given the row.
    as_column: RefCell<Option<ColumnGiven>>,
}

/// A `Property` as a table's column reads it: its unit, the statistics of each
/// cell's readings, each formula as it was given and what it reads, and
/// whether a value of its own was given, which a column refuses.
pub struct ColumnGiven {
    unit: Option<String>,
    statistics: crate::core::property::DeclaredStatistics,
    compute: Option<Py<PyAny>>,
    compute_value: Option<Py<PyAny>>,
    compute_uncertainty: Option<Py<PyAny>>,
    compute_column: Option<Py<PyAny>>,
    depends_on: Option<Py<PyAny>>,
    /// `value=5`, `uncertainty=0.1` or readings: a cell's own, never a column's.
    entered: Option<&'static str>,
}

impl PyProperty {
    pub fn detached(property: Property) -> PyProperty {
        PyProperty {
            state: RefCell::new(PropertyState::Detached(Box::new(property))),
            declared: RefCell::new(None),
            code: RefCell::new(Vec::new()),
            as_column: RefCell::new(None),
        }
    }

    pub fn attached(handle: PropertyHandle, file: FileRef, sample: SharedSample) -> PyProperty {
        PyProperty {
            state: RefCell::new(PropertyState::Attached {
                handle,
                file,
                sample,
            }),
            declared: RefCell::new(None),
            code: RefCell::new(Vec::new()),
            as_column: RefCell::new(None),
        }
    }

    fn cell(table: Py<PyTable>, index: Vec<Value>, column: Identifier) -> PyProperty {
        PyProperty {
            state: RefCell::new(PropertyState::Cell {
                table,
                index,
                column,
            }),
            declared: RefCell::new(None),
            code: RefCell::new(Vec::new()),
            as_column: RefCell::new(None),
        }
    }

    fn value_of(&self, py: Python<'_>) -> PyResult<Value> {
        let before = bridge::formula_runs();
        let (outcome, sample) = match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => (
                served(
                    py,
                    "this value",
                    property.value(),
                    (Value::absent(), last_value(property)),
                    never_valued(property),
                    &[],
                ),
                None,
            ),
            PropertyState::Attached {
                handle,
                sample,
                file,
            } => {
                let what = format!("'{}'", handle.name());
                let never = handle.peek(never_valued);
                let named = format!("{}{what}", whose(sample, file));
                let waits = waiting(sample, handle.name(), never)?;
                let last = handle.peek(last_value);
                let outcome = served(
                    py,
                    &named,
                    handle.value(),
                    (Value::absent(), last),
                    never,
                    &waits,
                );
                if !never {
                    warn_if_stale(py, sample, file, handle.name(), &named)?;
                }
                (outcome, Some(sample.clone()))
            }
            PropertyState::Cell {
                table,
                index,
                column,
            } => (
                table.borrow(py).with_cell(py, true, index, column, |cell| {
                    served(
                        py,
                        &format!("'{column}'"),
                        cell.value(),
                        (Value::absent(), last_value(cell)),
                        never_valued(cell),
                        &[],
                    )
                }),
                None,
            ),
        };
        if let Some(sample) = sample {
            bridge::record_computed(&sample, before);
        }
        outcome
    }

    fn uncertainty_of(&self, py: Python<'_>) -> PyResult<Option<Uncertainty>> {
        let before = bridge::formula_runs();
        let (outcome, sample) = match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => (
                served(
                    py,
                    "this uncertainty",
                    property.uncertainty(),
                    (None, last_uncertainty(property)),
                    never_uncertain(property),
                    &[],
                ),
                None,
            ),
            PropertyState::Attached {
                handle,
                sample,
                file,
            } => {
                let never = handle.peek(never_uncertain);
                // An uncertainty waiting for the value beside it waits for
                // that, not for a property of the same name.
                let waits: Vec<String> = waiting(sample, handle.name(), never)?
                    .into_iter()
                    .map(|input| {
                        if input == handle.name().as_str() {
                            "its value".to_string()
                        } else {
                            input
                        }
                    })
                    .collect();
                (
                    served(
                        py,
                        &format!(
                            "{}the uncertainty of '{}'",
                            whose(sample, file),
                            handle.name()
                        ),
                        handle.uncertainty(),
                        (None, handle.peek(last_uncertainty)),
                        never,
                        &waits,
                    ),
                    Some(sample.clone()),
                )
            }
            PropertyState::Cell {
                table,
                index,
                column,
            } => (
                table.borrow(py).with_cell(py, true, index, column, |cell| {
                    served(
                        py,
                        &format!("the uncertainty of '{column}'"),
                        cell.uncertainty(),
                        (None, last_uncertainty(cell)),
                        never_uncertain(cell),
                        &[],
                    )
                }),
                None,
            ),
        };
        if let Some(sample) = sample {
            bridge::record_computed(&sample, before);
        }
        outcome
    }

    fn presentation_of(&self, py: Python<'_>) -> PyResult<Presentation> {
        match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => Ok(property.presentation().clone()),
            PropertyState::Attached { handle, .. } => Ok(handle.presentation()),
            PropertyState::Cell {
                table,
                index,
                column,
            } => table.borrow(py).read(py, false, |held| {
                held.presentation_of(column, &RowAddress::Index(index.clone()))
                    .or_raise(py)
            }),
        }
    }

    /// Which assignment of the readings a list read now belongs to: a list
    /// read before they were assigned again is bound to nothing, as a tag
    /// list is — it replaced the readings typed since, in silence.
    fn readings_epoch(&self) -> u64 {
        match self.readings_key() {
            Some((sample, key)) => sample.epoch(&key),
            None => 0,
        }
    }

    /// The sample and the key its readings' assignments are counted under.
    fn readings_key(&self) -> Option<(SharedSample, Identifier)> {
        match &*self.state.try_borrow().ok()? {
            PropertyState::Attached { handle, sample, .. } => {
                let key = Identifier::new(&format!("readings_of_{}", handle.name())).ok()?;
                Some((sample.clone(), key))
            }
            _ => None,
        }
    }

    fn readings_of(&self, py: Python<'_>) -> PyResult<Option<Readings>> {
        match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => Ok(property.readings().cloned()),
            PropertyState::Attached { handle, .. } => Ok(handle.readings()),
            PropertyState::Cell {
                table,
                index,
                column,
            } => table
                .borrow(py)
                .with_cell(py, false, index, column, |cell| {
                    Ok(cell.readings().cloned())
                }),
        }
    }

    /// Readings given to a value a formula computes: they would replace the
    /// formula's value with their mean and hold no override, so the formula's
    /// value was lost and nothing said it had been replaced.
    fn refuse_readings_over_a_formula(&self, py: Python<'_>) -> PyResult<()> {
        if self.computed(py)? {
            return Err(PyValueError::new_err(
                "this value is computed by its formula, and readings are measured: assign one \
                 number to override the formula, or declare the property with \
                 value=sk.stats.mean in the model to compute it from readings",
            ));
        }
        Ok(())
    }

    /// Whether a number is what this holds now — a numeric value, or
    /// readings — as the command line judges a quantity without a unit: text
    /// or a boolean assigned to it would make a column of mixed kinds.
    fn holds_a_number(&self, py: Python<'_>) -> PyResult<bool> {
        let numeric = |property: &Property| {
            property.readings().is_some()
                || matches!(
                    property.peek_value(),
                    Some(Value::Integer(_) | Value::Number(_))
                )
        };
        match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => Ok(numeric(property)),
            PropertyState::Attached { handle, .. } => Ok(handle.peek(numeric)),
            PropertyState::Cell {
                table,
                index,
                column,
            } => table
                .borrow(py)
                .with_cell(py, false, index, column, |cell| Ok(numeric(cell))),
        }
    }

    fn computed(&self, py: Python<'_>) -> PyResult<bool> {
        match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => Ok(property.is_computed()),
            PropertyState::Attached { handle, .. } => Ok(handle.is_computed()),
            PropertyState::Cell { table, column, .. } => table
                .borrow(py)
                .read(py, false, |held| Ok(held.is_derived(column))),
        }
    }

    /// Who this is, for a message: the property's name, or the cell's address.
    fn label(&self, py: Python<'_>) -> String {
        match &*self.state.borrow() {
            PropertyState::Detached(_) => "this property".to_string(),
            PropertyState::Attached { handle, .. } => format!("'{}'", handle.name()),
            PropertyState::Cell {
                table,
                index,
                column,
            } => {
                let table = table.borrow(py);
                format!("'{}.{column}' at ({})", table.name(), describe_index(index))
            }
        }
    }

    /// The project a rendering resolves against, and the property's address in
    /// `[property.*]`.
    fn context(&self, py: Python<'_>) -> (Option<Rc<ProjectConfig>>, String) {
        match &*self.state.borrow() {
            PropertyState::Detached(_) => (None, String::new()),
            PropertyState::Attached { handle, file, .. } => {
                (file.borrow().config.clone(), handle.name().to_string())
            }
            PropertyState::Cell { table, column, .. } => {
                let table = table.borrow(py);
                (table.config(), format!("{}.{column}", table.name()))
            }
        }
    }

    fn apply(&self, py: Python<'_>, change: Change) -> PyResult<()> {
        // A property of no sample — one a formula makes for its own
        // arithmetic — is the formula's to change.
        if !matches!(&*self.state.borrow(), PropertyState::Detached(_)) {
            refuse_while_a_formula_runs(&self.label(py))?;
        }
        let cell = {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            match &mut *state {
                PropertyState::Detached(property) => {
                    change_property(property, change);
                    return Ok(());
                }
                PropertyState::Attached { handle, sample, .. } => {
                    refuse_while_computing(handle)?;
                    sample.touch(handle.name().as_str());
                    change_handle(handle, change);
                    return Ok(());
                }
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => (table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        let (table, index, column) = cell;
        let table = table.borrow(py);
        table.edit_cell(py, &index, &column, change)
    }

    /// What a sample or a table stores when this object is given to it: the
    /// property itself for one not yet assigned, and a copy of the quantity for
    /// one that belongs somewhere already. Whether it was moved is the second.
    /// Which formulas this property carries, read without taking it: what a
    /// declaration is judged against before the property is installed.
    fn formulas(&self, py: Python<'_>) -> PyResult<Formulas> {
        match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Detached(property) => Ok(Formulas::of(property)),
            PropertyState::Attached { handle, .. } => Ok(handle.peek(Formulas::of)),
            PropertyState::Cell { .. } => {
                let _ = py;
                Ok(Formulas::default())
            }
        }
    }

    fn take(&self, py: Python<'_>) -> PyResult<(Property, bool)> {
        let copied = {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            match &mut *state {
                PropertyState::Detached(property) => {
                    let taken =
                        std::mem::replace(&mut **property, Property::stored(Value::absent()));
                    return Ok((taken, true));
                }
                PropertyState::Attached { handle, .. } => {
                    return Ok((handle.with(copy_of).or_raise(py)?, false));
                }
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => (table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        let (table, index, column) = copied;
        let copy = table
            .borrow(py)
            .with_cell(py, true, &index, &column, |cell| copy_of(cell).or_raise(py))?;
        Ok((copy, false))
    }

    fn become_attached(&self, handle: PropertyHandle, file: FileRef, sample: SharedSample) {
        *self.state.borrow_mut() = PropertyState::Attached {
            handle,
            file,
            sample,
        };
    }

    fn render(
        &self,
        py: Python<'_>,
        precision: Option<&Precision>,
        unit: bool,
    ) -> PyResult<String> {
        let value = self.value_of(py)?;
        if value.is_not_applicable() {
            return Ok("n/a".to_string());
        }
        if value.is_absent() {
            return Ok(terminal_rendering::ABSENT.to_string());
        }
        let uncertainty = self.uncertainty_of(py)?;
        let mut presentation = self.presentation_of(py)?;
        if !unit {
            presentation.unit = Some(String::new());
        }
        let (config, address) = self.context(py);
        Ok(terminal_rendering::quantity(
            &value,
            uncertainty.as_ref(),
            &presentation,
            &address,
            config.as_deref(),
            precision,
        ))
    }

    /// The value as Python arithmetic takes it: a number, or a `TypeError`
    /// naming the property rather than Python's message about `NoneType`.
    fn operand<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        match self.value_of(py)? {
            value @ (Value::Integer(_) | Value::Number(_)) => {
                Ok(bridge::to_python(py, &value)?.into_bound(py))
            }
            Value::Absent => Err(PyTypeError::new_err(format!(
                "{} has no value, so there is nothing to compute with",
                self.label(py)
            ))),
            other => Err(PyTypeError::new_err(format!(
                "{} holds {}, and arithmetic uses a number",
                self.label(py),
                other.kind()
            ))),
        }
    }
}

fn operand_of<'py>(other: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    match other.cast::<PyProperty>() {
        Ok(property) => property.borrow().operand(other.py()),
        Err(_) => Ok(other.clone()),
    }
}

fn builtin<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    py.import("builtins")?.getattr(name)
}

fn operator<'py>(py: Python<'py>, name: &str) -> PyResult<Bound<'py, PyAny>> {
    py.import("operator")?.getattr(name)
}

/// `f(a, b)` for a binary operator, a `Property` on either side standing for
/// its value.
fn binary(
    py: Python<'_>,
    name: &str,
    left: &Bound<'_, PyAny>,
    right: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    Ok(operator(py, name)?.call1((left, right))?.unbind())
}

/// Gives a formula's result to the bridge: a `Property` object's value and
/// uncertainty, or nothing for any other object.
pub fn property_quantity(
    object: &Bound<'_, PyAny>,
) -> PyResult<Option<(Value, Option<Uncertainty>)>> {
    let Ok(property) = object.cast::<PyProperty>() else {
        return Ok(None);
    };
    let py = object.py();
    let property = property.borrow();
    Ok(Some((property.value_of(py)?, property.uncertainty_of(py)?)))
}

/// An argument Python gave, `None` counting as not given.
fn given<'py>(object: Option<&Bound<'py, PyAny>>) -> Option<Bound<'py, PyAny>> {
    object.filter(|object| !object.is_none()).cloned()
}

impl PyProperty {
    /// The state as the workbench and the messages say it.
    fn state_said(&self, py: Python<'_>) -> PyResult<&'static str> {
        let cell = {
            let state = self.state.try_borrow().map_err(|_| busy())?;
            match &*state {
                PropertyState::Detached(property) => return Ok(detached_state(property)),
                PropertyState::Attached {
                    handle,
                    sample,
                    file,
                } => {
                    let name = handle.name().clone();
                    let word = sample.read_sample("a value's state", |held| {
                        let handle = held.property(&name).or_raise(py)?;
                        Ok(if handle.is_edited() {
                            "edited"
                        } else if statistic_owed(&handle) {
                            "never computed"
                        } else if !handle.is_computed() && !handle.has_uncertainty_formula() {
                            // Read without its model, a derived value says what
                            // its records say.
                            if handle.records().computed.is_some() {
                                match fingerprint::check_property(held, &name).or_raise(py)? {
                                    fingerprint::Freshness::Current => "current",
                                    // `check_property` raises a cycle rather
                                    // than answering `Unjudged`; were it ever
                                    // to answer it, not current is the side to
                                    // err on.
                                    stale @ (fingerprint::Freshness::Stale { .. }
                                    | fingerprint::Freshness::Broken { .. }
                                    | fingerprint::Freshness::Unjudged { .. }) => {
                                        outdated_or_waiting(held, &stale)
                                    }
                                    fingerprint::Freshness::Edited
                                    | fingerprint::Freshness::RecordMissing => "edited",
                                    fingerprint::Freshness::Failed { .. } => "failed",
                                    fingerprint::Freshness::Source => {
                                        handle.peek(entered_or_absent)
                                    }
                                }
                            } else {
                                handle.peek(entered_or_absent)
                            }
                        } else if handle.has_failed() {
                            "failed"
                        } else if owed(&handle) {
                            "never computed"
                        } else if fingerprint::is_stale(held, &name).or_raise(py)? {
                            match fingerprint::check_property(held, &name) {
                                Ok(stale) => outdated_or_waiting(held, &stale),
                                Err(_) => "outdated",
                            }
                        } else {
                            "current"
                        })
                    })?;
                    // What the model owes it, as `status` says it: a formula
                    // changed since it computed this value, or one it reads —
                    // outdated, though current by its inputs; an input nobody
                    // entered — waiting, where it was said never computed.
                    let planned = planned_of(sample, file);
                    let owed = planned
                        .get(name.as_str())
                        .and_then(|said| crate::collection::computation::planned_word(said));
                    return Ok(match (word, owed) {
                        ("current", Some("outdated")) => "outdated",
                        ("current" | "never computed" | "outdated", Some("waiting")) => "waiting",
                        _ => word,
                    });
                }
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => (table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        let (table, index, column) = cell;
        let table = table.borrow(py);
        table.cell_state(py, &index, &column)
    }
}

#[pymethods]
impl PyProperty {
    #[classattr]
    const __hash__: Option<Py<PyAny>> = None;

    #[new]
    #[pyo3(signature = (
        value=None,
        uncertainty=None,
        unit=None,
        symbol=None,
        precision=None,
        compute=None,
        compute_value=None,
        compute_uncertainty=None,
        compute_column=None,
        depends_on=None
    ),
    text_signature = "(value=None, uncertainty=None, unit=None, symbol=None, precision=None, \
                      compute=None, compute_value=None, compute_uncertainty=None, \
                      compute_column=None, depends_on=None)")]
    #[allow(clippy::too_many_arguments)] // The keyword surface python-api specifies.
    fn new(
        py: Python<'_>,
        value: Option<&Bound<'_, PyAny>>,
        uncertainty: Option<&Bound<'_, PyAny>>,
        unit: Option<String>,
        symbol: Option<String>,
        precision: Option<&Bound<'_, PyAny>>,
        compute: Option<&Bound<'_, PyAny>>,
        compute_value: Option<&Bound<'_, PyAny>>,
        compute_uncertainty: Option<&Bound<'_, PyAny>>,
        compute_column: Option<&Bound<'_, PyAny>>,
        depends_on: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        refuse_symbol(symbol.as_deref())?;
        refuse_precision(precision)?;
        let (value, uncertainty) = (given(value), given(uncertainty));
        let (compute, compute_value, compute_uncertainty) = (
            given(compute),
            given(compute_value),
            given(compute_uncertainty),
        );
        let compute_column = given(compute_column);
        for callable in [
            &compute,
            &compute_value,
            &compute_uncertainty,
            &compute_column,
        ]
        .into_iter()
        .flatten()
        {
            if !callable.is_callable() {
                return Err(PyTypeError::new_err(format!(
                    "a formula is a callable, and this is {}",
                    bridge::a_type(callable)
                )));
            }
        }
        // A column's formula at once fills a table's column, and nothing beside
        // it computes one of its cells.
        if compute_column.is_some()
            && (compute.is_some()
                || compute_value.is_some()
                || compute_uncertainty.is_some()
                || value.is_some()
                || uncertainty.is_some())
        {
            return Err(PyValueError::new_err(
                "compute_column computes a table's whole column, each cell from it: pass it \
                 alone, with what it reads in depends_on",
            ));
        }
        // What a table taking this as a column reads of it, as it was given:
        // the formulas with their own `self`, and a value of its own, which a
        // column refuses.
        let entered = match (&value, &uncertainty) {
            (Some(given), _)
                if given.is_instance_of::<PyList>() || given.is_instance_of::<PyTuple>() =>
            {
                Some("readings")
            }
            (Some(given), _) if !given.is_instance_of::<PyStatistic>() => Some("a value"),
            (_, Some(given)) if !given.is_instance_of::<PyStatistic>() => Some("an uncertainty"),
            _ => None,
        };
        let kept =
            |given: &Option<Bound<'_, PyAny>>| given.as_ref().map(|given| given.clone().unbind());
        let column_given = (
            kept(&compute),
            kept(&compute_value),
            kept(&compute_uncertainty),
            kept(&compute_column),
            depends_on.map(|given| given.clone().unbind()),
            entered,
        );
        if compute.is_some() && compute_value.is_some() {
            return Err(PyValueError::new_err(
                "compute gives the whole property, its value included: pass compute, or \
                 compute_value for the value alone",
            ));
        }
        // An uncertainty beside the property's formula is what compute_value
        // is for: rc.1's compute= kept one, and this one would not.
        if compute.is_some() && (compute_uncertainty.is_some() || uncertainty.is_some()) {
            return Err(PyValueError::new_err(
                "compute gives the whole property, its uncertainty included: for a value \
                 computed beside an uncertainty given apart, pass compute_value",
            ));
        }
        if (compute.is_some() || compute_value.is_some()) && value.is_some() {
            return Err(PyValueError::new_err(
                "a computed property takes its value from its formula: pass value or the \
                 formula, not both",
            ));
        }
        if compute_uncertainty.is_some() && uncertainty.is_some() {
            return Err(PyValueError::new_err(
                "a computed uncertainty comes from its formula: pass uncertainty or the formula, \
                 not both",
            ));
        }

        // A statistic of the readings stands for the value or the uncertainty.
        let location = match &value {
            Some(given) if given.is_instance_of::<PyStatistic>() => Some(location_of(given)?),
            _ => None,
        };
        let convention = match &uncertainty {
            Some(given) if given.is_instance_of::<PyStatistic>() => Some(convention_of(given)?),
            _ => None,
        };
        // Kept as they were given, before a statistic stands for a value.
        // Each under the keyword it was given with: a formula's digest is
        // taken of it, and the reference says so.
        let mut code: Vec<(String, Py<PyAny>)> = Vec::new();
        for (role, given) in [
            ("compute", &compute),
            ("compute_value", &compute_value),
            ("compute_uncertainty", &compute_uncertainty),
        ] {
            if let Some(given) = given {
                code.push((role.to_string(), function_of(given)));
            }
        }
        for (role, given, statistic) in [
            ("value", &value, location.is_some()),
            ("uncertainty", &uncertainty, convention.is_some()),
        ] {
            if let (Some(given), true) = (given, statistic) {
                code.push((role.to_string(), given.clone().unbind()));
            }
        }
        if !code.is_empty()
            && let Some(unit) = &unit
        {
            code.push((
                "unit".to_string(),
                pyo3::types::PyString::new(py, unit).into_any().unbind(),
            ));
        }
        let value = if location.is_some() { None } else { value };
        let uncertainty = if convention.is_some() {
            None
        } else {
            uncertainty
        };

        let mut property = if let Some(formula) = compute_value {
            Property::computed(Rc::new(PythonCompute {
                callable: bridge::Formula::held(formula)?,
                gives_value: true,
            }))
        } else if let Some(formula) = compute {
            Property::joint(Rc::new(PythonQuantityCompute {
                callable: bridge::Formula::held(formula)?,
            }))
        } else {
            match value {
                None => Property::stored(Value::absent()),
                Some(value)
                    if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() =>
                {
                    Property::measured(readings_from(&value)?, None)
                }
                Some(value) => {
                    if unit.is_some() {
                        refuse_other_than_a_number(&value, "this property", unit.as_deref())?;
                    }
                    Property::stored(bridge::one_value(&value)?)
                }
            }
        };
        if let Some(uncertainty) = uncertainty {
            property.set_uncertainty(bridge::uncertainty_from_python(&uncertainty)?);
        }
        if let Some(formula) = compute_uncertainty {
            property.set_uncertainty_formula(Rc::new(PythonCompute {
                callable: bridge::Formula::held(formula)?,
                gives_value: false,
            }));
        }
        property.declare_statistics(location, convention);
        let unit_given = unit.clone();
        property.set_presentation(Presentation {
            unit,
            symbol: None,
            precision: None,
        });
        let built = PyProperty::detached(property);
        *built.code.borrow_mut() = code;
        let (compute, compute_value, compute_uncertainty, compute_column, given_inputs, entered) =
            column_given;
        *built.as_column.borrow_mut() = Some(ColumnGiven {
            unit: unit_given,
            statistics: crate::core::property::DeclaredStatistics {
                value: location,
                uncertainty: convention,
            },
            compute,
            compute_value,
            compute_uncertainty,
            compute_column,
            depends_on: given_inputs,
            entered,
        });
        // Kept, not applied: the names it gives need not exist yet, and a model
        // ordinarily declares them before the values they read.
        if let Some(given) = depends_on {
            *built.declared.borrow_mut() = Some(declared_from(given, "depends_on")?);
        }
        Ok(built)
    }

    /// The value — a number, text, a boolean, a date, ``samplekit.NA``, or
    /// ``None`` when there is none.
    ///
    /// Assigning a value to a computed property overrides its formula: the
    /// property reads *edited* until ``compute(force=True)`` gives it back to the
    /// formula. Assigning a list gives the property readings. A property with a
    /// unit takes numbers only, and raises ``ValueError`` otherwise.
    #[getter]
    fn value(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        bridge::to_python(py, &self.value_of(py)?)
    }

    #[setter]
    fn set_value(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
            self.refuse_readings_over_a_formula(py)?;
            return self.apply(py, Change::Readings(readings_from(value)?));
        }
        let unit = self.presentation_of(py)?.unit;
        if unit.is_some() || self.holds_a_number(py)? {
            refuse_other_than_a_number(value, &self.label(py), unit.as_deref())?;
        }
        let value = bridge::one_value(value)?;
        self.apply(py, Change::Value(value))
    }

    /// The value — the same as ``value``, spelt as a field path spells it
    /// (``area.v``).
    #[getter]
    fn v(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.value(py)
    }

    #[setter]
    fn set_v(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.set_value(py, value)
    }

    /// The absolute standard uncertainty, in the value's unit, or ``None``.
    #[getter]
    fn uncertainty(&self, py: Python<'_>) -> PyResult<Option<f64>> {
        Ok(self
            .uncertainty_of(py)?
            .map(|uncertainty| uncertainty.magnitude()))
    }

    #[setter]
    fn set_uncertainty(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let uncertainty = bridge::uncertainty_from_python(value)?;
        self.apply(py, Change::Uncertainty(uncertainty))
    }

    /// The uncertainty — the same as ``uncertainty``, spelt as a field path spells
    /// it (``area.u``).
    #[getter]
    fn u(&self, py: Python<'_>) -> PyResult<Option<f64>> {
        self.uncertainty(py)
    }

    #[setter]
    fn set_u(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.set_uncertainty(py, value)
    }

    /// The statistics of the readings, as a ``Summary``, or ``None`` without
    /// readings.
    #[getter]
    fn stats(&self, py: Python<'_>) -> PyResult<Option<PySummary>> {
        Ok(self.readings_of(py)?.map(|readings| PySummary {
            summary: crate::core::statistics::summarize(&readings),
            weighted_mean: None,
            weighted_error: None,
            considered: None,
            not_applicable: 0,
            row: None,
        }))
    }

    /// The readings, or ``None`` for a value not measured several times.
    ///
    /// The list is bound to the property: ``append``, ``remove`` or an item
    /// assigned changes the property, as assigning a whole list does. Readings are
    /// refused on a value a formula computes.
    #[getter]
    fn readings(slf: &Bound<'_, Self>) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        let Some(readings) = slf.borrow().readings_of(py)? else {
            return Ok(None);
        };
        let items = PyList::new(py, readings.as_slice())?;
        let epoch = slf.borrow().readings_epoch();
        Ok(Some(
            bridge::bound_list_factory(py)?
                .call1((slf, "readings", epoch, items))?
                .unbind(),
        ))
    }

    /// Stores the readings a bound list of them would become.
    fn _commit_list(
        &self,
        py: Python<'_>,
        name: &str,
        epoch: u64,
        candidate: &Bound<'_, PyList>,
    ) -> PyResult<Option<Py<PyAny>>> {
        if name != "readings" || self.readings_epoch() != epoch {
            return Ok(None);
        }
        let readings = readings_from(candidate.as_any())?;
        let committed = PyList::new(py, readings.as_slice())?;
        self.apply(py, Change::Readings(readings))?;
        Ok(Some(committed.into_any().unbind()))
    }

    #[setter]
    fn set_readings(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if value.is_none() {
            return Err(PyValueError::new_err(
                "readings are replaced by other readings, or dropped by assigning a value",
            ));
        }
        self.refuse_readings_over_a_formula(py)?;
        self.apply(py, Change::Readings(readings_from(value)?))?;
        if let Some((sample, key)) = self.readings_key() {
            sample.advance(&key);
        }
        Ok(())
    }

    /// The unit, as the file writes it — ``"g"``, ``"degC"`` — or ``None``.
    #[getter]
    fn unit(&self, py: Python<'_>) -> PyResult<Option<String>> {
        Ok(self.presentation_of(py)?.unit)
    }

    #[setter]
    fn set_unit(&self, py: Python<'_>, unit: Option<String>) -> PyResult<()> {
        let presentation = Presentation {
            unit,
            ..self.presentation_of(py)?
        };
        self.apply(py, Change::Presentation(presentation))
    }

    /// The property's symbol — the one this sample's file writes, else the one
    /// the project declares in ``.samplekitrc`` under ``[property.NAME]``, else
    /// ``None``. Read only: a symbol is the project's, declared there, never
    /// set by a model or a script.
    #[getter]
    fn symbol(&self, py: Python<'_>) -> PyResult<Option<String>> {
        // Read only: a setter let a model's `__init__` give a symbol the
        // constructor refuses, and a save write it into the file.
        if let Some(written) = self.presentation_of(py)?.symbol {
            return Ok(Some(written));
        }
        let (config, address) = self.context(py);
        Ok(config.and_then(|config| config.property(&address).and_then(|d| d.symbol.clone())))
    }

    /// Whether a formula gives the value.
    #[getter]
    fn is_computed(&self, py: Python<'_>) -> PyResult<bool> {
        self.computed(py)
    }

    /// Whether the value is the statistic its readings declare, not taken yet:
    /// a computation gives it, and a value reading it waits for nothing. The
    /// worker's plan asks it; nothing else does.
    #[getter]
    fn _statistic_owed(&self) -> PyResult<bool> {
        Ok(match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Attached { handle, .. } => {
                handle.peek(|property| {
                    property.has_declared_statistic()
                        && property.readings().is_some()
                        && property.written_value().is_none()
                        && !property.statistic_taken()
                }) || statistic_stated_otherwise(handle)
            }
            _ => false,
        })
    }

    /// How many formulas give it, for the worker's count alone: two where the
    /// value and the uncertainty have one each, counted apart.
    #[getter]
    fn _formulas(&self) -> PyResult<usize> {
        Ok(match &*self.state.try_borrow().map_err(|_| busy())? {
            PropertyState::Attached { handle, .. } => handle.peek(|property| {
                if property.is_computed()
                    && !property.is_joint()
                    && property.has_uncertainty_formula()
                {
                    2
                } else {
                    1
                }
            }),
            _ => 1,
        })
    }

    /// The computed value discarded, for the worker alone: a failure an earlier
    /// model gave, cleared where the value now waits. Withdrawn from scripts —
    /// fingerprints say what is outdated, and ``compute(force=True)`` gives a
    /// value back to its formula.
    fn _invalidate(&self, py: Python<'_>) -> PyResult<()> {
        self.apply(py, Change::Invalidate)
    }

    /// Where the value stands, in one word.
    ///
    /// One of ``"entered"`` (typed in, no formula), ``"absent"`` (no formula,
    /// and nothing typed in), ``"current"``,
    /// ``"outdated"`` (an input changed since it was computed), ``"waiting"``
    /// (an input it reads was emptied since: the number is the one computed
    /// before), ``"never_computed"``, ``"edited"`` (a value typed over its
    /// formula) or ``"failed"`` (its formula raised): the words a filter's
    /// ``state`` takes.
    #[getter]
    fn state(&self, py: Python<'_>) -> PyResult<&'static str> {
        // One word for a program, as a filter, `--accept` and a file write it.
        Ok(match self.state_said(py)? {
            "never computed" => "never_computed",
            word => word,
        })
    }

    /// What the formula raised when it last ran, or ``None`` if it did not fail.
    #[getter]
    fn failure(&self, py: Python<'_>) -> PyResult<Option<String>> {
        let state = self.state.try_borrow().map_err(|_| busy())?;
        match &*state {
            PropertyState::Detached(property) => Ok(property.peek_failure()),
            PropertyState::Attached { handle, sample, .. } => {
                let name = handle.name().clone();
                sample.read_sample("a value's failure", |held| {
                    let handle = held.property(&name).or_raise(py)?;
                    Ok(handle
                        .peek(Property::peek_failure)
                        .or_else(|| handle.records().failure.clone()))
                })
            }
            // A cell's failure, as a property's: what its formula raised in
            // this session, or what its file recorded. It always answered
            // `None`, so no script could see a broken column.
            PropertyState::Cell {
                table,
                index,
                column,
            } => {
                let (table, index, column) = (table.clone_ref(py), index.clone(), column.clone());
                drop(state);
                table
                    .borrow(py)
                    .with_cell(py, false, &index, &column, |cell| {
                        Ok(cell
                            .peek_failure()
                            .or_else(|| cell.records().failure.clone()))
                    })
            }
        }
    }

    /// Whether the value is outdated — an input changed since it was computed.
    #[getter]
    fn is_outdated(&self, py: Python<'_>) -> PyResult<bool> {
        let cell = {
            let state = self.state.try_borrow().map_err(|_| busy())?;
            match &*state {
                PropertyState::Detached(_) => return Ok(false),
                PropertyState::Attached {
                    handle,
                    sample,
                    file,
                } => {
                    let name = handle.name().clone();
                    let stale = sample.read_sample("whether a value is outdated", |held| {
                        fingerprint::is_stale(held, &name).or_raise(py)
                    })?;
                    // A changed formula outdates it as a changed input does,
                    // as `state` says: the two answered apart.
                    return Ok(stale
                        || planned_of(sample, file)
                            .get(name.as_str())
                            .is_some_and(|said| said.contains("formula changed")));
                }
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => (table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        let (table, index, column) = cell;
        let table = table.borrow(py);
        table.is_cell_stale(py, &index, &column)
    }

    /// Whether the value is edited — typed over its formula, or a computed table
    /// cell whose record is missing.
    #[getter]
    fn is_edited(&self, py: Python<'_>) -> PyResult<bool> {
        let cell = {
            let state = self.state.try_borrow().map_err(|_| busy())?;
            match &*state {
                PropertyState::Detached(property) => return Ok(property.is_edited()),
                PropertyState::Attached { handle, .. } => return Ok(handle.is_edited()),
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => (table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        let (table, index, column) = cell;
        let table = table.borrow(py);
        Ok(table.cell_state(py, &index, &column)? == "edited")
    }

    /// The edited values this one was computed from, directly or further up.
    #[getter]
    fn edited_upstream(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let (sample, name) = {
            let state = self.state.try_borrow().map_err(|_| busy())?;
            match &*state {
                PropertyState::Attached { handle, sample, .. } => {
                    (sample.clone(), handle.name().clone())
                }
                PropertyState::Detached(_) => return Ok(Vec::new()),
                // A cell, by what its column's formula reads: it answered
                // `[]` whatever its row held.
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => {
                    let (table, index, column) =
                        (table.clone_ref(py), index.clone(), column.clone());
                    drop(state);
                    return table.borrow(py).edited_upstream_of(py, &index, &column);
                }
            }
        };
        sample.read_sample("what a value rests on", |held| {
            Ok(fingerprint::edited_upstream(held, &name)
                .into_iter()
                .map(|input| input.to_string())
                .collect())
        })
    }

    /// Compute the value, after the inputs it needs.
    ///
    /// Only a value that is outdated or never computed runs, unless asked
    /// otherwise. The file is not written: ``save()`` does that.
    ///
    /// Args:
    ///     rerun: Run it even when it is current.
    ///     force: Run it even when it is edited, giving it back to its formula.
    ///
    /// Raises:
    ///     ValueError: The value has no formula.
    ///     Exception: Whatever the formula raised, with a note naming the value.
    #[pyo3(signature = (rerun=false, force=false))]
    fn compute(&self, py: Python<'_>, rerun: bool, force: bool) -> PyResult<()> {
        enum Where {
            Detached,
            Sample(SharedSample, FileRef, Identifier),
            Cell(Py<PyTable>, Vec<Value>, Identifier),
        }
        let place = {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            match &mut *state {
                PropertyState::Detached(property) => {
                    if !property.is_computed() && !property.has_uncertainty_formula() {
                        return Err(no_formula("this property"));
                    }
                    // A detached value has no inputs to be stale against: it is
                    // owed a run, or current.
                    let edited = property.is_edited();
                    let owed_here = (property.is_computed() && property.peek_value().is_none())
                        || (property.has_uncertainty_formula()
                            && property.peek_uncertainty().is_none());
                    if (edited && !force) || (!edited && !owed_here && !rerun) {
                        return Ok(());
                    }
                    property.restore_formula();
                    property.invalidate();
                    Where::Detached
                }
                PropertyState::Attached {
                    handle,
                    sample,
                    file,
                } => Where::Sample(sample.clone(), file.clone(), handle.name().clone()),
                PropertyState::Cell {
                    table,
                    index,
                    column,
                } => Where::Cell(table.clone_ref(py), index.clone(), column.clone()),
            }
        };
        match place {
            // A computation asked for, so reads run the formula.
            Where::Detached => crate::core::property::computing(|| {
                self.value_of(py)?;
                self.uncertainty_of(py).map(|_| ())
            }),
            Where::Sample(sample, file, name) => {
                compute_selected(py, &sample, &file, Some(&[name.to_string()]), rerun, force)
            }
            Where::Cell(table, index, column) => {
                let table = table.borrow(py);
                crate::core::property::computing(|| {
                    table.compute_cell(py, &index, &column, rerun, force)
                })
            }
        }
    }

    /// Format the property at its declared precision.
    ///
    /// Args:
    ///     unit: Include the unit.
    ///
    /// Returns:
    ///     The property as text, such as ``"25.056 ± 0.053 mm²"``. For another
    ///     precision, use a format specifier: ``f"{steel.area:.2f}"``.
    #[pyo3(signature = (unit=true))]
    fn format(&self, py: Python<'_>, unit: bool) -> PyResult<String> {
        self.render(py, None, unit)
    }

    fn __str__(&self, py: Python<'_>) -> PyResult<String> {
        self.render(py, None, true)
    }

    /// The property as a style writes it: its units, its symbols, its form
    /// of a property — ``\SI{12.5 \pm 0.05}{\gram}`` in a style declaring
    /// ``property = '\SI{{value} \pm {uncertainty}}{{unit}}'``.
    ///
    /// Args:
    ///     style: A style's name: ``"plain"``, one SampleKit gives —
    ///         ``"latex"``, ``"markdown"``, ``"html"`` — or one ``.samplekitrc``
    ///         declares. Without it, ``[render] style``'s, as ``str()`` gives.
    ///
    /// Returns:
    ///     The text, ``n/a`` where the value does not apply, and the style's
    ///     ``missing`` where there is none.
    ///
    /// Raises:
    ///     ValueError: No style has that name.
    ///
    /// Example:
    ///     >>> steel.area.to_text("latex")
    ///     '$25.056 \\pm 0.053$ mm²'
    #[pyo3(signature = (style=None))]
    fn to_text(&self, py: Python<'_>, style: Option<&str>) -> PyResult<String> {
        let (config, address) = self.context(py);
        style_known(config.as_deref(), style)?;
        let value = self.value_of(py)?;
        if value.is_not_applicable() {
            return Ok("n/a".to_string());
        }
        if value.is_absent() {
            return Ok(style
                .and_then(|name| config.as_deref().and_then(|config| config.style(name)))
                .and_then(|settings| settings.missing.clone())
                .unwrap_or_else(|| terminal_rendering::ABSENT.to_string()));
        }
        let uncertainty = self.uncertainty_of(py)?;
        let presentation = self.presentation_of(py)?;
        Ok(terminal_rendering::quantity_in(
            &value,
            uncertainty.as_ref(),
            &presentation,
            &address,
            config.as_deref(),
            style,
        ))
    }

    /// A format specifier is a column precision, as `-c area:.2f` writes one.
    fn __format__(&self, py: Python<'_>, spec: &str) -> PyResult<String> {
        if spec.is_empty() {
            return self.render(py, None, true);
        }
        // The precision's own refusal names the specifier: said once, with
        // what a format specifier is here.
        let precision = Precision::both(spec).map_err(|error| {
            PyValueError::new_err(format!(
                "{error} — a format specifier is a column precision, as -c area:.2f writes one"
            ))
        })?;
        self.render(py, Some(&precision), true)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let value = self.value_of(py)?;
        let mut parts = vec![format!(
            "value={}",
            bridge::repr(bridge::to_python(py, &value)?.bind(py))
        )];
        if let Some(uncertainty) = self.uncertainty_of(py)? {
            let magnitude = uncertainty.magnitude().into_pyobject(py)?;
            parts.push(format!("uncertainty={}", bridge::repr(magnitude.as_any())));
        }
        if let Some(unit) = self.presentation_of(py)?.unit {
            parts.push(format!(
                "unit={}",
                bridge::repr(PyString::new(py, &unit).as_any())
            ));
        }
        Ok(format!("Property({})", parts.join(", ")))
    }

    fn __float__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(builtin(py, "float")?.call1((self.operand(py)?,))?.unbind())
    }

    fn __int__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(builtin(py, "int")?.call1((self.operand(py)?,))?.unbind())
    }

    fn __bool__(&self, py: Python<'_>) -> PyResult<bool> {
        bridge::to_python(py, &self.value_of(py)?)?
            .bind(py)
            .is_truthy()
    }

    fn __add__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "add", &self.operand(py)?, &operand_of(other)?)
    }

    fn __radd__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "add", &operand_of(other)?, &self.operand(py)?)
    }

    fn __sub__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "sub", &self.operand(py)?, &operand_of(other)?)
    }

    fn __rsub__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "sub", &operand_of(other)?, &self.operand(py)?)
    }

    fn __mul__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "mul", &self.operand(py)?, &operand_of(other)?)
    }

    fn __rmul__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "mul", &operand_of(other)?, &self.operand(py)?)
    }

    fn __truediv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "truediv", &self.operand(py)?, &operand_of(other)?)
    }

    fn __rtruediv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "truediv", &operand_of(other)?, &self.operand(py)?)
    }

    fn __floordiv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "floordiv", &self.operand(py)?, &operand_of(other)?)
    }

    fn __rfloordiv__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "floordiv", &operand_of(other)?, &self.operand(py)?)
    }

    fn __mod__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "mod", &self.operand(py)?, &operand_of(other)?)
    }

    fn __rmod__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        binary(py, "mod", &operand_of(other)?, &self.operand(py)?)
    }

    /// `divmod(p, n)`, as `p // n` and `p % n` are taken.
    fn __divmod__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let quotient = self.__floordiv__(py, other)?;
        let remainder = self.__mod__(py, other)?;
        Ok(PyTuple::new(py, [quotient, remainder])?.into_any().unbind())
    }

    fn __rdivmod__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let quotient = self.__rfloordiv__(py, other)?;
        let remainder = self.__rmod__(py, other)?;
        Ok(PyTuple::new(py, [quotient, remainder])?.into_any().unbind())
    }

    fn __pow__(
        &self,
        py: Python<'_>,
        other: &Bound<'_, PyAny>,
        modulo: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let power = builtin(py, "pow")?;
        let result = if modulo.is_none() {
            power.call1((self.operand(py)?, operand_of(other)?))?
        } else {
            power.call1((self.operand(py)?, operand_of(other)?, modulo))?
        };
        Ok(result.unbind())
    }

    fn __rpow__(
        &self,
        py: Python<'_>,
        other: &Bound<'_, PyAny>,
        modulo: &Bound<'_, PyAny>,
    ) -> PyResult<Py<PyAny>> {
        let power = builtin(py, "pow")?;
        let result = if modulo.is_none() {
            power.call1((operand_of(other)?, self.operand(py)?))?
        } else {
            power.call1((operand_of(other)?, self.operand(py)?, modulo))?
        };
        Ok(result.unbind())
    }

    fn __neg__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(operator(py, "neg")?.call1((self.operand(py)?,))?.unbind())
    }

    fn __pos__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(operator(py, "pos")?.call1((self.operand(py)?,))?.unbind())
    }

    fn __abs__(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(builtin(py, "abs")?.call1((self.operand(py)?,))?.unbind())
    }

    #[pyo3(signature = (ndigits=None))]
    fn __round__(&self, py: Python<'_>, ndigits: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        let round = builtin(py, "round")?;
        let result = match ndigits {
            Some(ndigits) => round.call1((self.operand(py)?, ndigits))?,
            None => round.call1((self.operand(py)?,))?,
        };
        Ok(result.unbind())
    }

    /// Comparison compares the value. An absent property equals nothing,
    /// another absent property included, and has no order.
    fn __richcmp__(
        &self,
        py: Python<'_>,
        other: &Bound<'_, PyAny>,
        op: CompareOp,
    ) -> PyResult<Py<PyAny>> {
        let mine = self.value_of(py)?;
        let theirs = match other.cast::<PyProperty>() {
            Ok(property) => Some(property.borrow().value_of(py)?),
            Err(_) => None,
        };
        if mine.is_absent() || theirs.as_ref().is_some_and(Value::is_absent) {
            return match op {
                CompareOp::Eq => false.into_py_any(py),
                CompareOp::Ne => true.into_py_any(py),
                _ => Err(PyTypeError::new_err(format!(
                    "{} has no value to order",
                    if mine.is_absent() {
                        self.label(py)
                    } else {
                        "the other property".to_string()
                    }
                ))),
            };
        }
        let left = bridge::to_python(py, &mine)?.into_bound(py);
        let right = match theirs {
            Some(value) => bridge::to_python(py, &value)?.into_bound(py),
            None => other.clone(),
        };
        Ok(left.rich_compare(right, op)?.unbind())
    }

    fn __getattr__(slf: &Bound<'_, Self>, name: &str) -> PyResult<Py<PyAny>> {
        // The nearest member is offered, in the words every other refusal uses.
        Err(no_attribute(slf.as_any(), name))
    }
}

// ------------------------------------------------------------------ Table

/// Where a `Table` object's table lives.
pub enum TableState {
    /// Built and not yet assigned, under a provisional name.
    Detached {
        table: Box<Table>,
        /// The qualifiers that may be the table's own name: those whose column
        /// inputs all name columns the table declares.
        qualifiers: Vec<Identifier>,
        /// Each derived column's declaration, by the column, for the sample
        /// that takes the table.
        code: Vec<(String, FormulaCode)>,
    },
    /// A sample's table, reached by its name.
    Attached {
        sample: SharedSample,
        file: FileRef,
        name: Identifier,
    },
}

enum TableTarget<'a> {
    Detached(&'a mut Table),
    Held {
        sample: &'a mut Sample,
        name: &'a Identifier,
    },
}

impl TableTarget<'_> {
    fn add_row(self, py: Python<'_>, cells: Vec<(Identifier, Property)>) -> PyResult<()> {
        match self {
            TableTarget::Detached(table) => {
                table.refuses_what_is_no_number(&cells).or_raise(py)?;
                table.add_row(cells).or_raise(py)
            }
            TableTarget::Held { sample, name } => sample.add_row(name, cells).or_raise(py),
        }
    }

    fn update_row(
        self,
        py: Python<'_>,
        row: &RowAddress,
        cells: Vec<(Identifier, Property)>,
    ) -> PyResult<()> {
        match self {
            TableTarget::Detached(table) => {
                table.refuses_what_is_no_number(&cells).or_raise(py)?;
                table.update_row(row, cells).or_raise(py)
            }
            TableTarget::Held { sample, name } => sample.update_row(name, row, cells).or_raise(py),
        }
    }

    fn invalidate_cell(
        self,
        py: Python<'_>,
        row: &RowAddress,
        column: &Identifier,
    ) -> PyResult<()> {
        match self {
            TableTarget::Detached(table) => table.invalidate_cell(row, column).or_raise(py),
            TableTarget::Held { sample, name } => {
                sample.invalidate_cell(name, row, column).or_raise(py)
            }
        }
    }
}

/// The qualifiers that may be the table's own name: those whose column inputs
/// all name columns the table declares. The one it is assigned as is its own,
/// and any other names another table of the sample.
fn qualifiers_of(
    derivations: &[Derivation],
    declared: &IndexMap<Identifier, ColumnMeta>,
) -> Vec<Identifier> {
    let mut qualifiers: IndexMap<Identifier, bool> = IndexMap::new();
    for derivation in derivations {
        for input in derivation.inputs() {
            if let InputName::Column { table, column } = input {
                let own = qualifiers.entry(table.clone()).or_insert(true);
                *own &= declared.contains_key(column);
            }
        }
    }
    qualifiers
        .into_iter()
        .filter(|(_, own)| *own)
        .map(|(table, _)| table)
        .collect()
}

/// Resolves a table after the tables its derivations read, each in a pass of
/// its own, so that a formula reading one through the sample sees it resolved.
fn resolve_with_upstream(
    py: Python<'_>,
    shared: &SharedSample,
    name: &Identifier,
    only: Option<&[Identifier]>,
) -> PyResult<()> {
    fn walk(
        py: Python<'_>,
        shared: &SharedSample,
        name: &Identifier,
        seen: &mut Vec<Identifier>,
    ) -> PyResult<()> {
        if seen.contains(name) {
            return Ok(());
        }
        seen.push(name.clone());
        let upstream = shared.read_sample("a table", |held| {
            Ok(held
                .table(name)
                .map(|table| table.foreign_tables())
                .unwrap_or_default()
                .into_iter()
                .filter(|other| held.table(other).is_ok())
                .collect::<Vec<_>>())
        })?;
        for other in &upstream {
            walk(py, shared, other, seen)?;
        }
        shared
            .resolving(|held| held.resolve_table(name))?
            .or_raise(py)
    }
    match only {
        None => walk(py, shared, name, &mut Vec::new()),
        // Named columns: the tables read resolved whole, this one only for
        // them.
        Some(columns) => {
            let upstream = shared.read_sample("a table", |held| {
                Ok(held
                    .table(name)
                    .map(|table| table.foreign_tables())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|other| held.table(other).is_ok())
                    .collect::<Vec<_>>())
            })?;
            let mut seen = vec![name.clone()];
            for other in &upstream {
                walk(py, shared, other, &mut seen)?;
            }
            shared
                .resolving(|held| held.resolve_table_columns(name, columns))?
                .or_raise(py)
        }
    }
}

/// What a column's formulas read: a list, or one per channel, `{"v": [...],
/// "u": [...]}`, read together, each input once.
fn column_inputs(given: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let Ok(channels) = given.cast::<PyDict>() else {
        return strings_from(given, "depends_on");
    };
    let mut inputs: Vec<String> = Vec::new();
    for (key, listed) in channels.iter() {
        let key: String = key.extract()?;
        if key != "v" && key != "u" {
            return Err(PyValueError::new_err(format!(
                "depends_on is keyed by channel, \"v\" and \"u\", and '{key}' is not one"
            )));
        }
        for input in strings_from(&listed, "depends_on")? {
            if !inputs.contains(&input) {
                inputs.push(input);
            }
        }
    }
    Ok(inputs)
}

/// A table's formula as the core runs it: given each row, or the columns it
/// reads whole (`whole`), filling `outputs`, what of each cell `half` says.
fn table_derivation(
    _py: Python<'_>,
    outputs: Vec<Identifier>,
    inputs: &[String],
    callable: &Bound<'_, PyAny>,
    whole: bool,
    half: Half,
) -> PyResult<Derivation> {
    let inputs: Vec<InputName> = inputs
        .iter()
        .map(|text| input_name(text))
        .collect::<PyResult<_>>()?;
    let callable = bridge::Formula::held(callable.clone())?;
    Ok(if whole {
        let columns = inputs
            .iter()
            .filter_map(|input| match input {
                InputName::Column { table, column } => Some((table.clone(), column.clone())),
                _ => None,
            })
            .collect();
        Derivation::Column {
            formula: Rc::new(PythonColumnCompute {
                callable,
                outputs: outputs.clone(),
                columns,
            }),
            outputs,
            inputs,
        }
    } else {
        Derivation::Row {
            formula: Rc::new(PythonRowCompute {
                callable,
                outputs: outputs.clone(),
                half,
            }),
            outputs,
            inputs,
        }
    })
}

/// Several columns of a table filled by one formula: given each row, or the
/// columns it reads whole.
///
/// A column filled alone declares its formula on its ``Property``; this is for
/// one computation that gives several columns at once, a fit's two parameters.
///
/// Args:
///     columns: The columns it fills: a name, or a list of names.
///     compute: A function given each row, a ``RowView``, and returning a dict:
///         one cell for each column it fills, a number or
///         ``(value, uncertainty)``.
///     compute_column: A function given the columns it reads, by name, and
///         returning a dict: one list of cells for each column it fills, one
///         cell per row.
///     depends_on: What the formula reads: ``"row.force"`` for a cell of the
///         same row, ``"curve.force"`` for a whole column, ``"area"`` for a
///         property of the sample; ``[]`` for nothing.
///
/// Raises:
///     ValueError: Neither formula is given, or both, or ``depends_on`` is not.
///     TypeError: A formula is not callable.
///
/// Example:
///     >>> bounds = sk.ComputedColumns(
///     ...     ["low", "high"], compute=lambda row: {"low": 1.0, "high": 2.0},
///     ...     depends_on=[],
///     ... )
#[pyclass(name = "ComputedColumns", module = "samplekit", unsendable)]
pub struct PyComputedColumns {
    columns: Vec<String>,
    compute: Option<Py<PyAny>>,
    compute_column: Option<Py<PyAny>>,
    depends_on: Vec<String>,
}

#[pymethods]
impl PyComputedColumns {
    #[new]
    #[pyo3(signature = (columns, *, compute=None, compute_column=None, depends_on=None))]
    fn new(
        columns: &Bound<'_, PyAny>,
        compute: Option<&Bound<'_, PyAny>>,
        compute_column: Option<&Bound<'_, PyAny>>,
        depends_on: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let columns = strings_from(columns, "columns")?;
        if columns.is_empty() {
            return Err(PyValueError::new_err(
                "ComputedColumns fills at least one column: name them first",
            ));
        }
        let (compute, compute_column) = (given(compute), given(compute_column));
        let formula = match (&compute, &compute_column) {
            (Some(formula), None) | (None, Some(formula)) => formula,
            (None, None) => {
                return Err(PyValueError::new_err(
                    "ComputedColumns computes its columns: compute= given each row, or \
                     compute_column= given the columns whole",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(PyValueError::new_err(
                    "ComputedColumns takes one formula: compute= given each row, or \
                     compute_column= given the columns whole, not both",
                ));
            }
        };
        if !formula.is_callable() {
            return Err(PyTypeError::new_err(format!(
                "a formula is a callable, and this is {}",
                bridge::a_type(formula)
            )));
        }
        let Some(depends_on) = given(depends_on) else {
            return Err(PyValueError::new_err(
                "ComputedColumns says what its formula reads: depends_on=[\"row.force\"], or \
                 depends_on=[] for nothing",
            ));
        };
        let depends_on = strings_from(&depends_on, "depends_on")?;
        for input in &depends_on {
            input_name(input)?;
        }
        Ok(PyComputedColumns {
            columns,
            compute: compute.map(|formula| formula.unbind()),
            compute_column: compute_column.map(|formula| formula.unbind()),
            depends_on,
        })
    }

    fn __repr__(&self) -> String {
        let role = if self.compute.is_some() {
            "compute"
        } else {
            "compute_column"
        };
        format!(
            "ComputedColumns({:?}, {role}=…, depends_on={:?})",
            self.columns, self.depends_on
        )
    }
}

/// A table of a sample: named columns, and rows identified by their index.
///
/// The index is the column — or the columns — whose values identify a row: a
/// day, a temperature, a inspector. A cell is read by its index value, never by
/// its position: ``table.at(0.2, "force")``.
///
/// A column is declared with ``Property``: its unit, the statistics of its
/// cells' readings, or the formula that computes each cell, given the row
/// (``compute``, ``compute_value``, ``compute_uncertainty``), or the whole
/// column at once (``compute_column``). A formula filling several columns at
/// once is a ``ComputedColumns``, given in ``computed``.
///
/// Args:
///     columns: A dict of ``Property`` by name, in the order they are shown.
///     index: The name of the index column, or a list of names; always named.
///     title: A title for the table.
///     rows: The first rows: each a dict of cells by column name.
///     computed: The ``ComputedColumns`` of the table, each filling several
///         columns.
///
/// Raises:
///     TypeError: A column is not a ``Property``, or is given a value; the
///         index is not named.
///     ValueError: The index names no column, a row does not fit the columns,
///         or a column is filled by two formulas.
///
/// Example:
///     >>> inspection = sk.Table(
///     ...     {"inspector": sk.Property(), "score": sk.Property()},
///     ...     index="inspector",
///     ...     rows=[{"inspector": "Ana", "score": 41}],
///     ... )
///     >>> inspection.at("Ana", "score").value
///     41
#[pyclass(name = "Table", module = "samplekit", unsendable)]
pub struct PyTable {
    state: RefCell<TableState>,
}

impl PyTable {
    fn attached(sample: SharedSample, file: FileRef, name: Identifier) -> PyTable {
        PyTable {
            state: RefCell::new(TableState::Attached { sample, file, name }),
        }
    }

    /// Asked without insisting: a row formula printing its table while that
    /// very table resolves found its state held, and `.borrow()` panicked —
    /// a `PanicException` out of a `print`. A table in the middle of computing
    /// says so instead of its name.
    fn name(&self) -> String {
        match self.state.try_borrow().as_deref() {
            Ok(TableState::Detached { table, .. }) => table.name().to_string(),
            Ok(TableState::Attached { name, .. }) => name.to_string(),
            Err(_) => "a table being computed".to_string(),
        }
    }

    fn config(&self) -> Option<Rc<ProjectConfig>> {
        match self.state.try_borrow().as_deref() {
            Ok(TableState::Attached { file, .. }) => file.borrow().config.clone(),
            _ => None,
        }
    }

    /// Reads the table, resolving its derivations first when asked: a derived
    /// cell is read as what its formula yields.
    fn read<T>(
        &self,
        py: Python<'_>,
        resolve: bool,
        read: impl FnOnce(&Table) -> PyResult<T>,
    ) -> PyResult<T> {
        // A table is resolved only within a computation.
        let resolve = resolve && crate::core::property::reads_compute();
        if resolve {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            // A table not yet assigned has no sample, so it resolves against
            // none, and runs again once a sample holds it.
            if let TableState::Detached { table, .. } = &mut *state {
                table.resolve(&Sample::new()).or_raise(py)?;
            }
        }
        let (sample, name) = {
            let state = self.state.try_borrow().map_err(|_| busy())?;
            match &*state {
                TableState::Detached { table, .. } => return read(table),
                TableState::Attached { sample, name, .. } => (sample.clone(), name.clone()),
            }
        };
        if resolve {
            // While tables resolve, a table a derivation reads was resolved
            // before its copy was taken.
            let settled = sample.read(
                |held| Ok(held.table(&name).is_ok_and(|table| table.is_settled(held))),
                |snapshot| Ok(snapshot.copies.contains_key(&name)),
            )?;
            if !settled {
                let before = bridge::formula_runs();
                resolve_with_upstream(py, &sample, &name, None)?;
                bridge::record_computed(&sample, before);
            }
        }
        sample.read_table(&name, read)
    }

    /// Where one cell stands: entered, never computed, outdated or current.
    /// What a cell's column formula reads that is edited, for its row.
    fn edited_upstream_of(
        &self,
        py: Python<'_>,
        index: &[Value],
        column: &Identifier,
    ) -> PyResult<Vec<String>> {
        let state = self.state.try_borrow().map_err(|_| busy())?;
        match &*state {
            TableState::Detached { .. } => Ok(Vec::new()),
            TableState::Attached { sample, name, .. } => {
                let _ = py;
                sample.read_sample("what a cell rests on", |held| {
                    Ok(fingerprint::edited_upstream_of_cell(
                        held, name, column, index,
                    ))
                })
            }
        }
    }

    fn cell_state(
        &self,
        py: Python<'_>,
        index: &[Value],
        column: &Identifier,
    ) -> PyResult<&'static str> {
        let state = self.state.try_borrow().map_err(|_| busy())?;
        match &*state {
            // A table no sample holds computes nothing, so a derived cell of it
            // is what it is: never computed, until a sample holding the table
            // computes; or entered, when the row supplied it.
            TableState::Detached { table, .. } => Ok(if !table.is_derived(column) {
                "entered"
            } else if table
                .at(&RowAddress::Index(index.to_vec()), column)
                .or_raise(py)?
                .peek_value()
                .is_none_or(|value| value.is_absent())
            {
                "never computed"
            } else {
                "current"
            }),
            TableState::Attached { sample, file, name } => {
                let word = sample.read_sample("a cell's state", |held| {
                    if !held.table(name).or_raise(py)?.is_derived(column) {
                        return Ok("entered");
                    }
                    let word = cell_reason(py, held, name, column, index, false, true)?
                        .unwrap_or("current");
                    // An input of its row, or a cell its column reads, emptied
                    // since: it waits, as `status` says.
                    if word == "outdated"
                        && let Ok(state) = fingerprint::check_cell(held, name, column, index)
                        && !crate::collection::editing::emptied_inputs(held, &state).is_empty()
                    {
                        return Ok("waiting");
                    }
                    Ok(word)
                })?;
                let planned = planned_of(sample, file);
                let said = planned.get(&format!("{name}.{column}"));
                // Its column's formula changed since it computed it: outdated,
                // as `status` says, though current by its inputs.
                if word == "current" && said.is_some_and(|said| said.contains("formula changed")) {
                    return Ok("outdated");
                }
                // Its column waits for an input nobody entered — a row added
                // without its hardness — as `status` and `compute` say.
                if matches!(word, "current" | "outdated" | "never computed")
                    && said.is_some_and(|said| said.starts_with("waits for"))
                {
                    return Ok("waiting");
                }
                Ok(word)
            }
        }
    }

    fn is_cell_stale(
        &self,
        py: Python<'_>,
        index: &[Value],
        column: &Identifier,
    ) -> PyResult<bool> {
        let state = self.state.try_borrow().map_err(|_| busy())?;
        match &*state {
            // A table no sample holds has recorded nothing to be stale against.
            TableState::Detached { .. } => Ok(false),
            TableState::Attached { sample, name, .. } => sample
                .read_sample("whether a cell is outdated", |held| {
                    fingerprint::is_cell_stale(held, name, column, index).or_raise(py)
                }),
        }
    }

    /// One derived cell, when it is outdated or never ran, or with `rerun`.
    fn compute_cell(
        &self,
        py: Python<'_>,
        index: &[Value],
        column: &Identifier,
        rerun: bool,
        force: bool,
    ) -> PyResult<()> {
        let held = {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            match &mut *state {
                TableState::Detached { table, .. } => {
                    if !table.is_derived(column) {
                        return Err(no_formula(&format!("the column '{column}'")));
                    }
                    // A detached table computes a cell never run when it is
                    // read; running a current one again is asked for.
                    if rerun {
                        table
                            .invalidate_row(&RowAddress::Index(index.to_vec()))
                            .or_raise(py)?;
                    }
                    None
                }
                TableState::Attached { sample, name, .. } => Some((sample.clone(), name.clone())),
            }
        };
        match held {
            None => self.read(py, true, |_| Ok(())),
            Some((sample, name)) => {
                let due = sample.read_sample("the table", |held| {
                    if !held.table(&name).or_raise(py)?.is_derived(column) {
                        return Err(no_formula(&format!("'{name}.{column}'")));
                    }
                    cell_reason(py, held, &name, column, index, rerun, force)
                })?;
                if due.is_none() {
                    return Ok(());
                }
                compute_now(
                    py,
                    &sample,
                    &[Target::Cell {
                        table: name,
                        index: index.to_vec(),
                        column: column.clone(),
                    }],
                )
            }
        }
    }

    fn write<T>(&self, work: impl FnOnce(TableTarget<'_>) -> PyResult<T>) -> PyResult<T> {
        let (sample, name) = {
            let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
            match &mut *state {
                TableState::Detached { table, .. } => return work(TableTarget::Detached(table)),
                TableState::Attached { sample, name, .. } => (sample.clone(), name.clone()),
            }
        };
        sample.touch(name.as_str());
        sample.write(|held| {
            work(TableTarget::Held {
                sample: held,
                name: &name,
            })
        })
    }

    fn with_cell<T>(
        &self,
        py: Python<'_>,
        resolve: bool,
        index: &[Value],
        column: &Identifier,
        read: impl FnOnce(&Property) -> PyResult<T>,
    ) -> PyResult<T> {
        self.read(py, resolve, |table| {
            read(
                table
                    .at(&RowAddress::Index(index.to_vec()), column)
                    .or_raise(py)?,
            )
        })
    }

    fn edit_cell(
        &self,
        py: Python<'_>,
        index: &[Value],
        column: &Identifier,
        change: Change,
    ) -> PyResult<()> {
        let address = RowAddress::Index(index.to_vec());
        if let Change::Invalidate = change {
            return self.write(|target| target.invalidate_cell(py, &address, column));
        }
        let replacement = self.read(py, false, |table| {
            let cell = table.at(&address, column).or_raise(py)?;
            let mut copy = copy_of(cell).or_raise(py)?;
            // A value written beside an uncertainty its formula gives keeps
            // that formula's record: the uncertainty is then outdated, never
            // overridden.
            let beside = matches!(change, Change::Value(_))
                && table.produced_by(column) == Some(Produced::Uncertainty)
                && cell.records().computed.is_some();
            // A hand on the very channel a formula of one half gives is an
            // override, as a whole cell written by hand is.
            let overrides = match (table.produced_by(column), &change) {
                (Some(Produced::Uncertainty), Change::Uncertainty(Some(_)))
                | (Some(Produced::Value), Change::Value(_)) => true,
                // A whole cell a formula gives, its value typed.
                (None, Change::Value(_)) => table.is_derived(column),
                _ => false,
            };
            let kept = cell.records().clone();
            change_property(&mut copy, change);
            if beside {
                copy.set_records(kept);
            } else if overrides && kept.computed.is_some() {
                // The formula's record kept, its own digest marked edited, as
                // the command line's `set` writes it: cleared, the cell read
                // *record missing*.
                let produced = kept.produced;
                let digest = crate::format::fingerprint::own_of(
                    &crate::format::schema::property_as_is(&copy),
                    produced,
                )
                .marked_edited();
                copy.set_records(crate::core::property::Records {
                    fingerprint: Some(digest),
                    failure: None,
                    ..kept
                });
            } else if overrides {
                copy.set_records(crate::core::property::Records::none());
            }
            Ok(copy)
        })?;
        self.write(|target| target.update_row(py, &address, vec![(column.clone(), replacement)]))
    }

    fn column_view(
        slf: &Bound<'_, Self>,
        name: &str,
        missing: fn(String) -> PyErr,
    ) -> PyResult<PyColumnView> {
        let py = slf.py();
        let this = slf.borrow();
        let names: Vec<String> = this.read(py, false, |table| {
            Ok(table
                .column_names()
                .iter()
                .map(|name| name.to_string())
                .collect())
        })?;
        if !names.iter().any(|known| known == name) {
            return Err(missing(unknown_among(
                "column",
                name,
                &names,
                &members_of(slf.as_any()),
            )));
        }
        Ok(PyColumnView {
            source: ColumnSource::Live {
                table: slf.clone().unbind(),
                column: bridge::identifier_from(py, name)?,
            },
        })
    }
}

#[pymethods]
impl PyTable {
    #[new]
    #[pyo3(signature = (columns, *positional, index=None, title=None, rows=None, computed=None))]
    #[allow(clippy::too_many_arguments)] // The keyword surface python-api specifies.
    fn new(
        py: Python<'_>,
        columns: &Bound<'_, PyAny>,
        positional: &Bound<'_, PyTuple>,
        index: Option<&Bound<'_, PyAny>>,
        title: Option<String>,
        rows: Option<&Bound<'_, PyAny>>,
        computed: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        // The index is named: a bare second argument did not say what it was,
        // and an index of several columns has no place to stand.
        if let Ok(given) = positional.get_item(0) {
            return Err(PyTypeError::new_err(format!(
                "a table names its index: index={}, the column whose values name its rows \
                 (index=[\"day\", \"hour\"] for several)",
                given
                    .repr()
                    .map(|repr| repr.to_string())
                    .unwrap_or_default()
            )));
        }
        let Some(index) = index.filter(|index| !index.is_none()) else {
            return Err(PyTypeError::new_err(
                "a table names its index, the column whose values name its rows: \
                 index=\"day\", or index=[\"day\", \"hour\"] for several",
            ));
        };
        let text = |text: String| PyString::new(py, &text).into_any().unbind();
        let mut declared = IndexMap::new();
        let mut derivations: Vec<Derivation> = Vec::new();
        let mut code: Vec<(String, FormulaCode)> = Vec::new();
        for item in columns.call_method0("items")?.try_iter()? {
            let pair = item?;
            let name: String = pair.get_item(0)?.extract()?;
            let column = pair.get_item(1)?;
            let not_a_property = || {
                PyTypeError::new_err(format!(
                    "the column '{name}' is declared with sk.Property(...), its unit, the \
                     statistics of its cells' readings or its formulas, and this is {}",
                    bridge::a_type(&column)
                ))
            };
            let property = column.cast::<PyProperty>().map_err(|_| not_a_property())?;
            let property = property.borrow();
            let held = property.as_column.borrow();
            let Some(given) = held.as_ref() else {
                return Err(not_a_property());
            };
            if let Some(what) = given.entered {
                return Err(PyTypeError::new_err(format!(
                    "the column '{name}' is given {what}, and each cell holds its own, in the \
                     file: a column declares a unit, the statistics of its cells' readings, or \
                     its formulas"
                )));
            }
            let identifier = bridge::identifier_from(py, &name)?;
            let unit = given.unit.clone();
            let formulas: Vec<(&str, &Py<PyAny>, Half)> = [
                ("compute", &given.compute, Half::Whole),
                ("compute_value", &given.compute_value, Half::Value),
                (
                    "compute_uncertainty",
                    &given.compute_uncertainty,
                    Half::Uncertainty,
                ),
                ("compute_column", &given.compute_column, Half::Whole),
            ]
            .into_iter()
            .filter_map(|(role, formula, half)| {
                formula.as_ref().map(|formula| (role, formula, half))
            })
            .collect();
            if let Some((role, formula, half)) = formulas.first() {
                let Some(inputs) = given.depends_on.as_ref() else {
                    return Err(PyValueError::new_err(format!(
                        "the column '{name}' says what its formula reads: depends_on=[\"row.force\", \
                         \"area\"], or depends_on=[] for nothing"
                    )));
                };
                let inputs = column_inputs(inputs.bind(py))?;
                // A cell's uncertainty may read the value beside it, as a
                // property's does, by that channel's name: `row.force.v`.
                let own = format!("row.{name}");
                let own_value = format!("row.{name}.v");
                if inputs.contains(&own) {
                    return Err(PyValueError::new_err(format!(
                        "the column '{name}' declares that it reads itself, and a cell cannot \
                         rest on its own result\n  an uncertainty may read the value beside it \
                         — compute_uncertainty=…, depends_on=[\"{own_value}\"]"
                    )));
                }
                // Only an uncertainty's formula reads the value beside it.
                let value_reads_itself = given
                    .depends_on
                    .as_ref()
                    .and_then(|given| given.bind(py).cast::<PyDict>().ok().cloned())
                    .and_then(|channels| channels.get_item("v").ok().flatten())
                    .map(|listed| strings_from(&listed, "depends_on"))
                    .transpose()?
                    .is_some_and(|listed| listed.contains(&own_value));
                let reads_own_value = inputs.contains(&own_value);
                let uncertainty_formula = formulas
                    .iter()
                    .any(|(_, _, half)| matches!(half, Half::Uncertainty));
                if reads_own_value && (value_reads_itself || !uncertainty_formula) {
                    return Err(PyValueError::new_err(format!(
                        "the column '{name}' reads {own_value}, the value beside its uncertainty: \
                         only a formula of compute_uncertainty= reads it"
                    )));
                }
                if let [_, (second_role, second, _)] = formulas.as_slice() {
                    // Its value's formula, then its uncertainty's, on each
                    // row: the uncertainty is given the value just computed,
                    // which it reads within the run, no input of it.
                    let inputs_text = inputs.join(", ");
                    let inputs: Vec<InputName> = inputs
                        .iter()
                        .filter(|text| **text != own_value)
                        .map(|text| input_name(text))
                        .collect::<PyResult<_>>()?;
                    derivations.push(Derivation::Row {
                        formula: Rc::new(bridge::PythonHalvesCompute {
                            value: bridge::Formula::held(formula.bind(py).clone())?,
                            uncertainty: bridge::Formula::held(second.bind(py).clone())?,
                            output: identifier.clone(),
                        }),
                        outputs: vec![identifier.clone()],
                        inputs,
                    });
                    let mut parts = vec![
                        (role.to_string(), function_of(formula.bind(py))),
                        (second_role.to_string(), function_of(second.bind(py))),
                    ];
                    parts.push(("inputs".to_string(), text(inputs_text)));
                    parts.push(("outputs".to_string(), text(name.clone())));
                    if let Some(unit) = unit.clone() {
                        parts.push(("unit".to_string(), text(unit)));
                    }
                    code.push((name.clone(), parts));
                } else {
                    let whole = *role == "compute_column";
                    derivations.push(table_derivation(
                        py,
                        vec![identifier.clone()],
                        &inputs,
                        formula.bind(py),
                        whole,
                        *half,
                    )?);
                    let mut parts = vec![
                        (role.to_string(), function_of(formula.bind(py))),
                        ("inputs".to_string(), text(inputs.join(", "))),
                        ("outputs".to_string(), text(name.clone())),
                    ];
                    if let Some(unit) = unit.clone() {
                        parts.push(("unit".to_string(), text(unit)));
                    }
                    code.push((name.clone(), parts));
                }
            } else if given.depends_on.is_some() {
                return Err(PyValueError::new_err(format!(
                    "the column '{name}' says what a formula reads and has no formula: pass \
                     compute=, compute_value=, compute_uncertainty= or compute_column="
                )));
            }
            declared.insert(
                identifier,
                ColumnMeta {
                    presentation: Presentation {
                        unit,
                        symbol: None,
                        precision: None,
                    },
                    statistics: given.statistics,
                },
            );
        }
        let index: Vec<Identifier> = strings_from(index, "index columns")?
            .iter()
            .map(|name| bridge::identifier_from(py, name))
            .collect::<PyResult<_>>()?;
        // Several columns filled by one formula, each part named.
        if let Some(computed) = computed.filter(|computed| !computed.is_none()) {
            for item in computed.try_iter()? {
                let item = item?;
                let formula = item.cast::<PyComputedColumns>().map_err(|_| {
                    PyTypeError::new_err(format!(
                        "computed= holds sk.ComputedColumns(...), and this is {}",
                        bridge::a_type(&item)
                    ))
                })?;
                let formula = formula.borrow();
                let outputs: Vec<Identifier> = formula
                    .columns
                    .iter()
                    .map(|name| bridge::identifier_from(py, name))
                    .collect::<PyResult<_>>()?;
                let (role, callable, whole) = match (&formula.compute, &formula.compute_column) {
                    (Some(callable), None) => ("compute", callable, false),
                    (None, Some(callable)) => ("compute_column", callable, true),
                    _ => unreachable!("ComputedColumns holds exactly one formula"),
                };
                derivations.push(table_derivation(
                    py,
                    outputs,
                    &formula.depends_on,
                    callable.bind(py),
                    whole,
                    Half::Whole,
                )?);
                for output in &formula.columns {
                    let mut parts = vec![
                        (role.to_string(), function_of(callable.bind(py))),
                        ("inputs".to_string(), text(formula.depends_on.join(", "))),
                        ("outputs".to_string(), text(formula.columns.join(", "))),
                    ];
                    let unit = Identifier::new(output)
                        .ok()
                        .and_then(|name| declared.get(&name))
                        .and_then(|meta: &ColumnMeta| meta.presentation.unit.clone());
                    if let Some(unit) = unit {
                        parts.push(("unit".to_string(), text(unit)));
                    }
                    code.push((output.clone(), parts));
                }
            }
        }
        // One formula a column: a second one for it is refused, as two for a
        // property's value are.
        let mut filled: Vec<&Identifier> = Vec::new();
        for derivation in &derivations {
            let outputs = match derivation {
                Derivation::Row { outputs, .. } | Derivation::Column { outputs, .. } => outputs,
            };
            for output in outputs {
                if filled.contains(&output) {
                    return Err(PyValueError::new_err(format!(
                        "the column '{output}' is filled by two formulas, its own and a \
                         ComputedColumns, or two of those: keep one"
                    )));
                }
                filled.push(output);
            }
        }
        // A column declaring the statistics of its cells gives their values as
        // a formula does: its digest is taken from them, so a statistic changed
        // stales the cells.
        for (column, meta) in &declared {
            let statistics = &meta.statistics;
            if statistics.is_empty() {
                continue;
            }
            let mut parts = Vec::new();
            if let Some(location) = statistics.value {
                parts.push((
                    "value".to_string(),
                    text(format!("sk.stats.{}", location.name())),
                ));
            }
            if let Some(convention) = statistics.uncertainty {
                parts.push((
                    "uncertainty".to_string(),
                    text(format!("sk.stats.{}", convention.name())),
                ));
            }
            if let Some(unit) = meta.presentation.unit.clone() {
                parts.push(("unit".to_string(), text(unit)));
            }
            if !code.iter().any(|(name, _)| name == column.as_str()) {
                code.push((column.to_string(), parts));
            }
        }
        let qualifiers = qualifiers_of(&derivations, &declared);
        // With one candidate the table is checked under it now; with several,
        // where it is assigned, once its name settles which is its own.
        let provisional = match qualifiers.as_slice() {
            [only] => only.clone(),
            _ => Identifier::new("table").expect("a plain name"),
        };
        let mut table = Table::new(provisional, index, declared, derivations).or_raise(py)?;
        table.set_title(title);
        if let Some(rows) = rows.filter(|rows| !rows.is_none()) {
            for row in rows.try_iter()? {
                table.add_row(cells_from(&row?)?).or_raise(py)?;
            }
        }
        Ok(PyTable {
            state: RefCell::new(TableState::Detached {
                table: Box::new(table),
                qualifiers,
                code,
            }),
        })
    }

    /// Add a row at the end.
    ///
    /// Args:
    ///     **cells: The row's cells by column name: a value, a list of readings,
    ///         or a ``Property``. The index columns are required.
    ///
    /// Raises:
    ///     KeyError: A column the table does not have.
    ///     ValueError: A row with that index exists already.
    #[pyo3(signature = (**cells))]
    fn add(&self, py: Python<'_>, cells: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        let cells = match cells {
            Some(cells) => cells_from(cells.as_any())?,
            None => Vec::new(),
        };
        self.write(|target| target.add_row(py, cells))
    }

    /// Add several rows at the end.
    ///
    /// Args:
    ///     rows: An iterable of dicts, each a row's cells by column name.
    ///
    /// Raises:
    ///     KeyError: A column the table does not have.
    ///     ValueError: A row with that index exists already.
    fn extend(&self, py: Python<'_>, rows: &Bound<'_, PyAny>) -> PyResult<()> {
        for row in rows.try_iter()? {
            let cells = cells_from(&row?)?;
            self.write(|target| target.add_row(py, cells))?;
        }
        Ok(())
    }

    /// Change cells of one row.
    ///
    /// Values computed from those cells become outdated. A computed cell changed
    /// here is edited: kept by ``compute()`` until ``compute(force=True)``.
    ///
    /// Args:
    ///     index: The row's index value; a tuple for an index of several columns.
    ///     **cells: The new cells, by column name.
    ///
    /// Raises:
    ///     KeyError: No row has that index, or a column is unknown; the message
    ///         names the nearest.
    #[pyo3(signature = (index, **cells))]
    fn update(
        &self,
        py: Python<'_>,
        index: &Bound<'_, PyAny>,
        cells: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let address = RowAddress::Index(index_from(index)?);
        let cells = match cells {
            Some(cells) => cells_from(cells.as_any())?,
            None => Vec::new(),
        };
        // A number given to a cell whose uncertainty a formula gives is its
        // value alone: the uncertainty beside it stays, with its record, as
        // `cell.value = …` and the command line's `set` leave it.
        let cells = self.read(py, false, |table| {
            cells
                .into_iter()
                .map(|(column, cell)| {
                    let bare =
                        cell.peek_uncertainty().flatten().is_none() && cell.readings().is_none();
                    if bare
                        && table.produced_by(&column) == Some(Produced::Uncertainty)
                        && let Ok(existing) = table.at(&address, &column)
                    {
                        let kept = existing.records().clone();
                        let mut copy = copy_of(existing).or_raise(py)?;
                        copy.set_value(cell.peek_value().unwrap_or_else(Value::absent));
                        copy.set_records(kept);
                        return Ok((column, copy));
                    }
                    // A cell a formula gives, written by hand: an override,
                    // its record kept and marked edited, as `set` writes it.
                    let mut cell = cell;
                    if table.is_derived(&column)
                        && table.produced_by(&column) != Some(Produced::Uncertainty)
                        && cell.records().computed.is_none()
                        && let Ok(existing) = table.at(&address, &column)
                        && existing.records().computed.is_some()
                    {
                        let kept = existing.records().clone();
                        let digest = crate::format::fingerprint::own_of(
                            &crate::format::schema::property_as_is(&cell),
                            kept.produced,
                        )
                        .marked_edited();
                        cell.set_records(crate::core::property::Records {
                            fingerprint: Some(digest),
                            failure: None,
                            ..kept
                        });
                    }
                    Ok((column, cell))
                })
                .collect::<PyResult<Vec<_>>>()
        })?;
        self.write(|target| target.update_row(py, &address, cells))
    }

    /// The cell of a row, by the row's index value.
    ///
    /// Args:
    ///     index: The row's index value; a tuple for an index of several columns.
    ///     column: The column's name.
    ///
    /// Returns:
    ///     The cell, as a ``Property``.
    ///
    /// Raises:
    ///     KeyError: No row has that index, or the column is unknown; the message
    ///         names the nearest.
    ///
    /// Example:
    ///     >>> steel.curve.at(0.2, "force").value
    ///     9.516
    fn at(slf: &Bound<'_, Self>, index: &Bound<'_, PyAny>, column: &str) -> PyResult<PyProperty> {
        let py = slf.py();
        let this = slf.borrow();
        let wanted = RowAddress::Index(index_from(index)?);
        let column = bridge::identifier_from(py, column)?;
        let found: Vec<Value> = this.read(py, false, |table| {
            table.at(&wanted, &column).or_raise(py)?;
            Ok(table
                .row(&wanted)
                .or_raise(py)?
                .index()
                .into_iter()
                .cloned()
                .collect())
        })?;
        Ok(PyProperty::cell(slf.clone().unbind(), found, column))
    }

    /// The values of a column, in row order.
    ///
    /// Args:
    ///     column: The column's name.
    ///
    /// Returns:
    ///     A list of values, ``None`` where a cell has none.
    ///
    /// Raises:
    ///     KeyError: The column is unknown; the message lists the columns.
    ///
    /// Example:
    ///     >>> steel.curve.values("force")[:3]
    ///     [0.0, 1.281, 2.585]
    fn values(&self, py: Python<'_>, column: &str) -> PyResult<Vec<Py<PyAny>>> {
        let column = bridge::identifier_from(py, column)?;
        let read = self.read(py, true, |table| column_read(py, table, &column))?;
        said_column(py, read)
    }

    /// The uncertainties of a column, in row order.
    ///
    /// Args:
    ///     column: The column's name.
    ///
    /// Returns:
    ///     A list of uncertainties, ``None`` where a cell has none.
    ///
    /// Raises:
    ///     KeyError: The column is unknown; the message lists the columns.
    fn uncertainties(&self, py: Python<'_>, column: &str) -> PyResult<Vec<Option<f64>>> {
        let column = bridge::identifier_from(py, column)?;
        self.read(py, true, |table| {
            table
                .column(&column)
                .or_raise(py)?
                .cells()
                .map(|(_, cell)| {
                    Ok(cell
                        .uncertainty()
                        .or_raise(py)?
                        .map(|uncertainty| uncertainty.magnitude()))
                })
                .collect()
        })
    }

    /// The names of the columns, in the order they were declared.
    #[getter]
    fn column_names(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.read(py, false, |table| {
            Ok(table
                .column_names()
                .iter()
                .map(|name| name.to_string())
                .collect())
        })
    }

    /// The index value of each row, in row order; a tuple each for an index of
    /// several columns.
    #[getter]
    fn index_values(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        self.read(py, false, |table| {
            table
                .index_tuples()
                .iter()
                .map(|tuple| {
                    let values: Vec<Value> = tuple.iter().map(|value| (*value).clone()).collect();
                    index_to_python(py, &values)
                })
                .collect()
        })
    }

    /// Whether a formula gives `column` row by row. The worker's, to say
    /// what a column read whole waits for.
    fn _derived_by_rows(&self, py: Python<'_>, column: &str) -> PyResult<bool> {
        let column = Identifier::new(column).or_raise(py)?;
        self.read(py, false, |table| Ok(table.is_derived_by_rows(&column)))
    }

    /// The names of the columns that are not the index.
    #[getter]
    fn data_columns(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        self.read(py, false, |table| {
            let index = table.index_columns();
            Ok(table
                .column_names()
                .into_iter()
                .filter(|name| !index.contains(name))
                .map(|name| name.to_string())
                .collect())
        })
    }

    /// The name of the index column, or a list of names for an index of several
    /// columns.
    #[getter]
    fn index(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.read(py, false, |table| {
            let names: Vec<String> = table
                .index_columns()
                .iter()
                .map(|name| name.to_string())
                .collect();
            if names.len() == 1 {
                names[0].clone().into_py_any(py)
            } else {
                names.into_py_any(py)
            }
        })
    }

    /// The unit of the index column, or a list of units for an index of several
    /// columns.
    #[getter]
    fn index_unit(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.read(py, false, |table| {
            let units: Vec<Option<String>> = table
                .index_columns()
                .iter()
                .map(|name| {
                    table
                        .column(name)
                        .map(|view| view.presentation().unit.clone())
                        .or_raise(py)
                })
                .collect::<PyResult<_>>()?;
            if units.len() == 1 {
                units[0].clone().into_py_any(py)
            } else {
                units.into_py_any(py)
            }
        })
    }

    /// The table's title, or ``None``.
    #[getter]
    fn title(&self, py: Python<'_>) -> PyResult<Option<String>> {
        self.read(py, false, |table| Ok(table.title().map(str::to_string)))
    }

    fn __getitem__(slf: &Bound<'_, Self>, position: &Bound<'_, PyAny>) -> PyResult<PyRowView> {
        // A column is read by name through values() or a cell through at(), never
        // as a table's item.
        if let Ok(name) = position.extract::<String>() {
            return Err(PyTypeError::new_err(format!(
                "a table's item is a row by position, as in table[0]; the column '{name}' is \
                 table.values('{name}'), and one of its cells table.at(index, '{name}')"
            )));
        }
        let position: isize = position.extract().map_err(|_| {
            PyTypeError::new_err(format!(
                "a table's item is a row by position, an int, and this is {}",
                bridge::a_type(position)
            ))
        })?;
        let py = slf.py();
        let this = slf.borrow();
        let (index, ordinal, columns) = this.read(py, false, |table| {
            let length = table.index_tuples().len() as isize;
            let ordinal = if position < 0 {
                position + length
            } else {
                position
            };
            if ordinal < 0 || ordinal >= length {
                return Err(PyIndexError::new_err(format!(
                    "there is no row {position}: the table has {length}"
                )));
            }
            let row = table
                .row(&RowAddress::Ordinal(ordinal as usize))
                .or_raise(py)?;
            Ok((
                row.index().into_iter().cloned().collect::<Vec<_>>(),
                ordinal as usize,
                table
                    .column_names()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        })?;
        Ok(PyRowView {
            source: RowSource::Live {
                table: slf.clone().unbind(),
                index,
                position: ordinal,
                columns,
            },
        })
    }

    fn __getattr__(slf: &Bound<'_, Self>, name: &str) -> PyResult<PyColumnView> {
        // The message carries the nearest name already: Python adding its
        // own wrote the suggestion twice.
        (|| {
            if name.starts_with('_') {
                return Err(PyAttributeError::new_err(format!(
                    "'Table' object has no attribute '{name}'"
                )));
            }
            PyTable::column_view(slf, name, PyAttributeError::new_err)
        })()
        .map_err(bridge::said_in_full)
    }

    fn __contains__(&self, py: Python<'_>, index: &Bound<'_, PyAny>) -> PyResult<bool> {
        let wanted = RowAddress::Index(index_from(index)?);
        self.read(py, false, |table| Ok(table.row(&wanted).is_ok()))
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        self.read(py, false, |table| Ok(table.index_tuples().len()))
    }

    fn __iter__<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyIterator>> {
        let py = slf.py();
        let length = slf.borrow().__len__(py)?;
        let mut rows = Vec::with_capacity(length);
        for position in 0..length {
            rows.push(Py::new(
                py,
                PyTable::__getitem__(slf, (position as isize).into_pyobject(py)?.as_any())?,
            )?);
        }
        PyList::new(py, rows)?.try_iter()
    }

    /// The table as ``samplekit view`` unfolds it: its columns headed with
    /// their units, its index first, each cell in a style's forms — those of
    /// its sample's project, or of the project the current folder is in for a
    /// table of no sample.
    ///
    /// Args:
    ///     style: A style's name: ``"plain"``, one SampleKit gives, or one
    ///         ``.samplekitrc`` declares. Without it, ``[render] style``'s.
    ///
    /// Returns:
    ///     The text.
    ///
    /// Raises:
    ///     ValueError: No style has that name.
    ///
    /// Example:
    ///     >>> print(steel.curve.to_text())
    #[pyo3(signature = (style=None))]
    fn to_text(&self, py: Python<'_>, style: Option<&str>) -> PyResult<String> {
        // A table of no sample is written as the project here writes one.
        let config = match self.config() {
            Some(config) => Some(config),
            None => config_for(py, Path::new("table.md"))?,
        };
        style_known(config.as_deref(), style)?;
        let attached = match &*self.state.try_borrow().map_err(|_| busy())? {
            TableState::Attached { sample, file, name } => {
                Some((sample.clone(), file.borrow().path.clone(), name.clone()))
            }
            TableState::Detached { .. } => None,
        };
        if let Some((sample, path, name)) = attached {
            return sample.read_sample("the table", |sample| {
                Ok(sheet_of(
                    sample,
                    path.as_deref(),
                    config.as_deref(),
                    style,
                    Some(&name),
                ))
            });
        }
        // A table of no sample is lent to one made for it, and taken back.
        let mut state = self.state.try_borrow_mut().map_err(|_| busy())?;
        let TableState::Detached { table, .. } = &mut *state else {
            return Err(busy());
        };
        let name = table.name().clone();
        // Lent only where the loan cannot fail: a sample taking a table
        // refuses it for its own formulas alone, and a table refused would
        // be lost.
        crate::core::dependency_graph::DependencyGraph::new()
            .declare_all(&table.declarations())
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let key = Identifier::new("key").expect("a name");
        let placeholder = crate::core::table::Table::new(
            name.clone(),
            vec![key.clone()],
            IndexMap::from([(key, crate::core::table::ColumnMeta::default())]),
            Vec::new(),
        )
        .or_raise(py)?;
        let lent = std::mem::replace(table.as_mut(), placeholder);
        let mut holder = Sample::new();
        holder.set_table(name.clone(), lent).or_raise(py)?;
        let shown = sheet_of(&holder, None, config.as_deref(), style, Some(&name));
        if let Ok(back) = holder.remove_table(&name) {
            *table.as_mut() = back;
        }
        Ok(shown)
    }

    /// The table as rows of CSV, as ``samplekit FILE view TABLE --csv``
    /// prints it: its index first, each column a value and its uncertainty
    /// headed with the unit, at the precision its columns declare.
    ///
    /// Returns:
    ///     The text.
    ///
    /// Raises:
    ///     ValueError: The table belongs to no sample yet.
    ///
    /// Example:
    ///     >>> text = steel.curve.to_csv()
    fn to_csv(&self, py: Python<'_>) -> PyResult<String> {
        let (sample, path, name) = match &*self.state.try_borrow().map_err(|_| busy())? {
            TableState::Attached { sample, file, name } => (
                sample.inner.clone(),
                file.borrow().path.clone(),
                name.clone(),
            ),
            TableState::Detached { .. } => {
                return Err(PyValueError::new_err(
                    "a table of no sample has no rows to write: give it to a sample first",
                ));
            }
        };
        let config = self.config();
        let list = crate::collection::sample_list::from_entries(vec![
            crate::collection::sample_list::Entry {
                path,
                sample,
                states: None,
            },
        ]);
        let (dataset, _) = crate::collection::exports::table_rows(
            &name,
            &list,
            config.as_deref(),
            &Format::Csv,
            None,
            None,
        )
        .map_err(|error| PyValueError::new_err(error.to_string()))?;
        let (trimmed, empty) = export_formats::without_empty_uncertainties(&dataset);
        for header in empty {
            warn_value(
                py,
                &format!("{header} is left out: it is empty in every row"),
            )?;
        }
        export_formats::serialize(&trimmed, Format::Csv)
            .map_err(|error| PyValueError::new_err(error.to_string()))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let name = self.name();
        self.read(py, false, |table| {
            let columns: Vec<String> = table.column_names().iter().map(|c| c.to_string()).collect();
            Ok(format!(
                "Table('{name}', columns=[{}], rows={})",
                columns.join(", "),
                table.index_tuples().len()
            ))
        })
    }
}

// ----------------------------------------------------------- RowView

enum RowSource {
    Live {
        table: Py<PyTable>,
        index: Vec<Value>,
        position: usize,
        columns: Vec<Identifier>,
    },
    /// What a row formula receives: copies, read while the table is written.
    Snapshot {
        cells: IndexMap<Identifier, Py<PyProperty>>,
        position: usize,
    },
}

/// One row of a table, as a column's formula receives it.
///
/// A cell is read by its column's name, as an attribute or an item:
/// ``row.force`` or ``row["force"]``, each a ``Property``.
#[pyclass(name = "RowView", module = "samplekit", unsendable)]
pub struct PyRowView {
    source: RowSource,
}

/// Gives a row a formula received the value one of its cells was just
/// computed to, for the formula that runs after it on the same row.
pub fn set_snapshot_value(
    py: Python<'_>,
    view: &Py<PyRowView>,
    column: &Identifier,
    value: Value,
) -> PyResult<()> {
    let view = view.borrow(py);
    if let RowSource::Snapshot { cells, .. } = &view.source
        && let Some(cell) = cells.get(column)
    {
        cell.borrow(py).apply(py, Change::Value(value))?;
    }
    Ok(())
}

/// The row a row formula receives.
pub fn row_snapshot(py: Python<'_>, row: &RowView) -> PyResult<Py<PyRowView>> {
    let mut cells = IndexMap::new();
    for column in row.column_names() {
        let cell = row.cell(column).or_raise(py)?;
        cells.insert(
            column.clone(),
            Py::new(py, PyProperty::detached(copy_of(cell).or_raise(py)?))?,
        );
    }
    Py::new(
        py,
        PyRowView {
            source: RowSource::Snapshot {
                cells,
                position: row.position(),
            },
        },
    )
}

impl PyRowView {
    fn names(&self) -> Vec<String> {
        match &self.source {
            RowSource::Live { columns, .. } => columns.iter().map(|c| c.to_string()).collect(),
            RowSource::Snapshot { cells, .. } => cells.keys().map(|c| c.to_string()).collect(),
        }
    }

    fn cell(
        &self,
        py: Python<'_>,
        name: &str,
        missing: fn(String) -> PyErr,
        members: &[String],
    ) -> PyResult<Py<PyAny>> {
        let names = self.names();
        if !names.iter().any(|known| known == name) {
            return Err(missing(unknown_among("column", name, &names, members)));
        }
        let column = bridge::identifier_from(py, name)?;
        match &self.source {
            RowSource::Live { table, index, .. } => Ok(Py::new(
                py,
                PyProperty::cell(table.clone_ref(py), index.clone(), column),
            )?
            .into_any()),
            RowSource::Snapshot { cells, .. } => Ok(cells[&column].clone_ref(py).into_any()),
        }
    }

    fn as_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        for name in self.names() {
            dict.set_item(
                &name,
                self.cell(py, &name, bridge::key_error::<String>, &[])?,
            )?;
        }
        Ok(dict)
    }
}

#[pymethods]
impl PyRowView {
    /// The row's position in the table, from 0.
    #[getter]
    fn position(&self) -> usize {
        match &self.source {
            RowSource::Live { position, .. } | RowSource::Snapshot { position, .. } => *position,
        }
    }

    fn __getitem__(&self, py: Python<'_>, column: &str) -> PyResult<Py<PyAny>> {
        self.cell(py, column, bridge::key_error::<String>, &[])
    }

    fn __getattr__(slf: &Bound<'_, Self>, column: &str) -> PyResult<Py<PyAny>> {
        // The message carries the nearest name already: Python adding its
        // own wrote the suggestion twice.
        (|| {
            if column.starts_with('_') {
                return Err(PyAttributeError::new_err(format!(
                    "'RowView' object has no attribute '{column}'"
                )));
            }
            slf.borrow().cell(
                slf.py(),
                column,
                PyAttributeError::new_err,
                &members_of(slf.as_any()),
            )
        })()
        .map_err(bridge::said_in_full)
    }

    fn __contains__(&self, column: &Bound<'_, PyAny>) -> bool {
        column
            .extract::<String>()
            .is_ok_and(|column| self.names().contains(&column))
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.names())?.try_iter()
    }

    /// The names of the row's columns.
    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.as_dict(py)?.call_method0("keys")
    }

    /// Each column name with its cell.
    fn items<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        self.as_dict(py)?.call_method0("items")
    }

    fn __repr__(&self) -> String {
        format!(
            "RowView(position={}, columns=[{}])",
            self.position(),
            self.names().join(", ")
        )
    }
}

// --------------------------------------------------------- ColumnView

enum ColumnSource {
    Live {
        table: Py<PyTable>,
        column: Identifier,
    },
    /// What a column formula receives: copies, with the index of each cell.
    Snapshot {
        column: Identifier,
        cells: Vec<(Vec<Value>, Py<PyProperty>)>,
    },
}

/// One column of a table, as a ``compute_column`` formula receives it, and
/// as ``table.column_name`` reads it.
///
/// Iterating gives each cell as a ``Property``, in row order, and an item is
/// the cell of a row, by its index value: ``column[3]``.
#[pyclass(name = "ColumnView", module = "samplekit", unsendable)]
pub struct PyColumnView {
    source: ColumnSource,
}

/// The columns a column formula declared, as a read-only mapping.
pub fn column_snapshots<'py>(
    py: Python<'py>,
    set: &ColumnSet,
    names: &[Identifier],
) -> PyResult<Bound<'py, PyAny>> {
    let dict = PyDict::new(py);
    for name in names {
        let view = set.column(name).or_raise(py)?;
        let mut cells = Vec::new();
        for (index, cell) in view.cells() {
            cells.push((
                index.into_iter().cloned().collect(),
                Py::new(py, PyProperty::detached(copy_of(cell).or_raise(py)?))?,
            ));
        }
        let snapshot = PyColumnView {
            source: ColumnSource::Snapshot {
                column: name.clone(),
                cells,
            },
        };
        dict.set_item(name.as_str(), Py::new(py, snapshot)?)?;
    }
    py.import("types")?
        .getattr("MappingProxyType")?
        .call1((dict,))
}

impl PyColumnView {
    /// Every cell, with its index, as a `Property` object.
    fn cells(&self, py: Python<'_>) -> PyResult<Vec<(Vec<Value>, Py<PyProperty>)>> {
        match &self.source {
            ColumnSource::Snapshot { cells, .. } => Ok(cells
                .iter()
                .map(|(index, cell)| (index.clone(), cell.clone_ref(py)))
                .collect()),
            ColumnSource::Live { table, column } => {
                let indexes: Vec<Vec<Value>> = table.borrow(py).read(py, true, |held| {
                    Ok(held
                        .column(column)
                        .or_raise(py)?
                        .cells()
                        .map(|(index, _)| index.into_iter().cloned().collect())
                        .collect())
                })?;
                indexes
                    .into_iter()
                    .map(|index| {
                        let cell =
                            PyProperty::cell(table.clone_ref(py), index.clone(), column.clone());
                        Ok((index, Py::new(py, cell)?))
                    })
                    .collect()
            }
        }
    }
}

#[pymethods]
impl PyColumnView {
    /// The value of each cell, in row order; ``None`` where a cell has none.
    #[getter]
    fn values(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        // One resolution for the column, not one per cell.
        if let ColumnSource::Live { table, column } = &self.source {
            let read = table
                .borrow(py)
                .read(py, true, |held| column_read(py, held, column))?;
            return said_column(py, read);
        }
        self.cells(py)?
            .iter()
            .map(|(_, cell)| bridge::to_python(py, &cell.borrow(py).value_of(py)?))
            .collect()
    }

    /// The uncertainty of each cell, in row order; ``None`` where a cell has none.
    #[getter]
    fn uncertainties(&self, py: Python<'_>) -> PyResult<Vec<Option<f64>>> {
        if let ColumnSource::Live { table, column } = &self.source {
            return table.borrow(py).read(py, true, |held| {
                held.column(column)
                    .or_raise(py)?
                    .cells()
                    .map(|(_, cell)| {
                        Ok(cell
                            .uncertainty()
                            .or_raise(py)?
                            .map(|uncertainty| uncertainty.magnitude()))
                    })
                    .collect()
            });
        }
        self.cells(py)?
            .iter()
            .map(|(_, cell)| {
                Ok(cell
                    .borrow(py)
                    .uncertainty_of(py)?
                    .map(|uncertainty| uncertainty.magnitude()))
            })
            .collect()
    }

    fn __getitem__(&self, py: Python<'_>, index: &Bound<'_, PyAny>) -> PyResult<Py<PyProperty>> {
        let wanted = index_from(index)?;
        let cells = self.cells(py)?;
        let available: Vec<String> = cells
            .iter()
            .map(|(index, _)| describe_index(index))
            .collect();
        cells
            .into_iter()
            .find(|(index, _)| {
                index.len() == wanted.len()
                    && index
                        .iter()
                        .zip(&wanted)
                        .all(|(have, want)| crate::core::value::equals(have, want))
            })
            .map(|(_, cell)| cell)
            .ok_or_else(|| {
                bridge::key_error(format!(
                    "no row at ({})\n  available: {}",
                    describe_index(&wanted),
                    available.join("; ")
                ))
            })
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        let cells: Vec<Py<PyProperty>> =
            self.cells(py)?.into_iter().map(|(_, cell)| cell).collect();
        PyList::new(py, cells)?.try_iter()
    }

    fn __len__(&self, py: Python<'_>) -> PyResult<usize> {
        Ok(self.cells(py)?.len())
    }

    fn __repr__(&self) -> String {
        let column = match &self.source {
            ColumnSource::Live { column, .. } | ColumnSource::Snapshot { column, .. } => column,
        };
        format!("ColumnView('{column}')")
    }
}

// ----------------------------------------------------------------- Sample

enum Found {
    Property(PropertyHandle),
    Table,
    Attribute(AttributeValue),
}

fn tags_id() -> Identifier {
    Identifier::new("tags").expect("a plain name")
}

/// Names a reserved or private field refusal would name, before anything is
/// moved into a sample.
fn refusal(name: &Identifier) -> Option<SampleError> {
    if name.as_str().starts_with('_') {
        return Some(SampleError::PrivateName { name: name.clone() });
    }
    RESERVED
        .contains(&name.as_str())
        .then(|| SampleError::NameCollision {
            name: name.clone(),
            existing: NameKind::Reserved,
        })
}

fn tags_from(object: &Bound<'_, PyAny>) -> PyResult<Vec<Identifier>> {
    if object.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "tags are a list of names, and a str is one name: tags = [\"reference\"]",
        ));
    }
    let mut tags = Vec::new();
    for item in object.try_iter()? {
        let item = item?;
        let text: String = item.extract().map_err(|_| {
            PyTypeError::new_err(format!(
                "a tag is a str, and this is {}",
                bridge::a_type(&item)
            ))
        })?;
        let tag = tag_from(&text)?;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(tags)
}

/// A tag, refused in a tag's words: the name's own refusals speak of an
/// unnamed property and of command-line arguments, which a tag is neither.
fn tag_from(text: &str) -> PyResult<Identifier> {
    use crate::core::identifier::IdentifierError;
    if text.trim().is_empty() {
        return Err(PyValueError::new_err(
            "a tag cannot be empty: a tag is a name, such as 'reference'",
        ));
    }
    Identifier::new(text).map_err(|error| match error {
        IdentifierError::Whitespace { name, position } => PyValueError::new_err(format!(
            "'{name}' contains whitespace at character {}, and a tag is one word. Try '{}'",
            position + 1,
            name.split_whitespace().collect::<Vec<_>>().join("_")
        )),
        other => PyValueError::new_err(format!("'{text}' is not a usable tag: {other}")),
    })
}

/// One sample, read from and saved to a Markdown file.
#[pyclass(name = "Sample", module = "samplekit", subclass, weakref, unsendable)]
pub struct PySample {
    shared: RefCell<SharedSample>,
    file: FileRef,
}

impl PySample {
    fn shared(&self) -> SharedSample {
        self.shared.borrow().clone()
    }

    fn lookup(&self, name: &Identifier) -> PyResult<Option<Found>> {
        self.shared().read(
            |sample| {
                Ok(if let Ok(handle) = sample.property(name) {
                    Some(Found::Property(handle))
                } else if sample.table(name).is_ok() {
                    Some(Found::Table)
                } else {
                    sample
                        .attribute(name)
                        .ok()
                        .map(|value| Found::Attribute(value.clone()))
                })
            },
            |snapshot| {
                Ok(if let Some(handle) = snapshot.properties.get(name) {
                    Some(Found::Property(handle.clone()))
                } else if snapshot.tables.contains(name) {
                    Some(Found::Table)
                } else {
                    snapshot
                        .attributes
                        .get(name)
                        .map(|value| Found::Attribute(value.clone()))
                })
            },
        )
    }

    /// Every field name, in the order a file writes them: attributes, then
    /// properties, then tables.
    fn names(&self) -> PyResult<Vec<String>> {
        self.shared().read(
            |sample| {
                Ok(sample
                    .attribute_names()
                    .into_iter()
                    .chain(sample.property_names())
                    .chain(sample.table_names())
                    .map(|name| name.to_string())
                    .collect())
            },
            |snapshot| {
                Ok(snapshot
                    .attributes
                    .keys()
                    .chain(snapshot.properties.keys())
                    .chain(snapshot.tables.iter())
                    .map(|name| name.to_string())
                    .collect())
            },
        )
    }

    fn unknown(&self, name: &str, missing: fn(String) -> PyErr) -> PyResult<PyErr> {
        Ok(missing(unknown_message("field", name, &self.names()?)))
    }

    fn found(slf: &Bound<'_, Self>, name: &Identifier, found: Found) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let this = slf.borrow();
        match found {
            Found::Property(handle) => Ok(Py::new(
                py,
                PyProperty::attached(handle, this.file.clone(), this.shared()),
            )?
            .into_any()),
            Found::Table => Ok(Py::new(
                py,
                PyTable::attached(this.shared(), this.file.clone(), name.clone()),
            )?
            .into_any()),
            Found::Attribute(AttributeValue::Scalar(value)) => bridge::to_python(py, &value),
            Found::Attribute(AttributeValue::List(items)) => {
                let items: Vec<Py<PyAny>> = items
                    .iter()
                    .map(|item| bridge::to_python(py, item))
                    .collect::<PyResult<_>>()?;
                let epoch = this.shared().epoch(name);
                Ok(bridge::bound_list_factory(py)?
                    .call1((slf, name.as_str(), epoch, PyList::new(py, items)?))?
                    .unbind())
            }
        }
    }

    fn get(slf: &Bound<'_, Self>, name: &str, missing: fn(String) -> PyErr) -> PyResult<Py<PyAny>> {
        PySample::get_among(slf, name, missing, &[])
    }

    /// The same, a name close to one of `members` offered too: the attribute
    /// form reaches the methods, and `b.sav()` meant `save`.
    fn get_among(
        slf: &Bound<'_, Self>,
        name: &str,
        missing: fn(String) -> PyErr,
        members: &[String],
    ) -> PyResult<Py<PyAny>> {
        let this = slf.borrow();
        let unknown = |this: &PySample| -> PyResult<PyErr> {
            Ok(missing(unknown_among(
                "field",
                name,
                &this.names()?,
                members,
            )))
        };
        let Ok(id) = Identifier::new(name) else {
            return Err(unknown(&this)?);
        };
        match this.lookup(&id)? {
            Some(found) => {
                drop(this);
                PySample::found(slf, &id, found)
            }
            None => Err(unknown(&this)?),
        }
    }

    /// A field path, resolved as the command line resolves it.
    fn field(slf: &Bound<'_, Self>, path: &str) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let parsed = fields::parse(path).map_err(|error| bridge::key_error(error.to_string()))?;
        let this = slf.borrow();
        let file = this.file.borrow().path.clone();
        let resolution = this.shared().read_sample("a field", |sample| {
            // A row or a name the sample does not have is an error, as
            // `Table.at` and `s["name"]` raise one: the command line shows `-`
            // in a table of many samples, where one without the row is no
            // mistake, and read alike here it answered `None` for a typo.
            match &parsed {
                fields::Field::Cell {
                    table, column, row, ..
                } => {
                    let held = sample.table(table).map_err(|_| {
                        let tables: Vec<String> = sample
                            .table_names()
                            .iter()
                            .map(|name| name.to_string())
                            .collect();
                        bridge::key_error(unknown_message("table", table.as_str(), &tables))
                    })?;
                    held.at(row, column).or_raise(py)?;
                }
                fields::Field::Named { name, .. } | fields::Field::ListItem { name, .. }
                    if !sample.has_property(name)
                        && !sample.has_attribute(name)
                        && sample.table(name).is_err() =>
                {
                    let names: Vec<String> = sample
                        .property_names()
                        .into_iter()
                        .chain(sample.table_names())
                        .chain(sample.attribute_names())
                        .map(|name| name.to_string())
                        .collect();
                    return Err(bridge::key_error(unknown_message(
                        "field",
                        name.as_str(),
                        &names,
                    )));
                }
                _ => {}
            }
            let vocabulary = fields::vocabulary_of(&[sample]);
            let subject = fields::Subject {
                sample,
                path: file.as_deref(),
                vocabulary: &vocabulary,
                states: None,
            };
            fields::resolve(&parsed, &subject).map_err(|error| bridge::key_error(error.to_string()))
        })?;
        match resolution {
            fields::Resolution::Scalar(value) => {
                bridge::to_python(py, &value.unwrap_or_else(Value::absent))
            }
            fields::Resolution::List(values) => {
                let items = values
                    .iter()
                    .map(|value| bridge::to_python(py, value))
                    .collect::<PyResult<Vec<_>>>()?;
                Ok(PyList::new(py, items)?.into_any().unbind())
            }
            fields::Resolution::Tags(tags) => {
                Ok(PyList::new(py, tags.iter().map(|tag| tag.to_string()))?
                    .into_any()
                    .unbind())
            }
        }
    }

    /// Assignment by what is assigned.
    fn assign(slf: &Bound<'_, Self>, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.borrow();
        let id = bridge::identifier_from(py, name)?;
        let shared = this.shared();
        shared.touch(id.as_str());
        let existing = this.lookup(&id)?;

        if let Ok(property) = value.cast::<PyProperty>() {
            if let Some(Found::Table | Found::Attribute(_)) = existing {
                return Err(kind_change(name, &existing, "a Property"));
            }
            if let Some(refused) = refusal(&id) {
                return Err(refused.into_py_err(py));
            }
            if let Some(Found::Property(handle)) = &existing {
                refuse_while_computing(handle)?;
            }
            let whole_column = property
                .borrow()
                .as_column
                .borrow()
                .as_ref()
                .is_some_and(|given| given.compute_column.is_some());
            if whole_column {
                return Err(PyValueError::new_err(format!(
                    "'{name}' is given compute_column, which computes a table's column whole: \
                     a property of the sample is computed by compute=, given nothing"
                )));
            }
            // A value a formula gives, replaced by one without a formula, lost
            // the formula without a word: `s.modulus = Property(5)` read as setting
            // the modulus and dropped how it is computed. The override is the
            // form that keeps it; deleting first says the formula is meant to go.
            let made = this.file.borrow().settled;
            if made && let Some(Found::Property(handle)) = &existing {
                let before = handle.peek(Formulas::of);
                let after = property.borrow().formulas(py)?;
                if (before.value || before.uncertainty) && !after.value && !after.uncertainty {
                    return Err(PyValueError::new_err(format!(
                        "'{name}' is computed by a formula, and this Property has none: set \
                         s.{name}.value = ... to override the formula, or del s.{name} first \
                         to replace it"
                    )));
                }
            }
            // **A refused declaration changes nothing**. Once the sample is
            // made a declaration applies as it is assigned, and it used to be
            // judged after the property was installed: a typo in `depends_on`
            // raised, and left the formula in place, marked declared, reading
            // nothing — the very case forbidden, a formula running
            // undeclared, with the caller's object emptied. Judged first, on
            // what the property carries.
            let settled = this.file.borrow().settled;
            if settled && let Some(declared) = property.borrow().declared.borrow().clone() {
                let formulas = property.borrow().formulas(py)?;
                let unfiled = this.file.borrow().path.is_none();
                shared.read(
                    |sample| {
                        resolve_declaration(py, sample, &id, &declared, formulas, unfiled)
                            .map(|_| ())
                    },
                    |_| Ok(()),
                )?;
            }
            let (taken, moved) = property.borrow().take(py)?;
            shared.write(|sample| sample.set_property(id.clone(), taken).or_raise(py))?;
            // A property copied from another sample carries no declaration: its
            // formula is not known here, and no digest is taken of it.
            let code = if moved {
                std::mem::take(&mut *property.borrow().code.borrow_mut())
            } else {
                Vec::new()
            };
            {
                let mut file = this.file.borrow_mut();
                if code.is_empty() {
                    file.formula_code.shift_remove(id.as_str());
                } else {
                    file.formula_code.insert(id.to_string(), code);
                }
            }
            // What the constructor was told this value reads, kept until the
            // model is built: `depends_on=["diameter"]` is written before `diameter`
            // need exist, so the names cannot be checked here.
            let declared = property.borrow().declared.borrow().clone();
            if let Some(inputs) = declared {
                let mut file = this.file.borrow_mut();
                file.declared.push((id.clone(), inputs));
                if !file.declared_names.contains(&id) {
                    file.declared_names.push(id.clone());
                }
            } else if settled {
                // Replacing a property replaces its declaration: a new quantity
                // that declares nothing has declared nothing.
                this.file
                    .borrow_mut()
                    .declared_names
                    .retain(|name| name != &id);
            }
            if settled && let Err(error) = apply_declarations(py, &shared, &this.file) {
                // What judging first cannot see is a cycle through the graph,
                // which only the installed property closes. The formula stays,
                // undeclared, so that `compute()` refuses it by name rather
                // than running it reading nothing.
                this.file
                    .borrow_mut()
                    .declared_names
                    .retain(|name| name != &id);
                return Err(error);
            }
            if moved {
                let handle = shared.read(
                    |sample| sample.property(&id).or_raise(py),
                    |snapshot| snapshot.properties.get(&id).cloned().ok_or_else(busy),
                )?;
                property
                    .borrow()
                    .become_attached(handle, this.file.clone(), shared.clone());
            }
            return Ok(());
        }

        if let Ok(table) = value.cast::<PyTable>() {
            if let Some(Found::Property(_) | Found::Attribute(_)) = existing {
                return Err(kind_change(name, &existing, "a Table"));
            }
            if let Some(refused) = refusal(&id) {
                return Err(refused.into_py_err(py));
            }
            let table = table.borrow();
            let taken = {
                let mut state = table.state.try_borrow_mut().map_err(|_| busy())?;
                match &*state {
                    TableState::Detached { qualifiers, .. } => {
                        if !qualifiers.is_empty() && !qualifiers.contains(&id) {
                            let named: Vec<String> =
                                qualifiers.iter().map(|q| format!("'{q}'")).collect();
                            return Err(PyValueError::new_err(format!(
                                "this table's column inputs are qualified by {}, and it is \
                                 assigned as '{id}': qualify them as '{id}.column'",
                                named.join(" or ")
                            )));
                        }
                    }
                    TableState::Attached { sample, name, .. } => {
                        if sample.same(&shared) && *name == id {
                            return Ok(());
                        }
                        return Err(PyValueError::new_err(format!(
                            "the table '{name}' belongs to a sample already: build another Table to \
                             store one as '{id}'"
                        )));
                    }
                }
                let attached = TableState::Attached {
                    sample: shared.clone(),
                    file: this.file.clone(),
                    name: id.clone(),
                };
                match std::mem::replace(&mut *state, attached) {
                    TableState::Detached { table, code, .. } => (table, code),
                    TableState::Attached { .. } => unreachable!("matched as detached above"),
                }
            };
            let (taken, code) = taken;
            shared.write(|sample| sample.set_table(id.clone(), *taken).or_raise(py))?;
            // The table's columns are its derivations' now, and no longer any
            // it replaced.
            let mut file = this.file.borrow_mut();
            let prefix = format!("{id}.");
            file.formula_code
                .retain(|name, _| !name.starts_with(&prefix));
            for (column, parts) in code {
                file.formula_code.insert(format!("{id}.{column}"), parts);
            }
            return Ok(());
        }

        if let Some(Found::Property(_) | Found::Table) = existing {
            return Err(kind_change(name, &existing, "a plain value"));
        }
        let attribute = bridge::attribute_from_python(value).map_err(|error| {
            if error.is_instance_of::<PyTypeError>(py) {
                PyTypeError::new_err(format!(
                    "'{name}' cannot hold this value: {}\n  nothing assigned without a leading '_' \
                     is lost on save, so it is refused; keep it private with s._{name} = ...",
                    error.value(py)
                ))
            } else {
                located(py, error, &format!("'{name}'"))
            }
        })?;
        // An attribute changing kind — text to a number, a date to text — is
        // assigned, and said: a filter or a comparison reads it otherwise from
        // then on, where it read it one way before.
        if let Some(Found::Attribute(before)) = &existing {
            let (was, is) = (attribute_kind(before), attribute_kind(&attribute));
            if let (Some(was), Some(is)) = (was, is)
                && was != is
            {
                warn_value(
                    py,
                    &format!(
                        "'{name}' was {was} and is now {is}: a filter or a comparison of it reads \
                         it as {is} from now on"
                    ),
                )?;
            }
        }
        shared.write(|sample| sample.set_attribute(id.clone(), attribute).or_raise(py))?;
        shared.advance(&id);
        Ok(())
    }

    fn remove(&self, py: Python<'_>, name: &str, missing: fn(String) -> PyErr) -> PyResult<()> {
        let Ok(id) = Identifier::new(name) else {
            return Err(self.unknown(name, missing)?);
        };
        let shared = self.shared();
        shared.touch(id.as_str());
        match self.lookup(&id)? {
            Some(Found::Property(handle)) => {
                refuse_while_computing(&handle)?;
                shared.write(|sample| sample.remove_property(&id).map(|_| ()).or_raise(py))
            }
            Some(Found::Table) => {
                shared.write(|sample| sample.remove_table(&id).map(|_| ()).or_raise(py))
            }
            Some(Found::Attribute(_)) => {
                shared.write(|sample| sample.remove_attribute(&id).map(|_| ()).or_raise(py))?;
                shared.advance(&id);
                Ok(())
            }
            None => Err(self.unknown(name, missing)?),
        }
    }
}

/// A property with a unit, or one holding a number, holds a number: text is
/// refused, and so is `True`, which Python counts as a number and no quantity
/// is. `None` and `samplekit.NA` say there is none, and stand.
fn refuse_other_than_a_number(
    value: &Bound<'_, PyAny>,
    label: &str,
    unit: Option<&str>,
) -> PyResult<()> {
    let quantity = match unit {
        Some(unit) => format!("{label} is measured in {unit}"),
        None => format!("{label} holds a number"),
    };
    if let Ok(text) = value.cast::<PyString>() {
        return Err(PyValueError::new_err(format!(
            "{quantity}, and \"{text}\" is text: assign the number alone"
        )));
    }
    if value.is_instance_of::<PyBool>() {
        return Err(PyValueError::new_err(format!(
            "{quantity}, and {} is a boolean, not a number",
            bridge::repr(value)
        )));
    }
    Ok(())
}

/// A sample's name as Python gives it: text, not blank, and no path. An
/// empty name was stored and saved, and a sample nobody can name on the
/// command line, nor tell apart in a table, is no name; a path is refused as
/// `samplekit new` refuses it, since `save_all` writes a file by the name.
fn sample_name(value: &Bound<'_, PyAny>) -> PyResult<String> {
    let name: String = value
        .extract()
        .map_err(|_| PyTypeError::new_err("a sample's name is a str or None"))?;
    if name.trim().is_empty() {
        return Err(PyValueError::new_err(
            "a sample's name cannot be empty: give it text, or None to name it by its file",
        ));
    }
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Err(PyValueError::new_err(format!(
            "'{name}' is a path: a sample takes a name, and its file is where save() writes it"
        )));
    }
    // One rule with `set` and the workbench.
    if let Some(refused) = crate::collection::editing::refuses_written_name(&name) {
        return Err(PyValueError::new_err(refused.message));
    }
    Ok(name)
}

fn kind_change(name: &str, existing: &Option<Found>, assigned: &str) -> PyErr {
    let (kind, fix) = match existing {
        Some(Found::Property(_)) => (
            "a property",
            format!("set s.{name}.value = ... to change its value"),
        ),
        Some(Found::Table) => ("a table", format!("change its cells through s.{name}")),
        Some(Found::Attribute(_)) => ("an attribute", format!("assign s.{name} a plain value")),
        None => ("free", String::new()),
    };
    PyTypeError::new_err(format!(
        "'{name}' is {kind}, and assigning {assigned} would change its kind: {fix}, or del s.{name} \
         first"
    ))
}

#[pymethods]
impl PySample {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        PySample {
            shared: RefCell::new(SharedSample::new(Sample::new())),
            file: Rc::new(RefCell::new(SampleFile::default())),
        }
    }

    #[pyo3(signature = (path=None, name=None, model=None))]
    fn __init__(
        slf: &Bound<'_, Self>,
        path: Option<&Bound<'_, PyAny>>,
        name: Option<&Bound<'_, PyAny>>,
        model: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<()> {
        // Which class to construct was decided by the package's metaclass.
        let _ = model;
        let this = slf.borrow();
        if let Some(name) = name.filter(|name| !name.is_none()) {
            let name = sample_name(name)?;
            this.shared().write(|sample| {
                sample.set_name(Some(name));
                Ok(())
            })?;
        }
        if let Some(path) = path.filter(|path| !path.is_none()) {
            this.file.borrow_mut().pending = Some(file_path_from(path)?);
        }
        Ok(())
    }

    /// Reads the file `__init__` was given, now that the class has declared
    /// what it fills.
    // A comment, not the docstring, which Python shows.
    #[pyo3(signature = (args, kwargs=None))]
    fn _load(
        slf: &Bound<'_, Self>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let py = slf.py();
        prime_model(&slf.get_type());
        let this = slf.borrow();
        let pending = this.file.borrow_mut().pending.take();
        let Some(path) = pending else {
            if let Some(given) = path_given(slf, args, kwargs)? {
                return Err(PyTypeError::new_err(format!(
                    "{}.__init__ received the path {given} and did not pass it to \
                     super().__init__(), so the file was not read: forward it, as \
                     super().__init__(path, ...)",
                    slf.get_type().name()?
                )));
            }
            // A sample made in memory, or by `Sample.new`, is made now: its
            // declarations are settled and an undeclared formula refused at
            // this moment, as they are when a file is read.
            settle_declarations(py, &this.shared(), &this.file)?;
            this.file.borrow_mut().settled = true;
            return Ok(());
        };
        if !path.is_file() {
            return Err(missing_file(&path));
        }
        let config = config_for(py, &path)?;
        let shared = this.shared();
        let origin = shared.write(|sample| {
            document::load_into(&path, sample).map_err(|error| {
                ListError::Document {
                    path: Some(path.clone()),
                    error,
                }
                .into_py_err(py)
            })
        })?;
        // The model is built and its file is read: this is the moment a
        // declaration can be checked against what the sample actually holds,
        // and where a formula that declared nothing is refused.
        {
            // The declarations are applied against the file just read, so
            // `path` is known to them before it is recorded below.
            this.file.borrow_mut().path = Some(path.clone());
        }
        settle_declarations(py, &shared, &this.file)?;
        // Every value the file supplied is current once everything is in place
        // : the declarations just settled gave filled values inputs they had no
        // record of.
        shared.write(|sample| {
            sample.confirm_filled();
            Ok(())
        })?;
        let names: Vec<Identifier> = shared.read(
            |sample| Ok(sample.attribute_names().into_iter().cloned().collect()),
            |snapshot| Ok(snapshot.attributes.keys().cloned().collect()),
        )?;
        for name in names {
            shared.advance(&name);
        }
        shared.advance(&tags_id());
        // What the model's declarations set as the sample was made is no
        // change of the script's: a save writes what it changes from here.
        shared.forget_touched();
        let mut file = this.file.borrow_mut();
        // The text the origin's digest was taken of: one changed in between is
        // no base, and the save writes the sample whole, refused as changed.
        file.read = std::fs::read_to_string(&path)
            .ok()
            .filter(|text| document::digest_of(text) == origin.digest);
        file.path = Some(path);
        file.origin = Some(origin);
        file.config = config;
        file.settled = true;
        Ok(())
    }

    /// Gives a sample made by `Sample.new` the file it is to be saved as.
    fn _create_at(&self, py: Python<'_>, path: &Bound<'_, PyAny>) -> PyResult<()> {
        let path = path_from(path)?;
        let config = config_for(py, &path)?;
        // Named by the file it is to be saved as, where it writes no name.
        self.shared().write(|sample| {
            sample.set_file(Some(&path));
            Ok(())
        })?;
        let mut file = self.file.borrow_mut();
        file.path = Some(path);
        file.origin = None;
        file.config = config;
        Ok(())
    }

    /// A name given at construction, which a file's own name wins over.
    fn _declare_name(&self, name: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        if let Some(name) = name.filter(|name| !name.is_none()) {
            let name = sample_name(name)?;
            self.shared().write(|sample| {
                if sample.written_name().is_none() {
                    sample.set_name(Some(name));
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    /// Stores the list a bound list would become, or answers `None` when the
    /// list was detached by a later assignment.
    fn _commit_list(
        slf: &Bound<'_, Self>,
        name: &str,
        epoch: u64,
        candidate: &Bound<'_, PyList>,
    ) -> PyResult<Option<Py<PyAny>>> {
        let py = slf.py();
        // A list of the sample — its tags — changed by a formula as it runs:
        // `self.tags.append(…)` wrote its tag, and nothing failed.
        refuse_while_a_formula_runs(&format!("'{name}'"))?;
        let this = slf.borrow();
        let shared = this.shared();
        let id = bridge::identifier_from(py, name)?;
        if shared.epoch(&id) != epoch {
            return Ok(None);
        }
        shared.touch(id.as_str());
        if id == tags_id() {
            let tags = tags_from(candidate.as_any())?;
            let committed: Vec<String> = tags.iter().map(|tag| tag.to_string()).collect();
            // A tag the sample carries already, appended again, changes
            // nothing: said, rather than done in silence.
            let given: Vec<String> = candidate
                .iter()
                .filter_map(|item| item.extract::<String>().ok())
                .collect();
            let twice: Vec<&String> = given
                .iter()
                .enumerate()
                .filter(|(at, tag)| given[..*at].contains(tag))
                .map(|(_, tag)| tag)
                .collect();
            if let Some(tag) = twice.first() {
                warn_value(
                    py,
                    &format!("'{tag}' is a tag of this sample already: it is carried once"),
                )?;
            }
            shared.write(|sample| {
                sample.set_tags(tags);
                Ok(())
            })?;
            return Ok(Some(PyList::new(py, committed)?.into_any().unbind()));
        }
        let bound = shared.read(
            |sample| {
                Ok(sample
                    .attribute(&id)
                    .is_ok_and(|value| value.as_list().is_some()))
            },
            |snapshot| {
                Ok(snapshot
                    .attributes
                    .get(&id)
                    .is_some_and(|value| value.as_list().is_some()))
            },
        )?;
        if !bound {
            return Ok(None);
        }
        let value = bridge::attribute_from_python(candidate.as_any())
            .map_err(|error| located(py, error, &format!("the list '{name}'")))?;
        let items: Vec<Value> = value
            .as_list()
            .expect("a Python list converts to a list")
            .to_vec();
        shared.write(|sample| sample.set_attribute(id.clone(), value).or_raise(py))?;
        let committed: Vec<Py<PyAny>> = items
            .iter()
            .map(|item| bridge::to_python(py, item))
            .collect::<PyResult<_>>()?;
        Ok(Some(PyList::new(py, committed)?.into_any().unbind()))
    }

    /// The file the sample was read from or last saved to, or ``None`` for a
    /// sample never saved. ``save(path)`` changes it.
    #[getter]
    fn path(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.file
            .borrow()
            .path
            .as_ref()
            .map(|path| python_path(py, path))
            .transpose()
    }

    /// The name of the sample's file, without its folder, or ``None``.
    #[getter]
    fn filename(&self) -> Option<String> {
        self.file.borrow().path.as_ref().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    }

    /// The project the sample's file belongs to — what its ``.samplekitrc``
    /// declares.
    #[getter]
    fn project(&self) -> PyProject {
        PyProject {
            config: self.file.borrow().config.clone(),
        }
    }

    /// The sample's name, or ``None``.
    #[getter]
    fn name(&self) -> PyResult<Option<String>> {
        // Read from the snapshot while tables resolve, as an attribute is: a
        // row formula may name its sample.
        self.shared().read(
            |sample| Ok(sample.name().map(str::to_string)),
            |snapshot| Ok(snapshot.name.clone()),
        )
    }

    /// The note — the Markdown text below the frontmatter, kept as written.
    #[getter]
    fn note(&self) -> PyResult<String> {
        self.shared()
            .read_sample("the note", |sample| Ok(sample.note().to_string()))
    }

    /// The sample's tags, as a list whose changes are saved with the sample.
    ///
    /// Each tag is an identifier, and none appears twice.
    #[getter]
    fn tags(slf: &Bound<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let this = slf.borrow();
        let shared = this.shared();
        let tags: Vec<String> = shared.read_sample("the tags", |sample| {
            Ok(sample.tags().iter().map(|tag| tag.to_string()).collect())
        })?;
        let epoch = shared.epoch(&tags_id());
        Ok(bridge::bound_list_factory(py)?
            .call1((slf, "tags", epoch, PyList::new(py, tags)?))?
            .unbind())
    }

    /// The sample an attribute links to, read as ``sk.load`` reads it, or
    /// the file it leads to.
    ///
    /// A link is an attribute written ``"[[NAME]]"``, as Obsidian writes
    /// one: the sample of the project whose file is NAME.md, else whose
    /// ``name`` is NAME; or a Markdown link to a file,
    /// ``"[the second](NAME.md)"``, its path read from this sample's folder.
    /// A sample's file gives the sample; any other file its path. A list of
    /// links gives a list, in order.
    ///
    /// Args:
    ///     field: The attribute holding the link, or the links.
    ///
    /// Returns:
    ///     The sample it names, or a ``pathlib.Path`` for a file that is no
    ///     sample; for a list, a list of them, in order.
    ///
    /// Raises:
    ///     KeyError: The sample holds no such attribute.
    ///     ValueError: The attribute is no link: text, or a web address; or
    ///         several samples answer to it.
    ///     FileNotFoundError: No sample answers to it, or nothing is at its
    ///         path.
    ///
    /// Example:
    ///     >>> steel.linked("reference").name
    ///     'aluminium-01'
    fn linked(slf: &Bound<'_, Self>, field: &str) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let this = slf.borrow();
        let name =
            Identifier::new(field).map_err(|error| PyKeyError::new_err(error.to_string()))?;
        let (texts, list) = this.shared().read_sample(field, |sample| {
            Ok(match sample.attribute(&name) {
                Ok(crate::core::sample::AttributeValue::Scalar(value)) => {
                    (vec![value.clone()], false)
                }
                Ok(crate::core::sample::AttributeValue::List(values)) => (values.clone(), true),
                Err(_) => {
                    return Err(PyKeyError::new_err(format!(
                        "{field}: the sample holds no attribute of that name"
                    )));
                }
            })
        })?;
        let own = this.file.borrow().path.clone();
        let config = this.file.borrow().config.clone();
        drop(this);
        let from = own.unwrap_or_else(|| PathBuf::from("sample.md"));
        let mut samples = Vec::new();
        for (at, value) in texts.iter().enumerate() {
            let said = if list {
                format!("{field}[#{at}]")
            } else {
                field.to_string()
            };
            let link = match value {
                Value::Text(text) => crate::core::link::parse(text),
                _ => None,
            };
            let Some(link) = link else {
                return Err(PyValueError::new_err(format!(
                    "{said} is no link: a link to a sample is written \"[[name]]\", or \
                     \"[text](name.md)\""
                )));
            };
            let leads = crate::config::discovery::destination(&link, &from, || {
                crate::config::discovery::linkable(config.as_deref(), &from, &[])
            });
            let path = match leads {
                crate::config::discovery::Destination::Sample(path) => path,
                crate::config::discovery::Destination::Address(address) => {
                    return Err(PyValueError::new_err(format!(
                        "{said} is a web address, {address}, and no sample"
                    )));
                }
                crate::config::discovery::Destination::File(path) => {
                    samples.push(py.import("pathlib")?.getattr("Path")?.call1((path,))?);
                    continue;
                }
                crate::config::discovery::Destination::Nowhere(target) => {
                    let written = link.shown().to_string();
                    return Err(PyFileNotFoundError::new_err(match link {
                        crate::core::link::Link::Sample { .. } => format!(
                            "{said}: [[{target}]] leads to no sample: no file of the project \
                             is named {target}, and no sample's name is"
                        ),
                        _ => format!(
                            "{said}: {written} leads to nothing: no file is at {target}, from \
                             the sample's folder"
                        ),
                    }));
                }
                crate::config::discovery::Destination::Several(paths) => {
                    return Err(PyValueError::new_err(format!(
                        "{said}: {} leads to {} samples: {}",
                        link.shown(),
                        paths.len(),
                        paths
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            };
            samples.push(py.import("samplekit")?.getattr("load")?.call1((path,))?);
        }
        if list {
            Ok(PyList::new(py, samples)?.into_any().unbind())
        } else {
            Ok(samples
                .pop()
                .map(|sample| sample.unbind())
                .unwrap_or_else(|| py.None()))
        }
    }

    /// Delete the sample's file.
    ///
    /// The project's history keeps it, and ``samplekit FILE restore --write``
    /// gives it back; the sample's own files, under ``[collection] files``,
    /// stay. The sample keeps its values: ``save()`` writes it again.
    ///
    /// Raises:
    ///     ValueError: The sample has no file, or the script runs under
    ///         ``samplekit run``, whose preview cannot hold a removal.
    ///     FileNotFoundError: Its file was removed or moved since it was read.
    ///     OSError: The file changed on disk since it was read.
    ///
    /// Example:
    ///     >>> sk.Sample("samples/steel-01.md").remove()
    #[pyo3(name = "remove")]
    fn remove_file(slf: &Bound<'_, Self>) -> PyResult<()> {
        let py = slf.py();
        let this = slf.borrow();
        let Some(path) = this.file.borrow().path.clone() else {
            return Err(PyValueError::new_err("this sample has no file to remove"));
        };
        if document::holding_saves() {
            return Err(PyValueError::new_err(
                "a script samplekit run previews cannot remove a sample: samplekit FILE remove \
                 does",
            ));
        }
        let Ok(held) = fs::read_to_string(&path) else {
            return Err(PyFileNotFoundError::new_err(format!(
                "{} was removed or moved since it was read, and nothing was removed",
                path.display()
            )));
        };
        if let Some(read) = this.file.borrow().read.as_deref()
            && read != held
        {
            return Err(PyOSError::new_err(format!(
                "{} changed on disk since it was read, and nothing was removed: read it again \
                 to see what changed",
                path.display()
            )));
        }
        let unread = history_begin(&path);
        document::note_written(&path);
        fs::remove_file(&path)
            .map_err(|error| PyOSError::new_err(format!("{}: {error}", path.display())))?;
        {
            // Its values stay, and a save writes them as a new file's.
            let mut file = this.file.borrow_mut();
            file.origin = None;
            file.read = None;
        }
        history_saved(py, &path, unread, true)
    }

    /// Rename the sample: its file, its own files, and every link of its
    /// project leading to it.
    ///
    /// The file becomes ``NEW.md`` beside the old one, its own files under
    /// ``[collection] files`` renamed with it, its ``name`` too where it was
    /// the file's name; each link of the project that led to it, ``[[OLD]]``
    /// or ``[text](OLD.md)``, is written to lead to it again. The history
    /// keeps the samples; renamed back, the files and the links go back too.
    ///
    /// Args:
    ///     new: The new name: the file's, without its folder or ``.md``.
    ///
    /// Returns:
    ///     The sample's new path.
    ///
    /// Raises:
    ///     ValueError: The sample has no file, the name is no file's name, or
    ///         the script runs under ``samplekit run``, whose preview cannot
    ///         hold a rename.
    ///     FileExistsError: Another sample, or one of its files, has the name.
    ///     OSError: The file changed on disk since it was read.
    ///
    /// Example:
    ///     >>> sk.Sample("samples/steel-01.md").rename("steel-04")
    ///     PosixPath('samples/steel-04.md')
    fn rename(slf: &Bound<'_, Self>, new: &str) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let this = slf.borrow();
        let Some(path) = this.file.borrow().path.clone() else {
            return Err(PyValueError::new_err("this sample has no file to rename"));
        };
        if document::holding_saves() {
            return Err(PyValueError::new_err(
                "a script samplekit run previews cannot rename a sample: samplekit FILE rename \
                 does",
            ));
        }
        if let (Some(read), Ok(held)) = (
            this.file.borrow().read.as_deref(),
            fs::read_to_string(&path),
        ) && read != held
        {
            return Err(PyOSError::new_err(format!(
                "{} changed on disk since it was read, and nothing was renamed: read it again \
                 to see what changed",
                path.display()
            )));
        }
        let config = this.file.borrow().config.clone();
        let plan =
            crate::collection::renaming::plan(config.as_deref(), &path, new).map_err(|error| {
                if error.0.contains("exists") {
                    PyFileExistsError::new_err(error.0)
                } else {
                    PyValueError::new_err(error.0)
                }
            })?;
        let unread = history_begin(&path);
        crate::collection::renaming::apply(&plan).map_err(|error| PyOSError::new_err(error.0))?;
        if let Some((_, name)) = &plan.name {
            let name = name.clone();
            this.shared().write(|sample| {
                sample.set_name(Some(name));
                Ok(())
            })?;
        }
        {
            let mut file = this.file.borrow_mut();
            file.path = Some(plan.to.clone());
            file.read = fs::read_to_string(&plan.to).ok();
            file.origin = document::load_sample(&plan.to)
                .ok()
                .map(|(_, origin)| origin);
        }
        history_saved(py, &plan.to, unread, true)?;
        Ok(py
            .import("pathlib")?
            .getattr("Path")?
            .call1((plan.to.clone(),))?
            .unbind())
    }

    /// Write the sample to its file.
    ///
    /// Nothing is overwritten unless asked: a file changed on disk since it was
    /// read, or an existing file at a new path, is refused. Saving runs no
    /// formula: a value never computed is written without a value.
    ///
    /// Args:
    ///     path: Where to write it instead. The sample then belongs to that file:
    ///         a later ``save()`` writes there.
    ///     overwrite: Write even over a file changed since it was read, or over an
    ///         existing file at ``path``.
    ///
    /// Raises:
    ///     ValueError: The sample has no file yet and no ``path`` is given.
    ///         Also: another process is saving the same file at this moment.
    ///     FileExistsError: ``path`` exists already.
    ///     FileNotFoundError: The folder does not exist, or the sample's file was
    ///         removed or moved since it was read.
    ///     IsADirectoryError: ``path`` is a folder.
    ///     OSError: The file changed on disk since it was read.
    ///     PermissionError: The file is read-only.
    ///
    /// Example:
    ///     >>> steel.grade = "S275"
    ///     >>> steel.save()
    #[pyo3(signature = (path=None, overwrite=false))]
    fn save(
        slf: &Bound<'_, Self>,
        path: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<()> {
        let py = slf.py();
        let this = slf.borrow();
        let own = this.file.borrow().path.clone();
        let requested = path
            .filter(|path| !path.is_none())
            .map(file_path_from)
            .transpose()?;
        let target = match (&requested, &own) {
            (Some(requested), _) => requested.clone(),
            (None, Some(own)) => own.clone(),
            (None, None) => {
                return Err(PyValueError::new_err(
                    "this sample has no file yet: save(path) gives it one",
                ));
            }
        };
        // A folder is no file to write, whatever `overwrite` says: advising
        // `overwrite=True` sent the script to a second failure.
        if target.is_dir() {
            let name = this
                .name()?
                .or_else(|| {
                    own.as_ref()
                        .and_then(|own| own.file_stem())
                        .map(|stem| stem.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "sample".to_string());
            return Err(PyIsADirectoryError::new_err(format!(
                "{} is a folder, and nothing was written: name a file in it, save(\"{}\")",
                target.display(),
                target.join(format!("{name}.md")).display()
            )));
        }
        if let Some(folder) = target
            .parent()
            .filter(|folder| !folder.as_os_str().is_empty() && !folder.is_dir())
        {
            return Err(PyFileNotFoundError::new_err(format!(
                "the folder {} does not exist, and nothing was written: create it first",
                folder.display()
            )));
        }
        let rewriting = own.as_ref().is_some_and(|own| same_file(own, &target));
        // A sample made by `Sample.new` has its path and no file read yet: a
        // file there now was written by someone else.
        let created = rewriting && this.file.borrow().origin.is_none();
        let destination = if created {
            if target.exists() && !overwrite {
                return Err(PyFileExistsError::new_err(format!(
                    "{} was written since Sample.new, and nothing was written: \
                     save(overwrite=True) replaces it",
                    target.display()
                )));
            }
            Destination::Path(target.clone())
        } else if rewriting {
            if !target.exists() && !overwrite {
                return Err(PyFileNotFoundError::new_err(format!(
                    "{} was removed or moved since it was read, and nothing was written: \
                     save(overwrite=True) writes it again",
                    target.display()
                )));
            }
            match (&this.file.borrow().origin, overwrite) {
                (Some(origin), false) => Destination::Origin(origin.clone()),
                _ => Destination::Path(target.clone()),
            }
        } else {
            if target.exists() && !overwrite {
                return Err(PyFileExistsError::new_err(format!(
                    "{} already exists, and nothing was written: save(path, overwrite=True) \
                     replaces it",
                    target.display()
                )));
            }
            Destination::Path(target.clone())
        };

        // A folder made a project: what the project above kept of its files
        // brought into its own history before a first write here starts it.
        if !document::holding_saves()
            && let Some(project) =
                crate::config::version_control::projects_of(std::slice::from_ref(&target))
                    .into_iter()
                    .next()
            && let Some(said) = crate::config::version_control::bring_from_above(&project)
        {
            warn_value(py, &said)?;
        }
        let shared = this.shared();
        // Saves held for a preview keep no history: the command that writes
        // them does.
        let holding = document::holding_saves();
        // What the project held before the script's first save, for the one
        // snapshot the script takes.
        let unread = if holding {
            Vec::new()
        } else {
            history_begin(&target)
        };
        // The records the file held before: a record new in it is one the
        // script computed, and the trace keeps which formula did.
        // The file as it was: a save that wrote nothing is named in no
        // snapshot's message, which listed twelve files where two changed.
        let before_bytes = std::fs::read(&target).ok();
        let before_records = document::load_sample(&target)
            .map(|(sample, _)| crate::config::version_control::records_of_values(&sample))
            .unwrap_or_default();
        // Runs no formula: a value never computed is written absent, and a
        // stale value with the record that says so.
        // Its own file, as it was read: only what the script changed, or
        // computed, is written — a statistic the model took of its readings on
        // reading, or a unit it declares for a value the file does not hold,
        // is no change of the script's, where `tags.append` wrote them all.
        let touched = shared.touched();
        let changes_only = rewriting
            && !created
            && this.file.borrow().origin.is_some()
            && !touched.contains(COMPUTED);
        let read = this.file.borrow().read.clone();
        let saved = shared.write(|sample| {
            Ok(if changes_only {
                document::save_changed(sample, &destination, &touched, read.as_deref())
            } else {
                document::save_computed(sample, &destination)
            })
        })?;
        if saved.is_ok() {
            shared.forget_touched();
        }
        let file_config = this.file.borrow().config.clone();
        let origin = saved.map_err(|error| match error {
            DocumentError::ConcurrentEdit { path } => {
                // What changed there, value by value, and the way that keeps
                // it: `overwrite=True` alone was offered, which wrote over it
                // without a word of what it lost.
                let changed: Vec<String> = match (&read, std::fs::read(&path).ok(), &file_config) {
                    (Some(read), Some(now), Some(config)) => {
                        crate::presentation::changes::changes_of(
                            &path.display().to_string(),
                            Some(&read.clone().into_bytes()),
                            Some(&now),
                            config,
                        )
                        .into_iter()
                        .skip(1)
                        .collect()
                    }
                    _ => Vec::new(),
                };
                let said = if changed.is_empty() {
                    String::new()
                } else {
                    format!(":\n{}", changed.join("\n"))
                };
                PyOSError::new_err(format!(
                    "{} changed since it was read, and nothing was written{said}\n  read it \
                     again, sk.Sample(path), to make your change over what it holds now; \
                     save(overwrite=True) writes this sample over it, and what changed there \
                     is lost",
                    path.display()
                ))
            }
            other => other.into_py_err(py),
        })?;
        let config = if rewriting {
            this.file.borrow().config.clone()
        } else {
            config_for(py, &target)?
        };
        {
            let mut file = this.file.borrow_mut();
            file.path = Some(target.clone());
            file.read = document::current_text(&target)
                .ok()
                .filter(|text| document::digest_of(text) == origin.digest);
            file.origin = Some(origin);
            file.config = config;
        }
        let traced = this.file.borrow().config.clone();
        let computed = std::mem::take(&mut this.file.borrow_mut().computed);
        drop(this);
        if holding {
            return log_failures(slf);
        }
        if let Some(formulas) = formula_digests(slf)
            && let Ok(mut session) = HISTORY_SESSION.lock()
        {
            session.tracing.saved(
                traced.as_deref(),
                &before_records,
                &target,
                &formulas,
                &computed,
            );
        }
        let changed = std::fs::read(&target).ok() != before_bytes;
        history_saved(py, &target, unread, changed)?;
        log_failures(slf)
    }

    /// Whether anything was changed since the sample was read or last saved.
    #[getter]
    fn _changed(&self) -> bool {
        !self.shared().touched().is_empty()
    }

    /// The sample as ``samplekit view`` shows it — its values, then its
    /// tables — each value in a style's forms.
    ///
    /// Args:
    ///     style: A style's name: ``"plain"``, one SampleKit gives, or one
    ///         ``.samplekitrc`` declares. Without it, ``[render] style``'s.
    ///
    /// Returns:
    ///     The text.
    ///
    /// Raises:
    ///     ValueError: No style has that name.
    ///
    /// Example:
    ///     >>> print(steel.to_text())
    #[pyo3(signature = (style=None))]
    fn to_text(&self, style: Option<&str>) -> PyResult<String> {
        let file = self.file.borrow();
        let config = file.config.clone();
        style_known(config.as_deref(), style)?;
        let path = file.path.clone();
        drop(file);
        // What the file holds, as `view` reads it: a value or a table only the
        // model declares, empty, is not written.
        let held: Option<Vec<String>> = path.as_deref().and_then(|path| {
            crate::format::document::load_sample(path)
                .ok()
                .map(|(file, _)| {
                    file.property_names()
                        .into_iter()
                        .chain(file.table_names())
                        .map(ToString::to_string)
                        .collect()
                })
        });
        self.shared().read_sample("the sample", |sample| {
            Ok(sheet_held(
                sample,
                path.as_deref(),
                config.as_deref(),
                style,
                held.as_deref(),
            ))
        })
    }

    /// The sample's own files: images, reports, anything found by the sample's
    /// name in the folders ``[collection] files`` declares.
    ///
    /// Args:
    ///     pattern: A glob on the files' names, such as ``"*.png"``.
    ///
    /// Returns:
    ///     The files' paths.
    ///
    /// Raises:
    ///     ValueError: The sample has no file, or belongs to no project.
    #[pyo3(signature = (pattern=None))]
    fn files(&self, py: Python<'_>, pattern: Option<&str>) -> PyResult<Vec<Py<PyAny>>> {
        self.own_files(pattern)?
            .iter()
            .map(|path| python_path(py, path))
            .collect()
    }

    /// Open the sample's own files with the system's application.
    ///
    /// Args:
    ///     pattern: A glob on the files' names, such as ``"*.pdf"``.
    ///     navigate: Open the folders holding the files, in the file manager,
    ///         instead of the files.
    ///     many: Allow opening more than five.
    ///
    /// Returns:
    ///     The paths opened.
    ///
    /// Raises:
    ///     FileNotFoundError: No file of the sample matches.
    ///     ValueError: More than five would open and ``many`` is false.
    ///     RuntimeError: Over SSH without a display — ``DISPLAY`` and
    ///         ``WAYLAND_DISPLAY`` unset — there is nothing to open them on.
    ///         Elsewhere the system's opener is started, and what it does
    ///         without a display is its own.
    #[pyo3(signature = (pattern=None, navigate=false, many=false))]
    fn open(
        &self,
        py: Python<'_>,
        pattern: Option<&str>,
        navigate: bool,
        many: bool,
    ) -> PyResult<Vec<Py<PyAny>>> {
        use crate::presentation::opening;
        let found = self.own_files(pattern)?;
        if found.is_empty() {
            return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
                "no file of this sample{}: [collection] files says where they are looked for",
                pattern
                    .map(|pattern| format!(" matches '{pattern}'"))
                    .unwrap_or_default()
            )));
        }
        // Every file the pattern names; files of one folder are one folder to
        // navigate to.
        let targets = if navigate {
            opening::folders_of(&found)
        } else {
            found
        };
        if targets.len() > opening::OPENED_WITHOUT_ASKING && !many {
            let names: Vec<String> = targets
                .iter()
                .filter_map(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            return Err(PyValueError::new_err(format!(
                "{} {} of this sample{}: {} — more than {} open only with many=True",
                targets.len(),
                if navigate { "folders" } else { "files" },
                pattern
                    .map(|pattern| format!(" match '{pattern}'"))
                    .unwrap_or_default(),
                names.join(", "),
                opening::OPENED_WITHOUT_ASKING
            )));
        }
        let mut opened = Vec::new();
        for target in &targets {
            if navigate {
                opened.push(opening::navigate(target).map_err(opening_failure)?);
            } else {
                opening::open(target).map_err(opening_failure)?;
                opened.push(target.clone());
            }
        }
        opened.iter().map(|path| python_path(py, path)).collect()
    }

    /// Declare what a formula reads.
    ///
    /// A change to one of the inputs makes the value outdated, and a computation
    /// runs the inputs first. A later declaration replaces an earlier one.
    ///
    /// Args:
    ///     output: The value the formula gives: a property's name, or
    ///         ``table.column``.
    ///     depends_on: What it reads: a list of names, or ``{"v": [...],
    ///         "u": [...]}`` where the value and the uncertainty have a formula
    ///         each. ``[]`` says it reads nothing of the sample.
    ///
    /// Raises:
    ///     KeyError: An unknown name, once the model is built.
    ///     ValueError: The declaration makes a cycle, or names a channel with no
    ///         formula.
    ///
    /// Example:
    ///     >>> class Specimen(sk.Sample):
    ///     ...     def __init__(self, path=None, name=None):
    ///     ...         super().__init__(path, name=name)
    ///     ...         self.area = sk.Property(unit="mm²", compute_value=self._area)
    ///     ...         self.set_dependencies("area", depends_on=["width", "thickness"])
    ///     ...
    ///     ...     def _area(self):
    ///     ...         return self.width.value * self.thickness.value
    #[pyo3(signature = (output, *, depends_on))]
    fn set_dependencies(
        &self,
        py: Python<'_>,
        output: &str,
        depends_on: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let output = bridge::identifier_from(py, output)?;
        let declared = declared_from(depends_on, "depends_on")?;
        self.shared().write(|sample| {
            sample.property(&output).or_raise(py)?;
            let formulas = Formulas::in_sample(sample, &output);
            let (nodes, channels) =
                resolve_declaration(py, sample, &output, &declared, formulas, false)?;
            sample
                .declare_dependencies_by_channel(&output, &nodes, channels)
                .or_raise(py)
        })?;
        // Declared by this route too. What separates *this formula reads
        // nothing* from *nobody said* is that someone said it, not how.
        let mut file = self.file.borrow_mut();
        // **The later declaration wins, whichever route made the earlier.**
        // A constructor's `depends_on` waits to be applied until the sample is
        // made, and applied then it replaced this one, made after it in
        // `__init__` to correct it: the correction was silently undone.
        file.declared.retain(|(name, _)| name != &output);
        if !file.declared_names.contains(&output) {
            file.declared_names.push(output);
        }
        Ok(())
    }

    /// What a value's formula is declared to read.
    ///
    /// Args:
    ///     output: A property's name, or ``table.column``.
    ///
    /// Returns:
    ///     The input names, sorted.
    ///
    /// Raises:
    ///     KeyError: An unknown name; the message names the nearest.
    ///
    /// Example:
    ///     >>> steel.dependencies("area")
    ///     ['thickness', 'width']
    fn dependencies(&self, py: Python<'_>, output: &str) -> PyResult<Vec<String>> {
        if let Some((table, column)) = output.split_once('.') {
            let table = bridge::identifier_from(py, table)?;
            let column = bridge::identifier_from(py, column)?;
            return self.shared().read_sample("the dependencies", |sample| {
                let held = sample.table(&table).or_raise(py)?;
                held.column(&column).or_raise(py)?;
                let mut inputs: Vec<String> = held
                    .inputs_of(&column)
                    .map(|inputs| inputs.iter().map(ToString::to_string).collect())
                    .unwrap_or_default();
                // Sorted, as the reference says: the declaration's order was
                // given.
                inputs.sort();
                Ok(inputs)
            });
        }
        let output = bridge::identifier_from(py, output)?;
        self.shared().read_sample("the dependencies", |sample| {
            known(py, sample, &Node::Named(output.clone()))?;
            let mut inputs: Vec<String> = sample
                .dependencies_of(&output)
                .or_raise(py)?
                .iter()
                .map(node_name)
                .collect();
            inputs.sort();
            Ok(inputs)
        })
    }

    /// The values whose formula reads this one directly.
    ///
    /// Args:
    ///     input: A property's name, or ``table.column``.
    ///
    /// Returns:
    ///     Their names, sorted.
    ///
    /// Raises:
    ///     KeyError: An unknown name; the message names the nearest.
    fn dependents(&self, py: Python<'_>, input: &str) -> PyResult<Vec<String>> {
        let node = match input_name(input)? {
            InputName::Named(name) => Node::Named(name),
            InputName::Column { table, column } => Node::Column { table, column },
            InputName::Cell(_) | InputName::CellValue(_) => {
                return Err(PyValueError::new_err("a property depends on no row's cell"));
            }
        };
        self.shared().read_sample("the dependents", |sample| {
            known(py, sample, &node)?;
            let direct = match &node {
                Node::Named(name) => sample.dependents_of(name).or_raise(py)?,
                Node::Column { .. } => sample.affected_by_change(&node).or_raise(py)?,
            };
            let mut names: Vec<String> = direct.iter().map(node_name).collect();
            names.sort();
            Ok(names)
        })
    }

    /// Every value a change to this one would make outdated, directly or through
    /// other values.
    ///
    /// Args:
    ///     input: A property's name, or ``table.column``.
    ///
    /// Returns:
    ///     Their names, sorted.
    ///
    /// Raises:
    ///     KeyError: An unknown name; the message names the nearest.
    ///
    /// Example:
    ///     >>> steel.affected_by_change("thickness")
    ///     ['area', 'curve.stress', 'tensile_strength', 'yield_strength', 'young_modulus']
    fn affected_by_change(&self, py: Python<'_>, input: &str) -> PyResult<Vec<String>> {
        let node = match input_name(input)? {
            InputName::Named(name) => Node::Named(name),
            InputName::Column { table, column } => Node::Column { table, column },
            InputName::Cell(_) | InputName::CellValue(_) => {
                return Err(PyValueError::new_err("a property depends on no row's cell"));
            }
        };
        self.shared()
            .read_sample("what a change affects", |sample| {
                known(py, sample, &node)?;
                let mut names: Vec<String> = sample
                    .affected_by_change(&node)
                    .or_raise(py)?
                    .iter()
                    .map(node_name)
                    .collect();
                names.sort();
                Ok(names)
            })
    }

    /// Compute the sample's values that are not current, in dependency order.
    ///
    /// Without names, every value a formula gives is considered; only those
    /// outdated or never computed run. A value whose inputs nobody entered waits,
    /// with a warning saying for what, and a formula that raises is recorded as
    /// failed while the others run. The file is not written: ``save()`` does
    /// that.
    ///
    /// Args:
    ///     *names: The values to compute: properties, ``table.column``, or a
    ///         table's name for all its computed columns. Their inputs run first
    ///         when needed.
    ///     rerun: Run them even when they are current.
    ///     force: Run them even when they are edited, giving them back to their
    ///         formula.
    ///
    /// Raises:
    ///     KeyError: An unknown name; the message names the nearest.
    ///     ValueError: A named value has no formula.
    ///     Exception: The first formula that raised, with a note naming the value
    ///         and the others that failed.
    ///
    /// Example:
    ///     >>> steel.compute()
    ///     >>> steel.save()
    #[pyo3(signature = (*names, rerun=false, force=false))]
    fn compute(
        &self,
        py: Python<'_>,
        names: &Bound<'_, PyTuple>,
        rerun: bool,
        force: bool,
    ) -> PyResult<()> {
        let written = names_of(names)?;
        compute_selected(
            py,
            &self.shared(),
            &self.file,
            written.as_deref(),
            rerun,
            force,
        )
    }

    /// The outdated values — those an input changed under since they were
    /// computed.
    ///
    /// Returns:
    ///     The names of the outdated properties, and ``table.column`` for a
    ///     column with an outdated cell.
    fn outdated(&self, py: Python<'_>) -> PyResult<Vec<String>> {
        let mut names = target_names(&stale_targets(py, &self.shared())?);
        // And what a changed formula outdates, as `state` and `status` say.
        for (value, said) in planned_of(&self.shared(), &self.file) {
            if said.contains("formula changed") && !names.contains(&value) {
                names.push(value);
            }
        }
        Ok(names)
    }

    /// Every value that is not current, and why: what ``samplekit status``
    /// reports.
    ///
    /// ``outdated()`` and ``edited()`` answer one question each; this one also
    /// sees values that failed, were never computed, or wait for an input.
    ///
    /// Returns:
    ///     A dict from each name — a property, or ``table.column`` — to a reason:
    ///     ``"outdated"``, ``"formula changed"``, ``"edited"``, ``"failed"``,
    ///     ``"never computed"`` or ``"waits for notch"``.
    ///
    /// Example:
    ///     >>> steel.not_current()
    ///     {}
    fn not_current<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let out = PyDict::new(py);
        // A formula changed since it computed a value, or one it reads: as
        // `status` says it, from the trace — current by its inputs was said.
        for (value, said) in planned_of(&self.shared(), &self.file) {
            if said.contains("formula changed") {
                out.set_item(value, said)?;
            }
        }
        self.shared().read_sample("the sample", |sample| {
            let states = fingerprint::check(sample).unwrap_or_default();
            for name in sample.property_names() {
                let name: Identifier = name.clone();
                let handle = sample.property(&name).or_raise(py)?;
                // A failure outranks *never computed*, as it does in `status`
                // and in `.state`: a formula that raised has not produced a
                // value either, and saying so would bury the reason.
                let said = states.get(&name).and_then(not_current_word);
                // What waits for an input nobody entered is said so, as
                // `status` says it: *never computed* promised a run that
                // `compute()` then declined.
                if said != Some("edited") && (said.is_some() || owed(&handle)) {
                    let waits = waits_for(sample, &Target::Property(name.clone()), &mut Vec::new());
                    if !waits.is_empty() {
                        out.set_item(name.to_string(), format!("waits for {}", waits.join(", ")))?;
                        continue;
                    }
                }
                if said == Some("failed") {
                    out.set_item(name.to_string(), "failed")?;
                    continue;
                }
                // Never computed is the model's to know, and `owed` is how the
                // plan knows it: a formula that has not produced what it owes.
                // One whose input failed waits behind it, as `status` says.
                if owed(&handle) {
                    let failed: Vec<String> = sample
                        .dependencies_of(&name)
                        .unwrap_or_default()
                        .iter()
                        .map(node_name)
                        .filter(|input| {
                            Identifier::new(input).ok().is_some_and(|input| {
                                states.get(&input).and_then(not_current_word) == Some("failed")
                            })
                        })
                        .collect();
                    if failed.is_empty() {
                        out.set_item(name.to_string(), "never computed")?;
                    } else {
                        let said: Vec<String> = failed
                            .iter()
                            .map(|input| format!("{input}, which failed"))
                            .collect();
                        out.set_item(name.to_string(), format!("waits for {}", said.join("; ")))?;
                    }
                    continue;
                }
                if let Some(said) = said {
                    out.set_item(name.to_string(), said)?;
                }
            }
            // **And every derived column**, as `status` lists it: once, as
            // `table.column`, with the gravest word any of its cells earns. The
            // tables were left out, so a script asking what to fix skipped
            // every broken column.
            for table in sample.table_names() {
                let held = sample.table(table).or_raise(py)?;
                for column in held.column_names() {
                    if !held.is_derived(column) && !is_statistic_column(held, column) {
                        continue;
                    }
                    let mut gravest: Option<&'static str> = None;
                    for tuple in held.index_tuples() {
                        let index: Vec<Value> = tuple.into_iter().cloned().collect();
                        let word = cell_word(py, sample, table, column, &index)?;
                        if let Some(word) = word
                            && gravity(word) > gravest.map_or(0, gravity)
                        {
                            gravest = Some(word);
                        }
                    }
                    if let Some(word) = gravest {
                        let target = Target::Column {
                            table: table.clone(),
                            column: column.clone(),
                        };
                        let waits = if word == "edited" {
                            Vec::new()
                        } else {
                            waits_for(sample, &target, &mut Vec::new())
                        };
                        if waits.is_empty() {
                            out.set_item(format!("{table}.{column}"), word)?;
                        } else {
                            out.set_item(
                                format!("{table}.{column}"),
                                format!("waits for {}", waits.join(", ")),
                            )?;
                        }
                    }
                }
            }
            Ok(())
        })?;
        Ok(out)
    }

    /// The command line's plan: each value a computation would run, with why,
    /// in dependency order. The model's worker's, and no script's. `changed`
    /// names the values whose formula changed since they were computed.
    #[pyo3(signature = (names, rerun, force, changed=Vec::new()))]
    fn _plan(
        &self,
        py: Python<'_>,
        names: Vec<String>,
        rerun: bool,
        force: bool,
        changed: Vec<String>,
    ) -> PyResult<Vec<(String, String)>> {
        let shared = self.shared();
        let named = (!names.is_empty()).then_some(names.as_slice());
        let chosen = selected_among(py, &shared, named, rerun, force, &changed)?;
        let reasoned = if named.is_some() {
            let pending = selected(py, &shared, None, false, false)?;
            let targets: Vec<Target> = chosen.iter().map(|(target, _)| target.clone()).collect();
            with_pending_inputs(py, &shared, targets)?
                .into_iter()
                .map(|target| {
                    let reason = chosen
                        .iter()
                        .chain(pending.iter())
                        .find(|(held, _)| *held == target)
                        .map_or("outdated", |(_, reason)| *reason);
                    (target, reason)
                })
                .collect()
        } else {
            chosen
        };
        shared.read_sample("the sample", |sample| Ok(planned(sample, reasoned)))
    }

    /// Each formula's declaration, by the value it gives: a list of `(role,
    /// object)` — `compute`, `compute_value` or `compute_uncertainty` with
    /// its function, `value` or `uncertainty` with the statistic declared,
    /// `compute_column` with its function, `unit`, and for a column what it
    /// reads and fills. The worker's, to take each formula's digest from.
    fn _formula_code<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let out = PyDict::new(py);
        for (name, parts) in &self.file.borrow().formula_code {
            let parts: Vec<(String, Py<PyAny>)> = parts
                .iter()
                .map(|(role, object)| (role.clone(), object.clone_ref(py)))
                .collect();
            out.set_item(name, parts)?;
        }
        Ok(out)
    }

    /// Runs one entry of the plan: a property, or the cells of a column the same
    /// choice selects.
    /// A value whose formula the worker stopped from outside, past
    /// `--timeout`: failed by `error`, as though the formula had raised it.
    /// A property, or every cell of a table's column; `False` for anything
    /// else, which is then only said.
    fn _fail_planned(
        &self,
        py: Python<'_>,
        value: &str,
        error: Bound<'_, PyAny>,
    ) -> PyResult<bool> {
        let failure = bridge::formula_failed(py, PyErr::from_value(error));
        let shared = self.shared();
        // A table's column: each of its cells failed, as a formula's raise
        // fails them; it was left as it was, its old cells read as current.
        if let Some((table, column)) = value.split_once('.') {
            let (Ok(table), Ok(column)) = (Identifier::new(table), Identifier::new(column)) else {
                return Ok(false);
            };
            let message = failure.to_string();
            let failed =
                shared.write(|sample| Ok(sample.fail_column(&table, &column, &message).is_ok()))?;
            if failed {
                shared.touch(COMPUTED);
            }
            return Ok(failed);
        }
        let Ok(name) = Identifier::new(value) else {
            return Ok(false);
        };
        let failed = shared.read_sample("the sample", |sample| {
            Ok(match sample.property(&name) {
                Ok(handle) => {
                    handle.peek(|property| property.fail_with(failure));
                    true
                }
                Err(_) => false,
            })
        })?;
        // Saved as a computation is, so that the failure is written though
        // nothing else is computed after it.
        if failed {
            shared.touch(COMPUTED);
        }
        Ok(failed)
    }

    fn _compute_planned(
        &self,
        py: Python<'_>,
        value: &str,
        rerun: bool,
        force: bool,
    ) -> PyResult<()> {
        let shared = self.shared();
        let property = shared.read_sample("the sample", |sample| {
            Ok(Identifier::new(value)
                .ok()
                .filter(|name| sample.has_property(name)))
        })?;
        let targets: Vec<Target> = match property {
            Some(name) => vec![Target::Property(name)],
            None => selected(py, &shared, Some(&[value.to_string()]), rerun, force)?
                .into_iter()
                .map(|(target, _)| target)
                .collect(),
        };
        if targets.is_empty() {
            return Ok(());
        }
        compute_now(py, &shared, &targets)
    }

    /// The edited values — those typed over their formula.
    ///
    /// Returns:
    ///     The names of the edited properties, and ``table.column`` for a column
    ///     with an edited cell.
    fn edited(&self) -> PyResult<Vec<String>> {
        self.shared().read_sample("the sample", |sample| {
            let mut names: Vec<String> = sample
                .property_names()
                .into_iter()
                .filter(|name| {
                    // Or a record marked edited: an override of a formula
                    // the sample was read without, as `status` names it.
                    sample.property(name).is_ok_and(|handle| {
                        handle.is_edited()
                            || handle.peek(Property::holds_written_override)
                            || handle
                                .records()
                                .fingerprint
                                .is_some_and(|digest| digest.is_edited())
                    })
                })
                .map(|name| name.to_string())
                .collect();
            for table in sample.table_names() {
                let Ok(held) = sample.table(table) else {
                    continue;
                };
                for column in held.column_names() {
                    let any = held.index_tuples().into_iter().any(|tuple| {
                        let address = RowAddress::Index(tuple.into_iter().cloned().collect());
                        held.is_held(&address, column).unwrap_or(false)
                    });
                    if any {
                        names.push(format!("{table}.{column}"));
                    }
                }
            }
            Ok(names)
        })
    }

    fn __getattr__(slf: &Bound<'_, Self>, name: &str) -> PyResult<Py<PyAny>> {
        // The message carries the nearest name already: Python adding its
        // own wrote the suggestion twice.
        (|| {
            if name.starts_with('_') {
                return Err(PyAttributeError::new_err(format!(
                    "'{}' object has no attribute '{name}'",
                    slf.get_type().name()?
                )));
            }
            PySample::get_among(
                slf,
                name,
                PyAttributeError::new_err,
                &members_of(slf.as_any()),
            )
        })()
        .map_err(bridge::said_in_full)
    }

    fn __getitem__(slf: &Bound<'_, Self>, name: &str) -> PyResult<Py<PyAny>> {
        // A name is the object it names; anything else the command line
        // addresses, `name`, `diameter.u`, `tempering.nitridation[50]`, is read as it
        // reads it.
        if name.trim().is_empty() {
            return Err(bridge::key_error(format!(
                "'{name}' is not a field path: an empty field name names nothing"
            )));
        }
        // What the sample carries by a name of its own is read as its
        // attribute is: `s["tags"]` was an unknown field beside `s.tags`.
        if matches!(name, "tags" | "name" | "note" | "path" | "filename") {
            return Ok(slf.getattr(name)?.unbind());
        }
        let reserved = crate::core::sample::RESERVED.contains(&name);
        if !reserved && Identifier::new(name).is_ok() {
            return PySample::get(slf, name, bridge::key_error::<String>);
        }
        PySample::field(slf, name)
    }

    fn __setattr__(slf: &Bound<'_, Self>, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        if name.starts_with('_') {
            return generic_setattr(slf.as_any(), name, Some(value));
        }
        refuse_while_a_formula_runs(&format!("'{name}'"))?;
        match name {
            "path" | "filename" => {
                return Err(PyAttributeError::new_err(format!(
                    "'{name}' is read-only: a sample moves to another file with save(path)"
                )));
            }
            "project" => {
                return Err(PyAttributeError::new_err(
                    "the project is read-only: what a project declares is changed in .samplekitrc",
                ));
            }
            "name" => {
                let name = if value.is_none() {
                    None
                } else {
                    Some(sample_name(value)?)
                };
                let shared = slf.borrow().shared();
                shared.touch("name");
                return shared.write(|sample| {
                    sample.set_name(name);
                    Ok(())
                });
            }
            "note" => {
                let note: String = value
                    .extract()
                    .map_err(|_| PyTypeError::new_err("a sample's note is a str"))?;
                let shared = slf.borrow().shared();
                shared.touch("note");
                return shared.write(|sample| {
                    sample.set_note(note);
                    Ok(())
                });
            }
            "tags" => {
                let tags = tags_from(value)?;
                // A tag given twice is carried once, and said, as one
                // appended again is: `tags = ['a', 'a']` said nothing.
                let given = value.try_iter()?.count();
                if given > tags.len() {
                    warn_value(
                        slf.py(),
                        &format!(
                            "{} given more than once: each is carried once",
                            if given - tags.len() == 1 {
                                "a tag is"
                            } else {
                                "tags are"
                            }
                        ),
                    )?;
                }
                let this = slf.borrow();
                let shared = this.shared();
                shared.touch("tags");
                shared.write(|sample| {
                    sample.set_tags(tags);
                    Ok(())
                })?;
                shared.advance(&tags_id());
                return Ok(());
            }
            _ => {}
        }
        let class = slf.get_type();
        if let Some(attribute) = class.getattr_opt(name)? {
            if attribute.get_type().hasattr("__set__")? {
                return generic_setattr(slf.as_any(), name, Some(value));
            }
            return Err(PyAttributeError::new_err(format!(
                "'{name}' is a method of {}: store data under that name with s[\"{name}\"] = ...",
                class.name()?
            )));
        }
        PySample::assign(slf, name, value)
    }

    fn __setitem__(slf: &Bound<'_, Self>, name: &str, value: &Bound<'_, PyAny>) -> PyResult<()> {
        refuse_while_a_formula_runs(&format!("'{name}'"))?;
        // A path is read, not assigned: named with the assignment that does
        // it, where the identifier's advice made an attribute `radius_v`.
        if name.contains(['.', '[']) {
            let head = name.split(['.', '[']).next().unwrap_or(name);
            let way = if name.contains('[') {
                format!(
                    "a row's cell is changed through its table: s.{head}.update(<index>, \
                     <column>=<value>)"
                )
            } else {
                let channel = name.split_once('.').map_or("value", |(_, rest)| rest);
                let channel = match channel {
                    "v" => "value",
                    "u" => "uncertainty",
                    other => other,
                };
                format!("a channel is assigned on its property: s.{head}.{channel} = <value>")
            };
            return Err(PyValueError::new_err(format!(
                "'{name}' is a field's path, which is read and not assigned — {way}"
            )));
        }
        PySample::assign(slf, name, value)
    }

    fn __delattr__(slf: &Bound<'_, Self>, name: &str) -> PyResult<()> {
        if name.starts_with('_') {
            return generic_setattr(slf.as_any(), name, None);
        }
        refuse_while_a_formula_runs(&format!("'{name}'"))?;
        if matches!(
            name,
            "path" | "filename" | "project" | "name" | "note" | "tags"
        ) {
            return Err(PyAttributeError::new_err(format!(
                "'{name}' cannot be deleted"
            )));
        }
        slf.borrow()
            .remove(slf.py(), name, PyAttributeError::new_err)
    }

    fn __delitem__(&self, py: Python<'_>, name: &str) -> PyResult<()> {
        self.remove(py, name, bridge::key_error::<String>)
    }

    fn __contains__(&self, name: &Bound<'_, PyAny>) -> PyResult<bool> {
        let Ok(name) = name.extract::<String>() else {
            return Ok(false);
        };
        let Ok(id) = Identifier::new(&name) else {
            return Ok(false);
        };
        Ok(self.lookup(&id)?.is_some())
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.names()?)?.try_iter()
    }

    /// The names of the sample's attributes, properties and tables, in the order
    /// they were declared.
    fn keys<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        PySample::as_dict(slf)?.call_method0("keys")
    }

    /// Each name with its attribute, property or table, in the order they were
    /// declared.
    fn items<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyAny>> {
        PySample::as_dict(slf)?.call_method0("items")
    }

    fn __dir__(slf: &Bound<'_, Self>) -> PyResult<Vec<String>> {
        let py = slf.py();
        let mut names: Vec<String> = builtin(py, "object")?
            .getattr("__dir__")?
            .call1((slf,))?
            .extract()?;
        for name in slf.borrow().names()? {
            let usable = Identifier::new(&name).is_ok_and(|id| id.is_python_attribute());
            if usable && !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn __repr__(slf: &Bound<'_, Self>) -> PyResult<String> {
        let this = slf.borrow();
        let class = slf.get_type().name()?.to_string();
        let name = this.name()?;
        let file = this.file.borrow().path.clone();
        Ok(match (name, file) {
            (Some(name), Some(file)) => format!("<{class} '{name}' {}>", file.display()),
            (Some(name), None) => format!("<{class} '{name}', not saved>"),
            (None, Some(file)) => format!("<{class} {}>", file.display()),
            (None, None) => format!("<{class}, not saved>"),
        })
    }
}

impl PySample {
    fn as_dict<'py>(slf: &Bound<'py, Self>) -> PyResult<Bound<'py, PyDict>> {
        let py = slf.py();
        let dict = PyDict::new(py);
        let names = slf.borrow().names()?;
        for name in names {
            dict.set_item(
                &name,
                PySample::get(slf, &name, bridge::key_error::<String>)?,
            )?;
        }
        Ok(dict)
    }
}

/// A loaded sample in the package's `Sample` class, for a list read from a
/// directory without a model.
fn wrap_loaded(
    py: Python<'_>,
    sample: Sample,
    path: PathBuf,
    origin: Origin,
    config: Option<Rc<ProjectConfig>>,
) -> PyResult<Py<PyAny>> {
    let object = bridge::sample_class(py)?.call0()?;
    {
        let cell = object.cast::<PySample>()?;
        let this = cell.borrow();
        *this.shared.borrow_mut() = SharedSample::new(sample);
        let mut file = this.file.borrow_mut();
        file.read = std::fs::read_to_string(&path)
            .ok()
            .filter(|text| document::digest_of(text) == origin.digest);
        file.path = Some(path);
        file.origin = Some(origin);
        file.config = config;
    }
    Ok(object.unbind())
}

// ------------------------------------------------------------- SampleList

// ---------------------------------------------------------- recomputing

/// What a recomputation runs.
#[derive(Clone, PartialEq)]
pub enum Target {
    Property(Identifier),
    Column {
        table: Identifier,
        column: Identifier,
    },
    Cell {
        table: Identifier,
        index: Vec<Value>,
        column: Identifier,
    },
}

/// Readings with no statistic chosen for them: their state, said as the
/// reason nothing runs — which statistic stands for them is the model's to
/// choose, and without one the number written beside them is only written.
fn no_statistic(handle: &PropertyHandle, name: &str) -> Option<PyErr> {
    handle.readings()?;
    // No mean is offered in its place: a value is the declared statistic's, or
    // one written.
    Some(PyValueError::new_err(format!(
        "'{name}' has readings and no statistic chosen for them: nothing computes \
         its value from them\n  a model chooses one, sk.Property(value=sk.stats.mean, \
         …), or a value written beside them stands for them"
    )))
}

fn no_formula(what: &str) -> PyErr {
    PyValueError::new_err(format!(
        "{what} has no formula to run: its value was entered or read from a file, not derived"
    ))
}

/// Invalidates every target, then reads every target: a target another one
/// reads is computed before it is read, never served stale. Refused before
/// anything is invalidated when a target has no formula.
pub fn compute_now(py: Python<'_>, shared: &SharedSample, targets: &[Target]) -> PyResult<()> {
    prepare(py, shared, targets)?;
    let before = bridge::formula_runs();
    for target in targets {
        run_target(py, shared, target)?;
    }
    bridge::record_computed(shared, before);
    bridge::forget_raised();
    Ok(())
}

/// Every target checked to have a formula, and its column invalidated, before
/// any runs: a formula giving two columns then runs once for both.
fn prepare(py: Python<'_>, shared: &SharedSample, targets: &[Target]) -> PyResult<()> {
    shared.write(|sample| {
        for target in targets {
            match target {
                Target::Property(name) => {
                    let handle = sample.property(name).or_raise(py)?;
                    // A declared statistic runs no Python, so it is not a
                    // formula here — but a value written over it is the trial
                    // `--force` undoes, and refusing it was what made the plan
                    // announce a restore it then declined to perform.
                    if !handle.is_computed()
                        && !handle.has_uncertainty_formula()
                        && !handle.peek(Property::has_declared_statistic)
                    {
                        return Err(no_statistic(&handle, name.as_str())
                            .unwrap_or_else(|| no_formula(&format!("'{name}'"))));
                    }
                }
                Target::Column { table, column } | Target::Cell { table, column, .. } => {
                    let held = sample.table(table).or_raise(py)?;
                    held.column(column).or_raise(py)?;
                    if !held.is_derived(column) && !is_statistic_column(held, column) {
                        return Err(no_formula(&format!("'{table}.{column}'")));
                    }
                }
            }
        }
        // Every target is invalidated before any runs, and a column alone: its
        // derivation, not the table's others.
        for target in targets {
            // A statistic of a cell's readings runs no Python: named, it is
            // taken again from them here, and nothing is left to resolve.
            if let Target::Column { table, column } | Target::Cell { table, column, .. } = target
                && sample
                    .table(table)
                    .is_ok_and(|held| is_statistic_column(held, column))
            {
                let rows: Vec<RowAddress> = match target {
                    Target::Cell { index, .. } => vec![RowAddress::Index(index.clone())],
                    _ => sample
                        .table(table)
                        .or_raise(py)?
                        .index_tuples()
                        .into_iter()
                        .map(|tuple| RowAddress::Index(tuple.into_iter().cloned().collect()))
                        .collect(),
                };
                shared.touch(COMPUTED);
                for row in rows {
                    sample.retake_statistic(table, &row, column).or_raise(py)?;
                    // Recorded at once, as a computed value is when it runs:
                    // the statistic taken is current before any save.
                    fingerprint::record_cell_statistic(sample, table, &row, column).or_raise(py)?;
                }
                continue;
            }
            match target {
                // About to run again: what it gives is in the unit the model
                // declares now.
                Target::Property(name) => sample.property(name).or_raise(py)?.adopt_declared_unit(),
                Target::Column { table, column } => {
                    // Named, an override gives way to its formula.
                    sample
                        .release_cells(table, column, None)
                        .and_then(|()| sample.invalidate_column(table, column, None))
                        .or_raise(py)?
                }
                Target::Cell {
                    table,
                    column,
                    index,
                } => {
                    // Named, an override gives way to its formula.
                    let address = RowAddress::Index(index.clone());
                    sample
                        .release_cells(table, column, Some(&address))
                        .and_then(|()| sample.invalidate_column(table, column, Some(&address)))
                        .or_raise(py)?
                }
            }
        }
        Ok(())
    })
}

/// One target run, its failure raised as the formula's own exception.
fn run_target(py: Python<'_>, shared: &SharedSample, target: &Target) -> PyResult<()> {
    // A computation stamps the whole sample: the next save writes it whole.
    shared.touch(COMPUTED);
    {
        // A property is invalidated as it is reached: a failure raises, and what
        // comes after it keeps the value it had, stale, rather than none.
        if let Target::Property(name) = target {
            shared.write(|sample| {
                // Named, an override gives way to its formula.
                let handle = sample.property(name).or_raise(py)?;
                handle.restore_formula();
                handle.invalidate();
                Ok(())
            })?;
        }
        // The one place a formula runs from Python: a computation asked for.
        let ran = crate::core::property::computing(|| -> PyResult<()> {
            match target {
                Target::Property(name) => {
                    let handle = shared.read(
                        |sample| sample.property(name).or_raise(py),
                        |snapshot| snapshot.properties.get(name).cloned().ok_or_else(busy),
                    )?;
                    // Readings whose statistic nothing took yet: taken here,
                    // value and spread, as `compute` on the command line takes
                    // them. The value came, its uncertainty did not.
                    if statistic_owed(&handle) {
                        handle.take_statistic();
                    }
                    handle.value().or_raise(py)?;
                    handle.uncertainty().or_raise(py)?;
                }
                Target::Column { table, column } | Target::Cell { table, column, .. } => {
                    // A statistic column was taken again as it was prepared.
                    let statistic = shared.read_sample("a table", |sample| {
                        Ok(sample
                            .table(table)
                            .is_ok_and(|held| is_statistic_column(held, column)))
                    })?;
                    if statistic {
                        return Ok(());
                    }
                    resolve_with_upstream(py, shared, table, Some(std::slice::from_ref(column)))?;
                }
            }
            Ok(())
        });
        if let Err(error) = ran {
            // The formula's own exception, its type and traceback unchanged,
            // with a note saying which value it was computing: a bare
            // `ZeroDivisionError` out of a computation of forty values named
            // nothing a script could act on.
            let _ = error.value(py).call_method1(
                "add_note",
                (format!(
                    "while computing '{}'",
                    target_names(std::slice::from_ref(target)).join("")
                ),),
            );
            // Kept for errors that carry a failure only as text; this one is
            // being raised as itself, so nothing needs it held — and holding it
            // kept the formula's frames and locals alive until the next failure.
            bridge::forget_raised();
            return Err(error);
        }
    }
    Ok(())
}

/// What `compute(names)` names: a property, `table.column`, or a table,
/// meaning every column a formula fills.
fn named_targets(sample: &Sample, names: &[String]) -> PyResult<Vec<Target>> {
    let mut targets = Vec::new();
    for written in names {
        if let Ok(name) = Identifier::new(written) {
            if sample.has_property(&name) {
                targets.push(Target::Property(name));
                continue;
            }
            if sample.has_attribute(&name) {
                return Err(no_formula(&format!("the attribute '{name}'")));
            }
            if let Ok(table) = sample.table(&name) {
                let derived: Vec<Identifier> = table
                    .column_names()
                    .into_iter()
                    .filter(|column| table.is_derived(column) || is_statistic_column(table, column))
                    .cloned()
                    .collect();
                if derived.is_empty() {
                    return Err(no_formula(&format!("the table '{name}'")));
                }
                targets.extend(derived.into_iter().map(|column| Target::Column {
                    table: name.clone(),
                    column,
                }));
                continue;
            }
        }
        if let Some((table, column)) = written.split_once('.')
            && let (Ok(table), Ok(column)) = (Identifier::new(table), Identifier::new(column))
            && sample
                .table(&table)
                .is_ok_and(|held| held.column(&column).is_ok())
        {
            targets.push(Target::Column { table, column });
            continue;
        }
        let mut available: Vec<String> = sample
            .property_names()
            .into_iter()
            .map(|name| name.to_string())
            .collect();
        for table in sample.table_names() {
            available.push(table.to_string());
            if let Ok(held) = sample.table(table) {
                available.extend(
                    held.column_names()
                        .into_iter()
                        .filter(|column| {
                            held.is_derived(column) || is_statistic_column(held, column)
                        })
                        .map(|column| format!("{table}.{column}")),
                );
            }
        }
        return Err(bridge::key_error(unknown_message(
            "value", written, &available,
        )));
    }
    Ok(targets)
}

/// Every value a formula gives, overrides included: the scope of a computation
/// that names nothing.
fn formula_targets(sample: &Sample) -> Vec<Target> {
    let mut targets: Vec<Target> = sample
        .property_names()
        .into_iter()
        .filter(|name| {
            sample.property(name).is_ok_and(|handle| {
                handle.is_computed()
                    || handle.has_uncertainty_formula()
                    // A declared statistic runs no Python and is a derivation
                    // all the same: stale when a reading is corrected, an
                    // override when a value is written over it. A scope that
                    // left it out could run neither.
                    || handle.peek(Property::has_declared_statistic)
            })
        })
        .map(|name| Target::Property(name.clone()))
        .collect();
    for table in sample.table_names() {
        if let Ok(held) = sample.table(table) {
            targets.extend(
                held.column_names()
                    .into_iter()
                    .filter(|column| held.is_derived(column) || is_statistic_column(held, column))
                    .map(|column| Target::Column {
                        table: table.clone(),
                        column: column.clone(),
                    }),
            );
        }
    }
    targets
}

/// A value its formula owes a first run: the value, or an uncertainty a formula
/// gives beside it. The word `status` uses for a state that is not current, or
/// `None` when it is. One vocabulary for both surfaces, or a script and a
/// terminal describe the same file differently. What one derived cell is, in
/// `not_current`'s words: a failure first, as for a property, then *never
/// computed*, then its verdict.
fn cell_word(
    py: Python<'_>,
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
) -> PyResult<Option<&'static str>> {
    let address = RowAddress::Index(index.to_vec());
    let failed = sample
        .table(table)
        .ok()
        .and_then(|held| held.at(&address, column).ok())
        .is_some_and(|cell| cell.peek_failure().is_some() || cell.records().failure.is_some());
    if failed {
        return Ok(Some("failed"));
    }
    if sample.is_cell_held(table, &address, column).or_raise(py)? {
        return Ok(Some("edited"));
    }
    if sample.cell_run(table, &address, column).or_raise(py)? == CellRun::NeverRan {
        return Ok(Some("never computed"));
    }
    let verdict = fingerprint::check_cell(sample, table, column, index).or_raise(py)?;
    Ok(not_current_word(&verdict))
}

/// Which word a column takes when its cells differ: the one that most needs
/// doing something about.
fn gravity(word: &str) -> u8 {
    match word {
        "failed" => 4,
        "outdated" => 3,
        "never computed" => 2,
        "edited" => 1,
        _ => 0,
    }
}

fn not_current_word(state: &fingerprint::Freshness) -> Option<&'static str> {
    match state {
        fingerprint::Freshness::Source | fingerprint::Freshness::Current => None,
        fingerprint::Freshness::Edited | fingerprint::Freshness::RecordMissing => Some("edited"),
        fingerprint::Freshness::Failed { .. } => Some("failed"),
        fingerprint::Freshness::Stale { .. }
        | fingerprint::Freshness::Broken { .. }
        | fingerprint::Freshness::Unjudged { .. } => Some("outdated"),
    }
}

/// A value not current: *waiting* where an input it reads was emptied since —
/// its number the one computed before — as `status` says it, else *outdated*.
fn outdated_or_waiting(sample: &Sample, state: &fingerprint::Freshness) -> &'static str {
    if crate::collection::editing::emptied_inputs(sample, state).is_empty() {
        "outdated"
    } else {
        "waiting"
    }
}

fn owed(handle: &PropertyHandle) -> bool {
    (handle.is_computed() && handle.peek(Property::peek_value).is_none())
        || (handle.has_uncertainty_formula() && handle.peek(Property::peek_uncertainty).is_none())
        || statistic_owed(handle)
}

/// Readings whose declared statistic nothing has taken yet: no record of it,
/// and no value written beside them. The model gives the value as it reads
/// them, running nothing, but the file holds none — a table shows *—* — until a
/// computation writes it: *never computed*, as a cell of a statistic column is,
/// on every surface. The demo's blonde steel had its hardnesses *—* in the
/// table and in no list of `status`.
fn statistic_owed(handle: &PropertyHandle) -> bool {
    handle.records().computed.is_none()
        && handle.peek(|property| {
            property.has_declared_statistic()
                && property.readings().is_some()
                && property.written_value().is_none()
                // One the file states is read as it stands: owed, it was
                // *never computed* beside the value every table showed.
                && !property.statistic_taken()
        })
}

/// A statistic the file states where the model declares another: the
/// computation takes the model's, and says so.
fn statistic_stated_otherwise(handle: &PropertyHandle) -> bool {
    handle.peek(|property| property.stated_statistic() != (None, None))
}

/// What a computation changes of a statistic a file states otherwise.
fn statistic_change(sample: &Sample, label: &str) -> Option<String> {
    let name = Identifier::new(label).ok()?;
    let handle = sample.property(&name).ok()?;
    handle.peek(Property::statistic_change_said)
}

/// What a computation runs, each with why. In its scope — the values named, or
/// every value a formula gives — a value stale or never computed, always; a
/// current one with `rerun`; an override with `force`, and only then.
fn selected(
    py: Python<'_>,
    shared: &SharedSample,
    names: Option<&[String]>,
    rerun: bool,
    force: bool,
) -> PyResult<Vec<(Target, &'static str)>> {
    selected_among(py, shared, names, rerun, force, &[])
}

/// [`selected`], where `changed` names the values whose formula changed since
/// they were computed, as the worker found them: a current one among them runs,
/// as *formula changed*, and a column's every cell with it.
fn selected_among(
    py: Python<'_>,
    shared: &SharedSample,
    names: Option<&[String]>,
    rerun: bool,
    force: bool,
    changed: &[String],
) -> PyResult<Vec<(Target, &'static str)>> {
    let formula_changed = |label: String| changed.contains(&label);
    // A cell current by its inputs runs when its column's formula changed.
    let changed_cell = |reason: Option<&'static str>, changed: bool| match reason {
        Some("current") if changed => Some("formula changed"),
        other => other,
    };
    shared.read_sample("the sample", |sample| {
        // Every cell asked about in one pass: a column is hashed once, and
        // what the cells read is judged once.
        let _pass = fingerprint::Pass::begin();
        let _verdicts = fingerprint::Verdicts::begin();
        let scope = match names {
            Some(names) => named_targets(sample, names)?,
            None => formula_targets(sample),
        };
        let mut chosen = Vec::new();
        for target in scope {
            match target {
                Target::Property(name) => {
                    let handle = sample.property(&name).or_raise(py)?;
                    if !handle.is_computed()
                        && !handle.has_uncertainty_formula()
                        && !handle.peek(Property::has_declared_statistic)
                    {
                        return Err(no_statistic(&handle, name.as_str())
                            .unwrap_or_else(|| no_formula(&format!("'{name}'"))));
                    }
                    let reason =
                        if handle.is_edited() || handle.peek(Property::holds_written_override) {
                            force.then_some("edited")
                        } else if owed(&handle) {
                            Some("never computed")
                        } else if statistic_stated_otherwise(&handle) {
                            Some("statistic")
                        } else if fingerprint::is_stale(sample, &name).or_raise(py)? {
                            Some("outdated")
                        } else if formula_changed(name.to_string()) {
                            Some("formula changed")
                        } else {
                            rerun.then_some("current")
                        };
                    if let Some(reason) = reason {
                        chosen.push((Target::Property(name), reason));
                    }
                }
                Target::Column { table, column } => {
                    let held = sample.table(&table).or_raise(py)?;
                    held.column(&column).or_raise(py)?;
                    if !held.is_derived(&column) && !is_statistic_column(held, &column) {
                        return Err(no_formula(&format!("'{table}.{column}'")));
                    }
                    let indexes: Vec<Vec<Value>> = held
                        .index_tuples()
                        .into_iter()
                        .map(|tuple| tuple.into_iter().cloned().collect())
                        .collect();
                    let changed = formula_changed(format!("{table}.{column}"));
                    for index in indexes {
                        if let Some(reason) = changed_cell(
                            cell_reason(
                                py,
                                sample,
                                &table,
                                &column,
                                &index,
                                rerun || changed,
                                force,
                            )?,
                            changed,
                        ) {
                            chosen.push((
                                Target::Cell {
                                    table: table.clone(),
                                    index,
                                    column: column.clone(),
                                },
                                reason,
                            ));
                        }
                    }
                }
                Target::Cell {
                    table,
                    index,
                    column,
                } => {
                    let changed = formula_changed(format!("{table}.{column}"));
                    if let Some(reason) = changed_cell(
                        cell_reason(py, sample, &table, &column, &index, rerun || changed, force)?,
                        changed,
                    ) {
                        chosen.push((
                            Target::Cell {
                                table,
                                index,
                                column,
                            },
                            reason,
                        ));
                    }
                }
            }
        }
        Ok(chosen)
    })
}

/// Why one derived cell runs, if it does: never ran, stale, or current with
/// `rerun`. A cell is written, never overridden.
fn cell_reason(
    py: Python<'_>,
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
    rerun: bool,
    force: bool,
) -> PyResult<Option<&'static str>> {
    let address = RowAddress::Index(index.to_vec());
    if sample
        .table(table)
        .is_ok_and(|held| is_statistic_column(held, column))
    {
        return statistic_cell_reason(py, sample, table, column, index, rerun, force);
    }
    // An override runs only when forced, and then gives way.
    if sample.is_cell_held(table, &address, column).or_raise(py)? {
        return Ok(force.then_some("edited"));
    }
    // A cell its formula failed on runs again, said failed as `status` says
    // it: listed *never computed*, the run then contradicted the list.
    if sample.table(table).is_ok_and(|held| {
        held.at(&address, column)
            .is_ok_and(|cell| cell.records().failure.is_some())
    }) {
        return Ok(Some("failed"));
    }
    if sample.cell_run(table, &address, column).or_raise(py)? == CellRun::NeverRan {
        return Ok(Some("never computed"));
    }
    if fingerprint::is_cell_stale(sample, table, column, index).or_raise(py)? {
        return Ok(Some("outdated"));
    }
    Ok(rerun.then_some("current"))
}

/// A column no formula fills whose cells' readings stand for their values
/// through the statistics it declares: a derivation that runs no Python, as a
/// property's declared statistic is.
fn is_statistic_column(held: &Table, column: &Identifier) -> bool {
    !held.is_derived(column)
        && held
            .column(column)
            .is_ok_and(|view| !view.statistics().is_empty())
}

/// Why one cell of a statistic column is taken again, if it is: its readings
/// moved since the statistic was recorded, or nothing recorded one yet. A value
/// written beside them outranks the statistic, as over a property's, and gives
/// way only when forced; a cell without readings has nothing to take.
fn statistic_cell_reason(
    py: Python<'_>,
    sample: &Sample,
    table: &Identifier,
    column: &Identifier,
    index: &[Value],
    rerun: bool,
    force: bool,
) -> PyResult<Option<&'static str>> {
    let address = RowAddress::Index(index.to_vec());
    let held = sample.table(table).or_raise(py)?;
    let cell = held.at(&address, column).or_raise(py)?;
    if cell.readings().is_none() {
        return Ok(None);
    }
    if cell.records().computed.is_none() {
        return Ok(if cell.written_value().is_some() {
            force.then_some("edited")
        } else {
            Some("never computed")
        });
    }
    Ok(
        match fingerprint::check_cell(sample, table, column, index).or_raise(py)? {
            fingerprint::Freshness::Edited => force.then_some("edited"),
            fingerprint::Freshness::Current => rerun.then_some("current"),
            _ => Some("outdated"),
        },
    )
}

/// Where a property no sample holds stands: it has no inputs to be stale against.
/// A value typed in, or none typed: `"absent"`, as JSON says it, where it
/// holds neither a value nor readings — *entered* was said of nothing.
fn entered_or_absent(property: &Property) -> &'static str {
    let held = property
        .peek_value()
        .is_some_and(|value| !value.is_absent())
        || property.readings().is_some();
    if held { "entered" } else { "absent" }
}

fn detached_state(property: &Property) -> &'static str {
    if property.is_edited() {
        "edited"
    } else if !property.is_computed() && !property.has_uncertainty_formula() {
        entered_or_absent(property)
    } else if property.has_failed() {
        "failed"
    } else if (property.is_computed() && property.peek_value().is_none())
        || (property.has_uncertainty_formula() && property.peek_uncertainty().is_none())
    {
        "never computed"
    } else {
        "current"
    }
}

fn names_of(names: &Bound<'_, PyTuple>) -> PyResult<Option<Vec<String>>> {
    if names.is_empty() {
        return Ok(None);
    }
    names
        .iter()
        .map(|name| {
            name.extract::<String>()
                .map_err(|_| PyTypeError::new_err("compute takes names, as str"))
        })
        .collect::<PyResult<Vec<_>>>()
        .map(Some)
}

/// What the model owes each value of the sample, as `status` plans it from
/// the model's description — described again where it is not current, a
/// model edited or pulled since — and the project's trace; nothing where the
/// sample has no file.
fn planned_of(shared: &SharedSample, file: &FileRef) -> std::collections::BTreeMap<String, String> {
    let (path, config, computed) = {
        let file = file.borrow();
        (
            file.path.clone(),
            file.config.clone(),
            file.computed.clone(),
        )
    };
    let (Some(path), Some(config)) = (path, config) else {
        return Default::default();
    };
    let mut planned = shared
        .read_sample("the sample", |sample| {
            Ok(crate::collection::computation::planned_states_described(
                Some(&config),
                &path,
                sample,
            )
            .unwrap_or_default())
        })
        .unwrap_or_default();
    // What `compute()` has just run is its formula's now, before the save
    // that keeps it so.
    planned.retain(|value, _| !computed.contains(value));
    planned
}

/// [`planned_of`] by the model's description as it stands, never described
/// again: no Python starts, and no model is imported.
fn planned_now(
    shared: &SharedSample,
    file: &FileRef,
) -> std::collections::BTreeMap<String, String> {
    let (path, config, computed) = {
        let file = file.borrow();
        (
            file.path.clone(),
            file.config.clone(),
            file.computed.clone(),
        )
    };
    let (Some(path), Some(config)) = (path, config) else {
        return Default::default();
    };
    let mut planned = shared
        .read_sample("the sample", |sample| {
            Ok(
                crate::collection::computation::planned_states(Some(&config), &path, sample)
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default();
    planned.retain(|value, _| !computed.contains(value));
    planned
}

/// The values whose own formula changed since they were computed.
fn changed_formulas(shared: &SharedSample, file: &FileRef) -> Vec<String> {
    planned_of(shared, file)
        .into_iter()
        .filter(|(_, said)| said == "formula changed")
        .map(|(value, _)| value)
        .collect()
}

/// A computation as `Sample.compute` and `Property.compute` ask for one.
fn compute_selected(
    py: Python<'_>,
    shared: &SharedSample,
    file: &FileRef,
    names: Option<&[String]>,
    rerun: bool,
    force: bool,
) -> PyResult<()> {
    // A formula assigned after the sample was made is refused here if it never
    // said what it reads, as one declared in `__init__` is refused at
    // construction.
    refuse_undeclared(py, shared, file)?;
    // A formula changed since it computed a value runs, as `samplekit compute`
    // runs it: the trace says which.
    let changed = changed_formulas(shared, file);
    let chosen: Vec<Target> = selected_among(py, shared, names, rerun, force, &changed)?
        .into_iter()
        .map(|(target, _)| target)
        .collect();
    // Readings whose statistic nothing took yet — given in this session, a
    // quantity's first — taken now, value and spread, as `samplekit compute`
    // takes them: the value came, and its uncertainty stayed `None`.
    shared.write(|sample| {
        for name in sample
            .property_names()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            let asked = names.is_none_or(|names| names.iter().any(|named| named == name.as_str()));
            if asked
                && let Ok(handle) = sample.property(&name)
                && statistic_owed(&handle)
            {
                handle.take_statistic();
            }
        }
        Ok(())
    })?;
    // As `samplekit compute` does: a failure does not stop the others, and a
    // formula over an input nobody entered waits rather than runs.
    let mut run = Run::default();
    if names.is_some() {
        let targets = with_pending_inputs(py, shared, chosen)?;
        if !targets.is_empty() {
            let targets = in_dependency_order(shared, targets)?;
            run.each(py, shared, &targets)?;
            // A statistic of readings runs no Python, so no formula's run
            // records it: recorded here, as a whole computation does, or the
            // value named stayed *never computed* once taken.
            shared.write(|sample| {
                fingerprint::stamp(sample).ok();
                Ok(())
            })?;
        }
        return run.said(py, shared, file);
    }
    // A statistic of readings runs no Python, so no formula's run records
    // it: recorded after each round, as for values named, or it stayed
    // pending, and the run gave up *after 11 rounds* on a sample begun.
    let stamp = || {
        shared.write(|sample| {
            fingerprint::stamp(sample).ok();
            Ok(())
        })
    };
    if !chosen.is_empty() {
        let chosen = in_dependency_order(shared, chosen)?;
        run.each(py, shared, &chosen)?;
        stamp()?;
    }
    // A computed value can stale what reads it, so this runs until nothing is
    // pending but what waits or failed; each round computes only what needs it.
    let limit = shared.read_sample("the sample", |sample| Ok(formula_targets(sample).len()))? + 1;
    let mut before: Option<Vec<String>> = None;
    for _ in 0..=limit {
        let targets: Vec<Target> = pending_targets(py, shared)?
            .into_iter()
            .filter(|target| !run.settled(target))
            .collect();
        if targets.is_empty() {
            return run.said(py, shared, file);
        }
        // A round that leaves pending what the one before did computes
        // nothing more: what is left waits, as `samplekit compute` says.
        let names = target_names(&targets);
        if before.as_ref() == Some(&names) {
            run.waiting.extend(
                names
                    .into_iter()
                    .map(|name| (name, vec!["what it reads".to_string()])),
            );
            return run.said(py, shared, file);
        }
        before = Some(names);
        let targets = in_dependency_order(shared, targets)?;
        run.each(py, shared, &targets)?;
        stamp()?;
    }
    Err(PyRuntimeError::new_err(format!(
        "these values still need computing after {limit} rounds: {}",
        target_names(&pending_targets(py, shared)?).join(", ")
    )))
}

/// A computation from Python, as `samplekit compute` runs one: what failed,
/// with its exception, and what waits for an input nobody entered.
#[derive(Default)]
struct Run {
    failed: Vec<(String, PyErr)>,
    waiting: Vec<(String, Vec<String>)>,
    /// What ran and gave its value.
    ran: Vec<String>,
}

impl Run {
    /// Whether a value is settled for this run: failed, or waiting.
    fn settled(&self, target: &Target) -> bool {
        let name = target_names(std::slice::from_ref(target)).join("");
        self.failed.iter().any(|(failed, _)| *failed == name)
            || self.waiting.iter().any(|(waiting, _)| *waiting == name)
    }

    /// Each target in turn: one whose input is empty waits, one that fails is
    /// noted, and the others run.
    fn each(&mut self, py: Python<'_>, shared: &SharedSample, targets: &[Target]) -> PyResult<()> {
        // Checked and invalidated together, as `compute_now` does: a formula
        // giving two columns runs once.
        let runnable: Vec<Target> = targets
            .iter()
            .filter(|target| {
                !self.settled(target)
                    && shared
                        .read_sample("the sample", |sample| {
                            Ok(missing_inputs(sample, target, &[]).is_empty())
                        })
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        prepare(py, shared, &runnable)?;
        let before = bridge::formula_runs();
        for target in targets {
            let name = target_names(std::slice::from_ref(target)).join("");
            if self.settled(target) {
                continue;
            }
            let unavailable: Vec<String> = self
                .failed
                .iter()
                .map(|(name, _)| name.clone())
                .chain(self.waiting.iter().map(|(name, _)| name.clone()))
                .collect();
            let missing = shared.read_sample("the sample", |sample| {
                Ok(missing_inputs(sample, target, &unavailable))
            })?;
            if !missing.is_empty() {
                self.waiting.push((name, missing));
                continue;
            }
            match run_target(py, shared, target) {
                Ok(()) => self.ran.push(name),
                Err(error) => self.failed.push((name, error)),
            }
        }
        bridge::record_computed(shared, before);
        bridge::forget_raised();
        Ok(())
    }

    /// The run said: what waits as a warning, and the first failure raised
    /// with the others named — each already recorded as failed.
    fn said(mut self, py: Python<'_>, shared: &SharedSample, file: &FileRef) -> PyResult<()> {
        {
            let mut held = file.borrow_mut();
            for name in self.ran.drain(..) {
                if !held.computed.contains(&name) {
                    held.computed.push(name);
                }
            }
        }
        if !self.waiting.is_empty() {
            let failed: Vec<String> = self.failed.iter().map(|(name, _)| name.clone()).collect();
            let said: Vec<String> = self
                .waiting
                .iter()
                .map(|(name, inputs)| waiting_phrase(name, inputs, &failed))
                .collect();
            // Named for its sample, as every other warning about a value is:
            // two samples waiting alike wrote one text, and Python's filter
            // shows a text once.
            warn_value(py, &format!("{}{}", whose(shared, file), said.join("; ")))?;
        }
        if self.failed.is_empty() {
            return Ok(());
        }
        let (_, first) = self.failed.remove(0);
        if !self.failed.is_empty() {
            let others: Vec<String> = self.failed.iter().map(|(name, _)| name.clone()).collect();
            let _ = first.value(py).call_method1(
                "add_note",
                (format!(
                    "also failed, the others computed: {}",
                    others.join(", ")
                ),),
            );
        }
        Err(first)
    }
}

/// The inputs of a target nobody entered: a value absent that no formula of
/// this run will give — one that failed or waits — or, for an uncertainty a
/// formula gives beside an entered value, that value itself.
fn missing_inputs(sample: &Sample, target: &Target, unavailable: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    let own_table = match target {
        Target::Property(_) => None,
        Target::Column { table, .. } | Target::Cell { table, .. } => Some(table),
    };
    for input in node_inputs(sample, &target_node(target)) {
        let name = match input {
            Node::Named(name) => name,
            // A column of another table with no value in it is an input nobody
            // entered, as the command line judges it: a inspection table without a
            // row made a mean score of nothing raise, where `samplekit compute`
            // said the score waits. The target's own table is read row by row,
            // and a row being computed exists.
            Node::Column { table, column } => {
                let spelled = format!("{table}.{column}");
                // A formula of the target's own table giving its column row by
                // row reads each row, and a row being computed exists.
                let row_by_row = own_table == Some(&table)
                    && match target {
                        Target::Column { column: own, .. } | Target::Cell { column: own, .. } => {
                            sample
                                .table(&table)
                                .is_ok_and(|held| held.is_derived_by_rows(own))
                        }
                        Target::Property(_) => false,
                    };
                if row_by_row {
                    // The row's own cell it reads, where nobody entered it:
                    // what the cell waits for, named — it was said to wait
                    // for nothing, and its run gave up.
                    if let Target::Cell { index, .. } = target
                        && sample
                            .table(&table)
                            .ok()
                            .and_then(|held| {
                                held.at(&RowAddress::Index(index.clone()), &column).ok()
                            })
                            .is_some_and(|cell| {
                                cell.peek_value().is_none_or(|value| value.is_absent())
                            })
                    {
                        let at: Vec<String> = index
                            .iter()
                            .map(|value| {
                                crate::core::formatting::format_value(
                                    value,
                                    &crate::core::formatting::Resolved::plain(
                                        &crate::core::formatting::Presentation::default(),
                                    ),
                                )
                            })
                            .collect();
                        let said = format!("{spelled}[{}]", at.join(", "));
                        if !missing.contains(&said) {
                            missing.push(said);
                        }
                    }
                    continue;
                }
                // A column read whole with a cell nobody entered waits for
                // that cell, named: given `None` among the numbers, the formula
                // raised a TypeError.
                let said = if unavailable.contains(&spelled) {
                    Some(spelled.clone())
                } else {
                    match sample
                        .table(&table)
                        .ok()
                        .and_then(|held| held.column(&column).ok())
                    {
                        None => Some(spelled.clone()),
                        Some(view) => {
                            let cells: Vec<(Vec<&Value>, bool)> = view
                                .cells()
                                .map(|(index, cell)| {
                                    (
                                        index,
                                        cell.peek_value().is_none_or(|value| value.is_absent()),
                                    )
                                })
                                .collect();
                            if cells.iter().all(|(_, absent)| *absent) {
                                Some(spelled.clone())
                            } else {
                                cells.iter().find(|(_, absent)| *absent).map(|(index, _)| {
                                    let index: Vec<String> = index
                                        .iter()
                                        .map(|value| {
                                            crate::core::formatting::format_value(
                                                value,
                                                &crate::core::formatting::Resolved::plain(
                                                    &crate::core::formatting::Presentation::default(
                                                    ),
                                                ),
                                            )
                                        })
                                        .collect();
                                    format!("{spelled}[{}]", index.join(", "))
                                })
                            }
                        }
                    }
                };
                if let Some(said) = said
                    && !missing.contains(&said)
                {
                    missing.push(said);
                }
                continue;
            }
        };
        let absent = if unavailable.iter().any(|other| other == name.as_str()) {
            true
        } else if sample.has_property(&name) {
            sample.property(&name).is_ok_and(|handle| {
                // Not applicable is an answer, which the formula gives on : no
                // input is missing.
                handle
                    .peek(Property::peek_value)
                    .is_none_or(|value| matches!(value, Value::Absent))
                    // A statistic of readings is given by them where there are.
                    && !handle.peek(|property| {
                        property.has_declared_statistic() && property.readings().is_some()
                    })
            })
        } else {
            // An attribute held empty — read by a formula, and no file gave
            // it yet — is owed as a value nobody entered is.
            sample.attribute(&name).map_or(true, |held| {
                matches!(held, AttributeValue::Scalar(Value::Absent))
            })
        };
        if absent && !missing.contains(&name.to_string()) {
            missing.push(name.to_string());
        }
    }
    if let Target::Property(name) = target
        && let Ok(handle) = sample.property(name)
        && !handle.is_computed()
        && handle.has_uncertainty_formula()
        && handle
            .peek(Property::peek_value)
            .is_none_or(|value| value.is_absent())
    {
        missing.push(name.to_string());
    }
    missing
}

/// What a value waits for before `compute()` could run it: its inputs nobody
/// entered, where an absent input a formula would give counts only when that
/// formula waits in turn — as the command line's plan counts a value planned
/// before as given. Empty when nothing stands in the way.
fn waits_for(sample: &Sample, target: &Target, seen: &mut Vec<String>) -> Vec<String> {
    let own = target_names(std::slice::from_ref(target)).join("");
    if seen.contains(&own) {
        return Vec::new();
    }
    seen.push(own.clone());
    let waits = missing_inputs(sample, target, &[])
        .into_iter()
        .filter(|input| {
            if *input == own {
                return true;
            }
            let upstream = match input.split_once('.') {
                None => {
                    let Ok(name) = Identifier::new(input) else {
                        return true;
                    };
                    // A statistic without readings is nobody's to give.
                    if !sample
                        .property(&name)
                        .is_ok_and(|handle| handle.is_computed())
                    {
                        return true;
                    }
                    Target::Property(name)
                }
                Some((table, column)) => {
                    let (Ok(table), Ok(column)) = (Identifier::new(table), Identifier::new(column))
                    else {
                        return true;
                    };
                    let derived = sample
                        .table(&table)
                        .is_ok_and(|held| held.is_derived(&column));
                    if !derived {
                        return true;
                    }
                    Target::Column { table, column }
                }
            };
            !waits_for(sample, &upstream, seen).is_empty()
        })
        .collect();
    // The path, not every value visited: two inputs reading one waiting value
    // both wait.
    seen.pop();
    waits
}

/// A value waiting, in words: what it waits for, and an uncertainty waiting
/// for the value beside it said as that rather than as a value waiting for
/// itself. `failed` are the values that failed in this run.
fn waiting_phrase(name: &str, inputs: &[String], failed: &[String]) -> String {
    let mut others: Vec<String> = inputs
        .iter()
        .filter(|input| *input != name)
        .cloned()
        .collect();
    if others.is_empty() {
        return waiting_sentence(
            &format!("the uncertainty of '{name}'"),
            &["its value".to_string()],
            &[],
        );
    }
    if others.len() < inputs.len() {
        others.push("its own value".to_string());
    }
    waiting_sentence(&format!("'{name}'"), &others, failed)
}

/// The one sentence for a value waiting, whether `compute()` declines to run
/// it or a read serves it: two phrasings of one fact read as two facts. An
/// input whose formula failed is said so, as `samplekit compute` says it:
/// somebody entered it, and it is broken.
fn waiting_sentence(what: &str, inputs: &[String], failed: &[String]) -> String {
    fn listed(names: &[&String]) -> String {
        match names {
            [] => String::new(),
            [one] => (*one).clone(),
            [init @ .., last] => format!(
                "{} and {last}",
                init.iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    let all: Vec<&String> = inputs.iter().collect();
    let (broken, absent): (Vec<&String>, Vec<&String>) =
        inputs.iter().partition(|input| failed.contains(input));
    let mut reasons = Vec::new();
    match absent.as_slice() {
        [] => {}
        [one] if *one == "its value" || *one == "its own value" => {
            reasons.push("nobody entered it".to_string())
        }
        [_] => reasons.push(format!("nobody entered {}", listed(&absent))),
        _ if broken.is_empty() => reasons.push("nobody entered them".to_string()),
        _ => reasons.push(format!("nobody entered {}", listed(&absent))),
    }
    if !broken.is_empty() {
        reasons.push(format!("{} failed", listed(&broken)));
    }
    format!(
        "{what} waits for {}: {}, so its formula does not run",
        listed(&all),
        reasons.join(", and ")
    )
}

/// The targets ordered so that every value comes after the values it reads,
/// whatever order they were declared in: a value declared before its input
/// would otherwise run once for its own sake and once more inside the
/// dependent that read it first, and a derived value is measured in minutes.
/// Targets of one node keep their order among themselves; the first target of
/// a node sets where the node goes.
fn in_dependency_order(shared: &SharedSample, targets: Vec<Target>) -> PyResult<Vec<Target>> {
    shared.read_sample("the sample", |sample| {
        let mut by_node: IndexMap<Node, Vec<Target>> = IndexMap::new();
        for target in targets {
            by_node
                .entry(target_node(&target))
                .or_default()
                .push(target);
        }
        let mut ordered = Vec::new();
        let mut visited: HashSet<Node> = HashSet::new();
        let nodes: Vec<Node> = by_node.keys().cloned().collect();
        for node in &nodes {
            order_node(sample, node, &mut by_node, &mut visited, &mut ordered);
        }
        Ok(ordered)
    })
}

fn order_node(
    sample: &Sample,
    node: &Node,
    by_node: &mut IndexMap<Node, Vec<Target>>,
    visited: &mut HashSet<Node>,
    ordered: &mut Vec<Target>,
) {
    if !visited.insert(node.clone()) {
        return;
    }
    for input in node_inputs(sample, node) {
        if by_node.contains_key(&input) {
            order_node(sample, &input, by_node, visited, ordered);
        }
    }
    if let Some(targets) = by_node.get_mut(node) {
        ordered.append(targets);
    }
}

/// Every stale value: each property, and each derived cell, so that only the
/// rows that need it run again.
fn stale_targets(py: Python<'_>, shared: &SharedSample) -> PyResult<Vec<Target>> {
    shared.read_sample("what is outdated", |sample| {
        let _pass = fingerprint::Pass::begin();
        let _verdicts = fingerprint::Verdicts::begin();
        let mut targets = Vec::new();
        for name in sample.property_names() {
            if fingerprint::is_stale(sample, name).or_raise(py)? {
                targets.push(Target::Property(name.clone()));
            }
        }
        for table in sample.table_names() {
            let held = sample.table(table).or_raise(py)?;
            for column in held.column_names() {
                if !held.is_derived(column) && !is_statistic_column(held, column) {
                    continue;
                }
                for tuple in held.index_tuples() {
                    let index: Vec<Value> = tuple.into_iter().cloned().collect();
                    if fingerprint::is_cell_stale(sample, table, column, &index).or_raise(py)? {
                        targets.push(Target::Cell {
                            table: table.clone(),
                            index,
                            column: column.clone(),
                        });
                    }
                }
            }
        }
        Ok(targets)
    })
}

/// Every value stale or never computed: what `compute` runs.
fn pending_targets(py: Python<'_>, shared: &SharedSample) -> PyResult<Vec<Target>> {
    Ok(selected(py, shared, None, false, false)?
        .into_iter()
        .map(|(target, _)| target)
        .collect())
}

/// What named values need first: their inputs, transitively, that are stale or
/// were never computed. An override is kept, and nothing is followed past it.
fn with_pending_inputs(
    py: Python<'_>,
    shared: &SharedSample,
    named: Vec<Target>,
) -> PyResult<Vec<Target>> {
    let pending = pending_targets(py, shared)?;
    if pending.is_empty() {
        return Ok(named);
    }
    let upstream: HashSet<Node> = shared.read_sample("the sample", |sample| {
        let mut stack: Vec<Node> = named.iter().map(target_node).collect();
        let mut seen = HashSet::new();
        while let Some(node) = stack.pop() {
            if let Node::Named(name) = &node
                && sample.property(name).is_ok_and(|handle| handle.is_edited())
            {
                continue;
            }
            let inputs = node_inputs(sample, &node);
            for input in inputs {
                if seen.insert(input.clone()) {
                    stack.push(input);
                }
            }
        }
        Ok(seen)
    })?;
    let mut targets: Vec<Target> = pending
        .into_iter()
        .filter(|target| upstream.contains(&target_node(target)))
        .collect();
    targets.extend(named);
    Ok(targets)
}

/// What one node reads: a property's declared inputs, or a derived column's,
/// with a cell of its own row read as its column.
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
                InputName::Cell(other) | InputName::CellValue(other) => Node::Column {
                    table: table.clone(),
                    column: other,
                },
                InputName::Column { table, column } => Node::Column { table, column },
            })
            .collect(),
    }
}

/// A plan grouped by value — a column's cells as `table.column` — and ordered so
/// that every value comes after the values it reads.
fn planned(sample: &Sample, reasoned: Vec<(Target, &'static str)>) -> Vec<(String, String)> {
    let mut groups: IndexMap<Node, (String, &'static str)> = IndexMap::new();
    for (target, reason) in reasoned {
        let label = match &target {
            Target::Property(name) => name.to_string(),
            Target::Column { table, column } | Target::Cell { table, column, .. } => {
                format!("{table}.{column}")
            }
        };
        groups
            .entry(target_node(&target))
            .or_insert((label, reason));
    }
    let mut ordered = Vec::with_capacity(groups.len());
    let mut visited = HashSet::new();
    let nodes: Vec<Node> = groups.keys().cloned().collect();
    for node in &nodes {
        visit_in_order(sample, node, &groups, &mut visited, &mut ordered);
    }
    ordered
}

fn visit_in_order(
    sample: &Sample,
    node: &Node,
    groups: &IndexMap<Node, (String, &'static str)>,
    visited: &mut HashSet<Node>,
    ordered: &mut Vec<(String, String)>,
) {
    if !visited.insert(node.clone()) {
        return;
    }
    for input in node_inputs(sample, node) {
        visit_in_order(sample, &input, groups, visited, ordered);
    }
    if let Some((label, reason)) = groups.get(node) {
        let said = match *reason {
            "statistic" => statistic_change(sample, label).unwrap_or_else(|| "outdated".into()),
            other => other.to_string(),
        };
        ordered.push((label.clone(), said));
    }
}

fn target_node(target: &Target) -> Node {
    match target {
        Target::Property(name) => Node::Named(name.clone()),
        Target::Column { table, column } | Target::Cell { table, column, .. } => Node::Column {
            table: table.clone(),
            column: column.clone(),
        },
    }
}

fn target_names(targets: &[Target]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for target in targets {
        let name = match target {
            Target::Property(name) => name.to_string(),
            Target::Column { table, column } | Target::Cell { table, column, .. } => {
                format!("{table}.{column}")
            }
        };
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The `path` a subclass's `__init__` received, when it has a parameter of that
/// name: what `super.__init__` should have been given. A class using `Sample`'s
/// own `__init__` passes it by construction.
fn path_given(
    slf: &Bound<'_, PySample>,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Option<String>> {
    let py = slf.py();
    let init = slf.get_type().getattr("__init__")?;
    if init.is(&py.get_type::<PySample>().getattr("__init__")?) {
        return Ok(None);
    }
    // A signature Python cannot describe, or arguments it does not accept,
    // were already refused by the call itself; there is nothing more to check.
    let Ok(signature) = py.import("inspect")?.call_method1("signature", (&init,)) else {
        return Ok(None);
    };
    let mut positional = vec![py.None().into_bound(py)];
    positional.extend(args.iter());
    let Ok(bound) = signature.call_method("bind", PyTuple::new(py, positional)?, kwargs) else {
        return Ok(None);
    };
    let given = bound.getattr("arguments")?.call_method1("get", ("path",))?;
    Ok((!given.is_none()).then(|| bridge::repr(&given)))
}

const COMPOSED: &str = "a list is composed, not edited: build one with +, SampleList([...]), or \
                        list(samples) for a Python list";

/// What a list operation's formulas computed, recorded when it ends.
struct Recording {
    samples: Vec<SharedSample>,
    before: u64,
}

impl Drop for Recording {
    fn drop(&mut self) {
        for sample in &self.samples {
            bridge::record_computed(sample, self.before);
        }
    }
}

enum ProjectChoice {
    Outside,
    One(Rc<ProjectConfig>),
}

/// An ordered list of samples, read like a Python list.
///
/// A list is read from a folder, or built from samples already read. An item
/// is a sample, by position or by name — ``specimens["steel-01"]`` — and a slice
/// is a list. Lists join with ``+``. ``filter``, ``query``, ``sorted`` and
/// ``group_by`` make new lists; the samples in them are the same objects.
///
/// Args:
///     source: A folder, whose samples are read as the project declares, or
///         an iterable of samples.
///     pattern: A glob choosing the folder's files, instead of what
///         ``[collection]`` declares.
///     model: The model class to read the samples with, or ``False`` to read
///         the data alone. By default, the project's model.
///
/// Raises:
///     TypeError: The iterable holds something other than samples, such as
///         paths: ``sk.load`` reads paths.
///     ValueError: A sample appears twice, or ``pattern`` or ``model`` is
///         given without a folder.
///     FileNotFoundError: The folder does not exist.
///
/// Example:
///     >>> specimens = sk.SampleList("samples")
///     >>> len(specimens)
///     3
///     >>> specimens["steel-01"].material
///     'steel'
#[pyclass(name = "SampleList", module = "samplekit", unsendable, sequence)]
pub struct PySampleList {
    samples: RefCell<Vec<Py<PyAny>>>,
    /// The project of the list this one was taken from, kept for when this
    /// one holds no sample: a filter that kept nothing is still asked about
    /// the project's figures and profiles.
    origin: Option<Rc<ProjectConfig>>,
    /// The samples of the list this one was taken from, for when this one
    /// holds none: a column or a filter's field is checked against them, as the
    /// command line checks it against the collection before its filter. Checked against
    /// nothing, every field was unknown, and a filter keeping no sample made
    /// `to_csv` raise where `samplekit -f … --csv` writes the header.
    source: Vec<Py<PyAny>>,
    /// Whether the list is in a sort's order: it came from `sorted` or `sort`,
    /// or was narrowed from one that did, which keeps the order. Its groups
    /// then follow that order, as `-s` orders `--group`'s tables; `+` makes a
    /// list that was not sorted.
    sorted: Flag<bool>,
}

impl PySampleList {
    fn of(py: Python<'_>, samples: Vec<Py<PyAny>>) -> PyResult<PySampleList> {
        check_unique(py, &samples)?;
        Ok(PySampleList {
            samples: RefCell::new(samples),
            origin: None,
            source: Vec::new(),
            sorted: Flag::new(false),
        })
    }

    /// The order its groups come in: the list's own where it is sorted, else
    /// their values'.
    fn group_order(&self) -> list::GroupOrder {
        if self.sorted.get() {
            list::GroupOrder::Listed
        } else {
            list::GroupOrder::Values
        }
    }

    /// The list a field is checked against, as `checked_against` says.
    fn checking_list(&self, py: Python<'_>) -> PyResult<SampleList> {
        let entries = self
            .checked_against(py)
            .iter()
            .map(|object| {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                Ok(Entry {
                    path: sample.file.borrow().path.clone(),
                    sample: sample.shared().inner.clone(),
                    states: None,
                })
            })
            .collect::<PyResult<Vec<Entry>>>()?;
        Ok(list::from_entries(entries))
    }

    /// What a field is checked against: this list's samples, or, when it
    /// holds none, those of the list it was taken from.
    fn checked_against(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        let held = self.objects(py);
        if held.is_empty() {
            self.source
                .iter()
                .map(|object| object.clone_ref(py))
                .collect()
        } else {
            held
        }
    }

    fn objects(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.samples
            .borrow()
            .iter()
            .map(|sample| sample.clone_ref(py))
            .collect()
    }

    /// A file written from this list, tied in its samples' projects' history to
    /// the snapshot it was made from. A history not kept is a warning.
    fn keep_output(&self, py: Python<'_>, written: &Path, said: &str) -> PyResult<()> {
        // The script's snapshot, the file made from it.
        let paths: Vec<PathBuf> = self
            .entries(py)?
            .into_iter()
            .filter_map(|entry| entry.path)
            .collect();
        let here = std::env::current_dir().unwrap_or_default();
        for (config, snapshot) in script_snapshots(py, &paths, &[written.to_path_buf()])? {
            // Each project records the samples of its own.
            let root = canonical(config.root());
            let theirs: Vec<PathBuf> = paths
                .iter()
                .filter(|path| canonical(path).starts_with(&root))
                .cloned()
                .collect();
            if let Err(error) = crate::config::version_control::tag_output(
                &config, &snapshot, written, said, &theirs, &here,
            ) {
                warn_value(
                    py,
                    &format!("the history does not record {}: {error}", written.display()),
                )?;
            }
        }
        Ok(())
    }

    fn entries(&self, py: Python<'_>) -> PyResult<Vec<Entry>> {
        self.samples
            .borrow()
            .iter()
            .map(|object| {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                Ok(Entry {
                    path: sample.file.borrow().path.clone(),
                    sample: sample.shared().inner.clone(),
                    states: None,
                })
            })
            .collect()
    }

    /// The list, each sample carrying its states: what its file says, and what
    /// `not_current` says the model owes — nothing run. A sample read without
    /// its model is read by its project's model for this, from its file, as the
    /// command line plans with its worker; one whose model cannot be read
    /// answers from its file, and is counted in one warning.
    fn with_states(
        &self,
        py: Python<'_>,
        collection: &SampleList,
        needs_the_model: bool,
    ) -> PyResult<SampleList> {
        let mut states = crate::collection::validation::states(collection);
        // `samplekit.Sample` itself is a sample read without a model.
        let base = bridge::sample_class(py)?;
        // Why a sample's model was not read, and for how many.
        let mut unread: IndexMap<String, usize> = IndexMap::new();
        for (object, states) in self.objects(py).iter().zip(states.iter_mut()) {
            let bound = object.bind(py);
            let reader = if bound.get_type().is(&base) {
                // The project the file lies in, whatever the sample was read
                // with: `model=False` reads a sample without its configuration.
                let path = bound
                    .cast::<PySample>()?
                    .borrow()
                    .file
                    .borrow()
                    .path
                    .clone();
                match model_reader(py, path.as_deref()) {
                    Ok(Some(reader)) => reader,
                    // No model declared: nothing is owed, and that is known.
                    Ok(None) => {
                        *states = crate::query::field_addressing::States::new(
                            states.held().to_vec(),
                            true,
                        );
                        continue;
                    }
                    Err(reason) => {
                        *unread.entry(reason).or_default() += 1;
                        continue;
                    }
                }
            } else {
                bound.clone()
            };
            let said = reader.call_method0("not_current")?;
            let said = said.cast::<PyDict>()?;
            let mut held = states.held().to_vec();
            for (_, value) in said.iter() {
                let value: String = value.extract()?;
                held.push(if value.starts_with("waits for") {
                    fields::State::Waiting
                } else if value.contains("never computed") {
                    fields::State::NeverComputed
                } else if value == "failed" {
                    fields::State::Failed
                } else if value == "edited" {
                    fields::State::Edited
                } else {
                    fields::State::Stale
                });
            }
            *states = crate::query::field_addressing::States::new(held, true);
        }
        // Said only where a word asked needs what the model would say.
        let count: usize = unread.values().sum();
        if count > 0 && needs_the_model {
            let why: Vec<&str> = unread
                .keys()
                .map(|reason| crate::config::model_runtime::reason_line(reason))
                .collect();
            let (whose, what) = if count == 1 {
                ("1 sample".to_string(), "its state is what its file says")
            } else {
                (
                    format!("{count} samples"),
                    "their state is what their files say",
                )
            };
            warn_value(
                py,
                &format!(
                    "the model was not read for {whose} ({}): {what}, and what the model \
                     never computed is not known",
                    why.join("; ")
                ),
            )?;
        }
        Ok(collection.with_states(states))
    }

    fn rust_list(&self, py: Python<'_>) -> PyResult<SampleList> {
        Ok(list::from_entries(self.entries(py)?))
    }

    /// Resolves every sample's tables, so that a field read through the core
    /// sees a derived cell as what its formula yields.
    fn prepare(&self, py: Python<'_>) -> PyResult<()> {
        // Reads do not compute: a list reads its tables as they stand.
        if !crate::core::property::reads_compute() {
            return Ok(());
        }
        for object in self.objects(py) {
            let sample = object.bind(py).cast::<PySample>()?.borrow();
            let shared = sample.shared();
            let before = bridge::formula_runs();
            // A formula failing here fails the whole operation, so it says in
            // which sample: one among three hundred is otherwise a search.
            let label = sample
                .file
                .borrow()
                .path
                .as_ref()
                .and_then(|path| path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .or_else(|| sample.name().ok().flatten())
                .unwrap_or_else(|| "a sample".to_string());
            let names = shared.read_sample("the sample", |held| {
                Ok(held.table_names().into_iter().cloned().collect::<Vec<_>>())
            })?;
            names
                .iter()
                .try_for_each(|name| resolve_with_upstream(py, &shared, name, None))
                .map_err(|error| located(py, error, &label))?;
            bridge::record_computed(&shared, before);
        }
        Ok(())
    }

    /// Records, once the operation that holds it ends, what its formulas
    /// computed in every sample of the list.
    fn recording(&self, py: Python<'_>) -> PyResult<Recording> {
        let samples = self
            .objects(py)
            .iter()
            .map(|object| Ok(object.bind(py).cast::<PySample>()?.borrow().shared()))
            .collect::<PyResult<_>>()?;
        Ok(Recording {
            samples,
            before: bridge::formula_runs(),
        })
    }

    /// The Python objects of a list the core derived from this one.
    fn objects_of(&self, py: Python<'_>, derived: &SampleList) -> PyResult<Vec<Py<PyAny>>> {
        let mut by_pointer: HashMap<*const RefCell<Sample>, Py<PyAny>> = HashMap::new();
        for object in self.objects(py) {
            let pointer = Rc::as_ptr(&object.bind(py).cast::<PySample>()?.borrow().shared().inner);
            by_pointer.insert(pointer, object);
        }
        Ok(derived
            .iter()
            .filter_map(|entry| {
                by_pointer
                    .get(&Rc::as_ptr(&entry.sample))
                    .map(|object| object.clone_ref(py))
            })
            .collect())
    }

    fn project_choice(&self, py: Python<'_>) -> PyResult<ProjectChoice> {
        let mut roots: Vec<(Option<PathBuf>, Option<Rc<ProjectConfig>>)> = Vec::new();
        for object in self.objects(py) {
            let sample = object.bind(py).cast::<PySample>()?.borrow();
            let config = sample.file.borrow().config.clone();
            let root = config.as_ref().map(|config| config.root().to_path_buf());
            if !roots.iter().any(|(known, _)| *known == root) {
                roots.push((root, config));
            }
        }
        match roots.len() {
            0 => Ok(match &self.origin {
                Some(config) => ProjectChoice::One(config.clone()),
                None => ProjectChoice::Outside,
            }),
            1 => Ok(match roots.remove(0).1 {
                Some(config) => ProjectChoice::One(config),
                None => ProjectChoice::Outside,
            }),
            _ => {
                let named: Vec<String> = roots
                    .iter()
                    .map(|(root, _)| match root {
                        Some(root) => root.display().to_string(),
                        None => "outside any project".to_string(),
                    })
                    .collect();
                Err(PyValueError::new_err(format!(
                    "these samples come from more than one project, so no declared name resolves \
                     against them: {}\n  pass the entry itself, such as project.profiles.<name> \
                     or project.queries.<name>",
                    named.join(" and ")
                )))
            }
        }
    }

    /// Every project the list's samples come from, each once.
    fn project_configs(&self, py: Python<'_>) -> PyResult<Vec<Rc<ProjectConfig>>> {
        let mut configs: Vec<Rc<ProjectConfig>> = Vec::new();
        for object in self.objects(py) {
            let sample = object.bind(py).cast::<PySample>()?.borrow();
            if let Some(config) = sample.file.borrow().config.clone()
                && !configs.iter().any(|held| held.root() == config.root())
            {
                configs.push(config);
            }
        }
        if configs.is_empty()
            && let Some(origin) = &self.origin
        {
            configs.push(origin.clone());
        }
        Ok(configs)
    }

    fn single_config(&self, py: Python<'_>, kind: &str, name: &str) -> PyResult<Rc<ProjectConfig>> {
        let choice = match self.project_choice(py) {
            Ok(choice) => choice,
            // Several projects declaring one query alike are one query: the
            // command line runs it in each, and the list refused it.
            Err(refused) if kind == "query" => {
                let mut configs: Vec<Rc<ProjectConfig>> = Vec::new();
                for object in self.objects(py) {
                    let sample = object.bind(py).cast::<PySample>()?.borrow();
                    let config = sample.file.borrow().config.clone();
                    let Some(config) = config else {
                        return Err(refused);
                    };
                    if !configs.iter().any(|held| held.root() == config.root()) {
                        configs.push(config);
                    }
                }
                let declared: Vec<Option<NamedQuery>> = configs
                    .iter()
                    .map(|config| named_queries::named(config, name).ok().cloned())
                    .collect();
                let alike = declared
                    .first()
                    .and_then(Option::as_ref)
                    .is_some_and(|first| {
                        declared.iter().all(|other| {
                            other.as_ref().is_some_and(|other| {
                                other.filter == first.filter && other.directory == first.directory
                            })
                        })
                    });
                return match configs.into_iter().next() {
                    Some(config) if alike => Ok(config),
                    _ => Err(PyValueError::new_err(format!(
                        "these samples come from more than one project, which declare the query \
                         '{name}' differently, or not each: pass the entry itself, such as \
                         project.queries.{name}"
                    ))),
                };
            }
            Err(refused) => return Err(refused),
        };
        match choice {
            ProjectChoice::One(config) => Ok(config),
            ProjectChoice::Outside => Err(bridge::key_error(format!(
                "no {kind} '{name}': no .samplekitrc was found above these samples, so nothing \
                 is declared"
            ))),
        }
    }

    fn derived(&self, py: Python<'_>, samples: Vec<Py<PyAny>>) -> PyResult<PySampleList> {
        let mut list = PySampleList::of(py, samples)?;
        // Narrowed, the order kept: a sorted list's part is sorted.
        list.sorted.set(self.sorted.get());
        list.origin = match self.project_choice(py) {
            Ok(ProjectChoice::One(config)) => Some(config),
            _ => None,
        };
        // The list first taken from, through any number of filters, as the
        // command line's collection is the one before every filter.
        list.source = if self.source.is_empty() {
            self.objects(py)
        } else {
            self.source
                .iter()
                .map(|object| object.clone_ref(py))
                .collect()
        };
        Ok(list)
    }

    fn sorted_objects(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
        // A function is a key as Python's own sorted takes one.
        if key.is_callable() && key.cast::<PyString>().is_err() && key.cast::<PyField>().is_err() {
            let samples: Vec<Py<PyAny>> = self
                .samples
                .borrow()
                .iter()
                .map(|sample| sample.clone_ref(py))
                .collect();
            let arguments = PyDict::new(py);
            arguments.set_item("key", key)?;
            return py
                .import("builtins")?
                .getattr("sorted")?
                .call((samples,), Some(&arguments))?
                .extract();
        }
        let keys = sort_keys(key)?;
        let spec = ordering::parse_spec(&keys).or_raise(py)?;
        refuse_sorting_by_state(&spec)?;
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        // A row or a position no sample holds is a mistake, as `filter` and
        // `to_csv` say: sorted by it, the list came back as it was, in silence.
        {
            let guards: Vec<_> = collection
                .iter()
                .map(|entry| entry.sample.borrow())
                .collect();
            let samples: Vec<&crate::core::sample::Sample> =
                guards.iter().map(|guard| &**guard).collect();
            for key in spec.keys() {
                if let Some(error) = fields::held_nowhere(&key.field, &samples) {
                    return Err(bridge::key_error(error.to_string()));
                }
            }
        }
        let _recording = self.recording(py)?;
        bridge::forget_raised();
        let sorted = bridge::without_gil(py, || collection.sorted(&spec)).or_raise(py)?;
        self.objects_of(py, &sorted)
    }

    fn export_profile(
        &self,
        py: Python<'_>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Profile, bool)> {
        let columns = columns.filter(|columns| !columns.is_none());
        let profile = profile.filter(|profile| !profile.is_none());
        match (columns, profile) {
            (Some(_), Some(_)) => Err(PyValueError::new_err(
                "pass columns or a profile, not both: a profile already names its columns",
            )),
            (None, None) => {
                let declared = match self.project_choice(py)? {
                    ProjectChoice::One(config) => config
                        .profile_names()
                        .iter()
                        .map(|name| name.to_string())
                        .collect::<Vec<_>>(),
                    ProjectChoice::Outside => Vec::new(),
                };
                Err(PyValueError::new_err(if declared.is_empty() {
                    "nothing to export: pass columns=[...] or profile=..., and no profile is \
                     declared here"
                        .to_string()
                } else {
                    format!(
                        "nothing to export: pass columns=[...] or profile=...\n  declared profiles: {}",
                        declared.join(", ")
                    )
                }))
            }
            // A written column's name is what is written, as `-c` takes it:
            // a data file's header, a document's label.
            (Some(columns), None) => Ok((
                profiles::anonymous(
                    written_columns(columns)?
                        .into_iter()
                        .map(|column| ColumnSpec {
                            header: column.header.clone().or_else(|| column.label.clone()),
                            ..column
                        })
                        .collect(),
                ),
                false,
            )),
            (None, Some(profile)) => {
                if let Ok(entry) = profile.cast::<PyProfile>() {
                    return Ok((entry.borrow().declaration.clone(), true));
                }
                let name: String = profile.extract().map_err(|_| {
                    PyTypeError::new_err("profile is a declared name or project.profiles.<name>")
                })?;
                let config = self.single_config(py, "profile", &name)?;
                Ok((config.profile(&name).or_raise(py)?.clone(), true))
            }
        }
    }

    /// The rectangle an export writes, through the command line's own builder.
    fn dataset(
        &self,
        py: Python<'_>,
        profile: &Profile,
        declared: bool,
        includes: (bool, bool),
        numbers: exports::Numbers,
    ) -> PyResult<Dataset> {
        self.dataset_as(py, profile, declared, includes, numbers, Format::Csv)
    }

    /// [`Self::dataset`] for `format`, which says how units are spelled.
    fn dataset_as(
        &self,
        py: Python<'_>,
        profile: &Profile,
        declared: bool,
        includes: (bool, bool),
        numbers: exports::Numbers,
        format: Format,
    ) -> PyResult<Dataset> {
        self.prepare(py)?;
        let mut collection = self.rust_list(py)?;
        // What the model owes is planned from its description, had written
        // where it is missing, as the command line has it: on a fresh clone
        // a value waiting was written with no state.
        for config in self.project_configs(py)? {
            let _ = crate::collection::computation::describe_if_missing(&config);
        }
        // A column of `state` shows every word, so the model is read for it, as
        // `-c name,state` reads it.
        if profile
            .columns()
            .iter()
            .any(|column| column.field == "state")
        {
            collection = self.with_states(py, &collection, true)?;
        }
        let _recording = self.recording(py)?;
        let warnings = list::check_profile(&self.checking_list(py)?, profile);
        if let Some(warning) = warnings.first() {
            return Err(match warning {
                FieldWarning::Unknown { field, suggestion } => {
                    let mut message = format!("unknown field '{field}'");
                    if let Some(suggestion) = suggestion {
                        message.push_str(&format!("\n  did you mean: {suggestion}?"));
                    }
                    bridge::key_error(message)
                }
                FieldWarning::TableNeedsCell { table } => PyValueError::new_err(format!(
                    "'{table}' names a table, not one scalar field: choose a column and an index"
                )),
                FieldWarning::NoItem { field, .. } => {
                    bridge::key_error(format!("no sample has the item '{field}'"))
                }
                FieldWarning::NoRow { field, .. } => {
                    bridge::key_error(format!("no sample has the row '{field}'"))
                }
            });
        }
        bridge::forget_raised();
        let mut order = self.group_order();
        if declared && !profile.sort.is_empty() {
            let spec = ordering::parse_spec(&profile.sort).or_raise(py)?;
            refuse_sorting_by_state(&spec)?;
            collection = collection.sorted(&spec).or_raise(py)?;
            order = list::GroupOrder::Listed;
        }
        // A declared profile's groups: one table carrying them, as `--profile
        // NAME --csv` writes it.
        let grouped;
        let profile = if declared && !profile.group.is_empty() {
            let (carrying, rows) = exports::grouped(profile, &collection, order).or_raise(py)?;
            collection = rows;
            grouped = carrying;
            &grouped
        } else {
            profile
        };
        let target = ExportTarget {
            name: profile.name.clone(),
            profile: profile.name.clone(),
            format,
            output: PathBuf::from("-"),
            filename: includes.0,
            path: includes.1,
            query: None,
            style: None,
        };
        // Units spelled as the project declares them, as the command line does.
        let config = match self.project_choice(py) {
            Ok(ProjectChoice::One(config)) => Some(config),
            _ => None,
        };
        // Over several projects, each row at its own project's precision.
        if config.is_none() {
            let mut held = Vec::new();
            for object in self.objects(py) {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                let file = sample.file.borrow();
                if let (Some(path), Some(config)) = (&file.path, &file.config) {
                    held.push((path.clone(), (**config).clone()));
                }
            }
            collection = collection.with_configurations(held);
        }
        // Which columns a field makes — a quantity's two — is judged over the
        // list this one was taken from, as the command line judges it over
        // the collection before its filter: a selection that kept nothing
        // wrote `modulus` where the command line writes its value and its
        // uncertainty, each with its unit.
        // Over several projects the list carries each sample's configuration,
        // which a row's precision is read from, and is described by itself.
        let source = match config {
            Some(_) => Some(self.described_list(py)?),
            None => None,
        };
        let described = source.as_ref().unwrap_or(&collection);
        let written =
            exports::run_within(&target, profile, &collection, config.as_deref(), described)
                .or_raise(py)?;
        match numbers {
            exports::Numbers::AsWritten => Ok(written),
            // The stored numbers, under the headers the written export has.
            exports::Numbers::AsStored => {
                let stored =
                    exports::run_as_stored(&target, profile, &collection, config.as_deref())
                        .or_raise(py)?;
                let rows = stored
                    .rows
                    .iter()
                    .map(|row| {
                        written
                            .headers
                            .iter()
                            .map(|header| {
                                stored
                                    .headers
                                    .iter()
                                    .position(|held| held == header)
                                    .map_or(Cell::Scalar(None), |at| row[at].clone())
                            })
                            .collect()
                    })
                    .collect();
                Ok(Dataset { rows, ..written })
            }
        }
    }

    /// The list a selection is described by: the one it was first taken
    /// from, or this one.
    fn described_list(&self, py: Python<'_>) -> PyResult<SampleList> {
        // A list made by hand is described by its project's samples, as one a
        // filter left is by the list it came from: a unit no sample of it
        // held was left out of its headers.
        if self.source.is_empty() {
            if let Ok(ProjectChoice::One(config)) = self.project_choice(py)
                && let Ok(project) = list::from_directory(config.root())
            {
                return Ok(project);
            }
            return self.rust_list(py);
        }
        let entries = self
            .source
            .iter()
            .map(|object| {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                Ok(Entry {
                    path: sample.file.borrow().path.clone(),
                    sample: sample.shared().inner.clone(),
                    states: None,
                })
            })
            .collect::<PyResult<Vec<Entry>>>()?;
        Ok(list::from_entries(entries))
    }

    fn text(
        &self,
        py: Python<'_>,
        format: Format,
        path: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Option<String>> {
        let (profile, declared) = self.export_profile(py, columns, profile)?;
        let dataset = self.dataset_as(
            py,
            &profile,
            declared,
            (false, false),
            exports::Numbers::AsWritten,
            format.clone(),
        )?;
        let dataset = noted_empty(py, &dataset, &format)?;
        let text = export_formats::serialize(&dataset, format.clone()).or_raise(py)?;
        match path.filter(|path| !path.is_none()) {
            // The method names the format and the caller the file, as `--csv -o
            // x.json` writes CSV into `x.json`.
            Some(path) => {
                let path = path_from(path)?;
                exports::write(&text, &path, overwrite).or_raise(py)?;
                let method = match format {
                    Format::Csv => "to_csv",
                    Format::Tsv => "to_tsv",
                    Format::Json => "to_json",
                    Format::Text(_) => "to_text",
                };
                self.keep_output(py, &path, &format!("{} · {method}", script_said(py)))?;
                Ok(None)
            }
            None => Ok(Some(text)),
        }
    }

    fn position_of(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Vec<usize>> {
        let mut positions = Vec::new();
        for (position, object) in self.objects(py).into_iter().enumerate() {
            if same_sample(object.bind(py), value)? {
                positions.push(position);
            }
        }
        Ok(positions)
    }
}

/// Whether two objects are one sample: the same object, the same file, or a
/// name given as text.
fn same_sample(held: &Bound<'_, PyAny>, wanted: &Bound<'_, PyAny>) -> PyResult<bool> {
    if held.is(wanted) {
        return Ok(true);
    }
    let held = held.cast::<PySample>()?.borrow();
    if let Ok(name) = wanted.extract::<String>() {
        return Ok(held.name()?.as_deref() == Some(name.as_str()));
    }
    let Ok(other) = wanted.cast::<PySample>() else {
        return Ok(false);
    };
    let other = other.borrow();
    if held.shared().same(&other.shared()) {
        return Ok(true);
    }
    Ok(
        match (&held.file.borrow().path, &other.file.borrow().path) {
            (Some(left), Some(right)) => same_file(left, right),
            _ => false,
        },
    )
}

/// A list holds a sample once: by resolved path, or, never saved, by identity.
fn check_unique(py: Python<'_>, samples: &[Py<PyAny>]) -> PyResult<()> {
    let mut paths: HashSet<PathBuf> = HashSet::new();
    let mut unsaved: HashSet<*const RefCell<Sample>> = HashSet::new();
    for object in samples {
        let sample = object.bind(py).cast::<PySample>()?.borrow();
        let pointer = Rc::as_ptr(&sample.shared().inner);
        match &sample.file.borrow().path {
            Some(path) => {
                let resolved = dunce::canonicalize(path)
                    .or_else(|_| std::path::absolute(path))
                    .unwrap_or_else(|_| path.clone());
                if !paths.insert(resolved) {
                    return Err(PyValueError::new_err(format!(
                        "{} would be in this list twice, and a list holds a sample once",
                        path.display()
                    )));
                }
            }
            None => {
                if !unsaved.insert(pointer) {
                    return Err(PyValueError::new_err(
                        "the same unsaved sample would be in this list twice, and a list holds a \
                         sample once",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Refuses a sort by `state`, as `--sort state` is refused: a sample holds
/// several states, and the words have no order a sort could promise.
fn refuse_sorting_by_state(spec: &ordering::SortSpec) -> PyResult<()> {
    if spec
        .keys()
        .iter()
        .any(|key| key.field == fields::Field::Reserved(fields::ReservedField::State))
    {
        return Err(PyTypeError::new_err(
            "'state' cannot be sorted by: a sample holds several states, which have no \
             order — select by one with filter(\"state == failed\"), or show them with \
             columns=[..., \"state\"]",
        ));
    }
    Ok(())
}

fn sort_keys(key: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    // A comma list, as `-s` takes one.
    if let Ok(text) = key.cast::<PyString>() {
        return Ok(text
            .to_str()?
            .split(',')
            .map(|key| key.trim().to_string())
            .filter(|key| !key.is_empty())
            .collect());
    }
    if let Ok(field) = key.cast::<PyField>() {
        return Ok(vec![field.borrow().path.clone()]);
    }
    let mut keys = Vec::new();
    for item in key.try_iter()? {
        let item = item?;
        if let Ok(field) = item.cast::<PyField>() {
            keys.push(field.borrow().path.clone());
        } else {
            keys.push(item.extract::<String>().map_err(|_| {
                PyTypeError::new_err(format!(
                    "a sort key is a field path or a Field, and this is {}",
                    bridge::a_type(&item)
                ))
            })?);
        }
    }
    Ok(keys)
}

fn field_path(field: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(entry) = field.cast::<PyField>() {
        return Ok(entry.borrow().path.clone());
    }
    field
        .extract::<String>()
        .map_err(|_| PyTypeError::new_err("a field is a field path or a Field"))
}

/// The fields a group names: one, several in a sequence, or several in one
/// text separated by commas, as `--group` writes them.
fn group_fields(fields: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    let refused = || {
        PyTypeError::new_err(format!(
            "group_by takes a field, or several in a list: {}",
            fields
                .repr()
                .map_or_else(|_| "?".to_string(), |repr| repr.to_string())
        ))
    };
    let paths: Vec<String> = if let Ok(text) = fields.cast::<PyString>() {
        text.to_str()?
            .split(',')
            .map(|part| part.trim().to_string())
            .collect()
    } else if fields.cast::<PyField>().is_ok() {
        vec![field_path(fields)?]
    } else if fields.is_instance_of::<PyList>() || fields.is_instance_of::<PyTuple>() {
        fields
            .try_iter()?
            .map(|item| field_path(&item?).map_err(|_| refused()))
            .collect::<PyResult<_>>()?
    } else {
        return Err(refused());
    };
    if paths.is_empty() || paths.iter().any(String::is_empty) {
        return Err(PyValueError::new_err(format!(
            "group_by names a field, or several separated by commas: {}",
            fields
                .repr()
                .map_or_else(|_| "?".to_string(), |repr| repr.to_string())
        )));
    }
    Ok(paths)
}

/// A column's values read, with how many of a derived column's cells no
/// formula has given, of how many, and whose.
struct ColumnRead {
    values: Vec<Value>,
    never: usize,
    name: String,
}

fn column_read(py: Python<'_>, table: &Table, column: &Identifier) -> PyResult<ColumnRead> {
    let derived = table.is_derived(column);
    let mut values = Vec::new();
    let mut never = 0;
    for (_, cell) in table.column(column).or_raise(py)?.cells() {
        let value = cell.value().or_raise(py)?;
        if derived && value.is_absent() {
            never += 1;
        }
        values.push(value);
    }
    Ok(ColumnRead {
        values,
        never,
        name: format!("{}.{column}", table.name()),
    })
}

/// A column's values for Python, said once for the column read where cells of
/// it were never computed, as a property's own read says it: a column of None,
/// in silence, read as data.
fn said_column(py: Python<'_>, read: ColumnRead) -> PyResult<Vec<Py<PyAny>>> {
    let total = read.values.len();
    if read.never > 0 && !crate::core::property::reads_compute() {
        let what = &read.name;
        let never = read.never;
        warn_value(
            py,
            &if never == total {
                format!("{what} was never computed: compute() runs it")
            } else {
                format!(
                    "{never} of {total} cells of {what} were never computed: compute() runs them"
                )
            },
        )?;
    }
    read.values
        .iter()
        .map(|value| bridge::to_python(py, value))
        .collect()
}

/// What a read outside a computation gives for a value its formula has not
/// produced, or whose formula failed: nothing where it was never computed, and
/// a warning saying so; where its formula failed, the last value it gave — the
/// one its file keeps — with a warning saying it failed, and nothing where it
/// never gave one. Within a computation, a failure is raised as it always was.
fn served<T>(
    py: Python<'_>,
    what: &str,
    outcome: Result<T, crate::core::property::ComputeError>,
    (absent, last): (T, Option<T>),
    never: bool,
    waits: &[String],
) -> PyResult<T> {
    use crate::core::property::ComputeError;
    let reads_compute = crate::core::property::reads_compute();
    match outcome {
        Err(ComputeError::Failed { source }) if !reads_compute => match last {
            Some(last) => {
                warn_value(
                    py,
                    &format!("{what} failed when last computed: {source}; read as its last value"),
                )?;
                Ok(last)
            }
            None => {
                warn_value(py, &format!("{what} failed when last computed: {source}"))?;
                Ok(absent)
            }
        },
        Ok(value) => {
            if never && !reads_compute {
                // A value waiting for an input nobody entered is said so:
                // "compute() runs it" promised what `compute()` then declined.
                if waits.is_empty() {
                    warn_value(py, &format!("{what} was never computed: compute() runs it"))?;
                } else {
                    warn_value(py, &waiting_sentence(what, waits, &[]))?;
                }
            }
            Ok(value)
        }
        other => other.or_raise(py),
    }
}

/// The value a failed formula last gave, which a read serves.
fn last_value(property: &Property) -> Option<Value> {
    property.last_good().map(|last| last.value.clone())
}

/// The uncertainty a failed formula last gave beside its value.
fn last_uncertainty(property: &Property) -> Option<Option<Uncertainty>> {
    property.last_good().map(|last| last.uncertainty)
}

/// What a value never computed waits for, when a read is about to say so.
fn waiting(sample: &SharedSample, name: &Identifier, never: bool) -> PyResult<Vec<String>> {
    if !never {
        return Ok(Vec::new());
    }
    sample.read_sample("what a value waits for", |held| {
        Ok(waits_for(
            held,
            &Target::Property(name.clone()),
            &mut Vec::new(),
        ))
    })
}

/// A value only a formula could give, which none has given yet.
fn never_valued(property: &Property) -> bool {
    (property.is_computed() && property.peek_value().is_none() && !property.has_failed())
        // Readings whose statistic nothing took give `None`, and say why, as
        // a formula's value never computed does: silent, `None` read as data.
        || (property.declared_location().is_some()
            && property.readings().is_some()
            && property.written_value().is_none()
            && !property.statistic_taken())
}

fn never_uncertain(property: &Property) -> bool {
    property.has_uncertainty_formula()
        && property.peek_uncertainty().is_none()
        && !property.has_failed()
}

/// Each failure the file now records, its traceback kept beside the project as
/// the command line keeps it: the file says what was raised, and `explain`
/// shows where. Best effort, as the worker's is — a log that cannot be written
/// never turns a save into a failure.
fn log_failures(sample: &Bound<'_, PySample>) -> PyResult<()> {
    let py = sample.py();
    let failures: Vec<(String, Py<PyAny>)> =
        sample
            .borrow()
            .shared()
            .read_sample("a sample's failures", |held| {
                Ok(held
                    .property_names()
                    .into_iter()
                    .filter_map(|name| {
                        let handle = held.property(name).ok()?;
                        let source = handle.peek(Property::failure_source)?;
                        let raised = source.downcast_ref::<bridge::PythonException>()?;
                        Some((name.to_string(), raised.as_raised(py).unbind()))
                    })
                    .collect())
            })?;
    if failures.is_empty() {
        return Ok(());
    }
    let Ok(worker) = py.import("samplekit._worker") else {
        return Ok(());
    };
    let Ok(traceback) = py.import("traceback") else {
        return Ok(());
    };
    for (name, raised) in failures {
        // The model's frames, as the worker says them, the formula standing
        // for them where none is the model's: the log held `_worker.py`'s.
        let formula = worker
            .call_method1("formula_of", (sample, name.as_str()))
            .ok();
        let text = worker
            .call_method1("model_traceback", (raised.bind(py), formula))
            .and_then(|text| text.extract::<String>())
            .or_else(|_| {
                traceback
                    .call_method1("format_exception", (raised.bind(py),))
                    .and_then(|lines| lines.extract::<Vec<String>>())
                    .map(|lines| lines.concat())
            })
            .unwrap_or_else(|_| raised.to_string());
        // The note naming the value is added as the exception leaves the
        // computation, after it was taken as this value's: said here.
        let note = format!("while computing '{name}'");
        let text = match text.contains(&note) {
            true => text,
            false => format!("{}\n{note}\n", text.trim_end()),
        };
        let _ = worker.call_method1("record_failure", (sample, name, text));
    }
    Ok(())
}

/// `NAME: ` before a warning about one of a sample's values — its file's stem
/// where the file names none, as a table shows it — or nothing for a sample
/// with neither: forty samples' warnings say nothing of where without it.
fn whose(sample: &SharedSample, file: &FileRef) -> String {
    sample
        .read_sample("the sample's name", |held| {
            Ok(held.name().map(str::to_string))
        })
        .ok()
        .flatten()
        .or_else(|| {
            file.borrow()
                .path
                .as_ref()
                .and_then(|path| path.file_stem())
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .map(|name| format!("{name}: "))
        .unwrap_or_default()
}

/// A stale value is served as it is, and said — unless an input it reads is now
/// one nobody entered: `compute` would not run it again, and the read says what
/// it waits for, as `compute` does.
fn warn_if_stale(
    py: Python<'_>,
    sample: &SharedSample,
    file: &FileRef,
    name: &Identifier,
    named: &str,
) -> PyResult<()> {
    if crate::core::property::reads_compute() {
        return Ok(());
    }
    let (stale, waits) = sample
        .read_sample("whether a value is outdated", |held| {
            let stale = fingerprint::is_stale(held, name).unwrap_or(false);
            let waits = if stale {
                waits_for(held, &Target::Property(name.clone()), &mut Vec::new())
            } else {
                Vec::new()
            };
            Ok((stale, waits))
        })
        .unwrap_or((false, Vec::new()));
    if !stale {
        // A formula changed since it computed the value is said as a changed
        // input is: a read of it was silent. By the description as it stands:
        // a read starts no Python, and imports no model — a figure declared in
        // `.samplekitrc` runs no code of the project's.
        if let Some(said) = planned_now(sample, file)
            .get(name.as_str())
            .filter(|said| said.contains("formula changed"))
        {
            return warn_value(
                py,
                &format!("{named} is outdated — {said}: compute() runs it again"),
            );
        }
        return Ok(());
    }
    if waits.is_empty() {
        warn_value(py, &format!("{named} is outdated: compute() runs it again"))
    } else {
        warn_value(py, &waiting_sentence(named, &waits, &[]))
    }
}

/// What a script's saves have changed, project by project, since its first save
/// in each: one snapshot for the whole script, taken when it ends or at `keep`,
/// rather than one per save.
struct HistorySession {
    writings: Vec<(Vec<PathBuf>, crate::config::version_control::Writing)>,
    /// The files saved, from their project's folder, for a script that has
    /// no file of its own to be named by.
    saved: Vec<String>,
    /// What ran, read at the process's first save and kept for every
    /// snapshot after: the file may be edited, or the folder changed, before
    /// the script ends.
    script: Option<ScriptRun>,
    /// The snapshots this script took, which a file it writes is tied to while
    /// nothing has changed since.
    taken: Vec<String>,
    /// What the script's saves keep in each project's trace.
    tracing: crate::collection::computation::Tracing,
}

/// What a snapshot says ran: a script, its command line and its source as
/// it was when it first saved — or Python with no file to keep.
#[derive(Clone)]
enum ScriptRun {
    /// `python raise.py --dry`: the command line, quoted as a shell would
    /// take it; the script as typed; its file's name and source.
    File {
        command: String,
        typed: String,
        name: String,
        source: Vec<u8>,
    },
    /// `python -c`, or a script read from standard input: said so. The code
    /// `-c` ran is kept, read from `sys.orig_argv`; standard input is read by
    /// then, and nothing of it is kept.
    Inline(&'static str, Option<Vec<u8>>),
    /// An interactive session, or nothing to say which.
    Unnamed,
}

static HISTORY_SESSION: std::sync::Mutex<HistorySession> = std::sync::Mutex::new(HistorySession {
    writings: Vec::new(),
    saved: Vec::new(),
    script: None,
    taken: Vec::new(),
    tracing: crate::collection::computation::Tracing::new(),
});

fn canonical(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A word as a shell takes it, as the command line's own snapshots quote
/// theirs: left bare where it holds nothing a shell reads.
fn shell_quoted(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./=,:@+%".contains(c))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What is running: `sys.argv`, and the script's file — found by
/// `__main__.__file__`, absolute whatever folder the script moved to, or by
/// `argv[0]` — read now.
fn script_run(py: Python<'_>) -> ScriptRun {
    let argv: Vec<String> = py
        .import("sys")
        .and_then(|sys| sys.getattr("argv"))
        .and_then(|argv| argv.extract())
        .unwrap_or_default();
    let Some(typed) = argv.first() else {
        return ScriptRun::Unnamed;
    };
    match typed.as_str() {
        "-c" => {
            // `sys.argv` holds `-c` alone; the interpreter's own command line
            // holds the code after it.
            let original: Vec<String> = py
                .import("sys")
                .and_then(|sys| sys.getattr("orig_argv"))
                .and_then(|argv| argv.extract())
                .unwrap_or_default();
            let code = original
                .iter()
                .position(|word| word == "-c")
                .and_then(|at| original.get(at + 1))
                .map(|code| code.as_bytes().to_vec());
            return ScriptRun::Inline("-c", code);
        }
        "-" => return ScriptRun::Inline("-", None),
        "" => return ScriptRun::Unnamed,
        _ => {}
    }
    let main_file: Option<PathBuf> = py
        .import("__main__")
        .and_then(|main| main.getattr("__file__"))
        .and_then(|file| file.extract::<String>())
        .ok()
        .map(PathBuf::from);
    let path = main_file
        .filter(|path| path.is_file())
        .or_else(|| Some(PathBuf::from(typed)).filter(|path| path.is_file()))
        .map(|path| canonical(&path));
    let Some(path) = path else {
        return ScriptRun::Unnamed;
    };
    let Ok(source) = fs::read(&path) else {
        return ScriptRun::Unnamed;
    };
    ScriptRun::File {
        command: argv
            .iter()
            .map(|word| shell_quoted(word))
            .collect::<Vec<_>>()
            .join(" "),
        typed: shell_quoted(typed),
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        source,
    }
}

/// Before a save: what the projects it reaches held, taken once per project
/// for the whole script. The projects whose files could not be read.
fn history_begin(target: &Path) -> Vec<(PathBuf, crate::config::version_control::VcsError)> {
    use crate::config::version_control;
    let roots: Vec<PathBuf> = version_control::projects_of(&[target.to_path_buf()])
        .iter()
        .map(|config| canonical(config.root()))
        .collect();
    let Ok(mut session) = HISTORY_SESSION.lock() else {
        return Vec::new();
    };
    let known = !roots.is_empty()
        && roots
            .iter()
            .all(|root| session.writings.iter().any(|(held, _)| held.contains(root)));
    if known {
        return Vec::new();
    }
    let (writing, unread) = version_control::before_writing(&[target.to_path_buf()]);
    session.writings.push((roots, writing));
    unread
}

/// After a save: its file named for the snapshot, the script read where
/// this is the process's first save, and the history's failures said as
/// warnings — the save stands.
fn history_saved(
    py: Python<'_>,
    path: &Path,
    unread: Vec<(PathBuf, crate::config::version_control::VcsError)>,
    changed: bool,
) -> PyResult<()> {
    use crate::config::version_control;
    let named = version_control::projects_of(std::slice::from_ref(&path.to_path_buf()))
        .first()
        .and_then(|config| {
            canonical(path)
                .strip_prefix(canonical(config.root()))
                .ok()
                .map(|relative| relative.display().to_string())
        })
        .unwrap_or_else(|| path.display().to_string());
    let unknown = HISTORY_SESSION
        .lock()
        .map(|session| session.script.is_none())
        .unwrap_or(false);
    // Read outside the lock: reading `sys` runs Python.
    let script = unknown.then(|| script_run(py));
    if let Ok(mut session) = HISTORY_SESSION.lock() {
        if changed && !session.saved.contains(&named) {
            session.saved.push(named);
        }
        if session.script.is_none() {
            session.script = script;
        }
    }
    for (_, error) in unread {
        warn_value(py, &format!("the history was not kept: {error}"))?;
    }
    Ok(())
}

/// The script's saves kept as one snapshot in each project they changed,
/// saying `message` — or the script's command line, or the files saved —
/// and the script's source kept beside it. Run when the interpreter ends, by
/// `samplekit.keep()`, and before a figure or an export is tied to a
/// snapshot. False where nothing was saved since the last one.
#[pyfunction]
#[pyo3(signature = (message=None))]
fn _history_flush(py: Python<'_>, message: Option<String>) -> PyResult<bool> {
    use crate::config::version_control;
    let (writings, saved, script, tracing) = {
        let Ok(mut session) = HISTORY_SESSION.lock() else {
            return Ok(false);
        };
        (
            std::mem::take(&mut session.writings),
            std::mem::take(&mut session.saved),
            session.script.clone(),
            std::mem::take(&mut session.tracing),
        )
    };
    // What the script computed, kept in each project's trace before its
    // snapshot, so that the model's change is said with its formulas.
    for (_, error) in tracing.keep() {
        warn_value(
            py,
            &format!("the trace of the formulas was not kept: {error}"),
        )?;
    }
    if writings.is_empty() {
        return Ok(false);
    }
    let script = script.unwrap_or_else(|| script_run(py));
    // A message given still names what ran: `python raise.py · message`.
    let said = match (&script, &message) {
        (ScriptRun::File { typed, .. }, Some(message)) => format!("python {typed} · {message}"),
        (ScriptRun::File { command, .. }, None) => format!("python {command}"),
        (ScriptRun::Inline(flag, _), Some(message)) => format!("python {flag} · {message}"),
        (ScriptRun::Inline(flag, _), None) => {
            format!("python {flag} · saved {}", saved.join(", "))
        }
        (ScriptRun::Unnamed, Some(message)) => format!("python · {message}"),
        (ScriptRun::Unnamed, None) => format!("python · saved {}", saved.join(", ")),
    };
    let said = if SAVE_INTERRUPTED.swap(false, std::sync::atomic::Ordering::SeqCst) {
        format!("{said} — {}", version_control::INTERRUPTED)
    } else {
        said
    };
    let source = script_source(script);
    // Each project's last snapshot before: one that moved is the script's
    // own, which a file it writes next is tied to.
    let projects: Vec<crate::config::project_config::ProjectConfig> = writings
        .iter()
        .flat_map(|(roots, _)| version_control::projects_of(roots))
        .collect();
    let before: Vec<Option<String>> = projects
        .iter()
        .map(|config| version_control::head(config).ok().flatten())
        .collect();
    for (_, writing) in writings {
        for (_, error) in version_control::after_writing_with(writing, &said, source.as_ref()) {
            warn_value(py, &format!("the history was not kept: {error}"))?;
        }
    }
    for said in version_control::notices() {
        warn_value(py, &said)?;
    }
    let taken: Vec<String> = projects
        .iter()
        .zip(before)
        .filter_map(|(config, before)| {
            version_control::head(config)
                .ok()
                .flatten()
                .filter(|now| Some(now) != before.as_ref())
        })
        .collect();
    if let Ok(mut session) = HISTORY_SESSION.lock() {
        session.taken.extend(taken);
    }
    Ok(true)
}

/// The script kept beside a snapshot: its file's name and source, or the code
/// `python -c` ran.
fn script_source(script: ScriptRun) -> Option<(String, Vec<u8>)> {
    match script {
        ScriptRun::File { name, source, .. } => Some((name, source)),
        ScriptRun::Inline(flag, Some(source)) => Some((format!("python {flag}"), source)),
        ScriptRun::Inline(_, None) | ScriptRun::Unnamed => None,
    }
}

/// What ran, as a file written from it is recorded: `python` and the script
/// as it was typed — `python scripts/report.py` — as its snapshot names it
/// with a message; `python -c`, or `python` alone in an interactive session.
fn script_said(py: Python<'_>) -> String {
    let known = HISTORY_SESSION
        .lock()
        .ok()
        .and_then(|session| session.script.clone());
    match known.unwrap_or_else(|| script_run(py)) {
        ScriptRun::File { typed, .. } => format!("python {typed}"),
        ScriptRun::Inline(flag, _) => format!("python {flag}"),
        ScriptRun::Unnamed => "python".to_string(),
    }
}

#[pyfunction(name = "_script_said")]
fn script_said_to_python(py: Python<'_>) -> String {
    script_said(py)
}

/// Before a file is written from the samples at `paths`: in each of their
/// projects, the snapshot it is made from, by the project's folder. The
/// script's saves so far are its snapshot; a script that saved nothing, or
/// nothing since its last snapshot, takes one of its own, its source kept
/// beside it — a script is a step of the history when it writes anything.
/// `written` names the files, for a snapshot of a script with no file of its
/// own to be named by. A history not kept is a warning.
fn script_snapshots(
    py: Python<'_>,
    paths: &[PathBuf],
    written: &[PathBuf],
) -> PyResult<Vec<(crate::config::project_config::ProjectConfig, String)>> {
    use crate::config::version_control;
    _history_flush(py, None)?;
    let known = HISTORY_SESSION
        .lock()
        .ok()
        .and_then(|session| session.script.clone());
    let script = match known {
        Some(script) => script,
        None => {
            // Read once, at its first write, as a first save reads it.
            let script = script_run(py);
            if let Ok(mut session) = HISTORY_SESSION.lock()
                && session.script.is_none()
            {
                session.script = Some(script.clone());
            }
            script
        }
    };
    let mut found = Vec::new();
    for config in version_control::projects_of(paths) {
        if !version_control::keeps_history(&config) {
            continue;
        }
        // What changed outside SampleKit is its own snapshot first: the
        // script's is of what it found, not of an editor's change. The snapshot
        // that holds the files as they are: a join where no other does.
        let head = version_control::snapshot_for_output(&config);
        let head = match head {
            Ok(head) => head,
            Err(error) => {
                warn_value(py, &format!("the history was not kept: {error}"))?;
                continue;
            }
        };
        let ours = HISTORY_SESSION
            .lock()
            .map(|session| {
                head.as_ref()
                    .is_some_and(|head| session.taken.contains(head))
            })
            .unwrap_or(false);
        if ours && let Some(head) = head {
            found.push((config, head));
            continue;
        }
        let root = canonical(config.root());
        let named: Vec<String> = written
            .iter()
            .map(|path| {
                canonical(path)
                    .strip_prefix(&root)
                    .map(|relative| relative.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string())
            })
            .collect();
        let said = match &script {
            ScriptRun::File { command, .. } => format!("python {command}"),
            ScriptRun::Inline(flag, _) => format!("python {flag} · wrote {}", named.join(", ")),
            ScriptRun::Unnamed => format!("python · wrote {}", named.join(", ")),
        };
        match version_control::script_step(&config, &said, script_source(script.clone()).as_ref()) {
            Ok(Some(id)) => {
                if let Ok(mut session) = HISTORY_SESSION.lock() {
                    session.taken.push(id.clone());
                }
                found.push((config, id));
            }
            Ok(None) => {}
            Err(error) => warn_value(py, &format!("the history was not kept: {error}"))?,
        }
    }
    Ok(found)
}

/// Before a figure is written from these samples: in each of their projects,
/// the snapshot it is made from, and its id. From a script, `written` names
/// the files, and the script is a snapshot of its own where it has none
/// yet; the command line's drawing names none, its command keeping the
/// history. A history not kept is a warning.
#[pyfunction]
#[pyo3(signature = (paths, written=None))]
fn _history_before_output(
    py: Python<'_>,
    paths: Vec<PathBuf>,
    written: Option<Vec<PathBuf>>,
) -> PyResult<Vec<(PathBuf, String)>> {
    use crate::config::version_control;
    if let Some(written) = written {
        return Ok(script_snapshots(py, &paths, &written)?
            .into_iter()
            .map(|(config, id)| (config.root().to_path_buf(), id))
            .collect());
    }
    let mut found = Vec::new();
    for config in version_control::projects_of(&paths) {
        match version_control::snapshot_for_output(&config) {
            Ok(Some(snapshot)) => found.push((config.root().to_path_buf(), snapshot)),
            Ok(None) => {}
            Err(error) => warn_value(py, &format!("the history was not kept: {error}"))?,
        }
    }
    Ok(found)
}

/// After the figure is written: the file tied to the snapshot it was made
/// from, in the history of the project at `root`.
#[pyfunction]
fn _history_tag_output(
    py: Python<'_>,
    root: PathBuf,
    snapshot: String,
    written: PathBuf,
    said: String,
    samples: Vec<PathBuf>,
) -> PyResult<()> {
    use crate::config::{project_config, version_control};
    let Some(config) = project_config::find(&root).and_then(|rc| project_config::load(&rc).ok())
    else {
        return Ok(());
    };
    if let Err(error) = version_control::tag_output(
        &config,
        &snapshot,
        &written,
        &said,
        &samples,
        &std::env::current_dir().unwrap_or_default(),
    ) {
        warn_value(
            py,
            &format!("the history does not record {}: {error}", written.display()),
        )?;
    }
    Ok(())
}

fn warn_value(py: Python<'_>, message: &str) -> PyResult<()> {
    let message = std::ffi::CString::new(message).unwrap_or_default();
    PyErr::warn(
        py,
        py.get_type::<pyo3::exceptions::PyUserWarning>().as_any(),
        &message,
        callers_level(py),
    )
}

/// The stack level of the first frame outside the package: the script's line
/// that read, computed or printed. A fixed level named `<sys>:0` from a
/// script's top level, where there is no frame above the caller, and a line of
/// the package's own where a figure read the value; and Python's filter keys a
/// warning by that line, so every such warning pointed at one place.
fn callers_level(py: Python<'_>) -> i32 {
    let found = || -> PyResult<i32> {
        let package = py
            .import("samplekit")?
            .getattr("__file__")?
            .extract::<PathBuf>()?;
        let package = package.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut frame = py.import("sys")?.call_method1("_getframe", (0,))?;
        let mut level = 1;
        loop {
            let file: PathBuf = frame.getattr("f_code")?.getattr("co_filename")?.extract()?;
            let back = frame.getattr("f_back")?;
            if !file.starts_with(&package) || back.is_none() {
                return Ok(level);
            }
            frame = back;
            level += 1;
        }
    };
    found().unwrap_or(1)
}

/// A file a list could not read, said as a warning rather than raised.
fn warn_unread(py: Python<'_>, path: &Path, reason: &str) -> PyResult<()> {
    let message = std::ffi::CString::new(format!("{} was not read: {reason}", path.display()))
        .unwrap_or_default();
    PyErr::warn(
        py,
        py.get_type::<pyo3::exceptions::PyUserWarning>().as_any(),
        &message,
        callers_level(py),
    )
}

fn load_directory(
    py: Python<'_>,
    directory: &Path,
    pattern: Option<String>,
    model: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<Py<PyAny>>> {
    let config = if directory.is_dir() {
        project::load_for(directory).or_raise(py)?.map(Rc::new)
    } else {
        config_for(py, directory)?
    };
    // Read as the command line reads it: each project below by its own
    // `[collection]`, a draft its rules exclude left out. A `pattern=` states
    // the rules instead.
    let found = match pattern {
        None => bridge::without_gil(py, || discovery::discover(directory, config.as_deref()))
            .or_raise(py)?,
        Some(pattern) => {
            let mut rules = config
                .as_ref()
                .map(|config| config.collection().clone())
                .unwrap_or_default();
            rules.include = vec![pattern];
            bridge::without_gil(py, || discovery::discover_with(directory, &rules)).or_raise(py)?
        }
    };
    // A file that will not load is set aside and said, as the command line says
    // it; a model's own failure on a readable file is raised.
    for skipped in &found.skipped {
        if let discovery::SkipReason::Malformed { .. }
        | discovery::SkipReason::Unreadable { .. }
        | discovery::SkipReason::Configuration { .. } = skipped.reason
        {
            warn_unread(py, &skipped.path, &skipped.reason.to_string())?;
        }
    }
    let loaded = bridge::without_gil(py, || {
        found
            .paths
            .iter()
            .map(|path| (path.clone(), document::load_sample(path)))
            .collect::<Vec<_>>()
    });
    let mut readable = Vec::with_capacity(loaded.len());
    for (path, outcome) in loaded {
        match outcome {
            Ok((sample, origin)) => readable.push((path, sample, origin)),
            Err(error) => warn_unread(py, &path, &error.to_string())?,
        }
    }
    // Each sample follows its own project, as on the command line: a folder
    // above several projects reads each one's samples with that project's
    // configuration and model, not the folder's own.
    let mut configs: Vec<(Option<PathBuf>, Option<Rc<ProjectConfig>>)> = Vec::new();
    let mut loaded = Vec::with_capacity(readable.len());
    for (path, sample, origin) in readable {
        let found = project::find(path.parent().unwrap_or(directory));
        let own = match configs.iter().find(|(file, _)| *file == found) {
            Some((_, config)) => config.clone(),
            None => {
                let read = match &found {
                    Some(file) => Some(Rc::new(project::load(file).or_raise(py)?)),
                    None => None,
                };
                configs.push((found, read.clone()));
                read
            }
        };
        let object = match model_class(py, model, own.as_deref())? {
            Some(class) => class.bind(py).call1((python_path(py, &path)?,))?.unbind(),
            None => wrap_loaded(py, sample, path, origin, own)?,
        };
        loaded.push(object);
    }
    Ok(loaded)
}

#[pymethods]
impl PySampleList {
    #[new]
    #[pyo3(signature = (source=None, pattern=None, model=None))]
    fn new(
        py: Python<'_>,
        source: Option<&Bound<'_, PyAny>>,
        pattern: Option<String>,
        model: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let Some(source) = source.filter(|source| !source.is_none()) else {
            if pattern.is_some() {
                return Err(PyValueError::new_err(
                    "pattern= selects the files of a directory, and no directory was given",
                ));
            }
            return PySampleList::of(py, Vec::new());
        };
        if is_path_like(source)? {
            let directory = file_path_from(source)?;
            return PySampleList::of(py, load_directory(py, &directory, pattern, model)?);
        }
        if pattern.is_some() || model.is_some_and(|model| !model.is_none()) {
            return Err(PyValueError::new_err(
                "pattern= and model= apply to a directory, and these are samples already",
            ));
        }
        let mut samples = Vec::new();
        for item in source.try_iter()? {
            let item = item?;
            if item.cast::<PySample>().is_ok() {
                samples.push(item.unbind());
            } else if is_path_like(&item)? {
                return Err(PyTypeError::new_err(
                    "a list is built from samples, and this is a path: \
                     SampleList([Sample(p) for p in paths]) loads each one",
                ));
            } else {
                return Err(PyTypeError::new_err(format!(
                    "a list is built from samples, and this is {}",
                    bridge::a_type(&item)
                )));
            }
        }
        PySampleList::of(py, samples)
    }

    fn __len__(&self) -> usize {
        self.samples.borrow().len()
    }

    fn __getitem__(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let objects = self.objects(py);
        if let Ok(slice) = key.cast::<PySlice>() {
            let indices = slice.indices(objects.len() as isize)?;
            let mut chosen = Vec::new();
            let mut at = indices.start;
            while (indices.step > 0 && at < indices.stop) || (indices.step < 0 && at > indices.stop)
            {
                chosen.push(objects[at as usize].clone_ref(py));
                at += indices.step;
            }
            return Ok(Py::new(py, self.derived(py, chosen)?)?.into_any());
        }
        if let Ok(name) = key.cast::<PyString>() {
            let name = name.to_str()?;
            let mut matching = Vec::new();
            let mut names = Vec::new();
            for object in &objects {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                if let Some(held) = sample.name()? {
                    if held == name {
                        matching.push(object.clone_ref(py));
                    }
                    names.push(held);
                }
            }
            return match matching.len() {
                1 => Ok(matching.remove(0)),
                0 => Err(bridge::key_error(unknown_message("sample", name, &names))),
                count => Err(bridge::key_error(format!(
                    "{count} samples are named '{name}': address one by position"
                ))),
            };
        }
        if key.is_instance_of::<PyBool>() {
            return Err(PyTypeError::new_err(
                "a list is indexed by an int, a slice or a name",
            ));
        }
        let position: isize = key
            .extract()
            .map_err(|_| PyTypeError::new_err("a list is indexed by an int, a slice or a name"))?;
        let length = objects.len() as isize;
        let at = if position < 0 {
            position + length
        } else {
            position
        };
        if at < 0 || at >= length {
            return Err(PyIndexError::new_err(format!(
                "there is no sample {position}: the list has {length}"
            )));
        }
        Ok(objects[at as usize].clone_ref(py))
    }

    fn __setitem__(&self, _key: &Bound<'_, PyAny>, _value: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyTypeError::new_err(COMPOSED))
    }

    fn __delitem__(&self, _key: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyTypeError::new_err(COMPOSED))
    }

    fn __contains__(&self, py: Python<'_>, item: &Bound<'_, PyAny>) -> PyResult<bool> {
        Ok(!self.position_of(py, item)?.is_empty())
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.objects(py))?.try_iter()
    }

    fn __reversed__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        let mut objects = self.objects(py);
        objects.reverse();
        PyList::new(py, objects)?.try_iter()
    }

    /// The position of a sample in the list.
    ///
    /// Args:
    ///     value: A sample, or a sample's name.
    ///     start: Where to start looking.
    ///     stop: Where to stop looking.
    ///
    /// Raises:
    ///     ValueError: It is not in the list.
    #[pyo3(signature = (value, start=0, stop=None))]
    fn index(
        &self,
        py: Python<'_>,
        value: &Bound<'_, PyAny>,
        start: isize,
        stop: Option<isize>,
    ) -> PyResult<usize> {
        // Bounds as a slice takes them, which is what `Sequence.index` promises.
        let length = self.objects(py).len() as isize;
        let bound = |at: isize| -> usize {
            (if at < 0 { at + length } else { at }).clamp(0, length) as usize
        };
        let (start, stop) = (bound(start), bound(stop.unwrap_or(length)));
        let positions: Vec<usize> = self
            .position_of(py, value)?
            .into_iter()
            .filter(|position| (start..stop).contains(position))
            .collect();
        // A name two samples carry is no position, as `list["EXAMPLE"]`
        // refuses it: the first was answered in silence.
        if positions.len() > 1 && value.extract::<String>().is_ok() {
            return Err(PyValueError::new_err(format!(
                "{} samples are named {}: index by the sample itself, or address one by \
                 position",
                positions.len(),
                bridge::repr(value)
            )));
        }
        positions.into_iter().next().ok_or_else(|| {
            PyValueError::new_err(format!("{} is not in the list", bridge::repr(value)))
        })
    }

    /// How many times a sample is in the list: 0 or 1 for a sample, and, for a
    /// name, the number of samples of that name.
    fn count(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<usize> {
        Ok(self.position_of(py, value)?.len())
    }

    fn __add__(&self, py: Python<'_>, other: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let Ok(other) = other.cast::<PySampleList>() else {
            return Ok(py.NotImplemented());
        };
        let mut samples = self.objects(py);
        samples.extend(other.borrow().objects(py));
        Ok(Py::new(py, PySampleList::of(py, samples)?)?.into_any())
    }

    fn __getattr__(slf: &Bound<'_, Self>, name: &str) -> PyResult<Py<PyAny>> {
        // The nearest member is offered, in the words every other refusal uses.
        Err(no_attribute(slf.as_any(), name))
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let count = self.samples.borrow().len();
        Ok(match self.project_choice(py) {
            Ok(ProjectChoice::One(config)) => {
                format!("<SampleList of {count} from {}>", config.root().display())
            }
            _ => format!("<SampleList of {count}>"),
        })
    }

    /// The samples an expression selects, as ``-f`` selects them.
    ///
    /// Args:
    ///     predicate: A filter expression — ``"elongation > 20 && material == steel"`` — or a
    ///         function taking a sample and returning whether to keep it.
    ///
    /// Returns:
    ///     A new list, in this list's order.
    ///
    /// Raises:
    ///     KeyError: The expression names an unknown field; the message names
    ///         the nearest.
    ///     ValueError: The expression does not parse, or names a tag no sample
    ///         carries.
    ///     TypeError: The expression compares a field in a way its values do not
    ///         allow.
    ///
    /// Example:
    ///     >>> [specimen.name for specimen in specimens.filter("tensile_strength > 300")]
    ///     ['aluminium-01', 'steel-01']
    fn filter(&self, py: Python<'_>, predicate: &Bound<'_, PyAny>) -> PyResult<PySampleList> {
        let Ok(expression) = predicate.cast::<PyString>() else {
            if !predicate.is_callable() {
                return Err(PyTypeError::new_err(
                    "filter takes an expression, as -f writes one, or a function of a sample",
                ));
            }
            let mut kept = Vec::new();
            for object in self.objects(py) {
                if predicate.call1((object.bind(py),))?.is_truthy()? {
                    kept.push(object);
                }
            }
            return self.derived(py, kept);
        };
        let source = expression.to_str()?;
        let parsed = filter::parse(source).map_err(|error| {
            PyValueError::new_err(
                filter::caret(source, &error).unwrap_or_else(|| error.to_string()),
            )
        })?;
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        let _recording = self.recording(py)?;
        let checking = self.checking_list(py)?;
        let diagnostics = {
            let guards: Vec<_> = checking.iter().map(|entry| entry.sample.borrow()).collect();
            let samples: Vec<&Sample> = guards.iter().map(|guard| &**guard).collect();
            filter::check(&parsed, &samples)
        };
        if let Some(unknown) = diagnostics.unknown_fields.first() {
            return Err(bridge::key_error(unknown.to_string()));
        }
        if let Some(misuse) = diagnostics.wrong_operators.first() {
            return Err(PyTypeError::new_err(misuse.error().to_string()));
        }
        if let Some(conflict) = diagnostics.type_conflicts.first() {
            return Err(PyTypeError::new_err(
                filter::FilterError::TypeConflict(Box::new(conflict.clone())).to_string(),
            ));
        }
        // Left aside and named, as the command line does.
        for aside in &diagnostics.set_aside {
            warn_value(py, &aside.said())?;
        }
        // A text no sample holds, one close to it held: the empty answer's
        // likely cause, said as the command line says it.
        for unmatched in &diagnostics.unmatched_texts {
            warn_value(py, &unmatched.to_string())?;
        }
        for unmatched in &diagnostics.unmatched_numbers {
            warn_value(py, unmatched)?;
        }
        if let Some(tag) = diagnostics.unknown_tags.first() {
            let mut message = format!("no sample carries the tag '{}'", tag.tag);
            if let Some(suggestion) = &tag.suggestion {
                message.push_str(&format!("\n  did you mean: {suggestion}?"));
            }
            return Err(PyValueError::new_err(message));
        }
        bridge::forget_raised();
        // `state` is read only for an expression naming it.
        let words = filter::state_words(&parsed);
        let collection = if words.is_empty() {
            collection
        } else {
            let needs_the_model = words.iter().any(|word| word.needs_the_model());
            self.with_states(py, &collection, needs_the_model)?
        };
        let kept = collection.filter(&parsed).or_raise(py)?;
        let objects = self.objects_of(py, &kept)?;
        self.derived(py, objects)
    }

    /// Sort the list in place, and return ``None``.
    ///
    /// Args:
    ///     key: A field — ``"area"``, or ``"-area"`` for descending — several in a
    ///         list or comma-separated, or a function of a sample as ``sorted``
    ///         takes one.
    ///
    /// Raises:
    ///     KeyError: An unknown field; the message names the nearest.
    ///     TypeError: A sort by ``state``, which a sample holds several of.
    fn sort(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<()> {
        let sorted = self.sorted_objects(py, key)?;
        *self.samples.borrow_mut() = sorted;
        self.sorted.set(true);
        Ok(())
    }

    /// A new list in sorted order; this one is unchanged.
    ///
    /// Args:
    ///     key: A field — ``"area"``, or ``"-area"`` for descending — several in a
    ///         list or comma-separated, or a function of a sample as ``sorted``
    ///         takes one.
    ///
    /// Returns:
    ///     A new list.
    ///
    /// Raises:
    ///     KeyError: An unknown field; the message names the nearest.
    ///     TypeError: A sort by ``state``, which a sample holds several of.
    ///
    /// Example:
    ///     >>> specimens.sorted("-tensile_strength")[0].name
    ///     'steel-01'
    fn sorted(&self, py: Python<'_>, key: &Bound<'_, PyAny>) -> PyResult<PySampleList> {
        let sorted = self.sorted_objects(py, key)?;
        let list = self.derived(py, sorted)?;
        list.sorted.set(true);
        Ok(list)
    }

    /// The samples a saved query selects, as ``-Q`` selects them.
    ///
    /// Args:
    ///     name: The query's name in ``.samplekitrc``, or a ``Query``.
    ///
    /// Returns:
    ///     A new list, in this list's order.
    ///
    /// Raises:
    ///     KeyError: No query has that name; the message names the nearest.
    ///     ValueError: The list spans several projects that declare it
    ///         differently.
    ///
    /// Example:
    ///     >>> len(specimens.query("ductile"))
    ///     2
    fn query(&self, py: Python<'_>, name: &Bound<'_, PyAny>) -> PyResult<PySampleList> {
        let declaration: NamedQuery = if let Ok(entry) = name.cast::<PyQuery>() {
            entry.borrow().declaration.clone()
        } else {
            let text: String = name.extract().map_err(|_| {
                PyTypeError::new_err("a query is a declared name or project.queries.<name>")
            })?;
            let config = self.single_config(py, "query", &text)?;
            named_queries::named(&config, &text).or_raise(py)?.clone()
        };
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        let _recording = self.recording(py)?;
        bridge::forget_raised();
        let kept = named_queries::run(&declaration, &collection).or_raise(py)?;
        let objects = self.objects_of(py, &kept)?;
        self.derived(py, objects)
    }

    /// The statistics of one field over the list, as ``--summary`` gives them.
    ///
    /// Args:
    ///     field: A field: ``"area"``, ``"area.u"``, ``"curve.force[0.2]"``.
    ///
    /// Returns:
    ///     A ``Summary``. Its mean is weighted by 1/u² where every value has an
    ///     uncertainty above zero.
    ///
    /// Raises:
    ///     KeyError: An unknown field; the message names the nearest.
    ///     ValueError: No sample of the list holds a number there.
    ///
    /// Example:
    ///     >>> round(specimens.stats("tensile_strength").mean, 1)
    ///     352.3
    fn stats(&self, py: Python<'_>, field: &Bound<'_, PyAny>) -> PyResult<PySummary> {
        let path = field_path(field)?;
        let parsed = fields::parse(&path).or_raise(py)?;
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        let _recording = self.recording(py)?;
        bridge::forget_raised();
        let summarised = collection.summarize(&parsed).or_raise(py)?;
        // The row as `--summary` writes it — its label, its unit, the
        // project's precision — read where the command line reads them.
        let row = {
            let mut held = Vec::new();
            for object in self.objects(py) {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                let file = sample.file.borrow();
                if let (Some(path), Some(config)) = (&file.path, &file.config) {
                    held.push((path.clone(), (**config).clone()));
                }
            }
            let selected = self.rust_list(py)?.with_configurations(held);
            let described = self.described_list(py)?;
            profiles::parse_column(&path)
                .ok()
                .map(|column| profiles::anonymous(vec![column]))
                .and_then(|profile| {
                    crate::presentation::summaries::rows(
                        &selected, &profile, &described, None, true,
                    )
                    .ok()
                })
                .and_then(|(rows, _)| rows.into_iter().next())
        };
        match summarised {
            // Weighted by 1/u² where every value has an uncertainty, as the
            // command line's summary takes it.
            Some(column) => Ok(PySummary {
                weighted_mean: column.weighted_mean,
                weighted_error: column.weighted_error,
                considered: Some(column.considered),
                not_applicable: column.not_applicable,
                summary: column.summary,
                row,
            }),
            None => Err(PyValueError::new_err(match collection.len() {
                0 => format!("'{path}' has nothing to summarise: the list holds no sample"),
                1 => format!(
                    "'{path}' has no numeric value in the one sample of the list, so there is \
                     nothing to summarise"
                ),
                count => format!(
                    "'{path}' has no numeric value in any of these {count} samples, so there is \
                     nothing to summarise"
                ),
            })),
        }
    }

    /// Split the list into groups by the values of fields, as ``--group`` does.
    ///
    /// Args:
    ///     fields: A field, or several — a list, or comma-separated as
    ///         ``--group`` writes them.
    ///
    /// Returns:
    ///     A dict from each value — a tuple of values for several fields — to
    ///     the list of samples holding it. The groups come in the order their
    ///     values sort; for a sorted list, in the order of their first samples.
    ///
    /// Raises:
    ///     KeyError: An unknown field; the message names the nearest.
    ///     ValueError: The field is a whole table.
    ///     RuntimeError: The field is ``state``, which a sample holds several of.
    ///
    /// Example:
    ///     >>> list(specimens.group_by("material"))
    ///     ['aluminium', 'copper', 'steel']
    fn group_by(&self, py: Python<'_>, fields: &Bound<'_, PyAny>) -> PyResult<Py<PyDict>> {
        let paths = group_fields(fields)?;
        let parsed: Vec<fields::Field> = paths
            .iter()
            .map(|path| fields::parse(path).or_raise(py))
            .collect::<PyResult<_>>()?;
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        let _recording = self.recording(py)?;
        bridge::forget_raised();
        // Checked against the list this one came from, as a filter is: a
        // list holding none of a field's samples still knows the field.
        let order = self.group_order();
        if collection.is_empty() {
            self.checking_list(py)?
                .group_by(&parsed, order)
                .or_raise(py)?;
        }
        let groups = collection.group_by(&parsed, order).or_raise(py)?;
        let grouped = PyDict::new(py);
        for group in groups {
            let values: Vec<Py<PyAny>> = group
                .key
                .iter()
                .map(|value| bridge::to_python(py, value))
                .collect::<PyResult<_>>()?;
            let key: Py<PyAny> = if values.len() == 1 {
                values.into_iter().next().expect("one value")
            } else {
                PyTuple::new(py, values)?.into_any().unbind()
            };
            let objects = self.objects_of(py, &group.samples)?;
            grouped.set_item(key, Py::new(py, self.derived(py, objects)?)?)?;
        }
        Ok(grouped.unbind())
    }

    /// Compute every sample of the list, as ``Sample.compute`` does.
    ///
    /// Every sample is computed even when one fails; the failures are raised at
    /// the end. A ``KeyboardInterrupt`` stops at once.
    ///
    /// Args:
    ///     *names: The values to compute; every value that is not current by
    ///         default.
    ///     rerun: Run them even when they are current.
    ///     force: Run them even when they are edited.
    ///
    /// Raises:
    ///     Exception: The one failure, with a note naming the sample and the
    ///         value.
    ///     ExceptionGroup: Several failures, in the order they happened.
    #[pyo3(signature = (*names, rerun=false, force=false))]
    fn compute(
        &self,
        py: Python<'_>,
        names: &Bound<'_, PyTuple>,
        rerun: bool,
        force: bool,
    ) -> PyResult<()> {
        let keywords = PyDict::new(py);
        keywords.set_item("rerun", rerun)?;
        keywords.set_item("force", force)?;
        let mut failed: Vec<PyErr> = Vec::new();
        for object in self.objects(py) {
            let sample = object.bind(py);
            if let Err(error) = sample.call_method("compute", names.clone(), Some(&keywords)) {
                // A person stopping the run stops the list with it: it is not
                // one sample's failure to collect and move past.
                if error.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) {
                    return Err(error);
                }
                // Which sample, beside which value: forty samples' failures
                // raised together say nothing of where without it.
                let whose = match (
                    sample
                        .getattr("name")
                        .ok()
                        .and_then(|name| name.extract::<String>().ok()),
                    sample
                        .getattr("path")
                        .ok()
                        .filter(|path| !path.is_none())
                        .and_then(|path| path.str().ok().map(|text| text.to_string())),
                ) {
                    (Some(name), Some(path)) => format!("in sample '{name}' ({path})"),
                    (Some(name), None) => format!("in sample '{name}'"),
                    (None, Some(path)) => format!("in the sample at {path}"),
                    (None, None) => "in a sample with neither a name nor a file".to_string(),
                };
                let _ = error.value(py).call_method1("add_note", (whose,));
                failed.push(error);
            }
        }
        match failed.len() {
            0 => Ok(()),
            1 => Err(failed.remove(0)),
            count => {
                // `BaseExceptionGroup` is an `ExceptionGroup` whenever every
                // member is an `Exception`, which a formula's error nearly
                // always is; the base class keeps the rare other one legal.
                let members: Vec<Py<PyAny>> = failed
                    .iter()
                    .map(|error| error.value(py).clone().into_any().unbind())
                    .collect();
                let group = py
                    .import("builtins")?
                    .getattr("BaseExceptionGroup")?
                    .call1((format!("{count} samples failed to compute"), members))?;
                Err(PyErr::from_value(group))
            }
        }
    }

    /// Write every sample of the list: back where each was read, or into a folder.
    ///
    /// Without a folder, each sample is written to its own file, as ``save()``
    /// writes it: a file changed since it was read — by another object, or an
    /// editor — is refused rather than written over. Into a folder, each is
    /// written under its file's name, or its name, and nothing is written if
    /// any file is refused.
    ///
    /// Args:
    ///     directory: The folder, which must exist; each sample's own file when
    ///         left out.
    ///     overwrite: Replace existing files in the folder.
    ///
    /// Returns:
    ///     The paths written.
    ///
    /// Raises:
    ///     FileNotFoundError: The folder does not exist.
    ///     NotADirectoryError: It is a file.
    ///     FileExistsError: A file exists and ``overwrite`` is false.
    ///     ValueError: Two samples would be written to the same file, or,
    ///         without a folder, a sample has no file of its own.
    ///     OSError: Without a folder, a file changed on disk since it was read.
    #[pyo3(signature = (directory=None, overwrite=false))]
    #[allow(clippy::too_many_lines)]
    fn save_all(
        &self,
        py: Python<'_>,
        directory: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Vec<Py<PyAny>>> {
        let objects = self.objects(py);
        let Some(directory) = directory else {
            // Back where each was read, with `save()`'s check: a sample with
            // no file of its own is named before anything is written.
            for (position, object) in objects.iter().enumerate() {
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                if sample.file.borrow().path.is_none() {
                    return Err(PyValueError::new_err(format!(
                        "the sample at {position}{} has no file of its own, and nothing was \
                         written: save_all(directory) writes the list into a folder",
                        sample
                            .name()?
                            .map(|name| format!(", {name},"))
                            .unwrap_or_default()
                    )));
                }
            }
            // **Every sample checked before any is written**: a file changed
            // since it was read, removed, or locked by another process
            // stopped the list there, the samples before it written, and the
            // error said *nothing was written*.
            let refused: Vec<String> = objects
                .iter()
                .filter_map(|object| {
                    let sample = object.bind(py).cast::<PySample>().ok()?.borrow();
                    let file = sample.file.borrow();
                    let path = file.path.clone()?;
                    let said = path.display().to_string();
                    let lock = PathBuf::from(format!("{}.lock", path.display()));
                    if lock.exists() && crate::format::lock::abandoned(&lock).is_none() {
                        return Some(format!("  {said}: another process is saving it"));
                    }
                    // A file the system will not let be replaced, refused
                    // with the others before any is written.
                    if std::fs::metadata(&path).is_ok_and(|meta| meta.permissions().readonly()) {
                        return Some(format!("  {said}: read-only"));
                    }
                    let origin = file.origin.as_ref()?;
                    match std::fs::read_to_string(&path) {
                        Err(_) => Some(format!("  {said}: removed or moved since it was read")),
                        Ok(text) if document::digest_of(&text) != origin.digest => {
                            Some(format!("  {said}: changed since it was read"))
                        }
                        Ok(_) => None,
                    }
                })
                .collect();
            if !refused.is_empty() {
                return Err(PyOSError::new_err(format!(
                    "{} of the list cannot be written, and nothing was written:\n{}\n  \
                     save() of each sample, with overwrite=True, replaces a file changed since",
                    refused.len(),
                    refused.join("\n")
                )));
            }
            let caught = catch_interrupts(py);
            let mut written = Vec::with_capacity(objects.len());
            for object in &objects {
                stop_if_interrupted(py, &written, caught.as_ref())?;
                // A file changed between the check and its save: the list is
                // written up to it, which is said — *nothing was written*
                // spoke of that file alone.
                if let Err(error) = object.bind(py).call_method0("save") {
                    let _ = error.value(py).call_method1(
                        "add_note",
                        (format!(
                            "save_all() wrote {} of the {} samples before this one, and none \
                             after it: those written are saved, the rest are not",
                            written.len(),
                            objects.len()
                        ),),
                    );
                    return Err(error);
                }
                let sample = object.bind(py).cast::<PySample>()?.borrow();
                let path = sample
                    .file
                    .borrow()
                    .path
                    .clone()
                    .expect("each sample's file was looked for above");
                written.push(python_path(py, &path)?);
            }
            // Pressed during the last: every file written, and said so.
            stop_if_interrupted(py, &written, caught.as_ref())?;
            return Ok(written);
        };
        let directory = path_from(directory)?;
        // Said before anything is written, and in words: the first save failed
        // with the system's "(os error 2)". Not made, as a path given to
        // `plot(output=…)` or `export(output=…)` is not: a mistyped directory
        // would be created in silence.
        if !directory.is_dir() {
            return Err(if directory.exists() {
                PyNotADirectoryError::new_err(format!(
                    "{} is a file, not a directory, and nothing was written",
                    directory.display()
                ))
            } else {
                PyFileNotFoundError::new_err(format!(
                    "the directory {} does not exist, and nothing was written: create it first",
                    directory.display()
                ))
            });
        }
        let mut destinations = Vec::with_capacity(objects.len());
        for (position, object) in objects.iter().enumerate() {
            let sample = object.bind(py).cast::<PySample>()?.borrow();
            let own = sample.file.borrow().path.clone();
            let file_name = match own {
                Some(path) => path.file_name().map(PathBuf::from),
                None => sample
                    .name()?
                    .map(|name| PathBuf::from(format!("{name}.md"))),
            };
            let Some(file_name) = file_name else {
                return Err(ListError::MissingPath { position }.into_py_err(py));
            };
            destinations.push(directory.join(file_name));
        }
        // **Two samples, one file**: the second was written over the first,
        // whatever `overwrite` said — conflicts were looked for on disk, and a
        // collision inside the list is not on disk yet. Every one is named, and
        // nothing is written.
        // Compared as the system compares them: `S1.md` and `s1.md` are one
        // file on macOS and Windows.
        let mut seen: Vec<String> = Vec::new();
        let mut shared_files: Vec<String> = Vec::new();
        for destination in &destinations {
            let key = crate::collection::sample_list::file_key(destination);
            if seen.contains(&key) {
                let said = format!("  {}", destination.display());
                if !shared_files.contains(&said) {
                    shared_files.push(said);
                }
            } else {
                seen.push(key);
            }
        }
        if !shared_files.is_empty() {
            return Err(PyFileExistsError::new_err(format!(
                "two samples of this list would be written to one file, and nothing was \
                 written:\n{}\n  give them distinct names, or save them into distinct directories",
                shared_files.join("\n")
            )));
        }
        if !overwrite {
            let conflicts: Vec<String> = destinations
                .iter()
                .filter(|path| path.exists())
                .map(|path| format!("  {}", path.display()))
                .collect();
            if !conflicts.is_empty() {
                let (count, them) = match conflicts.len() {
                    1 => ("one destination already exists".to_string(), "it"),
                    many => (format!("{many} destinations already exist"), "them"),
                };
                return Err(PyFileExistsError::new_err(format!(
                    "{count}, and nothing was written:\n{}\n  \
                     save_all(directory, overwrite=True) replaces {them}",
                    conflicts.join("\n")
                )));
            }
        }
        let caught = catch_interrupts(py);
        let mut written = Vec::with_capacity(destinations.len());
        for (object, destination) in objects.iter().zip(destinations) {
            stop_if_interrupted(py, &written, caught.as_ref())?;
            let path = python_path(py, &destination)?;
            let keywords = PyDict::new(py);
            keywords.set_item("overwrite", true)?;
            object
                .bind(py)
                .call_method("save", (path.clone_ref(py),), Some(&keywords))?;
            written.push(path);
        }
        stop_if_interrupted(py, &written, caught.as_ref())?;
        Ok(written)
    }

    /// Every field the samples of the list can be addressed by, as
    /// ``samplekit list fields`` lists them.
    #[getter]
    fn fields(&self, py: Python<'_>) -> PyResult<PyNames> {
        self.prepare(py)?;
        let collection = self.rust_list(py)?;
        let _recording = self.recording(py)?;
        let mut entries = IndexMap::new();
        for field in collection.available_fields() {
            let path = fields::describe(&field);
            entries.insert(path.clone(), Py::new(py, PyField { path })?.into_any());
        }
        Ok(PyNames {
            kind: "field",
            entries,
            origin: "in these samples".to_string(),
        })
    }

    /// The project of the list's samples — what their ``.samplekitrc`` declares.
    ///
    /// Raises:
    ///     ValueError: The samples belong to several projects.
    #[getter]
    fn project(&self, py: Python<'_>) -> PyResult<PyProject> {
        Ok(PyProject {
            config: match self.project_choice(py)? {
                ProjectChoice::One(config) => Some(config),
                ProjectChoice::Outside => None,
            },
        })
    }

    /// The list as a dict of columns, one list of values per column heading.
    ///
    /// Values are the stored numbers, unrounded.
    ///
    /// Args:
    ///     columns: The fields to include: a list, or comma-separated as ``-c``
    ///         writes them, each ``FIELD[:PRECISION][=LABEL]``.
    ///     profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
    ///         instead of ``columns``.
    ///
    /// Returns:
    ///     A dict from each heading to its values, one per sample.
    ///
    /// Raises:
    ///     ValueError: Neither ``columns`` nor ``profile`` is given, or both.
    ///     KeyError: An unknown field or profile; the message names the nearest.
    ///
    /// Example:
    ///     >>> columns = specimens.to_dict(columns="name,area")
    ///     >>> list(columns)
    ///     ['name', 'area_value', 'area_uncertainty']
    #[pyo3(signature = (columns=None, profile=None))]
    fn to_dict<'py>(
        &self,
        py: Python<'py>,
        columns: Option<&Bound<'py, PyAny>>,
        profile: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let (profile, declared) = self.export_profile(py, columns, profile)?;
        // A declared precision shapes text, and a dict is not text: `to_dict`
        // hands Python the stored float.
        let dataset = self.dataset(
            py,
            &profile,
            declared,
            (false, false),
            exports::Numbers::AsStored,
        )?;
        let dict = PyDict::new(py);
        for (at, header) in dataset.headers.iter().enumerate() {
            let values: Vec<Py<PyAny>> = dataset
                .rows
                .iter()
                .map(|row| cell_to_python(py, &row[at]))
                .collect::<PyResult<_>>()?;
            dict.set_item(header, values)?;
        }
        Ok(dict)
    }

    /// The list as CSV, as the command line exports it.
    ///
    /// Args:
    ///     path: The file to write. Without it, the text is returned.
    ///     columns: The fields to include: a list, or comma-separated as ``-c``
    ///         writes them, each ``FIELD[:PRECISION][=LABEL]``.
    ///     profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
    ///         instead of ``columns``.
    ///     overwrite: Replace an existing file.
    ///
    /// Returns:
    ///     The text, when no ``path`` is given; otherwise ``None``.
    ///
    /// Raises:
    ///     ValueError: Neither ``columns`` nor ``profile`` is given, or both.
    ///     KeyError: An unknown field or profile; the message names the nearest.
    ///     FileExistsError: ``path`` exists and ``overwrite`` is false.
    ///
    /// Example:
    ///     >>> text = specimens.to_csv(columns="name,area")
    #[pyo3(signature = (path=None, columns=None, profile=None, overwrite=false))]
    fn to_csv(
        &self,
        py: Python<'_>,
        path: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Option<String>> {
        self.text(py, Format::Csv, path, columns, profile, overwrite)
    }

    /// The list as a document in a style: ``"latex"``, ``"markdown"``,
    /// ``"html"``, or one ``[style.NAME]`` declares, as ``--style NAME -o -``
    /// writes it.
    ///
    /// Args:
    ///     style: The style's name.
    ///     path: The file to write. Without it, the text is returned.
    ///     columns: The fields to include: a list, or comma-separated as ``-c``
    ///         writes them, each ``FIELD[:PRECISION][=LABEL]``.
    ///     profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
    ///         instead of ``columns``.
    ///     overwrite: Replace an existing file.
    ///
    /// Returns:
    ///     The text, when no ``path`` is given; otherwise ``None``.
    ///
    /// Raises:
    ///     ValueError: No style writing documents has that name, or neither
    ///         ``columns`` nor ``profile`` is given, or both.
    ///     KeyError: An unknown field or profile; the message names the nearest.
    ///     FileExistsError: ``path`` exists and ``overwrite`` is false.
    ///
    /// Example:
    ///     >>> table = specimens.to_text("latex", columns="name,area")
    #[pyo3(signature = (style, path=None, columns=None, profile=None, overwrite=false))]
    fn to_text(
        &self,
        py: Python<'_>,
        style: &str,
        path: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Option<String>> {
        // The project's styles, as the command line reads them: a list made
        // of samples carries none of its own, and the given style was written
        // over the project's `[style.latex]`.
        let config: Option<ProjectConfig> = match self.project_choice(py) {
            Ok(ProjectChoice::One(config)) => Some((*config).clone()),
            _ => None,
        };
        let chosen = config
            .as_ref()
            .and_then(|config| config.text_format(style).cloned())
            .or_else(|| crate::config::project_config::TextFormat::given(style))
            .ok_or_else(|| {
                let known: Vec<String> = config.as_ref().map_or_else(
                    || {
                        crate::config::project_config::TextFormat::GIVEN
                            .iter()
                            .map(|name| name.to_string())
                            .collect()
                    },
                    |config| {
                        config
                            .text_format_names()
                            .iter()
                            .map(|name| name.to_string())
                            .collect()
                    },
                );
                PyValueError::new_err(format!(
                    "'{style}' is no style writing documents: {} are, and [style.NAME] declares \
                     another",
                    known.join(", ")
                ))
            })?;
        self.text(
            py,
            Format::Text(Box::new(chosen)),
            path,
            columns,
            profile,
            overwrite,
        )
    }

    /// The list as TSV, as the command line exports it.
    ///
    /// Args:
    ///     path: The file to write. Without it, the text is returned.
    ///     columns: The fields to include: a list, or comma-separated as ``-c``
    ///         writes them, each ``FIELD[:PRECISION][=LABEL]``.
    ///     profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
    ///         instead of ``columns``.
    ///     overwrite: Replace an existing file.
    ///
    /// Returns:
    ///     The text, when no ``path`` is given; otherwise ``None``.
    ///
    /// Raises:
    ///     ValueError: Neither ``columns`` nor ``profile`` is given, or both.
    ///     KeyError: An unknown field or profile; the message names the nearest.
    ///     FileExistsError: ``path`` exists and ``overwrite`` is false.
    ///
    /// Example:
    ///     >>> text = specimens.to_tsv(columns="name,area")
    #[pyo3(signature = (path=None, columns=None, profile=None, overwrite=false))]
    fn to_tsv(
        &self,
        py: Python<'_>,
        path: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Option<String>> {
        self.text(py, Format::Tsv, path, columns, profile, overwrite)
    }

    /// The list as JSON, as the command line exports it.
    ///
    /// Args:
    ///     path: The file to write. Without it, the text is returned.
    ///     columns: The fields to include: a list, or comma-separated as ``-c``
    ///         writes them, each ``FIELD[:PRECISION][=LABEL]``.
    ///     profile: A profile's name in ``.samplekitrc``, or a ``Profile``,
    ///         instead of ``columns``.
    ///     overwrite: Replace an existing file.
    ///
    /// Returns:
    ///     The text, when no ``path`` is given; otherwise ``None``.
    ///
    /// Raises:
    ///     ValueError: Neither ``columns`` nor ``profile`` is given, or both.
    ///     KeyError: An unknown field or profile; the message names the nearest.
    ///     FileExistsError: ``path`` exists and ``overwrite`` is false.
    ///
    /// Example:
    ///     >>> text = specimens.to_json(columns="name,area")
    #[pyo3(signature = (path=None, columns=None, profile=None, overwrite=false))]
    fn to_json(
        &self,
        py: Python<'_>,
        path: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        profile: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Option<String>> {
        self.text(py, Format::Json, path, columns, profile, overwrite)
    }

    /// Write an export declared in ``.samplekitrc``, as ``samplekit export
    /// --write`` does.
    ///
    /// The export's own query, if it declares one, narrows the list first. An
    /// export of a figure draws it as ``sk.plot`` does, to its file.
    ///
    /// Args:
    ///     export: The export's name, or an ``Export``.
    ///     output: Write here instead of the declared file.
    ///     overwrite: Replace an existing file.
    ///
    /// Returns:
    ///     The path written, absolute.
    ///
    /// Raises:
    ///     KeyError: No export has that name, or its profile or figure is not
    ///         declared.
    ///     FileExistsError: The file exists and ``overwrite`` is false.
    ///     ValueError: ``output``'s extension does not match the export's format.
    ///
    /// Example:
    ///     >>> specimens.export("paper", output="paper/results.tex").name
    ///     'results.tex'
    #[pyo3(signature = (export, output=None, overwrite=false))]
    fn export(
        slf: &Bound<'_, Self>,
        py: Python<'_>,
        export: &Bound<'_, PyAny>,
        output: Option<&Bound<'_, PyAny>>,
        overwrite: bool,
    ) -> PyResult<Py<PyAny>> {
        let this = slf.borrow();
        let (declared, profile, root) = if let Ok(entry) = export.cast::<PyExport>() {
            let entry = entry.borrow();
            (
                entry.export.clone(),
                entry.profile.clone(),
                entry.root.clone(),
            )
        } else {
            let name: String = export.extract().map_err(|_| {
                PyTypeError::new_err("an export is a declared name or project.exports.<name>")
            })?;
            let config = this.single_config(py, "export", &name)?;
            let declared = config.export(&name).or_raise(py)?.clone();
            let profile = match &declared {
                Export::Table(target) => {
                    Some(config.profile(&target.profile).or_raise(py)?.clone())
                }
                Export::Figure(_) => None,
            };
            (declared, profile, config.root().to_path_buf())
        };
        let target = match declared {
            Export::Table(target) => target,
            // A figure's: drawn as `sk.plot` draws it, to the export's file.
            Export::Figure(figure) => {
                let samples = match &figure.query {
                    Some(query) => Py::new(
                        py,
                        this.query(py, &query.clone().into_pyobject(py)?.into_any())?,
                    )?
                    .into_any(),
                    None => slf.clone().into_any().unbind(),
                };
                let destination = match output.filter(|output| !output.is_none()) {
                    Some(output) => path_from(output)?,
                    None if figure.output.is_absolute() => figure.output.clone(),
                    None => root.join(&figure.output),
                };
                let arguments = pyo3::types::PyDict::new(py);
                arguments.set_item("output", python_path(py, &destination)?)?;
                arguments.set_item("overwrite", overwrite)?;
                if let Some(style) = &figure.style {
                    arguments.set_item("style", style)?;
                }
                drop(this);
                py.import("samplekit")?
                    .getattr("plot")?
                    .call((samples, &figure.figure), Some(&arguments))?;
                return python_path(py, &destination);
            }
        };
        let profile = profile.ok_or_else(|| {
            bridge::key_error(format!(
                "the export '{}' names the profile '{}', which is not declared",
                target.name, target.profile
            ))
        })?;
        // A list narrowed from the one it was taken from is said, as the
        // command line says a filter or a query narrowed a declared export:
        // the file names none of the samples it leaves out.
        let (held, taken) = (this.samples.borrow().len(), this.source.len());
        if held < taken {
            warn_value(
                py,
                &format!(
                    "'{}' is written from {held} of {taken} {}: a filter or a query narrowed it",
                    target.name,
                    if taken == 1 { "sample" } else { "samples" }
                ),
            )?;
        }
        // An export may carry its own selection: the query it declares narrows
        // this list before the profile shapes it.
        let dataset = match &target.query {
            Some(query) => {
                let narrowed = this.query(py, &query.clone().into_pyobject(py)?.into_any())?;
                narrowed.dataset(
                    py,
                    &profile,
                    true,
                    (target.filename, target.path),
                    exports::Numbers::AsWritten,
                )?
            }
            None => this.dataset(
                py,
                &profile,
                true,
                (target.filename, target.path),
                exports::Numbers::AsWritten,
            )?,
        };
        let dataset = noted_empty(py, &dataset, &target.format)?;
        // Written from values that are not current, the file is written and
        // said, as `samplekit export --write` says it: the table left for the
        // paper with nothing said.
        let listed = this.rust_list(py)?;
        let behind = exports::behind(&listed, &profile, None);
        if behind.any() {
            let said = behind.kinds();
            warn_value(
                py,
                &format!(
                    "'{}' is written from values that are not current: {} in {} {} — \
                     compute() brings them up to date",
                    target.name,
                    said.join(", "),
                    behind.samples,
                    if behind.samples == 1 {
                        "sample"
                    } else {
                        "samples"
                    }
                ),
            )?;
        }
        let text = export_formats::serialize(&dataset, target.format.clone()).or_raise(py)?;
        // `-` is standard output, as for `samplekit export -o -`.
        if output
            .filter(|output| !output.is_none())
            .and_then(|output| output.extract::<String>().ok())
            .as_deref()
            == Some("-")
        {
            py.import("sys")?
                .getattr("stdout")?
                .call_method1("write", (text,))?;
            return Ok(py.None());
        }
        let given = output.filter(|output| !output.is_none());
        let destination = match given {
            Some(output) => {
                let path = path_from(output)?;
                refuse_another_format(&path, &target.format, &target.name)?;
                path
            }
            None if target.output.is_absolute() => target.output.clone(),
            None => root.join(&target.output),
        };
        // The destination's folder, declared or given, is made by the write, as
        // `samplekit export` makes it.
        exports::write(&text, &destination, overwrite).or_raise(py)?;
        this.keep_output(
            py,
            &destination,
            &format!("{} · export {}", script_said(py), target.name),
        )?;
        python_path(py, &destination)
    }
}

// ------------------------------------------------------------------ Names

/// The entries a project declares, read-only, by name.
///
/// An entry is read as an attribute or an item — ``project.queries.ductile`` or
/// ``project.queries["ductile"]`` — and iterating gives the names. An unknown
/// name raises ``AttributeError`` or ``KeyError`` naming the nearest.
#[pyclass(name = "Names", module = "samplekit", unsendable)]
pub struct PyNames {
    kind: &'static str,
    entries: IndexMap<String, Py<PyAny>>,
    /// Where these names are declared, for a message about one that is not.
    origin: String,
}

impl PyNames {
    fn missing(&self, name: &str) -> String {
        let available: Vec<String> = self.entries.keys().cloned().collect();
        if self.origin.starts_with("no ") {
            return format!("no {} '{name}': {}", self.kind, self.origin);
        }
        let mut message = format!("no {} '{name}' {}", self.kind, self.origin);
        if let Some(nearest) = identifier::nearest(name, available.iter().map(String::as_str)) {
            message.push_str(&format!("\n  did you mean: {nearest}?"));
        }
        if !available.is_empty() {
            message.push_str(&format!("\n  available: {}", available.join(", ")));
        }
        message
    }
}

#[pymethods]
impl PyNames {
    fn __getattr__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        // The message carries the nearest name already: Python adding its
        // own wrote the suggestion twice.
        (|| {
            if name.starts_with('_') {
                return Err(PyAttributeError::new_err(format!(
                    "'Names' object has no attribute '{name}'"
                )));
            }
            self.entries
                .get(name)
                .map(|entry| entry.clone_ref(py))
                .ok_or_else(|| PyAttributeError::new_err(self.missing(name)))
        })()
        .map_err(bridge::said_in_full)
    }

    fn __getitem__(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        self.entries
            .get(name)
            .map(|entry| entry.clone_ref(py))
            .ok_or_else(|| bridge::key_error(self.missing(name)))
    }

    fn __setattr__(&self, name: &str, _value: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyAttributeError::new_err(format!(
            "'{name}' cannot be assigned: what a project declares is changed in .samplekitrc"
        )))
    }

    fn __delattr__(&self, name: &str) -> PyResult<()> {
        Err(PyAttributeError::new_err(format!(
            "'{name}' cannot be deleted: what a project declares is changed in .samplekitrc"
        )))
    }

    fn __contains__(&self, name: &Bound<'_, PyAny>) -> bool {
        name.extract::<String>()
            .is_ok_and(|name| self.entries.contains_key(&name))
    }

    fn __iter__<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyIterator>> {
        PyList::new(py, self.entries.keys())?.try_iter()
    }

    fn __len__(&self) -> usize {
        self.entries.len()
    }

    fn __dir__(slf: &Bound<'_, Self>) -> PyResult<Vec<String>> {
        let py = slf.py();
        let mut names: Vec<String> = builtin(py, "object")?
            .getattr("__dir__")?
            .call1((slf,))?
            .extract()?;
        names.extend(slf.borrow().entries.keys().cloned());
        Ok(names)
    }

    fn _ipython_key_completions_(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    fn __repr__(&self) -> String {
        let names: Vec<&str> = self.entries.keys().map(String::as_str).collect();
        format!("Names({})", names.join(", "))
    }
}

// ---------------------------------------------------------------- Project

/// A project: what its ``.samplekitrc`` declares, read-only.
///
/// Read from a sample or a list — ``steel.project``, ``specimens.project``. Outside
/// any project every collection of entries is empty.
///
/// Example:
///     >>> list(specimens.project.queries)
///     ['ductile']
#[pyclass(name = "Project", module = "samplekit", unsendable)]
pub struct PyProject {
    config: Option<Rc<ProjectConfig>>,
}

impl PyProject {
    fn names(&self, kind: &'static str, entries: IndexMap<String, Py<PyAny>>) -> PyNames {
        let origin = match &self.config {
            Some(config) => format!(
                "is declared in {}",
                config.root().join(project::FILENAME).display()
            ),
            None => {
                "no .samplekitrc was found above these samples, so nothing is declared".to_string()
            }
        };
        PyNames {
            kind,
            entries,
            origin,
        }
    }
}

#[pymethods]
impl PyProject {
    /// The folder holding ``.samplekitrc``, or ``None`` outside a project.
    #[getter]
    fn root(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.config
            .as_ref()
            .map(|config| python_path(py, config.root()))
            .transpose()
    }

    /// The saved queries, ``[query.*]``, by name, each a ``Query``.
    #[getter]
    fn queries(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for name in config.query_names() {
                let declaration = config.query(name).or_raise(py)?.clone();
                entries.insert(
                    name.to_string(),
                    Py::new(py, PyQuery { declaration })?.into_any(),
                );
            }
        }
        Ok(self.names("query", entries))
    }

    /// The profiles, ``[profile.*]``, by name, each a ``Profile``.
    #[getter]
    fn profiles(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for name in config.profile_names() {
                let declaration = config.profile(name).or_raise(py)?.clone();
                entries.insert(
                    name.to_string(),
                    Py::new(py, PyProfile { declaration })?.into_any(),
                );
            }
        }
        Ok(self.names("profile", entries))
    }

    /// The exports, ``[export.*]``, by name, each an ``Export``.
    #[getter]
    fn exports(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for name in config.export_names() {
                let export = config.export(name).or_raise(py)?.clone();
                let profile = match &export {
                    Export::Table(target) => config.profile(&target.profile).ok().cloned(),
                    Export::Figure(_) => None,
                };
                let entry = PyExport {
                    export,
                    profile,
                    root: config.root().to_path_buf(),
                };
                entries.insert(name.to_string(), Py::new(py, entry)?.into_any());
            }
        }
        Ok(self.names("export", entries))
    }

    /// A value of `field` as the project writes it: at its declared precision,
    /// without its unit — a group's value in a figure's legend, `80` and not
    /// `80.0`. A value of no declared precision is written in full.
    fn _value_text(
        &self,
        py: Python<'_>,
        field: &str,
        value: &Bound<'_, PyAny>,
    ) -> PyResult<String> {
        let value = bridge::one_value(value)?;
        let Some(config) = &self.config else {
            return Ok(formatting::format_value(
                &value,
                &Resolved::plain(&Presentation::default()),
            ));
        };
        let quantity = match fields::parse(field) {
            Ok(fields::Field::Cell { table, column, .. }) => format!("{table}.{column}"),
            Ok(fields::Field::Named { name, .. }) => name.to_string(),
            _ => field.to_string(),
        };
        let _ = py;
        let resolved = config
            .resolved_as(&Presentation::default(), &quantity, Some("plain"))
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(formatting::format_value(&value, &resolved))
    }

    /// The unit and the symbol an axis showing `field` is labelled with, in
    /// `style` — `plain` when not given, never `[render] style` — from what
    /// the file wrote and what `[property.*]` declares.
    #[pyo3(signature = (field, unit=None, symbol=None, style=None))]
    fn _axis_label(
        &self,
        field: &str,
        unit: Option<String>,
        symbol: Option<String>,
        style: Option<String>,
    ) -> PyResult<(Option<String>, Option<String>)> {
        let style = style.unwrap_or_else(|| "plain".to_string());
        let Some(config) = &self.config else {
            if style != "plain" {
                return Err(PyValueError::new_err(format!(
                    "no style '{style}' is declared: no .samplekitrc was found"
                )));
            }
            return Ok((unit, symbol));
        };
        // The `[property.*]` key: a cell's quantity is its column.
        let (quantity, channel) = match fields::parse(field) {
            Ok(fields::Field::Cell {
                table,
                column,
                channel,
                ..
            }) => (format!("{table}.{column}"), channel),
            Ok(fields::Field::Named { name, channel }) => (name.to_string(), channel),
            Ok(fields::Field::ListItem { name, .. }) => (name.to_string(), Channel::Value),
            _ => (field.to_string(), Channel::Value),
        };
        let presentation = Presentation {
            unit,
            symbol,
            precision: None,
        };
        let resolved = config
            .resolved_as(&presentation, &quantity, Some(&style))
            .map_err(|error| PyValueError::new_err(error.to_string()))?;
        Ok(channel_label(
            field,
            &quantity,
            channel,
            resolved.unit,
            resolved.symbol,
        ))
    }

    /// The matplotlib settings every figure is drawn with, ``[matplotlib]`` —
    /// a dict by matplotlib's own dotted names.
    #[getter]
    fn matplotlib(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let settings = PyDict::new(py);
        if let Some(config) = &self.config {
            for (name, value) in config.matplotlib() {
                settings.set_item(name, toml_to_python(py, value)?)?;
            }
        }
        Ok(settings.into_any().unbind())
    }

    /// The figures, ``[figure.*]``, by name, each a ``FigureDeclaration``.
    #[getter]
    fn figures(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for name in config.figure_names() {
                let declaration = config
                    .figure(name)
                    .expect("a name figure_names gives is declared")
                    .clone();
                entries.insert(
                    name.to_string(),
                    Py::new(py, PyFigureDeclaration { declaration })?.into_any(),
                );
            }
        }
        Ok(self.names("figure", entries))
    }

    /// The declared properties, ``[property.*]``, by name, each a
    /// ``PropertyDeclaration``.
    #[getter]
    fn properties(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for name in config.property_names() {
                let declaration = config
                    .property(name)
                    .expect("a listed quantity is declared")
                    .clone();
                entries.insert(
                    name.to_string(),
                    Py::new(py, PyPropertyDeclaration { declaration })?.into_any(),
                );
            }
        }
        Ok(self.names("property", entries))
    }

    /// The declared units, ``[unit.*]``, by spelling, each a ``Unit``.
    #[getter]
    fn units(&self, py: Python<'_>) -> PyResult<PyNames> {
        let mut entries = IndexMap::new();
        if let Some(config) = &self.config {
            for spelling in config.unit_vocabulary() {
                let variants = config
                    .unit(spelling)
                    .expect("a listed unit is declared")
                    .clone();
                let entry = PyUnit {
                    spelling: spelling.to_string(),
                    variants,
                };
                entries.insert(spelling.to_string(), Py::new(py, entry)?.into_any());
            }
        }
        Ok(self.names("unit", entries))
    }

    /// How tables and figures are rendered, ``[render]``, as a ``Render``.
    #[getter]
    fn render(&self) -> PyRender {
        PyRender {
            settings: self
                .config
                .as_ref()
                .map(|config| config.render().clone())
                .unwrap_or_default(),
        }
    }

    /// The model, ``[model]``, or ``None`` when the project declares none.
    #[getter]
    fn model(&self) -> Option<PyModel> {
        let config = self.config.as_ref()?;
        let declared = config.model()?;
        Some(PyModel {
            path: config.resolve(&declared.path.to_string_lossy()),
            class_name: declared.class.clone(),
        })
    }

    /// Which files are samples, ``[collection]``, as a ``Collection``.
    #[getter]
    fn collection(&self) -> PyCollection {
        let rules = self
            .config
            .as_ref()
            .map(|config| config.collection().clone())
            .unwrap_or_default();
        PyCollection {
            recursive: rules.recursive,
            include: rules.include,
            exclude: rules.exclude,
            files: rules.files,
        }
    }

    fn __setattr__(&self, name: &str, _value: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(PyAttributeError::new_err(format!(
            "'{name}' cannot be assigned: the project is read-only, and what it declares is \
             changed in .samplekitrc"
        )))
    }

    fn __repr__(&self) -> String {
        match &self.config {
            Some(config) => format!("Project({})", config.root().display()),
            None => "Project(no .samplekitrc)".to_string(),
        }
    }
}

// ------------------------------------------------------ declaration views

/// A saved query, ``[query.*]``: a filter under a name.
#[pyclass(name = "Query", module = "samplekit", unsendable, frozen)]
pub struct PyQuery {
    declaration: NamedQuery,
}

#[pymethods]
impl PyQuery {
    /// The query's name.
    #[getter]
    fn name(&self) -> String {
        self.declaration.name.clone()
    }

    /// The filter expression, as ``-f`` takes it.
    #[getter]
    fn filter(&self) -> String {
        self.declaration.filter.clone()
    }

    /// The folder the query searches by default, or ``None`` for the project's.
    #[getter]
    fn directory(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.declaration
            .directory
            .as_ref()
            .map(|directory| python_path(py, directory))
            .transpose()
    }

    fn __repr__(&self) -> String {
        format!(
            "Query('{}', filter={:?})",
            self.declaration.name, self.declaration.filter
        )
    }
}

/// One column of a profile: its field, its headings and its precision.
#[pyclass(name = "ColumnSpec", module = "samplekit", unsendable, frozen)]
pub struct PyColumnSpec {
    declaration: ColumnSpec,
}

#[pymethods]
impl PyColumnSpec {
    /// The field the column shows.
    #[getter]
    fn field(&self) -> String {
        self.declaration.field.clone()
    }

    /// The heading of the column in a terminal table, or ``None`` for the field.
    #[getter]
    fn label(&self) -> Option<String> {
        self.declaration.label.clone()
    }

    /// The heading of the column in an exported file, or ``None``.
    #[getter]
    fn header(&self) -> Option<String> {
        self.declaration.header.clone()
    }

    /// The column's precision — ``".3f"``, or a pair for the value and the
    /// uncertainty — or ``None``.
    #[getter]
    fn precision(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.declaration
            .precision
            .as_ref()
            .map(|precision| schema_precision_to_python(py, precision))
            .transpose()
    }

    /// The text each cell is written as, such as ``"{value:.3f}"``, or ``None``.
    #[getter]
    fn template(&self) -> Option<String> {
        self.declaration.template.clone()
    }

    fn __repr__(&self) -> String {
        format!("ColumnSpec('{}')", self.declaration.field)
    }
}

fn column_specs(py: Python<'_>, columns: &[ColumnSpec]) -> PyResult<Py<PyAny>> {
    let specs: Vec<Py<PyColumnSpec>> = columns
        .iter()
        .map(|column| {
            Py::new(
                py,
                PyColumnSpec {
                    declaration: column.clone(),
                },
            )
        })
        .collect::<PyResult<_>>()?;
    Ok(PyTuple::new(py, specs)?.into_any().unbind())
}

/// A profile, ``[profile.*]``: the columns of a table, and the order of its
/// rows.
#[pyclass(name = "Profile", module = "samplekit", unsendable, frozen)]
pub struct PyProfile {
    declaration: Profile,
}

#[pymethods]
impl PyProfile {
    /// The profile's name.
    #[getter]
    fn name(&self) -> String {
        self.declaration.name.clone()
    }

    /// The columns, each a ``ColumnSpec``, in order.
    #[getter]
    fn columns(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        column_specs(py, &self.declaration.columns)
    }

    /// The sort keys, in order of precedence; a leading ``-`` sorts descending.
    #[getter]
    fn sort<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.declaration.sort)
    }

    fn __repr__(&self) -> String {
        format!("Profile('{}')", self.declaration.name)
    }
}

/// An export, ``[export.*]``: a profile written to a file in a format, or
/// a figure drawn to a file.
#[pyclass(name = "Export", module = "samplekit", unsendable, frozen)]
pub struct PyExport {
    export: Export,
    profile: Option<Profile>,
    root: PathBuf,
}

#[pymethods]
impl PyExport {
    /// The export's name.
    #[getter]
    fn name(&self) -> String {
        self.export.name().to_string()
    }

    /// The name of the profile it writes, or ``None`` for a figure's export.
    #[getter]
    fn profile(&self) -> Option<String> {
        match &self.export {
            Export::Table(target) => Some(target.profile.clone()),
            Export::Figure(_) => None,
        }
    }

    /// The name of the figure it draws, or ``None`` for a table's export.
    #[getter]
    fn figure(&self) -> Option<String> {
        match &self.export {
            Export::Table(_) => None,
            Export::Figure(figure) => Some(figure.figure.clone()),
        }
    }

    /// The data format — ``"csv"``, ``"tsv"``, ``"json"`` — or ``None`` for
    /// a document its style writes, or a figure.
    #[getter]
    fn format(&self) -> Option<String> {
        match &self.export {
            Export::Table(target) => match &target.format {
                Format::Csv => Some("csv".to_string()),
                Format::Tsv => Some("tsv".to_string()),
                Format::Json => Some("json".to_string()),
                Format::Text(_) => None,
            },
            Export::Figure(_) => None,
        }
    }

    /// The file it writes.
    #[getter]
    fn output(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_path(py, self.export.output())
    }

    /// Whether the export adds a column with each sample's file name.
    #[getter]
    fn filename(&self) -> bool {
        matches!(&self.export, Export::Table(target) if target.filename)
    }

    /// Whether the export adds a column with each sample's path.
    #[getter]
    fn path(&self) -> bool {
        matches!(&self.export, Export::Table(target) if target.path)
    }

    /// The saved query that selects its samples, or ``None``.
    #[getter]
    fn query(&self) -> Option<String> {
        self.export.query().map(str::to_string)
    }

    fn __repr__(&self) -> String {
        format!("Export('{}')", self.export.name())
    }
}

/// A `[matplotlib]` value as Python reads it.
fn toml_to_python(py: Python<'_>, value: &toml::Value) -> PyResult<Py<PyAny>> {
    Ok(match value {
        toml::Value::String(text) => text.into_py_any(py)?,
        toml::Value::Integer(integer) => integer.into_py_any(py)?,
        toml::Value::Float(number) => number.into_py_any(py)?,
        toml::Value::Boolean(flag) => flag.into_py_any(py)?,
        toml::Value::Array(items) => {
            let items = items
                .iter()
                .map(|item| toml_to_python(py, item))
                .collect::<PyResult<Vec<_>>>()?;
            PyList::new(py, items)?.into_any().unbind()
        }
        // Refused as the file loads, or flattened into dotted names.
        toml::Value::Datetime(_) | toml::Value::Table(_) => py.None(),
    })
}

/// A figure declared in ``.samplekitrc``, ``[figure.*]``: what it draws, and
/// how its axes are set.
#[pyclass(name = "FigureDeclaration", module = "samplekit", unsendable, frozen)]
pub struct PyFigureDeclaration {
    declaration: FigureDeclaration,
}

#[pymethods]
impl PyFigureDeclaration {
    /// The figure's name.
    #[getter]
    fn name(&self) -> String {
        self.declaration.name.clone()
    }

    /// ``"scatter"``, ``"line"``, ``"step"``, ``"bar"`` or ``"box"``.
    #[getter]
    fn kind(&self) -> &'static str {
        self.declaration.kind.as_str()
    }

    /// The field on the x axis.
    #[getter]
    fn x(&self) -> String {
        self.declaration.x.clone()
    }

    /// The field on the y axis.
    #[getter]
    fn y(&self) -> String {
        self.declaration.y.clone()
    }

    /// The field, or comma-separated fields, whose values split the samples into
    /// series; ``None`` for one series.
    #[getter]
    fn group(&self) -> Option<String> {
        self.declaration.group.clone()
    }

    /// The saved query that selects its samples, or ``None``.
    #[getter]
    fn query(&self) -> Option<String> {
        self.declaration.query.clone()
    }

    /// The title, or ``None``.
    #[getter]
    fn title(&self) -> Option<String> {
        self.declaration.title.clone()
    }

    /// The x axis label, or ``None`` for the field's symbol and unit.
    #[getter]
    fn x_label(&self) -> Option<String> {
        self.declaration.x_label.clone()
    }

    /// The y axis label, or ``None`` for the field's symbol and unit.
    #[getter]
    fn y_label(&self) -> Option<String> {
        self.declaration.y_label.clone()
    }

    /// The style whose symbols and units label the axes, or ``None``.
    #[getter]
    fn style(&self) -> Option<String> {
        self.declaration.style.clone()
    }

    /// The x axis bounds, ``None`` for a free one; ``None`` when not declared.
    #[getter]
    fn x_limits(&self) -> Option<(Option<f64>, Option<f64>)> {
        self.declaration.x_limits
    }

    /// The y axis bounds, ``None`` for a free one; ``None`` when not declared.
    #[getter]
    fn y_limits(&self) -> Option<(Option<f64>, Option<f64>)> {
        self.declaration.y_limits
    }

    /// ``"linear"``, ``"log"`` or ``"symlog"``, or ``None``.
    #[getter]
    fn x_scale(&self) -> Option<String> {
        self.declaration.x_scale.clone()
    }

    /// ``"linear"``, ``"log"`` or ``"symlog"``, or ``None``.
    #[getter]
    fn y_scale(&self) -> Option<String> {
        self.declaration.y_scale.clone()
    }

    /// ``"equal"`` or ``"auto"``, or ``None``.
    #[getter]
    fn aspect(&self) -> Option<String> {
        self.declaration.aspect.clone()
    }

    /// Where the legend goes — ``"best"``, ``"outside"``, ``"none"`` or one of
    /// matplotlib's places — or ``None``.
    #[getter]
    fn legend(&self) -> Option<String> {
        self.declaration.legend.clone()
    }

    /// The figure's width and height in centimetres, or ``None``.
    #[getter]
    fn figsize(&self) -> Option<(f64, f64)> {
        self.declaration.figsize
    }

    /// Its own matplotlib settings, over the project's ``[matplotlib]``, by
    /// matplotlib's dotted names.
    #[getter]
    fn matplotlib(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let settings = PyDict::new(py);
        for (name, value) in &self.declaration.matplotlib {
            settings.set_item(name, toml_to_python(py, value)?)?;
        }
        Ok(settings.into_any().unbind())
    }

    fn __repr__(&self) -> String {
        format!("FigureDeclaration('{}')", self.declaration.name)
    }
}

/// A property declared in ``.samplekitrc``, ``[property.*]``: its unit, symbol
/// and precision, for every sample of the project.
#[pyclass(name = "PropertyDeclaration", module = "samplekit", unsendable, frozen)]
pub struct PyPropertyDeclaration {
    declaration: PropertyDeclaration,
}

#[pymethods]
impl PyPropertyDeclaration {
    /// The unit, or ``None``.
    #[getter]
    fn unit(&self) -> Option<String> {
        self.declaration.unit.clone()
    }

    /// The symbol, or ``None``.
    #[getter]
    fn symbol(&self) -> Option<String> {
        self.declaration.symbol.clone()
    }

    /// The symbol in each style, by style name:
    /// ``{"figure": "$\\sigma$"}``.
    #[getter]
    fn symbol_variants(&self) -> HashMap<String, String> {
        // By the style's name, as `Unit.variants` keys them: the file's key,
        // `symbol_math`, was given.
        self.declaration
            .symbol_variants
            .iter()
            .map(|(key, value)| {
                (
                    key.strip_prefix("symbol_").unwrap_or(key).to_string(),
                    value.clone(),
                )
            })
            .collect()
    }

    /// The precision — ``".3f"``, or a pair for the value and the uncertainty —
    /// or ``None``.
    #[getter]
    fn precision(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.declaration
            .precision
            .as_ref()
            .map(|precision| schema_precision_to_python(py, precision))
            .transpose()
    }

    fn __repr__(&self) -> String {
        format!("PropertyDeclaration(unit={:?})", self.declaration.unit)
    }
}

/// A unit declared in ``.samplekitrc``, ``[unit.*]``: how it is written in
/// each style.
#[pyclass(name = "Unit", module = "samplekit", unsendable, frozen)]
pub struct PyUnit {
    spelling: String,
    variants: IndexMap<String, String>,
}

#[pymethods]
impl PyUnit {
    /// The unit as the files write it — ``"degC"``.
    #[getter]
    fn spelling(&self) -> String {
        self.spelling.clone()
    }

    /// The unit in each style, by style name — ``{"plain": "°C"}``.
    #[getter]
    fn variants(&self) -> HashMap<String, String> {
        self.variants
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn __repr__(&self) -> String {
        format!("Unit('{}')", self.spelling)
    }
}

/// How the project renders tables and figures, ``[render]``.
#[pyclass(name = "Render", module = "samplekit", unsendable, frozen)]
pub struct PyRender {
    settings: RenderSettings,
}

#[pymethods]
impl PyRender {
    /// The style tables are rendered in, or ``None``.
    #[getter]
    fn style(&self) -> Option<String> {
        self.settings.style.clone()
    }

    /// The precision of every number without its own, or ``None``.
    #[getter]
    fn precision(&self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        self.settings
            .precision
            .as_ref()
            .map(|precision| schema_precision_to_python(py, precision))
            .transpose()
    }

    /// How a table is drawn — ``"plain"`` or ``"boxed"`` — or ``None``.
    #[getter]
    fn table(&self) -> Option<String> {
        self.settings.table.clone()
    }

    /// The style figures label their axes in when they name none, or ``None``.
    #[getter]
    fn figure_style(&self) -> Option<String> {
        self.settings.figure_style.clone()
    }

    fn __repr__(&self) -> String {
        format!("Render(style={:?})", self.settings.style)
    }
}

/// The project's model, ``[model]``: the Python file and class its samples are
/// read with.
#[pyclass(name = "Model", module = "samplekit", unsendable, frozen)]
pub struct PyModel {
    path: PathBuf,
    class_name: Option<String>,
}

#[pymethods]
impl PyModel {
    /// The model's Python file.
    #[getter]
    fn path(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        python_path(py, &self.path)
    }

    /// The class in that file, or ``None`` when the file declares only one.
    #[getter]
    fn class_name(&self) -> Option<String> {
        self.class_name.clone()
    }

    fn __repr__(&self) -> String {
        format!("Model({})", self.path.display())
    }
}

/// Which files of the project are samples, ``[collection]``.
#[pyclass(name = "Collection", module = "samplekit", unsendable, frozen)]
pub struct PyCollection {
    recursive: bool,
    include: Vec<String>,
    exclude: Vec<String>,
    files: Vec<String>,
}

#[pymethods]
impl PyCollection {
    /// Whether subfolders are searched.
    #[getter]
    fn recursive(&self) -> bool {
        self.recursive
    }

    /// The globs a sample's file must match.
    #[getter]
    fn include<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.include)
    }

    /// The globs of files that are not samples.
    #[getter]
    fn exclude<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.exclude)
    }

    /// Where a sample's own files are looked for, ``[collection] files`` —
    /// the folders searched by the sample's name, and the patterns naming it,
    /// as written.
    #[getter]
    fn files<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyTuple>> {
        PyTuple::new(py, &self.files)
    }

    fn __repr__(&self) -> String {
        format!("Collection(include={:?})", self.include)
    }
}

// ---------------------------------------------------------- Field, Summary

/// A field of a list, as ``SampleList.fields`` gives it, and as ``-c`` and
/// ``-s`` take it.
///
/// ``str(field)`` is its path, and a field is accepted wherever a field's name
/// is: ``specimens.sorted(specimens.fields.area)``.
#[pyclass(name = "Field", module = "samplekit", unsendable, frozen)]
pub struct PyField {
    path: String,
}

#[pymethods]
impl PyField {
    /// The field's path — ``"area"``, ``"curve.force[0.2]"``.
    #[getter]
    fn path(&self) -> String {
        self.path.clone()
    }

    fn __str__(&self) -> String {
        self.path.clone()
    }

    fn __repr__(&self) -> String {
        format!("Field('{}')", self.path)
    }
}

/// The statistics of a set of values: readings, or a field over a list.
///
/// Given by ``Property.stats`` and ``SampleList.stats``.
#[pyclass(name = "Summary", module = "samplekit", unsendable, frozen)]
pub struct PySummary {
    summary: Summary,
    /// The mean weighted by 1/u², where every value summarised has an
    /// uncertainty above zero.
    weighted_mean: Option<f64>,
    /// Its standard error, 1/√Σw.
    weighted_error: Option<f64>,
    /// The samples summarised over, and those saying *not applicable*,
    /// counted apart as `--summary` counts them; `None` for readings.
    considered: Option<usize>,
    not_applicable: usize,
    /// The row `--summary` writes of it, with its label, its unit and its
    /// precision: `None` for a property's own readings.
    row: Option<crate::presentation::summaries::SummaryRow>,
}

#[pymethods]
impl PySummary {
    /// How many values were summarised.
    #[getter]
    fn count(&self) -> usize {
        self.summary.count.get()
    }

    /// How many samples the value applies to, those holding no number
    /// included and those saying *not applicable* left out: the denominator of
    /// ``count``, as ``--summary`` writes ``3/4``. ``None`` for readings.
    #[getter]
    fn considered(&self) -> Option<usize> {
        self.considered
    }

    /// How many samples say the value is not applicable, counted apart —
    /// neither summarised nor missing.
    #[getter]
    fn not_applicable(&self) -> usize {
        self.not_applicable
    }

    /// The smallest value.
    #[getter]
    fn minimum(&self) -> f64 {
        self.summary.minimum
    }

    /// The largest value.
    #[getter]
    fn maximum(&self) -> f64 {
        self.summary.maximum
    }

    /// The mean — weighted by 1/u² where every value has an uncertainty above zero,
    /// the arithmetic mean otherwise. ``weighted`` says which.
    #[getter]
    fn mean(&self) -> f64 {
        self.weighted_mean.unwrap_or(self.summary.mean)
    }

    /// Whether ``mean`` is weighted by 1/u², as the command line's summary heads
    /// it ``mean (1/u²)``.
    #[getter]
    fn weighted(&self) -> bool {
        self.weighted_mean.is_some()
    }

    /// The median.
    #[getter]
    fn median(&self) -> f64 {
        self.summary.median
    }

    /// The first quartile.
    #[getter]
    fn first_quartile(&self) -> f64 {
        self.summary.first_quartile
    }

    /// The third quartile.
    #[getter]
    fn third_quartile(&self) -> f64 {
        self.summary.third_quartile
    }

    /// The sample standard deviation, dividing by n − 1; ``None`` for one value.
    #[getter]
    fn sample_stdev(&self) -> Option<f64> {
        self.summary.sample_stdev
    }

    /// The population standard deviation, dividing by n.
    #[getter]
    fn population_stdev(&self) -> f64 {
        self.summary.population_stdev
    }

    /// The standard error of the mean — 1/√Σw beside a weighted mean, the
    /// sample standard deviation over √n beside an arithmetic one; ``None``
    /// for one value unweighted.
    #[getter]
    fn standard_error(&self) -> Option<f64> {
        self.weighted_error.or(self.summary.standard_error)
    }

    /// The summary as ``--summary`` writes a row of it: the count, the
    /// mean, the deviation ``s``, its standard error ``sem``, the median and
    /// the extremes.
    ///
    /// Args:
    ///     style: Accepted for every ``to_text``; a summary's numbers are
    ///         written alike in every style.
    ///
    /// Returns:
    ///     The text.
    #[pyo3(signature = (style=None))]
    fn to_text(&self, style: Option<&str>) -> String {
        let _ = style;
        // A list's field: the row `--summary` writes, labelled, at the
        // project's precision. It wrote `5.672 … 4.33125` beside the command
        // line's `5.7 (1/u²) … 4.3`.
        if let Some(row) = &self.row {
            let mean = crate::presentation::summaries::mean_header([row]);
            let headers = ["", "n", mean.as_str(), "s", "sem", "median", "min", "max"]
                .iter()
                .map(|name| name.to_string())
                .collect();
            let label = match &row.unit {
                Some(unit) => format!("{} [{unit}]", row.label),
                None => row.label.clone(),
            };
            let mut cells = vec![label];
            cells.extend(crate::presentation::summaries::written(row, false));
            return terminal_rendering::table(
                headers,
                vec![cells],
                terminal_rendering::Target::Pipe,
                terminal_rendering::Style::Plain,
            )
            .trim_end()
            .to_string();
        }
        let considered = self.considered.unwrap_or(self.summary.count.get());
        let row = crate::presentation::summaries::SummaryRow {
            label: String::new(),
            unit: None,
            summary: Some(crate::collection::sample_list::ColumnSummary {
                summary: self.summary.clone(),
                considered,
                not_applicable: self.not_applicable,
                weighted_mean: self.weighted_mean,
                weighted_error: self.weighted_error,
            }),
            precision: None,
            places: None,
            of: considered + self.not_applicable,
            not_applicable: self.not_applicable,
        };
        let mean = crate::presentation::summaries::mean_header([&row]);
        let headers = ["n", mean.as_str(), "s", "sem", "median", "min", "max"]
            .iter()
            .map(|name| name.to_string())
            .collect();
        let cells = vec![crate::presentation::summaries::written(&row, false)];
        terminal_rendering::table(
            headers,
            cells,
            terminal_rendering::Target::Pipe,
            terminal_rendering::Style::Plain,
        )
        .trim_end()
        .to_string()
    }

    fn __repr__(&self) -> String {
        let of = self
            .considered
            .map(|considered| format!("/{considered}"))
            .unwrap_or_default();
        let not_applicable = if self.not_applicable > 0 {
            format!(", not_applicable={}", self.not_applicable)
        } else {
            String::new()
        };
        format!(
            "Summary(count={}{of}{not_applicable}, mean={}{}, median={})",
            self.count(),
            self.mean(),
            if self.weighted() { " (1/u²)" } else { "" },
            self.summary.median
        )
    }
}

// ----------------------------------------------------------------- module

/// The package's `Sample` class and `BoundList` factory.
#[pyfunction(name = "_register")]
fn register_package(sample_class: Py<PyAny>, bound_list: Py<PyAny>) -> PyResult<()> {
    bridge::register(sample_class, bound_list)
}

/// The class `Sample(path, model=...)` constructs instead of `Sample` itself,
/// or `None` for `Sample`.
#[pyfunction(name = "_model_for")]
fn model_for(
    py: Python<'_>,
    path: Option<&Bound<'_, PyAny>>,
    model: Option<&Bound<'_, PyAny>>,
) -> PyResult<Option<Py<PyAny>>> {
    let explicit = model.filter(|model| !model.is_none());
    if explicit.is_some() {
        return model_class(py, explicit, None);
    }
    let Some(path) = path.filter(|path| !path.is_none()) else {
        return Ok(None);
    };
    let path = path_from(path)?;
    let config = config_for(py, &path)?;
    model_class(py, None, config.as_deref())
}

/// The sample saved at `path` as its project's model reads it, for what the
/// model owes it: `None` where no model is declared, and why where the model
/// cannot be read.
fn model_reader<'py>(
    py: Python<'py>,
    path: Option<&Path>,
) -> Result<Option<Bound<'py, PyAny>>, String> {
    // The first line of what Python raised: the reason, not its traceback.
    let said = |error: PyErr| {
        error
            .value(py)
            .to_string()
            .lines()
            .next()
            .unwrap_or_default()
            .to_string()
    };
    let Some(path) = path else {
        return Ok(None);
    };
    let Some(config) = config_for(py, path)
        .map_err(said)?
        .filter(|config| config.model().is_some())
    else {
        return Ok(None);
    };
    let Some(class) = bridge::load_model(py, &config).map_err(said)? else {
        return Ok(None);
    };
    if !path.is_file() {
        return Err(format!("{} is not saved", path.display()));
    }
    class
        .bind(py)
        .call1((path.to_path_buf(),))
        .map(Some)
        .map_err(said)
}

/// Refuses a path `Sample.new` cannot create a sample at, before any model is
/// imported.
#[pyfunction(name = "_check_new")]
fn check_new(path: &Bound<'_, PyAny>) -> PyResult<()> {
    let path = path_from(path)?;
    if path.exists() {
        return Err(PyFileExistsError::new_err(format!(
            "{} already exists: Sample(path) reads it",
            path.display()
        )));
    }
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !directory.is_dir() {
        return Err(PyFileNotFoundError::new_err(format!(
            "the directory {} does not exist, so {} cannot be created there",
            directory.display(),
            path.display()
        )));
    }
    // A file the collection's patterns do not take is no sample any command
    // reads: `samples/noext` was written, and `sk.load` never saw it.
    let config = crate::config::project_config::load_for(directory)
        .ok()
        .flatten();
    let rules = config
        .as_ref()
        .map(|config| config.collection().clone())
        .unwrap_or_default();
    let root = config.as_ref().map_or_else(
        || directory.to_path_buf(),
        |config| config.root().to_path_buf(),
    );
    let absolute = std::path::absolute(&path).unwrap_or_else(|_| path.clone());
    let root = std::path::absolute(&root).unwrap_or(root);
    if !crate::config::discovery::matches(&absolute, &root, &rules) {
        let suggested = if path.extension().is_none() {
            format!(" — {}.md is one", path.display())
        } else {
            String::new()
        };
        return Err(PyValueError::new_err(format!(
            "{} is no file the collection reads as a sample: [collection] include takes {}, \
             and exclude leaves out {}{suggested}",
            path.display(),
            rules.include.join(", "),
            if rules.exclude.is_empty() {
                "nothing".to_string()
            } else {
                rules.exclude.join(", ")
            }
        )));
    }
    Ok(())
}

/// The class a model file declares, for the command line's worker.
#[pyfunction(name = "_model_at")]
#[pyo3(signature = (path, class_name, root))]
fn model_at(
    py: Python<'_>,
    path: &Bound<'_, PyAny>,
    class_name: Option<String>,
    root: &Bound<'_, PyAny>,
) -> PyResult<Py<PyAny>> {
    bridge::load_model_at(
        py,
        &path_from(path)?,
        class_name.as_deref(),
        &path_from(root)?,
    )
}

/// A style `to_text` may write in: `plain`, one given, or one the project
/// declares; any other refused naming those there are.
fn style_known(config: Option<&ProjectConfig>, style: Option<&str>) -> PyResult<()> {
    let Some(name) = style else {
        return Ok(());
    };
    if name == "plain"
        || name == crate::config::project_config::FILES_STYLE
        || config.is_some_and(|config| {
            config.style(name).is_some() || config.render_style_names().contains(&name)
        })
        || crate::config::project_config::TextFormat::GIVEN.contains(&name)
    {
        return Ok(());
    }
    let known: Vec<String> = config.map_or_else(
        || {
            crate::config::project_config::TextFormat::GIVEN
                .iter()
                .map(|name| name.to_string())
                .collect()
        },
        |config| {
            config
                .render_style_names()
                .iter()
                .map(|name| name.to_string())
                .collect()
        },
    );
    Err(PyValueError::new_err(format!(
        "no style named '{name}': {} are, and [style.NAME] declares another",
        known.join(", ")
    )))
}

/// A sample written as `view` shows it, each value in `style`'s forms.
/// A sample written as `view` writes its file: [`sheet_of`], leaving out
/// what only the model declares and nothing holds.
fn sheet_held(
    sample: &Sample,
    path: Option<&Path>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    held: Option<&[String]>,
) -> String {
    let vocabulary = fields::vocabulary_of(&[sample]);
    let subject = fields::Subject {
        sample,
        path,
        vocabulary: &vocabulary,
        states: None,
    };
    let style = style.or_else(|| config.and_then(|config| config.render().style.as_deref()));
    let precision = config.and_then(|config| config.render().precision.as_ref());
    let drawn = config
        .and_then(|config| config.render().table.as_deref())
        .and_then(terminal_rendering::Style::parse)
        .unwrap_or_default();
    terminal_rendering::sheet_of_held(
        &subject,
        config,
        style,
        precision,
        terminal_rendering::Target::Pipe,
        false,
        drawn,
        held,
        None,
    )
}

fn sheet_of(
    sample: &Sample,
    path: Option<&Path>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
    table: Option<&Identifier>,
) -> String {
    let vocabulary = fields::vocabulary_of(&[sample]);
    let subject = fields::Subject {
        sample,
        path,
        vocabulary: &vocabulary,
        states: None,
    };
    let style = style.or_else(|| config.and_then(|config| config.render().style.as_deref()));
    let precision = config.and_then(|config| config.render().precision.as_ref());
    let drawn = config
        .and_then(|config| config.render().table.as_deref())
        .and_then(terminal_rendering::Style::parse)
        .unwrap_or_default();
    let written = match table {
        Some(name) => terminal_rendering::unfolded_table(
            &subject,
            name,
            config,
            style,
            precision,
            terminal_rendering::Target::Pipe,
            drawn,
        )
        .unwrap_or_default(),
        None => terminal_rendering::sheet(
            &subject,
            config,
            style,
            precision,
            terminal_rendering::Target::Pipe,
            false,
            drawn,
            None,
        ),
    };
    written.trim_end().to_string()
}

/// From now on in this process, a save is checked as ever and held, not
/// written: a script run by `samplekit run`, whose changes are shown first.
#[pyfunction(name = "_hold_saves")]
fn hold_saves() {
    document::hold_saves();
}

/// What the held saves would write: each file's path, what it held before —
/// `None` where it did not exist — and what it would hold.
#[pyfunction(name = "_held_saves")]
fn held_saves() -> Vec<(String, Option<String>, String)> {
    document::held_saves()
        .into_iter()
        .map(|(path, before, after)| {
            (
                path.to_string_lossy().into_owned(),
                before.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
                after,
            )
        })
        .collect()
}

/// Writes the model's description beside the configuration whose folder is
/// `root`: what `sample` — one of the model's, made without a file —
/// declares, each formula's digest the worker took, the figures the class
/// declares, its scripts, and the digest of the model's files. The
/// worker's, and the figures' process's; no script's.
#[pyfunction(name = "_describe_model")]
#[pyo3(signature = (sample, model, class_name, root, formulas, figures, scripts = None))]
#[allow(clippy::too_many_arguments)]
fn describe_model(
    py: Python<'_>,
    sample: PyRef<'_, PySample>,
    model: &Bound<'_, PyAny>,
    class_name: String,
    root: &Bound<'_, PyAny>,
    formulas: std::collections::BTreeMap<String, String>,
    figures: Vec<(String, bool, String)>,
    scripts: Option<String>,
) -> PyResult<String> {
    // Each script as `_scripts.described` gives it, in JSON.
    let scripts: Vec<crate::config::model_runtime::DescribedScript> = match scripts {
        Some(text) => serde_json::from_str(&text)
            .map_err(|error| PyValueError::new_err(format!("the scripts' description: {error}")))?,
        None => Vec::new(),
    };
    use crate::config::model_runtime::{
        self as runtime, DescribedCell, DescribedColumn, DescribedFigure, DescribedModel,
        DescribedProperty, DescribedTable, Draws, ModelDescription, Origin as Given,
    };
    let model = path_from(model)?;
    let root = path_from(root)?;
    let template = runtime::Template::at(&model, Some(&class_name), &root.join(project::FILENAME));
    let digest =
        runtime::digest_of(&template).map_err(|error| PyOSError::new_err(error.to_string()))?;
    // What each table's derivation fills, and how often it runs, as the
    // model declared it.
    let spans: HashMap<String, (bool, Vec<String>)> = {
        let file = sample.file.borrow();
        file.formula_code
            .iter()
            .filter_map(|(name, parts)| {
                // A table's formula names what it fills; a property's does not.
                if !parts.iter().any(|(role, _)| role == "outputs") {
                    return None;
                }
                let by_row = !parts.iter().any(|(role, _)| role == "compute_column");
                let fills = parts
                    .iter()
                    .find(|(role, _)| role == "outputs")
                    .and_then(|(_, text)| text.extract::<String>(py).ok())
                    .map(|text| text.split(", ").map(str::to_string).collect())
                    .unwrap_or_default();
                Some((name.clone(), (by_row, fills)))
            })
            .collect()
    };
    let named = |nodes: Vec<Node>| -> Vec<String> { nodes.iter().map(node_name).collect() };
    let texts =
        |inputs: &[InputName]| -> Vec<String> { inputs.iter().map(ToString::to_string).collect() };
    let description = sample.shared().read_sample("the model", |held| {
        let mut properties = IndexMap::new();
        for name in held.property_names() {
            let handle = held.property(name).or_raise(py)?;
            let reads = named(held.dependencies_of(name).or_raise(py)?);
            let channels = held.channel_inputs_of(name);
            let value_reads = channels
                .as_ref()
                .map_or_else(|| reads.clone(), |channels| texts(&channels.value));
            let uncertainty_reads = channels
                .as_ref()
                .map_or_else(|| reads.clone(), |channels| texts(&channels.uncertainty));
            let described = handle.peek(|property| {
                let value = if property.is_joint() {
                    Given::Quantity {
                        reads: reads.clone(),
                    }
                } else if property.is_computed() {
                    Given::Formula {
                        reads: value_reads.clone(),
                    }
                } else if let Some(location) = property.declared_location() {
                    Given::Statistic {
                        statistic: location.name().to_string(),
                    }
                } else {
                    Given::Entered
                };
                let uncertainty = if property.is_joint() {
                    Some(Given::Quantity {
                        reads: reads.clone(),
                    })
                } else if property.has_uncertainty_formula() {
                    Some(Given::Formula {
                        reads: uncertainty_reads.clone(),
                    })
                } else {
                    property
                        .declared_convention()
                        .map(|convention| Given::Statistic {
                            statistic: convention.name().to_string(),
                        })
                };
                let default = (matches!(value, Given::Entered) && property.readings().is_none())
                    .then(|| property.peek_value())
                    .flatten()
                    .filter(|value| !matches!(value, Value::Absent))
                    .map(|value| runtime::value_to_json(&value));
                DescribedProperty {
                    unit: property.presentation().unit.clone(),
                    value,
                    uncertainty,
                    reads: reads.clone(),
                    formula: formulas.get(name.as_str()).cloned(),
                    default,
                }
            });
            properties.insert(name.to_string(), described);
        }
        let mut tables = IndexMap::new();
        for name in held.table_names() {
            let table = held.table(name).or_raise(py)?;
            let mut columns = Vec::new();
            for column in table.column_names() {
                let view = table.column(column).or_raise(py)?;
                let statistics = view.statistics();
                let label = format!("{name}.{column}");
                let derived = table.is_derived(column).then(|| {
                    let reads = table.inputs_of(column).map(texts).unwrap_or_default();
                    let (by_row, fills) = spans
                        .get(&label)
                        .cloned()
                        .unwrap_or_else(|| (true, vec![column.to_string()]));
                    if by_row {
                        Given::Rows { reads, fills }
                    } else {
                        Given::Columns { reads, fills }
                    }
                });
                let entered_value = || match statistics.value {
                    Some(location) => Given::Statistic {
                        statistic: location.name().to_string(),
                    },
                    None => Given::Entered,
                };
                let stated_uncertainty =
                    statistics.uncertainty.map(|convention| Given::Statistic {
                        statistic: convention.name().to_string(),
                    });
                // A formula of one channel alone says which: the other is
                // entered, or a statistic of the cell's readings.
                let (value, uncertainty) = match (derived, table.produced_by(column)) {
                    (None, _) => (entered_value(), stated_uncertainty),
                    (Some(derived), None) => (derived, stated_uncertainty),
                    (Some(derived), Some(Produced::Value)) => {
                        (derived, Some(stated_uncertainty.unwrap_or(Given::Entered)))
                    }
                    (Some(derived), Some(Produced::Uncertainty)) => {
                        (entered_value(), Some(derived))
                    }
                };
                columns.push(DescribedColumn {
                    name: column.to_string(),
                    unit: view.presentation().unit.clone(),
                    value,
                    uncertainty,
                    formula: formulas.get(&label).cloned(),
                });
            }
            tables.insert(
                name.to_string(),
                DescribedTable {
                    title: table.title().map(str::to_string),
                    index: table
                        .index_columns()
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    // Each cell as a file would hold it, running nothing: a
                    // derived cell no formula has filled holds nothing.
                    rows: table
                        .rows()
                        .map(|row| {
                            row.column_names()
                                .into_iter()
                                .filter_map(|column| {
                                    let held = crate::format::schema::property_as_is(
                                        row.cell(column).ok()?,
                                    );
                                    let cell = DescribedCell {
                                        value: held.value.as_ref().map(runtime::value_to_json),
                                        uncertainty: held.uncertainty,
                                        readings: held.readings,
                                    };
                                    (!cell.is_empty()).then(|| (column.to_string(), cell))
                                })
                                .collect()
                        })
                        .collect(),
                    columns,
                },
            );
        }
        let mut attributes = IndexMap::new();
        for name in held.attribute_names() {
            let value = held.attribute(name).or_raise(py)?;
            let json = match value.as_list() {
                Some(items) => {
                    serde_json::Value::Array(items.iter().map(runtime::value_to_json).collect())
                }
                None => value
                    .as_scalar()
                    .map_or(serde_json::Value::Null, runtime::value_to_json),
            };
            attributes.insert(name.to_string(), json);
        }
        Ok(ModelDescription {
            format: runtime::FORMAT,
            samplekit: runtime::VERSION.to_string(),
            model: DescribedModel {
                path: runtime::described_path(&root, &model),
                class: class_name.clone(),
                digest: digest.as_str().to_string(),
            },
            properties,
            tables,
            attributes,
            figures: figures
                .iter()
                .map(|(name, collection, description)| DescribedFigure {
                    name: name.clone(),
                    draws: if *collection {
                        Draws::Collection
                    } else {
                        Draws::Sample
                    },
                    description: description.clone(),
                })
                .collect(),
            scripts,
        })
    })?;
    runtime::write_description(&root, &description)
        .map_err(|error| PyOSError::new_err(error.to_string()))?;
    Ok(runtime::description_path_in(&root)
        .to_string_lossy()
        .into_owned())
}

/// The message for a class attribute that does not exist, naming what
/// replaced a removed loader.
#[pyfunction(name = "_missing_class_member")]
fn missing_class_member(class: &Bound<'_, PyAny>, name: &str) -> PyResult<String> {
    let class_name: String = class.getattr("__name__")?.extract()?;
    Ok(match name {
        "load" | "load_with_model" | "load_data" => format!(
            "{class_name}.{name} does not exist: a sample is loaded by constructing it, \
             {class_name}(path)"
        ),
        _ => {
            let members = members_of(class);
            let mut message = format!("type object '{class_name}' has no attribute '{name}'");
            if let Some(nearest) = identifier::nearest(name, members.iter().map(String::as_str)) {
                message.push_str(&format!("\n  did you mean: {nearest}?"));
            }
            message
        }
    })
}

/// A statistic of a property's readings, which a model declares to give the
/// value or the uncertainty.
///
/// The statistics are in ``samplekit.stats``.
///
/// Example:
///     >>> sk.Property(value=sk.stats.median, uncertainty=sk.stats.standard_error)
///     Property(value=None)
#[pyclass(name = "Statistic", module = "samplekit", frozen)]
pub struct PyStatistic {
    name: &'static str,
}

#[pymethods]
impl PyStatistic {
    /// Its name, as ``Summary`` spells it — ``"median"``.
    #[getter]
    fn name(&self) -> &'static str {
        self.name
    }

    fn __repr__(&self) -> String {
        format!("sk.stats.{}", self.name)
    }
}

/// The statistics a model can declare for readings.
///
/// Six give a value — ``mean``, ``median``, ``minimum``, ``maximum``,
/// ``first_quartile``, ``third_quartile`` — and three give an uncertainty —
/// ``standard_error``, ``sample_stdev``, ``population_stdev``.
///
/// Example:
///     >>> width = sk.Property(unit="mm", value=sk.stats.mean,
///     ...                     uncertainty=sk.stats.standard_error)
#[pyclass(name = "stats", module = "samplekit", frozen)]
pub struct PyStats;

#[pymethods]
impl PyStats {
    #[classattr]
    fn mean() -> PyStatistic {
        PyStatistic { name: "mean" }
    }
    #[classattr]
    fn median() -> PyStatistic {
        PyStatistic { name: "median" }
    }
    #[classattr]
    fn minimum() -> PyStatistic {
        PyStatistic { name: "minimum" }
    }
    #[classattr]
    fn maximum() -> PyStatistic {
        PyStatistic { name: "maximum" }
    }
    #[classattr]
    fn first_quartile() -> PyStatistic {
        PyStatistic {
            name: "first_quartile",
        }
    }
    #[classattr]
    fn third_quartile() -> PyStatistic {
        PyStatistic {
            name: "third_quartile",
        }
    }
    #[classattr]
    fn standard_error() -> PyStatistic {
        PyStatistic {
            name: "standard_error",
        }
    }
    #[classattr]
    fn sample_stdev() -> PyStatistic {
        PyStatistic {
            name: "sample_stdev",
        }
    }
    #[classattr]
    fn population_stdev() -> PyStatistic {
        PyStatistic {
            name: "population_stdev",
        }
    }
}

fn location_of(given: &Bound<'_, PyAny>) -> PyResult<crate::core::statistics::Location> {
    let statistic: PyRef<'_, PyStatistic> = given.extract()?;
    crate::core::statistics::Location::ALL
        .into_iter()
        .find(|location| location.name() == statistic.name)
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "{} is a spread, which stands for an uncertainty: a value is the mean, median, \
                 minimum, maximum, first_quartile or third_quartile",
                statistic.name
            ))
        })
}

fn convention_of(given: &Bound<'_, PyAny>) -> PyResult<crate::core::uncertainty::Convention> {
    use crate::core::uncertainty::Convention;
    let statistic: PyRef<'_, PyStatistic> = given.extract()?;
    match statistic.name {
        "standard_error" => Ok(Convention::StandardError),
        "sample_stdev" => Ok(Convention::SampleStdev),
        "population_stdev" => Ok(Convention::PopulationStdev),
        name => Err(PyValueError::new_err(format!(
            "{name} says where the readings lie, which stands for a value: an uncertainty is \
             the standard_error, sample_stdev or population_stdev"
        ))),
    }
}

/// Read samples as the command line reads its targets.
///
/// A folder is read as a ``SampleList`` of its samples, as its project
/// declares them, and a file as one ``Sample``; each is read with its
/// project's model. Several paths are one ``SampleList``, in the order given,
/// each sample once.
///
/// Args:
///     *paths: Folders and sample files.
///
/// Returns:
///     A ``Sample`` for one file, otherwise a ``SampleList``.
///
/// Raises:
///     FileNotFoundError: A path does not exist; the message names the
///         nearest.
///     ValueError: A file is not a sample.
///
/// Example:
///     >>> specimens = sk.load("samples")
///     >>> steel = sk.load("samples/steel-01.md")
///     >>> steel.area
///     Property(value=25.05586444444445, uncertainty=0.05217296159913354, unit='mm²')
#[pyfunction]
#[pyo3(signature = (*paths), text_signature = "(*paths)")]
fn load(py: Python<'_>, paths: &Bound<'_, PyTuple>) -> PyResult<Py<PyAny>> {
    let one = |path: &Bound<'_, PyAny>| -> PyResult<Py<PyAny>> {
        let written: PathBuf = path.extract()?;
        let class = if written.is_dir() {
            "SampleList"
        } else {
            "Sample"
        };
        Ok(py
            .import("samplekit")?
            .getattr(class)?
            .call1((path,))?
            .unbind())
    };
    match paths.len() {
        0 => Err(PyTypeError::new_err(
            "load wants a path: a sample file or a folder, or several",
        )),
        1 => one(&paths.get_item(0)?),
        _ => {
            // As the command line reads its targets: a target given twice is
            // read once, and said; a sample reached twice — a file and the
            // folder holding it — is one sample, first where it was first
            // reached.
            let own =
                |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
            let mut targets: Vec<PathBuf> = Vec::new();
            let mut given: Vec<Bound<'_, PyAny>> = Vec::new();
            for path in paths.iter() {
                let written: PathBuf = path.extract()?;
                if !targets.contains(&own(&written)) {
                    targets.push(own(&written));
                    given.push(path);
                }
            }
            let twice = paths.len() - given.len();
            if twice > 0 {
                warn_value(
                    py,
                    &format!(
                        "{twice} {} given more than once, read once",
                        if twice == 1 {
                            "target was"
                        } else {
                            "targets were"
                        }
                    ),
                )?;
            }
            let mut samples: Vec<Py<PyAny>> = Vec::new();
            let mut seen: Vec<PathBuf> = Vec::new();
            let mut keep = |sample: Py<PyAny>, seen: &mut Vec<PathBuf>| -> PyResult<()> {
                let path = sample
                    .bind(py)
                    .cast::<PySample>()?
                    .borrow()
                    .file
                    .borrow()
                    .path
                    .clone();
                if let Some(path) = path {
                    if seen.contains(&own(&path)) {
                        return Ok(());
                    }
                    seen.push(own(&path));
                }
                samples.push(sample);
                Ok(())
            };
            for path in given {
                let loaded = one(&path)?;
                let bound = loaded.bind(py);
                match bound.cast::<PySampleList>() {
                    Ok(list) => {
                        for sample in list.borrow().objects(py) {
                            keep(sample, &mut seen)?;
                        }
                    }
                    Err(_) => keep(loaded, &mut seen)?,
                }
            }
            Ok(Py::new(py, PySampleList::of(py, samples)?)?.into_any())
        }
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    // In Python, reading a value never runs its formula: compute does.
    crate::core::property::set_reads_compute(false);
    module.add_function(pyo3::wrap_pyfunction!(check_new, module)?)?;
    module.add_class::<PyProperty>()?;
    module.add_class::<PyComputedColumns>()?;
    module.add_class::<PyRowView>()?;
    module.add_class::<PyColumnView>()?;
    module.add_class::<PyTable>()?;
    module.add_class::<PySample>()?;
    module.add_class::<PySampleList>()?;
    module.add_class::<PySummary>()?;
    module.add_class::<PyStatistic>()?;
    module.add_class::<PyStats>()?;
    module.add_class::<PyField>()?;
    module.add_class::<PyNames>()?;
    module.add_class::<PyProject>()?;
    module.add_class::<PyQuery>()?;
    module.add_class::<PyColumnSpec>()?;
    module.add_class::<PyProfile>()?;
    module.add_class::<PyExport>()?;
    module.add_class::<PyFigureDeclaration>()?;
    module.add_class::<PyPropertyDeclaration>()?;
    module.add_class::<PyUnit>()?;
    module.add_class::<PyRender>()?;
    module.add_class::<PyModel>()?;
    module.add_class::<PyCollection>()?;
    module.add_function(wrap_pyfunction!(register_package, module)?)?;
    module.add_function(wrap_pyfunction!(load, module)?)?;
    module.add_function(wrap_pyfunction!(model_for, module)?)?;
    module.add_function(wrap_pyfunction!(model_at, module)?)?;
    module.add_function(wrap_pyfunction!(describe_model, module)?)?;
    module.add_function(wrap_pyfunction!(hold_saves, module)?)?;
    module.add_function(wrap_pyfunction!(held_saves, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    // The KeyError every lookup raises, for the package's Python to raise too.
    module.add(
        "_KeyError",
        bridge::key_error_type(module.py())?.clone_ref(module.py()),
    )?;
    module.add_function(wrap_pyfunction!(missing_class_member, module)?)?;
    module.add_function(wrap_pyfunction!(_history_before_output, module)?)?;
    module.add_function(wrap_pyfunction!(script_said_to_python, module)?)?;
    module.add_function(wrap_pyfunction!(_history_flush, module)?)?;
    module.add_function(wrap_pyfunction!(_history_tag_output, module)?)?;
    let py = module.py();
    py.import("collections.abc")?
        .getattr("Sequence")?
        .call_method1("register", (module.getattr("SampleList")?,))?;
    Ok(())
}

impl PySample {
    /// A sample's own files: it needs its file, whose name finds them, and a
    /// project, which says where to look.
    fn own_files(&self, pattern: Option<&str>) -> PyResult<Vec<PathBuf>> {
        let file = self.file.borrow();
        let Some(path) = file.path.clone() else {
            return Err(PyValueError::new_err(
                "this sample has no file yet, and its files are found by its file's name: \
                 save(path) gives it one",
            ));
        };
        let Some(config) = file.config.clone() else {
            return Err(PyValueError::new_err(
                "no .samplekitrc above this sample says where its files are: \
                 [collection] files = [\"../images\"]",
            ));
        };
        crate::config::discovery::sample_files(&config, &path, pattern)
            .map_err(|error| pyo3::exceptions::PyOSError::new_err(error.to_string()))
    }
}

/// What opening a file met, as the exception Python names it.
fn opening_failure(error: crate::presentation::opening::OpenError) -> PyErr {
    use crate::presentation::opening::OpenError;
    match error {
        OpenError::NoDisplay { .. } => PyRuntimeError::new_err(error.to_string()),
        _ => pyo3::exceptions::PyOSError::new_err(error.to_string()),
    }
}

/// Each formula's digest in the model of `sample`, by the value it gives, as
/// the worker takes them: `None` for a sample of no model, or where they could
/// not be taken — its values are then kept in no trace, and judged by their
/// inputs alone.
fn formula_digests(
    sample: &Bound<'_, PySample>,
) -> Option<std::collections::BTreeMap<String, String>> {
    let py = sample.py();
    let class = sample.get_type();
    if class.is(py.get_type::<PySample>()) {
        return None;
    }
    let Some(file) = model_file(&class) else {
        // Said, not passed over: nothing is kept of what computed the values.
        if let Ok(warnings) = py.import("warnings") {
            let _ = warnings.call_method1(
                "warn",
                (format!(
                    "{} is defined where no file holds it — a REPL, python -c: what computed \
                     its values is not kept, and a formula changed later is not seen",
                    class
                        .name()
                        .map(|name| name.to_string())
                        .unwrap_or_default()
                ),),
            );
        }
        return None;
    };
    py.import("samplekit._worker")
        .ok()?
        .call_method1("formula_digests", (sample, file))
        .ok()?
        .extract()
        .ok()
}

/// The file a model class is written in, where one holds it.
fn model_file(class: &Bound<'_, pyo3::types::PyType>) -> Option<String> {
    class
        .py()
        .import("inspect")
        .ok()?
        .call_method1("getfile", (class,))
        .ok()?
        .extract()
        .ok()
}

/// A model's formula digests taken the first time it is used in the process,
/// before any formula runs: the source as it runs, and what each formula
/// reaches as it was before them.
pub(crate) fn prime_model(class: &Bound<'_, pyo3::types::PyType>) {
    let py = class.py();
    if class.is(py.get_type::<PySample>()) {
        return;
    }
    let Some(file) = model_file(class) else {
        return;
    };
    if let Ok(worker) = py.import("samplekit._worker") {
        let _ = worker.call_method1("prime", (class, file));
    }
}

/// Ctrl-C while `save_all()` writes: the file under way was finished, and
/// the list stops there, `KeyboardInterrupt` naming the files written; the
/// script's snapshot says it was interrupted.
/// Ctrl-C, noted while a list is written rather than raised where it
/// lands: raised inside a sample's save, the `KeyboardInterrupt` was lost
/// there, and the list went on to its end. Python's own handler is put back
/// when this is dropped; `None` off the main thread, where Python allows no
/// handler, and the signals are then asked as they come.
struct CaughtInterrupts {
    caught: Py<PyList>,
    previous: Py<PyAny>,
}

impl Drop for CaughtInterrupts {
    fn drop(&mut self) {
        Python::attach(|py| {
            if let Ok(signal) = py.import("signal")
                && let Ok(number) = signal.getattr("SIGINT")
            {
                let _ = signal.call_method1("signal", (number, self.previous.bind(py)));
            }
        });
    }
}

fn catch_interrupts(py: Python<'_>) -> Option<CaughtInterrupts> {
    let signal = py.import("signal").ok()?;
    let number = signal.getattr("SIGINT").ok()?;
    let previous = signal.call_method1("getsignal", (&number,)).ok()?.unbind();
    let caught = PyList::empty(py);
    let noting = py
        .eval(
            c"lambda caught: lambda signum, frame: caught.append(signum)",
            None,
            None,
        )
        .ok()?
        .call1((&caught,))
        .ok()?;
    signal.call_method1("signal", (number, noting)).ok()?;
    Some(CaughtInterrupts {
        caught: caught.unbind(),
        previous,
    })
}

fn stop_if_interrupted(
    py: Python<'_>,
    written: &[Py<PyAny>],
    caught: Option<&CaughtInterrupts>,
) -> PyResult<()> {
    let noted = caught.is_some_and(|caught| !caught.caught.bind(py).is_empty());
    let asked = if noted {
        Err(pyo3::exceptions::PyKeyboardInterrupt::new_err(""))
    } else {
        py.check_signals()
    };
    if let Err(error) = asked {
        SAVE_INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
        if !error.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) {
            return Err(error);
        }
        let names: Vec<String> = written
            .iter()
            .map(|path| path.bind(py).str().map(|text| text.to_string()))
            .collect::<PyResult<_>>()?;
        return Err(pyo3::exceptions::PyKeyboardInterrupt::new_err(format!(
            "interrupted: {} of the list written, the others left as they were{}",
            names.len(),
            if names.is_empty() {
                String::new()
            } else {
                format!(": {}", names.join(", "))
            }
        )));
    }
    Ok(())
}

/// A `save_all()` stopped by Ctrl-C: the script's snapshot says so.
static SAVE_INTERRUPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A formula writes only the value it returns: one that changed an input, an
/// attribute or the tags while it ran raised, naming what it changed, and
/// its value failed — a measurement is the raw evidence, and one a formula
/// touched was rewritten at every run, in silence.
fn refuse_while_a_formula_runs(what: &str) -> PyResult<()> {
    if crate::core::property::is_computing() {
        return Err(PyValueError::new_err(format!(
            "a formula writes only the value it returns: this one changed {what}, and nothing \
             of it was written"
        )));
    }
    Ok(())
}

/// The kind of an attribute's value, as a warning names it: `text`, `a
/// number`, `a date`, `true or false`, `a list`; `None` for none at all.
fn attribute_kind(attribute: &AttributeValue) -> Option<&'static str> {
    match attribute {
        AttributeValue::List(_) => Some("a list"),
        AttributeValue::Scalar(value) => match value {
            Value::Text(_) => Some("text"),
            Value::Integer(_) | Value::Number(_) => Some("a number"),
            Value::Date(_) | Value::DateTime(_) => Some("a date"),
            Value::Boolean(_) => Some("true or false"),
            Value::Absent | Value::NotApplicable => None,
        },
    }
}

/// What a computation marks a sample as having done: its next save writes it
/// whole, every value computed and recorded; with none, a save writes only
/// the fields the script changed.
const COMPUTED: &str = "\u{0}computed";
