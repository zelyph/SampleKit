//! The syntax that names any value inside a sample, and the enumeration that
//! makes a wrong name answerable with the right one.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::core::identifier::{Identifier, IdentifierError};
use crate::core::sample::{AttributeValue, Sample};
use crate::core::statistics::Summary;
use crate::core::table::{IndexReport, RowAddress};
use crate::core::value::{Date, DateTime, Value};

// ------------------------------------------------------------------- types

/// Anything addressable. `Named` covers a property and an attribute alike:
/// they are spelled the same and only the data says which, so the syntax stays
/// ignorant of what a name turns out to be.
///
/// `PartialEq` and not `Eq`: a cell's index holds `Value`s, and a number is not
/// `Eq`. Comparing two fields is comparing two paths, which is what a test and
/// a picker do; nothing keys a map by one.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Named {
        name: Identifier,
        channel: Channel,
    },
    Cell {
        table: Identifier,
        column: Identifier,
        row: RowAddress,
        channel: Channel,
    },
    ListItem {
        name: Identifier,
        position: usize,
    },
    Reserved(ReservedField),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Value,
    Uncertainty,
    Unit,
    Symbol,
    /// The repeated observations themselves, as a list. The one channel that is
    /// not one number: `stats` summarises them, this one shows them.
    Readings,
    Stat(Statistic),
}

/// `Summary`'s fields, all ten, under their own names. An abbreviation invented
/// here would reintroduce what `statistics` refused: `stdev` does not say
/// whether it divides by `n` or `n - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Statistic {
    Count,
    Minimum,
    Maximum,
    Mean,
    Median,
    FirstQuartile,
    ThirdQuartile,
    SampleStdev,
    PopulationStdev,
    StandardError,
}

/// What SampleKit supplies rather than the file: `name` is the sample's own,
/// `path` and `filename` come from the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedField {
    Name,
    Path,
    Filename,
    /// The folder of the project the sample belongs to, by its name — `ana` for
    /// `ana/.samplekitrc`.
    Project,
    /// How the sample stands, as `status` and `validate` say it: a set of
    /// words, read by the caller and carried on the subject.
    State,
}

/// One word of a sample's state, in the order a column lists them: the gravest
/// first, as `status` ranks a failure over a staleness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Failed,
    Defective,
    Stale,
    NeverComputed,
    Waiting,
    Edited,
    Current,
}

/// What a filter asks of `state`: one state, or `not_current`, which is what
/// `status --exit-code` fails on — every state a value can be in but waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWord {
    Is(State),
    NotCurrent,
}

/// A sample's states, and whether its model was read to find them. What
/// the files say is `validation::states`'; what the model owes, its plan's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct States {
    held: Vec<State>,
    model_read: bool,
}

impl State {
    /// The word a filter and a column write, `status --json`'s in snake case.
    pub fn word(self) -> &'static str {
        match self {
            State::Failed => "failed",
            State::Defective => "defective",
            State::Stale => "outdated",
            State::NeverComputed => "never_computed",
            State::Waiting => "waiting",
            State::Edited => "edited",
            State::Current => "current",
        }
    }

    /// A state a value is in, rather than one its file is.
    fn of_a_value(self) -> bool {
        !matches!(self, State::Defective | State::Current)
    }
}

impl StateWord {
    /// Every word, in the order completion offers them.
    pub const WORDS: [&'static str; 8] = [
        "current",
        "not_current",
        "outdated",
        "failed",
        "edited",
        "waiting",
        "never_computed",
        "defective",
    ];

    pub fn parse(word: &str) -> Option<StateWord> {
        Some(match word {
            "not_current" => StateWord::NotCurrent,
            "current" => StateWord::Is(State::Current),
            "outdated" => StateWord::Is(State::Stale),
            "failed" => StateWord::Is(State::Failed),
            "edited" => StateWord::Is(State::Edited),
            "waiting" => StateWord::Is(State::Waiting),
            "never_computed" => StateWord::Is(State::NeverComputed),
            "defective" => StateWord::Is(State::Defective),
            _ => return None,
        })
    }

    /// The word to suggest for one that is not a state: the nearest, and
    /// `outdated` for `stale`, the word it replaced, which no edit distance
    /// would find.
    pub fn suggestion(written: &str) -> Option<String> {
        if written == "stale" {
            return Some("outdated".to_string());
        }
        crate::core::identifier::nearest(written, StateWord::WORDS)
    }

    /// Whether the model could make this word true where the files do not:
    /// what a caller reads the model's plan for, and nothing else.
    pub fn needs_the_model(self) -> bool {
        matches!(
            self,
            StateWord::NotCurrent
                | StateWord::Is(State::Current | State::NeverComputed | State::Waiting)
        )
    }
}

impl States {
    /// `current` is added where no value is in another state and the model
    /// was read: without it, a value never computed may still be owed.
    pub fn new(mut held: Vec<State>, model_read: bool) -> States {
        held.retain(|state| *state != State::Current);
        held.sort();
        held.dedup();
        if model_read && !held.iter().any(|state| state.of_a_value()) {
            held.push(State::Current);
            held.sort();
        }
        States { held, model_read }
    }

    pub fn held(&self) -> &[State] {
        &self.held
    }

    pub fn model_read(&self) -> bool {
        self.model_read
    }

    /// `Some(true)` where the sample holds the word, `Some(false)` where it is
    /// known not to, `None` where only the model could say and it was not read:
    /// an outdated value is `not_current` without asking anyone, and its
    /// absence proves nothing.
    pub fn answers(&self, word: StateWord) -> Option<bool> {
        let held = match word {
            StateWord::Is(state) => self.held.contains(&state),
            StateWord::NotCurrent => self.not_current(),
        };
        if held {
            return Some(true);
        }
        if self.model_read || !word.needs_the_model() {
            return Some(false);
        }
        // `current` is denied by any value in another state, which the files
        // can say; the rest only the model can deny.
        if word == StateWord::Is(State::Current) && self.held.iter().any(|state| state.of_a_value())
        {
            return Some(false);
        }
        None
    }

    fn not_current(&self) -> bool {
        self.held.iter().any(|state| {
            matches!(
                state,
                State::Failed | State::Stale | State::Edited | State::NeverComputed
            )
        })
    }
}

/// What a field is resolved against: the sample, where it came from, and what
/// the collection knows how to name.
pub struct Subject<'a> {
    pub sample: &'a Sample,
    pub path: Option<&'a Path>,
    pub vocabulary: &'a Vocabulary,
    /// The sample's states, where whoever built the subject read them: only a
    /// filter or a column naming `state` pays for them.
    pub states: Option<&'a States>,
}

