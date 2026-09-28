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
use crate::presentation::export_formats::{Cell, Dataset, QuantityColumns};
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
/// written: counted and named. A failed value is left out of the file, so it is
/// neither.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Behind {
    /// Outdated or broken, by the cells they fill.
    pub stale: usize,
    /// Edited by hand, or without a record.
    pub held: usize,
    /// The samples holding any of them.
    pub samples: usize,
    /// `BR-01 flatness`, each value not current.
    pub named: Vec<String>,
}

pub fn behind(selected: &SampleList, profile: &Profile) -> Behind {
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
    for entry in selected.iter() {
        let sample = entry.sample.borrow();
        let late: Vec<_> = crate::collection::editing::not_current(&sample)
            .into_iter()
            .filter(|value| exported(&value.name))
            .collect();
        // A sample whose only late values failed is in neither tally.
        if late
            .iter()
            .any(|value| !matches!(value.state, Freshness::Failed { .. }))
        {
            behind.samples += 1;
        }
        for value in late {
            if !matches!(value.state, Freshness::Failed { .. }) {
                behind.named.push(format!(
                    "{} {}",
                    sample.name().unwrap_or("(unnamed)"),
                    value.name
                ));
            }
            let cells = value.rows.max(1);
            match value.state {
                Freshness::Stale { .. } | Freshness::Broken { .. } => behind.stale += cells,
                Freshness::Failed { .. } => {}
                _ => behind.held += cells,
            }
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
        .export(name)
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
    let behind = behind(&selected, &profile);
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
    // samples that all lack `brix` has not made `brix` a misspelling.
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
    let unmade = NeverComputed::of(described, config);
    // Over several projects each may spell a unit its own way, and the first
    // target's spelling headed every row: the files' own spelling instead,
    // which no order of the targets changes.
    let spelled_by = if described.spans_configurations() || list.spans_configurations() {
        None
    } else {
        config
    };
    for column in profile.columns() {
        let emitted = profile.headers_for(column, has_uncertainty(column, described)?);
        // Plain, whatever style the project renders in: a file's header is read
        // by programs.
        let unit =
            terminal_rendering::column_unit(column, &subjects, profile, spelled_by, Some("plain"))
                // No selected row writes one: the collection's, so that a header
                // does not lose its unit with its last row.
                .or_else(|| {
                    terminal_rendering::column_unit(
                        column,
                        &described_subjects,
                        profile,
                        spelled_by,
                        Some("plain"),
                    )
                });
        // A value a formula gives is a quantity before it holds an
        // uncertainty, or any value: its state is said from the first.
        let formula_gives = matches!(
            fields::parse(&column.field),
            Ok(fields::Field::Named { ref name, channel: fields::Channel::Value })
                if unmade.gives(name.as_str())
        );
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
        units.extend(emitted.iter().map(|_| unit.clone()));
        headers.extend(emitted.iter().cloned());
        plan.push((column, emitted));
    }
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
    for entry in list.iter() {
        let sample = entry.sample.borrow();
        let states = fingerprint::check(&sample).unwrap_or_default();
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
                _ => false,
            };
            let not_applicable = matches!(
                rows.get(quantity.states.len()).and_then(|row| row.get(quantity.value)),
                Some(Cell::Scalar(Some(value)) | Cell::Written { value, .. })
                    if value.is_not_applicable()
            );
            quantity.states.push(if never {
                Some(NEVER_COMPUTED.to_string())
            } else if not_applicable {
                // Not absent: an answer.
                Some("not applicable".to_string())
            } else {
                match (state, field) {
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
/// selection. `cold_crashed` and `malt` are spelled identically and only the data
/// says which; but the *first selected sample* is not the data: a filter
/// keeping nothing wrote flat headers, and a first sample lacking an attribute
/// would have given it two columns.
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
        row.push(Cell::from(
            entry
                .path
                .as_deref()
                .map(|path| Value::text(path.to_string_lossy().into_owned())),
        ));
    }
    for (column, emitted) in plan {
        let parsed = fields::parse(&column.field).map_err(ExportError::Field)?;
        // A failed value stays out while it is failed, though its file keeps
        // the last one its formula gave.
        if failed(&parsed, &sample) {
            row.extend(emitted.iter().map(|_| Cell::from(None::<Value>)));
            continue;
        }
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
                // its uncertainty: a split precision's second half.
                Resolution::Scalar(value)
                    if channel_of(&parsed) == fields::Channel::Uncertainty =>
                {
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

/// Whether a field reads a number of a value whose formula failed: in a session
/// its cache says so, and in a file read without its model, its record.
fn failed(field: &fields::Field, sample: &crate::core::sample::Sample) -> bool {
    let fields::Field::Named { name, channel } = field else {
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
    Some(Dataset {
        headers,
        rows,
        units,
        quantities,
    })
}

/// A value its configuration's formula gives elsewhere, which this sample has
/// never been given: said, as `status` says it, never counted `current`.
pub const NEVER_COMPUTED: &str = "never computed";

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
                for name in config
                    .map(crate::config::model_runtime::declared_properties)
                    .unwrap_or_default()
                {
                    given.insert((root.clone(), name));
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
/// worst first — `failed: brix; edited: malt`. `; ` between states and `, `
/// between quantities, so that a script splits it without guessing.
///
/// Entered, absent and current values need nothing; a quantity with no state
/// is not a derived value and is left out.
pub fn row_state<'a>(states: impl IntoIterator<Item = (&'a str, Option<&'a str>)>) -> String {
    const WORST_FIRST: [&str; 7] = [
        "failed",
        "unjudged",
        NEVER_COMPUTED,
        "broken",
        "outdated",
        "record missing",
        "edited",
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
    let path = &destination_of(path)?;
    refuse_protected(path)?;
    make_folder(path)?;
    crate::format::document::replace_if_unchanged(path, expected, Some(text.as_bytes())).map_err(
        |error| match error {
            crate::format::document::DocumentError::Io { source, .. } => ExportError::Io {
                path: path.to_path_buf(),
                source,
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
    ChangedSince {
        path: PathBuf,
    },
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Field(error) => write!(f, "{error}"),
            ExportError::DuplicateHeader { header, fields } => {
                let [first, second] = fields;
                if first == second {
                    write!(
                        f,
                        "'{first}' is asked for twice, and would write '{header}' twice"
                    )
                } else {
                    write!(
                        f,
                        "'{first}' and '{second}' would both write a column '{header}', and \
                         nothing could tell them apart: drop one, or give one a header in a \
                         declared profile"
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
