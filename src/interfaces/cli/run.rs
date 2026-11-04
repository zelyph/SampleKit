//! `run`: a script the model declares, `@sk.script`, run by its Python on a
//! sample at a time or on the selection; what it changes is shown, asked
//! about and written.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::config::model_runtime::DescribedScript;
use samplekit::presentation::scripting::{self, Held, ScriptError, ScriptRequest};

/// What `run` was asked, beside the options every command shares.
pub(super) struct RunFlags {
    pub(super) script: Option<String>,
    /// `--set NAME=VALUE`, each.
    pub(super) set: Vec<String>,
    /// `--style`: what the script returns is written in it.
    pub(super) style: Option<String>,
    /// `--timeout`: past it the script is stopped, and nothing is written.
    pub(super) timeout: Option<std::time::Duration>,
}

pub(super) fn run(options: Options, flags: &RunFlags) -> Outcome {
    let (root, _) = target_and_extra(&options)?;
    let collection = loaded(&root)?;
    warn_skipped(&collection, &options);
    let Some(config) = collection.config() else {
        return Err(Fail::usage(format!(
            "no .samplekitrc is above {}: a script is a method of the project's model — \
             samplekit init makes one",
            root.display()
        )));
    };
    let python = match runtime::availability(Some(config), &root).map_err(model_failure)? {
        Availability::Ready { python, .. } => python,
        Availability::NoTemplate => {
            return Err(Fail::usage(
                "no model is declared: a script is a method of the model marked @sk.script, \
                 and [model] python in .samplekitrc names the model"
                    .to_string(),
            ));
        }
        Availability::Unavailable { reason } => return Err(Fail::io(reason)),
    };
    let description =
        runtime::describe(config, &root).map_err(|error| Fail::data(error.to_string()))?;
    let Some(name) = &flags.script else {
        return Err(Fail::usage(format!(
            "run wants a script's name\n  {}",
            scripts_said(&description.scripts)
        )));
    };
    let Some(script) = description.script(name) else {
        let names: Vec<String> = description
            .scripts
            .iter()
            .map(|script| script.name.clone())
            .collect();
        let nearest = samplekit::core::identifier::nearest(name, names.iter().map(String::as_str))
            .map(|near| format!(": did you mean '{near}'?"))
            .unwrap_or_default();
        // A path where the name goes is a name forgotten.
        let path = if Path::new(name).exists() {
            format!(
                "\n  '{name}' is a path: it comes before run, the script's name after — samplekit {} run <script>",
                shell_word(name)
            )
        } else {
            String::new()
        };
        return Err(Fail::usage(format!(
            "no script named '{name}'{nearest}{path}\n  {}",
            scripts_said(&description.scripts)
        )));
    };
    let given = scripting::options_of(&flags.set).map_err(Fail::usage)?;
    if let Some(unknown) = given
        .keys()
        .find(|option| !script.options.iter().any(|known| &&known.name == option))
    {
        return Err(Fail::usage(format!(
            "'{name}' takes no option '{unknown}' — {}",
            options_said(script)
        )));
    }
    if let Some(style) = &flags.style
        && style != "plain"
        && !config.render_style_names().contains(&style.as_str())
    {
        return Err(Fail::usage(format!(
            "no style '{style}' is declared: {}",
            config.render_style_names().join(", ")
        )));
    }
    let selected = select(&collection, &options, &root)?;
    let samples: Vec<PathBuf> = selected
        .iter()
        .filter_map(|entry| entry.path.clone())
        .collect();
    if samples.is_empty() {
        return Err(Fail::data(
            "no sample is selected: nothing to run".to_string(),
        ));
    }
    let request = ScriptRequest {
        script: name.clone(),
        samples,
        options: given,
        style: flags.style.clone(),
        timeout: flags.timeout,
    };
    let ran = scripting::run(&python, &request).map_err(script_failure)?;
    // The answers are part of the run: kept with the command in its snapshot.
    if !ran.answers.is_empty()
        && let Ok(mut said) = super::ANSWERED.lock()
    {
        *said = format!(
            " — answered {}",
            ran.answers
                .iter()
                .map(|asked| format!("'{}' {}", asked.prompt.trim(), asked.answer))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let held: Vec<Held> = ran
        .held
        .into_iter()
        .filter(|held| held.before.as_deref() != Some(held.after.as_slice()))
        .collect();
    if held.is_empty() {
        return Ok(());
    }
    // What and where, as every preview's first line says it.
    outln!(
        "\nrun {name} · {}\n",
        counted_as(held.len(), "file changed", "files changed")
    );
    // What the script asked, answered, before what the answers made of it.
    for asked in &ran.answers {
        outln!("  asked {}  {}", asked.prompt.trim(), asked.answer);
    }
    if !ran.answers.is_empty() {
        outln!();
    }
    for line in history::files_said(changes_said(config, &held)) {
        outln!("{line}");
    }
    // What the changes make not current, in one block, as `set` says it: a
    // script scaling the ore charge left recovery outdated, unsaid.
    let mut newly: Vec<(String, String)> = Vec::new();
    for file in &held {
        let after = samplekit::presentation::changes::sample_of(&file.after);
        let before = file
            .before
            .as_deref()
            .and_then(samplekit::presentation::changes::sample_of);
        let Some(after) = after else {
            continue;
        };
        let was = before
            .as_ref()
            .map(samplekit::collection::editing::not_current)
            .unwrap_or_default();
        let now = samplekit::collection::editing::not_current(&after);
        let label = file
            .path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        for entry in samplekit::collection::editing::newly_not_current(&was, &now) {
            let name = if held.len() > 1 {
                format!("{label} {}", entry.name)
            } else {
                entry.name.clone()
            };
            newly.push((name, state_of(&after, &entry.state, entry.rows)));
        }
    }
    outln!();
    for line in samplekit::presentation::changes::not_current_lines(&newly, 0) {
        outln!("{line}");
    }
    outln!();
    if !options.write && !ask("apply these changes?") {
        outln!("nothing written");
        return Ok(());
    }
    // A write like --write's: one snapshot in each project it changed, which
    // the command line takes itself only of --write.
    let places: Vec<PathBuf> = held.iter().map(|held| held.path.clone()).collect();
    let writing = (!options.write).then(|| {
        let (writing, failures) = samplekit::config::version_control::before_writing(&places);
        history_failures(failures);
        writing
    });
    let written = write_held(&held);
    // What the script computed is kept in the trace, before the snapshot,
    // under the formulas the worker that ran it described: the saves were
    // held, and the script kept none.
    if written.is_ok() {
        keep_trace_of(config, &held);
    }
    if let Some(writing) = writing {
        let said = COMMAND_LINE.get().map_or("samplekit run", String::as_str);
        let answered = super::ANSWERED
            .lock()
            .map(|said| said.clone())
            .unwrap_or_default();
        history_after(writing, &format!("{}{answered}", answered_yes(said)));
    }
    written?;
    let paths: Vec<String> = held
        .iter()
        .map(|file| relative_to_here(&file.path))
        .collect();
    for line in written_lines(&paths) {
        outln!("{line}");
    }
    Ok(())
}

/// The trace of what a script's held saves computed: a fact for each record
/// new in a file written, under the formula the model's description names.
fn keep_trace_of(config: &ProjectConfig, held: &[Held]) {
    use samplekit::config::{model_runtime, version_control};
    let Some(described) = model_runtime::current_description(config) else {
        return;
    };
    let formulas: std::collections::BTreeMap<String, String> = described
        .properties
        .iter()
        .filter_map(|(name, property)| Some((name.clone(), property.formula.clone()?)))
        .chain(described.tables.iter().flat_map(|(table, described)| {
            described.columns.iter().filter_map(move |column| {
                Some((format!("{table}.{}", column.name), column.formula.clone()?))
            })
        }))
        .collect();
    if formulas.is_empty() {
        return;
    }
    let mut tracing = samplekit::collection::computation::Tracing::new();
    for file in held {
        let before = file
            .before
            .as_deref()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .and_then(|text| samplekit::format::document::parse(text).ok())
            .and_then(|document| samplekit::format::schema::into_sample(document.schema).ok())
            .map(|sample| version_control::records_of_values(&sample))
            .unwrap_or_default();
        tracing.saved(Some(config), &before, &file.path, &formulas, &[]);
    }
    for (root, error) in tracing.keep() {
        warn(&format!(
            "the trace of the formulas was not kept in {}: {error}",
            root.display()
        ));
    }
}

/// Each file the script changed, as the history says a change: its name
/// from here, and the values that moved.
fn changes_said(config: &ProjectConfig, held: &[Held]) -> Vec<(String, Vec<String>)> {
    held.iter()
        .map(|held| {
            let name = relative_to_here(&held.path);
            let lines = match &held.before {
                None if name.ends_with(".md") => {
                    samplekit::presentation::changes::made_of(&name, &held.after, config)
                }
                before => samplekit::presentation::changes::changes_of(
                    &name,
                    before.as_ref(),
                    Some(&held.after),
                    config,
                ),
            };
            (name, lines)
        })
        .collect()
}

/// Each file as the script left it, or none: a file changed since the script
/// read it is refused, not lost, and those written before it are said.
fn write_held(held: &[Held]) -> Result<usize, Fail> {
    let mut done: Vec<String> = Vec::new();
    for file in held {
        let written = document::replace_if_unchanged(
            &file.path,
            file.before.as_deref(),
            Some(file.after.as_slice()),
        );
        if let Err(error) = written {
            let changed = !matches!(error, document::DocumentError::Io { .. });
            let mut message = error.to_string();
            if done.is_empty() {
                message.push_str("\n  nothing was written");
            } else {
                message.push_str(&format!("\n  written before it: {}", done.join(", ")));
            }
            return Err(if changed {
                Fail::data(message)
            } else {
                Fail::io(message)
            });
        }
        done.push(relative_to_here(&file.path));
    }
    Ok(done.len())
}

/// The Python side said why on stderr; its code is the command's.
fn script_failure(error: ScriptError) -> Fail {
    match error {
        ScriptError::NotStarted { .. } | ScriptError::NoReply { .. } => Fail::io(error.to_string()),
        ScriptError::Killed { .. } | ScriptError::TimedOut { .. } => Fail::data(error.to_string()),
        ScriptError::Exited { code } => Fail {
            code,
            message: String::new(),
        },
    }
}

/// The scripts the model declares, on one line, for a refusal.
fn scripts_said(scripts: &[DescribedScript]) -> String {
    if scripts.is_empty() {
        "the model declares none: mark a method @sk.script".to_string()
    } else {
        format!(
            "the model's scripts: {} — samplekit list scripts describes them",
            scripts
                .iter()
                .map(|script| script.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// A script's options, as `--set` gives them: `factor=0.9`, and a required
/// one by its name alone.
fn options_said(script: &DescribedScript) -> String {
    if script.options.is_empty() {
        return "it takes none".to_string();
    }
    format!(
        "its options: {}",
        script
            .options
            .iter()
            .map(|option| match &option.default {
                Some(default) => format!("{}={default}", option.name),
                None => option.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `list scripts`: the model's, its description written again first where
/// it is stale — each name, what it acts on, its options and its
/// description.
pub(super) fn list_scripts(collection: &SampleList, root: &Path) -> Outcome {
    let config = collection.config();
    match runtime::availability(config, root) {
        Ok(Availability::Ready { .. }) => {}
        Ok(Availability::Unavailable { reason }) => {
            return Err(Fail::io(format!(
                "the model's scripts cannot be listed: {reason}"
            )));
        }
        Ok(Availability::NoTemplate) => {
            outln!("no model is declared, and a script is a method of the model: @sk.script");
            return Ok(());
        }
        Err(error) => return Err(model_failure(error)),
    }
    let Some(config) = config else {
        return Ok(());
    };
    let description =
        runtime::describe(config, root).map_err(|error| Fail::data(error.to_string()))?;
    if description.scripts.is_empty() {
        outln!("no script is declared: mark a method of the model @sk.script");
        return Ok(());
    }
    let entries: Vec<Listed> = description
        .scripts
        .iter()
        .map(|script| Listed {
            name: script.name.clone(),
            scope: acts_on(script.acts).to_string(),
            lines: std::iter::once(script.description.clone())
                .filter(|line| !line.is_empty())
                .collect(),
            options: script
                .options
                .iter()
                .map(|option| {
                    let kind = match option.kind.as_str() {
                        "int" => "INTEGER",
                        "float" => "NUMBER",
                        "bool" => "YES|NO",
                        _ => "TEXT",
                    };
                    (
                        format!("--set {}={kind}", option.name),
                        match &option.default {
                            Some(default) if default.is_empty() => "empty by default".to_string(),
                            Some(default) => format!("{default} by default"),
                            None => "required".to_string(),
                        },
                    )
                })
                .collect(),
        })
        .collect();
    for line in listed_lines(&entries, 0) {
        outln!("{line}");
    }
    Ok(())
}
