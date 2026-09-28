//! An ordered collection of samples and the operations over it. It composes
//! lower layers and adds one thing of its own: the guarantee that a collection
//! is a *sequence*, with a defined order at all times.

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::config::discovery::{self, DiscoveryError, SkipReason, Skipped, Warning};
use crate::config::project_config::{self as config, ConfigError, Profile, ProjectConfig};
use crate::core::identifier::Identifier;
use crate::core::sample::Sample;
use crate::core::statistics::{self, Summary};
use crate::core::value::Value;
use crate::format::document::{self, Destination, DocumentError};
use crate::query::field_addressing::{
    self as fields, Field, FieldError, Resolution, States, Subject, Vocabulary,
};
use crate::query::filter_language::{self as filter, Filter, FilterError};
use crate::query::ordering::{self, OrderError, SortSpec};

// ------------------------------------------------------------------- types

/// `Debug` prints what a collection *is* — where it came from and how many
/// entries it holds — and not every sample in it: a debug line for two hundred
/// samples is not a diagnostic.
pub struct SampleList {
    root: Option<PathBuf>,
    entries: Vec<Entry>,
    config: Option<ProjectConfig>,
    skipped: Vec<Skipped>,
    /// Which configuration each sample was found under, when several were.
    configurations: Vec<(PathBuf, PathBuf)>,
    /// Those configurations, loaded, by file.
    nearest: Vec<(PathBuf, ProjectConfig)>,
    warnings: Vec<Warning>,
    /// A configuration given instead of the nearest one (`--rc`), which then
    /// describes every sample read.
    forced: bool,
    /// The entries of the list this one was narrowed from, and empty for a list
    /// read from a directory or built from samples. A filter does not change
    /// what exists: sorting the samples it kept by a field only the others hold
    /// is a sort of absent values, not a misspelt field.
    within: Vec<Entry>,
    /// What the models of the list this one was narrowed from declare: a
    /// narrowing keeps no configuration, and a value the model gives before
    /// any sample holds it stays a name that exists.
    declared: Vec<Identifier>,
}

/// The samples of a list one configuration describes, from a list spanning
/// several.
pub struct ConfigurationPart<'a> {
    /// The configuration's file, when the list spans several.
    pub file: Option<PathBuf>,
    pub config: Option<&'a ProjectConfig>,
    /// Those samples, as a list carrying that configuration.
    pub samples: SampleList,
}

/// A sample and where it came from. The sample is behind an `Rc` because
/// filtering **shares** rather than copies: `Sample` is an identity, not a
/// value, and the subset and its parent must hold the same one.
#[derive(Clone)]
pub struct Entry {
    pub path: Option<PathBuf>,
    pub sample: Rc<RefCell<Sample>>,
    /// How the sample stands, where whoever holds the list read it with
    /// `with_states`: carried beside the sample, since what says it —
    /// validation, the model's plan — is not this module's to reach.
    pub states: Option<Rc<States>>,
}

/// How many values a summary used, and out of how many rows. Averaging a
/// column where seventy of two hundred samples lack the measurement is
/// legitimate; not knowing it is not.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSummary {
    pub summary: Summary,
    /// The rows the value applies to: those saying *not applicable* are counted
    /// apart.
    pub considered: usize,
    pub not_applicable: usize,
    /// The mean weighted by 1/u², where every value summarised has an
    /// uncertainty above zero: the mean a measurement's own spread asks for.
    /// `None` where one has none, and the plain mean stands.
    pub weighted_mean: Option<f64>,
}

