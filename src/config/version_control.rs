//! SampleKit's history of a project, in `.samplekit/history/`: a Git repository
//! of its own, written and read here through gix, so that Git need not be
//! installed. The researcher's own repository, where there is one, is never
//! read or written.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use gix::object::tree::EntryKind;

use crate::config::discovery;
use crate::config::model_runtime;
use crate::config::project_config::{self, ProjectConfig};
use crate::core::sample::Sample;

/// The message of a snapshot taken before a write, of what changed since the
/// last: an editor's change is never credited to the command that follows.
pub const OUTSIDE: &str = "changed outside SampleKit";

/// The message of a history's first snapshot, taken before a write: the
/// project as SampleKit first found it.
pub const FIRST: &str = "the project as SampleKit first kept it";

/// The message of a join taken for a file written from it, where no snapshot
/// holds the files as they are.
pub const JOINED: &str = "joined with the snapshots of another machine";

/// This machine's identifier in the history, its branch's: `SAMPLEKIT_MACHINE`
/// where the environment sets it; else the one drawn once and kept in the
/// machine's state directory, so that two computers cloned from one image,
/// which share a host name, never share a branch; else, where the state
/// directory cannot be written, the host's name.
pub fn machine() -> String {
    if let Some(named) = named_by_environment() {
        return named;
    }
    identifier().unwrap_or_else(own_host)
}

fn named_by_environment() -> Option<String> {
    let given = std::env::var("SAMPLEKIT_MACHINE")
        .ok()
        .filter(|name| !name.trim().is_empty())?;
    let name = machine_name(&given);
    // Written otherwise than given, said once: `a/b` and `a:b` both became
    // `a-b`, two computers sharing one line of history without a word.
    if name != given.trim() {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| {
            notice(format!(
                "SAMPLEKIT_MACHINE='{given}' is written '{name}': a machine's name holds letters, \
                 digits, '-', '_' and '.' — two names written alike are one machine"
            ));
        });
    }
    Some(name)
}

/// This host's name, as a snapshot says it.
pub fn own_host() -> String {
    host_name(&gethostname::gethostname().to_string_lossy())
}

/// The computer a snapshot is written on, as its author's name says it:
/// `SAMPLEKIT_MACHINE`, else the host.
fn shown_host() -> String {
    named_by_environment().unwrap_or_else(own_host)
}

/// The identifier kept in the state directory, drawn the first time: sixteen
/// hexadecimal digits. `None` where it can be neither read nor written. The
/// file is made whole or not at all — written aside, then linked into place —
/// so that two processes starting together keep one, and a crash leaves none
/// half written; one found empty or damaged is drawn again, and said: kept,
/// the machine went by its host's name for good, without a word.
fn identifier() -> Option<String> {
    let path = model_runtime::state_directory()?.join("machine");
    let valid = |text: &str| {
        let text = text.trim();
        (!text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit())).then(|| text.to_string())
    };
    if let Ok(text) = std::fs::read_to_string(&path) {
        if let Some(found) = valid(&text) {
            return Some(found);
        }
        // Damaged: drawn again below, over it.
        let drawn = drawn_identifier();
        let aside = path.with_extension(format!("{}.drawn", std::process::id()));
        std::fs::write(&aside, format!("{drawn}\n")).ok()?;
        std::fs::rename(&aside, &path).ok()?;
        notice(format!(
            "this computer's identifier in {} could not be read, and was drawn again: its \
             snapshots from now on are another machine's",
            path.display()
        ));
        return valid(&std::fs::read_to_string(&path).ok()?);
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let drawn = drawn_identifier();
    let aside = path.with_extension(format!("{}.drawn", std::process::id()));
    std::fs::write(&aside, format!("{drawn}\n")).ok()?;
    // Linked into place, which fails where another process put its own
    // first: then that one is read.
    let linked = std::fs::hard_link(&aside, &path);
    let _ = std::fs::remove_file(&aside);
    match linked {
        Ok(()) => Some(drawn),
        Err(_) => valid(&std::fs::read_to_string(&path).ok()?),
    }
}

/// Sixteen hexadecimal digits no other machine draws: the standard library's
/// random hashing keys, with the time and the process.
fn drawn_identifier() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0),
    );
    hasher.write_u32(std::process::id());
    format!("{:016x}", hasher.finish())
}

/// A machine's name made safe as a branch's: each character but a letter, a
/// digit, `.`, `-` and `_` made `-`, a dot never first nor doubled; nothing
/// left is `machine`.
pub fn machine_name(raw: &str) -> String {
    let mut name = String::new();
    for c in raw.trim().chars() {
        let c = if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
            c
        } else {
            '-'
        };
        if c == '.' && (name.is_empty() || name.ends_with('.')) {
            continue;
        }
        name.push(c);
    }
    let name = name.trim_end_matches('.').to_string();
    if name.is_empty() {
        "machine".to_string()
    } else {
        name
    }
}

/// A host's name as a snapshot says it: up to its first dot, lowercased, made
/// safe as a branch's — `Toms-MacBook.local` is `toms-macbook`.
pub fn host_name(raw: &str) -> String {
    let first = raw.trim().split('.').next().unwrap_or_default();
    machine_name(&first.to_lowercase())
}

/// The file of the names this machine gave others.
fn names_file() -> Option<PathBuf> {
    model_runtime::state_directory().map(|directory| directory.join("machines"))
}

/// The names this machine gave other machines, by identifier; empty where it
/// gave none. Kept on this machine only: nothing of it travels.
pub fn machine_names() -> std::collections::BTreeMap<String, String> {
    let Some(text) = names_file().and_then(|path| std::fs::read_to_string(path).ok()) else {
        return std::collections::BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let (identifier, name) = line.trim().split_once(' ')?;
            let name = name.trim();
            (!name.is_empty()).then(|| (identifier.to_string(), name.to_string()))
        })
        .collect()
}

/// Gives another machine a name, on this machine only; an empty name takes it
/// back.
pub fn name_machine(identifier: &str, name: &str) -> Result<(), VcsError> {
    let Some(path) = names_file() else {
        return Err(VcsError::Failed {
            what: "the names given to machines".to_string(),
            message: "this system has no folder for a program's state".to_string(),
        });
    };
    let mut names = machine_names();
    let name = name.trim().replace(['\n', '\r'], " ");
    if name.is_empty() {
        names.remove(identifier);
    } else {
        names.insert(identifier.to_string(), name);
    }
    let text: String = names
        .iter()
        .map(|(identifier, name)| format!("{identifier} {name}\n"))
        .collect();
    let io_error = |source| VcsError::Io {
        path: path.clone(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    let written = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&written, text).map_err(io_error)?;
    std::fs::rename(&written, &path).map_err(io_error)
}

/// Where a machine's snapshots are committed.
fn branch_of(machine: &str) -> String {
    format!("{BRANCHES}{machine}")
}

const BRANCHES: &str = "refs/heads/";

/// What a snapshot, or a tag, is signed as: SampleKit, on this machine, so
/// that a snapshot names its machine wherever it is read from.
fn signature(machine: &str) -> gix::actor::Signature {
    gix::actor::Signature {
        name: shown_host().into(),
        email: format!("{machine}{AT_MACHINE}").into(),
        time: gix::date::Time::now_local_or_utc(),
    }
}

const AT_MACHINE: &str = "@samplekit";

/// What a snapshot did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Snapshot {
    /// No history here: the process runs with `SAMPLEKIT_HISTORY=off`.
    Off,
    /// The files are those of the last snapshot: nothing was taken.
    Unchanged,
    /// A commit, by its hash.
    Taken(String),
}

/// Where a project keeps its history: `.samplekit/history/` beside its
/// `.samplekitrc`.
pub fn history_directory(config: &ProjectConfig) -> PathBuf {
    if let Some((written, real)) = READ_AS.get() {
        let root =
            dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
        if root == *written {
            return real.join(".samplekit").join("history");
        }
    }
    config.root().join(".samplekit").join("history")
}

/// A project written out as a snapshot held it, and the project it is: its
/// history is the real one's. `--at` read the past without the trace, which
/// knows which formula computed each value, and called every value current.
static READ_AS: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();

/// The project written out at `written` reads the history of the one at
/// `real`: once in a process, which writes no history while it reads the past.
pub fn read_history_of(written: &Path, real: &Path) {
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let _ = READ_AS.set((canonical(written), canonical(real)));
}

/// Whether this process keeps the history: always, but in the model's worker
/// and a command reading the project as it was, which run with
/// `SAMPLEKIT_HISTORY=off` — the computation being the command's snapshot, and
/// the past not written again.
pub fn keeps_history(_config: &ProjectConfig) -> bool {
    std::env::var("SAMPLEKIT_HISTORY").map_or(true, |value| value != "off")
}

/// The paths a snapshot keeps, as the snapshot names them, each with the file
/// it is read from: the configuration, the samples, and the model's sources —
/// under `@model/` where they lie outside the project.
pub fn kept_files(config: &ProjectConfig) -> Result<Vec<(String, PathBuf)>, VcsError> {
    let root = config.root();
    let mut kept: Vec<(String, PathBuf)> = Vec::new();
    let rc = root.join(project_config::FILENAME);
    if rc.is_file() {
        kept.push((project_config::FILENAME.to_string(), rc.clone()));
    }
    let samples = discovery::samples_of_project(config).map_err(|error| VcsError::Failed {
        what: "reading the project's samples".to_string(),
        message: error.to_string(),
    })?;
    for path in samples {
        if let Some(name) = inside(root, &path) {
            kept.push((name, path));
        }
    }
    // A file the last snapshot held that no longer reads as a sample — its
    // `schema_version` gone, the file locked — is kept, not taken for
    // deleted: a restore then replaced every edit it held.
    let unread = discovery::unread_of_project(config).unwrap_or_default();
    if !unread.is_empty() {
        let held = last_held_names(config);
        for path in unread {
            if let Some(name) = inside(root, &path)
                && held.contains(&name)
            {
                notice(format!(
                    "{} does not read as a sample now: the history keeps it as it is, or as \
                     it last held it",
                    path.display()
                ));
                kept.push((name, path));
            }
        }
    }
    if let Some(template) = model_runtime::template_of(config)
        && let Ok(sources) = model_runtime::template_files(&template)
    {
        for path in sources {
            let name = inside(root, &path).or_else(|| {
                path.strip_prefix(template.directory())
                    .ok()
                    .map(|relative| format!("@model/{}", slashed(relative)))
            });
            if let Some(name) = name
                && !kept.iter().any(|(known, _)| *known == name)
            {
                kept.push((name, path));
            }
        }
    }
    kept.sort();
    Ok(kept)
}

/// A snapshot of what the project holds, saying `message`. The repository is
/// made at the first; a snapshot whose files are the last one's is not taken.
pub fn snapshot(config: &ProjectConfig, message: &str) -> Result<Snapshot, VcsError> {
    if !keeps_history(config) {
        return Ok(Snapshot::Off);
    }
    let files = contents(&kept_files(config)?)?;
    let repository = history(config)?;
    commit_with(&repository, &files, message, false, &files)
}

/// What a project held before a write, kept in memory: nothing is written
/// until the write proves to have changed something (see [`after`]).
#[derive(Debug, Clone)]
pub struct Before {
    config: ProjectConfig,
    files: Vec<(String, Vec<u8>)>,
    /// The history's last snapshot when this was read: where another has
    /// been taken since, what was read is no longer what came before.
    head: Option<String>,
    /// The files this process had written when this was read: those written
    /// since are the write's own.
    mark: usize,
    /// The journal saying a write is under way, held until its snapshot.
    journal: Option<std::sync::Arc<Journal>>,
}

/// A journal held: the files the process writes are added to it until it is
/// dropped, and it is removed then.
#[derive(Debug)]
struct Journal {
    lock: crate::format::lock::Lock,
    /// `.samplekit/`, where the journal made it: removed with the journal
    /// where nothing else went in it, a refused command leaving no trace.
    made: Option<PathBuf>,
}

impl Drop for Journal {
    fn drop(&mut self) {
        crate::format::document::journal_closed(self.lock.path());
        let _ = std::fs::remove_file(self.lock.path());
        if let Some(folder) = &self.made {
            let _ = std::fs::remove_dir(folder);
        }
    }
}

impl Before {
    /// The project's folder.
    pub fn root(&self) -> &Path {
        self.config.root()
    }
}

/// What the project holds before a write, or `None` where it keeps no
/// history.
pub fn before(config: &ProjectConfig) -> Result<Option<Before>, VcsError> {
    if !keeps_history(config) {
        return Ok(None);
    }
    if let Some(said) = bring_from_above(config) {
        notice(said);
    }
    Ok(Some(Before {
        config: config.clone(),
        files: contents(&kept_files(config)?)?,
        head: head_of(config)?,
        mark: crate::format::document::written_mark(),
        journal: journal(config.root()).map(std::sync::Arc::new),
    }))
}

/// The prefix of the journals beside the history, each saying that a write
/// is under way, and which: a write interrupted before its snapshot is kept
/// by the next under its own name.
const JOURNAL: &str = "writing-";

/// This process's journal in the project at `root`, saying its command; `None`
/// where it cannot be written, which only loses the name of an interrupted
/// write.
fn journal(root: &Path) -> Option<Journal> {
    let folder = root.join(".samplekit");
    let made = (!folder.exists()).then(|| folder.clone());
    std::fs::create_dir_all(&folder).ok()?;
    let at = folder.join(format!("{JOURNAL}{}.lock", std::process::id()));
    // Its own, left by an earlier write of this process, is taken again.
    let _ = std::fs::remove_file(&at);
    let lock = crate::format::lock::Lock::take_saying(&at, &own_command(), |_| {}).ok()?;
    crate::format::document::journal_opened(lock.path());
    Some(Journal { lock, made })
}

/// The files the writes under way in other processes have written, as the
/// project names them: theirs to keep, not a change made outside SampleKit.
fn written_by_others(root: &Path) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(root.join(".samplekit")) else {
        return names;
    };
    let own = format!("{JOURNAL}{}.lock", std::process::id());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(JOURNAL) || !name.ends_with(".lock") || name == own {
            continue;
        }
        if crate::format::lock::journal_abandoned(&entry.path()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            if let Some(path) = line.strip_prefix("wrote ")
                && let Some(name) = inside(root, Path::new(path))
            {
                names.insert(name);
            }
        }
    }
    names
}

/// What the snapshot of another write's files says, kept while it runs.
pub const UNDER_WAY: &str = "under way";

/// The other writes under way in the project at `root`, each its command
/// and the files it has written, as the project names them.
fn others_under_way(root: &Path) -> Vec<(String, std::collections::BTreeSet<String>)> {
    let Ok(entries) = std::fs::read_dir(root.join(".samplekit")) else {
        return Vec::new();
    };
    let own = format!("{JOURNAL}{}.lock", std::process::id());
    let mut under_way = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(JOURNAL) || !name.ends_with(".lock") || name == own {
            continue;
        }
        if crate::format::lock::journal_abandoned(&entry.path()) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let names: std::collections::BTreeSet<String> = text
            .lines()
            .filter_map(|line| line.strip_prefix("wrote "))
            .filter_map(|path| inside(root, Path::new(path)))
            .collect();
        if names.is_empty() {
            continue;
        }
        let command = crate::format::lock::held_for(&entry.path())
            .unwrap_or_else(|| "another command".to_string());
        under_way.push((command, names));
    }
    under_way
}

