//! The table, `view` and `export`: what is selected, shaped and written out.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

/// The default command: one table.
pub(super) fn table(options: &Options) -> Outcome {
    let root = match options.positionals.first() {
        Some(first) => PathBuf::from(first),
        None => default_target(options)?,
    };
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
    if collection.is_empty() {
        errln!("{}", no_sample_here(&root));
    }
    // Headers speak for every row: one configuration's when all rows share it.
    let header_config = shared_config(&collection, &selected);
    // **Two units in one column are refused on every surface**, a summary and
    // a data format as a table: a mean of litres and millilitres, or a CSV
    // column holding both under no unit, are numbers that do not compare.
    {
        let semantic_style = render_style(&collection, options)?;
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
        refuse_disagreeing_units(&profile, &subjects, header_config, semantic_style)?;
        // Over several projects, each sample's unit is its own project's:
        // one column of a summary or a data format holds them all.
        if collection.spans_configurations() {
            let units = units_across(&collection, &selected, &profile, semantic_style);
            let disagreeing: Vec<String> = units
                .iter()
                .filter(|(_, spellings)| spellings.len() > 1)
                .map(|(field, spellings)| format!("{field} is in {}", spellings.join(" and ")))
                .collect();
            if !disagreeing.is_empty() {
                return Err(Fail::data(format!(
                    "a column cannot say which unit it is in: {} — the projects declare \
                     different units\n  one table per project says each: drop --summary \
                     or the data format, or select one project",
                    disagreeing.join("; ")
                )));
            }
        }
    }
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
    if let Some(format) = resolved_format(options, collection.config())? {
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
            format: format.clone(),
            output: PathBuf::from("-"),
            filename: false,
            path: false,
            query: None,
            style: options.style.clone(),
        };
        // The model described before the states are read: described after, at
        // the preview's line, the first file written had no state column, and
        // the same command run again had one.
        if let Some(config) = collection.config() {
            super::edit::describe_now(config);
        }
        let dataset = exports::run_within(&target, &profile, &selected, header_config, vocabulary)
            .map_err(export_failure)?;
        // JSON already gives each quantity its state; the delimited formats say
        // it as a column, asked for.
        let dataset = with_states_if_asked(dataset, options, &format)?;
        let dataset = noted_empty(&dataset, &format);
        let text = export_formats::serialize(&dataset, format.clone())
            .map_err(|error| Fail::data(error.to_string()))?;
        let behind = behind_said(options, &selected, &profile, collection.config());
        return emit_output_behind(&text, options, &selected, behind);
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
    // Each row in its own configuration's units and precision.
    let configs: Vec<_> = subjects
        .iter()
        .map(|subject| {
            subject
                .path
                .map_or(collection.config(), |path| collection.config_for(path))
        })
        .collect();
    let mut rows: Vec<Vec<String>> = subjects
        .iter()
        .zip(&configs)
        .map(|(subject, own)| render::row(&profile, subject, *own, semantic_style))
        .collect();
    render::even_decimals(&profile, &subjects, &configs, semantic_style, &mut rows);
    if terminal {
        render::paint_cells(&profile, &subjects, &mut rows);
        render::show_links(&profile, &subjects, &mut rows, &known_samples(&collection));
    }
    let mut waits = false;
    for (subject, row) in subjects.iter().zip(rows.iter_mut()) {
        waits |= bracket_waiting(&profile, subject.sample, row);
        if terminal {
            mark_row(&profile, subject.sample, row);
        }
        // A table on a screen or in a pipe says it as a CSV does: the marks are
        // the terminal's, and a pipe loses them.
        if options.status {
            row.push(row_state_of(&profile, subject, &collection, &unmade));
        }
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
    if waits {
        errln!(
            "note: a value in brackets waits for an input emptied since: the number is the one \
             computed before it — samplekit status says for what"
        );
    }
    // This table's columns are named with -c, which the closing line offers.
    let mut text = with_fewer(rendered);
    if options.output.is_none() && !matches!(output_target(options), Target::Pipe) {
        text.push_str(&format!(
            "\n{} of {}\n",
            selected.len(),
            counted_as(collection.len(), "sample", "samples")
        ));
    }
    let behind = behind_said(options, &selected, &profile, collection.config());
    // Piped, a table carries no mark: what it shows that is not current is
    // said beside it, on stderr, for the reader at the terminal.
    if options.output.is_none() && matches!(output_target(options), Target::Pipe) {
        let shown = exports::behind(&selected, &profile, collection.config());
        not_current_shown(shown.stale + shown.held + shown.never + shown.failed);
    }
    emit_output_behind(&text, options, &selected, behind)
}

/// What a folder holding no sample says, rather than nothing: an empty
/// listing read as a failure, or as a collection with nothing in it. The
/// folders below holding samples are named, a project's own `samples/`
/// first among them — the root of a project reads only its own files.
pub(super) fn no_sample_here(root: &Path) -> String {
    let mut below: Vec<(String, usize)> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|entry| {
            let held = std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|file| {
                    file.path()
                        .extension()
                        .is_some_and(|extension| extension == "md")
                        && std::fs::read_to_string(file.path())
                            .is_ok_and(|text| text.starts_with("---"))
                })
                .count();
            (held > 0).then(|| (entry.file_name().to_string_lossy().into_owned(), held))
        })
        .collect();
    below.sort();
    let Some((first, _)) = below.first() else {
        return "no sample here".to_string();
    };
    let held: Vec<String> = below
        .iter()
        .map(|(folder, count)| {
            format!(
                "{} holds {}",
                folder_said(folder),
                counted_as(*count, "sample", "samples")
            )
        })
        .collect();
    let joined = shell_path(&root.join(first));
    let named = joined
        .strip_prefix("./")
        .or_else(|| joined.strip_prefix(".\\"))
        .unwrap_or(&joined);
    format!(
        "no sample here — {}: samplekit {named}, or [collection] recursive = true in \
         .samplekitrc to read the folders below",
        held.join(", ")
    )
}

