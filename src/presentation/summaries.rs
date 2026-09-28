//! A collection's columns summarised: the count with its denominator, the mean,
//! the sample deviation, its standard error, the median and the extremes — what
//! `--summary` and the workbench both show.

use crate::collection::sample_list::{self as list, SampleList};
use crate::config::project_config::{ColumnSpec, Profile};
use crate::core::formatting::{self, Precision, Resolved};
use crate::core::value::Value;
use crate::presentation::terminal_rendering as render;
use crate::query::field_addressing::{self as fields, Field, Subject};

/// One column summarised, with what it is written in.
pub struct SummaryRow {
    pub label: String,
    pub unit: Option<String>,
    pub summary: Option<list::ColumnSummary>,
    pub precision: Option<Precision>,
    /// The samples summarised over: the denominator of an empty count.
    pub of: usize,
}

/// A row per column of `profile`, over `selected`, and the labels of the
/// columns left out because they hold text: what `--summary` and the workbench
/// both show.
pub fn rows(
    selected: &SampleList,
    profile: &Profile,
    collection: &SampleList,
    style: Option<&str>,
    lenient: bool,
) -> Result<(Vec<SummaryRow>, Vec<String>), String> {
    let vocabulary = selected.vocabulary();
    let guards: Vec<_> = selected.iter().map(|entry| entry.sample.borrow()).collect();
    let subjects: Vec<Subject> = selected
        .iter()
        .zip(guards.iter())
        .map(|(entry, guard)| Subject {
            sample: guard,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        })
        .collect();
    let mut rows = Vec::new();
    let mut left_out = Vec::new();
    for column in profile.columns() {
        let field = fields::parse(&column.field).map_err(|error| error.to_string())?;
        // `state` is words, left out as text is: summarised, it was refused as
        // a list, and the summary with it.
        if field == Field::Reserved(fields::ReservedField::State) {
            left_out.push(profile.label_for(column).to_string());
            continue;
        }
        // Over one configuration's part, a column only another part holds is a
        // row of placeholders.
        let summary = match selected.summarize(&field) {
            Ok(summary) => summary,
            Err(_) if lenient => None,
            Err(error) => return Err(error.to_string()),
        };
        // A column holding text, booleans or dates has no mean: it is left out
        // and said, rather than shown as an empty row. An empty cell and an
        // inapplicable one are not the same answer.
        if summary.is_none()
            && let Ok(values) = selected.values(&field)
            && values
                .iter()
                .flatten()
                .any(|value| !matches!(value, Value::Integer(_) | Value::Number(_) | Value::Absent))
        {
            left_out.push(profile.label_for(column).to_string());
            continue;
        }
        // A column no sample fills still has the unit its project declares.
        let declared = match &field {
            Field::Named { name, .. } => collection
                .config()
                .and_then(|config| config.property(name.as_str()))
                .and_then(|declaration| declaration.unit.clone()),
            _ => None,
        };
        rows.push(SummaryRow {
            label: profile.label_for(column).to_string(),
            unit: render::column_unit(column, &subjects, profile, collection.config(), style)
                .or(declared),
            precision: precision(column, &field, profile, selected, collection, style),
            summary,
            of: selected.len(),
        });
    }
    Ok((rows, left_out))
}

/// A summary per group of `fields`: each group's key with its rows, in the
/// groups' order, and the labels left out over every group, once each — what
/// `--summary --group` and the workbench's `S` both show.
pub fn grouped(
    selected: &SampleList,
    profile: &Profile,
    collection: &SampleList,
    style: Option<&str>,
    lenient: bool,
    fields: &[Field],
    order: crate::collection::sample_list::GroupOrder,
) -> GroupedRows {
    let groups = selected
        .group_by(fields, order)
        .map_err(|error| error.to_string())?;
    let mut parts = Vec::with_capacity(groups.len());
    let mut left_out: Vec<String> = Vec::new();
    for group in groups {
        let (rows, left) = rows(&group.samples, profile, collection, style, lenient)?;
        for label in left {
            if !left_out.contains(&label) {
                left_out.push(label);
            }
        }
        parts.push((group.key, rows));
    }
    Ok((parts, left_out))
}

/// What [`grouped`] gives: each group's key and rows, and the labels left out.
pub type GroupedRows = Result<(Vec<(Vec<Value>, Vec<SummaryRow>)>, Vec<String>), String>;

/// A group's value as a terminal says it: `(none)` for the samples holding
/// none — a word, since `—` is what an empty cell of a table already shows —
/// and `n/a` for those it does not apply to.
pub fn group_value(value: &Value) -> String {
    match value {
        Value::Absent => "(none)".to_string(),
        Value::NotApplicable => "n/a".to_string(),
        Value::Integer(whole) => whole.to_string(),
        Value::Number(real) => format!("{real}"),
        Value::Text(text) => text.clone(),
        Value::Boolean(yes) => yes.to_string(),
        Value::Date(date) => date.to_string(),
        Value::DateTime(stamp) => stamp.to_string(),
    }
}

