//! Rendering a quantity as text: how many digits, where the uncertainty goes,
//! where the unit goes.
//!
//! **It is told; it does not look up.** A project can declare that `lintner`
//! renders as `\Omega`, but that table lives at Layer 3 and this module is
//! Layer 0. Every style-dependent literal reaches it already resolved, in
//! [`Resolved`].
//!

use std::fmt;

use crate::core::uncertainty::Uncertainty;
use crate::core::value::Value;

/// The most digits a precision may ask for. A double carries seventeen
/// significant figures, and `.3e` writes the smallest of them in a few — so a
/// precision past this asks the file for figures it does not have. **Refused
/// when the precision is read**, as every other flaw in one is: `.65536f` in a sample file reached the formatter,
/// whose limit is lower than `usize`'s, and panicked every command that drew
/// the sample.
const MOST_DIGITS: usize = 99;

/// The type character of a specifier, and what it does to a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpecKind {
    /// `f`, `F` — a fixed number of decimals.
    Fixed,
    /// `e`, `E` — scientific notation.
    Exponential,
    /// `g`, `G` — the shorter of the two, at a given number of significant
    /// digits, with trailing zeros removed.
    General,
    /// `d` — whole digits, no decimal point.
    Integer,
}

/// One format specifier in Python's mini-language.
///
/// Python's syntax is adopted rather than invented because the specifiers are
/// written by users in Python models and in TOML configuration, and must mean
/// the same thing in both. The supported subset is:
///
/// ```ebnf
/// spec       = fractional | integral ;
/// fractional = [ "." , digits ] , float-type ;
/// integral   = "d" ;
/// float-type = "f" | "F" | "e" | "E" | "g" | "G" ;
/// ```
///
/// Everything else is refused, and refusing is the decision: v1 answered `.2q`
/// by printing sixteen digits, which looks like a precision choice and is not
/// one. Validation happens here, at construction, so an invalid specifier is
/// reported where it was written rather than at render time inside a table
/// cell where its origin is no longer visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    text: String,
    precision: Option<usize>,
    kind: SpecKind,
    upper: bool,
}

impl Spec {
    pub fn new(specifier: &str) -> Result<Spec, FormatError> {
        match parse_spec(specifier) {
            Ok((precision, kind, upper)) => Ok(Spec {
                text: specifier.to_string(),
                precision,
                kind,
                upper,
            }),
            Err(reason) => Err(FormatError::InvalidPrecision {
                specifier: specifier.to_string(),
                reason,
            }),
        }
    }

    /// The specifier exactly as written, which is what `canonicalization`
    /// writes back to the file.
    pub fn as_str(&self) -> &str {
        &self.text
    }
}

impl fmt::Display for Spec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

type ParsedSpec = (Option<usize>, SpecKind, bool);

fn parse_spec(specifier: &str) -> Result<ParsedSpec, String> {
    if specifier.is_empty() {
        return Err("a format specifier cannot be empty: write .3f, .2e, g or d".to_string());
    }
    // The three exclusions that deserve their own sentence, checked before the
    // grammar so that the message explains the decision rather than the syntax.
    if specifier.contains('%') {
        return Err(
            "'%' multiplies the measurement by a hundred behind a one-character \
                    suffix, which is a silent transformation. Store the value you mean"
                .to_string(),
        );
    }
    if specifier.contains(',') || specifier.contains('_') {
        return Err(
            "a thousands separator is locale-dependent, and a terminal and a CSV \
                    file must not disagree about what a number is"
                .to_string(),
        );
    }

    let (precision, type_part) = match specifier.strip_prefix('.') {
        Some(after_dot) => {
            let digits: String = after_dot.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                return Err("'.' must be followed by a digit count, as in .3f".to_string());
            }
            let count = digits
                .parse::<usize>()
                .ok()
                .filter(|count| *count <= MOST_DIGITS)
                .ok_or_else(|| {
                    format!(
                        "{digits} digits is more than any number here carries: at most \
                         {MOST_DIGITS}, and .3e writes a very small number in a few"
                    )
                })?;
            (Some(count), &after_dot[digits.len()..])
        }
        None => (None, specifier),
    };

    if type_part.is_empty() {
        return Err(format!(
            "'{specifier}' says how many digits and not how to write them: add a type, \
             as in {specifier}f or {specifier}e"
        ));
    }
    if type_part.chars().count() != 1 {
        // `08.2f`, `>10.2f`, `+.2f`, `3f`: everything that carries a width, a
        // fill, an alignment or a sign.
        return Err(
            "a width, a fill, an alignment or a sign belongs to the column rather \
                    than to the quantity: write .2f and let the table choose its own width"
                .to_string(),
        );
    }
    let type_character = type_part
        .chars()
        .next()
        .expect("checked to be one character");

    match type_character {
        'f' => Ok((precision, SpecKind::Fixed, false)),
        'F' => Ok((precision, SpecKind::Fixed, true)),
        'e' => Ok((precision, SpecKind::Exponential, false)),
        'E' => Ok((precision, SpecKind::Exponential, true)),
        'g' => Ok((precision, SpecKind::General, false)),
        'G' => Ok((precision, SpecKind::General, true)),
        'd' if precision.is_none() => Ok((None, SpecKind::Integer, false)),
        'd' => Err(
            "d writes whole digits and takes no precision: write .3f for three \
                    decimals, or d on its own"
                .to_string(),
        ),
        other => Err(format!(
            "'{other}' is not a supported type: use f, e or g (or F, E, G) for numbers, \
             and d for whole digits"
        )),
    }
}

