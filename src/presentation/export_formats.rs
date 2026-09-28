//! Turning a rectangle of values into CSV, TSV or JSON. Every format reads the
//! same `Dataset`, which is what makes a CSV and a JSON generated from one
//! profile contain the same values rather than merely similar ones.

use crate::config::project_config::Format;
use crate::core::value::Value;
use indexmap::IndexMap;

/// A rectangle: one header per emitted column, one cell per header, the same
/// count on every row.
///
/// Cells are typed and not pre-rendered strings, because absence and lists must
/// survive to the serializer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Dataset {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
    /// Each column's unit where every row agrees, which CSV and TSV write
    /// beside the header and JSON inside a quantity. Empty when unknown.
    pub units: Vec<Option<String>>,
    /// Columns holding one quantity's two halves, which JSON writes as one
    /// object.
    pub quantities: Vec<QuantityColumns>,
}

/// Where a quantity's value and uncertainty sit in a dataset's columns.
#[derive(Debug, Clone, PartialEq)]
pub struct QuantityColumns {
    /// The key JSON writes it under: the field, or its declared header.
    pub key: String,
    pub value: usize,
    pub uncertainty: Option<usize>,
    /// Each row's state, `current`, `outdated`, `edited`, `failed`…, which JSON
    /// writes beside the value. Empty when unknown.
    pub states: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Scalar(Option<Value>),
    List(Vec<Value>),
    /// A tag set: an array in JSON, joined by `;` in CSV and TSV.
    Tags(Vec<String>),
    /// A number and its text at the declared precision, written as the text in
    /// every format — `1.23e+01`, `0.000` — which a CSV reader and a JSON
    /// parser both read as the number.
    Written {
        value: Value,
        text: String,
    },
}

impl From<Option<Value>> for Cell {
    fn from(value: Option<Value>) -> Cell {
        Cell::Scalar(value)
    }
}

/// LF on every platform. A CRLF option exists for CSV consumed by older Windows
/// tooling, and it is a declared setting rather than a platform default, so a
/// file exported on two machines is identical.
pub fn serialize(dataset: &Dataset, format: Format) -> Result<String, ExportFormatError> {
    serialize_with(dataset, format, false)
}

pub fn serialize_with(
    dataset: &Dataset,
    format: Format,
    crlf: bool,
) -> Result<String, ExportFormatError> {
    let text = match format {
        Format::Csv => separated(dataset, ',')?,
        Format::Tsv => separated(dataset, '\t')?,
        Format::Json => json(dataset),
    };
    if crlf && !matches!(format, Format::Json) {
        return Ok(text.replace('\n', "\r\n"));
    }
    Ok(text)
}

/// The dataset without its uncertainty columns empty in every row, and the
/// headers of those left out, for the caller to say. A dataset of no rows keeps
/// them: nothing says they are empty.
pub fn without_empty_uncertainties(dataset: &Dataset) -> (Dataset, Vec<String>) {
    let empty: Vec<usize> = dataset
        .quantities
        .iter()
        .filter_map(|quantity| quantity.uncertainty)
        .filter(|at| {
            !dataset.rows.is_empty()
                && dataset.rows.iter().all(|row| {
                    matches!(
                        row.get(*at),
                        None | Some(Cell::Scalar(None | Some(Value::Absent)))
                    )
                })
        })
        .collect();
    if empty.is_empty() {
        return (dataset.clone(), Vec::new());
    }
    let kept: Vec<usize> = (0..dataset.headers.len())
        .filter(|at| !empty.contains(at))
        .collect();
    let moved = |old: usize| kept.iter().position(|at| *at == old);
    let trimmed = Dataset {
        headers: kept.iter().map(|at| dataset.headers[*at].clone()).collect(),
        rows: dataset
            .rows
            .iter()
            .map(|row| {
                kept.iter()
                    .map(|at| row.get(*at).cloned().unwrap_or(Cell::Scalar(None)))
                    .collect()
            })
            .collect(),
        units: if dataset.units.is_empty() {
            Vec::new()
        } else {
            kept.iter()
                .map(|at| dataset.units.get(*at).cloned().flatten())
                .collect()
        },
        quantities: dataset
            .quantities
            .iter()
            .filter_map(|quantity| {
                Some(QuantityColumns {
                    key: quantity.key.clone(),
                    value: moved(quantity.value)?,
                    uncertainty: quantity.uncertainty.and_then(moved),
                    states: quantity.states.clone(),
                })
            })
            .collect(),
    };
    let dropped = empty
        .iter()
        .map(|at| dataset.headers[*at].clone())
        .collect();
    (trimmed, dropped)
}

