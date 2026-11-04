//! `compute`: the plan, the run with its progress and its log, and what it reports.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::collection::computation;

/// A value whose formula reads the value itself, which nobody entered: said
/// one way by the plan, the run and `explain`.
pub(super) const WAITS_FOR_ITSELF: &str = "waits for its value, which nobody entered";

/// What the model's plan says of a value, as every command says it: a value
/// waiting for itself as the run says it — `notch waits for notch` read as a riddle
/// — and an entered value whose uncertainty was never computed as `status`
/// says it, where the plan said *never computed* of a value that is there.
pub(super) fn said_of_plan(
    sample: &samplekit::core::sample::Sample,
    value: &str,
    said: &str,
) -> String {
    if said == format!("waits for {value}") {
        return WAITS_FOR_ITSELF.to_string();
    }
    if said == "never computed"
        && Identifier::new(value).ok().is_some_and(|name| {
            sample.property(&name).is_ok_and(|handle| {
                handle
                    .peek(samplekit::core::property::Property::peek_value)
                    .is_some_and(|value| !value.is_absent())
            })
        })
    {
        return "uncertainty never computed".to_string();
    }
    // Outdated, said with what it moved on from, as `status` says it.
    if said == "outdated"
        && let Ok(name) = Identifier::new(value)
        && let Ok(state) = samplekit::format::fingerprint::check_property(sample, &name)
        && matches!(state, Freshness::Stale { .. })
    {
        return state_of(sample, &state, 0);
    }
    said.to_string()
}

pub(super) fn interrupted() -> bool {
    INTERRUPTED.load(std::sync::atomic::Ordering::SeqCst)
}

