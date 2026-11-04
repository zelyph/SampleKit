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
                    than to the property: write .2f and let the table choose its own width"
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
    /// Whether both numbers are spreads, rounded up: an uncertainty standing
    /// alone, in a column or a file of its own.
    spread: bool,
}

impl Precision {
    /// One specifier covering the whole quantity.
    pub fn both(spec: &str) -> Result<Precision, FormatError> {
        let parsed = Spec::new(spec)?;
        Ok(Precision {
            value: parsed.clone(),
            uncertainty: parsed,
            spread: false,
        })
    }

    /// Two specifiers, deliberately different.
    pub fn split(value: &str, uncertainty: &str) -> Result<Precision, FormatError> {
        Ok(Precision {
            value: Spec::new(value)?,
            uncertainty: Spec::new(uncertainty)?,
            spread: false,
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
    /// quantity writes it, rounded up.
    pub fn of_uncertainty(&self) -> Precision {
        Precision {
            value: self.uncertainty.clone(),
            uncertainty: self.uncertainty.clone(),
            spread: true,
        }
    }

    /// The general form a table writes a spread in where nothing is declared,
    /// six significant digits, rounded up.
    pub fn general_spread() -> Precision {
        Precision::both(".6g")
            .expect("a specifier written here parses")
            .of_uncertainty()
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
    /// The style's form of a property, `{value}`, `{uncertainty}`, `{unit}`
    /// and `{symbol}` placed; `None`, the built-in `12.50 ± 0.05 g`.
    pub property: Option<String>,
    /// The piece `{uncertainty}` stands for in the style's forms.
    pub uncertainty: Option<String>,
    /// The style's form of a property with no unit.
    pub unitless: Option<String>,
    /// The unit as the style writes it beside a value, outside any form —
    /// escaped as its text is, a LaTeX form in math mode; `None`, `unit`.
    pub unit_alone: Option<String>,
    /// The unit as the style writes it where its form places `{unit}`.
    pub unit_in_form: Option<String>,
    pub precision: Option<Precision>,
}

impl Resolved {
    /// The stored strings unchanged, with `±` — what a pure Rust reader with no
    /// project, and every test in this module, needs.
    pub fn plain(p: &Presentation) -> Resolved {
        Resolved {
            unit: p.unit.clone(),
            symbol: p.symbol.clone(),
            property: None,
            uncertainty: None,
            unitless: None,
            unit_alone: None,
            unit_in_form: None,
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
        Value::Number(number) => match &r.precision {
            Some(precision) => render(precision.value(), *number, precision.spread),
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
/// is what is found in the other. **An uncertainty is rounded up, never
/// understated**: `0.00006` at `.3f` is `0.001`, never `0.000`.
pub fn format_uncertainty(uncertainty: &Uncertainty, r: &Resolved) -> String {
    match &r.precision {
        Some(precision) => render(precision.uncertainty(), uncertainty.magnitude(), true),
        None => general_rounded(uncertainty.magnitude(), 6, false, true),
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
                    true,
                )
            }),
        ),
        (None, Some(u)) => {
            let (value_text, uncertainty_text) = by_convention(magnitude, u.magnitude());
            (value_text, Some(uncertainty_text))
        }
        (None, None) => (format_value(value, r), None),
    };

    // The unit as the style writes it beside the quantity: `\%`, never the
    // `%` that begins a LaTeX comment.
    let alone = r
        .unit_alone
        .as_deref()
        .or(r.unit.as_deref())
        .filter(|unit| !unit.is_empty());
    // The style's own form, where it declares one: `\SI{12.5 \pm 0.05}{\gram}`.
    if let Some(mut filled) = filled_property(
        &StyleForms {
            property: r.property.as_deref(),
            uncertainty: r.uncertainty.as_deref(),
            unitless: r.unitless.as_deref(),
        },
        &rendered,
        rendered_uncertainty.as_deref(),
        r.unit_in_form.as_deref().or(r.unit.as_deref()),
        r.symbol.as_deref(),
    ) {
        // **A form that does not place the unit leaves it to follow**: a
        // given style puts it in a table's header, and a value written
        // alone — `view`, a script's property — lost it, `$6.8 \pm 0.1$`.
        if let Some(unit) = alone
            && !r
                .property
                .as_deref()
                .is_some_and(|form| form.contains("{unit}"))
        {
            filled.push(' ');
            filled.push_str(unit);
        }
        return filled;
    }
    if let Some(uncertainty_text) = rendered_uncertainty {
        rendered.push_str(" \u{b1} ");
        rendered.push_str(&uncertainty_text);
    }
    if let Some(unit) = alone {
        rendered.push(' ');
        rendered.push_str(unit);
    }
    rendered
}

/// A style's forms of a property: the whole, its piece for an uncertainty,
/// and its form with no unit.
#[derive(Debug, Clone, Copy, Default)]
pub struct StyleForms<'a> {
    pub property: Option<&'a str>,
    pub uncertainty: Option<&'a str>,
    pub unitless: Option<&'a str>,
}

/// A value written in a style's forms, `None` where the style declares
/// none. `unitless` stands for `property` where there is no unit; in either,
/// `{uncertainty}` is the piece `uncertainty` says — nothing where there is
/// no uncertainty — or the number itself where no piece is declared. A value
/// with no uncertainty and no piece to leave out is the caller's to write.
pub fn filled_property(
    forms: &StyleForms,
    value: &str,
    uncertainty: Option<&str>,
    unit: Option<&str>,
    symbol: Option<&str>,
) -> Option<String> {
    let unit = unit.filter(|unit| !unit.is_empty());
    let template = match (unit, forms.unitless) {
        (None, Some(unitless)) => unitless,
        // With no uncertainty and no piece to leave out, the value as the
        // caller writes it.
        _ => match (uncertainty, forms.uncertainty) {
            (None, None) => return None,
            _ => forms.property?,
        },
    };
    let uncertainty = match (uncertainty, forms.uncertainty) {
        (Some(number), Some(piece)) => piece.replace("{uncertainty}", number),
        (Some(number), None) => number.to_string(),
        (None, _) => String::new(),
    };
    Some(
        template
            .replace("{value}", value)
            .replace("{uncertainty}", &uncertainty)
            .replace("{unit}", unit.unwrap_or(""))
            .replace("{symbol}", symbol.unwrap_or(""))
            .trim_end()
            .to_string(),
    )
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
        Some(spec) => render(spec, integer as f64, false),
    }
}

/// A number rounded to zero is written without a sign: `-0.000` read as a
/// mistake, and the sign of a zero rounded is no information.
fn unsigned_zero(text: String) -> String {
    match text.strip_prefix('-') {
        Some(rest)
            if rest
                .chars()
                .take_while(|c| !matches!(c, 'e' | 'E'))
                .all(|c| c == '0' || c == '.') =>
        {
            rest.to_string()
        }
        _ => text,
    }
}

/// A number through a specifier. **Every number is rounded on its shortest
/// decimal writing**, so that two results equal in decimal are written alike:
/// rounded on the binary value, `6.9125` was `6.912` once and `6.913` another
/// time. **A value is rounded to the nearest, a half away from zero**,
/// `6.9125` at three decimals `6.913`; **an uncertainty, `up`, is rounded up,
/// never understated**, `0.121` at two `0.13` and `0.00006` at three `0.001`.
fn render(spec: &Spec, x: f64, up: bool) -> String {
    match spec.kind {
        // Python's default for `f` and `e` with no precision is six.
        SpecKind::Fixed => fixed_rounded(x, spec.precision.unwrap_or(6), up),
        SpecKind::Exponential => unsigned_zero(exponential_signed(
            x,
            spec.precision.unwrap_or(6),
            spec.upper,
            up,
        )),
        SpecKind::General => general_rounded(x, spec.precision.unwrap_or(6), spec.upper, up),
        // `d` on a number is `.0f` without a decimal point.
        SpecKind::Integer => fixed_rounded(x, 0, up),
    }
}

/// The shortest decimal writing of `x`'s magnitude: its digits, without
/// leading zeros, and the power of ten of the first; `None` for zero, and for
/// what is not finite.
fn shortest_digits(x: f64) -> Option<(Vec<u8>, i32)> {
    if x == 0.0 || !x.is_finite() {
        return None;
    }
    let written = format!("{:e}", x.abs());
    let (mantissa, exponent) = written.split_once('e')?;
    let digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|digit| digit - b'0')
        .collect();
    Some((digits, exponent.parse().ok()?))
}

/// `digits` cut to their first `keep`, a half up on what is cut, or `up`
/// whatever is cut: the digits kept, one more where a carry added one.
///
/// **Rounded up, the digits past the fifteenth significant one are dropped
/// first, to the nearest**: they are the arithmetic's, and `0.1 + 0.2`, written
/// `0.30000000000000004`, would be `0.31` at two decimals. Only where the cut
/// falls among the digits: a precision keeping them all, `.17g`, writes every
/// one.
fn cut(digits: &[u8], keep: usize, up: bool) -> Vec<u8> {
    const HELD: usize = 15;
    if up && digits.len() > HELD && keep < digits.len() {
        let held = cut(digits, HELD, false);
        // A carry past the first digit, `9.99…`, puts every place one further.
        return match held.len() > HELD {
            true => cut(&held, keep + 1, true),
            false => cut(&held, keep, true),
        };
    }
    let mut kept: Vec<u8> = digits.iter().take(keep).copied().collect();
    kept.resize(keep, 0);
    let rest = digits.get(keep..).unwrap_or(&[]);
    let carry = match up {
        true => rest.iter().any(|digit| *digit != 0),
        false => rest.first().is_some_and(|first| *first >= 5),
    };
    if carry {
        let mut at = kept.len();
        loop {
            if at == 0 {
                kept.insert(0, 1);
                break;
            }
            at -= 1;
            if kept[at] == 9 {
                kept[at] = 0;
            } else {
                kept[at] += 1;
                break;
            }
        }
    }
    kept
}

/// A spread — an uncertainty, a standard error — through a declared precision's
/// uncertainty specifier, rounded up.
pub fn spread_at(x: f64, precision: &Precision) -> String {
    render(precision.uncertainty(), x, true)
}

/// `x` with `decimals` places, rounded a half up on its shortest decimal
/// writing.
pub fn fixed(x: f64, decimals: usize) -> String {
    fixed_rounded(x, decimals, false)
}

/// A spread with `decimals` places, rounded up: never understated.
pub fn fixed_up(x: f64, decimals: usize) -> String {
    fixed_rounded(x, decimals, true)
}

fn fixed_rounded(x: f64, decimals: usize, up: bool) -> String {
    unsigned_zero(fixed_signed(x, decimals, up))
}

fn fixed_signed(x: f64, decimals: usize, up: bool) -> String {
    let Some((digits, exponent)) = shortest_digits(x) else {
        // Zero, and what is not finite, as Rust writes them.
        return format!("{x:.decimals$}");
    };
    let sign = if x < 0.0 { "-" } else { "" };
    // The digits before the cut: those of the integer part and `decimals`.
    let keep = exponent + 1 + decimals as i32;
    let kept: Vec<u8> = if keep < 0 {
        // Every digit is cut, and the first lies two places or more below the
        // last one kept: less than a half of it, and rounded up one of it.
        vec![u8::from(up)]
    } else {
        let mut kept = cut(&digits, keep as usize, up);
        if kept.is_empty() {
            kept.push(0);
        }
        kept
    };
    // Written right-aligned on the last place kept.
    let mut text: String = kept.iter().map(|digit| char::from(b'0' + digit)).collect();
    if text.len() <= decimals {
        text = format!("{}{text}", "0".repeat(decimals + 1 - text.len()));
    }
    let point = text.len() - decimals;
    let (whole, fraction) = text.split_at(point);
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    if decimals == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    }
}

