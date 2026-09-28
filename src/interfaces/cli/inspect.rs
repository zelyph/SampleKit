//! `status`, `explain`, `validate` and `list`: what a collection holds and how it stands.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::collection::explanation;

pub(super) fn unknown_kind(written: &str) -> Fail {
    let mut message = format!("'{written}' is not a kind to list");
    if let Some(suggestion) = identifier::nearest(written, KINDS.iter().copied()) {
        message.push_str(&format!("\n  did you mean: '{suggestion}'?"));
    }
    message.push_str(&format!("\n  available: {}", KINDS.join(", ")));
    Fail::usage(message)
}

pub(super) fn push_once(names: &mut Vec<String>, name: String) {
    if !names.contains(&name) {
        names.push(name);
    }
}

pub(super) fn field_groups(collection: &SampleList) -> FieldGroups {
    let mut groups = FieldGroups::default();
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        for name in sample.property_names() {
            push_once(&mut groups.properties, name.to_string());
        }
        for name in sample.attribute_names() {
            push_once(&mut groups.attributes, name.to_string());
        }
        for table_name in sample.table_names() {
            let Ok(table) = sample.table(table_name) else {
                continue;
            };
            let at = match groups
                .tables
                .iter()
                .position(|group| group.name == table_name.as_str())
            {
                Some(at) => at,
                None => {
                    groups.tables.push(TableGroup {
                        name: table_name.to_string(),
                        index: Vec::new(),
                        columns: Vec::new(),
                        rows: Vec::new(),
                    });
                    groups.tables.len() - 1
                }
            };
            let group = &mut groups.tables[at];
            let index: Vec<String> = table
                .index_columns()
                .iter()
                .map(|c| c.to_string())
                .collect();
            for column in &index {
                push_once(&mut group.index, column.clone());
            }
            let names = table.column_names();
            for column in &names {
                if !index.iter().any(|known| known == column.as_str()) {
                    push_once(&mut group.columns, column.to_string());
                }
            }
            let Some(first) = names.first() else {
                continue;
            };
            for tuple in table.index_tuples() {
                // The index as a path writes it, so what is printed can be typed.
                let described = fields::describe(&Field::Cell {
                    table: table_name.clone(),
                    column: (*first).clone(),
                    row: RowAddress::index(tuple.into_iter().cloned().collect::<Vec<_>>()),
                    channel: Channel::Value,
                });
                let prefix = format!("{table_name}.{first}[");
                let inner = described
                    .strip_prefix(&prefix)
                    .and_then(|rest| rest.strip_suffix(']'))
                    .unwrap_or(&described);
                let shown = if inner.contains(',') {
                    format!("[{inner}]")
                } else {
                    inner.to_string()
                };
                push_once(&mut group.rows, shown);
            }
        }
    }
    groups
}

/// Words after a prefix, wrapped under it rather than under the margin, two
/// spaces between them. By `textwrap`: a word's own spaces are held unbreakable
/// while it wraps, so that a quoted address stays whole.
pub(super) fn wrapped(prefix: &str, words: &[String], width: usize) -> String {
    const HELD: char = '\u{a0}';
    let indent = " ".repeat(render::width(prefix));
    let text = words
        .iter()
        .map(|word| word.replace(' ', &HELD.to_string()))
        .collect::<Vec<_>>()
        .join("  ");
    // No hyphenation: a name such as `farmhouse-saison` is one word, and
    // textwrap otherwise broke it at its hyphen.
    let options = textwrap::Options::new(width)
        .initial_indent(prefix)
        .subsequent_indent(&indent)
        .break_words(false)
        .word_splitter(textwrap::WordSplitter::NoHyphenation)
        .wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    let mut out = String::new();
    for line in textwrap::wrap(&text, options) {
        out.push_str(line.trim_end().replace(HELD, " ").as_str());
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str(prefix.trim_end());
        out.push('\n');
    }
    out
}

/// **A table is listed as a table, not one line per cell**: its index values,
/// its other columns, and one address to copy — quoted, because `[` is a glob
/// character in zsh.
pub(super) fn field_listing(collection: &SampleList, options: &Options) -> String {
    let width = match output_target(options) {
        Target::Terminal { width } => width,
        Target::Pipe => 100,
    };
    let groups = field_groups(collection);
    let mut out = String::new();
    if !groups.properties.is_empty() {
        out.push_str(&format!("properties ({})\n", groups.properties.len()));
        out.push_str(&wrapped("  ", &groups.properties, width));
        out.push('\n');
    }
    if !groups.attributes.is_empty() {
        out.push_str(&format!("attributes ({})\n", groups.attributes.len()));
        out.push_str(&wrapped("  ", &groups.attributes, width));
        out.push('\n');
    }
    if !groups.tables.is_empty() {
        out.push_str(&format!(
            "tables ({}) \u{2014} a cell is <table>.<column>[<index>], quoted in a shell\n",
            groups.tables.len()
        ));
        for table in &groups.tables {
            out.push_str(&table_group_listing(table, width));
        }
        out.push('\n');
    }
    out.push_str("always\n");
    out.push_str(&wrapped(
        "  ",
        &["name", "path", "filename", "project", "state", "tags"].map(str::to_string),
        width,
    ));
    out
}

pub(super) fn table_group_listing(table: &TableGroup, width: usize) -> String {
    let mut out = format!("  {}\n", table.name);
    out.push_str(&wrapped(
        &format!("    index    {}: ", table.index.join(", ")),
        &table.rows,
        width,
    ));
    out.push_str(&wrapped("    columns  ", &table.columns, width));
    if let (Some(column), Some(row)) = (table.columns.first(), table.rows.first()) {
        let inner = row.trim_start_matches('[').trim_end_matches(']');
        out.push_str(&format!(
            "    e.g.     '{}.{column}[{inner}]'\n",
            table.name
        ));
    }
    out
}

