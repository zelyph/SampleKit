//! The tests of `plotting`.
//!
//! A figure is drawn by matplotlib in the project's Python, so these tests run
//! the **compiled binary** against the repository's `.venv`, where `maturin
//! develop` installed the package and matplotlib beside it. Every figure is
//! written to a file: a test opens no window.

// Runs a project's Python through Unix paths and signals.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use samplekit::config::project_config::{FigureDeclaration, FigureKind};
use samplekit::presentation::plotting::{
    FigureChoice, FigureOverrides, FigureRequest, request_json,
};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("samplekit-plot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Scratch(dunce::canonicalize(&path).unwrap())
    }

    fn at(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    fn write(&self, relative: &str, body: &str) -> PathBuf {
        let path = self.at(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The repository's environment, with the package and matplotlib in it.
fn venv() -> PathBuf {
    let venv = Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    let python = venv.join("bin").join("python");
    assert!(
        python.is_file(),
        "these tests need the repository's .venv, with the package built by `maturin develop`"
    );
    let found = Command::new(&python)
        .args(["-c", "import matplotlib, samplekit._figure"])
        .output()
        .expect("the environment's python runs");
    assert!(
        found.status.success(),
        "these tests need matplotlib and the package in .venv: {}",
        String::from_utf8_lossy(&found.stderr)
    );
    venv
}

/// A model whose import leaves a mark, with a figure of one sample, one of the
/// collection, and one that raises.
const MODEL: &str = r#"import os

import samplekit as sk

open(os.path.join(os.path.dirname(__file__), "imported"), "w").close()
print("loading the adjustment")


class Keg(sk.Sample):
    @sk.figure
    def mass_bar(self, ax):
        ax.bar(["malt"], [self.malt.value])

    @sk.figure(subplots=(1, 2))
    @classmethod
    def overview(cls, samples, axes):
        names = [sample.name for sample in samples]
        axes[0].bar(names, [sample.malt.value for sample in samples])
        axes[1].set_title(",".join(names))
        open(os.path.join(os.path.dirname(__file__), "drew"), "w").write(",".join(names))

    @sk.figure
    def broken(self, ax):
        return 1 / 0
"#;

const RC: &str = "schema_version = 1\n[model]\npath = \"model.py\"\n\
[figure.malt_volume]\nx = \"volume\"\ny = \"malt\"\ngroup = \"beer\"\n\
[figure.schwarz]\nx = \"volume\"\ny = \"malt\"\nquery = \"schwarz\"\n\
[query.schwarz]\nfilter = \"beer == schwarz\"\n\
[style.figure]\n[style.math]\n\
[unit.g]\nfigure = '$\\mathrm{g}$'\nmath = '\\si{\\gram}'\n\
[property.malt]\nsymbol = \"m\"\nsymbol_figure = '$m$'\n";

fn project(scratch: &Scratch) -> PathBuf {
    scratch.write("project/.samplekitrc", RC);
    scratch.write("project/model.py", MODEL);
    for (name, malt, beer, ebc) in [("c1", 12.0, "schwarz", 5.1), ("c2", 10.0, "bock", 5.3)] {
        scratch.write(
            &format!("project/samples/{name}.md"),
            &format!(
                "---\nschema_version: 1\nname: {}\nbeer: {beer}\nproperties:\n  \
                 malt: {{v: {malt:?}, u: 0.1, unit: g}}\n  volume: {{v: 4.0, unit: L}}\n\
                 tables:\n  m:\n    index: T\n    columns:\n      T: {{unit: C}}\n      \
                 ebc: {{}}\n    rows:\n      - {{T: 20.0, ebc: {{v: {ebc}, u: 0.05}}}}\n---\n",
                name.to_uppercase()
            ),
        );
    }
    std::os::unix::fs::symlink(venv(), scratch.at("project/.venv")).unwrap();
    scratch.at("project")
}

/// The binary, in the project, with nobody at a terminal and no display over
/// SSH unless a test says so.
fn run(scratch: &Scratch, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("NO_COLOR", "1")
        .env("MPLBACKEND", "Agg")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("the binary runs")
}

fn code(output: &Output) -> i32 {
    output.status.code().expect("an exit code, not a signal")
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn starts_with(path: &Path, magic: &[u8]) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes.starts_with(magic))
}

#[test]
fn a_declared_figure_is_written_to_a_file() {
    let scratch = Scratch::new("declared");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "out.png", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.png"), b"\x89PNG"));
    // A declared figure runs no code of the project's.
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn an_output_is_previewed_until_write() {
    let scratch = Scratch::new("preview");
    project(&scratch);
    // An interpreter that cannot start: a preview must not need one.
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let output = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "out.png"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(out(&output).contains("out.png"), "{}", out(&output));
    assert!(out(&output).contains("--write"), "{}", out(&output));
    assert!(!scratch.at("project/out.png").exists());
}

#[test]
fn axes_given_on_the_command_line_draw_without_a_declaration() {
    let scratch = Scratch::new("axes");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "plot", "-x", "volume", "-y", "malt", "--kind", "bar", "--group", "beer", "samples",
            "-o", "out.svg", "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.svg"), b"<?xml"));
}

