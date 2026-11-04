//! What a computation over a selection runs: the samples grouped by the
//! configuration that names their model, the request for each, the samples left
//! out and why, and the trace of which formula computed what — the part of
//! `samplekit compute` the workbench and Python run too. The worker's events are model
//! runtime's; how they are shown is each surface's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::collection::sample_list::SampleList;
use crate::config::model_runtime::{self as runtime, Report, Request, Template};
use crate::config::project_config::{self, ConfigError, ProjectConfig};
use crate::config::version_control::{self, Fact, HeldRecord, Trace, VcsError};
use crate::core::sample::Sample;
use crate::core::value::{Value, ValueKind};
use crate::format::fingerprint::{self, Freshness};

/// Samples a configuration describes, by their position in the selection.
#[derive(Debug, Clone)]
pub struct Group {
    pub file: Option<PathBuf>,
    pub config: Option<ProjectConfig>,
    pub entries: Vec<usize>,
}

/// What a run is asked to compute beside what is not current.
#[derive(Debug, Clone, Default)]
pub struct Asked {
    /// Named values only; every value a formula gives when empty.
    pub names: Vec<String>,
    /// Current values too.
    pub rerun: bool,
    /// Overrides too.
    pub force: bool,
    /// How long one formula may run before it is stopped and failed.
    pub timeout: Option<std::time::Duration>,
}

/// The requests of one group, and the samples left out of it.
#[derive(Debug, Clone, Default)]
pub struct Requests {
    pub requests: Vec<Request>,
    /// A sample whose records cannot be followed has no order to run in, and is
    /// left out and said.
    pub unplannable: Vec<(PathBuf, String)>,
    /// The records each sample held before the run, by its path: what a write
    /// is told apart by, a record new since being one its run computed.
    pub before: HashMap<PathBuf, Vec<HeldRecord>>,
    /// The project's trace could not be read: said by the surface, and the
    /// run judges its values as a project with no trace.
    pub unread_trace: Option<String>,
}

/// The selection's samples grouped by the configuration that describes each —
/// `forced`, given with `--rc`, describing them all.
pub fn groups_of(
    selected: &SampleList,
    forced: Option<&(PathBuf, ProjectConfig)>,
) -> Result<Vec<Group>, ConfigError> {
    let mut loaded: HashMap<PathBuf, ProjectConfig> = HashMap::new();
    let mut groups: Vec<Group> = Vec::new();
    for (position, entry) in selected.iter().enumerate() {
        let Some(path) = entry.path.as_deref() else {
            continue;
        };
        let (file, config) = match forced {
            Some((file, config)) => (Some(file.clone()), Some(config.clone())),
            None => match project_config::find(path) {
                None => (None, None),
                Some(file) => {
                    if !loaded.contains_key(&file) {
                        loaded.insert(file.clone(), project_config::load(&file)?);
                    }
                    let config = loaded[&file].clone();
                    (Some(file), Some(config))
                }
            },
        };
        match groups.iter_mut().find(|group| group.file == file) {
            Some(group) => group.entries.push(position),
            None => groups.push(Group {
                file,
                config,
                entries: vec![position],
            }),
        }
    }
    Ok(groups)
}

/// The values typed over their formulas that a computation not forced keeps,
/// among `names` (every value where none is named): counted by cells, a
/// table's column by its cells edited.
pub fn edited_kept(selected: &SampleList, names: &[String]) -> usize {
    let named = |name: &str| names.is_empty() || names.iter().any(|written| written == name);
    selected
        .iter()
        .map(|entry| {
            let sample = entry.sample.borrow();
            let properties = fingerprint::check(&sample)
                .map(|states| {
                    states
                        .iter()
                        .filter(|(name, state)| {
                            matches!(state, Freshness::Edited) && named(name.as_str())
                        })
                        .count()
                })
                .unwrap_or(0);
            let cells: usize = sample
                .table_names()
                .into_iter()
                .map(|table| {
                    fingerprint::check_table(&sample, table)
                        .map(|columns| {
                            columns
                                .into_iter()
                                .filter(|(column, _)| {
                                    named(&format!("{table}.{column}")) || named(table.as_str())
                                })
                                .map(|(_, rows)| {
                                    rows.iter()
                                        .filter(|(_, state)| matches!(state, Freshness::Edited))
                                        .count()
                                })
                                .sum::<usize>()
                        })
                        .unwrap_or(0)
                })
                .sum();
            properties + cells
        })
        .sum()
}