impl ColumnSummary {
    /// The mean a summary shows: weighted where it could be.
    pub fn mean(&self) -> f64 {
        self.weighted_mean.unwrap_or(self.summary.mean)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldWarning {
    /// A name no sample in this collection has: the typo case, reported once.
    Unknown {
        field: String,
        suggestion: Option<String>,
    },
    /// The name exists, but denotes a table rather than one scalar cell.
    TableNeedsCell { table: String },
    /// A cell whose row no sample's table holds.
    NoRow { field: String, table: String },
    /// A list position no sample's list reaches.
    NoItem { field: String, name: String },
}

// ---------------------------------------------------------- construction

/// Loads a directory through [`discovery`], so the initial order is
/// deterministic and the configuration is the nearest one found.
///
/// **Loading never executes project Python.** A directory with a `[model]`
/// declaration loads as plain data.
pub fn from_directory(path: &Path) -> Result<SampleList, ListError> {
    // A configuration that will not read is set aside like an unreadable file,
    // and the samples it describes are read without it — the target's own as
    // much as a nested one. `load_directory` records it as skipped, since it
    // reads every configuration the collection spans.
    let (configuration, unread) = match config::load_for(path) {
        Ok(configuration) => (configuration, None),
        Err(error) => (
            None,
            config::find(path).map(|file| Skipped {
                path: file,
                reason: SkipReason::Configuration {
                    message: ListError::Config(error).to_string(),
                },
            }),
        ),
    };
    let mut list = load_directory(path, configuration, true)?;
    if let Some(unread) = unread
        && !list
            .skipped
            .iter()
            .any(|skipped| skipped.path == unread.path)
    {
        list.skipped.push(unread);
    }
    Ok(list)
}

/// The same, discovering and selecting with a configuration given instead of
/// the nearest one: what `--rc` asks for. The configuration describes every
/// sample, so none is recorded as found under another.
pub fn from_directory_with(
    path: &Path,
    configuration: Option<ProjectConfig>,
) -> Result<SampleList, ListError> {
    load_directory(path, configuration, false)
}

fn load_directory(
    path: &Path,
    configuration: Option<ProjectConfig>,
    nearest: bool,
) -> Result<SampleList, ListError> {
    let found = discovery::discover(path, configuration.as_ref()).map_err(ListError::Discovery)?;
    let mut entries = Vec::with_capacity(found.paths.len());
    let mut skipped = found.skipped;
    for path in &found.paths {
        // A file that will not load is set aside with its reason, never a stop:
        // one file must not hide the collection, and it is said.
        match document::load_sample(path) {
            Ok((sample, _)) => entries.push(Entry {
                path: Some(path.clone()),
                sample: Rc::new(RefCell::new(sample)),
                states: None,
            }),
            Err(error) => skipped.push(Skipped {
                path: path.clone(),
                reason: SkipReason::Malformed {
                    message: error.to_string(),
                },
            }),
        }
    }
    // Each sample answers to its nearest configuration: every one the list
    // spans is loaded, once.
    let mut loaded: Vec<(PathBuf, ProjectConfig)> = Vec::new();
    if nearest {
        let mut unread: Vec<PathBuf> = Vec::new();
        for (_, file) in &found.configurations {
            if !file.is_file()
                || loaded.iter().any(|(held, _)| held == file)
                || unread.contains(file)
            {
                continue;
            }
            // A configuration that will not read is set aside and said, and its
            // samples are read without it: one file must not hide the rest.
            match config::load(file) {
                Ok(config) => loaded.push((file.clone(), config)),
                Err(error) => {
                    unread.push(file.clone());
                    skipped.push(Skipped {
                        path: file.clone(),
                        reason: SkipReason::Configuration {
                            message: ListError::Config(error).to_string(),
                        },
                    });
                }
            }
        }
    }
    Ok(SampleList {
        declared: Vec::new(),
        forced: !nearest,
        root: Some(found.root),
        entries,
        config: configuration,
        skipped,
        configurations: if nearest {
            found.configurations
        } else {
            Vec::new()
        },
        nearest: loaded,
        within: Vec::new(),
        warnings: if nearest { found.warnings } else { Vec::new() },
    })
}

/// Lists read from several targets, as one: each sample keeps the configuration
/// that describes it, and a sample given twice is kept once.
pub fn merged(lists: Vec<SampleList>) -> SampleList {
    let own = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut root = None;
    let mut first_config = None;
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let mut configurations: Vec<(PathBuf, PathBuf)> = Vec::new();
    let mut nearest: Vec<(PathBuf, ProjectConfig)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for list in lists {
        if root.is_none() {
            root = list.root.clone();
        }
        if first_config.is_none() {
            first_config = list.config.clone();
        }
        for (file, config) in &list.nearest {
            let file = own(file);
            if !nearest.iter().any(|(held, _)| *held == file) {
                nearest.push((file, config.clone()));
            }
        }
        let own_file = list
            .config
            .as_ref()
            .map(|config| own(&config.root().join(config::FILENAME)));
        if list.configurations.is_empty()
            && let (Some(file), Some(config)) = (&own_file, &list.config)
            && !nearest.iter().any(|(held, _)| held == file)
        {
            nearest.push((file.clone(), config.clone()));
        }
        for entry in list.entries {
            if let Some(path) = &entry.path {
                if !seen.insert(own(path)) {
                    continue;
                }
                // A sample no configuration describes is placed under a file
                // that does not exist beside it, as discovery does.
                let file = list
                    .configurations
                    .iter()
                    .find(|(held, _)| held == path)
                    .map(|(_, file)| own(file))
                    .or_else(|| own_file.clone())
                    .unwrap_or_else(|| own(&directory_of(path)).join(config::FILENAME));
                configurations.push((path.clone(), file));
            }
            entries.push(entry);
        }
        skipped.extend(list.skipped);
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    for (_, file) in &configurations {
        if !roots.contains(file) {
            roots.push(file.clone());
        }
    }
    let spans = roots.len() > 1;
    SampleList {
        declared: Vec::new(),
        forced: false,
        root,
        entries,
        config: first_config,
        skipped,
        configurations: if spans { configurations } else { Vec::new() },
        nearest: if spans { nearest } else { Vec::new() },
        within: Vec::new(),
        warnings: if spans {
            vec![Warning::MixedConfigurations { roots }]
        } else {
            Vec::new()
        },
    }
}

pub fn from_paths(paths: &[PathBuf]) -> Result<SampleList, ListError> {
    let mut entries = Vec::with_capacity(paths.len());
    for path in paths {
        let (sample, _) = document::load_sample(path).map_err(|error| ListError::Document {
            path: Some(path.clone()),
            error,
        })?;
        entries.push(Entry {
            path: Some(path.clone()),
            sample: Rc::new(RefCell::new(sample)),
            states: None,
        });
    }
    Ok(SampleList {
        declared: Vec::new(),
        forced: false,
        root: None,
        entries,
        config: None,
        skipped: Vec::new(),
        configurations: Vec::new(),
        nearest: Vec::new(),
        within: Vec::new(),
        warnings: Vec::new(),
    })
}

/// The same files, **with the project found above the first of them**.
///
/// A file addressed on its own must resolve its units, symbols and precision
/// exactly as it does inside its collection, or a single-sample answer differs
/// from the same sample in a table. This is the collection of one that `cli`
/// builds for every file argument.
pub fn from_files(paths: &[PathBuf]) -> Result<SampleList, ListError> {
    // A configuration that will not read is set aside and said, and the files
    // are read without it, as a directory's is.
    let mut unread = None;
    let configuration = match paths.first() {
        Some(first) => match config::load_for(&directory_of(first)) {
            Ok(configuration) => configuration,
            Err(error) => {
                unread = config::find(&directory_of(first)).map(|file| Skipped {
                    path: file,
                    reason: SkipReason::Configuration {
                        message: ListError::Config(error).to_string(),
                    },
                });
                None
            }
        },
        None => None,
    };
    let mut list = from_files_with(paths, configuration)?;
    // The nearest configuration, found rather than given.
    list.forced = false;
    list.skipped.extend(unread);
    Ok(list)
}

/// The same files, with a configuration given instead of the one above them.
pub fn from_files_with(
    paths: &[PathBuf],
    configuration: Option<ProjectConfig>,
) -> Result<SampleList, ListError> {
    let mut list = from_paths(paths)?;
    let Some(first) = paths.first() else {
        return Ok(list);
    };
    let _ = first;
    list.config = configuration;
    list.forced = true;
    // **A list of named files has no root.** It is not a directory, and giving
    // it the configuration's own made every check that asks *am I looking at
    // the whole collection?* answer yes: `validate a.md b.md c.md` reported the
    // configuration's declarations as defects of those three files, where the
    // same command on the directory reports none. `--rc` still says "this
    // configuration describes every sample read" through `forced`.
    list.root = None;
    Ok(list)
}

/// Whether a directory is the other or lies under it.
fn within(inner: &Path, outer: &Path) -> bool {
    let own = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    own(inner).starts_with(own(outer))
}

fn directory_of(file: &Path) -> PathBuf {
    file.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Samples built in memory, which have no path until they are saved.
pub fn from_samples(samples: Vec<Sample>) -> SampleList {
    SampleList {
        declared: Vec::new(),
        forced: false,
        root: None,
        entries: samples
            .into_iter()
            .map(|sample| Entry {
                path: None,
                sample: Rc::new(RefCell::new(sample)),
                states: None,
            })
            .collect(),
        config: None,
        skipped: Vec::new(),
        configurations: Vec::new(),
        nearest: Vec::new(),
        within: Vec::new(),
        warnings: Vec::new(),
    }
}

/// Entries a caller already holds, in the order given: nothing is loaded and
/// nothing is copied, so the list and the caller hold the same samples.
pub fn from_entries(entries: Vec<Entry>) -> SampleList {
    SampleList {
        declared: Vec::new(),
        forced: false,
        root: None,
        entries,
        config: None,
        skipped: Vec::new(),
        configurations: Vec::new(),
        nearest: Vec::new(),
        within: Vec::new(),
        warnings: Vec::new(),
    }
}

pub fn empty() -> SampleList {
    SampleList {
        declared: Vec::new(),
        forced: false,
        root: None,
        entries: Vec::new(),
        config: None,
        skipped: Vec::new(),
        configurations: Vec::new(),
        nearest: Vec::new(),
        within: Vec::new(),
        warnings: Vec::new(),
    }
}

// ----------------------------------------------------- selection and order

impl SampleList {
    /// A **shallow** collection: the samples are shared, not copied.
    /// Correcting a value in a filtered subset corrects it in the parent,
    /// which is what a user means by *the approved samples*.
    pub fn filter(&self, predicate: &Filter) -> Result<SampleList, ListError> {
        let vocabulary = self.vocabulary();
        let mut kept = Vec::new();
        for entry in &self.entries {
            let sample = entry.sample.borrow();
            let subject = Subject {
                sample: &sample,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            };
            // A sample whose field holds text where a comparison asks for a
            // number is left aside, and named by whoever checked the filter :
            // one such sample refused the whole filter.
            if filter::evaluate_leaving_aside(predicate, &subject)
                .map_err(ListError::Filter)?
                .selects()
            {
                drop(sample);
                kept.push(entry.clone());
            }
        }
        Ok(self.derive(kept))
    }

    /// The same list, each sample carrying the states read of it, in the list's
    /// order. What was narrowed or sorted from it keeps them; a `state` asked
    /// of a list carrying none is `StatesNotRead`.
    pub fn with_states(&self, states: Vec<States>) -> SampleList {
        let mut entries = self.entries.clone();
        for (entry, states) in entries.iter_mut().zip(states) {
            entry.states = Some(Rc::new(states));
        }
        SampleList {
            root: self.root.clone(),
            entries,
            config: self.config.clone(),
            skipped: self.skipped.clone(),
            configurations: self.configurations.clone(),
            nearest: self.nearest.clone(),
            warnings: self.warnings.clone(),
            forced: self.forced,
            within: self.within.clone(),
            declared: self.declared.clone(),
        }
    }

    /// The same, for a caller holding a closure rather than a parsed filter.
    pub fn filter_by(&self, predicate: impl Fn(&Sample) -> bool) -> SampleList {
        let kept = self
            .entries
            .iter()
            .filter(|entry| predicate(&entry.sample.borrow()))
            .cloned()
            .collect();
        self.derive(kept)
    }

    /// The entries `predicate` keeps, seeing each whole — its path too, which a
    /// field such as `project` reads.
    pub fn filter_entries(&self, predicate: impl Fn(&Entry) -> bool) -> SampleList {
        let kept = self
            .entries
            .iter()
            .filter(|entry| predicate(entry))
            .cloned()
            .collect();
        self.derive(kept)
    }

    /// Reorders in place. [`ordering`] returns a permutation and this applies
    /// it: that module computes, this one owns.
    pub fn sort(&mut self, spec: &SortSpec) -> Result<(), ListError> {
        let order = self.permutation(spec)?;
        // Handles move, samples do not: a sort does not copy, so an edit after
        // one is still visible through the parent.
        let mut reordered: Vec<Entry> = Vec::with_capacity(order.len());
        for at in order {
            reordered.push(self.entries[at].clone());
        }
        self.entries = reordered;
        Ok(())
    }

    pub fn sorted(&self, spec: &SortSpec) -> Result<SampleList, ListError> {
        let order = self.permutation(spec)?;
        let kept = order
            .into_iter()
            .map(|at| self.entries[at].clone())
            .collect();
        Ok(self.derive(kept))
    }

    /// A failed sort leaves the order unchanged, because the permutation is
    /// computed in full before a single handle moves.
    fn permutation(&self, spec: &SortSpec) -> Result<Vec<usize>, ListError> {
        let vocabulary = self.vocabulary();
        let borrowed: Vec<_> = self.entries.iter().map(|e| e.sample.borrow()).collect();
        let subjects: Vec<Subject> = borrowed
            .iter()
            .zip(&self.entries)
            .map(|(sample, entry)| Subject {
                sample,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            })
            .collect();
        ordering::sorted(&subjects, spec).map_err(ListError::Order)
    }

    fn derive(&self, entries: Vec<Entry>) -> SampleList {
        SampleList {
            declared: {
                let mut declared = self.declared.clone();
                declared.extend(self.declared_by_models());
                declared
            },
            forced: self.forced,
            root: self.root.clone(),
            entries,
            // A derived collection keeps no configuration: a subset is not a
            // project, and the one place a configuration is needed is where a
            // named query is run, which holds its own.
            config: None,
            skipped: Vec::new(),
            configurations: Vec::new(),
            nearest: Vec::new(),
            // What a narrowing was made from, so that *what exists* stays the
            // collection's answer.
            within: if self.within.is_empty() {
                self.entries.clone()
            } else {
                self.within.clone()
            },
            warnings: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- reading

impl SampleList {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, position: usize) -> Option<&Entry> {
        self.entries.get(position)
    }

    pub fn by_path(&self, path: &Path) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|entry| entry.path.as_deref() == Some(path))
    }

    pub fn iter(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter()
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn config(&self) -> Option<&ProjectConfig> {
        self.config.as_ref()
    }

    /// What discovery examined and did not include. Retained so that
    /// *why is my sample missing* has an answer.
    /// The configuration a sample was found under, when the list spans several.
    pub fn configuration_of(&self, path: &Path) -> Option<&Path> {
        self.configurations
            .iter()
            .find(|(held, _)| held == path)
            .map(|(_, configuration)| configuration.as_path())
    }

    /// Whether the configuration was given instead of the nearest one, and so
    /// describes every sample read.
    pub fn configuration_forced(&self) -> bool {
        self.forced
    }

    /// Whether several configurations describe the samples of this list.
    pub fn spans_configurations(&self) -> bool {
        !self.configurations.is_empty()
    }

    /// The configuration each sample answers to, given by a caller that holds
    /// its samples already — Python's — in the entries' own order, which a
    /// merge by project would not keep. Several configurations make the list
    /// span them, as one read from several targets does.
    pub fn with_configurations(mut self, held: Vec<(PathBuf, ProjectConfig)>) -> SampleList {
        let mut roots: Vec<PathBuf> = Vec::new();
        for (path, config) in held {
            let file = config.root().join(config::FILENAME);
            if !roots.contains(&file) {
                roots.push(file.clone());
            }
            if !self.nearest.iter().any(|(known, _)| *known == file) {
                self.nearest.push((file.clone(), config));
            }
            self.configurations.push((path, file));
        }
        if roots.len() < 2 {
            self.configurations.clear();
            self.nearest.clear();
        }
        self
    }

    /// The configuration describing a sample: the nearest above it, the same
    /// directory first, and none when there is none.
    pub fn config_for(&self, path: &Path) -> Option<&ProjectConfig> {
        match self.configuration_of(path) {
            None => self.config.as_ref(),
            Some(file) => self
                .nearest
                .iter()
                .find(|(held, _)| held == file)
                .map(|(_, config)| config),
        }
    }

    /// A selection from this list by the configuration describing each sample,
    /// in the order the samples come: one part when one configuration describes
    /// them all. Each part is a list carrying its configuration, so whatever
    /// reads a list's configuration reads the right one.
    pub fn by_configuration(&self, selection: &SampleList) -> Vec<ConfigurationPart<'_>> {
        let mut parts: Vec<ConfigurationPart<'_>> = Vec::new();
        for entry in selection.iter() {
            let file = entry
                .path
                .as_deref()
                .and_then(|path| self.configuration_of(path))
                .map(Path::to_path_buf);
            let at = match parts.iter().position(|part| part.file == file) {
                Some(at) => at,
                None => {
                    let config = entry
                        .path
                        .as_deref()
                        .map_or(self.config.as_ref(), |path| self.config_for(path));
                    // A configuration wholly inside the list answers for its own
                    // directory; one above it, for a part of its samples only.
                    let root = match (config, self.root.as_deref()) {
                        (Some(config), Some(root)) if within(config.root(), root) => {
                            Some(config.root().to_path_buf())
                        }
                        _ => self.root.clone(),
                    };
                    parts.push(ConfigurationPart {
                        file,
                        config,
                        samples: SampleList {
                            declared: Vec::new(),
                            forced: self.forced,
                            root,
                            entries: Vec::new(),
                            config: config.cloned(),
                            skipped: Vec::new(),
                            configurations: Vec::new(),
                            nearest: Vec::new(),
                            within: Vec::new(),
                            warnings: Vec::new(),
                        },
                    });
                    parts.len() - 1
                }
            };
            parts[at].samples.entries.push(entry.clone());
        }
        parts
    }

    /// What discovery had to say about the directory: a list spanning several
    /// configurations.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    pub fn skipped(&self) -> &[Skipped] {
        &self.skipped
    }

    /// Everything the collection can name, built once and shared — which is
    /// what makes the absent-versus-unknown check affordable per sample. Every
    /// name a sample holds — of this list, and of the list it was narrowed
    /// from. Read from the samples each time, which are shared: a property
    /// added to one since is a name that exists.
    pub fn vocabulary(&self) -> Vocabulary {
        let borrowed: Vec<_> = self
            .entries
            .iter()
            .chain(self.within.iter())
            .map(|e| e.sample.borrow())
            .collect();
        let samples: Vec<&Sample> = borrowed.iter().map(|s| &**s).collect();
        fields::vocabulary_of(&samples)
            .with_names(self.declared.iter().cloned())
            .with_names(self.declared_by_models())
    }

    /// The properties the models describing this list declare, read from
    /// their source — nothing runs. Their tables and columns are not among
    /// them: a column named for a table is the table's.
    fn declared_by_models(&self) -> Vec<Identifier> {
        self.config
            .iter()
            .chain(self.nearest.iter().map(|(_, config)| config))
            .flat_map(crate::config::model_runtime::declared_properties)
            .filter_map(|name| Identifier::new(&name).ok())
            .collect()
    }

    pub fn available_fields(&self) -> Vec<Field> {
        let borrowed: Vec<_> = self.entries.iter().map(|e| e.sample.borrow()).collect();
        let samples: Vec<&Sample> = borrowed.iter().map(|s| &**s).collect();
        fields::available_in(&samples)
    }

    /// One entry per sample, in collection order, with `None` for absent.
    pub fn values(&self, field: &Field) -> Result<Vec<Option<Value>>, ListError> {
        let vocabulary = self.vocabulary();
        let mut values = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let sample = entry.sample.borrow();
            let subject = Subject {
                sample: &sample,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            };
            match fields::resolve(field, &subject) {
                Ok(Resolution::Scalar(value)) => values.push(value),
                Ok(Resolution::Tags(_)) => values.push(None),
                Ok(Resolution::List(_)) => {
                    return Err(ListError::NonScalar {
                        field: fields::describe(field),
                    });
                }
                Err(FieldError::Compute { field, reason }) => {
                    return Err(ListError::Compute {
                        sample: entry.path.clone(),
                        field,
                        reason,
                    });
                }
                Err(error) => return Err(ListError::Field(error)),
            }
        }
        Ok(values)
    }

    /// Absent values and failed ones are ignored, and `ColumnSummary` carries
    /// both counts:
    /// `summary.count` is how many were used, `considered` is how many rows
    /// the selection held. Every renderer shows them together — **58 / 70**.
    pub fn summarize(&self, field: &Field) -> Result<Option<ColumnSummary>, ListError> {
        let values = self.values(field)?;
        // Each value's uncertainty, where the field is a value that has one:
        // a channel named, `og.u`, has none of its own.
        let uncertainties: Option<Vec<Option<Value>>> = match field {
            Field::Named {
                channel: fields::Channel::Value,
                ..
            }
            | Field::Cell {
                channel: fields::Channel::Value,
                ..
            } => self
                .values(&fields::with_channel(field, fields::Channel::Uncertainty))
                .ok(),
            _ => None,
        };
        // A value whose formula failed is the last one it gave, kept and marked.
        // It leaves an export for that reason, and it leaves a summary for the
        // same one: a mean, a minimum or a deviation computed over a number
        // nothing produced is a confident wrong answer — and the summary is
        // where it is least visible, since no mark can be put on a statistic.
        let measured: Vec<(f64, Option<f64>)> = values
            .iter()
            .enumerate()
            .zip(self.entries.iter())
            .filter(|(_, entry)| !failed_value(field, &entry.sample.borrow()))
            .filter_map(|((at, value), _)| {
                let number = match value {
                    Some(Value::Number(number)) => *number,
                    Some(Value::Integer(integer)) => *integer as f64,
                    _ => return None,
                };
                let uncertainty = uncertainties
                    .as_ref()
                    .and_then(|all| all.get(at).cloned().flatten())
                    .and_then(|held| match held {
                        Value::Number(number) => Some(number),
                        Value::Integer(integer) => Some(integer as f64),
                        _ => None,
                    });
                Some((number, uncertainty))
            })
            .collect();
        let numbers: Vec<f64> = measured.iter().map(|(number, _)| *number).collect();
        // Weighted by 1/u² only where every value has an uncertainty above
        // zero: one without would weigh infinitely, or not at all.
        let weighted_mean = measured
            .iter()
            .map(|(number, uncertainty)| {
                uncertainty
                    .filter(|u| u.is_finite() && *u > 0.0)
                    .map(|u| (*number, 1.0 / (u * u)))
            })
            .collect::<Option<Vec<(f64, f64)>>>()
            .filter(|weighted| !weighted.is_empty())
            .map(|weighted| {
                let total: f64 = weighted.iter().map(|(_, weight)| weight).sum();
                weighted
                    .iter()
                    .map(|(number, weight)| number * weight)
                    .sum::<f64>()
                    / total
            });
        // `None` for a column with no numeric values is distinct from a
        // summary of count zero, which cannot occur.
        let not_applicable = values
            .iter()
            .filter(|value| value.as_ref().is_some_and(Value::is_not_applicable))
            .count();
        Ok(
            statistics::summarize_slice(&numbers).map(|summary| ColumnSummary {
                summary,
                considered: values.len() - not_applicable,
                not_applicable,
                weighted_mean,
            }),
        )
    }
}

/// Whether a field reads a number of a value whose formula failed: in a session
/// its cache says so, and in a file read without its model, its record.
///
/// The same criterion `exports` applies to keep a failed value out of a file.
/// It is copied rather than shared because that module builds on this one, and
/// a dependency the other way would be a cycle for eleven lines.
fn failed_value(field: &Field, sample: &Sample) -> bool {
    let Field::Named { name, channel } = field else {
        return false;
    };
    if matches!(channel, fields::Channel::Unit | fields::Channel::Symbol) {
        return false;
    }
    sample.property(name).is_ok_and(|handle| {
        handle.peek(|property| {
            if property.is_computed() || property.has_uncertainty_formula() {
                property.has_failed()
            } else {
                property.records().failure.is_some()
            }
        })
    })
}

// --------------------------------------------------------------- grouping

/// One group of [`SampleList::group_by`]: the value its samples hold of each
/// field, in the fields' order, and those samples, in the list's order.
pub struct Group {
    pub key: Vec<Value>,
    pub samples: SampleList,
}

/// The order groups come in. A list does not know whether it was sorted — a
/// filter keeps an order, a directory has one — so the caller, which knows
/// whether a sort is in force, says which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupOrder {
    /// By their values, as `ordering` sorts them: no sort is in force.
    Values,
    /// Where their first samples come in the list: a sort is in force, and
    /// the groups follow it — sorted by `-og` and grouped by `og`, the
    /// highest first.
    Listed,
}

impl SampleList {
    /// The list split by the values of `fields`: a group per combination of
    /// values some sample holds, in the order the values sort. Every surface
    /// groups through here, so that a table, a summary, a Python dict and the
    /// workbench's headings agree on what a group is and in which order the
    /// groups come.
    pub fn group_by(&self, fields: &[Field], order: GroupOrder) -> Result<Vec<Group>, ListError> {
        let vocabulary = self.vocabulary();
        let mut keyed: Vec<(Vec<Value>, Entry)> = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let sample = entry.sample.borrow();
            let subject = Subject {
                sample: &sample,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            };
            let mut key = Vec::with_capacity(fields.len());
            for field in fields {
                key.push(group_value(field, &subject, entry)?);
            }
            drop(sample);
            keyed.push((key, entry.clone()));
        }
        // Listed, each group where its first sample is: the sort in force
        // decides, and a group's samples keep their order within it.
        if order == GroupOrder::Listed {
            let mut groups: Vec<(Vec<Value>, Vec<Entry>)> = Vec::new();
            for (key, entry) in keyed {
                match groups
                    .iter_mut()
                    .find(|(held, _)| compare_keys(held, &key) == std::cmp::Ordering::Equal)
                {
                    Some((_, members)) => members.push(entry),
                    None => groups.push((key, vec![entry])),
                }
            }
            return Ok(groups
                .into_iter()
                .map(|(key, members)| Group {
                    key,
                    samples: self.derive(members),
                })
                .collect());
        }
        // A stable sort: within a group the samples keep the list's order,
        // so a list sorted first is sorted within each group.
        keyed.sort_by(|(left, _), (right, _)| compare_keys(left, right));
        let mut groups: Vec<Group> = Vec::new();
        let mut members: Vec<Entry> = Vec::new();
        let mut current: Option<Vec<Value>> = None;
        for (key, entry) in keyed {
            if current
                .as_ref()
                .is_some_and(|held| compare_keys(held, &key) != std::cmp::Ordering::Equal)
            {
                groups.push(Group {
                    key: current.take().unwrap_or_default(),
                    samples: self.derive(std::mem::take(&mut members)),
                });
            }
            if current.is_none() {
                current = Some(key);
            }
            members.push(entry);
        }
        if let Some(key) = current {
            groups.push(Group {
                key,
                samples: self.derive(members),
            });
        }
        Ok(groups)
    }