#[test]
fn a_model_figure_draws_one_sample() {
    let scratch = Scratch::new("model-one");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "plot",
            "mass_bar",
            "samples/c1.md",
            "-o",
            "one.pdf",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/one.pdf"), b"%PDF"));
}

#[test]
fn a_collection_figure_draws_the_selection() {
    let scratch = Scratch::new("model-many");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "overview", "samples", "-o", "all.png", "--write"],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    let drew = fs::read_to_string(scratch.at("project/drew")).unwrap();
    assert_eq!(drew, "C1,C2");
}

// Only Linux refuses: over SSH a Mac still has its own screen, and the figure
// would open there and wait.
#[test]
#[cfg(target_os = "linux")]
fn a_window_over_ssh_without_a_display_is_refused() {
    let scratch = Scratch::new("ssh");
    project(&scratch);
    let over_ssh = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .current_dir(scratch.at("project"))
            .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
            .env("SSH_CONNECTION", "10.0.0.2 51000 10.0.0.1 22")
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .args(arguments)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let window = over_ssh(&["plot", "malt_volume", "samples"]);
    assert_eq!(code(&window), 1, "{}", err(&window));
    assert!(err(&window).contains("SSH"), "{}", err(&window));
    assert!(err(&window).contains("-o"), "{}", err(&window));
    let file = over_ssh(&["plot", "malt_volume", "samples", "-o", "ssh.png", "--write"]);
    assert_eq!(code(&file), 0, "{}", err(&file));
    assert!(starts_with(&scratch.at("project/ssh.png"), b"\x89PNG"));
}

#[test]
fn an_unknown_figure_names_those_that_exist() {
    let scratch = Scratch::new("unknown");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "malt_volum", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    for name in ["malt_volume", "mass_bar", "overview"] {
        assert!(err(&output).contains(name), "{}", err(&output));
    }
}

#[test]
fn a_figure_that_raises_exits_two_with_its_traceback() {
    let scratch = Scratch::new("raises");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "broken", "samples/c1.md", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("ZeroDivisionError"),
        "{}",
        err(&output)
    );
    assert!(err(&output).contains("model.py"), "{}", err(&output));
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn a_one_sample_figure_over_several_with_an_output_is_refused() {
    let scratch = Scratch::new("one-over-several");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "mass_bar", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("one sample"), "{}", err(&output));
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn list_figures_names_the_model_figures_where_it_may_run() {
    // Nothing to list is said, not printed as nothing.
    let empty = Scratch::new("list-empty");
    empty.write("project/.samplekitrc", "schema_version = 1\n");
    empty.write(
        "project/samples/c1.md",
        "---\nschema_version: 1\nname: C1\nproperties:\n  malt: 1.0\n---\n",
    );
    let said = run(&empty, &["list", "figures", "samples"]);
    assert_eq!(code(&said), 0, "{}", err(&said));
    assert!(
        out(&said).contains("no figure is declared"),
        "{}",
        out(&said)
    );

    let scratch = Scratch::new("list");
    project(&scratch);
    // The model is read to list its figures, nothing asked.
    let after = run(&scratch, &["list", "figures", "samples"]);
    assert_eq!(code(&after), 0, "{}", err(&after));
    for name in ["malt_volume", "mass_bar", "overview", "broken"] {
        assert!(out(&after).contains(name), "{}", out(&after));
    }
    // What the model prints as it loads is not a figure.
    assert!(!out(&after).contains("adjustment"), "{}", out(&after));
}