/// Inputs held as text where the collection holds numbers, by sample: what
/// reads them fails by name, and no formula runs on them.
pub fn text_among_numbers(selected: &SampleList) -> HashMap<PathBuf, Vec<(String, String)>> {
    let held = |sample: &Sample, name: &crate::core::identifier::Identifier| {
        sample
            .property(name)
            .ok()
            .and_then(|handle| handle.peek(|property| property.peek_value()))
    };
    let mut numeric = HashSet::new();
    for entry in selected.iter() {
        let sample = entry.sample.borrow();
        for name in sample.property_names() {
            if held(&sample, name)
                .is_some_and(|value| matches!(value.kind(), ValueKind::Integer | ValueKind::Number))
            {
                numeric.insert(name.to_string());
            }
        }
    }
    let mut refused = HashMap::new();
    for entry in selected.iter() {
        let Some(path) = &entry.path else {
            continue;
        };
        let sample = entry.sample.borrow();
        let texts: Vec<(String, String)> = sample
            .property_names()
            .into_iter()
            .filter(|name| numeric.contains(name.as_str()))
            .filter_map(|name| match held(&sample, name)? {
                Value::Text(text) => Some((name.to_string(), text)),
                _ => None,
            })
            .collect();
        if !texts.is_empty() {
            refused.insert(path.clone(), texts);
        }
    }
    refused
}

/// Why a sample cannot be planned — its records form a cycle — or `None`.
pub fn unplannable(sample: &Sample) -> Option<String> {
    let standing = fingerprint::check_sample(sample);
    standing
        .properties
        .values()
        .chain(
            standing
                .tables
                .values()
                .flat_map(|columns| columns.values())
                .flat_map(|rows| rows.iter().map(|(_, state)| state)),
        )
        .find_map(|state| match state {
            Freshness::Unjudged { reason } => Some(reason.clone()),
            _ => None,
        })
}

/// The request for each sample of a group that came from a file, under
/// `template`; a sample that cannot be planned is left out, with why.
pub fn requests(
    selected: &SampleList,
    group: &Group,
    template: &Template,
    asked: &Asked,
) -> Requests {
    let refused = text_among_numbers(selected);
    let mut out = Requests::default();
    // What the project's trace knows of the formulas that computed each value,
    // for the worker to tell a changed formula by.
    let trace = match trace_of(group.config.as_ref()) {
        Ok(trace) => trace,
        Err(error) => {
            out.unread_trace = Some(error.to_string());
            None
        }
    };
    for entry in group.entries.iter().filter_map(|at| selected.get(*at)) {
        let Some(path) = entry.path.clone() else {
            continue;
        };
        if let Some(reason) = unplannable(&entry.sample.borrow()) {
            out.unplannable.push((path, reason));
            continue;
        }
        let sample = entry.sample.borrow();
        out.before
            .insert(path.clone(), version_control::records_of_values(&sample));
        out.requests.push(Request {
            refused: refused.get(&path).cloned().unwrap_or_default(),
            overran: Vec::new(),
            recorded: recorded_for(trace.as_deref(), &sample),
            sample: path,
            template: template.clone(),
            names: asked.names.clone(),
            rerun: asked.rerun,
            force: asked.force,
            timeout: asked.timeout,
        });
    }
    out
}

/// The trace of the project `config` describes, read once per command; `None`
/// where no configuration is.
pub fn trace_of(config: Option<&ProjectConfig>) -> Result<Option<Arc<Trace>>, VcsError> {
    match config {
        Some(config) => version_control::trace(config).map(Some),
        None => Ok(None),
    }
}

/// [`trace_of`], a trace that cannot be read said — once, as the history's
/// notices are — rather than read as one that knows no formula: every
/// formula changed was then called current, and nothing said why.
pub fn trace_or_said(config: Option<&ProjectConfig>) -> Option<Arc<Trace>> {
    match trace_of(config) {
        Ok(trace) => trace,
        Err(error) => {
            version_control::notice(format!(
                "the trace of the formulas could not be read, so a formula changed since its \
                 values were computed is not seen: {error}"
            ));
            None
        }
    }
}

/// The formulas known to have computed each of `sample`'s values, by the value
/// each gives: what its project's trace knows of the records it holds, and
/// nothing where that history holds no trace yet.
pub fn recorded_for(trace: Option<&Trace>, sample: &Sample) -> BTreeMap<String, Vec<String>> {
    match trace {
        Some(trace) if !trace.is_empty() => trace.recorded(sample),
        _ => BTreeMap::new(),
    }
}

