//! The boundary between Rust and Python: converting scalar values, translating
//! each error family into the standard exception a Python programmer expects,
//! wrapping a Python callable as a formula the core runs, sharing one sample
//! between every wrapper of it, and importing a project's model.
//!
//! It adds no behavior of its own. A rule that existed only here would be one
//! the command line does not follow.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use indexmap::IndexMap;
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::{
    PyAttributeError, PyFileExistsError, PyFileNotFoundError, PyIndexError, PyIsADirectoryError,
    PyKeyError, PyNotADirectoryError, PyOSError, PyOverflowError, PyPermissionError,
    PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{
    PyBool, PyDate, PyDateAccess, PyDateTime, PyDict, PyFloat, PyInt, PyList, PyString,
    PyTimeAccess, PyTuple, PyType, PyTzInfoAccess,
};

use crate::collection::exports::ExportError;
use crate::collection::named_queries::QueryError;
use crate::collection::sample_list::ListError;
use crate::config::discovery::DiscoveryError;
use crate::config::profiles::ColumnError;
use crate::config::project_config::{self as config, ConfigError, ProjectConfig};
use crate::core::dependency_graph::DependencyError;
use crate::core::formatting::FormatError;
use crate::core::identifier::{self, Identifier, IdentifierError};
use crate::core::property::{Compute, ComputeError, ComputeQuantity};
use crate::core::sample::{AttributeError, AttributeValue, PropertyHandle, Sample, SampleError};
use crate::core::table::{
    CellOutput, ColumnSet, ComputeColumn, ComputeRow, RowView, Scope, TableError,
};
use crate::core::uncertainty::{Uncertainty, UncertaintyError};
use crate::core::value::{Date, DateTime, Value, ValueError};
use crate::format::document::DocumentError;
use crate::format::fingerprint::{self, FingerprintError};
use crate::format::schema::SchemaError;
use crate::presentation::export_formats::ExportFormatError;
use crate::query::field_addressing::FieldError;
use crate::query::filter_language::FilterError;
use crate::query::ordering::OrderError;

use super::python_api;

// ------------------------------------------------------------- conversion

/// A scalar, back in the Python type it came from. A date-time is always a
/// naive `datetime`, because a sample stores no timezone.
pub fn to_python(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Absent => Ok(py.None()),
        // The package's one `NA`.
        Value::NotApplicable => Ok(py.import("samplekit")?.getattr("NA")?.unbind()),
        Value::Boolean(flag) => flag.into_py_any(py),
        Value::Integer(integer) => integer.into_py_any(py),
        Value::Number(number) => number.into_py_any(py),
        Value::Text(text) => text.into_py_any(py),
        Value::Date(date) => {
            let (year, month, day) = calendar(&date.iso());
            beyond_python(&date.iso(), year)?;
            PyDate::new(py, year, month, day)?.into_py_any(py)
        }
        Value::DateTime(moment) => {
            let iso = moment.iso();
            let (year, month, day) = calendar(&iso[..10]);
            beyond_python(&iso, year)?;
            let part = |range: std::ops::Range<usize>| -> u8 {
                iso.get(range)
                    .map(|digits| digits.parse().expect("a canonical time is digits"))
                    .unwrap_or(0)
            };
            PyDateTime::new(
                py,
                year,
                month,
                day,
                part(11..13),
                part(14..16),
                part(17..19),
                moment.nanosecond() / 1_000,
                None,
            )?
            .into_py_any(py)
        }
    }
}

/// Refuses a date Python's `datetime` cannot hold — before year 1, which a
/// file may write — saying so, rather than Python's bare "year 0 is out of
/// range" with no date named.
fn beyond_python(iso: &str, year: i32) -> PyResult<()> {
    if year < 1 {
        return Err(PyValueError::new_err(format!(
            "{iso} is a date Python cannot hold: its dates begin in year 1; the file keeps \
             it, and the command line reads it"
        )));
    }
    Ok(())
}

/// The three numbers of the `YYYY-MM-DD` that `Date::iso` writes.
fn calendar(iso: &str) -> (i32, u8, u8) {
    let year = iso[0..4].parse().expect("a canonical year is digits");
    let month = iso[5..7].parse().expect("a canonical month is digits");
    let day = iso[8..10].parse().expect("a canonical day is digits");
    (year, month, day)
}

/// A scalar from Python, checked in the order that keeps its overlapping types
/// apart: `bool` before `int`, and `datetime` before `date`.
pub fn from_python(object: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = object.py();
    if object.is_none() {
        return Ok(Value::absent());
    }
    if let Ok(package) = py.import("samplekit")
        && let Ok(not_applicable) = package.getattr("NA")
        && object.is(&not_applicable)
    {
        return Ok(Value::NotApplicable);
    }
    if let Ok(flag) = object.cast::<PyBool>() {
        return Ok(Value::boolean(flag.is_true()));
    }
    if object.is_instance_of::<PyInt>() {
        return object.extract::<i64>().map(Value::integer).map_err(|_| {
            PyOverflowError::new_err(format!(
                "{} does not fit in 64 bits, the widest integer a sample stores",
                repr(object)
            ))
        });
    }
    if let Ok(number) = object.cast::<PyFloat>() {
        return Value::number(number.value()).map_err(|error| error.into_py_err(py));
    }
    if let Ok(text) = object.cast::<PyString>() {
        return Ok(Value::text(text.to_str()?));
    }
    if let Ok(moment) = object.cast::<PyDateTime>() {
        if moment.get_tzinfo().is_some() {
            return Err(PyValueError::new_err(format!(
                "{} has a timezone, and a sample stores a local time without one: \
                 remove its tzinfo",
                repr(object)
            )));
        }
        // Its microseconds are kept.
        let microsecond = moment.get_microsecond();
        return DateTime::new(
            moment.get_year(),
            u32::from(moment.get_month()),
            u32::from(moment.get_day()),
            u32::from(moment.get_hour()),
            u32::from(moment.get_minute()),
            u32::from(moment.get_second()),
        )
        .and_then(|built| built.with_nanosecond(microsecond * 1_000))
        .map(Value::date_time)
        .map_err(|error| error.into_py_err(py));
    }
    if let Ok(date) = object.cast::<PyDate>() {
        return Date::new(
            date.get_year(),
            u32::from(date.get_month()),
            u32::from(date.get_day()),
        )
        .map(Value::date)
        .map_err(|error| error.into_py_err(py));
    }
    if let Some(value) = numpy_scalar(object)? {
        return Ok(value);
    }
    Err(PyTypeError::new_err(format!(
        "{} cannot be stored: a sample holds a bool, an int, a float, a str, a date \
         or a datetime",
        a_type(object)
    )))
}