/// Every name any sample in the collection has. Built once, shared, and what
/// decides absent from unknown.
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    names: Vec<Identifier>,
    tables: Vec<(Identifier, Vec<Identifier>)>,
    /// The tags actually in use. Values rather than fields, kept here because
    /// *is this a tag nobody uses* is the same question as *is this a name
    /// nobody has*, and one answer needs one place.
    tags: Vec<Identifier>,
}

/// Almost everything resolves to one scalar. `tags` does not.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Scalar(Option<Value>),
    List(Vec<Value>),
    Tags(Vec<Identifier>),
}

impl Resolution {
    /// The scalar, or `None` for a shape that is not one.
    pub fn scalar(&self) -> Option<&Value> {
        match self {
            Resolution::Scalar(value) => value.as_ref(),
            Resolution::List(_) | Resolution::Tags(_) => None,
        }
    }
}

// ------------------------------------------------------------------ parsing

/// A path, without consulting any sample. What a name turns out to *be* is
/// resolution's business; this only says the path is well formed.
pub fn parse(source: &str) -> Result<Field, FieldError> {
    let text = source.trim();
    if text.is_empty() {
        return Err(malformed(source, "a field path is empty"));
    }
    match text.find('[') {
        Some(open) => parse_cell(source, text, open),
        None => parse_named(source, text),
    }
}

fn parse_named(source: &str, text: &str) -> Result<Field, FieldError> {
    let segments: Vec<&str> = text.split('.').collect();
    let name = segments[0];
    if let Some(reserved) = reserved_of(name) {
        if segments.len() > 1 {
            return Err(FieldError::UnknownChannel {
                name: segments[1..].join("."),
                on: name.to_string(),
                available: Vec::new(),
            });
        }
        return Ok(Field::Reserved(reserved));
    }
    // `malt.uncertanty` and `mashing.wort` have one shape and two
    // diagnoses: a misspelled channel, and a column that needs its brackets.
    // Nearness decides, because *count the columns* is a different fix from
    // *correct the spelling* and neither message helps with the other mistake.
    let channel = match channel_of(source, &segments[1..], name) {
        Ok(channel) => channel,
        Err(FieldError::UnknownChannel {
            name: written,
            on,
            available,
        }) => {
            // A column without its index has exactly two segments and a second
            // one that is nothing like a channel. `stats` is a channel that
            // needs a statistic after it, and three segments are never a
            // column, so neither of those falls through to here.
            let looks_like_a_column = segments.len() == 2
                && written != "stats"
                && !RETIRED_CHANNELS.contains(&written.as_str())
                && nearest(&written, &available).is_none()
                && Identifier::new(&written).is_ok();
            if looks_like_a_column {
                return Err(FieldError::MissingIndex {
                    table: identifier(source, name)?,
                    column: identifier(source, &written)?,
                });
            }
            return Err(FieldError::UnknownChannel {
                name: written,
                on,
                available,
            });
        }
        Err(other) => return Err(other),
    };
    Ok(Field::Named {
        name: identifier(source, name)?,
        channel,
    })
}

fn parse_cell(source: &str, text: &str, open: usize) -> Result<Field, FieldError> {
    let head = &text[..open];
    let rest = &text[open + 1..];
    let close = closing_bracket(rest)
        .ok_or_else(|| malformed(source, "the index opened with '[' is never closed"))?;
    let inside = &rest[..close];
    let tail = &rest[close + 1..];

    let parts: Vec<&str> = head.split('.').collect();
    if parts.len() == 1 {
        if !tail.is_empty() {
            return Err(malformed(
                source,
                "a list item is a scalar and takes no channel",
            ));
        }
        let RowAddress::Ordinal(position) = parse_index_list(source, inside)? else {
            return Err(malformed(
                source,
                "a list item is addressed by position, as in temperatures[#0]",
            ));
        };
        return Ok(Field::ListItem {
            name: identifier(source, parts[0])?,
            position,
        });
    }
    if parts.len() != 2 {
        return Err(malformed(
            source,
            "a cell is table.column[index]: two names before the bracket",
        ));
    }
    let table = identifier(source, parts[0])?;
    let column = identifier(source, parts[1])?;
    let row = parse_index_list(source, inside)?;

    let channel = if tail.is_empty() {
        Channel::Value
    } else {
        let suffix = tail
            .strip_prefix('.')
            .ok_or_else(|| malformed(source, "a channel follows the index after a '.'"))?;
        let segments: Vec<&str> = suffix.split('.').collect();
        channel_of(source, &segments, &format!("{table}.{column}"))?
    };
    Ok(Field::Cell {
        table,
        column,
        row,
        channel,
    })
}

/// The matching `]`, skipping any inside a quoted index.
fn closing_bracket(rest: &str) -> Option<usize> {
    let mut quoted = false;
    for (at, character) in rest.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ']' if !quoted => return Some(at),
            _ => {}
        }
    }
    None
}

fn parse_index_list(source: &str, inside: &str) -> Result<RowAddress, FieldError> {
    let trimmed = inside.trim();
    if trimmed.is_empty() {
        return Err(malformed(source, "an index is empty"));
    }
    // An ordinal addresses the whole row whatever the table's arity, so it is
    // never one component among several.
    if let Some(digits) = trimmed.strip_prefix('#') {
        let wrong = || {
            malformed(
                source,
                "an ordinal is '#' followed by a position, as in [#3], or counted from the \
                 last row, as in [#-1]",
            )
        };
        // From the end: `#-1` the last row, `#-2` the one before.
        if let Some(back) = digits.trim().strip_prefix('-') {
            let back: usize = back.trim().parse().map_err(|_| wrong())?;
            if back == 0 {
                return Err(wrong());
            }
            return Ok(RowAddress::FromEnd(back));
        }
        let position: usize = digits.trim().parse().map_err(|_| wrong())?;
        return Ok(RowAddress::Ordinal(position));
    }
    let mut values = Vec::new();
    for component in split_indexes(trimmed) {
        values.push(index_value(source, component.trim())?);
    }
    Ok(RowAddress::Index(values))
}

