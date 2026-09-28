//! A project set up, or completed: what a directory already holds, the steps
//! that would complete it, and those the user accepts, done — what `init` and
//! the workbench both do, asked as questions and ending in an environment with
//! samplekit in it.

use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::model_runtime::find_interpreter;
use crate::config::project_config;
use crate::format::document::{DocumentError, replace_if_unchanged};

/// A file of the starter project: where it goes from the root, what it holds,
/// and what it is for. Kept as files so that they are edited as files: a
/// project a newcomer reads is not a string literal in Rust.
pub struct Template {
    pub path: &'static str,
    pub contents: &'static str,
    pub what: &'static str,
}

/// The project a newcomer needs: a configuration, a model with one of every
/// kind of formula, and a sample the model can run on.
pub const TEMPLATE: [Template; 5] = [
    Template {
        path: ".samplekitrc",
        contents: include_str!("project_template/samplekitrc"),
        what: "the project: units, precisions, a query, a profile, an export",
    },
    Template {
        path: "model/brew.py",
        contents: include_str!("project_template/brew.py"),
        what: "the model of a brew: 9 quantities, a table, one of each kind of formula",
    },
    Template {
        path: "model/helpers/__init__.py",
        contents: include_str!("project_template/helpers__init__.py"),
        what: "the helpers package, so the model imports one name from one place",
    },
    Template {
        path: "model/helpers/hydrometer.py",
        contents: include_str!("project_template/helpers_hydrometer.py"),
        what: "what a hydrometer gives, and least squares, with nothing to install",
    },
    Template {
        path: "samples/EXAMPLE.md",
        contents: include_str!("project_template/sample.md"),
        what: "one brew, measured values only",
    },
];

/// The empty project's configuration, in parts: the model's section is either
/// written or said in a comment, as the project has one or not.
const EMPTY_HEAD: &str = include_str!("project_template/empty/samplekitrc_head");
const EMPTY_MODEL: &str = include_str!("project_template/empty/samplekitrc_model");
const EMPTY_NO_MODEL: &str = include_str!("project_template/empty/samplekitrc_no_model");
/// A model the user already has, `{path}` its path as TOML writes it.
const EMPTY_EXISTING_MODEL: &str =
    include_str!("project_template/empty/samplekitrc_existing_model");
const EMPTY_REST: &str = include_str!("project_template/empty/samplekitrc_rest");
const EMPTY_MODEL_FILE: &str = include_str!("project_template/empty/model.py");

/// Where a project starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Starter {
    /// The structure alone, to fill in: the default.
    Empty,
    /// A model with one formula of every kind, and a sample it runs on.
    Example,
}

/// The model an empty project is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelAnswer {
    /// `model/main.py`, to fill in: it runs as it is. The default.
    Create,
    /// A model file the user already has, named in `.samplekitrc` where it
    /// is: its path from the project's folder, or whole outside it, as
    /// [`model_path`] gives it. Nothing is copied or written beside it.
    Existing(PathBuf),
    /// None: every value typed in the samples.
    Without,
}

/// What the person setting a project up chose. The default is the minimal
/// project: empty, with a model to fill in, and an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    pub starter: Starter,
    /// The model; asked of an empty project only.
    pub model: ModelAnswer,
    /// The environment made, or completed, with samplekit in it.
    pub environment: bool,
}

impl Default for Answers {
    fn default() -> Self {
        Answers {
            starter: Starter::Empty,
            model: ModelAnswer::Create,
            environment: true,
        }
    }
}

impl Answers {
    /// Whether the project will have a model: one made, or one named.
    pub fn has_model(&self) -> bool {
        self.model != ModelAnswer::Without
    }
}

