//! `plot`: a declared figure, the model's, or axes given here, drawn by
//! matplotlib in the project's Python.
//!
//! Part of `cli.rs`, which holds the grammar, the types and what every command
//! shares.

use super::*;

use samplekit::config::project_config::{FigureDeclaration, FigureKind};
use samplekit::presentation::plotting::{
    self, FigureChoice, FigureOverrides, FigureRequest, PlotError,
};

/// What `plot` was asked to draw, beside the options every command shares.
pub(super) struct PlotFlags {
    pub(super) figure: Option<String>,
    /// `-x` and `-y`.
    pub(super) axes: Option<(String, String)>,
    /// `--no-project-style`: `[matplotlib]` set aside.
    pub(super) no_project_style: bool,
    /// `--title`, `--x-label`, `--y-label`, `--style`, `--group` and `--kind`,
    /// replacing what a figure declares.
    pub(super) overrides: FigureOverrides,
}

/// What matplotlib writes, by extension: a figure's file names its format.
const FORMATS: [&str; 14] = [
    "eps", "jpeg", "jpg", "pdf", "pgf", "png", "ps", "raw", "rgba", "svg", "svgz", "tif", "tiff",
    "webp",
];

pub(super) fn plot(mut options: Options, flags: &PlotFlags) -> Outcome {
    let (root, _) = target_and_extra(&options)?;
    if flags.axes.is_some() && !root.exists() {
        // With axes given every positional is a target: a figure's name among
        // them reads as a missing file.
        return Err(Fail::usage(format!(
            "{}: no such file or directory — with -x and -y there is no figure's name, \
             and every positional is a target: draw a declared figure by its name, or \
             axes with -x and -y, not both",
            root.display()
        )));
    }
    let collection = loaded(&root)?;
    warn_skipped(&collection, &options);
    let config = collection.config();

    let choice = match (&flags.figure, &flags.axes) {
        (_, Some((x, y))) => FigureChoice::AdHoc(Box::new(FigureDeclaration::ad_hoc(
            x,
            y,
            FigureKind::parse(flags.overrides.kind.as_deref().unwrap_or("scatter"))
                .map_err(Fail::usage)?,
            flags.overrides.group.clone(),
        ))),
        // A name the project declares is that declaration, drawn without
        // loading the model; any other is the model's.
        (Some(name), None) => FigureChoice::Named {
            name: name.clone(),
            model: config.and_then(|config| config.figure(name)).is_none(),
        },
        (None, None) => {
            let declared = config.map(ProjectConfig::figure_names).unwrap_or_default();
            return Err(Fail::usage(format!(
                "plot wants a figure's name, or axes: -x FIELD -y FIELD\n  {}",
                if declared.is_empty() {
                    "no figure is declared here — samplekit list figures names the model's"
                        .to_string()
                } else {
                    format!("declared: {}", declared.join(", "))
                }
            )));
        }
    };

    // Everything refused without Python is refused before the preview, so
    // that a preview --write would refuse never reads as approved.
    // A figure the model's source does not declare, and nothing near it,
    // may still be one it makes as it runs: --write asks it, and the preview
    // says so rather than promising a figure.
    let mut unconfirmed = None;
    let mut figures: Vec<String> = config
        .map(ProjectConfig::figure_names)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect();
    if let FigureChoice::Named { name, model } = &choice {
        let (of_the_model, _) = config.map(runtime::model_declarations).unwrap_or_default();
        figures.extend(of_the_model.iter().cloned());
        check_the_name(name, *model, config, &of_the_model)?;
        // A folder where the figure's name goes is a name forgotten, not a
        // figure the model might make: `plot -o x.png --write brews` ran the
        // model to ask it for a figure called `brews`.
        if *model && !of_the_model.iter().any(|figure| figure == name) && Path::new(name).exists() {
            return Err(Fail::usage(format!(
                "no figure named '{name}' — it is a path: a figure's name comes first, \
                 samplekit plot <figure> {}\n  the figures: {}",
                shell_word(name),
                if figures.is_empty() {
                    "none is declared".to_string()
                } else {
                    figures.join(", ")
                }
            )));
        }
        if *model && !of_the_model.iter().any(|figure| figure == name) {
            // One sentence for a figure nobody declares, whether the model
            // is asked or not: the preview named the model's alone.
            unconfirmed = Some(no_such_figure(name, &figures));
        }
    }
    if let FigureChoice::Named { name, model: true } = &choice {
        refuse_axes_of_the_model(name, &flags.overrides)?;
    }
    // A box plot splits by its x, so a group has nothing left to split — and
    // the refusal names whose group it is: `--kind box` on a figure declaring
    // `group` was answered *--group has nothing left to split*, of a --group
    // nobody had typed. Refused here, before Python starts.
    let drawn = match &choice {
        FigureChoice::Named { name, model: false } => config
            .and_then(|config| config.figure(name))
            .map(|declared| (declared.kind, declared.x.clone(), declared.group.clone())),
        FigureChoice::AdHoc(declared) => {
            Some((declared.kind, declared.x.clone(), declared.group.clone()))
        }
        FigureChoice::Named { .. } => None,
    };
    if let Some((kind, x, group)) = drawn {
        let kind = match flags.overrides.kind.as_deref() {
            Some(written) => FigureKind::parse(written).map_err(Fail::usage)?,
            None => kind,
        };
        let group = flags.overrides.group.clone().or(group);
        if kind == FigureKind::Box
            && let Some(group) = group
        {
            return Err(Fail::usage(if flags.overrides.group.is_some() {
                format!(
                    "a box plot's boxes are the values of '{x}': --group {group} has nothing \
                     left to split"
                )
            } else {
                format!(
                    "a box plot's boxes are the values of '{x}': the figure's group, '{group}', \
                     has nothing left to split — draw it as the figure declares it, or \
                     declare one without a group"
                )
            }));
        }
    }
    // A style naming nothing is refused before anything starts.
    if let Some(style) = &flags.overrides.style
        && style != "plain"
        && !config.is_some_and(|config| config.render_style_names().contains(&style.as_str()))
    {
        let declared = config
            .map(ProjectConfig::render_style_names)
            .unwrap_or_default();
        return Err(Fail::usage(format!(
            "no style '{style}' is declared: {}",
            declared.join(", ")
        )));
    }
    if let Some(path) = &options.output
        && let Some(refused) = output_refusal(path)
    {
        return Err(Fail::usage(format!("{}: {refused}", path.display())));
    }
    // `{name}` writes a file per sample, which only a figure of one sample
    // draws.
    let per_sample = options
        .output
        .as_ref()
        .is_some_and(|path| path.to_string_lossy().contains("{name}"));
    if per_sample && !matches!(choice, FigureChoice::Named { model: true, .. }) {
        return Err(Fail::usage(
            "{name} writes a file per sample, and this figure draws every sample in one: \
             name one file"
                .to_string(),
        ));
    }

    // A declared figure's query narrows the selection here, so that the
    // preview counts the samples it draws.
    let declared_query = match &choice {
        FigureChoice::Named { name, model: false } => config
            .and_then(|config| config.figure(name))
            .and_then(|figure| figure.query.clone()),
        _ => None,
    };
    if let Some(query) = &declared_query {
        options.query.push(query.clone());
    }
    let selected = select(&collection, &options, &root)?;
    let selected = one_project(&collection, selected, &choice)?;
    // A figure of nothing is refused, as Python refuses it: an empty file, or a
    // window of empty axes, would read as a figure of data.
    if selected.is_empty() {
        return Err(Fail::data(format!(
            "no sample is selected{}: nothing to draw",
            declared_query
                .map(|query| format!(" (the figure's query is '{query}')"))
                .unwrap_or_default()
        )));
    }
    let samples: Vec<PathBuf> = selected
        .iter()
        .filter_map(|entry| entry.path.clone())
        .collect();
    let what = match &choice {
        FigureChoice::Named { name, .. } => format!("'{name}'"),
        FigureChoice::AdHoc(axes) => format!("{} against {}", axes.y, axes.x),
    };
    // A figure no selected sample can give a point to is the data's answer,
    // said before anything starts: matplotlib's refusal came back as a usage
    // error, of *the 1*.
    let named_axes = match &choice {
        FigureChoice::Named { name, model: false } => config
            .and_then(|config| config.figure(name))
            .map(|declared| (declared.x.clone(), declared.y.clone())),
        FigureChoice::AdHoc(declared) => Some((declared.x.clone(), declared.y.clone())),
        FigureChoice::Named { .. } => None,
    }
    .filter(|(x, y)| {
        // Only names the project knows: a misspelt one is the question's,
        // and is answered with its nearest where it is resolved.
        let neighbours = siblings_of(&root);
        let project = neighbours.as_ref().unwrap_or(&collection);
        let declared = config
            .map(|config| runtime::model_declarations(config).1)
            .unwrap_or_default();
        [x, y].iter().all(|axis| match fields::parse(axis) {
            Ok(Field::Named { name, .. }) => {
                declared.iter().any(|known| known == name.as_str())
                    || project.iter().any(|entry| {
                        let sample = entry.sample.borrow();
                        sample.has_property(&name) || sample.has_attribute(&name)
                    })
            }
            _ => false,
        })
    });
    if let Some((x, y)) = &named_axes {
        let missing = left_out(&selected, &[x, y]);
        if missing.iter().map(|(_, count)| count).sum::<usize>() == samples.len() {
            return Err(Fail::data(if samples.len() == 1 {
                format!(
                    "the one sample selected holds no {}: nothing to draw",
                    missing
                        .first()
                        .map_or_else(|| x.clone(), |(field, _)| field.clone())
                )
            } else {
                format!(
                    "none of the {} samples selected holds both '{x}' and '{y}': nothing to draw",
                    samples.len()
                )
            }));
        }
    }

    // `-o` shows what it would write until --write, and starts nothing to do
    // so.
    if let Some(path) = &options.output {
        if !options.write {
            if let Some(unconfirmed) = &unconfirmed {
                outln!(
                    "{unconfirmed}\n  the model may make one as it runs: --write asks it, which \
                     may refuse\n"
                );
            }
            if per_sample {
                outln!(
                    "{what} → {}\n  a file per sample, {}",
                    relative_to_here(path),
                    counted_as(samples.len(), "file", "files")
                );
            } else {
                // What will not be drawn, where the axes are fields the files
                // answer without the model: said now, not after --write.
                let axes = match &choice {
                    FigureChoice::Named { name, model: false } => config
                        .and_then(|config| config.figure(name))
                        .map(|declared| (declared.x.clone(), declared.y.clone())),
                    FigureChoice::AdHoc(declared) => Some((declared.x.clone(), declared.y.clone())),
                    FigureChoice::Named { .. } => None,
                };
                let left_out = axes
                    .map(|(x, y)| left_out(&selected, &[&x, &y]))
                    .unwrap_or_default();
                let drawn = samples.len() - left_out.iter().map(|(_, count)| count).sum::<usize>();
                if left_out.is_empty() {
                    outln!(
                        "{what} → {}\n  a figure of {}",
                        relative_to_here(path),
                        counted_as(samples.len(), "sample", "samples")
                    );
                } else {
                    let said: Vec<String> = left_out
                        .iter()
                        .map(|(field, count)| {
                            format!("{} no {field}", counted_as(*count, "has", "have"))
                        })
                        .collect();
                    outln!(
                        "{what} → {}\n  a figure of {drawn} of {} — {}",
                        relative_to_here(path),
                        counted_as(samples.len(), "sample", "samples"),
                        said.join(", ")
                    );
                }
            }
            if per_sample {
                // Each file is named by the drawing: none is known to replace yet.
            } else if let Some(refused) = exports::refusal(path) {
                outln!("  --write refuses it: {refused}");
            } else if path.exists() {
                outln!("  the file is there, and --write replaces it");
            }
            // The folder is made as an export's is: said, and made only by
            // --write.
            if let Some(directory) = missing_folder(path) {
                outln!(
                    "  {} does not exist, and --write creates it",
                    relative_to_here(&directory)
                );
            }
            outln!("\n{}", advice("nothing written — pass --write to apply"));
            return Ok(());
        }
        if !per_sample && let Some(refused) = exports::refusal(path) {
            return Err(Fail::io(format!("{}: {refused}", path.display())));
        }
        if let Some(directory) = missing_folder(path) {
            std::fs::create_dir_all(&directory)
                .map_err(|error| Fail::io(format!("{}: {error}", directory.display())))?;
        }
    }

    let python = match &choice {
        // A name no declaration holds, asked of a model that could not be
        // reached, is said as the figure it is not before why the model
        // could not answer: the reason alone — *no environment found* —
        // hid that the name was never declared.
        FigureChoice::Named { name, model: true } => model_python(&collection, &root, name)
            .map_err(|fail| match &unconfirmed {
                Some(unconfirmed) => Fail {
                    code: fail.code,
                    message: format!(
                        "{unconfirmed}\n  the model may make one as it runs, and could not be \
                         asked: {}",
                        fail.message
                    ),
                },
                _ => fail,
            })?,
        _ => project_python(config, &root)?,
    };
    let request = FigureRequest {
        choice,
        samples,
        output: options.output.clone(),
        overrides: flags.overrides.clone(),
        project_style: !flags.no_project_style,
        said: COMMAND_LINE.get().cloned(),
        snapshot: AT.get().map(|at| at.snapshot.clone()),
    };
    plotting::draw(&python, &request).map_err(plot_failure)?;
    // Drawn from the project as it was, the file is recorded here, against that
    // snapshot: the drawing wrote no history.
    if AT.get().is_some()
        && !per_sample
        && let Some(path) = &options.output
    {
        keep_output_at(path, &request.samples);
    }
    if let Some(path) = &options.output {
        // In the words every command that writes a file uses.
        if per_sample {
            outln!(
                "written: {}, {}",
                relative_to_here(path),
                counted_as(request.samples.len(), "file", "files")
            );
        } else {
            outln!("written: {}", relative_to_here(path));
        }
    }
    Ok(())
}

