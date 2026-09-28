//! Where one number came from — or what one table is — as a value every surface
//! renders its own way: the command line's `explain`, the workbench's sample
//! screen.

use std::path::{Path, PathBuf};

use crate::config::project_config::ProjectConfig;
use crate::core::identifier::Identifier;
use crate::core::property::{InputName, InputRecord, Produced};
use crate::core::sample::Sample;
use crate::core::table::RowAddress;
use crate::core::value::Value;
use crate::format::fingerprint::{self, Freshness};
use crate::query::field_addressing::{
    self as fields, Channel, Field, FieldError, Resolution, Subject,
};

/// What one question asked of a sample is about.
#[derive(Debug, Clone, PartialEq)]
pub enum Explanation {
    Value(ValueExplanation),
    Table(TableExplanation),
}

/// One number: whether anything is stored, where it came from, and how it
/// stands.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueExplanation {
    pub field: String,
    pub origin: Origin,
    pub state: Option<Freshness>,
    /// The traceback the project's failure log kept, for a failed value.
    pub trace: Option<String>,
}

/// Which number a formula made, where it made one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    Value,
    Uncertainty,
    Whole,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    /// Nothing is stored here.
    Nothing,
    /// Its formula failed, and left no value: the failure is the answer.
    Failed { message: String },
    /// A statistic of a quantity's readings.
    Statistic { readings: usize },
    /// Put here by hand, over what a formula or a statistic gave.
    ByHand,
    /// Entered or measured; nothing computed it.
    Entered,
    /// A formula that reads nothing of the sample.
    ReadsNothing(Made),
    /// A statistic of its own readings, the one its model declares.
    OwnReadings,
    /// A formula, and what it read.
    Inputs { made: Made, inputs: Vec<Input> },
}

/// One input as the record holds it, and how it stands now.
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    /// As the record names it: `foam`, `row.co2`, `mashing.wort`.
    pub name: String,
    /// The field a surface renders its value from — the explained row's cell
    /// for a row input — or `None` for a whole column, which is `cells` long.
    pub field: Option<String>,
    pub cells: Option<usize>,
    /// The digest or the literal the record kept, as a file writes it.
    pub recorded: String,
    pub state: Option<InputState>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputState {
    Of(Freshness),
    /// A whole column: stale when any of its cells is.
    Column {
        stale: bool,
    },
}

/// A table: its index, its columns, which are measured and which the model
/// fills, and how many rows it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct TableExplanation {
    pub name: Identifier,
    pub rows: usize,
    pub columns: Vec<TableColumn>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    pub name: Identifier,
    pub unit: Option<String>,
    pub kind: ColumnKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnKind {
    Index,
    Measured,
    Computed { from: Vec<String> },
}

/// Why a question cannot be answered about one number.
#[derive(Debug, Clone, PartialEq)]
pub enum ExplainError {
    /// The field does not resolve in this sample.
    Unknown(FieldError),
    /// A table's name, written with a column that is not one number.
    IsTable(String),
    /// A list — readings, a column — rather than one value.
    NotOneValue,
    /// A row the table does not hold.
    NoSuchRow { table: String },
}

