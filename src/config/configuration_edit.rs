//! A `.samplekitrc` changed and written back, keeping what was not touched —
//! comments, order, spacing — as its author wrote it: what the workbench's
//! configuration screens and `init` write through. Checked as `load` would read
//! it before it is written.

use std::fmt;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

use crate::config::project_config::{self, ConfigError, ProjectConfig};

/// A named section: `[query.approved]` is a `Query` named `approved`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Query,
    Profile,
    Export,
    Figure,
    Property,
    Unit,
    Style,
}

impl Kind {
    pub const ALL: [Kind; 7] = [
        Kind::Query,
        Kind::Profile,
        Kind::Export,
        Kind::Figure,
        Kind::Property,
        Kind::Unit,
        Kind::Style,
    ];

    /// The section's name in the file.
    pub fn section(self) -> &'static str {
        match self {
            Kind::Query => "query",
            Kind::Profile => "profile",
            Kind::Export => "export",
            Kind::Figure => "figure",
            Kind::Property => "property",
            Kind::Unit => "unit",
            Kind::Style => "style",
        }
    }
}

/// Why an edit is not written.
#[derive(Debug)]
pub enum EditError {
    /// The file is not TOML to begin with.
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    /// The edited text would not load: nothing is written.
    Refused(ConfigError),
    /// The file changed since it was read: another hand's edit is not
    /// overwritten.
    Changed {
        path: PathBuf,
    },
    Io {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::Unreadable { path, reason } => {
                write!(f, "{} is not TOML to edit: {reason}", path.display())
            }
            EditError::Refused(error) => {
                write!(
                    f,
                    "the configuration would not load: {error} — nothing is written until it does"
                )
            }
            EditError::Changed { path } => write!(
                f,
                "{} changed since it was read, and nothing was written: read it again",
                path.display()
            ),
            EditError::Io { path, reason } => {
                write!(f, "{}: {reason}, and nothing was written", path.display())
            }
        }
    }
}

impl std::error::Error for EditError {}

/// A configuration being edited: what was read, and what it becomes.
pub struct ConfigurationEdit {
    path: PathBuf,
    /// The text read, `None` for a file that does not exist yet.
    read: Option<String>,
    document: DocumentMut,
}

