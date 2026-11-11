//! The file: splitting it into canonical frontmatter and a verbatim note,
//! parsing the first, carrying the second untouched, and writing both back.
//!
//! It is the only module that reads and writes sample files, and the only one
//! that can damage a researcher's notes — which is why the note never passes
//! through any transformation here.
//!

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::core::dependency_graph::Node;
use crate::core::identifier::Identifier;
use crate::core::property::{Fingerprint, Property};
use crate::core::sample::{Note, PropertyHandle, Sample};
use crate::format::canonicalization;
use crate::format::fingerprint;
use crate::format::schema::{self, SampleSchema, SchemaError};
use indexmap::IndexMap;

const DELIMITER: &str = "---";
const BOM: char = '\u{feff}';

/// A file, in two parts, plus what its bytes looked like.
///
/// `bom` and `crlf` are not decoration: they are detected at parse and
/// reapplied at write, because a file that gains or loses either is a file the
/// program changed without being asked to.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub schema: SampleSchema,
    pub note: Note,
    pub bom: bool,
    pub crlf: bool,
}

/// Where a document was read from, and what it contained then.
///
/// Transient and never written to the file: a stored integrity hash is still
/// refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Origin {
    pub path: PathBuf,
    pub digest: Fingerprint,
}

/// Where a document is written, and whether that is checked.
///
/// A destination rather than a flag: a boolean would sit in every call site
/// defaulted to the safe value and be flipped by whoever met the error first.
#[derive(Debug, Clone, PartialEq)]
pub enum Destination {
    /// Overwrite the file it came from, refusing if it moved on.
    Origin(Origin),
    /// Write here, comparing nothing: a deliberate overwrite.
    Path(PathBuf),
    /// Write a file that does not exist yet, refusing if another writer made
    /// one there since it was looked for.
    New(PathBuf),
}

impl Destination {
    fn path(&self) -> &Path {
        match self {
            Destination::Origin(origin) => &origin.path,
            Destination::Path(path) | Destination::New(path) => path,
        }
    }
}

pub(crate) fn digest_of(bytes: &str) -> Fingerprint {
    let hash = Sha256::digest(bytes.as_bytes());
    let hex: String = hash.iter().map(|byte| format!("{byte:02x}")).collect();
    Fingerprint::new(&hex[..12])
}

// -------------------------------------------------------------- splitting

/// The frontmatter is everything between the opening `---` on the first line
/// and the next line that is exactly `---`. The note is everything after that
/// line, starting from the following byte.
/// The deepest `[` and `{` a frontmatter nests: a sample's deepest — a
/// table's rows, a cell's readings — is a handful.
pub(crate) const MAX_NESTING: usize = 64;

/// The line of the frontmatter, counted from its first, where its brackets
/// and braces, outside quotes, nest past [`MAX_NESTING`].
/// The lines of the frontmatter, from 1, where `key` is written at the same
/// indentation more than once, from the mapping the parser placed at line
/// `from`: those of the first indentation that holds it twice. Empty where
/// the key is not found so — written inside braces, on one line.
fn key_lines(frontmatter: &str, key: &str, from: usize) -> Vec<usize> {
    let mut by_indent: Vec<(usize, Vec<usize>)> = Vec::new();
    for (at, line) in frontmatter.lines().enumerate().skip(from.saturating_sub(1)) {
        let trimmed = line.trim_start();
        let written = trimmed
            .strip_prefix(key)
            .or_else(|| trimmed.strip_prefix(&format!("\"{key}\"")))
            .or_else(|| trimmed.strip_prefix(&format!("'{key}'")));
        if !written.is_some_and(|rest| rest.trim_start().starts_with(':')) {
            continue;
        }
        let indent = line.len() - trimmed.len();
        match by_indent.iter_mut().find(|(held, _)| *held == indent) {
            Some((_, lines)) => lines.push(at + 1),
            None => by_indent.push((indent, vec![at + 1])),
        }
    }
    by_indent
        .into_iter()
        .find(|(_, lines)| lines.len() > 1)
        .map(|(_, lines)| lines)
        .unwrap_or_default()
}

/// A link written unquoted, said with its line in the file and the quoted
/// form to write: what every reader of a frontmatter refuses first.
pub(crate) fn unquoted_link_said(frontmatter: &str) -> Option<(usize, String)> {
    unquoted_link(frontmatter).map(|(line, key, link)| {
        (
            line,
            format!("{link} unquoted reads as a list: a link is quoted, {key}: \"{link}\""),
        )
    })
}

/// The first `key: [[target]]`, or `key: [text](path)`, a frontmatter holds
/// unquoted: its line in the file, its key and the link as written.
fn unquoted_link(frontmatter: &str) -> Option<(usize, String, String)> {
    frontmatter.lines().enumerate().find_map(|(at, text)| {
        let (key, value) = text.split_once(':')?;
        let key = key.trim();
        if key.is_empty() || key.starts_with(['#', '-', '"', '\'']) {
            return None;
        }
        let value = value.split(" #").next().unwrap_or_default().trim();
        // `[text](path)`, which YAML reads as a list and then stops at.
        if value.starts_with('[')
            && !value.starts_with("[[")
            && matches!(
                crate::core::link::parse(value),
                Some(crate::core::link::Link::Path { .. } | crate::core::link::Link::Url { .. })
            )
        {
            return Some((at + 2, key.to_string(), value.to_string()));
        }
        let inner = value.strip_prefix("[[")?.strip_suffix("]]")?;
        (!inner.is_empty() && !inner.contains(['[', ']', ',']))
            .then(|| (at + 2, key.to_string(), value.to_string()))
    })
}

pub(crate) fn nested_too_deep(frontmatter: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (line, text) in frontmatter.lines().enumerate() {
        let mut quote: Option<char> = None;
        for c in text.chars() {
            match (quote, c) {
                (Some(open), c) if c == open => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '#') => break,
                (None, '[' | '{') => {
                    depth += 1;
                    if depth > MAX_NESTING {
                        return Some(line);
                    }
                }
                (None, ']' | '}') => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    None
}

/// Documents parsed, by the text they were parsed from: a file read again
/// unchanged, by the next command of the terminal interface or by a plan
/// reading the sample a list already read, is not parsed again. Parsing is
/// most of reading a sample.
const PARSED_KEPT: usize = 50_000;

/// Shared by every thread: the terminal interface plans what the model owes
/// beside the screen, from the files the screen read.
fn parsed() -> &'static std::sync::Mutex<std::collections::HashMap<String, Document>> {
    static PARSED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Document>>,
    > = std::sync::OnceLock::new();
    PARSED.get_or_init(Default::default)
}

/// `work` done on each of `items` across the machine's cores, the answers in
/// the order of the items: a project's files read and parsed at once, where
/// one after another left all but one core idle. A few items are done here,
/// where starting threads costs more than it gives.
pub fn in_parallel<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    // The stack the command line's own thread has: a sample's YAML nests
    // as deep there as here.
    const STACK: usize = 16 << 20;
    const ALONE_BELOW: usize = 64;
    let cores = std::thread::available_parallelism().map_or(1, |cores| cores.get().min(8));
    if cores < 2 || items.len() < ALONE_BELOW {
        return items.iter().map(&work).collect();
    }
    let chunk = items.len().div_ceil(cores);
    let work = &work;
    // Writing through what waits, a thread started here reads through it too.
    let through = super::stage::through_now();
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| {
                std::thread::Builder::new()
                    .stack_size(STACK)
                    .spawn_scoped(scope, move || {
                        let _through = through.then(super::stage::Through::new);
                        part.iter().map(work).collect::<Vec<R>>()
                    })
            })
            .collect();
        let mut done = Vec::with_capacity(items.len());
        for (handle, part) in handles.into_iter().zip(items.chunks(chunk)) {
            match handle.ok().and_then(|handle| handle.join().ok()) {
                Some(answers) => done.extend(answers),
                // A thread the system would not start, or that panicked: its
                // part done here, so that a panic is a panic of this thread.
                None => done.extend(part.iter().map(work)),
            }
        }
        done
    })
}