/// Where the number `field_name` of `sample` came from, or what the table of
/// that name is. `config` is the sample's own project, which says where its
/// failure log is.
pub fn explain(
    sample: &Sample,
    path: Option<&Path>,
    config: Option<&ProjectConfig>,
    field_name: &str,
    vocabulary: &fields::Vocabulary,
) -> Result<Explanation, ExplainError> {
    if !field_name.contains(['.', '['])
        && let Ok(name) = Identifier::new(field_name)
        && sample.table(&name).is_ok()
    {
        return Ok(Explanation::Table(table(sample, &name)));
    }
    let field = fields::parse(field_name).map_err(ExplainError::Unknown)?;
    let subject = Subject {
        sample,
        path,
        vocabulary,
        states: None,
    };
    let resolved = fields::resolve(&field, &subject).map_err(|error| {
        if !field_name.contains(['.', '['])
            && Identifier::new(field_name).is_ok_and(|name| vocabulary.has_table(&name))
        {
            ExplainError::IsTable(field_name.to_string())
        } else {
            ExplainError::Unknown(error)
        }
    })?;
    let Resolution::Scalar(value) = resolved else {
        return Err(ExplainError::NotOneValue);
    };
    if let Field::Cell { table, row, .. } = &field
        && let Ok(held) = sample.table(table)
        && held.row(row).is_err()
    {
        return Err(ExplainError::NoSuchRow {
            table: table.to_string(),
        });
    }
    let states = fingerprint::check(sample).unwrap_or_default();
    let explained = |origin, state, trace| {
        Ok(Explanation::Value(ValueExplanation {
            field: field_name.to_string(),
            origin,
            state,
            trace,
        }))
    };
    if value.is_none() {
        if let Field::Named { name, .. } = &field
            && let Some(Freshness::Failed { message }) = states.get(name)
        {
            let trace = failure_trace(config, path, name.as_str());
            return explained(
                Origin::Failed {
                    message: message.clone(),
                },
                None,
                trace,
            );
        }
        return explained(Origin::Nothing, None, None);
    }
    if let Field::Named {
        name,
        channel: Channel::Stat(_),
    } = &field
    {
        let readings = sample
            .property(name)
            .ok()
            .map(|handle| {
                handle.with(|property| {
                    property
                        .readings()
                        .map_or(0, |readings| readings.as_slice().len())
                })
            })
            .unwrap_or(0);
        return explained(Origin::Statistic { readings }, None, None);
    }
    let (records, state) = match &field {
        Field::Named { name, .. } => (
            sample
                .property(name)
                .ok()
                .map(|handle| handle.with(|property| property.records().clone())),
            states.get(name).cloned(),
        ),
        Field::Cell {
            table, column, row, ..
        } => {
            let view = sample.table(table).ok().and_then(|held| held.row(row).ok());
            let records = view
                .as_ref()
                .and_then(|view| view.cell(column).ok())
                .map(|cell| cell.records().clone());
            let state = view.as_ref().and_then(|view| {
                let index: Vec<Value> = view.index().into_iter().cloned().collect();
                fingerprint::check_cell_among(sample, table, column, &index).ok()
            });
            (records, state)
        }
        _ => return explained(Origin::Entered, None, None),
    };
    let made = match records.as_ref().and_then(|records| records.produced) {
        Some(Produced::Value) => Made::Value,
        Some(Produced::Uncertainty) => Made::Uncertainty,
        None => Made::Whole,
    };
    let origin = match records.and_then(|records| records.computed) {
        None => match &state {
            Some(Freshness::Edited) => Origin::ByHand,
            _ => Origin::Entered,
        },
        Some(inputs) if inputs.is_empty() => Origin::ReadsNothing(made),
        Some(inputs)
            if inputs.len() == 1 && inputs.keys().all(|key| key.to_string() == "readings") =>
        {
            Origin::OwnReadings
        }
        Some(inputs) => Origin::Inputs {
            made,
            inputs: inputs
                .iter()
                .map(|(input, record)| {
                    described_input(sample, &field, field_name, &states, input, record)
                })
                .collect(),
        },
    };
    let trace = match (&state, &field) {
        (Some(Freshness::Failed { .. }), Field::Named { name, .. }) => {
            failure_trace(config, path, name.as_str())
        }
        _ => None,
    };
    explained(origin, state, trace)
}

