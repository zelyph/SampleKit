//! *What is wrong with this collection* — every check that needs no model, no
//! interpreter and no network, only arithmetic on data already in the files.
//!
//! It is a module rather than the body of a command because `cli`'s invariant
//! forbids the alternative: a check reachable only by typing `samplekit
//! validate` is a check the TUI and Python cannot make.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::collection::sample_list::SampleList;
use crate::config::project_config::ProjectConfig;
use crate::core::formatting::{self, Precision, Presentation, Resolved};
use crate::core::identifier;
use crate::core::property::Property;
use crate::core::table::RowAddress;
use crate::core::value::{Value, ValueKind};
use crate::format::schema::PrecisionSchema;
use crate::query::field_addressing::{Channel, Field, State, States, describe};

// ------------------------------------------------------------------- types

/// Everything found, and how much was looked at: a count without a denominator
/// is not a result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub samples: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub path: Option<PathBuf>,
    pub sample: String,
    pub severity: Severity,
    pub detail: Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The data says something wrong.
    Defect,
    /// The data is fine and something is worth saying.
    Note,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Detail {
    /// A tag its file holds that is not an identifier.
    UnusableTag { tag: String, reason: String },
    /// A table its file holds that could not be read, set aside.
    TableSetAside { table: String, reason: String },
    /// A file declaring which statistic stood for a channel, and a number that
    /// is not it. The one statistical finding that is a `Defect`: the file
    /// states the intent, so the disagreement is evidence and not a guess.
    DeclaredStatisticDisagrees {
        quantity: String,
        channel: &'static str,
        declared: &'static str,
        stored: String,
        expected: String,
    },
    /// One quantity, two spellings of a unit, across the collection. Reported
    /// once per quantity, attached to a sample holding the rarest spelling.
    UnitDisagrees {
        quantity: String,
        spellings: Vec<(String, usize)>,
    },
    /// A file's unit contradicting the one `[property.*]` declares for that
    /// quantity. Once per spelling: a mistake made in seventy files is one
    /// mistake.
    ///
    /// It does not say whose mistake it is. The project's declaration may be
    /// the wrong one, or the files may be, and only the person reading knows
    /// which — so the finding states what disagrees with what.
    UnitContradictsDeclaration {
        quantity: String,
        written: String,
        declared: String,
        samples: usize,
    },
    /// `.3f` on a date, a boolean or a text value.
    PrecisionCannotApply {
        quantity: String,
        specifier: String,
        kind: ValueKind,
    },
    /// The file would be rewritten by a canonicalising pass.
    NotCanonical,
    /// `[matplotlib]` could not be checked: no Python, or no matplotlib in
    /// it. A `Note`: `plot` checks it when it draws.
    MatplotlibNotChecked { reason: String },
    /// A copy a synchroniser left beside `original`, left out. A `Note`.
    SyncConflict { original: String },
    /// A value a formula computed that the model no longer computes —
    /// renamed, or its formula removed. A `Note`: it blocks nothing.
    Orphan { value: String, said: String },
    /// A statistic the file states that its model declares otherwise —
    /// `{v: median}` under the model's mean. A `Note`: the next computation
    /// takes the model's.
    StatisticDiffersFromModel { value: String, said: String },
    /// A quantity its model declares, written in the file at the top level,
    /// as an attribute: the model cannot read the file. A `Defect`.
    QuantityAsAttribute { name: String, held: String },
    /// Something the file holds that the next write drops or changes: a
    /// `null`, a YAML tag, a date no calendar has, an uncertainty beside
    /// `n/a`. A `Note`, said in full.
    ChangedByAWrite { said: String },
    /// A file of the collection that could not be read at all.
    Unreadable { reason: String },
    /// One `name` held by several files: a report naming the sample cannot
    /// say which.
    DuplicateName { name: String, files: Vec<PathBuf> },
    /// An attribute linking to a sample, `"[[steel-02]]"`, that no sample of
    /// its project is — by its file's name, nor by its name — or that
    /// several are, in `files`; or a Markdown link, `"[report](r.pdf)"`, to
    /// a path where nothing is. A `Defect`.
    LinkLeadsNowhere {
        attribute: String,
        link: String,
        target: String,
        files: Vec<PathBuf>,
    },
    /// Names that differ only by case or by how their accents are written:
    /// one file on macOS and Windows, and one sample to a reader. A `Note`.
    NamesAlike {
        names: Vec<String>,
        files: Vec<PathBuf>,
    },
    /// A field one sample alone holds, a slip away from one others hold:
    /// `ogg` beside `radius`. A `Note`.
    FieldAlike {
        field: String,
        near: String,
        others: usize,
    },
    /// A query, a profile or a view naming what no sample of the collection
    /// holds.
    DeclarationNamesNoField { declaration: String, reason: String },
    /// A declared model that cannot be found: its file, or its class.
    ModelUnusable { reason: String },
    /// A declaration a command would refuse when it reads it — a table style
    /// no table draws — found before any command does. A `Defect`.
    DeclarationUnusable { declaration: String, reason: String },
    /// A `[collection] files` entry that cannot find what it means to — a
    /// defect — or whose folder is not there — a note.
    FilesPatternUnusable { entry: String, reason: String },
    /// A declaration a `.samplekitrc` writes as the file it imports already
    /// does: a copy to remove. A `Note`.
    CopiedDeclaration { key: String, from: PathBuf },
    /// A text where a number belongs: a quoted number, or a decimal comma.
    NumberExpected { quantity: String, text: String },
    /// A date, a date and time, or true/false where the model or a unit
    /// declares a number.
    NotANumber { quantity: String, found: String },
    /// A value outside the range `[property.*] range` declares. A `Defect`.
    OutsideRange { quantity: String, said: String },
    /// Files that declare no `schema_version`, left out as no samples: how
    /// many, said once, at the first of them. A `Note`.
    MissingVersion { files: usize },
    /// A brace in a column's template that stands where a placeholder would
    /// and names no channel: `{bogus}`, with the channel it is one edit or two
    /// from where it is one, `{valeu}`. A `Note` — it is written out as it
    /// stands, and only its author knows whether that was meant.
    TemplateBraceNearAChannel {
        declaration: String,
        written: String,
        channel: Option<String>,
    },
    /// A `[property.*]` symbol the files override with a different one of their
    /// own, and how many write theirs. A `Note`: a file wins over the project,
    /// and nothing anywhere said so. `channel` is `symbol`, the one a file may
    /// still carry: a precision is the project's alone.
    DeclarationIsShadowed {
        quantity: String,
        channel: &'static str,
        declared: String,
        written: String,
        samples: usize,
    },
    /// A nonzero value its declared precision writes as `0.000`. A `Note`: the
    /// screen, the file, the export and the template all write it so, and this
    /// is where a reader learns that it is the declaration rather than the
    /// measurement that says nothing.
    PrecisionWritesZero {
        quantity: String,
        specifier: String,
        value: String,
    },
    /// Derived values that no longer rest on their inputs, or whose formula
    /// failed: how many, in this sample. A `Note` — work to do, not something
    /// wrong in the data — and `samplekit status` names each.
    NotCurrent { values: usize },
    /// Values held over their formula — edited, or kept with no record of how
    /// they came: how many, in this sample. A `Note` — a choice somebody made,
    /// not work to do and not a fault — and `samplekit status` names each.
    Overridden { values: usize },
    /// Records no walk can follow: a cycle, which no write produces and a hand
    /// edit can. `quantity` is the first value it costs its verdict, `others`
    /// how many more. A `Defect`: the file says something impossible.
    RecordsCannotBeFollowed {
        quantity: String,
        reason: String,
        others: usize,
    },
}

impl Detail {
    /// The value or table a finding is about, where it is one: what a
    /// surface opens the sample on.
    pub fn quantity(&self) -> Option<&str> {
        match self {
            Detail::DeclaredStatisticDisagrees { quantity, .. }
            | Detail::UnitDisagrees { quantity, .. }
            | Detail::UnitContradictsDeclaration { quantity, .. }
            | Detail::PrecisionCannotApply { quantity, .. }
            | Detail::NumberExpected { quantity, .. }
            | Detail::NotANumber { quantity, .. }
            | Detail::OutsideRange { quantity, .. } => Some(quantity),
            Detail::TableSetAside { table, .. } => Some(table),
            _ => None,
        }
    }
}

// --------------------------------------------------------------- functions

/// What a sample's records say of its derived values: one note counting what is
/// stale, broken or failed, one counting overrides — edited, or held without
/// their record — which are a choice somebody made and so are counted apart
/// from the work to do, and one defect where the records themselves cannot be
/// followed.
fn freshness_of(
    sample: &crate::core::sample::Sample,
    at: &Where,
    orphans: &[String],
) -> Vec<Finding> {
    use crate::format::fingerprint::Freshness;
    // Counted as `status` counts them: a property once, a table's column once
    // for each way its rows stand. What the model no longer computes is no
    // value behind, nor is what reads it through that value alone.
    let states: Vec<(String, Freshness)> = crate::collection::editing::not_current(sample)
        .into_iter()
        .filter(|entry| !orphans.contains(&entry.name))
        .filter(|entry| {
            !matches!(&entry.state, Freshness::Stale { changed, upstream }
                if changed.is_empty()
                    && !upstream.is_empty()
                    && upstream.iter().all(|name| orphans.contains(&name.to_string())))
        })
        .map(|entry| (entry.name, entry.state))
        .collect();
    let mut findings = Vec::new();
    let behind = states
        .iter()
        .filter(|(_, state)| {
            matches!(
                state,
                Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Failed { .. }
            )
        })
        .count();
    if behind > 0 {
        findings.push(Finding {
            path: at.path.clone(),
            sample: at.sample.clone(),
            severity: Severity::Note,
            detail: Detail::NotCurrent { values: behind },
        });
    }
    let held = states
        .iter()
        .filter(|(_, state)| matches!(state, Freshness::Edited | Freshness::RecordMissing))
        .count();
    if held > 0 {
        findings.push(Finding {
            path: at.path.clone(),
            sample: at.sample.clone(),
            severity: Severity::Note,
            detail: Detail::Overridden { values: held },
        });
    }
    let mut unjudged = states.into_iter().filter_map(|(name, state)| match state {
        Freshness::Unjudged { reason } => Some((name, reason)),
        _ => None,
    });
    if let Some((quantity, reason)) = unjudged.next() {
        findings.push(Finding {
            path: at.path.clone(),
            sample: at.sample.clone(),
            severity: Severity::Defect,
            detail: Detail::RecordsCannotBeFollowed {
                quantity,
                reason,
                others: unjudged.count(),
            },
        });
    }
    findings
}

