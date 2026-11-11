//! A value changed by hand: what a typed text is, what the value becomes, what
//! readings it keeps, a new row, and what the change makes not current — the
//! rule every surface shares. Nothing here writes a file.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::collection::sample_list::SampleList;
use crate::config::project_config::PropertyDeclaration;
use crate::core::formatting::Presentation;
use crate::core::identifier::Identifier;
use crate::core::property::{Produced, Property, Records};
use crate::core::sample::{AttributeValue, PropertyHandle, Sample};
use crate::core::table::{ColumnMeta, RowAddress, Table};
use crate::core::uncertainty::Uncertainty;
use crate::core::value::{self, Readings, Value};
use crate::format::fingerprint::{self, Freshness};
use crate::format::schema;
use crate::query::field_addressing::{self as fields, Channel, Field, Subject};

// ------------------------------------------------------------------ types

/// A change refused, in the words that say why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditError {
    pub message: String,
}

impl EditError {
    fn new(message: impl Into<String>) -> EditError {
        EditError {
            message: message.into(),
        }
    }
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EditError {}

/// What one assignment made of its field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Became {
    Unchanged,
    Changed,
    /// A value a formula owns, set by hand: the formula stays.
    Override,
    Cleared,
    /// A name the project declares, written as the quantity it is.
    NewQuantity,
    /// A name nothing declares; `numeric` when a number was written as one.
    NewAttribute {
        numeric: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub field: String,
    pub before: Option<Value>,
    pub after: Option<Value>,
    pub became: Became,
    /// Readings a scalar was written beside, which stay.
    pub kept: Option<Kept>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Kept {
    pub readings: Vec<f64>,
    pub uncertainty: Option<f64>,
    /// Whether the file records the statistic the readings declare: only then
    /// does `compute --force` give the value back to it. Without one, nothing
    /// says which number the readings stood for.
    pub statistic: bool,
}

/// A table's shape, borrowed from another sample.
pub struct Lent {
    pub from: PathBuf,
    pub table: Table,
    pub derived: Vec<Identifier>,
    /// Each derived column's records, from a row of the sample lending it:
    /// what a hand value in that column of the new row is marked with.
    pub records: Vec<(Identifier, Records)>,
}

/// What adding a row did.
#[derive(Debug, Clone, PartialEq)]
pub struct RowAdded {
    pub lent_from: Option<PathBuf>,
    /// Each cell given, and whether it overrides a column the model fills.
    pub given: Vec<(Identifier, Value, bool)>,
    /// Each cell given readings, with them.
    pub readings: Vec<(Identifier, Vec<f64>)>,
    pub left_absent: Vec<Identifier>,
    pub by_the_model: Vec<Identifier>,
}

/// A derived value that is not current: a property, or a column once.
#[derive(Debug, Clone, PartialEq)]
pub struct NotCurrent {
    pub name: String,
    pub state: Freshness,
    pub rows: usize,
}

/// A new sample, and what it took from another — what `new --keep` and the
/// workbench both write: the quantities and attributes named, nothing else.
/// `left_out`, `tables` and `not_carried` stay empty, nothing being carried
/// unasked.
pub struct Shaped {
    pub sample: Sample,
    /// Quantities carried, waiting for their values.
    pub measured: Vec<String>,
    /// Quantities a formula gives, left out.
    pub left_out: Vec<String>,
    pub tables: Vec<String>,
    /// Attributes carried, each with its value as shown.
    pub attributes: Vec<(String, String)>,
    /// Attributes the pattern's own, not carried: its dates and its `status`,
    /// in order.
    pub not_carried: Vec<String>,
}

/// A new sample's name, as `new` and the workbench both check it: a name, never
/// a path — which put the file anywhere, outside the collection too — and none
/// another sample of `project` holds, since a name identifies one sample.
/// Another project's samples, nested below, keep their own names.
/// A character of Unicode's *format* kind — shown as nothing — and the
/// others a screen draws as nothing.
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

pub fn refuses_name(
    name: &str,
    collection: &SampleList,
    project: Option<&Path>,
) -> Option<EditError> {
    // A blank name makes a hidden `.md`, or one named by spaces: refused
    // here, so that every surface refuses it.
    if name.trim().is_empty() {
        return Some(EditError::new(
            "a sample needs a name: it names its file too".to_string(),
        ));
    }
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Some(EditError::new(format!(
            "'{name}' is a path: a new sample takes a name"
        )));
    }
    // A name is typed on command lines, after `samplekit set` and in filters: a
    // space splits it into two words there, and a leading dash reads as an
    // option.
    if name.chars().any(char::is_whitespace) {
        return Some(EditError::new(format!(
            "'{name}' holds a space, which splits it into two words on a command line: \
             write {} instead",
            name.split_whitespace().collect::<Vec<_>>().join("-")
        )));
    }
    // A control character or an escape is a file named by what the screen
    // cannot show: `new` wrote `a<BEL>b.md` and said `ab.md`.
    if name.chars().any(char::is_control) {
        return Some(EditError::new(format!(
            "{name:?} holds a control character: a name is text a screen shows as it is"
        )));
    }
    // A character that shows nothing, or turns the text around — a
    // zero-width space, a direction override, a byte order mark — makes two
    // names that look one, or a name that reads otherwise than it is.
    if let Some(hidden) = name.chars().find(|c| invisible(*c)) {
        return Some(EditError::new(format!(
            "{name:?} holds U+{:04X}, a character a screen does not show: a name is text a \
             screen shows as it is",
            hidden as u32
        )));
    }
    // `x.md` is the file, not the name: it made `x.md.md` — `x.MD` too.
    if let Some(stem) = name
        .len()
        .checked_sub(3)
        .filter(|at| name.is_char_boundary(*at) && name[*at..].eq_ignore_ascii_case(".md"))
        .map(|at| &name[..at])
        .filter(|stem| !stem.is_empty())
    {
        return Some(EditError::new(format!(
            "'{name}' is a file's name: a new sample takes a name, {stem}, and its file is \
             {stem}.md"
        )));
    }
    // A project goes to other computers: a name Windows cannot give a file
    // made a project that one of them could not hold.
    if let Some(reason) = unportable(name) {
        return Some(EditError::new(format!(
            "'{name}' {reason}, which Windows cannot name a file by: a project shared with it \
             could not hold the sample"
        )));
    }
    if name.starts_with('-') {
        let bare = name.trim_start_matches('-');
        return Some(EditError::new(format!(
            "'{name}' begins with a dash, which a command line reads as an option: {}",
            if bare.is_empty() {
                "a name begins with a letter or a digit".to_string()
            } else {
                format!("write {bare} instead")
            }
        )));
    }
    name_taken(name, collection, project, None)
}

/// A sample renamed, `name=` given to `set` or on the workbench: refused where
/// another sample of `project` holds the name, as a new sample's is — the two
/// were written, and `validate` then found the name twice. `renamed` is the
/// file that takes the name, which may hold it already.
pub fn refuses_rename(
    name: &str,
    collection: &SampleList,
    project: Option<&Path>,
    renamed: &Path,
) -> Option<EditError> {
    name_taken(name, collection, project, Some(renamed))
}

/// Why Windows cannot name a file `name`: a character it refuses, or a name
/// it keeps for a device.
fn unportable(name: &str) -> Option<String> {
    if let Some(refused) = name
        .chars()
        .find(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Some(format!("holds '{refused}'"));
    }
    if name.ends_with(['.', ' ']) {
        return Some("ends with a dot or a space".to_string());
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem[3..].chars().all(|c| c.is_ascii_digit() && c != '0'));
    device.then(|| format!("is {stem}, a device's name"))
}

/// A name as a file system that folds case, or one that normalises Unicode,
/// takes it: `Ab` and `ab`, `é` written whole or as `e` and its accent, are
/// one file on macOS and Windows.
pub fn name_key(name: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    name.nfc().collect::<String>().to_lowercase()
}

fn name_taken(
    name: &str,
    collection: &SampleList,
    project: Option<&Path>,
    renamed: Option<&Path>,
) -> Option<EditError> {
    let same_file = |path: &Path| {
        renamed.is_some_and(|renamed| {
            path == renamed
                || std::fs::canonicalize(path)
                    .ok()
                    .is_some_and(|path| std::fs::canonicalize(renamed).ok() == Some(path))
        })
    };
    let taken = collection
        .iter()
        .filter(|entry| {
            entry
                .path
                .as_deref()
                .and_then(|path| collection.config_for(path))
                .map(|config| config.root())
                == project
        })
        .filter(|entry| !entry.path.as_deref().is_some_and(same_file))
        .find(|entry| {
            entry
                .sample
                .borrow()
                .name()
                .is_some_and(|held| name_key(held) == name_key(name))
        })?;
    let place = taken
        .path
        .as_deref()
        .map_or_else(|| "?".to_string(), |path| path.display().to_string());
    let held = taken.sample.borrow().name().unwrap_or_default().to_string();
    Some(EditError::new(if held == name {
        format!("{name} is already a sample's name, in {place}: a name identifies one sample")
    } else {
        format!(
            "{name} differs from {held}, in {place}, only by case or by how its accents are \
             written: on macOS and Windows the two files are one"
        )
    }))
}

pub fn shaped_like(
    name: &str,
    pattern: Option<&Sample>,
    keep: &[String],
) -> Result<Shaped, EditError> {
    let mut sample = Sample::new();
    // Named by its file, and writing no `name:`: the file's name stands for it,
    // and the pattern's is not carried.
    sample.set_file(Some(Path::new(&format!("{name}.md"))));
    let mut measured: Vec<String> = Vec::new();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let Some(pattern) = pattern else {
        return Ok(Shaped {
            sample,
            measured,
            left_out: Vec::new(),
            tables: Vec::new(),
            attributes,
            not_carried: Vec::new(),
        });
    };
    // Only what is named is taken: another sample's attributes — its tester,
    // its dates — and the shape of its quantities and tables came across
    // unasked, someone else's identity in a new file.
    for kept in keep {
        let Ok(held) = Identifier::new(kept) else {
            return Err(EditError::new(format!(
                "--keep {kept}: the sample holds no field of that name"
            )));
        };
        if let Ok(handle) = pattern.property(&held) {
            // A value the file records as derived belongs to the model, not to
            // a new file: kept, it would be written with no record, a computed
            // value passed off as entered. **A value, not a record**: a record
            // whose channel is the uncertainty's says the value beside it was
            // entered; nor readings, whose statistic records them as input.
            let derived = handle.peek(|property| {
                let records = property.records();
                records.computed.is_some()
                    && records.produced != Some(Produced::Uncertainty)
                    && property.readings().is_none()
            });
            if derived {
                return Err(EditError::new(format!(
                    "--keep {held}: the model computes it, and computes it for the new \
                     sample too"
                )));
            }
            // Kept whole: the readings a kept value stands for come with it,
            // a value written beside them only where one was, its entered
            // uncertainty, and the statistics declared for it.
            let mut property = match handle.readings() {
                Some(readings) => {
                    let mut measured = Property::measured(readings, None);
                    measured.set_written_value(
                        handle.peek(|property| property.written_value().cloned()),
                    );
                    measured
                }
                None => Property::stored(
                    handle
                        .peek(|property| property.peek_value())
                        .unwrap_or_else(Value::absent),
                ),
            };
            let (location, convention) =
                handle.peek(|kept| (kept.declared_location(), kept.declared_convention()));
            if location.is_some() || convention.is_some() {
                property.declare_statistics(location, convention);
                if handle.peek(Property::statistic_taken) {
                    property.take_statistic();
                }
            }
            // An uncertainty a formula gives — its record's channel — is the
            // model's to give the new sample, as a value it computes is: kept,
            // it was written with no record, *record missing* for good.
            let spread_derived = handle.peek(|property| {
                property.records().produced == Some(Produced::Uncertainty)
                    || property.has_uncertainty_formula()
            });
            if !spread_derived
                && let Some(spread) = handle
                    .peek(|property| property.peek_uncertainty())
                    .flatten()
            {
                property.set_uncertainty(Some(spread));
            }
            // The statistic its readings give, recorded: kept with its record,
            // or the value it stands for is written with none, *record
            // missing*, and `status --exit-code` fails over a copy.
            let recorded = handle.peek(|kept| {
                (kept.readings().is_some() && kept.records().computed.is_some())
                    .then(|| kept.records().clone())
            });
            if let Some(records) = recorded {
                property.set_records(records);
            }
            property.set_presentation(handle.presentation());
            sample
                .set_property(held.clone(), property)
                .map_err(|error| EditError::new(error.to_string()))?;
            measured.push(held.to_string());
        } else if let Ok(value) = pattern.attribute(&held) {
            let shown = value.as_scalar().map_or_else(
                || {
                    value.as_list().map_or_else(
                        || "—".to_string(),
                        |items| {
                            let written: Vec<String> = items.iter().map(shown_value).collect();
                            format!("[{}]", written.join(", "))
                        },
                    )
                },
                shown_value,
            );
            sample
                .set_attribute(held.clone(), value.clone())
                .map_err(|error| EditError::new(error.to_string()))?;
            attributes.push((held.to_string(), shown));
        } else if pattern.table_names().iter().any(|table| **table == held) {
            // A table is no value to keep: its rows are the pattern's own
            // measurements.
            return Err(EditError::new(format!(
                "--keep {held}: --keep takes values, and {held} is a table — its rows are that \
                 sample's own measurements, and are not carried"
            )));
        } else {
            return Err(EditError::new(format!(
                "--keep {held}: the sample holds no field of that name"
            )));
        }
    }
    Ok(Shaped {
        sample,
        measured,
        left_out: Vec::new(),
        tables: Vec::new(),
        attributes,
        not_carried: Vec::new(),
    })
}

// ------------------------------------------------------------- parsing

/// `field=value`, refused as a whole rather than half-read.
pub fn split_assignment(written: &str) -> Result<(String, String), EditError> {
    match written.split_once('=') {
        Some((field, value)) if !field.is_empty() => {
            Ok((field.trim().to_string(), value.trim().to_string()))
        }
        _ => Err(EditError::new(format!(
            "'{written}' is not a change: write it as <field>=<value>, such as notch=1.010"
        ))),
    }
}

/// What a person typed for `field`: the value it plainly is ([`value_of`]), and
/// for the name, the text as typed — `name=007` is no number, and a label may
/// hold a comma or look like a date. Blank is absent, which removes the name
/// written.
pub fn typed(field: &str, text: &str) -> Result<Value, EditError> {
    if matches!(
        fields::parse(field),
        Ok(Field::Reserved(fields::ReservedField::Name))
    ) {
        let text = text.trim();
        return Ok(if text.is_empty() {
            Value::absent()
        } else {
            Value::text(text)
        });
    }
    // A list typed by hand is readings, said with the field's own name: the
    // advice named `notch` whatever was typed.
    if text.starts_with('[') && !text.starts_with("[[") && crate::core::link::parse(text).is_none()
    {
        let quantity = field.split(['.', '[']).next().unwrap_or(field);
        return Err(EditError::new(format!(
            "'{text}' is a list, and a value is one number\n  \
             repeated measurements of one property are its readings: {quantity}.readings={}",
            text.trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(",")
        )));
    }
    value_of(text)
}

/// A list typed for an attribute, `[ana, tom]`: its items, each read as a
/// value is — `'b, c'` quoted keeps its comma — or `None` where the text is no
/// list. A wikilink, `[[Other note]]`, is text, not a list.
pub fn list_typed(text: &str) -> Option<Result<Vec<Value>, EditError>> {
    let text = text.trim();
    if let Some(links) = links_listed(text) {
        return Some(Ok(links.into_iter().map(Value::text).collect()));
    }
    if !text.starts_with('[') || text.starts_with("[[") || !text.ends_with(']') {
        return None;
    }
    let inner = &text[1..text.len() - 1];
    if inner.trim().is_empty() {
        return Some(Ok(Vec::new()));
    }
    let mut items: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for character in inner.chars() {
        match (quote, character) {
            (None, '\'' | '"') if current.trim().is_empty() => {
                quote = Some(character);
                current.clear();
            }
            (Some(open), close) if close == open => quote = None,
            (None, ',') => items.push(std::mem::take(&mut current)),
            _ => current.push(character),
        }
    }
    items.push(current);
    Some(
        items
            .iter()
            .map(|item| {
                let item = item.trim();
                if item.is_empty() {
                    return Err(EditError::new(format!(
                        "'{text}' holds an empty item: a list's items are separated by commas, \
                         each one given"
                    )));
                }
                value_of(item)
            })
            .collect(),
    )
}

/// Several links typed as one text, `[[a]],[[b]]` or `[ [[a]], [[b]] ]`: each
/// link, where every piece is one; `None` for one link alone, or other text.
fn links_listed(text: &str) -> Option<Vec<String>> {
    if crate::core::link::parse(text).is_some() {
        return None;
    }
    // Split at the commas outside any bracket, as a link holds its own.
    let pieces = |text: &str| -> Vec<String> {
        let (mut pieces, mut current, mut depth) = (Vec::new(), String::new(), 0i32);
        for character in text.chars() {
            match character {
                '[' | '(' => depth += 1,
                ']' | ')' => depth -= 1,
                ',' if depth == 0 => {
                    pieces.push(std::mem::take(&mut current).trim().to_string());
                    continue;
                }
                _ => {}
            }
            current.push(character);
        }
        pieces.push(current.trim().to_string());
        pieces
    };
    let all_links = |pieces: Vec<String>| {
        (!pieces.is_empty()
            && pieces
                .iter()
                .all(|piece| crate::core::link::parse(piece).is_some()))
        .then_some(pieces)
    };
    let bare = pieces(text);
    if bare.len() > 1
        && let Some(links) = all_links(bare)
    {
        return Some(links);
    }
    let inner = text.strip_prefix('[')?.strip_suffix(']')?;
    all_links(pieces(inner))
}

/// An attribute written as a list, `authors=[ana, tom]`: what it was and what
/// it becomes, as a preview shows them. A quantity takes no list — its
/// repeated measurements are its readings — nor does a name SampleKit keeps.
pub fn assign_list(
    sample: &mut Sample,
    field: &str,
    items: Vec<Value>,
) -> Result<(String, String), EditError> {
    let name = match fields::parse(field) {
        Ok(Field::Named {
            name,
            channel: Channel::Value,
        }) => name,
        _ => {
            return Err(EditError::new(format!(
                "{field} takes no list: a list is an attribute's, written by its name"
            )));
        }
    };
    if sample.property(&name).is_ok() {
        let shown: Vec<String> = items.iter().map(shown_value).collect();
        return Err(EditError::new(format!(
            "{name} is a property, and a value is one number\n  repeated measurements of one \
             property are its readings: {name}.readings={}",
            shown.join(",")
        )));
    }
    let before = sample
        .attribute(&name)
        .ok()
        .map_or("—".to_string(), shown_attribute);
    let value = AttributeValue::list(items).map_err(|error| EditError::new(error.to_string()))?;
    let after = shown_attribute(&value);
    sample
        .set_attribute(name, value)
        .map_err(|error| EditError::new(error.to_string()))?;
    Ok((before, after))
}

/// An attribute as a preview shows it: its value, or its items in brackets.
pub fn shown_attribute(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Scalar(value) => shown_value(value),
        // An item holding a comma quoted, as it is typed: `['a, b', c]`; a
        // list of links spaced from its brackets, `[ [[a]], [[b]] ]`.
        AttributeValue::List(items) => format!(
            "{}{}{}",
            if spaced(items) { "[ " } else { "[" },
            items
                .iter()
                .map(|item| {
                    let shown = shown_value(item);
                    if shown.contains(',') {
                        format!("'{shown}'")
                    } else {
                        shown
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            if spaced(items) { " ]" } else { "]" },
        ),
    }
}

fn spaced(items: &[Value]) -> bool {
    items
        .first()
        .is_some_and(|item| matches!(item, Value::Text(text) if text.starts_with('[')))
}

/// A name to write in a sample's file, as `set`, Python and the workbench check
/// it: a label — `Sample A` — and never a path, which names a file where the
/// name is written in one, nor text over several lines or holding a control
/// character, which a table's line and a filter cannot hold. A name another
/// sample holds is not refused here: a name may repeat, and `validate` says
/// where one written twice makes a report ambiguous.
pub fn refuses_written_name(name: &str) -> Option<EditError> {
    if name.contains(['/', '\\']) || name.starts_with('.') {
        return Some(EditError::new(format!(
            "'{name}' is a path: a name is written in the sample's file, and the file keeps \
             its own name"
        )));
    }
    if name.chars().any(char::is_control) {
        return Some(EditError::new(format!(
            "{name:?} holds a control character, a line break or a tab: a name is one line \
             of text"
        )));
    }
    None
}

/// What a person typed, as the value it plainly is.
pub fn value_of(text: &str) -> Result<Value, EditError> {
    if text.is_empty() {
        return Ok(Value::absent());
    }
    // A list typed by hand is someone entering readings, which a value is
    // not. Storing it as text would put a string where numbers belong. A
    // wikilink, `[[Other note]]`, or a Markdown link, `[text](file.md)`, is
    // text: a link, as the file holds it.
    if text.starts_with('[') && !text.starts_with("[[") && crate::core::link::parse(text).is_none()
    {
        return Err(EditError::new(format!(
            "'{text}' is a list, and a value is one number\n  \
             repeated measurements of one property are its readings: notch.readings={text}"
        )));
    }
    if let Ok(whole) = text.parse::<i64>() {
        return Ok(Value::integer(whole));
    }
    if let Ok(real) = text.parse::<f64>() {
        return Value::number(real).map_err(|error| EditError::new(error.to_string()));
    }
    if matches!(text, "true" | "false") {
        return Ok(Value::boolean(text == "true"));
    }
    value::recognize_text(text).map_err(|error| EditError::new(error.to_string()))
}

/// `1.011,1.010,1.012` or `[1.011, 1.010, 1.012]`, each a number: the brackets
/// are the list's, and a shell or a hand may leave them out.
pub fn readings_of(field: &str, text: &str) -> Result<Readings, EditError> {
    let listed = unbracketed(text);
    if listed.is_empty() {
        return Err(EditError::new(
            "no readings given: readings are numbers apart by commas, 1.011, 1.010, 1.012"
                .to_string(),
        ));
    }
    let mut numbers = Vec::new();
    for piece in listed.split(',') {
        let piece = piece.trim();
        numbers.push(piece.parse::<f64>().map_err(|_| {
            EditError::new(format!(
                "'{piece}' is not a number: readings are numbers apart by commas, 1.011, \
                 1.010, 1.012"
            ))
        })?);
    }
    // One number given as readings would drop the readings before it, or
    // make a list of one whose spread is nothing — how `± 0.000` reached
    // three real samples. One number is a value.
    if let [only] = numbers.as_slice() {
        let quantity = field.strip_suffix(".readings").unwrap_or(field);
        return Err(EditError::new(format!(
            "'{listed}' is one number, and readings are several: one number is a value, \
             {quantity}={only}"
        )));
    }
    Readings::new(numbers).map_err(|error| EditError::new(error.to_string()))
}

/// The list inside its brackets, where it has them.
fn unbracketed(text: &str) -> &str {
    let text = text.trim();
    text.strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(text)
        .trim()
}

/// A warning where readings look like numbers written with a decimal comma :
/// `20,5,21,5` is four readings to a comma-separated list and two numbers to a
/// continental hand. Said, not refused — four integers can be four readings —
/// with the spelling that cannot be misread: brackets and points, `[20.5,
/// 21.5]`.
///
/// Two shapes read so: whole numbers alternating with fractions of one or two
/// digits, where no piece has a point already; and a piece with a leading zero,
/// `063`, which is how a fraction reads and a reading is never written.
pub fn decimal_commas(field: &str, text: &str) -> Option<String> {
    let pieces: Vec<&str> = unbracketed(text).split(',').map(str::trim).collect();
    let digits = |piece: &str| !piece.is_empty() && piece.chars().all(|c| c.is_ascii_digit());
    let whole = |piece: &str| {
        let unsigned = piece.strip_prefix('-').unwrap_or(piece);
        digits(unsigned)
    };
    let alternating = pieces.len() >= 2
        && pieces.len().is_multiple_of(2)
        && !pieces.iter().any(|piece| piece.contains(['.', 'e', 'E']))
        && pieces
            .chunks(2)
            .all(|pair| whole(pair[0]) && digits(pair[1]) && (1..=2).contains(&pair[1].len()));
    let leading_zero = pieces
        .iter()
        .any(|piece| piece.len() > 1 && piece.starts_with('0') && digits(piece));
    if !alternating && !leading_zero {
        return None;
    }
    // The pieces paired as a decimal comma pairs them, where they pair.
    let paired: Option<Vec<String>> = pieces.len().is_multiple_of(2).then(|| {
        pieces
            .chunks(2)
            .map(|pair| format!("{}.{}", pair[0], pair[1]))
            .collect()
    });
    let example = paired.map_or_else(
        || "[1.05, 1.06]".to_string(),
        |paired| format!("[{}]", paired.join(", ")),
    );
    Some(format!(
        "'{}' reads as {} readings: if some are decimals written with a comma, write \
         them with points, {field}={example}",
        unbracketed(text),
        pieces.len()
    ))
}

/// Whether an assignment gives readings — `radius.readings=…`, or a cell's
/// `table.column[…].readings=…`.
pub fn gives_readings(field: &str) -> bool {
    matches!(
        fields::parse(field),
        Ok(Field::Named {
            channel: Channel::Readings,
            ..
        } | Field::Cell {
            channel: Channel::Readings,
            ..
        })
    )
}

/// One value, rendered plainly enough for a before-and-after line.
pub fn shown_value(value: &Value) -> String {
    match value {
        Value::Integer(whole) => whole.to_string(),
        Value::Number(real) => format!("{real}"),
        Value::Text(text) => text.clone(),
        Value::Boolean(yes) => yes.to_string(),
        Value::Date(date) => date.to_string(),
        Value::DateTime(stamp) => stamp.to_string(),
        Value::Absent => "—".to_string(),
        Value::NotApplicable => "n/a".to_string(),
    }
}

/// The precision the project declares for a field a preview shows :
/// `[property.radius]`'s for `radius` and its readings, a column's for its cells, the
/// uncertainty's for `radius.u`. A precision is declared in the project and nowhere
/// else.
pub fn declared_precision(
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
) -> Option<crate::core::formatting::Precision> {
    let (quantity, channel) = match fields::parse(field).ok()? {
        Field::Named { name, channel } => (name.to_string(), channel),
        Field::Cell {
            table,
            column,
            channel,
            ..
        } => (format!("{table}.{column}"), channel),
        _ => return None,
    };
    let precision = config?
        .property(&quantity)?
        .precision
        .as_ref()?
        .precision()?;
    Some(if channel == Channel::Uncertainty {
        precision.of_uncertainty()
    } else {
        precision
    })
}

/// A value as a preview shows it: a number at the precision the project
/// declares for its field, as a table shows it — `6.08`, where the computation
/// left `6.081250000000001` — and as a table shows it where none is declared.
/// Text, dates and the rest as the file writes them.
pub fn previewed(
    value: &Value,
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
) -> String {
    // A spread with nothing declared is rounded up, as a table writes it.
    let spread =
        crate::query::field_addressing::parse(field).is_ok_and(|parsed| fields_spread(&parsed));
    let precision = declared_precision(field, config)
        .or_else(|| spread.then(crate::core::formatting::Precision::general_spread));
    shown_at(value, precision)
}

fn fields_spread(field: &crate::query::field_addressing::Field) -> bool {
    crate::query::field_addressing::is_spread(crate::query::field_addressing::channel(field))
}

/// A value typed, as a preview shows it: at the declared precision where that
/// writes it whole, and every digit where it would round it — `1.0555` was
/// shown `1.056`, `18.75` `18.8`, and a reader copying from a spreadsheet could
/// not check the digits written.
pub fn previewed_typed(
    value: &Value,
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
) -> String {
    let shown = previewed(value, field, config);
    match value {
        Value::Number(number) if shown.parse::<f64>().ok() != Some(*number) => shown_value(value),
        _ => shown,
    }
}

/// A value at `precision`, or as a table shows it without one: the one rule a
/// preview shows a number by. Every digit was shown where no precision was
/// declared, `1.0505 ± 0.0004999999999999449`, the noise a computation leaves.
pub fn shown_at(value: &Value, precision: Option<crate::core::formatting::Precision>) -> String {
    match (value, precision) {
        (Value::Number(_) | Value::Integer(_), precision) => crate::core::formatting::format_value(
            value,
            &crate::core::formatting::Resolved {
                unit: None,
                symbol: None,
                property: None,
                uncertainty: None,
                unitless: None,
                unit_alone: None,
                unit_in_form: None,
                precision,
            },
        ),
        _ => shown_value(value),
    }
}

/// A change's two sides as a preview shows them, `—` for none: each by
/// [`previewed`], and **both whole where the precision writes them alike**
/// though they differ — a preview never reads `1.021 → 1.021` of a value that
/// moved.
pub fn previewed_change(
    before: Option<&Value>,
    after: Option<&Value>,
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
) -> (String, String) {
    let side = |value: Option<&Value>, shown: &dyn Fn(&Value) -> String| {
        value.map_or("—".to_string(), shown)
    };
    let at_precision = |value: &Value| previewed(value, field, config);
    let (was, is) = (side(before, &at_precision), side(after, &at_precision));
    let differ = match (before, after) {
        (Some(before), Some(after)) => !value::equals(before, after),
        (None, None) => false,
        _ => true,
    };
    if was == is && differ {
        (side(before, &shown_value), side(after, &shown_value))
    } else {
        // What is typed is what is written: a precision that would round it —
        // `1.03` shown `1.0` — hides the digits the reader is checking.
        let whole = after.is_some_and(|after| match after {
            Value::Number(number) => is.parse::<f64>().ok() != Some(*number),
            _ => false,
        });
        (was, if whole { side(after, &shown_value) } else { is })
    }
}

/// The value a field holds, to compare before with after: two different
/// numbers render alike at a declared precision, which is exactly the
/// comparison that must not be made on text.
pub fn value_at(sample: &Sample, field_name: &str) -> Option<Value> {
    let field = fields::parse(field_name).ok()?;
    let vocabulary = fields::vocabulary_of(&[sample]);
    let subject = Subject {
        sample,
        path: None,
        vocabulary: &vocabulary,
        states: None,
    };
    fields::resolve(&field, &subject).ok()?.scalar().cloned()
}

// ------------------------------------------------------------- applying

/// A value set by hand on a property — the rule every surface shares. A live
/// formula holds it as an override, which the save marks. A formula's value
/// read without its model keeps the formula's record, and its own digest is
/// marked edited now: dropping the record made the file read *current*, the
/// override laundered.
pub fn set_by_hand(handle: &PropertyHandle, value: Value) {
    // A value that does not apply has no spread either: the uncertainty goes
    // with it. Its readings stay — what was measured is never dropped by a
    // value written over it — and `n/a` outranks their statistic as any value
    // written does.
    let not_applicable = value.is_not_applicable();
    // Emptied beside readings a statistic stands for: given back to it, which
    // is no override of anything.
    if matches!(value, Value::Absent)
        && handle
            .peek(|property| property.readings().is_some() && property.has_declared_statistic())
    {
        handle.set_value(value);
        return;
    }
    if handle.peek(Property::is_computed) {
        handle.set_value(value);
        if not_applicable {
            handle.set_uncertainty(None);
        }
        return;
    }
    let kept = handle.records();
    handle.set_value(value);
    if not_applicable {
        handle.set_uncertainty(None);
    }
    if kept.computed.is_none() {
        return;
    }
    // Marked only where a formula owns this channel: a value entered beside a
    // computed uncertainty overrides nothing, and the record stays for the
    // uncertainty's formula.
    let owned = kept.produced != Some(Produced::Uncertainty);
    mark_by_hand(handle, kept, owned);
}

/// An uncertainty typed by hand. Over one a statistic of the readings or a
/// formula gave, it is an override, as a value typed over its formula is —
/// said by every surface, and given back by `--force` or by emptying it: the
/// record is kept, marked edited. Whether it is one is returned.
pub fn set_uncertainty_by_hand(handle: &PropertyHandle, uncertainty: Uncertainty) -> bool {
    let records = handle.records();
    let owned_by = handle.peek(|property| {
        property.declared_convention().is_some() || property.has_uncertainty_formula()
    }) || records.produced == Some(Produced::Uncertainty);
    handle.set_uncertainty(Some(uncertainty));
    if records.computed.is_some() && owned_by {
        mark_by_hand(handle, records, true);
        return true;
    }
    false
}

/// The record of a quantity one of whose channels a hand wrote: its own
/// digest taken again, marked edited where a formula or a statistic owned
/// that channel, so that every surface reads the same override.
fn mark_by_hand(handle: &PropertyHandle, kept: Records, owned: bool) {
    let shape = handle.peek(schema::property_as_is);
    // Written the way the check computes it, or the two disagree and an
    // untouched channel reads edited.
    let digest = fingerprint::own_of(&shape, kept.produced);
    // A channel the hand overrode before stays overridden: writing the other
    // beside it gives nothing back to the formula.
    let overridden = kept
        .fingerprint
        .as_ref()
        .is_some_and(|fingerprint| fingerprint.is_edited());
    let digest = if owned || overridden {
        digest.marked_edited()
    } else {
        digest
    };
    handle.set_records(Records {
        failure: None,
        fingerprint: Some(digest),
        computed: kept.computed,
        // Which channel a formula owns does not change because a hand wrote
        // one; what the file states about where its numbers came from survives,
        // and which formula read what.
        produced: kept.produced,
        statistics: kept.statistics,
        channel_only: kept.channel_only,
    });
}

/// What assigning a scalar to a quantity's value meets: its readings, which
/// **stay**, with the uncertainty that goes on following them.
pub fn kept_by(sample: &Sample, field_name: &str) -> Option<Kept> {
    let Ok(Field::Named {
        name,
        channel: Channel::Value,
    }) = fields::parse(field_name)
    else {
        return None;
    };
    let handle = sample.property_if(&name)?;
    let readings = handle.readings()?;
    let uncertainty = handle
        .peek(Property::peek_uncertainty)
        .flatten()
        .map(|uncertainty| uncertainty.magnitude());
    let statistic = handle.records().statistics.is_some();
    Some(Kept {
        readings: readings.as_slice().to_vec(),
        uncertainty,
        statistic,
    })
}

/// Text where a column holds numbers, or declares a unit, as `refuses_text`
/// says of a quantity: a cell of a numeric column is a number. `in_row` when
/// the cell is given by `--add-row`, whose hint writes the column alone —
/// `hardness=1.011` — rather than the address `set` takes.
fn refuses_text_in(
    held: &Table,
    table: &Identifier,
    column: &Identifier,
    value: &Value,
    in_row: bool,
) -> Option<EditError> {
    if !matches!(value, Value::Text(_) | Value::Boolean(_)) {
        return None;
    }
    let numeric = held.rows().any(|row| {
        row.cell(column).is_ok_and(|cell| {
            matches!(
                cell.peek_value(),
                Some(Value::Integer(_) | Value::Number(_))
            )
        })
    });
    // The column's own unit where no row holds a cell yet: a table borrowed
    // for its first row has none, and took text in a column with a unit.
    let unit = held
        .presentation_of(column, &RowAddress::ordinal(0))
        .ok()
        .or_else(|| {
            held.column(column)
                .ok()
                .map(|view| view.presentation().clone())
        })
        .and_then(|presentation| presentation.unit);
    if !numeric && unit.is_none() {
        return None;
    }
    // A column a formula fills is not measured: it holds numbers.
    let derived = held.is_derived(column);
    let what = unit.map_or_else(
        || format!("{table}.{column} holds numbers"),
        |unit| {
            if derived {
                format!("{table}.{column} holds numbers in {unit}")
            } else {
                format!("{table}.{column} is measured in {unit}")
            }
        },
    );
    let written = if in_row {
        format!("{column}=")
    } else {
        format!("{table}.{column}[…]=")
    };
    // Several numbers are readings, which are given as a channel of their own :
    // no mean is offered in their place, since which statistic stands for them
    // is the column's to declare.
    if readings_in(value).is_some() {
        let readings = if in_row {
            format!("{column}.readings=")
        } else {
            format!("{table}.{column}[…].readings=")
        };
        return Some(EditError::new(format!(
            "{what}, and '{}' is several numbers: a value is one\n  \
             readings are given as such: {readings}{}",
            shown_value(value),
            shown_value(value)
        )));
    }
    Some(EditError::new(format!(
        "{what}, and '{}' is {}\n  write the number alone: {written}{}",
        shown_value(value),
        not_a_number(value),
        number_in(value)
    )))
}

/// What a value that is no number is, said plainly.
fn not_a_number(value: &Value) -> &'static str {
    match value {
        Value::Boolean(_) => "a yes-or-no, not a number",
        _ => "text",
    }
}

/// `12.1,12.3,12.2`: several numbers, which a decimal comma is not — `1,011`
/// is one number written the continental way, and its hint writes `1.011`.
fn readings_in(value: &Value) -> Option<Vec<f64>> {
    let Value::Text(text) = value else {
        return None;
    };
    let pieces: Vec<&str> = text.split(',').map(str::trim).collect();
    if pieces.len() < 2 || (pieces.len() == 2 && !pieces.iter().any(|piece| piece.contains('.'))) {
        return None;
    }
    pieces
        .iter()
        .map(|piece| piece.parse::<f64>().ok())
        .collect()
}

/// The number a text begins with, as a hint to write it alone — `21 L` gives
/// `21`, `21,5` gives `21.5` — or a number of the demo's where there is none.
fn number_in(value: &Value) -> String {
    let Value::Text(text) = value else {
        return "1.05".to_string();
    };
    // A number in a form YAML would not read as one, `1_000` or `0x10`: the
    // advice is that number, not the digits before the first letter.
    let bare = text.trim().replace('_', "");
    if bare != text.trim()
        && let Ok(number) = bare.parse::<f64>()
    {
        return number.to_string();
    }
    if let Some(hex) = bare.strip_prefix("0x").or_else(|| bare.strip_prefix("0X"))
        && let Ok(number) = i64::from_str_radix(hex, 16)
    {
        return number.to_string();
    }
    let leading: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    if leading.parse::<f64>().is_ok() {
        leading
    } else {
        "1.05".to_string()
    }
}

/// Text where a number belongs: a quantity that holds a number, or declares a
/// unit, which nothing but a measurement has. A yes-or-no is no number either:
/// `radius=true` was written `v: true`.
fn refuses_text(
    sample: &Sample,
    name: &Identifier,
    channel: Channel,
    value: &Value,
) -> Option<EditError> {
    if !matches!(channel, Channel::Value) || !matches!(value, Value::Text(_) | Value::Boolean(_)) {
        return None;
    }
    let handle = sample.property_if(name)?;
    // Readings, a statistic of them declared, or an uncertainty, are a
    // number's too: a quantity a computation left without a value, holding
    // only the statistic it declares, took `1,016` as text.
    let numeric = handle.peek(|property| {
        matches!(
            property.peek_value(),
            Some(Value::Integer(_) | Value::Number(_))
        ) || property.readings().is_some()
            || property.has_declared_statistic()
            || property.peek_uncertainty().is_some()
    });
    let unit = handle.presentation().unit.clone();
    if !numeric && unit.is_none() {
        return None;
    }
    Some(not_a_number_for(name.as_str(), unit.as_deref(), value))
}

/// Text, or a yes-or-no, refused for a quantity that holds numbers: one
/// message, whether the sample holds the quantity already or, for a new
/// sample, the collection does.
pub fn not_a_number_for(name: &str, unit: Option<&str>, value: &Value) -> EditError {
    let quantity = unit.map_or_else(
        || format!("'{name}' holds a number"),
        |unit| format!("'{name}' is measured in {unit}"),
    );
    // A value and its uncertainty written together, `1.048±0.001`: both kept
    // in the advice, where the number alone lost the uncertainty unseen.
    if let Value::Text(text) = value
        && let Some((number, uncertainty)) = ["±", "+/-", "+-"]
            .iter()
            .find_map(|sign| text.split_once(sign))
        && number.trim().parse::<f64>().is_ok()
        && uncertainty.trim().parse::<f64>().is_ok()
    {
        return EditError::new(format!(
            "{quantity}, and '{}' is {}\n  its uncertainty is a field of its own: {name}={} \
             {name}.u={}",
            shown_value(value),
            not_a_number(value),
            number.trim(),
            // An uncertainty is a size: `±-1` is advised as 1, which `set`
            // takes, not as -1, which it refuses.
            uncertainty.trim().trim_start_matches('-')
        ));
    }
    EditError::new(format!(
        "{quantity}, and '{}' is {}\n  write the number alone: {name}={}",
        shown_value(value),
        not_a_number(value),
        number_in(value)
    ))
}

/// How the samples of a collection hold a name, for a sample not yet written
/// that is given it: as a quantity, with the unit they agree on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldAs {
    /// The unit every sample holding it writes; `None` where none does, or
    /// where they disagree.
    pub unit: Option<String>,
    /// Every unit they write, where more than one.
    pub units: Vec<String>,
    /// Whether one holds a number for it.
    pub numeric: bool,
}

/// Whether other samples hold `name` as a quantity. A new sample given a value
/// for it is written as they write it: `radius=1.048` beside pieces that measure
/// `radius` was written as an attribute, which the model then refused to read.
pub fn held_as(collection: &SampleList, name: &str) -> Option<HeldAs> {
    let name = Identifier::new(name).ok()?;
    let mut found = false;
    let mut numeric = false;
    let mut units: Vec<String> = Vec::new();
    let mut unitless = false;
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        let Ok(handle) = sample.property(&name) else {
            continue;
        };
        found = true;
        numeric |= handle.peek(|property| {
            matches!(
                property.peek_value(),
                Some(Value::Integer(_) | Value::Number(_))
            ) || property.readings().is_some()
        });
        match handle.presentation().unit {
            Some(unit) if !units.contains(&unit) => units.push(unit),
            Some(_) => {}
            None => unitless = true,
        }
    }
    if !found {
        return None;
    }
    // One unit, written by every sample holding the name, is agreed on; a
    // sample writing none beside others writing one is a disagreement.
    let agreed = units.len() == 1 && !unitless;
    let unit = agreed.then(|| units[0].clone());
    if agreed {
        units.clear();
    }
    Some(HeldAs {
        unit,
        units,
        numeric,
    })
}

