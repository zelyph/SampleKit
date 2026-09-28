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

/// The message of a snapshot taken before a write, of what changed since the
/// last: an editor's change is never credited to the command that follows.
pub const OUTSIDE: &str = "changed outside SampleKit";

/// The message of a history's first snapshot, taken before a write: the
/// project as SampleKit first found it.
pub const FIRST: &str = "the project as SampleKit first kept it";

/// The message of a join taken for a file written from it, where no snapshot
/// holds the files as they are.
pub const JOINED: &str = "joined with the snapshots of another machine";

/// This machine's name in the history, its branch's: `SAMPLEKIT_MACHINE` where
/// the environment sets it, else the host's name.
pub fn machine() -> String {
    let raw = std::env::var("SAMPLEKIT_MACHINE")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned());
    machine_name(&raw)
}

/// A machine's name made safe as a branch's: up to its first dot,
/// lowercased, each character but a letter, a digit, `-` and `_` made `-`;
/// nothing left is `machine`.
pub fn machine_name(raw: &str) -> String {
    let first = raw.trim().split('.').next().unwrap_or_default();
    let name: String = first
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if name.is_empty() {
        "machine".to_string()
    } else {
        name
    }
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
        name: "SampleKit".into(),
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
    config.root().join(".samplekit").join("history")
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
    // Every folder below, whatever `recursive` says: a command reaches a
    // subfolder's samples by naming it, and they are the project's too.
    let rules = project_config::CollectionRules {
        recursive: true,
        ..config.collection().clone()
    };
    let collection = discovery::discover_with(root, &rules).map_err(|error| VcsError::Failed {
        what: "reading the project's samples".to_string(),
        message: error.to_string(),
    })?;
    for path in collection.paths {
        // A project inside this one keeps its own samples.
        let same = |found: &Path| dunce::canonicalize(found).ok() == dunce::canonicalize(&rc).ok();
        if project_config::find(&path).is_some_and(|found| !same(&found)) {
            continue;
        }
        if let Some(name) = inside(root, &path) {
            kept.push((name, path));
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
    Ok(Some(Before {
        config: config.clone(),
        files: contents(&kept_files(config)?)?,
        head: head_of(config)?,
    }))
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
    let now = contents(&kept_files(&before.config)?)?;
    if now == before.files {
        return Ok(Snapshot::Unchanged);
    }
    let moved = head_of(&before.config)? != before.head;
    let repository = history(&before.config)?;
    // Whether another machine's snapshots are joined is decided by what the
    // files held before the write: the write may change a file they changed.
    if !moved {
        commit_with(&repository, &before.files, OUTSIDE, false, &before.files)?;
    }
    commit_with(&repository, &now, message, false, &before.files)
}

/// A write under way: what the projects it reaches held before it.
#[derive(Debug, Clone)]
pub struct Writing {
    before: Vec<Before>,
    places: Vec<PathBuf>,
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
        if let Ok(config) = project_config::load(&found) {
            projects.push(config);
        }
    }
    projects
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
    let writing = Writing {
        before,
        places: places.to_vec(),
    };
    (writing, failures)
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
    for before in writing.before {
        let config = before.config.clone();
        kept(&config, after(before, message));
    }
    for config in projects_of(&writing.places) {
        if known.contains(&canonical(config.root())) {
            continue;
        }
        let taken = snapshot(&config, message);
        kept(&config, taken);
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

fn contents(kept: &[(String, PathBuf)]) -> Result<Vec<(String, Vec<u8>)>, VcsError> {
    kept.iter()
        .map(|(name, path)| {
            std::fs::read(path)
                .map(|bytes| (name.clone(), bytes))
                .map_err(|source| VcsError::Io {
                    path: path.clone(),
                    source,
                })
        })
        .collect()
}

/// The project's history, made where it is not yet. Opened apart from the
/// user's Git configuration: their identity, hooks and settings do not apply.
fn history(config: &ProjectConfig) -> Result<gix::Repository, VcsError> {
    let directory = history_directory(config);
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the history in {}", directory.display()),
        message: error.to_string(),
    };
    if !directory.join("HEAD").is_file() {
        std::fs::create_dir_all(&directory).map_err(|source| VcsError::Io {
            path: directory.clone(),
            source,
        })?;
        gix::init_bare(&directory).map_err(|error| failed(&error))?;
        // The researcher's `git status` lists none of it, with nothing added
        // to their own `.gitignore`.
        let ignore = directory.join(".gitignore");
        std::fs::write(&ignore, "*\n").map_err(|source| VcsError::Io {
            path: ignore,
            source,
        })?;
        let repository = gix::open_opts(&directory, gix::open::Options::isolated())
            .map_err(|error| failed(&error))?;
        // `HEAD` names the branch of the machine that made the history, so
        // that a `git log` in it reads a branch; nothing writes it after.
        point_head(&repository, &machine())?;
    }
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
    adopt(&repository, &machine())?;
    Ok(repository)
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

/// A history kept before branches per machine — one branch, whose snapshots name no machine —
/// adopted by the first machine to write in it: its branch made on that
/// branch's last snapshot, `HEAD` naming it, the old name removed. No snapshot
/// is rewritten.
fn adopt(repository: &gix::Repository, machine: &str) -> Result<(), VcsError> {
    let failed = |error: &dyn std::fmt::Display| VcsError::Failed {
        what: format!("the history in {}", repository.path().display()),
        message: error.to_string(),
    };
    if branch_head(repository, machine)?.is_some() {
        return Ok(());
    }
    let Some(named) = repository.head_name().map_err(|error| failed(&error))? else {
        return Ok(());
    };
    let named = named.as_bstr().to_string();
    let Some(old) = named.strip_prefix(BRANCHES) else {
        return Ok(());
    };
    if old == machine {
        return Ok(());
    }
    let Some(reference) = repository
        .try_find_reference(named.as_str())
        .map_err(|error| failed(&error))?
    else {
        return Ok(());
    };
    let Some(last) = reference.try_id().map(|id| id.detach()) else {
        return Ok(());
    };
    // Another machine's branch, which `HEAD` names since it made the
    // history, is joined, never taken over.
    let commit = repository
        .find_commit(last)
        .map_err(|error| failed(&error))?;
    let author = commit.author().map_err(|error| failed(&error))?;
    if machine_of_email(&author.email.to_string()).is_some() {
        return Ok(());
    }
    repository
        .reference(
            branch_of(machine),
            last,
            gix::refs::transaction::PreviousValue::MustNotExist,
            "adopted",
        )
        .map_err(|error| failed(&error))?;
    point_head(repository, machine)?;
    reference.delete().map_err(|error| failed(&error))?;
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

    /// The last snapshot both `a` and `b` follow, where they share one.
    fn merge_base(
        &self,
        a: gix::ObjectId,
        b: gix::ObjectId,
        generations: &std::collections::HashMap<gix::ObjectId, usize>,
    ) -> Option<gix::ObjectId> {
        let of_a = self.ancestors(a);
        self.ancestors(b)
            .into_iter()
            .filter(|id| of_a.contains(id))
            .max_by_key(|id| (generations.get(id).copied().unwrap_or(0), *id))
    }

    /// The machine each snapshot was taken on: the one its author names; a
    /// snapshot kept before branches per machine is the machine's whose branch reaches it
    /// through its own snapshots — its adopter's; on a branch not adopted yet,
    /// whose last snapshot names no machine, `me`'s, which adopts it at its
    /// next write.
    fn machines(&self, me: &str) -> std::collections::HashMap<gix::ObjectId, String> {
        let mut machine: std::collections::HashMap<gix::ObjectId, String> = self
            .nodes
            .iter()
            .filter_map(|(id, node)| node.machine.clone().map(|name| (*id, name)))
            .collect();
        for (name, head) in &self.branches {
            let unadopted = self
                .nodes
                .get(head)
                .is_some_and(|node| node.machine.is_none());
            let name = if unadopted { me } else { name.as_str() };
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
    let generations = graph.generations();
    let mut joined = Vec::new();
    for other in others {
        let base = own.and_then(|own| graph.merge_base(own, other, &generations));
        let base_files = trees.at(repository, graph, base)?;
        let other_files = trees.at(repository, graph, Some(other))?;
        let changed: Vec<&String> = other_files
            .keys()
            .chain(base_files.keys())
            .filter(|name| other_files.get(*name) != base_files.get(*name))
            .collect();
        let holds = changed.iter().all(|name| {
            let now = files.get(*name);
            let mine = own_files.get(*name);
            now == other_files.get(*name) || (mine != base_files.get(*name) && now != mine)
        });
        if !holds {
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
    generations: &std::collections::HashMap<gix::ObjectId, usize>,
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
        let base = graph.merge_base(first, *other, generations);
        let base_files = trees.at(repository, graph, base)?;
        let other_files = trees.at(repository, graph, Some(*other))?;
        let changed: Vec<String> = other_files
            .keys()
            .chain(base_files.keys())
            .filter(|name| other_files.get(*name) != base_files.get(*name))
            .cloned()
            .collect();
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

/// The SHA-256 of a file, in hex.
pub fn file_hash(path: &Path) -> Result<String, VcsError> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path).map_err(|source| VcsError::Io {
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
/// — `<hash>@<machine>`, then `<hash>@<machine>.2` and on, or, as a record
/// written before records named their machine is named, `<hash>` and `<hash>.2` — newest first.
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
        if tag != hash
            && !tag
                .strip_prefix(hash)
                .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('@'))
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
        // `<hash>`, `<hash>.<n>`, `<hash>@<machine>` or `<hash>@<machine>.<n>`:
        // the hash is the name before its point or its machine.
        let tag = name.rsplit('/').next().unwrap_or_default();
        let hash = tag.split(['.', '@']).next().unwrap_or_default().to_string();
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
    /// The machine that took it.
    pub machine: String,
    /// The snapshots it was taken over: its machine's last first, then those
    /// of the machines it joined.
    pub parents: Vec<String>,
    /// The machines it joined, where it was taken over another machine's
    /// snapshot.
    pub joined: Vec<String>,
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
    let machines = graph.machines(&machine());
    let mut trees = Trees::default();
    let mut order: Vec<gix::ObjectId> = graph.nodes.keys().copied().collect();
    order.sort_by_key(|id| {
        let node = &graph.nodes[id];
        std::cmp::Reverse((node.seconds, generations.get(id).copied().unwrap_or(0), *id))
    });
    let mut found = Vec::new();
    for id in order {
        let node = &graph.nodes[&id];
        let files = trees.of(&repository, node.tree)?;
        let before = before_of(&repository, &graph, &mut trees, &generations, id)?;
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
        let machine = machines.get(&id).cloned().unwrap_or_default();
        let mut joined: Vec<String> = Vec::new();
        for parent in &node.parents {
            if let Some(other) = machines.get(parent)
                && *other != machine
                && !joined.contains(other)
            {
                joined.push(other.clone());
            }
        }
        found.push(Entry {
            id: id.to_string(),
            seconds: node.seconds,
            offset: node.offset,
            message: node.message.clone(),
            changed,
            machine,
            parents: node.parents.iter().map(ToString::to_string).collect(),
            joined,
        });
    }
    Ok(found)
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
    let generations = graph.generations();
    let mut trees = Trees::default();
    blobs(
        &repository,
        before_of(&repository, &graph, &mut trees, &generations, id)?,
    )
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
/// made outside SampleKit, not the first, not a join, not an undo — whose files
/// do not already hold what they held before it. Whether they still hold what
/// it wrote is the caller's to check, as for any undo.
pub fn last_undoable(config: &ProjectConfig) -> Result<Option<Undoable>, VcsError> {
    let entries = entries(config)?;
    // This machine's changes alone: another's is taken back there.
    let me = machine();
    for entry in &entries {
        if entry.machine != me
            || entry.message == OUTSIDE
            || entry.message == FIRST
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
            .all(|file| std::fs::read(&file.path).ok() == file.before)
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
/// the model runs in is today's, linked beside it: a snapshot keeps no
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
    if let Some(text) = rc {
        written(&root.join(project_config::FILENAME), text.as_bytes())?;
    }
    // The nearest environment above the project, as the model finds one.
    #[cfg(unix)]
    {
        let real =
            dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
        if let Some(environment) = real
            .ancestors()
            .map(|folder| folder.join(".venv"))
            .find(|candidate| candidate.is_dir())
        {
            let _ = std::os::unix::fs::symlink(environment, root.join(".venv"));
        }
    }
    Ok(root)
}

/// The project's history where one is kept, opened; `None` where none is.
fn existing(config: &ProjectConfig) -> Result<Option<gix::Repository>, VcsError> {
    let directory = history_directory(config);
    if !directory.join("HEAD").is_file() {
        return Ok(None);
    }
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
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let path = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.strip_prefix(&root).ok().map(slashed)
}

fn slashed(relative: &Path) -> String {
    relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
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