pub fn parse(source: &str) -> Result<Document, DocumentError> {
    if let Some(held) = parsed()
        .lock()
        .ok()
        .and_then(|parsed| parsed.get(source).cloned())
    {
        return Ok(held);
    }
    let document = parse_text(source)?;
    if let Ok(mut parsed) = parsed().lock() {
        if parsed.len() >= PARSED_KEPT {
            parsed.clear();
        }
        parsed.insert(source.to_string(), document.clone());
    }
    Ok(document)
}

fn parse_text(source: &str) -> Result<Document, DocumentError> {
    let bom = source.starts_with(BOM);
    let body = source.strip_prefix(BOM).unwrap_or(source);
    // The file's own convention, taken from its first line ending.
    let crlf = body
        .split('\n')
        .next()
        .is_some_and(|line| line.ends_with('\r'));

    let first = body.lines().next().unwrap_or("");
    if first.trim_end_matches('\r') != DELIMITER {
        return Err(DocumentError::MissingFrontmatter);
    }

    // Only the first closing delimiter is structural; a `---` inside the note
    // is text.
    let mut offset = body.find('\n').map(|at| at + 1).unwrap_or(body.len());
    let mut frontmatter = String::new();
    let mut closed = false;
    while offset < body.len() {
        let rest = &body[offset..];
        let end = rest.find('\n').map(|at| at + 1).unwrap_or(rest.len());
        let line = &rest[..end];
        if line.trim_end_matches(['\n', '\r']) == DELIMITER {
            offset += end;
            closed = true;
            break;
        }
        frontmatter.push_str(&line.replace("\r\n", "\n"));
        offset += end;
    }
    if !closed {
        return Err(DocumentError::UnterminatedFrontmatter { opened_at_line: 1 });
    }
    // **Nesting past any a sample holds is refused before the parser reads
    // it**: it read `[[[…]]]` in a time that grew as its square — 120 kB held
    // every command reading the folder for 13 s — before refusing it.
    if let Some(line) = nested_too_deep(&frontmatter) {
        return Err(DocumentError::Yaml {
            line: Some(line + 1),
            message: format!(
                "brackets or braces nest deeper than {MAX_NESTING}, as no sample's fields do"
            ),
        });
    }

    // **A link written unquoted is a YAML list**, refused at its own line
    // with the quotes it needs: it was said *a list is not a scalar*, at the
    // line the fields began, or *did not find expected key*.
    if let Some((line, message)) = unquoted_link_said(&frontmatter) {
        return Err(DocumentError::Yaml {
            line: Some(line),
            message,
        });
    }

    // **A key written twice is refused**: the parser kept the last, every
    // command read it without a word, and a write dropped the other.
    if let Err(error) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&frontmatter)
        && let Some(rest) = error.to_string().split("duplicate entry with key ").nth(1)
    {
        let key = rest
            .split(" at line ")
            .next()
            .unwrap_or_default()
            .trim_matches('"')
            .to_string();
        // The parser places the mapping that holds the key, not the key:
        // `line 2` was said of a key on lines 8 and 9.
        let mapping = error.location().map_or(1, |at| at.line());
        let lines = key_lines(&frontmatter, &key, mapping);
        let (line, said) = match lines.as_slice() {
            [first, .., last] => (
                Some(first + 1),
                format!(
                    "'{key}' is written twice, at lines {} and {}: which of its values is \
                     meant cannot be told — keep one",
                    first + 1,
                    last + 1
                ),
            ),
            _ => (
                error.location().map(|at| at.line() + 1),
                format!(
                    "'{key}' is written twice: which of its values is meant cannot be told — \
                     keep one"
                ),
            ),
        };
        return Err(DocumentError::Yaml {
            line,
            message: said,
        });
    }
    let schema: SampleSchema = serde_yaml_ng::from_str(&frontmatter).map_err(|error| {
        // A line number relative to the **file**: one relative to a fragment
        // sends the reader to the wrong place, which is worse than none.
        let mut line = error.location().map(|at| at.line() + 1);
        let mut message = error.to_string();
        // One line number, ours: the parser's own, relative to the fragment,
        // would contradict it.
        if let Some(at) = message.rfind(" at line ")
            && message[at + " at line ".len()..].starts_with(|c: char| c.is_ascii_digit())
        {
            message.truncate(at);
        }
        // A precision is the project's: refused as the unknown field it is, and
        // told where it went rather than handed the list of fields a property
        // has, which says what to delete and not where to declare it.
        if message.contains("unknown field `precision`") {
            let at = message.split(": unknown field").next().unwrap_or_default();
            message = format!(
                "{at}: a file holds no precision — it is declared once for the project, as \
                 [property.{}] precision = \".3f\" in .samplekitrc",
                declared_as(at)
            );
        }
        // A YAML tag on a value SampleKit reads: serde's own words were about
        // enums, which no one writing a sample knows of.
        if message.contains("do not support enum input")
            && let Some(held) = error
                .location()
                .and_then(|at| frontmatter.lines().nth(at.line().saturating_sub(1)))
            && let Some(tag) = held.split_whitespace().find(|word| word.starts_with('!'))
        {
            let key = message.split(": ").next().unwrap_or_default();
            message = format!(
                "{key}: the YAML tag {tag} is not something SampleKit reads: remove it, \
                 and write the value alone"
            );
        }
        // Lines the parser counts from the frontmatter, said from the file,
        // whose first line is the `---` above it.
        message = from_the_file(&message);
        // serde's names of Rust's types, said as what the file should hold.
        message = plain_type_error(&message);
        // `{v: 1,062}` read as a key `062` after `1`: a decimal comma, which
        // *unknown field `062`* hid.
        if let Some(rest) = message.split("unknown field `").nth(1)
            && let Some(digits) = rest.split('`').next()
            && !digits.is_empty()
            && digits.chars().all(|c| c.is_ascii_digit())
        {
            message = format!(
                "{message} — a decimal comma? `,{digits}` after a number is read as a key \
                 of its own: write the number with a point"
            );
        }
        // A merge left unresolved: said as one, not as the YAML it breaks.
        if let Some(at) = frontmatter
            .lines()
            .position(|held| held.starts_with("<<<<<<<") || held.starts_with(">>>>>>>"))
        {
            line = Some(at + 2);
            message = "a git merge left its conflict markers here: keep one side, \
                       remove the markers, and the file reads again"
                .to_string();
        }
        // An attribute held as a mapping is reported where the frontmatter
        // starts: the key is found and named instead.
        if message.contains("a mapping is not an attribute")
            && let Some((key, at)) = mapping_attribute(&frontmatter)
        {
            line = Some(at);
            message = format!(
                "'{key}' is a mapping, and an attribute is one scalar or one homogeneous list"
            );
        }
        DocumentError::Yaml { line, message }
    })?;
    schema::check_version(schema.schema_version).map_err(DocumentError::Schema)?;
    // A table the file holds without a row is prepared, and written so.
    let mut schema = schema;
    for table in schema.tables.values_mut() {
        if table.rows.is_empty() {
            table.prepared = schema::Prepared(true);
        }
    }

    Ok(Document {
        schema,
        // Byte for byte: sliced from the source, never parsed, trimmed or
        // re-encoded.
        note: Note::new(&body[offset..]),
        bom,
        crlf,
    })
}