/// The precision of one quantity — **both of its numbers**.
///
/// A declared `.3f` formats the value and the uncertainty alike; a pair is
/// given only to separate them deliberately. The earlier shape had two
/// independent fields, which let `12.500 ± 0.1` be written by accident. Under
/// this shape that output is still reachable, through [`Precision::split`], and
/// reaching it is a decision rather than an oversight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precision {
    value: Spec,
    uncertainty: Spec,
}

impl Precision {
    /// One specifier covering the whole quantity.
    pub fn both(spec: &str) -> Result<Precision, FormatError> {
        let parsed = Spec::new(spec)?;
        Ok(Precision {
            value: parsed.clone(),
            uncertainty: parsed,
        })
    }

    /// Two specifiers, deliberately different.
    pub fn split(value: &str, uncertainty: &str) -> Result<Precision, FormatError> {
        Ok(Precision {
            value: Spec::new(value)?,
            uncertainty: Spec::new(uncertainty)?,
        })
    }

    pub fn value(&self) -> &Spec {
        &self.value
    }

    pub fn uncertainty(&self) -> &Spec {
        &self.uncertainty
    }

    /// The uncertainty's specifier for both numbers: what writes an
    /// uncertainty standing alone, in a column or a file of its own, as the
    /// quantity writes it.
    pub fn of_uncertainty(&self) -> Precision {
        Precision {
            value: self.uncertainty.clone(),
            uncertainty: self.uncertainty.clone(),
        }
    }

    /// Whether one specifier describes both numbers.
    ///
    /// This is what lets `canonicalization` write one specifier instead of two,
    /// so a round trip never invents a second value the author did not type.
    pub fn is_uniform(&self) -> bool {
        self.value.as_str() == self.uncertainty.as_str()
    }
}

/// The complete display metadata of one quantity, exactly as stored.
///
/// Portable data, not runtime state: what is in the file is what the author
/// wrote — `lintner`, `.3f` — so that a reader with no project configuration still
/// renders something exact.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Presentation {
    pub unit: Option<String>,
    pub symbol: Option<String>,
    pub precision: Option<Precision>,
}

/// The literals for **one** rendering, in the style in force.
///
/// `unit` and `symbol` are already the right variant; `separator` is already
/// `±`, `\pm` or `+/-`. Nothing here is looked up, and nothing here is stored.
///
/// `symbol` is carried rather than rendered: it names the quantity, which is a
/// header's business, not a cell's. It is resolved here so that whoever writes
/// the header does not have to reach for the project table a second time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub unit: Option<String>,
    pub symbol: Option<String>,
    pub separator: String,
    pub precision: Option<Precision>,
}

impl Resolved {
    /// The stored strings unchanged, with `±` — what a pure Rust reader with no
    /// project, and every test in this module, needs.
    pub fn plain(p: &Presentation) -> Resolved {
        Resolved {
            unit: p.unit.clone(),
            symbol: p.symbol.clone(),
            separator: "\u{b1}".to_string(),
            precision: p.precision.clone(),
        }
    }
}