/// A figure nobody declares, said one way: with every figure there is, the
/// project's then the model's, each in the order declared.
fn no_such_figure(name: &str, figures: &[String]) -> String {
    format!(
        "no figure named '{name}' is declared — the figures: {}",
        if figures.is_empty() {
            "none".to_string()
        } else {
            figures.join(", ")
        }
    )
}

/// A name two figures share is refused on every surface; one close to a
/// figure's is offered before the model is asked to run. The model's figures
/// are read from its source, so that a declared figure still runs no code of
/// the project's.
fn check_the_name(
    name: &str,
    model: bool,
    config: Option<&ProjectConfig>,
    of_the_model: &[String],
) -> Outcome {
    let declared: Vec<&str> = config.map(ProjectConfig::figure_names).unwrap_or_default();
    if !model && of_the_model.iter().any(|figure| figure == name) {
        return Err(Fail::usage(format!(
            "'{name}' is both a figure .samplekitrc declares and one of the model's: rename \
             one of them"
        )));
    }
    if model && !of_the_model.iter().any(|figure| figure == name) {
        let known: Vec<&str> = declared
            .iter()
            .copied()
            .chain(of_the_model.iter().map(String::as_str))
            .collect();
        if let Some(near) = samplekit::core::identifier::nearest(name, known.iter().copied()) {
            return Err(Fail::usage(format!(
                "no figure named '{name}' — did you mean '{near}'?\n  the figures: {}",
                known.join(", ")
            )));
        }
    }
    Ok(())
}