/// numpy's scalars are not Python's `int` and `float`, and a formula returns
/// them constantly. Only numpy's: a `Decimal` has `__float__` too, and would
/// lose digits without a word.
fn numpy_scalar(object: &Bound<'_, PyAny>) -> PyResult<Option<Value>> {
    let kind = object.get_type();
    let module: String = kind.getattr("__module__")?.extract()?;
    if module != "numpy" {
        return Ok(None);
    }
    let name = kind.name()?.to_string();
    if name.starts_with("bool") {
        return Ok(Some(Value::boolean(object.is_truthy()?)));
    }
    // An array, which `__index__` refused in numpy's own words — *only integer
    // scalar arrays can be converted to a scalar index* — naming nothing the
    // formula did. One element is the number it holds; more is said as it is.
    if name == "ndarray" {
        let size: usize = object.getattr("size")?.extract()?;
        if size == 1 {
            return from_python(&object.call_method0("item")?).map(Some);
        }
        let shape = object.getattr("shape")?.str()?.to_string();
        return Err(PyTypeError::new_err(format!(
            "a formula returned an array of shape {shape}, and a value is one number: \
             return one element, or .item() of a one-element array"
        )));
    }
    if object.hasattr("__index__")? {
        return from_python(&object.call_method0("__index__")?).map(Some);
    }
    if object.hasattr("__float__")? {
        return from_python(&object.call_method0("__float__")?).map(Some);
    }
    Ok(None)
}

/// An attribute's value: a scalar, or a list whose items are each a scalar of
/// one family. A list is converted here and nowhere else, so that a formula
/// promised to return one cell cannot return a list.
pub fn attribute_from_python(object: &Bound<'_, PyAny>) -> PyResult<AttributeValue> {
    let py = object.py();
    if object.is_instance_of::<PyTuple>() {
        return Err(PyTypeError::new_err(
            "a tuple is not stored: use a list, whose items and order a sample keeps",
        ));
    }
    if !object.is_instance_of::<PyList>() {
        return from_python(object).map(AttributeValue::scalar);
    }
    let mut items = Vec::new();
    for item in object.try_iter()? {
        let item = item?;
        if item.is_instance_of::<PyList>() || item.is_instance_of::<PyTuple>() {
            return Err(PyTypeError::new_err(
                "a list attribute holds scalars, and a nested list is not one",
            ));
        }
        items.push(from_python(&item)?);
    }
    AttributeValue::list(items).map_err(|error| error.into_py_err(py))
}

/// An uncertainty from Python: a non-negative number, or `None` for none.
pub fn uncertainty_from_python(object: &Bound<'_, PyAny>) -> PyResult<Option<Uncertainty>> {
    let py = object.py();
    let magnitude = match from_python(object)? {
        Value::Absent => return Ok(None),
        Value::Number(number) => number,
        Value::Integer(integer) => integer as f64,
        other => {
            return Err(PyTypeError::new_err(format!(
                "an uncertainty is a number, and this is {}",
                other.kind()
            )));
        }
    };
    Uncertainty::new(magnitude)
        .map(Some)
        .map_err(|error| error.into_py_err(py))
}

/// An identifier from Python text, refused with the grammar's own message.
pub fn identifier_from(py: Python<'_>, name: &str) -> PyResult<Identifier> {
    Identifier::new(name).map_err(|error| error.into_py_err(py))
}

pub fn type_name(object: &Bound<'_, PyAny>) -> String {
    object
        .get_type()
        .name()
        .map(|name| name.to_string())
        .unwrap_or_else(|_| "object".to_string())
}

/// The type's name with its article, for a message: `an int`, `a str` — where
/// `a {}` wrote "a int" and "a object".
pub fn a_type(object: &Bound<'_, PyAny>) -> String {
    let name = type_name(object);
    let vowel = name
        .chars()
        .next()
        .is_some_and(|first| "aeiouAEIOU".contains(first));
    format!("{} {name}", if vowel { "an" } else { "a" })
}

pub fn repr(object: &Bound<'_, PyAny>) -> String {
    object
        .repr()
        .map(|text| text.to_string())
        .unwrap_or_else(|_| type_name(object))
}

// ---------------------------------------------------------------- errors

/// A Python exception raised inside a formula, carried through the core as the
/// source of a `ComputeError` and handed back unchanged: the traceback is how a
/// scientist debugs a formula.
pub struct PythonException(pub PyErr);

impl fmt::Debug for PythonException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PythonException({self})")
    }
}

/// The exception's type and message, as Python's last traceback line reads:
/// `ZeroDivisionError: division by zero`.
impl fmt::Display for PythonException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Python::attach(|py| {
            let kind = self
                .0
                .get_type(py)
                .name()
                .map(|name| name.to_string())
                .unwrap_or_default();
            if kind.is_empty() {
                write!(f, "{}", self.0.value(py))
            } else {
                write!(f, "{kind}: {}", self.0.value(py))
            }
        })
    }
}

impl std::error::Error for PythonException {}

thread_local! {
    /// The last exception a formula raised, for the errors that carry a
    /// formula's failure only as text — a sort key, a column of an export —
    /// and so have nowhere to hold the exception itself.
    static RAISED: RefCell<Option<PyErr>> = const { RefCell::new(None) };

    /// How many Python formulas have started on this thread, and how many are
    /// running now.
    static FORMULAS: Cell<(u64, u32)> = const { Cell::new((0, 0)) };
}

/// A formula running, for as long as it lives, whether it returns or unwinds.
struct Running;

impl Running {
    fn start() -> Running {
        FORMULAS.with(|state| {
            let (runs, depth) = state.get();
            state.set((runs + 1, depth + 1));
        });
        Running
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        FORMULAS.with(|state| {
            let (runs, depth) = state.get();
            state.set((runs, depth.saturating_sub(1)));
        });
    }
}

/// How many Python formulas have started on this thread.
pub fn formula_runs() -> u64 {
    FORMULAS.with(|state| state.get().0)
}

/// Records what the formulas started since `before` computed, while their
/// inputs are still the ones they read. Only outside every formula: the read
/// that started them records them, once they have all returned.
pub fn record_computed(sample: &SharedSample, before: u64) {
    let (runs, depth) = FORMULAS.with(Cell::get);
    if runs == before || depth > 0 {
        return;
    }
    if let Ok(mut held) = sample.inner.try_borrow_mut() {
        // A value that cannot be recorded is refused where it would be
        // written wrong — at save — so there is nothing to refuse here.
        fingerprint::stamp(&mut held).ok();
    }
}