/// *Show me what is here.* Alone it counts; with a kind it enumerates.
pub(super) fn inventory(collection: &SampleList, kind: Option<&str>, options: &Options) -> Outcome {
    let Some(kind) = kind else {
        outln!("{}", counted_as(collection.len(), "sample", "samples"));
        let groups = field_groups(collection);
        // Each kind counted every time, none of them too: an empty folder left
        // the attributes out, and the line then named two kinds of three.
        let counted = [
            counted_as(groups.properties.len(), "property", "properties"),
            counted_as(groups.attributes.len(), "attribute", "attributes"),
            counted_as(groups.tables.len(), "table", "tables"),
        ];
        outln!("{} — samplekit list fields", counted.join(", "));
        let tags = sorted_tags(collection);
        if tags.is_empty() {
            outln!("no tags in use");
        } else if tags.len() <= 8 {
            let written: Vec<String> = tags.iter().map(Identifier::to_string).collect();
            outln!(
                "{} in use — {}",
                counted_as(tags.len(), "tag", "tags"),
                written.join(", ")
            );
        } else {
            outln!(
                "{} in use — samplekit list tags",
                counted_as(tags.len(), "tag", "tags")
            );
        }
        let declaring = offered(collection);
        if declaring.is_empty() {
            outln!("no .samplekitrc above this directory");
        }
        for (file, config) in &declaring {
            outln!(
                "{}, {}, {}, {} — declared in {}",
                counted_as(config.query_names().len(), "query", "queries"),
                counted_as(config.profile_names().len(), "profile", "profiles"),
                counted_as(config.export_names().len(), "export", "exports"),
                counted_as(config.figure_names().len(), "figure", "figures"),
                file.as_deref().map_or_else(
                    || ".samplekitrc".to_string(),
                    |file| file.display().to_string()
                )
            );
        }
        if !collection.skipped().is_empty() {
            // Two counts, told apart: a file that failed to read, and one that
            // was never a sample — a README — which the warning above does not
            // count. One number under the words *not read* disagreed with the
            // warning's, on the same run.
            let unread = collection
                .skipped()
                .iter()
                .filter(|skipped| is_unread(skipped))
                .count();
            let other = collection.skipped().len() - unread;
            let said = match (unread, other) {
                (0, other) => format!("{} skipped", counted_as(other, "file", "files")),
                (unread, 0) => format!("{} not read", counted_as(unread, "file", "files")),
                (unread, other) => format!(
                    "{} not read, {} skipped",
                    counted_as(unread, "file", "files"),
                    other
                ),
            };
            outln!("{said} — samplekit list skipped");
        }
        let stale = collection
            .iter()
            .filter(|entry| {
                let sample = entry.sample.borrow();
                fingerprint::check(&sample).is_ok_and(|states| {
                    states
                        .values()
                        .any(|state| !matches!(state, Freshness::Source | Freshness::Current))
                })
            })
            .count();
        if stale > 0 {
            outln!(
                "{} an outdated or edited value — samplekit status lists them, and values never computed",
                counted_as(stale, "sample holds", "samples hold")
            );
        }
        return Ok(());
    };

    match kind {
        "fields" => out!("{}", field_listing(collection, options)),
        "tags" => {
            let tags = sorted_tags(collection);
            // Said, as `skipped` says it: an empty output reads, to a script
            // and to a person, as a command that failed.
            if tags.is_empty() {
                outln!("no tags in use");
            }
            for tag in tags {
                outln!("{tag}");
            }
        }
        "skipped" => {
            // Said when there is nothing to list: an empty output read, to a
            // script, the same as a command that crashed.
            if collection.skipped().is_empty() {
                outln!("no file was skipped");
            }
            // In the words `validate` uses for the same file — *not read*, not
            // *malformed frontmatter* for one a newer SampleKit wrote — and in
            // two columns, the reasons lined up.
            let rows: Vec<(String, String)> = collection
                .skipped()
                .iter()
                .map(|skipped| {
                    let reason = match &skipped.reason {
                        samplekit::config::discovery::SkipReason::Malformed { message } => {
                            format!("not read: {message}")
                        }
                        reason if is_unread(skipped) => format!("not read: {reason}"),
                        reason => reason.to_string(),
                    };
                    (shown_path(&skipped.path), reason)
                })
                .collect();
            let widest = rows
                .iter()
                .map(|(path, _)| render::width(path))
                .max()
                .unwrap_or(0);
            for (path, reason) in &rows {
                let pad = " ".repeat(widest - render::width(path));
                outln!("{path}{pad}  {reason}");
            }
        }
        "files" => return list_files(collection),
        "figures" => {
            let root = options
                .positionals
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            return list_figures(collection, &root);
        }
        "queries" | "profiles" | "exports" => {
            let declaring = offered(collection);
            if declaring.is_empty() {
                return Err(Fail::usage(
                    "no .samplekitrc above this directory declares anything".to_string(),
                ));
            }
            fn names_of<'a>(config: &'a ProjectConfig, kind: &str) -> Vec<&'a str> {
                match kind {
                    "queries" => config.query_names(),
                    "profiles" => config.profile_names(),
                    _ => config.export_names(),
                }
            }
            let declaration = match kind {
                "queries" => DeclarationKind::Query,
                "profiles" => DeclarationKind::Profile,
                _ => DeclarationKind::Export,
            };
            if declaring
                .iter()
                .all(|(_, config)| names_of(config, kind).is_empty())
            {
                outln!(
                    "no {} declared",
                    match kind {
                        "queries" => "query",
                        "profiles" => "profile",
                        _ => "export",
                    }
                );
                return Ok(());
            }
            for (file, config) in &declaring {
                let names = names_of(config, kind);
                // Several configurations: each names its own, under its file.
                let indent = match file {
                    Some(file) => {
                        outln!("{}", file.display());
                        "  "
                    }
                    None => "",
                };
                let widest = names.iter().map(|name| render::width(name)).max();
                for name in names {
                    match origin_of(config, declaration, name) {
                        Some(origin) => {
                            let pad = " ".repeat(widest.unwrap_or(0) - render::width(name));
                            outln!("{indent}{name}{pad}  {origin}");
                        }
                        None => outln!("{indent}{name}"),
                    }
                }
            }
        }
        other => return Err(unknown_kind(other)),
    }
    Ok(())
}

/// The tags in use, in alphabetical order: gathered from several projects,
/// their order was the order the files happened to be met in.
fn sorted_tags(collection: &SampleList) -> Vec<Identifier> {
    let mut tags: Vec<Identifier> = collection.vocabulary().tags().to_vec();
    tags.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    tags
}

/// The configurations whose declarations a listing offers: the one of the
/// folder the command runs from, or of its target, with what it imports — never
/// every collection's below it; several targets, or a folder with none of its
/// own above several, each one's.
pub(super) fn offered(collection: &SampleList) -> Vec<(Option<PathBuf>, &ProjectConfig)> {
    let several_targets = EXTRA_TARGETS.with(|extra| !extra.borrow().is_empty());
    match collection.config() {
        Some(config) if !several_targets => vec![(None, config)],
        _ => declaring(collection),
    }
}

/// Where a declaration offered comes from, where its configuration imports
/// another: `local`, the imported file, or `local over` it. Nothing where it
/// imports none, every name being its own.
pub(super) fn origin_of(
    config: &ProjectConfig,
    kind: DeclarationKind,
    name: &str,
) -> Option<String> {
    if config.imports().is_empty() {
        return None;
    }
    Some(match config.origin(kind, name) {
        DeclaredIn::Local => "local".to_string(),
        DeclaredIn::Imported(file) => relative_to_here(&file),
        DeclaredIn::Over(file) => format!("local over {}", relative_to_here(&file)),
    })
}

/// The configurations whose declarations a listing names: the list's own, or
/// each describing some of its samples when it spans several.
pub(super) fn declaring(collection: &SampleList) -> Vec<(Option<PathBuf>, &ProjectConfig)> {
    if !collection.spans_configurations() {
        return collection
            .config()
            .map(|config| (None, config))
            .into_iter()
            .collect();
    }
    let everything = collection.filter_by(|_| true);
    collection
        .by_configuration(&everything)
        .into_iter()
        .filter_map(|part| Some((part.file, part.config?)))
        .collect()
}

/// Paths, one per line, and nothing else — which is what makes `xargs`
/// possible. It is a verb rather than a format because a list of paths is not
/// the table encoded differently: it takes no columns and honours no precision.
pub(super) fn find(options: &Options) -> Outcome {
    let (root, _) = target_and_extra(options)?;
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    let mut text = String::new();
    for entry in selected.iter() {
        if let Some(path) = &entry.path {
            text.push_str(&format!("{}\n", path.display()));
        }
    }
    emit_output(&text, options, &selected)
}