/// Where what a name is was learnt, for a preview to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownFrom {
    /// `[property.*]` in `.samplekitrc`.
    Configuration,
    /// The model, as its description says: the model's file.
    Model(PathBuf),
    /// The other samples, which hold it as a quantity.
    Samples,
}

/// What a name is known as, and from where.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub declaration: PropertyDeclaration,
    pub from: KnownFrom,
    /// How the other samples hold it, where they do, whoever declares it.
    pub held: Option<HeldAs>,
    /// `.samplekitrc`'s unit, and the other one the model gives.
    pub disagreement: Option<(String, String)>,
}

/// What `field` is, for an assignment to `sample`, as the project knows it:
/// **`.samplekitrc`, then the model, then the other samples** — the first that
/// names it says it is a quantity, and the unit is the first any of them gives.
/// The model is read from its description, where it is current, and never from
/// its source: a name only the model declares, `self.hb =
/// sk.Property(unit="mm")`, is written as a quantity with its unit, where it
/// was written as an attribute the model then refused. The model and the
/// samples are asked only of a name the sample holds as no quantity — the
/// samples only of one it holds not at all — and only for its value.
pub fn known_as(
    sample: &Sample,
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
    collection: Option<&SampleList>,
) -> Option<Known> {
    let declared = config.and_then(|config| config.property(field)).cloned();
    let name = match fields::parse(field) {
        Ok(Field::Named {
            name,
            channel: Channel::Value,
        }) if !sample.has_property(&name) => Some(name),
        _ => None,
    };
    let model = name.as_ref().zip(config).and_then(|(name, config)| {
        let template = crate::config::model_runtime::template_of(config)?;
        let described = crate::config::model_runtime::current_description(config)?;
        let property = described.property(name.as_str())?;
        Some((template.path().to_path_buf(), property.unit.clone()))
    });
    let held = name
        .as_ref()
        .filter(|name| !sample.has_attribute(name))
        .zip(collection)
        .and_then(|(name, collection)| held_as(collection, name.as_str()));
    let disagreement = declared
        .as_ref()
        .and_then(|declared| declared.unit.clone())
        .zip(model.as_ref().and_then(|(_, unit)| unit.clone()))
        .filter(|(configured, modelled)| configured != modelled);
    let unit = declared
        .as_ref()
        .and_then(|declared| declared.unit.clone())
        .or_else(|| model.as_ref().and_then(|(_, unit)| unit.clone()))
        .or_else(|| held.as_ref().and_then(|held| held.unit.clone()));
    let (declaration, from) = match (declared, model, &held) {
        (Some(declared), _, _) => (declared, KnownFrom::Configuration),
        (None, Some((file, _)), _) => (PropertyDeclaration::default(), KnownFrom::Model(file)),
        (None, None, Some(_)) => (PropertyDeclaration::default(), KnownFrom::Samples),
        (None, None, None) => return None,
    };
    Some(Known {
        declaration: PropertyDeclaration {
            unit,
            ..declaration
        },
        from,
        held,
        disagreement,
    })
}

