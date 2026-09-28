//! The exact bytes of the YAML frontmatter: which form each value takes, in
//! what order fields appear, and what is omitted.
//!
//! Its goal is that saving an unmodified sample produces the file it came from,
//! byte for byte — so that a diff shows what the researcher actually did.
//!
//! **This module is the only writer.** serde reads and never writes, so every
//! rule the format has is stated here and applied in one place.
//!

use crate::core::identifier::Identifier;
use crate::core::property::{Fingerprint, InputName};
use crate::core::sample::AttributeValue;
use crate::core::value::Value;
use crate::format::schema::{ColumnSchema, PropertySchema, Recorded, SampleSchema, TableSchema};

/// A flow collection wraps here. Eighty is a habit from terminals that no
/// longer exist, and a property with a unit, an uncertainty and a precision
/// exceeds it routinely.
const WRAP: usize = 100;

/// Emit the frontmatter of one sample. Deterministic: the same schema always
/// produces the same bytes.
pub fn write(schema: &SampleSchema) -> String {
    let mut out = String::new();
    out.push_str(&format!("schema_version: {}\n", schema.schema_version));
    if let Some(name) = &schema.name {
        out.push_str(&format!("name: {}\n", scalar(&Value::text(name.clone()))));
    }
    // Omitted entirely when empty — never written as `tags: []`.
    if !schema.tags.is_empty() {
        let tags: Vec<String> = schema
            .tags
            .iter()
            .map(|tag| scalar(&Value::text(tag.to_string())))
            .collect();
        out.push_str(&flow_line("tags", &tags, '[', ']', 0));
    }
    // Attributes sit between `tags` and `properties`: everything that describes
    // the sample before everything that measures it.
    for (name, value) in &schema.attributes {
        if !value.is_empty() {
            out.push_str(&format!("{name}: {}\n", attribute_form(value)));
        }
    }
    let properties: String = schema
        .properties
        .iter()
        .map(|(name, property)| property_form(name, property, 2))
        .collect();
    if !properties.is_empty() {
        out.push_str("properties:\n");
        out.push_str(&properties);
    }
    // A table without a row is not written, as an empty property is not.
    let tables: String = schema
        .tables
        .iter()
        .filter(|(_, table)| !table.rows.is_empty())
        .map(|(name, table)| table_form(name, table))
        .collect();
    if !tables.is_empty() {
        out.push_str("tables:\n");
        out.push_str(&tables);
    }
    out
}