/// A file this write changed after another write under way had written it —
/// a `set` on a sample a long computation had just computed: what the other
/// had made of it, as this write read it, kept first under the other's
/// command, `… — under way`, so that this write's snapshot holds its own
/// change alone. It held the computation's values under the `set`'s name.
/// Called under the snapshot lock.
fn keep_shared_under_way(
    repository: &gix::Repository,
    root: &Path,
    own: &std::collections::BTreeSet<String>,
    read: &[(String, Vec<u8>)],
) -> Result<(), VcsError> {
    let Some(head) = branch_head(repository, &machine())? else {
        return Ok(());
    };
    let tree = repository
        .find_commit(head)
        .map_err(|error| reading(repository)(&error))?
        .tree_id()
        .map_err(|error| reading(repository)(&error))?
        .detach();
    let mut last = blobs(repository, tree_files(repository, tree)?)?;
    for (command, names) in others_under_way(root) {
        let mut files = last.clone();
        for name in names.intersection(own) {
            match read.iter().find(|(held, _)| held == name) {
                Some((_, bytes)) => {
                    files.insert(name.clone(), bytes.clone());
                }
                None => {
                    files.remove(name);
                }
            }
        }
        if files == last {
            continue;
        }
        let kept: Vec<(String, Vec<u8>)> = files.clone().into_iter().collect();
        commit_with(
            repository,
            &kept,
            &format!("{command} — {UNDER_WAY}"),
            false,
            read,
        )?;
        last = files;
    }
    Ok(())
}

/// The command this process runs, as a snapshot's message names it: its
/// program's name and its arguments.
fn own_command() -> String {
    let mut words = std::env::args();
    let program = words
        .next()
        .map(|program| {
            Path::new(&program)
                .file_name()
                .map_or(program.clone(), |name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "samplekit".to_string());
    std::iter::once(program)
        .chain(words)
        .collect::<Vec<_>>()
        .join(" ")
}

/// How a journal says a value computed and written, and by which formula:
/// `fact VALUE\tRECORD\tFORMULA`.
pub const FACT: &str = "fact ";

/// The journals of writes interrupted — this host's and user's, their process
/// gone — removed, and the commands they name; the formulas they say computed
/// what they wrote kept in the project's trace.
fn interrupted(config: &ProjectConfig) -> Vec<String> {
    let folder = config.root().join(".samplekit");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut commands = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(JOURNAL) || !name.ends_with(".lock") {
            continue;
        }
        let at = entry.path();
        if crate::format::lock::journal_abandoned(&at)
            && let Some(command) = crate::format::lock::held_for(&at)
        {
            // A write whose snapshot failed was whole, and kept late: it was
            // said interrupted, which it was not.
            let late = std::fs::read_to_string(&at)
                .is_ok_and(|text| text.lines().any(|line| line == NOT_KEPT));
            commands.push(format!(
                "{command} — {}",
                if late { KEPT_LATE } else { INTERRUPTED }
            ));
        }
    }
    keep_journal_facts(config);
    commands.sort();
    commands
}

/// What a snapshot of changes nobody named says: [`OUTSIDE`], or the
/// commands interrupted before they could keep them.
fn outside_message(config: &ProjectConfig) -> String {
    let commands = interrupted(config);
    if commands.is_empty() {
        OUTSIDE.to_string()
    } else {
        commands.join("; ")
    }
}

/// What a snapshot of a write cut short says after its command.
pub const INTERRUPTED: &str = "interrupted";

/// What a snapshot says after a command whose own snapshot failed — a full
/// disk, a history it could not write — kept by the next command.
pub const KEPT_LATE: &str = "kept late";

/// The line a journal is given where its command's snapshot failed.
const NOT_KEPT: &str = "not kept";

/// What the history said this process should know, taken: a lock left in
/// it taken back.
static NOTICES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub fn notice(said: String) {
    if let Ok(mut notices) = NOTICES.lock() {
        if notices.contains(&said) {
            return;
        }
        notices.push(said);
    }
}

/// What the history has said since last asked, to be said as warnings.
pub fn notices() -> Vec<String> {
    NOTICES
        .lock()
        .map(|mut notices| std::mem::take(&mut *notices))
        .unwrap_or_default()
}

/// The history's own lock, held while a snapshot is written: two commands
/// ending together write their snapshots one after the other. A lock of a
/// process gone is taken back; one held elsewhere is waited for, a while.
fn snapshot_lock(directory: &Path) -> Result<crate::format::lock::Lock, VcsError> {
    let at = directory.join("snapshot.lock");
    let started = std::time::Instant::now();
    loop {
        match crate::format::lock::Lock::take(&at, |_| {}) {
            Ok(lock) => {
                take_back_ref_locks(directory);
                return Ok(lock);
            }
            Err(crate::format::lock::Refused::Held(holder))
                if started.elapsed() < std::time::Duration::from_secs(30) =>
            {
                let _ = holder;
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(crate::format::lock::Refused::Held(holder)) => {
                return Err(VcsError::Failed {
                    what: format!("a snapshot in {}", directory.display()),
                    message: format!(
                        "another snapshot is being kept, by {}: where nothing of SampleKit runs \
                         there any more, remove {}",
                        holder
                            .map_or_else(|| "another process".to_string(), |holder| holder.said()),
                        at.display()
                    ),
                });
            }
            Err(crate::format::lock::Refused::Io(source)) => {
                return Err(VcsError::Io { path: at, source });
            }
        }
    }
}

/// How old a reference's lock in the history is when it is taken for one a
/// snapshot cut short left: a reference is written in milliseconds.
const REF_LOCK_LEFT: std::time::Duration = std::time::Duration::from_secs(60);

/// The locks a snapshot cut short left on the history's references, taken
/// back, and said: left, each stopped every snapshot after it, for good.
fn take_back_ref_locks(directory: &Path) {
    let mut pending = vec![directory.join("refs")];
    let mut found: Vec<PathBuf> = ["packed-refs.lock", "HEAD.lock"]
        .iter()
        .map(|name| directory.join(name))
        .filter(|path| path.is_file())
        .collect();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "lock")
            {
                found.push(path);
            }
        }
    }
    for path in found {
        let old = std::fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > REF_LOCK_LEFT);
        if old && std::fs::remove_file(&path).is_ok() {
            notice(format!(
                "{} was left by a snapshot cut short, and was taken back: the history \
                 is kept again",
                path.display()
            ));
        }
    }
}

/// The id of this machine's last snapshot; `None` where it has taken none.
fn head_of(config: &ProjectConfig) -> Result<Option<String>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(None);
    };
    Ok(branch_head(&repository, &machine())?.map(|id| id.to_string()))
}

/// The last snapshot of `machine`'s branch, where it has one.
fn branch_head(
    repository: &gix::Repository,
    machine: &str,
) -> Result<Option<gix::ObjectId>, VcsError> {
    let failed = reading(repository);
    Ok(repository
        .try_find_reference(branch_of(machine).as_str())
        .map_err(|error| failed(&error))?
        .and_then(|reference| reference.try_id().map(|id| id.detach())))
}

/// After a write: where it changed a kept file, a snapshot of what the
/// project held before it — said [`OUTSIDE`], and taken only where that
/// differs from the last — then one of what it holds now, saying `message`.
/// A write that changed nothing kept, a refused command among them, leaves
/// the history as it was, or unmade.
///
/// Where a snapshot was taken since `before` read the project — another
/// command, in another terminal or run by a script — what was read is not
/// committed: it would take back that snapshot's change and keep a state
/// the project never held. What it holds now is the snapshot then.
pub fn after(before: Before, message: &str) -> Result<Snapshot, VcsError> {
    // A snapshot that fails — the disk full after the write — leaves the
    // write's journal: the next command keeps what it wrote under its name,
    // where it took it for a change made outside SampleKit.
    let journal = before.journal.clone();
    let config = before.config.clone();
    let taken = after_kept(before, message);
    // What a command killed said of the formulas it ran, where it wrote
    // nothing that differs: kept all the same, its journal removed.
    if taken.is_ok() {
        keep_journal_facts(&config);
    }
    if taken.is_err()
        && let Some(journal) = journal
    {
        use std::io::Write as _;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .append(true)
            .open(journal.lock.path())
        {
            let _ = writeln!(file, "{NOT_KEPT}");
        }
        std::mem::forget(journal);
    }
    taken
}

fn after_kept(before: Before, message: &str) -> Result<Snapshot, VcsError> {
    let now = contents(&kept_files(&before.config)?)?;
    let root = before.root().to_path_buf();
    if now == before.files {
        // A computation that found the changed formulas give the numbers
        // they gave wrote no file, and its trace names the model it ran:
        // that model is kept, in a snapshot of its change. It stayed *changed
        // outside SampleKit, not yet kept*, the formula the history held for
        // its values in no snapshot.
        let Some(noted) = take_model_note(&root) else {
            return Ok(Snapshot::Unchanged);
        };
        let repository = history(&before.config)?;
        let _held = snapshot_lock(&history_directory(&before.config))?;
        let head = head_of(&before.config)?;
        if head == before.head || stepped_ahead(head.as_ref()) {
            model_step(&repository, &before, Some(&noted))?;
        }
        return Ok(Snapshot::Unchanged);
    }
    // Its own: what this process wrote, and what the model's worker wrote for
    // it, which its journal holds.
    let by_worker = before
        .journal
        .as_ref()
        .map(|journal| crate::format::document::written_in(journal.lock.path()))
        .unwrap_or_default();
    let own: std::collections::BTreeSet<String> =
        crate::format::document::written_since(before.mark)
            .iter()
            .chain(&by_worker)
            .filter_map(|path| inside(&root, path))
            .collect();
    let repository = history(&before.config)?;
    let _held = snapshot_lock(&history_directory(&before.config))?;
    // Its own step of the model's change, taken ahead of its computation, is
    // no other command's snapshot.
    let head = head_of(&before.config)?;
    let moved = head != before.head && !stepped_ahead(head.as_ref());
    let noted = take_model_note(&root);
    // What another write under way has written is its own, kept by it: as
    // the last snapshot holds it here.
    let others = written_by_others(&root);
    // A file both wrote: the other's part kept first, under its name.
    if !moved && own.iter().any(|name| others.contains(name)) {
        keep_shared_under_way(&repository, &root, &own, &before.files)?;
    }
    let others: std::collections::BTreeSet<String> = others
        .into_iter()
        .filter(|name| !own.contains(name))
        .collect();
    let last = || -> Result<std::collections::BTreeMap<String, Vec<u8>>, VcsError> {
        match branch_head(&repository, &machine())? {
            Some(head) => {
                let tree = repository
                    .find_commit(head)
                    .map_err(|error| reading(&repository)(&error))?
                    .tree_id()
                    .map_err(|error| reading(&repository)(&error))?
                    .detach();
                blobs(&repository, tree_files(&repository, tree)?)
            }
            // No snapshot yet: what another write under way wrote is left out
            // of the first, which is the project before SampleKit wrote —
            // that write's own snapshot keeps it. The first held a script's
            // change, and the script's snapshot changed nothing of it.
            None => Ok(before
                .files
                .iter()
                .filter(|(name, _)| !others.contains(name))
                .cloned()
                .collect()),
        }
    };
    let mut held: std::collections::BTreeMap<String, Vec<u8>> =
        before.files.iter().cloned().collect();
    if !others.is_empty() {
        let last = last()?;
        for name in &others {
            match last.get(name) {
                Some(bytes) => {
                    held.insert(name.clone(), bytes.clone());
                }
                None => {
                    held.remove(name);
                }
            }
        }
    }
    let held: Vec<(String, Vec<u8>)> = held.into_iter().collect();
    // Whether another machine's snapshots are joined is decided by what the
    // files held before the write: the write may change a file they changed.
    if !moved {
        // The files as another machine's last snapshot holds them — a pull,
        // and nothing else — are that snapshot joined: its state, said so. A
        // model's step first set the model pulled over this machine's own
        // samples, a state no disk ever held, and then called the rest a
        // change made outside SampleKit.
        match adopted(&repository, &held)? {
            Some(machine) => {
                let said = format!("{ADOPTED} {machine} kept it");
                commit_with(&repository, &held, &said, false, &before.files)?;
            }
            None => {
                model_step(&repository, &before, noted.as_deref())?;
                let said = outside_message(&before.config);
                commit_with(&repository, &held, &said, false, &before.files)?;
            }
        }
    }
    // Its own files as it wrote them, on what came before it — the last
    // snapshot, where another command kept one since: what another command
    // wrote meanwhile is that command's to keep, under its own name.
    let files = if own.is_empty() && others.is_empty() && !moved {
        now
    } else {
        let mut files: std::collections::BTreeMap<String, Vec<u8>> = if moved {
            last()?
        } else {
            held.iter().cloned().collect()
        };
        for name in &own {
            match now.iter().find(|(held, _)| held == name) {
                Some((_, bytes)) => {
                    files.insert(name.clone(), bytes.clone());
                }
                None => {
                    files.remove(name);
                }
            }
        }
        files.into_iter().collect()
    };
    let taken = commit_with(&repository, &files, message, false, &before.files);
    drop(before.journal);
    taken
}

/// What a snapshot says that joins another machine's last one as the files
/// hold it, before the machine's name: *the project as ana kept it*.
pub const ADOPTED: &str = "the project as";

/// The machine whose last snapshot, joined by a write here, the files hold
/// exactly — where they are no other state.
fn adopted(
    repository: &gix::Repository,
    files: &[(String, Vec<u8>)],
) -> Result<Option<String>, VcsError> {
    let graph = Graph::load(repository)?;
    if graph.branches.is_empty() {
        return Ok(None);
    }
    let mut trees = Trees::default();
    let basis = hashed(repository, files)?;
    let planned = plan(repository, &graph, &mut trees, &machine(), &basis)?;
    if planned.own.is_none() {
        return Ok(None);
    }
    for joined in &planned.joined {
        if trees.at(repository, &graph, Some(*joined))? == basis {
            return Ok(graph
                .nodes
                .get(joined)
                .and_then(|node| node.machine.clone()));
        }
    }
    Ok(None)
}

/// The message of a snapshot of the model's own change, taken before what
/// else changed outside SampleKit: which version of the model computed what
/// can be read, and computed again, from its snapshot.
pub const MODEL: &str = "the model changed";

/// Where the model's sources changed since the last snapshot, a snapshot of
/// that change alone, said [`MODEL`] — naming the formulas whose digest
/// changed where a computation said which. The rest of what changed outside
/// SampleKit follows in its own snapshot.
fn model_step(
    repository: &gix::Repository,
    before: &Before,
    formulas: Option<&str>,
) -> Result<(), VcsError> {
    let model = model_names(&before.config);
    if model.is_empty() {
        return Ok(());
    }
    let graph = Graph::load(repository)?;
    if graph.branches.is_empty() {
        return Ok(());
    }
    let mut trees = Trees::default();
    let basis = hashed(repository, &before.files)?;
    let planned = plan(repository, &graph, &mut trees, &machine(), &basis)?;
    // A machine with no snapshot of its own yet — a clone's first write — has
    // no last state of its own to set the model's change against: made from
    // nothing, the step was a first snapshot holding the model alone, which a
    // restore to it took for the whole project.
    if planned.own.is_none() {
        return Ok(());
    }
    let last = planned.expected;
    let held = |files: &std::collections::BTreeMap<String, gix::ObjectId>| -> Vec<(String, gix::ObjectId)> {
        files
            .iter()
            .filter(|(name, _)| model.contains(*name))
            .map(|(name, id)| (name.clone(), *id))
            .collect()
    };
    if held(&last) == held(&basis) {
        return Ok(());
    }
    let mut files = blobs(repository, last)?;
    files.retain(|name, _| !model.contains(name));
    for (name, bytes) in &before.files {
        if model.contains(name) {
            files.insert(name.clone(), bytes.clone());
        }
    }
    let files: Vec<(String, Vec<u8>)> = files.into_iter().collect();
    let message = match formulas {
        Some(formulas) if !formulas.is_empty() => format!("{MODEL}: {formulas}"),
        _ => MODEL.to_string(),
    };
    commit_with(repository, &files, &message, false, &before.files)?;
    Ok(())
}

/// The snapshots of the model's change this process took ahead of its
/// computation: its own, which its write's snapshot is not to take for
/// another command's.
static STEPS_AHEAD: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// The model's change kept now, in a snapshot of its own naming the
/// `formulas` that changed — before a computation runs, not after it. Taken
/// at its end, another command writing meanwhile took it first, without the
/// formulas' names, which only the computation knew.
pub fn keep_model_change(config: &ProjectConfig, formulas: &[String]) -> Result<(), VcsError> {
    if formulas.is_empty() || !keeps_history(config) {
        return Ok(());
    }
    let Some(repository) = existing(config)? else {
        return Ok(());
    };
    let _held = snapshot_lock(&history_directory(config))?;
    let was = head_of(config)?;
    let before = Before {
        config: config.clone(),
        files: contents(&kept_files(config)?)?,
        head: was.clone(),
        mark: 0,
        journal: None,
    };
    model_step(&repository, &before, Some(&formulas.join(", ")))?;
    let now = head_of(config)?;
    if now != was
        && let Some(now) = now
        && let Ok(mut steps) = STEPS_AHEAD.lock()
    {
        steps.push(now);
    }
    Ok(())
}

