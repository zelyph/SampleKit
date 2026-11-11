//! Writing a profile's table to a file: which columns, in which order, with
//! which headers, and where it goes. It declares no selection — the caller has
//! already made it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::collection::sample_list::{Entry, GroupOrder, ListError, SampleList};
use crate::config::project_config::{ColumnSpec, ExportTarget, Format, Profile, ProjectConfig};
use crate::core::formatting::{self, Precision, Resolved};
use crate::core::value::Value;
use crate::format::fingerprint::{self, Freshness};
use crate::presentation::export_formats::{Cell, Dataset, QuantityColumns, Templated};
use crate::presentation::terminal_rendering;
use crate::query::field_addressing::{self as fields, FieldError, Resolution, Subject, Vocabulary};

/// Whether the numbers are on their way to **text** or to a **caller**.
///
/// A declared precision shapes text, so a file and a table carry it. A dict is
/// not text: `to_dict` hands Python the stored float, and rounding it there
/// would be this module deciding what a script may compute with. Naming the two
/// is what keeps that distinction from being an accident of which function
/// happened to be called.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Numbers {
    /// Rounded to the precision the project declares.
    #[default]
    AsWritten,
    /// Exactly as the file holds them.
    AsStored,
}

/// Builds the dataset. Rows are in the collection's order — the profile's
/// ordering is applied by the caller before this, so that regenerating from
/// unchanged data produces a byte-identical file.
pub fn run(
    target: &ExportTarget,
    profile: &Profile,
    list: &SampleList,
) -> Result<Dataset, ExportError> {
    run_with(target, profile, list, list.config())
}

/// The same, with the configuration whose units the headers show: a selection
/// carries none of its own.
pub fn run_with(
    target: &ExportTarget,
    profile: &Profile,
    list: &SampleList,
    config: Option<&ProjectConfig>,
) -> Result<Dataset, ExportError> {
    run_within(target, profile, list, config, list)
}

/// The same, handing back the stored numbers rather than the written ones.
pub fn run_as_stored(
    target: &ExportTarget,
    profile: &Profile,
    list: &SampleList,
    config: Option<&ProjectConfig>,
) -> Result<Dataset, ExportError> {
    build(target, profile, list, config, list, Numbers::AsStored)
}

/// What a selection holds that is not current, as an export says before it is
/// written: counted and named.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Behind {
    /// Outdated or broken, by the cells they fill.
    pub stale: usize,
    /// Edited by hand, or without a record.
    pub held: usize,
    /// Never computed, which the file writes empty.
    pub never: usize,
    /// Failed, by the cells they fill, written as their last number.
    pub failed: usize,
    /// The samples holding any of them.
    pub samples: usize,
    /// `BR-01 flatness`, each value not current.
    pub named: Vec<String>,
    /// The names of the values edited, which `--accept edited:NAME` names.
    pub edited_values: Vec<String>,
}

impl Behind {
    /// Another selection's, counted with this one: a table over several
    /// projects is written from them all.
    pub fn add(&mut self, other: &Behind) {
        self.stale += other.stale;
        self.held += other.held;
        self.never += other.never;
        self.failed += other.failed;
        self.samples += other.samples;
        self.named.extend(other.named.iter().cloned());
        for value in &other.edited_values {
            if !self.edited_values.contains(value) {
                self.edited_values.push(value.clone());
            }
        }
    }

    /// Whether the file holds a value that is not current.
    pub fn any(&self) -> bool {
        self.stale + self.held + self.never + self.failed > 0
    }

    /// The kinds there are, each counted, as every surface says them: `0
    /// outdated` counted nothing. A failed value was said by none — the
    /// preview of `-o` said nothing of a column that had failed.
    pub fn kinds(&self) -> Vec<String> {
        [
            (self.stale, "outdated"),
            (self.held, "edited or without a record"),
            (self.never, "never computed, written empty"),
            (self.failed, "failed"),
        ]
        .iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, kind)| format!("{count} {kind}"))
        .collect()
    }
}

/// `config` is the project's, where the selection carries none: a list
/// grouped for its profile holds the samples without their configuration.
pub fn behind(selected: &SampleList, profile: &Profile, config: Option<&ProjectConfig>) -> Behind {
    // What the file holds, only: a column the profile does not write is no
    // part of what the export is written from.
    let written: Vec<String> = profile
        .columns()
        .iter()
        .map(|column| {
            column
                .field
                .split('[')
                .next()
                .unwrap_or(&column.field)
                .to_string()
        })
        .collect();
    let exported = |name: &str| {
        written.iter().any(|field| {
            field == name
                || field.starts_with(&format!("{name}."))
                || name.starts_with(&format!("{field}."))
        })
    };
    let mut behind = Behind::default();
    let mut traced = crate::collection::computation::Traced::default();
    for entry in selected.iter() {
        let sample = entry.sample.borrow();
        // A value waiting for an input emptied since is no outdated one: its
        // number is the one computed before, as `status` says — counted
        // outdated, a waiting recovery was sent to `compute`.
        let late: Vec<_> = crate::collection::editing::not_current(&sample)
            .into_iter()
            .filter(|value| exported(&value.name))
            .filter(|value| {
                !matches!(
                    value.state,
                    Freshness::Stale { .. } | Freshness::Broken { .. }
                ) || crate::collection::editing::emptied_inputs(&sample, &value.state).is_empty()
            })
            .collect();
        let tallied = !late.is_empty();
        if tallied {
            behind.samples += 1;
        }
        let counted: Vec<String> = late.iter().map(|value| value.name.clone()).collect();
        for value in late {
            let failed = matches!(value.state, Freshness::Failed { .. });
            // A column whose cells stand in two states is one value named.
            let named = format!(
                "{} {}{}",
                sample.name().unwrap_or("(unnamed)"),
                value.name,
                if failed { ", failed" } else { "" }
            );
            if !behind.named.contains(&named) {
                behind.named.push(named);
            }
            // The cells the file holds of it: a column written whole is its
            // rows, `curve.stress[10]` alone is one.
            let whole = profile
                .columns()
                .iter()
                .any(|column| column.field == value.name);
            let addressed = profile
                .columns()
                .iter()
                .filter(|column| column.field.starts_with(&format!("{}[", value.name)))
                .count();
            let cells = if whole || addressed == 0 {
                value.rows.max(1)
            } else {
                addressed
            };
            match value.state {
                Freshness::Stale { .. } | Freshness::Broken { .. } => behind.stale += cells,
                Freshness::Failed { .. } => behind.failed += cells,
                _ => {
                    behind.held += cells;
                    if !behind.edited_values.contains(&value.name) {
                        behind.edited_values.push(value.name.clone());
                    }
                }
            }
        }
        // What the model owes, as `status` plans it: a formula changed since
        // it computed a value is an outdated one — the file was written from
        // it and the export exited 0 — and a value never computed is written
        // empty.
        let Some(path) = entry.path.as_deref() else {
            continue;
        };
        let planned = crate::collection::computation::planned_states_traced(
            selected.config_for(path).or(selected.config()).or(config),
            path,
            &sample,
            &mut traced,
        )
        .unwrap_or_default();
        let mut owed = false;
        for (value, said) in &planned {
            if !exported(value) || counted.contains(value) {
                continue;
            }
            // A value entered whose uncertainty a formula owes is written,
            // and its uncertainty empty: said so, where the value was said
            // never computed beside its number in the file.
            let mut owing = value.to_string();
            match crate::collection::computation::planned_word(said) {
                Some("outdated") => behind.stale += 1,
                Some(NEVER_COMPUTED) => {
                    behind.never += 1;
                    let entered = crate::core::identifier::Identifier::new(value)
                        .ok()
                        .and_then(|name| sample.property_if(&name))
                        // Readings alone are no value entered: their statistic
                        // is owed too, and the file writes both empty.
                        .is_some_and(|handle| {
                            handle.peek(|property| match property.readings() {
                                Some(_) => property.written_value().is_some(),
                                None => property.peek_value().is_some(),
                            })
                        });
                    if entered {
                        owing = format!("{value}'s uncertainty");
                    }
                }
                _ => continue,
            }
            owed = true;
            behind
                .named
                .push(format!("{} {owing}", sample.name().unwrap_or("(unnamed)")));
        }
        if owed && !tallied {
            behind.samples += 1;
        }
    }
    behind
}

