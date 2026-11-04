//! Turning a rectangle of values into CSV, TSV or JSON. Every format reads the
//! same `Dataset`, which is what makes a CSV and a JSON generated from one
//! profile contain the same values rather than merely similar ones.

use crate::config::project_config::{Escape, Format, TextFormat};
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
    /// Columns a template writes, which CSV and TSV write as one column of
    /// its text, and JSON as their numbers.
    pub templated: Vec<Templated>,
    /// Each column's title, as the terminal gives it — a text format heads
    /// its columns with them; empty where unknown, the headers standing in.
    pub labels: Vec<String>,
    /// The columns a profile groups by: a text format marks where their
    /// values change.
    pub group_columns: Vec<usize>,
    /// A column's key in JSON, by its header, where it is not the header: a
    /// channel named alone, `modulus.u`, is keyed as it is named, as a quantity
    /// whole (`radius`) and its readings (`radius.readings`) are — its CSV header,
    /// `modulus_uncertainty`, keyed it apart from them.
    pub json_keys: std::collections::BTreeMap<String, String>,
}

/// A column a template writes: the dataset's columns it stands for, its
/// header, and each row's text — `None` where the row holds no value.
#[derive(Debug, Clone, PartialEq)]
pub struct Templated {
    pub columns: Vec<usize>,
    pub header: String,
    pub texts: Vec<Option<String>>,
}