/// Piped or written to a file, a table carries no `⚠` nor `✎`: how many
/// values it shows are not current is said on stderr, for the reader at the
/// terminal, the cells left clean for the program reading them.
fn not_current_shown(count: usize) {
    if count == 0 {
        return;
    }
    errln!(
        "{} shown {} not current — {} says why",
        counted_as(count, "value", "values"),
        if count == 1 { "is" } else { "are" },
        super::advised("status"),
    );
}

/// `--status` adds the row's state as a column, where the format does not
/// already carry one.
fn with_states_if_asked(
    dataset: export_formats::Dataset,
    options: &Options,
    format: &Format,
) -> Result<export_formats::Dataset, Fail> {
    if options.status && *format != Format::Json {
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

/// The units each column's samples are in, each sample's by its own
/// project's declarations: what a summary, a data format or a figure over
/// several projects puts in one column or on one axis.
pub(super) fn units_across(
    collection: &SampleList,
    selected: &SampleList,
    profile: &Profile,
    style: Option<&str>,
) -> Vec<(String, Vec<String>)> {
    let vocabulary = selected.vocabulary();
    let mut units: Vec<(String, Vec<String>)> = Vec::new();
    for part in collection.by_configuration(selected) {
        let part_guards: Vec<_> = part
            .samples
            .iter()
            .map(|entry| entry.sample.borrow())
            .collect();
        let part_subjects: Vec<Subject> = part
            .samples
            .iter()
            .zip(part_guards.iter())
            .map(|(entry, guard)| Subject {
                sample: guard,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            })
            .collect();
        for (field, spellings) in render::units_of(profile, &part_subjects, part.config, style) {
            match units.iter_mut().find(|(held, _)| *held == field) {
                Some((_, held)) => {
                    for spelling in spellings {
                        if !held.contains(&spelling) {
                            held.push(spelling);
                        }
                    }
                }
                None => units.push((field, spellings)),
            }
        }
    }
    units
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
    let mut behind: Vec<(String, exports::Behind)> = Vec::new();
    for mut part in parts {
        let mut profile = match &anonymous_profile {
            Some(profile) => profile.clone(),
            None => match profile_in(
                part.config,
                &part.samples,
                options,
                options.profile.as_deref(),
            ) {
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
        // The part's own profile's order: the list it was taken from knows
        // the first project's.
        let own_order = Options {
            profile: None,
            ..options.clone()
        };
        let sorted = sort_spec(&part.samples, &own_order, &profile.sort)?;
        let listed = sorted.is_some();
        if let Some(spec) = sorted {
            part.samples
                .sort(&spec)
                .map_err(|error| Fail::usage(error.to_string()))?;
        }
        shown += part.samples.len();
        behind.extend(behind_said(options, &part.samples, &profile, part.config));
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
        if let Some(format) = resolved_format(options, part.config)? {
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
                format: format.clone(),
                output: PathBuf::from("-"),
                filename: false,
                path: false,
                query: None,
                style: options.style.clone(),
            };
            if let Some(config) = part.config {
                super::edit::describe_now(config);
            }
            let dataset =
                exports::run_within(&target, &profile, &samples, part.config, &part.samples)
                    .map_err(export_failure)?;
            // The same state column the one-configuration path adds: left out
            // here, `--status` was accepted and silently ignored.
            let dataset = with_states_if_asked(dataset, options, &format)?;
            let dataset = noted_empty(&dataset, &format);
            text.push_str(
                &export_formats::serialize(&dataset, format.clone())
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
                    let heading = summaries::group_heading_in(names, &key, part.config);
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
            // A style this project does not declare leaves it its own.
            let semantic_style = semantic_style.filter(|style| {
                part.config.map_or(*style == "plain", |config| {
                    config.render_style_names().contains(style)
                })
            });
            refuse_disagreeing_units(&profile, &subjects, part.config, semantic_style)?;
            let mut headers = render::columns(&profile, &subjects, part.config, semantic_style);
            let terminal = matches!(output_target(options), Target::Terminal { .. });
            let mut rows: Vec<Vec<String>> = subjects
                .iter()
                .map(|subject| render::row(&profile, subject, part.config, semantic_style))
                .collect();
            let configs = vec![part.config; subjects.len()];
            render::even_decimals(&profile, &subjects, &configs, semantic_style, &mut rows);
            if terminal {
                render::paint_cells(&profile, &subjects, &mut rows);
                render::show_links(&profile, &subjects, &mut rows, &known_samples(collection));
            }
            for (subject, row) in subjects.iter().zip(rows.iter_mut()) {
                if terminal {
                    mark_row(&profile, subject.sample, row);
                }
                // The same state column the one-configuration path adds.
                if options.status {
                    row.push(row_state_of(&profile, subject, collection, &unmade));
                }
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
    let behind = (!behind.is_empty()).then(|| {
        let mut all = exports::Behind::default();
        for (_, part) in &behind {
            all.add(part);
        }
        let said: Vec<&str> = behind.iter().map(|(said, _)| said.as_str()).collect();
        (said.join("\n"), all)
    });
    emit_output_behind(&text, options, &selected, behind)
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

/// A value that waits for an input emptied since keeps the number computed
/// before it: bracketed, on a screen as in a pipe, where nothing else told it
/// from a current one. Whether any was.
pub(super) fn bracket_waiting(
    profile: &Profile,
    sample: &samplekit::core::sample::Sample,
    cells: &mut [String],
) -> bool {
    let mut any = false;
    for (state, cell) in column_states(profile, sample).iter().zip(cells.iter_mut()) {
        let Some(state) = state else {
            continue;
        };
        let stale = matches!(
            state,
            Freshness::Stale { .. } | Freshness::Broken { .. } | Freshness::Unjudged { .. }
        );
        if stale
            && !cell.trim().is_empty()
            && cell.trim() != "—"
            && !samplekit::collection::editing::emptied_inputs(sample, state).is_empty()
        {
            *cell = format!("({})", cell.trim());
            any = true;
        }
    }
    any
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
    // What the model owes, as the CSV's column and `status` say it: the
    // table said `current` of a value whose formula changed, or waiting.
    let planned = subject.path.and_then(|path| {
        samplekit::collection::computation::planned_states_described(
            collection.config_for(path).or(collection.config()),
            path,
            sample,
        )
    });
    let named: Vec<(String, Option<&'static str>)> = profile
        .columns()
        .iter()
        .zip(&states)
        .map(|(column, state)| {
            let key = column
                .header
                .clone()
                .unwrap_or_else(|| column.field.clone());
            let owed = match fields::parse(&column.field) {
                Ok(Field::Named { name, .. }) => planned
                    .as_ref()
                    .and_then(|planned| planned.get(name.as_str()))
                    .and_then(|said| samplekit::collection::computation::planned_word(said)),
                // A cell's column whose formula changed, as an export says it.
                Ok(Field::Cell { table, column, .. }) => planned
                    .as_ref()
                    .and_then(|planned| planned.get(&format!("{table}.{column}")))
                    .and_then(|said| samplekit::collection::computation::planned_word(said))
                    .filter(|word| *word == "outdated"),
                _ => None,
            };
            // A row's cell its column derives, that no formula filled: a new
            // row's was said current.
            let never_filled = match fields::parse(&column.field) {
                Ok(Field::Cell {
                    table,
                    column,
                    row,
                    channel: fields::Channel::Value,
                }) => {
                    let held = sample.table(&table).ok();
                    let in_table = held.as_ref().is_some_and(|held| held.row(&row).is_ok());
                    let filled = held
                        .as_ref()
                        .and_then(|held| held.at(&row, &column).ok())
                        .is_some_and(|cell| {
                            cell.records().computed.is_some()
                                || cell.records().failure.is_some()
                                || cell.peek_value().is_some_and(|value| !value.is_absent())
                        });
                    in_table
                        && !filled
                        && unmade.holds(collection, subject.path, &format!("{table}.{column}"))
                }
                _ => false,
            };
            let said = match (state, fields::parse(&column.field)) {
                _ if owed.is_some() => owed,
                _ if never_filled => Some(exports::NEVER_COMPUTED),
                // Outdated for want of an input emptied since: it waits.
                (Some(state), _)
                    if matches!(
                        state,
                        Freshness::Stale { .. }
                            | Freshness::Broken { .. }
                            | Freshness::Unjudged { .. }
                    ) && !samplekit::collection::editing::emptied_inputs(sample, state)
                        .is_empty() =>
                {
                    Some("waiting")
                }
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
    format: &samplekit::config::project_config::Format,
) -> export_formats::Dataset {
    noted_empty_in(dataset, format, "for every sample")
}

/// [`noted_empty`], saying where it is empty: `in every row` of a sample's
/// table.
fn noted_empty_in(
    dataset: &export_formats::Dataset,
    format: &samplekit::config::project_config::Format,
    where_empty: &str,
) -> export_formats::Dataset {
    let (trimmed, empty) = export_formats::without_empty_uncertainties(dataset);
    // Said where a column was left: a document writing each property in one
    // cell has no uncertainty column to leave out.
    let columns = match format {
        samplekit::config::project_config::Format::Json => false,
        samplekit::config::project_config::Format::Text(document) => document.property.is_none(),
        _ => true,
    };
    if columns {
        for header in empty {
            errln!("note: {header} is left out: it is empty {where_empty}");
        }
    }
    trimmed
}

/// The style a summary spells its units in: in a data format, the files'
/// forms — `degC` — as an export writes them, where it wrote the screen's
/// `°C`; `--style` otherwise.
fn data_style<'a>(options: &'a Options, format: Option<&'a Format>) -> Option<&'a str> {
    match (options.style.as_deref(), format) {
        (Some(style), _) => Some(style),
        (None, Some(Format::Csv | Format::Tsv | Format::Json)) => {
            Some(samplekit::config::project_config::FILES_STYLE)
        }
        // A document's style is the one its file is: `-o s.tex` writes as
        // `--style latex` does, where it spelt `°C` beside the other's
        // `degC`.
        (None, Some(Format::Text(document))) => Some(document.name.as_str()),
        (None, _) => None,
    }
}

/// A summary's number in a data format: the text the screen shows of it —
/// `4.8490` at `.4f`, where the file held `4.849` — which reads as the
/// number; the number itself where the screen writes none.
fn summary_cell(value: Option<f64>, shown: Option<&str>) -> export_formats::Cell {
    let Some(number) = value.and_then(|value| Value::number(value).ok()) else {
        return export_formats::Cell::Scalar(None);
    };
    match shown {
        Some(text) if text.parse::<f64>().is_ok() => export_formats::Cell::Written {
            value: number,
            text: text.to_string(),
        },
        _ => export_formats::Cell::Scalar(Some(number)),
    }
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
    collection: &SampleList,
    lenient: bool,
    style: Option<&str>,
) -> Result<Vec<SummaryRow>, Fail> {
    let (rows, left_out) =
        summaries::rows(selected, profile, collection, style, lenient).map_err(Fail::data)?;
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
    let format = resolved_format(options, collection.config())?;
    let rows = summary_rows(
        selected,
        profile,
        collection,
        lenient,
        data_style(options, format.as_ref()),
    )?;
    if let Some(format) = format {
        let headers = [
            "column",
            "unit",
            "n",
            "of",
            "not_applicable",
            "mean",
            "weights",
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
                // At the column's precision, as the ordinary rows of a data
                // format are: the screen showed `3.0571` and the file held
                // `3.057066666666667`, the defect this began with, still alive
                // on this one path.
                // The screen's own text of each, as a table's CSV writes it.
                let shown = summaries::written(row, false);
                let at = |column: usize, value: Option<f64>| {
                    summary_cell(value, shown.get(column).map(String::as_str))
                };
                let summary = row.summary.as_ref();
                vec![
                    export_formats::Cell::Scalar(Some(Value::text(row.label.clone()))),
                    export_formats::Cell::Scalar(row.unit.clone().map(Value::text)),
                    export_formats::Cell::Scalar(Some(Value::integer(
                        summary.map_or(0, |summary| summary.summary.count.get()) as i64,
                    ))),
                    export_formats::Cell::Scalar(Some(Value::integer(row.of as i64))),
                    export_formats::Cell::Scalar(Some(Value::integer(row.not_applicable as i64))),
                    at(1, summary.map(list::ColumnSummary::mean)),
                    weights_cell(summary),
                    at(2, summary.and_then(|summary| summary.summary.sample_stdev)),
                    at(3, summary.and_then(list::ColumnSummary::standard_error)),
                    at(4, summary.map(|summary| summary.summary.median)),
                    at(5, summary.map(|summary| summary.summary.minimum)),
                    at(6, summary.map(|summary| summary.summary.maximum)),
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
    let mixed = summaries::weighting_mixed(&rows);
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
            cells.extend(summaries::written(row, mixed));
            cells
        })
        .collect();
    let (headers, cells) = painted_summary(headers, cells, 0, options);
    let style = table_style(collection, options)?;
    Ok(render::table(headers, cells, output_target(options), style))
}

/// A summary as a terminal shows it, where the palette is on: its headers
/// bold, each row's label as a header is with its unit, the groups' values as
/// attributes and the statistics as values. `grouped` columns come first.
fn painted_summary(
    headers: Vec<String>,
    cells: Vec<Vec<String>>,
    grouped: usize,
    options: &Options,
) -> (Vec<String>, Vec<Vec<String>>) {
    if !matches!(output_target(options), Target::Terminal { .. }) {
        return (headers, cells);
    }
    let palette = render::palette();
    let cells = cells
        .into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(at, cell)| {
                    if at < grouped {
                        render::paint(&cell, palette.attribute)
                    } else if at == grouped {
                        render::painted_header(&cell)
                    } else if matches!(cell.as_str(), "" | "—") {
                        cell
                    } else {
                        render::paint(&cell, palette.value)
                    }
                })
                .collect()
        })
        .collect();
    (headers, cells)
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
            "--group names a field, or several separated by commas: --group style,process",
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
    // Said as the groups a text format marks, `\midrule` between two.
    profile.group = names.to_vec();
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
    let format = resolved_format(options, collection.config())?;
    let (parts, left_out) = summaries::grouped(
        selected,
        profile,
        collection,
        data_style(options, format.as_ref()),
        lenient,
        &groups,
        order,
    )
    .map_err(Fail::data)?;
    warn_left_out(&left_out);
    if let Some(format) = resolved_format(options, selected.config())? {
        let mut headers: Vec<String> = names.clone();
        headers.extend(
            [
                "column",
                "unit",
                "n",
                "of",
                "not_applicable",
                "mean",
                "weights",
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
            for row in rows {
                let summary = row.summary.as_ref();
                // The screen's own text of each, as a table's CSV writes it.
                let shown = summaries::written(row, false);
                let at = |column: usize, value: Option<f64>| {
                    summary_cell(value, shown.get(column).map(String::as_str))
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
                    export_formats::Cell::Scalar(Some(Value::integer(row.of as i64))),
                    export_formats::Cell::Scalar(Some(Value::integer(row.not_applicable as i64))),
                    at(1, summary.map(list::ColumnSummary::mean)),
                    weights_cell(summary),
                    at(2, summary.and_then(|summary| summary.summary.sample_stdev)),
                    at(3, summary.and_then(list::ColumnSummary::standard_error)),
                    at(4, summary.map(|summary| summary.summary.median)),
                    at(5, summary.map(|summary| summary.summary.minimum)),
                    at(6, summary.map(|summary| summary.summary.maximum)),
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
    let mixed = summaries::weighting_mixed(parts.iter().flat_map(|(_, rows)| rows));
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
            line.extend(summaries::written(row, mixed));
            cells.push(line);
        }
    }
    let (headers, cells) = painted_summary(headers, cells, names.len(), options);
    let style = table_style(collection, options)?;
    Ok(render::table(headers, cells, output_target(options), style))
}

/// Each selected sample shown whole, as the workbench's sample screen shows it
/// : its values, then its tables unfolded, the note with `--note`.
pub(super) fn view(options: &Options, note: bool, table: Option<&str>) -> Outcome {
    let root = match options.positionals.first() {
        Some(first) => PathBuf::from(first),
        None => default_target(options)?,
    };
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    if let Some(table) = table {
        return view_table(options, table, &collection, &selected);
    }
    let target = output_target(options);
    let semantic_style = render_style(&collection, options)?;
    // `--table-style` and `[render] table` draw its tables, as a table's.
    let drawn = table_style(&collection, options)?;
    let vocabulary = selected.vocabulary();
    let samples = known_samples(&collection);
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
                drawn,
                Some(&samples),
            )
        );
    }
    if matches!(target, Target::Pipe) {
        not_current_shown(
            selected
                .iter()
                .map(|entry| {
                    samplekit::collection::editing::not_current(&entry.sample.borrow())
                        .iter()
                        .map(|value| value.rows.max(1))
                        .sum::<usize>()
                })
                .sum(),
        );
    }
    Ok(())
}

/// `view TABLE`: one table of each sample selected that holds it, unfolded,
/// or written as a data file or a document, a row of the table a row of the
/// file.
fn view_table(
    options: &Options,
    table: &str,
    collection: &SampleList,
    selected: &SampleList,
) -> Outcome {
    let name = Identifier::new(table).ok();
    let holding = |entry: &samplekit::collection::sample_list::Entry| {
        name.as_ref()
            .is_some_and(|name| entry.sample.borrow().table(name).is_ok())
    };
    let held = selected.filter_entries(holding);
    let Some(name) = name.filter(|_| !held.is_empty()) else {
        let mut tables: Vec<String> = selected
            .iter()
            .flat_map(|entry| {
                entry
                    .sample
                    .borrow()
                    .table_names()
                    .into_iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        tables.sort();
        tables.dedup();
        return Err(Fail::usage(format!(
            "'{table}' is no file here, and no table of the samples selected{}",
            if tables.is_empty() {
                " — they hold none".to_string()
            } else {
                format!("\n  tables: {}", tables.join(", "))
            }
        )));
    };
    let format = resolved_format(options, collection.config())?;
    let Some(format) = format else {
        if options.output.is_some() {
            return Err(Fail::usage(
                "-o writes the table as data or as a document: .csv, .tsv, .json, .tex, .md \
                 or .html"
                    .to_string(),
            ));
        }
        let semantic_style = render_style(collection, options)?;
        let drawn = table_style(collection, options)?;
        let target = output_target(options);
        let vocabulary = held.vocabulary();
        for (at, entry) in held.iter().enumerate() {
            let sample = entry.sample.borrow();
            let subject = Subject {
                sample: &sample,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            };
            let config = entry
                .path
                .as_deref()
                .map_or(collection.config(), |path| collection.config_for(path));
            if at > 0 {
                outln!();
            }
            outln!("{}", render::name_of(&subject));
            out!(
                "{}",
                render::unfolded_table(
                    &subject,
                    &name,
                    config,
                    semantic_style,
                    options.precision.as_ref(),
                    target,
                    drawn,
                )
                .unwrap_or_default()
            );
        }
        return Ok(());
    };
    let header_config = shared_config(collection, &held);
    if let Some(config) = collection.config() {
        super::edit::describe_now(config);
    }
    let (dataset, profile) = exports::table_rows(
        &name,
        &held,
        header_config,
        &format,
        options.precision.as_ref(),
        options.style.clone(),
    )
    .map_err(export_failure)?;
    let dataset = noted_empty_in(&dataset, &format, "in every row");
    let text = export_formats::serialize(&dataset, format.clone())
        .map_err(|error| Fail::data(error.to_string()))?;
    let behind = behind_said(options, &held, &profile, collection.config());
    emit_output_behind(&text, options, &held, behind)
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
    let declared = config
        .export(&name)
        .map_err(|error| Fail::usage(error.to_string()))?;
    if let Export::Figure(figure) = declared {
        return export_figure(figure, config, options);
    }
    let selected = select(&collection, &unordered(options), &root)?;
    export_one(&name, config, selected, &collection, &collection, options)
}

/// An export of a figure: `plot` to the export's file, its query narrowing
/// the samples and its style labelling the axes — the preview, the folder
/// made, `{name}`, the refusals and the record all `plot`'s.
fn export_figure(figure: &FigureExport, config: &ProjectConfig, options: &Options) -> Outcome {
    let table_options: Vec<&str> = [
        ("--precision", options.precision.is_some()),
        ("--status", options.status),
    ]
    .into_iter()
    .filter_map(|(option, given)| given.then_some(option))
    .collect();
    if !table_options.is_empty() {
        return Err(Fail::usage(format!(
            "'{}' exports the figure '{}': {} {} a table's",
            figure.name,
            figure.figure,
            table_options.join(" and "),
            if table_options.len() == 1 {
                "is"
            } else {
                "are"
            }
        )));
    }
    if options.style.is_some() {
        return Err(Fail::usage(format!(
            "'{}' exports the figure '{}': --style spells a table's units — the style its \
             labels use is the export's style = …, or plot {} --style",
            figure.name, figure.figure, figure.figure
        )));
    }
    let mut options = options.clone();
    // Its targets, the export's name taken off: `plot` reads the first, and
    // the others beside it.
    let mut targets: Vec<PathBuf> = options
        .positionals
        .drain(..)
        .skip(1)
        .map(PathBuf::from)
        .collect();
    if !targets.is_empty() {
        options
            .positionals
            .push(targets.remove(0).to_string_lossy().into_owned());
    }
    EXTRA_TARGETS.with(|extra| *extra.borrow_mut() = targets);
    if options.output.is_none() {
        let declared = config.resolve(&figure.output.to_string_lossy());
        options.output = Some(PathBuf::from(relative_to_here(&declared)));
    }
    options.query.extend(figure.query.clone());
    plot(
        options.clone(),
        &PlotFlags {
            figure: Some(figure.figure.clone()),
            export: Some(figure.name.clone()),
            axes: None,
            no_project_style: false,
            overrides: samplekit::presentation::plotting::FigureOverrides {
                style: figure.style.clone(),
                ..Default::default()
            },
            accept: options.accept.clone(),
        },
    )
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
    // A figure is drawn from one project.
    if let Some(project) = parts.iter().find(|part| {
        part.config
            .is_some_and(|config| matches!(config.export(&name), Ok(Export::Figure(_))))
    }) {
        return Err(Fail::usage(format!(
            "'{name}' exports a figure, and a figure is drawn from one project: the \
             selection spans several\n  samplekit {} export {name}",
            project
                .config
                .map(|config| shell_word(&relative_to_here(config.root())))
                .unwrap_or_default()
        )));
    }
    let whole = collection.by_configuration(&collection.filter_by(|_| true));
    let declares = |config: Option<&ProjectConfig>| {
        config.is_some_and(|config| config.table_export(&name).is_ok())
    };
    let declaring = parts.iter().filter(|part| declares(part.config)).count();
    if declaring == 0 {
        let message = parts
            .iter()
            .find_map(|part| {
                part.config
                    .and_then(|config| config.table_export(&name).err())
            })
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
        let Ok(target) = config.table_export(&name) else {
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
                 first\n  name the project in its output — output = \"…/{{project}}/…\" — \
                 or export each on its own, to a file of its own with -o",
                relative_to_here(&place),
                configuration_title(other.as_deref()),
                configuration_title(part.file.as_deref())
            )));
        }
        destinations.push((place, part.file.clone()));
    }
    // Every project's columns checked before any file is written: one that
    // failed stopped the rest, after the first was written.
    for part in &parts {
        let Some(config) = part
            .config
            .filter(|config| config.table_export(&name).is_ok())
        else {
            continue;
        };
        let described = whole
            .iter()
            .find(|other| other.file == part.file)
            .map_or(collection, |other| &other.samples);
        export_ready(&name, config, described).map_err(|mut fail| {
            fail.message.push_str(&format!(
                "\n  nothing was written: '{name}' is checked in every project before any \
                 file is"
            ));
            fail
        })?;
    }
    // Configurations declaring no such export: one line, each under -v.
    let mut left_out: Vec<String> = Vec::new();
    let mut samples_left_out = 0;
    for part in parts {
        match part.config {
            Some(config) if config.table_export(&name).is_ok() => {
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

/// What makes an export fail before its file is made: its profile, and the
/// columns it shows among those its samples hold.
fn export_ready(name: &str, config: &ProjectConfig, described: &SampleList) -> Outcome {
    let target = config
        .table_export(name)
        .map_err(|error| Fail::usage(error.to_string()))?;
    let profile = config
        .profile(&target.profile)
        .map_err(|error| Fail::usage(error.to_string()))?;
    check_columns(described, &profile.carrying(&profile.group))
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
    let mut target = config
        .table_export(name)
        .cloned()
        .map_err(|error| Fail::usage(error.to_string()))?;
    // `--style` spells the units for this invocation, a spreadsheet's headers
    // in Unicode with `--style plain`; one naming nothing is refused.
    if let Some(style) = &options.style {
        let known = config.render_style_names();
        if style != samplekit::config::project_config::FILES_STYLE
            && !known.contains(&style.as_str())
        {
            return Err(Fail::usage(format!(
                "'{style}' is not a style this project declares\n  available: {}, {}",
                samplekit::config::project_config::FILES_STYLE,
                known.join(", ")
            )));
        }
        target.style = options.style.clone();
    }
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
    // same, and exits 2 unless --accept outdated says so: a script going on
    // with `set -e` shipped the file. None of it was built, so a
    // deliverable was regenerated from a stale number with nothing said — the
    // worst place for silence, because the file leaves the building.
    // What the model owes is planned from its description, had written where
    // it is missing, as `status` has it.
    super::edit::describe_now(config);
    let behind = exports::behind(&selected, &profile, Some(config));
    let exports::Behind {
        stale,
        held,
        never,
        samples: behind_samples,
        ref named,
        ..
    } = behind;
    // Said in the preview when previewing, and warned when writing outright:
    // the fact has to reach whoever is deciding, and only one of the two is
    // reading at that moment. As a warning alone it was on stderr, suppressed
    // by -q, and left the exit code at 0 — three ways to miss the one thing
    // that matters about the file.
    let not_current = behind.any().then(|| {
        let said = behind.kinds();
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
             the file is written as they stand — samplekit {} compute {}",
            said.join(", "),
            counted_as(behind_samples, "sample", "samples"),
            // Where this project's samples are: over two projects, the first
            // target named was offered for the other's samples.
            selected
                .iter()
                .find_map(|entry| entry.path.as_deref().and_then(Path::parent))
                // `samples`, not `./samples`, as a listing writes it.
                .map(|folder| match folder.strip_prefix(".") {
                    Ok(below) if !below.as_os_str().is_empty() => below,
                    _ => folder,
                })
                .filter(|folder| !folder.as_os_str().is_empty())
                .map(|folder| folder.display().to_string())
                .or_else(|| options.positionals.get(1).cloned())
                .unwrap_or_else(|| ".".to_string()),
            // An override is what compute keeps: --force is its way back.
            match (stale > 0 || never > 0, held > 0) {
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
    let dataset = with_states_if_asked(dataset, options, &target.format)?;
    let dataset = noted_empty(&dataset, &target.format);
    let text = export_formats::serialize(&dataset, target.format.clone())
        .map_err(|error| Fail::data(error.to_string()))?;

    let destination = options
        .output
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| target.output.to_string_lossy().into_owned());
    // A file named for another format would hold this one under its name:
    // `-o r.json` wrote CSV into it. Refused, as a figure's extension is.
    // The declared `output` is held to it too: it wrote CSV into `x.json`.
    if let Some(written) = options
        .output
        .as_ref()
        .or(Some(&target.output))
        .filter(|path| *path != Path::new("-"))
        && let Some(named) = written
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(|extension| match extension.to_ascii_lowercase().as_str() {
                "csv" => Some(Format::Csv),
                "tsv" => Some(Format::Tsv),
                "json" => Some(Format::Json),
                // A text format's own extension names it.
                other if matches!(&target.format, Format::Text(text) if text.extension == other) => {
                    Some(target.format.clone())
                }
                _ => None,
            })
        && named != target.format
    {
        let spelled = |format: &Format| match format {
            Format::Csv => "csv".to_string(),
            Format::Tsv => "tsv".to_string(),
            Format::Json => "json".to_string(),
            Format::Text(text) => text.extension.clone(),
        };
        return Err(Fail::usage(format!(
            "{}{}: '{name}' writes {}, and the extension names {} — name the file \
             .{}, or change format in [export.{name}]",
            if options.output.is_some() {
                "-o "
            } else {
                "output = "
            },
            written.display(),
            spelled(&target.format).to_ascii_uppercase(),
            spelled(&named).to_ascii_uppercase(),
            spelled(&target.format)
        )));
    }
    if destination == "-" {
        out!("{text}");
        // The file is the output: what is not current in it is said on
        // stderr, and fails the command as a file written would.
        if let Some(said) = &not_current {
            for line in said.lines() {
                errln!("{}", line.trim_start());
            }
        }
        return accepted_or_failed("the export", &behind, &options.accept);
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
        // What and where, as every preview's first line says it.
        outln!(
            "export {name} · {} · {}\n",
            relative_to_here(&path),
            counted_as(dataset.rows.len(), "row", "rows")
        );
        if let Some(refused) = exports::refusal(&path) {
            outln!("  --write refuses it: {refused}");
        } else if unread_kept() {
            outln!(
                "  --write writes nothing: a sample could not be read, and its rows would be \
                 missing"
            );
        } else if path.exists() {
            outln!("  the file is there, and is replaced");
        } else if let Some(directory) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
        {
            // A destination's directory, declared or given with -o, is made by
            // --write: said here, before it happens.
            outln!(
                "  {} does not exist, and is made",
                relative_to_here(directory)
            );
        }
        // What is not current, a consequence, its samples beneath it.
        if let Some(said) = &not_current {
            outln!();
            for line in said.lines() {
                outln!("  {line}");
            }
        }
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    // A sample that could not be read has no row: the export would be written
    // without it — a new file as incomplete as a replaced one, and a job
    // regenerating it every night would lose it for good. None is written.
    if unread_kept() {
        return Err(Fail::data(format!(
            "{} not {}: a sample of the selection could not be read, and its rows would be \
             missing — samplekit validate names it",
            path.display(),
            if path.exists() { "replaced" } else { "written" }
        )));
    }
    // Written outright, it says what and where first, as `set --write` shows
    // its change: a yes at the question has shown it already.
    if !APPLYING.load(AtomicOrdering::Relaxed) {
        outln!(
            "export {name} · {} · {}",
            relative_to_here(&path),
            counted_as(dataset.rows.len(), "row", "rows")
        );
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
    outln!(
        "written: {}, {}",
        relative_to_here(&path),
        counted_as(dataset.rows.len(), "row", "rows")
    );
    // Written, and a failure all the same: what the file holds is not
    // current, and a script stops here unless it says it accepts that.
    // Each kind accepted by its own word: values outdated or edited by
    // `outdated`, values written empty by `never-computed`.
    accepted_or_failed(&relative_to_here(&path), &behind, &options.accept)
}

/// A file written from values not current fails the command, exit 2, unless
/// `--accept` names each state it holds: the same rule for an export,
/// a table written with `-o` and a figure.
pub(super) fn accepted_or_failed(
    written: &str,
    behind: &exports::Behind,
    accept: &super::Accepted,
) -> Result<(), Fail> {
    let edited_passes = accept.edited
        || (!behind.edited_values.is_empty()
            && behind
                .edited_values
                .iter()
                .all(|value| accept.edited_names.contains(value)));
    let words: Vec<&str> = [
        (behind.stale > 0 && !accept.outdated).then_some("outdated"),
        (behind.held > 0 && !edited_passes).then_some("edited"),
        (behind.failed > 0 && !accept.failed).then_some("failed"),
        (behind.never > 0 && !accept.never_computed).then_some("never_computed"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if words.is_empty() {
        return Ok(());
    }
    // Named, the first few, for a figure, which says them nowhere else: a
    // table and an export listed them just above.
    const SHOWN: usize = 5;
    let figure = written == "the figure";
    let named = match behind.named.len() {
        _ if !figure => String::new(),
        0 => String::new(),
        count if count > SHOWN => format!(
            ": {}, and {} more",
            behind.named[..SHOWN].join(", "),
            count - SHOWN
        ),
        _ => format!(": {}", behind.named.join(", ")),
    };
    let verb = if written == "the figure" {
        "drawn"
    } else {
        "written"
    };
    Err(Fail::data(format!(
        "{written} is {verb} from values not current{named} — --accept {} exits 0 for it",
        words.join(",")
    )))
}

/// What a table written to a file holds that is not current, said as an
/// export says it: the file does not show the states the terminal did.
fn behind_said(
    options: &Options,
    selected: &SampleList,
    profile: &samplekit::config::project_config::Profile,
    config: Option<&samplekit::config::project_config::ProjectConfig>,
) -> Option<(String, exports::Behind)> {
    options.output.as_ref()?;
    if let Some(config) = config {
        super::edit::describe_now(config);
    }
    let behind = exports::behind(selected, profile, config);
    if !behind.any() {
        return None;
    }
    let said = behind.kinds();
    let exports::Behind { samples, named, .. } = behind.clone();
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
    let text = format!(
        "the table is written from values that are not current: {} in {}\n  {listed}\n  \
         {} says why, and compute --write brings them up to date",
        said.join(", "),
        counted_as(samples, "sample", "samples"),
        super::advised("status"),
    );
    Some((text, behind))
}

/// Two columns asked for under one header are a question to rephrase; the
/// rest is the data's.
fn export_failure(error: exports::ExportError) -> Fail {
    match error {
        exports::ExportError::DuplicateHeader { .. } => Fail::usage(error.to_string()),
        exports::ExportError::MixedUnits { .. } => Fail::data(error.to_string()),
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

/// The format asked for, a style's document read from the project's
/// declarations: `--style NAME` names one before any configuration is read,
/// and SampleKit's own stands until a declaration of that name changes it.
fn resolved_format(
    options: &Options,
    config: Option<&ProjectConfig>,
) -> Result<Option<Format>, Fail> {
    let Some(Format::Text(asked)) = &options.format else {
        return Ok(options.format.clone());
    };
    if let Some(declared) = config.and_then(|config| config.text_format(&asked.name)) {
        return Ok(Some(Format::Text(Box::new(declared.clone()))));
    }
    if let Some(given) = samplekit::config::project_config::TextFormat::given(&asked.name) {
        return Ok(Some(Format::Text(Box::new(given))));
    }
    // On the terminal, a style saying units alone is the terminal's table.
    if options.document_if_declared {
        return Ok(None);
    }
    let known: Vec<String> = config.map_or_else(
        || {
            samplekit::config::project_config::TextFormat::GIVEN
                .iter()
                .map(|name| name.to_string())
                .collect()
        },
        |config| {
            config
                .text_format_names()
                .iter()
                .map(|name| name.to_string())
                .collect()
        },
    );
    Err(Fail::usage(format!(
        "--style {}: no style writing documents — {} do, and [style.NAME] declares another",
        asked.name,
        known.join(", ")
    )))
}

/// `--save KIND=NAME`: what the table shows, declared in the project's
/// `.samplekitrc` — previewed as the lines it adds, written with `--write`.
pub(super) fn save_view(options: &Options, save: &SaveAs, has_shape: bool) -> Outcome {
    let root = match options.positionals.first() {
        Some(first) => PathBuf::from(first),
        None => default_target(options)?,
    };
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let kind = save.kind.section();
    if collection.spans_configurations() {
        return Err(Fail::usage(
            "--save writes one project's .samplekitrc, and these samples are of several: \
             name one project's folder"
                .to_string(),
        ));
    }
    let Some(config) = collection.config() else {
        return Err(Fail::usage(needs_configuration(
            &collection,
            &format!("the {kind} {}", save.name),
        )));
    };
    if save.kind != Named::Query && !has_shape {
        return Err(Fail::usage(format!(
            "a {kind} saves the columns shown: name them with --columns or --profile"
        )));
    }
    let options = &with_profile_groups(&collection, options)?;
    // The filter as the table selects: every --filter and every --query.
    let mut filters: Vec<String> = options.filter.clone();
    for name in &options.query {
        let query = config
            .query(name)
            .map_err(|error| Fail::usage(error.to_string()))?;
        filters.push(query.filter.clone());
    }
    let filter = match filters.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        several => several
            .iter()
            .map(|filter| format!("({filter})"))
            .collect::<Vec<_>>()
            .join(" && "),
    };
    let profile = if has_shape {
        let mut profile = profile_of(&collection, options, options.profile.as_deref())?;
        let siblings = siblings_of(&root);
        let vocabulary = siblings.as_ref().unwrap_or(&collection);
        correct_column_typos(vocabulary, &mut profile)?;
        check_columns(vocabulary, &profile)?;
        Some(profile)
    } else {
        None
    };
    // As the table sorts: -s, or the profile's, -r turning every key.
    let keys: Vec<String> = if !options.sort.is_empty() {
        options.sort.clone()
    } else {
        stored_profile(&collection, options)
            .map(|profile| profile.sort.clone())
            .unwrap_or_default()
    };
    let sort: Vec<String> = keys
        .iter()
        .map(|key| match (options.reverse, key.strip_prefix('-')) {
            (false, _) => key.clone(),
            (true, Some(field)) => field.to_string(),
            (true, None) => format!("-{key}"),
        })
        .collect();
    let group: Vec<String> = options
        .group
        .iter()
        .flat_map(|fields| fields.split(','))
        .map(|field| field.trim().to_string())
        .filter(|field| !field.is_empty())
        .collect();
    let format = match &options.format {
        Some(Format::Tsv) => "tsv",
        Some(Format::Json) => "json",
        Some(Format::Text(_)) if save.kind == Named::Export => {
            return Err(Fail::usage(
                "--save export= saves a csv, tsv or json export: --style writes a document, \
                 which an export declares by hand in .samplekitrc"
                    .to_string(),
            ));
        }
        _ => "csv",
    };
    let saved = match save.kind {
        Named::Query => configuration_edit::Saved::Query,
        Named::Profile => configuration_edit::Saved::Profile,
        _ => configuration_edit::Saved::Export(format),
    };
    let path = config.root().join(".samplekitrc");
    let mut edit =
        ConfigurationEdit::open(&path).map_err(|error| Fail::usage(error.to_string()))?;
    let view = configuration_edit::ShownView {
        filter: &filter,
        columns: profile.as_ref().map_or(&[][..], |profile| &profile.columns),
        sort: &sort,
        group: &group,
    };
    edit.declare_view(saved, &save.name, &view)
        .map_err(Fail::usage)?;
    edit.checked()
        .map_err(|error| Fail::usage(error.to_string()))?;
    let name = &save.name;
    if !options.write {
        outln!("save in {}", relative_to_here(&path));
        for line in edit.changes() {
            outln!("  {}", line.trim_end());
        }
        outln!();
        offer_to_apply("apply these changes?", "");
        return Ok(());
    }
    edit.write()
        .map_err(|error| Fail::usage(error.to_string()))?;
    outln!("written: {}", relative_to_here(&path));
    outln!(
        "{}",
        match save.kind {
            Named::Query => format!("query {name} declared — --query {name} selects with it"),
            Named::Profile => format!("profile {name} declared — --profile {name} shows it"),
            _ => format!("export {name} declared — samplekit export {name} writes it"),
        }
    );
    Ok(())
}

/// The samples a wikilink may name, each its path and its `name:`.
fn known_samples(
    collection: &samplekit::collection::sample_list::SampleList,
) -> Vec<(std::path::PathBuf, Option<String>)> {
    collection
        .iter()
        .filter_map(|entry| {
            Some((
                entry.path.clone()?,
                entry.sample.try_borrow().ok()?.name().map(str::to_string),
            ))
        })
        .collect()
}
