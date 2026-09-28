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
#[derive(Debug)]
enum Expr {
    Or(Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Compare {
        field: Field,
        operator: Operator,
        operand: Value,
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
    /// `state == failed`, `state has failed`, `state in (a, b)`: whether the
    /// sample holds any of the words; `!=` is its negation. Its own variant
    /// because a state is a set asked about, never a value compared.
    State {
        words: Vec<StateWord>,
        negated: bool,
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
    /// Samples a comparison cannot be asked of because the field holds text
    /// there, where other samples hold what it compares: left aside by
    /// [`evaluate_leaving_aside`], and named by the caller. Not a problem of
    /// the filter, so [`Diagnostics::is_empty`] does not count it.
    pub set_aside: Vec<SetAside>,
}

/// A comparison some samples cannot answer, because the field holds text in
/// them: `og > 1.06` where one brew's `og` is `high`.
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
/// what made `beer == "Dunkel 57"` require care and made an index containing a
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
            // An operator being typed belongs to the tail: `malt !` is a partial
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
fn expectation(tokens: &[Token]) -> (Expect, Option<String>) {
    let mut expect = Expect::Field;
    let mut field: Option<String> = None;
    // `in` opens a list, and its parenthesis holds operands rather than the
    // start of another predicate.
    let mut membership = false;
    for token in tokens {
        match &token.kind {
            TokenKind::Open => {
                expect = if membership {
                    membership = false;
                    Expect::Operand
                } else {
                    Expect::Field
                };
            }
            TokenKind::Close => expect = Expect::Connective,
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
    (expect, field)
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
    let (expect, field) = expectation(&tokens);
    // A word operator accepted against the closing quote cannot be continued:
    // `has` and `in` need the space that follows them, and so does a connective.
    let (candidates, trailing) = match expect {
        Expect::Field => (fields::complete(&tail.typed, subject), ""),
        Expect::Operator => (operators_for(field.as_deref(), subject), " "),
        Expect::Presence => (vec!["missing".to_string(), "present".to_string()], " "),
        Expect::Operand => (operands_for(field.as_deref(), subject), ""),
        Expect::Connective => (["&&", "||"].iter().map(|w| (*w).to_string()).collect(), " "),
    };
    let typed = tail.typed.to_lowercase();
    // A field's candidates are already those the typed text leads to — and some
    // do not begin with it: `ebc` finds the column `measurements.ebc[20]` by
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
}

impl<'a> Parser<'a> {
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
            let right = self.conjunction()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn conjunction(&mut self) -> Result<Expr, FilterError> {
        let mut left = self.negation()?;
        while self.connective("&&") {
            let right = self.negation()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn negation(&mut self) -> Result<Expr, FilterError> {
        if self.connective("!") {
            return Ok(Expr::Not(Box::new(self.negation()?)));
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
                let inside = self.expression()?;
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
        let field = fields::parse(&path).map_err(|e| FilterError::Field(Box::new(e)))?;
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
            let Some(token) = self.peek() else {
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
        let operand = self.operand()?;
        let _ = at;
        Ok(Expr::Compare {
            field,
            operator,
            operand,
        })
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
/// making, `og == high` over numbers, is still one.
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
        Expr::HasTag { field, operand } => match resolve(field, subject)? {
            Resolution::Tags(tags) => Ok(
                if matches!(operand, Value::Text(text) if Identifier::new(text).is_ok_and(|tag| tags.contains(&tag)))
                {
                    Truth::True
                } else {
                    Truth::False
                },
            ),
            Resolution::List(values) => Ok(
                if values.iter().any(|value| predicate_equals(value, operand)) {
                    Truth::True
                } else {
                    Truth::False
                },
            ),
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
                if predicate_equals(&left, operand) {
                    return Ok(Truth::True);
                }
            }
            Ok(Truth::False)
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
    // field's kind and the operand, because `malt > "heavy"` needs to say which
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
    /// The one sentence every surface says it in: *b left aside: 'og' holds
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
            // sentence: `og < 2 && og > 1` said b twice.
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
    Some(format!("{error}\n  {source}\n  {pad}^"))
}