/// A formula's exception, kept for whichever error reaches Python.
pub fn formula_failed(py: Python<'_>, error: PyErr) -> ComputeError {
    RAISED.with(|slot| *slot.borrow_mut() = Some(error.clone_ref(py)));
    // Ctrl-C inside a formula stops it; it is not something the formula got
    // wrong, and it must never be recorded as the value's failure.
    if error.is_instance_of::<pyo3::exceptions::PyKeyboardInterrupt>(py) {
        return ComputeError::interrupted(PythonException(error));
    }
    ComputeError::failed(PythonException(error))
}

/// Forgets a kept exception, before an operation whose failure may carry one
/// only as text, so that an old one is never raised for a new failure.
pub fn forget_raised() {
    RAISED.with(|slot| slot.borrow_mut().take());
}

fn raised_or(error: PyErr) -> PyErr {
    RAISED
        .with(|slot| slot.borrow_mut().take())
        .unwrap_or(error)
}

/// A `KeyError` whose message prints as it is written. Python prints a
/// `KeyError`'s one argument as its `repr`, because it is usually the key: a
/// message listing the names available and the nearest printed on one line,
/// quoted, its line breaks written `\n`. This one is a `KeyError` in every
/// other respect — `except KeyError` takes it, and a traceback names it so.
pub fn key_error<M: Into<String>>(message: M) -> PyErr {
    let message = suggestion_first(&message.into());
    Python::attach(|py| match key_error_type(py) {
        Ok(kind) => PyErr::from_type(kind.bind(py).clone(), (message,)),
        Err(_) => PyKeyError::new_err(message),
    })
}

/// A refusal's lines as Python writes every one: what failed, then `did you
/// mean: X?`, then what exists. The core's messages, written for the
/// command line, say the suggestion last; one form in a script is read
/// faster than two. Blank lines, which a list of nothing leaves, go too.
pub fn suggestion_first(message: &str) -> String {
    let mut lines: Vec<&str> = message
        .trim_end()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if let Some(at) = lines
        .iter()
        .position(|line| line.trim_start().starts_with("did you mean"))
        && at > 1
    {
        let suggestion = lines.remove(at);
        lines.insert(1, suggestion);
    }
    lines.join("\n")
}

/// An `AttributeError` whose message says everything, the nearest name
/// included. Python 3.12 and later look for a nearest name of their own in an
/// `AttributeError` that names none, and append it: the message said "did you
/// mean" twice. A `name` of `None` tells Python the error was named already.
pub fn said_in_full(error: PyErr) -> PyErr {
    Python::attach(|py| {
        if error.is_instance_of::<PyAttributeError>(py) {
            let _ = error.value(py).setattr("name", py.None());
        }
    });
    error
}

static KEY_ERROR: pyo3::sync::PyOnceLock<Py<PyType>> = pyo3::sync::PyOnceLock::new();

/// The class `key_error` raises, made once.
pub fn key_error_type(py: Python<'_>) -> PyResult<&Py<PyType>> {
    KEY_ERROR.get_or_try_init(py, || {
        let module = PyModule::from_code(
            py,
            c"class KeyError(KeyError):
    \"\"\"A KeyError whose message prints as it is written, on its lines.\"\"\"

    __module__ = \"builtins\"

    def __str__(self):
        if len(self.args) == 1 and isinstance(self.args[0], str):
            return self.args[0]
        return super().__str__()
",
            c"<samplekit>",
            c"samplekit._key_error",
        )?;
        Ok(module.getattr("KeyError")?.cast_into::<PyType>()?.unbind())
    })
}

/// Each error family becomes the standard Python exception that fits it. No
/// SampleKit exception hierarchy exists: a lookup's `KeyError` only prints
/// differently.
pub trait IntoPyErr {
    fn into_py_err(self, py: Python<'_>) -> PyErr;
}

pub fn map_error<E: IntoPyErr>(py: Python<'_>, error: E) -> PyErr {
    error.into_py_err(py)
}

/// `result.or_raise(py)?`: the exception decided by the error's family.
pub trait OrRaise<T> {
    fn or_raise(self, py: Python<'_>) -> PyResult<T>;
}

impl<T, E: IntoPyErr> OrRaise<T> for Result<T, E> {
    fn or_raise(self, py: Python<'_>) -> PyResult<T> {
        self.map_err(|error| error.into_py_err(py))
    }
}

fn os_error(source: &io::Error, message: String) -> PyErr {
    let message = without_os_code(&message);
    match source.kind() {
        io::ErrorKind::NotFound => PyFileNotFoundError::new_err(message),
        io::ErrorKind::PermissionDenied => PyPermissionError::new_err(message),
        io::ErrorKind::AlreadyExists => PyFileExistsError::new_err(message),
        io::ErrorKind::IsADirectory => PyIsADirectoryError::new_err(message),
        io::ErrorKind::NotADirectory => PyNotADirectoryError::new_err(message),
        _ => PyOSError::new_err(message),
    }
}

/// A system's message without the code it carries, `(os error 2)`: the
/// exception's type already says which failure it was, and the number
/// speaks to nobody reading it.
pub fn without_os_code(message: &str) -> String {
    let mut plain = String::with_capacity(message.len());
    let mut rest = message;
    while let Some(at) = rest.find(" (os error ") {
        let after = &rest[at + " (os error ".len()..];
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with(')') {
            plain.push_str(&rest[..at]);
            rest = &after[digits + 1..];
        } else {
            plain.push_str(&rest[..at + 1]);
            rest = &rest[at + 1..];
        }
    }
    plain.push_str(rest);
    plain
}

/// A formula's failure: its own exception when Python raised it, and a
/// `ValueError` with the core's message otherwise — a cycle, a bad magnitude.
fn compute_or(py: Python<'_>, source: ComputeError, message: String) -> PyErr {
    match &source {
        ComputeError::Failed { source: inner } | ComputeError::Interrupted { source: inner }
            if inner.downcast_ref::<PythonException>().is_some() =>
        {
            source.into_py_err(py)
        }
        _ => PyValueError::new_err(message),
    }
}

impl IntoPyErr for ComputeError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        if let ComputeError::Failed { source } | ComputeError::Interrupted { source } = &self
            && let Some(raised) = source.downcast_ref::<PythonException>()
        {
            return raised.0.clone_ref(py);
        }
        PyValueError::new_err(self.to_string())
    }
}

