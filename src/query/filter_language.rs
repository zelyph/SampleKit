//! The expression language that selects samples, its evaluation, and — equally —
//! its diagnostics. The most user-facing part of the library that is not a user
//! interface.

use std::fmt;

use crate::core::identifier::Identifier;
use crate::core::sample::Sample;
use crate::core::value::{self, Value, ValueError, ValueKind};
use crate::query::field_addressing::{
    self as fields, Field, FieldError, ReservedField, Resolution, StateWord, Subject,
};

// ------------------------------------------------------------------- types

#[derive(Debug)]
pub struct Filter {
    root: Expr,
    fields: Vec<Field>,
    tags: Vec<Identifier>,
    /// The words asked of `state`, so that a caller reads the states only when
    /// `state` is named, and the model only when a word needs it.
    states: Vec<StateWord>,
}

/// The parsed expression, one variant per production of the grammar. Private:
/// a filter is built by `parse` and read by `evaluate`, and nothing outside
/// needs to walk it.
#[derive(Debug, Clone)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Compare {
        field: Field,
        operator: Operator,
        operand: Value,
    },
    /// `radius > notch`: a field on either side. A bare word after `==` or `!=` is
    /// the field only where the sample holds one of that name, and the text
    /// otherwise, as it always was; quoted, it is the text.
    CompareFields {
        field: Field,
        operator: Operator,
        other: Field,
        word: String,
    },
    Membership {
        field: Field,
        operands: Vec<Value>,
    },
    Presence {
        field: Field,
        present: bool,
    },
    HasTag {
        field: Field,
        operand: Value,
    },
    /// `printing.gas has argon`, `printing has argon`: whether a cell of the
    /// column — of any column, where none is named — holds the operand.
    TableHas {
        table: Identifier,
        column: Option<Identifier>,
        operand: Value,
    },
    /// `printing where (gas == argon && laser_power > 200)`: whether one row
    /// meets the condition, a name of the table's columns that row's cell.
    Where {
        table: Identifier,
        condition: Box<Expr>,
    },
    /// `state == failed`, `state has failed`, `state in (a, b)`: whether the
    /// sample holds any of the words; `!=` is its negation. Its own variant
    /// because a state is a set asked about, never a value compared.
    State {
        words: Vec<StateWord>,
        negated: bool,
    },
    /// `links to steel-02`, `parents links to P3`: whether an attribute, or
    /// the one named, holds a link to that sample as the link writes it: a
    /// wikilink's target, a Markdown link's file name.
    LinksTo {
        field: Option<Identifier>,
        target: String,
    },
}

/// The eight that compare two scalars. `has` is not among them: it is its own
/// production, so it cannot be built against a field that is not `tags`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Contains,
    StartsWith,
}

/// A predicate over an absent value is unknown, and unknown is a third answer
/// rather than a second kind of false.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    Unknown,
}

/// Every problem in the collection, reported in one pass. A user fixing a
/// filter one error per run is a user running the command five times.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub unknown_fields: Vec<FieldError>,
    pub type_conflicts: Vec<TypeConflict>,
    /// An operator the field cannot take, whatever the collection holds.
    pub wrong_operators: Vec<OperatorMisuse>,
    pub unknown_tags: Vec<TagWarning>,
    /// A text compared with that no sample holds for its field, where one
    /// holds a text close to it: `style == ipaa` beside `titanium`. A warning, as
    /// an unknown tag is, and not counted by [`Diagnostics::is_empty`].
    pub unmatched_texts: Vec<TextWarning>,
    /// A number compared with `==` that no sample holds, where one holds a
    /// number shown as it, rounded: `radius == 1.049` beside a stored 1.0487. A
    /// warning, said whole.
    pub unmatched_numbers: Vec<String>,
    /// Samples a comparison cannot be asked of because the field holds text
    /// there, where other samples hold what it compares: left aside by
    /// [`evaluate_leaving_aside`], and named by the caller. Not a problem of
    /// the filter, so [`Diagnostics::is_empty`] does not count it.
    pub set_aside: Vec<SetAside>,
}

/// A comparison some samples cannot answer, because the field holds text in
/// them: `radius > 1.06` where one piece's `radius` is `high`.
#[derive(Debug, Clone, PartialEq)]
pub struct SetAside {
    pub field: Field,
    pub operator: String,
    /// The samples, by name, or by nothing where one has none.
    pub samples: Vec<String>,
}

/// A tag no sample in the collection carries. A warning and not an error: `tags
/// has broke` is a well-formed question whose answer may legitimately be empty,
/// which a misspelled *field* never is.
#[derive(Debug, Clone, PartialEq)]
pub struct TagWarning {
    pub tag: Identifier,
    pub suggestion: Option<String>,
}

/// A text no sample holds for the field compared with, and the one held that
/// it is nearest to — the same letters in another case, or a slip away.
#[derive(Debug, Clone, PartialEq)]
pub struct TextWarning {
    pub field: String,
    pub written: String,
    pub suggestion: String,
}

impl fmt::Display for TextWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "no sample has {} '{}' — did you mean '{}'?",
            self.field, self.written, self.suggestion
        )
    }
}

/// One comparison that cannot succeed anywhere in the collection.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeConflict {
    pub field: Field,
    pub operator: String,
    pub operand: Value,
    pub field_kind: ValueKind,
}

/// An operator used on a field that cannot take it — `contains` on `tags` —
/// with the reason, which says which operator to use instead.
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorMisuse {
    pub field: Field,
    pub operator: String,
    pub reason: String,
}

impl OperatorMisuse {
    pub fn error(&self) -> FilterError {
        FilterError::WrongOperator {
            field: Box::new(self.field.clone()),
            operator: self.operator.clone(),
            reason: self.reason.clone(),
        }
    }
}

impl Diagnostics {
    pub fn is_empty(&self) -> bool {
        self.unknown_fields.is_empty()
            && self.type_conflicts.is_empty()
            && self.wrong_operators.is_empty()
            && self.unknown_tags.is_empty()
    }
}

impl Truth {
    /// The collapse to a yes-or-no, which happens once and in the caller — the
    /// layer that knows whether to report the unknowns.
    pub fn selects(self) -> bool {
        matches!(self, Truth::True)
    }

    fn not(self) -> Truth {
        match self {
            Truth::True => Truth::False,
            Truth::False => Truth::True,
            Truth::Unknown => Truth::Unknown,
        }
    }

    fn and(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::False, _) | (_, Truth::False) => Truth::False,
            (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
            _ => Truth::True,
        }
    }

    fn or(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::True, _) | (_, Truth::True) => Truth::True,
            (Truth::Unknown, _) | (_, Truth::Unknown) => Truth::Unknown,
            _ => Truth::False,
        }
    }
}

impl Operator {
    fn as_str(self) -> &'static str {
        match self {
            Operator::Eq => "==",
            Operator::Ne => "!=",
            Operator::Gt => ">",
            Operator::Ge => ">=",
            Operator::Lt => "<",
            Operator::Le => "<=",
            Operator::Contains => "contains",
            Operator::StartsWith => "starts_with",
        }
    }
}

// --------------------------------------------------------------- tokenizing

/// One lexeme and where it started, because every syntax error carries a
/// character position and renders with a caret under it.
#[derive(Debug, Clone, PartialEq)]
struct Token {
    at: usize,
    kind: TokenKind,
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Open,
    Close,
    Comma,
    /// A bare run of characters: a field path, an operator word, a keyword, or
    /// an unquoted operand. Which of those it is, the parser decides.
    Word(String),
    Quoted(String),
    Symbol(String),
}

/// Quote-aware rather than whitespace-splitting. Splitting on whitespace is
/// what made `alloy == "Duplex 57"` require care and made an index containing a
/// space unaddressable.
fn tokenize(source: &str) -> Result<Vec<Token>, FilterError> {
    let characters: Vec<(usize, char)> = source.char_indices().collect();
    let mut tokens = Vec::new();
    let mut at = 0usize;
    while at < characters.len() {
        // Positions count characters, as a caret under the expression does.
        let (_, character) = characters[at];
        let start = at;
        match character {
            c if c.is_whitespace() => at += 1,
            '(' => {
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Open,
                });
                at += 1;
            }
            ')' => {
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Close,
                });
                at += 1;
            }
            ',' => {
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Comma,
                });
                at += 1;
            }
            // Double or single quotes: a filter is often written inside the
            // other kind for the shell.
            quote @ ('"' | '\'') => {
                let mut text = String::new();
                at += 1;
                loop {
                    match characters.get(at) {
                        None => {
                            return Err(FilterError::UnterminatedString { position: start });
                        }
                        Some((_, c)) if *c == quote => {
                            at += 1;
                            break;
                        }
                        Some((_, c)) => {
                            text.push(*c);
                            at += 1;
                        }
                    }
                }
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Quoted(text),
                });
            }
            // `&&` and `||` are the connectives, and the only spelling of them.
            '&' | '|' => {
                if characters.get(at + 1).map(|(_, next)| *next) != Some(character) {
                    return Err(FilterError::Syntax {
                        position: start,
                        expected: format!("'{character}{character}'"),
                        found: format!("'{character}'"),
                    });
                }
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Word(format!("{character}{character}")),
                });
                at += 2;
            }
            // A `!` not followed by `=` is negation.
            '!' if characters.get(at + 1).map(|(_, next)| *next) != Some('=') => {
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Word("!".to_string()),
                });
                at += 1;
            }
            // The comparison operators, longest first so that `>=` is not `>`.
            '=' | '!' | '<' | '>' => {
                let mut symbol = String::new();
                symbol.push(character);
                at += 1;
                if let Some((_, '=')) = characters.get(at) {
                    symbol.push('=');
                    at += 1;
                }
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Symbol(symbol),
                });
            }
            _ => {
                let mut word = String::new();
                // Inside a field's brackets everything is its index, quotes
                // and spaces included: `process.duration["stage 2"]` is one
                // field, as a column takes it, and not a field cut at `"`.
                let mut depth = 0usize;
                let mut quoted: Option<char> = None;
                while let Some((_, c)) = characters.get(at) {
                    let c = *c;
                    if let Some(quote) = quoted {
                        if c == quote {
                            quoted = None;
                        }
                    } else if depth > 0 {
                        match c {
                            '"' | '\'' => quoted = Some(c),
                            '[' => depth += 1,
                            ']' => depth -= 1,
                            _ => {}
                        }
                    } else if c == '[' {
                        depth = 1;
                    } else if c.is_whitespace()
                        || matches!(
                            c,
                            '(' | ')' | ',' | '"' | '\'' | '=' | '!' | '<' | '>' | '&' | '|'
                        )
                    {
                        break;
                    }
                    word.push(c);
                    at += 1;
                }
                tokens.push(Token {
                    at: start,
                    kind: TokenKind::Word(word),
                });
            }
        }
    }
    Ok(tokens)
}