/// A model file typed after *use a model file I already have*, checked: `typed`
/// read from the project's folder `root`, or whole. It must be a Python file
/// that is there. What `.samplekitrc` will name: relative to `root` where it is
/// inside it, `/`-separated.
pub fn model_path(root: &Path, typed: &str) -> Result<PathBuf, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("type the model file's path, from the project's folder".to_string());
    }
    let given = Path::new(typed);
    let file = root.join(given);
    if !file.is_file() {
        return Err(format!(
            "no file at {typed}: a path from the project's folder, or a whole path"
        ));
    }
    if file.extension().and_then(|extension| extension.to_str()) != Some("py") {
        return Err(format!(
            "{typed} is not a Python file: a model is a .py file"
        ));
    }
    // Inside the project, said from it; outside, whole.
    let inside = if given.is_absolute() {
        let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let canonical = dunce::canonicalize(&file).unwrap_or_else(|_| file.clone());
        canonical
            .strip_prefix(&canonical_root)
            .ok()
            .map(Path::to_path_buf)
    } else {
        Some(given.to_path_buf())
    };
    Ok(match inside {
        Some(relative) => PathBuf::from(
            relative
                .components()
                .filter(|component| !matches!(component, std::path::Component::CurDir))
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("/"),
        ),
        None => given.to_path_buf(),
    })
}

/// What the nearest environment holds, from `root` upward, as a setup
/// sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Environment {
    /// None in `root`, nor one with samplekit above it.
    Missing,
    /// `root/.venv/`, without samplekit.
    WithoutSamplekit,
    /// One with samplekit, here or above: nothing to do.
    Ready(PathBuf),
}

/// The environment `root` would compute in.
pub fn environment(root: &Path) -> Environment {
    let Some(python) = find_interpreter(root) else {
        return Environment::Missing;
    };
    // `.venv/bin/python`: the environment is two levels up.
    let Some(venv) = python.parent().and_then(Path::parent) else {
        return Environment::Missing;
    };
    if has_samplekit(venv) {
        return Environment::Ready(venv.to_path_buf());
    }
    let here = dunce::canonicalize(root.join(".venv")).ok();
    if here.as_deref() == Some(venv) {
        Environment::WithoutSamplekit
    } else {
        // One above without samplekit is not this project's to change.
        Environment::Missing
    }
}

/// Whether the environment at `venv` holds the samplekit package, read from
/// its `site-packages` rather than by starting its Python.
fn has_samplekit(venv: &Path) -> bool {
    let holds = |site: PathBuf| site.join("samplekit").join("__init__.py").is_file();
    if holds(venv.join("Lib").join("site-packages")) {
        return true;
    }
    std::fs::read_dir(venv.join("lib"))
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| holds(entry.path().join("site-packages")))
        })
        .unwrap_or(false)
}

/// One question a setup asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    Starter,
    Model,
    Environment,
}

impl Question {
    /// The question, as it is asked: what is asked, in words a newcomer reads
    /// without knowing SampleKit's.
    pub fn ask(self) -> &'static str {
        match self {
            Question::Starter => "What should the new project start from?",
            Question::Model => {
                "Which model should the project use? A model is a Python file that \
                 says what a sample holds and computes the values derived from those \
                 you measure."
            }
            Question::Environment => {
                "Should SampleKit prepare the Python environment the model runs in?"
            }
        }
    }

    /// Its answers, the first the default and said so: a short label, and what
    /// choosing it does.
    pub fn choices(self, root: &Path) -> Vec<(&'static str, String)> {
        let version = python_spelling(env!("CARGO_PKG_VERSION"));
        match self {
            Question::Starter => vec![
                (
                    "an empty project (default)",
                    "a configuration and an empty samples/ folder, for your own samples; \
                     the next question asks about a model"
                        .to_string(),
                ),
                (
                    "the example project",
                    "a working project to learn from, a brew as in the demo: a model with \
                     one formula of each kind, and a sample it computes"
                        .to_string(),
                ),
            ],
            Question::Model => vec![
                (
                    "create a model to fill in (default)",
                    "writes model/main.py, which runs as it is and shows in comments \
                     how to declare each kind of value"
                        .to_string(),
                ),
                (
                    "use a model file I already have",
                    "you type its path next; .samplekitrc names it where it is, and \
                     nothing is copied"
                        .to_string(),
                ),
                (
                    "no model for now",
                    "every value is typed in the samples; .samplekitrc says in a comment \
                     how to add a model later"
                        .to_string(),
                ),
            ],
            Question::Environment => match environment(root) {
                Environment::WithoutSamplekit => vec![
                    (
                        "yes, install samplekit (default)",
                        format!(
                            "installs samplekit {version} in the .venv/ already here, \
                             a minute or so"
                        ),
                    ),
                    (
                        "no",
                        "leaves .venv/ as it is: the model runs only once samplekit is in it"
                            .to_string(),
                    ),
                ],
                _ => vec![
                    (
                        "yes, make one (default)",
                        format!(
                            "creates .venv/ in the project and installs samplekit {version} \
                             in it, a minute or so"
                        ),
                    ),
                    (
                        "no",
                        "you use a Python of your own, which [model] python in .samplekitrc \
                         names"
                            .to_string(),
                    ),
                ],
            },
        }
    }

    /// Which of its answers `answers` holds.
    pub fn chosen(self, answers: &Answers) -> usize {
        match self {
            Question::Starter => usize::from(answers.starter != Starter::Empty),
            Question::Model => match answers.model {
                ModelAnswer::Create => 0,
                ModelAnswer::Existing(_) => 1,
                ModelAnswer::Without => 2,
            },
            Question::Environment => usize::from(!answers.environment),
        }
    }

    /// Whether `choice` asks for more before it is an answer: the model file's
    /// path, which [`model_path`] checks and the surface then sets as
    /// [`ModelAnswer::Existing`].
    pub fn asks_path(self, choice: usize) -> bool {
        self == Question::Model && choice == 1
    }

    /// `answers` with this question's `choice` in it. The model file one
    /// already has is chosen with its path, which the surface sets once
    /// [`model_path`] has checked it; a path given before is kept.
    pub fn choose(self, answers: &mut Answers, choice: usize) {
        let first = choice == 0;
        match self {
            Question::Starter => {
                answers.starter = if first {
                    Starter::Empty
                } else {
                    Starter::Example
                }
            }
            Question::Model => {
                answers.model = match choice {
                    0 => ModelAnswer::Create,
                    1 => match &answers.model {
                        ModelAnswer::Existing(path) => ModelAnswer::Existing(path.clone()),
                        _ => ModelAnswer::Existing(PathBuf::new()),
                    },
                    _ => ModelAnswer::Without,
                }
            }
            Question::Environment => answers.environment = first,
        }
    }
}