/// The dataset as a delimited file writes it: each templated column's
/// columns replaced by one of its text, under its header, with no unit beside
/// it — the template places the unit where it says.
fn with_templates(dataset: &Dataset) -> Dataset {
    if dataset.templated.is_empty() {
        return dataset.clone();
    }
    let mut headers = Vec::new();
    let mut units = Vec::new();
    let mut rows: Vec<Vec<Cell>> = vec![Vec::new(); dataset.rows.len()];
    let mut quantities = Vec::new();
    let mut at = 0;
    while at < dataset.headers.len() {
        if let Some(templated) = dataset
            .templated
            .iter()
            .find(|templated| templated.columns.first() == Some(&at))
        {
            headers.push(templated.header.clone());
            units.push(None);
            for (row, text) in rows.iter_mut().zip(&templated.texts) {
                row.push(Cell::Scalar(text.clone().map(Value::text)));
            }
            at += templated.columns.len().max(1);
            continue;
        }
        let moved = headers.len();
        headers.push(dataset.headers[at].clone());
        units.push(dataset.units.get(at).cloned().flatten());
        for (row, held) in rows.iter_mut().zip(&dataset.rows) {
            row.push(held.get(at).cloned().unwrap_or(Cell::Scalar(None)));
        }
        for quantity in &dataset.quantities {
            if quantity.uncertainty.unwrap_or(quantity.value) == at {
                let mut quantity = quantity.clone();
                let shift = at - moved;
                quantity.value -= shift;
                quantity.uncertainty = quantity.uncertainty.map(|column| column - shift);
                quantities.push(quantity);
            }
        }
        at += 1;
    }
    Dataset {
        headers,
        rows,
        units: if dataset.units.is_empty() {
            Vec::new()
        } else {
            units
        },
        quantities,
        templated: Vec::new(),
        labels: Vec::new(),
        group_columns: Vec::new(),
        json_keys: dataset.json_keys.clone(),
    }
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

/// Rectangles of the same columns, one after another: a column one of them
/// holds and another lacks, an uncertainty one row has, is the stack's, empty
/// where a rectangle has none. Each column's unit, title and quantity are the
/// first rectangle's to give them.
pub fn stacked(parts: &[Dataset]) -> Dataset {
    let mut headers: Vec<String> = Vec::new();
    for part in parts {
        let mut after: Option<usize> = None;
        for header in &part.headers {
            match headers.iter().position(|held| held == header) {
                Some(at) => after = Some(at),
                None => {
                    let at = after.map_or(headers.len(), |after| after + 1);
                    headers.insert(at, header.clone());
                    after = Some(at);
                }
            }
        }
    }
    let at = |header: &str| headers.iter().position(|held| held == header);
    let mut units = vec![None; headers.len()];
    let mut labels = vec![String::new(); headers.len()];
    let mut rows = Vec::new();
    let mut quantities: Vec<QuantityColumns> = Vec::new();
    let mut json_keys = std::collections::BTreeMap::new();
    for part in parts {
        for (column, header) in part.headers.iter().enumerate() {
            let Some(to) = at(header) else { continue };
            if units[to].is_none() {
                units[to] = part.units.get(column).cloned().flatten();
            }
            if labels[to].is_empty() {
                labels[to] = part.labels.get(column).cloned().unwrap_or_default();
            }
        }
        json_keys.extend(part.json_keys.clone());
        for (row_at, row) in part.rows.iter().enumerate() {
            let mut cells = vec![Cell::Scalar(None); headers.len()];
            for (column, cell) in row.iter().enumerate() {
                if let Some(to) = part.headers.get(column).and_then(|header| at(header)) {
                    cells[to] = cell.clone();
                }
            }
            let row_number = rows.len();
            rows.push(cells);
            for quantity in &part.quantities {
                let value = at(&part.headers[quantity.value]);
                let Some(value) = value else { continue };
                let uncertainty = quantity
                    .uncertainty
                    .and_then(|column| at(&part.headers[column]));
                let state = quantity.states.get(row_at).cloned().flatten();
                let held = match quantities.iter_mut().find(|held| held.key == quantity.key) {
                    Some(held) => held,
                    None => {
                        quantities.push(QuantityColumns {
                            key: quantity.key.clone(),
                            value,
                            uncertainty,
                            states: Vec::new(),
                        });
                        quantities.last_mut().expect("just pushed")
                    }
                };
                if held.uncertainty.is_none() {
                    held.uncertainty = uncertainty;
                }
                held.states.resize(row_number, None);
                held.states.push(state);
            }
        }
    }
    for quantity in &mut quantities {
        quantity.states.resize(rows.len(), None);
        if quantity.states.iter().all(Option::is_none) {
            quantity.states.clear();
        }
    }
    Dataset {
        headers,
        rows,
        units,
        quantities,
        templated: Vec::new(),
        labels,
        group_columns: Vec::new(),
        json_keys,
    }
}

/// LF on every platform. A CRLF option exists for CSV consumed by older Windows
/// tooling, and it is a declared setting rather than a platform default, so a
/// file exported on two machines is identical.
pub fn serialize(dataset: &Dataset, format: Format) -> Result<String, ExportFormatError> {
    serialize_with(dataset, format, false)
}

/// A table as a text format writes it — see [`TextFormat`]: the lines before
/// it, its header, a rule, each row, a line between two groups, the lines
/// after it, each ended by a newline.
pub fn text(dataset: &Dataset, format: &TextFormat) -> String {
    // The columns as the format writes them: a template's columns as one, and
    // a quantity's two as one where the format joins them.
    struct Column {
        label: String,
        unit: Option<String>,
        cells: Vec<Option<String>>,
        numeric: bool,
        /// Its cells write the unit, the style's form placing `{unit}`: the
        /// header then writes none.
        unit_in_cells: bool,
    }
    // A unit as a form places it: escaped as text, a LaTeX form as it is —
    // `\SI{6.8}{%}` began a comment that took the line's `\\` with it.
    let in_form = |unit: &str| -> String {
        if unit.contains(['\\', '^', '_', '{']) {
            unit.to_string()
        } else {
            escaped(unit, &format.escape)
        }
    };
    let label = |at: usize| {
        dataset
            .labels
            .get(at)
            .filter(|label| !label.is_empty())
            .cloned()
            .unwrap_or_else(|| dataset.headers[at].clone())
    };
    let written = |cell: Option<&Cell>| -> (Option<String>, bool) {
        match cell {
            None | Some(Cell::Scalar(None | Some(Value::Absent))) => (None, true),
            Some(Cell::Written { text, .. }) => (Some(text.clone()), true),
            Some(Cell::Scalar(Some(value))) => match value {
                Value::Integer(_) | Value::Number(_) => (Some(plain_number(value)), true),
                Value::Text(text) => (Some(escaped(text, &format.escape)), false),
                other => (
                    Some(escaped(&flat_as_written(other), &format.escape)),
                    false,
                ),
            },
            Some(Cell::List(values)) => (
                Some(escaped(
                    &values
                        .iter()
                        .map(flat_as_written)
                        .collect::<Vec<_>>()
                        .join(", "),
                    &format.escape,
                )),
                false,
            ),
            Some(Cell::Tags(tags)) => (Some(escaped(&tags.join(", "), &format.escape)), false),
        }
    };
    let mut columns: Vec<Column> = Vec::new();
    let mut groups: Vec<usize> = Vec::new();
    let mut at = 0;
    while at < dataset.headers.len() {
        if dataset.group_columns.contains(&at) {
            groups.push(columns.len());
        }
        if let Some(templated) = dataset
            .templated
            .iter()
            .find(|templated| templated.columns.first() == Some(&at))
        {
            columns.push(Column {
                label: escaped(&label(at), &format.escape),
                unit: None,
                cells: templated.texts.clone(),
                numeric: true,
                unit_in_cells: false,
            });
            at += templated.columns.len().max(1);
            continue;
        }
        let pair = dataset.quantities.iter().find(|quantity| {
            quantity.value == at && quantity.uncertainty.is_some() && format.property.is_some()
        });
        let unit = dataset.units.get(at).cloned().flatten();
        let unit_in_form = unit.as_deref().map(in_form);
        let mut numeric = true;
        let mut unit_in_cells = false;
        let cells: Vec<Option<String>> = dataset
            .rows
            .iter()
            .map(|row| {
                let (value, is_number) = written(row.get(at));
                numeric &= is_number;
                // The style's form of a property, its unit placed where the
                // template names it: `\SI{12.5 \pm 0.05}{\gram}`, its pieces
                // where the value lacks what they write.
                let forms = crate::core::formatting::StyleForms {
                    property: format.property.as_deref(),
                    uncertainty: format.uncertainty.as_deref(),
                    unitless: format.unitless.as_deref(),
                };
                let mut filled = |value: &str, uncertainty: Option<&str>| {
                    let written = crate::core::formatting::filled_property(
                        &forms,
                        value,
                        uncertainty,
                        unit_in_form.as_deref(),
                        None,
                    );
                    if written.is_some()
                        && unit.as_deref().is_some_and(|unit| !unit.is_empty())
                        && format
                            .property
                            .as_deref()
                            .is_some_and(|form| form.contains("{unit}"))
                    {
                        unit_in_cells = true;
                    }
                    written.unwrap_or_else(|| value.to_string())
                };
                match (pair, &format.property) {
                    (Some(quantity), Some(_)) => {
                        let (uncertainty, _) = written(row.get(quantity.uncertainty?));
                        value.map(|value| filled(&value, uncertainty.as_deref()))
                    }
                    // A number alone, written in the style's form where it
                    // gives one for a value with no uncertainty.
                    _ => match &value {
                        Some(text)
                            if is_number
                                && match unit.as_deref().filter(|unit| !unit.is_empty()) {
                                    Some(_) => format.uncertainty.is_some(),
                                    None => format.unitless.is_some(),
                                } =>
                        {
                            Some(filled(text, None))
                        }
                        _ => value,
                    },
                }
            })
            .collect();
        columns.push(Column {
            label: escaped(&label(at), &format.escape),
            unit,
            cells,
            numeric,
            unit_in_cells,
        });
        at += if pair.is_some() { 2 } else { 1 };
    }
    let align: String = columns
        .iter()
        .map(|column| {
            if column.numeric {
                format.align_number.as_str()
            } else {
                format.align_text.as_str()
            }
        })
        .collect();
    let line =
        |template: &str, cells: &[String]| template.replace("{cells}", &cells.join(&format.cells));
    // The header's cells are joined as the style says: HTML's `</th><th>`,
    // where its rows join theirs with `</td><td>`.
    let header_line = |template: &str, cells: &[String]| {
        template.replace(
            "{cells}",
            &cells.join(format.header_cells.as_deref().unwrap_or(&format.cells)),
        )
    };
    let mut out = String::new();
    let mut push = |text: &str| {
        if !text.is_empty() {
            out.push_str(text);
            out.push('\n');
        }
    };
    push(&format.before.replace("{align}", &align));
    if let Some(header) = &format.header {
        let cells: Vec<String> = columns
            .iter()
            .map(|column| match &column.unit {
                // A unit its cells write is not written again above them.
                Some(_) if column.unit_in_cells => column.label.clone(),
                // Escaped, as a text is, unless the format names a style whose
                // forms are written for it: `%` began a LaTeX comment.
                Some(unit) => {
                    // A LaTeX form — `^\circ\mathrm{C}` — is math, written in
                    // math mode: escaped as text, or left outside it, it did
                    // not compile; any other unit is text, and escaped — `%`
                    // began a comment where the format named a style.
                    let math = unit.contains(['\\', '^', '_', '{']);
                    let unit = match format.escape {
                        Escape::Latex if math => format!("${unit}$"),
                        _ => escaped(unit, &format.escape),
                    };
                    match &format.header_unit {
                        Some(after) => {
                            format!("{}{}", column.label, after.replace("{unit}", &unit))
                        }
                        None => column.label.clone(),
                    }
                }
                None => column.label.clone(),
            })
            .collect();
        push(&header_line(header, &cells));
        if let Some(rule) = &format.rule {
            let cells = vec![rule.clone(); columns.len()];
            push(&line(header, &cells));
        }
    }
    let mut previous: Option<Vec<Option<String>>> = None;
    for index in 0..dataset.rows.len() {
        let grouped: Vec<Option<String>> = groups
            .iter()
            .map(|column| columns[*column].cells[index].clone())
            .collect();
        // A line holding `{group}` heads each group, the first included,
        // its values in place; another is written between two groups.
        let begins = !groups.is_empty() && previous.as_ref() != Some(&grouped);
        if let Some(between) = &format.between_groups
            && begins
        {
            if between.contains("{group}") {
                let values: Vec<String> = grouped
                    .iter()
                    .map(|value| value.clone().unwrap_or_else(|| format.missing.clone()))
                    .collect();
                push(
                    &between
                        .replace("{group}", &values.join(" · "))
                        .replace("{columns}", &columns.len().to_string()),
                );
            } else if previous.is_some() {
                push(between);
            }
        }
        previous = Some(grouped);
        let cells: Vec<String> = columns
            .iter()
            .map(|column| {
                column.cells[index]
                    .clone()
                    .unwrap_or_else(|| format.missing.clone())
            })
            .collect();
        push(&line(&format.row, &cells));
    }
    push(&format.after.replace("{align}", &align));
    out
}

/// A number as the file writes it, never prefixed as a spreadsheet's text.
fn plain_number(value: &Value) -> String {
    match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => decimal(*number),
        other => flat_as_written(other),
    }
}