fn described_input(
    sample: &Sample,
    field: &Field,
    field_name: &str,
    states: &indexmap::IndexMap<Identifier, Freshness>,
    input: &InputName,
    record: &InputRecord,
) -> Input {
    let name = input.to_string();
    let recorded = match record {
        InputRecord::Digest(digest) if digest.is_edited() => format!("{digest} (edited)"),
        InputRecord::Digest(digest) => digest.to_string(),
        InputRecord::Literal(value) => format!("{value:?}"),
    };
    match input {
        InputName::Named(input_name) => Input {
            field: Some(name.clone()),
            name,
            cells: None,
            recorded,
            state: states.get(input_name).cloned().map(InputState::Of),
        },
        // A row input is a cell of the row explained.
        InputName::Cell(column) => {
            let (spelled, state) = match field {
                Field::Cell { table, row, .. } => {
                    let spelled = match (field_name.find('['), field_name.rfind(']')) {
                        (Some(open), Some(close)) if open < close => {
                            format!("{table}.{column}[{}]", &field_name[open + 1..close])
                        }
                        _ => name.clone(),
                    };
                    let state = sample
                        .table(table)
                        .ok()
                        .and_then(|held| held.row(row).ok())
                        .and_then(|view| {
                            let index: Vec<Value> = view.index().into_iter().cloned().collect();
                            fingerprint::check_cell_among(sample, table, column, &index).ok()
                        });
                    (spelled, state)
                }
                _ => (name.clone(), None),
            };
            Input {
                name,
                field: Some(spelled),
                cells: None,
                recorded,
                state: state.map(InputState::Of),
            }
        }
        InputName::Column { table, column } => {
            let stale = fingerprint::check_table(sample, table)
                .ok()
                .and_then(|cells| {
                    cells.get(column).map(|rows| {
                        rows.iter().any(|(_, state)| {
                            !matches!(
                                state,
                                Freshness::Source
                                    | Freshness::Current
                                    | Freshness::Edited
                                    | Freshness::RecordMissing
                            )
                        })
                    })
                });
            Input {
                name,
                field: None,
                cells: sample.table(table).ok().map(|held| held.rows().count()),
                recorded,
                state: stale.map(|stale| InputState::Column { stale }),
            }
        }
    }
}

/// What a table is.
pub fn table(sample: &Sample, name: &Identifier) -> TableExplanation {
    let Ok(held) = sample.table(name) else {
        return TableExplanation {
            name: name.clone(),
            rows: 0,
            columns: Vec::new(),
        };
    };
    let rows: Vec<_> = held.rows().collect();
    let index: Vec<Identifier> = held.index_columns().to_vec();
    let first = rows.first();
    let columns = held
        .column_names()
        .into_iter()
        .map(|column| {
            let cell = first.and_then(|row| row.cell(column).ok());
            let unit = held
                .presentation_of(column, &RowAddress::ordinal(0))
                .ok()
                .and_then(|presentation| presentation.unit);
            let kind = if index.contains(column) {
                ColumnKind::Index
            } else if let Some(recorded) = cell.and_then(|cell| cell.records().computed.clone()) {
                ColumnKind::Computed {
                    from: recorded.keys().map(ToString::to_string).collect(),
                }
            } else {
                ColumnKind::Measured
            };
            TableColumn {
                name: column.clone(),
                unit,
                kind,
            }
        })
        .collect();
    TableExplanation {
        name: name.clone(),
        rows: rows.len(),
        columns,
    }
}

/// Where a sample's failure log lives, from the project's root: named after the
/// sample's file where it lies in the project, so that `a/S1.md` and `b/S1.md`
/// never share one. The model runtime's worker names it the same way.
pub fn failure_log(root: &Path, sample: &Path) -> Option<PathBuf> {
    let own = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = own(sample)
        .strip_prefix(own(root))
        .map(Path::to_path_buf)
        .ok()
        .or_else(|| sample.file_name().map(PathBuf::from))?;
    Some(
        PathBuf::from(".samplekit")
            .join("failures")
            .join(relative.with_extension("log")),
    )
}

/// The traceback a value's failure left in its sample's log.
pub fn failure_trace(
    config: Option<&ProjectConfig>,
    sample: Option<&Path>,
    value: &str,
) -> Option<String> {
    let root = config?.root();
    let log = root.join(failure_log(root, sample?)?);
    let text = std::fs::read_to_string(log).ok()?;
    let opening = format!("## {value}\n");
    let start = text.find(&opening)? + opening.len();
    let rest = &text[start..];
    let end = rest.find("\n## ").map_or(rest.len(), |at| at + 1);
    let block = rest[..end].trim_end();
    (!block.is_empty()).then(|| block.to_string())
}