/// The folder `-o` names that does not exist yet, which `--write` makes as
/// `export` makes its own: `mkdir` was the paper's first step.
fn missing_folder(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty() && !parent.is_dir())
        .map(Path::to_path_buf)
}

/// Why a figure cannot be written at `path` as named: matplotlib gives a path
/// without an extension one of its own, and writes a file the command did not
/// name; `-` is standard output elsewhere, and a figure is no text.
fn output_refusal(path: &Path) -> Option<String> {
    if path == Path::new("-") {
        return Some(
            "a figure is written to a file, not to standard output: name one, as in \
             -o figure.pdf"
                .to_string(),
        );
    }
    if path
        .parent()
        .is_some_and(|parent| parent.to_string_lossy().contains("{name}"))
    {
        return Some(
            "{name} names a file per sample, not a directory: write it in the file's name, \
             as figs/{name}.pdf"
                .to_string(),
        );
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return Some(
            "a figure's file says its format by its extension — .pdf, .svg, .png — and \
             this one has none"
                .to_string(),
        );
    };
    let extension = extension.to_ascii_lowercase();
    if FORMATS.contains(&extension.as_str()) {
        return None;
    }
    Some(format!(
        "matplotlib writes no '.{extension}': {}",
        FORMATS.map(|format| format!(".{format}")).join(", ")
    ))
}