impl IntoPyErr for ValueError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            ValueError::IncomparableKinds { .. }
            | ValueError::ComparisonWithAbsent
            | ValueError::NotText { .. } => PyTypeError::new_err(message),
            _ => PyValueError::new_err(message),
        }
    }
}

macro_rules! value_errors {
    ($($family:ty),* $(,)?) => {
        $(impl IntoPyErr for $family {
            fn into_py_err(self, _: Python<'_>) -> PyErr {
                PyValueError::new_err(self.to_string())
            }
        })*
    };
}

value_errors!(
    UncertaintyError,
    IdentifierError,
    AttributeError,
    FormatError,
    ColumnError,
    DependencyError,
    ExportFormatError,
);

impl IntoPyErr for FingerprintError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            FingerprintError::UnknownInput { .. } => key_error(message),
            _ => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for SampleError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            SampleError::UnknownProperty { .. }
            | SampleError::UnknownTable { .. }
            | SampleError::UnknownAttribute { .. } => key_error(message),
            SampleError::Compute { source, .. } | SampleError::ComputeCell { source, .. } => {
                compute_or(py, source, message)
            }
            SampleError::Table(error) => error.into_py_err(py),
            SampleError::PrivateName { .. }
            | SampleError::NameCollision { .. }
            | SampleError::Dependency(_)
            | SampleError::FillConflict { .. } => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for TableError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        // `no row at (99)` was followed by an empty line.
        let message = suggestion_first(&self.to_string());
        match self {
            TableError::UnknownColumn { .. }
            | TableError::UnknownIndex { .. }
            | TableError::IndexArity { .. } => key_error(message),
            TableError::OrdinalOutOfRange { .. } => PyIndexError::new_err(message),
            TableError::Compute { source, .. } => compute_or(py, source, message),
            _ => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for SchemaError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        match self {
            SchemaError::Sample(error) => error.into_py_err(py),
            other => PyValueError::new_err(other.to_string()),
        }
    }
}

/// A file without frontmatter, said to a script. The command line's words
/// speak of a file something else claimed was a sample; a script named this
/// one itself.
const NO_FRONTMATTER: &str = "this file has no frontmatter, so it is not a sample: a sample \
                              file begins with --- on its first line";

impl IntoPyErr for DocumentError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            DocumentError::Io { source, .. } => os_error(&source, message),
            DocumentError::ConcurrentEdit { .. } | DocumentError::WrittenSince { .. } => {
                PyOSError::new_err(message)
            }
            DocumentError::Schema(error) => error.into_py_err(py),
            DocumentError::Compute { .. } => raised_or(PyValueError::new_err(message)),
            DocumentError::MissingFrontmatter => PyValueError::new_err(NO_FRONTMATTER),
            _ => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for DiscoveryError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            DiscoveryError::NotFound { .. } => PyFileNotFoundError::new_err(message),
            DiscoveryError::NotADirectory { .. } => PyNotADirectoryError::new_err(message),
            DiscoveryError::Io { source, .. } => os_error(&source, message),
        }
    }
}

impl IntoPyErr for ConfigError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            ConfigError::Io { source, .. } => os_error(&source, message),
            ConfigError::UnknownProfile { .. } => key_error(message),
            _ => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for FieldError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            FieldError::UnknownProperty { .. }
            | FieldError::UnknownTable { .. }
            | FieldError::UnknownColumn { .. }
            | FieldError::UnknownIndex { .. }
            | FieldError::UnknownChannel { .. }
            | FieldError::MissingIndex { .. } => key_error(message),
            FieldError::ListIndexOutOfRange { .. } => PyIndexError::new_err(message),
            FieldError::Compute { .. } => raised_or(PyValueError::new_err(message)),
            FieldError::Malformed { .. } | FieldError::TableNeedsCell { .. } => {
                PyValueError::new_err(message)
            } // The caller's omission, not the data's.
            FieldError::StatesNotRead => PyRuntimeError::new_err(message),
        }
    }
}

impl IntoPyErr for FilterError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            FilterError::Field(error) => error.into_py_err(py),
            FilterError::TypeConflict(_) | FilterError::WrongOperator { .. } => {
                PyTypeError::new_err(message)
            }
            _ => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for OrderError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            OrderError::UnknownField(error) => error.into_py_err(py),
            OrderError::Compute { .. } => raised_or(PyValueError::new_err(message)),
            OrderError::Unsortable { .. } => PyTypeError::new_err(message),
            OrderError::EmptySpec | OrderError::Syntax { .. } => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for ListError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            ListError::Discovery(error) => error.into_py_err(py),
            ListError::Document { error, path } => match error {
                DocumentError::Io { source, .. } => os_error(&source, message),
                DocumentError::Schema(SchemaError::Sample(error)) => error.into_py_err(py),
                DocumentError::MissingFrontmatter => PyValueError::new_err(match path {
                    Some(path) => format!("{}: {NO_FRONTMATTER}", path.display()),
                    None => NO_FRONTMATTER.to_string(),
                }),
                _ => PyValueError::new_err(message),
            },
            ListError::Config(error) => error.into_py_err(py),
            ListError::Filter(error) => error.into_py_err(py),
            ListError::Order(error) => error.into_py_err(py),
            ListError::Field(error) => error.into_py_err(py),
            ListError::Compute { .. } => raised_or(PyValueError::new_err(message)),
            ListError::AlreadyExists { .. } => PyFileExistsError::new_err(message),
            ListError::NonScalar { .. }
            | ListError::MissingPath { .. }
            | ListError::SameDestination { .. } => PyValueError::new_err(message),
        }
    }
}

impl IntoPyErr for QueryError {
    fn into_py_err(self, _: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            QueryError::UnknownQuery { .. } | QueryError::Field { .. } => key_error(message),
            QueryError::Filter { source, .. } => match source {
                FilterError::TypeConflict(_) | FilterError::WrongOperator { .. } => {
                    PyTypeError::new_err(message)
                }
                FilterError::Field(_) => key_error(message),
                _ => PyValueError::new_err(message),
            },
            QueryError::Collection { .. } => raised_or(PyValueError::new_err(message)),
        }
    }
}