// -------------------------------------------------------------- completion

/// What the grammar expects at the position being typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Expect {
    Field,
    /// After `TABLE where`: the parenthesis opening the condition.
    Open,
    Operator,
    /// After `is`: `missing` or `present`, and nothing else.
    Presence,
    Operand,
    Connective,
}

/// The token being typed, and everything settled before it. Only the tail is
/// split by hand: what precedes it is always well formed, so the real
/// `tokenize` reads it and completion inherits every rule it enforces.
struct Tail<'a> {
    head: &'a str,
    typed: String,
    /// The quote still open around the tail, when there is one.
    quote: Option<char>,
}

fn split_tail(source: &str) -> Tail<'_> {
    let characters: Vec<(usize, char)> = source.char_indices().collect();
    let mut at = 0usize;
    let mut opened: Option<(usize, char)> = None;
    while at < characters.len() {
        let (index, character) = characters[at];
        if let '"' | '\'' = character {
            let quote = character;
            let start = index;
            at += 1;
            let mut closed = false;
            while at < characters.len() {
                let (_, c) = characters[at];
                at += 1;
                if c == quote {
                    closed = true;
                    break;
                }
            }
            if !closed {
                opened = Some((start, quote));
            }
        } else {
            at += 1;
        }
    }
    if let Some((start, quote)) = opened {
        return Tail {
            head: &source[..start],
            typed: source[start + quote.len_utf8()..].to_string(),
            quote: Some(quote),
        };
    }
    // A bare tail runs back to the last whitespace or delimiter.
    let split = source
        .char_indices()
        .rev()
        .find(|(_, c)| {
            // An operator being typed belongs to the tail: `diameter !` is a partial
            // `!=`, which the tokenizer alone would read as the connective `not`.
            c.is_whitespace() || matches!(c, '(' | ')' | ',')
        })
        .map_or(0, |(index, c)| index + c.len_utf8());
    Tail {
        head: &source[..split],
        typed: source[split..].to_string(),
        quote: None,
    }
}

/// Walks the settled tokens and reports what comes next, and which field the
/// position belongs to. A forward walk rather than a backward look: the
/// grammar is what decides, and reading it forwards is how the parser reads it.
fn expectation(tokens: &[Token]) -> (Expect, Option<String>, Option<String>) {
    let mut expect = Expect::Field;
    let mut field: Option<String> = None;
    // `in` opens a list, and its parenthesis holds operands rather than the
    // start of another predicate.
    let mut membership = false;
    // The table each open parenthesis asks a row of — `TABLE where (` — or
    // none, innermost last.
    let mut rows: Vec<Option<String>> = Vec::new();
    let mut table: Option<String> = None;
    for token in tokens {
        match &token.kind {
            TokenKind::Open => {
                expect = if membership {
                    membership = false;
                    Expect::Operand
                } else {
                    Expect::Field
                };
                rows.push(table.take());
            }
            TokenKind::Close => {
                expect = Expect::Connective;
                rows.pop();
            }
            // Inside `in (a, b`, a comma opens another operand.
            TokenKind::Comma => expect = Expect::Operand,
            TokenKind::Symbol(_) => expect = Expect::Operand,
            TokenKind::Quoted(_) => expect = Expect::Connective,
            TokenKind::Word(word) => {
                let lowered = word.to_lowercase();
                expect = match lowered.as_str() {
                    // `and`, `or` and `not` are names: as words they fall
                    // through to what a field is followed by.
                    "&&" | "||" | "!" => Expect::Field,
                    "contains" | "starts_with" | "has" => Expect::Operand,
                    "in" => {
                        membership = true;
                        Expect::Operand
                    }
                    "is" => Expect::Presence,
                    "where" if matches!(expect, Expect::Operator) => {
                        table = field.clone();
                        Expect::Open
                    }
                    "missing" | "present" => Expect::Connective,
                    _ => match expect {
                        Expect::Field => {
                            field = Some(word.clone());
                            Expect::Operator
                        }
                        _ => Expect::Connective,
                    },
                };
            }
        }
    }
    let row = rows.into_iter().rev().flatten().next();
    (expect, field, row)
}

