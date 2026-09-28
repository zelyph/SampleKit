//! The scalar domain: what a single recorded quantity can be, how two of them
//! compare, and how a list of them is ordered.
//!

use std::cmp::Ordering;
use std::fmt;
use std::num::NonZeroUsize;

use chrono::Timelike;
use serde::de::{self, Deserialize, Deserializer, Visitor};

/// A calendar date: no time, no timezone.
///
/// A newtype so that the crate behind it stays an implementation choice.
/// Swapping `chrono` for `time` touches this file and the PyO3 conversion,
/// not thirty signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(chrono::NaiveDate);

impl Date {
    /// Build a date from its parts.
    ///
    /// The year is restricted to `0..=9999` so that [`Date::iso`] always
    /// produces the four-digit form [`Date::parse`] accepts. Without the
    /// restriction the two would stop being inverses at year 10000.
    pub fn new(year: i32, month: u32, day: u32) -> Result<Date, ValueError> {
        let text = format!("{year}-{month:02}-{day:02}");
        if !(0..=9999).contains(&year) {
            return Err(ValueError::InvalidDate { text });
        }
        match chrono::NaiveDate::from_ymd_opt(year, month, day) {
            Some(date) => Ok(Date(date)),
            None => Err(ValueError::InvalidDate { text }),
        }
    }

    /// Read `YYYY-MM-DD`, and nothing else.
    ///
    /// The shape is checked before the calendar is, so `14/03/2026` is refused
    /// as malformed rather than reinterpreted, and `2026-3-14` is refused too:
    /// accepting it would make `iso` something other than the exact inverse.
    pub fn parse(text: &str) -> Result<Date, ValueError> {
        let invalid = || ValueError::InvalidDate {
            text: text.to_string(),
        };
        let bytes = text.as_bytes();
        let shaped = bytes.len() == 10
            && bytes[0..4].iter().all(u8::is_ascii_digit)
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(u8::is_ascii_digit)
            && bytes[7] == b'-'
            && bytes[8..10].iter().all(u8::is_ascii_digit);
        if !shaped {
            return Err(invalid());
        }
        let year = text[0..4].parse::<i32>().map_err(|_| invalid())?;
        let month = text[5..7].parse::<u32>().map_err(|_| invalid())?;
        let day = text[8..10].parse::<u32>().map_err(|_| invalid())?;
        Date::new(year, month, day).map_err(|_| invalid())
    }

    /// The storage form: what the file holds and what a filter literal reads.
    ///
    /// Displaying a date to a person belongs to `formatting`, which a project
    /// may configure without the file changing.
    pub fn iso(&self) -> String {
        self.0.format("%Y-%m-%d").to_string()
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iso())
    }
}

/// A local date and time, to a fraction of a second and without a timezone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime(chrono::NaiveDateTime);

