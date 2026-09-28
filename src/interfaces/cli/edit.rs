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
        outln!(
            "'{}' renamed to itself changes nothing — unchanged",
            tags[0]
        );
        return Ok(());
    }
    let root = rest
        .get(names)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let collection = loaded(&root)?;
    warn_skipped(&collection, options);
    let selected = select(&collection, options, &root)?;

    // A tag to rename or remove that no sample carries leaves the collection
    // as it was: said *unchanged*, exit 0, since nothing was left to do
    //  — with the nearest tag that exists, so
    // that a typo in the repair of a typo is still caught by the eye.
    if matches!(edit, Edit::Rename | Edit::Remove)
        && !collection
            .iter()
            .any(|entry| entry.sample.borrow().has_tag(&tags[0]))
    {
        outln!("no sample carries '{}' — unchanged", tags[0]);
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
            // Renaming `Infected` to `infected` is how the second spelling
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
    let named = |path: &Path| {
        selected
            .iter()
            .find(|entry| entry.path.as_deref() == Some(path))
            .map_or_else(
                || path.display().to_string(),
                |entry| file_of(Some(path), &entry.sample.borrow()),
            )
    };
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

    // Said in the tense that is true when it is read: *would* before a write,
    // and after one, what was done — `will add` stood on the screen above the
    // account of a write already made.
    let verb = |done: bool| match (&edit, done) {
        (Edit::Add, false) => format!("would add '{}' to", tags[0]),
        (Edit::Add, true) => format!("added '{}' to", tags[0]),
        (Edit::Remove, false) => format!("would remove '{}' from", tags[0]),
        (Edit::Remove, true) => format!("removed '{}' from", tags[0]),
        (Edit::Rename, false) => format!("would rename '{}' to '{}' on", tags[0], tags[1]),
        (Edit::Rename, true) => format!("renamed '{}' to '{}' on", tags[0], tags[1]),
    };
    let show_plan = |done: bool| {
        outln!(
            "{} {} of {} samples\n",
            verb(done),
            changes.len(),
            selected.len()
        );
        for (_, name, what) in &changes {
            outln!("  {name:<28} {what}");
        }
        for (_, name) in unchanged.iter().take(5) {
            outln!("  {name:<28} unchanged");
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
        outln!("\nnothing to write: no sample of the selection would change");
        return Ok(());
    }
    if !options.write {
        show_plan(false);
        warn_outside();
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }

    // Written one file at a time, each atomically: a failure halfway names the
    // files changed before it, and leaves the ones after it untouched.
    let written: Vec<String> = match tagging::apply(&plan) {
        Ok(paths) => paths.iter().map(|path| named(path)).collect(),
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
            return Err(Fail::data(said));
        }
    };
    show_plan(true);
    warn_outside();
    // The files are named above, each with what changed in it.
    outln!("\nwritten: {}", counted_as(written.len(), "file", "files"));
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
        outln!("no sample carries '{old}' — unchanged");
        return Ok(());
    }
    let carrying: Vec<(PathBuf, String)> = selected
        .iter()
        .filter_map(|entry| {
            let path = entry.path.clone()?;
            let sample = entry.sample.borrow();
            let name = file_of(Some(&path), &sample);
            sample
                .unusable_tags()
                .iter()
                .any(|tag| tag == old)
                .then_some((path, name))
        })
        .collect();
    let show_plan = |done: bool| {
        outln!(
            "{} '{old}' to '{new}' on {} of {} samples\n",
            if done { "renamed" } else { "would rename" },
            carrying.len(),
            selected.len()
        );
        for (_, name) in &carrying {
            outln!("  {name:<28} '{old}' → '{new}'");
        }
    };
    if carrying.is_empty() {
        show_plan(false);
        outln!("\nnothing to write: no sample of the selection carries '{old}'");
        return Ok(());
    }
    if !options.write {
        show_plan(false);
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }
    let mut written = Vec::new();
    for (path, name) in &carrying {
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
    // In the words every command that writes a file says it.
    outln!("\nwritten: {}", counted_as(written.len(), "file", "files"));
    Ok(())
}

/// One file, re-read and written back through its origin.
pub(super) fn note_of(
    before: &[Identifier],
    after: &[Identifier],
    tags: &[Identifier],
    edit: &Edit,
) -> String {
    match edit {
        Edit::Add => format!("+{}", tags[0]),
        Edit::Remove => format!("-{}", tags[0]),
        Edit::Rename if before.contains(&tags[1]) && after.len() < before.len() => {
            format!(
                "{} → {}; already had it, the duplicate is dropped",
                tags[0], tags[1]
            )
        }
        Edit::Rename => format!("{} → {}", tags[0], tags[1]),
    }
}

