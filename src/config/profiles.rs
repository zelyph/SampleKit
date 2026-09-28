//! What a profile *does*: its labels, its export headers, and the composition
//! of presentation. The declaration is
//! [`crate::config::project_config`]'s: the `impl` lives beside the behavior
//! rather than beside the type, which a layer below could not reach.

use std::fmt;

use crate::config::project_config::{ColumnSpec, Profile};
use crate::core::formatting::{Precision, Presentation};
use crate::core::identifier::Identifier;
use crate::format::schema::PrecisionSchema;

impl Profile {
    pub fn columns(&self) -> &[ColumnSpec] {
        &self.columns
    }

    /// What a terminal shows. Falls back to the field as written, and never
    /// reads `header`.
    pub fn label_for<'a>(&self, column: &'a ColumnSpec) -> &'a str {
        column.label.as_deref().unwrap_or(&column.field)
    }

    /// One name per emitted column, from `header` or the field — **never**
    /// from `label`, so that a column labelled *Plato (g/L)* still exports
    /// as `plato` and a reader can round-trip the header back to a field.
    ///
    /// A quantity flattens where the format cannot carry a pair: two names
    /// when `expanded`, which every export format is and a terminal is not.
    pub fn headers_for(&self, column: &ColumnSpec, expanded: bool) -> Vec<String> {
        let base = column
            .header
            .clone()
            .unwrap_or_else(|| column.field.clone());
        // A column that names a channel outright is one column, and it is
        // called `malt_value` too: a column's name does not depend on how it
        // was asked for.
        if let Some(channel) = channel_of(&column.field) {
            // The field without its channel: a cell keeps its table, column
            // and row, or `m.srm[20].u` and `m.ebc[20].u` were both `m_uncertainty`.
            let stem = column.header.clone().unwrap_or_else(|| {
                column
                    .field
                    .rsplit_once('.')
                    .map_or(column.field.as_str(), |(head, _)| head)
                    .to_string()
            });
            return vec![format!("{stem}_{channel}")];
        }
        if expanded {
            return vec![format!("{base}_value"), format!("{base}_uncertainty")];
        }
        vec![base]
    }

    /// Two columns that would write one header, as `(header, first field,
    /// second field)`: in a file nothing could tell them apart afterwards.
    pub fn duplicate_header(
        &self,
        expanded: impl Fn(&ColumnSpec) -> bool,
    ) -> Option<(String, String, String)> {
        let mut seen: Vec<(String, &str)> = Vec::new();
        for column in &self.columns {
            for header in self.headers_for(column, expanded(column)) {
                if let Some((_, first)) = seen.iter().find(|(name, _)| *name == header) {
                    return Some((header, first.to_string(), column.field.clone()));
                }
                seen.push((header, column.field.as_str()));
            }
        }
        None
    }

    /// The column's overrides composed onto the property's own metadata, which
    /// the caller supplies: a profile has no access to the sample a
    /// presentation comes from.
    pub fn presentation_for(&self, column: &ColumnSpec, property: &Presentation) -> Presentation {
        compose(column, property)
    }

    pub fn covers(&self, name: &Identifier) -> bool {
        self.columns
            .iter()
            .any(|column| stem(&column.field) == name.as_str())
    }

    /// The profile a grouped table is written with: a file has no place for a
    /// heading, so each field grouped by is a column — first, where the columns
    /// do not already name it, so that a spreadsheet groups again by the first
    /// columns. One shown keeps its place.
    pub fn carrying(&self, groups: &[String]) -> Profile {
        let mut columns: Vec<ColumnSpec> = groups
            .iter()
            .filter(|field| !self.columns.iter().any(|column| column.field == **field))
            .map(|field| ColumnSpec {
                field: field.clone(),
                label: None,
                header: None,
                precision: None,
                template: None,
            })
            .collect();
        columns.extend(self.columns.iter().cloned());
        Profile {
            columns,
            ..self.clone()
        }
    }
}

// ------------------------------------------------------- a written column

/// One column as the command line and `columns=` write it:
/// `field:precision=label`, every part after the field optional.
pub fn parse_column(written: &str) -> Result<ColumnSpec, ColumnError> {
    let column = written.trim();
    if column.is_empty() {
        return Err(ColumnError::Empty);
    }
    let (declaration, label) = match first_top_level(column, '=') {
        Some(at) => {
            let label = column[at + 1..].trim();
            if label.is_empty() {
                return Err(ColumnError::EmptyLabel {
                    column: column.to_string(),
                });
            }
            (&column[..at], Some(label.to_string()))
        }
        None => (column, None),
    };
    let (field, precision) = match first_top_level(declaration, ':') {
        Some(at) => {
            let precision = declaration[at + 1..].trim();
            if precision.is_empty() {
                return Err(ColumnError::EmptyPrecision {
                    column: column.to_string(),
                });
            }
            Precision::both(precision).map_err(|error| ColumnError::Precision {
                column: column.to_string(),
                reason: error.to_string(),
            })?;
            (
                declaration[..at].trim(),
                Some(PrecisionSchema::Both(precision.to_string())),
            )
        }
        None => (declaration.trim(), None),
    };
    if field.is_empty() {
        return Err(ColumnError::Empty);
    }
    Ok(ColumnSpec {
        field: field.to_string(),
        label,
        header: None,
        precision,
        template: None,
    })
}