pub fn write(document: &Document) -> String {
    let mut out = String::new();
    if document.bom {
        out.push(BOM);
    }
    out.push_str(DELIMITER);
    out.push('\n');
    out.push_str(&canonicalization::write(&document.schema));
    out.push_str(DELIMITER);
    out.push('\n');
    if document.crlf {
        out = out.replace('\n', "\r\n");
    }
    // The note is appended after the conversion, so that its own line endings
    // are the ones it arrived with.
    out.push_str(document.note.as_str());
    out
}

// ------------------------------------------------------------------- files

pub fn load(path: &Path) -> Result<(Document, Origin), DocumentError> {
    let source = read_text(path).map_err(|source| {
        // What the file holds, not what the filesystem refused.
        if source.kind() == io::ErrorKind::InvalidData {
            DocumentError::NotText {
                path: path.to_path_buf(),
            }
        } else {
            DocumentError::Io {
                path: path.to_path_buf(),
                source,
            }
        }
    })?;
    let document = parse(&source)?;
    Ok((
        document,
        Origin {
            path: path.to_path_buf(),
            digest: digest_of(&source),
        },
    ))
}

/// Write atomically: a temporary file in the destination directory, then a
/// rename. A save interrupted midway must not leave a truncated file where a
/// someone's records were.
/// The exclusive section a save happens in: the lock beside the file, its
/// holder written in it. One whose writer was killed inside the section is
/// taken back, with the temporary that writer left.
struct Lock(#[allow(dead_code)] super::lock::Lock);

impl Lock {
    fn acquire(destination: &Path) -> Result<Lock, DocumentError> {
        let mut name = destination.as_os_str().to_os_string();
        name.push(".lock");
        let at = PathBuf::from(name);
        let left = |dead: Option<u32>| {
            if let Some(pid) = dead {
                let _ = fs::remove_file(temporary_of(destination, pid));
            }
        };
        match super::lock::Lock::take(&at, left) {
            Ok(lock) => Ok(Lock(lock)),
            Err(super::lock::Refused::Held(holder)) => Err(DocumentError::Locked {
                path: destination.to_path_buf(),
                holder: holder.map(|holder| holder.said()),
            }),
            // Said of the file being saved, never of the lock beside it: the
            // folder refusing a new file refuses the save.
            Err(super::lock::Refused::Io(source)) => Err(DocumentError::Io {
                path: destination.to_path_buf(),
                source: folder_refusing(source, destination),
            }),
        }
    }
}

/// serde's *invalid type: integer `3`, expected struct WrittenColumn* in the
/// words of a file: `3 where a column belongs, as {field = "radius"}`. A message
/// of another shape is returned as it is.
pub fn plain_type_error(message: &str) -> String {
    let Some(at) = message.find("invalid type: ") else {
        return message.to_string();
    };
    let rest = &message[at + "invalid type: ".len()..];
    let Some((found, expected)) = rest.split_once(", expected ") else {
        return message.to_string();
    };
    // What follows the expected type, `, in ``…``` where the caller added it.
    let (expected, after) = match expected.find(", in `") {
        Some(cut) => (&expected[..cut], &expected[cut..]),
        None => (expected, ""),
    };
    let quoted = |text: &str| -> Option<String> {
        let start = text.find(['`', '"'])?;
        let mark = text[start..].chars().next()?;
        let end = text[start + 1..].find(mark)?;
        Some(text[start + 1..start + 1 + end].to_string())
    };
    let found = if found.starts_with("integer") || found.starts_with("floating point") {
        quoted(found).map_or_else(
            || "a number".to_string(),
            |number| format!("the number {number}"),
        )
    } else if found.starts_with("string") {
        quoted(found).map_or_else(|| "text".to_string(), |text| format!("the text '{text}'"))
    } else if found.starts_with("boolean") {
        quoted(found).unwrap_or_else(|| "a yes-or-no".to_string())
    } else if found.starts_with("unit value") {
        "nothing".to_string()
    } else if found.starts_with("sequence") {
        "a list".to_string()
    } else if found.starts_with("map") {
        "keys and values".to_string()
    } else {
        found.to_string()
    };
    let expected = match expected {
        "struct WrittenColumn" => "a column belongs, as {field = \"radius\"}".to_string(),
        "f64" | "f32" | "a number" => "a number belongs".to_string(),
        "u8" | "u16" | "u32" | "u64" | "usize" | "i32" | "i64" | "a whole number" => {
            "a whole number belongs".to_string()
        }
        "a string" | "string" => "text belongs".to_string(),
        "a sequence" => "a list belongs".to_string(),
        "a map" => "keys and values belong".to_string(),
        "a boolean" => "true or false belongs".to_string(),
        other if other.starts_with("struct ") => "keys and values belong".to_string(),
        other => format!("{other} belongs"),
    };
    format!("{}{found} where {expected}{after}", &message[..at])
}

/// What a file holds that SampleKit reads otherwise than it is written, and
/// that the next write therefore drops or changes, each said as a sentence
/// naming its key: a `null`, a YAML tag, a date no calendar has — read as
/// text — and an uncertainty beside `n/a`.
pub fn changed_by_a_write(source: &str) -> Vec<String> {
    let body = source.strip_prefix(BOM).unwrap_or(source);
    let mut lines = body.lines();
    if lines.next().map(|line| line.trim_end_matches('\r')) != Some(DELIMITER) {
        return Vec::new();
    }
    let frontmatter: String = lines
        .take_while(|line| line.trim_end_matches('\r') != DELIMITER)
        .map(|line| format!("{line}\n"))
        .collect();
    if nested_too_deep(&frontmatter).is_some() {
        return Vec::new();
    }
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&frontmatter) else {
        return Vec::new();
    };
    let mut said = Vec::new();
    walk_for_changes(&value, "", &mut said);
    // A tag written twice is carried once: the next write drops the second,
    // which nothing said.
    if let Some(tags) = value
        .get("tags")
        .and_then(serde_yaml_ng::Value::as_sequence)
    {
        let mut seen: Vec<&serde_yaml_ng::Value> = Vec::new();
        for tag in tags {
            if seen.contains(&tag) {
                if let Some(name) = tag.as_str() {
                    said.push(format!(
                        "tags: '{name}' is written twice: the next write carries it once"
                    ));
                }
            } else {
                seen.push(tag);
            }
        }
    }
    // A tag of YAML's own, `!!binary`, is read away by the parser itself:
    // found in the text, where the value no longer carries it.
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_start_matches("- ");
        // The tag opens the value: `!!` further in is text.
        if let Some(tag) = value
            .split_whitespace()
            .next()
            .filter(|word| word.starts_with("!!"))
        {
            said.push(format!(
                "{key} carries the YAML tag {tag}: the next write drops it"
            ));
        }
    }
    // A number below the smallest a file holds, `1e-400`, read as 0: said,
    // since the next write writes `0.0` and no preview showed it.
    for line in frontmatter.lines() {
        let key = line.split(':').next().unwrap_or_default().trim();
        for token in line
            .split(|c: char| c.is_whitespace() || matches!(c, ':' | ',' | '[' | ']' | '{' | '}'))
        {
            let lowered = token.to_ascii_lowercase();
            let (mantissa, exponent) = match lowered.split_once('e') {
                Some(parts) => parts,
                None => continue,
            };
            if exponent.parse::<i32>().is_ok()
                && mantissa.chars().any(|c| matches!(c, '1'..='9'))
                && token.parse::<f64>() == Ok(0.0)
            {
                said.push(format!(
                    "{key}: {token} is smaller than any number a file holds, read as 0: the \
                     next write writes 0.0"
                ));
            }
        }
    }
    said
}