impl IntoPyErr for ExportError {
    fn into_py_err(self, py: Python<'_>) -> PyErr {
        let message = self.to_string();
        match self {
            ExportError::Field(error) => error.into_py_err(py),
            ExportError::DuplicateHeader { .. } => PyValueError::new_err(message),
            // In the words every refusal to replace a file uses in Python.
            ExportError::AlreadyExists { path } => PyFileExistsError::new_err(format!(
                "{} already exists, and nothing was written: overwrite=True replaces it",
                path.display()
            )),
            ExportError::ReadOnly { .. } => pyo3::exceptions::PyPermissionError::new_err(message),
            ExportError::IsASample { .. } => PyFileExistsError::new_err(message),
            ExportError::DanglingLink { .. } => {
                pyo3::exceptions::PyFileNotFoundError::new_err(message)
            }
            ExportError::Io { source, .. } => os_error(&source, message),
            ExportError::ChangedSince { .. } => PyOSError::new_err(message),
        }
    }
}

// --------------------------------------------------------------- formulas

/// A formula as the core holds it. A method of the sample it computes is held
/// **weakly**: held strongly, it was a cycle — the sample's Python object holds
/// the Rust sample, which held the bound method, which holds the Python object
/// — that the collector cannot see through Rust, so no such sample was ever
/// freed. Anything else, a function or a lambda, is held as it was given.
pub struct Formula {
    held: Py<PyAny>,
    weak: bool,
}

impl Formula {
    pub fn held(callable: Bound<'_, PyAny>) -> PyResult<Formula> {
        let py = callable.py();
        let of_a_sample = callable
            .getattr("__self__")
            .is_ok_and(|owner| owner.is_instance_of::<python_api::PySample>())
            && callable.hasattr("__func__").unwrap_or(false);
        // A class whose instances take no weak reference is held as before.
        if of_a_sample
            && let Ok(weak) = py
                .import("weakref")?
                .getattr("WeakMethod")?
                .call1((&callable,))
        {
            return Ok(Formula {
                held: weak.unbind(),
                weak: true,
            });
        }
        Ok(Formula {
            held: callable.unbind(),
            weak: false,
        })
    }

    /// The callable, or `ReferenceError` where the sample it was a method of
    /// is gone.
    pub fn bind<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        if !self.weak {
            return Ok(self.held.bind(py).clone());
        }
        let method = self.held.bind(py).call0()?;
        if method.is_none() {
            return Err(pyo3::exceptions::PyReferenceError::new_err(
                "this formula is a method of a sample that no longer exists",
            ));
        }
        Ok(method)
    }
}

/// A Python callable standing for a value. The core holds it as
/// `Rc<dyn Compute>` and never sees the interpreter.
pub struct PythonCompute {
    pub callable: Formula,
}

impl Compute for PythonCompute {
    fn compute(&self) -> Result<Value, ComputeError> {
        let _running = Running::start();
        Python::attach(|py| {
            let result = self
                .callable
                .bind(py)
                .and_then(|callable| callable.call0())
                .map_err(|error| formula_failed(py, error))?;
            one_value(&result).map_err(|error| formula_failed(py, error))
        })
    }
}

/// A Python callable returning a value and its uncertainty from one run.
pub struct PythonQuantityCompute {
    pub callable: Formula,
}

impl ComputeQuantity for PythonQuantityCompute {
    fn compute(&self) -> Result<(Value, Option<Uncertainty>), ComputeError> {
        let _running = Running::start();
        Python::attach(|py| {
            let result = self
                .callable
                .bind(py)
                .and_then(|callable| callable.call0())
                .map_err(|error| formula_failed(py, error))?;
            quantity_of(&result).map_err(|error| formula_failed(py, error))
        })
    }
}

/// A Python callable filling one row's outputs.
pub struct PythonRowCompute {
    pub callable: Formula,
    pub outputs: Vec<Identifier>,
}

impl ComputeRow for PythonRowCompute {
    fn compute(
        &self,
        row: &RowView,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, CellOutput>, ComputeError> {
        let _running = Running::start();
        Python::attach(|py| {
            let run = || -> PyResult<IndexMap<Identifier, CellOutput>> {
                let view = python_api::row_snapshot(py, row)?;
                let callable = self.callable.bind(py)?;
                let result = callable.call1((view,))?;
                outputs_of(&result, &self.outputs, cell_output)
                    .map_err(|error| at_the_formula(&callable, error))
            };
            run().map_err(|error| formula_failed(py, error))
        })
    }
}

/// A Python callable filling whole columns, given the columns it declared.
pub struct PythonColumnCompute {
    pub callable: Formula,
    pub outputs: Vec<Identifier>,
    /// The whole columns the formula names, with their table: those of its own
    /// table are handed to it, those of another it reads through the sample.
    pub columns: Vec<(Identifier, Identifier)>,
}

impl ComputeColumn for PythonColumnCompute {
    fn compute(
        &self,
        columns: &ColumnSet,
        _: &dyn Scope,
    ) -> Result<IndexMap<Identifier, Vec<CellOutput>>, ComputeError> {
        let _running = Running::start();
        Python::attach(|py| {
            let run = || -> PyResult<IndexMap<Identifier, Vec<CellOutput>>> {
                let own: Vec<Identifier> = self
                    .columns
                    .iter()
                    .filter(|(table, _)| table == columns.table_name())
                    .map(|(_, column)| column.clone())
                    .collect();
                let views = python_api::column_snapshots(py, columns, &own)?;
                let callable = self.callable.bind(py)?;
                let result = callable.call1((views,))?;
                outputs_of(&result, &self.outputs, column_output)
                    .map_err(|error| at_the_formula(&callable, error))
            };
            run().map_err(|error| formula_failed(py, error))
        })
    }
}

/// One scalar from a formula promised to return one. A `Property` stands for
/// its value; a list is refused rather than read as a cell.
pub fn one_value(result: &Bound<'_, PyAny>) -> PyResult<Value> {
    if let Some((value, _)) = python_api::property_quantity(result)? {
        return Ok(value);
    }
    if result.is_instance_of::<PyList>() || result.is_instance_of::<PyTuple>() {
        return Err(PyTypeError::new_err(format!(
            "a formula promised one value and returned {}: a list is not one cell",
            a_type(result)
        )));
    }
    from_python(result)
}

/// A joint formula's `(value, uncertainty)`, or a `Property` carrying both.
pub fn quantity_of(result: &Bound<'_, PyAny>) -> PyResult<(Value, Option<Uncertainty>)> {
    if let Some(pair) = python_api::property_quantity(result)? {
        return Ok(pair);
    }
    let pair = result
        .cast::<PyTuple>()
        .ok()
        .filter(|tuple| tuple.len() == 2)
        .ok_or_else(|| {
            PyTypeError::new_err(format!(
                "a joint formula returns (value, uncertainty), and this one returned {}",
                a_type(result)
            ))
        })?;
    Ok((
        one_value(&pair.get_item(0)?)?,
        uncertainty_from_python(&pair.get_item(1)?)?,
    ))
}

/// One cell a formula gave. A pair is refused naming the form that carries
/// an uncertainty, since `(value, uncertainty)` is what a property's joint
/// formula returns and the natural thing to try here.
fn cell_output_of(object: &Bound<'_, PyAny>, formula: &str) -> PyResult<CellOutput> {
    if let Some((value, uncertainty)) = python_api::property_quantity(object)? {
        return Ok(CellOutput::Quantity(value, uncertainty));
    }
    if object.is_instance_of::<PyList>() || object.is_instance_of::<PyTuple>() {
        return Err(PyTypeError::new_err(format!(
            "{formula}, or sk.Property(value=…, uncertainty=…) for a value with its \
             uncertainty, and this one returned {}",
            a_type(object)
        )));
    }
    Ok(CellOutput::Value(one_value(object)?))
}

fn cell_output(object: &Bound<'_, PyAny>) -> PyResult<CellOutput> {
    cell_output_of(object, "a row formula returns one value")
}

fn column_output(object: &Bound<'_, PyAny>) -> PyResult<Vec<CellOutput>> {
    if object.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "a column formula returns one sequence of cells per output, and a str is not one",
        ));
    }
    let mut cells = Vec::new();
    for item in object.try_iter()? {
        cells.push(cell_output_of(
            &item?,
            "a column formula returns one value per cell",
        )?);
    }
    Ok(cells)
}