/// The questions that apply to `root` as `answers` stand: none about the
/// files where a configuration is already there, the model's only for an
/// empty project, and the environment's unless samplekit is already in
/// one.
pub fn questions(root: &Path, answers: &Answers) -> Vec<Question> {
    let mut asked = Vec::new();
    if !root.join(".samplekitrc").exists() {
        asked.push(Question::Starter);
        if answers.starter == Starter::Empty {
            asked.push(Question::Model);
        }
    }
    if !matches!(environment(root), Environment::Ready(_)) {
        asked.push(Question::Environment);
    }
    asked
}

/// One thing setting a project up does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    File {
        path: PathBuf,
        contents: String,
        what: &'static str,
    },
    /// A directory made empty: where the samples go.
    Directory { path: PathBuf, what: &'static str },
    /// The environment, `create`d where there is none, with samplekit installed
    /// in it.
    Environment { create: bool },
}

impl Step {
    /// What the step is for, as a preview says it.
    pub fn what(&self) -> String {
        match self {
            Step::File { what, .. } | Step::Directory { what, .. } => what.to_string(),
            Step::Environment { create: true } => format!(
                "the environment the model runs in, with samplekit {} installed",
                python_spelling(env!("CARGO_PKG_VERSION"))
            ),
            Step::Environment { create: false } => format!(
                "samplekit {} installed in the environment already here",
                python_spelling(env!("CARGO_PKG_VERSION"))
            ),
        }
    }

    /// Its name in a preview: a path from `root`, a directory with its
    /// slash.
    pub fn name(&self, root: &Path) -> String {
        match self {
            Step::File { path, .. } => path
                .strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string(),
            Step::Directory { path, .. } => {
                format!("{}/", path.strip_prefix(root).unwrap_or(path).display())
            }
            Step::Environment { .. } => ".venv/".to_string(),
        }
    }
}

/// A step, and whether the directory already has what it would make. A file
/// already there is kept, never overwritten.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    pub step: Step,
    pub present: bool,
    /// Said beside a step already there: a configuration that does not load
    /// as it stands.
    pub note: Option<String>,
}

/// What a directory holds of a project, and what would complete it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub root: PathBuf,
    pub proposals: Vec<Proposal>,
    /// A file where a directory the steps need must go: nothing can be
    /// written until it moves, said before anything is tried.
    pub blocked: Option<PathBuf>,
}

