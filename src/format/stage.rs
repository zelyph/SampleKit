//! Changes kept until they are written: a save, a file removed, a file moved,
//! each waiting as one entry per file. A change waiting is what a file reads
//! as for every reader in this process; the files themselves are untouched
//! until [`commit`] writes everything at once.
//!
//! What waits is a project's: one folder of entries per project, each entry
//! a file of its own, written whole and renamed into place. A process started
//! with that folder named in [`STAGE_GIVEN`] — the model's worker, a script, a
//! figure — reads what waits and adds to it, and what waits outlasts the
//! process that made it: a process killed between two entries loses neither.

use super::document::{self, DocumentError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The variable naming the folder of what waits, for a process this one
/// starts.
pub const STAGE_GIVEN: &str = "SAMPLEKIT_STAGE";

/// What a file is once what waits is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum After {
    /// This text.
    Text(String),
    /// No file.
    Removed,
    /// The file that was at this path, moved here: a sample's own file, a
    /// photo, renamed with it.
    MovedFrom(PathBuf),
}

/// One file's change waiting: the file as it was when the first change to it
/// began to wait — its text, `None` where there was no file or it is no text
/// — and what it becomes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    pub base: Option<String>,
    /// Whether a file was there when it began to wait.
    pub existed: bool,
    /// The digest of what it held then, text or not: what tells another
    /// hand's change of a photo moved.
    #[serde(default)]
    pub digest: Option<String>,
    pub after: After,
    /// When the first change to it began to wait, in seconds since 1970.
    pub since: u64,
}

impl Entry {
    /// Whether the file on disk is no longer the one this change was made
    /// over: written since by another hand — an editor, the command line.
    pub fn conflicts(&self) -> bool {
        match (&self.after, fs::read(&self.path)) {
            // A move's destination is free, and what it moves still there.
            (After::MovedFrom(_), Ok(_)) => true,
            (After::MovedFrom(from), Err(_)) => fs::symlink_metadata(from).is_err(),
            (_, Ok(bytes)) => {
                !self.existed
                    || match &self.base {
                        Some(base) => base.as_bytes() != bytes.as_slice(),
                        None => self.digest.as_deref() != Some(digest_of(&bytes).as_str()),
                    }
            }
            (_, Err(_)) => self.existed,
        }
    }

    /// The text this file had when its change began to wait, as bytes.
    pub fn before(&self) -> Option<Vec<u8>> {
        self.existed
            .then(|| self.base.clone().map(String::into_bytes))
            .flatten()
    }

    /// What it holds once written, as bytes: `None` removed.
    pub fn after_bytes(&self) -> Option<Vec<u8>> {
        match &self.after {
            After::Text(text) => Some(text.clone().into_bytes()),
            After::Removed => None,
            After::MovedFrom(from) => fs::read(from).ok(),
        }
    }
}

struct Stage {
    /// Who began it: a stage begun again over the same folder is the new
    /// one's, and the first one's end leaves it.
    owner: u64,
    folder: PathBuf,
    /// The project whose files wait here; `None` for every file, in a
    /// process started to work on them.
    root: Option<PathBuf>,
    entries: IndexMap<PathBuf, Entry>,
}

static STAGES: Mutex<Vec<Stage>> = Mutex::new(Vec::new());
static OWNERS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

thread_local! {
    /// Writes go to the disk on this thread whatever waits: what writes a
    /// change of its own at once, an undo of what was written.
    static THROUGH: Cell<bool> = const { Cell::new(false) };
}

/// While it lives, writes on this thread go to the disk whatever waits.
pub struct Through(bool);

impl Through {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Through {
        Through(THROUGH.with(|through| through.replace(true)))
    }
}

impl Drop for Through {
    fn drop(&mut self) {
        let was = self.0;
        THROUGH.with(|through| through.set(was));
    }
}

/// `run` with its saves written at once, whatever waits.
pub fn through<R>(run: impl FnOnce() -> R) -> R {
    let was = THROUGH.with(|through| through.replace(true));
    let result = run();
    THROUGH.with(|through| through.set(was));
    result
}

fn digest_of(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A path as what waits knows it: absolute, its folder resolved, so that a
/// file named from two places is one entry.
pub fn key(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |here| here.join(path))
    };
    match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => {
            let mut parent = parent.to_path_buf();
            let mut missing = Vec::new();
            // A folder a change waiting makes is not there yet: what is above
            // it is resolved.
            while !parent.exists() {
                match (parent.parent(), parent.file_name()) {
                    (Some(up), Some(part)) => {
                        missing.push(part.to_os_string());
                        parent = up.to_path_buf();
                    }
                    _ => break,
                }
            }
            let mut resolved = dunce::canonicalize(&parent).unwrap_or(parent);
            for part in missing.into_iter().rev() {
                resolved.push(part);
            }
            resolved.join(name)
        }
        _ => absolute,
    }
}