/// What a formula returned, refused at the formula: its name and where it is
/// written. The refusal is raised by SampleKit reading the result, so its
/// traceback ends in the package, and it named nothing of the model.
fn at_the_formula(callable: &Bound<'_, PyAny>, error: PyErr) -> PyErr {
    let py = callable.py();
    match formula_place(callable) {
        Some(place) => PyErr::from_type(
            error.get_type(py),
            format!(
                "{place} returned what cannot be stored: {}",
                error.value(py)
            ),
        ),
        None => error,
    }
}

/// `Brew._apparent (model/brew.py, line 122)`, where the callable says.
fn formula_place(callable: &Bound<'_, PyAny>) -> Option<String> {
    let function = callable
        .getattr("__func__")
        .unwrap_or_else(|_| callable.clone());
    let name: String = function.getattr("__qualname__").ok()?.extract().ok()?;
    let code = function.getattr("__code__").ok()?;
    let file: String = code.getattr("co_filename").ok()?.extract().ok()?;
    let line: usize = code.getattr("co_firstlineno").ok()?.extract().ok()?;
    Some(format!("{name} ({file}, line {line})"))
}

/// What a table formula returned, keyed by output: a mapping keyed by every
/// output, or — for one output — the one result itself. Which outputs are
/// missing or surplus is the table's to say.
fn outputs_of<T>(
    result: &Bound<'_, PyAny>,
    outputs: &[Identifier],
    convert: fn(&Bound<'_, PyAny>) -> PyResult<T>,
) -> PyResult<IndexMap<Identifier, T>> {
    let py = result.py();
    if let Ok(mapping) = result.cast::<PyDict>() {
        let mut produced = IndexMap::new();
        for (key, item) in mapping.iter() {
            let name: String = key.extract()?;
            produced.insert(identifier_from(py, &name)?, convert(&item)?);
        }
        return Ok(produced);
    }
    if outputs.len() != 1 {
        let names: Vec<&str> = outputs.iter().map(Identifier::as_str).collect();
        return Err(PyTypeError::new_err(format!(
            "a formula filling [{}] returns a dict keyed by each of them",
            names.join(", ")
        )));
    }
    Ok([(outputs[0].clone(), convert(result)?)]
        .into_iter()
        .collect())
}

// ------------------------------------------------------------ one sample

/// One sample, shared by every wrapper of it: a `Sample`, its `Property`
/// objects, its tables' cells and the lists bound to its attributes.
#[derive(Clone)]
pub struct SharedSample {
    pub inner: Rc<RefCell<Sample>>,
    epochs: Rc<RefCell<HashMap<Identifier, u64>>>,
    snapshot: Rc<RefCell<Option<Rc<Snapshot>>>>,
}

/// What a formula reads of its sample while the sample is borrowed to write
/// the cells formulas fill. Handles are shared, so a property read through it
/// is the property itself.
pub struct Snapshot {
    pub properties: IndexMap<Identifier, PropertyHandle>,
    pub attributes: IndexMap<Identifier, AttributeValue>,
    pub tables: Vec<Identifier>,
    /// The tables a derivation of the sample reads, copied as they stand once
    /// resolved: what a formula reads of them through the sample.
    pub copies: IndexMap<Identifier, Rc<crate::core::table::Table>>,
    /// The sample's name — written, else its file's — which a table formula
    /// may read as it reads an attribute: refused, a model rebuilt it from
    /// the file's path.
    pub name: Option<String>,
}

/// Every table some derivation of the sample reads, copied.
fn copies_of(sample: &Sample) -> IndexMap<Identifier, Rc<crate::core::table::Table>> {
    let mut copies = IndexMap::new();
    for name in sample.table_names() {
        let Ok(table) = sample.table(name) else {
            continue;
        };
        for read in table.foreign_tables() {
            if copies.contains_key(&read) {
                continue;
            }
            if let Ok(held) = sample.table(&read)
                && let Ok(copy) = crate::format::schema::table_copy(held)
            {
                copies.insert(read, Rc::new(copy));
            }
        }
    }
    copies
}

impl Snapshot {
    fn of(sample: &Sample) -> Snapshot {
        Snapshot {
            properties: sample
                .property_names()
                .into_iter()
                .map(|name| {
                    let handle = sample.property(name).expect("just enumerated");
                    (name.clone(), handle)
                })
                .collect(),
            attributes: sample
                .attribute_names()
                .into_iter()
                .map(|name| {
                    let value = sample.attribute(name).expect("just enumerated").clone();
                    (name.clone(), value)
                })
                .collect(),
            tables: sample.table_names().into_iter().cloned().collect(),
            copies: copies_of(sample),
            name: sample.name().map(str::to_string),
        }
    }
}

impl SharedSample {
    pub fn new(sample: Sample) -> SharedSample {
        SharedSample::from_rc(Rc::new(RefCell::new(sample)))
    }

    pub fn from_rc(inner: Rc<RefCell<Sample>>) -> SharedSample {
        SharedSample {
            inner,
            epochs: Rc::new(RefCell::new(HashMap::new())),
            snapshot: Rc::new(RefCell::new(None)),
        }
    }

    pub fn same(&self, other: &SharedSample) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// Which binding of `name` a bound list belongs to.
    pub fn epoch(&self, name: &Identifier) -> u64 {
        self.epochs.borrow().get(name).copied().unwrap_or(0)
    }

    /// Detaches every list bound to `name` before now.
    pub fn advance(&self, name: &Identifier) {
        *self.epochs.borrow_mut().entry(name.clone()).or_insert(0) += 1;
    }

    /// A read: the sample itself, or the snapshot a formula reads while tables
    /// resolve.
    pub fn read<T>(
        &self,
        from_sample: impl FnOnce(&Sample) -> PyResult<T>,
        from_snapshot: impl FnOnce(&Snapshot) -> PyResult<T>,
    ) -> PyResult<T> {
        match self.inner.try_borrow() {
            Ok(sample) => from_sample(&sample),
            Err(_) => {
                let snapshot = self.snapshot.borrow().clone();
                match snapshot {
                    Some(snapshot) => from_snapshot(&snapshot),
                    None => Err(PyRuntimeError::new_err(
                        "this sample is being written and cannot be read until that ends",
                    )),
                }
            }
        }
    }

    /// A read that needs the sample itself — a table — and says why when a
    /// formula asks for one while tables resolve.
    pub fn read_sample<T>(
        &self,
        what: &str,
        read: impl FnOnce(&Sample) -> PyResult<T>,
    ) -> PyResult<T> {
        match self.inner.try_borrow() {
            Ok(sample) => read(&sample),
            Err(_) => Err(PyRuntimeError::new_err(format!(
                "{what} cannot be read while this sample's tables resolve: a table formula \
                 reads its row and the sample's properties and attributes, never a table"
            ))),
        }
    }

    /// A table of the sample, or, while its tables resolve, the copy of one a
    /// derivation reads.
    pub fn read_table<T>(
        &self,
        name: &Identifier,
        read: impl FnOnce(&crate::core::table::Table) -> PyResult<T>,
    ) -> PyResult<T> {
        if let Ok(sample) = self.inner.try_borrow() {
            let table = sample
                .table(name)
                .map_err(|error| key_error(error.to_string()))?;
            return read(table);
        }
        let copy = self
            .snapshot
            .borrow()
            .as_ref()
            .and_then(|snapshot| snapshot.copies.get(name).cloned());
        match copy {
            Some(table) => read(&table),
            None => Err(PyRuntimeError::new_err(format!(
                "the table '{name}' cannot be read while this sample's tables resolve: a table \
                 formula reads its row or its columns, the sample's properties and attributes, \
                 and the tables its derivations name"
            ))),
        }
    }

    pub fn write<T>(&self, write: impl FnOnce(&mut Sample) -> PyResult<T>) -> PyResult<T> {
        let mut sample = self.inner.try_borrow_mut().map_err(|_| {
            PyRuntimeError::new_err(
                "this sample cannot be changed while one of its formulas is running",
            )
        })?;
        write(&mut sample)
    }

    /// Work that runs formulas while the sample is borrowed to write what they
    /// fill, every read meanwhile answered from a snapshot.
    pub fn resolving<T>(&self, work: impl FnOnce(&mut Sample) -> T) -> PyResult<T> {
        let busy = || {
            PyRuntimeError::new_err(
                "this sample is already being resolved: a formula cannot resolve its own sample",
            )
        };
        let snapshot = {
            let sample = self.inner.try_borrow().map_err(|_| busy())?;
            Rc::new(Snapshot::of(&sample))
        };
        let previous = self.snapshot.replace(Some(snapshot));
        let outcome = match self.inner.try_borrow_mut() {
            Ok(mut sample) => Ok(work(&mut sample)),
            Err(_) => Err(busy()),
        };
        self.snapshot.replace(previous);
        outcome
    }
}

// ------------------------------------------------------------ the GIL

/// Work carried across `Python::detach`, which runs it on this same thread.
pub struct Unshared<T>(pub T);

// SAFETY: `Python::detach` calls its closure on the calling thread and returns
// on it. What is wrapped here holds samples, which are `unsendable` and so are
// never reachable from another thread; nothing is sent anywhere.
unsafe impl<T> Send for Unshared<T> {}

/// Rust-side work with the GIL released, so that other Python threads advance
/// while a directory loads or a collection sorts. A formula met during it
/// reacquires the GIL.
pub fn without_gil<T>(py: Python<'_>, work: impl FnOnce() -> T) -> T {
    let work = Unshared(work);
    py.detach(move || {
        let work = work;
        Unshared((work.0)())
    })
    .0
}

// ------------------------------------------------------------ the package

static SAMPLE_CLASS: OnceLock<Py<PyAny>> = OnceLock::new();
static BOUND_LIST: OnceLock<Py<PyAny>> = OnceLock::new();

/// The package's `Sample` class and `BoundList` factory, handed over as the
/// package imports.
pub fn register(sample_class: Py<PyAny>, bound_list: Py<PyAny>) -> PyResult<()> {
    let _ = SAMPLE_CLASS.set(sample_class);
    let _ = BOUND_LIST.set(bound_list);
    Ok(())
}

fn registered<'py>(py: Python<'py>, slot: &OnceLock<Py<PyAny>>) -> PyResult<Bound<'py, PyAny>> {
    slot.get()
        .map(|class| class.bind(py).clone())
        .ok_or_else(|| {
            PyRuntimeError::new_err("samplekit._native was imported without the samplekit package")
        })
}