/// Every check, over a whole collection.
///
/// The collection is the unit because two of the checks are collection-wide: a
/// unit disagreement has no meaning inside one file. A single sample is
/// validated as a collection of one, which is what `cli` does with a file
/// argument.
///
/// **Never fails.** A sample whose value cannot be computed produces a finding,
/// not an error: a report that stops at the first problem is a report nobody
/// can plan a morning around.
pub fn run(list: &SampleList) -> Report {
    run_judging(list, true)
}

/// The same over files named one by one: each judged alone, and the
/// configuration's queries, profiles, exports and figures not judged against
/// them — a file cannot answer for the fields of the rest. What the
/// configuration says of the file's own values, a precision or a unit, still
/// is.
pub fn run_on_files(list: &SampleList) -> Report {
    run_judging(list, false)
}

fn run_judging(list: &SampleList, declarations: bool) -> Report {
    let mut findings = Vec::new();
    // Quantity -> spelling -> (how many samples, the first one seen).
    let mut units: Units = BTreeMap::new();
    // What each project's model declares, read once per project: a text in a
    // quantity it declares is refused whatever other samples hold.
    let mut declared_by_model: BTreeMap<Option<PathBuf>, std::collections::BTreeSet<String>> =
        BTreeMap::new();
    // Configuration and name -> where each sample holding it came from: a
    // name identifies one sample of a project, and two projects nested one in
    // the other keep their own, as `new` allows.
    let mut named: BTreeMap<(Option<PathBuf>, String), Vec<Where>> = BTreeMap::new();
    // How many samples hold each name: a shared one is shown with its file.
    let mut name_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in list.iter() {
        if let Some(name) = entry.sample.borrow().name() {
            *name_counts.entry(name.to_string()).or_default() += 1;
        }
    }

    for entry in list.iter() {
        let sample = entry.sample.borrow();
        let at = Where {
            path: entry.path.clone(),
            sample: label_of(&sample, entry.path.as_deref(), &name_counts),
        };
        // A name written, `name:`, or its file's name standing for it: two
        // `s-1.md` in two folders are two samples nothing can tell apart.
        if let Some(name) = sample.name() {
            let project = entry
                .path
                .as_deref()
                .and_then(|path| list.configuration_of(path))
                .map(std::path::Path::to_path_buf);
            named
                .entry((project, name.to_string()))
                .or_default()
                .push(at.clone());
        }
        // A part its file could not read is reported, and the rest validated.
        for tag in sample.unusable_tags() {
            findings.push(Finding {
                path: at.path.clone(),
                sample: at.sample.clone(),
                severity: Severity::Defect,
                detail: Detail::UnusableTag {
                    tag: tag.clone(),
                    reason: crate::core::identifier::Identifier::new(tag)
                        .err()
                        .map(|error| error.to_string())
                        .unwrap_or_default(),
                },
            });
        }
        for (table, reason) in sample.set_aside_tables() {
            findings.push(Finding {
                path: at.path.clone(),
                sample: at.sample.clone(),
                severity: Severity::Defect,
                detail: Detail::TableSetAside {
                    table: table.to_string(),
                    reason: reason.clone(),
                },
            });
        }
        // A precision is the project's: the one describing this sample.
        let project = match entry.path.as_deref() {
            Some(path) => list.config_for(path),
            None => list.config(),
        };
        let declared = |quantity: &str| declared_precision(project, quantity);
        // Units are the vocabulary of the project describing this sample.
        let configuration: Option<PathBuf> = entry
            .path
            .as_deref()
            .and_then(|path| list.configuration_of(path))
            .map(|path| path.to_path_buf());

        // A list gathered without its configurations — a script's — finds each
        // sample's own, once per file.
        let found = match (project, entry.path.as_deref()) {
            (None, Some(path)) => crate::config::project_config::find(path),
            _ => None,
        };
        let model_values = declared_by_model
            .entry(
                found
                    .clone()
                    .or_else(|| project.map(|config| config.root().to_path_buf())),
            )
            .or_insert_with(|| {
                let loaded = found
                    .as_deref()
                    .and_then(|file| crate::config::project_config::load(file).ok());
                loaded
                    .as_ref()
                    .or(project)
                    .map(|config| crate::config::model_runtime::model_declarations(config).1)
                    .unwrap_or_default()
                    .into_iter()
                    .collect()
            })
            .clone();
        for name in sample.property_names() {
            let field = Field::Named {
                name: name.clone(),
                channel: Channel::Value,
            };
            // The model's description, or the project's declaration of a
            // unit, says a number belongs here.
            let numeric = model_values.contains(name.as_str())
                || project
                    .and_then(|config| config.property(name.as_str()))
                    .is_some_and(|declaration| declaration.unit.is_some());
            let quantity = describe(&field);
            sample
                .property(name)
                .expect("a name from property_names resolves")
                .with(|property| {
                    inspect(
                        property,
                        &quantity,
                        &at,
                        &configuration,
                        declared(name.as_str()).as_ref(),
                        &mut findings,
                        &mut units,
                    );
                    let unit = property.presentation().unit.clone();
                    number_expected(
                        property,
                        &quantity,
                        unit.as_deref(),
                        numeric,
                        &at,
                        &mut findings,
                    );
                    if let Some(value) = property.peek_value()
                        && let Some(said) =
                            outside_range(project, name.as_str(), &value, unit.as_deref())
                    {
                        findings.push(Finding {
                            path: at.path.clone(),
                            sample: at.sample.clone(),
                            severity: Severity::Defect,
                            detail: Detail::OutsideRange {
                                quantity: quantity.clone(),
                                said,
                            },
                        });
                    }
                });
        }

        for table_name in sample.table_names() {
            let Ok(table) = sample.table(table_name) else {
                continue;
            };
            // A column's unit and precision are declared once for the column,
            // so they are read from the column rather than from a cell.
            for column in table.column_names() {
                let quantity = format!("{table_name}.{column}");
                if let Ok(view) = table.column(column) {
                    remember(
                        &mut units,
                        &configuration,
                        &quantity,
                        view.presentation(),
                        &at,
                    );
                }
            }
            // A column most of whose cells hold numbers holds numbers: a text
            // written by hand in one of its cells passed, where the same text
            // in a quantity was a defect. Most, and two at least: a column of
            // remarks holding one number stays a column of remarks.
            let mut kinds: BTreeMap<String, (usize, usize)> = BTreeMap::new();
            for row in table.rows() {
                for column in row.column_names() {
                    let counted = kinds.entry(column.to_string()).or_default();
                    match row.cell(column).ok().and_then(|cell| cell.peek_value()) {
                        Some(Value::Number(_) | Value::Integer(_)) => counted.0 += 1,
                        Some(Value::Text(_)) => counted.1 += 1,
                        _ => {}
                    }
                }
            }
            let numeric: std::collections::BTreeSet<String> = kinds
                .into_iter()
                .filter(|(_, (numbers, texts))| *numbers >= 2 && numbers > texts)
                .map(|(column, _)| column)
                .collect();
            for row in table.rows() {
                let index: Vec<Value> = row.index().into_iter().cloned().collect();
                for column in row.column_names() {
                    let Ok(cell) = row.cell(column) else {
                        continue;
                    };
                    let quantity = describe(&Field::Cell {
                        table: table_name.clone(),
                        column: column.clone(),
                        row: RowAddress::index(index.clone()),
                        channel: Channel::Value,
                    });
                    // Units are the column's, checked above; a cell is checked
                    // for what a cell carries of its own.
                    statistic_check(cell, &quantity, &at, &mut findings);
                    let unit = table
                        .presentation_of(column, &RowAddress::index(index.clone()))
                        .ok()
                        .and_then(|presentation| presentation.unit);
                    let in_numbers = numeric.contains(column.as_str());
                    number_expected(
                        cell,
                        &quantity,
                        unit.as_deref(),
                        in_numbers,
                        &at,
                        &mut findings,
                    );
                    precision_check(
                        cell,
                        declared(&format!("{table_name}.{column}")).as_ref(),
                        &quantity,
                        &at,
                        &mut findings,
                    );
                }
            }
        }

        if let Some(path) = &entry.path
            && let Ok(source) = std::fs::read_to_string(path)
        {
            for said in crate::format::document::changed_by_a_write(&source) {
                findings.push(Finding {
                    path: Some(path.clone()),
                    sample: at.sample.clone(),
                    severity: Severity::Note,
                    detail: Detail::ChangedByAWrite { said },
                });
            }
        }
        if let Some(path) = &entry.path
            && !is_canonical(path)
        {
            findings.push(Finding {
                path: Some(path.clone()),
                sample: at.sample.clone(),
                severity: Severity::Note,
                detail: Detail::NotCanonical,
            });
        }
        // What the model no longer computes, as its description written last
        // says: no Python is started here.
        let orphans = match (
            entry.path.as_deref(),
            entry.path.as_deref().and_then(|path| list.config_for(path)),
        ) {
            (Some(path), Some(config)) => crate::config::model_runtime::current_description(config)
                .map(|description| {
                    crate::collection::computation::orphans_in(&description, &sample)
                })
                .unwrap_or_default()
                .into_iter()
                .map(|value| {
                    let said = crate::collection::computation::orphan_said(
                        &path.display().to_string(),
                        &value,
                    );
                    (value, said)
                })
                .collect(),
            _ => Vec::new(),
        };
        for (value, said) in &orphans {
            findings.push(Finding {
                path: at.path.clone(),
                sample: at.sample.clone(),
                severity: Severity::Note,
                detail: Detail::Orphan {
                    value: value.clone(),
                    said: said.clone(),
                },
            });
        }
        if let Some(config) = entry.path.as_deref().and_then(|path| list.config_for(path))
            && let Some(description) = crate::config::model_runtime::current_description(config)
        {
            for (value, said) in
                crate::collection::computation::statistics_stated_otherwise(&description, &sample)
            {
                findings.push(Finding {
                    path: at.path.clone(),
                    sample: at.sample.clone(),
                    severity: Severity::Note,
                    detail: Detail::StatisticDiffersFromModel { value, said },
                });
            }
            for name in sample.attribute_names() {
                if !description.properties.contains_key(name.as_str()) {
                    continue;
                }
                let held = match sample.attribute(name) {
                    Ok(crate::core::sample::AttributeValue::Scalar(value)) => {
                        crate::collection::editing::shown_value(value)
                    }
                    _ => "…".to_string(),
                };
                findings.push(Finding {
                    path: at.path.clone(),
                    sample: at.sample.clone(),
                    severity: Severity::Defect,
                    detail: Detail::QuantityAsAttribute {
                        name: name.to_string(),
                        held,
                    },
                });
            }
        }
        let orphan_names: Vec<String> = orphans.iter().map(|(value, _)| value.clone()).collect();
        findings.extend(freshness_of(&sample, &at, &orphan_names));
    }

    // A file set aside unread is a defect: a report over what could be read
    // must say what could not. A sync conflict's copy is said, a note: it is
    // no sample, and its owner compares the two.
    for skipped in list.skipped() {
        if let crate::config::discovery::SkipReason::SyncConflict { original } = &skipped.reason {
            findings.push(Finding {
                path: Some(skipped.path.clone()),
                sample: skipped
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                severity: Severity::Note,
                detail: Detail::SyncConflict {
                    original: original
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                },
            });
        }
        if let crate::config::discovery::SkipReason::Malformed { message }
        | crate::config::discovery::SkipReason::Unreadable { message }
        | crate::config::discovery::SkipReason::Configuration { message } = &skipped.reason
        {
            findings.push(Finding {
                path: Some(skipped.path.clone()),
                sample: skipped
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                severity: Severity::Defect,
                detail: Detail::Unreadable {
                    reason: message.clone(),
                },
            });
        }
    }

    // One name on two files makes every finding and every report naming that
    // sample ambiguous.
    for ((_, name), holders) in &named {
        let [.., last] = holders.as_slice() else {
            continue;
        };
        if holders.len() < 2 {
            continue;
        }
        findings.push(Finding {
            path: last.path.clone(),
            sample: name.clone(),
            severity: Severity::Defect,
            detail: Detail::DuplicateName {
                name: name.clone(),
                files: holders
                    .iter()
                    .filter_map(|holder| holder.path.clone())
                    .collect(),
            },
        });
    }

    // A link leads somewhere: a wikilink to one of its project's samples,
    // every file of the project read for it; a Markdown link to a file there.
    // A web address is not checked: nothing here goes on the network.
    let known: Vec<(PathBuf, Option<String>)> = list
        .iter()
        .filter_map(|entry| {
            let name = entry.sample.borrow().name().map(str::to_string);
            Some((entry.path.clone()?, name))
        })
        .collect();
    let mut projects_samples: BTreeMap<Option<PathBuf>, Vec<(PathBuf, Option<String>)>> =
        BTreeMap::new();
    for entry in list.iter() {
        let Some(from) = entry.path.as_deref() else {
            continue;
        };
        let sample = entry.sample.borrow();
        let config = list.config_for(from);
        for attribute in sample.attribute_names() {
            let held: Vec<&Value> = match sample.attribute(attribute) {
                Ok(crate::core::sample::AttributeValue::Scalar(value)) => vec![value],
                Ok(crate::core::sample::AttributeValue::List(values)) => values.iter().collect(),
                Err(_) => Vec::new(),
            };
            for value in held {
                let Value::Text(text) = value else {
                    continue;
                };
                let Some(link) = crate::core::link::parse(text) else {
                    continue;
                };
                let leads = crate::config::discovery::destination(&link, from, || {
                    projects_samples
                        .entry(config.map(|config| config.root().to_path_buf()))
                        .or_insert_with(|| crate::config::discovery::linkable(config, from, &known))
                        .clone()
                });
                let (target, files) = match leads {
                    crate::config::discovery::Destination::Nowhere(target) => (target, Vec::new()),
                    crate::config::discovery::Destination::Several(files) => {
                        (link.shown().to_string(), files)
                    }
                    _ => continue,
                };
                findings.push(Finding {
                    path: entry.path.clone(),
                    sample: label_of(&sample, entry.path.as_deref(), &name_counts),
                    severity: Severity::Defect,
                    detail: Detail::LinkLeadsNowhere {
                        attribute: attribute.to_string(),
                        link: text.trim().to_string(),
                        target,
                        files,
                    },
                });
            }
        }
    }

    // Names alike but not the same, `Ab` and `ab`, or an accent written whole
    // and as a letter with its mark: a note, never said of a name held twice,
    // which is a defect of its own above.
    type Alike<'a> = BTreeMap<(Option<PathBuf>, String), Vec<(&'a String, &'a Where)>>;
    let mut alike: Alike = BTreeMap::new();
    for ((project, name), holders) in &named {
        if let Some(first) = holders.first() {
            alike
                .entry((project.clone(), crate::collection::editing::name_key(name)))
                .or_default()
                .push((name, first));
        }
    }
    for held in alike.values() {
        if held.len() < 2 {
            continue;
        }
        let (_, last) = held[held.len() - 1];
        findings.push(Finding {
            path: last.path.clone(),
            sample: last.sample.clone(),
            severity: Severity::Note,
            detail: Detail::NamesAlike {
                names: held.iter().map(|(name, _)| (*name).clone()).collect(),
                files: held
                    .iter()
                    .filter_map(|(_, holder)| holder.path.clone())
                    .collect(),
            },
        });
    }

    // A field one sample alone holds, a slip from one others hold: `set
    // ogg=1.070`, refused with its advice once, written anyway, and never
    // said again. Judged over a collection, not over files named one by one.
    if declarations {
        findings.extend(field_slips(list));
    }

    // Each configuration answers for the samples it describes.
    if list.spans_configurations() {
        let everything = list.filter_by(|_| true);
        let canonical = |path: &std::path::Path| {
            dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
        };
        for part in list.by_configuration(&everything) {
            if declarations {
                // The list's own configuration — a root each project imports
                // — declares for those projects too: its columns judged
                // against their samples, `measurements.eps` said held by no
                // sample though every cylinder holds it.
                let root = list
                    .config()
                    .filter(|root| part.config.is_some_and(|own| own.root() == root.root()));
                match root {
                    Some(root) => {
                        let file =
                            canonical(&root.root().join(crate::config::project_config::FILENAME));
                        let importing = everything.filter_entries(|entry| {
                            entry
                                .path
                                .as_deref()
                                .and_then(|path| list.config_for(path))
                                .is_some_and(|config| {
                                    config.root() != root.root()
                                        && config
                                            .imports()
                                            .iter()
                                            .any(|imported| canonical(imported) == file)
                                })
                        });
                        findings.extend(declaration_findings(&part.samples.joined(&importing)));
                    }
                    None => findings.extend(declaration_findings(&part.samples)),
                }
                findings.extend(profile_precision_findings(&part.samples));
            }
            findings.extend(shadowed_findings(&part.samples));
            findings.extend(template_findings(&part.samples));
            findings.extend(model_findings(&part.samples));
            findings.extend(files_findings(&part.samples));
            findings.extend(copy_findings(&part.samples));
            findings.extend(export_findings(&part.samples));
            findings.extend(matplotlib_findings(&part.samples));
        }
    } else {
        if declarations {
            findings.extend(declaration_findings(list));
            findings.extend(profile_precision_findings(list));
        }
        findings.extend(shadowed_findings(list));
        findings.extend(template_findings(list));
        findings.extend(model_findings(list));
        findings.extend(files_findings(list));
        findings.extend(copy_findings(list));
        findings.extend(export_findings(list));
        findings.extend(matplotlib_findings(list));
    }
    findings.extend(readings_under_formula_findings(list));
    findings.extend(missing_version_findings(list));
    findings.extend(all_excluded_findings(list));
    findings.extend(unit_findings(&units, list));
    Report {
        findings,
        samples: list.len(),
    }
}