/// A model's figure draws its own axes: labelling them from here would be a
/// second hand on the same figure. The refusal names what was given.
fn refuse_axes_of_the_model(name: &str, overrides: &FigureOverrides) -> Outcome {
    let given: Vec<&str> = [
        ("--x-label", overrides.x_label.is_some()),
        ("--y-label", overrides.y_label.is_some()),
        ("--style", overrides.style.is_some()),
        ("--group", overrides.group.is_some()),
        ("--kind", overrides.kind.is_some()),
        ("--x-limits", overrides.x_limits.is_some()),
        ("--y-limits", overrides.y_limits.is_some()),
        ("--x-scale", overrides.x_scale.is_some()),
        ("--y-scale", overrides.y_scale.is_some()),
        ("--aspect", overrides.aspect.is_some()),
        ("--legend", overrides.legend.is_some()),
    ]
    .into_iter()
    .filter_map(|(flag, given)| given.then_some(flag))
    .collect();
    if given.is_empty() {
        return Ok(());
    }
    Err(Fail::usage(format!(
        "'{name}' is the model's, and draws its own axes: {} {} a figure .samplekitrc \
         declares, or axes given with -x and -y — a model's figure takes --title only",
        given.join(", "),
        if given.len() == 1 {
            "changes"
        } else {
            "change"
        }
    )))
}

