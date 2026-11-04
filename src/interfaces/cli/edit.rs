//! `new`, `set` and `tag`: the commands that change a sample, each previewing first.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::collection::editing;

/// It previews by default and needs `--write`, the shape every command that
/// writes shares.
pub(super) fn tag(options: &Options) -> Outcome {
    let (edit, names, rest) = match options.positionals.split_first() {
        Some((verb, rest)) if verb == "add" => (Edit::Add, 1, rest),
        Some((verb, rest)) if verb == "remove" => (Edit::Remove, 1, rest),
        Some((verb, rest)) if verb == "rename" => (Edit::Rename, 2, rest),
        _ => {
            return Err(Fail::usage(
                "tag wants add, remove or rename: samplekit tag add reference -f '…'".to_string(),
            ));
        }
    };
    if rest.len() < names {
        return Err(Fail::usage(match names {
            1 => "tag wants a tag's name".to_string(),
            _ => "tag rename wants two names: the old one and the new one".to_string(),
        }));
    }
    // **A tag that is not an identifier is refused before anything is
    // written**, naming the rule.
    let mut tags = Vec::new();
    for (position, written) in rest[..names].iter().enumerate() {
        match Identifier::new(written) {
            Ok(tag) => tags.push(tag),
            // An old tag its files hold unusable is what a rename repairs.
            Err(_)
                if matches!(edit, Edit::Rename)
                    && position == 0
                    && let Ok(new) = Identifier::new(&rest[1]) =>
            {
                return rename_unusable(options, written, &new, rest.get(names));
            }
            Err(_) if written.is_empty() => {
                return Err(Fail::usage("a tag's name cannot be empty".to_string()));
            }
            Err(error) => {
                return Err(Fail::usage(format!(
                    "'{written}' is not a usable tag: {}",
                    identifier_refused(&error)
                )));
            }
        }
    }
    // Nothing to do is an answer, not a mistake: it exits 0 and says the
    // collection is unchanged.
    if matches!(edit, Edit::Rename) && tags[0] == tags[1] {
        outln!("nothing to change — '{}' renamed to itself", tags[0]);
        return Ok(());
    }
    let root = rest
        .get(names)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    // Nothing selected is said as every command with nothing to change says
    // it, before a plan of no sample.
    if selected.is_empty() {
        outln!("nothing to change — no sample is selected");
        return Ok(());
    }

    // A tag to rename or remove that no sample carries leaves the collection
    // as it was: said *unchanged*, exit 0, since nothing was left to do
    //  — with the nearest tag that exists, so
    // that a typo in the repair of a typo is still caught by the eye.
    if matches!(edit, Edit::Rename | Edit::Remove)
        && !collection
            .iter()
            .any(|entry| entry.sample.borrow().has_tag(&tags[0]))
    {
        outln!("nothing to change — no sample carries '{}'", tags[0]);
        if let Some(suggestion) = collection.vocabulary().nearest_tag(tags[0].as_str()) {
            outln!("  did you mean: '{suggestion}'?");
        }
        return Ok(());
    }
    // A tag that differs from one in use only by case is a second spelling of
    // it in the making: warned, and still done, since case may be meant. The
    // project's tags, not the target's: a file named alone holds only its own.
    if matches!(edit, Edit::Add | Edit::Rename) {
        let new = &tags[names - 1];
        let beside = if root.is_dir() {
            root.clone()
        } else {
            root.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
        };
        let project = project_samples(&beside).0;
        let in_use = project.as_ref().unwrap_or(&collection).vocabulary();
        if let Some(existing) = in_use
            .tags()
            .iter()
            // Renaming `Impure` to `impure` is how the second spelling
            // goes: the tag renamed is not the one warned about.
            .find(|tag| {
                *tag != new
                    && !(matches!(edit, Edit::Rename) && **tag == tags[0])
                    && tag.as_str().eq_ignore_ascii_case(new.as_str())
            })
        {
            warn(&format!(
                "'{new}' differs from the tag '{existing}' only by case: they are two tags"
            ));
        }
    }

    // What each file would become, decided before anything is written — by
    // `tagging`, which the workbench asks too.
    let plan = tagging::plan(
        &selected,
        match edit {
            Edit::Add => tagging::Edit::Add(tags[0].clone()),
            Edit::Remove => tagging::Edit::Remove(tags[0].clone()),
            Edit::Rename => tagging::Edit::Rename {
                from: tags[0].clone(),
                to: tags[1].clone(),
            },
        },
    );
    // A sample by its path from here, as every preview names one.
    let named = |path: &Path| relative_to_here(path);
    let changes: Vec<(PathBuf, String, String)> = plan
        .changes
        .iter()
        .map(|change| {
            (
                change.path.clone(),
                named(&change.path),
                note_of(&change.before, &change.after, &tags, &edit),
            )
        })
        .collect();
    let unchanged: Vec<(PathBuf, String)> = plan
        .unchanged
        .iter()
        .map(|path| (path.clone(), named(path)))
        .collect();

    // What and where, as every preview's first line says it, before a write
    // and after it alike.
    // A tag in its colour, as the TUI draws it: an attribute's.
    let tag = |name: &Identifier| render::paint(name.as_str(), render::palette().attribute);
    let what = match &edit {
        Edit::Add => format!("+{}", tag(&tags[0])),
        Edit::Remove => format!("-{}", tag(&tags[0])),
        Edit::Rename => format!("{} → {}", tag(&tags[0]), tag(&tags[1])),
    };
    let show_plan = |_done: bool| {
        outln!(
            "tag {what} · {} of {}\n",
            changes.len(),
            counted_as(selected.len(), "sample", "samples")
        );
        let mut lines: Vec<samplekit::presentation::changes::FieldChange> = changes
            .iter()
            .map(|(_, name, what)| samplekit::presentation::changes::FieldChange::said(name, what))
            .collect();
        lines.extend(unchanged.iter().take(5).map(|(_, name)| {
            samplekit::presentation::changes::FieldChange::said(name, "unchanged")
        }));
        for line in samplekit::presentation::changes::change_lines(&lines) {
            outln!("{line}");
        }
        if unchanged.len() > 5 {
            outln!("  … {} more unchanged", unchanged.len() - 5);
        }
    };

    // **Renaming over a narrowed selection warns**, naming how many samples
    // outside it still carry the old tag: the operation did what was asked, and
    // the thing it was for did not happen.
    let warn_outside = || {
        if matches!(edit, Edit::Rename) {
            let outside = collection
                .iter()
                .filter(|entry| {
                    let inside = selected
                        .iter()
                        .any(|chosen| chosen.path == entry.path && entry.path.is_some());
                    !inside && entry.sample.borrow().has_tag(&tags[0])
                })
                .count();
            if outside > 0 {
                warn(&format!(
                    "'{}' is still carried by {} outside this selection",
                    tags[0],
                    counted_as(outside, "sample", "samples")
                ));
            }
        }
    };

    // A plan that changes no file has nothing for --write to apply, and
    // offering the flag promised a write that would not happen.
    if changes.is_empty() {
        show_plan(false);
        warn_outside();
        outln!("\nnothing to change — no sample of the selection would change");
        return Ok(());
    }
    let written_paths: Vec<PathBuf> = changes.iter().map(|(path, _, _)| path.clone()).collect();
    if !options.write {
        show_plan(false);
        super::say_changed_by_this_write(written_paths.iter().map(PathBuf::as_path));
        warn_outside();
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    let changed_too = super::changed_by_this_write(written_paths.iter().map(PathBuf::as_path));

    // Written one file at a time, each atomically: a failure halfway names the
    // files changed before it, and leaves the ones after it untouched.
    let stop = || super::STOP.load(std::sync::atomic::Ordering::SeqCst);
    let written: Vec<String> = match tagging::apply_until(&plan, stop) {
        Ok((paths, true)) => {
            let written: Vec<String> = paths.iter().map(|path| named(path)).collect();
            return Err(super::stopped(&written));
        }
        Ok((paths, false)) => paths.iter().map(|path| named(path)).collect(),
        Err(error) => {
            let before: Vec<String> = error.written.iter().map(|path| named(path)).collect();
            for name in &before {
                outln!("  changed {name}");
            }
            let mut said = of_file(&error.path, &error.reason);
            // Said only where there is something to name: an empty list after
            // a colon read as a sentence cut off.
            if !before.is_empty() {
                said.push_str(&format!(
                    "\n  {} changed before this one: {}",
                    counted_as(before.len(), "file was", "files were"),
                    before.join(", ")
                ));
            }
            // Exit 3 where the filesystem refused, as `set` does.
            return Err(if error.filesystem {
                Fail::io(said)
            } else {
                Fail::data(said)
            });
        }
    };
    show_plan(true);
    if !changed_too.is_empty() {
        outln!("\n  what else this write changed:");
        for line in &changed_too {
            outln!(
                "    {}",
                line.replace("this write removes", "removed")
                    .replace("this write drops", "dropped")
            );
        }
    }
    warn_outside();
    outln!();
    for line in super::written_lines(&written) {
        outln!("{line}");
    }
    Ok(())
}

/// Renames a tag its files hold that is not an identifier, which only a rename
/// repairs.
pub(super) fn rename_unusable(
    options: &Options,
    old: &str,
    new: &Identifier,
    target: Option<&String>,
) -> Outcome {
    let root = target
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    // Unchanged, as a tag no sample carries is, before a plan of nothing.
    if !collection.iter().any(|entry| {
        entry
            .sample
            .borrow()
            .unusable_tags()
            .iter()
            .any(|tag| tag == old)
    }) {
        outln!("nothing to change — no sample carries '{old}'");
        return Ok(());
    }
    let carrying: Vec<(PathBuf, String)> = selected
        .iter()
        .filter_map(|entry| {
            let path = entry.path.clone()?;
            let sample = entry.sample.borrow();
            let name = relative_to_here(&path);
            sample
                .unusable_tags()
                .iter()
                .any(|tag| tag == old)
                .then_some((path, name))
        })
        .collect();
    let show_plan = |_done: bool| {
        outln!(
            "tag {old} → {new} · {} of {}\n",
            carrying.len(),
            counted_as(selected.len(), "sample", "samples")
        );
        let lines: Vec<samplekit::presentation::changes::FieldChange> = carrying
            .iter()
            .map(|(_, name)| {
                samplekit::presentation::changes::FieldChange::said(name, format!("{old} → {new}"))
            })
            .collect();
        for line in samplekit::presentation::changes::change_lines(&lines) {
            outln!("{line}");
        }
    };
    if carrying.is_empty() {
        show_plan(false);
        outln!("\nnothing to change — no sample of the selection carries '{old}'");
        return Ok(());
    }
    if !options.write {
        show_plan(false);
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    let mut written = Vec::new();
    for (path, name) in &carrying {
        if super::STOP.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(super::stopped(&written));
        }
        let result = document::load_sample(path)
            .map_err(|error| error.to_string())
            .and_then(|(mut sample, origin)| {
                let kept: Vec<String> = sample
                    .unusable_tags()
                    .iter()
                    .filter(|tag| *tag != old)
                    .cloned()
                    .collect();
                sample.set_unusable_tags(kept);
                sample.add_tag(new.clone());
                samplekit::collection::sample_list::declare_model_units_at(&mut sample, path);
                document::save_sample(&mut sample, &Destination::Origin(origin))
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(()) => written.push(name.clone()),
            Err(error) => {
                for name in &written {
                    outln!("  changed {name}");
                }
                return Err(Fail::data(of_file(path, &error.to_string())));
            }
        }
    }
    show_plan(true);
    outln!();
    for line in super::written_lines(&written) {
        outln!("{line}");
    }
    Ok(())
}

/// One file, re-read and written back through its origin.
pub(super) fn note_of(
    before: &[Identifier],
    after: &[Identifier],
    tags: &[Identifier],
    edit: &Edit,
) -> String {
    let tag = |name: &Identifier| render::paint(name.as_str(), render::palette().attribute);
    match edit {
        Edit::Add => format!("+{}", tag(&tags[0])),
        Edit::Remove => format!("-{}", tag(&tags[0])),
        Edit::Rename if before.contains(&tags[1]) && after.len() < before.len() => {
            format!(
                "{} → {}; already had it, the duplicate is dropped",
                tag(&tags[0]),
                tag(&tags[1])
            )
        }
        Edit::Rename => format!("{} → {}", tag(&tags[0]), tag(&tags[1])),
    }
}

/// The sample `--keep` names by its name or its file's stem, among those
/// where the new one goes: one, or refused naming those there are.
fn sample_named(written: &Path, into: Option<&Path>) -> Result<PathBuf, Fail> {
    let word = written.to_string_lossy();
    let root = into
        .filter(|into| into.is_dir())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let found: Vec<PathBuf> = super::loaded(&root)
        .map(|collection| {
            collection
                .iter()
                .filter(|entry| {
                    entry.sample.borrow().name() == Some(word.as_ref())
                        || entry
                            .path
                            .as_deref()
                            .and_then(Path::file_stem)
                            .is_some_and(|stem| stem.to_string_lossy() == word)
                })
                .filter_map(|entry| entry.path.clone())
                .collect()
        })
        .unwrap_or_default();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(Fail::io(format!(
            "{word}: no such file or directory, and no sample here is named so"
        ))),
        several => Err(Fail::usage(format!(
            "{} samples are named '{word}': name its file — {}",
            several.len(),
            several
                .iter()
                .map(|path| relative_to_here(path))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// A new sample, shaped by what the collection already holds — and never
/// carrying another sample's computed values or their records, which is what
/// copying a file by hand does wrong.
pub(super) fn new_sample(
    name: &str,
    keeping: Option<(PathBuf, Vec<String>)>,
    into: Option<&Path>,
    options: &Options,
) -> Outcome {
    let (like, keep): (Option<&Path>, &[String]) = match &keeping {
        Some((sample, fields)) => (Some(sample.as_path()), fields.as_slice()),
        None => (None, &[]),
    };
    // A name is something: an empty one wrote `.md`, a file every scan skips
    // as hidden, and a blank one a file named by a space.
    if name.trim().is_empty() {
        return Err(Fail::usage(
            "a sample's name cannot be empty: samplekit new <name>, its file <name>.md".to_string(),
        ));
    }
    // A name: where the file goes is --into's. `new cells/CC-11` wrote
    // cells/cells/CC-11.md, and a `..` would leave the collection.
    if let Some(refused) =
        editing::refuses_name(name, &samplekit::collection::sample_list::empty(), None)
    {
        // Where it goes is said only of a name that was a path.
        return Err(Fail::usage(if refused.message.contains("is a path") {
            format!("{} — --into DIRECTORY says where it goes", refused.message)
        } else {
            refused.message
        }));
    }
    let pattern_path = match like {
        Some(path) if path.exists() => Some(path.to_path_buf()),
        // A sample's name, as the advice writes `--keep <sample>`: the one
        // sample of the collection so named.
        Some(path) => Some(sample_named(path, into)?),
        // Without --keep, nothing is taken from anywhere: copying a
        // neighbour's attributes unasked is how another sample's identity
        // ends up in a new file.
        None => None,
    };

    let pattern = match &pattern_path {
        Some(path) => Some(
            document::load_sample(path)
                .map_err(|error| read_failed(path, &error))?
                .0,
        ),
        None => None,
    };
    let editing::Shaped {
        mut sample,
        measured,
        left_out,
        tables,
        attributes,
        not_carried,
    } = editing::shaped_like(name, pattern.as_ref(), keep)
        // What --keep names is the command's to correct, not the data's.
        .map_err(|error| Fail::usage(error.message))?;

    // A destination is a sample's file: `--into pieces/x.txt` previewed a file
    // no scan would ever read as a sample.
    if let Some(path) = into.filter(|path| !path.is_dir())
        && path.extension().is_some()
        && !is_markdown(path)
    {
        return Err(Fail::usage(format!(
            "{}: a sample's file ends in .md — --into names a folder, or a file such as {}",
            path.display(),
            path.with_file_name(format!("{name}.md")).display()
        )));
    }

    let destination = match into {
        Some(path) if path.is_dir() => path.join(format!("{name}.md")),
        // A destination whose directory is not there is named **once**, with
        // what to do about it. Left to the write, it surfaced as the operating
        // system's own words with the path repeated inside them.
        Some(path) if !path.exists() => {
            let missing = if path.extension().is_none() {
                path.to_path_buf()
            } else {
                path.parent().unwrap_or(Path::new(".")).to_path_buf()
            };
            if !missing.as_os_str().is_empty() && !missing.is_dir() {
                return Err(Fail::io(format!(
                    "{}: no such directory\n  create it first, or name one that exists",
                    missing.display()
                )));
            }
            path.to_path_buf()
        }
        Some(path) => path.to_path_buf(),
        // Beside the sample it was shaped from; failing that, in the project's
        // own samples/ rather than at its root, where nothing else lives.
        None => {
            let directory = pattern_path
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .or_else(|| {
                    let samples = Path::new("samples");
                    samples.is_dir().then(|| samples.to_path_buf())
                });
            // `.` joined with a name reads as ./CC-50.md — the current
            // directory said twice, where the name alone says it once.
            match directory {
                Some(directory) => directory.join(format!("{name}.md")),
                None => PathBuf::from(format!("{name}.md")),
            }
        }
    };
    // A name already taken is the command's to change, as any name refused
    // is: nothing on the disk failed.
    if destination.exists() {
        return Err(Fail::usage(format!(
            "{}: already here\n  samplekit {} set <field>=<value> changes what it holds",
            destination.display(),
            shell_path(&destination)
        )));
    }

    // What the project declares about a name, so that a quantity it already
    // knows is written as one rather than as an attribute: the project the file
    // goes into, which `--keep`'s sample alone decides when `--into` is not given. A
    // configuration that will not read is reported elsewhere and never stops a
    // write here.
    let declaring = samplekit::config::project_config::load_for(
        destination
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .ok()
    .flatten();

    // A name the project already holds, anywhere in it, would make two
    // samples no report can tell apart: looked for from the project's root,
    // not only beside the new file.
    let beside = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let (neighbours, root) = project_samples(beside);
    if let Some(neighbours) = &neighbours
        && let Some(refused) = editing::refuses_name(name, neighbours, root.as_deref())
    {
        return Err(Fail::usage(format!("{}: choose another", refused.message)));
    }

    // Values given on the command line, applied to what was just built.
    let mut filled: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut unvalued: Vec<String> = Vec::new();
    for written in &options.positionals {
        let (field_name, text) = split_assignment(written)?;
        // Readings, `radius.readings=1.05,1.06`, as `set` takes them: shown under
        // the quantity's own name, which is what is filled in.
        if editing::gives_readings(&field_name) {
            let mut lines = Vec::new();
            readings_change(
                &mut sample,
                &field_name,
                &text,
                declaring.as_ref(),
                neighbours.as_ref(),
                Said {
                    lines: &mut lines,
                    notes: &mut notes,
                    unvalued: &mut unvalued,
                },
            )?;
            let quantity = field_name
                .strip_suffix(".readings")
                .unwrap_or(&field_name)
                .to_string();
            let listed = editing::readings_of(&field_name, &text)
                .map(|readings| {
                    readings
                        .as_slice()
                        .iter()
                        .map(|reading| {
                            number_declared(*reading, &quantity, declaring.as_ref(), false)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            filled.push(format!("{quantity} = readings [{listed}]"));
            continue;
        }
        // A list, `carbon=[a, b]`, as `set` writes one. A name the project
        // declares a quantity takes none, as `set` refuses it: its repeated
        // measurements are its readings.
        if let Some(items) = editing::list_typed(&text) {
            let items = items.map_err(edit_failure)?;
            let declared_quantity = measured.contains(&field_name)
                || declaring
                    .as_ref()
                    .is_some_and(|config| config.property(&field_name).is_some())
                || declaring
                    .as_ref()
                    .and_then(samplekit::config::model_runtime::current_description)
                    .is_some_and(|description| description.properties.contains_key(&field_name));
            if declared_quantity {
                let shown: Vec<String> = items.iter().map(editing::shown_value).collect();
                return Err(Fail::usage(format!(
                    "{field_name} is a property, and a value is one number\n  repeated \
                     measurements of one property are its readings: {field_name}.readings={}",
                    shown.join(",")
                )));
            }
            editing::assign_list(&mut sample, &field_name, items).map_err(edit_failure)?;
            filled.push(field_name.to_string());
            continue;
        }
        let value = typed(&field_name, &text)?;
        // The number is shown back, so that a preview can be read over before
        // it is written — as the project writes it.
        let shown = editing::previewed_typed(&value, &field_name, declaring.as_ref());
        let (_, what) = apply_known(
            &mut sample,
            &field_name,
            value,
            declaring.as_ref(),
            neighbours.as_ref(),
        )?;
        filled.push(format!("{what} = {shown}"));
    }

    outln!("{} · new sample", relative_to_here(&destination));
    // The sections below are set off from the heading by one blank line, and
    // only where there is one: `new inconel` alone printed two blank lines
    // round nothing.
    let opened = std::cell::Cell::new(false);
    let open = || {
        if !opened.replace(true) {
            outln!();
        }
    };
    // A value given on the command line is filled in, not waiting to be: it is
    // said once, on the line that is true.
    let given = |name: &str| {
        filled
            .iter()
            .any(|written| written.split(' ').next() == Some(name))
    };
    // Neither what is filled in now nor what `--keep` copied is waiting: one
    // name on two lines of one preview contradicted itself.
    let waiting: Vec<&String> = measured
        .iter()
        .filter(|name| !given(name) && !keep.iter().any(|kept| kept == name.as_str()))
        .collect();
    let left_out: Vec<String> = left_out.into_iter().filter(|name| !given(name)).collect();
    if !waiting.is_empty() {
        open();
        outln!(
            "  measured, for you to fill in   {}",
            waiting
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        // An empty quantity is written only for what it holds, its unit: one
        // with none has no line in the file until a value is set. The preview
        // listed `radius` to fill in, and the file then had no `radius` anywhere.
        let unwritten: Vec<&str> = waiting
            .iter()
            .map(|name| name.as_str())
            .filter(|name| !written_in(&sample, name))
            .collect();
        if !unwritten.is_empty() {
            outln!(
                "    {} {} no unit, so the file holds no line for {} until samplekit set writes a value",
                unwritten.join(", "),
                if unwritten.len() == 1 { "has" } else { "have" },
                if unwritten.len() == 1 { "it" } else { "them" }
            );
        }
    }
    if !filled.is_empty() {
        // What is given now, as every change is said: from nothing, a number
        // typed with every digit it was typed with.
        open();
        let changes: Vec<samplekit::presentation::changes::FieldChange> = filled
            .iter()
            .map(|written| {
                // What the name was taken as — `new, declared in .samplekitrc`
                // — said beneath it.
                // Every field of a new sample is new to it: only how it was
                // taken is said, a quantity `declared in .samplekitrc`.
                let aside = written
                    .split_once(" (")
                    .and_then(|(_, rest)| rest.split_once(')'))
                    .map(|(aside, _)| aside.trim_start_matches("new").trim_start_matches(", "))
                    .filter(|aside| !aside.is_empty())
                    .map(str::to_string);
                let named = written
                    .split([' ', '('])
                    .next()
                    .unwrap_or(written)
                    .to_string();
                // Readings given are said as the readings they are.
                let (field, typed) = if written.contains(" = readings ") {
                    (format!("{named}.readings"), String::new())
                } else {
                    let typed = written
                        .split_once(" = ")
                        .map_or_else(String::new, |(_, value)| value.to_string());
                    (named, typed)
                };
                let mut change =
                    samplekit::presentation::changes::FieldChange::new(field, "—", typed);
                change.notes.extend(aside);
                change
            })
            .collect();
        let changes = samplekit::presentation::changes::made_whole(
            changes,
            None,
            &sample,
            declaring.as_ref(),
        );
        for line in samplekit::presentation::changes::change_lines(&changes) {
            outln!("{line}");
        }
    }
    // What --keep took is said, an attribute with the value it carries: it
    // is another sample's, read before it is written.
    let mut kept: Vec<String> = measured
        .iter()
        .filter(|name| !given(name) && keep.iter().any(|kept| kept == name.as_str()))
        .cloned()
        .collect();
    kept.extend(
        attributes
            .iter()
            .filter(|(name, _)| !given(name))
            .map(|(name, shown)| format!("{name} = {shown}")),
    );
    if let (false, Some(path)) = (kept.is_empty(), &pattern_path) {
        open();
        outln!(
            "  taken from {}   {}",
            relative_to_here(path),
            kept.join(", ")
        );
    }
    // The pattern's dates and `status` are its own, as the workbench's `N`
    // says: one filled in on the command line is not said here.
    let own: Vec<&str> = not_carried
        .iter()
        .filter(|name| !given(name))
        .map(String::as_str)
        .collect();
    if !own.is_empty() {
        open();
        outln!(
            "  not carried                    {}: the new sample's own to write",
            own.join(", ")
        );
    }
    if !tables.is_empty() {
        // A table with no rows reaches no file: saying it was created would be
        // a promise the write does not keep.
        open();
        outln!("  tables, not carried over       {}", tables.join(", "));
        outln!(
            "    their rows are that sample's own measurements, and a table with no rows is not written:\n    \
             samplekit set … --add-row <table> starts one"
        );
    }
    if !left_out.is_empty() {
        if opened.get() {
            outln!();
        }
        open();
        outln!(
            "  left out, because the model computes them: {}",
            left_out.join(", ")
        );
    }
    // What readings given here come to.
    notes.extend(unvalued_note(&unvalued, declaring.as_ref()));
    for note in &notes {
        outln!("\n  {note}");
    }

    if !options.write {
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }

    sample.set_note(format!("# {name}\n"));
    // A file another writer made there since the name was checked is not
    // replaced.
    samplekit::collection::sample_list::declare_model_units_at(&mut sample, &destination);
    document::save_sample(&mut sample, &Destination::New(destination.clone()))
        .map_err(|error| write_failed(&destination, &error))?;
    let quoted = shell_path(&destination);
    // The model is offered only where there is one to run: a project with
    // none was sent to a command with nothing to compute.
    let model = declaring
        .as_ref()
        .is_some_and(|config| config.model().is_some());
    outln!(
        "\nwritten: {}\n\n  samplekit {quoted} set <field>=<value>   fill a value in{}",
        relative_to_here(&destination),
        if model {
            format!("\n  samplekit {quoted} compute --write        run the model on it")
        } else {
            String::new()
        }
    );
    Ok(())
}

/// Change values in one sample, saying what the change would make outdated before
/// anything is written.
pub(super) fn set_values(file: &Path, add_row: Option<&str>, options: &Options) -> Outcome {
    if options.positionals.is_empty() {
        // The example names the file given and, where it reads, one of its own
        // quantities: an example from another domain taught nothing here.
        let quantity = document::load_sample(file)
            .ok()
            .and_then(|(sample, _)| sample.property_names().first().map(|name| name.to_string()))
            .unwrap_or_else(|| "<field>".to_string());
        let shown = shell_path(file);
        return Err(Fail::usage(match add_row {
            Some(table) => format!(
                "--add-row wants the row's cells: samplekit {shown} set --add-row {table} <column>=<value> …"
            ),
            None => format!(
                "set wants what to change, as field=value: samplekit {shown} set {quantity}=<value>"
            ),
        }));
    }
    if !file.exists() {
        return Err(Fail::io(format!(
            "{}: no such file or directory",
            file.display()
        )));
    }
    if let Some(table) = add_row {
        return add_table_row(file, table, options);
    }
    let (mut sample, origin) =
        document::load_sample(file).map_err(|error| read_failed(file, &error))?;
    // What its model declares, read from its description: the two formulas
    // of a value and its uncertainty are counted apart.
    samplekit::collection::sample_list::declare_model_units_at(&mut sample, file);

    let before_standing = editing::not_current(&sample);
    let before_states: Vec<String> = before_standing
        .iter()
        .map(|entry| entry.name.clone())
        .collect();

    // As for `new`: a name this project declares is a quantity. `old.md` has an
    // empty parent, which names no directory: the current one.
    let declaring = samplekit::config::project_config::load_for(
        file.parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .ok()
    .flatten();

    let mut changes: Vec<String> = Vec::new();
    let mut changed = 0usize;
    // A field written twice in one command would keep the last and drop the
    // first without a word.
    let mut seen: Vec<String> = Vec::new();
    for written in &options.positionals {
        let (field_name, _) = split_assignment(written)?;
        // By the field it addresses, not as typed: `diameter` and `diameter.v` are one.
        let field_name = fields::parse(&field_name)
            .map(|field| fields::describe(&field))
            .unwrap_or(field_name);
        if seen.contains(&field_name) {
            return Err(Fail::usage(format!(
                "{field_name} is given twice: one value per field in one command"
            )));
        }
        seen.push(field_name);
    }
    // The project's samples, read only where a name this one does not hold
    // is given: it is written as they hold it.
    let unheld = options.positionals.iter().any(|written| {
        split_assignment(written).is_ok_and(|(field_name, _)| {
            matches!(
                fields::parse(&field_name),
                Ok(Field::Named { name, channel: Channel::Value | Channel::Readings })
                    if !sample.has_property(&name) && !sample.has_attribute(&name)
            )
        })
    });
    let neighbours = if unheld {
        project_samples(
            file.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .0
    } else {
        None
    };

    // A name another sample of the project holds is refused, as `new` refuses
    // it: a name identifies one sample.
    for written in &options.positionals {
        let Ok((field_name, text)) = split_assignment(written) else {
            continue;
        };
        let renamed = text.trim();
        if field_name != "name" || renamed.is_empty() || sample.name() == Some(renamed) {
            continue;
        }
        let (others, root) = project_samples(
            file.parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        );
        if let Some(others) = &others
            && let Some(refused) = editing::refuses_rename(renamed, others, root.as_deref(), file)
        {
            return Err(edit_failure(refused));
        }
    }

    // A name nothing here holds, one slip from one held: written as asked —
    // a new field is a new field — and said, where `ogg=` became an attribute
    // beside `radius` without a word.
    for written in &options.positionals {
        let Ok((field_name, _)) = split_assignment(written) else {
            continue;
        };
        let Ok(Field::Named { name, .. }) = fields::parse(&field_name) else {
            continue;
        };
        if sample.has_property(&name) || sample.has_attribute(&name) {
            continue;
        }
        let mut held: Vec<String> = sample
            .property_names()
            .iter()
            .chain(sample.attribute_names().iter())
            .map(|held| held.to_string())
            .collect();
        if let Some(neighbours) = &neighbours {
            held.extend(neighbours.available_fields().iter().map(fields::describe));
        }
        if let Some(description) = declaring
            .as_ref()
            .and_then(samplekit::config::model_runtime::current_description)
        {
            held.extend(description.properties.keys().cloned());
        }
        if held.iter().any(|field| field == name.as_str()) {
            continue;
        }
        // A short name is one letter from another of its length: `fq` for
        // `notch` was offered nothing, the rule for longer names asking more
        // letters than it has.
        let near =
            samplekit::core::identifier::nearest(name.as_str(), held.iter().map(String::as_str))
                .or_else(|| {
                    let written = name.as_str();
                    (written.chars().count() <= 3)
                        .then(|| {
                            held.iter().find(|field| {
                                field.chars().count() == written.chars().count()
                                    && field
                                        .chars()
                                        .zip(written.chars())
                                        .filter(|(a, b)| a != b)
                                        .count()
                                        == 1
                            })
                        })
                        .flatten()
                        .cloned()
                });
        if let Some(near) = near {
            warn(&format!(
                "no sample here holds '{name}', written as a new field — did you mean '{near}'?"
            ));
        }
    }

    // What renaming a unit leaves as it was, said after the change itself.
    let mut unit_readers = false;
    // What readings given here come to, said once below the changes.
    let mut notes: Vec<String> = Vec::new();
    let mut unvalued: Vec<String> = Vec::new();
    for written in &options.positionals {
        let mut relabelled: Vec<String> = Vec::new();
        let (field_name, text) = split_assignment(written)?;
        // Readings are a channel of their own, `radius.readings=1.05,1.06`, and
        // take a list where every other channel takes one value.
        if editing::gives_readings(&field_name) {
            if readings_change(
                &mut sample,
                &field_name,
                &text,
                declaring.as_ref(),
                neighbours.as_ref(),
                Said {
                    lines: &mut changes,
                    notes: &mut notes,
                    unvalued: &mut unvalued,
                },
            )? {
                changed += 1;
            }
            continue;
        }
        // A list, `authors=[ana, tom]`, for an attribute: it was refused as
        // readings, and no command wrote one.
        if let Some(items) = editing::list_typed(&text) {
            let items = items.map_err(edit_failure)?;
            let (before, after) =
                editing::assign_list(&mut sample, &field_name, items).map_err(edit_failure)?;
            if before == after {
                changes.push(format!("{field_name:<28} {after}  unchanged"));
            } else {
                changes.push(format!("{field_name:<28} {before}  →  {after}"));
                changed += 1;
            }
            continue;
        }
        let value = typed(&field_name, &text)?;
        // What a scalar replaces: readings a quantity holds stay, and an
        // uncertainty entered beside them stays with them.
        let (change, what) = apply_known(
            &mut sample,
            &field_name,
            value,
            declaring.as_ref(),
            neighbours.as_ref(),
        )?;
        // Every digit written, before and after: the preview is read over
        // before the write, and a rounded one hid what the write did.
        let config = declaring.as_ref();
        let shown = |value: &Option<Value>| {
            value.as_ref().map_or("—".to_string(), |value| {
                shown_declared(value, &field_name, config)
            })
        };
        let (mut before, mut after) = (
            shown(&change.before),
            change.after.as_ref().map_or("—".to_string(), |value| {
                editing::previewed_typed(value, &field_name, config)
            }),
        );
        // A blank value removes what was there: an edit, said as one, where `→
        // —` read as a value nobody had given yet.
        // An uncertainty emptied takes the uncertainty alone: the value stays,
        // said whole beneath, where `notch 1.012 ± 0.0010  →  removed` read as
        // the measurement lost.
        if change.kept.is_none()
            && change.became != editing::Became::Unchanged
            && !field_name.ends_with(".u")
            && change
                .after
                .as_ref()
                .is_none_or(|after| matches!(after, Value::Absent))
        {
            after = "removed".to_string();
        }
        // Two numbers the precision writes alike are still a change, and are
        // then shown whole: a preview must not read `1.021 → 1.021`.
        let whole = |value: &Option<Value>| value.as_ref().map_or("—".to_string(), shown_value);
        let alike = before == shown(&change.after)
            || before == after
            || samplekit::presentation::changes::exaggerated(
                &before,
                &after,
                &whole(&change.before),
                &whole(&change.after),
            );
        if alike && change.became != editing::Became::Unchanged {
            // With the fewest digits that tell them apart, never every one.
            (before, after) = samplekit::presentation::changes::apart(
                &whole(&change.before),
                &whole(&change.after),
            );
        }
        // The name keeps its column; what the change *means* goes on its own
        // line, where it does not push the numbers out of alignment.
        let (name, aside) = match what.split_once(" (") {
            Some((name, rest)) => (
                name.to_string(),
                Some(rest.trim_end_matches(')').to_string()),
            ),
            None => (what, None),
        };
        // A value already equal to what it is asked to become is not a change.
        if change.became == editing::Became::Unchanged {
            changes.push(format!("{name:<28} {after}  unchanged"));
            continue;
        }
        changed += 1;
        // A unit renamed over a number converts nothing: said, with what reads
        // that number and was computed from it under the other unit.
        if let Ok(fields::Field::Named {
            name: quantity,
            channel: fields::Channel::Unit,
        }) = fields::parse(&field_name)
            && let (Some(Value::Text(from)), Some(Value::Text(to))) =
                (&change.before, &change.after)
            && let Some(number) = sample
                .property(&quantity)
                .ok()
                .and_then(|handle| handle.peek(|property| property.peek_value()))
                .filter(|value| !value.is_absent())
        {
            let shown_number = shown_value(&number);
            let readers: Vec<String> = sample
                .property_names()
                .into_iter()
                .filter(|reader| {
                    sample.property(reader).is_ok_and(|handle| {
                        handle.records().computed.as_ref().is_some_and(|inputs| {
                            inputs.keys().any(|input| {
                                matches!(input, samplekit::core::property::InputName::Named(read) if *read == quantity)
                            })
                        })
                    })
                })
                .map(|reader| reader.to_string())
                .collect();
            relabelled.push(format!(
                "{:<28} renamed, not converted: {shown_number} {from} is now read as {shown_number} {to}",
                ""
            ));
            if !readers.is_empty() {
                relabelled.push(format!(
                    "{:<28} {} read the number {shown_number} and {} what {} gave — \
                     write {quantity} in {to} too, then compute",
                    "",
                    readers.join(", "),
                    if readers.len() == 1 { "keeps" } else { "keep" },
                    if readers.len() == 1 { "it" } else { "they" }
                ));
                unit_readers = true;
            }
        }
        // A value cleared beside readings leaves what their statistic gives,
        // which the value after the change already is — and, where no statistic
        // is recorded, no value at all: no mean stands in. Not applicable is a
        // value written, not one cleared.
        let emptied = change
            .after
            .as_ref()
            .is_none_or(|after| matches!(after, Value::Absent));
        let after = match &change.kept {
            Some(_) if emptied && !name.ends_with(".u") => {
                format!("{after} (no statistic of the readings is recorded)")
            }
            _ => after,
        };
        changes.push(format!("{name:<28} {before}  →  {after}"));
        if let Some(aside) = aside {
            changes.push(format!("{:<28} {aside}", ""));
        }
        changes.append(&mut relabelled);
        if let Some(kept) = &change.kept {
            // The quantity's name, not the path written: `notch.v=` offered
            // `notch.v.u=`, which is no field.
            let quantity = match fields::parse(&field_name) {
                Ok(fields::Field::Named { name, .. }) => name.to_string(),
                _ => name.clone(),
            };
            changes.push(format!(
                "{:<28} keeps {}: [{}]",
                "",
                counted_as(kept.readings.len(), "reading", "readings"),
                kept.readings
                    .iter()
                    .map(|reading| number_declared(*reading, &field_name, config, false))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            // **A trial, not a loss**. The readings stand, the value written
            // outranks the statistic they declare, and `--force` gives the
            // property back to it — where the file records one. Without a
            // record nothing says which number they stood for, and the way back
            // is to write it.
            if emptied {
                // The readings' own value again: nothing to give back.
            } else if kept.statistic {
                changes.push(format!(
                    "{:<28} the value you wrote outranks their statistic — \
                     samplekit {} compute --force returns it",
                    "",
                    shell_path(file)
                ));
            } else {
                // No mean is offered in its place: which statistic stands for
                // the readings is the model's to declare.
                changes.push(format!(
                    "{:<28} no statistic of them is recorded: nothing computes the value back — \
                     {quantity}= removes it, and the readings then give none until the model \
                     declares one",
                    ""
                ));
            }
            let not_applicable = change.after.as_ref().is_some_and(Value::is_not_applicable);
            if let Some(uncertainty) = kept.uncertainty.filter(|_| not_applicable) {
                // A value that does not apply has no spread.
                changes.push(format!(
                    "{:<28} the uncertainty {} goes: a value that does not apply has none",
                    "",
                    number_declared(uncertainty, &field_name, config, true)
                ));
            } else if let Some(uncertainty) = kept.uncertainty {
                changes.push(format!(
                    "{:<28} the uncertainty {} goes on following them — \
                     {quantity}.u=0.01 changes it, {quantity}.u= clears it",
                    "",
                    number_declared(uncertainty, &field_name, config, true)
                ));
            }
        }
    }

    // A value written outside the range the project declares is written, and
    // said: the range is a check, not a refusal.
    for written in &options.positionals {
        let Ok((field_name, _)) = split_assignment(written) else {
            continue;
        };
        let Ok(Field::Named { name, .. }) = fields::parse(&field_name) else {
            continue;
        };
        let Ok(handle) = sample.property(&name) else {
            continue;
        };
        let outside = handle.with(|property| {
            let unit = property.presentation().unit.clone();
            property.peek_value().and_then(|value| {
                samplekit::collection::validation::outside_range(
                    declaring.as_ref(),
                    name.as_str(),
                    &value,
                    unit.as_deref(),
                )
            })
        });
        if let Some(said) = outside {
            warn(&said);
        }
    }

    outln!(
        "{} · {}\n",
        file.display(),
        // What actually changes, not how many assignments were typed, and not
        // how many lines it takes to show them: an override adds a line of its
        // own below the value.
        if changed == 0 {
            "nothing to change".to_string()
        } else {
            counted_as(changed, "change", "changes")
        }
    );
    // Each change as every surface says it: the values whole, with their
    // uncertainty and unit, what they mean beneath.
    let original = document::load_sample(file).ok().map(|(sample, _)| sample);
    for line in relaid(&changes, original.as_ref(), &sample, declaring.as_ref()) {
        outln!("{line}");
    }
    notes.extend(unvalued_note(&unvalued, declaring.as_ref()));
    for note in &notes {
        outln!("\n  {note}");
    }
    if changed == 0 {
        // Of several samples, the verdict is said once, after them all.
        if !super::ASK_AFTER.load(AtomicOrdering::Relaxed) {
            outln!("\nnothing to change — every value is already what it was asked to be");
        }
        return Ok(());
    }

    // What this costs downstream, which is the question an editor cannot answer.
    let after_states = not_current(&sample);
    let newly: Vec<(String, Freshness, usize)> =
        editing::newly_not_current(&before_standing, &editing::not_current(&sample))
            .into_iter()
            .map(|entry| (entry.name, entry.state, entry.rows))
            .collect();
    // An override is what compute leaves alone: promising that it recomputes
    // them sent the reader to a command that kept the value.
    let way_back = |listed: Vec<&Freshness>| {
        let overridden = listed
            .iter()
            .filter(|state| matches!(state, Freshness::Edited | Freshness::RecordMissing))
            .count();
        match overridden {
            0 => format!(
                "samplekit {} compute --write recomputes them",
                shell_path(file)
            ),
            all if all == listed.len() => format!(
                "compute keeps a value typed over its formula — samplekit {} compute --force \
                 --write gives it back to its formula",
                shell_path(file)
            ),
            _ => format!(
                "samplekit {} compute --write recomputes the others; --force also gives a \
                 value typed over its formula back to it",
                shell_path(file)
            ),
        }
    };
    if newly.is_empty() {
        // *Nothing depends on this* and *what depends on this was already out
        // of date* are different facts. Saying the first when the second is
        // true is a lie told at the moment someone decides something.
        if unit_readers {
            // Said above, value by value: they rest on the number, which did
            // not move, and nothing marks them.
        } else if before_states.is_empty() {
            // What the model reads of what changed, never computed yet: *nothing
            // derived rests on this* was said of the `notch` that `modulus` waited for.
            let changed_fields: Vec<String> = options
                .positionals
                .iter()
                .filter_map(|written| split_assignment(written).ok())
                .map(|(field, _)| {
                    let field = field.split('[').next().unwrap_or(&field).to_string();
                    match field.rsplit_once('.') {
                        Some((base, "readings" | "u" | "v" | "unit")) => base.to_string(),
                        _ => field,
                    }
                })
                .collect();
            let readers: Vec<String> = declaring
                .as_ref()
                .and_then(samplekit::config::model_runtime::current_description)
                .map(|described| {
                    described
                        .properties
                        .iter()
                        .filter(|(_, property)| {
                            property.reads.iter().any(|read| {
                                changed_fields.iter().any(|field| {
                                    read == field || read.strip_prefix("row.") == Some(field)
                                })
                            })
                        })
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_default();
            // A model never read here says nothing of what rests on the
            // change: *nothing derived* was said all the same.
            let unread = declaring.as_ref().is_some_and(|config| {
                samplekit::config::model_runtime::template_of(config).is_some()
                    && samplekit::config::model_runtime::current_description(config).is_none()
            });
            if readers.is_empty() && unread {
                outln!(
                    "\n  what rests on this is not known here: the model could not be read — \
                     samplekit validate says why"
                );
            } else if readers.is_empty() {
                outln!();
                for line in samplekit::presentation::changes::not_current_lines(&[], 0) {
                    outln!("{line}");
                }
            } else {
                outln!(
                    "\n  {} {} it, never computed yet: {}",
                    readers.join(", "),
                    if readers.len() == 1 { "reads" } else { "read" },
                    advice(&format!(
                        "samplekit {} compute --write computes {}",
                        shell_path(file),
                        if readers.len() == 1 { "it" } else { "them" }
                    ))
                );
            }
        } else {
            outln!();
            for line in samplekit::presentation::changes::not_current_lines(
                &[],
                // By cells, as every count is.
                before_standing.iter().map(|entry| entry.rows.max(1)).sum(),
            ) {
                outln!("{line}");
            }
            let listed = after_states
                .iter()
                .filter(|(name, _, _)| before_states.contains(name))
                .map(|(_, state, _)| state)
                .collect();
            outln!("\n  {}", advice(&way_back(listed)));
        }
    } else {
        outln!();
        let said: Vec<(String, String)> = newly
            .iter()
            .map(|(name, state, rows)| (name.clone(), state_of(&sample, state, *rows)))
            .collect();
        for line in samplekit::presentation::changes::not_current_lines(&said, 0) {
            outln!("{line}");
        }
        let listed = newly.iter().map(|(_, state, _)| state).collect();
        outln!("\n  {}", advice(&way_back(listed)));
    }

    super::say_changed_by_this_write(std::iter::once(file));
    if !options.write {
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    samplekit::collection::sample_list::declare_model_units_at(&mut sample, file);
    document::save_sample(&mut sample, &Destination::Origin(origin))
        .map_err(|error| write_failed(file, &error))?;
    // Of several samples, what was written is said once, after them all.
    if !super::WRITTEN_AFTER.load(AtomicOrdering::Relaxed) {
        outln!("\nwritten: {}", file.display());
    }
    Ok(())
}

/// The preview's lines as they are built — a field in its column of 28, then
/// `before  →  after` or what became of it, and a note beneath with that
/// column blank — said as every surface says a change.
fn relaid(
    lines: &[String],
    before: Option<&samplekit::core::sample::Sample>,
    after: &samplekit::core::sample::Sample,
    config: Option<&ProjectConfig>,
) -> Vec<String> {
    use samplekit::presentation::changes::{FieldChange, change_lines, made_whole};
    let mut changes: Vec<FieldChange> = Vec::new();
    for line in lines {
        if line.starts_with(' ') {
            if let Some(last) = changes.last_mut() {
                last.notes.push(line.trim().to_string());
            }
            continue;
        }
        let (field, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
        let rest = rest.trim();
        changes.push(match rest.split_once("  →  ") {
            Some((before, after)) => FieldChange::new(field, before, after),
            None => FieldChange::said(field, rest),
        });
    }
    change_lines(&made_whole(changes, before, after, config))
}

/// Where what readings given in one command come to is said: lines of the
/// preview, notes below it, and the quantities they leave with no value,
/// which one note names together.
struct Said<'a> {
    lines: &'a mut Vec<String>,
    notes: &'a mut Vec<String>,
    unvalued: &'a mut Vec<String>,
}

/// A row's index values, as a cell's verdict is asked for by them.
fn row_index(
    sample: &samplekit::core::sample::Sample,
    table: &Identifier,
    row: &samplekit::core::table::RowAddress,
) -> Vec<Value> {
    sample
        .table(table)
        .ok()
        .and_then(|held| held.row(row).ok())
        .map(|view| view.index().into_iter().cloned().collect())
        .unwrap_or_default()
}

/// Readings with no statistic recorded and no value beside them, said once for
/// every quantity given so: by the statistic the model's description declares
/// where it declares one, and not guessed at where it is not known.
fn unvalued_note(names: &[String], config: Option<&ProjectConfig>) -> Option<String> {
    let description = config.and_then(runtime::current_description);
    let declared = |name: &String| {
        description
            .as_ref()
            .and_then(|description| description.properties.get(name.as_str()))
            .and_then(|property| property.value.statistic().map(str::to_string))
    };
    // One line for the quantities of one statistic: a line each read as
    // fragments, the second set apart from the first.
    let mut by_statistic: Vec<(String, Vec<&String>)> = Vec::new();
    for name in names {
        if let Some(statistic) = declared(name) {
            match by_statistic.iter_mut().find(|(held, _)| *held == statistic) {
                Some((_, names)) => names.push(name),
                None => by_statistic.push((statistic, vec![name])),
            }
        }
    }
    let mut said: Vec<String> = by_statistic
        .into_iter()
        .map(|(statistic, names)| {
            let listed = match names.as_slice() {
                [one] => one.to_string(),
                [first @ .., last] => format!(
                    "{} and {last}",
                    first
                        .iter()
                        .map(|name| name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                [] => String::new(),
            };
            format!(
                "{listed} {} for samplekit compute: the {statistic} of {} readings, as the \
                 model declares",
                if names.len() == 1 { "waits" } else { "wait" },
                if names.len() == 1 { "its" } else { "their" }
            )
        })
        .collect();
    let unknown: Vec<String> = names
        .iter()
        .filter(|name| declared(name).is_none())
        .cloned()
        .collect();
    said.extend(unknown_statistic_note(&unknown));
    (!said.is_empty()).then(|| said.join("\n"))
}

fn unknown_statistic_note(names: &[String]) -> Option<String> {
    let first = names.first()?;
    Some(format!(
        "{} no statistic recorded for {} readings: samplekit compute takes the one the model \
         declares, and where it declares none they give no value until one is written beside \
         them, {first}=<value>",
        match names {
            [one] => format!("{one} has"),
            _ => format!("{} have", names.join(", ")),
        },
        if names.len() == 1 { "its" } else { "their" }
    ))
}

/// The repeated measurements behind one quantity, or one cell: `radius.readings=…`,
/// `toughness.stiffness[40].readings=…`, with or without brackets. Said as lines of
/// the preview `set` prints, and whether they change anything.
///
/// Entering one reading where three were made does not fail: it reports an
/// uncertainty of zero, which is a wrong number told quietly. Both personas of
/// round three met exactly that, and neither was warned.
///
/// **No statistic is chosen here.** Readings stand for a value only through the
/// statistic the model declares, and choosing one on the author's behalf is the
/// kind of confident guess this project exists to refuse. So the readings are
/// written, and the preview says plainly what they give: the statistic the file
/// records for them, or no value until a model declares one.
fn readings_change(
    sample: &mut samplekit::core::sample::Sample,
    field_name: &str,
    list: &str,
    config: Option<&ProjectConfig>,
    collection: Option<&SampleList>,
    said: Said<'_>,
) -> Result<bool, Fail> {
    let Said {
        lines,
        notes,
        unvalued,
    } = said;
    let field = fields::parse(field_name).map_err(|error| Fail::usage(error.to_string()))?;
    let readings = editing::readings_of(field_name, list).map_err(edit_failure)?;
    // Numbers written with a decimal comma read as twice as many readings:
    // said, since four whole numbers can be four readings.
    if let Some(warning) = editing::decimal_commas(field_name, list) {
        warn_gathered("decimal commas", decimal_commas_counted, warning);
    }
    let shown_list = |at: &str| {
        readings
            .as_slice()
            .iter()
            .map(|reading| number_declared(*reading, at, config, false))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let count = readings.as_slice().len();
    match field {
        Field::Cell {
            table, column, row, ..
        } => {
            let at = format!("{table}.{column}");
            // A value the column's statistic gave, as its record says, is the
            // statistic's own and goes not current; any other is a hand's, and
            // stays over them.
            let recorded = samplekit::format::fingerprint::check_cell(
                sample,
                &table,
                &column,
                &row_index(sample, &table, &row),
            )
            .is_ok_and(|verdict| {
                !matches!(
                    verdict,
                    samplekit::format::fingerprint::Freshness::Edited
                        | samplekit::format::fingerprint::Freshness::Source
                )
            });
            let (before, written) = sample
                .table(&table)
                .ok()
                .and_then(|held| held.at(&row, &column).ok())
                .map(|cell| {
                    (
                        cell.readings().map(|held| held.as_slice().to_vec()),
                        match cell.readings() {
                            Some(_) => cell.written_value().cloned().filter(|_| !recorded),
                            None => cell.peek_value().filter(|value| !value.is_absent()),
                        },
                    )
                })
                .unwrap_or((None, None));
            if before.as_deref() == Some(readings.as_slice()) {
                lines.push(format!("{field_name:<28} {}  unchanged", shown_list(&at)));
                return Ok(false);
            }
            editing::set_cell_readings(sample, &table, &row, &column, readings.clone())
                .map_err(edit_failure)?;
            lines.push(format!(
                "{field_name:<28} {}: {}",
                counted_as(count, "reading", "readings"),
                shown_list(&at)
            ));
            if let Some(before) = before {
                lines.push(format!(
                    "{:<28} they replace the {} it held",
                    "",
                    counted_as(before.len(), "reading", "readings")
                ));
            }
            let statistics = sample
                .table(&table)
                .ok()
                .and_then(|held| held.column(&column).ok().map(|view| view.statistics()))
                .unwrap_or_default();
            if let Some(written) = written {
                lines.push(format!(
                    "{:<28} the value {} written stays beside them, and outranks their statistic",
                    "",
                    shown_declared(&written, &at, config)
                ));
            } else if recorded {
                lines.push(format!(
                    "{:<28} its value, their statistic, is not current until samplekit compute \
                     takes it again",
                    ""
                ));
            } else if let Some(location) = statistics.value {
                lines.push(format!(
                    "{:<28} its value is their {}, as its column declares",
                    "",
                    location.name().replace('_', " ")
                ));
            } else {
                notes.push(format!(
                    "{at} declares no statistic of its readings: they give the cell no value \
                     until the model's column declares one — sk.Property(value=sk.stats.mean)"
                ));
            }
            Ok(true)
        }
        Field::Named { name, .. } => {
            // As for a value: a name this project declares, its model declares,
            // or the samples here hold as a quantity, is written as one, with
            // its unit.
            describe_for(sample, name.as_str(), config);
            let unit = (!sample.has_property(&name))
                .then(|| {
                    editing::known_as(sample, name.as_str(), config, collection)
                        .and_then(|known| known.declaration.unit)
                })
                .flatten();
            if sample.has_attribute(&name) {
                return Err(Fail::usage(format!(
                    "'{name}' is an attribute here, and readings belong to a property: \
                     remove it first, {name}="
                )));
            }
            // What the readings replace is said: a value written, readings made
            // before, and an uncertainty entered beside them. A value and an
            // uncertainty their statistic gave are its own, not a hand's: only
            // a value written over them is named.
            let statistic = sample
                .property(&name)
                .is_ok_and(|handle| handle.records().computed.is_some());
            // Written over its statistic by a hand, as its record says: the
            // file's state, which the property alone cannot tell.
            let edited = matches!(
                samplekit::format::fingerprint::check_property(sample, &name),
                Ok(samplekit::format::fingerprint::Freshness::Edited)
            );
            let held = sample.property(&name).ok().map(|handle| {
                handle.peek(|property| {
                    (
                        property.peek_value(),
                        property
                            .written_value()
                            .filter(|_| !statistic || edited || property.is_edited())
                            .cloned(),
                        property.readings().map(|held| held.as_slice().to_vec()),
                        property.peek_uncertainty().flatten(),
                    )
                })
            });
            if held
                .as_ref()
                .and_then(|(_, _, before, _)| before.as_deref())
                == Some(readings.as_slice())
            {
                lines.push(format!(
                    "{:<28} {}  unchanged",
                    format!("{name}.readings"),
                    shown_list(name.as_str())
                ));
                return Ok(false);
            }
            if let Some(refused) = editing::readings_refused_by_model(config, name.as_str()) {
                return Err(edit_failure(refused));
            }
            editing::set_readings(sample, &name, readings.clone()).map_err(edit_failure)?;
            if let (Some(unit), Ok(handle)) = (unit, sample.property(&name)) {
                let mut presentation = handle.presentation();
                presentation.unit = Some(unit);
                handle.set_presentation(presentation);
            }
            // The statistic the file records for them, where it records one: a
            // median said as a mean was the preview's own guess.
            let location = sample.property(&name).ok().and_then(|handle| {
                handle.peek(samplekit::core::property::Property::declared_location)
            });
            let mut convention = sample.property(&name).ok().and_then(|handle| {
                handle
                    .records()
                    .statistics
                    .as_ref()
                    .and_then(|statistics| statistics.uncertainty)
            });
            let statistic_said = match location {
                Some(location) => {
                    let summary = samplekit::core::statistics::summarize(&readings);
                    let statistic = location.of(&summary);
                    // Known to the uncertainty the file records for them, the
                    // statistic is written to its place, as a table writes the
                    // value: *their mean 1.99133* said digits their standard
                    // error of 0.0015 denies.
                    let places = convention
                        .filter(|_| editing::declared_precision(name.as_str(), config).is_none())
                        .and_then(|convention| {
                            samplekit::core::uncertainty::from_readings(&readings, convention)
                        })
                        .and_then(|uncertainty| {
                            samplekit::core::formatting::uncertainty_places(uncertainty.magnitude())
                        })
                        .map(|(_, most)| most)
                        .filter(|most| *most >= 0);
                    format!(
                        ", their {} {}",
                        location.name().replace('_', " "),
                        match places {
                            Some(places) => {
                                samplekit::core::formatting::fixed(statistic, places as usize)
                            }
                            None => number_declared(statistic, name.as_str(), config, false),
                        }
                    )
                }
                None => String::new(),
            };
            lines.push(format!(
                "{:<28} {}: {}{statistic_said}",
                format!("{name}.readings"),
                counted_as(count, "reading", "readings"),
                shown_list(name.as_str()),
            ));
            let statistic_name = location.map_or_else(
                || "statistic".to_string(),
                |location| location.name().replace('_', " "),
            );
            if let Some((value, written, before, spread)) = held {
                // New readings replace the readings before and nothing written
                // : what stands beside them is said as staying — each a
                // sentence of its own, with its subject.
                if let Some(before) = &before {
                    lines.push(format!(
                        "{:<28} they replace the {} it held",
                        "",
                        counted_as(before.len(), "reading", "readings")
                    ));
                }
                let kept = match (&before, written, value.filter(|value| !value.is_absent())) {
                    (Some(_), Some(written), _) | (None, _, Some(written)) => Some(written),
                    _ => None,
                };
                if let Some(kept) = kept {
                    lines.push(format!(
                        "{:<28} the value {} written stays, and outranks their {statistic_name} — \
                         {name}= clears it",
                        "",
                        shown_declared(&kept, name.as_str(), config)
                    ));
                } else if statistic {
                    // The uncertainty the file records waits as the value
                    // does: one sentence for both.
                    match convention.take() {
                        Some(convention) => lines.push(format!(
                            "{:<28} its value and its uncertainty, their {statistic_name} and \
                             {}, wait for samplekit compute",
                            "",
                            convention.name().replace('_', " ")
                        )),
                        None => lines.push(format!(
                            "{:<28} its value, their {statistic_name}, is not current until \
                             samplekit compute takes it again",
                            ""
                        )),
                    }
                }
                if let Some(spread) = spread.filter(|_| !statistic) {
                    lines.push(format!(
                        "{:<28} the uncertainty {} entered stays",
                        "",
                        number_declared(spread.magnitude(), name.as_str(), config, true),
                    ));
                }
            }
            // Whether an uncertainty follows is said from what the file
            // records: a note saying *no convention* stood beneath readings
            // whose file named standard_error.
            if let Some(convention) = convention {
                notes.push(format!(
                    "{name}'s uncertainty is their {}, as its file records: samplekit compute \
                     takes it",
                    convention.name().replace('_', " ")
                ));
            }
            // **Readings with no statistic have no value**: said, with the two
            // ways a value comes of them.
            let valued = sample.property(&name).is_ok_and(|handle| {
                handle
                    .peek(|property| property.peek_value())
                    .is_some_and(|value| !value.is_absent())
            });
            if location.is_none() && !valued {
                unvalued.push(name.to_string());
            }
            Ok(true)
        }
        _ => Err(Fail::usage(format!(
            "{field_name}: readings belong to a property or a table's cell"
        ))),
    }
}

/// A new row in a table — a measurement taken, written down.
///
/// Every persona of both review rounds needed this and none could do it;
/// three of them wrote YAML by hand instead, which is the gesture the format
/// exists to spare them.
pub(super) fn add_table_row(file: &Path, table: &str, options: &Options) -> Outcome {
    let (mut sample, origin) =
        document::load_sample(file).map_err(|error| read_failed(file, &error))?;
    let mut cells = Vec::new();
    // A cell given readings, `stiffness.readings=1.05,1.06`, holds them as a
    // measurement does.
    let mut measured = Vec::new();
    for written in &options.positionals {
        let (column, text) = split_assignment(written)?;
        if let Some(column) = column.strip_suffix(".readings") {
            if let Some(warning) = editing::decimal_commas(&format!("{column}.readings"), &text) {
                warn_gathered("decimal commas", decimal_commas_counted, warning);
            }
            let readings =
                editing::readings_of(&format!("{column}.readings"), &text).map_err(edit_failure)?;
            measured.push((column.to_string(), readings));
            continue;
        }
        cells.push((column, value_of(&text)?));
    }
    // A table this sample holds none of takes the shape of one a sample of the
    // collection holds, which is what `new` already does. `nt.md` has an
    // empty parent, which names the current directory: read as a path, it
    // found no sibling, and the shape came from the model instead.
    let directory = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    // What is not current before the row, for what the row makes so.
    let before_standing = editing::not_current(&sample);
    let added = editing::add_row_with_readings(&mut sample, table, cells, measured, |name| {
        // The model's declaration of the table first: it says the columns,
        // where another sample's file was copied, whichever came first.
        samplekit::config::project_config::load_for(directory)
            .ok()
            .flatten()
            .and_then(|config| {
                describe_now(&config);
                editing::shape_from_model(&config, name)
            })
            .or_else(|| {
                loaded(directory)
                    .ok()
                    .and_then(|collection| editing::shape_from(&collection, file, name))
            })
    })
    .map_err(edit_failure)?;

    outln!("{} · a row in {table}\n", file.display());
    // Where the columns come from, said rather than done quietly: this file had
    // no such table a moment ago.
    if let Some(source) = &added.lent_from {
        if source
            .extension()
            .is_some_and(|extension| extension == "py")
        {
            outln!("  the table is created here, with the columns the model declares");
        } else {
            outln!(
                "  the table is created here, with the columns of {}",
                display_relative(source, Path::new("."))
            );
        }
    }
    let declaring = samplekit::config::project_config::load_for(directory)
        .ok()
        .flatten();
    // Each cell given, as every change is said: from nothing.
    let mut shown: Vec<samplekit::presentation::changes::FieldChange> = added
        .given
        .iter()
        .map(|(column, value, overriding)| {
            let mut change = samplekit::presentation::changes::FieldChange::new(
                format!("{table}.{column}"),
                "—",
                editing::previewed_typed(value, &format!("{table}.{column}"), declaring.as_ref()),
            );
            if *overriding {
                change
                    .notes
                    .push("a computed column, typed over its formula".to_string());
            }
            change
        })
        .collect();
    // Readings as they are written, each one, and the statistic their column
    // declares — or that it declares none, and they give no value.
    for (column, readings) in &added.readings {
        let at = format!("{table}.{column}");
        let statistic = sample
            .table(&Identifier::new(table).map_err(|error| Fail::usage(error.to_string()))?)
            .ok()
            .and_then(|held| held.column(column).ok().map(|view| view.statistics().value));
        let mut change = samplekit::presentation::changes::FieldChange::new(
            format!("{at}.readings"),
            "—",
            readings
                .iter()
                .map(|reading| number_declared(*reading, &at, declaring.as_ref(), false))
                .collect::<Vec<_>>()
                .join(", "),
        );
        change.notes.push(match statistic.flatten() {
            Some(location) => format!("its value their {}", location.name().replace('_', " ")),
            // The model is not read here: what it declares is taken when it
            // computes.
            None => "no statistic recorded for the column: no value until compute takes the \
                     model's"
                .to_string(),
        });
        shown.push(change);
    }
    for line in samplekit::presentation::changes::change_lines(&shown) {
        outln!("{line}");
    }
    let names = |columns: &[Identifier]| {
        columns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !added.left_absent.is_empty() {
        outln!("\n  left absent             {}", names(&added.left_absent));
    }
    if !added.by_the_model.is_empty() {
        outln!("  computed by the model   {}", names(&added.by_the_model));
    }
    // What the new row makes not current, as a cell's `set` says it: `rate`'s
    // other rows and `drop` went outdated unannounced.
    let newly: Vec<(String, samplekit::format::fingerprint::Freshness, usize)> =
        editing::newly_not_current(&before_standing, &editing::not_current(&sample))
            .into_iter()
            .map(|entry| (entry.name, entry.state, entry.rows))
            .collect();
    outln!();
    let said: Vec<(String, String)> = newly
        .iter()
        .map(|(name, state, rows)| (name.clone(), state_of(&sample, state, *rows)))
        .collect();
    for line in samplekit::presentation::changes::not_current_lines(&said, 0) {
        outln!("{line}");
    }

    super::say_changed_by_this_write(std::iter::once(file));
    if !options.write {
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    samplekit::collection::sample_list::declare_model_units_at(&mut sample, file);
    document::save_sample(&mut sample, &Destination::Origin(origin))
        .map_err(|error| write_failed(file, &error))?;
    outln!(
        "\nwritten: {}\n\n  samplekit {} compute --write   fills what the model derives from it",
        file.display(),
        shell_path(file)
    );
    Ok(())
}

/// A refusal of the editing module, as a usage error: the change asked was not
/// one this sample can take.
pub(super) fn edit_failure(error: editing::EditError) -> Fail {
    Fail::usage(error.message)
}

pub(super) fn split_assignment(written: &str) -> Result<(String, String), Fail> {
    editing::split_assignment(written).map_err(edit_failure)
}

pub(super) fn value_of(text: &str) -> Result<Value, Fail> {
    editing::value_of(text).map_err(edit_failure)
}

/// What was typed for a field: the name as its text.
pub(super) fn typed(field: &str, text: &str) -> Result<Value, Fail> {
    editing::typed(field, text).map_err(edit_failure)
}

pub(super) fn shown_value(value: &Value) -> String {
    editing::shown_value(value)
}

/// The samples of the project a folder belongs to, read quietly — what a new
/// name must not repeat and what a new value is written like — and the
/// project's root.
pub(super) fn project_samples(beside: &Path) -> (Option<SampleList>, Option<PathBuf>) {
    let project = samplekit::config::project_config::find(beside)
        .and_then(|rc| rc.parent().map(Path::to_path_buf));
    let Ok(neighbours) =
        samplekit::collection::sample_list::from_directory(project.as_deref().unwrap_or(beside))
    else {
        return (None, None);
    };
    let root = project.as_deref().and_then(|_| {
        neighbours
            .config()
            .map(|config| config.root().to_path_buf())
    });
    (Some(neighbours), root)
}

/// Has the model's description written again where it is missing or stale and a
/// name may be the model's alone — one the sample does not hold as a quantity
/// and `.samplekitrc` does not declare — so that `known_as` reads it: one
/// Python start, once per configuration, and a warning where the model cannot
/// be read.
pub(super) fn describe_for(
    sample: &samplekit::core::sample::Sample,
    field_name: &str,
    config: Option<&ProjectConfig>,
) {
    let Some(config) = config else {
        return;
    };
    let name = match fields::parse(field_name) {
        Ok(Field::Named {
            name,
            channel: Channel::Value | Channel::Readings,
        }) => name,
        _ => return,
    };
    if sample.has_property(&name) || config.property(name.as_str()).is_some() {
        return;
    }
    describe_now(config);
}

/// Has the model's description written again where it is missing or stale: one
/// Python start, once per configuration, and a warning where the model cannot
/// be read.
pub(super) fn describe_now(config: &ProjectConfig) {
    thread_local! {
        static TRIED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    }
    let root = config.root().to_path_buf();
    let first = TRIED.with(|tried| {
        let mut tried = tried.borrow_mut();
        let first = !tried.contains(&root);
        if first {
            tried.push(root.clone());
        }
        first
    });
    if !first {
        return;
    }
    if let Err(said) = samplekit::collection::computation::describe_if_missing(config) {
        // Whole: its first line alone was `Traceback (most recent call
        // last):`, which says nothing of what went wrong.
        let trace = said
            .trim_end()
            .lines()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        warn(&format!(
            "the model could not be read, so what it declares is not known, and a name only \
             the model declares is written as the samples here hold it:\n{trace}"
        ));
    }
}

/// One assignment to a sample that may not hold its name yet, written as the
/// project knows it (`editing::known_as`): as `[property.*]` declares it, else
/// as the model's description says it declares it, else as the collection's
/// samples hold it — a quantity, with its unit. `radius=1.048` for a new piece
/// beside pieces that all measure `radius` was written as an attribute, and so was
/// `hb=25.1` where only the model declared it; and a text the model's
/// property holds is a property too.
pub(super) fn apply_known(
    sample: &mut samplekit::core::sample::Sample,
    field_name: &str,
    value: Value,
    config: Option<&ProjectConfig>,
    collection: Option<&SampleList>,
) -> Result<(editing::Change, String), Fail> {
    // **A bare number is in the unit the project declares** — the unit its
    // `[property.*]` gives, else the model's: written into a file that holds
    // another, the number went in under the file's unit, with the old
    // uncertainty, in that unit, beside it. The unit is written again, the
    // old uncertainty removed, and the preview says both.
    let switched = match (fields::parse(field_name), &value) {
        (
            Ok(Field::Named {
                name,
                channel: Channel::Value,
            }),
            Value::Integer(_) | Value::Number(_),
        ) if sample.has_property(&name) => {
            let handle = sample.property(&name).ok();
            let written = handle
                .as_ref()
                .and_then(|handle| handle.presentation().unit);
            let declared = config
                .and_then(|config| config.property(name.as_str()))
                .and_then(|declaration| declaration.unit.clone())
                .or_else(|| {
                    config
                        .and_then(runtime::current_description)
                        .and_then(|description| {
                            description
                                .properties
                                .get(name.as_str())
                                .and_then(|property| property.unit.clone())
                        })
                });
            match (written, declared) {
                (Some(written), Some(declared)) if written != declared => {
                    let uncertainty = handle
                        .as_ref()
                        .and_then(|handle| handle.uncertainty().ok().flatten());
                    Some((name, written, declared, uncertainty))
                }
                _ => None,
            }
        }
        _ => None,
    };
    let (mut change, mut what) =
        apply_known_as_written(sample, field_name, value, config, collection)?;
    // Whether the model computes it is read from its description, had written
    // where it is missing, as a fresh copy has it.
    if matches!(
        change.became,
        editing::Became::Changed | editing::Became::NewQuantity
    ) && let Some(config) = config
    {
        describe_now(config);
    }
    editing::over_the_model(sample, &mut change, config);
    if change.became == editing::Became::Override {
        what = described(&change);
    }
    if let Some((name, written, declared, uncertainty)) = switched
        && let Ok(handle) = sample.property(&name)
    {
        let mut presentation = handle.presentation();
        presentation.unit = Some(declared.clone());
        handle.set_presentation(presentation);
        let mut said =
            format!("in {declared}, the unit the project declares — the file wrote {written}");
        if let Some(uncertainty) = uncertainty {
            handle.set_uncertainty(None);
            said.push_str(&format!(
                "; its uncertainty, {} {written}, is removed with the unit it was in",
                uncertainty.magnitude()
            ));
        }
        // Said as every note on a change is: in brackets after its field,
        // which the preview writes under it.
        what = match what.strip_suffix(')') {
            Some(noted) if noted.contains(" (") => format!("{noted}; {said})"),
            _ => format!("{what} ({said})"),
        };
    }
    Ok((change, what))
}

/// [`apply_known`], the value in whatever unit the file holds.
fn apply_known_as_written(
    sample: &mut samplekit::core::sample::Sample,
    field_name: &str,
    value: Value,
    config: Option<&ProjectConfig>,
    collection: Option<&SampleList>,
) -> Result<(editing::Change, String), Fail> {
    describe_for(sample, field_name, config);
    // A value the model no longer computes, emptied: removed, record and all —
    // made an override, it stayed a value of no formula for good.
    if matches!(value, Value::Absent)
        && let Some(config) = config
        && let Ok(name) = Identifier::new(field_name)
        && sample.has_property(&name)
        && samplekit::collection::computation::orphans(Some(config), config.root(), sample)
            .iter()
            .any(|orphan| orphan == field_name)
    {
        let before = sample
            .property(&name)
            .ok()
            .and_then(|handle| handle.value().ok());
        sample
            .remove_property(&name)
            .map_err(|error| Fail::data(error.to_string()))?;
        let change = editing::Change {
            field: field_name.to_string(),
            before,
            after: None,
            became: editing::Became::Cleared,
            kept: None,
        };
        let what = described(&change);
        return Ok((change, what));
    }
    let known = editing::known_as(sample, field_name, config, collection);
    if let Some(known) = &known
        && let Some(held) = &known.held
        // Text where every sample holds a number is refused as `set` refuses
        // it: `radius=1,050` was written as the text "1,050".
        && held.numeric
        && matches!(value, Value::Text(_) | Value::Boolean(_))
    {
        return Err(edit_failure(editing::not_a_number_for(
            field_name,
            known.declaration.unit.as_deref(),
            &value,
        )));
    }
    let change =
        editing::apply_known(sample, field_name, value, known.as_ref()).map_err(edit_failure)?;
    let Some(known) = known.filter(|_| change.became == editing::Became::NewQuantity) else {
        let what = described(&change);
        return Ok((change, what));
    };
    // Two units for one name: `.samplekitrc`'s is written, and said.
    if let Some((configured, modelled)) = &known.disagreement {
        let model = config
            .and_then(runtime::template_of)
            .map(|template| relative_to_here(template.path()))
            .unwrap_or_else(|| "the model".to_string());
        warn_gathered(
            "units disagree",
            |count| {
                format!(
                    "{count} names are in one unit by .samplekitrc and in another by the \
                     model: each is written in .samplekitrc's, and one of the two is wrong"
                )
            },
            format!(
                "'{}' is in {configured} by .samplekitrc and in {modelled} by {model}: it is \
                 written in {configured}, and one of the two is wrong",
                change.field
            ),
        );
    }
    let what = match known.from {
        editing::KnownFrom::Configuration => described(&change),
        editing::KnownFrom::Model(file) => format!(
            "{} (new, a property {} declares)",
            change.field,
            relative_to_here(&file)
        ),
        editing::KnownFrom::Samples => {
            if let Some(held) = known.held.filter(|held| !held.units.is_empty()) {
                warn_gathered(
                    "written without a unit",
                    |count| {
                        format!(
                            "{count} names are written without a unit, the samples here \
                             holding each in several: <name>.unit=<unit> gives one"
                        )
                    },
                    format!(
                        "'{}' is written in {} by the samples here, so it is written without \
                         a unit\n  {}.unit=<unit> gives it one",
                        change.field,
                        held.units.join(" and "),
                        change.field
                    ),
                );
            }
            format!(
                "{} (new, a property as the other samples hold it)",
                change.field
            )
        }
    };
    Ok((change, what))
}

/// What a change is, as a preview names it. Several readings that look like
/// decimal commas, said as one line.
fn decimal_commas_counted(count: usize) -> String {
    format!("{count} lists of readings look written with decimal commas")
}

pub(super) fn described(change: &editing::Change) -> String {
    let field = &change.field;
    match change.became {
        editing::Became::Override => format!("{field} (typed over its formula, which stays)"),
        editing::Became::NewQuantity => {
            format!("{field} (new, declared in .samplekitrc)")
        }
        // No name written: the file's stands for it.
        editing::Became::Cleared if field == "name" => {
            format!("{field} (none written: the file's name stands for it)")
        }
        editing::Became::Cleared => format!("{field} (cleared)"),
        editing::Became::NewAttribute { numeric } => {
            // A number is warned about rather than turned into a quantity: a
            // model declaring that name later collides with the attribute.
            if numeric {
                // The way out that works from here: a declared name is written
                // as a quantity by `set` and `new` alike. Said only where true
                // — neither `.samplekitrc` nor the model's source names it, and
                // no other sample holds it as a quantity, or it would have been
                // written as one.
                warn_gathered(
                    "a property nowhere here",
                    |count| {
                        format!(
                            "{count} names are a property nowhere here, so they are written as \
                             attributes: declare each in the model or as [property.<name>] in \
                             .samplekitrc to write it as a property"
                        )
                    },
                    format!(
                        "'{field}' is a property nowhere here — no sample holds it as one, and \
                         neither .samplekitrc nor the model declares it — so it is written as \
                         an attribute\n  a property cannot share its name with one: if it is a \
                         quantity, declare it in the model or as [property.{field}] in \
                         .samplekitrc and it is written as one"
                    ),
                );
            }
            format!("{field} (new)")
        }
        editing::Became::Changed | editing::Became::Unchanged => field.to_string(),
    }
}

/// A value as a preview shows it: at the precision the project declares for the
/// field, as a table shows it, and every digit it holds where none is declared
/// — `editing::previewed`, which the workbench shares.
pub(super) fn shown_declared(value: &Value, field: &str, config: Option<&ProjectConfig>) -> String {
    editing::previewed(value, field, config)
}

/// A number of a field — a reading, a mean, an uncertainty — as a preview
/// shows it, as [`shown_declared`]: an uncertainty at the uncertainty's
/// precision.
fn number_declared(
    number: f64,
    field: &str,
    config: Option<&ProjectConfig>,
    uncertainty: bool,
) -> String {
    let precision = match (editing::declared_precision(field, config), uncertainty) {
        (Some(precision), true) => Some(precision.of_uncertainty()),
        (Some(precision), false) => Some(precision),
        // An uncertainty with nothing declared, rounded up as a table writes it.
        (None, true) => Some(samplekit::core::formatting::Precision::general_spread()),
        (None, false) => None,
    };
    editing::shown_at(&Value::Number(number), precision)
}

/// Whether a quantity of a sample not yet written has a line in its file: an
/// empty one is written for its unit alone.
fn written_in(sample: &samplekit::core::sample::Sample, name: &str) -> bool {
    let Ok(schema) = samplekit::format::schema::from_sample(sample) else {
        return true;
    };
    let text = samplekit::format::canonicalization::write(&schema);
    let Some(properties) = text.split("\nproperties:\n").nth(1) else {
        return false;
    };
    properties
        .lines()
        .take_while(|line| line.starts_with(' '))
        .any(|line| {
            line.strip_prefix("  ")
                .and_then(|rest| rest.strip_prefix(name))
                .is_some_and(|rest| rest.starts_with(':'))
        })
}

/// `remove`: sample files deleted, previewed first. Their own files stay; the
/// history keeps the samples, and `restore` gives them back.
pub(super) fn remove(options: &Options) -> Outcome {
    let named = !options.positionals.is_empty();
    let narrowed = !options.filter.is_empty() || !options.query.is_empty();
    if !named && !narrowed {
        return Err(Fail::usage(
            "remove wants the samples to delete: name their files, or narrow a folder with \
             -f or --query"
                .to_string(),
        ));
    }
    let root = options
        .positionals
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    // A folder is every sample in it: named alone, it is refused, as a typo
    // of a file's name would have deleted them all.
    let folders: Vec<PathBuf> = std::iter::once(root.clone())
        .chain(EXTRA_TARGETS.with(|extra| extra.borrow().clone()))
        .filter(|path| path.is_dir())
        .collect();
    if let Some(folder) = folders.first()
        && named
        && !narrowed
    {
        return Err(Fail::usage(format!(
            "{} is a folder, and remove deletes no folder whole: name the files, or narrow \
             it with -f or --query",
            relative_to_here(folder)
        )));
    }
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;
    let paths: Vec<PathBuf> = selected
        .iter()
        .filter_map(|entry| entry.path.clone())
        .collect();
    if paths.is_empty() {
        outln!("nothing to change — no sample is selected");
        return Ok(());
    }
    let own: usize = paths
        .iter()
        .filter_map(|path| {
            let config = collection.config_for(path)?;
            samplekit::config::discovery::sample_files(config, path, None).ok()
        })
        .map(|files| files.len())
        .sum();
    let them = paths.len() == 1;
    // The links of the project leading to them are left as written, and lead
    // nowhere once they are gone: said, file by file.
    let incoming = samplekit::collection::renaming::links_leading_to(
        paths
            .first()
            .and_then(|path| collection.config_for(path))
            .or(collection.config()),
        &paths,
    );
    let show = || {
        for path in &paths {
            outln!("  {}", relative_to_here(path));
        }
        outln!();
        if own > 0 {
            outln!(
                "their own files stay: {} under [collection] files",
                counted_as(own, "file", "files")
            );
        }
        if !incoming.is_empty() {
            outln!(
                "{} to {}, and will lead nowhere:",
                counted_as(incoming.len(), "link leads", "links lead"),
                if them { "it" } else { "them" }
            );
            let width = incoming
                .iter()
                .map(|link| link.field.chars().count())
                .max()
                .unwrap_or(0);
            let mut file: Option<&Path> = None;
            for link in &incoming {
                if file != Some(link.file.as_path()) {
                    outln!("  {}", relative_to_here(&link.file));
                    file = Some(link.file.as_path());
                }
                outln!("    {:width$}  {}", link.field, link.text);
            }
            outln!();
        }
    };
    if !options.write {
        outln!(
            "remove · {}\n",
            counted_as(paths.len(), "sample file", "sample files")
        );
        show();
        outln!(
            "samplekit restore gives {} back, as {} {}\n",
            if them { "it" } else { "them" },
            if them { "it" } else { "they" },
            if them { "was" } else { "were" }
        );
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    let mut removed: Vec<String> = Vec::new();
    for path in &paths {
        if super::STOP.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(super::stopped(&removed));
        }
        samplekit::format::document::note_written(path);
        if let Err(error) = std::fs::remove_file(path) {
            for name in &removed {
                outln!("removed: {name}");
            }
            return Err(Fail::io(format!("{}: {error}", relative_to_here(path))));
        }
        removed.push(relative_to_here(path));
    }
    for name in &removed {
        outln!("removed: {name}");
    }
    if own > 0 {
        outln!(
            "their own files stay: {} under [collection] files",
            counted_as(own, "file", "files")
        );
    }
    if !incoming.is_empty() {
        outln!(
            "{} now {} nowhere — samplekit validate lists {}",
            counted_as(incoming.len(), "link", "links"),
            if incoming.len() == 1 { "leads" } else { "lead" },
            if incoming.len() == 1 { "it" } else { "them" }
        );
    }
    outln!(
        "samplekit restore gives {} back",
        if them { "it" } else { "them" }
    );
    Ok(())
}

/// `rename`: a sample's file named again, its own files with it, its `name:`
/// where it was the file's name, and every link of its project leading to it;
/// previewed first.
pub(super) fn rename(options: &Options, new: &str) -> Outcome {
    let Some(target) = options.positionals.first().map(PathBuf::from) else {
        return Err(Fail::usage(
            "rename wants the sample to rename: samplekit FILE rename NEW".to_string(),
        ));
    };
    if EXTRA_TARGETS.with(|extra| !extra.borrow().is_empty()) {
        return Err(Fail::usage(
            "rename renames one sample: name one file".to_string(),
        ));
    }
    if target.is_dir() {
        return Err(Fail::usage(format!(
            "{} is a folder, and rename renames one sample: name its file",
            relative_to_here(&target)
        )));
    }
    let collection = loaded(&target)?;
    warn_skipped(&collection, options);
    let config = collection.config_for(&target).or(collection.config());
    let plan = samplekit::collection::renaming::plan(config, &target, new)
        .map_err(|error| Fail::usage(error.to_string()))?;
    // What it was and what it becomes, painted as every preview paints them.
    let was = |text: &str| render::paint(text, render::palette().attribute);
    let show = |plan: &samplekit::collection::renaming::Plan| {
        outln!(
            "  {:<12} {}  →  {}",
            "file",
            was(&relative_to_here(&plan.from)),
            was(&relative_to_here(&plan.to))
        );
        if let Some((before, after)) = &plan.name {
            outln!("  {:<12} {}  →  {}", "name", was(before), was(after));
        }
        for (from, to) in &plan.own {
            outln!(
                "  {:<12} {}  →  {}",
                "files",
                was(&relative_to_here(from)),
                was(&relative_to_here(to))
            );
        }
        outln!();
        if plan.links.is_empty() {
            outln!("  no link of the project leads to it");
        } else {
            outln!(
                "  {} leading to it, written again:",
                counted_as(plan.links.len(), "link", "links")
            );
            let mut files: Vec<&PathBuf> = plan.links.iter().map(|change| &change.file).collect();
            files.dedup();
            let width = plan
                .links
                .iter()
                .map(|change| change.field.chars().count() + change.item.map_or(0, |_| 4))
                .max()
                .unwrap_or(0)
                .max(12);
            for file in files {
                outln!("  {}", relative_to_here(file));
                for change in plan.links.iter().filter(|change| change.file == *file) {
                    let field = match change.item {
                        Some(at) => format!("{}[#{at}]", change.field),
                        None => change.field.clone(),
                    };
                    outln!(
                        "    {field:<width$}  {}  →  {}",
                        was(&change.before),
                        was(&change.after)
                    );
                }
            }
        }
    };
    if !options.write {
        outln!(
            "{} · rename to {}\n",
            relative_to_here(&plan.from),
            plan.to
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        show(&plan);
        outln!();
        offer_to_apply(
            "apply these changes?",
            "nothing written — pass --write to apply",
        );
        return Ok(());
    }
    let written = samplekit::collection::renaming::apply(&plan)
        .map_err(|error| Fail::io(error.to_string()))?;
    outln!(
        "renamed: {}  →  {}",
        relative_to_here(&plan.from),
        relative_to_here(&plan.to)
    );
    for (from, to) in &plan.own {
        outln!(
            "moved: {}  →  {}",
            relative_to_here(from),
            relative_to_here(to)
        );
    }
    let mut links: Vec<&PathBuf> = plan.links.iter().map(|change| &change.file).collect();
    links.sort();
    links.dedup();
    for file in links {
        if written.contains(file) && *file != plan.from {
            outln!("written: {}", relative_to_here(file));
        }
    }
    // The history keeps the samples, not their own files: renamed back, both
    // go back, the links with them.
    outln!(
        "{}",
        advice(&format!(
            "samplekit {} rename {} --write renames it back, its files and links with it",
            shell_path(&plan.to),
            plan.from
                .file_stem()
                .map(|stem| stem
                    .to_string_lossy()
                    .trim_end_matches(".sample")
                    .to_string())
                .unwrap_or_default()
        ))
    );
    Ok(())
}