impl DateTime {
    pub fn new(
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Result<DateTime, ValueError> {
        let text = format!("{year}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
        let date = Date::new(year, month, day)
            .map_err(|_| ValueError::InvalidDateTime { text: text.clone() })?;
        date.0
            .and_hms_opt(hour, minute, second)
            .map(DateTime)
            .ok_or(ValueError::InvalidDateTime { text })
    }

    /// Read a local ISO-like time with `T` or a space and optional seconds.
    pub fn parse(text: &str) -> Result<DateTime, ValueError> {
        let invalid = || ValueError::InvalidDateTime {
            text: text.to_string(),
        };
        let bytes = text.as_bytes();
        let shaped = (matches!(bytes.len(), 16 | 19)
            || ((21..=29).contains(&bytes.len()) && bytes.get(19) == Some(&b'.')))
            && bytes
                .get(10)
                .is_some_and(|byte| matches!(byte, b'T' | b' '))
            && bytes.get(13) == Some(&b':')
            && (bytes.len() == 16 || bytes.get(16) == Some(&b':'));
        if !shaped {
            return Err(invalid());
        }
        let date = Date::parse(&text[..10]).map_err(|_| invalid())?;
        let digits = |range: std::ops::Range<usize>| {
            text.get(range)
                .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
                .and_then(|part| part.parse::<u32>().ok())
        };
        let hour = digits(11..13).ok_or_else(invalid)?;
        let minute = digits(14..16).ok_or_else(invalid)?;
        let second = if bytes.len() >= 19 {
            digits(17..19).ok_or_else(invalid)?
        } else {
            0
        };
        // Seconds may carry a fraction of up to nine digits. Only a text
        // already shaped as one is cut, where byte 19 is the `.`: any other
        // text, accented or not, is refused before it is sliced.
        let fraction = if bytes.len() > 19 {
            Some(text.get(20..).ok_or_else(invalid)?)
        } else {
            None
        };
        let nanosecond = match fraction {
            Some(part) if !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) => {
                format!("{part:0<9}")
                    .parse::<u32>()
                    .map_err(|_| invalid())?
            }
            Some(_) => return Err(invalid()),
            None => 0,
        };
        date.0
            .and_hms_nano_opt(hour, minute, second, nanosecond)
            .map(DateTime)
            .ok_or_else(invalid)
    }

    pub fn iso(&self) -> String {
        let nanosecond = self.0.nanosecond();
        if self.0.second() == 0 && nanosecond == 0 {
            self.0.format("%Y-%m-%dT%H:%M").to_string()
        } else if nanosecond == 0 {
            self.0.format("%Y-%m-%dT%H:%M:%S").to_string()
        } else {
            // The fraction's digits as written, no zero after the last: `%.f`
            // pads to three, six or nine.
            let fraction = format!("{nanosecond:09}");
            format!(
                "{}.{}",
                self.0.format("%Y-%m-%dT%H:%M:%S"),
                fraction.trim_end_matches('0')
            )
        }
    }

    /// The same moment with a fraction of a second, in nanoseconds.
    pub fn with_nanosecond(self, nanosecond: u32) -> Result<DateTime, ValueError> {
        let text = format!("{}.{nanosecond:09}", self.iso());
        if nanosecond >= 1_000_000_000 {
            return Err(ValueError::InvalidDateTime { text });
        }
        self.0
            .with_nanosecond(nanosecond)
            .map(DateTime)
            .ok_or(ValueError::InvalidDateTime { text })
    }

    pub fn nanosecond(&self) -> u32 {
        self.0.nanosecond()
    }

    fn date(&self) -> Date {
        Date(self.0.date())
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.iso())
    }
}

/// One recorded scalar.
///
/// `Absent` is a kind, not the absence of one: it means *not recorded*, and is
/// distinct from `0`, `0.0` and `""`.
///
/// `Number` never holds NaN or infinity. Rejecting them at construction is
/// what allows [`total_order`] to be total.
#[derive(Debug, Clone)]
pub enum Value {
    Integer(i64),
    Number(f64),
    Text(String),
    Boolean(bool),
    Date(Date),
    DateTime(DateTime),
    Absent,
    /// The value does not apply to this sample — `fg: n/a` for a drink never
    /// fermented. Read as no value wherever a value is used, and said `n/a`; a
    /// formula reading it gives it too.
    NotApplicable,
}

/// The kind of a value without its content, for diagnostics.
///
/// **The declaration order is the cross-kind ordering rule**, with `Integer`
/// and `Number` sharing one position. See [`total_order`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueKind {
    Integer,
    Number,
    Text,
    Boolean,
    Date,
    DateTime,
    Absent,
    NotApplicable,
}

impl ValueKind {
    /// The position of this kind in the cross-kind order.
    ///
    /// `Integer` and `Number` share position 0: they are two storage forms of
    /// one thing, and they interleave numerically rather than grouping.
    fn rank(self) -> u8 {
        match self {
            ValueKind::Integer | ValueKind::Number => 0,
            ValueKind::Text => 1,
            ValueKind::Boolean => 2,
            ValueKind::Date | ValueKind::DateTime => 3,
            ValueKind::Absent | ValueKind::NotApplicable => 4,
        }
    }