/// A value typed where the model computes it and never did — no record of
/// any formula beside it — held as an override, as one typed over a value
/// computed is: said *an override* and marked so in the file. It was written
/// as a value entered, and each surface named it apart — *record missing*,
/// *waits for*, *entered or measured*.
pub fn over_the_model(
    sample: &Sample,
    change: &mut Change,
    config: Option<&crate::config::project_config::ProjectConfig>,
) {
    if !matches!(change.became, Became::Changed | Became::NewQuantity)
        || change.after.as_ref().is_none_or(Value::is_absent)
    {
        return;
    }
    let Ok(Field::Named {
        name,
        channel: Channel::Value,
    }) = fields::parse(&change.field)
    else {
        return;
    };
    let computes = config
        .and_then(crate::config::model_runtime::current_description)
        .and_then(|described| described.properties.get(name.as_str()).cloned())
        .is_some_and(|property| property.value.is_formula());
    let Ok(handle) = sample.property(&name) else {
        return;
    };
    let records = handle.records();
    if !computes || records.computed.is_some() || handle.peek(Property::is_computed) {
        return;
    }
    mark_by_hand(&handle, records, true);
    change.became = Became::Override;
}

/// [`apply`], with what the project knows of the name: **a name the model
/// declares is a quantity whatever is written** — text too, where the model
/// gives it no unit, since a property of the model holds text as the model
/// reads it; a unit refuses text as `apply` does.
pub fn apply_known(
    sample: &mut Sample,
    field: &str,
    value: Value,
    known: Option<&Known>,
) -> Result<Change, EditError> {
    let text_for_the_model = known.is_some_and(|known| {
        matches!(known.from, KnownFrom::Model(_)) && known.declaration.unit.is_none()
    }) && matches!(
        value,
        Value::Text(_) | Value::Boolean(_) | Value::Date(_) | Value::DateTime(_)
    );
    let named = match fields::parse(field) {
        Ok(Field::Named {
            name,
            channel: Channel::Value,
        }) => Some(name),
        _ => None,
    };
    if text_for_the_model
        && let Some(name) = named
        && !sample.has_property(&name)
        && !sample.has_attribute(&name)
    {
        let before = value_at(sample, field);
        sample
            .set_property(name, Property::stored(value))
            .map_err(|error| EditError::new(error.to_string()))?;
        let after = value_at(sample, field);
        return Ok(Change {
            field: field.to_string(),
            before,
            after,
            kept: None,
            became: Became::NewQuantity,
        });
    }
    apply(sample, field, value, known.map(|known| &known.declaration))
}

