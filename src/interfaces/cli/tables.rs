//! The table, `view` and `export`: what is selected, shaped and written out.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

/// The default command: one table.
pub(super) fn table(options: &Options) -> Outcome {
    let root = options
        .positionals
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    // A profile groups as `--group` does, and `--group` replaces its groups as
    // `-s` replaces its sort.
    let options = &with_profile_groups(&collection, options)?;
    // A declared profile is each configuration's, and columns asked for are
    // drawn the same way: one table per project, headed by it. A data format is
    // one rectangle still.
    //
    // Groups are drawn the same way, one table per group headed by its values,
    // within each project where there are several: the path that heads tables
    // is this one.
    let grouped_tables = options.group.is_some() && options.format.is_none() && !options.summary;
    if grouped_tables
        || (collection.spans_configurations()
            && (options.profile.is_some()
                || (!options.columns.is_empty() && options.format.is_none() && !options.summary)))
    {
        return table_by_configuration(&collection, options, &root);
    }
    let mut profile = profile_of(&collection, options, options.profile.as_deref())?;
    let siblings = siblings_of(&root);
    let vocabulary = siblings.as_ref().unwrap_or(&collection);
    correct_column_typos(vocabulary, &mut profile)?;
    check_columns(vocabulary, &profile)?;
    let mut selected = select(&collection, options, &root)?;
    // Headers speak for every row: one configuration's when all rows share it.
    let header_config = shared_config(&collection, &selected);
    if options.summary {
        return emit_output(
            &summary_table(
                &selected,
                &profile,
                options,
                &collection,
                collection.spans_configurations(),
                "",
            )?,
            options,
            &selected,
        );
    }
    // A data format is an export of the same rectangle, so it goes through the
    // same builder: one definition of what a column is called and holds.
    if let Some(format) = options.format {
        // A data format is an export with no destination: the same rectangle,
        // through the same builder, so a column is called and holds the same
        // thing whether it lands in a terminal or in a file.
        //
        // **It identifies its rows.** A terminal table names each row, and a
        // CSV of four numbers with nothing saying which sample they belong to
        // is not the same table written differently. A *declared* export obeys
        // its own `filename` flag instead: that is the author's choice.
        // `name`, `filename`, and `path` are ordinary profile fields here: one
        // column has one source, and the CLI adds none implicitly.
        //
        // Grouped, it is still one rectangle, its groups' rows together and the
        // fields grouped by among its columns.
        if let Some((names, groups)) = grouping(options, vocabulary)? {
            // The selection is in the sort's order where one is in force, and
            // the groups follow it.
            let order = group_order(sort_spec(&collection, options, &[])?.is_some());
            let groups = groups_by(&selected, &groups, order)?;
            selected = grouped_for_format(&mut profile, &selected, &names, &groups, false);
        }
        let target = samplekit::config::project_config::ExportTarget {
            name: profile.name.clone(),
            profile: profile.name.clone(),
            format,
            output: PathBuf::from("-"),
            filename: false,
            path: false,
            query: None,
        };
        let dataset = exports::run_within(&target, &profile, &selected, header_config, vocabulary)
            .map_err(export_failure)?;
        // JSON already gives each quantity its state; the delimited formats say
        // it as a column, asked for.
        let dataset = with_states_if_asked(dataset, options, format)?;
        let dataset = noted_empty(&dataset, format);
        let text = export_formats::serialize(&dataset, format)
            .map_err(|error| Fail::data(error.to_string()))?;
        return emit_output(&text, options, &selected);
    }

    let style = table_style(&collection, options)?;
    let semantic_style = render_style(&collection, options)?;
    let unmade = exports::NeverComputed::of(&collection, collection.config());
    let guards: Vec<_> = selected.iter().map(|entry| entry.sample.borrow()).collect();
    let vocabulary = selected.vocabulary();
    let subjects: Vec<Subject> = selected
        .iter()
        .zip(guards.iter())
        .map(|(entry, guard)| Subject {
            sample: guard,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        })
        .collect();
    // A column whose samples do not agree on a unit cannot be given a header:
    // dropping the unit silently prints incomparable numbers under a word that
    // promises nothing, which is the purest silence there is. `validate`
    // already knows how to say which spellings and where.
    refuse_disagreeing_units(&profile, &subjects, header_config, semantic_style)?;
    let mut headers = render::columns(&profile, &subjects, header_config, semantic_style);
    let terminal = matches!(output_target(options), Target::Terminal { .. });
    let mut rows = Vec::with_capacity(subjects.len());
    for subject in &subjects {
        // Each row in its own configuration's units and precision.
        let own = subject
            .path
            .map_or(collection.config(), |path| collection.config_for(path));
        let mut row = render::row(&profile, subject, own, semantic_style);
        if terminal {
            mark_row(&profile, subject.sample, &mut row);
        }
        // A table on a screen or in a pipe says it as a CSV does: the marks are
        // the terminal's, and a pipe loses them.
        if options.status {
            row.push(row_state_of(&profile, subject, &collection, &unmade));
        }
        rows.push(row);
    }
    if options.status {
        if headers.iter().any(|header| header == exports::STATE_HEADER) {
            return Err(state_column_taken());
        }
        headers.push(exports::STATE_HEADER.to_string());
    }
    let projected = with_project(&collection, &selected, options, &mut headers, &mut rows);
    let identified = with_identity(
        &profile,
        &subjects,
        collection.config(),
        options,
        &mut headers,
        &mut rows,
    );
    let rendered = render::table_identified(
        headers,
        rows,
        output_target(options),
        style,
        usize::from(identified) + usize::from(projected),
        coloured(),
    );
    // This table's columns are named with -c, which the closing line offers.
    let mut text = with_fewer(rendered);
    if options.output.is_none() && !matches!(output_target(options), Target::Pipe) {
        text.push_str(&format!(
            "\n{} of {}\n",
            selected.len(),
            counted_as(collection.len(), "sample", "samples")
        ));
    }
    emit_output(&text, options, &selected)
}

/// `--status` adds the row's state as a column, where the format does not
/// already carry one.
fn with_states_if_asked(
    dataset: export_formats::Dataset,
    options: &Options,
    format: Format,
) -> Result<export_formats::Dataset, Fail> {
    if options.status && format != Format::Json {
        return exports::with_states(dataset).ok_or_else(state_column_taken);
    }
    Ok(dataset)
}

/// Two columns called `state` are two columns a reader cannot tell apart, the
/// same refusal a profile makes of two equal headers.
fn state_column_taken() -> Fail {
    Fail::usage(format!(
        "--status adds a column named '{}', and this table already has one: give yours \
         another label, as in -c state=condition",
        exports::STATE_HEADER
    ))
}

