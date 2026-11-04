//! A collection's columns summarised: the count with its denominator, the mean,
//! the sample deviation, its standard error, the median and the extremes — what
//! `--summary` and the workbench both show.

use crate::collection::sample_list::{self as list, SampleList};
use crate::config::project_config::{ColumnSpec, Profile, ProjectConfig};
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
    /// The decimals the column's values are written to, where they carry
    /// uncertainties and nothing is declared: its median and extremes are.
    pub places: Option<usize>,
    /// The samples the value applies to: the denominator of the count, those
    /// saying *not applicable* left out.
    pub of: usize,
    /// Those saying *not applicable*, counted apart on every surface.
    pub not_applicable: usize,
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
        // A tag set has no mean: left out and said, as text is — read as
        // absent, it was a row saying no sample carried a tag.
        if subjects.iter().any(|subject| {
            matches!(
                fields::resolve(&field, subject),
                Ok(fields::Resolution::Tags(_))
            )
        }) {
            left_out.push(profile.label_for(column).to_string());
            continue;
        }
        // Over one configuration's part, a column only another part holds is a
        // row of placeholders — that, and nothing else: a formula that failed,
        // units that disagree, are said.
        let summary = match selected.summarize(&field) {
            Ok(summary) => summary,
            Err(crate::collection::sample_list::ListError::Field(_)) if lenient => None,
            Err(error) => return Err(error.to_string()),
        };
        // A column holding text, booleans or dates has no mean: it is left out
        // and said, rather than shown as an empty row. An empty cell and an
        // inapplicable one are not the same answer.
        if summary.is_none()
            && let Ok(values) = selected.values(&field)
            && values.iter().flatten().any(|value| {
                // Not applicable is no text: a group whose samples all say
                // so was left out as *holding text*.
                !matches!(
                    value,
                    Value::Integer(_) | Value::Number(_) | Value::Absent | Value::NotApplicable
                )
            })
        {
            left_out.push(profile.label_for(column).to_string());
            continue;
        }
        // Over several projects, each sample's unit and precision are its
        // own project's, and the row's are those every project agrees on:
        // the first project named headed them all.
        let configs = configs_of(selected, collection);
        let distinct = distinct(&configs);
        // A column no sample fills still has the unit its project declares.
        let declared = match &field {
            Field::Named { name, .. } => agreed(distinct.iter().map(|config| {
                config
                    .and_then(|config| config.property(name.as_str()))
                    .and_then(|declaration| declaration.unit.clone())
            })),
            _ => None,
        };
        let unit = if collection.spans_configurations() {
            render::column_unit_each(column, &subjects, profile, &configs, style)
        } else {
            render::column_unit(column, &subjects, profile, collection.config(), style)
        };
        let places = render::column_places(profile, column, &subjects, &configs, style);
        rows.push(SummaryRow {
            label: profile.label_for(column).to_string(),
            unit: unit.or(declared),
            places,
            precision: agreed(
                distinct
                    .iter()
                    .map(|config| precision(column, &field, profile, selected, *config, style)),
            ),
            // Counted from the values where nothing is summarised: a group
            // whose samples all say *n/a* said `0/1` on a screen and `0` of `0`
            // in a file.
            of: summary.as_ref().map_or_else(
                || selected.len() - not_applicable_in(selected, &field),
                |summary| summary.considered,
            ),
            not_applicable: summary.as_ref().map_or_else(
                || not_applicable_in(selected, &field),
                |summary| summary.not_applicable,
            ),
            summary,
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

/// How many samples of `selected` say `field` is not applicable.
fn not_applicable_in(selected: &SampleList, field: &Field) -> usize {
    selected.values(field).map_or(0, |values| {
        values
            .iter()
            .filter(|value| matches!(value, Some(Value::NotApplicable)))
            .count()
    })
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

/// A group's heading, each field beside its value — `style = TITANIUM · process =
/// UT-05` — so that a table read alone still says which group it is.
pub fn group_heading(names: &[String], key: &[Value]) -> String {
    group_heading_in(names, key, None)
}

/// [`group_heading`], a number at the precision `config` declares for its
/// field, as the column shows it: `radius = 1.054`, where the computation left
/// `1.0539999999999998`.
pub fn group_heading_in(names: &[String], key: &[Value], config: Option<&ProjectConfig>) -> String {
    names
        .iter()
        .zip(key)
        .map(|(name, value)| {
            let precision = crate::collection::editing::declared_precision(name, config);
            let shown = match value {
                Value::Number(_) if precision.is_some() => {
                    crate::collection::editing::shown_at(value, precision)
                }
                _ => group_value(value),
            };
            format!("{name} = {shown}")
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The heading of the mean's column, which says how it was taken: `mean (1/u²)`
/// where every row with a mean is weighted, `mean` otherwise — each weighted
/// mean then says so beside it.
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
    if weighted.len() == with_mean.len() && !weighted.is_empty() {
        "mean (1/u²)".to_string()
    } else {
        // None weighted, or some: each weighted mean then says so itself,
        // since a heading naming a column said it of every group, one whose
        // uncertainty was 0 taking the plain mean among them.
        "mean".to_string()
    }
}

/// Whether some rows' means are weighted and others' not: each weighted one
/// then says `(1/u²)` beside it, as [`written`] writes it.
pub fn weighting_mixed<'a>(rows: impl IntoIterator<Item = &'a SummaryRow>) -> bool {
    let mut kinds = rows
        .into_iter()
        .filter_map(|row| row.summary.as_ref())
        .map(|summary| summary.weighted_mean.is_some());
    let Some(first) = kinds.next() else {
        return false;
    };
    kinds.any(|kind| kind != first)
}

/// A row's cells after its label, as a terminal table writes them: `n/of`,
/// mean, s, sem, median, min, max — placeholders for a column with nothing
/// to summarise. `mixed`, a weighted mean says `(1/u²)` beside it.
pub fn written(row: &SummaryRow, mixed: bool) -> Vec<String> {
    let mut cells = match &row.summary {
        // Nothing to summarise still says out of how many.
        None => vec![
            match row.not_applicable {
                0 => format!("0/{}", row.of),
                apart => format!("0/{} ({apart} n/a)", row.of),
            },
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
            "—".to_string(),
        ],
        Some(column) => {
            let summary = &column.summary;
            // The mean is known to its standard error, not to the spread of
            // the values: rounded to the deviation, a mean of 1437 beside a
            // standard error of 420 read 1400.
            let known_to = column
                .standard_error()
                .filter(|spread| *spread > 0.0)
                .or(summary.sample_stdev.filter(|spread| *spread > 0.0));
            let decimals = known_to.map(decimals_of);
            let written = |value: f64| match (&row.precision, decimals) {
                (Some(precision), _) => number(value, Some(precision)),
                // A deviation of ten or more puts its two significant figures
                // above the point: the mean is rounded to them, not written
                // whole — `{:.0}` of a mean near 1e20 wrote the float's
                // noise, 100000000000000131072.
                (None, Some(0)) => to_the_deviation(value, known_to.unwrap_or_default(), false),
                (None, Some(decimals)) => formatting::fixed(value, decimals),
                (None, None) => number(value, None),
            };
            // A standard error is an uncertainty of the mean, rounded up, as
            // every uncertainty is.
            let spread = |value: Option<f64>, own: bool| match value {
                None => "—".to_string(),
                Some(value) if own && row.precision.is_none() && value > 0.0 => {
                    match decimals_of(value) {
                        0 => to_the_deviation(value, value, true),
                        decimals => formatting::fixed_up(value, decimals),
                    }
                }
                Some(value) if own => match &row.precision {
                    Some(precision) => formatting::spread_at(value, precision),
                    None => written(value),
                },
                Some(value) => written(value),
            };
            let value_of_column = |value: f64| match (&row.precision, row.places) {
                (None, Some(places)) => formatting::fixed(value, places),
                (precision, _) => number(value, precision.as_ref()),
            };
            vec![
                match row.not_applicable {
                    0 => format!("{}/{}", summary.count.get(), row.of),
                    // Counted apart: the value does not apply to them.
                    apart => format!("{}/{} ({apart} n/a)", summary.count.get(), row.of),
                },
                if mixed && column.weighted_mean.is_some() {
                    format!("{} (1/u²)", written(column.mean()))
                } else {
                    written(column.mean())
                },
                spread(summary.sample_stdev, false),
                spread(column.standard_error(), true),
                // A median, a minimum and a maximum are values of the column,
                // written as the column writes them: rounded to the spread of
                // the others, a minimum of 0.99 read 0. Values known to an
                // uncertainty are written to their table's decimals, where
                // the six-figure form read `12.5027` under `12.503 ± 0.004`.
                value_of_column(summary.median),
                value_of_column(summary.minimum),
                value_of_column(summary.maximum),
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
/// figure, where that lies at or above the units, to the nearest or `up`: in
/// full while that is exact in a double, in scientific notation beyond, its
/// digits the significant ones.
pub fn to_the_deviation(value: f64, deviation: f64, up: bool) -> String {
    let exponent = |number: f64| -> i32 {
        format!("{number:.1e}")
            .split_once('e')
            .and_then(|(_, exponent)| exponent.parse().ok())
            .unwrap_or(0)
    };
    // The place of the deviation's second significant figure.
    let place = (exponent(deviation) - 1).max(0);
    let scale = 10f64.powi(place);
    // Rounded up on its decimal writing, the arithmetic's last digits
    // dropped first: a standard error of exactly 120 was written 130.
    let rounded = match up {
        true => {
            formatting::fixed_up(value / scale, 0)
                .parse::<f64>()
                .unwrap_or(value / scale)
                * scale
        }
        false => (value / scale).round() * scale,
    };
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
                    property: None,
                    uncertainty: None,
                    unitless: None,
                    unit_alone: None,
                    unit_in_form: None,
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
        property: None,
        uncertainty: None,
        unitless: None,
        unit_alone: None,
        unit_in_form: None,
        precision: Some(precision.clone()),
    };
    formatting::format_value(&value, &resolved)
}

/// Each selected sample's configuration, in the selection's order.
fn configs_of<'a>(
    selected: &SampleList,
    collection: &'a SampleList,
) -> Vec<Option<&'a ProjectConfig>> {
    selected
        .iter()
        .map(|entry| {
            entry
                .path
                .as_deref()
                .map_or(collection.config(), |path| collection.config_for(path))
        })
        .collect()
}

/// The configurations once each, by their folder: at least one, `None` for
/// samples no project describes.
fn distinct<'a>(configs: &[Option<&'a ProjectConfig>]) -> Vec<Option<&'a ProjectConfig>> {
    let mut held: Vec<Option<&ProjectConfig>> = Vec::new();
    for config in configs {
        if !held
            .iter()
            .any(|seen| seen.map(ProjectConfig::root) == config.map(ProjectConfig::root))
        {
            held.push(*config);
        }
    }
    if held.is_empty() {
        held.push(None);
    }
    held
}

/// The one answer every project gives, else none.
fn agreed<T: PartialEq>(mut answers: impl Iterator<Item = Option<T>>) -> Option<T> {
    let first = answers.next()??;
    answers
        .all(|answer| answer.as_ref() == Some(&first))
        .then_some(first)
}

fn precision(
    column: &ColumnSpec,
    field: &Field,
    profile: &Profile,
    selected: &SampleList,
    config: Option<&ProjectConfig>,
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
    let declared = match config {
        Some(config) => {
            config
                .resolved_as(&composed, &address, style)
                .ok()?
                .precision
        }
        None => composed.precision,
    };
    // **A column of spreads is summarised as spreads**: rounded up, at the
    // uncertainty's own precision, as its cells are; `0.01` beside `0.001`
    // gave a mean written `0.00`.
    if fields::is_spread(fields::channel(field)) {
        return Some(match declared {
            Some(precision) => precision.of_uncertainty(),
            None => Precision::general_spread(),
        });
    }
    declared
}