/// A comma list of written columns, split where a comma is not inside an index.
pub fn parse_columns(written: &str) -> Result<Vec<ColumnSpec>, ColumnError> {
    split_top_level(written, ',')
        .into_iter()
        .map(parse_column)
        .collect()
}

/// The profile a written column list makes: named `-c`, with no sort.
pub fn anonymous(columns: Vec<ColumnSpec>) -> Profile {
    Profile {
        name: "-c".to_string(),
        columns,
        sort: Vec::new(),
        group: Vec::new(),
    }
}

/// Split column syntax without mistaking a comma, colon or equals sign in a
/// table index for column punctuation.
pub fn split_top_level(source: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for at in top_level_delimiters(source, delimiter) {
        parts.push(&source[start..at]);
        start = at + delimiter.len_utf8();
    }
    parts.push(&source[start..]);
    parts
}

pub fn first_top_level(source: &str, delimiter: char) -> Option<usize> {
    top_level_delimiters(source, delimiter).next()
}

pub fn last_top_level(source: &str, delimiter: char) -> Option<usize> {
    top_level_delimiters(source, delimiter).last()
}

fn top_level_delimiters(source: &str, delimiter: char) -> impl Iterator<Item = usize> + '_ {
    let mut depth = 0usize;
    let mut quoted = false;
    source.char_indices().filter_map(move |(at, character)| {
        match character {
            '"' if depth > 0 => quoted = !quoted,
            '[' if !quoted => depth += 1,
            ']' if !quoted => depth = depth.saturating_sub(1),
            _ => {}
        }
        (character == delimiter && depth == 0 && !quoted).then_some(at)
    })
}

/// A written column that names no column. The messages are the command line's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnError {
    Empty,
    EmptyLabel { column: String },
    EmptyPrecision { column: String },
    Precision { column: String, reason: String },
}

impl fmt::Display for ColumnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ColumnError::Empty => f.write_str("a column name cannot be empty"),
            ColumnError::EmptyLabel { column } => {
                write!(f, "column '{column}' has an empty label")
            }
            ColumnError::EmptyPrecision { column } => {
                write!(f, "column '{column}' has an empty precision")
            }
            ColumnError::Precision { reason, .. } => f.write_str(reason),
        }
    }
}

impl std::error::Error for ColumnError {}

/// The channel a field path ends with, spelled as an export header would say
/// it: `malt.v` and `malt.value` both flatten to `value`.
fn channel_of(field: &str) -> Option<&'static str> {
    let tail = field.rsplit('.').next()?;
    // A path with no dot has no channel, and a cell path's tail is its channel
    // only when it follows the bracket.
    if !field.contains('.') || field.ends_with(']') {
        return None;
    }
    // The **label** keeps its long spelling: an export header is one name,
    // decided here, and not a second way of writing an address.
    match tail {
        "v" => Some("value"),
        "u" => Some("uncertainty"),
        "unit" => Some("unit"),
        "symbol" => Some("symbol"),
        _ => None,
    }
}

/// The quantity a field path names, without its channel or its index. `malt.v`
/// and `malt` are the same quantity; `mashing.wort[77]` is `mashing`.
pub(crate) fn stem(field: &str) -> &str {
    let head = field.split('[').next().unwrap_or(field);
    head.split('.').next().unwrap_or(head)
}

/// A declared value wins where it speaks, and the property's own is kept where
/// it does not. Nothing here touches a value: presentation is never
/// destructive.
pub(crate) fn compose(entry: &ColumnSpec, property: &Presentation) -> Presentation {
    Presentation {
        unit: property.unit.clone(),
        symbol: property.symbol.clone(),
        precision: match &entry.precision {
            Some(declared) => precision_of(declared).or_else(|| property.precision.clone()),
            None => property.precision.clone(),
        },
    }
}

/// The one conversion lives on [`PrecisionSchema`]; this alias is what the
/// callers here already say.
pub(crate) fn precision_of(declared: &PrecisionSchema) -> Option<Precision> {
    declared.precision()
}