/// What a finding says, as every surface says it — without the sample it is
/// about, which each lays out its own way.
pub fn described(finding: &Finding) -> String {
    let counted_as = |count: usize, one: &str, many: &str| {
        format!("{count} {}", if count == 1 { one } else { many })
    };
    match &finding.detail {
        Detail::UnusableTag { tag, reason } => format!(
            "tag '{tag}' is not an identifier: {reason}\n    samplekit tag rename '{tag}' NEW --write repairs it"
        ),
        Detail::TableSetAside { table, reason } => format!(
            "table {table} was not read: {reason}; the sample is not written until its file is repaired"
        ),
        Detail::DeclaredStatisticDisagrees {
            quantity,
            channel,
            declared,
            stored,
            expected,
        } => format!(
            "{quantity}: {channel} {stored} is not the declared {declared} \
             of its readings, which is {expected}"
        ),
        Detail::UnitDisagrees {
            quantity,
            spellings,
        } => format!(
            "{quantity}: two units — {}",
            spellings
                .iter()
                .map(|(unit, count)| format!("{unit} on {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        // Which of the two is wrong is not this message's to say.
        Detail::UnitContradictsDeclaration {
            quantity,
            written,
            declared,
            samples,
        } => format!(
            "{quantity}: {} in {}, and the project declares {declared} — \
             one of the two is wrong",
            written,
            counted_as(*samples, "sample", "samples")
        ),
        Detail::PrecisionCannotApply {
            quantity,
            specifier,
            kind,
        } => format!("{quantity}: precision {specifier} cannot apply to a {kind:?} value"),
        Detail::DeclarationIsShadowed {
            quantity,
            channel,
            declared,
            written,
            samples,
        } => format!(
            "{quantity}: the project declares {channel} {declared}, and {} {written} — a file \
             wins over the project",
            counted_as(*samples, "sample writes", "samples write")
        ),
        Detail::PrecisionWritesZero {
            quantity,
            specifier,
            value,
        } => format!(
            "{quantity}: precision {specifier} writes {value} as zero, on every surface \
             alike — declare a finer one"
        ),
        Detail::NotCanonical => {
            "would be rewritten by a canonicalising pass: the next command that \
             writes it does so"
                .to_string()
        }
        Detail::ChangedByAWrite { said } => said.clone(),
        Detail::Orphan { value, said } => format!("{value}: {said}"),
        Detail::StatisticDiffersFromModel { value, said } => format!("{value}: {said}"),
        Detail::QuantityAsAttribute { name, held } => format!(
            "'{name}' is written as an attribute, at the top level, and the model declares it a \
             property, so the model cannot read the file: set {name}={held} on it writes it as one"
        ),
        Detail::MatplotlibNotChecked { reason } => {
            format!("[matplotlib] was not checked — {reason}; plot checks it when it draws")
        }
        Detail::SyncConflict { original } => format!(
            "a copy left by a sync conflict of {original}, left out: compare the two, keep \
             one, and remove the other"
        ),
        Detail::TemplateBraceNearAChannel {
            declaration,
            written,
            channel,
        } => match channel {
            Some(channel) => format!(
                "{declaration}: {{{written}}} names no channel and is written as it stands — \
                 did you mean {{{channel}}}?"
            ),
            None => format!(
                "{declaration}: {{{written}}} names no channel and is written as it stands — \
                 a template fills {{value}}, {{uncertainty}}, {{unit}} and {{symbol}}"
            ),
        },
        Detail::NotCurrent { values } => format!(
            "{} not current: samplekit status names {}",
            counted_as(*values, "value is", "values are"),
            if *values == 1 { "it" } else { "them" }
        ),
        Detail::Overridden { values } => format!(
            "{} over {} formula, kept until compute --force: samplekit status names {}",
            counted_as(*values, "value is typed", "values are typed"),
            if *values == 1 { "its" } else { "their" },
            if *values == 1 { "it" } else { "them" }
        ),
        Detail::RecordsCannotBeFollowed {
            quantity,
            reason,
            others,
        } => {
            let more = match others {
                0 => String::new(),
                n => format!(", and {} resting on it", counted_as(*n, "value", "values")),
            };
            format!(
                "{quantity}: {reason}{more}\n    no write produces this, so the file was edited by hand: remove the computed record that closes the cycle, and samplekit compute --force --write gives the value back to its formula"
            )
        }
        Detail::Unreadable { reason } => format!("not read: {}", without_own_path(reason)),
        Detail::ModelUnusable { reason } => reason.clone(),
        Detail::DeclarationUnusable {
            declaration,
            reason,
        } => format!("{declaration}: {reason}"),
        // `include` and `exclude` name themselves; a `files` entry is only
        // its pattern.
        Detail::FilesPatternUnusable { entry, reason }
            if entry.starts_with("include '") || entry.starts_with("exclude '") =>
        {
            format!("[collection] {entry}: {reason}")
        }
        Detail::FilesPatternUnusable { entry, reason } => {
            format!("[collection] files '{entry}': {reason}")
        }
        Detail::CopiedDeclaration { key, from } => format!(
            "{key} repeats what {} declares, which this file imports: a copy to remove",
            from.display()
        ),
        Detail::DeclarationNamesNoField {
            declaration,
            reason,
        } => format!(
            "{declaration}: {}",
            reason
                .lines()
                .map(str::trim)
                .collect::<Vec<_>>()
                .join(" — ")
        ),
        Detail::FieldAlike {
            field,
            near,
            others,
        } => format!(
            "'{field}' is held here alone, and '{near}' by {others} other {}: written for \
             '{near}'?",
            if *others == 1 { "sample" } else { "samples" }
        ),
        Detail::NamesAlike { names, files } => format!(
            "the names {} differ only by case or by how their accents are written, in {}: on \
             macOS and Windows two files named so are one, and a reader takes them for one \
             sample",
            names
                .iter()
                .map(|name| format!("'{name}'"))
                .collect::<Vec<_>>()
                .join(" and "),
            files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Detail::LinkLeadsNowhere {
            attribute,
            link,
            target,
            files,
        } if files.is_empty() && link.starts_with("[[") => format!(
            "{attribute}: {link} leads to no sample — no file of the project is named \
             {target}, and no sample's name is"
        ),
        Detail::LinkLeadsNowhere {
            attribute,
            link,
            target,
            files,
        } if files.is_empty() => format!(
            "{attribute}: {link} leads to nothing — no file is at {target}, from the \
             sample's folder"
        ),
        Detail::LinkLeadsNowhere {
            attribute,
            link,
            files,
            ..
        } => format!(
            "{attribute}: {link} leads to {} samples, {}: a link names one",
            files.len(),
            files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Detail::DuplicateName { name, files } => format!(
            "the name '{name}' is held by {} files: {}",
            files.len(),
            files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Detail::NotANumber { quantity, found } => {
            format!("{quantity}: number expected, found {found}")
        }
        Detail::OutsideRange { said, .. } => said.clone(),
        Detail::NumberExpected { quantity, text } => format!(
            "{quantity}: number expected, found the text \"{text}\"{}",
            if text.contains(',') && text.trim().replace(',', ".").parse::<f64>().is_ok() {
                " — a decimal comma?"
            } else if text.contains(',')
                && text
                    .split(',')
                    .all(|piece| piece.trim().parse::<f64>().is_ok())
            {
                " — several readings? a value is one number"
            } else if text.trim().parse::<f64>().is_ok_and(f64::is_infinite) {
                " — a number too large for any file to hold, and no measurement: a \
                 measurement is finite"
            } else {
                ""
            }
        ),
        Detail::MissingVersion { files } => format!(
            "{} no schema_version, and {} left out as no sample — a sample says which shape \
             it is, 'schema_version: 1'",
            if *files == 1 {
                "this file declares".to_string()
            } else {
                format!(
                    "this file and {} declare",
                    if *files == 2 {
                        "1 other".to_string()
                    } else {
                        format!("{} others", files - 1)
                    }
                )
            },
            if *files == 1 { "is" } else { "are" }
        ),
    }
}

/// Whether a report should make a caller fail.
pub fn has_defects(report: &Report) -> bool {
    report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Defect)
}

// ----------------------------------------------------------------- checks

/// Where a finding came from. Every finding carries this, because a report
/// naming no file is a report nobody can act on.
#[derive(Debug, Clone, PartialEq)]
struct Where {
    path: Option<PathBuf>,
    sample: String,
}

/// Quantity spellings, per the configuration describing the samples that wrote
/// them: projects are free to spell one quantity differently.
type Units = BTreeMap<(Option<PathBuf>, String), BTreeMap<String, (usize, Where)>>;

fn inspect(
    property: &Property,
    quantity: &str,
    at: &Where,
    configuration: &Option<PathBuf>,
    precision: Option<&Precision>,
    findings: &mut Vec<Finding>,
    units: &mut Units,
) {
    remember(units, configuration, quantity, property.presentation(), at);
    statistic_check(property, quantity, at, findings);
    precision_check(property, precision, quantity, at, findings);
}

/// The precision the project declares for this quantity in `[property.*]`, the
/// only place a quantity's own is declared; `table.column` for a table's cells.
/// `[render] precision` is a default for every number, not a statement about
/// this one, so a date it cannot apply to is not a defect.
fn declared_precision(project: Option<&ProjectConfig>, quantity: &str) -> Option<Precision> {
    project?
        .property(quantity)?
        .precision
        .as_ref()
        .and_then(PrecisionSchema::precision)
}

/// A stored uncertainty against the statistic its file states stood for it :
/// the one comparison with evidence. A number beside readings no statistic
/// gives is otherwise the model's or a hand's, and not a finding.
fn statistic_check(property: &Property, quantity: &str, at: &Where, findings: &mut Vec<Finding>) {
    let Some(readings) = property.readings() else {
        return;
    };
    if readings.len().get() < 2 {
        return;
    }
    let Ok(Some(uncertainty)) = property.uncertainty() else {
        return;
    };
    let stored = uncertainty.magnitude();
    let Some(declared) = property
        .records()
        .statistics
        .and_then(|stated| stated.uncertainty)
    else {
        return;
    };
    let Some(expected) = crate::core::uncertainty::from_readings(readings, declared)
        .map(|expected| expected.magnitude())
    else {
        return;
    };
    // Readings changed since the statistic was taken: the stored number is the
    // old readings' statistic, which is *stale — readings* and said as such,
    // not a disagreement. A defect was called over `set --readings` used as
    // documented, and not over new readings that happened to keep the spread.
    let recorded = property.records().computed.as_ref().and_then(|inputs| {
        inputs.get(&crate::core::property::InputName::Named(
            crate::format::fingerprint::readings_key(),
        ))
    });
    if let Some(crate::core::property::InputRecord::Digest(then)) = recorded {
        let now = crate::format::fingerprint::of_readings(&crate::format::schema::PropertySchema {
            readings: Some(readings.as_slice().to_vec()),
            ..Default::default()
        });
        if now.is_some_and(|now| !now.same_digest(then)) {
            return;
        }
    }
    if !same(stored, expected) {
        findings.push(Finding {
            path: at.path.clone(),
            sample: at.sample.clone(),
            severity: Severity::Defect,
            detail: Detail::DeclaredStatisticDisagrees {
                quantity: quantity.to_string(),
                channel: "uncertainty",
                declared: declared.name(),
                stored: written(stored),
                expected: written(expected),
            },
        });
    }
}

/// **A match is judged at full precision**: the last-bit noise of a value
/// written back by the code that computed it, and nothing more. A display
/// precision is how a number looks — at `.2f` the owner's combined uncertainty
/// and its bare standard error both print `0.02`, and the earlier rule called
/// that a match 210 times.
fn same(stored: f64, statistic: f64) -> bool {
    (stored - statistic).abs() <= 1e-9 * stored.abs().max(statistic.abs()).max(1e-12)
}

/// A number as `formatting` writes one with nothing declared: enough digits to
/// see how far apart two of them are, which a display precision would hide.
fn written(number: f64) -> String {
    match Value::number(number) {
        Ok(value) => formatting::format_value(
            &value,
            &Resolved {
                unit: None,
                symbol: None,
                property: None,
                uncertainty: None,
                unitless: None,
                unit_alone: None,
                unit_in_form: None,
                precision: None,
            },
        ),
        Err(_) => number.to_string(),
    }
}

/// A numeric precision declared on something that is not a number. Every
/// specifier this project accepts is numeric, so the kind alone settles it.
fn precision_check(
    property: &Property,
    precision: Option<&Precision>,
    quantity: &str,
    at: &Where,
    findings: &mut Vec<Finding>,
) {
    let Some(precision) = precision else {
        return;
    };
    let Ok(value) = property.value() else {
        return;
    };
    let kind = value.kind();
    // Absent is not a mismatch: an unmeasured quantity is ordinary, and its
    // declaration is a statement about the number it will hold. Nor is `n/a`,
    // an answer with no digits for a precision to shape.
    if kind == ValueKind::NotApplicable {
        return;
    }
    if matches!(
        kind,
        ValueKind::Integer | ValueKind::Number | ValueKind::Absent
    ) {
        // An uncertainty is rounded up, and never written as zero.
        zero_check(&value, precision, quantity, at, findings);
        return;
    }
    findings.push(Finding {
        path: at.path.clone(),
        sample: at.sample.clone(),
        severity: Severity::Defect,
        detail: Detail::PrecisionCannotApply {
            quantity: quantity.to_string(),
            specifier: precision.value().as_str().to_string(),
            kind,
        },
    });
}

/// A nonzero value its own precision writes as all zeros.
///
/// Every surface writes that zero, so the declaration says *nothing was
/// measured* wherever the number is shown, and this note is how a reader learns
/// the zero is the declaration's.
fn zero_check(
    value: &Value,
    precision: &Precision,
    quantity: &str,
    at: &Where,
    findings: &mut Vec<Finding>,
) {
    let number = match value {
        Value::Number(number) => *number,
        // An integer is written by `d` or by a fixed specifier, and neither
        // rounds a nonzero whole number away.
        _ => return,
    };
    if number == 0.0 {
        return;
    }
    let resolved = Resolved {
        unit: None,
        symbol: None,
        property: None,
        uncertainty: None,
        unitless: None,
        unit_alone: None,
        unit_in_form: None,
        precision: Some(precision.clone()),
    };
    let as_written = formatting::format_value(value, &resolved);
    if as_written.bytes().any(|byte| matches!(byte, b'1'..=b'9')) {
        return;
    }
    findings.push(Finding {
        path: at.path.clone(),
        sample: at.sample.clone(),
        severity: Severity::Note,
        detail: Detail::PrecisionWritesZero {
            quantity: quantity.to_string(),
            specifier: precision.value().as_str().to_string(),
            value: written(number),
        },
    });
}

/// A query, a profile or a view naming what no sample holds: it fails, or shows
/// a column of nothing, on the day it is used. A collection of one cannot
/// answer what the collection holds, so it is not asked. Braces of a template
/// that look like a channel misspelt. A brace that names no channel is LaTeX's
/// and is written as it stands, so `{valeu}` prints `{valeu}` in a manuscript
/// table with nothing said. What separates it from LaTeX is where it stands:
/// `\text{valeur}` is a command's argument, and a placeholder never follows a
/// command's name.
fn template_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::project_config::FILENAME;
    const CHANNELS: [&str; 5] = ["value", "uncertainty", "u", "unit", "symbol"];
    let Some(config) = list.config() else {
        return Vec::new();
    };
    let file = config.root().join(FILENAME);
    let mut findings = Vec::new();
    let mut look = |declaration: String, template: &str| {
        let mut rest = template;
        let mut before = ' ';
        while let Some(open) = rest.find('{') {
            if let Some(last) = rest[..open].chars().last() {
                before = last;
            }
            let after = &rest[open + 1..];
            let Some(close) = after.find(['{', '}']) else {
                break;
            };
            let closed = after[close..].starts_with('}');
            let written = after[..close].split(':').next().unwrap_or_default();
            // Not a LaTeX argument, `\textbf{x}`, nor a subscript, `_{max}`.
            let stands_alone = !before.is_alphanumeric() && !matches!(before, '_' | '^');
            if closed
                && stands_alone
                && !written.is_empty()
                && written.chars().all(|c| c.is_ascii_alphabetic())
                && !CHANNELS.contains(&written)
            {
                let channel = near_channel(written);
                findings.push(Finding {
                    path: Some(file.clone()),
                    sample: FILENAME.to_string(),
                    severity: Severity::Note,
                    detail: Detail::TemplateBraceNearAChannel {
                        declaration: declaration.clone(),
                        written: written.to_string(),
                        channel,
                    },
                });
            }
            before = '{';
            rest = after;
        }
    };
    for name in config.profile_names() {
        if let Ok(profile) = config.profile(name) {
            for column in profile.columns() {
                if let Some(template) = &column.template {
                    look(
                        format!("profile '{name}', column '{}'", column.field),
                        template,
                    );
                }
            }
        }
    }
    findings
}

/// The channel a word is a slip of the hand away from: two edits at most, which
/// is what a transposition costs — `valeu`, `untis`. `identifier::nearest` is
/// stricter, as a suggestion among a collection's many names has to be; here the
/// candidates are four words. `u` is left out: every one-letter word is near it.
fn near_channel(written: &str) -> Option<String> {
    if written.chars().count() < 4 {
        return None;
    }
    ["value", "uncertainty", "unit", "symbol"]
        .into_iter()
        .map(|channel| (identifier::edit_distance(channel, written), channel))
        .filter(|(distance, _)| *distance <= 2)
        .min()
        .map(|(_, channel)| channel.to_string())
}

/// A `[property.*]` precision or symbol the files override with a different one
/// of their own.
///
/// **A file wins over the project**, and nothing said so. The owner declared
/// `precision = ['.3f', '.5f']` for a quantity every file writes `.3f` on, saw
/// nothing change, and reasonably concluded the syntax was wrong. It was right;
/// the rule was invisible. One note per quantity and channel, not one per file:
/// a declaration shadowed in seventy files is one shadowed declaration.
/// A profile column's own precision, checked as a quantity's is: `.3f` on a
/// hardness known to ±0.00033 writes `± 0.000` into every file the profile
/// exports — a claim of a perfect measurement — and was noted only where
/// `[property.*]` declared it.
fn profile_precision_findings(list: &SampleList) -> Vec<Finding> {
    let Some(config) = list.config() else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for name in config.profile_names() {
        let Ok(profile) = config.profile(name) else {
            continue;
        };
        for column in &profile.columns {
            let Some(precision) = column
                .precision
                .as_ref()
                .and_then(PrecisionSchema::precision)
            else {
                continue;
            };
            let Ok(Field::Named {
                name: quantity,
                channel: Channel::Value,
            }) = crate::query::field_addressing::parse(&column.field)
            else {
                continue;
            };
            for entry in list.iter() {
                let sample = entry.sample.borrow();
                let Ok(handle) = sample.property(&quantity) else {
                    continue;
                };
                let at = Where {
                    path: entry.path.clone(),
                    sample: name_of(&sample, entry.path.as_deref()),
                };
                handle.peek(|property| {
                    precision_check(
                        property,
                        Some(&precision),
                        &format!("profile {name}: {quantity}"),
                        &at,
                        &mut findings,
                    );
                });
            }
        }
    }
    findings
}

fn shadowed_findings(list: &SampleList) -> Vec<Finding> {
    let Some(config) = list.config() else {
        return Vec::new();
    };
    // (quantity, channel) -> declared, what the files write, how many, where.
    let mut seen: BTreeMap<(String, &'static str), (String, String, usize, Where)> =
        BTreeMap::new();
    for entry in list.iter() {
        let sample = entry.sample.borrow();
        let at = Where {
            path: entry.path.clone(),
            sample: name_of(&sample, entry.path.as_deref()),
        };
        for name in sample.property_names() {
            let Some(declaration) = config.property(name.as_str()) else {
                continue;
            };
            let _ = sample.property(name).map(|handle| {
                handle.with(|property| {
                    let presentation = property.presentation();
                    // A symbol alone: a file holds no precision to shadow one
                    // with.
                    for (channel, declared, written) in [(
                        "symbol",
                        declaration.symbol.clone(),
                        presentation.symbol.clone(),
                    )] {
                        let (Some(declared), Some(written)) = (declared, written) else {
                            continue;
                        };
                        // The same spelling in both places is redundancy, not a
                        // shadow: nothing is lost by it.
                        if declared == written {
                            continue;
                        }
                        let key = (name.to_string(), channel);
                        match seen.get_mut(&key) {
                            Some((_, _, count, _)) => *count += 1,
                            None => {
                                seen.insert(key, (declared, written, 1, at.clone()));
                            }
                        }
                    }
                })
            });
        }
    }
    seen.into_iter()
        .map(
            |((quantity, channel), (declared, written, samples, at))| Finding {
                path: at.path,
                sample: at.sample,
                severity: Severity::Note,
                detail: Detail::DeclarationIsShadowed {
                    quantity,
                    channel,
                    declared,
                    written,
                    samples,
                },
            },
        )
        .collect()
}

fn declaration_findings(list: &SampleList) -> Vec<Finding> {
    use crate::collection::sample_list::{FieldWarning, check_figure, check_profile};
    use crate::config::project_config::FILENAME;
    use crate::query::filter_language as filter;

    let Some(config) = list.config() else {
        return Vec::new();
    };
    // A collection of none cannot answer for any field; files named one by
    // one are judged by `run_on_files`, which does not ask here.
    if list.is_empty() {
        return Vec::new();
    }
    let file = config.root().join(FILENAME);
    // A defect on the configuration's own directory, or for a configuration
    // given with --rc, which describes every sample read; a note on a narrower
    // target, whose samples cannot answer for the rest.
    let own =
        |path: &std::path::Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let severity = if list.configuration_forced()
        || list
            .root()
            .is_some_and(|root| own(root) == own(config.root()))
    {
        Severity::Defect
    } else {
        Severity::Note
    };
    let finding = |declaration: String, reason: String| Finding {
        path: Some(file.clone()),
        sample: FILENAME.to_string(),
        severity,
        detail: Detail::DeclarationNamesNoField {
            declaration,
            reason,
        },
    };
    // A declaration this file imports is the importing file's to answer for
    // where it is judged with every project importing it: here, over this
    // project's samples alone, a note, said of the file declaring it. It was
    // a defect of this project's file, which does not hold it, and a root
    // imported by two models failed where each project passed.
    let imported =
        |kind: crate::config::project_config::DeclarationKind, name: &str, found: Finding| {
            match config.origin(kind, name) {
                crate::config::project_config::DeclaredIn::Imported(declaring) => Finding {
                    path: Some(declaring),
                    severity: Severity::Note,
                    ..found
                },
                _ => found,
            }
        };
    let mut findings = Vec::new();
    // What the model's source declares, read as text: a value it declares and
    // no sample holds yet is not a misspelling, and a figure it defines is one
    // a declaration may not share a name with.
    let (model_figures, model_values) = crate::config::model_runtime::model_declarations(config);
    let declared_by_the_model = |warning: &FieldWarning| match warning {
        FieldWarning::Unknown { field, .. } => {
            let quantity = field.split(['.', '[']).next().unwrap_or(field);
            let column = field
                .split('[')
                .next()
                .and_then(|head| head.split('.').nth(1));
            model_values.iter().any(|value| value == quantity)
                || column.is_some_and(|column| model_values.iter().any(|value| value == column))
        }
        _ => false,
    };
    let borrowed: Vec<_> = list.iter().map(|entry| entry.sample.borrow()).collect();
    let samples: Vec<&crate::core::sample::Sample> =
        borrowed.iter().map(|sample| &**sample).collect();
    for name in config.query_names() {
        let Ok(query) = config.query(name) else {
            continue;
        };
        if let Some(directory) = &query.directory
            && !config.resolve(&directory.to_string_lossy()).is_dir()
        {
            findings.push(imported(
                crate::config::project_config::DeclarationKind::Query,
                name,
                finding(
                    format!("query '{name}'"),
                    format!(
                        "its directory '{}' is no folder: the query would select from nothing",
                        directory.display()
                    ),
                ),
            ));
        }
        match filter::parse(&query.filter) {
            Err(error) => findings.push(imported(
                crate::config::project_config::DeclarationKind::Query,
                name,
                finding(format!("query '{name}'"), error.to_string()),
            )),
            Ok(parsed) => {
                for error in filter::check(&parsed, &samples).unknown_fields {
                    let declared = match &error {
                        crate::query::field_addressing::FieldError::UnknownProperty {
                            name,
                            ..
                        } => model_values.iter().any(|value| value == name),
                        _ => false,
                    };
                    if !declared {
                        findings.push(imported(
                            crate::config::project_config::DeclarationKind::Query,
                            name,
                            finding(format!("query '{name}'"), error.to_string()),
                        ));
                    }
                }
            }
        }
    }
    for name in config.profile_names() {
        let Ok(profile) = config.profile(name) else {
            continue;
        };
        for warning in check_profile(list, profile) {
            if !declared_by_the_model(&warning) {
                findings.push(imported(
                    crate::config::project_config::DeclarationKind::Profile,
                    name,
                    finding(format!("profile '{name}'"), warning_text(&warning)),
                ));
            }
        }
    }
    // A declaration of an attribute: a text has no unit, and `style [kg]`
    // headed a column of alloy styles.
    for name in config.property_names() {
        let Some(declaration) = config.property(name) else {
            continue;
        };
        let Ok(identifier) = crate::core::identifier::Identifier::new(name) else {
            continue;
        };
        let attributes = samples
            .iter()
            .filter(|sample| sample.has_attribute(&identifier))
            .count();
        if attributes > 0 && (declaration.unit.is_some() || declaration.precision.is_some()) {
            findings.push(Finding {
                severity: Severity::Note,
                ..finding(
                    format!("[property.{name}]"),
                    format!(
                        "{name} is an attribute in {}, and an attribute has no unit or \
                         precision: [property.*] describes a property",
                        if attributes == 1 {
                            "1 sample".to_string()
                        } else {
                            format!("{attributes} samples")
                        }
                    ),
                )
            });
        }
    }
    for name in config.figure_names() {
        let Some(figure) = config.figure(name) else {
            continue;
        };
        for warning in check_figure(list, figure) {
            if !declared_by_the_model(&warning) {
                findings.push(imported(
                    crate::config::project_config::DeclarationKind::Figure,
                    name,
                    finding(format!("figure '{name}'"), warning_text(&warning)),
                ));
            }
        }
        if model_figures.iter().any(|figure| figure == name) {
            findings.push(imported(
                crate::config::project_config::DeclarationKind::Figure,
                name,
                finding(
                    format!("figure '{name}'"),
                    "the model defines a figure of the same name: rename one of them".to_string(),
                ),
            ));
        }
        if let Some(query) = &figure.query
            && config.query(query).is_err()
        {
            findings.push(imported(
                crate::config::project_config::DeclarationKind::Figure,
                name,
                finding(
                    format!("figure '{name}'"),
                    format!(
                        "no query '{query}' is declared{}",
                        crate::core::identifier::nearest(query, config.query_names().into_iter())
                            .map(|near| format!(" — did you mean '{near}'?"))
                            .unwrap_or_default()
                    ),
                ),
            ));
        }
    }
    // A `[property.*]` naming what no sample holds and the model does not
    // declare — `ogg`, a column misspelt — was accepted in silence, as were a
    // variant for a style one slip from a declared one.
    let vocabulary = list.vocabulary();
    let known: Vec<String> = list
        .available_fields()
        .iter()
        .map(crate::query::field_addressing::describe)
        .collect();
    let styles = config.render_style_names();
    // A declaration names a column `table.column`, never one cell of it.
    let columns: Vec<String> = vocabulary
        .tables_as_strings()
        .into_iter()
        .flat_map(|table| {
            let columns = crate::core::identifier::Identifier::new(&table)
                .map(|id| vocabulary.columns_as_strings(&id))
                .unwrap_or_default();
            columns
                .into_iter()
                .map(move |column| format!("{table}.{column}"))
        })
        .collect();
    let near = |written: &str, among: &[String]| {
        crate::core::identifier::nearest(written, among.iter().map(String::as_str))
            .map(|near| format!(" — did you mean '{near}'?"))
            .unwrap_or_default()
    };
    let style_names: Vec<String> = styles.iter().map(|style| (*style).to_string()).collect();
    // Only what this file writes: a root's declarations are for every project
    // importing it, and one project holding none of a quantity is no mistake.
    let written: Option<toml::Table> = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| text.parse().ok());
    let own = |section: &str, name: &str| {
        written
            .as_ref()
            .and_then(|table| table.get(section)?.as_table()?.get(name))
            .is_some()
    };
    // A note, whatever the target: a declaration may come before the data.
    let finding = |declaration: String, reason: String| Finding {
        severity: Severity::Note,
        ..finding(declaration, reason)
    };
    for name in config
        .property_names()
        .into_iter()
        .filter(|name| own("property", name))
    {
        let declaration = format!("[property.{name}]");
        // A reserved field is described too: `[property.name]` titles a
        // legend grouped by name.
        if matches!(
            name,
            "name" | "path" | "filename" | "project" | "state" | "tags"
        ) {
            // Its symbol labels it; a unit, a precision or a range give a
            // text nothing, and passed in silence.
            if let Some(declared) = config.property(name) {
                let mut given: Vec<&str> = Vec::new();
                if declared.unit.is_some() {
                    given.push("unit");
                }
                if declared.precision.is_some() {
                    given.push("precision");
                }
                if declared.range.is_some() {
                    given.push("range");
                }
                if !given.is_empty() {
                    findings.push(finding(
                        declaration.clone(),
                        format!(
                            "{} {}: '{name}' is SampleKit's own field, a text, which a {} \
                             applies nothing to",
                            given.join(", "),
                            if given.len() == 1 {
                                "is declared"
                            } else {
                                "are declared"
                            },
                            given.join(" or ")
                        ),
                    ));
                }
            }
            continue;
        }
        let held = match name.split_once('.') {
            None => crate::core::identifier::Identifier::new(name)
                .is_ok_and(|id| vocabulary.has_name(&id) && !vocabulary.has_table(&id)),
            Some((table, column)) => {
                match (
                    crate::core::identifier::Identifier::new(table),
                    crate::core::identifier::Identifier::new(column),
                ) {
                    (Ok(table), Ok(column)) => vocabulary.has_column(&table, &column),
                    _ => false,
                }
            }
        };
        let quantity = name.split('.').next_back().unwrap_or(name);
        let by_the_model = model_values
            .iter()
            .any(|value| value == name || value == quantity);
        // A quantity declared before any sample holds it is ordinary: only
        // one a slip from a field held is noted, `ogg` beside `radius`.
        if !held && !by_the_model {
            let reason = match crate::core::identifier::Identifier::new(name) {
                Ok(id) if vocabulary.has_table(&id) => Some(format!(
                    "'{name}' is a table: a declaration describes one of its columns, \
                     [property.\"{name}.<column>\"]"
                )),
                _ => {
                    let slip = near(name, if name.contains('.') { &columns } else { &known });
                    (!slip.is_empty()).then(|| format!("no sample has '{name}'{slip}"))
                }
            };
            if let Some(reason) = reason {
                findings.push(finding(declaration.clone(), reason));
            }
        }
        if let Some(declared) = config.property(name) {
            for key in declared.symbol_variants.keys() {
                let style = key.trim_start_matches("symbol_");
                // A variant for a style declared later, or by a project
                // importing this one, is no mistake: a note, `symbol_mth`
                // with the style it is a slip from, `symbol_nope` alone.
                if !styles.contains(&style) {
                    findings.push(finding(
                        declaration.clone(),
                        format!(
                            "{key}: no style '{style}' is declared{}",
                            unwritten(&near(style, &style_names))
                        ),
                    ));
                }
            }
        }
    }
    for spelling in config
        .unit_vocabulary()
        .into_iter()
        .filter(|unit| own("unit", unit))
    {
        for key in config
            .unit(spelling)
            .map(|variants| variants.keys())
            .into_iter()
            .flatten()
        {
            if !styles.contains(&key.as_str()) {
                findings.push(finding(
                    format!("[unit.\"{spelling}\"]"),
                    format!(
                        "{key}: no style '{key}' is declared{}",
                        unwritten(&near(key, &style_names))
                    ),
                ));
            }
        }
    }
    findings
}

/// What follows *no style is declared*: the style it is a slip from, else
/// that no surface writes the variant until one is.
fn unwritten(slip: &str) -> String {
    if slip.is_empty() {
        ": no surface writes this variant until a [style.NAME] declares it".to_string()
    } else {
        slip.to_string()
    }
}

/// What matplotlib refuses of `[matplotlib]`, asked of the project's Python
/// where the section is — the one case `validate` starts Python: a setting
/// misspelt, `axes.gird`, passed it, and only `plot` refused it after.
fn matplotlib_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::project_config::FILENAME;
    let Some(config) = list.config() else {
        return Vec::new();
    };
    // The section, then each figure's own settings: `plot` refused a key of
    // `[figure.NAME] matplotlib` that `validate` had passed.
    let mut declared = vec![("[matplotlib]".to_string(), config.matplotlib())];
    for name in config.figure_names() {
        if let Some(figure) = config.figure(name)
            && !figure.matplotlib.is_empty()
        {
            declared.push((format!("[figure.{name}] matplotlib"), &figure.matplotlib));
        }
    }
    declared.retain(|(_, settings)| !settings.is_empty());
    let at = |severity: Severity, detail: Detail| Finding {
        path: Some(config.root().join(FILENAME)),
        sample: FILENAME.to_string(),
        severity,
        detail,
    };
    let mut findings = Vec::new();
    for (declaration, settings) in declared {
        match crate::config::model_runtime::matplotlib_problems_of(config, settings) {
            Ok(problems) => findings.extend(problems.into_iter().map(|reason| {
                let reason = reason
                    .strip_prefix("[matplotlib] ")
                    .map_or_else(|| reason.clone(), str::to_string);
                at(
                    Severity::Defect,
                    Detail::DeclarationUnusable {
                        declaration: declaration.clone(),
                        reason,
                    },
                )
            })),
            // Python unreachable is said once, not once per figure.
            Err(reason) => {
                return vec![at(Severity::Note, Detail::MatplotlibNotChecked { reason })];
            }
        }
    }
    findings
}

/// Declared exports a run would contradict: an `output` whose extension names
/// another format than `format` — `-o` refuses it, and the declaration wrote
/// CSV into a `.json` — and two exports writing one file, the second replacing
/// the first without a word.
fn export_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::project_config::FILENAME;
    use crate::config::project_config::{Export, Format};

    let Some(config) = list.config() else {
        return Vec::new();
    };
    let finding = |name: &str, reason: String| Finding {
        path: Some(config.root().join(FILENAME)),
        sample: FILENAME.to_string(),
        severity: Severity::Defect,
        detail: Detail::DeclarationUnusable {
            declaration: format!("[export.{name}]"),
            reason,
        },
    };
    let spelled = |format: &Format| match format {
        Format::Csv => "CSV".to_string(),
        Format::Tsv => "TSV".to_string(),
        Format::Json => "JSON".to_string(),
        Format::Text(text) => format!("the style {}'s document", text.name),
    };
    let mut findings = Vec::new();
    let mut outputs: Vec<(String, &str)> = Vec::new();
    for name in config.export_names() {
        let Ok(export) = config.export(name) else {
            continue;
        };
        // A figure's: one the project or its model declares.
        if let Export::Figure(figure) = export
            && config.figure(&figure.figure).is_none()
        {
            let (of_the_model, _) = crate::config::model_runtime::model_declarations(config);
            if !of_the_model.contains(&figure.figure) {
                let mut figures: Vec<String> = config
                    .figure_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect();
                figures.extend(of_the_model);
                findings.push(finding(
                    name,
                    format!(
                        "figure = \"{}\" names no figure: {}",
                        figure.figure,
                        if figures.is_empty() {
                            "neither .samplekitrc nor the model declares one".to_string()
                        } else {
                            format!("the figures are {}", figures.join(", "))
                        }
                    ),
                ));
            }
        }
        let output = export.output().to_string_lossy().into_owned();
        if output == "-" {
            continue;
        }
        // `{project}` is `.` in the project's own file: `{project}.tsv`
        // wrote `..tsv`, a file hidden from a listing.
        if let Some(file) = export.output().file_name().and_then(|name| name.to_str())
            && file.starts_with('.')
        {
            findings.push(Finding {
                severity: Severity::Note,
                ..finding(
                    name,
                    format!(
                        "it writes {file}, a file a listing hides — {{project}} is '.' in \
                         the project's own file: name the file, and the project in its \
                         folder, out/{{project}}/table.csv"
                    ),
                )
            });
        }
        let named = export
            .output()
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "csv" => Some(Format::Csv),
                "tsv" => Some(Format::Tsv),
                "json" => Some(Format::Json),
                _ => None,
            });
        if let Some(named) = named
            && let Export::Table(table) = export
            && named != table.format
        {
            findings.push(finding(
                name,
                format!(
                    "output = \"{output}\" names {}, and format writes {}",
                    spelled(&named),
                    spelled(&table.format)
                ),
            ));
        }
        // Compared by the file it names: `out/./overview.csv` is
        // `out/overview.csv`, and passed.
        let mut same = PathBuf::new();
        for part in export.output().components() {
            match part {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir if same.file_name().is_some() => {
                    same.pop();
                }
                other => same.push(other),
            }
        }
        let same = same.to_string_lossy().into_owned();
        if let Some((_, first)) = outputs.iter().find(|(held, _)| *held == same) {
            findings.push(finding(
                name,
                format!(
                    "output = \"{output}\" is [export.{first}]'s too: the second written replaces \
                     the first"
                ),
            ));
        } else {
            outputs.push((same, name));
        }
    }
    findings
}