/// The interpreter for a figure that runs no code of the project's: the one
/// the project names, else the nearest environment.
fn project_python(config: Option<&ProjectConfig>, root: &Path) -> Result<PathBuf, Fail> {
    match config {
        Some(config) => runtime::interpreter_for(config, root).map_err(model_failure),
        None => runtime::find_interpreter(root).ok_or_else(|| {
            Fail::io(format!(
                "no environment found: a figure is drawn by matplotlib, in the .venv/ beside \
                 the project — looked from {} upward",
                root.display()
            ))
        }),
    }
}

/// The interpreter for the model's figure: the model's code, run as `compute`
/// runs it, nothing asked.
fn model_python(collection: &SampleList, root: &Path, name: &str) -> Result<PathBuf, Fail> {
    match runtime::availability(collection.config(), root).map_err(model_failure)? {
        Availability::Ready { python, .. } => Ok(python),
        Availability::NoTemplate => {
            let declared = collection
                .config()
                .map(ProjectConfig::figure_names)
                .unwrap_or_default();
            Err(Fail::usage(format!(
                "no figure named '{name}': .samplekitrc declares {}, and no model is declared \
                 to hold one",
                if declared.is_empty() {
                    "none".to_string()
                } else {
                    declared.join(", ")
                }
            )))
        }
        Availability::Unavailable { reason } => Err(Fail::io(reason)),
    }
}

/// The Python side said why on stderr; its code is the command's.
fn plot_failure(error: PlotError) -> Fail {
    match error {
        PlotError::NotStarted { .. } => Fail::io(error.to_string()),
        PlotError::Killed { .. } => Fail::data(error.to_string()),
        PlotError::Exited { code } => Fail {
            code,
            message: String::new(),
        },
    }
}

/// `list figures`: the declared ones, and the model's, which its description
/// holds — written again first where it is stale.
pub(super) fn list_figures(collection: &SampleList, root: &Path) -> Outcome {
    let declaring = offered(collection);
    let mut listed = 0usize;
    for (file, config) in &declaring {
        let indent = match file {
            Some(file) => {
                outln!("{}", file.display());
                "  "
            }
            None => "",
        };
        for name in config.figure_names() {
            let figure = config.figure(name).expect("a listed name is declared");
            let grouped = figure
                .group
                .as_ref()
                .map(|group| format!(", one series per {group}"))
                .unwrap_or_default();
            listed += 1;
            // The name alone on its line, what it draws beneath it: several
            // figures side by side on one line each read as one run of text.
            match origin_of(config, DeclarationKind::Figure, name) {
                Some(origin) => outln!("{indent}{name}  {origin}"),
                None => outln!("{indent}{name}"),
            }
            if let Some(title) = &figure.title {
                outln!("{indent}    {title}");
            }
            outln!(
                "{indent}    {} of {} against {}{grouped}",
                figure.kind.as_str(),
                figure.y,
                figure.x
            );
        }
    }
    match runtime::availability(collection.config(), root) {
        Ok(Availability::Ready { .. }) => {
            if let Some(config) = collection.config() {
                let names = runtime::describe(config, root)
                    .map_err(|error| Fail::data(error.to_string()))?
                    .figure_names();
                // Under the configuration whose model draws them, where the
                // list is split by configuration: printed after the last one,
                // they read as a nested project's.
                let headed = declaring.iter().any(|(file, _)| file.is_some());
                let indent = if headed && !names.is_empty() {
                    if let Some(config) = collection.config() {
                        outln!(
                            "{} — its model",
                            config.root().join(".samplekitrc").display()
                        );
                    }
                    "  "
                } else {
                    ""
                };
                for name in names {
                    listed += 1;
                    outln!("{indent}{name}\n{indent}    drawn by the model");
                }
            }
        }
        Ok(Availability::Unavailable { reason }) => {
            warn(&format!("the model's figures cannot be listed: {reason}"));
        }
        Ok(Availability::NoTemplate) => {}
        Err(error) => warn(&format!("the model's figures cannot be listed: {error}")),
    }
    // Said when there is nothing to list: an empty output reads, to a
    // script, the same as a command that crashed.
    if listed == 0 {
        outln!(
            "no figure is declared: [figure.<name>] in .samplekitrc, or @sk.figure in the model"
        );
    }
    Ok(())
}