/// One assignment, applied to the sample in memory.
pub fn apply(
    sample: &mut Sample,
    field: &str,
    value: Value,
    declared: Option<&PropertyDeclaration>,
) -> Result<Change, EditError> {
    // `note` is the text below the frontmatter, as Python's `sample.note`
    // is: written whole, an empty value emptying it.
    if field == "note" {
        let shown = |note: &str| (!note.is_empty()).then(|| Value::text(note.trim_end()));
        let before = shown(sample.note());
        let text = match &value {
            Value::Absent => String::new(),
            Value::Text(text) => text.clone(),
            Value::Integer(whole) => whole.to_string(),
            Value::Number(real) => real.to_string(),
            Value::Boolean(flag) => flag.to_string(),
            Value::Date(date) => date.iso(),
            Value::DateTime(moment) => moment.iso(),
            Value::NotApplicable => "n/a".to_string(),
        };
        let text = if text.is_empty() || text.ends_with('\n') {
            text
        } else {
            format!("{text}\n")
        };
        let became = if text == sample.note() {
            Became::Unchanged
        } else {
            Became::Changed
        };
        sample.set_note(text);
        return Ok(Change {
            field: field.to_string(),
            before,
            after: shown(sample.note()),
            kept: None,
            became,
        });
    }
    // A list is no value, and is said as its items: removing one was said
    // *unchanged*, from nothing to nothing.
    let before = value_at(sample, field).or_else(|| match fields::parse(field) {
        Ok(Field::Named {
            name,
            channel: Channel::Value,
        }) => sample
            .attribute(&name)
            .ok()
            .map(|held| Value::Text(shown_attribute(held))),
        _ => None,
    });
    let kept = kept_by(sample, field);
    // The name is what the file writes, compared as written: removing a `name:`
    // that says what the file's name says changes the file, and the name read
    // before and after is the same.
    let naming = matches!(
        fields::parse(field),
        Ok(Field::Reserved(fields::ReservedField::Name))
    );
    let written_before = sample.written_name().map(str::to_string);
    // A row's index changed moves the row: read at the old address after,
    // it was said `11 → —`, as if the row went away.
    let reindexed = match fields::parse(field) {
        Ok(Field::Cell { table, column, .. }) => sample
            .table(&table)
            .is_ok_and(|held| held.index_columns().contains(&column)),
        _ => false,
    };
    let assigned = reindexed.then(|| value.clone());
    let became = assign(sample, field, value, declared)?;
    let after = value_at(sample, field).or(assigned.filter(|value| !value.is_absent()));
    let untouched = if naming {
        written_before.as_deref() == sample.written_name()
    } else {
        match (&before, &after) {
            (Some(before), Some(after)) => value::equals(before, after),
            (None, None) => true,
            _ => false,
        }
    };
    let became =
        if untouched && matches!(became, Became::Changed | Became::Override | Became::Cleared) {
            Became::Unchanged
        } else {
            became
        };
    Ok(Change {
        field: field.to_string(),
        before,
        after,
        kept: kept.filter(|_| became != Became::Unchanged),
        became,
    })
}

