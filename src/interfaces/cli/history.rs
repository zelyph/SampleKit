//! `log` and `diff`: the project's history read back, as values rather than as
//! lines of YAML.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::config::version_control::{self as history, Entry};
use samplekit::presentation::changes::{
    changed_said, changes_among, changes_of, computed_from, field_base, machine_said, machines_of,
    when,
};

/// A project the command reaches, and the paths its targets name in it, as
/// the history names them: a file whole, a folder with its slash; none for
/// the whole project.
struct Reached {
    config: ProjectConfig,
    within: Vec<String>,
}

impl Reached {
    fn holds(&self, name: &str) -> bool {
        self.elsewhere(name).is_none() && self.named(name)
    }

    /// The folder of the project that keeps a file a snapshot of this one
    /// holds, where it is another now: a folder below made a project of its
    /// own since, with its own history. Said so, its files were said
    /// *removed* — and `restore` named a command that found no history.
    fn elsewhere(&self, name: &str) -> Option<PathBuf> {
        if name.starts_with('@') || name == samplekit::config::project_config::FILENAME {
            return None;
        }
        let path = self.config.root().join(name);
        if !path.is_file() {
            return None;
        }
        let rc = samplekit::config::project_config::find(path.parent()?)?;
        let theirs = dunce::canonicalize(rc.parent()?).ok()?;
        let ours = dunce::canonicalize(self.config.root()).ok()?;
        (theirs != ours).then_some(theirs)
    }

    /// The projects below that keep files this one's snapshots `names`
    /// hold, from this project's folder, with how many.
    fn kept_elsewhere<'a>(
        &self,
        names: impl IntoIterator<Item = &'a String>,
    ) -> Vec<(String, usize)> {
        let ours = dunce::canonicalize(self.config.root()).unwrap_or_default();
        let mut found: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for name in names {
            if !self.named(name) {
                continue;
            }
            if let Some(theirs) = self.elsewhere(name) {
                let shown = theirs.strip_prefix(&ours).map_or_else(
                    |_| theirs.display().to_string(),
                    |below| below.display().to_string(),
                );
                *found.entry(shown).or_default() += 1;
            }
        }
        found.into_iter().collect()
    }

    /// Whether a target names the file, whoever keeps it now.
    fn named(&self, name: &str) -> bool {
        self.within.is_empty()
            || self.within.iter().any(|within| {
                within.is_empty()
                    || name == within
                    || (within.ends_with('/') && name.starts_with(within.as_str()))
                    // A folder gone from the disk is named without its slash.
                    || (!within.ends_with('/')
                        && name.strip_prefix(within.as_str()).is_some_and(|rest| rest.starts_with('/')))
            })
    }

    /// One file named, and no folder: what changed is that file on every
    /// line of `log`, which is then not said.
    fn one_file(&self, entries: &[Entry]) -> bool {
        match self.within.as_slice() {
            [within] if !within.is_empty() && !within.ends_with('/') => {
                let below = format!("{within}/");
                !entries
                    .iter()
                    .any(|entry| entry.changed.iter().any(|name| name.starts_with(&below)))
            }
            _ => false,
        }
    }
}