/// Whether `head` is a snapshot of the model's change this process took.
fn stepped_ahead(head: Option<&String>) -> bool {
    head.is_some_and(|head| STEPS_AHEAD.lock().is_ok_and(|steps| steps.contains(head)))
}

/// The names the model's sources take in a snapshot.
pub fn model_names(config: &ProjectConfig) -> std::collections::BTreeSet<String> {
    let root = config.root();
    let mut names = std::collections::BTreeSet::new();
    if let Some(template) = model_runtime::template_of(config)
        && let Ok(sources) = model_runtime::template_files(&template)
    {
        for path in sources {
            let name = inside(root, &path).or_else(|| {
                path.strip_prefix(template.directory())
                    .ok()
                    .map(|relative| format!("@model/{}", slashed(relative)))
            });
            if let Some(name) = name {
                names.insert(name);
            }
        }
    }
    names
}

static MODEL_NOTES: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

/// Which formulas changed with the model a computation ran under, for the
/// snapshot of the model's change the write that follows takes in the
/// project at `root`.
pub fn note_model_change(root: &Path, formulas: &[String]) {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if let Ok(mut notes) = MODEL_NOTES.lock() {
        notes.retain(|(held, _)| *held != root);
        notes.push((root, formulas.join(", ")));
    }
}

fn take_model_note(root: &Path) -> Option<String> {
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut notes = MODEL_NOTES.lock().ok()?;
    let at = notes.iter().position(|(held, _)| *held == root)?;
    Some(notes.remove(at).1)
}

/// A write under way: what the projects it reaches held before it.
#[derive(Debug)]
pub struct Writing {
    before: Vec<Before>,
    places: Vec<PathBuf>,
    /// Held by the outermost write of the thread, the one that keeps the
    /// projects reached beyond its places.
    late: Option<LateGuard>,
}

/// The projects `places` belong to, each once: a file's, a folder's, or the
/// one above it.
pub fn projects_of(places: &[PathBuf]) -> Vec<ProjectConfig> {
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut projects = Vec::new();
    for place in places {
        let Some(found) = project_config::find(place) else {
            continue;
        };
        let found = dunce::canonicalize(&found).unwrap_or(found);
        if seen.contains(&found) {
            continue;
        }
        seen.push(found.clone());
        // A configuration that does not read still has a history, and a
        // write under it is kept: under the configuration last kept. It went
        // unkept, and was then taken for an edit made outside SampleKit.
        projects.push(project_config::load(&found).unwrap_or_else(|_| kept_configuration(&found)));
    }
    projects
}

/// The configuration the history last kept of the file `rc` that reads, for
/// a project whose own does not: what its history is read and written with.
/// A bare one where none was kept. The newest kept may be the broken one, a
/// first write under it having kept it as changed outside SampleKit: read
/// with it, the next snapshot knew no model, and kept it as deleted.
pub fn kept_configuration(rc: &Path) -> ProjectConfig {
    let bare = project_config::unread(rc);
    let kept = entries(&bare).ok().and_then(|entries| {
        entries.into_iter().find_map(|entry| {
            let files = files_at(&bare, &entry.id).ok()?;
            let text = files.get(project_config::FILENAME)?;
            project_config::parse(rc, &String::from_utf8_lossy(text)).ok()
        })
    });
    kept.unwrap_or(bare)
}

/// Before a write reaching `places`: what each of their projects holds, and
/// the projects whose files could not be read, by folder.
pub fn before_writing(places: &[PathBuf]) -> (Writing, Vec<(PathBuf, VcsError)>) {
    let mut before = Vec::new();
    let mut failures = Vec::new();
    for config in projects_of(places) {
        match self::before(&config) {
            Ok(Some(kept)) => before.push(kept),
            Ok(None) => {}
            Err(error) => failures.push((config.root().to_path_buf(), error)),
        }
    }
    let late = REACHED_LATE.with(|open| {
        let mut open = open.borrow_mut();
        if open.is_some() {
            return None;
        }
        *open = Some((
            before
                .iter()
                .map(|kept| canonical_root(kept.root()))
                .collect(),
            Vec::new(),
        ));
        Some(LateGuard)
    });
    let writing = Writing {
        before,
        places: places.to_vec(),
        late,
    };
    (writing, failures)
}

thread_local! {
    /// While a write is prepared: the projects its places named, and those
    /// its samples were found in since — a project below a folder named —
    /// each with what it held then. A write from the folder above changed
    /// their samples and was kept in no history: its change was then taken
    /// for one made outside SampleKit.
    #[allow(clippy::type_complexity)]
    static REACHED_LATE: std::cell::RefCell<Option<(Vec<PathBuf>, Vec<Before>)>> =
        const { std::cell::RefCell::new(None) };
}

/// The outermost write's hold on [`REACHED_LATE`]: what it kept let go
/// where the write ends without its snapshot, its journals removed.
#[derive(Debug)]
struct LateGuard;

impl Drop for LateGuard {
    fn drop(&mut self) {
        let taken = REACHED_LATE.with(|open| open.borrow_mut().take());
        drop(taken);
    }
}

fn canonical_root(root: &Path) -> PathBuf {
    dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

/// A project whose samples a write under way reads, beyond the places it
/// named: what it holds kept now, so that the write is kept in its history
/// too. Nothing where no write is under way, or the project was known.
pub fn reached_by_write(config: &ProjectConfig) {
    let root = canonical_root(config.root());
    let open = REACHED_LATE.with(|open| match open.borrow_mut().as_mut() {
        Some((known, _)) if !known.contains(&root) => {
            known.push(root);
            true
        }
        _ => false,
    });
    if !open {
        return;
    }
    // Read outside the borrow: `before` reaches the folder above, which
    // may itself be reached.
    match before(config) {
        Ok(Some(kept)) => REACHED_LATE.with(|open| {
            if let Some((_, late)) = open.borrow_mut().as_mut() {
                late.push(kept);
            }
        }),
        Ok(None) => {}
        Err(error) => notice(format!(
            "the history of {} will not keep this write: {error}",
            shown(config.root())
        )),
    }
}

/// After the write: [`after`] in each project it changed, and a first
/// snapshot in a project it made, as the setup does. The failures, by
/// folder, are its caller's to say as warnings, never the write's failure.
pub fn after_writing(writing: Writing, message: &str) -> Vec<(PathBuf, VcsError)> {
    after_writing_with(writing, message, None)
}

/// [`after_writing`], and where a snapshot is taken, the script that made the
/// change kept beside it — its name and its source — so that what changed the
/// data from Python can be read again.
pub fn after_writing_with(
    writing: Writing,
    message: &str,
    script: Option<&(String, Vec<u8>)>,
) -> Vec<(PathBuf, VcsError)> {
    let mut writing = writing;
    if writing.late.is_some()
        && let Some((_, late)) = REACHED_LATE.with(|open| open.borrow_mut().take())
    {
        writing.before.extend(late);
    }
    let mut failures = Vec::new();
    let mut kept = |config: &ProjectConfig, taken: Result<Snapshot, VcsError>| match taken {
        Ok(Snapshot::Taken(id)) => {
            if let Some((name, source)) = script
                && let Err(error) = keep_script(config, &id, name, source)
            {
                failures.push((config.root().to_path_buf(), error));
            }
        }
        Ok(_) => {}
        Err(error) => failures.push((config.root().to_path_buf(), error)),
    };
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let known: Vec<PathBuf> = writing
        .before
        .iter()
        .map(|before| canonical(before.root()))
        .collect();
    let mut written: Vec<ProjectConfig> = Vec::new();
    for before in writing.before {
        let config = before.config.clone();
        kept(&config, after(before, message));
        written.push(config);
    }
    for config in projects_of(&writing.places) {
        if known.contains(&canonical(config.root())) {
            continue;
        }
        let taken = snapshot(&config, message);
        kept(&config, taken);
        written.push(config);
    }
    // The first write of a project whose history holds no formula: the
    // values its files hold taken as computed by the model as it is, so that
    // a formula changed from now on is seen.
    for config in &written {
        crate::collection::computation::seed_first_trace(config);
    }
    failures
}

/// The prefix of the tags that keep a script beside the snapshot it made, each
/// named after the snapshot.
const SCRIPT: &str = "script/";

/// The script `name`, its `source`, kept beside the snapshot `id`: a tag on
/// its blob, saying its name.
pub fn keep_script(
    config: &ProjectConfig,
    id: &str,
    name: &str,
    source: &[u8],
) -> Result<(), VcsError> {
    let repository = history(config)?;
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the script beside {id}"),
        message: error.to_string(),
    };
    let blob = repository
        .write_blob(source)
        .map_err(|error| failed(&error))?
        .detach();
    let tagger = signature(&machine());
    let mut time = gix::date::parse::TimeBuf::default();
    repository
        .tag(
            format!("{SCRIPT}{id}"),
            blob,
            gix::object::Kind::Blob,
            Some(tagger.to_ref(&mut time)),
            name,
            gix::refs::transaction::PreviousValue::Any,
        )
        .map_err(|error| failed(&error))?;
    Ok(())
}

/// The script kept beside the snapshot `id`, if one is: its name and its
/// source as it ran.
pub fn script_of(config: &ProjectConfig, id: &str) -> Result<Option<(String, Vec<u8>)>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(None);
    };
    let failed = reading(&repository);
    let Some(reference) = repository
        .try_find_reference(format!("refs/tags/{SCRIPT}{id}").as_str())
        .map_err(|error| failed(&error))?
    else {
        return Ok(None);
    };
    let object = repository
        .find_object(reference.id().detach())
        .map_err(|error| failed(&error))?;
    let tag = object.try_into_tag().map_err(|error| failed(&error))?;
    let decoded = tag.decode().map_err(|error| failed(&error))?;
    let name = decoded.message.to_string().trim().to_string();
    let source = repository
        .find_blob(decoded.target())
        .map_err(|error| failed(&error))?
        .data
        .clone();
    Ok(Some((name, source)))
}

/// A file's bytes as the history reads them: in a file holding text, `\r\n`
/// as `\n`. Git for Windows writes what a clone or a pull brings with CRLF,
/// and an editor may save so: compared byte for byte, every such file read as
/// changed outside SampleKit, and another machine's state was never found in
/// the files. A file holding a NUL byte, a PDF or an image, is read as it is.
pub fn as_kept(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.contains(&0) || !bytes.windows(2).any(|pair| pair == b"\r\n") {
        return bytes;
    }
    let mut kept = Vec::with_capacity(bytes.len());
    for (at, byte) in bytes.iter().enumerate() {
        if *byte == b'\r' && bytes.get(at + 1) == Some(&b'\n') {
            continue;
        }
        kept.push(*byte);
    }
    kept
}

fn contents(kept: &[(String, PathBuf)]) -> Result<Vec<(String, Vec<u8>)>, VcsError> {
    kept.iter()
        .map(|(name, path)| {
            std::fs::read(path)
                .map(|bytes| (name.clone(), as_kept(bytes)))
                .or_else(|source| {
                    // One the system would not let be read is kept as the
                    // history last held it.
                    LAST_HELD
                        .with(|held| held.borrow().get(name).cloned())
                        .map(|bytes| (name.clone(), bytes))
                        .ok_or(VcsError::Io {
                            path: path.clone(),
                            source,
                        })
                })
        })
        .collect()
}

thread_local! {
    /// What the last snapshot of the project being kept held of each file
    /// that no longer reads, by name: read by [`contents`].
    static LAST_HELD: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The names the newest snapshot of the project holds, and what it holds of
/// each, kept for [`contents`].
fn last_held_names(config: &ProjectConfig) -> std::collections::HashSet<String> {
    let Ok(Some(head)) = head(config) else {
        return std::collections::HashSet::new();
    };
    let Ok(files) = files_at(config, &head) else {
        return std::collections::HashSet::new();
    };
    let names = files.keys().cloned().collect();
    LAST_HELD.with(|held| *held.borrow_mut() = files.into_iter().collect());
    names
}

/// The project's history, made where it is not yet. Opened apart from the
/// user's Git configuration: their identity, hooks and settings do not apply.
fn history(config: &ProjectConfig) -> Result<gix::Repository, VcsError> {
    let directory = history_directory(config);
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the history in {}", directory.display()),
        message: error.to_string(),
    };
    model_runtime::keep_out_of_git(config.root());
    if !directory.join("HEAD").is_file() && directory.join("objects").is_dir() {
        // A history shared through git arrives without the files each
        // machine keeps for itself: they are given back, and every snapshot
        // kept.
        given_back(&directory)?;
    }
    if !directory.join("HEAD").is_file() {
        std::fs::create_dir_all(&directory).map_err(|source| VcsError::Io {
            path: directory.clone(),
            source,
        })?;
        gix::init_bare(&directory).map_err(|error| failed(&error))?;
        // The researcher's `git status` lists none of it, with nothing added
        // to their own `.gitignore`.
        let ignore = directory.join(".gitignore");
        std::fs::write(&ignore, HIDDEN_IGNORE).map_err(|source| VcsError::Io {
            path: ignore,
            source,
        })?;
        let repository = gix::open_opts(&directory, gix::open::Options::isolated())
            .map_err(|error| failed(&error))?;
        // `HEAD` names the branch of the machine that made the history, so
        // that a `git log` in it reads a branch; nothing writes it after.
        point_head(&repository, &machine())?;
    }
    share_as_declared(config, &directory)?;
    // Git carries the history's files byte for byte where it shares them: a
    // ref given CRLF by Windows' `core.autocrlf` would not be read back.
    let attributes = directory.join(".gitattributes");
    if !attributes.is_file() {
        std::fs::write(&attributes, "* -text\n").map_err(|source| VcsError::Io {
            path: attributes,
            source,
        })?;
    }
    let repository = gix::open_opts(&directory, gix::open::Options::isolated())
        .map_err(|error| failed(&error))?;
    Ok(repository)
}

/// The history's `.gitignore` where git carries it: what a write has under
/// way, and what each machine keeps for itself; `refs/` and `objects/`
/// travel.
const SHARED_IGNORE: &str = ".tmp*\n*.lock\n/HEAD\n/config\n/description\n/hooks/\n/logs/\n";

/// The history's `.gitignore` where git does not carry it: the researcher's
/// `git status` lists none of it.
const HIDDEN_IGNORE: &str = "*\n";

/// The history's `.gitignore` as `[history] shared` declares it, written
/// where it differs; left as it is where nothing declares it, a history
/// shared by hand staying shared. The seven lines were the researcher's to
/// write by hand.
fn share_as_declared(config: &ProjectConfig, directory: &Path) -> Result<(), VcsError> {
    let Some(shared) = config.history_shared() else {
        return Ok(());
    };
    let wanted = if shared { SHARED_IGNORE } else { HIDDEN_IGNORE };
    let path = directory.join(".gitignore");
    if std::fs::read_to_string(&path).is_ok_and(|held| held == wanted) {
        return Ok(());
    }
    std::fs::write(&path, wanted).map_err(|source| VcsError::Io { path, source })
}

/// `HEAD`, naming this machine's branch, and a bare repository's `config`,
/// written again in a history that arrived without them.
fn given_back(directory: &Path) -> Result<(), VcsError> {
    let write = |name: &str, text: String| {
        let path = directory.join(name);
        if path.is_file() {
            return Ok(());
        }
        std::fs::write(&path, text).map_err(|source| VcsError::Io { path, source })
    };
    write(
        "config",
        "[core]\n\trepositoryformatversion = 0\n\tbare = true\n".to_string(),
    )?;
    write("HEAD", format!("ref: {}\n", branch_of(&machine())))
}