/// A declared export ready to write: its selection narrowed by the query it
/// declares and ordered by its profile, its rows, where it goes, and what in it
/// is not current — what the workbench writes; `samplekit export`, with options
/// of its own, shares `behind` and `run_within` with it.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub dataset: Dataset,
    pub format: Format,
    /// The declared output, resolved against the configuration's directory.
    pub path: PathBuf,
    pub behind: Behind,
}

pub fn prepare(
    name: &str,
    config: &ProjectConfig,
    mut selected: SampleList,
    described: &SampleList,
) -> Result<Prepared, String> {
    let target = config
        .table_export(name)
        .map_err(|error| error.to_string())?
        .clone();
    let profile = config
        .profile(&target.profile)
        .map_err(|error| error.to_string())?
        .clone();
    if let Some(query) = &target.query {
        let declared = crate::collection::named_queries::named(config, query)
            .map_err(|error| error.to_string())?;
        let parsed = crate::query::filter_language::parse(&declared.filter)
            .map_err(|error| error.to_string())?;
        selected = selected
            .filter(&parsed)
            .map_err(|error| error.to_string())?;
    }
    // Its profile's order, so that unchanged data writes the same file.
    if !profile.sort.is_empty() {
        let spec =
            crate::query::ordering::parse_spec(&profile.sort).map_err(|error| error.to_string())?;
        selected.sort(&spec).map_err(|error| error.to_string())?;
    }
    // Its profile's groups, in the order its sort gives them.
    let order = if profile.sort.is_empty() {
        GroupOrder::Values
    } else {
        GroupOrder::Listed
    };
    let (profile, selected) =
        grouped(&profile, &selected, order).map_err(|error| error.to_string())?;
    let behind = behind(&selected, &profile, Some(config));
    let dataset = run_within(&target, &profile, &selected, Some(config), described)
        .map_err(|error| error.to_string())?;
    Ok(Prepared {
        dataset,
        format: target.format,
        path: config.resolve(&target.output.to_string_lossy()),
        behind,
    })
}

/// A profile's groups over a selection ready to write: each group's rows
/// together, in `order`, and the profile carrying the fields grouped by as
/// first columns — one table, as a grouped `--csv` is, since a file has no
/// place for a heading. Unchanged where it groups by nothing.
pub fn grouped(
    profile: &Profile,
    selected: &SampleList,
    order: GroupOrder,
) -> Result<(Profile, SampleList), ListError> {
    if profile.group.is_empty() {
        return Ok((profile.clone(), selected.filter_entries(|_| true)));
    }
    let fields = profile
        .group
        .iter()
        .map(|field| fields::parse(field).map_err(ListError::Field))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = selected.grouped(&fields, order)?;
    Ok((profile.carrying(&profile.group), rows))
}

/// The same over a selection, with the collection it was made from: what says
/// whether a column is a quantity, so that an export has the same columns
/// whatever the selection.
pub fn run_within(
    target: &ExportTarget,
    profile: &Profile,
    list: &SampleList,
    config: Option<&ProjectConfig>,
    described: &SampleList,
) -> Result<Dataset, ExportError> {
    build(target, profile, list, config, described, Numbers::AsWritten)
}

/// A sample's table `name` as rows of a file: each row of each sample's
/// table one row, its cells as a table of samples writes them, its index
/// first; over several samples, a column `name` first. With the profile of
/// every cell written, which says what is not current among them. A sample
/// without the table gives no row.
pub fn table_rows(
    name: &crate::core::identifier::Identifier,
    list: &SampleList,
    config: Option<&ProjectConfig>,
    format: &Format,
    precision: Option<&crate::format::schema::PrecisionSchema>,
    style: Option<String>,
) -> Result<(Dataset, Profile), ExportError> {
    let several = list.len() > 1;
    let mut stacked: Vec<Dataset> = Vec::new();
    let mut every_cell: Vec<ColumnSpec> = Vec::new();
    let column = |field: String, header: Option<String>| ColumnSpec {
        field,
        label: header.clone(),
        header,
        precision: precision.cloned(),
        template: None,
    };
    for entry in list.iter() {
        let one = list.holding(vec![entry.clone()]);
        let (columns, rows) = {
            let sample = entry.sample.borrow();
            let Ok(found) = sample.table(name) else {
                continue;
            };
            let columns: Vec<crate::core::identifier::Identifier> =
                found.column_names().into_iter().cloned().collect();
            (columns, found.rows().count())
        };
        for row in 0..rows {
            let mut specs = Vec::new();
            if several {
                specs.push(ColumnSpec {
                    field: "name".to_string(),
                    label: None,
                    header: None,
                    precision: None,
                    template: None,
                });
            }
            for held in &columns {
                let field = fields::describe(&fields::Field::Cell {
                    table: name.clone(),
                    column: held.clone(),
                    row: crate::core::table::RowAddress::ordinal(row),
                    channel: fields::Channel::Value,
                });
                every_cell.push(column(field.clone(), None));
                specs.push(column(field, Some(held.to_string())));
            }
            let profile = crate::config::profiles::anonymous(specs);
            let target = ExportTarget {
                name: profile.name.clone(),
                profile: profile.name.clone(),
                format: format.clone(),
                output: PathBuf::from("-"),
                filename: false,
                path: false,
                query: None,
                style: style.clone(),
            };
            stacked.push(run_within(&target, &profile, &one, config, &one)?);
        }
    }
    Ok((
        crate::presentation::export_formats::stacked(&stacked),
        crate::config::profiles::anonymous(every_cell),
    ))
}

