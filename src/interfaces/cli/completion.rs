//! Dynamic shell completion: what a Tab offers, read from the collection in front of it.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

pub(super) fn completion_candidates(
    names: impl IntoIterator<Item = impl Into<String>>,
    current: &OsStr,
) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    names
        .into_iter()
        .map(Into::into)
        .filter(|name| name.starts_with(prefix.as_ref()))
        .map(CompletionCandidate::new)
        .collect()
}

/// Reject fictitious clusters produced when clap-complete extends an unknown
/// short-option prefix (`-p` used to produce `-pV`, even though neither parses).
pub(super) fn completion_starts_with_an_unknown_short_option() -> bool {
    if std::env::var_os("COMPLETE").is_none() {
        return false;
    }
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(separator) = arguments.iter().position(|word| word == "--") else {
        return false;
    };
    let words = &arguments[separator + 1..];
    let Some(index) = std::env::var("_CLAP_COMPLETE_INDEX")
        .ok()
        .and_then(|index| index.parse::<usize>().ok())
    else {
        return false;
    };
    let Some(current) = words.get(index).and_then(|word| word.to_str()) else {
        return false;
    };
    if !current.starts_with('-') || current.starts_with("--") || current == "-" {
        return false;
    }

    let mut root = Cli::command();
    root.build();
    let mut command = &root;
    for word in words.iter().skip(1).take(index.saturating_sub(1)) {
        let Some(word) = word.to_str() else {
            continue;
        };
        if let Some(subcommand) = command.find_subcommand(word) {
            command = subcommand;
        }
    }

    for short in current[1..].chars() {
        let Some(argument) = command.get_arguments().find(|argument| {
            argument
                .get_short_and_visible_aliases()
                .is_some_and(|aliases| aliases.contains(&short))
        }) else {
            return true;
        };
        if argument
            .get_num_args()
            .is_some_and(|range| range.takes_values())
        {
            return false;
        }
    }
    false
}

pub(super) fn zsh_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "_./-".contains(character))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

pub(super) fn complete_environment() {
    let zsh = SamplekitZsh;
    let powershell = SamplekitPowershell;
    let completers: [&dyn EnvCompleter; 5] = [&Bash, &Elvish, &Fish, &powershell, &zsh];
    let environment =
        clap_complete::CompleteEnv::with_factory(Cli::command).shells(Shells(&completers));
    // PowerShell's empty last word, which it cannot pass as an argument, put
    // back where the bridge says it was.
    if std::env::var_os("COMPLETE").is_some() && std::env::var_os(POWERSHELL_EMPTY_WORD).is_some() {
        let mut arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
        arguments.push(std::ffi::OsString::new());
        let (arguments, index) = path_first_completion_of(arguments.clone()).unwrap_or_else(|| {
            let index = arguments.len().saturating_sub(1);
            (arguments, index)
        });
        unsafe { std::env::set_var("_CLAP_COMPLETE_INDEX", index.to_string()) };
        let current = std::env::current_dir().ok();
        if environment
            .try_complete(arguments, current.as_deref())
            .unwrap_or(false)
        {
            std::process::exit(0);
        }
        return;
    }
    // The path first, `samplekit pieces status -c <Tab>`: completed as the
    // command takes it, `status pieces -c <Tab>`. Typed first to be completed
    // from, it left the command's arguments uncompleted.
    if let Some((arguments, index)) = path_first_completion() {
        // Read by the shell's completer as the word being completed; set
        // before anything else runs, in a process that is only completing.
        unsafe { std::env::set_var("_CLAP_COMPLETE_INDEX", index.to_string()) };
        let current = std::env::current_dir().ok();
        if environment
            .try_complete(arguments, current.as_deref())
            .unwrap_or(false)
        {
            std::process::exit(0);
        }
        return;
    }
    environment.complete();
}

/// The completion's command line with its path moved after the command, and
/// the index of the word being completed there: `None` where the path does
/// not come first.
fn path_first_completion() -> Option<(Vec<std::ffi::OsString>, usize)> {
    std::env::var_os("COMPLETE")?;
    path_first_completion_of(std::env::args_os().collect())
}