/// `HEAD` made to name `machine`'s branch.
fn point_head(repository: &gix::Repository, machine: &str) -> Result<(), VcsError> {
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit, RefLog};
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the history in {}", repository.path().display()),
        message: error.to_string(),
    };
    let target: gix::refs::FullName = branch_of(machine)
        .as_str()
        .try_into()
        .map_err(|error| failed(&error))?;
    let head: gix::refs::FullName = "HEAD".try_into().map_err(|error| failed(&error))?;
    repository
        .edit_reference(RefEdit {
            change: Change::Update {
                log: LogChange {
                    mode: RefLog::AndReference,
                    force_create_reflog: false,
                    message: "".into(),
                },
                expected: PreviousValue::Any,
                new: gix::refs::Target::Symbolic(target),
            },
            name: head,
            deref: false,
        })
        .map_err(|error| failed(&error))?;
    Ok(())
}

/// The machine a snapshot's author names, where it names one.
fn machine_of_email(email: &str) -> Option<String> {
    email
        .strip_suffix(AT_MACHINE)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// A snapshot of its own for a script that wrote a file and changed nothing the
/// history keeps: what the project holds, said [`OUTSIDE`] where it differs
/// from the last, then a snapshot saying `message` even where its files are the
/// last one's, the script kept beside it. Its id, or `None` where no history is
/// kept.
pub fn script_step(
    config: &ProjectConfig,
    message: &str,
    script: Option<&(String, Vec<u8>)>,
) -> Result<Option<String>, VcsError> {
    if !keeps_history(config) {
        return Ok(None);
    }
    snapshot(config, OUTSIDE)?;
    let files = contents(&kept_files(config)?)?;
    let repository = history(config)?;
    let Snapshot::Taken(id) = commit_with(&repository, &files, message, true, &files)? else {
        return Ok(None);
    };
    if let Some((name, source)) = script {
        keep_script(config, &id, name, source)?;
    }
    Ok(Some(id))
}

/// The id of this machine's last snapshot, `None` where it has taken none.
pub fn head(config: &ProjectConfig) -> Result<Option<String>, VcsError> {
    head_of(config)
}

/// A commit of `files` on this machine's branch, where they differ from what
/// the files are compared with — this machine's last snapshot, with what the
/// other machines' branches that `basis` holds changed — and with
/// `even_unchanged`, even where they do not: a step that wrote a file and
/// changed no kept one. The branches `basis` holds are its parents beside this
/// machine's last: a join.
fn commit_with(
    repository: &gix::Repository,
    files: &[(String, Vec<u8>)],
    message: &str,
    even_unchanged: bool,
    basis: &[(String, Vec<u8>)],
) -> Result<Snapshot, VcsError> {
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("a snapshot in {}", repository.path().display()),
        message: error.to_string(),
    };
    let me = machine();
    let graph = Graph::load(repository)?;
    say_missing_trees(repository, &graph);
    let mut trees = Trees::default();
    let plan = plan(
        repository,
        &graph,
        &mut trees,
        &me,
        &hashed(repository, basis)?,
    )?;
    let mut editor = repository
        .edit_tree(gix::ObjectId::empty_tree(repository.object_hash()))
        .map_err(|error| failed(&error))?;
    let mut written = std::collections::BTreeMap::new();
    for (name, bytes) in files {
        let blob = repository
            .write_blob(bytes)
            .map_err(|error| failed(&error))?
            .detach();
        written.insert(name.clone(), blob);
        editor
            .upsert(name.as_str(), EntryKind::Blob, blob)
            .map_err(|error| failed(&error))?;
    }
    if written == plan.expected && !even_unchanged && (plan.own.is_some() || !written.is_empty()) {
        return Ok(Snapshot::Unchanged);
    }
    let tree = editor.write().map_err(|error| failed(&error))?.detach();
    let parents: Vec<gix::ObjectId> = plan.own.into_iter().chain(plan.joined).collect();
    // The first snapshot is the project as it was found, not a change.
    let message = if parents.is_empty() && message == OUTSIDE {
        FIRST
    } else {
        message
    };
    let signature = signature(&me);
    let mut time = gix::date::parse::TimeBuf::default();
    let signed = signature.to_ref(&mut time);
    let id = repository
        .commit_as(
            signed,
            signed,
            branch_of(&me).as_str(),
            message,
            tree,
            parents,
        )
        .map_err(|error| failed(&error))?;
    Ok(Snapshot::Taken(id.to_string()))
}

/// Says, once, a snapshot whose files the history no longer holds: a
/// write is kept all the same, while `log`, `--at` and `restore` cannot read
/// the history until it is repaired.
fn say_missing_trees(repository: &gix::Repository, graph: &Graph) {
    let mut missing: Vec<String> = graph
        .nodes
        .values()
        .filter(|node| !repository.has_object(node.tree))
        .map(|node| node.tree.to_string())
        .collect();
    if missing.is_empty() {
        return;
    }
    missing.sort();
    notice(format!(
        "the history in {} is damaged: {} snapshot{} lost {} files ({}); this write is \
         kept, but log, --at and restore cannot read the history",
        repository.path().display(),
        missing.len(),
        if missing.len() == 1 { "" } else { "s" },
        if missing.len() == 1 { "its" } else { "their" },
        missing.join(", ")
    ));
}

/// Each file's blob id, as the history would name it, written or not.
fn hashed(
    repository: &gix::Repository,
    files: &[(String, Vec<u8>)],
) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, VcsError> {
    let failed = reading(repository);
    files
        .iter()
        .map(|(name, bytes)| {
            gix::objs::compute_hash(repository.object_hash(), gix::objs::Kind::Blob, bytes)
                .map(|id| (name.clone(), id))
                .map_err(|error| failed(&error))
        })
        .collect()
}

// ------------------------------------------------------ several machines

// ------------------------------------------------- a folder made a project

/// The project above `config`, and how many of its snapshots changed the
/// files below `config`'s folder, where `config` keeps no history yet and the
/// project above keeps some: what a folder made a project of its own may
/// bring with it — counted as [`bring_history`] brings them. `None`
/// otherwise.
pub fn history_above(config: &ProjectConfig) -> Result<Option<(ProjectConfig, usize)>, VcsError> {
    if existing(config)?
        .is_some_and(|own| Graph::load(&own).is_ok_and(|graph| !graph.branches.is_empty()))
    {
        return Ok(None);
    }
    let Some(rc) = config.root().parent().and_then(project_config::find) else {
        return Ok(None);
    };
    let above = project_config::load(&rc).unwrap_or_else(|_| kept_configuration(&rc));
    let Some(within) = within_above(config, &above) else {
        return Ok(None);
    };
    let Some(repository) = existing(&above)? else {
        return Ok(None);
    };
    let graph = Graph::load(&repository)?;
    let count = copy_plan(&repository, &graph, &within)?.copied.len();
    Ok((count > 0).then_some((above, count)))
}

/// Which snapshots of the project above are copied, oldest first, and which
/// copy each of its snapshots stands for: one that changed none of the
/// folder's files stands for its first parent's.
struct CopyPlan {
    copied: Vec<(gix::ObjectId, gix::ObjectId)>,
    standing: std::collections::HashMap<gix::ObjectId, Option<gix::ObjectId>>,
}

fn copy_plan(
    repository: &gix::Repository,
    graph: &Graph,
    within: &str,
) -> Result<CopyPlan, VcsError> {
    let mut plan = CopyPlan {
        copied: Vec::new(),
        standing: std::collections::HashMap::new(),
    };
    for id in oldest_first(graph) {
        let node = &graph.nodes[&id];
        let mut parents: Vec<gix::ObjectId> = Vec::new();
        for parent in &node.parents {
            if let Some(Some(stood)) = plan.standing.get(parent)
                && !parents.contains(stood)
            {
                parents.push(*stood);
            }
        }
        let here = subtree(repository, node.tree, within)?;
        let before = match node.parents.first() {
            Some(parent) => subtree(repository, graph.nodes[parent].tree, within)?,
            None => None,
        };
        match here.filter(|_| here != before || parents.len() > 1) {
            Some(tree) => {
                plan.copied.push((id, tree));
                plan.standing.insert(id, Some(id));
            }
            None => {
                plan.standing.insert(id, parents.first().copied());
            }
        }
    }
    Ok(plan)
}

/// The snapshots of the project `above` brought into `config`'s history,
/// each holding the files below `config`'s folder alone, with its date, its
/// message and its machine; a snapshot that changed none of them is none of
/// its own. The project above keeps them all. Taken under the history's lock,
/// a snapshot taken meanwhile refusing it. How many were brought.
pub fn bring_history(config: &ProjectConfig, above: &ProjectConfig) -> Result<usize, VcsError> {
    let Some(within) = within_above(config, above) else {
        return Ok(0);
    };
    let Some(source) = existing(above)? else {
        return Ok(0);
    };
    let target = history(config)?;
    let _held = snapshot_lock(&history_directory(config))?;
    if !Graph::load(&target)?.branches.is_empty() {
        return Err(VcsError::Failed {
            what: format!("the history of {}", config.root().display()),
            message: "it keeps snapshots of its own already: the project above's are \
                      brought only before its first"
                .to_string(),
        });
    }
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("bringing the history into {}", target.path().display()),
        message: error.to_string(),
    };
    let read = reading(&source);
    let graph = Graph::load(&source)?;
    let plan = copy_plan(&source, &graph, &within)?;
    let mut written: std::collections::HashMap<gix::ObjectId, gix::ObjectId> =
        std::collections::HashMap::new();
    for (id, tree) in &plan.copied {
        let node = &graph.nodes[id];
        let mut parents: Vec<gix::ObjectId> = Vec::new();
        for parent in &node.parents {
            if let Some(Some(stood)) = plan.standing.get(parent)
                && let Some(copy) = written.get(stood)
                && !parents.contains(copy)
            {
                parents.push(*copy);
            }
        }
        let mut editor = target
            .edit_tree(gix::ObjectId::empty_tree(target.object_hash()))
            .map_err(|error| failed(&error))?;
        for (name, blob) in tree_files(&source, *tree)? {
            let bytes = source.find_blob(blob).map_err(|error| read(&error))?;
            let blob = target
                .write_blob(&bytes.data)
                .map_err(|error| failed(&error))?
                .detach();
            editor
                .upsert(name.as_str(), EntryKind::Blob, blob)
                .map_err(|error| failed(&error))?;
        }
        let tree = editor.write().map_err(|error| failed(&error))?.detach();
        let commit = source.find_commit(*id).map_err(|error| read(&error))?;
        let decoded = commit.decode().map_err(|error| read(&error))?;
        let copied = gix::objs::Commit {
            tree,
            parents: parents.into_iter().collect(),
            author: decoded.author().map_err(|error| read(&error))?.into(),
            committer: decoded.committer().map_err(|error| read(&error))?.into(),
            encoding: None,
            message: decoded.message.to_owned(),
            extra_headers: Vec::new(),
        };
        let copy = target
            .write_object(&copied)
            .map_err(|error| failed(&error))?
            .detach();
        written.insert(*id, copy);
    }
    for (machine, head) in &graph.branches {
        if let Some(Some(stood)) = plan.standing.get(head)
            && let Some(copy) = written.get(stood)
        {
            target
                .reference(
                    branch_of(machine).as_str(),
                    *copy,
                    gix::refs::transaction::PreviousValue::MustNotExist,
                    "brought from the project above",
                )
                .map_err(|error| failed(&error))?;
        }
    }
    Ok(plan.copied.len())
}

/// The history of a folder made a project shared as the one above is: its
/// `.gitignore`, where git carries that one, so that the copy goes with the
/// collection. Kept apart, a colleague's write there reached no one, and was
/// melted into a snapshot of the other computer's.
fn share_as_above(config: &ProjectConfig, above: &ProjectConfig) -> Result<(), VcsError> {
    // What the folder's own configuration declares is its own.
    if config.history_shared().is_some() {
        return Ok(());
    }
    let theirs = history_directory(above).join(".gitignore");
    let Ok(text) = std::fs::read_to_string(&theirs) else {
        return Ok(());
    };
    if text.lines().any(|line| line.trim() == "*") {
        return Ok(());
    }
    let ours = history_directory(config).join(".gitignore");
    std::fs::write(&ours, text).map_err(|source| VcsError::Io { path: ours, source })
}

/// The snapshot after those brought: the project as it is, joining every
/// machine's line brought — then held by this machine's branch alone, the
/// others' left unwritten. Each computer that brings the history writes the
/// same copies, and its own branch only: two that both wrote the other's
/// branch had git keep one, and a snapshot of the other's was lost.
fn keep_made_a_project(config: &ProjectConfig) -> Result<(), VcsError> {
    let repository = history(config)?;
    let _held = snapshot_lock(&history_directory(config))?;
    let files = contents(&kept_files(config)?)?;
    commit_with(&repository, &files, MADE_A_PROJECT, true, &files)?;
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the history of {}", config.root().display()),
        message: error.to_string(),
    };
    let graph = Graph::load(&repository)?;
    let me = machine();
    let Some(own) = graph.branch(&me) else {
        return Ok(());
    };
    let reached = graph.ancestors(own);
    for (name, id) in &graph.branches {
        if *name == me || !reached.contains(id) {
            continue;
        }
        repository
            .find_reference(branch_of(name).as_str())
            .map_err(|error| failed(&error))?
            .delete()
            .map_err(|error| failed(&error))?;
    }
    Ok(())
}

/// What a snapshot says that keeps a folder made a project as it is, after
/// the snapshots brought from the project above.
pub const MADE_A_PROJECT: &str = "the folder made a project of its own";

/// A folder made a project, reached the first time in this process: the
/// snapshots the project above kept of its files brought into its history,
/// before its own first, and said. Nothing is asked: the copy changes
/// nothing above, and once the project keeps a snapshot of its own they can
/// no longer be brought. What is to say of it, where something is: the
/// snapshots brought, or why they were not. A folder is reached once.
pub fn bring_from_above(config: &ProjectConfig) -> Option<String> {
    static REACHED: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
    if !keeps_history(config) {
        return None;
    }
    let root = dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
    match REACHED.lock() {
        Ok(reached) if reached.contains(&root) => return None,
        Ok(mut reached) => reached.push(root),
        Err(_) => return None,
    }
    let here = shown(config.root());
    // The project as it is now, after what was brought: its configuration
    // and its model, which the folder's snapshots never held, were otherwise
    // said changed — the model too, though it had not — by the first write
    // there, on each computer.
    let brought = history_above(config).and_then(|found| match found {
        Some((above, _)) => {
            let count = bring_history(config, &above)?;
            share_as_above(config, &above)?;
            keep_made_a_project(config)?;
            Ok(Some((above, count)))
        }
        None => Ok(None),
    });
    match brought {
        Ok(Some((above, count))) if count > 0 => Some(format!(
            "{count} snapshot{} of {here} brought from the history of {}, a project of its own now",
            if count == 1 { "" } else { "s" },
            shown(above.root())
        )),
        Ok(_) => None,
        Err(error) => Some(format!(
            "the snapshots the project above kept of {here} were not brought into its history: \
             {error}"
        )),
    }
}