pub(super) fn compute(options: &Options, flags: &ComputeFlags) -> Outcome {
    // What the model prints is kept, and shown only when asked.
    let (root, _) = target_and_extra(options)?;
    if !flags.dry_run {
        // One log a project, the last run's: its project is the
        // configuration's folder, else the folder computed.
        let project = samplekit::config::project_config::find(&root)
            .and_then(|file| file.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| root.clone());
        *run_log() = Some(RunLog::new(flags.show_output, &project));
        runtime::relay_output_through(relay_model_output);
    }
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    if selected.is_empty() {
        outln!("nothing to change — no sample is selected");
        return Ok(());
    }

    let groups = groups_of(&selected)?;
    if !flags.dry_run {
        // Ctrl-C, or the command killed: stop, interrupt the workers, and say
        // what was kept.
        let _ = ctrlc::set_handler(|| {
            INTERRUPTED.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Ok(pids) = WORKER_PIDS.lock() {
                for pid in pids.iter() {
                    // Silent, as every other signal passed on: a worker the
                    // watchdog stopped already made the shell's `kill` say,
                    // in the machine's language, that no such process is.
                    let _ = std::process::Command::new("kill")
                        .args(["-INT", &pid.to_string()])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
            }
        });
    }

    let mut workers: Vec<Worker> = Vec::new();
    let mut totals = Totals::default();
    // Over several projects, one that cannot run stops only itself: said,
    // naming it, and the others computed; its code is the command's.
    let mut stopped: Option<Fail> = None;
    for group in &groups {
        if let Err(error) = compute_group(&selected, group, flags, &mut workers, &mut totals) {
            if groups.len() == 1 || error.code == 130 {
                // A run an error stops still says where its log is.
                say_run_log();
                return Err(error);
            }
            let project = group.file.as_deref().map_or_else(
                || "samples with no configuration".to_string(),
                |file| file.display().to_string(),
            );
            errln!("error: {project}: {}", error.message);
            if stopped.as_ref().is_none_or(|held| error.code > held.code) {
                stopped = Some(error);
            }
        }
    }

    // What was written is kept in each project's trace, a run interrupted
    // included: what it wrote was computed.
    if flags.write {
        trace_failures(std::mem::take(&mut totals.tracing).keep());
    }
    if interrupted() {
        // Said once: *nothing was written; the value in progress was not
        // written* said the second half twice.
        let kept = if flags.write {
            format!(
                "{} kept in {}; the value in progress was not written",
                counted_as(totals.computed, "value", "values"),
                counted_as(totals.samples, "sample", "samples")
            )
        } else {
            "nothing was written".to_string()
        };
        errln!("\ninterrupted: {kept}");
        say_run_log();
        return Err(Fail {
            code: 130,
            message: String::new(),
        });
    }
    if totals.asked > 0 && totals.unknown == totals.asked {
        // A value the samples hold without a formula — or readings with no
        // statistic chosen — is said as such, with its way out, never as a
        // value they lack.
        if let Some(message) = totals
            .unknown_messages
            .iter()
            .find(|message| message.contains("no formula") || message.contains("no statistic"))
        {
            return Err(Fail::usage(message.trim().to_string()));
        }
        let suggestion = totals
            .unknown_messages
            .iter()
            .find_map(|message| message.lines().find(|line| line.contains("did you mean")))
            .map(|line| format!("\n{line}"))
            .unwrap_or_default();
        return Err(Fail::usage(format!(
            "no selected sample has {}{suggestion}",
            flags.names.join(", ")
        )));
    }
    for message in &totals.unknown_messages {
        warn(message);
    }
    // A model that does not load has nothing to say about what is pending:
    // *nothing to compute* would be a claim it cannot make.
    if totals.unloaded > 0 && totals.computed == 0 && totals.planned.is_empty() {
        return Err(Fail::data(format!(
            "the model raised while loading {}: nothing was planned or computed",
            counted_as(totals.unloaded, "sample", "samples")
        )));
    }
    if flags.dry_run {
        if totals.planned.is_empty() && !totals.unread_models.is_empty() {
            // Nothing the files record is pending; what the model owes is not
            // known, and *nothing to compute* would claim it.
            outln!(
                "every value the files record in {} is current",
                counted_as(selected.len(), "sample", "samples")
            );
        } else if totals.planned.is_empty() {
            outln!(
                "nothing to change — nothing to compute in {}",
                counted_as(selected.len(), "sample", "samples")
            );
        } else {
            let waiting =
                |row: &Vec<String>| row.get(2).is_some_and(|said| said.starts_with("waits for"));
            // Counted where something would be computed: a sample whose only
            // values wait computes nothing, and was counted among those that do.
            let samples: std::collections::HashSet<&String> = totals
                .planned
                .iter()
                .filter(|row| !waiting(row))
                .map(|row| &row[0])
                .collect();
            let style = table_style(&collection, options)?;
            out!(
                "{}",
                render::table_keeping(
                    ["sample", "value", "state"]
                        .iter()
                        .map(|name| name.to_string())
                        .collect(),
                    // Each state in its colour, once its words have been read.
                    totals
                        .planned
                        .iter()
                        .map(|row| {
                            let mut row = row.clone();
                            if let Some(state) = row.get_mut(2) {
                                *state = render::painted_state(state);
                            }
                            row
                        })
                        .collect(),
                    output_target(options),
                    style,
                    3
                )
            );
            // What will wait is not what would be computed.
            // By cells, as what is computed is counted.
            let cells = |at: usize| totals.planned_cells.get(at).copied().unwrap_or(1);
            let waits: usize = totals
                .planned
                .iter()
                .enumerate()
                .filter(|(_, row)| waiting(row))
                .map(|(at, _)| cells(at))
                .sum();
            let computed: usize = (0..totals.planned.len()).map(cells).sum::<usize>() - waits;
            let wait = counted_as(waits, "waits", "wait");
            // Without its model nothing can be computed: the flags that would
            // compute are not offered where they could only fail.
            let how = if totals.unread_models.is_empty() {
                "--write computes and writes without asking"
            } else {
                "once the model is found, it can be computed"
            };
            // Asked once the command has said all it has to say.
            if totals.unread_models.is_empty() && computed > 0 {
                TO_COMPUTE.store(computed, AtomicOrdering::Relaxed);
            }
            outln!(
                "\n{}",
                advice(&if computed == 0 {
                    // Nothing to compute is said as such, not as *0 values in
                    // 1 sample would be computed*.
                    format!("nothing would be computed: {wait} for an input nobody entered")
                } else if waits > 0 {
                    format!(
                        "{} in {} would be computed, {wait} for an input nobody entered — {how}",
                        counted_as(computed, "value", "values"),
                        counted_as(samples.len(), "sample", "samples")
                    )
                } else {
                    format!(
                        "{} in {} would be computed — {how}",
                        counted_as(computed, "value", "values"),
                        counted_as(samples.len(), "sample", "samples")
                    )
                })
            );
        }
        say_kept(&selected, flags);
        // The model that could not be read, after what the files say: the
        // listing is given, and the failure is the command's.
        if !totals.unread_models.is_empty() {
            warn(
                "the model was not read, so this lists what the files record: \
                 values never computed are known only to the model",
            );
            left_out(&totals).or_else(|error| {
                errln!("error: {}", error.message);
                Ok::<(), Fail>(())
            })?;
            let mut unread = std::mem::take(&mut totals.unread_models);
            if unread.len() == 1 {
                return Err(unread.remove(0));
            }
            for fail in &unread {
                errln!("error: {}", fail.message);
            }
            return Err(Fail {
                code: unread.iter().map(|fail| fail.code).max().unwrap_or(1),
                message: String::new(),
            });
        }
        return left_out(&totals);
    }
    if totals.computed == 0 && totals.failed == 0 && totals.not_saved.is_empty() {
        if totals.waiting > 0 {
            let first = to_fill(&totals);
            outln!(
                "\nnothing computed in {}: {} for what nobody entered — fill in {}, then compute",
                counted_as(selected.len(), "sample", "samples"),
                counted_as(totals.waiting, "value waits", "values wait"),
                first.join(", ")
            );
            say_run_log();
            return Ok(());
        }
        outln!(
            "nothing to change — nothing to compute in {}",
            counted_as(selected.len(), "sample", "samples")
        );
        say_kept(&selected, flags);
        say_run_log();
        return Ok(());
    }
    show_changes(&totals, &collection, options)?;
    let failures = if totals.failed > 0 {
        format!(", {} failed", totals.failed)
    } else {
        String::new()
    };
    let fill = to_fill(&totals);
    let failures = match (totals.waiting, fill.is_empty()) {
        (0, _) => failures,
        (waiting, true) => format!(
            "{failures}, {} behind what failed",
            counted_as(waiting, "waits", "wait")
        ),
        (waiting, false) => format!(
            "{failures}, {} for {}",
            counted_as(waiting, "waits", "wait"),
            fill.join(", ")
        ),
    };
    // By cells, as what is computed is counted.
    let cells = |at: usize| totals.change_cells.get(at).copied().unwrap_or(1);
    let same: usize = totals
        .changes
        .iter()
        .enumerate()
        .filter(|(_, change)| unchanged(&change[2], &change[3]) && !gave_nothing(&change[3]))
        .map(|(at, _)| cells(at))
        .sum();
    let nothing: usize = totals
        .changes
        .iter()
        .filter(|change| gave_nothing(&change[3]))
        .count();
    let same = match (same, nothing) {
        (0, 0) => String::new(),
        (same, 0) if same >= totals.computed => ", all unchanged".to_string(),
        (same, 0) => format!(", {same} unchanged"),
        (0, nothing) => format!(
            ", {} no value",
            counted_as(nothing, "formula gave", "formulas gave")
        ),
        (same, nothing) => format!(
            ", {same} unchanged, {} no value",
            counted_as(nothing, "formula gave", "formulas gave")
        ),
    };
    outln!(
        "\ncomputed {} in {}{same}{failures}",
        counted_as(totals.computed, "value", "values"),
        counted_as(totals.samples, "sample", "samples")
    );
    // In the words every command that writes a file says it.
    if flags.write && totals.computed > 0 {
        for line in written_lines(&totals.written_paths) {
            outln!("{line}");
        }
    }
    // What a narrowed run left outdated is said, not computed.
    if totals.pending > 0 {
        outln!(
            "{} left outdated or never computed — samplekit compute",
            counted_as(totals.pending, "value", "values")
        );
    }
    say_kept(&selected, flags);
    say_run_log();
    // What a write of these values would lose.
    if !flags.write && totals.computed > 0 {
        let comments: usize = totals
            .tried
            .iter()
            .filter_map(|(_, sample, _)| std::fs::read_to_string(sample).ok())
            .map(|text| samplekit::format::document::comments_in(&text))
            .sum();
        if comments > 0 {
            outln!(
                "a write drops {} — remarks belong in the note",
                counted_as(comments, "YAML comment", "YAML comments")
            );
        }
    }
    // Computed and not written: asked whether to write it.
    if !flags.write && totals.computed > 0 {
        outln!();
        {
            if ask("apply these changes?") {
                // A write like `--write`'s: recorded as computed by the model
                // the run began with, and one snapshot in the history, which
                // the command line takes only of `--write`.
                let places: Vec<PathBuf> = totals
                    .tried
                    .iter()
                    .map(|(_, sample, _)| sample.clone())
                    .collect();
                let (writing, failures) =
                    samplekit::config::version_control::before_writing(&places);
                history_failures(failures);
                let mut written_paths: Vec<String> = Vec::new();
                let mut refused = None;
                for (position, sample, template) in &totals.tried {
                    let request = Request {
                        sample: sample.clone(),
                        template: template.clone(),
                        names: Vec::new(),
                        rerun: false,
                        force: false,
                        timeout: flags.timeout,
                        refused: Vec::new(),
                        overran: Vec::new(),
                        recorded: Default::default(),
                    };
                    match workers[*position].write(&request) {
                        Ok(report) if !report.saved => {}
                        Ok(report) => {
                            written_paths.push(relative_to_here(sample));
                            totals.tracing.wrote(
                                totals.configurations.get(sample).and_then(Option::as_ref),
                                totals.before.get(sample).map_or(&[][..], Vec::as_slice),
                                sample,
                                &report,
                            );
                        }
                        Err(error) => {
                            refused = Some(model_failure(error));
                            break;
                        }
                    }
                }
                // Taken whatever the writes answered: when one is refused,
                // those before it were written.
                trace_failures(std::mem::take(&mut totals.tracing).keep());
                let said = COMMAND_LINE
                    .get()
                    .map_or("samplekit compute", String::as_str);
                history_after(writing, &answered_yes(said));
                if let Some(refused) = refused {
                    return Err(refused);
                }
                for line in written_lines(&written_paths) {
                    outln!("{line}");
                }
            } else {
                outln!("nothing written");
            }
        }
    }
    if let Some(error) = stopped {
        return Err(Fail {
            code: error.code,
            message: "a project was not computed, said above; the others were".to_string(),
        });
    }
    if !totals.not_saved.is_empty() {
        return Err(Fail::data(format!(
            "{} changed while {} computed, and {} not saved: samplekit {} compute --write \
             computes {} again from what {} now",
            totals.not_saved.join(", "),
            if totals.not_saved.len() == 1 {
                "it was"
            } else {
                "they were"
            },
            if totals.not_saved.len() == 1 {
                "was"
            } else {
                "were"
            },
            totals.not_saved.join(" "),
            if totals.not_saved.len() == 1 {
                "it"
            } else {
                "them"
            },
            if totals.not_saved.len() == 1 {
                "it holds"
            } else {
                "they hold"
            },
        )));
    }
    if totals.failed > 0 {
        return Err(Fail::data(format!(
            "{} failed; their tracebacks are above",
            counted_as(totals.failed, "value", "values")
        )));
    }
    left_out(&totals)
}

/// A sample whose records cannot be followed was left out of the plan or of the
/// run, and that is a failure of the command, said last so that what did run is
/// read first. What the files record as not current, planned as the model would
/// plan it where the model cannot be read: an override is kept, not computed,
/// and a sample whose records form a cycle is left out.
fn list_from_files(entries: &[&list::Entry], totals: &mut Totals) {
    for entry in entries {
        let sample = entry.sample.borrow();
        let label = file_of(entry.path.as_deref(), &sample);
        let behind = not_current(&sample);
        // Not judged is not *would be computed*: the sample is left out of a
        // run, and said here as it is there.
        if let Some(reason) = behind.iter().find_map(|(_, state, _)| match state {
            Freshness::Unjudged { reason } => Some(reason.clone()),
            _ => None,
        }) {
            totals.unplannable.push(format!("{label}: {reason}"));
            continue;
        }
        for (quantity, state, rows) in behind {
            if state != Freshness::Edited {
                totals.planned_cells.push(rows.max(1));
                totals.planned.push(vec![
                    label.clone(),
                    quantity,
                    state_of(&sample, &state, rows),
                ]);
            }
        }
    }
}

/// The cells a planned value counts: a table's column its rows; a value and
/// its uncertainty of two formulas two; else one.
fn planned_cells(sample: &samplekit::core::sample::Sample, value: &str) -> usize {
    if let Some((table, column)) = value.split_once('.') {
        if column.contains(['.', '[']) {
            return 1;
        }
        return Identifier::new(table)
            .ok()
            .and_then(|table| {
                sample
                    .table(&table)
                    .ok()
                    .map(|held| held.index_tuples().len())
            })
            .unwrap_or(1)
            .max(1);
    }
    let two = Identifier::new(value).ok().is_some_and(|name| {
        sample
            .property(&name)
            .is_ok_and(|handle| handle.peek(samplekit::core::property::Property::has_two_formulas))
    });
    if two { 2 } else { 1 }
}

pub(super) fn left_out(totals: &Totals) -> Outcome {
    if totals.unplannable.is_empty() {
        return Ok(());
    }
    Err(Fail::data(format!(
        "{} left out: records that cannot be followed give no order to compute in\n  {}\n  samplekit validate says how to repair each",
        counted_as(totals.unplannable.len(), "sample was", "samples were"),
        totals.unplannable.join("\n  ")
    )))
}

/// The overrides a run left alone, said with the flag that gives them back.
pub(super) fn say_kept(selected: &SampleList, flags: &ComputeFlags) {
    if flags.force {
        return;
    }
    let kept = computation::edited_kept(selected, &flags.names);
    if kept > 0 {
        outln!("{}", overrides_kept(kept));
    }
}

/// Before and after, laid out so: one sample, a table of its values
/// under its file; one value over several samples, a table of the samples; and
/// otherwise a table per sample. Whether a value computed again came out as it
/// was.
pub(super) fn unchanged(before: &str, after: &str) -> bool {
    before == after || after == "0 changed"
}

/// A value computed that came out as nothing: its formula returned `None` —
/// a `return` forgotten — which read *unchanged* beside a value never there,
/// and `status` then called it never computed.
pub(super) fn gave_nothing(after: &str) -> bool {
    after == render::ABSENT
}

pub(super) fn show_changes(totals: &Totals, collection: &SampleList, options: &Options) -> Outcome {
    if totals.changes.is_empty() {
        return Ok(());
    }
    let style = table_style(collection, options)?;
    let target = output_target(options);
    let samples: Vec<&String> =
        totals
            .changes
            .iter()
            .map(|change| &change[0])
            .fold(Vec::new(), |mut seen, sample| {
                if !seen.contains(&sample) {
                    seen.push(sample);
                }
                seen
            });
    let values: std::collections::HashSet<&String> =
        totals.changes.iter().map(|change| &change[1]).collect();
    // Before, and after — or `unchanged` where after would repeat before.
    let row = |first: &str, change: &[String; 4]| {
        let after = if gave_nothing(&change[3]) {
            format!("{} its formula gave no value", render::ABSENT)
        } else if unchanged(&change[2], &change[3]) {
            "unchanged".to_string()
        } else {
            change[3].clone()
        };
        // Two values said with every digit, because their precision writes
        // them alike, are said with the fewest that tell them apart.
        if samplekit::presentation::changes::every_digit(&change[2])
            || samplekit::presentation::changes::every_digit(&after)
        {
            let (before, after) = samplekit::presentation::changes::apart(&change[2], &after);
            return vec![first.to_string(), before, after];
        }
        vec![first.to_string(), change[2].clone(), after]
    };
    // After is what a computation is read for: before gives way first.
    let draw = |first: &str, rows: Vec<Vec<String>>| {
        let headers = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();
        // Before and after, each value's number bold and its unit coloured,
        // where a terminal reads them.
        let rows: Vec<Vec<String>> = rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .enumerate()
                    .map(|(at, cell)| {
                        if at == 0 {
                            cell
                        } else {
                            render::painted_said(&cell)
                        }
                    })
                    .collect()
            })
            .collect();
        let full = render::table(
            headers(&[first, "before", "after"]),
            rows.clone(),
            target,
            style,
        );
        if !full.contains(render::HIDDEN) {
            return full;
        }
        let narrow = rows
            .into_iter()
            .map(|mut row| {
                row.remove(1);
                row
            })
            .collect();
        format!(
            "{}before hidden — --width shows it\n",
            render::table(headers(&[first, "after"]), narrow, target, style)
        )
    };
    outln!();
    if samples.len() > 1 && values.len() == 1 {
        let value = totals.changes[0][1].clone();
        outln!("{value}");
        let rows = totals
            .changes
            .iter()
            .map(|change| row(&change[0], change))
            .collect();
        out!("{}", draw("sample", rows));
        return Ok(());
    }
    for sample in samples {
        outln!("{sample}");
        let rows = totals
            .changes
            .iter()
            .filter(|change| &change[0] == sample)
            .map(|change| row(&change[1], change))
            .collect();
        out!("{}", draw("value", rows));
    }
    Ok(())
}