pub fn sample_class(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    registered(py, &SAMPLE_CLASS)
}

pub fn bound_list_factory(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    registered(py, &BOUND_LIST)
}

// --------------------------------------------------------------- models

type ModelKey = (PathBuf, Option<String>);

fn models() -> &'static Mutex<HashMap<ModelKey, Py<PyAny>>> {
    static MODELS: OnceLock<Mutex<HashMap<ModelKey, Py<PyAny>>>> = OnceLock::new();
    MODELS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The class a project's `[model]` declares, imported once per file per
/// process. No consent is asked: a script is already code.
pub fn load_model(py: Python<'_>, config: &ProjectConfig) -> PyResult<Option<Py<PyAny>>> {
    let Some(declared) = config.model() else {
        return Ok(None);
    };
    let path = config.resolve(&declared.path.to_string_lossy());
    if !path.is_file() {
        return Err(PyFileNotFoundError::new_err(format!(
            "the model {} declared by {} does not exist",
            path.display(),
            config.root().join(config::FILENAME).display()
        )));
    }
    load_model_at(py, &path, declared.class.as_deref(), config.root()).map(Some)
}

/// The class a model file declares, imported once per file per process: a
/// project's, or the one the command line's worker names.
pub fn load_model_at(
    py: Python<'_>,
    path: &Path,
    class: Option<&str>,
    root: &Path,
) -> PyResult<Py<PyAny>> {
    // One file, one key, however it was reached: `ana/brews` and
    // `tom/brews` both naming `../../models/brew.py` imported the
    // model twice, two classes where one is promised.
    let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let key = (canonical, class.map(str::to_string));
    if let Some(held) = models()
        .lock()
        .expect("the model cache is never poisoned")
        .get(&key)
    {
        return Ok(held.clone_ref(py));
    }
    if !path.is_file() {
        return Err(PyFileNotFoundError::new_err(format!(
            "the model {} does not exist",
            path.display()
        )));
    }
    let module = import_file(py, path, root)?;
    let found = class_of(py, &module, path, class)?;
    models()
        .lock()
        .expect("the model cache is never poisoned")
        .insert(key, found.clone_ref(py));
    Ok(found)
}