/// A folder as a message names it: from the current folder where it lies
/// below it, `.` for that one.
fn shown(folder: &Path) -> String {
    let here = std::env::current_dir()
        .ok()
        .and_then(|here| dunce::canonicalize(here).ok());
    let folder_full = dunce::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    match here
        .as_ref()
        .and_then(|here| folder_full.strip_prefix(here).ok())
    {
        Some(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        Some(rest) => rest.display().to_string(),
        None => folder.display().to_string(),
    }
}

/// `config`'s folder as the snapshots of the project `above` name it.
fn within_above(config: &ProjectConfig, above: &ProjectConfig) -> Option<String> {
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = canonical(config.root())
        .strip_prefix(canonical(above.root()))
        .ok()?
        .to_path_buf();
    (!relative.as_os_str().is_empty()).then(|| slashed(&relative))
}

/// The tree at `within` in `tree`, where there is one.
fn subtree(
    repository: &gix::Repository,
    tree: gix::ObjectId,
    within: &str,
) -> Result<Option<gix::ObjectId>, VcsError> {
    let failed = reading(repository);
    let mut current = tree;
    for part in within.split('/').filter(|part| !part.is_empty()) {
        let tree = repository
            .find_tree(current)
            .map_err(|error| failed(&error))?;
        let mut next = None;
        for entry in tree.iter() {
            let entry = entry.map_err(|error| failed(&error))?;
            if entry.filename() == part && entry.mode().is_tree() {
                next = Some(entry.oid().to_owned());
                break;
            }
        }
        match next {
            Some(id) => current = id,
            None => return Ok(None),
        }
    }
    Ok(Some(current))
}

/// Every snapshot of the graph, each after those it follows.
fn oldest_first(graph: &Graph) -> Vec<gix::ObjectId> {
    let mut order = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut heads: Vec<gix::ObjectId> = graph.branches.iter().map(|(_, id)| *id).collect();
    heads.sort();
    for head in heads {
        let mut pending = vec![(head, false)];
        while let Some((id, done)) = pending.pop() {
            if done {
                order.push(id);
                continue;
            }
            if !seen.insert(id) {
                continue;
            }
            pending.push((id, true));
            if let Some(node) = graph.nodes.get(&id) {
                for parent in node.parents.iter().rev() {
                    if !seen.contains(parent) {
                        pending.push((*parent, false));
                    }
                }
            }
        }
    }
    order
}

/// One snapshot of the history, as the graph of every branch holds it.
#[derive(Debug, Clone)]
struct Node {
    parents: Vec<gix::ObjectId>,
    seconds: i64,
    offset: i32,
    message: String,
    tree: gix::ObjectId,
    /// The machine its author names; `None` for a snapshot kept before branches per machine.
    machine: Option<String>,
    /// The computer its author's name says; `None` where it says SampleKit, as
    /// a snapshot kept before machines were identifiers did.
    host: Option<String>,
}

/// Every branch of the history, by machine, and every snapshot they reach.
struct Graph {
    nodes: std::collections::HashMap<gix::ObjectId, Node>,
    branches: Vec<(String, gix::ObjectId)>,
}

impl Graph {
    fn load(repository: &gix::Repository) -> Result<Graph, VcsError> {
        let failed = reading(repository);
        let mut branches = Vec::new();
        let references = repository.references().map_err(|error| failed(&error))?;
        let prefixed = references
            .prefixed(BRANCHES)
            .map_err(|error| failed(&error))?;
        for reference in prefixed {
            let reference = reference.map_err(|error| failed(&error))?;
            let name = reference.name().as_bstr().to_string();
            let (Some(machine), Some(id)) = (
                name.strip_prefix(BRANCHES).map(str::to_string),
                reference.try_id().map(|id| id.detach()),
            ) else {
                continue;
            };
            branches.push((machine, id));
        }
        branches.sort();
        let mut nodes = std::collections::HashMap::new();
        let mut pending: Vec<gix::ObjectId> = branches.iter().map(|(_, id)| *id).collect();
        while let Some(id) = pending.pop() {
            if nodes.contains_key(&id) {
                continue;
            }
            let commit = repository.find_commit(id).map_err(|error| failed(&error))?;
            let parents: Vec<gix::ObjectId> = commit.parent_ids().map(|id| id.detach()).collect();
            let time = commit.time().map_err(|error| failed(&error))?;
            let author = commit.author().map_err(|error| failed(&error))?;
            let node = Node {
                parents: parents.clone(),
                seconds: time.seconds,
                offset: time.offset,
                message: commit
                    .message_raw()
                    .map_err(|error| failed(&error))?
                    .to_string()
                    .trim_end()
                    .to_string(),
                tree: commit.tree_id().map_err(|error| failed(&error))?.detach(),
                machine: machine_of_email(&author.email.to_string()),
                host: Some(author.name.to_string()),
            };
            nodes.insert(id, node);
            pending.extend(parents);
        }
        Ok(Graph { nodes, branches })
    }

    fn branch(&self, machine: &str) -> Option<gix::ObjectId> {
        self.branches
            .iter()
            .find(|(name, _)| name == machine)
            .map(|(_, id)| *id)
    }

    /// `id` and every snapshot it follows.
    fn ancestors(&self, id: gix::ObjectId) -> std::collections::HashSet<gix::ObjectId> {
        let mut seen = std::collections::HashSet::new();
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            if seen.insert(id)
                && let Some(node) = self.nodes.get(&id)
            {
                pending.extend(node.parents.iter().copied());
            }
        }
        seen
    }

    /// How far each snapshot lies from the first: one past its furthest
    /// parent's.
    fn generations(&self) -> std::collections::HashMap<gix::ObjectId, usize> {
        let mut generation = std::collections::HashMap::new();
        for start in self.nodes.keys() {
            let mut stack = vec![(*start, false)];
            while let Some((id, expanded)) = stack.pop() {
                if generation.contains_key(&id) {
                    continue;
                }
                let Some(node) = self.nodes.get(&id) else {
                    generation.insert(id, 0);
                    continue;
                };
                if expanded {
                    let depth = node
                        .parents
                        .iter()
                        .map(|parent| generation.get(parent).copied().unwrap_or(0) + 1)
                        .max()
                        .unwrap_or(1);
                    generation.insert(id, depth);
                } else {
                    stack.push((id, true));
                    for parent in &node.parents {
                        if !generation.contains_key(parent) {
                            stack.push((*parent, false));
                        }
                    }
                }
            }
        }
        generation
    }

    /// The last snapshots both `a` and `b` follow: each one they share that
    /// no other they share follows. Two machines that each joined the other's
    /// line share two — a criss-cross — and one alone took the other's own
    /// change, already held here, for one the other machine made: the join
    /// was refused, and its work kept as *changed outside SampleKit*.
    fn merge_bases(&self, a: gix::ObjectId, b: gix::ObjectId) -> Vec<gix::ObjectId> {
        let of_a = self.ancestors(a);
        let common: Vec<gix::ObjectId> = self
            .ancestors(b)
            .into_iter()
            .filter(|id| of_a.contains(id))
            .collect();
        let mut followed = std::collections::HashSet::new();
        for id in &common {
            let mut pending: Vec<gix::ObjectId> = self
                .nodes
                .get(id)
                .map(|node| node.parents.clone())
                .unwrap_or_default();
            while let Some(parent) = pending.pop() {
                if followed.insert(parent)
                    && let Some(node) = self.nodes.get(&parent)
                {
                    pending.extend(node.parents.iter().copied());
                }
            }
        }
        let mut bases: Vec<gix::ObjectId> = common
            .into_iter()
            .filter(|id| !followed.contains(id))
            .collect();
        bases.sort();
        bases
    }

    /// The machine each snapshot was taken on: the one its author names; one
    /// naming none is the machine's whose branch reaches it through its own
    /// snapshots.
    fn machines(&self) -> std::collections::HashMap<gix::ObjectId, String> {
        let mut machine: std::collections::HashMap<gix::ObjectId, String> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| node.machine.clone().map(|name| (*id, name)))
            .collect();
        for (name, head) in &self.branches {
            let name = name.as_str();
            let mut next = Some(*head);
            while let Some(id) = next {
                let Some(node) = self.nodes.get(&id) else {
                    break;
                };
                match &node.machine {
                    Some(other) if other != name => break,
                    Some(_) => {}
                    None => {
                        machine.entry(id).or_insert_with(|| name.to_string());
                    }
                }
                next = node.parents.first().copied();
            }
        }
        machine
    }
}

/// Trees read once each, by id.
#[derive(Default)]
struct Trees(
    std::collections::HashMap<gix::ObjectId, std::collections::BTreeMap<String, gix::ObjectId>>,
);

impl Trees {
    fn of(
        &mut self,
        repository: &gix::Repository,
        tree: gix::ObjectId,
    ) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, VcsError> {
        if let Some(files) = self.0.get(&tree) {
            return Ok(files.clone());
        }
        let files = tree_files(repository, tree)?;
        self.0.insert(tree, files.clone());
        Ok(files)
    }

    /// The files of a snapshot; none where there is no snapshot.
    fn at(
        &mut self,
        repository: &gix::Repository,
        graph: &Graph,
        id: Option<gix::ObjectId>,
    ) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, VcsError> {
        match id.and_then(|id| graph.nodes.get(&id)) {
            Some(node) => self.of(repository, node.tree),
            None => Ok(std::collections::BTreeMap::new()),
        }
    }
}

/// What a write on this machine is compared with, and joins.
struct Plan {
    /// This machine's last snapshot.
    own: Option<gix::ObjectId>,
    /// The other machines' last snapshots the files hold.
    joined: Vec<gix::ObjectId>,
    /// This machine's last files, with what the joined branches changed.
    expected: std::collections::BTreeMap<String, gix::ObjectId>,
    /// The snapshot each of those files is as.
    by: std::collections::BTreeMap<String, gix::ObjectId>,
}

/// Which other machines' branches `files` hold — every file a branch changed
/// since the last snapshot it shares with this machine's is as it holds it,
/// or, changed on this machine too, differs from both: merged — and what the
/// files are then compared with.
/// The files `other` changed since the snapshots both lines follow: those
/// it holds otherwise than every one of them. With no snapshot shared, every
/// file it holds.
fn changed_since_bases(
    repository: &gix::Repository,
    graph: &Graph,
    trees: &mut Trees,
    other_files: &std::collections::BTreeMap<String, gix::ObjectId>,
    bases: &[gix::ObjectId],
) -> Result<Vec<String>, VcsError> {
    let mut bases_files = Vec::new();
    for base in bases {
        bases_files.push(trees.at(repository, graph, Some(*base))?);
    }
    if bases_files.is_empty() {
        bases_files.push(std::collections::BTreeMap::new());
    }
    let mut names: Vec<&String> = other_files
        .keys()
        .chain(bases_files.iter().flat_map(|files| files.keys()))
        .collect();
    names.sort();
    names.dedup();
    Ok(names
        .into_iter()
        .filter(|name| {
            bases_files
                .iter()
                .all(|base| other_files.get(*name) != base.get(*name))
        })
        .cloned()
        .collect())
}

fn plan(
    repository: &gix::Repository,
    graph: &Graph,
    trees: &mut Trees,
    me: &str,
    files: &std::collections::BTreeMap<String, gix::ObjectId>,
) -> Result<Plan, VcsError> {
    let own = graph.branch(me);
    let own_files = trees.at(repository, graph, own)?;
    let mut expected = own_files.clone();
    let mut by: std::collections::BTreeMap<String, gix::ObjectId> = match own {
        Some(own) => own_files.keys().map(|name| (name.clone(), own)).collect(),
        None => std::collections::BTreeMap::new(),
    };
    let held = own.map(|own| graph.ancestors(own)).unwrap_or_default();
    // Each other branch this one does not hold yet, nor another of them.
    let mut others: Vec<gix::ObjectId> = graph
        .branches
        .iter()
        .filter(|(name, id)| name != me && !held.contains(id))
        .map(|(_, id)| *id)
        .collect();
    // Two branches on one snapshot are one parent.
    let mut seen = std::collections::HashSet::new();
    others.retain(|id| seen.insert(*id));
    let reaches: Vec<std::collections::HashSet<gix::ObjectId>> =
        others.iter().map(|id| graph.ancestors(*id)).collect();
    let others: Vec<gix::ObjectId> = others
        .iter()
        .enumerate()
        .filter(|(at, id)| {
            !reaches
                .iter()
                .enumerate()
                .any(|(other, reach)| other != *at && others[other] != **id && reach.contains(*id))
        })
        .map(|(_, id)| *id)
        .collect();
    let mut joined = Vec::new();
    // The closest other branch, for a machine with none of its own yet.
    let mut closest: Option<(usize, gix::ObjectId)> = None;
    for other in others {
        let bases = own
            .map(|own| graph.merge_bases(own, other))
            .unwrap_or_default();
        let other_files = trees.at(repository, graph, Some(other))?;
        let changed = changed_since_bases(repository, graph, trees, &other_files, &bases)?;
        let changed: Vec<&String> = changed.iter().collect();
        // A file is held as the other branch holds it, or moved on from this
        // machine's last content — merged, or edited by hand since it was
        // pulled: a colleague's work under an edit by hand was taken for no
        // join, and a restore of the file took that work away.
        let holds = changed.iter().all(|name| {
            let now = files.get(*name);
            now == other_files.get(*name) || now != own_files.get(*name)
        });
        if !holds {
            if own.is_none() {
                let apart = other_files
                    .keys()
                    .chain(files.keys())
                    .filter(|name| other_files.get(*name) != files.get(*name))
                    .count();
                if closest.is_none_or(|(held, _)| apart < held) {
                    closest = Some((apart, other));
                }
            }
            continue;
        }
        for name in changed {
            match other_files.get(name) {
                Some(blob) => {
                    expected.insert(name.clone(), *blob);
                    by.insert(name.clone(), other);
                }
                None => {
                    expected.remove(name);
                    by.remove(name);
                }
            }
        }
        joined.push(other);
    }
    // A clone's first write, its files edited since the clone: the history git
    // brought is still where this machine's begins, and what differs is what
    // changed outside SampleKit here. Joined to nothing, its first snapshot
    // was a root, and a restore to it took the other machine's work away.
    if own.is_none()
        && joined.is_empty()
        && let Some((_, other)) = closest
    {
        let other_files = trees.at(repository, graph, Some(other))?;
        for (name, blob) in &other_files {
            expected.insert(name.clone(), *blob);
            by.insert(name.clone(), other);
        }
        joined.push(other);
    }
    Ok(Plan {
        own,
        joined,
        expected,
        by,
    })
}

/// The files a snapshot was taken over, by blob: its parent's; a join's,
/// its first parent's with what each other parent changed since their merge
/// base; none for a first snapshot.
fn before_of(
    repository: &gix::Repository,
    graph: &Graph,
    trees: &mut Trees,
    id: gix::ObjectId,
) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, VcsError> {
    let Some(node) = graph.nodes.get(&id) else {
        return Ok(std::collections::BTreeMap::new());
    };
    let Some(first) = node.parents.first().copied() else {
        return Ok(std::collections::BTreeMap::new());
    };
    let mut files = trees.at(repository, graph, Some(first))?;
    for other in node.parents.iter().skip(1) {
        let bases = graph.merge_bases(first, *other);
        let other_files = trees.at(repository, graph, Some(*other))?;
        let changed = changed_since_bases(repository, graph, trees, &other_files, &bases)?;
        for name in changed {
            match other_files.get(&name) {
                Some(blob) => {
                    files.insert(name, *blob);
                }
                None => {
                    files.remove(&name);
                }
            }
        }
    }
    Ok(files)
}

// ---------------------------------------------------------- outputs

/// The prefix of the tags that tie a file SampleKit wrote to the snapshot it
/// was made from, each named after the file's SHA-256.
const OUTPUT: &str = "output/";

/// What a figure carries in its metadata, before its snapshot's id: a second
/// way back where the file's hash no longer matches.
pub const CARRIED: &str = "samplekit snapshot ";

/// A file SampleKit wrote, as its tag in the history records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The file's SHA-256, in hex.
    pub hash: String,
    /// The snapshot it was made from.
    pub snapshot: String,
    /// What wrote it: the command line, the workbench's action, or Python's.
    pub said: String,
    /// Where it was written, from the project's folder where it lies inside.
    pub written: String,
    /// The samples it was made from, as the history names them.
    pub samples: Vec<String>,
    /// The folder it was written from, from the project's folder where it lies
    /// inside — where its command, run again, finds its paths.
    pub ran_in: String,
    /// When it was written, as a snapshot's time is.
    pub seconds: i64,
    pub offset: i32,
}