fn assign(
    sample: &mut Sample,
    field_name: &str,
    value: Value,
    declared: Option<&PropertyDeclaration>,
) -> Result<Became, EditError> {
    let field = fields::parse(field_name).map_err(|error| match &error {
        // Beside a quantity of that name, the second is a channel it lacks:
        // said so, rather than that it named a column.
        fields::FieldError::MissingIndex { table, column } if sample.has_property(table) => {
            EditError::new(format!(
                "{column} is no channel of {table}: its channels are v, u, unit, symbol, \
                 readings and stats.*"
            ))
        }
        _ => EditError::new(error.to_string()),
    })?;
    match &field {
        // Tags are a list with a command of their own: `tags=a,b` was refused
        // as a name already taken.
        Field::Named { name, .. } if name.as_str() == "tags" => Err(EditError::new(
            "tags are not set as a value: samplekit FILE tag add <tag> --write adds one, \
             samplekit FILE tag remove <tag> --write takes one away",
        )),
        Field::Named { name, channel } => {
            if sample.has_property(name) {
                if let Some(refusal) = refuses_text(sample, name, *channel, &value) {
                    return Err(refusal);
                }
                let handle = sample
                    .property(name)
                    .map_err(|error| EditError::new(error.to_string()))?;
                // A sample read without a model carries no formula, so the
                // record is what says a formula owns the value.
                let records = handle.records();
                let computed = records.computed.is_some() || handle.peek(Property::is_computed);
                match channel {
                    // Readings are given as text and read as a list, by
                    // `set_readings`: a single value here is one number
                    // where several were measured.
                    Channel::Readings => {
                        return Err(EditError::new(format!(
                            "readings are a list: {name}.readings=1.011,1.010,1.012"
                        )));
                    }
                    Channel::Value => {
                        let given_back = matches!(value, Value::Absent)
                            && handle.peek(|property| {
                                property.readings().is_some() && property.has_declared_statistic()
                            });
                        set_by_hand(&handle, value);
                        if given_back {
                            return Ok(Became::Cleared);
                        }
                    }
                    Channel::Uncertainty => {
                        let magnitude = match &value {
                            Value::Integer(whole) => *whole as f64,
                            Value::Number(real) => *real,
                            Value::Absent => {
                                handle.set_uncertainty(None);
                                return Ok(Became::Cleared);
                            }
                            _ => {
                                return Err(EditError::new(
                                    "an uncertainty is a number, as 0.01".to_string(),
                                ));
                            }
                        };
                        let uncertainty = Uncertainty::new(magnitude)
                            .map_err(|error| EditError::new(error.to_string()))?;
                        if set_uncertainty_by_hand(&handle, uncertainty) {
                            return Ok(Became::Override);
                        }
                    }
                    Channel::Unit | Channel::Symbol => {
                        let mut presentation = handle.presentation();
                        let written = match &value {
                            Value::Absent => None,
                            other => Some(shown_value(other)),
                        };
                        if written
                            .as_deref()
                            .is_some_and(|text| text.chars().any(char::is_control))
                        {
                            return Err(EditError::new(format!(
                                "{field_name} is written in letters and signs: '{}' holds a \
                                 control character, as an escape a terminal reads",
                                written.unwrap_or_default().escape_default()
                            )));
                        }
                        if matches!(channel, Channel::Unit) {
                            presentation.unit = written;
                        } else {
                            presentation.symbol = written;
                        }
                        handle.set_presentation(presentation);
                    }
                    Channel::Stat(_) => {
                        return Err(EditError::new(format!(
                            "{field_name} is a statistic of readings, which nothing sets by hand"
                        )));
                    }
                }
                // An override is a hand on a channel a formula owns: an entered
                // value beside a computed uncertainty is nobody's output.
                let owns = match records.produced {
                    Some(Produced::Value) | None => matches!(channel, Channel::Value),
                    Some(Produced::Uncertainty) => false,
                };
                return Ok(if computed && owns {
                    Became::Override
                } else {
                    Became::Changed
                });
            }
            // A blank value removes the value there is; where there is none, it
            // has nothing to remove, and writes no empty name.
            if matches!(value, Value::Absent)
                && matches!(channel, Channel::Value)
                && !sample.has_attribute(name)
            {
                return Ok(Became::Unchanged);
            }
            if sample.has_attribute(name) || !matches!(channel, Channel::Value) {
                // A blank value removes the attribute, a list as a value.
                if matches!(value, Value::Absent) && matches!(channel, Channel::Value) {
                    sample
                        .remove_attribute(name)
                        .map_err(|error| EditError::new(error.to_string()))?;
                    return Ok(Became::Cleared);
                }
                if !matches!(channel, Channel::Value) {
                    return Err(EditError::new(format!(
                        "'{name}' is no property here, so it has no {}",
                        match channel {
                            Channel::Uncertainty => "uncertainty",
                            Channel::Unit => "unit",
                            Channel::Symbol => "symbol",
                            _ => "channel",
                        }
                    )));
                }
                // A declared quantity written as text before — `20,5` — is
                // put right by a number: the quantity it was meant to be.
                let repairs = declared.is_some()
                    && matches!(
                        value,
                        Value::Integer(_) | Value::Number(_) | Value::NotApplicable
                    );
                if !repairs {
                    refuse_text_for_a_quantity(name, &value, declared)?;
                    sample
                        .set_attribute(name.clone(), AttributeValue::scalar(value))
                        .map_err(|error| EditError::new(error.to_string()))?;
                    return Ok(Became::Changed);
                }
                sample
                    .remove_attribute(name)
                    .map_err(|error| EditError::new(error.to_string()))?;
            }
            refuse_text_for_a_quantity(name, &value, declared)?;
            // What the project declares is a quantity: no model is run to learn
            // it.
            if let Some(declared) = declared
                // Not applicable is a quantity's answer too.
                && matches!(
                    value,
                    Value::Integer(_) | Value::Number(_) | Value::NotApplicable
                )
            {
                let mut property = Property::stored(value);
                property.set_presentation(Presentation {
                    unit: declared.unit.clone(),
                    // The unit is data, and the file keeps it; a symbol is how
                    // the project shows the value, and is read from it at
                    // rendering — copied, a symbol changed later never reached
                    // the file.
                    symbol: None,
                    // The project's, and read from it at rendering.
                    precision: None,
                });
                sample
                    .set_property(name.clone(), property)
                    .map_err(|error| EditError::new(error.to_string()))?;
                return Ok(Became::NewQuantity);
            }
            // A name nobody declared is an attribute: something that belongs to
            // this file alone.
            let numeric = matches!(value, Value::Integer(_) | Value::Number(_));
            sample
                .set_attribute(name.clone(), AttributeValue::scalar(value))
                .map_err(|error| EditError::new(error.to_string()))?;
            Ok(Became::NewAttribute { numeric })
        }
        Field::Cell {
            table,
            column,
            row,
            channel,
        } => {
            if !matches!(channel, Channel::Value | Channel::Uncertainty) {
                return Err(EditError::new(format!(
                    "a cell takes its value, its uncertainty and its readings by hand: \
                     {table}.{column}[…]=1.05, {table}.{column}[…].u=0.01, \
                     {table}.{column}[…].readings=1.05,1.06"
                )));
            }
            // An address that names nothing is said as the value's own
            // address would say it: what exists, and the nearest.
            {
                let held = sample
                    .table(table)
                    .map_err(|error| EditError::new(error.to_string()))?;
                let view = held
                    .row(row)
                    .map_err(|error| EditError::new(error.to_string()))?;
                view.cell(column)
                    .map_err(|error| EditError::new(error.to_string()))?;
                // An index names its row: it is never empty, and it is a
                // name rather than a measure, so it carries no uncertainty.
                if held.index_columns().contains(column) {
                    if matches!(channel, Channel::Uncertainty) {
                        return Err(EditError::new(format!(
                            "{table}.{column} is the table's index: it names a row and \
                             carries no uncertainty"
                        )));
                    }
                    if value.is_absent() {
                        return Err(EditError::new(format!(
                            "{table}.{column} is the table's index: it names its row and \
                             cannot be cleared"
                        )));
                    }
                }
                if matches!(channel, Channel::Value) {
                    if let Some(refused) = refuses_text_in(held, table, column, &value, false) {
                        return Err(refused);
                    }
                    // An index names one row: a value that another row's
                    // index already holds would make two, and the table
                    // unreadable.
                    if let Some(at) = held.index_columns().iter().position(|name| name == column) {
                        let mut index: Vec<Value> = view.index().into_iter().cloned().collect();
                        index[at] = value.clone();
                        if let Ok(other) = held.row(&RowAddress::index(index.clone()))
                            && other.position() != view.position()
                        {
                            let shown: Vec<String> = index.iter().map(shown_value).collect();
                            return Err(EditError::new(format!(
                                "{table} already has a row at ({}): an index names one row",
                                shown.join(", ")
                            )));
                        }
                    }
                }
            }
            let presentation = sample
                .table(table)
                .ok()
                .and_then(|held| held.presentation_of(column, row).ok())
                .unwrap_or_default();
            // A derived cell set by hand is held as an override, exactly as a
            // derived property is.
            let held = sample
                .table(table)
                .ok()
                .and_then(|held| held.at(row, column).ok())
                .map(|cell| {
                    (
                        cell.records().clone(),
                        cell.peek_value(),
                        cell.peek_uncertainty().flatten(),
                        // Readings stay under a value written over them, with
                        // the value written beside them and whether their
                        // spread is a statistic of them.
                        cell.readings().cloned().map(|readings| {
                            (
                                readings,
                                cell.written_value().cloned(),
                                cell.convention().is_some(),
                            )
                        }),
                    )
                });
            let (kept, before, spread, readings) = match held {
                Some((records, before, spread, readings)) => {
                    (Some(records), before, spread, readings)
                }
                None => (None, None, None, None),
            };
            let (was, had) = (before.clone(), spread);
            // The channel not written stays as it was: a value written keeps
            // the cell's uncertainty, which rebuilding the cell from the value
            // alone dropped without a word; an uncertainty keeps its value.
            let (value, spread, cleared) = match channel {
                Channel::Value => (value, spread, false),
                _ => {
                    let Some(before) = before.filter(|before| !before.is_absent()) else {
                        return Err(EditError::new(format!(
                            "{field_name}: the cell holds no value to give an uncertainty to"
                        )));
                    };
                    match &value {
                        Value::Absent => (before, None, true),
                        Value::Integer(_) | Value::Number(_) => {
                            let magnitude = match value {
                                Value::Integer(whole) => whole as f64,
                                Value::Number(real) => real,
                                _ => unreachable!(),
                            };
                            let spread = Uncertainty::new(magnitude)
                                .map_err(|error| EditError::new(error.to_string()))?;
                            (before, Some(spread), false)
                        }
                        _ => {
                            return Err(EditError::new(
                                "an uncertainty is a number, as 0.01".to_string(),
                            ));
                        }
                    }
                }
            };
            // The cell as it was is no change, and nothing is marked: marked
            // first and found unchanged after, it was written as an override.
            let same_value = was.as_ref().is_some_and(|was| value::equals(was, &value));
            if same_value && had == spread {
                return Ok(Became::Unchanged);
            }
            let mut property = match readings {
                // **A value written over readings keeps them**, in a cell as in
                // a property: the value is written beside them and outranks the
                // statistic its column declares; an uncertainty their statistic
                // gives goes on following them.
                Some((readings, written, derived)) => {
                    let mut measured = Property::measured(readings, None);
                    let written = match channel {
                        Channel::Value => Some(value).filter(|value| !value.is_absent()),
                        _ => written,
                    };
                    measured.set_written_value(written);
                    if !derived || !matches!(channel, Channel::Value) {
                        measured.set_uncertainty(spread);
                    }
                    measured
                }
                None => {
                    let mut stored = Property::stored(value);
                    stored.set_uncertainty(spread);
                    stored
                }
            };
            property.set_presentation(presentation);
            // Marked only where a formula owns the channel written, as a
            // property is: a value entered beside a computed uncertainty
            // overrides nothing, nor an uncertainty given beside a computed
            // value.
            // A channel the hand overrode before stays overridden: writing the
            // other beside it gives nothing back to the formula.
            let owned = kept.as_ref().is_some_and(|records| {
                records.computed.is_some()
                    && (match channel {
                        Channel::Value => records.produced != Some(Produced::Uncertainty),
                        _ => records.produced == Some(Produced::Uncertainty),
                    } || records
                        .fingerprint
                        .as_ref()
                        .is_some_and(|fingerprint| fingerprint.is_edited()))
            });
            if let Some(records) = kept.filter(|records| records.computed.is_some()) {
                let shape = schema::property_as_is(&property);
                let digest = fingerprint::own_of(&shape, records.produced);
                property.set_records(Records {
                    failure: None,
                    fingerprint: Some(if owned {
                        digest.marked_edited()
                    } else {
                        digest
                    }),
                    computed: records.computed,
                    produced: records.produced,
                    statistics: None,
                    channel_only: records.channel_only,
                });
            }
            sample
                .update_row(table, row, vec![(column.clone(), property)])
                .map_err(|error| EditError::new(error.to_string()))?;
            Ok(if owned {
                Became::Override
            } else if cleared {
                Became::Cleared
            } else {
                Became::Changed
            })
        }
        Field::ListItem { .. } => Err(EditError::new(format!(
            "one value is set at a time, and {field_name} is a position in a list"
        ))),
        // The name is an attribute a user edits as any other: what the file
        // writes, never the file's name. A blank one removes `name:`, and the
        // file's name stands for it again.
        Field::Reserved(fields::ReservedField::Name) => {
            let written = match value {
                Value::Absent => None,
                Value::Text(text) => Some(text),
                other => Some(shown_value(&other)),
            };
            match written {
                None => {
                    if sample.written_name().is_none() {
                        return Ok(Became::Unchanged);
                    }
                    sample.set_name(None);
                    Ok(Became::Cleared)
                }
                Some(name) => {
                    if let Some(refused) = refuses_written_name(&name) {
                        return Err(refused);
                    }
                    sample.set_name(Some(name));
                    Ok(Became::Changed)
                }
            }
        }
        Field::Reserved(_) => Err(EditError::new(format!(
            "'{field_name}' is SampleKit's own, not a value of the sample"
        ))),
    }
}