/// Imports a model by path. The file's directory and the project root go on
/// `sys.path`, so a model imports its neighbours as a script would; a file the
/// root can name as a module is imported under that name, so that a script
/// importing it too gets the same class.
fn import_file<'py>(py: Python<'py>, path: &Path, root: &Path) -> PyResult<Bound<'py, PyAny>> {
    let sys = py.import("sys")?;
    let search = sys.getattr("path")?;
    for directory in [path.parent(), Some(root)].into_iter().flatten() {
        let text = directory.to_string_lossy().into_owned();
        if !search.contains(&text)? {
            search.call_method1("insert", (0, text))?;
        }
    }
    // Imported under that name only when the name is this file: another
    // project's `model.py` may already hold it.
    if let Some(dotted) = module_name(path, root) {
        let held = sys
            .getattr("modules")?
            .call_method1("get", (dotted.as_str(),))?;
        if held.is_none() || same_origin(&held, path)? {
            let module = py.import(dotted.as_str())?.into_any();
            if same_origin(&module, path)? {
                return Ok(module);
            }
        }
    }
    static FALLBACK: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "_samplekit_model_{}",
        FALLBACK.fetch_add(1, Ordering::Relaxed)
    );
    let util = py.import("importlib.util")?;
    let spec = util.call_method1("spec_from_file_location", (&name, path))?;
    let module = util.call_method1("module_from_spec", (&spec,))?;
    sys.getattr("modules")?.set_item(&name, &module)?;
    spec.getattr("loader")?
        .call_method1("exec_module", (&module,))?;
    Ok(module)
}

/// Whether a module was imported from this file.
fn same_origin(module: &Bound<'_, PyAny>, path: &Path) -> PyResult<bool> {
    let Some(file) = module.getattr_opt("__file__")? else {
        return Ok(false);
    };
    let Ok(file) = file.extract::<String>() else {
        return Ok(false);
    };
    Ok(
        match (dunce::canonicalize(&file), dunce::canonicalize(path)) {
            (Ok(left), Ok(right)) => left == right,
            _ => false,
        },
    )
}

/// `scripts/brew_model.py` under the root is `scripts.brew_model`, when
/// every part is a Python name.
fn module_name(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?.with_extension("");
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let valid = !parts.is_empty()
        && parts
            .iter()
            .all(|part| Identifier::new(part).is_ok_and(|name| name.is_python_attribute()));
    valid.then(|| parts.join("."))
}

/// `[model] class`, or the one subclass of `Sample` the file defines.
fn class_of(
    py: Python<'_>,
    module: &Bound<'_, PyAny>,
    path: &Path,
    declared: Option<&str>,
) -> PyResult<Py<PyAny>> {
    let base = sample_class(py)?;
    let module_name: String = module.getattr("__name__")?.extract()?;
    let mut classes: Vec<(String, Bound<'_, PyAny>)> = Vec::new();
    for (name, value) in module.getattr("__dict__")?.cast::<PyDict>()?.iter() {
        if !value.is_instance_of::<PyType>() {
            continue;
        }
        let defined_here = value
            .getattr("__module__")
            .and_then(|owner| owner.extract::<String>())
            .is_ok_and(|owner| owner == module_name);
        if defined_here {
            classes.push((name.extract()?, value));
        }
    }
    let is_sample = |class: &Bound<'_, PyAny>| -> PyResult<bool> {
        Ok(class.is_instance_of::<PyType>() && class.cast::<PyType>()?.is_subclass(&base)?)
    };
    match declared {
        Some(wanted) => {
            let Some(class) = module.getattr_opt(wanted)? else {
                let names: Vec<&str> = classes.iter().map(|(name, _)| name.as_str()).collect();
                let mut message = format!("{} defines no class '{wanted}'", path.display());
                if let Some(nearest) = identifier::nearest(wanted, names.iter().copied()) {
                    message.push_str(&format!("\n  did you mean: {nearest}?"));
                }
                message.push_str(&format!("\n  classes there: {}", names.join(", ")));
                return Err(PyAttributeError::new_err(message));
            };
            if !is_sample(&class)? {
                return Err(PyTypeError::new_err(format!(
                    "'{wanted}' in {} is not a Sample: a model is a subclass of samplekit.Sample",
                    path.display()
                )));
            }
            Ok(class.unbind())
        }
        None => {
            let mut candidates = Vec::new();
            for (name, class) in classes {
                if is_sample(&class)? {
                    candidates.push((name, class));
                }
            }
            match candidates.len() {
                1 => Ok(candidates.remove(0).1.unbind()),
                0 => Err(PyValueError::new_err(format!(
                    "{} defines no subclass of Sample to be the model",
                    path.display()
                ))),
                _ => {
                    let names: Vec<&str> =
                        candidates.iter().map(|(name, _)| name.as_str()).collect();
                    Err(PyValueError::new_err(format!(
                        "{} defines several subclasses of Sample: {}\n  name one with \
                         [model] class = \"{}\"",
                        path.display(),
                        names.join(", "),
                        names[0]
                    )))
                }
            }
        }
    }
}