#[test]
fn the_request_is_one_json_object() {
    let named = FigureRequest {
        choice: FigureChoice::Named {
            name: "vfi".to_string(),
            model: false,
        },
        samples: vec![PathBuf::from("a.md"), PathBuf::from("b.md")],
        output: Some(PathBuf::from("vfi.pdf")),
        overrides: FigureOverrides {
            title: Some("VFI".to_string()),
            style: Some("figure".to_string()),
            ..FigureOverrides::default()
        },
        project_style: true,
        said: None,
        snapshot: None,
    };
    let body: serde_json::Value = serde_json::from_str(&request_json(&named)).unwrap();
    assert_eq!(body["figure"], "vfi");
    assert_eq!(body["model"], false);
    assert_eq!(body["samples"], serde_json::json!(["a.md", "b.md"]));
    assert_eq!(body["output"], "vfi.pdf");
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["overrides"]["title"], "VFI");
    assert_eq!(body["overrides"]["style"], "figure");
    assert!(body["overrides"]["x_label"].is_null());
    assert!(body["overrides"]["group"].is_null());
    assert_eq!(body["project_style"], true);
    let axes = FigureRequest {
        choice: FigureChoice::AdHoc(Box::new(FigureDeclaration {
            name: String::new(),
            kind: FigureKind::Line,
            x: "period".to_string(),
            y: "vfi".to_string(),
            group: Some("beer".to_string()),
            query: None,
            title: None,
            x_label: None,
            y_label: None,
            style: None,
            x_limits: None,
            y_limits: None,
            x_scale: None,
            y_scale: None,
            aspect: None,
            legend: None,
            figsize: None,
        })),
        samples: Vec::new(),
        output: None,
        overrides: FigureOverrides::default(),
        project_style: false,
        said: None,
        snapshot: None,
    };
    let body: serde_json::Value = serde_json::from_str(&request_json(&axes)).unwrap();
    assert_eq!(body["axes"]["kind"], "line");
    assert_eq!(body["axes"]["group"], "beer");
    assert_eq!(body["model"], false);
    assert!(body["output"].is_null());
}

#[test]
fn a_table_cell_is_an_axis_across_samples() {
    // A cell is a field like any other, a point per sample.
    let scratch = Scratch::new("cell");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "plot",
            "-x",
            "malt",
            "-y",
            "m.ebc[20]",
            "samples",
            "-o",
            "cell.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/cell.png"), b"\x89PNG"));
    let nowhere = run(
        &scratch,
        &[
            "plot",
            "-x",
            "malt",
            "-y",
            "m.ebc[99]",
            "samples",
            "-o",
            "x.png",
            "--write",
        ],
    );
    assert_eq!(code(&nowhere), 1, "{}", err(&nowhere));
    assert!(!scratch.at("project/x.png").exists());
    let misspelt = run(
        &scratch,
        &[
            "plot", "-x", "mal", "-y", "volume", "samples", "-o", "x.png", "--write",
        ],
    );
    assert_eq!(code(&misspelt), 1, "{}", err(&misspelt));
    assert!(err(&misspelt).contains("malt"), "{}", err(&misspelt));
}

#[test]
fn a_title_labels_and_a_style_are_given_on_the_command_line() {
    let scratch = Scratch::new("labels");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--title",
            "Malts",
            "--x-label",
            "$V$",
            "--style",
            "figure",
            "-o",
            "l.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/l.png"), b"\x89PNG"));
    let unknown = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--style",
            "figur",
            "-o",
            "u.png",
            "--write",
        ],
    );
    assert_eq!(code(&unknown), 1, "{}", err(&unknown));
    assert!(err(&unknown).contains("figure"), "{}", err(&unknown));
    let on_the_model = run(
        &scratch,
        &[
            "plot",
            "mass_bar",
            "samples/c1.md",
            "--x-label",
            "V",
            "-o",
            "m.png",
            "--write",
        ],
    );
    assert_eq!(code(&on_the_model), 1, "{}", err(&on_the_model));
    assert!(
        err(&on_the_model).contains("its own axes"),
        "{}",
        err(&on_the_model)
    );
}