fn build(
    target: &ExportTarget,
    profile: &Profile,
    list: &SampleList,
    config: Option<&ProjectConfig>,
    described: &SampleList,
    numbers: Numbers,
) -> Result<Dataset, ExportError> {
    let mut headers = Vec::new();
    if target.filename {
        headers.push("filename".to_string());
    }
    if target.path {
        headers.push("path".to_string());
    }
    // What exists is what the collection holds: a selection narrowed to
    // samples that all lack `porosity` has not made `porosity` a misspelling.
    let vocabulary = described.vocabulary();
    // The emitted names, which is the profile's business: a label renames a
    // terminal column and never a file's. Whether a column flattens into two
    // is *this* layer's, because only a quantity has two halves and finding
    // out means resolving a channel.
    let borrowed: Vec<_> = list.iter().map(|entry| entry.sample.borrow()).collect();
    let subjects: Vec<Subject> = list
        .iter()
        .zip(borrowed.iter())
        .map(|(entry, sample)| Subject {
            sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        })
        .collect();
    let described_borrowed: Vec<_> = described
        .iter()
        .map(|entry| entry.sample.borrow())
        .collect();
    let described_subjects: Vec<Subject> = described
        .iter()
        .zip(described_borrowed.iter())
        .map(|(entry, sample)| Subject {
            sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        })
        .collect();
    let mut units: Vec<Option<String>> = vec![None; headers.len()];
    let mut quantities = Vec::new();
    let mut quantity_fields = Vec::new();
    let mut plan: Vec<(&ColumnSpec, Vec<String>)> = Vec::new();
    // The columns a template writes, and the dataset's columns each stands for.
    let mut templated_columns: Vec<(&ColumnSpec, Vec<usize>)> = Vec::new();
    let unmade = NeverComputed::of(described, config);
    // Over several projects each may spell a unit its own way, and the first
    // target's spelling headed every row: the files' own spelling instead,
    // which no order of the targets changes.
    let spelled_by = if described.spans_configurations() || list.spans_configurations() {
        None
    } else {
        config
    };
    // **Two units in one column are refused**, as a table refuses them: written,
    // the file held litres and millilitres under a header naming neither.
    // Units as the files spell them in a data file; in a document, as the
    // style writing it spells them.
    let spelled_in = match (&target.format, target.style.as_deref()) {
        (_, Some(style)) => Some(style),
        (crate::config::project_config::Format::Text(text), None) => Some(text.name.as_str()),
        (_, None) => Some(crate::config::project_config::FILES_STYLE),
    };
    // A template is written for a document: in the project's style, as the
    // terminal writes it, unless the export or its format says another.
    let templated_in = match (&target.format, target.style.as_deref()) {
        (_, Some(style)) => Some(style),
        (crate::config::project_config::Format::Text(text), None) => Some(text.name.as_str()),
        (_, None) => None,
    };
    let mut labels: Vec<String> = headers.clone();
    let mut group_columns: Vec<usize> = Vec::new();
    let mut json_keys = std::collections::BTreeMap::new();
    let mixed = terminal_rendering::disagreeing_units(profile, &subjects, spelled_by, spelled_in);
    if !mixed.is_empty() {
        return Err(ExportError::MixedUnits { columns: mixed });
    }
    for column in profile.columns() {
        let emitted = profile.headers_for(column, has_uncertainty(column, described)?);
        // As the files spell it, whatever style the project renders in: a
        // file's header is read by programs.
        // Over several projects, each row's unit is its own project's: one
        // every project declares alike heads the column.
        let unit = if spelled_by.is_none() && config.is_some() {
            terminal_rendering::column_unit_each(
                column,
                &subjects,
                profile,
                &configs_of(list),
                spelled_in,
            )
            .or_else(|| {
                terminal_rendering::column_unit_each(
                    column,
                    &described_subjects,
                    profile,
                    &configs_of(described),
                    spelled_in,
                )
            })
        } else {
            terminal_rendering::column_unit(column, &subjects, profile, spelled_by, spelled_in)
                // No selected row writes one: the collection's, so that a header
                // does not lose its unit with its last row.
                .or_else(|| {
                    terminal_rendering::column_unit(
                        column,
                        &described_subjects,
                        profile,
                        spelled_by,
                        spelled_in,
                    )
                })
        };
        // A value a formula gives is a quantity before it holds an
        // uncertainty, or any value: its state is said from the first.
        let formula_gives = match fields::parse(&column.field) {
            Ok(fields::Field::Named {
                ref name,
                channel: fields::Channel::Value,
            }) => unmade.gives(name.as_str()),
            Ok(fields::Field::Cell {
                ref table,
                ref column,
                channel: fields::Channel::Value,
                ..
            }) => unmade.gives(&format!("{table}.{column}")),
            _ => false,
        };
        if emitted.len() == 2 || formula_gives {
            quantities.push(QuantityColumns {
                key: column
                    .header
                    .clone()
                    .unwrap_or_else(|| column.field.clone()),
                value: headers.len(),
                uncertainty: (emitted.len() == 2).then_some(headers.len() + 1),
                states: Vec::new(),
            });
            quantity_fields.push(fields::parse(&column.field).map_err(ExportError::Field)?);
        }
        // Two columns writing one header could not be told apart in the
        // file, and a reader keyed by name keeps one: refused, naming both.
        for header in &emitted {
            if let Some((earlier, _)) = plan.iter().find(|(_, written)| written.contains(header)) {
                return Err(ExportError::DuplicateHeader {
                    header: header.clone(),
                    fields: [earlier.field.clone(), column.field.clone()],
                });
            }
        }
        if profile.group.contains(&column.field) {
            group_columns.push(headers.len());
        }
        labels.extend(
            emitted
                .iter()
                .map(|_| profile.label_for(column).to_string()),
        );
        if column.template.is_some() {
            templated_columns.push((
                column,
                (headers.len()..headers.len() + emitted.len()).collect(),
            ));
        }
        // A channel named alone keys JSON as it is named, its header
        // declared aside.
        if let [header] = emitted.as_slice()
            && column.header.is_none()
            && *header != column.field
        {
            json_keys.insert(header.clone(), column.field.clone());
        }
        units.extend(emitted.iter().map(|_| unit.clone()));
        headers.extend(emitted.iter().cloned());
        plan.push((column, emitted));
    }
    // Each row's text of each template, as the screen writes it.
    let templated: Vec<Templated> = templated_columns
        .iter()
        .map(|(column, columns)| Templated {
            columns: columns.clone(),
            header: column
                .header
                .clone()
                .unwrap_or_else(|| column.field.clone()),
            texts: subjects
                .iter()
                .map(|subject| {
                    terminal_rendering::templated(
                        column,
                        subject,
                        terminal_rendering::Declaration::Profile(profile),
                        config,
                        templated_in,
                    )
                })
                .collect(),
        })
        .collect();
    drop(subjects);
    drop(borrowed);

    let mut rows = Vec::with_capacity(list.len());
    for entry in list.iter() {
        // Each row at its own project's precision: over several projects
        // the first one's wrote every row, and the numbers a file held
        // turned on the order the targets were named in.
        // A selection keeps no configurations of its own: the collection it
        // was made from knows each sample's.
        let own = match entry.path.as_deref() {
            Some(path) if described.spans_configurations() => described.config_for(path).or(config),
            Some(path) if list.spans_configurations() => list.config_for(path).or(config),
            _ => config,
        };
        rows.push(row_of(
            target,
            entry,
            &plan,
            &vocabulary,
            profile,
            own,
            numbers,
        )?);
    }
    // Each quantity's state, which JSON writes beside it.
    let _pass = fingerprint::Pass::begin();
    let mut traced = crate::collection::computation::Traced::default();
    for entry in list.iter() {
        let sample = entry.sample.borrow();
        let states = fingerprint::check(&sample).unwrap_or_default();
        // What the model owes, as `status` plans it: a formula changed is
        // outdated and a value waiting is waiting, on this surface as on every
        // other — read, they were `current`.
        // Planned under its own project's, as its precision is: under the
        // first project's, a formula changed in the other was `current`.
        let own = match entry.path.as_deref() {
            Some(path) if described.spans_configurations() => described.config_for(path).or(config),
            Some(path) if list.spans_configurations() => list.config_for(path).or(config),
            _ => config,
        };
        let planned = entry.path.as_deref().and_then(|path| {
            crate::collection::computation::planned_states_traced(own, path, &sample, &mut traced)
        });
        for (quantity, field) in quantities.iter_mut().zip(&quantity_fields) {
            let state = match field {
                fields::Field::Named { name, .. } => states.get(name).cloned(),
                fields::Field::Cell {
                    table, column, row, ..
                } => sample
                    .table(table)
                    .ok()
                    .and_then(|held| held.row(row).ok())
                    .and_then(|view| {
                        let index: Vec<Value> = view.index().into_iter().cloned().collect();
                        fingerprint::check_cell_among(&sample, table, column, &index).ok()
                    }),
                _ => None,
            };
            let present = matches!(
                rows.get(quantity.states.len()).and_then(|row| row.get(quantity.value)),
                Some(Cell::Scalar(Some(value)) | Cell::Written { value, .. }) if !value.is_absent()
            );
            let never = match (&state, field) {
                (None, fields::Field::Named { name, .. }) if !present => {
                    unmade.holds(described, entry.path.as_deref(), name.as_str())
                }
                // A row's cell its column derives and no formula has filled.
                (
                    None | Some(Freshness::Source),
                    fields::Field::Cell {
                        table, column, row, ..
                    },
                ) if !present => {
                    sample.table(table).is_ok_and(|held| held.row(row).is_ok())
                        && unmade.holds(
                            described,
                            entry.path.as_deref(),
                            &format!("{table}.{column}"),
                        )
                }
                _ => false,
            };
            let not_applicable = matches!(
                rows.get(quantity.states.len()).and_then(|row| row.get(quantity.value)),
                Some(Cell::Scalar(Some(value)) | Cell::Written { value, .. })
                    if value.is_not_applicable()
            );
            let owed = match field {
                fields::Field::Named { name, .. } => planned
                    .as_ref()
                    .and_then(|planned| planned.get(name.as_str()))
                    .and_then(|said| crate::collection::computation::planned_word(said)),
                // A cell's column whose formula changed: every cell of it was
                // computed by the formula before, and was said current.
                fields::Field::Cell { table, column, .. } => planned
                    .as_ref()
                    .and_then(|planned| planned.get(&format!("{table}.{column}")))
                    .and_then(|said| crate::collection::computation::planned_word(said))
                    .filter(|word| *word == "outdated"),
                _ => None,
            };
            quantity.states.push(if let Some(word) = owed {
                Some(word.to_string())
            } else if never {
                Some(NEVER_COMPUTED.to_string())
            } else if not_applicable {
                // Not absent: an answer.
                Some("not applicable".to_string())
            } else {
                match (state, field) {
                    // Outdated for want of an input emptied since: it waits,
                    // its number the one computed before, as `status` says.
                    (Some(state), fields::Field::Named { .. } | fields::Field::Cell { .. })
                        if matches!(
                            state,
                            Freshness::Stale { .. }
                                | Freshness::Broken { .. }
                                | Freshness::Unjudged { .. }
                        ) && !crate::collection::editing::emptied_inputs(&sample, &state)
                            .is_empty() =>
                    {
                        Some("waiting".to_string())
                    }
                    (Some(state), _) => Some(state_name(&state, present).to_string()),
                    // A row the sample does not have holds no cell: absent,
                    // as a quantity no file holds is.
                    (None, fields::Field::Cell { .. }) if !present => Some("absent".to_string()),
                    (None, _) => None,
                }
            });
        }
    }
    Ok(Dataset {
        headers,
        rows,
        units,
        quantities,
        templated,
        labels,
        group_columns,
        json_keys,
    })
}