    /// The same samples, each group's together, the groups in their order:
    /// what a data format writes when a table is grouped.
    pub fn grouped(&self, fields: &[Field], order: GroupOrder) -> Result<SampleList, ListError> {
        let entries = self
            .group_by(fields, order)?
            .into_iter()
            .flat_map(|group| group.samples.entries)
            .collect();
        Ok(self.derive(entries))
    }
}

/// A sample's value of one field as a group holds it: a tag set or a list
/// joined into one text, so that samples carrying the same tags share a
/// group; `Absent` for none, an empty set or list among them.
fn group_value(field: &Field, subject: &Subject, entry: &Entry) -> Result<Value, ListError> {
    let joined = |items: Vec<String>| {
        if items.is_empty() {
            Value::Absent
        } else {
            Value::text(items.join(", "))
        }
    };
    match fields::resolve(field, subject) {
        Ok(Resolution::Scalar(value)) => Ok(value.unwrap_or(Value::Absent)),
        Ok(Resolution::List(values)) => Ok(joined(values.iter().map(item_text).collect())),
        Ok(Resolution::Tags(tags)) => Ok(joined(tags.iter().map(ToString::to_string).collect())),
        Err(FieldError::Compute { field, reason }) => Err(ListError::Compute {
            sample: entry.path.clone(),
            field,
            reason,
        }),
        Err(error) => Err(ListError::Field(error)),
    }
}