/// A value a sample does not have: the first line of the model's message, and
/// its nearest name when it has one.
pub(super) fn unknown_value_line(label: &str, message: &str) -> String {
    let mut line = format!("{label}: {}", message.lines().next().unwrap_or_default());
    // A suggestion always follows; the rest of a refusal only where it is
    // the way out — the write that gives a value back — and never the list of
    // every name available, repeated for each sample.
    let refusal = message.contains("no statistic") || message.contains("no formula");
    for more in message.lines().skip(1) {
        if more.contains("did you mean") || (refusal && more.starts_with(' ')) {
            line.push_str(&format!("\n{more}"));
        }
    }
    line
}

/// The selected samples by the configuration that describes each.
pub(super) fn groups_of(selected: &SampleList) -> Result<Vec<Group>, Fail> {
    computation::groups_of(selected, forced_configuration().as_ref())
        .map_err(|error| Fail::data(error.to_string()))
}

pub(super) fn compute_group(
    selected: &SampleList,
    group: &Group,
    flags: &ComputeFlags,
    workers: &mut Vec<Worker>,
    totals: &mut Totals,
) -> Outcome {
    let entries: Vec<&list::Entry> = group
        .entries
        .iter()
        .filter_map(|position| selected.get(*position))
        .collect();
    // The model is named before it runs, nothing being asked: from here, in
    // one line. Its configuration, and their whole paths, under -v as every
    // command says them. **Once**: computed after its question, it was named
    // again before the second.
    thread_local! {
        static NAMED: std::cell::RefCell<Vec<Option<PathBuf>>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    let first = NAMED.with(|named| {
        let mut named = named.borrow_mut();
        let first = !named.contains(&group.file);
        if first {
            named.push(group.file.clone());
        }
        first
    });
    if !first {
    } else if VERBOSE.load(AtomicOrdering::Relaxed) {
        match &group.file {
            Some(file) => errln!("configuration  {}", super::as_here(file).display()),
            None => errln!("configuration  none found"),
        }
        errln!("model          {}", model_of(group.config.as_ref()));
    } else if let Some(template) = group.config.as_ref().and_then(runtime::template_of) {
        errln!("model  {}", relative_to_here(template.path()));
    }

    let from = entries
        .first()
        .and_then(|entry| entry.path.as_deref())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let unavailable = match runtime::availability(group.config.as_ref(), &from) {
        Ok(Availability::NoTemplate) => {
            outln!(
                "nothing to change — no model is declared for {}",
                counted_as(entries.len(), "sample", "samples")
            );
            return Ok(());
        }
        Ok(Availability::Ready { template, python }) => Ok((template, python)),
        Ok(Availability::Unavailable { reason }) => {
            Err(Fail::io(format!("the model cannot run: {reason}")))
        }
        Err(error) => Err(model_failure(error)),
    };
    let (template, python) = match unavailable {
        Ok(ready) => ready,
        // The listing reads the model wherever it adds something, and gives
        // what the files record where it cannot be read: the failure is said
        // once the listing is.
        Err(fail) if flags.dry_run => {
            list_from_files(&entries, totals);
            totals.unread_models.push(fail);
            return Ok(());
        }
        Err(fail) => return Err(fail),
    };

    // One worker per interpreter, started at the first computation that needs
    // it and kept for the rest.
    let position = match workers.iter().position(|worker| worker.python() == python) {
        Some(position) => position,
        None => {
            let started = Worker::start(&python).map_err(model_failure)?;
            if let Ok(mut pids) = WORKER_PIDS.lock() {
                pids.push(started.id());
            }
            workers.push(started);
            workers.len() - 1
        }
    };
    let worker = &mut workers[position];

    let counts = name_counts(
        entries
            .iter()
            .map(|entry| entry.sample.borrow().name().map(str::to_string)),
    );
    // What runs, and what is left out and said: a sample whose records form a
    // cycle has no order to run in, and the run fails at its end.
    let planned = computation::requests(
        selected,
        group,
        &template,
        &computation::Asked {
            names: flags.names.clone(),
            rerun: flags.rerun,
            force: flags.force,
            timeout: flags.timeout,
        },
    );
    let label_of = |path: &Path| {
        entries
            .iter()
            .find(|entry| entry.path.as_deref() == Some(path))
            .map(|entry| labelled(&counts, Some(path), &entry.sample.borrow()))
            .unwrap_or_else(|| path.display().to_string())
    };
    if let Some(error) = &planned.unread_trace {
        warn(&format!(
            "the trace of the formulas could not be read ({error}): each value is judged \
             by its inputs alone"
        ));
    }
    for (path, before) in &planned.before {
        totals.before.insert(path.clone(), before.clone());
        totals
            .configurations
            .insert(path.clone(), group.config.clone());
    }
    for (path, reason) in &planned.unplannable {
        totals
            .unplannable
            .push(format!("{}: {reason}", label_of(path)));
    }
    let requests: Vec<(String, Request)> = planned
        .requests
        .into_iter()
        .map(|request| (label_of(&request.sample), request))
        .collect();

    if !flags.names.is_empty() {
        totals.asked += requests.len();
    }
    if flags.dry_run {
        for (label, request) in &requests {
            match worker.plan(request) {
                Ok(plan) => {
                    let entry = entries
                        .iter()
                        .find(|entry| entry.path.as_deref() == Some(request.sample.as_path()));
                    totals
                        .planned_cells
                        .extend(plan.iter().map(|planned| match entry {
                            Some(entry) => planned_cells(&entry.sample.borrow(), &planned.value),
                            None => 1,
                        }));
                    totals.planned.extend(plan.into_iter().map(|planned| {
                        let said = match entry {
                            Some(entry) => {
                                said_of_plan(&entry.sample.borrow(), &planned.value, &planned.said)
                            }
                            None => planned.said.clone(),
                        };
                        vec![label.clone(), planned.value, said]
                    }))
                }
                Err(ModelError::TemplateFailed { traceback }) => {
                    warn_gathered(
                        "raised while loading",
                        |count| format!("the model raised while loading {count} samples"),
                        format!("{label}: the model raised while loading it"),
                    );
                    totals.unloaded += 1;
                    let last = traceback.trim_end().lines().last().unwrap_or_default();
                    if totals.tracebacks.insert(last.to_string()) {
                        errp!("{traceback}");
                    }
                }
                Err(ModelError::UnknownValue { message }) => {
                    totals.unknown += 1;
                    totals
                        .unknown_messages
                        .push(unknown_value_line(label, &message));
                }
                Err(other) => return Err(model_failure(other)),
            }
        }
        return Ok(());
    }

    // The model's change kept before anything runs, its formulas named: a
    // command writing during a long computation kept it first, unnamed.
    if flags.write
        && !requests.is_empty()
        && let Some(config) = &group.config
    {
        samplekit::collection::computation::keep_model_change_ahead(config);
    }
    let width = requests
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let progress = Progress::new(requests.len());
    for (label, request) in &requests {
        if interrupted() {
            break;
        }
        progress.next_sample(label);
        let finished_before = totals.changes.len();
        // The columns of one table finish together, since a table resolves as a
        // whole: they are one line, with the time they took.
        let group: std::cell::RefCell<Option<(String, Vec<String>, f64)>> =
            std::cell::RefCell::new(None);
        let flush = |progress: &Progress| {
            let taken = group.borrow_mut().take();
            if let Some((table, columns, seconds)) = taken {
                let value = match columns.as_slice() {
                    [column] => format!("{table}.{column}"),
                    _ => format!("{table}: {}", columns.join(", ")),
                };
                let line = finished_line(label, width, &value, seconds);
                progress.above(|| outln!("{line}"));
            }
        };
        let finished_from = totals.changes.len();
        let mut on = |event: &Event| match event {
            Event::Planned { count } => progress.planned(*count),
            Event::Started { value } => {
                // The next value starting closes a table's line: said now, not
                // once that value is done.
                let continues = value.split_once('.').is_some_and(|(table, _)| {
                    group
                        .borrow()
                        .as_ref()
                        .is_some_and(|(current, _, _)| current == table)
                });
                if !continues {
                    flush(&progress);
                }
                run_log_at(label, value);
                progress.at(value);
            }
            Event::Output {
                value: printed,
                text,
            } => {
                let at = printed.clone().unwrap_or_default();
                let (show, _opened) = run_log_output(label, &at, text);
                if show {
                    progress.above(|| errln!("{text}"));
                }
            }
            Event::Log {
                level,
                value: logged,
                message,
            } => {
                let at = logged.clone().unwrap_or_default();
                let _ = run_log_record(label, &at, level, message);
                // Under `--show-output`, what the model logs is shown as what
                // it prints is: its `logging` lines went to the run log alone.
                let shown = run_log().as_ref().is_some_and(|log| log.show);
                if shown {
                    progress.above(|| {
                        for line in message.lines() {
                            errln!("{level} {line}");
                        }
                    });
                } else if matches!(level.as_str(), "WARNING" | "ERROR" | "CRITICAL") {
                    let first = message.lines().next().unwrap_or_default();
                    let text = if at.is_empty() {
                        format!("{label}: {first}")
                    } else {
                        format!("{label} {at}: {first}")
                    };
                    // Said as the run ends, several as one line; the run log
                    // keeps each.
                    warn_gathered(
                        "the model logged",
                        |count| {
                            format!("the model logged {count} warnings, which the run log keeps")
                        },
                        text,
                    );
                }
            }
            Event::Finished {
                value,
                seconds,
                before,
                after,
                cells,
            } => {
                // Finished again by the fresh worker that took the sample over
                // after a --timeout: said once, counted once, its last value
                // kept.
                let from = finished_from.min(totals.changes.len());
                if let Some(at) = totals.changes[from..]
                    .iter()
                    .position(|change| change[1] == *value)
                {
                    totals.changes[from + at][3] = after.clone();
                    return;
                }
                totals
                    .changes
                    .push([label.clone(), value.clone(), before.clone(), after.clone()]);
                totals.change_cells.push(*cells);
                progress.finished();
                match value.split_once('.') {
                    Some((table, column)) => {
                        let same = group
                            .borrow()
                            .as_ref()
                            .is_some_and(|(current, _, _)| current == table);
                        if same {
                            if let Some((_, columns, took)) = group.borrow_mut().as_mut() {
                                columns.push(column.to_string());
                                *took += seconds;
                            }
                        } else {
                            flush(&progress);
                            *group.borrow_mut() =
                                Some((table.to_string(), vec![column.to_string()], *seconds));
                        }
                    }
                    None => {
                        flush(&progress);
                        let line = finished_line(label, width, value, *seconds);
                        progress.above(|| outln!("{line}"));
                    }
                }
            }
            // Not run, and no failure: what it reads is not there.
            Event::Waiting {
                value,
                inputs,
                failed,
            } => {
                flush(&progress);
                progress.finished();
                // What is to be filled in: an input that does not itself
                // wait, or a value waiting for its own — never one that failed,
                // whose own line says why.
                for input in inputs {
                    if !failed.contains(input) && !totals.waited_for.contains(input) {
                        totals.waited_for.push(input.clone());
                    }
                }
                totals.waiting_values.push(value.clone());
                if inputs.contains(value) {
                    totals.own.push(value.clone());
                }
                let named: Vec<String> = inputs
                    .iter()
                    .map(|input| {
                        if failed.contains(input) {
                            format!("{input}, which failed")
                        } else {
                            input.clone()
                        }
                    })
                    .collect();
                let said = if inputs.as_slice() == [value.clone()] {
                    WAITS_FOR_ITSELF.to_string()
                } else {
                    format!("waits for {}", named.join("; "))
                };
                progress.above(|| outln!("  · {label:<width$}  {value:<24}  {said}"));
            }
            Event::Failed { value, traceback } => {
                flush(&progress);
                progress.finished();
                // The error's line, not the note a failure carries after it.
                let last = samplekit::presentation::changes::error_line(traceback);
                // Said once per traceback, whole: two failures ending on one
                // line can differ above it, and were collapsed.
                let whole = traceback.trim_end().to_string();
                let first = totals.tracebacks.insert(whole.clone());
                let whose = totals
                    .traced_by
                    .entry(whole)
                    .or_insert_with(|| format!("{label} {value}"))
                    .clone();
                // What it wrote arrived before the failure, on the protocol.
                let recent = run_log_recent();
                progress.above(|| {
                    outln!("  ✗ {label:<width$}  {value:<24}  {last}");
                    if first {
                        errp!("{traceback}");
                    } else {
                        errln!("{label} {value}: {last} — the same error as {whose}'s above");
                    }
                    if !recent.is_empty() {
                        errln!("  the last lines {value} printed:");
                        for line in &recent {
                            errln!("    {line}");
                        }
                    }
                });
            }
        };
        let before = flags.write.then(|| std::fs::read(&request.sample).ok());
        let computed_path = request.sample.clone();
        // A formula past `--timeout` that did not stop takes its worker down
        // with it: a fresh one takes the same sample again, that value failed
        // `TimedOut` and never run again, and the others computed. Its
        // failure is said by the fresh worker, as any failure is.
        let mut asked = request.clone();
        let mut resumed = false;
        let outcome = loop {
            let outcome = worker.compute(&asked, flags.write, &mut on);
            match &outcome {
                Err(ModelError::TimedOut {
                    value: Some(value), ..
                }) if !asked.overran.contains(value) => {
                    let fresh = Worker::start(&python).map_err(model_failure)?;
                    if let Ok(mut pids) = WORKER_PIDS.lock() {
                        pids.push(fresh.id());
                    }
                    *worker = fresh;
                    asked.overran.push(value.clone());
                    resumed = true;
                }
                _ => break outcome,
            }
        };
        // What this sample's run wrote, outside the range its project declares:
        // written, and said.
        if flags.write && outcome.is_ok() {
            let names: Vec<String> = totals.changes[finished_from.min(totals.changes.len())..]
                .iter()
                .map(|[_, value, _, _]| value.clone())
                .collect();
            let config = totals
                .configurations
                .get(&request.sample)
                .cloned()
                .flatten();
            warn_outside_ranges(&request.sample, config.as_ref(), label, &names);
        }
        if let Some(before) = before
            && std::fs::read(&request.sample).ok() != before
        {
            totals.written += 1;
            totals.written_paths.push(relative_to_here(&request.sample));
        }
        flush(&progress);
        match outcome {
            Ok(report) => {
                // Resumed after a worker stopped, what the stopped one finished
                // counts too: each finished value was said, and written.
                totals.computed += if resumed {
                    report
                        .computed
                        .max(totals.changes.len().saturating_sub(finished_before))
                } else {
                    report.computed
                };
                totals.pending += report.pending;
                totals.waiting += report.waiting;
                // Which formula computed what was written.
                if flags.write {
                    totals.tracing.wrote(
                        totals
                            .configurations
                            .get(&request.sample)
                            .and_then(Option::as_ref),
                        totals
                            .before
                            .get(&request.sample)
                            .map_or(&[][..], Vec::as_slice),
                        &request.sample,
                        &report,
                    );
                }
                if !flags.write && report.computed > 0 {
                    totals
                        .tried
                        .push((position, request.sample.clone(), request.template.clone()));
                }
                totals.failed += report.failed;
                if report.computed + report.failed > 0 {
                    totals.samples += 1;
                }
            }
            Err(ModelError::UnknownValue { message }) => {
                totals.unknown += 1;
                totals
                    .unknown_messages
                    .push(unknown_value_line(label, &message));
            }
            Err(ModelError::NotSaved { message }) => {
                // Said in the command line's words: Python's
                // `save(overwrite=True)` is no command here.
                let why = message
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split(": save(overwrite=True)")
                    .next()
                    .unwrap_or_default()
                    .to_string();
                progress.above(|| outln!("  ✗ {label:<width$}  not saved: {why}"));
                totals.not_saved.push(shell_path(&computed_path));
                totals.samples += 1;
            }
            Err(ModelError::TemplateFailed { traceback }) => {
                let last = traceback
                    .trim_end()
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .to_string();
                let first = totals.tracebacks.insert(last);
                progress.above(|| {
                    outln!("  ✗ {label:<width$}  the model raised while loading it");
                    if first {
                        errp!("{traceback}");
                    }
                });
                totals.unloaded += 1;
                totals.failed += 1;
                totals.samples += 1;
            }
            // Stopped past `--timeout`: that value failed, and a new worker
            // takes the samples after it.
            Err(error @ ModelError::TimedOut { .. }) => {
                progress.above(|| outln!("  ✗ {label:<width$}  {error}"));
                totals.failed += 1;
                totals.samples += 1;
                let fresh = Worker::start(&python).map_err(model_failure)?;
                if let Ok(mut pids) = WORKER_PIDS.lock() {
                    pids.push(fresh.id());
                }
                *worker = fresh;
            }
            Err(other) => {
                // A worker interrupted with the command is not a failure to report;
                // what it finished was saved as it finished.
                if interrupted() {
                    let kept = totals.changes.len() - finished_before;
                    totals.computed += kept;
                    if kept > 0 {
                        totals.samples += 1;
                    }
                    break;
                }
                progress.finish();
                return Err(model_failure(other));
            }
        }
    }
    progress.finish();
    Ok(())
}

/// Which exit code a model's failure is: the machine (3), the invocation (1),
/// or the data and the code (2).
pub(super) fn model_failure(error: ModelError) -> Fail {
    match error {
        ModelError::NoInterpreter { .. }
        | ModelError::NotInstalled { .. }
        | ModelError::VersionMismatch { .. }
        | ModelError::NotRecorded { .. } => Fail::io(error.to_string()),
        ModelError::UnknownValue { .. } => Fail::usage(error.to_string()),
        _ => Fail::data(error.to_string()),
    }
}

pub(super) fn bar() -> std::sync::MutexGuard<'static, Option<Bar>> {
    BAR.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn run_log() -> std::sync::MutexGuard<'static, Option<RunLog>> {
    RUN_LOG
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A line the model wrote, as the worker sent it with its value: kept in the
/// run's log; says whether it is to be shown, and the log's path when this line
/// opened it.
pub(super) fn run_log_output(label: &str, value: &str, text: &str) -> (bool, Option<PathBuf>) {
    match run_log().as_mut() {
        Some(log) => {
            let opened = log.write(label, value, text);
            (log.show, opened)
        }
        None => (true, None),
    }
}

/// A line written to the worker's stderr itself — a C library, a subprocess —
/// kept against the value in progress, and shown above the bar only under
/// `--show-output`.
pub(super) fn relay_model_output(line: &str) {
    // After Ctrl-C, what reaches stderr is the interpreter stopping — an
    // "Exception ignored while flushing" — not the model's output, and was
    // counted as lines it printed.
    if interrupted() {
        return;
    }
    let (show, opened) = match run_log().as_mut() {
        Some(log) => {
            let (label, value) = (log.label.clone(), log.value.clone());
            let opened = log.write(&label, &value, line);
            (log.show, opened)
        }
        None => (true, None),
    };
    if let Some(path) = opened {
        let _ = (&path, show);
    }
    if show {
        relay_above_bar(line);
    }
}

/// The value the model's next lines belong to.
pub(super) fn run_log_at(label: &str, value: &str) {
    if let Some(log) = run_log().as_mut() {
        log.label = label.to_string();
        log.value = value.to_string();
        log.recent.clear();
    }
}

/// What the value in progress printed last, when it was not already shown.
pub(super) fn run_log_recent() -> Vec<String> {
    run_log()
        .as_ref()
        .filter(|log| !log.show)
        .map(|log| log.recent.iter().cloned().collect())
        .unwrap_or_default()
}

/// A record the model logged, kept; the log's path and whether output is shown,
/// when this record opened it.
pub(super) fn run_log_record(
    label: &str,
    value: &str,
    level: &str,
    message: &str,
) -> Option<(PathBuf, bool)> {
    let mut held = run_log();
    let log = held.as_mut()?;
    let mut opened = None;
    for line in message.lines() {
        if let Some(path) = log.write(label, value, &format!("{level} {line}")) {
            opened = Some((path, log.show));
        }
    }
    opened
}

/// The closing line naming the run's log, when the model printed anything.
pub(super) fn say_run_log() {
    let held = run_log();
    let Some(log) = held.as_ref() else { return };
    let Some((path, _)) = &log.file else { return };
    if log.lines == 0 {
        return;
    }
    if log.show {
        outln!("the model's output is also kept in {}", path.display());
    } else {
        outln!(
            "the model printed {} — {} — --show-output shows them as they come",
            counted_as(log.lines, "line", "lines"),
            path.display()
        );
    }
}

/// A line the model printed, written above the bar rather than into it.
pub(super) fn relay_above_bar(line: &str) {
    let mut held = bar();
    match held.as_mut() {
        Some(bar) => {
            bar.erase();
            errln!("{line}");
            bar.draw();
        }
        None => errln!("{line}"),
    }
}

/// The width of the terminal stderr draws on: `$COLUMNS`, else what the
/// terminal reports, else 80.
pub(super) fn terminal_columns() -> usize {
    let exported = std::env::var("COLUMNS")
        .ok()
        .and_then(|written| written.parse::<usize>().ok());
    exported
        .filter(|columns| *columns > 20)
        .or_else(|| reported_columns().filter(|columns| *columns > 20))
        .unwrap_or(80)
}

/// The width the controlling terminal reports, as `stty size` reads it.
pub(super) fn reported_columns() -> Option<usize> {
    let tty = std::fs::File::open("/dev/tty").ok()?;
    let output = std::process::Command::new("stty")
        .arg("size")
        .stdin(tty)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .split_whitespace()
        .nth(1)?
        .parse::<usize>()
        .ok()
}

/// A value finished, or the columns of a table: a duration only past a second.
pub(super) fn finished_line(label: &str, width: usize, value: &str, seconds: f64) -> String {
    let took = if seconds >= 1.0 {
        duration(seconds)
    } else {
        String::new()
    };
    format!("  ✓ {label:<width$}  {value:<24}  {took}")
        .trim_end()
        .to_string()
}

pub(super) fn duration(seconds: f64) -> String {
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        let whole = seconds.round() as u64;
        format!("{}m{:02}s", whole / 60, whole % 60)
    }
}