/// A state as JSON writes it, and as a row's `state` column names it.
pub fn state_name(state: &Freshness, present: bool) -> &'static str {
    match state {
        Freshness::Source if present => "entered",
        Freshness::Source => "absent",
        Freshness::Current => "current",
        Freshness::Stale { .. } => "outdated",
        Freshness::Broken { .. } => "broken",
        Freshness::Edited => "edited",
        Freshness::RecordMissing => "record missing",
        Freshness::Failed { .. } => "failed",
        Freshness::Unjudged { .. } => "unjudged",
    }
}

/// Whether this column is a quantity — something with an uncertainty channel to
/// put in a second column. An attribute or a reserved field has none, and
/// asking for two would emit a column that can never hold anything.
///
/// Answered by the collection the selection was made from — never by the
/// selection. `cold_crashed` and `diameter` are spelled identically and only the data
/// says which; but the *first selected sample* is not the data: a filter
/// keeping nothing wrote flat headers, and a first sample lacking an attribute
/// would have given it two columns.
/// A sample's path as a declared export writes it: from its project's
/// folder, `/` between its parts, so that the file is the same whichever
/// folder the export runs from; absolute where it lies outside.
fn from_project(path: &Path, base: Option<&Path>) -> String {
    let resolved = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let relative = base
        .and_then(|base| dunce::canonicalize(base).ok())
        .and_then(|base| resolved.strip_prefix(base).ok().map(Path::to_path_buf));
    match relative {
        Some(relative) => relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
        None => resolved.to_string_lossy().into_owned(),
    }
}