/// [`path_first_completion`] of `arguments`: the word completed is the one
/// `_CLAP_COMPLETE_INDEX` names, or, where the shell names none, as
/// PowerShell's, the last.
fn path_first_completion_of(
    arguments: Vec<std::ffi::OsString>,
) -> Option<(Vec<std::ffi::OsString>, usize)> {
    let separator = arguments.iter().position(|word| word == "--")?;
    let index: usize = match std::env::var("_CLAP_COMPLETE_INDEX") {
        Ok(index) => index.parse().ok()?,
        Err(_) => arguments.len().checked_sub(separator + 2)?,
    };
    let words: Vec<String> = arguments[separator + 1..]
        .iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let (program, rest) = words.split_first()?;
    let path = rest.first()?;
    if path.starts_with('-') || is_command(path) || !Path::new(path).exists() {
        return None;
    }
    // The paths written before the command: every word up to the first
    // command, the one being typed left out.
    let paths = rest
        .iter()
        .take(index.saturating_sub(1))
        .take_while(|word| !word.starts_with('-') && !is_command(word))
        .count();
    let head: Vec<std::ffi::OsString> = arguments[..=separator].to_vec();
    let rebuilt = |moved: Vec<String>| {
        head.iter()
            .cloned()
            .chain(std::iter::once(program.into()))
            .chain(moved.into_iter().map(Into::into))
            .collect::<Vec<std::ffi::OsString>>()
    };
    // The command being typed after the paths: offered as the first word,
    // the paths behind it.
    if index == paths + 1 {
        let current = rest.get(paths).cloned().unwrap_or_default();
        let mut moved = vec![current];
        moved.extend(rest.iter().take(paths).cloned());
        moved.extend(rest.iter().skip(paths + 1).cloned());
        return Some((rebuilt(moved), 1));
    }
    let (moved, at, count) = path_first(rest)?;
    // The word being completed, among those after the command: before the
    // paths' new place, as many words further left as there were paths;
    // after them, where it was.
    let index = match index.checked_sub(count + 2) {
        Some(after) if after < at => index - count,
        _ => index,
    };
    Some((rebuilt(moved), index))
}

pub(super) fn complete_list_kind_or_target(current: &OsStr) -> Vec<CompletionCandidate> {
    let mut candidates = completion_candidates(KINDS, current);
    candidates.extend(complete_targets(current));
    candidates
}

/// After `explain`: a target, or the field explained.
pub(super) fn complete_targets_or_fields(current: &OsStr) -> Vec<CompletionCandidate> {
    let mut candidates = complete_fields(current);
    candidates.extend(complete_targets(current));
    candidates
}

/// After `open`: a target, or a pattern naming the sample's files.
pub(super) fn complete_targets_or_sample_files(current: &OsStr) -> Vec<CompletionCandidate> {
    let mut candidates = complete_sample_files(current);
    candidates.extend(complete_targets(current));
    candidates
}

/// After `set`: a target, or a change.
pub(super) fn complete_targets_or_values(current: &OsStr) -> Vec<CompletionCandidate> {
    let mut candidates = complete_values(current);
    candidates.extend(complete_targets(current));
    candidates
}

/// A target: a path, before the command or after it.
pub(super) fn complete_targets(current: &OsStr) -> Vec<CompletionCandidate> {
    PathCompleter::any().complete(current)
}

pub(super) fn completion_config() -> Option<ProjectConfig> {
    samplekit::config::project_config::load_for(&completion_root())
        .ok()
        .flatten()
}

/// The target already written on a dynamic-completion command line.
///
/// clap_complete invokes the binary as `samplekit -- <words...>`. This small
/// partial parser only locates an existing positional path; it never evaluates
/// a filter or opens a model. If the line is incomplete or ambiguous, falling
/// back to the shell's current directory is the safe useful answer.
pub(super) fn completion_root() -> PathBuf {
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(separator) = arguments.iter().position(|word| word == "--") else {
        return PathBuf::from(".");
    };
    let mut words = arguments[separator + 1..].iter();
    let _program = words.next();
    let mut option_takes_value = false;
    for word in words {
        if option_takes_value {
            option_takes_value = false;
            continue;
        }
        let text = word.to_string_lossy();
        if matches!(
            text.as_ref(),
            "-f" | "--filter"
                | "--query"
                | "-s"
                | "--sort"
                | "-c"
                | "--columns"
                | "--profile"
                | "-o"
                | "--output"
                | "--width"
        ) {
            option_takes_value = true;
            continue;
        }
        if text.starts_with('-') || is_command(&text) {
            continue;
        }
        let path = PathBuf::from(word);
        if path.exists() {
            return if path.is_file() {
                path.parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or(Path::new("."))
                    .to_path_buf()
            } else {
                path
            };
        }
    }
    PathBuf::from(".")
}