/// A list's item as its joined text writes it.
fn item_text(value: &Value) -> String {
    match value {
        Value::Integer(whole) => whole.to_string(),
        Value::Number(real) => format!("{real}"),
        Value::Text(text) => text.clone(),
        Value::Boolean(yes) => yes.to_string(),
        Value::Date(date) => date.to_string(),
        Value::DateTime(stamp) => stamp.to_string(),
        Value::Absent => String::new(),
        Value::NotApplicable => "n/a".to_string(),
    }
}

/// Two groups' keys, field by field: numbers as numbers, then text ignoring
/// case — the written case breaking a tie — as `ordering` sorts it, then
/// yes-or-no and dates; the samples holding none after every value, and
/// those it does not apply to last, as a figure's legend reads them.
fn compare_keys(left: &[Value], right: &[Value]) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank = |value: &Value| match value {
        Value::NotApplicable => 2,
        Value::Absent => 1,
        _ => 0,
    };
    for (a, b) in left.iter().zip(right) {
        let ordering = rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
            (Value::Text(x), Value::Text(y)) => x
                .to_lowercase()
                .cmp(&y.to_lowercase())
                .then_with(|| x.cmp(y)),
            _ => crate::core::value::total_order(a, b),
        });
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

// ----------------------------------------------------------------- saving