#[test]
fn a_label_matplotlib_cannot_typeset_is_refused() {
    // The math style's unit is siunitx, which matplotlib writes out as it is.
    let scratch = Scratch::new("typeset");
    project(&scratch);
    let refused = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--style",
            "math",
            "-o",
            "t.png",
            "--write",
        ],
    );
    assert_eq!(code(&refused), 1, "{}", err(&refused));
    assert!(err(&refused).contains("\\si{\\gram}"), "{}", err(&refused));
    assert!(!scratch.at("project/t.png").exists());
}

#[test]
fn axes_complete_as_columns_do() {
    let scratch = Scratch::new("complete");
    project(&scratch);
    let complete = |words: &[&str]| {
        let mut arguments = vec!["--", "samplekit", "plot"];
        arguments.extend_from_slice(words);
        let output = Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(&arguments)
            .current_dir(scratch.at("project/samples"))
            .env("COMPLETE", "zsh")
            .env("_CLAP_COMPLETE_INDEX", (arguments.len() - 2).to_string())
            .env("_CLAP_IFS", "\n")
            .output()
            .unwrap();
        out(&output)
    };
    assert!(complete(&["-x", "vol"]).contains("volume"));
    assert!(complete(&["-y", "m.ebc["]).contains("m.ebc[20]"));
    assert!(complete(&["--group", "bee"]).contains("beer"));
    assert!(complete(&["--style", "fig"]).contains("figure"));
    // A figure's name: the declared ones, and the model's read from its
    // source, which completion never imports.
    let figures = complete(&["m"]);
    for name in ["malt_volume", "mass_bar"] {
        assert!(figures.contains(name), "{figures}");
    }
    assert!(complete(&["o"]).contains("overview"));
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn a_group_and_a_kind_replace_a_declared_figures_own() {
    // `--group` and `--kind` change a declared figure as `--title` does, and
    // are refused for a model's, which draws its own axes.
    let scratch = Scratch::new("override");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--group",
            "name",
            "--kind",
            "bar",
            "-o",
            "g.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/g.png"), b"\x89PNG"));
    let on_the_model = run(
        &scratch,
        &[
            "plot",
            "mass_bar",
            "samples/c1.md",
            "--group",
            "beer",
            "-o",
            "m.png",
            "--write",
        ],
    );
    assert_eq!(code(&on_the_model), 1, "{}", err(&on_the_model));
    assert!(
        err(&on_the_model).contains("--group"),
        "{}",
        err(&on_the_model)
    );
}

#[test]
fn no_project_style_sets_the_projects_settings_aside() {
    // Read with the project, from any directory; set aside on request.
    let scratch = Scratch::new("project-style");
    project(&scratch);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        // Centimetres, as every size SampleKit takes: 7 inches wide.
        &format!("{rc}[matplotlib]\n\"figure.figsize\" = [17.78, 12.7]\n\"figure.dpi\" = 100\n"),
    );
    let width = |name: &str| {
        let bytes = fs::read(scratch.at(&format!("project/samples/{name}"))).unwrap();
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]])
    };
    let inside = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project/samples"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("MPLBACKEND", "Agg")
        .args(["plot", "malt_volume", ".", "-o", "styled.png", "--write"])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&inside), 0, "{}", err(&inside));
    assert_eq!(width("styled.png"), 700);
    let aside = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .current_dir(scratch.at("project/samples"))
        .env("SAMPLEKIT_STATE_DIR", scratch.at("state"))
        .env("MPLBACKEND", "Agg")
        .args([
            "plot",
            "malt_volume",
            ".",
            "--no-project-style",
            "-o",
            "plain.png",
            "--write",
        ])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(code(&aside), 0, "{}", err(&aside));
    assert_eq!(width("plain.png"), 640);
}

#[test]
fn limits_scales_and_aspect_are_given_on_the_command_line() {
    // Replacing a declared figure's own, and refused where they cannot apply.
    let scratch = Scratch::new("axes-settings");
    project(&scratch);
    let drawn = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--x-limits",
            "0,auto",
            "--y-scale",
            "log",
            "--aspect",
            "equal",
            "-o",
            "a.png",
            "--write",
        ],
    );
    assert_eq!(code(&drawn), 0, "{}", err(&drawn));
    assert!(starts_with(&scratch.at("project/a.png"), b"\x89PNG"));
    let unreadable = run(
        &scratch,
        &["plot", "malt_volume", "samples", "--x-limits", "0,cent"],
    );
    assert_eq!(code(&unreadable), 1, "{}", err(&unreadable));
    assert!(err(&unreadable).contains("'cent'"), "{}", err(&unreadable));
    let at_zero = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--y-limits",
            "0,20",
            "--y-scale",
            "log",
            "-o",
            "z.png",
            "--write",
        ],
    );
    assert_eq!(code(&at_zero), 1, "{}", err(&at_zero));
    assert!(
        err(&at_zero).contains("at or below zero"),
        "{}",
        err(&at_zero)
    );
}