/// Readings refused for a value the model computes, as its description says
/// it: a sample that never held the value has no record to say so, and the
/// model then refused to load the file, for the whole project.
pub fn readings_refused_by_model(
    config: Option<&crate::config::project_config::ProjectConfig>,
    name: &str,
) -> Option<EditError> {
    use crate::config::model_runtime::Origin;
    let description = crate::config::model_runtime::current_description(config?)?;
    let declared = description.properties.get(name)?;
    matches!(
        declared.value,
        Origin::Formula { .. } | Origin::Quantity { .. }
    )
    .then(|| {
        EditError::new(format!(
            "{name} is computed by the model: readings belong to a measured value"
        ))
    })
}

/// New readings of a quantity. They replace the readings before and nothing
/// written: a value written or entered stands, stale, beside them, and its
/// entered uncertainty, its declared statistic, its record, its unit and its
/// symbol stay. **No convention is carried**: a file records none, and a
/// convention is the model's.
pub fn set_readings(
    sample: &mut Sample,
    name: &Identifier,
    readings: Readings,
) -> Result<(), EditError> {
    // Readings are what was measured: a value a formula owns has none, and
    // the model then refused to load the file that gave it some. A statistic
    // of readings is recorded as computed too, and its readings are welcome.
    // A record of the uncertainty alone leaves the value measured: its
    // readings are welcome (`volume` measured, its uncertainty a formula's).
    let uncertainty_only = |handle: &crate::core::sample::PropertyHandle| {
        matches!(
            handle.records().produced,
            Some(crate::core::property::Produced::Uncertainty)
        ) || (handle.has_uncertainty_formula() && !handle.peek(Property::is_computed))
    };
    if let Ok(handle) = sample.property(name)
        && handle.readings().is_none()
        && !uncertainty_only(&handle)
        && (handle.records().computed.is_some() || handle.peek(Property::is_computed))
    {
        return Err(EditError::new(format!(
            "{name} is computed by the model: readings belong to a measured value"
        )));
    }
    // **New readings replace no writing**: a value written beside the old ones
    // — or entered where there were none — stands, stale, beside the new
    // evidence; an entered uncertainty and a declared statistic stay; the
    // record stays, and says the readings moved.
    let held = sample.property_if(name).map(|handle| {
        let records = handle.records().clone();
        let presentation = handle.presentation();
        handle.peek(|property| {
            let written = match property.readings() {
                // A statistic the model declared stays as it was, the statistic
                // of the readings before: its record then says *stale —
                // readings*, and compute takes it again. Rewritten here, its
                // digest would move, and read as a hand's. Any other value
                // beside them is a value written — the mean an earlier
                // SampleKit wrote included, which nothing tells from one typed
                // — and stands.
                Some(_) => property.written_value().cloned(),
                None => property.peek_value().filter(|value| !value.is_absent()),
            };
            let entered = (property.convention().is_none() && !property.has_uncertainty_formula())
                .then(|| property.peek_uncertainty().flatten())
                .flatten();
            let statistics = (
                property.declared_location(),
                property.declared_convention(),
                property.statistic_taken(),
            );
            (written, entered, statistics, records, presentation)
        })
    });
    let mut property = Property::measured(readings, None);
    if let Some((written, entered, (location, convention, taken), records, presentation)) = held {
        property.set_written_value(written);
        if entered.is_some() {
            property.set_uncertainty(entered);
        }
        if location.is_some() || convention.is_some() {
            property.declare_statistics(location, convention);
            if taken {
                property.take_statistic();
            }
        }
        property.set_records(records);
        property.set_presentation(presentation);
    }
    sample
        .set_property(name.clone(), property)
        .map_err(|error| EditError::new(error.to_string()))
}