/// The SHA-256 of a file, in hex, as the history reads it ([`as_kept`]): a
/// file git brought with CRLF is still the one written.
pub fn file_hash(path: &Path) -> Result<String, VcsError> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)
        .map(as_kept)
        .map_err(|source| VcsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Before a file is written from the project: a snapshot of what it holds,
/// said [`OUTSIDE`] where it differs from the last, and the id of the
/// snapshot the file is made from — `None` where no history is kept.
pub fn snapshot_for_output(config: &ProjectConfig) -> Result<Option<String>, VcsError> {
    if !keeps_history(config) {
        return Ok(None);
    }
    if let Snapshot::Taken(id) = snapshot(config, OUTSIDE)? {
        return Ok(Some(id));
    }
    // The files are as this machine last kept them, or as the joined branches
    // changed them: the snapshot that holds them, else the join taken for the
    // file to name.
    let files = contents(&kept_files(config)?)?;
    let repository = history(config)?;
    let graph = Graph::load(&repository)?;
    let mut trees = Trees::default();
    let now = hashed(&repository, &files)?;
    let plan = plan(&repository, &graph, &mut trees, &machine(), &now)?;
    let holding = plan
        .own
        .into_iter()
        .chain(plan.joined.iter().copied())
        .map(|id| Ok((id, trees.at(&repository, &graph, Some(id))? == now)))
        .collect::<Result<Vec<_>, VcsError>>()?;
    if let Some((id, _)) = holding.iter().find(|(_, holds)| *holds) {
        return Ok(Some(id.to_string()));
    }
    match commit_with(&repository, &files, JOINED, true, &files)? {
        Snapshot::Taken(id) => Ok(Some(id)),
        _ => Ok(None),
    }
}

/// After the file is written: a tag named after its hash, on the snapshot it
/// was made from, saying what wrote it, from which folder, where, and from
/// which samples. The file itself is not kept.
///
/// Each write is recorded: the same contents made twice — once `--at` a
/// snapshot, once from today's project that holds the same values — are two
/// records, the first `output/<hash>@<machine>`, the next
/// `output/<hash>@<machine>.2` and on, so that `explain` can tell them apart by
/// where each was written. A record always names the machine that wrote it, so
/// that no two machines write one tag, even two that each kept a history alone
/// before they were synchronised. A write that records exactly what an existing
/// record says adds nothing.
pub fn tag_output(
    config: &ProjectConfig,
    snapshot: &str,
    written: &Path,
    said: &str,
    samples: &[PathBuf],
    ran_in: &Path,
) -> Result<String, VcsError> {
    let hash = file_hash(written)?;
    let repository = history(config)?;
    let failed = reading(&repository);
    let target = gix::ObjectId::from_hex(snapshot.as_bytes()).map_err(|error| failed(&error))?;
    let written = inside(config.root(), written).unwrap_or_else(|| {
        dunce::canonicalize(written)
            .unwrap_or_else(|_| written.to_path_buf())
            .display()
            .to_string()
    });
    let samples: Vec<String> = samples
        .iter()
        .filter_map(|path| inside(config.root(), path))
        .collect();
    let ran_in = inside(config.root(), ran_in).map_or_else(
        || ran_in.display().to_string(),
        |relative| {
            if relative.is_empty() {
                ".".to_string()
            } else {
                relative
            }
        },
    );
    let records = records_of(&repository, &hash)?;
    // The same contents made again, the same way, from the same snapshot:
    // an export remade as it was is the file it was, already recorded.
    if records.iter().any(|(_, made)| {
        made.snapshot == snapshot
            && made.said == said
            && made.written == written
            && made.ran_in == ran_in
            && made.samples == samples
    }) {
        return Ok(hash);
    }
    let me = machine();
    let stem = format!("{hash}@{me}");
    let taken = |name: &str| records.iter().any(|(tag, _)| tag == name);
    let name = if taken(&stem) {
        let n = (2..).find(|n| !taken(&format!("{stem}.{n}"))).unwrap_or(2);
        format!("{OUTPUT}{stem}.{n}")
    } else {
        format!("{OUTPUT}{stem}")
    };
    let message = format!(
        "{said}\n\nwritten: {written}\nran in: {ran_in}\nsamples: {}\n",
        samples.join(", ")
    );
    let tagger = signature(&me);
    let mut time = gix::date::parse::TimeBuf::default();
    repository
        .tag(
            name,
            target,
            gix::object::Kind::Commit,
            Some(tagger.to_ref(&mut time)),
            message,
            gix::refs::transaction::PreviousValue::MustNotExist,
        )
        .map_err(|error| failed(&error))?;
    Ok(hash)
}

/// Every record of a file's contents, by the name its tag takes after `output/`
/// — `<hash>@<machine>`, then `<hash>@<machine>.2` and on — newest first.
fn records_of(repository: &gix::Repository, hash: &str) -> Result<Vec<(String, Made)>, VcsError> {
    let failed = reading(repository);
    let mut found = Vec::new();
    let references = repository.references().map_err(|error| failed(&error))?;
    let prefixed = references
        .prefixed(format!("refs/tags/{OUTPUT}{hash}").as_str())
        .map_err(|error| failed(&error))?;
    for reference in prefixed {
        let reference = reference.map_err(|error| failed(&error))?;
        let name = reference.name().as_bstr().to_string();
        let Some(tag) = name.strip_prefix(&format!("refs/tags/{OUTPUT}")) else {
            continue;
        };
        if !tag
            .strip_prefix(hash)
            .is_some_and(|rest| rest.starts_with('@'))
        {
            continue;
        }
        let made = made_by_tag(repository, reference.id().detach(), hash)?;
        found.push((tag.to_string(), made));
    }
    found.sort_by_key(|(tag, made)| {
        let n: usize = tag
            .rsplit_once('.')
            .and_then(|(_, n)| n.parse().ok())
            .unwrap_or(1);
        std::cmp::Reverse((made.seconds, n))
    });
    Ok(found)
}

/// Both at once, for a file written in one step: an export, a table.
pub fn keep_output(
    config: &ProjectConfig,
    written: &Path,
    said: &str,
    samples: &[PathBuf],
) -> Result<Option<String>, VcsError> {
    match snapshot_for_output(config)? {
        Some(snapshot) => {
            let here = std::env::current_dir().unwrap_or_default();
            tag_output(config, &snapshot, written, said, samples, &here).map(Some)
        }
        None => Ok(None),
    }
}

/// A file written from `samples`, tied in the history of each project they
/// belong to, with that project's samples. The failures, by folder, are the
/// caller's to say as warnings.
pub fn keep_output_from(
    written: &Path,
    said: &str,
    samples: &[PathBuf],
) -> Vec<(PathBuf, VcsError)> {
    let mut failures = Vec::new();
    for config in projects_of(samples) {
        let root = dunce::canonicalize(config.root()).unwrap_or_default();
        let theirs: Vec<PathBuf> = samples
            .iter()
            .filter(|path| dunce::canonicalize(path).is_ok_and(|path| path.starts_with(&root)))
            .cloned()
            .collect();
        if let Err(error) = keep_output(&config, written, said, &theirs) {
            failures.push((config.root().to_path_buf(), error));
        }
    }
    failures
}

/// What the history records of a file with this hash, if SampleKit wrote
/// it: its newest record.
pub fn made(config: &ProjectConfig, hash: &str) -> Result<Option<Made>, VcsError> {
    Ok(made_all(config, hash)?.into_iter().next())
}

/// Every record of a file with this hash, newest first: the same contents
/// may have been written more than once, from different snapshots or to
/// different places.
pub fn made_all(config: &ProjectConfig, hash: &str) -> Result<Vec<Made>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(Vec::new());
    };
    Ok(records_of(&repository, hash)?
        .into_iter()
        .map(|(_, made)| made)
        .collect())
}

/// The files the history records as made from one snapshot, newest first.
pub fn made_from(config: &ProjectConfig, snapshot: &str) -> Result<Vec<Made>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(Vec::new());
    };
    let failed = reading(&repository);
    let mut found = Vec::new();
    let references = repository.references().map_err(|error| failed(&error))?;
    let prefixed = references
        .prefixed(format!("refs/tags/{OUTPUT}").as_str())
        .map_err(|error| failed(&error))?;
    for reference in prefixed {
        let reference = reference.map_err(|error| failed(&error))?;
        let name = reference.name().as_bstr().to_string();
        // `<hash>@<machine>` or `<hash>@<machine>.<n>`: the hash is the name
        // before its machine; one naming none is no record.
        let tag = name.rsplit('/').next().unwrap_or_default();
        let Some((hash, _)) = tag.split_once('@') else {
            continue;
        };
        let hash = hash.to_string();
        let made = made_by_tag(&repository, reference.id().detach(), &hash)?;
        if made.snapshot == snapshot {
            found.push(made);
        }
    }
    found.sort_by_key(|made| std::cmp::Reverse(made.seconds));
    Ok(found)
}

fn made_by_tag(
    repository: &gix::Repository,
    tag: gix::ObjectId,
    hash: &str,
) -> Result<Made, VcsError> {
    let failed = reading(repository);
    let object = repository
        .find_object(tag)
        .map_err(|error| failed(&error))?;
    let tag = object.try_into_tag().map_err(|error| failed(&error))?;
    let decoded = tag.decode().map_err(|error| failed(&error))?;
    let message = decoded.message.to_string();
    let (said, rest) = message.split_once("\n\n").unwrap_or((message.as_str(), ""));
    let field = |key: &str| {
        rest.lines()
            .find_map(|line| line.strip_prefix(key))
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let samples = field("samples:")
        .split(", ")
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    let time = decoded
        .tagger()
        .map_err(|error| failed(&error))?
        .map(|tagger| tagger.time())
        .transpose()
        .map_err(|error| failed(&error))?
        .unwrap_or_default();
    Ok(Made {
        hash: hash.to_string(),
        snapshot: decoded.target().to_string(),
        said: said.trim().to_string(),
        written: field("written:"),
        ran_in: field("ran in:"),
        samples,
        seconds: time.seconds,
        offset: time.offset,
    })
}

/// The snapshot a file carries in its metadata, where SampleKit wrote one there
/// — a figure's.
pub fn carried_snapshot(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let marker = CARRIED.as_bytes();
    let at = bytes
        .windows(marker.len())
        .position(|window| window == marker)?;
    let id: String = bytes[at + marker.len()..]
        .iter()
        .take(40)
        .map(|byte| *byte as char)
        .collect();
    (id.len() == 40 && id.chars().all(|c| c.is_ascii_hexdigit())).then_some(id)
}

// ------------------------------------------------------------- reading

/// One snapshot, as `samplekit log` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its commit, in full.
    pub id: String,
    /// When it was taken: seconds since the epoch, and the offset from UTC,
    /// in seconds, of the machine that took it.
    pub seconds: i64,
    pub offset: i32,
    /// What wrote, as the snapshot says it.
    pub message: String,
    /// The kept paths that differ from the state it was taken over — every
    /// one, for the first.
    pub changed: Vec<String>,
    /// The machine that took it, as it is shown: *you* for this one, the name
    /// this machine gave it, else the host its snapshots name.
    pub machine: String,
    /// That machine's identifier, its branch's name.
    pub machine_id: String,
    /// The computer it is, as its snapshots name it — this one's too — for an
    /// account read away from this machine.
    pub host: String,
    /// The snapshots it was taken over: its machine's last first, then those
    /// of the machines it joined.
    pub parents: Vec<String>,
    /// The machines it joined, where it was taken over another machine's
    /// snapshot, as they are shown.
    pub joined: Vec<String>,
    /// Dated before the snapshot it follows on its machine.
    pub clock_went_back: bool,
}

/// What this machine calls it, you.
pub const YOU: &str = "you";

/// How each machine of a history is shown, by identifier, and the host each
/// is: *you* for this one; the name this machine gave another; else the host
/// its newest snapshot names — the identifier's first eight digits after it
/// where two machines shown share one host — or, for a snapshot kept before
/// machines were identifiers, its machine's name, which was its host's.
fn shown_machines(
    graph: &Graph,
    machines: &std::collections::HashMap<gix::ObjectId, String>,
    me: &str,
) -> std::collections::HashMap<String, (String, String)> {
    let mut hosts: std::collections::HashMap<String, (i64, String)> =
        std::collections::HashMap::new();
    for (id, identifier) in machines {
        let node = &graph.nodes[id];
        let host = node.host.clone().unwrap_or_else(|| identifier.clone());
        let newest = hosts
            .entry(identifier.clone())
            .or_insert((i64::MIN, String::new()));
        if node.seconds >= newest.0 {
            *newest = (node.seconds, host);
        }
    }
    let names = machine_names();
    let mut shared: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (identifier, (_, host)) in &hosts {
        if identifier != me && !names.contains_key(identifier) {
            *shared.entry(host.as_str()).or_default() += 1;
        }
    }
    hosts
        .iter()
        .map(|(identifier, (_, host))| {
            let shown = if identifier == me {
                YOU.to_string()
            } else if let Some(name) = names.get(identifier) {
                name.clone()
            } else if shared.get(host.as_str()).copied().unwrap_or(0) > 1 || host == YOU {
                let digits: String = identifier.chars().take(8).collect();
                format!("{host} {digits}")
            } else {
                host.clone()
            };
            (identifier.clone(), (shown, host.clone()))
        })
        .collect()
}

/// The history of every machine, newest first by when each snapshot was taken —
/// a snapshot before those it follows where two share a second; empty where
/// none is kept yet.
pub fn entries(config: &ProjectConfig) -> Result<Vec<Entry>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(Vec::new());
    };
    let graph = Graph::load(&repository)?;
    let generations = graph.generations();
    let me = machine();
    let machines = graph.machines();
    let shown = shown_machines(&graph, &machines, &me);
    let shown_as = |identifier: &String| {
        shown
            .get(identifier)
            .cloned()
            .unwrap_or_else(|| (identifier.clone(), identifier.clone()))
    };
    let mut trees = Trees::default();
    let order = newest_first(&graph, &generations);
    let mut found = Vec::new();
    for id in order {
        let node = &graph.nodes[&id];
        // A snapshot dated before the one it follows on its machine: that
        // machine's clock was set back, or had been ahead.
        let clock_went_back = node
            .parents
            .first()
            .and_then(|parent| graph.nodes.get(parent))
            .is_some_and(|parent| {
                parent.seconds > node.seconds && machines.get(&id) == machines.get(&node.parents[0])
            });
        let files = trees.of(&repository, node.tree)?;
        let before = before_of(&repository, &graph, &mut trees, id)?;
        let mut changed: Vec<String> = files
            .iter()
            .filter(|(name, id)| before.get(*name) != Some(*id))
            .map(|(name, _)| name.clone())
            .chain(
                before
                    .keys()
                    .filter(|name| !files.contains_key(*name))
                    .cloned(),
            )
            .collect();
        changed.sort();
        let machine_id = machines.get(&id).cloned().unwrap_or_default();
        let mut joined: Vec<String> = Vec::new();
        for parent in &node.parents {
            if let Some(other) = machines.get(parent)
                && *other != machine_id
            {
                let (other, _) = shown_as(other);
                if !joined.contains(&other) {
                    joined.push(other);
                }
            }
        }
        let (machine, host) = shown_as(&machine_id);
        found.push(Entry {
            id: id.to_string(),
            seconds: node.seconds,
            offset: node.offset,
            message: node.message.clone(),
            changed,
            machine,
            machine_id,
            host,
            parents: node.parents.iter().map(ToString::to_string).collect(),
            joined,
            clock_went_back,
        });
    }
    Ok(found)
}

/// Every snapshot, the newest first, each before those it follows: a
/// machine's own are in the order their chain gives, whatever their clock
/// said, and their dates only interleave several machines'.
fn newest_first(
    graph: &Graph,
    generations: &std::collections::HashMap<gix::ObjectId, usize>,
) -> Vec<gix::ObjectId> {
    let mut children: std::collections::HashMap<gix::ObjectId, usize> =
        graph.nodes.keys().map(|id| (*id, 0)).collect();
    for node in graph.nodes.values() {
        for parent in &node.parents {
            if let Some(count) = children.get_mut(parent) {
                *count += 1;
            }
        }
    }
    let key = |id: &gix::ObjectId| {
        let node = &graph.nodes[id];
        (node.seconds, generations.get(id).copied().unwrap_or(0), *id)
    };
    let mut ready: std::collections::BinaryHeap<(i64, usize, gix::ObjectId)> = children
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| key(id))
        .collect();
    let mut order = Vec::with_capacity(graph.nodes.len());
    while let Some((_, _, id)) = ready.pop() {
        order.push(id);
        for parent in &graph.nodes[&id].parents {
            if let Some(count) = children.get_mut(parent) {
                *count -= 1;
                if *count == 0 {
                    ready.push(key(parent));
                }
            }
        }
    }
    order
}