/// Refuses a table whose columns cannot name one unit.
fn refuse_disagreeing_units(
    profile: &Profile,
    subjects: &[Subject],
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Outcome {
    let disagreeing = render::disagreeing_units(profile, subjects, config, style);
    if disagreeing.is_empty() {
        return Ok(());
    }
    let said: Vec<String> = disagreeing
        .iter()
        .map(|(field, spellings)| format!("{field} is in {}", spellings.join(" and ")))
        .collect();
    Err(Fail::data(format!(
        "a column cannot say which unit it is in: {}\n  \
         a table of two units is two tables — narrow the selection, or samplekit \
         validate says which files disagree",
        said.join("; ")
    )))
}

/// A table over samples several configurations describe: one table per
/// configuration, under its file, each with that configuration's profile, units
/// and precision. Grouped, one table per group within each, headed by its
/// project where there are several and by its values.
pub(super) fn table_by_configuration(
    collection: &SampleList,
    options: &Options,
    root: &Path,
) -> Outcome {
    let selected = select(collection, &unordered(options), root)?;
    let mut parts = collection.by_configuration(&selected);
    // In the order the projects were given: the loaded collection follows
    // the targets, where a profile's sort put the other project first.
    let first_at = |part: &list::ConfigurationPart<'_>| {
        let path = part
            .samples
            .iter()
            .next()
            .and_then(|entry| entry.path.clone());
        collection
            .iter()
            .position(|entry| entry.path == path)
            .unwrap_or(usize::MAX)
    };
    parts.sort_by_key(first_at);
    let unmade = exports::NeverComputed::of(collection, collection.config());
    if parts.len() > 1 && options.format.is_some() {
        return Err(Fail::usage(format!(
            "this selection spans {} configurations, and a data format writes one table\n  \
             narrow the target to one project, or give --rc PATH",
            parts.len()
        )));
    }
    // A file named alone is checked against its directory, as the one table
    // is: a column its siblings hold is no typo.
    let siblings = siblings_of(root);
    let vocabulary_list = siblings.as_ref().unwrap_or(collection);
    // The groups are made over the whole selection, whose vocabulary is the
    // collection's, and then found in each project's part: a part is a
    // collection of its own, where a field only another project holds is
    // unknown rather than absent.
    // In the values' order: each part puts them in its sort's below.
    let grouping = match grouping(options, vocabulary_list)? {
        Some((names, fields)) => Some((
            names,
            groups_by(&selected, &fields, list::GroupOrder::Values)?,
        )),
        None => None,
    };
    let anonymous_profile = match options.profile {
        Some(_) => None,
        None => {
            let mut profile = profile_of(collection, options, None)?;
            correct_column_typos(vocabulary_list, &mut profile)?;
            check_columns(vocabulary_list, &profile)?;
            Some(profile)
        }
    };
    let semantic_style = render_style(collection, options)?;
    let vocabulary = selected.vocabulary();
    let projects = collection.spans_configurations();
    let mut text = String::new();
    let mut undeclared = Vec::new();
    let mut refusal = None;
    let mut shown = 0;
    for mut part in parts {
        let mut profile = match &anonymous_profile {
            Some(profile) => profile.clone(),
            None => match profile_of(&part.samples, options, options.profile.as_deref()) {
                Ok(profile) => {
                    check_columns(&part.samples, &profile)?;
                    profile
                }
                Err(fail) => {
                    undeclared.push((part.file.clone(), part.samples.len()));
                    refusal.get_or_insert(fail);
                    continue;
                }
            },
        };
        // `-s` within each table, else the profile's order: the selection was
        // taken unordered, and `-s` was dropped with it. The groups then follow
        // that order.
        let sorted = sort_spec(&part.samples, options, &profile.sort)?;
        let listed = sorted.is_some();
        if let Some(spec) = sorted {
            part.samples
                .sort(&spec)
                .map_err(|error| Fail::usage(error.to_string()))?;
        }
        shown += part.samples.len();
        // Drawn as its own configuration draws a table.
        let style = table_style(&part.samples, options)?;
        // Headed by its project, which then needs no column of its own.
        let label = project_label(
            collection,
            part.samples
                .iter()
                .next()
                .and_then(|entry| entry.path.as_deref()),
        );
        if options.summary {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&summary_table(
                &part.samples,
                &profile,
                options,
                &part.samples,
                true,
                &label,
            )?);
            continue;
        }
        if let Some(format) = options.format {
            // One configuration after all: its rectangle, as a table writes one
            // — grouped, its groups' rows together.
            let samples = match &grouping {
                Some((names, groups)) => {
                    grouped_for_format(&mut profile, &part.samples, names, groups, listed)
                }
                None => part.samples.filter_entries(|_| true),
            };
            let target = samplekit::config::project_config::ExportTarget {
                name: profile.name.clone(),
                profile: profile.name.clone(),
                format,
                output: PathBuf::from("-"),
                filename: false,
                path: false,
                query: None,
            };
            let dataset =
                exports::run_within(&target, &profile, &samples, part.config, &part.samples)
                    .map_err(export_failure)?;
            // The same state column the one-configuration path adds: left out
            // here, `--status` was accepted and silently ignored.
            let dataset = with_states_if_asked(dataset, options, format)?;
            let dataset = noted_empty(&dataset, format);
            text.push_str(
                &export_formats::serialize(&dataset, format)
                    .map_err(|error| Fail::data(error.to_string()))?,
            );
            continue;
        }
        // Each group a table of its own, headed by its values after its
        // project's title where the selection spans several.
        let pieces: Vec<(String, SampleList)> = match &grouping {
            Some((names, groups)) => split_by(&part.samples, groups, listed)
                .into_iter()
                .map(|(key, samples)| {
                    let heading = summaries::group_heading(names, &key);
                    let heading = if projects {
                        format!("{label} · {heading}")
                    } else {
                        heading
                    };
                    (heading, samples)
                })
                .collect(),
            None => vec![(label, part.samples.filter_entries(|_| true))],
        };
        for (heading, samples) in pieces {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&format!("{heading}\n"));
            let guards: Vec<_> = samples.iter().map(|entry| entry.sample.borrow()).collect();
            let subjects: Vec<Subject> = samples
                .iter()
                .zip(guards.iter())
                .map(|(entry, guard)| Subject {
                    sample: guard,
                    path: entry.path.as_deref(),
                    vocabulary: &vocabulary,
                    states: entry.states.as_deref(),
                })
                .collect();
            refuse_disagreeing_units(&profile, &subjects, part.config, semantic_style)?;
            let mut headers = render::columns(&profile, &subjects, part.config, semantic_style);
            let terminal = matches!(output_target(options), Target::Terminal { .. });
            let mut rows = Vec::with_capacity(subjects.len());
            for subject in &subjects {
                let mut row = render::row(&profile, subject, part.config, semantic_style);
                if terminal {
                    mark_row(&profile, subject.sample, &mut row);
                }
                // The same state column the one-configuration path adds.
                if options.status {
                    row.push(row_state_of(&profile, subject, collection, &unmade));
                }
                rows.push(row);
            }
            if options.status {
                if headers.iter().any(|header| header == exports::STATE_HEADER) {
                    return Err(state_column_taken());
                }
                headers.push(exports::STATE_HEADER.to_string());
            }
            let identified = with_identity(
                &profile,
                &subjects,
                part.config,
                options,
                &mut headers,
                &mut rows,
            );
            text.push_str(&with_fewer(render::table_identified(
                headers,
                rows,
                output_target(options),
                style,
                usize::from(identified),
                coloured(),
            )));
        }
    }
    if shown == 0
        && let Some(fail) = refusal
    {
        return Err(fail);
    }
    if let Some(name) = &options.profile {
        warn_undeclared(&undeclared, &format!("profile '{name}'"));
    }
    if options.output.is_none()
        && options.format.is_none()
        && !options.summary
        && !matches!(output_target(options), Target::Pipe)
    {
        text.push_str(&format!(
            "\n{shown} of {}\n",
            counted_as(collection.len(), "sample", "samples")
        ));
    }
    emit_output(&text, options, &selected)
}