/// Render the value alone: no unit, no uncertainty.
///
/// The form for a column that carries its unit in its header, and for an export
/// that keeps the value and the uncertainty in separate fields.
pub fn format_value(value: &Value, r: &Resolved) -> String {
    match value {
        // Not `0`, not `None`, not a dash: the caller decides whether to show a
        // placeholder, because a dash is right in a table and wrong in a CSV.
        Value::Absent => String::new(),
        // Said, where absence is left to the caller: it is an answer.
        Value::NotApplicable => "n/a".to_string(),
        Value::Text(text) => text.clone(),
        Value::Boolean(flag) => flag.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        Value::Integer(integer) => render_integer(spec_for_value(r), *integer),
        Value::Number(number) => match spec_for_value(r) {
            Some(spec) => render(spec, *number),
            None => general(*number, 6, false),
        },
    }
}

/// Render the whole quantity: `12.500 ± 0.050 g`.
///
/// The unit follows the whole quantity, not the value: `12.50 ± 0.05 g` states
/// that both numbers are in grams, whereas `12.50 g ± 0.05` invites the reader
/// to wonder about the second.
pub fn format_quantity(value: &Value, uncertainty: Option<&Uncertainty>, r: &Resolved) -> String {
    quantity(value, uncertainty, r)
}

/// The uncertainty alone: through its own specifier, else the general form.
///
/// **A declared precision is applied as it is written, everywhere**: a screen,
/// a table, an export and a template round alike, so that what is read on one
/// is what is found in the other. A magnitude the specifier rounds to zero is
/// written as zero; `validate` is where that is noted.
pub fn format_uncertainty(uncertainty: &Uncertainty, r: &Resolved) -> String {
    match &r.precision {
        Some(precision) => render(precision.uncertainty(), uncertainty.magnitude()),
        None => general(uncertainty.magnitude(), 6, false),
    }
}

/// A declared precision writes both numbers as it says; with none, the
/// convention writes them to the uncertainty's two significant digits.
fn quantity(value: &Value, uncertainty: Option<&Uncertainty>, r: &Resolved) -> String {
    let magnitude = match value {
        Value::Integer(integer) => *integer as f64,
        Value::Number(number) => *number,
        // A non-numeric quantity renders alone. A unit on a date and an
        // uncertainty on a label describe nothing, and attaching them would
        // make a mis-declaration look like data.
        _ => return format_value(value, r),
    };

    let (mut rendered, rendered_uncertainty) = match (spec_for_value(r), uncertainty) {
        (Some(_), _) => (
            format_value(value, r),
            uncertainty.map(|u| {
                render(
                    r.precision
                        .as_ref()
                        .expect("a value spec means a precision")
                        .uncertainty(),
                    u.magnitude(),
                )
            }),
        ),
        (None, Some(u)) => {
            let (value_text, uncertainty_text) = by_convention(magnitude, u.magnitude());
            (value_text, Some(uncertainty_text))
        }
        (None, None) => (format_value(value, r), None),
    };

    if let Some(uncertainty_text) = rendered_uncertainty {
        rendered.push(' ');
        rendered.push_str(&r.separator);
        rendered.push(' ');
        rendered.push_str(&uncertainty_text);
    }
    if let Some(unit) = r.unit.as_deref().filter(|unit| !unit.is_empty()) {
        rendered.push(' ');
        rendered.push_str(unit);
    }
    rendered
}

fn spec_for_value(r: &Resolved) -> Option<&Spec> {
    r.precision.as_ref().map(Precision::value)
}

/// An integer renders as its exact digits — `3`, never `3.0` — until a
/// precision says otherwise.
///
/// `d` keeps it exact rather than routing it through an `f64`, which beyond
/// 2^53 would print a different number.
fn render_integer(spec: Option<&Spec>, integer: i64) -> String {
    match spec {
        None => integer.to_string(),
        Some(spec) if spec.kind == SpecKind::Integer => integer.to_string(),
        Some(spec) => render(spec, integer as f64),
    }
}

