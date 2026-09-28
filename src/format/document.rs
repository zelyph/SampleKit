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
pub fn parse(source: &str) -> Result<Document, DocumentError> {
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
    let mut line_number = 1usize;
    let mut closed = false;
    while offset < body.len() {
        line_number += 1;
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
    let _ = line_number;

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
    let source = fs::read_to_string(path).map_err(|source| DocumentError::Io {
        path: path.to_path_buf(),
        source,
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
/// The exclusive section a save happens in. Creating the file *is* acquiring
/// the lock — `create_new` fails if it exists — and dropping it releases,
/// whether the save succeeded or not.
struct Lock(PathBuf);

impl Lock {
    fn acquire(destination: &Path) -> Result<Lock, DocumentError> {
        let mut name = destination.as_os_str().to_os_string();
        name.push(".lock");
        let at = PathBuf::from(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&at)
        {
            Ok(_) => Ok(Lock(at)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                Err(DocumentError::Locked {
                    path: destination.to_path_buf(),
                })
            }
            // Said of the file being saved, never of the lock beside it.
            Err(source) => Err(DocumentError::Io {
                path: destination.to_path_buf(),
                source,
            }),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
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

pub fn save(document: &Document, to: &Destination) -> Result<Origin, DocumentError> {
    let path = to.path().to_path_buf();
    // Held across the check, the write and the rename: two syscalls with
    // anything at all between them is not a check.
    let _lock = Lock::acquire(&path)?;
    // A write that would change nothing writes nothing: not the file, not its
    // time, not its permissions.
    if let Ok(current) = fs::read_to_string(&path)
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
        && fs::symlink_metadata(path).is_ok()
    {
        return Err(DocumentError::WrittenSince { path: path.clone() });
    }
    if let Destination::Origin(origin) = to {
        let current = fs::read_to_string(&origin.path).map_err(|source| DocumentError::Io {
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
        Ok(_) => {
            return Err(DocumentError::NotReadBack {
                path,
                reason: "it reads back as a different document".to_string(),
            });
        }
        Err(error) => {
            return Err(DocumentError::NotReadBack {
                path,
                reason: error.to_string(),
            });
        }
    }
    write_atomically(&path, rendered.as_bytes()).map_err(|source| DocumentError::Io {
        path: path.clone(),
        source,
    })?;
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
    let (document, origin) = load(path)?;
    let mut sample = schema::into_sample(document.schema).map_err(DocumentError::Schema)?;
    sample.set_note(document.note.as_str().to_string());
    // Named by its file where it writes no name.
    sample.set_file(Some(path));
    Ok((sample, origin))
}

/// A model's sample filled by a file: its declarations stay, and the file's
/// values and note arrive. The loading path of a sample whose class declared
/// properties before the file was read.
pub fn load_into(path: &Path, sample: &mut Sample) -> Result<Origin, DocumentError> {
    let (file, origin) = load_sample(path)?;
    sample
        .fill_from(file)
        .map_err(|error| DocumentError::Schema(SchemaError::Sample(error)))?;
    // A computed value edited by hand in the file is an override nobody marked,
    // and is held as one. A hold changes no value, so the pass stands.
    let names: Vec<Identifier> = sample.property_names().into_iter().cloned().collect();
    let _pass = fingerprint::Pass::begin();
    for name in &names {
        // A value beside readings with no record of the statistic that gave it
        // is a value written, whoever wrote it: a file an earlier SampleKit
        // wrote holds their mean so, and it is read as it is written rather
        // than guessed at by its digits. A value with no record of its formula
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
            handle.hold_as_edited();
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
        None if current.is_some() => fs::remove_file(path).map_err(io),
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
pub fn write_atomically(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write;
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
    let temporary = directory.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("samplekit"),
        std::process::id()
    ));
    let written = (|| {
        let mut file = fs::File::create(&temporary)?;
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
    match fs::read_to_string(to.path()) {
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
    Locked {
        path: PathBuf,
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
            DocumentError::WrittenSince { path } => write!(
                f,
                "{} was written by another process since this command looked for it, \
                 and nothing was written over it",
                path.display()
            ),
            DocumentError::Locked { path } => write!(
                f,
                "{} is being saved by another process. A lock is held only \
                 across a write, so if this persists, remove {}.lock",
                path.display(),
                path.display()
            ),
            DocumentError::Compute { property, reason } => {
                write!(f, "computing '{property}' before saving: {reason}")
            }
            DocumentError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for DocumentError {}

/// The `[property.*]` key a precision at `at` belongs under: `properties.malt`
/// is `malt`, a table's column or cell `tables.m.columns.ebc` or
/// `tables.m.rows[3].ebc` is `"m.ebc"`.
fn declared_as(at: &str) -> String {
    let parts: Vec<&str> = at.split('.').collect();
    match parts.as_slice() {
        ["properties", name, ..] => (*name).to_string(),
        // Quoted: TOML reads a bare dot as a nested table.
        ["tables", table, _, column, ..] => format!("\"{table}.{column}\""),
        _ => "<name>".to_string(),
    }
}
