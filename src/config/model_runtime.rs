//! Running a project's model from the command line: what runs it, and the
//! Python worker that computes and saves. Nothing is asked before it runs.
//!
//! **Nothing here links Python**. The interpreter is a child process, and the
//! worker is `samplekit._worker`, the Python package's own module, so a model
//! computes from the command line exactly as it does in a script.
//!

use std::collections::{BTreeMap, VecDeque};
use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{Value as Json, json};
use sha2::{Digest, Sha256};

use crate::config::project_config::{FILENAME, ProjectConfig};

mod description;
pub use description::{
    DescribedCell, DescribedColumn, DescribedFigure, DescribedModel, DescribedProperty,
    DescribedTable, Draws, FORMAT, ModelDescription, Origin, current_description, describe,
    described_path, description_path, description_path_in, forget_description, value_to_json,
    write_description,
};

/// The version a worker must announce: this crate's.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How many of the worker's last stderr lines a failure carries.
const TAIL: usize = 40;

#[cfg(windows)]
const PYTHON_IN_VENV: [&str; 2] = ["Scripts", "python.exe"];
#[cfg(not(windows))]
const PYTHON_IN_VENV: [&str; 2] = ["bin", "python"];

// ------------------------------------------------------------------- types

/// The model a configuration declares, with its paths resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    path: PathBuf,
    class: Option<String>,
    configuration: PathBuf,
}

impl Template {
    /// The model at `path`, as the configuration at `configuration` declares
    /// it: what the worker is told of it, rebuilt on its side.
    pub fn at(path: &Path, class: Option<&str>, configuration: &Path) -> Template {
        Template {
            path: dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()),
            class: class.map(str::to_string),
            configuration: configuration.to_path_buf(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn class(&self) -> Option<&str> {
        self.class.as_deref()
    }

    /// The `.samplekitrc` that declared it.
    pub fn configuration(&self) -> &Path {
        &self.configuration
    }

    /// Where a model imports its neighbours from, as a script run there would.
    pub fn root(&self) -> &Path {
        self.configuration.parent().unwrap_or(Path::new("."))
    }

    /// The templates directory: the one holding the declared file, whose tree
    /// the model's digest covers.
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }
}

/// Three states, because three different things can be true and each sends the
/// user somewhere else. A fourth, *not permitted*, went with the consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    NoTemplate,
    Unavailable { reason: String },
    Ready { template: Template, python: PathBuf },
}

/// A full SHA-256 of a templates tree: the version of the whole model that
/// computed a sample, recorded beside its formulas'. Not a `Fingerprint`, and
/// never truncated: it names a version of the code, and costs nothing to keep
/// whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateDigest(String);

impl TemplateDigest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TemplateDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One computation asked of the worker.
#[derive(Debug, Clone)]
pub struct Request {
    pub sample: PathBuf,
    pub template: Template,
    /// `--properties`; empty for every value a formula gives.
    pub names: Vec<String>,
    /// Current values too.
    pub rerun: bool,
    /// Overrides too.
    pub force: bool,
    /// Inputs held as text where numbers belong, each with its text: what reads
    /// them fails by name, and no formula runs.
    pub refused: Vec<(String, String)>,
    /// Each formula's digest as the user's state recorded it, by the value it
    /// gives: sent when the model changed since that record, so that the worker
    /// plans what a changed formula gives. Empty otherwise.
    pub recorded: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub value: String,
    pub reason: Reason,
    /// The reason as the worker says it: `waits for fg` names what.
    pub said: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Stale,
    NeverComputed,
    /// Run again with `--rerun`.
    Current,
    /// An override given back to its formula with `--force`.
    Edited,
    /// Its formula raised when last run: the file records the failure.
    Failed,
    /// Declared to read what nobody entered: it will wait.
    Waiting,
    /// Its formula, or that of a value it reads, is not the one that computed
    /// it: `said` tells which.
    FormulaChanged,
}