fn walk_for_changes(value: &serde_yaml_ng::Value, key: &str, said: &mut Vec<String>) {
    use serde_yaml_ng::Value as Yaml;
    match value {
        Yaml::Null if !key.is_empty() => {
            said.push(format!("{key} is null: the next write removes it"));
        }
        Yaml::Tagged(tagged) => {
            said.push(format!(
                "{key} carries the YAML tag {}: the next write drops it",
                tagged.tag
            ));
            walk_for_changes(&tagged.value, key, said);
        }
        Yaml::String(text)
            if looks_like_a_date(text) && crate::core::value::Date::parse(text).is_err() =>
        {
            said.push(format!(
                "{key}: {text} is no date — no such day — and is read as text"
            ));
        }
        Yaml::Mapping(mapping) => {
            let not_applicable = mapping
                .get("v")
                .and_then(Yaml::as_str)
                .is_some_and(|text| text == "n/a");
            if not_applicable && mapping.get("u").is_some_and(|u| !u.is_null()) {
                said.push(format!(
                    "{key} is n/a beside an uncertainty, which says nothing of it: the \
                     next write drops the uncertainty"
                ));
            }
            for (inner, value) in mapping {
                let name = match inner {
                    Yaml::String(name) => name.clone(),
                    other => serde_yaml_ng::to_string(other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                // The reserved keys keep their parents' name: `properties.fg`
                // is said `notch`.
                let joined = if matches!(key, "" | "properties" | "tables" | "rows" | "columns") {
                    name
                } else {
                    format!("{key}.{name}")
                };
                walk_for_changes(value, &joined, said);
            }
        }
        Yaml::Sequence(items) => {
            for item in items {
                walk_for_changes(item, key, said);
            }
        }
        _ => {}
    }
}

/// `2026-02-30`: four digits, two, two.
fn looks_like_a_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit())
}

/// A frontmatter the YAML reader refused, said as the file shows it: a merge
/// left unresolved named as one, and the lines counted from the file.
pub fn frontmatter_refused(frontmatter: &str, message: &str) -> String {
    if let Some(at) = frontmatter
        .lines()
        .position(|held| held.starts_with("<<<<<<<") || held.starts_with(">>>>>>>"))
    {
        return format!(
            "line {}: a git merge left its conflict markers here: keep one side, remove \
             the markers, and the file reads again",
            at + 2
        );
    }
    from_the_file(message)
}

/// A YAML message whose `line N` counts from the frontmatter's first line,
/// said from the file's: one more, for the `---` above it.
pub fn from_the_file(message: &str) -> String {
    let mut said = String::new();
    let mut rest = message;
    while let Some(at) = rest.find("line ") {
        let (before, after) = rest.split_at(at + "line ".len());
        said.push_str(before);
        let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
        match digits.parse::<usize>() {
            Ok(line) => {
                said.push_str(&(line + 1).to_string());
                rest = &after[digits.len()..];
            }
            Err(_) => rest = after,
        }
    }
    said.push_str(rest);
    said
}

/// The first top-level key, outside the reserved ones, whose value is a
/// mapping, and its line in the file (the opening `---` is line 1).
fn mapping_attribute(frontmatter: &str) -> Option<(String, usize)> {
    const RESERVED: [&str; 5] = ["schema_version", "name", "tags", "properties", "tables"];
    let lines: Vec<&str> = frontmatter.lines().collect();
    for (at, line) in lines.iter().enumerate() {
        if line.starts_with([' ', '\t', '#', '-']) {
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if RESERVED.contains(&key) {
            continue;
        }
        let rest = rest.trim();
        let nested = rest.is_empty()
            && lines[at + 1..]
                .iter()
                .find(|next| !next.trim().is_empty())
                .is_some_and(|next| next.starts_with(' ') && !next.trim_start().starts_with('-'));
        if rest.starts_with('{') || nested {
            return Some((key.to_string(), at + 2));
        }
    }
    None
}

/// A path that is a symbolic link leading out of the project it lies in — the
/// nearest folder above the link holding a `.samplekitrc` — refused as a place
/// to write: written through, its file changed in another folder, with no
/// snapshot of it and nothing of that project's configuration. Read, it is
/// what the link says. A link inside its project, or in none, is written.
pub fn refuse_link_out_of_project(path: &Path) -> Result<(), DocumentError> {
    if !fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Ok(());
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let Some(project) = absolute
        .ancestors()
        .skip(1)
        .find(|folder| folder.join(".samplekitrc").is_file())
    else {
        return Ok(());
    };
    let own = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let target = own(&absolute);
    if target.starts_with(own(project)) {
        return Ok(());
    }
    Err(DocumentError::LinkLeavesProject {
        path: path.to_path_buf(),
        target,
        project: project.to_path_buf(),
    })
}

/// The saves held rather than written while a script's changes are shown
/// before they are: by file, what it held before the first save, and what it
/// would hold. `None` while saves write.
type Held = IndexMap<PathBuf, (Option<Vec<u8>>, String)>;
static HELD: std::sync::Mutex<Option<Held>> = std::sync::Mutex::new(None);

/// From now on in this process, a save is checked as ever and held, not
/// written: `held` gives what the saves would write.
pub fn hold_saves() {
    if let Ok(mut held) = HELD.lock() {
        held.get_or_insert_with(IndexMap::new);
    }
}

/// Whether saves are held.
pub fn holding_saves() -> bool {
    HELD.lock().is_ok_and(|held| held.is_some())
}

/// Each file a held save would change: what it held before, none where it
/// did not exist, and what it would hold.
pub fn held_saves() -> Vec<(PathBuf, Option<Vec<u8>>, String)> {
    HELD.lock()
        .ok()
        .and_then(|held| held.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|(path, (before, after))| (path, before, after))
        .collect()
}

/// What a file holds as a save sees it: what a held save wrote there, or the
/// disk's.
pub fn current_text(path: &Path) -> io::Result<String> {
    if let Ok(held) = HELD.lock()
        && let Some((_, after)) = held.as_ref().and_then(|held| held.get(path))
    {
        return Ok(after.clone());
    }
    read_text(path)
}

/// A file removed: at once, or waiting with the project's other changes.
pub fn remove_file(path: &Path) -> io::Result<()> {
    if super::stage::staging(path) {
        return super::stage::remove(path).map_err(|error| io::Error::other(error.to_string()));
    }
    note_written(path);
    fs::remove_file(path)
}

/// A file moved — a sample's own, renamed with it: at once, or waiting.
pub fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    if super::stage::staging(from) {
        return super::stage::moved(from, to).map_err(|error| io::Error::other(error.to_string()));
    }
    note_written(from);
    note_written(to);
    fs::rename(from, to)
}

/// What a file reads as: a change waiting for it, or the disk's.
pub fn read_text(path: &Path) -> io::Result<String> {
    match super::stage::text(path) {
        Some(Some(text)) => Ok(text),
        Some(None) => Err(io::Error::new(
            io::ErrorKind::NotFound,
            "removed, the removal not written yet",
        )),
        None => fs::read_to_string(path),
    }
}