/// A configuration's declarations identical to what it imports: a copy to
/// remove, noted on the importing file.
fn copy_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::project_config::FILENAME;

    let Some(config) = list.config() else {
        return Vec::new();
    };
    config
        .copies()
        .iter()
        .map(|copy| Finding {
            path: Some(config.root().join(FILENAME)),
            sample: FILENAME.to_string(),
            severity: Severity::Note,
            detail: Detail::CopiedDeclaration {
                key: copy.key.clone(),
                from: copy.from.clone(),
            },
        })
        .collect()
}

/// What is wrong with the `[collection] files` entries: a sample's files never
/// found are said nowhere else.
fn files_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::project_config::FILENAME;

    let Some(config) = list.config() else {
        return Vec::new();
    };
    // A folder of no sample has no files to find: a root's
    // `{project}/photos` read from the root was said to name nothing. An
    // `include` or `exclude` that does not read is said all the same: it may
    // be why no sample is read.
    let empty = list.is_empty();
    crate::config::discovery::files_entry_problems(config)
        .into_iter()
        .filter(|problem| {
            !empty
                || problem.entry.starts_with("include '")
                || problem.entry.starts_with("exclude '")
        })
        .map(|problem| {
            // An entry an imported file declares is that file's, quoted as
            // it writes it, `{project}/photos`.
            let declared = config.files_declared().and_then(|(file, written)| {
                let at = config
                    .collection()
                    .files
                    .iter()
                    .position(|entry| *entry == problem.entry)?;
                Some((file.to_path_buf(), written.get(at)?.clone()))
            });
            let (path, entry) = match declared {
                Some((file, written)) => (file, written),
                None => (config.root().join(FILENAME), problem.entry),
            };
            Finding {
                path: Some(path),
                sample: FILENAME.to_string(),
                severity: if problem.defect {
                    Severity::Defect
                } else {
                    Severity::Note
                },
                detail: Detail::FilesPatternUnusable {
                    entry,
                    reason: problem.reason,
                },
            }
        })
        .collect()
}

