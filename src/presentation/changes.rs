//! What changed between two states of a project's history, as values:
//! `samplekit diff`'s and the workbench's history screen's one way of saying
//! it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::collection::editing::shown_value;
use crate::config::project_config::ProjectConfig;
use crate::config::version_control::Entry;
use crate::core::identifier::Identifier;
use crate::core::sample::{AttributeValue, Sample};
use crate::core::table::RowAddress;
use crate::format::document;
use crate::presentation::terminal_rendering as render;

fn counted_as(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// When a snapshot was taken, in the time of the machine that took it.
pub fn when(entry: &Entry) -> String {
    chrono::DateTime::from_timestamp(entry.seconds + i64::from(entry.offset), 0)
        .map(|time| time.naive_utc().format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// The machines that wrote a history, each once, by name.
pub fn machines_of(entries: &[Entry]) -> Vec<String> {
    let mut machines: Vec<String> = entries.iter().map(|entry| entry.machine.clone()).collect();
    machines.sort();
    machines.dedup();
    machines
}

/// The machine that took a snapshot, as `log` says it where more than one
/// wrote the history: its name, and the machines it joined.
pub fn machine_said(entry: &Entry) -> String {
    if entry.joined.is_empty() {
        entry.machine.clone()
    } else {
        format!("{}, joins {}", entry.machine, entry.joined.join(", "))
    }
}

/// What a snapshot changed, as a line of `log` says it: samples by name, a
/// count past three, then the other files.
pub fn changed_said(names: &[&String]) -> String {
    let (samples, others): (Vec<&String>, Vec<&String>) = names
        .iter()
        .partition(|name| name.ends_with(".md") && name.as_str() != ".samplekitrc");
    let mut said = Vec::new();
    if samples.len() > 3 {
        said.push(counted_as(samples.len(), "sample", "samples"));
    } else {
        said.extend(samples.iter().map(|name| {
            Path::new(name.as_str())
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default()
        }));
    }
    // Besides the samples, only the configuration and the model are kept.
    let (configuration, model): (Vec<&String>, Vec<&String>) = others
        .iter()
        .partition(|name| name.as_str() == ".samplekitrc");
    said.extend(configuration.iter().map(|name| name.to_string()));
    match model.as_slice() {
        [] => {}
        [one] => said.push(one.to_string()),
        many => said.push(format!("the model ({} files)", many.len())),
    }
    said.join(", ")
}

/// What changed in one kept file, as the lines `diff` prints for it: none
/// where nothing did.
pub fn changes_of(
    name: &str,
    before: Option<&Vec<u8>>,
    after: Option<&Vec<u8>>,
    config: &ProjectConfig,
) -> Vec<String> {
    changes(name, before, after, config, Digits::Declared, None)
}

/// The same lines, among the fields `among` accepts by name — `og` for `og` and
/// `og.readings`, `fermentation` for any of its cells, `tags` — the note never:
/// what changed of what an export writes, and of what that is computed from.
/// None where nothing it accepts changed, a sample whose other values changed
/// included.
pub fn changes_among(
    name: &str,
    before: Option<&Vec<u8>>,
    after: Option<&Vec<u8>>,
    config: &ProjectConfig,
    among: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    changes(name, before, after, config, Digits::Declared, Some(among))
}

/// The name a field is held under: `og` for `og.u`, `fermentation` for
/// `fermentation.gravity[3]`, what a sample's file holds it as.
pub fn field_base(field: &str) -> &str {
    let field = field.trim_start_matches('-');
    let end = field.find(['.', '[']).unwrap_or(field.len());
    &field[..end]
}

/// What each value of a kept sample is computed from, as its records say:
/// `(efficiency, grain_mass)`, `(fermentation, og)` for a cell of that table,
/// each by the name it is held under. Empty for a file that does not read.
pub fn computed_from(bytes: &[u8]) -> Vec<(String, String)> {
    use crate::core::property::InputName;
    let Some(sample) = sample_of(bytes) else {
        return Vec::new();
    };
    let mut edges = Vec::new();
    let mut read = |what: &str, inputs: Option<&indexmap::IndexMap<InputName, _>>| {
        for input in inputs.into_iter().flat_map(|inputs| inputs.keys()) {
            let from = match input {
                InputName::Named(name) => name.to_string(),
                InputName::Cell(_) => what.to_string(),
                InputName::Column { table, .. } => table.to_string(),
            };
            edges.push((what.to_string(), from));
        }
    };
    for name in sample.property_names() {
        if let Ok(handle) = sample.property(name) {
            read(name.as_str(), handle.records().computed.as_ref());
        }
    }
    for name in sample.table_names() {
        let Ok(table) = sample.table(name) else {
            continue;
        };
        let columns: Vec<Identifier> = table.column_names().into_iter().cloned().collect();
        for row in table.rows() {
            for column in &columns {
                if let Ok(cell) = row.cell(column) {
                    read(name.as_str(), cell.records().computed.as_ref());
                }
            }
        }
    }
    edges
}

/// How a number is said: at the precision the project declares, or whole.
#[derive(Clone, Copy)]
enum Digits {
    Declared,
    Every,
}

fn changes(
    name: &str,
    before: Option<&Vec<u8>>,
    after: Option<&Vec<u8>>,
    config: &ProjectConfig,
    digits: Digits,
    among: Option<&dyn Fn(&str) -> bool>,
) -> Vec<String> {
    if before == after {
        return Vec::new();
    }
    let sample = name.ends_with(".md") && name != ".samplekitrc";
    match (before, after) {
        (None, Some(_)) => vec![format!(
            "{name}   {}",
            if sample { "new sample" } else { "new" }
        )],
        (Some(_), None) => vec![format!("{name}   removed")],
        (Some(before), Some(after)) => {
            if sample && let (Some(old), Some(new)) = (sample_of(before), sample_of(after)) {
                // Every digit decides what changed; the digits asked for
                // say it. Two values the precision writes alike are still a
                // change, said whole: `1.066  →  1.066` was said *changed
                // only what SampleKit records* of a value that moved.
                let (whole_old, whole_new) = (
                    fields_of(&old, config, Digits::Every),
                    fields_of(&new, config, Digits::Every),
                );
                let old = fields_of(&old, config, digits);
                let new = fields_of(&new, config, digits);
                // In the file's order, the note last; a field only the new
                // version holds after those of the old.
                let mut keys: Vec<&String> = old.keys().collect();
                keys.extend(new.keys().filter(|key| !old.contains_key(*key)));
                keys.sort_by_key(|key| key.as_str() == NOTE);
                // Compared as they are shown whole: a property absent from
                // one side and held without a value on the other is `—` on
                // both, and was said `fg   —  →  —`.
                let changed: Vec<(String, String, String)> = keys
                    .into_iter()
                    .filter_map(|key| {
                        let shown = |side: &indexmap::IndexMap<String, String>| {
                            side.get(key).cloned().unwrap_or_else(|| "—".to_string())
                        };
                        let (was, is) = (shown(&whole_old), shown(&whole_new));
                        if was == is {
                            return None;
                        }
                        let (old, new) = (shown(&old), shown(&new));
                        Some(if old == new {
                            (key.clone(), was, is)
                        } else {
                            (key.clone(), old, new)
                        })
                    })
                    .filter(|(key, _, _)| {
                        among.is_none_or(|among| key != NOTE && among(field_base(key)))
                    })
                    .collect();
                // Nothing of what was asked about changed: no line at all,
                // where *changed only what SampleKit records* is said of the
                // whole sample.
                if among.is_some() && changed.is_empty() {
                    return Vec::new();
                }
                let mut lines = vec![name.to_string()];
                if changed.is_empty() {
                    lines.push(format!("  {RECORDS_ONLY}"));
                } else {
                    let width = changed
                        .iter()
                        .map(|(field, _, _)| render::width(field))
                        .max()
                        .unwrap_or(0);
                    for (field, old, new) in changed {
                        let padding = " ".repeat(width.saturating_sub(render::width(&field)));
                        if field == NOTE {
                            let (added, removed) = lines_changed(old.as_bytes(), new.as_bytes());
                            lines.push(format!(
                                "  {field}{padding}   changed, +{added} −{removed} lines"
                            ));
                        } else {
                            lines.push(format!("  {field}{padding}   {old}  →  {new}"));
                        }
                    }
                }
                lines
            } else {
                let (added, removed) = lines_changed(before, after);
                vec![format!("{name}   changed, +{added} −{removed} lines")]
            }
        }
        (None, None) => Vec::new(),
    }
}

/// Lines added and removed, counted as a multiset: enough to say how much a
/// configuration or a model changed, which `git diff` shows in full.
pub fn lines_changed(before: &[u8], after: &[u8]) -> (usize, usize) {
    let count = |text: &[u8]| {
        let mut counted: BTreeMap<String, isize> = BTreeMap::new();
        for line in String::from_utf8_lossy(text).lines() {
            *counted.entry(line.to_string()).or_insert(0) += 1;
        }
        counted
    };
    let (old, new) = (count(before), count(after));
    let lines: std::collections::BTreeSet<&String> = old.keys().chain(new.keys()).collect();
    let mut added = 0;
    let mut removed = 0;
    for line in lines {
        let difference = new.get(line).copied().unwrap_or(0) - old.get(line).copied().unwrap_or(0);
        if difference > 0 {
            added += difference.unsigned_abs();
        } else {
            removed += difference.unsigned_abs();
        }
    }
    (added, removed)
}

/// A kept file read as the sample it held, where it holds one.
fn sample_of(bytes: &[u8]) -> Option<Sample> {
    let text = std::str::from_utf8(bytes).ok()?;
    let document = document::parse(text).ok()?;
    let mut sample = crate::format::schema::into_sample(document.schema).ok()?;
    sample.set_note(document.note.as_str().to_string());
    Some(sample)
}

/// Every field of a sample, each as `diff` shows it: tags, attributes,
/// properties and their readings, table cells, and the note by its lines.
fn fields_of(
    sample: &Sample,
    config: &ProjectConfig,
    digits: Digits,
) -> indexmap::IndexMap<String, String> {
    let mut fields = indexmap::IndexMap::new();
    // The name the file writes, which a user edits as any attribute.
    fields.insert(
        "name".to_string(),
        sample.written_name().unwrap_or("—").to_string(),
    );
    let tags: Vec<String> = sample.tags().iter().map(ToString::to_string).collect();
    fields.insert(
        "tags".to_string(),
        if tags.is_empty() {
            "—".to_string()
        } else {
            tags.join(", ")
        },
    );
    for name in sample.attribute_names() {
        if let Ok(value) = sample.attribute(name) {
            let shown = match value {
                AttributeValue::Scalar(value) => shown_value(value),
                AttributeValue::List(values) => values
                    .iter()
                    .map(shown_value)
                    .collect::<Vec<_>>()
                    .join(", "),
            };
            fields.insert(name.to_string(), shown);
        }
    }
    for name in sample.property_names() {
        let Ok(handle) = sample.property(name) else {
            continue;
        };
        let (shown, readings) = handle.peek(|property| {
            (
                quantity_of(property, name.as_str(), config, digits),
                property
                    .readings()
                    .map(|readings| readings_shown(readings.as_slice())),
            )
        });
        fields.insert(name.to_string(), shown);
        if let Some(readings) = readings {
            fields.insert(format!("{name}.readings"), readings);
        }
    }
    for name in sample.table_names() {
        let Ok(table) = sample.table(name) else {
            continue;
        };
        let columns: Vec<Identifier> = table.column_names().into_iter().cloned().collect();
        for row in table.rows() {
            let index: Vec<String> = row.index().into_iter().map(shown_value).collect();
            let index = index.join(", ");
            let at = RowAddress::index(row.index().into_iter().cloned().collect::<Vec<_>>());
            for column in &columns {
                if let Ok(cell) = row.cell(column) {
                    let address = format!("{name}.{column}");
                    // The unit is the column's, which its cells do not repeat.
                    let presentation = table
                        .presentation_of(column, &at)
                        .unwrap_or_else(|_| cell.presentation().clone());
                    fields.insert(
                        format!("{address}[{index}]"),
                        shown_quantity(cell, &presentation, &address, config, digits),
                    );
                    // A cell's readings, as a property's: a reading corrected
                    // was said *changed only what SampleKit records*.
                    if let Some(readings) = cell.readings() {
                        fields.insert(
                            format!("{address}[{index}].readings"),
                            readings_shown(readings.as_slice()),
                        );
                    }
                }
            }
        }
    }
    // Kept whole: `changes_of` says a note by the lines it gained and lost.
    let note = sample.note().trim();
    if !note.is_empty() {
        fields.insert(NOTE.to_string(), note.to_string());
    }
    fields
}

/// Readings as `diff` shows them: each as the file writes it.
fn readings_shown(readings: &[f64]) -> String {
    readings
        .iter()
        .map(|reading| format!("{reading}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// What a sample whose file changed with no value changed is said to have
/// changed: its records — fingerprints, states — alone.
pub const RECORDS_ONLY: &str = "changed only what SampleKit records about its values";

/// The note's key among the fields, which no field can be: a name holds no
/// space.
const NOTE: &str = "the note";

/// A property as the project shows it: its value, uncertainty and unit, at
/// the precision it declares.
fn quantity_of(
    property: &crate::core::property::Property,
    address: &str,
    config: &ProjectConfig,
    digits: Digits,
) -> String {
    shown_quantity(property, property.presentation(), address, config, digits)
}

fn shown_quantity(
    property: &crate::core::property::Property,
    presentation: &crate::core::formatting::Presentation,
    address: &str,
    config: &ProjectConfig,
    digits: Digits,
) -> String {
    let value = property.value().ok().filter(|value| !value.is_absent());
    match (value, digits) {
        (None, _) => "—".to_string(),
        // The number as the file writes it, its uncertainty too, and the
        // unit as the file writes it.
        (Some(value), Digits::Every) => {
            let mut said = shown_value(&value);
            if let Some(uncertainty) = property.uncertainty().ok().flatten() {
                said.push_str(&format!(" ± {}", uncertainty.magnitude()));
            }
            if let Some(unit) = &presentation.unit {
                said.push_str(&format!(" {unit}"));
            }
            said
        }
        // Where the project declares no precision, every digit: the six figures
        // a table falls back to hid what the file holds.
        (Some(_), Digits::Declared) if !declares_precision(presentation, address, config) => {
            shown_quantity(property, presentation, address, config, Digits::Every)
        }
        (Some(value), Digits::Declared) => render::quantity(
            &value,
            property.uncertainty().ok().flatten().as_ref(),
            presentation,
            address,
            Some(config),
            None,
        ),
    }
}

/// Whether a precision is declared for the quantity at `address`: in the
/// project, where a precision is declared.
fn declares_precision(
    presentation: &crate::core::formatting::Presentation,
    address: &str,
    config: &ProjectConfig,
) -> bool {
    presentation.precision.is_some()
        || config
            .property(address)
            .and_then(|declared| declared.precision.as_ref())
            .and_then(crate::format::schema::PrecisionSchema::precision)
            .is_some()
}

/// What a snapshot changed from the state it was taken over, as `diff` says it, over
/// the files `holds` keeps: the lines the workbench's history shows beside the
/// snapshot.
pub fn of_snapshot(
    config: &ProjectConfig,
    entries: &[Entry],
    at: usize,
    holds: impl Fn(&str) -> bool,
) -> Result<Vec<String>, crate::config::version_control::VcsError> {
    use crate::config::version_control as history;
    let Some(entry) = entries.get(at) else {
        return Ok(Vec::new());
    };
    let after = history::files_at(config, &entry.id)?;
    // The state it was taken over, which may be another machine's too : not the
    // snapshot listed below it.
    let before = history::files_before(config, &entry.id)?;
    let mut lines = Vec::new();
    for name in entry.changed.iter().filter(|name| holds(name)) {
        lines.extend(changes_of(name, before.get(name), after.get(name), config));
    }
    Ok(lines)
}