/// What the writes of one command keep in each project's trace: the formulas
/// they ran under, and a fact for each value they computed and wrote.
#[derive(Debug, Default)]
pub struct Tracing {
    projects: Vec<(ProjectConfig, BTreeMap<String, String>, Vec<Fact>)>,
}

impl Tracing {
    /// Nothing written yet.
    pub const fn new() -> Tracing {
        Tracing {
            projects: Vec::new(),
        }
    }

    /// A sample written at `path` in the project `config` describes: a fact
    /// for each value its run computed — every value it computed whole, and
    /// every record new since `before`, what the run changed — under the
    /// formula the worker imported for it. An override is no formula's.
    pub fn wrote(
        &mut self,
        config: Option<&ProjectConfig>,
        before: &[HeldRecord],
        path: &Path,
        report: &Report,
    ) {
        // A run that wrote nothing keeps no fact, but still names its project:
        // where the history holds no trace yet, the machine's record seeds it.
        let whole: &[String] = if report.saved {
            &report.formulas.computed
        } else {
            &[]
        };
        self.kept(config, before, path, &report.formulas.digests, whole);
    }

    /// A sample saved from Python at `path`: a fact for each record new since
    /// `before` — a value its script computed — under `formulas`, its model's
    /// as the script imported it, and for each value in `computed` — what its
    /// `compute()` ran — whatever record it gave: a formula giving the very
    /// number it gave before is the formula of that record now.
    pub fn saved(
        &mut self,
        config: Option<&ProjectConfig>,
        before: &[HeldRecord],
        path: &Path,
        formulas: &BTreeMap<String, String>,
        computed: &[String],
    ) {
        self.kept(config, before, path, formulas, computed);
    }

    fn kept(
        &mut self,
        config: Option<&ProjectConfig>,
        before: &[HeldRecord],
        path: &Path,
        formulas: &BTreeMap<String, String>,
        whole: &[String],
    ) {
        let Some(config) = config else {
            return;
        };
        if formulas.is_empty() {
            return;
        }
        let Ok((written, _)) = crate::format::document::load_sample(path) else {
            return;
        };
        let held: HashSet<&str> = before.iter().map(|record| record.record.as_str()).collect();
        let whole: HashSet<&str> = whole.iter().map(String::as_str).collect();
        let facts: Vec<Fact> = version_control::records_of_values(&written)
            .into_iter()
            .filter(|record| !record.edited)
            .filter(|record| {
                whole.contains(record.value.as_str()) || !held.contains(record.record.as_str())
            })
            .filter_map(|record| {
                Some(Fact {
                    formula: formulas.get(&record.value)?.clone(),
                    value: record.value,
                    record: record.record,
                })
            })
            .collect();
        // Said in the command's journal as they come: a command killed before
        // it kept them left values with no formula known, called current after
        // their formula changed. The next write keeps them.
        for fact in &facts {
            crate::format::document::to_journals(&format!(
                "{}{}\t{}\t{}",
                version_control::FACT,
                fact.value,
                fact.record,
                fact.formula
            ));
        }
        let root = config.root().to_path_buf();
        match self
            .projects
            .iter_mut()
            .find(|(held, _, _)| held.root() == root.as_path())
        {
            Some((_, known, kept)) => {
                known.extend(formulas.clone());
                kept.extend(facts);
            }
            None => self
                .projects
                .push((config.clone(), formulas.clone(), facts)),
        }
    }

    /// Keeps what was written in each project's trace. Before it, which
    /// formulas changed since the project's last write is noted for the
    /// snapshot of the model's change the command takes; where the project's
    /// history holds no trace yet, what this machine recorded of its other
    /// samples seeds it. The projects whose trace could not be kept, each with
    /// why: a warning, never the write's failure.
    pub fn keep(self) -> Vec<(PathBuf, VcsError)> {
        let mut failures = Vec::new();
        for (config, formulas, mut facts) in self.projects {
            let trace = match version_control::trace(&config) {
                Ok(trace) => trace,
                Err(error) => {
                    failures.push((config.root().to_path_buf(), error));
                    continue;
                }
            };
            if let Some(latest) = trace.latest_formulas() {
                let changed: Vec<String> = formulas
                    .iter()
                    .filter(|(value, formula)| {
                        latest.get(*value).is_some_and(|was| was != *formula)
                            && !trace.knows_formula(formula)
                    })
                    .map(|(value, _)| value.clone())
                    .collect();
                if !changed.is_empty() {
                    version_control::note_model_change(config.root(), &changed);
                }
            }
            // The values a first trace takes as computed, those this command
            // did not compute: said, as a first write by `set` says them.
            let mut seeded = 0;
            if trace.is_empty() {
                let taken: Vec<Fact> = seed(&config, &formulas)
                    .into_iter()
                    .filter(|fact| !facts.contains(fact))
                    .collect();
                seeded = taken.len();
                facts.extend(taken);
            } else if facts.is_empty() {
                continue;
            }
            if let Err(error) = version_control::keep_trace(&config, &formulas, &facts) {
                failures.push((config.root().to_path_buf(), error));
            } else {
                say_seeded(&config, seeded);
            }
        }
        failures
    }
}