/// How each sample of the list stands, as far as its files say, in the list's
/// order: its values' freshness as `status` says it without the model — a stale
/// value whose inputs were emptied waiting for them — and `defective` where
/// `run` finds a defect in its file. The model is not read: a caller that reads
/// it adds what it owes.
pub fn states(list: &SampleList) -> Vec<States> {
    use crate::format::fingerprint::Freshness;

    let report = run(list);
    list.iter()
        .map(|entry| {
            let sample = entry.sample.borrow();
            let mut held: Vec<State> = crate::collection::editing::not_current(&sample)
                .iter()
                .filter_map(|value| match &value.state {
                    Freshness::Source | Freshness::Current => None,
                    Freshness::Failed { .. } => Some(State::Failed),
                    Freshness::Edited | Freshness::RecordMissing => Some(State::Edited),
                    stale @ (Freshness::Stale { .. }
                    | Freshness::Broken { .. }
                    | Freshness::Unjudged { .. }) => Some(
                        if crate::collection::editing::emptied_inputs(&sample, stale).is_empty() {
                            State::Stale
                        } else {
                            State::Waiting
                        },
                    ),
                })
                .collect();
            let defective = entry.path.is_some()
                && report.findings.iter().any(|finding| {
                    finding.severity == Severity::Defect && finding.path == entry.path
                });
            if defective {
                held.push(State::Defective);
            }
            States::new(held, false)
        })
        .collect()
}