    /// The kind's name, as an error message spells it.
    pub fn name(self) -> &'static str {
        match self {
            ValueKind::Integer => "integer",
            ValueKind::Number => "number",
            ValueKind::Text => "text",
            ValueKind::Boolean => "boolean",
            ValueKind::Date => "date",
            ValueKind::DateTime => "date-time",
            ValueKind::Absent => "absent",
            ValueKind::NotApplicable => "n/a",
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl Value {
    pub fn integer(v: i64) -> Value {
        Value::Integer(v)
    }

    /// The one fallible constructor: NaN and infinity are refused here so that
    /// no later layer has to ask.
    pub fn number(v: f64) -> Result<Value, ValueError> {
        if v.is_finite() {
            Ok(Value::Number(v))
        } else {
            Err(ValueError::NotFinite { position: None })
        }
    }

    pub fn text(s: impl Into<String>) -> Value {
        Value::Text(s.into())
    }

    pub fn boolean(v: bool) -> Value {
        Value::Boolean(v)
    }

    pub fn date(d: Date) -> Value {
        Value::Date(d)
    }

    pub fn date_time(d: DateTime) -> Value {
        Value::DateTime(d)
    }

    pub fn absent() -> Value {
        Value::Absent
    }

    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Integer(_) => ValueKind::Integer,
            Value::Number(_) => ValueKind::Number,
            Value::Text(_) => ValueKind::Text,
            Value::Boolean(_) => ValueKind::Boolean,
            Value::Date(_) => ValueKind::Date,
            Value::DateTime(_) => ValueKind::DateTime,
            Value::Absent => ValueKind::Absent,
            Value::NotApplicable => ValueKind::NotApplicable,
        }
    }

    /// No value: absent, or not applicable, which every use of a value reads as
    /// none.
    pub fn is_absent(&self) -> bool {
        matches!(self, Value::Absent | Value::NotApplicable)
    }

    /// Not applicable to this sample, as distinct from not there yet.
    pub fn is_not_applicable(&self) -> bool {
        matches!(self, Value::NotApplicable)
    }
}

/// Compare an `i64` with an `f64` exactly, without widening either.
///
/// Beyond 2^53 an `i64` and an `f64` no longer represent the same integers, so
/// `i as f64` would report two different numbers as equal. Comparing against
/// `floor(f)` keeps the answer exact across the whole range.
fn cmp_integer_number(i: i64, f: f64) -> Ordering {
    // The float is finite: `Value::number` and `Readings::new` are the only
    // ways one enters the system.
    let floor = f.floor();
    // 2^63 is the first f64 above `i64::MAX`, and -2^63 is exactly `i64::MIN`.
    if floor >= 9_223_372_036_854_775_808.0 {
        return Ordering::Less;
    }
    if floor < -9_223_372_036_854_775_808.0 {
        return Ordering::Greater;
    }
    // `floor` is integral and inside the i64 range, so this conversion is exact.
    match i.cmp(&(floor as i64)) {
        // Equal to the floor: the integer is smaller as soon as the float has
        // any fractional part.
        Ordering::Equal if f > floor => Ordering::Less,
        other => other,
    }
}

/// Compare two finite floats. Never `None`, because `Value::Number` is finite.
fn cmp_numbers(x: f64, y: f64) -> Ordering {
    x.partial_cmp(&y)
        .expect("Value::Number holds only finite numbers")
}

/// A total order over every value. Never fails. **For sorting only.**
///
/// A sort cannot stop halfway, so this answers for every pair. The cross-kind
/// rule is [`ValueKind`]'s declaration order:
///
/// ```text
/// Integer / Number  <  Text  <  Boolean  <  Date  <  Absent
/// ```
///
/// The choice among the five is arbitrary, and being arbitrary is exactly why
/// it is written down: two implementations obeying a rule described only as
/// "fixed" would produce two different collection orders, both conforming.
pub fn total_order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => cmp_numbers(*x, *y),
        (Value::Integer(x), Value::Number(y)) => cmp_integer_number(*x, *y),
        (Value::Number(x), Value::Integer(y)) => cmp_integer_number(*y, *x).reverse(),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        (Value::Boolean(x), Value::Boolean(y)) => x.cmp(y),
        (Value::Date(x), Value::Date(y)) => x.cmp(y),
        (Value::DateTime(x), Value::DateTime(y)) => x.cmp(y),
        (Value::Date(x), Value::DateTime(y)) => match x.cmp(&y.date()) {
            Ordering::Equal => Ordering::Less,
            other => other,
        },
        (Value::DateTime(x), Value::Date(y)) => match x.date().cmp(y) {
            Ordering::Equal => Ordering::Greater,
            other => other,
        },
        (Value::Absent, Value::Absent) => Ordering::Equal,
        _ => a.kind().rank().cmp(&b.kind().rank()),
    }
}