/// The first trace of a project: each value its samples hold, not edited,
/// taken as computed by the formula the model gives it now — the baseline a
/// later edit is told apart by. One changed before it cannot be told.
fn seed(config: &ProjectConfig, formulas: &BTreeMap<String, String>) -> Vec<Fact> {
    let Ok(kept) = version_control::kept_files(config) else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for (_, path) in kept {
        if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
            continue;
        }
        let Ok((sample, _)) = crate::format::document::load_sample(&path) else {
            continue;
        };
        facts.extend(
            version_control::records_of_values(&sample)
                .into_iter()
                .filter(|record| !record.edited)
                .filter_map(|record| {
                    Some(Fact {
                        formula: formulas.get(&record.value)?.clone(),
                        value: record.value,
                        record: record.record,
                    })
                }),
        );
    }
    facts
}

/// The values of `selected` a formula computed whose formula the project's
/// trace does not know, by sample: computed on a computer whose history is
/// not here, or before the history kept the trace. Nothing where the history
/// holds no trace yet.
///
/// `without_history`: a project that keeps no history, or kept no formula
/// yet, is counted too — every value it came with — for a check that must say
/// what it cannot vouch for.
pub fn formulas_not_known(
    selected: &SampleList,
    groups: &[Group],
    without_history: bool,
) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut unknown = BTreeMap::new();
    for group in groups {
        if group.config.is_none() {
            continue;
        }
        let trace = trace_or_said(group.config.as_ref());
        for entry in group.entries.iter().filter_map(|at| selected.get(*at)) {
            let Some(path) = &entry.path else {
                continue;
            };
            let sample = entry.sample.borrow();
            let values = match &trace {
                Some(trace) if !trace.is_empty() => trace.unknown(&sample),
                _ if !without_history => continue,
                // No history here, or one that kept no formula yet: a clone
                // without its history knows none of the values it came with.
                _ => Trace::default().unknown(&sample),
            };
            if !values.is_empty() {
                unknown.insert(path.clone(), values);
            }
        }
    }
    unknown
}

/// A history holding no formula yet — a collection moved in, a demo copied,
/// written first by `set` — takes the model as it is now as the one that
/// computed the values its files hold, as a first computation's trace does:
/// a formula changed after it is then said changed. One changed before it
/// cannot be told, and is not.
pub fn seed_first_trace(config: &ProjectConfig) {
    // No history, no trace: the model's worker saves with none, and its
    // command keeps the trace — said once, by it.
    if !version_control::keeps_history(config) {
        return;
    }
    let Ok(trace) = version_control::trace(config) else {
        return;
    };
    if !trace.is_empty() || runtime::template_of(config).is_none() {
        return;
    }
    // The description `set` and `new` read before they write: no Python is
    // started here, a figure written imported no model, and kept none.
    let Some(description) = runtime::current_description(config) else {
        return;
    };
    let mut formulas: BTreeMap<String, String> = description
        .properties
        .iter()
        .filter_map(|(name, property)| Some((name.clone(), property.formula.clone()?)))
        .collect();
    for (table, described) in &description.tables {
        for column in &described.columns {
            if let Some(formula) = &column.formula {
                formulas.insert(format!("{table}.{}", column.name), formula.clone());
            }
        }
    }
    if formulas.is_empty() {
        return;
    }
    let facts = seed(config, &formulas);
    if version_control::keep_trace(config, &formulas, &facts).is_ok() {
        say_seeded(config, facts.len());
    }
}

