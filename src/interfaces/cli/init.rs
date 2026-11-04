//! `init`: setting a project up, and completing one that exists.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

/// What `init` was told on its command line: each question's answer, when a
/// flag gives it.
pub(super) struct Told {
    pub example: bool,
    pub no_model: bool,
    /// A model one already has, as typed: from the current folder, or whole.
    pub model: Option<PathBuf>,
    pub no_venv: bool,
}

impl Told {
    fn anything(&self) -> bool {
        self.example || self.no_model || self.model.is_some() || self.no_venv
    }

    /// The answers the flags give; a `--model` that names no Python file is
    /// refused as the question at the terminal refuses it.
    fn answers(&self, root: &Path) -> Result<project_setup::Answers, Fail> {
        let model = match &self.model {
            Some(typed) => {
                // Typed at the shell, so read from where it runs; said to
                // `model_path` whole, which gives it from the project's
                // folder when it is inside.
                let whole = dunce::canonicalize(typed).unwrap_or_else(|_| typed.clone());
                let path = project_setup::model_path(root, &whole.to_string_lossy())
                    .map_err(|_| Fail::usage(model_refused(typed)))?;
                project_setup::ModelAnswer::Existing(path)
            }
            None if self.no_model => project_setup::ModelAnswer::Without,
            None => project_setup::ModelAnswer::Create,
        };
        Ok(project_setup::Answers {
            starter: if self.example {
                project_setup::Starter::Example
            } else {
                project_setup::Starter::Empty
            },
            model,
            environment: !self.no_venv,
        })
    }
}

/// Why `--model` names no model: the path as it was typed, since it is read
/// from where the command runs.
fn model_refused(typed: &Path) -> String {
    let shown = typed.display();
    if typed.is_file() {
        format!("--model {shown}: not a Python file — a model is a .py file")
    } else {
        format!("--model {shown}: no file there — a path from the current folder, or a whole path")
    }
}