/// An operand as it can be typed back, which is the rule Field addressing's
/// index paths already follow: text holding a space is quoted, and everything
/// else is written as it reads.
fn typeable(value: &Value) -> String {
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

/// The operators a field can take. `has` belongs to `tags` and to lists, and is
/// offered only there, so that completion cannot propose what `check` refuses.
fn operators_for(field: Option<&str>, subject: &Subject) -> Vec<String> {
    // A set of words: asked whether it holds one, never ordered, never absent.
    if field == Some("state") {
        return ["==", "!=", "has", "in"]
            .iter()
            .map(|operator| (*operator).to_string())
            .collect();
    }
    let list_like = field.is_some_and(|name| {
        name == "tags"
            || fields::parse(name).is_ok_and(|field| {
                matches!(
                    fields::resolve(&field, subject),
                    Ok(fields::Resolution::List(_) | fields::Resolution::Tags(_))
                )
            })
    });
    // A whole list takes `has` and none of the scalar comparisons: offering
    // those would propose exactly what `check` refuses, which is worse than
    // offering nothing. `is missing` remains askable of any field.
    let mut offered: Vec<String> = if list_like {
        vec!["has".to_string()]
    } else {
        ["==", "!=", ">", ">=", "<", "<=", "contains", "starts_with"]
            .iter()
            .map(|operator| (*operator).to_string())
            .collect()
    };
    offered.push("is".to_string());
    if !list_like {
        offered.push("in".to_string());
    }
    offered
}

/// Every value this sample holds for the field, as operands. One sample's
/// answer: a caller with a collection unions them, which is how the menu comes
/// to show what the collection holds rather than what one sample does.
fn operands_for(field: Option<&str>, subject: &Subject) -> Vec<String> {
    let Some(name) = field else {
        return Vec::new();
    };
    // Its words, not what the collection holds: the states are read only for a
    // filter that names them, and every word can be asked.
    if name == "state" {
        return StateWord::WORDS
            .iter()
            .map(|word| (*word).to_string())
            .collect();
    }
    let Ok(parsed) = fields::parse(name) else {
        return Vec::new();
    };
    match fields::resolve(&parsed, subject) {
        Ok(fields::Resolution::Scalar(Some(value))) => vec![typeable(&value)],
        Ok(fields::Resolution::List(values)) => values.iter().map(typeable).collect(),
        Ok(fields::Resolution::Tags(tags)) => tags.iter().map(Identifier::to_string).collect(),
        _ => Vec::new(),
    }
}

/// A table or a column without its row, named in a filter: `(table, column)`.
fn whole(name: &str, subject: &Subject) -> Option<(Identifier, Option<Identifier>)> {
    match fields::parse(name) {
        Ok(Field::Named {
            name,
            channel: fields::Channel::Value,
        }) if subject.vocabulary.has_table(&name) => Some((name, None)),
        Err(FieldError::MissingIndex { table, column })
            if subject.vocabulary.has_column(&table, &column) =>
        {
            Some((table, Some(column)))
        }
        _ => None,
    }
}

/// What a whole table takes — `has`, `where` — and a whole column — `has`;
/// inside `where`, a column is one row's cell, and takes what a cell does.
fn whole_operators(field: Option<&str>, in_row: bool, subject: &Subject) -> Option<Vec<String>> {
    if in_row {
        return None;
    }
    match whole(field?, subject)? {
        (_, None) => Some(vec!["has".to_string(), "where".to_string()]),
        (_, Some(_)) => Some(vec!["has".to_string()]),
    }
}

/// What a whole table's or column's cells hold, as operands.
fn cells_of(field: Option<&str>, subject: &Subject) -> Option<Vec<String>> {
    let (table, column) = whole(field?, subject)?;
    let Ok(held) = subject.sample.table(&table) else {
        return Some(Vec::new());
    };
    let columns: Vec<Identifier> = match column {
        Some(column) => vec![column],
        None => held.column_names().into_iter().cloned().collect(),
    };
    let mut values: Vec<String> = Vec::new();
    for column in &columns {
        for position in 0..held.rows().count() {
            let cell = Field::Cell {
                table: table.clone(),
                column: column.clone(),
                row: crate::core::table::RowAddress::Ordinal(position),
                channel: fields::Channel::Value,
            };
            if let Ok(Some(value)) = scalar(&cell, subject) {
                let written = typeable(&value);
                if !written.is_empty() && !values.contains(&written) {
                    values.push(written);
                }
            }
        }
    }
    Some(values)
}

/// The fields a filter may name here: the sample's; a table by its name and a
/// column without its row, which `has` and `where` ask; inside `TABLE where
/// (…)`, that table's columns first.
fn field_candidates(typed: &str, row: Option<&str>, subject: &Subject) -> Vec<String> {
    let lowered = typed.to_lowercase();
    let mut candidates: Vec<String> = Vec::new();
    if let Some(table) = row.and_then(|table| Identifier::new(table).ok()) {
        candidates.extend(
            subject
                .vocabulary
                .columns_as_strings(&table)
                .into_iter()
                .filter(|column| column.to_lowercase().starts_with(&lowered)),
        );
    }
    for table in subject.vocabulary.tables_as_strings() {
        if table.to_lowercase().starts_with(&lowered) && !candidates.contains(&table) {
            candidates.push(table.clone());
        }
        if let Some(column) = typed.strip_prefix(&format!("{table}."))
            && !column.contains('[')
            && let Ok(name) = Identifier::new(&table)
        {
            for held in subject.vocabulary.columns_as_strings(&name) {
                let whole = format!("{table}.{held}");
                if held.starts_with(column) && !candidates.contains(&whole) {
                    candidates.push(whole);
                }
            }
        }
    }
    for candidate in fields::complete(typed, subject) {
        if !candidates.contains(&candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

/// What could come next in a half-typed expression.
///
/// Never parses and never fails: completion runs on every keystroke, where a
/// half-written expression is the normal state. Candidates are whole
/// expressions — what precedes the position is carried through — so that a
/// shell replaces the written word and the rest of the filter survives.
pub fn complete(source: &str, subject: &Subject) -> Vec<String> {
    let tail = split_tail(source);
    // The settled part is well formed by construction; a head that still does
    // not read (a lone `&`) leaves completion silent rather than wrong.
    let Ok(tokens) = tokenize(tail.head) else {
        return Vec::new();
    };
    let (expect, field, row) = expectation(&tokens);
    // Inside `TABLE where (…)`, a name of its columns is that column.
    let field = match (&row, field) {
        (Some(table), Some(name))
            if Identifier::new(table).is_ok_and(|table| {
                Identifier::new(&name)
                    .is_ok_and(|column| subject.vocabulary.has_column(&table, &column))
            }) =>
        {
            Some(format!("{table}.{name}"))
        }
        (_, field) => field,
    };
    // A word operator accepted against the closing quote cannot be continued:
    // `has` and `in` need the space that follows them, and so does a connective.
    let (candidates, trailing) = match expect {
        Expect::Field => (field_candidates(&tail.typed, row.as_deref(), subject), ""),
        Expect::Open => (vec!["(".to_string()], ""),
        Expect::Operator => (
            whole_operators(field.as_deref(), row.is_some(), subject)
                .unwrap_or_else(|| operators_for(field.as_deref(), subject)),
            " ",
        ),
        Expect::Presence => (vec!["missing".to_string(), "present".to_string()], " "),
        Expect::Operand => (
            cells_of(field.as_deref(), subject)
                .unwrap_or_else(|| operands_for(field.as_deref(), subject)),
            "",
        ),
        Expect::Connective => (["&&", "||"].iter().map(|w| (*w).to_string()).collect(), " "),
    };
    let typed = tail.typed.to_lowercase();
    // A field's candidates are already those the typed text leads to — and some
    // do not begin with it: `hv` finds the column `measurements.hv[20]` by
    // its own name, which a second prefix test here threw away.
    let matched = matches!(expect, Expect::Field);
    candidates
        .into_iter()
        .filter(|candidate| {
            if matched {
                return true;
            }
            // Inside an open quote the reader has typed the text itself, never
            // the quote a value carries: `"DC` must still find `"DC 57"`.
            let bare = match tail.quote {
                Some(_) => candidate.trim_matches('"'),
                None => candidate.as_str(),
            };
            tail.typed.is_empty() || bare.to_lowercase().starts_with(&typed)
        })
        .map(|candidate| match tail.quote {
            // Inside an open quote the candidate closes it, and any quoting the
            // value would have carried is already the quote the reader opened.
            Some(quote) => format!("{}{quote}{}{quote}", tail.head, candidate.trim_matches('"')),
            None => format!("{}{candidate}{trailing}", tail.head),
        })
        .collect()
}

// ----------------------------------------------------------------- parsing

/// Syntax only: no sample is consulted, and no field is resolved. A filter
/// validates against no data, which is what lets `.samplekitrc` hold one.
pub fn parse(source: &str) -> Result<Filter, FilterError> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Err(FilterError::Syntax {
            position: 0,
            expected: "a predicate".to_string(),
            found: "nothing".to_string(),
        });
    }
    let mut parser = Parser {
        tokens: &tokens,
        at: 0,
        end: source.chars().count(),
        fields: Vec::new(),
        tags: Vec::new(),
        states: Vec::new(),
        depth: 0,
        terms: 0,
    };
    let root = parser.expression()?;
    if let Some(token) = parser.peek() {
        if token.kind == TokenKind::Close {
            return Err(FilterError::UnexpectedClose { position: token.at });
        }
        // The words were once operators, and are what a reader of other
        // languages types first: the refusal says what to type instead.
        if let TokenKind::Word(word) = &token.kind
            && let Some(symbol) = symbol_once_spelt(word)
        {
            return Err(FilterError::Syntax {
                position: token.at,
                expected: format!("'{symbol}'"),
                found: format!("'{word}', which is a name: the operator is written '{symbol}'"),
            });
        }
        return Err(parser.unexpected("the end of the expression"));
    }
    Ok(Filter {
        root,
        fields: parser.fields,
        tags: parser.tags,
        states: parser.states,
    })
}

/// The symbol a word stood for before it became a name.
fn symbol_once_spelt(word: &str) -> Option<&'static str> {
    match word.to_lowercase().as_str() {
        "and" => Some("&&"),
        "or" => Some("||"),
        "not" => Some("!"),
        _ => None,
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    end: usize,
    fields: Vec<Field>,
    tags: Vec<Identifier>,
    states: Vec<StateWord>,
    /// How deep the parentheses and negations open now.
    depth: usize,
    /// How many predicates the connectives joined so far.
    terms: usize,
}

/// The deepest a filter nests, and the most predicates it joins: past them a
/// filter is refused rather than read, since reading it — and weighing it,
/// sample by sample — goes as deep as it nests. Ten thousand parentheses
/// overflowed the stack, and `samplekit` stopped on SIGABRT, `validate`
/// among the commands, a query in `.samplekitrc` holding one.
const MAX_DEPTH: usize = 200;
const MAX_TERMS: usize = 2000;

impl<'a> Parser<'a> {
    /// One more level, refused past [`MAX_DEPTH`].
    fn deeper(&mut self, at: usize) -> Result<(), FilterError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(FilterError::Syntax {
                position: at,
                expected: format!("at most {MAX_DEPTH} parentheses and negations, one in another"),
                found: "more".to_string(),
            });
        }
        Ok(())
    }

    /// One more predicate joined, refused past [`MAX_TERMS`].
    fn joined(&mut self) -> Result<(), FilterError> {
        self.terms += 1;
        if self.terms > MAX_TERMS {
            return Err(FilterError::Syntax {
                position: self.position(),
                expected: format!("at most {MAX_TERMS} predicates joined"),
                found: "more".to_string(),
            });
        }
        Ok(())
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.at)
    }

    fn position(&self) -> usize {
        self.peek().map_or(self.end, |token| token.at)
    }

    fn word(&self) -> Option<&'a str> {
        match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Word(word)) => Some(word),
            _ => None,
        }
    }

    /// A keyword matches case-insensitively, so `IS` works — but it is only a
    /// keyword where one is expected: `is`, `in`, `has`, `contains` and
    /// `starts_with` after a field, `not`, `missing` and `present` after `is`.
    /// The sample `links to` names, a word or quoted.
    fn links_to(&mut self, field: Option<Identifier>) -> Result<Expr, FilterError> {
        let target = match self.peek().map(|token| token.kind.clone()) {
            Some(TokenKind::Word(word)) => word,
            Some(TokenKind::Quoted(text)) => text,
            _ => return Err(self.unexpected("the sample linked to, by its file's name")),
        };
        self.at += 1;
        Ok(Expr::LinksTo { field, target })
    }

    fn keyword(&mut self, word: &str) -> bool {
        if self.word().is_some_and(|w| w.eq_ignore_ascii_case(word)) {
            self.at += 1;
            return true;
        }
        false
    }

    /// `&&`, `||` or `!`: the connectives are symbols, and `and`, `or` and
    /// `not` are names like any other.
    fn connective(&mut self, symbol: &str) -> bool {
        if self.word() == Some(symbol) {
            self.at += 1;
            return true;
        }
        false
    }

    fn unexpected(&self, expected: &str) -> FilterError {
        let (position, found) = match self.peek() {
            None => (self.end, "the end of the expression".to_string()),
            Some(token) => (
                token.at,
                match &token.kind {
                    TokenKind::Open => "'('".to_string(),
                    TokenKind::Close => "')'".to_string(),
                    TokenKind::Comma => "','".to_string(),
                    TokenKind::Word(word) => format!("'{word}'"),
                    TokenKind::Quoted(text) => format!("\"{text}\""),
                    TokenKind::Symbol(symbol) => format!("'{symbol}'"),
                },
            ),
        };
        FilterError::Syntax {
            position,
            expected: expected.to_string(),
            found,
        }
    }

    fn expression(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.conjunction()?;
        while self.connective("||") {
            self.joined()?;
            let right = self.conjunction()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn conjunction(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.negation()?;
        while self.connective("&&") {
            self.joined()?;
            let right = self.negation()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn negation(&mut self) -> Result<Expr, FilterError> {
        if self.connective("!") {
            self.deeper(self.position())?;
            let inside = self.negation()?;
            self.depth -= 1;
            return Ok(Expr::Not(Box::new(inside)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, FilterError> {
        if let Some(token) = self.peek()
            && token.kind == TokenKind::Open
        {
            {
                let opened = token.at;
                self.at += 1;
                self.deeper(opened)?;
                let inside = self.expression()?;
                self.depth -= 1;
                match self.peek() {
                    Some(Token {
                        kind: TokenKind::Close,
                        ..
                    }) => {
                        self.at += 1;
                        return Ok(inside);
                    }
                    // The position of the '(' and not of the end of input: the
                    // reader needs to see which one was never closed.
                    _ => return Err(FilterError::UnbalancedParenthesis { position: opened }),
                }
            }
        }
        self.predicate()
    }

    fn predicate(&mut self) -> Result<Expr, FilterError> {
        let at = self.position();
        let Some(path) = self.word().map(str::to_string) else {
            return Err(self.unexpected("a field"));
        };
        self.at += 1;
        // `links to NAME`: any attribute's link.
        if path.eq_ignore_ascii_case("links") && self.keyword("to") {
            return self.links_to(None);
        }
        // `FIELD links to NAME`: that attribute's.
        if self.word().is_some_and(|word| word.eq_ignore_ascii_case("links"))
            && self
                .tokens
                .get(self.at + 1)
                .is_some_and(|token| matches!(&token.kind, TokenKind::Word(word) if word.eq_ignore_ascii_case("to")))
        {
            let name = Identifier::new(&path).map_err(|_| FilterError::Syntax {
                position: at,
                expected: "an attribute's name".to_string(),
                found: format!("'{path}'"),
            })?;
            self.at += 2;
            return self.links_to(Some(name));
        }
        // `not x > 1`: a field called `not` followed by another field, or by a
        // parenthesis, was once a negation, and is said as such.
        if path.eq_ignore_ascii_case("not")
            && matches!(
                self.peek().map(|token| &token.kind),
                Some(TokenKind::Word(_) | TokenKind::Open)
            )
            && !["is", "in", "has", "contains", "starts_with"]
                .iter()
                .any(|operator| {
                    self.word()
                        .is_some_and(|w| w.eq_ignore_ascii_case(operator))
                })
        {
            return Err(FilterError::Syntax {
                position: at,
                expected: "'!'".to_string(),
                found: format!("'{path}', which is a name: negation is written '!'"),
            });
        }
        // A column without its row is a list of its cells, which `has` asks.
        let field = match fields::parse(&path) {
            Err(FieldError::MissingIndex { table, column }) if self.keyword("has") => {
                let operand = self.has_operand()?;
                return Ok(Expr::TableHas {
                    table,
                    column: Some(column),
                    operand,
                });
            }
            parsed => parsed.map_err(|e| FilterError::Field(Box::new(e)))?,
        };
        // `TABLE where (…)`: one row meeting the condition, its names that
        // row's cells. They are not the sample's fields.
        if let Field::Named {
            name,
            channel: fields::Channel::Value,
        } = &field
            && self.keyword("where")
        {
            let opened = match self.peek() {
                Some(Token {
                    kind: TokenKind::Open,
                    at,
                }) => *at,
                _ => return Err(self.unexpected("'(' opening the condition a row meets")),
            };
            self.at += 1;
            self.deeper(opened)?;
            let held = self.fields.len();
            let condition = self.expression()?;
            self.fields.truncate(held);
            self.depth -= 1;
            match self.peek() {
                Some(Token {
                    kind: TokenKind::Close,
                    ..
                }) => self.at += 1,
                _ => return Err(FilterError::UnbalancedParenthesis { position: opened }),
            }
            return Ok(Expr::Where {
                table: name.clone(),
                condition: Box::new(condition),
            });
        }
        if !self.fields.contains(&field) {
            self.fields.push(field.clone());
        }

        if field == Field::Reserved(ReservedField::State) {
            return self.state(field);
        }

        // `is`, `in` and `has` are productions of their own, which is what
        // stops `has` from being buildable against a field that is not `tags`.
        if self.keyword("is") {
            let negated = self.keyword("not");
            let present = match self.word() {
                Some(word) if word.eq_ignore_ascii_case("missing") => false,
                Some(word) if word.eq_ignore_ascii_case("present") => true,
                _ => return Err(self.unexpected("'missing' or 'present'")),
            };
            self.at += 1;
            return Ok(Expr::Presence {
                field,
                present: present != negated,
            });
        }
        if self.keyword("in") {
            return self.membership(field);
        }
        if self.keyword("has") {
            let Some(token) = self.peek().cloned() else {
                return Err(self.unexpected("a tag"));
            };
            let (text, operand) = match &token.kind {
                TokenKind::Word(word) => (word.clone(), literal(word)),
                TokenKind::Quoted(text) => (text.clone(), Value::text(text.clone())),
                _ => return Err(self.unexpected("a tag")),
            };
            self.at += 1;
            let is_tags = matches!(
                &field,
                Field::Named { name, .. } if name.as_str() == "tags"
            );
            if is_tags {
                let tag = Identifier::new(&text).map_err(|error| FilterError::Syntax {
                    position: token.at,
                    expected: "a tag, which is an identifier".to_string(),
                    found: error.to_string(),
                })?;
                if !self.tags.contains(&tag) {
                    self.tags.push(tag);
                }
            }
            return Ok(Expr::HasTag { field, operand });
        }

        let operator = self.operator()?;
        // A bare word naming a field compares two fields: `radius > notch`. After an
        // ordering operator it can be nothing else a sample holds; after `==`
        // or `!=` it may be the text `style == titanium` always was, and is the
        // field only where the sample holds one.
        if let Some(Token {
            kind: TokenKind::Word(word),
            ..
        }) = self.peek()
            && matches!(literal(word), Value::Text(_))
            && !matches!(operator, Operator::Contains | Operator::StartsWith)
            && let Ok(other) = fields::parse(word)
            && !matches!(other, Field::Reserved(_))
        {
            let word = word.clone();
            self.at += 1;
            if !matches!(operator, Operator::Eq | Operator::Ne) && !self.fields.contains(&other) {
                self.fields.push(other.clone());
            }
            let _ = at;
            return Ok(Expr::CompareFields {
                field,
                operator,
                other,
                word,
            });
        }
        let operand = self.operand()?;
        let _ = at;
        Ok(Expr::Compare {
            field,
            operator,
            operand,
        })
    }

    /// What `has` asks of a column or a table: a word, or a quoted text.
    fn has_operand(&mut self) -> Result<Value, FilterError> {
        let operand = match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Word(word)) => literal(word),
            Some(TokenKind::Quoted(text)) => Value::text(text.clone()),
            _ => return Err(self.unexpected("a value a cell may hold")),
        };
        self.at += 1;
        Ok(operand)
    }

    /// `state == X`, `state != X`, `state has X`, `state in (X, Y)`: whether
    /// the sample holds a word, its words checked here, since they are the
    /// language's and need no sample.
    fn state(&mut self, field: Field) -> Result<Expr, FilterError> {
        let refused = |operator: &str| FilterError::WrongOperator {
            field: Box::new(field.clone()),
            operator: operator.to_string(),
            reason: "'state' is a set of words: ==, has and in ask whether a sample holds one, \
                     as in state == failed"
                .to_string(),
        };
        let (words, negated) = if self.keyword("has") {
            (vec![self.state_word()?], false)
        } else if self.keyword("in") {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::Open,
                    ..
                }) => self.at += 1,
                _ => return Err(self.unexpected("'(' opening a list")),
            }
            let mut words = vec![self.state_word()?];
            loop {
                match self.peek().map(|token| &token.kind) {
                    Some(TokenKind::Comma) => {
                        self.at += 1;
                        words.push(self.state_word()?);
                    }
                    Some(TokenKind::Close) => {
                        self.at += 1;
                        break;
                    }
                    _ => return Err(self.unexpected("',' or ')'")),
                }
            }
            (words, false)
        } else if self
            .word()
            .is_some_and(|word| word.eq_ignore_ascii_case("is"))
        {
            return Err(refused("is"));
        } else {
            match self.operator()? {
                Operator::Eq => (vec![self.state_word()?], false),
                Operator::Ne => (vec![self.state_word()?], true),
                other => return Err(refused(other.as_str())),
            }
        };
        for word in &words {
            if !self.states.contains(word) {
                self.states.push(*word);
            }
        }
        Ok(Expr::State { words, negated })
    }

    /// One of `state`'s words, bare or quoted; any other is refused with the
    /// nearest and every word.
    fn state_word(&mut self) -> Result<StateWord, FilterError> {
        let Some(token) = self.peek() else {
            return Err(self.unexpected("a state"));
        };
        let written = match &token.kind {
            TokenKind::Word(word) | TokenKind::Quoted(word) => word.clone(),
            _ => return Err(self.unexpected("a state")),
        };
        let word = StateWord::parse(&written).ok_or_else(|| FilterError::UnknownState {
            position: token.at,
            suggestion: StateWord::suggestion(&written),
            word: written.clone(),
        })?;
        self.at += 1;
        Ok(word)
    }

    fn membership(&mut self, field: Field) -> Result<Expr, FilterError> {
        match self.peek() {
            Some(Token {
                kind: TokenKind::Open,
                ..
            }) => self.at += 1,
            _ => return Err(self.unexpected("'(' opening a list")),
        }
        let mut operands = vec![self.operand()?];
        loop {
            match self.peek() {
                Some(Token {
                    kind: TokenKind::Comma,
                    ..
                }) => {
                    self.at += 1;
                    operands.push(self.operand()?);
                }
                Some(Token {
                    kind: TokenKind::Close,
                    ..
                }) => {
                    self.at += 1;
                    return Ok(Expr::Membership { field, operands });
                }
                _ => return Err(self.unexpected("',' or ')'")),
            }
        }
    }

    fn operator(&mut self) -> Result<Operator, FilterError> {
        if let Some(TokenKind::Symbol(symbol)) = self.peek().map(|token| &token.kind) {
            let operator = match symbol.as_str() {
                "==" => Operator::Eq,
                "!=" => Operator::Ne,
                ">" => Operator::Gt,
                ">=" => Operator::Ge,
                "<" => Operator::Lt,
                "<=" => Operator::Le,
                _ => return Err(self.unexpected("a comparison operator")),
            };
            self.at += 1;
            return Ok(operator);
        }
        if self.keyword("contains") {
            return Ok(Operator::Contains);
        }
        if self.keyword("starts_with") {
            return Ok(Operator::StartsWith);
        }
        Err(self.unexpected("an operator"))
    }

    fn operand(&mut self) -> Result<Value, FilterError> {
        let Some(token) = self.peek() else {
            return Err(self.unexpected("a value"));
        };
        let value = match &token.kind {
            // Quoting escapes back to text, which is what an attribute
            // genuinely holding the word `true` needs; a quoted date is still a
            // date.
            TokenKind::Quoted(text) => quoted(text),
            // `1e999` is a number no value can hold, never the text `1e999`.
            TokenKind::Word(word)
                if word
                    .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
                    && word.parse::<f64>().is_ok_and(|number| !number.is_finite()) =>
            {
                return Err(self.unexpected("a finite number"));
            }
            TokenKind::Word(word) => literal(word),
            _ => return Err(self.unexpected("a value")),
        };
        self.at += 1;
        Ok(value)
    }
}

/// Four literal forms are recognized before the bare-word rule, one per scalar
/// kind that is not text. Without them, `cold_crashed == true` would compare
/// `Text("true")` against `Boolean(true)` and silently answer *no sample is
/// cold_crashed* on a collection where every one is.
fn literal(word: &str) -> Value {
    if let Ok(integer) = word.parse::<i64>() {
        return Value::integer(integer);
    }
    if let Ok(number) = word.parse::<f64>()
        && let Ok(value) = Value::number(number)
    {
        return value;
    }
    match word {
        "true" => return Value::boolean(true),
        "false" => return Value::boolean(false),
        _ => {}
    }
    if let Ok(date) = crate::core::value::Date::parse(word) {
        return Value::date(date);
    }
    if let Ok(date_time) = crate::core::value::DateTime::parse(word) {
        return Value::date_time(date_time);
    }
    Value::text(word)
}

/// A quoted operand: text, unless it is a date or a date-time.
fn quoted(text: &str) -> Value {
    if let Ok(date) = crate::core::value::Date::parse(text) {
        return Value::date(date);
    }
    if let Ok(date_time) = crate::core::value::DateTime::parse(text) {
        return Value::date_time(date_time);
    }
    Value::text(text)
}

// -------------------------------------------------------------- evaluation

/// The fields a filter references, known before anything is evaluated — which
/// is what lets a caller resolve only what a query needs, a real cost when
/// resolving a field means running a Python formula.
pub fn fields(filter: &Filter) -> &[Field] {
    &filter.fields
}

/// The fields a filter compares with a number, or with another field: those
/// whose unit matters, since `volume > 20` means nothing over litres and
/// millilitres. A presence test, a tag or a text asked for is none.
pub fn numbers_compared(filter: &Filter) -> Vec<Field> {
    fn walk(expr: &Expr, out: &mut Vec<Field>) {
        match expr {
            Expr::Or(left, right) | Expr::And(left, right) => {
                walk(left, out);
                walk(right, out);
            }
            Expr::Not(inner) => walk(inner, out),
            Expr::Compare {
                field,
                operand: Value::Number(_) | Value::Integer(_),
                ..
            } => {
                out.push(field.clone());
            }
            Expr::CompareFields { field, other, .. } => {
                out.push(field.clone());
                out.push(other.clone());
            }
            Expr::Membership { field, operands }
                if operands
                    .iter()
                    .any(|operand| matches!(operand, Value::Number(_) | Value::Integer(_))) =>
            {
                out.push(field.clone());
            }
            // A row's names that are the sample's fields; its columns are
            // `columns_compared`'s.
            Expr::Where { condition, .. } => walk(condition, out),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&filter.root, &mut out);
    out
}

/// The whole columns a filter compares with a number — `t.c has 3`, a name
/// of `t where (c > 3)` — as `(table, column)`: a column's unit must agree
/// across the samples as a field's must. A name of a row no column holds is
/// left to `check`, which says it.
pub fn columns_compared(filter: &Filter) -> Vec<(Identifier, Identifier)> {
    fn walk(expr: &Expr, out: &mut Vec<(Identifier, Identifier)>) {
        match expr {
            Expr::Or(left, right) | Expr::And(left, right) => {
                walk(left, out);
                walk(right, out);
            }
            Expr::Not(inner) => walk(inner, out),
            Expr::TableHas {
                table,
                column: Some(column),
                operand: Value::Number(_) | Value::Integer(_),
            } => out.push((table.clone(), column.clone())),
            Expr::Where { table, condition } => {
                let mut inner = Vec::new();
                numbers_walk(condition, &mut inner);
                for field in inner {
                    if let Field::Named { name, .. } = field {
                        out.push((table.clone(), name));
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&filter.root, &mut out);
    out
}

/// `numbers_compared`'s walk, over one expression.
fn numbers_walk(expr: &Expr, out: &mut Vec<Field>) {
    let filter = Filter {
        root: expr.clone(),
        fields: Vec::new(),
        tags: Vec::new(),
        states: Vec::new(),
    };
    out.extend(numbers_compared(&filter));
}

/// The words a filter asks of `state`, empty where it names no `state`.
pub fn state_words(filter: &Filter) -> &[StateWord] {
    &filter.states
}

/// One sample's answer, in three values. The collapse to a yes-or-no is the
/// caller's, which is the layer that knows whether to report the unknowns.
pub fn evaluate(filter: &Filter, subject: &Subject) -> Result<Truth, FilterError> {
    evaluate_expr(&filter.root, subject)
}

/// The same, with a comparison the field's text cannot answer read as unknown,
/// as an absent value is: the sample is left aside, never selected, even under
/// `!` — what [`Diagnostics::set_aside`] names. A conflict of the operand's own
/// making, `radius == high` over numbers, is still one.
pub fn evaluate_leaving_aside(filter: &Filter, subject: &Subject) -> Result<Truth, FilterError> {
    evaluate_in(&filter.root, subject, true)
}

fn evaluate_expr(expr: &Expr, subject: &Subject) -> Result<Truth, FilterError> {
    evaluate_in(expr, subject, false)
}

fn evaluate_in(expr: &Expr, subject: &Subject, leaving_aside: bool) -> Result<Truth, FilterError> {
    let evaluate_expr = |expr: &Expr, subject: &Subject| evaluate_in(expr, subject, leaving_aside);
    match expr {
        Expr::Or(left, right) => {
            Ok(evaluate_expr(left, subject)?.or(evaluate_expr(right, subject)?))
        }
        Expr::And(left, right) => {
            Ok(evaluate_expr(left, subject)?.and(evaluate_expr(right, subject)?))
        }
        Expr::Not(inner) => Ok(evaluate_expr(inner, subject)?.not()),
        Expr::LinksTo { field, target } => Ok(links_to(subject, field.as_ref(), target)),
        Expr::State { words, negated } => {
            let states = subject
                .states
                .ok_or_else(|| FilterError::Field(Box::new(FieldError::StatesNotRead)))?;
            // Any word held is enough; every word denied is a no; a word only
            // the unread model could deny leaves it unknown.
            let mut truth = Truth::False;
            for word in words {
                truth = truth.or(match states.answers(*word) {
                    Some(true) => Truth::True,
                    Some(false) => Truth::False,
                    None => Truth::Unknown,
                });
            }
            Ok(if *negated { truth.not() } else { truth })
        }
        Expr::Presence { field, present } => {
            let resolved = resolve(field, subject)?;
            let there = match &resolved {
                Resolution::Scalar(value) => value.is_some(),
                Resolution::List(values) => !values.is_empty(),
                Resolution::Tags(tags) => !tags.is_empty(),
            };
            // The only predicate that is true on absence, which is why absence
            // is askable at all.
            Ok(if there == *present {
                Truth::True
            } else {
                Truth::False
            })
        }
        Expr::TableHas {
            table,
            column,
            operand,
        } => table_has(table, column.as_ref(), operand, subject),
        // A name of the table's columns is that row's cell; any row meeting
        // the condition is enough.
        Expr::Where { table, condition } => {
            // A name neither the table's column nor the sample's is said a
            // column of the table, the nearest offered: `atmospher` was an
            // unknown field among forty others.
            if subject.vocabulary.has_table(table) {
                for name in row_names(condition) {
                    if !subject.vocabulary.has_column(table, name)
                        && !subject.vocabulary.has_name(name)
                        && !subject.vocabulary.has_table(name)
                    {
                        let available = subject.vocabulary.columns_as_strings(table);
                        return Err(FilterError::Field(Box::new(FieldError::UnknownColumn {
                            table: table.clone(),
                            name: name.to_string(),
                            suggestion: crate::core::identifier::nearest(
                                name.as_str(),
                                available.iter().map(String::as_str),
                            ),
                            available,
                        })));
                    }
                }
            }
            let rows = match subject.sample.table(table) {
                Ok(held) => held.rows().count(),
                Err(_) => return absent_table(table, subject),
            };
            let mut truth = Truth::False;
            for position in 0..rows {
                let row = in_row(condition, table, position, subject);
                truth = truth.or(evaluate_expr(&row, subject)?);
                if truth == Truth::True {
                    break;
                }
            }
            Ok(truth)
        }
        // `printing has argon`: a table's every cell.
        Expr::HasTag {
            field:
                Field::Named {
                    name,
                    channel: fields::Channel::Value,
                },
            operand,
        } if subject.vocabulary.has_table(name) => table_has(name, None, operand, subject),
        Expr::HasTag { field, operand } => match resolve(field, subject)? {
            Resolution::Tags(tags) => Ok(
                if matches!(operand, Value::Text(text) if Identifier::new(text).is_ok_and(|tag| tags.contains(&tag)))
                {
                    Truth::True
                } else {
                    Truth::False
                },
            ),
            Resolution::List(values) => {
                for value in &values {
                    if equals_checked(field, value, operand, "has")? {
                        return Ok(Truth::True);
                    }
                }
                Ok(Truth::False)
            }
            // `has` applies to `tags` and to nothing else. Overloading one
            // operator with two meanings by field type would put a substring
            // test and an exact test a keystroke apart.
            // A sample without the list does not hold the item: unknown, as any
            // absent field is.
            Resolution::Scalar(None) => Ok(Truth::Unknown),
            Resolution::Scalar(Some(held)) => Err(FilterError::WrongOperator {
                field: Box::new(field.clone()),
                operator: "has".to_string(),
                // `contains` is the text's; a number or a date has neither.
                reason: if matches!(held, Value::Text(_)) {
                    "'has' tests membership and applies to tags or a list attribute: for text, \
                     contains"
                        .to_string()
                } else {
                    "'has' tests membership and applies to tags or a list attribute".to_string()
                },
            }),
        },
        Expr::Membership { field, operands } => {
            let Some(left) = scalar(field, subject)? else {
                return Ok(Truth::Unknown);
            };
            for operand in operands {
                if equals_checked(field, &left, operand, "in")? {
                    return Ok(Truth::True);
                }
            }
            Ok(Truth::False)
        }
        Expr::CompareFields {
            field,
            operator,
            other,
            word,
        } => {
            let equality = matches!(operator, Operator::Eq | Operator::Ne);
            // `style == titanium` beside no field named titanium: the text, as ever.
            let right = match scalar(other, subject) {
                Err(FilterError::Field(_)) if equality => {
                    let as_text = Expr::Compare {
                        field: field.clone(),
                        operator: *operator,
                        operand: Value::text(word.clone()),
                    };
                    return evaluate_expr(&as_text, subject);
                }
                answer => answer?,
            };
            let Some(left) = scalar(field, subject)? else {
                return Ok(Truth::Unknown);
            };
            let Some(right) = right else {
                return Ok(Truth::Unknown);
            };
            // Litres against kilograms compare nothing: refused, as a table
            // refuses two units in a column.
            if let (Some(this), Some(that)) = (unit_of(field, subject), unit_of(other, subject))
                && this != that
            {
                return Err(FilterError::UnitsDiffer {
                    left: Box::new(field.clone()),
                    right: Box::new(other.clone()),
                    units: (this, that),
                });
            }
            compare(field, &left, *operator, &right)
        }
        Expr::Compare {
            field,
            operator,
            operand,
        } => {
            let Some(left) = scalar(field, subject)? else {
                return Ok(Truth::Unknown);
            };
            match compare(field, &left, *operator, operand) {
                Err(FilterError::TypeConflict(_))
                    if leaving_aside && matches!(left, Value::Text(_)) =>
                {
                    Ok(Truth::Unknown)
                }
                answer => answer,
            }
        }
    }
}

/// Whether a cell of `table` — of `column`, or of any — holds `operand`. A
/// sample without the table answers unknown; a table or a column no sample
/// holds is an error.
fn table_has(
    table: &Identifier,
    column: Option<&Identifier>,
    operand: &Value,
    subject: &Subject,
) -> Result<Truth, FilterError> {
    if let Some(column) = column
        && !subject.vocabulary.has_column(table, column)
        && subject.vocabulary.has_table(table)
    {
        let available = subject.vocabulary.columns_as_strings(table);
        return Err(FilterError::Field(Box::new(FieldError::UnknownColumn {
            table: table.clone(),
            name: column.to_string(),
            suggestion: crate::core::identifier::nearest(
                column.as_str(),
                available.iter().map(String::as_str),
            ),
            available,
        })));
    }
    let held = match subject.sample.table(table) {
        Ok(held) => held,
        Err(_) => return absent_table(table, subject),
    };
    // A column another sample's table holds and this one's does not is
    // unknown here, as an absent field is: answered *no*, its negation
    // selected a sample nothing was known of.
    if let Some(column) = column
        && !held.column_names().into_iter().any(|held| held == column)
    {
        return Ok(Truth::Unknown);
    }
    let column_named = column.is_some();
    let columns: Vec<Identifier> = match column {
        Some(column) => vec![column.clone()],
        None => held.column_names().into_iter().cloned().collect(),
    };
    let rows = held.rows().count();
    for column in &columns {
        for position in 0..rows {
            let cell = Field::Cell {
                table: table.clone(),
                column: column.clone(),
                row: crate::core::table::RowAddress::Ordinal(position),
                channel: fields::Channel::Value,
            };
            // A formula that failed is never an absence: said, as everywhere.
            // A whole table's columns hold text and numbers both: each cell
            // of another kind than the operand is no match; one column named
            // is asked as `in` is.
            let Some(value) = scalar(&cell, subject)? else {
                continue;
            };
            let equal = if column_named {
                equals_checked(&cell, &value, operand, "has")?
            } else {
                predicate_equals(&value, operand)
            };
            if equal {
                return Ok(Truth::True);
            }
        }
    }
    Ok(Truth::False)
}

/// A table this sample does not hold: unknown where another sample holds it,
/// an error where none does.
fn absent_table(table: &Identifier, subject: &Subject) -> Result<Truth, FilterError> {
    if subject.vocabulary.has_table(table) {
        return Ok(Truth::Unknown);
    }
    let available = subject.vocabulary.tables_as_strings();
    Err(FilterError::Field(Box::new(FieldError::UnknownTable {
        name: table.to_string(),
        suggestion: crate::core::identifier::nearest(
            table.as_str(),
            available.iter().map(String::as_str),
        ),
        available,
    })))
}

/// The names a row's condition reads: those of its fields, not a bare word
/// after `==` that may be a text.
fn row_names(condition: &Expr) -> Vec<&Identifier> {
    let mut names = Vec::new();
    fn walk<'a>(expr: &'a Expr, names: &mut Vec<&'a Identifier>) {
        let mut push = |field: &'a Field| {
            if let Field::Named { name, .. } = field {
                names.push(name);
            }
        };
        match expr {
            Expr::Or(left, right) | Expr::And(left, right) => {
                walk(left, names);
                walk(right, names);
            }
            Expr::Not(inner) => walk(inner, names),
            Expr::Compare { field, .. }
            | Expr::Membership { field, .. }
            | Expr::Presence { field, .. }
            | Expr::HasTag { field, .. } => push(field),
            Expr::CompareFields {
                field,
                operator,
                other,
                ..
            } => {
                push(field);
                if !matches!(operator, Operator::Eq | Operator::Ne) {
                    push(other);
                }
            }
            _ => {}
        }
    }
    walk(condition, &mut names);
    names
}

/// `condition` asked of one row of `table`: each name of the table's columns
/// that row's cell, every other name the sample's.
fn in_row(condition: &Expr, table: &Identifier, position: usize, subject: &Subject) -> Expr {
    let cell = |field: &Field| match field {
        Field::Named { name, channel } if subject.vocabulary.has_column(table, name) => {
            Field::Cell {
                table: table.clone(),
                column: name.clone(),
                row: crate::core::table::RowAddress::Ordinal(position),
                channel: *channel,
            }
        }
        other => other.clone(),
    };
    let inner = |expr: &Expr| Box::new(in_row(expr, table, position, subject));
    match condition {
        Expr::Or(left, right) => Expr::Or(inner(left), inner(right)),
        Expr::And(left, right) => Expr::And(inner(left), inner(right)),
        Expr::Not(expr) => Expr::Not(inner(expr)),
        Expr::Compare {
            field,
            operator,
            operand,
        } => Expr::Compare {
            field: cell(field),
            operator: *operator,
            operand: operand.clone(),
        },
        Expr::CompareFields {
            field,
            operator,
            other,
            word,
        } => Expr::CompareFields {
            field: cell(field),
            operator: *operator,
            other: cell(other),
            word: word.clone(),
        },
        Expr::Membership { field, operands } => Expr::Membership {
            field: cell(field),
            operands: operands.clone(),
        },
        Expr::Presence { field, present } => Expr::Presence {
            field: cell(field),
            present: *present,
        },
        Expr::HasTag { field, operand } => Expr::HasTag {
            field: cell(field),
            operand: operand.clone(),
        },
        other => other.clone(),
    }
}

/// The unit a field's value is in, where the sample says one: a property's,
/// or a cell's column.
fn unit_of(field: &Field, subject: &Subject) -> Option<String> {
    match field {
        Field::Named { name, .. } => subject
            .sample
            .property(name)
            .ok()
            .and_then(|handle| handle.presentation().unit),
        Field::Cell { table, column, .. } => subject
            .sample
            .table(table)
            .ok()
            .and_then(|held| held.column(column).ok())
            .and_then(|view| view.presentation().unit.clone()),
        _ => None,
    }
}

fn resolve(field: &Field, subject: &Subject) -> Result<Resolution, FilterError> {
    // Not applicable is no value to a filter, as absence is.
    fields::resolve(field, subject)
        .map(|resolution| match resolution {
            Resolution::Scalar(Some(value)) if value.is_not_applicable() => {
                Resolution::Scalar(None)
            }
            other => other,
        })
        .map_err(|e| FilterError::Field(Box::new(e)))
}

/// `None` is *absent*, which becomes unknown. A tag set reached by a scalar
/// operator is a mistake and says which operator to use instead.
fn scalar(field: &Field, subject: &Subject) -> Result<Option<Value>, FilterError> {
    match resolve(field, subject)? {
        Resolution::Scalar(value) => Ok(value),
        Resolution::Tags(_) => Err(FilterError::WrongOperator {
            field: Box::new(field.clone()),
            operator: "a scalar operator".to_string(),
            reason: "'tags' holds several identifiers; use 'has' to test membership".to_string(),
        }),
        Resolution::List(_) => Err(FilterError::WrongOperator {
            field: Box::new(field.clone()),
            operator: "a scalar operator".to_string(),
            reason: format!(
                "'{}' holds a list; use 'has' to test membership, or [#n] for one item",
                fields::describe(field)
            ),
        }),
    }
}

fn compare(
    field: &Field,
    left: &Value,
    operator: Operator,
    operand: &Value,
) -> Result<Truth, FilterError> {
    let truth = |yes: bool| Ok(if yes { Truth::True } else { Truth::False });
    // A comparison that cannot succeed is reported with both sides named: the
    // field's kind and the operand, because `diameter > "heavy"` needs to say which
    // half is wrong.
    let conflict = |_: ValueError| {
        FilterError::TypeConflict(Box::new(TypeConflict {
            field: field.clone(),
            operator: operator.as_str().to_string(),
            operand: operand.clone(),
            field_kind: left.kind(),
        }))
    };
    match operator {
        // A number is never text: asking whether one equals the other is a
        // mistake, and said, whichever side the number is on.
        Operator::Eq | Operator::Ne
            if (matches!(left, Value::Number(_) | Value::Integer(_))
                && matches!(operand, Value::Text(_)))
                || (matches!(left, Value::Text(_))
                    && matches!(operand, Value::Number(_) | Value::Integer(_))) =>
        {
            Err(conflict(ValueError::NotText { found: left.kind() }))
        }
        Operator::Eq => truth(predicate_equals(left, operand)),
        Operator::Ne => truth(!predicate_equals(left, operand)),
        Operator::Contains | Operator::StartsWith => {
            let needle = match operand {
                Value::Text(text) => text.clone(),
                other => {
                    return Err(conflict(ValueError::NotText {
                        found: other.kind(),
                    }));
                }
            };
            let answer = match operator {
                Operator::Contains => value::contains(left, &needle),
                _ => value::starts_with(left, &needle),
            };
            truth(answer.map_err(conflict)?)
        }
        _ => {
            // Text orders without regard to case, as a sort does. The sort's
            // tie-break on the written case orders two spellings of one word; a
            // predicate asks nothing of it.
            let ordering = match (left, operand) {
                (Value::Text(this), Value::Text(that)) => {
                    this.to_lowercase().cmp(&that.to_lowercase())
                }
                _ => value::compare(left, operand).map_err(conflict)?,
            };
            truth(match operator {
                Operator::Gt => ordering.is_gt(),
                Operator::Ge => ordering.is_ge(),
                Operator::Lt => ordering.is_lt(),
                _ => ordering.is_le(),
            })
        }
    }
}

/// Equality as `in` and `has` ask it: a number is never text, and asking
/// whether one is the other is a mistake said, as `==` says it — answered
/// *no*, `radius in (high, low)` selected nothing, and its negation everything.
fn equals_checked(
    field: &Field,
    left: &Value,
    operand: &Value,
    operator: &str,
) -> Result<bool, FilterError> {
    let number = |value: &Value| matches!(value, Value::Number(_) | Value::Integer(_));
    let text = |value: &Value| matches!(value, Value::Text(_));
    if (number(left) && text(operand)) || (text(left) && number(operand)) {
        return Err(FilterError::TypeConflict(Box::new(TypeConflict {
            field: field.clone(),
            operator: operator.to_string(),
            operand: operand.clone(),
            field_kind: left.kind(),
        })));
    }
    Ok(predicate_equals(left, operand))
}

/// Predicate equality has one domain-specific widening that value identity
/// cannot have without violating `Eq`: a date operand denotes its whole day.
fn predicate_equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Date(_), Value::DateTime(_)) | (Value::DateTime(_), Value::Date(_)) => {
            value::compare(left, right).is_ok_and(|ordering| ordering.is_eq())
        }
        _ => value::equals(left, right),
    }
}

impl SetAside {
    /// The one sentence every surface says it in: *b left aside: 'radius' holds
    /// text there, which > cannot compare*.
    pub fn said(&self) -> String {
        let mut names: Vec<&str> = self.samples.iter().take(5).map(String::as_str).collect();
        let more = self.samples.len().saturating_sub(names.len());
        let more = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        names.dedup();
        format!(
            "{}{more} left aside: '{}' holds text there, which {} cannot compare",
            names.join(", "),
            fields::describe(&self.field),
            self.operator
        )
    }
}

/// What one comparison met over the collection: whether a sample answered
/// it, and the samples whose text it could not be asked of.
#[derive(Default)]
struct Met {
    answered: bool,
    conflict: Option<TypeConflict>,
    text_in: Vec<String>,
}

/// Walk every predicate of the tree and record what each one cannot do,
/// without letting the first failure hide the rest.
fn inspect(
    expr: &Expr,
    subject: &Subject,
    diagnostics: &mut Diagnostics,
    met: &mut Vec<(*const Expr, Met)>,
) {
    match expr {
        Expr::Or(left, right) | Expr::And(left, right) => {
            inspect(left, subject, diagnostics, met);
            inspect(right, subject, diagnostics, met);
        }
        Expr::Not(inner) => inspect(inner, subject, diagnostics, met),
        leaf => match evaluate_expr(leaf, subject) {
            Ok(truth) => {
                // Absent answers nothing: only a sample that held a value
                // shows the comparison can be asked here.
                if truth != Truth::Unknown {
                    met_at(met, leaf).answered = true;
                }
            }
            // The states are read by the caller that evaluates, never by a
            // check, which judges the filter against the collection's names.
            Err(FilterError::Field(error)) if *error == FieldError::StatesNotRead => {}
            Err(FilterError::Field(error)) => {
                if !diagnostics.unknown_fields.contains(&error) {
                    diagnostics.unknown_fields.push(*error);
                }
            }
            // Held until the whole collection is walked: text where other
            // samples hold numbers is set aside, text everywhere is a question
            // that cannot be asked.
            Err(FilterError::TypeConflict(conflict)) if conflict.field_kind == ValueKind::Text => {
                let entry = met_at(met, leaf);
                entry
                    .text_in
                    .push(subject.sample.name().unwrap_or("?").to_string());
                entry.conflict.get_or_insert(*conflict);
            }
            Err(FilterError::TypeConflict(conflict)) => {
                if !diagnostics.type_conflicts.contains(&conflict) {
                    diagnostics.type_conflicts.push(*conflict);
                }
            }
            // A wrong operator is a fact about the filter, not about the
            // collection, and `parse` cannot see it: report it once.
            Err(FilterError::WrongOperator {
                field,
                operator,
                reason,
            }) => {
                // Kept as it is: its reason names the operator to use, which a
                // type conflict of nothing against nothing would lose.
                let misuse = OperatorMisuse {
                    field: *field,
                    operator,
                    reason,
                };
                if !diagnostics.wrong_operators.contains(&misuse) {
                    diagnostics.wrong_operators.push(misuse);
                }
            }
            Err(_) => {}
        },
    }
}

/// The texts `root` compares with that no sample holds for their field and
/// one holds a text near: a list of the empty answer's likely cause. Text
/// matched exactly somewhere, or near nothing held, is no warning.
fn unmatched_texts(
    root: &Expr,
    collection: &[&Sample],
    vocabulary: &fields::Vocabulary,
    diagnostics: &mut Diagnostics,
) {
    let mut asked: Vec<(&Field, &str)> = Vec::new();
    // A bare word after `==` naming a field no sample holds is the text it
    // always was, and is asked about as text.
    let mut either: Vec<(&Field, &Field, &str)> = Vec::new();
    // A text asked of a table's cells: the table, its column or any, the text.
    let mut cells: Vec<(&Identifier, Option<&Identifier>, &str)> = Vec::new();
    fn gather<'a>(
        expr: &'a Expr,
        asked: &mut Vec<(&'a Field, &'a str)>,
        either: &mut Vec<(&'a Field, &'a Field, &'a str)>,
        cells: &mut Vec<(&'a Identifier, Option<&'a Identifier>, &'a str)>,
        row: Option<&'a Identifier>,
    ) {
        match expr {
            Expr::Or(left, right) | Expr::And(left, right) => {
                gather(left, asked, either, cells, row);
                gather(right, asked, either, cells, row);
            }
            Expr::Not(inner) => gather(inner, asked, either, cells, row),
            Expr::TableHas {
                table,
                column,
                operand: Value::Text(text),
            } => cells.push((table, column.as_ref(), text)),
            Expr::Where { table, condition } => {
                gather(condition, asked, either, cells, Some(table));
            }
            // Inside `where`, a name may be the row's column.
            Expr::Compare {
                field: Field::Named { name, .. },
                operator: Operator::Eq | Operator::Ne,
                operand: Value::Text(text),
            } if row.is_some() => cells.push((row.expect("just matched"), Some(name), text)),
            // `atmosphere == aire`: a bare word that may be a text.
            Expr::CompareFields {
                field: Field::Named { name, .. },
                operator: Operator::Eq | Operator::Ne,
                word,
                ..
            } if row.is_some() => cells.push((row.expect("just matched"), Some(name), word)),
            Expr::CompareFields {
                field,
                operator: Operator::Eq | Operator::Ne,
                other,
                word,
            } => either.push((field, other, word)),
            Expr::Compare {
                field,
                operator: Operator::Eq | Operator::Ne,
                operand: Value::Text(text),
            } => asked.push((field, text)),
            Expr::Membership { field, operands } => {
                asked.extend(operands.iter().filter_map(|operand| match operand {
                    Value::Text(text) => Some((field, text.as_str())),
                    _ => None,
                }))
            }
            Expr::HasTag {
                field,
                operand: Value::Text(text),
            } => asked.push((field, text)),
            _ => {}
        }
    }
    gather(root, &mut asked, &mut either, &mut cells, None);
    // `carbon has citr`: a table, which only the collection can tell.
    asked.retain(|(field, text)| match field {
        Field::Named {
            name,
            channel: fields::Channel::Value,
        } if vocabulary.has_table(name) => {
            cells.push((name, None, text));
            false
        }
        _ => true,
    });
    // `printing has argn`: the texts the table's cells hold, nearest first.
    for (table, column, written) in cells {
        if let Some(column) = column
            && !vocabulary.has_column(table, column)
        {
            continue;
        }
        let mut held: Vec<String> = Vec::new();
        let name = match column {
            Some(column) => format!("{table}.{column}"),
            None => table.to_string(),
        };
        for sample in collection {
            let subject = Subject {
                sample,
                path: None,
                vocabulary,
                states: None,
            };
            for written in cells_of(Some(&name), &subject).unwrap_or_default() {
                let text = written.trim_matches('"').to_string();
                if !held.contains(&text) {
                    held.push(text);
                }
            }
        }
        if held.is_empty() || held.iter().any(|text| text == written) {
            continue;
        }
        let suggestion = held
            .iter()
            .find(|text| text.eq_ignore_ascii_case(written))
            .cloned()
            .or_else(|| crate::core::identifier::nearest(written, held.iter().map(String::as_str)));
        if let Some(suggestion) = suggestion {
            let warning = TextWarning {
                field: name.clone(),
                written: written.to_string(),
                suggestion,
            };
            if !diagnostics.unmatched_texts.contains(&warning) {
                diagnostics.unmatched_texts.push(warning);
            }
        }
    }
    for (field, other, word) in either {
        let held = collection.iter().any(|sample| {
            let subject = Subject {
                sample,
                path: None,
                vocabulary,
                states: None,
            };
            matches!(scalar(other, &subject), Ok(Some(_)))
        });
        if !held {
            asked.push((field, word));
        }
    }
    for (field, written) in asked {
        let mut held: Vec<String> = Vec::new();
        for sample in collection {
            let subject = Subject {
                sample,
                path: None,
                vocabulary,
                states: None,
            };
            let values = match fields::resolve(field, &subject) {
                Ok(fields::Resolution::Scalar(Some(value))) => vec![value],
                Ok(fields::Resolution::List(values)) => values,
                _ => Vec::new(),
            };
            for value in values {
                if let Value::Text(text) = value
                    && !held.contains(&text)
                {
                    held.push(text);
                }
            }
        }
        if held.is_empty() || held.iter().any(|text| text == written) {
            continue;
        }
        let suggestion = held
            .iter()
            .find(|text| text.eq_ignore_ascii_case(written))
            .cloned()
            .or_else(|| crate::core::identifier::nearest(written, held.iter().map(String::as_str)));
        if let Some(suggestion) = suggestion {
            let warning = TextWarning {
                field: fields::describe(field),
                written: written.to_string(),
                suggestion,
            };
            if !diagnostics.unmatched_texts.contains(&warning) {
                diagnostics.unmatched_texts.push(warning);
            }
        }
    }
}

/// The numbers `root` asks equality of that no sample holds, where one holds a
/// number that, rounded to as many decimals as the number written, is it:
/// what a table shows was typed, and matched nothing.
fn unmatched_numbers(
    root: &Expr,
    collection: &[&Sample],
    vocabulary: &fields::Vocabulary,
    diagnostics: &mut Diagnostics,
) {
    fn gather<'a>(expr: &'a Expr, asked: &mut Vec<(&'a Field, f64)>) {
        match expr {
            Expr::Or(left, right) | Expr::And(left, right) => {
                gather(left, asked);
                gather(right, asked);
            }
            Expr::Not(inner) => gather(inner, asked),
            Expr::Compare {
                field,
                operator: Operator::Eq,
                operand,
            } => match operand {
                Value::Number(number) => asked.push((field, *number)),
                Value::Integer(number) => asked.push((field, *number as f64)),
                _ => {}
            },
            _ => {}
        }
    }
    let mut asked = Vec::new();
    gather(root, &mut asked);
    for (field, written) in asked {
        // As many decimals as the number written, read back from its
        // shortest form.
        let shown = format!("{written}");
        let decimals = shown.split_once('.').map_or(0, |(_, after)| after.len()) as i32;
        let half = 0.5 * 10f64.powi(-decimals) * (1.0 + 1e-9);
        let mut exact = false;
        let mut near: Option<f64> = None;
        for sample in collection {
            let subject = Subject {
                sample,
                path: None,
                vocabulary,
                states: None,
            };
            let held = match fields::resolve(field, &subject) {
                Ok(fields::Resolution::Scalar(Some(Value::Number(held)))) => held,
                Ok(fields::Resolution::Scalar(Some(Value::Integer(held)))) => held as f64,
                _ => continue,
            };
            if held == written {
                exact = true;
                break;
            }
            if near.is_none() && (held - written).abs() <= half {
                near = Some(held);
            }
        }
        if exact {
            continue;
        }
        if let Some(held) = near {
            let name = fields::describe(field);
            let low = written - 0.5 * 10f64.powi(-decimals);
            let high = written + 0.5 * 10f64.powi(-decimals);
            let digits = (decimals + 1).max(0) as usize;
            let warning = format!(
                "no sample has {name} == {shown}; one holds {held}, shown {shown} rounded — \
                 {name} >= {low:.digits$} && {name} < {high:.digits$} asks for those"
            );
            if !diagnostics.unmatched_numbers.contains(&warning) {
                diagnostics.unmatched_numbers.push(warning);
            }
        }
    }
}

/// Every problem across the whole collection, in one pass: unknown fields,
/// misspellings with suggestions, and comparisons that cannot succeed anywhere.
pub fn check(filter: &Filter, collection: &[&Sample]) -> Diagnostics {
    let mut diagnostics = Diagnostics::default();
    let vocabulary = fields::vocabulary_of(collection);
    let mut met: Vec<(*const Expr, Met)> = Vec::new();
    for sample in collection {
        let subject = Subject {
            sample,
            path: None,
            vocabulary: &vocabulary,
            states: None,
        };
        // Every predicate, not the expression: evaluation stops at the first
        // failure, and a user fixing a filter one error per run is a user
        // running the command five times.
        inspect(&filter.root, &subject, &mut diagnostics, &mut met);
    }
    for (_, met) in met {
        let Some(conflict) = met.conflict else {
            continue;
        };
        if met.answered {
            // Two comparisons of one field over the same samples are one
            // sentence: `radius < 2 && radius > 1` said b twice.
            match diagnostics
                .set_aside
                .iter_mut()
                .find(|aside| aside.field == conflict.field && aside.samples == met.text_in)
            {
                Some(aside)
                    if !aside
                        .operator
                        .split(" and ")
                        .any(|held| held == conflict.operator) =>
                {
                    aside.operator = format!("{} and {}", aside.operator, conflict.operator);
                }
                Some(_) => {}
                None => diagnostics.set_aside.push(SetAside {
                    field: conflict.field,
                    operator: conflict.operator,
                    samples: met.text_in,
                }),
            }
        } else if !diagnostics.type_conflicts.contains(&conflict) {
            diagnostics.type_conflicts.push(conflict);
        }
    }
    // A row or a list position one sample lacks is absent there; one that no
    // sample holds is a mistake.
    for field in &filter.fields {
        if let Some(error) = fields::held_nowhere(field, collection)
            && !diagnostics.unknown_fields.contains(&error)
        {
            diagnostics.unknown_fields.push(error);
        }
    }
    unmatched_texts(&filter.root, collection, &vocabulary, &mut diagnostics);
    unmatched_numbers(&filter.root, collection, &vocabulary, &mut diagnostics);
    for tag in &filter.tags {
        if !vocabulary.has_tag(tag) {
            diagnostics.unknown_tags.push(TagWarning {
                suggestion: vocabulary.nearest_tag(tag.as_str()),
                tag: tag.clone(),
            });
        }
    }
    // A filter over an empty collection still has its names checked, against
    // an empty vocabulary: every field it names is unknown.
    if collection.is_empty() {
        for field in &filter.fields {
            if let Field::Named { name, .. } = field {
                diagnostics
                    .unknown_fields
                    .push(FieldError::UnknownProperty {
                        name: name.to_string(),
                        available: Vec::new(),
                        suggestion: None,
                    });
            }
        }
    }
    diagnostics
}

/// The record of one comparison, found by the node it is.
fn met_at<'a>(met: &'a mut Vec<(*const Expr, Met)>, leaf: &Expr) -> &'a mut Met {
    let key = std::ptr::from_ref(leaf);
    let at = match met.iter().position(|(held, _)| *held == key) {
        Some(at) => at,
        None => {
            met.push((key, Met::default()));
            met.len() - 1
        }
    };
    &mut met[at].1
}

// ------------------------------------------------------------------ errors

#[derive(Debug, Clone, PartialEq)]
pub enum FilterError {
    Syntax {
        position: usize,
        expected: String,
        found: String,
    },
    UnbalancedParenthesis {
        position: usize,
    },
    /// A `)` with no `(` open before it.
    UnexpectedClose {
        position: usize,
    },
    UnterminatedString {
        position: usize,
    },
    /// Boxed, all three: a `Field` is 80 bytes, and an unboxed `FilterError`
    /// puts that on every `Result` this module returns, successes included.
    Field(Box<FieldError>),
    TypeConflict(Box<TypeConflict>),
    /// `has` on a field that is not `tags`, or a scalar operator on `tags`.
    /// Its own variant because the fix is a keyword, not a kind.
    WrongOperator {
        field: Box<Field>,
        operator: String,
        reason: String,
    },
    /// Two fields compared whose units differ: `volume > ore_mass`.
    UnitsDiffer {
        left: Box<Field>,
        right: Box<Field>,
        units: (String, String),
    },
    /// A word `state` does not have, refused at `parse`.
    UnknownState {
        position: usize,
        word: String,
        suggestion: Option<String>,
    },
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // A syntax error carries a character position, because the caret
            // under the offending character is the message.
            FilterError::Syntax {
                position,
                expected,
                found,
            } => write!(
                f,
                "at character {}: expected {expected}, found {found}",
                position + 1
            ),
            FilterError::UnbalancedParenthesis { position } => write!(
                f,
                "unbalanced parenthesis: the '(' at character {} was never closed",
                position + 1
            ),
            FilterError::UnexpectedClose { position } => write!(
                f,
                "the ')' at character {} closes nothing: no '(' is open before it",
                position + 1
            ),
            FilterError::UnterminatedString { position } => write!(
                f,
                "the quote opened at character {} is never closed",
                position + 1
            ),
            FilterError::Field(error) => write!(f, "{error}"),
            FilterError::TypeConflict(conflict) => write!(
                f,
                "'{}' holds {} in this collection, and cannot be compared to {} with {}",
                fields::describe(&conflict.field),
                describe_kind(conflict.field_kind),
                describe_value(&conflict.operand),
                conflict.operator
            ),
            FilterError::WrongOperator {
                field,
                operator,
                reason,
            } => write!(
                f,
                "{operator} cannot be used on '{}': {reason}",
                fields::describe(field.as_ref())
            ),
            FilterError::UnitsDiffer { left, right, units } => write!(
                f,
                "'{}' is in {} and '{}' in {}: two properties compared must be in one unit",
                fields::describe(left.as_ref()),
                units.0,
                fields::describe(right.as_ref()),
                units.1
            ),
            FilterError::UnknownState {
                position,
                word,
                suggestion,
            } => {
                write!(f, "at character {}: '{word}' is not a state", position + 1)?;
                if let Some(suggestion) = suggestion {
                    write!(f, "\n  did you mean: '{suggestion}'?")?;
                }
                write!(f, "\n  available: {}", StateWord::WORDS.join(", "))
            }
        }
    }
}