/// The canonical bytes of an attribute. A list uses YAML flow style so the
/// same bytes can also be hashed when it is a computation input.
pub fn attribute_form(value: &AttributeValue) -> String {
    match value {
        AttributeValue::Scalar(value) => scalar(value),
        AttributeValue::List(values) => format!(
            "[{}]",
            values.iter().map(scalar).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Whether a frontmatter is already what this module would write.
///
/// It takes the parsed shape beside the bytes because it cannot parse: reading
/// YAML belongs to `document`, which depends on this module.
pub fn is_canonical(frontmatter: &str, schema: &SampleSchema) -> bool {
    write(schema) == frontmatter
}

/// A property restricted to `v`, `readings` and `u`, always as a flow mapping
/// and **never** the bare-scalar shorthand.
///
/// It spells the channels the way the file spells them. That is the point of
/// this function living beside the writer: one vocabulary, in the bytes a
/// reader sees and in the bytes that are hashed.
///
/// It is the byte string `fingerprint` hashes, and it lives here so that a
/// digest is taken over the same number formatting, string quoting and omission
/// rules a reader sees in the file. A second serialization would drift from
/// this one, and the drift would surface as a collection reporting itself stale
/// after an upgrade that changed nothing.
pub fn value_form(property: &PropertySchema) -> String {
    let mut entries = Vec::new();
    if let Some(value) = &property.value {
        // An integer hashes as the equal number: `200` rewritten `200.0` by hand
        // is the same value, and stales nothing. **Only where the number is that
        // integer**: past 2^53 a double cannot hold every integer, `…001` and
        // `…002` became one number, and a changed input read current — the
        // digest weaker than the equality `value` keeps exact. Beyond that bound
        // no number is written equal to it, so the integer hashes as itself.
        const EXACT: i64 = 1 << 53;
        let hashed = match value {
            Value::Integer(integer) if (-EXACT..=EXACT).contains(integer) => {
                number(*integer as f64)
            }
            other => scalar(other),
        };
        entries.push(format!("v: {hashed}"));
    }
    if let Some(readings) = &property.readings {
        let numbers: Vec<String> = readings.iter().map(|r| number(*r)).collect();
        entries.push(format!("readings: [{}]", numbers.join(", ")));
    }
    if let Some(uncertainty) = property.uncertainty {
        entries.push(format!("u: {}", number(uncertainty)));
    }
    format!("{{{}}}", entries.join(", "))
}

// ------------------------------------------------------------- properties

/// The eight fields, in the one order they are ever written.
///
/// **`v` and `u`, not `value` and `uncertainty`**. Both spellings are read,
/// here and as field addresses, and always were; this is the one that is
/// written, and it is 5.87 % of a real collection. The digest's pre-image in
/// `value_form` keeps the long spelling deliberately: it is never written
/// anywhere, so shortening it would save nothing and restamp every fingerprint
/// in every file.
///
/// `computed` is last but one because it is the longest field and the least
/// often read: placing it between the value and its unit would push the unit
/// off the first line of a property that has one. The readings **alone**, for
/// the digest a declared statistic records.
///
/// A second scope, not a second serialization: the same number formatting as
/// `value_form`, so the bytes hashed stay bytes a reader can see in the file.
/// `of` asks *has this quantity changed*; this asks *have the observations
/// changed*, which is what says a statistic no longer stands for them.
pub fn readings_form(property: &PropertySchema) -> Option<String> {
    let readings = property.readings.as_ref()?;
    let numbers: Vec<String> = readings.iter().map(|r| number(*r)).collect();
    Some(format!("{{readings: [{}]}}", numbers.join(", ")))
}

/// `{v: mean, u: standard_error}` — the channels that have one, each statistic
/// written bare, since it is one of `Summary`'s fields. The channels take the
/// short spelling every written channel takes.
fn statistics_form(statistics: &crate::format::schema::Statistics) -> String {
    let mut entries = Vec::new();
    if let Some(location) = statistics.value {
        entries.push(format!("v: {}", location.name()));
    }
    if let Some(convention) = statistics.uncertainty {
        entries.push(format!("u: {}", convention.name()));
    }
    format!("{{{}}}", entries.join(", "))
}

fn property_entries(property: &PropertySchema) -> Vec<String> {
    let mut entries = Vec::new();
    // The value as it stands, and **only** one that stands: a value written
    // beside readings, or the statistic the model declares and the record
    // beside it vouches for. Readings with neither are written alone — no mean
    // is taken here on the reader's behalf, since which statistic stands for
    // them is the model's to say, and one written by the writer would read back
    // as a value somebody chose.
    if let Some(value) = &property.value {
        entries.push(format!("v: {}", scalar(value)));
    }
    if let Some(readings) = &property.readings {
        let numbers: Vec<String> = readings.iter().map(|r| number(*r)).collect();
        entries.push(format!("readings: [{}]", numbers.join(", ")));
    }
    if let Some(uncertainty) = property.uncertainty {
        entries.push(format!("u: {}", number(uncertainty)));
    }
    if let Some(unit) = &property.unit {
        entries.push(format!("unit: {}", scalar(&Value::text(unit.clone()))));
    }
    if let Some(symbol) = &property.symbol {
        entries.push(format!("symbol: {}", scalar(&Value::text(symbol.clone()))));
    }
    // With the records, not with the presentation: it says where a number came
    // from, not how it is written.
    if let Some(statistics) = &property.statistics
        && (statistics.value.is_some() || statistics.uncertainty.is_some())
    {
        entries.push(format!("statistics: {}", statistics_form(statistics)));
    }
    if let Some(computed) = &property.computed {
        entries.push(format!("computed: {}", computed_form(computed)));
    }
    if let Some(fingerprint) = &property.fingerprint {
        entries.push(format!("fingerprint: {}", digest_form(fingerprint)));
    } else if let Some(failure) = &property.failure {
        // Where the digest would be, as an edited mark is.
        entries.push(format!("fingerprint: {{failed: {}}}", key(failure)));
    }
    entries
}

/// A record's keys carry their scope: bare for the sample, `row.` for this
/// row's cell, `table.column` for a whole column — **never shortened**, because
/// a bare name is already the sample's. A `computed` record: the inputs alone
/// where no channel is named, and one entry per channel where they are.
fn computed_form(computed: &crate::format::schema::Computed) -> String {
    if !computed.names_channels() {
        return record_map(computed.quantity.as_ref().unwrap_or(&Default::default()));
    }
    // The common inputs first, plainly, then each channel's own: a reader sees
    // what the property reads before what one formula reads.
    let mut entries = Vec::new();
    if let Some(common) = &computed.quantity {
        let written = record_map(common);
        entries.extend(
            written
                .trim_start_matches('{')
                .trim_end_matches('}')
                .split(", ")
                .filter(|entry| !entry.is_empty())
                .map(str::to_string),
        );
    }
    for (key, held) in [("v", &computed.value), ("u", &computed.uncertainty)] {
        if let Some(inputs) = held {
            entries.push(format!("{key}: {}", record_map(inputs)));
        }
    }
    format!("{{{}}}", entries.join(", "))
}

fn record_map(computed: &indexmap::IndexMap<InputName, Recorded>) -> String {
    let entries: Vec<String> = computed
        .iter()
        .map(|(name, record)| {
            let written = match record {
                Recorded::Value(value) => scalar(value),
                Recorded::Edited(digest) => digest_form(digest),
            };
            format!("{}: {written}", key(&name.to_string()))
        })
        .collect();
    format!("{{{}}}", entries.join(", "))
}

/// A digest, or a one-key mapping naming it an override's. Quoted as any string
/// is: `972327790827` and `844409772e92` are hexadecimal, and a reader would
/// take them for numbers.
fn digest_form(digest: &Fingerprint) -> String {
    if digest.is_edited() {
        format!("{{edited: {}}}", key(digest.as_str()))
    } else {
        key(digest.as_str())
    }
}

fn property_form(name: &Identifier, property: &PropertySchema, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let entries = property_entries(property);

    // A property carrying only a value is a bare scalar.
    if let (1, Some(value)) = (entries.len(), &property.value) {
        return format!("{pad}{name}: {}\n", scalar(value));
    }
    // Nothing without a value is written: no empty property.
    if entries.is_empty() {
        return String::new();
    }

    // A property carrying `computed` is a block mapping. The exception is
    // forced — a records map pushes a flow mapping well past the limit — and it
    // earns its keep: a derived value looks different from an entered one at a
    // glance.
    if property.computed.is_some() {
        let mut out = format!("{pad}{name}:\n");
        for entry in entries {
            out.push_str(&wrapped(&entry, indent + 2));
        }
        return out;
    }
    flow_mapping(&format!("{pad}{name}"), &entries, indent)
}

fn table_form(name: &Identifier, table: &TableSchema) -> String {
    let mut out = format!("  {name}:\n");
    if let Some(title) = &table.title {
        out.push_str(&format!(
            "    title: {}\n",
            scalar(&Value::text(title.clone()))
        ));
    }
    // One index column is written bare; several as a sequence. The common
    // length, not a different shape.
    if table.index.len() == 1 {
        out.push_str(&format!("    index: {}\n", table.index[0]));
    } else {
        let names: Vec<String> = table.index.iter().map(Identifier::to_string).collect();
        out.push_str(&flow_line("index", &names, '[', ']', 4));
    }
    out.push_str("    columns:\n");
    for (column, meta) in &table.columns {
        out.push_str(&column_form(column, meta));
    }
    out.push_str("    rows:\n");
    for row in &table.rows {
        let mut first = true;
        // The columns' order, not the file's: a row is a mapping and a file may
        // spell it in any order, but a table has one declared order and an
        // emitter that echoed the file's would give one table two canonical
        // forms. A cell whose column is not declared is written after the
        // declared ones rather than dropped; the next read refuses it, which is
        // where that belongs.
        let ordered = table
            .columns
            .keys()
            .filter_map(|column| row.get_key_value(column))
            .chain(
                row.iter()
                    .filter(|(column, _)| !table.columns.contains_key(*column)),
            );
        for (column, cell) in ordered {
            let body = cell_form(column, cell);
            if first {
                // `- ` opens the row, and the first cell sits on that line;
                // the eight-space indent of a cell is what the dash replaces.
                out.push_str(&format!("      -{}", &body[7..]));
                first = false;
            } else {
                out.push_str(&body);
            }
        }
    }
    out
}

/// A cell on one line, however long: a derived cell's record stays on its row
/// rather than spreading a row of seven cells over twenty lines.
fn cell_form(name: &Identifier, cell: &PropertySchema) -> String {
    let entries = property_entries(cell);
    if let (1, Some(value)) = (entries.len(), &cell.value) {
        return format!("        {name}: {}\n", scalar(value));
    }
    format!("        {name}: {{{}}}\n", entries.join(", "))
}

fn column_form(name: &Identifier, meta: &ColumnSchema) -> String {
    let mut entries = Vec::new();
    if let Some(unit) = &meta.unit {
        entries.push(format!("unit: {}", scalar(&Value::text(unit.clone()))));
    }
    if let Some(symbol) = &meta.symbol {
        entries.push(format!("symbol: {}", scalar(&Value::text(symbol.clone()))));
    }
    // After the presentation, as a property's: it says where a cell's number
    // came from, not how it is written.
    if let Some(statistics) = &meta.statistics
        && (statistics.value.is_some() || statistics.uncertainty.is_some())
    {
        entries.push(format!("statistics: {}", statistics_form(statistics)));
    }
    if entries.is_empty() {
        return format!("      {name}: {{}}\n");
    }
    flow_mapping(&format!("      {name}"), &entries, 6)
}

// ------------------------------------------------------------------ layout

/// A flow mapping on one line, wrapped when it would exceed the limit.
fn flow_mapping(prefix: &str, entries: &[String], indent: usize) -> String {
    let one_line = format!("{prefix}: {{{}}}\n", entries.join(", "));
    if one_line.trim_end().chars().count() <= WRAP {
        return one_line;
    }
    let pad = " ".repeat(indent + 2);
    let mut out = format!("{prefix}: {{");
    let mut line = out.chars().count();
    for (at, entry) in entries.iter().enumerate() {
        let piece = if at + 1 == entries.len() {
            entry.clone()
        } else {
            format!("{entry},")
        };
        if at > 0 && line + 1 + piece.chars().count() > WRAP {
            out.push('\n');
            out.push_str(&pad);
            line = pad.chars().count();
        } else if at > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(&piece);
        line += piece.chars().count();
    }
    out.push_str("}\n");
    out
}

/// A key with a flow sequence for its value, wrapped the same way.
fn flow_line(key: &str, items: &[String], open: char, close: char, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let one_line = format!("{pad}{key}: {open}{}{close}\n", items.join(", "));
    if one_line.trim_end().chars().count() <= WRAP {
        return one_line;
    }
    let continuation = " ".repeat(indent + 2);
    let mut out = format!("{pad}{key}: {open}");
    let mut line = out.chars().count();
    for (at, item) in items.iter().enumerate() {
        let piece = if at + 1 == items.len() {
            item.clone()
        } else {
            format!("{item},")
        };
        if at > 0 && line + 1 + piece.chars().count() > WRAP {
            out.push('\n');
            out.push_str(&continuation);
            line = continuation.chars().count();
        } else if at > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(&piece);
        line += piece.chars().count();
    }
    out.push(close);
    out.push('\n');
    out
}

/// One `key: value` line of a block mapping, wrapped if its value is a flow
/// collection that does not fit.
fn wrapped(entry: &str, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let line = format!("{pad}{entry}\n");
    if line.trim_end().chars().count() <= WRAP {
        return line;
    }
    match entry.split_once(": {") {
        Some((key, rest)) => {
            // Exactly the one closing brace: the last entry may be a mapping of
            // its own, `{edited: …}`, whose brace belongs to it.
            let body = rest.strip_suffix('}').unwrap_or(rest);
            let items: Vec<String> = split_entries(body);
            let mut out = flow_mapping(&format!("{pad}{key}"), &items, indent);
            if out.ends_with('\n') {
                out.truncate(out.len() - 1);
            }
            out.push('\n');
            out
        }
        None => line,
    }
}

/// Split a rendered flow body at top-level commas, so a wrapped map keeps its
/// nested collections intact.
fn split_entries(body: &str) -> Vec<String> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut current = String::new();
    let mut quoted = false;
    for c in body.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            '[' | '{' if !quoted => {
                depth += 1;
                current.push(c);
            }
            ']' | '}' if !quoted => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ',' if depth == 0 && !quoted => {
                entries.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        entries.push(current.trim().to_string());
    }
    entries
}

// ------------------------------------------------------------------ scalars

/// How one value is spelled.
fn scalar(value: &Value) -> String {
    match value {
        Value::Integer(integer) => integer.to_string(),
        Value::Number(n) => number(*n),
        Value::Boolean(flag) => flag.to_string(),
        Value::Date(date) => date.iso(),
        Value::DateTime(date_time) => date_time.iso(),
        Value::Text(text) => key(text),
        // Never written as `null`: a field that carries nothing is omitted.
        Value::Absent => String::new(),
        // Written, as the answer it is.
        Value::NotApplicable => "n/a".to_string(),
    }
}

/// Numbers round-trip exactly: the shortest decimal representation that parses
/// back to the same `f64`. Not a fixed number of digits, and never the display
/// precision — writing a rounded value would make display metadata destructive.
fn number(x: f64) -> String {
    let positional = {
        let shortest = format!("{x}");
        // An integral float keeps its `.0`, so a temperature stored as 20.0 does
        // not come back as an integer.
        if shortest.contains(['.', 'e', 'E']) {
            shortest
        } else {
            format!("{shortest}.0")
        }
    };
    let exponential = exponent_form(x);
    // Shortest means shortest on the page. A tie goes to the positional form,
    // which is the one a reader does not have to decode.
    if exponential.len() < positional.len() {
        exponential
    } else {
        positional
    }
}

/// `1.0e-5`: the point and the sign are what a YAML 1.1 reader needs to see a
/// number rather than the string `1e-5`. The same defence that quotes `no`,
/// pointed the other way.
fn exponent_form(x: f64) -> String {
    let written = format!("{x:e}");
    let (mantissa, exponent) = written.split_once('e').expect("LowerExp writes an e");
    let mantissa = if mantissa.contains('.') {
        mantissa.to_string()
    } else {
        format!("{mantissa}.0")
    };
    if exponent.starts_with('-') {
        format!("{mantissa}e{exponent}")
    } else {
        format!("{mantissa}e+{exponent}")
    }
}

/// A string, quoted when a plain one would parse as something else.
fn key(text: &str) -> String {
    if needs_quoting(text) {
        let escaped: String = text
            .chars()
            .map(|c| match c {
                '\\' => "\\\\".to_string(),
                '"' => "\\\"".to_string(),
                '\n' => "\\n".to_string(),
                '\t' => "\\t".to_string(),
                '\r' => "\\r".to_string(),
                // A control character, and the line breaks a YAML reader
                // takes as one, escaped: written as they are, they end the
                // line or are refused.
                other if (other as u32) < 0x20 || other == '\u{7f}' => {
                    format!("\\x{:02X}", other as u32)
                }
                '\u{85}' | '\u{2028}' | '\u{2029}' => format!("\\u{:04X}", c as u32),
                // Unicode is not escaped: `é` stays `é`.
                other => other.to_string(),
            })
            .collect();
        format!("\"{escaped}\"")
    } else {
        text.to_string()
    }
}

fn needs_quoting(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    if text.trim() != text {
        return true;
    }
    // Resolved as something other than text by one YAML reader or another. The
    // 1.1 spellings are quoted defensively: this format's own reader follows
    // 1.2, and the file is read by things that are not SampleKit.
    const RESOLVED: [&str; 9] = ["true", "false", "null", "~", "yes", "no", "on", "off", "y"];
    if RESOLVED.contains(&text.to_ascii_lowercase().as_str()) {
        return true;
    }
    if text.parse::<f64>().is_ok() || text.parse::<i64>().is_ok() {
        return true;
    }
    // Numbers in the forms YAML knows and Rust does not parse: `0x1F`,
    // `0o17`, `.inf`, `.NaN`, `+.inf`, and 1.1's `1_000`.
    let lower = text.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if unsigned.starts_with("0x")
        || unsigned.starts_with("0o")
        || unsigned == ".inf"
        || unsigned == ".nan"
        || (text.contains('_') && text.replace('_', "").parse::<f64>().is_ok())
    {
        return true;
    }
    // A key's colon at the end, and what YAML does not read as the start of
    // plain text.
    if text.ends_with(':') {
        return true;
    }
    if text.chars().any(|c| {
        (c as u32) < 0x20 || c == '\u{7f}' || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}')
    }) {
        return true;
    }
    // A date-shaped string is quoted even though quoting cannot save it: the
    // file stays honest about what was meant, for every reader but this one.
    if crate::core::value::Date::parse(text).is_ok()
        || crate::core::value::DateTime::parse(text).is_ok()
    {
        return true;
    }
    let first = text.chars().next().expect("non-empty");
    if "-?:,[]{}#&*!|>'\"%@`.+".contains(first) {
        return true;
    }
    // A comma or a bracket ends a value written in flow style, `{value: 12,5}`.
    text.contains(": ")
        || text.contains(" #")
        || text.contains(['\n', '\t', '\r', ',', '[', ']', '{', '}'])
}