/// A typed comparison. **For filters and predicates.**
///
/// Refuses what has no answer: `malt > "heavy"` is a mistake, and returning
/// `false` would hide it. Absence is refused too — `malt > 3` on an unrecorded
/// malt is unanswerable, not false.
pub fn compare(a: &Value, b: &Value) -> Result<Ordering, ValueError> {
    if a.is_absent() || b.is_absent() {
        return Err(ValueError::ComparisonWithAbsent);
    }
    match (a, b) {
        (Value::Integer(_), Value::Integer(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::Integer(_), Value::Number(_))
        | (Value::Number(_), Value::Integer(_))
        | (Value::Text(_), Value::Text(_))
        | (Value::Boolean(_), Value::Boolean(_))
        | (Value::Date(_), Value::Date(_))
        | (Value::DateTime(_), Value::DateTime(_)) => Ok(total_order(a, b)),
        (Value::Date(x), Value::DateTime(y)) | (Value::DateTime(y), Value::Date(x)) => {
            Ok(x.cmp(&y.date()))
        }
        _ => Err(ValueError::IncomparableKinds {
            left: a.kind(),
            right: b.kind(),
        }),
    }
}

/// Equality. Never fails: values of different kinds are simply unequal.
///
/// The exception is `Integer` against `Number`, which is numeric equality, so
/// that `batch == 3` matches a stored `3` and a stored `3.0`.
///
/// `equals(Absent, Absent)` is `true` — required, because [`total_order`] must
/// agree with this function and it orders two absences equal. A filter never
/// reaches here with an absent operand; `filter-language` decides that first.
pub fn equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::Integer(x), Value::Number(y)) | (Value::Number(y), Value::Integer(x)) => {
            cmp_integer_number(*x, *y) == Ordering::Equal
        }
        (Value::Text(x), Value::Text(y)) => x == y,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Date(x), Value::Date(y)) => x == y,
        (Value::DateTime(x), Value::DateTime(y)) => x == y,
        (Value::Absent, Value::Absent) => true,
        // Reflexive, as `Eq` promises: a value not applicable is not a
        // change from itself.
        (Value::NotApplicable, Value::NotApplicable) => true,
        _ => false,
    }
}

/// `==` is [`equals`], so the two can never drift apart.
///
/// There is deliberately no `PartialOrd`: `<` would have to pick between
/// [`total_order`] and [`compare`], and picking silently is the conflation
/// those two functions exist to prevent.
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        equals(self, other)
    }
}

impl Eq for Value {}

/// Case-insensitive substring search over text.
///
/// Applied to any non-text kind this is an error rather than a match against a
/// rendered form: the text of a number or a date depends on `formatting` and on
/// a project's configuration, neither of which is visible from here.
pub fn contains(haystack: &Value, needle: &str) -> Result<bool, ValueError> {
    let text = as_text(haystack)?;
    Ok(text.to_lowercase().contains(&needle.to_lowercase()))
}

/// Case-insensitive prefix test over text. Same rule as [`contains`].
pub fn starts_with(haystack: &Value, prefix: &str) -> Result<bool, ValueError> {
    let text = as_text(haystack)?;
    Ok(text.to_lowercase().starts_with(&prefix.to_lowercase()))
}

fn as_text(value: &Value) -> Result<&str, ValueError> {
    match value {
        Value::Text(s) => Ok(s),
        other => Err(ValueError::NotText {
            found: other.kind(),
        }),
    }
}

/// The repeated readings behind one measured quantity: non-empty, all finite.
///
/// It exists as a type so that "empty series" and "series containing NaN" are
/// unrepresentable rather than checked at every use site.
#[derive(Debug, Clone, PartialEq)]
pub struct Readings(Vec<f64>);

impl Readings {
    pub fn new(readings: Vec<f64>) -> Result<Readings, ValueError> {
        if readings.is_empty() {
            return Err(ValueError::EmptyReadings);
        }
        if let Some(position) = readings.iter().position(|r| !r.is_finite()) {
            return Err(ValueError::NotFinite {
                position: Some(position),
            });
        }
        Ok(Readings(readings))
    }

    pub fn as_slice(&self) -> &[f64] {
        &self.0
    }

    /// The number of readings, which is never zero.
    ///
    /// There is no `is_empty`: the return type already answers it.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.0.len()).expect("Readings is non-empty by construction")
    }
}