/// The failure log a sample's traceback was kept in, as a path to say, where
/// one is there: beside the project, named after the sample's file.
pub(super) fn failure_log_of(_collection: &SampleList, sample: &Path) -> Option<String> {
    // The sample's own project, which a selection over several configurations
    // does not have one of.
    let root = samplekit::config::project_config::find(sample)
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let relative = failure_log(&root, sample)?;
    root.join(&relative)
        .exists()
        .then(|| relative.display().to_string())
}

pub(super) fn failure_log(root: &Path, sample: &Path) -> Option<PathBuf> {
    explanation::failure_log(root, sample)
}

pub(super) fn stale(options: &Options, status: &StatusGate) -> Outcome {
    let gate = status.exit_code;
    let (root, _) = target_and_extra(options)?;
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    let counts = name_counts(
        selected
            .iter()
            .map(|entry| entry.sample.borrow().name().map(str::to_string)),
    );
    let mut lines = Vec::new();
    // Where the tracebacks went, for the samples that have one — and only where
    // one is there: a failure whose log was deleted, or written by an earlier
    // save that kept none, has no log, and pointing at one sent the reader to a
    // file that does not exist.
    let mut logs: Vec<String> = Vec::new();
    let mut unlogged = 0usize;
    // An override is what `--force` gives back; the ordinary advice alone sent
    // the reader to a `compute --write` that answered *nothing to compute*.
    let mut overrides = 0usize;
    for entry in selected.iter() {
        let sample = entry.sample.borrow();
        for (quantity, state, rows) in not_current(&sample) {
            if matches!(state, Freshness::Edited | Freshness::RecordMissing) {
                overrides += 1;
            }
            if matches!(state, Freshness::Failed { .. })
                && let Some(path) = entry.path.as_deref()
            {
                match failure_log_of(&collection, path) {
                    Some(log) => {
                        if !logs.contains(&log) {
                            logs.push(log);
                        }
                    }
                    None => unlogged += 1,
                }
            }
            lines.push(vec![
                labelled(&counts, entry.path.as_deref(), &sample),
                quantity,
                state_of(&sample, &state, rows),
            ]);
        }
    }
    let unread_reasons = never_computed(&selected, &mut lines)?;
    let unread: usize = unread_reasons.values().map(Vec::len).sum();
    lines.sort_by(|left, right| left[0].cmp(&right[0]).then(left[1].cmp(&right[1])));
    // Counted here: the rows themselves are handed to the renderer below.
    let not_current = lines.len();
    // What fails the gate: every state but those accepted, and never a value
    // waiting for an input nobody entered, which is not yet a failure of
    // anything.
    let failing = lines
        .iter()
        .filter(|line| {
            let kind = state_kind(&line[2]);
            kind != "waiting" && !status.accept.iter().any(|accepted| accepted == kind)
        })
        .count();
    // A model that could not be read heads the answer, with the samples it
    // could not read, whatever else is listed below it: under a table of stale
    // values it was a warning after the advice.
    let broken_labels: Vec<&String> = unread_reasons.values().flatten().collect();
    let unreadable = if broken_labels.is_empty() {
        None
    } else {
        let named: Vec<&str> = broken_labels
            .iter()
            .take(5)
            .map(|label| label.as_str())
            .collect();
        Some(format!(
            "the model could not be read for {}{}: what it has never computed there is not known",
            named.join(", "),
            if broken_labels.len() > named.len() {
                format!(" and {} more", broken_labels.len() - named.len())
            } else {
                String::new()
            }
        ))
    };
    if status.json {
        let objects: Vec<serde_json::Value> = lines
            .iter()
            .map(|line| {
                serde_json::json!({
                    "sample": line[0],
                    "value": line[1],
                    "state": state_kind(&line[2]),
                    "said": line[2],
                })
            })
            .collect();
        outln!(
            "{}",
            serde_json::to_string_pretty(&objects).unwrap_or_else(|_| "[]".to_string())
        );
    } else if lines.is_empty() {
        // Silence here would be indistinguishable from a failure.
        let counted = counted_as(selected.len(), "sample", "samples");
        if let Some(unreadable) = &unreadable {
            outln!("{unreadable}, and every value the files record in {counted} is current");
        } else if unread > 0 {
            outln!("every value the files record in {counted} is current");
        } else {
            outln!("every derived value in {counted} is current");
        }
    } else {
        if let Some(unreadable) = &unreadable {
            outln!("{unreadable}");
        }
        outln!(
            "{} not current\n",
            counted_as(lines.len(), "value is", "values are")
        );
        let style = table_style(&collection, options)?;
        // The sample, the value and its state are the answer: they wrap
        // rather than hide.
        out!(
            "{}",
            render::table_keeping(
                ["sample", "value", "state"]
                    .iter()
                    .map(|n| n.to_string())
                    .collect(),
                lines,
                output_target(options),
                style,
                3
            )
        );
        // A failed value keeps the exception's type; the traceback is beside the
        // project, and unreadable unless it is named here.
        match logs.as_slice() {
            [] => {}
            [only] => outln!("\nwhat failed left its traceback in {only}"),
            many => outln!(
                "\nwhat failed left its traceback in .samplekit/failures/, one log per sample ({})",
                counted_as(many.len(), "sample", "samples")
            ),
        }
        if unlogged > 0 {
            outln!(
                "{}{} kept no traceback beside the project — samplekit compute --rerun \
                 runs it again and keeps one",
                if logs.is_empty() { "\n" } else { "" },
                counted_as(unlogged, "failure", "failures")
            );
        }
        // What acts on the answer, as a preview names what applies it.
        if overrides < not_current {
            outln!(
                "\n{}",
                advice("samplekit compute lists what would run; --write computes and writes")
            );
        }
        if overrides > 0 {
            outln!(
                "{}{}",
                if overrides < not_current { "" } else { "\n" },
                advice(&overrides_kept(overrides))
            );
        }
    }
    // Why the model was not read, so that the answer can be acted on. Several
    // reasons are one line, each under -v.
    let each: Vec<String> = unread_reasons
        .iter()
        .map(|(reason, labels)| {
            format!(
                "{} a model that was not read, so values never computed are not listed: {reason}",
                counted_as(labels.len(), "sample has", "samples have")
            )
        })
        .collect();
    let unread: usize = unread_reasons.values().map(Vec::len).sum();
    warn_several(
        &format!(
            "{} a model that was not read, for {} reasons, so values never computed are not \
             listed",
            counted_as(unread, "sample has", "samples have"),
            each.len()
        ),
        &each,
    );
    say_model_changed(&selected);
    // *This is what is stale* is an answer, not a failure: being outdated does
    // not make a collection invalid. A check that must
    // fail says so explicitly, with `--exit-code`.
    if gate && failing > 0 {
        return Err(Fail::data(format!(
            "{} not current",
            counted_as(failing, "value is", "values are")
        )));
    }
    // A model that should have been read and could not be leaves the answer
    // incomplete: a gate passing on it passed a broken model.
    let broken: usize = unread_reasons.values().map(Vec::len).sum();
    if gate && broken > 0 {
        return Err(Fail::data(format!(
            "{} could not be checked: the model was not read",
            counted_as(broken, "sample", "samples")
        )));
    }
    Ok(())
}