/// The fields one sample alone holds that are a slip from one held by others.
fn field_slips(list: &SampleList) -> Vec<Finding> {
    let mut held: BTreeMap<String, Vec<Where>> = BTreeMap::new();
    let mut name_counts: BTreeMap<String, usize> = BTreeMap::new();
    for entry in list.iter() {
        if let Some(name) = entry.sample.borrow().name() {
            *name_counts.entry(name.to_string()).or_default() += 1;
        }
    }
    for entry in list.iter() {
        let sample = entry.sample.borrow();
        let at = Where {
            path: entry.path.clone(),
            sample: label_of(&sample, entry.path.as_deref(), &name_counts),
        };
        let names: std::collections::BTreeSet<String> = sample
            .property_names()
            .into_iter()
            .chain(sample.attribute_names())
            .chain(sample.table_names())
            .map(|name| name.to_string())
            .collect();
        for name in names {
            held.entry(name).or_default().push(at.clone());
        }
    }
    let common: Vec<&String> = held
        .iter()
        .filter(|(_, holders)| holders.len() >= 2)
        .map(|(name, _)| name)
        .collect();
    let mut findings = Vec::new();
    for (field, holders) in &held {
        let [alone] = holders.as_slice() else {
            continue;
        };
        let Some(near) =
            crate::core::identifier::nearest(field, common.iter().map(|name| name.as_str()))
        else {
            continue;
        };
        findings.push(Finding {
            path: alone.path.clone(),
            sample: alone.sample.clone(),
            severity: Severity::Note,
            detail: Detail::FieldAlike {
                field: field.clone(),
                others: held.get(&near).map_or(0, Vec::len),
                near,
            },
        });
    }
    findings
}