/// A first trace said where it took values as computed: no record says
/// which formula gave them, and a model changed before the project's first
/// command was never seen changed — its old values called current.
fn say_seeded(config: &ProjectConfig, count: usize) {
    if count == 0 {
        return;
    }
    let folder = std::env::current_dir()
        .ok()
        .and_then(|here| config.root().strip_prefix(here).ok().map(Path::to_path_buf))
        .map_or_else(
            || config.root().display().to_string(),
            |relative| {
                if relative.as_os_str().is_empty() {
                    ".".to_string()
                } else {
                    relative.display().to_string()
                }
            },
        );
    version_control::notice(format!(
        "{count} value{} taken as computed by the model as it is now — samplekit {folder} \
         compute --rerun --write runs {} again",
        if count == 1 { "" } else { "s" },
        if count == 1 { "it" } else { "them" },
    ));
}

/// Before a computation writes: the model's change since the history's last
/// snapshot kept now, naming the formulas whose digest the model's
/// description holds and the trace does not — so that a command writing
/// while it runs does not keep it first, unnamed.
pub fn keep_model_change_ahead(config: &ProjectConfig) {
    // Described again where the model changed since: one Python start, and
    // only then.
    if describe_if_missing(config).is_err() {
        return;
    }
    let Some(description) = runtime::current_description(config) else {
        return;
    };
    let Ok(Some(trace)) = trace_of(Some(config)) else {
        return;
    };
    let Some(latest) = trace.latest_formulas() else {
        return;
    };
    let changed_from = |key: &str, formula: Option<&String>| {
        let (Some(formula), Some(was)) = (formula, latest.get(key)) else {
            return false;
        };
        formula != was && !trace.knows_formula(formula)
    };
    let mut changed: Vec<String> = description
        .properties
        .iter()
        .filter(|(name, property)| changed_from(name, property.formula.as_ref()))
        .map(|(name, _)| name.clone())
        .collect();
    for (table, described) in &description.tables {
        for column in &described.columns {
            let key = format!("{table}.{}", column.name);
            if changed_from(&key, column.formula.as_ref()) {
                changed.push(key);
            }
        }
    }
    let _ = version_control::keep_model_change(config, &changed);
}

/// What the model owes each value of the sample at `path`, as `status` plans
/// it — *formula changed*, *reads …, whose formula changed*, *waits for …*,
/// *never computed* — by value, from the model's description and the
/// project's trace alone: no Python starts. `None` where no model is
/// described, or its description is not current: what only the model knows
/// is then not said, and every surface reads the files alone.
pub fn planned_states(
    config: Option<&ProjectConfig>,
    path: &Path,
    sample: &Sample,
) -> Option<BTreeMap<String, String>> {
    let config = config?;
    let description = runtime::current_description(config)?;
    let trace = trace_or_said(Some(config));
    let recorded = recorded_for(trace.as_deref(), sample);
    let plan = description.plan(path, &recorded).ok()?;
    Some(
        plan.into_iter()
            .map(|planned| (planned.value, planned.said))
            .collect(),
    )
}