impl SampleList {
    /// Writes the whole batch into a directory. **Every destination is checked
    /// before anything is written**: a batch of two hundred either runs or
    /// does not, because failing on the hundred-and-first leaves a directory
    /// half old and half new — the state hardest to recover from, because it
    /// looks complete.
    pub fn save_all(
        &mut self,
        directory: &Path,
        overwrite: bool,
    ) -> Result<Vec<PathBuf>, ListError> {
        let mut destinations = Vec::with_capacity(self.entries.len());
        for (position, entry) in self.entries.iter().enumerate() {
            let name = match &entry.path {
                Some(path) => path
                    .file_name()
                    .map(PathBuf::from)
                    .ok_or(ListError::MissingPath { position })?,
                // An in-memory sample is named by its own `name`, or refuses:
                // inventing `sample-3.md` would produce a file nobody asked for.
                None => {
                    let sample = entry.sample.borrow();
                    let named = sample.name().ok_or(ListError::MissingPath { position })?;
                    PathBuf::from(format!("{named}.md"))
                }
            };
            destinations.push(directory.join(name));
        }
        // Two samples of one file name, from two folders, would be one file:
        // the second written over the first, and a batch said written whole
        // missing a sample.
        let mut shared: Vec<(PathBuf, Vec<PathBuf>)> = Vec::new();
        for (position, destination) in destinations.iter().enumerate() {
            if destinations[..position].contains(destination)
                || shared.iter().any(|(held, _)| held == destination)
            {
                continue;
            }
            let sources: Vec<PathBuf> = destinations
                .iter()
                .zip(&self.entries)
                .filter(|(other, _)| *other == destination)
                .map(|(_, entry)| {
                    entry.path.clone().unwrap_or_else(|| {
                        PathBuf::from(entry.sample.borrow().name().unwrap_or("?"))
                    })
                })
                .collect();
            if sources.len() > 1 {
                shared.push((destination.clone(), sources));
            }
        }
        if !shared.is_empty() {
            return Err(ListError::SameDestination { shared });
        }
        if !overwrite {
            let conflicts: Vec<PathBuf> = destinations
                .iter()
                .filter(|path| path.exists())
                .cloned()
                .collect();
            // Every conflict, not the first: clearing them one run at a time
            // is the same defect as one-error-at-a-time filter diagnostics.
            if !conflicts.is_empty() {
                return Err(ListError::AlreadyExists { paths: conflicts });
            }
        }
        let mut written = Vec::with_capacity(destinations.len());
        for (entry, destination) in self.entries.iter().zip(destinations) {
            // One borrow at a time: the `RefCell` is a shared handle, and
            // holding two is this module's bug rather than the type's.
            let mut sample = entry.sample.borrow_mut();
            // Checked above as absent: one made since is another writer's,
            // unless replacing was asked for.
            let to = if overwrite {
                Destination::Path(destination.clone())
            } else {
                Destination::New(destination.clone())
            };
            document::save_sample(&mut sample, &to).map_err(|error| ListError::Document {
                path: Some(destination.clone()),
                error,
            })?;
            written.push(destination);
        }
        Ok(written)
    }