pub fn save(document: &Document, to: &Destination) -> Result<Origin, DocumentError> {
    let path = to.path().to_path_buf();
    refuse_link_out_of_project(&path)?;
    let staging = super::stage::staging(&path) && !holding_saves();
    // Held across the check, the write and the rename: two syscalls with
    // anything at all between them is not a check. A save that waits writes
    // nothing beside the file.
    let _lock = if staging {
        None
    } else {
        Some(Lock::acquire(&path)?)
    };
    // A write that would change nothing writes nothing: not the file, not its
    // time, not its permissions.
    if let Ok(current) = current_text(&path)
        && current == write(document)
    {
        return Ok(Origin {
            digest: digest_of(&current),
            path,
        });
    }
    // A file its owner made read-only is not rewritten behind their back.
    if fs::metadata(&path).is_ok_and(|metadata| metadata.permissions().readonly()) {
        return Err(DocumentError::Io {
            path: path.clone(),
            source: io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the file is read-only; make it writable to change it",
            ),
        });
    }
    // Looked for again inside the lock: a file made there since the command
    // looked is another writer's, and is not replaced.
    if let Destination::New(path) = to
        && ((!staging && fs::symlink_metadata(path).is_ok())
            || (staging && super::stage::exists(path))
            || current_text(path).is_ok())
    {
        return Err(DocumentError::WrittenSince { path: path.clone() });
    }
    if let Destination::Origin(origin) = to {
        let current = current_text(&origin.path).map_err(|source| DocumentError::Io {
            path: origin.path.clone(),
            source,
        })?;
        if digest_of(&current) != origin.digest {
            return Err(DocumentError::ConcurrentEdit { path });
        }
    }

    let rendered = write(document);
    // Nothing is written that would not read back as this document: a file
    // SampleKit cannot open again is the one outcome a save must never have.
    match parse(&rendered) {
        // Compared as text: what is kept in memory and never written, an
        // attribute without a value, is not a difference.
        Ok(back) if write(&back) == rendered => {}
        Ok(back) => {
            // The line that changes, so that the field is named: a text shaped
            // as a date is read back as a date, and the message named nothing.
            let again = write(&back);
            let differs = rendered
                .lines()
                .zip(again.lines())
                .find(|(written, read)| written != read)
                .map(|(written, read)| {
                    let date = read.split_once(": ").is_some_and(|(_, value)| {
                        crate::core::value::Date::parse(value.trim()).is_ok()
                            || crate::core::value::DateTime::parse(value.trim()).is_ok()
                    });
                    format!(
                        "`{}` reads back as `{}`{}",
                        written.trim(),
                        read.trim(),
                        if date {
                            " — a text shaped as a date is read as a date: give the date \
                             itself, or write the text another way"
                        } else {
                            ""
                        }
                    )
                });
            return Err(DocumentError::NotReadBack {
                path,
                reason: differs
                    .unwrap_or_else(|| "it reads back as a different document".to_string()),
            });
        }
        Err(error) => {
            return Err(DocumentError::NotReadBack {
                path,
                reason: error.to_string(),
            });
        }
    }
    if let Ok(mut held) = HELD.lock()
        && let Some(held) = held.as_mut()
    {
        let before = fs::read(&path).ok();
        held.entry(path.clone())
            .or_insert((before, String::new()))
            .1 = rendered.clone();
    } else if staging {
        super::stage::save(&path, &rendered)?;
    } else {
        write_atomically(&path, rendered.as_bytes()).map_err(|source| DocumentError::Io {
            path: path.clone(),
            source,
        })?;
    }
    Ok(Origin {
        path,
        digest: digest_of(&rendered),
    })
}

/// A sample with no formulas and no runtime dependency graph, carrying whatever
/// records the file held. Everything except recomputation works in that state,
/// including a freshness check — which is the point of the records being in the
/// file at all.
pub fn load_sample(path: &Path) -> Result<(Sample, Origin), DocumentError> {
    sample_of(load(path)?, path)
}

/// The sample a document read from `path` holds: what [`load_sample`] makes
/// of what [`load`] read, the reading done apart, on another thread.
pub fn sample_of(
    (document, origin): (Document, Origin),
    path: &Path,
) -> Result<(Sample, Origin), DocumentError> {
    let mut sample = schema::into_sample(document.schema).map_err(DocumentError::Schema)?;
    sample.set_note(document.note.as_str().to_string());
    // Named by its file where it writes no name.
    sample.set_file(Some(path));
    Ok((sample, origin))
}

/// A sample read from `text`, as the file at `path` held it once — a
/// version the history kept — named by that file where it writes no name.
pub fn sample_from_text(text: &str, path: &Path) -> Result<Sample, DocumentError> {
    let document = parse(text)?;
    let mut sample = schema::into_sample(document.schema).map_err(DocumentError::Schema)?;
    sample.set_note(document.note.as_str().to_string());
    sample.set_file(Some(path));
    Ok(sample)
}

/// A model's sample filled by a file: its declarations stay, and the file's
/// values and note arrive. The loading path of a sample whose class declared
/// properties before the file was read.
pub fn load_into(path: &Path, sample: &mut Sample) -> Result<Origin, DocumentError> {
    let (file, origin) = load_sample(path)?;
    // A quantity the model declares written in the file at the top level, as
    // an attribute — data moved in before the model said what it is: said as
    // such, with the command that moves it, where *'mass' is already a
    // property* named neither the file nor the way out.
    if let Some(name) = sample
        .property_names()
        .into_iter()
        .find(|name| file.has_attribute(name))
    {
        let held = file
            .attribute(name)
            .ok()
            .map(|value| match value {
                crate::core::sample::AttributeValue::Scalar(value) => {
                    crate::collection::editing::shown_value(value)
                }
                crate::core::sample::AttributeValue::List(_) => "…".to_string(),
            })
            .unwrap_or_else(|| "…".to_string());
        let line = std::fs::read_to_string(path).ok().and_then(|text| {
            text.lines()
                .position(|line| line.starts_with(&format!("{name}:")))
                .map(|at| at + 1)
        });
        return Err(DocumentError::Yaml {
            line,
            message: format!(
                "'{name}' is written as an attribute, at the top level, and the model declares \
                 it a property: samplekit {} set {name}={held} writes it as one",
                std::env::current_dir()
                    .ok()
                    .and_then(|here| path.strip_prefix(here).ok().map(Path::to_path_buf))
                    .unwrap_or_else(|| path.to_path_buf())
                    .display()
            ),
        });
    }
    sample
        .fill_from(file)
        .map_err(|error| DocumentError::Schema(SchemaError::Sample(error)))?;
    // A computed value edited by hand in the file is an override nobody marked,
    // and is held as one. A hold changes no value, so the pass stands.
    let names: Vec<Identifier> = sample.property_names().into_iter().cloned().collect();
    let _pass = fingerprint::Pass::begin();
    for name in &names {
        // A value beside readings with no record of the statistic that gave it
        // is a value written, whoever wrote it, and it is read as it is
        // written rather than guessed at by its digits. A value with no record of its formula
        // at all is held the same way, and said as such. A formula's product is
        // its value, its uncertainty, or both: the record belongs to the
        // property.
        let derived =
            |handle: &PropertyHandle| handle.is_computed() || handle.has_uncertainty_formula();
        if let Ok(handle) = sample.property(name)
            && derived(&handle)
            && handle.hold_record_missing()
        {
            continue;
        }
        if let Ok(handle) = sample.property(name)
            && derived(&handle)
            && !handle.is_edited()
            && matches!(
                fingerprint::check_property(sample, name),
                Ok(fingerprint::Freshness::Edited)
            )
        {
            // Unmarked in the file, and kept so until it changes.
            let unmarked = handle
                .records()
                .fingerprint
                .as_ref()
                .is_some_and(|recorded| !recorded.is_edited());
            if unmarked {
                handle.hold_as_edited_unmarked();
            } else {
                handle.hold_as_edited();
            }
        }
        // A value written beside readings is what every file holds, so it is an
        // override only where its record does not vouch for it: marked edited,
        // a digest that moved, or no record at all. Current or stale, it is the
        // declared statistic's own product.
        if let Ok(handle) = sample.property(name)
            && handle.peek(Property::holds_written_override)
            && matches!(
                fingerprint::check_property(sample, name),
                Ok(fingerprint::Freshness::Current
                    | fingerprint::Freshness::Stale { .. }
                    | fingerprint::Freshness::Broken { .. })
            )
        {
            handle.confirm_written_as_recorded();
        }
    }
    // A derived cell edited by hand in the file, marked so, or supplied without
    // the record of its formula, is held as an override too.
    //
    // Every cell is judged before any is held, in one check: a held cell was
    // judged edited or record missing, and stays so, and what reads it counts
    // either the same way — so holding first would change no verdict.
    let mut to_hold = Vec::new();
    {
        let _verdicts = fingerprint::Verdicts::begin();
        let tables: Vec<Identifier> = sample.table_names().into_iter().cloned().collect();
        for table in &tables {
            let Ok(held) = sample.table(table) else {
                continue;
            };
            let columns: Vec<Identifier> = held
                .column_names()
                .into_iter()
                .filter(|column| held.is_derived(column))
                .cloned()
                .collect();
            let indexes: Vec<Vec<crate::core::value::Value>> = held
                .index_tuples()
                .into_iter()
                .map(|tuple| tuple.into_iter().cloned().collect())
                .collect();
            for column in &columns {
                for index in &indexes {
                    if matches!(
                        fingerprint::check_cell(sample, table, column, index),
                        Ok(fingerprint::Freshness::Edited | fingerprint::Freshness::RecordMissing)
                    ) {
                        to_hold.push((table.clone(), index.clone(), column.clone()));
                    }
                }
            }
        }
    }
    for (table, index, column) in to_hold {
        let _ = sample.hold_cell(
            &table,
            &crate::core::table::RowAddress::Index(index),
            &column,
        );
    }
    Ok(origin)
}