/// New readings of a table's cell, by the rule a property's follow : they
/// replace the readings before and nothing written — a value the cell held
/// stands beside them, written, and so does an uncertainty entered with it; the
/// record of a statistic taken before stays, and says the readings moved. The
/// column's declared statistics reach the cell as the table takes it.
///
/// A column a formula fills takes none, as a computed property does not: the
/// model would refuse the file. An index names its row, and is no measurement.
pub fn set_cell_readings(
    sample: &mut Sample,
    table: &Identifier,
    row: &RowAddress,
    column: &Identifier,
    readings: Readings,
) -> Result<(), EditError> {
    let replacement = {
        let held = sample
            .table(table)
            .map_err(|error| EditError::new(error.to_string()))?;
        let view = held
            .row(row)
            .map_err(|error| EditError::new(error.to_string()))?;
        let cell = view
            .cell(column)
            .map_err(|error| EditError::new(error.to_string()))?;
        if held.index_columns().contains(column) {
            return Err(EditError::new(format!(
                "{table}.{column} is the table's index: it names a row, and takes no readings"
            )));
        }
        let computed = cell.records().computed.is_some() && cell.readings().is_none();
        if held.is_derived(column) || computed {
            return Err(EditError::new(format!(
                "{table}.{column} is computed by the model: readings belong to a measured value"
            )));
        }
        let written = match cell.readings() {
            Some(_) => cell.written_value().cloned(),
            None => cell.peek_value().filter(|value| !value.is_absent()),
        };
        let entered = cell
            .convention()
            .is_none()
            .then(|| cell.peek_uncertainty().flatten())
            .flatten();
        let mut property = Property::measured(readings, None);
        property.set_written_value(written);
        if entered.is_some() {
            property.set_uncertainty(entered);
        }
        property.set_records(cell.records().clone());
        property.set_presentation(cell.presentation().clone());
        property
    };
    sample
        .update_row(table, row, vec![(column.clone(), replacement)])
        .map_err(|error| EditError::new(error.to_string()))
}

/// A name the project declares as a quantity takes a number: `20,5`, text,
/// was written as an attribute of that name, which the model then refused as
/// the file loaded — a project no command could compute, said to be sound.
///
/// A number is asked for where the declaration gives a unit, which only a
/// number has. A declaration without one — `[property.style]` naming how a
/// figure's legend says it — describes text as well, and `style=titanium` was
/// refused with *write a number*.
fn refuse_text_for_a_quantity(
    name: &Identifier,
    value: &Value,
    declared: Option<&PropertyDeclaration>,
) -> Result<(), EditError> {
    let Some(declared) = declared.filter(|declared| declared.unit.is_some()) else {
        return Ok(());
    };
    if matches!(value, Value::Text(_) | Value::Boolean(_)) {
        let unit = declared
            .unit
            .as_deref()
            .map(|unit| format!(" in {unit}"))
            .unwrap_or_default();
        let hint = match value {
            Value::Text(text)
                if text.contains(',') && text.replace(',', ".").parse::<f64>().is_ok() =>
            {
                format!(
                    ": write {}, with a point for decimals",
                    text.replace(',', ".")
                )
            }
            _ => ": write a number".to_string(),
        };
        return Err(EditError::new(format!(
            "'{name}' is a property this project declares{unit}, and '{}' is {}{hint}",
            shown_value(value),
            not_a_number(value)
        )));
    }
    Ok(())
}

// ------------------------------------------------------------------ rows

/// The shape of `name` as the model's description says it declares it: where no
/// sample of the project holds the table yet, its first row still has columns
/// and an index to go into. None where the description is not current.
pub fn shape_from_model(
    config: &crate::config::project_config::ProjectConfig,
    name: &Identifier,
) -> Option<Lent> {
    use crate::config::model_runtime as runtime;
    let template = runtime::template_of(config)?;
    let described = runtime::current_description(config)?;
    let declared = described.table(name.as_str())?;
    let index: Vec<Identifier> = declared
        .index
        .iter()
        .map(|column| Identifier::new(column).ok())
        .collect::<Option<_>>()?;
    let mut columns: indexmap::IndexMap<Identifier, ColumnMeta> = indexmap::IndexMap::new();
    let mut derived = Vec::new();
    for column in &declared.columns {
        let id = Identifier::new(&column.name).ok()?;
        // A column whose value a formula gives: one whose formula gives the
        // uncertainty alone takes its value by hand.
        if matches!(
            column.value,
            runtime::Origin::Rows { .. } | runtime::Origin::Columns { .. }
        ) {
            derived.push(id.clone());
        }
        columns.insert(
            id,
            ColumnMeta {
                presentation: Presentation {
                    unit: column.unit.clone(),
                    symbol: None,
                    precision: None,
                },
                // A statistic the model declares is the model's to give when
                // it computes.
                statistics: Default::default(),
            },
        );
    }
    let table = Table::new(name.clone(), index, columns, Vec::new()).ok()?;
    Some(Lent {
        from: template.path().to_path_buf(),
        table,
        derived,
        records: Vec::new(),
    })
}

/// The shape of `name` in another sample of `collection`: its columns, empty,
/// and which of them the model fills.
pub fn shape_from(collection: &SampleList, except: &Path, name: &Identifier) -> Option<Lent> {
    for entry in collection.iter() {
        let path = entry.path.clone();
        if path.as_deref() == Some(except) {
            continue;
        }
        let source = entry.sample.borrow();
        let Ok(held) = source.table(name) else {
            continue;
        };
        let index: Vec<Identifier> = held.index_columns().to_vec();
        let mut columns: indexmap::IndexMap<Identifier, ColumnMeta> = indexmap::IndexMap::new();
        for column in held.column_names().into_iter().cloned().collect::<Vec<_>>() {
            let presentation = held
                .presentation_of(&column, &RowAddress::ordinal(0))
                .unwrap_or_default();
            let statistics = held
                .column(&column)
                .map(|view| view.statistics())
                .unwrap_or_default();
            columns.insert(
                column,
                ColumnMeta {
                    presentation,
                    statistics,
                },
            );
        }
        let column_names: Vec<Identifier> = columns.keys().cloned().collect();
        let derived = derived_columns(held, &column_names);
        let records = derived_records(held, &derived);
        let table = Table::new(name.clone(), index, columns, Vec::new()).ok()?;
        return Some(Lent {
            from: path.unwrap_or_else(|| PathBuf::from("?")),
            table,
            derived,
            records,
        });
    }
    None
}

/// The columns whose cells carry a record: the model's to fill.
fn derived_columns(table: &Table, columns: &[Identifier]) -> Vec<Identifier> {
    table.rows().next().map_or_else(Vec::new, |row| {
        columns
            .iter()
            // A column whose formula gives the uncertainty alone takes its
            // value by hand, and one whose cells' value is a statistic of
            // their readings takes readings: both are entered, not derived.
            .filter(|column| {
                row.cell(column).is_ok_and(|cell| {
                    let records = cell.records();
                    let of_readings = records.computed.as_ref().is_some_and(|inputs| {
                        inputs.contains_key(&crate::core::property::InputName::Named(
                            fingerprint::readings_key(),
                        ))
                    });
                    records.computed.is_some()
                        && records.produced != Some(Produced::Uncertainty)
                        && !of_readings
                })
            })
            .cloned()
            .collect()
    })
}

/// Each derived column's records, from the first row whose cell carries one.
fn derived_records(table: &Table, derived: &[Identifier]) -> Vec<(Identifier, Records)> {
    derived
        .iter()
        .filter_map(|column| {
            table.rows().find_map(|row| {
                row.cell(column)
                    .ok()
                    .map(|cell| cell.records().clone())
                    .filter(|records| records.computed.is_some())
                    .map(|records| (column.clone(), records))
            })
        })
        .collect()
}

/// A hand value in a derived column of a new row, held as a cell override is
/// in [`assign`]: the column's records, its digest marked edited where the
/// formula owns the value. Stored bare, it read *record missing* beside a
/// model and *source* without one, while the preview said *an override*.
fn overriding_cell(value: Value, records: Option<&Records>) -> (Property, bool) {
    let mut property = Property::stored(value);
    let Some(records) = records else {
        return (property, true);
    };
    // A value beside a computed uncertainty overrides nothing, and carries no
    // record of another row's: its uncertainty is owed, never computed.
    if records.produced == Some(Produced::Uncertainty) {
        return (property, false);
    }
    let owned = true;
    let digest = fingerprint::own_of(&schema::property_as_is(&property), records.produced);
    property.set_records(Records {
        failure: None,
        fingerprint: Some(if owned {
            digest.marked_edited()
        } else {
            digest
        }),
        computed: records.computed.clone(),
        produced: records.produced,
        statistics: None,
        channel_only: records.channel_only.clone(),
    });
    (property, owned)
}

/// A new row, its index given. A table this sample holds none of is borrowed
/// through `lend`: a sample `new` just wrote holds no table, since a table
/// with no rows reaches no file.
pub fn add_row(
    sample: &mut Sample,
    table: &str,
    cells: Vec<(String, Value)>,
    lend: impl FnOnce(&Identifier) -> Option<Lent>,
) -> Result<RowAdded, EditError> {
    add_row_with_readings(sample, table, cells, Vec::new(), lend)
}