/// What a run's waiting values wait for that is to be filled in: the inputs
/// that do not wait themselves, and the values waiting for their own.
fn to_fill(totals: &Totals) -> Vec<String> {
    totals
        .waited_for
        .iter()
        .filter(|input| !totals.waiting_values.contains(input) || totals.own.contains(input))
        .cloned()
        .collect()
}

/// A trace of the formulas not kept is a warning, as a history not kept is,
/// never the command's failure.
fn trace_failures(failures: Vec<(PathBuf, samplekit::config::version_control::VcsError)>) {
    for (root, error) in failures {
        warn(&format!(
            "the trace of the formulas in {} was not kept: {error}",
            root.display()
        ));
    }
}

/// The values a run wrote into `sample` that its project's `[property.*]
/// range` leaves out, each said as a warning.
fn warn_outside_ranges(
    sample: &Path,
    config: Option<&ProjectConfig>,
    label: &str,
    names: &[String],
) {
    if names.is_empty() || config.is_none() {
        return;
    }
    let Ok((read, _)) = samplekit::format::document::load_sample(sample) else {
        return;
    };
    for name in names {
        let Ok(id) = samplekit::core::identifier::Identifier::new(name) else {
            continue;
        };
        let Ok(handle) = read.property(&id) else {
            continue;
        };
        let said = handle.with(|property| {
            let unit = property.presentation().unit.clone();
            property.peek_value().and_then(|value| {
                samplekit::collection::validation::outside_range(
                    config,
                    name,
                    &value,
                    unit.as_deref(),
                )
            })
        });
        if let Some(said) = said {
            warn(&format!("{label}: {said}"));
        }
    }
}