/// Samples whose values a version of the model computed that is no longer the
/// model, where which formula changed is not known — a record from before
/// formulas were told apart: said, with the flag that computes them again. A
/// formula told apart is judged by the plan, value by value, and a model
/// changed elsewhere — a figure, a comment — says nothing.
pub(super) fn say_model_changed(selected: &SampleList) {
    let Some(store) = runtime::ComputedStore::user() else {
        return;
    };
    let Ok(groups) = groups_of(selected) else {
        return;
    };
    let mut changed = 0usize;
    for group in groups {
        let Some(template) = group.config.as_ref().and_then(runtime::template_of) else {
            continue;
        };
        let Ok(digest) = runtime::digest_of(&template) else {
            continue;
        };
        for position in &group.entries {
            if let Some(path) = selected
                .get(*position)
                .and_then(|entry| entry.path.as_deref())
                && store.formulas_unknown(path, &digest)
            {
                changed += 1;
            }
        }
    }
    if changed > 0 {
        warn(&format!(
            "the model changed since the values of {} were computed — --rerun computes them again",
            counted_as(changed, "sample", "samples")
        ));
    }
}

/// Values never computed, which only a model can name: planned from the model's
/// description and each file, nothing asked, no formula run, and no Python
/// started while the description is current — one worker writing it again where
/// it is not. Returns the samples whose model was not read, by why.
pub(super) fn never_computed(
    selected: &SampleList,
    lines: &mut Vec<Vec<String>>,
) -> Result<std::collections::BTreeMap<String, Vec<String>>, Fail> {
    let counts = name_counts(
        selected
            .iter()
            .map(|entry| entry.sample.borrow().name().map(str::to_string)),
    );
    // What each formula was when a sample was last computed.
    let computed = runtime::ComputedStore::user();
    let mut unread: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for group in groups_of(selected)? {
        let entries: Vec<&list::Entry> = group
            .entries
            .iter()
            .filter_map(|position| selected.get(*position))
            .collect();
        let Some(config) = group.config.as_ref() else {
            continue;
        };
        if runtime::template_of(config).is_none() {
            continue;
        }
        let from = entries
            .first()
            .and_then(|entry| entry.path.as_deref())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let labels: Vec<String> = entries
            .iter()
            .map(|entry| labelled(&counts, entry.path.as_deref(), &entry.sample.borrow()))
            .collect();
        let description = match runtime::describe(config, &from) {
            Ok(description) => description,
            Err(error) => {
                let reason = match error {
                    ModelError::TemplateNotFound { path } => {
                        format!("the model {} cannot be found", path.display())
                    }
                    ModelError::NoInterpreter { reason } => reason,
                    error => error.to_string(),
                };
                unread.entry(reason).or_default().extend(labels.clone());
                continue;
            }
        };
        for entry in &entries {
            let Some(path) = entry.path.clone() else {
                continue;
            };
            let label = labelled(&counts, Some(&path), &entry.sample.borrow());
            let recorded = computed
                .as_ref()
                .map(|store| store.recorded_formulas(&path))
                .unwrap_or_default();
            // Planned forced, as the worker plans it for `status`, so that a
            // value held for want of its record is planned, and can be told
            // from an override.
            match description.plan(&path, &recorded) {
                Ok(plan) => {
                    let missing = |value: &str| {
                        Identifier::new(value).ok().is_some_and(|name| {
                            entry.sample.borrow().property(&name).is_ok_and(|handle| {
                                let records = handle.records();
                                records.computed.is_none()
                                    && records.failure.is_none()
                                    && !records
                                        .fingerprint
                                        .as_ref()
                                        .is_some_and(|digest| digest.is_edited())
                                    && handle
                                        .peek(samplekit::core::property::Property::peek_value)
                                        .is_some_and(|value| !value.is_absent())
                            })
                        })
                    };
                    lines.extend(
                        plan.iter()
                            .filter(|planned| {
                                planned.reason == Reason::Edited && missing(&planned.value)
                            })
                            .map(|planned| {
                                vec![
                                    label.clone(),
                                    planned.value.clone(),
                                    describe(&Freshness::RecordMissing),
                                ]
                            }),
                    );
                    // A column holding cells no record explains, which only the
                    // model knows it derives.
                    let cells_missing = |value: &str| -> usize {
                        let Some((table, column)) = value.split_once('.') else {
                            return 0;
                        };
                        let (Ok(table), Ok(column)) =
                            (Identifier::new(table), Identifier::new(column))
                        else {
                            return 0;
                        };
                        let sample = entry.sample.borrow();
                        let Ok(held) = sample.table(&table) else {
                            return 0;
                        };
                        held.index_tuples()
                            .into_iter()
                            .filter(|tuple| {
                                let address = samplekit::core::table::RowAddress::Index(
                                    tuple.iter().map(|value| (*value).clone()).collect(),
                                );
                                held.at(&address, &column).is_ok_and(|cell| {
                                    let records = cell.records();
                                    records.computed.is_none()
                                        && records.failure.is_none()
                                        && !records
                                            .fingerprint
                                            .as_ref()
                                            .is_some_and(|digest| digest.is_edited())
                                        && cell.peek_value().is_some_and(|value| !value.is_absent())
                                })
                            })
                            .count()
                    };
                    lines.extend(
                        plan.iter()
                            .filter(|planned| planned.reason == Reason::Edited)
                            .filter_map(|planned| {
                                let rows = cells_missing(&planned.value);
                                (rows > 0).then(|| {
                                    vec![
                                        label.clone(),
                                        planned.value.clone(),
                                        with_rows(&Freshness::RecordMissing, rows),
                                    ]
                                })
                            }),
                    );
                    let fresh: Vec<Vec<String>> = plan
                        .into_iter()
                        .filter(|planned| {
                            matches!(
                                planned.reason,
                                Reason::NeverComputed | Reason::Waiting | Reason::FormulaChanged
                            )
                        })
                        // A failure the file records is already listed as one.
                        .filter(|planned| {
                            !lines
                                .iter()
                                .any(|line| line[0] == label && line[1] == planned.value)
                        })
                        .map(|planned| {
                            // Said as `compute` says it: what nobody entered,
                            // a value waiting for itself, an entered value
                            // whose uncertainty a formula owes.
                            let said = if matches!(
                                planned.reason,
                                Reason::Waiting | Reason::FormulaChanged
                            ) {
                                planned.said.clone()
                            } else {
                                "never computed".to_string()
                            };
                            let state = said_of_plan(&entry.sample.borrow(), &planned.value, &said);
                            vec![label.clone(), planned.value, state]
                        })
                        .collect();
                    lines.extend(fresh);
                }
                Err(error) => unread.entry(error.to_string()).or_default().push(label),
            }
        }
    }
    Ok(unread)
}