/// A group's heading, each field beside its value — `style = IPA · yeast =
/// US-05` — so that a table read alone still says which group it is.
pub fn group_heading(names: &[String], key: &[Value]) -> String {
    names
        .iter()
        .zip(key)
        .map(|(name, value)| format!("{name} = {}", group_value(value)))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The heading of the mean's column, which says how it was taken: `mean` where
/// no row is weighted, `mean (1/u²)` where every row with a mean is, and the
/// weighted rows named where only some are.
pub fn mean_header<'a>(rows: impl IntoIterator<Item = &'a SummaryRow>) -> String {
    let with_mean: Vec<&SummaryRow> = rows
        .into_iter()
        .filter(|row| row.summary.is_some())
        .collect();
    let weighted: Vec<&SummaryRow> = with_mean
        .iter()
        .copied()
        .filter(|row| {
            row.summary
                .as_ref()
                .is_some_and(|summary| summary.weighted_mean.is_some())
        })
        .collect();
    // Each column once, over the groups of a grouped summary.
    let mut labels: Vec<&str> = Vec::new();
    for row in &weighted {
        if !labels.contains(&row.label.as_str()) {
            labels.push(row.label.as_str());
        }
    }
    if weighted.is_empty() {
        "mean".to_string()
    } else if weighted.len() == with_mean.len() {
        "mean (1/u²)".to_string()
    } else {
        format!("mean (1/u²: {})", labels.join(", "))
    }
}

/// A row's cells after its label, as a terminal table writes them: `n/of`,
/// mean, s, sem, median, min, max — placeholders for a column with nothing
/// to summarise.
pub fn written(row: &SummaryRow) -> Vec<String> {
    let mut cells = match &row.summary {
        // Nothing to summarise still says out of how many.
        None => vec![
            format!("0/{}", row.of),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
        ],
        Some(column) => {
            let summary = &column.summary;
            let decimals = summary
                .sample_stdev
                .filter(|spread| *spread > 0.0)
                .map(decimals_of);
            let written = |value: f64| match (&row.precision, decimals) {
                (Some(precision), _) => number(value, Some(precision)),
                // A deviation of ten or more puts its two significant figures
                // above the point: the mean is rounded to them, not written
                // whole — `{:.0}` of a mean near 1e20 wrote the float's
                // noise, 100000000000000131072.
                (None, Some(0)) => {
                    to_the_deviation(value, summary.sample_stdev.unwrap_or_default())
                }
                (None, Some(decimals)) => format!("{value:.decimals$}"),
                (None, None) => number(value, None),
            };
            let spread = |value: Option<f64>, own: bool| match value {
                None => "—".to_string(),
                Some(value) if own && row.precision.is_none() && value > 0.0 => {
                    match decimals_of(value) {
                        0 => to_the_deviation(value, value),
                        decimals => format!("{value:.decimals$}"),
                    }
                }
                Some(value) => written(value),
            };
            vec![
                match column.not_applicable {
                    0 => format!("{}/{}", summary.count.get(), column.considered),
                    // Counted apart: the value does not apply to them.
                    apart => format!(
                        "{}/{} ({apart} n/a)",
                        summary.count.get(),
                        column.considered
                    ),
                },
                written(column.mean()),
                spread(summary.sample_stdev, false),
                spread(summary.standard_error, true),
                written(summary.median),
                written(summary.minimum),
                written(summary.maximum),
            ]
        }
    };
    cells.shrink_to_fit();
    cells
}

/// The decimal places two significant figures of a number need.
pub fn decimals_of(number: f64) -> usize {
    let written = format!("{number:.1e}");
    let exponent: i32 = written
        .split_once('e')
        .and_then(|(_, exponent)| exponent.parse().ok())
        .unwrap_or(0);
    (1 - exponent).max(0) as usize
}

/// A number rounded to the power of ten of a deviation's second significant
/// figure, where that lies at or above the units: in full while that is exact
/// in a double, in scientific notation beyond, its digits the significant ones.
pub fn to_the_deviation(value: f64, deviation: f64) -> String {
    let exponent = |number: f64| -> i32 {
        format!("{number:.1e}")
            .split_once('e')
            .and_then(|(_, exponent)| exponent.parse().ok())
            .unwrap_or(0)
    };
    // The place of the deviation's second significant figure.
    let place = (exponent(deviation) - 1).max(0);
    let scale = 10f64.powi(place);
    let rounded = (value / scale).round() * scale;
    if rounded.abs() < 1e15 {
        return format!("{rounded:.0}");
    }
    let digits = (exponent(rounded) - place).max(0) as usize;
    format!("{rounded:.digits$e}")
}

/// A summary is written as its column is: the precision the column declares,
/// or the one its quantity resolves to in the project.
pub fn number(number: f64, precision: Option<&Precision>) -> String {
    let Some(precision) = precision else {
        return match Value::number(number) {
            Ok(value) => formatting::format_value(
                &value,
                &Resolved {
                    unit: None,
                    symbol: None,
                    separator: "±".to_string(),
                    precision: None,
                },
            ),
            Err(_) => number.to_string(),
        };
    };
    // A mean can overflow on extreme data; it is written as it is rather than
    // aborting the command.
    let Ok(value) = Value::number(number) else {
        return number.to_string();
    };
    let resolved = Resolved {
        unit: None,
        symbol: None,
        separator: "±".to_string(),
        precision: Some(precision.clone()),
    };
    formatting::format_value(&value, &resolved)
}

fn precision(
    column: &ColumnSpec,
    field: &Field,
    profile: &Profile,
    selected: &SampleList,
    collection: &SampleList,
    style: Option<&str>,
) -> Option<Precision> {
    let (address, presentation) = match field {
        Field::Named { name, .. } => (
            name.to_string(),
            selected
                .iter()
                .find_map(|entry| {
                    entry
                        .sample
                        .borrow()
                        .property(name)
                        .ok()
                        .map(|handle| handle.presentation())
                })
                .unwrap_or_default(),
        ),
        Field::Cell { table, column, .. } => (
            format!("{table}.{column}"),
            formatting::Presentation::default(),
        ),
        _ => return None,
    };
    let composed = profile.presentation_for(column, &presentation);
    match collection.config() {
        Some(config) => {
            config
                .resolved_as(&composed, &address, style)
                .ok()?
                .precision
        }
        None => composed.precision,
    }
}