/// What a figure draws over several projects: every sample selected, for axes
/// given or a figure the projects declare alike — Python checks the
/// declarations and the units, and follows the first project's settings. A
/// model's figure alone stays one project's, the target's own: it runs that
/// project's code; the rest are said, not drawn.
fn one_project(
    collection: &SampleList,
    selected: SampleList,
    choice: &FigureChoice,
) -> Result<SampleList, Fail> {
    if !collection.spans_configurations()
        || !matches!(choice, FigureChoice::Named { model: true, .. })
    {
        return Ok(selected);
    }
    let parts = collection.by_configuration(&selected);
    let own = collection
        .config()
        .map(|config| config.root().to_path_buf());
    let drawn = parts
        .iter()
        .position(|part| part.config.map(|config| config.root().to_path_buf()) == own);
    // Several projects' are one line, each under -v.
    let mut not_drawn: Vec<String> = Vec::new();
    let mut samples = 0;
    for (at, part) in parts.iter().enumerate() {
        if Some(at) == drawn || part.samples.is_empty() {
            continue;
        }
        samples += part.samples.len();
        not_drawn.push(format!(
            "{} not drawn: a model's figure runs its own project's model, and these are {}'s",
            counted_as(part.samples.len(), "sample", "samples"),
            configuration_title(part.file.as_deref())
        ));
    }
    warn_several(
        &format!(
            "{} not drawn: a model's figure runs its own project's model, and these are {} \
             other projects'",
            counted_as(samples, "sample", "samples"),
            not_drawn.len()
        ),
        &not_drawn,
    );
    Ok(match drawn {
        Some(at) => parts
            .into_iter()
            .nth(at)
            .map(|part| part.samples)
            .unwrap_or_else(|| selected.filter_by(|_| false)),
        None => selected.filter_by(|_| false),
    })
}

/// How many selected samples hold no value of each axis, the first missing one
/// counted: a sample the figure will leave out. A table's column is read per
/// row by the drawing, and not counted here.
fn left_out(selected: &SampleList, axes: &[&String]) -> Vec<(String, usize)> {
    let parsed: Vec<(String, Field)> = axes
        .iter()
        .filter_map(|axis| Some(((*axis).clone(), fields::parse(axis).ok()?)))
        .filter(|(_, field)| matches!(field, Field::Named { .. }))
        .collect();
    let vocabulary = selected.vocabulary();
    let mut counted: Vec<(String, usize)> = Vec::new();
    for entry in selected.iter() {
        let sample = entry.sample.borrow();
        let subject = Subject {
            sample: &sample,
            path: entry.path.as_deref(),
            vocabulary: &vocabulary,
            states: entry.states.as_deref(),
        };
        let missing = parsed.iter().find(|(_, field)| {
            !matches!(
                fields::resolve(field, &subject),
                Ok(fields::Resolution::Scalar(Some(ref value))) if !value.is_absent()
            )
        });
        if let Some((axis, _)) = missing {
            match counted.iter_mut().find(|(known, _)| known == axis) {
                Some((_, count)) => *count += 1,
                None => counted.push((axis.clone(), 1)),
            }
        }
    }
    counted
}