/// A new row whose cells may hold readings — `stiffness.readings=1.05,1.06` beside
/// `temperature=40`. A cell given readings is measured, and takes the
/// statistics its column declares as the table takes it; a column a formula
/// fills, or the index, takes none.
pub fn add_row_with_readings(
    sample: &mut Sample,
    table: &str,
    cells: Vec<(String, Value)>,
    readings: Vec<(String, Readings)>,
    lend: impl FnOnce(&Identifier) -> Option<Lent>,
) -> Result<RowAdded, EditError> {
    let name = Identifier::new(table)
        .map_err(|error| EditError::new(format!("'{table}' is not a table's name: {error}")))?;
    let mut lent_derived = Vec::new();
    let mut lent_records = Vec::new();
    let lent_from = if sample.table(&name).is_err() {
        match lend(&name) {
            Some(lent) => {
                sample
                    .set_table(name.clone(), lent.table)
                    .map_err(|error| EditError::new(error.to_string()))?;
                lent_derived = lent.derived;
                lent_records = lent.records;
                Some(lent.from)
            }
            None => {
                let available: Vec<String> = sample
                    .table_names()
                    .iter()
                    .map(ToString::to_string)
                    .collect();
                return Err(EditError::new(if available.is_empty() {
                    format!(
                        "unknown table '{table}': this sample holds none, and no sample here \
                         holds one by that name\n  a table's columns come from the model: \
                         compute a sample once, or name a file that already has the table"
                    )
                } else {
                    let mut message = format!("unknown table '{table}'");
                    if let Some(nearest) = crate::core::identifier::nearest(
                        table,
                        available.iter().map(String::as_str),
                    ) {
                        message.push_str(&format!("\n  did you mean: '{nearest}'?"));
                    }
                    message.push_str(&format!("\n  available: {}", available.join(", ")));
                    message
                }));
            }
        }
    } else {
        None
    };
    let (columns, index, derived, records) = {
        let held = sample
            .table(&name)
            .map_err(|error| EditError::new(error.to_string()))?;
        let columns: Vec<Identifier> = held.column_names().into_iter().cloned().collect();
        let index: Vec<Identifier> = held.index_columns().to_vec();
        let derived = derived_columns(held, &columns);
        // A table just borrowed holds no cell to say what the model fills.
        let (derived, records) = if derived.is_empty() {
            (lent_derived, lent_records)
        } else {
            let records = derived_records(held, &derived);
            (derived, records)
        };
        (columns, index, derived, records)
    };
    let mut given = Vec::new();
    let mut row = Vec::new();
    let mut named: Vec<Identifier> = Vec::new();
    for (column, value) in cells {
        let column = Identifier::new(&column).map_err(|error| EditError::new(error.to_string()))?;
        // Written twice, one of the two would be dropped without a word.
        if named.contains(&column) {
            return Err(EditError::new(format!(
                "'{column}' is given twice: a row holds one value per column"
            )));
        }
        named.push(column.clone());
        if let Ok(held) = sample.table(&name)
            && let Some(refused) = refuses_text_in(held, &name, &column, &value, true)
        {
            return Err(refused);
        }
        if !columns.contains(&column) {
            return Err(EditError::new(format!(
                "'{table}' has no column '{column}'\n  columns: {}",
                columns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        // A cell a formula owns, written by hand, is an override.
        let (cell, overriding) = if derived.contains(&column) {
            let held = records
                .iter()
                .find(|(derived, _)| *derived == column)
                .map(|(_, records)| records);
            overriding_cell(value.clone(), held)
        } else {
            (Property::stored(value.clone()), false)
        };
        given.push((column.clone(), value, overriding));
        row.push((column, cell));
    }
    let mut measured = Vec::new();
    for (column, observed) in readings {
        let column = Identifier::new(&column).map_err(|error| EditError::new(error.to_string()))?;
        if named.contains(&column) {
            return Err(EditError::new(format!(
                "'{column}' is given twice: a row holds one value per column"
            )));
        }
        named.push(column.clone());
        if !columns.contains(&column) {
            return Err(EditError::new(format!(
                "'{table}' has no column '{column}'\n  columns: {}",
                columns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        if index.contains(&column) {
            return Err(EditError::new(format!(
                "{table}.{column} is the table's index: it names a row, and takes no readings"
            )));
        }
        if derived.contains(&column) {
            return Err(EditError::new(format!(
                "{table}.{column} is computed by the model: readings belong to a measured value"
            )));
        }
        measured.push((column.clone(), observed.as_slice().to_vec()));
        row.push((column, Property::measured(observed, None)));
    }
    for key in &index {
        if !row.iter().any(|(column, _)| column == key) {
            return Err(EditError::new(format!(
                "'{table}' is indexed by {}, and this row has no {key}",
                index
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    let left_absent = columns
        .iter()
        .filter(|column| {
            !derived.contains(column) && !row.iter().any(|(given, _)| &given == column)
        })
        .cloned()
        .collect();
    let by_the_model = derived
        .iter()
        .filter(|column| !row.iter().any(|(given, _)| &given == column))
        .cloned()
        .collect();
    sample
        .add_row(&name, row)
        .map_err(|error| EditError::new(error.to_string()))?;
    Ok(RowAdded {
        lent_from,
        given,
        readings: measured,
        left_absent,
        by_the_model,
    })
}

/// The inputs a stale value moved on from that now hold nothing: its value then
/// waits for them, as the model says — `status` said *stale — volume* where
/// `compute` said *waits for volume* of one value. `state` reads it too, so
/// that a value waiting is `waiting` without the model. A column read whole
/// waits for each of its cells emptied, `ageing.hardness[4]`.
pub fn emptied_inputs(sample: &Sample, state: &Freshness) -> Vec<String> {
    emptied_inputs_at(sample, state, None)
}

/// [`emptied_inputs`] of a table's cell, in the row `row` names — its table
/// and its index: the cell of its own row an input `row.hardness` names is
/// that row's. Said of a cell, `status` called *outdated — row.hardness* what
/// `compute` said waits for `ageing.hardness[4]`.
pub fn emptied_inputs_at(
    sample: &Sample,
    state: &Freshness,
    row: Option<(&Identifier, &[Value])>,
) -> Vec<String> {
    let Freshness::Stale { changed, upstream } = state else {
        return Vec::new();
    };
    if changed.is_empty() && upstream.is_empty() {
        return Vec::new();
    }
    // Readings with no value are owed their statistic, which a computation
    // takes: nothing to wait for. Said *waits for radius*, a piece whose hardnesses
    // were read was told to wait for a number nobody needed to type.
    let empty = |property: &Property| {
        property.readings().is_none()
            && property
                .peek_value()
                .is_none_or(|value| matches!(value, Value::Absent))
    };
    let addressed = |table: &Identifier, column: &Identifier, index: &[&Value]| {
        let index: Vec<String> = index.iter().map(|value| shown_value(value)).collect();
        format!("{table}.{column}[{}]", index.join(", "))
    };
    let mut named: Vec<String> = Vec::new();
    let mut matched = 0;
    for input in changed {
        let found: Vec<String> = match input {
            crate::core::property::InputName::Named(name) => sample
                .property(name)
                .ok()
                .filter(|handle| handle.peek(empty))
                .map(|_| vec![name.to_string()])
                .unwrap_or_default(),
            // A column a formula gives owes its new rows' cells, which nobody
            // emptied: what reads it is outdated, said so, never *waits for
            // curve.stress[11]* of a row just added. Known from the model, or
            // from the records its other cells carry.
            crate::core::property::InputName::Column { table, column } => sample
                .table(table)
                .map(|held| {
                    let computed = held.is_derived(column)
                        || held.rows().any(|each| {
                            each.cell(column).is_ok_and(|cell| {
                                cell.is_computed() || cell.records().computed.is_some()
                            })
                        });
                    if computed {
                        return Vec::new();
                    }
                    held.rows()
                        .filter(|each| each.cell(column).is_ok_and(empty))
                        .map(|each| addressed(table, column, &each.index()))
                        .collect()
                })
                .unwrap_or_default(),
            crate::core::property::InputName::Cell(column)
            | crate::core::property::InputName::CellValue(column) => row
                .and_then(|(table, index)| {
                    let held = sample.table(table).ok()?;
                    let each = held
                        .rows()
                        .find(|each| each.index().into_iter().eq(index.iter()))?;
                    each.cell(column)
                        .is_ok_and(empty)
                        .then(|| vec![addressed(table, column, &each.index())])
                })
                .unwrap_or_default(),
        };
        if !found.is_empty() {
            matched += 1;
            named.extend(found);
        }
    }
    // An input itself waiting is waited for, as the model says: `roughness`
    // waits for `ductility`, which waits for the `volume` emptied.
    for input in upstream {
        if let crate::core::property::InputName::Named(name) = input
            && fingerprint::check_property(sample, name)
                .ok()
                .is_some_and(|state| !emptied_inputs(sample, &state).is_empty())
        {
            matched += 1;
            named.push(name.to_string());
        }
    }
    // Waiting only where every input that moved is empty, or waits: one that
    // holds a new value makes the value stale, whatever the others hold.
    if matched == changed.len() + upstream.len() {
        named
    } else {
        Vec::new()
    }
}

// ------------------------------------------------------------ what it costs

/// Every derived value of a sample that is not current: a property by its name,
/// and a table column once, as `table.column`, with how many of its rows are. A
/// value whose records cannot be followed is listed.
/// What a change makes not current: in `after` and not in `before` as it
/// stands — a value by its name and the way it stands, and a column's rows
/// beyond those that already stood so. Compared by name alone, a column
/// holding an override was already *not current*, and the rows a change made
/// outdated beside it went unsaid.
pub fn newly_not_current(before: &[NotCurrent], after: &[NotCurrent]) -> Vec<NotCurrent> {
    after
        .iter()
        .filter_map(|entry| {
            let was = before.iter().find(|was| {
                was.name == entry.name
                    && std::mem::discriminant(&was.state) == std::mem::discriminant(&entry.state)
            });
            match was {
                None => Some(entry.clone()),
                Some(was) if entry.rows > was.rows => Some(NotCurrent {
                    rows: entry.rows - was.rows,
                    ..entry.clone()
                }),
                Some(_) => None,
            }
        })
        .collect()
}

pub fn not_current(sample: &Sample) -> Vec<NotCurrent> {
    let standing = fingerprint::check_sample(sample);
    let mut found = Vec::new();
    let mut named: Vec<_> = standing.properties.into_iter().collect();
    named.sort_by_key(|(name, _)| name.to_string());
    for (name, state) in named {
        if matches!(state, Freshness::Source | Freshness::Current) {
            continue;
        }
        // **A value and its uncertainty given by two formulas count apart**
        //: where their records say which input each read alone, an
        // input moved is said of the channel it reaches, `q` for the value,
        // `q.u` for the uncertainty, and of both where it reaches both.
        let channels = match &state {
            Freshness::Stale { changed, upstream } => sample
                .property(&name)
                .ok()
                .map(|handle| handle.records().channel_only)
                .filter(|only| !only.is_empty())
                .map(|only| {
                    let reach = |channel: Produced| {
                        changed
                            .iter()
                            .chain(upstream)
                            .any(|input| only.get(input).is_none_or(|alone| *alone == channel))
                    };
                    (reach(Produced::Value), reach(Produced::Uncertainty))
                }),
            _ => None,
        };
        match channels {
            Some((value, uncertainty)) => {
                if value || !uncertainty {
                    found.push(NotCurrent {
                        name: name.to_string(),
                        state: state.clone(),
                        rows: 0,
                    });
                }
                if uncertainty {
                    found.push(NotCurrent {
                        name: format!("{name}.u"),
                        state,
                        rows: 0,
                    });
                }
            }
            // Two formulas read the same inputs: their record is one, and
            // both are outdated by what moved — each counted.
            None if matches!(state, Freshness::Stale { .. })
                && sample
                    .property(&name)
                    .is_ok_and(|handle| handle.peek(Property::has_two_formulas)) =>
            {
                found.push(NotCurrent {
                    name: name.to_string(),
                    state: state.clone(),
                    rows: 0,
                });
                found.push(NotCurrent {
                    name: format!("{name}.u"),
                    state,
                    rows: 0,
                });
            }
            None => found.push(NotCurrent {
                name: name.to_string(),
                state,
                rows: 0,
            }),
        }
    }
    for (table, cells) in standing.tables {
        for (column, rows) in cells {
            // A column once for each way its rows stand, counting its rows:
            // one edited row said for a column whose other rows are stale
            // hid them, and a defect behind a first stale row went unsaid.
            let mut ways: Vec<NotCurrent> = Vec::new();
            for (_, state) in rows {
                if matches!(state, Freshness::Source | Freshness::Current) {
                    continue;
                }
                let kind = std::mem::discriminant(&state);
                match ways
                    .iter_mut()
                    .find(|way| std::mem::discriminant(&way.state) == kind)
                {
                    Some(way) => way.rows += 1,
                    None => ways.push(NotCurrent {
                        name: format!("{table}.{column}"),
                        state,
                        rows: 1,
                    }),
                }
            }
            found.extend(ways);
        }
    }
    found
}
