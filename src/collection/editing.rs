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
use crate::core::value::{self, Readings, Value, ValueKind};
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

/// A new sample shaped like `pattern` — what `new --like` and the workbench
/// both write: its attributes carried, its measured quantities empty for their
/// author to fill (those in `keep` with their values), its derived ones left
/// out, records and all, and its tables' columns without their rows, which are
/// another sample's measurements.
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
        .find(|entry| entry.sample.borrow().name() == Some(name))?;
    Some(EditError::new(format!(
        "{name} is already a sample's name, in {}: a name identifies one sample",
        taken
            .path
            .as_deref()
            .map_or_else(|| "?".to_string(), |path| path.display().to_string())
    )))
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
    let mut left_out: Vec<String> = Vec::new();
    let mut tables: Vec<String> = Vec::new();
    let mut attributes: Vec<(String, String)> = Vec::new();
    let mut not_carried: Vec<String> = Vec::new();

    if let Some(pattern) = pattern {
        // A name to keep that the pattern does not hold was a typo, and
        // accepted without a word.
        if let Some(missing) = keep.iter().find(|kept| {
            !pattern
                .property_names()
                .iter()
                .any(|held| held.as_str() == kept.as_str())
        }) {
            // A table is no value to keep: its rows are the pattern's own
            // measurements, and saying it held nothing of that name was false.
            if pattern
                .table_names()
                .iter()
                .any(|table| table.as_str() == missing.as_str())
            {
                return Err(EditError::new(format!(
                    "--keep {missing}: --keep takes values, and {missing} is a table — its rows \
                     are that sample's own measurements, and are not carried"
                )));
            }
            return Err(EditError::new(format!(
                "--keep {missing}: the pattern holds no value of that name"
            )));
        }
        for held in pattern
            .property_names()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            let handle = pattern
                .property(&held)
                .map_err(|error| EditError::new(error.to_string()))?;
            // A value the file records as derived belongs to the model, not to
            // a new file: it is left out entirely, records and all. `computed`
            // present — even empty — is what says a formula owns this value. A
            // measured value carries a fingerprint of its own as soon as
            // something derives from it, so an empty record is not the test.
            // **A value, not a record**: a record whose channel is the
            // uncertainty's says the value beside it was entered — a reading
            // whose uncertainty is a formula of it is still a reading, and was
            // left out of the new sample as if the model computed it. **Nor
            // readings**: a statistic of them records `readings` as its input,
            // and is still a measurement — the new sample's own, to fill in.
            let derived = handle.peek(|property| {
                let records = property.records();
                records.computed.is_some()
                    && records.produced != Some(Produced::Uncertainty)
                    && property.readings().is_none()
            });
            if derived {
                // Kept, it would be written with no record: a computed value
                // passed off as entered.
                if keep.iter().any(|kept| kept == held.as_str()) {
                    return Err(EditError::new(format!(
                        "--keep {held}: the model computes it, and computes it for the new \
                         sample too"
                    )));
                }
                left_out.push(held.to_string());
                continue;
            }
            let presentation = handle.presentation();
            let keeping = keep.iter().any(|kept| kept == held.as_str());
            let value = if keeping {
                handle
                    .peek(|property| property.peek_value())
                    .unwrap_or_else(Value::absent)
            } else {
                Value::absent()
            };
            // Kept whole: the readings a kept value stands for come with it,
            // a value written beside them only where one was, its entered
            // uncertainty, and the statistics declared for it.
            let mut property = match handle.readings().filter(|_| keeping) {
                Some(readings) => {
                    let mut measured = Property::measured(readings, None);
                    measured.set_written_value(
                        handle.peek(|property| property.written_value().cloned()),
                    );
                    measured
                }
                None => Property::stored(value),
            };
            if keeping {
                let (location, convention) =
                    handle.peek(|kept| (kept.declared_location(), kept.declared_convention()));
                if location.is_some() || convention.is_some() {
                    property.declare_statistics(location, convention);
                }
                if let Some(spread) = handle
                    .peek(|property| property.peek_uncertainty())
                    .flatten()
                {
                    property.set_uncertainty(Some(spread));
                }
            }
            property.set_presentation(presentation);
            sample
                .set_property(held.clone(), property)
                .map_err(|error| EditError::new(error.to_string()))?;
            measured.push(held.to_string());
        }
        // An attribute describes the sample, so it is carried; the values
        // that identify this one are the author's to change.
        for held in pattern
            .attribute_names()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            if let Ok(value) = pattern.attribute(&held) {
                // Its dates — when it was made, measured, received — and its
                // `status` are the pattern's own, and were carried into the new
                // sample as if they were its too. Here, so that `new --like`
                // and the workbench's `N` agree.
                let dated =
                    |value: &Value| matches!(value.kind(), ValueKind::Date | ValueKind::DateTime);
                if held.as_str() == "status"
                    || value.as_scalar().is_some_and(dated)
                    || value.as_list().is_some_and(|items| items.iter().any(dated))
                {
                    not_carried.push(held.to_string());
                    continue;
                }
                // Shown with its value, not only its name: `brewer: ana`
                // carried silently into a new sample is someone else's
                // identity, published without anyone reading it.
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
                let carried = value.clone();
                sample
                    .set_attribute(held.clone(), carried)
                    .map_err(|error| EditError::new(error.to_string()))?;
                attributes.push((held.to_string(), shown));
            }
        }
        // A table keeps its columns and loses its rows: the columns describe
        // the measurement, the rows *are* another sample's measurements.
        for held in pattern
            .table_names()
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
        {
            let table = pattern
                .table(&held)
                .map_err(|error| EditError::new(error.to_string()))?;
            let index: Vec<Identifier> = table.index_columns().to_vec();
            let mut columns: indexmap::IndexMap<Identifier, ColumnMeta> = indexmap::IndexMap::new();
            for column in table
                .column_names()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
            {
                let presentation = table
                    .presentation_of(&column, &RowAddress::ordinal(0))
                    .unwrap_or(Presentation {
                        unit: None,
                        symbol: None,
                        precision: None,
                    });
                // The statistics its readings are read through are the
                // column's, as its unit is.
                let statistics = table
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
            let empty = Table::new(held.clone(), index, columns, Vec::new())
                .map_err(|error| EditError::new(error.to_string()))?;
            sample
                .set_table(held.clone(), empty)
                .map_err(|error| EditError::new(error.to_string()))?;
            tables.push(held.to_string());
        }
    }
    Ok(Shaped {
        sample,
        measured,
        left_out,
        tables,
        attributes,
        not_carried,
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
            "'{written}' is not a change: write it as <field>=<value>, such as fg=1.010"
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
    value_of(text)
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
    // not. Storing it as text would put a string where numbers belong.
    if text.starts_with('[') {
        return Err(EditError::new(format!(
            "'{text}' is a list, and a value is one number\n  \
             repeated measurements of one quantity are its readings: fg.readings={text}"
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
        return Err(EditError::new(format!(
            "no readings given: they are written {field}=1.011,1.010,1.012"
        )));
    }
    let mut numbers = Vec::new();
    for piece in listed.split(',') {
        let piece = piece.trim();
        numbers.push(piece.parse::<f64>().map_err(|_| {
            EditError::new(format!(
                "'{piece}' is not a number: readings are written {field}=1.011,1.010,1.012"
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
             {quantity}={only}; readings are written {field}={only},…"
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

/// Whether an assignment gives readings — `og.readings=…`, or a cell's
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
/// `[property.og]`'s for `og` and its readings, a column's for its cells, the
/// uncertainty's for `og.u`. A precision is declared in the project and nowhere
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
/// left `6.081250000000001` — and every digit it holds where none is declared.
/// Text, dates and the rest as the file writes them.
pub fn previewed(
    value: &Value,
    field: &str,
    config: Option<&crate::config::project_config::ProjectConfig>,
) -> String {
    shown_at(value, declared_precision(field, config))
}

/// A value at `precision`, every digit without one: the one rule a preview
/// shows a number by.
pub fn shown_at(value: &Value, precision: Option<crate::core::formatting::Precision>) -> String {
    match (value, precision) {
        (Value::Number(_) | Value::Integer(_), Some(precision)) => {
            crate::core::formatting::format_value(
                value,
                &crate::core::formatting::Resolved {
                    unit: None,
                    symbol: None,
                    separator: "±".to_string(),
                    precision: Some(precision),
                },
            )
        }
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
        (was, is)
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
    let shape = handle.peek(schema::property_as_is);
    // Written the way the check computes it, or the two disagree and an
    // untouched channel reads edited.
    let digest = fingerprint::own_of(&shape, kept.produced);
    let digest = if owned {
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
    let handle = sample.property(&name).ok()?;
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
/// `gravity=1.011` — rather than the address `set` takes.
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
    let what = unit.map_or_else(
        || format!("{table}.{column} holds numbers"),
        |unit| format!("{table}.{column} is measured in {unit}"),
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
/// `og=true` was written `v: true`.
fn refuses_text(
    sample: &Sample,
    name: &Identifier,
    channel: Channel,
    value: &Value,
) -> Option<EditError> {
    if !matches!(channel, Channel::Value) || !matches!(value, Value::Text(_) | Value::Boolean(_)) {
        return None;
    }
    let handle = sample.property(name).ok()?;
    let numeric = handle.peek(|property| {
        matches!(
            property.peek_value(),
            Some(Value::Integer(_) | Value::Number(_))
        )
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
/// for it is written as they write it: `og=1.048` beside brews that measure
/// `og` was written as an attribute, which the model then refused to read.
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
/// its source: a name only the model declares, `self.ibu =
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
    let before = value_at(sample, field);
    let kept = kept_by(sample, field);
    // The name is what the file writes, compared as written: removing a `name:`
    // that says what the file's name says changes the file, and the name read
    // before and after is the same.
    let naming = matches!(
        fields::parse(field),
        Ok(Field::Reserved(fields::ReservedField::Name))
    );
    let written_before = sample.written_name().map(str::to_string);
    let became = assign(sample, field, value, declared)?;
    let after = value_at(sample, field);
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
            "tags are not set as a value: samplekit tag add <tag> FILE --write adds one, \
             samplekit tag remove <tag> FILE --write takes one away",
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
                    Channel::Value => set_by_hand(&handle, value),
                    Channel::Uncertainty => {
                        let magnitude = match &value {
                            Value::Integer(whole) => *whole as f64,
                            Value::Number(real) => *real,
                            Value::Absent => {
                                handle.set_uncertainty(None);
                                return Ok(Became::Cleared);
                            }
                            _ => {
                                return Err(EditError::new(format!(
                                    "an uncertainty is a number: {field_name}=0.01"
                                )));
                            }
                        };
                        let uncertainty = Uncertainty::new(magnitude)
                            .map_err(|error| EditError::new(error.to_string()))?;
                        handle.set_uncertainty(Some(uncertainty));
                    }
                    Channel::Unit | Channel::Symbol => {
                        let mut presentation = handle.presentation();
                        let written = match &value {
                            Value::Absent => None,
                            other => Some(shown_value(other)),
                        };
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
                if !matches!(channel, Channel::Value) {
                    return Err(EditError::new(format!(
                        "'{name}' holds no quantity here, so it has no {}",
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
                            return Err(EditError::new(format!(
                                "an uncertainty is a number: {field_name}=0.01"
                            )));
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
            let owned = kept.as_ref().is_some_and(|records| {
                records.computed.is_some()
                    && match channel {
                        Channel::Value => records.produced != Some(Produced::Uncertainty),
                        _ => records.produced == Some(Produced::Uncertainty),
                    }
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
    let held = sample.property(name).ok().map(|handle| {
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
            let statistics = (property.declared_location(), property.declared_convention());
            (written, entered, statistics, records, presentation)
        })
    });
    let mut property = Property::measured(readings, None);
    if let Some((written, entered, (location, convention), records, presentation)) = held {
        property.set_written_value(written);
        if entered.is_some() {
            property.set_uncertainty(entered);
        }
        if location.is_some() || convention.is_some() {
            property.declare_statistics(location, convention);
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
/// figure's legend says it — describes text as well, and `style=ipa` was
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
            "'{name}' is a quantity this project declares{unit}, and '{}' is {}{hint}",
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
    use crate::config::model_runtime::{self as runtime, Origin};
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
        if matches!(column.value, Origin::Rows { .. } | Origin::Columns { .. }) {
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
            .filter(|column| {
                row.cell(column)
                    .is_ok_and(|cell| cell.records().computed.is_some())
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
    // A value beside a computed uncertainty overrides nothing.
    let owned = records.produced != Some(Produced::Uncertainty);
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

/// A new row whose cells may hold readings — `sweetness.readings=1.05,1.06` beside
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
/// that a value waiting is `waiting` without the model.
pub fn emptied_inputs(sample: &Sample, state: &Freshness) -> Vec<String> {
    let Freshness::Stale { changed, upstream } = state else {
        return Vec::new();
    };
    if changed.is_empty() && upstream.is_empty() {
        return Vec::new();
    }
    let emptied: Vec<String> = changed
        .iter()
        .filter_map(|input| match input {
            crate::core::property::InputName::Named(name) => sample
                .property(name)
                .ok()
                .filter(|handle| {
                    handle
                        .peek(Property::peek_value)
                        .is_none_or(|value| matches!(value, Value::Absent))
                })
                .map(|_| name.to_string()),
            _ => None,
        })
        .collect();
    // An input itself waiting is waited for, as the model says: `haze`
    // waits for `plato`, which waits for the `volume` emptied.
    let waiting: Vec<String> = upstream
        .iter()
        .filter_map(|input| match input {
            crate::core::property::InputName::Named(name) => {
                fingerprint::check_property(sample, name)
                    .ok()
                    .filter(|state| !emptied_inputs(sample, state).is_empty())
                    .map(|_| name.to_string())
            }
            _ => None,
        })
        .collect();
    // Waiting only where every input that moved is empty, or waits: one that
    // holds a new value makes the value stale, whatever the others hold.
    if emptied.len() + waiting.len() == changed.len() + upstream.len() {
        emptied.into_iter().chain(waiting).collect()
    } else {
        Vec::new()
    }
}

// ------------------------------------------------------------ what it costs

/// Every derived value of a sample that is not current: a property by its name,
/// and a table column once, as `table.column`, with how many of its rows are. A
/// value whose records cannot be followed is listed.
pub fn not_current(sample: &Sample) -> Vec<NotCurrent> {
    let standing = fingerprint::check_sample(sample);
    let mut found = Vec::new();
    let mut named: Vec<_> = standing.properties.into_iter().collect();
    named.sort_by_key(|(name, _)| name.to_string());
    for (name, state) in named {
        if !matches!(state, Freshness::Source | Freshness::Current) {
            found.push(NotCurrent {
                name: name.to_string(),
                state,
                rows: 0,
            });
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