#[test]
fn a_preview_refuses_what_write_would() {
    // A preview that approves what --write then refuses reads as approval.
    let scratch = Scratch::new("preview-refuses");
    project(&scratch);
    for arguments in [
        &[
            "plot",
            "malt_volume",
            "samples",
            "-o",
            "x.png",
            "--style",
            "nope",
        ][..],
        &[
            "plot",
            "mass_bar",
            "samples",
            "-o",
            "x.png",
            "--x-label",
            "m",
        ][..],
        &["plot", "malt_volume", "samples", "-o", "noext"][..],
        &["plot", "malt_volume", "samples", "-o", "-"][..],
        &["plot", "malt_volume", "samples", "-o", "x.xyz"][..],
    ] {
        let output = run(&scratch, arguments);
        assert_eq!(code(&output), 1, "{arguments:?}: {}", out(&output));
        assert!(
            !out(&output).contains("--write to apply"),
            "{}",
            out(&output)
        );
    }
    // matplotlib would have written noext.png, and said nothing.
    let written = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "noext", "--write"],
    );
    assert_eq!(code(&written), 1, "{}", err(&written));
    assert!(!scratch.at("project/noext.png").exists());

    let missing = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "figs/x.png"],
    );
    assert_eq!(code(&missing), 0, "{}", err(&missing));
    assert!(
        out(&missing).contains("figs does not exist"),
        "{}",
        out(&missing)
    );
}

#[test]
fn an_output_makes_its_folder() {
    // As `export` makes its own: the paper's how-to began with mkdir.
    let scratch = Scratch::new("output-folder");
    project(&scratch);
    let preview = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "figs/x.png"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("figs does not exist, and --write creates it"),
        "{}",
        out(&preview)
    );
    assert!(!scratch.at("project/figs").exists());
    let written = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "-o",
            "figs/x.png",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    assert!(starts_with(&scratch.at("project/figs/x.png"), b"\x89PNG"));
}

#[test]
fn a_declared_figures_query_is_counted_by_the_preview() {
    let scratch = Scratch::new("preview-query");
    project(&scratch);
    let output = run(&scratch, &["plot", "schwarz", "samples", "-o", "x.png"]);
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a figure of 1 sample"),
        "{}",
        out(&output)
    );
}

#[test]
fn an_unknown_group_is_refused_naming_the_nearest() {
    let scratch = Scratch::new("group");
    project(&scratch);
    for arguments in [
        &["plot", "-x", "volume", "-y", "malt", "--group", "beet"][..],
        &["plot", "malt_volume", "--group", "beet"][..],
    ] {
        let mut arguments = arguments.to_vec();
        arguments.extend(["samples", "-o", "x.png", "--write"]);
        let output = run(&scratch, &arguments);
        assert_eq!(code(&output), 1, "{}", err(&output));
        assert!(
            err(&output).contains("did you mean: beer"),
            "{}",
            err(&output)
        );
    }
    assert!(!scratch.at("project/x.png").exists());
}

#[test]
fn a_figures_name_beside_axes_is_a_usage_error() {
    let scratch = Scratch::new("name-and-axes");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "-x",
            "volume",
            "-y",
            "malt",
            "samples",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("not both"), "{}", err(&output));
}