/// Split on commas that are not inside quotes.
fn split_indexes(inside: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted) = (0usize, false);
    for (at, character) in inside.char_indices() {
        match character {
            '"' => quoted = !quoted,
            ',' if !quoted => {
                parts.push(&inside[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&inside[start..]);
    parts
}

/// An index component is a number, a date, or text — quoted when it holds a
/// space. The same shapes the format's own scalars have, read the same way.
fn index_value(source: &str, component: &str) -> Result<Value, FieldError> {
    if let Some(quoted) = component.strip_prefix('"') {
        let text = quoted
            .strip_suffix('"')
            .ok_or_else(|| malformed(source, "a quoted index is never closed"))?;
        return Ok(Value::text(text));
    }
    if component.contains('#') {
        return Err(malformed(
            source,
            "an ordinal addresses a whole row and cannot sit beside another index",
        ));
    }
    if let Ok(integer) = component.parse::<i64>() {
        return Ok(Value::integer(integer));
    }
    if let Ok(number) = component.parse::<f64>() {
        return Value::number(number).map_err(|error| malformed(source, &error.to_string()));
    }
    if let Ok(date) = Date::parse(component) {
        return Ok(Value::date(date));
    }
    if let Ok(date_time) = DateTime::parse(component) {
        return Ok(Value::date_time(date_time));
    }
    match component {
        "true" => Ok(Value::boolean(true)),
        "false" => Ok(Value::boolean(false)),
        _ => Ok(Value::text(component)),
    }
}

// ------------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq)]
pub enum FieldError {
    Malformed {
        source: String,
        reason: String,
    },
    MissingIndex {
        table: Identifier,
        column: Identifier,
    },
    TableNeedsCell {
        table: Identifier,
        columns: Vec<String>,
    },
    UnknownProperty {
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    UnknownTable {
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    UnknownColumn {
        table: Identifier,
        name: String,
        available: Vec<String>,
        suggestion: Option<String>,
    },
    UnknownIndex {
        table: Identifier,
        index: Vec<Value>,
        per_column: Vec<IndexReport>,
    },
    ListIndexOutOfRange {
        name: Identifier,
        position: usize,
        length: usize,
    },
    UnknownChannel {
        name: String,
        on: String,
        available: Vec<String>,
    },
    /// A formula failed while the field was being read. Not an absence: a
    /// broken model must not look like a collection of unmeasured samples.
    Compute {
        field: String,
        reason: String,
    },
    /// `state` asked of a subject whose caller did not read the states: said,
    /// because an empty set would answer *nothing failed* for nobody having
    /// looked.
    StatesNotRead,
}

fn malformed(source: &str, reason: &str) -> FieldError {
    FieldError::Malformed {
        source: source.to_string(),
        reason: reason.to_string(),
    }
}

fn identifier(source: &str, text: &str) -> Result<Identifier, FieldError> {
    Identifier::new(text).map_err(|error: IdentifierError| malformed(source, &error.to_string()))
}

fn reserved_of(name: &str) -> Option<ReservedField> {
    match name {
        "name" => Some(ReservedField::Name),
        "path" => Some(ReservedField::Path),
        "filename" => Some(ReservedField::Filename),
        "project" => Some(ReservedField::Project),
        "state" => Some(ReservedField::State),
        _ => None,
    }
}

/// Spellings a channel used to have. They are refused, and they are refused
/// **as channels**: read as a table's column instead, `malt.value` answers *a
/// column has one cell per row, address one* — which is nonsense to someone
/// typing what the documentation said last week.
const RETIRED_CHANNELS: [&str; 2] = ["value", "uncertainty"];

/// What a retired spelling is called now.
fn retired_as(name: &str) -> Option<&'static str> {
    match name {
        "value" => Some("v"),
        "uncertainty" => Some("u"),
        _ => None,
    }
}

/// The channels a path may end with, for a message that lists them.
///
/// One spelling each: a message that offered `value` while the parser refused
/// it answered *did you mean: uncertainty?* to `uncertainty`, which is worse
/// than saying nothing.
fn channel_names() -> Vec<String> {
    ["v", "u", "unit", "symbol", "readings", "stats.<statistic>"]
        .iter()
        .map(|name| name.to_string())
        .collect()
}

fn completion_channel_names(has_readings: bool) -> Vec<String> {
    ["v", "u", "unit", "symbol"]
        .iter()
        .map(|name| name.to_string())
        .chain(has_readings.then(|| "readings".to_string()))
        .chain(has_readings.then(|| "stats.".to_string()))
        .collect()
}

fn statistic_names() -> Vec<String> {
    [
        "count",
        "minimum",
        "maximum",
        "mean",
        "median",
        "first_quartile",
        "third_quartile",
        "sample_stdev",
        "population_stdev",
        "standard_error",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect()
}

/// `v` and `u` are input forms accepted everywhere, and `unit` has no
/// single-letter alias: `u` is the first letter of both, and it goes to
/// uncertainty, following the GUM's `u(x)`.
fn channel_of(source: &str, segments: &[&str], on: &str) -> Result<Channel, FieldError> {
    match segments {
        [] => Ok(Channel::Value),
        ["v"] => Ok(Channel::Value),
        ["u"] => Ok(Channel::Uncertainty),
        ["unit"] => Ok(Channel::Unit),
        ["symbol"] => Ok(Channel::Symbol),
        ["readings"] => Ok(Channel::Readings),
        ["stats"] => Err(FieldError::UnknownChannel {
            name: "stats".to_string(),
            on: on.to_string(),
            available: statistic_names(),
        }),
        ["stats", statistic] => {
            statistic_of(statistic)
                .map(Channel::Stat)
                .ok_or_else(|| FieldError::UnknownChannel {
                    name: format!("stats.{statistic}"),
                    on: on.to_string(),
                    available: statistic_names(),
                })
        }
        [one] => Err(FieldError::UnknownChannel {
            name: (*one).to_string(),
            on: on.to_string(),
            available: channel_names(),
        }),
        _ => Err(malformed(source, "a path has one channel, not several")),
    }
}

fn statistic_of(name: &str) -> Option<Statistic> {
    Some(match name {
        "count" => Statistic::Count,
        "minimum" => Statistic::Minimum,
        "maximum" => Statistic::Maximum,
        "mean" => Statistic::Mean,
        "median" => Statistic::Median,
        "first_quartile" => Statistic::FirstQuartile,
        "third_quartile" => Statistic::ThirdQuartile,
        "sample_stdev" => Statistic::SampleStdev,
        "population_stdev" => Statistic::PopulationStdev,
        "standard_error" => Statistic::StandardError,
        _ => return None,
    })
}

// -------------------------------------------------------------- suggestions

/// The nearest candidate, through `identifier`'s one definition of it.
fn nearest(written: &str, candidates: &[String]) -> Option<String> {
    crate::core::identifier::nearest(written, candidates.iter().map(String::as_str))
}

// ---------------------------------------------------------------- vocabulary

/// Every name any sample in the collection has, so that a name present in none
/// of them can be told from one merely absent here.
pub fn vocabulary_of(collection: &[&Sample]) -> Vocabulary {
    let mut names: Vec<Identifier> = Vec::new();
    let mut tables: Vec<(Identifier, Vec<Identifier>)> = Vec::new();
    let mut tags: Vec<Identifier> = Vec::new();
    for sample in collection {
        for tag in sample.tags() {
            push_once(&mut tags, tag);
        }
        for name in sample.property_names() {
            push_once(&mut names, name);
        }
        for name in sample.attribute_names() {
            push_once(&mut names, name);
        }
        for table_name in sample.table_names() {
            let columns = match sample.table(table_name) {
                Ok(table) => table
                    .column_names()
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            };
            match tables.iter_mut().find(|(name, _)| name == table_name) {
                Some((_, known)) => {
                    for column in columns {
                        if !known.contains(&column) {
                            known.push(column);
                        }
                    }
                }
                None => tables.push((table_name.clone(), columns)),
            }
        }
    }
    // `tags` is addressable on every sample, which is what `available` says
    // when it lists the field unconditionally. Leaving it out of the names
    // made a column a caller *can* ask for look like a name nobody has — and
    // produced "unknown field 'tags'; did you mean 'tags'?".
    push_once(
        &mut names,
        &Identifier::new("tags").expect("a reserved name is a name"),
    );
    Vocabulary {
        names,
        tables,
        tags,
    }
}

fn push_once(names: &mut Vec<Identifier>, name: &Identifier) {
    if !names.contains(name) {
        names.push(name.clone());
    }
}

impl Vocabulary {
    /// The same, knowing `names` too: what a model declares before any sample
    /// holds it, so that a profile or a sort naming one reads it as absent
    /// rather than as a misspelling.
    pub fn with_names(mut self, names: impl IntoIterator<Item = Identifier>) -> Vocabulary {
        for name in names {
            push_once(&mut self.names, &name);
        }
        self
    }

    /// A vocabulary of one sample, for a caller that has no collection.
    pub fn of(sample: &Sample) -> Vocabulary {
        vocabulary_of(&[sample])
    }

    pub fn has_name(&self, name: &Identifier) -> bool {
        self.names.contains(name)
    }

    pub fn has_tag(&self, tag: &Identifier) -> bool {
        self.tags.contains(tag)
    }

    /// The tags in use, in the order the collection introduced them.
    pub fn tags(&self) -> &[Identifier] {
        &self.tags
    }

    /// The nearest known tag, for a caller reporting a typo.
    pub fn nearest_tag(&self, written: &str) -> Option<String> {
        let known: Vec<String> = self.tags.iter().map(Identifier::to_string).collect();
        nearest(written, &known)
    }

    pub fn has_table(&self, name: &Identifier) -> bool {
        self.tables.iter().any(|(table, _)| table == name)
    }

    pub fn has_column(&self, table: &Identifier, column: &Identifier) -> bool {
        self.tables
            .iter()
            .any(|(name, columns)| name == table && columns.contains(column))
    }

    fn names_as_strings(&self) -> Vec<String> {
        self.names.iter().map(Identifier::to_string).collect()
    }

    pub fn tables_as_strings(&self) -> Vec<String> {
        self.tables
            .iter()
            .map(|(name, _)| name.to_string())
            .collect()
    }

    pub fn columns_as_strings(&self, table: &Identifier) -> Vec<String> {
        self.tables
            .iter()
            .find(|(name, _)| name == table)
            .map(|(_, columns)| columns.iter().map(Identifier::to_string).collect())
            .unwrap_or_default()
    }
}

// --------------------------------------------------------------- resolution

/// A field against one subject. `Ok(Scalar(None))` is *absent from this
/// sample*; an error is *the collection has no such name*, which is a mistake
/// rather than data. The previous implementation answered both with a dash.
pub fn resolve(field: &Field, subject: &Subject) -> Result<Resolution, FieldError> {
    match field {
        Field::Reserved(ReservedField::State) => {
            let states = subject.states.ok_or(FieldError::StatesNotRead)?;
            // Nothing where the model was not read and the files say
            // nothing: not `current`, which nobody could say.
            Ok(if states.held().is_empty() {
                Resolution::Scalar(None)
            } else {
                Resolution::List(
                    states
                        .held()
                        .iter()
                        .map(|state| Value::text(state.word()))
                        .collect(),
                )
            })
        }
        Field::Reserved(reserved) => Ok(Resolution::Scalar(reserved_value(reserved, subject))),
        Field::Named { name, channel } => resolve_named(name, *channel, subject),
        Field::ListItem { name, position } => resolve_list_item(name, *position, subject),
        Field::Cell {
            table,
            column,
            row,
            channel,
        } => resolve_cell(table, column, row, *channel, subject),
    }
}

fn resolve_list_item(
    name: &Identifier,
    position: usize,
    subject: &Subject,
) -> Result<Resolution, FieldError> {
    if let Ok(value) = subject.sample.attribute(name) {
        return match value {
            // A position this sample's list does not reach is absent here; only
            // one that no sample reaches is a mistake (`held_nowhere`).
            AttributeValue::List(values) => Ok(Resolution::Scalar(values.get(position).cloned())),
            AttributeValue::Scalar(_) => Err(malformed(
                &format!("{name}[#{position}]"),
                "the named attribute is one scalar, not a list",
            )),
        };
    }
    if subject.vocabulary.has_name(name) {
        return Ok(Resolution::Scalar(None));
    }
    let available = subject.vocabulary.names_as_strings();
    Err(FieldError::UnknownProperty {
        name: name.to_string(),
        suggestion: nearest(name.as_str(), &available),
        available,
    })
}

/// A table row, or a list position, that no sample of a collection holds: the
/// collection's question, asked by whoever checks a filter or a profile, since
/// one sample lacking it is data. `None` when a sample holds it, or when no
/// sample has the table or the list at all.
pub fn held_nowhere(field: &Field, samples: &[&Sample]) -> Option<FieldError> {
    match field {
        Field::Cell { table, row, .. } => {
            let mut missed = None;
            for sample in samples {
                let Ok(held) = sample.table(table) else {
                    continue;
                };
                match held.row(row) {
                    Ok(_) => return None,
                    Err(error) => {
                        if missed.is_none() {
                            missed = Some(index_error(table, row, error));
                        }
                    }
                }
            }
            missed
        }
        Field::ListItem { name, position } => {
            let mut longest: Option<usize> = None;
            for sample in samples {
                if let Ok(AttributeValue::List(values)) = sample.attribute(name) {
                    if *position < values.len() {
                        return None;
                    }
                    longest = Some(longest.map_or(values.len(), |length| length.max(values.len())));
                }
            }
            longest.map(|length| FieldError::ListIndexOutOfRange {
                name: name.clone(),
                position: *position,
                length,
            })
        }
        _ => None,
    }
}

fn reserved_value(reserved: &ReservedField, subject: &Subject) -> Option<Value> {
    match reserved {
        ReservedField::Name => subject.sample.name().map(Value::text),
        // A sample that was never written has no path, which is absence rather
        // than an invented empty string.
        ReservedField::Path => subject
            .path
            .map(|path| Value::text(path.to_string_lossy().into_owned())),
        ReservedField::Filename => subject
            .path
            .and_then(|path| path.file_name())
            .map(|name| Value::text(name.to_string_lossy().into_owned())),
        ReservedField::Project => subject.path.and_then(project_of).map(Value::text),
        // Resolved as a list before this is reached.
        ReservedField::State => None,
    }
}

/// The name of the folder holding the `.samplekitrc` a file belongs to, looked
/// up once per folder: a sort compares it many times.
fn project_of(path: &Path) -> Option<String> {
    thread_local! {
        static KNOWN: std::cell::RefCell<std::collections::HashMap<PathBuf, Option<String>>> =
            std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let folder = path.parent()?.to_path_buf();
    if let Some(known) = KNOWN.with(|known| known.borrow().get(&folder).cloned()) {
        return known;
    }
    let found = crate::config::project_config::find(path)
        .and_then(|rc| dunce::canonicalize(&rc).ok())
        .and_then(|rc| {
            rc.parent()?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
    KNOWN.with(|known| known.borrow_mut().insert(folder, found.clone()));
    found
}

fn resolve_named(
    name: &Identifier,
    channel: Channel,
    subject: &Subject,
) -> Result<Resolution, FieldError> {
    if name.as_str() == "tags" {
        return match channel {
            Channel::Value => Ok(Resolution::Tags(subject.sample.tags().to_vec())),
            other => Err(attribute_has_no_channel(name, other)),
        };
    }
    if let Ok(value) = subject.sample.attribute(name) {
        return match channel {
            Channel::Value => Ok(match value {
                AttributeValue::Scalar(Value::Absent) => Resolution::Scalar(None),
                AttributeValue::Scalar(value) => Resolution::Scalar(Some(value.clone())),
                AttributeValue::List(values) => Resolution::List(values.clone()),
            }),
            other => Err(attribute_has_no_channel(name, other)),
        };
    }
    if let Ok(handle) = subject.sample.property(name) {
        return handle.with(|property| channel_of_property(property, channel, name.as_str()));
    }
    // Absent here, or absent everywhere: only the collection can say which.
    if subject.vocabulary.has_name(name) {
        return Ok(Resolution::Scalar(None));
    }
    if subject.vocabulary.has_table(name) {
        return Err(FieldError::TableNeedsCell {
            table: name.clone(),
            columns: subject.vocabulary.columns_as_strings(name),
        });
    }
    let available = subject.vocabulary.names_as_strings();
    Err(FieldError::UnknownProperty {
        name: name.to_string(),
        suggestion: nearest(name.as_str(), &available),
        available,
    })
}

fn attribute_has_no_channel(name: &Identifier, channel: Channel) -> FieldError {
    FieldError::UnknownChannel {
        name: describe_channel(channel),
        on: format!("{name}, which is an attribute and has no channels"),
        available: Vec::new(),
    }
}

fn channel_of_property(
    property: &crate::core::property::Property,
    channel: Channel,
    named: &str,
) -> Result<Resolution, FieldError> {
    // A formula that fails is an error, never an absence: discarding it makes a
    // broken model look like a collection of unmeasured samples.
    let failed = |error: crate::core::property::ComputeError| FieldError::Compute {
        field: named.to_string(),
        reason: error.to_string(),
    };
    // The one channel that answers a list rather than a scalar: the readings
    // are the evidence, and showing their mean where they were asked for would
    // be the tool answering a question nobody put to it.
    if channel == Channel::Readings {
        let values = property.readings().map_or_else(Vec::new, |readings| {
            readings
                .as_slice()
                .iter()
                .filter_map(|number| Value::number(*number).ok())
                .collect()
        });
        return Ok(Resolution::List(values));
    }
    // Where reads do not compute, a failure is an earlier run's, recorded and
    // said by `status`: read as the file would hold it — no value — as the
    // command line reads it, rather than failing every sort and filter over the
    // list the sample is in.
    let recorded = |error: &crate::core::property::ComputeError| {
        matches!(error, crate::core::property::ComputeError::Failed { .. })
            && !crate::core::property::reads_compute()
    };
    let scalar = match channel {
        // `Absent` is a stored value meaning *not recorded*, so it answers as
        // absence rather than as a value that happens to be nothing.
        Channel::Value => match property.value() {
            // Not applicable answers as itself, which a table says `n/a` and
            // every use of a value reads as none.
            Ok(value) => Some(value).filter(|v| !matches!(v, Value::Absent)),
            Err(error) if recorded(&error) => None,
            Err(error) => return Err(failed(error)),
        },
        Channel::Uncertainty => match property.uncertainty() {
            Ok(uncertainty) => uncertainty.and_then(|u| Value::number(u.magnitude()).ok()),
            Err(error) if recorded(&error) => None,
            Err(error) => return Err(failed(error)),
        },
        Channel::Unit => property.presentation().unit.clone().map(Value::text),
        Channel::Symbol => property.presentation().symbol.clone().map(Value::text),
        // Answered as a list above, before this match is reached.
        Channel::Readings => unreachable!("readings are resolved as a list"),
        // Nothing to summarize is absence, distinct from a misspelled channel.
        Channel::Stat(statistic) => property
            .readings()
            .map(crate::core::statistics::summarize)
            .and_then(|summary| statistic_value(&summary, statistic)),
    };
    Ok(Resolution::Scalar(scalar))
}

fn statistic_value(summary: &Summary, statistic: Statistic) -> Option<Value> {
    let number = match statistic {
        Statistic::Count => return Some(Value::integer(summary.count.get() as i64)),
        Statistic::Minimum => summary.minimum,
        Statistic::Maximum => summary.maximum,
        Statistic::Mean => summary.mean,
        Statistic::Median => summary.median,
        Statistic::FirstQuartile => summary.first_quartile,
        Statistic::ThirdQuartile => summary.third_quartile,
        // `None` for a single reading, which `statistics` refuses to call zero
        // spread. Absent, not an error.
        Statistic::SampleStdev => summary.sample_stdev?,
        Statistic::PopulationStdev => summary.population_stdev,
        Statistic::StandardError => summary.standard_error?,
    };
    Value::number(number).ok()
}

fn resolve_cell(
    table_name: &Identifier,
    column: &Identifier,
    row: &RowAddress,
    channel: Channel,
    subject: &Subject,
) -> Result<Resolution, FieldError> {
    let Ok(table) = subject.sample.table(table_name) else {
        if subject.vocabulary.has_table(table_name) {
            return Ok(Resolution::Scalar(None));
        }
        let available = subject.vocabulary.tables_as_strings();
        return Err(FieldError::UnknownTable {
            name: table_name.to_string(),
            suggestion: nearest(table_name.as_str(), &available),
            available,
        });
    };
    if !table.column_names().contains(&column) {
        if subject.vocabulary.has_column(table_name, column) {
            return Ok(Resolution::Scalar(None));
        }
        let available = subject.vocabulary.columns_as_strings(table_name);
        return Err(FieldError::UnknownColumn {
            table: table_name.clone(),
            name: column.to_string(),
            suggestion: nearest(column.as_str(), &available),
            available,
        });
    }
    // The row is this sample's own question: another sample having that index
    // says nothing about this one, so there is no vocabulary to consult.
    // A row this sample lacks is absent here: another sample may hold it, and
    // only a row no sample holds is a mistake (`held_nowhere`).
    let view = match table.row(row) {
        Ok(view) => view,
        // A position past this sample's rows is as absent here as an index
        // it lacks: `[#2]` of a table of one row names nothing in it.
        Err(
            crate::core::table::TableError::UnknownIndex { .. }
            | crate::core::table::TableError::OrdinalOutOfRange { .. }
            | crate::core::table::TableError::FromEndOutOfRange { .. },
        ) => {
            return Ok(Resolution::Scalar(None));
        }
        Err(error) => return Err(index_error(table_name, row, error)),
    };
    // A cell's unit and symbol are its column's unless it says otherwise: the
    // composition `presentation_of` owns, which a cell alone does not know.
    // Read from the cell, `measurements.f[20].unit` was empty where the
    // table's header said brix.
    if matches!(channel, Channel::Unit | Channel::Symbol) {
        let composed = table
            .presentation_of(column, row)
            .map_err(|error| malformed(&format!("{table_name}.{column}"), &error.to_string()))?;
        let text = match channel {
            Channel::Unit => composed.unit,
            _ => composed.symbol,
        };
        return Ok(Resolution::Scalar(text.map(Value::text)));
    }
    let cell = view
        .cell(column)
        .map_err(|error| malformed(&format!("{table_name}.{column}"), &error.to_string()))?;
    channel_of_property(cell, channel, &format!("{table_name}.{column}"))
}

fn index_error(
    table: &Identifier,
    row: &RowAddress,
    error: crate::core::table::TableError,
) -> FieldError {
    match error {
        crate::core::table::TableError::UnknownIndex { index, per_column } => {
            FieldError::UnknownIndex {
                table: table.clone(),
                index,
                per_column,
            }
        }
        other => FieldError::Malformed {
            source: describe_row(row),
            reason: other.to_string(),
        },
    }
}

// ------------------------------------------------------------- descriptions

/// A field, written the way it is typed. Round-trips through `parse`, except
/// for an ordinal, which is refused where paths are written.
pub fn describe(field: &Field) -> String {
    match field {
        Field::Reserved(reserved) => match reserved {
            ReservedField::Name => "name".to_string(),
            ReservedField::Path => "path".to_string(),
            ReservedField::Filename => "filename".to_string(),
            ReservedField::Project => "project".to_string(),
            ReservedField::State => "state".to_string(),
        },
        Field::Named { name, channel } => match channel {
            Channel::Value => name.to_string(),
            other => format!("{name}.{}", describe_channel(*other)),
        },
        Field::ListItem { name, position } => format!("{name}[#{position}]"),
        Field::Cell {
            table,
            column,
            row,
            channel,
        } => {
            let head = format!("{table}.{column}[{}]", describe_row_inner(row));
            match channel {
                Channel::Value => head,
                other => format!("{head}.{}", describe_channel(*other)),
            }
        }
    }
}

fn describe_channel(channel: Channel) -> String {
    match channel {
        // As a path reads them back.
        Channel::Value => "v".to_string(),
        Channel::Uncertainty => "u".to_string(),
        Channel::Unit => "unit".to_string(),
        Channel::Symbol => "symbol".to_string(),
        Channel::Readings => "readings".to_string(),
        Channel::Stat(statistic) => format!("stats.{}", describe_statistic(statistic)),
    }
}

fn describe_statistic(statistic: Statistic) -> &'static str {
    match statistic {
        Statistic::Count => "count",
        Statistic::Minimum => "minimum",
        Statistic::Maximum => "maximum",
        Statistic::Mean => "mean",
        Statistic::Median => "median",
        Statistic::FirstQuartile => "first_quartile",
        Statistic::ThirdQuartile => "third_quartile",
        Statistic::SampleStdev => "sample_stdev",
        Statistic::PopulationStdev => "population_stdev",
        Statistic::StandardError => "standard_error",
    }
}

fn describe_row(row: &RowAddress) -> String {
    format!("[{}]", describe_row_inner(row))
}

fn describe_row_inner(row: &RowAddress) -> String {
    match row {
        RowAddress::Ordinal(position) => format!("#{position}"),
        RowAddress::FromEnd(back) => format!("#-{back}"),
        RowAddress::Index(values) => values
            .iter()
            .map(describe_index_value)
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// An index component, written so that what is printed can be typed back.
/// Text is quoted when it holds a space — the path's own rule, which is not
/// the file's: a path has no YAML to be ambiguous against.
fn describe_index_value(value: &Value) -> String {
    match value {
        Value::Text(text) if text.contains(' ') => format!("\"{text}\""),
        Value::Text(text) => text.clone(),
        Value::Integer(integer) => integer.to_string(),
        Value::Number(number) => number.to_string(),
        Value::Boolean(boolean) => boolean.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        Value::Absent => String::new(),
        Value::NotApplicable => "n/a".to_string(),
    }
}

// -------------------------------------------------------------- enumeration

/// Every addressable field of one sample, in declaration order: properties,
/// then attributes, then each table's cells. One entry per quantity — channels
/// are a suffix that applies uniformly, and expanding them multiplies the list
/// by five while teaching nothing.
pub fn available(sample: &Sample) -> Vec<Field> {
    let mut fields = vec![
        Field::Reserved(ReservedField::Name),
        Field::Reserved(ReservedField::Path),
        Field::Reserved(ReservedField::Filename),
        Field::Reserved(ReservedField::Project),
        Field::Reserved(ReservedField::State),
    ];
    for name in sample.property_names() {
        fields.push(Field::Named {
            name: name.clone(),
            channel: Channel::Value,
        });
    }
    for name in sample.attribute_names() {
        fields.push(Field::Named {
            name: name.clone(),
            channel: Channel::Value,
        });
        if let Ok(AttributeValue::List(values)) = sample.attribute(name) {
            fields.extend((0..values.len()).map(|position| Field::ListItem {
                name: name.clone(),
                position,
            }));
        }
    }
    // `tags` is an attribute like the others here, listed once. The tags a
    // collection actually uses are values, not fields, and are enumerated
    // elsewhere.
    fields.push(Field::Named {
        name: Identifier::new("tags").expect("a reserved name is a name"),
        channel: Channel::Value,
    });
    for table_name in sample.table_names() {
        let Ok(table) = sample.table(table_name) else {
            continue;
        };
        let columns: Vec<Identifier> = table.column_names().into_iter().cloned().collect();
        for index in table.index_tuples() {
            let row: Vec<Value> = index.into_iter().cloned().collect();
            for column in &columns {
                fields.push(Field::Cell {
                    table: table_name.clone(),
                    column: column.clone(),
                    row: RowAddress::Index(row.clone()),
                    channel: Channel::Value,
                });
            }
        }
    }
    fields
}

/// The union across a collection, so that a picker offers what any sample has.
/// Order follows the first sample that introduced each field.
pub fn available_in(collection: &[&Sample]) -> Vec<Field> {
    // Seen by their written form: a table of twenty thousand rows names forty
    // thousand cells, and a linear search for each made listing them quadratic.
    let mut seen = std::collections::HashSet::new();
    let mut fields: Vec<Field> = Vec::new();
    for sample in collection {
        for field in available(sample) {
            if seen.insert(describe(&field)) {
                fields.push(field);
            }
        }
    }
    fields
}

// ---------------------------------------------------------------- messages

impl fmt::Display for FieldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldError::Malformed { source, reason } => {
                write!(f, "'{source}' is not a field path: {reason}")
            }
            // Two names read as a column without its row — or as a quantity
            // and a channel it does not have: parsing cannot tell which, and
            // `og.x` beside a quantity `og` was told it named a column.
            FieldError::MissingIndex { table, column } => write!(
                f,
                "'{table}.{column}' names a column without its row — {table}.{column}[<index>], \
                 or {table}.{column}[#0] by position — or, if {table} is a quantity, {column} \
                 is no channel of it: v, u, unit, symbol, readings, stats.*"
            ),
            FieldError::TableNeedsCell { table, columns } => {
                write!(
                    f,
                    "'{table}' names a table, not one scalar field. Address one cell as {table}.<column>[<index>]"
                )?;
                if !columns.is_empty() {
                    write!(f, "\n  columns: {}", columns.join(", "))?;
                }
                Ok(())
            }
            FieldError::UnknownProperty {
                name,
                available,
                suggestion,
            } => write_unknown(f, &format!("unknown field '{name}'"), available, suggestion),
            FieldError::UnknownTable {
                name,
                available,
                suggestion,
            } => write_unknown(f, &format!("unknown table '{name}'"), available, suggestion),
            FieldError::UnknownColumn {
                table,
                name,
                available,
                suggestion,
            } => write_unknown(
                f,
                &format!("unknown column '{name}' in table '{table}'"),
                available,
                suggestion,
            ),
            FieldError::UnknownIndex {
                table,
                index,
                per_column,
            } => {
                let written: Vec<String> = index.iter().map(describe_index_value).collect();
                writeln!(f, "no row of '{table}' is indexed [{}]", written.join(", "))?;
                // Per column, because a two-column index that missed says which
                // half missed, and that is the fix.
                // *Matched* only where it did; a text is offered by its
                // spelling, as a question.
                for (report, asked) in per_column.iter().zip(index) {
                    match &report.nearest {
                        Some(nearest @ Value::Text(_)) => writeln!(
                            f,
                            "  {}: did you mean {}?",
                            report.column,
                            describe_index_value(nearest)
                        )?,
                        Some(nearest) => writeln!(
                            f,
                            "  {}: nearest is {}",
                            report.column,
                            describe_index_value(nearest)
                        )?,
                        None if report.available.contains(asked) => {
                            writeln!(f, "  {}: matched", report.column)?
                        }
                        None => {}
                    }
                }
                Ok(())
            }
            FieldError::ListIndexOutOfRange {
                name,
                position,
                length,
            } => write!(
                f,
                "no sample has item #{position} in '{name}': items are numbered from #0, and the \
                 longest list has {}",
                if *length == 1 {
                    "1 item".to_string()
                } else {
                    format!("{length} items")
                }
            ),
            FieldError::Compute { field, reason } => {
                write!(f, "reading '{field}' ran a formula that failed: {reason}")
            }
            FieldError::StatesNotRead => write!(
                f,
                "'state' was asked where the samples' states were not read: a filter reads \
                 them, and this is not one"
            ),
            FieldError::UnknownChannel {
                name,
                on,
                available,
            } => {
                write!(f, "'{name}' is not a channel of {on}")?;
                // A spelling that *was* one gets told so outright: the edit
                // distance from `uncertainty` to `u` is too far for a
                // suggestion to find, and it is the one answer that is certain.
                if let Some(now) = retired_as(name) {
                    write!(
                        f,
                        "\n  '{name}' was renamed '{now}': one spelling for one channel"
                    )?;
                    return Ok(());
                }
                if !available.is_empty() {
                    write!(f, "\n  available: {}", available.join(", "))?;
                    if let Some(suggestion) = nearest(name, available) {
                        write!(f, "\n  did you mean: {suggestion}?")?;
                    }
                }
                Ok(())
            }
        }
    }
}

fn write_unknown(
    f: &mut fmt::Formatter<'_>,
    headline: &str,
    available: &[String],
    suggestion: &Option<String>,
) -> fmt::Result {
    write!(f, "{headline}")?;
    if let Some(suggestion) = suggestion {
        write!(f, "\n  did you mean: '{suggestion}'?")?;
    }
    if !available.is_empty() {
        write!(f, "\n  available: {}", available.join(", "))?;
    }
    Ok(())
}

impl std::error::Error for FieldError {}

// ---------------------------------------------------------------- completion

/// What could come next after what has been typed. One segment at a time: a
/// table with eleven indexes and four columns is forty-four cells and four
/// segments of typing, and offering all forty-four at the first keystroke is
/// the same drowning as expanding channels, one level up.
pub fn complete(prefix: &str, subject: &Subject) -> Vec<String> {
    // Inside brackets: the indexes of that column's table.
    if let Some(open) = prefix.rfind('[')
        && closing_bracket(&prefix[open + 1..]).is_none()
    {
        return complete_index(&prefix[..open], &prefix[open + 1..], subject);
    }
    if quantity_exists(prefix, subject) {
        return completion_channel_names(quantity_has_readings(prefix, subject))
            .into_iter()
            .map(|candidate| format!("{prefix}.{candidate}"))
            .collect();
    }
    match prefix.rsplit_once('.') {
        // A first segment: quantities, attributes and tables, never channels.
        None => {
            let mut matches = starting_with(prefix, top_level(subject));
            if !prefix.is_empty() {
                let lowered = prefix.to_lowercase();
                for field in available(subject.sample) {
                    if let Field::Cell { column, .. } = &field
                        && column.as_str().to_lowercase().starts_with(&lowered)
                    {
                        let candidate = describe(&field);
                        if !matches.contains(&candidate) {
                            matches.push(candidate);
                        }
                    }
                }
            }
            matches
        }
        Some((head, tail)) if head.ends_with(".stats") => {
            let quantity = head.trim_end_matches(".stats");
            if !quantity_has_readings(quantity, subject) {
                return Vec::new();
            }
            starting_with(tail, statistic_names())
                .into_iter()
                .map(|candidate| format!("{head}.{candidate}"))
                .collect()
        }
        Some((head, tail)) => {
            let candidates = if head.contains('[') {
                completion_channel_names(quantity_has_readings(head, subject))
            } else if let Ok(table_name) = Identifier::new(head) {
                if subject.vocabulary.has_table(&table_name) {
                    subject.vocabulary.columns_as_strings(&table_name)
                } else if subject.sample.has_property(&table_name) {
                    completion_channel_names(quantity_has_readings(head, subject))
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            let is_table = Identifier::new(head)
                .is_ok_and(|table_name| subject.vocabulary.has_table(&table_name));
            starting_with(tail, candidates)
                .into_iter()
                .map(|candidate| {
                    if is_table {
                        format!("{head}.{candidate}[")
                    } else {
                        format!("{head}.{candidate}")
                    }
                })
                .collect()
        }
    }
}

fn quantity_exists(source: &str, subject: &Subject) -> bool {
    if has_explicit_channel(source) {
        return false;
    }
    let Ok(field) = parse(source) else {
        return false;
    };
    match field {
        Field::Named { name, .. } => subject.sample.has_property(&name),
        Field::Cell {
            table, column, row, ..
        } => subject
            .sample
            .table(&table)
            .is_ok_and(|table| table.at(&row, &column).is_ok()),
        Field::ListItem { .. } => false,
        Field::Reserved(_) => false,
    }
}

fn quantity_has_readings(source: &str, subject: &Subject) -> bool {
    let Ok(field) = parse(source) else {
        return false;
    };
    match field {
        Field::Named { name, .. } => subject
            .sample
            .property(&name)
            .is_ok_and(|property| property.with(|property| property.readings().is_some())),
        Field::Cell {
            table, column, row, ..
        } => subject
            .sample
            .table(&table)
            .ok()
            .and_then(|table| table.at(&row, &column).ok())
            .is_some_and(|property| property.readings().is_some()),
        Field::ListItem { .. } => false,
        Field::Reserved(_) => false,
    }
}

/// Whether the spelling asks for one channel rather than relying on the value
/// default. Parsed equality deliberately forgets this distinction; a renderer
/// or exporter still needs it to tell a whole quantity from `malt.value`.
pub fn has_explicit_channel(source: &str) -> bool {
    let text = source.trim();
    let Ok(field) = parse(text) else {
        return false;
    };
    match field {
        Field::Named { .. } => text.contains('.'),
        Field::Cell { .. } => text
            .rfind(']')
            .is_some_and(|close| text[close + 1..].starts_with('.')),
        Field::ListItem { .. } => false,
        Field::Reserved(_) => false,
    }
}

fn top_level(subject: &Subject) -> Vec<String> {
    available(subject.sample)
        .iter()
        .map(|field| match field {
            Field::Named { name, .. } => name.to_string(),
            Field::Reserved(_) => describe(field),
            // A table is offered by its own name; its cells come one segment
            // later, when the reader has said which table.
            Field::Cell { table, .. } => format!("{table}."),
            Field::ListItem { name, .. } => name.to_string(),
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

fn complete_index(head: &str, typed: &str, subject: &Subject) -> Vec<String> {
    let Some((table_name, _)) = head.rsplit_once('.') else {
        let Ok(name) = Identifier::new(head) else {
            return Vec::new();
        };
        let Ok(AttributeValue::List(values)) = subject.sample.attribute(&name) else {
            return Vec::new();
        };
        return (0..values.len())
            .map(|position| format!("{head}[#{position}]"))
            .filter(|candidate| candidate.starts_with(&format!("{head}[{typed}")))
            .collect();
    };
    let Ok(table_name) = Identifier::new(table_name) else {
        return Vec::new();
    };
    let Ok(table) = subject.sample.table(&table_name) else {
        return Vec::new();
    };
    let written: Vec<String> = table
        .index_tuples()
        .into_iter()
        .map(|tuple| {
            tuple
                .into_iter()
                .map(describe_index_value)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    starting_with(typed, written)
        .into_iter()
        .map(|index| format!("{head}[{index}]"))
        .collect()
}

/// Candidates matching what has been typed, in enumeration order. A single
/// strong match is still one entry: shortening the path to a name is the point,
/// and the caller decides whether to take it without waiting for a separator.
fn starting_with(typed: &str, candidates: Vec<String>) -> Vec<String> {
    if typed.is_empty() {
        return candidates;
    }
    let lowered = typed.to_lowercase();
    if let Some(exact) = candidates
        .iter()
        .find(|candidate| candidate.trim_end_matches(['.', '[', ']']).to_lowercase() == lowered)
    {
        return vec![exact.clone()];
    }
    candidates
        .into_iter()
        .filter(|candidate| candidate.to_lowercase().starts_with(&lowered))
        .collect()
}

/// A path meant to be **stored** — in a configuration file, a saved query —
/// and read back later against a collection that may have moved on.
///
/// This is the one place a field path is written, so it is where an ordinal is
/// refused: `[#3]` names a different row as soon as one is inserted, and a
/// stored path that silently changes meaning is worse than one that never
/// saved.
pub fn serialize(field: &Field) -> Result<String, FieldError> {
    if let Field::ListItem { name, position } = field {
        return Err(malformed(
            &format!("{name}[#{position}]"),
            "a position cannot be stored: it names a different item as soon as one is inserted",
        ));
    }
    if let Field::Cell {
        table,
        column,
        row: row @ (RowAddress::Ordinal(_) | RowAddress::FromEnd(_)),
        ..
    } = field
    {
        let position = match row {
            RowAddress::FromEnd(back) => format!("-{back}"),
            RowAddress::Ordinal(position) => position.to_string(),
            _ => String::new(),
        };
        return Err(malformed(
            &format!("{table}.{column}[#{position}]"),
            "a position cannot be stored: it names a different row as soon as one \
             is inserted. Write the index values instead",
        ));
    }
    Ok(describe(field))
}

/// The same quantity, read on another channel.
///
/// An export needs both halves of every column — `malt_value` and
/// `malt_uncertainty` — and re-parsing `"malt.uncertainty"` from a string would
/// mean building a path in order to take it apart again. A reserved field has
/// no channel to set and comes back unchanged.
pub fn with_channel(field: &Field, channel: Channel) -> Field {
    match field {
        Field::Named { name, .. } => Field::Named {
            name: name.clone(),
            channel,
        },
        Field::ListItem { name, position } => Field::ListItem {
            name: name.clone(),
            position: *position,
        },
        Field::Cell {
            table, column, row, ..
        } => Field::Cell {
            table: table.clone(),
            column: column.clone(),
            row: row.clone(),
            channel,
        },
        Field::Reserved(reserved) => Field::Reserved(*reserved),
    }
}

/// The channel a field reads on, for a caller deciding what a path names.
pub fn channel(field: &Field) -> Channel {
    match field {
        Field::Named { channel, .. } | Field::Cell { channel, .. } => *channel,
        Field::ListItem { .. } => Channel::Value,
        Field::Reserved(_) => Channel::Value,
    }
}