/// A project set up, or completed: at a terminal, by questions, and written on
/// a yes; given `--write`, a flag or no terminal, by its flags, and previewed
/// without `--write`.
///
/// Idempotent, and only proposes what is missing, so that it also serves a
/// directory that already holds samples.
pub(super) fn init_project(directory: Option<&Path>, told: &Told, options: &Options) -> Outcome {
    let root = directory.map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let asking = !options.write
        && !told.anything()
        && io::stdin().is_terminal()
        && io::stdout().is_terminal();
    let mut answers = told.answers(&root)?;
    if asking {
        let Some(asked) = ask(&root) else {
            outln!("\nnothing written");
            return Ok(());
        };
        answers = asked;
    }
    // What is here and what would complete it, as the workbench's setup sees
    // it: one plan, two ways of showing it.
    let plan = project_setup::plan(&root, &answers);
    let files = |present: bool| -> Vec<(PathBuf, String, Option<String>)> {
        plan.proposals
            .iter()
            .filter(|proposal| proposal.present == present)
            .filter_map(|proposal| match &proposal.step {
                project_setup::Step::File { path, what, .. } => {
                    Some((path.clone(), what.to_string(), proposal.note.clone()))
                }
                // Said as a directory is: with its slash.
                project_setup::Step::Directory { path, what } => {
                    Some((path.join(""), what.to_string(), proposal.note.clone()))
                }
                _ => None,
            })
            .collect()
    };
    let missing = files(false);
    let present = files(true);
    let environment = plan
        .missing()
        .find(|proposal| matches!(proposal.step, project_setup::Step::Environment { .. }))
        .map(|proposal| proposal.step.clone());

    // **`init` writes what it is there to write, and rewrites nothing**. What
    // another version of SampleKit wrote — a sample, a configuration, or a key
    // this one renamed before it was published — is named where it is met and
    // carried across by its owner. Where the completion bridge goes, for the
    // shell in use. Said, and never written: the file it belongs in is outside
    // the project.
    let bridge = completion_step();

    // A configuration that does not load is said wherever init looks at it:
    // with nothing else missing, "already set up" alone hid it.
    let unloadable = present
        .iter()
        .any(|(path, _, note)| note.is_some() && path.ends_with(".samplekitrc"));
    let unloadable_said = "\n  .samplekitrc does not load as it stands, and is kept as it is: \
                           repair it,\n  or every command reads the project without it";
    if missing.is_empty() && environment.is_none() {
        outln!("nothing to change — this project is set up, every file init writes is here\n");
        for (path, what, note) in &present {
            outln!("  {:<26} {what}", shown_step(path, &root));
            if let Some(note) = note {
                outln!("  {:<26} {note}", "");
            }
        }
        if let project_setup::Environment::Ready(venv) = project_setup::environment(&root) {
            outln!(
                "  {:<26} computing runs there, samplekit in it",
                display_relative(&venv, &root)
            );
        }
        if unloadable {
            outln!("{unloadable_said}");
        }
        outln!("\n  {:<26} {bridge}", "completion, by hand");
        // Where this project's samples are: `samples/` was named in a project
        // keeping them elsewhere.
        let folder = samplekit::config::project_config::load_for(&root)
            .ok()
            .flatten()
            .and_then(|config| samplekit::config::discovery::samples_of_project(&config).ok())
            .and_then(|paths| {
                paths
                    .first()
                    .and_then(|path| path.parent())
                    .map(Path::to_path_buf)
            })
            .unwrap_or_else(|| root.clone());
        let canonical =
            |path: &Path| dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let named = canonical(&folder)
            .strip_prefix(canonical(&root))
            .map(|relative| relative.display().to_string())
            .unwrap_or_else(|_| display_relative(&folder, &root));
        let named = if named.is_empty() || named == "." {
            ".".to_string()
        } else {
            folder_said(&named)
        };
        outln!("\n  samplekit {named} status    what is not current, and why");
        return Ok(());
    }

    // What and where, as every preview's first line says it.
    outln!(
        "{}init · {}\n",
        if asking { "\n" } else { "" },
        if root == Path::new(".") {
            "here".to_string()
        } else {
            relative_to_here(&root)
        }
    );
    for (path, what, _) in &missing {
        outln!("  {:<26} {what}", shown_step(path, &root));
    }
    for (path, _, note) in &present {
        outln!("  {:<26} already here, kept", shown_step(path, &root));
        if let Some(note) = note {
            outln!("  {:<26} {note}", "");
        }
    }
    // The environment is proposed only where samplekit is in none, and
    // `--no-venv` declines it, so this stays usable in a script.
    if let Some(step) = &environment {
        outln!("  {:<26} {}", ".venv/", step.what());
    }
    // Said with the preview, so that it says everything setting up takes, and
    // apart from what is written: listed among the files, it read as one.
    outln!("\n  {:<26} {bridge}", "completion, by hand");
    // What --write would refuse is said in the preview, not found by it.
    if let Some(directory) = &plan.blocked {
        outln!(
            "\n  {} is a file where init needs a directory: --write writes nothing until it moves",
            display_relative(directory, &root)
        );
    }
    if unloadable {
        outln!("{unloadable_said}");
    }

    // Asked as every preview asks, off a terminal too — `yes |` answers — and
    // `--write` answers beforehand.
    let write = options.write
        || (plan.blocked.is_none() && {
            outln!();
            super::ask("apply these changes?")
        });
    if !write {
        outln!("nothing written");
        return Ok(());
    }

    // A write is a snapshot in the project's history. The command line takes
    // one where `--write` is typed; a yes at the terminal is the same consent
    // without the word, and wrote a project with no history.
    let history = (!options.write).then(|| {
        let (writing, failures) =
            samplekit::config::version_control::before_writing(std::slice::from_ref(&root));
        history_failures(failures);
        writing
    });
    let applied = project_setup::apply(&plan, |_| true);
    if let Some(writing) = history {
        let said = COMMAND_LINE
            .get()
            .cloned()
            .unwrap_or_else(|| "samplekit init".to_string());
        history_after(writing, &format!("{said} (answered at the terminal)"));
    }
    let applied = applied.map_err(|error| {
        // Another writer's file is a data error, as a sample written since is;
        // what the filesystem refused is exit 3.
        let fail = if matches!(error, project_setup::SetupError::WrittenSince { .. }) {
            Fail::data
        } else {
            Fail::io
        };
        // Each path said from the project's root, as the preview said it.
        fail(match error {
            project_setup::SetupError::Blocked { directory } => format!(
                "{} is a file where init needs a directory, and nothing was written",
                display_relative(&directory, &root)
            ),
            project_setup::SetupError::Io {
                path,
                reason,
                written,
            } => said_after(&path, &reason, &written, &root),
            project_setup::SetupError::WrittenSince { path, written } => {
                said_after(&path, project_setup::WRITTEN_SINCE, &written, &root)
            }
        })
    })?;
    // In the words every command that writes uses: the count, then the paths.
    let files: Vec<String> = applied
        .written
        .iter()
        .filter(|path| !path.is_dir())
        .map(|path| display_relative(path, &root))
        .collect();
    outln!();
    for line in written_lines(&files) {
        outln!("{line}");
    }
    // The environment last: it takes a minute, and everything else is written
    // whatever it gives.
    if let Some(step) = &applied.environment {
        outln!(
            "installing samplekit {} in .venv/ — a minute or so",
            project_setup::python_spelling(env!("CARGO_PKG_VERSION"))
        );
        let _ = io::stdout().flush();
        match project_setup::make_environment(&root, step) {
            Ok(said) => outln!("{said}"),
            Err(reason) => warn(&reason),
        }
    }

    let samples = format!("{}/", display_relative(&root.join("samples"), &root));
    // From where the commands below read the project, and what computing
    // needs where the environment was declined.
    let mut first: Vec<String> = Vec::new();
    if root != Path::new(".") {
        first.push(format!("cd {}", root.display()));
    }
    // Without a model nothing is computed, and reading and writing samples
    // need no environment: none is asked for first.
    let computes = samplekit::config::project_config::load_for(&root)
        .ok()
        .flatten()
        .is_none_or(|config| config.model().is_some());
    if computes
        && !matches!(
            project_setup::environment(&root),
            project_setup::Environment::Ready(_)
        )
    {
        first.push(format!(
            "an environment with samplekit {} in it, where computing runs — samplekit init offers it",
            project_setup::python_spelling(env!("CARGO_PKG_VERSION"))
        ));
    }
    // Written around a configuration that does not load: said again, since
    // everything just written is read without it until it is repaired.
    if present
        .iter()
        .any(|(path, _, note)| note.is_some() && path.ends_with(".samplekitrc"))
    {
        warn(
            ".samplekitrc does not load as it stands: repair it before the project is read with it",
        );
    }
    if !first.is_empty() {
        outln!("\nfirst\n  {}", first.join("\n  "));
    }
    // The empty project's first steps: the model to fill in, a first sample,
    // then what the example's list says. A project that was there already is
    // not shown its first steps again.
    if present
        .iter()
        .any(|(path, ..)| path.ends_with(".samplekitrc"))
    {
        return Ok(());
    }
    if answers.starter == project_setup::Starter::Empty {
        let mut tried = vec![
            (
                "samplekit".to_string(),
                "the TUI: M the model, N a first sample, P the configuration",
            ),
            (
                format!(
                    "$EDITOR {}",
                    match &answers.model {
                        // A model of one's own is opened where it is.
                        project_setup::ModelAnswer::Existing(path) => path.display().to_string(),
                        _ => display_relative(&root.join("model/main.py"), &root),
                    }
                ),
                if matches!(answers.model, project_setup::ModelAnswer::Existing(_)) {
                    "your model, named in .samplekitrc"
                } else {
                    "declare what a sample has — its comments show each kind"
                },
            ),
            (
                format!("samplekit {samples} new S-01 --write"),
                "a first sample, named by its file",
            ),
            (
                format!("samplekit {samples}S-01.md set width=12.5 --write"),
                "a value, written as a property once the project declares it",
            ),
            (
                format!("samplekit {samples} compute --write"),
                "run the model, once it derives something",
            ),
        ];
        // Without a model, a value is typed: nothing is declared or computed.
        if !answers.has_model() {
            tried.retain(|(command, _)| {
                !command.starts_with("$EDITOR") && !command.ends_with(" compute --write")
            });
            tried[0].1 = "the TUI: N a first sample, P the configuration";
        }
        let width = tried
            .iter()
            .map(|(command, _)| command.chars().count())
            .max()
            .unwrap_or(0)
            + 3;
        // *then* follows a *first*; alone, the steps are simply in order.
        outln!(
            "\n{}",
            if first.is_empty() {
                "in this order"
            } else {
                "then, in this order"
            }
        );
        for (command, what) in &tried {
            outln!("  {command:<width$}{what}");
        }
        outln!(
            "\n  samplekit ANOTHER-FOLDER init --example   the example project, one formula of every kind, to read beside yours"
        );
        return Ok(());
    }
    // Aligned by the widest command, so that what each does reads as a column.
    let tried = [
        (format!("samplekit {samples}"), "what the example holds"),
        (
            format!("samplekit {samples} compute --write"),
            "run the model, and write what it gives",
        ),
        (
            format!("samplekit {samples} --profile overview"),
            "a declared table, now that it has values",
        ),
        (
            format!("samplekit {samples} status"),
            "what is not current, and why",
        ),
    ];
    let then = [
        (
            "samplekit new NAME".to_string(),
            "add a sample, from what the project knows",
        ),
        (
            "samplekit PATH set field=value".to_string(),
            "change a value, previewed before it writes",
        ),
        (
            "samplekit PATH tag add TAG".to_string(),
            "tag a selection, and filter on it after",
        ),
        (
            format!("samplekit {samples} validate"),
            "what is wrong, before it reaches a figure",
        ),
        (
            "samplekit --help".to_string(),
            "every command, each with its own --help",
        ),
    ];
    let width = tried
        .iter()
        .chain(&then)
        .map(|(command, _)| command.chars().count())
        .max()
        .unwrap_or(0)
        + 3;
    let lines = |rows: &[(String, &str)]| -> String {
        rows.iter()
            .map(|(command, what)| format!("  {command:<width$}{what}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    // Computing comes first: a declared profile names derived values, and
    // until they are computed the command this suggests shows them empty.
    //
    // Then the commands that work *on* what was written. A project that runs is
    // still a dead end to someone who does not know that the files it just made
    // are meant to be added to and edited by name.
    outln!(
        "\ntry it, in this order\n{}\n\nthen, to work on what is here\n{}",
        lines(&tried),
        lines(&then)
    );
    Ok(())
}

/// The questions that apply to `root`, asked one at a time at the terminal,
/// Enter keeping the answer in brackets; `None` when the input ends.
fn ask(root: &Path) -> Option<project_setup::Answers> {
    let mut answers = project_setup::Answers::default();
    outln!(
        "setting up {} — Enter keeps the answer in brackets",
        if root == Path::new(".") {
            "this folder".to_string()
        } else {
            root.display().to_string()
        }
    );
    let mut index = 0;
    // The questions that apply change with the answers: the example asks
    // nothing of a model.
    while let Some(question) = project_setup::questions(root, &answers).get(index).copied() {
        let choices = question.choices(root);
        let width = choices
            .iter()
            .map(|(label, _)| label.chars().count())
            .max()
            .unwrap_or(0)
            + 3;
        outln!("\n{}", question.ask());
        for (number, (label, detail)) in choices.iter().enumerate() {
            outln!("  {}  {label:<width$}{detail}", number + 1);
        }
        let default = question.chosen(&answers);
        let numbers: Vec<String> = (1..=choices.len()).map(|n| n.to_string()).collect();
        loop {
            print!("[{}] ", default + 1);
            let _ = io::stdout().flush();
            let mut line = String::new();
            if io::stdin().read_line(&mut line).ok()? == 0 {
                return None;
            }
            let choice = match line.trim() {
                "" => default,
                typed => match numbers.iter().position(|number| number == typed) {
                    Some(choice) => choice,
                    None => {
                        outln!(
                            "  {}, or Enter for {}",
                            said_as_choices(&numbers),
                            default + 1
                        );
                        continue;
                    }
                },
            };
            question.choose(&mut answers, choice);
            // A model one already has: its path, asked until one is there.
            if question.asks_path(choice) {
                answers.model = project_setup::ModelAnswer::Existing(ask_model_path(root)?);
            }
            break;
        }
        index += 1;
    }
    Some(answers)
}

/// `1, 2 or 3`: the numbers a question takes.
fn said_as_choices(numbers: &[String]) -> String {
    match numbers.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The path of a model file one already has, asked until it names one; `None`
/// when the input ends.
fn ask_model_path(root: &Path) -> Option<PathBuf> {
    loop {
        print!("  the model file's path, from the project's folder: ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().read_line(&mut line).ok()? == 0 {
            return None;
        }
        match project_setup::model_path(root, &line) {
            Ok(path) => return Some(path),
            Err(reason) => outln!("  {reason}"),
        }
    }
}

/// A step's path as a preview shows it: a directory with its slash.
fn shown_step(path: &Path, root: &Path) -> String {
    let shown = display_relative(path, root);
    if path.as_os_str().to_string_lossy().ends_with('/') || path.is_dir() {
        format!("{}/", shown.trim_end_matches('/'))
    } else {
        shown
    }
}

/// A path as it reads from the project's root, so that a preview shows
/// `model/main.py` rather than `./model/main.py`.
pub(super) fn display_relative(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// A file init could not write, and those it wrote before it, each said from
/// the project's root.
fn said_after(path: &Path, reason: &str, written: &[PathBuf], root: &Path) -> String {
    let mut said = format!("{}: {reason}", display_relative(path, root));
    if !written.is_empty() {
        let names: Vec<String> = written
            .iter()
            .map(|path| display_relative(path, root))
            .collect();
        said.push_str(&format!(
            "\n  written before it: {} — delete them to start again",
            names.join(", ")
        ));
    }
    said
}