fn describe_kind(kind: ValueKind) -> &'static str {
    match kind {
        ValueKind::Integer => "an integer",
        ValueKind::Number => "a number",
        ValueKind::Text => "text",
        ValueKind::Boolean => "a boolean",
        ValueKind::Date => "a date",
        ValueKind::DateTime => "a date-time",
        ValueKind::Absent => "nothing",
        ValueKind::NotApplicable => "not applicable",
    }
}

fn describe_value(value: &Value) -> String {
    match value {
        Value::Text(text) => format!("\"{text}\""),
        other => describe_kind(other.kind()).to_string(),
    }
}

impl std::error::Error for FilterError {}

/// A caret under the character that failed, for a caller printing to a
/// terminal. The position alone is a number; this is what makes it a message.
pub fn caret(source: &str, error: &FilterError) -> Option<String> {
    let position = match error {
        FilterError::Syntax { position, .. }
        | FilterError::UnbalancedParenthesis { position }
        | FilterError::UnexpectedClose { position }
        | FilterError::UnterminatedString { position }
        | FilterError::UnknownState { position, .. } => *position,
        _ => return None,
    };
    // The message first, so that whatever prefixes it cannot shift the caret
    // off the character it points at.
    let pad = " ".repeat(position.min(source.chars().count()));
    // A comma between two digits is a decimal comma: `radius > 1,05`.
    let characters: Vec<char> = source.chars().collect();
    let decimal_comma = characters.get(position) == Some(&',')
        && position > 0
        && characters[position - 1].is_ascii_digit()
        && characters
            .get(position + 1)
            .is_some_and(char::is_ascii_digit);
    let hint = if decimal_comma {
        let whole: String = characters[..position]
            .iter()
            .rev()
            .take_while(|c| c.is_ascii_digit())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let fraction: String = characters[position + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        format!("\n  a decimal comma? a number is written with a point: {whole}.{fraction}")
    } else {
        String::new()
    };
    Some(format!("{error}\n  {source}\n  {pad}^{hint}"))
}

/// Whether the sample's attributes, or the one named, hold a link to
/// `target` as the link writes it: a wikilink's target, a Markdown link's file
/// name without its `.md` (and `.sample`).
fn links_to(subject: &Subject, field: Option<&Identifier>, target: &str) -> Truth {
    use crate::core::link::{self, Link};
    use crate::core::sample::AttributeValue;
    let leads = |text: &str| match link::parse(text) {
        Some(Link::Sample { target: named, .. }) => named == target,
        Some(Link::Path { path, .. }) => std::path::Path::new(&path)
            .file_stem()
            .map(|stem| stem.to_string_lossy())
            .is_some_and(|stem| stem.strip_suffix(".sample").unwrap_or(&stem) == target),
        _ => false,
    };
    let holds = |value: &AttributeValue| {
        let texts: Vec<&Value> = match value {
            AttributeValue::Scalar(value) => vec![value],
            AttributeValue::List(items) => items.iter().collect(),
        };
        texts
            .into_iter()
            .any(|value| matches!(value, Value::Text(text) if leads(text)))
    };
    let found = match field {
        Some(name) => subject.sample.attribute(name).is_ok_and(holds),
        None => subject
            .sample
            .attribute_names()
            .into_iter()
            .filter_map(|name| subject.sample.attribute(name).ok())
            .any(holds),
    };
    if found { Truth::True } else { Truth::False }
}
