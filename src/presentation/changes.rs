//! What changed between two states of a project's history, as values:
//! `samplekit diff`'s and the workbench's history screen's one way of saying
//! it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::collection::editing::shown_value;
use crate::config::project_config::ProjectConfig;
use crate::config::version_control::Entry;
use crate::core::identifier::Identifier;
use crate::core::sample::Sample;
use crate::core::table::RowAddress;
use crate::format::document;
use crate::format::fingerprint::Freshness;
use crate::presentation::terminal_rendering as render;

fn counted_as(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// When a snapshot was taken, in the time of the machine that took it.
/// A snapshot dated before the one it follows says so: its place is its
/// chain's, and its date was the clock's mistake.
pub fn when(entry: &Entry) -> String {
    let date = chrono::DateTime::from_timestamp(entry.seconds + i64::from(entry.offset), 0)
        .map(|time| time.naive_utc().format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default();
    if entry.clock_went_back {
        format!("{date} (clock went back)")
    } else {
        date
    }
}

/// The machines that wrote a history, each once, as they are shown.
pub fn machines_of(entries: &[Entry]) -> Vec<String> {
    let mut machines: Vec<(&str, &str)> = entries
        .iter()
        .map(|entry| (entry.machine.as_str(), entry.machine_id.as_str()))
        .collect();
    machines.sort();
    machines.dedup();
    machines
        .into_iter()
        .map(|(shown, _)| shown.to_string())
        .collect()
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

/// One field's change as every surface says it: the field, what it held and
/// what it holds — each whole, its uncertainty and unit, `—` for nothing —
/// or, instead, what became of it, and what the change means, beneath.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
    /// Said in the place of `before  →  after`: `unchanged`, `+prize`.
    pub said: Option<String>,
    pub notes: Vec<String>,
}

impl FieldChange {
    pub fn new(
        field: impl Into<String>,
        before: impl Into<String>,
        after: impl Into<String>,
    ) -> Self {
        FieldChange {
            field: field.into(),
            before: before.into(),
            after: after.into(),
            said: None,
            notes: Vec::new(),
        }
    }

    /// A field whose line says what became of it rather than two values.
    pub fn said(field: impl Into<String>, said: impl Into<String>) -> Self {
        FieldChange {
            field: field.into(),
            before: String::new(),
            after: String::new(),
            said: Some(said.into()),
            notes: Vec::new(),
        }
    }
}

/// The lines of a block of changes, under their file: each field padded to
/// the block's widest, `before  →  after`, and its notes beneath, under the
/// values.
pub fn change_lines(changes: &[FieldChange]) -> Vec<String> {
    let width = changes
        .iter()
        .map(|change| render::width(&change.field))
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for change in changes {
        let padding = " ".repeat(width.saturating_sub(render::width(&change.field)));
        match &change.said {
            Some(said) => lines.push(format!("  {}{padding}   {said}", change.field)),
            None => lines.push(format!(
                "  {}{padding}   {}  →  {}",
                change.field,
                painted_side(&change.before),
                painted_side(&change.after)
            )),
        }
        // A note explains; it is muted where the palette is on.
        for note in &change.notes {
            lines.push(format!(
                "  {}   {}",
                " ".repeat(width),
                render::paint(note, render::palette().muted)
            ));
        }
    }
    lines
}

/// One side of `before  →  after`, painted where the palette is on: a number
/// bold with its unit coloured, a word that is a value an attribute's colour,
/// and what says there is nothing, `—`, as it is.
fn painted_side(text: &str) -> String {
    let palette = render::palette();
    if !palette.enabled || matches!(text, "" | "—" | "removed") {
        return text.to_string();
    }
    let said = render::painted_said(text);
    if said != text || text.contains(' ') {
        return said;
    }
    render::paint(text, palette.attribute)
}

/// Each field of a sample as a change shows it, by its address: `radius`, its
/// readings `radius.readings`, a cell `ageing.hardness[1]`, `tags`, an
/// attribute — at the precision declared, with its uncertainty and unit.
pub fn shown_fields(
    sample: &Sample,
    config: Option<&ProjectConfig>,
) -> indexmap::IndexMap<String, String> {
    match config {
        Some(config) => fields_of(sample, config, Digits::Declared),
        None => {
            let bare = crate::config::project_config::parse(
                std::path::Path::new(crate::config::project_config::FILENAME),
                "schema_version = 1\n",
            )
            .expect("a configuration of its version alone reads");
            fields_of(sample, &bare, Digits::Declared)
        }
    }
}

/// Changes as a preview built them, made whole: each value as the sample
/// held it and holds it now — with its uncertainty and unit, at the declared
/// precision — where the field is one `shown_fields` names; a channel said as
/// its quantity, `radius.u` as `radius`; what a line said beside a value kept as a
/// note; two changes of one quantity one. A cell whose row moved, its index
/// changed, is said as it was built.
pub fn made_whole(
    changes: Vec<FieldChange>,
    before: Option<&Sample>,
    after: &Sample,
    config: Option<&ProjectConfig>,
) -> Vec<FieldChange> {
    let was = before
        .map(|sample| shown_fields(sample, config))
        .unwrap_or_default();
    let is = shown_fields(after, config);
    // Every digit, where the precision writes two values alike.
    let every = |sample: &Sample| match config {
        Some(config) => fields_of(sample, config, Digits::Every),
        None => shown_fields(sample, None),
    };
    let (was_whole, is_whole) = (before.map(every).unwrap_or_default(), every(after));
    let key_of = |field: &str| -> Option<String> {
        if is.contains_key(field) || was.contains_key(field) {
            return Some(field.to_string());
        }
        let (base, channel) = field.rsplit_once('.')?;
        (matches!(channel, "u" | "v" | "unit" | "symbol")
            && (is.contains_key(base) || was.contains_key(base)))
        .then(|| base.to_string())
    };
    let mut made: Vec<FieldChange> = Vec::new();
    for mut change in changes {
        let field = change.field.clone();
        let moved = field.contains('[') && !(was.contains_key(&field) && is.contains_key(&field));
        if let Some(key) = key_of(&field).filter(|_| !moved) {
            let shown = |side: &indexmap::IndexMap<String, String>| {
                side.get(&key).cloned().unwrap_or_else(|| "—".to_string())
            };
            let (mut whole_before, mut whole_after) = (shown(&was), shown(&is));
            // Two values the precision writes alike are a change all the
            // same, said with every digit.
            if (whole_before == whole_after
                || exaggerated(
                    &whole_before,
                    &whole_after,
                    &shown(&was_whole),
                    &shown(&is_whole),
                ))
                && shown(&was_whole) != shown(&is_whole)
            {
                (whole_before, whole_after) = apart(&shown(&was_whole), &shown(&is_whole));
            }
            match change.said.take() {
                // Unchanged: its value, whole.
                Some(said) if said.ends_with("unchanged") => {
                    change.said = Some(format!("{whole_after}  unchanged"));
                }
                // What became of it, said beside it: a note now, under what
                // it was and what it is.
                Some(said) => {
                    // Said again beneath what it is now, it would say it twice
                    // — `1.050` under `1.05` too: the same numbers.
                    if said != whole_after && !same_numbers(&said, &whole_after) {
                        change.notes.insert(0, said);
                    }
                    change.before = whole_before;
                    change.after = whole_after;
                }
                None => {
                    // What the file does not write — a name it takes from its
                    // file — is said as the line said it.
                    let whole_before = if whole_before == "—" && !change.before.is_empty() {
                        change.before.clone()
                    } else {
                        whole_before
                    };
                    // `removed`, and what a line said in brackets, stay said.
                    if change.after != "removed" {
                        if let Some((said, aside)) = change.after.split_once(" (") {
                            change
                                .notes
                                .insert(0, aside.trim_end_matches(')').to_string());
                            change.after = said.to_string();
                        }
                        // A number typed is shown as a table shows it, at its
                        // uncertainty's decimals, `25.0` as `25.000 ± 0.053`;
                        // where that would round it, every digit typed, the
                        // table's decimals at least: a number copied from a
                        // spreadsheet is checked against what was typed.
                        let typed = change.after.parse::<f64>().ok().filter(|_| key == field);
                        let (number, rest) = whole_after
                            .split_once(' ')
                            .map_or((whole_after.as_str(), None), |(number, rest)| {
                                (number, Some(rest))
                            });
                        if let (Some(typed), Ok(shown)) = (typed, number.parse::<f64>()) {
                            if shown == typed {
                                change.after = whole_after;
                            } else if let Some(rest) = rest {
                                let decimals = |text: &str| {
                                    text.split_once('.').map_or(0, |(_, after)| after.len())
                                };
                                let places = decimals(&change.after).max(decimals(number));
                                let written = if decimals(&change.after) < places {
                                    format!("{typed:.places$}")
                                } else {
                                    change.after.clone()
                                };
                                change.after = format!("{written} {rest}");
                            }
                        } else if whole_after != "—"
                            || change.after.is_empty()
                            || change.after == "—"
                        {
                            change.after = whole_after;
                        }
                    }
                    change.before = whole_before;
                }
            }
            change.field = key;
        }
        match made
            .iter_mut()
            .find(|held| held.field == change.field && held.said.is_none() && change.said.is_none())
        {
            Some(held) => {
                held.after = change.after;
                held.notes.extend(change.notes);
            }
            None => made.push(change),
        }
    }
    made
}

/// Whether two texts are the same list of numbers, however written:
/// `1.050, 1.051` and `1.05, 1.051`.
fn same_numbers(a: &str, b: &str) -> bool {
    let numbers = |text: &str| -> Option<Vec<f64>> {
        text.split(',')
            .map(|part| part.trim().parse::<f64>().ok())
            .collect()
    };
    matches!((numbers(a), numbers(b)), (Some(x), Some(y)) if x == y)
}

/// How many cells a state said of a value stands for: a table's column says
/// its rows, `(13 rows)`, and anything else is one.
pub fn cells_in(said: &str) -> usize {
    said.rsplit_once('(')
        .and_then(|(_, rest)| rest.strip_suffix(')'))
        .and_then(|inside| {
            inside
                .strip_suffix(" rows")
                .or_else(|| inside.strip_suffix(" row"))
        })
        .and_then(|count| count.trim().parse().ok())
        .unwrap_or(1)
}

/// What a change makes not current, as every surface says it: each value
/// with its state, padded to the widest, a row's input said by its table's
/// column; where nothing, one sentence, and how many were already.
pub fn not_current_lines(newly: &[(String, String)], already: usize) -> Vec<String> {
    if newly.is_empty() {
        return vec![match already {
            0 => "  nothing becomes not current".to_string(),
            1 => "  nothing more becomes not current — 1 value was already".to_string(),
            n => format!("  nothing more becomes not current — {n} values were already"),
        }];
    }
    let width = newly
        .iter()
        .map(|(name, _)| render::width(name))
        .max()
        .unwrap_or(0);
    // Counted by cells, as every surface counts them.
    let cells: usize = newly.iter().map(|(_, said)| cells_in(said)).sum();
    let mut lines = vec![format!(
        "  this makes {} not current",
        if cells == 1 {
            "1 value".to_string()
        } else {
            format!("{cells} values")
        }
    )];
    for (name, said) in newly {
        // `row.hardness` is the model's word for this row's cell: said as the
        // column it is, `ageing.hardness`.
        // The value's own name: one said in its file, `samples/b1.md
        // tests.ratio`, is the last word.
        let value = name.rsplit("  ").next().unwrap_or(name);
        let said = match value.split_once('.') {
            Some((table, _)) => said.replace("row.", &format!("{table}.")),
            None => said.clone(),
        };
        let padding = " ".repeat(width.saturating_sub(render::width(name)));
        lines.push(format!("    {name}{padding}   {said}"));
    }
    lines
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

/// A sample a script made, as its preview says it: `new sample`, then what it
/// holds, each field from nothing, as `new` previews one — `new sample` alone
/// said none of the values the script gave it. Its empty fields and its note
/// are left out.
pub fn made_of(name: &str, after: &[u8], config: &ProjectConfig) -> Vec<String> {
    let mut lines = vec![format!("{name}   new sample")];
    let Some(new) = sample_of(after) else {
        return lines;
    };
    let changes: Vec<FieldChange> = fields_of(&new, config, Digits::Declared)
        .into_iter()
        .filter(|(field, value)| field != NOTE && value != "—")
        .map(|(field, value)| FieldChange::new(field, "—", value))
        .collect();
    lines.extend(change_lines(&changes));
    lines
}

/// The same lines, among the fields `among` accepts by name — `radius` for `radius` and
/// `radius.readings`, `ageing` for any of its cells, `tags` — the note never:
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

/// The name a field is held under: `radius` for `radius.u`, `ageing` for
/// `ageing.hardness[3]`, what a sample's file holds it as.
pub fn field_base(field: &str) -> &str {
    let field = field.trim_start_matches('-');
    let end = field.find(['.', '[']).unwrap_or(field.len());
    &field[..end]
}

/// What each value of a kept sample is computed from, as its records say:
/// `(recovery, ore_mass)`, `(ageing, radius)` for a cell of that table,
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
                InputName::Cell(_) | InputName::CellValue(_) => what.to_string(),
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
                // both, and was said `notch   —  →  —`.
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
                        Some(if old == new || exaggerated(&old, &new, &was, &is) {
                            let (was, is) = apart(&was, &is);
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
                    let changes: Vec<FieldChange> = changed
                        .into_iter()
                        .map(|(field, old, new)| {
                            if field == NOTE {
                                let (added, removed) =
                                    lines_changed(old.as_bytes(), new.as_bytes());
                                FieldChange::said(
                                    field,
                                    format!("changed, +{added} −{removed} lines"),
                                )
                            } else {
                                FieldChange::new(field, old, new)
                            }
                        })
                        .collect();
                    lines.extend(change_lines(&changes));
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

/// Whether a value is said with every digit its file holds, past what a reader
/// tells apart: a number with more than eight decimals.
pub fn every_digit(said: &str) -> bool {
    said.split(' ').any(|word| {
        word.parse::<f64>().is_ok()
            && word
                .split_once('.')
                .is_some_and(|(_, decimals)| decimals.len() > 8)
    })
}

/// Whether two values as shown, `207 → 208`, say a change far larger than
/// the one between them whole, `207.48 → 207.53`: their difference is less
/// than half a unit of the last digit shown, which rounding shows as a whole
/// unit. Said whole then, as values shown
/// alike are.
pub fn exaggerated(shown_was: &str, shown_is: &str, whole_was: &str, whole_is: &str) -> bool {
    let first = |said: &str| {
        said.split(' ').find_map(|word| {
            word.parse::<f64>()
                .ok()
                .map(|number| (word.to_string(), number))
        })
    };
    // One unit of the last digit shown: `207` 1, `1.02` 0.01, `2.08e+03` 10.
    let unit = |shown: &str| {
        let lower = shown.to_ascii_lowercase();
        let (mantissa, exponent) = match lower.split_once('e') {
            Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().unwrap_or(0)),
            None => (lower.as_str(), 0),
        };
        let decimals = mantissa.split_once('.').map_or(0, |(_, after)| after.len()) as i32;
        10f64.powi(exponent - decimals)
    };
    let (Some((was_shown, _)), Some((is_shown, _))) = (first(shown_was), first(shown_is)) else {
        return false;
    };
    let (Some((_, was)), Some((_, is))) = (first(whole_was), first(whole_is)) else {
        return false;
    };
    // The finer of the two: `20.1235 → 20` shows its move.
    let step = unit(&was_shown).min(unit(&is_shown));
    let difference = (was - is).abs();
    // Less than half a step, which rounding shows as a whole one.
    difference > 0.0 && difference < step / 2.0
}

/// Two values the declared precision writes alike, said with the fewest
/// digits that tell them apart — `1.0117 → 1.0123` — never every digit the
/// file holds, `1.0116666666666665 ± 0.0003333333333333707`. Each number of
/// one beside its fellow of the other: one that differs only past twelve
/// significant digits — an uncertainty taken again from the same spread — is
/// the same number, said at two significant digits.
pub fn apart(was: &str, is: &str) -> (String, String) {
    let (old, new): (Vec<&str>, Vec<&str>) = (was.split(' ').collect(), is.split(' ').collect());
    if old.len() != new.len() {
        return (was.to_string(), is.to_string());
    }
    let mut said_old = Vec::with_capacity(old.len());
    let mut said_new = Vec::with_capacity(new.len());
    for (at, (a, b)) in old.iter().zip(&new).enumerate() {
        // A number after `±` is an uncertainty: rounded up, never understated.
        let spread = at > 0 && old[at - 1] == "\u{b1}";
        match (a.parse::<f64>(), b.parse::<f64>()) {
            _ if a == b => {
                said_old.push((*a).to_string());
                said_new.push((*b).to_string());
            }
            (Ok(x), Ok(y)) if x.is_finite() && y.is_finite() => {
                let (x_said, y_said) = numbers_apart(x, y, spread);
                said_old.push(x_said);
                said_new.push(y_said);
            }
            _ => {
                said_old.push((*a).to_string());
                said_new.push((*b).to_string());
            }
        }
    }
    (said_old.join(" "), said_new.join(" "))
}

/// `x` and `y` at the fewest decimals that tell them apart, both alike, on
/// their shortest decimal writing: to the nearest, or `up` for a spread.
fn numbers_apart(x: f64, y: f64, up: bool) -> (String, String) {
    let magnitude = x.abs().max(y.abs());
    // Decimals for two significant digits of the larger.
    let lead = if magnitude > 0.0 {
        (1 - magnitude.log10().floor() as i32).max(0) as usize
    } else {
        1
    };
    // At least to the place of their difference's first digit: rounded
    // before it, 207.48 → 207.53 was said 207 → 208, a jump of one.
    // A difference past twelve significant digits is a computation's noise,
    // not a change: it says nothing of where the numbers part.
    let difference = (x - y).abs();
    let lead = if difference > magnitude * 1e-12 {
        lead.max((-difference.log10().floor()).max(0.0) as usize)
    } else {
        lead
    };
    let at = |decimals: usize, number: f64| match up {
        true => crate::core::formatting::fixed_up(number, decimals),
        false => crate::core::formatting::fixed(number, decimals),
    };
    for decimals in lead..=lead + 10 {
        if at(decimals, x) != at(decimals, y) {
            return (at(decimals, x), at(decimals, y));
        }
    }
    (at(lead, x), at(lead, y))
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
pub fn sample_of(bytes: &[u8]) -> Option<Sample> {
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
            // In brackets: a list of one is not its single value.
            let shown = crate::collection::editing::shown_attribute(value);
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
        // Where the project declares no precision, as a table shows it: every
        // digit said the noise a computation leaves. Two values shown alike are
        // told apart by their caller, at the fewest digits.
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
    if let Some(join) = of_join(config, entries, at, &holds)? {
        let mut lines: Vec<String> = join.own.into_iter().flat_map(|(_, lines)| lines).collect();
        if !join.came.is_empty() {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(format!(
                "came with the join, from {}:",
                join.from.join(", ")
            ));
            lines.extend(
                join.came
                    .into_iter()
                    .flat_map(|(_, lines)| lines)
                    .map(|line| format!("  {line}")),
            );
        }
        return Ok(lines);
    }
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

/// What a join changed, compared with this machine's last snapshot, its first
/// parent: a value set back to what this machine held is no change of it.
pub struct JoinChanges {
    /// Where its first parent is in the entries.
    pub first: usize,
    /// What the join itself changed, file by file.
    pub own: Vec<(String, Vec<String>)>,
    /// What came from the other machines' snapshots and stayed, file by file.
    pub came: Vec<(String, Vec<String>)>,
    /// Those snapshots, `#2 of tom`.
    pub from: Vec<String>,
}

/// The changes of the snapshot at `at` where it is a join; `None` where it is
/// not, or its first parent is not among the entries.
pub fn of_join(
    config: &ProjectConfig,
    entries: &[Entry],
    at: usize,
    holds: &dyn Fn(&str) -> bool,
) -> Result<Option<JoinChanges>, crate::config::version_control::VcsError> {
    use crate::config::version_control as history;
    let Some(entry) = entries.get(at) else {
        return Ok(None);
    };
    let place = |id: &String| entries.iter().position(|entry| entry.id == *id);
    let (Some(first), true) = (
        entry.parents.first().and_then(place),
        entry.parents.len() > 1,
    ) else {
        return Ok(None);
    };
    let mine = history::files_at(config, &entries[first].id)?;
    let arrived = history::files_before(config, &entry.id)?;
    let after = history::files_at(config, &entry.id)?;
    let mut names: Vec<&String> = mine
        .keys()
        .chain(after.keys())
        .filter(|name| holds(name))
        .collect();
    names.sort();
    names.dedup();
    let (mut own, mut came) = (Vec::new(), Vec::new());
    for name in names {
        let all = changes_of(name, mine.get(name), after.get(name), config);
        let theirs = changes_of(name, mine.get(name), arrived.get(name), config);
        let (header, body): (Vec<String>, Vec<String>) = match all.split_first() {
            Some((first, rest)) if first == name && !rest.is_empty() => {
                (vec![first.clone()], rest.to_vec())
            }
            _ => (Vec::new(), all.clone()),
        };
        // Told apart by their words: each set aligns its lines on its own
        // longest name, and a value computed here — `recovery`, longer than
        // `volume` — made every line the other machine brought differ by its
        // spaces, all of them said this machine's.
        let words = |line: &str| line.split_whitespace().collect::<Vec<_>>().join(" ");
        let theirs: Vec<String> = theirs.iter().map(|line| words(line)).collect();
        let (from_them, from_here): (Vec<String>, Vec<String>) = body
            .into_iter()
            .partition(|line| theirs.contains(&words(line)));
        for (lines, into) in [(from_here, &mut own), (from_them, &mut came)] {
            match (header.is_empty(), lines.as_slice()) {
                (_, []) => {}
                (false, [one]) => {
                    into.push((name.clone(), vec![format!("{name}   {}", one.trim_start())]));
                }
                _ => into.push((name.clone(), [header.clone(), lines].concat())),
            }
        }
    }
    let from = entry.parents[1..]
        .iter()
        .filter_map(place)
        .map(|other| format!("#{} of {}", other + 1, entries[other].machine))
        .collect();
    Ok(Some(JoinChanges {
        first,
        own,
        came,
        from,
    }))
}

/// A state in the words `status` writes it: whole, each input named.
pub fn state_said(freshness: &Freshness) -> String {
    match freshness {
        Freshness::RecordMissing => {
            "record missing — --force gives it back to its formula".to_string()
        }
        Freshness::Source => "entered".to_string(),
        Freshness::Current => "current".to_string(),
        // One sentence for it on every surface: typed over its formula.
        Freshness::Edited => "edited — typed over its formula".to_string(),
        Freshness::Failed { message } => format!("failed — {message}"),
        Freshness::Unjudged { reason } => format!("not judged — {reason}"),
        Freshness::Stale { changed, upstream } => {
            let mut parts: Vec<String> = changed.iter().map(|name| name.to_string()).collect();
            // *Not current*, the word that covers what an upstream input can
            // be — outdated, broken, failed: a failed input was called outdated two
            // lines below the line calling it failed.
            parts.extend(
                upstream
                    .iter()
                    .map(|name| format!("{name} (itself not current)")),
            );
            format!("outdated — {}", parts.join(", "))
        }
        Freshness::Broken {
            missing,
            changed_kind,
        } => {
            let mut parts: Vec<String> = missing
                .iter()
                .map(|name| format!("{name} is gone"))
                .collect();
            parts.extend(
                changed_kind
                    .iter()
                    .map(|name| format!("{name} changed kind")),
            );
            format!("broken — {}", parts.join(", "))
        }
    }
}

/// A traceback's error line: its last, the note a failure carries after it,
/// *while computing 'area'*, left out, as the line beside a ✗ says why.
pub fn error_line(traceback: &str) -> String {
    traceback
        .trim_end()
        .lines()
        .rev()
        .find(|line| !line.starts_with("while computing '"))
        .unwrap_or_default()
        .to_string()
}

/// An outdated state whose every reason is a value that waits, said as
/// waiting behind it, *waits for curve.stress, which waits*, its rows kept;
/// `None` for any other.
pub fn behind_waiting(said: &str, waiting: &std::collections::HashSet<String>) -> Option<String> {
    let reasons = said.strip_prefix("outdated — ")?;
    let rows = cells_in(said);
    let reasons = match reasons.rsplit_once(" (") {
        Some((before, after)) if after.ends_with("rows)") || after.ends_with("row)") => before,
        _ => reasons,
    };
    let upstream: Vec<&str> = reasons
        .split(", ")
        .map(|part| part.strip_suffix(" (itself not current)"))
        .collect::<Option<_>>()?;
    if upstream.is_empty() || !upstream.iter().all(|name| waiting.contains(*name)) {
        return None;
    }
    // As `compute` names an input that failed, each its own: *x, which
    // waits; y, which waits*.
    let mut written = format!(
        "waits for {}",
        upstream
            .iter()
            .map(|name| format!("{name}, which waits"))
            .collect::<Vec<_>>()
            .join("; ")
    );
    if rows > 1 {
        written.push_str(&format!(" ({rows} rows)"));
    }
    Some(written)
}