impl Reason {
    fn from_worker(written: &str) -> Reason {
        match written {
            "never computed" => Reason::NeverComputed,
            "current" => Reason::Current,
            "edited" => Reason::Edited,
            "failed" => Reason::Failed,
            waits if waits.starts_with("waits for") => Reason::Waiting,
            changed if changed.ends_with("formula changed") => Reason::FormulaChanged,
            _ => Reason::Stale,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Reason::Stale => "outdated",
            Reason::NeverComputed => "never computed",
            Reason::Current => "current",
            Reason::Edited => "edited",
            Reason::Failed => "failed",
            Reason::Waiting => "waits",
            Reason::FormulaChanged => "formula changed",
        }
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One value's progress, as the worker reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// What is left to compute in the sample, planned again as values stale others.
    Planned {
        count: usize,
    },
    Started {
        value: String,
    },
    /// With the value as it stood before and as it stands after, rendered.
    Finished {
        value: String,
        seconds: f64,
        before: String,
        after: String,
    },
    Failed {
        value: String,
        traceback: String,
    },
    /// Not run: what it is declared to read is absent, and a formula over
    /// nothing gives nothing — no failure, and nothing written.
    Waiting {
        value: String,
        inputs: Vec<String>,
        /// Those of `inputs` that failed in this run, rather than nobody gave.
        failed: Vec<String>,
    },
    /// A line the model wrote, beside the value in progress.
    Output {
        value: Option<String>,
        text: String,
    },
    /// A record the model logged, beside the value in progress.
    Log {
        level: String,
        value: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub computed: usize,
    pub failed: usize,
    /// Values not run for want of an input.
    pub waiting: usize,
    pub saved: bool,
    /// Values a run narrowed by names left stale.
    pub pending: usize,
    /// What the worker said of the model's formulas.
    pub formulas: Formulas,
}

/// What the worker said of a model's formulas for one sample.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Formulas {
    /// Each formula's digest as the worker imported the model, by the value it
    /// gives — a property, or `table.column`. Empty when the worker never read
    /// the model.
    pub digests: BTreeMap<String, String>,
    /// The values the run computed whole, whose formula is now the one
    /// recorded: a column only when every cell of it ran.
    pub computed: Vec<String>,
}

// ------------------------------------------------------------- the model

pub fn template_of(config: &ProjectConfig) -> Option<Template> {
    let declared = config.model()?;
    let path = config.resolve(&declared.path.to_string_lossy());
    Some(Template {
        path: dunce::canonicalize(&path).unwrap_or(path),
        class: declared.class.clone(),
        configuration: config.root().join(FILENAME),
    })
}

/// Checks, in order, that a model is declared, that its file exists and that an
/// interpreter exists. Whether it may run is not asked: a user runs a model
/// whose code they know.
pub fn availability(
    config: Option<&ProjectConfig>,
    from: &Path,
) -> Result<Availability, ModelError> {
    let Some(template) = config.and_then(template_of) else {
        return Ok(Availability::NoTemplate);
    };
    let config = config.expect("a template comes from a configuration");
    if !template.path.is_file() {
        return Err(ModelError::TemplateNotFound {
            path: template.path,
        });
    }
    let python = match interpreter_for(config, from) {
        Ok(python) => python,
        Err(ModelError::NoInterpreter { reason }) => {
            return Ok(Availability::Unavailable { reason });
        }
        Err(other) => return Err(other),
    };
    Ok(Availability::Ready { template, python })
}

// ------------------------------------------------------- the interpreter

/// The nearest `.venv/` walking upward, by the same walk that finds a
/// configuration. `None` is an ordinary answer, not a failure.
pub fn find_interpreter(from: &Path) -> Option<PathBuf> {
    let start = dunce::canonicalize(from).unwrap_or_else(|_| from.to_path_buf());
    let mut at = if start.is_dir() {
        start
    } else {
        start.parent()?.to_path_buf()
    };
    loop {
        let candidate = at
            .join(".venv")
            .join(PYTHON_IN_VENV[0])
            .join(PYTHON_IN_VENV[1]);
        if candidate.is_file() {
            return Some(candidate);
        }
        if !at.pop() {
            return None;
        }
    }
}

/// `[model] python` when the configuration names one, the convention otherwise.
pub fn interpreter_for(config: &ProjectConfig, from: &Path) -> Result<PathBuf, ModelError> {
    if let Some(declared) = config.model().and_then(|model| model.python.as_ref()) {
        let python = config.resolve(&declared.to_string_lossy());
        if python.is_file() {
            return Ok(python);
        }
        return Err(ModelError::NoInterpreter {
            reason: format!(
                "the interpreter [model] python names does not exist\n\n  {}\n\nfix it in {}",
                python.display(),
                config.root().join(FILENAME).display()
            ),
        });
    }
    find_interpreter(from).ok_or_else(|| {
        let start = std::path::absolute(from).unwrap_or_else(|_| from.to_path_buf());
        let top = start.ancestors().last().unwrap_or(&start).to_path_buf();
        ModelError::NoInterpreter {
            reason: format!(
                "no environment found\n\n  looked for .venv/ from {} upward to {}\n\n\
                 create one beside the project and install samplekit {VERSION} into it",
                start.display(),
                top.display()
            ),
        }
    })
}

// ------------------------------------------------------ the model's files

/// Every `*.py` under the templates directory, in sorted order. A cache, a
/// hidden directory and a virtual environment hold no code anyone wrote there.
pub fn template_files(template: &Template) -> Result<Vec<PathBuf>, ModelError> {
    let mut found = Vec::new();
    collect(template.directory(), &mut found).map_err(|error| ModelError::Unreadable {
        path: template.directory().to_path_buf(),
        reason: error.to_string(),
    })?;
    if !found.contains(&template.path) {
        found.push(template.path.clone());
    }
    found.sort();
    Ok(found)
}

fn collect(directory: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // `file_type` does not follow a link, so a linked directory is never
        // walked into, and a link back up cannot loop.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            if name.starts_with('.') || name == "__pycache__" || path.join("pyvenv.cfg").is_file() {
                continue;
            }
            collect(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "py") {
            found.push(path);
        }
    }
    Ok(())
}