/// The model `[model]` declares, looked for without importing it: its file, and
/// a line declaring its class.
fn model_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::model_runtime::{template_files, template_of};
    use crate::config::project_config::FILENAME;

    let Some(config) = list.config() else {
        return Vec::new();
    };
    let finding = |reason: String| Finding {
        path: Some(config.root().join(FILENAME)),
        sample: FILENAME.to_string(),
        severity: Severity::Defect,
        detail: Detail::ModelUnusable { reason },
    };
    // Read from the project's folder, where `compute` and `plot` read it,
    // never from the folder a command runs in; declared without a model too,
    // for the figures.
    if let Some(python) = config.python()
        && !config.resolve(&python.to_string_lossy()).exists()
    {
        return vec![finding(format!(
            "the interpreter [model] python names, {}, does not exist",
            config.resolve(&python.to_string_lossy()).display()
        ))];
    }
    let Some(template) = template_of(config) else {
        return Vec::new();
    };
    if !template.path().exists() {
        return vec![finding(format!(
            "the model {} does not exist",
            template.path().display()
        ))];
    }
    // `[model] path` names the model's file: a folder, however it holds the
    // model's sources, is none a command can load.
    if template.path().is_dir() {
        return vec![finding(format!(
            "[model] path names the folder {}, and a model is a file: name the file \
             declaring the model's class",
            template.path().display()
        ))];
    }
    if let Some(class) = template.class() {
        let declares = |text: &str| {
            text.lines().any(|line| {
                line.trim_start()
                    .strip_prefix("class ")
                    .and_then(|rest| rest.strip_prefix(class))
                    .is_some_and(|rest| rest.starts_with(['(', ':', ' ']))
            })
        };
        let found = template_files(&template)
            .unwrap_or_default()
            .iter()
            .any(|file| std::fs::read_to_string(file).is_ok_and(|text| declares(&text)));
        if !found {
            return vec![finding(format!(
                "no file of the model declares the class {class}"
            ))];
        }
    }
    // A model with no interpreter declared runs nowhere from the command line:
    // nothing is searched for, and `compute` stopped at it where `validate`
    // said nothing. A note: Python's own scripts run it in their interpreter.
    if config.python().is_none() {
        return vec![Finding {
            severity: Severity::Note,
            ..finding(
                "[model] names a model and no python for the command line to run it in: \
                 compute and plot need one, python = \".venv\" under [model]; Python's \
                 scripts run it in their own"
                    .to_string(),
            )
        }];
    }
    Vec::new()
}

/// Readings a file holds for a value the model computes, as its description
/// says it: the model refuses to load such a file, and the whole project's
/// computation stops on it, where `validate` said nothing.
fn readings_under_formula_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::model_runtime::Origin;
    let mut findings = Vec::new();
    for entry in list.iter() {
        let Some(path) = &entry.path else {
            continue;
        };
        let Some(description) = list
            .config_for(path)
            .and_then(crate::config::model_runtime::current_description)
        else {
            continue;
        };
        let sample = entry.sample.borrow();
        for name in sample.property_names() {
            let computed = description
                .properties
                .get(name.as_str())
                .is_some_and(|declared| {
                    matches!(
                        declared.value,
                        Origin::Formula { .. } | Origin::Quantity { .. }
                    )
                });
            if computed
                && sample
                    .property(name)
                    .is_ok_and(|handle| handle.readings().is_some())
            {
                findings.push(Finding {
                    path: Some(path.clone()),
                    sample: sample
                        .name()
                        .map_or_else(|| path.display().to_string(), str::to_string),
                    severity: Severity::Defect,
                    detail: Detail::ModelUnusable {
                        reason: format!(
                            "{name}: {}",
                            crate::core::property::FillConflict::ReadingsUnderFormula
                        ),
                    },
                });
            }
        }
    }
    findings
}