fn render(spec: &Spec, x: f64) -> String {
    match spec.kind {
        // Python's default for `f` and `e` with no precision is six.
        SpecKind::Fixed => format!("{x:.*}", spec.precision.unwrap_or(6)),
        SpecKind::Exponential => exponential(x, spec.precision.unwrap_or(6), spec.upper),
        SpecKind::General => general(x, spec.precision.unwrap_or(6), spec.upper),
        // `d` on a number is `.0f` without a decimal point: it rounds, half to
        // even, like every other path here.
        SpecKind::Integer => format!("{x:.0}"),
    }
}

/// Rust writes `1.234e5`; Python writes `1.234e+05`. The specifiers are
/// Python's, so the output is Python's.
fn exponential(x: f64, precision: usize, upper: bool) -> String {
    let (mantissa, exponent) = split_exponential(&format!("{x:.*e}", precision));
    join_exponential(&mantissa, exponent, upper)
}

/// The general format, as Python defines it: significant digits, fixed
/// notation when the exponent is in `-4..precision` and scientific otherwise,
/// with trailing zeros removed either way.
fn general(x: f64, precision: usize, upper: bool) -> String {
    let significant = precision.max(1);
    let (mantissa, exponent) = split_exponential(&format!("{x:.*e}", significant - 1));
    if exponent < -4 || exponent >= significant as i32 {
        join_exponential(&strip_trailing_zeros(&mantissa), exponent, upper)
    } else {
        let decimals = (significant as i32 - 1 - exponent).max(0) as usize;
        strip_trailing_zeros(&format!("{x:.*}", decimals))
    }
}

fn split_exponential(rust_form: &str) -> (String, i32) {
    let (mantissa, exponent) = rust_form
        .rsplit_once('e')
        .expect("Rust's exponential format always writes an exponent");
    (
        mantissa.to_string(),
        exponent.parse().expect("and it is always an integer"),
    )
}

fn join_exponential(mantissa: &str, exponent: i32, upper: bool) -> String {
    let letter = if upper { 'E' } else { 'e' };
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}{letter}{sign}{:02}", exponent.abs())
}

fn strip_trailing_zeros(text: &str) -> String {
    match text.contains('.') {
        true => text.trim_end_matches('0').trim_end_matches('.').to_string(),
        false => text.to_string(),
    }
}

/// The significant digits convention, used when nothing declares a precision:
/// the uncertainty to two significant digits, and the value rounded to the same
/// decimal place.
///
/// ```text
/// value 12.4987, uncertainty 0.0523   ->   "12.499 +- 0.052"
/// value 1284.3,  uncertainty 27.0     ->   "1284 +- 27"
/// ```
///
/// The alternative default — printing the stored `f64` — produces
/// `12.498700000000001 +- 0.05230000000000001`, which is unreadable and, worse,
/// claims sixteen digits of precision from a measurement that supports four.
fn by_convention(value: f64, uncertainty: f64) -> (String, String) {
    if uncertainty == 0.0 {
        // An exactly known quantity has no decimal place to lend. Both numbers
        // go through the general form rather than through a made-up one.
        return (general(value, 6, false), general(uncertainty, 6, false));
    }
    // Formatting to one decimal in scientific notation *is* rounding to two
    // significant digits, and it renormalises: 0.0999 becomes 1.0e-1, so the
    // exponent read back is the one after rounding.
    let two_significant = format!("{uncertainty:.1e}");
    let (_, exponent) = split_exponential(&two_significant);
    let rounded: f64 = two_significant
        .parse()
        .expect("a number Rust wrote is a number Rust reads");
    let decimals = 1 - exponent;
    if decimals >= 0 {
        let decimals = decimals as usize;
        (
            format!("{value:.*}", decimals),
            format!("{rounded:.*}", decimals),
        )
    } else {
        // Two significant digits land above the decimal point: both numbers are
        // rounded to that power of ten. `1284.3 +- 270` reads `1280 +- 270`.
        let scale = 10f64.powi(-decimals);
        (
            // Half to even, as every other path here rounds.
            format!("{:.0}", (value / scale).round_ties_even() * scale),
            format!("{rounded:.0}"),
        )
    }
}

/// The one way a specifier fails, raised at the point of declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    InvalidPrecision { specifier: String, reason: String },
}

impl fmt::Display for FormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FormatError::InvalidPrecision { specifier, reason } => {
                write!(f, "'{specifier}' is not a usable precision: {reason}")
            }
        }
    }
}

impl std::error::Error for FormatError {}
