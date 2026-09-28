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
        let config = samplekit::config::project_config::load(&rc)
            .map_err(|error| Fail::usage(error.to_string()))?;
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
pub(super) fn log(targets: &[PathBuf], since: Option<&str>, options: &Options) -> Outcome {
    let projects = reached(targets)?;
    for (at, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if at > 0 {
                outln!();
            }
            outln!("{}", project.config.root().display());
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
        // Where more than one machine wrote the history, each snapshot's is
        // named, after *when*.
        let machines = machines_of(&entries).len() > 1;
        // What differs from the last snapshot, not yet kept.
        let pending = pending(project)?;
        let pending: Vec<&String> = pending.iter().collect();
        // `now` is a moment, said under *when*: in the `#` column it made
        // the numbers' column three wide for one digit.
        if !pending.is_empty() && first.is_none() {
            rows.push(vec![
                String::new(),
                "now".to_string(),
                history::machine(),
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
                    format!("no kept file · log --script {}", number + 1)
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
        let machines = machines_of(&entries);
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
                    markdown_text(&entry.machine),
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
             samplekit explain {} says where it came from",
            only.display(),
            shell_path(only)
        ),
        many => format!(
            "{} are files SampleKit wrote, which no snapshot keeps as it keeps a sample — \
             samplekit explain FILE says where each came from",
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
fn files_said(said: Vec<(String, Vec<String>)>) -> Vec<String> {
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
    match options
        .table_style
        .clone()
        .or_else(|| config.render().table.clone())
    {
        None => Ok(Style::default()),
        Some(name) => Style::parse(&name).ok_or_else(|| {
            Fail::usage(format!(
                "'{name}' is not a table style\n  available: {}",
                Style::names().join(", ")
            ))
        }),
    }
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
    for (at, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if at > 0 {
                outln!();
            }
            outln!("{}", project.config.root().display());
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
        if !any {
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

/// `samplekit explain FILE`: a file SampleKit wrote, found by its hash — or by
/// the snapshot a figure carries — and what changed since it was made.
pub(super) fn explain_output(file: &Path, project: Option<&Path>) -> Outcome {
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
            "no project here keeps a history to look {} up in: name one, as samplekit explain \
             {} FOLDER",
            file.display(),
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
    // the record of the write that went where the file is, else the newest.
    let chosen = |records: Vec<history::Made>| {
        records
            .iter()
            .find(|made| made.written == written_here)
            .or(records.first())
            .cloned()
    };
    let made = match chosen(history::made_all(&config, &hash).map_err(failed)?) {
        Some(made) => made,
        None => {
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
        parents: Vec::new(),
        joined: Vec::new(),
    };
    outln!("{}", file.display());
    outln!("  made      {}, by {}", when(&taken), made.said);
    outln!("  written   {}", made.written);
    outln!(
        "  from      snapshot {}, {}",
        number(&made.snapshot),
        counted_as(made.samples.len(), "sample", "samples")
    );
    let then = history::files_at(&config, &made.snapshot).map_err(failed)?;
    let now = history::files_now(&config).map_err(failed)?;
    // What it was made from: its samples, the configuration and the model.
    let concerned = |name: &str| {
        made.samples.iter().any(|sample| sample == name)
            || !(name.ends_with(".md") && name != ".samplekitrc")
    };
    let mut names: Vec<&String> = then
        .keys()
        .chain(now.keys())
        .filter(|name| concerned(name))
        .collect();
    names.sort();
    names.dedup();
    // A file is current or not by the fields it writes and by what they are
    // computed from: `set grain_mass=6.2` made an overview holding no
    // grain_mass *not current*. Where its fields cannot be told — a script, the
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
    if !lines.is_empty() {
        outln!("since then");
        for line in &lines {
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
    if !differs {
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
    // The command that made it, run on the project as it was then.
    if made.said.starts_with("samplekit ") {
        outln!("{}", made_again(&made));
    }
    Ok(())
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
    let words = shell_words::split(&made.said).ok()?;
    let cli = Cli::try_parse_from(&words).ok()?;
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
            let export = declared.as_ref()?.export(&args.export).ok()?;
            of_profile(&export.profile.clone(), &mut fields)?;
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
/// say it — through every step: an export of `abv` reads `og` through it.
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
/// `-o` and where it was written.
fn made_again(made: &history::Made) -> String {
    let words = shell_words::split(&made.said)
        .unwrap_or_else(|_| made.said.split(' ').map(str::to_string).collect());
    let mut kept: Vec<String> = Vec::new();
    let mut skip = false;
    for word in &words {
        if skip {
            skip = false;
        } else if word == "--at" {
            skip = true;
        } else if !word.starts_with("--at=") {
            kept.push(word.clone());
        }
    }
    let export = kept.get(1).is_some_and(|word| word == "export");
    let output = kept
        .iter()
        .any(|word| word == "-o" || word == "--output" || word.starts_with("--output="));
    if export && !output {
        // `written` is from the project's folder, the command from the
        // folder it ran in.
        let ran_in = made.ran_in.trim_end_matches('/');
        let written =
            if ran_in.is_empty() || ran_in == "." || Path::new(&made.written).is_absolute() {
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
        kept.push(written);
    }
    let place = if made.ran_in.is_empty() || made.ran_in == "." {
        "from the project's folder".to_string()
    } else {
        format!("from {}", made.ran_in)
    };
    let quoted: Vec<String> = kept
        .iter()
        .map(|word| shell_words::quote(word).into_owned())
        .collect();
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
            "explain --at shows one value as it was, and wants its field: samplekit explain \
             {first} {field} --at {named}"
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
    // The file as it was, read beside nothing, under the project's
    // configuration as it is: a value's states are its records' then. A folder
    // of its own name, removed when it is dropped.
    let held = tempfile::Builder::new()
        .prefix("samplekit-at-")
        .tempdir()
        .map_err(|error| Fail::io(format!("a temporary folder: {error}")))?;
    let directory = held.path().to_path_buf();
    let copy = directory.join(
        Path::new(&name)
            .file_name()
            .map_or_else(|| "sample.md".into(), |name| name.to_os_string()),
    );
    std::fs::write(&copy, bytes)
        .map_err(|error| Fail::io(format!("{}: {error}", copy.display())))?;
    force_configuration(&config.root().join(".samplekitrc"))?;
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
    // Whether anything would be written: the advice to pass --write is
    // given only then.
    let mut to_restore = false;
    for (index, project) in projects.iter().enumerate() {
        if projects.len() > 1 {
            if index > 0 {
                outln!();
            }
            outln!("{}", project.config.root().display());
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
        // Nothing to do is said *unchanged*, and exits 0.
        if entries.is_empty() {
            outln!("{NO_HISTORY} — unchanged");
            continue;
        }
        let now = history::files_now(config).map_err(|error| Fail::io(error.to_string()))?;
        // A file named that the disk holds otherwise than the last snapshot —
        // deleted since, or changed outside SampleKit — and that the snapshot
        // holds: its last content kept is what comes back, taking back only
        // what was done outside. The snapshot before the last change kept of it
        // was the one before its last edit, which lost that edit too.
        let files: Vec<&String> = project
            .within
            .iter()
            .filter(|within| !within.is_empty() && !within.ends_with('/'))
            .collect();
        // As the history last kept them: this machine's last snapshot, with
        // what the other machines' it would join changed.
        let kept = if files.is_empty() || named.is_some() {
            None
        } else {
            history::last_kept(config).map_err(|error| Fail::io(error.to_string()))?
        };
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
        let (source, then) = match (named, latest) {
            (Some(at), _) => (
                at,
                history::files_at(config, &entries[at].id)
                    .map_err(|error| Fail::io(error.to_string()))?,
            ),
            (None, Some(latest)) => latest,
            (None, None) => {
                let Some(last) = entries
                    .iter()
                    .position(|entry| entry.changed.iter().any(|name| project.holds(name)))
                else {
                    outln!(
                        "no snapshot changed what is named here: nothing to take back — unchanged"
                    );
                    continue;
                };
                // The state it was taken over, named by its parent on its own
                // machine's branch.
                let Some(parent) = entries[last]
                    .parents
                    .first()
                    .and_then(|id| entries.iter().position(|entry| entry.id == *id))
                else {
                    outln!(
                        "the last change kept of what is named here is the project as SampleKit \
                         first kept it: there is nothing before it to go back to — unchanged"
                    );
                    continue;
                };
                (
                    parent,
                    history::files_before(config, &entries[last].id)
                        .map_err(|error| Fail::io(error.to_string()))?,
                )
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
        if write {
            outln!(
                "restored to {} (#{} until now) · {} · {}",
                short(&entries[source].id),
                source + 1,
                when(&entries[source]),
                entries[source].message
            );
        } else {
            outln!("would restore to {}", snapshot_said(&entries, source));
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
        if names.is_empty() {
            outln!("\nthe files are already as it kept them — unchanged");
            continue;
        }
        to_restore = true;
        outln!();
        let said: Vec<(String, Vec<String>)> = names
            .iter()
            .map(|name| {
                // A file gone that the state held is brought back, not new.
                let lines = if !now.contains_key(*name) {
                    vec![format!("{name}   restored")]
                } else {
                    changes_of(name, now.get(*name), then.get(*name), config)
                };
                ((*name).clone(), lines)
            })
            .collect();
        for line in files_said(said) {
            outln!("{line}");
        }
        if !write {
            continue;
        }
        // One by one: where one fails, those put back before it are said,
        // as the history's own snapshot of this restore will keep them.
        let mut done: Vec<&String> = Vec::new();
        for name in &names {
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
        outln!("\nwritten: {}", counted_as(done.len(), "file", "files"));
    }
    if !write && to_restore {
        outln!("\n{}", advice("nothing written — pass --write to restore"));
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
    let words = shell_words::split(&made.said).ok()?;
    let cli = Cli::try_parse_from(&words).ok()?;
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
    let mut queries = selection.queries;
    if let Some((kind, name)) = &declared {
        let query = match *kind {
            "export" => now_config
                .as_ref()
                .and_then(|config| config.export(name).ok())
                .and_then(|export| export.query.clone()),
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
    for filter in selection.filters {
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
