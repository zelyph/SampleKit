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
    /// A file of the collection that could not be read at all.
    Unreadable { reason: String },
    /// One `name` held by several files: a report naming the sample cannot
    /// say which.
    DuplicateName { name: String, files: Vec<PathBuf> },
    /// One quantity holding values of different kinds, text among numbers.
    /// Once per quantity, attached to a sample holding the rarest kind.
    KindDisagrees {
        quantity: String,
        kinds: Vec<(String, usize)>,
    },
    /// A query, a profile or a view naming what no sample of the collection
    /// holds.
    DeclarationNamesNoField { declaration: String, reason: String },
    /// A declared model that cannot be found: its file, or its class.
    ModelUnusable { reason: String },
    /// A `[collection] files` entry that cannot find what it means to — a
    /// defect — or whose folder is not there — a note.
    FilesPatternUnusable { entry: String, reason: String },
    /// A declaration a `.samplekitrc` writes as the file it imports already
    /// does: a copy to remove. A `Note`.
    CopiedDeclaration { key: String, from: PathBuf },
    /// A key written twice in one mapping of the frontmatter: the last one
    /// wins, and the first is lost at the next write.
    DuplicateKey { key: String },
    /// A text where a number belongs: a quoted number, or a decimal comma.
    NumberExpected { quantity: String, text: String },
    /// A file that declares no `schema_version`, read as the current one.
    MissingVersion,
    /// A brace in a column's template that stands where a placeholder would and
    /// is one edit or two from a channel's name: `{valeu}`. A `Note` — it is
    /// written out as it stands, and only its author knows whether that was
    /// meant.
    TemplateBraceNearAChannel {
        declaration: String,
        written: String,
        channel: String,
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
            | Detail::KindDisagrees { quantity, .. }
            | Detail::NumberExpected { quantity, .. } => Some(quantity),
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
fn freshness_of(sample: &crate::core::sample::Sample, at: &Where) -> Vec<Finding> {
    use crate::format::fingerprint::Freshness;
    // Counted as `status` counts them: a property once, a table's column once
    // for each way its rows stand.
    let states: Vec<(String, Freshness)> = crate::collection::editing::not_current(sample)
        .into_iter()
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
    let mut kinds: Kinds = BTreeMap::new();
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

        for name in sample.property_names() {
            let field = Field::Named {
                name: name.clone(),
                channel: Channel::Value,
            };
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
                    remember_kind(&mut kinds, &configuration, &quantity, property, &at);
                    let unit = property.presentation().unit.clone();
                    number_expected(property, &quantity, unit.as_deref(), &at, &mut findings);
                });
        }

        for name in sample.attribute_names() {
            if let Ok(crate::core::sample::AttributeValue::Scalar(value)) = sample.attribute(name) {
                remember_value_kind(&mut kinds, &configuration, name.as_str(), value.kind(), &at);
            }
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
                    number_expected(cell, &quantity, unit.as_deref(), &at, &mut findings);
                    precision_check(
                        cell,
                        declared(&format!("{table_name}.{column}")).as_ref(),
                        &quantity,
                        &at,
                        &mut findings,
                    );
                    remember_kind(
                        &mut kinds,
                        &configuration,
                        &format!("{table_name}.{column}"),
                        cell,
                        &at,
                    );
                }
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
        findings.extend(freshness_of(&sample, &at));
    }

    // A file set aside unread is a defect: a report over what could be read
    // must say what could not.
    for skipped in list.skipped() {
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

    // Each configuration answers for the samples it describes.
    if list.spans_configurations() {
        let everything = list.filter_by(|_| true);
        for part in list.by_configuration(&everything) {
            if declarations {
                findings.extend(declaration_findings(&part.samples));
            }
            findings.extend(shadowed_findings(&part.samples));
            findings.extend(template_findings(&part.samples));
            findings.extend(model_findings(&part.samples));
            findings.extend(files_findings(&part.samples));
            findings.extend(copy_findings(&part.samples));
        }
    } else {
        if declarations {
            findings.extend(declaration_findings(list));
        }
        findings.extend(shadowed_findings(list));
        findings.extend(template_findings(list));
        findings.extend(model_findings(list));
        findings.extend(files_findings(list));
        findings.extend(copy_findings(list));
    }
    findings.extend(duplicate_key_findings(list));
    findings.extend(missing_version_findings(list));
    findings.extend(unit_findings(&units, list));
    findings.extend(kind_findings(&kinds));
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
        Detail::TemplateBraceNearAChannel {
            declaration,
            written,
            channel,
        } => format!(
            "{declaration}: {{{written}}} names no channel and is written as it stands — did you mean {{{channel}}}?"
        ),
        Detail::NotCurrent { values } => format!(
            "{} not current: samplekit status names {}",
            counted_as(*values, "value is", "values are"),
            if *values == 1 { "it" } else { "them" }
        ),
        Detail::Overridden { values } => format!(
            "{} over {} formula, kept until compute --force: samplekit status names {}",
            counted_as(*values, "value is written", "values are written"),
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
        Detail::Unreadable { reason } => format!("not read: {reason}"),
        Detail::ModelUnusable { reason } => reason.clone(),
        Detail::FilesPatternUnusable { entry, reason } => {
            format!("[collection] files '{entry}': {reason}")
        }
        Detail::CopiedDeclaration { key, from } => format!(
            "{key} is written as {} already declares it, and imported from there: a copy to \
             remove",
            from.display()
        ),
        Detail::DuplicateKey { key } => {
            format!("'{key}' is written twice: the first value is lost at the next write")
        }
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
        Detail::DuplicateName { name, files } => format!(
            "the name '{name}' is held by {} files: {}",
            files.len(),
            files
                .iter()
                .map(|file| file.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
            } else {
                ""
            }
        ),
        Detail::MissingVersion => {
            "no schema_version: read as 1, and the next write adds it".to_string()
        }
        Detail::KindDisagrees { quantity, kinds } => format!(
            "{quantity}: {} kinds of value — {}",
            kinds.len(),
            kinds
                .iter()
                .map(|(kind, count)| format!("{kind} on {count}"))
                .collect::<Vec<_>>()
                .join(", ")
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
                separator: "\u{b1}".to_string(),
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
        zero_check(&value, precision, quantity, at, findings);
        // **And the uncertainty, under its own half of the precision.** It was
        // never looked at: `u: 0.0004` under `.2f` is written `0.0` in every
        // file and export — a claim of a perfect measurement — while a value
        // of exactly zero beside it left this check with nothing to say.
        if let Ok(Some(uncertainty)) = property.uncertainty()
            && let Ok(number) = Value::number(uncertainty.magnitude())
            && let Ok(own) = Precision::both(precision.uncertainty().as_str())
        {
            zero_check(&number, &own, &format!("{quantity}.u"), at, findings);
        }
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
        separator: "\u{b1}".to_string(),
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

// ------------------------------------------------------------------ kinds

/// The kinds of value a quantity holds, per configuration, as [`Units`] holds
/// its spellings.
type Kinds = BTreeMap<(Option<PathBuf>, String), BTreeMap<&'static str, (usize, Where)>>;

fn remember_kind(
    kinds: &mut Kinds,
    configuration: &Option<PathBuf>,
    quantity: &str,
    property: &Property,
    at: &Where,
) {
    if let Ok(value) = property.value() {
        remember_value_kind(kinds, configuration, quantity, value.kind(), at);
    }
}

/// An attribute is counted as a property is: `2026-13-45` among dates is text.
fn remember_value_kind(
    kinds: &mut Kinds,
    configuration: &Option<PathBuf>,
    quantity: &str,
    kind: ValueKind,
    at: &Where,
) {
    // An integer and a number are two storage forms of one kind, and absence
    // is no kind at all.
    let kind = match kind {
        ValueKind::Integer | ValueKind::Number => "number",
        ValueKind::Text => "text",
        ValueKind::Boolean => "boolean",
        ValueKind::Date => "date",
        ValueKind::DateTime => "date-time",
        ValueKind::Absent | ValueKind::NotApplicable => return,
    };
    let counted = kinds
        .entry((configuration.clone(), quantity.to_string()))
        .or_default()
        .entry(kind)
        .or_insert((0, at.clone()));
    counted.0 += 1;
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
            let stands_alone = !before.is_alphanumeric();
            if closed
                && stands_alone
                && !written.is_empty()
                && written.chars().all(|c| c.is_ascii_alphabetic())
                && !CHANNELS.contains(&written)
                && let Some(channel) = near_channel(written)
            {
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
        match filter::parse(&query.filter) {
            Err(error) => findings.push(finding(format!("query '{name}'"), error.to_string())),
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
                        findings.push(finding(format!("query '{name}'"), error.to_string()));
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
                findings.push(finding(format!("profile '{name}'"), warning_text(&warning)));
            }
        }
    }
    for name in config.figure_names() {
        let Some(figure) = config.figure(name) else {
            continue;
        };
        for warning in check_figure(list, figure) {
            if !declared_by_the_model(&warning) {
                findings.push(finding(format!("figure '{name}'"), warning_text(&warning)));
            }
        }
        if model_figures.iter().any(|figure| figure == name) {
            findings.push(finding(
                format!("figure '{name}'"),
                "the model defines a figure of the same name: rename one of them".to_string(),
            ));
        }
        if let Some(query) = &figure.query
            && config.query(query).is_err()
        {
            findings.push(finding(
                format!("figure '{name}'"),
                format!(
                    "no query '{query}' is declared{}",
                    crate::core::identifier::nearest(query, config.query_names().into_iter())
                        .map(|near| format!(" — did you mean '{near}'?"))
                        .unwrap_or_default()
                ),
            ));
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
    crate::config::discovery::files_entry_problems(config)
        .into_iter()
        .map(|problem| Finding {
            path: Some(config.root().join(FILENAME)),
            sample: FILENAME.to_string(),
            severity: if problem.defect {
                Severity::Defect
            } else {
                Severity::Note
            },
            detail: Detail::FilesPatternUnusable {
                entry: problem.entry,
                reason: problem.reason,
            },
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

/// The model `[model]` declares, looked for without importing it: its file, and
/// a line declaring its class.
fn model_findings(list: &SampleList) -> Vec<Finding> {
    use crate::config::model_runtime::{template_files, template_of};
    use crate::config::project_config::FILENAME;

    let Some(config) = list.config() else {
        return Vec::new();
    };
    let Some(template) = template_of(config) else {
        return Vec::new();
    };
    let finding = |reason: String| Finding {
        path: Some(config.root().join(FILENAME)),
        sample: FILENAME.to_string(),
        severity: Severity::Defect,
        detail: Detail::ModelUnusable { reason },
    };
    if !template.path().exists() {
        return vec![finding(format!(
            "the model {} does not exist",
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
    Vec::new()
}

/// Keys written twice in one mapping, read from the file's text: a parsed
/// document keeps only the last.
fn duplicate_key_findings(list: &SampleList) -> Vec<Finding> {
    let mut findings = Vec::new();
    for entry in list.iter() {
        let Some(path) = &entry.path else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let sample = name_of(&entry.sample.borrow(), Some(path));
        for key in duplicate_keys(&frontmatter_of(&text)) {
            findings.push(Finding {
                path: Some(path.clone()),
                sample: sample.clone(),
                severity: Severity::Defect,
                detail: Detail::DuplicateKey { key },
            });
        }
    }
    findings
}

/// A file declaring no version is read as the current one, and said.
fn missing_version_findings(list: &SampleList) -> Vec<Finding> {
    let mut findings = Vec::new();
    for entry in list.iter() {
        let Some(path) = &entry.path else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if !frontmatter_of(&text)
            .lines()
            .any(|line| line.starts_with("schema_version:"))
        {
            findings.push(Finding {
                path: Some(path.clone()),
                sample: name_of(&entry.sample.borrow(), Some(path)),
                severity: Severity::Note,
                detail: Detail::MissingVersion,
            });
        }
    }
    findings
}

/// A text that reads as a number once its comma is a point, or as it stands:
/// what a spreadsheet or a French keyboard leaves behind. Any text where a unit
/// is declared, since only a measurement has one: a cell given `12.1,12.3,12.2`
/// in a column with a unit was reported by nothing.
fn number_expected(
    property: &crate::core::property::Property,
    quantity: &str,
    unit: Option<&str>,
    at: &Where,
    findings: &mut Vec<Finding>,
) {
    let Some(Value::Text(text)) = property.peek_value() else {
        return;
    };
    let written = text.trim();
    if unit.is_some()
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

fn frontmatter_of(text: &str) -> String {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = body.lines();
    if lines.next().map(|line| line.trim_end_matches('\r')) != Some("---") {
        return String::new();
    }
    lines
        .take_while(|line| line.trim_end_matches('\r') != "---")
        .collect::<Vec<_>>()
        .join("\n")
}

/// The dotted path of every key a block mapping repeats.
///
/// What is text is not read as keys: the body of a block scalar —
/// `protocol: |` above `step: a` and `step: b` — a flow collection, or a
/// quoted scalar, running over several lines. Read as keys, a protocol's
/// steps were reported as a key written twice.
fn duplicate_keys(frontmatter: &str) -> Vec<String> {
    let mut scopes: Vec<(usize, Vec<String>)> = vec![(0, Vec::new())];
    let mut parents: Vec<(usize, String)> = Vec::new();
    let mut found = Vec::new();
    // Lines indented deeper than this are a block scalar's text.
    let mut block: Option<usize> = None;
    // What closes a flow collection or a quoted scalar still open.
    let mut open = Open::default();
    for line in frontmatter.lines() {
        let trimmed = line.trim_start();
        if open.is_open() {
            open.read(line);
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        let at = line.len() - trimmed.len();
        match block {
            Some(parent) if at > parent => continue,
            _ => block = None,
        }
        if trimmed.starts_with('#') {
            continue;
        }
        let mut indent = at;
        let mut body = trimmed;
        // A sequence item opens a mapping of its own.
        if let Some(rest) = body.strip_prefix("- ") {
            indent += 2;
            scopes.retain(|(at, _)| *at < indent);
            parents.retain(|(at, _)| *at < indent);
            scopes.push((indent, Vec::new()));
            body = rest;
            if opens_text(rest.trim(), at, &mut block, &mut open) {
                continue;
            }
        }
        let Some((key, value)) = body.split_once(':') else {
            continue;
        };
        // The key is counted; what follows it may be text.
        opens_text(value.trim(), indent, &mut block, &mut open);
        if key.is_empty() || key.contains(|c: char| c.is_whitespace() || "{[\"'".contains(c)) {
            continue;
        }
        scopes.retain(|(at, _)| *at <= indent);
        parents.retain(|(at, _)| *at < indent);
        if scopes.last().is_none_or(|(at, _)| *at < indent) {
            scopes.push((indent, Vec::new()));
        }
        let scope = scopes.last_mut().expect("a scope was pushed");
        if scope.1.iter().any(|seen| seen == key) {
            let mut path: Vec<&str> = parents.iter().map(|(_, parent)| parent.as_str()).collect();
            path.push(key);
            found.push(path.join("."));
        } else {
            scope.1.push(key.to_string());
        }
        parents.push((indent, key.to_string()));
    }
    found
}

/// Whether `value` begins text that runs over the lines after it: a block
/// scalar, whose lines deeper than `parent` are then skipped, or a flow
/// collection or a quoted scalar left open, which `open` then follows.
fn opens_text(value: &str, parent: usize, block: &mut Option<usize>, open: &mut Open) -> bool {
    let indicator = value.split(" #").next().unwrap_or_default().trim_end();
    if let Some(rest) = indicator.strip_prefix(['|', '>'])
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || c == '+' || c == '-')
    {
        *block = Some(parent);
        return true;
    }
    if value.starts_with(['[', '{', '"', '\'']) {
        open.read(value);
        return open.is_open();
    }
    false
}

/// A flow collection's depth and a quoted scalar's quote, followed from line
/// to line until both close.
#[derive(Default)]
struct Open {
    depth: usize,
    quote: Option<char>,
}

impl Open {
    fn is_open(&self) -> bool {
        self.depth > 0 || self.quote.is_some()
    }

    fn read(&mut self, text: &str) {
        let mut characters = text.chars().peekable();
        // A quote opens a scalar only where a token starts: `don't` is not one.
        let mut starts = true;
        let mut spaced = true;
        while let Some(c) = characters.next() {
            if let Some(quote) = self.quote {
                if quote == '"' && c == '\\' {
                    characters.next();
                } else if c == quote {
                    if quote == '\'' && characters.peek() == Some(&'\'') {
                        characters.next();
                    } else {
                        self.quote = None;
                    }
                }
                continue;
            }
            match c {
                '"' | '\'' if starts => self.quote = Some(c),
                '#' if spaced => return,
                '[' | '{' => self.depth += 1,
                ']' | '}' => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
            starts = c.is_whitespace() || "[{,:".contains(c);
            spaced = c.is_whitespace();
        }
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

/// A quantity holding two kinds: `"ninety"` among numbers, which every formula
/// reading it then fails on.
fn kind_findings(kinds: &Kinds) -> Vec<Finding> {
    let mut findings = Vec::new();
    for ((_, quantity), seen) in kinds {
        if seen.len() < 2 {
            continue;
        }
        let mut counted: Vec<(String, usize)> = seen
            .iter()
            .map(|(kind, (count, _))| (kind.to_string(), *count))
            .collect();
        counted.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
        let Some(rarest) = counted
            .last()
            .and_then(|(kind, _)| seen.get(kind.as_str()))
            .map(|(_, at)| at.clone())
        else {
            continue;
        };
        findings.push(Finding {
            path: rarest.path,
            sample: rarest.sample,
            severity: Severity::Defect,
            detail: Detail::KindDisagrees {
                quantity: quantity.clone(),
                kinds: counted,
            },
        });
    }
    findings
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