/// The projects `targets` belong to — the current folder's without one.
fn reached(targets: &[PathBuf]) -> Result<Vec<Reached>, Fail> {
    let places = if targets.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        targets.to_vec()
    };
    let mut found: Vec<Reached> = Vec::new();
    for place in &places {
        // A path gone from the disk — a sample deleted — is still one the
        // history names: it is looked for from its folder, so that it can be
        // logged, compared and restored by name.
        let exists = place.exists();
        let folder = if exists {
            place.clone()
        } else {
            let parent = place
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
            if !parent.is_dir() || place.file_name().is_none() {
                return Err(Fail::usage(format!("{} does not exist", place.display())));
            }
            parent
        };
        let Some(rc) = samplekit::config::project_config::find(&folder) else {
            return Err(Fail::usage(format!(
                "{} is in no project: no .samplekitrc above it keeps a history",
                place.display()
            )));
        };
        // A configuration that cannot be read is what a restore may be
        // there to mend: the history is read from its folder regardless.
        let config = samplekit::config::project_config::load(&rc).unwrap_or_else(|error| {
            errln!("warning: {error}; the history is read with the configuration it last kept");
            kept_configuration(&rc)
        });
        super::bring_history_from_above(&config)?;
        let root = dunce::canonicalize(config.root()).unwrap_or_default();
        let here = if exists {
            dunce::canonicalize(place).unwrap_or_default()
        } else {
            dunce::canonicalize(&folder)
                .unwrap_or_default()
                .join(place.file_name().unwrap_or_default())
        };
        // Typed with its slash, a folder gone is still a folder.
        let folder_named = here.is_dir() || (!exists && place.to_string_lossy().ends_with('/'));
        let within = here
            .strip_prefix(&root)
            .ok()
            .map(|relative| {
                let name = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if folder_named && !name.is_empty() {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .unwrap_or_default();
        // Gone, and never kept either: a misspelt path, not a deleted one.
        if !exists {
            let probe = Reached {
                config: config.clone(),
                within: vec![within.clone()],
            };
            let entries = history::entries(&config).map_err(|error| Fail::io(error.to_string()))?;
            if !entries
                .iter()
                .any(|entry| entry.changed.iter().any(|name| probe.holds(name)))
            {
                return Err(Fail::usage(format!(
                    "{} does not exist, and no snapshot of {} held it",
                    place.display(),
                    config.root().display()
                )));
            }
        }
        let same =
            |other: &Reached| dunce::canonicalize(other.config.root()).unwrap_or_default() == root;
        match found.iter_mut().find(|other| same(other)) {
            Some(other) => other.within.push(within),
            None => found.push(Reached {
                config,
                within: vec![within],
            }),
        }
    }
    // **A folder above several projects answers for each**: named
    // whole, its projects below are reached too, each with its own history,
    // as `status` reads them; the folder's own, where it kept none, is left
    // out rather than said empty above theirs.
    let mut below: Vec<Reached> = Vec::new();
    for project in &found {
        if !project.within.iter().any(String::is_empty) {
            continue;
        }
        for config in projects_below(project.config.root()) {
            let root = dunce::canonicalize(config.root()).unwrap_or_default();
            let known = found
                .iter()
                .chain(&below)
                .any(|other| dunce::canonicalize(other.config.root()).unwrap_or_default() == root);
            // Its snapshots from before it was a project brought first, as
            // for a project named; one with no history yet is left out.
            if !known
                && super::bring_history_from_above(&config).is_ok()
                && history::entries(&config).is_ok_and(|entries| !entries.is_empty())
            {
                below.push(Reached {
                    config,
                    within: vec![String::new()],
                });
            }
        }
    }
    if !below.is_empty() {
        found.retain(|project| {
            !project.within.iter().any(String::is_empty)
                || history::entries(&project.config).is_ok_and(|entries| !entries.is_empty())
        });
        found.extend(below);
    }
    Ok(found)
}

/// When a snapshot was taken, as `log` says it.
pub(super) fn when_said(entry: &Entry) -> String {
    when(entry)
}

/// The kept files that differ from what the history last kept of them — this
/// machine's last snapshot, with what the other machines' snapshots it would
/// join changed — that is, changed outside SampleKit and not yet kept.
fn pending(project: &Reached) -> Result<Vec<String>, Fail> {
    let failed = |error: history::VcsError| Fail::io(error.to_string());
    let last = history::last_kept(&project.config)
        .map_err(failed)?
        .map(|kept| kept.files)
        .unwrap_or_default();
    let now = history::files_now(&project.config).map_err(failed)?;
    let mut pending: Vec<String> = now
        .iter()
        .filter(|(name, bytes)| last.get(*name) != Some(*bytes))
        .map(|(name, _)| name.clone())
        .chain(last.keys().filter(|name| !now.contains_key(*name)).cloned())
        .filter(|name| project.holds(name))
        .collect();
    pending.sort();
    Ok(pending)
}

/// `samplekit log`: the snapshots, newest first, numbered over the whole
/// history; a target keeps those that changed it, and `--at` starts the list at
/// the snapshot it names.
/// A state named over several projects refused before anything is said: a
/// snapshot's number, or an instant, names one project's history.
fn one_project(projects: &[Reached], option: &str, again: &str) -> Outcome {
    if projects.len() < 2 {
        return Ok(());
    }
    let folders: Vec<String> = projects
        .iter()
        .map(|project| super::relative_to_here(project.config.root()))
        .collect();
    Err(Fail::usage(format!(
        "{option} names a snapshot of one project's history, and here are {}: {} — name one, \
         samplekit {} {again}",
        folders.len(),
        folders.join(", "),
        // A folder below names its project alone: `.` would name them all
        // again.
        folders
            .iter()
            .find(|folder| *folder != ".")
            .unwrap_or(&folders[0])
    )))
}

pub(super) fn log(targets: &[PathBuf], since: Option<&str>, options: &Options) -> Outcome {
    let projects = reached(targets)?;
    if let Some(named) = since {
        one_project(&projects, "--at", &format!("log --at {named}"))?;
    }
    for (at, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if at > 0 {
                outln!();
            }
            outln!("{}", super::relative_to_here(project.config.root()));
        }
        let entries =
            history::entries(&project.config).map_err(|error| Fail::io(error.to_string()))?;
        // Where the list begins: the snapshot `--at` names, or the newest. A
        // state that names nothing is refused, a history not yet kept too.
        let first = match since {
            Some(named) => match state_named(named, &entries)? {
                State::Kept(at) => Some(at),
                State::Now | State::Before(_) => None,
            },
            None => None,
        };
        if entries.is_empty() {
            outln!("{NO_HISTORY}");
            continue;
        }
        let mut rows: Vec<Vec<String>> = Vec::new();
        let one_file = project.one_file(&entries);
        // Where another machine than this one wrote a snapshot, each
        // snapshot's is named, after *when*: a fresh clone's history, all
        // another's, read as if this machine had written it.
        let machines = machines_of(&entries).len() > 1
            || entries.iter().any(|entry| {
                !entry.machine.is_empty()
                    && entry.machine != samplekit::config::version_control::YOU
            });
        // What differs from the last snapshot, not yet kept.
        let pending = pending(project)?;
        let pending: Vec<&String> = pending.iter().collect();
        // `now` is a moment, said under *when*: in the `#` column it made
        // the numbers' column three wide for one digit.
        if !pending.is_empty() && first.is_none() {
            rows.push(vec![
                String::new(),
                "now".to_string(),
                // This computer, as each of its snapshots is named.
                history::YOU.to_string(),
                "changed outside SampleKit, not yet kept".to_string(),
                changed_said(&pending),
            ]);
        }
        for (number, entry) in entries.iter().enumerate().skip(first.unwrap_or(0)) {
            let changed: Vec<&String> = entry
                .changed
                .iter()
                .filter(|name| project.holds(name))
                .collect();
            // A script that wrote a figure or an export and no kept file has a
            // snapshot of its own, empty, for its source: listed over the
            // project, where it is what made that file.
            let script_only =
                entry.changed.is_empty() && !one_file && entry.message.starts_with("python");
            if changed.is_empty() && !script_only {
                continue;
            }
            rows.push(vec![
                (number + 1).to_string(),
                when(entry),
                machine_said(entry),
                entry.message.clone(),
                if script_only {
                    // What it wrote, named: *no kept file* read as though the
                    // figure it made were not recorded.
                    let written: Vec<String> = history::made_from(&project.config, &entry.id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|made| {
                            Path::new(&made.written)
                                .file_name()
                                .map_or(made.written.clone(), |name| {
                                    name.to_string_lossy().into_owned()
                                })
                        })
                        .collect();
                    if written.is_empty() {
                        format!("no sample changed · log --script {}", number + 1)
                    } else {
                        format!(
                            "wrote {}, no sample · log --script {}",
                            written.join(", "),
                            number + 1
                        )
                    }
                } else {
                    changed_said(&changed)
                },
            ]);
        }
        if rows.is_empty() {
            outln!("{}", nothing_changed(targets, &project.config));
            continue;
        }
        let mut headers = vec!["#", "when", "machine", "what", "changed"];
        if one_file {
            headers.pop();
            for row in &mut rows {
                row.pop();
            }
        }
        if !machines {
            headers.remove(2);
            for row in &mut rows {
                row.remove(2);
            }
        }
        let style = project_table_style(&project.config, options)?;
        let mut layout = render::lay_out(
            headers.iter().map(|name| name.to_string()).collect(),
            rows.clone(),
            output_target(options),
            style,
            0,
            false,
        );
        // Every column but `#` is text: a column of *12 samples, …* reads
        // to the renderer as numbers, and was aligned on a point it does not
        // hold. Its cells are given back as written, set from the left; a
        // column aligned as numbers is never narrowed, so each still fits.
        for (column, alignment) in layout.alignment.iter_mut().enumerate().skip(1) {
            if *alignment != render::Alignment::Left {
                *alignment = render::Alignment::Left;
                for (cells, row) in layout.cells.iter_mut().zip(&rows) {
                    cells[column] = row[column].clone();
                }
            }
        }
        out!("{}", render::draw(&layout, style));
    }
    Ok(())
}

/// `samplekit log --export`: an account of the history in Markdown, to share
/// beside the files, since the history stays on each machine. The snapshots
/// `log` lists, each with every file it changed and the script kept beside it;
/// printed, or written with `-o` and `--write`.
pub(super) fn log_export(targets: &[PathBuf], since: Option<&str>, options: &Options) -> Outcome {
    let projects = reached(targets)?;
    let mut text = String::new();
    for project in &projects {
        let config = &project.config;
        let entries = history::entries(config).map_err(|error| Fail::io(error.to_string()))?;
        let first = match since {
            Some(named) => match state_named(named, &entries)? {
                State::Kept(at) => Some(at),
                State::Now | State::Before(_) => None,
            },
            None => None,
        };
        let name = dunce::canonicalize(config.root())
            .ok()
            .and_then(|root| {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| config.root().display().to_string());
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&format!("# History of {name}\n\n"));
        if entries.is_empty() {
            text.push_str(&format!("{NO_HISTORY}.\n"));
            continue;
        }
        let one_file = project.one_file(&entries);
        // An account is read away from this machine: each machine by its host,
        // this one's too.
        let mut machines: Vec<(&str, &str)> = entries
            .iter()
            .map(|entry| (entry.host.as_str(), entry.machine_id.as_str()))
            .collect();
        machines.sort();
        machines.dedup();
        let machines: Vec<String> = machines
            .into_iter()
            .map(|(host, _)| host.to_string())
            .collect();
        let mut sections: Vec<String> = Vec::new();
        // What is not kept yet comes first, as `log` puts `now`.
        if first.is_none() {
            let pending = pending(project)?;
            let pending: Vec<&String> = pending.iter().collect();
            if !pending.is_empty() {
                sections.push(format!(
                    "## Not kept yet\n\nChanged outside SampleKit since the last snapshot.\n\n{}",
                    listed(&pending)
                ));
            }
        }
        for (number, entry) in entries.iter().enumerate().skip(first.unwrap_or(0)) {
            let changed: Vec<&String> = entry
                .changed
                .iter()
                .filter(|name| project.holds(name))
                .collect();
            // As `log` lists it: over the project, not in one file's account.
            let script_only =
                entry.changed.is_empty() && !one_file && entry.message.starts_with("python");
            if changed.is_empty() && !script_only {
                continue;
            }
            // Its machine, where more than one wrote the history, and the
            // machines it joined.
            let mut section = if machines.len() > 1 {
                format!(
                    "## #{} · {} · {}\n\n{}\n",
                    number + 1,
                    when(entry),
                    markdown_text(&entry.host),
                    markdown_text(&entry.message)
                )
            } else {
                format!(
                    "## #{} · {}\n\n{}\n",
                    number + 1,
                    when(entry),
                    markdown_text(&entry.message)
                )
            };
            if !entry.joined.is_empty() {
                section.push_str(&format!(
                    "\nJoins the snapshots of {}.\n",
                    markdown_text(&entry.joined.join(", "))
                ));
            }
            if let Some((script, _)) = history::script_of(config, &entry.id)
                .map_err(|error| Fail::io(error.to_string()))?
            {
                section.push_str(&format!("\nScript: {}\n", markdown_text(&script)));
            }
            section.push('\n');
            if changed.is_empty() {
                section.push_str("No kept file changed.\n");
            } else {
                section.push_str(&listed(&changed));
            }
            section.push_str(&format!("\nSnapshot {}.\n", short(&entry.id)));
            sections.push(section);
        }
        let kept = sections
            .iter()
            .filter(|section| section.starts_with("## #"))
            .count();
        // No date of writing: the account is read beside files that moved
        // on since, and its newest snapshot says when it was.
        match machines.split_last() {
            Some((last, others)) if !others.is_empty() => text.push_str(&format!(
                "Kept on {} machines, {} and {}: {}, newest first.\n",
                machines.len(),
                markdown_text(&others.join(", ")),
                markdown_text(last),
                counted_as(kept, "snapshot", "snapshots")
            )),
            _ => text.push_str(&format!(
                "As kept on the machine that wrote this account: {}, newest first.\n",
                counted_as(kept, "snapshot", "snapshots")
            )),
        }
        for section in sections {
            text.push('\n');
            text.push_str(&section);
        }
    }
    emit_output(&text, options, &list::empty())
}

/// Files as a Markdown list, each as the history names it.
fn listed(names: &[&String]) -> String {
    names
        .iter()
        .map(|name| format!("- {}\n", markdown_text(name)))
        .collect()
}

/// Text that Markdown reads as written: what would be emphasis, code or a
/// link is escaped, so `fix_a_b.py` keeps its underscores.
fn markdown_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(
            character,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '#' | '|'
        ) {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

/// What `log` and `diff` say where no snapshot changed what they were given.
///
/// An export, a table written with `-o` or a figure is a file SampleKit wrote,
/// and the history keeps it as a record of how it was made, not as a file that
/// changes: *no snapshot changed what is named here* was true, and sent the
/// reader nowhere. `explain` is what answers for it.
fn nothing_changed(targets: &[PathBuf], config: &ProjectConfig) -> String {
    let root = dunce::canonicalize(config.root()).unwrap_or_default();
    let written: Vec<&PathBuf> = targets
        .iter()
        .filter(|target| target.is_file())
        .filter(|target| dunce::canonicalize(target).is_ok_and(|here| here.starts_with(&root)))
        .filter(|target| {
            let made = history::file_hash(target)
                .ok()
                .and_then(|hash| history::made_all(config, &hash).ok())
                .is_some_and(|made| !made.is_empty());
            made || history::carried_snapshot(target).is_some()
        })
        .collect();
    match written.as_slice() {
        [] => "no snapshot changed what is named here".to_string(),
        [only] => format!(
            "{} is a file SampleKit wrote, which no snapshot keeps as it keeps a sample — \
             samplekit {} explain says where it came from",
            only.display(),
            shell_path(only)
        ),
        many => format!(
            "{} are files SampleKit wrote, which no snapshot keeps as it keeps a sample — \
             samplekit FILE explain says where each came from",
            many.iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// What `log`, `diff` and `restore` say where no history is kept.
const NO_HISTORY: &str =
    "no history is kept here yet: it begins with the first change SampleKit writes";

/// A snapshot as a line says it: its number from the newest, the beginning
/// of its id — which stays when the number moves on — when, and what wrote.
fn snapshot_said(entries: &[Entry], at: usize) -> String {
    format!(
        "#{} · {} · {} · {}",
        at + 1,
        short(&entries[at].id),
        when(&entries[at]),
        entries[at].message
    )
}

/// The beginning of a snapshot's id, as `--at`, `--from` and `--to` take it.
fn short(id: &str) -> &str {
    &id[..7.min(id.len())]
}

/// Each file's lines, one after the other; a file said in one line — new,
/// removed, restored, changed by its lines — has its word set in one column
/// with the others'.
pub(super) fn files_said(said: Vec<(String, Vec<String>)>) -> Vec<String> {
    let one_line = |name: &str, lines: &[String]| -> Option<String> {
        match lines {
            [line] => line
                .strip_prefix(name)
                .and_then(|rest| rest.strip_prefix("   "))
                .map(str::to_string),
            _ => None,
        }
    };
    let width = said
        .iter()
        .filter(|(name, lines)| one_line(name, lines).is_some())
        .map(|(name, _)| render::width(name))
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    for (name, lines) in said {
        match one_line(&name, &lines) {
            Some(rest) => out.push(format!(
                "{name}{}   {rest}",
                " ".repeat(width.saturating_sub(render::width(&name)))
            )),
            None => out.extend(lines),
        }
    }
    out
}

fn project_table_style(config: &ProjectConfig, options: &Options) -> Result<Style, Fail> {
    style_of(options, Some(config))
}

/// A state of the project a `diff` compares: a snapshot, or the files now.
enum State {
    Now,
    Kept(usize),
    /// The state a snapshot was taken over: its parent's files, a join's with
    /// what it joined — not the snapshot listed below it, which may be another
    /// machine's.
    Before(usize),
}

/// What `WHEN` names: `now`, a number from `log`, the beginning of a snapshot's
/// id, or a date — the last snapshot at or before that moment, a bare date
/// being the whole day, up to its end.
fn state_named(written: &str, entries: &[Entry]) -> Result<State, Fail> {
    let written = written.trim();
    if written == "now" {
        return Ok(State::Now);
    }
    if entries.is_empty() {
        return Err(Fail::usage(format!(
            "there is no snapshot {written}: {NO_HISTORY}"
        )));
    }
    if !written.is_empty() && written.len() <= 4 && written.chars().all(|c| c.is_ascii_digit()) {
        let number: usize = written.parse().unwrap_or(0);
        return if (1..=entries.len()).contains(&number) {
            Ok(State::Kept(number - 1))
        } else {
            Err(Fail::usage(format!(
                "there is no snapshot {written}: samplekit log numbers them from 1 to {}",
                entries.len()
            )))
        };
    }
    if written.len() >= 4 && written.chars().all(|c| c.is_ascii_hexdigit()) {
        let lower = written.to_ascii_lowercase();
        let matching: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.id.starts_with(&lower))
            .map(|(at, _)| at)
            .collect();
        return match matching.as_slice() {
            [one] => Ok(State::Kept(*one)),
            [] => Err(Fail::usage(format!(
                "no snapshot's id begins with {written}"
            ))),
            _ => Err(Fail::usage(format!(
                "{} snapshots' ids begin with {written}: give more of it",
                matching.len()
            ))),
        };
    }
    // A minute is the whole minute, as a bare date is the whole day: a
    // snapshot taken at 14:30:40 is one taken by 14:30.
    let moment = chrono::NaiveDateTime::parse_from_str(written, "%Y-%m-%d %H:%M")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(written, "%Y-%m-%dT%H:%M"))
        .map(|minute| minute + chrono::Duration::seconds(59))
        .or_else(|_| {
            chrono::NaiveDate::parse_from_str(written, "%Y-%m-%d")
                .map(|day| day.and_hms_opt(23, 59, 59).unwrap_or_default())
        })
        .map_err(|_| {
            Fail::usage(format!(
                "'{written}' names no state: now, a number from samplekit log, the beginning \
                 of a snapshot's id, or a date such as 2026-09-12 or '2026-09-12 14:30'"
            ))
        })?;
    // In the time of the machine that took each snapshot, as log shows it.
    let taken = |entry: &Entry| {
        chrono::DateTime::from_timestamp(entry.seconds + i64::from(entry.offset), 0)
            .map(|time| time.naive_utc())
    };
    entries
        .iter()
        .position(|entry| taken(entry).is_some_and(|time| time <= moment))
        .map(State::Kept)
        .ok_or_else(|| {
            Fail::usage(format!(
                "no snapshot was taken by {written}: the first is from {}",
                entries.last().map(when).unwrap_or_default()
            ))
        })
}

/// The snapshot `WHEN` names, by its place from the newest; `None` for now.
pub(super) fn state_at(named: &str, entries: &[Entry]) -> Result<Option<usize>, Fail> {
    Ok(match state_named(named, entries)? {
        State::Now | State::Before(_) => None,
        State::Kept(at) => Some(at),
    })
}

/// `samplekit diff`: two states of the project compared, sample by sample and
/// field by field, as values.
pub(super) fn diff(targets: &[PathBuf], from: Option<&str>, to: Option<&str>) -> Outcome {
    let projects = reached(targets)?;
    match (from, to) {
        (Some(named), _) => one_project(&projects, "--from", &format!("diff --from {named}"))?,
        (None, Some(named)) => one_project(&projects, "--to", &format!("diff --to {named}"))?,
        (None, None) => {}
    }
    for (at, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if at > 0 {
                outln!();
            }
            outln!("{}", super::relative_to_here(project.config.root()));
        }
        let config = &project.config;
        let entries = history::entries(config).map_err(|error| Fail::io(error.to_string()))?;
        // A state that names nothing is refused before anything is said,
        // where no history is kept too.
        for named in [from, to].into_iter().flatten() {
            state_named(named, &entries)?;
        }
        if entries.is_empty() {
            outln!("{NO_HISTORY}");
            continue;
        }
        // Without a state named, the last change kept of what is named here :
        // SampleKit keeps every change it writes, so the files as they are
        // differ from the last snapshot only by what was changed outside it —
        // said below, and compared with `--from 1`.
        let (from, to) = match (from, to) {
            (None, None) => {
                let Some(last) = entries
                    .iter()
                    .position(|entry| entry.changed.iter().any(|name| project.holds(name)))
                else {
                    outln!("{}", nothing_changed(targets, config));
                    continue;
                };
                (State::Before(last), State::Kept(last))
            }
            (from, None) => (state_named(from.unwrap_or("1"), &entries)?, State::Now),
            (None, Some(to)) => {
                let to = state_named(to, &entries)?;
                let from = match to {
                    State::Kept(at) => State::Before(at),
                    _ => State::Kept(0),
                };
                (from, to)
            }
            (Some(from), Some(to)) => (state_named(from, &entries)?, state_named(to, &entries)?),
        };
        let files = |state: &State| match state {
            State::Now => history::files_now(config),
            State::Kept(at) => history::files_at(config, &entries[*at].id),
            State::Before(at) => history::files_before(config, &entries[*at].id),
        };
        let kept_said = |at: usize| {
            format!(
                "#{} · {} · {}",
                at + 1,
                when(&entries[at]),
                entries[at].message
            )
        };
        let place = |id: &String| entries.iter().position(|entry| entry.id == *id);
        let said = |state: &State| match state {
            State::Now => "now".to_string(),
            State::Before(at) if entries[*at].parents.is_empty() => {
                "(before the first snapshot)".to_string()
            }
            State::Kept(at) => kept_said(*at),
            // The first parent, and each machine's it joined.
            State::Before(at) => {
                let parents: Vec<usize> = entries[*at].parents.iter().filter_map(place).collect();
                match parents.split_first() {
                    Some((first, [])) => kept_said(*first),
                    Some((first, others)) => format!(
                        "{}, with {}",
                        kept_said(*first),
                        others
                            .iter()
                            .map(|other| format!("#{} of {}", other + 1, entries[*other].machine))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    None => "(before the first snapshot)".to_string(),
                }
            }
        };
        // A join, shown alone: compared with this machine's last snapshot,
        // what came from the others listed apart.
        if let (State::Before(at), State::Kept(same)) = (&from, &to)
            && at == same
            && let Some(join) =
                samplekit::presentation::changes::of_join(config, &entries, *at, &|name: &str| {
                    project.holds(name)
                })
                .map_err(|error| Fail::io(error.to_string()))?
        {
            // One that changed only files a project below keeps now is one
            // this project's `log` does not show: said, where `#6` was looked
            // for in vain.
            let elsewhere = !entries[join.first]
                .changed
                .iter()
                .any(|name| project.holds(name));
            outln!(
                "from  {}, this machine's last{}",
                kept_said(join.first),
                if elsewhere {
                    " — of files a project below keeps now, whose log shows it"
                } else {
                    ""
                }
            );
            outln!("to    {}", kept_said(*at));
            outln!();
            if join.own.is_empty() {
                outln!(
                    "{}",
                    if join.came.is_empty() {
                        "the files hold what this machine last kept"
                    } else {
                        "nothing changed here but what the join brought"
                    }
                );
            } else {
                for line in files_said(join.own) {
                    outln!("{line}");
                }
            }
            if !join.came.is_empty() {
                outln!();
                outln!("came with the join, from {}:", join.from.join(", "));
                for line in files_said(join.came) {
                    outln!("  {line}");
                }
            }
            continue;
        }
        let before = files(&from).map_err(|error| Fail::io(error.to_string()))?;
        let after = files(&to).map_err(|error| Fail::io(error.to_string()))?;
        outln!("from  {}", said(&from));
        outln!("to    {}", said(&to));
        let mut names: Vec<&String> = before
            .keys()
            .chain(after.keys())
            .filter(|name| project.holds(name))
            .collect();
        names.sort();
        names.dedup();
        let said: Vec<(String, Vec<String>)> = names
            .into_iter()
            .map(|name| {
                let lines = changes_of(name, before.get(name), after.get(name), config);
                (name.clone(), lines)
            })
            .filter(|(_, lines)| !lines.is_empty())
            .collect();
        let any = !said.is_empty();
        if any {
            outln!();
        }
        for line in files_said(said) {
            outln!("{line}");
        }
        // What another project keeps now, said once rather than *removed*.
        let elsewhere =
            project.kept_elsewhere(before.keys().filter(|name| !after.contains_key(*name)));
        if !elsewhere.is_empty() {
            outln!();
            for (folder, count) in &elsewhere {
                outln!("{}", kept_by_another(*count, folder));
            }
        }
        if !any && elsewhere.is_empty() {
            outln!();
            match (&from, &to) {
                (State::Kept(0), State::Now) => outln!(
                    "nothing changed since SampleKit last kept the project — samplekit diff alone \
                     shows the last change it kept, and samplekit log lists them"
                ),
                _ => outln!("nothing changed between them"),
            }
        }
        // What was changed outside SampleKit since its last snapshot is said,
        // where the last change kept was shown.
        if matches!(to, State::Kept(_)) && !matches!(from, State::Now) {
            let outside = !pending(project)?.is_empty();
            if outside {
                outln!(
                    "\n{}",
                    advice("changed outside SampleKit since, not yet kept — --from 1 shows it")
                );
            }
        }
    }
    Ok(())
}

/// Files a snapshot holds that a project below keeps now, said with that
/// project: they are not removed, and neither shown nor restored here.
fn kept_by_another(count: usize, folder: &str) -> String {
    let files = if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    };
    format!(
        "{files} the project in {folder} keeps now, in a history of its own — samplekit \
         {folder} log reads it"
    )
}

/// The files of the samples a selection keeps among `targets`, read as the
/// history kept them at `named`: what `restore --at` puts back, a sample
/// deleted since among them. `None` where `named` is no snapshot kept.
pub(super) fn selected_at(
    targets: &[PathBuf],
    narrowing: &super::FilterArgs,
    named: &str,
) -> Result<Option<Vec<PathBuf>>, Fail> {
    let failed = |error: history::VcsError| Fail::io(error.to_string());
    let mut files = Vec::new();
    for project in reached(targets)? {
        let config = &project.config;
        let entries = history::entries(config).map_err(failed)?;
        // A state naming no snapshot is the caller's to say.
        let State::Kept(at) = state_named(named, &entries)? else {
            return Ok(None);
        };
        let held = tempfile::Builder::new()
            .prefix("samplekit-at-")
            .tempdir()
            .map_err(|error| Fail::io(format!("a temporary folder: {error}")))?;
        let past = history::materialize(config, &entries[at].id, &held.path().join("project"))
            .map_err(failed)?;
        let past = dunce::canonicalize(&past).unwrap_or(past);
        let root = dunce::canonicalize(config.root()).unwrap_or_default();
        let mut places: Vec<PathBuf> = project
            .within
            .iter()
            .map(|within| past.join(within.trim_end_matches('/')))
            .collect();
        if places.is_empty() {
            places.push(past.clone());
        }
        // How a sample stands is asked of today's history and model: the
        // copy of the past has neither.
        let options = super::Options::default().narrowing(narrowing.clone());
        if super::states_wanted(&super::loaded(&past)?, &options).is_some() {
            return Err(Fail::usage(
                "restore --at reads the samples as the snapshot kept them, and state is how \
                 they stand today: select by what the files held"
                    .to_string(),
            ));
        }
        for file in super::sample_files(places, narrowing.clone())? {
            let file = dunce::canonicalize(&file).unwrap_or(file);
            if let Ok(relative) = file.strip_prefix(&past) {
                files.push(super::relative_to_here_path(&root).join(relative));
            }
        }
    }
    Ok(Some(files))
}

/// `samplekit explain FILE`: a file SampleKit wrote, looked for in the
/// projects around it — the one holding it, the current folder's, those below
/// the current folder, those the workbench opened last — the first step
/// finding a project that keeps it ending the search.
pub(super) fn explain_output(file: &Path) -> Outcome {
    if !file.is_file() {
        return Err(Fail::io(format!(
            "{}: no such file or directory",
            file.display()
        )));
    }
    let Ok(hash) = history::file_hash(file) else {
        return explain_output_in(file, None);
    };
    let carried = history::carried_snapshot(file);
    let keeps = |config: &ProjectConfig| {
        !history::made_all(config, &hash)
            .unwrap_or_default()
            .is_empty()
            || carried.as_ref().is_some_and(|snapshot| {
                !history::made_from(config, snapshot)
                    .unwrap_or_default()
                    .is_empty()
            })
    };
    let around = |place: &Path| {
        let mut configs = history::projects_of(&[place.to_path_buf()]);
        if let Some(config) = configs.first() {
            configs.extend(projects_below(config.root()));
        }
        configs
    };
    let own = if samplekit::config::project_config::find(file).is_some() {
        around(file)
    } else {
        Vec::new()
    };
    let here = around(Path::new("."));
    let steps: [Box<dyn Fn() -> Vec<ProjectConfig>>; 4] = [
        Box::new(|| own.clone()),
        Box::new(|| here.clone()),
        Box::new(|| projects_below(Path::new("."))),
        Box::new(|| {
            samplekit::tui::model::recent_projects()
                .iter()
                .flat_map(|folder| history::projects_of(std::slice::from_ref(folder)))
                .collect()
        }),
    ];
    for (step_at, step) in steps.iter().enumerate() {
        let mut found: Vec<ProjectConfig> = Vec::new();
        for config in step() {
            let root = dunce::canonicalize(config.root()).unwrap_or_default();
            let seen = found
                .iter()
                .any(|held| dunce::canonicalize(held.root()).unwrap_or_default() == root);
            if !seen && keeps(&config) {
                found.push(config);
            }
        }
        if !found.is_empty() {
            for (at, config) in found.iter().enumerate() {
                if at > 0 {
                    outln!();
                }
                // Found away from the file: the project is said.
                if step_at > 0 {
                    outln!(
                        "{} is kept by the project in {}",
                        file.display(),
                        super::relative_to_here(config.root())
                    );
                }
                explain_output_in(file, Some(config.root()))?;
            }
            return Ok(());
        }
    }
    // Kept nowhere: the project holding it, or this folder's, says why.
    if !own.is_empty() || !here.is_empty() {
        return explain_output_in(file, None);
    }
    Err(Fail::usage(format!(
        "no project here or below keeps {}: run explain from the project that wrote it",
        file.display()
    )))
}

/// A file SampleKit wrote, found by its hash — or by the snapshot a figure
/// carries — in the project named, else its own or the current folder's, and
/// what changed since it was made.
fn explain_output_in(file: &Path, project: Option<&Path>) -> Outcome {
    if !file.is_file() {
        return Err(Fail::io(format!(
            "{}: no such file or directory",
            file.display()
        )));
    }
    // Where to look: the project named, the file's own, or this folder's.
    let place = project.map_or_else(
        || {
            samplekit::config::project_config::find(file)
                .map(|_| file.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        },
        Path::to_path_buf,
    );
    let Some(config) = history::projects_of(std::slice::from_ref(&place))
        .into_iter()
        .next()
    else {
        return Err(Fail::usage(format!(
            "no project here or below keeps {}: run explain from the project that wrote it",
            file.display()
        )));
    };
    let failed = |error: history::VcsError| Fail::io(error.to_string());
    let hash = history::file_hash(file).map_err(failed)?;
    let entries = history::entries(&config).map_err(failed)?;
    let number = |snapshot: &str| {
        entries
            .iter()
            .position(|entry| entry.id == snapshot)
            .map_or_else(|| "?".to_string(), |at| format!("#{}", at + 1))
    };
    // The file as a record names where it was written: from the project's
    // folder where it lies inside.
    let root = dunce::canonicalize(config.root()).unwrap_or_default();
    let here = dunce::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let written_here = here.strip_prefix(&root).map_or_else(
        |_| here.display().to_string(),
        |relative| {
            relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        },
    );
    // The same contents may have been written more than once — from a
    // snapshot `--at`, and from today's project holding the same values:
    // the record of the write that went where the file is, else the newest
    // made from the project of its day. A copy — the paper's table — was said
    // *made on purpose with --at* by the newest, a remake of a table since
    // gone stale, and its staleness went unsaid.
    let chosen = |records: Vec<history::Made>| {
        records
            .iter()
            .find(|made| made.written == written_here)
            .or_else(|| records.iter().find(|made| !made.said.contains(" --at ")))
            .or(records.first())
            .cloned()
    };
    let made = match chosen(history::made_all(&config, &hash).map_err(failed)?) {
        Some(made) => made,
        None => {
            // Written over the projects below a root that has a history of
            // its own: kept in theirs, and read from the first that holds it.
            if project.is_none() {
                for below in projects_below(config.root()) {
                    if !history::made_all(&below, &hash)
                        .unwrap_or_default()
                        .is_empty()
                    {
                        return explain_output_in(file, Some(below.root()));
                    }
                }
            }
            // A figure changed since keeps the snapshot in its metadata, and
            // is one of the figures made from it — never an export.
            let carried = history::carried_snapshot(file);
            let figure = |made: &history::Made| {
                Path::new(&made.written)
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        ["pdf", "png", "svg"].contains(&extension.to_ascii_lowercase().as_str())
                    })
            };
            let candidates = match &carried {
                Some(snapshot) => history::made_from(&config, snapshot)
                    .map_err(failed)?
                    .into_iter()
                    .filter(figure)
                    .collect(),
                None => Vec::new(),
            };
            let Some(first) = chosen(candidates) else {
                return Err(Fail::usage(match carried {
                    Some(snapshot) => format!(
                        "{} names snapshot {} of a history, and {} records no figure made from \
                         it",
                        file.display(),
                        short(&snapshot),
                        config.root().display()
                    ),
                    None => format!(
                        "{} is not a file SampleKit wrote in {}, or it was changed since: \
                         SampleKit knows a file by its exact contents",
                        file.display(),
                        config.root().display()
                    ),
                }));
            };
            outln!(
                "{} is not a file SampleKit wrote as it is now — it was changed or converted \
                 since — but it carries the snapshot it was made from, {}",
                file.display(),
                number(&first.snapshot)
            );
            first
        }
    };
    let taken = Entry {
        id: made.snapshot.clone(),
        seconds: made.seconds,
        offset: made.offset,
        message: String::new(),
        changed: Vec::new(),
        machine: String::new(),
        machine_id: String::new(),
        host: String::new(),
        parents: Vec::new(),
        joined: Vec::new(),
        clock_went_back: false,
    };
    outln!("{}", file.display());
    outln!("  made      {}, by {}", when(&taken), made.said);
    outln!("  written   {}", native(&made.written));
    outln!(
        "  from      snapshot {}, {}",
        number(&made.snapshot),
        counted_as(made.samples.len(), "sample", "samples")
    );
    let then = history::files_at(&config, &made.snapshot).map_err(failed)?;
    let now = history::files_now(&config).map_err(failed)?;
    // Whose fields can be told, it was made from its samples' files and the
    // configuration: a table, an export or a declared figure runs no code of
    // the model, whose change reaches it only through values written since.
    let model = if fields_written(&made, &config).is_some() {
        history::model_names(&config)
    } else {
        std::collections::BTreeSet::new()
    };
    // What it was made from: its samples, the configuration and the model.
    let concerned = |name: &str| {
        made.samples.iter().any(|sample| sample == name)
            || !(name.ends_with(".md") && name != ".samplekitrc")
                && !model.contains(name)
                && !(!model.is_empty() && name.starts_with("@model/"))
    };
    let mut names: Vec<&String> = then
        .keys()
        .chain(now.keys())
        .filter(|name| concerned(name))
        .collect();
    names.sort();
    names.dedup();
    // A file is current or not by the fields it writes and by what they are
    // computed from: `set ore_mass=6.2` made an overview holding no
    // ore_mass *not current*. Where its fields cannot be told — a script, the
    // workbench — every field of its samples counts, as before.
    let read = fields_written(&made, &config).map(|written| {
        let samples: Vec<&Vec<u8>> = made
            .samples
            .iter()
            .flat_map(|sample| [then.get(sample), now.get(sample)])
            .flatten()
            .collect();
        read_through(written, &samples)
    });
    // A configuration changed only where the file reads nothing — another
    // export, a figure, the workbench's colours — leaves it current: said
    // aside, it no longer made every export *not current*.
    let reads = sections_read(&made, &config);
    let mut aside: Vec<String> = Vec::new();
    let names: Vec<&String> = names
        .into_iter()
        .filter(|name| {
            let unread = name.as_str() == ".samplekitrc"
                && reads.as_ref().is_some_and(|reads| {
                    !configuration_read_changed(
                        then.get(name.as_str()),
                        now.get(name.as_str()),
                        reads,
                    )
                })
                && then.get(name.as_str()) != now.get(name.as_str());
            if unread {
                aside.push(format!("{name}  changed, in nothing it reads"));
            }
            !unread
        })
        .collect();
    let lines = files_said(
        names
            .into_iter()
            .map(|name| {
                let sample = name.ends_with(".md") && name != ".samplekitrc";
                let lines = match &read {
                    Some(read) if sample => {
                        changes_among(name, then.get(name), now.get(name), &config, &|field| {
                            read.contains(field)
                        })
                    }
                    _ => changes_of(name, then.get(name), now.get(name), &config),
                };
                (name.clone(), lines)
            })
            .filter(|(_, lines)| !lines.is_empty())
            .collect(),
    );
    let added = now
        .keys()
        .filter(|name| name.ends_with(".md") && !then.contains_key(*name))
        .count();
    // Its selection run again, and its declaration compared: a sample added, or
    // one changed into or out of the filter, is a file that is no longer what
    // its command makes. Said before the verdict, which accounts for it.
    let again = reselect(&made, &config, &then);
    let mut selection: Vec<String> = Vec::new();
    match &again {
        Some(again) => {
            if !again.taken.is_empty() {
                selection.push(format!(
                    "its selection would now also take {}",
                    again.taken.join(", ")
                ));
            }
            if !again.dropped.is_empty() {
                selection.push(format!(
                    "its selection would now leave out {}",
                    again.dropped.join(", ")
                ));
            }
            if let Some(declaration) = &again.declaration {
                selection.push(format!(
                    "{declaration} changed since, which makes it differently"
                ));
            }
        }
        None if added > 0 => selection.push(format!(
            "{} added to the project since, which the selection that made it may now take",
            counted_as(added, "sample was", "samples were")
        )),
        None => {}
    }
    // Only a sample added, where the selection could not be run again, is a
    // file that may still be current.
    let differs = !lines.is_empty()
        || again.as_ref().is_some_and(|again| {
            !again.taken.is_empty() || !again.dropped.is_empty() || again.declaration.is_some()
        });
    outln!();
    if !lines.is_empty() || !aside.is_empty() {
        outln!("since then");
        for line in lines.iter().chain(&aside) {
            outln!("  {line}");
        }
        outln!();
    }
    for line in &selection {
        outln!("{line}");
    }
    if !selection.is_empty() {
        outln!();
    }
    // Nothing changed since is not current where it was written from values
    // not current then, and still: an export over outdated values was said
    // up to date.
    let behind: Vec<String> = if differs {
        Vec::new()
    } else {
        let written = fields_written(&made, &config);
        let mut behind: Vec<String> = made
            .samples
            .iter()
            .filter_map(|name| {
                samplekit::format::document::load_sample(&config.root().join(name))
                    .ok()
                    .map(|(sample, _)| sample)
            })
            .flat_map(|sample| samplekit::collection::editing::not_current(&sample))
            .filter(|value| {
                written.as_ref().is_none_or(|written| {
                    written.iter().any(|field| {
                        field == &value.name
                            || field.starts_with(&format!("{}.", value.name))
                            || field.starts_with(&format!("{}[", value.name))
                    })
                })
            })
            .map(|value| value.name)
            .collect();
        behind.sort();
        behind.dedup();
        behind
    };
    if !differs && !behind.is_empty() {
        outln!(
            "nothing it was made from has changed since, but it was written from values not \
             current: {} — samplekit status says why; computed, {} makes it again",
            behind.join(", "),
            made.said
        );
    } else if !differs {
        if selection.is_empty() {
            outln!("nothing it was made from has changed since: it is up to date");
        } else {
            outln!("nothing it was made from has changed since");
        }
    } else if made.said.contains(" --at ") {
        outln!(
            "it was made from the project as it was then, on purpose: the same command \
             without --at makes it from today's"
        );
    } else if made.said.starts_with("samplekit ") {
        outln!("it is not current — {} makes it again", made.said);
    } else {
        outln!(
            "it is not current — what made it, {}, makes it again",
            made.said
        );
    }
    // The command that made it, run on the project as it was then — where
    // one project's history holds it: `--at` reads one, and a file written
    // over several was made from a snapshot of each.
    if made.said.starts_with("samplekit ") {
        let spanned = projects_spanned(&made, &root);
        if spanned.len() > 1 {
            outln!(
                "written over {} projects, {}: no one snapshot holds them all, and --at reads \
                 one project's history, so it is not made again as it was; {} makes it from \
                 today's",
                spanned.len(),
                spanned.join(", "),
                made.said
            );
        } else {
            outln!("{}", made_again(&made, &root));
        }
    }
    Ok(())
}

/// The projects in the folders below `root`, nearest first: each folder
/// holding a `.samplekitrc`, hidden folders and environments passed over.
pub(super) fn projects_below(root: &Path) -> Vec<samplekit::config::project_config::ProjectConfig> {
    let mut found = Vec::new();
    let mut folders = vec![(root.to_path_buf(), 0usize)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let mut below: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.') && name != "node_modules")
            })
            .collect();
        below.sort();
        for path in below {
            if path.join("pyvenv.cfg").is_file() {
                continue;
            }
            if path
                .join(samplekit::config::project_config::FILENAME)
                .is_file()
            {
                found.extend(history::projects_of(std::slice::from_ref(&path)));
            }
            if depth < 4 {
                folders.push((path, depth + 1));
            }
        }
    }
    found
}