/// Each file's path relative to the templates directory and its bytes, in
/// sorted order: moving the tree keeps the digest, editing any file changes it.
pub fn digest_of(template: &Template) -> Result<TemplateDigest, ModelError> {
    let directory = template.directory();
    let mut hasher = Sha256::new();
    for file in template_files(template)? {
        let relative = file.strip_prefix(directory).unwrap_or(&file);
        let spelled: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        let bytes = fs::read(&file).map_err(|error| ModelError::Unreadable {
            path: file.clone(),
            reason: error.to_string(),
        })?;
        hasher.update(spelled.join("/").as_bytes());
        hasher.update([0]);
        hasher.update(&bytes);
        hasher.update([0]);
    }
    Ok(TemplateDigest(
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    ))
}

/// Where the machine keeps what it records beside the projects — which version
/// of a model computed what, the run logs, the workbench's state:
/// `$SAMPLEKIT_STATE_DIR`, or the system's own place for a program's state, by
/// `etcetera`: `$XDG_STATE_HOME/samplekit`, `~/.local/state/samplekit` without
/// it; the data folder where a system has no state folder —
/// `~/Library/Application Support/samplekit` — and on Windows
/// `%LOCALAPPDATA%\samplekit`, where it always was. Never under a project,
/// which would carry it to whoever the project is shared with.
pub fn state_directory() -> Option<PathBuf> {
    use etcetera::BaseStrategy;
    if let Some(directory) =
        std::env::var_os("SAMPLEKIT_STATE_DIR").filter(|value| !value.is_empty())
    {
        return Some(PathBuf::from(directory));
    }
    // The native strategy: the base one is XDG on macOS, which put the state
    // in `~/.local/state` there.
    let strategy = etcetera::base_strategy::choose_native_strategy().ok()?;
    // Windows has no state folder, and its data folder is the roaming one,
    // copied from machine to machine: what this machine records belongs to
    // its `%LOCALAPPDATA%`, which `etcetera` calls the cache.
    let base = if cfg!(windows) {
        strategy.cache_dir()
    } else {
        strategy.state_dir().unwrap_or_else(|| strategy.data_dir())
    };
    Some(base.join("samplekit"))
}

/// Which version of a model, and of each of its formulas, computed each sample's
/// values, kept in the machine's state directory and never in a file: what says
/// *formula changed*, and *the model changed since* where the formulas cannot be told
/// apart.
///
/// `computed.json` holds, by sample, the whole model's digest and each
/// formula's, `null` where it is not known. `computed`, a line per sample with
/// the whole model's digest alone, is what came before, and is still read for
/// a sample the first file does not hold.
pub struct ComputedStore {
    file: PathBuf,
    legacy: PathBuf,
}

/// What the store holds for one sample.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    model: String,
    /// By the value each formula gives, its digest, or `None` where it is not
    /// known. `None` as a whole: a record that never told formulas apart.
    formulas: Option<BTreeMap<String, Option<String>>>,
}

impl ComputedStore {
    /// In the machine's state directory.
    pub fn user() -> Option<ComputedStore> {
        state_directory().map(|directory| ComputedStore::at(&directory))
    }

    pub fn at(directory: &Path) -> ComputedStore {
        ComputedStore {
            file: directory.join("computed.json"),
            legacy: directory.join("computed"),
        }
    }