impl ConfigurationEdit {
    /// The file at `path`, read to be edited; a file not there yet begins as
    /// `schema_version = 1`.
    pub fn open(path: &Path) -> Result<ConfigurationEdit, EditError> {
        let read = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(EditError::Io {
                    path: path.to_path_buf(),
                    reason: error.to_string(),
                });
            }
        };
        let text = read
            .clone()
            .unwrap_or_else(|| "schema_version = 1\n".to_string());
        let document = text
            .parse::<DocumentMut>()
            .map_err(|error| EditError::Unreadable {
                path: path.to_path_buf(),
                reason: error.to_string(),
            })?;
        Ok(ConfigurationEdit {
            path: path.to_path_buf(),
            read,
            document,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The text read, `None` for a file that was not there.
    pub fn read_text(&self) -> Option<&str> {
        self.read.as_deref()
    }

    /// The names a kind of section declares, in the file's order.
    pub fn names(&self, kind: Kind) -> Vec<String> {
        self.document
            .get(kind.section())
            .and_then(Item::as_table)
            .map(|table| table.iter().map(|(name, _)| name.to_string()).collect())
            .unwrap_or_default()
    }

    /// `key = value` in `[kind.name]`, the section made where it is missing.
    /// A key already there keeps its place and the comment beside it.
    /// Refused, and false, where the file holds something other than a
    /// table under that name.
    pub fn set(&mut self, kind: Kind, name: &str, key: &str, value: impl Into<Value>) -> bool {
        let Some(entry) = self.entry(kind, name) else {
            return false;
        };
        put(entry, key, value.into());
        true
    }

    /// Removes `key` from `[kind.name]`; says whether it was there.
    pub fn unset(&mut self, kind: Kind, name: &str, key: &str) -> bool {
        self.document
            .get_mut(kind.section())
            .and_then(Item::as_table_mut)
            .and_then(|table| table.get_mut(name))
            .and_then(Item::as_table_mut)
            .is_some_and(|entry| entry.remove(key).is_some())
    }

    /// Removes `[kind.name]` whole; says whether it was there.
    pub fn remove(&mut self, kind: Kind, name: &str) -> bool {
        let Some(section) = self
            .document
            .get_mut(kind.section())
            .and_then(Item::as_table_mut)
        else {
            return false;
        };
        let removed = section.remove(name).is_some();
        if section.is_empty() {
            self.document.remove(kind.section());
        }
        removed
    }

    /// `key = value` in a section of its own — `[render]`, `[collection]`,
    /// `[model]`, `[matplotlib]`, `[workbench.colors]` — made where it is
    /// missing, a dotted name a section within a section.
    /// Refused, and false, where the file holds something other than a
    /// table there.
    pub fn set_setting(&mut self, section: &str, key: &str, value: impl Into<Value>) -> bool {
        let Some(table) = self.setting_table(section) else {
            return false;
        };
        put(table, key, value.into());
        true
    }

    /// Removes `key` from a section of its own; says whether it was there.
    pub fn unset_setting(&mut self, section: &str, key: &str) -> bool {
        let mut table: &mut dyn TableLike = self.document.as_table_mut();
        for part in section.split('.') {
            match table.get_mut(part).and_then(Item::as_table_like_mut) {
                Some(inner) => table = inner,
                None => return false,
            }
        }
        take(table, key)
    }

    /// Each key of `[kind.name]`, with its value as the file writes it.
    pub fn entries(&self, kind: Kind, name: &str) -> Vec<(String, String)> {
        self.document
            .get(kind.section())
            .and_then(Item::as_table_like)
            .and_then(|table| table.get(name))
            .and_then(Item::as_table_like)
            .map(|table| written_keys(table, ""))
            .unwrap_or_default()
    }

    /// Each key of a section of its own, with its value as the file writes
    /// it; a table within it is left to its own dotted name.
    pub fn settings(&self, section: &str) -> Vec<(String, String)> {
        let mut table: &dyn TableLike = self.document.as_table();
        for part in section.split('.') {
            match table.get(part).and_then(Item::as_table_like) {
                Some(inner) => table = inner,
                None => return Vec::new(),
            }
        }
        written_keys(table, "")
    }

    /// The text the file would hold.
    pub fn text(&self) -> String {
        self.document.to_string()
    }

    /// The configuration the text would load as, or why it would not.
    pub fn checked(&self) -> Result<ProjectConfig, EditError> {
        project_config::parse(&self.path, &self.text()).map_err(EditError::Refused)
    }

    /// The lines that change, `-` for what goes and `+` for what comes, each
    /// with the line it is at.
    pub fn changes(&self) -> Vec<String> {
        let before = self.read.as_deref().unwrap_or_default();
        line_changes(before, &self.text())
    }

    /// Writes the file, checked first and atomically, refusing one that
    /// changed since it was read.
    pub fn write(&self) -> Result<ProjectConfig, EditError> {
        use crate::format::document::{self, DocumentError};
        let config = self.checked()?;
        let io = |error: std::io::Error| EditError::Io {
            path: self.path.clone(),
            reason: error.to_string(),
        };
        // Through a link, to the file it names: the rename replaced a
        // `.samplekitrc` linked from a shared folder with a copy of its own,
        // and the next edit of the shared one no longer reached this project.
        let target = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                dunce::canonicalize(&self.path).map_err(io)?
            }
            _ => self.path.clone(),
        };
        // Read again inside the lock that covers the write, as a sample's save
        // reads it: compared before taking it, another writer's edit made
        // between the comparison and the rename was lost.
        document::replace_if_unchanged(
            &target,
            self.read.as_deref().map(str::as_bytes),
            Some(self.text().as_bytes()),
        )
        .map_err(|error| match error {
            DocumentError::ConcurrentEdit { .. } | DocumentError::WrittenSince { .. } => {
                EditError::Changed {
                    path: self.path.clone(),
                }
            }
            DocumentError::Io { source, .. } => io(source),
            other => EditError::Io {
                path: self.path.clone(),
                reason: other.to_string(),
            },
        })?;
        Ok(config)
    }

    fn setting_table(&mut self, section: &str) -> Option<&mut Table> {
        let mut table = self.document.as_table_mut();
        for part in section.split('.') {
            table.entry(part).or_insert_with(|| {
                let mut inner = Table::new();
                // `[workbench.colors]`, never an empty `[workbench]` above.
                inner.set_implicit(true);
                Item::Table(inner)
            });
            table = section_in(table, part)?;
        }
        Some(table)
    }

    fn entry(&mut self, kind: Kind, name: &str) -> Option<&mut Table> {
        self.document.entry(kind.section()).or_insert_with(|| {
            let mut table = Table::new();
            // `[query.approved]`, never an empty `[query]` above it.
            table.set_implicit(true);
            Item::Table(table)
        });
        let section = section_in(self.document.as_table_mut(), kind.section())?;
        section.entry(name).or_insert(Item::Table(Table::new()));
        section_in(section, name)
    }
}