/// A value as its file writes it, text as it is: the quote a CSV puts before
/// `=` or `-` is a spreadsheet's affair.
fn flat_as_written(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        other => flat(other),
    }
}

/// Text made safe for a document's language: LaTeX's and HTML's by the
/// crates written for them, Markdown's cell bar, or the replacements a style
/// declares.
fn escaped(text: &str, escape: &Escape) -> String {
    escape.apply(text)
}

pub fn serialize_with(
    dataset: &Dataset,
    format: Format,
    crlf: bool,
) -> Result<String, ExportFormatError> {
    let text = match &format {
        Format::Csv => separated(&with_templates(dataset), ',')?,
        Format::Tsv => separated(&with_templates(dataset), '\t')?,
        Format::Json => json(dataset),
        Format::Text(text_format) => text(dataset, text_format),
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
        templated: dataset
            .templated
            .iter()
            .map(|templated| Templated {
                columns: templated
                    .columns
                    .iter()
                    .filter_map(|at| moved(*at))
                    .collect(),
                ..templated.clone()
            })
            .collect(),
        labels: if dataset.labels.is_empty() {
            Vec::new()
        } else {
            kept.iter()
                .map(|at| dataset.labels.get(*at).cloned().unwrap_or_default())
                .collect()
        },
        group_columns: dataset
            .group_columns
            .iter()
            .filter_map(|at| moved(*at))
            .collect(),
        json_keys: dataset.json_keys.clone(),
    };
    let dropped = empty
        .iter()
        .map(|at| dataset.headers[*at].clone())
        .collect();
    (trimmed, dropped)
}