    /// Everything `computed.json` holds. A file that cannot be read holds
    /// nothing: the store is best effort, and a lost record only makes the
    /// next run the baseline.
    fn table(&self) -> serde_json::Map<String, Json> {
        fs::read_to_string(&self.file)
            .ok()
            .and_then(|text| serde_json::from_str::<Json>(&text).ok())
            .and_then(|json| match json {
                Json::Object(map) => Some(map),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn key(sample: &Path) -> String {
        dunce::canonicalize(sample)
            .unwrap_or_else(|_| sample.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    fn held(&self, key: &str) -> Option<Held> {
        if let Some(entry) = self.table().get(key) {
            let model = entry.get("model")?.as_str()?.to_string();
            let formulas = entry.get("formulas").and_then(Json::as_object).map(|map| {
                map.iter()
                    .map(|(name, digest)| (name.clone(), digest.as_str().map(str::to_string)))
                    .collect()
            });
            return Some(Held { model, formulas });
        }
        fs::read_to_string(&self.legacy)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_once(' '))
            .find(|(_, path)| *path == key)
            .map(|(digest, _)| Held {
                model: digest.to_string(),
                formulas: None,
            })
    }

    /// The digest of the whole model that last computed a sample's values.
    pub fn digest_for(&self, sample: &Path) -> Option<String> {
        self.held(&Self::key(sample)).map(|held| held.model)
    }

    /// Each formula's digest as recorded for the sample, by the value it gives:
    /// what the worker compares with the formulas as they are now. A formula
    /// not known is left out, having nothing to compare.
    pub fn recorded_formulas(&self, sample: &Path) -> BTreeMap<String, String> {
        self.held(&Self::key(sample))
            .and_then(|held| held.formulas)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(name, digest)| Some((name, digest?)))
            .collect()
    }

    /// Whether the model changed since the sample was computed and some
    /// formula has no digest to be compared by: a record from before formulas
    /// were told apart, under another model, or a formula recorded *not known*.
    pub fn formulas_unknown(&self, sample: &Path, model: &TemplateDigest) -> bool {
        match self.held(&Self::key(sample)) {
            None => false,
            Some(Held {
                model: held,
                formulas: None,
            }) => held != model.as_str(),
            Some(Held {
                formulas: Some(formulas),
                ..
            }) => formulas.values().any(Option::is_none),
        }
    }

    /// Records the model a run began with, and each formula by the rules of
    /// model-runtime: the digest it ran under for a value computed; the one
    /// held for a value not computed; for a formula with none, the digest it
    /// has now where the record is the first, or names this model, or is of
    /// formulas already — a formula new since — and *not known* where a record
    /// that never told formulas apart names another model. A formula the model
    /// no longer has is dropped.
    pub fn record(
        &self,
        sample: &Path,
        digest: &TemplateDigest,
        formulas: &Formulas,
    ) -> Result<(), ModelError> {
        let key = Self::key(sample);
        let old = self.held(&key);
        let held = if formulas.digests.is_empty() {
            // The worker said nothing of its formulas. A record that cannot
            // tell them apart, under another model, is kept as it is: moving
            // its model forward would silence the one warning it can give.
            match old {
                Some(Held {
                    model,
                    formulas: None,
                }) if model != digest.as_str() => return Ok(()),
                Some(old) => Held {
                    model: digest.to_string(),
                    formulas: old.formulas,
                },
                None => Held {
                    model: digest.to_string(),
                    formulas: None,
                },
            }
        } else {
            let recorded = formulas
                .digests
                .iter()
                .map(|(name, now)| {
                    let kept = if formulas.computed.contains(name) {
                        Some(now.clone())
                    } else {
                        match &old {
                            None => Some(now.clone()),
                            Some(Held {
                                formulas: Some(held),
                                ..
                            }) => held.get(name).cloned().unwrap_or_else(|| Some(now.clone())),
                            Some(Held {
                                model,
                                formulas: None,
                            }) => (model == digest.as_str()).then(|| now.clone()),
                        }
                    };
                    (name.clone(), kept)
                })
                .collect();
            Held {
                model: digest.to_string(),
                formulas: Some(recorded),
            }
        };
        let mut table = self.table();
        table.insert(
            key,
            json!({
                "model": held.model,
                "formulas": held.formulas,
            }),
        );
        let failed = |error: std::io::Error| ModelError::NotRecorded {
            path: self.file.clone(),
            reason: error.to_string(),
        };
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent).map_err(failed)?;
        }
        let text = serde_json::to_string_pretty(&Json::Object(table))
            .map_err(|error| failed(std::io::Error::other(error)))?
            + "\n";
        crate::format::document::write_atomically(&self.file, text.as_bytes()).map_err(failed)
    }
}

// ----------------------------------------------------------------- worker

/// One interpreter running `samplekit._worker`, spoken to in JSON lines.
pub struct Worker {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    errors: Arc<Mutex<VecDeque<String>>>,
    reader: Option<JoinHandle<()>>,
    python: PathBuf,
}

/// Where the model's own output goes once the worker is ready: stderr, unless
/// the command line writes it above a progress bar.
static RELAY: Mutex<Option<fn(&str)>> = Mutex::new(None);

/// Routes every line the model prints through `relay` from now on.
pub fn relay_output_through(relay: fn(&str)) {
    *RELAY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(relay);
}

fn relay(line: &str) {
    // Copied out, so that the relay never runs under this lock.
    let hook = *RELAY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match hook {
        Some(hook) => hook(line),
        // Never `eprintln!`: it panics when the write fails, and a reader that
        // left — `samplekit compute. 2>&1 | head` — is how a pipeline ends, not
        // a reason to kill a computation mid-value.
        None => {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }
}

impl Worker {
    /// Spawns the worker and reads its announcement. A package of another
    /// version is refused before any request is sent.
    pub fn start(python: &Path) -> Result<Worker, ModelError> {
        // No `__pycache__` left beside the model: a command that only reads
        // writes nothing into the project, the model's folder included.
        let mut child = Command::new(python)
            .env("PYTHONDONTWRITEBYTECODE", "1")
            // The computation is the command's snapshot, not one per sample the
            // worker saves.
            .env("SAMPLEKIT_HISTORY", "off")
            // `-P`: the folder the command runs in is not put on `sys.path`,
            // where a `json.py` or a `logging.py` of the user's shadowed the
            // standard library the worker imports (Python 3.11, as required).
            .args(["-P", "-u", "-m", "samplekit._worker"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| ModelError::NoInterpreter {
                reason: format!("{} could not be started: {error}", python.display()),
            })?;
        let errors = Arc::new(Mutex::new(VecDeque::new()));
        let ready = Arc::new(AtomicBool::new(false));
        let stderr = child.stderr.take().expect("stderr is piped");
        let reader = {
            let errors = errors.clone();
            let ready = ready.clone();
            std::thread::spawn(move || {
                // Bytes, then text with what does not decode replaced: one
                // Latin-1 line from a C library stopped the relay, the pipe
                // closed, and the model's next write raised a broken pipe.
                let mut reader = BufReader::new(stderr);
                let mut bytes = Vec::new();
                loop {
                    bytes.clear();
                    match std::io::BufRead::read_until(&mut reader, b'\n', &mut bytes) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let line = String::from_utf8_lossy(&bytes)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();
                    // Before the announcement this is an import failure, which
                    // the error reports; after it, the model's own output.
                    if ready.load(Ordering::Relaxed) {
                        relay(&line);
                    }
                    let mut tail = errors.lock().expect("the tail is never poisoned");
                    if tail.len() == TAIL {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            })
        };
        let mut worker = Worker {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().expect("stdout is piped")),
            child,
            errors,
            reader: Some(reader),
            python: python.to_path_buf(),
        };
        let Ok(first) = worker.receive() else {
            let detail = worker.finish();
            return Err(
                if detail.contains("No module named") && detail.contains("samplekit") {
                    ModelError::NotInstalled {
                        python: python.to_path_buf(),
                        detail,
                    }
                } else {
                    ModelError::WorkerDied {
                        detail: Some(detail),
                    }
                },
            );
        };
        match first
            .get("ready")
            .and_then(|ready| ready.get("version"))
            .and_then(Json::as_str)
        {
            Some(version) if version == VERSION => {
                ready.store(true, Ordering::Relaxed);
                Ok(worker)
            }
            Some(version) => {
                let package = version.to_string();
                worker.finish();
                Err(ModelError::VersionMismatch {
                    python: python.to_path_buf(),
                    binary: VERSION.to_string(),
                    package,
                })
            }
            None => {
                let detail = worker.finish();
                Err(ModelError::WorkerDied {
                    detail: Some(format!("it began with {first}\n{detail}")),
                })
            }
        }
    }

    pub fn python(&self) -> &Path {
        &self.python
    }

    /// Imports the model and writes its description, computing nothing.
    pub fn describe(&mut self, template: &Template) -> Result<(), ModelError> {
        self.send(&json!({
            "op": "describe",
            "model": template.path.to_string_lossy(),
            "class": template.class,
            "root": template.root().to_string_lossy(),
        }))?;
        let mut failure = None;
        loop {
            let received = self.receive()?;
            if let Some(error) = received.get("error") {
                failure = Some(worker_error(error));
            } else if received.get("done").is_some() {
                return failure.map_or(Ok(()), Err);
            }
        }
    }

    /// What a computation would run, and why; runs nothing.
    pub fn plan(&mut self, request: &Request) -> Result<Vec<Planned>, ModelError> {
        self.send(&message("plan", request))?;
        let mut plan = Vec::new();
        let mut failure = None;
        loop {
            let received = self.receive()?;
            if let Some(entries) = received.get("plan").and_then(Json::as_array) {
                plan = entries.iter().filter_map(planned).collect();
            } else if let Some(error) = received.get("error") {
                failure = Some(worker_error(error));
            } else if received.get("done").is_some() {
                return failure.map_or(Ok(plan), Err);
            }
        }
    }

    /// Computes a sample value by value, reporting each, and saves it when
    /// `write`; otherwise the worker keeps what it computed for `write`.
    pub fn compute(
        &mut self,
        request: &Request,
        write: bool,
        on: &mut dyn FnMut(&Event),
    ) -> Result<Report, ModelError> {
        self.send(&message(if write { "compute" } else { "try" }, request))?;
        let mut report = Report::default();
        let mut failure = None;
        loop {
            let received = self.receive()?;
            let text = |key: &str| received.get(key).and_then(Json::as_str).map(str::to_string);
            if let Some(entries) = received.get("plan").and_then(Json::as_array) {
                on(&Event::Planned {
                    count: entries.len(),
                });
            } else if let Some(value) = text("started") {
                on(&Event::Started { value });
            } else if let Some(value) = text("finished") {
                report.computed += 1;
                let seconds = received
                    .get("seconds")
                    .and_then(Json::as_f64)
                    .unwrap_or(0.0);
                let before = text("before").unwrap_or_default();
                let after = text("after").unwrap_or_default();
                on(&Event::Finished {
                    value,
                    seconds,
                    before,
                    after,
                });
            } else if let Some(value) = text("failed") {
                report.failed += 1;
                let traceback = text("traceback").unwrap_or_default();
                on(&Event::Failed { value, traceback });
            } else if let Some(value) = text("waiting") {
                report.waiting += 1;
                let names = |key: &str| -> Vec<String> {
                    received
                        .get(key)
                        .and_then(Json::as_array)
                        .map(|names| {
                            names
                                .iter()
                                .filter_map(Json::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default()
                };
                on(&Event::Waiting {
                    value,
                    inputs: names("inputs"),
                    failed: names("failed"),
                });
            } else if let Some(output) = received.get("output") {
                let field = |key: &str| output.get(key).and_then(Json::as_str).map(str::to_string);
                on(&Event::Output {
                    value: field("value"),
                    text: field("text").unwrap_or_default(),
                });
            } else if let Some(log) = received.get("log") {
                let field = |key: &str| log.get(key).and_then(Json::as_str).map(str::to_string);
                on(&Event::Log {
                    level: field("level").unwrap_or_default(),
                    value: field("value"),
                    message: field("message").unwrap_or_default(),
                });
            } else if let Some(pending) = received.get("pending").and_then(Json::as_u64) {
                report.pending += usize::try_from(pending).unwrap_or(usize::MAX);
            } else if take_formulas(&received, &mut report.formulas) {
                // Taken: what the run records of the model's formulas.
            } else if received.get("saved").is_some() {
                report.saved = true;
            } else if let Some(error) = received.get("error") {
                failure = Some(worker_error(error));
            } else if received.get("done").is_some() {
                return failure.map_or(Ok(report), Err);
            }
        }
    }

    /// Writes what a `compute` without `write` computed for this sample: the
    /// report says whether it was saved, and which formulas computed it.
    pub fn write(&mut self, request: &Request) -> Result<Report, ModelError> {
        self.send(&message("write", request))?;
        let mut report = Report::default();
        let mut failure = None;
        loop {
            let received = self.receive()?;
            if take_formulas(&received, &mut report.formulas) {
                // Taken: what the write records of the model's formulas.
            } else if received.get("saved").is_some() {
                report.saved = true;
            } else if let Some(error) = received.get("error") {
                failure = Some(worker_error(error));
            } else if received.get("done").is_some() {
                return failure.map_or(Ok(report), Err);
            }
        }
    }

    /// The worker's process, for whoever must interrupt it.
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    fn send(&mut self, request: &Json) -> Result<(), ModelError> {
        let written = match self.input.as_mut() {
            Some(input) => writeln!(input, "{request}").and_then(|()| input.flush()),
            None => Err(std::io::Error::other("the worker's input is closed")),
        };
        if written.is_err() {
            let detail = self.abandon();
            return Err(ModelError::WorkerDied {
                detail: Some(detail),
            });
        }
        Ok(())
    }

    fn receive(&mut self) -> Result<Json, ModelError> {
        loop {
            let mut line = String::new();
            let read = self.output.read_line(&mut line);
            match read {
                Ok(0) | Err(_) => {
                    let detail = self.abandon();
                    return Err(ModelError::WorkerDied {
                        detail: (!detail.is_empty()).then_some(detail),
                    });
                }
                Ok(_) if line.trim().is_empty() => continue,
                Ok(_) => {
                    return serde_json::from_str(&line).map_err(|error| {
                        let detail = self.abandon();
                        ModelError::WorkerDied {
                            detail: Some(format!("it wrote {}: {error}\n{detail}", line.trim())),
                        }
                    });
                }
            }
        }
    }

    /// Kills the worker and returns what it last wrote to stderr: after a line
    /// that cannot be read, or a pipe that broke, the conversation is over,
    /// and a worker left running went on computing and saving a run already
    /// reported as ended — or, waiting on a request that never came, was
    /// waited for for ever.
    fn abandon(&mut self) -> String {
        let _ = self.child.kill();
        self.finish()
    }

    /// Closes the worker's input, waits for it, and returns what it last wrote
    /// to stderr.
    fn finish(&mut self) -> String {
        drop(self.input.take());
        // What it still writes is read and dropped: a worker blocked on a full
        // pipe that nobody reads never reaches the end of its input, and
        // waiting for it waited for ever.
        let _ = std::io::copy(&mut self.output, &mut std::io::sink());
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let tail = self.errors.lock().expect("the tail is never poisoned");
        tail.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.finish();
    }
}

fn message(op: &str, request: &Request) -> Json {
    json!({
        "op": op,
        "sample": request.sample.to_string_lossy(),
        "model": request.template.path.to_string_lossy(),
        "class": request.template.class,
        "root": request.template.root().to_string_lossy(),
        "names": request.names,
        "rerun": request.rerun,
        "force": request.force,
        "refused": request
            .refused
            .iter()
            .map(|(name, text)| json!({"name": name, "text": text}))
            .collect::<Vec<_>>(),
        "recorded": request.recorded,
    })
}

/// Takes what a message says of the model's formulas — their digests, or the
/// values computed whole — into `formulas`; whether it said anything of them.
fn take_formulas(received: &Json, formulas: &mut Formulas) -> bool {
    if let Some(digests) = received.get("formulas").and_then(Json::as_object) {
        formulas.digests = digests
            .iter()
            .filter_map(|(name, digest)| Some((name.clone(), digest.as_str()?.to_string())))
            .collect();
        true
    } else if let Some(computed) = received.get("computed").and_then(Json::as_array) {
        formulas.computed = computed
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect();
        true
    } else {
        false
    }
}

fn planned(entry: &Json) -> Option<Planned> {
    let pair = entry.as_array()?;
    let said = pair.get(1)?.as_str()?;
    Some(Planned {
        value: pair.first()?.as_str()?.to_string(),
        reason: Reason::from_worker(said),
        said: said.to_string(),
    })
}

fn worker_error(error: &Json) -> ModelError {
    let text = |key: &str| {
        error
            .get(key)
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_string()
    };
    match error.get("kind").and_then(Json::as_str) {
        // The model's own traceback where it raised as it was imported — a
        // file, a line, a type — and the message alone where the error is
        // ours, a class not found, which a traceback would only bury.
        Some("model") => {
            let traceback = text("traceback");
            ModelError::ModelClass {
                message: if traceback.contains("\n  File ") {
                    traceback.trim_end().to_string()
                } else {
                    text("message")
                },
            }
        }
        Some("names") => ModelError::UnknownValue {
            message: text("message"),
        },
        Some("save") => ModelError::NotSaved {
            message: text("message"),
        },
        _ => ModelError::TemplateFailed {
            traceback: text("traceback"),
        },
    }
}

// ----------------------------------------------------------------- errors

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelError {
    TemplateNotFound {
        path: PathBuf,
    },
    ModelClass {
        message: String,
    },
    NoInterpreter {
        reason: String,
    },
    NotInstalled {
        python: PathBuf,
        detail: String,
    },
    VersionMismatch {
        python: PathBuf,
        binary: String,
        package: String,
    },
    TemplateFailed {
        traceback: String,
    },
    /// A value `--only` names that this sample does not have.
    UnknownValue {
        message: String,
    },
    /// A sample the worker computed and could not write.
    NotSaved {
        message: String,
    },
    WorkerDied {
        detail: Option<String>,
    },
    /// What the machine records — `computed.json` — could not be written.
    NotRecorded {
        path: PathBuf,
        reason: String,
    },
    Unreadable {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::TemplateNotFound { path } => {
                write!(f, "the model {} does not exist", path.display())
            }
            ModelError::ModelClass { message } => f.write_str(message),
            ModelError::NoInterpreter { reason } => write!(f, "the model cannot run: {reason}"),
            ModelError::NotInstalled { python, detail } => {
                let last = detail.lines().last().unwrap_or_default();
                write!(
                    f,
                    "the model cannot run: {} cannot import samplekit\n  {last}\n\n\
                     install samplekit {VERSION} into that environment",
                    python.display()
                )
            }
            ModelError::VersionMismatch {
                python,
                binary,
                package,
            } => write!(
                f,
                "the model cannot run: this is samplekit {binary}, and {} has samplekit \
                 {package}\n\ninstall samplekit {binary} into that environment",
                python.display()
            ),
            ModelError::UnknownValue { message } => f.write_str(message),
            ModelError::TemplateFailed { traceback } => {
                write!(f, "the model raised while loading a sample\n{traceback}")
            }
            ModelError::NotSaved { message } => write!(f, "not saved: {message}"),
            ModelError::WorkerDied { detail } => {
                f.write_str("the Python worker exited without answering")?;
                if let Some(detail) = detail.as_deref().filter(|detail| !detail.is_empty()) {
                    write!(f, "\n{detail}")?;
                }
                Ok(())
            }
            ModelError::NotRecorded { path, reason } => {
                write!(f, "{} could not be written: {reason}", path.display())
            }
            ModelError::Unreadable { path, reason } => {
                write!(f, "{} could not be read: {reason}", path.display())
            }
        }
    }
}

impl Error for ModelError {}

// ------------------------------------------------------- read as text

/// The figures a model's source declares, read as text: a method whose
/// decorators include `@sk.figure` (or `@figure`, `@samplekit.figure`). No code
/// runs, which is what completion can afford, and a figure made at run time is
/// not found; the model's description holds it.
pub fn figures_in_source(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut marked = false;
    for line in source.lines() {
        let line = line.trim_start();
        if let Some(decorator) = line.strip_prefix('@') {
            let name = decorator
                .split(['(', ' ', '#'])
                .next()
                .unwrap_or_default()
                .trim();
            marked |= matches!(name, "sk.figure" | "figure" | "samplekit.figure");
            continue;
        }
        if marked
            && let Some(rest) = line
                .strip_prefix("def ")
                .or_else(|| line.strip_prefix("async def "))
            && let Some(name) = rest.split('(').next()
        {
            found.push(name.trim().to_string());
        }
        if !line.is_empty() && !line.starts_with('#') {
            marked = false;
        }
    }
    // In the order the source declares them, as `[figure.*]` lists its own:
    // a message naming the figures listed them sorted beside the project's
    // in declaration order.
    let mut seen = std::collections::HashSet::new();
    found.retain(|name| seen.insert(name.clone()));
    found
}

/// The names a model's source declares as values — `self.brix = sk.Property(…)`,
/// `self.m = sk.Table(…)` and the columns of such a table, `"ebc":
/// sk.Column(…)` — read as text, so that a value the model declares and no
/// sample holds yet is told from a misspelt one without importing the model.
pub fn quantities_in_source(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in source.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("self.")
            && let Some((name, value)) = rest.split_once('=')
            && !value.trim_start().starts_with('=')
            && (value.contains("Property(") || value.contains("Table("))
        {
            let name = name.trim();
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
                found.push(name.to_string());
            }
        }
        for (at, _) in line.match_indices("sk.Column(") {
            let before = line[..at].trim_end().trim_end_matches(':').trim_end();
            if let Some(quoted) = before
                .strip_suffix('"')
                .or_else(|| before.strip_suffix('\''))
                && let Some(open) = quoted.rfind(['"', '\''])
            {
                found.push(quoted[open + 1..].to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The properties a model's source declares, `self.name = sk.Property(…)`:
/// a table's name and its columns' are not values a column or a sort names.
pub fn properties_in_source(source: &str) -> Vec<String> {
    let mut found: Vec<String> = source
        .lines()
        .filter_map(|line| {
            let (name, value) = line.trim_start().strip_prefix("self.")?.split_once('=')?;
            let name = name.trim();
            (!value.trim_start().starts_with('=')
                && value.contains("Property(")
                && !name.is_empty()
                && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
            .then(|| name.to_string())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The properties a declared model's source files declare — read, never run —
/// kept while none of those files changes, a helper beside the model
/// included. Empty where no model is declared or nothing can be read.
fn source_properties(config: &ProjectConfig) -> Vec<String> {
    type Read = (Vec<(PathBuf, Option<std::time::SystemTime>)>, Vec<String>);
    thread_local! {
        static READ: std::cell::RefCell<Vec<Read>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let Some(template) = template_of(config) else {
        return Vec::new();
    };
    let files = template_files(&template).unwrap_or_default();
    let stamp: Vec<(PathBuf, Option<std::time::SystemTime>)> = files
        .iter()
        .map(|file| {
            let modified = fs::metadata(file).and_then(|m| m.modified()).ok();
            (file.clone(), modified)
        })
        .collect();
    READ.with(|read| {
        if let Some((_, names)) = read.borrow().iter().find(|(at, _)| *at == stamp) {
            return names.clone();
        }
        let mut names: Vec<String> = files
            .iter()
            .filter_map(|file| fs::read_to_string(file).ok())
            .flat_map(|source| properties_in_source(&source))
            .collect();
        names.sort();
        names.dedup();
        let mut read = read.borrow_mut();
        let directory = template.directory().to_path_buf();
        read.retain(|(at, _)| !at.iter().all(|(file, _)| file.starts_with(&directory)));
        read.push((stamp, names.clone()));
        names
    })
}

/// The properties a declared model declares, sorted: its description's where it
/// is current, its source's read as text where it is not. Empty where no model
/// is declared or nothing can be read.
pub fn declared_properties(config: &ProjectConfig) -> Vec<String> {
    if let Some(description) = current_description(config) {
        let mut names: Vec<String> = description.properties.keys().cloned().collect();
        names.sort();
        return names;
    }
    source_properties(config)
}

/// The names a declared model declares, for what may start no Python: the
/// figures, in the order the model declares them, and the values — properties,
/// tables and their columns — sorted. Its description's where it is current,
/// its source's read as text where it is not. Empty where no model is declared
/// or its files cannot be read — a reading for completion and for `validate`,
/// and never a reason to refuse anything by itself.
pub fn model_declarations(config: &ProjectConfig) -> (Vec<String>, Vec<String>) {
    if let Some(description) = current_description(config) {
        return (description.figure_names(), description.value_names());
    }
    let Some(template) = template_of(config) else {
        return (Vec::new(), Vec::new());
    };
    let (mut figures, mut quantities) = (Vec::new(), Vec::new());
    for file in template_files(&template).unwrap_or_default() {
        if let Ok(source) = fs::read_to_string(&file) {
            figures.extend(figures_in_source(&source));
            quantities.extend(quantities_in_source(&source));
        }
    }
    // Figures in the order the files declare them; quantities sorted.
    let mut seen = std::collections::HashSet::new();
    figures.retain(|name| seen.insert(name.clone()));
    quantities.sort();
    quantities.dedup();
    (figures, quantities)
}

#[cfg(test)]
mod tests {
    use super::{figures_in_source, quantities_in_source};

    #[test]
    fn a_models_figures_are_read_from_its_source() {
        let source = "import samplekit as sk\n\nclass C(sk.Sample):\n    @sk.figure\n    \
                      def one(self, ax):\n        pass\n\n    @sk.figure(subplots=(1, 2))\n    \
                      @classmethod\n    def many(cls, samples, axes):\n        pass\n\n    \
                      @property\n    def plain(self):\n        pass\n";
        assert_eq!(figures_in_source(source), ["one", "many"]);
    }

    #[test]
    fn a_models_values_are_read_from_its_source() {
        let source = "class C(sk.Sample):\n    def __init__(self):\n        self.brix = \
                      sk.Property(compute=self._brix)\n        self.m = sk.Table({\n            \
                      \"T\": sk.Column(unit=\"C\"),\n            'ebc': sk.Column()})\n        \
                      self.helper = 3\n        if self.brix == 2: pass\n";
        assert_eq!(quantities_in_source(source), ["T", "brix", "ebc", "m"]);
    }
}