/// Replaces `path` with `contents` — removes it, given none — only if it still
/// holds `expected`, `None` meaning that it did not exist. The file is read
/// again inside the lock that covers the write: what the command read and
/// showed is what it replaces, and another writer's change since is refused,
/// `ConcurrentEdit`, rather than lost.
pub fn replace_if_unchanged(
    path: &Path,
    expected: Option<&[u8]>,
    contents: Option<&[u8]>,
) -> Result<(), DocumentError> {
    if super::stage::staging(path) {
        refuse_link_out_of_project(path)?;
        let current = read_text(path).ok();
        if current.as_deref().map(str::as_bytes) != expected {
            return Err(match expected {
                None => DocumentError::WrittenSince {
                    path: path.to_path_buf(),
                },
                Some(_) => DocumentError::ConcurrentEdit {
                    path: path.to_path_buf(),
                },
            });
        }
        return match contents {
            Some(contents) => super::stage::save(path, &String::from_utf8_lossy(contents)),
            None if current.is_some() => super::stage::remove(path),
            None => Ok(()),
        };
    }
    replace_on_disk(path, expected, contents)
}

/// [`replace_if_unchanged`] on the disk itself, whatever waits: what writes
/// the changes waiting.
pub(crate) fn replace_on_disk(
    path: &Path,
    expected: Option<&[u8]>,
    contents: Option<&[u8]>,
) -> Result<(), DocumentError> {
    refuse_link_out_of_project(path)?;
    let _lock = Lock::acquire(path)?;
    let io = |source: io::Error| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    };
    let current = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(io(error)),
    };
    // A file its owner made read-only is not replaced behind their back, as
    // `save` refuses it: the rename would go through where a write would not.
    if contents.is_some()
        && fs::metadata(path).is_ok_and(|metadata| metadata.permissions().readonly())
    {
        return Err(io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the file is read-only; make it writable to change it",
        )));
    }
    if current.as_deref() != expected {
        return Err(match expected {
            None => DocumentError::WrittenSince {
                path: path.to_path_buf(),
            },
            Some(_) => DocumentError::ConcurrentEdit {
                path: path.to_path_buf(),
            },
        });
    }
    match contents {
        Some(contents) => write_atomically(path, contents).map_err(io),
        None if current.is_some() => {
            note_written(path);
            fs::remove_file(path).map_err(io)
        }
        None => Ok(()),
    }
}

/// The one way this project replaces a file: a temporary file beside the
/// destination, flushed to the device, given the destination's permissions,
/// renamed over it, and the directory flushed after.
///
/// The rename alone survives a killed process and a full disk. It does not
/// survive a power cut: a filesystem may commit the rename before the data, and
/// what is left is an empty file where someone's records were. A failure at
/// any step removes the temporary file and leaves the destination as it was.
/// A new file refused beside `path`, said as its folder refusing it.
fn folder_refusing(error: io::Error, path: &Path) -> io::Error {
    if error.kind() != io::ErrorKind::PermissionDenied {
        return error;
    }
    let folder = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    io::Error::new(
        error.kind(),
        format!(
            "its folder {} cannot be written in, and nothing was written",
            folder.display()
        ),
    )
}

/// The temporary a writer of this process id writes `path` through, beside it.
fn temporary_of(path: &Path, pid: u32) -> PathBuf {
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    directory.join(format!(
        ".{}.{pid}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("samplekit"),
    ))
}

/// Every file this process wrote or removed, in order, as a path its folder
/// resolved: what a command wrote is read from it, so that its snapshot keeps
/// its own files and no other command's written meanwhile.
static WRITTEN: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// `path` noted as written, or removed, by this process.
pub fn note_written(path: &Path) {
    let resolved = match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let parent = if parent.as_os_str().is_empty() {
                Path::new(".")
            } else {
                parent
            };
            dunce::canonicalize(parent)
                .map(|parent| parent.join(name))
                .unwrap_or_else(|_| path.to_path_buf())
        }
        _ => path.to_path_buf(),
    };
    // The journals of the command that started this process too: a model's
    // worker writes the files its command computes, and they went unseen by
    // that command's snapshot and by every other command meanwhile.
    let given: Vec<PathBuf> = std::env::var_os(JOURNALS_GIVEN)
        .map(|joined| std::env::split_paths(&joined).collect())
        .unwrap_or_default();
    if let Ok(journals) = JOURNALS.lock() {
        for journal in journals.iter().chain(&given) {
            use std::io::Write as _;
            if let Ok(mut file) = fs::OpenOptions::new().append(true).open(journal) {
                let _ = writeln!(file, "wrote {}", resolved.display());
            }
        }
    }
    if let Ok(mut written) = WRITTEN.lock() {
        written.push(resolved);
    }
}

/// The journals each file written is added to, so that another process sees
/// which files a write under way has written.
static JOURNALS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// The variable a process started by a command — the model's worker — reads
/// that command's journals from, to add the files it writes to them.
pub const JOURNALS_GIVEN: &str = "SAMPLEKIT_JOURNALS";

/// The journals this process writes to, joined as a variable holds paths:
/// what a process it starts is given.
pub fn journals_given() -> std::ffi::OsString {
    let journals = JOURNALS.lock().map(|held| held.clone()).unwrap_or_default();
    std::env::join_paths(journals).unwrap_or_default()
}