/// Everything this module refuses to guess at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    NotFinite { position: Option<usize> },
    EmptyReadings,
    InvalidDate { text: String },
    InvalidDateTime { text: String },
    IncomparableKinds { left: ValueKind, right: ValueKind },
    ComparisonWithAbsent,
    NotText { found: ValueKind },
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueError::NotFinite {
                position: Some(position),
            } => write!(
                f,
                "reading {position} is not a finite number: NaN and infinity are not measurements"
            ),
            ValueError::NotFinite { position: None } => write!(
                f,
                "not a finite number: NaN and infinity are not measurements"
            ),
            ValueError::EmptyReadings => write!(
                f,
                "a measured quantity needs at least one reading; an empty series has \
                 neither a value nor an uncertainty"
            ),
            ValueError::InvalidDate { text } => write!(
                f,
                "'{text}' is not a date: write it as YYYY-MM-DD, with a day that exists"
            ),
            ValueError::InvalidDateTime { text } => write!(
                f,
                "'{text}' is not a local date-time: write YYYY-MM-DDTHH:MM with optional seconds and a fraction of a second, no timezone"
            ),
            ValueError::IncomparableKinds { left, right } => write!(
                f,
                "cannot order {left} against {right}: only integers and numbers compare \
                 across kinds. Compare values of one kind, or use == to test equality"
            ),
            ValueError::ComparisonWithAbsent => write!(
                f,
                "cannot order against an unrecorded value: the answer is unknown, not \
                 false. Use 'is present' or 'is missing' to test for absence"
            ),
            ValueError::NotText { found } => write!(
                f,
                "this operator reads text, and found {found}: how a {found} reads as text \
                 depends on a project's formatting, which is not visible here"
            ),
        }
    }
}

impl std::error::Error for ValueError {}

/// A scalar as a file spells it.
///
/// | In the file | Becomes |
/// | --- | --- |
/// | `12.5`, `-3` | `Number`, `Integer` |
/// | `true`, `false` | `Boolean` |
/// | `2026-03-14` | `Date`, **whether or not it is quoted** |
/// | anything else | `Text` |
/// | `null`, or the key absent | `Absent` |
///
/// A date is recognized by its shape and by nothing else, because serde's data
/// model has no notion of scalar style: a quoted date and a plain one arrive
/// identical. A text value of exactly that shape is therefore not
/// representable.
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ScalarVisitor)
    }
}

struct ScalarVisitor;

impl<'de> Visitor<'de> for ScalarVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a number, a date, a boolean or text — one scalar, never a list or a mapping")
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Value, E> {
        Ok(Value::boolean(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Value, E> {
        Ok(Value::integer(v))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Value, E> {
        match i64::try_from(v) {
            Ok(integer) => Ok(Value::integer(integer)),
            // Beyond i64 it is no longer an integer this domain holds; a whole
            // number that large in a measurement collection is a data-entry
            // accident, and refusing it is louder than widening it.
            Err(_) => Err(E::custom(format!(
                "{v} is too large to be an integer here; the domain is i64"
            ))),
        }
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        Value::number(v).map_err(de::Error::custom)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Value, E> {
        recognize_text(v).map_err(E::custom)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::absent())
    }

    fn visit_none<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::absent())
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ScalarVisitor)
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<Value, A::Error> {
        Err(de::Error::custom(
            "a list is not a scalar; only `tags` holds several values",
        ))
    }

    fn visit_map<A: de::MapAccess<'de>>(self, _: A) -> Result<Value, A::Error> {
        Err(de::Error::custom(
            "a mapping is not a scalar; an attribute is one value and nothing else",
        ))
    }
}

/// Apply the file format's shape-based date and date-time recognition.
pub fn recognize_text(text: &str) -> Result<Value, ValueError> {
    // Not applicable to this sample, written as the file and `set` spell it.
    if text == "n/a" {
        return Ok(Value::NotApplicable);
    }
    if let Ok(date) = Date::parse(text) {
        return Ok(Value::date(date));
    }
    // A date, a `T` or a space, then `HH:MM`: a date-time, refused when it
    // does not read as one. A date followed by other text is text —
    // `2024-01-01 meeting notes` — and a first character wider than a byte
    // is never sliced through.
    let bytes = text.as_bytes();
    let digit = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_digit);
    let date_time_shaped = bytes
        .get(10)
        .is_some_and(|byte| matches!(byte, b'T' | b' '))
        && digit(11)
        && digit(12)
        && bytes.get(13) == Some(&b':')
        && digit(14)
        && digit(15)
        && text.get(..10).is_some_and(|date| Date::parse(date).is_ok());
    if date_time_shaped {
        return DateTime::parse(text).map(Value::date_time);
    }
    Ok(Value::text(text))
}