/// Each value column's state, in the profile's order: `None` for a column that
/// is not a value, or whose value no record judges. One lookup, which the
/// marks and the `state` column both read, so that they cannot disagree.
fn column_states(
    profile: &Profile,
    sample: &samplekit::core::sample::Sample,
) -> Vec<Option<Freshness>> {
    let states = fingerprint::check(sample).unwrap_or_default();
    profile
        .columns()
        .iter()
        .map(|column| {
            let field = fields::parse(&column.field).ok()?;
            if fields::channel(&field) != fields::Channel::Value {
                return None;
            }
            match &field {
                Field::Named { name, .. } => states.get(name).cloned(),
                Field::Cell {
                    table, column, row, ..
                } => sample
                    .table(table)
                    .ok()
                    .and_then(|held| held.row(row).ok())
                    .and_then(|view| {
                        let index: Vec<Value> = view.index().into_iter().cloned().collect();
                        fingerprint::check_cell_among(sample, table, column, &index).ok()
                    }),
                _ => None,
            }
        })
        .collect()
}

pub(super) fn mark_row(
    profile: &Profile,
    sample: &samplekit::core::sample::Sample,
    cells: &mut [String],
) {
    for (state, cell) in column_states(profile, sample).iter().zip(cells.iter_mut()) {
        if let Some(state) = state {
            *cell = render::with_mark(cell, state, coloured());
        }
    }
}

/// What the row's `state` column says on a drawn table, in the words an
/// export's says them: one rule for every row-shaped format.
fn row_state_of(
    profile: &Profile,
    subject: &Subject,
    collection: &SampleList,
    unmade: &exports::NeverComputed,
) -> String {
    let sample = subject.sample;
    let states = column_states(profile, sample);
    let named: Vec<(String, Option<&'static str>)> = profile
        .columns()
        .iter()
        .zip(&states)
        .map(|(column, state)| {
            let key = column
                .header
                .clone()
                .unwrap_or_else(|| column.field.clone());
            let said = match (state, fields::parse(&column.field)) {
                (Some(state), _) => Some(exports::state_name(state, true)),
                // Nothing judges it and the sample holds none: never computed
                // where its configuration's formula gives it, as a CSV says.
                (
                    None,
                    Ok(Field::Named {
                        name,
                        channel: fields::Channel::Value,
                    }),
                ) => {
                    let held = sample
                        .property(&name)
                        .ok()
                        .and_then(|handle| handle.peek(|held| held.peek_value()))
                        .is_some_and(|value| !value.is_absent());
                    (!held && unmade.holds(collection, subject.path, name.as_str()))
                        .then_some(exports::NEVER_COMPUTED)
                }
                _ => None,
            };
            (key, said)
        })
        .collect();
    exports::row_state(named.iter().map(|(key, state)| (key.as_str(), *state)))
}

/// The sample's identity as a table's first column, in a terminal only and
/// unless a column already names it: `[render] identify`, the name by default.
/// Whether it was added, so that it is greyed.
pub(super) fn with_identity(
    profile: &Profile,
    subjects: &[Subject],
    config: Option<&ProjectConfig>,
    options: &Options,
    headers: &mut Vec<String>,
    rows: &mut [Vec<String>],
) -> bool {
    use samplekit::config::project_config::Identify;
    if !matches!(output_target(options), Target::Terminal { .. }) {
        return false;
    }
    let identify = config
        .map(|config| config.render().identify)
        .unwrap_or_default();
    let header = match identify {
        Identify::None => return false,
        Identify::Name => "name",
        Identify::Filename => "file",
    };
    if profile
        .columns()
        .iter()
        .any(|column| matches!(column.field.as_str(), "name" | "filename" | "path"))
    {
        return false;
    }
    let counts = name_counts(
        subjects
            .iter()
            .map(|subject| subject.sample.name().map(str::to_string)),
    );
    headers.insert(0, header.to_string());
    for (subject, row) in subjects.iter().zip(rows.iter_mut()) {
        let identity = match identify {
            Identify::Filename => subject.path.and_then(Path::file_name).map_or_else(
                || file_of(subject.path, subject.sample),
                |file| file.to_string_lossy().into_owned(),
            ),
            _ => labelled(&counts, subject.path, subject.sample),
        };
        row.insert(0, render::truncate_middle(&identity, IDENTITY_WIDTH));
    }
    true
}

/// A dataset without its uncertainty columns empty for every sample, each said
/// on stderr: a column nobody can fill is read as an uncertainty of zero, or as
/// a defect of the export. JSON writes a quantity as one object whose
/// `uncertainty` stays, null.
pub(super) fn noted_empty(
    dataset: &export_formats::Dataset,
    format: samplekit::config::project_config::Format,
) -> export_formats::Dataset {
    let (trimmed, empty) = export_formats::without_empty_uncertainties(dataset);
    if !matches!(format, samplekit::config::project_config::Format::Json) {
        for header in empty {
            errln!("note: {header} is left out: it is empty for every sample");
        }
    }
    trimmed
}

/// The configuration every selected sample shares, or the list's own.
pub(super) fn shared_config<'a>(
    collection: &'a SampleList,
    selected: &SampleList,
) -> Option<&'a ProjectConfig> {
    if !collection.spans_configurations() {
        return collection.config();
    }
    let mut files = selected
        .iter()
        .filter_map(|entry| entry.path.as_deref())
        .map(|path| (path, collection.configuration_of(path)));
    let Some((first, file)) = files.next() else {
        return collection.config();
    };
    if files.all(|(_, other)| other == file) {
        collection.config_for(first)
    } else {
        collection.config()
    }
}