#[test]
fn a_python_killed_by_a_signal_says_so() {
    // A crash in matplotlib's toolkit is not Ctrl-C, and is not silent.
    let scratch = Scratch::new("killed");
    project(&scratch);
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let python = scratch.write("project/.venv/bin/python", "#!/bin/sh\nkill -SEGV $$\n");
    fs::set_permissions(&python, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let output = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(err(&output).contains("signal 11"), "{}", err(&output));
}

#[test]
fn an_environment_without_samplekit_is_an_environment_failure() {
    let scratch = Scratch::new("no-samplekit");
    project(&scratch);
    fs::remove_file(scratch.at("project/.venv")).unwrap();
    let python = scratch.write(
        "project/.venv/bin/python",
        "#!/bin/sh\nexec /usr/bin/env -i PATH=/usr/bin:/bin python3 -I \"$@\"\n",
    );
    fs::set_permissions(&python, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let output = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 3, "{}", err(&output));
    assert!(
        err(&output).contains("has no samplekit"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_name_close_to_a_figures_is_offered_before_consent() {
    // A typo is not the model's figure, and the model is not loaded.
    let scratch = Scratch::new("near-name");
    project(&scratch);
    let output = run(
        &scratch,
        &["plot", "malt_volum", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(
        err(&output).contains("did you mean 'malt_volume'"),
        "{}",
        err(&output)
    );
    assert!(!scratch.at("project/imported").exists());
}

#[test]
fn a_name_declared_and_the_models_is_refused() {
    // Two figures of one name are refused, the model read as text.
    let scratch = Scratch::new("double-name");
    project(&scratch);
    let rc = std::fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &format!("{rc}[figure.mass_bar]\nx = \"volume\"\ny = \"malt\"\n"),
    );
    let output = run(
        &scratch,
        &["plot", "mass_bar", "samples", "-o", "x.png", "--write"],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("rename one"), "{}", err(&output));
    assert!(!scratch.at("project/imported").exists());
    let validated = run(&scratch, &["validate", "samples"]);
    assert!(
        out(&validated).contains("the model defines a figure of the same name"),
        "{}",
        out(&validated)
    );
}

#[test]
fn a_figure_of_one_sample_writes_a_file_per_sample() {
    // {name} in -o, one file each.
    let scratch = Scratch::new("per-sample");
    project(&scratch);
    let preview = run(
        &scratch,
        &["plot", "mass_bar", "samples", "-o", "figs/{name}.png"],
    );
    assert_eq!(code(&preview), 0, "{}", err(&preview));
    assert!(
        out(&preview).contains("a file per sample, 2 files"),
        "{}",
        out(&preview)
    );
    fs::create_dir_all(scratch.at("project/figs")).unwrap();
    let written = run(
        &scratch,
        &[
            "plot",
            "mass_bar",
            "samples",
            "-o",
            "figs/{name}.png",
            "--write",
        ],
    );
    assert_eq!(code(&written), 0, "{}", err(&written));
    for name in ["C1", "C2"] {
        assert!(starts_with(
            &scratch.at(&format!("project/figs/{name}.png")),
            b"\x89PNG"
        ));
    }
    let declared = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "figs/{name}.png"],
    );
    assert_eq!(code(&declared), 1, "{}", err(&declared));
}

#[test]
fn a_legend_a_size_and_a_box_are_given_on_the_command_line() {
    let scratch = Scratch::new("legend-size-box");
    project(&scratch);
    let sized = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--legend",
            "outside",
            "--figsize",
            // Centimetres: 4 by 3 inches, 400 by 300 pixels.
            "10.16,7.62",
            "-o",
            "s.png",
            "--write",
        ],
    );
    assert_eq!(code(&sized), 0, "{}", err(&sized));
    let png = fs::read(scratch.at("project/s.png")).unwrap();
    let width = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!((width, height), (400, 300));
    let boxed = run(
        &scratch,
        &[
            "plot", "-x", "beer", "-y", "malt", "--kind", "box", "samples", "-o", "b.png",
            "--write",
        ],
    );
    assert_eq!(code(&boxed), 0, "{}", err(&boxed));
    let bad = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--figsize",
            "4",
            "-o",
            "x.png",
        ],
    );
    assert_eq!(code(&bad), 1, "{}", err(&bad));
}

