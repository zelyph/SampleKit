//! What a computation over a selection runs: the samples grouped by the
//! configuration that names their model, the request for each, the samples left
//! out and why, and the record of which model computed what — the part of
//! `samplekit compute` the workbench runs too. The worker's events are model
//! runtime's; how they are shown is each surface's.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::collection::sample_list::SampleList;
use crate::config::model_runtime::{
    self as runtime, ComputedStore, Report, Request, Template, TemplateDigest,
};
use crate::config::project_config::{self, ConfigError, ProjectConfig};
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
}

/// The requests of one group, and the samples left out of it.
#[derive(Debug, Clone, Default)]
pub struct Requests {
    pub requests: Vec<Request>,
    /// A sample whose records cannot be followed has no order to run in, and is
    /// left out and said.
    pub unplannable: Vec<(PathBuf, String)>,
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
    // What the formulas were when each sample was last computed, for the worker
    // to tell a changed formula by.
    let store = ComputedStore::user();
    let mut out = Requests::default();
    for entry in group.entries.iter().filter_map(|at| selected.get(*at)) {
        let Some(path) = entry.path.clone() else {
            continue;
        };
        if let Some(reason) = unplannable(&entry.sample.borrow()) {
            out.unplannable.push((path, reason));
            continue;
        }
        out.requests.push(Request {
            refused: refused.get(&path).cloned().unwrap_or_default(),
            recorded: store
                .as_ref()
                .map(|store| store.recorded_formulas(&path))
                .unwrap_or_default(),
            sample: path,
            template: template.clone(),
            names: asked.names.clone(),
            rerun: asked.rerun,
            force: asked.force,
        });
    }
    out
}

/// Which model computed what was written, as `digest`, taken of the model
/// before its worker started, and which formula computed each value, as the
/// worker took them when it imported the model. A digest taken after the run
/// named a model edited meanwhile — by the user, or by the formula itself — as
/// the one that computed, and *the model changed since* was never said.
///
/// Recorded even when nothing was computed, once the worker read the model:
/// each formula's digest is the baseline the next edit is told apart by.
/// Best effort — a record that cannot be kept never fails a run.
pub fn record_computed(
    store: &ComputedStore,
    request: &Request,
    report: &Report,
    digest: &TemplateDigest,
) {
    if report.computed == 0 && report.formulas.digests.is_empty() {
        return;
    }
    let _ = store.record(&request.sample, digest, &report.formulas);
}

/// [`record_computed`] in the user's state folder, with the model's digest as
/// it is now: for a surface that took none before its worker started, which
/// then records a model edited during the run as the one that computed.
pub fn record_written(request: &Request, report: &Report) {
    if report.computed == 0 && report.formulas.digests.is_empty() {
        return;
    }
    if let (Some(store), Ok(digest)) = (
        runtime::ComputedStore::user(),
        runtime::digest_of(&request.template),
    ) {
        record_computed(&store, request, report, &digest);
    }
}