/// The projects a record's command read, by their folders as the command
/// names them: each word naming a file or folder from where it ran, an
/// output left out.
fn projects_spanned(made: &history::Made, root: &Path) -> Vec<String> {
    let Ok(words) = shell_words::split(&made.said) else {
        return Vec::new();
    };
    let ran_in = Path::new(made.ran_in.trim_end_matches('/'));
    let base = if ran_in.is_absolute() {
        ran_in.to_path_buf()
    } else {
        root.join(ran_in)
    };
    let mut places: Vec<PathBuf> = Vec::new();
    let mut output = false;
    for word in words.iter().skip(1) {
        if output {
            output = false;
            continue;
        }
        if word == "-o" || word == "--output" {
            output = true;
            continue;
        }
        if word.starts_with('-') {
            continue;
        }
        let place = base.join(word);
        if place.exists() {
            places.push(place);
        }
    }
    let mut roots: Vec<String> = Vec::new();
    for config in history::projects_of(&places) {
        let root =
            dunce::canonicalize(config.root()).unwrap_or_else(|_| config.root().to_path_buf());
        let shown = root
            .strip_prefix(dunce::canonicalize(&base).unwrap_or_else(|_| base.clone()))
            .ok()
            .filter(|below| !below.as_os_str().is_empty())
            .map_or_else(
                || root.display().to_string(),
                |below| below.display().to_string(),
            );
        if !roots.contains(&shown) {
            roots.push(shown);
        }
    }
    roots
}