/// Each entry's configuration, in the list's order: its own project's.
fn configs_of(list: &SampleList) -> Vec<Option<&ProjectConfig>> {
    list.iter()
        .map(|entry| {
            entry
                .path
                .as_deref()
                .map_or(list.config(), |path| list.config_for(path))
        })
        .collect()
}

fn has_uncertainty(column: &ColumnSpec, described: &SampleList) -> Result<bool, ExportError> {
    let parsed = fields::parse(&column.field).map_err(ExportError::Field)?;
    // A path that already names a channel is one column by construction.
    if fields::has_explicit_channel(&column.field) {
        return Ok(false);
    }
    Ok(match &parsed {
        fields::Field::Reserved(_) | fields::Field::ListItem { .. } => false,
        fields::Field::Named { name, .. } => described
            .iter()
            .any(|entry| entry.sample.borrow().has_property(name)),
        // A cell is a property wherever its table holds that column.
        fields::Field::Cell { table, column, .. } => described.iter().any(|entry| {
            entry
                .sample
                .borrow()
                .table(table)
                .is_ok_and(|held| held.column(column).is_ok())
        }),
    })
}

/// **The column count does not vary.** Fifty samples where one lacks an
/// uncertainty still export two columns; that cell is empty, because a file
/// whose column count changes by row is malformed.
fn row_of(
    target: &ExportTarget,
    entry: &Entry,
    plan: &[(&ColumnSpec, Vec<String>)],
    vocabulary: &Vocabulary,
    profile: &Profile,
    config: Option<&ProjectConfig>,
    numbers: Numbers,
) -> Result<Vec<Cell>, ExportError> {
    let sample = entry.sample.borrow();
    let subject = Subject {
        sample: &sample,
        path: entry.path.as_deref(),
        vocabulary,
        states: entry.states.as_deref(),
    };
    let mut row = Vec::new();
    if target.filename {
        row.push(Cell::from(
            entry
                .path
                .as_deref()
                .and_then(Path::file_name)
                .map(|name| Value::text(name.to_string_lossy().into_owned())),
        ));
    }
    if target.path {
        row.push(Cell::from(entry.path.as_deref().map(|path| {
            Value::text(from_project(path, config.map(ProjectConfig::root)))
        })));
    }
    for (column, emitted) in plan {
        let parsed = fields::parse(&column.field).map_err(ExportError::Field)?;
        // A failed value is written as the screen shows it, its last number,
        // its state saying not to trust it.
        let resolved = fields::resolve(&parsed, &subject).map_err(ExportError::Field)?;
        // The precision the project declares for this column, resolved the way
        // every other surface resolves it. The screen showed `12.22` and the
        // file held `12.219999999999999`, because this was the one caller that
        // never asked.
        let shown = terminal_rendering::presentation(
            column,
            &subject,
            terminal_rendering::Declaration::Profile(profile),
            config,
            None,
        );
        match emitted.len() {
            // A quantity exports both of its numbers, and a missing
            // uncertainty leaves an empty field rather than a missing column.
            2 => {
                let (value, uncertainty) = pair(&parsed, &subject)?;
                row.push(at_declared_precision(value, &shown, numbers));
                row.push(at_declared_precision(
                    uncertainty,
                    &of_uncertainty(&shown),
                    numbers,
                ));
            }
            _ => row.push(match resolved {
                // An uncertainty on its own is written as the quantity writes
                // its uncertainty: a split precision's second half, rounded up;
                // a statistic of spread, the standard error, alike.
                Resolution::Scalar(value) if fields::is_spread(channel_of(&parsed)) => {
                    at_declared_precision(value, &of_uncertainty(&shown), numbers)
                }
                // A count is a whole number whatever the quantity's
                // precision: `3`, never `3.000`, as the screen writes it.
                Resolution::Scalar(value)
                    if channel_of(&parsed) == fields::Channel::Stat(fields::Statistic::Count) =>
                {
                    Cell::Scalar(value)
                }
                Resolution::Scalar(value) => at_declared_precision(value, &shown, numbers),
                // A tag set is its own cell, which each format writes its way.
                Resolution::Tags(tags) => {
                    Cell::Tags(tags.iter().map(|tag| tag.to_string()).collect())
                }
                Resolution::List(values) => Cell::List(values),
            }),
        }
    }
    Ok(row)
}

/// The header of the column `--status` adds.
pub const STATE_HEADER: &str = "state";

/// The dataset with one more column, `state`, saying for each row what in it is
/// not current.
///
/// A state is drawn as a **mark** for a human reading a screen, and a mark is
/// the terminal's: an export and a pipe write exactly the columns asked for.
/// So a state that has to travel is asked for like any other column, and in a
/// file it is a word rather than a glyph — a `✗` in a CSV is the same class of
/// mistake as a dash in a numeric column, something a parser reads as text
/// because of a decision made about display.
///
/// **One column for the row**, last, not one per quantity: a column per
/// quantity doubled a table's width to say, most of the time, `current`. JSON
/// already writes each quantity's state inside it, so this is what the
/// row-shaped formats need. `None` when the dataset already has a `state`
/// column, which the caller refuses rather than write two.
pub fn with_states(dataset: Dataset) -> Option<Dataset> {
    if dataset.headers.iter().any(|header| header == STATE_HEADER) {
        return None;
    }
    let Dataset {
        mut headers,
        mut rows,
        mut units,
        quantities,
        templated,
        mut labels,
        group_columns,
        json_keys,
    } = dataset;
    for (index, row) in rows.iter_mut().enumerate() {
        let said = row_state(quantities.iter().map(|quantity| {
            (
                quantity.key.as_str(),
                quantity
                    .states
                    .get(index)
                    .and_then(|state| state.as_deref()),
            )
        }));
        row.push(Cell::from(Some(Value::text(said))));
    }
    headers.push(STATE_HEADER.to_string());
    units.push(None);
    if !labels.is_empty() {
        labels.push(STATE_HEADER.to_string());
    }
    Some(Dataset {
        headers,
        rows,
        units,
        quantities,
        templated,
        labels,
        group_columns,
        json_keys,
    })
}

/// A value its configuration's formula gives elsewhere, which this sample has
/// never been given: said, as `status` says it, never counted `current`.
pub const NEVER_COMPUTED: &str = "never_computed";

/// What each configuration's formulas give: the properties its model's source
/// declares, and those a sample of it holds with a formula's record — the
/// second for a model whose source says nothing readable. A sample holding
/// none of one has never been given it: `never computed`, on every surface
/// that says a row's state.
pub struct NeverComputed {
    given: std::collections::HashSet<(Option<PathBuf>, String)>,
    fallback: Option<PathBuf>,
}