/// `x` to `significant` digits, rounded on its shortest decimal writing: the
/// mantissa's digits and the exponent after rounding.
fn significant_digits(x: f64, significant: usize, up: bool) -> Option<(Vec<u8>, i32)> {
    let (digits, exponent) = shortest_digits(x)?;
    let kept = cut(&digits, significant, up);
    // A carry past the first digit renormalises: 9.99 at two digits is 1.0e1.
    if kept.len() > significant {
        Some((kept[..significant].to_vec(), exponent + 1))
    } else {
        Some((kept, exponent))
    }
}

fn mantissa_of(digits: &[u8]) -> String {
    let mut text: String = digits
        .iter()
        .take(1)
        .map(|digit| char::from(b'0' + digit))
        .collect();
    if digits.len() > 1 {
        text.push('.');
        text.extend(digits[1..].iter().map(|digit| char::from(b'0' + digit)));
    }
    text
}

/// Rust writes `1.234e5`; Python writes `1.234e+05`. The specifiers are
/// Python's, so the output is Python's.
fn exponential(x: f64, precision: usize, upper: bool) -> String {
    unsigned_zero(exponential_signed(x, precision, upper, false))
}

fn exponential_signed(x: f64, precision: usize, upper: bool, up: bool) -> String {
    // A number that is no number is written as one is everywhere: rendering
    // never fails, where it panicked here.
    if !x.is_finite() {
        return format!("{x}");
    }
    let Some((digits, exponent)) = significant_digits(x, precision + 1, up) else {
        let (mantissa, exponent) = split_exponential(&format!("{x:.*e}", precision));
        return join_exponential(&mantissa, exponent, upper);
    };
    let sign = if x < 0.0 { "-" } else { "" };
    join_exponential(&format!("{sign}{}", mantissa_of(&digits)), exponent, upper)
}