/// A new sample, shaped by what the collection already holds — and never
/// carrying another sample's computed values or their records, which is what
/// copying a file by hand does wrong.
pub(super) fn new_sample(
    name: &str,
    like: Option<&Path>,
    into: Option<&Path>,
    keep: &[String],
    options: &Options,
) -> Outcome {
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
        Some(path) => {
            if !path.exists() {
                return Err(Fail::io(format!(
                    "{}: no such file or directory",
                    path.display()
                )));
            }
            Some(path.to_path_buf())
        }
        // Without --like, nothing is taken from anywhere: taking the shape of
        // another sample is exactly what --like is for, and copying a
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

    // A destination is a sample's file: `--into brews/x.txt` previewed a file
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
            "{}: already here\n  samplekit set {} <field>=<value> changes what it holds",
            destination.display(),
            shell_path(&destination)
        )));
    }

    // What the project declares about a name, so that a quantity it already
    // knows is written as one rather than as an attribute: the project the file
    // goes into, which `--like` alone decides when `--into` is not given. A
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
        // Readings, `og.readings=1.05,1.06`, as `set` takes them: shown under
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
        let value = typed(&field_name, &text)?;
        // The number is shown back, so that a preview can be read over before
        // it is written — as the project writes it.
        let shown = shown_declared(&value, &field_name, declaring.as_ref());
        let (_, what) = apply_known(
            &mut sample,
            &field_name,
            value,
            declaring.as_ref(),
            neighbours.as_ref(),
        )?;
        filled.push(format!("{what} = {shown}"));
    }

    match &pattern_path {
        Some(path) => outln!(
            "{} {}, shaped like {}",
            if options.write {
                "writing"
            } else {
                "would write"
            },
            destination.display(),
            path.display()
        ),
        None => outln!(
            "{} {} — what you give here, named by its file\n  \
             samplekit new {} --like <sample> takes another sample's shape",
            if options.write {
                "writing"
            } else {
                "would write"
            },
            destination.display(),
            shell_word(name)
        ),
    }
    // The sections below are set off from the heading by one blank line, and
    // only where there is one: `new pilsner` alone printed two blank lines
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
        // listed `og` to fill in, and the file then had no `og` anywhere.
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
        open();
        outln!("  filled in now                  {}", filled.join(", "));
    }
    // What --keep copied is said, with the rest the pattern gives.
    let kept: Vec<&str> = measured
        .iter()
        .filter(|name| !given(name) && keep.iter().any(|kept| kept == name.as_str()))
        .map(String::as_str)
        .collect();
    if !kept.is_empty() {
        open();
        outln!("  kept from the pattern          {}", kept.join(", "));
    }
    let carried: Vec<String> = attributes
        .iter()
        .filter(|(name, _)| {
            !filled
                .iter()
                .any(|written| written.split(' ').next() == Some(name.as_str()))
        })
        .map(|(name, shown)| format!("{name} = {shown}"))
        .collect();
    if !carried.is_empty() {
        // These describe the sample rather than measure it — a batch, an
        // operator, a date — so they come across with the shape. They are also
        // the pattern's own identity, which is why they are shown in full.
        open();
        outln!("  copied from the pattern        {}", carried.join(", "));
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
    notes.extend(unvalued_note(&unvalued));
    for note in &notes {
        outln!("\n  {note}");
    }

    if !options.write {
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }

    sample.set_note(format!("# {name}\n"));
    // A file another writer made there since the name was checked is not
    // replaced.
    document::save_sample(&mut sample, &Destination::New(destination.clone()))
        .map_err(|error| write_failed(&destination, &error))?;
    let quoted = shell_path(&destination);
    // The model is offered only where there is one to run: a project with
    // none was sent to a command with nothing to compute.
    let model = declaring
        .as_ref()
        .is_some_and(|config| config.model().is_some());
    outln!(
        "\nwritten: {}\n\n  samplekit set {quoted} <field>=<value>   fill a value in{}",
        destination.display(),
        if model {
            format!("\n  samplekit compute {quoted} --write        run the model on it")
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
                "--add-row wants the row's cells: samplekit set {shown} --add-row {table} <column>=<value> …"
            ),
            None => format!(
                "set wants what to change, as field=value: samplekit set {shown} {quantity}=<value>"
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

    let before_states: Vec<String> = not_current(&sample)
        .into_iter()
        .map(|(name, _, _)| name)
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
        // By the field it addresses, not as typed: `malt` and `malt.v` are one.
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

    // What renaming a unit leaves as it was, said after the change itself.
    let mut unit_readers = false;
    // What readings given here come to, said once below the changes.
    let mut notes: Vec<String> = Vec::new();
    let mut unvalued: Vec<String> = Vec::new();
    for written in &options.positionals {
        let mut relabelled: Vec<String> = Vec::new();
        let (field_name, text) = split_assignment(written)?;
        // Readings are a channel of their own, `og.readings=1.05,1.06`, and
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
        let (mut before, mut after) = (shown(&change.before), shown(&change.after));
        // A blank value removes what was there: an edit, said as one, where `→
        // —` read as a value nobody had given yet.
        if change.kept.is_none()
            && change.became != editing::Became::Unchanged
            && change
                .after
                .as_ref()
                .is_none_or(|after| matches!(after, Value::Absent))
        {
            after = "removed".to_string();
        }
        // Two numbers the precision writes alike are still a change, and are
        // then shown whole: a preview must not read `1.021 → 1.021`.
        if before == after && change.became != editing::Became::Unchanged {
            let whole = |value: &Option<Value>| value.as_ref().map_or("—".to_string(), shown_value);
            (before, after) = (whole(&change.before), whole(&change.after));
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
            // The quantity's name, not the path written: `fg.v=` offered
            // `fg.v.u=`, which is no field.
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
                     samplekit compute --force returns it",
                    ""
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
    for change in &changes {
        outln!("  {change}");
    }
    notes.extend(unvalued_note(&unvalued));
    for note in &notes {
        outln!("\n  {note}");
    }
    if changed == 0 {
        outln!("\nnothing written — every value is already what it was asked to be");
        return Ok(());
    }

    // What this costs downstream, which is the question an editor cannot answer.
    let after_states = not_current(&sample);
    let newly: Vec<(String, Freshness, usize)> = after_states
        .iter()
        .filter(|(name, _, _)| !before_states.contains(name))
        .cloned()
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
                "samplekit compute {} --write recomputes them",
                shell_path(file)
            ),
            all if all == listed.len() => format!(
                "compute keeps an override — samplekit compute {} --force --write gives it back \
                 to its formula",
                shell_path(file)
            ),
            _ => format!(
                "samplekit compute {} --write recomputes the others; --force also gives an \
                 override back to its formula",
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
            outln!("\n  nothing derived rests on this");
        } else {
            outln!(
                "\n  nothing more becomes outdated — {} already not current before this",
                counted_as(before_states.len(), "value was", "values were")
            );
            for name in &before_states {
                outln!("    {name}");
            }
            let listed = after_states
                .iter()
                .filter(|(name, _, _)| before_states.contains(name))
                .map(|(_, state, _)| state)
                .collect();
            outln!("\n  {}", advice(&way_back(listed)));
        }
    } else {
        outln!(
            "\n  this makes {} not current",
            counted_as(newly.len(), "value", "values")
        );
        for (name, state, rows) in &newly {
            outln!("    {name:<28} {}", state_of(&sample, state, *rows));
        }
        let listed = newly.iter().map(|(_, state, _)| state).collect();
        outln!("\n  {}", advice(&way_back(listed)));
    }

    if !options.write {
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }
    document::save_sample(&mut sample, &Destination::Origin(origin))
        .map_err(|error| write_failed(file, &error))?;
    outln!("\nwritten: {}", file.display());
    Ok(())
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
/// every quantity given so. `set` reads no model, so what the model declares is
/// not known here, and is not guessed at.
fn unvalued_note(names: &[String]) -> Option<String> {
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

/// The repeated measurements behind one quantity, or one cell: `og.readings=…`,
/// `mouthfeel.sweetness[40].readings=…`, with or without brackets. Said as lines of
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
                     until the model's Column declares one — sk.Column(value=sk.stats.mean)"
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
                    "'{name}' is an attribute here, and readings belong to a quantity: \
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
            let convention = sample.property(&name).ok().and_then(|handle| {
                handle
                    .records()
                    .statistics
                    .as_ref()
                    .and_then(|statistics| statistics.uncertainty)
            });
            let statistic_said = match location {
                Some(location) => {
                    let summary = samplekit::core::statistics::summarize(&readings);
                    format!(
                        ", their {} {}",
                        location.name().replace('_', " "),
                        number_declared(location.of(&summary), name.as_str(), config, false)
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
                    lines.push(format!(
                        "{:<28} its value, their {statistic_name}, is not current until \
                         samplekit compute takes it again",
                        ""
                    ));
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
            "{field_name}: readings belong to a quantity or a table's cell"
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
    // A cell given readings, `sweetness.readings=1.05,1.06`, holds them as a
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
    // collection holds, which is what `new --like` already does. `nt.md` has an
    // empty parent, which names the current directory: read as a path, it
    // found no sibling, and the shape came from the model instead.
    let directory = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let added = editing::add_row_with_readings(&mut sample, table, cells, measured, |name| {
        loaded(directory)
            .ok()
            .and_then(|collection| editing::shape_from(&collection, file, name))
            // No sample holds it yet: the model's description says its columns,
            // written again first where it is stale.
            .or_else(|| {
                let config = samplekit::config::project_config::load_for(directory)
                    .ok()
                    .flatten()?;
                describe_now(&config);
                editing::shape_from_model(&config, name)
            })
    })
    .map_err(edit_failure)?;

    outln!(
        "{} · {} row in {table}\n",
        file.display(),
        if options.write {
            "a new"
        } else {
            "would add a"
        }
    );
    // Where the columns come from, said rather than done quietly: this file had
    // no such table a moment ago.
    if let Some(source) = &added.lent_from {
        outln!(
            "  the table is created here, with the columns of {}",
            display_relative(source, Path::new("."))
        );
    }
    let declaring = samplekit::config::project_config::load_for(directory)
        .ok()
        .flatten();
    let mut shown: Vec<String> = added
        .given
        .iter()
        .map(|(column, value, overriding)| {
            format!(
                "{column} = {}{}",
                shown_declared(value, &format!("{table}.{column}"), declaring.as_ref()),
                if *overriding {
                    "   (a computed column, set by hand: an override)"
                } else {
                    ""
                }
            )
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
        shown.push(format!(
            "{column}.readings = [{}]{}",
            readings
                .iter()
                .map(|reading| number_declared(*reading, &at, declaring.as_ref(), false))
                .collect::<Vec<_>>()
                .join(", "),
            match statistic.flatten() {
                Some(location) =>
                    format!(" (its value their {})", location.name().replace('_', " ")),
                // The model is not read here: what it declares is taken when it
                // computes.
                None => " (no statistic recorded for the column: no value until compute takes \
                          the model's)"
                    .to_string(),
            }
        ));
    }
    outln!("  {}", shown.join(", "));
    let names = |columns: &[Identifier]| {
        columns
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !added.left_absent.is_empty() {
        outln!(
            "  left absent                    {}",
            names(&added.left_absent)
        );
    }
    if !added.by_the_model.is_empty() {
        outln!(
            "  computed by the model          {}",
            names(&added.by_the_model)
        );
    }

    if !options.write {
        outln!("\n{}", advice("nothing written — pass --write to apply"));
        return Ok(());
    }
    document::save_sample(&mut sample, &Destination::Origin(origin))
        .map_err(|error| write_failed(file, &error))?;
    outln!(
        "\nwritten: {}\n\n  samplekit compute {} --write   fills what the model derives from it",
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
    if runtime::template_of(config).is_none() || runtime::current_description(config).is_some() {
        return;
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
    if let Err(error) = runtime::describe(config, &root) {
        let said = error.to_string();
        warn(&format!(
            "the model could not be read, so what it declares is not known: {}\n  \
             a name only the model declares is written as the samples here hold it",
            said.lines().next().unwrap_or_default()
        ));
    }
}

/// One assignment to a sample that may not hold its name yet, written as the
/// project knows it (`editing::known_as`): as `[property.*]` declares it, else
/// as the model's description says it declares it, else as the collection's
/// samples hold it — a quantity, with its unit. `og=1.048` for a new brew
/// beside brews that all measure `og` was written as an attribute, and so was
/// `ibu=25.1` where only the model declared it; and a text the model's
/// property holds is a property too.
pub(super) fn apply_known(
    sample: &mut samplekit::core::sample::Sample,
    field_name: &str,
    value: Value,
    config: Option<&ProjectConfig>,
    collection: Option<&SampleList>,
) -> Result<(editing::Change, String), Fail> {
    describe_for(sample, field_name, config);
    let known = editing::known_as(sample, field_name, config, collection);
    if let Some(known) = &known
        && let Some(held) = &known.held
        // Text where every sample holds a number is refused as `set` refuses
        // it: `og=1,050` was written as the text "1,050".
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
            "{} (new, a quantity {} declares)",
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
                "{} (new, a quantity as the other samples hold it)",
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
        editing::Became::Override => format!("{field} (an override: the formula stays)"),
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
                    "a quantity nowhere here",
                    |count| {
                        format!(
                            "{count} names are a quantity nowhere here, so they are written as \
                             attributes: declare each in the model or as [property.<name>] in \
                             .samplekitrc to write it as a quantity"
                        )
                    },
                    format!(
                        "'{field}' is a quantity nowhere here — no sample holds it as one, and \
                         neither .samplekitrc nor the model declares it — so it is written as \
                         an attribute\n  a quantity cannot share its name with one: if it is a \
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
    let precision = editing::declared_precision(field, config).map(|precision| {
        if uncertainty {
            precision.of_uncertainty()
        } else {
            precision
        }
    });
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