impl NeverComputed {
    /// `config` describes the samples `described` holds no configuration for,
    /// as a narrowed list holds none.
    pub fn of(described: &SampleList, config: Option<&ProjectConfig>) -> NeverComputed {
        let mut given = std::collections::HashSet::new();
        let config_of =
            |path: Option<&Path>| path.and_then(|path| described.config_for(path)).or(config);
        let root_of =
            |path: Option<&Path>| config_of(path).map(|config| config.root().to_path_buf());
        let mut read: Vec<Option<PathBuf>> = Vec::new();
        for entry in described.iter() {
            let root = root_of(entry.path.as_deref());
            if !read.contains(&root) {
                let config = config_of(entry.path.as_deref());
                // What a formula gives: a value its readings give, the model
                // declaring their statistic, waits for readings nobody
                // entered, as `status` says it, and is never *never computed*.
                let described = config.and_then(crate::config::model_runtime::current_description);
                let names: Vec<String> = match &described {
                    Some(description) => description
                        .properties
                        .iter()
                        .filter(|(_, property)| {
                            use crate::config::model_runtime::Origin;
                            !matches!(property.value, Origin::Entered | Origin::Statistic { .. })
                                || matches!(
                                    property.uncertainty,
                                    Some(Origin::Formula { .. } | Origin::Quantity { .. })
                                )
                        })
                        .map(|(name, _)| name.clone())
                        .collect(),
                    None => config
                        .map(crate::config::model_runtime::declared_properties)
                        .unwrap_or_default(),
                };
                for name in names {
                    given.insert((root.clone(), name));
                }
                // And the columns its tables derive, `ageing.nominal`:
                // a new row's cell was said current, or absent.
                if let Some(description) =
                    config.and_then(crate::config::model_runtime::current_description)
                {
                    for (table, described) in &description.tables {
                        for column in described
                            .columns
                            .iter()
                            .filter(|column| column.formula.is_some())
                        {
                            given.insert((root.clone(), format!("{table}.{}", column.name)));
                        }
                    }
                }
                read.push(root.clone());
            }
            let sample = entry.sample.borrow();
            for name in sample.property_names() {
                if sample
                    .property(name)
                    .is_ok_and(|handle| handle.records().computed.is_some())
                {
                    given.insert((root.clone(), name.to_string()));
                }
            }
        }
        NeverComputed {
            given,
            fallback: config.map(|config| config.root().to_path_buf()),
        }
    }

    /// Whether some configuration's formula gives `name`.
    pub fn gives(&self, name: &str) -> bool {
        self.given.iter().any(|(_, given)| given == name)
    }

    /// Whether the sample at `path`, which holds no value of `name` and no
    /// record of it, is one its configuration's formula has never run on.
    pub fn holds(&self, described: &SampleList, path: Option<&Path>, name: &str) -> bool {
        let root = path
            .and_then(|path| described.config_for(path))
            .map(|config| config.root().to_path_buf())
            .or_else(|| self.fallback.clone());
        self.given.contains(&(root, name.to_string()))
    }
}

/// What a row's `state` column says: `current` when nothing in it needs
/// attention, and otherwise each state that does with the quantities in it,
/// worst first — `failed: porosity; edited: diameter`. `; ` between states and `, `
/// between quantities, so that a script splits it without guessing.
///
/// Entered, absent and current values need nothing; a quantity with no state
/// is not a derived value and is left out.
pub fn row_state<'a>(states: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> String {
    // `waiting` last: not *not current*, but never `current` either — a row
    // whose value waits for an input nobody entered was said current.
    const WORST_FIRST: [&str; 8] = [
        "failed",
        "unjudged",
        NEVER_COMPUTED,
        "broken",
        "outdated",
        "record missing",
        "edited",
        "waiting",
    ];
    let mut named: Vec<Vec<&str>> = vec![Vec::new(); WORST_FIRST.len()];
    for (quantity, state) in states {
        if let Some(at) = state.and_then(|state| WORST_FIRST.iter().position(|w| *w == state)) {
            named[at].push(quantity);
        }
    }
    let said: Vec<String> = WORST_FIRST
        .iter()
        .zip(named)
        .filter(|(_, quantities)| !quantities.is_empty())
        .map(|(state, quantities)| format!("{state}: {}", quantities.join(", ")))
        .collect();
    if said.is_empty() {
        "current".to_string()
    } else {
        said.join("; ")
    }
}

/// A number written at the precision the project declares for it: its **text**,
/// as the screen writes it, and the number beside it. A CSV holds `1.23e+01`
/// and `0.000` where the screen shows them, and a reader still reads a number;
/// JSON writes the same text, which is a JSON number. The text goes through the
/// same `format_value` a screen uses, so the two cannot drift apart.
///
/// Without a declared precision nothing is touched: the full value is what the
/// file has always held, and a script that wants it says `--precision .6e`.
/// A dict (`Numbers::AsStored`) is not text, and keeps the stored number.
fn at_declared_precision(value: Option<Value>, shown: &Resolved, numbers: Numbers) -> Cell {
    let Some(value) = value else {
        return Cell::Scalar(None);
    };
    if numbers == Numbers::AsStored
        || shown.precision.is_none()
        || !matches!(value, Value::Number(_) | Value::Integer(_))
    {
        return Cell::Scalar(Some(value));
    }
    let text = formatting::format_value(&value, shown);
    if text.parse::<f64>().is_err() {
        // A rendering that does not read back as a number: keep the number.
        return Cell::Scalar(Some(value));
    }
    Cell::Written { value, text }
}

/// The same presentation, with the uncertainty's precision for both numbers.
fn of_uncertainty(shown: &Resolved) -> Resolved {
    Resolved {
        precision: shown.precision.as_ref().map(Precision::of_uncertainty),
        ..shown.clone()
    }
}

fn channel_of(field: &fields::Field) -> fields::Channel {
    match field {
        fields::Field::Named { channel, .. } | fields::Field::Cell { channel, .. } => *channel,
        _ => fields::Channel::Value,
    }
}

fn pair(
    field: &fields::Field,
    subject: &Subject,
) -> Result<(Option<Value>, Option<Value>), ExportError> {
    let value = read(field, fields::Channel::Value, subject)?;
    let uncertainty = read(field, fields::Channel::Uncertainty, subject)?;
    Ok((value, uncertainty))
}

fn read(
    field: &fields::Field,
    channel: fields::Channel,
    subject: &Subject,
) -> Result<Option<Value>, ExportError> {
    let with_channel = fields::with_channel(field, channel);
    match fields::resolve(&with_channel, subject).map_err(ExportError::Field)? {
        Resolution::Scalar(value) => Ok(value),
        Resolution::List(_) | Resolution::Tags(_) => Ok(None),
    }
}