    /// Writes each sample back where it came from. Requires every entry to
    /// have a path, and says which position does not.
    pub fn save_each(&mut self) -> Result<Vec<PathBuf>, ListError> {
        for (position, entry) in self.entries.iter().enumerate() {
            if entry.path.is_none() {
                return Err(ListError::MissingPath { position });
            }
        }
        let mut written = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let path = entry.path.clone().expect("checked above");
            let mut sample = entry.sample.borrow_mut();
            document::save_sample(&mut sample, &Destination::Path(path.clone())).map_err(
                |error| ListError::Document {
                    path: Some(path.clone()),
                    error,
                },
            )?;
            written.push(path);
        }
        Ok(written)
    }
}

// ------------------------------------------------- checking a declaration

/// A figure's axes and group, checked as a profile's columns are: a whole
/// column of a table is an axis of its own (a curve per sample), and `tags`
/// a group.
pub fn check_figure(
    list: &SampleList,
    figure: &crate::config::project_config::FigureDeclaration,
) -> Vec<FieldWarning> {
    let vocabulary = list.vocabulary();
    let column = |field: &str| {
        field.split_once('.').is_some_and(|(table, column)| {
            Identifier::new(table).is_ok_and(|table| {
                Identifier::new(column).is_ok_and(|column| vocabulary.has_column(&table, &column))
            })
        })
    };
    let axes = [figure.x.as_str(), figure.y.as_str()]
        .into_iter()
        .filter(|field| !column(field));
    // Several fields, `style,yeast`, each checked.
    let group = figure
        .group
        .as_deref()
        .into_iter()
        .flat_map(|group| group.split(','))
        .map(str::trim)
        .filter(|group| *group != "tags");
    check_fields(list, axes.chain(group))
}