pub(super) fn is_command(word: &str) -> bool {
    command_words().iter().any(|command| command == word)
}

/// The tags the collection holds, which is what a tag argument names.
///
/// `add` is offered them too: a tag is most often given to more samples rather
/// than invented, and a spelling that already exists is the one worth reaching
/// for. A new tag is simply typed, as a new query name is.
pub(super) fn complete_tags(current: &OsStr) -> Vec<CompletionCandidate> {
    let tags = list::from_directory(&completion_root())
        .map(|collection| {
            collection
                .vocabulary()
                .tags()
                .iter()
                .map(|tag| tag.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    completion_candidates(tags, current)
}

pub(super) fn complete_queries(current: &OsStr) -> Vec<CompletionCandidate> {
    completion_candidates(
        completion_config()
            .map(|config| {
                config
                    .query_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        current,
    )
}

pub(super) fn complete_profiles(current: &OsStr) -> Vec<CompletionCandidate> {
    completion_candidates(
        completion_config()
            .map(|config| {
                config
                    .profile_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        current,
    )
}

/// The figures `.samplekitrc` declares; the model's are not listed here, since
/// naming them would import the model at Tab time.
/// The declared figures and the model's: the model's read from its source,
/// never imported, since completion runs no code of the project's.
pub(super) fn complete_figures(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(config) = completion_config() else {
        return Vec::new();
    };
    let mut names: Vec<String> = config
        .figure_names()
        .into_iter()
        .map(str::to_string)
        .collect();
    names.extend(samplekit::config::model_runtime::model_declarations(&config).0);
    names.sort();
    names.dedup();
    completion_candidates(names, current)
}

pub(super) fn complete_scripts(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(config) = completion_config() else {
        return Vec::new();
    };
    completion_candidates(
        samplekit::config::model_runtime::model_scripts(&config),
        current,
    )
}

/// `--set`'s `NAME=`: the options of the scripts the model's description
/// declares, each once.
pub(super) fn complete_script_options(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(config) = completion_config() else {
        return Vec::new();
    };
    let mut names: Vec<String> = samplekit::config::model_runtime::current_description(&config)
        .map(|description| {
            description
                .scripts
                .iter()
                .flat_map(|script| {
                    script
                        .options
                        .iter()
                        .map(|option| format!("{}=", option.name))
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.dedup();
    completion_candidates(names, current)
}

pub(super) fn complete_exports(current: &OsStr) -> Vec<CompletionCandidate> {
    completion_candidates(
        completion_config()
            .map(|config| {
                config
                    .export_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        current,
    )
}

/// A half-typed filter expression. One subject answers for one sample; the
/// collection's answer is their union, which is how `alloy ==` comes to
/// offer the alloys the collection holds rather than one sample's.
pub(super) fn complete_filters(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(written) = current.to_str() else {
        return Vec::new();
    };
    let Ok(collection) = list::from_directory(&completion_root()) else {
        return Vec::new();
    };
    let vocabulary = collection.vocabulary();
    let mut candidates: Vec<String> = Vec::new();
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        let subject = Subject {
            sample: &sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        };
        for candidate in filter::complete(written, &subject) {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

pub(super) fn complete_fields(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(prefix) = current.to_str() else {
        return Vec::new();
    };
    let Ok(collection) = list::from_directory(&completion_root()) else {
        return Vec::new();
    };
    let vocabulary = collection.vocabulary();
    let mut candidates = Vec::new();
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        let subject = Subject {
            sample: &sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        };
        for candidate in fields::complete(prefix, &subject) {
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
        .into_iter()
        .map(|candidate| {
            if candidate.ends_with(".stats.") {
                CompletionCandidate::new(candidate.trim_end_matches('.'))
                    .help(Some("statistics".into()))
            } else if candidate.ends_with('.') && !prefix.contains('.') {
                CompletionCandidate::new(candidate.trim_end_matches('.')).help(Some("table".into()))
            } else if candidate.contains('[')
                && !prefix.contains('[')
                && !candidate.to_lowercase().starts_with(&prefix.to_lowercase())
            {
                CompletionCandidate::new(candidate).help(Some("cell".into()))
            } else {
                CompletionCandidate::new(candidate)
            }
        })
        .collect()
}

pub(super) fn complete_columns(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(written) = current.to_str() else {
        return Vec::new();
    };
    let start = last_top_level(written, ',').map_or(0, |at| at + 1);
    let prefix = &written[..start];
    let column = &written[start..];
    if first_top_level(column, ':').is_some() || first_top_level(column, '=').is_some() {
        return Vec::new();
    }
    complete_fields(OsStr::new(column))
        .into_iter()
        .map(|candidate| candidate.add_prefix(prefix))
        .collect()
}

/// A value `compute -p` names — a property, or a table column as `table.column`
/// — completed after the last comma. A table is offered bare, as `-c` offers
/// it, and its columns once its dot is written. The tables of the collection,
/// for `set --add-row`.
pub(super) fn complete_tables(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(prefix) = current.to_str() else {
        return Vec::new();
    };
    let Ok(collection) = list::from_directory(&completion_root()) else {
        return Vec::new();
    };
    let mut named: Vec<String> = Vec::new();
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        for table in sample.table_names() {
            let name = table.to_string();
            if name.starts_with(prefix) && !named.contains(&name) {
                named.push(name);
            }
        }
    }
    named.into_iter().map(CompletionCandidate::new).collect()
}

pub(super) fn complete_values(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(written) = current.to_str() else {
        return Vec::new();
    };
    // `reference=[[st`: a link being typed names a sample, by its file.
    if written.contains('=')
        && let Some(open) = written.rfind("[[")
        && !written[open..].contains("]]")
    {
        let (before, typed) = written.split_at(open + 2);
        let Ok(collection) = list::from_directory(&completion_root()) else {
            return Vec::new();
        };
        let mut stems: Vec<String> = collection
            .iter()
            .filter_map(|entry| {
                let stem = entry
                    .path
                    .as_ref()?
                    .file_stem()?
                    .to_string_lossy()
                    .into_owned();
                Some(
                    stem.strip_suffix(".sample")
                        .map(str::to_string)
                        .unwrap_or(stem),
                )
            })
            .filter(|stem| stem.starts_with(typed))
            .collect();
        stems.sort();
        stems.dedup();
        return stems
            .into_iter()
            .map(|stem| CompletionCandidate::new(format!("{before}{stem}]]")))
            .collect();
    }
    let start = written.rfind(',').map_or(0, |at| at + 1);
    let (prefix, segment) = written.split_at(start);
    let Ok(collection) = list::from_directory(&completion_root()) else {
        return Vec::new();
    };
    let (mut properties, mut tables, mut columns) = (Vec::new(), Vec::new(), Vec::new());
    let push = |into: &mut Vec<String>, name: String| {
        if !into.contains(&name) {
            into.push(name);
        }
    };
    for entry in collection.iter() {
        let sample = entry.sample.borrow();
        for name in sample.property_names() {
            push(&mut properties, name.to_string());
        }
        // Attributes are addressable like anything else — `operator`, a batch,
        // a date — and leaving them out of completion made them look private.
        for name in sample.attribute_names() {
            push(&mut properties, name.to_string());
        }
        for table in sample.table_names() {
            push(&mut tables, table.to_string());
            if let Ok(held) = sample.table(table) {
                for column in held.column_names() {
                    push(&mut columns, format!("{table}.{column}"));
                }
            }
        }
    }
    let offered = |name: &String| name.starts_with(segment);
    if segment.contains('.') {
        columns
            .iter()
            .filter(|name| offered(name))
            .map(|name| CompletionCandidate::new(format!("{prefix}{name}")))
            .collect()
    } else {
        properties
            .iter()
            .filter(|name| offered(name))
            .map(|name| CompletionCandidate::new(format!("{prefix}{name}")))
            .chain(tables.iter().filter(|name| offered(name)).map(|name| {
                CompletionCandidate::new(format!("{prefix}{name}")).help(Some("table".into()))
            }))
            .collect()
    }
}

/// A sort key is a field with an optional leading `-`, so the field completes
/// after the last comma and after that sign, and both stay in the candidate.
pub(super) fn complete_sort_keys(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(written) = current.to_str() else {
        return Vec::new();
    };
    let start = last_top_level(written, ',').map_or(0, |at| at + 1);
    let key = &written[start..];
    let field = key.strip_prefix('-').unwrap_or(key);
    let prefix = &written[..written.len() - field.len()];
    complete_fields(OsStr::new(field))
        .into_iter()
        .map(|candidate| candidate.add_prefix(prefix))
        .collect()
}

pub(super) fn complete_table_styles(current: &OsStr) -> Vec<CompletionCandidate> {
    completion_candidates(Style::names(), current)
}

pub(super) fn complete_render_styles(current: &OsStr) -> Vec<CompletionCandidate> {
    let names = list::from_directory(&completion_root())
        .ok()
        .and_then(|collection| {
            collection.config().map(|config| {
                config
                    .render_style_names()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_else(|| vec!["plain".to_string()]);
    completion_candidates(names, current)
}

pub(super) fn completions(shell: CompletionShell) -> Outcome {
    let shell = format!("{shell:?}").to_lowercase();
    // SAFETY: SampleKit is single-threaded. clap_complete reads and removes
    // this variable immediately, before any other application work begins.
    unsafe { std::env::set_var("COMPLETE", shell) };
    complete_environment();
    Ok(())
}

/// The line that installs shell completion, for the shell in use.
///
/// Said and never written: the file it belongs in is the user's, outside the
/// project, and a setup command does not edit a login shell's configuration
/// behind its owner.
pub(super) fn completion_step() -> String {
    let shell = std::env::var("SHELL")
        .ok()
        .and_then(|path| {
            Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let (name, file) = match shell.as_str() {
        "bash" => ("bash", "~/.bashrc"),
        "fish" => ("fish", "~/.config/fish/config.fish"),
        "elvish" => ("elvish", "~/.elvish/rc.elv"),
        "zsh" => ("zsh", "~/.zshrc"),
        // PowerShell sets no SHELL, and its module path in every session.
        "" if std::env::var_os("PSModulePath").is_some() => {
            return "add to your PowerShell profile, $PROFILE:  samplekit completions powershell \
                    | Out-String | Invoke-Expression"
                .to_string();
        }
        _ => {
            return "samplekit completions <shell> prints the bridge to source".to_string();
        }
    };
    // Each shell sources the bridge in its own words: fish and elvish have no
    // `<(…)`, and the line given them failed.
    let line = match name {
        "fish" => "samplekit completions fish | source".to_string(),
        "elvish" => "eval (samplekit completions elvish | slurp)".to_string(),
        name => format!("source <(samplekit completions {name})"),
    };
    format!("add to {file}:  {line}")
}

/// A sample's own files, by name: what `open`'s pattern names. Read from the
/// sample already on the line, as a field completes from it.
pub(super) fn complete_sample_files(current: &OsStr) -> Vec<CompletionCandidate> {
    let arguments: Vec<_> = std::env::args_os().collect();
    let Some(sample) = arguments
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file() && path.extension().is_some_and(|extension| extension == "md"))
    else {
        return Vec::new();
    };
    let Ok(Some(config)) = samplekit::config::project_config::load_for(&sample) else {
        return Vec::new();
    };
    let names: Vec<String> = samplekit::config::discovery::sample_files(&config, &sample, None)
        .unwrap_or_default()
        .iter()
        .filter_map(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .collect();
    completion_candidates(names, current)
}

/// A state of the project's history, for `--at`, `--from` and `--to`: `now`,
/// then each snapshot by its number from `samplekit log`, when it was taken
/// and what took it beside it.
pub(super) fn complete_when(current: &OsStr) -> Vec<CompletionCandidate> {
    states(current, true)
}

/// A snapshot alone, for `log --script` and `restore --at`, which refuse
/// `now`: a script is kept beside a snapshot, and now is where the files
/// already are.
pub(super) fn complete_snapshot(current: &OsStr) -> Vec<CompletionCandidate> {
    states(current, false)
}

fn states(current: &OsStr, now: bool) -> Vec<CompletionCandidate> {
    let prefix = current.to_string_lossy();
    let Ok(Some(config)) = samplekit::config::project_config::load_for(&completion_root()) else {
        return Vec::new();
    };
    let entries = samplekit::config::version_control::entries(&config).unwrap_or_default();
    now.then(|| CompletionCandidate::new("now").help(Some("the files as they are".into())))
        .into_iter()
        .chain(entries.iter().enumerate().map(|(at, entry)| {
            CompletionCandidate::new((at + 1).to_string()).help(Some(
                format!(
                    "{} · {}",
                    samplekit::presentation::changes::when(entry),
                    entry.message
                )
                .into(),
            ))
        }))
        .filter(|candidate| {
            candidate
                .get_value()
                .to_string_lossy()
                .starts_with(prefix.as_ref())
        })
        .collect()
}