/// Atomically, and refusing an existing destination without `overwrite` — the
/// same write-then-rename `document` uses, for the same reason.
pub fn write(text: &str, path: &Path, overwrite: bool) -> Result<(), ExportError> {
    let path = &destination_of(path)?;
    if !overwrite && path.exists() {
        return Err(ExportError::AlreadyExists {
            path: path.to_path_buf(),
        });
    }
    refuse_protected(path)?;
    make_folder(path)?;
    crate::format::document::write_atomically(path, text.as_bytes()).map_err(|source| {
        ExportError::Io {
            path: path.to_path_buf(),
            source,
        }
    })
}

/// The same, replacing the destination only if it still holds `expected` — what
/// the command read of it before building what it writes, `None` where there
/// was nothing — read again just before writing.
pub fn write_replacing(
    text: &str,
    path: &Path,
    expected: Option<&[u8]>,
) -> Result<(), ExportError> {
    // Judged as named too: a link inside the model's environment led the
    // write to the system's interpreter, judged only where it led.
    if let Some(what) = project_file(path) {
        return Err(ExportError::ProjectFile {
            path: path.to_path_buf(),
            what,
        });
    }
    let path = &destination_of(path)?;
    refuse_protected(path)?;
    make_folder(path)?;
    crate::format::document::replace_if_unchanged(path, expected, Some(text.as_bytes())).map_err(
        |error| match error {
            crate::format::document::DocumentError::Io { source, .. } => ExportError::Io {
                path: path.to_path_buf(),
                source,
            },
            crate::format::document::DocumentError::Locked { .. } => ExportError::Busy {
                path: path.to_path_buf(),
            },
            _ => ExportError::ChangedSince {
                path: path.to_path_buf(),
            },
        },
    )
}

/// The folder a destination names, made where it does not exist yet, whatever
/// named it: asking first stopped every first export at a `mkdir`. A preview
/// makes nothing: only a write reaches here.
fn make_folder(path: &Path) -> Result<(), ExportError> {
    match path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
    {
        Some(directory) => std::fs::create_dir_all(directory).map_err(|source| ExportError::Io {
            path: directory.to_path_buf(),
            source,
        }),
        None => Ok(()),
    }
}

/// What the destination holds now, for [`write_replacing`] to compare: a
/// symbolic link's target, as the write goes through it.
pub fn held_at(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(destination_of(path).ok()?).ok()
}

/// Why a write to `path` would be refused, asked before anything is written:
/// what a preview says, so that `--write` is never the first to know.
pub fn refusal(path: &Path) -> Option<ExportError> {
    destination_of(path)
        .and_then(|resolved| refuse_protected(&resolved))
        .err()
}

/// Where a write to `path` lands: through a symbolic link to the file it
/// names, so that a link published as `latest.csv` survives the export and
/// its target is what changes.
///
/// Writing *to* the link path instead replaced the link with a regular file —
/// the link lost, the file it pointed at never updated, and exit 0.
fn destination_of(path: &Path) -> Result<PathBuf, ExportError> {
    let is_link = std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink());
    if !is_link {
        return Ok(path.to_path_buf());
    }
    dunce::canonicalize(path).map_err(|_| ExportError::DanglingLink {
        path: path.to_path_buf(),
        target: std::fs::read_link(path).unwrap_or_default(),
    })
}