#[test]
fn a_figure_says_what_it_leaves_out() {
    let scratch = Scratch::new("left-out");
    project(&scratch);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nproperties:\n  malt: {v: 1.0, unit: g}\n---\n",
    );
    let output = run(
        &scratch,
        &[
            "plot", "-x", "volume", "-y", "malt", "samples", "-o", "x.png", "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        err(&output).contains("warning: 1 of 3 samples not drawn: no volume"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_figure_over_two_projects_draws_them_both() {
    // Axes given draw every sample, whatever project; a
    // declared figure needs every project to declare it alike.
    let scratch = Scratch::new("two-projects");
    project(&scratch);
    let rc = fs::read_to_string(scratch.at("project/.samplekitrc")).unwrap();
    scratch.write(
        "project/.samplekitrc",
        &rc.replace(
            "schema_version = 1\n",
            "schema_version = 1\n[collection]\nrecursive = true\n",
        ),
    );
    scratch.write(
        "project/samples/nested/.samplekitrc",
        "schema_version = 1\n",
    );
    scratch.write(
        "project/samples/nested/n1.md",
        "---\nschema_version: 1\nname: N1\nbeer: tin\nproperties:\n  malt: {v: 3.0, unit: g}\n  \
         volume: {v: 1.0, unit: L}\n---\n",
    );
    let axes = run(
        &scratch,
        &[
            "plot", "-x", "volume", "-y", "malt", "samples", "-o", "a.png", "--write",
        ],
    );
    assert_eq!(code(&axes), 0, "{}", err(&axes));
    assert!(!err(&axes).contains("not drawn"), "{}", err(&axes));
    assert!(starts_with(&scratch.at("project/a.png"), b"\x89PNG"));
    // The nested project does not declare `malt_volume`: refused, naming it.
    let declared = run(
        &scratch,
        &["plot", "malt_volume", "samples", "-o", "d.png", "--write"],
    );
    assert_ne!(code(&declared), 0, "{}", err(&declared));
    assert!(
        err(&declared).contains("does not declare it"),
        "{}",
        err(&declared)
    );
    assert!(!scratch.at("project/d.png").exists());
}

#[test]
fn a_figure_written_is_found_again_and_carries_its_snapshot() {
    // The file tied to the snapshot it was made from, by its hash; a copy
    // changed since still names that snapshot in its metadata.
    let scratch = Scratch::new("figure-history");
    project(&scratch);
    for (file, marker) in [("out.svg", true), ("out.pdf", true), ("out.png", true)] {
        let output = run(
            &scratch,
            &["plot", "malt_volume", "samples", "-o", file, "--write"],
        );
        assert_eq!(code(&output), 0, "{}", err(&output));
        let bytes = fs::read(scratch.at(&format!("project/{file}"))).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.contains("samplekit snapshot "), marker, "{file}");
    }
    let explained = run(&scratch, &["explain", "out.svg"]);
    assert_eq!(code(&explained), 0, "{}", err(&explained));
    assert!(
        out(&explained).contains("by samplekit plot malt_volume samples -o out.svg --write"),
        "{}",
        out(&explained)
    );
    let svg = scratch.at("project/out.svg");
    let edited = fs::read_to_string(&svg)
        .unwrap()
        .replace("</svg>", "<!-- edited --></svg>");
    fs::write(scratch.at("project/edited.svg"), edited).unwrap();
    let carried = run(&scratch, &["explain", "edited.svg"]);
    assert_eq!(code(&carried), 0, "{}", err(&carried));
    assert!(
        out(&carried).contains("carries the snapshot it was made from"),
        "{}",
        out(&carried)
    );
}

#[test]
fn a_figure_is_drawn_again_as_it_was() {
    // The figure of the project as a snapshot kept it, carrying that snapshot,
    // recorded in the history against it.
    let scratch = Scratch::new("figure-at");
    project(&scratch);
    let first = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "-o",
            "first.svg",
            "--write",
        ],
    );
    assert_eq!(code(&first), 0, "{}", err(&first));
    let set = run(&scratch, &["set", "samples/c1.md", "malt=15", "--write"]);
    assert_eq!(code(&set), 0, "{}", err(&set));
    let log = out(&run(&scratch, &["log"]));
    assert!(log.contains("samplekit set samples/c1.md malt=15"), "{log}");
    let then = run(
        &scratch,
        &[
            "plot",
            "malt_volume",
            "samples",
            "--at",
            "2",
            "-o",
            "then.svg",
            "--write",
        ],
    );
    assert_eq!(code(&then), 0, "{}", err(&then));
    let drawn = fs::read_to_string(scratch.at("project/then.svg")).unwrap();
    let carried = drawn
        .split("samplekit snapshot ")
        .nth(1)
        .map(|rest| rest[..40].to_string())
        .unwrap();
    let first_drawn = fs::read_to_string(scratch.at("project/first.svg")).unwrap();
    assert!(
        first_drawn.contains(&carried),
        "drawn from the snapshot the first was"
    );
    let explained = out(&run(&scratch, &["explain", "then.svg"]));
    assert!(
        explained.contains("made from the project as it was then, on purpose"),
        "{explained}"
    );
}