/// Has the model of `config` described where its description is missing or
/// stale: the one Python start a surface that says what the model owes makes,
/// before it reads [`planned_states`]. `Err` says why it could not be read;
/// a project with no model, or one described already, is `Ok`.
pub fn describe_if_missing(config: &ProjectConfig) -> Result<(), String> {
    if runtime::template_of(config).is_none() || runtime::current_description(config).is_some() {
        return Ok(());
    }
    runtime::describe(config, config.root())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The same, the model described again where its description is not current
/// — Python started once, as `status` starts it: after an edit to the model,
/// or a pull that brought one, `explain` and a script read the description
/// the edit left behind, and called a value of the old formula current.
pub fn planned_states_described(
    config: Option<&ProjectConfig>,
    path: &Path,
    sample: &Sample,
) -> Option<BTreeMap<String, String>> {
    let config = config?;
    let description = runtime::describe(config, path).ok()?;
    let trace = trace_or_said(Some(config));
    let recorded = recorded_for(trace.as_deref(), sample);
    let plan = description.plan(path, &recorded).ok()?;
    Some(
        plan.into_iter()
            .map(|planned| (planned.value, planned.said))
            .collect(),
    )
}

/// The values of `sample` a formula computed — a `computed` record, not an
/// override — that the model no longer computes: renamed, or its formula
/// removed. Such a value blocks nothing: what reads it takes it as entered,
/// and `status` and `validate` say it with its two ways out. Empty where the
/// project declares no model, or it cannot be described.
pub fn orphans(config: Option<&ProjectConfig>, path: &Path, sample: &Sample) -> Vec<String> {
    let Some(config) = config else {
        return Vec::new();
    };
    if runtime::template_of(config).is_none() {
        return Vec::new();
    }
    let Ok(description) = runtime::describe(config, path) else {
        return Vec::new();
    };
    orphans_in(&description, sample)
}

/// [`orphans`] by a description already read: `validate`'s, which starts no
/// Python.
pub fn orphans_in(description: &runtime::ModelDescription, sample: &Sample) -> Vec<String> {
    let computes = |name: &str| -> bool {
        if let Some(property) = description.properties.get(name) {
            return property.value.is_formula()
                || property
                    .uncertainty
                    .as_ref()
                    .is_some_and(runtime::Origin::is_formula);
        }
        let Some((table, column)) = name.split_once('.') else {
            return false;
        };
        description.tables.get(table).is_some_and(|held| {
            held.columns
                .iter()
                .any(|described| described.name == column && described.derived().is_some())
        })
    };
    // A statistic of readings is no formula's: the readings give it, whatever
    // the model declares.
    let a_statistic = |name: &str| -> bool {
        if let Some(property) = description.properties.get(name)
            && matches!(property.value, runtime::Origin::Statistic { .. })
        {
            return true;
        }
        match name.split_once('.') {
            None => crate::core::identifier::Identifier::new(name)
                .ok()
                .and_then(|name| sample.property(&name).ok())
                .is_some_and(|handle| handle.peek(|property| property.readings().is_some())),
            Some((table, column)) => {
                let (Ok(table), Ok(column)) = (
                    crate::core::identifier::Identifier::new(table),
                    crate::core::identifier::Identifier::new(column),
                ) else {
                    return false;
                };
                sample.table(&table).is_ok_and(|held| {
                    held.column(&column).is_ok_and(|view| {
                        !view.statistics().is_empty()
                            || view.cells().any(|(_, cell)| cell.readings().is_some())
                    })
                })
            }
        }
    };
    let mut found: Vec<String> = version_control::records_of_values(sample)
        .into_iter()
        .filter(|held| !held.edited && !computes(&held.value) && !a_statistic(&held.value))
        .map(|held| held.value)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The values whose file states a statistic of their readings the model
/// declares otherwise, each with what `validate` says of it: the file's
/// statistic is read as it stands, and the next computation takes the
/// model's. By a description already read: no Python starts.
pub fn statistics_stated_otherwise(
    description: &runtime::ModelDescription,
    sample: &Sample,
) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for (name, described) in &description.properties {
        let Ok(identifier) = crate::core::identifier::Identifier::new(name) else {
            continue;
        };
        let Ok(handle) = sample.property(&identifier) else {
            continue;
        };
        let Some(stated) = handle.records().statistics else {
            continue;
        };
        let mut said = Vec::new();
        if let (Some(stated), Some(declared)) = (stated.value, described.value.statistic())
            && stated.name() != declared
        {
            said.push(format!(
                "the file states the {} of its readings, the model the {declared}",
                stated.name().replace('_', " "),
                declared = declared.replace('_', " ")
            ));
        }
        if let (Some(stated), Some(declared)) = (
            stated.uncertainty,
            described
                .uncertainty
                .as_ref()
                .and_then(runtime::Origin::statistic),
        ) && stated.name() != declared
        {
            said.push(format!(
                "its uncertainty the {}, the model the {declared}",
                stated.name().replace('_', " "),
                declared = declared.replace('_', " ")
            ));
        }
        if !said.is_empty() {
            found.push((
                name.clone(),
                format!(
                    "{} — the next computation takes the model's",
                    said.join("; ")
                ),
            ));
        }
    }
    found
}

/// What `status` and `validate` say of a value the model no longer computes.
pub fn orphan_said(file: &str, value: &str) -> String {
    format!(
        "the model no longer computes it — samplekit {file} set {value}= removes it; removing \
         its computed record keeps it as entered"
    )
}

/// The state word a planned reason is said by on every surface that names a
/// state: `outdated` for a formula changed, or read through one; `waiting`;
/// `never_computed`. `None` for what the files already say.
pub fn planned_word(said: &str) -> Option<&'static str> {
    if said.contains("formula changed") {
        Some("outdated")
    } else if said.starts_with("waits for") {
        Some("waiting")
    } else if said.contains("never computed") {
        Some(crate::collection::exports::NEVER_COMPUTED)
    } else if said.starts_with("edited") || said.starts_with("record missing") {
        // A value typed over its formula, which the plan leaves alone: an
        // override, as `status --json` says it, where a table said current.
        Some("edited")
    } else {
        None
    }
}