/// Where one number came from. **A sample and a field**, in that order: the
/// question is about one number, so a directory is refused.
pub(super) fn explain(options: &Options) -> Outcome {
    let (Some(first), Some(field_name)) = (options.positionals.first(), options.positionals.get(1))
    else {
        // Said with the file given, where there is one: an example from
        // another collection named fields nobody here has.
        let file = options
            .positionals
            .first()
            .map_or_else(|| "<file>".to_string(), |first| shell_word(first));
        return Err(Fail::usage(format!(
            "explain wants a file and a field: samplekit explain {file} <field>"
        )));
    };
    let path = PathBuf::from(first);
    if !path.exists() {
        return Err(Fail::io(format!("{first}: no such file or directory")));
    }
    if path.is_dir() {
        return Err(Fail::usage(format!(
            "explain is about one number, and '{first}' is a directory\n  \
             samplekit explain {first}/<file>.md {field_name}"
        )));
    }
    let collection = loaded(&path)?;
    let entry = collection
        .iter()
        .next()
        .ok_or_else(|| Fail::data(format!("{first} holds no sample")))?;
    let sample = entry.sample.borrow();
    let vocabulary = collection.vocabulary();
    let own_config = entry
        .path
        .as_deref()
        .map_or(collection.config(), |path| collection.config_for(path));
    // The explanation is a value `explanation` builds; this renders it.
    let explained = explanation::explain(
        &sample,
        entry.path.as_deref(),
        own_config,
        field_name,
        &vocabulary,
    )
    .map_err(|error| match error {
        explanation::ExplainError::Unknown(error) => {
            let mut message = with_suggestion(error, &collection);
            // A value a model computes and nobody computed yet is in no file —
            // said of a name the model's source declares, and of no other: after
            // *unknown field 'nosuch'*, it offered to compute a misspelling.
            if let Some(config) = own_config
                && runtime::model_declarations(config)
                    .1
                    .iter()
                    .any(|declared| declared == field_name)
            {
                message.push_str(&format!(
                    "\n  the model declares it, and it was never computed: samplekit compute {}",
                    shell_word(first)
                ));
            }
            Fail::usage(message)
        }
        explanation::ExplainError::IsTable(name) => Fail::usage(format!(
            "'{name}' is a table: explain one of its cells, {name}.<column>[<index>]"
        )),
        // Refusing is right — explain is about one number — but a refusal that
        // names no way forward sends the reader away with nothing.
        explanation::ExplainError::NotOneValue => {
            let mut message = format!("'{field_name}' is not one value, and explain is about one");
            message.push_str(&format!("\n  see them: samplekit {first} -c {field_name}"));
            if let Some(quantity) = field_name.strip_suffix(".readings") {
                message.push_str(&format!(
                    "\n  a statistic of them is one value: {quantity}.stats.mean"
                ));
            }
            Fail::usage(message)
        }
        // The rows there are, and the nearest, as `set` offers it.
        explanation::ExplainError::NoSuchRow { table } => {
            let mut message = format!("{field_name}: {table} holds no such row");
            if let Some(held) = Identifier::new(&table)
                .ok()
                .and_then(|name| sample.table(&name).ok())
            {
                let rows: Vec<Value> = held
                    .rows()
                    .filter_map(|row| row.index().first().map(|value| (*value).clone()))
                    .collect();
                if !rows.is_empty() {
                    message.push_str(&format!(
                        "\n  the rows of '{table}': {}",
                        rows.iter().map(shown_value).collect::<Vec<_>>().join(", ")
                    ));
                }
                if let Some(nearest) = nearest_row_address(field_name, &rows) {
                    message.push_str(&format!("\n  did you mean: '{nearest}'?"));
                }
            }
            Fail::usage(message)
        }
    })?;
    let semantic_style = render_style(&collection, options)?;
    let explained = match explained {
        explanation::Explanation::Table(table) => {
            return shown_table(
                &table,
                &sample,
                entry.path.as_deref(),
                own_config,
                semantic_style,
            );
        }
        explanation::Explanation::Value(value) => value,
    };
    let subject = Subject {
        sample: &sample,
        path: entry.path.as_deref(),
        vocabulary: &vocabulary,
        states: entry.states.as_deref(),
    };
    // A field as a table's cell shows it: a cell carries its unit, and the
    // unit a reader sees here must be the one the table's header shows.
    let shown = |field: &str| {
        let profile = anonymous(
            &[ColumnSpec {
                field: field.to_string(),
                label: None,
                header: None,
                precision: None,
                template: None,
            }],
            options.precision.as_ref(),
        );
        render::cell(
            &profile.columns()[0],
            &subject,
            render::Declaration::Profile(&profile),
            own_config,
            semantic_style,
            true,
        )
    };
    outln!(
        "{} · {field_name} = {}",
        file_of(entry.path.as_deref(), &sample),
        shown(field_name)
    );
    let said_channel = |made: explanation::Made| match made {
        explanation::Made::Value => "its value",
        explanation::Made::Uncertainty => "its uncertainty",
        explanation::Made::Whole => "it",
    };
    let trace_lines = |trace: &Option<String>, missing: &str| match trace {
        Some(trace) => {
            outln!("");
            for line in trace.lines() {
                outln!("  {line}");
            }
        }
        None => outln!("\n  {missing}"),
    };
    match &explained.origin {
        explanation::Origin::Failed { message } => {
            // A formula that failed wrote its failure, and that is the answer;
            // the whole traceback is in the project's failure log.
            outln!("\n  its formula failed: {message}");
            trace_lines(
                &explained.trace,
                "no traceback kept for it — the log is written when a failure is written to the file",
            );
            return Ok(());
        }
        explanation::Origin::Nothing => {
            // Where a model computes it, *never computed* is what this is —
            // unless an input it reads holds nothing, and then it waits, as
            // `status` says it: sending the reader to `compute` computed
            // nothing, since the input was still missing.
            outln!("\n  nothing is stored here");
            if own_config.and_then(runtime::template_of).is_some() {
                let shown = shell_word(first);
                match waiting_on(field_name, &sample, &path) {
                    Some(waiting) if !waiting.missing.is_empty() => {
                        outln!(
                            "  it waits for {}, which nobody entered here: the model computes it \
                             from {}, as {}'s file records",
                            waiting.missing.join(" and "),
                            waiting.inputs.join(", "),
                            waiting.from
                        );
                        // A table's column is filled a row at a time.
                        let giving = match waiting.missing[0].split_once('.') {
                            Some((table, _)) => {
                                format!("--add-row {table} <column>=<value> … adds a row")
                            }
                            None => format!("{}=<value> gives it one", waiting.missing[0]),
                        };
                        outln!(
                            "  samplekit set {shown} {giving}, then samplekit compute {shown} \
                             --write"
                        );
                    }
                    Some(_) => outln!(
                        "  the model computes it, and it was never computed: samplekit compute \
                         {shown} --write"
                    ),
                    None => outln!(
                        "  if the model computes it, it was never computed — samplekit status \
                         {shown} says whether it waits for an input: samplekit compute {shown} \
                         --write computes it"
                    ),
                }
            }
            return Ok(());
        }
        explanation::Origin::Statistic { readings } => {
            let quantity = field_name.split('.').next().unwrap_or(field_name);
            outln!(
                "\n  a statistic of {} of {quantity}",
                counted_as(*readings, "reading", "readings")
            );
            return Ok(());
        }
        explanation::Origin::ByHand => {
            outln!("\n  put here by hand, over what a formula or a statistic gave")
        }
        // Said once: *this value is entered* below repeated it.
        explanation::Origin::Entered => outln!("\n  entered or measured; nothing computed it"),
        explanation::Origin::ReadsNothing(made) => outln!(
            "\n  a formula that reads nothing of this sample computed {}",
            said_channel(*made)
        ),
        explanation::Origin::OwnReadings => {
            outln!("\n  computed from its own readings, by the statistic its model declares");
        }
        explanation::Origin::Inputs { inputs, .. } => {
            outln!("\n  computed from");
            // An input emptied since is what the value now waits for, as
            // `status` and `compute` say: not *current* beside *outdated — it*.
            let emptied = explained
                .state
                .as_ref()
                .map(|state| emptied_inputs(&sample, state))
                .unwrap_or_default();
            // Each input's value as a table shows it, and its state; the digest
            // it was recorded with only with -v.
            let lines: Vec<[String; 4]> = inputs
                .iter()
                .map(|input| {
                    let value = match (&input.field, input.cells) {
                        (_, Some(cells)) => counted_as(cells, "cell", "cells"),
                        (Some(field), None) => {
                            let text = shown(field);
                            if text.is_empty() {
                                "—".to_string()
                            } else {
                                text
                            }
                        }
                        (None, None) => "—".to_string(),
                    };
                    let state = match &input.state {
                        _ if emptied.contains(&input.name) && value == "—" => {
                            "nobody entered it".to_string()
                        }
                        Some(explanation::InputState::Of(state)) => state_of(&sample, state, 0),
                        Some(explanation::InputState::Column { stale: true }) => {
                            "outdated".to_string()
                        }
                        Some(explanation::InputState::Column { stale: false }) => {
                            "current".to_string()
                        }
                        None => String::new(),
                    };
                    [input.name.clone(), value, input.recorded.clone(), state]
                })
                .collect();
            // Padded to the widest of each column, in characters: a unit such
            // as g/L pushed its state out of line under a fixed width.
            let width = |at: usize, least: usize| {
                lines
                    .iter()
                    .map(|line| line[at].chars().count())
                    .max()
                    // One space at least after the widest: `fermentation.gravity`
                    // ran into its value.
                    .map_or(0, |widest| widest + 1)
                    .max(least)
            };
            let (names, values, digests) = (width(0, 16), width(1, 18), width(2, 14));
            let pad = |text: &str, to: usize| {
                format!(
                    "{text}{}",
                    " ".repeat(to.saturating_sub(text.chars().count()))
                )
            };
            for [spelled, value, written, state] in &lines {
                if options.verbose {
                    outln!(
                        "    {} {} {} {state}",
                        pad(spelled, names),
                        pad(value, values),
                        pad(written, digests)
                    );
                } else {
                    outln!("    {} {} {state}", pad(spelled, names), pad(value, values));
                }
            }
        }
    }
    if let Some(state) = &explained.state {
        let emptied = emptied_inputs(&sample, state);
        if !emptied.is_empty() {
            let shown = shell_word(first);
            // What nobody entered is given a value; what itself waits is
            // explained in its turn.
            let absent = |name: &String| {
                Identifier::new(name).ok().is_none_or(|name| {
                    sample.property(&name).is_ok_and(|handle| {
                        handle
                            .peek(samplekit::core::property::Property::peek_value)
                            .is_none_or(|value| matches!(value, Value::Absent))
                    })
                })
            };
            match emptied.iter().find(|name| absent(name)) {
                Some(missing) => outln!(
                    "\n  this value waits for {}, which nobody entered — samplekit set {shown} \
                     {missing}=<value>, then samplekit compute {shown} --write",
                    emptied.join(", ")
                ),
                None => outln!(
                    "\n  this value waits for {}, which waits in turn — samplekit explain \
                     {shown} {} says for what",
                    emptied.join(", "),
                    emptied[0]
                ),
            }
            return Ok(());
        }
        // An entered value was said entered above.
        if matches!(explained.origin, explanation::Origin::Entered)
            && matches!(state, Freshness::Source)
        {
            return Ok(());
        }
        if matches!(state, Freshness::Edited) {
            // *This value is edited since it was computed* read badly, and
            // named no way back.
            outln!(
                "\n  this value was edited by hand since it was computed — samplekit compute {} \
                 --force gives it back to its formula",
                shell_word(first)
            );
            return Ok(());
        }
        outln!("\n  this value is {}", describe(state));
        // A failure keeps the value its formula last gave; beside it, the
        // exception's type alone told the reader nothing to act on.
        if matches!(state, Freshness::Failed { .. }) {
            trace_lines(
                &explained.trace,
                "no traceback kept for it — samplekit compute --rerun runs it again and keeps one",
            );
        }
    }
    Ok(())
}