/// The command a snapshot says, read as the command line reads it, in
/// whatever order it was typed.
fn said_parsed(said: &str) -> Option<Cli> {
    let words = shell_words::split(said).ok()?;
    let (program, rest) = words.split_first()?;
    let rest = super::path_first(rest).map_or_else(|| rest.to_vec(), |(moved, _, _)| moved);
    Cli::try_parse_from(std::iter::once(program.clone()).chain(rest)).ok()
}

/// The declarations of `.samplekitrc` a file SampleKit wrote reads, as
/// `(section, name)`: its export, profile, queries and style. `None` where
/// they cannot be told — a figure, a script, the workbench — and every
/// change then counts.
fn sections_read(made: &history::Made, config: &ProjectConfig) -> Option<Vec<(String, String)>> {
    if !made.said.starts_with("samplekit ") {
        return None;
    }
    let cli = said_parsed(&made.said)?;
    let declared =
        samplekit::config::project_config::load(&config.root().join(".samplekitrc")).ok();
    let mut read: Vec<(String, String)> = Vec::new();
    let mut add = |section: &str, name: &str| read.push((section.to_string(), name.to_string()));
    match cli.command {
        None => {
            let query = cli.query;
            for name in &query.selection.narrowing.queries {
                add("query", name);
            }
            if let Some(profile) = &query.profile {
                add("profile", profile);
            }
            if let Some(style) = &query.presentation.style {
                add("style", style);
            }
            // A document's format is its style's, by the file's extension.
            if let Some(extension) = query
                .output
                .as_deref()
                .and_then(Path::extension)
                .and_then(|extension| extension.to_str())
            {
                add("style", extension);
                if let Some(config) = &declared {
                    for name in config.text_format_names() {
                        if config
                            .text_format(name)
                            .is_some_and(|text| text.extension == extension)
                        {
                            add("style", name);
                        }
                    }
                }
            }
        }
        Some(Commands::Export(args)) => {
            for name in &args.selection.narrowing.queries {
                add("query", name);
            }
            if let Some(style) = &args.style {
                add("style", style);
            }
            add("export", &args.export);
            match declared.as_ref()?.export(&args.export).ok()? {
                Export::Table(export) => {
                    add("profile", &export.profile);
                    if let Some(query) = &export.query {
                        add("query", query);
                    }
                    if let Some(style) = &export.style {
                        add("style", style);
                    }
                    if let Format::Text(text) = &export.format {
                        add("style", &text.name);
                    }
                }
                // A figure's: the figure it draws, its query and the style
                // its labels use.
                Export::Figure(export) => {
                    add("figure", &export.figure);
                    if let Some(query) = &export.query {
                        add("query", query);
                    }
                    if let Some(style) = &export.style {
                        add("style", style);
                    }
                }
            }
        }
        _ => return None,
    }
    Some(read)
}