/// What an export refuses to replace, whatever `--write` says.
///
/// **A read-only file**, because the atomic write renames over it and a rename
/// needs only the directory's permission: the file's own protection was never
/// consulted, and `chmod a-w` — the one defence a careful owner reaches for —
/// did not hold. **A sample**, because an export is made *from* samples, and a
/// destination mistyped into the collection replaced a sample's frontmatter,
/// tags and note with a table, exit 0.
fn refuse_protected(path: &Path) -> Result<(), ExportError> {
    // **A file the project runs on**, whether or not it exists yet: its
    // configuration, SampleKit's own folder — the history, the description —
    // and the model's sources. Written over by `-o`, the history could only be
    // repaired by hand with git.
    if let Some(what) = project_file(path) {
        return Err(ExportError::ProjectFile {
            path: path.to_path_buf(),
            what,
        });
    }
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(());
    };
    if metadata.permissions().readonly() {
        return Err(ExportError::ReadOnly {
            path: path.to_path_buf(),
        });
    }
    if is_a_sample(path) {
        return Err(ExportError::IsASample {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

/// What of the project `path` is, where it is one of the files the project
/// runs on: `None` for any other.
fn project_file(path: &Path) -> Option<&'static str> {
    use crate::config::{model_runtime, project_config};
    if path.file_name().is_some_and(|name| {
        crate::collection::sample_list::file_key(Path::new(name))
            == crate::collection::sample_list::file_key(Path::new(project_config::FILENAME))
    }) {
        return Some("the project's configuration");
    }
    // A lock SampleKit takes, whatever it locks: written as an export, a
    // sample's lock was never taken back, and every save of it refused.
    if path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lock"))
    {
        return Some("a lock SampleKit takes while it writes");
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    if absolute.components().any(|part| {
        crate::collection::sample_list::file_key(Path::new(part.as_os_str()))
            == crate::collection::sample_list::file_key(Path::new(".samplekit"))
    }) {
        return Some("in SampleKit's own folder");
    }
    let canonical = |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // A file not there yet resolves through its folder: `-o link/x`, the link
    // set in the project, lands where the link leads.
    let wanted = match (
        dunce::canonicalize(&absolute),
        absolute.parent(),
        absolute.file_name(),
    ) {
        (Ok(resolved), _, _) => resolved,
        (Err(_), Some(parent), Some(name)) => canonical(parent).join(name),
        _ => absolute.clone(),
    };
    // The model of the project the file lies in, and of the project the
    // command runs in: a model kept beside the projects that use it,
    // `../shared/piece.py`, lies in none of them, and was written over.
    let places = [
        absolute.parent().map(Path::to_path_buf),
        std::env::current_dir().ok(),
    ];
    let configs: Vec<project_config::ProjectConfig> = places
        .into_iter()
        .flatten()
        .filter_map(|place| {
            project_config::find(&place).and_then(|found| project_config::load(&found).ok())
        })
        .collect();
    if configs.iter().any(|config| {
        model_runtime::template_of(config)
            .and_then(|template| model_runtime::template_files(&template).ok())
            .is_some_and(|sources| sources.iter().any(|source| canonical(source) == wanted))
    }) {
        return Some("the project's model");
    }
    // **The environment the model runs in**, named or reached through a
    // link: `-o .venv/pyvenv.cfg` wrote over it, exit 0, and a link to the
    // system's interpreter led the write there.
    configs
        .iter()
        .any(|config| {
            let Some(python) = config.python() else {
                return false;
            };
            let named = config.resolve(&python.to_string_lossy());
            let environment = if named.is_dir() {
                named.clone()
            } else {
                match named.parent() {
                    Some(bin)
                        if bin
                            .file_name()
                            .is_some_and(|name| name == "bin" || name == "Scripts") =>
                    {
                        bin.parent()
                            .map_or_else(|| named.clone(), Path::to_path_buf)
                    }
                    _ => named.clone(),
                }
            };
            // The environment the interpreter really is in, where the one
            // named links elsewhere — pyenv's, conda's.
            let interpreter = if named.is_dir() {
                ["bin/python", "Scripts/python.exe"]
                    .iter()
                    .map(|inside| named.join(inside))
                    .find(|path| path.exists())
                    .unwrap_or_else(|| named.clone())
            } else {
                named.clone()
            };
            let resolved = canonical(&interpreter);
            // Each link on the way to the interpreter, and the environment
            // each lies in: a venv's `python` leads to pyenv's or conda's,
            // which may lead further.
            let mut hops = vec![interpreter.clone()];
            while hops.len() < 16 {
                let last = hops.last().expect("one hop at least");
                let Ok(target) = std::fs::read_link(last) else {
                    break;
                };
                let next = match last.parent() {
                    Some(parent) if target.is_relative() => parent.join(target),
                    _ => target,
                };
                hops.push(next);
            }
            let environments: Vec<PathBuf> = hops
                .iter()
                .filter_map(|hop| {
                    let bin = canonical(hop.parent()?);
                    bin.file_name()
                        .is_some_and(|name| name == "bin" || name == "Scripts")
                        .then(|| bin.parent().map(Path::to_path_buf))
                        .flatten()
                })
                .collect();
            absolute.starts_with(&environment)
                || wanted.starts_with(canonical(&environment))
                || named.is_file() && resolved == wanted
                || environments.iter().any(|real| wanted.starts_with(real))
        })
        .then_some("the environment the model runs in")
}

/// Whether a file may be a sample: it opens with a frontmatter line. Judged
/// by that line alone, not by what the frontmatter holds: a sample of
/// attributes only, or one whose YAML does not parse today, is still data, and
/// a guard asking for a name, properties or tables let an export replace it.
/// An export is a delimited table, JSON, or a template's text; a template
/// written as a note with its own frontmatter is refused too, and written
/// elsewhere.
fn is_a_sample(path: &Path) -> bool {
    use std::io::{BufRead as _, Read as _};
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut first = Vec::new();
    if std::io::BufReader::new(file)
        .take(1024)
        .read_until(b'\n', &mut first)
        .is_err()
    {
        return false;
    }
    let first = first.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&first);
    first.trim_ascii_end() == b"---"
}

// ------------------------------------------------------------------ errors

#[derive(Debug)]
pub enum ExportError {
    Field(FieldError),
    /// A column whose samples write it in two units: numbers written under no
    /// unit that do not compare.
    MixedUnits {
        columns: Vec<(String, Vec<String>)>,
    },
    /// Two columns that would write one header.
    DuplicateHeader {
        header: String,
        fields: [String; 2],
    },
    AlreadyExists {
        path: PathBuf,
    },
    /// A destination the file system protects: replacing it would ignore what
    /// its owner said about it.
    ReadOnly {
        path: PathBuf,
    },
    /// A destination that is a sample: an export never replaces the data it
    /// was made from.
    IsASample {
        path: PathBuf,
    },
    /// A destination that is a file the project runs on — its configuration,
    /// SampleKit's own folder, its model's sources.
    ProjectFile {
        path: PathBuf,
        what: &'static str,
    },
    /// A symbolic link to nothing: there is no file to write through it to.
    DanglingLink {
        path: PathBuf,
        target: PathBuf,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
    /// The destination changed since the command read it: another writer's file
    /// is not replaced.
    /// Another process was writing the destination at that moment.
    Busy {
        path: PathBuf,
    },
    ChangedSince {
        path: PathBuf,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Field(error) => write!(f, "{error}"),
            ExportError::MixedUnits { columns } => {
                let said: Vec<String> = columns
                    .iter()
                    .map(|(field, units)| format!("{field} is in {}", units.join(" and ")))
                    .collect();
                write!(
                    f,
                    "a column cannot say which unit it is in: {} — a file of two units \
                     compares numbers that do not compare; narrow the selection, or \
                     samplekit validate says which files disagree",
                    said.join("; ")
                )
            }
            ExportError::DuplicateHeader { header, fields } => {
                let [first, second] = fields;
                if first == second {
                    write!(
                        f,
                        "'{first}' is asked for twice, and would write '{header}' twice"
                    )
                } else if second
                    .strip_suffix(".u")
                    .or_else(|| second.strip_suffix(".uncertainty"))
                    == Some(first.as_str())
                    || first
                        .strip_suffix(".u")
                        .or_else(|| first.strip_suffix(".uncertainty"))
                        == Some(second.as_str())
                {
                    // A quantity writes its uncertainty already: the channel
                    // beside it is that column again.
                    let quantity = if first.contains('.') { second } else { first };
                    write!(
                        f,
                        "'{quantity}' writes its uncertainty already, as '{header}': drop \
                         '{quantity}.u', or name '{quantity}.v' for the value alone"
                    )
                } else {
                    write!(
                        f,
                        "'{first}' and '{second}' would both write a column '{header}', and \
                         nothing could tell them apart: drop one, or name one — `-c \
                         {second}=NAME`, or a header in a declared profile"
                    )
                }
            }
            ExportError::AlreadyExists { path } => write!(
                f,
                "{} already exists, and nothing was written: overwrite=True \
                 replaces it",
                path.display()
            ),
            ExportError::ReadOnly { path } => write!(
                f,
                "{} is read-only, and nothing was written: make it writable to replace it",
                path.display()
            ),
            ExportError::ProjectFile { path, what } => write!(
                f,
                "{} is {what}, and nothing was written: an export never replaces a file the \
                 project runs on — choose another destination",
                path.display()
            ),
            ExportError::IsASample { path } => write!(
                f,
                "{} is a sample, and nothing was written: an export never replaces the \
                 data it is made from — choose another destination",
                path.display()
            ),
            ExportError::DanglingLink { path, target } => write!(
                f,
                "{} is a link to {}, which does not exist, and nothing was written",
                path.display(),
                target.display()
            ),
            ExportError::Io { path, source } => write!(f, "{}: {source}", path.display()),
            ExportError::Busy { path } => write!(
                f,
                "{} is being written by another process, and nothing was written: \
                 run the export again in a moment. A lock left by a process that was \
                 stopped is taken back by the next write; one that persists while no \
                 SampleKit runs is {}.lock, which can then be removed",
                path.display(),
                path.display()
            ),
            ExportError::ChangedSince { path } => write!(
                f,
                "{} changed since this command read it, and nothing was written: \
                 another writer's file is not replaced — run the export again",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ExportError {}