impl Plan {
    /// The steps that would do something.
    pub fn missing(&self) -> impl Iterator<Item = &Proposal> {
        self.proposals.iter().filter(|proposal| !proposal.present)
    }

    /// Whether nothing is left to do.
    pub fn is_complete(&self) -> bool {
        self.missing().next().is_none()
    }
}

/// The files `answers` asks for: the example's five, or the empty project's
/// configuration and, with a model to fill in, the model; a model already there
/// is named, not written.
fn files(answers: &Answers) -> Vec<(&'static str, String, &'static str)> {
    match answers.starter {
        Starter::Example => TEMPLATE
            .iter()
            .map(|template| (template.path, template.contents.to_string(), template.what))
            .collect(),
        Starter::Empty => {
            let (model, what) = match &answers.model {
                ModelAnswer::Create => (
                    EMPTY_MODEL.to_string(),
                    "the project: its model and its collection, and in comments what else it may say",
                ),
                ModelAnswer::Existing(path) => (
                    EMPTY_EXISTING_MODEL.replace(
                        "{path}",
                        &toml::Value::String(path.to_string_lossy().to_string()).to_string(),
                    ),
                    "the project: your model, named where it is, its collection, and in comments what else it may say",
                ),
                ModelAnswer::Without => (
                    EMPTY_NO_MODEL.to_string(),
                    "the project: its collection, and in comments what else it may say",
                ),
            };
            let mut files = vec![(
                ".samplekitrc",
                format!("{EMPTY_HEAD}{model}{EMPTY_REST}"),
                what,
            )];
            if answers.model == ModelAnswer::Create {
                files.push((
                    "model/main.py",
                    EMPTY_MODEL_FILE.to_string(),
                    "the model, empty: it runs, and says in comments how to declare each kind",
                ));
            }
            files
        }
    }
}

/// What `root` holds, and the steps that would complete it as `answers` ask:
/// where a configuration is there already, the example's missing files if the
/// example is asked for and no others; the environment only where samplekit is
/// in none. Reads, writes nothing. No repository is offered: SampleKit keeps
/// its own history.
pub fn plan(root: &Path, answers: &Answers) -> Plan {
    let configured = root.join(".samplekitrc").exists();
    let mut proposals: Vec<Proposal> = files(answers)
        .into_iter()
        // A project already set up is given no model it did not ask for: the
        // example's files are completed only where it is asked for by name, the
        // empty project's never.
        .filter(|(path, ..)| {
            !configured || answers.starter == Starter::Example || *path == ".samplekitrc"
        })
        .map(|(relative, contents, what)| {
            let path = root.join(relative);
            let present = path.exists();
            // Kept is not read: a configuration that does not load is said
            // where a project is being looked at.
            let note = (present && relative == ".samplekitrc")
                .then(|| project_config::load(&path).err())
                .flatten()
                .map(|error| format!("and not read as it stands: {error}"));
            Proposal {
                step: Step::File {
                    path,
                    contents,
                    what,
                },
                present,
                note,
            }
        })
        .collect();
    if answers.starter == Starter::Empty && !configured {
        let path = root.join("samples");
        proposals.push(Proposal {
            present: path.is_dir(),
            step: Step::Directory {
                path,
                what: "where the samples go, empty: N in the TUI, or samplekit new, makes one",
            },
            note: None,
        });
    }
    if answers.environment {
        match environment(root) {
            Environment::Ready(_) => {}
            Environment::Missing => proposals.push(Proposal {
                step: Step::Environment { create: true },
                present: false,
                note: None,
            }),
            Environment::WithoutSamplekit => proposals.push(Proposal {
                step: Step::Environment { create: false },
                present: false,
                note: None,
            }),
        }
    }
    let blocked = proposals.iter().find_map(|proposal| match &proposal.step {
        Step::File { path, .. } if !proposal.present => blocking(path),
        Step::Directory { path, .. } if !proposal.present => {
            blocking(&path.join("_")).or_else(|| path.exists().then(|| path.clone()))
        }
        _ => None,
    });
    Plan {
        root: root.to_path_buf(),
        proposals,
        blocked,
    }
}