/// What a value no file of a sample holds is computed from, as another sample
/// of its folder records it — read from the files, the model never run, since
/// `explain` reads the files alone — and which of those inputs this sample
/// lacks.
struct Waiting {
    inputs: Vec<String>,
    missing: Vec<String>,
    from: String,
}

fn waiting_on(
    field: &str,
    sample: &samplekit::core::sample::Sample,
    path: &Path,
) -> Option<Waiting> {
    let name = Identifier::new(field).ok()?;
    let siblings = siblings_of(path)?;
    let (from, inputs) = siblings.iter().find_map(|entry| {
        let other = entry.sample.borrow();
        let inputs: Vec<samplekit::core::property::InputName> = other
            .property(&name)
            .ok()?
            .records()
            .computed
            .as_ref()?
            .keys()
            .cloned()
            .collect();
        (!inputs.is_empty()).then(|| (other.name().unwrap_or("another sample").to_string(), inputs))
    })?;
    let mut said = Vec::new();
    let mut missing = Vec::new();
    for input in &inputs {
        let (shown, held) = match input {
            // A statistic's record names the value's own readings: held where
            // this sample holds readings of it. Looked for as a property of
            // that name, the blonde saison's gravities *waited for readings*
            // their file held.
            samplekit::core::property::InputName::Named(input) if input.as_str() == "readings" => (
                input.to_string(),
                sample
                    .property(&name)
                    .is_ok_and(|handle| handle.peek(|property| property.readings().is_some())),
            ),
            samplekit::core::property::InputName::Named(input) => (
                input.to_string(),
                sample.property(input).is_ok_and(|handle| {
                    handle
                        .peek(samplekit::core::property::Property::peek_value)
                        .is_some_and(|value| !value.is_absent())
                }) || sample.attribute(input).is_ok(),
            ),
            samplekit::core::property::InputName::Column { table, column } => (
                format!("{table}.{column}"),
                sample
                    .table(table)
                    .is_ok_and(|held| held.column(column).is_ok()),
            ),
            // A row's cell is read inside a table, which is not this case.
            samplekit::core::property::InputName::Cell(_) => continue,
        };
        if !held {
            missing.push(shown.clone());
        }
        said.push(shown);
    }
    Some(Waiting {
        inputs: said,
        missing,
        from,
    })
}

/// What is wrong, and nothing else: it dispatches to `validation`, which holds
/// a `&SampleList` and therefore cannot modify anything. A state as `status
/// --json` names it and `--accept` takes it: `outdated`, `failed`, `edited` (an
/// override, a record missing among them), `waiting`, `never computed`.
fn state_kind(said: &str) -> &'static str {
    if said.starts_with("outdated") {
        "outdated"
    } else if said.starts_with("failed") {
        "failed"
    } else if said.starts_with("edited") || said.starts_with("record missing") {
        "edited"
    } else if said.starts_with("waits for") {
        "waiting"
    } else {
        "never computed"
    }
}