/// Where a sample's configuration is, relative to the current directory: the
/// project a row belongs to.
pub(super) fn project_label(collection: &SampleList, path: Option<&Path>) -> String {
    let Some(directory) = path
        .and_then(|path| collection.configuration_of(path))
        .and_then(Path::parent)
    else {
        return ".".to_string();
    };
    let own = dunce::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let here = std::env::current_dir()
        .ok()
        .and_then(|here| dunce::canonicalize(here).ok());
    match here.and_then(|here| own.strip_prefix(here).ok().map(Path::to_path_buf)) {
        Some(relative) if relative.as_os_str().is_empty() => ".".to_string(),
        Some(relative) => relative.display().to_string(),
        None => directory.display().to_string(),
    }
}

/// The project each row belongs to, in a terminal, when the selection spans
/// configurations. Whether the column was added, so that the renderer knows how
/// many of the leading columns are the terminal's own rather than asked for.
pub(super) fn with_project(
    collection: &SampleList,
    selected: &SampleList,
    options: &Options,
    headers: &mut Vec<String>,
    rows: &mut [Vec<String>],
) -> bool {
    // Asked for as a column, it is one already.
    if !collection.spans_configurations()
        || !matches!(output_target(options), Target::Terminal { .. })
        || headers.iter().any(|header| header == "project")
    {
        return false;
    }
    headers.insert(0, "project".to_string());
    for (entry, row) in selected.iter().zip(rows.iter_mut()) {
        row.insert(0, project_label(collection, entry.path.as_deref()));
    }
    true
}

/// A part's heading: the configuration describing its samples.
pub(super) fn configuration_title(file: Option<&Path>) -> String {
    match file {
        Some(file) if file.is_file() => file.display().to_string(),
        Some(file) => format!(
            "{} (no .samplekitrc)",
            file.parent()
                .map_or_else(|| ".".to_string(), |parent| parent.display().to_string())
        ),
        None => "no .samplekitrc".to_string(),
    }
}

/// Samples left out because their configuration declares no such thing. Columns
/// a summary leaves out, holding text: several are one line, each under `-v`.
fn warn_left_out(left_out: &[String]) {
    let each: Vec<String> = left_out
        .iter()
        .map(|one| format!("{one} left out of the summary: it holds text, and text has no mean"))
        .collect();
    warn_several(
        &format!(
            "{} columns left out of the summary: they hold text, and text has no mean",
            left_out.len()
        ),
        &each,
    );
}

/// Several configurations lacking it are one line, each under `-v`.
pub(super) fn warn_undeclared(undeclared: &[(Option<PathBuf>, usize)], what: &str) {
    let each: Vec<String> = undeclared
        .iter()
        .map(|(file, count)| {
            format!(
                "{} not shown: {} declares no {what}",
                counted_as(*count, "sample", "samples"),
                configuration_title(file.as_deref())
            )
        })
        .collect();
    let samples: usize = undeclared.iter().map(|(_, count)| count).sum();
    warn_several(
        &format!(
            "{} not shown: {} configurations declare no {what}",
            counted_as(samples, "sample", "samples"),
            undeclared.len()
        ),
        &each,
    );
}

pub(super) fn summary_rows(
    selected: &SampleList,
    profile: &Profile,
    options: &Options,
    collection: &SampleList,
    lenient: bool,
) -> Result<Vec<SummaryRow>, Fail> {
    let (rows, left_out) = summaries::rows(
        selected,
        profile,
        collection,
        options.style.as_deref(),
        lenient,
    )
    .map_err(Fail::data)?;
    warn_left_out(&left_out);
    Ok(rows)
}