/// The file standing where a directory `path` needs must go.
fn blocking(path: &Path) -> Option<PathBuf> {
    let mut ancestor = path.parent();
    while let Some(directory) = ancestor.filter(|d| !d.as_os_str().is_empty()) {
        if directory.exists() && !directory.is_dir() {
            return Some(directory.to_path_buf());
        }
        ancestor = directory.parent();
    }
    None
}

/// What was done. The environment is not: it takes a minute, and is made
/// by [`make_environment`] where the surface can wait for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    /// The files written and the directories made, in order.
    pub written: Vec<PathBuf>,
    /// The environment step accepted, to make next.
    pub environment: Option<Step>,
    /// How many of `written` are directories.
    directories: usize,
}

impl Applied {
    /// The files written, the directories made left out: *3 files written*
    /// was said of two files and `samples/`.
    pub fn files_written(&self) -> usize {
        self.written.len() - self.directories
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// A file where a directory must go: nothing was written.
    Blocked { directory: PathBuf },
    /// A write failed, after those listed.
    Io {
        path: PathBuf,
        reason: String,
        written: Vec<PathBuf>,
    },
    /// A file made by another writer since the plan looked for it, kept as it
    /// is, after those listed.
    WrittenSince {
        path: PathBuf,
        written: Vec<PathBuf>,
    },
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (path, reason, written) = match self {
            SetupError::Blocked { directory } => {
                return write!(
                    f,
                    "{} is a file where init needs a directory, and nothing was written",
                    directory.display()
                );
            }
            SetupError::Io {
                path,
                reason,
                written,
            } => (path, reason.as_str(), written),
            SetupError::WrittenSince { path, written } => (path, WRITTEN_SINCE, written),
        };
        write!(f, "{}: {reason}", path.display())?;
        if !written.is_empty() {
            let names: Vec<String> = written
                .iter()
                .map(|path| path.display().to_string())
                .collect();
            write!(
                f,
                "\n  written before it: {} — delete them to start again",
                names.join(", ")
            )?;
        }
        Ok(())
    }
}

/// What a file another writer made since the plan is said to be.
pub const WRITTEN_SINCE: &str =
    "written by another process since the plan looked for it, and left as it is";

impl std::error::Error for SetupError {}

/// The missing steps `accept` keeps, done: every file checked before any is
/// written, a file already there never touched.
pub fn apply(plan: &Plan, accept: impl Fn(&Step) -> bool) -> Result<Applied, SetupError> {
    let chosen: Vec<&Step> = plan
        .missing()
        .map(|proposal| &proposal.step)
        .filter(|step| accept(step))
        .collect();
    // **Everything checked before anything is written.** A file where a
    // directory must go stopped the run halfway, having written some files
    // and said nothing of them.
    for step in &chosen {
        let blocked = match step {
            Step::File { path, .. } => blocking(path),
            Step::Directory { path, .. } => {
                blocking(&path.join("_")).or_else(|| path.exists().then(|| path.clone()))
            }
            _ => None,
        };
        if let Some(directory) = blocked {
            return Err(SetupError::Blocked { directory });
        }
    }
    let mut applied = Applied::default();
    for step in &chosen {
        if let Step::File { path, contents, .. } = step {
            // Never over a file that appeared after the plan was made: the file
            // is looked for again inside the lock that covers the write, as
            // every write of a sample is, and one another writer made meanwhile
            // — a `.samplekitrc` written by another hand — is kept. Written
            // whole or not at all.
            let wrote = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|source| DocumentError::Io {
                    path: path.clone(),
                    source,
                })
                .and_then(|()| replace_if_unchanged(path, None, Some(contents.as_bytes())));
            if let Err(error) = wrote {
                let path = path.clone();
                let written = applied.written;
                return Err(match error {
                    DocumentError::WrittenSince { .. } | DocumentError::ConcurrentEdit { .. } => {
                        SetupError::WrittenSince { path, written }
                    }
                    DocumentError::Io { source, .. } => SetupError::Io {
                        path,
                        reason: source.to_string(),
                        written,
                    },
                    other => SetupError::Io {
                        path,
                        reason: other.to_string(),
                        written,
                    },
                });
            }
            applied.written.push(path.clone());
        }
        if let Step::Directory { path, .. } = step {
            if let Err(error) = std::fs::create_dir_all(path) {
                return Err(SetupError::Io {
                    path: path.clone(),
                    reason: error.to_string(),
                    written: applied.written,
                });
            }
            applied.written.push(path.clone());
            applied.directories += 1;
        }
    }
    applied.environment = chosen
        .iter()
        .find(|step| matches!(step, Step::Environment { .. }))
        .map(|step| (*step).clone());
    Ok(applied)
}