pub fn delimiter(format: Format) -> Option<char> {
    match format {
        Format::Csv => Some(','),
        Format::Tsv => Some('\t'),
        Format::Json => None,
    }
}

pub fn extension(format: Format) -> &'static str {
    match format {
        Format::Csv => "csv",
        Format::Tsv => "tsv",
        Format::Json => "json",
    }
}

// -------------------------------------------------------------- delimited

/// CSV or TSV by the `csv` crate, RFC 4180's quoting for both: a field holding
/// the delimiter, a quote or a newline is quoted, inner quotes doubled — TSV
/// has no standard, and quoting is what spreadsheet software accepts. LF ends
/// each line.
fn separated(dataset: &Dataset, delimiter: char) -> Result<String, ExportFormatError> {
    let mut writer = csv::WriterBuilder::new()
        .delimiter(u8::try_from(delimiter).unwrap_or(b','))
        .terminator(csv::Terminator::Any(b'\n'))
        .quote_style(csv::QuoteStyle::Necessary)
        .from_writer(Vec::new());
    // A unit beside its header, `hops [g/L]`.
    let headers: Vec<String> = dataset
        .headers
        .iter()
        .enumerate()
        .map(|(at, header)| match dataset.units.get(at) {
            Some(Some(unit)) => format!("{header} [{unit}]"),
            _ => header.clone(),
        })
        .collect();
    // Writing to memory fails only on a record of another length, which a
    // dataset never holds.
    let _ = writer.write_record(&headers);
    for row in &dataset.rows {
        let cells: Vec<String> = row
            .iter()
            .map(|cell| match cell {
                // An empty field, never `-`, never `N/A`: each of those is a
                // *value* to a parser, and pandas turns a numeric column into
                // an object column on the strength of one dash.
                Cell::Scalar(None | Some(Value::Absent)) => String::new(),
                Cell::Scalar(Some(value)) => flat(value),
                Cell::Written { text, .. } => text.clone(),
                // A list — an attribute's items, a quantity's readings — as
                // tags are: joined by `;`.
                Cell::List(values) => joined(values.iter().map(flat)),
                Cell::Tags(tags) => joined(tags.iter().cloned()),
            })
            .collect();
        let _ = writer.write_record(&cells);
    }
    let bytes = writer.into_inner().unwrap_or_default();
    Ok(String::from_utf8(bytes).unwrap_or_default())
}

/// Items in one field: `;` between them, and a `;` or `\` inside one escaped,
/// so that the list reads back whole.
fn joined(items: impl Iterator<Item = String>) -> String {
    items
        .map(|item| item.replace('\\', "\\\\").replace(';', "\\;"))
        .collect::<Vec<_>>()
        .join(";")
}

/// Nothing a dataset holds is refused any longer: a list is joined as tags are.
/// Kept as the result's error so that a format with a refusal of its own has a
/// place for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportFormatError {}

impl std::fmt::Display for ExportFormatError {
    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {}
    }
}

impl std::error::Error for ExportFormatError {}

// ------------------------------------------------------------------- json

/// An array of objects, one per sample, keys in column order and **flat**.
/// Absent values are `null` and *present*: a consumer iterating keys must find
/// the same keys in every object, or a missing measurement becomes a missing
/// field and *not measured* stops being distinguishable from *not in this
/// export*.
fn json(dataset: &Dataset) -> String {
    let rows: Vec<IndexMap<String, Json>> = dataset
        .rows
        .iter()
        .enumerate()
        .map(|(at, row)| {
            let mut entries = IndexMap::new();
            for (index, header) in dataset.headers.iter().enumerate() {
                // A quantity is one object, `{value, uncertainty, unit}` : its
                // uncertainty column is written inside it.
                if dataset
                    .quantities
                    .iter()
                    .any(|quantity| quantity.uncertainty == Some(index))
                {
                    continue;
                }
                match dataset
                    .quantities
                    .iter()
                    .find(|quantity| quantity.value == index)
                {
                    Some(quantity) => {
                        let mut object = IndexMap::new();
                        object.insert("value".to_string(), json_value(row.get(quantity.value)));
                        object.insert(
                            "uncertainty".to_string(),
                            quantity
                                .uncertainty
                                .map_or(Json::Plain(serde_json::Value::Null), |at| {
                                    json_value(row.get(at))
                                }),
                        );
                        object.insert(
                            "unit".to_string(),
                            Json::Plain(
                                dataset
                                    .units
                                    .get(quantity.value)
                                    .cloned()
                                    .flatten()
                                    .map_or(serde_json::Value::Null, serde_json::Value::String),
                            ),
                        );
                        if let Some(state) = quantity.states.get(at).cloned().flatten() {
                            object.insert(
                                "state".to_string(),
                                Json::Plain(serde_json::Value::String(state)),
                            );
                        }
                        entries.insert(quantity.key.clone(), Json::Object(object));
                    }
                    None => {
                        entries.insert(header.clone(), json_value(row.get(index)));
                    }
                }
            }
            entries
        })
        .collect();
    // Writing to a string fails only on a map key that is not a string.
    let mut written = serde_json::to_string_pretty(&rows).unwrap_or_default();
    written.push('\n');
    written
}