pub(super) fn validate(options: &Options, json: bool) -> Outcome {
    let (root, _) = target_and_extra(options)?;
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    // Files named one by one are judged alone: the configuration's queries and
    // profiles were judged against the one file given, and a field its siblings
    // hold was *unknown* there.
    let files = !options.positionals.is_empty()
        && options
            .positionals
            .iter()
            .all(|target| Path::new(target).is_file());
    let report = if files {
        validation::run_on_files(&collection)
    } else {
        validation::run(&collection)
    };
    if json {
        let objects: Vec<serde_json::Value> = report
            .findings
            .iter()
            .map(|finding| {
                serde_json::json!({
                    "sample": finding.sample,
                    "path": finding.path.as_ref().map(|path| path.display().to_string()),
                    "severity": match finding.severity {
                        Severity::Defect => "defect",
                        Severity::Note => "note",
                    },
                    "said": validation::described(finding),
                })
            })
            .collect();
        outln!(
            "{}",
            serde_json::to_string_pretty(&objects).unwrap_or_else(|_| "[]".to_string())
        );
        if validation::has_defects(&report) {
            return Err(Fail::data(String::new()));
        }
        return Ok(());
    }
    let (mut defects, mut notes) = (0usize, 0usize);
    // Defects one by one: each is something to fix. Notes grouped by what they
    // say — seventy identical lines bury the defect worth reading.
    let mut grouped: Vec<(String, Vec<&validation::Finding>)> = Vec::new();
    for finding in &report.findings {
        match finding.severity {
            Severity::Defect => {
                defects += 1;
                outln!("  {}", describe_finding(finding));
            }
            Severity::Note => {
                notes += 1;
                let key = note_key(finding);
                match grouped.iter_mut().find(|(known, _)| *known == key) {
                    Some((_, members)) => members.push(finding),
                    None => grouped.push((key, vec![finding])),
                }
            }
        }
    }
    for (key, members) in &grouped {
        if let [only] = members.as_slice() {
            outln!("  {}", describe_finding(only));
            continue;
        }
        // Samples, each once: a note about a table's rows is one per row, and
        // counting findings named one sample three times over.
        let mut samples: Vec<&str> = Vec::new();
        for finding in members {
            if !samples.contains(&finding.sample.as_str()) {
                samples.push(finding.sample.as_str());
            }
        }
        let mut names: Vec<&str> = samples.iter().take(3).copied().collect();
        if samples.len() > 3 {
            names.push("…");
        }
        outln!(
            "  {key} — {}: {}",
            counted_as(samples.len(), "sample", "samples"),
            names.join(", ")
        );
    }
    // Set off from the findings above it, where there are any: alone, the
    // summary began with a blank line.
    outln!(
        "{}{} · {} · {}",
        if report.findings.is_empty() { "" } else { "\n" },
        counted_as(report.samples, "sample", "samples"),
        counted_as(defects, "defect", "defects"),
        counted_as(notes, "note", "notes")
    );
    if validation::has_defects(&report) {
        // A defect is a data error. A note is not: *this file would be
        // rewritten the next time something writes it* is not a failure.
        // Where the defects are, which is not how many samples were read: an
        // unread file is a defect in no sample.
        let files: std::collections::HashSet<_> = report
            .findings
            .iter()
            .filter(|finding| finding.severity == validation::Severity::Defect)
            .map(|finding| {
                finding
                    .path
                    .clone()
                    .unwrap_or_else(|| finding.sample.clone().into())
            })
            .collect();
        return Err(Fail::data(format!(
            "{} in {}",
            counted_as(defects, "defect", "defects"),
            counted_as(files.len(), "file", "files")
        )));
    }
    Ok(())
}

/// What a note says, without which sample — or which row — it is about.
pub(super) fn note_key(finding: &validation::Finding) -> String {
    let stem = |quantity: &str| quantity.split('[').next().unwrap_or(quantity).to_string();
    match &finding.detail {
        Detail::UnusableTag { tag, .. } => format!("tag '{tag}' is not an identifier"),
        Detail::TableSetAside { table, .. } => format!("table {table} was not read"),
        Detail::DeclaredStatisticDisagrees {
            quantity, channel, ..
        } => format!(
            "{}: {channel} is not the statistic its file declares",
            stem(quantity)
        ),
        Detail::NotCanonical => {
            "would be rewritten by a canonicalising pass: the next command that writes it does so"
                .to_string()
        }
        Detail::NotCurrent { .. } => {
            "holds values that are not current: samplekit status names them".to_string()
        }
        Detail::Overridden { .. } => {
            "holds values written over their formula: samplekit status names them".to_string()
        }
        Detail::TemplateBraceNearAChannel {
            declaration,
            written,
            ..
        } => format!("{declaration}: {{{written}}} in its template"),
        Detail::RecordsCannotBeFollowed { quantity, .. } => {
            format!("{quantity}: records cannot be followed")
        }
        Detail::Unreadable { reason } => format!("not read: {reason}"),
        Detail::DeclarationNamesNoField {
            declaration,
            reason,
        } => format!(
            "{declaration}: {}",
            reason.lines().next().unwrap_or_default()
        ),
        // Grouped by what it says of the quantity, not of each value.
        Detail::PrecisionWritesZero {
            quantity,
            specifier,
            ..
        } => {
            // The row goes, the channel stays: `m.ebc[20].u` is `m.ebc.u`.
            let (head, tail) = quantity.split_once('[').unwrap_or((quantity, ""));
            let channel = tail.split_once(']').map_or("", |(_, rest)| rest);
            format!(
                "{head}{channel}: precision {specifier} writes a value as zero — declare a finer one"
            )
        }
        // The rest reads as it does for one sample, without the sample.
        _ => describe_finding(&validation::Finding {
            sample: String::new(),
            ..finding.clone()
        })
        .trim_start()
        .to_string(),
    }
}

pub(super) fn describe_finding(finding: &validation::Finding) -> String {
    format!("{:<22} {}", finding.sample, validation::described(finding))
}

/// What a table is: its index, its columns, which are measured and which the
/// model fills, and how many rows it holds.
fn shown_table(
    table: &explanation::TableExplanation,
    sample: &samplekit::core::sample::Sample,
    path: Option<&Path>,
    config: Option<&ProjectConfig>,
    style: Option<&str>,
) -> Outcome {
    outln!(
        "{} · {} — {}, {}\n",
        file_of(path, sample),
        table.name,
        counted_as(table.rows, "row", "rows"),
        counted_as(table.columns.len(), "column", "columns")
    );
    for column in &table.columns {
        let kind = match &column.kind {
            explanation::ColumnKind::Index => "index".to_string(),
            explanation::ColumnKind::Measured => "measured".to_string(),
            explanation::ColumnKind::Computed { from } if from.is_empty() => "computed".to_string(),
            explanation::ColumnKind::Computed { from } => {
                format!("computed from {}", from.join(", "))
            }
        };
        // Spelled as every table of the project spells it — `°C`, not the
        // file's `degC`.
        let unit = column.unit.as_ref().map_or_else(String::new, |unit| {
            config
                .and_then(|config| {
                    config
                        .resolved_as(
                            &samplekit::core::formatting::Presentation {
                                unit: Some(unit.clone()),
                                symbol: None,
                                precision: None,
                            },
                            &format!("{}.{}", table.name, column.name),
                            style,
                        )
                        .ok()
                })
                .and_then(|resolved| resolved.unit)
                .unwrap_or_else(|| unit.clone())
        });
        outln!("  {:<16} {:<10} {}", column.name.to_string(), unit, kind);
    }
    outln!(
        "\n  samplekit explain <file> {}.<column>[<index>]   where one cell's number came from",
        table.name
    );
    Ok(())
}