/// The environment `step` asks for, made: created where there is none, then
/// samplekit installed in it by pip, in this command's version, and nothing
/// looked for beside it. What was installed, or what went wrong and the command
/// to run by hand.
pub fn make_environment(root: &Path, step: &Step) -> Result<String, String> {
    let Step::Environment { create } = step else {
        return Ok(String::new());
    };
    let environment = root.join(".venv");
    if *create {
        create_environment(&environment)?;
    }
    let python = if cfg!(windows) {
        environment.join("Scripts").join("python.exe")
    } else {
        environment.join("bin").join("python")
    };
    let package = format!("samplekit=={}", python_spelling(env!("CARGO_PKG_VERSION")));
    let installed = std::process::Command::new(&python)
        .args([
            "-m",
            "pip",
            "install",
            "--quiet",
            "--disable-pip-version-check",
            &package,
        ])
        .output();
    let by_hand = format!(
        "install it by hand: {} -m pip install {package}",
        python.strip_prefix(root).unwrap_or(&python).display()
    );
    match installed {
        Ok(output) if output.status.success() => Ok(format!(
            "samplekit {} installed in .venv/",
            python_spelling(env!("CARGO_PKG_VERSION"))
        )),
        Ok(output) => {
            let reason = last_line(&output.stderr).unwrap_or_else(|| "pip failed".to_string());
            Err(format!(
                "samplekit was not installed: {reason}\n  {by_hand}"
            ))
        }
        Err(error) => Err(format!("samplekit was not installed: {error}\n  {by_hand}")),
    }
}

/// A version as Python spells it, as pip and a user installing the package
/// write it: the build's `1.0.0-rc.1` is `1.0.0rc1`, `-alpha.N` is `aN`,
/// `-beta.N` `bN`; a release is itself.
pub fn python_spelling(version: &str) -> String {
    let Some((release, tag)) = version.split_once('-') else {
        return version.to_string();
    };
    let (kind, number) = tag.split_once('.').unwrap_or((tag, "0"));
    let short = match kind {
        "alpha" | "a" => "a",
        "beta" | "b" => "b",
        "rc" => "rc",
        // Nothing Python spells otherwise: left as the build says it.
        _ => return version.to_string(),
    };
    format!("{release}{short}{number}")
}

/// The interpreters an environment is made with, tried in this order: on
/// Windows the launcher `py -3` first, since `python3` is rarely there and
/// `python` may be the Store's placeholder; elsewhere `python3`, then
/// `python`.
pub fn interpreters() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(windows) {
        &[("py", &["-3"]), ("python", &[]), ("python3", &[])]
    } else {
        &[("python3", &[]), ("python", &[])]
    }
}

/// `-m venv` by the first interpreter of [`interpreters`] found. One found
/// that fails says its own reason; none found says which were looked for.
fn create_environment(environment: &Path) -> Result<(), String> {
    for (program, before) in interpreters() {
        let said = std::iter::once(*program)
            .chain(before.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let made = std::process::Command::new(program)
            .args(*before)
            .args(["-m", "venv"])
            .arg(environment)
            .output();
        match made {
            Ok(output) if output.status.success() => return Ok(()),
            // Python's own reason — ensurepip missing, a permission — is the
            // one worth saying.
            Ok(output) => {
                return Err(format!(
                    "{said} -m venv did not run: {}",
                    last_line(&output.stderr).unwrap_or_else(|| "it failed".to_string())
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("{said} -m venv did not run: {error}")),
        }
    }
    let tried: Vec<String> = interpreters()
        .iter()
        .map(|(program, before)| {
            std::iter::once(*program)
                .chain(before.iter().copied())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    Err(format!(
        "no Python was found to make the environment with: {} — install Python 3.11 \
         or later, then run init again",
        tried.join(", ")
    ))
}

/// The last line said that is not blank. Read with what does not decode
/// replaced: pip on a system in another code page said nothing at all.
fn last_line(said: &[u8]) -> Option<String> {
    String::from_utf8_lossy(said)
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}