#[test]
fn a_preview_says_which_samples_will_not_be_drawn() {
    // The warning came with --write only; the preview said all were drawn.
    let scratch = Scratch::new("preview-left-out");
    project(&scratch);
    scratch.write(
        "project/samples/c3.md",
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nproperties:\n  volume: {v: 4.0, unit: L}\n---\n",
    );
    let output = run(
        &scratch,
        &[
            "plot", "-x", "volume", "-y", "malt", "samples", "-o", "out.png",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(
        out(&output).contains("a figure of 2 of 3 samples — 1 has no malt"),
        "{}",
        out(&output)
    );
}

#[cfg(unix)]
#[test]
fn a_termination_reaches_the_drawing_and_ends_the_command() {
    // SIGTERM or SIGHUP to `plot`, a window open, did nothing: the handler
    // taking them was empty, and the drawing went on. An interpreter that
    // sleeps stands in for a window, so this needs no environment.
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};
    let scratch = Scratch::new("terminated");
    scratch.write(".samplekitrc", "schema_version = 1\n");
    scratch.write(
        "a.md",
        "---\nschema_version: 1\nname: a\nproperties:\n  m: {v: 1.0}\n---\n",
    );
    let python = scratch.write(".venv/bin/python", "#!/bin/sh\nexec sleep 30\n");
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    let mut plot = Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["plot", "-x", "m", "-y", "m", "."])
        .current_dir(&scratch.0)
        .env_remove("SSH_CONNECTION")
        .env_remove("SSH_TTY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    // Still drawing: what ends it below is the signal, not a refusal.
    assert!(plot.try_wait().unwrap().is_none(), "plot ended by itself");
    Command::new("kill")
        .args(["-TERM", &plot.id().to_string()])
        .status()
        .unwrap();
    let sent = Instant::now();
    loop {
        if let Some(status) = plot.try_wait().unwrap() {
            assert!(!status.success(), "{status:?}");
            break;
        }
        if sent.elapsed() > Duration::from_secs(10) {
            let _ = plot.kill();
            panic!("plot was still drawing ten seconds after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn an_empty_selection_is_refused() {
    // A figure of nothing is refused, as Python refuses it: it said *nothing
    // drawn* and exited 0.
    let scratch = Scratch::new("empty-selection");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "plot",
            "-x",
            "volume",
            "-y",
            "malt",
            "samples",
            "-f",
            "beer == gold",
            "-o",
            "out.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("no sample is selected: nothing to draw"),
        "{}",
        err(&output)
    );
    assert!(!scratch.at("project/out.png").exists());
    // A declared figure's query that keeps nothing is named.
    let output = run(
        &scratch,
        &["plot", "schwarz", "samples", "-f", "beer == bock"],
    );
    assert_eq!(code(&output), 2, "{}", err(&output));
    assert!(
        err(&output).contains("(the figure's query is 'schwarz')"),
        "{}",
        err(&output)
    );
}

#[test]
fn a_group_over_several_fields_draws_their_combinations() {
    // `--group beer,name`: a series per combination of the two.
    let scratch = Scratch::new("several-groups");
    project(&scratch);
    let output = run(
        &scratch,
        &[
            "plot",
            "-x",
            "volume",
            "-y",
            "malt",
            "--group",
            "beer,name",
            "samples",
            "-o",
            "out.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 0, "{}", err(&output));
    assert!(starts_with(&scratch.at("project/out.png"), b"\x89PNG"));
    // Each field is checked: a misspelt one is refused, naming the nearest.
    let output = run(
        &scratch,
        &[
            "plot",
            "-x",
            "volume",
            "-y",
            "malt",
            "--group",
            "beer,nmae",
            "samples",
            "-o",
            "bad.png",
            "--write",
        ],
    );
    assert_eq!(code(&output), 1, "{}", err(&output));
    assert!(err(&output).contains("name"), "{}", err(&output));
    assert!(!scratch.at("project/bad.png").exists());
}