/// An entry's file in the folder: named by its path, so that the same file
/// waiting twice is one entry.
fn entry_file(folder: &Path, path: &Path) -> PathBuf {
    let digest = Sha256::digest(path.to_string_lossy().as_bytes());
    let name: String = digest
        .iter()
        .take(10)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    folder.join(format!("{name}.json"))
}

fn read_folder(folder: &Path) -> io::Result<IndexMap<PathBuf, Entry>> {
    let mut entries: Vec<Entry> = Vec::new();
    match fs::read_dir(folder) {
        Ok(listing) => {
            for item in listing.flatten() {
                let file = item.path();
                if file.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                // One entry unreadable — a write cut short — is not every
                // other one lost: it is left out.
                if let Ok(text) = fs::read_to_string(&file)
                    && let Ok(entry) = serde_json::from_str::<Entry>(&text)
                {
                    entries.push(entry);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    entries.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.path.cmp(&b.path)));
    Ok(entries
        .into_iter()
        .map(|entry| (entry.path.clone(), entry))
        .collect())
}

fn write_entry(folder: &Path, entry: &Entry) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let text = serde_json::to_string(entry).map_err(io::Error::other)?;
    let file = entry_file(folder, &entry.path);
    let temporary = file.with_extension(format!("json.{}", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, &file)
}

fn forget_entry(folder: &Path, path: &Path) -> io::Result<()> {
    match fs::remove_file(entry_file(folder, path)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn poisoned() -> io::Error {
    io::Error::other("what waits could not be read: another thread failed holding it")
}

/// The stage `path` waits in: the project's that holds it, the nearest.
fn of_path<R>(path: &Path, run: impl FnOnce(&mut Stage, PathBuf) -> R) -> Option<R> {
    if THROUGH.with(Cell::get) {
        return None;
    }
    let mut stages = STAGES.lock().ok()?;
    if stages.is_empty() {
        return None;
    }
    let key = key(path);
    let stage = stages
        .iter_mut()
        .filter(|stage| stage.root.as_ref().is_none_or(|root| key.starts_with(root)))
        .max_by_key(|stage| {
            stage
                .root
                .as_ref()
                .map_or(0, |root| root.components().count())
        })?;
    Some(run(stage, key))
}

/// The stage kept in `folder`.
fn of_folder<R>(folder: &Path, run: impl FnOnce(&mut Stage) -> R) -> Option<R> {
    let mut stages = STAGES.lock().ok()?;
    stages
        .iter_mut()
        .find(|stage| stage.folder == folder)
        .map(run)
}

/// From now on in this process, the changes to the files under `root` wait
/// in `folder`, and what waits there already is read: every file's, where no
/// root is given. Gives how many files have a change waiting.
pub fn begin(folder: &Path, root: Option<&Path>) -> io::Result<usize> {
    begin_owned(folder, root).map(|(_, count)| count)
}

/// [`begin`], with what [`end_owned`] ends it by: only this beginning,
/// whatever began over the same folder since.
pub fn begin_owned(folder: &Path, root: Option<&Path>) -> io::Result<(u64, usize)> {
    let entries = read_folder(folder)?;
    let count = entries.len();
    let root = root.map(|root| dunce::canonicalize(root).unwrap_or_else(|_| key(root)));
    let owner = OWNERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut stages = STAGES.lock().map_err(|_| poisoned())?;
    stages.retain(|stage| stage.folder != folder);
    stages.push(Stage {
        owner,
        folder: folder.to_path_buf(),
        root,
        entries,
    });
    Ok((owner, count))
}

/// What [`begin_owned`] began ended, where nothing began over its folder
/// since.
pub fn end_owned(owner: u64) {
    if let Ok(mut stages) = STAGES.lock() {
        stages.retain(|stage| stage.owner != owner);
    }
}

/// Whether writes go to the disk on this thread whatever waits: what a
/// thread started under it is given.
pub fn through_now() -> bool {
    THROUGH.with(Cell::get)
}

/// Waits where the process that started this one said to, [`STAGE_GIVEN`]:
/// a worker, a script or a figure the TUI started reads and adds to what
/// waits there.
pub fn begin_as_given() {
    if let Some(folder) = std::env::var_os(STAGE_GIVEN).filter(|folder| !folder.is_empty()) {
        let _ = begin(Path::new(&folder), None);
    }
}

/// The changes to the files of `folder`'s project are written again, as
/// before [`begin`]; what waits in the folder stays there.
pub fn end(folder: &Path) {
    if let Ok(mut stages) = STAGES.lock() {
        stages.retain(|stage| stage.folder != folder);
    }
}

/// The folder the changes to any of `paths` wait in, for a process started
/// over them: empty where none waits.
pub fn given_for(paths: &[PathBuf]) -> std::ffi::OsString {
    paths
        .iter()
        .find_map(|path| of_path(path, |stage, _| stage.folder.clone()))
        .map(PathBuf::into_os_string)
        .unwrap_or_default()
}

/// Whether a change to `path` waits rather than being written.
pub fn staging(path: &Path) -> bool {
    of_path(path, |_, _| ()).is_some()
}

/// What waits there, read again: what a process this one started added.
pub fn refresh(folder: &Path) -> io::Result<()> {
    let entries = read_folder(folder)?;
    of_folder(folder, |stage| stage.entries = entries);
    Ok(())
}

/// Every change waiting in `folder`, oldest first.
pub fn entries(folder: &Path) -> Vec<Entry> {
    of_folder(folder, |stage| stage.entries.values().cloned().collect()).unwrap_or_default()
}

/// How many files have a change waiting in `folder`.
pub fn count(folder: &Path) -> usize {
    of_folder(folder, |stage| stage.entries.len()).unwrap_or(0)
}

/// What waits for `path`, where something does.
pub fn entry(path: &Path) -> Option<Entry> {
    of_path(path, |stage, key| stage.entries.get(&key).cloned()).flatten()
}

/// What `path` reads as while changes wait: `Some(Some(text))` a save
/// waiting, `Some(None)` removed, `None` nothing waiting — the disk's.
pub fn text(path: &Path) -> Option<Option<String>> {
    let entry = entry(path)?;
    match entry.after {
        After::Text(text) => Some(Some(text)),
        After::Removed => Some(None),
        After::MovedFrom(from) => fs::read_to_string(from).ok().map(Some),
    }
}

/// Whether `path` is a file once what waits is written: a save or a move
/// waiting there, or the disk's, not removed.
pub fn exists(path: &Path) -> bool {
    match entry(path).map(|entry| entry.after) {
        Some(After::Removed) => false,
        Some(_) => true,
        None => fs::symlink_metadata(path).is_ok(),
    }
}

/// The files a change waiting makes, under `root`: a sample made, one
/// renamed to there, not on disk yet.
pub fn made_under(root: &Path) -> Vec<PathBuf> {
    let root_key = dunce::canonicalize(root).unwrap_or_else(|_| key(root));
    let Ok(stages) = STAGES.lock() else {
        return Vec::new();
    };
    let made: Vec<PathBuf> = stages
        .iter()
        .flat_map(|stage| stage.entries.values())
        .filter(|entry| !matches!(entry.after, After::Removed) && !entry.existed)
        .map(|entry| entry.path.clone())
        .filter(|path| path.starts_with(&root_key))
        .collect();
    // Named as the root is: a relative root's samples are relative too.
    made.into_iter()
        .map(|path| match path.strip_prefix(&root_key) {
            Ok(below) => root.join(below),
            Err(_) => path,
        })
        .collect()
}

fn put(path: &Path, after: After) -> Result<(), DocumentError> {
    let io = |source: io::Error| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    };
    of_path(path, |stage, key| -> Result<(), DocumentError> {
        let entry = match stage.entries.get(&key) {
            Some(entry) => Entry {
                after,
                ..entry.clone()
            },
            None => Entry {
                existed: fs::symlink_metadata(&key).is_ok(),
                digest: fs::read(&key).ok().map(|bytes| digest_of(&bytes)),
                base: fs::read_to_string(&key).ok(),
                path: key.clone(),
                after,
                since: now(),
            },
        };
        // Back to what the disk holds: nothing waits for it any more.
        let undone = match (&entry.after, &entry.base) {
            (After::Text(text), Some(base)) => entry.existed && text == base,
            (After::Removed, _) => !entry.existed,
            _ => false,
        };
        if undone {
            forget_entry(&stage.folder, &key).map_err(io)?;
            stage.entries.shift_remove(&key);
            return Ok(());
        }
        write_entry(&stage.folder, &entry).map_err(io)?;
        stage.entries.insert(key, entry);
        Ok(())
    })
    .unwrap_or_else(|| Err(io(io::Error::other("no change to it is kept waiting"))))
}

/// A save waiting: `path` reads `text` until it is written.
pub fn save(path: &Path, text: &str) -> Result<(), DocumentError> {
    put(path, After::Text(text.to_string()))
}

/// A removal waiting.
pub fn remove(path: &Path) -> Result<(), DocumentError> {
    // A file made while waiting, removed, was never there.
    if let Some(entry) = entry(path)
        && !entry.existed
    {
        return forget(path);
    }
    put(path, After::Removed)
}

/// A move waiting: `from` is removed and `to` holds what it held.
pub fn moved(from: &Path, to: &Path) -> Result<(), DocumentError> {
    match text(from) {
        // A text waiting moves as a save: it is read from here.
        Some(Some(text)) => save(to, &text)?,
        Some(None) => {
            return Err(DocumentError::Io {
                path: from.to_path_buf(),
                source: io::Error::new(io::ErrorKind::NotFound, "it is removed already"),
            });
        }
        None => put(to, After::MovedFrom(key(from)))?,
    }
    remove(from)
}

/// What waits for `path` dropped: it reads as the disk again.
pub fn forget(path: &Path) -> Result<(), DocumentError> {
    let io = |source: io::Error| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    };
    of_path(path, |stage, key| {
        forget_entry(&stage.folder, &key).map_err(io)?;
        stage.entries.shift_remove(&key);
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// Every change waiting in `folder` dropped.
pub fn forget_all(folder: &Path) -> io::Result<()> {
    of_folder(folder, |stage| -> io::Result<()> {
        for path in stage.entries.keys() {
            forget_entry(&stage.folder, path)?;
        }
        stage.entries.clear();
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// What waited, waiting again in place of what waits now: a change taken
/// back.
pub fn restore(folder: &Path, snapshot: Vec<Entry>) -> io::Result<()> {
    of_folder(folder, |stage| -> io::Result<()> {
        for path in stage.entries.keys() {
            forget_entry(&stage.folder, path)?;
        }
        for entry in &snapshot {
            write_entry(&stage.folder, entry)?;
        }
        stage.entries = snapshot
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect();
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// What waits for `path` laid again over the file as it is now, `text`: the
/// change it makes, `after`, now made over that text.
pub fn relay(path: &Path, text: &str, after: &str) -> Result<(), DocumentError> {
    let io = |source: io::Error| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    };
    of_path(path, |stage, key| -> Result<(), DocumentError> {
        let Some(entry) = stage.entries.get(&key).cloned() else {
            return Ok(());
        };
        let entry = Entry {
            base: Some(text.to_string()),
            digest: Some(digest_of(text.as_bytes())),
            existed: true,
            after: After::Text(after.to_string()),
            ..entry
        };
        write_entry(&stage.folder, &entry).map_err(io)?;
        stage.entries.insert(key, entry);
        Ok(())
    })
    .unwrap_or(Ok(()))
}

/// The files whose change waits over a file changed since by another hand.
pub fn conflicts(folder: &Path) -> Vec<PathBuf> {
    entries(folder)
        .into_iter()
        .filter(Entry::conflicts)
        .map(|entry| entry.path)
        .collect()
}

/// Writes every change waiting in `folder`: the moves first, then the saves,
/// then the removals, each file checked against what it was when its change
/// began to wait. A file changed since by another hand is refused, and
/// nothing is written. Gives the files written.
pub fn commit(folder: &Path) -> Result<Vec<PathBuf>, DocumentError> {
    let waiting = entries(folder);
    if let Some(conflict) = waiting.iter().find(|entry| entry.conflicts()) {
        return Err(DocumentError::ConcurrentEdit {
            path: conflict.path.clone(),
        });
    }
    let io = |path: &Path, source: io::Error| DocumentError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut written = Vec::new();
    let moved_from = |path: &Path| {
        waiting
            .iter()
            .any(|entry| matches!(&entry.after, After::MovedFrom(from) if from == path))
    };
    for entry in &waiting {
        let After::MovedFrom(from) = &entry.after else {
            continue;
        };
        if let Some(parent) = entry.path.parent() {
            fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
        }
        document::note_written(from);
        document::note_written(&entry.path);
        fs::rename(from, &entry.path).map_err(|error| io(&entry.path, error))?;
        written.push(entry.path.clone());
    }
    for entry in &waiting {
        let expected = entry.before();
        match &entry.after {
            After::Text(text) => {
                if let Some(parent) = entry.path.parent() {
                    fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
                }
                document::replace_on_disk(&entry.path, expected.as_deref(), Some(text.as_bytes()))?;
                written.push(entry.path.clone());
            }
            After::Removed if !moved_from(&entry.path) => {
                document::replace_on_disk(&entry.path, expected.as_deref(), None)?;
                written.push(entry.path.clone());
            }
            _ => {}
        }
    }
    forget_all(folder).map_err(|error| io(folder, error))?;
    Ok(written)
}