/// Its columns, and the fields it groups by, each checked the same.
pub fn check_profile(list: &SampleList, profile: &Profile) -> Vec<FieldWarning> {
    check_fields(
        list,
        profile
            .columns
            .iter()
            .map(|column| column.field.as_str())
            .chain(profile.group.iter().map(String::as_str)),
    )
}

fn check_fields<'a>(
    list: &SampleList,
    declared: impl Iterator<Item = &'a str>,
) -> Vec<FieldWarning> {
    let vocabulary = list.vocabulary();
    let known: Vec<String> = list
        .available_fields()
        .iter()
        .map(fields::describe)
        .collect();
    let mut warnings = Vec::new();
    for field in declared {
        if let Some(table) = field.strip_suffix('.')
            && let Ok(table) = Identifier::new(table)
            && vocabulary.has_table(&table)
        {
            warnings.push(FieldWarning::TableNeedsCell {
                table: table.to_string(),
            });
            continue;
        }
        let Ok(parsed) = fields::parse(field) else {
            warnings.push(FieldWarning::Unknown {
                suggestion: Identifier::new(field).ok().and_then(|_| {
                    crate::core::identifier::nearest(field, known.iter().map(String::as_str))
                }),
                field: field.to_string(),
            });
            continue;
        };
        let named = match &parsed {
            Field::Named { name, .. } => Some(name.clone()),
            _ => None,
        };
        if let Some(name) = &named
            && vocabulary.has_table(name)
            && !vocabulary.has_name(name)
        {
            warnings.push(FieldWarning::TableNeedsCell {
                table: name.to_string(),
            });
            continue;
        }
        // Only a name the *collection* lacks is a warning. A field absent from
        // one sample is data.
        if let Some(name) = named
            && !vocabulary.has_name(&name)
        {
            warnings.push(FieldWarning::Unknown {
                suggestion: crate::core::identifier::nearest(
                    name.as_str(),
                    known.iter().map(String::as_str),
                ),
                field: field.to_string(),
            });
        }
        // A cell is checked as far as the collection can answer: its table and
        // column are vocabulary, and a row that no sample holds is a mistake
        // rather than data, or `-c 'conditioning.carbonation[51]'` is a column of dashes.
        if let Field::Cell {
            table, column, row, ..
        } = &parsed
        {
            if !vocabulary.has_table(table) {
                let tables = vocabulary.tables_as_strings();
                warnings.push(FieldWarning::Unknown {
                    suggestion: crate::core::identifier::nearest(
                        table.as_str(),
                        tables.iter().map(String::as_str),
                    )
                    .and_then(|nearest| {
                        field
                            .strip_prefix(table.as_str())
                            .map(|rest| format!("{nearest}{rest}"))
                    }),
                    field: field.to_string(),
                });
            } else if !vocabulary.has_column(table, column) {
                let columns = vocabulary.columns_as_strings(table);
                warnings.push(FieldWarning::Unknown {
                    suggestion: crate::core::identifier::nearest(
                        column.as_str(),
                        columns.iter().map(String::as_str),
                    )
                    .and_then(|nearest| {
                        field
                            .strip_prefix(format!("{table}.{column}").as_str())
                            .map(|rest| format!("{table}.{nearest}{rest}"))
                    }),
                    field: field.to_string(),
                });
            } else if !list.iter().any(|entry| {
                entry
                    .sample
                    .borrow()
                    .table(table)
                    .is_ok_and(|held| held.row(row).is_ok())
            }) {
                warnings.push(FieldWarning::NoRow {
                    field: field.to_string(),
                    table: table.to_string(),
                });
            }
        }
        if let Field::ListItem { .. } = &parsed {
            let borrowed: Vec<_> = list.iter().map(|entry| entry.sample.borrow()).collect();
            let samples: Vec<&crate::core::sample::Sample> =
                borrowed.iter().map(|sample| &**sample).collect();
            if let Some(crate::query::field_addressing::FieldError::ListIndexOutOfRange {
                name,
                ..
            }) = fields::held_nowhere(&parsed, &samples)
            {
                warnings.push(FieldWarning::NoItem {
                    field: field.to_string(),
                    name: name.to_string(),
                });
            }
        }
    }
    warnings
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum ListError {
    NonScalar {
        field: String,
    },
    Discovery(DiscoveryError),
    /// **With the file it happened to.** One failure among two hundred is
    /// otherwise a search, which is the same reasoning as `Compute`.
    Document {
        path: Option<PathBuf>,
        error: DocumentError,
    },
    Config(ConfigError),
    Filter(FilterError),
    Order(OrderError),
    Field(FieldError),
    /// A formula failed, naming the sample: one failing formula among two
    /// hundred is otherwise a search.
    Compute {
        sample: Option<PathBuf>,
        field: String,
        reason: String,
    },
    AlreadyExists {
        paths: Vec<PathBuf>,
    },
    /// Samples that would be written to one file in a directory: each
    /// destination with the samples it would hold.
    SameDestination {
        shared: Vec<(PathBuf, Vec<PathBuf>)>,
    },
    MissingPath {
        position: usize,
    },
}

impl fmt::Display for ListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListError::NonScalar { field } => write!(
                f,
                "'{field}' is a list, not one value; address an item with [#n]"
            ),
            ListError::Discovery(error) => write!(f, "{error}"),
            // An I/O failure already names the file it happened to, so naming
            // it again gives `RO.md: RO.md: Permission denied`.
            ListError::Document {
                path: Some(_),
                error: error @ DocumentError::Io { .. },
            } => write!(f, "{error}"),
            ListError::Document {
                path: Some(path),
                error,
            } => write!(f, "{}: {error}", path.display()),
            ListError::Document { path: None, error } => write!(f, "{error}"),
            ListError::Config(error) => write!(f, "{error}"),
            ListError::Filter(error) => write!(f, "{error}"),
            ListError::Order(error) => write!(f, "{error}"),
            ListError::Field(error) => write!(f, "{error}"),
            ListError::Compute {
                sample,
                field,
                reason,
            } => match sample {
                Some(path) => write!(
                    f,
                    "reading '{field}' on {} ran a formula that failed: {reason}",
                    path.display()
                ),
                None => write!(
                    f,
                    "reading '{field}' on a sample with no path ran a formula \
                     that failed: {reason}"
                ),
            },
            ListError::AlreadyExists { paths } => {
                writeln!(f, "{} destination(s) already exist:", paths.len())?;
                for path in paths {
                    writeln!(f, "  {}", path.display())?;
                }
                write!(f, "nothing was written: overwrite=True replaces them")
            }
            ListError::SameDestination { shared } => {
                for (destination, sources) in shared {
                    writeln!(
                        f,
                        "{} samples would be written to {}:",
                        sources.len(),
                        destination.display()
                    )?;
                    for source in sources {
                        writeln!(f, "  {}", source.display())?;
                    }
                }
                write!(
                    f,
                    "nothing was written: a file name holds one sample — save them to \
                     separate directories, or rename one"
                )
            }
            ListError::MissingPath { position } => write!(
                f,
                "the sample at position {position} has never been written and \
                 has no destination: save the collection to a directory instead"
            ),
        }
    }
}

impl std::error::Error for ListError {}

impl fmt::Debug for SampleList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SampleList")
            .field("root", &self.root)
            .field("entries", &self.entries.len())
            .field("configured", &self.config.is_some())
            .field("skipped", &self.skipped.len())
            .finish()
    }
}