pub fn delimiter(format: &Format) -> Option<char> {
    match format {
        Format::Csv => Some(','),
        Format::Tsv => Some('\t'),
        Format::Json | Format::Text(_) => None,
    }
}

pub fn extension(format: &Format) -> String {
    match format {
        Format::Csv => "csv".to_string(),
        Format::Tsv => "tsv".to_string(),
        Format::Json => "json".to_string(),
        Format::Text(text) => text.extension.clone(),
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
    // A quantity one of whose values is not current — outdated, edited,
    // failed, never computed, or waiting for an input emptied since with the
    // number computed before — has a column of its states after its own:
    // nothing else in a CSV tells it from a current one. One edited by hand
    // over its formula had none where no other value of its column was late.
    let late: Vec<(usize, &QuantityColumns)> = dataset
        .quantities
        .iter()
        .filter(|quantity| {
            quantity.states.iter().any(|state| {
                state.as_deref().is_some_and(|state| {
                    !matches!(state, "current" | "entered" | "absent" | "not applicable")
                })
            })
        })
        .map(|quantity| (quantity.uncertainty.unwrap_or(quantity.value), quantity))
        .collect();
    let states_after = |at: usize| late.iter().filter(move |(after, _)| *after == at);
    // A unit beside its header, `carbon [g/L]`.
    let mut headers: Vec<String> = Vec::new();
    for (at, header) in dataset.headers.iter().enumerate() {
        headers.push(match dataset.units.get(at) {
            Some(Some(unit)) => format!("{header} [{unit}]"),
            _ => header.clone(),
        });
        headers.extend(states_after(at).map(|(_, quantity)| format!("{}_state", quantity.key)));
    }
    // Writing to memory fails only on a record of another length, which a
    // dataset never holds.
    let _ = writer.write_record(&headers);
    for (number, row) in dataset.rows.iter().enumerate() {
        let written: Vec<String> = row
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
        let mut cells = Vec::new();
        for (at, cell) in written.into_iter().enumerate() {
            cells.push(cell);
            cells.extend(states_after(at).map(|(_, quantity)| {
                // Every state but current: a column that says `waiting` and
                // leaves an outdated value blank calls it current.
                match quantity.states.get(number).cloned().flatten() {
                    Some(state) if state != "current" => state,
                    _ => String::new(),
                }
            }));
        }
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
                        let key = dataset.json_keys.get(header).unwrap_or(header);
                        entries.insert(key.clone(), json_value(row.get(index)));
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