/// The kept files a snapshot was taken over: its parent's; a join's, its first
/// parent's with what the others changed since their merge base; none for a
/// first snapshot.
pub fn files_before(
    config: &ProjectConfig,
    id: &str,
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(std::collections::BTreeMap::new());
    };
    let failed = reading(&repository);
    let id = gix::ObjectId::from_hex(id.as_bytes()).map_err(|error| failed(&error))?;
    let graph = Graph::load(&repository)?;
    let mut trees = Trees::default();
    blobs(&repository, before_of(&repository, &graph, &mut trees, id)?)
}

/// The files as this machine last kept them, with what the other machines'
/// branches it would join changed: what the files are compared with, to say
/// what changed outside SampleKit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastKept {
    pub files: std::collections::BTreeMap<String, Vec<u8>>,
    /// The snapshot each file is as.
    pub by: std::collections::BTreeMap<String, String>,
}

/// What the files are compared with; `None` where no history is kept.
pub fn last_kept(config: &ProjectConfig) -> Result<Option<LastKept>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(None);
    };
    let graph = Graph::load(&repository)?;
    if graph.branches.is_empty() {
        return Ok(None);
    }
    let now = hashed(&repository, &contents(&kept_files(config)?)?)?;
    let mut trees = Trees::default();
    let plan = plan(&repository, &graph, &mut trees, &machine(), &now)?;
    Ok(Some(LastKept {
        files: blobs(&repository, plan.expected)?,
        by: plan
            .by
            .into_iter()
            .map(|(name, id)| (name, id.to_string()))
            .collect(),
    }))
}

/// Files by blob, read.
fn blobs(
    repository: &gix::Repository,
    files: std::collections::BTreeMap<String, gix::ObjectId>,
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, VcsError> {
    let failed = reading(repository);
    files
        .into_iter()
        .map(|(name, blob)| {
            let data = repository
                .find_blob(blob)
                .map_err(|error| failed(&error))?
                .data
                .clone();
            Ok((name, data))
        })
        .collect()
}

/// The kept files of a snapshot, by the names the snapshot gives them.
pub fn files_at(
    config: &ProjectConfig,
    id: &str,
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Err(VcsError::Failed {
            what: format!("the history in {}", history_directory(config).display()),
            message: "none is kept yet".to_string(),
        });
    };
    let failed = reading(&repository);
    let id = gix::ObjectId::from_hex(id.as_bytes()).map_err(|error| failed(&error))?;
    let tree = repository
        .find_commit(id)
        .map_err(|error| failed(&error))?
        .tree_id()
        .map_err(|error| failed(&error))?
        .detach();
    tree_files(&repository, tree)?
        .into_iter()
        .map(|(name, blob)| {
            let data = repository
                .find_blob(blob)
                .map_err(|error| failed(&error))?
                .data
                .clone();
            Ok((name, data))
        })
        .collect()
}

/// The kept files as they are now, by the names a snapshot would give them.
pub fn files_now(
    config: &ProjectConfig,
) -> Result<std::collections::BTreeMap<String, Vec<u8>>, VcsError> {
    Ok(contents(&kept_files(config)?)?.into_iter().collect())
}

/// A change the history can take back: the files a snapshot changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undoable {
    pub snapshot: String,
    pub message: String,
    pub seconds: i64,
    pub offset: i32,
    pub files: Vec<Restored>,
}

/// One file of a change: as it was before — `None` where the change made it —
/// and as the change left it — `None` where the change removed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Option<Vec<u8>>,
}

/// What an undo's own snapshot says, before what it took back: the history
/// never offers an undo to be undone.
pub const UNDO: &str = "undo · ";

/// The last change the history can take back, for an undo that outlives the
/// workbench: the newest snapshot a write took on this machine — not a change
/// made outside SampleKit, not the first, not a folder made a project — its
/// `.samplekitrc` the researcher's own, which `u` removed — not a join, not
/// an undo — whose files
/// do not already hold what they held before it. Whether they still hold what
/// it wrote is the caller's to check, as for any undo.
pub fn last_undoable(config: &ProjectConfig) -> Result<Option<Undoable>, VcsError> {
    let entries = entries(config)?;
    // This machine's changes alone: another's is taken back there.
    let me = machine();
    for entry in &entries {
        if entry.machine_id != me
            || entry.message == OUTSIDE
            || entry.message == FIRST
            || entry.message == MADE_A_PROJECT
            || entry.message == JOINED
            || entry.message.starts_with(UNDO)
            || entry.message.contains(&format!(" · {UNDO}"))
            || entry.parents.is_empty()
        {
            continue;
        }
        let before = files_before(config, &entry.id)?;
        let after = files_at(config, &entry.id)?;
        let files: Vec<Restored> = entry
            .changed
            .iter()
            .filter(|name| !name.starts_with("@model/"))
            .map(|name| Restored {
                path: config.root().join(name),
                before: before.get(name).cloned(),
                after: after.get(name).cloned(),
            })
            .collect();
        // Already as it was before: taken back once, by any means.
        if files
            .iter()
            .all(|file| std::fs::read(&file.path).ok().map(as_kept) == file.before)
        {
            continue;
        }
        return Ok(Some(Undoable {
            snapshot: entry.id.clone(),
            message: entry.message.clone(),
            seconds: entry.seconds,
            offset: entry.offset,
            files,
        }));
    }
    Ok(None)
}

/// The project as a snapshot kept it, written out under `base` for a command
/// to read — the samples, `.samplekitrc` and the model's sources, each where
/// the configuration looks for it — and its folder there. The environment
/// the model runs in is today's, named where it is: a snapshot keeps no
/// environment.
pub fn materialize(
    config: &ProjectConfig,
    snapshot: &str,
    base: &Path,
) -> Result<PathBuf, VcsError> {
    let files = files_at(config, snapshot)?;
    let written = |path: &Path, bytes: &[u8]| {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| VcsError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(path, bytes).map_err(|source| VcsError::Io {
            path: path.to_path_buf(),
            source,
        })
    };
    // Where the model was declared then, which may lie above the project.
    let rc = files
        .get(project_config::FILENAME)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
    let declared = rc
        .as_deref()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("model")?.get("path")?.as_str().map(PathBuf::from));
    let model_directory = declared
        .as_ref()
        .map(|path| path.parent().map(Path::to_path_buf).unwrap_or_default());
    // Deep enough below `base` that a model up the tree lands inside it.
    let ups = model_directory.as_ref().map_or(0, |directory| {
        directory
            .components()
            .take_while(|part| matches!(part, std::path::Component::ParentDir))
            .count()
    });
    let mut root = base.to_path_buf();
    for level in 0..ups {
        root.push(format!("above-{level}"));
    }
    root.push("project");
    let mut rc = rc;
    for (name, bytes) in &files {
        if name == project_config::FILENAME {
            continue;
        }
        let target = match name.strip_prefix("@model/") {
            Some(rest) => match &model_directory {
                Some(directory) if !directory.is_absolute() => root.join(directory).join(rest),
                // A model named by an absolute path is read from its copy.
                _ => root.join(".samplekit-model").join(rest),
            },
            None => root.join(name),
        };
        written(&target, bytes)?;
    }
    if let (Some(text), Some(declared)) = (&rc, &declared)
        && declared.is_absolute()
        && let Ok(mut document) = text.parse::<toml_edit::DocumentMut>()
        && let Some(file) = declared.file_name()
    {
        document["model"]["path"] = toml_edit::value(
            Path::new(".samplekit-model")
                .join(file)
                .to_string_lossy()
                .into_owned(),
        );
        rc = Some(document.to_string());
    }
    // What the configuration imports is no file of the project's, and no
    // snapshot keeps it: it is read where it is, as it is now. Named from the
    // project's folder, it is named from there still.
    if let Some(text) = &rc
        && let Ok(mut document) = text.parse::<toml_edit::DocumentMut>()
        && document.contains_key("import")
    {
        let real =
            dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
        let placed = |written: &str| -> String {
            let path = Path::new(written);
            if path.is_absolute() || written.starts_with('~') {
                written.to_string()
            } else {
                real.join(path).to_string_lossy().into_owned()
            }
        };
        let item = &mut document["import"];
        if let Some(one) = item.as_str() {
            *item = toml_edit::value(placed(one));
        } else if let Some(list) = item.as_array_mut() {
            // Each of a list alike.
            for entry in list.iter_mut() {
                if let Some(one) = entry.as_str() {
                    *entry = toml_edit::Value::from(placed(one));
                }
            }
        }
        rc = Some(document.to_string());
    }
    // The environment `[model] python` names is today's, where it is: named
    // from the project's folder, it is named from there still — `../.venv`
    // was looked for beside the snapshot written out, and found nowhere.
    if let Some(text) = &rc
        && let Ok(mut document) = text.parse::<toml_edit::DocumentMut>()
        && let Some(python) = document
            .get("model")
            .and_then(|model| model.get("python"))
            .and_then(|python| python.as_str())
            .map(str::to_string)
        && !Path::new(&python).is_absolute()
        && !python.starts_with('~')
    {
        let real =
            dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
        // `../.venv` from the project is the folder above's, said as such.
        let mut named = PathBuf::new();
        for part in real.join(&python).components() {
            match part {
                std::path::Component::ParentDir => {
                    named.pop();
                }
                std::path::Component::CurDir => {}
                other => named.push(other),
            }
        }
        document["model"]["python"] = toml_edit::value(named.to_string_lossy().into_owned());
        rc = Some(document.to_string());
    }
    if let Some(text) = rc {
        written(&root.join(project_config::FILENAME), text.as_bytes())?;
    }
    Ok(root)
}

/// The project's history where one is kept, opened; `None` where none is.
fn existing(config: &ProjectConfig) -> Result<Option<gix::Repository>, VcsError> {
    let directory = history_directory(config);
    // A history a clone brought, without the files each machine keeps for
    // itself, is read as it is written: given them back. A fresh clone had
    // no history until it wrote — no `log`, no `--at`, and a formula changed
    // since the values were computed not seen.
    if !directory.join("HEAD").is_file() && directory.join("objects").is_dir() {
        given_back(&directory)?;
    }
    if !directory.join("HEAD").is_file() {
        return Ok(None);
    }
    share_as_declared(config, &directory)?;
    gix::open_opts(&directory, gix::open::Options::isolated())
        .map(Some)
        .map_err(|error| VcsError::Failed {
            what: format!("the history in {}", directory.display()),
            message: error.to_string(),
        })
}

fn reading(repository: &gix::Repository) -> impl Fn(&dyn std::fmt::Display) -> VcsError + '_ {
    move |error| VcsError::Failed {
        what: format!("reading the history in {}", repository.path().display()),
        message: error.to_string(),
    }
}

/// Every version of each sample file the history keeps, each once — the
/// files' names from the project's folder, the model's and the
/// configuration's left out: what a selection over the history is asked of,
/// a sample that met it once being one it reaches.
pub fn sample_versions(config: &ProjectConfig) -> Result<Vec<(String, Vec<u8>)>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(Vec::new());
    };
    let read = reading(&repository);
    let graph = Graph::load(&repository)?;
    let mut seen = std::collections::HashSet::new();
    let mut trees = std::collections::HashSet::new();
    let mut versions = Vec::new();
    for node in graph.nodes.values() {
        if !trees.insert(node.tree) {
            continue;
        }
        for (name, blob) in tree_files(&repository, node.tree)? {
            if name.starts_with('@') || !name.ends_with(".md") || !seen.insert(blob) {
                continue;
            }
            let bytes = repository.find_blob(blob).map_err(|error| read(&error))?;
            versions.push((name, bytes.data.clone()));
        }
    }
    Ok(versions)
}

/// Every file of a tree, by its path, with its blob.
fn tree_files(
    repository: &gix::Repository,
    tree: gix::ObjectId,
) -> Result<std::collections::BTreeMap<String, gix::ObjectId>, VcsError> {
    let failed = reading(repository);
    let mut files = std::collections::BTreeMap::new();
    let mut pending = vec![(String::new(), tree)];
    while let Some((prefix, id)) = pending.pop() {
        let tree = repository.find_tree(id).map_err(|error| failed(&error))?;
        for entry in tree.iter() {
            let entry = entry.map_err(|error| failed(&error))?;
            let name = format!("{prefix}{}", entry.filename());
            if entry.mode().is_tree() {
                pending.push((format!("{name}/"), entry.oid().to_owned()));
            } else {
                files.insert(name, entry.oid().to_owned());
            }
        }
    }
    Ok(files)
}

/// `path` as the snapshot names it, where it lies under `root`.
fn inside(root: &Path, path: &Path) -> Option<String> {
    // Resolved, else as written: a sample in a folder linked into the
    // project resolves outside it, and is kept by the path the link gives it.
    let resolved = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let written = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    resolved(path)
        .strip_prefix(resolved(root))
        .ok()
        .map(slashed)
        .or_else(|| written(path).strip_prefix(written(root)).ok().map(slashed))
        .or_else(|| written(path).strip_prefix(resolved(root)).ok().map(slashed))
}

fn slashed(relative: &Path) -> String {
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------- the trace

/// The prefix of the tags that keep, each, what one write recorded of the
/// formulas that computed its values: named after the blob that holds it, so
/// that no two machines ever write one tag with two contents.
const TRACE: &str = "trace/";

/// One value a formula computed and a write kept: its name — a property, or
/// `table.column` for a cell — the digest of the record its file holds beside
/// it, and the digest of the formula that computed it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fact {
    pub value: String,
    pub record: String,
    pub formula: String,
}

/// What a project's history knows of the formulas that computed its values:
/// for each record a value was written with, the formulas known to have
/// computed it.
#[derive(Debug, Clone, Default)]
pub struct Trace {
    formulas: std::collections::HashMap<String, std::collections::BTreeSet<String>>,
    /// The formulas the latest write ran under, by the value each gives, and
    /// when it was kept.
    latest: Option<(i64, std::collections::BTreeMap<String, String>)>,
    /// The tags read, by name.
    read: std::collections::HashSet<String>,
}

impl Trace {
    /// Whether the history holds no fact at all: a project computed before
    /// its history kept the trace, or never computed.
    pub fn is_empty(&self) -> bool {
        self.formulas.is_empty()
    }

    /// Whether `formula` computed a value this trace knows: one a model
    /// restored gives again is no change of the model's.
    pub fn knows_formula(&self, formula: &str) -> bool {
        self.formulas.values().any(|known| known.contains(formula))
    }

    /// The formulas the latest write ran under, by the value each gives.
    pub fn latest_formulas(&self) -> Option<&std::collections::BTreeMap<String, String>> {
        self.latest.as_ref().map(|(_, formulas)| formulas)
    }

    /// Each value of `sample` held with a record the trace knows, with the
    /// formulas known to have computed it — for a column, those known for
    /// every cell of it the trace knows. A value computed where this trace
    /// was not kept is not among them, and is judged by its inputs alone.
    pub fn recorded(&self, sample: &Sample) -> std::collections::BTreeMap<String, Vec<String>> {
        let mut known: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for held in records_of_values(sample) {
            if held.edited {
                continue;
            }
            let Some(formulas) = self.formulas.get(&held.record) else {
                continue;
            };
            match known.get_mut(&held.value) {
                Some(common) => common.retain(|formula| formulas.contains(formula)),
                None => {
                    known.insert(held.value, formulas.clone());
                }
            }
        }
        known
            .into_iter()
            .map(|(value, formulas)| (value, formulas.into_iter().collect()))
            .collect()
    }

    /// The values of `sample` a formula computed whose record the trace does
    /// not know — computed on a computer whose history is not here, or before
    /// the history kept the trace — each once, a column once.
    pub fn unknown(&self, sample: &Sample) -> std::collections::BTreeSet<String> {
        records_of_values(sample)
            .into_iter()
            .filter(|held| !held.edited && !self.formulas.contains_key(&held.record))
            .map(|held| held.value)
            .collect()
    }