/// The general format, as Python defines it: significant digits, fixed
/// notation when the exponent is in `-4..precision` and scientific otherwise,
/// with trailing zeros removed either way.
fn general(x: f64, precision: usize, upper: bool) -> String {
    general_rounded(x, precision, upper, false)
}

fn general_rounded(x: f64, precision: usize, upper: bool, up: bool) -> String {
    unsigned_zero(general_signed(x, precision, upper, up))
}

fn general_signed(x: f64, precision: usize, upper: bool, up: bool) -> String {
    let significant = precision.max(1);
    let Some((_, exponent)) = significant_digits(x, significant, up) else {
        return strip_trailing_zeros(&format!("{x:.*}", significant - 1));
    };
    if exponent < -4 || exponent >= significant as i32 {
        strip_trailing_zeros_of_exponential(&unsigned_zero(exponential_signed(
            x,
            significant - 1,
            upper,
            up,
        )))
    } else {
        let decimals = (significant as i32 - 1 - exponent).max(0) as usize;
        strip_trailing_zeros(&fixed_rounded(x, decimals, up))
    }
}

/// `1.200e+05` as `1.2e+05`: the mantissa's trailing zeros, not the exponent's.
fn strip_trailing_zeros_of_exponential(text: &str) -> String {
    match text.find(['e', 'E']) {
        Some(at) => format!("{}{}", strip_trailing_zeros(&text[..at]), &text[at..]),
        None => strip_trailing_zeros(text),
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

/// The decimal places the convention may write an uncertainty to, fewest
/// first: two significant digits at most, one at least, and two when the
/// first is a 1, which rounded alone loses up to a third of its size. `None`
/// for zero, or what is not a finite number: there is no place to lend.
pub fn uncertainty_places(uncertainty: f64) -> Option<(i32, i32)> {
    if uncertainty == 0.0 {
        return None;
    }
    let (two, exponent_two) = significant_digits(uncertainty.abs(), 2, true)?;
    let (one, exponent_one) = significant_digits(uncertainty.abs(), 1, true)?;
    let most = 1 - exponent_two;
    if two.first() == Some(&1) || one.first() == Some(&1) {
        return Some((most, most));
    }
    Some((-exponent_one, most))
}

/// The significant digits convention, used when nothing declares a precision:
/// the uncertainty to two significant digits, and the value rounded to the same
/// decimal place.
///
/// ```text
/// value 12.4987, uncertainty 0.0523   ->   "12.499 +- 0.053"
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
    // Two significant digits of the uncertainty, rounded up and renormalised:
    // 0.0991 is 0.10, so the exponent read back is the one after rounding.
    let Some((digits, exponent)) = significant_digits(uncertainty, 2, true) else {
        return (general(value, 6, false), general(uncertainty, 6, false));
    };
    let rounded: f64 = format!("{}e{exponent}", mantissa_of(&digits))
        .parse()
        .expect("a number written here reads back");
    let decimals = 1 - exponent;
    if decimals >= 0 {
        let decimals = decimals as usize;
        (fixed(value, decimals), fixed(rounded, decimals))
    } else {
        // Two significant digits land above the decimal point: both numbers are
        // rounded to that power of ten. `1284.3 +- 270` reads `1280 +- 270`.
        let scale = 10f64.powi(-decimals);
        let value: f64 = fixed(value / scale, 0)
            .parse::<f64>()
            .unwrap_or(value / scale)
            * scale;
        // **Past what a double holds exactly, the digits are the significant
        // ones, in scientific notation**: written whole, 6.02e23 ± 3e20 read
        // 601999999999999995805696, digits no measurement gave.
        if value.abs() >= 1e15 {
            let (_, value_exponent) = split_exponential(&format!("{value:e}"));
            let digits = (value_exponent + decimals).max(0) as usize;
            return (
                exponential(value, digits, false),
                exponential(rounded, 1, false),
            );
        }
        (fixed(value, 0), fixed(rounded, 0))
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