/// A value as JSON is written, by `serde_json`: a number kept as the file
/// writes it — `1.230` stays `1.230` — or any other value.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum Json {
    Raw(Box<serde_json::value::RawValue>),
    Plain(serde_json::Value),
    Object(IndexMap<String, Json>),
}

fn json_value(cell: Option<&Cell>) -> Json {
    match cell {
        None | Some(Cell::Scalar(None | Some(Value::Absent))) => {
            Json::Plain(serde_json::Value::Null)
        }
        Some(Cell::Scalar(Some(value))) => Json::Plain(json_scalar(value)),
        // The text, where it is a JSON number; the number otherwise.
        Some(Cell::Written { value, text }) => {
            match serde_json::value::RawValue::from_string(text.clone()) {
                Ok(raw) if serde_json::from_str::<serde_json::Number>(text).is_ok() => {
                    Json::Raw(raw)
                }
                _ => Json::Plain(json_scalar(value)),
            }
        }
        Some(Cell::Tags(tags)) => Json::Plain(serde_json::Value::Array(
            tags.iter()
                .map(|tag| serde_json::Value::String(tag.clone()))
                .collect(),
        )),
        Some(Cell::List(values)) => Json::Plain(serde_json::Value::Array(
            values.iter().map(json_scalar).collect(),
        )),
    }
}

/// A number with its point: `13.0` stays `13.0`, so that a reader does not take
/// it for an integer and a column does not change kind from row to row. A
/// number JSON cannot hold, infinite or not a number, is `null`.
fn json_scalar(value: &Value) -> serde_json::Value {
    match value {
        Value::Absent | Value::NotApplicable => serde_json::Value::Null,
        Value::Integer(integer) => serde_json::Value::from(*integer),
        Value::Number(number) => serde_json::Value::from(*number),
        Value::Boolean(boolean) => serde_json::Value::Bool(*boolean),
        Value::Text(text) => serde_json::Value::String(text.clone()),
        Value::Date(date) => serde_json::Value::String(date.iso()),
        Value::DateTime(date_time) => serde_json::Value::String(date_time.iso()),
    }
}

// ----------------------------------------------------------------- values

/// A number with its point: `13.0` stays `13.0`, so that a reader does not take it
/// for an integer and a column does not change kind from row to row.
fn decimal(number: f64) -> String {
    let written = number.to_string();
    if written.contains(['.', 'e', 'E']) || !number.is_finite() {
        written
    } else {
        format!("{written}.0")
    }
}

/// A cell's text. Numbers carry no thousands separator and no locale: a
/// comma-separated file with comma decimal separators is unparseable, and a
/// locale-dependent export is irreproducible across machines.
fn flat(value: &Value) -> String {
    match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => decimal(*number),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        // Text beginning with `=`, `+`, `-` or `@` is prefixed with a single
        // quote: spreadsheet applications read those as formulas, and a sample
        // named `-control` becoming a broken formula is the mildest outcome of
        // a known injection class. A *number* is never prefixed, which is why
        // this arm is text only.
        Value::Text(text) => match text.chars().next() {
            Some('=') | Some('+') | Some('-') | Some('@') => format!("'{text}"),
            _ => text.clone(),
        },
        // An empty field, as absence is: a data file holds no word for it.
        Value::Absent => String::new(),
        // Written as the file writes it: an empty field read as a value nobody
        // measured, where this one was answered.
        Value::NotApplicable => "n/a".to_string(),
    }
}