/// `open`: a sample's own file, the folder holding it, or the sample's file in
/// the editor. Opened through `opening`, as Python and the workbench open it.
pub(super) fn open_file(
    options: &Options,
    sample: &Path,
    pattern: Option<&str>,
    navigate: bool,
    edit: bool,
) -> Outcome {
    use samplekit::presentation::opening;
    // One sample's file, and nothing else: a folder opened — or, with --edit,
    // edited — whichever sample its scan met first, without a word.
    if sample.is_dir() {
        return Err(Fail::usage(format!(
            "open is about one sample, and '{}' is a folder: samplekit open {}/<sample>.md",
            sample.display(),
            shell_path(sample).trim_end_matches('/')
        )));
    }
    let collection = loaded(sample)?;
    warn_skipped(&collection, options);
    let Some(entry) = collection.iter().next() else {
        return Err(Fail::data(format!("{} holds no sample", sample.display())));
    };
    let path = entry.path.clone().unwrap_or_else(|| sample.to_path_buf());
    let failed = |error: opening::OpenError| match error {
        opening::OpenError::NoDisplay { .. } => Fail::usage(error.to_string()),
        _ => Fail::io(error.to_string()),
    };
    if edit {
        opening::edit(&path).map_err(failed)?;
        outln!("edited {}", path.display());
        return Ok(());
    }
    let Some(config) = collection.config_for(&path) else {
        return Err(Fail::usage(format!(
            "no .samplekitrc above {} says where its files are: [collection] files = \
             [\"../images\"]",
            path.display()
        )));
    };
    if config.collection().files.is_empty() {
        return Err(Fail::usage(
            ".samplekitrc declares no directory for the samples' own files: [collection] \
             files = [\"../images\"], and a file named after its sample is found there"
                .to_string(),
        ));
    }
    let every = samplekit::config::discovery::sample_files(config, &path, None)
        .map_err(|error| Fail::io(error.to_string()))?;
    let found = samplekit::config::discovery::sample_files(config, &path, pattern)
        .map_err(|error| Fail::io(error.to_string()))?;
    let name = file_of(Some(&path), &entry.sample.borrow());
    let listed = |files: &[PathBuf]| {
        files
            .iter()
            .map(|file| format!("    {}", from_here(file)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if found.is_empty() {
        return Err(Fail::usage(if every.is_empty() {
            format!(
                "{name} has no file: none under [collection] files begins with its name, {}",
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default()
            )
        } else {
            format!(
                "no file of {name} matches '{}'\n  its files:\n{}",
                pattern.unwrap_or_default(),
                listed(&every)
            )
        }));
    }
    // Every file the pattern names opens; several files of one folder are one
    // folder to navigate to.
    let targets = if navigate {
        opening::folders_of(&found)
    } else {
        found.clone()
    };
    if targets.len() > opening::OPENED_WITHOUT_ASKING {
        let what = if navigate {
            counted_as(targets.len(), "folder", "folders")
        } else {
            counted_as(targets.len(), "file", "files")
        };
        let asked = format!(
            "{what} of {name}{}:\n{}",
            pattern
                .map(|pattern| format!(" match '{pattern}'"))
                .unwrap_or_default(),
            listed(&targets)
        );
        if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
            return Err(Fail::usage(format!(
                "{asked}\n  more than {} open only after a yes at a terminal: a narrower \
                 pattern names fewer",
                opening::OPENED_WITHOUT_ASKING
            )));
        }
        errln!("{asked}");
        eprint!("open them all? [y/N] ");
        io::stderr()
            .flush()
            .map_err(|error| Fail::io(format!("could not ask: {error}")))?;
        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|error| Fail::io(format!("could not read the answer: {error}")))?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            outln!("nothing opened");
            return Ok(());
        }
    }
    for target in &targets {
        if navigate {
            let folder = opening::navigate(target).map_err(failed)?;
            outln!("opened {}", from_here(&folder));
        } else {
            opening::open(target).map_err(failed)?;
            outln!("opened {}", from_here(target));
        }
    }
    Ok(())
}

/// `list files`: every sample's own files, and those belonging to none — a file
/// whose name was mistyped reads there.
pub(super) fn list_files(collection: &SampleList) -> Outcome {
    let everything = collection.filter_by(|_| true);
    let mut declared = false;
    let mut listed = false;
    for part in collection.by_configuration(&everything) {
        let Some(config) = part.config else {
            continue;
        };
        if config.collection().files.is_empty() {
            continue;
        }
        declared = true;
        let paths: Vec<PathBuf> = part
            .samples
            .iter()
            .filter_map(|entry| entry.path.clone())
            .collect();
        // Whose each file is, among every sample of the project: listing one
        // sample called the others' files nobody's.
        let project: Vec<PathBuf> =
            samplekit::collection::sample_list::from_directory(config.root())
                .map(|whole| {
                    // This configuration's samples: a nested project's are its own.
                    whole
                        .iter()
                        .filter_map(|entry| entry.path.clone())
                        .filter(|path| {
                            whole
                                .config_for(path)
                                .is_some_and(|theirs| theirs.root() == config.root())
                        })
                        .collect()
                })
                .unwrap_or_else(|_| paths.clone());
        let canonical = |path: &PathBuf| dunce::canonicalize(path).unwrap_or_else(|_| path.clone());
        let asked: Vec<PathBuf> = paths.iter().map(canonical).collect();
        let whole = project.len() <= paths.len();
        let mut found = samplekit::config::discovery::files_by_sample(config, &project)
            .map_err(|error| Fail::io(error.to_string()))?;
        found
            .owned
            .retain(|(sample, _)| asked.contains(&canonical(sample)));
        // Only the whole project can say a file belongs to nobody: over part of
        // it they are left out — and said to be, where there are some, rather
        // than left out in silence.
        let set_aside = if whole {
            0
        } else {
            std::mem::take(&mut found.unowned).len()
        };
        for (sample, files) in &found.owned {
            listed = true;
            let name = part
                .samples
                .iter()
                .find(|entry| entry.path.as_ref().map(canonical) == Some(canonical(sample)))
                .map(|entry| file_of(Some(sample), &entry.sample.borrow()))
                .unwrap_or_else(|| sample.display().to_string());
            outln!("{name}");
            for file in files {
                outln!("  {}", from_here(file));
            }
        }
        if !found.unowned.is_empty() {
            listed = true;
            // A file is a sample's where its name begins with the sample's.
            outln!("\nbelonging to no sample — no sample's name begins these names");
            for file in &found.unowned {
                outln!("  {}", from_here(file));
            }
        }
        if set_aside > 0 {
            let root = from_here(config.root());
            outln!(
                "\n{} no sample — listed over the whole project: samplekit list files {}",
                counted_as(set_aside, "file belongs to", "files belong to"),
                shell_word(if root.is_empty() { "." } else { &root })
            );
        }
    }
    if !declared {
        // The configuration named only where there is one to write it in.
        if collection.config().is_some() || !declaring(collection).is_empty() {
            outln!(
                "no directory is declared for the samples' own files: [collection] files = \
                 [\"../images\"] in .samplekitrc"
            );
        } else {
            outln!(
                "no directory is declared for the samples' own files, and no .samplekitrc is \
                 here to declare one: samplekit init writes it, then [collection] files = \
                 [\"../images\"]"
            );
        }
    } else if !listed {
        outln!("no file found under [collection] files");
    }
    Ok(())
}

/// A path as it reads from where the command runs.
fn from_here(path: &Path) -> String {
    relative_to_here(path)
}