    fn add(
        &mut self,
        facts: &[Fact],
        seconds: i64,
        formulas: std::collections::BTreeMap<String, String>,
    ) {
        for fact in facts {
            self.formulas
                .entry(fact.record.clone())
                .or_default()
                .insert(fact.formula.clone());
        }
        if !formulas.is_empty()
            && self
                .latest
                .as_ref()
                .is_none_or(|(when, _)| *when <= seconds)
        {
            self.latest = Some((seconds, formulas));
        }
    }
}

/// A value's record, as a trace names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldRecord {
    /// A property's name, or `table.column` for a cell.
    pub value: String,
    /// The digest of the record the file holds beside it.
    pub record: String,
    /// The record is an override's: no formula computed what it holds.
    pub edited: bool,
}

/// The record each value a formula computed holds in `sample` — every
/// property and every cell with a `computed` record and a digest of its own,
/// whatever channel it names — keyed as a trace keys it: by the value's name,
/// its own digest and what it read, so that the same value computed from the
/// same inputs has the same key wherever it is.
pub fn records_of_values(sample: &Sample) -> Vec<HeldRecord> {
    let mut held = Vec::new();
    for name in sample.property_names() {
        if let Ok(handle) = sample.property(name) {
            let records = handle.records();
            if let Some(found) = record_digest(name.as_str(), &records) {
                held.push(found);
            }
        }
    }
    for name in sample.table_names() {
        let Ok(table) = sample.table(name) else {
            continue;
        };
        for row in table.rows() {
            for column in row.column_names() {
                if let Ok(cell) = row.cell(column)
                    && let Some(found) = record_digest(&format!("{name}.{column}"), cell.records())
                {
                    held.push(found);
                }
            }
        }
    }
    held
}

fn record_digest(value: &str, records: &crate::core::property::Records) -> Option<HeldRecord> {
    use crate::core::property::{InputRecord, Produced};
    use sha2::{Digest, Sha256};
    let computed = records.computed.as_ref()?;
    let own = records.fingerprint.as_ref()?;
    let mut text = format!("{value}\n{own}\n");
    text.push_str(match records.produced {
        Some(Produced::Value) => "v\n",
        Some(Produced::Uncertainty) => "u\n",
        None => "\n",
    });
    let mut inputs: Vec<String> = computed
        .iter()
        .map(|(input, record)| match record {
            InputRecord::Digest(digest) if digest.is_edited() => format!("{input}={digest}!"),
            InputRecord::Digest(digest) => format!("{input}={digest}"),
            InputRecord::Literal(literal) => format!("{input}:{literal:?}"),
        })
        .collect();
    inputs.sort();
    let mut alone: Vec<String> = records
        .channel_only
        .iter()
        .map(|(input, channel)| {
            let channel = if *channel == Produced::Value {
                "v"
            } else {
                "u"
            };
            format!("{input}@{channel}")
        })
        .collect();
    alone.sort();
    inputs.extend(alone);
    text.push_str(&inputs.join("\n"));
    let digest: String = Sha256::digest(text.as_bytes())
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Some(HeldRecord {
        value: value.to_string(),
        record: digest,
        edited: own.is_edited(),
    })
}

type Traces = std::collections::HashMap<PathBuf, std::sync::Arc<Trace>>;

/// The traces read in this process, by history: each tag is read once, and a
/// later call reads only the tags written since — by this process or another.
static TRACES: std::sync::Mutex<Option<Traces>> = std::sync::Mutex::new(None);

/// What the project's history knows of the formulas that computed its
/// values; an empty trace where no history is kept yet. A tag that cannot be
/// read is an error, never a part of the trace silently left out.
pub fn trace(config: &ProjectConfig) -> Result<std::sync::Arc<Trace>, VcsError> {
    let Some(repository) = existing(config)? else {
        return Ok(std::sync::Arc::default());
    };
    let directory = history_directory(config);
    let key = dunce::canonicalize(&directory).unwrap_or(directory);
    let failed = reading(&repository);
    let mut names = Vec::new();
    let references = repository.references().map_err(|error| failed(&error))?;
    let prefixed = references
        .prefixed(format!("refs/tags/{TRACE}").as_str())
        .map_err(|error| failed(&error))?;
    for reference in prefixed {
        let reference = reference.map_err(|error| failed(&error))?;
        names.push((
            reference.name().as_bstr().to_string(),
            reference.id().detach(),
        ));
    }
    let mut cache = TRACES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let traces = cache.get_or_insert_with(Default::default);
    let held = traces.entry(key).or_default();
    for (name, id) in names {
        if held.read.contains(&name) {
            continue;
        }
        let (facts, seconds, formulas) = read_trace(&repository, id)?;
        let trace = std::sync::Arc::make_mut(held);
        trace.add(&facts, seconds, formulas);
        trace.read.insert(name);
    }
    // What a command killed before it kept its trace said in its journal:
    // known until the next write keeps it.
    let left = journal_facts(config.root(), false);
    if !left.is_empty() {
        std::sync::Arc::make_mut(held).add(&left, 0, std::collections::BTreeMap::new());
    }
    Ok(held.clone())
}

/// The facts the journals of writes interrupted say — this host's and user's,
/// their process gone — and, where `remove`, those journals removed.
/// What the abandoned journals say of the formulas a write ran, kept in the
/// trace, and the journals removed then — only then: a trace that could not
/// be written left its facts lost and unsaid. One that cannot be read is
/// kept, and said.
fn keep_journal_facts(config: &ProjectConfig) {
    let (facts, read, unread) = journal_facts_read(config.root());
    for path in &unread {
        notice(format!(
            "an interrupted write's journal, {}, could not be read: kept, its formulas not \
             yet in the trace",
            path.display()
        ));
    }
    if !facts.is_empty() {
        let formulas: std::collections::BTreeMap<String, String> = facts
            .iter()
            .map(|fact| (fact.value.clone(), fact.formula.clone()))
            .collect();
        if let Err(error) = keep_trace(config, &formulas, &facts) {
            notice(format!(
                "the formulas an interrupted write ran were not kept in the trace, and its \
                 journal is kept for the next: {error}"
            ));
            return;
        }
    }
    for path in read {
        let _ = std::fs::remove_file(path);
    }
}

/// The facts of the abandoned journals, the journals read, and those that
/// could not be.
fn journal_facts_read(root: &Path) -> (Vec<Fact>, Vec<PathBuf>, Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root.join(".samplekit")) else {
        return (Vec::new(), Vec::new(), Vec::new());
    };
    let (mut facts, mut read, mut unread) = (Vec::new(), Vec::new(), Vec::new());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(JOURNAL) || !name.ends_with(".lock") {
            continue;
        }
        let at = entry.path();
        if !crate::format::lock::journal_abandoned(&at) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&at) else {
            unread.push(at);
            continue;
        };
        facts.extend(text.lines().filter_map(|line| {
            let mut parts = line.strip_prefix(FACT)?.split('\t');
            Some(Fact {
                value: parts.next()?.to_string(),
                record: parts.next()?.to_string(),
                formula: parts.next()?.to_string(),
            })
        }));
        read.push(at);
    }
    (facts, read, unread)
}

fn journal_facts(root: &Path, remove: bool) -> Vec<Fact> {
    let Ok(entries) = std::fs::read_dir(root.join(".samplekit")) else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(JOURNAL) || !name.ends_with(".lock") {
            continue;
        }
        let at = entry.path();
        if !crate::format::lock::journal_abandoned(&at) {
            continue;
        }
        let text = std::fs::read_to_string(&at).unwrap_or_default();
        facts.extend(text.lines().filter_map(|line| {
            let mut parts = line.strip_prefix(FACT)?.split('\t');
            Some(Fact {
                value: parts.next()?.to_string(),
                record: parts.next()?.to_string(),
                formula: parts.next()?.to_string(),
            })
        }));
        if remove {
            let _ = std::fs::remove_file(&at);
        }
    }
    facts
}

type Kept = (Vec<Fact>, i64, std::collections::BTreeMap<String, String>);

fn read_trace(repository: &gix::Repository, id: gix::ObjectId) -> Result<Kept, VcsError> {
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!(
            "reading the trace of the formulas in {}",
            repository.path().display()
        ),
        message: error.to_string(),
    };
    let object = repository.find_object(id).map_err(|error| failed(&error))?;
    // A reference to the blob, its time inside it.
    if object.kind != gix::object::Kind::Blob {
        return Err(failed(&"the trace is no blob"));
    }
    let data = object.data.clone();
    let json: serde_json::Value = serde_json::from_slice(&data).map_err(|error| failed(&error))?;
    let seconds = json
        .get("seconds")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    let formulas = json
        .get("formulas")
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(name, digest)| Some((name.clone(), digest.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let facts = json
        .get("facts")
        .and_then(serde_json::Value::as_array)
        .map(|facts| {
            facts
                .iter()
                .filter_map(|fact| {
                    let field = |at: usize| fact.get(at)?.as_str().map(str::to_string);
                    Some(Fact {
                        value: field(0)?,
                        record: field(1)?,
                        formula: field(2)?,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((facts, seconds, formulas))
}

/// Keeps `facts` in the project's history, beside the formulas the write ran
/// under, by the value each gives: one blob, and a tag named after it. A fact
/// already kept is kept again harmlessly; nothing kept is ever rewritten.
pub fn keep_trace(
    config: &ProjectConfig,
    formulas: &std::collections::BTreeMap<String, String>,
    facts: &[Fact],
) -> Result<(), VcsError> {
    if !keeps_history(config) || facts.is_empty() {
        return Ok(());
    }
    let repository = history(config)?;
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!(
            "keeping the trace of the formulas in {}",
            repository.path().display()
        ),
        message: error.to_string(),
    };
    let mut facts = facts.to_vec();
    facts.sort();
    facts.dedup();
    // When, in the blob itself: a tag object carried its machine and its
    // time, so that two machines keeping the same facts wrote one name with
    // two contents, and git met them as a conflict in files nobody edits.
    let seconds = signature(&machine()).time.seconds;
    let json = serde_json::json!({
        "formulas": formulas,
        "facts": facts
            .iter()
            .map(|fact| [&fact.value, &fact.record, &fact.formula])
            .collect::<Vec<_>>(),
        "seconds": seconds,
    });
    let text = serde_json::to_string(&json).map_err(|error| failed(&error))? + "\n";
    let blob = repository
        .write_blob(text.as_bytes())
        .map_err(|error| failed(&error))?
        .detach();
    // A reference straight to the blob, named by it: the same on every
    // machine that keeps the same content.
    repository
        .reference(
            format!("refs/tags/{TRACE}{blob}"),
            blob,
            gix::refs::transaction::PreviousValue::Any,
            "trace",
        )
        .map_err(|error| failed(&error))?;
    Ok(())
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum VcsError {
    /// The history could not be read or written.
    Failed { what: String, message: String },
    /// A file could not be read or written.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for VcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VcsError::Failed { what, message } => write!(f, "{what}: {message}"),
            VcsError::Io { path, source } => write!(f, "{}: {source}", path.display()),
        }
    }
}

impl std::error::Error for VcsError {}

// ------------------------------------------------------------- taking back

/// A file with one change taken back: what it holds now, with each field that
/// change changed given back as it was before it. `Ok(None)` where the file
/// is to be gone; `Err` names each field changed since in what the change
/// changed, where taking it back would lose that.
///
/// A sample is taken back field by field — its name, its tags, each attribute,
/// property and table, its note — so that what another machine or an editor
/// changed since in other fields stays; any other file is taken back whole.
pub fn taken_back(
    name: &str,
    before: Option<&[u8]>,
    after: Option<&[u8]>,
    now: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, Vec<String>> {
    if now == after {
        return Ok(before.map(<[u8]>::to_vec));
    }
    if now == before {
        return Ok(now.map(<[u8]>::to_vec));
    }
    let whole = || Err(vec![format!("{name}, changed since")]);
    let (Some(before), Some(after), Some(now)) = (before, after, now) else {
        return whole();
    };
    let is_sample = name.ends_with(".md") && name != project_config::FILENAME;
    let read = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| crate::format::document::parse(text).ok())
    };
    let (true, Some(before), Some(after), Some(mut now)) =
        (is_sample, read(before), read(after), read(now))
    else {
        return whole();
    };
    let mut conflicts = Vec::new();
    // Each field: given back where it is still as the change left it; kept
    // where it is as before; a conflict where it moved on.
    fn one<T: PartialEq + Clone>(
        field: String,
        before: Option<&T>,
        after: Option<&T>,
        now: Option<&T>,
        conflicts: &mut Vec<String>,
    ) -> Option<Option<T>> {
        if before == after || now == before {
            return None;
        }
        if now == after {
            return Some(before.cloned());
        }
        conflicts.push(field);
        None
    }
    fn map<K: Clone + std::hash::Hash + Eq + std::fmt::Display, T: PartialEq + Clone>(
        prefix: &str,
        before: &indexmap::IndexMap<K, T>,
        after: &indexmap::IndexMap<K, T>,
        now: &mut indexmap::IndexMap<K, T>,
        conflicts: &mut Vec<String>,
    ) {
        let mut keys: Vec<K> = before.keys().cloned().collect();
        keys.extend(
            after
                .keys()
                .filter(|key| !before.contains_key(*key))
                .cloned(),
        );
        for key in keys {
            let given = one(
                format!("{prefix}{key}"),
                before.get(&key),
                after.get(&key),
                now.get(&key),
                conflicts,
            );
            match given {
                Some(Some(value)) => {
                    now.insert(key, value);
                }
                Some(None) => {
                    now.shift_remove(&key);
                }
                None => {}
            }
        }
    }
    let (b, a) = (&before.schema, &after.schema);
    if let Some(given) = one(
        "name".to_string(),
        Some(&b.name),
        Some(&a.name),
        Some(&now.schema.name),
        &mut conflicts,
    ) {
        now.schema.name = given.flatten();
    }
    if let Some(Some(tags)) = one(
        "tags".to_string(),
        Some(&b.tags),
        Some(&a.tags),
        Some(&now.schema.tags),
        &mut conflicts,
    ) {
        now.schema.tags = tags;
    }
    map(
        "",
        &b.attributes,
        &a.attributes,
        &mut now.schema.attributes,
        &mut conflicts,
    );
    map(
        "",
        &b.properties,
        &a.properties,
        &mut now.schema.properties,
        &mut conflicts,
    );
    map(
        "",
        &b.tables,
        &a.tables,
        &mut now.schema.tables,
        &mut conflicts,
    );
    if let Some(Some(note)) = one(
        "the note".to_string(),
        Some(&before.note),
        Some(&after.note),
        Some(&now.note),
        &mut conflicts,
    ) {
        now.note = note;
    }
    if !conflicts.is_empty() {
        return Err(conflicts
            .into_iter()
            .map(|field| format!("{name}: {field}, changed since"))
            .collect());
    }
    Ok(Some(crate::format::document::write(&now).into_bytes()))
}

/// The last change SampleKit wrote on this machine among the files `holds`
/// keeps — not a change made outside SampleKit, not the first, not a join's
/// own snapshot: what `restore` without `--at` takes back. Its place in
/// `entries`.
pub fn last_own_change(entries: &[Entry], holds: impl Fn(&str) -> bool) -> Option<usize> {
    let me = machine();
    entries.iter().position(|entry| {
        entry.machine_id == me
            && entry.message != OUTSIDE
            && entry.message != FIRST
            && entry.message != MADE_A_PROJECT
            && entry.message != JOINED
            && !entry.parents.is_empty()
            && entry.changed.iter().any(|name| holds(name))
    })
}

/// The newest change made outside SampleKit on this machine, kept since, to
/// what `holds` names: what `restore` of a file named takes back where no
/// change SampleKit wrote touched it — a configuration broken by hand, its
/// edit kept by the write after it. Its place in `entries`.
pub fn last_outside_change(entries: &[Entry], holds: impl Fn(&str) -> bool) -> Option<usize> {
    let me = machine();
    entries.iter().position(|entry| {
        entry.machine_id == me
            && entry.message == OUTSIDE
            && !entry.parents.is_empty()
            && entry.changed.iter().any(|name| holds(name))
    })
}
