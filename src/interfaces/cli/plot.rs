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
    /// `--accept`: the states the values it is drawn from may be in, and the
    /// figure exit 0.
    pub(super) accept: super::Accepted,
    /// The export drawing it, said first, as a table's export says itself.
    pub(super) export: Option<String>,
}

/// What matplotlib writes, by extension: a figure's file names its format.
const FORMATS: [&str; 14] = [
    "eps", "jpeg", "jpg", "pdf", "pgf", "png", "ps", "raw", "rgba", "svg", "svgz", "tif", "tiff",
    "webp",
];

/// Whether `config`, or its model, declares the figure `name`.
fn declares_figure(config: Option<&ProjectConfig>, name: &str) -> bool {
    config.is_some_and(|config| {
        config.figure(name).is_some()
            || runtime::model_declarations(config)
                .0
                .iter()
                .any(|declared| declared == name)
    })
}

/// `plot` again, its targets the selection's projects: their samples the
/// filters and queries given select, as from the root.
fn plot_in(mut options: Options, flags: &PlotFlags, roots: Vec<PathBuf>) -> Outcome {
    let mut roots = roots.into_iter();
    let Some(first) = roots.next() else {
        return Ok(());
    };
    options.positionals = vec![first.to_string_lossy().into_owned()];
    EXTRA_TARGETS.with(|extra| *extra.borrow_mut() = roots.collect());
    plot(options, flags)
}

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

    // From a root above several projects, a figure the root does not declare
    // is the selection's projects': drawn from them, as `samplekit
    // data/samples plot NAME` draws it. `samplekit . plot NAME -f …` drew
    // nothing, though every sample selected was one project's.
    if let (Some(name), None) = (&flags.figure, &flags.axes)
        && !declares_figure(config, name)
        && collection.spans_configurations()
        && let Ok(selected) = select(&collection, &options, &root)
    {
        let here = |path: &Path| relative_to_here(path);
        match collection.scope_of(&selected) {
            Some(list::Scope::One(own)) if config.is_none_or(|root| root.root() != own.root()) => {
                return plot_in(options, flags, vec![own.root().to_path_buf()]);
            }
            Some(list::Scope::SameModel(projects)) => {
                let roots = projects
                    .iter()
                    .map(|project| project.root().to_path_buf())
                    .collect();
                return plot_in(options, flags, roots);
            }
            Some(list::Scope::Mixed(projects)) => {
                // The projects declaring it first, for the command suggested.
                let mut projects = projects;
                projects.sort_by_key(|project| !declares_figure(Some(project), name));
                let named: Vec<String> = projects
                    .iter()
                    .filter(|project| config.is_none_or(|root| root.root() != project.root()))
                    .map(|project| here(project.root()))
                    .collect();
                return Err(Fail::usage(format!(
                    "the selection spans several projects of different models — {}: only the \
                     root's figures are offered, and it declares no '{name}'\n  name one \
                     project, samplekit {} plot {name}, or narrow the selection",
                    named.join(", "),
                    named.first().cloned().unwrap_or_default()
                )));
            }
            _ => {}
        }
    }

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
        // figure the model might make: `plot -o x.png --write pieces` ran the
        // model to ask it for a figure called `pieces`.
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
        FigureChoice::Named { name, .. } => name.clone(),
        FigureChoice::AdHoc(axes) => format!("{} against {}", axes.y, axes.x),
    };
    let header = match &flags.export {
        Some(export) => format!("export {export} · figure {what}"),
        None => format!("figure {what}"),
    };
    // A figure no selected sample can give a point to is the data's answer,
    // said before anything starts: matplotlib's refusal came back as a usage
    // error, of *the 1*.
    let declared_axes = match &choice {
        FigureChoice::Named { name, model: false } => config
            .and_then(|config| config.figure(name))
            .map(|declared| (declared.x.clone(), declared.y.clone())),
        FigureChoice::AdHoc(declared) => Some((declared.x.clone(), declared.y.clone())),
        FigureChoice::Named { .. } => None,
    };
    let named_axes = declared_axes.clone().filter(|(x, y)| {
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

    // What the axes read that is not current: said in the preview, and a
    // written figure exits 2 for it, as an export does, unless accepted.
    let mut behind = samplekit::collection::exports::Behind::default();
    // Every declared axis, a table's column as well as a property.
    let behind_axes: Vec<String> = match (&declared_axes, config) {
        (Some((x, y)), Some(config)) => {
            let columns = [x, y]
                .iter()
                .filter_map(|axis| samplekit::config::profiles::parse_column(axis).ok())
                .collect();
            let profile = samplekit::config::profiles::anonymous(columns);
            behind = samplekit::collection::exports::behind(&selected, &profile, Some(config));
            behind.named.clone()
        }
        _ => Vec::new(),
    };

    // **Two units on one axis are refused**, as in a table's column: points
    // in Ductility and in SG drawn on one scale compare nothing.
    if let Some((x, y)) = &named_axes {
        let columns = [x, y]
            .iter()
            .filter_map(|axis| samplekit::config::profiles::parse_column(axis).ok())
            .collect();
        let axes = samplekit::config::profiles::anonymous(columns);
        let disagreeing: Vec<String> = tables::units_across(&collection, &selected, &axes, None)
            .into_iter()
            .filter(|(_, spellings)| spellings.len() > 1)
            .map(|(field, spellings)| format!("{field} is in {}", spellings.join(" and ")))
            .collect();
        if !disagreeing.is_empty() {
            return Err(Fail::data(format!(
                "an axis cannot say which unit it is in: {}\n  \
                 a figure of two units is two figures — narrow the selection, or samplekit \
                 validate says which files disagree",
                disagreeing.join("; ")
            )));
        }
        // Over several projects, a unit in one and none in another disagree
        // as two units do: the axis took the first one's, `RADIUS [SG]`, over
        // samples that have none.
        let unitless = unit_against_none(&collection, &selected, &axes);
        if !unitless.is_empty() {
            return Err(Fail::data(format!(
                "an axis cannot say which unit it is in: {}\n  \
                 the projects drawn must give it one unit — declare it in each, or draw \
                 them apart",
                unitless.join("; ")
            )));
        }
        if let Some(note) = declared_otherwise(&collection, &selected, &axes) {
            errp!("{note}\n");
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
            // What and where, as every preview's first line says it.
            if per_sample {
                outln!(
                    "{header} · {} · a file per sample, {}\n",
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
                    .as_ref()
                    .map(|(x, y)| left_out(&selected, &[x, y]))
                    .unwrap_or_default();
                let drawn = samples.len() - left_out.iter().map(|(_, count)| count).sum::<usize>();
                if left_out.is_empty() {
                    outln!(
                        "{header} · {} · {}\n",
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
                        "{header} · {} · {drawn} of {}\n\n  {}",
                        relative_to_here(path),
                        counted_as(samples.len(), "sample", "samples"),
                        said.join(", ")
                    );
                }
                // What it is drawn from that is not current, as an export
                // says it: its --write exits 2 for it, unless accepted.
                if !behind_axes.is_empty() {
                    outln!(
                        "\n  drawn from values that are not current: {}\n    {}",
                        counted_as(behind_axes.len(), "value", "values"),
                        behind_axes.join(", ")
                    );
                }
            }
            if per_sample {
                // Each file is named by the drawing: none is known to replace yet.
            } else if let Some(refused) = exports::refusal(path) {
                outln!("  --write refuses it: {refused}");
            } else if path.exists() {
                outln!("  the file is there, and is replaced");
            }
            // The folder is made as an export's is: said, and made only by
            // --write.
            if let Some(directory) = missing_folder(path) {
                outln!(
                    "  {} does not exist, and is made",
                    relative_to_here(&directory)
                );
            }
            outln!();
            offer_to_apply(
                "apply these changes?",
                "nothing written — pass --write to apply",
            );
            return Ok(());
        }
        if !per_sample && let Some(refused) = exports::refusal(path) {
            return Err(Fail::usage(format!("{}: {refused}", path.display())));
        }
        // Written outright, it says what and where first, as every write
        // does: a yes at the question has shown it already.
        if !APPLYING.load(AtomicOrdering::Relaxed) {
            outln!(
                "{header} · {} · {}",
                relative_to_here(path),
                if per_sample {
                    format!(
                        "a file per sample, {}",
                        counted_as(samples.len(), "file", "files")
                    )
                } else {
                    counted_as(samples.len(), "sample", "samples")
                }
            );
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
        workbench: false,
        // The axes known here, what they read is judged here, by every state
        // `--accept` names; a figure the model draws reads what it reads, and
        // any state accepted lets it pass.
        accept_outdated: declared_axes.is_some() || flags.accept != super::Accepted::default(),
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
    // Drawn, and a failure all the same, as an export written from values not
    // current is, by the same rule.
    super::tables::accepted_or_failed("the figure", &behind, &flags.accept)
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
/// the project declares. Without a project, none is declared, and none is
/// searched for.
fn project_python(config: Option<&ProjectConfig>, root: &Path) -> Result<PathBuf, Fail> {
    match config {
        Some(config) => runtime::figure_interpreter(config).map_err(Fail::io),
        None => Err(Fail::io(format!(
            "no interpreter is declared: a figure is drawn by matplotlib, in the Python a \
             project's .samplekitrc names under [model] python, and no .samplekitrc is above {} \
             — samplekit init makes one",
            root.display()
        ))),
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
/// holds — written again first where it is stale. Each as `list scripts`
/// writes a script: its name and what it draws, its title or docstring
/// beneath, then what it plots.
pub(super) fn list_figures(collection: &SampleList, root: &Path) -> Outcome {
    let declaring = offered(collection);
    let mut listed = 0usize;
    // Each configuration's heading, its indent and its figures: written once
    // all are known, their names in one column.
    let mut groups: Vec<(Option<String>, &str, Vec<Listed>)> = Vec::new();
    for (file, config) in &declaring {
        let heading = file.as_ref().map(|file| file.display().to_string());
        let indent = if heading.is_some() { "  " } else { "" };
        let entries: Vec<Listed> = config
            .figure_names()
            .into_iter()
            .map(|name| {
                let figure = config.figure(name).expect("a listed name is declared");
                let grouped = figure
                    .group
                    .as_ref()
                    .map(|group| format!(", a series per {group}"))
                    .unwrap_or_default();
                let mut scope = "on the selection".to_string();
                // Where another file declares it, said beside what it draws.
                if let Some(origin) = origin_of(config, DeclarationKind::Figure, name) {
                    scope.push_str(&format!(" · {origin}"));
                }
                let kind = figure.kind.as_str();
                let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                    "an"
                } else {
                    "a"
                };
                Listed {
                    name: name.to_string(),
                    scope,
                    lines: figure
                        .title
                        .iter()
                        .cloned()
                        .chain(std::iter::once(format!(
                            "{article} {kind} of {} against {}{grouped}",
                            figure.y, figure.x
                        )))
                        .collect(),
                    options: Vec::new(),
                }
            })
            .collect();
        listed += entries.len();
        groups.push((heading, indent, entries));
    }
    match runtime::availability(collection.config(), root) {
        Ok(Availability::Ready { .. }) => {
            if let Some(config) = collection.config() {
                let described = runtime::describe(config, root)
                    .map_err(|error| Fail::data(error.to_string()))?;
                // Under the configuration whose model draws them, where the
                // list is split by configuration: printed after the last one,
                // they read as a nested project's.
                let headed = declaring.iter().any(|(file, _)| file.is_some());
                let (heading, indent) = if headed && !described.figures.is_empty() {
                    (
                        Some(format!(
                            "{} — its model",
                            config.root().join(".samplekitrc").display()
                        )),
                        "  ",
                    )
                } else {
                    (None, "")
                };
                let entries: Vec<Listed> = described
                    .figures
                    .iter()
                    .map(|figure| Listed {
                        name: figure.name.clone(),
                        scope: acts_on(figure.draws).to_string(),
                        lines: std::iter::once(figure.description.clone())
                            .filter(|line| !line.is_empty())
                            .collect(),
                        options: Vec::new(),
                    })
                    .collect();
                listed += entries.len();
                groups.push((heading, indent, entries));
            }
        }
        Ok(Availability::Unavailable { reason }) => {
            warn(&format!("the model's figures cannot be listed: {reason}"));
        }
        Ok(Availability::NoTemplate) => {}
        Err(error) => warn(&format!("the model's figures cannot be listed: {error}")),
    }
    let widest = groups
        .iter()
        .flat_map(|(_, _, entries)| entries.iter())
        .map(|entry| render::width(&entry.name))
        .max()
        .unwrap_or(0);
    for (heading, indent, entries) in &groups {
        if let Some(heading) = heading {
            outln!("{heading}");
        }
        for line in listed_lines(entries, widest) {
            outln!("{indent}{line}");
        }
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

/// A figure or a script as a listing writes it: its name, what it acts on,
/// the lines that describe it, and its options as `--set` gives them.
pub(super) struct Listed {
    pub(super) name: String,
    pub(super) scope: String,
    pub(super) lines: Vec<String>,
    /// Each `--set NAME=TYPE` and what it is by default.
    pub(super) options: Vec<(String, String)>,
}

/// What a figure draws or a script acts on, in the listing's words.
pub(super) fn acts_on(draws: runtime::Draws) -> &'static str {
    match draws {
        runtime::Draws::Sample => "on each sample",
        runtime::Draws::Collection => "on the selection",
    }
}

/// The listing's lines: each name alone, what it acts on in one column after
/// the longest, its description and options indented beneath.
pub(super) fn listed_lines(entries: &[Listed], widest: usize) -> Vec<String> {
    let widest = entries
        .iter()
        .map(|entry| render::width(&entry.name))
        .max()
        .unwrap_or(0)
        .max(widest);
    let option_width = entries
        .iter()
        .flat_map(|entry| entry.options.iter())
        .map(|(option, _)| render::width(option))
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    for entry in entries {
        out.push(format!(
            "{}{}   {}",
            entry.name,
            " ".repeat(widest - render::width(&entry.name)),
            entry.scope
        ));
        for line in &entry.lines {
            out.push(format!("    {line}"));
        }
        for (option, said) in &entry.options {
            out.push(format!(
                "    {option}{}   {said}",
                " ".repeat(option_width - render::width(option))
            ));
        }
    }
    out
}

/// Each axis one project gives a unit and another, holding the field, none:
/// `radius is in SG in alpha, and in no unit in gamma`.
fn unit_against_none(
    collection: &SampleList,
    selected: &SampleList,
    axes: &samplekit::config::project_config::Profile,
) -> Vec<String> {
    if !collection.spans_configurations() {
        return Vec::new();
    }
    let vocabulary = selected.vocabulary();
    // By field: the projects with its unit, and those holding it without one.
    let mut with: Vec<(String, Vec<String>)> = Vec::new();
    let mut without: Vec<(String, Vec<String>)> = Vec::new();
    for part in collection.by_configuration(selected) {
        let title = configuration_title(part.file.as_deref());
        let guards: Vec<_> = part
            .samples
            .iter()
            .map(|entry| entry.sample.borrow())
            .collect();
        let subjects: Vec<Subject> = part
            .samples
            .iter()
            .zip(guards.iter())
            .map(|(entry, guard)| Subject {
                sample: guard,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            })
            .collect();
        let units = render::units_of(axes, &subjects, part.config, None);
        for column in axes.columns() {
            let Ok(Field::Named { name, .. }) = fields::parse(&column.field) else {
                continue;
            };
            let field = name.to_string();
            if let Some((_, spellings)) = units
                .iter()
                .find(|(held, spellings)| *held == field && !spellings.is_empty())
            {
                let said = format!("{} in {title}", spellings.join(" and "));
                match with.iter_mut().find(|(held, _)| *held == field) {
                    Some((_, list)) => list.push(said),
                    None => with.push((field, vec![said])),
                }
            } else if guards.iter().any(|sample| sample.has_property(&name)) {
                match without.iter_mut().find(|(held, _)| *held == field) {
                    Some((_, list)) => list.push(title.clone()),
                    None => without.push((field, vec![title.clone()])),
                }
            }
        }
    }
    with.into_iter()
        .filter_map(|(field, units)| {
            let none = without.iter().find(|(held, _)| *held == field)?;
            Some(format!(
                "{field} is in {}, and in no unit in {}",
                units.join(", "),
                none.1.join(", ")
            ))
        })
        .collect()
}

/// Each axis's field and its unit's spellings.
type Spellings = Vec<(String, Vec<String>)>;

/// Over several projects, the first named draws: its Python, its spellings
/// of the units. Said once where another project declares others, so that
/// naming the projects in another order is no surprise.
fn declared_otherwise(
    collection: &SampleList,
    selected: &SampleList,
    axes: &samplekit::config::project_config::Profile,
) -> Option<String> {
    if !collection.spans_configurations() {
        return None;
    }
    let vocabulary = selected.vocabulary();
    let mut first: Option<(String, Option<PathBuf>, Spellings)> = None;
    let mut others: Vec<String> = Vec::new();
    for part in collection.by_configuration(selected) {
        if part.samples.is_empty() {
            continue;
        }
        let title = configuration_title(part.file.as_deref());
        let guards: Vec<_> = part
            .samples
            .iter()
            .map(|entry| entry.sample.borrow())
            .collect();
        let subjects: Vec<Subject> = part
            .samples
            .iter()
            .zip(guards.iter())
            .map(|(entry, guard)| Subject {
                sample: guard,
                path: entry.path.as_deref(),
                vocabulary: &vocabulary,
                states: entry.states.as_deref(),
            })
            .collect();
        let units = render::units_of(axes, &subjects, part.config, Some("figure"));
        let python = part
            .config
            .and_then(|config| runtime::figure_interpreter(config).ok());
        match &first {
            None => first = Some((title, python, units)),
            Some((_, first_python, first_units)) => {
                if python != *first_python || units != *first_units {
                    others.push(title);
                }
            }
        }
    }
    let (title, _, _) = first?;
    (!others.is_empty()).then(|| {
        format!(
            "drawn with the Python and the unit spellings of {title}, the first project named; \
             {} declares others",
            others.join(", ")
        )
    })
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
    // Projects of the same model — its file and its class — are one model's
    // samples, drawn together.
    let model = |config: Option<&ProjectConfig>| {
        config.and_then(runtime::template_of).map(|template| {
            (
                dunce::canonicalize(template.path())
                    .unwrap_or_else(|_| template.path().to_path_buf()),
                template.class().map(str::to_string),
            )
        })
    };
    let own_model = model(collection.config());
    let alike: Vec<usize> = parts
        .iter()
        .enumerate()
        .filter(|(_, part)| {
            part.config.map(|config| config.root().to_path_buf()) == own
                || (own_model.is_some() && model(part.config) == own_model)
        })
        .map(|(at, _)| at)
        .collect();

    // Several projects' are one line, each under -v.
    let mut not_drawn: Vec<String> = Vec::new();
    let mut samples = 0;
    for (at, part) in parts.iter().enumerate() {
        if alike.contains(&at) || part.samples.is_empty() {
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
    let kept: Vec<PathBuf> = parts
        .iter()
        .enumerate()
        .filter(|(at, _)| alike.contains(at))
        .flat_map(|(_, part)| {
            part.samples
                .iter()
                .filter_map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
        })
        .collect();
    Ok(
        selected
            .filter_entries(|entry| entry.path.as_ref().is_some_and(|path| kept.contains(path))),
    )
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