/// Whether `.samplekitrc` changed in what a file reads: a declaration it
/// names, or anything outside the declarations by name — the units, the
/// properties, the collection — which every file may read. The workbench's
/// colours and the declarations it does not name are not.
fn configuration_read_changed(
    before: Option<&Vec<u8>>,
    after: Option<&Vec<u8>>,
    reads: &[(String, String)],
) -> bool {
    let table = |bytes: Option<&Vec<u8>>| {
        bytes.and_then(|bytes| String::from_utf8_lossy(bytes).parse::<toml::Table>().ok())
    };
    let (Some(before), Some(after)) = (table(before), table(after)) else {
        return true;
    };
    const NAMED: [&str; 5] = ["export", "profile", "query", "figure", "style"];
    const UNREAD: [&str; 1] = ["colors"];
    let mut keys: Vec<&String> = before.keys().chain(after.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter().any(|key| {
        let (was, is) = (before.get(key), after.get(key));
        if was == is || UNREAD.contains(&key.as_str()) {
            return false;
        }
        if !NAMED.contains(&key.as_str()) {
            return true;
        }
        let names = |value: Option<&toml::Value>| -> Vec<String> {
            value
                .and_then(toml::Value::as_table)
                .map(|table| table.keys().cloned().collect())
                .unwrap_or_default()
        };
        let mut all = names(was);
        all.extend(names(is));
        all.into_iter().any(|name| {
            let entry = |value: Option<&toml::Value>| {
                value
                    .and_then(toml::Value::as_table)
                    .and_then(|table| table.get(&name))
                    .cloned()
            };
            entry(was) != entry(is)
                && reads
                    .iter()
                    .any(|(section, read)| section == key && *read == name)
        })
    })
}

/// The names a file SampleKit wrote holds values of — its columns, its sort
/// keys, which order its rows, a figure's axes and group — read from the
/// command that made it and the project's declarations, by the name each is
/// held under. `None` where they cannot be told: a script, the workbench, a
/// command no longer read. A file of paths holds none.
fn fields_written(made: &history::Made, config: &ProjectConfig) -> Option<Vec<String>> {
    if !made.said.starts_with("samplekit ") {
        return None;
    }
    let cli = said_parsed(&made.said)?;
    let declared =
        samplekit::config::project_config::load(&config.root().join(".samplekitrc")).ok();
    let mut fields: Vec<String> = Vec::new();
    let of_profile = |name: &str, fields: &mut Vec<String>| -> Option<()> {
        let profile = declared.as_ref()?.profile(name).ok()?;
        fields.extend(profile.columns.iter().map(|column| column.field.clone()));
        fields.extend(profile.sort.iter().cloned());
        // A file of its groups holds them as columns.
        fields.extend(profile.group.iter().cloned());
        Some(())
    };
    let sorted = |selection: &SelectionArgs, fields: &mut Vec<String>| {
        fields.extend(
            selection
                .sorts
                .iter()
                .flat_map(|keys| keys.0.iter().cloned()),
        );
    };
    match cli.command {
        None => {
            let query = cli.query;
            if let Some(profile) = &query.profile {
                of_profile(profile, &mut fields)?;
            }
            fields.extend(
                query
                    .columns
                    .iter()
                    .flat_map(|list| list.0.iter().map(|column| column.field.clone())),
            );
            fields.extend(
                query
                    .group
                    .iter()
                    .flat_map(|group| group.split(',').map(|field| field.trim().to_string())),
            );
            sorted(&query.selection, &mut fields);
        }
        Some(Commands::Export(args)) => {
            match declared.as_ref()?.export(&args.export).ok()? {
                Export::Table(export) => of_profile(&export.profile.clone(), &mut fields)?,
                // A figure's export draws its figure's axes and group; the
                // model's figure may draw from what it names alone.
                Export::Figure(export) => {
                    let figure = declared.as_ref()?.figure(&export.figure)?;
                    fields.extend([figure.x.clone(), figure.y.clone()]);
                    fields.extend(figure.group.iter().cloned());
                }
            }
            sorted(&args.selection, &mut fields);
        }
        Some(Commands::Plot(args)) => {
            let figure = args
                .figure
                .as_ref()
                .filter(|_| args.x.is_none())
                .and_then(|name| declared.as_ref()?.figure(name));
            if let Some(figure) = figure {
                fields.extend([figure.x.clone(), figure.y.clone()]);
                fields.extend(figure.group.iter().cloned());
            }
            fields.extend(args.x.iter().chain(&args.y).chain(&args.group).cloned());
            // A figure's own declaration may draw from what it names alone.
            if fields.is_empty() {
                return None;
            }
            sorted(&args.selection, &mut fields);
        }
        _ => return None,
    }
    Some(fields)
}

/// `written`, and what each is computed from, as the records of `samples`
/// say it — through every step: an export of `modulus` reads `radius` through it.
fn read_through(written: Vec<String>, samples: &[&Vec<u8>]) -> std::collections::BTreeSet<String> {
    let edges: Vec<(String, String)> = samples
        .iter()
        .flat_map(|bytes| computed_from(bytes))
        .collect();
    let mut read: std::collections::BTreeSet<String> = written
        .iter()
        .map(|field| field_base(field).to_string())
        .collect();
    loop {
        let more: Vec<String> = edges
            .iter()
            .filter(|(what, from)| read.contains(what) && !read.contains(from))
            .map(|(_, from)| from.clone())
            .collect();
        if more.is_empty() {
            return read;
        }
        read.extend(more);
    }
}

/// The command that makes a file again as it was, from the folder it was
/// run in, with `--at` and the snapshot's id. An export `--at` a snapshot
/// writes only where `-o` says, so one written to its declared file is given
/// `-o` and where it was written. Where that file holds something else now —
/// a later export, the copy explained being an old one — the command writes
/// beside it, named with the snapshot's id: run as it is said, it replaced the
/// file as it is now with the file as it was.
fn made_again(made: &history::Made, root: &Path) -> String {
    let words = shell_words::split(&made.said)
        .unwrap_or_else(|_| made.said.split(' ').map(str::to_string).collect());
    let there = root.join(&made.written);
    let replaced =
        there.exists() && history::file_hash(&there).map_or(true, |hash| hash != made.hash);
    let beside = |written: &str| -> String {
        if !replaced {
            return written.to_string();
        }
        let path = Path::new(written);
        let stem = path
            .file_stem()
            .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let named = match path.extension() {
            Some(extension) => format!(
                "{stem}-{}.{}",
                short(&made.snapshot),
                extension.to_string_lossy()
            ),
            None => format!("{stem}-{}", short(&made.snapshot)),
        };
        path.with_file_name(named).to_string_lossy().into_owned()
    };
    let mut kept: Vec<String> = Vec::new();
    let mut skip = false;
    let mut destination = false;
    for word in &words {
        if skip {
            skip = false;
        } else if destination {
            kept.push(beside(word));
            destination = false;
        } else if word == "--at" {
            skip = true;
        } else if word == "-o" || word == "--output" {
            kept.push(word.clone());
            destination = true;
        } else if let Some(path) = word.strip_prefix("--output=") {
            kept.push(format!("--output={}", beside(path)));
        } else if !word.starts_with("--at=") {
            kept.push(word.clone());
        }
    }
    // `export` as the command, written first or after its paths.
    let export = kept.get(1).is_some_and(|word| word == "export")
        || super::path_first(&kept[1.min(kept.len())..])
            .is_some_and(|(moved, _, _)| moved.first().is_some_and(|word| word == "export"));
    let output = kept
        .iter()
        .any(|word| word == "-o" || word == "--output" || word.starts_with("--output="));
    if export && !output {
        // `written` is from the project's folder, the command from the
        // folder it ran in.
        let ran_in = made.ran_in.trim_end_matches('/');
        let written = if Path::new(ran_in).is_absolute() {
            // Run from a folder outside the project, kept whole: the file
            // from there, where it lies below it, else whole too. Its
            // components were counted as steps up from the project.
            let file = root.join(&made.written);
            let from = dunce::canonicalize(ran_in).unwrap_or_else(|_| PathBuf::from(ran_in));
            let file = dunce::canonicalize(&file).unwrap_or(file);
            file.strip_prefix(&from)
                .map(|below| below.to_string_lossy().into_owned())
                .unwrap_or_else(|_| file.to_string_lossy().into_owned())
        } else if ran_in.is_empty() || ran_in == "." || Path::new(&made.written).is_absolute() {
            made.written.clone()
        } else {
            made.written
                .strip_prefix(&format!("{ran_in}/"))
                .map_or_else(
                    || {
                        let ups = ran_in.split('/').count();
                        format!("{}{}", "../".repeat(ups), made.written)
                    },
                    str::to_string,
                )
        };
        kept.push("-o".to_string());
        // As the system writes a path: the history keeps it with `/`.
        kept.push(native(&beside(&written)));
    }
    let place = if made.ran_in.is_empty() || made.ran_in == "." {
        "from the project's folder".to_string()
    } else {
        format!("from {}", made.ran_in)
    };
    let quoted: Vec<String> = kept.iter().map(|word| shell_word(word)).collect();
    format!(
        "made again as it was, {place}: {} --at {}",
        quoted.join(" "),
        short(&made.snapshot)
    )
}

/// `samplekit explain SAMPLE FIELD --at WHEN`: the value as the history kept it
/// — the file of that snapshot read under the project's configuration, its
/// states as its records said then.
pub(super) fn explain_at(options: &Options, named: &str) -> Outcome {
    let Some(first) = options.positionals.first() else {
        return explain(options);
    };
    let file = PathBuf::from(first);
    let Some(config) = history::projects_of(std::slice::from_ref(&file))
        .into_iter()
        .next()
    else {
        return Err(Fail::usage(format!(
            "{first} is in no project: no .samplekitrc above it keeps a history"
        )));
    };
    let failed = |error: history::VcsError| Fail::io(error.to_string());
    let entries = history::entries(&config).map_err(failed)?;
    let at = match state_named(named, &entries)? {
        State::Now | State::Before(_) => None,
        State::Kept(at) => Some(at),
    };
    let root = dunce::canonicalize(config.root()).unwrap_or_default();
    let name = dunce::canonicalize(&file)
        .ok()
        .and_then(|path| {
            path.strip_prefix(&root).ok().map(|relative| {
                relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
        })
        .ok_or_else(|| Fail::usage(format!("{first} is not a file of {}", root.display())))?;
    let files = match at {
        Some(at) => history::files_at(&config, &entries[at].id).map_err(failed)?,
        None => history::files_now(&config).map_err(failed)?,
    };
    // A field is what explain explains: without one, the fields the sample
    // held then are what to name, the first given as the example.
    if options.positionals.get(1).is_none() {
        let field = files
            .get(&name)
            .and_then(|bytes| {
                let parsed = document::parse(&String::from_utf8_lossy(bytes)).ok()?;
                let sample = samplekit::format::schema::into_sample(parsed.schema).ok()?;
                sample.property_names().first().map(|name| name.to_string())
            })
            .unwrap_or_else(|| "FIELD".to_string());
        return Err(Fail::usage(format!(
            "explain --at shows one value as it was, and wants its field: samplekit {first} \
             explain {field} --at {named}"
        )));
    }
    let Some(at) = at else {
        return explain(options);
    };
    let Some(bytes) = files.get(&name) else {
        return Err(Fail::usage(format!(
            "{first} was not in the project at #{} · {}",
            at + 1,
            when(&entries[at])
        )));
    };
    let _ = bytes;
    // The project as the snapshot kept it — its configuration and its model
    // then — and the history it has: a value is judged by the formula that
    // computed it then, against the model then. The sample alone, under
    // today's model, called a value of its day's formula outdated. A folder
    // of its own name, removed when it is dropped.
    let held = tempfile::Builder::new()
        .prefix("samplekit-at-")
        .tempdir()
        .map_err(|error| Fail::io(format!("a temporary folder: {error}")))?;
    let written = history::materialize(&config, &entries[at].id, &held.path().join("project"))
        .map_err(failed)?;
    let copy = written.join(&name);
    // Nothing read from the past writes history of its own.
    // SAFETY: set before any thread is started, and read by the children.
    unsafe { std::env::set_var("SAMPLEKIT_HISTORY", "off") };
    history::read_history_of(&written, config.root());
    super::PAST.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = super::PAST_TYPED.set(first.clone());
    outln!(
        "as kept at #{} · {} · {}\n",
        at + 1,
        when(&entries[at]),
        entries[at].message
    );
    let mut then = options.clone();
    then.positionals[0] = copy.to_string_lossy().into_owned();
    let explained = explain(&then);
    drop(held);
    explained
}

/// `samplekit restore`: the files named put back as the history kept them — by default as they
/// were before the last change kept of them — a file named that was deleted or changed outside
/// SampleKit since as last kept — or as the state `at` names — previewed value by value, written
/// with `--write`, which is a snapshot of its own. A model lying outside the project is kept,
/// never put back.
pub(super) fn restore(targets: &[PathBuf], at: Option<&str>, write: bool) -> Outcome {
    let projects = reached(targets)?;
    // A snapshot is one project's: its number names another moment in each
    //.
    if let Some(named) = at {
        one_project(&projects, "--at", &format!("restore --at {named}"))?;
    }
    // Whether anything would be written: the advice to pass --write is
    // given only then.
    let mut to_restore = false;
    for (index, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if index > 0 {
                outln!();
            }
            outln!("{}", super::relative_to_here(project.config.root()));
        }
        let config = &project.config;
        let entries = history::entries(config).map_err(|error| Fail::io(error.to_string()))?;
        // A state named is checked first: one naming nothing, a history not
        // yet kept among them, is refused.
        let named = match at {
            Some(named) => match state_named(named, &entries)? {
                State::Kept(at) => Some(at),
                State::Now | State::Before(_) => {
                    return Err(Fail::usage(
                        "restore --at names a snapshot to put the files back to, and now is \
                         where they already are: a number from samplekit log, the beginning of \
                         a snapshot's id, or a date"
                            .to_string(),
                    ));
                }
            },
            None => None,
        };
        // Nothing to do is said so, and exits 0.
        if entries.is_empty() {
            outln!("nothing to change — {NO_HISTORY}");
            continue;
        }
        let now = history::files_now(config).map_err(|error| Fail::io(error.to_string()))?;
        // A file named that the disk holds otherwise than the last snapshot —
        // deleted since, or changed outside SampleKit — and that the snapshot
        // holds: its last content kept is what comes back, taking back only
        // what was done outside. The snapshot before the last change kept of it
        // was the one before its last edit, which lost that edit too.
        let named_files: Vec<String> = project
            .within
            .iter()
            .filter(|within| !within.is_empty() && !within.ends_with('/'))
            .cloned()
            .collect();
        // As the history last kept them: this machine's last snapshot, with
        // what the other machines' it would join changed.
        let kept = if named.is_some() {
            None
        } else {
            history::last_kept(config).map_err(|error| Fail::io(error.to_string()))?
        };
        // A sample deleted or changed outside SampleKit since the last
        // snapshot comes back where it is named; otherwise it stays, and is
        // said, the last change SampleKit wrote taken back field by field
        // around it. A file made by hand is no change of a kept one, and a
        // configuration edited by hand stays.
        let by_hand: Vec<String> = match &kept {
            Some(kept) if named.is_none() && named_files.is_empty() => {
                let mut differing: Vec<String> = kept
                    .files
                    .keys()
                    .filter(|name| {
                        !name.starts_with('@')
                            && !name.ends_with(samplekit::config::project_config::FILENAME)
                            && project.holds(name)
                            && kept.files.get(*name) != now.get(*name)
                    })
                    .cloned()
                    .collect();
                differing.sort();
                differing
            }
            _ => Vec::new(),
        };
        // Each said, and how it comes back: naming it.
        let say_by_hand = || {
            for name in &by_hand {
                outln!(
                    "  {name}, {} since, stays — samplekit {name} restore brings back its last \
                     content kept",
                    if now.contains_key(name) {
                        "changed outside SampleKit"
                    } else {
                        "deleted"
                    }
                );
            }
        };
        let files: Vec<&String> = named_files.iter().collect();
        let (gone, outside): (Vec<&String>, Vec<&String>) = match &kept {
            Some(kept) => files
                .iter()
                .filter(|within| kept.files.get(**within) != now.get(**within))
                .partition(|within| !now.contains_key(**within)),
            None => (Vec::new(), Vec::new()),
        };
        // Named by the snapshot the first of them is as.
        let latest = kept
            .as_ref()
            .filter(|kept| {
                !(gone.is_empty() && outside.is_empty())
                    && gone
                        .iter()
                        .chain(&outside)
                        .all(|within| kept.files.contains_key(*within))
            })
            .map(|kept| {
                let by = gone
                    .iter()
                    .chain(&outside)
                    .find_map(|within| kept.by.get(*within));
                let at = by
                    .and_then(|id| entries.iter().position(|entry| entry.id == *id))
                    .unwrap_or(0);
                (at, kept.files.clone())
            });
        let has_latest = latest.is_some();
        // Without a state named, the last change is taken back rather than a
        // state put back.
        let mut taking_back = false;
        let (source, then) = match (named, latest) {
            (Some(at), _) => (
                at,
                history::files_at(config, &entries[at].id)
                    .map_err(|error| Fail::io(error.to_string()))?,
            ),
            (None, Some(latest)) => latest,
            (None, None) => {
                // The last change SampleKit wrote here, taken back field by
                // field: what a colleague or an editor changed since stays.
                // A file named that no change SampleKit wrote touched, and that
                // was changed by hand since: that change, kept, is taken back.
                let last =
                    history::last_own_change(&entries, |name| project.holds(name)).or_else(|| {
                        (!named_files.is_empty())
                            .then(|| {
                                history::last_outside_change(&entries, |name| {
                                    named_files.iter().any(|named| named == name)
                                })
                            })
                            .flatten()
                    });
                let Some(last) = last else {
                    outln!(
                        "nothing to change — no change SampleKit wrote on this machine touched \
                         what is named here"
                    );
                    say_by_hand();
                    continue;
                };
                let failed = |error: history::VcsError| Fail::io(error.to_string());
                let before = history::files_before(config, &entries[last].id).map_err(failed)?;
                let after = history::files_at(config, &entries[last].id).map_err(failed)?;
                let mut then = now.clone();
                let mut conflicts = Vec::new();
                for name in entries[last]
                    .changed
                    .iter()
                    .filter(|name| !name.starts_with("@model/") && project.holds(name))
                {
                    match history::taken_back(
                        name,
                        before.get(name).map(Vec::as_slice),
                        after.get(name).map(Vec::as_slice),
                        now.get(name).map(Vec::as_slice),
                    ) {
                        Ok(Some(bytes)) => {
                            then.insert(name.clone(), bytes);
                        }
                        Ok(None) => {
                            then.remove(name);
                        }
                        Err(found) => conflicts.extend(found),
                    }
                }
                if !conflicts.is_empty() {
                    // The snapshot that holds the project as it was before
                    // this change: the parent whose files are those. A change
                    // that joined another computer's work has none, and its
                    // own line's last state takes that work back too — the
                    // advice said otherwise, and undid a colleague's formula.
                    let parents: Vec<usize> = entries[last]
                        .parents
                        .iter()
                        .filter_map(|id| entries.iter().position(|entry| entry.id == *id))
                        .collect();
                    let held_before = |at: usize| {
                        history::files_at(config, &entries[at].id).is_ok_and(|files| {
                            files
                                .iter()
                                .filter(|(name, _)| project.holds(name))
                                .eq(before.iter().filter(|(name, _)| project.holds(name)))
                        })
                    };
                    // What it set first, by hand: `diff` shows what the
                    // change made of each value, and `restore --at` is said
                    // for what it reaches — the files named, or the project —
                    // and for what it takes back with them.
                    let shown = format!(
                        "set those values again by hand: samplekit diff --from {} --to {} shows \
                         what this change made of them",
                        last + 2,
                        last + 1
                    );
                    let reach = if named_files.is_empty() {
                        "the project".to_string()
                    } else {
                        named_files.join(", ")
                    };
                    let advice = match parents.iter().copied().find(|at| held_before(*at)) {
                        Some(parent) => format!(
                            "{shown}; samplekit restore --at {} puts {reach} back as it was \
                             before it, and takes back with it what was changed there since",
                            parent + 1
                        ),
                        None => match parents.first() {
                            Some(parent) if parents.len() > 1 => format!(
                                "set those values again by hand: it joined another computer's \
                                 work, and samplekit restore --at {} — this computer's state before \
                                 it — takes that work back too",
                                parent + 1
                            ),
                            _ => shown.clone(),
                        },
                    };
                    return Err(Fail::data(format!(
                        "restore cannot take back {}: what it changed was changed since\n{}\n  {}",
                        snapshot_said(&entries, last),
                        conflicts
                            .iter()
                            .map(|conflict| format!("  {conflict}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                        advice
                    )));
                }
                taking_back = true;
                (last, then)
            }
        };
        let mut names: Vec<&String> = then
            .keys()
            .chain(now.keys())
            .filter(|name| !name.starts_with("@model/") && project.holds(name))
            .filter(|name| then.get(*name) != now.get(*name))
            .collect();
        names.sort();
        names.dedup();
        // Its number moves on as soon as the restore is itself a snapshot:
        // its id does not.
        // What and where, as every preview's first line says it: the change
        // taken back, or the state gone back to.
        if taking_back {
            outln!("restore · take back {}", snapshot_said(&entries, source));
            say_by_hand();
        } else {
            outln!("restore · back to {}", snapshot_said(&entries, source));
        }
        if has_latest {
            let joined = |names: &[&String]| {
                names
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            if !gone.is_empty() {
                outln!(
                    "  the last content kept of {}, deleted since",
                    joined(&gone)
                );
            }
            if !outside.is_empty() {
                outln!(
                    "  the last content kept of {}, changed outside SampleKit since",
                    joined(&outside)
                );
            }
        }
        let elsewhere = project.kept_elsewhere(then.keys().filter(|name| !now.contains_key(*name)));
        for (folder, count) in &elsewhere {
            outln!("  {}", kept_by_another(*count, folder));
        }
        if names.is_empty() {
            outln!("\nnothing to change — the files are already as it kept them");
            continue;
        }
        to_restore = true;
        outln!();
        let said: Vec<(String, Vec<String>)> = names
            .iter()
            .map(|name| {
                // A file gone that the state held is brought back, not new.
                // Its values said too: `restored` alone hid what came back.
                let lines = if !now.contains_key(*name) {
                    let mut lines = vec![format!("{name}   restored")];
                    lines.extend(
                        changes_of(
                            name,
                            Some(&b"---\nschema_version: 1\n---\n".to_vec()),
                            then.get(*name),
                            config,
                        )
                        .into_iter()
                        .filter(|line| line.starts_with(' ')),
                    );
                    lines
                } else {
                    changes_of(name, now.get(*name), then.get(*name), config)
                };
                ((*name).clone(), lines)
            })
            .collect();
        for line in files_said(said) {
            outln!("{line}");
        }
        // What the files put back make not current, as a `set`'s preview says
        // it: readings kept new under a value put back made three values
        // outdated, and nothing said so.
        let mut newly: Vec<(String, String)> = Vec::new();
        for name in names.iter().filter(|name| name.ends_with(".md")) {
            let (Some(after), before) = (
                then.get(*name)
                    .and_then(|bytes| samplekit::presentation::changes::sample_of(bytes)),
                now.get(*name)
                    .and_then(|bytes| samplekit::presentation::changes::sample_of(bytes)),
            ) else {
                continue;
            };
            let was = before
                .as_ref()
                .map(samplekit::collection::editing::not_current)
                .unwrap_or_default();
            for value in samplekit::collection::editing::newly_not_current(
                &was,
                &samplekit::collection::editing::not_current(&after),
            ) {
                // Each in its file, its state in its colour, said as `set`
                // says them: the names padded to the longest.
                newly.push((
                    format!("{name}  {}", value.name),
                    render::painted_state(&super::state_of(&after, &value.state, value.rows)),
                ));
            }
        }
        if !newly.is_empty() {
            outln!();
            for line in samplekit::presentation::changes::not_current_lines(&newly, 0) {
                outln!("{line}");
            }
        }
        if !write {
            continue;
        }
        // One by one: where one fails, those put back before it are said,
        // as the history's own snapshot of this restore will keep them.
        let mut done: Vec<&String> = Vec::new();
        for name in &names {
            if super::STOP.load(std::sync::atomic::Ordering::SeqCst) {
                let done: Vec<String> = done.iter().map(|name| name.to_string()).collect();
                return Err(super::stopped(&done));
            }
            let path = config.root().join(name);
            // Each file as it was read above, or not at all: another writer's
            // change since is refused, not lost, and a file is replaced
            // atomically, as every save is.
            let written = path
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .map_err(|source| document::DocumentError::Io {
                    path: path.clone(),
                    source,
                })
                .and_then(|()| {
                    document::replace_if_unchanged(
                        &path,
                        now.get(*name).map(Vec::as_slice),
                        then.get(*name).map(Vec::as_slice),
                    )
                });
            if let Err(error) = written {
                let changed = !matches!(error, document::DocumentError::Io { .. });
                let mut message = match error {
                    document::DocumentError::Io { source, .. } => {
                        format!("{}: {source}", path.display())
                    }
                    other => other.to_string(),
                };
                if done.is_empty() {
                    message.push_str("\n  nothing was restored");
                } else {
                    message.push_str(&format!(
                        "\n  restored before it: {}",
                        done.iter()
                            .map(|name| name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                return Err(if changed {
                    Fail::data(message)
                } else {
                    Fail::io(message)
                });
            }
            done.push(name);
        }
        // The heading said to what state; this says what the disk holds now, in
        // the words every command that writes a file uses.
        outln!();
        let done: Vec<String> = done.iter().map(|name| name.to_string()).collect();
        for line in super::written_lines(&done) {
            outln!("{line}");
        }
    }
    if !write && to_restore {
        outln!();
        offer_to_apply(
            "restore these files?",
            "nothing written — pass --write to restore",
        );
    }
    Ok(())
}

/// `samplekit log --script WHEN`: the Python script kept beside that snapshot,
/// as it ran.
pub(super) fn script(targets: &[PathBuf], named: &str) -> Outcome {
    for project in reached(targets)? {
        let config = &project.config;
        let entries = history::entries(config).map_err(|error| Fail::io(error.to_string()))?;
        let State::Kept(at) = state_named(named, &entries)? else {
            return Err(Fail::usage(
                "--script names a snapshot: a number from samplekit log, the beginning of its id, \
                 or a date"
                    .to_string(),
            ));
        };
        match history::script_of(config, &entries[at].id)
            .map_err(|error| Fail::io(error.to_string()))?
        {
            Some((name, source)) => {
                errln!(
                    "{name}, as it made #{} · {} · {}",
                    at + 1,
                    when(&entries[at]),
                    entries[at].message
                );
                out!("{}", String::from_utf8_lossy(&source));
            }
            // What was asked for is not there: exit 1, whatever the reason, so
            // that `log --script N > fix.py` never leaves an empty file taken
            // for a script. Python with no file to keep: standard input, an
            // interactive session.
            None if entries[at].message.starts_with("python") => {
                return Err(Fail::usage(format!(
                    "no script is kept beside #{}: {} ran from no script file — standard \
                     input or an interactive session — so there was no source to keep",
                    at + 1,
                    entries[at].message
                )));
            }
            None => {
                return Err(Fail::usage(format!(
                    "no script is kept beside #{}: {} was not made by a Python script",
                    at + 1,
                    entries[at].message
                )));
            }
        }
    }
    Ok(())
}

/// A file's selection run again, and its declaration compared.
struct Reselected {
    /// Samples it would take now and did not, by their paths in the history.
    taken: Vec<String>,
    /// Samples it took and would leave out now.
    dropped: Vec<String>,
    /// `[export.NAME]` or `[figure.NAME]`, where it changed since.
    declaration: Option<String>,
}

/// The command that made a file, its selection run again on the project as
/// it is — its targets, its filters and queries, and its declaration's query
/// — from the folder it ran in; `None` where it was not a command line, or
/// its words no longer read.
fn reselect(
    made: &history::Made,
    config: &ProjectConfig,
    then: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Option<Reselected> {
    if !made.said.starts_with("samplekit ") || made.said.contains(" --at ") {
        return None;
    }
    let cli = said_parsed(&made.said)?;
    let (targets, selection, declared) = match cli.command {
        None => (cli.query.targets, cli.query.selection, None),
        Some(Commands::Export(args)) => {
            (args.targets, args.selection, Some(("export", args.export)))
        }
        Some(Commands::Plot(args)) => {
            let mut targets = args.targets;
            let declared = match (args.figure, &args.x) {
                (Some(figure), None) => Some(("figure", figure)),
                (Some(target), Some(_)) => {
                    targets.insert(0, PathBuf::from(target));
                    None
                }
                (None, _) => None,
            };
            (targets, args.selection, declared)
        }
        _ => return None,
    };
    let now_config =
        samplekit::config::project_config::load(&config.root().join(".samplekitrc")).ok();
    let mut queries = selection.narrowing.queries;
    if let Some((kind, name)) = &declared {
        let query = match *kind {
            "export" => now_config
                .as_ref()
                .and_then(|config| config.export(name).ok())
                .and_then(|export| export.query().map(str::to_string)),
            _ => now_config
                .as_ref()
                .and_then(|config| config.figure(name))
                .and_then(|figure| figure.query.clone()),
        };
        queries.extend(query);
    }
    // Without a target the command read the folder it ran in: run with none,
    // the implicit selection prints the help instead.
    let mut arguments: Vec<String> = if targets.is_empty() {
        vec![".".to_string()]
    } else {
        targets
            .iter()
            .map(|target| target.to_string_lossy().into_owned())
            .collect()
    };
    for filter in selection.narrowing.filters {
        arguments.extend(["--filter".to_string(), filter]);
    }
    for query in queries {
        arguments.extend(["--query".to_string(), query]);
    }
    let root = dunce::canonicalize(config.root()).ok()?;
    let ran_in = if made.ran_in.is_empty() {
        root.clone()
    } else {
        root.join(&made.ran_in)
    };
    let output = std::process::Command::new(std::env::current_exe().ok()?)
        .args(&arguments)
        .current_dir(&ran_in)
        .env("NO_COLOR", "1")
        .output()
        .ok()?;
    if output.status.code() != Some(0) {
        return None;
    }
    // One sample's path per line; anything else — an empty line, a word
    // that is no file — is not a sample taken.
    let now: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let path = ran_in.join(line);
            if !path.is_file() {
                return None;
            }
            let path = dunce::canonicalize(path).ok()?;
            path.strip_prefix(&root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        })
        .collect();
    let taken = now
        .iter()
        .filter(|name| !made.samples.contains(name))
        .cloned()
        .collect();
    let dropped = made
        .samples
        .iter()
        .filter(|name| !now.contains(name))
        .cloned()
        .collect();
    // The declaration as the snapshot kept it, against the one here now.
    let declaration = declared.and_then(|(kind, name)| {
        let section = |text: &str| {
            text.parse::<toml::Table>()
                .ok()?
                .get(kind)?
                .get(&name)
                .cloned()
        };
        let before = then
            .get(".samplekitrc")
            .and_then(|bytes| section(&String::from_utf8_lossy(bytes)));
        let after = std::fs::read_to_string(config.root().join(".samplekitrc"))
            .ok()
            .and_then(|text| section(&text));
        (before != after).then(|| format!("[{kind}.{name}]"))
    });
    Some(Reselected {
        taken,
        dropped,
        declaration,
    })
}

/// The configuration as the newest snapshot kept it, for a project whose file
/// cannot be read now; nothing declared where none was kept that reads.
fn kept_configuration(rc: &Path) -> samplekit::config::project_config::ProjectConfig {
    history::kept_configuration(rc)
}