/// `line` added to each journal this process writes to: what the next
/// command reads there where this one is interrupted.
pub fn to_journals(line: &str) {
    if let Ok(journals) = JOURNALS.lock() {
        for journal in journals.iter() {
            use std::io::Write as _;
            if let Ok(mut file) = fs::OpenOptions::new().append(true).open(journal) {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

/// The files `journal` says were written, whoever wrote them.
pub fn written_in(journal: &Path) -> Vec<PathBuf> {
    fs::read_to_string(journal)
        .map(|text| {
            text.lines()
                .filter_map(|line| line.strip_prefix("wrote "))
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Each file this process writes from now on added to `journal`.
pub fn journal_opened(journal: &Path) {
    if let Ok(mut journals) = JOURNALS.lock() {
        journals.push(journal.to_path_buf());
    }
}

/// `journal` no longer written to.
pub fn journal_closed(journal: &Path) {
    if let Ok(mut journals) = JOURNALS.lock() {
        journals.retain(|held| held != journal);
    }
}

/// How many files this process has written so far: a mark to read the files
/// written since with [`written_since`].
pub fn written_mark() -> usize {
    WRITTEN.lock().map_or(0, |written| written.len())
}

/// The files this process wrote or removed since `mark`.
pub fn written_since(mark: usize) -> Vec<PathBuf> {
    WRITTEN
        .lock()
        .map(|written| written.iter().skip(mark).cloned().collect())
        .unwrap_or_default()
}

pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
    note_written(path);
    // Through a link, the file it points at is written: renaming onto the
    // link replaced it with a copy, and the two drifted apart silently.
    let resolved = fs::read_link(path)
        .ok()
        .and_then(|_| dunce::canonicalize(path).ok());
    let path = resolved.as_deref().unwrap_or(path);
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    // The process id is what keeps two writers at one destination from writing
    // through the same temporary and renaming what the other left.
    let temporary = temporary_of(path, std::process::id());
    let written = (|| {
        // Refused here, it is the folder that refuses, whatever the file.
        let mut file =
            fs::File::create(&temporary).map_err(|error| folder_refusing(error, path))?;
        file.write_all(contents)?;
        file.sync_all()?;
        // The file keeps the permissions it had; a new file keeps the default.
        // Best effort: a filesystem without permissions still gets its data.
        if let Ok(metadata) = fs::metadata(path) {
            let _ = fs::set_permissions(&temporary, metadata.permissions());
        }
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        // Best effort: the error worth reporting is the one that stopped the write.
        let _ = fs::remove_file(&temporary);
        return written;
    }
    // The rename itself is in the directory. Not every platform can flush one,
    // and the data is already safe, so this failing is not the write failing.
    if let Ok(held) = fs::File::open(directory) {
        let _ = held.sync_all();
    }
    Ok(())
}

/// Writes a sample whose records it did not produce and does not touch. A part
/// its file held and could not read is dropped by any write, so none is made.
fn refuse_set_aside(sample: &Sample) -> Result<(), DocumentError> {
    let parts: Vec<String> = sample
        .set_aside_tables()
        .iter()
        .map(|(table, _)| format!("table {table}"))
        .collect();
    if parts.is_empty() {
        Ok(())
    } else {
        Err(DocumentError::PartSetAside { parts })
    }
}

pub fn save_sample(sample: &mut Sample, to: &Destination) -> Result<Origin, DocumentError> {
    refuse_set_aside(sample)?;
    sample
        .materialize()
        .map_err(|error| DocumentError::Schema(SchemaError::Sample(error)))?;
    let document = document_of(sample, existing(to))?;
    let origin = save(&document, to)?;
    // Its file names it now, where it writes no name.
    sample.set_file(Some(&origin.path));
    Ok(origin)
}

/// Writes a sample a model computes, **running no formula**.
///
/// Every value is written as it stands and every record as the value carries
/// it, so a stale value keeps the record that says so, and a value never
/// computed is written absent. A value computed in this session is recorded
/// first, while its inputs are still the ones it came from; one whose inputs
/// moved before anything recorded it is refused, rather than written as current
/// or as a value somebody typed.
/// An input's own fingerprint written again: legibility, what it hashes to
/// now beside what a derived value recorded it as then.
fn refresh_input_fingerprints(sample: &Sample, schema: &mut SampleSchema) {
    // An input's own fingerprint is legibility: what it hashes to now, beside
    // what a derived value recorded it as then.
    for name in sample.property_names() {
        for node in sample.dependencies_of(name).unwrap_or_default() {
            // Refreshed at every save, so the file never contradicts itself; an
            // override keeps its mark. A failed value has none to hash: its
            // failure stands where the digest would.
            if let Node::Named(input) = node
                && let Some(shape) = schema.properties.get_mut(&input)
                && shape.failure.is_none()
                && shape
                    .fingerprint
                    .as_ref()
                    .is_none_or(|fingerprint| !fingerprint.is_edited())
            {
                // By the same rule the record was written under: a property
                // recording its own readings holds the quantity without them,
                // and refreshing it by `of` here undid what `stamp` had just
                // stored, reporting an untouched sample as edited. And over the
                // channel its formula produced, which the record itself says:
                // refreshing it over the whole quantity would restore the
                // digest that called an entered value edited. Read back by the
                // one rule a reader uses: a hand-made copy of it here once
                // lacked the case of common inputs beside a channel's, and
                // called a value nothing touched edited.
                let produced = shape.computed.as_ref().and_then(schema::channel_of_record);
                shape.fingerprint = Some(fingerprint::own_of(shape, produced));
            }
        }
    }
}

pub fn save_computed(sample: &mut Sample, to: &Destination) -> Result<Origin, DocumentError> {
    refuse_set_aside(sample)?;
    let unrecordable = fingerprint::stamp(sample).map_err(|error| DocumentError::Compute {
        property: Identifier::new("_").expect("a placeholder is a name"),
        reason: error.to_string(),
    })?;
    if !unrecordable.is_empty() {
        return Err(DocumentError::Unrecordable {
            values: unrecordable,
        });
    }
    let mut schema = schema::from_sample_as_is(sample).map_err(DocumentError::Schema)?;
    refresh_input_fingerprints(sample, &mut schema);
    let layout = existing(to);
    let document = Document {
        schema,
        note: Note::new(sample.note()),
        bom: layout.0,
        crlf: layout.1,
    };
    let origin = save(&document, to)?;
    sample.set_file(Some(&origin.path));
    Ok(origin)
}

/// A save after no computation: of `sample` only the fields in `touched` — a
/// property, a table or an attribute by its name, `tags`, `name`, `note` —
/// and the file as it was read for every other: what a script changed, and
/// nothing the model gave the sample as it was read — a statistic of
/// readings taken on reading, a unit declared for a value the file does not
/// hold. A file that cannot be read back is written whole.
pub fn save_changed(
    sample: &mut Sample,
    to: &Destination,
    touched: &std::collections::BTreeSet<String>,
    read: Option<&str>,
) -> Result<Origin, DocumentError> {
    // The file as it was read, not as it is: a change made to it since is
    // refused by the save below, never taken in.
    let Some(file) = read.and_then(|text| parse(text).ok()) else {
        return save_computed(sample, to);
    };
    refuse_set_aside(sample)?;
    // Nothing is stamped: no value was computed, and one the model derived as
    // the sample was read is no change of the script's.
    let mut current = schema::from_sample_as_is(sample).map_err(DocumentError::Schema)?;
    refresh_input_fingerprints(sample, &mut current);
    let merged = merged(file.schema, current, touched);
    let note = if touched.contains("note") {
        Note::new(sample.note())
    } else {
        file.note
    };
    let document = Document {
        schema: merged,
        note,
        bom: file.bom,
        crlf: file.crlf,
    };
    let origin = save(&document, to)?;
    sample.set_file(Some(&origin.path));
    Ok(origin)
}

/// The file's fields in its order, each `touched` one as the sample holds it
/// now — or gone, where the sample holds it no more — and those it adds after.
fn merged(
    file: SampleSchema,
    current: SampleSchema,
    touched: &std::collections::BTreeSet<String>,
) -> SampleSchema {
    fn by_name<V: Clone>(
        file: IndexMap<Identifier, V>,
        current: &IndexMap<Identifier, V>,
        touched: &std::collections::BTreeSet<String>,
    ) -> IndexMap<Identifier, V> {
        let mut out = IndexMap::new();
        for (name, shape) in file {
            if touched.contains(name.as_str()) {
                if let Some(now) = current.get(&name) {
                    out.insert(name, now.clone());
                }
            } else {
                out.insert(name, shape);
            }
        }
        for (name, now) in current {
            if touched.contains(name.as_str()) && !out.contains_key(name) {
                out.insert(name.clone(), now.clone());
            }
        }
        out
    }
    // A property computed again to what the file holds already — its value,
    // uncertainty, readings, unit, record and failure — keeps the file's form:
    // a rerun that changes nothing writes nothing, and the model's statistic
    // declared, or an input's fingerprint, is no change of the script's.
    let mut current = current;
    for (name, now) in current.properties.iter_mut() {
        if let Some(held) = file.properties.get(name)
            && held.value == now.value
            && held.uncertainty == now.uncertainty
            && held.readings == now.readings
            && held.unit == now.unit
            && held.symbol == now.symbol
            && held.computed == now.computed
            && held.failure == now.failure
            && held
                .fingerprint
                .as_ref()
                .is_some_and(|mark| mark.is_edited())
                == now
                    .fingerprint
                    .as_ref()
                    .is_some_and(|mark| mark.is_edited())
        {
            *now = held.clone();
        }
    }
    SampleSchema {
        schema_version: current.schema_version,
        name: if touched.contains("name") {
            current.name.clone()
        } else {
            file.name
        },
        tags: if touched.contains("tags") {
            current.tags.clone()
        } else {
            file.tags
        },
        properties: by_name(file.properties, &current.properties, touched),
        tables: by_name(file.tables, &current.tables, touched),
        attributes: by_name(file.attributes, &current.attributes, touched),
    }
}

fn document_of(sample: &Sample, layout: (bool, bool)) -> Result<Document, DocumentError> {
    Ok(Document {
        schema: schema::from_sample(sample).map_err(DocumentError::Schema)?,
        note: Note::new(sample.note()),
        bom: layout.0,
        crlf: layout.1,
    })
}

/// A file's existing convention, so that saving over it changes neither.
fn existing(to: &Destination) -> (bool, bool) {
    match current_text(to.path()) {
        Ok(source) => (
            source.starts_with(BOM),
            source
                .strip_prefix(BOM)
                .unwrap_or(&source)
                .split('\n')
                .next()
                .is_some_and(|line| line.ends_with('\r')),
        ),
        Err(_) => (false, false),
    }
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum DocumentError {
    MissingFrontmatter,
    UnterminatedFrontmatter {
        opened_at_line: usize,
    },
    Yaml {
        line: Option<usize>,
        message: String,
    },
    Schema(SchemaError),
    ConcurrentEdit {
        path: PathBuf,
    },
    /// A file written where none was when the command looked.
    WrittenSince {
        path: PathBuf,
    },
    /// `holder` as the lock says it, where it says it.
    Locked {
        path: PathBuf,
        holder: Option<String>,
    },
    Compute {
        property: Identifier,
        reason: String,
    },
    Unrecordable {
        values: Vec<String>,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// A file that is not UTF-8 text: saved as UTF-16, or no text at all.
    NotText {
        path: PathBuf,
    },
    /// A symbolic link leading out of the project it lies in: read, never
    /// written through.
    LinkLeavesProject {
        path: PathBuf,
        target: PathBuf,
        project: PathBuf,
    },
    /// The text rendered for a file would not read back as what was saved, so
    /// nothing was written.
    NotReadBack {
        path: PathBuf,
        reason: String,
    },
    /// A sample holding a part its file could not read: a write would drop it.
    PartSetAside {
        parts: Vec<String>,
    },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::NotText { path } => write!(
                f,
                "{} is not UTF-8 text, which a sample is written in: saved as UTF-16 by \
                 an editor, or not text at all",
                path.display()
            ),
            DocumentError::PartSetAside { parts } => write!(
                f,
                "not written: {} could not be read, and a write would drop it; repair the file first",
                parts.join(", ")
            ),
            DocumentError::NotReadBack { path, reason } => write!(
                f,
                "{} was not written: the text rendered for it does not read back ({reason})",
                path.display()
            ),
            DocumentError::Unrecordable { values } => write!(
                f,
                "{} computed in this session from inputs that have changed since, and \
                 nothing recorded what those inputs were, so no record of them can be \
                 written: compute them again, then save\n  {}",
                if values.len() == 1 {
                    "this value was"
                } else {
                    "these values were"
                },
                values.join(", ")
            ),
            DocumentError::MissingFrontmatter => write!(
                f,
                "this file has no frontmatter: a sample begins with --- on its \
                 first line. A file without one is not read as a note-only \
                 document, because something already claimed it was a sample"
            ),
            DocumentError::UnterminatedFrontmatter { opened_at_line } => write!(
                f,
                "the frontmatter opened at line {opened_at_line} is never closed: \
                 add a --- on its own line where the data ends"
            ),
            DocumentError::Yaml { line, message } => match line {
                Some(line) => write!(f, "line {line}: {message}"),
                None => write!(f, "{message}"),
            },
            DocumentError::Schema(error) => write!(f, "{error}"),
            DocumentError::ConcurrentEdit { path } => write!(
                f,
                "{} changed since it was read, and saving would discard that \
                 change. Reload and redo the edit, or write to a path having \
                 looked at what moved",
                path.display()
            ),
            DocumentError::LinkLeavesProject {
                path,
                target,
                project,
            } => write!(
                f,
                "{} is a link to {}, outside its project, {}: it is read, and never written \
                 through — change the file it leads to where it is, or copy it into the project",
                path.display(),
                target.display(),
                project.display()
            ),
            DocumentError::WrittenSince { path } => write!(
                f,
                "{} was written by another process since this command looked for it, \
                 and nothing was written over it",
                path.display()
            ),
            DocumentError::Locked { path, holder } => match holder {
                Some(holder) => write!(
                    f,
                    "{} is being saved by {holder}, and nothing was written: run the \
                     command again in a moment. Where nothing of SampleKit runs there \
                     any more, remove {}.lock",
                    path.display(),
                    path.display()
                ),
                None => write!(
                    f,
                    "{} is being saved by another process, and nothing was written: \
                     run the command again in a moment. A lock left by a process that \
                     was stopped is taken back by the next save; one that persists \
                     while no SampleKit runs is {}.lock, which can then be removed",
                    path.display(),
                    path.display()
                ),
            },
            DocumentError::Compute { property, reason } => {
                write!(f, "computing '{property}' before saving: {reason}")
            }
            DocumentError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for DocumentError {}

/// The `[property.*]` key a precision at `at` belongs under: `properties.diameter`
/// is `diameter`, a table's column or cell `tables.m.columns.hv` or
/// `tables.m.rows[3].hv` is `"m.hv"`.
fn declared_as(at: &str) -> String {
    let parts: Vec<&str> = at.split('.').collect();
    match parts.as_slice() {
        ["properties", name, ..] => (*name).to_string(),
        // Quoted: TOML reads a bare dot as a nested table.
        ["tables", table, _, column, ..] => format!("\"{table}.{column}\""),
        _ => "<name>".to_string(),
    }
}

/// How many YAML comments a file's frontmatter holds: what a canonical write
/// drops. A `#` counts when it starts a line or follows a space outside quotes.
pub fn comments_in(source: &str) -> usize {
    let body = source.strip_prefix('\u{feff}').unwrap_or(source);
    let body = body.replace("\r\n", "\n");
    let Some(after_open) = body.strip_prefix("---\n") else {
        return 0;
    };
    // Closed by a `---` that ends the file too, without a line ending, as
    // `parse` reads it.
    let close = match after_open.find("\n---\n") {
        Some(close) => close,
        None if after_open.ends_with("\n---") => after_open.len() - 4,
        None => return 0,
    };
    after_open[..close + 1]
        .lines()
        .filter(|line| {
            let mut quote: Option<char> = None;
            let mut previous = ' ';
            for character in line.chars() {
                match (quote, character) {
                    (Some(open), c) if c == open => quote = None,
                    (Some(_), _) => {}
                    (None, '"' | '\'') => quote = Some(character),
                    (None, '#') if previous.is_whitespace() => return true,
                    _ => {}
                }
                previous = character;
            }
            false
        })
        .count()
}