/// `key` of `parent` as a section to write in: a table, or an inline one made a table
/// — `colors = { failed = "red" }` becomes `[workbench.colors]` once changed
/// — and `None` for anything else, which is refused rather than overwritten.
fn section_in<'a>(parent: &'a mut Table, key: &str) -> Option<&'a mut Table> {
    if let Some(inline) = parent.get(key).and_then(Item::as_inline_table) {
        // The comment beside `malt = { … }  # why` goes above its section,
        // and the space before `=` does not follow the name into `[… ]`.
        let comment = inline
            .decor()
            .suffix()
            .and_then(|suffix| suffix.as_str())
            .map(str::trim)
            .filter(|suffix| suffix.starts_with('#'))
            .map(str::to_string);
        let mut table = inline.clone().into_table();
        table.set_implicit(true);
        if let Some(comment) = comment {
            table.decor_mut().set_prefix(format!("\n{comment}\n"));
        }
        parent.insert(key, Item::Table(table));
        if let Some(mut written) = parent.key_mut(key) {
            written.leaf_decor_mut().clear();
        }
    }
    parent.get_mut(key)?.as_table_mut()
}

/// `key = value`, a key already there keeping its place and the comment beside
/// it; a dotted key — `font.size` — written where the file already dots it.
fn put(table: &mut Table, key: &str, mut value: Value) {
    if let Some((head, rest)) = key.split_once('.')
        && table.get(key).is_none()
        && let Some(inner) = table
            .get_mut(head)
            .and_then(Item::as_table_like_mut)
            .filter(|inner| inner.is_dotted())
    {
        if let Some(Item::Value(held)) = inner.get(rest) {
            *value.decor_mut() = held.decor().clone();
        }
        inner.insert(rest, Item::Value(value));
        return;
    }
    if let Some(Item::Value(held)) = table.get(key) {
        *value.decor_mut() = held.decor().clone();
    }
    table.insert(key, Item::Value(value));
}

/// Removes `key`, dotted where the file dots it; whether it was there.
fn take(table: &mut dyn TableLike, key: &str) -> bool {
    if table.remove(key).is_some() {
        return true;
    }
    match key.split_once('.') {
        Some((head, rest)) => table
            .get_mut(head)
            .and_then(Item::as_table_like_mut)
            .is_some_and(|inner| take(inner, rest)),
        None => false,
    }
}

/// A table's own keys and their values as written — without the comment
/// beside one, which is not the value — a dotted key by its whole name.
fn written_keys(table: &dyn TableLike, prefix: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (key, item) in table.iter() {
        let name = format!("{prefix}{key}");
        if let Some(value) = item.as_value() {
            let mut bare = value.clone();
            bare.decor_mut().clear();
            found.push((name, bare.to_string().trim().to_string()));
        } else if let Some(inner) = item.as_table_like().filter(|inner| inner.is_dotted()) {
            found.extend(written_keys(inner, &format!("{name}.")));
        }
    }
    found
}

/// A value as someone types it: TOML where it reads as TOML — `12`, `true`,
/// `"text"`, `[1, 2]`, `{ field = "malt" }` — and text otherwise, so that
/// `approved` needs no quotes. What begins as a list or a table and does not
/// read as one, or carries a comment, is refused: written as text, `[7, 5`
/// and `14 # bigger` passed every check and meant nothing.
pub fn value_of(text: &str) -> Result<Value, String> {
    let text = text.trim();
    match text.parse::<Value>() {
        Ok(value) => Ok(value),
        Err(error) if text.starts_with(['[', '{']) => {
            // The parser's own words, without its picture of the line.
            let said: Vec<String> = error
                .to_string()
                .lines()
                .map(str::trim)
                .filter(|line| {
                    !line.is_empty() && !line.starts_with("TOML parse error") && !line.contains('|')
                })
                .map(str::to_string)
                .collect();
            Err(format!("{text} does not read as TOML: {}", said.join(", ")))
        }
        Err(_) if text.contains(" #") => Err(format!(
            "'#' begins a comment, which a value holds none of: quote it to write text, \"{}\"",
            text.replace('"', "\\\"")
        )),
        Err(_) => Ok(Value::from(text.to_string())),
    }
}

/// A line diff by longest common subsequence: small files, and every line
/// that changes said.
fn line_changes(before: &str, after: &str) -> Vec<String> {
    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();
    let mut common = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            common[i][j] = if old[i] == new[j] {
                common[i + 1][j + 1] + 1
            } else {
                common[i + 1][j].max(common[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut changes = Vec::new();
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
        } else if j < new.len() && (i == old.len() || common[i][j + 1] >= common[i + 1][j]) {
            changes.push(format!("+{:>4}  {}", j + 1, new[j]));
            j += 1;
        } else {
            changes.push(format!("-{:>4}  {}", i + 1, old[i]));
            i += 1;
        }
    }
    changes
}