/// A summary as a table, or as CSV, TSV or JSON when one is asked for: the
/// count with its denominator, the mean, the sample deviation `s`, its standard
/// error `sem`, the median and the extremes.
pub(super) fn summary_table(
    selected: &SampleList,
    profile: &Profile,
    options: &Options,
    collection: &SampleList,
    lenient: bool,
    label: &str,
) -> Result<String, Fail> {
    if options.group.is_some() {
        return grouped_summary(selected, profile, options, collection, lenient, label);
    }
    let rows = summary_rows(selected, profile, options, collection, lenient)?;
    if let Some(format) = options.format {
        let headers = [
            "column", "unit", "n", "of", "mean", "weights", "s", "sem", "median", "min", "max",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect();
        let cells = rows
            .iter()
            .map(|row| {
                // At the column's precision, as the ordinary rows of a data
                // format are: the screen showed `3.0571` and the file held
                // `3.057066666666667`, the defect this began with, still alive
                // on this one path.
                let number = |value: Option<f64>| {
                    let written = value.and_then(|value| match &row.precision {
                        Some(precision) => summaries::number(value, Some(precision))
                            .parse::<f64>()
                            .ok()
                            .or(Some(value)),
                        None => Some(value),
                    });
                    export_formats::Cell::Scalar(
                        written.and_then(|value| Value::number(value).ok()),
                    )
                };
                let summary = row.summary.as_ref();
                vec![
                    export_formats::Cell::Scalar(Some(Value::text(row.label.clone()))),
                    export_formats::Cell::Scalar(row.unit.clone().map(Value::text)),
                    export_formats::Cell::Scalar(Some(Value::integer(
                        summary.map_or(0, |summary| summary.summary.count.get()) as i64,
                    ))),
                    export_formats::Cell::Scalar(Some(Value::integer(
                        summary.map_or(0, |summary| summary.considered) as i64,
                    ))),
                    number(summary.map(list::ColumnSummary::mean)),
                    weights_cell(summary),
                    number(summary.and_then(|summary| summary.summary.sample_stdev)),
                    number(summary.and_then(|summary| summary.summary.standard_error)),
                    number(summary.map(|summary| summary.summary.median)),
                    number(summary.map(|summary| summary.summary.minimum)),
                    number(summary.map(|summary| summary.summary.maximum)),
                ]
            })
            .collect();
        let dataset = export_formats::Dataset {
            headers,
            rows: cells,
            ..Default::default()
        };
        return export_formats::serialize(&dataset, format)
            .map_err(|error| Fail::data(error.to_string()));
    }
    // The mean's heading says how it was taken.
    let mean = summaries::mean_header(&rows);
    let headers = [
        label,
        "n",
        mean.as_str(),
        "s",
        "sem",
        "median",
        "min",
        "max",
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    let cells = rows
        .iter()
        .map(|row| {
            let label = match &row.unit {
                Some(unit) => format!("{} [{unit}]", row.label),
                None => row.label.clone(),
            };
            let mut cells = vec![label];
            cells.extend(summaries::written(row));
            cells
        })
        .collect();
    let style = table_style(collection, options)?;
    Ok(render::table(headers, cells, output_target(options), style))
}

/// How a summary's mean was taken, as a data format writes it beside the mean:
/// `1/u²`, or empty for the plain mean.
fn weights_cell(summary: Option<&list::ColumnSummary>) -> export_formats::Cell {
    export_formats::Cell::Scalar(
        summary
            .filter(|summary| summary.weighted_mean.is_some())
            .map(|_| Value::text("1/u²")),
    )
}

/// The fields `--group` names, as written and parsed.
pub(super) type Grouping = (Vec<String>, Vec<Field>);

/// What `--group` names: the fields as written, for headings and columns, and
/// parsed. A field nobody holds is refused as a column's is, with the nearest:
/// it grouped every sample under one placeholder, exit 0.
pub(super) fn grouping(
    options: &Options,
    collection: &SampleList,
) -> Result<Option<Grouping>, Fail> {
    let Some(group) = &options.group else {
        return Ok(None);
    };
    let names: Vec<String> = group
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    if names.is_empty() {
        return Err(Fail::usage(
            "--group names a field, or several separated by commas: --group style,yeast",
        ));
    }
    let parsed: Vec<Field> = names
        .iter()
        .map(|name| fields::parse(name).map_err(|error| Fail::usage(error.to_string())))
        .collect::<Result<_, _>>()?;
    let asked: Vec<ColumnSpec> = names
        .iter()
        .map(|name| ColumnSpec {
            field: name.clone(),
            label: None,
            header: None,
            precision: None,
            template: None,
        })
        .collect();
    check_columns(collection, &anonymous(&asked, None))?;
    Ok(Some((names, parsed)))
}

/// `--group`, or else the groups the profile named declares. Over several
/// projects each one's profile of that name is read, and the groups are made
/// over the whole selection: profiles grouping differently cannot all be
/// followed, and which one wins would be a guess.
pub(super) fn with_profile_groups(
    collection: &SampleList,
    options: &Options,
) -> Result<Options, Fail> {
    let (None, Some(name)) = (&options.group, &options.profile) else {
        return Ok(options.clone());
    };
    let mut declared: Vec<(Option<PathBuf>, Vec<String>)> = Vec::new();
    for (file, config) in declaring(collection) {
        if let Ok(profile) = config.profile(name)
            && !declared.iter().any(|(_, group)| *group == profile.group)
        {
            declared.push((file, profile.group.clone()));
        }
    }
    match declared.as_slice() {
        [] => Ok(options.clone()),
        [(_, group)] => Ok(Options {
            group: (!group.is_empty()).then(|| group.join(",")),
            ..options.clone()
        }),
        several => {
            let said: Vec<String> = several
                .iter()
                .map(|(file, group)| {
                    format!(
                        "{} in {}",
                        if group.is_empty() {
                            "by nothing".to_string()
                        } else {
                            format!("by {}", group.join(", "))
                        },
                        configuration_title(file.as_deref())
                    )
                })
                .collect();
            Err(Fail::usage(format!(
                "the profiles '{name}' group differently: {}\n  one selection has one \
                 grouping — give --group to choose it, --group {} for instance",
                said.join("; "),
                several
                    .iter()
                    .find(|(_, group)| !group.is_empty())
                    .map_or("FIELD".to_string(), |(_, group)| group.join(","))
            )))
        }
    }
}

/// The order groups come in: the sort's where one is in force.
pub(super) fn group_order(sorted: bool) -> list::GroupOrder {
    if sorted {
        list::GroupOrder::Listed
    } else {
        list::GroupOrder::Values
    }
}

/// A selection split into its groups, or a refusal in the words a column's
/// would use: a table named whole is no value per sample.
pub(super) fn groups_by(
    samples: &SampleList,
    fields: &[Field],
    order: list::GroupOrder,
) -> Result<Vec<list::Group>, Fail> {
    samples.group_by(fields, order).map_err(group_failure)
}

pub(super) fn group_failure(error: list::ListError) -> Fail {
    match error {
        list::ListError::Field(_) => Fail::usage(error.to_string()),
        other => Fail::data(other.to_string()),
    }
}

/// Samples split by groups made over a wider selection: each group's samples
/// among them, in their own order, none empty. The groups come in theirs, or,
/// `listed`, where their first sample comes among `samples` — the sort in
/// force, which the wider selection was not made in.
pub(super) fn split_by(
    samples: &SampleList,
    groups: &[list::Group],
    listed: bool,
) -> Vec<(Vec<Value>, SampleList)> {
    let mut pieces: Vec<(Vec<Value>, SampleList)> = groups
        .iter()
        .filter_map(|group| {
            let members: std::collections::HashSet<_> = group
                .samples
                .iter()
                .map(|entry| std::rc::Rc::as_ptr(&entry.sample))
                .collect();
            let kept = samples
                .filter_entries(|entry| members.contains(&std::rc::Rc::as_ptr(&entry.sample)));
            (!kept.is_empty()).then(|| (group.key.clone(), kept))
        })
        .collect();
    if listed {
        let at: std::collections::HashMap<_, usize> = samples
            .iter()
            .enumerate()
            .map(|(at, entry)| (std::rc::Rc::as_ptr(&entry.sample), at))
            .collect();
        pieces.sort_by_key(|(_, part)| {
            part.iter()
                .next()
                .and_then(|entry| at.get(&std::rc::Rc::as_ptr(&entry.sample)).copied())
                .unwrap_or(usize::MAX)
        });
    }
    pieces
}

/// A grouped selection as a data format writes it: one table, each group's rows
/// together, and each field grouped by a first column where the columns asked
/// for do not already name it — a file has no place for a heading, and a
/// spreadsheet groups again by a column.
pub(super) fn grouped_for_format(
    profile: &mut Profile,
    samples: &SampleList,
    names: &[String],
    groups: &[list::Group],
    listed: bool,
) -> SampleList {
    *profile = profile.carrying(names);
    let entries = split_by(samples, groups, listed)
        .into_iter()
        .flat_map(|(_, part)| part.iter().cloned().collect::<Vec<_>>())
        .collect();
    list::from_entries(entries)
}

/// A summary per group of `--group` — per combination of values where it names
/// several: the summary's rows under each, a column per field, the groups in
/// [`list::SampleList::group_by`]'s order.
fn grouped_summary(
    selected: &SampleList,
    profile: &Profile,
    options: &Options,
    collection: &SampleList,
    lenient: bool,
    label: &str,
) -> Result<String, Fail> {
    let Some((names, groups)) = grouping(options, collection)? else {
        return Err(Fail::usage("--group names no field"));
    };
    // The group is refused as a column is before any summary is made. The
    // summaries come in the sort's order where one is in force.
    let order = group_order(sort_spec(collection, options, &profile.sort)?.is_some());
    groups_by(selected, &groups, order)?;
    let (parts, left_out) = summaries::grouped(
        selected,
        profile,
        collection,
        options.style.as_deref(),
        lenient,
        &groups,
        order,
    )
    .map_err(Fail::data)?;
    warn_left_out(&left_out);
    if let Some(format) = options.format {
        let mut headers: Vec<String> = names.clone();
        headers.extend(
            [
                "column", "unit", "n", "of", "mean", "weights", "s", "sem", "median", "min", "max",
            ]
            .iter()
            .map(|name| name.to_string()),
        );
        let mut cells = Vec::new();
        for (key, rows) in &parts {
            for row in rows {
                let summary = row.summary.as_ref();
                let number = |value: Option<f64>| {
                    let written = value.and_then(|value| match &row.precision {
                        Some(precision) => summaries::number(value, Some(precision))
                            .parse::<f64>()
                            .ok()
                            .or(Some(value)),
                        None => Some(value),
                    });
                    export_formats::Cell::Scalar(
                        written.and_then(|value| Value::number(value).ok()),
                    )
                };
                // The value as the samples hold it, typed: a number stays
                // one, and none is an empty cell — a data format carries no
                // placeholder.
                let mut line: Vec<export_formats::Cell> = key
                    .iter()
                    .map(|value| {
                        export_formats::Cell::Scalar((!value.is_absent()).then(|| value.clone()))
                    })
                    .collect();
                line.extend([
                    export_formats::Cell::Scalar(Some(Value::text(row.label.clone()))),
                    export_formats::Cell::Scalar(row.unit.clone().map(Value::text)),
                    export_formats::Cell::Scalar(Some(Value::integer(
                        summary.map_or(0, |summary| summary.summary.count.get()) as i64,
                    ))),
                    export_formats::Cell::Scalar(Some(Value::integer(
                        summary.map_or(0, |summary| summary.considered) as i64,
                    ))),
                    number(summary.map(list::ColumnSummary::mean)),
                    weights_cell(summary),
                    number(summary.and_then(|summary| summary.summary.sample_stdev)),
                    number(summary.and_then(|summary| summary.summary.standard_error)),
                    number(summary.map(|summary| summary.summary.median)),
                    number(summary.map(|summary| summary.summary.minimum)),
                    number(summary.map(|summary| summary.summary.maximum)),
                ]);
                cells.push(line);
            }
        }
        let dataset = export_formats::Dataset {
            headers,
            rows: cells,
            ..Default::default()
        };
        return export_formats::serialize(&dataset, format)
            .map_err(|error| Fail::data(error.to_string()));
    }
    let mean = summaries::mean_header(parts.iter().flat_map(|(_, rows)| rows));
    let mut headers: Vec<String> = names.clone();
    headers.extend(
        [
            label,
            "n",
            mean.as_str(),
            "s",
            "sem",
            "median",
            "min",
            "max",
        ]
        .iter()
        .map(|name| name.to_string()),
    );
    let mut cells = Vec::new();
    for (key, rows) in &parts {
        for (at, row) in rows.iter().enumerate() {
            let label = match &row.unit {
                Some(unit) => format!("{} [{unit}]", row.label),
                None => row.label.clone(),
            };
            // The values once, on the group's first row.
            let mut line: Vec<String> = if at == 0 {
                key.iter().map(summaries::group_value).collect()
            } else {
                vec![String::new(); key.len()]
            };
            line.push(label);
            line.extend(summaries::written(row));
            cells.push(line);
        }
    }
    let style = table_style(collection, options)?;
    Ok(render::table(headers, cells, output_target(options), style))
}

/// Each selected sample shown whole, as the workbench's sample screen shows it
/// : its values, then its tables unfolded, the note with `--note`.
pub(super) fn view(options: &Options, note: bool) -> Outcome {
    let root = options
        .positionals
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    let target = output_target(options);
    let semantic_style = render_style(&collection, options)?;
    let vocabulary = selected.vocabulary();
    for (at, entry) in selected.iter().enumerate() {
        let sample = entry.sample.borrow();
        let subject = Subject {
            sample: &sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        };
        // Each sample in its own configuration's units and precisions.
        let config = entry
            .path
            .as_deref()
            .map_or(collection.config(), |path| collection.config_for(path));
        if at > 0 {
            outln!();
        }
        out!(
            "{}",
            render::sheet(
                &subject,
                config,
                semantic_style,
                options.precision.as_ref(),
                target,
                note,
            )
        );
    }
    Ok(())
}

/// A configured dataset, regenerated. `-` as the destination writes to stdout,
/// for a redirection the author controls.
pub(super) fn export(options: &Options) -> Outcome {
    let name = options.positionals.first().cloned();
    let root = options
        .positionals
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    if collection.spans_configurations() {
        return export_by_configuration(&collection, options, name, &root);
    }
    let config = collection
        .config()
        .ok_or_else(|| Fail::usage(needs_configuration(&collection, "export")))?;
    let Some(name) = name else {
        let names = config.export_names();
        return Err(Fail::usage(format!(
            "export wants a declared export's name\n  {}",
            if names.is_empty() {
                "none is declared here".to_string()
            } else {
                format!("available: {}", names.join(", "))
            }
        )));
    };
    // The declaration is checked before anything is selected.
    config
        .export(&name)
        .map_err(|error| Fail::usage(error.to_string()))?;
    let selected = select(&collection, &unordered(options), &root)?;
    export_one(&name, config, selected, &collection, &collection, options)
}

/// The selection alone: an export orders it itself, by `--sort` or by its
/// profile's order, which `-r` reverses.
fn unordered(options: &Options) -> Options {
    Options {
        sort: Vec::new(),
        reverse: false,
        ..options.clone()
    }
}

/// A declared export over samples several configurations describe: each
/// configuration declaring it writes its own samples, to its own destination.
pub(super) fn export_by_configuration(
    collection: &SampleList,
    options: &Options,
    name: Option<String>,
    root: &Path,
) -> Outcome {
    let everything = collection.filter_by(|_| true);
    let Some(name) = name else {
        let mut names: Vec<&str> = collection
            .by_configuration(&everything)
            .into_iter()
            .filter_map(|part| part.config)
            .flat_map(ProjectConfig::export_names)
            .collect();
        names.sort_unstable();
        names.dedup();
        return Err(Fail::usage(format!(
            "export wants a declared export's name\n  {}",
            if names.is_empty() {
                "none is declared here".to_string()
            } else {
                format!("available: {}", names.join(", "))
            }
        )));
    };
    let selected = select(collection, &unordered(options), root)?;
    let parts = collection.by_configuration(&selected);
    let whole = collection.by_configuration(&collection.filter_by(|_| true));
    let declares =
        |config: Option<&ProjectConfig>| config.is_some_and(|config| config.export(&name).is_ok());
    let declaring = parts.iter().filter(|part| declares(part.config)).count();
    if declaring == 0 {
        let message = parts
            .iter()
            .find_map(|part| part.config.and_then(|config| config.export(&name).err()))
            .map_or_else(
                || needs_configuration(collection, "export"),
                |error| error.to_string(),
            );
        return Err(Fail::usage(message));
    }
    if declaring > 1 && options.output.is_some() {
        return Err(Fail::usage(format!(
            "-o names one file, and {declaring} configurations declare '{name}'\n  \
             export each project on its own, or leave each its declared destination"
        )));
    }
    // Two collections writing one file: the second would replace the first
    // without a word, as an export imported by both does unless its destination
    // names the collection.
    let mut destinations: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    for part in &parts {
        let Some(config) = part.config else {
            continue;
        };
        let Ok(target) = config.export(&name) else {
            continue;
        };
        if target.output == Path::new("-") {
            continue;
        }
        let written = config.resolve(&target.output.to_string_lossy());
        let place = std::path::absolute(&written).unwrap_or(written);
        let place: PathBuf = place.components().fold(PathBuf::new(), |mut place, part| {
            match part {
                std::path::Component::ParentDir => {
                    place.pop();
                }
                std::path::Component::CurDir => {}
                part => place.push(part),
            }
            place
        });
        if let Some((_, other)) = destinations.iter().find(|(held, _)| *held == place) {
            return Err(Fail::usage(format!(
                "'{name}' would write {} for {} and for {}: the second would replace the \
                 first\n  name the collection in its output — output = \"…/{{collection}}/…\" — \
                 or export each on its own",
                relative_to_here(&place),
                configuration_title(other.as_deref()),
                configuration_title(part.file.as_deref())
            )));
        }
        destinations.push((place, part.file.clone()));
    }
    // Configurations declaring no such export: one line, each under -v.
    let mut left_out: Vec<String> = Vec::new();
    let mut samples_left_out = 0;
    for part in parts {
        match part.config {
            Some(config) if config.export(&name).is_ok() => {
                let described = whole
                    .iter()
                    .find(|other| other.file == part.file)
                    .map_or(collection, |other| &other.samples);
                export_one(&name, config, part.samples, collection, described, options)?;
            }
            _ => {
                samples_left_out += part.samples.len();
                left_out.push(format!(
                    "{} not exported: {} declares no export '{name}'",
                    counted_as(part.samples.len(), "sample", "samples"),
                    configuration_title(part.file.as_deref())
                ));
            }
        }
    }
    warn_several(
        &format!(
            "{} not exported: {} configurations declare no export '{name}'",
            counted_as(samples_left_out, "sample", "samples"),
            left_out.len()
        ),
        &left_out,
    );
    Ok(())
}

/// One configuration's declared export over the samples it describes.
pub(super) fn export_one(
    name: &str,
    config: &ProjectConfig,
    mut selected: SampleList,
    collection: &SampleList,
    // Every sample this configuration describes, selected or not: what the
    // columns are checked against and what says which are quantities.
    described: &SampleList,
    options: &Options,
) -> Outcome {
    let target = config
        .export(name)
        .cloned()
        .map_err(|error| Fail::usage(error.to_string()))?;
    let mut profile = config
        .profile(&target.profile)
        .cloned()
        .map_err(|error| Fail::usage(error.to_string()))?;
    // An export carries the declared precision; `--precision` overrides it for
    // this invocation, as it does for a displayed table. It is how a script
    // asks for the whole number: `--precision .6e`.
    if let Some(precision) = &options.precision {
        for column in &mut profile.columns {
            column.precision = Some(precision.clone());
        }
    }
    // Against what was selected from, as a table's are: a selection that kept
    // nothing, or only samples lacking a column, is not a misspelt field. The
    // fields it groups by are checked the same.
    check_columns(described, &profile.carrying(&profile.group))?;
    // An export may carry its own selection: the query it declares is part of
    // it, and a filter or a query on the command line narrows further, as every
    // `--filter` and `--query` must hold.
    if let Some(query) = &target.query {
        let declared =
            named_queries::named(config, query).map_err(|error| Fail::usage(error.to_string()))?;
        let file = config.root().join(".samplekitrc");
        query_parses(query, &declared.filter, Some(&file))?;
        let parsed = filter::parse(&declared.filter).map_err(|error| {
            Fail::usage(
                filter::caret(&declared.filter, &error).unwrap_or_else(|| error.to_string()),
            )
        })?;
        selected = selected
            .filter(&parsed)
            .map_err(|error| Fail::data(error.to_string()))?;
    }
    if !options.filter.is_empty() || !options.query.is_empty() {
        // One export per configuration: said once over several.
        warn_gathered(
            "export narrowed",
            |count| {
                format!(
                    "{count} files are written from part of their samples: a filter or a \
                     query narrowed them"
                )
            },
            format!(
                "'{name}' is written from {} of {}: a filter or a query narrowed it",
                selected.len(),
                counted_as(collection.len(), "sample", "samples")
            ),
        );
    }
    // An export sorts by its profile's order, so that regenerating from
    // unchanged data produces a byte-identical file; -r reverses that order.
    let sorted = sort_spec(collection, options, &profile.sort)?;
    let order = group_order(sorted.is_some());
    if let Some(spec) = sorted {
        selected
            .sort(&spec)
            .map_err(|error| Fail::usage(error.to_string()))?;
    }
    // One table carrying its groups, as a grouped `--csv` is.
    let (profile, selected) =
        exports::grouped(&profile, &selected, order).map_err(group_failure)?;
    // An export says how many values are not current, writes the file all the
    // same, and leaves the exit code alone. None of it was built, so a
    // deliverable was regenerated from a stale number with nothing said — the
    // worst place for silence, because the file leaves the building.
    let exports::Behind {
        stale,
        held,
        samples: behind_samples,
        named,
    } = exports::behind(&selected, &profile);
    // Said in the preview when previewing, and warned when writing outright:
    // the fact has to reach whoever is deciding, and only one of the two is
    // reading at that moment. As a warning alone it was on stderr, suppressed
    // by -q, and left the exit code at 0 — three ways to miss the one thing
    // that matters about the file.
    let not_current = (stale > 0 || held > 0).then(|| {
        let mut said = Vec::new();
        if stale > 0 {
            said.push(format!("{stale} outdated"));
        }
        if held > 0 {
            said.push(format!("{held} edited or without a record"));
        }
        const SHOWN: usize = 5;
        let listed = if named.len() > SHOWN {
            format!(
                "{}, and {} more — samplekit status names them all",
                named[..SHOWN].join(", "),
                named.len() - SHOWN
            )
        } else {
            named.join(", ")
        };
        format!(
            "'{name}' is written from values that are not current: {} in {}\n  \
             {listed}\n  \
             the file is written as they stand — samplekit compute {} {}",
            said.join(", "),
            counted_as(behind_samples, "sample", "samples"),
            // Where this project's samples are: over two projects, the first
            // target named was offered for the other's samples.
            selected
                .iter()
                .find_map(|entry| entry.path.as_deref().and_then(Path::parent))
                .filter(|folder| !folder.as_os_str().is_empty())
                .map(|folder| folder.display().to_string())
                .or_else(|| options.positionals.get(1).cloned())
                .unwrap_or_else(|| ".".to_string()),
            // An override is what compute keeps: --force is its way back.
            match (stale > 0, held > 0) {
                (_, false) => "--write brings them up to date",
                (false, true) => "--force --write gives them back to their formulas",
                (true, true) => {
                    "--write brings the outdated up to date; --force also gives the edited back"
                }
            }
        )
    });

    let dataset = exports::run_within(&target, &profile, &selected, Some(config), described)
        .map_err(export_failure)?;
    let dataset = with_states_if_asked(dataset, options, target.format)?;
    let dataset = noted_empty(&dataset, target.format);
    let text = export_formats::serialize(&dataset, target.format)
        .map_err(|error| Fail::data(error.to_string()))?;

    let destination = options
        .output
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.output.to_string_lossy().into_owned());
    // A file named for another format would hold this one under its name:
    // `-o r.json` wrote CSV into it. Refused, as a figure's extension is.
    if let Some(written) = options
        .output
        .as_ref()
        .filter(|path| *path != Path::new("-"))
        && let Some(named) = written
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "csv" => Some(Format::Csv),
                "tsv" => Some(Format::Tsv),
                "json" => Some(Format::Json),
                _ => None,
            })
        && named != target.format
    {
        let spelled = |format: Format| match format {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Json => "json",
        };
        return Err(Fail::usage(format!(
            "-o {}: '{name}' writes {}, and the extension names {} — name the file \
             .{}, or change format in [export.{name}]",
            written.display(),
            spelled(target.format).to_ascii_uppercase(),
            spelled(named).to_ascii_uppercase(),
            spelled(target.format)
        )));
    }
    if destination == "-" {
        out!("{text}");
        return Ok(());
    }
    // Written on the command line, a path is the shell's, as for every -o;
    // declared in .samplekitrc, it is the project's. Resolved without touching
    // anything: a preview creates no directory.
    let path = match options.output.as_ref() {
        Some(written) => written.clone(),
        None => config.resolve(&destination),
    };
    // What the destination holds, read again just before the write.
    let held = exports::held_at(&path);
    if !options.write {
        outln!(
            "{name} → {}\n  {} to write",
            relative_to_here(&path),
            counted_as(dataset.rows.len(), "row", "rows")
        );
        if let Some(refused) = exports::refusal(&path) {
            outln!("  --write refuses it: {refused}");
        } else if path.exists() && unread_kept() {
            outln!(
                "  the file is there, and --write keeps it: a sample could not be read, \
                 and its rows would be lost"
            );
        } else if path.exists() {
            outln!("  the file is there, and --write replaces it");
        } else if let Some(directory) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
        {
            // A destination's directory, declared or given with -o, is made by
            // --write: said here, before it happens.
            outln!(
                "  {} does not exist, and --write creates it",
                directory.display()
            );
        }
        if let Some(said) = &not_current {
            for line in said.lines() {
                outln!("  {}", line.trim_start());
            }
        }
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }
    // A sample that could not be read has no row: replacing the export would
    // lose what the previous one held of it, and a job regenerating it every
    // night would lose it for good. A new file is written; one there is kept.
    if path.exists() && unread_kept() {
        return Err(Fail::data(format!(
            "{} not replaced: a sample of the selection could not be read, and its rows \
             would be lost — samplekit validate names it",
            path.display()
        )));
    }
    // **On stdout, with --write as in the preview.** A deliverable regenerated
    // from a stale number is the mistake this tool exists to prevent, so it is
    // said where `-q` and a redirected stderr cannot hide it — which as a
    // warning, with `--write`, they both did.
    if let Some(said) = &not_current {
        for line in said.lines() {
            outln!("{}", line.trim_start());
        }
    }
    // The write makes the destination's folder, declared or given. The preview
    // announced the replacement, so --write is the answer to it.
    exports::write_replacing(&text, &path, held.as_deref()).map_err(export_write_failed)?;
    keep_output(&path, &selected);
    errln!(
        "written: {}, {}",
        relative_to_here(&path),
        counted_as(dataset.rows.len(), "row", "rows")
    );
    Ok(())
}

/// Two columns asked for under one header are a question to rephrase; the
/// rest is the data's.
fn export_failure(error: exports::ExportError) -> Fail {
    match error {
        exports::ExportError::DuplicateHeader { .. } => Fail::usage(error.to_string()),
        _ => Fail::data(error.to_string()),
    }
}

/// Whether a sample met while loading could not be read: what an export then
/// writes is short of its rows.
fn unread_kept() -> bool {
    super::UNREAD.load(std::sync::atomic::Ordering::Relaxed)
}

/// A table whose columns `-c` names: its closing line offers fewer of them,
/// which a command with no `-c` did not have to give.
fn with_fewer(rendered: String) -> String {
    rendered.replace(
        render::HIDDEN,
        &format!("{}{}", render::HIDDEN, render::FEWER),
    )
}