/// No sample read, and every candidate left out by an `exclude` pattern: a
/// pattern that excludes everything, `exclude = ["*"]`, read as a collection
/// of none.
fn all_excluded_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::discovery::SkipReason;
    if !list.is_empty() {
        return Vec::new();
    }
    let mut patterns: Vec<&str> = Vec::new();
    let mut files = 0;
    for skipped in list.skipped() {
        if let SkipReason::Excluded { pattern } = &skipped.reason {
            // A README excluded is no sample left out.
            let a_sample = std::fs::read_to_string(&skipped.path)
                .is_ok_and(|text| text.trim_start_matches('\u{feff}').starts_with("---"));
            if !a_sample {
                continue;
            }
            files += 1;
            if !patterns.contains(&pattern.as_str()) {
                patterns.push(pattern);
            }
        }
    }
    if files == 0 {
        return Vec::new();
    }
    let Some(config) = list.config() else {
        return Vec::new();
    };
    vec![Finding {
        path: Some(config.root().join(crate::config::project_config::FILENAME)),
        sample: crate::config::project_config::FILENAME.to_string(),
        severity: Severity::Note,
        detail: Detail::DeclarationUnusable {
            declaration: "[collection] exclude".to_string(),
            reason: format!(
                "{} excludes every sample — {} left out, none read",
                patterns
                    .iter()
                    .map(|pattern| format!("'{pattern}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                if files == 1 {
                    "1 file".to_string()
                } else {
                    format!("{files} files")
                }
            ),
        },
    }]
}

/// A file declaring no version is read as the current one, and said.
fn missing_version_findings(list: &SampleList) -> Vec<Finding> {
    let left_out: Vec<&PathBuf> = list
        .skipped()
        .iter()
        .filter(|skipped| {
            matches!(
                skipped.reason,
                crate::config::discovery::SkipReason::NoVersion
            )
        })
        .map(|skipped| &skipped.path)
        .collect();
    let Some(first) = left_out.first() else {
        return Vec::new();
    };
    vec![Finding {
        path: Some((*first).clone()),
        sample: first
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        severity: Severity::Note,
        detail: Detail::MissingVersion {
            files: left_out.len(),
        },
    }]
}

/// What is said of a value outside the range its project declares for it,
/// `[property.NAME] range`: `volume = -5 L is outside its range, 0 to 50 L`.
/// `None` for a value inside it, one that is no number, one in another unit
/// than the declared one — the unit's disagreement is said apart — and a
/// quantity with no range. One sentence for `validate`, `set` and `compute`.
pub fn outside_range(
    config: Option<&crate::config::project_config::ProjectConfig>,
    name: &str,
    value: &Value,
    unit: Option<&str>,
) -> Option<String> {
    let declaration = config?.property(name)?;
    let (low, high) = declaration.range?;
    if unit.is_some() && declaration.unit.is_some() && unit != declaration.unit.as_deref() {
        return None;
    }
    let number = match value {
        Value::Integer(held) => *held as f64,
        Value::Number(held) => *held,
        _ => return None,
    };
    if (low..=high).contains(&number) {
        return None;
    }
    let unit = unit
        .or(declaration.unit.as_deref())
        .map(|unit| format!(" {unit}"))
        .unwrap_or_default();
    let bound = |bound: f64| {
        if bound.is_infinite() {
            if bound > 0.0 {
                "∞".to_string()
            } else {
                "−∞".to_string()
            }
        } else {
            bound.to_string()
        }
    };
    Some(format!(
        "{name} = {number}{unit} is outside its range, {} to {}{unit}",
        bound(low),
        bound(high)
    ))
}

/// A text that reads as a number once its comma is a point, or as it stands:
/// what a spreadsheet or a French keyboard leaves behind. Any text where a unit
/// is declared, since only a measurement has one: a cell given `12.1,12.3,12.2`
/// in a column with a unit was reported by nothing. Any text in a quantity the
/// model declares, `numeric`, whatever other samples hold: a sample is judged
/// by the model's description, never by the others.
fn number_expected(
    property: &crate::core::property::Property,
    quantity: &str,
    unit: Option<&str>,
    numeric: bool,
    at: &Where,
    findings: &mut Vec<Finding>,
) {
    // Where a number is declared, a date or true/false is no number either.
    if unit.is_some() || numeric {
        let shown = match property.peek_value() {
            Some(Value::Boolean(held)) => Some(held.to_string()),
            Some(Value::Date(date)) => Some(date.to_string()),
            Some(Value::DateTime(moment)) => Some(moment.to_string()),
            _ => None,
        };
        if let Some(text) = shown {
            findings.push(Finding {
                path: at.path.clone(),
                sample: at.sample.clone(),
                severity: Severity::Defect,
                detail: Detail::NotANumber {
                    quantity: quantity.to_string(),
                    found: text,
                },
            });
            return;
        }
    }
    let Some(Value::Text(text)) = property.peek_value() else {
        return;
    };
    let written = text.trim();
    if unit.is_some()
        || numeric
        || written.parse::<f64>().is_ok()
        || written.replace(',', ".").parse::<f64>().is_ok()
    {
        findings.push(Finding {
            path: at.path.clone(),
            sample: at.sample.clone(),
            severity: Severity::Defect,
            detail: Detail::NumberExpected {
                quantity: quantity.to_string(),
                text: text.clone(),
            },
        });
    }
}

fn warning_text(warning: &crate::collection::sample_list::FieldWarning) -> String {
    use crate::collection::sample_list::FieldWarning;
    match warning {
        FieldWarning::Unknown { field, suggestion } => match suggestion {
            Some(suggestion) => format!("no sample has '{field}' — did you mean '{suggestion}'?"),
            None => format!("no sample has '{field}'"),
        },
        FieldWarning::TableNeedsCell { table } => {
            format!("'{table}' names a table; a cell is {table}.<column>[<index>]")
        }
        FieldWarning::NoRow { field, .. } => format!("no sample has the row '{field}'"),
        FieldWarning::NoItem { field, .. } => format!("no sample has the item '{field}'"),
    }
}

// ------------------------------------------------------------------ units

fn remember(
    units: &mut Units,
    configuration: &Option<PathBuf>,
    quantity: &str,
    presentation: &Presentation,
    at: &Where,
) {
    // **A missing unit is not a defect of either kind.** Absence is already
    // visible in the file, and a sample not yet measured is ordinary; only
    // two units disagreeing is evidence of something wrong.
    let Some(unit) = &presentation.unit else {
        return;
    };
    let seen = units
        .entry((configuration.clone(), quantity.to_string()))
        .or_default();
    let counted = seen.entry(unit.clone()).or_insert((0, at.clone()));
    counted.0 += 1;
}

fn unit_findings(units: &Units, list: &SampleList) -> Vec<Finding> {
    let mut findings = Vec::new();
    for ((configuration, quantity), spellings) in units {
        if spellings.len() > 1 {
            let mut counted: Vec<(String, usize)> = spellings
                .iter()
                .map(|(unit, (count, _))| (unit.clone(), *count))
                .collect();
            counted.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
            // The rarest spelling is the file to open first, and usually the
            // mistake.
            let rarest = counted
                .last()
                .and_then(|(unit, _)| spellings.get(unit))
                .map(|(_, at)| at.clone())
                .unwrap_or_else(|| Where {
                    path: None,
                    sample: String::new(),
                });
            findings.push(Finding {
                path: rarest.path,
                sample: rarest.sample,
                severity: Severity::Defect,
                detail: Detail::UnitDisagrees {
                    quantity: quantity.clone(),
                    spellings: counted,
                },
            });
        }
        // **A unit is optional everywhere**. `[unit.*]` declares display forms,
        // keyed by the spelling files write, and a spelling it does not name is
        // shown as written — so it never was a vocabulary, and a project no
        // longer opts into a closed one by wanting a LaTeX form for a single
        // unit.
        //
        // What is a defect is **disagreement**: a file whose unit contradicts
        // the declaration the project makes for that quantity. Invisible
        // before, because only the spellings files wrote were compared with
        // each other: a project declaring `g` while every file wrote `kg`
        // reported nothing at all.
        let Some(declared) = declared_unit(configuration.as_deref(), list, quantity) else {
            continue;
        };
        for (unit, (count, at)) in spellings {
            if unit == &declared {
                continue;
            }
            findings.push(Finding {
                path: at.path.clone(),
                sample: at.sample.clone(),
                severity: Severity::Defect,
                detail: Detail::UnitContradictsDeclaration {
                    quantity: quantity.clone(),
                    written: unit.clone(),
                    declared: declared.clone(),
                    samples: *count,
                },
            });
        }
    }
    findings
}

/// The unit a project declares for one quantity in `[property.*]`, if it
/// declares one.
fn declared_unit(
    configuration: Option<&std::path::Path>,
    list: &SampleList,
    quantity: &str,
) -> Option<String> {
    let own = configuration.and_then(|file| crate::config::project_config::load(file).ok());
    let config = own.as_ref().or(list.config())?;
    config.property(quantity).and_then(|d| d.unit.clone())
}

// ---------------------------------------------------------------- the file

/// A file written differently from its canonical form. `document`'s writer *is*
/// the canonical form, so the check is a comparison and not a second parser.
fn is_canonical(path: &std::path::Path) -> bool {
    let Ok(source) = std::fs::read_to_string(path) else {
        return true;
    };
    match crate::format::document::parse(&source) {
        // A file that does not parse is not this check's business: it is
        // reported by whoever tried to load it, with a better message.
        Err(_) => true,
        Ok(document) => crate::format::document::write(&document) == source,
    }
}

/// A sample's name, and its file beside it where another sample shares the
/// name.
fn label_of(
    sample: &crate::core::sample::Sample,
    path: Option<&std::path::Path>,
    counts: &BTreeMap<String, usize>,
) -> String {
    let base = name_of(sample, path);
    match (sample.name(), path.and_then(|path| path.file_name())) {
        (Some(name), Some(file)) if counts.get(name).is_some_and(|count| *count > 1) => {
            format!("{base} ({})", file.to_string_lossy())
        }
        _ => base,
    }
}

fn name_of(sample: &crate::core::sample::Sample, path: Option<&std::path::Path>) -> String {
    if let Some(name) = sample.name() {
        return name.to_string();
    }
    path.and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A configuration's error as its own line says it: `line 35: …`, where it
/// opened with the whole path of the file the finding already names.
pub fn without_own_path(reason: &str) -> String {
    if let Some((path, rest)) = reason.split_once(".samplekitrc:")
        && !path.contains(' ')
        && let Some((line, said)) = rest.split_once(": ")
        && line.chars().all(|c| c.is_ascii_digit())
    {
        return format!("line {line}: {said}");
    }
    reason.to_string()
}
